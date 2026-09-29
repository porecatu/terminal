// SPDX-License-Identifier: GPL-3.0-or-later

//! Frases de interface compostas a partir de erros tipados (ADR-0056 §2).
//!
//! Os crates abaixo de `porecatu-ui` devolvem o motivo como variante e nunca
//! prosa; é aqui, e só aqui, que a variante vira frase de aviso, diálogo ou
//! nota. Nenhuma função chama `to_string()` num erro de outro crate: casa a
//! variante e monta o texto.
//!
//! Três tipos de texto entram nas frases sem tradução, e é de propósito:
//! - o `detail` do crate `toml` e o do compilador de regex (detalhe técnico,
//!   mostrado como chegou);
//! - a `cause` do sistema operacional (`io::Error`, PTY), idem;
//! - o que o usuário escreveu (nome de tema, de tecla, de ação).
//!
//! Hoje as frases são as de pt-BR de sempre; a etapa seguinte troca o corpo
//! destas funções por acessores do catálogo, sem mudar as assinaturas.

use std::fmt::Display;
use std::path::Path;

use porecatu_config::{ConfigError, ConfigErrorKind};
use porecatu_core::ActionParseError;
use porecatu_session::CURRENT_SCHEMA_VERSION;
use porecatu_session::named::SaveError;
use porecatu_term::TerminalSpawnError;

use crate::SaveNamedFailure;
use crate::keymap::{ChordParseError, KeymapIssue};

/// Texto de uma falha que veio do sistema operacional ou de uma biblioteca
/// externa (`io::Error`, `opener`), como chegou. É o `{cause}` do ADR-0056:
/// nunca traduzido, porque quem o escreveu foi o SO.
pub(crate) fn os_cause(err: &dyn Display) -> String {
    err.to_string()
}

/// Corpo do aviso de config inválida: posição (se houver) mais o motivo.
pub(crate) fn config_error(error: &ConfigError) -> String {
    let reason = match &error.kind {
        ConfigErrorKind::Toml { detail } => detail.clone(),
        ConfigErrorKind::Unreadable { path, cause } => {
            format!("não foi possível ler \"{}\": {cause}", path.display())
        }
        ConfigErrorKind::DuplicateThemeName { name } => {
            format!("nome de tema duplicado: \"{name}\"")
        }
    };
    match (error.line, error.column) {
        (Some(line), Some(column)) => format!("linha {line}, coluna {column}: {reason}"),
        _ => reason,
    }
}

/// Nota no grid de uma aba cujo `.porecatu` existe, é de diretório
/// autorizado e não pôde ser lido. `reason` é o texto do `io::Error`.
pub(crate) fn project_file_unreadable(path: &Path, reason: &str) -> String {
    format!("não foi possível ler \"{}\": {reason}", path.display())
}

/// Corpo do aviso "Falha ao iniciar terminal".
pub(crate) fn terminal_spawn_error(error: &TerminalSpawnError) -> String {
    match error {
        TerminalSpawnError::Pty(err) => {
            format!("terminal: pty: {}: {}", err.kind().as_str(), err.cause())
        }
    }
}

/// Corpo do aviso de falha ao salvar sessão nomeada.
pub(crate) fn save_named_failure(failure: &SaveNamedFailure) -> String {
    match failure {
        SaveNamedFailure::WindowNotFound => "janela não encontrada".to_owned(),
        SaveNamedFailure::Save(SaveError::Io(err)) => {
            format!("erro de E/S ao gravar sessão nomeada: {err}")
        }
        SaveNamedFailure::Save(SaveError::NewerSchema { found }) => format!(
            "arquivo existente tem schema_version {found}, mais nova que {CURRENT_SCHEMA_VERSION}; não sobrescrito"
        ),
        SaveNamedFailure::Save(SaveError::EmptyName) => {
            "nome de sessão vazio depois de aparado".to_owned()
        }
    }
}

fn chord_parse_error(error: &ChordParseError) -> String {
    match error {
        ChordParseError::EmptyKey { text } => format!("tecla vazia: \"{text}\""),
        ChordParseError::UnknownModifier { modifier, text } => {
            format!("modificador desconhecido: \"{modifier}\" em \"{text}\"")
        }
        ChordParseError::UnknownKey { key, text } => {
            format!("tecla desconhecida: \"{key}\" em \"{text}\"")
        }
    }
}

fn action_parse_error(error: &ActionParseError) -> String {
    match error {
        ActionParseError::Unknown { input, suggestion } => {
            format!("ação desconhecida: \"{input}\" -- você quis dizer \"{suggestion}\"?")
        }
        ActionParseError::NotBindable { input } => {
            format!("\"{input}\" tem argumento e não é vinculável a tecla")
        }
    }
}

/// Corpo do aviso "Keybinding inválido".
pub(crate) fn keymap_issue(issue: &KeymapIssue) -> String {
    match issue {
        KeymapIssue::MalformedKey(err) => chord_parse_error(err),
        KeymapIssue::DuplicateBinding { keys } => format!(
            "binding duplicado: {} resolvem pra mesma tecla",
            keys.iter()
                .map(|k| format!("\"{k}\""))
                .collect::<Vec<_>>()
                .join(" e ")
        ),
        KeymapIssue::InvalidAction { key, error } => {
            format!("\"{key}\": {}", action_parse_error(error))
        }
    }
}

/// Rótulo do contador da barra de busca quando o padrão não compila
/// (RF-11.4). O detalhe do compilador de regex fica de fora do rótulo
/// (espec. da barra); quem o guarda mostra como chegou.
pub(crate) fn search_pattern_invalid() -> &'static str {
    "padrão inválido"
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use porecatu_term::{PtySize, SpawnConfig, TermParams, Terminal};

    use super::*;

    fn config_error_of(kind: ConfigErrorKind, at: Option<(usize, usize)>) -> ConfigError {
        match at {
            Some((line, column)) => ConfigError::at(line, column, kind),
            None => ConfigError::new(kind),
        }
    }

    #[test]
    fn config_toml_error_carries_position_and_detail() {
        let error = config_error_of(
            ConfigErrorKind::Toml {
                detail: "expected an equals".to_owned(),
            },
            Some((3, 5)),
        );
        assert_eq!(
            config_error(&error),
            "linha 3, coluna 5: expected an equals"
        );
    }

    #[test]
    fn config_unreadable_and_duplicate_have_no_position() {
        let unreadable = config_error_of(
            ConfigErrorKind::Unreadable {
                path: PathBuf::from("p.toml"),
                cause: "acesso negado".to_owned(),
            },
            None,
        );
        assert_eq!(
            config_error(&unreadable),
            "não foi possível ler \"p.toml\": acesso negado"
        );
        let duplicate = config_error_of(
            ConfigErrorKind::DuplicateThemeName {
                name: "x".to_owned(),
            },
            None,
        );
        assert_eq!(config_error(&duplicate), "nome de tema duplicado: \"x\"");
    }

    #[test]
    fn config_phrases_are_the_ones_the_error_displayed_before() {
        for error in [
            config_error_of(
                ConfigErrorKind::Toml {
                    detail: "d".to_owned(),
                },
                Some((1, 2)),
            ),
            config_error_of(
                ConfigErrorKind::Unreadable {
                    path: PathBuf::from("p.toml"),
                    cause: "c".to_owned(),
                },
                None,
            ),
            config_error_of(
                ConfigErrorKind::DuplicateThemeName {
                    name: "n".to_owned(),
                },
                None,
            ),
        ] {
            assert_eq!(config_error(&error), error.to_string());
        }
    }

    #[test]
    fn save_failure_phrases() {
        assert_eq!(
            save_named_failure(&SaveNamedFailure::WindowNotFound),
            "janela não encontrada"
        );
        assert_eq!(
            save_named_failure(&SaveNamedFailure::Save(SaveError::EmptyName)),
            "nome de sessão vazio depois de aparado"
        );
        let newer =
            save_named_failure(&SaveNamedFailure::Save(SaveError::NewerSchema { found: 9 }));
        assert!(newer.starts_with("arquivo existente tem schema_version 9,"));
        let io = save_named_failure(&SaveNamedFailure::Save(SaveError::Io(
            std::io::Error::other("disco cheio"),
        )));
        assert_eq!(io, "erro de E/S ao gravar sessão nomeada: disco cheio");
    }

    #[test]
    fn save_failure_phrases_match_the_display_of_the_error() {
        for error in [
            SaveError::EmptyName,
            SaveError::NewerSchema { found: 9 },
            SaveError::Io(std::io::Error::other("disco cheio")),
        ] {
            let display = error.to_string();
            assert_eq!(save_named_failure(&SaveNamedFailure::Save(error)), display);
        }
    }

    #[test]
    fn action_issue_phrases() {
        let unknown = KeymapIssue::InvalidAction {
            key: "ctrl+z".to_owned(),
            error: ActionParseError::Unknown {
                input: "tab.clsoe".to_owned(),
                suggestion: "tab.close",
            },
        };
        assert_eq!(
            keymap_issue(&unknown),
            "\"ctrl+z\": ação desconhecida: \"tab.clsoe\" -- você quis dizer \"tab.close\"?"
        );
        let not_bindable = KeymapIssue::InvalidAction {
            key: "ctrl+z".to_owned(),
            error: ActionParseError::NotBindable {
                input: "group.set_color".to_owned(),
            },
        };
        assert_eq!(
            keymap_issue(&not_bindable),
            "\"ctrl+z\": \"group.set_color\" tem argumento e não é vinculável a tecla"
        );
    }

    #[test]
    fn chord_issue_phrases() {
        let issue = KeymapIssue::MalformedKey(ChordParseError::UnknownKey {
            key: "bogus".to_owned(),
            text: "ctrl+bogus".to_owned(),
        });
        assert_eq!(
            keymap_issue(&issue),
            "tecla desconhecida: \"bogus\" em \"ctrl+bogus\""
        );
        let empty = KeymapIssue::MalformedKey(ChordParseError::EmptyKey {
            text: "x".to_owned(),
        });
        assert_eq!(keymap_issue(&empty), "tecla vazia: \"x\"");
        let duplicate = KeymapIssue::DuplicateBinding {
            keys: vec!["a".to_owned(), "b".to_owned()],
        };
        assert_eq!(
            keymap_issue(&duplicate),
            "binding duplicado: \"a\" e \"b\" resolvem pra mesma tecla"
        );
    }

    #[test]
    fn terminal_spawn_phrase_names_the_operation_and_keeps_the_cause() {
        // `PtyError::new` is crate-private to `porecatu-pty`; a real failed
        // spawn is the way to get one.
        let result = Terminal::spawn(
            SpawnConfig {
                program: Some("porecatu-no-such-program-xyz".to_owned()),
                args: Vec::new(),
                env: Vec::new(),
                cwd: None,
                size: PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            },
            TermParams::default(),
            || {},
        );
        let Err(error) = result else {
            panic!("spawning a program that does not exist should fail");
        };
        let phrase = terminal_spawn_error(&error);
        assert!(
            phrase.starts_with("terminal: pty: spawn_command: "),
            "{phrase}"
        );
        assert_eq!(phrase, error.to_string());
    }
}
