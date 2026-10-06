// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.window]` -- RF-4.1. Classe de recarga A, com quatro
//! exceções marcadas campo a campo.

use serde::Deserialize;

use crate::BackgroundImage;
use crate::color::Color;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Window {
    /// [B] muda a área útil, logo colunas e linhas.
    pub padding_x: i32,
    /// [B] idem.
    pub padding_y: i32,
    /// [C] "vale na próxima janela": atributo de superfície decidido na
    /// criação. 0.0 a 1.0.
    pub opacity: f64,
    pub background: Color,
    pub border: Color,
    pub corner_radius: i32,
    /// [C] "reinicie o app": `winit` não recria o frame do SO sem recriar a
    /// janela. Valor literal do arquivo de exemplo -- o hoje-hardcoded
    /// `cfg(target_os = "macos")` de `porecatu-ui` continua sendo quem
    /// decide o comportamento real no macOS (ver relato de entrega).
    pub decorations: bool,
    pub animations: bool,
    /// Formar/arrastar grupo (RF-2.5).
    pub animation_reflow_ms: u64,
    /// Colapso e expansão de grupo (RF-2.13).
    pub animation_collapse_ms: u64,
    /// `[appearance.window.background_image]` -- PRD-018, ADR-0062 §8. [A]
    /// Same type as `[terminal.background_image]`, reused rather than copied;
    /// the reference area is the whole window, not a terminal frame.
    pub background_image: BackgroundImage,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            padding_x: 0,
            padding_y: 0,
            opacity: 1.0,
            background: Color::hex("#15181d"),
            border: Color::hex("#2a2f38"),
            corner_radius: 8,
            decorations: false,
            animations: true,
            animation_reflow_ms: 180,
            animation_collapse_ms: 150,
            background_image: BackgroundImage::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BackgroundImageMode, parse};

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(
            Window::default(),
            Window {
                padding_x: 0,
                padding_y: 0,
                opacity: 1.0,
                background: Color::hex("#15181d"),
                border: Color::hex("#2a2f38"),
                corner_radius: 8,
                decorations: false,
                animations: true,
                animation_reflow_ms: 180,
                animation_collapse_ms: 150,
                background_image: BackgroundImage::default(),
            }
        );
    }

    #[test]
    fn background_image_defaults_when_the_table_is_missing() {
        let (config, unknown) = parse("[appearance.window]\nopacity = 0.9\n").unwrap();
        assert!(unknown.is_empty());
        let image = config.appearance.window.background_image;
        assert_eq!(image, BackgroundImage::default());
        assert_eq!(image.path, "");
        assert_eq!(image.mode, BackgroundImageMode::Stretch);
        assert_eq!(image.opacity, 1.0);
    }

    #[test]
    fn background_image_parses_the_three_fields() {
        let text = "[appearance.window.background_image]\n\
                    path = \"imagens/praia.jpg\"\n\
                    mode = \"center\"\n\
                    opacity = 0.4\n";
        let (config, unknown) = parse(text).unwrap();
        assert!(unknown.is_empty());
        let image = config.appearance.window.background_image;
        assert_eq!(image.path, "imagens/praia.jpg");
        assert_eq!(image.mode, BackgroundImageMode::Center);
        assert_eq!(image.opacity, 0.4);
    }

    #[test]
    fn background_image_is_independent_of_the_terminal_one() {
        let text = "[appearance.window.background_image]\n\
                    path = \"a.png\"\n\
                    [terminal.background_image]\n\
                    path = \"b.png\"\n\
                    mode = \"tile\"\n";
        let (config, _) = parse(text).unwrap();
        assert_eq!(config.appearance.window.background_image.path, "a.png");
        assert_eq!(
            config.appearance.window.background_image.mode,
            BackgroundImageMode::Stretch
        );
        assert_eq!(config.terminal.background_image.path, "b.png");
        assert_eq!(
            config.terminal.background_image.mode,
            BackgroundImageMode::Tile
        );
    }

    #[test]
    fn background_image_invalid_mode_is_a_localized_error() {
        for bad in ["fill", "Stretch", ""] {
            let err = parse(&format!(
                "[appearance.window.background_image]\nmode = \"{bad}\"\n"
            ))
            .expect_err(bad);
            assert!(err.line.is_some(), "modo {bad:?} sem posição");
        }
    }

    #[test]
    fn background_image_opacity_out_of_range_is_clamped_by_the_consumer() {
        let (config, _) = parse("[appearance.window.background_image]\nopacity = 1.5\n").unwrap();
        let image = config.appearance.window.background_image;
        assert_eq!(image.opacity, 1.5);
        assert_eq!(image.clamped_opacity(), 1.0);
    }
}
