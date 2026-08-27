// SPDX-License-Identifier: GPL-3.0-or-later

//! Popover de destino do `tab.move_to_group` (RF-2.20, ADR-0023 §4): "a
//! mesma anatomia do menu de contexto, com uma linha por grupo -- swatch,
//! nome truncado, contagem -- mais 'Novo grupo' no fim." Abre no lugar do
//! item "Mover para grupo" do menu de aba, em vez de executar direto --
//! mesmo padrão de `group.set_color` abrindo o editor.
//!
//! É a primeira lista rolável do chrome (ADR-0023): diferente do menu de
//! contexto (§2.16, "não rola"), o tamanho aqui é o número de grupos do
//! usuário, não conhecido em tempo de escrita. Este módulo não guarda
//! deslocamento de rolagem -- `overlay.rs` deriva o quanto rolar a partir
//! de `highlighted`, pra manter o item realçado sempre visível, o mesmo
//! espírito de `WindowState::ensure_active_tab_visible` pra trilha.

use porecatu_core::{GroupId, TabId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveTarget {
    Group(GroupId),
    /// Espec.: "mais 'Novo grupo' no fim" -- sempre a última linha.
    NewGroup,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MoveToGroupPopover {
    pub tab: TabId,
    /// Coordenadas lógicas da janela -- mesmo ponto onde o menu de aba que
    /// abriu isto estava ancorado (ADR-0023 §4 não introduz submenu: é uma
    /// substituição de superfície, não um filho cascateando).
    pub anchor: (f32, f32),
    /// Grupos explícitos candidatos, na ordem visual -- já exclui o grupo
    /// atual da aba (mover pra onde já está é no-op, `Workspace::
    /// move_tab_to_group` também recusa). "Novo grupo" não entra aqui --
    /// é sempre a linha de índice `targets.len()`, pra não duplicar a
    /// contagem em dois lugares.
    targets: Vec<GroupId>,
    highlighted: usize,
}

impl MoveToGroupPopover {
    pub fn new(tab: TabId, anchor: (f32, f32), targets: Vec<GroupId>) -> Self {
        Self {
            tab,
            anchor,
            targets,
            highlighted: 0,
        }
    }

    pub fn targets(&self) -> &[GroupId] {
        &self.targets
    }

    pub const fn highlighted(&self) -> usize {
        self.highlighted
    }

    pub fn row_count(&self) -> usize {
        self.targets.len() + 1
    }

    pub fn move_highlight(&mut self, delta: isize) {
        let len = self.row_count() as isize;
        self.highlighted = (self.highlighted as isize + delta).rem_euclid(len) as usize;
    }

    pub fn set_highlight(&mut self, index: usize) {
        if index < self.row_count() {
            self.highlighted = index;
        }
    }

    pub fn selected(&self) -> MoveTarget {
        if self.highlighted < self.targets.len() {
            MoveTarget::Group(self.targets[self.highlighted])
        } else {
            MoveTarget::NewGroup
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(n: u32) -> GroupId {
        GroupId::new(n)
    }

    #[test]
    fn new_group_is_always_the_last_row() {
        let popover = MoveToGroupPopover::new(TabId::new(0), (0.0, 0.0), vec![g(1), g(2)]);
        assert_eq!(popover.row_count(), 3);
        let mut popover = popover;
        popover.set_highlight(2);
        assert_eq!(popover.selected(), MoveTarget::NewGroup);
    }

    #[test]
    fn empty_targets_still_offer_new_group() {
        let popover = MoveToGroupPopover::new(TabId::new(0), (0.0, 0.0), vec![]);
        assert_eq!(popover.row_count(), 1);
        assert_eq!(popover.selected(), MoveTarget::NewGroup);
    }

    #[test]
    fn move_highlight_wraps_across_all_rows() {
        let mut popover = MoveToGroupPopover::new(TabId::new(0), (0.0, 0.0), vec![g(1)]);
        assert_eq!(popover.selected(), MoveTarget::Group(g(1)));
        popover.move_highlight(-1);
        assert_eq!(popover.selected(), MoveTarget::NewGroup);
        popover.move_highlight(1);
        assert_eq!(popover.selected(), MoveTarget::Group(g(1)));
    }

    #[test]
    fn set_highlight_ignores_out_of_range() {
        let mut popover = MoveToGroupPopover::new(TabId::new(0), (0.0, 0.0), vec![g(1)]);
        popover.set_highlight(99);
        assert_eq!(popover.selected(), MoveTarget::Group(g(1)));
    }
}
