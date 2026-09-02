// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.groups]` -- RF-4.13 a RF-4.19, RF-2.4, RF-2.16. Classe de
//! recarga A, inclusive `wrapper_padding` (muda a altura da aba solta, não
//! a da barra).
//!
//! Sem chave de estilo de indicador -- a pílula mais a cápsula são a única
//! forma (ADR-0032, que supersede o ADR-0009 §5).

use serde::Deserialize;

use crate::color::Color;

/// Uma entrada da paleta de cores de grupo (RF-4.18, RF-2.4).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GroupPaletteEntry {
    pub name: String,
    pub color: Color,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Groups {
    // --- pílula do grupo -------------------------------------- RF-4.13
    pub label_padding_left: i32,
    pub label_padding_right: i32,
    pub label_font_size: f64,
    /// 400 | 500 -- a face Fixed só embute os dois.
    pub label_font_weight: i32,
    pub label_corner_radius: i32,
    pub label_hover_brightness: f64,
    /// Teto do nome do grupo (RF-2.12); sem piso.
    pub label_max_width: i32,
    pub count_background: Color,
    pub count_foreground: Color,
    pub count_corner_radius: i32,

    // --- espaçamento -------------------------------------------- RF-4.16
    pub gap: i32,
    pub wrapper_padding: i32,
    pub wrapper_corner_radius: i32,

    // --- cor e vidro -------------------------------------------- RF-4.19
    pub tint_strength: f64,
    pub capsule_alpha: f64,
    pub label_alpha: f64,
    pub glass_border: Color,
    pub glass_border_alpha: f64,
    pub glass_border_width: i32,
    pub shadow: bool,
    pub border: Color,

    /// RF-4.17.
    pub show_tab_count_when_collapsed: bool,

    /// RF-2.16.
    pub collapsed_indicator: bool,
    pub collapsed_indicator_size: i32,

    /// Seis cores, na ordem do design (RF-4.18, RF-2.4).
    pub palette: Vec<GroupPaletteEntry>,
    /// Cor usada por abas fora de qualquer grupo (grupo implícito,
    /// ADR-0006).
    pub ungrouped_color: Color,

    /// `[v2]` PRD-007.
    pub badge_tint_strength: f64,
}

impl Default for Groups {
    fn default() -> Self {
        Self {
            label_padding_left: 10,
            label_padding_right: 9,
            label_font_size: 13.0,
            label_font_weight: 500,
            label_corner_radius: 6,
            label_hover_brightness: 1.25,
            label_max_width: 140,
            count_background: Color::hex("#12151a"),
            count_foreground: Color::hex("#7b838f"),
            count_corner_radius: 9,
            gap: 6,
            wrapper_padding: 3,
            wrapper_corner_radius: 8,
            tint_strength: 1.0,
            capsule_alpha: 0.85,
            label_alpha: 0.92,
            glass_border: Color::hex("#ffffff"),
            glass_border_alpha: 0.16,
            glass_border_width: 1,
            shadow: true,
            border: Color::hex("#262b34"),
            show_tab_count_when_collapsed: true,
            collapsed_indicator: true,
            collapsed_indicator_size: 6,
            palette: vec![
                GroupPaletteEntry {
                    name: "vermelho".to_owned(),
                    color: Color::hex("#ef8a8a"),
                },
                GroupPaletteEntry {
                    name: "amarelo".to_owned(),
                    color: Color::hex("#e0b060"),
                },
                GroupPaletteEntry {
                    name: "ciano".to_owned(),
                    color: Color::hex("#5ed3bc"),
                },
                GroupPaletteEntry {
                    name: "azul".to_owned(),
                    color: Color::hex("#6fa8f5"),
                },
                GroupPaletteEntry {
                    name: "roxo".to_owned(),
                    color: Color::hex("#a68cf0"),
                },
                GroupPaletteEntry {
                    name: "verde".to_owned(),
                    color: Color::hex("#86c56a"),
                },
            ],
            ungrouped_color: Color::hex("#7b838f"),
            badge_tint_strength: 0.14,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let groups = Groups::default();
        assert_eq!(groups.palette.len(), 6);
        assert_eq!(groups.palette[0].name, "vermelho");
        assert_eq!(groups.tint_strength, 1.0);
        assert_eq!(groups.ungrouped_color, Color::hex("#7b838f"));
    }
}
