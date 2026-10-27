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

use porecatu_config::{
    ConfigError, ConfigErrorKind, ProfileProblem, ProfileProblemKind, ThemeFileProblem,
};
use porecatu_core::ActionParseError;
use porecatu_locale::{Catalog, MessageSpec, Schema};
use porecatu_session::CURRENT_SCHEMA_VERSION;
use porecatu_session::named::SaveError;
use porecatu_term::{
    EndReason, FailCause, HostKeyEntry, HostKeyPresented, KnownHostsCause, LostCause,
    TerminalSpawnError,
};

use crate::SaveNamedFailure;
use crate::keymap::{ChordParseError, KeymapIssue};
use crate::warning::Severity;

/// Texto de uma falha que veio do sistema operacional ou de uma biblioteca
/// externa (`io::Error`, `opener`), como chegou. É o `{cause}` do ADR-0056:
/// nunca traduzido, porque quem o escreveu foi o SO.
pub(crate) fn os_cause(err: &dyn Display) -> String {
    err.to_string()
}

/// Corpo do aviso de config inválida. Posição só existe para erro de sintaxe
/// do TOML (`ConfigError::at`); a leitura não tem linha nem coluna.
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
    }
}

/// Título e corpo do aviso de um arquivo de tema com problema (ADR-0067):
/// cita o nome do arquivo, não o caminho inteiro, que o aviso cortaria.
pub(crate) fn theme_file_problem(
    catalog: &Catalog,
    problem: &ThemeFileProblem,
) -> (Severity, String, String) {
    let file_name = |path: &Path| {
        path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    };
    match problem {
        ThemeFileProblem::Unreadable { path, cause } => (
            Severity::Warning,
            msg::notice::theme_file_invalid::title(catalog),
            msg::notice::theme_file_invalid::unreadable(catalog, file_name(path), cause),
        ),
        ThemeFileProblem::Invalid { path, error } => {
            let body = match (&error.kind, error.line, error.column) {
                (ConfigErrorKind::Toml { detail }, Some(line), Some(column)) => {
                    msg::notice::theme_file_invalid::body_at(
                        catalog,
                        file_name(path),
                        line,
                        column,
                        detail,
                    )
                }
                _ => msg::notice::theme_file_invalid::body(
                    catalog,
                    file_name(path),
                    config_error(catalog, error),
                ),
            };
            (
                Severity::Warning,
                msg::notice::theme_file_invalid::title(catalog),
                body,
            )
        }
        ThemeFileProblem::UnknownKeys { path, keys } => (
            Severity::Warning,
            msg::notice::theme_file_unknown_key::title(catalog),
            msg::notice::theme_file_unknown_key::body(catalog, file_name(path), keys.join(", ")),
        ),
    }
}

/// O aviso único das cores que um `porecatu.toml` antigo ainda declara
/// (ADR-0067): um só, com a contagem, em vez de um por chave.
pub(crate) fn colors_ignored(catalog: &Catalog, count: usize) -> (Severity, String, String) {
    (
        Severity::Info,
        msg::notice::colors_ignored::title(catalog),
        msg::notice::colors_ignored::body(catalog, count),
    )
}

/// Aviso de um item de `[[profiles.list]]` recusado, ou de um `default`
/// inexistente (RF-7.5). Cita linha e coluna, como o aviso de config inválida.
pub(crate) fn profile_problem(
    catalog: &Catalog,
    problem: &ProfileProblem,
) -> (Severity, String, String) {
    use msg::notice::profile_reason as reason;
    let reason = match &problem.kind {
        ProfileProblemKind::IdInvalid => reason::id_invalid(catalog),
        ProfileProblemKind::IdReserved => reason::id_reserved(catalog),
        ProfileProblemKind::IdDuplicate { id } => reason::id_duplicate(catalog, id),
        ProfileProblemKind::NameEmpty => reason::name_empty(catalog),
        ProfileProblemKind::BadgeEmpty => reason::badge_empty(catalog),
        ProfileProblemKind::BadgeTooLong => reason::badge_too_long(catalog),
        ProfileProblemKind::TypeUnknown => reason::type_unknown(catalog),
        ProfileProblemKind::ProgramEmpty => reason::program_empty(catalog),
        ProfileProblemKind::DefaultUnknown { id } => reason::default_unknown(catalog, id),
        ProfileProblemKind::SshFieldNotAllowed { field } => reason::ssh_field(catalog, field),
        ProfileProblemKind::HostInvalid => reason::host_invalid(catalog),
        ProfileProblemKind::UserInvalid => reason::user_invalid(catalog),
        ProfileProblemKind::PortInvalid => reason::port_invalid(catalog),
        ProfileProblemKind::AuthInvalid => reason::auth_invalid(catalog),
        ProfileProblemKind::KeyWithoutIdentityFile => reason::key_without_identity_file(catalog),
    };
    (
        Severity::Warning,
        msg::notice::profile_problem::title(catalog),
        msg::notice::profile_problem::body(catalog, problem.line, problem.column, reason),
    )
}

/// Aviso de uma chave de perfil com `password`, `passphrase` ou `secret` no
/// nome (ADR-0072 §6): ignorada como toda chave desconhecida, mas a frase diz
/// que segredo vai no cofre do sistema. O valor nunca foi lido.
/// ADR-0073 §3: a conexão foi aceita, mas o cofre do sistema não guardou o
/// segredo. Aviso do app (canal 1): a nota no grid se perderia na saída do
/// shell.
pub(crate) fn secret_not_saved(catalog: &Catalog) -> (Severity, String, String) {
    (
        Severity::Warning,
        msg::notice::secret_not_saved::title(catalog),
        msg::notice::secret_not_saved::body(catalog),
    )
}

pub(crate) fn secret_profile_key(catalog: &Catalog, key: &str) -> (Severity, String, String) {
    (
        Severity::Warning,
        msg::notice::secret_profile_key::title(catalog),
        msg::notice::secret_profile_key::body(catalog, key),
    )
}

/// A nota de um painel SSH restaurado, que só conecta no primeiro gesto
/// (RF-20.33, ADR-0075 §5): "`Enter` para conectar a `nome`".
pub(crate) fn ssh_dormant_note(catalog: &Catalog, name: &str) -> String {
    msg::note::ssh::dormant(catalog, name)
}

/// A nota "conectando a `host`…" de uma aba SSH recém-aberta (ADR-0075 §5).
pub(crate) fn ssh_connecting_note(catalog: &Catalog, host: &str) -> String {
    msg::note::ssh::connecting(catalog, host)
}

/// Antes de uma tentativa de reconexão (RF-20.28, ADR-0076 §4): a contagem
/// regressiva é **uma frase**, não se reescreve a cada segundo.
pub(crate) fn ssh_reconnect_waiting_note(
    catalog: &Catalog,
    attempt: u8,
    total: u8,
    seconds: u64,
) -> String {
    msg::note::ssh::reconnect_waiting(catalog, attempt, total, seconds)
}

/// A tentativa de reconexão começou.
pub(crate) fn ssh_reconnect_connecting_note(
    catalog: &Catalog,
    attempt: u8,
    total: u8,
    host: &str,
) -> String {
    msg::note::ssh::reconnect_connecting(catalog, attempt, total, host)
}

/// RF-20.29: o processo remoto da sessão perdida não foi retomado.
pub(crate) fn ssh_reconnect_not_resumed_note(catalog: &Catalog) -> String {
    msg::note::ssh::reconnect_not_resumed(catalog)
}

/// Esgotaram as tentativas (RF-20.31).
pub(crate) fn ssh_reconnect_gave_up_note(catalog: &Catalog, total: u8) -> String {
    msg::note::ssh::reconnect_gave_up(catalog, total)
}

fn ssh_entry(catalog: &Catalog, entry: &HostKeyEntry) -> String {
    msg::note::ssh::entry(
        catalog,
        &entry.fingerprint,
        entry.file.display(),
        entry.line,
    )
}

fn ssh_presented(presented: &HostKeyPresented) -> String {
    format!("{} {}", presented.algorithm, presented.fingerprint)
}

/// A nota de uma sessão que acabou por um motivo que não é um processo local
/// (ADR-0071 §3), escrita abaixo do conteúdo (ADR-0075 §5). `None` para
/// `Exited`, que tem a frase de sempre (`note.process_exited`) -- e para o
/// fim limpo (`RemoteEnded` com código 0), que fecha o painel sem nota.
///
/// **Informativa:** nenhuma nota pede ação que mude confiança. A de chave
/// alterada diz as impressões digitais e o arquivo:linha, e para aí
/// (ADR-0075 §5): quem diz como resolver é o OpenSSH, não o Porecatu.
pub(crate) fn ssh_end_note(catalog: &Catalog, host: &str, reason: &EndReason) -> Option<String> {
    use msg::note::ssh;
    Some(match reason {
        EndReason::Exited { .. } => return None,
        EndReason::RemoteEnded { code, signal } => match (code, signal) {
            (Some(0), None) => return None,
            (Some(code), None) => msg::note::process_exited(catalog, code),
            (_, Some(signal)) => ssh::remote_signal(catalog, signal),
            (None, None) => ssh::remote_closed(catalog),
        },
        EndReason::Lost { cause } => match cause {
            LostCause::KeepaliveTimeout => ssh::lost_keepalive(catalog, host),
            LostCause::Transport => ssh::lost_transport(catalog, host),
            LostCause::ServerDisconnect => ssh::lost_disconnect(catalog, host),
        },
        EndReason::Failed { cause } => match cause {
            FailCause::Refused => ssh::refused(catalog, host),
            FailCause::Timeout => ssh::timeout(catalog, host),
            FailCause::AuthRejected => ssh::auth_rejected(catalog, host),
            FailCause::AuthCancelled => ssh::auth_cancelled(catalog),
            FailCause::AuthUnsupported => ssh::auth_unsupported(catalog, host),
            FailCause::KeyUnreadable => ssh::key_unreadable(catalog),
            FailCause::HostKeyRejectedByUser => ssh::host_key_rejected(catalog),
            FailCause::HostKeyChanged {
                presented,
                expected,
            } => {
                let entries: Vec<String> = expected
                    .iter()
                    .map(|entry| ssh_entry(catalog, entry))
                    .collect();
                ssh::host_key_changed(catalog, host, ssh_presented(presented), entries.join("\n"))
            }
            FailCause::HostKeyRevoked { presented, entry } => ssh::host_key_revoked(
                catalog,
                host,
                ssh_presented(presented),
                ssh_entry(catalog, entry),
            ),
            FailCause::KnownHostsUnreadable { file, cause } => {
                let why = match cause {
                    KnownHostsCause::PermissionDenied => ssh::known_hosts_permission(catalog),
                    KnownHostsCause::NotText => ssh::known_hosts_not_text(catalog),
                    KnownHostsCause::Other => ssh::known_hosts_other(catalog),
                };
                ssh::known_hosts_unreadable(catalog, file.display(), why)
            }
            FailCause::ChannelRefused => ssh::channel_refused(catalog, host),
            FailCause::Runtime => ssh::runtime(catalog),
        },
    })
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
            // Um perfil que sumiu do arquivo entre a leitura e a edição: o
            // arquivo mudou por fora, e a mensagem de estrutura diz onde.
            EditError::NoSuchElement { array, .. } => {
                msg::notice::settings_save_failed::structure(catalog, array)
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
        TerminalSpawnError::Stream(err) => {
            msg::notice::spawn_failed::body(catalog, "stream", err.to_string())
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
        reconnect(),
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
            background_image_label(),
            background_image_description(),
            background_image_not_found(path),
            background_image_mode_label(),
            background_image_mode_description(),
            background_image_opacity_label(),
            background_image_opacity_description(),
            theme_label(),
            theme_description(),
            animations_label(),
            animations_description(),
            window_opacity_label(),
            window_opacity_description(),
            window_background_image_label(),
            window_background_image_description(),
            window_background_image_not_found(path),
            window_background_image_mode_label(),
            window_background_image_mode_description(),
            window_background_image_opacity_label(),
            window_background_image_opacity_description(),
            acrylic_enabled_label(),
            acrylic_enabled_description(),
            acrylic_blur_label(),
            acrylic_blur_description(),
            acrylic_requires_window_opacity(),
            acrylic_requires_window_image(),
            decorations_label(),
            decorations_description(),
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
            stretch(),
            tile(),
            center(),
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
        profile_open_menu(),
        profile_new_tab(n),
        ssh_reconnect(),
    }
    profile_menu {
        new_group(),
    }
    action_domain {
        tab(),
        group(),
        window(),
        session(),
        pane(),
        profile(),
        ssh(),
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
        host_key {
            title(host),
            body(),
            type_line(algorithm),
            fingerprint_label(),
            other_type(types),
            skipped_lines(plural),
            confirm(),
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
        secret {
            title_password(target),
            title_passphrase(),
            remember(),
            remember_unavailable(),
            confirm(),
            refused_password(),
            refused_passphrase(),
        },
    }
    note {
        process_exited(code),
        cwd_missing(path),
        profile_missing(id),
        project_file_untrusted(name, dir, toml_value),
        project_file_unreadable(path, reason),
        ssh {
            dormant(name),
            connecting(host),
            refused(host),
            timeout(host),
            auth_rejected(host),
            auth_cancelled(),
            auth_unsupported(host),
            key_unreadable(),
            host_key_rejected(),
            host_key_changed(host, presented, entries),
            host_key_revoked(host, presented, entry),
            entry(fingerprint, file, line),
            known_hosts_unreadable(file, reason),
            known_hosts_permission(),
            known_hosts_not_text(),
            known_hosts_other(),
            channel_refused(host),
            runtime(),
            lost_keepalive(host),
            lost_transport(host),
            lost_disconnect(host),
            remote_signal(signal),
            remote_closed(),
            reconnect_waiting(attempt, total, seconds),
            reconnect_connecting(attempt, total, host),
            reconnect_not_resumed(),
            reconnect_gave_up(total),
        },
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
        },
        unknown_config_key {
            title(),
        },
        unknown_theme {
            title(),
            body(name),
        },
        colors_ignored {
            title(),
            body(count),
        },
        profile_problem {
            title(),
            body(line, column, reason),
        },
        profile_reason {
            id_invalid(),
            id_reserved(),
            id_duplicate(id),
            name_empty(),
            badge_empty(),
            badge_too_long(),
            type_unknown(),
            program_empty(),
            default_unknown(id),
            ssh_field(field),
            host_invalid(),
            user_invalid(),
            port_invalid(),
            auth_invalid(),
            key_without_identity_file(),
        },
        secret_profile_key {
            title(),
            body(key),
        },
        secret_not_saved {
            title(),
            body(),
        },
        theme_file_invalid {
            title(),
            body_at(file, line, column, detail),
            body(file, detail),
            unreadable(file, cause),
        },
        theme_file_unknown_key {
            title(),
            body(file, keys),
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
        background_image {
            title(),
            not_found(path),
            unreadable(path),
            unsupported(path),
            malformed(path),
            too_large(path),
        },
        window_background_image {
            title(),
            not_found(path),
            unreadable(path),
            unsupported(path),
            malformed(path),
            too_large(path),
        },
        acrylic {
            title(),
            window_opacity(),
            no_window_image(),
        },
        acrylic_blur_adjusted {
            title(),
            body(key, min, max, used),
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

    /// Os cinco idiomas que o projeto entrega.
    pub(crate) fn all_catalogs() -> Vec<Catalog> {
        ["en_US", "pt_BR", "es_ES", "fr_FR", "de_DE"]
            .into_iter()
            .map(catalog)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use porecatu_term::{BackendSpec, PtySize, SpawnConfig, TermParams, Terminal};

    use super::*;

    fn config_error_of(kind: ConfigErrorKind, at: Option<(usize, usize)>) -> ConfigError {
        match at {
            Some((line, column)) => ConfigError::at(line, column, kind),
            None => ConfigError::new(kind),
        }
    }

    // ---- SSH: notas no grid (ADR-0075 §5) e avisos de perfil (ADR-0072 §6)

    fn presented() -> HostKeyPresented {
        HostKeyPresented {
            algorithm: "ssh-ed25519".to_owned(),
            fingerprint: "SHA256:NEWNEWNEW".to_owned(),
        }
    }

    fn entry(line: usize) -> HostKeyEntry {
        HostKeyEntry {
            file: PathBuf::from("/home/u/.ssh/known_hosts"),
            line,
            fingerprint: "SHA256:OLDOLDOLD".to_owned(),
        }
    }

    /// Cada variante de `FailCause`, `LostCause` e `EndReason` que acaba uma
    /// sessão SSH (ADR-0071 §3). Se uma variante nascer, `ssh_end_note` (um
    /// `match` sem curinga) deixa de compilar -- e esta lista a acompanha.
    fn every_end() -> Vec<EndReason> {
        let failed = |cause| EndReason::Failed { cause };
        vec![
            EndReason::RemoteEnded {
                code: Some(2),
                signal: None,
            },
            EndReason::RemoteEnded {
                code: None,
                signal: Some("TERM".to_owned()),
            },
            EndReason::RemoteEnded {
                code: None,
                signal: None,
            },
            EndReason::Lost {
                cause: LostCause::KeepaliveTimeout,
            },
            EndReason::Lost {
                cause: LostCause::Transport,
            },
            EndReason::Lost {
                cause: LostCause::ServerDisconnect,
            },
            failed(FailCause::Refused),
            failed(FailCause::Timeout),
            failed(FailCause::AuthRejected),
            failed(FailCause::AuthCancelled),
            failed(FailCause::AuthUnsupported),
            failed(FailCause::KeyUnreadable),
            failed(FailCause::HostKeyRejectedByUser),
            failed(FailCause::HostKeyChanged {
                presented: presented(),
                expected: vec![entry(4), entry(9)],
            }),
            failed(FailCause::HostKeyRevoked {
                presented: presented(),
                entry: entry(7),
            }),
            failed(FailCause::KnownHostsUnreadable {
                file: PathBuf::from("/home/u/.ssh/known_hosts"),
                cause: KnownHostsCause::PermissionDenied,
            }),
            failed(FailCause::KnownHostsUnreadable {
                file: PathBuf::from("/home/u/.ssh/known_hosts"),
                cause: KnownHostsCause::NotText,
            }),
            failed(FailCause::KnownHostsUnreadable {
                file: PathBuf::from("/home/u/.ssh/known_hosts"),
                cause: KnownHostsCause::Other,
            }),
            failed(FailCause::ChannelRefused),
            failed(FailCause::Runtime),
        ]
    }

    #[test]
    fn every_way_a_session_can_end_has_a_note_in_every_language() {
        for locale in ["en_US", "pt_BR", "es_ES", "fr_FR", "de_DE"] {
            let catalog = test_support::catalog(locale);
            for reason in every_end() {
                let note = ssh_end_note(&catalog, "prod-web-01", &reason)
                    .unwrap_or_else(|| panic!("{locale}: no note for {reason:?}"));
                assert!(!note.trim().is_empty(), "{locale}: {reason:?}");
                // No placeholder was left unfilled.
                assert!(
                    !note.contains('{') && !note.contains('}'),
                    "{locale}: {note}"
                );
            }
        }
    }

    #[test]
    fn a_local_exit_and_a_clean_remote_exit_have_no_ssh_note() {
        let pt = test_support::pt_br();
        let exited = EndReason::Exited {
            success: false,
            code: 3,
        };
        let clean = EndReason::RemoteEnded {
            code: Some(0),
            signal: None,
        };
        assert_eq!(ssh_end_note(&pt, "h", &exited), None);
        assert_eq!(ssh_end_note(&pt, "h", &clean), None);
    }

    #[test]
    fn a_remote_exit_code_reuses_the_process_exited_phrase() {
        let pt = test_support::pt_br();
        let with_code = EndReason::RemoteEnded {
            code: Some(2),
            signal: None,
        };
        assert_eq!(
            ssh_end_note(&pt, "h", &with_code).as_deref(),
            Some("processo encerrado (código 2)")
        );
    }

    #[test]
    fn the_changed_key_note_says_both_fingerprints_and_where_but_asks_for_nothing() {
        let pt = test_support::pt_br();
        let reason = EndReason::Failed {
            cause: FailCause::HostKeyChanged {
                presented: presented(),
                expected: vec![entry(4), entry(9)],
            },
        };

        let note = ssh_end_note(&pt, "10.4.1.20", &reason).unwrap();

        assert!(note.contains("10.4.1.20"));
        assert!(note.contains("ssh-ed25519 SHA256:NEWNEWNEW"), "{note}");
        // Every divergent entry: fingerprint, file and line.
        assert!(
            note.contains("SHA256:OLDOLDOLD (/home/u/.ssh/known_hosts:4)"),
            "{note}"
        );
        assert!(
            note.contains("SHA256:OLDOLDOLD (/home/u/.ssh/known_hosts:9)"),
            "{note}"
        );
        assert!(note.contains("conexão bloqueada"));
        // ADR-0075 §5: informative. Not a word about replacing, accepting,
        // trusting, removing or fixing -- OpenSSH says how to resolve it.
        let lower = note.to_lowercase();
        for asking in [
            "substitua",
            "aceite",
            "confie",
            "remova",
            "apague",
            "ssh-keygen",
            "-r ",
        ] {
            assert!(!lower.contains(asking), "{asking:?} in {note}");
        }
    }

    #[test]
    fn no_ssh_note_asks_the_user_to_change_trust_in_any_language() {
        // The same rule for every language: a note never tells anyone to trust,
        // accept, replace or delete a key (ADR-0075 §5). The notes are not
        // authenticated, a hostile server can imitate one.
        // Imperative forms and the OpenSSH command: a statement such as "host
        // key not trusted" is information, not a request.
        let forbidden: [(&str, &[&str]); 5] = [
            (
                "en_US",
                &[
                    "ssh-keygen",
                    "replace ",
                    "delete ",
                    "remove ",
                    "accept the",
                    "trust it",
                    "to trust",
                    "run ",
                ],
            ),
            (
                "pt_BR",
                &[
                    "ssh-keygen",
                    "substitua",
                    "apague",
                    "remova",
                    "aceite",
                    "confie",
                ],
            ),
            (
                "es_ES",
                &[
                    "ssh-keygen",
                    "sustituya",
                    "borre",
                    "elimine",
                    "acepte",
                    "confíe",
                ],
            ),
            (
                "fr_FR",
                &[
                    "ssh-keygen",
                    "remplacez",
                    "supprimez",
                    "acceptez",
                    "faites confiance",
                ],
            ),
            (
                "de_DE",
                &[
                    "ssh-keygen",
                    "ersetzen sie",
                    "löschen sie",
                    "akzeptieren sie",
                    "vertrauen sie",
                ],
            ),
        ];
        for (locale, words) in forbidden {
            let catalog = test_support::catalog(locale);
            for reason in every_end() {
                let note = ssh_end_note(&catalog, "h", &reason).unwrap().to_lowercase();
                for word in words {
                    assert!(!note.contains(word), "{locale}: {word:?} in {note}");
                }
            }
        }
    }

    #[test]
    fn the_unreadable_known_hosts_note_names_the_file_and_the_reason() {
        let pt = test_support::pt_br();
        let note = |cause| {
            ssh_end_note(
                &pt,
                "h",
                &EndReason::Failed {
                    cause: FailCause::KnownHostsUnreadable {
                        file: PathBuf::from("/x/known_hosts"),
                        cause,
                    },
                },
            )
            .unwrap()
        };
        assert!(note(KnownHostsCause::PermissionDenied).contains("permissão negada"));
        assert!(note(KnownHostsCause::NotText).contains("UTF-8"));
        assert!(note(KnownHostsCause::Other).contains("erro de leitura"));
        assert!(note(KnownHostsCause::NotText).contains("/x/known_hosts"));
        assert!(note(KnownHostsCause::NotText).contains("não foi tentada"));
    }

    #[test]
    fn the_dormant_note_names_the_profile_and_the_key() {
        assert_eq!(
            ssh_dormant_note(&test_support::pt_br(), "prod-web-01"),
            "Enter para conectar a prod-web-01"
        );
        assert_eq!(
            ssh_dormant_note(&test_support::en_us(), "prod-web-01"),
            "Enter to connect to prod-web-01"
        );
    }

    #[test]
    fn the_connecting_note_names_the_host() {
        assert_eq!(
            ssh_connecting_note(&test_support::pt_br(), "10.4.1.20"),
            "conectando a 10.4.1.20…"
        );
        assert_eq!(
            ssh_connecting_note(&test_support::en_us(), "10.4.1.20"),
            "connecting to 10.4.1.20…"
        );
    }

    #[test]
    fn every_profile_problem_has_a_sentence_with_its_position() {
        use ProfileProblemKind as Kind;
        let kinds = [
            Kind::IdInvalid,
            Kind::IdReserved,
            Kind::IdDuplicate { id: "dup".into() },
            Kind::NameEmpty,
            Kind::BadgeEmpty,
            Kind::BadgeTooLong,
            Kind::TypeUnknown,
            Kind::ProgramEmpty,
            Kind::DefaultUnknown { id: "nope".into() },
            Kind::SshFieldNotAllowed { field: "program" },
            Kind::HostInvalid,
            Kind::UserInvalid,
            Kind::PortInvalid,
            Kind::AuthInvalid,
            Kind::KeyWithoutIdentityFile,
        ];
        for locale in ["en_US", "pt_BR", "es_ES", "fr_FR", "de_DE"] {
            let catalog = test_support::catalog(locale);
            for kind in &kinds {
                let problem = ProfileProblem {
                    line: 12,
                    column: 7,
                    kind: kind.clone(),
                };
                let (severity, title, body) = profile_problem(&catalog, &problem);
                assert_eq!(severity, Severity::Warning);
                assert!(!title.is_empty(), "{locale}");
                assert!(
                    body.contains("12") && body.contains('7'),
                    "{locale}: {body}"
                );
                assert!(!body.contains('{'), "{locale}: {body}");
            }
        }
    }

    #[test]
    fn the_profile_problem_sentence_names_what_is_wrong() {
        let pt = test_support::pt_br();
        let (_, title, body) = profile_problem(
            &pt,
            &ProfileProblem {
                line: 3,
                column: 8,
                kind: ProfileProblemKind::SshFieldNotAllowed { field: "cwd" },
            },
        );
        assert_eq!(title, "Problema em perfil");
        assert_eq!(
            body,
            "linha 3, coluna 8: perfil ignorado: \"cwd\" não vale para o tipo \"ssh\""
        );
    }

    #[test]
    fn the_secret_dialog_is_translated_and_its_title_names_the_destination() {
        let titles: Vec<String> = test_support::all_catalogs()
            .iter()
            .map(|catalog| msg::dialog::secret::title_password(catalog, "deploy@10.4.1.20"))
            .collect();
        assert!(
            titles
                .iter()
                .all(|title| title.contains("deploy@10.4.1.20"))
        );
        // Five languages, five different words for "password".
        let mut unique = titles.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 5, "{titles:?}");
        for catalog in test_support::all_catalogs() {
            for text in [
                msg::dialog::secret::title_passphrase(&catalog),
                msg::dialog::secret::remember(&catalog),
                msg::dialog::secret::remember_unavailable(&catalog),
                msg::dialog::secret::confirm(&catalog),
                msg::dialog::secret::refused_password(&catalog),
                msg::dialog::secret::refused_passphrase(&catalog),
            ] {
                assert!(!text.is_empty() && !text.contains('{'), "{text}");
            }
        }
        assert_eq!(
            msg::dialog::secret::remember(&test_support::pt_br()),
            "Lembrar no cofre do sistema"
        );
    }

    #[test]
    fn the_unsaved_secret_warning_is_a_warning_in_every_language_and_leaks_nothing() {
        for catalog in test_support::all_catalogs() {
            let (severity, title, body) = secret_not_saved(&catalog);
            assert_eq!(severity, Severity::Warning);
            assert!(!title.is_empty() && !body.is_empty());
        }
        let (_, title, body) = secret_not_saved(&test_support::pt_br());
        assert_eq!(title, "Segredo não salvo");
        assert!(!body.to_lowercase().contains("guia"));
    }

    #[test]
    fn the_secret_key_warning_says_the_value_was_ignored_and_where_secrets_go() {
        let pt = test_support::pt_br();
        let (severity, title, body) = secret_profile_key(&pt, "profiles.list.0.password");
        assert_eq!(severity, Severity::Warning);
        assert_eq!(title, "Chave ignorada");
        assert!(body.contains("profiles.list.0.password"));
        assert!(body.contains("o valor não foi lido"));
        assert!(body.contains("cofre do sistema"));
        // It is not the generic unknown-key sentence: that one is the key alone.
        assert_ne!(body, "profiles.list.0.password");
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
    fn config_unreadable_has_no_position() {
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
    }

    #[test]
    fn theme_file_problems_cite_the_file_name() {
        let pt = test_support::pt_br();
        let (_, title, body) = theme_file_problem(
            &pt,
            &ThemeFileProblem::UnknownKeys {
                path: PathBuf::from("/c/thm_meu.toml"),
                keys: vec!["terminal.foo".to_owned(), "bar".to_owned()],
            },
        );
        assert_eq!(title, "Chave desconhecida no tema");
        assert_eq!(body, "thm_meu.toml: terminal.foo, bar");
        let (_, _, body) = theme_file_problem(
            &pt,
            &ThemeFileProblem::Invalid {
                path: PathBuf::from("/c/thm_meu.toml"),
                error: config_error_of(
                    ConfigErrorKind::Toml {
                        detail: "d".to_owned(),
                    },
                    Some((3, 4)),
                ),
            },
        );
        assert_eq!(body, "thm_meu.toml, linha 3, coluna 4: d");
        let (_, _, body) = colors_ignored(&pt, 7);
        assert_eq!(
            body,
            "7 chave(s) de cor ignorada(s): as cores agora ficam nos arquivos de tema thm_*.toml."
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
            BackendSpec::Local(SpawnConfig {
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
            }),
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
