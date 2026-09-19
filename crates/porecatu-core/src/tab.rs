// SPDX-License-Identifier: GPL-3.0-or-later

//! `Tab`, seu ciclo de vida (ADR-0017) e a precedência de título (RF-1.7,
//! reconciliada pelo ADR-0017 -- sem o nível de processo em primeiro
//! plano).
//!
//! ADR-0053 §2: uma aba passa a conter uma árvore de painéis
//! ([`PaneTree`]), não mais um terminal só. Os seis campos que descreviam
//! um shell (`process_title`, `cwd`, `shell_name`, o estado de vida,
//! `activity`, `bell`) migraram para [`Pane`] -- `Tab` fica com identidade,
//! `custom_title` e a árvore, e **deriva** o que a barra de abas precisa:
//! título do painel focado (`custom_title` continua vencendo tudo,
//! RF-6.17), e atividade/campainha por **agregação** de todos os painéis
//! (RF-6.18) -- a escolha oposta à do título, e de propósito.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::id::TabId;
use crate::pane::PaneTree;

/// Uma aba. Não carrega PTY nem motor VT -- isso é `porecatu-term`, do
/// outro lado da fronteira da seção 4 da arquitetura. `Tab` só guarda o
/// que o domínio precisa para desenhar a barra e decidir foco.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    id: TabId,
    /// Título definido pelo usuário (RF-1.8). `Some` congela o título:
    /// atualizações do painel focado continuam sendo aplicadas por baixo,
    /// mas [`Tab::title`] as ignora enquanto isto for `Some`. RF-1.9 limpa
    /// voltando a `None`.
    custom_title: Option<String>,
    panes: PaneTree,
}

impl Tab {
    pub fn new(id: TabId, shell_name: impl Into<String>) -> Self {
        Self {
            id,
            custom_title: None,
            panes: PaneTree::new(shell_name),
        }
    }

    /// ADR-0037 §1: única forma de nascer com o painel em `NotStarted` --
    /// reservada à restauração de sessão (F5 etapa 4). Todo outro caminho
    /// (`tab.new`, `group.new_tab`, `window.new`) usa [`Tab::new`], que
    /// nasce `Running`.
    pub fn new_not_started(id: TabId, shell_name: impl Into<String>) -> Self {
        Self {
            id,
            custom_title: None,
            panes: PaneTree::new_not_started(shell_name),
        }
    }

    pub const fn id(&self) -> TabId {
        self.id
    }

    /// A árvore de painéis desta aba (ADR-0053 §1) -- layout (etapa 3),
    /// input (etapa 4) e sessão (etapa 5) leem daqui.
    pub const fn panes(&self) -> &PaneTree {
        &self.panes
    }

    pub const fn panes_mut(&mut self) -> &mut PaneTree {
        &mut self.panes
    }

    /// Título exibido (RF-6.17): customizado -> o do painel **focado**
    /// (que por si já segue OSC 0/2 -> nome do shell, RF-1.7 um nível
    /// abaixo).
    pub fn title(&self) -> &str {
        self.custom_title
            .as_deref()
            .unwrap_or_else(|| self.panes.focused().title())
    }

    pub fn has_custom_title(&self) -> bool {
        self.custom_title.is_some()
    }

    /// RF-1.8 (renomear) e RF-1.9 (`None` limpa e devolve ao automático).
    pub fn set_custom_title(&mut self, title: Option<String>) {
        self.custom_title = title;
    }

    /// Aplica um título vindo de OSC 0 / OSC 2 -- do painel **focado**
    /// (RF-6.17). Sempre atualizado, mesmo com título customizado ativo --
    /// é [`Tab::title`] quem ignora o valor enquanto o congelamento
    /// estiver em vigor, não este método.
    pub fn set_process_title(&mut self, title: Option<String>) {
        self.panes.focused_mut().set_process_title(title);
    }

    /// `cwd` do painel **focado** (RF-6.17/ADR-0053 §2).
    pub fn cwd(&self) -> Option<&PathBuf> {
        self.panes.focused().cwd()
    }

    /// Captura de OSC 7 (ADR-0017 item 1) -- do painel focado.
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.panes.focused_mut().set_cwd(cwd);
    }

    /// Nome do shell do painel focado (ADR-0036 §3: `porecatu-session`
    /// grava isto como `TabV1::spawn_program`, para diferenciar do shell
    /// padrão da config na restauração).
    pub fn shell_name(&self) -> &str {
        self.panes.focused().shell_name()
    }

    pub fn is_exited(&self) -> bool {
        self.panes.focused().is_exited()
    }

    /// ADR-0037 §1: só a restauração de sessão produz este estado.
    pub fn is_not_started(&self) -> bool {
        self.panes.focused().is_not_started()
    }

    /// `NotStarted` não tem PTY ainda, `Exited` não tem mais (ADR-0017
    /// item 6, ADR-0037 §1) -- do painel focado.
    pub fn accepts_input(&self) -> bool {
        self.panes.focused().accepts_input()
    }

    /// Primeiro foco de uma aba restaurada sem shell (ADR-0037 §2): quem
    /// chama já spawnou o `Terminal` de verdade antes de chamar isto --
    /// este método só formaliza a transição no modelo, no painel focado.
    pub fn start(&mut self) {
        self.panes.focused_mut().start();
    }

    /// RF-1.3/RF-6.11: processo do painel focado encerrou com código
    /// diferente de zero, a aba permanece aberta. Encerramento com código
    /// zero remove o painel (a aba, se ele era o último) -- isso é
    /// `Workspace::close_tab`/`PaneTree::close`, chamado pelo `ui`, não uma
    /// transição de estado deste tipo.
    pub fn mark_exited(&mut self, exit_code: i32) {
        self.panes.focused_mut().mark_exited(exit_code);
    }

    /// RF-6.18: agregação -- qualquer painel com atividade acende o
    /// indicador da aba.
    pub fn activity(&self) -> bool {
        self.panes.panes().iter().any(crate::pane::Pane::activity)
    }

    /// RF-1.20: saída nova enquanto a aba está em segundo plano -- no
    /// painel focado, que é o único que existe até a etapa 3/4 ligar o
    /// resto da árvore ao runtime.
    pub fn mark_activity(&mut self) {
        self.panes.focused_mut().mark_activity();
    }

    /// RF-6.18: agregação, mesmo motivo de [`Tab::activity`].
    pub fn bell(&self) -> bool {
        self.panes.panes().iter().any(crate::pane::Pane::bell)
    }

    /// RF-1.21: campainha (BEL) emitida em segundo plano -- no painel
    /// focado.
    pub fn mark_bell(&mut self) {
        self.panes.focused_mut().mark_bell();
    }

    /// RF-1.22/RF-6.18: visitar a aba limpa os indicadores agregados --
    /// que só apagam de verdade quando **nenhum** painel tem indicador
    /// aceso, então limpa todos, não só o focado. Chamado por
    /// `Workspace::activate_tab`, não diretamente -- "visitar" é um
    /// conceito de workspace (qual aba está ativa), não de aba isolada.
    pub(crate) fn clear_indicators(&mut self) {
        for pane in self.panes.panes_mut() {
            pane.clear_indicators();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::PaneState;

    #[test]
    fn title_falls_back_to_shell_name() {
        let tab = Tab::new(TabId::new(0), "zsh");
        assert_eq!(tab.title(), "zsh");
    }

    #[test]
    fn process_title_overrides_shell_name() {
        let mut tab = Tab::new(TabId::new(0), "zsh");
        tab.set_process_title(Some("vim: main.rs".to_string()));
        assert_eq!(tab.title(), "vim: main.rs");
    }

    #[test]
    fn custom_title_freezes_over_process_title() {
        let mut tab = Tab::new(TabId::new(0), "zsh");
        tab.set_custom_title(Some("backend".to_string()));
        tab.set_process_title(Some("vim: main.rs".to_string()));
        assert_eq!(tab.title(), "backend");
    }

    #[test]
    fn clearing_custom_title_reveals_process_title() {
        let mut tab = Tab::new(TabId::new(0), "zsh");
        tab.set_custom_title(Some("backend".to_string()));
        tab.set_process_title(Some("vim: main.rs".to_string()));
        tab.set_custom_title(None);
        assert_eq!(tab.title(), "vim: main.rs");
    }

    /// RF-6.17, a metade que a F3 não tinha como testar: trocar o painel
    /// focado muda o título derivado, mas `custom_title` continua
    /// vencendo mesmo depois da troca.
    #[test]
    fn switching_focused_pane_changes_the_derived_title_but_not_a_custom_one() {
        let mut tab = Tab::new(TabId::new(0), "zsh");
        let first = tab.panes().focused_id();
        let second = tab
            .panes_mut()
            .split(first, crate::pane::SplitAxis::Vertical, "bash", None)
            .unwrap();
        assert_eq!(tab.title(), "bash", "painel novo nasce focado (RF-6.1)");

        tab.panes_mut().focus(first);
        assert_eq!(tab.title(), "zsh");

        tab.set_custom_title(Some("meu terminal".to_string()));
        tab.panes_mut().focus(second);
        assert_eq!(
            tab.title(),
            "meu terminal",
            "custom_title vence a troca de painel focado"
        );
    }

    #[test]
    fn exited_tab_rejects_input() {
        let mut tab = Tab::new(TabId::new(0), "zsh");
        assert!(tab.accepts_input());
        tab.mark_exited(1);
        assert!(!tab.accepts_input());
    }

    #[test]
    fn exiting_clears_indicators() {
        let mut tab = Tab::new(TabId::new(0), "zsh");
        tab.mark_activity();
        tab.mark_bell();
        tab.mark_exited(0);
        assert!(!tab.activity());
        assert!(!tab.bell());
    }

    /// RF-6.18: painel em segundo plano (não o focado) acende o indicador
    /// agregado da aba -- não só o do painel focado.
    #[test]
    fn activity_on_any_pane_lights_up_the_aggregated_indicator() {
        let mut tab = Tab::new(TabId::new(0), "zsh");
        let first = tab.panes().focused_id();
        let second = tab
            .panes_mut()
            .split(first, crate::pane::SplitAxis::Horizontal, "bash", None)
            .unwrap();
        assert_eq!(tab.panes().focused_id(), second);

        tab.panes_mut().focus(first);
        assert!(!tab.activity() && !tab.bell());

        tab.panes_mut().pane_mut(second).unwrap().mark_activity();
        assert!(tab.activity(), "atividade em painel não focado agrega");

        tab.panes_mut().pane_mut(second).unwrap().mark_bell();
        assert!(tab.bell());
    }

    /// ADR-0037 §1: aba nova nasce sempre `Running` -- só a restauração
    /// (F5 etapa 4) produz `NotStarted`, e essa etapa não existe ainda.
    #[test]
    fn new_tab_is_always_running() {
        let tab = Tab::new(TabId::new(0), "zsh");
        assert_eq!(tab.panes().focused().state(), PaneState::Running);
    }

    fn not_started_tab() -> Tab {
        Tab::new_not_started(TabId::new(0), "zsh")
    }

    /// F5 etapa 4: agora existe um construtor público -- a restauração de
    /// sessão é a única chamadora de verdade, mas o teste garante que ele
    /// produz o mesmo shape que o literal de módulo que o testava até aqui.
    #[test]
    fn new_not_started_produces_the_not_started_state() {
        let tab = Tab::new_not_started(TabId::new(0), "zsh");
        assert_eq!(tab.panes().focused().state(), PaneState::NotStarted);
        assert_eq!(tab.title(), "zsh");
    }

    #[test]
    fn not_started_transitions_to_running_on_start() {
        let mut tab = not_started_tab();
        tab.start();
        assert_eq!(tab.panes().focused().state(), PaneState::Running);
    }

    #[test]
    fn start_on_already_running_is_a_no_op() {
        let mut tab = Tab::new(TabId::new(0), "zsh");
        tab.start();
        assert_eq!(tab.panes().focused().state(), PaneState::Running);
    }

    /// ADR-0037 §1: "sem volta" -- uma aba `Exited` não regride nunca,
    /// nem por `start()`.
    #[test]
    fn start_never_revives_an_exited_tab() {
        let mut tab = Tab::new(TabId::new(0), "zsh");
        tab.mark_exited(1);
        tab.start();
        assert!(tab.is_exited());
    }

    #[test]
    fn not_started_tab_rejects_input() {
        let tab = not_started_tab();
        assert!(!tab.accepts_input());
    }

    /// ADR-0037 §4: sem PTY, atividade e campainha não têm como acender --
    /// `mark_activity`/`mark_bell` só são chamados a partir de um
    /// `TermEvent`, que uma aba sem `Terminal` nunca recebe (mesma razão
    /// que já valia para `Exited`). Documenta o dado que sustenta a
    /// exclusão do indicador agregado (RF-2.16) em `porecatu-ui`, sem
    /// precisar construir um `Workspace` inteiro pra provar.
    #[test]
    fn not_started_tab_has_no_indicators_by_default() {
        let tab = not_started_tab();
        assert!(!tab.activity());
        assert!(!tab.bell());
    }
}
