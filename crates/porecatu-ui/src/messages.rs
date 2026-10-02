// SPDX-License-Identifier: GPL-3.0-or-later

//! Frases de interface (ADR-0056). Todo texto que chega a uma superfície --
//! aviso, diálogo, nota no grid, rótulo, leitor de tela -- sai daqui e só
//! daqui, lido do catálogo de idioma.
//!
//! Duas camadas:
//! - o **registro** (`msg`, mais abaixo): cada identificador pontilhado, seus
//!   marcadores e se é plural, num lugar só, de onde saem os acessores tipados
//!   e o esquema que `porecatu-locale` valida;
//! - as funções compostas (`config_error`, `keymap_issue`, ...): casam a
//!   variante de um erro tipado dos crates de baixo e escolhem a frase do
//!   registro. Nenhuma chama `to_string()` num erro de outro crate
//!   (ADR-0056 §2).
//!
//! Três tipos de texto entram nas frases sem tradução, e é de propósito:
//! - o `detail` do crate `toml` e o do compilador de regex (detalhe técnico,
//!   mostrado como chegou);
//! - a `cause` do sistema operacional (`io::Error`, PTY, `git`), idem;
//! - o que o usuário escreveu (nome de tema, de tecla, de ação, de sessão) e o
//!   que um programa escreveu (título de aba, URI).

use std::fmt::Display;
use std::path::Path;

use porecatu_config::{ConfigError, ConfigErrorKind};
use porecatu_core::ActionParseError;
use porecatu_locale::{Catalog, MessageSpec, Schema};
use porecatu_session::CURRENT_SCHEMA_VERSION;
use porecatu_session::named::SaveError;
use porecatu_term::TerminalSpawnError;

use crate::SaveNamedFailure;
use crate::keymap::{ChordParseError, KeymapIssue};

/// Texto de uma falha que veio do sistema operacional ou de uma biblioteca
/// externa (`io::Error`, `opener`), como chegou. É o `{cause}` do ADR-0056:
/// nunca traduzido, porque quem o escreveu foi o SO.
pub(crate) fn os_cause(err: &dyn Display) -> String {
    err.to_string()
}

/// Corpo do aviso de config inválida. Posição só existe para erro de sintaxe
/// do TOML (`ConfigError::at`); leitura e nome de tema duplicado não têm
/// linha nem coluna.
pub(crate) fn config_error(catalog: &Catalog, error: &ConfigError) -> String {
    match (&error.kind, error.line, error.column) {
        (ConfigErrorKind::Toml { detail }, Some(line), Some(column)) => {
            msg::notice::config_invalid::body_at(catalog, line, column, detail)
        }
        (ConfigErrorKind::Toml { detail }, _, _) => {
            msg::notice::config_invalid::body(catalog, detail)
        }
        (ConfigErrorKind::Unreadable { path, cause }, _, _) => {
            msg::notice::config_invalid::unreadable(catalog, path.display(), cause)
        }
        (ConfigErrorKind::DuplicateThemeName { name }, _, _) => {
            msg::notice::config_invalid::duplicate_theme(catalog, name)
        }
    }
}

/// Corpo do aviso de Salvar que não gravou (RF-16.19): o texto que o
/// carregador recusaria cita linha e coluna como o aviso de config inválida, a
/// falha de disco cita a causa do sistema, e o resto diz por que a edição não
/// se aplica.
pub(crate) fn settings_save_error(
    catalog: &Catalog,
    error: &crate::settings::save::SaveError,
) -> String {
    use crate::settings::save::SaveError as Failure;
    use porecatu_config::EditError;

    match error {
        Failure::Read { path, cause } => {
            msg::notice::config_invalid::unreadable(catalog, path.display(), cause)
        }
        Failure::Edit(edit) => match edit {
            EditError::Invalid(error) => config_error(catalog, error),
            EditError::Syntax {
                line: Some(line),
                column: Some(column),
                detail,
            } => msg::notice::config_invalid::body_at(catalog, line, column, detail),
            EditError::Syntax { detail, .. } => msg::notice::config_invalid::body(catalog, detail),
            EditError::Changed { .. } => msg::notice::settings_save_failed::changed(catalog),
            EditError::Io { path, cause, .. } => {
                msg::notice::settings_save_failed::io(catalog, path.display(), cause)
            }
            EditError::NotATable { path } | EditError::NotAValue { path } => {
                msg::notice::settings_save_failed::structure(catalog, path)
            }
            // Erros de quem monta a edição, nunca do usuário.
            EditError::EmptyKeyPath | EditError::InvalidKeyPath { .. } => {
                msg::notice::settings_save_failed::structure(catalog, edit)
            }
        },
    }
}

/// Nota no grid de uma aba cujo `.porecatu` existe, é de diretório
/// autorizado e não pôde ser lido. `reason` é o texto do `io::Error`.
pub(crate) fn project_file_unreadable(catalog: &Catalog, path: &Path, reason: &str) -> String {
    msg::note::project_file_unreadable(catalog, path.display(), reason)
}

/// Corpo do aviso "Falha ao iniciar terminal".
pub(crate) fn terminal_spawn_error(catalog: &Catalog, error: &TerminalSpawnError) -> String {
    match error {
        TerminalSpawnError::Pty(err) => {
            msg::notice::spawn_failed::body(catalog, err.kind().as_str(), err.cause())
        }
    }
}

/// Corpo do aviso de falha ao salvar sessão nomeada.
pub(crate) fn save_named_failure(catalog: &Catalog, failure: &SaveNamedFailure) -> String {
    match failure {
        SaveNamedFailure::WindowNotFound => msg::notice::save_failed::window_not_found(catalog),
        SaveNamedFailure::Save(SaveError::Io(err)) => {
            msg::notice::save_failed::io(catalog, os_cause(err))
        }
        SaveNamedFailure::Save(SaveError::NewerSchema { found }) => {
            msg::notice::save_failed::newer_schema(catalog, found, CURRENT_SCHEMA_VERSION)
        }
        SaveNamedFailure::Save(SaveError::EmptyName) => {
            msg::notice::save_failed::empty_name(catalog)
        }
    }
}

/// Corpo do aviso "Keybinding inválido".
pub(crate) fn keymap_issue(catalog: &Catalog, issue: &KeymapIssue) -> String {
    use msg::notice::keybinding_invalid as kb;
    match issue {
        KeymapIssue::MalformedKey(ChordParseError::EmptyKey { text }) => {
            kb::empty_key(catalog, text)
        }
        KeymapIssue::MalformedKey(ChordParseError::UnknownModifier { modifier, text }) => {
            kb::unknown_modifier(catalog, modifier, text)
        }
        KeymapIssue::MalformedKey(ChordParseError::UnknownKey { key, text }) => {
            kb::unknown_key(catalog, key, text)
        }
        KeymapIssue::DuplicateBinding { keys } => {
            let joiner = kb::join_and(catalog);
            let quoted: Vec<String> = keys.iter().map(|k| format!("\"{k}\"")).collect();
            kb::duplicate(catalog, quoted.join(&joiner))
        }
        KeymapIssue::InvalidAction {
            key,
            error: ActionParseError::Unknown { input, suggestion },
        } => kb::action_unknown(catalog, key, input, suggestion),
        KeymapIssue::InvalidAction {
            key,
            error: ActionParseError::NotBindable { input },
        } => kb::action_not_bindable(catalog, key, input),
    }
}

/// Substitui o modelo da frase `id` no catálogo. Frase ausente devolve o
/// **identificador** (RF-15.11): o defeito aparece na tela em vez de sumir.
/// `count` escolhe `one`/`other` numa frase de plural e é ignorado numa
/// simples.
pub(crate) fn render(
    catalog: &Catalog,
    id: &'static str,
    count: Option<u64>,
    args: &[(&str, String)],
) -> String {
    let Some(message) = catalog.get(id) else {
        return id.to_owned();
    };
    let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    porecatu_locale::format(message.select(count.unwrap_or(0)), &args)
}

/// Registro de mensagens (ADR-0056 §1). Cada linha declara uma frase:
///
/// - `nome()` é uma frase simples, sem marcadores;
/// - `nome(a, b)` tem os marcadores `{a}` e `{b}`, e o acessor recebe um
///   argumento por marcador -- esquecer um é erro de compilação, não um
///   `{a}` cru na tela;
/// - `nome(plural)` e `nome(plural, a)` são frases de plural: o acessor
///   recebe `count: usize` primeiro, que escolhe `one`/`other` e preenche
///   `{count}`.
///
/// Uma tabela pode ter tabelas dentro (`dialog { close_tab { .. }, }`), até
/// o teto de dois níveis do formato. O identificador é o caminho pontilhado
/// (`dialog.close_tab.title`), e o mesmo registro gera o [`schema`].
///
/// Toda entrada termina em vírgula, menos as tabelas de primeiro nível.
macro_rules! registry {
    ( $( $table:ident { $($body:tt)* } )* ) => {
        /// Acessores tipados: `msg::tab_menu::close(&catalog)`.
        pub(crate) mod msg {
            $(
                pub(crate) mod $table {
                    registry!(@items [$table] $($body)*);
                }
            )*
        }

        /// O que o app declara ao `porecatu-locale`: todo identificador,
        /// com marcadores e plural.
        pub(crate) fn schema() -> Schema {
            #[allow(unused_mut)]
            let mut schema = Schema::new();
            $( registry!(@schema schema [$table] $($body)*); )*
            schema
        }
    };

    (@items [$($pfx:ident)*]) => {};
    (@items [$($pfx:ident)*] $name:ident ( plural $(, $arg:ident)* ) , $($rest:tt)*) => {
        pub(crate) fn $name(
            catalog: &::porecatu_locale::Catalog,
            count: usize
            $(, $arg: impl ::std::fmt::Display)*
        ) -> String {
            crate::messages::render(
                catalog,
                concat!($(stringify!($pfx), ".",)* stringify!($name)),
                Some(count as u64),
                &[
                    ("count", count.to_string())
                    $(, (stringify!($arg), $arg.to_string()))*
                ],
            )
        }
        registry!(@items [$($pfx)*] $($rest)*);
    };
    (@items [$($pfx:ident)*] $name:ident ( $($arg:ident),* ) , $($rest:tt)*) => {
        pub(crate) fn $name(
            catalog: &::porecatu_locale::Catalog
            $(, $arg: impl ::std::fmt::Display)*
        ) -> String {
            crate::messages::render(
                catalog,
                concat!($(stringify!($pfx), ".",)* stringify!($name)),
                None,
                &[ $( (stringify!($arg), $arg.to_string()) ),* ],
            )
        }
        registry!(@items [$($pfx)*] $($rest)*);
    };
    (@items [$($pfx:ident)*] $sub:ident { $($body:tt)* } , $($rest:tt)*) => {
        pub(crate) mod $sub {
            registry!(@items [$($pfx)* $sub] $($body)*);
        }
        registry!(@items [$($pfx)*] $($rest)*);
    };

    (@schema $schema:ident [$($pfx:ident)*]) => {};
    (@schema $schema:ident [$($pfx:ident)*] $name:ident ( plural $(, $arg:ident)* ) , $($rest:tt)*) => {
        $schema.insert(
            concat!($(stringify!($pfx), ".",)* stringify!($name)),
            MessageSpec::plural(&["count" $(, stringify!($arg))*]),
        );
        registry!(@schema $schema [$($pfx)*] $($rest)*);
    };
    (@schema $schema:ident [$($pfx:ident)*] $name:ident ( $($arg:ident),* ) , $($rest:tt)*) => {
        $schema.insert(
            concat!($(stringify!($pfx), ".",)* stringify!($name)),
            MessageSpec::simple(&[ $( stringify!($arg) ),* ]),
        );
        registry!(@schema $schema [$($pfx)*] $($rest)*);
    };
    (@schema $schema:ident [$($pfx:ident)*] $sub:ident { $($body:tt)* } , $($rest:tt)*) => {
        registry!(@schema $schema [$($pfx)* $sub] $($body)*);
        registry!(@schema $schema [$($pfx)*] $($rest)*);
    };
}

registry! {
    tab_menu {
        new(),
        close(),
        move_to_group(),
    }
    terminal_menu {
        copy(),
        paste(),
        select_all(),
        search(),
        open_link(),
        copy_link(),
    }
    group_menu {
        rename(),
        set_color(),
        collapse(),
        expand(),
        new_tab(),
        close(plural),
        dissolve(),
    }
    group_editor {
        section_group(),
        section_color(),
        default_name(),
    }
    move_to_group {
        new_group(),
    }
    session_picker {
        save_item(),
        empty_list(),
        name_placeholder(),
    }
    settings {
        window_title(),
        group {
            general(),
            shell(),
            terminal(),
            appearance(),
            session(),
            project(),
            git(),
            panes(),
            shortcuts(),
        },
        section {
            language(),
            startup(),
            confirmations(),
            program(),
            environment(),
            font(),
            cursor(),
            scrollback(),
            selection(),
            clipboard(),
            links(),
            background(),
            theme(),
            window(),
            tabs(),
            status_bar(),
            restore(),
            project_file(),
            git_remote(),
            panes_size(),
        },
        option {
            language_label(),
            language_description(),
            startup_directory_label(),
            startup_directory_description(),
            confirm_close_with_process_label(),
            confirm_close_with_process_description(),
            confirm_close_window_label(),
            confirm_close_window_description(),
            shell_program_label(),
            shell_program_description(),
            shell_args_label(),
            shell_args_description(),
            shell_env_label(),
            shell_env_description(),
            font_family_label(),
            font_family_description(),
            font_size_label(),
            font_size_description(),
            line_height_label(),
            line_height_description(),
            letter_spacing_label(),
            letter_spacing_description(),
            bold_is_bright_label(),
            bold_is_bright_description(),
            cursor_shape_label(),
            cursor_shape_description(),
            cursor_blink_label(),
            cursor_blink_description(),
            cursor_follows_group_color_label(),
            cursor_follows_group_color_description(),
            cursor_unfocused_hollow_label(),
            cursor_unfocused_hollow_description(),
            scrollback_lines_label(),
            scrollback_lines_description(),
            scroll_multiplier_label(),
            scroll_multiplier_description(),
            scroll_on_output_label(),
            scroll_on_output_description(),
            scroll_on_input_label(),
            scroll_on_input_description(),
            alternate_scroll_label(),
            alternate_scroll_description(),
            copy_on_select_label(),
            copy_on_select_description(),
            word_separators_label(),
            word_separators_description(),
            osc52_write_label(),
            osc52_write_description(),
            osc52_read_label(),
            osc52_read_description(),
            osc52_max_bytes_label(),
            osc52_max_bytes_description(),
            hyperlinks_enabled_label(),
            hyperlinks_enabled_description(),
            background_opacity_label(),
            background_opacity_description(),
            theme_label(),
            theme_description(),
            animations_label(),
            animations_description(),
            window_opacity_label(),
            window_opacity_description(),
            decorations_label(),
            decorations_description(),
            tab_bar_position_label(),
            tab_bar_position_description(),
            show_close_button_label(),
            show_close_button_description(),
            show_index_label(),
            show_index_description(),
            show_activity_indicator_label(),
            show_activity_indicator_description(),
            show_bell_indicator_label(),
            show_bell_indicator_description(),
            show_new_tab_button_label(),
            show_new_tab_button_description(),
            hide_when_single_tab_label(),
            hide_when_single_tab_description(),
            status_bar_enabled_label(),
            status_bar_enabled_description(),
            session_enabled_label(),
            session_enabled_description(),
            lazy_restore_label(),
            lazy_restore_description(),
            restore_window_geometry_label(),
            restore_window_geometry_description(),
            suggest_shell_integration_label(),
            suggest_shell_integration_description(),
            project_file_enabled_label(),
            project_file_enabled_description(),
            trusted_paths_label(),
            trusted_paths_description(),
            trusted_paths_warning(),
            git_remote_poll_label(),
            git_remote_poll_description(),
            min_columns_label(),
            min_columns_description(),
            min_rows_label(),
            min_rows_description(),
        },
        button {
            open_file(),
            discard(),
            save(),
            restore_default(),
            add_item(),
            remove_item(),
        },
        scope {
            new_tab(),
            next_window(),
            restart(),
            next_open(),
        },
        banner {
            file_changed(),
            reload(),
            keep_mine(),
            invalid_file(),
        },
        choice {
            block(),
            beam(),
            underline(),
            always(),
            hover(),
            never(),
            top(),
            bottom(),
        },
        dialog {
            close_title(),
            close_body(plural),
            save_and_close(),
            discard_and_close(),
            cancel(),
        },
        validation {
            out_of_range(min, max),
            not_a_number(),
            not_an_integer(),
            not_a_choice(),
            empty_name(),
            duplicate_name(name),
        },
        theme {
            none(),
            session_using(name),
        },
        shortcut {
            none(),
            press_keys(),
            conflict(action),
            replace(),
            filter_placeholder(),
            reserved(chord),
        },
    }
    action {
        tab_new(),
        tab_close(),
        tab_next(),
        tab_prev(),
        tab_goto(n),
        tab_rename(),
        tab_move_left(),
        tab_move_right(),
        group_create(),
        group_dissolve(),
        group_rename(),
        group_toggle_collapse(),
        group_next(),
        group_prev(),
        group_new_tab(),
        group_close_all(),
        window_new(),
        window_close(),
        window_toggle_fullscreen(),
        scrollback_line_up(),
        scrollback_line_down(),
        scrollback_page_up(),
        scrollback_page_down(),
        scrollback_to_top(),
        scrollback_to_bottom(),
        clipboard_copy(),
        clipboard_paste(),
        selection_select_all(),
        font_increase(),
        font_decrease(),
        font_reset(),
        theme_cycle(),
        config_reload(),
        settings_open(),
        search_open(),
        search_next(),
        search_prev(),
        app_quit(),
        pane_split_horizontal(),
        pane_split_vertical(),
        pane_close(),
        pane_focus_left(),
        pane_focus_right(),
        pane_focus_up(),
        pane_focus_down(),
        session_save_named(),
        session_open_list(),
    }
    action_domain {
        tab(),
        group(),
        window(),
        session(),
        pane(),
        scrollback(),
        clipboard(),
        selection(),
        font(),
        theme(),
        config(),
        settings(),
        search(),
        app(),
    }
    search_bar {
        invalid_pattern(),
        no_results(),
        alt_screen_counter(counter),
    }
    status_bar {
        pane_count(plural),
        commits_behind(plural),
        behind_ahead(behind, ahead),
    }
    color {
        red(),
        yellow(),
        cyan(),
        blue(),
        purple(),
        green(),
    }
    dialog {
        cancel(),
        close_tab {
            title(),
            body(title),
            confirm(),
        },
        close_pane {
            title(),
            body(title),
            confirm(),
        },
        close_window {
            title(),
            body_tabs(),
            body_program(),
            confirm(),
        },
        close_group {
            title(),
            body(plural),
            confirm(plural),
        },
        delete_session {
            title(),
            body(name),
            confirm(),
        },
        overwrite_session {
            title(name),
            body(),
            confirm(),
        },
    }
    note {
        process_exited(code),
        cwd_missing(path),
        project_file_untrusted(name, dir, toml_value),
        project_file_unreadable(path, reason),
        shell_invite {
            text(consequence, body, marker),
            consequence_windows(),
            consequence_other(),
            body_snippet(snippet),
            body_cmd(),
            body_generic(),
        },
    }
    access {
        new_tab_ungrouped(),
        new_tab_in_group(),
        sessions_button(),
        settings_button(),
        window_minimize(),
        window_maximize(),
        window_close(),
        tab_close(),
        regex_toggle(),
        group_editor(),
        unnamed_group(),
        tabs_hidden_left(plural),
        tabs_hidden_right(plural),
        group(name),
        group_collapsed(name),
        group_colored(name, color),
        group_collapsed_colored(name, color),
        tab_state(state),
        state_active(),
        state_not_started(),
        state_bell(),
        state_activity(),
        pane_focused(title),
        cwd_stale(),
        ahead_behind_clickable(),
        ahead_behind_blocked(),
        segment_shell(),
        segment_cwd(),
        segment_branch(),
        segment_ahead_behind(),
        segment_group(),
        segment_pane_count(),
        segment_encoding(),
        segment_system(),
        severity_error(),
        severity_warning(),
        severity_info(),
        warning(severity, title, body),
        settings_groups(),
        settings_panel(),
    }
    notice {
        config_invalid {
            title(),
            body_at(line, column, detail),
            body(detail),
            unreadable(path, cause),
            duplicate_theme(name),
        },
        unknown_config_key {
            title(),
        },
        unknown_theme {
            title(),
            body(name),
        },
        color_overridden {
            title(),
        },
        theme_vanished {
            title(),
            body_reload(name),
            body_start(name),
        },
        keybinding_invalid {
            title(),
            join_and(),
            empty_key(text),
            unknown_modifier(modifier, text),
            unknown_key(key, text),
            duplicate(keys),
            action_unknown(key, input, suggestion),
            action_not_bindable(key, input),
        },
        deferred {
            title(),
            next_window(key),
            new_tab(key),
            restart(key),
        },
        config_path_unresolved {
            title(),
            body(),
        },
        config_create_failed {
            title(),
        },
        config_open_failed {
            title(),
        },
        settings_save_failed {
            title(),
            changed(),
            io(path, cause),
            structure(path),
        },
        link_open_failed {
            title(),
        },
        link_reveal_failed {
            title(),
            body(uri),
        },
        link_refused {
            title(),
            body(scheme),
        },
        spawn_failed {
            title(),
            body(operation, cause),
        },
        pane_too_small {
            title(),
            body(columns, rows),
        },
        session_corrupt {
            title(),
            body(path),
        },
        session_newer {
            title(),
            body(found, supported),
        },
        named_session_corrupt {
            title(),
            body(path),
        },
        named_session_newer {
            title(),
            body(found, supported),
        },
        named_session_unreadable {
            title(),
            body(),
        },
        overwrite_blocked {
            title(),
            body(name),
        },
        save_failed {
            title(),
            window_not_found(),
            io(cause),
            newer_schema(found, supported),
            empty_name(),
        },
        delete_failed {
            title(),
        },
        software_rendering {
            title(),
            body(),
        },
        font_not_found {
            title(),
            body(family),
        },
        git_interval_adjusted {
            title(),
            body(key, floor),
        },
        git_missing {
            title(),
            body(),
        },
        git_integration_ok {
            title(),
            body(),
        },
        git_integration_failed {
            title(),
            missing(),
            timed_out(),
            no_detail(),
        },
        language_not_found {
            title(),
            body(language, searched),
        },
        language_invalid_name {
            title(),
            body(value),
        },
        language_syntax {
            title(),
            body(path, detail),
            body_at(path, line, column, detail),
        },
        language_unreadable {
            title(),
            body(path, cause),
        },
        language_missing_messages {
            title(),
            body(plural, locale),
        },
        language_unknown_keys {
            title(),
            body(plural, path),
        },
    }
}

/// Auxiliar dos testes de frase (ADR-0056 §10): carrega os arquivos de
/// `locales/` do repositório -- o mesmo par que o app lê em disco --, para
/// que um teste que compara uma frase compare a que o usuário vê, e não uma
/// cópia dela escrita no teste.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    use porecatu_locale::{Catalog, parse_layer};

    use super::schema;

    fn locales_dir() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales"))
    }

    fn layer(locale: &str) -> porecatu_locale::Messages {
        let path = locales_dir().join(format!("{locale}.toml"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
        let outcome = parse_layer(&text, &schema());
        if let Some(error) = outcome.syntax_error {
            panic!("{}: {error:?}", path.display());
        }
        outcome.messages
    }

    /// `en_US` por baixo, `locale` por cima -- a mesma mescla do app.
    pub(crate) fn catalog(locale: &str) -> Catalog {
        Catalog::from_layers([layer("en_US"), layer(locale)])
    }

    pub(crate) fn pt_br() -> Catalog {
        catalog("pt_BR")
    }

    pub(crate) fn en_us() -> Catalog {
        catalog("en_US")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use porecatu_term::{PtySize, SpawnConfig, TermParams, Terminal};

    use super::*;

    fn config_error_of(kind: ConfigErrorKind, at: Option<(usize, usize)>) -> ConfigError {
        match at {
            Some((line, column)) => ConfigError::at(line, column, kind),
            None => ConfigError::new(kind),
        }
    }

    #[test]
    fn the_schema_declares_placeholders_and_plural_for_every_id() {
        let schema = schema();
        let close = schema["tab_menu.close"];
        assert!(!close.plural);
        assert!(close.placeholders.is_empty());

        let group_close = schema["group_menu.close"];
        assert!(group_close.plural);
        assert_eq!(group_close.placeholders, ["count"]);

        // Tabelas com tabelas dentro: o identificador é o caminho pontilhado.
        assert_eq!(schema["dialog.close_tab.body"].placeholders, ["title"]);
        assert_eq!(
            schema["dialog.overwrite_session.title"].placeholders,
            ["name"]
        );
        assert_eq!(schema["dialog.cancel"].placeholders.len(), 0);
        assert_eq!(
            schema["notice.language_syntax.body_at"].placeholders,
            ["path", "line", "column", "detail"]
        );
        let missing = schema["notice.language_missing_messages.body"];
        assert!(missing.plural);
        assert_eq!(missing.placeholders, ["count", "locale"]);
        // Plural com marcador extra, e frase de uma tabela de primeiro nível
        // dentro de `access`.
        assert!(schema["access.tabs_hidden_left"].plural);
        assert_eq!(
            schema["access.warning"].placeholders,
            ["severity", "title", "body"]
        );
    }

    #[test]
    fn a_phrase_missing_from_the_catalog_is_its_identifier() {
        let empty = Catalog::new();
        assert_eq!(msg::tab_menu::close(&empty), "tab_menu.close");
        assert_eq!(msg::group_menu::close(&empty, 3), "group_menu.close");
        assert_eq!(
            msg::dialog::close_tab::body(&empty, "vim"),
            "dialog.close_tab.body"
        );
    }

    /// O valor de um marcador vem de fora (um título de aba, que vem de um
    /// programa) e nunca é lido de novo como modelo (ADR-0056 §5).
    #[test]
    fn placeholder_values_are_substituted_literally_and_never_expanded() {
        let pt = test_support::pt_br();
        assert_eq!(
            msg::dialog::close_tab::body(&pt, "{count} {title}"),
            "\"{count} {title}\" tem um programa em primeiro plano. Fechar mesmo assim?"
        );
    }

    #[test]
    fn plural_uses_one_only_for_exactly_one() {
        for (catalog, one, other) in [
            (
                test_support::pt_br(),
                "Fechar grupo (1 aba)",
                "Fechar grupo (2 abas)",
            ),
            (
                test_support::en_us(),
                "Close group (1 tab)",
                "Close group (2 tabs)",
            ),
        ] {
            assert_eq!(msg::group_menu::close(&catalog, 1), one);
            assert_eq!(msg::group_menu::close(&catalog, 2), other);
            assert!(msg::group_menu::close(&catalog, 0).contains("(0 "));
        }
    }

    #[test]
    fn the_same_accessor_answers_in_the_language_of_the_catalog() {
        assert_eq!(msg::tab_menu::close(&test_support::pt_br()), "Fechar aba");
        assert_eq!(msg::tab_menu::close(&test_support::en_us()), "Close tab");
        assert_eq!(
            msg::group_editor::section_group(&test_support::pt_br()),
            "GRUPO"
        );
        assert_eq!(
            msg::group_editor::section_group(&test_support::en_us()),
            "GROUP"
        );
    }

    #[test]
    fn config_toml_error_carries_position_and_detail() {
        let error = config_error_of(
            ConfigErrorKind::Toml {
                detail: "expected an equals".to_owned(),
            },
            Some((3, 5)),
        );
        assert_eq!(
            config_error(&test_support::pt_br(), &error),
            "linha 3, coluna 5: expected an equals"
        );
        assert_eq!(
            config_error(&test_support::en_us(), &error),
            "line 3, column 5: expected an equals"
        );
    }

    #[test]
    fn config_unreadable_and_duplicate_have_no_position() {
        let pt = test_support::pt_br();
        let unreadable = config_error_of(
            ConfigErrorKind::Unreadable {
                path: PathBuf::from("p.toml"),
                cause: "acesso negado".to_owned(),
            },
            None,
        );
        assert_eq!(
            config_error(&pt, &unreadable),
            "não foi possível ler \"p.toml\": acesso negado"
        );
        let duplicate = config_error_of(
            ConfigErrorKind::DuplicateThemeName {
                name: "x".to_owned(),
            },
            None,
        );
        assert_eq!(
            config_error(&pt, &duplicate),
            "nome de tema duplicado: \"x\""
        );
        assert_eq!(
            config_error(&test_support::en_us(), &duplicate),
            "duplicate theme name: \"x\""
        );
    }

    /// A frase de pt-BR é a que o `Display` do erro sempre escreveu.
    #[test]
    fn config_phrases_in_pt_br_are_the_ones_the_error_displayed_before() {
        let pt = test_support::pt_br();
        for error in [
            config_error_of(
                ConfigErrorKind::Toml {
                    detail: "d".to_owned(),
                },
                Some((1, 2)),
            ),
            config_error_of(
                ConfigErrorKind::Unreadable {
                    path: PathBuf::from("p.toml"),
                    cause: "c".to_owned(),
                },
                None,
            ),
            config_error_of(
                ConfigErrorKind::DuplicateThemeName {
                    name: "n".to_owned(),
                },
                None,
            ),
        ] {
            assert_eq!(config_error(&pt, &error), error.to_string());
        }
    }

    #[test]
    fn save_failure_phrases() {
        let pt = test_support::pt_br();
        assert_eq!(
            save_named_failure(&pt, &SaveNamedFailure::WindowNotFound),
            "janela não encontrada"
        );
        assert_eq!(
            save_named_failure(&pt, &SaveNamedFailure::Save(SaveError::EmptyName)),
            "nome de sessão vazio depois de aparado"
        );
        let newer = save_named_failure(
            &pt,
            &SaveNamedFailure::Save(SaveError::NewerSchema { found: 9 }),
        );
        assert!(newer.starts_with("arquivo existente tem schema_version 9,"));
        let io = save_named_failure(
            &pt,
            &SaveNamedFailure::Save(SaveError::Io(std::io::Error::other("disco cheio"))),
        );
        assert_eq!(io, "erro de E/S ao gravar sessão nomeada: disco cheio");

        let en = test_support::en_us();
        assert_eq!(
            save_named_failure(&en, &SaveNamedFailure::WindowNotFound),
            "window not found"
        );
        let io = save_named_failure(
            &en,
            &SaveNamedFailure::Save(SaveError::Io(std::io::Error::other("disk full"))),
        );
        assert_eq!(io, "I/O error writing the named session: disk full");
    }

    #[test]
    fn save_failure_phrases_in_pt_br_match_the_display_of_the_error() {
        let pt = test_support::pt_br();
        for error in [
            SaveError::EmptyName,
            SaveError::NewerSchema { found: 9 },
            SaveError::Io(std::io::Error::other("disco cheio")),
        ] {
            let display = error.to_string();
            assert_eq!(
                save_named_failure(&pt, &SaveNamedFailure::Save(error)),
                display
            );
        }
    }

    #[test]
    fn action_issue_phrases() {
        let unknown = KeymapIssue::InvalidAction {
            key: "ctrl+z".to_owned(),
            error: ActionParseError::Unknown {
                input: "tab.clsoe".to_owned(),
                suggestion: "tab.close",
            },
        };
        assert_eq!(
            keymap_issue(&test_support::pt_br(), &unknown),
            "\"ctrl+z\": ação desconhecida: \"tab.clsoe\" -- você quis dizer \"tab.close\"?"
        );
        assert_eq!(
            keymap_issue(&test_support::en_us(), &unknown),
            "\"ctrl+z\": unknown action: \"tab.clsoe\" -- did you mean \"tab.close\"?"
        );
        let not_bindable = KeymapIssue::InvalidAction {
            key: "ctrl+z".to_owned(),
            error: ActionParseError::NotBindable {
                input: "group.set_color".to_owned(),
            },
        };
        assert_eq!(
            keymap_issue(&test_support::pt_br(), &not_bindable),
            "\"ctrl+z\": \"group.set_color\" tem argumento e não é vinculável a tecla"
        );
    }

    #[test]
    fn chord_issue_phrases() {
        let pt = test_support::pt_br();
        let issue = KeymapIssue::MalformedKey(ChordParseError::UnknownKey {
            key: "bogus".to_owned(),
            text: "ctrl+bogus".to_owned(),
        });
        assert_eq!(
            keymap_issue(&pt, &issue),
            "tecla desconhecida: \"bogus\" em \"ctrl+bogus\""
        );
        let empty = KeymapIssue::MalformedKey(ChordParseError::EmptyKey {
            text: "x".to_owned(),
        });
        assert_eq!(keymap_issue(&pt, &empty), "tecla vazia: \"x\"");
        let duplicate = KeymapIssue::DuplicateBinding {
            keys: vec!["a".to_owned(), "b".to_owned()],
        };
        assert_eq!(
            keymap_issue(&pt, &duplicate),
            "binding duplicado: \"a\" e \"b\" resolvem pra mesma tecla"
        );
        assert_eq!(
            keymap_issue(&test_support::en_us(), &duplicate),
            "duplicate binding: \"a\" and \"b\" resolve to the same key"
        );
    }

    #[test]
    fn terminal_spawn_phrase_names_the_operation_and_keeps_the_cause() {
        // `PtyError::new` is crate-private to `porecatu-pty`; a real failed
        // spawn is the way to get one.
        let result = Terminal::spawn(
            SpawnConfig {
                program: Some("porecatu-no-such-program-xyz".to_owned()),
                args: Vec::new(),
                env: Vec::new(),
                cwd: None,
                size: PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
            },
            TermParams::default(),
            || {},
        );
        let Err(error) = result else {
            panic!("spawning a program that does not exist should fail");
        };
        for catalog in [test_support::pt_br(), test_support::en_us()] {
            let phrase = terminal_spawn_error(&catalog, &error);
            assert!(
                phrase.starts_with("terminal: pty: spawn_command: "),
                "{phrase}"
            );
            assert_eq!(phrase, error.to_string());
        }
    }
}
