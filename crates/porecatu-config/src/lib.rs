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
//! Cor não mora aqui: os campos de cor de `Config` não são lidos do
//! `porecatu.toml` e valem o default embutido até um tema `thm_*.toml` ser
//! aplicado (ADR-0067, ver `theme.rs`).

mod appearance;
mod color;
mod edit;
mod error;
mod general;
mod git;
mod keybindings;
mod panes;
mod path;
mod profiles;
mod project_file;
mod session;
mod shell;
mod ssh_import;
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
pub use profiles::{
    AutoPlatform, MissingProfile, ProfileDef, ProfileKind, ProfileProblem, ProfileProblemKind,
    ProfileRef, ProfilesConfig, ResolvedProfile, ResolvedSsh, SshAuth, SshProfile,
    is_secret_profile_key, resolve_auto_profile, resolve_profile,
};
pub use project_file::{
    PROJECT_FILE_NAME, ProjectFile, ProjectFileOutcome, ProjectScript, expand_tilde, is_trusted,
    parse as parse_project_file, resolve as resolve_project_file,
};
pub use session::Session;
pub use shell::Shell;
pub use ssh_import::{
    SshConfigImport, SshConfigImportError, SshConfigImportErrorKind, SshImportCandidate,
    SshImportInvalid, SshImportStatus, SshImportUnsupported, default_ssh_config_path,
    local_user_name, read_ssh_config,
};
pub use terminal::{
    Acrylic, AcrylicBlock, AcrylicState, AnsiPalette, BackgroundImage, BackgroundImageMode,
    Clipboard, Colors as TerminalColors, Cursor, CursorShape, DEFAULT_BLUR, Font, MAX_BLUR,
    MIN_BLUR, Scrollback, Selection, Terminal, ZoomScope, acrylic_requirement, acrylic_state,
    resolve_background_image_path,
};
pub use theme::{
    BUILT_IN_THEME_FILES, DEFAULT_LIGHT_THEME, DEFAULT_THEME, THEME_FILE_PREFIX, Theme,
    ThemeAnsiPalette, ThemeContextMenu, ThemeDialog, ThemeFileProblem, ThemeGroupEditor,
    ThemeGroups, ThemeNotices, ThemeSettings, ThemeStatusBar, ThemeTabs, ThemeTerminal,
    ThemeTooltip, ThemeWindow, ThemeWindowControls, apply as apply_theme, is_color_key,
    load_theme_files, parse_theme, theme_name_from_file,
};

use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub shell: Shell,
    pub profiles: ProfilesConfig,
    pub appearance: Appearance,
    pub terminal: Terminal,
    /// Os temas conhecidos: os embutidos, mais os `thm_*.toml` do disco
    /// depois de `load_theme_files`. Nunca lido do `porecatu.toml` --
    /// `[[themes]]` lá é chave ignorada (ADR-0067).
    #[serde(skip_deserializing)]
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
            profiles: ProfilesConfig::default(),
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
        /// `[[profiles.list]]` itens recusados e `[profiles] default`
        /// inexistente (ADR-0070 §1, RF-7.5): a config já só tem os válidos.
        profile_problems: Vec<ProfileProblem>,
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
    match parse_checked(&text) {
        Ok(parsed) => LoadResult::Loaded {
            config: parsed.config,
            unknown_keys: parsed.unknown_keys,
            profile_problems: parsed.profile_problems,
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
/// sintaticamente inválido ou tipo errado num campo. Os temas são só os
/// embutidos: os do disco entram por `load_theme_files`.
pub fn parse(text: &str) -> Result<(Config, Vec<String>), ConfigError> {
    let parsed = parse_checked(text)?;
    Ok((parsed.config, parsed.unknown_keys))
}

/// What `parse_checked` read: the config, the unknown-key warnings and the
/// profile items that were refused (ADR-0070 §1, RF-7.5).
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub config: Config,
    pub unknown_keys: Vec<String>,
    /// Refused `[[profiles.list]]` items and a dangling `[profiles] default`,
    /// in file order. The config already holds only the valid ones.
    pub profile_problems: Vec<ProfileProblem>,
}

/// Like [`parse`], but also reports the profile items it dropped. A refused
/// item is a localized problem, not a `ConfigError`: the rest of the file
/// keeps applying.
pub fn parse_checked(text: &str) -> Result<Parsed, ConfigError> {
    let deserializer =
        toml::de::Deserializer::parse(text).map_err(|err| ConfigError::from_toml(text, err))?;
    let mut unknown_keys = Vec::new();
    let mut config: Config = serde_ignored::deserialize(deserializer, |path| {
        // `Spanned` (profile items) puts a private segment in the path.
        unknown_keys.push(
            path.to_string()
                .replace(".$__serde_spanned_private_value", ""),
        );
    })
    .map_err(|err| ConfigError::from_toml(text, err))?;
    let profile_problems = std::mem::take(&mut config.profiles.problems)
        .into_iter()
        .map(|problem| {
            let (line, column) = error::line_column_at(text, problem.offset);
            ProfileProblem {
                line,
                column,
                kind: problem.kind,
            }
        })
        .collect();
    Ok(Parsed {
        config,
        unknown_keys,
        profile_problems,
    })
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
    fn color_in_config_is_an_ignored_color_key() {
        let (config, unknown_keys) = parse(
            "[appearance.window]\nbackground = \"#123456\"\n[terminal.colors]\nforeground = \"#654321\"\n[[themes]]\nname = \"x\"\n",
        )
        .expect("cor no porecatu.toml é aviso");
        assert_eq!(
            config.appearance.window.background,
            Config::default().appearance.window.background
        );
        assert_eq!(config.themes, Config::default().themes);
        assert!(!unknown_keys.is_empty());
        assert!(
            unknown_keys.iter().all(|key| is_color_key(key)),
            "{unknown_keys:?}"
        );
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
        // Deserialization fails on the opacity; the TOML itself is fine.
        let result = load_text(
            "[general]
language = \"pt_BR\"
[appearance.window]
opacity = \"nope\"
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
