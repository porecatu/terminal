// SPDX-License-Identifier: GPL-3.0-or-later

//! `[[themes]]` -- PRD-005 RF-5.18 a RF-5.21, ADR-0031.
//!
//! Nesta etapa: carregar e validar nome duplicado (erro) e escopo (chave
//! fora do escopo de cor é aviso de chave desconhecida, coletado pelo
//! chamador em `crate::load`). O **merge** por folha contra a config ativa
//! é a etapa 6 (ADR-0031) -- aqui cada tema fica como uma árvore
//! independente de overrides `Option`, nunca aplicada.
//!
//! Um tema declara cor, e só cor (ADR-0031): nunca fonte, dimensão, raio,
//! espaçamento ou tempo. É por isso que as duas listas abaixo -- campos de
//! chrome e de terminal -- são a superfície fechada; qualquer outra chave
//! sob `[[themes]]` cai como desconhecida.

use serde::Deserialize;

use crate::color::Color;

/// Override parcial das 16 cores ANSI de um tema (RF-5.11), cada canal
/// opcional -- mesma regra de merge por folha do resto do tema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeAnsiPalette {
    pub black: Option<Color>,
    pub red: Option<Color>,
    pub green: Option<Color>,
    pub yellow: Option<Color>,
    pub blue: Option<Color>,
    pub magenta: Option<Color>,
    pub cyan: Option<Color>,
    pub white: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct Theme {
    pub name: String,

    // --- cores de CHROME, mesmos nomes de [appearance.tabs.colors] -----
    pub bar_background: Option<Color>,
    pub tab_active_background: Option<Color>,
    pub tab_inactive_background: Option<Color>,
    pub tab_active_foreground: Option<Color>,
    pub tab_inactive_foreground: Option<Color>,

    // --- cores de TERMINAL, mesmos nomes de [terminal.colors] -----------
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub cursor: Option<Color>,
    pub cursor_text: Option<Color>,
    pub selection_background: Option<Color>,
    pub selection_foreground: Option<Color>,

    // --- `[themes.normal]` / `[themes.bright]` --------------------------
    pub normal: ThemeAnsiPalette,
    pub bright: ThemeAnsiPalette,
}

/// Nome duplicado entre dois `[[themes]]` é erro: não há como o usuário
/// saber qual dos dois vale.
pub fn find_duplicate_name(themes: &[Theme]) -> Option<&str> {
    for (i, theme) in themes.iter().enumerate() {
        if themes[..i].iter().any(|other| other.name == theme.name) {
            return Some(&theme.name);
        }
    }
    None
}

/// Os dois temas embutidos do arquivo de exemplo, que também são o default
/// de `Config` (regra 1 do ADR-0003: tudo o que o exemplo mostra é
/// default).
pub(crate) fn built_in_themes() -> Vec<Theme> {
    vec![catppuccin_mocha(), gruvbox_dark()]
}

fn catppuccin_mocha() -> Theme {
    Theme {
        name: "catppuccin-mocha".to_owned(),
        bar_background: Some(Color::hex("#181825")),
        tab_active_background: Some(Color::hex("#313244")),
        tab_inactive_background: Some(Color::hex("#1e1e2e")),
        tab_active_foreground: Some(Color::hex("#cdd6f4")),
        tab_inactive_foreground: Some(Color::hex("#a6adc8")),
        foreground: Some(Color::hex("#cdd6f4")),
        background: Some(Color::hex("#1e1e2e")),
        cursor: Some(Color::hex("#f5e0dc")),
        cursor_text: Some(Color::hex("#1e1e2e")),
        selection_background: Some(Color::hex("#585b70")),
        selection_foreground: Some(Color::hex("#cdd6f4")),
        normal: ThemeAnsiPalette {
            black: Some(Color::hex("#45475a")),
            red: Some(Color::hex("#f38ba8")),
            green: Some(Color::hex("#a6e3a1")),
            yellow: Some(Color::hex("#f9e2af")),
            blue: Some(Color::hex("#89b4fa")),
            magenta: Some(Color::hex("#f5c2e7")),
            cyan: Some(Color::hex("#94e2d5")),
            white: Some(Color::hex("#bac2de")),
        },
        bright: ThemeAnsiPalette {
            black: Some(Color::hex("#585b70")),
            red: Some(Color::hex("#f37799")),
            green: Some(Color::hex("#89d88b")),
            yellow: Some(Color::hex("#ebd391")),
            blue: Some(Color::hex("#74a8fc")),
            magenta: Some(Color::hex("#f2aede")),
            cyan: Some(Color::hex("#6bd7ca")),
            white: Some(Color::hex("#a6adc8")),
        },
    }
}

fn gruvbox_dark() -> Theme {
    Theme {
        name: "gruvbox-dark".to_owned(),
        bar_background: None,
        tab_active_background: None,
        tab_inactive_background: None,
        tab_active_foreground: None,
        tab_inactive_foreground: None,
        foreground: Some(Color::hex("#ebdbb2")),
        background: Some(Color::hex("#282828")),
        cursor: Some(Color::hex("#ebdbb2")),
        cursor_text: Some(Color::hex("#282828")),
        selection_background: Some(Color::hex("#504945")),
        selection_foreground: Some(Color::hex("#ebdbb2")),
        normal: ThemeAnsiPalette {
            black: Some(Color::hex("#282828")),
            red: Some(Color::hex("#cc241d")),
            green: Some(Color::hex("#98971a")),
            yellow: Some(Color::hex("#d79921")),
            blue: Some(Color::hex("#458588")),
            magenta: Some(Color::hex("#b16286")),
            cyan: Some(Color::hex("#689d6a")),
            white: Some(Color::hex("#a89984")),
        },
        bright: ThemeAnsiPalette {
            black: Some(Color::hex("#928374")),
            red: Some(Color::hex("#fb4934")),
            green: Some(Color::hex("#b8bb26")),
            yellow: Some(Color::hex("#fabd2f")),
            blue: Some(Color::hex("#83a598")),
            magenta: Some(Color::hex("#d3869b")),
            cyan: Some(Color::hex("#8ec07c")),
            white: Some(Color::hex("#ebdbb2")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_themes_match_example_toml() {
        let themes = built_in_themes();
        assert_eq!(themes.len(), 2);
        assert_eq!(themes[0].name, "catppuccin-mocha");
        assert_eq!(themes[1].name, "gruvbox-dark");
        assert_eq!(themes[1].bar_background, None);
    }

    #[test]
    fn detects_duplicate_name() {
        let themes = vec![
            Theme {
                name: "a".to_owned(),
                ..Theme::default()
            },
            Theme {
                name: "a".to_owned(),
                ..Theme::default()
            },
        ];
        assert_eq!(find_duplicate_name(&themes), Some("a"));
    }

    #[test]
    fn no_duplicate_among_distinct_names() {
        let themes = vec![
            Theme {
                name: "a".to_owned(),
                ..Theme::default()
            },
            Theme {
                name: "b".to_owned(),
                ..Theme::default()
            },
        ];
        assert_eq!(find_duplicate_name(&themes), None);
    }
}
