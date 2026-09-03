// SPDX-License-Identifier: GPL-3.0-or-later

//! `[terminal.colors]`, `[terminal.colors.normal]` e
//! `[terminal.colors.bright]` -- RF-5.11 a RF-5.17. Classe de recarga A.

use serde::Deserialize;

use crate::color::Color;

/// As 16 cores ANSI, em dois grupos de oito (RF-5.11). Usado tanto pela
/// paleta base do terminal (aqui, sempre completa) quanto, de forma
/// parcial, por um override de `[[themes]]` -- ver `crate::theme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AnsiPalette {
    pub black: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub blue: Color,
    pub magenta: Color,
    pub cyan: Color,
    pub white: Color,
}

impl AnsiPalette {
    fn normal_default() -> Self {
        Self {
            black: Color::hex("#3b434f"),
            red: Color::hex("#ef8a8a"),
            green: Color::hex("#86c56a"),
            yellow: Color::hex("#e0b060"),
            blue: Color::hex("#6fa8f5"),
            magenta: Color::hex("#a68cf0"),
            cyan: Color::hex("#5ed3bc"),
            white: Color::hex("#c7ccd6"),
        }
    }

    fn bright_default() -> Self {
        Self {
            black: Color::hex("#6f7783"),
            red: Color::hex("#f5a3a3"),
            green: Color::hex("#9bd482"),
            yellow: Color::hex("#ecc37c"),
            blue: Color::hex("#8dbcf8"),
            magenta: Color::hex("#bda6f5"),
            cyan: Color::hex("#7fdfcc"),
            white: Color::hex("#eaeef3"),
        }
    }
}

/// Placeholder até o `Default` real ser escolhido por quem instancia --
/// `Colors::default()` chama `AnsiPalette::normal_default()`/
/// `bright_default()` explicitamente, então este impl nunca é exercitado
/// pelo default de `Colors`. Existe só porque o `#[serde(default)]` do
/// container exige `Default` no tipo.
impl Default for AnsiPalette {
    fn default() -> Self {
        Self::normal_default()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Colors {
    pub foreground: Color,
    pub background: Color,
    pub cursor: Color,
    pub cursor_text: Color,
    pub selection_background: Color,
    pub selection_foreground: Color,
    /// Segunda metade do prompt (caminho), quando o shell emite OSC 7.
    pub prompt_secondary: Color,
    pub normal: AnsiPalette,
    pub bright: AnsiPalette,
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            foreground: Color::hex("#c7ccd6"),
            background: Color::hex("#0f1216"),
            cursor: Color::hex("#5ed3bc"),
            cursor_text: Color::hex("#0f1216"),
            selection_background: Color::hex("#2e6b62"),
            selection_foreground: Color::hex("#eef2f4"),
            prompt_secondary: Color::hex("#6b737e"),
            normal: AnsiPalette::normal_default(),
            bright: AnsiPalette::bright_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let colors = Colors::default();
        assert_eq!(colors.foreground, Color::hex("#c7ccd6"));
        assert_eq!(colors.normal.red, Color::hex("#ef8a8a"));
        assert_eq!(colors.bright.white, Color::hex("#eaeef3"));
    }
}
