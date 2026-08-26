// SPDX-License-Identifier: GPL-3.0-or-later

//! Rolagem do scrollback (PRD-010 RF-10.12 a RF-10.14, ADR-0013). Tipo
//! próprio -- `alacritty_terminal::grid::Scroll` não atravessa a API
//! pública deste crate.

/// `Lines` positivo sobe no histórico (mostra conteúdo mais antigo);
/// negativo desce de volta ao fundo. Mapeia direto para
/// `alacritty_terminal::grid::Scroll::Delta`, que usa a mesma convenção.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermScroll {
    Lines(i32),
    PageUp,
    PageDown,
    Top,
    Bottom,
}

impl From<TermScroll> for alacritty_terminal::grid::Scroll {
    fn from(scroll: TermScroll) -> Self {
        match scroll {
            TermScroll::Lines(n) => alacritty_terminal::grid::Scroll::Delta(n),
            TermScroll::PageUp => alacritty_terminal::grid::Scroll::PageUp,
            TermScroll::PageDown => alacritty_terminal::grid::Scroll::PageDown,
            TermScroll::Top => alacritty_terminal::grid::Scroll::Top,
            TermScroll::Bottom => alacritty_terminal::grid::Scroll::Bottom,
        }
    }
}
