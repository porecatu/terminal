// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.window_controls]` -- ADR-0027. Sem RF próprio em PRD-004:
//! o ADR veio depois do PRD (registro na seção 4.4 da espec.). Classe de
//! recarga A -- mudam a largura da zona fixa, não a grade.

use serde::Deserialize;

use crate::color::Color;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct WindowControls {
    pub button_width: i32,
    pub gap: i32,
    pub hover_background: Color,
    pub close_hover_background: Color,
    pub close_hover_foreground: Color,
    /// Espessura da zona de resize em cada borda da janela.
    pub resize_border: i32,
    /// Espaço reservado à esquerda da trilha para o semáforo nativo do
    /// macOS. Valor literal do arquivo de exemplo -- zerar fora do macOS é
    /// responsabilidade de `porecatu-ui`, como `decorations` acima.
    pub macos_traffic_light_inset: i32,
}

impl Default for WindowControls {
    fn default() -> Self {
        Self {
            button_width: 46,
            gap: 6,
            hover_background: Color::hex("#252a33"),
            close_hover_background: Color::hex("#c4413f"),
            close_hover_foreground: Color::hex("#ffffff"),
            resize_border: 6,
            macos_traffic_light_inset: 78,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(
            WindowControls::default(),
            WindowControls {
                button_width: 46,
                gap: 6,
                hover_background: Color::hex("#252a33"),
                close_hover_background: Color::hex("#c4413f"),
                close_hover_foreground: Color::hex("#ffffff"),
                resize_border: 6,
                macos_traffic_light_inset: 78,
            }
        );
    }
}
