// SPDX-License-Identifier: GPL-3.0-or-later

//! `[task_manager]` -- PRD-024, ADR-0095 §9. Comportamento do gerenciador de
//! tarefas. Classe de recarga A: a chave é lida a cada amostra e a cada
//! pintura pelo `Config` corrente, então não há trabalho extra de hot
//! reload.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct TaskManager {
    /// Period shown by the task manager charts, in seconds (RF-24.26).
    /// The 30..=600 range is applied in the UI (`effective_history`), which
    /// warns once when it adjusts the value, like the Git poll interval.
    pub history_seconds: u32,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self {
            history_seconds: 60,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(TaskManager::default().history_seconds, 60);
    }
}
