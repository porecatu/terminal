// SPDX-License-Identifier: GPL-3.0-or-later

//! O catálogo curado da tela de configurações (RF-16.11, ADR-0059 §4): uma
//! **tabela declarativa** com as opções que a tela mostra -- nem mais, nem
//! menos --, na ordem dos grupos do RF-16.7 e, dentro deles, na ordem em que o
//! RF-16.11 as lista. Mudar a tabela é mudar o requisito.
//!
//! Cada linha diz onde a opção mora no arquivo (`KeyPath`), em que grupo e
//! seção ela aparece, que controle ela pede e quais são as frases dela. Duas
//! coisas **não** são escritas à mão:
//!
//! - o valor em vigor e o padrão, lidos de um `Config` por `read` -- o padrão
//!   é `read(&Config::default())`, então a tabela nunca repete um número que
//!   o crate de config já tem;
//! - o escopo de recarga, derivado de `reload::diff` por sondagem
//!   ([`OptionDef::reload_scope`]): a mesma função que decide, depois de
//!   gravar, que aviso a recarga mostra. "Vale em aba nova" na tela e o aviso
//!   nunca divergem, porque é uma conta só.
//!
//! As frases são ponteiros para os acessores do registro de mensagens, não
//! texto: esquecer uma é erro de compilação, e o teste de completude de
//! `locales/` cobre os cinco arquivos (ADR-0056).

// Sem consumidor até as tarefas que desenham a tela (06 em diante): o catálogo
// é dado, e é testado por inteiro aqui.
#![allow(dead_code)]

use std::sync::OnceLock;

use porecatu_config::{Config, ConfigDocument, Edit, EditValue, KeyPath};
use porecatu_locale::Catalog;

use crate::messages::msg;
use crate::reload::{self, DeferredScope};

/// Os nove grupos da guia lateral, na ordem do RF-16.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Group {
    General,
    Shell,
    Terminal,
    Appearance,
    Session,
    Project,
    Git,
    Panes,
    /// Sem opções nesta tabela: a lista de atalhos tem estado próprio
    /// (RF-16.28 a RF-16.31).
    Shortcuts,
}

impl Group {
    pub(crate) const ALL: [Group; 9] = [
        Group::General,
        Group::Shell,
        Group::Terminal,
        Group::Appearance,
        Group::Session,
        Group::Project,
        Group::Git,
        Group::Panes,
        Group::Shortcuts,
    ];

    pub(crate) fn label(self, catalog: &Catalog) -> String {
        match self {
            Group::General => msg::settings::group::general(catalog),
            Group::Shell => msg::settings::group::shell(catalog),
            Group::Terminal => msg::settings::group::terminal(catalog),
            Group::Appearance => msg::settings::group::appearance(catalog),
            Group::Session => msg::settings::group::session(catalog),
            Group::Project => msg::settings::group::project(catalog),
            Group::Git => msg::settings::group::git(catalog),
            Group::Panes => msg::settings::group::panes(catalog),
            Group::Shortcuts => msg::settings::group::shortcuts(catalog),
        }
    }
}

/// Seção dentro de um grupo: o rótulo curto (FONTE, CURSOR, ...) que agrupa
/// opções parecidas no painel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Section {
    Language,
    Startup,
    Confirmations,
    Program,
    Environment,
    Font,
    Cursor,
    Scrollback,
    Selection,
    Clipboard,
    Links,
    Background,
    Theme,
    Window,
    Tabs,
    StatusBar,
    Restore,
    ProjectFile,
    GitRemote,
    PanesSize,
}

impl Section {
    pub(crate) fn label(self, catalog: &Catalog) -> String {
        match self {
            Section::Language => msg::settings::section::language(catalog),
            Section::Startup => msg::settings::section::startup(catalog),
            Section::Confirmations => msg::settings::section::confirmations(catalog),
            Section::Program => msg::settings::section::program(catalog),
            Section::Environment => msg::settings::section::environment(catalog),
            Section::Font => msg::settings::section::font(catalog),
            Section::Cursor => msg::settings::section::cursor(catalog),
            Section::Scrollback => msg::settings::section::scrollback(catalog),
            Section::Selection => msg::settings::section::selection(catalog),
            Section::Clipboard => msg::settings::section::clipboard(catalog),
            Section::Links => msg::settings::section::links(catalog),
            Section::Background => msg::settings::section::background(catalog),
            Section::Theme => msg::settings::section::theme(catalog),
            Section::Window => msg::settings::section::window(catalog),
            Section::Tabs => msg::settings::section::tabs(catalog),
            Section::StatusBar => msg::settings::section::status_bar(catalog),
            Section::Restore => msg::settings::section::restore(catalog),
            Section::ProjectFile => msg::settings::section::project_file(catalog),
            Section::GitRemote => msg::settings::section::git_remote(catalog),
            Section::PanesSize => msg::settings::section::panes_size(catalog),
        }
    }
}

/// O controle que o tipo da opção pede (RF-16.12, ADR-0060 §3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Control {
    /// Booleano.
    Toggle,
    /// Texto livre -- caminho e família de fonte também (nenhum seletor
    /// nativo, RF-16.12).
    Text,
    /// Número com faixa de **edição** (RF-16.18): `step` é o passo do
    /// campo, `float` diz se o valor é gravado com parte decimal.
    Number {
        min: f64,
        max: f64,
        step: f64,
        float: bool,
    },
    /// Valor nomeado de um enum, na grafia do arquivo.
    Choice(&'static [&'static str]),
    /// Escolha entre os arquivos de idioma encontrados -- a lista vem do
    /// disco, não da tabela.
    Language,
    /// Lista editável de textos (`shell.args`, `project_file.trusted_paths`).
    StringList,
    /// Lista de nome e valor (`shell.env`).
    StringMap,
    /// Lista de temas com amostra (RF-16.24); o valor é o nome, vazio para
    /// "sem tema".
    Theme,
    /// `git.remote_poll_interval_secs`: alternância que grava `0` desligada,
    /// mais o número em segundos (`GIT_POLL_MIN..=GIT_POLL_MAX`).
    GitPoll,
}

/// Menor intervalo de consulta ao remoto que a tela aceita. O app eleva
/// valores de 1 a 29 a 30 e avisa (RF-13.3); a tela simplesmente não os
/// oferece.
pub(crate) const GIT_POLL_MIN: i64 = 30;
/// Maior intervalo: um dia.
pub(crate) const GIT_POLL_MAX: i64 = 86_400;

/// Até quando uma mudança vale, como a tela a mostra ao lado do nome
/// (RF-13). Espelha `reload::DeferredScope` mais o caso "na hora".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReloadScope {
    /// Classes A e B do ADR-0030: vale assim que a recarga roda.
    Live,
    /// "vale em aba nova".
    NewTab,
    /// "vale na próxima janela".
    NextWindow,
    /// "após reiniciar".
    Restart,
}

/// Uma opção do catálogo.
pub(crate) struct OptionDef {
    /// Identificador curto e estável (`font_size`): o que as frases do
    /// registro e o rascunho usam para nomear a opção.
    pub id: &'static str,
    pub group: Group,
    pub section: Section,
    /// Caminho pontilhado da chave no arquivo (`terminal.font.size`).
    pub path: &'static str,
    pub control: Control,
    pub label: fn(&Catalog) -> String,
    pub description: fn(&Catalog) -> String,
    /// O valor desta opção num `Config`.
    read: fn(&Config) -> EditValue,
}

impl OptionDef {
    pub(crate) fn key_path(&self) -> KeyPath {
        KeyPath::parse(self.path).expect("o catálogo só tem caminhos válidos")
    }

    /// O valor em vigor em `config`.
    pub(crate) fn read(&self, config: &Config) -> EditValue {
        (self.read)(config)
    }

    /// O padrão: o valor de `Config::default()`.
    pub(crate) fn default_value(&self) -> EditValue {
        (self.read)(&Config::default())
    }

    /// Escopo de recarga, derivado de `reload::diff` (ver o módulo). Calculado
    /// uma vez para a tabela inteira.
    pub(crate) fn reload_scope(&self) -> ReloadScope {
        static SCOPES: OnceLock<Vec<ReloadScope>> = OnceLock::new();
        let scopes = SCOPES.get_or_init(|| OPTIONS.iter().map(probe_scope).collect());
        let index = OPTIONS
            .iter()
            .position(|option| option.id == self.id)
            .expect("toda opção está na tabela");
        scopes[index]
    }
}

/// Os escopos da tabela, **sem** cache: o que `reload_scope` guarda. Separado
/// para o teste comparar a sondagem com `diff` direto.
fn probe_scope(option: &OptionDef) -> ReloadScope {
    let default = Config::default();
    let changed = config_with(option, &probe_value(option))
        .expect("o valor de sondagem é válido para a própria opção");
    match reload::diff(&default, &changed).deferred.first() {
        Some(deferred) => match deferred.scope {
            DeferredScope::NewTab => ReloadScope::NewTab,
            DeferredScope::NextWindow => ReloadScope::NextWindow,
            DeferredScope::Restart => ReloadScope::Restart,
        },
        None => ReloadScope::Live,
    }
}

/// Um `Config` igual ao padrão, salvo por `option` valer `value`: um `Set`
/// aplicado a um arquivo vazio e relido pelo mesmo `parse` da carga.
pub(crate) fn config_with(option: &OptionDef, value: &EditValue) -> Option<Config> {
    let document = ConfigDocument::parse("").ok()?;
    let (text, _) = document
        .apply_checked(&[Edit::Set(option.key_path(), value.clone())])
        .ok()?;
    porecatu_config::parse(&text).ok().map(|(config, _)| config)
}

/// Um valor válido para `option` **diferente do padrão** -- o que a sondagem
/// de escopo grava para ver o que `diff` diz.
pub(crate) fn probe_value(option: &OptionDef) -> EditValue {
    let default = option.default_value();
    match (option.control, &default) {
        (Control::Toggle, EditValue::Bool(value)) => EditValue::Bool(!value),
        (
            Control::Number {
                min, max, float, ..
            },
            current,
        ) => {
            let current = number_of(current).unwrap_or(min);
            let target = if (current - max).abs() > f64::EPSILON {
                max
            } else {
                min
            };
            if float {
                EditValue::Float(target)
            } else {
                EditValue::Integer(target as i64)
            }
        }
        (Control::Choice(choices), EditValue::String(current)) => EditValue::String(
            choices
                .iter()
                .find(|choice| **choice != current)
                .expect("uma escolha tem ao menos dois valores")
                .to_string(),
        ),
        (Control::Text | Control::Language | Control::Theme, EditValue::String(current)) => {
            EditValue::String(format!("{current}x"))
        }
        (Control::StringList, _) => EditValue::StringList(vec!["x".to_owned()]),
        (Control::StringMap, _) => {
            EditValue::StringMap([("X".to_owned(), "1".to_owned())].into_iter().collect())
        }
        (Control::GitPoll, EditValue::Integer(current)) => {
            EditValue::Integer(if *current == 0 { GIT_POLL_MIN } else { 0 })
        }
        (control, value) => unreachable!("controle {control:?} sobre valor {value:?}"),
    }
}

/// O número de um valor numérico, para a faixa.
pub(crate) fn number_of(value: &EditValue) -> Option<f64> {
    match value {
        EditValue::Integer(value) => Some(*value as f64),
        EditValue::Float(value) => Some(*value),
        _ => None,
    }
}

fn string(value: &str) -> EditValue {
    EditValue::String(value.to_owned())
}

fn list(values: &[String]) -> EditValue {
    EditValue::StringList(values.to_vec())
}

const TOGGLE: Control = Control::Toggle;

/// Faixas de edição (RF-16.18). `Control::Number` com `float: true` grava
/// `14.0`, nunca `14`.
const fn float(min: f64, max: f64, step: f64) -> Control {
    Control::Number {
        min,
        max,
        step,
        float: true,
    }
}

const fn integer(min: f64, max: f64, step: f64) -> Control {
    Control::Number {
        min,
        max,
        step,
        float: false,
    }
}

/// Monta uma linha da tabela. Os acessores das frases vêm do módulo
/// `labels` pelo identificador: `option!(font_size, ...)` usa
/// `labels::font_size::{label, description}`.
macro_rules! option {
    ($id:ident, $group:ident, $section:ident, $path:literal, $control:expr, $read:expr) => {
        OptionDef {
            id: stringify!($id),
            group: Group::$group,
            section: Section::$section,
            path: $path,
            control: $control,
            label: labels::$id::label,
            description: labels::$id::description,
            read: $read,
        }
    };
}

/// Um acessor de rótulo e um de descrição por opção, ligados ao registro.
/// Existe porque o registro nomeia as frases `<id>_label`/`<id>_description`
/// (no máximo dois níveis de tabela, ADR-0056 §1) e a tabela abaixo as
/// pede por `<id>::label`.
macro_rules! label_modules {
    ($($id:ident => ($label:ident, $description:ident)),* $(,)?) => {
        mod labels {
            $(
                pub(super) mod $id {
                    use porecatu_locale::Catalog;

                    use crate::messages::msg;

                    pub(in crate::settings::catalog) fn label(catalog: &Catalog) -> String {
                        msg::settings::option::$label(catalog)
                    }

                    pub(in crate::settings::catalog) fn description(catalog: &Catalog) -> String {
                        msg::settings::option::$description(catalog)
                    }
                }
            )*
        }
    };
}

label_modules! {
    language => (language_label, language_description),
    startup_directory => (startup_directory_label, startup_directory_description),
    confirm_close_with_process => (confirm_close_with_process_label, confirm_close_with_process_description),
    confirm_close_window => (confirm_close_window_label, confirm_close_window_description),
    shell_program => (shell_program_label, shell_program_description),
    shell_args => (shell_args_label, shell_args_description),
    shell_env => (shell_env_label, shell_env_description),
    font_family => (font_family_label, font_family_description),
    font_size => (font_size_label, font_size_description),
    line_height => (line_height_label, line_height_description),
    letter_spacing => (letter_spacing_label, letter_spacing_description),
    bold_is_bright => (bold_is_bright_label, bold_is_bright_description),
    cursor_shape => (cursor_shape_label, cursor_shape_description),
    cursor_blink => (cursor_blink_label, cursor_blink_description),
    cursor_follows_group_color => (cursor_follows_group_color_label, cursor_follows_group_color_description),
    cursor_unfocused_hollow => (cursor_unfocused_hollow_label, cursor_unfocused_hollow_description),
    scrollback_lines => (scrollback_lines_label, scrollback_lines_description),
    scroll_multiplier => (scroll_multiplier_label, scroll_multiplier_description),
    scroll_on_output => (scroll_on_output_label, scroll_on_output_description),
    scroll_on_input => (scroll_on_input_label, scroll_on_input_description),
    alternate_scroll => (alternate_scroll_label, alternate_scroll_description),
    copy_on_select => (copy_on_select_label, copy_on_select_description),
    word_separators => (word_separators_label, word_separators_description),
    osc52_write => (osc52_write_label, osc52_write_description),
    osc52_read => (osc52_read_label, osc52_read_description),
    osc52_max_bytes => (osc52_max_bytes_label, osc52_max_bytes_description),
    hyperlinks_enabled => (hyperlinks_enabled_label, hyperlinks_enabled_description),
    background_opacity => (background_opacity_label, background_opacity_description),
    background_image => (background_image_label, background_image_description),
    background_image_mode => (background_image_mode_label, background_image_mode_description),
    background_image_opacity => (background_image_opacity_label, background_image_opacity_description),
    theme => (theme_label, theme_description),
    animations => (animations_label, animations_description),
    window_opacity => (window_opacity_label, window_opacity_description),
    decorations => (decorations_label, decorations_description),
    tab_bar_position => (tab_bar_position_label, tab_bar_position_description),
    show_close_button => (show_close_button_label, show_close_button_description),
    show_index => (show_index_label, show_index_description),
    show_activity_indicator => (show_activity_indicator_label, show_activity_indicator_description),
    show_bell_indicator => (show_bell_indicator_label, show_bell_indicator_description),
    show_new_tab_button => (show_new_tab_button_label, show_new_tab_button_description),
    hide_when_single_tab => (hide_when_single_tab_label, hide_when_single_tab_description),
    status_bar_enabled => (status_bar_enabled_label, status_bar_enabled_description),
    session_enabled => (session_enabled_label, session_enabled_description),
    lazy_restore => (lazy_restore_label, lazy_restore_description),
    restore_window_geometry => (restore_window_geometry_label, restore_window_geometry_description),
    suggest_shell_integration => (suggest_shell_integration_label, suggest_shell_integration_description),
    project_file_enabled => (project_file_enabled_label, project_file_enabled_description),
    trusted_paths => (trusted_paths_label, trusted_paths_description),
    git_remote_poll => (git_remote_poll_label, git_remote_poll_description),
    min_columns => (min_columns_label, min_columns_description),
    min_rows => (min_rows_label, min_rows_description),
}

/// Os valores nomeados de cada enum, na grafia do arquivo.
const CURSOR_SHAPES: &[&str] = &["block", "beam", "underline"];
const CLOSE_BUTTON: &[&str] = &["always", "hover", "never"];
const TAB_BAR_POSITIONS: &[&str] = &["top", "bottom"];
const BACKGROUND_IMAGE_MODES: &[&str] = &["stretch", "tile", "center"];

fn background_image_mode(config: &Config) -> EditValue {
    string(match config.terminal.background_image.mode {
        porecatu_config::BackgroundImageMode::Stretch => "stretch",
        porecatu_config::BackgroundImageMode::Tile => "tile",
        porecatu_config::BackgroundImageMode::Center => "center",
    })
}

/// A opacidade da imagem é `f32` no arquivo e `f64` no rascunho: `0.35` lido
/// como `f32` e alargado dá `0.3499999940...`, e o valor digitado (`0.35`)
/// pareceria uma alteração pendente mesmo igual ao do arquivo. Seis casas
/// bastam para devolver o que foi escrito (um `f32` tem ~7 dígitos).
fn image_opacity(config: &Config) -> EditValue {
    let value = f64::from(config.terminal.background_image.opacity);
    EditValue::Float((value * 1_000_000.0).round() / 1_000_000.0)
}

fn cursor_shape(config: &Config) -> EditValue {
    string(match config.terminal.cursor.shape {
        porecatu_config::CursorShape::Block => "block",
        porecatu_config::CursorShape::Beam => "beam",
        porecatu_config::CursorShape::Underline => "underline",
    })
}

fn close_button(config: &Config) -> EditValue {
    string(match config.appearance.tabs.show_close_button {
        porecatu_config::CloseButtonVisibility::Always => "always",
        porecatu_config::CloseButtonVisibility::Hover => "hover",
        porecatu_config::CloseButtonVisibility::Never => "never",
    })
}

fn tab_bar_position(config: &Config) -> EditValue {
    string(match config.appearance.window.tab_bar_position {
        porecatu_config::TabBarPosition::Top => "top",
        porecatu_config::TabBarPosition::Bottom => "bottom",
    })
}

/// As 52 opções do RF-16.11 (49 e as três da imagem de fundo, RF-17.20).
pub(crate) static OPTIONS: &[OptionDef] = &[
    // ---- Geral
    option!(
        language,
        General,
        Language,
        "general.language",
        Control::Language,
        |c| string(&c.general.language)
    ),
    option!(
        startup_directory,
        General,
        Startup,
        "general.startup_directory",
        Control::Text,
        |c| string(&c.general.startup_directory)
    ),
    option!(
        confirm_close_with_process,
        General,
        Confirmations,
        "general.confirm_close_with_process",
        TOGGLE,
        |c| EditValue::Bool(c.general.confirm_close_with_process)
    ),
    option!(
        confirm_close_window,
        General,
        Confirmations,
        "general.confirm_close_window",
        TOGGLE,
        |c| EditValue::Bool(c.general.confirm_close_window)
    ),
    // ---- Shell
    option!(
        shell_program,
        Shell,
        Program,
        "shell.program",
        Control::Text,
        |c| string(&c.shell.program)
    ),
    option!(
        shell_args,
        Shell,
        Program,
        "shell.args",
        Control::StringList,
        |c| list(&c.shell.args)
    ),
    option!(
        shell_env,
        Shell,
        Environment,
        "shell.env",
        Control::StringMap,
        |c| EditValue::StringMap(c.shell.env.clone())
    ),
    // ---- Terminal
    option!(
        font_family,
        Terminal,
        Font,
        "terminal.font.family",
        Control::Text,
        |c| string(&c.terminal.font.family)
    ),
    option!(
        font_size,
        Terminal,
        Font,
        "terminal.font.size",
        float(6.0, 72.0, 0.5),
        |c| EditValue::Float(c.terminal.font.size)
    ),
    option!(
        line_height,
        Terminal,
        Font,
        "terminal.font.line_height",
        float(0.8, 3.0, 0.05),
        |c| EditValue::Float(c.terminal.font.line_height)
    ),
    option!(
        letter_spacing,
        Terminal,
        Font,
        "terminal.font.letter_spacing",
        float(-0.2, 1.0, 0.01),
        |c| EditValue::Float(c.terminal.font.letter_spacing)
    ),
    option!(
        bold_is_bright,
        Terminal,
        Font,
        "terminal.font.bold_is_bright",
        TOGGLE,
        |c| EditValue::Bool(c.terminal.font.bold_is_bright)
    ),
    option!(
        cursor_shape,
        Terminal,
        Cursor,
        "terminal.cursor.shape",
        Control::Choice(CURSOR_SHAPES),
        cursor_shape
    ),
    option!(
        cursor_blink,
        Terminal,
        Cursor,
        "terminal.cursor.blink",
        TOGGLE,
        |c| EditValue::Bool(c.terminal.cursor.blink)
    ),
    option!(
        cursor_follows_group_color,
        Terminal,
        Cursor,
        "terminal.cursor.follows_group_color",
        TOGGLE,
        |c| EditValue::Bool(c.terminal.cursor.follows_group_color)
    ),
    option!(
        cursor_unfocused_hollow,
        Terminal,
        Cursor,
        "terminal.cursor.unfocused_hollow",
        TOGGLE,
        |c| EditValue::Bool(c.terminal.cursor.unfocused_hollow)
    ),
    option!(
        scrollback_lines,
        Terminal,
        Scrollback,
        "terminal.scrollback.lines",
        integer(0.0, 1_000_000.0, 1000.0),
        |c| EditValue::Integer(i64::from(c.terminal.scrollback.lines))
    ),
    option!(
        scroll_multiplier,
        Terminal,
        Scrollback,
        "terminal.scrollback.scroll_multiplier",
        integer(1.0, 50.0, 1.0),
        |c| EditValue::Integer(i64::from(c.terminal.scrollback.scroll_multiplier))
    ),
    option!(
        scroll_on_output,
        Terminal,
        Scrollback,
        "terminal.scrollback.scroll_on_output",
        TOGGLE,
        |c| EditValue::Bool(c.terminal.scrollback.scroll_on_output)
    ),
    option!(
        scroll_on_input,
        Terminal,
        Scrollback,
        "terminal.scrollback.scroll_on_input",
        TOGGLE,
        |c| EditValue::Bool(c.terminal.scrollback.scroll_on_input)
    ),
    option!(
        alternate_scroll,
        Terminal,
        Scrollback,
        "terminal.scrollback.alternate_scroll",
        TOGGLE,
        |c| EditValue::Bool(c.terminal.scrollback.alternate_scroll)
    ),
    option!(
        copy_on_select,
        Terminal,
        Selection,
        "terminal.selection.copy_on_select",
        TOGGLE,
        |c| EditValue::Bool(c.terminal.selection.copy_on_select)
    ),
    option!(
        word_separators,
        Terminal,
        Selection,
        "terminal.selection.word_separators",
        Control::Text,
        |c| string(&c.terminal.selection.word_separators)
    ),
    option!(
        osc52_write,
        Terminal,
        Clipboard,
        "terminal.clipboard.osc52_write",
        TOGGLE,
        |c| EditValue::Bool(c.terminal.clipboard.osc52_write)
    ),
    option!(
        osc52_read,
        Terminal,
        Clipboard,
        "terminal.clipboard.osc52_read",
        TOGGLE,
        |c| EditValue::Bool(c.terminal.clipboard.osc52_read)
    ),
    option!(
        osc52_max_bytes,
        Terminal,
        Clipboard,
        "terminal.clipboard.osc52_max_bytes",
        integer(1024.0, 10_485_760.0, 1024.0),
        |c| EditValue::Integer(c.terminal.clipboard.osc52_max_bytes as i64)
    ),
    option!(
        hyperlinks_enabled,
        Terminal,
        Links,
        "terminal.hyperlinks.enabled",
        TOGGLE,
        |c| EditValue::Bool(c.terminal.hyperlinks.enabled)
    ),
    option!(
        background_opacity,
        Terminal,
        Background,
        "terminal.background_opacity",
        float(0.0, 1.0, 0.05),
        |c| EditValue::Float(c.terminal.background_opacity)
    ),
    // PRD-017 RF-17.20: o caminho é texto (sem seletor de arquivo, RF-16.12),
    // o modo uma escolha de três e a opacidade um número de 0.0 a 1.0 no passo
    // e na precisão de `background_opacity`.
    option!(
        background_image,
        Terminal,
        Background,
        "terminal.background_image.path",
        Control::Text,
        |c| string(&c.terminal.background_image.path)
    ),
    option!(
        background_image_mode,
        Terminal,
        Background,
        "terminal.background_image.mode",
        Control::Choice(BACKGROUND_IMAGE_MODES),
        background_image_mode
    ),
    option!(
        background_image_opacity,
        Terminal,
        Background,
        "terminal.background_image.opacity",
        float(0.0, 1.0, 0.05),
        image_opacity
    ),
    // ---- Aparência
    option!(
        theme,
        Appearance,
        Theme,
        "terminal.theme",
        Control::Theme,
        |c| string(&c.terminal.theme)
    ),
    option!(
        animations,
        Appearance,
        Window,
        "appearance.window.animations",
        TOGGLE,
        |c| EditValue::Bool(c.appearance.window.animations)
    ),
    option!(
        window_opacity,
        Appearance,
        Window,
        "appearance.window.opacity",
        float(0.0, 1.0, 0.05),
        |c| EditValue::Float(c.appearance.window.opacity)
    ),
    option!(
        decorations,
        Appearance,
        Window,
        "appearance.window.decorations",
        TOGGLE,
        |c| EditValue::Bool(c.appearance.window.decorations)
    ),
    option!(
        tab_bar_position,
        Appearance,
        Window,
        "appearance.window.tab_bar_position",
        Control::Choice(TAB_BAR_POSITIONS),
        tab_bar_position
    ),
    option!(
        show_close_button,
        Appearance,
        Tabs,
        "appearance.tabs.show_close_button",
        Control::Choice(CLOSE_BUTTON),
        close_button
    ),
    option!(
        show_index,
        Appearance,
        Tabs,
        "appearance.tabs.show_index",
        TOGGLE,
        |c| EditValue::Bool(c.appearance.tabs.show_index)
    ),
    option!(
        show_activity_indicator,
        Appearance,
        Tabs,
        "appearance.tabs.show_activity_indicator",
        TOGGLE,
        |c| EditValue::Bool(c.appearance.tabs.show_activity_indicator)
    ),
    option!(
        show_bell_indicator,
        Appearance,
        Tabs,
        "appearance.tabs.show_bell_indicator",
        TOGGLE,
        |c| EditValue::Bool(c.appearance.tabs.show_bell_indicator)
    ),
    option!(
        show_new_tab_button,
        Appearance,
        Tabs,
        "appearance.tabs.show_new_tab_button",
        TOGGLE,
        |c| EditValue::Bool(c.appearance.tabs.show_new_tab_button)
    ),
    option!(
        hide_when_single_tab,
        Appearance,
        Tabs,
        "appearance.tabs.hide_when_single_tab",
        TOGGLE,
        |c| EditValue::Bool(c.appearance.tabs.hide_when_single_tab)
    ),
    option!(
        status_bar_enabled,
        Appearance,
        StatusBar,
        "appearance.status_bar.enabled",
        TOGGLE,
        |c| EditValue::Bool(c.appearance.status_bar.enabled)
    ),
    // ---- Sessão
    option!(
        session_enabled,
        Session,
        Restore,
        "session.enabled",
        TOGGLE,
        |c| EditValue::Bool(c.session.enabled)
    ),
    option!(
        lazy_restore,
        Session,
        Restore,
        "session.lazy_restore",
        TOGGLE,
        |c| EditValue::Bool(c.session.lazy_restore)
    ),
    option!(
        restore_window_geometry,
        Session,
        Restore,
        "session.restore_window_geometry",
        TOGGLE,
        |c| EditValue::Bool(c.session.restore_window_geometry)
    ),
    option!(
        suggest_shell_integration,
        Session,
        Restore,
        "session.suggest_shell_integration",
        TOGGLE,
        |c| EditValue::Bool(c.session.suggest_shell_integration)
    ),
    // ---- Projeto
    option!(
        project_file_enabled,
        Project,
        ProjectFile,
        "project_file.enabled",
        TOGGLE,
        |c| EditValue::Bool(c.project_file.enabled)
    ),
    option!(
        trusted_paths,
        Project,
        ProjectFile,
        "project_file.trusted_paths",
        Control::StringList,
        |c| list(&c.project_file.trusted_paths)
    ),
    // ---- Git
    option!(
        git_remote_poll,
        Git,
        GitRemote,
        "git.remote_poll_interval_secs",
        Control::GitPoll,
        |c| EditValue::Integer(c.git.remote_poll_interval_secs as i64)
    ),
    // ---- Painéis
    option!(
        min_columns,
        Panes,
        PanesSize,
        "panes.min_columns",
        integer(1.0, 200.0, 1.0),
        |c| EditValue::Integer(i64::from(c.panes.min_columns))
    ),
    option!(
        min_rows,
        Panes,
        PanesSize,
        "panes.min_rows",
        integer(1.0, 100.0, 1.0),
        |c| EditValue::Integer(i64::from(c.panes.min_rows))
    ),
];

/// As opções de um grupo, na ordem da tabela.
pub(crate) fn options_in(group: Group) -> impl Iterator<Item = &'static OptionDef> {
    OPTIONS.iter().filter(move |option| option.group == group)
}

/// A opção de identificador `id`.
pub(crate) fn option(id: &str) -> Option<&'static OptionDef> {
    OPTIONS.iter().find(|option| option.id == id)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const EXAMPLE: &str = include_str!("../../../../docs/config/porecatu.example.toml");

    #[test]
    fn the_catalog_has_exactly_the_options_of_rf_16_11() {
        assert_eq!(OPTIONS.len(), 52);
        let per_group: Vec<usize> = Group::ALL
            .iter()
            .map(|group| options_in(*group).count())
            .collect();
        assert_eq!(per_group, [4, 3, 24, 12, 4, 2, 1, 2, 0]);
    }

    /// RF-17.20: as três opções da imagem de fundo ficam no grupo Terminal,
    /// no subgrupo do fundo, logo depois de `background_opacity` e nesta
    /// ordem, todas com recarga na hora (classe A, ADR-0061 §9).
    #[test]
    fn the_background_image_options_follow_background_opacity_in_the_terminal_group() {
        let ids: Vec<&str> = options_in(Group::Terminal).map(|o| o.id).collect();
        let at = ids
            .iter()
            .position(|id| *id == "background_opacity")
            .unwrap();
        assert_eq!(
            &ids[at..at + 4],
            [
                "background_opacity",
                "background_image",
                "background_image_mode",
                "background_image_opacity"
            ]
        );
        for id in [
            "background_image",
            "background_image_mode",
            "background_image_opacity",
        ] {
            let option = option(id).unwrap();
            assert_eq!(option.group, Group::Terminal, "{id}");
            assert_eq!(option.section, Section::Background, "{id}");
            assert_eq!(option.reload_scope(), ReloadScope::Live, "{id}");
        }
        assert_eq!(
            option("background_image").unwrap().path,
            "terminal.background_image.path"
        );
        assert_eq!(
            option("background_image_mode").unwrap().path,
            "terminal.background_image.mode"
        );
        assert_eq!(
            option("background_image_opacity").unwrap().path,
            "terminal.background_image.opacity"
        );
    }

    #[test]
    fn the_image_controls_are_a_text_field_a_choice_of_three_and_the_same_number_as_the_background()
    {
        assert_eq!(option("background_image").unwrap().control, Control::Text);
        assert_eq!(
            option("background_image_mode").unwrap().control,
            Control::Choice(&["stretch", "tile", "center"])
        );
        // Mesmo passo e mesma precisão de `background_opacity`.
        assert_eq!(
            option("background_image_opacity").unwrap().control,
            option("background_opacity").unwrap().control
        );
    }

    #[test]
    fn the_image_defaults_are_the_config_defaults() {
        assert_eq!(
            option("background_image").unwrap().default_value(),
            string("")
        );
        assert_eq!(
            option("background_image_mode").unwrap().default_value(),
            string("stretch")
        );
        assert_eq!(
            option("background_image_opacity").unwrap().default_value(),
            EditValue::Float(1.0)
        );
    }

    /// `0.35` no arquivo é `f32`; a tela tem de lê-lo como `0.35`, não como
    /// `0.3499999940...`, ou o valor digitado pareceria pendente.
    #[test]
    fn the_image_opacity_reads_back_as_written() {
        let option = option("background_image_opacity").unwrap();
        for written in [0.35, 0.05, 0.6, 0.95, 1.0, 0.0, 0.123456] {
            let config = config_with(option, &EditValue::Float(written)).expect("valor na faixa");
            assert_eq!(option.read(&config), EditValue::Float(written), "{written}");
        }
    }

    #[test]
    fn options_follow_the_group_order_of_the_guide() {
        let mut last = 0;
        for option in OPTIONS {
            let index = Group::ALL
                .iter()
                .position(|group| *group == option.group)
                .unwrap();
            assert!(index >= last, "{} fora da ordem dos grupos", option.id);
            last = index;
        }
    }

    #[test]
    fn ids_and_paths_are_unique() {
        let ids: HashSet<_> = OPTIONS.iter().map(|option| option.id).collect();
        let paths: HashSet<_> = OPTIONS.iter().map(|option| option.path).collect();
        assert_eq!(ids.len(), OPTIONS.len());
        assert_eq!(paths.len(), OPTIONS.len());
    }

    /// Toda entrada aponta para uma chave que existe: um `Set` dela sobre o
    /// arquivo de exemplo passa pelo `parse` sem chave desconhecida, e o
    /// `Config` que sai tem o valor posto -- o que também prova que `read`
    /// lê a mesma chave que `path` escreve.
    #[test]
    fn every_entry_points_to_a_key_that_exists() {
        let document = ConfigDocument::parse(EXAMPLE).unwrap();
        for option in OPTIONS {
            let probe = probe_value(option);
            assert_ne!(
                probe,
                option.default_value(),
                "{}: a sondagem não difere do padrão",
                option.id
            );
            let (text, unknown) = document
                .apply_checked(&[Edit::Set(option.key_path(), probe.clone())])
                .unwrap_or_else(|err| panic!("{}: {err:?}", option.id));
            assert!(
                unknown.is_empty(),
                "{}: chave desconhecida {unknown:?}",
                option.id
            );
            let (config, _) = porecatu_config::parse(&text).unwrap();
            assert_eq!(option.read(&config), probe, "{}", option.id);
        }
    }

    #[test]
    fn every_default_is_in_the_range_of_its_option() {
        for option in OPTIONS {
            if let Control::Number { min, max, .. } = option.control {
                let default = number_of(&option.default_value()).unwrap();
                assert!(
                    (min..=max).contains(&default),
                    "{}: o padrão {default} está fora de {min}..={max}",
                    option.id
                );
            }
            if let Control::Choice(choices) = option.control {
                let EditValue::String(default) = option.default_value() else {
                    panic!("{}: escolha sem texto", option.id);
                };
                assert!(choices.contains(&default.as_str()), "{}", option.id);
            }
        }
    }

    #[test]
    fn choices_are_exactly_the_values_the_config_accepts() {
        let option = option("cursor_shape").unwrap();
        let Control::Choice(choices) = option.control else {
            panic!()
        };
        for choice in choices {
            let config = config_with(option, &string(choice))
                .unwrap_or_else(|| panic!("{choice} é recusado pelo parse"));
            assert_eq!(option.read(&config), string(choice));
        }
        assert!(config_with(option, &string("triangle")).is_none());
    }

    #[test]
    fn every_option_has_its_phrases() {
        // O acessor devolve o identificador quando a frase falta (RF-15.11),
        // e um catálogo vazio não tem frase nenhuma -- então, com ele, cada
        // acessor tem de devolver o próprio identificador, distinto dos
        // demais. Pega acessor ligado à frase errada; que as frases existem
        // nos cinco arquivos é o `tests/locales.rs`.
        let empty = Catalog::default();
        let mut seen = HashSet::new();
        for option in OPTIONS {
            let label = (option.label)(&empty);
            let description = (option.description)(&empty);
            assert!(label.starts_with("settings.option."), "{label}");
            assert!(label.ends_with("_label"), "{label}");
            assert!(description.ends_with("_description"), "{description}");
            assert!(label.contains(option.id), "{} -> {label}", option.id);
            assert!(
                description.contains(option.id),
                "{} -> {description}",
                option.id
            );
            assert!(seen.insert(label));
            assert!(seen.insert(description));
        }
    }

    #[test]
    fn reload_scope_of_shell_program_is_new_tab() {
        assert_eq!(
            option("shell_program").unwrap().reload_scope(),
            ReloadScope::NewTab
        );
    }

    #[test]
    fn reload_scope_of_font_size_is_live() {
        assert_eq!(
            option("font_size").unwrap().reload_scope(),
            ReloadScope::Live
        );
    }

    #[test]
    fn reload_scope_matches_diff_for_every_option() {
        // As quatro classes de `diff`, cada uma com um exemplo.
        let scope = |id: &str| option(id).unwrap().reload_scope();
        assert_eq!(scope("window_opacity"), ReloadScope::NextWindow);
        assert_eq!(scope("decorations"), ReloadScope::Restart);
        assert_eq!(scope("tab_bar_position"), ReloadScope::Restart);
        assert_eq!(scope("scrollback_lines"), ReloadScope::NewTab);
        assert_eq!(scope("session_enabled"), ReloadScope::Restart);
        assert_eq!(scope("project_file_enabled"), ReloadScope::Restart);
        assert_eq!(scope("git_remote_poll"), ReloadScope::Live);
        assert_eq!(scope("min_columns"), ReloadScope::Live);
        assert_eq!(scope("status_bar_enabled"), ReloadScope::Live);
        assert_eq!(scope("language"), ReloadScope::Live);
        // E o cache devolve o mesmo que a sondagem direta, para toda opção.
        for option in OPTIONS {
            assert_eq!(option.reload_scope(), probe_scope(option), "{}", option.id);
        }
    }
}
