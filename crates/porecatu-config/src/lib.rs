// SPDX-License-Identifier: GPL-3.0-or-later

//! Configuração do Porecatu (ADR-0003). `Config` espelha a árvore de
//! `docs/config/porecatu.example.toml`, com defaults completos: esse
//! arquivo lista os mesmos valores que `Config::default()` produz, e é o
//! que a etapa verifica (ver `tests/example_toml.rs`).
//!
//! **Sem consumidor nesta etapa.** `porecatu-ui` passa a ler `Config` na
//! etapa 2 da F4; aqui a superfície é carregar, validar e comparar por
//! igualdade -- é o que a torna testável sem GPU e sem janela.
//!
//! Hot reload (etapa 4), o enum `Action` e o parser de `[keybindings]`
//! (etapa 5) e o merge de temas (etapa 6) não vivem aqui: `[keybindings]`
//! é só um mapa de string para string preservado, e cada `[[themes]]` é
//! uma árvore independente de overrides, nunca aplicada.

mod appearance;
mod color;
mod edit;
mod error;
mod general;
mod git;
mod keybindings;
mod panes;
mod path;
mod project_file;
mod session;
mod shell;
mod terminal;
mod theme;

pub use appearance::{
    Appearance, CloseButtonVisibility, ContextMenu, Dialog, GroupEditor, GroupPaletteEntry, Groups,
    MoveToGroup, Notices, StatusBar, Tabs, TabsColors, TabsOverflow, TabsRename, TerminalFrame,
    Tooltip, Window, WindowControls,
};
pub use color::{Color, ColorParseError};
pub use edit::{ConfigDocument, Edit, EditError, EditValue, KeyPath, SaveOutcome};
pub use error::{ConfigError, ConfigErrorKind};
pub use general::General;
pub use git::Git;
pub use keybindings::Keybindings;
pub use panes::Panes;
pub use path::resolve_config_path;
pub use project_file::{
    PROJECT_FILE_NAME, ProjectFile, ProjectFileOutcome, ProjectScript, is_trusted,
    parse as parse_project_file, resolve as resolve_project_file,
};
pub use session::Session;
pub use shell::Shell;
pub use terminal::{
    AnsiPalette, BackgroundImage, BackgroundImageMode, Clipboard, Colors as TerminalColors, Cursor,
    CursorShape, Font, Scrollback, Selection, Terminal, ZoomScope, resolve_background_image_path,
};
pub use theme::{
    Theme, ThemeAnsiPalette, ThemeContextMenu, ThemeDialog, ThemeGroupEditor, ThemeGroups,
    ThemeNotices, ThemeStatusBar, ThemeTooltip, apply as apply_theme,
    overridden_keys as theme_overridden_keys,
};

use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub shell: Shell,
    pub appearance: Appearance,
    pub terminal: Terminal,
    pub themes: Vec<Theme>,
    pub keybindings: Keybindings,
    pub session: Session,
    pub project_file: ProjectFile,
    pub git: Git,
    pub panes: Panes,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: General::default(),
            shell: Shell::default(),
            appearance: Appearance::default(),
            terminal: Terminal::default(),
            themes: theme::built_in_themes(),
            keybindings: Keybindings::default(),
            session: Session::default(),
            project_file: ProjectFile::default(),
            git: Git::default(),
            panes: Panes::default(),
        }
    }
}

/// Resultado de carregar a config no início do processo (ADR-0003 regra
/// 2). `config` está sempre presente e pronto para uso em qualquer
/// variante -- inclusive `Invalid`, onde é `Config::default()`: o chamador
/// decide o que fazer com o erro (mostrar aviso, por exemplo), a config
/// nunca fica pela metade.
///
/// Uma exceção ao "defaults inteiros" em `Invalid`: `general.language` é lido
/// do texto quando ele é TOML sintaticamente válido (ADR-0056 §7), para que
/// um erro de digitação na config não ponha em inglês quem escolheu outro
/// idioma. Sintaxe quebrada, ou arquivo ilegível, não deixa o que ler.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadResult {
    /// Nenhum arquivo no caminho resolvido -- estado válido (ADR-0003
    /// regra 1).
    Missing { config: Config },
    /// Arquivo lido e parseado com sucesso. `unknown_keys` são os avisos
    /// de chave desconhecida (ADR-0003 regra 4), em caminho com pontos
    /// (ex.: `"appearance.tabs.foo"`).
    Loaded {
        config: Config,
        unknown_keys: Vec<String>,
    },
    /// Arquivo presente mas inválido -- sintaticamente ou semanticamente
    /// (ADR-0003 regra 2: config inválida no start devolve o erro **e**
    /// os defaults).
    Invalid { config: Config, error: ConfigError },
}

impl LoadResult {
    /// A config a usar, em qualquer variante -- `Config::default()` nos
    /// casos `Missing` e `Invalid`.
    pub fn config(&self) -> &Config {
        match self {
            Self::Missing { config }
            | Self::Loaded { config, .. }
            | Self::Invalid { config, .. } => config,
        }
    }
}

/// Resolve o caminho, lê e parseia a config no início do processo.
/// `cli_config` é o valor de `--config <caminho>`, se fornecido.
pub fn load(cli_config: Option<&Path>) -> LoadResult {
    let Some(path) = resolve_config_path(cli_config) else {
        return LoadResult::Missing {
            config: Config::default(),
        };
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return LoadResult::Missing {
                config: Config::default(),
            };
        }
        Err(err) => {
            return LoadResult::Invalid {
                config: Config::default(),
                error: ConfigError::new(ConfigErrorKind::Unreadable {
                    path,
                    cause: err.to_string(),
                }),
            };
        }
    };
    match parse(&text) {
        Ok((config, unknown_keys)) => LoadResult::Loaded {
            config,
            unknown_keys,
        },
        Err(error) => {
            let mut config = Config::default();
            if let Some(language) = raw_language(&text) {
                config.general.language = language;
            }
            LoadResult::Invalid { config, error }
        }
    }
}

/// `general.language` de um parse cru (`toml::Table`), sem desserializar a
/// config: é o que sobra de legível quando a desserialização falhou em outro
/// campo. `None` com sintaxe quebrada ou quando a chave não é texto.
fn raw_language(text: &str) -> Option<String> {
    let table: toml::Table = text.parse().ok()?;
    table
        .get("general")?
        .get("language")?
        .as_str()
        .map(str::to_owned)
}

/// Parseia o texto de uma config já lida -- usado por `load` acima e pelo
/// hot reload (etapa 4, que já tem o texto em mãos após o evento do
/// `notify`). Devolve a config carregada mais as chaves desconhecidas
/// (ADR-0003 regra 4), ou o primeiro erro localizado (regra 3): TOML
/// sintaticamente inválido, tipo errado num campo (inclusive cor
/// inválida, RF-4.9) ou nome de tema duplicado.
pub fn parse(text: &str) -> Result<(Config, Vec<String>), ConfigError> {
    let deserializer =
        toml::de::Deserializer::parse(text).map_err(|err| ConfigError::from_toml(text, err))?;
    let mut unknown_keys = Vec::new();
    let config: Config =
        serde_ignored::deserialize(deserializer, |path| unknown_keys.push(path.to_string()))
            .map_err(|err| ConfigError::from_toml(text, err))?;

    if let Some(name) = theme::find_duplicate_name(&config.themes) {
        return Err(ConfigError::new(ConfigErrorKind::DuplicateThemeName {
            name: name.to_owned(),
        }));
    }

    Ok((config, unknown_keys))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_defaults() {
        let (config, unknown_keys) = parse("").expect("toml vazio é válido");
        assert_eq!(config, Config::default());
        assert!(unknown_keys.is_empty());
    }

    #[test]
    fn syntax_error_is_localized() {
        let err = parse("this is not toml").unwrap_err();
        assert!(err.line.is_some());
        assert!(err.column.is_some());
        assert!(matches!(err.kind, ConfigErrorKind::Toml { .. }));
    }

    #[test]
    fn unknown_key_is_collected_not_rejected() {
        let (config, unknown_keys) =
            parse("[general]\nfoo = 1\n").expect("chave desconhecida é aviso");
        assert_eq!(config.general, General::default());
        assert_eq!(unknown_keys, vec!["general.foo".to_owned()]);
    }

    #[test]
    fn invalid_color_is_localized_error() {
        let err = parse("[appearance.window]\nbackground = \"not-a-color\"\n").unwrap_err();
        assert!(err.line.is_some());
        let ConfigErrorKind::Toml { detail } = &err.kind else {
            panic!("expected a Toml error, got {:?}", err.kind);
        };
        assert!(detail.contains("invalid color \"not-a-color\""));
        assert!(detail.contains("expected \"#rrggbb\", \"#rrggbbaa\" or \"transparent\""));
    }

    #[test]
    fn duplicate_theme_name_is_error() {
        let text = r#"
            [[themes]]
            name = "x"
            [[themes]]
            name = "x"
        "#;
        let err = parse(text).unwrap_err();
        assert_eq!(
            err.kind,
            ConfigErrorKind::DuplicateThemeName {
                name: "x".to_owned()
            }
        );
        assert_eq!((err.line, err.column), (None, None));
    }

    #[test]
    fn missing_file_is_defaults() {
        let result = load_from_nonexistent_path();
        assert_eq!(result.config(), &Config::default());
        assert!(matches!(result, LoadResult::Missing { .. }));
    }

    #[test]
    fn unreadable_file_is_unreadable_error() {
        // A directory where the file should be: exists, but cannot be read
        // as text, and the error is not `NotFound`.
        let dir = tempfile::tempdir().unwrap();
        let result = load(Some(dir.path()));
        let LoadResult::Invalid { config, error } = result else {
            panic!("expected Invalid, got {result:?}");
        };
        assert_eq!(config, Config::default());
        assert_eq!((error.line, error.column), (None, None));
        let ConfigErrorKind::Unreadable { path, cause } = error.kind else {
            panic!("expected Unreadable, got {:?}", error.kind);
        };
        assert_eq!(path, dir.path());
        assert!(!cause.is_empty());
    }

    fn load_text(text: &str) -> LoadResult {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("porecatu.toml");
        std::fs::write(&path, text).unwrap();
        load(Some(&path))
    }

    #[test]
    fn invalid_config_with_valid_syntax_keeps_the_language() {
        // Deserialization fails on the color; the TOML itself is fine.
        let result = load_text(
            "[general]
language = \"pt_BR\"
[appearance.window]
background = \"nope\"
",
        );
        let LoadResult::Invalid { config, .. } = result else {
            panic!("expected Invalid, got {result:?}");
        };
        assert_eq!(config.general.language, "pt_BR");
        // Everything else is still the defaults.
        let mut expected = Config::default();
        expected.general.language = "pt_BR".to_owned();
        assert_eq!(config, expected);
    }

    #[test]
    fn invalid_config_with_broken_syntax_falls_back_to_en_us() {
        let result = load_text(
            "[general]
language = \"pt_BR\"
this is not toml
",
        );
        let LoadResult::Invalid { config, error } = result else {
            panic!("expected Invalid, got {result:?}");
        };
        assert!(matches!(error.kind, ConfigErrorKind::Toml { .. }));
        assert_eq!(config, Config::default());
        assert_eq!(config.general.language, "en_US");
    }

    #[test]
    fn invalid_config_with_non_text_language_falls_back_to_en_us() {
        let result = load_text(
            "[general]
language = 5
",
        );
        let LoadResult::Invalid { config, .. } = result else {
            panic!("expected Invalid, got {result:?}");
        };
        assert_eq!(config.general.language, "en_US");
    }

    fn load_from_nonexistent_path() -> LoadResult {
        let path = std::env::temp_dir().join("porecatu-config-does-not-exist.toml");
        load(Some(&path))
    }
}
