// SPDX-License-Identifier: GPL-3.0-or-later

//! Completude dos arquivos de idioma (ADR-0056 §10, PRD-015): lê `locales/`
//! do repositório por `CARGO_MANIFEST_DIR` -- os arquivos do produto, não uma
//! cópia -- e os confere contra o registro de mensagens de `porecatu-ui`.
//!
//! O que reprova: arquivo que não parseia; conjunto de chaves diferente do
//! registro, nos dois sentidos; marcadores que divergem dos declarados; plural
//! sem exatamente `one` e `other`; valor vazio; e "guia" (ADR-0009 §8) em
//! `pt_BR.toml`.
//!
//! Esse trabalho é do `parse_layer` de `porecatu-locale`, que já classifica
//! cada defeito; este teste o chama e exige que **nada** sobre. Mais dois
//! passos que o `parse_layer` não dá: o conjunto de arquivos (só nomes
//! válidos, sem `en_US` faltando) e a varredura de "guia", feita à mão, sem
//! crate de regex.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use porecatu_locale::{LocaleName, Message, parse_layer};

fn locales_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales")
}

/// `(nome do idioma, texto do arquivo)` de cada `*.toml` de `locales/`.
fn locale_files() -> Vec<(String, String)> {
    let dir = locales_dir();
    let mut files = Vec::new();
    for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap()
            .to_owned();
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        files.push((name, text));
    }
    files.sort();
    files
}

#[test]
fn the_repository_ships_en_us_and_pt_br() {
    let names: BTreeSet<String> = locale_files().into_iter().map(|(name, _)| name).collect();
    assert!(names.contains("en_US"), "en_US is the fallback: {names:?}");
    assert!(names.contains("pt_BR"), "{names:?}");
}

#[test]
fn every_file_name_is_a_valid_locale_name() {
    for (name, _) in locale_files() {
        assert!(
            LocaleName::parse(&name).is_ok(),
            "{name}.toml: o nome do arquivo é o idioma e tem de casar ^[a-z]{{2,3}}_[A-Z]{{2}}$"
        );
    }
}

#[test]
fn every_file_parses_and_matches_the_registry_exactly() {
    let schema = porecatu_ui::message_schema();
    assert!(!schema.is_empty(), "the registry is empty");

    for (name, text) in locale_files() {
        let outcome = parse_layer(&text, &schema);

        if let Some(error) = &outcome.syntax_error {
            panic!("{name}.toml não parseia: {error:?}");
        }

        // Chave do arquivo que o registro não conhece.
        assert!(
            outcome.unknown_keys.is_empty(),
            "{name}.toml tem chaves que o registro não declara: {:?}",
            outcome.unknown_keys
        );

        // Valor inválido: marcador que o registro não declara, marcador mal
        // formado, plural sem `other`, forma errada, valor vazio.
        assert!(
            outcome.invalid_keys.is_empty(),
            "{name}.toml tem frases inválidas: {:?}",
            outcome.invalid_keys
        );

        // Chave do registro que falta no arquivo.
        let present: BTreeSet<&str> = outcome.messages.keys().map(String::as_str).collect();
        let missing: Vec<&str> = schema
            .keys()
            .copied()
            .filter(|id| !present.contains(id))
            .collect();
        assert!(
            missing.is_empty(),
            "{name}.toml não tem chaves do registro: {missing:?}"
        );
        assert_eq!(present.len(), schema.len(), "{name}.toml");
    }
}

/// O `parse_layer` aceita frase que **omite** um marcador declarado (o
/// usuário pode escrever uma versão mais curta); os dois arquivos do projeto,
/// não: têm de usar todos (ADR-0056 §5).
#[test]
fn the_project_files_use_every_declared_placeholder() {
    let schema = porecatu_ui::message_schema();
    for (name, text) in locale_files() {
        let outcome = parse_layer(&text, &schema);
        for (id, spec) in &schema {
            let message = &outcome.messages[*id];
            let templates: Vec<&str> = match message {
                Message::Simple(text) => vec![text.as_str()],
                Message::Plural { one, other } => {
                    let mut all = vec![other.as_str()];
                    all.extend(one.as_deref());
                    all
                }
            };
            for template in templates {
                for placeholder in spec.placeholders {
                    assert!(
                        template.contains(&format!("{{{placeholder}}}")),
                        "{name}.toml, {id}: falta o marcador {{{placeholder}}} em {template:?}"
                    );
                }
            }
        }
    }
}

/// Uma frase de plural tem as duas formas, `one` e `other`, nos arquivos do
/// projeto (o `parse_layer` só exige `other`).
#[test]
fn the_project_files_give_both_plural_forms() {
    let schema = porecatu_ui::message_schema();
    for (name, text) in locale_files() {
        let outcome = parse_layer(&text, &schema);
        for (id, spec) in &schema {
            let message = &outcome.messages[*id];
            match (spec.plural, message) {
                (true, Message::Plural { one, .. }) => {
                    assert!(one.is_some(), "{name}.toml, {id}: plural sem `one`");
                }
                (true, Message::Simple(_)) => {
                    panic!("{name}.toml, {id}: o registro pede plural");
                }
                (false, Message::Plural { .. }) => {
                    panic!("{name}.toml, {id}: o registro pede frase simples");
                }
                (false, Message::Simple(_)) => {}
            }
        }
    }
}

/// Toda forma de toda frase tem texto (valor vazio, ou só espaço, não vale).
#[test]
fn no_value_is_blank() {
    let schema = porecatu_ui::message_schema();
    for (name, text) in locale_files() {
        let outcome = parse_layer(&text, &schema);
        for (id, message) in &outcome.messages {
            let templates: Vec<&str> = match message {
                Message::Simple(text) => vec![text.as_str()],
                Message::Plural { one, other } => {
                    let mut all = vec![other.as_str()];
                    all.extend(one.as_deref());
                    all
                }
            };
            for template in templates {
                assert!(
                    !template.trim().is_empty(),
                    "{name}.toml, {id}: valor vazio"
                );
            }
        }
    }
}

/// Ocorrências de `word` em `text` como palavra inteira, sem distinguir caixa.
/// À mão, sem regex: um caractere alfanumérico (Unicode) de cada lado quebra
/// a palavra.
fn whole_word_hits(text: &str, word: &str) -> Vec<usize> {
    let haystack = text.to_lowercase();
    let mut hits = Vec::new();
    let mut from = 0;
    while let Some(found) = haystack[from..].find(word) {
        let start = from + found;
        let end = start + word.len();
        let before = haystack[..start].chars().next_back();
        let after = haystack[end..].chars().next();
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
        if !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char) {
            hits.push(start);
        }
        from = end;
    }
    hits
}

/// ADR-0009 §8: a regra de terminologia "abas", nunca "guias", que era de
/// revisão humana, é teste.
#[test]
fn pt_br_never_says_guia() {
    let (_, text) = locale_files()
        .into_iter()
        .find(|(name, _)| name == "pt_BR")
        .expect("pt_BR.toml");
    // Só os valores: o comentário do cabeçalho cita a palavra proibida para
    // explicar a regra.
    let outcome = parse_layer(&text, &porecatu_ui::message_schema());
    for (id, message) in &outcome.messages {
        let templates: Vec<&str> = match message {
            Message::Simple(text) => vec![text.as_str()],
            Message::Plural { one, other } => {
                let mut all = vec![other.as_str()];
                all.extend(one.as_deref());
                all
            }
        };
        for template in templates {
            for word in ["guia", "guias"] {
                assert!(
                    whole_word_hits(template, word).is_empty(),
                    "pt_BR.toml, {id}: usa \"{word}\"; o termo do projeto é \"aba\" (ADR-0009 §8)"
                );
            }
        }
    }
}

#[test]
fn the_whole_word_scan_respects_word_boundaries() {
    assert_eq!(whole_word_hits("Nova Guia", "guia"), vec![5]);
    assert_eq!(whole_word_hits("guias abertas", "guias"), vec![0]);
    // Parte de outra palavra não conta.
    assert!(whole_word_hits("guiar, aguia, guiado", "guia").is_empty());
    assert!(whole_word_hits("a guia_x", "guia").is_empty());
    // Pontuação e aspas contam como fronteira.
    assert_eq!(whole_word_hits("\"guia\"", "guia"), vec![1]);
    assert_eq!(whole_word_hits("(GUIA)", "guia"), vec![1]);
}
