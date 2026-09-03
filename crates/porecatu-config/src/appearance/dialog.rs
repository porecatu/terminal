// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.dialog]` -- PRD-010, ADR-0014. Diálogo modal de
//! confirmação. Foco inicial é sempre o cancelar, e isso não é
//! configurável (RF-10.18). Classe de recarga A.

use serde::Deserialize;

use crate::color::Color;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Dialog {
    pub overlay: Color,
    pub background: Color,
    pub border: Color,
    pub corner_radius: i32,
    pub width: i32,
    pub padding: i32,
    /// Entre título, corpo e a fila de botões.
    pub gap: i32,
    pub title_foreground: Color,
    pub title_font_size: f64,
    pub foreground: Color,
    pub font_size: f64,
    pub button_height: i32,
    pub button_gap: i32,
    pub button_padding_x: i32,
    pub button_corner_radius: i32,
    pub button_border: Color,
    pub destructive_foreground: Color,
    pub destructive_hover_background: Color,
}

impl Default for Dialog {
    fn default() -> Self {
        Self {
            overlay: Color::hex("#06070973"),
            background: Color::hex("#1a1e25"),
            border: Color::hex("#2e343e"),
            corner_radius: 10,
            width: 380,
            padding: 16,
            gap: 14,
            title_foreground: Color::hex("#e6eaef"),
            title_font_size: 13.0,
            foreground: Color::hex("#d7dce3"),
            font_size: 12.5,
            button_height: 30,
            button_gap: 8,
            button_padding_x: 12,
            button_corner_radius: 5,
            button_border: Color::hex("#262b34"),
            destructive_foreground: Color::hex("#e08585"),
            destructive_hover_background: Color::hex("#2e2224"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(Dialog::default().overlay, Color::hex("#06070973"));
        assert_eq!(Dialog::default().width, 380);
    }
}
