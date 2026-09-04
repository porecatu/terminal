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
mod dismiss;
mod engine;
mod event;
mod keys;
mod mouse;
mod osc7;
mod params;
mod scroll;
mod search;
mod selection;
mod snapshot;
mod terminal;

pub use color::TermColor;
pub use dismiss::DISMISS_MARKER as SHELL_INTEGRATION_DISMISS_MARKER;
pub use engine::{TermEngine, TermSize};
pub use event::{ClipboardResponder, ColorQueryResponder, TermEvent};
pub use keys::{Modifiers, TermKey, encode_ctrl_char, encode_key, encode_text, wrap_paste};
pub use mouse::{MouseAction, MouseButton, encode_mouse_report};
pub use params::TermParams;
pub use scroll::TermScroll;
pub use search::{
    DEFAULT_SEARCH_LINES_PER_STEP, GridPos, InvalidPattern, Occurrence, SearchJob, SearchMode,
    SearchStep,
};
pub use selection::{SelectionKind, SelectionSide};
pub use snapshot::{
    Cell, CellFlags, CellText, Cursor, CursorShape, GridSnapshot, MouseReporting, OccurrenceSpan,
    SelectionSpan, TermModes,
};
pub use terminal::{ShutdownWait, Terminal, TerminalSpawnError};

// `porecatu-ui` monta `SpawnConfig` para chamar `Terminal::spawn`, mas não
// pode depender de `porecatu-pty` diretamente (tabela de dependências do
// CLAUDE.md: só `porecatu-term` depende de `pty`). Re-exportar aqui é o
// único caminho permitido para esses tipos chegarem em `ui`.
//
// `resolve_default_shell`/`search_path` entram na Etapa 4: `ui` precisa do
// mesmo nome de shell que `porecatu_pty::spawn` vai de fato spawnar
// (`SpawnConfig.program = None` cai nessa mesma resolução), pra popular o
// `shell_name` do `Tab` -- último nível da precedência de título do RF-1.7.
pub use porecatu_pty::{
    ProcessGroup, PtyError, PtySize, SpawnConfig, resolve_default_shell, search_path,
};
