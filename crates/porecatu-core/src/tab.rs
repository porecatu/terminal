// SPDX-License-Identifier: GPL-3.0-or-later

//! `Tab`, seu ciclo de vida (ADR-0017) e a precedência de título (RF-1.7,
//! reconciliada pelo ADR-0017 -- sem o nível de processo em primeiro
//! plano).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::id::TabId;

/// Estado de vida da aba (ADR-0017 item 6). Uma aba `Exited` não tem PTY,
/// não aceita input, mas continua rolável, selecionável e copiável -- é
/// para isso que o RF-1.3 a mantém aberta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabState {
    Running,
    Exited { exit_code: i32 },
}

/// Uma aba. Não carrega PTY nem motor VT -- isso é `porecatu-term`, do
/// outro lado da fronteira da seção 4 da arquitetura. `Tab` só guarda o
/// que o domínio precisa para desenhar a barra e decidir foco.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    id: TabId,
    /// Título definido pelo usuário (RF-1.8). `Some` congela o título:
    /// atualizações de `process_title` continuam sendo aplicadas por baixo,
    /// mas [`Tab::title`] as ignora enquanto isto for `Some`. RF-1.9 limpa
    /// voltando a `None`.
    custom_title: Option<String>,
    /// Último título recebido por OSC 0 / OSC 2.
    process_title: Option<String>,
    /// Nome do shell spawnado -- fallback de última instância, sempre
    /// presente.
    shell_name: String,
    /// Diretório de trabalho conhecido, capturado por OSC 7 (ADR-0017 item
    /// 1). `None` até o primeiro OSC 7 chegar; quem decide o fallback
    /// (`startup_directory`) é o chamador, não este tipo.
    cwd: Option<PathBuf>,
    state: TabState,
    /// Indicador de atividade (RF-1.20): saída nova enquanto em segundo
    /// plano.
    activity: bool,
    /// Indicador de campainha (RF-1.21), distinto do de atividade.
    bell: bool,
}

impl Tab {
    pub fn new(id: TabId, shell_name: impl Into<String>) -> Self {
        Self {
            id,
            custom_title: None,
            process_title: None,
            shell_name: shell_name.into(),
            cwd: None,
            state: TabState::Running,
            activity: false,
            bell: false,
        }
    }

    pub const fn id(&self) -> TabId {
        self.id
    }

    /// Título exibido, na precedência do RF-1.7 já sem o nível de processo
    /// em primeiro plano (ADR-0017): customizado -> OSC 0/2 -> nome do
    /// shell.
    pub fn title(&self) -> &str {
        self.custom_title
            .as_deref()
            .or(self.process_title.as_deref())
            .unwrap_or(&self.shell_name)
    }

    pub fn has_custom_title(&self) -> bool {
        self.custom_title.is_some()
    }

    /// RF-1.8 (renomear) e RF-1.9 (`None` limpa e devolve ao automático).
    pub fn set_custom_title(&mut self, title: Option<String>) {
        self.custom_title = title;
    }

    /// Aplica um título vindo de OSC 0 / OSC 2. Sempre atualizado, mesmo
    /// com título customizado ativo -- é [`Tab::title`] quem ignora o valor
    /// enquanto o congelamento estiver em vigor, não este método.
    pub fn set_process_title(&mut self, title: Option<String>) {
        self.process_title = title;
    }

    pub fn cwd(&self) -> Option<&PathBuf> {
        self.cwd.as_ref()
    }

    /// Nome do shell spawnado (ADR-0036 §3: `porecatu-session` grava isto
    /// como `TabV1::spawn_program`, para diferenciar do shell padrão da
    /// config na restauração).
    pub fn shell_name(&self) -> &str {
        &self.shell_name
    }

    /// Captura de OSC 7 (ADR-0017 item 1).
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = Some(cwd);
    }

    pub const fn state(&self) -> TabState {
        self.state
    }

    pub const fn is_exited(&self) -> bool {
        matches!(self.state, TabState::Exited { .. })
    }

    /// Aba `Exited` não aceita input (ADR-0017 item 6).
    pub const fn accepts_input(&self) -> bool {
        !self.is_exited()
    }

    /// RF-1.3: processo encerrou com código diferente de zero, a aba
    /// permanece aberta. Encerramento com código zero remove a aba
    /// inteiramente -- isso é `Workspace::close_tab`, chamado pelo `ui`, não
    /// uma transição de estado deste tipo.
    pub fn mark_exited(&mut self, exit_code: i32) {
        self.state = TabState::Exited { exit_code };
        // Aba morta não produz mais saída (ADR-0017 item 6): os
        // indicadores de atividade e campainha deixam de fazer sentido.
        self.activity = false;
        self.bell = false;
    }

    pub const fn activity(&self) -> bool {
        self.activity
    }

    /// RF-1.20: saída nova enquanto a aba está em segundo plano. Aba
    /// `Exited` nunca produz saída nova; chamar isto nela é erro do
    /// chamador, não algo que este método precise validar -- `ui` só chama
    /// a partir de um `TermEvent`, que uma aba morta não emite mais.
    pub fn mark_activity(&mut self) {
        self.activity = true;
    }

    pub const fn bell(&self) -> bool {
        self.bell
    }

    /// RF-1.21: campainha (BEL) emitida em segundo plano.
    pub fn mark_bell(&mut self) {
        self.bell = true;
    }

    /// RF-1.22: visitar a aba limpa os dois indicadores. Chamado por
    /// `Workspace::activate_tab`, não diretamente -- "visitar" é um
    /// conceito de workspace (qual aba está ativa), não de aba isolada.
    pub(crate) fn clear_indicators(&mut self) {
        self.activity = false;
        self.bell = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
