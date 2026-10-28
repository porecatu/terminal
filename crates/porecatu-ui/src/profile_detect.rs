// SPDX-License-Identifier: GPL-3.0-or-later

//! Shell detection for the profile menu (ADR-0070 §5, PRD-007 RF-7.17/RF-7.18).
//!
//! Runs once per process, on a thread, the first time the menu opens with
//! `[profiles] detect = true`; the result goes back to the event loop as
//! **data** (same rule as the Git poll, ADR-0052). Every decision that can be
//! tested without the system is a pure function here: the two parsers, the
//! identifier-to-profile derivation and the give-up rule, which takes the
//! clock as a parameter. Failure and timeout are silent (RF-7.18).

use std::path::Path;
use std::time::{Duration, Instant};

use porecatu_config::{AutoPlatform, ResolvedProfile, resolve_auto_profile};

/// The whole detection thread gets this long; `wsl.exe` with the service
/// stopped can hang, and past the limit it is killed and WSL contributes
/// nothing.
pub(crate) const DETECT_LIMIT: Duration = Duration::from_secs(3);

/// The Windows shells looked up on `PATH`, in menu order.
#[cfg_attr(not(windows), allow(dead_code))]
const WINDOWS_SHELLS: [&str; 3] = ["pwsh.exe", "powershell.exe", "cmd.exe"];

/// A shell the machine offers. `id` is the deterministic `auto:` identifier
/// the session records; `name` is data, not a catalog phrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetectedProfile {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) resolved: ResolvedProfile,
}

/// When to stop waiting. The clock is an argument everywhere, so the rule is
/// testable without sleeping.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct Budget {
    started: Instant,
    limit: Duration,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Budget {
    pub(crate) fn new(started: Instant, limit: Duration) -> Self {
        Self { started, limit }
    }

    pub(crate) fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) >= self.limit
    }
}

/// Distribution names out of `wsl.exe --list --quiet`: UTF-16LE (not UTF-8),
/// `\r\n` line breaks, a possible BOM, empty lines. A stray odd byte is
/// dropped; unpaired surrogates become U+FFFD, which no distro name has.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parse_wsl_list(bytes: &[u8]) -> Vec<String> {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let text = String::from_utf16_lossy(&units);
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(|line| line.trim().trim_matches('\0'))
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Shells out of `/etc/shells`: comments and blank lines gone, and only paths
/// that exist (the check is a parameter so the parser stays pure).
#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn parse_etc_shells(text: &str, exists: impl Fn(&str) -> bool) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|path| exists(path))
        .map(str::to_owned)
        .collect()
}

/// Turns identifiers into profiles through the same validation the restore
/// uses, so what the menu offers is exactly what a restored session accepts.
/// What does not resolve is dropped.
pub(crate) fn build_detected(
    ids: &[String],
    platform: AutoPlatform,
    unix_shells: &[String],
) -> Vec<DetectedProfile> {
    ids.iter()
        .filter_map(|id| {
            let resolved = resolve_auto_profile(id, platform, unix_shells).ok()?;
            let (_, value) = id.strip_prefix("auto:")?.split_once(':')?;
            let name = if id.starts_with("auto:wsl:") {
                value.to_owned()
            } else {
                Path::new(value)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .filter(|stem| !stem.is_empty())
                    .unwrap_or_else(|| value.to_owned())
            };
            Some(DetectedProfile {
                id: id.clone(),
                name,
                resolved,
            })
        })
        .collect()
}

/// What launching the profile runs, for the "does not repeat a line above"
/// rule (RF-7.9): the program's file name, case-insensitive, and the
/// arguments. A directory in front of the same executable does not make it a
/// different shell.
pub(crate) fn launch_key(program: &str, args: &[String]) -> (String, Vec<String>) {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_lowercase();
    (name, args.to_vec())
}

/// Whether the thread starts now: only for a menu that is open, only once per
/// process, and never with `[profiles] detect = false` (RF-7.17).
pub(crate) fn detection_due(already_started: bool, detect: bool, menu_open: bool) -> bool {
    detect && menu_open && !already_started
}

/// Starts the detection thread. `report` receives the result and runs on that
/// thread; the caller forwards it to the event loop.
pub(crate) fn spawn(report: impl FnOnce(Vec<DetectedProfile>) + Send + 'static) {
    let _ = std::thread::Builder::new()
        .name("profile-detect".into())
        .spawn(move || report(detect(Budget::new(Instant::now(), DETECT_LIMIT))));
}

#[cfg(windows)]
fn detect(budget: Budget) -> Vec<DetectedProfile> {
    let mut ids: Vec<String> = WINDOWS_SHELLS
        .iter()
        .filter(|name| porecatu_term::search_path(name))
        .map(|name| format!("auto:shell:{name}"))
        .collect();
    ids.extend(
        wsl_distributions(budget)
            .into_iter()
            .map(|distro| format!("auto:wsl:{distro}")),
    );
    build_detected(&ids, AutoPlatform::Windows, &[])
}

#[cfg(not(windows))]
fn detect(_budget: Budget) -> Vec<DetectedProfile> {
    let Ok(text) = std::fs::read_to_string("/etc/shells") else {
        return Vec::new();
    };
    let shells = parse_etc_shells(&text, |path| Path::new(path).exists());
    let ids: Vec<String> = shells
        .iter()
        .map(|path| format!("auto:shell:{path}"))
        .collect();
    build_detected(&ids, AutoPlatform::Unix, &shells)
}

/// `wsl.exe --list --quiet`, without a console window, killed when the budget
/// runs out. Any failure is an empty list.
#[cfg(windows)]
fn wsl_distributions(budget: Budget) -> Vec<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let mut command = Command::new("wsl.exe");
    command
        .args(["--list", "--quiet"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::git::suppress_console_window(&mut command);
    let Ok(mut child) = command.spawn() else {
        return Vec::new();
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Vec::new();
                }
                let mut bytes = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_end(&mut bytes);
                }
                return parse_wsl_list(&bytes);
            }
            Ok(None) if budget.expired(Instant::now()) => {
                let _ = child.kill();
                let _ = child.wait();
                return Vec::new();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(text: &str, bom: bool) -> Vec<u8> {
        let mut units: Vec<u16> = Vec::new();
        if bom {
            units.push(0xfeff);
        }
        units.extend(text.encode_utf16());
        units.iter().flat_map(|u| u.to_le_bytes()).collect()
    }

    /// Real shape of the output: UTF-16LE, BOM, `\r\n`, a trailing blank line.
    #[test]
    fn wsl_list_decodes_utf16le_with_bom_and_crlf() {
        let bytes = utf16le("Ubuntu-24.04\r\ndocker-desktop\r\n\r\n", true);
        assert_eq!(&bytes[..2], &[0xff, 0xfe], "the fixture starts with a BOM");
        assert_eq!(parse_wsl_list(&bytes), ["Ubuntu-24.04", "docker-desktop"]);
    }

    #[test]
    fn wsl_list_without_bom_and_with_blank_lines() {
        let bytes = utf16le("\r\nDebian\r\n   \r\n", false);
        assert_eq!(parse_wsl_list(&bytes), ["Debian"]);
    }

    #[test]
    fn wsl_list_of_nothing_is_empty() {
        assert!(parse_wsl_list(&[]).is_empty());
        assert!(parse_wsl_list(&[0xff, 0xfe]).is_empty());
        // A stray odd byte does not panic or invent a name.
        assert!(parse_wsl_list(&[0x41]).is_empty());
    }

    #[test]
    fn etc_shells_drops_comments_blanks_and_missing_paths() {
        let text =
            "# valid login shells\n/bin/sh\n\n  /bin/zsh  \n#/bin/csh\n/usr/bin/fish\n/bin/gone\n";
        let exists = |path: &str| path != "/bin/gone";
        assert_eq!(
            parse_etc_shells(text, exists),
            ["/bin/sh", "/bin/zsh", "/usr/bin/fish"]
        );
    }

    #[test]
    fn ids_become_profiles_with_data_names_and_badges() {
        let ids: Vec<String> = [
            "auto:shell:pwsh.exe",
            "auto:shell:cmd.exe",
            "auto:wsl:Ubuntu-24.04",
        ]
        .map(str::to_owned)
        .to_vec();
        let found = build_detected(&ids, AutoPlatform::Windows, &[]);
        let view: Vec<_> = found
            .iter()
            .map(|d| (d.name.as_str(), d.resolved.badge.as_deref().unwrap()))
            .collect();
        assert_eq!(
            view,
            [("pwsh", "PS"), ("cmd", "CMD"), ("Ubuntu-24.04", "WSL")]
        );
        let wsl = &found[2].resolved;
        assert_eq!(wsl.program.as_deref(), Some("wsl.exe"));
        assert_eq!(wsl.args, ["-d", "Ubuntu-24.04"]);
    }

    #[test]
    fn unix_shells_get_the_first_three_letters_as_badge() {
        let shells = vec!["/bin/zsh".to_owned(), "/usr/bin/fish".to_owned()];
        let ids: Vec<String> = shells.iter().map(|s| format!("auto:shell:{s}")).collect();
        let found = build_detected(&ids, AutoPlatform::Unix, &shells);
        assert_eq!(found[0].id, "auto:shell:/bin/zsh");
        assert_eq!(found[0].name, "zsh");
        assert_eq!(found[0].resolved.badge.as_deref(), Some("ZSH"));
        assert_eq!(found[1].resolved.badge.as_deref(), Some("FIS"));
    }

    /// What the restore would refuse, the menu never offers.
    #[test]
    fn identifiers_the_restore_would_refuse_are_dropped() {
        let ids: Vec<String> = [
            "auto:shell:C:\\evil.exe",
            "auto:wsl:bad name",
            "auto:shell:pwsh.exe",
        ]
        .map(str::to_owned)
        .to_vec();
        let found = build_detected(&ids, AutoPlatform::Windows, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "auto:shell:pwsh.exe");
    }

    #[test]
    fn detection_runs_once_and_only_for_an_open_menu_with_detect_on() {
        assert!(detection_due(false, true, true));
        assert!(!detection_due(true, true, true), "once per process");
        assert!(!detection_due(false, false, true), "detect = false");
        assert!(!detection_due(false, true, false), "no menu, no detection");
    }

    #[test]
    fn launch_key_ignores_directory_and_case() {
        assert_eq!(
            launch_key("C:\\Program Files\\PowerShell\\7\\PWSH.EXE", &[]),
            launch_key("pwsh.exe", &[])
        );
        assert_ne!(
            launch_key("wsl.exe", &["-d".into(), "A".into()]),
            launch_key("wsl.exe", &["-d".into(), "B".into()])
        );
    }

    /// The give-up rule takes the clock: no sleeping, just two instants.
    #[test]
    fn the_budget_expires_at_the_limit_and_not_before() {
        let start = Instant::now();
        let budget = Budget::new(start, DETECT_LIMIT);
        assert!(!budget.expired(start));
        assert!(!budget.expired(start + DETECT_LIMIT - Duration::from_millis(1)));
        assert!(budget.expired(start + DETECT_LIMIT));
        assert!(budget.expired(start + DETECT_LIMIT * 2));
        // A clock that went backwards never expires it.
        if let Some(before) = start.checked_sub(Duration::from_secs(1)) {
            assert!(!budget.expired(before));
        }
    }
}
