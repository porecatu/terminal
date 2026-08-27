// SPDX-License-Identifier: GPL-3.0-or-later

//! Identidade de aba e de grupo (ADR-0006). Inteiros opacos e estáveis,
//! gerados por contador monotônico por `Workspace` -- índice de posição não
//! serve como identidade porque reordenar invalidaria referências (sessão
//! salva, `Wakeup` da thread de leitura do PTY, drag em andamento).

use serde::{Deserialize, Serialize};

/// Identificador opaco de aba, estável dentro de um workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TabId(u32);

impl TabId {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Identificador opaco de grupo, estável dentro de um workspace. O grupo
/// implícito (ADR-0006) tem um `GroupId` como qualquer outro -- é um grupo
/// de verdade para fins de identidade, só não é desenhado como pílula e não
/// pode ser renomeado, colapsado ou removido.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GroupId(u32);

impl GroupId {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}
