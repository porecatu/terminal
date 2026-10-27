// SPDX-License-Identifier: GPL-3.0-or-later

//! `[profiles]` and `[[profiles.list]]` (ADR-0070 §1-§3, §5, §9): tab
//! profiles, their validation and the pure function that resolves one into
//! what to spawn. Reload class C, like `[shell]`: it applies to new tabs.
//!
//! A bad item is dropped and reported as a [`ProfileProblem`] with line and
//! column; the remaining items stay valid (RF-7.5). The problems travel
//! beside the config (`parse_checked`), not inside `ConfigError`, because
//! `ConfigError` aborts the whole file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer};
use toml::Spanned;

use crate::Config;
use crate::project_file::expand_tilde;

/// Reserved prefix of detected-profile identifiers (ADR-0070 §5).
const AUTO_PREFIX: &str = "auto:";
/// Longest badge, in Unicode scalar values (ADR-0070 §1).
const MAX_BADGE_CHARS: usize = 4;
/// The only executables `auto:shell:` accepts on Windows (ADR-0070 §5).
const WINDOWS_AUTO_SHELLS: [&str; 3] = ["pwsh.exe", "powershell.exe", "cmd.exe"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProfileKind {
    #[default]
    Local,
    /// `type = "ssh"` (ADR-0072 §6): the profile carries an [`SshProfile`].
    Ssh,
}

/// An SSH authentication method (ADR-0072 §6). The order of
/// [`SshProfile::auth`] is the order tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshAuth {
    Agent,
    Key,
    Password,
}

/// The destination of a `type = "ssh"` profile, already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshProfile {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: Vec<SshAuth>,
    /// Empty = none. A leading `~` is expanded on resolve.
    pub identity_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDef {
    pub id: String,
    pub name: String,
    pub badge: String,
    pub kind: ProfileKind,
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Empty = inherit, like `tab.new`. A leading `~` is expanded on resolve.
    pub cwd: String,
    /// `Some` exactly when `kind` is [`ProfileKind::Ssh`]; `program`, `args`,
    /// `env` and `cwd` are then empty.
    pub ssh: Option<SshProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilesConfig {
    /// Empty = the `[shell]` itself.
    pub default: String,
    pub detect: bool,
    pub list: Vec<ProfileDef>,
    /// Problems found while deserializing, with byte offsets. `parse_checked`
    /// takes them out and converts them to line and column.
    pub(crate) problems: Vec<RawProblem>,
}

impl Default for ProfilesConfig {
    fn default() -> Self {
        Self {
            default: String::new(),
            detect: true,
            list: Vec::new(),
            problems: Vec::new(),
        }
    }
}

/// Why a profile item, or `default`, was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileProblemKind {
    /// `id` missing, or outside `^[a-z0-9][a-z0-9_-]{0,31}$`.
    IdInvalid,
    /// `id` starts with the reserved `auto:` prefix.
    IdReserved,
    /// `id` already used by an earlier item.
    IdDuplicate {
        id: String,
    },
    NameEmpty,
    BadgeEmpty,
    BadgeTooLong,
    /// `type` is neither `"local"` nor `"ssh"`.
    TypeUnknown,
    ProgramEmpty,
    /// `program`, `args`, `env` or `cwd` in a `type = "ssh"` profile: known
    /// keys with no meaning for the type, so an error and not an unknown-key
    /// warning (ADR-0072 §6).
    SshFieldNotAllowed {
        field: &'static str,
    },
    /// `host` missing, empty, or with a character that would corrupt a
    /// `known_hosts` line (ADR-0072 §6).
    HostInvalid,
    /// `user` missing, empty, or with whitespace, a control character, `@`
    /// or `:`.
    UserInvalid,
    /// `port` outside 1..=65535.
    PortInvalid,
    /// `auth` empty, with a value other than `agent`/`key`/`password`, or
    /// with a repeated one.
    AuthInvalid,
    /// `auth = ["key"]` without `identity_file`: nothing could ever succeed.
    KeyWithoutIdentityFile,
    /// `[profiles] default` names no valid item; it falls back to `""`.
    DefaultUnknown {
        id: String,
    },
}

/// A refused profile item, localized like a `ConfigError`. The UI composes
/// the sentence from `kind` (ADR-0056 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileProblem {
    pub line: usize,
    pub column: usize,
    pub kind: ProfileProblemKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawProblem {
    pub(crate) offset: usize,
    pub(crate) kind: ProfileProblemKind,
}

#[derive(Deserialize)]
#[serde(default)]
struct RawProfiles {
    default: Option<Spanned<String>>,
    detect: bool,
    list: Vec<Spanned<RawProfile>>,
}

impl Default for RawProfiles {
    fn default() -> Self {
        Self {
            default: None,
            detect: true,
            list: Vec::new(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawProfile {
    id: Option<Spanned<String>>,
    name: Option<Spanned<String>>,
    badge: Option<Spanned<String>>,
    #[serde(rename = "type")]
    kind: Option<Spanned<String>>,
    program: Option<Spanned<String>>,
    args: Option<Spanned<Vec<String>>>,
    env: Option<Spanned<BTreeMap<String, String>>>,
    cwd: Option<Spanned<String>>,
    host: Option<Spanned<String>>,
    port: Option<Spanned<i64>>,
    user: Option<Spanned<String>>,
    auth: Option<Spanned<Vec<String>>>,
    identity_file: Option<Spanned<String>>,
}

impl<'de> Deserialize<'de> for ProfilesConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(RawProfiles::deserialize(deserializer)?.validate())
    }
}

impl RawProfiles {
    fn validate(self) -> ProfilesConfig {
        let mut problems = Vec::new();
        let mut list: Vec<ProfileDef> = Vec::new();
        for item in self.list {
            let item_start = item.span().start;
            match validate_item(item.into_inner(), item_start, &list) {
                Ok(def) => list.push(def),
                Err(problem) => problems.push(problem),
            }
        }

        let mut default = String::new();
        if let Some(raw) = self.default {
            let offset = raw.span().start;
            let value = raw.into_inner();
            if value.is_empty() || list.iter().any(|def| def.id == value) {
                default = value;
            } else {
                problems.push(RawProblem {
                    offset,
                    kind: ProfileProblemKind::DefaultUnknown { id: value },
                });
            }
        }

        ProfilesConfig {
            default,
            detect: self.detect,
            list,
            problems,
        }
    }
}

/// Where a field's problem points: the value when the key is present, the
/// item otherwise.
fn offset_of(field: &Option<Spanned<String>>, item_start: usize) -> usize {
    field.as_ref().map_or(item_start, |f| f.span().start)
}

fn validate_item(
    raw: RawProfile,
    item_start: usize,
    accepted: &[ProfileDef],
) -> Result<ProfileDef, RawProblem> {
    let fail = |offset: usize, kind: ProfileProblemKind| Err(RawProblem { offset, kind });

    let id_offset = offset_of(&raw.id, item_start);
    let id = raw.id.as_ref().map_or("", |f| f.get_ref().as_str());
    if id.starts_with(AUTO_PREFIX) {
        return fail(id_offset, ProfileProblemKind::IdReserved);
    }
    if !is_valid_id(id) {
        return fail(id_offset, ProfileProblemKind::IdInvalid);
    }
    if accepted.iter().any(|def| def.id == id) {
        return fail(
            id_offset,
            ProfileProblemKind::IdDuplicate { id: id.to_owned() },
        );
    }

    let name = raw.name.as_ref().map_or("", |f| f.get_ref().as_str());
    if name.trim().is_empty() {
        return fail(
            offset_of(&raw.name, item_start),
            ProfileProblemKind::NameEmpty,
        );
    }

    let kind = match raw.kind.as_ref().map(|f| f.get_ref().as_str()) {
        None | Some("local") => ProfileKind::Local,
        Some("ssh") => ProfileKind::Ssh,
        Some(_) => {
            return fail(
                offset_of(&raw.kind, item_start),
                ProfileProblemKind::TypeUnknown,
            );
        }
    };

    // A local profile must name its badge; an SSH one defaults to `SSH`.
    let badge = match (&raw.badge, kind) {
        (None, ProfileKind::Ssh) => SSH_DEFAULT_BADGE,
        (_, _) => raw.badge.as_ref().map_or("", |f| f.get_ref().as_str()),
    };
    if badge.trim().is_empty() {
        return fail(
            offset_of(&raw.badge, item_start),
            ProfileProblemKind::BadgeEmpty,
        );
    }
    if badge.chars().count() > MAX_BADGE_CHARS {
        return fail(
            offset_of(&raw.badge, item_start),
            ProfileProblemKind::BadgeTooLong,
        );
    }

    if kind == ProfileKind::Ssh {
        let ssh = validate_ssh(&raw, item_start)?;
        return Ok(ProfileDef {
            id: id.to_owned(),
            name: name.to_owned(),
            badge: badge.to_owned(),
            kind,
            program: String::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: String::new(),
            ssh: Some(ssh),
        });
    }

    let program = raw.program.as_ref().map_or("", |f| f.get_ref().as_str());
    if program.trim().is_empty() {
        return fail(
            offset_of(&raw.program, item_start),
            ProfileProblemKind::ProgramEmpty,
        );
    }

    Ok(ProfileDef {
        id: id.to_owned(),
        name: name.to_owned(),
        badge: badge.to_owned(),
        kind,
        program: program.to_owned(),
        args: raw.args.map(Spanned::into_inner).unwrap_or_default(),
        env: raw.env.map(Spanned::into_inner).unwrap_or_default(),
        cwd: raw.cwd.map(Spanned::into_inner).unwrap_or_default(),
        ssh: None,
    })
}

/// The badge of an SSH profile that does not name one (ADR-0072 §6).
const SSH_DEFAULT_BADGE: &str = "SSH";

/// Default SSH port.
const SSH_DEFAULT_PORT: u16 = 22;

/// Validation of a `type = "ssh"` item (ADR-0072 §6). `host` and `user` become
/// a `known_hosts` line and a vault account, so the characters that would
/// change the meaning of either are refused.
fn validate_ssh(raw: &RawProfile, item_start: usize) -> Result<SshProfile, RawProblem> {
    let fail = |offset: usize, kind: ProfileProblemKind| Err(RawProblem { offset, kind });

    // `program`, `args`, `env` and `cwd` mean nothing here.
    let forbidden: [(&'static str, Option<usize>); 4] = [
        ("program", raw.program.as_ref().map(|f| f.span().start)),
        ("args", raw.args.as_ref().map(|f| f.span().start)),
        ("env", raw.env.as_ref().map(|f| f.span().start)),
        ("cwd", raw.cwd.as_ref().map(|f| f.span().start)),
    ];
    for (field, offset) in forbidden {
        if let Some(offset) = offset {
            return fail(offset, ProfileProblemKind::SshFieldNotAllowed { field });
        }
    }

    let host = raw.host.as_ref().map_or("", |f| f.get_ref().as_str());
    if !is_valid_ssh_host(host) {
        return fail(
            offset_of(&raw.host, item_start),
            ProfileProblemKind::HostInvalid,
        );
    }
    let user = raw.user.as_ref().map_or("", |f| f.get_ref().as_str());
    if !is_valid_ssh_user(user) {
        return fail(
            offset_of(&raw.user, item_start),
            ProfileProblemKind::UserInvalid,
        );
    }

    let port = match &raw.port {
        None => SSH_DEFAULT_PORT,
        Some(port) => match u16::try_from(*port.get_ref()) {
            Ok(value) if value != 0 => value,
            _ => return fail(port.span().start, ProfileProblemKind::PortInvalid),
        },
    };

    let identity_file = raw
        .identity_file
        .as_ref()
        .map_or("", |f| f.get_ref().as_str())
        .to_owned();

    let auth = match &raw.auth {
        // The default order (RF-20.9) tries the key only when there is a file.
        None => {
            let mut auth = vec![SshAuth::Agent];
            if !identity_file.is_empty() {
                auth.push(SshAuth::Key);
            }
            auth.push(SshAuth::Password);
            auth
        }
        Some(list) => {
            let mut auth = Vec::new();
            for value in list.get_ref() {
                let method = match value.as_str() {
                    "agent" => SshAuth::Agent,
                    "key" => SshAuth::Key,
                    "password" => SshAuth::Password,
                    _ => return fail(list.span().start, ProfileProblemKind::AuthInvalid),
                };
                if auth.contains(&method) {
                    return fail(list.span().start, ProfileProblemKind::AuthInvalid);
                }
                auth.push(method);
            }
            if auth.is_empty() {
                return fail(list.span().start, ProfileProblemKind::AuthInvalid);
            }
            if auth == [SshAuth::Key] && identity_file.is_empty() {
                return fail(
                    list.span().start,
                    ProfileProblemKind::KeyWithoutIdentityFile,
                );
            }
            auth
        }
    };

    Ok(SshProfile {
        host: host.to_owned(),
        port,
        user: user.to_owned(),
        auth,
        identity_file,
    })
}

/// No whitespace, control character, or any of `,|[]@#*?!` (ADR-0072 §6). An
/// IPv6 literal is written without brackets (`::1`).
pub(crate) fn is_valid_ssh_host(host: &str) -> bool {
    !host.is_empty()
        && !host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || ",|[]@#*?!".contains(c))
}

/// No whitespace, control character, `@` or `:` (ADR-0072 §6).
pub(crate) fn is_valid_ssh_user(user: &str) -> bool {
    !user.is_empty()
        && !user
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '@' || c == ':')
}

/// Key names that smell like a secret inside a profile (ADR-0072 §6): they are
/// ignored like any unknown key, but the warning says secrets belong in the
/// system vault. `path` is the `serde_ignored` path, e.g.
/// `profiles.list.1.password`.
pub fn is_secret_profile_key(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("profiles.list.") else {
        return false;
    };
    let name = rest.rsplit('.').next().unwrap_or(rest).to_lowercase();
    ["password", "passphrase", "secret"]
        .iter()
        .any(|word| name.contains(word))
}

/// `^[a-z0-9][a-z0-9_-]{0,31}$`.
fn is_valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    let tail_ok = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-');
    (1..=32).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes[1..].iter().all(tail_ok)
}

/// `^[A-Za-z0-9._-]{1,64}$`: what a WSL distribution name may be.
fn is_valid_distro(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileRef<'a> {
    /// The profile `[profiles] default` names, or the `[shell]`.
    Default,
    /// A declared id, an `auto:` id, or `""` for the `[shell]`.
    Id(&'a str),
}

/// What to spawn, ready for `SpawnConfig` (ADR-0070 §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProfile {
    /// `""` for the `[shell]`; never written to the session.
    pub id: String,
    pub badge: Option<String>,
    /// `None` = `resolve_default_shell`.
    pub program: Option<String>,
    pub args: Vec<String>,
    /// `[shell.env]` with the profile's `env` merged over it.
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    /// `Some` for a `type = "ssh"` profile: then `program` is `None` **and
    /// does not mean the default shell** -- the caller must open a
    /// connection instead.
    pub ssh: Option<ResolvedSsh>,
}

/// A `type = "ssh"` profile ready for `SshSpawn` (ADR-0072 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSsh {
    /// The profile's `name`: what labels the tab when nothing else does
    /// (RF-20.20).
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: Vec<SshAuth>,
    /// `~` already expanded.
    pub identity_file: Option<PathBuf>,
}

/// The identifier names no profile, or an `auto:` one that failed
/// validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingProfile {
    pub id: String,
}

/// Which platform's `auto:` rules apply. A parameter so both rule sets are
/// testable on any host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoPlatform {
    Windows,
    Unix,
}

impl AutoPlatform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

pub fn resolve_profile(
    config: &Config,
    profile: ProfileRef<'_>,
) -> Result<ResolvedProfile, MissingProfile> {
    let id = match profile {
        ProfileRef::Default => config.profiles.default.as_str(),
        ProfileRef::Id(id) => id,
    };
    if id.is_empty() {
        return Ok(shell_profile(config));
    }
    if id.starts_with(AUTO_PREFIX) {
        let platform = AutoPlatform::current();
        let shells = if platform == AutoPlatform::Unix && id.starts_with("auto:shell:") {
            system_shells()
        } else {
            Vec::new()
        };
        let mut resolved = resolve_auto_profile(id, platform, &shells)?;
        resolved.env = merged_env(&config.shell.env, &BTreeMap::new());
        return Ok(resolved);
    }
    let def = config
        .profiles
        .list
        .iter()
        .find(|def| def.id == id)
        .ok_or_else(|| MissingProfile { id: id.to_owned() })?;
    if let Some(ssh) = &def.ssh {
        return Ok(ResolvedProfile {
            id: def.id.clone(),
            badge: Some(def.badge.clone()),
            program: None,
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            ssh: Some(ResolvedSsh {
                name: def.name.clone(),
                host: ssh.host.clone(),
                port: ssh.port,
                user: ssh.user.clone(),
                auth: ssh.auth.clone(),
                identity_file: (!ssh.identity_file.is_empty())
                    .then(|| expand_tilde(&ssh.identity_file)),
            }),
        });
    }
    Ok(ResolvedProfile {
        id: def.id.clone(),
        badge: Some(def.badge.clone()),
        program: Some(def.program.clone()),
        args: def.args.clone(),
        env: merged_env(&config.shell.env, &def.env),
        cwd: (!def.cwd.is_empty()).then(|| expand_tilde(&def.cwd)),
        ssh: None,
    })
}

fn shell_profile(config: &Config) -> ResolvedProfile {
    let shell = &config.shell;
    ResolvedProfile {
        id: String::new(),
        badge: None,
        program: (!shell.program.is_empty()).then(|| shell.program.clone()),
        args: shell.args.clone(),
        env: merged_env(&shell.env, &BTreeMap::new()),
        cwd: None,
        ssh: None,
    }
}

fn merged_env(
    base: &BTreeMap<String, String>,
    over: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let mut merged = base.clone();
    merged.extend(over.iter().map(|(k, v)| (k.clone(), v.clone())));
    merged.into_iter().collect()
}

/// Validates an `auto:` identifier and derives the profile from it, without
/// touching the system (ADR-0070 §5). `unix_shells` is the content of
/// `/etc/shells`, read by the caller at restore time. The result has an empty
/// `env`: the caller fills in `[shell.env]`.
pub fn resolve_auto_profile(
    id: &str,
    platform: AutoPlatform,
    unix_shells: &[String],
) -> Result<ResolvedProfile, MissingProfile> {
    let missing = || MissingProfile { id: id.to_owned() };
    let rest = id.strip_prefix(AUTO_PREFIX).ok_or_else(missing)?;
    let (kind, value) = rest.split_once(':').ok_or_else(missing)?;
    let (program, args, badge) = match (kind, platform) {
        ("shell", AutoPlatform::Windows) if WINDOWS_AUTO_SHELLS.contains(&value) => {
            let badge = if value == "cmd.exe" { "CMD" } else { "PS" };
            (value.to_owned(), Vec::new(), badge.to_owned())
        }
        ("shell", AutoPlatform::Unix) if unix_shells.iter().any(|shell| shell == value) => {
            let name = value.rsplit('/').next().unwrap_or(value);
            let badge: String = name.chars().take(3).flat_map(char::to_uppercase).collect();
            (value.to_owned(), Vec::new(), badge)
        }
        ("wsl", AutoPlatform::Windows) if is_valid_distro(value) => (
            "wsl.exe".to_owned(),
            vec!["-d".to_owned(), value.to_owned()],
            "WSL".to_owned(),
        ),
        _ => return Err(missing()),
    };
    Ok(ResolvedProfile {
        id: id.to_owned(),
        badge: Some(badge),
        program: Some(program),
        args,
        env: Vec::new(),
        cwd: None,
        ssh: None,
    })
}

/// `/etc/shells`, read on the spot: shells only, comments and blanks gone.
/// Empty when the file is missing and on Windows.
fn system_shells() -> Vec<String> {
    let Ok(text) = std::fs::read_to_string("/etc/shells") else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_checked;

    #[test]
    fn default_matches_example_toml() {
        assert_eq!(
            ProfilesConfig::default(),
            ProfilesConfig {
                default: String::new(),
                detect: true,
                list: Vec::new(),
                problems: Vec::new(),
            }
        );
    }

    fn checked(text: &str) -> (Config, Vec<ProfileProblem>) {
        let parsed = parse_checked(text).expect("valid toml");
        assert!(parsed.unknown_keys.is_empty(), "{:?}", parsed.unknown_keys);
        (parsed.config, parsed.profile_problems)
    }

    #[test]
    fn empty_input_has_no_profiles() {
        let (config, problems) = checked("");
        assert_eq!(config.profiles, ProfilesConfig::default());
        assert!(problems.is_empty());
    }

    #[test]
    fn full_profiles_table_parses() {
        let (config, problems) = checked(
            r#"
[profiles]
default = "wsl"
detect = false

[[profiles.list]]
id = "wsl"
name = "Ubuntu 24.04"
badge = "WSL"
program = "wsl.exe"
args = ["-d", "Ubuntu"]
cwd = "~/projetos"
[profiles.list.env]
FOO = "bar"

[[profiles.list]]
id = "plain"
name = "Plain"
badge = "P"
type = "local"
program = "sh"
"#,
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(config.profiles.default, "wsl");
        assert!(!config.profiles.detect);
        assert_eq!(config.profiles.list.len(), 2);
        let wsl = &config.profiles.list[0];
        assert_eq!(wsl.name, "Ubuntu 24.04");
        assert_eq!(wsl.args, ["-d", "Ubuntu"]);
        assert_eq!(wsl.env.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(wsl.kind, ProfileKind::Local);
        assert_eq!(config.profiles.list[1].cwd, "");
    }

    #[test]
    fn unknown_key_inside_a_profile_is_a_warning() {
        let parsed = parse_checked(
            "[[profiles.list]]\nid = \"a\"\nname = \"A\"\nbadge = \"A\"\nprogram = \"x\"\nfoo = 1\n",
        )
        .unwrap();
        assert_eq!(parsed.unknown_keys, vec!["profiles.list.0.foo".to_owned()]);
        assert_eq!(parsed.config.profiles.list.len(), 1);
        assert!(parsed.profile_problems.is_empty());
    }

    /// One invalid item between two valid ones: it alone is dropped, and the
    /// problem points at `line`/`column` (1-based).
    fn assert_item_refused(bad: &str, kind: ProfileProblemKind, line: usize, column: usize) {
        let text = format!(
            "[[profiles.list]]\nid = \"first\"\nname = \"First\"\nbadge = \"F\"\nprogram = \"a\"\n\n{bad}\n[[profiles.list]]\nid = \"last\"\nname = \"Last\"\nbadge = \"L\"\nprogram = \"b\"\n"
        );
        let (config, problems) = checked(&text);
        let ids: Vec<_> = config.profiles.list.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["first", "last"], "{bad}");
        assert_eq!(
            problems,
            vec![ProfileProblem { line, column, kind }],
            "{bad}"
        );
    }

    #[test]
    fn invalid_id_is_refused() {
        assert_item_refused(
            "[[profiles.list]]\nid = \"Bad Id\"\nname = \"X\"\nbadge = \"X\"\nprogram = \"x\"",
            ProfileProblemKind::IdInvalid,
            8,
            6,
        );
    }

    #[test]
    fn id_too_long_is_refused() {
        let long = "a".repeat(33);
        assert_item_refused(
            &format!(
                "[[profiles.list]]\nid = \"{long}\"\nname = \"X\"\nbadge = \"X\"\nprogram = \"x\""
            ),
            ProfileProblemKind::IdInvalid,
            8,
            6,
        );
    }

    #[test]
    fn missing_id_points_at_the_item() {
        assert_item_refused(
            "[[profiles.list]]\nname = \"X\"\nbadge = \"X\"\nprogram = \"x\"",
            ProfileProblemKind::IdInvalid,
            7,
            1,
        );
    }

    #[test]
    fn reserved_auto_prefix_is_refused() {
        assert_item_refused(
            "[[profiles.list]]\nid = \"auto:shell:cmd.exe\"\nname = \"X\"\nbadge = \"X\"\nprogram = \"x\"",
            ProfileProblemKind::IdReserved,
            8,
            6,
        );
    }

    #[test]
    fn duplicate_id_is_refused_and_the_first_wins() {
        assert_item_refused(
            "[[profiles.list]]\nid = \"first\"\nname = \"Other\"\nbadge = \"X\"\nprogram = \"x\"",
            ProfileProblemKind::IdDuplicate {
                id: "first".to_owned(),
            },
            8,
            6,
        );
    }

    #[test]
    fn blank_name_is_refused() {
        assert_item_refused(
            "[[profiles.list]]\nid = \"x\"\nname = \"   \"\nbadge = \"X\"\nprogram = \"x\"",
            ProfileProblemKind::NameEmpty,
            9,
            8,
        );
    }

    #[test]
    fn empty_badge_is_refused() {
        assert_item_refused(
            "[[profiles.list]]\nid = \"x\"\nname = \"X\"\nbadge = \"\"\nprogram = \"x\"",
            ProfileProblemKind::BadgeEmpty,
            10,
            9,
        );
    }

    #[test]
    fn badge_over_four_scalars_is_refused() {
        assert_item_refused(
            "[[profiles.list]]\nid = \"x\"\nname = \"X\"\nbadge = \"ABCDE\"\nprogram = \"x\"",
            ProfileProblemKind::BadgeTooLong,
            10,
            9,
        );
    }

    #[test]
    fn badge_counts_scalars_not_bytes() {
        let (config, problems) = checked(
            "[[profiles.list]]\nid = \"x\"\nname = \"X\"\nbadge = \"äöüß\"\nprogram = \"x\"\n",
        );
        assert!(problems.is_empty());
        assert_eq!(config.profiles.list[0].badge, "äöüß");
    }

    #[test]
    fn empty_program_is_refused() {
        assert_item_refused(
            "[[profiles.list]]\nid = \"x\"\nname = \"X\"\nbadge = \"X\"\nprogram = \"\"",
            ProfileProblemKind::ProgramEmpty,
            11,
            11,
        );
    }

    #[test]
    fn missing_program_is_refused() {
        assert_item_refused(
            "[[profiles.list]]\nid = \"x\"\nname = \"X\"\nbadge = \"X\"",
            ProfileProblemKind::ProgramEmpty,
            7,
            1,
        );
    }

    // ---- type = "ssh" (ADR-0072 §6) ----

    /// An SSH item between two valid local ones, `extra` lines after the
    /// mandatory `host` and `user`. The item starts at line 7.
    fn ssh_item(extra: &str) -> String {
        format!(
            "[[profiles.list]]\nid = \"x\"\nname = \"X\"\ntype = \"ssh\"\nhost = \"10.4.1.20\"\nuser = \"deploy\"\n{extra}"
        )
    }

    fn ssh_def(extra: &str) -> ProfileDef {
        let (config, problems) = checked(&ssh_item(extra));
        assert!(problems.is_empty(), "{problems:?}");
        config.profiles.list.into_iter().next().expect("one item")
    }

    #[test]
    fn a_minimal_ssh_profile_gets_the_defaults() {
        let def = ssh_def("");

        assert_eq!(def.kind, ProfileKind::Ssh);
        assert_eq!(def.badge, "SSH");
        assert!(def.program.is_empty() && def.args.is_empty() && def.cwd.is_empty());
        let ssh = def.ssh.expect("ssh part");
        assert_eq!(ssh.host, "10.4.1.20");
        assert_eq!(ssh.port, 22);
        assert_eq!(ssh.user, "deploy");
        assert_eq!(ssh.identity_file, "");
        // No `identity_file`: the key is left out of the default order.
        assert_eq!(ssh.auth, [SshAuth::Agent, SshAuth::Password]);
    }

    #[test]
    fn the_default_auth_order_tries_the_key_when_there_is_a_file() {
        let def = ssh_def("identity_file = \"~/.ssh/id_ed25519\"\n");

        assert_eq!(
            def.ssh.unwrap().auth,
            [SshAuth::Agent, SshAuth::Key, SshAuth::Password]
        );
    }

    #[test]
    fn explicit_ssh_values_are_kept_in_the_order_given() {
        let def = ssh_def(
            "badge = \"PRD\"\nport = 2222\nauth = [\"password\", \"agent\"]\nidentity_file = \"/k\"\n",
        );

        assert_eq!(def.badge, "PRD");
        let ssh = def.ssh.unwrap();
        assert_eq!(ssh.port, 2222);
        assert_eq!(ssh.auth, [SshAuth::Password, SshAuth::Agent]);
        assert_eq!(ssh.identity_file, "/k");
    }

    #[test]
    fn key_alone_with_a_file_and_key_among_other_methods_are_both_valid() {
        assert_eq!(
            ssh_def("auth = [\"key\"]\nidentity_file = \"/k\"\n")
                .ssh
                .unwrap()
                .auth,
            [SshAuth::Key]
        );
        // `key` among other methods needs no file: it is simply not usable.
        assert_eq!(
            ssh_def("auth = [\"agent\", \"key\"]\n").ssh.unwrap().auth,
            [SshAuth::Agent, SshAuth::Key]
        );
    }

    fn assert_ssh_refused(extra: &str, kind: ProfileProblemKind, line: usize, column: usize) {
        assert_item_refused(&ssh_item(extra), kind, line, column);
    }

    #[test]
    fn program_args_env_and_cwd_are_errors_in_an_ssh_profile() {
        // Lines: item at 7, id 8, name 9, type 10, host 11, user 12, extra 13.
        assert_ssh_refused(
            "program = \"ssh\"\n",
            ProfileProblemKind::SshFieldNotAllowed { field: "program" },
            13,
            11,
        );
        assert_ssh_refused(
            "args = [\"-v\"]\n",
            ProfileProblemKind::SshFieldNotAllowed { field: "args" },
            13,
            8,
        );
        assert_ssh_refused(
            "env = { A = \"b\" }\n",
            ProfileProblemKind::SshFieldNotAllowed { field: "env" },
            13,
            7,
        );
        assert_ssh_refused(
            "cwd = \"/tmp\"\n",
            ProfileProblemKind::SshFieldNotAllowed { field: "cwd" },
            13,
            7,
        );
    }

    #[test]
    fn host_with_a_character_that_would_corrupt_known_hosts_is_refused() {
        for bad in [
            "a b",
            "a,b",
            "a|b",
            "[::1]",
            "a@b",
            "a#b",
            "*.example.org",
            "a?b",
            "!a",
            "a\\tb",
            "",
        ] {
            let text = format!(
                "[[profiles.list]]\nid = \"x\"\nname = \"X\"\ntype = \"ssh\"\nhost = \"{bad}\"\nuser = \"u\"\n"
            );
            let (config, problems) = checked(&text);
            assert!(config.profiles.list.is_empty(), "{bad:?}");
            assert_eq!(problems.len(), 1, "{bad:?}");
            assert_eq!(problems[0].kind, ProfileProblemKind::HostInvalid, "{bad:?}");
        }
    }

    #[test]
    fn a_missing_host_is_refused_pointing_at_the_item() {
        let (config, problems) =
            checked("[[profiles.list]]\nid = \"x\"\nname = \"X\"\ntype = \"ssh\"\nuser = \"u\"\n");

        assert!(config.profiles.list.is_empty());
        assert_eq!(
            problems,
            vec![ProfileProblem {
                line: 1,
                column: 1,
                kind: ProfileProblemKind::HostInvalid
            }]
        );
    }

    #[test]
    fn an_ipv6_literal_without_brackets_is_a_valid_host() {
        let def = ssh_def("");
        assert_eq!(def.ssh.unwrap().host, "10.4.1.20");
        let (config, problems) = checked(
            "[[profiles.list]]\nid = \"x\"\nname = \"X\"\ntype = \"ssh\"\nhost = \"::1\"\nuser = \"u\"\n",
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(config.profiles.list[0].ssh.as_ref().unwrap().host, "::1");
    }

    #[test]
    fn user_is_required_and_cannot_hold_at_colon_or_whitespace() {
        for bad in ["a@b", "a:b", "a b", ""] {
            let text = format!(
                "[[profiles.list]]\nid = \"x\"\nname = \"X\"\ntype = \"ssh\"\nhost = \"h\"\nuser = \"{bad}\"\n"
            );
            let (config, problems) = checked(&text);
            assert!(config.profiles.list.is_empty(), "{bad:?}");
            assert_eq!(problems[0].kind, ProfileProblemKind::UserInvalid, "{bad:?}");
        }
        let (config, problems) =
            checked("[[profiles.list]]\nid = \"x\"\nname = \"X\"\ntype = \"ssh\"\nhost = \"h\"\n");
        assert!(config.profiles.list.is_empty());
        assert_eq!(problems[0].kind, ProfileProblemKind::UserInvalid);
    }

    #[test]
    fn a_port_outside_the_range_is_refused() {
        for bad in ["0", "65536", "-1"] {
            assert_ssh_refused(
                &format!("port = {bad}\n"),
                ProfileProblemKind::PortInvalid,
                13,
                8,
            );
        }
    }

    #[test]
    fn auth_accepts_only_the_three_methods_once_each_and_not_empty() {
        for bad in [
            "auth = [\"agent\", \"telnet\"]\n",
            "auth = [\"agent\", \"agent\"]\n",
            "auth = []\n",
        ] {
            assert_ssh_refused(bad, ProfileProblemKind::AuthInvalid, 13, 8);
        }
    }

    #[test]
    fn key_as_the_only_method_without_identity_file_is_refused() {
        assert_ssh_refused(
            "auth = [\"key\"]\n",
            ProfileProblemKind::KeyWithoutIdentityFile,
            13,
            8,
        );
    }

    #[test]
    fn a_secret_looking_key_is_an_unknown_key_and_its_value_is_never_kept() {
        let parsed = parse_checked(&ssh_item(
            "password = \"hunter2\"\nssh_passphrase = \"x\"\nApiSecret = \"y\"\n",
        ))
        .unwrap();

        let mut keys = parsed.unknown_keys.clone();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "profiles.list.0.ApiSecret".to_owned(),
                "profiles.list.0.password".to_owned(),
                "profiles.list.0.ssh_passphrase".to_owned(),
            ]
        );
        assert!(parsed.unknown_keys.iter().all(|k| is_secret_profile_key(k)));
        // The profile is still valid, and nothing of the secret is in it.
        assert!(parsed.profile_problems.is_empty());
        let rendered = format!("{:?}", parsed.config.profiles);
        assert!(!rendered.contains("hunter2"), "{rendered}");
    }

    #[test]
    fn only_secret_names_inside_a_profile_are_secret_keys() {
        assert!(is_secret_profile_key("profiles.list.3.password"));
        assert!(is_secret_profile_key("profiles.list.0.my_Passphrase_file"));
        assert!(!is_secret_profile_key("profiles.list.0.foo"));
        assert!(!is_secret_profile_key("general.password"));
        assert!(!is_secret_profile_key("shell.env.SECRET"));
    }

    #[test]
    fn an_ssh_profile_resolves_to_a_connection_not_a_program() {
        let (config, _) = checked(&ssh_item(
            "port = 2200\nauth = [\"key\"]\nidentity_file = \"/keys/id\"\n",
        ));

        let resolved = resolve_profile(&config, ProfileRef::Id("x")).unwrap();

        assert_eq!(resolved.id, "x");
        assert_eq!(resolved.badge.as_deref(), Some("SSH"));
        assert_eq!(resolved.program, None);
        assert!(resolved.args.is_empty() && resolved.env.is_empty());
        assert_eq!(
            resolved.ssh,
            Some(ResolvedSsh {
                name: "X".to_owned(),
                host: "10.4.1.20".to_owned(),
                port: 2200,
                user: "deploy".to_owned(),
                auth: vec![SshAuth::Key],
                identity_file: Some(PathBuf::from("/keys/id")),
            })
        );
    }

    #[test]
    fn the_identity_file_expands_a_leading_tilde_on_resolve() {
        let (config, _) = checked(&ssh_item("identity_file = \"~/.ssh/id_ed25519\"\n"));

        let resolved = resolve_profile(&config, ProfileRef::Id("x")).unwrap();

        let file = resolved.ssh.unwrap().identity_file.unwrap();
        assert!(file.ends_with(".ssh/id_ed25519") || file.ends_with(".ssh\\id_ed25519"));
        assert!(!file.to_string_lossy().starts_with('~'), "{file:?}");
    }

    #[test]
    fn local_profiles_resolve_without_an_ssh_part() {
        let (config, _) = checked(
            "[[profiles.list]]\nid = \"a\"\nname = \"A\"\nbadge = \"A\"\nprogram = \"x\"\n",
        );

        let resolved = resolve_profile(&config, ProfileRef::Id("a")).unwrap();

        assert_eq!(resolved.ssh, None);
        assert_eq!(resolved.program.as_deref(), Some("x"));
    }

    #[test]
    fn unknown_type_is_refused() {
        assert_item_refused(
            "[[profiles.list]]\nid = \"x\"\nname = \"X\"\nbadge = \"X\"\ntype = \"telnet\"\nprogram = \"x\"",
            ProfileProblemKind::TypeUnknown,
            11,
            8,
        );
    }

    #[test]
    fn unknown_default_is_refused_and_reset() {
        let (config, problems) = checked(
            "[profiles]\ndefault = \"nope\"\n[[profiles.list]]\nid = \"a\"\nname = \"A\"\nbadge = \"A\"\nprogram = \"x\"\n",
        );
        assert_eq!(config.profiles.default, "");
        assert_eq!(config.profiles.list.len(), 1);
        assert_eq!(
            problems,
            vec![ProfileProblem {
                line: 2,
                column: 11,
                kind: ProfileProblemKind::DefaultUnknown {
                    id: "nope".to_owned()
                },
            }]
        );
    }

    #[test]
    fn default_pointing_at_a_dropped_item_is_refused() {
        let (config, problems) = checked(
            "[profiles]\ndefault = \"a\"\n[[profiles.list]]\nid = \"a\"\nname = \"\"\nbadge = \"A\"\nprogram = \"x\"\n",
        );
        assert_eq!(config.profiles.default, "");
        assert_eq!(problems.len(), 2);
    }

    #[test]
    fn plain_parse_drops_the_problems_and_keeps_the_valid_items() {
        let (config, _) = crate::parse(
            "[[profiles.list]]\nid = \"BAD\"\nname = \"X\"\nbadge = \"X\"\nprogram = \"x\"\n[[profiles.list]]\nid = \"ok\"\nname = \"X\"\nbadge = \"X\"\nprogram = \"x\"\n",
        )
        .unwrap();
        assert_eq!(config.profiles.list.len(), 1);
        assert_eq!(config.profiles.problems, Vec::new());
    }

    fn config_of(text: &str) -> Config {
        checked(text).0
    }

    #[test]
    fn default_resolves_to_the_shell_with_an_empty_program() {
        let config = config_of("[shell]\nargs = [\"-l\"]\n[shell.env]\nEDITOR = \"vim\"\n");
        let resolved = resolve_profile(&config, ProfileRef::Default).unwrap();
        assert_eq!(
            resolved,
            ResolvedProfile {
                id: String::new(),
                badge: None,
                program: None,
                args: vec!["-l".to_owned()],
                env: vec![("EDITOR".to_owned(), "vim".to_owned())],
                cwd: None,
                ssh: None,
            }
        );
        assert_eq!(resolve_profile(&config, ProfileRef::Id("")), Ok(resolved));
    }

    #[test]
    fn shell_with_a_program_keeps_it() {
        let config = config_of("[shell]\nprogram = \"zsh\"\n");
        let resolved = resolve_profile(&config, ProfileRef::Default).unwrap();
        assert_eq!(resolved.program.as_deref(), Some("zsh"));
    }

    const WSL_PROFILE: &str = "[[profiles.list]]\nid = \"wsl\"\nname = \"Ubuntu\"\nbadge = \"WSL\"\nprogram = \"wsl.exe\"\nargs = [\"-d\", \"Ubuntu\"]\n";

    #[test]
    fn default_can_point_at_a_declared_profile() {
        let config = config_of(&format!("[profiles]\ndefault = \"wsl\"\n{WSL_PROFILE}"));
        let resolved = resolve_profile(&config, ProfileRef::Default).unwrap();
        assert_eq!(resolved.id, "wsl");
        assert_eq!(resolved.badge.as_deref(), Some("WSL"));
        assert_eq!(resolved.program.as_deref(), Some("wsl.exe"));
        assert_eq!(resolved.args, ["-d", "Ubuntu"]);
        // The shell stays reachable by its internal id.
        let shell = resolve_profile(&config, ProfileRef::Id("")).unwrap();
        assert_eq!(shell.program, None);
    }

    #[test]
    fn profile_env_is_merged_over_shell_env() {
        let config = config_of(
            "[shell.env]\nA = \"shell\"\nB = \"shell\"\n[[profiles.list]]\nid = \"p\"\nname = \"P\"\nbadge = \"P\"\nprogram = \"x\"\n[profiles.list.env]\nB = \"profile\"\nC = \"profile\"\n",
        );
        let resolved = resolve_profile(&config, ProfileRef::Id("p")).unwrap();
        assert_eq!(
            resolved.env,
            vec![
                ("A".to_owned(), "shell".to_owned()),
                ("B".to_owned(), "profile".to_owned()),
                ("C".to_owned(), "profile".to_owned()),
            ]
        );
    }

    #[test]
    fn cwd_expands_the_tilde() {
        let home = dirs::home_dir().expect("home dir must exist in test env");
        let config = config_of(
            "[[profiles.list]]\nid = \"p\"\nname = \"P\"\nbadge = \"P\"\nprogram = \"x\"\ncwd = \"~/projetos\"\n[[profiles.list]]\nid = \"q\"\nname = \"Q\"\nbadge = \"Q\"\nprogram = \"x\"\n",
        );
        assert_eq!(
            resolve_profile(&config, ProfileRef::Id("p")).unwrap().cwd,
            Some(home.join("projetos"))
        );
        assert_eq!(
            resolve_profile(&config, ProfileRef::Id("q")).unwrap().cwd,
            None
        );
    }

    #[test]
    fn unknown_id_is_missing() {
        let config = Config::default();
        assert_eq!(
            resolve_profile(&config, ProfileRef::Id("nope")),
            Err(MissingProfile {
                id: "nope".to_owned()
            })
        );
    }

    fn unix_shells() -> Vec<String> {
        vec!["/bin/sh".to_owned(), "/bin/zsh".to_owned()]
    }

    #[test]
    fn auto_windows_shells_are_accepted() {
        let resolved =
            resolve_auto_profile("auto:shell:pwsh.exe", AutoPlatform::Windows, &[]).unwrap();
        assert_eq!(resolved.program.as_deref(), Some("pwsh.exe"));
        assert!(resolved.args.is_empty());
        assert_eq!(resolved.badge.as_deref(), Some("PS"));
        let cmd = resolve_auto_profile("auto:shell:cmd.exe", AutoPlatform::Windows, &[]).unwrap();
        assert_eq!(cmd.badge.as_deref(), Some("CMD"));
    }

    #[test]
    fn auto_wsl_distro_is_a_separate_argument() {
        let resolved =
            resolve_auto_profile("auto:wsl:Ubuntu-24.04", AutoPlatform::Windows, &[]).unwrap();
        assert_eq!(resolved.id, "auto:wsl:Ubuntu-24.04");
        assert_eq!(resolved.program.as_deref(), Some("wsl.exe"));
        assert_eq!(resolved.args, ["-d", "Ubuntu-24.04"]);
        assert_eq!(resolved.badge.as_deref(), Some("WSL"));
    }

    #[test]
    fn auto_unix_shell_must_be_listed() {
        let resolved =
            resolve_auto_profile("auto:shell:/bin/zsh", AutoPlatform::Unix, &unix_shells())
                .unwrap();
        assert_eq!(resolved.program.as_deref(), Some("/bin/zsh"));
        assert_eq!(resolved.badge.as_deref(), Some("ZSH"));
    }

    #[test]
    fn auto_refuses_what_detection_would_not_offer() {
        let refused = [
            ("auto:shell:C:\\x\\evil.exe", AutoPlatform::Windows),
            ("auto:shell:pwsh", AutoPlatform::Windows),
            ("auto:shell:/bin/zsh", AutoPlatform::Windows),
            ("auto:wsl:a b;c", AutoPlatform::Windows),
            ("auto:wsl:", AutoPlatform::Windows),
            ("auto:wsl:Ubuntu", AutoPlatform::Unix),
            ("auto:shell:/tmp/x", AutoPlatform::Unix),
            ("auto:shell:pwsh.exe", AutoPlatform::Unix),
            ("auto:other:x", AutoPlatform::Windows),
            ("auto:shell", AutoPlatform::Windows),
            ("shell:pwsh.exe", AutoPlatform::Windows),
        ];
        for (id, platform) in refused {
            assert_eq!(
                resolve_auto_profile(id, platform, &unix_shells()),
                Err(MissingProfile { id: id.to_owned() }),
                "{id} on {platform:?}"
            );
        }
        let too_long = format!("auto:wsl:{}", "a".repeat(65));
        assert!(resolve_auto_profile(&too_long, AutoPlatform::Windows, &[]).is_err());
    }

    #[test]
    fn resolve_profile_validates_auto_ids() {
        let config = Config::default();
        assert!(resolve_profile(&config, ProfileRef::Id("auto:shell:/tmp/x")).is_err());
        assert!(resolve_profile(&config, ProfileRef::Id("auto:shell:C:\\x\\evil.exe")).is_err());
        assert!(resolve_profile(&config, ProfileRef::Id("auto:wsl:a b;c")).is_err());
        if cfg!(windows) {
            let resolved =
                resolve_profile(&config, ProfileRef::Id("auto:wsl:Ubuntu-24.04")).unwrap();
            assert_eq!(resolved.args, ["-d", "Ubuntu-24.04"]);
        }
    }

    #[test]
    fn auto_profile_gets_the_shell_env() {
        if !cfg!(windows) {
            return;
        }
        let config = config_of("[shell.env]\nEDITOR = \"vim\"\n");
        let resolved = resolve_profile(&config, ProfileRef::Id("auto:shell:cmd.exe")).unwrap();
        assert_eq!(resolved.env, vec![("EDITOR".to_owned(), "vim".to_owned())]);
    }
}
