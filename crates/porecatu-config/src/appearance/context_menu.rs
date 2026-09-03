// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.context_menu]` -- RF-1.1, RF-1.2, RF-2.20, RF-2.22,
//! RF-10.19 a RF-10.21. Classe de recarga A.

use serde::Deserialize;

use crate::color::Color;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ContextMenu {
    pub background: Color,
    pub border: Color,
    pub corner_radius: i32,
    /// Largura mínima.
    pub width: i32,
    pub padding: i32,
    pub item_height: i32,
    pub item_padding_x: i32,
    pub item_corner_radius: i32,
    pub item_hover_background: Color,
    pub separator: Color,
    pub foreground: Color,
    /// Item indisponível fica esmaecido, nunca ausente. RF-10.20.
    pub disabled_foreground: Color,
    pub destructive_foreground: Color,
    pub font_size: f64,
}

impl Default for ContextMenu {
    fn default() -> Self {
        Self {
            background: Color::hex("#1a1e25"),
            border: Color::hex("#2e343e"),
            corner_radius: 8,
            width: 200,
            padding: 6,
            item_height: 28,
            item_padding_x: 8,
            item_corner_radius: 5,
            item_hover_background: Color::hex("#242a33"),
            separator: Color::hex("#2a2f38"),
            foreground: Color::hex("#d7dce3"),
            disabled_foreground: Color::hex("#5c646f"),
            destructive_foreground: Color::hex("#e08585"),
            font_size: 12.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(ContextMenu::default().width, 200);
        assert_eq!(ContextMenu::default().item_height, 28);
    }
}
