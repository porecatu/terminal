// SPDX-License-Identifier: GPL-3.0-or-later

//! `Group` (ADR-0006, revisto pelo ADR-0020). Um `Group` é implícito ou
//! explícito, com o mesmo tipo e um discriminante -- `GroupKind` -- em vez
//! de dois tipos distintos: o resto da aplicação nunca lida com
//! `Option<GroupId>` (ADR-0006), e agora também nunca lida com dois tipos
//! de grupo.
//!
//! O grupo implícito deixou de ser único na F3 (ADR-0020 §1): há um por
//! *run* contíguo de abas sem grupo. `GroupId` de grupo implícito não é
//! identidade estável entre sessões -- só o de grupo explícito é.

use serde::{Deserialize, Serialize};

use crate::id::{GroupId, TabId};

/// Seis cores nomeadas, na ordem da paleta de grupos (espec. visual
/// §1.6) -- é também a ordem de atribuição automática (RF-2.4,
/// ADR-0020 §5). O valor hexadecimal concreto é responsabilidade de
/// `porecatu-ui`; este tipo só carrega a escolha, não a cor resolvida --
/// mesma separação que `porecatu-term::TermColor` faz para a grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupColor {
    Red,
    Yellow,
    Cyan,
    Blue,
    Purple,
    Green,
}

impl GroupColor {
    /// Na ordem da espec. visual §1.6 -- é o que `Workspace::next_auto_color`
    /// (ADR-0020 §5) percorre.
    pub const ALL: [GroupColor; 6] = [
        Self::Red,
        Self::Yellow,
        Self::Cyan,
        Self::Blue,
        Self::Purple,
        Self::Green,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Red => 0,
            Self::Yellow => 1,
            Self::Cyan => 2,
            Self::Blue => 3,
            Self::Purple => 4,
            Self::Green => 5,
        }
    }
}

/// Metadados de um grupo explícito (ADR-0020 §1). Nome vazio é válido
/// (RF-2.9: "o grupo aparece apenas como um marcador colorido").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMeta {
    name: String,
    color: GroupColor,
    collapsed: bool,
}

impl GroupMeta {
    fn new(name: String, color: GroupColor) -> Self {
        Self {
            name,
            color,
            collapsed: false,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn color(&self) -> GroupColor {
        self.color
    }

    pub const fn is_collapsed(&self) -> bool {
        self.collapsed
    }
}

/// Discriminante de `Group` (ADR-0020 §1). Implícito não tem nome, cor
/// nem colapso -- não pode ser renomeado, recolorido ou colapsado
/// (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupKind {
    Implicit,
    Explicit(GroupMeta),
}

/// Abas contíguas, na ordem em que aparecem na barra (ADR-0006: "grupos
/// são contíguos na barra; a ordem visual é a ordem do modelo").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    id: GroupId,
    kind: GroupKind,
    tabs: Vec<TabId>,
    /// MRU do grupo (ADR-0020 §6), usado por `group.next`/`group.prev`
    /// (RF-2.21). Não persistido -- a lista do ADR-0005 não o inclui.
    last_active: Option<TabId>,
}

impl Group {
    pub const fn new_implicit(id: GroupId) -> Self {
        Self {
            id,
            kind: GroupKind::Implicit,
            tabs: Vec::new(),
            last_active: None,
        }
    }

    pub fn new_explicit(id: GroupId, name: impl Into<String>, color: GroupColor) -> Self {
        Self {
            id,
            kind: GroupKind::Explicit(GroupMeta::new(name.into(), color)),
            tabs: Vec::new(),
            last_active: None,
        }
    }

    pub const fn id(&self) -> GroupId {
        self.id
    }

    pub const fn kind(&self) -> &GroupKind {
        &self.kind
    }

    pub const fn is_implicit(&self) -> bool {
        matches!(self.kind, GroupKind::Implicit)
    }

    pub const fn is_explicit(&self) -> bool {
        !self.is_implicit()
    }

    /// `None` para grupo implícito -- ele não tem nome (ADR-0006).
    pub fn name(&self) -> Option<&str> {
        match &self.kind {
            GroupKind::Explicit(meta) => Some(meta.name()),
            GroupKind::Implicit => None,
        }
    }

    pub const fn color(&self) -> Option<GroupColor> {
        match &self.kind {
            GroupKind::Explicit(meta) => Some(meta.color()),
            GroupKind::Implicit => None,
        }
    }

    /// Sempre `false` para grupo implícito -- ele não pode ser colapsado
    /// (ADR-0006).
    pub const fn is_collapsed(&self) -> bool {
        match &self.kind {
            GroupKind::Explicit(meta) => meta.is_collapsed(),
            GroupKind::Implicit => false,
        }
    }

    /// RF-2.9. Sem efeito (devolve `false`) sobre grupo implícito.
    pub fn rename(&mut self, name: impl Into<String>) -> bool {
        match &mut self.kind {
            GroupKind::Explicit(meta) => {
                meta.name = name.into();
                true
            }
            GroupKind::Implicit => false,
        }
    }

    /// RF-2.10. Sem efeito (devolve `false`) sobre grupo implícito.
    pub fn set_color(&mut self, color: GroupColor) -> bool {
        match &mut self.kind {
            GroupKind::Explicit(meta) => {
                meta.color = color;
                true
            }
            GroupKind::Implicit => false,
        }
    }

    /// RF-2.13. Sem efeito (devolve `false`) sobre grupo implícito.
    pub fn set_collapsed(&mut self, collapsed: bool) -> bool {
        match &mut self.kind {
            GroupKind::Explicit(meta) => {
                meta.collapsed = collapsed;
                true
            }
            GroupKind::Implicit => false,
        }
    }

    pub const fn last_active(&self) -> Option<TabId> {
        self.last_active
    }

    /// Chamado por `Workspace::activate_tab` quando a aba ativada pertence
    /// a este grupo.
    pub(crate) fn set_last_active(&mut self, id: TabId) {
        debug_assert!(self.tabs.contains(&id), "aba precisa pertencer ao grupo");
        self.last_active = Some(id);
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
    /// informação de posição. Se a aba removida era `last_active`
    /// (ADR-0020 §6), o campo cai para `None`.
    pub(crate) fn remove(&mut self, id: TabId) -> Option<usize> {
        let pos = self.position_of(id)?;
        self.tabs.remove(pos);
        if self.last_active == Some(id) {
            self.last_active = None;
        }
        Some(pos)
    }

    /// Remove todas as abas para as quais `keep` devolve `false`.
    /// `last_active` cai para `None` se a aba que ele apontava foi
    /// removida.
    pub(crate) fn retain_tabs(&mut self, mut keep: impl FnMut(TabId) -> bool) {
        self.tabs.retain(|&t| keep(t));
        if let Some(active) = self.last_active
            && !self.tabs.contains(&active)
        {
            self.last_active = None;
        }
    }

    /// Consome o grupo, devolvendo só as abas -- usado por
    /// `Workspace::group_tabs`/`ungroup` para reconstruir a lista de
    /// grupos.
    pub(crate) fn into_tabs(self) -> Vec<TabId> {
        self.tabs
    }

    /// Move dentro do próprio grupo (RF-1.15, RF-1.17). Mover entre grupos
    /// é `tab.move_to_group`/arraste entre grupos (F3, etapa 6) -- fora do
    /// escopo deste método de propósito.
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
        let mut group = Group::new_implicit(GroupId::new(0));
        group.insert(0, TabId::new(1));
        group.insert(99, TabId::new(2));
        assert_eq!(group.tabs(), [TabId::new(1), TabId::new(2)]);
    }

    #[test]
    fn remove_returns_previous_position() {
        let mut group = Group::new_implicit(GroupId::new(0));
        group.insert(0, TabId::new(1));
        group.insert(1, TabId::new(2));
        assert_eq!(group.remove(TabId::new(1)), Some(0));
        assert_eq!(group.tabs(), [TabId::new(2)]);
        assert_eq!(group.remove(TabId::new(1)), None);
    }

    #[test]
    fn move_within_reorders() {
        let mut group = Group::new_implicit(GroupId::new(0));
        group.insert(0, TabId::new(1));
        group.insert(1, TabId::new(2));
        group.insert(2, TabId::new(3));
        assert!(group.move_within(TabId::new(3), 0));
        assert_eq!(group.tabs(), [TabId::new(3), TabId::new(1), TabId::new(2)]);
    }

    #[test]
    fn implicit_group_has_no_name_color_or_collapse() {
        let mut group = Group::new_implicit(GroupId::new(0));
        assert_eq!(group.name(), None);
        assert_eq!(group.color(), None);
        assert!(!group.is_collapsed());
        assert!(!group.rename("x"));
        assert!(!group.set_color(GroupColor::Blue));
        assert!(!group.set_collapsed(true));
    }

    #[test]
    fn explicit_group_carries_meta() {
        let mut group = Group::new_explicit(GroupId::new(1), "api", GroupColor::Blue);
        assert_eq!(group.name(), Some("api"));
        assert_eq!(group.color(), Some(GroupColor::Blue));
        assert!(!group.is_collapsed());
        assert!(group.set_collapsed(true));
        assert!(group.is_collapsed());
        assert!(group.rename("backend"));
        assert_eq!(group.name(), Some("backend"));
    }

    #[test]
    fn last_active_clears_when_removed() {
        let mut group = Group::new_implicit(GroupId::new(0));
        group.insert(0, TabId::new(1));
        group.set_last_active(TabId::new(1));
        assert_eq!(group.last_active(), Some(TabId::new(1)));
        group.remove(TabId::new(1));
        assert_eq!(group.last_active(), None);
    }

    #[test]
    fn last_active_clears_on_retain() {
        let mut group = Group::new_implicit(GroupId::new(0));
        group.insert(0, TabId::new(1));
        group.insert(1, TabId::new(2));
        group.set_last_active(TabId::new(1));
        group.retain_tabs(|t| t != TabId::new(1));
        assert_eq!(group.last_active(), None);
        assert_eq!(group.tabs(), [TabId::new(2)]);
    }

    #[test]
    fn color_all_matches_spec_order_and_index() {
        for (i, color) in GroupColor::ALL.iter().enumerate() {
            assert_eq!(color.index(), i);
        }
    }
}
