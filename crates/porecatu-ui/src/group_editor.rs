// SPDX-License-Identifier: GPL-3.0-or-later

//! Editor de grupo (ADR-0023): quinto widget de chrome, popover com três
//! regiões navegáveis por `Tab`/`Shift+Tab` -- campo de nome, faixa de
//! seis swatches, lista de ações (subconjunto de
//! `group_menu::group_action_items`, RF-10.21). Estado puro, testável sem
//! `winit` -- mesmo padrão de `context_menu.rs`/`dialog.rs`/`rename.rs`/
//! `tooltip.rs`.
//!
//! **Edição de nome ao vivo** (espec. §2.10, item 1) reaproveita o truque
//! do rename de aba (F2, `rename.rs`): o buffer nunca é escrito no
//! `Workspace` enquanto edita -- quem pinta prefere o buffer ao nome real
//! (`chrome.rs`), e `Esc` só precisa descartar este estado, sem restaurar
//! nada, porque o modelo nunca mudou. `Enter` confirma escrevendo o buffer
//! de verdade (`lib.rs`). **Desde o ADR-0035** o campo tem cursor navegável
//! e seleção (`text_field::TextFieldState`), superando a simplificação
//! "sempre no fim" que este ADR (§3) e o rename de aba assumiam.
//!
//! **Cor e ações não são ao vivo**: mover o realce entre swatches ou itens
//! da lista não aplica nada -- mesma regra do menu de contexto, "`Enter`
//! aciona o que está realçado" (espec. §2.10/§2.16). Só
//! `selected_color`/`selected_action` mais `Enter` (ou clique), do lado de
//! fora deste módulo, tocam o `Workspace`.

use porecatu_core::GroupId;

use crate::group_menu::{EDITOR_ACTION_ORDER, GroupAction};
use crate::text_field::TextFieldState;

/// As três regiões navegáveis (espec. §2.10: "`Tab`/`Shift+Tab` percorrem
/// as três regiões -- campo, faixa de swatches, lista de ações").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorRegion {
    Name,
    Swatches,
    Actions,
}

const REGION_ORDER: [EditorRegion; 3] = [
    EditorRegion::Name,
    EditorRegion::Swatches,
    EditorRegion::Actions,
];

/// Seis swatches na mesma ordem de `GroupColor::ALL` (`porecatu-core`).
const SWATCH_COUNT: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct GroupEditor {
    pub group: GroupId,
    focus: EditorRegion,
    name_field: TextFieldState,
    swatch_highlight: usize,
    action_highlight: usize,
}

impl GroupEditor {
    /// `initial_focus`: `group.rename` abre com foco no campo,
    /// `group.set_color` com foco na faixa (ADR-0023 §4); duplo clique na
    /// pílula abre com foco no campo, mesmo default de `group.rename`.
    /// `current_color_index` é `GroupColor::index()` da cor atual do grupo
    /// -- de onde o realce da faixa começa.
    pub fn new(
        group: GroupId,
        current_name: &str,
        current_color_index: usize,
        initial_focus: EditorRegion,
    ) -> Self {
        Self {
            group,
            focus: initial_focus,
            name_field: TextFieldState::new(current_name),
            swatch_highlight: current_color_index.min(SWATCH_COUNT - 1),
            action_highlight: 0,
        }
    }

    pub const fn focus(&self) -> EditorRegion {
        self.focus
    }

    pub fn name_buffer(&self) -> &str {
        self.name_field.text()
    }

    /// Estado completo do campo (cursor, seleção) -- usado por quem pinta
    /// (`overlay.rs`) e por quem faz hit-test de caractere no clique.
    pub const fn name_field(&self) -> &TextFieldState {
        &self.name_field
    }

    /// `Some` só com o foco no campo -- usado por `lib.rs::handle_group_editor_key`
    /// pra repassar `Ctrl+A`/`Shift+seta`/`Home`/`End` a `apply_text_field_key`
    /// (ADR-0035), mesmo caminho que `RenameState::field`/`push_char`/
    /// `backspace` já guardam internamente.
    pub fn name_field_mut(&mut self) -> Option<&mut TextFieldState> {
        if self.focus == EditorRegion::Name {
            Some(&mut self.name_field)
        } else {
            None
        }
    }

    pub const fn swatch_highlight(&self) -> usize {
        self.swatch_highlight
    }

    pub const fn action_highlight(&self) -> usize {
        self.action_highlight
    }

    pub fn backspace(&mut self) {
        if self.focus == EditorRegion::Name {
            self.name_field.backspace();
        }
    }

    /// Clique no campo (`lib.rs::dispatch_group_editor_click`): sempre
    /// move o foco pra `Name`, mesmo que já estivesse noutra região -- é
    /// clicar no campo que o traz ao primeiro plano.
    pub fn click_name_at(&mut self, byte_index: usize) {
        self.focus = EditorRegion::Name;
        self.name_field.click_at(byte_index);
    }

    /// Arraste dentro do campo (`lib.rs::dispatch_cursor_moved`) -- só tem
    /// efeito com o foco já no campo (arma-se no `click_name_at`
    /// anterior).
    pub fn drag_name_to(&mut self, byte_index: usize) {
        if self.focus == EditorRegion::Name {
            self.name_field.drag_to(byte_index);
        }
    }

    /// `Tab`/`Shift+Tab`.
    pub fn cycle_focus(&mut self, forward: bool) {
        let idx = REGION_ORDER
            .iter()
            .position(|&r| r == self.focus)
            .unwrap_or(0) as isize;
        let delta = if forward { 1 } else { -1 };
        let next = (idx + delta).rem_euclid(REGION_ORDER.len() as isize) as usize;
        self.focus = REGION_ORDER[next];
    }

    pub fn set_focus(&mut self, focus: EditorRegion) {
        self.focus = focus;
    }

    /// `ArrowUp`/`ArrowDown`: dentro da faixa movem `swatch_highlight`,
    /// dentro da lista movem `action_highlight`. Sem efeito no campo --
    /// `ArrowLeft`/`ArrowRight`/`Home`/`End` são as setas do texto (ADR-0035,
    /// `move_name_left`/`move_name_right`/`move_name_home`/`move_name_end`),
    /// tratadas fora daqui.
    pub fn move_highlight(&mut self, delta: isize) {
        match self.focus {
            EditorRegion::Swatches => {
                self.swatch_highlight = (self.swatch_highlight as isize + delta)
                    .rem_euclid(SWATCH_COUNT as isize)
                    as usize;
            }
            EditorRegion::Actions => {
                let len = EDITOR_ACTION_ORDER.len() as isize;
                self.action_highlight =
                    (self.action_highlight as isize + delta).rem_euclid(len) as usize;
            }
            EditorRegion::Name => {}
        }
    }

    pub fn set_swatch_highlight(&mut self, index: usize) {
        if index < SWATCH_COUNT {
            self.swatch_highlight = index;
        }
    }

    pub fn set_action_highlight(&mut self, index: usize) {
        if index < EDITOR_ACTION_ORDER.len() {
            self.action_highlight = index;
        }
    }

    pub const fn selected_action(&self) -> GroupAction {
        EDITOR_ACTION_ORDER[self.action_highlight]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g() -> GroupId {
        GroupId::new(0)
    }

    #[test]
    fn new_starts_with_given_focus_and_name() {
        let editor = GroupEditor::new(g(), "trabalho", 2, EditorRegion::Swatches);
        assert_eq!(editor.focus(), EditorRegion::Swatches);
        assert_eq!(editor.name_buffer(), "trabalho");
        assert_eq!(editor.swatch_highlight(), 2);
    }

    #[test]
    fn backspace_and_name_field_mut_only_affect_buffer_in_name_focus() {
        let mut editor = GroupEditor::new(g(), "xy", 0, EditorRegion::Swatches);
        assert!(editor.name_field_mut().is_none());
        editor.backspace();
        assert_eq!(
            editor.name_buffer(),
            "xy",
            "backspace fora do foco do campo é no-op"
        );

        editor.set_focus(EditorRegion::Name);
        editor.name_field_mut().unwrap().insert_char('!');
        assert_eq!(editor.name_buffer(), "xy!");
        editor.backspace();
        assert_eq!(editor.name_buffer(), "xy");
    }

    #[test]
    fn cycle_focus_forward_goes_through_all_three_and_wraps() {
        let mut editor = GroupEditor::new(g(), "", 0, EditorRegion::Name);
        editor.cycle_focus(true);
        assert_eq!(editor.focus(), EditorRegion::Swatches);
        editor.cycle_focus(true);
        assert_eq!(editor.focus(), EditorRegion::Actions);
        editor.cycle_focus(true);
        assert_eq!(editor.focus(), EditorRegion::Name);
    }

    #[test]
    fn cycle_focus_backward_wraps() {
        let mut editor = GroupEditor::new(g(), "", 0, EditorRegion::Name);
        editor.cycle_focus(false);
        assert_eq!(editor.focus(), EditorRegion::Actions);
    }

    #[test]
    fn move_highlight_in_swatches_wraps_at_six() {
        let mut editor = GroupEditor::new(g(), "", 5, EditorRegion::Swatches);
        editor.move_highlight(1);
        assert_eq!(editor.swatch_highlight(), 0);
        editor.move_highlight(-1);
        assert_eq!(editor.swatch_highlight(), 5);
    }

    #[test]
    fn move_highlight_in_actions_wraps_at_three() {
        let mut editor = GroupEditor::new(g(), "", 0, EditorRegion::Actions);
        editor.move_highlight(-1);
        assert_eq!(editor.action_highlight(), 2);
        assert_eq!(editor.selected_action(), GroupAction::CloseAll);
    }

    #[test]
    fn move_highlight_in_name_region_is_noop() {
        let mut editor = GroupEditor::new(g(), "", 0, EditorRegion::Name);
        editor.move_highlight(1);
        assert_eq!(editor.swatch_highlight(), 0);
        assert_eq!(editor.action_highlight(), 0);
    }

    #[test]
    fn selected_action_starts_at_toggle_collapse() {
        let editor = GroupEditor::new(g(), "", 0, EditorRegion::Actions);
        assert_eq!(editor.selected_action(), GroupAction::ToggleCollapse);
    }
}
