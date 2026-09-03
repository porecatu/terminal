// SPDX-License-Identifier: GPL-3.0-or-later

//! `[terminal.clipboard]` -- PRD-010, ADR-0013. OSC 52. Classe de recarga
//! A.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Clipboard {
    /// Faz copiar de dentro de um tmux/nvim por SSH funcionar. RF-10.10.
    pub osc52_write: bool,
    /// Negado por default: leitura remota do clipboard sem aviso. RF-10.11.
    pub osc52_read: bool,
    /// Teto do payload de escrita, em bytes. RF-10.10.
    pub osc52_max_bytes: u64,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self {
            osc52_write: true,
            osc52_read: false,
            osc52_max_bytes: 102_400,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert!(Clipboard::default().osc52_write);
        assert!(!Clipboard::default().osc52_read);
        assert_eq!(Clipboard::default().osc52_max_bytes, 102_400);
    }
}
