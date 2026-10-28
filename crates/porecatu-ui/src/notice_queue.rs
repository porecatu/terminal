// SPDX-License-Identifier: GPL-3.0-or-later

//! Avisos do app que esperam o usuário chegar ao painel. Estado puro, como
//! `prompt_queue`: a decisão de abrir é função do que a janela mostra.
//!
//! O que antes era uma nota escrita no grid do terminal (ADR-0014, canal 2)
//! vira um diálogo modal. Um aviso nasce de qualquer painel, inclusive de aba
//! em segundo plano, e **não rouba o foco**: abre quando a janela está em foco,
//! a aba é a ativa, o painel é o focado e nenhum outro diálogo está aberto; até
//! lá espera. Um aviso com [`NoticeSlot`] (o da reconexão SSH) é **substituído**
//! por um igual que chegue antes de abrir, em vez de empilhar.

use porecatu_core::{PaneId, TabId};

use crate::dialog::ConfirmDialog;
use crate::prompt_queue::FocusView;

/// Um aviso que espera o painel dele.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pending {
    pub(crate) tab: TabId,
    pub(crate) pane: PaneId,
    pub(crate) dialog: ConfirmDialog,
}

/// Os avisos pendentes de uma janela, na ordem em que chegaram.
#[derive(Debug, Default)]
pub(crate) struct PendingNotices {
    queue: Vec<Pending>,
}

impl PendingNotices {
    /// Enfileira `dialog`. Se ele tem `slot` e já há um pendente igual, o
    /// substitui no lugar (mantém a ordem de chegada).
    pub(crate) fn push(&mut self, tab: TabId, pane: PaneId, dialog: ConfirmDialog) {
        if let Some(slot) = dialog.slot
            && let Some(existing) = self
                .queue
                .iter_mut()
                .find(|pending| pending.dialog.slot == Some(slot))
        {
            *existing = Pending { tab, pane, dialog };
            return;
        }
        self.queue.push(Pending { tab, pane, dialog });
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Esquece o aviso pendente que ocupa `slot` (a reconexão deu certo e o
    /// aviso de espera perdeu o sentido).
    pub(crate) fn forget(&mut self, slot: crate::dialog::NoticeSlot) {
        self.queue
            .retain(|pending| pending.dialog.slot != Some(slot));
    }

    /// Tira o primeiro aviso cujo painel a janela mostra agora. O que ficou
    /// sem painel (`alive` falso) é esquecido no caminho.
    pub(crate) fn take_ready(
        &mut self,
        view: &FocusView,
        alive: impl Fn(TabId, PaneId) -> bool,
    ) -> Option<Pending> {
        self.queue
            .retain(|pending| alive(pending.tab, pending.pane));
        let index = self
            .queue
            .iter()
            .position(|pending| view.shows(pending.tab, pending.pane))?;
        Some(self.queue.remove(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialog::NoticeSlot;
    use crate::messages::test_support;

    fn tab(n: u32) -> TabId {
        TabId::new(n)
    }

    fn pane(n: u32) -> PaneId {
        PaneId::new(n)
    }

    fn view(active: u32, focused: u32) -> FocusView {
        FocusView {
            window_focused: true,
            active_tab: Some(tab(active)),
            focused_pane: Some(pane(focused)),
            dialog_open: false,
        }
    }

    fn info(text: &str) -> ConfirmDialog {
        ConfirmDialog::notice(&test_support::pt_br(), "t", text)
    }

    fn reconnect(state: &str) -> ConfirmDialog {
        ConfirmDialog::reconnect(&test_support::pt_br(), tab(1), pane(1), state)
    }

    #[test]
    fn a_notice_for_the_shown_pane_is_ready() {
        let mut queue = PendingNotices::default();
        queue.push(tab(1), pane(1), info("a"));

        let ready = queue.take_ready(&view(1, 1), |_, _| true);

        assert_eq!(ready.map(|p| p.dialog.body), Some("a".to_owned()));
        assert!(queue.is_empty());
    }

    #[test]
    fn a_notice_waits_for_its_tab_and_its_pane() {
        let mut queue = PendingNotices::default();
        queue.push(tab(2), pane(2), info("a"));

        assert!(queue.take_ready(&view(1, 1), |_, _| true).is_none());
        assert!(queue.take_ready(&view(2, 1), |_, _| true).is_none());
        assert!(queue.take_ready(&view(2, 2), |_, _| true).is_some());
    }

    #[test]
    fn nothing_opens_without_window_focus_or_over_another_dialog() {
        let mut queue = PendingNotices::default();
        queue.push(tab(1), pane(1), info("a"));
        let unfocused = FocusView {
            window_focused: false,
            ..view(1, 1)
        };
        let busy = FocusView {
            dialog_open: true,
            ..view(1, 1)
        };

        assert!(queue.take_ready(&unfocused, |_, _| true).is_none());
        assert!(queue.take_ready(&busy, |_, _| true).is_none());
        assert!(queue.take_ready(&view(1, 1), |_, _| true).is_some());
    }

    #[test]
    fn notices_of_one_pane_open_in_arrival_order() {
        let mut queue = PendingNotices::default();
        queue.push(tab(1), pane(1), info("first"));
        queue.push(tab(1), pane(1), info("second"));

        let first = queue.take_ready(&view(1, 1), |_, _| true).unwrap();
        let second = queue.take_ready(&view(1, 1), |_, _| true).unwrap();

        assert_eq!(
            (first.dialog.body.as_str(), second.dialog.body.as_str()),
            ("first", "second")
        );
    }

    #[test]
    fn a_live_notice_is_replaced_in_place_not_stacked() {
        let mut queue = PendingNotices::default();
        queue.push(tab(1), pane(1), info("before"));
        queue.push(tab(1), pane(1), reconnect("waiting 1"));
        queue.push(tab(1), pane(1), reconnect("waiting 2"));

        let first = queue.take_ready(&view(1, 1), |_, _| true).unwrap();
        let second = queue.take_ready(&view(1, 1), |_, _| true).unwrap();

        assert_eq!(first.dialog.body, "before");
        assert_eq!(second.dialog.body, "waiting 2");
        assert!(queue.is_empty());
    }

    #[test]
    fn forgetting_a_slot_drops_only_its_pending_notice() {
        let mut queue = PendingNotices::default();
        queue.push(tab(1), pane(1), info("keep"));
        queue.push(tab(1), pane(1), reconnect("waiting"));

        queue.forget(NoticeSlot::Reconnect(tab(1), pane(1)));

        let left = queue.take_ready(&view(1, 1), |_, _| true).unwrap();
        assert_eq!(left.dialog.body, "keep");
        assert!(queue.is_empty());
    }

    #[test]
    fn a_notice_whose_pane_was_closed_is_forgotten() {
        let mut queue = PendingNotices::default();
        queue.push(tab(2), pane(2), info("a"));

        assert!(queue.take_ready(&view(2, 2), |_, _| false).is_none());
        assert!(queue.is_empty());
    }
}
