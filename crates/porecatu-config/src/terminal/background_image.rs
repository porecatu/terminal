// SPDX-License-Identifier: GPL-3.0-or-later

//! `[terminal.background_image]` -- PRD-017 (RF-17.1 a RF-17.5, RF-17.11,
//! RF-17.16), ADR-0061 §8. Classe de recarga A.
//!
//! Só a configuração e a resolução do caminho vivem aqui. Decodificar,
//! reduzir e desenhar é de `porecatu-ui` e `porecatu-render`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Como a imagem ocupa o quadro do terminal. RF-17.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundImageMode {
    /// Esticada até o quadro, nos dois eixos, sem manter a proporção.
    #[default]
    Stretch,
    /// Repetida no tamanho natural, a partir do canto superior esquerdo.
    Tile,
    /// No tamanho natural, centralizada.
    Center,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct BackgroundImage {
    /// Vazio = sem imagem. Relativo ao diretório do arquivo de config
    /// (RF-17.3); veja [`resolve_background_image_path`]. RF-17.1.
    pub path: String,
    pub mode: BackgroundImageMode,
    /// 0.0 a 1.0, independente de `[terminal] background_opacity`; a
    /// opacidade efetiva é o produto das duas (RF-17.12). Fora da faixa
    /// não é erro de config -- como `background_opacity`, quem consome
    /// limita (`clamped_opacity`). RF-17.11.
    pub opacity: f32,
}

impl Default for BackgroundImage {
    fn default() -> Self {
        Self {
            path: String::new(),
            mode: BackgroundImageMode::Stretch,
            opacity: 1.0,
        }
    }
}

impl BackgroundImage {
    /// `opacity` limitada a 0.0..=1.0; NaN vira 1.0 (o padrão).
    pub fn clamped_opacity(&self) -> f32 {
        if self.opacity.is_nan() {
            1.0
        } else {
            self.opacity.clamp(0.0, 1.0)
        }
    }
}

/// Resolve o `path` cru de `[terminal.background_image]` num caminho
/// utilizável (RF-17.3, ADR-0061 §8). Função pura: não toca o disco.
///
/// - vazio ou só espaço: `None` (sem imagem);
/// - `~/` ou `~\` no início: a pasta pessoal (`dirs::home_dir`); `None` se
///   ela não puder ser descoberta;
/// - absoluto: como está;
/// - relativo: junto com o diretório de `config_path`. Sem `config_path`,
///   usa o diretório do config padrão da plataforma -- o mesmo que o app
///   usaria para carregar um.
///
/// É a **primeira** chave com caminho relativo ao arquivo de config:
/// `startup_directory` e `trusted_paths` seguem outra regra.
pub fn resolve_background_image_path(config_path: Option<&Path>, raw: &str) -> Option<PathBuf> {
    resolve(
        config_path,
        raw,
        dirs::home_dir,
        crate::path::platform_default_dir,
    )
}

fn resolve(
    config_path: Option<&Path>,
    raw: &str,
    home_dir: impl FnOnce() -> Option<PathBuf>,
    default_config_dir: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return home_dir().map(|home| home.join(rest));
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    let base = config_path
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .or_else(default_config_dir)?;
    Some(base.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, parse};

    fn home() -> Option<PathBuf> {
        Some(PathBuf::from("home-dir"))
    }

    fn default_dir() -> Option<PathBuf> {
        Some(PathBuf::from("default-config-dir"))
    }

    fn resolved(config_path: Option<&Path>, raw: &str) -> Option<PathBuf> {
        resolve(config_path, raw, home, default_dir)
    }

    #[test]
    fn default_matches_example_toml() {
        let image = BackgroundImage::default();
        assert_eq!(image.path, "");
        assert_eq!(image.mode, BackgroundImageMode::Stretch);
        assert_eq!(image.opacity, 1.0);
    }

    #[test]
    fn parse_without_the_table_is_defaults() {
        let (config, unknown) = parse("[terminal]\nbackground_opacity = 0.5\n").unwrap();
        assert_eq!(config.terminal.background_image, BackgroundImage::default());
        assert!(unknown.is_empty());
    }

    #[test]
    fn parse_with_the_table() {
        let text = "[terminal.background_image]\n\
                    path = \"imagens/montanha.jpg\"\n\
                    mode = \"tile\"\n\
                    opacity = 0.35\n";
        let (config, unknown) = parse(text).unwrap();
        assert!(unknown.is_empty());
        let image = config.terminal.background_image;
        assert_eq!(image.path, "imagens/montanha.jpg");
        assert_eq!(image.mode, BackgroundImageMode::Tile);
        assert_eq!(image.opacity, 0.35);
    }

    #[test]
    fn partial_table_keeps_the_other_defaults() {
        let (config, _) = parse("[terminal.background_image]\npath = \"a.png\"\n").unwrap();
        let image = config.terminal.background_image;
        assert_eq!(image.path, "a.png");
        assert_eq!(image.mode, BackgroundImageMode::Stretch);
        assert_eq!(image.opacity, 1.0);
        assert_ne!(Config::default().terminal.background_image, image);
    }

    #[test]
    fn all_three_modes_parse_in_lowercase() {
        for (text, expected) in [
            ("stretch", BackgroundImageMode::Stretch),
            ("tile", BackgroundImageMode::Tile),
            ("center", BackgroundImageMode::Center),
        ] {
            let (config, _) =
                parse(&format!("[terminal.background_image]\nmode = \"{text}\"\n")).unwrap();
            assert_eq!(config.terminal.background_image.mode, expected);
        }
    }

    #[test]
    fn invalid_mode_is_a_localized_error() {
        for bad in ["fill", "Stretch", ""] {
            let err =
                parse(&format!("[terminal.background_image]\nmode = \"{bad}\"\n")).expect_err(bad);
            assert!(err.line.is_some(), "modo {bad:?} sem posição");
        }
    }

    #[test]
    fn opacity_out_of_range_parses_and_is_clamped_by_the_consumer() {
        let (config, _) = parse("[terminal.background_image]\nopacity = 1.5\n").unwrap();
        let image = config.terminal.background_image;
        assert_eq!(image.opacity, 1.5);
        assert_eq!(image.clamped_opacity(), 1.0);

        let (config, _) = parse("[terminal.background_image]\nopacity = -0.2\n").unwrap();
        assert_eq!(config.terminal.background_image.clamped_opacity(), 0.0);

        let nan = BackgroundImage {
            opacity: f32::NAN,
            ..BackgroundImage::default()
        };
        assert_eq!(nan.clamped_opacity(), 1.0);
    }

    #[test]
    fn opacity_of_the_wrong_type_is_an_error() {
        assert!(parse("[terminal.background_image]\nopacity = \"x\"\n").is_err());
    }

    #[test]
    fn empty_or_blank_is_none() {
        assert_eq!(resolved(Some(Path::new("/c/porecatu.toml")), ""), None);
        assert_eq!(resolved(Some(Path::new("/c/porecatu.toml")), "  \t"), None);
        assert_eq!(resolved(None, ""), None);
    }

    #[test]
    fn tilde_slash_and_tilde_backslash_are_home() {
        assert_eq!(
            resolved(None, "~/img/a.png"),
            Some(PathBuf::from("home-dir").join("img/a.png"))
        );
        assert_eq!(
            resolved(None, "~\\img\\a.png"),
            Some(PathBuf::from("home-dir").join("img\\a.png"))
        );
    }

    #[test]
    fn tilde_is_home_whatever_the_config_path() {
        assert_eq!(
            resolved(Some(Path::new("/c/porecatu.toml")), "~/a.png"),
            Some(PathBuf::from("home-dir").join("a.png"))
        );
    }

    #[test]
    fn tilde_without_a_home_is_none() {
        assert_eq!(resolve(None, "~/a.png", || None, default_dir), None);
    }

    #[test]
    fn tilde_not_followed_by_a_separator_is_a_relative_name() {
        // `~foo` and a bare `~` are ordinary file names, not the home.
        assert_eq!(
            resolved(Some(Path::new("/c/porecatu.toml")), "~foo.png"),
            Some(PathBuf::from("/c").join("~foo.png"))
        );
        assert_eq!(
            resolved(Some(Path::new("/c/porecatu.toml")), "~"),
            Some(PathBuf::from("/c").join("~"))
        );
    }

    #[test]
    fn relative_is_joined_to_the_config_directory() {
        assert_eq!(
            resolved(Some(Path::new("/etc/porecatu/porecatu.toml")), "fundo.jpg"),
            Some(PathBuf::from("/etc/porecatu").join("fundo.jpg"))
        );
        assert_eq!(
            resolved(
                Some(Path::new("/etc/porecatu/porecatu.toml")),
                "imagens/a.png"
            ),
            Some(PathBuf::from("/etc/porecatu").join("imagens/a.png"))
        );
    }

    #[test]
    fn relative_without_a_config_path_uses_the_default_config_directory() {
        assert_eq!(
            resolved(None, "fundo.jpg"),
            Some(PathBuf::from("default-config-dir").join("fundo.jpg"))
        );
    }

    #[test]
    fn relative_with_a_bare_config_file_name_stays_relative() {
        // `--config porecatu.toml`: its parent is the empty path, so the
        // image resolves against the process's directory, like the file.
        assert_eq!(
            resolved(Some(Path::new("porecatu.toml")), "fundo.jpg"),
            Some(PathBuf::from("fundo.jpg"))
        );
    }

    #[test]
    fn relative_without_any_directory_is_none() {
        assert_eq!(resolve(None, "fundo.jpg", home, || None), None);
    }

    #[test]
    fn surrounding_space_is_ignored() {
        assert_eq!(
            resolved(Some(Path::new("/c/porecatu.toml")), "  fundo.jpg "),
            Some(PathBuf::from("/c").join("fundo.jpg"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_absolute_is_kept_as_is() {
        assert_eq!(
            resolved(Some(Path::new("/c/porecatu.toml")), "/usr/share/bg.png"),
            Some(PathBuf::from("/usr/share/bg.png"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_absolute_is_kept_as_is() {
        assert_eq!(
            resolved(Some(Path::new("C:\\cfg\\porecatu.toml")), "D:\\img\\bg.png"),
            Some(PathBuf::from("D:\\img\\bg.png"))
        );
        assert_eq!(
            resolved(None, "C:/img/bg.png"),
            Some(PathBuf::from("C:/img/bg.png"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_relative_is_joined_to_the_config_directory() {
        assert_eq!(
            resolved(Some(Path::new("C:\\cfg\\porecatu.toml")), "imagens\\a.png"),
            Some(PathBuf::from("C:\\cfg").join("imagens\\a.png"))
        );
    }

    #[test]
    fn public_entry_point_matches_the_pure_core_for_absolute_and_empty() {
        assert_eq!(resolve_background_image_path(None, ""), None);
        let abs = std::env::temp_dir().join("bg.png");
        let raw = abs.to_str().unwrap();
        assert_eq!(resolve_background_image_path(None, raw), Some(abs));
    }
}
