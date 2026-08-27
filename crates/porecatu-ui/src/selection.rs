// SPDX-License-Identifier: GPL-3.0-or-later

//! Seleção múltipla de abas na barra (ADR-0021). Estado efêmero de janela,
//! ao lado de `RenameState`/`Hover`/`TabDrag` -- **não persistido** (a lista
//! do ADR-0005 não a inclui): sobreviver a um restart armaria uma ação
//! destrutiva de grupo sem o usuário ter escolhido o alvo desta sessão.
//!
//! `porecatu-core` não sabe o que é "seleção" -- `group_tabs` recebe a lista
//! de `TabId` pronta (ADR-0021 §1); este módulo só decide *quais* IDs.

use std::collections::BTreeSet;

use porecatu_core::TabId;

/// Seleção múltipla com âncora explícita (ADR-0021 §1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    tabs: BTreeSet<TabId>,
    /// Origem do intervalo de `Shift`+clique. `None` até o primeiro
    /// `Shift`+clique depois do último [`Selection::clear`].
    anchor: Option<TabId>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn is_selected(&self, id: TabId) -> bool {
        self.tabs.contains(&id)
    }

    /// RF-2.3 (literal): clique sem modificador limpa seleção e âncora.
    /// Também usado por `Esc` (ADR-0021 §2, mesma tecla que dispensa aviso e
    /// cancela rename). `group.create` (F3 etapa 3+) também limpa ao criar o
    /// grupo -- fora do escopo desta etapa, que ainda não wireia a ação.
    pub fn clear(&mut self) {
        self.tabs.clear();
        self.anchor = None;
    }

    /// `Ctrl`/`Cmd`+clique (RF-2.1): alterna a aba na seleção sem tocar a
    /// âncora -- só `Shift`+clique define ou lê âncora (ADR-0021 §2).
    pub fn toggle(&mut self, id: TabId) {
        if !self.tabs.remove(&id) {
            self.tabs.insert(id);
        }
    }

    /// `Shift`+clique (RF-2.1). Sem âncora (ou âncora que não existe mais em
    /// `order`): seleciona só a aba clicada e a torna âncora. Com âncora:
    /// seleciona o intervalo entre as duas em `order` -- passar
    /// `Workspace::navigable_order()` cobre as duas regras do ADR-0021 §2 de
    /// uma vez (atravessa fronteira de grupo, exclui abas de grupo
    /// colapsado) porque essa já é a ordem visual filtrada. A âncora **não**
    /// se move: repetir `Shift`+clique estende ou encolhe a partir da mesma
    /// origem, não da última seleção.
    pub fn select_range(&mut self, order: &[TabId], clicked: TabId) {
        let anchor_pos = self
            .anchor
            .and_then(|anchor| order.iter().position(|&t| t == anchor));
        let Some(anchor_pos) = anchor_pos else {
            self.tabs = BTreeSet::from([clicked]);
            self.anchor = Some(clicked);
            return;
        };
        let Some(clicked_pos) = order.iter().position(|&t| t == clicked) else {
            return;
        };
        let (lo, hi) = if anchor_pos <= clicked_pos {
            (anchor_pos, clicked_pos)
        } else {
            (clicked_pos, anchor_pos)
        };
        self.tabs = order[lo..=hi].iter().copied().collect();
    }

    /// Fechar aba selecionada (ADR-0021 §2): sai da seleção. Se era a
    /// âncora, a nova âncora é a aba selecionada mais próxima de `id` em
    /// `order` -- a ordem visual de **antes** da remoção, porque é nela que
    /// "mais próxima" faz sentido. Sem nenhuma selecionada restante, `None`.
    pub fn remove_tab(&mut self, id: TabId, order: &[TabId]) {
        self.tabs.remove(&id);
        if self.anchor == Some(id) {
            self.anchor = nearest_selected(order, id, &self.tabs);
        }
    }

    // Colapsar o grupo de uma aba selecionada também invalida a seleção
    // dela (ADR-0021 §2), no mesmo critério de `remove_tab` acima -- mas o
    // gesto que colapsa um grupo (`group.toggle_collapse`) só existe a
    // partir da F3 etapa 4 (`docs/roadmap.md`), então o método equivalente
    // entra junto com ele: sem chamador, seria código morto agora.
}

/// Aba selecionada mais próxima de `from` em `order`, por distância
/// crescente. Empate (mesma distância dos dois lados) resolve para o lado de
/// índice menor -- escolha de implementação, o ADR não distingue os lados.
fn nearest_selected(order: &[TabId], from: TabId, selected: &BTreeSet<TabId>) -> Option<TabId> {
    let pos = order.iter().position(|&t| t == from)?;
    for distance in 1..order.len() {
        if pos >= distance && selected.contains(&order[pos - distance]) {
            return Some(order[pos - distance]);
        }
        let right = pos + distance;
        if right < order.len() && selected.contains(&order[right]) {
            return Some(order[right]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: u32) -> TabId {
        TabId::new(n)
    }

    #[test]
    fn starts_empty_with_no_anchor() {
        let sel = Selection::default();
        assert!(sel.is_empty());
        assert!(!sel.is_selected(t(1)));
    }

    #[test]
    fn toggle_adds_and_removes() {
        let mut sel = Selection::default();
        sel.toggle(t(1));
        sel.toggle(t(2));
        assert!(sel.is_selected(t(1)));
        assert!(sel.is_selected(t(2)));
        sel.toggle(t(1));
        assert!(!sel.is_selected(t(1)));
        assert!(sel.is_selected(t(2)));
    }

    #[test]
    fn toggle_does_not_touch_anchor() {
        let mut sel = Selection::default();
        let order = [t(1), t(2), t(3)];
        sel.select_range(&order, t(1)); // âncora = t(1)
        sel.toggle(t(3)); // não deveria mexer na âncora
        // âncora continua t(1): Shift+clique em t(2) reabre [1,2], provando
        // que a âncora não pulou para t(3).
        sel.select_range(&order, t(2));
        assert!(sel.is_selected(t(1)));
        assert!(sel.is_selected(t(2)));
        assert!(!sel.is_selected(t(3)));
    }

    #[test]
    fn clear_resets_tabs_and_anchor() {
        let mut sel = Selection::default();
        sel.toggle(t(1));
        sel.clear();
        assert!(sel.is_empty());
        // Depois de clear, Shift+clique trata como se nunca tivesse âncora.
        sel.select_range(&[t(5), t(6)], t(6));
        assert!(sel.is_selected(t(6)));
        assert!(!sel.is_selected(t(5)));
    }

    #[test]
    fn select_range_without_anchor_selects_only_clicked_and_sets_anchor() {
        let mut sel = Selection::default();
        let order = [t(1), t(2), t(3)];
        sel.select_range(&order, t(2));
        assert!(sel.is_selected(t(2)));
        assert!(!sel.is_selected(t(1)));
        assert!(!sel.is_selected(t(3)));
    }

    #[test]
    fn select_range_with_anchor_selects_inclusive_span() {
        let mut sel = Selection::default();
        let order = [t(1), t(2), t(3), t(4), t(5)];
        sel.select_range(&order, t(2)); // âncora = t(2)
        sel.select_range(&order, t(4));
        assert!(!sel.is_selected(t(1)));
        assert!(sel.is_selected(t(2)));
        assert!(sel.is_selected(t(3)));
        assert!(sel.is_selected(t(4)));
        assert!(!sel.is_selected(t(5)));
    }

    #[test]
    fn select_range_anchor_stays_fixed_across_repeated_shift_clicks() {
        let mut sel = Selection::default();
        let order = [t(1), t(2), t(3), t(4), t(5)];
        sel.select_range(&order, t(3)); // âncora = t(3)
        sel.select_range(&order, t(5)); // [3,5]
        sel.select_range(&order, t(1)); // âncora continua t(3) -> [1,3]
        assert!(sel.is_selected(t(1)));
        assert!(sel.is_selected(t(2)));
        assert!(sel.is_selected(t(3)));
        assert!(!sel.is_selected(t(4)));
        assert!(!sel.is_selected(t(5)));
    }

    #[test]
    fn select_range_clicked_missing_from_order_is_noop() {
        let mut sel = Selection::default();
        let order = [t(1), t(2)];
        sel.select_range(&order, t(1));
        sel.select_range(&order, t(99));
        assert!(sel.is_selected(t(1)));
        assert!(!sel.is_selected(t(99)));
    }

    #[test]
    fn remove_tab_drops_from_selection() {
        let mut sel = Selection::default();
        let order = [t(1), t(2)];
        sel.toggle(t(1));
        sel.toggle(t(2));
        sel.remove_tab(t(1), &order);
        assert!(!sel.is_selected(t(1)));
        assert!(sel.is_selected(t(2)));
    }

    #[test]
    fn remove_tab_reassigns_anchor_to_nearest_selected() {
        let mut sel = Selection::default();
        let order = [t(1), t(2), t(3), t(4), t(5)];
        sel.select_range(&order, t(3)); // âncora = t(3), seleção {3}
        sel.toggle(t(1));
        sel.toggle(t(5));
        // fecha a âncora (t(3)): mais próxima é empate entre t(1) e t(5)
        // (distância 2 dos dois lados) -- desempate para o lado esquerdo.
        sel.remove_tab(t(3), &order);
        assert!(sel.is_selected(t(1)));
        assert!(sel.is_selected(t(5)));
        // âncora virou t(1): Shift+clique em t(4) seleciona [1,4].
        sel.select_range(&order, t(4));
        assert!(sel.is_selected(t(1)));
        assert!(sel.is_selected(t(2)));
        assert!(sel.is_selected(t(3)));
        assert!(sel.is_selected(t(4)));
        assert!(!sel.is_selected(t(5)));
    }

    #[test]
    fn remove_tab_clears_anchor_when_no_selection_remains() {
        let mut sel = Selection::default();
        let order = [t(1), t(2)];
        sel.select_range(&order, t(1));
        sel.remove_tab(t(1), &order);
        assert!(sel.is_empty());
        // âncora sumiu -- próximo Shift+clique se comporta como "sem
        // âncora".
        sel.select_range(&order, t(2));
        assert!(sel.is_selected(t(2)));
        assert!(!sel.is_selected(t(1)));
    }

    #[test]
    fn remove_tab_of_non_anchor_keeps_anchor() {
        let mut sel = Selection::default();
        let order = [t(1), t(2), t(3)];
        sel.select_range(&order, t(1)); // âncora = t(1)
        sel.toggle(t(3));
        sel.remove_tab(t(3), &order);
        // âncora continua t(1): Shift+clique em t(3) de novo reabre [1,3].
        sel.select_range(&order, t(3));
        assert!(sel.is_selected(t(1)));
        assert!(sel.is_selected(t(2)));
        assert!(sel.is_selected(t(3)));
    }
}
