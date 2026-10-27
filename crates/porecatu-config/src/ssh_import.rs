// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading `~/.ssh/config` to offer SSH profiles (ADR-0075 §7, RF-20.5 to
//! RF-20.7). The result is data: candidates with a typed state and a typed
//! error, never prose. The import is a copy, not a link (RF-20.6).
//!
//! `Match` is the one thing `ssh2-config` cannot read, and it refuses it in
//! the default mode with `unknown field: Match`. Tolerating unknown fields
//! would be worse: the directives under a `Match` would be merged into the
//! `Host` block above it (a probe showed `HostName` of the `Match` replacing
//! the one of the block). So the reader runs without `ALLOW_UNKNOWN_FIELDS`
//! and a file with `Match` is a refused file (ADR-0075 §7).

use std::fs;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use ssh2_config::{HostParams, ParseRule, SshConfig, SshParserError};

use crate::profiles::{SshProfile, is_valid_ssh_host, is_valid_ssh_user};

const DEFAULT_PORT: u16 = 22;

/// What the embedded client cannot do, so the block is shown disabled
/// (RF-20.6). One reason per block: the first found, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshImportUnsupported {
    ProxyJump,
    ProxyCommand,
    LocalForward,
    RemoteForward,
    DynamicForward,
    CertificateFile,
}

/// Which field of the candidate the ADR-0072 §6 validation refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshImportInvalid {
    Host,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshImportStatus {
    Importable,
    /// Same host, port and user as an existing SSH profile (RF-20.7).
    AlreadyImported,
    Unsupported(SshImportUnsupported),
    Invalid(SshImportInvalid),
}

/// One concrete `Host` name of the file, with the directives resolved the way
/// `ssh` would (earlier blocks win; `Host *` defaults apply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshImportCandidate {
    /// The name as written in the `Host` line; it becomes the profile name.
    pub alias: String,
    /// `HostName`, or the alias when the block has none.
    pub host: String,
    /// `User`, or the local user when the block has none.
    pub user: String,
    pub port: u16,
    /// First `IdentityFile`, with `~` already expanded; empty = none.
    pub identity_file: String,
    pub status: SshImportStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfigImport {
    /// `false` when the file does not exist: an empty list that says why.
    pub file_found: bool,
    pub candidates: Vec<SshImportCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshConfigImportErrorKind {
    /// The file exists but could not be read (permissions, not UTF-8).
    Unreadable,
    /// A `Match` block: the reader does not support it (ADR-0075 §7).
    Match,
    /// Any other directive the reader does not know.
    UnknownDirective { name: String },
    /// A known directive with a value the reader refuses (port, boolean...).
    InvalidValue,
    /// An `Include` that loops, has a bad glob or needs a missing home.
    Include,
}

/// A refused file: nothing is imported (ADR-0075 §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfigImportError {
    /// 1-based, when the reader names the directive and it is in the main file.
    pub line: Option<usize>,
    pub kind: SshConfigImportErrorKind,
}

/// `<home>/.ssh/config`.
pub fn default_ssh_config_path(home: &Path) -> PathBuf {
    home.join(".ssh").join("config")
}

/// The login name of the local user: what a block without `User` gets
/// (ADR-0072 §6). Empty when the environment has none.
pub fn local_user_name() -> String {
    ["USERNAME", "USER"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default()
}

/// Reads `path` and lists its concrete `Host` names. `home` expands `~` and
/// resolves relative `Include`s; `existing` marks the already imported ones.
pub fn read_ssh_config(
    path: &Path,
    home: &Path,
    local_user: &str,
    existing: &[SshProfile],
) -> Result<SshConfigImport, SshConfigImportError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok(SshConfigImport {
                file_found: false,
                candidates: Vec::new(),
            });
        }
        Err(_) => return Err(error(None, SshConfigImportErrorKind::Unreadable)),
    };
    let mut reader = BufReader::new(text.as_bytes());
    let config = SshConfig::default()
        .home_dir(home)
        .parse(&mut reader, ParseRule::ALLOW_UNSUPPORTED_FIELDS)
        .map_err(|err| refused(err, &text))?;

    let mut aliases: Vec<&str> = Vec::new();
    for host in config.get_hosts() {
        for clause in &host.pattern {
            let alias = clause.pattern.as_str();
            if !clause.negated && is_concrete(alias) && !aliases.contains(&alias) {
                aliases.push(alias);
            }
        }
    }
    let candidates = aliases
        .into_iter()
        .map(|alias| candidate(alias, &config.query(alias), home, local_user, existing))
        .collect();
    Ok(SshConfigImport {
        file_found: true,
        candidates,
    })
}

fn is_concrete(alias: &str) -> bool {
    !alias.is_empty() && !alias.contains(['*', '?', '!'])
}

fn candidate(
    alias: &str,
    params: &HostParams,
    home: &Path,
    local_user: &str,
    existing: &[SshProfile],
) -> SshImportCandidate {
    let host = params.host_name.clone().unwrap_or_else(|| alias.to_owned());
    let user = params.user.clone().unwrap_or_else(|| local_user.to_owned());
    let port = params.port.unwrap_or(DEFAULT_PORT);
    let identity_file = params
        .identity_file
        .as_ref()
        .and_then(|files| files.first())
        .map(|file| tilde_form(file, home))
        .unwrap_or_default();
    let status = if let Some(reason) = unsupported(params) {
        SshImportStatus::Unsupported(reason)
    } else if !is_valid_ssh_host(&host) {
        SshImportStatus::Invalid(SshImportInvalid::Host)
    } else if !is_valid_ssh_user(&user) {
        SshImportStatus::Invalid(SshImportInvalid::User)
    } else if existing
        .iter()
        .any(|p| p.host.eq_ignore_ascii_case(&host) && p.port == port && p.user == user)
    {
        SshImportStatus::AlreadyImported
    } else {
        SshImportStatus::Importable
    };
    SshImportCandidate {
        alias: alias.to_owned(),
        host,
        user,
        port,
        identity_file,
        status,
    }
}

/// The reader expands `~`; the profile keeps it as `~/…`, so it does not carry
/// this machine's home folder (and Windows separators) into the file.
fn tilde_form(file: &Path, home: &Path) -> String {
    match file.strip_prefix(home) {
        Ok(rest) => {
            let rest: Vec<_> = rest.iter().map(|part| part.to_string_lossy()).collect();
            format!("~/{}", rest.join("/"))
        }
        Err(_) => file.to_string_lossy().into_owned(),
    }
}

fn unsupported(params: &HostParams) -> Option<SshImportUnsupported> {
    let ignored = |key: &str| params.unsupported_fields.contains_key(key);
    if params.proxy_jump.is_some() {
        Some(SshImportUnsupported::ProxyJump)
    } else if ignored("proxycommand") {
        Some(SshImportUnsupported::ProxyCommand)
    } else if ignored("localforward") {
        Some(SshImportUnsupported::LocalForward)
    } else if !params.remote_forward.is_empty() {
        Some(SshImportUnsupported::RemoteForward)
    } else if ignored("dynamicforward") {
        Some(SshImportUnsupported::DynamicForward)
    } else if params.certificate_file.is_some() {
        Some(SshImportUnsupported::CertificateFile)
    } else {
        None
    }
}

fn error(line: Option<usize>, kind: SshConfigImportErrorKind) -> SshConfigImportError {
    SshConfigImportError { line, kind }
}

/// Maps the crate's error to ours: its English message never leaves here.
fn refused(err: SshParserError, text: &str) -> SshConfigImportError {
    use SshConfigImportErrorKind as Kind;
    match err {
        SshParserError::UnknownField(name, _) | SshParserError::UnsupportedField(name, _) => {
            let line = directive_line(text, &name);
            if name.eq_ignore_ascii_case("match") {
                error(line, Kind::Match)
            } else {
                error(line, Kind::UnknownDirective { name })
            }
        }
        SshParserError::Io(_) => error(None, Kind::Unreadable),
        SshParserError::IncludeCycle(_)
        | SshParserError::Glob(_)
        | SshParserError::PatternError(_)
        | SshParserError::HomeNotFound(_) => error(None, Kind::Include),
        _ => error(None, Kind::InvalidValue),
    }
}

/// The 1-based line of the first line of `text` that starts with `name`.
fn directive_line(text: &str, name: &str) -> Option<usize> {
    text.lines()
        .position(|line| {
            let line = line.trim_start();
            line.get(..name.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(name))
                && line[name.len()..].starts_with(|c: char| c.is_whitespace() || c == '=')
        })
        .map(|index| index + 1)
}
