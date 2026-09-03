// SPDX-License-Identifier: GPL-3.0-or-later

//! `[shell]` e `[shell.env]` -- PRD-000, PRD-001. Classe de recarga C:
//! "vale em aba nova", a aba já aberta tem processo já lançado.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(default)]
pub struct Shell {
    /// Vazio = detecta automaticamente (ver comentário do arquivo de
    /// exemplo para a cadeia por plataforma).
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(
            Shell::default(),
            Shell {
                program: String::new(),
                args: Vec::new(),
                env: BTreeMap::new(),
            }
        );
    }
}
