// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.move_to_group]` -- RF-2.20, ADR-0023. Popover de grupo de
//! destino do `tab.move_to_group`, única lista rolável do chrome. Classe
//! de recarga A.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct MoveToGroup {
    /// = `context_menu.width`.
    pub width: i32,
    /// = `context_menu.item_height`.
    pub row_height: i32,
    pub row_padding_x: i32,
    /// Teto de linhas visíveis de uma vez; acima disso a lista rola.
    pub max_visible_rows: i32,
    pub swatch_size: i32,
    pub swatch_gap: i32,
}

impl Default for MoveToGroup {
    fn default() -> Self {
        Self {
            width: 200,
            row_height: 28,
            row_padding_x: 8,
            max_visible_rows: 6,
            swatch_size: 8,
            swatch_gap: 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(MoveToGroup::default().max_visible_rows, 6);
    }
}
