// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.notices]` -- PRD-010, ADR-0014. Avisos do app, empilhados
//! no canto superior direito. Classe de recarga A.

use serde::Deserialize;

use crate::color::Color;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Notices {
    pub background: Color,
    pub border: Color,
    pub corner_radius: i32,
    /// Título.
    pub foreground: Color,
    /// Corpo.
    pub body_foreground: Color,
    pub font_size: f64,
    pub body_font_size: f64,
    pub width: i32,
    pub padding_x: i32,
    pub padding_y: i32,
    pub gap: i32,
    pub stack_margin_top: i32,
    pub stack_margin_right: i32,
    pub line_height: i32,
    pub close_button_size: i32,
    pub severity_bar_width: i32,
    pub error: Color,
    pub warning: Color,
    pub info: Color,
    /// Quantos convivem na tela; o quarto substitui o mais antigo.
    pub max_visible: i32,
    /// RF-10.16. Erro e aviso persistem até dispensa.
    pub info_timeout_ms: u64,
}

impl Default for Notices {
    fn default() -> Self {
        Self {
            background: Color::hex("#1a1e25"),
            border: Color::hex("#2e343e"),
            corner_radius: 8,
            foreground: Color::hex("#dfe4ea"),
            body_foreground: Color::hex("#6b737e"),
            font_size: 12.5,
            body_font_size: 11.0,
            width: 320,
            padding_x: 12,
            padding_y: 11,
            gap: 8,
            stack_margin_top: 8,
            stack_margin_right: 10,
            line_height: 16,
            close_button_size: 17,
            severity_bar_width: 2,
            error: Color::hex("#ef8a8a"),
            warning: Color::hex("#e0b060"),
            info: Color::hex("#5ed3bc"),
            max_visible: 3,
            info_timeout_ms: 6000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(Notices::default().max_visible, 3);
        assert_eq!(Notices::default().info_timeout_ms, 6000);
    }
}
