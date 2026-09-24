// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.session_picker]` -- RF-14.1 a RF-14.18, ADR-0055 §2.
//! Popover de sessões nomeadas, sétimo widget de chrome. Só geometria --
//! cores e tipografia vêm de `[appearance.context_menu]`, o campo de nome
//! de `[appearance.group_editor]`, o mesmo padrão de
//! `[appearance.move_to_group]`. Classe de recarga A.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct SessionPicker {
    /// Largura fixa -- o teto do menu de contexto e do aviso (ADR-0055
    /// §2), não a largura mínima de `context_menu.width`.
    pub width: i32,
    /// = `context_menu.item_height`, para as linhas da lista.
    pub row_height: i32,
    pub row_padding_x: i32,
    /// Teto de linhas visíveis de uma vez; acima disso a lista rola.
    pub max_visible_rows: i32,
}

impl Default for SessionPicker {
    fn default() -> Self {
        Self {
            width: 320,
            row_height: 28,
            row_padding_x: 8,
            max_visible_rows: 6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(SessionPicker::default().width, 320);
        assert_eq!(SessionPicker::default().max_visible_rows, 6);
    }
}
