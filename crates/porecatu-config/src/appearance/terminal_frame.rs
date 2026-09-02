// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.terminal_frame]` -- quadro arredondado em volta da grade
//! do terminal, pedido do usuário, sem origem no design. Classe de recarga
//! B: as três primeiras chaves mudam a área útil dentro do quadro, logo
//! colunas e linhas.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct TerminalFrame {
    /// Margem entre a borda da janela e o quadro, nos três lados que não
    /// encostam na barra de abas.
    pub margin: i32,
    /// Padding entre a borda do quadro e a grade, nos quatro lados.
    pub padding: i32,
    pub corner_radius: i32,
    /// Mesma sombra em camadas da cápsula.
    pub shadow: bool,
}

impl Default for TerminalFrame {
    fn default() -> Self {
        Self {
            margin: 6,
            padding: 6,
            corner_radius: 6,
            shadow: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(TerminalFrame::default().margin, 6);
        assert!(TerminalFrame::default().shadow);
    }
}
