// SPDX-License-Identifier: GPL-3.0-or-later

//! Uma camada: o texto de um arquivo de idioma, validado contra o esquema
//! que o chamador entrega (ADR-0056 §4). O crate não sabe quais frases o app
//! tem; só confere o que o arquivo diz contra o que o esquema declara.

use std::collections::{BTreeMap, HashMap};

use crate::message::{Message, Piece, pieces};

/// Tabelas aninhadas além deste nível viram chave desconhecida
/// (`dialog.close_tab.title`: duas tabelas, depois a chave).
const MAX_TABLE_DEPTH: usize = 2;

/// Tabela reservada: ignorada, sem aviso.
const RESERVED_TABLE: &str = "meta";

/// O que o app declara sobre uma frase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageSpec {
    pub placeholders: &'static [&'static str],
    pub plural: bool,
}

impl MessageSpec {
    pub const fn simple(placeholders: &'static [&'static str]) -> Self {
        Self {
            placeholders,
            plural: false,
        }
    }

    pub const fn plural(placeholders: &'static [&'static str]) -> Self {
        Self {
            placeholders,
            plural: true,
        }
    }
}

/// Identificador pontilhado (`"group_menu.close"`) para o que o app declara.
pub type Schema = BTreeMap<&'static str, MessageSpec>;

/// Frases de uma camada, por identificador pontilhado.
pub type Messages = HashMap<String, Message>;

/// Por que uma chave do esquema foi recusada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidReason {
    /// `{x}` que o esquema não declara para esta chave.
    UnknownPlaceholder {
        placeholder: String,
    },
    /// `{` ou `}` solto, ou `{...}` que não é um identificador.
    MalformedPlaceholder,
    /// Tabela de plural sem a forma `other`.
    MissingOther,
    /// Tabela de plural onde o esquema diz frase simples.
    PluralWhereSimple,
    /// Texto onde o esquema diz plural.
    SimpleWherePlural,
    Empty,
    /// Valor que não é texto (nem tabela de plural, quando cabe).
    WrongType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidKey {
    pub key: String,
    pub reason: InvalidReason,
}

/// Erro de sintaxe do TOML, com linha e coluna contadas a partir de 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    /// `None` quando o crate `toml` não informa posição.
    pub position: Option<(usize, usize)>,
    /// Texto do crate `toml`, mostrado como chegou.
    pub detail: String,
}

/// Resultado de ler uma camada. Com `syntax_error` presente, o resto vem
/// vazio: a camada inteira falhou.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayerOutcome {
    pub messages: Messages,
    pub unknown_keys: Vec<String>,
    pub invalid_keys: Vec<InvalidKey>,
    pub syntax_error: Option<SyntaxError>,
}

/// Lê e valida `source` contra `schema`. Frase inválida não entra em
/// `messages`; chave desconhecida também não.
pub fn parse_layer(source: &str, schema: &Schema) -> LayerOutcome {
    // Bloco de notas do Windows grava BOM; o TOML não o aceita no início.
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let table = match source.parse::<toml::Table>() {
        Ok(table) => table,
        Err(err) => {
            return LayerOutcome {
                syntax_error: Some(SyntaxError {
                    position: err.span().map(|span| line_column_at(source, span.start)),
                    detail: err.message().to_owned(),
                }),
                ..LayerOutcome::default()
            };
        }
    };
    let mut outcome = LayerOutcome::default();
    for (key, value) in &table {
        if key == RESERVED_TABLE {
            continue;
        }
        walk(key.clone(), value, 1, schema, &mut outcome);
    }
    outcome
}

fn walk(id: String, value: &toml::Value, depth: usize, schema: &Schema, out: &mut LayerOutcome) {
    match (schema.get(id.as_str()), value) {
        (Some(spec), toml::Value::String(text)) => {
            let result = if spec.plural {
                Err(InvalidReason::SimpleWherePlural)
            } else {
                check_template(text, spec).map(|()| Message::Simple(text.clone()))
            };
            record(id, result, out);
        }
        (Some(spec), toml::Value::Table(table)) => {
            let result = plural_message(&id, table, spec, out);
            record(id, result, out);
        }
        (Some(_), _) => record(id, Err(InvalidReason::WrongType), out),
        (None, toml::Value::Table(table)) => {
            if looks_plural(table) || depth > MAX_TABLE_DEPTH {
                out.unknown_keys.push(id);
            } else {
                for (key, child) in table {
                    walk(format!("{id}.{key}"), child, depth + 1, schema, out);
                }
            }
        }
        (None, _) => out.unknown_keys.push(id),
    }
}

fn record(id: String, result: Result<Message, InvalidReason>, out: &mut LayerOutcome) {
    match result {
        Ok(message) => {
            out.messages.insert(id, message);
        }
        Err(reason) => out.invalid_keys.push(InvalidKey { key: id, reason }),
    }
}

/// Tabela que o esquema não conhece mas tem cara de frase de plural: conta
/// como uma chave desconhecida só, em vez de uma por forma.
fn looks_plural(table: &toml::Table) -> bool {
    (table.contains_key("one") || table.contains_key("other"))
        && table.values().all(|value| !value.is_table())
}

fn plural_message(
    id: &str,
    table: &toml::Table,
    spec: &MessageSpec,
    out: &mut LayerOutcome,
) -> Result<Message, InvalidReason> {
    // Forma além de `one`/`other` (`few`, `many`...) é chave desconhecida
    // (ADR-0056 §5), mas não invalida a frase.
    for key in table
        .keys()
        .filter(|key| !matches!(key.as_str(), "one" | "other"))
    {
        out.unknown_keys.push(format!("{id}.{key}"));
    }
    if !spec.plural {
        return Err(InvalidReason::PluralWhereSimple);
    }
    let form = |name: &str| match table.get(name) {
        None => Ok(None),
        Some(toml::Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(InvalidReason::WrongType),
    };
    let one = form("one")?;
    let other = form("other")?.ok_or(InvalidReason::MissingOther)?;
    for text in one.iter().chain(std::iter::once(&other)) {
        check_template(text, spec)?;
    }
    Ok(Message::Plural { one, other })
}

fn check_template(template: &str, spec: &MessageSpec) -> Result<(), InvalidReason> {
    if template.is_empty() {
        return Err(InvalidReason::Empty);
    }
    for piece in pieces(template) {
        match piece {
            Piece::Malformed(_) => return Err(InvalidReason::MalformedPlaceholder),
            Piece::Placeholder(name) if !spec.placeholders.contains(&name) => {
                return Err(InvalidReason::UnknownPlaceholder {
                    placeholder: name.to_owned(),
                });
            }
            Piece::Placeholder(_) | Piece::Text(_) => {}
        }
    }
    Ok(())
}

/// Linha e coluna (ambas contadas a partir de 1) do byte offset `pos` em
/// `source`. Cópia de `porecatu-config`: este crate não depende dele.
fn line_column_at(source: &str, pos: usize) -> (usize, usize) {
    let pos = pos.min(source.len());
    let prefix = &source[..pos];
    let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
    let column = match prefix.rfind('\n') {
        Some(last_newline) => pos - last_newline,
        None => pos + 1,
    };
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Schema {
        Schema::from([
            ("tab_menu.close", MessageSpec::simple(&[])),
            ("dialog.close_tab.title", MessageSpec::simple(&[])),
            ("dialog.close_tab.body", MessageSpec::simple(&["title"])),
            ("group_menu.close", MessageSpec::plural(&["count"])),
            ("status", MessageSpec::simple(&[])),
        ])
    }

    fn simple(text: &str) -> Message {
        Message::Simple(text.to_owned())
    }

    #[test]
    fn parses_simple_plural_and_two_levels() {
        let source = r#"
[tab_menu]
close = "Close tab"

[group_menu]
close = { one = "Close group ({count} tab)", other = "Close group ({count} tabs)" }

[dialog.close_tab]
title = "Close tab?"
body = "\"{title}\" is running. Close anyway?"
"#;
        let out = parse_layer(source, &schema());
        assert_eq!(out.syntax_error, None);
        assert!(out.unknown_keys.is_empty(), "{:?}", out.unknown_keys);
        assert!(out.invalid_keys.is_empty(), "{:?}", out.invalid_keys);
        assert_eq!(out.messages.len(), 4);
        assert_eq!(out.messages["tab_menu.close"], simple("Close tab"));
        assert_eq!(
            out.messages["group_menu.close"],
            Message::Plural {
                one: Some("Close group ({count} tab)".to_owned()),
                other: "Close group ({count} tabs)".to_owned(),
            }
        );
        assert_eq!(out.messages["dialog.close_tab.title"], simple("Close tab?"));
    }

    #[test]
    fn dotted_keys_and_top_level_keys_work() {
        let out = parse_layer(
            "status = \"ok\"\ndialog.close_tab.title = \"T\"\n",
            &schema(),
        );
        assert_eq!(out.messages["status"], simple("ok"));
        assert_eq!(out.messages["dialog.close_tab.title"], simple("T"));
    }

    #[test]
    fn meta_is_ignored_silently() {
        let out = parse_layer(
            "[meta]\nname = \"Português\"\nnested = { a = 1 }\n[tab_menu]\nclose = \"x\"\n",
            &schema(),
        );
        assert!(out.unknown_keys.is_empty());
        assert!(out.invalid_keys.is_empty());
        assert_eq!(out.messages.len(), 1);
    }

    #[test]
    fn unknown_keys_are_collected_not_loaded() {
        let source = r#"
[tab_menu]
close = "x"
open = "y"

[nowhere]
a = "b"

[other_plural]
close = { one = "a", other = "b" }
"#;
        let out = parse_layer(source, &schema());
        assert_eq!(
            out.unknown_keys,
            ["nowhere.a", "other_plural.close", "tab_menu.open"]
        );
        assert_eq!(out.messages.len(), 1);
        assert!(out.invalid_keys.is_empty());
    }

    #[test]
    fn table_deeper_than_two_levels_is_unknown() {
        let out = parse_layer("[a.b.c]\nd = \"x\"\n", &schema());
        assert_eq!(out.unknown_keys, ["a.b.c"]);
    }

    #[test]
    fn extra_plural_forms_are_unknown_but_keep_the_message() {
        let out = parse_layer(
            "[group_menu]\nclose = { one = \"a\", other = \"b\", few = \"c\" }\n",
            &schema(),
        );
        assert_eq!(out.unknown_keys, ["group_menu.close.few"]);
        assert!(out.messages.contains_key("group_menu.close"));
        assert!(out.invalid_keys.is_empty());
    }

    #[test]
    fn syntax_error_has_line_and_column() {
        let out = parse_layer("[tab_menu]\nclose = \"ok\"\nbroken = = \"x\"\n", &schema());
        let err = out.syntax_error.expect("erro de sintaxe");
        assert_eq!(err.position, Some((3, 10)));
        assert!(!err.detail.is_empty());
        assert!(out.messages.is_empty());
    }

    #[test]
    fn syntax_error_position_after_crlf_and_utf8() {
        let out = parse_layer("[tab_menu]\r\nclose = \"é\"\r\nx = = 1\r\n", &schema());
        let err = out.syntax_error.expect("erro de sintaxe");
        assert_eq!(err.position, Some((3, 5)));
    }

    #[test]
    fn byte_order_mark_is_tolerated() {
        let out = parse_layer("\u{feff}[tab_menu]\nclose = \"x\"\n", &schema());
        assert_eq!(out.syntax_error, None);
        assert_eq!(out.messages.len(), 1);
    }

    fn invalid(source: &str) -> Vec<InvalidKey> {
        parse_layer(source, &schema()).invalid_keys
    }

    #[test]
    fn placeholder_outside_schema_invalidates_the_key() {
        let source = "[tab_menu]\nclose = \"Close {title}\"\n";
        let out = parse_layer(source, &schema());
        assert_eq!(
            out.invalid_keys,
            [InvalidKey {
                key: "tab_menu.close".to_owned(),
                reason: InvalidReason::UnknownPlaceholder {
                    placeholder: "title".to_owned()
                },
            }]
        );
        assert!(out.messages.is_empty());
    }

    #[test]
    fn placeholder_outside_schema_in_plural_form_invalidates() {
        let out = parse_layer(
            "[group_menu]\nclose = { one = \"{count}\", other = \"{contagem}\" }\n",
            &schema(),
        );
        assert_eq!(out.invalid_keys.len(), 1);
        assert!(out.messages.is_empty());
    }

    #[test]
    fn omitted_placeholder_is_accepted() {
        let out = parse_layer("[dialog.close_tab]\nbody = \"Close?\"\n", &schema());
        assert!(out.invalid_keys.is_empty());
        assert_eq!(out.messages["dialog.close_tab.body"], simple("Close?"));
    }

    #[test]
    fn escaped_braces_are_not_placeholders() {
        let out = parse_layer("[tab_menu]\nclose = \"{{title}}\"\n", &schema());
        assert!(out.invalid_keys.is_empty());
        assert_eq!(out.messages["tab_menu.close"], simple("{{title}}"));
    }

    #[test]
    fn malformed_braces_invalidate() {
        for value in ["a {", "a }", "{Count}", "{}", "{count"] {
            let source = format!("[tab_menu]\nclose = \"{value}\"\n");
            assert_eq!(
                invalid(&source)[0].reason,
                InvalidReason::MalformedPlaceholder,
                "{value}"
            );
        }
    }

    #[test]
    fn plural_without_other_is_invalid() {
        let got = invalid("[group_menu]\nclose = { one = \"a\" }\n");
        assert_eq!(got[0].reason, InvalidReason::MissingOther);
    }

    #[test]
    fn plural_without_one_is_valid() {
        let out = parse_layer("[group_menu]\nclose = { other = \"tabs\" }\n", &schema());
        assert!(out.invalid_keys.is_empty());
        assert_eq!(
            out.messages["group_menu.close"],
            Message::Plural {
                one: None,
                other: "tabs".to_owned()
            }
        );
    }

    #[test]
    fn plural_and_simple_mismatch_are_invalid() {
        assert_eq!(
            invalid("[tab_menu]\nclose = { one = \"a\", other = \"b\" }\n")[0].reason,
            InvalidReason::PluralWhereSimple
        );
        assert_eq!(
            invalid("[group_menu]\nclose = \"plain\"\n")[0].reason,
            InvalidReason::SimpleWherePlural
        );
    }

    #[test]
    fn empty_value_is_invalid() {
        assert_eq!(
            invalid("[tab_menu]\nclose = \"\"\n")[0].reason,
            InvalidReason::Empty
        );
        assert_eq!(
            invalid("[group_menu]\nclose = { one = \"\", other = \"b\" }\n")[0].reason,
            InvalidReason::Empty
        );
    }

    #[test]
    fn wrong_type_is_invalid() {
        assert_eq!(
            invalid("[tab_menu]\nclose = 3\n")[0].reason,
            InvalidReason::WrongType
        );
        assert_eq!(
            invalid("[group_menu]\nclose = { other = 3 }\n")[0].reason,
            InvalidReason::WrongType
        );
    }

    #[test]
    fn an_invalid_key_does_not_drop_its_neighbours() {
        let out = parse_layer(
            "[tab_menu]\nclose = \"\"\n[dialog.close_tab]\ntitle = \"T\"\n",
            &schema(),
        );
        assert_eq!(out.invalid_keys.len(), 1);
        assert_eq!(out.messages.len(), 1);
    }
}
