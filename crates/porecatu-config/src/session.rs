// SPDX-License-Identifier: GPL-3.0-or-later

//! `[session]` -- PRD-003. Classe de recarga C: "reinicie o app". Gravação
//! de sessão é F5; trocar destino com sessão em memória pediria migração
//! de arquivo.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Session {
    /// `false` = sempre abre com uma aba limpa. RF-3.6.
    pub enabled: bool,
    /// Atraso do agrupamento de gravações. RF-3.3.
    pub save_debounce_ms: u64,
    /// Só a aba ativa de cada janela inicia o shell no start. RF-3.8.
    pub lazy_restore: bool,
    pub restore_window_geometry: bool,
    /// Oferece uma vez o trecho de integração de shell quando OSC 7 não é
    /// detectado. RF-3.1.
    pub suggest_shell_integration: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            enabled: true,
            save_debounce_ms: 2000,
            lazy_restore: true,
            restore_window_geometry: true,
            suggest_shell_integration: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let session = Session::default();
        assert!(session.enabled);
        assert_eq!(session.save_debounce_ms, 2000);
    }
}
