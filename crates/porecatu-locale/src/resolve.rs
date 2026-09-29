// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolvedor puro (ADR-0056 §6 e §8): lê os arquivos dos diretórios que
//! recebe e devolve o catálogo mais diagnósticos **tipados**, sem prosa. Não
//! toca `std::env` nem `current_exe` -- quem monta os diretórios é o chamador,
//! como em `resolve_config_path`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::catalog::Catalog;
use crate::layer::{Messages, Schema, parse_layer};
use crate::name::LocaleName;

/// A única frase de interface do projeto escrita no código (RF-15.12): sem
/// nenhum arquivo, não há de onde ler o texto do aviso que diz isso. Marcador
/// `{searched}`: os diretórios procurados, separados por vírgula.
pub const NO_CATALOG_MESSAGE: &str = "language files not found (searched: {searched})";

/// Algo que o chamador deve mostrar. Nenhuma variante carrega frase pronta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    /// Nenhum arquivo do idioma em nenhum diretório; vale a reserva.
    /// `requested` é o valor como foi escrito (ou `en_US`, quando é a reserva
    /// que falta).
    LanguageNotFound {
        requested: String,
        searched: Vec<PathBuf>,
    },
    /// `language` fora de `^[a-z]{2,3}_[A-Z]{2}$`; tratado como não achado.
    InvalidLanguageName { value: String },
    /// Erro de sintaxe; só aquele arquivo é descartado.
    LayerSyntax {
        path: PathBuf,
        line: Option<usize>,
        column: Option<usize>,
        detail: String,
    },
    /// O arquivo existe e não pôde ser lido.
    LayerUnreadable { path: PathBuf, cause: String },
    /// Frases do esquema ausentes ou inválidas no idioma, agregadas.
    MissingMessages { locale: LocaleName, count: usize },
    /// Chaves que o esquema não conhece, agregadas por arquivo.
    UnknownKeys { path: PathBuf, count: usize },
    /// Nem o idioma escolhido nem `en_US` em disco algum.
    NoCatalogAtAll { searched: Vec<PathBuf> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogOutcome {
    pub catalog: Catalog,
    /// Idioma que de fato vale: o escolhido, ou `en_US` quando cai na
    /// reserva. `None` sem nenhum arquivo.
    pub locale: Option<LocaleName>,
    pub diagnostics: Vec<Diagnostic>,
}

/// O que um idioma rendeu: se algum arquivo dele existia e as frases válidas
/// de `app ⊕ usuário`.
struct LocaleLoad {
    found: bool,
    messages: Messages,
}

/// Monta `resolve(en_US) ⊕ resolve(language)`, com `resolve(L) = app/L ⊕
/// user/L`. Cada camada falha sozinha. Diretório ausente (`None`, ou que não
/// existe) só não contribui.
pub fn build_catalog(
    language: &str,
    app_dir: Option<&Path>,
    user_dir: Option<&Path>,
    schema: &Schema,
) -> CatalogOutcome {
    let dirs: Vec<&Path> = app_dir.into_iter().chain(user_dir).collect();
    let searched = || dirs.iter().map(|dir| dir.to_path_buf()).collect();
    let mut diagnostics = Vec::new();

    let fallback = LocaleName::fallback();
    let chosen = match LocaleName::parse(language) {
        Ok(name) if name != fallback => Some(name),
        Ok(_) => None,
        Err(err) => {
            diagnostics.push(Diagnostic::InvalidLanguageName { value: err.value });
            None
        }
    };

    let fallback_load = load_locale(&fallback, &dirs, schema, &mut diagnostics);
    let chosen_load = chosen
        .as_ref()
        .map(|name| load_locale(name, &dirs, schema, &mut diagnostics));
    let chosen_found = chosen_load.as_ref().is_some_and(|load| load.found);

    if !fallback_load.found && !chosen_found {
        diagnostics.push(Diagnostic::NoCatalogAtAll {
            searched: searched(),
        });
        return CatalogOutcome {
            catalog: Catalog::new(),
            locale: None,
            diagnostics,
        };
    }

    if chosen.is_some() && !chosen_found {
        diagnostics.push(Diagnostic::LanguageNotFound {
            requested: language.to_owned(),
            searched: searched(),
        });
    }
    if !fallback_load.found {
        diagnostics.push(Diagnostic::LanguageNotFound {
            requested: fallback.as_str().to_owned(),
            searched: searched(),
        });
    }

    for (name, load) in [
        (Some(&fallback), Some(&fallback_load)),
        (chosen.as_ref(), chosen_load.as_ref()),
    ]
    .into_iter()
    .filter_map(|(name, load)| Some((name?, load?)))
    .filter(|(_, load)| load.found)
    {
        let count = schema
            .keys()
            .filter(|id| !load.messages.contains_key(**id))
            .count();
        if count > 0 {
            diagnostics.push(Diagnostic::MissingMessages {
                locale: name.clone(),
                count,
            });
        }
    }

    let locale = match chosen {
        Some(name) if chosen_found => name,
        _ => fallback,
    };
    let catalog = Catalog::from_layers(
        [Some(fallback_load), chosen_load]
            .into_iter()
            .flatten()
            .map(|load| load.messages),
    );
    CatalogOutcome {
        catalog,
        locale: Some(locale),
        diagnostics,
    }
}

fn load_locale(
    locale: &LocaleName,
    dirs: &[&Path],
    schema: &Schema,
    diagnostics: &mut Vec<Diagnostic>,
) -> LocaleLoad {
    // O nome já passou pela gramática: não carrega `/` nem `..`.
    let file_name = format!("{locale}.toml");
    let mut load = LocaleLoad {
        found: false,
        messages: Messages::new(),
    };
    for dir in dirs {
        let path = dir.join(&file_name);
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => {
                load.found = true;
                diagnostics.push(Diagnostic::LayerUnreadable {
                    path,
                    cause: err.to_string(),
                });
                continue;
            }
        };
        load.found = true;
        let outcome = parse_layer(&source, schema);
        if let Some(err) = outcome.syntax_error {
            diagnostics.push(Diagnostic::LayerSyntax {
                path,
                line: err.position.map(|(line, _)| line),
                column: err.position.map(|(_, column)| column),
                detail: err.detail,
            });
            continue;
        }
        if !outcome.unknown_keys.is_empty() {
            diagnostics.push(Diagnostic::UnknownKeys {
                path,
                count: outcome.unknown_keys.len(),
            });
        }
        load.messages.extend(outcome.messages);
    }
    load
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::MessageSpec;
    use crate::message::format;

    fn schema() -> Schema {
        Schema::from([
            ("tab_menu.new", MessageSpec::simple(&[])),
            ("tab_menu.close", MessageSpec::simple(&[])),
            ("group_menu.close", MessageSpec::plural(&["count"])),
        ])
    }

    const EN: &str = r#"
[tab_menu]
new = "New tab"
close = "Close tab"

[group_menu]
close = { one = "Close group ({count} tab)", other = "Close group ({count} tabs)" }
"#;

    const PT: &str = r#"
[tab_menu]
new = "Nova aba"
close = "Fechar aba"

[group_menu]
close = { one = "Fechar grupo ({count} aba)", other = "Fechar grupo ({count} abas)" }
"#;

    fn write(dir: &Path, file: &str, content: &str) {
        fs::create_dir_all(dir).expect("cria diretório de teste");
        fs::write(dir.join(file), content).expect("escreve arquivo de teste");
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("cria diretório temporário de teste")
    }

    fn text(outcome: &CatalogOutcome, id: &str) -> Option<String> {
        outcome.catalog.get(id).map(|m| m.select(1).to_owned())
    }

    /// Instalação completa: `en_US.toml` e `pt_BR.toml` no diretório do app.
    fn installed() -> tempfile::TempDir {
        let app = tempdir();
        write(app.path(), "en_US.toml", EN);
        write(app.path(), "pt_BR.toml", PT);
        app
    }

    #[test]
    fn chosen_language_wins_over_fallback() {
        let app = installed();
        let out = build_catalog("pt_BR", Some(app.path()), None, &schema());
        assert_eq!(out.diagnostics, []);
        assert_eq!(out.locale, Some(LocaleName::parse("pt_BR").unwrap()));
        assert_eq!(text(&out, "tab_menu.close").as_deref(), Some("Fechar aba"));
        let en = build_catalog("en_US", Some(app.path()), None, &schema());
        assert_eq!(text(&en, "tab_menu.close").as_deref(), Some("Close tab"));
        assert_eq!(en.diagnostics, []);
    }

    #[test]
    fn plural_comes_through_the_catalog() {
        let app = installed();
        let out = build_catalog("pt_BR", Some(app.path()), None, &schema());
        let message = out.catalog.get("group_menu.close").unwrap();
        assert_eq!(
            format(message.select(1), &[("count", "1")]),
            "Fechar grupo (1 aba)"
        );
        assert_eq!(
            format(message.select(3), &[("count", "3")]),
            "Fechar grupo (3 abas)"
        );
    }

    #[test]
    fn user_wins_over_installed_by_key() {
        let app = installed();
        let user = tempdir();
        write(
            user.path(),
            "pt_BR.toml",
            "[tab_menu]\nclose = \"Encerrar aba\"\n",
        );
        let out = build_catalog("pt_BR", Some(app.path()), Some(user.path()), &schema());
        assert_eq!(
            text(&out, "tab_menu.close").as_deref(),
            Some("Encerrar aba")
        );
        // A que o usuário não escreveu vem da instalada.
        assert_eq!(text(&out, "tab_menu.new").as_deref(), Some("Nova aba"));
        // Uma frase só no arquivo do usuário não gera "sem tradução": o
        // idioma, mesclado, está completo.
        assert_eq!(out.diagnostics, []);
    }

    #[test]
    fn missing_in_chosen_falls_back_to_en_us() {
        let app = tempdir();
        write(app.path(), "en_US.toml", EN);
        write(app.path(), "pt_BR.toml", "[tab_menu]\nnew = \"Nova aba\"\n");
        let out = build_catalog("pt_BR", Some(app.path()), None, &schema());
        assert_eq!(text(&out, "tab_menu.new").as_deref(), Some("Nova aba"));
        assert_eq!(text(&out, "tab_menu.close").as_deref(), Some("Close tab"));
        assert_eq!(
            out.diagnostics,
            [Diagnostic::MissingMessages {
                locale: LocaleName::parse("pt_BR").unwrap(),
                count: 2,
            }]
        );
    }

    #[test]
    fn missing_everywhere_is_none() {
        let app = tempdir();
        write(app.path(), "en_US.toml", "[tab_menu]\nnew = \"New tab\"\n");
        let out = build_catalog("en_US", Some(app.path()), None, &schema());
        assert_eq!(out.catalog.get("tab_menu.close"), None);
        assert_eq!(
            out.diagnostics,
            [Diagnostic::MissingMessages {
                locale: LocaleName::fallback(),
                count: 2,
            }]
        );
    }

    #[test]
    fn missing_messages_is_one_aggregate_per_language() {
        let app = tempdir();
        write(app.path(), "en_US.toml", "[tab_menu]\nnew = \"x\"\n");
        write(app.path(), "de_DE.toml", "[tab_menu]\nnew = \"y\"\n");
        let out = build_catalog("de_DE", Some(app.path()), None, &schema());
        let aggregates: Vec<_> = out
            .diagnostics
            .iter()
            .filter(|d| matches!(d, Diagnostic::MissingMessages { .. }))
            .collect();
        assert_eq!(
            aggregates,
            [
                &Diagnostic::MissingMessages {
                    locale: LocaleName::fallback(),
                    count: 2
                },
                &Diagnostic::MissingMessages {
                    locale: LocaleName::parse("de_DE").unwrap(),
                    count: 2
                },
            ]
        );
    }

    #[test]
    fn invalid_phrases_count_as_missing() {
        let app = tempdir();
        write(app.path(), "en_US.toml", EN);
        write(
            app.path(),
            "pt_BR.toml",
            "[tab_menu]\nnew = \"Nova {aba}\"\nclose = \"Fechar aba\"\n",
        );
        let out = build_catalog("pt_BR", Some(app.path()), None, &schema());
        assert_eq!(text(&out, "tab_menu.new").as_deref(), Some("New tab"));
        assert_eq!(
            out.diagnostics,
            [Diagnostic::MissingMessages {
                locale: LocaleName::parse("pt_BR").unwrap(),
                count: 2,
            }]
        );
    }

    #[test]
    fn broken_user_layer_keeps_the_installed_one() {
        let app = installed();
        let user = tempdir();
        write(
            user.path(),
            "pt_BR.toml",
            "[tab_menu]\nclose = \"ok\"\nnew = = \"x\"\n",
        );
        let out = build_catalog("pt_BR", Some(app.path()), Some(user.path()), &schema());
        assert_eq!(text(&out, "tab_menu.close").as_deref(), Some("Fechar aba"));
        assert_eq!(
            out.diagnostics,
            [Diagnostic::LayerSyntax {
                path: user.path().join("pt_BR.toml"),
                line: Some(3),
                column: Some(7),
                detail: match &out.diagnostics[0] {
                    Diagnostic::LayerSyntax { detail, .. } => detail.clone(),
                    other => panic!("{other:?}"),
                },
            }]
        );
    }

    #[test]
    fn broken_installed_layer_keeps_the_user_one() {
        let app = tempdir();
        write(app.path(), "en_US.toml", EN);
        write(app.path(), "pt_BR.toml", "not = = toml");
        let user = tempdir();
        write(
            user.path(),
            "pt_BR.toml",
            "[tab_menu]\nclose = \"Encerrar aba\"\n",
        );
        let out = build_catalog("pt_BR", Some(app.path()), Some(user.path()), &schema());
        assert_eq!(
            text(&out, "tab_menu.close").as_deref(),
            Some("Encerrar aba")
        );
        assert_eq!(text(&out, "tab_menu.new").as_deref(), Some("New tab"));
    }

    #[test]
    fn unreadable_layer_is_reported_and_others_survive() {
        let app = installed();
        let user = tempdir();
        // Um diretório no lugar do arquivo: existe, e não se lê como texto.
        fs::create_dir(user.path().join("pt_BR.toml")).unwrap();
        let out = build_catalog("pt_BR", Some(app.path()), Some(user.path()), &schema());
        assert_eq!(text(&out, "tab_menu.close").as_deref(), Some("Fechar aba"));
        assert_eq!(out.diagnostics.len(), 1);
        assert!(matches!(
            &out.diagnostics[0],
            Diagnostic::LayerUnreadable { path, .. } if *path == user.path().join("pt_BR.toml")
        ));
    }

    #[test]
    fn unknown_keys_are_aggregated_per_file() {
        let app = tempdir();
        write(
            app.path(),
            "en_US.toml",
            &format!("{EN}\n[extra]\na = \"1\"\nb = \"2\"\n"),
        );
        let out = build_catalog("en_US", Some(app.path()), None, &schema());
        assert_eq!(
            out.diagnostics,
            [Diagnostic::UnknownKeys {
                path: app.path().join("en_US.toml"),
                count: 2,
            }]
        );
        assert_eq!(text(&out, "tab_menu.new").as_deref(), Some("New tab"));
    }

    #[test]
    fn nonexistent_language_falls_back_with_searched_dirs() {
        let app = installed();
        let user = tempdir();
        let out = build_catalog("fr_FR", Some(app.path()), Some(user.path()), &schema());
        assert_eq!(out.locale, Some(LocaleName::fallback()));
        assert_eq!(text(&out, "tab_menu.close").as_deref(), Some("Close tab"));
        assert_eq!(
            out.diagnostics,
            [Diagnostic::LanguageNotFound {
                requested: "fr_FR".to_owned(),
                searched: vec![app.path().to_path_buf(), user.path().to_path_buf()],
            }]
        );
    }

    #[test]
    fn nothing_on_disk_is_no_catalog_at_all() {
        let app = tempdir();
        let user = tempdir();
        let out = build_catalog("pt_BR", Some(app.path()), Some(user.path()), &schema());
        assert!(out.catalog.is_empty());
        assert_eq!(out.locale, None);
        assert_eq!(
            out.diagnostics,
            [Diagnostic::NoCatalogAtAll {
                searched: vec![app.path().to_path_buf(), user.path().to_path_buf()],
            }]
        );
    }

    #[test]
    fn no_directories_at_all_is_no_catalog_at_all() {
        let out = build_catalog("en_US", None, None, &schema());
        assert_eq!(
            out.diagnostics,
            [Diagnostic::NoCatalogAtAll { searched: vec![] }]
        );
    }

    #[test]
    fn missing_directories_do_not_fail() {
        let base = tempdir();
        let out = build_catalog(
            "en_US",
            Some(&base.path().join("no-app")),
            Some(&base.path().join("no-user")),
            &schema(),
        );
        assert!(matches!(
            out.diagnostics.as_slice(),
            [Diagnostic::NoCatalogAtAll { searched }] if searched.len() == 2
        ));
    }

    #[test]
    fn invalid_name_falls_back_and_never_touches_disk_paths() {
        let app = installed();
        let out = build_catalog("../x", Some(app.path()), None, &schema());
        assert_eq!(out.locale, Some(LocaleName::fallback()));
        assert_eq!(text(&out, "tab_menu.close").as_deref(), Some("Close tab"));
        assert_eq!(
            out.diagnostics,
            [Diagnostic::InvalidLanguageName {
                value: "../x".to_owned()
            }]
        );
    }

    #[test]
    fn invalid_name_with_nothing_on_disk_reports_both() {
        let app = tempdir();
        let out = build_catalog("pt-BR", Some(app.path()), None, &schema());
        assert_eq!(
            out.diagnostics,
            [
                Diagnostic::InvalidLanguageName {
                    value: "pt-BR".to_owned()
                },
                Diagnostic::NoCatalogAtAll {
                    searched: vec![app.path().to_path_buf()]
                },
            ]
        );
    }

    #[test]
    fn language_only_in_user_dir_is_valid() {
        let app = installed();
        let user = tempdir();
        write(
            user.path(),
            "de_DE.toml",
            &EN.replace("New tab", "Neuer Tab"),
        );
        let out = build_catalog("de_DE", Some(app.path()), Some(user.path()), &schema());
        assert_eq!(text(&out, "tab_menu.new").as_deref(), Some("Neuer Tab"));
        assert_eq!(out.locale, Some(LocaleName::parse("de_DE").unwrap()));
        assert_eq!(out.diagnostics, []);
    }

    #[test]
    fn missing_fallback_file_is_reported_when_chosen_language_loads() {
        let app = tempdir();
        write(app.path(), "pt_BR.toml", PT);
        let out = build_catalog("pt_BR", Some(app.path()), None, &schema());
        assert_eq!(text(&out, "tab_menu.close").as_deref(), Some("Fechar aba"));
        assert_eq!(
            out.diagnostics,
            [Diagnostic::LanguageNotFound {
                requested: "en_US".to_owned(),
                searched: vec![app.path().to_path_buf()],
            }]
        );
    }

    #[test]
    fn no_catalog_message_has_a_searched_placeholder() {
        assert_eq!(
            format(NO_CATALOG_MESSAGE, &[("searched", "/a, /b")]),
            "language files not found (searched: /a, /b)"
        );
    }
}
