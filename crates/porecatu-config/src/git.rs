// SPDX-License-Identifier: GPL-3.0-or-later

//! `[git]` -- PRD-013, ADR-0052. Sincronização com o remoto: consulta
//! periódica de quantos commits a branch da aba ativa está atrás/à frente.
//! Classe de recarga A: o prazo é recalculado a cada volta do laço de
//! eventos, lendo a config atual -- não há trabalho extra de hot reload.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Git {
    /// Intervalo entre consultas ao remoto, em segundos. `0` desliga o
    /// recurso inteiro: nenhuma consulta, nenhum indicador, nenhum prazo
    /// agendado. RF-13.1, RF-13.2. Valor entre 1 e 29 é elevado a 30 com
    /// aviso -- RF-13.3, piso decidido no ADR-0052 §10.
    pub remote_poll_interval_secs: u64,
}

impl Default for Git {
    fn default() -> Self {
        Self {
            remote_poll_interval_secs: 300,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(Git::default().remote_poll_interval_secs, 300);
    }
}
