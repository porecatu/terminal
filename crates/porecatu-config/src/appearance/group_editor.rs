// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.group_editor]` -- ADR-0023. Quinto widget de chrome, e a
//! única superfície de escolha de cor do v1. Classe de recarga A.

use serde::Deserialize;

use crate::color::Color;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct GroupEditor {
    pub background: Color,
    pub border: Color,
    pub corner_radius: i32,
    pub padding: i32,
    pub gap: i32,
    pub width: i32,
    /// Folga entre a borda da barra de abas e o topo do popover.
    pub offset_y: i32,
    pub section_foreground: Color,
    pub section_font_size: f64,
    pub input_background: Color,
    pub input_border: Color,
    pub input_border_focus: Color,
    pub input_corner_radius: i32,
    pub input_foreground: Color,
    pub input_font_size: f64,
    pub input_height: i32,
    pub input_padding_x: i32,
    pub section_caption_gap: i32,
    /// Faixa de cores: seis quadrados, um por cor da paleta.
    pub swatch_size: i32,
    pub swatch_corner_radius: i32,
    pub swatch_gap: i32,
    pub swatch_border_width: i32,
    pub swatch_ring_selected: Color,
    pub swatch_highlight_pad: i32,
    pub item_foreground: Color,
    pub item_hover_background: Color,
    pub item_height: i32,
    pub divider: Color,
    pub divider_height: i32,
    /// "Fechar grupo (N abas)" -- a ação destrutiva.
    pub destructive_foreground: Color,
    pub destructive_hover_background: Color,
}

impl Default for GroupEditor {
    fn default() -> Self {
        Self {
            background: Color::hex("#1a1e25"),
            border: Color::hex("#2e343e"),
            corner_radius: 8,
            padding: 14,
            gap: 13,
            width: 286,
            offset_y: 8,
            section_foreground: Color::hex("#5c646f"),
            section_font_size: 10.0,
            input_background: Color::hex("#0f1216"),
            input_border: Color::hex("#333a45"),
            input_border_focus: Color::hex("#5ed3bc"),
            input_corner_radius: 5,
            input_foreground: Color::hex("#e4e8ee"),
            input_font_size: 13.0,
            input_height: 30,
            input_padding_x: 9,
            section_caption_gap: 6,
            swatch_size: 28,
            swatch_corner_radius: 6,
            swatch_gap: 8,
            swatch_border_width: 2,
            swatch_ring_selected: Color::hex("#eef2f4"),
            swatch_highlight_pad: 3,
            item_foreground: Color::hex("#d7dce3"),
            item_hover_background: Color::hex("#242a33"),
            item_height: 28,
            divider: Color::hex("#2a2f38"),
            divider_height: 1,
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
        assert_eq!(GroupEditor::default().width, 286);
        assert_eq!(GroupEditor::default().swatch_size, 28);
    }
}
