// SPDX-License-Identifier: GPL-3.0-or-later

//! Which profile each spawn path launches (ADR-0070 §2-§4). Pure: every
//! decision is a function of the config and the id the pane carries, so each
//! path is testable without a window.
//!
//! - new tab, `window.new`, first window without session: the default;
//! - split: the origin pane's profile, never the default;
//! - restore (also first focus of a `NotStarted` tab and sibling panes): the
//!   saved profile, resolved when the process starts.

use porecatu_config::{
    Config, MissingProfile, ProfileRef, ResolvedProfile, ResolvedSsh, SshAuth, resolve_profile,
};
use porecatu_term::{AuthMethod, SshSpawn, resolve_default_shell, search_path};

/// What to spawn, and the saved id that could not be honored (so the pane
/// gets `note.profile_missing`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpawnProfile {
    pub(crate) resolved: ResolvedProfile,
    /// A restored pane whose saved profile no longer exists, or whose `auto:`
    /// id failed validation: the id as it was saved.
    pub(crate) missing: Option<String>,
}

impl SpawnProfile {
    fn found(resolved: ResolvedProfile) -> Self {
        Self {
            resolved,
            missing: None,
        }
    }
}

/// The `[shell]` itself (internal id `""`), which always resolves.
fn shell_profile(config: &Config) -> ResolvedProfile {
    resolve_profile(config, ProfileRef::Id("")).expect("the [shell] always resolves")
}

/// `[profiles] default`, or the `[shell]` when it cannot be resolved.
pub(crate) fn default_profile(config: &Config) -> SpawnProfile {
    SpawnProfile::found(
        resolve_profile(config, ProfileRef::Default).unwrap_or_else(|_| shell_profile(config)),
    )
}

/// A profile the user picked from the menu or by `profile.new_tab_N`
/// (`""` is the `[shell]`). One that vanished from the config since the menu
/// opened falls back to the default.
pub(crate) fn chosen_profile(config: &Config, id: &str) -> SpawnProfile {
    match resolve_profile(config, ProfileRef::Id(id)) {
        Ok(resolved) => SpawnProfile::found(resolved),
        Err(_) => default_profile(config),
    }
}

/// A split launches "one more of these" (RF-7.15): the origin pane's
/// profile, `None` being the `[shell]` whatever `default` says. A profile
/// that left the config since the pane opened falls back to the default.
pub(crate) fn split_profile(config: &Config, origin: Option<&str>) -> SpawnProfile {
    match resolve_saved(config, origin) {
        Ok(resolved) => SpawnProfile::found(resolved),
        Err(_) => default_profile(config),
    }
}

/// A restored pane launches the profile it was saved with. No `profile`
/// means the `[shell]`, even if `default` points elsewhere now (ADR-0070 §4).
/// A missing id launches the default and carries the id for the note.
pub(crate) fn restored_profile(config: &Config, saved: Option<&str>) -> SpawnProfile {
    match resolve_saved(config, saved) {
        Ok(resolved) => SpawnProfile::found(resolved),
        Err(MissingProfile { id }) => SpawnProfile {
            missing: Some(id),
            ..default_profile(config)
        },
    }
}

fn resolve_saved(config: &Config, saved: Option<&str>) -> Result<ResolvedProfile, MissingProfile> {
    resolve_profile(config, ProfileRef::Id(saved.unwrap_or("")))
}

/// The badge a tab shows for a pane running `pane_profile` (ADR-0070 §7,
/// RF-7.13): only when it is **not** the default profile, never for the
/// `[shell]`, and none when the profile cannot be resolved (nothing to draw).
pub(crate) fn badge_to_show(
    config: &Config,
    pane_profile: Option<&str>,
) -> Option<porecatu_core::ProfileBadge> {
    let resolved = resolve_profile(config, ProfileRef::Id(pane_profile?)).ok()?;
    if resolved.id.is_empty() || resolved.id == default_profile(config).resolved.id {
        return None;
    }
    let name = match config
        .profiles
        .list
        .iter()
        .find(|def| def.id == resolved.id)
    {
        Some(def) => def.name.clone(),
        None => match resolved.args.as_slice() {
            // `auto:wsl:<distro>` launches `wsl.exe -d <distro>`.
            [flag, distro] if flag == "-d" => distro.clone(),
            _ => display_name(&resolved),
        },
    };
    Some(porecatu_core::ProfileBadge {
        badge: resolved.badge?,
        name,
    })
}

/// What `porecatu-term` needs to open the channel of an SSH profile
/// (ADR-0072 §4, §6): the destination, the methods in the order to try, the
/// key file, and the size of the grid the pty is born with. A split still
/// opens a **new connection** (sharing is a later step).
pub(crate) fn ssh_spawn(ssh: &ResolvedSsh, rows: u16, cols: u16) -> SshSpawn {
    let mut spawn = SshSpawn::new(ssh.host.clone(), ssh.port, ssh.user.clone());
    spawn.auth = ssh
        .auth
        .iter()
        .map(|method| match method {
            SshAuth::Agent => AuthMethod::Agent,
            SshAuth::Key => AuthMethod::Key,
            SshAuth::Password => AuthMethod::Password,
        })
        .collect();
    spawn.identity_file = ssh.identity_file.clone();
    spawn.rows = rows;
    spawn.cols = cols;
    spawn
}

/// Whether a saved profile id launches an SSH connection **when restored**
/// (RF-20.33): the answer of `restored_profile`, so an id that vanished from
/// the config and fell back to an SSH default counts too -- the restore must
/// never connect on its own either way.
pub(crate) fn restores_as_ssh(config: &Config, saved: &str) -> bool {
    restored_profile(config, Some(saved)).resolved.ssh.is_some()
}

/// What the user did to a pane, as far as a dormant SSH pane cares
/// (RF-20.33, ADR-0075 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneGesture {
    /// A left click inside the pane.
    Click,
    /// `Enter`, with the pane focused.
    Enter,
    /// The pane only gained focus: `Ctrl+Tab`, `tab.goto_N`, the tab the
    /// restore picked as active, a pane-focus action.
    Focus,
    /// Any other key.
    Key,
}

/// Does this gesture connect a restored SSH pane? Only the two that say "I
/// mean this pane": a click in it and `Enter` on it. Gaining focus never does
/// -- opening the app, or walking through tabs, must not reach a server.
pub(crate) fn gesture_connects(gesture: PaneGesture) -> bool {
    matches!(gesture, PaneGesture::Click | PaneGesture::Enter)
}

/// `user@host`, what the status bar shows in place of a shell name
/// (RF-20.21).
pub(crate) fn ssh_target(ssh: &ResolvedSsh) -> String {
    format!("{}@{}", ssh.user, ssh.host)
}

/// The directory an SSH pane's OSC 7 reports, **only when** the host in it is
/// the profile's (RF-20.21): the path is the server's, and a shell that names
/// another machine -- or none -- is not vouching for it. Host names compare
/// ignoring case. The result is never a local directory (RF-20.22).
pub(crate) fn remote_cwd(
    profile_host: &str,
    osc7_host: Option<&str>,
    path: &std::path::Path,
) -> Option<std::path::PathBuf> {
    osc7_host
        .filter(|host| host.eq_ignore_ascii_case(profile_host))
        .map(|_| path.to_path_buf())
}

/// The id the pane records: `None` for the `[shell]`, never `""`.
pub(crate) fn pane_profile_id(resolved: &ResolvedProfile) -> Option<String> {
    (!resolved.id.is_empty()).then(|| resolved.id.clone())
}

/// Display name of what the profile launches (the title fallback of
/// RF-1.7/ADR-0017): its `program`, or the detected shell when empty, cut to
/// the base name without path or extension.
pub(crate) fn display_name(resolved: &ResolvedProfile) -> String {
    // An SSH profile launches no local program: with no custom title and no
    // OSC 0/2, the tab is labeled by the profile's name (RF-20.20).
    if let Some(ssh) = &resolved.ssh {
        return ssh.name.clone();
    }
    let program = match &resolved.program {
        Some(program) => program.clone(),
        None => resolve_default_shell(std::env::var("SHELL").ok().as_deref(), search_path),
    };
    std::path::Path::new(&program)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> Config {
        porecatu_config::parse(text).expect("valid toml").0
    }

    const WSL: &str = "[[profiles.list]]\nid = \"wsl\"\nname = \"Ubuntu\"\nbadge = \"WSL\"\nprogram = \"wsl.exe\"\nargs = [\"-d\", \"Ubuntu\"]\n";

    #[test]
    fn without_profiles_every_path_launches_the_shell() {
        let config =
            config("[shell]\nprogram = \"zsh\"\nargs = [\"-l\"]\n[shell.env]\nA = \"1\"\n");
        let shell = shell_profile(&config);
        assert_eq!(shell.program.as_deref(), Some("zsh"));
        assert_eq!(shell.args, ["-l"]);
        assert_eq!(shell.env, [("A".to_owned(), "1".to_owned())]);
        assert_eq!(default_profile(&config), SpawnProfile::found(shell.clone()));
        assert_eq!(
            split_profile(&config, None),
            SpawnProfile::found(shell.clone())
        );
        assert_eq!(restored_profile(&config, None), SpawnProfile::found(shell));
        assert_eq!(pane_profile_id(&default_profile(&config).resolved), None);
    }

    #[test]
    fn default_points_new_tabs_at_the_profile_but_not_splits_of_the_shell() {
        let config = config(&format!("[profiles]\ndefault = \"wsl\"\n{WSL}"));
        let new_tab = default_profile(&config);
        assert_eq!(new_tab.resolved.id, "wsl");
        assert_eq!(pane_profile_id(&new_tab.resolved).as_deref(), Some("wsl"));

        // A pane of the `[shell]` splits into the `[shell]`.
        let shell_split = split_profile(&config, None);
        assert_eq!(shell_split.resolved.id, "");
        assert_eq!(shell_split.resolved.program, None);
        // A pane of `wsl` splits into `wsl`.
        let wsl_split = split_profile(&config, Some("wsl"));
        assert_eq!(wsl_split.resolved.program.as_deref(), Some("wsl.exe"));
        assert_eq!(wsl_split.missing, None);
    }

    #[test]
    fn restore_without_profile_launches_the_shell_even_if_default_moved() {
        let config = config(&format!("[profiles]\ndefault = \"wsl\"\n{WSL}"));
        let restored = restored_profile(&config, None);
        assert_eq!(restored.resolved.id, "");
        assert_eq!(restored.missing, None);
        assert_eq!(
            restored_profile(&config, Some("wsl")).resolved.id,
            "wsl",
            "a saved declared profile comes back"
        );
    }

    #[test]
    fn restore_of_a_missing_profile_launches_the_default_with_a_note() {
        let config = config(&format!("[profiles]\ndefault = \"wsl\"\n{WSL}"));
        let restored = restored_profile(&config, Some("py"));
        assert_eq!(restored.resolved.id, "wsl", "the default, not the shell");
        assert_eq!(restored.missing.as_deref(), Some("py"));

        let plain = restored_profile(&Config::default(), Some("py"));
        assert_eq!(plain.resolved.id, "");
        assert_eq!(plain.missing.as_deref(), Some("py"));
    }

    #[test]
    fn restore_refuses_auto_ids_that_detection_would_not_offer() {
        let config = Config::default();
        for id in [
            "auto:shell:C:\\x\\evil.exe",
            "auto:shell:/tmp/x",
            "auto:wsl:a b;c",
        ] {
            let restored = restored_profile(&config, Some(id));
            assert_eq!(restored.missing.as_deref(), Some(id), "{id}");
            assert_eq!(restored.resolved.id, "", "{id}");
        }
    }

    #[test]
    fn split_of_a_profile_removed_from_the_config_falls_back_to_the_default() {
        let config = config(&format!("[profiles]\ndefault = \"wsl\"\n{WSL}"));
        let split = split_profile(&config, Some("gone"));
        assert_eq!(split.resolved.id, "wsl");
        assert_eq!(split.missing, None, "a live split raises no note");
    }

    #[test]
    fn badge_shows_only_for_a_profile_that_is_not_the_default() {
        let config = config(&format!(
            "[profiles]\ndefault = \"a\"\n{}{}",
            "[[profiles.list]]\nid = \"a\"\nname = \"A\"\nbadge = \"AA\"\nprogram = \"a\"\n",
            "[[profiles.list]]\nid = \"b\"\nname = \"B\"\nbadge = \"BB\"\nprogram = \"b\"\n"
        ));
        assert_eq!(badge_to_show(&config, Some("b")).unwrap().badge, "BB");
        assert_eq!(badge_to_show(&config, Some("a")), None, "the default");
        assert_eq!(badge_to_show(&config, None), None, "the [shell]");
        assert_eq!(badge_to_show(&config, Some("gone")), None);
    }

    #[test]
    fn no_profile_declared_means_no_badge_anywhere() {
        let config = config("");
        assert_eq!(badge_to_show(&config, None), None);
        assert_eq!(badge_to_show(&config, Some("")), None);
    }

    /// With the `[shell]` as the default, a declared profile is not the
    /// default and shows its badge; the `[shell]` pane still shows none.
    #[test]
    fn declared_profile_shows_a_badge_when_the_shell_is_the_default() {
        let config = config(WSL);
        assert_eq!(badge_to_show(&config, Some("wsl")).unwrap().badge, "WSL");
        assert_eq!(badge_to_show(&config, None), None);
    }

    const SSH: &str = "[[profiles.list]]\nid = \"prod\"\nname = \"prod-web-01\"\ntype = \"ssh\"\nhost = \"10.4.1.20\"\nport = 2222\nuser = \"deploy\"\nauth = [\"key\", \"agent\"]\nidentity_file = \"/keys/id\"\n";

    #[test]
    fn an_ssh_profile_becomes_an_ssh_spawn_with_the_grid_size() {
        let config = config(SSH);
        let resolved = resolve_profile(&config, ProfileRef::Id("prod")).unwrap();

        let spawn = ssh_spawn(resolved.ssh.as_ref().unwrap(), 31, 117);

        assert_eq!(spawn.host, "10.4.1.20");
        assert_eq!(spawn.port, 2222);
        assert_eq!(spawn.user, "deploy");
        // The order the profile gives is the order tried.
        assert_eq!(spawn.auth, [AuthMethod::Key, AuthMethod::Agent]);
        assert_eq!(
            spawn.identity_file.as_deref(),
            Some(std::path::Path::new("/keys/id"))
        );
        assert_eq!((spawn.rows, spawn.cols), (31, 117));
        assert_eq!(spawn.term, "xterm-256color");
        assert_eq!(spawn.connection, porecatu_term::Connection::New);
    }

    #[test]
    fn an_ssh_profile_without_a_key_file_or_auth_gets_the_default_order() {
        let config = config(
            "[[profiles.list]]\nid = \"p\"\nname = \"P\"\ntype = \"ssh\"\nhost = \"h\"\nuser = \"u\"\n",
        );
        let resolved = resolve_profile(&config, ProfileRef::Id("p")).unwrap();

        let spawn = ssh_spawn(resolved.ssh.as_ref().unwrap(), 24, 80);

        assert_eq!(spawn.port, 22);
        assert_eq!(spawn.auth, [AuthMethod::Agent, AuthMethod::Password]);
        assert_eq!(spawn.identity_file, None);
    }

    #[test]
    fn an_ssh_pane_is_titled_by_the_profile_name_and_never_by_the_local_shell() {
        let config = config(SSH);
        let resolved = resolve_profile(&config, ProfileRef::Id("prod")).unwrap();

        assert_eq!(display_name(&resolved), "prod-web-01");
        // A split of an SSH pane launches the same profile (a new connection).
        let split = split_profile(&config, Some("prod"));
        assert!(split.resolved.ssh.is_some());
        assert_eq!(split.missing, None);
    }

    #[test]
    fn only_a_click_and_enter_connect_a_restored_pane_never_focus() {
        assert!(gesture_connects(PaneGesture::Click));
        assert!(gesture_connects(PaneGesture::Enter));
        assert!(!gesture_connects(PaneGesture::Focus));
        assert!(!gesture_connects(PaneGesture::Key));
    }

    #[test]
    fn an_ssh_profile_restores_dormant_a_local_one_and_the_shell_do_not() {
        let config = config(
            &(SSH.to_owned()
                + "\n[[profiles.list]]\nid = \"loc\"\nname = \"loc\"\nprogram = \"zsh\"\n"),
        );
        assert!(restores_as_ssh(&config, "prod"));
        assert!(!restores_as_ssh(&config, "loc"));
        assert!(!restores_as_ssh(&config, ""));
    }

    #[test]
    fn a_vanished_id_that_falls_back_to_an_ssh_default_still_restores_dormant() {
        let config = config(&(SSH.to_owned() + "\n[profiles]\ndefault = \"prod\"\n"));
        assert!(restores_as_ssh(&config, "gone"));
    }

    #[test]
    fn the_status_target_is_user_at_host_and_the_directory_needs_the_same_host() {
        let config = config(SSH);
        let resolved = resolve_profile(&config, ProfileRef::Id("prod")).unwrap();
        let ssh = resolved.ssh.as_ref().unwrap();
        assert_eq!(ssh_target(ssh), "deploy@10.4.1.20");

        let path = std::path::Path::new("/srv/app");
        assert_eq!(
            remote_cwd("10.4.1.20", Some("10.4.1.20"), path),
            Some(path.to_path_buf())
        );
        // Case does not matter in a host name.
        assert_eq!(
            remote_cwd("Prod.Example.COM", Some("prod.example.com"), path),
            Some(path.to_path_buf())
        );
        // Another machine, or none at all: not vouched for.
        assert_eq!(remote_cwd("10.4.1.20", Some("other"), path), None);
        assert_eq!(remote_cwd("10.4.1.20", None, path), None);
    }

    #[test]
    fn an_ssh_profile_shows_its_badge_and_the_default_ssh_one_when_none_is_named() {
        let config = config(SSH);
        let badge = badge_to_show(&config, Some("prod")).unwrap();
        assert_eq!(badge.badge, "SSH");
        assert_eq!(badge.name, "prod-web-01");
    }

    #[test]
    fn display_name_is_the_base_name_of_the_program() {
        // Forward slashes: a backslash is a separator only on Windows.
        let config = config(&format!("{WSL}[shell]\nprogram = \"C:/x/zsh.exe\"\n"));
        assert_eq!(
            display_name(&resolve_profile(&config, ProfileRef::Id("wsl")).unwrap()),
            "wsl"
        );
        assert_eq!(display_name(&shell_profile(&config)), "zsh");
        assert!(!display_name(&shell_profile(&Config::default())).is_empty());
    }
}
