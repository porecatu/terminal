// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.window]` -- RF-4.1. Classe de recarga A, com quatro
//! exceções marcadas campo a campo.

use serde::Deserialize;

use crate::color::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TabBarPosition {
    #[default]
    Top,
    Bottom,
}

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
    /// [C] "reinicie o app": move a barra de aresta, e com ela a origem de
    /// todo hit-testing e o recorte da trilha.
    pub tab_bar_position: TabBarPosition,
    pub animations: bool,
    /// Formar/arrastar grupo (RF-2.5).
    pub animation_reflow_ms: u64,
    /// Colapso e expansão de grupo (RF-2.13).
    pub animation_collapse_ms: u64,
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
            tab_bar_position: TabBarPosition::Top,
            animations: true,
            animation_reflow_ms: 180,
            animation_collapse_ms: 150,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                tab_bar_position: TabBarPosition::Top,
                animations: true,
                animation_reflow_ms: 180,
                animation_collapse_ms: 150,
            }
        );
    }
}
