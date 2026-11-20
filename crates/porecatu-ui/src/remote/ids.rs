// SPDX-License-Identifier: GPL-3.0-or-later

//! Map of the protocol ids to the process ids of windows, tabs and panes
//! (ADR-0105 section 1).
//!
//! The protocol ids are four monotonic counters of the **process**: they are
//! born the first time an object shows up in a tree sent to a client, they do
//! not change when a tab or a group moves to another window (the transfer
//! re-keys them, [`ProtocolIds::rekey_tabs`] and [`ProtocolIds::rekey_group`])
//! and they are never reused -- not after the object is forgotten, not after the
//! remote access is turned off. Restarting the app zeroes them, which is what
//! the `host_instance` of the `welcome` is for.
//!
//! Pure logic, generic in the window id like `pinned::PinnedRegistry`: the
//! app instantiates it with `winit::window::WindowId`, the tests with a
//! number. The counters start at 1.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::path::{Path, PathBuf};

use porecatu_core::{GroupId, PaneId, TabId};

/// What the last tree built by `tree::build_workspace` contained: the keys
/// whose protocol ids are still in use. [`ProtocolIds::retain_seen`] forgets
/// everything else.
#[derive(Debug)]
pub(crate) struct SeenIds<W> {
    pub(crate) windows: HashSet<W>,
    pub(crate) groups: HashSet<(W, GroupId)>,
    pub(crate) tabs: HashSet<(W, TabId)>,
    pub(crate) panes: HashSet<(W, TabId, PaneId)>,
}

impl<W> Default for SeenIds<W> {
    fn default() -> Self {
        Self {
            windows: HashSet::new(),
            groups: HashSet::new(),
            tabs: HashSet::new(),
            panes: HashSet::new(),
        }
    }
}

/// One forward map (internal key to protocol id) and its reverse, kept
/// together so they cannot disagree.
#[derive(Debug)]
struct Table<K> {
    forward: HashMap<K, u64>,
    reverse: HashMap<u64, K>,
}

impl<K: Clone + Eq + Hash> Table<K> {
    fn new() -> Self {
        Self {
            forward: HashMap::new(),
            reverse: HashMap::new(),
        }
    }

    fn get_or_assign(&mut self, key: K, counter: &mut u64) -> u64 {
        if let Some(id) = self.forward.get(&key) {
            return *id;
        }
        let id = *counter;
        *counter += 1;
        self.forward.insert(key.clone(), id);
        self.reverse.insert(id, key);
        id
    }

    fn of(&self, id: u64) -> Option<K> {
        self.reverse.get(&id).cloned()
    }

    /// The key of `id`, borrowed (for keys that are not cheap to clone).
    fn of_ref(&self, id: u64) -> Option<&K> {
        self.reverse.get(&id)
    }

    fn remove(&mut self, key: &K) -> Option<u64> {
        let id = self.forward.remove(key)?;
        self.reverse.remove(&id);
        Some(id)
    }

    /// Binds `id` to `key` (the id keeps its number, only the key changes).
    fn insert(&mut self, key: K, id: u64) {
        self.forward.insert(key.clone(), id);
        self.reverse.insert(id, key);
    }

    fn retain(&mut self, keep: impl Fn(&K) -> bool) {
        self.forward.retain(|key, _| keep(key));
        self.reverse.retain(|_, key| keep(key));
    }
}

/// The protocol ids of the process.
#[derive(Debug)]
pub(crate) struct ProtocolIds<W> {
    next_window: u64,
    next_group: u64,
    next_tab: u64,
    next_pane: u64,
    next_session: u64,
    windows: Table<W>,
    groups: Table<(W, GroupId)>,
    tabs: Table<(W, TabId)>,
    panes: Table<(W, TabId, PaneId)>,
    /// Saved named sessions, keyed by the file (ADR-0113 section 2). Not
    /// tied to the tree: `retain_seen` and `forget_window` never touch it, and
    /// it only grows.
    sessions: Table<PathBuf>,
}

impl<W: Copy + Eq + Hash> Default for ProtocolIds<W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Copy + Eq + Hash> ProtocolIds<W> {
    pub(crate) fn new() -> Self {
        Self {
            next_window: 1,
            next_group: 1,
            next_tab: 1,
            next_pane: 1,
            next_session: 1,
            windows: Table::new(),
            groups: Table::new(),
            tabs: Table::new(),
            panes: Table::new(),
            sessions: Table::new(),
        }
    }

    /// The protocol id of a window, assigned on first use.
    pub(crate) fn window(&mut self, window: W) -> u64 {
        self.windows.get_or_assign(window, &mut self.next_window)
    }

    /// The protocol id of a group, assigned on first use.
    pub(crate) fn group(&mut self, window: W, group: GroupId) -> u64 {
        self.groups
            .get_or_assign((window, group), &mut self.next_group)
    }

    /// The protocol id of a tab, assigned on first use.
    pub(crate) fn tab(&mut self, window: W, tab: TabId) -> u64 {
        self.tabs.get_or_assign((window, tab), &mut self.next_tab)
    }

    /// The protocol id of a pane, assigned on first use.
    pub(crate) fn pane(&mut self, window: W, tab: TabId, pane: PaneId) -> u64 {
        self.panes
            .get_or_assign((window, tab, pane), &mut self.next_pane)
    }

    /// The protocol id of a saved named session, assigned on first use
    /// (ADR-0113 section 2). The same file keeps its id for the life of the
    /// process; ids are never reused.
    pub(crate) fn session(&mut self, file: &Path) -> u64 {
        self.sessions
            .get_or_assign(file.to_path_buf(), &mut self.next_session)
    }

    /// The file of a session id the host listed before.
    pub(crate) fn session_of(&self, id: u64) -> Option<&Path> {
        self.sessions.of_ref(id).map(PathBuf::as_path)
    }

    pub(crate) fn window_of(&self, id: u64) -> Option<W> {
        self.windows.of(id)
    }

    pub(crate) fn group_of(&self, id: u64) -> Option<(W, GroupId)> {
        self.groups.of(id)
    }

    pub(crate) fn tab_of(&self, id: u64) -> Option<(W, TabId)> {
        self.tabs.of(id)
    }

    pub(crate) fn pane_of(&self, id: u64) -> Option<(W, TabId, PaneId)> {
        self.panes.of(id)
    }

    /// The protocol id of a pane **if it has one**; unlike [`Self::pane`] it
    /// assigns nothing (a pane that never was in a tree has no id, and the
    /// bell or the dirt of such a pane is not for the phone).
    pub(crate) fn pane_id_of(&self, window: W, tab: TabId, pane: PaneId) -> Option<u64> {
        self.panes.forward.get(&(window, tab, pane)).copied()
    }

    /// A transfer between windows (ADR-0105 section 1, ADR-0090): `moves` are
    /// the `(old, new)` tab ids of `from` and `to`. The protocol id of each
    /// tab **and of all its panes** stays; only the internal key changes.
    /// A tab that never had a protocol id is ignored (it gets one, under its
    /// new key, the first time it shows up in a tree). All the old keys are
    /// taken out before any new one goes in, so overlapping ids cannot clash.
    pub(crate) fn rekey_tabs(&mut self, from: W, to: W, moves: &[(TabId, TabId)]) {
        let mut tabs = Vec::new();
        let mut panes = Vec::new();
        for &(old, new) in moves {
            if let Some(id) = self.tabs.remove(&(from, old)) {
                tabs.push(((to, new), id));
            }
            let keys: Vec<(W, TabId, PaneId)> = self
                .panes
                .forward
                .keys()
                .filter(|(window, tab, _)| *window == from && *tab == old)
                .copied()
                .collect();
            for key in keys {
                if let Some(id) = self.panes.remove(&key) {
                    panes.push(((to, new, key.2), id));
                }
            }
        }
        for (key, id) in tabs {
            self.tabs.insert(key, id);
        }
        for (key, id) in panes {
            self.panes.insert(key, id);
        }
    }

    /// The same for a group that changes window (and gets a new `GroupId`
    /// there). The tabs of the group go through [`Self::rekey_tabs`].
    pub(crate) fn rekey_group(&mut self, from: W, old: GroupId, to: W, new: GroupId) {
        if let Some(id) = self.groups.remove(&(from, old)) {
            self.groups.insert((to, new), id);
        }
    }

    /// A closed window: everything keyed by it goes. The counters stay.
    pub(crate) fn forget_window(&mut self, window: W) {
        self.windows.remove(&window);
        self.groups.retain(|(w, _)| *w != window);
        self.tabs.retain(|(w, _)| *w != window);
        self.panes.retain(|(w, _, _)| *w != window);
    }

    /// Forgets the objects that were not in the last tree built. The counters
    /// do not go back, so a forgotten id is never given to anything else.
    pub(crate) fn retain_seen(&mut self, seen: &SeenIds<W>) {
        self.windows.retain(|key| seen.windows.contains(key));
        self.groups.retain(|key| seen.groups.contains(key));
        self.tabs.retain(|key| seen.tabs.contains(key));
        self.panes.retain(|key| seen.panes.contains(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(n: u32) -> TabId {
        TabId::new(n)
    }

    fn pane(n: u32) -> PaneId {
        PaneId::new(n)
    }

    fn group(n: u32) -> GroupId {
        GroupId::new(n)
    }

    #[test]
    fn counters_start_at_one_and_ids_are_stable() {
        let mut ids = ProtocolIds::<u8>::new();
        assert_eq!(ids.window(7), 1);
        assert_eq!(ids.tab(7, tab(0)), 1);
        assert_eq!(ids.pane(7, tab(0), pane(0)), 1);
        assert_eq!(ids.group(7, group(0)), 1);
        // Asking again, in any order, gives the same numbers.
        assert_eq!(ids.window(7), 1);
        assert_eq!(ids.tab(7, tab(0)), 1);
        assert_eq!(ids.tab(7, tab(1)), 2);
        assert_eq!(ids.tab(7, tab(0)), 1);
        assert_eq!(ids.pane(7, tab(0), pane(0)), 1);
        assert_eq!(ids.window(8), 2);
    }

    #[test]
    fn looking_up_a_pane_never_assigns_an_id() {
        let mut ids = ProtocolIds::<u8>::new();
        assert_eq!(ids.pane_id_of(1, tab(0), pane(0)), None);
        // Nothing was taken from the counter by the lookup.
        assert_eq!(ids.pane(1, tab(0), pane(0)), 1);
        assert_eq!(ids.pane_id_of(1, tab(0), pane(0)), Some(1));
        assert_eq!(ids.pane_id_of(2, tab(0), pane(0)), None);
    }

    #[test]
    fn the_four_counters_are_independent() {
        let mut ids = ProtocolIds::<u8>::new();
        ids.tab(1, tab(0));
        ids.tab(1, tab(1));
        assert_eq!(ids.window(1), 1);
        assert_eq!(ids.pane(1, tab(0), pane(0)), 1);
        assert_eq!(ids.group(1, group(5)), 1);
    }

    #[test]
    fn the_same_internal_id_in_two_windows_gets_two_protocol_ids() {
        let mut ids = ProtocolIds::<u8>::new();
        let a = ids.tab(1, tab(0));
        let b = ids.tab(2, tab(0));
        assert_ne!(a, b);
        assert_eq!(ids.tab_of(a), Some((1, tab(0))));
        assert_eq!(ids.tab_of(b), Some((2, tab(0))));
        // The same pane id in two tabs is two panes.
        let p1 = ids.pane(1, tab(0), pane(0));
        let p2 = ids.pane(1, tab(1), pane(0));
        assert_ne!(p1, p2);
    }

    #[test]
    fn the_reverse_lookups_point_at_the_keys() {
        let mut ids = ProtocolIds::<u8>::new();
        let w = ids.window(3);
        let g = ids.group(3, group(2));
        let t = ids.tab(3, tab(4));
        let p = ids.pane(3, tab(4), pane(1));
        assert_eq!(ids.window_of(w), Some(3));
        assert_eq!(ids.group_of(g), Some((3, group(2))));
        assert_eq!(ids.tab_of(t), Some((3, tab(4))));
        assert_eq!(ids.pane_of(p), Some((3, tab(4), pane(1))));
        assert_eq!(ids.window_of(99), None);
        assert_eq!(ids.group_of(99), None);
        assert_eq!(ids.tab_of(99), None);
        assert_eq!(ids.pane_of(99), None);
    }

    #[test]
    fn rekey_keeps_the_number_and_the_reverse_points_at_the_new_key() {
        let mut ids = ProtocolIds::<u8>::new();
        let t = ids.tab(1, tab(3));
        let other = ids.tab(1, tab(4));
        let p0 = ids.pane(1, tab(3), pane(0));
        let p1 = ids.pane(1, tab(3), pane(1));
        let other_pane = ids.pane(1, tab(4), pane(0));

        ids.rekey_tabs(1, 2, &[(tab(3), tab(0))]);

        // Same numbers under the new keys.
        assert_eq!(ids.tab(2, tab(0)), t);
        assert_eq!(ids.pane(2, tab(0), pane(0)), p0);
        assert_eq!(ids.pane(2, tab(0), pane(1)), p1);
        assert_eq!(ids.tab_of(t), Some((2, tab(0))));
        assert_eq!(ids.pane_of(p0), Some((2, tab(0), pane(0))));
        assert_eq!(ids.pane_of(p1), Some((2, tab(0), pane(1))));
        // The tab that stayed is untouched.
        assert_eq!(ids.tab(1, tab(4)), other);
        assert_eq!(ids.pane(1, tab(4), pane(0)), other_pane);
        // And the old key is free: asking for it gives a brand new number.
        let fresh = ids.tab(1, tab(3));
        assert!(fresh > other);
        assert_ne!(fresh, t);
    }

    #[test]
    fn rekey_of_several_tabs_with_overlapping_ids_does_not_clash() {
        let mut ids = ProtocolIds::<u8>::new();
        let a = ids.tab(1, tab(0));
        let b = ids.tab(1, tab(1));
        // Within one window, 0 -> 1 and 1 -> 0 (a swap).
        ids.rekey_tabs(1, 1, &[(tab(0), tab(1)), (tab(1), tab(0))]);
        assert_eq!(ids.tab(1, tab(1)), a);
        assert_eq!(ids.tab(1, tab(0)), b);
    }

    #[test]
    fn rekey_of_an_unknown_tab_assigns_nothing() {
        let mut ids = ProtocolIds::<u8>::new();
        ids.rekey_tabs(1, 2, &[(tab(0), tab(0))]);
        // The first tab asked for is still number 1.
        assert_eq!(ids.tab(2, tab(0)), 1);
    }

    #[test]
    fn rekey_group_keeps_the_number() {
        let mut ids = ProtocolIds::<u8>::new();
        let g = ids.group(1, group(2));
        ids.rekey_group(1, group(2), 2, group(0));
        assert_eq!(ids.group(2, group(0)), g);
        assert_eq!(ids.group_of(g), Some((2, group(0))));
        assert!(ids.group(1, group(2)) > g);
    }

    #[test]
    fn nothing_is_reused_after_forgetting() {
        let mut ids = ProtocolIds::<u8>::new();
        let t = ids.tab(1, tab(0));
        let p = ids.pane(1, tab(0), pane(0));
        let g = ids.group(1, group(0));
        let w = ids.window(1);
        ids.forget_window(1);
        assert_eq!(ids.tab_of(t), None);
        assert_eq!(ids.pane_of(p), None);
        assert_eq!(ids.group_of(g), None);
        assert_eq!(ids.window_of(w), None);
        // The same internal keys come back as new objects, with new numbers.
        assert!(ids.window(1) > w);
        assert!(ids.tab(1, tab(0)) > t);
        assert!(ids.pane(1, tab(0), pane(0)) > p);
        assert!(ids.group(1, group(0)) > g);
    }

    #[test]
    fn forgetting_a_window_leaves_the_others() {
        let mut ids = ProtocolIds::<u8>::new();
        let keep = ids.tab(2, tab(0));
        let keep_pane = ids.pane(2, tab(0), pane(0));
        ids.tab(1, tab(0));
        ids.forget_window(1);
        assert_eq!(ids.tab(2, tab(0)), keep);
        assert_eq!(ids.pane(2, tab(0), pane(0)), keep_pane);
    }

    #[test]
    fn retain_seen_drops_what_was_not_in_the_last_tree() {
        let mut ids = ProtocolIds::<u8>::new();
        let w = ids.window(1);
        let t0 = ids.tab(1, tab(0));
        let t1 = ids.tab(1, tab(1));
        let p0 = ids.pane(1, tab(0), pane(0));
        let p1 = ids.pane(1, tab(1), pane(0));
        let g = ids.group(1, group(0));

        let mut seen = SeenIds::default();
        seen.windows.insert(1);
        seen.tabs.insert((1, tab(0)));
        seen.panes.insert((1, tab(0), pane(0)));
        ids.retain_seen(&seen);

        assert_eq!(ids.window_of(w), Some(1));
        assert_eq!(ids.tab_of(t0), Some((1, tab(0))));
        assert_eq!(ids.pane_of(p0), Some((1, tab(0), pane(0))));
        assert_eq!(ids.tab_of(t1), None);
        assert_eq!(ids.pane_of(p1), None);
        assert_eq!(ids.group_of(g), None);
        // The counters did not go back.
        assert!(ids.tab(1, tab(1)) > t1);
        assert!(ids.group(1, group(0)) > g);
    }

    #[test]
    fn the_same_session_file_keeps_its_id_and_other_files_get_growing_ones() {
        let mut ids = ProtocolIds::<u8>::new();
        let a = ids.session(Path::new("a.json"));
        let b = ids.session(Path::new("b.json"));
        assert_eq!(a, 1);
        assert!(b > a);
        assert_eq!(ids.session(Path::new("a.json")), a);
        assert_eq!(ids.session(Path::new("b.json")), b);
        assert_eq!(ids.session_of(a), Some(Path::new("a.json")));
        assert_eq!(ids.session_of(b), Some(Path::new("b.json")));
        assert_eq!(ids.session_of(99), None);
        // The session counter is independent of the other four.
        assert_eq!(ids.window(1), 1);
        assert_eq!(ids.tab(1, tab(0)), 1);
    }

    #[test]
    fn retain_seen_and_forget_window_do_not_prune_sessions() {
        let mut ids = ProtocolIds::<u8>::new();
        let s = ids.session(Path::new("a.json"));
        ids.window(1);
        ids.retain_seen(&SeenIds::default());
        ids.forget_window(1);
        assert_eq!(ids.session_of(s), Some(Path::new("a.json")));
        assert_eq!(ids.session(Path::new("a.json")), s);
        assert!(ids.session(Path::new("c.json")) > s);
    }

    #[test]
    fn panes_follow_the_tab_through_two_transfers() {
        let mut ids = ProtocolIds::<u8>::new();
        let t = ids.tab(1, tab(0));
        let p = ids.pane(1, tab(0), pane(2));
        ids.rekey_tabs(1, 2, &[(tab(0), tab(5))]);
        ids.rekey_tabs(2, 3, &[(tab(5), tab(1))]);
        assert_eq!(ids.tab_of(t), Some((3, tab(1))));
        assert_eq!(ids.pane_of(p), Some((3, tab(1), pane(2))));
    }
}
