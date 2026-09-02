// SPDX-License-Identifier: GPL-3.0-or-later

//! `[terminal.selection]` -- PRD-010, ADR-0013. Gestos são fixos, não
//! configuráveis; só o que está aqui é. Classe de recarga A -- vale para a
//! próxima seleção.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Selection {
    /// Desligado por default: quem seleciona só para ler não espera perder
    /// o que tinha copiado. RF-10.8.
    pub copy_on_select: bool,
    /// Caracteres que delimitam "palavra" no duplo clique. RF-10.5.
    pub word_separators: String,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            copy_on_select: false,
            word_separators: " \t\n\"'`()[]{}<>,;:!?=&|".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(
            Selection::default().word_separators,
            " \t\n\"'`()[]{}<>,;:!?=&|"
        );
    }
}
