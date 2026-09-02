// SPDX-License-Identifier: GPL-3.0-or-later

//! `[terminal.*]` -- PRD-005 (aparência do terminal), mais os pedaços de
//! PRD-010/ADR-0013 que vivem sob `[terminal.selection]` e
//! `[terminal.clipboard]`.

mod clipboard;
mod colors;
mod cursor;
mod font;
mod scrollback;
mod selection;

pub use clipboard::Clipboard;
pub use colors::{AnsiPalette, Colors};
pub use cursor::{Cursor, CursorShape};
pub use font::{Font, ZoomScope};
pub use scrollback::Scrollback;
pub use selection::Selection;

use serde::Deserialize;

/// `[terminal]` -- RF-5.15, RF-5.18, RF-5.19. Classe de recarga A.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Terminal {
    /// Nome de um tema definido em `[[themes]]`. Vazio = usa as cores de
    /// `[terminal.colors]` diretamente.
    pub theme: String,
    /// Opacidade do fundo do terminal, independente da opacidade da
    /// janela. RF-5.15.
    pub background_opacity: f64,
    pub font: Font,
    pub cursor: Cursor,
    pub scrollback: Scrollback,
    pub selection: Selection,
    pub clipboard: Clipboard,
    pub colors: Colors,
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            theme: String::new(),
            background_opacity: 1.0,
            font: Font::default(),
            cursor: Cursor::default(),
            scrollback: Scrollback::default(),
            selection: Selection::default(),
            clipboard: Clipboard::default(),
            colors: Colors::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let terminal = Terminal::default();
        assert_eq!(terminal.theme, "");
        assert_eq!(terminal.background_opacity, 1.0);
    }
}
