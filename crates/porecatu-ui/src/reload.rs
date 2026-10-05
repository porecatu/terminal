// SPDX-License-Identifier: GPL-3.0-or-later

//! Hot reload do arquivo de config (F4 etapa 4, ADR-0003 regra 5,
//! ADR-0030). O watcher roda numa thread própria, nunca a main thread
//! (ADR-0007): lê e parseia o arquivo fora dela e manda pra `lib.rs`, pelo
//! mesmo `EventLoopProxy` que o PTY usa para `Wakeup`, o resultado já
//! pronto -- `Config` carregado ou erro já formatado, nunca um caminho de
//! arquivo para a main thread abrir.
//!
//! O idioma da interface (ADR-0056 §9) entra no mesmo caminho: a thread do
//! watcher guarda o último `language` que leu, monta o catálogo ali e entrega
//! o `Arc<Catalog>` pronto junto com a config, num só evento (`Reload`) --
//! uma recarga, um evento, um frame. Além da pasta da config ela assiste,
//! sem recursão, o `locales/` ao lado dela (criado e descartado conforme a
//! pasta nasce e some).
//!
//! `diff` decide o que uma recarga precisa fazer, comparando a config
//! antiga com a nova -- puro, sem `notify` nem `winit`, testável sem GPU e
//! sem janela. As classes e as chaves de cada uma são as do
//! `porecatu.example.toml`, não uma lista reinventada aqui: `diff` só
//! espelha o que já está anotado lá.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use notify::{Event, RecursiveMode, Watcher};
use porecatu_config::{Config, ConfigError, ConfigErrorKind};
use porecatu_locale::{Catalog, CatalogOutcome, Diagnostic, LocaleName};

use crate::language;
use crate::messages;

/// ADR-0003 regra 5 / ADR-0030: uma gravação pode disparar vários eventos
/// do SO (escreve, renomeia, toca mtime) -- o debounce colapsa a rajada
/// inteira numa recarga só, "um resize por recarga".
const DEBOUNCE: Duration = Duration::from_millis(200);
/// Teto de espera entre checagens do temporizador -- não é o debounce em
/// si, só garante que `Debounce::ready` seja consultado logo depois que o
/// período de silêncio termina, sem gastar CPU num loop apertado.
const POLL: Duration = Duration::from_millis(50);

/// O que a thread do watcher manda pronto para a main thread aplicar.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigReload {
    Loaded {
        // `Config` é grande (a árvore inteira de aparência) -- `Box` evita
        // que a variante `Invalid`, bem menor, pague o mesmo tamanho.
        config: Box<Config>,
        /// ADR-0003 regra 4 / RF-4.22.
        unknown_keys: Vec<String>,
    },
    /// ADR-0003 regra 2: a main thread mantém a config anterior e só
    /// mostra o erro.
    Invalid { error: ConfigError },
}

/// O que a recarga do idioma decidiu (ADR-0056 §9, RF-15.23).
#[derive(Debug, Clone, PartialEq)]
pub enum LanguageReload {
    /// O catálogo novo pode ser montado: ele vale a partir do próximo
    /// frame. `locale` é o idioma **efetivamente carregado** (a reserva
    /// `en_US` quando o pedido não foi achado, mas isso é `Keep`, não
    /// chega aqui).
    Replace {
        catalog: Arc<Catalog>,
        locale: Option<LocaleName>,
        diagnostics: Vec<Diagnostic>,
    },
    /// O catálogo novo não pôde ser montado: o em uso **continua**, e os
    /// diagnósticos dizem o que falhou.
    Keep { diagnostics: Vec<Diagnostic> },
}

/// Uma recarga inteira: config, idioma, ou os dois -- **um** evento, então
/// **um** frame, mesmo quando `language` e outra chave mudam na mesma
/// gravação.
#[derive(Debug, Clone, PartialEq)]
pub struct Reload {
    pub config: Option<ConfigReload>,
    pub language: Option<LanguageReload>,
}

/// Decide, sem tocar em disco, se a recarga precisa reconstruir o catálogo, e
/// para qual idioma. `last` é o último `language` lido da config; `config` é
/// a config que esta recarga leu (se leu); `locales_changed` diz que um
/// arquivo de `locales/` (ou a própria pasta) mudou.
///
/// - arquivo de idioma mudou: reconstrói, no idioma corrente;
/// - `language` mudou na config: reconstrói, no idioma novo;
/// - config inválida ou ausente não troca o idioma (mantém a anterior,
///   ADR-0003 regra 2);
/// - nada disso: não reconstrói -- mudar outra chave não relê arquivo de
///   idioma nenhum.
pub(crate) fn language_to_rebuild(
    last: &str,
    config: Option<&ConfigReload>,
    locales_changed: bool,
) -> Option<String> {
    let read = match config {
        Some(ConfigReload::Loaded { config, .. }) => Some(config.general.language.as_str()),
        _ => None,
    };
    let changed = read.is_some_and(|language| language != last);
    (locales_changed || changed).then(|| read.unwrap_or(last).to_owned())
}

/// `true` quando o resultado do resolvedor serve de catálogo **novo** numa
/// troca ao vivo. Não serve: o idioma pedido não foi achado (sobraria a
/// reserva `en_US`, um rebaixamento silencioso), o nome é inválido, não há
/// catálogo nenhum, ou todas as camadas do idioma têm erro e não sobrou
/// frase alguma -- é o "sintaxe quebrada na única camada". Camada ruim com
/// outra válida do mesmo idioma **serve**: o resolvedor já derrubou só a
/// ruim, e o erro segue nos diagnósticos.
fn is_usable(outcome: &CatalogOutcome, requested: &str, schema_len: usize) -> bool {
    let diagnostics = &outcome.diagnostics;
    let unusable = diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic,
            Diagnostic::LanguageNotFound { requested: name, .. } if name == requested
        ) || matches!(
            diagnostic,
            Diagnostic::InvalidLanguageName { .. } | Diagnostic::NoCatalogAtAll { .. }
        )
    });
    if unusable {
        return false;
    }
    let layer_broken = diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic,
            Diagnostic::LayerSyntax { .. } | Diagnostic::LayerUnreadable { .. }
        )
    });
    let nothing_left = diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic,
            Diagnostic::MissingMessages { locale, count }
                if Some(locale) == outcome.locale.as_ref() && *count >= schema_len
        )
    });
    !(layer_broken && nothing_left)
}

/// Transforma o resultado do resolvedor em o que a main thread aplica.
pub(crate) fn settle_language(
    outcome: CatalogOutcome,
    requested: &str,
    schema_len: usize,
) -> LanguageReload {
    if is_usable(&outcome, requested, schema_len) {
        LanguageReload::Replace {
            catalog: Arc::new(outcome.catalog),
            locale: outcome.locale,
            diagnostics: outcome.diagnostics,
        }
    } else {
        LanguageReload::Keep {
            diagnostics: outcome.diagnostics,
        }
    }
}

/// Monta uma recarga inteira: lê a config (se mudou), decide o idioma, e
/// junta tudo num `Reload`. Um `Reloader` por watcher; o construtor de
/// catálogo entra por parâmetro, então o teste roda sem `notify`, sem
/// dormir e sem app dir.
struct Reloader<B: Fn(&str) -> CatalogOutcome> {
    config_path: PathBuf,
    /// Último `language` lido da config -- não o do catálogo em uso: uma
    /// troca que falhou (`xx_XX`) ainda é o que a config diz, e um arquivo
    /// `locales/xx_XX.toml` que nasça depois deve ser montado com ele.
    last_language: String,
    build: B,
    schema_len: usize,
}

impl<B: Fn(&str) -> CatalogOutcome> Reloader<B> {
    fn new(config_path: PathBuf, language: String, build: B, schema_len: usize) -> Self {
        Self {
            config_path,
            last_language: language,
            build,
            schema_len,
        }
    }

    fn fire(&mut self, config_touched: bool, locales_touched: bool) -> Option<Reload> {
        let config = if config_touched {
            read_and_parse(&self.config_path)
        } else {
            None
        };
        let target = language_to_rebuild(&self.last_language, config.as_ref(), locales_touched);
        if let Some(ConfigReload::Loaded { config, .. }) = &config {
            self.last_language.clone_from(&config.general.language);
        }
        let language = target.map(|requested| {
            let outcome = (self.build)(&requested);
            settle_language(outcome, &requested, self.schema_len)
        });
        (config.is_some() || language.is_some()).then_some(Reload { config, language })
    }
}

/// O que um evento do `notify` tocou.
#[derive(Debug, Default, PartialEq, Eq)]
struct Touched {
    /// O `porecatu.toml`.
    config: bool,
    /// Um arquivo dentro de `locales/`.
    locales_file: bool,
    /// A própria pasta `locales/` (nasceu, sumiu, ou teve a entrada mexida).
    locales_dir: bool,
}

fn classify(paths: &[PathBuf], config_path: &Path, locales_dir: &Path) -> Touched {
    let mut touched = Touched::default();
    for path in paths {
        if path == config_path {
            touched.config = true;
        } else if path == locales_dir {
            touched.locales_dir = true;
        } else if path.parent() == Some(locales_dir) {
            touched.locales_file = true;
        }
    }
    touched
}

/// Estado do watch de `locales/`: assiste enquanto a pasta existe.
#[derive(Debug, Default)]
struct LocalesWatch {
    watching: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum WatchChange {
    Start,
    Stop,
}

impl LocalesWatch {
    /// `exists` é o estado da pasta agora. Devolve o que fazer para o watch
    /// acompanhá-lo, ou `None` se já acompanha.
    fn sync(&mut self, exists: bool) -> Option<WatchChange> {
        match (exists, self.watching) {
            (true, false) => {
                self.watching = true;
                Some(WatchChange::Start)
            }
            (false, true) => {
                self.watching = false;
                Some(WatchChange::Stop)
            }
            _ => None,
        }
    }
}

/// Faz o watch de `locales/` acompanhar a pasta. `true` se o estado dela
/// mudou (nasceu ou sumiu) -- o catálogo precisa ser refeito.
fn sync_locales_watch(
    watcher: &mut impl Watcher,
    state: &mut LocalesWatch,
    locales_dir: &Path,
) -> bool {
    match state.sync(locales_dir.is_dir()) {
        Some(WatchChange::Start) => {
            if watcher
                .watch(locales_dir, RecursiveMode::NonRecursive)
                .is_err()
            {
                // Sem watch, sem recarga por arquivo -- não é erro do
                // usuário, e o `config.reload` ainda relê tudo.
                state.watching = false;
                return false;
            }
            true
        }
        Some(WatchChange::Stop) => {
            // A pasta sumiu: o SO já soltou o handle, e `unwatch` pode
            // falhar por isso. Sem erro.
            let _ = watcher.unwatch(locales_dir);
            true
        }
        None => false,
    }
}

/// Estado puro do debounce, sobre `Instant` injetado -- mesmo padrão de
/// `WarningStack`/`AnimationClock`/`Hover`: testável sem dormir de
/// verdade. A espera real (`recv_timeout`) fica no loop da thread do
/// watcher, não aqui.
struct Debounce {
    deadline: Option<Instant>,
}

impl Debounce {
    fn new() -> Self {
        Self { deadline: None }
    }

    /// Um evento do `notify` chegou -- adia o disparo.
    fn notice(&mut self, now: Instant) {
        self.deadline = Some(now + DEBOUNCE);
    }

    /// `true` uma vez só, quando o período de silêncio termina; limpa o
    /// estado, então a mesma rajada não dispara duas vezes.
    fn ready(&mut self, now: Instant) -> bool {
        match self.deadline {
            Some(deadline) if now >= deadline => {
                self.deadline = None;
                true
            }
            _ => false,
        }
    }
}

/// Lê e parseia o arquivo já resolvido, com `porecatu_config::parse` --
/// mesma função que `load` usa no start, mas o caminho já é conhecido: a
/// resolução (`--config`/`PORECATU_CONFIG`/plataforma) só acontece uma
/// vez, no start.
pub(crate) fn read_and_parse(path: &Path) -> Option<ConfigReload> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        // Escrita em duas etapas (rename) pode deixar o arquivo
        // momentaneamente ausente entre o evento de remoção e o de
        // criação -- não é erro do usuário, é o próximo evento chegando.
        // Sem retry com sleep (ADR-0030): se o arquivo não voltar, também
        // não há nada de novo pra recarregar.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            return Some(ConfigReload::Invalid {
                error: ConfigError::new(ConfigErrorKind::Unreadable {
                    path: path.to_path_buf(),
                    cause: err.to_string(),
                }),
            });
        }
    };
    Some(match porecatu_config::parse(&text) {
        Ok((config, unknown_keys)) => ConfigReload::Loaded {
            config: Box::new(config),
            unknown_keys,
        },
        Err(error) => ConfigReload::Invalid { error },
    })
}

/// Inicia o watcher numa thread própria e detached -- como as threads de
/// leitura de PTY (ADR-0007), sem `join`: o processo inteiro sai junto
/// dela. `on_reload` roda **na thread do watcher**, nunca na main; quem
/// chama passa um fecho que só manda o resultado pelo `EventLoopProxy`.
/// `language` é o `general.language` com que o catálogo do arranque foi
/// montado -- o ponto de partida do "mudou?".
///
/// Assiste a pasta da config **sem** recursão e, num segundo watch também
/// sem recursão, o `locales/` ao lado dela (ADR-0056 §9): com `--config
/// ~/porecatu.toml` um watch recursivo assistiria o home inteiro. O
/// diretório de idiomas do **app** não é assistido -- só muda quando o
/// binário também muda.
///
/// `None` se o diretório do arquivo de config não existir -- degrada para
/// "sem hot reload" em vez de falhar o start (ADR-0003 regra 1: ausência
/// de config é estado válido, e o mesmo vale pro diretório dela).
pub fn watch(
    path: PathBuf,
    language: String,
    on_reload: impl Fn(Reload) + Send + 'static,
) -> Option<()> {
    let watch_dir = path.parent()?.to_path_buf();
    if !watch_dir.exists() {
        return None;
    }
    let locales_dir = language::user_dir(Some(&path))?;

    std::thread::spawn(move || {
        let (tx, rx) = channel::<Event>();
        // Assiste o DIRETÓRIO, não o arquivo: escrita em duas etapas
        // (write-then-rename, comum em editores) troca o arquivo inteiro,
        // e assistir só ele pode perder esse evento.
        let mut watcher = match notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        }) {
            Ok(w) => w,
            Err(_) => return,
        };
        if watcher
            .watch(&watch_dir, RecursiveMode::NonRecursive)
            .is_err()
        {
            return;
        }
        // Já existe no arranque: assiste desde já. Se ainda não existe, é o
        // evento da pasta de config vendo `locales/` nascer que arma o watch.
        let mut locales_watch = LocalesWatch::default();
        sync_locales_watch(&mut watcher, &mut locales_watch, &locales_dir);

        let config_path = path.clone();
        let mut reloader = Reloader::new(
            path.clone(),
            language,
            move |requested| language::load_catalog(requested, Some(&config_path)),
            messages::schema().len(),
        );
        let mut debounce = Debounce::new();
        let (mut config_touched, mut locales_touched) = (false, false);
        loop {
            match rx.recv_timeout(POLL) {
                Ok(event) => {
                    let touched = classify(&event.paths, &path, &locales_dir);
                    let mut locales_changed = touched.locales_file;
                    if touched.locales_dir
                        && sync_locales_watch(&mut watcher, &mut locales_watch, &locales_dir)
                    {
                        locales_changed = true;
                    }
                    if touched.config || locales_changed {
                        config_touched |= touched.config;
                        locales_touched |= locales_changed;
                        debounce.notice(Instant::now());
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if debounce.ready(Instant::now()) {
                let reload = reloader.fire(config_touched, locales_touched);
                (config_touched, locales_touched) = (false, false);
                if let Some(reload) = reload {
                    on_reload(reload);
                }
            }
        }
    });
    Some(())
}

/// Até quando uma mudança de classe C espera (ADR-0030). A frase que diz
/// isso é do catálogo (`notice.deferred.*`); aqui só o escopo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredScope {
    /// "vale na próxima janela".
    NextWindow,
    /// "vale em aba nova".
    NewTab,
    /// "reinicie o app".
    Restart,
}

/// Uma mudança de classe C: a chave e o escopo em que ela vale. `key` é o
/// nome da chave como aparece no arquivo de exemplo (`appearance.window.
/// opacity`, `[shell]`) -- identificador, igual em todo idioma.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deferred {
    pub key: &'static str,
    pub scope: DeferredScope,
}

/// O que uma recarga precisa fazer, decidido comparando config antiga e
/// nova (ADR-0030). Classe A não aparece aqui: ela é só trocar o `Arc` e
/// redesenhar, o caminho comum a toda recarga.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReloadEffects {
    /// Classe B: recalcula métrica de célula, deriva colunas/linhas e
    /// redimensiona todos os PTYs da janela -- um resize por recarga.
    pub grid_changed: bool,
    /// Classe C: uma entrada por chave que mudou e não se aplica agora,
    /// com o escopo real (próxima janela, aba nova, reinício) -- o mesmo do
    /// arquivo de exemplo. Quem compõe a frase é `ui`, pelo catálogo.
    pub deferred: Vec<Deferred>,
}

/// Compara `old` e `new` e devolve os efeitos de classe B/C. As chaves de
/// cada classe são as que `porecatu.example.toml` já anota -- este
/// código não reclassifica nada, só olha os mesmos campos.
pub fn diff(old: &Config, new: &Config) -> ReloadEffects {
    let grid_changed = old.terminal.font != new.terminal.font
        || old.appearance.window.padding_x != new.appearance.window.padding_x
        || old.appearance.window.padding_y != new.appearance.window.padding_y
        || old.appearance.tabs.height != new.appearance.tabs.height
        || old.appearance.tabs.tab_height != new.appearance.tabs.tab_height
        || old.appearance.tabs.trilha_padding != new.appearance.tabs.trilha_padding
        || old.appearance.terminal_frame.margin != new.appearance.terminal_frame.margin
        || old.appearance.terminal_frame.padding != new.appearance.terminal_frame.padding
        || old.appearance.terminal_frame.corner_radius
            != new.appearance.terminal_frame.corner_radius
        // A barra de status encolhe a grade (ADR-0048 §1): ligá-la,
        // desligá-la ou mudar a altura muda o número de linhas.
        || old.appearance.status_bar.enabled != new.appearance.status_bar.enabled
        || old.appearance.status_bar.height != new.appearance.status_bar.height
        || old.appearance.status_bar.font_size != new.appearance.status_bar.font_size;

    let mut deferred = Vec::new();
    if old.appearance.window.opacity != new.appearance.window.opacity {
        deferred.push(Deferred {
            key: "appearance.window.opacity",
            scope: DeferredScope::NextWindow,
        });
    }
    if old.appearance.window.decorations != new.appearance.window.decorations {
        deferred.push(Deferred {
            key: "appearance.window.decorations",
            scope: DeferredScope::Restart,
        });
    }
    if old.shell != new.shell {
        deferred.push(Deferred {
            key: "[shell]",
            scope: DeferredScope::NewTab,
        });
    }
    if old.terminal.scrollback != new.terminal.scrollback {
        deferred.push(Deferred {
            key: "[terminal.scrollback]",
            scope: DeferredScope::NewTab,
        });
    }
    if old.session != new.session {
        deferred.push(Deferred {
            key: "[session]",
            scope: DeferredScope::Restart,
        });
    }
    if old.project_file != new.project_file {
        deferred.push(Deferred {
            key: "[project_file]",
            scope: DeferredScope::Restart,
        });
    }
    // [git] não entra aqui: é classe A -- o prazo da consulta é recalculado
    // a cada volta do laço lendo a config atual (ADR-0052 §10).
    // [panes] também não entra: é classe A (ADR-0053 §9) -- o mínimo
    // governa o próximo split e o próximo arraste, sem redimensionar o
    // que já existe.

    ReloadEffects {
        grid_changed,
        deferred,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_collapses_a_burst_into_one_fire() {
        let mut d = Debounce::new();
        let t0 = Instant::now();
        d.notice(t0);
        assert!(!d.ready(t0 + Duration::from_millis(50)));
        d.notice(t0 + Duration::from_millis(50)); // segundo evento da rajada
        d.notice(t0 + Duration::from_millis(90)); // terceiro
        assert!(!d.ready(t0 + Duration::from_millis(150))); // ainda na janela do 3º
        assert!(d.ready(t0 + Duration::from_millis(291))); // 90 + 200 + 1
        // a mesma rajada não dispara duas vezes.
        assert!(!d.ready(t0 + Duration::from_millis(1000)));
    }

    #[test]
    fn debounce_without_events_never_fires() {
        let mut d = Debounce::new();
        assert!(!d.ready(Instant::now() + Duration::from_secs(10)));
    }

    #[test]
    fn debounce_fires_again_for_a_second_burst() {
        let mut d = Debounce::new();
        let t0 = Instant::now();
        d.notice(t0);
        assert!(d.ready(t0 + DEBOUNCE + Duration::from_millis(1)));
        d.notice(t0 + Duration::from_secs(5));
        assert!(!d.ready(t0 + Duration::from_secs(5) + Duration::from_millis(100)));
        assert!(d.ready(t0 + Duration::from_secs(5) + DEBOUNCE + Duration::from_millis(1)));
    }

    fn base() -> Config {
        Config::default()
    }

    #[test]
    fn identical_configs_have_no_effects() {
        let effects = diff(&base(), &base());
        assert_eq!(effects, ReloadEffects::default());
    }

    #[test]
    fn font_change_is_class_b() {
        let mut new = base();
        new.terminal.font.size = 20.0;
        assert!(diff(&base(), &new).grid_changed);
    }

    #[test]
    fn tab_height_change_is_class_b() {
        let mut new = base();
        new.appearance.tabs.tab_height = 40;
        assert!(diff(&base(), &new).grid_changed);
    }

    #[test]
    fn status_bar_toggle_is_class_b() {
        let mut new = base();
        new.appearance.status_bar.enabled = false;
        assert!(diff(&base(), &new).grid_changed);
    }

    #[test]
    fn status_bar_height_change_is_class_b() {
        let mut new = base();
        new.appearance.status_bar.height = 40;
        assert!(diff(&base(), &new).grid_changed);
    }

    #[test]
    fn status_bar_color_change_is_class_a_not_b() {
        let mut new = base();
        new.appearance.status_bar.foreground = new.appearance.status_bar.shell;
        assert!(!diff(&base(), &new).grid_changed);
    }

    #[test]
    fn git_poll_interval_change_is_class_a_not_deferred() {
        // ADR-0052 §10: classe A -- o prazo é recalculado a cada volta do
        // laço de eventos lendo a config atual, sem trabalho de hot reload.
        let mut new = base();
        new.git.remote_poll_interval_secs = 0;
        let effects = diff(&base(), &new);
        assert!(!effects.grid_changed);
        assert!(effects.deferred.is_empty());
    }

    #[test]
    fn panes_min_change_is_class_a_not_deferred() {
        // ADR-0053 §9: classe A -- governa o próximo split e o próximo
        // arraste, sem tocar em PTY já existente.
        let mut new = base();
        new.panes.min_columns = 40;
        new.panes.min_rows = 10;
        let effects = diff(&base(), &new);
        assert!(!effects.grid_changed);
        assert!(effects.deferred.is_empty());
    }

    #[test]
    fn color_change_is_class_a_not_b() {
        let mut new = base();
        new.terminal.colors.foreground = new.terminal.colors.background;
        let effects = diff(&base(), &new);
        assert!(!effects.grid_changed);
        assert!(effects.deferred.is_empty());
    }

    #[test]
    fn shell_change_is_deferred_not_grid() {
        let mut new = base();
        new.shell.program = "zsh".to_owned();
        let effects = diff(&base(), &new);
        assert!(!effects.grid_changed);
        assert_eq!(
            effects.deferred,
            vec![Deferred {
                key: "[shell]",
                scope: DeferredScope::NewTab
            }]
        );
    }

    #[test]
    fn scrollback_change_is_deferred() {
        let mut new = base();
        new.terminal.scrollback.lines = 500;
        let effects = diff(&base(), &new);
        assert!(!effects.grid_changed);
        assert_eq!(
            effects.deferred,
            vec![Deferred {
                key: "[terminal.scrollback]",
                scope: DeferredScope::NewTab
            }]
        );
    }

    #[test]
    fn decorations_change_is_deferred() {
        let mut new = base();
        new.appearance.window.decorations = !new.appearance.window.decorations;
        let effects = diff(&base(), &new);
        assert!(!effects.grid_changed);
        assert_eq!(
            effects.deferred,
            vec![Deferred {
                key: "appearance.window.decorations",
                scope: DeferredScope::Restart
            }]
        );
    }

    #[test]
    fn session_change_is_deferred() {
        let mut new = base();
        new.session.enabled = !new.session.enabled;
        let effects = diff(&base(), &new);
        assert_eq!(
            effects.deferred,
            vec![Deferred {
                key: "[session]",
                scope: DeferredScope::Restart
            }]
        );
    }

    #[test]
    fn multiple_deferred_changes_collect_all() {
        let mut new = base();
        new.shell.program = "zsh".to_owned();
        new.session.enabled = !new.session.enabled;
        assert_eq!(diff(&base(), &new).deferred.len(), 2);
    }

    #[test]
    fn language_change_is_class_a_not_deferred() {
        // ADR-0030 (blockquote da classe A): `language` não é classe C, então
        // não gera aviso de "reinicie", e não mexe na grade.
        let mut new = base();
        new.general.language = "pt_BR".to_owned();
        assert_eq!(diff(&base(), &new), ReloadEffects::default());
    }

    fn loaded(language: &str) -> ConfigReload {
        let mut config = base();
        config.general.language = language.to_owned();
        ConfigReload::Loaded {
            config: Box::new(config),
            unknown_keys: Vec::new(),
        }
    }

    fn invalid() -> ConfigReload {
        ConfigReload::Invalid {
            error: ConfigError::new(ConfigErrorKind::Unreadable {
                path: PathBuf::from("porecatu.toml"),
                cause: "x".to_owned(),
            }),
        }
    }

    #[test]
    fn rebuilds_when_language_changes_in_the_config() {
        assert_eq!(
            language_to_rebuild("en_US", Some(&loaded("pt_BR")), false).as_deref(),
            Some("pt_BR")
        );
    }

    #[test]
    fn does_not_rebuild_when_another_key_changes() {
        assert_eq!(
            language_to_rebuild("pt_BR", Some(&loaded("pt_BR")), false),
            None
        );
        assert_eq!(language_to_rebuild("pt_BR", None, false), None);
    }

    #[test]
    fn a_language_file_event_rebuilds_with_the_current_language() {
        assert_eq!(
            language_to_rebuild("pt_BR", None, true).as_deref(),
            Some("pt_BR")
        );
        // Config e arquivo juntos: vale o idioma da config nova.
        assert_eq!(
            language_to_rebuild("en_US", Some(&loaded("pt_BR")), true).as_deref(),
            Some("pt_BR")
        );
    }

    #[test]
    fn an_invalid_config_never_changes_the_language() {
        assert_eq!(language_to_rebuild("pt_BR", Some(&invalid()), false), None);
        // ... mas um arquivo de idioma que mudou junto ainda é refeito, no
        // idioma anterior.
        assert_eq!(
            language_to_rebuild("pt_BR", Some(&invalid()), true).as_deref(),
            Some("pt_BR")
        );
    }

    fn outcome(diagnostics: Vec<Diagnostic>) -> CatalogOutcome {
        CatalogOutcome {
            catalog: Catalog::new(),
            locale: LocaleName::parse("pt_BR").ok(),
            diagnostics,
        }
    }

    fn syntax_error() -> Diagnostic {
        Diagnostic::LayerSyntax {
            path: PathBuf::from("pt_BR.toml"),
            line: Some(2),
            column: Some(1),
            detail: "x".to_owned(),
        }
    }

    fn missing(count: usize) -> Diagnostic {
        Diagnostic::MissingMessages {
            locale: LocaleName::parse("pt_BR").unwrap(),
            count,
        }
    }

    #[test]
    fn a_clean_catalog_replaces_the_current_one() {
        assert!(matches!(
            settle_language(outcome(Vec::new()), "pt_BR", 10),
            LanguageReload::Replace { .. }
        ));
    }

    #[test]
    fn a_language_that_was_not_found_keeps_the_previous_catalog() {
        let diagnostics = vec![Diagnostic::LanguageNotFound {
            requested: "xx_XX".to_owned(),
            searched: vec![PathBuf::from("/a")],
        }];
        let settled = settle_language(outcome(diagnostics.clone()), "xx_XX", 10);
        assert_eq!(settled, LanguageReload::Keep { diagnostics });
    }

    #[test]
    fn an_invalid_name_or_no_catalog_keeps_the_previous_catalog() {
        for diagnostic in [
            Diagnostic::InvalidLanguageName {
                value: "pt_br".to_owned(),
            },
            Diagnostic::NoCatalogAtAll {
                searched: vec![PathBuf::from("/a")],
            },
        ] {
            assert!(matches!(
                settle_language(outcome(vec![diagnostic]), "pt_br", 10),
                LanguageReload::Keep { .. }
            ));
        }
    }

    /// A falta da reserva (`en_US`) não invalida uma troca cujo idioma
    /// pedido foi achado.
    #[test]
    fn a_missing_fallback_does_not_block_a_found_language() {
        let diagnostics = vec![Diagnostic::LanguageNotFound {
            requested: "en_US".to_owned(),
            searched: vec![PathBuf::from("/a")],
        }];
        assert!(matches!(
            settle_language(outcome(diagnostics), "pt_BR", 10),
            LanguageReload::Replace { .. }
        ));
    }

    /// RF-15.23: sintaxe quebrada na **única** camada -- não sobrou frase
    /// nenhuma do idioma -- mantém o anterior.
    #[test]
    fn a_broken_only_layer_keeps_the_previous_catalog() {
        let diagnostics = vec![syntax_error(), missing(10)];
        assert!(matches!(
            settle_language(outcome(diagnostics), "pt_BR", 10),
            LanguageReload::Keep { .. }
        ));
    }

    /// Camada ruim com a instalada válida do mesmo idioma: vale o merge
    /// normal, e o erro com linha e coluna segue nos diagnósticos.
    #[test]
    fn a_broken_layer_beside_a_good_one_still_replaces() {
        let settled = settle_language(outcome(vec![syntax_error(), missing(3)]), "pt_BR", 10);
        match settled {
            LanguageReload::Replace { diagnostics, .. } => {
                assert!(diagnostics.contains(&syntax_error()));
            }
            other => panic!("esperava Replace, veio {other:?}"),
        }
        // Sem nada faltando também.
        assert!(matches!(
            settle_language(outcome(vec![syntax_error()]), "pt_BR", 10),
            LanguageReload::Replace { .. }
        ));
    }

    #[test]
    fn classify_tells_the_config_from_the_locales_folder() {
        let config = Path::new("/c/porecatu.toml");
        let locales = Path::new("/c/locales");
        let paths = |list: &[&str]| list.iter().map(PathBuf::from).collect::<Vec<_>>();
        assert_eq!(
            classify(&paths(&["/c/porecatu.toml"]), config, locales),
            Touched {
                config: true,
                ..Touched::default()
            }
        );
        assert_eq!(
            classify(&paths(&["/c/locales/pt_BR.toml"]), config, locales),
            Touched {
                locales_file: true,
                ..Touched::default()
            }
        );
        assert_eq!(
            classify(&paths(&["/c/locales"]), config, locales),
            Touched {
                locales_dir: true,
                ..Touched::default()
            }
        );
        // Outro arquivo da pasta da config, e arquivo em subpasta de
        // `locales/` (o watch é não recursivo), não contam.
        assert_eq!(
            classify(
                &paths(&["/c/other.toml", "/c/locales/x/y.toml"]),
                config,
                locales
            ),
            Touched::default()
        );
    }

    /// O watch de `locales/` nasce quando a pasta nasce e é descartado
    /// quando ela some -- sem erro em nenhum dos dois casos.
    #[test]
    fn the_locales_watch_follows_the_folder() {
        let mut watch = LocalesWatch::default();
        assert_eq!(watch.sync(false), None);
        assert_eq!(watch.sync(true), Some(WatchChange::Start));
        assert_eq!(watch.sync(true), None);
        assert_eq!(watch.sync(false), Some(WatchChange::Stop));
        assert_eq!(watch.sync(false), None);
        assert_eq!(watch.sync(true), Some(WatchChange::Start));
    }

    fn temp_config(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "porecatu-reload-test-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("porecatu.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn builder(calls: &std::cell::Cell<usize>) -> impl Fn(&str) -> CatalogOutcome + '_ {
        move |_| {
            calls.set(calls.get() + 1);
            outcome(Vec::new())
        }
    }

    /// Config e idioma mudando na mesma gravação viram **um** `Reload`, e o
    /// catálogo é montado uma vez só.
    #[test]
    fn language_and_another_key_in_one_write_are_one_event() {
        let path = temp_config(
            "combined",
            "[general]\nlanguage = \"pt_BR\"\n[terminal.font]\nsize = 20.0\n",
        );
        let calls = std::cell::Cell::new(0);
        let mut reloader = Reloader::new(path, "en_US".to_owned(), builder(&calls), 10);
        let reload = reloader.fire(true, false).expect("um evento");
        assert!(matches!(reload.config, Some(ConfigReload::Loaded { .. })));
        assert!(matches!(
            reload.language,
            Some(LanguageReload::Replace { .. })
        ));
        assert_eq!(calls.get(), 1);
        // A mesma gravação, relida: idioma igual, então nada a refazer.
        let again = reloader.fire(true, false).expect("config relida");
        assert!(again.language.is_none());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn a_language_file_alone_rebuilds_without_touching_the_config() {
        let path = temp_config("file-only", "[general]\nlanguage = \"pt_BR\"\n");
        let calls = std::cell::Cell::new(0);
        let mut reloader = Reloader::new(path, "pt_BR".to_owned(), builder(&calls), 10);
        let reload = reloader.fire(false, true).expect("um evento");
        assert!(reload.config.is_none());
        assert!(reload.language.is_some());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn nothing_touched_sends_nothing() {
        let path = temp_config("idle", "");
        let calls = std::cell::Cell::new(0);
        let mut reloader = Reloader::new(path, "en_US".to_owned(), builder(&calls), 10);
        assert_eq!(reloader.fire(false, false), None);
        assert_eq!(calls.get(), 0);
    }

    /// A troca que falha mantém o catálogo anterior, e o idioma que a
    /// config pede continua sendo o "último lido": criar o arquivo depois
    /// basta para a troca funcionar.
    #[test]
    fn a_failed_switch_keeps_the_previous_and_remembers_what_the_config_says() {
        let path = temp_config("failed", "[general]\nlanguage = \"xx_XX\"\n");
        let not_found = |requested: &str| CatalogOutcome {
            catalog: Catalog::new(),
            locale: LocaleName::parse("en_US").ok(),
            diagnostics: vec![Diagnostic::LanguageNotFound {
                requested: requested.to_owned(),
                searched: Vec::new(),
            }],
        };
        let mut reloader = Reloader::new(path.clone(), "en_US".to_owned(), not_found, 10);
        let reload = reloader.fire(true, false).expect("um evento");
        assert!(matches!(reload.language, Some(LanguageReload::Keep { .. })));
        // Um arquivo de idioma novo aparece: o idioma refeito é o da config.
        let seen = std::cell::RefCell::new(String::new());
        let mut reloader = Reloader {
            config_path: path,
            last_language: "xx_XX".to_owned(),
            build: |requested: &str| {
                *seen.borrow_mut() = requested.to_owned();
                outcome(Vec::new())
            },
            schema_len: 10,
        };
        reloader.fire(false, true).expect("um evento");
        assert_eq!(*seen.borrow(), "xx_XX");
    }

    /// Config inválida: o erro segue, o idioma não muda; arquivo de idioma
    /// alterado na mesma rajada ainda é refeito.
    #[test]
    fn an_invalid_config_with_a_language_file_still_rebuilds_the_language() {
        let path = temp_config("invalid", "[general\nlanguage = ");
        let calls = std::cell::Cell::new(0);
        let mut reloader = Reloader::new(path, "pt_BR".to_owned(), builder(&calls), 10);
        let reload = reloader.fire(true, true).expect("um evento");
        assert!(matches!(reload.config, Some(ConfigReload::Invalid { .. })));
        assert!(matches!(
            reload.language,
            Some(LanguageReload::Replace { .. })
        ));
        assert_eq!(calls.get(), 1);
    }
}
