// SPDX-License-Identifier: GPL-3.0-or-later

//! `alacritty_terminal` encapsulado (ADR-0002). Fronteira mais crítica do
//! projeto -- ver docs/arquitetura.md seção 4. Três regras:
//!
//! - Nenhum tipo do `alacritty_terminal` atravessa a API pública deste
//!   crate (nem no snapshot, nem nos eventos).
//! - `porecatu-term` não importa `porecatu-config`: [`TermParams`] é o
//!   struct de parâmetros próprio, montado por `porecatu-ui`.
//! - Cor de célula não é resolvida aqui -- ver [`TermColor`].

mod color;
mod engine;
mod event;
mod params;
mod snapshot;

pub use color::TermColor;
pub use engine::{TermEngine, TermSize};
pub use event::{ClipboardResponder, ColorQueryResponder, TermEvent};
pub use params::TermParams;
pub use snapshot::{
    Cell, CellFlags, CellText, Cursor, CursorShape, GridSnapshot, MouseReporting, SelectionSpan,
    TermModes,
};
