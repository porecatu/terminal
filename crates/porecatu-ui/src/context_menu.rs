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
    CloseTab,
    /// RF-2.20: mover a aba pra um grupo. Sempre habilitado desde a F3
    /// etapa 5 (ADR-0023 §4): "Novo grupo" é sempre um destino válido,
    /// mesmo sem nenhum grupo explícito existir ainda. Aciona abre o
    /// popover de destino (`move_to_group.rs`), não move direto.
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

/// Origem: `docs/reference/acoes.md` -- RF-1.1 (nova aba), RF-1.2 (fechar
/// aba), RF-2.20 (mover pra grupo, desabilitado nesta fase).
pub const TAB_MENU_ITEMS: [MenuItem; 3] = [
    MenuItem {
        action: MenuAction::NewTab,
        enabled: true,
    },
    MenuItem {
        action: MenuAction::CloseTab,
        enabled: true,
    },
    MenuItem {
        action: MenuAction::MoveToGroup,
        enabled: true,
    },
];

/// Os itens do menu de uma aba SSH: os de sempre e "Reconectar" por último
/// (ADR-0075 §8).
pub const SSH_TAB_MENU_ITEMS: [MenuItem; 4] = [
    TAB_MENU_ITEMS[0],
    TAB_MENU_ITEMS[1],
    TAB_MENU_ITEMS[2],
    MenuItem {
        action: MenuAction::Reconnect,
        enabled: true,
    },
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
}

impl ContextMenu {
    #[cfg(test)]
    pub fn new(tab: TabId, anchor: (f32, f32)) -> Self {
        Self::for_tab(tab, anchor, false)
    }

    /// O menu de uma aba; `ssh` acrescenta "Reconectar" (ADR-0075 §8).
    pub fn for_tab(tab: TabId, anchor: (f32, f32), ssh: bool) -> Self {
        Self {
            tab,
            anchor,
            highlighted: 0,
            ssh,
        }
    }

    /// Os itens deste menu, na ordem em que aparecem.
    pub fn items(&self) -> &'static [MenuItem] {
        if self.ssh {
            &SSH_TAB_MENU_ITEMS
        } else {
            &TAB_MENU_ITEMS
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
        menu.set_highlight(2);
        assert_eq!(menu.selected(), MenuAction::MoveToGroup);
        menu.set_highlight(1);
        assert_eq!(menu.selected(), MenuAction::CloseTab);
    }

    #[test]
    fn a_plain_tab_menu_has_no_reconnect_and_an_ssh_one_ends_with_it() {
        let plain = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), false);
        let ssh = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), true);

        assert!(
            plain
                .items()
                .iter()
                .all(|item| item.action != MenuAction::Reconnect)
        );
        assert_eq!(ssh.items().len(), plain.items().len() + 1);
        assert_eq!(ssh.items().last().unwrap().action, MenuAction::Reconnect);
        // The usual items keep their order.
        assert_eq!(&ssh.items()[..3], plain.items());
    }

    #[test]
    fn the_ssh_menu_can_highlight_and_select_reconnect() {
        let mut menu = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), true);
        menu.move_highlight(-1);
        assert_eq!(menu.selected(), MenuAction::Reconnect);
        menu.move_highlight(1);
        assert_eq!(menu.selected(), MenuAction::NewTab);
        menu.set_highlight(3);
        assert_eq!(menu.selected(), MenuAction::Reconnect);

        // A plain menu has nothing at index 3.
        let mut plain = ContextMenu::for_tab(TabId::new(0), (0.0, 0.0), false);
        plain.set_highlight(3);
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
    fn set_highlight_ignores_out_of_range_index() {
        let mut menu = ContextMenu::new(TabId::new(0), (0.0, 0.0));
        menu.set_highlight(99);
        assert_eq!(menu.selected(), MenuAction::NewTab);
    }
}
