// SPDX-License-Identifier: GPL-3.0-or-later

//! O que a tela sabe do arquivo (RF-16.21 a RF-16.23, ADR-0058 §3, ADR-0059
//! §4): o texto que ela viu pela última vez -- a **base** --, e o que fazer
//! quando o disco diz outra coisa. Estado puro, sem janela: a tela entrega o
//! que leu do disco ([`Disk`]) e recebe de volta o que mostrar ([`Effect`]).
//!
//! A decisão é por **conteúdo**, nunca por `mtime`:
//!
//! - o texto do disco é o da base: foi a própria gravação (ou um `touch`), e
//!   nada muda além dos valores mostrados;
//! - difere e a tela não tem pendências: a base troca em silêncio;
//! - difere e há pendências: faixa de conflito, com Recarregar e Manter.
//!
//! Arquivo inválido põe a tela em somente leitura até um próximo texto válido.
//! Arquivo inexistente é uma base vazia: o primeiro Salvar parte do exemplo
//! embutido.

use std::fs;
use std::io;
use std::path::Path;

use porecatu_config::{Config, ConfigError, ConfigErrorKind};

/// O arquivo como o disco o mostra agora.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Disk {
    /// Não existe.
    Missing,
    /// Existe e o carregador o aceita.
    Valid { text: String, config: Box<Config> },
    /// Existe e o carregador o recusa (sintaxe ou tipo), ou não pôde ser lido.
    Invalid {
        text: Option<String>,
        error: ConfigError,
    },
}

impl Disk {
    /// Lê `path` e passa o texto pelo mesmo `parse` da carga.
    pub(crate) fn observe(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => match porecatu_config::parse(&text) {
                Ok((config, _)) => Disk::Valid {
                    text,
                    config: Box::new(config),
                },
                Err(error) => Disk::Invalid {
                    text: Some(text),
                    error,
                },
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => Disk::Missing,
            Err(err) => Disk::Invalid {
                text: None,
                error: ConfigError::new(ConfigErrorKind::Unreadable {
                    path: path.to_path_buf(),
                    cause: err.to_string(),
                }),
            },
        }
    }

    /// O texto do arquivo; `None` se não existe ou não pôde ser lido.
    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Disk::Missing => None,
            Disk::Valid { text, .. } => Some(text),
            Disk::Invalid { text, .. } => text.as_deref(),
        }
    }
}

/// Em que pé a tela está com o arquivo.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Status {
    Normal,
    /// O arquivo mudou fora daqui com pendências na tela (RF-16.23). `disk` é
    /// o arquivo como está agora.
    Conflict {
        disk: Box<Disk>,
    },
    /// O arquivo é inválido: somente leitura (RF-16.22).
    Invalid {
        error: ConfigError,
    },
}

/// O que a tela deve fazer com o que o disco disse.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Effect {
    /// Mostrar `config`: a base do rascunho passa a ser ela. Vale para a
    /// própria gravação, que não muda nada além disso, e para a troca de base
    /// em silêncio.
    Show(Box<Config>),
    /// Há pendências e o arquivo mudou: a faixa de conflito. O rascunho fica
    /// como está.
    Conflict,
    /// Arquivo inválido: nada a mostrar, a tela fica em somente leitura.
    ReadOnly,
}

/// A decisão pura de uma recarga (ADR-0059 §4): `base` é o texto que a tela
/// viu da última vez (`None` se o arquivo não existia) e `disk` o texto de
/// agora.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Igual à base: a própria gravação. Nunca é conflito.
    Own,
    /// Diferente, sem pendências: troca a base em silêncio.
    Swap,
    /// Diferente, com pendências: a faixa.
    Conflict,
}

pub(crate) fn decide(base: Option<&str>, disk: Option<&str>, has_pending: bool) -> Decision {
    if base == disk {
        Decision::Own
    } else if has_pending {
        Decision::Conflict
    } else {
        Decision::Swap
    }
}

/// A base e o status.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FileState {
    /// O texto que a tela viu pela última vez: o que ela leu ao abrir, ou
    /// gravou, ou aceitou numa recarga. É a base da edição do Salvar.
    base: Option<String>,
    status: Status,
}

impl FileState {
    /// O estado de uma tela que abre com o arquivo `disk`.
    pub(crate) fn new(disk: &Disk) -> Self {
        let status = match disk {
            Disk::Invalid { error, .. } => Status::Invalid {
                error: error.clone(),
            },
            _ => Status::Normal,
        };
        Self {
            base: disk.text().map(str::to_owned),
            status,
        }
    }

    /// O texto que a edição do Salvar parte: `None` é arquivo inexistente, e
    /// aí o Salvar parte do exemplo embutido (RF-16.21).
    pub(crate) fn base(&self) -> Option<&str> {
        self.base.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn status(&self) -> &Status {
        &self.status
    }

    /// A tela está em somente leitura: controles e textos esmaecidos, e
    /// nenhuma edição aceita (RF-16.22).
    pub(crate) fn read_only(&self) -> bool {
        matches!(self.status, Status::Invalid { .. })
    }

    /// Um controle pode mudar o rascunho agora.
    pub(crate) fn allows_edit(&self) -> bool {
        !self.read_only()
    }

    /// O disco foi lido de novo (uma recarga, qualquer que seja a causa).
    pub(crate) fn observe(&mut self, disk: &Disk, has_pending: bool) -> Effect {
        let config = match disk {
            Disk::Invalid { error, .. } => {
                self.status = Status::Invalid {
                    error: error.clone(),
                };
                return Effect::ReadOnly;
            }
            Disk::Missing => Box::new(Config::default()),
            Disk::Valid { config, .. } => config.clone(),
        };
        // Um arquivo que voltou a ser válido já não é somente leitura, mas
        // pendência nenhuma foi aceita enquanto era.
        match decide(self.base(), disk.text(), has_pending) {
            Decision::Own => {
                self.status = Status::Normal;
                Effect::Show(config)
            }
            Decision::Swap => {
                self.base = disk.text().map(str::to_owned);
                self.status = Status::Normal;
                Effect::Show(config)
            }
            Decision::Conflict => {
                self.status = Status::Conflict {
                    disk: Box::new(disk.clone()),
                };
                Effect::Conflict
            }
        }
    }

    /// Aceita o arquivo como está num conflito -- Recarregar e Manter fazem as
    /// duas isto, e só diferem no que fazem com as pendências: a base passa a
    /// ser o texto do disco, e o `Config` dele é o que o rascunho mostra.
    pub(crate) fn accept_disk(&mut self) -> Option<Box<Config>> {
        let Status::Conflict { disk } = &self.status else {
            return None;
        };
        let config = match &**disk {
            Disk::Valid { config, .. } => config.clone(),
            Disk::Missing => Box::new(Config::default()),
            Disk::Invalid { .. } => return None,
        };
        self.base = disk.text().map(str::to_owned);
        self.status = Status::Normal;
        Some(config)
    }

    /// O Salvar gravou `text`: é a base de agora, e a recarga que ele dispara
    /// vai chegar com o mesmo texto.
    pub(crate) fn saved(&mut self, text: String) {
        self.base = Some(text);
        self.status = Status::Normal;
    }

    /// A faixa que o estado pede, se algum.
    pub(crate) fn banner(&self) -> Option<Banner> {
        match &self.status {
            Status::Normal => None,
            Status::Conflict { .. } => Some(Banner::Conflict),
            Status::Invalid { error } => Some(Banner::Invalid(error.clone())),
        }
    }
}

/// A faixa no topo do painel (ADR-0060 §2).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Banner {
    /// "O arquivo mudou fora daqui", com Recarregar e Manter minhas
    /// alterações (RF-16.23).
    Conflict,
    /// "O arquivo é inválido", com o erro e Abrir arquivo no editor
    /// (RF-16.22).
    Invalid(ConfigError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid(text: &str) -> Disk {
        Disk::Valid {
            text: text.to_owned(),
            config: Box::new(porecatu_config::parse(text).unwrap().0),
        }
    }

    fn invalid(text: &str) -> Disk {
        Disk::Invalid {
            text: Some(text.to_owned()),
            error: porecatu_config::parse(text).unwrap_err(),
        }
    }

    const A: &str = "[terminal.font]\nsize = 14.0\n";
    const B: &str = "[terminal.font]\nsize = 14.0\n[terminal]\ntheme = \"nord\"\n";

    // ---- as três ramificações

    #[test]
    fn the_decision_has_three_branches() {
        // Igual à base: a própria gravação, haja ou não pendência.
        assert_eq!(decide(Some(A), Some(A), false), Decision::Own);
        assert_eq!(decide(Some(A), Some(A), true), Decision::Own);
        // Diferente sem pendências: troca em silêncio.
        assert_eq!(decide(Some(A), Some(B), false), Decision::Swap);
        // Diferente com pendências: conflito.
        assert_eq!(decide(Some(A), Some(B), true), Decision::Conflict);
        // Inexistente é uma base como outra: `None` contra texto.
        assert_eq!(decide(None, None, true), Decision::Own);
        assert_eq!(decide(None, Some(A), false), Decision::Swap);
        assert_eq!(decide(Some(A), None, true), Decision::Conflict);
    }

    #[test]
    fn an_unchanged_text_is_never_a_conflict_even_with_pending_changes() {
        let mut state = FileState::new(&valid(A));
        let effect = state.observe(&valid(A), true);
        assert!(matches!(effect, Effect::Show(_)));
        assert_eq!(state.status(), &Status::Normal);
        assert!(state.banner().is_none());
    }

    #[test]
    fn a_different_text_without_pending_changes_swaps_the_base_silently() {
        let mut state = FileState::new(&valid(A));
        let Effect::Show(config) = state.observe(&valid(B), false) else {
            panic!()
        };
        assert_eq!(config.terminal.theme, "nord");
        assert_eq!(state.base(), Some(B));
        assert!(state.banner().is_none());
    }

    #[test]
    fn a_different_text_with_pending_changes_raises_the_conflict_and_keeps_the_base() {
        let mut state = FileState::new(&valid(A));
        assert_eq!(state.observe(&valid(B), true), Effect::Conflict);
        assert_eq!(state.banner(), Some(Banner::Conflict));
        // A base só muda quando o usuário decide.
        assert_eq!(state.base(), Some(A));
        assert!(state.allows_edit(), "o conflito não bloqueia a edição");
    }

    // ---- a própria gravação

    #[test]
    fn the_own_write_comes_back_as_the_base_and_is_never_a_conflict() {
        let mut state = FileState::new(&valid(A));
        // Salvar gravou B: a base passa a ser B antes de a recarga chegar.
        state.saved(B.to_owned());
        assert_eq!(state.base(), Some(B));
        let effect = state.observe(&valid(B), true);
        assert!(matches!(effect, Effect::Show(_)));
        assert!(state.banner().is_none());
        // E de novo: a recarga é entregue mais de uma vez sem virar conflito.
        assert!(matches!(state.observe(&valid(B), true), Effect::Show(_)));
    }

    // ---- recarregar e manter

    #[test]
    fn keep_and_reload_both_take_the_disk_text_as_the_new_base() {
        let mut state = FileState::new(&valid(A));
        state.observe(&valid(B), true);
        let config = state.accept_disk().expect("o arquivo novo");
        assert_eq!(config.terminal.theme, "nord");
        assert_eq!(state.base(), Some(B));
        assert!(state.banner().is_none());
        // Sem conflito, não há o que aceitar.
        assert!(state.accept_disk().is_none());
    }

    #[test]
    fn a_second_change_while_the_banner_is_up_updates_the_text_it_offers() {
        let mut state = FileState::new(&valid(A));
        state.observe(&valid(B), true);
        let c = "[terminal]\ntheme = \"dracula\"\n";
        assert_eq!(state.observe(&valid(c), true), Effect::Conflict);
        let config = state.accept_disk().unwrap();
        assert_eq!(config.terminal.theme, "dracula");
        assert_eq!(state.base(), Some(c));
    }

    // ---- arquivo inválido

    #[test]
    fn an_invalid_file_is_read_only_with_the_error_and_a_valid_one_brings_it_back() {
        let broken = "[terminal.font\nsize = 14.0\n";
        let mut state = FileState::new(&valid(A));
        assert_eq!(state.observe(&invalid(broken), false), Effect::ReadOnly);
        assert!(state.read_only());
        assert!(!state.allows_edit());
        let Some(Banner::Invalid(error)) = state.banner() else {
            panic!()
        };
        assert!(error.line.is_some(), "a faixa cita a linha");
        // Corrigido fora: volta sozinha, e a base acompanha o texto novo.
        let Effect::Show(config) = state.observe(&valid(B), false) else {
            panic!()
        };
        assert!(!state.read_only());
        assert_eq!(config.terminal.theme, "nord");
        assert_eq!(state.base(), Some(B));
    }

    #[test]
    fn a_window_that_opens_on_an_invalid_file_starts_read_only() {
        let broken = "[terminal.font\nsize = 14.0\n";
        let state = FileState::new(&invalid(broken));
        assert!(state.read_only());
        assert_eq!(state.base(), Some(broken));
        // E ao ser corrigido, sem pendência nenhuma (não se pôde editar), troca.
        let mut state = state;
        assert!(matches!(state.observe(&valid(A), false), Effect::Show(_)));
        assert!(!state.read_only());
    }

    #[test]
    fn read_only_blocks_every_edit_and_normal_allows() {
        let broken = "x = [\n";
        assert!(!FileState::new(&invalid(broken)).allows_edit());
        assert!(FileState::new(&valid(A)).allows_edit());
        assert!(FileState::new(&Disk::Missing).allows_edit());
    }

    // ---- arquivo inexistente

    #[test]
    fn a_missing_file_opens_normal_with_no_base_and_defaults() {
        let state = FileState::new(&Disk::Missing);
        assert!(!state.read_only());
        assert_eq!(state.base(), None);
        assert!(state.banner().is_none());
    }

    #[test]
    fn a_missing_file_that_stays_missing_is_not_a_change() {
        let mut state = FileState::new(&Disk::Missing);
        let Effect::Show(config) = state.observe(&Disk::Missing, true) else {
            panic!()
        };
        assert_eq!(*config, Config::default());
    }

    #[test]
    fn a_file_created_meanwhile_is_a_conflict_if_there_is_a_pending_change() {
        let mut state = FileState::new(&Disk::Missing);
        assert_eq!(state.observe(&valid(A), true), Effect::Conflict);
        let mut quiet = FileState::new(&Disk::Missing);
        assert!(matches!(quiet.observe(&valid(A), false), Effect::Show(_)));
        assert_eq!(quiet.base(), Some(A));
    }

    // ---- leitura do disco

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "porecatu-file-state-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn observe_reads_a_valid_an_invalid_and_a_missing_file() {
        let path = scratch("observe.toml");
        assert_eq!(Disk::observe(&path), Disk::Missing);
        fs::write(&path, A).unwrap();
        assert!(matches!(Disk::observe(&path), Disk::Valid { .. }));
        fs::write(&path, "[terminal.font\n").unwrap();
        let Disk::Invalid { text, error } = Disk::observe(&path) else {
            panic!()
        };
        assert_eq!(text.as_deref(), Some("[terminal.font\n"));
        assert!(error.line.is_some());
        fs::remove_file(&path).unwrap();
    }
}
