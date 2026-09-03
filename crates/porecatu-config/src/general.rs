// SPDX-License-Identifier: GPL-3.0-or-later

//! `[general]` -- PRD-000, PRD-001. Classe de recarga A.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct General {
    /// RF-1.6, [ADR-0017](../../../docs/adr/0017-ciclo-de-vida-da-aba.md),
    /// [ADR-0034](../../../docs/adr/0034-deteccao-de-processo-ativo-para-confirmacao.md).
    /// Governa os dois sinais de "processo ativo": modo do terminal
    /// (alt-screen/mouse reporting) e, desde o ADR-0034, contagem de
    /// processos no grupo do Job Object (Windows).
    pub confirm_close_with_process: bool,
    /// RF-10.23.
    pub confirm_close_window: bool,
    /// `"home"` ou um caminho absoluto. RF-1.1.
    pub startup_directory: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            confirm_close_with_process: true,
            confirm_close_window: true,
            startup_directory: "home".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(
            General::default(),
            General {
                confirm_close_with_process: true,
                confirm_close_window: true,
                startup_directory: "home".to_owned(),
            }
        );
    }
}
