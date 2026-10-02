// SPDX-License-Identifier: GPL-3.0-or-later

//! Diálogo de confirmação (ADR-0014, PRD-010 RF-10.18): modal por janela --
//! `App` carrega no máximo um por `WindowState`, o que já implementa "modal
//! é por janela, não por app" (ADR-0014, mitigação de risco). Foco inicial
//! no cancelar; `Enter` aciona o botão focado, `Esc` sempre cancela.
//! `action` é um enum fechado, não uma closure: o diálogo é dado puro, sem
//! capturar estado de `App` dentro dele.

use std::path::PathBuf;

use porecatu_core::{GroupId, PaneId, TabId};
use porecatu_locale::Catalog;

use crate::messages::msg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogButton {
    Cancel,
    /// O botão do meio do diálogo de três saídas (RF-16.4); os de dois
    /// botões nunca o têm.
    Discard,
    Confirm,
}

/// O que confirmar faz de verdade -- resolvido por `lib.rs`, que já tem
/// acesso ao `WindowState`/`App` que o diálogo não carrega. Os quatro
/// primeiros diálogos do v1 (RF-10.19) são todos de fechamento -- o
/// prefixo comum não é redundância, é coincidência de escopo. As duas
/// últimas variantes (RF-14.5/RF-14.16, ADR-0055 §3) carregam o dado que
/// falta pra confirmar de verdade -- nome pro sobrescrever, caminho pro
/// excluir -- e é isso que tira `Copy` do enum (`PathBuf`/`String` não
/// são `Copy`); todo call site que fazia `dialog.action` (cópia) passou a
/// `dialog.action.clone()`.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogAction {
    /// RF-1.6 (ADR-0017): fechar aba com tela alternativa ou reporte de
    /// mouse ligado.
    CloseTab(TabId),
    /// RF-6.10/RF-1.6: fechar um painel (não o último da aba) com tela
    /// alternativa ou reporte de mouse ligado.
    ClosePane(TabId, PaneId),
    /// RF-10.23 (ADR-0015): fechar janela com mais de uma aba.
    CloseWindow,
    /// RF-2.22/RF-2.23 (`group.close_all`): fecha todas as abas do grupo.
    /// Confirmação **sempre**, não configurável -- "a ação mais destrutiva
    /// da interface" (ADR-0023).
    CloseGroup(GroupId),
    /// RF-14.5 (ADR-0054 §5, ADR-0055 §3): nome que já existe na lista
    /// confirmado -- grava por cima do arquivo que já tem esse nome.
    OverwriteNamedSession(String),
    /// RF-14.16: exclui de verdade o arquivo da sessão nomeada.
    DeleteNamedSession(PathBuf),
    /// RF-16.4: fechar a janela de configurações (ou encerrar o app) com
    /// alterações pendentes. Quem responde é a própria janela de
    /// configurações, que guarda o rascunho; `App` só vê a resposta.
    SettingsPending,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmDialog {
    pub title: String,
    pub body: String,
    pub confirm_label: String,
    /// Copiado na abertura junto com os outros três textos: um diálogo
    /// aberto não muda de idioma no meio (ADR-0056 §9).
    pub cancel_label: String,
    /// O botão do meio, só no diálogo de três saídas (RF-16.4); `None` nos
    /// outros, que continuam com dois botões.
    pub discard_label: Option<String>,
    pub action: DialogAction,
    focused: DialogButton,
    /// O botão sob o cursor, só no diálogo de três saídas: os de dois botões
    /// não têm realce de hover.
    hovered: Option<DialogButton>,
}

impl ConfirmDialog {
    pub fn new(
        title: impl Into<String>,
        body: impl Into<String>,
        confirm_label: impl Into<String>,
        cancel_label: impl Into<String>,
        action: DialogAction,
    ) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            confirm_label: confirm_label.into(),
            cancel_label: cancel_label.into(),
            discard_label: None,
            action,
            focused: DialogButton::Cancel,
            hovered: None,
        }
    }

    pub const fn focused(&self) -> DialogButton {
        self.focused
    }

    /// RF-1.6 (ADR-0017): fechar aba com processo ativo. `title` é o título
    /// da aba, que vem de um programa e entra na frase como chegou.
    pub fn close_tab(catalog: &Catalog, title: &str, tab: TabId) -> Self {
        Self::new(
            msg::dialog::close_tab::title(catalog),
            msg::dialog::close_tab::body(catalog, title),
            msg::dialog::close_tab::confirm(catalog),
            msg::dialog::cancel(catalog),
            DialogAction::CloseTab(tab),
        )
    }

    /// RF-6.10: fechar um painel (não o último da aba) com processo ativo.
    pub fn close_pane(catalog: &Catalog, title: &str, tab: TabId, pane: PaneId) -> Self {
        Self::new(
            msg::dialog::close_pane::title(catalog),
            msg::dialog::close_pane::body(catalog, title),
            msg::dialog::close_pane::confirm(catalog),
            msg::dialog::cancel(catalog),
            DialogAction::ClosePane(tab, pane),
        )
    }

    /// RF-10.23 (ADR-0015): fechar janela. O corpo escolhe pela contagem de
    /// **abas** de verdade, não de painéis: uma aba dividida em três painéis
    /// não é "mais de uma aba".
    pub fn close_window(catalog: &Catalog, real_tab_count: usize) -> Self {
        let body = if real_tab_count > 1 {
            msg::dialog::close_window::body_tabs(catalog)
        } else {
            msg::dialog::close_window::body_program(catalog)
        };
        Self::new(
            msg::dialog::close_window::title(catalog),
            body,
            msg::dialog::close_window::confirm(catalog),
            msg::dialog::cancel(catalog),
            DialogAction::CloseWindow,
        )
    }

    /// RF-2.23 (`group.close_all`): a forma de plural vem do idioma.
    pub fn close_group(catalog: &Catalog, tab_count: usize, group: GroupId) -> Self {
        Self::new(
            msg::dialog::close_group::title(catalog),
            msg::dialog::close_group::body(catalog, tab_count),
            msg::dialog::close_group::confirm(catalog, tab_count),
            msg::dialog::cancel(catalog),
            DialogAction::CloseGroup(group),
        )
    }

    /// RF-14.16: excluir uma sessão salva. `name` é o que o usuário deu à
    /// sessão.
    pub fn delete_session(catalog: &Catalog, name: &str, file: PathBuf) -> Self {
        Self::new(
            msg::dialog::delete_session::title(catalog),
            msg::dialog::delete_session::body(catalog, name),
            msg::dialog::delete_session::confirm(catalog),
            msg::dialog::cancel(catalog),
            DialogAction::DeleteNamedSession(file),
        )
    }

    /// RF-14.5: o nome digitado já existe na lista. `existing` é o nome da
    /// entrada que seria substituída, `typed` o que foi digitado (o que
    /// `OverwriteNamedSession` grava).
    pub fn overwrite_session(catalog: &Catalog, existing: &str, typed: String) -> Self {
        Self::new(
            msg::dialog::overwrite_session::title(catalog, existing),
            msg::dialog::overwrite_session::body(catalog),
            msg::dialog::overwrite_session::confirm(catalog),
            msg::dialog::cancel(catalog),
            DialogAction::OverwriteNamedSession(typed),
        )
    }

    /// RF-16.4 (ADR-0060 §4): fechar a janela de configurações com
    /// `changes` alterações pendentes. Três saídas, foco inicial no cancelar.
    pub fn settings_pending(catalog: &Catalog, changes: usize) -> Self {
        let mut dialog = Self::new(
            msg::settings::dialog::close_title(catalog),
            msg::settings::dialog::close_body(catalog, changes),
            msg::settings::dialog::save_and_close(catalog),
            msg::settings::dialog::cancel(catalog),
            DialogAction::SettingsPending,
        );
        dialog.discard_label = Some(msg::settings::dialog::discard_and_close(catalog));
        dialog
    }

    /// O botão sob o cursor, realçado só no diálogo de três saídas.
    pub const fn hovered(&self) -> Option<DialogButton> {
        self.hovered
    }

    pub fn set_hovered(&mut self, button: Option<DialogButton>) {
        self.hovered = button;
    }

    /// Os botões do diálogo, da esquerda para a direita: a ordem de leitura e
    /// de `Tab`.
    pub fn buttons(&self) -> Vec<DialogButton> {
        let mut buttons = vec![DialogButton::Cancel];
        if self.discard_label.is_some() {
            buttons.push(DialogButton::Discard);
        }
        buttons.push(DialogButton::Confirm);
        buttons
    }

    /// Navegação por teclado entre os botões -- a espec. não descreve a
    /// tecla, mas um diálogo só-mouse não seria alcançável do teclado além
    /// do default seguro (`Enter` = cancelar). `Tab`, `Left` e `Right`
    /// andam em círculo: com dois botões é "trocar".
    pub fn toggle_focus(&mut self) {
        self.step_focus(1);
    }

    /// Anda `delta` botões, em círculo (`Shift+Tab` e `Left` usam -1).
    pub fn step_focus(&mut self, delta: i32) {
        let buttons = self.buttons();
        let at = buttons
            .iter()
            .position(|button| *button == self.focused)
            .unwrap_or(0) as i32;
        let next = (at + delta).rem_euclid(buttons.len() as i32) as usize;
        self.focused = buttons[next];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::test_support;

    /// Regressão achada na verificação ao vivo da etapa 5 dos painéis: uma
    /// janela com uma aba só, dividida em painéis (`real_tab_count == 1`),
    /// não pode dizer "mais de uma aba aberta" -- `request_close_window`
    /// decidia essa frase pela contagem de **painéis**, não de abas.
    #[test]
    fn close_window_body_names_the_program_for_a_single_tab_with_many_panes() {
        let pt = test_support::pt_br();
        assert_eq!(
            ConfirmDialog::close_window(&pt, 1).body,
            "Esta janela tem um programa em primeiro plano."
        );
        let en = test_support::en_us();
        assert_eq!(
            ConfirmDialog::close_window(&en, 1).body,
            "This window has a program running in the foreground."
        );
    }

    #[test]
    fn close_window_body_names_more_than_one_tab_when_there_really_is() {
        let pt = test_support::pt_br();
        assert_eq!(
            ConfirmDialog::close_window(&pt, 2).body,
            "Esta janela tem mais de uma aba aberta."
        );
        let en = test_support::en_us();
        assert_eq!(
            ConfirmDialog::close_window(&en, 2).body,
            "This window has more than one tab open."
        );
    }

    #[test]
    fn dialogs_copy_all_four_texts_when_they_open() {
        let pt = test_support::pt_br();
        let dialog = ConfirmDialog::close_tab(&pt, "vim", TabId::new(1));
        assert_eq!(dialog.title, "Fechar aba?");
        assert_eq!(
            dialog.body,
            "\"vim\" tem um programa em primeiro plano. Fechar mesmo assim?"
        );
        assert_eq!(dialog.confirm_label, "Fechar aba");
        assert_eq!(dialog.cancel_label, "Cancelar");
        assert_eq!(dialog.action, DialogAction::CloseTab(TabId::new(1)));
    }

    #[test]
    fn close_group_dialog_pluralizes_body_and_button_in_both_languages() {
        let pt = test_support::pt_br();
        let one = ConfirmDialog::close_group(&pt, 1, GroupId::new(1));
        assert_eq!(one.body, "Isso fecha 1 aba.");
        assert_eq!(one.confirm_label, "Fechar grupo (1 aba)");
        let many = ConfirmDialog::close_group(&pt, 3, GroupId::new(1));
        assert_eq!(many.body, "Isso fecha 3 abas.");
        assert_eq!(many.confirm_label, "Fechar grupo (3 abas)");

        let en = test_support::en_us();
        let one = ConfirmDialog::close_group(&en, 1, GroupId::new(1));
        assert_eq!(one.body, "This closes 1 tab.");
        assert_eq!(one.confirm_label, "Close group (1 tab)");
        let many = ConfirmDialog::close_group(&en, 3, GroupId::new(1));
        assert_eq!(many.body, "This closes 3 tabs.");
        assert_eq!(many.confirm_label, "Close group (3 tabs)");
    }

    #[test]
    fn session_dialogs_carry_the_user_typed_name_as_it_is() {
        let pt = test_support::pt_br();
        let overwrite = ConfirmDialog::overwrite_session(&pt, "api", "api ".to_owned());
        assert_eq!(overwrite.title, "Sobrescrever a sessão «api»?");
        assert_eq!(
            overwrite.action,
            DialogAction::OverwriteNamedSession("api ".to_owned())
        );
        let delete = ConfirmDialog::delete_session(&pt, "api", PathBuf::from("api.json"));
        assert_eq!(delete.body, "Isso remove «api» permanentemente.");
        assert_eq!(delete.confirm_label, "Excluir");
    }

    #[test]
    fn starts_focused_on_cancel() {
        let dialog = ConfirmDialog::new("t", "b", "Fechar", "Cancelar", DialogAction::CloseWindow);
        assert_eq!(dialog.focused(), DialogButton::Cancel);
    }

    #[test]
    fn toggle_focus_swaps_between_the_two_buttons() {
        let mut dialog =
            ConfirmDialog::new("t", "b", "Fechar", "Cancelar", DialogAction::CloseWindow);
        dialog.toggle_focus();
        assert_eq!(dialog.focused(), DialogButton::Confirm);
        dialog.toggle_focus();
        assert_eq!(dialog.focused(), DialogButton::Cancel);
    }

    #[test]
    fn the_settings_dialog_has_three_buttons_and_starts_on_cancel() {
        let dialog = ConfirmDialog::settings_pending(&test_support::pt_br(), 3);
        assert_eq!(dialog.title, "Fechar com alterações pendentes?");
        assert_eq!(dialog.body, "Há 3 alterações não salvas.");
        assert_eq!(dialog.confirm_label, "Salvar e fechar");
        assert_eq!(dialog.discard_label.as_deref(), Some("Descartar e fechar"));
        assert_eq!(dialog.cancel_label, "Cancelar");
        assert_eq!(dialog.focused(), DialogButton::Cancel);
        assert_eq!(
            dialog.buttons(),
            [
                DialogButton::Cancel,
                DialogButton::Discard,
                DialogButton::Confirm
            ]
        );
    }

    #[test]
    fn focus_goes_around_all_three_buttons_both_ways() {
        let mut dialog = ConfirmDialog::settings_pending(&test_support::pt_br(), 1);
        dialog.step_focus(1);
        assert_eq!(dialog.focused(), DialogButton::Discard);
        dialog.step_focus(1);
        assert_eq!(dialog.focused(), DialogButton::Confirm);
        dialog.step_focus(1);
        assert_eq!(dialog.focused(), DialogButton::Cancel);
        dialog.step_focus(-1);
        assert_eq!(dialog.focused(), DialogButton::Confirm);
    }

    #[test]
    fn a_two_button_dialog_never_reaches_the_discard_button() {
        let mut dialog =
            ConfirmDialog::new("t", "b", "Fechar", "Cancelar", DialogAction::CloseWindow);
        assert_eq!(
            dialog.buttons(),
            [DialogButton::Cancel, DialogButton::Confirm]
        );
        dialog.step_focus(-1);
        assert_eq!(dialog.focused(), DialogButton::Confirm);
        dialog.step_focus(1);
        assert_eq!(dialog.focused(), DialogButton::Cancel);
    }
}
