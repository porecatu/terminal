// SPDX-License-Identifier: GPL-3.0-or-later

//! Diálogo de confirmação (ADR-0014, PRD-010 RF-10.18): modal por janela --
//! `App` carrega no máximo um por `WindowState`, o que já implementa "modal
//! é por janela, não por app" (ADR-0014, mitigação de risco). Foco inicial
//! no cancelar; `Enter` aciona o botão focado, `Esc` sempre cancela.
//! `action` é um enum fechado, não uma closure: o diálogo é dado puro, sem
//! capturar estado de `App` dentro dele.

use porecatu_core::TabId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogButton {
    Cancel,
    Confirm,
}

/// O que confirmar faz de verdade -- resolvido por `lib.rs`, que já tem
/// acesso ao `WindowState`/`App` que o diálogo não carrega.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogAction {
    /// RF-1.6 (ADR-0017): fechar aba com tela alternativa ou reporte de
    /// mouse ligado.
    CloseTab(TabId),
    /// RF-10.23 (ADR-0015): fechar janela com mais de uma aba.
    CloseWindow,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmDialog {
    pub title: String,
    pub body: String,
    pub confirm_label: String,
    pub action: DialogAction,
    focused: DialogButton,
}

impl ConfirmDialog {
    pub fn new(
        title: impl Into<String>,
        body: impl Into<String>,
        confirm_label: impl Into<String>,
        action: DialogAction,
    ) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            confirm_label: confirm_label.into(),
            action,
            focused: DialogButton::Cancel,
        }
    }

    pub const fn focused(&self) -> DialogButton {
        self.focused
    }

    /// Navegação por teclado entre os dois botões -- a espec. não descreve
    /// a tecla, mas um diálogo só-mouse não seria alcançável do teclado
    /// além do default seguro (`Enter` = cancelar). `Tab`, `Left` e `Right`
    /// alternam; são só dois estados, então "alternar" é "trocar".
    pub fn toggle_focus(&mut self) {
        self.focused = match self.focused {
            DialogButton::Cancel => DialogButton::Confirm,
            DialogButton::Confirm => DialogButton::Cancel,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_focused_on_cancel() {
        let dialog = ConfirmDialog::new("t", "b", "Fechar", DialogAction::CloseWindow);
        assert_eq!(dialog.focused(), DialogButton::Cancel);
    }

    #[test]
    fn toggle_focus_swaps_between_the_two_buttons() {
        let mut dialog = ConfirmDialog::new("t", "b", "Fechar", DialogAction::CloseWindow);
        dialog.toggle_focus();
        assert_eq!(dialog.focused(), DialogButton::Confirm);
        dialog.toggle_focus();
        assert_eq!(dialog.focused(), DialogButton::Cancel);
    }
}
