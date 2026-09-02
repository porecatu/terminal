// SPDX-License-Identifier: GPL-3.0-or-later

//! `[keybindings]` -- ADR-0008 (semântica), ADR-0029 (gramática, ainda não
//! implementada aqui). Nesta etapa é só um mapa de string para string,
//! preservado: o enum `Action`, o parser de tecla e a validação contra o
//! catálogo (`docs/reference/acoes.md`) são a etapa 5.
//!
//! `[keybindings]` é a tabela comum (`common`, abaixo, via
//! `#[serde(flatten)]`); `[keybindings.windows]`, `[keybindings.linux]` e
//! `[keybindings.macos]` sobrescrevem por plataforma. Resolver a tabela
//! efetiva de uma plataforma (defaults embutidos -> comum -> da
//! plataforma) também é etapa 5 -- aqui as quatro tabelas só são
//! carregadas, sem merge.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Keybindings {
    #[serde(flatten)]
    pub common: BTreeMap<String, String>,
    pub windows: BTreeMap<String, String>,
    pub linux: BTreeMap<String, String>,
    pub macos: BTreeMap<String, String>,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            common: common_defaults(),
            windows: BTreeMap::new(),
            linux: BTreeMap::new(),
            macos: macos_defaults(),
        }
    }
}

fn map(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, action)| (key.to_string(), action.to_string()))
        .collect()
}

fn common_defaults() -> BTreeMap<String, String> {
    map(&[
        ("ctrl+shift+t", "tab.new"),
        ("ctrl+shift+w", "tab.close"),
        ("ctrl+tab", "tab.next"),
        ("ctrl+shift+tab", "tab.prev"),
        ("alt+1", "tab.goto_1"),
        ("alt+9", "tab.goto_9"),
        ("ctrl+shift+r", "tab.rename"),
        ("ctrl+shift+left", "tab.move_left"),
        ("ctrl+shift+right", "tab.move_right"),
        ("ctrl+shift+g", "group.create"),
        ("ctrl+shift+u", "group.dissolve"),
        ("ctrl+shift+e", "group.rename"),
        ("ctrl+shift+k", "group.toggle_collapse"),
        ("ctrl+shift+pagedown", "group.next"),
        ("ctrl+shift+pageup", "group.prev"),
        ("ctrl+shift+n", "window.new"),
        ("ctrl+shift+q", "window.close"),
        ("ctrl+shift+c", "clipboard.copy"),
        ("ctrl+shift+v", "clipboard.paste"),
        ("shift+pageup", "scrollback.page_up"),
        ("shift+pagedown", "scrollback.page_down"),
        ("shift+home", "scrollback.to_top"),
        ("shift+end", "scrollback.to_bottom"),
        ("ctrl+equals", "font.increase"),
        ("ctrl+minus", "font.decrease"),
        ("ctrl+0", "font.reset"),
        ("ctrl+shift+y", "theme.cycle"),
        ("ctrl+shift+comma", "config.reload"),
        ("ctrl+shift+p", "command.palette"),
    ])
}

fn macos_defaults() -> BTreeMap<String, String> {
    map(&[
        ("cmd+t", "tab.new"),
        ("cmd+w", "tab.close"),
        ("ctrl+tab", "tab.next"),
        ("ctrl+shift+tab", "tab.prev"),
        ("cmd+1", "tab.goto_1"),
        ("cmd+9", "tab.goto_9"),
        ("cmd+r", "tab.rename"),
        ("cmd+shift+left", "tab.move_left"),
        ("cmd+shift+right", "tab.move_right"),
        ("cmd+g", "group.create"),
        ("cmd+shift+g", "group.dissolve"),
        ("cmd+e", "group.rename"),
        ("cmd+k", "group.toggle_collapse"),
        ("cmd+alt+right", "group.next"),
        ("cmd+alt+left", "group.prev"),
        ("cmd+n", "window.new"),
        ("cmd+shift+w", "window.close"),
        ("cmd+c", "clipboard.copy"),
        ("cmd+v", "clipboard.paste"),
        ("shift+pageup", "scrollback.page_up"),
        ("shift+pagedown", "scrollback.page_down"),
        ("shift+home", "scrollback.to_top"),
        ("shift+end", "scrollback.to_bottom"),
        ("cmd+equals", "font.increase"),
        ("cmd+minus", "font.decrease"),
        ("cmd+0", "font.reset"),
        ("cmd+shift+y", "theme.cycle"),
        ("cmd+comma", "config.reload"),
        ("cmd+q", "app.quit"),
        ("cmd+shift+p", "command.palette"),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let bindings = Keybindings::default();
        assert_eq!(bindings.common.len(), 29);
        assert_eq!(
            bindings.common.get("ctrl+shift+t"),
            Some(&"tab.new".to_owned())
        );
        assert_eq!(bindings.macos.len(), 30);
        assert_eq!(bindings.macos.get("cmd+q"), Some(&"app.quit".to_owned()));
        assert!(bindings.windows.is_empty());
        assert!(bindings.linux.is_empty());
    }
}
