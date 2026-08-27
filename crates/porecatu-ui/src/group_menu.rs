// SPDX-License-Identifier: GPL-3.0-or-later

//! Lista única de ações de grupo (RF-10.21, `docs/reference/acoes.md`): o
//! menu de contexto do grupo (aqui, `GroupContextMenu`) e a lista de ações
//! do editor de grupo (`group_editor.rs`) leem [`group_action_items`], não
//! duas definições que divergiriam na primeira mudança.
//!
//! O menu de grupo só abre a partir da pílula (`TabBarHit::Pill`,
//! `lib.rs`), que só existe pra grupo **explícito** (`tab_bar.rs`) --
//! diferente de `context_menu::TAB_MENU_ITEMS`, nenhum item nasce
//! desabilitado aqui: as seis ações do RF-2.22 estão sempre disponíveis
//! sobre um grupo explícito (`docs/reference/acoes.md`: "sobre um grupo
//! implícito... ficam indisponíveis" -- mas o menu nunca abre sobre um).

use porecatu_core::GroupId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupAction {
    Rename,
    SetColor,
    ToggleCollapse,
    NewTab,
    CloseAll,
    Dissolve,
}

/// Ordem do RF-2.22 e do catálogo: renomear, mudar cor, colapsar/expandir,
/// nova aba no grupo, fechar grupo, desagrupar.
pub const GROUP_ACTION_ORDER: [GroupAction; 6] = [
    GroupAction::Rename,
    GroupAction::SetColor,
    GroupAction::ToggleCollapse,
    GroupAction::NewTab,
    GroupAction::CloseAll,
    GroupAction::Dissolve,
];

/// Editor de grupo (espec. §2.10, item 4): só três das seis -- renomear e
/// mudar cor já são o campo de nome e a faixa de swatches, não itens de
/// lista lá dentro (ADR-0023 §4). Ordem própria da espec.: colapsar/
/// expandir, desagrupar, fechar grupo.
pub const EDITOR_ACTION_ORDER: [GroupAction; 3] = [
    GroupAction::ToggleCollapse,
    GroupAction::Dissolve,
    GroupAction::CloseAll,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupActionItem {
    pub action: GroupAction,
    pub label: String,
    /// "Fechar grupo (N abas)" -- `#e08585`, a única ação destrutiva das
    /// seis (RF-2.23: "a ação mais destrutiva da interface").
    pub destructive: bool,
}

/// Definição única (RF-10.21). `is_collapsed`/`tab_count` resolvem os dois
/// rótulos dinâmicos -- "Colapsar"/"Expandir grupo" e "Fechar grupo (N
/// abas)" -- a partir do estado corrente do grupo, nunca guardado aqui.
pub fn group_action_items(is_collapsed: bool, tab_count: usize) -> [GroupActionItem; 6] {
    let collapse_label = if is_collapsed {
        "Expandir grupo"
    } else {
        "Colapsar grupo"
    };
    let plural = if tab_count == 1 { "" } else { "s" };
    [
        GroupActionItem {
            action: GroupAction::Rename,
            label: "Renomear".to_string(),
            destructive: false,
        },
        GroupActionItem {
            action: GroupAction::SetColor,
            label: "Mudar cor".to_string(),
            destructive: false,
        },
        GroupActionItem {
            action: GroupAction::ToggleCollapse,
            label: collapse_label.to_string(),
            destructive: false,
        },
        GroupActionItem {
            action: GroupAction::NewTab,
            label: "Nova aba no grupo".to_string(),
            destructive: false,
        },
        GroupActionItem {
            action: GroupAction::CloseAll,
            label: format!("Fechar grupo ({tab_count} aba{plural})"),
            destructive: true,
        },
        GroupActionItem {
            action: GroupAction::Dissolve,
            label: "Desagrupar".to_string(),
            destructive: false,
        },
    ]
}

/// Menu de contexto do grupo (espec. §2.16, RF-2.22). Mesmo padrão de
/// `context_menu::ContextMenu`, mas sem item desabilitado -- ver nota do
/// módulo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupContextMenu {
    pub group: GroupId,
    /// Coordenadas lógicas da janela, o ponto do clique que abriu o menu.
    pub anchor: (f32, f32),
    highlighted: usize,
}

impl GroupContextMenu {
    pub const fn new(group: GroupId, anchor: (f32, f32)) -> Self {
        Self {
            group,
            anchor,
            highlighted: 0,
        }
    }

    pub const fn highlighted(&self) -> usize {
        self.highlighted
    }

    pub fn move_highlight(&mut self, delta: isize) {
        let len = GROUP_ACTION_ORDER.len() as isize;
        self.highlighted = (self.highlighted as isize + delta).rem_euclid(len) as usize;
    }

    pub fn set_highlight(&mut self, index: usize) {
        if index < GROUP_ACTION_ORDER.len() {
            self.highlighted = index;
        }
    }

    pub fn selected(&self) -> GroupAction {
        GROUP_ACTION_ORDER[self.highlighted]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(n: u32) -> GroupId {
        GroupId::new(n)
    }

    #[test]
    fn group_action_items_toggle_collapse_label_reflects_state() {
        let expanded = group_action_items(false, 3);
        let collapsed = group_action_items(true, 3);
        assert_eq!(expanded[2].label, "Colapsar grupo");
        assert_eq!(collapsed[2].label, "Expandir grupo");
    }

    #[test]
    fn group_action_items_close_all_label_counts_tabs_and_pluralizes() {
        let one = group_action_items(false, 1);
        let many = group_action_items(false, 4);
        assert_eq!(one[4].label, "Fechar grupo (1 aba)");
        assert_eq!(many[4].label, "Fechar grupo (4 abas)");
        assert!(one[4].destructive);
    }

    #[test]
    fn group_action_items_order_matches_rf_2_22() {
        let items = group_action_items(false, 0);
        let actions: Vec<GroupAction> = items.iter().map(|i| i.action).collect();
        assert_eq!(actions, GROUP_ACTION_ORDER);
    }

    #[test]
    fn editor_action_order_is_subset_in_spec_order() {
        assert_eq!(
            EDITOR_ACTION_ORDER,
            [
                GroupAction::ToggleCollapse,
                GroupAction::Dissolve,
                GroupAction::CloseAll,
            ]
        );
    }

    #[test]
    fn group_context_menu_starts_at_first_item() {
        let menu = GroupContextMenu::new(g(1), (0.0, 0.0));
        assert_eq!(menu.highlighted(), 0);
        assert_eq!(menu.selected(), GroupAction::Rename);
    }

    #[test]
    fn group_context_menu_move_highlight_wraps() {
        let mut menu = GroupContextMenu::new(g(1), (0.0, 0.0));
        menu.move_highlight(-1);
        assert_eq!(menu.selected(), GroupAction::Dissolve);
        menu.move_highlight(1);
        assert_eq!(menu.selected(), GroupAction::Rename);
    }

    #[test]
    fn group_context_menu_set_highlight_ignores_out_of_range() {
        let mut menu = GroupContextMenu::new(g(1), (0.0, 0.0));
        menu.set_highlight(99);
        assert_eq!(menu.selected(), GroupAction::Rename);
        menu.set_highlight(3);
        assert_eq!(menu.selected(), GroupAction::NewTab);
    }
}
