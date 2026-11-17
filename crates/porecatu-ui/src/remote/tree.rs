// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds the `workspace` message of the protocol from the windows of the
//! process (ADR-0105 section 2, PRD-025 RF-25.17; spec 03 section 4.1).
//!
//! Pure: the input is the core `Workspace` of each window plus two closures
//! that answer what only the runtime knows (the facts of a tab and of a pane),
//! so the whole thing is tested without a window. The protocol ids come from
//! [`ProtocolIds`]; every object that shows up in the tree is recorded in the
//! returned [`SeenIds`], which the caller hands to `ProtocolIds::retain_seen`.
//!
//! The encoder of the protocol does not validate the domain, so this module
//! does: integers within `0 ..= 2^53 - 1`, a split `ratio` in `1..=65535`,
//! no empty group.

use std::hash::Hash;

use porecatu_core::{GroupKind, PaneId, PaneNode as CoreNode, SplitAxis, TabId, Workspace as Core};
use porecatu_remote::protocol::cbor::MAX_SAFE_INT;
use porecatu_remote::protocol::message::{
    Group, Item, Pane, PaneNode, PaneState, Split, SplitDir, Tab, Window, Workspace,
};
use porecatu_term::EndReason;

use super::ids::{ProtocolIds, SeenIds};

/// What the runtime knows about one pane and the model does not: its state
/// (see [`pane_state`]), size, directory (`None` on an SSH pane and when
/// unknown) and displayed title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneFacts {
    pub state: PaneState,
    /// Exit code, for `PaneState::Exited`.
    pub exit: Option<i64>,
    pub cols: u64,
    pub rows: u64,
    pub cwd: Option<String>,
    pub title: String,
}

/// What the runtime knows about one tab: the profile id the session records
/// (`None` is the `[shell]`) and `user@host` on an SSH tab.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TabFacts {
    pub profile: Option<String>,
    pub remote: Option<String>,
}

/// One window of the process, as `build_workspace` reads it.
pub(crate) struct WindowInput<'a, W> {
    pub id: W,
    /// The title of the window's active tab.
    pub title: String,
    pub workspace: &'a Core,
    pub tab_facts: &'a dyn Fn(TabId) -> TabFacts,
    pub pane_facts: &'a dyn Fn(TabId, PaneId) -> PaneFacts,
}

/// The wire form of a split `ratio`: `round(ratio * 65536)` limited to
/// `1..=65535` (spec 03: the first child gets `ratio / 65536`). A `NaN` (it
/// cannot come out of the core, whose ratio is in `(0, 1)`) is the middle.
fn wire_ratio(ratio: f32) -> u16 {
    if ratio.is_nan() {
        return 32768;
    }
    let scaled = (f64::from(ratio) * 65536.0).round();
    // The clamp makes the cast exact.
    scaled.clamp(1.0, 65535.0) as u16
}

/// The direction of a split on the wire. In the core, `Horizontal` is the
/// divider lying down, the panes **stacked** (`pane.split_horizontal`: the
/// new pane is born below); `Vertical` is the divider standing, the panes
/// **side by side** (`pane.split_vertical`: the new pane is born at the right).
fn wire_dir(axis: SplitAxis) -> SplitDir {
    match axis {
        SplitAxis::Horizontal => SplitDir::Stacked,
        SplitAxis::Vertical => SplitDir::SideBySide,
    }
}

/// `value` as a wire integer: the encoder does not check the 2^53 - 1 limit.
fn safe(value: u64) -> u64 {
    value.min(MAX_SAFE_INT as u64)
}

fn build_node<W: Copy + Eq + Hash>(
    node: &CoreNode,
    window: &WindowInput<'_, W>,
    tab: TabId,
    ids: &mut ProtocolIds<W>,
    seen: &mut SeenIds<W>,
) -> PaneNode {
    match node {
        CoreNode::Leaf(pane) => {
            let facts = (window.pane_facts)(tab, *pane);
            seen.panes.insert((window.id, tab, *pane));
            PaneNode::Pane(Pane {
                id: ids.pane(window.id, tab, *pane),
                title: facts.title,
                cwd: facts.cwd,
                state: facts.state,
                exit: facts.exit,
                cols: safe(facts.cols),
                rows: safe(facts.rows),
            })
        }
        CoreNode::Split {
            axis,
            ratio,
            first,
            second,
        } => PaneNode::Split(Box::new(Split {
            dir: wire_dir(*axis),
            ratio: wire_ratio(*ratio),
            first: build_node(first, window, tab, ids, seen),
            second: build_node(second, window, tab, ids, seen),
        })),
    }
}

/// `None` when the tab is not in the workspace (it cannot happen with a
/// consistent model; the tree just leaves it out).
fn build_tab<W: Copy + Eq + Hash>(
    id: TabId,
    window: &WindowInput<'_, W>,
    ids: &mut ProtocolIds<W>,
    seen: &mut SeenIds<W>,
) -> Option<Tab> {
    let tab = window.workspace.tab(id)?;
    let facts = (window.tab_facts)(id);
    seen.tabs.insert((window.id, id));
    let layout = build_node(tab.panes().root(), window, id, ids, seen);
    let focused = ids.pane(window.id, id, tab.panes().focused().id());
    Some(Tab {
        id: ids.tab(window.id, id),
        title: tab.title().to_owned(),
        custom: tab.has_custom_title(),
        profile: facts.profile,
        remote: facts.remote,
        activity: tab.activity(),
        bell: tab.bell(),
        layout,
        focused,
    })
}

fn build_window<W: Copy + Eq + Hash>(
    window: &WindowInput<'_, W>,
    ids: &mut ProtocolIds<W>,
    seen: &mut SeenIds<W>,
) -> Window {
    let workspace = window.workspace;
    seen.windows.insert(window.id);
    let id = ids.window(window.id);

    let pinned: Vec<Tab> = workspace
        .pinned()
        .iter()
        .filter_map(|tab| build_tab(*tab, window, ids, seen))
        .collect();

    let mut items = Vec::new();
    for group in workspace.groups() {
        let tabs: Vec<Tab> = group
            .tabs()
            .iter()
            .filter_map(|tab| build_tab(*tab, window, ids, seen))
            .collect();
        if tabs.is_empty() {
            continue;
        }
        match group.kind() {
            // An implicit group is a run of loose tabs.
            GroupKind::Implicit => items.extend(tabs.into_iter().map(Item::Tab)),
            GroupKind::Explicit(meta) => {
                seen.groups.insert((window.id, group.id()));
                items.push(Item::Group(Group {
                    id: ids.group(window.id, group.id()),
                    name: meta.name().to_owned(),
                    color: meta.color().index() as u64,
                    collapsed: meta.is_collapsed(),
                    tabs,
                }));
            }
        }
    }

    // The active tab, only if it is in the tree (pinned or on the track).
    let active = workspace
        .active_tab()
        .filter(|tab| seen.tabs.contains(&(window.id, *tab)))
        .map(|tab| ids.tab(window.id, tab));

    Window {
        id,
        title: window.title.clone(),
        pinned,
        items,
        active,
    }
}

/// The whole tree of the process. `windows` is in the order of focus, most
/// recent first, and the output keeps it. `rev` is 0: the host rewrites it
/// on each send (spec 03 section 4.1).
pub(crate) fn build_workspace<W: Copy + Eq + Hash>(
    windows: &[WindowInput<'_, W>],
    ids: &mut ProtocolIds<W>,
) -> (Workspace, SeenIds<W>) {
    let mut seen = SeenIds::default();
    let windows = windows
        .iter()
        .map(|window| build_window(window, ids, &mut seen))
        .collect();
    (Workspace { rev: 0, windows }, seen)
}

/// What the runtime says about one pane, to be turned into the protocol state
/// by [`pane_state`]. Every field is a signal that exists today; the ones the
/// runtime does not keep yet are marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaneSignals {
    /// `Pane::state` of the model.
    pub model: porecatu_core::PaneState,
    /// The pane has a `PaneRuntime` (a `NotStarted` tab has none).
    pub has_runtime: bool,
    /// `PaneRuntime::dormant`: a restored SSH pane waiting for the first
    /// gesture (RF-20.33).
    pub dormant: bool,
    /// `PaneRuntime::ssh_spawn.is_some()`.
    pub ssh: bool,
    /// `PaneRuntime::reconnecting`: the pane is in a reconnection cycle
    /// (ADR-0076 section 4).
    pub reconnecting: bool,
    /// The SSH shell opened (`TermEvent::Connected`). **Not kept by the
    /// runtime today**: stage 12 adds the flag, set on `Connected` and cleared
    /// when the connection is started again. Ignored on a local pane.
    pub connected: bool,
    /// The SSH backend ended because the connection was lost or never opened
    /// ([`link_dropped`]), not because the remote shell exited. **Not kept by
    /// the runtime today**: stage 12 records it where it marks the pane
    /// `Exited` (`TermEvent::Exit`, and `give_up` of the reconnection).
    pub link_dropped: bool,
    /// A backend question (password, passphrase, unknown host key) waits for
    /// an answer: a `PendingPrompts` entry of the pane or the open dialog of
    /// the prompt (ADR-0107 section 6).
    pub prompt_pending: bool,
}

/// Did the SSH backend end for a reason of the **link** (`Lost`, `Failed`)
/// and not because the remote shell ended (`Exited`, `RemoteEnded`)?
pub(crate) fn link_dropped(reason: &EndReason) -> bool {
    matches!(reason, EndReason::Lost { .. } | EndReason::Failed { .. })
}

/// The protocol state of a pane and its exit code (`Some` only for
/// `Exited`), by this order of precedence:
///
/// 1. model `Exited`: `Disconnected` on an SSH pane whose link dropped,
///    `Exited` with the code otherwise;
/// 2. no runtime, dormant or model `NotStarted`: `NotStarted`;
/// 3. a backend question pending: `AwaitingDesktop` (ahead of the next two,
///    because it is what the user has to do);
/// 4. in a reconnection cycle: `Reconnecting`;
/// 5. SSH that did not open the shell yet: `Connecting`;
/// 6. `Running`.
pub(crate) fn pane_state(signals: &PaneSignals) -> (PaneState, Option<i64>) {
    use porecatu_core::PaneState as Model;
    if let Model::Exited { exit_code } = signals.model {
        return if signals.ssh && signals.link_dropped {
            (PaneState::Disconnected, None)
        } else {
            (PaneState::Exited, Some(i64::from(exit_code)))
        };
    }
    if !signals.has_runtime || signals.dormant || signals.model == Model::NotStarted {
        return (PaneState::NotStarted, None);
    }
    if signals.prompt_pending {
        return (PaneState::AwaitingDesktop, None);
    }
    if signals.reconnecting {
        return (PaneState::Reconnecting, None);
    }
    if signals.ssh && !signals.connected {
        return (PaneState::Connecting, None);
    }
    (PaneState::Running, None)
}

#[cfg(test)]
mod tests {
    use porecatu_core::{GroupColor, PaneState as Model, Workspace as CoreWorkspace};
    use porecatu_remote::protocol::message::{Direction, Message, decode, encode};

    use super::*;

    type Ids = ProtocolIds<u8>;

    fn input<'a>(
        id: u8,
        workspace: &'a CoreWorkspace,
        tab_facts: &'a dyn Fn(TabId) -> TabFacts,
        pane_facts: &'a dyn Fn(TabId, PaneId) -> PaneFacts,
    ) -> WindowInput<'a, u8> {
        WindowInput {
            id,
            title: format!("window {id}"),
            workspace,
            tab_facts,
            pane_facts,
        }
    }

    fn no_tab_facts(_: TabId) -> TabFacts {
        TabFacts::default()
    }

    fn plain_pane(_: TabId, pane: PaneId) -> PaneFacts {
        PaneFacts {
            state: PaneState::Running,
            exit: None,
            cols: 80,
            rows: 24,
            cwd: None,
            title: format!("p{}", pane.get()),
        }
    }

    /// Builds one window with the default facts.
    fn build_one(ws: &CoreWorkspace, ids: &mut Ids) -> Workspace {
        let tabs: &dyn Fn(TabId) -> TabFacts = &no_tab_facts;
        let panes: &dyn Fn(TabId, PaneId) -> PaneFacts = &plain_pane;
        build_workspace(&[input(1, ws, tabs, panes)], ids).0
    }

    fn tab_of(item: &Item) -> &Tab {
        match item {
            Item::Tab(tab) => tab,
            Item::Group(_) => panic!("a group"),
        }
    }

    fn flatten(items: &[Item]) -> Vec<u64> {
        let mut out = Vec::new();
        for item in items {
            match item {
                Item::Tab(tab) => out.push(tab.id),
                Item::Group(group) => out.extend(group.tabs.iter().map(|t| t.id)),
            }
        }
        out
    }

    /// Six tabs; two pinned, two in a collapsed explicit group, two loose.
    fn world() -> CoreWorkspace {
        let mut ws = CoreWorkspace::new();
        let tabs: Vec<TabId> = (0..6).map(|i| ws.new_tab(None, "sh", None, i)).collect();
        let group = ws
            .group_tabs(&[tabs[2], tabs[3]], "work", GroupColor::Blue)
            .expect("group");
        assert!(ws.collapse_group(group, true));
        assert!(ws.pin_tab(tabs[0], 0));
        assert!(ws.pin_tab(tabs[1], 1));
        ws
    }

    #[test]
    fn pinned_group_and_loose_tabs_come_out_in_the_order_of_the_model() {
        let ws = world();
        let mut ids = Ids::new();
        let tree = build_one(&ws, &mut ids);
        assert_eq!(tree.rev, 0);
        let window = &tree.windows[0];
        assert_eq!(window.title, "window 1");

        let pinned: Vec<u64> = ws.pinned().iter().map(|t| ids.tab(1, *t)).collect();
        assert_eq!(
            window.pinned.iter().map(|t| t.id).collect::<Vec<_>>(),
            pinned
        );

        let track: Vec<u64> = ws.track_order().map(|t| ids.tab(1, t)).collect();
        assert_eq!(flatten(&window.items), track);

        let groups: Vec<&Group> = window
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Group(group) => Some(group),
                Item::Tab(_) => None,
            })
            .collect();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "work");
        assert_eq!(groups[0].color, GroupColor::Blue.index() as u64);
        assert!(groups[0].collapsed);
        assert_eq!(groups[0].tabs.len(), 2);
        let loose = window
            .items
            .iter()
            .filter(|item| matches!(item, Item::Tab(_)))
            .count();
        assert_eq!(loose, 2);
        assert_eq!(window.pinned.len(), 2);
    }

    #[test]
    fn an_explicit_group_splits_the_implicit_run_around_it() {
        let mut ws = CoreWorkspace::new();
        let a = ws.new_tab(None, "sh", None, 0);
        let implicit = ws.group_of_tab(a).unwrap();
        let b = ws.new_tab(Some(implicit), "sh", None, 1);
        ws.new_tab(Some(implicit), "sh", None, 2);
        ws.group_tabs(&[b], "g", GroupColor::Red).unwrap();
        let mut ids = Ids::new();
        let tree = build_one(&ws, &mut ids);
        let kinds: Vec<&str> = tree.windows[0]
            .items
            .iter()
            .map(|item| match item {
                Item::Tab(_) => "tab",
                Item::Group(_) => "group",
            })
            .collect();
        assert_eq!(kinds, ["tab", "group", "tab"]);
    }

    #[test]
    fn active_is_the_protocol_id_of_the_active_tab() {
        let ws = world();
        let mut ids = Ids::new();
        let tree = build_one(&ws, &mut ids);
        let active = ws.active_tab().expect("an active tab");
        assert_eq!(tree.windows[0].active, Some(ids.tab(1, active)));
    }

    #[test]
    fn an_empty_window_has_no_items_and_no_active_tab() {
        let ws = CoreWorkspace::new();
        let mut ids = Ids::new();
        let tree = build_one(&ws, &mut ids);
        let window = &tree.windows[0];
        assert!(window.pinned.is_empty());
        assert!(window.items.is_empty());
        assert_eq!(window.active, None);
    }

    #[test]
    fn split_panes_become_a_tree_with_direction_ratio_and_focus() {
        let mut ws = CoreWorkspace::new();
        let tab = ws.new_tab(None, "sh", None, 0);
        let first = ws.tab(tab).unwrap().panes().focused().id();
        // `pane.split_vertical`: standing divider, panes side by side.
        let second = {
            let panes = ws.tab_mut(tab).unwrap().panes_mut();
            panes.split(first, SplitAxis::Vertical, "sh", None).unwrap()
        };
        // `pane.split_horizontal` of the second: lying divider, stacked.
        let third = {
            let panes = ws.tab_mut(tab).unwrap().panes_mut();
            panes
                .split(second, SplitAxis::Horizontal, "sh", None)
                .unwrap()
        };
        {
            let panes = ws.tab_mut(tab).unwrap().panes_mut();
            assert!(panes.set_ratio(first, 0.25));
            assert!(panes.focus(second));
        }

        let mut ids = Ids::new();
        let tree = build_one(&ws, &mut ids);
        let wire_tab = tab_of(&tree.windows[0].items[0]);

        let PaneNode::Split(root) = &wire_tab.layout else {
            panic!("the root is a split");
        };
        assert_eq!(root.dir, SplitDir::SideBySide);
        assert_eq!(root.ratio, 16384);
        let PaneNode::Pane(left) = &root.first else {
            panic!("a pane");
        };
        assert_eq!(left.id, ids.pane(1, tab, first));
        assert_eq!(left.title, "p0");
        let PaneNode::Split(right) = &root.second else {
            panic!("a split");
        };
        assert_eq!(right.dir, SplitDir::Stacked);
        assert_eq!(right.ratio, 32768);
        let PaneNode::Pane(top) = &right.first else {
            panic!("a pane");
        };
        let PaneNode::Pane(bottom) = &right.second else {
            panic!("a pane");
        };
        assert_eq!(top.id, ids.pane(1, tab, second));
        assert_eq!(bottom.id, ids.pane(1, tab, third));
        assert_eq!(wire_tab.focused, ids.pane(1, tab, second));
    }

    #[test]
    fn the_ratio_is_rounded_and_limited_to_the_wire_domain() {
        assert_eq!(wire_ratio(0.5), 32768);
        assert_eq!(wire_ratio(0.25), 16384);
        assert_eq!(wire_ratio(0.001), 66);
        assert_eq!(wire_ratio(0.0), 1);
        assert_eq!(wire_ratio(-3.0), 1);
        assert_eq!(wire_ratio(1.0), 65535);
        assert_eq!(wire_ratio(7.0), 65535);
        assert_eq!(wire_ratio(0.999_99), 65535);
        assert_eq!(wire_ratio(f32::NAN), 32768);
    }

    #[test]
    fn tab_labels_come_from_the_model_and_the_facts() {
        let mut ws = CoreWorkspace::new();
        let a = ws.new_tab(None, "sh", None, 0);
        let b = ws.new_tab(None, "sh", None, 1);
        ws.tab_mut(a).unwrap().set_custom_title(Some("mine".into()));
        ws.tab_mut(b).unwrap().set_process_title(Some("vim".into()));
        ws.tab_mut(b).unwrap().mark_activity();
        ws.tab_mut(b).unwrap().mark_bell();
        let tab_facts = move |tab: TabId| {
            if tab == b {
                TabFacts {
                    profile: Some("auto:wsl:Ubuntu".into()),
                    remote: Some("me@host".into()),
                }
            } else {
                TabFacts::default()
            }
        };
        let mut ids = Ids::new();
        let panes: &dyn Fn(TabId, PaneId) -> PaneFacts = &plain_pane;
        let (tree, _) = build_workspace(&[input(1, &ws, &tab_facts, panes)], &mut ids);
        let items = &tree.windows[0].items;
        let first = tab_of(&items[0]);
        let second = tab_of(&items[1]);
        assert_eq!(first.title, "mine");
        assert!(first.custom);
        assert_eq!(first.profile, None);
        assert_eq!(first.remote, None);
        assert_eq!(second.title, "vim");
        assert!(!second.custom);
        assert_eq!(second.profile.as_deref(), Some("auto:wsl:Ubuntu"));
        assert_eq!(second.remote.as_deref(), Some("me@host"));
        assert!(second.activity);
        assert!(second.bell);
        assert!(!first.activity && !first.bell);
    }

    #[test]
    fn pane_facts_are_copied_and_the_integers_clamped_to_the_safe_range() {
        let mut ws = CoreWorkspace::new();
        ws.new_tab(None, "sh", None, 0);
        let panes = |_: TabId, _: PaneId| PaneFacts {
            state: PaneState::Exited,
            exit: Some(-3),
            cols: u64::MAX,
            rows: 24,
            cwd: Some("/home/a".into()),
            title: "t".into(),
        };
        let tabs: &dyn Fn(TabId) -> TabFacts = &no_tab_facts;
        let mut ids = Ids::new();
        let (tree, _) = build_workspace(&[input(1, &ws, tabs, &panes)], &mut ids);
        let PaneNode::Pane(pane) = &tab_of(&tree.windows[0].items[0]).layout else {
            panic!("a pane");
        };
        assert_eq!(pane.state, PaneState::Exited);
        assert_eq!(pane.exit, Some(-3));
        assert_eq!(pane.cols, MAX_SAFE_INT as u64);
        assert_eq!(pane.rows, 24);
        assert_eq!(pane.cwd.as_deref(), Some("/home/a"));
        let bytes = encode(&Message::Workspace(tree));
        assert!(decode(&bytes, Direction::HostToClient).is_ok());
    }

    #[test]
    fn windows_keep_the_given_order_and_ids_are_stable_between_builds() {
        let mut one = CoreWorkspace::new();
        one.new_tab(None, "sh", None, 0);
        let mut two = CoreWorkspace::new();
        two.new_tab(None, "sh", None, 0);
        two.new_tab(None, "sh", None, 1);
        let tabs: &dyn Fn(TabId) -> TabFacts = &no_tab_facts;
        let panes: &dyn Fn(TabId, PaneId) -> PaneFacts = &plain_pane;
        let mut ids = Ids::new();

        let (first, _) = build_workspace(
            &[input(7, &two, tabs, panes), input(3, &one, tabs, panes)],
            &mut ids,
        );
        let order: Vec<u64> = first.windows.iter().map(|w| w.id).collect();
        assert_eq!(order, [ids.window(7), ids.window(3)]);

        // Focus changes: the order flips, the ids do not.
        let (second, _) = build_workspace(
            &[input(3, &one, tabs, panes), input(7, &two, tabs, panes)],
            &mut ids,
        );
        assert_eq!(second.windows[0].id, first.windows[1].id);
        assert_eq!(second.windows[1].id, first.windows[0].id);
        assert_eq!(second.windows[1].items, first.windows[0].items);
        assert_eq!(second.windows[0].items, first.windows[1].items);
    }

    #[test]
    fn a_transfer_keeps_the_ids_of_the_tab_and_its_panes() {
        fn split_tab(ws: &mut CoreWorkspace, tab: TabId) {
            let panes = ws.tab_mut(tab).unwrap().panes_mut();
            let focused = panes.focused().id();
            panes
                .split(focused, SplitAxis::Vertical, "sh", None)
                .unwrap();
        }
        let mut source = CoreWorkspace::new();
        let tab = source.new_tab(None, "sh", None, 0);
        split_tab(&mut source, tab);
        let mut ids = Ids::new();
        let before = build_one(&source, &mut ids);

        // The tab arrives in window 2 under another `TabId` (the pane tree
        // travels intact; here an equal one stands in for it).
        let mut target = CoreWorkspace::new();
        target.new_tab(None, "sh", None, 0);
        let new_tab = target.new_tab(None, "sh", None, 1);
        split_tab(&mut target, new_tab);
        ids.rekey_tabs(1, 2, &[(tab, new_tab)]);
        let tabs: &dyn Fn(TabId) -> TabFacts = &no_tab_facts;
        let panes: &dyn Fn(TabId, PaneId) -> PaneFacts = &plain_pane;
        let (after, _) = build_workspace(&[input(2, &target, tabs, panes)], &mut ids);

        let old = tab_of(&before.windows[0].items[0]);
        let moved = after.windows[0]
            .items
            .iter()
            .map(tab_of)
            .find(|t| t.id == old.id)
            .expect("the same tab id in the new window");
        assert_eq!(moved.focused, old.focused);
        assert_eq!(moved.layout, old.layout);
    }

    #[test]
    fn retain_seen_after_a_build_forgets_the_closed_tab() {
        let mut ws = CoreWorkspace::new();
        let a = ws.new_tab(None, "sh", None, 0);
        let b = ws.new_tab(None, "sh", None, 1);
        let mut ids = Ids::new();
        let tabs: &dyn Fn(TabId) -> TabFacts = &no_tab_facts;
        let panes: &dyn Fn(TabId, PaneId) -> PaneFacts = &plain_pane;
        let (_, seen) = build_workspace(&[input(1, &ws, tabs, panes)], &mut ids);
        ids.retain_seen(&seen);
        let id_a = ids.tab(1, a);
        let id_b = ids.tab(1, b);
        ws.close_tab(b);
        let (_, seen) = build_workspace(&[input(1, &ws, tabs, panes)], &mut ids);
        ids.retain_seen(&seen);
        assert_eq!(ids.tab_of(id_b), None);
        assert_eq!(ids.tab_of(id_a), Some((1, a)));
        // A tab opened later never gets the number of the closed one.
        let c = ws.new_tab(None, "sh", None, 1);
        assert!(ids.tab(1, c) > id_b);
    }

    #[test]
    fn the_output_encodes_and_decodes() {
        let ws = world();
        let mut other = CoreWorkspace::new();
        other.new_tab(None, "sh", None, 0);
        let tab_facts = |_: TabId| TabFacts {
            profile: Some("work".into()),
            remote: Some("me@host".into()),
        };
        let panes: &dyn Fn(TabId, PaneId) -> PaneFacts = &plain_pane;
        let none: &dyn Fn(TabId) -> TabFacts = &no_tab_facts;
        let mut ids = Ids::new();
        let (tree, _) = build_workspace(
            &[
                input(1, &ws, &tab_facts, panes),
                input(2, &other, none, panes),
            ],
            &mut ids,
        );
        let bytes = encode(&Message::Workspace(tree.clone()));
        let decoded = decode(&bytes, Direction::HostToClient).expect("decodes");
        assert_eq!(decoded, Message::Workspace(tree));
    }

    fn signals() -> PaneSignals {
        PaneSignals {
            model: Model::Running,
            has_runtime: true,
            dormant: false,
            ssh: false,
            reconnecting: false,
            connected: false,
            link_dropped: false,
            prompt_pending: false,
        }
    }

    #[test]
    fn pane_state_of_a_local_pane() {
        assert_eq!(pane_state(&signals()), (PaneState::Running, None));
        let not_started = PaneSignals {
            model: Model::NotStarted,
            has_runtime: false,
            ..signals()
        };
        assert_eq!(pane_state(&not_started), (PaneState::NotStarted, None));
        let exited = PaneSignals {
            model: Model::Exited { exit_code: 3 },
            ..signals()
        };
        assert_eq!(pane_state(&exited), (PaneState::Exited, Some(3)));
        // A local pane ignores the SSH-only signals.
        let local = PaneSignals {
            link_dropped: true,
            ..signals()
        };
        assert_eq!(pane_state(&local), (PaneState::Running, None));
        let local_exit = PaneSignals {
            model: Model::Exited { exit_code: 1 },
            link_dropped: true,
            ..signals()
        };
        assert_eq!(pane_state(&local_exit), (PaneState::Exited, Some(1)));
    }

    #[test]
    fn pane_state_of_an_ssh_pane() {
        let ssh = PaneSignals {
            ssh: true,
            ..signals()
        };
        // Dormant: the runtime exists, the model is still NotStarted.
        let dormant = PaneSignals {
            model: Model::NotStarted,
            dormant: true,
            ..ssh
        };
        assert_eq!(pane_state(&dormant), (PaneState::NotStarted, None));
        // Connecting until the shell opens, then running.
        assert_eq!(pane_state(&ssh), (PaneState::Connecting, None));
        let connected = PaneSignals {
            connected: true,
            ..ssh
        };
        assert_eq!(pane_state(&connected), (PaneState::Running, None));
        // In a reconnection cycle.
        let cycle = PaneSignals {
            reconnecting: true,
            connected: true,
            ..ssh
        };
        assert_eq!(pane_state(&cycle), (PaneState::Reconnecting, None));
        // A pending question wins over connecting and reconnecting.
        let asking = PaneSignals {
            prompt_pending: true,
            ..ssh
        };
        assert_eq!(pane_state(&asking), (PaneState::AwaitingDesktop, None));
        let asking_again = PaneSignals {
            prompt_pending: true,
            ..cycle
        };
        assert_eq!(
            pane_state(&asking_again),
            (PaneState::AwaitingDesktop, None)
        );
        // The link dropped and the cycle gave up (the model says Exited).
        let gave_up = PaneSignals {
            model: Model::Exited { exit_code: 1 },
            link_dropped: true,
            ..ssh
        };
        assert_eq!(pane_state(&gave_up), (PaneState::Disconnected, None));
        // The remote shell exited with an error: a plain exit.
        let remote_exit = PaneSignals {
            model: Model::Exited { exit_code: 2 },
            ..ssh
        };
        assert_eq!(pane_state(&remote_exit), (PaneState::Exited, Some(2)));
    }

    #[test]
    fn only_a_lost_or_failed_link_counts_as_dropped() {
        use porecatu_term::{FailCause, LostCause};
        assert!(link_dropped(&EndReason::Lost {
            cause: LostCause::Transport
        }));
        assert!(link_dropped(&EndReason::Failed {
            cause: FailCause::Refused
        }));
        assert!(!link_dropped(&EndReason::RemoteEnded {
            code: Some(1),
            signal: None
        }));
        assert!(!link_dropped(&EndReason::Exited {
            success: false,
            code: 1
        }));
    }
}
