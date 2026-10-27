// SPDX-License-Identifier: GPL-3.0-or-later

//! Perguntas do backend que esperam o foco (ADR-0075 §4). Estado puro: a
//! decisão de abrir o diálogo é função do que a janela mostra, sem `winit` nem
//! GPU.
//!
//! Uma pergunta chega por `TermEvent::Prompt` a qualquer momento -- inclusive
//! com a aba em segundo plano --, e o diálogo **não rouba o foco**:
//!
//! - aba ativa, painel focado, janela em foco, nenhum outro diálogo aberto
//!   → abre;
//! - senão → fica **pendente no painel** (que acende a campainha), e abre
//!   quando o usuário chega ao painel;
//! - duas pendentes em painéis diferentes abrem uma de cada vez, conforme o
//!   usuário visita.
//!
//! Fechar o painel com pergunta pendente não precisa de nada aqui: o
//! `Terminal` cai, e a pergunta recebe `Cancel` (ADR-0071 §5). Esta fila só
//! esquece o que ficou sem painel.

use porecatu_core::{PaneId, TabId};
use porecatu_term::Prompt;

/// O que a janela mostra agora.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FocusView {
    pub(crate) window_focused: bool,
    pub(crate) active_tab: Option<TabId>,
    pub(crate) focused_pane: Option<PaneId>,
    /// Outro diálogo (modal) já está aberto.
    pub(crate) dialog_open: bool,
}

impl FocusView {
    /// O diálogo da pergunta de `pane` em `tab` pode abrir agora.
    pub(crate) fn shows(&self, tab: TabId, pane: PaneId) -> bool {
        self.window_focused
            && !self.dialog_open
            && self.active_tab == Some(tab)
            && self.focused_pane == Some(pane)
    }
}

/// Uma pergunta que espera o usuário chegar ao painel.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pending {
    pub(crate) tab: TabId,
    pub(crate) pane: PaneId,
    pub(crate) prompt: Prompt,
}

/// As perguntas pendentes de uma janela, na ordem em que chegaram.
#[derive(Debug, Default)]
pub(crate) struct PendingPrompts {
    queue: Vec<Pending>,
}

impl PendingPrompts {
    pub(crate) fn push(&mut self, tab: TabId, pane: PaneId, prompt: Prompt) {
        self.queue.push(Pending { tab, pane, prompt });
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Tira a primeira pergunta cujo painel a janela mostra agora. O que ficou
    /// sem painel (`alive` falso) é esquecido no caminho -- a pergunta dele já
    /// foi cancelada pela queda do `Terminal`.
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
    use porecatu_term::{PromptId, PromptKind};

    use super::*;

    fn prompt(n: u64) -> Prompt {
        Prompt {
            id: PromptId::new(n),
            kind: PromptKind::HostKey {
                algorithm: "ssh-ed25519".into(),
                fingerprint: "SHA256:abc".into(),
                known_types: Vec::new(),
                skipped_lines: 0,
            },
        }
    }

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

    #[test]
    fn a_prompt_for_the_shown_pane_is_ready() {
        let mut queue = PendingPrompts::default();
        queue.push(tab(1), pane(1), prompt(0));

        let ready = queue.take_ready(&view(1, 1), |_, _| true);

        assert_eq!(ready.map(|p| (p.tab, p.pane)), Some((tab(1), pane(1))));
        assert!(queue.is_empty());
    }

    #[test]
    fn a_prompt_in_a_background_tab_stays_pending_until_the_tab_is_shown() {
        let mut queue = PendingPrompts::default();
        queue.push(tab(2), pane(2), prompt(0));

        assert!(queue.take_ready(&view(1, 1), |_, _| true).is_none());
        assert!(!queue.is_empty());
        // The user visits the tab.
        assert!(queue.take_ready(&view(2, 2), |_, _| true).is_some());
    }

    #[test]
    fn a_prompt_for_an_unfocused_pane_of_the_active_tab_waits_for_its_focus() {
        let mut queue = PendingPrompts::default();
        queue.push(tab(1), pane(2), prompt(0));

        assert!(queue.take_ready(&view(1, 1), |_, _| true).is_none());
        assert!(queue.take_ready(&view(1, 2), |_, _| true).is_some());
    }

    #[test]
    fn nothing_opens_while_the_window_has_no_focus() {
        let mut queue = PendingPrompts::default();
        queue.push(tab(1), pane(1), prompt(0));
        let unfocused = FocusView {
            window_focused: false,
            ..view(1, 1)
        };

        assert!(queue.take_ready(&unfocused, |_, _| true).is_none());
        assert!(queue.take_ready(&view(1, 1), |_, _| true).is_some());
    }

    #[test]
    fn nothing_opens_over_another_dialog() {
        let mut queue = PendingPrompts::default();
        queue.push(tab(1), pane(1), prompt(0));
        let busy = FocusView {
            dialog_open: true,
            ..view(1, 1)
        };

        assert!(queue.take_ready(&busy, |_, _| true).is_none());
        assert!(queue.take_ready(&view(1, 1), |_, _| true).is_some());
    }

    #[test]
    fn two_pending_prompts_in_different_panes_open_one_at_a_time_as_visited() {
        let mut queue = PendingPrompts::default();
        queue.push(tab(1), pane(1), prompt(0));
        queue.push(tab(2), pane(2), prompt(1));

        let first = queue.take_ready(&view(2, 2), |_, _| true).unwrap();
        assert_eq!((first.tab, first.pane), (tab(2), pane(2)));
        // The other one is still waiting for its own pane.
        assert!(queue.take_ready(&view(2, 2), |_, _| true).is_none());
        let second = queue.take_ready(&view(1, 1), |_, _| true).unwrap();
        assert_eq!((second.tab, second.pane), (tab(1), pane(1)));
        assert!(queue.is_empty());
    }

    #[test]
    fn two_prompts_in_one_pane_open_in_arrival_order() {
        let mut queue = PendingPrompts::default();
        let first = prompt(0);
        let second = prompt(1);
        let (first_id, second_id) = (first.id, second.id);
        queue.push(tab(1), pane(1), first);
        queue.push(tab(1), pane(1), second);

        assert_eq!(
            queue
                .take_ready(&view(1, 1), |_, _| true)
                .unwrap()
                .prompt
                .id,
            first_id
        );
        assert_eq!(
            queue
                .take_ready(&view(1, 1), |_, _| true)
                .unwrap()
                .prompt
                .id,
            second_id
        );
    }

    #[test]
    fn a_prompt_whose_pane_was_closed_is_forgotten_and_never_opens() {
        let mut queue = PendingPrompts::default();
        queue.push(tab(2), pane(2), prompt(0));

        // The pane is gone (its terminal dropped and answered `Cancel`).
        assert!(queue.take_ready(&view(2, 2), |_, _| false).is_none());
        assert!(queue.is_empty());
    }
}
