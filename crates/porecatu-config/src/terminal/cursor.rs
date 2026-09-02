// SPDX-License-Identifier: GPL-3.0-or-later

//! `[terminal.cursor]` -- RF-5.22 a RF-5.25. Classe de recarga A.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CursorShape {
    #[default]
    Block,
    Beam,
    Underline,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Cursor {
    pub shape: CursorShape,
    /// px lógicos quando `shape` é `beam` ou `block`.
    pub width: i32,
    /// No design, cursor e prompt assumem a cor do grupo da aba.
    pub follows_group_color: bool,
    pub blink: bool,
    pub blink_interval_ms: u64,
    /// Cursor vazado quando a janela perde foco. RF-5.24.
    pub unfocused_hollow: bool,
    // DECSCUSR emitido pelo programa tem precedência sobre `shape`
    // (RF-5.25) -- comportamento do consumidor, não uma chave.
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            shape: CursorShape::Block,
            width: 7,
            follows_group_color: true,
            blink: false,
            blink_interval_ms: 500,
            unfocused_hollow: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let cursor = Cursor::default();
        assert_eq!(cursor.shape, CursorShape::Block);
        assert_eq!(cursor.width, 7);
        assert!(!cursor.blink);
    }
}
