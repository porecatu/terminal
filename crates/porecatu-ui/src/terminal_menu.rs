// SPDX-License-Identifier: GPL-3.0-or-later

//! Menu de contexto do terminal (espec §2.16, PRD-011 RF-11.14 a RF-11.16,
//! ADR-0042 §9). Terceiro menu -- o de aba é `context_menu.rs`, o de grupo
//! é `group_menu.rs`; mesma anatomia, reusada, não um widget novo.
//!
//! Diferente dos outros dois, a **contagem** de itens varia: quatro sem
//! hyperlink sob o clique, seis com ele (abrir link, copiar link). Por
//! isso [`TerminalContextMenu`] não indexa uma constante -- `move_highlight`/
//! `set_highlight`/`selected` recebem a lista corrente (`terminal_menu_items`,
//! recomputada de estado ao vivo em `lib.rs`, mesmo padrão de
//! `group_menu::group_action_items`), nunca a guardam.

use porecatu_core::TabId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalMenuAction {
    Copy,
    Paste,
    SelectAll,
    OpenSearch,
    OpenLink,
    CopyLink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalMenuItem {
    pub action: TerminalMenuAction,
    pub label: &'static str,
    pub enabled: bool,
}

/// Ordem do escopo da etapa 3: copiar, colar, selecionar tudo, abrir a
/// busca -- mais, só sobre um hyperlink, abrir e copiar o link.
///
/// `has_selection` é o único que desabilita item em vez de omiti-lo
/// (RF-11.15, "copiar sem seleção é o caso óbvio"): colar, selecionar tudo
/// e abrir a busca sempre têm alvo válido enquanto há uma aba.
pub fn terminal_menu_items(has_selection: bool, over_link: bool) -> Vec<TerminalMenuItem> {
    let mut items = vec![
        TerminalMenuItem {
            action: TerminalMenuAction::Copy,
            label: "Copiar",
            enabled: has_selection,
        },
        TerminalMenuItem {
            action: TerminalMenuAction::Paste,
            label: "Colar",
            enabled: true,
        },
        TerminalMenuItem {
            action: TerminalMenuAction::SelectAll,
            label: "Selecionar tudo",
            enabled: true,
        },
        TerminalMenuItem {
            action: TerminalMenuAction::OpenSearch,
            label: "Buscar",
            enabled: true,
        },
    ];
    if over_link {
        items.push(TerminalMenuItem {
            action: TerminalMenuAction::OpenLink,
            label: "Abrir link",
            enabled: true,
        });
        items.push(TerminalMenuItem {
            action: TerminalMenuAction::CopyLink,
            label: "Copiar link",
            enabled: true,
        });
    }
    items
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalContextMenu {
    pub tab: TabId,
    /// Coordenadas lógicas da janela, o ponto do clique que abriu o menu
    /// -- e a mesma célula que decide `over_link` em toda recomputação
    /// (RF-10.19: "ancorados no cursor").
    pub anchor: (f32, f32),
    highlighted: usize,
}

impl TerminalContextMenu {
    pub fn new(tab: TabId, anchor: (f32, f32), items: &[TerminalMenuItem]) -> Self {
        Self {
            tab,
            anchor,
            highlighted: first_enabled_index(items),
        }
    }

    pub const fn highlighted(&self) -> usize {
        self.highlighted
    }

    /// Navega por setas, pulando item desabilitado -- mesma regra do menu
    /// de aba (`context_menu::ContextMenu::move_highlight`), com a lista
    /// passada em vez de lida de uma constante.
    pub fn move_highlight(&mut self, delta: isize, items: &[TerminalMenuItem]) {
        let len = items.len() as isize;
        if len == 0 {
            return;
        }
        let mut idx = self.highlighted as isize;
        for _ in 0..len {
            idx = (idx + delta).rem_euclid(len);
            if items[idx as usize].enabled {
                self.highlighted = idx as usize;
                return;
            }
        }
    }

    pub fn set_highlight(&mut self, index: usize, items: &[TerminalMenuItem]) {
        if items.get(index).is_some_and(|item| item.enabled) {
            self.highlighted = index;
        }
    }

    pub fn selected(&self, items: &[TerminalMenuItem]) -> Option<TerminalMenuAction> {
        items.get(self.highlighted).map(|item| item.action)
    }
}

fn first_enabled_index(items: &[TerminalMenuItem]) -> usize {
    items.iter().position(|item| item.enabled).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: u32) -> TabId {
        TabId::new(n)
    }

    #[test]
    fn sem_link_tem_quatro_itens_na_ordem_do_escopo() {
        let items = terminal_menu_items(true, false);
        let actions: Vec<_> = items.iter().map(|i| i.action).collect();
        assert_eq!(
            actions,
            [
                TerminalMenuAction::Copy,
                TerminalMenuAction::Paste,
                TerminalMenuAction::SelectAll,
                TerminalMenuAction::OpenSearch,
            ]
        );
    }

    #[test]
    fn sobre_link_ganha_abrir_e_copiar_link_no_fim() {
        let items = terminal_menu_items(true, true);
        let actions: Vec<_> = items.iter().map(|i| i.action).collect();
        assert_eq!(
            actions,
            [
                TerminalMenuAction::Copy,
                TerminalMenuAction::Paste,
                TerminalMenuAction::SelectAll,
                TerminalMenuAction::OpenSearch,
                TerminalMenuAction::OpenLink,
                TerminalMenuAction::CopyLink,
            ]
        );
    }

    /// RF-11.15: sem seleção, "Copiar" aparece esmaecido, não ausente.
    #[test]
    fn copiar_sem_selecao_fica_desabilitado_mas_presente() {
        let items = terminal_menu_items(false, false);
        assert_eq!(items.len(), 4);
        assert!(!items[0].enabled);
        assert_eq!(items[0].action, TerminalMenuAction::Copy);
    }

    #[test]
    fn abre_realcando_o_primeiro_item_habilitado() {
        let disabled_copy = terminal_menu_items(false, false);
        let menu = TerminalContextMenu::new(t(1), (0.0, 0.0), &disabled_copy);
        assert_eq!(
            menu.selected(&disabled_copy),
            Some(TerminalMenuAction::Paste)
        );
    }

    #[test]
    fn move_highlight_pula_item_desabilitado_e_encurrala() {
        let items = terminal_menu_items(false, false);
        let mut menu = TerminalContextMenu::new(t(1), (0.0, 0.0), &items);
        menu.move_highlight(-1, &items);
        assert_eq!(menu.selected(&items), Some(TerminalMenuAction::OpenSearch));
    }

    #[test]
    fn set_highlight_ignora_item_desabilitado() {
        let items = terminal_menu_items(false, false);
        let mut menu = TerminalContextMenu::new(t(1), (0.0, 0.0), &items);
        menu.set_highlight(0, &items);
        assert_eq!(menu.selected(&items), Some(TerminalMenuAction::Paste));
        menu.set_highlight(2, &items);
        assert_eq!(menu.selected(&items), Some(TerminalMenuAction::SelectAll));
    }
}
