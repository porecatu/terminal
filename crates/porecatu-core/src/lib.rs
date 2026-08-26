// SPDX-License-Identifier: GPL-3.0-or-later

//! Modelo de domínio puro (docs/arquitetura.md seção 1). Nesta fase (F1) só
//! existe o necessário para o `Wakeup { window, tab }` do ADR-0015 já
//! nascer com o formato certo -- `Workspace`/`Tab`/`Group` completos
//! (ADR-0006) são da F2.

/// Identificador opaco de aba, estável dentro de um workspace. Gerado por
/// contador monotônico por workspace (ADR-0006) -- em F1 existe uma única
/// aba por janela, então não há Workspace ainda para possuir esse contador;
/// quem cria o `TabId` hoje é `porecatu-ui`, com o valor fixo `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(u32);

impl TabId {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}
