// SPDX-License-Identifier: GPL-3.0-or-later

//! "Uma frase de interface no código" (ADR-0056 §10, métrica do PRD-015).
//!
//! Este teste varre o código-fonte de todo crate do projeto (`crates/*/src`)
//! em busca de literais de string que **pareçam prosa de interface em
//! português**, e reprova qualquer um fora da [`ALLOWLIST`] abaixo. E confere
//! que a única frase em inglês escrita no código, a do aviso "nenhum arquivo
//! de idioma" (`NO_CATALOG_MESSAGE`), aparece **uma vez**.
//!
//! **A heurística não é prova, é rede.** Um literal é suspeito se tem letra
//! acentuada do português (`á à â ã é ê í ó ô õ ú ç`) ou uma palavra inteira
//! de uma lista curta (aba, abas, grupo, sessão, fechar, janela, painel,
//! nenhum, não). Não pega prosa em inglês nem frase sem nenhuma dessas
//! palavras; quem escreve frase nova no código sem acento e sem essas
//! palavras passa batido -- é para isso que o registro de mensagens torna o
//! caminho certo o mais curto, e o teste de completude de `locales/` cobre o
//! outro lado.
//!
//! **O que não é varrido:** comentários; blocos `#[cfg(test)]`; argumentos de
//! `eprintln!`/`println!`/`print!`/`eprint!`/`panic!`/`assert*!`/
//! `debug_assert*!`/`unreachable!`/`todo!`/`unimplemented!` e de
//! `.expect(...)` -- saída de erro e de desenvolvedor, fora do PRD-015
//! (RF-15.14) --; e `tests/` (não está em `src/`).

use std::fs;
use std::path::{Path, PathBuf};

/// Literal que a heurística acha suspeito e que **não é** texto de
/// interface. `(sufixo do caminho, trecho do literal, por quê)`. Nada aqui
/// chega a uma superfície: cada item diz por que não.
const ALLOWLIST: &[(&str, &str, &str)] = &[
    // ---- `Display` dos erros tipados dos crates de baixo (ADR-0056 §2) ------
    // O `Display` continua existindo para a saída de erro e para depuração,
    // fora do PRD-015. `porecatu-ui` nunca o usa para montar aviso, diálogo ou
    // nota: casa a variante e escreve a frase pelo catálogo (`messages.rs`).
    (
        "porecatu-config/src/error.rs",
        "não foi possível ler",
        "Display de ConfigErrorKind::Unreadable (stderr/depuração)",
    ),
    (
        "porecatu-core/src/action.rs",
        "ação desconhecida",
        "Display de ActionParseError::Unknown (stderr/depuração)",
    ),
    (
        "porecatu-core/src/action.rs",
        "não é vinculável",
        "Display de ActionParseError::NotBindable (stderr/depuração)",
    ),
    (
        "porecatu-session/src/named.rs",
        "erro de E/S ao gravar",
        "Display de SaveError::Io (stderr/depuração)",
    ),
    (
        "porecatu-session/src/named.rs",
        "arquivo existente tem schema_version",
        "Display de SaveError::NewerSchema (stderr/depuração)",
    ),
    (
        "porecatu-session/src/named.rs",
        "nome de sessão vazio",
        "Display de SaveError::EmptyName (stderr/depuração)",
    ),
    (
        "porecatu-session/src/named.rs",
        "sem diretório de sessões nomeadas",
        "Display de erro de resolução de caminho (stderr/depuração)",
    ),
    (
        "porecatu-session/src/lib.rs",
        "sem caminho de sessão",
        "Display de erro de resolução de caminho (stderr/depuração)",
    ),
    (
        "porecatu-render/src/gpu.rs",
        "surface não suportada",
        "Display de SurfaceError, só vai para eprintln! (stderr)",
    ),
    (
        "porecatu-render/src/gpu.rs",
        "surface incompatível",
        "Display de SurfaceError, só vai para eprintln! (stderr)",
    ),
    // ---- não é texto: rótulo de métrica --------------------------------------
    (
        "porecatu-ui/src/lib.rs",
        "restauração de sessão",
        "rótulo de métrica de `trace::report`, impresso em stderr só com \
         PORECATU_TRACE; leitura de desenvolvedor, não de usuário",
    ),
];

/// Palavras inteiras suspeitas, além de qualquer letra acentuada.
const WORDS: &[&str] = &[
    "aba", "abas", "grupo", "sessão", "fechar", "janela", "painel", "nenhum", "não",
];

/// Macros e métodos cujos argumentos são saída de erro/desenvolvedor.
const SKIPPED_CALLS: &[&str] = &[
    "eprintln",
    "println",
    "print",
    "eprint",
    "panic",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "unreachable",
    "todo",
    "unimplemented",
    "expect",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    let crates = repo_root().join("crates");
    let mut dirs: Vec<PathBuf> = fs::read_dir(&crates)
        .expect("crates/")
        .flatten()
        .map(|e| e.path().join("src"))
        .collect();
    dirs.sort();
    for dir in dirs {
        walk(&dir, &mut out);
    }
    // O binário (`src/cli.rs`, `src/main.rs`): linha de comando em inglês
    // fixo, mas varrido do mesmo jeito.
    walk(&repo_root().join("src"), &mut out);
    out
}

/// Um literal de string achado no código de produção.
#[derive(Debug, PartialEq, Eq)]
struct Literal {
    line: usize,
    text: String,
}

/// Percorre `source` e devolve os literais de string que estão em código de
/// produção: fora de comentário, de `#[cfg(test)]` e dos argumentos das
/// chamadas de `SKIPPED_CALLS`. Tokenizador mínimo à mão -- só o que importa
/// aqui (comentário, string com escape, string crua, literal de caractere,
/// chaves e parênteses).
fn production_literals(source: &str) -> Vec<Literal> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;

    // `Some(depth)`: dentro de um item `#[cfg(test)]`, saindo ao voltar a esse
    // nível de chave.
    let mut brace_depth: i64 = 0;
    let mut test_item_depth: Option<i64> = None;
    let mut pending_test_attr = false;
    // Dentro dos argumentos de uma chamada ignorada: nível de parêntese em
    // que ela fecha.
    let mut paren_depth: i64 = 0;
    let mut skip_until_paren: Option<i64> = None;
    let mut last_ident = String::new();

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                let mut depth = 1;
                i += 2;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                        depth += 1;
                        i += 2;
                    } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            '#' if chars.get(i + 1) == Some(&'[') => {
                // Atributo: lê até o `]` que fecha, para reconhecer
                // `#[cfg(test)]`.
                let start = i;
                let mut depth = 0;
                while i < chars.len() {
                    match chars[i] {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        '\n' => line += 1,
                        _ => {}
                    }
                    i += 1;
                }
                let attr: String = chars[start..i].iter().collect();
                let compact: String = attr.chars().filter(|c| !c.is_whitespace()).collect();
                if compact == "#[cfg(test)]" {
                    pending_test_attr = true;
                }
            }
            'r' if is_raw_string_start(&chars, i) => {
                let (text, next, lines) = read_raw_string(&chars, i);
                if test_item_depth.is_none() && skip_until_paren.is_none() {
                    out.push(Literal { line, text });
                }
                line += lines;
                i = next;
            }
            '"' => {
                let (text, next, lines) = read_string(&chars, i);
                if test_item_depth.is_none() && skip_until_paren.is_none() {
                    out.push(Literal { line, text });
                }
                line += lines;
                i = next;
            }
            '\'' => {
                // Literal de caractere (`'"'`, `'\n'`) ou lifetime (`'a`).
                if chars.get(i + 1) == Some(&'\\') {
                    i += 2;
                    while i < chars.len() && chars[i] != '\'' {
                        i += 1;
                    }
                    i += 1;
                } else if chars.get(i + 2) == Some(&'\'') {
                    i += 3;
                } else {
                    i += 1;
                }
            }
            '{' => {
                brace_depth += 1;
                if pending_test_attr {
                    test_item_depth = Some(brace_depth);
                    pending_test_attr = false;
                }
                i += 1;
            }
            '}' => {
                if test_item_depth == Some(brace_depth) {
                    test_item_depth = None;
                }
                brace_depth -= 1;
                i += 1;
            }
            ';' => {
                // `#[cfg(test)] use ...;` -- item sem chaves.
                pending_test_attr = false;
                i += 1;
            }
            '(' => {
                paren_depth += 1;
                if skip_until_paren.is_none() && SKIPPED_CALLS.contains(&last_ident.as_str()) {
                    skip_until_paren = Some(paren_depth);
                }
                i += 1;
            }
            ')' => {
                if skip_until_paren == Some(paren_depth) {
                    skip_until_paren = None;
                }
                paren_depth -= 1;
                i += 1;
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                // `eprintln!(` -- o identificador é seguido de `!`, que não
                // reseta `last_ident`.
                last_ident = ident;
            }
            '!' => {
                i += 1;
            }
            c if c.is_whitespace() => {
                i += 1;
            }
            _ => {
                last_ident.clear();
                i += 1;
            }
        }
    }
    out
}

fn is_raw_string_start(chars: &[char], i: usize) -> bool {
    // `r"`, `r#"`, `r##"`... e o `r` não pode ser o fim de outro identificador.
    if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        return false;
    }
    let mut j = i + 1;
    while chars.get(j) == Some(&'#') {
        j += 1;
    }
    chars.get(j) == Some(&'"')
}

fn read_raw_string(chars: &[char], i: usize) -> (String, usize, usize) {
    let mut j = i + 1;
    let mut hashes = 0;
    while chars.get(j) == Some(&'#') {
        hashes += 1;
        j += 1;
    }
    j += 1; // abre-aspas
    let start = j;
    let mut lines = 0;
    while j < chars.len() {
        if chars[j] == '"' && (0..hashes).all(|k| chars.get(j + 1 + k) == Some(&'#')) {
            let text: String = chars[start..j].iter().collect();
            return (text, j + 1 + hashes, lines);
        }
        if chars[j] == '\n' {
            lines += 1;
        }
        j += 1;
    }
    (chars[start..].iter().collect(), chars.len(), lines)
}

fn read_string(chars: &[char], i: usize) -> (String, usize, usize) {
    let mut j = i + 1;
    let mut text = String::new();
    let mut lines = 0;
    while j < chars.len() {
        match chars[j] {
            '"' => return (text, j + 1, lines),
            '\\' => {
                // Escape: `\"` e `\\` entram como o caractere; `\n`, `\u{..}`
                // não importam para a heurística e ficam como escritos.
                if let Some(&next) = chars.get(j + 1) {
                    if next == '\n' {
                        lines += 1;
                    }
                    if next == '"' || next == '\\' {
                        text.push(next);
                    } else {
                        text.push('\\');
                        text.push(next);
                    }
                    j += 2;
                } else {
                    j += 1;
                }
            }
            '\n' => {
                lines += 1;
                text.push('\n');
                j += 1;
            }
            c => {
                text.push(c);
                j += 1;
            }
        }
    }
    (text, chars.len(), lines)
}

const ACCENTED: &str = "áàâãéêíóôõúçÁÀÂÃÉÊÍÓÔÕÚÇ";

/// Palavras inteiras, sem distinguir caixa, separadas por qualquer caractere
/// que não seja letra, dígito ou `_`.
fn has_word(text: &str, word: &str) -> bool {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|w| w == word)
}

fn looks_like_portuguese_prose(text: &str) -> bool {
    text.chars().any(|c| ACCENTED.contains(c)) || WORDS.iter().any(|w| has_word(text, w))
}

#[test]
fn no_interface_prose_is_written_in_the_code() {
    let mut offenders = Vec::new();
    for path in rust_sources() {
        let source = fs::read_to_string(&path).unwrap();
        let shown = path
            .strip_prefix(repo_root())
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let shown = shown.trim_start_matches("../../").to_owned();
        for literal in production_literals(&source) {
            if !looks_like_portuguese_prose(&literal.text) {
                continue;
            }
            let allowed = ALLOWLIST
                .iter()
                .any(|(suffix, part, _)| shown.ends_with(suffix) && literal.text.contains(part));
            if !allowed {
                offenders.push(format!("{shown}:{}: {:?}", literal.line, literal.text));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "frase de interface escrita no código (ADR-0056): mova para o registro de \
         `messages.rs` e para os arquivos de `locales/`, ou, se não é interface, \
         acrescente à ALLOWLIST com o motivo:\n{}",
        offenders.join("\n")
    );
}

/// Toda entrada da allowlist ainda tem um literal que a justifique -- senão
/// ela é resíduo e esconderia frase nova.
#[test]
fn every_allowlist_entry_is_still_needed() {
    let mut used = vec![false; ALLOWLIST.len()];
    for path in rust_sources() {
        let source = fs::read_to_string(&path).unwrap();
        let shown = path.to_string_lossy().replace('\\', "/");
        for literal in production_literals(&source) {
            if !looks_like_portuguese_prose(&literal.text) {
                continue;
            }
            for (i, (suffix, part, _)) in ALLOWLIST.iter().enumerate() {
                if shown.ends_with(suffix) && literal.text.contains(part) {
                    used[i] = true;
                }
            }
        }
    }
    let stale: Vec<&str> = ALLOWLIST
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|((suffix, part, _), _)| {
            let _ = suffix;
            *part
        })
        .collect();
    assert!(stale.is_empty(), "entradas de allowlist sem uso: {stale:?}");
}

/// A única frase de interface que o código escreve (RF-15.12): sem arquivo
/// nenhum, não há de onde ler o texto do aviso que diz isso.
#[test]
fn the_only_english_phrase_in_the_code_is_the_no_catalog_message() {
    let mut hits = Vec::new();
    for path in rust_sources() {
        let source = fs::read_to_string(&path).unwrap();
        for literal in production_literals(&source) {
            if literal.text.contains("language files not found") {
                hits.push(format!("{}:{}", path.display(), literal.line));
            }
        }
    }
    assert_eq!(
        hits.len(),
        1,
        "NO_CATALOG_MESSAGE deve aparecer uma vez só no código: {hits:?}"
    );
    assert!(
        hits[0]
            .replace('\\', "/")
            .contains("porecatu-locale/src/resolve.rs"),
        "{hits:?}"
    );
}

// ---- o próprio varredor ------------------------------------------------------

#[test]
fn the_scanner_skips_comments_test_blocks_and_debug_output() {
    let source = r##"
// "comentário não varrido"
/* "outro comentário não varrido" */
fn a() {
    eprintln!("mensagem de erro não varrida");
    x.expect("falha não varrida");
    assert!(ok, "asserção não varrida");
    let visible = "frase varrida com ação";
}
#[cfg(test)]
mod tests {
    fn t() { let s = "dentro do teste não varrido"; }
}
fn b() { let raw = r#"crua "com aspas" e não"#; }
"##;
    let found: Vec<String> = production_literals(source)
        .into_iter()
        .map(|l| l.text)
        .collect();
    assert_eq!(
        found,
        vec![
            "frase varrida com ação".to_owned(),
            "crua \"com aspas\" e não".to_owned()
        ]
    );
}

#[test]
fn the_scanner_reports_the_right_line() {
    let source = "fn a() {}\n\nfn b() { let s = \"x\"; }\n";
    assert_eq!(
        production_literals(source),
        vec![Literal {
            line: 3,
            text: "x".to_owned()
        }]
    );
}

#[test]
fn the_heuristic_flags_accents_and_the_word_list_only() {
    assert!(looks_like_portuguese_prose("ação desconhecida"));
    assert!(looks_like_portuguese_prose("Fechar aba"));
    assert!(looks_like_portuguese_prose("nenhum resultado"));
    assert!(looks_like_portuguese_prose("NÃO"));
    // Palavra inteira: "abaixo" não é "aba", "grupos" não é "grupo".
    assert!(!looks_like_portuguese_prose("abaixo"));
    assert!(!looks_like_portuguese_prose("subgrupos"));
    // Identificadores e caminhos passam.
    assert!(!looks_like_portuguese_prose("tab_menu.close"));
    assert!(!looks_like_portuguese_prose(
        "../../../docs/config/porecatu.example.toml"
    ));
    assert!(!looks_like_portuguese_prose(
        "--config requires an argument"
    ));
}
