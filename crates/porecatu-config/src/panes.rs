// SPDX-License-Identifier: GPL-3.0-or-later

//! `[panes]` -- PRD-006, ADR-0053 §9. Mínimo útil de um painel: comportamento,
//! não aparência. Governa o `pane.split_*` (recusado se algum dos dois
//! resultantes ficasse abaixo do mínimo, RF-6.4) e o arraste do divisor
//! (RF-6.14, clampado no mínimo). Não é `[appearance]` -- não descreve
//! dimensão desenhada nenhuma -- e por isso não entra na tabela `VALORES`
//! de `scripts/verify-docs.py`.
//!
//! Classe de recarga A (ADR-0030): aplica a quente sem tocar no PTY --
//! muda o mínimo não redimensiona nada que já existe, só governa o
//! próximo split e o próximo arraste.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Panes {
    /// RF-6.4/RF-6.14. Dois números, não um: colunas e linhas não são a
    /// mesma quantidade de terminal.
    pub min_columns: u32,
    pub min_rows: u32,
}

impl Default for Panes {
    fn default() -> Self {
        Self {
            min_columns: 20,
            min_rows: 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let panes = Panes::default();
        assert_eq!(panes.min_columns, 20);
        assert_eq!(panes.min_rows, 5);
    }
}
