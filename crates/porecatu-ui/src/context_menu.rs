// SPDX-License-Identifier: GPL-3.0-or-later

//! Menu de contexto da aba (ADR-0014 §2.16, ADR-0008, PRD-010 RF-10.19/
//! RF-10.20). F2 só tem este menu -- o de grupo é RF-2.22/F3
//! (`group_menu.rs`), o de terminal é F6 (docs/reference/acoes.md).
//! Ancorado no cursor; a geometria de virada pra caber na janela é
//! pintura (`overlay.rs`), não deste módulo.

use porecatu_core::TabId;
use porecatu_locale::Catalog;

use crate::messages::msg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    NewTab,
    /// ADR-0087 §6, RF-21.7: fixa a aba clicada. Só no menu de uma aba da
    /// trilha ([`TAB_MENU_ITEMS`]).
    Pin,
    /// ADR-0087 §6, RF-21.10: desafixa a aba clicada. Só no menu de uma
    /// fixada ([`PINNED_TAB_MENU_ITEMS`]).
    Unpin,
    CloseTab,
    /// RF-2.20: mover a aba pra um grupo. Sempre habilitado desde a F3
    /// etapa 5 (ADR-0023 §4): "Novo grupo" é sempre um destino válido,
    /// mesmo sem nenhum grupo explícito existir ainda. Aciona abre o
    /// popover de destino (`move_to_group.rs`), não move direto. Numa aba
    /// **fixada** o item vem esmaecido, nunca ausente (RF-21.12): ela não
    /// pertence a grupo.
    MoveToGroup,
    /// ADR-0075 §8, RF-20.31: reconecta o painel SSH focado da aba. Só existe
    /// no menu de uma aba SSH ([`SSH_TAB_MENU_ITEMS`]).
    Reconnect,
}

impl MenuAction {
    /// Rótulo do item no idioma do catálogo (ADR-0056): o texto é lido a
    /// cada frame, então uma troca de idioma vale no menu já aberto.
    pub fn label(self, catalog: &Catalog) -> String {
        match self {
            MenuAction::NewTab => msg::tab_menu::new(catalog),
            MenuAction::Pin => msg::tab_menu::pin(catalog),
            MenuAction::Unpin => msg::tab_menu::unpin(catalog),
            MenuAction::CloseTab => msg::tab_menu::close(catalog),
            MenuAction::MoveToGroup => msg::tab_menu::move_to_group(catalog),
            MenuAction::Reconnect => msg::tab_menu::reconnect(catalog),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuItem {
    pub action: MenuAction,
    pub enabled: bool,
}

const fn item(action: MenuAction, enabled: bool) -> MenuItem {
    MenuItem { action, enabled }
}

/// Os itens do menu de uma aba da trilha. Origem: `docs/reference/acoes.md`
/// -- RF-1.1 (nova aba), RF-1.2 (fechar aba), RF-2.20 (mover pra grupo) e,
/// em segundo lugar, "Fixar aba" (ADR-0087 §6): o item novo entra sem mudar
/// a ordem relativa dos que já existiam.
pub const TAB_MENU_ITEMS: [MenuItem; 4] = [
    item(MenuAction::NewTab, true),
    item(MenuAction::Pin, true),
    item(MenuAction::CloseTab, true),
    item(MenuAction::MoveToGroup, true),
];

/// Os itens do menu de uma aba SSH: os de sempre e "Reconectar" por último
/// (ADR-0075 §8).
pub const SSH_TAB_MENU_ITEMS: [MenuItem; 5] = [
    TAB_MENU_ITEMS[0],
    TAB_MENU_ITEMS[1],
    TAB_MENU_ITEMS[2],
    TAB_MENU_ITEMS[3],
    item(MenuAction::Reconnect, true),
];

/// Os itens do menu de uma aba **fixada** (ADR-0087 §6): "Desafixar aba" no
/// lugar de "Fixar aba", e "Mover para grupo" esmaecido (RF-21.12).
pub const PINNED_TAB_MENU_ITEMS: [MenuItem; 4] = [
    item(MenuAction::NewTab, true),
    item(MenuAction::Unpin, true),
    item(MenuAction::CloseTab, true),
    item(MenuAction::MoveToGroup, false),
];

/// Uma fixada SSH: os da fixada e "Reconectar" por último.
pub const PINNED_SSH_TAB_MENU_ITEMS: [MenuItem; 5] = [
    PINNED_TAB_MENU_ITEMS[0],
    PINNED_TAB_MENU_ITEMS[1],
    PINNED_TAB_MENU_ITEMS[2],
    PINNED_TAB_MENU_ITEMS[3],
    item(MenuAction::Reconnect, true),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextMenu {
    pub tab: TabId,
    /// Coordenadas lógicas da janela, o ponto do clique que abriu o menu
    /// (RF-10.19: "ancorados no cursor").
    pub anchor: (f32, f32),
    highlighted: usize,
    /// A aba é SSH: o menu ganha "Reconectar".
    ssh: bool,
    /// A aba é fixada: o menu troca "Fixar" por "Desafixar" e esmaece
    /// "Mover para grupo".
    pinned: bool,
}

impl ContextMenu {
    #[cfg(test)]
    pub fn new(tab: TabId, anchor: (f32, f32)) -> Self {
        Self::for_tab(tab, anchor, false, false)
    }

    /// O menu de uma aba; `ssh` acrescenta "Reconectar" (ADR-0075 §8) e
    /// `pinned` escolhe o menu da fixada (ADR-0087 §6).
    pub fn for_tab(tab: TabId, anchor: (f32, f32), ssh: bool, pinned: bool) -> Self {
        Self {
            tab,
            anchor,
            highlighted: 0,
            ssh,
            pinned,
        }
    }

    /// Os itens deste menu, na ordem em que aparecem.
    pub fn items(&self) -> &'static [MenuItem] {
        match (self.pinned, self.ssh) {
            (false, false) => &TAB_MENU_ITEMS,
            (false, true) => &SSH_TAB_MENU_ITEMS,
            (true, false) => &PINNED_TAB_MENU_ITEMS,
            (true, true) => &PINNED_SSH_TAB_MENU_ITEMS,
        }
    }

    pub const fn highlighted(&self) -> usize {
        self.highlighted
    }

    /// Navega por setas (espec §2.16: "navegável por setas"), pulando
    /// itens desabilitados -- "hover e foco por teclado são o mesmo estado
    /// visual", então o item realçado é sempre um que `Enter` pode
    /// acionar de verdade.
    pub fn move_highlight(&mut self, delta: isize) {
        let items = self.items();
        let len = items.len() as isize;
        let mut idx = self.highlighted as isize;
        for _ in 0..len {
            idx = (idx + delta).rem_euclid(len);
            if items[idx as usize].enabled {
                self.highlighted = idx as usize;
                return;
            }
        }
    }

    pub fn set_highlight(&mut self, index: usize) {
        if self.items().get(index).is_some_and(|item| item.enabled) {
            self.highlighted = index;
        }
    }

    pub fn selected(&self) -> MenuAction {
        self.items()[self.highlighted].action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actions(menu: &ContextMenu) -> Vec<MenuAction> {
        menu.items().iter().map(|item| item.action).collect()
    }

    #[test]
    fn starts_highlighting_first_enabled_item() {
        let menu = ContextMenu::new(TabId::new(0), (0.0, 0.0));
        assert_eq!(menu.highlighted(), 0);
        assert_eq!(menu.selected(), MenuAction::NewTab);
    }

    #[test]
    fn move_highlight_advances_and_wraps() {
        let mut menu = ContextMenu::new(TabId::new(0), (0.0, 0.0));
        menu.move_highlight(1);
        assert_eq!(menu.selected(), MenuAction::Pin);
        menu.move_highlight(1);
        assert_eq!(menu.selected(), MenuAction::CloseTab);
        menu.move_highlight(1);
        assert_eq!(menu.selected(), MenuAction::MoveToGroup);
        menu.move_highlight(1);
        assert_eq!(menu.selected(), MenuAction::NewTab);
    }

    #[test]
    fn move_highlight_backwards_wraps_to_last() {
        let mut menu = ContextMenu::new(TabId::new(0), (0.0, 0.0));
        menu.move_highlight(-1);
        assert_eq!(menu.selected(), MenuAction::MoveToGroup);
    }

    #[test]
    fn set_highlight_moves_to_valid_index() {
        let mut menu = ContextMenu::new(TabId::new(0), (0.0, 0.0));
        menu.set_highlight(3);
        assert_eq!(menu.selected(), MenuAction::MoveToGroup);
        menu.set_highlight(2);
        assert_eq!(menu.selected(), MenuAction::CloseTab);
    }

    #[test]
    fn a_plain_tab_menu_lists_new_pin_close_move_in_that_order() {
        let menu = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), false, false);
        assert_eq!(
            actions(&menu),
            [
                MenuAction::NewTab,
                MenuAction::Pin,
                MenuAction::CloseTab,
                MenuAction::MoveToGroup
            ]
        );
        assert!(menu.items().iter().all(|item| item.enabled));
    }

    #[test]
    fn the_item_that_pins_keeps_the_relative_order_of_the_old_ones() {
        // Reordering the existing items would be a visual change without
        // the owner's approval: only the new item is inserted.
        let old: Vec<MenuAction> = actions(&ContextMenu::new(TabId::new(0), (0.0, 0.0)))
            .into_iter()
            .filter(|action| *action != MenuAction::Pin)
            .collect();
        assert_eq!(
            old,
            [
                MenuAction::NewTab,
                MenuAction::CloseTab,
                MenuAction::MoveToGroup
            ]
        );
    }

    #[test]
    fn a_pinned_tab_menu_offers_unpin_and_dims_move_to_group() {
        let menu = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), false, true);
        assert_eq!(
            actions(&menu),
            [
                MenuAction::NewTab,
                MenuAction::Unpin,
                MenuAction::CloseTab,
                MenuAction::MoveToGroup
            ]
        );
        let move_item = menu
            .items()
            .iter()
            .find(|item| item.action == MenuAction::MoveToGroup)
            .expect("RF-21.12: dimmed, never absent");
        assert!(!move_item.enabled);
        assert!(
            menu.items()
                .iter()
                .filter(|item| item.action != MenuAction::MoveToGroup)
                .all(|item| item.enabled)
        );
    }

    #[test]
    fn a_pinned_ssh_tab_menu_ends_with_reconnect_and_still_dims_move_to_group() {
        let menu = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), true, true);
        assert_eq!(
            actions(&menu),
            [
                MenuAction::NewTab,
                MenuAction::Unpin,
                MenuAction::CloseTab,
                MenuAction::MoveToGroup,
                MenuAction::Reconnect
            ]
        );
        assert!(!menu.items()[3].enabled);
        assert!(menu.items()[4].enabled);
    }

    #[test]
    fn the_pinned_menu_navigation_skips_the_dimmed_item() {
        let mut menu = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), false, true);
        menu.move_highlight(1);
        assert_eq!(menu.selected(), MenuAction::Unpin);
        menu.move_highlight(1);
        assert_eq!(menu.selected(), MenuAction::CloseTab);
        // The next one is dimmed: wraps to the first.
        menu.move_highlight(1);
        assert_eq!(menu.selected(), MenuAction::NewTab);
        // Backwards from the first, the dimmed last one is skipped too.
        menu.move_highlight(-1);
        assert_eq!(menu.selected(), MenuAction::CloseTab);
        // A dimmed item cannot be highlighted by the mouse either.
        menu.set_highlight(3);
        assert_eq!(menu.selected(), MenuAction::CloseTab);
    }

    #[test]
    fn a_plain_tab_menu_has_no_reconnect_and_an_ssh_one_ends_with_it() {
        let plain = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), false, false);
        let ssh = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), true, false);

        assert!(
            plain
                .items()
                .iter()
                .all(|item| item.action != MenuAction::Reconnect)
        );
        assert_eq!(ssh.items().len(), plain.items().len() + 1);
        assert_eq!(ssh.items().last().unwrap().action, MenuAction::Reconnect);
        // The usual items keep their order.
        assert_eq!(&ssh.items()[..4], plain.items());
    }

    #[test]
    fn the_ssh_menu_can_highlight_and_select_reconnect() {
        let mut menu = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), true, false);
        menu.move_highlight(-1);
        assert_eq!(menu.selected(), MenuAction::Reconnect);
        menu.move_highlight(1);
        assert_eq!(menu.selected(), MenuAction::NewTab);
        menu.set_highlight(4);
        assert_eq!(menu.selected(), MenuAction::Reconnect);

        // A plain menu has nothing at index 4.
        let mut plain = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), false, false);
        plain.set_highlight(4);
        assert_eq!(plain.selected(), MenuAction::NewTab);
    }

    #[test]
    fn reconnect_is_translated() {
        use crate::messages::test_support;
        assert_eq!(
            MenuAction::Reconnect.label(&test_support::pt_br()),
            "Reconectar"
        );
        for catalog in test_support::all_catalogs() {
            assert!(!MenuAction::Reconnect.label(&catalog).is_empty());
        }
    }

    #[test]
    fn pin_and_unpin_are_translated_in_every_language() {
        use crate::messages::test_support;
        let pt = test_support::pt_br();
        assert_eq!(MenuAction::Pin.label(&pt), "Fixar aba");
        assert_eq!(MenuAction::Unpin.label(&pt), "Desafixar aba");
        for catalog in test_support::all_catalogs() {
            for action in [MenuAction::Pin, MenuAction::Unpin] {
                let label = action.label(&catalog);
                assert!(!label.is_empty());
                assert!(!label.starts_with("tab_menu."), "missing phrase: {label}");
            }
        }
    }

    #[test]
    fn set_highlight_ignores_out_of_range_index() {
        let mut menu = ContextMenu::new(TabId::new(0), (0.0, 0.0));
        menu.set_highlight(99);
        assert_eq!(menu.selected(), MenuAction::NewTab);
    }
}
