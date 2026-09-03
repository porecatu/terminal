// SPDX-License-Identifier: GPL-3.0-or-later

//! Hot reload do arquivo de config (F4 etapa 4, ADR-0003 regra 5,
//! ADR-0030). O watcher roda numa thread própria, nunca a main thread
//! (ADR-0007): lê e parseia o arquivo fora dela e manda pra `lib.rs`, pelo
//! mesmo `EventLoopProxy` que o PTY usa para `Wakeup`, o resultado já
//! pronto -- `Config` carregado ou erro já formatado, nunca um caminho de
//! arquivo para a main thread abrir.
//!
//! `diff` decide o que uma recarga precisa fazer, comparando a config
//! antiga com a nova -- puro, sem `notify` nem `winit`, testável sem GPU e
//! sem janela. As classes e as chaves de cada uma são as do
//! `porecatu.example.toml`, não uma lista reinventada aqui: `diff` só
//! espelha o que já está anotado lá.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use notify::{Event, RecursiveMode, Watcher};
use porecatu_config::{Config, ConfigError};

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
                error: ConfigError::new(format!(
                    "não foi possível ler \"{}\": {err}",
                    path.display()
                )),
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
///
/// `None` se o diretório do arquivo de config não existir -- degrada para
/// "sem hot reload" em vez de falhar o start (ADR-0003 regra 1: ausência
/// de config é estado válido, e o mesmo vale pro diretório dela).
pub fn watch(path: PathBuf, on_reload: impl Fn(ConfigReload) + Send + 'static) -> Option<()> {
    let watch_dir = path.parent()?.to_path_buf();
    if !watch_dir.exists() {
        return None;
    }

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

        let mut debounce = Debounce::new();
        loop {
            match rx.recv_timeout(POLL) {
                Ok(event) if event.paths.iter().any(|p| p == &path) => {
                    debounce.notice(Instant::now());
                }
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if debounce.ready(Instant::now())
                && let Some(reload) = read_and_parse(&path)
            {
                on_reload(reload);
            }
        }
    });
    Some(())
}

/// O que uma recarga precisa fazer, decidido comparando config antiga e
/// nova (ADR-0030). Classe A não aparece aqui: ela é só trocar o `Arc` e
/// redesenhar, o caminho comum a toda recarga.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReloadEffects {
    /// Classe B: recalcula métrica de célula, deriva colunas/linhas e
    /// redimensiona todos os PTYs da janela -- um resize por recarga.
    pub grid_changed: bool,
    /// Classe C: uma mensagem por chave que mudou e não se aplica agora,
    /// com o escopo real ("vale na próxima janela", "vale em aba nova",
    /// "reinicie o app") -- o mesmo texto do arquivo de exemplo.
    pub deferred: Vec<String>,
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
            != new.appearance.terminal_frame.corner_radius;

    let mut deferred = Vec::new();
    if old.appearance.window.opacity != new.appearance.window.opacity {
        deferred.push("appearance.window.opacity: vale na próxima janela".to_owned());
    }
    if old.appearance.window.decorations != new.appearance.window.decorations {
        deferred.push("appearance.window.decorations: reinicie o app".to_owned());
    }
    if old.appearance.window.tab_bar_position != new.appearance.window.tab_bar_position {
        deferred.push("appearance.window.tab_bar_position: reinicie o app".to_owned());
    }
    if old.shell != new.shell {
        deferred.push("[shell]: vale em aba nova".to_owned());
    }
    if old.terminal.scrollback != new.terminal.scrollback {
        deferred.push("[terminal.scrollback]: vale em aba nova".to_owned());
    }
    if old.session != new.session {
        deferred.push("[session]: reinicie o app".to_owned());
    }

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
            vec!["[shell]: vale em aba nova".to_owned()]
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
            vec!["[terminal.scrollback]: vale em aba nova".to_owned()]
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
            vec!["appearance.window.decorations: reinicie o app".to_owned()]
        );
    }

    #[test]
    fn session_change_is_deferred() {
        let mut new = base();
        new.session.enabled = !new.session.enabled;
        let effects = diff(&base(), &new);
        assert_eq!(
            effects.deferred,
            vec!["[session]: reinicie o app".to_owned()]
        );
    }

    #[test]
    fn multiple_deferred_changes_collect_all() {
        let mut new = base();
        new.shell.program = "zsh".to_owned();
        new.session.enabled = !new.session.enabled;
        assert_eq!(diff(&base(), &new).deferred.len(), 2);
    }
}
