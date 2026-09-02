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
mod error;
mod general;
mod keybindings;
mod path;
mod session;
mod shell;
mod terminal;
mod theme;

pub use appearance::{
    Appearance, CloseButtonVisibility, ContextMenu, Dialog, GroupEditor, GroupPaletteEntry, Groups,
    MoveToGroup, Notices, TabBarPosition, Tabs, TabsColors, TabsOverflow, TabsRename,
    TerminalFrame, Tooltip, Window, WindowControls,
};
pub use color::{Color, ColorParseError};
pub use error::ConfigError;
pub use general::General;
pub use keybindings::Keybindings;
pub use path::resolve_config_path;
pub use session::Session;
pub use shell::Shell;
pub use terminal::{
    AnsiPalette, Clipboard, Colors as TerminalColors, Cursor, CursorShape, Font, Scrollback,
    Selection, Terminal, ZoomScope,
};
pub use theme::{Theme, ThemeAnsiPalette};

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
        }
    }
}

/// Resultado de carregar a config no início do processo (ADR-0003 regra
/// 2). `config` está sempre presente e pronto para uso em qualquer
/// variante -- inclusive `Invalid`, onde é `Config::default()`: o chamador
/// decide o que fazer com o erro (mostrar aviso, por exemplo), a config
/// nunca fica pela metade.
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
                error: ConfigError::new(format!(
                    "não foi possível ler \"{}\": {err}",
                    path.display()
                )),
            };
        }
    };
    match parse(&text) {
        Ok((config, unknown_keys)) => LoadResult::Loaded {
            config,
            unknown_keys,
        },
        Err(error) => LoadResult::Invalid {
            config: Config::default(),
            error,
        },
    }
}

/// Parseia o texto de uma config já lida -- usado por `load` acima e pelo
/// hot reload (etapa 4, que já tem o texto em mãos após o evento do
/// `notify`). Devolve a config carregada mais as chaves desconhecidas
/// (ADR-0003 regra 4), ou o primeiro erro localizado (regra 3): TOML
/// sintaticamente inválido, tipo errado num campo (inclusive cor
/// inválida, RF-4.9) ou nome de tema duplicado.
pub fn parse(text: &str) -> Result<(Config, Vec<String>), ConfigError> {
    let deserializer = toml::de::Deserializer::new(text);
    let mut unknown_keys = Vec::new();
    let config: Config =
        serde_ignored::deserialize(deserializer, |path| unknown_keys.push(path.to_string()))
            .map_err(|err| ConfigError::from_toml(text, err))?;

    if let Some(name) = theme::find_duplicate_name(&config.themes) {
        return Err(ConfigError::new(format!(
            "nome de tema duplicado: \"{name}\""
        )));
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
        assert!(err.message.contains("cor inválida"));
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
        assert!(err.message.contains("x"));
    }

    #[test]
    fn missing_file_is_defaults() {
        let result = load_from_nonexistent_path();
        assert_eq!(result.config(), &Config::default());
        assert!(matches!(result, LoadResult::Missing { .. }));
    }

    fn load_from_nonexistent_path() -> LoadResult {
        let path = std::env::temp_dir().join("porecatu-config-does-not-exist.toml");
        load(Some(&path))
    }
}
