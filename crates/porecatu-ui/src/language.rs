// SPDX-License-Identifier: GPL-3.0-or-later

//! Idioma da interface: onde procurar os arquivos (ADR-0056 §6) e como os
//! diagnósticos do catálogo viram avisos do canal 1 (§8).
//!
//! `porecatu-locale` é puro -- recebe os diretórios por argumento e nunca
//! lê `std::env` nem `current_exe`. Quem sabe onde o app está instalado é
//! este módulo, e a escolha de candidatos é uma função pura (recebe o
//! caminho do executável, o valor do ambiente, a plataforma e a flag de
//! debug), no mesmo desenho de `resolve_config_path`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use porecatu_locale::{
    Catalog, CatalogOutcome, Diagnostic, LocaleName, NO_CATALOG_MESSAGE, build_catalog,
};

use crate::keymap::Platform;
use crate::messages::{msg, schema};
use crate::warning::Severity;

/// Costura de teste e de desenvolvimento, **não contrato público** -- mesmo
/// estatuto de `PORECATU_SESSION` (ADR-0056 §6). Um build de release fora de
/// instalação (o `--target-dir` separado do CLAUDE.md) precisa dela.
pub(crate) const LOCALES_ENV: &str = "PORECATU_LOCALES";

/// Nome do diretório de idiomas, ao lado do `porecatu.toml` e dentro de cada
/// instalação.
const LOCALES_DIR_NAME: &str = "locales";

/// Candidato de `cargo run` no repositório (ADR-0056 §6, ordem 5): um
/// **caminho**, não um conteúdo -- o arquivo continua sendo lido do disco.
/// Só entra na lista em build de debug.
const DEV_LOCALES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales");

/// Diretórios candidatos do app, na ordem de preferência (ADR-0056 §6).
/// `exe` é `canonicalize(current_exe())`; os relativos partem de `exe.parent()`.
pub(crate) fn app_dir_candidates(
    exe: &Path,
    env_value: Option<&OsStr>,
    platform: Platform,
    debug: bool,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(value) = env_value.filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(value));
    }
    if let Some(exe_dir) = exe.parent() {
        let beside_exe = exe_dir.join(LOCALES_DIR_NAME);
        match platform {
            Platform::Windows => candidates.push(beside_exe),
            Platform::Linux => {
                if let Some(prefix) = exe_dir.parent() {
                    candidates.push(prefix.join("share").join("porecatu").join(LOCALES_DIR_NAME));
                }
                candidates.push(beside_exe);
            }
            Platform::Macos => {
                if let Some(contents) = exe_dir.parent() {
                    candidates.push(contents.join("Resources").join(LOCALES_DIR_NAME));
                }
                candidates.push(beside_exe);
            }
        }
    }
    if debug {
        candidates.push(PathBuf::from(DEV_LOCALES_DIR));
    }
    candidates
}

/// O primeiro candidato que existe como diretório.
pub(crate) fn first_existing(
    candidates: Vec<PathBuf>,
    is_dir: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| is_dir(candidate))
}

/// `locales/` ao lado do **caminho resolvido** do `porecatu.toml` -- derivado,
/// não resolvido de novo (mesmo raciocínio de `sessions/`, ADR-0054 §2), então
/// `--config` e `PORECATU_CONFIG` o deslocam junto. O app nunca cria a pasta.
pub(crate) fn user_dir(config_path: Option<&Path>) -> Option<PathBuf> {
    Some(config_path?.parent()?.join(LOCALES_DIR_NAME))
}

/// O diretório do app neste processo, ou `None` quando nenhum candidato
/// existe.
fn app_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let env_value = std::env::var_os(LOCALES_ENV);
    let candidates = app_dir_candidates(
        &exe,
        env_value.as_deref(),
        Platform::current(),
        cfg!(debug_assertions),
    );
    first_existing(candidates, Path::is_dir)
}

/// Os diretórios de idioma que existem neste processo -- o do app e o do
/// usuário --, os mesmos que `load_catalog` procura. A tela de configurações
/// lista os arquivos deles (RF-16.12).
pub(crate) fn locale_dirs(config_path: Option<&Path>) -> Vec<PathBuf> {
    [app_dir(), user_dir(config_path)]
        .into_iter()
        .flatten()
        .collect()
}

/// Os idiomas que `dirs` têm, pelo nome do arquivo (`pt_BR` de `pt_BR.toml`),
/// sem repetição e em ordem alfabética. Arquivo cujo nome não é um idioma
/// válido (`LocaleName`) fica de fora, como o carregador o ignoraria.
/// Diretório ilegível ou ausente conta como vazio.
pub(crate) fn available_languages(dirs: &[PathBuf]) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(OsStr::to_str) != Some("toml") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(OsStr::to_str)
                && LocaleName::parse(stem).is_ok()
            {
                names.insert(stem.to_owned());
            }
        }
    }
    names.into_iter().collect()
}

/// Monta o catálogo do processo: `language` da config, os dois diretórios
/// reais, o esquema do registro.
pub(crate) fn load_catalog(language: &str, config_path: Option<&Path>) -> CatalogOutcome {
    build_catalog(
        language,
        app_dir().as_deref(),
        user_dir(config_path).as_deref(),
        &schema(),
    )
}

/// Etiqueta BCP 47 do idioma **efetivamente carregado** (ADR-0056 §11), para
/// a raiz da árvore de acessibilidade. Quando o pedido caiu na reserva, é a
/// da reserva (`en-US`), não a do que foi pedido; sem catálogo nenhum, também
/// -- a única frase que resta é em inglês.
pub(crate) fn language_tag(locale: Option<&LocaleName>) -> String {
    locale.map_or_else(|| LocaleName::fallback().bcp47(), LocaleName::bcp47)
}

/// A frase fixa do ADR-0056 §8, com os diretórios procurados. É a única
/// frase de interface escrita no código: sem nenhum arquivo, não há de onde
/// ler o texto que diz isso.
pub(crate) fn no_catalog_text(searched: &[PathBuf]) -> String {
    let searched = searched
        .iter()
        .map(|dir| dir.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    porecatu_locale::format(NO_CATALOG_MESSAGE, &[("searched", &searched)])
}

/// Título do aviso da frase fixa: nome próprio, então fica igual em todo
/// idioma (como o rótulo da raiz da árvore de acessibilidade).
const NO_CATALOG_TITLE: &str = "Porecatu";

/// Severidade, título e corpo do aviso de um diagnóstico do catálogo
/// (ADR-0056 §8). Sai no idioma do `catalog` que carregou.
pub(crate) fn diagnostic_notice(
    diagnostic: &Diagnostic,
    catalog: &Catalog,
) -> (Severity, String, String) {
    match diagnostic {
        Diagnostic::LanguageNotFound {
            requested,
            searched,
        } => (
            Severity::Warning,
            msg::notice::language_not_found::title(catalog),
            msg::notice::language_not_found::body(catalog, requested, list_dirs(searched)),
        ),
        Diagnostic::InvalidLanguageName { value } => (
            Severity::Warning,
            msg::notice::language_invalid_name::title(catalog),
            msg::notice::language_invalid_name::body(catalog, value),
        ),
        Diagnostic::LayerSyntax {
            path,
            line,
            column,
            detail,
        } => {
            let path = path.display();
            let body = match (line, column) {
                (Some(line), Some(column)) => {
                    msg::notice::language_syntax::body_at(catalog, path, line, column, detail)
                }
                _ => msg::notice::language_syntax::body(catalog, path, detail),
            };
            (
                Severity::Error,
                msg::notice::language_syntax::title(catalog),
                body,
            )
        }
        Diagnostic::LayerUnreadable { path, cause } => (
            Severity::Error,
            msg::notice::language_unreadable::title(catalog),
            msg::notice::language_unreadable::body(catalog, path.display(), cause),
        ),
        Diagnostic::MissingMessages { locale, count } => (
            // Informação agregada: expira sozinha (RF-10.16). Um idioma do
            // usuário fica incompleto a cada versão que acrescenta frases, e
            // isso não pode virar um aviso a dispensar em todo arranque.
            Severity::Info,
            msg::notice::language_missing_messages::title(catalog),
            msg::notice::language_missing_messages::body(catalog, *count, locale),
        ),
        Diagnostic::UnknownKeys { path, count } => (
            Severity::Warning,
            msg::notice::language_unknown_keys::title(catalog),
            msg::notice::language_unknown_keys::body(catalog, *count, path.display()),
        ),
        Diagnostic::NoCatalogAtAll { searched } => (
            Severity::Error,
            NO_CATALOG_TITLE.to_owned(),
            no_catalog_text(searched),
        ),
    }
}

fn list_dirs(dirs: &[PathBuf]) -> String {
    dirs.iter()
        .map(|dir| dir.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::messages::test_support;

    fn exe() -> PathBuf {
        PathBuf::from("/opt/porecatu/bin/porecatu")
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "porecatu-language-test-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn available_languages_lists_valid_locale_files_once_and_sorted() {
        let app = scratch_dir("app");
        let user = scratch_dir("user");
        for name in [
            "pt_BR.toml",
            "en_US.toml",
            "notes.txt",
            "Bad.toml",
            "pt.toml",
        ] {
            std::fs::write(app.join(name), "").unwrap();
        }
        // O usuário repete um e traz outro.
        for name in ["en_US.toml", "es_ES.toml"] {
            std::fs::write(user.join(name), "").unwrap();
        }
        let missing = std::env::temp_dir().join("porecatu-language-test-missing-dir");
        let found = available_languages(&[app.clone(), missing, user.clone()]);
        assert_eq!(found, ["en_US", "es_ES", "pt_BR"]);
        std::fs::remove_dir_all(app).unwrap();
        std::fs::remove_dir_all(user).unwrap();
    }

    #[test]
    fn env_value_comes_first_and_empty_is_ignored() {
        let with_env = app_dir_candidates(
            &exe(),
            Some(OsStr::new("/somewhere/locales")),
            Platform::Windows,
            false,
        );
        assert_eq!(with_env[0], PathBuf::from("/somewhere/locales"));

        let empty = app_dir_candidates(&exe(), Some(OsStr::new("")), Platform::Windows, false);
        assert_eq!(empty, vec![exe().parent().unwrap().join("locales")]);
    }

    #[test]
    fn windows_looks_only_beside_the_exe() {
        let candidates = app_dir_candidates(&exe(), None, Platform::Windows, false);
        assert_eq!(candidates, vec![PathBuf::from("/opt/porecatu/bin/locales")]);
    }

    #[test]
    fn linux_prefers_share_then_beside_the_exe() {
        let candidates = app_dir_candidates(&exe(), None, Platform::Linux, false);
        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/opt/porecatu/share/porecatu/locales"),
                PathBuf::from("/opt/porecatu/bin/locales"),
            ]
        );
    }

    #[test]
    fn macos_prefers_resources_then_beside_the_exe() {
        let exe = PathBuf::from("/Applications/Porecatu.app/Contents/MacOS/porecatu");
        let candidates = app_dir_candidates(&exe, None, Platform::Macos, false);
        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/Applications/Porecatu.app/Contents/Resources/locales"),
                PathBuf::from("/Applications/Porecatu.app/Contents/MacOS/locales"),
            ]
        );
    }

    #[test]
    fn the_repository_path_is_a_debug_only_last_resort() {
        let debug = app_dir_candidates(&exe(), None, Platform::Windows, true);
        assert_eq!(debug.last().unwrap(), &PathBuf::from(DEV_LOCALES_DIR));
        assert!(DEV_LOCALES_DIR.ends_with("/../../locales"));

        let release = app_dir_candidates(&exe(), None, Platform::Windows, false);
        assert!(!release.contains(&PathBuf::from(DEV_LOCALES_DIR)));
    }

    #[test]
    fn the_first_candidate_that_exists_wins() {
        let candidates = vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/c"),
        ];
        let picked = first_existing(candidates.clone(), |dir| {
            dir == Path::new("/b") || dir == Path::new("/c")
        });
        assert_eq!(picked, Some(PathBuf::from("/b")));
        assert_eq!(first_existing(candidates, |_| false), None);
    }

    #[test]
    fn user_dir_sits_beside_the_resolved_config_file() {
        assert_eq!(
            user_dir(Some(Path::new("/home/ana/.config/porecatu/porecatu.toml"))),
            Some(PathBuf::from("/home/ana/.config/porecatu/locales"))
        );
        assert_eq!(user_dir(None), None);
    }

    #[test]
    fn the_language_tag_is_the_bcp47_form_of_the_loaded_locale() {
        let pt = LocaleName::parse("pt_BR").unwrap();
        assert_eq!(language_tag(Some(&pt)), "pt-BR");
        assert_eq!(language_tag(Some(&LocaleName::fallback())), "en-US");
        // Sem nenhum arquivo, sobra a frase em inglês.
        assert_eq!(language_tag(None), "en-US");
    }

    fn notice(diagnostic: &Diagnostic, catalog: &Catalog) -> (Severity, String, String) {
        diagnostic_notice(diagnostic, catalog)
    }

    /// A tabela do ADR-0056 §8: severidade de cada caso.
    #[test]
    fn diagnostics_have_the_severity_the_adr_assigns() {
        let en = test_support::en_us();
        let path = PathBuf::from("pt_BR.toml");
        let cases = [
            (
                Diagnostic::LanguageNotFound {
                    requested: "fr_FR".to_owned(),
                    searched: vec![PathBuf::from("/a")],
                },
                Severity::Warning,
            ),
            (
                Diagnostic::InvalidLanguageName {
                    value: "pt_br".to_owned(),
                },
                Severity::Warning,
            ),
            (
                Diagnostic::LayerSyntax {
                    path: path.clone(),
                    line: Some(3),
                    column: Some(4),
                    detail: "x".to_owned(),
                },
                Severity::Error,
            ),
            (
                Diagnostic::LayerUnreadable {
                    path: path.clone(),
                    cause: "x".to_owned(),
                },
                Severity::Error,
            ),
            (
                Diagnostic::MissingMessages {
                    locale: porecatu_locale::LocaleName::parse("pt_BR").unwrap(),
                    count: 4,
                },
                // Informação agregada: expira sozinha em 6 s.
                Severity::Info,
            ),
            (
                Diagnostic::UnknownKeys {
                    path: path.clone(),
                    count: 2,
                },
                Severity::Warning,
            ),
            (
                Diagnostic::NoCatalogAtAll {
                    searched: vec![PathBuf::from("/a")],
                },
                Severity::Error,
            ),
        ];
        for (diagnostic, severity) in cases {
            assert_eq!(notice(&diagnostic, &en).0, severity, "{diagnostic:?}");
        }
    }

    #[test]
    fn language_notices_come_out_in_the_language_of_the_catalog() {
        let syntax = Diagnostic::LayerSyntax {
            path: PathBuf::from("pt_BR.toml"),
            line: Some(3),
            column: Some(4),
            detail: "expected an equals".to_owned(),
        };
        let (_, title, body) = notice(&syntax, &test_support::en_us());
        assert_eq!(title, "Language file error");
        assert_eq!(body, "pt_BR.toml, line 3, column 4: expected an equals");

        let (_, title, body) = notice(&syntax, &test_support::pt_br());
        assert_eq!(title, "Erro no arquivo de idioma");
        assert_eq!(body, "pt_BR.toml, linha 3, coluna 4: expected an equals");

        let no_position = Diagnostic::LayerSyntax {
            path: PathBuf::from("pt_BR.toml"),
            line: None,
            column: None,
            detail: "boom".to_owned(),
        };
        let (_, _, body) = notice(&no_position, &test_support::en_us());
        assert_eq!(body, "pt_BR.toml: boom");
    }

    #[test]
    fn the_untranslated_count_pluralizes() {
        let locale = porecatu_locale::LocaleName::parse("pt_BR").unwrap();
        let en = test_support::en_us();
        let one = Diagnostic::MissingMessages {
            locale: locale.clone(),
            count: 1,
        };
        assert_eq!(notice(&one, &en).2, "1 text without translation in pt_BR");
        let many = Diagnostic::MissingMessages { locale, count: 7 };
        assert_eq!(notice(&many, &en).2, "7 texts without translation in pt_BR");
        assert_eq!(
            notice(&many, &test_support::pt_br()).2,
            "7 textos sem tradução em pt_BR"
        );
    }

    #[test]
    fn a_language_that_was_not_found_names_the_value_and_the_directories() {
        let missing = Diagnostic::LanguageNotFound {
            requested: "fr_FR".to_owned(),
            searched: vec![
                PathBuf::from("/app/locales"),
                PathBuf::from("/user/locales"),
            ],
        };
        let (_, _, body) = notice(&missing, &test_support::en_us());
        assert!(body.contains("\"fr_FR\""), "{body}");
        assert!(
            body.contains(&format!(
                "{}, {}",
                PathBuf::from("/app/locales").display(),
                PathBuf::from("/user/locales").display()
            )),
            "{body}"
        );
    }

    /// Sem catálogo nenhum, o aviso usa a frase fixa em inglês -- a única do
    /// código -- e o título é o nome do app, que não se traduz.
    #[test]
    fn no_catalog_at_all_uses_the_fixed_phrase() {
        let none = Diagnostic::NoCatalogAtAll {
            searched: vec![PathBuf::from("/a")],
        };
        let (severity, title, body) = notice(&none, &Catalog::new());
        assert_eq!(severity, Severity::Error);
        assert_eq!(title, "Porecatu");
        assert_eq!(body, no_catalog_text(&[PathBuf::from("/a")]));
        assert!(body.starts_with("language files not found (searched: "));
    }

    #[test]
    fn the_only_hardcoded_phrase_lists_the_searched_directories() {
        let text = no_catalog_text(&[PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(
            text,
            format!(
                "language files not found (searched: {}, {})",
                PathBuf::from("/a").display(),
                PathBuf::from("/b").display()
            )
        );
    }
}
