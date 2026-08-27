// SPDX-License-Identifier: GPL-3.0-or-later

//! `Group` (ADR-0006). Na F2 só existe o grupo implícito: sem nome, sem
//! cor, não desenhado como pílula, não renomeável nem colapsável. Nome,
//! cor e colapso do grupo explícito são F3 (`group.create` etc. em
//! docs/reference/acoes.md) -- adicionados quando houver operação que os
//! use, não antes.

use serde::{Deserialize, Serialize};

use crate::id::{GroupId, TabId};

/// Abas contíguas, na ordem em que aparecem na barra (ADR-0006: "grupos são
/// contíguos na barra; a ordem visual é a ordem do modelo").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    id: GroupId,
    tabs: Vec<TabId>,
}

impl Group {
    pub const fn new(id: GroupId) -> Self {
        Self {
            id,
            tabs: Vec::new(),
        }
    }

    pub const fn id(&self) -> GroupId {
        self.id
    }

    pub fn tabs(&self) -> &[TabId] {
        &self.tabs
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn position_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|&t| t == id)
    }

    /// Insere na posição, saturando no fim se `pos` estourar o tamanho
    /// atual -- quem chama (`Workspace`) não precisa clampar antes.
    pub(crate) fn insert(&mut self, pos: usize, id: TabId) {
        let pos = pos.min(self.tabs.len());
        self.tabs.insert(pos, id);
    }

    /// Remove a aba, se presente. Devolve a posição em que ela estava, para
    /// que o chamador calcule a vizinha (RF-1.5) antes de perder a
    /// informação de posição.
    pub(crate) fn remove(&mut self, id: TabId) -> Option<usize> {
        let pos = self.position_of(id)?;
        self.tabs.remove(pos);
        Some(pos)
    }

    /// Move dentro do próprio grupo (RF-1.15, RF-1.17). Mover entre grupos
    /// é `tab.move_to_group`, F3 -- fora do escopo deste método de
    /// propósito.
    pub(crate) fn move_within(&mut self, id: TabId, pos: usize) -> bool {
        let Some(from) = self.position_of(id) else {
            return false;
        };
        let pos = pos.min(self.tabs.len() - 1);
        if from == pos {
            return true;
        }
        self.tabs.remove(from);
        self.tabs.insert(pos, id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_saturates_at_end() {
        let mut group = Group::new(GroupId::new(0));
        group.insert(0, TabId::new(1));
        group.insert(99, TabId::new(2));
        assert_eq!(group.tabs(), [TabId::new(1), TabId::new(2)]);
    }

    #[test]
    fn remove_returns_previous_position() {
        let mut group = Group::new(GroupId::new(0));
        group.insert(0, TabId::new(1));
        group.insert(1, TabId::new(2));
        assert_eq!(group.remove(TabId::new(1)), Some(0));
        assert_eq!(group.tabs(), [TabId::new(2)]);
        assert_eq!(group.remove(TabId::new(1)), None);
    }

    #[test]
    fn move_within_reorders() {
        let mut group = Group::new(GroupId::new(0));
        group.insert(0, TabId::new(1));
        group.insert(1, TabId::new(2));
        group.insert(2, TabId::new(3));
        assert!(group.move_within(TabId::new(3), 0));
        assert_eq!(group.tabs(), [TabId::new(3), TabId::new(1), TabId::new(2)]);
    }
}
