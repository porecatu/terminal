// SPDX-License-Identifier: GPL-3.0-or-later

//! The pinned-tab registry (ADR-0086 §2, PRD-021): the list of pinned tabs
//! of the **process**, not of a window -- the same place the per-repository
//! Git state (ADR-0052) occupies in `App`.
//!
//! - `entries` is the order of `pinned.json`: one [`PinnedEntry`] per pinned
//!   tab, with its snapshot (`TabV1`, RF-21.9) and an internal
//!   [`PinnedKey`]. The key is the identity, never the index: reordering,
//!   unpinning another tab or pinning a new one changes nobody's key (the
//!   lesson of the profile editor, ADR-0080). It is born on load and on the
//!   pin gesture and is never written to disk.
//! - `live` says which entries are open right now and where (`window`,
//!   `TabId`); closing a window forgets its bindings and leaves `entries`
//!   alone (RF-21.25).
//! - `writable` is `false` after a newer schema was found: the file is not
//!   ours to overwrite (RF-21.29).
//!
//! Pure logic, generic in the window id so the ordering rules are tested
//! with two "windows" (a winit `WindowId` cannot be built in a test). `App`
//! instantiates it with `WindowId`.
//!
//! *When* to write the file is the caller's rule (ADR-0086 §3: on the spot,
//! on each gesture that changes the list, never derived from live state);
//! [`PinnedRegistry::save`] is only the primitive.

use std::collections::HashMap;
use std::hash::Hash;
use std::path::PathBuf;
use std::{io, mem};

use porecatu_core::TabId;
use porecatu_session::{PinnedFileV1, PinnedLoadOutcome, TabV1};

/// Identity of a pinned entry, internal to the process (ADR-0086 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PinnedKey(u64);

/// One pinned tab of the list: its key and its snapshot (RF-21.9).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PinnedEntry {
    pub key: PinnedKey,
    pub saved: TabV1,
}

/// What the load has to say to the user (ADR-0086 §3); `Missing` says
/// nothing (RF-21.29). Typed, never prose: the text is composed in `ui`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PinnedLoadNotice {
    /// Invalid file, kept at `path` (a `.corrupt[.N]`); the list is empty.
    Corrupt(PathBuf),
    /// `schema_version` newer than this binary reads; the file is untouched,
    /// the list is empty and read-only.
    NewerSchema { found: u32, supported: u32 },
}

/// Why [`PinnedRegistry::save`] did not write. Typed, never prose: the text
/// is composed in `messages` (CLAUDE.md, "Texto de interface só nasce em
/// `porecatu-ui`").
#[derive(Debug)]
pub(crate) enum PinnedSaveError {
    /// A newer schema was found: the file is not ours to overwrite
    /// (ADR-0086 §3).
    ReadOnly,
    /// The state directory could not be resolved.
    NoPath,
    /// The write itself failed.
    Io(io::Error),
}

pub(crate) struct PinnedRegistry<W> {
    entries: Vec<PinnedEntry>,
    live: HashMap<PinnedKey, (W, TabId)>,
    writable: bool,
    path: Option<PathBuf>,
    next_key: u64,
}

impl<W: Copy + Eq + Hash> PinnedRegistry<W> {
    /// Builds the registry from what the load found. `Missing` is an empty,
    /// writable list; `Quarantined` and `NewerSchema` are empty too (fail
    /// closed, never a partial list) and come with a notice, the latter
    /// also read-only.
    pub(crate) fn from_load(
        outcome: PinnedLoadOutcome,
        path: Option<PathBuf>,
    ) -> (Self, Option<PinnedLoadNotice>) {
        let mut registry = Self {
            entries: Vec::new(),
            live: HashMap::new(),
            writable: true,
            path,
            next_key: 0,
        };
        let notice = match outcome {
            PinnedLoadOutcome::Missing => None,
            PinnedLoadOutcome::Loaded(file) => {
                for saved in file.tabs {
                    let key = registry.fresh_key();
                    registry.entries.push(PinnedEntry { key, saved });
                }
                None
            }
            PinnedLoadOutcome::Quarantined { path } => Some(PinnedLoadNotice::Corrupt(path)),
            PinnedLoadOutcome::NewerSchema { found, supported } => {
                registry.writable = false;
                Some(PinnedLoadNotice::NewerSchema { found, supported })
            }
        };
        (registry, notice)
    }

    fn fresh_key(&mut self) -> PinnedKey {
        let key = PinnedKey(self.next_key);
        self.next_key += 1;
        key
    }

    /// The list, in the order of the file.
    pub(crate) fn entries(&self) -> &[PinnedEntry] {
        &self.entries
    }

    /// `false` after a newer schema: pinning is refused then (ADR-0086 §3).
    pub(crate) fn is_writable(&self) -> bool {
        self.writable
    }

    /// The key of the entry that is the live tab `tab` of `window`.
    pub(crate) fn key_of(&self, window: W, tab: TabId) -> Option<PinnedKey> {
        self.live
            .iter()
            .find(|&(_, &(w, t))| w == window && t == tab)
            .map(|(&key, _)| key)
    }

    /// Where the entry is open right now, if it is.
    pub(crate) fn live_of(&self, key: PinnedKey) -> Option<(W, TabId)> {
        self.live.get(&key).copied()
    }

    /// The keys of the entries open in `window` right now -- what the window
    /// takes with it when it closes (ADR-0088 §5 frees their icon textures).
    pub(crate) fn live_keys_in(&self, window: W) -> Vec<PinnedKey> {
        self.live
            .iter()
            .filter(|&(_, &(w, _))| w == window)
            .map(|(&key, _)| key)
            .collect()
    }

    /// Records that `key` is open as `tab` in `window` (RF-21.26: any
    /// window may hold it; a second binding replaces the first).
    pub(crate) fn bind_live(&mut self, key: PinnedKey, window: W, tab: TabId) {
        self.live.insert(key, (window, tab));
    }

    /// The window is gone: its bindings go, the list does not change
    /// (RF-21.25).
    pub(crate) fn forget_window(&mut self, window: W) {
        self.live.retain(|_, &mut (w, _)| w != window);
    }

    /// The live tab is gone (closed without touching the list); the entry
    /// stays.
    // Every close that reaches the registry today is a gesture that removes
    // the entry (`remove` drops the binding with it); this is for a close
    // that is not one.
    #[allow(dead_code)]
    pub(crate) fn forget_tab(&mut self, window: W, tab: TabId) {
        self.live
            .retain(|_, &mut (w, t)| !(w == window && t == tab));
    }

    /// The position of `key` in the list -- what `WindowV1::active_pinned`
    /// records (ADR-0086 §4).
    pub(crate) fn index_of(&self, key: PinnedKey) -> Option<usize> {
        self.entries.iter().position(|e| e.key == key)
    }

    /// The file as it would be written, in the order of `entries`.
    pub(crate) fn to_file(&self) -> PinnedFileV1 {
        PinnedFileV1 {
            tabs: self.entries.iter().map(|e| e.saved.clone()).collect(),
            ..PinnedFileV1::default()
        }
    }

    /// Writes the list (atomic, `porecatu_session::save_pinned_to`). Refuses
    /// when the registry is read-only (newer schema: the file is not ours)
    /// or without a path. Called only by a gesture, never by derivation
    /// (ADR-0086 §3).
    pub(crate) fn save(&self) -> Result<(), PinnedSaveError> {
        if !self.writable {
            return Err(PinnedSaveError::ReadOnly);
        }
        let path = self.path.as_deref().ok_or(PinnedSaveError::NoPath)?;
        porecatu_session::save_pinned_to(path, &self.to_file()).map_err(PinnedSaveError::Io)
    }

    /// Whether `window` holds at least one live entry.
    fn is_live_in(&self, key: PinnedKey, window: W) -> bool {
        self.live.get(&key).is_some_and(|&(w, _)| w == window)
    }

    /// Pin gesture (ADR-0086 §2): inserts `saved` for the live tab `tab` of
    /// `window` and binds it. Position: right after the entry of the left
    /// neighbour in the zone; with no left neighbour, right before the first
    /// entry live in that window; with no entry live in that window, at the
    /// end of the list. A neighbour that is not in the list counts as none.
    pub(crate) fn insert_for_pin(
        &mut self,
        window: W,
        tab: TabId,
        saved: TabV1,
        left_neighbor: Option<PinnedKey>,
    ) -> PinnedKey {
        let index = self.insertion_index(window, left_neighbor);
        let key = self.fresh_key();
        self.entries.insert(index, PinnedEntry { key, saved });
        self.bind_live(key, window, tab);
        key
    }

    /// The ADR-0086 §2 rule, shared by the pin gesture and by a pinned tab
    /// that changes window ([`Self::relocate`]): right after the left
    /// neighbour; with none, right before the first entry live in `window`;
    /// with none, at the end.
    fn insertion_index(&self, window: W, left_neighbor: Option<PinnedKey>) -> usize {
        let after_neighbor = left_neighbor
            .and_then(|key| self.index_of(key))
            .map(|i| i + 1);
        after_neighbor
            .or_else(|| {
                self.entries
                    .iter()
                    .position(|e| self.is_live_in(e.key, window))
            })
            .unwrap_or(self.entries.len())
    }

    /// A pinned tab arrived in the zone of another window (ADR-0090 §5,
    /// RF-22.26): the entry leaves its position and is reinserted by the same
    /// rule as [`Self::insert_for_pin`], keeping its key and snapshot, and
    /// `live[key]` becomes `(to_window, new_tab)`. Returns `true` when the
    /// **order** of the list changed -- only then does the caller write
    /// `pinned.json`. `false` also when `key` is not in the list.
    pub(crate) fn relocate(
        &mut self,
        key: PinnedKey,
        to_window: W,
        new_tab: TabId,
        left_neighbor: Option<PinnedKey>,
    ) -> bool {
        let Some(old_index) = self.index_of(key) else {
            return false;
        };
        let entry = self.entries.remove(old_index);
        // The entry is out of the list while the index is computed, so it is
        // neither its own neighbour nor "the first entry of the window".
        let index = self.insertion_index(to_window, left_neighbor);
        self.entries.insert(index, entry);
        self.bind_live(key, to_window, new_tab);
        index != old_index
    }

    /// Reorder gesture in the zone of `window` (ADR-0086 §2): `zone_order`
    /// is the new order of the live tabs of that window. Its entries swap
    /// **among the positions they already occupy** in the list, so the
    /// entries of other windows do not move. An entry the order does not
    /// mention keeps its relative place after the mentioned ones.
    pub(crate) fn reorder_window(&mut self, window: W, zone_order: &[TabId]) {
        let positions: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| self.is_live_in(e.key, window))
            .map(|(i, _)| i)
            .collect();
        let mut slots: Vec<Option<PinnedEntry>> =
            mem::take(&mut self.entries).into_iter().map(Some).collect();
        let mut mine: Vec<PinnedEntry> =
            positions.iter().filter_map(|&i| slots[i].take()).collect();
        // Stable: ties (unmentioned entries) keep their relative order.
        mine.sort_by_key(|entry| {
            self.live
                .get(&entry.key)
                .and_then(|&(_, tab)| zone_order.iter().position(|&t| t == tab))
                .unwrap_or(usize::MAX)
        });
        for (&position, entry) in positions.iter().zip(mine) {
            slots[position] = Some(entry);
        }
        self.entries = slots.into_iter().flatten().collect();
    }

    /// Unpin or close-a-pinned-tab gesture: the entry leaves the list and
    /// its binding goes with it.
    pub(crate) fn remove(&mut self, key: PinnedKey) {
        self.entries.retain(|e| e.key != key);
        self.live.remove(&key);
    }
}

/// What a process exit does to the tab that owns the pane (RF-1.3,
/// RF-21.24, ADR-0086 §7). One decision point, asked in the handler of
/// `TermEvent::Exit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitDisposition {
    /// A zero exit of a pane that is not the last: the pane closes.
    ClosePane,
    /// A zero exit of the last pane of a tab that is not pinned: the tab
    /// closes (RF-1.3).
    CloseTab,
    /// The pane stays, `Exited`, with the note: a non-zero exit, or the last
    /// pane of a **pinned** tab, which a typed `exit` must not cost the list
    /// (RF-21.24).
    KeepExited,
}

/// Decides what a process exit does. `pinned` is `Workspace::is_pinned` of
/// the tab: the exception lives in this one function, with no state of its
/// own.
pub(crate) fn exit_disposition(success: bool, is_last_pane: bool, pinned: bool) -> ExitDisposition {
    match (success, is_last_pane, pinned) {
        (false, _, _) => ExitDisposition::KeepExited,
        (true, false, _) => ExitDisposition::ClosePane,
        (true, true, true) => ExitDisposition::KeepExited,
        (true, true, false) => ExitDisposition::CloseTab,
    }
}

/// What closing a tab by a gesture (`tab.close`, the menu item, the close
/// button) does first (RF-1.6, RF-21.23).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseAsk {
    /// Closes at once.
    Direct,
    /// The RF-1.6 dialog: a program is in the foreground.
    ActiveProcess,
    /// The dialog of a pinned tab: always asked, outside
    /// `confirm_close_with_process`; with `active_process` the RF-1.6 phrase
    /// joins it as one more line, never a second dialog.
    Pinned { active_process: bool },
}

/// Which dialog closing a tab opens, if any. `process_would_confirm` is the
/// RF-1.6 answer (already behind `confirm_close_with_process`), `false` for a
/// tab with nothing running (ADR-0037 §3).
pub(crate) fn close_ask(pinned: bool, process_would_confirm: bool) -> CloseAsk {
    match (pinned, process_would_confirm) {
        (true, active_process) => CloseAsk::Pinned { active_process },
        (false, true) => CloseAsk::ActiveProcess,
        (false, false) => CloseAsk::Direct,
    }
}

/// The tab right before position `pos` in the zone: the "left neighbour"
/// the registry inserts a new entry after (ADR-0086 §2). `pos` is clamped
/// like `Workspace::pin_tab` does.
pub(crate) fn left_neighbor_in_zone(zone: &[TabId], pos: usize) -> Option<TabId> {
    let pos = pos.min(zone.len());
    pos.checked_sub(1).and_then(|i| zone.get(i)).copied()
}

/// Where the keyboard reorder moves `tab` in the zone, or `None` at the
/// border: a pinned tab never crosses into the track, nor the first tab of
/// the track into the zone (RF-21.11).
pub(crate) fn zone_move_target(zone: &[TabId], tab: TabId, delta: isize) -> Option<usize> {
    let index = zone.iter().position(|&t| t == tab)?;
    let target = index.checked_add_signed(delta)?;
    (target < zone.len()).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny window id for the tests.
    type Win = u8;

    fn saved(tag: &str) -> TabV1 {
        TabV1 {
            id: 0,
            custom_title: Some(tag.to_string()),
            cwd: None,
            spawn_program: Some("zsh".to_string()),
            profile: None,
            panes: None,
        }
    }

    fn tag(entry: &PinnedEntry) -> &str {
        entry.saved.custom_title.as_deref().unwrap()
    }

    fn tags(registry: &PinnedRegistry<Win>) -> Vec<&str> {
        registry.entries().iter().map(tag).collect()
    }

    fn loaded(tags: &[&str]) -> PinnedRegistry<Win> {
        let file = PinnedFileV1 {
            tabs: tags.iter().map(|t| saved(t)).collect(),
            ..PinnedFileV1::default()
        };
        let (registry, notice) = PinnedRegistry::from_load(PinnedLoadOutcome::Loaded(file), None);
        assert_eq!(notice, None);
        registry
    }

    fn key(registry: &PinnedRegistry<Win>, index: usize) -> PinnedKey {
        registry.entries()[index].key
    }

    fn tab(n: u32) -> TabId {
        TabId::new(n)
    }

    // --- from_load ---

    #[test]
    fn missing_is_an_empty_writable_list_without_a_notice() {
        let (registry, notice) = PinnedRegistry::<Win>::from_load(PinnedLoadOutcome::Missing, None);
        assert!(registry.entries().is_empty());
        assert!(registry.is_writable());
        assert_eq!(notice, None);
    }

    #[test]
    fn loaded_keeps_the_order_of_the_file_with_distinct_keys() {
        let registry = loaded(&["a", "b", "c"]);
        assert_eq!(tags(&registry), ["a", "b", "c"]);
        assert!(registry.is_writable());
        let keys: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();
        assert_ne!(keys[0], keys[1]);
        assert_ne!(keys[1], keys[2]);
        assert_ne!(keys[0], keys[2]);
        assert_eq!(registry.index_of(keys[2]), Some(2));
    }

    #[test]
    fn quarantined_is_empty_and_writable_with_a_notice_naming_the_file() {
        let corrupt = PathBuf::from("/state/pinned.json.corrupt");
        let (registry, notice) = PinnedRegistry::<Win>::from_load(
            PinnedLoadOutcome::Quarantined {
                path: corrupt.clone(),
            },
            None,
        );
        assert!(registry.entries().is_empty());
        assert!(
            registry.is_writable(),
            "the bad file is gone: a pin may write"
        );
        assert_eq!(notice, Some(PinnedLoadNotice::Corrupt(corrupt)));
    }

    #[test]
    fn newer_schema_is_empty_read_only_with_a_notice() {
        let (registry, notice) = PinnedRegistry::<Win>::from_load(
            PinnedLoadOutcome::NewerSchema {
                found: 7,
                supported: 1,
            },
            Some(PathBuf::from("/state/pinned.json")),
        );
        assert!(registry.entries().is_empty());
        assert!(!registry.is_writable());
        assert_eq!(
            notice,
            Some(PinnedLoadNotice::NewerSchema {
                found: 7,
                supported: 1
            })
        );
        assert!(
            registry.save().is_err(),
            "a newer file is never overwritten"
        );
    }

    // --- live binding ---

    #[test]
    fn bind_and_lookup_by_window_and_tab() {
        let mut registry = loaded(&["a", "b"]);
        let (ka, kb) = (key(&registry, 0), key(&registry, 1));
        registry.bind_live(ka, 1, tab(10));
        registry.bind_live(kb, 2, tab(10));
        assert_eq!(registry.key_of(1, tab(10)), Some(ka));
        assert_eq!(registry.key_of(2, tab(10)), Some(kb));
        assert_eq!(registry.key_of(1, tab(11)), None);
        assert_eq!(registry.live_of(ka), Some((1, tab(10))));
    }

    #[test]
    fn forget_window_drops_only_its_bindings_and_never_changes_the_list() {
        let mut registry = loaded(&["a", "b", "c"]);
        let keys: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();
        registry.bind_live(keys[0], 1, tab(1));
        registry.bind_live(keys[1], 2, tab(1));
        registry.bind_live(keys[2], 1, tab(2));

        registry.forget_window(1);

        assert_eq!(registry.live_of(keys[0]), None);
        assert_eq!(registry.live_of(keys[2]), None);
        assert_eq!(registry.live_of(keys[1]), Some((2, tab(1))));
        assert_eq!(tags(&registry), ["a", "b", "c"], "RF-21.25: the list stays");
    }

    #[test]
    fn live_keys_in_lists_only_the_entries_open_in_that_window() {
        let mut registry = loaded(&["a", "b", "c"]);
        let keys: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();
        registry.bind_live(keys[0], 1, tab(1));
        registry.bind_live(keys[1], 2, tab(1));
        // `c` is in the list but open nowhere (its spawn failed).
        let mut in_one = registry.live_keys_in(1);
        in_one.sort_by_key(|k| registry.index_of(*k));
        assert_eq!(in_one, [keys[0]]);
        assert_eq!(registry.live_keys_in(2), [keys[1]]);
        assert!(registry.live_keys_in(3).is_empty());
    }

    #[test]
    fn forget_tab_drops_one_binding_and_keeps_the_entry() {
        let mut registry = loaded(&["a", "b"]);
        let (ka, kb) = (key(&registry, 0), key(&registry, 1));
        registry.bind_live(ka, 1, tab(1));
        registry.bind_live(kb, 1, tab(2));
        registry.forget_tab(1, tab(1));
        assert_eq!(registry.live_of(ka), None);
        assert_eq!(registry.live_of(kb), Some((1, tab(2))));
        assert_eq!(tags(&registry), ["a", "b"]);
    }

    // --- insert_for_pin (ADR-0086 §2) ---

    /// Two windows: W1 holds `a`, `c`; W2 holds `b`, `d`.
    /// List: a b c d.
    fn two_windows() -> PinnedRegistry<Win> {
        let mut registry = loaded(&["a", "b", "c", "d"]);
        for (index, (window, tab_n)) in [(1, 1), (2, 1), (1, 2), (2, 2)].into_iter().enumerate() {
            let k = key(&registry, index);
            registry.bind_live(k, window, tab(tab_n));
        }
        registry
    }

    #[test]
    fn pin_goes_right_after_the_left_neighbour_in_the_zone() {
        let mut registry = two_windows();
        let neighbour = key(&registry, 0); // `a`, the first of W1's zone
        let new_key = registry.insert_for_pin(1, tab(9), saved("x"), Some(neighbour));
        assert_eq!(tags(&registry), ["a", "x", "b", "c", "d"]);
        assert_eq!(registry.live_of(new_key), Some((1, tab(9))));
        assert_eq!(registry.index_of(new_key), Some(1));
    }

    #[test]
    fn pin_after_the_last_neighbour_of_a_window_is_still_right_after_it() {
        let mut registry = two_windows();
        let neighbour = key(&registry, 2); // `c`, the last of W1's zone
        registry.insert_for_pin(1, tab(9), saved("x"), Some(neighbour));
        assert_eq!(tags(&registry), ["a", "b", "c", "x", "d"]);
    }

    #[test]
    fn pin_without_a_left_neighbour_goes_before_the_first_entry_of_the_window() {
        let mut registry = two_windows();
        registry.insert_for_pin(2, tab(9), saved("x"), None);
        assert_eq!(
            tags(&registry),
            ["a", "x", "b", "c", "d"],
            "before `b`, W2's first"
        );

        let mut registry = two_windows();
        registry.insert_for_pin(1, tab(9), saved("y"), None);
        assert_eq!(
            tags(&registry),
            ["y", "a", "b", "c", "d"],
            "before `a`, W1's first"
        );
    }

    #[test]
    fn pin_in_a_window_with_no_entries_goes_to_the_end() {
        let mut registry = two_windows();
        registry.insert_for_pin(3, tab(1), saved("x"), None);
        assert_eq!(tags(&registry), ["a", "b", "c", "d", "x"]);
    }

    #[test]
    fn pin_in_an_empty_list_is_the_first_entry() {
        let (mut registry, _) = PinnedRegistry::<Win>::from_load(PinnedLoadOutcome::Missing, None);
        let k = registry.insert_for_pin(1, tab(1), saved("x"), None);
        assert_eq!(tags(&registry), ["x"]);
        assert_eq!(registry.key_of(1, tab(1)), Some(k));
    }

    #[test]
    fn a_neighbour_that_left_the_list_counts_as_none() {
        let mut registry = two_windows();
        let gone = key(&registry, 0);
        registry.remove(gone);
        registry.insert_for_pin(1, tab(9), saved("x"), Some(gone));
        // W1's first live entry is now `c`.
        assert_eq!(tags(&registry), ["b", "x", "c", "d"]);
    }

    #[test]
    fn pinning_never_changes_the_keys_of_the_others() {
        let mut registry = two_windows();
        let before: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();
        registry.insert_for_pin(1, tab(9), saved("x"), Some(before[0]));
        let after: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();
        assert_eq!([after[0], after[2], after[3], after[4]], before[..]);
    }

    // --- relocate (ADR-0090 §5) ---

    #[test]
    fn relocating_without_changing_position_reports_no_change() {
        let mut registry = two_windows(); // a(W1) b(W2) c(W1) d(W2)
        let (kc, kd) = (key(&registry, 2), key(&registry, 3));
        // `d` goes to W1, right after `c`: where it already is.
        assert!(!registry.relocate(kd, 1, tab(7), Some(kc)));
        assert_eq!(tags(&registry), ["a", "b", "c", "d"]);
        assert_eq!(registry.live_of(kd), Some((1, tab(7))));

        // `a` goes to W2 with no neighbour: before W2's first entry (`b`),
        // which is right after the position it leaves.
        let mut registry = two_windows();
        let ka = key(&registry, 0);
        assert!(!registry.relocate(ka, 2, tab(8), None));
        assert_eq!(tags(&registry), ["a", "b", "c", "d"]);
        assert_eq!(registry.live_of(ka), Some((2, tab(8))));
    }

    #[test]
    fn relocating_before_another_entry_changes_the_order_and_keeps_identity() {
        let mut registry = two_windows();
        let keys: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();
        // `b` (W2) arrives in W1 right after `c`.
        assert!(registry.relocate(keys[1], 1, tab(9), Some(keys[2])));
        assert_eq!(tags(&registry), ["a", "c", "b", "d"]);
        assert_eq!(registry.live_of(keys[1]), Some((1, tab(9))));
        assert_eq!(registry.key_of(1, tab(9)), Some(keys[1]));
        assert_eq!(registry.key_of(2, tab(1)), None, "the old binding is gone");
        let after: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();
        assert_eq!(after, [keys[0], keys[2], keys[1], keys[3]]);

        // With no neighbour, before the first entry live in W1 (`a`).
        let mut registry = two_windows();
        let kb = key(&registry, 1);
        assert!(registry.relocate(kb, 1, tab(9), None));
        assert_eq!(tags(&registry), ["b", "a", "c", "d"]);
    }

    #[test]
    fn relocating_keeps_the_snapshot() {
        let mut registry = two_windows();
        let kb = key(&registry, 1);
        let before = registry.entries()[1].saved.clone();
        registry.relocate(kb, 1, tab(9), Some(key(&registry, 2)));
        let at = registry.index_of(kb).unwrap();
        assert_eq!(registry.entries()[at].saved, before);
    }

    #[test]
    fn relocating_to_a_window_with_no_entries_goes_to_the_end() {
        let mut registry = two_windows();
        let ka = key(&registry, 0);
        assert!(registry.relocate(ka, 3, tab(1), None));
        assert_eq!(tags(&registry), ["b", "c", "d", "a"]);
        assert_eq!(registry.live_of(ka), Some((3, tab(1))));
    }

    #[test]
    fn relocating_the_last_entry_to_an_empty_window_changes_nothing() {
        let mut registry = two_windows();
        let kd = key(&registry, 3);
        assert!(!registry.relocate(kd, 3, tab(1), None));
        assert_eq!(tags(&registry), ["a", "b", "c", "d"]);
    }

    #[test]
    fn relocating_an_entry_that_left_the_list_does_nothing() {
        let mut registry = two_windows();
        let gone = key(&registry, 0);
        registry.remove(gone);
        assert!(!registry.relocate(gone, 2, tab(5), None));
        assert_eq!(tags(&registry), ["b", "c", "d"]);
        assert_eq!(registry.live_of(gone), None);
    }

    #[test]
    fn a_neighbour_that_left_the_list_counts_as_none_when_relocating() {
        let mut registry = two_windows();
        let (ka, kd) = (key(&registry, 0), key(&registry, 3));
        registry.remove(ka);
        // List: b(W2) c(W1) d(W2); `d` goes to W1 with a stale neighbour:
        // right before W1's first live entry (`c`).
        assert!(registry.relocate(kd, 1, tab(9), Some(ka)));
        assert_eq!(tags(&registry), ["b", "d", "c"]);
    }

    // --- reorder_window ---

    #[test]
    fn reorder_swaps_among_the_positions_the_window_already_holds() {
        let mut registry = two_windows(); // a(W1) b(W2) c(W1) d(W2)
        // W1's zone becomes [tab 2 (`c`), tab 1 (`a`)].
        registry.reorder_window(1, &[tab(2), tab(1)]);
        assert_eq!(
            tags(&registry),
            ["c", "b", "a", "d"],
            "W2's entries did not move"
        );
    }

    #[test]
    fn reorder_of_the_other_window_leaves_the_first_alone() {
        let mut registry = two_windows();
        registry.reorder_window(2, &[tab(2), tab(1)]);
        assert_eq!(tags(&registry), ["a", "d", "c", "b"]);
    }

    #[test]
    fn reorder_with_the_same_order_changes_nothing_and_keeps_bindings() {
        let mut registry = two_windows();
        let before: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();
        registry.reorder_window(1, &[tab(1), tab(2)]);
        assert_eq!(tags(&registry), ["a", "b", "c", "d"]);
        let after: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();
        assert_eq!(before, after);
        assert_eq!(registry.key_of(1, tab(2)), Some(before[2]));
    }

    #[test]
    fn reorder_of_three_moves_the_last_to_the_front() {
        let mut registry = loaded(&["a", "b", "c"]);
        for i in 0..3 {
            let k = key(&registry, i);
            registry.bind_live(k, 1, tab(i as u32 + 1));
        }
        registry.reorder_window(1, &[tab(3), tab(1), tab(2)]);
        assert_eq!(tags(&registry), ["c", "a", "b"]);
    }

    // --- remove, to_file ---

    #[test]
    fn remove_drops_the_entry_and_its_binding_and_shifts_the_indexes() {
        let mut registry = two_windows();
        let (kb, kc) = (key(&registry, 1), key(&registry, 2));
        registry.remove(kb);
        assert_eq!(tags(&registry), ["a", "c", "d"]);
        assert_eq!(registry.live_of(kb), None);
        assert_eq!(registry.index_of(kb), None);
        assert_eq!(registry.index_of(kc), Some(1));
    }

    #[test]
    fn to_file_follows_the_order_of_the_entries() {
        let mut registry = two_windows();
        registry.reorder_window(1, &[tab(2), tab(1)]);
        let file = registry.to_file();
        assert_eq!(file.schema_version, porecatu_session::PINNED_SCHEMA_VERSION);
        let titles: Vec<&str> = file
            .tabs
            .iter()
            .map(|t| t.custom_title.as_deref().unwrap())
            .collect();
        assert_eq!(titles, ["c", "b", "a", "d"]);
    }

    #[test]
    fn save_writes_the_list_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pinned.json");
        let file = PinnedFileV1 {
            tabs: vec![saved("a"), saved("b")],
            ..PinnedFileV1::default()
        };
        let (registry, _) =
            PinnedRegistry::<Win>::from_load(PinnedLoadOutcome::Loaded(file), Some(path.clone()));
        registry.save().unwrap();
        match porecatu_session::load_pinned_from(&path) {
            PinnedLoadOutcome::Loaded(read) => {
                let titles: Vec<_> = read
                    .tabs
                    .iter()
                    .map(|t| t.custom_title.clone().unwrap())
                    .collect();
                assert_eq!(titles, ["a", "b"]);
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }

    #[test]
    fn save_without_a_path_is_an_error() {
        let (registry, _) = PinnedRegistry::<Win>::from_load(PinnedLoadOutcome::Missing, None);
        assert!(matches!(registry.save(), Err(PinnedSaveError::NoPath)));
    }

    #[test]
    fn a_read_only_registry_refuses_to_save_even_with_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pinned.json");
        let (registry, _) = PinnedRegistry::<Win>::from_load(
            PinnedLoadOutcome::NewerSchema {
                found: 9,
                supported: 1,
            },
            Some(path.clone()),
        );
        assert!(matches!(registry.save(), Err(PinnedSaveError::ReadOnly)));
        assert!(!path.exists(), "a newer file is never touched");
    }

    // --- exit_disposition (RF-1.3, RF-21.24) ---

    #[test]
    fn a_zero_exit_closes_the_pane_or_the_tab_of_an_ordinary_tab() {
        assert_eq!(
            exit_disposition(true, false, false),
            ExitDisposition::ClosePane
        );
        assert_eq!(
            exit_disposition(true, true, false),
            ExitDisposition::CloseTab
        );
    }

    #[test]
    fn a_zero_exit_of_the_last_pane_of_a_pinned_tab_keeps_it_exited() {
        assert_eq!(
            exit_disposition(true, true, true),
            ExitDisposition::KeepExited,
            "a typed `exit` must not cost the list"
        );
    }

    #[test]
    fn a_zero_exit_of_a_pane_that_is_not_the_last_closes_it_even_when_pinned() {
        assert_eq!(
            exit_disposition(true, false, true),
            ExitDisposition::ClosePane
        );
    }

    #[test]
    fn a_failed_exit_always_keeps_the_pane_exited() {
        for is_last in [false, true] {
            for pinned in [false, true] {
                assert_eq!(
                    exit_disposition(false, is_last, pinned),
                    ExitDisposition::KeepExited
                );
            }
        }
    }

    // --- close_ask (RF-21.23) ---

    #[test]
    fn closing_a_pinned_tab_always_asks_with_or_without_a_process() {
        assert_eq!(
            close_ask(true, false),
            CloseAsk::Pinned {
                active_process: false
            }
        );
        assert_eq!(
            close_ask(true, true),
            CloseAsk::Pinned {
                active_process: true
            }
        );
    }

    #[test]
    fn an_ordinary_tab_asks_only_with_an_active_process() {
        assert_eq!(close_ask(false, false), CloseAsk::Direct);
        assert_eq!(close_ask(false, true), CloseAsk::ActiveProcess);
    }

    // --- zone geometry helpers ---

    #[test]
    fn the_left_neighbour_is_the_zone_tab_before_the_position() {
        let zone = [tab(1), tab(2), tab(3)];
        assert_eq!(left_neighbor_in_zone(&zone, 0), None);
        assert_eq!(left_neighbor_in_zone(&zone, 1), Some(tab(1)));
        assert_eq!(left_neighbor_in_zone(&zone, 3), Some(tab(3)));
        assert_eq!(
            left_neighbor_in_zone(&zone, 99),
            Some(tab(3)),
            "clamped like pin_tab"
        );
        assert_eq!(left_neighbor_in_zone(&[], 0), None);
    }

    #[test]
    fn the_keyboard_reorder_stops_at_the_border_of_the_zone() {
        let zone = [tab(1), tab(2), tab(3)];
        assert_eq!(zone_move_target(&zone, tab(2), -1), Some(0));
        assert_eq!(zone_move_target(&zone, tab(2), 1), Some(2));
        assert_eq!(zone_move_target(&zone, tab(1), -1), None, "first stays");
        assert_eq!(zone_move_target(&zone, tab(3), 1), None, "last stays");
        assert_eq!(zone_move_target(&zone, tab(9), 1), None, "not in the zone");
    }

    // --- the gestures, workspace and registry in step ---

    /// Mirrors what `App::pin_tab_gesture`/`unpin_tab_gesture` do, with the
    /// same helpers, on a real `Workspace`.
    fn pin(
        ws: &mut porecatu_core::Workspace,
        registry: &mut PinnedRegistry<Win>,
        id: TabId,
        pos: Option<usize>,
    ) {
        let pos = pos.unwrap_or(ws.pinned().len());
        let neighbour = left_neighbor_in_zone(ws.pinned(), pos).and_then(|t| registry.key_of(1, t));
        assert!(ws.pin_tab(id, pos));
        registry.insert_for_pin(1, id, saved(&format!("t{}", id.get())), neighbour);
    }

    fn unpin(ws: &mut porecatu_core::Workspace, registry: &mut PinnedRegistry<Win>, id: TabId) {
        let key = registry.key_of(1, id).expect("pinned tab has an entry");
        assert!(ws.unpin_tab(id, porecatu_core::UnpinTarget::TrackStart));
        registry.remove(key);
    }

    #[test]
    fn pin_then_unpin_keeps_workspace_and_registry_in_step() {
        let mut ws = porecatu_core::Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let (mut registry, _) = PinnedRegistry::<Win>::from_load(PinnedLoadOutcome::Missing, None);

        pin(&mut ws, &mut registry, b, None);
        pin(&mut ws, &mut registry, c, None);
        assert_eq!(ws.pinned(), [b, c]);
        assert_eq!(ws.track_order().collect::<Vec<_>>(), [a]);
        assert_eq!(
            tags(&registry),
            [format!("t{}", b.get()), format!("t{}", c.get())]
        );

        // Unpinning goes to the start of the track and out of the list.
        unpin(&mut ws, &mut registry, b);
        assert_eq!(ws.pinned(), [c]);
        assert_eq!(ws.track_order().collect::<Vec<_>>(), [b, a]);
        assert_eq!(tags(&registry), [format!("t{}", c.get())]);
        assert_eq!(registry.key_of(1, b), None);
    }

    #[test]
    fn pinning_at_a_position_lands_there_in_the_zone_and_in_the_list() {
        let mut ws = porecatu_core::Workspace::new();
        let ids: Vec<TabId> = (0..4).map(|_| ws.append_tab("zsh", None)).collect();
        let (mut registry, _) = PinnedRegistry::<Win>::from_load(PinnedLoadOutcome::Missing, None);

        pin(&mut ws, &mut registry, ids[0], None);
        pin(&mut ws, &mut registry, ids[1], None);
        // A drop between them (the drag, a later stage, reuses this).
        pin(&mut ws, &mut registry, ids[2], Some(1));
        assert_eq!(ws.pinned(), [ids[0], ids[2], ids[1]]);
        let expected: Vec<String> = [ids[0], ids[2], ids[1]]
            .iter()
            .map(|t| format!("t{}", t.get()))
            .collect();
        assert_eq!(tags(&registry), expected);
    }

    #[test]
    fn the_keyboard_reorder_of_the_zone_reaches_the_list() {
        let mut ws = porecatu_core::Workspace::new();
        let ids: Vec<TabId> = (0..3).map(|_| ws.append_tab("zsh", None)).collect();
        let (mut registry, _) = PinnedRegistry::<Win>::from_load(PinnedLoadOutcome::Missing, None);
        ws.append_tab("zsh", None); // a tab stays on the track
        for &id in &ids {
            pin(&mut ws, &mut registry, id, None);
        }
        assert_eq!(ws.pinned(), [ids[0], ids[1], ids[2]]);

        let target = zone_move_target(ws.pinned(), ids[2], -1).unwrap();
        assert!(ws.move_pinned(ids[2], target));
        registry.reorder_window(1, ws.pinned());

        assert_eq!(ws.pinned(), [ids[0], ids[2], ids[1]]);
        let expected: Vec<String> = [ids[0], ids[2], ids[1]]
            .iter()
            .map(|t| format!("t{}", t.get()))
            .collect();
        assert_eq!(tags(&registry), expected);
    }

    /// RF-21.25: closing the last ordinary tab of a window that still has
    /// pinned ones leaves a tab active, so the caller's `window_empty` is
    /// false and the window stays.
    #[test]
    fn closing_the_last_track_tab_with_a_pinned_one_alive_does_not_empty_the_window() {
        let mut ws = porecatu_core::Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let (mut registry, _) = PinnedRegistry::<Win>::from_load(PinnedLoadOutcome::Missing, None);
        pin(&mut ws, &mut registry, b, None);
        ws.activate_tab(a);

        let active = ws.close_tab(a);

        assert_eq!(active, Some(b), "the focus falls on the last pinned tab");
        assert!(ws.active_tab().is_some());
        assert_eq!(ws.track_order().count(), 0);
    }

    /// RF-21.22: closing a pinned tab takes it out of the list; the others
    /// keep their keys and their order.
    #[test]
    fn closing_a_pinned_tab_removes_only_its_entry() {
        let mut ws = porecatu_core::Workspace::new();
        let ids: Vec<TabId> = (0..3).map(|_| ws.append_tab("zsh", None)).collect();
        let (mut registry, _) = PinnedRegistry::<Win>::from_load(PinnedLoadOutcome::Missing, None);
        ws.append_tab("zsh", None);
        for &id in &ids {
            pin(&mut ws, &mut registry, id, None);
        }
        let keys: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();

        let key = registry.key_of(1, ids[1]).unwrap();
        ws.close_tab(ids[1]);
        registry.remove(key);

        assert_eq!(ws.pinned(), [ids[0], ids[2]]);
        let remaining: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();
        assert_eq!(remaining, [keys[0], keys[2]]);
        assert_eq!(registry.key_of(1, ids[0]), Some(keys[0]));
        assert_eq!(registry.key_of(1, ids[2]), Some(keys[2]));
    }
}
