// SPDX-License-Identifier: GPL-3.0-or-later

//! Menu de contexto da aba (ADR-0014 §2.16, ADR-0008, PRD-010 RF-10.19/
//! RF-10.20). F2 só tem este menu -- o de grupo é RF-2.22/F3, o de
//! terminal é F6 (docs/reference/acoes.md). Ancorado no cursor; a
//! geometria de virada pra caber na janela é pintura (`overlay.rs`), não
//! deste módulo.

use porecatu_core::TabId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    NewTab,
    CloseTab,
    /// RF-2.20: mover a aba pra um grupo. Sem grupo explícito até a F3
    /// (só existe o implícito, ADR-0006), o item fica **esmaecido, nunca
    /// ausente** (RF-10.20) -- não é omissão, é o estado que o requisito
    /// pede enquanto a ação de destino não existe.
    MoveToGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuItem {
    pub action: MenuAction,
    pub label: &'static str,
    pub enabled: bool,
}

/// Origem: `docs/reference/acoes.md` -- RF-1.1 (nova aba), RF-1.2 (fechar
/// aba), RF-2.20 (mover pra grupo, desabilitado nesta fase).
pub const TAB_MENU_ITEMS: [MenuItem; 3] = [
    MenuItem {
        action: MenuAction::NewTab,
        label: "Nova aba",
        enabled: true,
    },
    MenuItem {
        action: MenuAction::CloseTab,
        label: "Fechar aba",
        enabled: true,
    },
    MenuItem {
        action: MenuAction::MoveToGroup,
        label: "Mover para grupo",
        enabled: false,
    },
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextMenu {
    pub tab: TabId,
    /// Coordenadas lógicas da janela, o ponto do clique que abriu o menu
    /// (RF-10.19: "ancorados no cursor").
    pub anchor: (f32, f32),
    highlighted: usize,
}

impl ContextMenu {
    pub fn new(tab: TabId, anchor: (f32, f32)) -> Self {
        Self {
            tab,
            anchor,
            highlighted: first_enabled_index(),
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
        let len = TAB_MENU_ITEMS.len() as isize;
        let mut idx = self.highlighted as isize;
        for _ in 0..len {
            idx = (idx + delta).rem_euclid(len);
            if TAB_MENU_ITEMS[idx as usize].enabled {
                self.highlighted = idx as usize;
                return;
            }
        }
    }

    pub fn set_highlight(&mut self, index: usize) {
        if TAB_MENU_ITEMS.get(index).is_some_and(|item| item.enabled) {
            self.highlighted = index;
        }
    }

    pub fn selected(&self) -> MenuAction {
        TAB_MENU_ITEMS[self.highlighted].action
    }
}

fn first_enabled_index() -> usize {
    TAB_MENU_ITEMS
        .iter()
        .position(|item| item.enabled)
        .unwrap_or(0)
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
    fn move_highlight_skips_disabled_items() {
        let mut menu = ContextMenu::new(TabId::new(0), (0.0, 0.0));
        menu.move_highlight(1);
        assert_eq!(menu.selected(), MenuAction::CloseTab);
        // O terceiro item (mover pra grupo) está desabilitado -- avançar
        // de novo deve pular pra ele e circular de volta ao primeiro.
        menu.move_highlight(1);
        assert_eq!(menu.selected(), MenuAction::NewTab);
    }

    #[test]
    fn move_highlight_backwards_wraps_to_last_enabled() {
        let mut menu = ContextMenu::new(TabId::new(0), (0.0, 0.0));
        menu.move_highlight(-1);
        assert_eq!(menu.selected(), MenuAction::CloseTab);
    }

    #[test]
    fn set_highlight_ignores_disabled_index() {
        let mut menu = ContextMenu::new(TabId::new(0), (0.0, 0.0));
        menu.set_highlight(2);
        assert_eq!(menu.selected(), MenuAction::NewTab);
        menu.set_highlight(1);
        assert_eq!(menu.selected(), MenuAction::CloseTab);
    }
}
