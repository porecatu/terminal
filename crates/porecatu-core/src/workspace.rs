// SPDX-License-Identifier: GPL-3.0-or-later

//! `Workspace` (ADR-0006): `Vec<Group>` de `Vec<TabId>`, contador de IDs
//! monotônico, aba ativa. Uma janela == um `Workspace` (ADR-0015).
//!
//! Só as operações que a F2 exercita estão aqui: `new_tab`, `close_tab`,
//! `move_tab` (reordenação dentro do próprio grupo -- RF-1.15/RF-1.17,
//! nunca entre grupos), `activate_tab` e navegação sequencial/por índice.
//! `group_tabs`, `ungroup`, `rename_group`, `set_group_color` e
//! `collapse_group` da tabela do ADR-0006 ficam de fora de propósito: são
//! F3, e nada nesta fase os chama -- só existe o grupo implícito criado por
//! `Workspace::new`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::group::Group;
use crate::id::{GroupId, TabId};
use crate::tab::Tab;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Sempre com exatamente um elemento na F2: o grupo implícito criado
    /// por `new`. Nenhuma operação desta fase adiciona um segundo grupo --
    /// isso é `group.create`, F3.
    groups: Vec<Group>,
    /// Dados das abas, sem relação com a ordem visual -- a ordem visual
    /// vive só em `Group::tabs`.
    tabs: Vec<Tab>,
    active_tab: Option<TabId>,
    next_tab_id: u32,
    next_group_id: u32,
}

impl Workspace {
    pub fn new() -> Self {
        let implicit_group_id = GroupId::new(0);
        Self {
            groups: vec![Group::new(implicit_group_id)],
            tabs: Vec::new(),
            active_tab: None,
            next_tab_id: 0,
            next_group_id: 1,
        }
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id() == id)
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id() == id)
    }

    pub const fn active_tab(&self) -> Option<TabId> {
        self.active_tab
    }

    /// Ordem visual: grupos na ordem do `Vec`, abas na ordem dentro de cada
    /// grupo (ADR-0006: "a ordem visual é a ordem do modelo"). Base de
    /// `next_tab`/`prev_tab` (RF-1.11) e `tab_at_visual_index` (RF-1.12).
    pub fn visual_order(&self) -> impl Iterator<Item = TabId> + '_ {
        self.groups.iter().flat_map(|g| g.tabs().iter().copied())
    }

    /// RF-1.1: cria aba no grupo implícito, na posição dada, com o `cwd`
    /// que o chamador já resolveu (aba ativa -> OSC 7 -> `startup_directory`,
    /// ADR-0017 -- essa cadeia de fallback não é deste tipo, que só guarda o
    /// valor final). A aba nova se torna a ativa.
    pub fn new_tab(
        &mut self,
        shell_name: impl Into<String>,
        cwd: Option<PathBuf>,
        pos: usize,
    ) -> TabId {
        let id = TabId::new(self.next_tab_id);
        self.next_tab_id += 1;

        let mut tab = Tab::new(id, shell_name);
        if let Some(cwd) = cwd {
            tab.set_cwd(cwd);
        }
        self.tabs.push(tab);
        self.groups[0].insert(pos, id);
        self.activate_tab(id);
        id
    }

    /// RF-1.2/RF-1.5: remove a aba. Se ela era a ativa, o foco vai para a
    /// vizinha seguinte no mesmo grupo; sem vizinha seguinte, para a
    /// anterior. O terceiro nível do RF-1.5 -- "a aba mais próxima do grupo
    /// adjacente" -- fica inerte até existir um segundo grupo alcançável
    /// (F3, `group.create`); com um grupo só, esgotadas as duas primeiras
    /// regras a aba ativa vira `None`.
    ///
    /// Devolve a aba ativa do workspace depois da remoção (`None` se ele
    /// ficou vazio). Não bloqueia em I/O nem em confirmação: quem decide se
    /// a aba pode fechar (RF-1.6, ADR-0017) e quem drena o PTY é o `ui`,
    /// antes de chamar isto.
    pub fn close_tab(&mut self, id: TabId) -> Option<TabId> {
        let tab_index = self.tabs.iter().position(|t| t.id() == id)?;
        let group_index = self
            .groups
            .iter()
            .position(|g| g.position_of(id).is_some())?;

        let removed_pos = self.groups[group_index]
            .remove(id)
            .expect("posição verificada acima");
        self.tabs.remove(tab_index);

        if self.active_tab == Some(id) {
            let group = &self.groups[group_index];
            let neighbor = group
                .tabs()
                .get(removed_pos)
                .or_else(|| removed_pos.checked_sub(1).and_then(|p| group.tabs().get(p)))
                .copied();
            self.active_tab = neighbor;
            if let Some(active) = self.active_tab {
                self.tab_mut(active)
                    .expect("id veio do próprio grupo")
                    .clear_indicators();
            }
        }

        self.active_tab
    }

    /// RF-1.15 (arraste) e RF-1.17 (teclado): reordena dentro do próprio
    /// grupo. Mover entre grupos é `tab.move_to_group`, F3 -- fora do
    /// escopo desta fase (RF-1.16).
    pub fn move_tab(&mut self, id: TabId, pos: usize) -> bool {
        let Some(group) = self.groups.iter_mut().find(|g| g.position_of(id).is_some()) else {
            return false;
        };
        group.move_within(id, pos)
    }

    /// RF-1.13 (clique ativa) e base de `next_tab`/`prev_tab`/goto-índice.
    /// RF-1.22: visitar a aba limpa seus indicadores de atividade e
    /// campainha.
    pub fn activate_tab(&mut self, id: TabId) -> bool {
        if self.tab(id).is_none() {
            return false;
        }
        self.active_tab = Some(id);
        self.tab_mut(id).expect("checado acima").clear_indicators();
        true
    }

    /// RF-1.11: próxima aba na ordem visual, circulando.
    pub fn next_tab(&mut self) -> Option<TabId> {
        self.step_tab(1)
    }

    /// RF-1.11: aba anterior na ordem visual, circulando.
    pub fn prev_tab(&mut self) -> Option<TabId> {
        self.step_tab(-1)
    }

    fn step_tab(&mut self, delta: isize) -> Option<TabId> {
        let order: Vec<TabId> = self.visual_order().collect();
        if order.is_empty() {
            return None;
        }
        let current = self.active_tab?;
        let idx = order.iter().position(|&t| t == current)?;
        let len = order.len() as isize;
        let next_idx = (idx as isize + delta).rem_euclid(len) as usize;
        let next = order[next_idx];
        self.activate_tab(next);
        Some(next)
    }

    /// RF-1.12: acesso direto por índice na ordem visual da janela inteira
    /// (0-based; `tab.goto_1` do catálogo de ações é índice 0).
    pub fn tab_at_visual_index(&self, index: usize) -> Option<TabId> {
        self.visual_order().nth(index)
    }

    /// `tab.goto_N`: resolve pelo índice visual e ativa.
    pub fn activate_visual_index(&mut self, index: usize) -> Option<TabId> {
        let id = self.tab_at_visual_index(index)?;
        self.activate_tab(id);
        Some(id)
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab_ids(ws: &Workspace) -> Vec<TabId> {
        ws.tabs.iter().map(Tab::id).collect()
    }

    #[test]
    fn new_tab_becomes_active_and_appends() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let b = ws.new_tab("zsh", None, 1);
        assert_eq!(ws.active_tab(), Some(b));
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, b]);
    }

    #[test]
    fn new_tab_inherits_given_cwd() {
        let mut ws = Workspace::new();
        let id = ws.new_tab("zsh", Some(PathBuf::from("/home/user/projeto")), 0);
        assert_eq!(
            ws.tab(id).unwrap().cwd(),
            Some(&PathBuf::from("/home/user/projeto"))
        );
    }

    // Invariante do ADR-0006: todo TabId está em exatamente um grupo.
    #[test]
    fn every_tab_is_in_exactly_one_group() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let b = ws.new_tab("zsh", None, 1);
        let c = ws.new_tab("zsh", None, 2);

        let mut seen = Vec::new();
        for group in ws.groups() {
            seen.extend_from_slice(group.tabs());
        }
        seen.sort_by_key(|id| id.get());
        let mut expected = [a, b, c];
        expected.sort_by_key(|id| id.get());
        assert_eq!(seen, expected);
    }

    // Invariante do ADR-0006: ordem total, sem lacunas.
    #[test]
    fn order_is_total_and_gapless() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let b = ws.new_tab("zsh", None, 1);
        let c = ws.new_tab("zsh", None, 1); // inserida entre a e b
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, c, b]);
    }

    // Cenário de aceite do PRD-001: "foco após fechar".
    #[test]
    fn closing_active_tab_focuses_next_sibling() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let b = ws.new_tab("zsh", None, 1);
        let c = ws.new_tab("zsh", None, 2);
        ws.activate_tab(b);

        let active = ws.close_tab(b);
        assert_eq!(active, Some(c));
        assert_eq!(tab_ids(&ws), [a, c]);
    }

    #[test]
    fn closing_active_last_tab_focuses_previous_sibling() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let b = ws.new_tab("zsh", None, 1);
        ws.activate_tab(b);

        let active = ws.close_tab(b);
        assert_eq!(active, Some(a));
    }

    #[test]
    fn closing_last_tab_leaves_workspace_without_active_tab() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        assert_eq!(ws.close_tab(a), None);
        assert!(ws.tab(a).is_none());
    }

    #[test]
    fn closing_inactive_tab_keeps_focus() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let b = ws.new_tab("zsh", None, 1);
        ws.activate_tab(a);

        let active = ws.close_tab(b);
        assert_eq!(active, Some(a));
    }

    #[test]
    fn move_tab_reorders_within_group() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let b = ws.new_tab("zsh", None, 1);
        let c = ws.new_tab("zsh", None, 2);

        assert!(ws.move_tab(c, 0));
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [c, a, b]);
    }

    #[test]
    fn move_unknown_tab_is_noop() {
        let mut ws = Workspace::new();
        ws.new_tab("zsh", None, 0);
        assert!(!ws.move_tab(TabId::new(999), 0));
    }

    #[test]
    fn next_and_prev_tab_wrap_around() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let b = ws.new_tab("zsh", None, 1);
        ws.activate_tab(a);

        assert_eq!(ws.next_tab(), Some(b));
        assert_eq!(ws.next_tab(), Some(a));
        assert_eq!(ws.prev_tab(), Some(b));
    }

    #[test]
    fn activating_tab_clears_indicators() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let b = ws.new_tab("zsh", None, 1);
        ws.tab_mut(b).unwrap().mark_activity();
        ws.tab_mut(b).unwrap().mark_bell();
        ws.activate_tab(a); // não é b: não deveria afetar b

        assert!(ws.tab(b).unwrap().activity());
        ws.activate_tab(b);
        assert!(!ws.tab(b).unwrap().activity());
        assert!(!ws.tab(b).unwrap().bell());
    }

    #[test]
    fn goto_visual_index_matches_tab_goto_n() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let b = ws.new_tab("zsh", None, 1);
        assert_eq!(ws.tab_at_visual_index(0), Some(a));
        assert_eq!(ws.activate_visual_index(1), Some(b));
        assert_eq!(ws.active_tab(), Some(b));
        assert_eq!(ws.tab_at_visual_index(2), None);
    }

    // Invariante do ADR-0006: round-trip Workspace -> JSON -> Workspace
    // preserva IDs, ordem e metadados.
    #[test]
    fn round_trips_through_json() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", Some(PathBuf::from("/home/user")), 0);
        let b = ws.new_tab("bash", None, 1);
        ws.tab_mut(a)
            .unwrap()
            .set_custom_title(Some("backend".to_string()));
        ws.tab_mut(b).unwrap().mark_activity();
        ws.activate_tab(a);

        let json = serde_json::to_string(&ws).expect("serializa");
        let restored: Workspace = serde_json::from_str(&json).expect("deserializa");

        assert_eq!(ws, restored);
    }
}
