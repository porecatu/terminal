// SPDX-License-Identifier: GPL-3.0-or-later

//! Temas -- PRD-005 RF-5.18 a RF-5.21, ADR-0031, ADR-0067.
//!
//! Um tema é um arquivo `thm_<nome>.toml` ao lado do `porecatu.toml`, e é o
//! **único** lugar onde mora cor (ADR-0067): os campos de cor de `Config`
//! nunca são lidos do `porecatu.toml` e valem o default embutido até um
//! tema ser aplicado sobre eles. Dois temas vêm embutidos no binário,
//! `default` e `default_light`, transcritos de `docs/config/thm_*.toml`;
//! arquivo em disco com o mesmo nome substitui o embutido.
//!
//! Um tema declara cor, e só cor (ADR-0031): nunca fonte, dimensão, raio,
//! espaçamento ou tempo. Chave ausente num tema cai no tema `default`,
//! campo a campo; `palette` é substituída inteira (ADR-0031 §2).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::Config;
use crate::appearance::GroupPaletteEntry;
use crate::color::Color;
use crate::error::ConfigError;

/// Nome do tema escuro embutido -- também o default de `[terminal] theme`
/// e a reserva de toda chave que outro tema não declara.
pub const DEFAULT_THEME: &str = "default";
/// Nome do tema claro embutido.
pub const DEFAULT_LIGHT_THEME: &str = "default_light";

/// Prefixo do nome de arquivo de um tema: `thm_<nome>.toml`.
pub const THEME_FILE_PREFIX: &str = "thm_";

const DEFAULT_THEME_TOML: &str = include_str!("../../../docs/config/thm_default.toml");
const DEFAULT_LIGHT_THEME_TOML: &str = include_str!("../../../docs/config/thm_default_light.toml");

/// Os arquivos dos dois temas embutidos, em nome de arquivo e texto -- o
/// que `porecatu-ui` grava ao lado do `porecatu.toml` quando o cria.
pub const BUILT_IN_THEME_FILES: [(&str, &str); 2] = [
    ("thm_default.toml", DEFAULT_THEME_TOML),
    ("thm_default_light.toml", DEFAULT_LIGHT_THEME_TOML),
];

/// As 16 cores ANSI de um tema (RF-5.11), cada uma opcional.
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

/// `[terminal]` do tema -- cores de `[terminal.colors]`.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeTerminal {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub cursor: Option<Color>,
    pub cursor_text: Option<Color>,
    pub selection_background: Option<Color>,
    pub selection_foreground: Option<Color>,
    pub prompt_secondary: Option<Color>,
    pub normal: ThemeAnsiPalette,
    pub bright: ThemeAnsiPalette,
}

/// `[window]` do tema -- cores de `[appearance.window]`.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeWindow {
    pub background: Option<Color>,
    pub border: Option<Color>,
}

/// `[tabs]` do tema -- cores de `[appearance.tabs.colors]`, mais o fundo
/// do indicador de overflow (`[appearance.tabs.overflow]`).
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeTabs {
    pub bar_background: Option<Color>,
    pub bar_border: Option<Color>,
    pub active_background: Option<Color>,
    pub active_foreground: Option<Color>,
    pub inactive_background: Option<Color>,
    pub inactive_foreground: Option<Color>,
    pub close_button_foreground: Option<Color>,
    pub close_button_hover_background: Option<Color>,
    pub close_button_hover_foreground: Option<Color>,
    pub exited_foreground: Option<Color>,
    pub active_border: Option<Color>,
    pub inactive_border: Option<Color>,
    pub selected_border: Option<Color>,
    pub activity_indicator: Option<Color>,
    pub bell_indicator: Option<Color>,
    pub rename_background: Option<Color>,
    pub rename_border: Option<Color>,
    pub rename_foreground: Option<Color>,
    pub overflow_indicator_background: Option<Color>,
}

/// `[groups]` do tema. `palette` é substituída inteira, nunca mesclada por
/// índice ou nome de entrada (ADR-0031 §2).
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

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeTooltip {
    pub background: Option<Color>,
    pub border: Option<Color>,
    pub foreground: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeStatusBar {
    pub foreground: Option<Color>,
    pub shell: Option<Color>,
    /// RF-9.4. Um degrau abaixo de `foreground`, e cada tema escolhe o
    /// seu: um alfa fixo não sobrevive a fundos claros e escuros.
    pub stale_cwd: Option<Color>,
    pub ahead_behind: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeWindowControls {
    pub hover_background: Option<Color>,
    pub close_hover_background: Option<Color>,
    pub close_hover_foreground: Option<Color>,
    pub pressed_background: Option<Color>,
    pub close_pressed_background: Option<Color>,
}

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

/// `[settings]` do tema -- cores do corpo da janela de configurações.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ThemeSettings {
    pub panel_background: Option<Color>,
    pub row_background: Option<Color>,
    pub chip_background: Option<Color>,
    pub footer_separator: Option<Color>,
}

/// Um arquivo `thm_<nome>.toml`. O nome vem do arquivo, não do conteúdo.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct Theme {
    #[serde(skip)]
    pub name: String,
    pub terminal: ThemeTerminal,
    pub window: ThemeWindow,
    pub tabs: ThemeTabs,
    pub window_controls: ThemeWindowControls,
    pub groups: ThemeGroups,
    pub notices: ThemeNotices,
    pub dialog: ThemeDialog,
    pub context_menu: ThemeContextMenu,
    pub tooltip: ThemeTooltip,
    pub group_editor: ThemeGroupEditor,
    pub status_bar: ThemeStatusBar,
    pub settings: ThemeSettings,
}

/// Parseia o texto de um arquivo de tema. Devolve o tema mais as chaves
/// desconhecidas (aviso, como no `porecatu.toml`), ou o primeiro erro
/// localizado.
pub fn parse_theme(name: &str, text: &str) -> Result<(Theme, Vec<String>), ConfigError> {
    let deserializer =
        toml::de::Deserializer::parse(text).map_err(|err| ConfigError::from_toml(text, err))?;
    let mut unknown_keys = Vec::new();
    let mut theme: Theme =
        serde_ignored::deserialize(deserializer, |path| unknown_keys.push(path.to_string()))
            .map_err(|err| ConfigError::from_toml(text, err))?;
    theme.name = name.to_owned();
    Ok((theme, unknown_keys))
}

/// Os dois temas embutidos, `default` e `default_light`, nessa ordem.
pub(crate) fn built_in_themes() -> Vec<Theme> {
    static BUILT_IN: OnceLock<Vec<Theme>> = OnceLock::new();
    BUILT_IN
        .get_or_init(|| {
            [
                (DEFAULT_THEME, DEFAULT_THEME_TOML),
                (DEFAULT_LIGHT_THEME, DEFAULT_LIGHT_THEME_TOML),
            ]
            .into_iter()
            .map(|(name, text)| {
                parse_theme(name, text)
                    .unwrap_or_else(|err| panic!("tema embutido {name:?} inválido: {err}"))
                    .0
            })
            .collect()
        })
        .clone()
}

/// O nome do tema de um arquivo `thm_<nome>.toml`; `None` para qualquer
/// outro arquivo (inclusive `thm_.toml`, sem nome).
pub fn theme_name_from_file(path: &Path) -> Option<&str> {
    if path.extension()? != "toml" {
        return None;
    }
    let name = path
        .file_stem()?
        .to_str()?
        .strip_prefix(THEME_FILE_PREFIX)?;
    (!name.is_empty()).then_some(name)
}

/// Problema com um arquivo de tema do disco: nunca derruba a carga, vira
/// aviso. O tema de um arquivo inválido não entra na lista.
#[derive(Debug, Clone, PartialEq)]
pub enum ThemeFileProblem {
    Unreadable { path: PathBuf, cause: String },
    Invalid { path: PathBuf, error: ConfigError },
    UnknownKeys { path: PathBuf, keys: Vec<String> },
}

/// Lê os `thm_*.toml` de `dir` e junta-os aos embutidos de `config.themes`:
/// arquivo com o nome de um embutido o substitui, os demais entram na
/// lista. Ordem final: `default`, `default_light`, depois alfabética.
/// `dir` inexistente não é problema -- só não há tema do disco.
pub fn load_theme_files(config: &mut Config, dir: &Path) -> Vec<ThemeFileProblem> {
    let mut problems = Vec::new();
    let mut paths: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| theme_name_from_file(path).is_some() && path.is_file())
            .collect(),
        Err(_) => Vec::new(),
    };
    paths.sort();
    for path in paths {
        let Some(name) = theme_name_from_file(&path).map(str::to_owned) else {
            continue;
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                problems.push(ThemeFileProblem::Unreadable {
                    path,
                    cause: err.to_string(),
                });
                continue;
            }
        };
        match parse_theme(&name, &text) {
            Ok((theme, keys)) => {
                if !keys.is_empty() {
                    problems.push(ThemeFileProblem::UnknownKeys {
                        path: path.clone(),
                        keys,
                    });
                }
                match config.themes.iter_mut().find(|t| t.name == name) {
                    Some(slot) => *slot = theme,
                    None => config.themes.push(theme),
                }
            }
            Err(error) => problems.push(ThemeFileProblem::Invalid { path, error }),
        }
    }
    sort_themes(&mut config.themes);
    problems
}

fn sort_themes(themes: &mut [Theme]) {
    let rank = |name: &str| match name {
        DEFAULT_THEME => 0,
        DEFAULT_LIGHT_THEME => 1,
        _ => 2,
    };
    themes.sort_by(|a, b| {
        rank(&a.name)
            .cmp(&rank(&b.name))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// Uma cor temável: o caminho dela no `Config` (o mesmo de quando ela era
/// chave do `porecatu.toml`), o valor que o tema declara e o setter.
/// `set` é um ponteiro de função: todo fechamento aqui é não-capturante.
struct MergeField {
    name: &'static str,
    theme_value: Option<Color>,
    set: fn(&mut Config, Color),
}

macro_rules! merge_fields {
    ($theme:ident; $( $name:literal => $($cfg:ident).+ <= $($thm:ident).+ ;)*) => {
        vec![$(
            MergeField {
                name: $name,
                theme_value: $theme.$($thm).+,
                set: |cfg: &mut Config, v: Color| cfg.$($cfg).+ = v,
            },
        )*]
    };
}

fn mergeable_fields(theme: &Theme) -> Vec<MergeField> {
    merge_fields! { theme;
        "terminal.colors.foreground" => terminal.colors.foreground <= terminal.foreground;
        "terminal.colors.background" => terminal.colors.background <= terminal.background;
        "terminal.colors.cursor" => terminal.colors.cursor <= terminal.cursor;
        "terminal.colors.cursor_text" => terminal.colors.cursor_text <= terminal.cursor_text;
        "terminal.colors.selection_background" => terminal.colors.selection_background <= terminal.selection_background;
        "terminal.colors.selection_foreground" => terminal.colors.selection_foreground <= terminal.selection_foreground;
        "terminal.colors.prompt_secondary" => terminal.colors.prompt_secondary <= terminal.prompt_secondary;
        "terminal.colors.normal.black" => terminal.colors.normal.black <= terminal.normal.black;
        "terminal.colors.normal.red" => terminal.colors.normal.red <= terminal.normal.red;
        "terminal.colors.normal.green" => terminal.colors.normal.green <= terminal.normal.green;
        "terminal.colors.normal.yellow" => terminal.colors.normal.yellow <= terminal.normal.yellow;
        "terminal.colors.normal.blue" => terminal.colors.normal.blue <= terminal.normal.blue;
        "terminal.colors.normal.magenta" => terminal.colors.normal.magenta <= terminal.normal.magenta;
        "terminal.colors.normal.cyan" => terminal.colors.normal.cyan <= terminal.normal.cyan;
        "terminal.colors.normal.white" => terminal.colors.normal.white <= terminal.normal.white;
        "terminal.colors.bright.black" => terminal.colors.bright.black <= terminal.bright.black;
        "terminal.colors.bright.red" => terminal.colors.bright.red <= terminal.bright.red;
        "terminal.colors.bright.green" => terminal.colors.bright.green <= terminal.bright.green;
        "terminal.colors.bright.yellow" => terminal.colors.bright.yellow <= terminal.bright.yellow;
        "terminal.colors.bright.blue" => terminal.colors.bright.blue <= terminal.bright.blue;
        "terminal.colors.bright.magenta" => terminal.colors.bright.magenta <= terminal.bright.magenta;
        "terminal.colors.bright.cyan" => terminal.colors.bright.cyan <= terminal.bright.cyan;
        "terminal.colors.bright.white" => terminal.colors.bright.white <= terminal.bright.white;
        "appearance.window.background" => appearance.window.background <= window.background;
        "appearance.window.border" => appearance.window.border <= window.border;
        "appearance.tabs.colors.bar_background" => appearance.tabs.colors.bar_background <= tabs.bar_background;
        "appearance.tabs.colors.bar_border" => appearance.tabs.colors.bar_border <= tabs.bar_border;
        "appearance.tabs.colors.active_background" => appearance.tabs.colors.active_background <= tabs.active_background;
        "appearance.tabs.colors.active_foreground" => appearance.tabs.colors.active_foreground <= tabs.active_foreground;
        "appearance.tabs.colors.inactive_background" => appearance.tabs.colors.inactive_background <= tabs.inactive_background;
        "appearance.tabs.colors.inactive_foreground" => appearance.tabs.colors.inactive_foreground <= tabs.inactive_foreground;
        "appearance.tabs.colors.close_button_foreground" => appearance.tabs.colors.close_button_foreground <= tabs.close_button_foreground;
        "appearance.tabs.colors.close_button_hover_background" => appearance.tabs.colors.close_button_hover_background <= tabs.close_button_hover_background;
        "appearance.tabs.colors.close_button_hover_foreground" => appearance.tabs.colors.close_button_hover_foreground <= tabs.close_button_hover_foreground;
        "appearance.tabs.colors.exited_foreground" => appearance.tabs.colors.exited_foreground <= tabs.exited_foreground;
        "appearance.tabs.colors.active_border" => appearance.tabs.colors.active_border <= tabs.active_border;
        "appearance.tabs.colors.inactive_border" => appearance.tabs.colors.inactive_border <= tabs.inactive_border;
        "appearance.tabs.colors.selected_border" => appearance.tabs.colors.selected_border <= tabs.selected_border;
        "appearance.tabs.colors.activity_indicator" => appearance.tabs.colors.activity_indicator <= tabs.activity_indicator;
        "appearance.tabs.colors.bell_indicator" => appearance.tabs.colors.bell_indicator <= tabs.bell_indicator;
        "appearance.tabs.colors.rename_background" => appearance.tabs.colors.rename_background <= tabs.rename_background;
        "appearance.tabs.colors.rename_border" => appearance.tabs.colors.rename_border <= tabs.rename_border;
        "appearance.tabs.colors.rename_foreground" => appearance.tabs.colors.rename_foreground <= tabs.rename_foreground;
        "appearance.tabs.overflow.indicator_background" => appearance.tabs.overflow.indicator_background <= tabs.overflow_indicator_background;
        "appearance.window_controls.hover_background" => appearance.window_controls.hover_background <= window_controls.hover_background;
        "appearance.window_controls.close_hover_background" => appearance.window_controls.close_hover_background <= window_controls.close_hover_background;
        "appearance.window_controls.close_hover_foreground" => appearance.window_controls.close_hover_foreground <= window_controls.close_hover_foreground;
        "appearance.window_controls.pressed_background" => appearance.window_controls.pressed_background <= window_controls.pressed_background;
        "appearance.window_controls.close_pressed_background" => appearance.window_controls.close_pressed_background <= window_controls.close_pressed_background;
        "appearance.groups.count_background" => appearance.groups.count_background <= groups.count_background;
        "appearance.groups.count_foreground" => appearance.groups.count_foreground <= groups.count_foreground;
        "appearance.groups.glass_border" => appearance.groups.glass_border <= groups.glass_border;
        "appearance.groups.border" => appearance.groups.border <= groups.border;
        "appearance.groups.ungrouped_color" => appearance.groups.ungrouped_color <= groups.ungrouped_color;
        "appearance.notices.background" => appearance.notices.background <= notices.background;
        "appearance.notices.border" => appearance.notices.border <= notices.border;
        "appearance.notices.foreground" => appearance.notices.foreground <= notices.foreground;
        "appearance.notices.body_foreground" => appearance.notices.body_foreground <= notices.body_foreground;
        "appearance.notices.error" => appearance.notices.error <= notices.error;
        "appearance.notices.warning" => appearance.notices.warning <= notices.warning;
        "appearance.notices.info" => appearance.notices.info <= notices.info;
        "appearance.dialog.overlay" => appearance.dialog.overlay <= dialog.overlay;
        "appearance.dialog.background" => appearance.dialog.background <= dialog.background;
        "appearance.dialog.border" => appearance.dialog.border <= dialog.border;
        "appearance.dialog.title_foreground" => appearance.dialog.title_foreground <= dialog.title_foreground;
        "appearance.dialog.foreground" => appearance.dialog.foreground <= dialog.foreground;
        "appearance.dialog.button_border" => appearance.dialog.button_border <= dialog.button_border;
        "appearance.dialog.destructive_foreground" => appearance.dialog.destructive_foreground <= dialog.destructive_foreground;
        "appearance.dialog.destructive_hover_background" => appearance.dialog.destructive_hover_background <= dialog.destructive_hover_background;
        "appearance.context_menu.background" => appearance.context_menu.background <= context_menu.background;
        "appearance.context_menu.border" => appearance.context_menu.border <= context_menu.border;
        "appearance.context_menu.item_hover_background" => appearance.context_menu.item_hover_background <= context_menu.item_hover_background;
        "appearance.context_menu.separator" => appearance.context_menu.separator <= context_menu.separator;
        "appearance.context_menu.foreground" => appearance.context_menu.foreground <= context_menu.foreground;
        "appearance.context_menu.disabled_foreground" => appearance.context_menu.disabled_foreground <= context_menu.disabled_foreground;
        "appearance.context_menu.destructive_foreground" => appearance.context_menu.destructive_foreground <= context_menu.destructive_foreground;
        "appearance.tooltip.background" => appearance.tooltip.background <= tooltip.background;
        "appearance.tooltip.border" => appearance.tooltip.border <= tooltip.border;
        "appearance.tooltip.foreground" => appearance.tooltip.foreground <= tooltip.foreground;
        "appearance.group_editor.background" => appearance.group_editor.background <= group_editor.background;
        "appearance.group_editor.border" => appearance.group_editor.border <= group_editor.border;
        "appearance.group_editor.section_foreground" => appearance.group_editor.section_foreground <= group_editor.section_foreground;
        "appearance.group_editor.input_background" => appearance.group_editor.input_background <= group_editor.input_background;
        "appearance.group_editor.input_border" => appearance.group_editor.input_border <= group_editor.input_border;
        "appearance.group_editor.input_border_focus" => appearance.group_editor.input_border_focus <= group_editor.input_border_focus;
        "appearance.group_editor.input_foreground" => appearance.group_editor.input_foreground <= group_editor.input_foreground;
        "appearance.group_editor.swatch_ring_selected" => appearance.group_editor.swatch_ring_selected <= group_editor.swatch_ring_selected;
        "appearance.group_editor.item_foreground" => appearance.group_editor.item_foreground <= group_editor.item_foreground;
        "appearance.group_editor.item_hover_background" => appearance.group_editor.item_hover_background <= group_editor.item_hover_background;
        "appearance.group_editor.divider" => appearance.group_editor.divider <= group_editor.divider;
        "appearance.group_editor.destructive_foreground" => appearance.group_editor.destructive_foreground <= group_editor.destructive_foreground;
        "appearance.group_editor.destructive_hover_background" => appearance.group_editor.destructive_hover_background <= group_editor.destructive_hover_background;
        "appearance.status_bar.foreground" => appearance.status_bar.foreground <= status_bar.foreground;
        "appearance.status_bar.shell" => appearance.status_bar.shell <= status_bar.shell;
        "appearance.status_bar.stale_cwd" => appearance.status_bar.stale_cwd <= status_bar.stale_cwd;
        "appearance.status_bar.ahead_behind" => appearance.status_bar.ahead_behind <= status_bar.ahead_behind;
        "appearance.settings.panel_background" => appearance.settings.panel_background <= settings.panel_background;
        "appearance.settings.row_background" => appearance.settings.row_background <= settings.row_background;
        "appearance.settings.chip_background" => appearance.settings.chip_background <= settings.chip_background;
        "appearance.settings.footer_separator" => appearance.settings.footer_separator <= settings.footer_separator;
    }
}

/// Caminho de `palette`, que não cabe no formato escalar de `MergeField`.
const PALETTE_KEY: &str = "appearance.groups.palette";

/// `true` se `path` (uma chave desconhecida do `porecatu.toml`, em caminho
/// com pontos) é cor -- coisa que agora só um tema declara (ADR-0067) --
/// ou um `[[themes]]` antigo. É o que deixa o chamador juntar essas chaves
/// num aviso só em vez de um aviso por cor de um arquivo antigo.
pub fn is_color_key(path: &str) -> bool {
    if path == "themes" || path.starts_with("themes.") || path.starts_with("themes[") {
        return true;
    }
    let matches = |field: &str| {
        path == field
            || path
                .strip_prefix(field)
                .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('['))
            || field
                .strip_prefix(path)
                .is_some_and(|rest| rest.starts_with('.'))
    };
    matches(PALETTE_KEY)
        || mergeable_fields(&Theme::default())
            .iter()
            .any(|field| matches(field.name))
}

fn apply_fields(out: &mut Config, theme: &Theme) {
    for field in mergeable_fields(theme) {
        if let Some(value) = field.theme_value {
            (field.set)(out, value);
        }
    }
    if let Some(palette) = &theme.groups.palette {
        out.appearance.groups.palette = palette.clone();
    }
}

fn find<'a>(themes: &'a [Theme], name: &str) -> Option<&'a Theme> {
    themes.iter().find(|t| t.name == name)
}

/// Aplica o tema `name` sobre `config`: primeiro o tema `default` (a
/// reserva de toda chave que o outro tema não declara), depois `name`.
/// `name` vazio ou desconhecido aplica só o `default` -- nome desconhecido
/// é aviso do chamador (ADR-0031 §5), não erro aqui.
pub fn apply(config: &Config, name: &str) -> Config {
    let mut out = config.clone();
    if let Some(default) = find(&config.themes, DEFAULT_THEME) {
        apply_fields(&mut out, default);
    }
    if name != DEFAULT_THEME
        && let Some(theme) = find(&config.themes, name)
    {
        apply_fields(&mut out, theme);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built_in(name: &str) -> Theme {
        built_in_themes()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap()
    }

    #[test]
    fn built_in_themes_are_default_then_default_light() {
        let names: Vec<String> = built_in_themes().into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["default", "default_light"]);
    }

    #[test]
    fn built_in_theme_files_have_no_unknown_keys() {
        for (file, text) in BUILT_IN_THEME_FILES {
            let (_, unknown) = parse_theme("x", text).unwrap();
            assert!(unknown.is_empty(), "{file}: {unknown:?}");
        }
    }

    /// Todo tema embutido declara toda cor: um campo de cor novo em
    /// `Config` sem entrada aqui e nos dois arquivos reprova.
    #[test]
    fn built_in_themes_declare_every_color() {
        for theme in built_in_themes() {
            for field in mergeable_fields(&theme) {
                assert!(
                    field.theme_value.is_some(),
                    "{}: falta {}",
                    theme.name,
                    field.name
                );
            }
            assert!(
                theme.groups.palette.is_some(),
                "{}: falta palette",
                theme.name
            );
        }
    }

    /// `thm_default.toml` e os defaults embutidos de `Config` são a mesma
    /// coisa -- é o que deixa o código citar o arquivo como origem.
    #[test]
    fn default_theme_is_the_built_in_defaults() {
        let config = Config::default();
        let mut applied = config.clone();
        apply_fields(&mut applied, &built_in(DEFAULT_THEME));
        assert_eq!(applied, config);
        assert_eq!(apply(&config, DEFAULT_THEME), config);
        assert_eq!(apply(&config, ""), config);
    }

    #[test]
    fn apply_light_changes_colors() {
        let config = Config::default();
        let light = apply(&config, DEFAULT_LIGHT_THEME);
        assert_eq!(light.terminal.colors.background, Color::hex("#eff1f5"));
        assert_eq!(
            light.appearance.window_controls.hover_background,
            Color::hex("#0000001a")
        );
    }

    #[test]
    fn unknown_theme_applies_default() {
        let config = Config::default();
        assert_eq!(apply(&config, "nao-existe"), config);
    }

    #[test]
    fn partial_theme_falls_back_to_default_theme() {
        let mut config = Config::default();
        let (theme, _) = parse_theme("p", "[terminal]\nforeground = \"#111111\"\n").unwrap();
        config.themes.push(theme);
        let themed = apply(&config, "p");
        assert_eq!(themed.terminal.colors.foreground, Color::hex("#111111"));
        assert_eq!(
            themed.terminal.colors.background,
            Config::default().terminal.colors.background
        );
    }

    #[test]
    fn partial_theme_falls_back_to_a_customized_default_theme() {
        let mut config = Config::default();
        let (default, _) =
            parse_theme(DEFAULT_THEME, "[terminal]\nbackground = \"#222222\"\n").unwrap();
        config.themes[0] = default;
        let (theme, _) = parse_theme("p", "[terminal]\nforeground = \"#111111\"\n").unwrap();
        config.themes.push(theme);
        let themed = apply(&config, "p");
        assert_eq!(themed.terminal.colors.background, Color::hex("#222222"));
    }

    #[test]
    fn palette_is_replaced_whole() {
        let mut config = Config::default();
        let (theme, _) = parse_theme(
            "p",
            "[groups]\npalette = [{ name = \"um\", color = \"#101010\" }]\n",
        )
        .unwrap();
        config.themes.push(theme);
        let themed = apply(&config, "p");
        assert_eq!(
            themed.appearance.groups.palette,
            vec![GroupPaletteEntry {
                name: "um".to_owned(),
                color: Color::hex("#101010"),
            }]
        );
    }

    #[test]
    fn theme_name_comes_from_the_file_name() {
        assert_eq!(
            theme_name_from_file(Path::new("/x/thm_default_light.toml")),
            Some("default_light")
        );
        assert_eq!(theme_name_from_file(Path::new("/x/thm_.toml")), None);
        assert_eq!(theme_name_from_file(Path::new("/x/porecatu.toml")), None);
        assert_eq!(theme_name_from_file(Path::new("/x/thm_a.txt")), None);
    }

    #[test]
    fn theme_files_override_built_ins_and_sort() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("thm_default.toml"),
            "[terminal]\nbackground = \"#222222\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("thm_zeta.toml"), "").unwrap();
        std::fs::write(dir.path().join("thm_alfa.toml"), "").unwrap();
        std::fs::write(dir.path().join("porecatu.toml"), "").unwrap();
        let mut config = Config::default();
        let problems = load_theme_files(&mut config, dir.path());
        assert!(problems.is_empty(), "{problems:?}");
        let names: Vec<&str> = config.themes.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["default", "default_light", "alfa", "zeta"]);
        assert_eq!(
            config.themes[0].terminal.background,
            Some(Color::hex("#222222"))
        );
    }

    #[test]
    fn invalid_theme_file_is_a_problem_not_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("thm_ruim.toml"), "isto nao e toml").unwrap();
        std::fs::write(dir.path().join("thm_extra.toml"), "[terminal]\nfoo = 1\n").unwrap();
        let mut config = Config::default();
        let problems = load_theme_files(&mut config, dir.path());
        assert_eq!(problems.len(), 2);
        assert!(problems.iter().any(|p| matches!(
            p,
            ThemeFileProblem::UnknownKeys { keys, .. } if keys == &vec!["terminal.foo".to_owned()]
        )));
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, ThemeFileProblem::Invalid { .. }))
        );
        let names: Vec<&str> = config.themes.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["default", "default_light", "extra"]);
    }

    #[test]
    fn missing_dir_has_no_theme_files() {
        let mut config = Config::default();
        let problems = load_theme_files(&mut config, Path::new("/porecatu/nao/existe"));
        assert!(problems.is_empty());
        assert_eq!(config.themes.len(), 2);
    }

    #[test]
    fn color_keys_are_recognized() {
        assert!(is_color_key("terminal.colors"));
        assert!(is_color_key("terminal.colors.foreground"));
        assert!(is_color_key("appearance.tabs.colors.bar_background"));
        assert!(is_color_key("appearance.groups.palette"));
        assert!(is_color_key("themes"));
        assert!(!is_color_key("appearance.tabs.colors.active_border_width"));
        assert!(!is_color_key("general.foo"));
    }
}
