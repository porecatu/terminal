// SPDX-License-Identifier: GPL-3.0-or-later

//! `[terminal.scrollback]` -- RF-5.26, RF-5.27, RF-10.13, RF-10.14. Classe
//! de recarga C: "vale em aba nova" -- mudar a capacidade de um motor em
//! execução descartaria linhas já roladas.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Scrollback {
    pub lines: u32,
    /// Linhas por passo da roda do mouse.
    pub scroll_multiplier: i32,
    /// Saída em segundo plano não arranca o usuário de onde ele estava
    /// lendo. RF-10.13.
    pub scroll_on_output: bool,
    /// Digitar volta ao final, que é onde o prompt está. RF-10.13.
    pub scroll_on_input: bool,
    /// Na tela alternativa a roda do mouse vira setas. RF-10.14.
    pub alternate_scroll: bool,
}

impl Default for Scrollback {
    fn default() -> Self {
        Self {
            lines: 10_000,
            scroll_multiplier: 3,
            scroll_on_output: false,
            scroll_on_input: true,
            alternate_scroll: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(Scrollback::default().lines, 10_000);
        assert!(Scrollback::default().alternate_scroll);
    }
}
