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
mod keys;
mod params;
mod scroll;
mod snapshot;
mod terminal;

pub use color::TermColor;
pub use engine::{TermEngine, TermSize};
pub use event::{ClipboardResponder, ColorQueryResponder, TermEvent};
pub use keys::{Modifiers, TermKey, encode_ctrl_char, encode_key, encode_text, wrap_paste};
pub use params::TermParams;
pub use scroll::TermScroll;
pub use snapshot::{
    Cell, CellFlags, CellText, Cursor, CursorShape, GridSnapshot, MouseReporting, SelectionSpan,
    TermModes,
};
pub use terminal::{Terminal, TerminalSpawnError};

// `porecatu-ui` monta `SpawnConfig` para chamar `Terminal::spawn`, mas não
// pode depender de `porecatu-pty` diretamente (tabela de dependências do
// CLAUDE.md: só `porecatu-term` depende de `pty`). Re-exportar aqui é o
// único caminho permitido para esses tipos chegarem em `ui`.
pub use porecatu_pty::{PtyError, PtySize, SpawnConfig};
