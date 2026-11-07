// SPDX-License-Identifier: GPL-3.0-or-later

//! `App::append_named_window`: restoring a named session **into the window
//! the gesture came from**, at the end of its track, when
//! `[session] restore_named_in_current_window` is on (PRD-023 RF-23.4 to
//! RF-23.15, ADR-0092 §4).
//!
//! The session is turned into a throw-away `Workspace` by the same
//! `workspace_from_window` the startup and the new-window restore use; its
//! groups and loose tabs then leave it through the core's `take_*` and enter
//! the window's workspace through `insert_detached_*` (ADR-0090 §1) -- the
//! one path the core has for inserting tabs that come from elsewhere, so ids,
//! same-name groups, adjacent implicit runs and `last_active` are already
//! settled. Nothing here is taken from the window's own workspace.
//!
//! What is pure (the append, the set of tabs to start) is a free function
//! with a test; what needs a window stays in the `impl App` below.

use std::collections::HashMap;

use porecatu_core::{DetachedGroup, DetachedTab, InsertAt};

use super::*;

/// What [`append_session_workspace`] added to the destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Appended {
    /// New ids at the destination, in track order of the session.
    pub(crate) tabs: Vec<TabId>,
    /// The session's active tab, translated to its new id (RF-23.9).
    pub(crate) active: Option<TabId>,
}

/// One item of the session's track, in visual order.
#[derive(Debug)]
enum Item {
    /// An explicit group, by its id in the temporary workspace.
    Group(GroupId),
    /// A run of loose tabs.
    Loose(Vec<TabId>),
}

impl Item {
    fn holds(&self, source: &Workspace, tab: TabId) -> bool {
        match self {
            Self::Group(id) => source.group(*id).is_some_and(|g| g.tabs().contains(&tab)),
            Self::Loose(ids) => ids.contains(&tab),
        }
    }
}

/// What left the temporary workspace for one [`Item`], with the ids the tabs
/// had there (the order of the ids is the order of the tabs).
enum Extracted {
    Group {
        group: DetachedGroup,
        old: Vec<TabId>,
    },
    Loose {
        old: Vec<TabId>,
        tabs: Vec<DetachedTab>,
    },
}

/// The track of `source` as items, in visual order.
fn plan(source: &Workspace) -> Vec<Item> {
    source
        .groups()
        .iter()
        .filter(|g| !g.is_empty())
        .map(|g| {
            if g.is_explicit() {
                Item::Group(g.id())
            } else {
                Item::Loose(g.tabs().to_vec())
            }
        })
        .collect()
}

/// ADR-0092 §4 steps 2-3: moves every group and loose tab of `source` (a
/// workspace freshly built by `workspace_from_window`) to the end of the
/// track of `dest`, in track order, through the core's `take_*` /
/// `insert_detached_*`. Nothing already in `dest` is taken, moved or
/// renamed; pinned tabs of `dest` stay; nothing is activated in `dest`.
///
/// **The order of extraction matters.** Taking the *active* tab of `source`
/// runs the focus ladder over what is left, and `activate_tab` expands a
/// collapsed group -- so a collapsed group still in `source` would leave
/// with `collapsed = false` (RF-23.6). The items are therefore planned
/// before anything leaves, and the one holding the active tab is extracted
/// **last**: when it goes there is nothing left for the ladder to activate.
/// Insertion then follows the plan, not the order of extraction.
pub(crate) fn append_session_workspace(dest: &mut Workspace, mut source: Workspace) -> Appended {
    let items = plan(&source);
    let session_active = source.active_tab();
    let active_item = session_active.and_then(|a| items.iter().position(|i| i.holds(&source, a)));

    let mut extracted: Vec<Option<Extracted>> = items.iter().map(|_| None).collect();
    let order = (0..items.len())
        .filter(|&i| Some(i) != active_item)
        .chain(active_item);
    for index in order {
        extracted[index] = match &items[index] {
            Item::Group(id) => source
                .take_group(*id)
                .map(|(group, old)| Extracted::Group { group, old }),
            Item::Loose(ids) => {
                let (old, tabs): (Vec<TabId>, Vec<DetachedTab>) =
                    source.take_tabs(ids).into_iter().unzip();
                (!old.is_empty()).then_some(Extracted::Loose { old, tabs })
            }
        };
    }

    let mut new_ids: HashMap<TabId, TabId> = HashMap::new();
    let mut tabs: Vec<TabId> = Vec::new();
    for entry in extracted.into_iter().flatten() {
        let (old, new) = match entry {
            Extracted::Group { group, old } => {
                let at = dest.groups().len();
                let (_, new) = dest.insert_detached_group(group, at);
                (old, new)
            }
            Extracted::Loose { old, tabs } => {
                (old, dest.insert_detached_tabs(tabs, InsertAt::TrackEnd))
            }
        };
        new_ids.extend(old.into_iter().zip(new.iter().copied()));
        tabs.extend(new);
    }

    Appended {
        tabs,
        active: session_active.and_then(|old| new_ids.get(&old).copied()),
    }
}

/// ADR-0092 §4 step 4: eager starts every appended tab; lazy only the
/// translated active one (RF-23.10, RF-3.8). Lazy with no active tab starts
/// nothing: the tabs stay `NotStarted` until they are focused.
pub(crate) fn tabs_to_start(appended: &Appended, lazy_restore: bool) -> Vec<TabId> {
    if lazy_restore {
        appended.active.into_iter().collect()
    } else {
        appended.tabs.clone()
    }
}

impl App {
    /// ADR-0092 §4: appends the groups and tabs of a named session to the
    /// track of `origin`. Geometry, maximization, full screen, theme, zoom
    /// and the pinned tabs are not touched (RF-23.11, RF-23.12): `geometry`,
    /// `monitor` and `active_pinned` of the saved window are never read, and
    /// `open_pinned_tabs_into` and `window_needs_a_minimum_tab` are not
    /// called. The panes are sized by `spawn_tab_runtime` from the size the
    /// window has, so no resize follows.
    pub(crate) fn append_named_window(
        &mut self,
        origin: WindowId,
        window_v1: &porecatu_session::WindowV1,
    ) {
        let lazy_restore = self.config.session.lazy_restore;
        // Built before `state` borrows `self.windows`: the same conversion
        // `open_window_from_session` uses.
        let source = self.workspace_from_window_v1(window_v1);
        let Some(state) = self.windows.get_mut(&origin) else {
            return;
        };

        let appended = append_session_workspace(&mut state.workspace, source);
        if appended.tabs.is_empty() {
            // RF-23.13: a session with nothing to restore adds nothing.
            state.window.request_redraw();
            return;
        }

        state.start_restored_tabs(
            &tabs_to_start(&appended, lazy_restore),
            self.cell_metrics,
            &self.proxy,
            &self.style,
            &self.term_params,
            &self.config,
            &self.startup_directory,
            Instant::now(),
        );

        match appended.active {
            Some(active) => {
                let _ = state.touch_workspace(|ws| ws.activate_tab(active));
            }
            // RF-23.14: the automatic session has to write the new tabs.
            None => state.mark_session_dirty(),
        }
        state.profile_badges_dirty = true;
        state.sync_window_title();
        state.window.request_redraw();

        self.reveal_active_tab(origin);
        self.refresh_access_tree(origin);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use porecatu_core::GroupColor;
    use porecatu_session::{GroupV1, PaneNodeV1, PaneTreeV1, PaneV1, SplitAxisV1, TabV1, WindowV1};

    use super::*;

    fn tab_v1(id: u32, title: &str) -> TabV1 {
        TabV1 {
            id,
            custom_title: Some(title.to_string()),
            cwd: Some(PathBuf::from(format!("/work/{title}"))),
            spawn_program: Some("zsh".to_string()),
            ..TabV1::default()
        }
    }

    fn explicit(id: u32, name: &str, collapsed: bool, tabs: &[u32]) -> GroupV1 {
        GroupV1 {
            id,
            name: Some(name.to_string()),
            color: Some("blue".to_string()),
            collapsed,
            tabs: tabs.to_vec(),
        }
    }

    fn loose(id: u32, tabs: &[u32]) -> GroupV1 {
        GroupV1 {
            id,
            tabs: tabs.to_vec(),
            ..GroupV1::default()
        }
    }

    fn window(groups: Vec<GroupV1>, tabs: Vec<TabV1>, active_tab: Option<u32>) -> WindowV1 {
        WindowV1 {
            groups,
            tabs,
            active_tab,
            ..WindowV1::default()
        }
    }

    fn build(window: &WindowV1, lazy_restore: bool) -> Workspace {
        porecatu_session::convert::workspace_from_window(
            &window.groups,
            &window.tabs,
            window.active_tab,
            lazy_restore,
            &|_| false,
        )
    }

    /// `[infra: i1 i2] [loose: l0]`, with `infra` collapsed and `l0` active.
    fn dest() -> Workspace {
        build(
            &window(
                vec![explicit(0, "infra", true, &[0, 1]), loose(1, &[2])],
                vec![tab_v1(0, "i1"), tab_v1(1, "i2"), tab_v1(2, "l0")],
                Some(2),
            ),
            false,
        )
    }

    /// `[api: a1 a2] [loose: l1] [front: f1 f2 (collapsed)]`, with `a2`
    /// active -- the collapsed group comes **after** the active tab.
    fn session() -> WindowV1 {
        window(
            vec![
                explicit(0, "api", false, &[0, 1]),
                loose(1, &[2]),
                explicit(2, "front", true, &[3, 4]),
            ],
            vec![
                tab_v1(0, "a1"),
                tab_v1(1, "a2"),
                tab_v1(2, "l1"),
                tab_v1(3, "f1"),
                tab_v1(4, "f2"),
            ],
            Some(1),
        )
    }

    /// The track as `(group name or None for a loose run, titles)`.
    fn layout(ws: &Workspace) -> Vec<(Option<String>, Vec<String>)> {
        ws.groups()
            .iter()
            .map(|g| {
                (
                    g.name().map(str::to_owned),
                    g.tabs()
                        .iter()
                        .map(|&id| ws.tab(id).unwrap().title().to_owned())
                        .collect(),
                )
            })
            .collect()
    }

    fn entry(name: Option<&str>, titles: &[&str]) -> (Option<String>, Vec<String>) {
        (
            name.map(str::to_owned),
            titles.iter().map(|t| (*t).to_owned()).collect(),
        )
    }

    /// Everything the model says about a group, ids included.
    #[derive(Debug, PartialEq)]
    struct Shape {
        id: GroupId,
        name: Option<String>,
        color: Option<GroupColor>,
        collapsed: bool,
        tabs: Vec<TabId>,
    }

    fn shapes(ws: &Workspace) -> Vec<Shape> {
        ws.groups()
            .iter()
            .map(|g| Shape {
                id: g.id(),
                name: g.name().map(str::to_owned),
                color: g.color(),
                collapsed: g.is_collapsed(),
                tabs: g.tabs().to_vec(),
            })
            .collect()
    }

    #[test]
    fn the_session_lands_after_everything_in_the_order_it_was_saved() {
        let mut dest = dest();
        let before = shapes(&dest);
        let appended = append_session_workspace(&mut dest, build(&session(), false));

        assert_eq!(
            layout(&dest),
            [
                entry(Some("infra"), &["i1", "i2"]),
                entry(None, &["l0"]),
                entry(Some("api"), &["a1", "a2"]),
                entry(None, &["l1"]),
                entry(Some("front"), &["f1", "f2"]),
            ]
        );
        // The original prefix is identical: same group ids, same tab ids.
        assert_eq!(shapes(&dest)[..before.len()], before[..]);
        // The suffix is in the order of the session, with fresh ids.
        let after = shapes(&dest);
        let suffix_tabs: Vec<TabId> = after[before.len()..]
            .iter()
            .flat_map(|s| s.tabs.iter().copied())
            .collect();
        assert_eq!(suffix_tabs, appended.tabs);
        assert!(
            appended
                .tabs
                .iter()
                .all(|t| before.iter().all(|s| !s.tabs.contains(t)))
        );
        // The track order of the whole window ends with the appended tabs.
        let order: Vec<TabId> = dest.track_order().collect();
        assert_eq!(
            order[order.len() - appended.tabs.len()..],
            appended.tabs[..]
        );
    }

    #[test]
    fn a_group_with_the_same_name_is_not_merged() {
        let mut dest = build(
            &window(
                vec![explicit(0, "api", false, &[0])],
                vec![tab_v1(0, "mine")],
                Some(0),
            ),
            false,
        );
        let original = shapes(&dest);
        let from_session = window(
            vec![explicit(0, "api", false, &[0, 1])],
            vec![tab_v1(0, "theirs1"), tab_v1(1, "theirs2")],
            Some(0),
        );

        append_session_workspace(&mut dest, build(&from_session, false));

        assert_eq!(
            layout(&dest),
            [
                entry(Some("api"), &["mine"]),
                entry(Some("api"), &["theirs1", "theirs2"]),
            ]
        );
        let after = shapes(&dest);
        assert_eq!(after[0], original[0]);
        assert_ne!(after[0].id, after[1].id);
    }

    #[test]
    fn the_active_tab_of_the_session_is_translated_to_its_new_id() {
        let mut dest = dest();
        let appended = append_session_workspace(&mut dest, build(&session(), false));

        let active = appended.active.expect("the session had an active tab");
        assert!(appended.tabs.contains(&active));
        let tab = dest.tab(active).expect("it exists at the destination");
        assert_eq!(tab.title(), "a2");
        assert_eq!(tab.cwd(), Some(&PathBuf::from("/work/a2")));
        // The second tab of the session's first group, not the first.
        assert_eq!(appended.tabs[1], active);
    }

    #[test]
    fn a_collapsed_group_stays_collapsed_whichever_item_holds_the_active_tab() {
        // The trap: taking the active tab runs the focus ladder, which would
        // expand a collapsed group still waiting in the source. Collapsed
        // groups on both sides of the active one, in two shapes.
        let tabs = || {
            vec![
                tab_v1(0, "c1"),
                tab_v1(1, "c2"),
                tab_v1(2, "x"),
                tab_v1(3, "y"),
                tab_v1(4, "d1"),
            ]
        };
        let active_in_loose = window(
            vec![
                explicit(0, "before", true, &[0, 1]),
                loose(1, &[2]),
                explicit(2, "after", true, &[4]),
            ],
            tabs(),
            Some(2),
        );
        let active_in_group = window(
            vec![
                explicit(0, "before", true, &[0, 1]),
                explicit(1, "live", false, &[2, 3]),
                explicit(2, "after", true, &[4]),
            ],
            tabs(),
            Some(3),
        );

        for (session, active_title) in [(active_in_loose, "x"), (active_in_group, "y")] {
            let mut dest = dest();
            let appended = append_session_workspace(&mut dest, build(&session, false));
            let added = &shapes(&dest)[2..];

            let collapsed: Vec<(Option<&str>, bool)> = added
                .iter()
                .map(|s| (s.name.as_deref(), s.collapsed))
                .collect();
            let expected: &[(Option<&str>, bool)] = if active_title == "x" {
                &[(Some("before"), true), (None, false), (Some("after"), true)]
            } else {
                &[
                    (Some("before"), true),
                    (Some("live"), false),
                    (Some("after"), true),
                ]
            };
            assert_eq!(collapsed, expected);
            // Name and colour of every group came through.
            assert!(
                added
                    .iter()
                    .filter_map(|s| s.color)
                    .all(|c| c == GroupColor::Blue)
            );
            let active = appended.active.unwrap();
            assert_eq!(dest.tab(active).unwrap().title(), active_title);
        }
        // The group that was already collapsed at the destination as well.
        let mut dest = dest();
        append_session_workspace(&mut dest, build(&session(), false));
        assert!(dest.groups()[0].is_collapsed());
    }

    #[test]
    fn lazy_restore_leaves_all_but_the_active_tab_not_started() {
        let mut dest = dest();
        let appended = append_session_workspace(&mut dest, build(&session(), true));
        let active = appended.active.unwrap();

        for &id in &appended.tabs {
            assert_eq!(
                dest.tab(id).unwrap().is_not_started(),
                id != active,
                "tab {id:?}"
            );
        }
        assert_eq!(tabs_to_start(&appended, true), vec![active]);
        assert_eq!(tabs_to_start(&appended, false), appended.tabs);
    }

    #[test]
    fn lazy_restore_without_an_active_tab_starts_nothing() {
        let mut dest = dest();
        let mut saved = session();
        saved.active_tab = None;
        let appended = append_session_workspace(&mut dest, build(&saved, true));

        assert_eq!(appended.active, None);
        assert_eq!(appended.tabs.len(), 5);
        assert!(tabs_to_start(&appended, true).is_empty());
        assert!(
            appended
                .tabs
                .iter()
                .all(|&id| dest.tab(id).unwrap().is_not_started())
        );
    }

    #[test]
    fn nothing_that_was_in_the_window_changes() {
        let mut dest = dest();
        let tabs_before: Vec<(TabId, String, String, bool)> = dest
            .track_order()
            .map(|id| {
                let tab = dest.tab(id).unwrap();
                (
                    id,
                    tab.title().to_owned(),
                    tab.shell_name().to_owned(),
                    tab.is_not_started(),
                )
            })
            .collect();
        let active_before = dest.active_tab();
        let panes_before: Vec<_> = dest
            .track_order()
            .map(|id| dest.tab(id).unwrap().panes().clone())
            .collect();

        append_session_workspace(&mut dest, build(&session(), false));

        // The function activates nothing: that is the caller's step.
        assert_eq!(dest.active_tab(), active_before);
        for (id, title, shell, not_started) in tabs_before {
            let tab = dest.tab(id).expect("the tab is still there");
            assert_eq!(tab.title(), title);
            assert_eq!(tab.shell_name(), shell);
            assert_eq!(tab.is_not_started(), not_started);
        }
        for (id, panes) in dest.track_order().zip(panes_before) {
            assert_eq!(dest.tab(id).unwrap().panes(), &panes);
        }
        assert_eq!(dest.groups()[0].name(), Some("infra"));
        assert_eq!(dest.groups()[0].color(), Some(GroupColor::Blue));
        assert!(dest.groups()[0].is_collapsed());
    }

    #[test]
    fn the_pinned_tabs_stay_as_they_are_and_nothing_appended_is_pinned() {
        let mut dest = dest();
        let first = dest.track_order().next().unwrap();
        let second = dest.track_order().nth(1).unwrap();
        assert!(dest.pin_tab(first, 0));
        assert!(dest.pin_tab(second, 1));
        let pinned_before = dest.pinned().to_vec();
        assert_eq!(pinned_before.len(), 2);

        let appended = append_session_workspace(&mut dest, build(&session(), false));

        assert_eq!(dest.pinned(), pinned_before);
        assert!(appended.tabs.iter().all(|&id| !dest.is_pinned(id)));
        let order: Vec<TabId> = dest.visual_order().collect();
        assert_eq!(order[..2], pinned_before[..]);
    }

    #[test]
    fn a_session_with_no_tab_changes_nothing() {
        let mut dest = dest();
        let before = shapes(&dest);
        let active_before = dest.active_tab();

        let appended = append_session_workspace(&mut dest, Workspace::new());
        assert_eq!(
            appended,
            Appended {
                tabs: vec![],
                active: None
            }
        );
        // Groups that point at tabs the file does not have restore to nothing.
        let hollow = window(vec![explicit(0, "ghost", false, &[7])], vec![], Some(7));
        let appended = append_session_workspace(&mut dest, build(&hollow, false));
        assert!(appended.tabs.is_empty());
        assert_eq!(appended.active, None);

        assert_eq!(shapes(&dest), before);
        assert_eq!(dest.active_tab(), active_before);
        assert!(tabs_to_start(&appended, true).is_empty());
        assert!(tabs_to_start(&appended, false).is_empty());
    }

    #[test]
    fn restoring_twice_appends_twice() {
        let mut dest = dest();
        let first = append_session_workspace(&mut dest, build(&session(), false));
        let second = append_session_workspace(&mut dest, build(&session(), false));

        assert_eq!(
            layout(&dest),
            [
                entry(Some("infra"), &["i1", "i2"]),
                entry(None, &["l0"]),
                entry(Some("api"), &["a1", "a2"]),
                entry(None, &["l1"]),
                entry(Some("front"), &["f1", "f2"]),
                entry(Some("api"), &["a1", "a2"]),
                entry(None, &["l1"]),
                entry(Some("front"), &["f1", "f2"]),
            ]
        );
        let mut all: Vec<TabId> = dest.track_order().collect();
        assert_eq!(all.len(), 3 + 5 + 5);
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 13, "every id is distinct");
        assert_ne!(first.active, second.active);
        assert!(first.tabs.iter().all(|t| !second.tabs.contains(t)));
    }

    /// A tab with a split tree: ratio, two panes, a saved profile.
    fn split_tab(id: u32, title: &str) -> TabV1 {
        let leaf = |pane: u32, cwd: &str, profile: Option<&str>| {
            PaneNodeV1::Leaf(PaneV1 {
                id: pane,
                cwd: Some(PathBuf::from(cwd)),
                spawn_program: Some("zsh".to_string()),
                profile: profile.map(str::to_owned),
            })
        };
        TabV1 {
            id,
            custom_title: Some(title.to_string()),
            panes: Some(PaneTreeV1 {
                root: PaneNodeV1::Split {
                    axis: SplitAxisV1::Vertical,
                    ratio: 0.3,
                    first: Box::new(leaf(0, "/work/left", None)),
                    second: Box::new(leaf(1, "/work/right", Some("auto:wsl"))),
                },
                focused: 1,
            }),
            ..TabV1::default()
        }
    }

    #[test]
    fn the_appended_tabs_are_the_ones_a_new_window_would_have_had() {
        let saved = window(
            vec![
                explicit(0, "api", false, &[0, 1]),
                loose(1, &[2]),
                explicit(2, "front", true, &[3]),
            ],
            vec![
                tab_v1(0, "plain"),
                split_tab(1, "split"),
                tab_v1(2, "loose"),
                split_tab(3, "folded"),
            ],
            Some(1),
        );

        for lazy_restore in [false, true] {
            // What the new-window restore builds, tab by tab.
            let fresh = build(&saved, lazy_restore);
            let mut dest = dest();
            let appended = append_session_workspace(&mut dest, build(&saved, lazy_restore));

            let fresh_order: Vec<TabId> = fresh.track_order().collect();
            assert_eq!(fresh_order.len(), appended.tabs.len());
            for (&old, &new) in fresh_order.iter().zip(&appended.tabs) {
                let a = fresh.tab(old).unwrap();
                let b = dest.tab(new).unwrap();
                assert_eq!(a.is_not_started(), b.is_not_started(), "state");
                assert_eq!(a.cwd(), b.cwd(), "cwd");
                assert_eq!(a.title(), b.title(), "title");
                assert_eq!(
                    a.panes().focused().profile(),
                    b.panes().focused().profile(),
                    "profile of the focused pane"
                );
                // Shape, ratios, every pane's state, cwd and profile.
                assert_eq!(a.panes(), b.panes(), "pane tree");
            }
            // The active tab of the session is the one that was active there.
            assert_eq!(
                appended.active.map(|id| dest.tab(id).unwrap().title()),
                fresh.active_tab().map(|id| fresh.tab(id).unwrap().title()),
            );
        }
    }
}
