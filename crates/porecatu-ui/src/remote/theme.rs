// SPDX-License-Identifier: GPL-3.0-or-later

//! Theme and profiles as the protocol describes them (ADR-0105 section 6;
//! spec 03 sections 4.2 and 4.3).
//!
//! Pure: the colors come from the palettes the app already resolved
//! ([`ResolvedTermPalette`], [`ResolvedPalette`]), the profiles from the
//! `Config` and the detected shells. The caller (stage 12) sends both messages
//! after `welcome` and again whenever the theme or the profiles change.

use std::path::Path;

use porecatu_config::{Config, ProfileKind as ConfigKind, ResolvedProfile};
use porecatu_core::GroupColor;
use porecatu_remote::protocol::message::{Profile, ProfileKind, Profiles, Theme};
use porecatu_render::Color;
use porecatu_term::TermColor;

use crate::palette::{ResolvedPalette, ResolvedTermPalette};
use crate::profile_detect::DetectedProfile;
use crate::profile_spawn::{default_profile, display_name};

/// A resolved color as `0xRRGGBB`: each channel rounded, alpha ignored.
pub(crate) fn rgb_u32(color: Color) -> u32 {
    let channel = |c: f64| u32::from((c.clamp(0.0, 1.0) * 255.0).round() as u8);
    (channel(color.r) << 16) | (channel(color.g) << 8) | channel(color.b)
}

/// The `theme` message: the terminal colors the client paints the grid with
/// and the group colors, in the order of [`GroupColor::ALL`] (the `color` of a
/// wire `Group` is [`GroupColor::index`]).
pub(crate) fn protocol_theme(term: &ResolvedTermPalette, chrome: &ResolvedPalette) -> Theme {
    let mut palette = [0u32; 16];
    for (index, slot) in palette.iter_mut().enumerate() {
        // 0..=7 normal, 8..=15 bright; `bold` plays no part in a background.
        *slot = rgb_u32(term.resolve(TermColor::Indexed(index as u8), false, false));
    }
    Theme {
        fg: rgb_u32(term.foreground),
        bg: rgb_u32(term.background),
        cursor: rgb_u32(term.cursor),
        selection_bg: rgb_u32(term.selection_background),
        palette,
        group_colors: GroupColor::ALL
            .iter()
            .map(|color| rgb_u32(chrome.group_color(*color)))
            .collect(),
    }
}

/// The wire kind of a profile. The config knows `Local` and `Ssh`; a local one
/// that launches `wsl` (declared with `program = "wsl.exe"`, or detected with
/// the `auto:wsl:` prefix) is `Wsl`.
fn kind_of(id: &str, kind: ConfigKind, program: Option<&str>) -> ProfileKind {
    if kind == ConfigKind::Ssh {
        return ProfileKind::Ssh;
    }
    let launches_wsl = program
        .and_then(|program| Path::new(program).file_stem())
        .is_some_and(|stem| stem.eq_ignore_ascii_case("wsl"));
    if id.starts_with("auto:wsl:") || launches_wsl {
        ProfileKind::Wsl
    } else {
        ProfileKind::Local
    }
}

fn resolved_kind(resolved: &ResolvedProfile) -> ProfileKind {
    let kind = if resolved.ssh.is_some() {
        ConfigKind::Ssh
    } else {
        ConfigKind::Local
    };
    kind_of(&resolved.id, kind, resolved.program.as_deref())
}

/// The `profiles` message: what the desktop's profile menu offers, in its
/// order (ADR-0070 section 6, minus the "new group" row), with the **id the
/// session records**:
///
/// 1. the default profile (`[profiles] default`, or the `[shell]`);
/// 2. the declared ones, without repeating the default;
/// 3. the `[shell]`, when it is not the default;
/// 4. the detected ones that do not launch what a row above launches.
///
/// Ids: a declared profile is its `id` from `[[profiles.list]]`; a detected
/// one is its deterministic `auto:...` id; the `[shell]` is `""` (the id
/// `chosen_profile` already takes, never written to a session, where
/// `Tab::profile = None` stands for it). `default` is the id of row 1.
pub(crate) fn protocol_profiles(config: &Config, detected: &[DetectedProfile]) -> Profiles {
    let default = default_profile(config).resolved;
    let default_id = default.id.clone();

    let row_for = |resolved: &ResolvedProfile| {
        let declared = config
            .profiles
            .list
            .iter()
            .find(|def| !resolved.id.is_empty() && def.id == resolved.id);
        Profile {
            id: resolved.id.clone(),
            name: declared.map_or_else(|| display_name(resolved), |def| def.name.clone()),
            kind: resolved_kind(resolved),
        }
    };

    // Same de-duplication key as the menu: what the profile launches.
    let launch_of = |resolved: &ResolvedProfile| {
        let program = resolved
            .program
            .clone()
            .unwrap_or_else(|| display_name(resolved));
        crate::profile_detect::launch_key(&program, &resolved.args)
    };

    let mut seen = vec![launch_of(&default)];
    let mut list = vec![row_for(&default)];
    for def in &config.profiles.list {
        if def.id == default.id {
            continue;
        }
        if let Ok(resolved) =
            porecatu_config::resolve_profile(config, porecatu_config::ProfileRef::Id(&def.id))
        {
            seen.push(launch_of(&resolved));
            list.push(row_for(&resolved));
        }
    }
    if !default.id.is_empty()
        && let Ok(shell) =
            porecatu_config::resolve_profile(config, porecatu_config::ProfileRef::Id(""))
    {
        seen.push(launch_of(&shell));
        list.push(row_for(&shell));
    }
    for found in detected {
        let program = found.resolved.program.as_deref().unwrap_or_default();
        let key = crate::profile_detect::launch_key(program, &found.resolved.args);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        list.push(Profile {
            id: found.id.clone(),
            name: found.name.clone(),
            kind: kind_of(
                &found.id,
                ConfigKind::Local,
                found.resolved.program.as_deref(),
            ),
        });
    }

    Profiles {
        list,
        default: default_id,
    }
}

#[cfg(test)]
mod tests {
    use porecatu_remote::protocol::message::{Direction, Message, decode, encode};

    use super::*;

    fn config(text: &str) -> Config {
        porecatu_config::parse(text).expect("valid toml").0
    }

    fn detected(id: &str, name: &str, program: &str, args: &[&str]) -> DetectedProfile {
        DetectedProfile {
            id: id.to_owned(),
            name: name.to_owned(),
            resolved: ResolvedProfile {
                id: id.to_owned(),
                badge: None,
                program: Some(program.to_owned()),
                args: args.iter().map(|a| (*a).to_owned()).collect(),
                env: Vec::new(),
                cwd: None,
                ssh: None,
            },
        }
    }

    #[test]
    fn colors_round_to_the_nearest_channel_and_ignore_alpha() {
        let color = |r, g, b, a| Color { r, g, b, a };
        assert_eq!(rgb_u32(color(0.0, 0.0, 0.0, 1.0)), 0x000000);
        assert_eq!(rgb_u32(color(1.0, 1.0, 1.0, 0.0)), 0xffffff);
        assert_eq!(
            rgb_u32(color(
                0x12 as f64 / 255.0,
                0x34 as f64 / 255.0,
                0x56 as f64 / 255.0,
                0.5
            )),
            0x123456
        );
        // 0.5 * 255 = 127.5 rounds up.
        assert_eq!(rgb_u32(color(0.5, 0.5, 0.5, 1.0)), 0x808080);
        assert_eq!(rgb_u32(color(2.0, -1.0, 1.0, 1.0)), 0xff00ff);
    }

    #[test]
    fn the_default_theme_gives_the_colors_of_thm_default() {
        let config = Config::default();
        let theme = protocol_theme(
            &ResolvedTermPalette::from_config(&config),
            &ResolvedPalette::from_config(&config),
        );
        assert_eq!(theme.fg, 0xc7ccd6);
        assert_eq!(theme.bg, 0x0f1216);
        assert_eq!(theme.cursor, 0x5ed3bc);
        assert_eq!(theme.selection_bg, 0x2e6b62);
        assert_eq!(
            theme.palette,
            [
                // normal: black red green yellow blue magenta cyan white
                0x3b434f, 0xef8a8a, 0x86c56a, 0xe0b060, 0x6fa8f5, 0xa68cf0, 0x5ed3bc, 0xc7ccd6,
                // bright, same order
                0x6f7783, 0xf5a3a3, 0x9bd482, 0xecc37c, 0x8dbcf8, 0xbda6f5, 0x7fdfcc, 0xeaeef3,
            ]
        );
    }

    #[test]
    fn group_colors_follow_the_order_of_the_group_color_index() {
        let config = Config::default();
        let chrome = ResolvedPalette::from_config(&config);
        let theme = protocol_theme(&ResolvedTermPalette::from_config(&config), &chrome);
        assert_eq!(
            theme.group_colors,
            [0xef8a8a, 0xe0b060, 0x5ed3bc, 0x6fa8f5, 0xa68cf0, 0x86c56a]
        );
        for color in GroupColor::ALL {
            assert_eq!(
                theme.group_colors[color.index()],
                rgb_u32(chrome.group_color(color))
            );
        }
    }

    #[test]
    fn the_light_theme_changes_the_message() {
        let base = Config::default();
        let light = porecatu_config::apply_theme(&base, "default_light");
        let dark_theme = protocol_theme(
            &ResolvedTermPalette::from_config(&base),
            &ResolvedPalette::from_config(&base),
        );
        let light_theme = protocol_theme(
            &ResolvedTermPalette::from_config(&light),
            &ResolvedPalette::from_config(&light),
        );
        assert_eq!(dark_theme.bg, 0x0f1216);
        assert_eq!(light_theme.bg, 0xeff1f5);
        assert_eq!(light_theme.fg, 0x4c4f69);
        let message = encode(&Message::Theme(light_theme));
        assert!(decode(&message, Direction::HostToClient).is_ok());
    }

    #[test]
    fn without_profiles_the_default_is_the_shell() {
        let profiles = protocol_profiles(&Config::default(), &[]);
        assert_eq!(profiles.default, "");
        assert_eq!(profiles.list.len(), 1);
        assert_eq!(profiles.list[0].id, "");
        assert_eq!(profiles.list[0].kind, ProfileKind::Local);
        assert!(!profiles.list[0].name.is_empty());
    }

    const DECLARED: &str = r#"
[profiles]
default = "work"

[[profiles.list]]
id = "ubuntu"
name = "Ubuntu"
badge = "WSL"
program = "wsl.exe"
args = ["-d", "Ubuntu"]

[[profiles.list]]
id = "work"
name = "Work shell"
badge = "WK"
program = "zsh"

[[profiles.list]]
id = "prod"
name = "Production"
type = "ssh"
host = "prod.example.com"
user = "me"
auth = ["agent"]
"#;

    #[test]
    fn declared_profiles_come_default_first_with_their_ids_and_kinds() {
        let profiles = protocol_profiles(&config(DECLARED), &[]);
        assert_eq!(profiles.default, "work");
        let rows: Vec<(&str, &str, ProfileKind)> = profiles
            .list
            .iter()
            .map(|p| (p.id.as_str(), p.name.as_str(), p.kind))
            .collect();
        assert_eq!(rows[0], ("work", "Work shell", ProfileKind::Local));
        assert_eq!(rows[1], ("ubuntu", "Ubuntu", ProfileKind::Wsl));
        assert_eq!(rows[2], ("prod", "Production", ProfileKind::Ssh));
        // The `[shell]` closes the declared ones when it is not the default.
        assert_eq!(rows[3].0, "");
        assert_eq!(rows[3].2, ProfileKind::Local);
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn detected_profiles_follow_and_skip_what_a_row_above_launches() {
        let found = [
            // Same program and arguments as the declared `ubuntu`: skipped.
            detected("auto:wsl:Ubuntu", "Ubuntu", "wsl.exe", &["-d", "Ubuntu"]),
            detected("auto:wsl:Debian", "Debian", "wsl.exe", &["-d", "Debian"]),
            detected("auto:fish", "fish", "/usr/bin/fish", &[]),
        ];
        let profiles = protocol_profiles(&config(DECLARED), &found);
        let ids: Vec<&str> = profiles.list.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            ["work", "ubuntu", "prod", "", "auto:wsl:Debian", "auto:fish"]
        );
        assert_eq!(profiles.list[4].kind, ProfileKind::Wsl);
        assert_eq!(profiles.list[5].kind, ProfileKind::Local);
    }

    #[test]
    fn the_messages_encode_and_decode() {
        let config = config(DECLARED);
        let profiles = protocol_profiles(&config, &[]);
        let bytes = encode(&Message::Profiles(profiles.clone()));
        assert_eq!(
            decode(&bytes, Direction::HostToClient).expect("decodes"),
            Message::Profiles(profiles)
        );
        // An empty id (the `[shell]`) survives the wire too.
        let shell_only = protocol_profiles(&Config::default(), &[]);
        let bytes = encode(&Message::Profiles(shell_only.clone()));
        assert_eq!(
            decode(&bytes, Direction::HostToClient).expect("decodes"),
            Message::Profiles(shell_only)
        );
        let theme = protocol_theme(
            &ResolvedTermPalette::from_config(&Config::default()),
            &ResolvedPalette::from_config(&Config::default()),
        );
        let bytes = encode(&Message::Theme(theme.clone()));
        assert_eq!(
            decode(&bytes, Direction::HostToClient).expect("decodes"),
            Message::Theme(theme)
        );
    }
}
