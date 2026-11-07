// SPDX-License-Identifier: GPL-3.0-or-later

//! Extracting tabs and groups from a [`Workspace`] and inserting them into
//! another one (ADR-0090 §1, PRD-022 RF-22.1, RF-22.3, RF-22.5, RF-22.6,
//! RF-22.17), plus [`Workspace::move_tabs`], the same movement inside one
//! workspace that keeps every `TabId` (ADR-0091 §6).
//!
//! Nothing here is new policy. Leaving a workspace follows `close_tab`
//! (RF-2.7, `normalize_groups`, the focus ladder of ADR-0020 §3 revised by
//! ADR-0086 §1) -- the only difference is that the `Tab` is handed back.
//! Arriving follows `insert_tab_with_panes`: ids come from the **destination**
//! counters, and nothing is activated (focus after a transfer is an
//! interaction rule, RF-22.10, owned by the UI).

use std::collections::{HashMap, HashSet};

use super::Workspace;
use crate::group::{Group, GroupKind, GroupMeta};
use crate::id::{GroupId, TabId};
use crate::tab::Tab;

/// A tab outside any workspace (ADR-0090 §1). It has no valid `TabId`: the
/// workspace that receives it assigns one. The `Tab` inside still carries
/// the id it had at the origin, which means nothing here -- the origin id
/// travels only in the map the `take_*` operations return.
#[derive(Debug, Clone, PartialEq)]
pub struct DetachedTab {
    tab: Tab,
}

impl DetachedTab {
    /// The tab as it left the origin: title, pane tree and state. Its
    /// [`Tab::id`] is the **origin** id and is meaningless for the
    /// destination.
    pub const fn tab(&self) -> &Tab {
        &self.tab
    }
}

/// An explicit group outside any workspace (ADR-0090 §1, RF-22.5): name,
/// colour and collapse state exactly as they were before the extraction.
#[derive(Debug, Clone, PartialEq)]
pub struct DetachedGroup {
    meta: GroupMeta,
    tabs: Vec<DetachedTab>,
    last_active: Option<usize>,
}

impl DetachedGroup {
    pub const fn meta(&self) -> &GroupMeta {
        &self.meta
    }

    /// The tabs in the order of the group.
    pub fn tabs(&self) -> &[DetachedTab] {
        &self.tabs
    }

    /// The group's most recently visited tab (ADR-0020 §6), as an index in
    /// [`Self::tabs`] -- a `TabId` would mean nothing at the destination.
    pub const fn last_active(&self) -> Option<usize> {
        self.last_active
    }
}

/// Where a block of tabs lands (ADR-0090 §1). Same shape as the drop the
/// tab bar already resolves (`DragDrop`, ADR-0021 §4, ADR-0087 §5), plus
/// `TrackEnd`. Positions follow the conventions the drag already uses:
///
/// - `IntoGroup { group, pos }`: position among the group's tabs **as they
///   are when the block enters**, saturating at the end;
/// - `NewRun { group_index }`: index in the group list, saturating;
/// - `Pinned { pos }`: position in the pinned zone, saturating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertAt {
    /// The block enters `group` contiguously from `pos`. An unknown group
    /// behaves as [`InsertAt::TrackEnd`].
    IntoGroup { group: GroupId, pos: usize },
    /// A new implicit run holding the block, at `group_index` of the group
    /// list. It merges with an adjacent implicit run.
    NewRun { group_index: usize },
    /// The block enters the pinned zone at `pos..pos + n`.
    Pinned { pos: usize },
    /// Loose tabs at the end of the track (RF-22.23): a new implicit run
    /// at the end of the group list, merged with the last one if implicit.
    TrackEnd,
}

impl Workspace {
    /// `true` when there is no tab at all: nothing on the track and nothing
    /// in the pinned zone. A window in this state has nothing left to show
    /// (RF-22.19).
    pub fn is_empty(&self) -> bool {
        self.pinned.is_empty() && self.groups.iter().all(Group::is_empty)
    }

    /// ADR-0090 §1 (RF-22.1, RF-22.17): takes the **track** tabs listed in
    /// `ids` out of the workspace and returns them with their origin ids,
    /// in the origin's visual order ([`Workspace::track_order`]) whatever
    /// the order of `ids`. Unknown ids and **pinned** tabs are ignored --
    /// [`Workspace::take_pinned`] is the way out for those.
    ///
    /// Leaving works as `close_tab`: a group that empties goes away
    /// (RF-2.7), neighbouring implicit runs merge, and when the active tab
    /// leaves the focus ladder picks the new one (falling to the last
    /// pinned tab at the end of it). With several tabs the active one is
    /// removed **last**, so the ladder runs once, over the tabs that stay.
    pub fn take_tabs(&mut self, ids: &[TabId]) -> Vec<(TabId, DetachedTab)> {
        let wanted: HashSet<TabId> = ids.iter().copied().collect();
        let ordered: Vec<TabId> = self.track_order().filter(|t| wanted.contains(t)).collect();
        let active = self.active_tab;

        let mut taken: HashMap<TabId, Tab> = HashMap::with_capacity(ordered.len());
        for &id in ordered.iter().filter(|&&id| Some(id) != active) {
            if let Some(tab) = self.remove_tab(id) {
                taken.insert(id, tab);
            }
        }
        if let Some(active) = active.filter(|a| ordered.contains(a))
            && let Some(tab) = self.remove_tab(active)
        {
            taken.insert(active, tab);
        }

        ordered
            .into_iter()
            .filter_map(|id| taken.remove(&id).map(|tab| (id, DetachedTab { tab })))
            .collect()
    }

    /// ADR-0090 §1 (RF-22.1): takes a **pinned** tab out of the zone, with
    /// the focus rule `close_tab` has for pinned tabs. `None` -- nothing
    /// changed -- when `id` is not pinned.
    pub fn take_pinned(&mut self, id: TabId) -> Option<DetachedTab> {
        if !self.is_pinned(id) {
            return None;
        }
        self.remove_tab(id).map(|tab| DetachedTab { tab })
    }

    /// ADR-0090 §1 (RF-22.3, RF-22.5): takes a whole **explicit** group out
    /// of the workspace. Implicit and unknown groups give `None`, with
    /// nothing changed.
    ///
    /// The [`DetachedGroup`] has the name, colour and collapse state from
    /// **before** the extraction, the tabs in the group's order and
    /// `last_active` as an index; the origin ids come back in the same
    /// order. When the active tab was in the group, the new active tab
    /// comes from the focus ladder starting at the group's position, as if
    /// its tabs had been closed: the ladder never expands, nor activates a
    /// tab of, the group that is leaving. Implicit runs left adjacent
    /// merge.
    pub fn take_group(&mut self, id: GroupId) -> Option<(DetachedGroup, Vec<TabId>)> {
        let group_index = self.group_index(id)?;
        let GroupKind::Explicit(meta) = self.groups[group_index].kind() else {
            return None;
        };
        let meta = meta.clone();
        let last_active = self.groups[group_index].last_active();

        let members: Vec<(TabId, Tab)> = self.groups[group_index]
            .tabs()
            .to_vec()
            .into_iter()
            .filter_map(|tab_id| {
                let index = self.tabs.iter().position(|t| t.id() == tab_id)?;
                Some((tab_id, self.tabs.remove(index)))
            })
            .collect();
        let old_ids: Vec<TabId> = members.iter().map(|(id, _)| *id).collect();
        let last_active = last_active.and_then(|t| old_ids.iter().position(|&id| id == t));
        let active_inside = self.active_tab.is_some_and(|a| old_ids.contains(&a));

        // Empty the group but keep its slot while the ladder runs, so the
        // ladder starts from the group's position exactly as in
        // `close_tab` (where the emptied group is still in the list).
        self.groups[group_index].retain_tabs(|_| false);
        if active_inside {
            let next = self
                .focus_ladder(group_index, None, None)
                .or_else(|| self.collapsed_fallback(group_index))
                .or_else(|| self.pinned.last().copied());
            match next {
                Some(next) => {
                    self.activate_tab(next);
                }
                None => self.active_tab = None,
            }
        }
        self.groups.remove(group_index);
        self.normalize_groups();

        let detached = DetachedGroup {
            meta,
            tabs: members
                .into_iter()
                .map(|(_, tab)| DetachedTab { tab })
                .collect(),
            last_active,
        };
        Some((detached, old_ids))
    }

    /// ADR-0090 §1 (RF-22.1, RF-22.6): inserts `tabs` as one contiguous
    /// block at `at`, giving each a new `TabId` from this workspace's
    /// counter, in the order of `tabs`. Returns the new ids in that order.
    /// **Activates nothing**: focus after a transfer is the UI's (RF-22.10).
    /// A collapsed destination group takes the block and stays collapsed.
    /// See [`InsertAt`] for the position conventions, read against the
    /// workspace as it is now.
    pub fn insert_detached_tabs(&mut self, tabs: Vec<DetachedTab>, at: InsertAt) -> Vec<TabId> {
        let ids: Vec<TabId> = tabs
            .into_iter()
            .map(|detached| self.adopt_tab(detached.tab))
            .collect();
        self.place_block(&ids, at);
        ids
    }

    /// ADR-0090 §1 (RF-22.5, RF-22.6): inserts a detached group at `pos` of
    /// the group list (saturating at the end), with a new `GroupId` and new
    /// `TabId`s from this workspace's counters, and the same name, colour
    /// and collapse state. `last_active` is translated to the new id. It
    /// **never** merges with a group of the same name, and activates
    /// nothing. Returns the new group id and the new tab ids in the group's
    /// order.
    pub fn insert_detached_group(
        &mut self,
        group: DetachedGroup,
        pos: usize,
    ) -> (GroupId, Vec<TabId>) {
        let DetachedGroup {
            meta,
            tabs,
            last_active,
        } = group;
        let ids: Vec<TabId> = tabs
            .into_iter()
            .map(|detached| self.adopt_tab(detached.tab))
            .collect();

        let group_id = self.fresh_group_id();
        let mut new_group = Group::from_meta(group_id, meta);
        for (offset, &id) in ids.iter().enumerate() {
            new_group.insert(offset, id);
        }
        if let Some(&id) = last_active.and_then(|index| ids.get(index)) {
            new_group.set_last_active(id);
        }
        let at = pos.min(self.groups.len());
        self.groups.insert(at, new_group);
        self.normalize_groups();
        (group_id, ids)
    }

    /// ADR-0091 §6: moves the **track** tabs in `ids` as one contiguous
    /// block to `at`, inside this workspace, **keeping their `TabId`s**.
    /// Pinned and unknown ids are ignored; `false` -- nothing changed --
    /// when no valid tab is left.
    ///
    /// It is "take every tab, then insert the block": the block keeps the
    /// origin's visual order ([`Workspace::track_order`]) whatever the order
    /// of `ids`, a group that empties goes away (RF-2.7) and neighbouring
    /// implicit runs merge, exactly as in [`Workspace::take_tabs`]. Unlike
    /// it, the **active tab never changes**, even when it is in the block.
    /// If the active tab moves into a collapsed group, the group expands
    /// (RF-2.17, as `unpin_tab` does).
    ///
    /// **Position convention:** `at` is read against the workspace *after*
    /// the block was removed -- positions and indices count only the tabs
    /// and groups that stay. This is what the UI computes the drop target
    /// against (a clone of the workspace without the block's tabs). A
    /// `IntoGroup` whose group disappeared with the removal behaves as
    /// [`InsertAt::TrackEnd`].
    pub fn move_tabs(&mut self, ids: &[TabId], at: InsertAt) -> bool {
        let wanted: HashSet<TabId> = ids.iter().copied().collect();
        let ordered: Vec<TabId> = self.track_order().filter(|t| wanted.contains(t)).collect();
        if ordered.is_empty() {
            return false;
        }

        let mut stripped: Vec<Tab> = Vec::with_capacity(ordered.len());
        for &id in &ordered {
            if let Some((tab, _, _)) = self.strip_track_tab(id) {
                stripped.push(tab);
            }
        }
        self.normalize_groups();
        self.tabs.extend(stripped);
        self.place_block(&ordered, at);

        if let Some(active) = self.active_tab.filter(|a| ordered.contains(a))
            && let Some(group) = self.group_of_tab(active).and_then(|g| self.group_mut(g))
        {
            group.set_last_active(active);
            group.set_collapsed(false);
        }
        true
    }

    /// Gives `tab` a new id from this workspace's counter and stores it. It
    /// is not placed anywhere yet: `place_block` does that.
    fn adopt_tab(&mut self, tab: Tab) -> TabId {
        let id = TabId::new(self.next_tab_id);
        self.next_tab_id += 1;
        self.tabs.push(tab.with_id(id));
        id
    }

    /// Puts `ids` -- tabs already in `tabs` but in no place -- as one block
    /// at `at` (see [`InsertAt`]). Shared by `insert_detached_tabs` and
    /// `move_tabs`. Empty `ids` changes nothing.
    fn place_block(&mut self, ids: &[TabId], at: InsertAt) {
        if ids.is_empty() {
            return;
        }
        match at {
            InsertAt::IntoGroup { group, pos } => {
                if let Some(index) = self.group_index(group) {
                    let target = &mut self.groups[index];
                    for (offset, &id) in ids.iter().enumerate() {
                        // Saturates at the end, so an out-of-range `pos`
                        // appends the block in order.
                        target.insert(pos.saturating_add(offset), id);
                    }
                } else {
                    self.insert_run(ids, self.groups.len());
                }
            }
            InsertAt::NewRun { group_index } => self.insert_run(ids, group_index),
            InsertAt::Pinned { pos } => {
                let pos = pos.min(self.pinned.len());
                self.pinned.splice(pos..pos, ids.iter().copied());
            }
            InsertAt::TrackEnd => self.insert_run(ids, self.groups.len()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::assert_invariants;
    use super::*;
    use crate::group::GroupColor;
    use crate::pane::SplitAxis;

    fn track(ws: &Workspace) -> Vec<TabId> {
        ws.track_order().collect()
    }

    fn visual(ws: &Workspace) -> Vec<TabId> {
        ws.visual_order().collect()
    }

    fn tabs_of(ws: &Workspace, group: GroupId) -> Vec<TabId> {
        ws.group(group).unwrap().tabs().to_vec()
    }

    fn ids(n: &[u32]) -> Vec<TabId> {
        n.iter().map(|&i| TabId::new(i)).collect()
    }

    /// `n` tabs (`TabId(0)..`) in one implicit run; the last is active.
    fn workspace_of(n: u32) -> (Workspace, Vec<TabId>) {
        let mut ws = Workspace::new();
        let tabs = (0..n).map(|_| ws.append_tab("zsh", None)).collect();
        (ws, tabs)
    }

    /// Eight tabs: `imp[0] G1[1,2] imp[3] G2[4,5] G3[6,7]`, active is 7.
    fn eight() -> (Workspace, GroupId, GroupId, GroupId) {
        let (mut ws, t) = workspace_of(8);
        let g1 = ws.group_tabs(&[t[1], t[2]], "g1", GroupColor::Red).unwrap();
        let g2 = ws
            .group_tabs(&[t[4], t[5]], "g2", GroupColor::Blue)
            .unwrap();
        let g3 = ws
            .group_tabs(&[t[6], t[7]], "g3", GroupColor::Green)
            .unwrap();
        assert_invariants(&ws);
        assert_eq!(ws.groups().len(), 5);
        (ws, g1, g2, g3)
    }

    fn detached(ws: &mut Workspace, id: TabId) -> DetachedTab {
        let mut taken = ws.take_tabs(&[id]);
        assert_eq!(taken.len(), 1);
        taken.remove(0).1
    }

    // ---------------------------------------------------------------
    // take_tabs
    // ---------------------------------------------------------------

    #[test]
    fn take_from_the_middle_of_an_explicit_group() {
        let (mut origin, t) = workspace_of(5);
        let g = origin
            .group_tabs(&[t[1], t[2], t[3]], "g", GroupColor::Red)
            .unwrap();
        let taken = origin.take_tabs(&[t[2]]);

        assert_eq!(taken.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [t[2]]);
        assert_eq!(track(&origin), [t[0], t[1], t[3], t[4]]);
        assert_eq!(tabs_of(&origin, g), [t[1], t[3]]);
        assert_eq!(origin.groups().len(), 3);
        assert_eq!(origin.active_tab(), Some(t[4]));
        assert!(origin.tab(t[2]).is_none());
        assert_invariants(&origin);

        let mut dest = Workspace::new();
        let x = dest.append_tab("zsh", None);
        let y = dest.append_tab("zsh", None);
        let dg = dest.group_tabs(&[x, y], "d", GroupColor::Blue).unwrap();
        let new = dest.insert_detached_tabs(
            taken.into_iter().map(|(_, t)| t).collect(),
            InsertAt::IntoGroup { group: dg, pos: 1 },
        );
        assert_eq!(new, ids(&[2]));
        assert_eq!(track(&dest), [x, new[0], y]);
        assert_eq!(tabs_of(&dest, dg), [x, new[0], y]);
        assert_eq!(dest.active_tab(), Some(y));
        assert_invariants(&dest);
    }

    #[test]
    fn taking_the_last_tab_of_a_group_removes_it_and_merges_the_runs() {
        let (mut origin, t) = workspace_of(3);
        origin.group_tabs(&[t[1]], "g", GroupColor::Red).unwrap();
        assert_eq!(origin.groups().len(), 3);

        let taken = origin.take_tabs(&[t[1]]);
        assert_eq!(taken.len(), 1);
        assert_eq!(origin.groups().len(), 1);
        assert!(origin.groups()[0].is_implicit());
        assert_eq!(track(&origin), [t[0], t[2]]);
        assert_eq!(origin.active_tab(), Some(t[2]));
        assert_invariants(&origin);
    }

    #[test]
    fn taking_a_selection_spread_over_three_groups_returns_visual_order() {
        let (mut origin, t) = workspace_of(7);
        let g1 = origin
            .group_tabs(&[t[1], t[2]], "g1", GroupColor::Red)
            .unwrap();
        let g2 = origin
            .group_tabs(&[t[3], t[4]], "g2", GroupColor::Blue)
            .unwrap();
        let g3 = origin
            .group_tabs(&[t[5], t[6]], "g3", GroupColor::Green)
            .unwrap();

        // Out of order on purpose; plus an unknown id.
        let taken = origin.take_tabs(&[t[6], TabId::new(99), t[2], t[0]]);
        assert_eq!(
            taken.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [t[0], t[2], t[6]]
        );
        assert_eq!(track(&origin), [t[1], t[3], t[4], t[5]]);
        assert_eq!(tabs_of(&origin, g1), [t[1]]);
        assert_eq!(tabs_of(&origin, g2), [t[3], t[4]]);
        assert_eq!(tabs_of(&origin, g3), [t[5]]);
        assert_eq!(origin.groups().len(), 3);
        // The active tab (6) left: the ladder ran over the tabs that stay.
        assert_eq!(origin.active_tab(), Some(t[5]));
        assert_invariants(&origin);

        let mut dest = Workspace::new();
        let first = dest.append_tab("zsh", None);
        let new = dest.insert_detached_tabs(
            taken.into_iter().map(|(_, t)| t).collect(),
            InsertAt::TrackEnd,
        );
        assert_eq!(new, ids(&[1, 2, 3]));
        assert_eq!(track(&dest), [first, new[0], new[1], new[2]]);
        assert_eq!(dest.groups().len(), 1);
        assert_invariants(&dest);
    }

    #[test]
    fn taking_the_active_tab_picks_the_same_one_as_close_tab() {
        // Every track tab as the active one, with and without collapsed
        // groups and with and without a pinned tab.
        for with_pinned in [false, true] {
            for collapsed in [false, true] {
                let (mut base, ..) = eight();
                let g: Vec<GroupId> = base
                    .groups()
                    .iter()
                    .filter(|g| g.is_explicit())
                    .map(Group::id)
                    .collect();
                if with_pinned {
                    assert!(base.pin_tab(TabId::new(3), 0));
                }
                if collapsed {
                    for id in &g {
                        base.collapse_group(*id, true);
                    }
                }
                for t in track(&base) {
                    let mut closed = base.clone();
                    closed.activate_tab(t);
                    let mut taken = closed.clone();
                    closed.close_tab(t);
                    let result = taken.take_tabs(&[t]);
                    assert_eq!(result.len(), 1);
                    assert_eq!(
                        taken.active_tab(),
                        closed.active_tab(),
                        "active {t:?} pinned={with_pinned} collapsed={collapsed}"
                    );
                    assert_eq!(taken, closed, "active {t:?}");
                    assert_invariants(&taken);
                }
            }
        }
    }

    #[test]
    fn taking_the_pinned_tab_picks_the_same_focus_as_close_tab() {
        let (mut base, t) = workspace_of(5);
        base.pin_tab(t[0], 0);
        base.pin_tab(t[1], 1);
        base.pin_tab(t[2], 2);
        for p in base.pinned().to_vec() {
            let mut closed = base.clone();
            closed.activate_tab(p);
            let mut taken = closed.clone();
            closed.close_tab(p);
            assert!(taken.take_pinned(p).is_some());
            assert_eq!(taken, closed, "pinned {p:?}");
            assert_invariants(&taken);
        }
    }

    #[test]
    fn taking_with_a_pinned_tab_in_the_middle_leaves_it_alone() {
        let (mut origin, t) = workspace_of(4);
        origin.pin_tab(t[1], 0);
        let taken = origin.take_tabs(&[t[0], t[1], t[2]]);
        assert_eq!(
            taken.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [t[0], t[2]]
        );
        assert_eq!(origin.pinned(), [t[1]]);
        assert_eq!(visual(&origin), [t[1], t[3]]);
        assert_eq!(origin.active_tab(), Some(t[3]));
        assert_invariants(&origin);
    }

    #[test]
    fn taking_every_track_tab_falls_to_the_last_pinned() {
        let (mut origin, t) = workspace_of(4);
        origin.pin_tab(t[0], 0);
        origin.pin_tab(t[1], 1);
        let taken = origin.take_tabs(&[t[2], t[3]]);
        assert_eq!(taken.len(), 2);
        assert!(origin.groups().is_empty());
        assert_eq!(origin.active_tab(), Some(t[1]));
        assert!(!origin.is_empty());
        assert_invariants(&origin);
    }

    #[test]
    fn taking_unknown_or_nothing_changes_nothing() {
        let (mut origin, t) = workspace_of(3);
        origin.pin_tab(t[0], 0);
        let before = origin.clone();
        assert!(origin.take_tabs(&[]).is_empty());
        assert!(origin.take_tabs(&[TabId::new(42), t[0]]).is_empty());
        assert_eq!(origin, before);
    }

    // ---------------------------------------------------------------
    // take_group
    // ---------------------------------------------------------------

    #[test]
    fn taking_a_group_hands_over_meta_tabs_and_last_active() {
        let (mut origin, t) = workspace_of(5);
        let g = origin
            .group_tabs(&[t[1], t[2], t[3]], "api", GroupColor::Purple)
            .unwrap();
        origin.activate_tab(t[3]);
        origin.activate_tab(t[4]);
        origin.collapse_group(g, true);
        assert_eq!(origin.group(g).unwrap().last_active(), Some(t[3]));

        let (group, old) = origin.take_group(g).unwrap();
        assert_eq!(old, [t[1], t[2], t[3]]);
        assert_eq!(group.meta().name(), "api");
        assert_eq!(group.meta().color(), GroupColor::Purple);
        assert!(group.meta().is_collapsed());
        assert_eq!(group.tabs().len(), 3);
        assert_eq!(group.last_active(), Some(2));
        // The two implicit runs around it merged.
        assert_eq!(origin.groups().len(), 1);
        assert_eq!(track(&origin), [t[0], t[4]]);
        assert_eq!(origin.active_tab(), Some(t[4]));
        assert_invariants(&origin);
    }

    #[test]
    fn taking_a_collapsed_group_between_two_destination_groups_keeps_everything() {
        let (mut origin, t) = workspace_of(4);
        let g = origin
            .group_tabs(&[t[1], t[2]], "moved", GroupColor::Cyan)
            .unwrap();
        origin.activate_tab(t[1]);
        origin.activate_tab(t[3]);
        origin.collapse_group(g, true);
        let (group, _) = origin.take_group(g).unwrap();
        assert_invariants(&origin);

        // Destination `A[0,1] B[2]`, with the same name as the incoming one
        // on `B`.
        let (mut dest, d) = workspace_of(3);
        let a = dest
            .group_tabs(&[d[0], d[1]], "a", GroupColor::Red)
            .unwrap();
        let b = dest.group_tabs(&[d[2]], "moved", GroupColor::Blue).unwrap();
        let active_before = dest.active_tab();

        let (new_group, new_tabs) = dest.insert_detached_group(group, 1);
        assert_eq!(new_tabs, ids(&[3, 4]));
        assert_ne!(new_group, a);
        assert_ne!(new_group, b);
        let order: Vec<GroupId> = dest.groups().iter().map(Group::id).collect();
        assert_eq!(order, [a, new_group, b]);
        assert_eq!(
            dest.groups().len(),
            3,
            "never merges with a same-named group"
        );
        let inserted = dest.group(new_group).unwrap();
        assert_eq!(inserted.name(), Some("moved"));
        assert_eq!(inserted.color(), Some(GroupColor::Cyan));
        assert!(inserted.is_collapsed());
        assert_eq!(inserted.tabs(), new_tabs);
        assert_eq!(inserted.last_active(), Some(new_tabs[0]));
        assert_eq!(dest.group(b).unwrap().name(), Some("moved"));
        assert_eq!(track(&dest), [d[0], d[1], new_tabs[0], new_tabs[1], d[2]]);
        assert_eq!(dest.active_tab(), active_before);
        assert_invariants(&dest);
    }

    #[test]
    fn inserting_a_group_saturates_its_position() {
        let (mut origin, t) = workspace_of(2);
        let g = origin.group_tabs(&[t[1]], "g", GroupColor::Red).unwrap();
        let (group, _) = origin.take_group(g).unwrap();
        let (mut dest, d) = workspace_of(2);
        let (new_group, tabs) = dest.insert_detached_group(group, 99);
        assert_eq!(dest.groups().last().map(Group::id), Some(new_group));
        assert_eq!(track(&dest), [d[0], d[1], tabs[0]]);
        assert_invariants(&dest);
    }

    #[test]
    fn taking_the_active_group_uses_the_ladder_from_its_position() {
        // imp[a] G[b,c] imp[d]: the next group comes first.
        let (mut ws, t) = workspace_of(4);
        let g = ws.group_tabs(&[t[1], t[2]], "g", GroupColor::Red).unwrap();
        ws.activate_tab(t[2]);
        ws.take_group(g).unwrap();
        assert_eq!(ws.active_tab(), Some(t[3]));
        assert_eq!(track(&ws), [t[0], t[3]]);
        assert_eq!(ws.groups().len(), 1);
        assert_invariants(&ws);

        // imp[a] G[b,c]: nothing after, the previous group's last tab.
        let (mut ws, t) = workspace_of(3);
        let g = ws.group_tabs(&[t[1], t[2]], "g", GroupColor::Red).unwrap();
        ws.activate_tab(t[1]);
        ws.take_group(g).unwrap();
        assert_eq!(ws.active_tab(), Some(t[0]));
        assert_invariants(&ws);
    }

    #[test]
    fn taking_the_active_group_expands_a_collapsed_fallback_but_not_itself() {
        let (mut ws, t) = workspace_of(4);
        let g = ws.group_tabs(&[t[0], t[1]], "g", GroupColor::Red).unwrap();
        let h = ws.group_tabs(&[t[2], t[3]], "h", GroupColor::Blue).unwrap();
        ws.activate_tab(t[3]);
        ws.collapse_group(h, true);
        ws.activate_tab(t[1]);
        ws.collapse_group(g, false);
        // Only collapsed group left to fall to is `h`.
        ws.take_group(g).unwrap();
        assert_eq!(ws.active_tab(), Some(t[3]), "h's last visited tab");
        assert!(!ws.group(h).unwrap().is_collapsed());
        assert_invariants(&ws);
    }

    #[test]
    fn taking_the_only_group_leaves_an_empty_workspace() {
        let (mut ws, t) = workspace_of(2);
        let g = ws.group_tabs(&[t[0], t[1]], "g", GroupColor::Red).unwrap();
        ws.take_group(g).unwrap();
        assert!(ws.is_empty());
        assert_eq!(ws.active_tab(), None);
        assert_invariants(&ws);
    }

    #[test]
    fn taking_an_inactive_group_keeps_the_active_tab() {
        let (mut ws, g1, _, _) = eight();
        ws.activate_tab(TabId::new(3));
        ws.take_group(g1).unwrap();
        assert_eq!(ws.active_tab(), Some(TabId::new(3)));
        assert_invariants(&ws);
    }

    #[test]
    fn taking_an_implicit_or_unknown_group_changes_nothing() {
        let (mut ws, _, _, _) = eight();
        let implicit = ws.groups()[0].id();
        assert!(ws.groups()[0].is_implicit());
        let before = ws.clone();
        assert!(ws.take_group(implicit).is_none());
        assert!(ws.take_group(GroupId::new(999)).is_none());
        assert_eq!(ws, before);
    }

    // ---------------------------------------------------------------
    // take_pinned / insert at
    // ---------------------------------------------------------------

    #[test]
    fn take_pinned_and_insert_in_the_middle_of_the_destination_zone() {
        let (mut origin, t) = workspace_of(3);
        origin.pin_tab(t[0], 0);
        origin.pin_tab(t[1], 1);
        let taken = origin.take_pinned(t[0]).unwrap();
        assert_eq!(origin.pinned(), [t[1]]);
        assert_eq!(visual(&origin), [t[1], t[2]]);
        assert!(origin.take_pinned(t[2]).is_none(), "a track tab is refused");
        assert!(origin.take_pinned(t[0]).is_none(), "already gone");
        assert_invariants(&origin);

        let (mut dest, d) = workspace_of(4);
        dest.pin_tab(d[0], 0);
        dest.pin_tab(d[1], 1);
        dest.pin_tab(d[2], 2);
        let new = dest.insert_detached_tabs(vec![taken], InsertAt::Pinned { pos: 1 });
        assert_eq!(new, ids(&[4]));
        assert_eq!(dest.pinned(), [d[0], new[0], d[1], d[2]]);
        assert_eq!(visual(&dest), [d[0], new[0], d[1], d[2], d[3]]);
        assert_invariants(&dest);
    }

    #[test]
    fn inserting_into_a_collapsed_group_keeps_it_collapsed_and_activates_nothing() {
        let (mut dest, d) = workspace_of(4);
        let g = dest
            .group_tabs(&[d[1], d[2]], "g", GroupColor::Red)
            .unwrap();
        dest.collapse_group(g, true);
        let active_before = dest.active_tab();

        let (mut origin, t) = workspace_of(2);
        let taken = origin.take_tabs(&[t[0], t[1]]);
        let new = dest.insert_detached_tabs(
            taken.into_iter().map(|(_, t)| t).collect(),
            InsertAt::IntoGroup { group: g, pos: 1 },
        );
        assert_eq!(tabs_of(&dest, g), [d[1], new[0], new[1], d[2]]);
        assert!(dest.group(g).unwrap().is_collapsed());
        assert_eq!(dest.active_tab(), active_before);
        assert_invariants(&dest);
    }

    #[test]
    fn inserting_into_a_group_saturates_the_position() {
        let (mut dest, d) = workspace_of(3);
        let g = dest
            .group_tabs(&[d[0], d[1]], "g", GroupColor::Red)
            .unwrap();
        let (mut origin, t) = workspace_of(2);
        let taken = origin.take_tabs(&[t[0], t[1]]);
        let new = dest.insert_detached_tabs(
            taken.into_iter().map(|(_, t)| t).collect(),
            InsertAt::IntoGroup { group: g, pos: 99 },
        );
        assert_eq!(tabs_of(&dest, g), [d[0], d[1], new[0], new[1]]);
        assert_invariants(&dest);
    }

    #[test]
    fn inserting_with_a_new_run_places_and_merges() {
        let (mut origin, t) = workspace_of(2);
        let a = detached(&mut origin, t[0]);
        let b = detached(&mut origin, t[1]);

        // `G1[0,1] imp[2] G2[3]`.
        let (mut dest, d) = workspace_of(4);
        dest.group_tabs(&[d[0], d[1]], "g1", GroupColor::Red)
            .unwrap();
        dest.group_tabs(&[d[3]], "g2", GroupColor::Blue).unwrap();

        // Between G1 and the implicit run: merges with it.
        let new = dest.insert_detached_tabs(
            vec![a.clone(), b.clone()],
            InsertAt::NewRun { group_index: 1 },
        );
        assert_eq!(new, ids(&[4, 5]));
        assert_eq!(track(&dest), ids(&[0, 1, 4, 5, 2, 3]));
        assert_eq!(dest.groups().len(), 3);
        assert_invariants(&dest);

        // Past the end saturates: new implicit run after G2.
        let new = dest.insert_detached_tabs(vec![a], InsertAt::NewRun { group_index: 99 });
        assert_eq!(new, ids(&[6]));
        assert_eq!(track(&dest), ids(&[0, 1, 4, 5, 2, 3, 6]));
        assert_eq!(dest.groups().len(), 4);
        assert!(dest.groups().last().unwrap().is_implicit());
        assert_invariants(&dest);
    }

    #[test]
    fn inserting_at_the_track_end_merges_with_a_trailing_implicit_run() {
        let (mut origin, t) = workspace_of(1);
        let a = detached(&mut origin, t[0]);

        let (mut dest, d) = workspace_of(3);
        dest.group_tabs(&[d[0]], "g", GroupColor::Red).unwrap();
        assert_eq!(dest.groups().len(), 2);
        let new = dest.insert_detached_tabs(vec![a.clone()], InsertAt::TrackEnd);
        assert_eq!(track(&dest), [d[0], d[1], d[2], new[0]]);
        assert_eq!(dest.groups().len(), 2, "merged with the trailing run");
        assert_invariants(&dest);

        // After an explicit group: a new run.
        let (mut dest, d) = workspace_of(2);
        dest.group_tabs(&[d[0], d[1]], "g", GroupColor::Red)
            .unwrap();
        let new = dest.insert_detached_tabs(vec![a], InsertAt::TrackEnd);
        assert_eq!(track(&dest), [d[0], d[1], new[0]]);
        assert_eq!(dest.groups().len(), 2);
        assert!(dest.groups()[1].is_implicit());
        assert_invariants(&dest);
    }

    #[test]
    fn inserting_into_an_unknown_group_falls_back_to_the_track_end() {
        let (mut origin, t) = workspace_of(1);
        let a = detached(&mut origin, t[0]);
        let (mut dest, d) = workspace_of(2);
        dest.group_tabs(&[d[0]], "g", GroupColor::Red).unwrap();
        let new = dest.insert_detached_tabs(
            vec![a],
            InsertAt::IntoGroup {
                group: GroupId::new(999),
                pos: 0,
            },
        );
        assert_eq!(track(&dest), [d[0], d[1], new[0]]);
        assert_invariants(&dest);
    }

    #[test]
    fn inserting_nothing_changes_nothing() {
        let (mut dest, _) = workspace_of(2);
        let before = dest.clone();
        for at in [
            InsertAt::TrackEnd,
            InsertAt::NewRun { group_index: 0 },
            InsertAt::Pinned { pos: 0 },
        ] {
            assert!(dest.insert_detached_tabs(Vec::new(), at).is_empty());
        }
        assert_eq!(dest, before);
    }

    #[test]
    fn new_ids_never_collide_with_the_destination() {
        let (mut origin, t) = workspace_of(3);
        let taken = origin.take_tabs(&[t[0], t[1]]);
        let (mut dest, d) = workspace_of(2);
        assert_eq!(d, ids(&[0, 1]));
        let new = dest.insert_detached_tabs(
            taken.into_iter().map(|(_, t)| t).collect(),
            InsertAt::TrackEnd,
        );
        // Origin ids 0 and 1 are taken in the destination: new ids come
        // from its counter.
        assert_eq!(new, ids(&[2, 3]));
        assert_eq!(dest.tab(new[0]).unwrap().id(), new[0]);
        assert_eq!(dest.tab(new[1]).unwrap().id(), new[1]);
        assert_eq!(track(&dest), ids(&[0, 1, 2, 3]));
        assert_invariants(&dest);

        // The counter keeps going: a later tab does not reuse an id.
        let next = dest.append_tab("zsh", None);
        assert_eq!(next, TabId::new(4));
    }

    #[test]
    fn panes_and_title_arrive_intact() {
        let (mut origin, t) = workspace_of(2);
        let tab = origin.tab_mut(t[0]).unwrap();
        tab.set_custom_title(Some("build".to_string()));
        let first = tab.panes().focused_id();
        let second = tab
            .panes_mut()
            .split(first, SplitAxis::Vertical, "bash", None)
            .unwrap();
        assert!(tab.panes_mut().focus(first));
        let leaves = tab.panes().leaves_in_order();
        let panes_before = tab.panes().clone();

        let taken = detached(&mut origin, t[0]);
        assert_eq!(taken.tab().panes(), &panes_before);

        let (mut dest, _) = workspace_of(2);
        let new = dest.insert_detached_tabs(vec![taken], InsertAt::TrackEnd)[0];
        let arrived = dest.tab(new).unwrap();
        assert_eq!(arrived.id(), new);
        assert_eq!(arrived.title(), "build");
        assert!(arrived.has_custom_title());
        assert_eq!(arrived.panes().leaves_in_order(), leaves);
        assert_eq!(arrived.panes().focused_id(), first);
        assert!(arrived.panes().pane(second).is_some());
        assert_eq!(arrived.panes(), &panes_before);
    }

    // ---------------------------------------------------------------
    // move_tabs
    // ---------------------------------------------------------------

    #[test]
    fn move_tabs_into_another_group_keeps_ids_order_and_active() {
        let (mut ws, _, _, g3) = eight();
        let active = ws.active_tab();
        // Out of order on purpose; one from each of three places.
        assert!(ws.move_tabs(&ids(&[5, 0, 2]), InsertAt::IntoGroup { group: g3, pos: 1 }));
        assert_eq!(tabs_of(&ws, g3), ids(&[6, 0, 2, 5, 7]));
        assert_eq!(track(&ws), ids(&[1, 3, 4, 6, 0, 2, 5, 7]));
        assert_eq!(ws.groups().len(), 4);
        assert_eq!(ws.active_tab(), active);
        assert_eq!(ws.tab(TabId::new(2)).unwrap().id(), TabId::new(2));
        assert_invariants(&ws);
    }

    #[test]
    fn move_tabs_to_a_new_run_keeps_ids_order_and_active() {
        let (mut ws, ..) = eight();
        let active = ws.active_tab();
        assert!(ws.move_tabs(&ids(&[5, 0, 2]), InsertAt::NewRun { group_index: 0 }));
        assert_eq!(track(&ws), ids(&[0, 2, 5, 1, 3, 4, 6, 7]));
        assert!(ws.groups()[0].is_implicit());
        assert_eq!(ws.active_tab(), active);
        assert_invariants(&ws);

        // Next to an implicit run: merges (`G1[1] imp[3] ...` after removal).
        let (mut ws, ..) = eight();
        assert!(ws.move_tabs(&ids(&[5, 0, 2]), InsertAt::NewRun { group_index: 1 }));
        assert_eq!(track(&ws), ids(&[1, 0, 2, 5, 3, 4, 6, 7]));
        assert_eq!(ws.groups().len(), 4);
        assert_eq!(ws.groups()[1].tabs(), ids(&[0, 2, 5, 3]));
        assert_invariants(&ws);
    }

    #[test]
    fn move_tabs_to_the_pinned_zone_keeps_ids_order_and_active() {
        let (mut ws, ..) = eight();
        let active = ws.active_tab();
        assert!(ws.move_tabs(&ids(&[5, 0, 2]), InsertAt::Pinned { pos: 0 }));
        assert_eq!(ws.pinned(), ids(&[0, 2, 5]));
        assert_eq!(track(&ws), ids(&[1, 3, 4, 6, 7]));
        assert_eq!(visual(&ws), ids(&[0, 2, 5, 1, 3, 4, 6, 7]));
        assert_eq!(ws.active_tab(), active);
        assert_invariants(&ws);

        // Into the middle of an existing zone.
        assert!(ws.move_tabs(&ids(&[7, 6]), InsertAt::Pinned { pos: 1 }));
        assert_eq!(ws.pinned(), ids(&[0, 6, 7, 2, 5]));
        assert_invariants(&ws);
    }

    #[test]
    fn move_tabs_keeps_the_active_tab_when_it_is_in_the_block() {
        let (mut ws, _, _, g3) = eight();
        ws.activate_tab(TabId::new(2));
        assert!(ws.move_tabs(&ids(&[2, 3]), InsertAt::IntoGroup { group: g3, pos: 0 }));
        assert_eq!(ws.active_tab(), Some(TabId::new(2)));
        assert_eq!(tabs_of(&ws, g3), ids(&[2, 3, 6, 7]));
        assert_eq!(ws.group(g3).unwrap().last_active(), Some(TabId::new(2)));
        assert_invariants(&ws);

        assert!(ws.move_tabs(&ids(&[2]), InsertAt::Pinned { pos: 0 }));
        assert_eq!(ws.active_tab(), Some(TabId::new(2)));
        assert!(ws.is_pinned(TabId::new(2)));
        assert_invariants(&ws);
    }

    #[test]
    fn move_tabs_expands_a_collapsed_group_that_receives_the_active_tab() {
        let (mut ws, _, _, g3) = eight();
        ws.activate_tab(TabId::new(2));
        ws.collapse_group(g3, true);
        assert!(ws.group(g3).unwrap().is_collapsed());
        assert!(ws.move_tabs(&ids(&[2]), InsertAt::IntoGroup { group: g3, pos: 0 }));
        assert!(!ws.group(g3).unwrap().is_collapsed());
        assert_eq!(ws.active_tab(), Some(TabId::new(2)));
        assert_invariants(&ws);
    }

    #[test]
    fn move_tabs_keeps_a_collapsed_group_collapsed_when_the_active_tab_is_elsewhere() {
        let (mut ws, _, _, g3) = eight();
        ws.activate_tab(TabId::new(0));
        ws.collapse_group(g3, true);
        assert!(ws.move_tabs(&ids(&[1]), InsertAt::IntoGroup { group: g3, pos: 0 }));
        assert!(ws.group(g3).unwrap().is_collapsed());
        assert_eq!(ws.active_tab(), Some(TabId::new(0)));
        assert_invariants(&ws);
    }

    #[test]
    fn move_tabs_of_a_whole_group_into_itself_falls_back_to_the_track_end() {
        let (mut ws, g1, ..) = eight();
        let active = ws.active_tab();
        assert!(ws.move_tabs(&ids(&[1, 2]), InsertAt::IntoGroup { group: g1, pos: 0 }));
        assert!(ws.group(g1).is_none());
        assert_eq!(track(&ws), ids(&[0, 3, 4, 5, 6, 7, 1, 2]));
        assert_eq!(ws.active_tab(), active);
        assert_invariants(&ws);
    }

    #[test]
    fn move_tabs_within_the_same_group_reorders_it() {
        let (mut ws, g1, ..) = eight();
        assert!(ws.move_tabs(&ids(&[1]), InsertAt::IntoGroup { group: g1, pos: 1 }));
        assert_eq!(tabs_of(&ws, g1), ids(&[2, 1]));
        assert_invariants(&ws);
    }

    #[test]
    fn move_tabs_with_nothing_valid_is_refused_and_changes_nothing() {
        let (mut ws, ..) = eight();
        ws.pin_tab(TabId::new(3), 0);
        let before = ws.clone();
        assert!(!ws.move_tabs(&[], InsertAt::TrackEnd));
        assert!(!ws.move_tabs(
            &[TabId::new(3), TabId::new(77)],
            InsertAt::NewRun { group_index: 0 }
        ));
        assert_eq!(ws, before);
    }

    #[test]
    fn move_tabs_ignores_a_pinned_tab_among_valid_ones() {
        let (mut ws, ..) = eight();
        ws.pin_tab(TabId::new(3), 0);
        assert!(ws.move_tabs(&ids(&[3, 0]), InsertAt::TrackEnd));
        assert_eq!(ws.pinned(), ids(&[3]));
        assert_eq!(track(&ws).last(), Some(&TabId::new(0)));
        assert_invariants(&ws);
    }

    #[test]
    fn move_tabs_does_not_touch_the_id_counters() {
        let (mut ws, ..) = eight();
        let mut grown = ws.clone();
        assert!(ws.move_tabs(&ids(&[0, 1]), InsertAt::TrackEnd));
        let next = ws.append_tab("zsh", None);
        assert_eq!(next, grown.append_tab("zsh", None));
    }

    // ---------------------------------------------------------------
    // is_empty and whole-flow invariants
    // ---------------------------------------------------------------

    #[test]
    fn is_empty_counts_the_track_and_the_pinned_zone() {
        let mut ws = Workspace::new();
        assert!(ws.is_empty());
        let a = ws.append_tab("zsh", None);
        assert!(!ws.is_empty());
        ws.pin_tab(a, 0);
        assert!(!ws.is_empty(), "only a pinned tab is still not empty");
        ws.close_tab(a);
        assert!(ws.is_empty());
    }

    #[test]
    fn invariants_hold_in_both_workspaces_after_a_mixed_transfer_sequence() {
        let (mut origin, g1, g2, _) = eight();
        origin.pin_tab(TabId::new(0), 0);
        let mut dest = Workspace::new();
        dest.append_tab("zsh", None);

        let taken = origin.take_tabs(&ids(&[4, 3]));
        assert_invariants(&origin);
        let new = dest.insert_detached_tabs(
            taken.into_iter().map(|(_, t)| t).collect(),
            InsertAt::Pinned { pos: 0 },
        );
        assert_invariants(&dest);
        assert_eq!(dest.pinned(), new);

        let (group, old) = origin.take_group(g1).unwrap();
        assert_eq!(old, ids(&[1, 2]));
        assert_invariants(&origin);
        let (_, tabs) = dest.insert_detached_group(group, 0);
        assert_invariants(&dest);
        assert_eq!(track(&dest)[..2], tabs[..]);

        let pinned = origin.take_pinned(TabId::new(0)).unwrap();
        assert_invariants(&origin);
        dest.insert_detached_tabs(vec![pinned], InsertAt::TrackEnd);
        assert_invariants(&dest);

        let _ = origin.take_group(g2);
        assert_invariants(&origin);
        assert!(origin.move_tabs(&ids(&[6]), InsertAt::NewRun { group_index: 0 }));
        assert_invariants(&origin);
    }

    #[test]
    fn detached_types_round_trip_through_clone() {
        let (mut ws, t) = workspace_of(2);
        let g = ws.group_tabs(&[t[0], t[1]], "g", GroupColor::Red).unwrap();
        let (group, _) = ws.take_group(g).unwrap();
        assert_eq!(group.clone(), group);
        assert_eq!(group.tabs()[0].clone(), group.tabs()[0]);
    }
}
