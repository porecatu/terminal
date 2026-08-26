// SPDX-License-Identifier: GPL-3.0-or-later

//! Seleção com mouse pelos quatro modos do motor (ADR-0002/0013 -- usar o
//! que já existe, não reimplementar). Tipos próprios para o "tipo" de
//! seleção e o "lado" da âncora -- `alacritty_terminal::selection` não
//! atravessa a API pública.

use alacritty_terminal::index::Side as AlacSide;
use alacritty_terminal::selection::SelectionType as AlacSelectionType;

/// Os quatro modos de seleção do motor (PRD-010 RF-10.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// Arraste: caractere a caractere.
    Simple,
    /// Duplo clique: palavra, com separadores de `TermParams::word_separators`.
    Semantic,
    /// Triplo clique: linha lógica inteira.
    Lines,
    /// `Alt` + arraste: retangular.
    Block,
}

impl From<SelectionKind> for AlacSelectionType {
    fn from(kind: SelectionKind) -> Self {
        match kind {
            SelectionKind::Simple => AlacSelectionType::Simple,
            SelectionKind::Semantic => AlacSelectionType::Semantic,
            SelectionKind::Lines => AlacSelectionType::Lines,
            SelectionKind::Block => AlacSelectionType::Block,
        }
    }
}

/// De que lado (metade esquerda/direita) da célula o ponto está -- decide
/// se a célula sob o cursor entra ou não na borda da seleção.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionSide {
    Left,
    Right,
}

impl From<SelectionSide> for AlacSide {
    fn from(side: SelectionSide) -> Self {
        match side {
            SelectionSide::Left => AlacSide::Left,
            SelectionSide::Right => AlacSide::Right,
        }
    }
}
