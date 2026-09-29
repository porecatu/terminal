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
//! Duas famílias convivem aqui. O **registro** (`msg`, mais abaixo) é o que
//! já lê o catálogo de idioma: cada identificador pontilhado, seus marcadores
//! e se é plural, num lugar só, de onde saem os acessores tipados e o esquema
//! que `porecatu-locale` valida. As funções acima dele (`config_error`,
//! `keymap_issue`, ...) ainda compõem a frase de pt-BR de sempre a partir da
//! variante do erro; a etapa seguinte troca o corpo delas por acessores do
//! registro, sem mudar as assinaturas.

use std::fmt::Display;
use std::path::Path;

use porecatu_config::{ConfigError, ConfigErrorKind};
use porecatu_core::ActionParseError;
use porecatu_locale::{Catalog, MessageSpec, Schema};
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

/// Substitui o modelo da frase `id` no catálogo. Frase ausente devolve o
/// **identificador** (RF-15.11): o defeito aparece na tela em vez de sumir.
/// `count` escolhe `one`/`other` numa frase de plural e é ignorado numa
/// simples.
pub(crate) fn render(
    catalog: &Catalog,
    id: &'static str,
    count: Option<u64>,
    args: &[(&str, String)],
) -> String {
    let Some(message) = catalog.get(id) else {
        return id.to_owned();
    };
    let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    porecatu_locale::format(message.select(count.unwrap_or(0)), &args)
}

/// Registro de mensagens (ADR-0056 §1). Cada linha declara uma frase:
///
/// - `nome()` é uma frase simples, sem marcadores;
/// - `nome(a, b)` tem os marcadores `{a}` e `{b}`, e o acessor recebe um
///   argumento por marcador -- esquecer um é erro de compilação, não um
///   `{a}` cru na tela;
/// - `nome(plural)` e `nome(plural, a)` são frases de plural: o acessor
///   recebe `count: usize` primeiro, que escolhe `one`/`other` e preenche
///   `{count}`.
///
/// Uma tabela pode ter tabelas dentro (`dialog { close_tab { .. }, }`), até
/// o teto de dois níveis do formato. O identificador é o caminho pontilhado
/// (`dialog.close_tab.title`), e o mesmo registro gera o [`schema`].
///
/// Toda entrada termina em vírgula, menos as tabelas de primeiro nível.
macro_rules! registry {
    ( $( $table:ident { $($body:tt)* } )* ) => {
        /// Acessores tipados: `msg::tab_menu::close(&catalog)`.
        pub(crate) mod msg {
            $(
                pub(crate) mod $table {
                    registry!(@items [$table] $($body)*);
                }
            )*
        }

        /// O que o app declara ao `porecatu-locale`: todo identificador,
        /// com marcadores e plural.
        pub(crate) fn schema() -> Schema {
            #[allow(unused_mut)]
            let mut schema = Schema::new();
            $( registry!(@schema schema [$table] $($body)*); )*
            schema
        }
    };

    (@items [$($pfx:ident)*]) => {};
    (@items [$($pfx:ident)*] $name:ident ( plural $(, $arg:ident)* ) , $($rest:tt)*) => {
        pub(crate) fn $name(
            catalog: &::porecatu_locale::Catalog,
            count: usize
            $(, $arg: impl ::std::fmt::Display)*
        ) -> String {
            crate::messages::render(
                catalog,
                concat!($(stringify!($pfx), ".",)* stringify!($name)),
                Some(count as u64),
                &[
                    ("count", count.to_string())
                    $(, (stringify!($arg), $arg.to_string()))*
                ],
            )
        }
        registry!(@items [$($pfx)*] $($rest)*);
    };
    (@items [$($pfx:ident)*] $name:ident ( $($arg:ident),* ) , $($rest:tt)*) => {
        pub(crate) fn $name(
            catalog: &::porecatu_locale::Catalog
            $(, $arg: impl ::std::fmt::Display)*
        ) -> String {
            crate::messages::render(
                catalog,
                concat!($(stringify!($pfx), ".",)* stringify!($name)),
                None,
                &[ $( (stringify!($arg), $arg.to_string()) ),* ],
            )
        }
        registry!(@items [$($pfx)*] $($rest)*);
    };
    (@items [$($pfx:ident)*] $sub:ident { $($body:tt)* } , $($rest:tt)*) => {
        pub(crate) mod $sub {
            registry!(@items [$($pfx)* $sub] $($body)*);
        }
        registry!(@items [$($pfx)*] $($rest)*);
    };

    (@schema $schema:ident [$($pfx:ident)*]) => {};
    (@schema $schema:ident [$($pfx:ident)*] $name:ident ( plural $(, $arg:ident)* ) , $($rest:tt)*) => {
        $schema.insert(
            concat!($(stringify!($pfx), ".",)* stringify!($name)),
            MessageSpec::plural(&["count" $(, stringify!($arg))*]),
        );
        registry!(@schema $schema [$($pfx)*] $($rest)*);
    };
    (@schema $schema:ident [$($pfx:ident)*] $name:ident ( $($arg:ident),* ) , $($rest:tt)*) => {
        $schema.insert(
            concat!($(stringify!($pfx), ".",)* stringify!($name)),
            MessageSpec::simple(&[ $( stringify!($arg) ),* ]),
        );
        registry!(@schema $schema [$($pfx)*] $($rest)*);
    };
    (@schema $schema:ident [$($pfx:ident)*] $sub:ident { $($body:tt)* } , $($rest:tt)*) => {
        registry!(@schema $schema [$($pfx)* $sub] $($body)*);
        registry!(@schema $schema [$($pfx)*] $($rest)*);
    };
}

registry! {
    tab_menu {
        new(),
        close(),
        move_to_group(),
    }
    terminal_menu {
        copy(),
        paste(),
        select_all(),
        search(),
        open_link(),
        copy_link(),
    }
    group_menu {
        rename(),
        set_color(),
        collapse(),
        expand(),
        new_tab(),
        close(plural),
        dissolve(),
    }
    group_editor {
        section_group(),
        section_color(),
        default_name(),
    }
    move_to_group {
        new_group(),
    }
    session_picker {
        save_item(),
        empty_list(),
        name_placeholder(),
    }
    dialog {
        cancel(),
        close_tab {
            title(),
            body(title),
            confirm(),
        },
        close_pane {
            title(),
            body(title),
            confirm(),
        },
        close_window {
            title(),
            body_tabs(),
            body_program(),
            confirm(),
        },
        close_group {
            title(),
            body(plural),
            confirm(plural),
        },
        delete_session {
            title(),
            body(name),
            confirm(),
        },
        overwrite_session {
            title(name),
            body(),
            confirm(),
        },
    }
    notice {
        language_not_found {
            title(),
            body(language, searched),
        },
        language_invalid_name {
            title(),
            body(value),
        },
        language_syntax {
            title(),
            body(path, detail),
            body_at(path, line, column, detail),
        },
        language_unreadable {
            title(),
            body(path, cause),
        },
        language_missing_messages {
            title(),
            body(plural, locale),
        },
        language_unknown_keys {
            title(),
            body(plural, path),
        },
    }
}

/// Auxiliar dos testes de frase (ADR-0056 §10): carrega os arquivos de
/// `locales/` do repositório -- o mesmo par que o app lê em disco --, para
/// que um teste que compara uma frase compare a que o usuário vê, e não uma
/// cópia dela escrita no teste.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    use porecatu_locale::{Catalog, parse_layer};

    use super::schema;

    fn locales_dir() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales"))
    }

    fn layer(locale: &str) -> porecatu_locale::Messages {
        let path = locales_dir().join(format!("{locale}.toml"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
        let outcome = parse_layer(&text, &schema());
        if let Some(error) = outcome.syntax_error {
            panic!("{}: {error:?}", path.display());
        }
        outcome.messages
    }

    /// `en_US` por baixo, `locale` por cima -- a mesma mescla do app.
    pub(crate) fn catalog(locale: &str) -> Catalog {
        Catalog::from_layers([layer("en_US"), layer(locale)])
    }

    pub(crate) fn pt_br() -> Catalog {
        catalog("pt_BR")
    }

    pub(crate) fn en_us() -> Catalog {
        catalog("en_US")
    }
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
    fn the_schema_declares_placeholders_and_plural_for_every_id() {
        let schema = schema();
        let close = schema["tab_menu.close"];
        assert!(!close.plural);
        assert!(close.placeholders.is_empty());

        let group_close = schema["group_menu.close"];
        assert!(group_close.plural);
        assert_eq!(group_close.placeholders, ["count"]);

        // Tabelas com tabelas dentro: o identificador é o caminho pontilhado.
        assert_eq!(schema["dialog.close_tab.body"].placeholders, ["title"]);
        assert_eq!(
            schema["dialog.overwrite_session.title"].placeholders,
            ["name"]
        );
        assert_eq!(schema["dialog.cancel"].placeholders.len(), 0);
        assert_eq!(
            schema["notice.language_syntax.body_at"].placeholders,
            ["path", "line", "column", "detail"]
        );
        let missing = schema["notice.language_missing_messages.body"];
        assert!(missing.plural);
        assert_eq!(missing.placeholders, ["count", "locale"]);
    }

    #[test]
    fn a_phrase_missing_from_the_catalog_is_its_identifier() {
        let empty = Catalog::new();
        assert_eq!(msg::tab_menu::close(&empty), "tab_menu.close");
        assert_eq!(msg::group_menu::close(&empty, 3), "group_menu.close");
        assert_eq!(
            msg::dialog::close_tab::body(&empty, "vim"),
            "dialog.close_tab.body"
        );
    }

    /// O valor de um marcador vem de fora (um título de aba, que vem de um
    /// programa) e nunca é lido de novo como modelo (ADR-0056 §5).
    #[test]
    fn placeholder_values_are_substituted_literally_and_never_expanded() {
        let pt = test_support::pt_br();
        assert_eq!(
            msg::dialog::close_tab::body(&pt, "{count} {title}"),
            "\"{count} {title}\" tem um programa em primeiro plano. Fechar mesmo assim?"
        );
    }

    #[test]
    fn plural_uses_one_only_for_exactly_one() {
        for (catalog, one, other) in [
            (
                test_support::pt_br(),
                "Fechar grupo (1 aba)",
                "Fechar grupo (2 abas)",
            ),
            (
                test_support::en_us(),
                "Close group (1 tab)",
                "Close group (2 tabs)",
            ),
        ] {
            assert_eq!(msg::group_menu::close(&catalog, 1), one);
            assert_eq!(msg::group_menu::close(&catalog, 2), other);
            assert!(msg::group_menu::close(&catalog, 0).contains("(0 "));
        }
    }

    #[test]
    fn the_same_accessor_answers_in_the_language_of_the_catalog() {
        assert_eq!(msg::tab_menu::close(&test_support::pt_br()), "Fechar aba");
        assert_eq!(msg::tab_menu::close(&test_support::en_us()), "Close tab");
        assert_eq!(
            msg::group_editor::section_group(&test_support::pt_br()),
            "GRUPO"
        );
        assert_eq!(
            msg::group_editor::section_group(&test_support::en_us()),
            "GROUP"
        );
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
