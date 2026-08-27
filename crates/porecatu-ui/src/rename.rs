// SPDX-License-Identifier: GPL-3.0-or-later

//! Modo de captura do rename inline (RF-1.8/RF-1.9). ADR-0008 passo 1:
//! enquanto ativo, consome toda tecla exceto `Esc` e `Enter` -- nenhuma
//! delas chega ao roteamento de keybind nem ao terminal. Edição pura, sem
//! posição de cursor no meio da string (sempre no fim do buffer) -- é a
//! simplificação que `chrome.rs` já assume ao desenhar o caret.

use porecatu_core::TabId;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RenameState {
    #[default]
    Idle,
    Editing {
        tab: TabId,
        buffer: String,
    },
}

impl RenameState {
    pub fn editing_tab(&self) -> Option<TabId> {
        match self {
            RenameState::Editing { tab, .. } => Some(*tab),
            RenameState::Idle => None,
        }
    }

    /// Buffer atual, ou string vazia fora do modo de edição -- conveniência
    /// pra quem pinta (`chrome.rs`), que já checou `editing_tab` antes de
    /// chamar isto.
    pub fn buffer(&self) -> &str {
        match self {
            RenameState::Editing { buffer, .. } => buffer,
            RenameState::Idle => "",
        }
    }

    pub fn push_char(&mut self, c: char) {
        if let RenameState::Editing { buffer, .. } = self {
            buffer.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if let RenameState::Editing { buffer, .. } = self {
            buffer.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_buffer_is_empty() {
        assert_eq!(RenameState::Idle.buffer(), "");
        assert_eq!(RenameState::Idle.editing_tab(), None);
    }

    #[test]
    fn editing_tracks_tab_and_buffer() {
        let state = RenameState::Editing {
            tab: TabId::new(3),
            buffer: "zsh".to_string(),
        };
        assert_eq!(state.editing_tab(), Some(TabId::new(3)));
        assert_eq!(state.buffer(), "zsh");
    }

    #[test]
    fn push_and_backspace_edit_buffer() {
        let mut state = RenameState::Editing {
            tab: TabId::new(0),
            buffer: String::new(),
        };
        state.push_char('a');
        state.push_char('b');
        assert_eq!(state.buffer(), "ab");
        state.backspace();
        assert_eq!(state.buffer(), "a");
    }

    #[test]
    fn backspace_on_empty_buffer_is_noop() {
        let mut state = RenameState::Editing {
            tab: TabId::new(0),
            buffer: String::new(),
        };
        state.backspace();
        assert_eq!(state.buffer(), "");
    }

    #[test]
    fn push_and_backspace_on_idle_are_noop() {
        let mut state = RenameState::Idle;
        state.push_char('a');
        state.backspace();
        assert_eq!(state, RenameState::Idle);
    }
}
