// SPDX-License-Identifier: GPL-3.0-or-later

//! Modo de captura do rename inline (RF-1.8/RF-1.9). ADR-0008 passo 1:
//! enquanto ativo, consome toda tecla exceto `Esc` e `Enter` -- nenhuma
//! delas chega ao roteamento de keybind nem ao terminal. **Desde o
//! ADR-0035**, o campo tem cursor navegável e seleção
//! (`text_field::TextFieldState`), superando a simplificação "sempre no
//! fim do buffer" que `chrome.rs` assumia ao desenhar o caret.

use porecatu_core::TabId;

use crate::text_field::TextFieldState;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RenameState {
    #[default]
    Idle,
    Editing {
        tab: TabId,
        field: TextFieldState,
    },
}

impl RenameState {
    pub fn editing_tab(&self) -> Option<TabId> {
        match self {
            RenameState::Editing { tab, .. } => Some(*tab),
            RenameState::Idle => None,
        }
    }

    /// Estado completo do campo (cursor, seleção), ou `None` fora do modo
    /// de edição -- usado por quem pinta e por quem faz hit-test de
    /// caractere no clique/arraste.
    pub fn field(&self) -> Option<&TextFieldState> {
        match self {
            RenameState::Editing { field, .. } => Some(field),
            RenameState::Idle => None,
        }
    }

    pub fn backspace(&mut self) {
        if let RenameState::Editing { field, .. } = self {
            field.backspace();
        }
    }

    /// Clique dentro do campo -- posiciona o cursor sem seleção. No-op
    /// fora do modo de edição (chamador confere `editing_tab` antes).
    pub fn click_at(&mut self, byte_index: usize) {
        if let RenameState::Editing { field, .. } = self {
            field.click_at(byte_index);
        }
    }

    /// Arraste dentro do campo -- estende/move a seleção.
    pub fn drag_to(&mut self, byte_index: usize) {
        if let RenameState::Editing { field, .. } = self {
            field.drag_to(byte_index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_has_no_field() {
        assert_eq!(RenameState::Idle.field(), None);
        assert_eq!(RenameState::Idle.editing_tab(), None);
    }

    #[test]
    fn editing_tracks_tab_and_buffer() {
        let state = RenameState::Editing {
            tab: TabId::new(3),
            field: TextFieldState::new("zsh"),
        };
        assert_eq!(state.editing_tab(), Some(TabId::new(3)));
        assert_eq!(state.field().unwrap().text(), "zsh");
    }

    #[test]
    fn backspace_edits_buffer() {
        let mut state = RenameState::Editing {
            tab: TabId::new(0),
            field: TextFieldState::new("ab"),
        };
        state.backspace();
        assert_eq!(state.field().unwrap().text(), "a");
    }

    #[test]
    fn backspace_on_empty_buffer_is_noop() {
        let mut state = RenameState::Editing {
            tab: TabId::new(0),
            field: TextFieldState::new(""),
        };
        state.backspace();
        assert_eq!(state.field().unwrap().text(), "");
    }

    #[test]
    fn backspace_on_idle_is_noop() {
        let mut state = RenameState::Idle;
        state.backspace();
        assert_eq!(state, RenameState::Idle);
    }

    #[test]
    fn click_and_drag_select_a_range() {
        let mut state = RenameState::Editing {
            tab: TabId::new(0),
            field: TextFieldState::new("abcdef"),
        };
        state.click_at(1);
        state.drag_to(4);
        assert_eq!(state.field().unwrap().selection_range(), Some((1, 4)));
    }
}
