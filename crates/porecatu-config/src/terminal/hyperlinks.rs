// SPDX-License-Identifier: GPL-3.0-or-later

//! `[terminal.hyperlinks]` -- OSC 8, PRD-011 RF-11.10 a RF-11.13,
//! ADR-0042. Classe de recarga A.
//!
//! Única chave de propósito: a lista de esquemas aceitos não é
//! configurável (ADR-0042 §5) -- mover a fronteira de segurança para um
//! arquivo de texto copiado de terceiros transformaria a config em vetor
//! de ataque. O que se pode desligar é o recurso inteiro, affordance
//! inclusa.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Hyperlinks {
    pub enabled: bool,
}

impl Default for Hyperlinks {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert!(Hyperlinks::default().enabled);
    }
}
