// SPDX-License-Identifier: GPL-3.0-or-later

//! The profile menu (ADR-0070 §6, PRD-007 RF-7.8 to RF-7.12): the context menu
//! anchored on a `+` button, with a badge column and the key of each row.
//!
//! Everything here is pure: the rows come from the config, the catalog and the
//! resolved key map, the anchor from the tab-bar layout. The menu keeps a
//! **snapshot** of its rows taken when it opens, so a reload while it is open
//! never shifts what the highlighted index means; painting, hit testing and
//! the accessibility tree read the same rows.

use std::collections::HashMap;

use porecatu_config::{Config, ProfileRef, ResolvedProfile, resolve_profile};
use porecatu_core::{Action, GroupId, TabId};
use porecatu_locale::Catalog;
use porecatu_render::Rect;

use crate::keymap::{Chord, chord_for_action};
use crate::messages::msg;
use crate::profile_spawn::{default_profile, display_name};
use crate::tab_bar::{self, TabBarHit, TabBarLayout};

/// Which `+` the menu belongs to, and so which group the new tab joins and
/// whose color the badges take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProfileMenuTarget {
    /// The `+` at the end of a group (explicit or an implicit run).
    Group(GroupId),
    /// The `+` at the end of the track: a tab outside every group.
    Ungrouped,
}

/// What a row does when chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProfileRowKind {
    /// Opens a tab with this profile id (`""` is the `[shell]`).
    Profile { id: String },
    /// Runs `group.create`.
    NewGroup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileRow {
    pub(crate) kind: ProfileRowKind,
    pub(crate) label: String,
    /// `None` for the `[shell]` row and for "new group".
    pub(crate) badge: Option<String>,
    /// The bound key, already rendered (`Ctrl+Shift+2`), when there is one.
    pub(crate) chip: Option<String>,
}

/// The rows, in the order of ADR-0070 §6:
///
/// 1. the default profile;
/// 2. the declared ones, without repeating the default;
/// 3. the `[shell]`, when it is not the default, named after the shell it
///    resolves to and without a badge;
/// 4. (prompt 05 adds the detected section here, see [`detected_rows`]);
/// 5. "new tab group", always last. A divider is painted before it.
///
/// The key of a declared row is `profile.new_tab_N`, `N` being its position in
/// `[[profiles.list]]` whether or not it is also the default row; detected
/// profiles never get one (RF-7.11).
pub(crate) fn profile_menu_rows(
    config: &Config,
    catalog: &Catalog,
    bindings: &HashMap<Chord, Action>,
) -> Vec<ProfileRow> {
    let chip_of = |action: Action| chord_for_action(bindings, action).map(|chord| chord.label());
    let declared_index = |id: &str| {
        config
            .profiles
            .list
            .iter()
            .position(|def| def.id == id)
            .and_then(|i| u8::try_from(i + 1).ok())
            .filter(|n| *n <= 9)
    };
    let row_for = |resolved: &ResolvedProfile| {
        let declared = config
            .profiles
            .list
            .iter()
            .find(|def| !resolved.id.is_empty() && def.id == resolved.id);
        ProfileRow {
            kind: ProfileRowKind::Profile {
                id: resolved.id.clone(),
            },
            label: declared.map_or_else(|| display_name(resolved), |def| def.name.clone()),
            badge: resolved.badge.clone(),
            chip: declared_index(&resolved.id).and_then(|n| chip_of(Action::ProfileNewTab(n))),
        }
    };

    let default = default_profile(config).resolved;
    let mut rows = vec![row_for(&default)];
    for def in &config.profiles.list {
        if def.id == default.id {
            continue;
        }
        if let Ok(resolved) = resolve_profile(config, ProfileRef::Id(&def.id)) {
            rows.push(row_for(&resolved));
        }
    }
    if !default.id.is_empty()
        && let Ok(shell) = resolve_profile(config, ProfileRef::Id(""))
    {
        rows.push(row_for(&shell));
    }
    rows.extend(detected_rows(config));
    rows.push(ProfileRow {
        kind: ProfileRowKind::NewGroup,
        label: msg::profile_menu::new_group(catalog),
        badge: None,
        chip: chip_of(Action::GroupCreate),
    });
    rows
}

/// Insertion point of the detected-profiles section (ADR-0070 §5, prompt 05):
/// between the `[shell]` row and the divider before "new tab group".
fn detected_rows(_config: &Config) -> Vec<ProfileRow> {
    Vec::new()
}

/// The id `profile.new_tab_N` opens: the N-th **declared** profile, 1-based.
/// Detected profiles never count, so a shortcut does not change target when
/// the machine gains a shell. `None` when there is no N-th profile.
pub(crate) fn nth_declared_id(config: &Config, n: u8) -> Option<&str> {
    let index = usize::from(n).checked_sub(1)?;
    config.profiles.list.get(index).map(|def| def.id.as_str())
}

/// A secondary click on a `+` asks for this menu; anything else does not.
pub(crate) fn target_for_hit(hit: TabBarHit) -> Option<ProfileMenuTarget> {
    match hit {
        TabBarHit::GroupNewTab(id) => Some(ProfileMenuTarget::Group(id)),
        TabBarHit::UngroupedNewTab => Some(ProfileMenuTarget::Ungrouped),
        TabBarHit::Tab(_) | TabBarHit::CloseButton(_) | TabBarHit::Pill(_) => None,
    }
}

/// Where `profile.open_menu` anchors, in track coordinates (the caller
/// subtracts the scroll offset): the `+` of the group the active tab is in,
/// or, when that button is hidden (`show_new_tab_button = false`), the active
/// tab itself, so the menu opens below it.
pub(crate) fn open_menu_anchor(
    layout: &TabBarLayout,
    group: GroupId,
    active: TabId,
) -> Option<Rect> {
    layout
        .groups
        .iter()
        .find(|g| g.id == group)
        .and_then(|g| g.new_tab_button)
        .or_else(|| tab_bar::tab_rect(layout, active))
}

/// Track-coordinate rectangle of the `+` a hit refers to.
pub(crate) fn plus_rect(layout: &TabBarLayout, target: ProfileMenuTarget) -> Option<Rect> {
    match target {
        ProfileMenuTarget::Group(id) => layout
            .groups
            .iter()
            .find(|g| g.id == id)
            .and_then(|g| g.new_tab_button),
        ProfileMenuTarget::Ungrouped => layout.ungrouped_new_tab_button,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProfileMenu {
    pub(crate) target: ProfileMenuTarget,
    /// The `+` (or the active tab) the menu hangs from, in window
    /// coordinates.
    pub(crate) anchor: Rect,
    /// Measured when the menu opens (`overlay::profile_menu_width`).
    pub(crate) width: f32,
    rows: Vec<ProfileRow>,
    highlighted: usize,
}

impl ProfileMenu {
    pub(crate) fn new(
        target: ProfileMenuTarget,
        anchor: Rect,
        width: f32,
        rows: Vec<ProfileRow>,
    ) -> Self {
        Self {
            target,
            anchor,
            width,
            rows,
            highlighted: 0,
        }
    }

    pub(crate) fn rows(&self) -> &[ProfileRow] {
        &self.rows
    }

    pub(crate) const fn highlighted(&self) -> usize {
        self.highlighted
    }

    pub(crate) fn move_highlight(&mut self, delta: isize) {
        let len = self.rows.len() as isize;
        if len > 0 {
            self.highlighted = (self.highlighted as isize + delta).rem_euclid(len) as usize;
        }
    }

    pub(crate) fn set_highlight(&mut self, index: usize) {
        if index < self.rows.len() {
            self.highlighted = index;
        }
    }

    pub(crate) fn selected(&self) -> Option<&ProfileRow> {
        self.rows.get(self.highlighted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap;
    use crate::messages::test_support;
    use porecatu_core::{GroupColor, Workspace};

    fn config(text: &str) -> Config {
        porecatu_config::parse(text).expect("valid toml").0
    }

    fn profile(id: &str, name: &str, badge: &str) -> String {
        format!(
            "[[profiles.list]]\nid = \"{id}\"\nname = \"{name}\"\nbadge = \"{badge}\"\nprogram = \"{id}.exe\"\n"
        )
    }

    fn rows(text: &str) -> Vec<ProfileRow> {
        profile_menu_rows(&config(text), &test_support::pt_br(), &HashMap::new())
    }

    fn ids(rows: &[ProfileRow]) -> Vec<Option<&str>> {
        rows.iter()
            .map(|r| match &r.kind {
                ProfileRowKind::Profile { id } => Some(id.as_str()),
                ProfileRowKind::NewGroup => None,
            })
            .collect()
    }

    #[test]
    fn without_profiles_the_menu_is_the_shell_and_new_group() {
        let rows = rows("");
        assert_eq!(ids(&rows), [Some(""), None]);
        assert_eq!(rows[0].badge, None);
        assert_eq!(rows[1].label, "Novo grupo de abas");
    }

    #[test]
    fn the_default_comes_first_and_is_not_repeated() {
        let text = format!(
            "[profiles]\ndefault = \"b\"\n{}{}{}",
            profile("a", "A", "A"),
            profile("b", "B", "B"),
            profile("c", "C", "C")
        );
        let rows = rows(&text);
        // default, the other declared ones in declaration order, the
        // `[shell]` (not the default), "new group".
        assert_eq!(
            ids(&rows),
            [Some("b"), Some("a"), Some("c"), Some(""), None]
        );
        assert_eq!(rows[0].label, "B");
        assert_eq!(rows[0].badge.as_deref(), Some("B"));
    }

    #[test]
    fn the_shell_row_is_present_only_when_it_is_not_the_default() {
        let declared = profile("a", "A", "A");
        let shell_default = rows(&format!("[profiles]\ndefault = \"\"\n{declared}"));
        assert_eq!(ids(&shell_default), [Some(""), Some("a"), None]);

        let a_default = rows(&format!("[profiles]\ndefault = \"a\"\n{declared}"));
        assert_eq!(ids(&a_default), [Some("a"), Some(""), None]);
        let shell = &a_default[1];
        assert_eq!(shell.badge, None, "the [shell] has no badge");
    }

    #[test]
    fn an_unresolvable_default_collapses_into_the_shell() {
        let rows = rows("[profiles]\ndefault = \"gone\"\n");
        assert_eq!(ids(&rows), [Some(""), None]);
    }

    #[test]
    fn new_group_is_always_the_last_row() {
        let text = format!("{}{}", profile("a", "A", "A"), profile("b", "B", "B"));
        let rows = rows(&text);
        assert_eq!(rows.last().unwrap().kind, ProfileRowKind::NewGroup);
        assert_eq!(
            rows.iter()
                .filter(|r| r.kind == ProfileRowKind::NewGroup)
                .count(),
            1
        );
    }

    #[test]
    fn declared_rows_carry_the_key_of_their_position_in_the_list() {
        let text = format!(
            "[profiles]\ndefault = \"b\"\n{}{}",
            profile("a", "A", "A"),
            profile("b", "B", "B")
        );
        let resolved = keymap::resolve(
            &porecatu_config::parse(&format!(
                "{text}[keybindings]\n\"ctrl+shift+2\" = \"profile.new_tab_2\"\n\"ctrl+shift+g\" = \"group.create\"\n"
            ))
            .unwrap()
            .0
            .keybindings,
            keymap::Platform::Linux,
        );
        let rows = profile_menu_rows(&config(&text), &test_support::pt_br(), &resolved.bindings);
        // `b` is the default row but is still the second declared profile.
        assert_eq!(rows[0].chip.as_deref(), Some("Ctrl+Shift+2"));
        assert_eq!(rows[1].chip, None, "profile.new_tab_1 is not bound");
        assert_eq!(rows.last().unwrap().chip.as_deref(), Some("Ctrl+Shift+G"));
    }

    #[test]
    fn nth_declared_ignores_the_default_and_counts_from_one() {
        let text = format!(
            "[profiles]\ndefault = \"b\"\n{}{}",
            profile("a", "A", "A"),
            profile("b", "B", "B")
        );
        let config = config(&text);
        assert_eq!(nth_declared_id(&config, 1), Some("a"));
        assert_eq!(nth_declared_id(&config, 2), Some("b"));
        assert_eq!(nth_declared_id(&config, 3), None);
        assert_eq!(nth_declared_id(&config, 0), None);
    }

    #[test]
    fn highlight_wraps_and_ignores_out_of_range() {
        let rows = rows("");
        let mut menu = ProfileMenu::new(
            ProfileMenuTarget::Ungrouped,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            200.0,
            rows,
        );
        assert_eq!(menu.highlighted(), 0);
        menu.move_highlight(-1);
        assert_eq!(menu.selected().unwrap().kind, ProfileRowKind::NewGroup);
        menu.move_highlight(1);
        assert_eq!(menu.highlighted(), 0);
        menu.set_highlight(99);
        assert_eq!(menu.highlighted(), 0);
    }

    fn measurer() -> porecatu_render::TextMeasurer {
        porecatu_render::TextMeasurer::new()
    }

    /// The secondary click on a `+` asks for the menu of **that** group; a
    /// primary click keeps resolving to the `NewTabRequest` it always did
    /// (`TabBarHit` is the same value, only `handle_bar_click` branches on
    /// the button).
    #[test]
    fn a_plus_hit_names_the_group_that_owns_it() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let ga = ws.group_tabs(&[a], "api", GroupColor::Red).unwrap();
        let gb = ws.group_tabs(&[b], "web", GroupColor::Blue).unwrap();
        let mut m = measurer();
        let layout = tab_bar::layout(&ws, &tab_bar::TabBarStyle::DEFAULT, &mut m);

        for group in [ga, gb] {
            let button = plus_rect(&layout, ProfileMenuTarget::Group(group)).unwrap();
            let center = (
                button.x + button.width / 2.0,
                button.y + button.height / 2.0,
            );
            let hit = tab_bar::hit_test(&layout, center).unwrap();
            assert_eq!(hit, TabBarHit::GroupNewTab(group));
            assert_eq!(target_for_hit(hit), Some(ProfileMenuTarget::Group(group)));
        }
        // The trailing `+` (the last group is explicit) is the ungrouped one.
        let button = plus_rect(&layout, ProfileMenuTarget::Ungrouped).unwrap();
        let center = (
            button.x + button.width / 2.0,
            button.y + button.height / 2.0,
        );
        let hit = tab_bar::hit_test(&layout, center).unwrap();
        assert_eq!(hit, TabBarHit::UngroupedNewTab);
        assert_eq!(target_for_hit(hit), Some(ProfileMenuTarget::Ungrouped));
    }

    #[test]
    fn only_the_plus_buttons_open_the_profile_menu() {
        assert_eq!(target_for_hit(TabBarHit::Tab(TabId::new(0))), None);
        assert_eq!(target_for_hit(TabBarHit::CloseButton(TabId::new(0))), None);
        assert_eq!(target_for_hit(TabBarHit::Pill(GroupId::new(0))), None);
    }

    #[test]
    fn open_menu_hangs_from_the_plus_or_from_the_tab_without_one() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_of_tab(a).unwrap();
        let mut m = measurer();

        let with_button = tab_bar::layout(&ws, &tab_bar::TabBarStyle::DEFAULT, &mut m);
        assert_eq!(
            open_menu_anchor(&with_button, group, a),
            with_button.groups[0].new_tab_button
        );

        let style = tab_bar::TabBarStyle {
            show_new_tab_button: false,
            ..tab_bar::TabBarStyle::DEFAULT
        };
        let without = tab_bar::layout(&ws, &style, &mut m);
        assert_eq!(without.groups[0].new_tab_button, None);
        assert_eq!(
            open_menu_anchor(&without, group, a),
            tab_bar::tab_rect(&without, a)
        );
    }
}
