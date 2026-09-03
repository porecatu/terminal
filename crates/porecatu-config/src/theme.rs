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
//! espaçamento ou tempo. É por isso que as listas abaixo -- campos de
//! chrome, terminal, grupos e dos cinco widgets -- são a superfície
//! fechada; qualquer outra chave sob `[[themes]]` cai como desconhecida.
//!
//! O merge cobre toda a superfície de cor do ADR-0031 §1: as 16 ANSI, os
//! dez campos nomeados de chrome/terminal, `[appearance.groups]` (incl.
//! `palette`/`ungrouped_color`, com a exceção de substituição inteira do
//! ADR-0031 §2) e as cores dos cinco widgets de chrome.

use serde::Deserialize;

use crate::Config;

use crate::appearance::GroupPaletteEntry;
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

/// `[themes.groups]` -- override de `[appearance.groups]` (ADR-0031 §1).
/// `palette` é a exceção de merge do ADR-0031 §2: substituída inteira,
/// nunca mesclada por índice ou nome de entrada.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeGroups {
    pub count_background: Option<Color>,
    pub count_foreground: Option<Color>,
    pub glass_border: Option<Color>,
    pub border: Option<Color>,
    pub ungrouped_color: Option<Color>,
    pub palette: Option<Vec<GroupPaletteEntry>>,
}

/// `[themes.notices]` -- override de `[appearance.notices]`.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeNotices {
    pub background: Option<Color>,
    pub border: Option<Color>,
    pub foreground: Option<Color>,
    pub body_foreground: Option<Color>,
    pub error: Option<Color>,
    pub warning: Option<Color>,
    pub info: Option<Color>,
}

/// `[themes.dialog]` -- override de `[appearance.dialog]`.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeDialog {
    pub overlay: Option<Color>,
    pub background: Option<Color>,
    pub border: Option<Color>,
    pub title_foreground: Option<Color>,
    pub foreground: Option<Color>,
    pub button_border: Option<Color>,
    pub destructive_foreground: Option<Color>,
    pub destructive_hover_background: Option<Color>,
}

/// `[themes.context_menu]` -- override de `[appearance.context_menu]`.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeContextMenu {
    pub background: Option<Color>,
    pub border: Option<Color>,
    pub item_hover_background: Option<Color>,
    pub separator: Option<Color>,
    pub foreground: Option<Color>,
    pub disabled_foreground: Option<Color>,
    pub destructive_foreground: Option<Color>,
}

/// `[themes.tooltip]` -- override de `[appearance.tooltip]`.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeTooltip {
    pub background: Option<Color>,
    pub border: Option<Color>,
    pub foreground: Option<Color>,
}

/// `[themes.group_editor]` -- override de `[appearance.group_editor]`.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeGroupEditor {
    pub background: Option<Color>,
    pub border: Option<Color>,
    pub section_foreground: Option<Color>,
    pub input_background: Option<Color>,
    pub input_border: Option<Color>,
    pub input_border_focus: Option<Color>,
    pub input_foreground: Option<Color>,
    pub swatch_ring_selected: Option<Color>,
    pub item_foreground: Option<Color>,
    pub item_hover_background: Option<Color>,
    pub divider: Option<Color>,
    pub destructive_foreground: Option<Color>,
    pub destructive_hover_background: Option<Color>,
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

    // --- `[themes.groups]` e os cinco widgets de chrome -----------------
    pub groups: ThemeGroups,
    pub notices: ThemeNotices,
    pub dialog: ThemeDialog,
    pub context_menu: ThemeContextMenu,
    pub tooltip: ThemeTooltip,
    pub group_editor: ThemeGroupEditor,
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
        ..Theme::default()
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
        ..Theme::default()
    }
}

/// Uma chave mesclável (ADR-0031 §2): nome (chave TOML de origem), valor
/// atual, default embutido, override do tema, e o setter que escreve o
/// valor mesclado de volta no `Config`. `apply` e `overridden_keys` -- o
/// "aviso quando há cor declarada fora do tema" que o ADR-0031 pede --
/// andam sobre a mesma lista, então as duas nunca divergem sobre quais
/// chaves existem. `set` é um ponteiro de função, não `Box<dyn FnOnce>`:
/// todo fechamento aqui é não-capturante, então não há alocação.
struct MergeField {
    name: &'static str,
    current: Color,
    default: Color,
    theme_value: Option<Color>,
    set: fn(&mut Config, Color),
}

fn mergeable_fields(config: &Config, theme: &Theme) -> Vec<MergeField> {
    let td = crate::terminal::Colors::default();
    let c = &config.terminal.colors;
    let bd = crate::appearance::TabsColors::default();
    let b = &config.appearance.tabs.colors;
    let gd = crate::appearance::Groups::default();
    let g = &config.appearance.groups;
    let nd = crate::appearance::Notices::default();
    let n = &config.appearance.notices;
    let dlgd = crate::appearance::Dialog::default();
    let dlg = &config.appearance.dialog;
    let cmd = crate::appearance::ContextMenu::default();
    let cm = &config.appearance.context_menu;
    let ttd = crate::appearance::Tooltip::default();
    let tt = &config.appearance.tooltip;
    let ged = crate::appearance::GroupEditor::default();
    let ge = &config.appearance.group_editor;
    // `AnsiPalette::default()` sozinho devolve os valores de `normal`
    // (comentário em `terminal/colors.rs`: existe só pra satisfazer
    // `#[serde(default)]`, `Colors::default()` é quem monta os dois
    // corretamente) -- usar `td.normal`/`td.bright` evita comparar o
    // brilhante contra o default errado.
    let ansi_normal_default = td.normal;
    let ansi_bright_default = td.bright;

    let mut fields = vec![
        MergeField {
            name: "terminal.colors.foreground",
            current: c.foreground,
            default: td.foreground,
            theme_value: theme.foreground,
            set: |cfg, v| cfg.terminal.colors.foreground = v,
        },
        MergeField {
            name: "terminal.colors.background",
            current: c.background,
            default: td.background,
            theme_value: theme.background,
            set: |cfg, v| cfg.terminal.colors.background = v,
        },
        MergeField {
            name: "terminal.colors.cursor",
            current: c.cursor,
            default: td.cursor,
            theme_value: theme.cursor,
            set: |cfg, v| cfg.terminal.colors.cursor = v,
        },
        MergeField {
            name: "terminal.colors.cursor_text",
            current: c.cursor_text,
            default: td.cursor_text,
            theme_value: theme.cursor_text,
            set: |cfg, v| cfg.terminal.colors.cursor_text = v,
        },
        MergeField {
            name: "terminal.colors.selection_background",
            current: c.selection_background,
            default: td.selection_background,
            theme_value: theme.selection_background,
            set: |cfg, v| cfg.terminal.colors.selection_background = v,
        },
        MergeField {
            name: "terminal.colors.selection_foreground",
            current: c.selection_foreground,
            default: td.selection_foreground,
            theme_value: theme.selection_foreground,
            set: |cfg, v| cfg.terminal.colors.selection_foreground = v,
        },
        MergeField {
            name: "appearance.tabs.colors.bar_background",
            current: b.bar_background,
            default: bd.bar_background,
            theme_value: theme.bar_background,
            set: |cfg, v| cfg.appearance.tabs.colors.bar_background = v,
        },
        MergeField {
            name: "appearance.tabs.colors.active_background",
            current: b.active_background,
            default: bd.active_background,
            theme_value: theme.tab_active_background,
            set: |cfg, v| cfg.appearance.tabs.colors.active_background = v,
        },
        MergeField {
            name: "appearance.tabs.colors.inactive_background",
            current: b.inactive_background,
            default: bd.inactive_background,
            theme_value: theme.tab_inactive_background,
            set: |cfg, v| cfg.appearance.tabs.colors.inactive_background = v,
        },
        MergeField {
            name: "appearance.tabs.colors.active_foreground",
            current: b.active_foreground,
            default: bd.active_foreground,
            theme_value: theme.tab_active_foreground,
            set: |cfg, v| cfg.appearance.tabs.colors.active_foreground = v,
        },
        MergeField {
            name: "appearance.tabs.colors.inactive_foreground",
            current: b.inactive_foreground,
            default: bd.inactive_foreground,
            theme_value: theme.tab_inactive_foreground,
            set: |cfg, v| cfg.appearance.tabs.colors.inactive_foreground = v,
        },
        MergeField {
            name: "appearance.groups.count_background",
            current: g.count_background,
            default: gd.count_background,
            theme_value: theme.groups.count_background,
            set: |cfg, v| cfg.appearance.groups.count_background = v,
        },
        MergeField {
            name: "appearance.groups.count_foreground",
            current: g.count_foreground,
            default: gd.count_foreground,
            theme_value: theme.groups.count_foreground,
            set: |cfg, v| cfg.appearance.groups.count_foreground = v,
        },
        MergeField {
            name: "appearance.groups.glass_border",
            current: g.glass_border,
            default: gd.glass_border,
            theme_value: theme.groups.glass_border,
            set: |cfg, v| cfg.appearance.groups.glass_border = v,
        },
        MergeField {
            name: "appearance.groups.border",
            current: g.border,
            default: gd.border,
            theme_value: theme.groups.border,
            set: |cfg, v| cfg.appearance.groups.border = v,
        },
        MergeField {
            name: "appearance.groups.ungrouped_color",
            current: g.ungrouped_color,
            default: gd.ungrouped_color,
            theme_value: theme.groups.ungrouped_color,
            set: |cfg, v| cfg.appearance.groups.ungrouped_color = v,
        },
        MergeField {
            name: "appearance.notices.background",
            current: n.background,
            default: nd.background,
            theme_value: theme.notices.background,
            set: |cfg, v| cfg.appearance.notices.background = v,
        },
        MergeField {
            name: "appearance.notices.border",
            current: n.border,
            default: nd.border,
            theme_value: theme.notices.border,
            set: |cfg, v| cfg.appearance.notices.border = v,
        },
        MergeField {
            name: "appearance.notices.foreground",
            current: n.foreground,
            default: nd.foreground,
            theme_value: theme.notices.foreground,
            set: |cfg, v| cfg.appearance.notices.foreground = v,
        },
        MergeField {
            name: "appearance.notices.body_foreground",
            current: n.body_foreground,
            default: nd.body_foreground,
            theme_value: theme.notices.body_foreground,
            set: |cfg, v| cfg.appearance.notices.body_foreground = v,
        },
        MergeField {
            name: "appearance.notices.error",
            current: n.error,
            default: nd.error,
            theme_value: theme.notices.error,
            set: |cfg, v| cfg.appearance.notices.error = v,
        },
        MergeField {
            name: "appearance.notices.warning",
            current: n.warning,
            default: nd.warning,
            theme_value: theme.notices.warning,
            set: |cfg, v| cfg.appearance.notices.warning = v,
        },
        MergeField {
            name: "appearance.notices.info",
            current: n.info,
            default: nd.info,
            theme_value: theme.notices.info,
            set: |cfg, v| cfg.appearance.notices.info = v,
        },
        MergeField {
            name: "appearance.dialog.overlay",
            current: dlg.overlay,
            default: dlgd.overlay,
            theme_value: theme.dialog.overlay,
            set: |cfg, v| cfg.appearance.dialog.overlay = v,
        },
        MergeField {
            name: "appearance.dialog.background",
            current: dlg.background,
            default: dlgd.background,
            theme_value: theme.dialog.background,
            set: |cfg, v| cfg.appearance.dialog.background = v,
        },
        MergeField {
            name: "appearance.dialog.border",
            current: dlg.border,
            default: dlgd.border,
            theme_value: theme.dialog.border,
            set: |cfg, v| cfg.appearance.dialog.border = v,
        },
        MergeField {
            name: "appearance.dialog.title_foreground",
            current: dlg.title_foreground,
            default: dlgd.title_foreground,
            theme_value: theme.dialog.title_foreground,
            set: |cfg, v| cfg.appearance.dialog.title_foreground = v,
        },
        MergeField {
            name: "appearance.dialog.foreground",
            current: dlg.foreground,
            default: dlgd.foreground,
            theme_value: theme.dialog.foreground,
            set: |cfg, v| cfg.appearance.dialog.foreground = v,
        },
        MergeField {
            name: "appearance.dialog.button_border",
            current: dlg.button_border,
            default: dlgd.button_border,
            theme_value: theme.dialog.button_border,
            set: |cfg, v| cfg.appearance.dialog.button_border = v,
        },
        MergeField {
            name: "appearance.dialog.destructive_foreground",
            current: dlg.destructive_foreground,
            default: dlgd.destructive_foreground,
            theme_value: theme.dialog.destructive_foreground,
            set: |cfg, v| cfg.appearance.dialog.destructive_foreground = v,
        },
        MergeField {
            name: "appearance.dialog.destructive_hover_background",
            current: dlg.destructive_hover_background,
            default: dlgd.destructive_hover_background,
            theme_value: theme.dialog.destructive_hover_background,
            set: |cfg, v| cfg.appearance.dialog.destructive_hover_background = v,
        },
        MergeField {
            name: "appearance.context_menu.background",
            current: cm.background,
            default: cmd.background,
            theme_value: theme.context_menu.background,
            set: |cfg, v| cfg.appearance.context_menu.background = v,
        },
        MergeField {
            name: "appearance.context_menu.border",
            current: cm.border,
            default: cmd.border,
            theme_value: theme.context_menu.border,
            set: |cfg, v| cfg.appearance.context_menu.border = v,
        },
        MergeField {
            name: "appearance.context_menu.item_hover_background",
            current: cm.item_hover_background,
            default: cmd.item_hover_background,
            theme_value: theme.context_menu.item_hover_background,
            set: |cfg, v| cfg.appearance.context_menu.item_hover_background = v,
        },
        MergeField {
            name: "appearance.context_menu.separator",
            current: cm.separator,
            default: cmd.separator,
            theme_value: theme.context_menu.separator,
            set: |cfg, v| cfg.appearance.context_menu.separator = v,
        },
        MergeField {
            name: "appearance.context_menu.foreground",
            current: cm.foreground,
            default: cmd.foreground,
            theme_value: theme.context_menu.foreground,
            set: |cfg, v| cfg.appearance.context_menu.foreground = v,
        },
        MergeField {
            name: "appearance.context_menu.disabled_foreground",
            current: cm.disabled_foreground,
            default: cmd.disabled_foreground,
            theme_value: theme.context_menu.disabled_foreground,
            set: |cfg, v| cfg.appearance.context_menu.disabled_foreground = v,
        },
        MergeField {
            name: "appearance.context_menu.destructive_foreground",
            current: cm.destructive_foreground,
            default: cmd.destructive_foreground,
            theme_value: theme.context_menu.destructive_foreground,
            set: |cfg, v| cfg.appearance.context_menu.destructive_foreground = v,
        },
        MergeField {
            name: "appearance.tooltip.background",
            current: tt.background,
            default: ttd.background,
            theme_value: theme.tooltip.background,
            set: |cfg, v| cfg.appearance.tooltip.background = v,
        },
        MergeField {
            name: "appearance.tooltip.border",
            current: tt.border,
            default: ttd.border,
            theme_value: theme.tooltip.border,
            set: |cfg, v| cfg.appearance.tooltip.border = v,
        },
        MergeField {
            name: "appearance.tooltip.foreground",
            current: tt.foreground,
            default: ttd.foreground,
            theme_value: theme.tooltip.foreground,
            set: |cfg, v| cfg.appearance.tooltip.foreground = v,
        },
        MergeField {
            name: "appearance.group_editor.background",
            current: ge.background,
            default: ged.background,
            theme_value: theme.group_editor.background,
            set: |cfg, v| cfg.appearance.group_editor.background = v,
        },
        MergeField {
            name: "appearance.group_editor.border",
            current: ge.border,
            default: ged.border,
            theme_value: theme.group_editor.border,
            set: |cfg, v| cfg.appearance.group_editor.border = v,
        },
        MergeField {
            name: "appearance.group_editor.section_foreground",
            current: ge.section_foreground,
            default: ged.section_foreground,
            theme_value: theme.group_editor.section_foreground,
            set: |cfg, v| cfg.appearance.group_editor.section_foreground = v,
        },
        MergeField {
            name: "appearance.group_editor.input_background",
            current: ge.input_background,
            default: ged.input_background,
            theme_value: theme.group_editor.input_background,
            set: |cfg, v| cfg.appearance.group_editor.input_background = v,
        },
        MergeField {
            name: "appearance.group_editor.input_border",
            current: ge.input_border,
            default: ged.input_border,
            theme_value: theme.group_editor.input_border,
            set: |cfg, v| cfg.appearance.group_editor.input_border = v,
        },
        MergeField {
            name: "appearance.group_editor.input_border_focus",
            current: ge.input_border_focus,
            default: ged.input_border_focus,
            theme_value: theme.group_editor.input_border_focus,
            set: |cfg, v| cfg.appearance.group_editor.input_border_focus = v,
        },
        MergeField {
            name: "appearance.group_editor.input_foreground",
            current: ge.input_foreground,
            default: ged.input_foreground,
            theme_value: theme.group_editor.input_foreground,
            set: |cfg, v| cfg.appearance.group_editor.input_foreground = v,
        },
        MergeField {
            name: "appearance.group_editor.swatch_ring_selected",
            current: ge.swatch_ring_selected,
            default: ged.swatch_ring_selected,
            theme_value: theme.group_editor.swatch_ring_selected,
            set: |cfg, v| cfg.appearance.group_editor.swatch_ring_selected = v,
        },
        MergeField {
            name: "appearance.group_editor.item_foreground",
            current: ge.item_foreground,
            default: ged.item_foreground,
            theme_value: theme.group_editor.item_foreground,
            set: |cfg, v| cfg.appearance.group_editor.item_foreground = v,
        },
        MergeField {
            name: "appearance.group_editor.item_hover_background",
            current: ge.item_hover_background,
            default: ged.item_hover_background,
            theme_value: theme.group_editor.item_hover_background,
            set: |cfg, v| cfg.appearance.group_editor.item_hover_background = v,
        },
        MergeField {
            name: "appearance.group_editor.divider",
            current: ge.divider,
            default: ged.divider,
            theme_value: theme.group_editor.divider,
            set: |cfg, v| cfg.appearance.group_editor.divider = v,
        },
        MergeField {
            name: "appearance.group_editor.destructive_foreground",
            current: ge.destructive_foreground,
            default: ged.destructive_foreground,
            theme_value: theme.group_editor.destructive_foreground,
            set: |cfg, v| cfg.appearance.group_editor.destructive_foreground = v,
        },
        MergeField {
            name: "appearance.group_editor.destructive_hover_background",
            current: ge.destructive_hover_background,
            default: ged.destructive_hover_background,
            theme_value: theme.group_editor.destructive_hover_background,
            set: |cfg, v| cfg.appearance.group_editor.destructive_hover_background = v,
        },
    ];

    let ansi_normal_setters: [fn(&mut Config, Color); 8] = [
        |cfg, v| cfg.terminal.colors.normal.black = v,
        |cfg, v| cfg.terminal.colors.normal.red = v,
        |cfg, v| cfg.terminal.colors.normal.green = v,
        |cfg, v| cfg.terminal.colors.normal.yellow = v,
        |cfg, v| cfg.terminal.colors.normal.blue = v,
        |cfg, v| cfg.terminal.colors.normal.magenta = v,
        |cfg, v| cfg.terminal.colors.normal.cyan = v,
        |cfg, v| cfg.terminal.colors.normal.white = v,
    ];
    let ansi_bright_setters: [fn(&mut Config, Color); 8] = [
        |cfg, v| cfg.terminal.colors.bright.black = v,
        |cfg, v| cfg.terminal.colors.bright.red = v,
        |cfg, v| cfg.terminal.colors.bright.green = v,
        |cfg, v| cfg.terminal.colors.bright.yellow = v,
        |cfg, v| cfg.terminal.colors.bright.blue = v,
        |cfg, v| cfg.terminal.colors.bright.magenta = v,
        |cfg, v| cfg.terminal.colors.bright.cyan = v,
        |cfg, v| cfg.terminal.colors.bright.white = v,
    ];
    for i in 0..8 {
        fields.push(MergeField {
            name: ANSI_NORMAL_NAMES[i],
            current: ansi_component(&c.normal, i),
            default: ansi_component(&ansi_normal_default, i),
            theme_value: ansi_component_opt(&theme.normal, i),
            set: ansi_normal_setters[i],
        });
        fields.push(MergeField {
            name: ANSI_BRIGHT_NAMES[i],
            current: ansi_component(&c.bright, i),
            default: ansi_component(&ansi_bright_default, i),
            theme_value: ansi_component_opt(&theme.bright, i),
            set: ansi_bright_setters[i],
        });
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
/// declarado ou não, não muda nada visualmente de qualquer jeito. Mesma
/// regra vale para `palette` (ADR-0031 §2), substituída inteira em vez de
/// mesclada por folha -- daí o genérico em vez de um `merged_color` só
/// para `Color`.
fn merged<T: Clone + PartialEq>(current: T, default: T, theme_value: Option<T>) -> T {
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
    let mut out = config.clone();
    for field in mergeable_fields(config, theme) {
        let value = merged(field.current, field.default, field.theme_value);
        (field.set)(&mut out, value);
    }

    // `palette` (ADR-0031 §2): substituída inteira, não cabe no formato
    // escalar de `MergeField`.
    let groups_default = crate::appearance::Groups::default();
    out.appearance.groups.palette = merged(
        config.appearance.groups.palette.clone(),
        groups_default.palette,
        theme.groups.palette.clone(),
    );
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
    let mut keys: Vec<&'static str> = mergeable_fields(config, theme)
        .into_iter()
        .filter(|f| f.current != f.default && f.theme_value.is_some())
        .map(|f| f.name)
        .collect();

    let groups_default = crate::appearance::Groups::default();
    if config.appearance.groups.palette != groups_default.palette && theme.groups.palette.is_some()
    {
        keys.push("appearance.groups.palette");
    }
    keys
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
            groups: ThemeGroups {
                border: Some(Color::hex("#333333")),
                ..ThemeGroups::default()
            },
            notices: ThemeNotices {
                background: Some(Color::hex("#444444")),
                ..ThemeNotices::default()
            },
            dialog: ThemeDialog {
                overlay: Some(Color::hex("#555555")),
                ..ThemeDialog::default()
            },
            context_menu: ThemeContextMenu {
                foreground: Some(Color::hex("#666666")),
                ..ThemeContextMenu::default()
            },
            tooltip: ThemeTooltip {
                foreground: Some(Color::hex("#777777")),
                ..ThemeTooltip::default()
            },
            group_editor: ThemeGroupEditor {
                input_border_focus: Some(Color::hex("#888888")),
                ..ThemeGroupEditor::default()
            },
            ..Theme::default()
        }
    }

    fn theme_with_custom_palette() -> Theme {
        Theme {
            name: "p".to_owned(),
            groups: ThemeGroups {
                palette: Some(vec![
                    GroupPaletteEntry {
                        name: "um".to_owned(),
                        color: Color::hex("#101010"),
                    },
                    GroupPaletteEntry {
                        name: "dois".to_owned(),
                        color: Color::hex("#202020"),
                    },
                ]),
                ..ThemeGroups::default()
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
    fn apply_uses_theme_widget_colors_when_config_is_at_default() {
        let config = Config {
            themes: vec![theme_with_colors()],
            ..Config::default()
        };
        let themed = apply(&config, "t");
        assert_eq!(themed.appearance.groups.border, Color::hex("#333333"));
        assert_eq!(themed.appearance.notices.background, Color::hex("#444444"));
        assert_eq!(themed.appearance.dialog.overlay, Color::hex("#555555"));
        assert_eq!(
            themed.appearance.context_menu.foreground,
            Color::hex("#666666")
        );
        assert_eq!(themed.appearance.tooltip.foreground, Color::hex("#777777"));
        assert_eq!(
            themed.appearance.group_editor.input_border_focus,
            Color::hex("#888888")
        );
        // Campos que o tema não declara continuam no default.
        assert_eq!(
            themed.appearance.groups.count_background,
            Config::default().appearance.groups.count_background
        );
        assert_eq!(
            themed.appearance.dialog.background,
            Config::default().appearance.dialog.background
        );
    }

    #[test]
    fn apply_widget_field_declared_outside_theme_wins() {
        let mut config = Config {
            themes: vec![theme_with_colors()],
            ..Config::default()
        };
        config.appearance.groups.border = Color::hex("#ffffff");
        config.appearance.notices.background = Color::hex("#eeeeee");
        let themed = apply(&config, "t");
        assert_eq!(themed.appearance.groups.border, Color::hex("#ffffff"));
        assert_eq!(themed.appearance.notices.background, Color::hex("#eeeeee"));
        // O que o usuário não tocou continua vindo do tema.
        assert_eq!(themed.appearance.dialog.overlay, Color::hex("#555555"));
    }

    #[test]
    fn apply_replaces_whole_palette_when_config_is_at_default() {
        let config = Config {
            themes: vec![theme_with_custom_palette()],
            ..Config::default()
        };
        let themed = apply(&config, "p");
        let expected = vec![
            GroupPaletteEntry {
                name: "um".to_owned(),
                color: Color::hex("#101010"),
            },
            GroupPaletteEntry {
                name: "dois".to_owned(),
                color: Color::hex("#202020"),
            },
        ];
        assert_eq!(themed.appearance.groups.palette, expected);
    }

    #[test]
    fn apply_keeps_config_palette_when_already_customized() {
        let mut config = Config {
            themes: vec![theme_with_custom_palette()],
            ..Config::default()
        };
        let custom = vec![GroupPaletteEntry {
            name: "meu".to_owned(),
            color: Color::hex("#303030"),
        }];
        config.appearance.groups.palette = custom.clone();
        let themed = apply(&config, "p");
        assert_eq!(themed.appearance.groups.palette, custom);
    }

    #[test]
    fn apply_palette_field_absent_in_theme_is_a_noop() {
        let config = Config {
            themes: vec![theme_with_colors()],
            ..Config::default()
        };
        let themed = apply(&config, "t");
        assert_eq!(
            themed.appearance.groups.palette,
            Config::default().appearance.groups.palette
        );
    }

    #[test]
    fn overridden_keys_includes_palette_when_config_customized_and_theme_declares_it() {
        let mut config = Config {
            themes: vec![theme_with_custom_palette()],
            ..Config::default()
        };
        config.appearance.groups.palette = vec![GroupPaletteEntry {
            name: "meu".to_owned(),
            color: Color::hex("#303030"),
        }];
        let keys = overridden_keys(&config, "p");
        assert_eq!(keys, vec!["appearance.groups.palette"]);
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
