// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.tooltip]` -- ADR-0019. Tooltip do texto truncado. Classe
//! de recarga A -- `delay_ms` vale para o próximo hover.

use serde::Deserialize;

use crate::color::Color;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Tooltip {
    pub background: Color,
    pub border: Color,
    pub corner_radius: i32,
    pub foreground: Color,
    pub font_size: f64,
    /// Largura máxima; texto além disso trunca com reticências.
    pub max_width: i32,
    pub padding_x: i32,
    pub padding_y: i32,
    /// Folga entre o alvo e o tooltip, no eixo em que ele ancora.
    pub gap: i32,
    /// Atraso de hover parado antes de aparecer.
    pub delay_ms: u64,
    /// Aparece só quando o texto do alvo foi efetivamente truncado.
    /// Desligar aqui remove o tooltip por completo.
    pub enabled: bool,
}

impl Default for Tooltip {
    fn default() -> Self {
        Self {
            background: Color::hex("#1a1e25"),
            border: Color::hex("#2e343e"),
            corner_radius: 6,
            foreground: Color::hex("#d7dce3"),
            font_size: 11.0,
            max_width: 320,
            padding_x: 8,
            padding_y: 7,
            gap: 6,
            delay_ms: 600,
            enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(Tooltip::default().delay_ms, 600);
        assert!(Tooltip::default().enabled);
    }
}
