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
//!
//! **Cobertura parcial, registrada aqui de propósito.** O ADR-0031 §1 diz
//! que um tema aceita cor de `[terminal.colors]`, `[appearance.tabs.colors]`,
//! `[appearance.groups]` (incl. `palette`/`ungrouped_color`) e dos cinco
//! widgets de chrome. Esta etapa (F4 etapa 6) só implementa o merge para os
//! campos que `Theme` já tinha desde a etapa 1: as 16 cores ANSI e os dez
//! campos nomeados abaixo. Estender `Theme` para o resto da superfície do
//! ADR-0031 (grupos, widgets) é trabalho novo, não fechamento desta etapa
//! -- relatado ao usuário, não inventado aqui.

use serde::Deserialize;

use crate::Config;

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

/// `(nome, valor atual, default embutido, override do tema)` de uma chave
/// mesclável (ADR-0031 §2). `apply` e `overridden_keys` -- o "aviso quando
/// há cor declarada fora do tema" que o ADR-0031 pede -- andam sobre a
/// mesma lista, então as duas nunca divergem sobre quais chaves existem.
fn mergeable_fields(
    config: &Config,
    theme: &Theme,
) -> Vec<(&'static str, Color, Color, Option<Color>)> {
    let td = crate::terminal::Colors::default();
    let c = &config.terminal.colors;
    let bd = crate::appearance::TabsColors::default();
    let b = &config.appearance.tabs.colors;
    // `AnsiPalette::default()` sozinho devolve os valores de `normal`
    // (comentário em `terminal/colors.rs`: existe só pra satisfazer
    // `#[serde(default)]`, `Colors::default()` é quem monta os dois
    // corretamente) -- usar `td.normal`/`td.bright` evita comparar o
    // brilhante contra o default errado.
    let ansi_normal_default = td.normal;
    let ansi_bright_default = td.bright;

    let mut fields = vec![
        (
            "terminal.colors.foreground",
            c.foreground,
            td.foreground,
            theme.foreground,
        ),
        (
            "terminal.colors.background",
            c.background,
            td.background,
            theme.background,
        ),
        ("terminal.colors.cursor", c.cursor, td.cursor, theme.cursor),
        (
            "terminal.colors.cursor_text",
            c.cursor_text,
            td.cursor_text,
            theme.cursor_text,
        ),
        (
            "terminal.colors.selection_background",
            c.selection_background,
            td.selection_background,
            theme.selection_background,
        ),
        (
            "terminal.colors.selection_foreground",
            c.selection_foreground,
            td.selection_foreground,
            theme.selection_foreground,
        ),
        (
            "appearance.tabs.colors.bar_background",
            b.bar_background,
            bd.bar_background,
            theme.bar_background,
        ),
        (
            "appearance.tabs.colors.active_background",
            b.active_background,
            bd.active_background,
            theme.tab_active_background,
        ),
        (
            "appearance.tabs.colors.inactive_background",
            b.inactive_background,
            bd.inactive_background,
            theme.tab_inactive_background,
        ),
        (
            "appearance.tabs.colors.active_foreground",
            b.active_foreground,
            bd.active_foreground,
            theme.tab_active_foreground,
        ),
        (
            "appearance.tabs.colors.inactive_foreground",
            b.inactive_foreground,
            bd.inactive_foreground,
            theme.tab_inactive_foreground,
        ),
    ];

    for i in 0..8 {
        fields.push((
            ANSI_NORMAL_NAMES[i],
            ansi_component(&c.normal, i),
            ansi_component(&ansi_normal_default, i),
            ansi_component_opt(&theme.normal, i),
        ));
        fields.push((
            ANSI_BRIGHT_NAMES[i],
            ansi_component(&c.bright, i),
            ansi_component(&ansi_bright_default, i),
            ansi_component_opt(&theme.bright, i),
        ));
    }
    fields
}

const ANSI_NORMAL_NAMES: [&str; 8] = [
    "terminal.colors.normal.black",
    "terminal.colors.normal.red",
    "terminal.colors.normal.green",
    "terminal.colors.normal.yellow",
    "terminal.colors.normal.blue",
    "terminal.colors.normal.magenta",
    "terminal.colors.normal.cyan",
    "terminal.colors.normal.white",
];
const ANSI_BRIGHT_NAMES: [&str; 8] = [
    "terminal.colors.bright.black",
    "terminal.colors.bright.red",
    "terminal.colors.bright.green",
    "terminal.colors.bright.yellow",
    "terminal.colors.bright.blue",
    "terminal.colors.bright.magenta",
    "terminal.colors.bright.cyan",
    "terminal.colors.bright.white",
];

fn ansi_component(p: &crate::terminal::AnsiPalette, i: usize) -> Color {
    [
        p.black, p.red, p.green, p.yellow, p.blue, p.magenta, p.cyan, p.white,
    ][i]
}

fn ansi_component_opt(p: &ThemeAnsiPalette, i: usize) -> Option<Color> {
    [
        p.black, p.red, p.green, p.yellow, p.blue, p.magenta, p.cyan, p.white,
    ][i]
}

/// ADR-0031 §2: "chave declarada fora do tema > chave do tema > default
/// embutido". Sem rastro de "o usuário escreveu isto" (o
/// `#[serde(default)]` apaga essa distinção na desserialização), a
/// aproximação é "diferente do default": um valor igual ao default,
/// declarado ou não, não muda nada visualmente de qualquer jeito.
fn merged_color(current: Color, default: Color, theme_value: Option<Color>) -> Color {
    if current != default {
        current
    } else {
        theme_value.unwrap_or(current)
    }
}

fn find<'a>(themes: &'a [Theme], name: &str) -> Option<&'a Theme> {
    if name.is_empty() {
        return None;
    }
    themes.iter().find(|t| t.name == name)
}

/// Aplica o tema `name` sobre `config`, seguindo a precedência do
/// ADR-0031 §2. `name` vazio ou desconhecido devolve `config` inalterado
/// -- nome desconhecido é aviso do chamador (ADR-0031 §5), não erro aqui.
pub fn apply(config: &Config, name: &str) -> Config {
    let Some(theme) = find(&config.themes, name) else {
        return config.clone();
    };
    let merged: Vec<Color> = mergeable_fields(config, theme)
        .into_iter()
        .map(|(_, current, default, theme_value)| merged_color(current, default, theme_value))
        .collect();

    let mut out = config.clone();
    out.terminal.colors.foreground = merged[0];
    out.terminal.colors.background = merged[1];
    out.terminal.colors.cursor = merged[2];
    out.terminal.colors.cursor_text = merged[3];
    out.terminal.colors.selection_background = merged[4];
    out.terminal.colors.selection_foreground = merged[5];
    out.appearance.tabs.colors.bar_background = merged[6];
    out.appearance.tabs.colors.active_background = merged[7];
    out.appearance.tabs.colors.inactive_background = merged[8];
    out.appearance.tabs.colors.active_foreground = merged[9];
    out.appearance.tabs.colors.inactive_foreground = merged[10];
    let ansi = &merged[11..27];
    out.terminal.colors.normal = crate::terminal::AnsiPalette {
        black: ansi[0],
        red: ansi[2],
        green: ansi[4],
        yellow: ansi[6],
        blue: ansi[8],
        magenta: ansi[10],
        cyan: ansi[12],
        white: ansi[14],
    };
    out.terminal.colors.bright = crate::terminal::AnsiPalette {
        black: ansi[1],
        red: ansi[3],
        green: ansi[5],
        yellow: ansi[7],
        blue: ansi[9],
        magenta: ansi[11],
        cyan: ansi[13],
        white: ansi[15],
    };
    out
}

/// As chaves que "vencem" o tema `name` -- declaradas fora dele E que o
/// tema também define (ADR-0031 §1: "aviso quando há cor declarada fora
/// do tema ao trocar de tema, listando quais chaves estão vencendo").
/// Vazio se `name` é vazio, desconhecido, ou nenhuma chave colide.
pub fn overridden_keys(config: &Config, name: &str) -> Vec<&'static str> {
    let Some(theme) = find(&config.themes, name) else {
        return Vec::new();
    };
    mergeable_fields(config, theme)
        .into_iter()
        .filter(|(_, current, default, theme_value)| current != default && theme_value.is_some())
        .map(|(name, ..)| name)
        .collect()
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

    fn theme_with_colors() -> Theme {
        Theme {
            name: "t".to_owned(),
            foreground: Some(Color::hex("#111111")),
            bar_background: Some(Color::hex("#222222")),
            normal: ThemeAnsiPalette {
                red: Some(Color::hex("#aa0000")),
                ..ThemeAnsiPalette::default()
            },
            bright: ThemeAnsiPalette {
                red: Some(Color::hex("#bb0000")),
                ..ThemeAnsiPalette::default()
            },
            ..Theme::default()
        }
    }

    #[test]
    fn apply_unknown_or_empty_name_is_a_noop() {
        let config = Config {
            themes: vec![theme_with_colors()],
            ..Config::default()
        };
        assert_eq!(apply(&config, ""), config);
        assert_eq!(apply(&config, "nao-existe"), config);
    }

    #[test]
    fn apply_uses_theme_value_when_config_is_at_default() {
        let config = Config {
            themes: vec![theme_with_colors()],
            ..Config::default()
        };
        let themed = apply(&config, "t");
        assert_eq!(themed.terminal.colors.foreground, Color::hex("#111111"));
        assert_eq!(
            themed.appearance.tabs.colors.bar_background,
            Color::hex("#222222")
        );
        assert_eq!(themed.terminal.colors.normal.red, Color::hex("#aa0000"));
        assert_eq!(themed.terminal.colors.bright.red, Color::hex("#bb0000"));
        // Campos que o tema não declara continuam no default.
        assert_eq!(
            themed.terminal.colors.background,
            Config::default().terminal.colors.background
        );
        assert_eq!(
            themed.terminal.colors.normal.green,
            Config::default().terminal.colors.normal.green
        );
    }

    #[test]
    fn apply_key_declared_outside_theme_wins() {
        let mut config = Config {
            themes: vec![theme_with_colors()],
            ..Config::default()
        };
        config.terminal.colors.foreground = Color::hex("#ffffff");
        config.terminal.colors.normal.red = Color::hex("#ff00ff");
        let themed = apply(&config, "t");
        assert_eq!(themed.terminal.colors.foreground, Color::hex("#ffffff"));
        assert_eq!(themed.terminal.colors.normal.red, Color::hex("#ff00ff"));
        // O que o usuário não tocou continua vindo do tema.
        assert_eq!(themed.terminal.colors.bright.red, Color::hex("#bb0000"));
    }

    #[test]
    fn overridden_keys_lists_only_colliding_keys() {
        let mut config = Config {
            themes: vec![theme_with_colors()],
            ..Config::default()
        };
        config.terminal.colors.foreground = Color::hex("#ffffff");
        let keys = overridden_keys(&config, "t");
        assert_eq!(keys, vec!["terminal.colors.foreground"]);
    }

    #[test]
    fn overridden_keys_empty_for_unknown_theme() {
        let config = Config::default();
        assert!(overridden_keys(&config, "nao-existe").is_empty());
    }
}
