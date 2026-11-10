// SPDX-License-Identifier: GPL-3.0-or-later

//! The process list as a pure function of the workspace and the last sample
//! (ADR-0095 §5, PRD-024 RF-24.13 to RF-24.18), plus the selection, scroll
//! and keyboard navigation rules that only look at the list.

use std::collections::{HashMap, HashSet};

use porecatu_core::{GroupColor, PaneId, TabId, Workspace};
use porecatu_term::ProcessStat;

use super::{Totals, WindowSample, totals};

/// Identity of a row (ADR-0095 §5): stable across samples, so the selection
/// survives the list being rebuilt (RF-24.18). A process is identified by
/// its PID **and** start time, because a PID can be reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RowKey {
    Tab(TabId),
    Note(TabId, PaneId),
    Process {
        tab: TabId,
        pane: PaneId,
        pid: u32,
        start_time: u64,
    },
}

impl RowKey {
    /// The tab the row belongs to.
    pub(crate) fn tab(&self) -> TabId {
        match self {
            Self::Tab(tab) | Self::Note(tab, _) | Self::Process { tab, .. } => *tab,
        }
    }

    /// The pane the row belongs to; `None` for a tab row.
    pub(crate) fn pane(&self) -> Option<PaneId> {
        match self {
            Self::Tab(_) => None,
            Self::Note(_, pane) | Self::Process { pane, .. } => Some(*pane),
        }
    }
}

/// Why a pane has a note instead of processes (RF-24.17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoteKind {
    /// `lazy_restore`: the tab has no runtime until its first focus.
    NotStarted,
    /// SSH pane, dormant or connected: the client is embedded, there is no
    /// local process and the remote ones are not listed.
    Ssh,
    /// The pane's process ended.
    Exited,
    /// A local pane the sampler has not answered for yet ("waiting for the
    /// first measurement").
    Waiting,
}

/// What the window knows about a pane, for the list (ADR-0095 §5). Step 06
/// builds these from `WindowState.panes`; the rules, one per variant:
///
/// - SSH pane, dormant or connected (`ssh_host.is_some()`): [`Self::Ssh`];
/// - pane whose process ended (`PaneState::Exited`): [`Self::Exited`];
/// - any other pane with a runtime: [`Self::Local`], with the root PID of its
///   `ProcessGroup`.
///
/// A pane **absent** from [`PaneFacts`] has no runtime (a `NotStarted` tab,
/// ADR-0037) and gets the [`NoteKind::NotStarted`] note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneFact {
    /// A local PTY. `root_pid` is `Terminal::process_group().root_pid()`,
    /// `None` when the PTY has no `ProcessGroup` (nothing to measure, so the
    /// note is [`NoteKind::Waiting`]).
    Local {
        root_pid: Option<u32>,
    },
    Ssh,
    Exited,
}

pub(crate) type PaneFacts = HashMap<(TabId, PaneId), PaneFact>;

/// One line of the list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    pub key: RowKey,
    pub kind: RowKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RowKind {
    Tab {
        tab: TabId,
        /// Same precedence as the tab bar (RF-1.7): `Tab::title()`.
        title: String,
        /// The group's color choice, `None` for an implicit run, a loose or a
        /// pinned tab. Resolved to a `Color` by the painting, per frame, with
        /// the live palette (so a theme change does not leave stale colors
        /// in a list built from an older sample).
        group_color: Option<GroupColor>,
        /// Sum of every process listed under the tab (RF-24.15).
        totals: Totals,
    },
    Process {
        stat: ProcessStat,
        /// The pane root is at depth 1 (it is already one level below the
        /// tab title).
        depth: usize,
        /// The root of the pane: the one "End process" refuses (RF-24.22).
        is_root: bool,
        /// How many processes hang under this one in the sample's tree
        /// (RF-24.21: "and N child processes").
        descendants: usize,
    },
    Note {
        kind: NoteKind,
        /// Always 1: the note sits where the root would.
        depth: usize,
    },
}

/// Builds the list (RF-24.13 to RF-24.17).
///
/// - Tabs in `workspace.visual_order()`: the pinned first, then the track,
///   collapsed groups included.
/// - Panes of a tab in the order they appear on screen
///   (`PaneTree::leaves_in_order`, top to bottom and left to right, RF-24.15).
///   The flat `PaneTree::panes()` slice is creation order, not screen order.
/// - A `Local` pane with its root in the sample lists the tree: the root at
///   depth 1, each child under its parent, siblings by `start_time` then
///   PID. The sample does not carry the root PID, and a member whose parent
///   is outside the sample (the Job list reaches processes the parent chain
///   does not, ADR-0093 §5) hangs from the root, so the root comes from
///   `facts`.
/// - A `Local` pane **without** the root in the sample (no entry yet, or the
///   root died between samples) gets a [`NoteKind::Waiting`] note; `Exited`
///   is decided by the fact, never by the sample.
/// - `Ssh` and `Exited` facts give their notes, a pane absent from `facts`
///   gives [`NoteKind::NotStarted`]. In a tab with panes of different kinds
///   the note is per pane (RF-24.17).
pub(crate) fn build_rows(
    workspace: &Workspace,
    facts: &PaneFacts,
    sample: &WindowSample,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for tab_id in workspace.visual_order() {
        let Some(tab) = workspace.tab(tab_id) else {
            continue;
        };
        let group_color = workspace
            .group_of_tab(tab_id)
            .and_then(|group| workspace.group(group))
            .and_then(|group| group.color());

        // The tab row is pushed first and filled in once its panes are known.
        let tab_row = rows.len();
        rows.push(Row {
            key: RowKey::Tab(tab_id),
            kind: RowKind::Tab {
                tab: tab_id,
                title: tab.title().to_owned(),
                group_color,
                totals: totals(std::iter::empty()),
            },
        });

        let mut listed: Vec<&ProcessStat> = Vec::new();
        for pane_id in tab.panes().leaves_in_order() {
            let key = (tab_id, pane_id);
            let note = |kind| Row {
                key: RowKey::Note(tab_id, pane_id),
                kind: RowKind::Note { kind, depth: 1 },
            };
            match facts.get(&key) {
                None => rows.push(note(NoteKind::NotStarted)),
                Some(PaneFact::Ssh) => rows.push(note(NoteKind::Ssh)),
                Some(PaneFact::Exited) => rows.push(note(NoteKind::Exited)),
                Some(PaneFact::Local { root_pid }) => {
                    let stats = sample.panes.get(&key).map(Vec::as_slice).unwrap_or(&[]);
                    match root_pid.filter(|root| stats.iter().any(|s| s.pid == *root)) {
                        Some(root) => {
                            push_tree(&mut rows, tab_id, pane_id, root, stats);
                            listed.extend(stats);
                        }
                        None => rows.push(note(NoteKind::Waiting)),
                    }
                }
            }
        }

        if let RowKind::Tab { totals: sum, .. } = &mut rows[tab_row].kind {
            *sum = totals(listed);
        }
    }
    rows
}

/// Appends the rows of one pane's process tree, in pre-order.
///
/// Iterative on purpose: a pathological chain (thousands of nested
/// processes) must not blow the stack. Processes unreachable from the root
/// (a parent cycle from PID reuse) are hung from the root too, so nothing
/// the sampler measured disappears from the list.
fn push_tree(rows: &mut Vec<Row>, tab: TabId, pane: PaneId, root: u32, stats: &[ProcessStat]) {
    // Pre-order walk: (index into `stats`, depth, slot of the parent entry).
    let mut order: Vec<(usize, usize, Option<usize>)> = Vec::with_capacity(stats.len());

    // Duplicated PIDs would make the tree ambiguous; the first one wins.
    let mut index_of: HashMap<u32, usize> = HashMap::with_capacity(stats.len());
    for (i, stat) in stats.iter().enumerate() {
        index_of.entry(stat.pid).or_insert(i);
    }
    let Some(&root_index) = index_of.get(&root) else {
        return;
    };

    let sort_key = |i: &usize| (stats[*i].start_time, stats[*i].pid);
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut root_children: Vec<usize> = Vec::new();
    for (i, stat) in stats.iter().enumerate() {
        if i == root_index || index_of.get(&stat.pid) != Some(&i) {
            continue;
        }
        match stat
            .parent
            .and_then(|parent| index_of.get(&parent))
            .copied()
            .filter(|parent| *parent != i)
        {
            Some(parent) if parent != root_index => children.entry(parent).or_default().push(i),
            // The root itself, or a parent outside the sample: root's child.
            _ => root_children.push(i),
        }
    }
    for list in children.values_mut() {
        list.sort_by_key(sort_key);
    }
    root_children.sort_by_key(sort_key);
    children.insert(root_index, root_children);

    let mut visited: HashSet<usize> = HashSet::with_capacity(stats.len());
    let mut walk = |start: usize,
                    depth: usize,
                    parent_slot: Option<usize>,
                    order: &mut Vec<(usize, usize, Option<usize>)>| {
        // Stack of (index, depth, slot of the parent); children pushed in
        // reverse so the pop order is the sorted order.
        let mut stack = vec![(start, depth, parent_slot)];
        while let Some((index, depth, parent_slot)) = stack.pop() {
            if !visited.insert(index) {
                continue;
            }
            let slot = order.len();
            order.push((index, depth, parent_slot));
            if let Some(kids) = children.get(&index) {
                for kid in kids.iter().rev() {
                    stack.push((*kid, depth + 1, Some(slot)));
                }
            }
        }
    };
    walk(root_index, 1, None, &mut order);

    // Whatever the walk did not reach is in a cycle: hang each leftover
    // (oldest first) from the root, unless an earlier one already took it.
    let mut leftovers: Vec<usize> = (0..stats.len())
        .filter(|i| index_of.get(&stats[*i].pid) == Some(i))
        .collect();
    leftovers.sort_by_key(sort_key);
    for index in leftovers {
        walk(index, 2, Some(0), &mut order);
    }

    // Subtree sizes: children come after their parent in pre-order, so one
    // reverse pass accumulates them.
    let mut subtree = vec![1_usize; order.len()];
    for slot in (0..order.len()).rev() {
        if let Some(parent) = order[slot].2 {
            subtree[parent] += subtree[slot];
        }
    }

    for (slot, (index, depth, _)) in order.iter().enumerate() {
        let stat = &stats[*index];
        rows.push(Row {
            key: RowKey::Process {
                tab,
                pane,
                pid: stat.pid,
                start_time: stat.start_time,
            },
            kind: RowKind::Process {
                stat: stat.clone(),
                depth: *depth,
                is_root: slot == 0,
                descendants: subtree[slot] - 1,
            },
        });
    }
}

/// The index of the row to select (RF-24.18): the exact row; else the tab
/// row of the same tab (a process that died, a note that became a tree);
/// else the first row. `None` for an empty list.
pub(crate) fn resolve_selection(rows: &[Row], wanted: Option<&RowKey>) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    if let Some(wanted) = wanted {
        if let Some(index) = rows.iter().position(|row| &row.key == wanted) {
            return Some(index);
        }
        let tab = wanted.tab();
        if let Some(index) = rows
            .iter()
            .position(|row| matches!(row.key, RowKey::Tab(t) if t == tab))
        {
            return Some(index);
        }
    }
    Some(0)
}

/// Scroll that keeps `selected` in view, moving only when it left the view
/// (ADR-0095 §5: "adjusted only if the selection leaves the view").
pub(crate) fn keep_visible(first_visible: usize, selected: usize, visible_rows: usize) -> usize {
    if visible_rows == 0 {
        return first_visible;
    }
    if selected < first_visible {
        selected
    } else if selected >= first_visible + visible_rows {
        selected + 1 - visible_rows
    } else {
        first_visible
    }
}

/// Clamps the scroll to the end of the list: a list that shrank (processes
/// died) never leaves the view past its last row.
pub(crate) fn clamp_scroll(first_visible: usize, total_rows: usize, visible_rows: usize) -> usize {
    first_visible.min(total_rows.saturating_sub(visible_rows))
}

/// A keyboard move of the selection (ADR-0095 §3, RF-24.19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListNav {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// The index the selection goes to; no wrap-around. With no selection,
/// moves that go down or to the start pick the first row and the others the
/// last. `page` is the number of rows a screen shows.
pub(crate) fn navigate(
    rows: &[Row],
    current: Option<usize>,
    nav: ListNav,
    page: usize,
) -> Option<usize> {
    let last = rows.len().checked_sub(1)?;
    let page = page.max(1);
    let Some(current) = current else {
        return Some(match nav {
            ListNav::Down | ListNav::Home | ListNav::PageDown => 0,
            ListNav::Up | ListNav::End | ListNav::PageUp => last,
        });
    };
    let current = current.min(last);
    Some(match nav {
        ListNav::Up => current.saturating_sub(1),
        ListNav::Down => (current + 1).min(last),
        ListNav::Home => 0,
        ListNav::End => last,
        ListNav::PageUp => current.saturating_sub(page),
        ListNav::PageDown => (current + page).min(last),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use porecatu_core::{SplitAxis, UnpinTarget};

    fn stat(pid: u32, parent: Option<u32>, start_time: u64) -> ProcessStat {
        ProcessStat {
            pid,
            parent,
            start_time,
            name: format!("p{pid}"),
            cpu: Some(0.1),
            memory: 100,
            disk_per_sec: 1.0,
        }
    }

    /// Workspace with one implicit run of `n` tabs; returns the ids.
    fn workspace_of(n: usize) -> (Workspace, Vec<TabId>) {
        let mut ws = Workspace::new();
        let mut ids = Vec::new();
        for _ in 0..n {
            ids.push(ws.append_ungrouped_tab("pwsh", None));
        }
        (ws, ids)
    }

    fn only_pane(ws: &Workspace, tab: TabId) -> PaneId {
        ws.tab(tab).unwrap().panes().focused_id()
    }

    fn local(root: u32) -> PaneFact {
        PaneFact::Local {
            root_pid: Some(root),
        }
    }

    fn keys(rows: &[Row]) -> Vec<RowKey> {
        rows.iter().map(|r| r.key).collect()
    }

    fn pids(rows: &[Row]) -> Vec<(u32, usize)> {
        rows.iter()
            .filter_map(|row| match &row.kind {
                RowKind::Process { stat, depth, .. } => Some((stat.pid, *depth)),
                _ => None,
            })
            .collect()
    }

    fn process(rows: &[Row], pid: u32) -> (&ProcessStat, usize, bool, usize) {
        rows.iter()
            .find_map(|row| match &row.kind {
                RowKind::Process {
                    stat,
                    depth,
                    is_root,
                    descendants,
                } if stat.pid == pid => Some((stat, *depth, *is_root, *descendants)),
                _ => None,
            })
            .unwrap()
    }

    fn build(
        ws: &Workspace,
        facts: &[((TabId, PaneId), PaneFact)],
        panes: &[((TabId, PaneId), Vec<ProcessStat>)],
    ) -> Vec<Row> {
        let facts: PaneFacts = facts.iter().cloned().collect();
        let sample = WindowSample {
            taken_at: None,
            panes: panes.iter().cloned().collect(),
        };
        build_rows(ws, &facts, &sample)
    }

    #[test]
    fn order_is_pinned_first_then_the_track_with_collapsed_groups_included() {
        let mut ws = Workspace::new();
        let a = ws.append_ungrouped_tab("a", None);
        let b = ws.append_ungrouped_tab("b", None);
        let c = ws.append_ungrouped_tab("c", None);
        let d = ws.append_ungrouped_tab("d", None);
        let group = ws.group_tabs(&[b, c], "api", GroupColor::Blue).unwrap();
        assert!(ws.collapse_group(group, true));
        assert!(ws.pin_tab(d, 0));
        ws.activate_tab(a);

        let rows = build(&ws, &[], &[]);
        // d (pinned) first; then a, then the collapsed group's b and c.
        let tab_rows: Vec<TabId> = rows
            .iter()
            .filter(|row| matches!(row.key, RowKey::Tab(_)))
            .map(|row| row.key.tab())
            .collect();
        assert_eq!(tab_rows, vec![d, a, b, c]);
        // Facts missing: every tab shows a NotStarted note for its pane.
        assert_eq!(rows.len(), 8);
        assert!(matches!(
            rows[1].kind,
            RowKind::Note {
                kind: NoteKind::NotStarted,
                depth: 1
            }
        ));
    }

    #[test]
    fn tab_row_carries_title_group_color_and_none_for_implicit_and_pinned() {
        let mut ws = Workspace::new();
        let a = ws.append_ungrouped_tab("alpha", None);
        let b = ws.append_ungrouped_tab("beta", None);
        let c = ws.append_ungrouped_tab("gamma", None);
        ws.group_tabs(&[b], "api", GroupColor::Green).unwrap();
        ws.tab_mut(a)
            .unwrap()
            .set_custom_title(Some("custom".to_owned()));
        assert!(ws.pin_tab(c, 0));

        let rows = build(&ws, &[], &[]);
        let info = |tab: TabId| {
            rows.iter()
                .find_map(|row| match &row.kind {
                    RowKind::Tab {
                        tab: t,
                        title,
                        group_color,
                        ..
                    } if *t == tab => Some((title.clone(), *group_color)),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(info(a), ("custom".to_owned(), None));
        assert_eq!(info(b), ("beta".to_owned(), Some(GroupColor::Green)));
        assert_eq!(info(c), ("gamma".to_owned(), None));
    }

    #[test]
    fn tree_root_child_grandchild_with_siblings_by_start_time_then_pid() {
        let (ws, ids) = workspace_of(1);
        let tab = ids[0];
        let pane = only_pane(&ws, tab);
        let stats = vec![
            // Listed out of order on purpose.
            stat(40, Some(10), 300),
            stat(10, Some(1), 100),
            stat(30, Some(10), 200),
            stat(35, Some(10), 200), // same start as 30 -> PID decides
            stat(50, Some(30), 400),
        ];
        let rows = build(&ws, &[((tab, pane), local(10))], &[((tab, pane), stats)]);
        assert_eq!(
            pids(&rows),
            vec![(10, 1), (30, 2), (50, 3), (35, 2), (40, 2)]
        );
        let (_, depth, is_root, descendants) = process(&rows, 10);
        assert_eq!((depth, is_root, descendants), (1, true, 4));
        assert_eq!(process(&rows, 30).3, 1);
        assert_eq!(process(&rows, 35).3, 0);
        assert!(!process(&rows, 50).2);
    }

    #[test]
    fn an_orphan_hangs_from_the_root() {
        let (ws, ids) = workspace_of(1);
        let tab = ids[0];
        let pane = only_pane(&ws, tab);
        let stats = vec![
            stat(10, Some(1), 100),
            // Parent 999 is not in the sample (cut chain the Job list found).
            stat(20, Some(999), 150),
            stat(21, Some(20), 160),
        ];
        let rows = build(&ws, &[((tab, pane), local(10))], &[((tab, pane), stats)]);
        assert_eq!(pids(&rows), vec![(10, 1), (20, 2), (21, 3)]);
        assert_eq!(process(&rows, 10).3, 2);
    }

    #[test]
    fn a_parent_cycle_unreachable_from_the_root_still_lists_everything_once() {
        let (ws, ids) = workspace_of(1);
        let tab = ids[0];
        let pane = only_pane(&ws, tab);
        let stats = vec![
            stat(10, None, 100),
            stat(20, Some(21), 150),
            stat(21, Some(20), 160),
        ];
        let rows = build(&ws, &[((tab, pane), local(10))], &[((tab, pane), stats)]);
        let listed = pids(&rows);
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0], (10, 1));
        assert!(listed.contains(&(20, 2)));
    }

    #[test]
    fn a_process_that_is_its_own_parent_does_not_loop() {
        let (ws, ids) = workspace_of(1);
        let tab = ids[0];
        let pane = only_pane(&ws, tab);
        let stats = vec![stat(10, None, 100), stat(20, Some(20), 150)];
        let rows = build(&ws, &[((tab, pane), local(10))], &[((tab, pane), stats)]);
        assert_eq!(pids(&rows), vec![(10, 1), (20, 2)]);
    }

    #[test]
    fn notes_by_kind_and_per_pane_in_a_mixed_tab() {
        let (mut ws, ids) = workspace_of(2);
        let (mixed, plain) = (ids[0], ids[1]);
        let p0 = only_pane(&ws, mixed);
        let p1 = ws
            .tab_mut(mixed)
            .unwrap()
            .panes_mut()
            .split(p0, SplitAxis::Vertical, "ssh", None)
            .unwrap();
        let p2 = ws
            .tab_mut(mixed)
            .unwrap()
            .panes_mut()
            .split(p1, SplitAxis::Horizontal, "x", None)
            .unwrap();
        let plain_pane = only_pane(&ws, plain);

        let facts = [
            ((mixed, p0), local(10)),
            ((mixed, p1), PaneFact::Ssh),
            ((mixed, p2), PaneFact::Exited),
            // `plain` has no runtime: absent from the facts.
        ];
        let rows = build(&ws, &facts, &[]);
        let kinds: Vec<(RowKey, NoteKind)> = rows
            .iter()
            .filter_map(|row| match &row.kind {
                RowKind::Note { kind, .. } => Some((row.key, *kind)),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                (RowKey::Note(mixed, p0), NoteKind::Waiting),
                (RowKey::Note(mixed, p1), NoteKind::Ssh),
                (RowKey::Note(mixed, p2), NoteKind::Exited),
                (RowKey::Note(plain, plain_pane), NoteKind::NotStarted),
            ]
        );
    }

    #[test]
    fn a_local_pane_without_a_group_or_without_its_root_waits() {
        let (ws, ids) = workspace_of(2);
        let (a, b) = (ids[0], ids[1]);
        let (pa, pb) = (only_pane(&ws, a), only_pane(&ws, b));
        let facts = [
            ((a, pa), PaneFact::Local { root_pid: None }),
            ((b, pb), local(10)),
        ];
        // `b` has stats, but not the root: it died between samples.
        let rows = build(&ws, &facts, &[((b, pb), vec![stat(77, Some(10), 1)])]);
        let notes: Vec<NoteKind> = rows
            .iter()
            .filter_map(|row| match row.kind {
                RowKind::Note { kind, .. } => Some(kind),
                _ => None,
            })
            .collect();
        assert_eq!(notes, vec![NoteKind::Waiting, NoteKind::Waiting]);
        assert!(pids(&rows).is_empty());
    }

    #[test]
    fn an_exited_fact_wins_over_a_stale_sample() {
        let (ws, ids) = workspace_of(1);
        let tab = ids[0];
        let pane = only_pane(&ws, tab);
        let rows = build(
            &ws,
            &[((tab, pane), PaneFact::Exited)],
            &[((tab, pane), vec![stat(10, None, 1)])],
        );
        assert!(matches!(
            rows[1].kind,
            RowKind::Note {
                kind: NoteKind::Exited,
                ..
            }
        ));
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn panes_follow_the_screen_order_not_the_creation_order() {
        let (mut ws, ids) = workspace_of(1);
        let tab = ids[0];
        let p0 = only_pane(&ws, tab);
        let panes = ws.tab_mut(tab).unwrap().panes_mut();
        let p1 = panes.split(p0, SplitAxis::Vertical, "a", None).unwrap();
        // Splitting p0 again puts p2 between p0 and p1 on screen.
        let p2 = panes.split(p0, SplitAxis::Vertical, "b", None).unwrap();
        assert_eq!(panes.leaves_in_order(), vec![p0, p2, p1]);

        let facts: Vec<_> = [p0, p1, p2]
            .iter()
            .map(|p| ((tab, *p), PaneFact::Ssh))
            .collect();
        let rows = build(&ws, &facts, &[]);
        assert_eq!(
            keys(&rows),
            vec![
                RowKey::Tab(tab),
                RowKey::Note(tab, p0),
                RowKey::Note(tab, p2),
                RowKey::Note(tab, p1),
            ]
        );
    }

    #[test]
    fn the_tab_total_sums_the_processes_of_every_pane() {
        let (mut ws, ids) = workspace_of(1);
        let tab = ids[0];
        let p0 = only_pane(&ws, tab);
        let p1 = ws
            .tab_mut(tab)
            .unwrap()
            .panes_mut()
            .split(p0, SplitAxis::Vertical, "b", None)
            .unwrap();
        let mut a = stat(10, None, 1);
        a.memory = 1000;
        a.cpu = Some(0.25);
        let mut b = stat(11, Some(10), 2);
        b.memory = 500;
        b.cpu = Some(0.5);
        let mut c = stat(20, None, 1);
        c.memory = 7;
        c.cpu = None;
        let rows = build(
            &ws,
            &[((tab, p0), local(10)), ((tab, p1), local(20))],
            &[((tab, p0), vec![a, b]), ((tab, p1), vec![c])],
        );
        let RowKind::Tab { totals: sum, .. } = &rows[0].kind else {
            panic!("first row is the tab row");
        };
        assert_eq!(sum.memory, 1507);
        assert_eq!(sum.cpu, Some(0.75));
        assert_eq!(sum.disk_per_sec, 3.0);
    }

    #[test]
    fn an_unpinned_tab_moves_its_row_to_the_track_start() {
        // Guards the `visual_order` contract the list relies on.
        let (mut ws, ids) = workspace_of(2);
        assert!(ws.pin_tab(ids[1], 0));
        assert!(ws.unpin_tab(ids[1], UnpinTarget::TrackStart));
        let rows = build(&ws, &[], &[]);
        assert_eq!(rows[0].key.tab(), ids[1]);
    }

    fn sample_rows() -> (Vec<Row>, TabId, PaneId) {
        let (ws, ids) = workspace_of(2);
        let (a, b) = (ids[0], ids[1]);
        let (pa, pb) = (only_pane(&ws, a), only_pane(&ws, b));
        let stats = vec![stat(10, None, 1), stat(11, Some(10), 2)];
        let rows = build(
            &ws,
            &[((a, pa), local(10)), ((b, pb), PaneFact::Ssh)],
            &[((a, pa), stats)],
        );
        (rows, a, pa)
    }

    #[test]
    fn selection_survives_while_the_process_lives() {
        let (rows, a, pa) = sample_rows();
        let key = RowKey::Process {
            tab: a,
            pane: pa,
            pid: 11,
            start_time: 2,
        };
        assert_eq!(resolve_selection(&rows, Some(&key)), Some(2));
    }

    #[test]
    fn selection_falls_to_the_tab_row_when_the_process_dies() {
        let (rows, a, pa) = sample_rows();
        let dead = RowKey::Process {
            tab: a,
            pane: pa,
            pid: 99,
            start_time: 5,
        };
        assert_eq!(resolve_selection(&rows, Some(&dead)), Some(0));
        // A reused PID (same pid, other start time) is another process.
        let reused = RowKey::Process {
            tab: a,
            pane: pa,
            pid: 11,
            start_time: 999,
        };
        assert_eq!(resolve_selection(&rows, Some(&reused)), Some(0));
        // A note that is gone falls to its tab row, not to the first row:
        // tab 1 is the fourth row.
        let gone_note = RowKey::Note(TabId::new(1), PaneId::new(9));
        assert_eq!(resolve_selection(&rows, Some(&gone_note)), Some(3));
    }

    #[test]
    fn selection_falls_to_the_first_row_when_the_tab_is_gone() {
        let (rows, _, _) = sample_rows();
        let gone = RowKey::Process {
            tab: TabId::new(77),
            pane: PaneId::new(0),
            pid: 1,
            start_time: 1,
        };
        assert_eq!(resolve_selection(&rows, Some(&gone)), Some(0));
        assert_eq!(resolve_selection(&rows, None), Some(0));
        assert_eq!(resolve_selection(&[], Some(&gone)), None);
        assert_eq!(resolve_selection(&[], None), None);
    }

    #[test]
    fn keep_visible_moves_only_when_the_selection_leaves_the_view() {
        // View of 5 rows starting at 10: rows 10..=14.
        assert_eq!(keep_visible(10, 12, 5), 10);
        assert_eq!(keep_visible(10, 10, 5), 10);
        assert_eq!(keep_visible(10, 14, 5), 10);
        assert_eq!(keep_visible(10, 15, 5), 11);
        assert_eq!(keep_visible(10, 9, 5), 9);
        assert_eq!(keep_visible(10, 0, 5), 0);
        assert_eq!(keep_visible(3, 40, 5), 36);
        // No room at all: leave the scroll alone.
        assert_eq!(keep_visible(3, 40, 0), 3);
    }

    #[test]
    fn clamp_scroll_keeps_the_view_inside_the_list() {
        assert_eq!(clamp_scroll(50, 20, 5), 15);
        assert_eq!(clamp_scroll(3, 20, 5), 3);
        assert_eq!(clamp_scroll(3, 2, 5), 0);
        assert_eq!(clamp_scroll(0, 0, 5), 0);
    }

    #[test]
    fn navigation_clamps_at_the_ends_and_pages() {
        let (rows, _, _) = sample_rows(); // 5 rows
        assert_eq!(rows.len(), 5);
        let go = |current, nav, page| navigate(&rows, current, nav, page);
        assert_eq!(go(Some(2), ListNav::Up, 3), Some(1));
        assert_eq!(go(Some(0), ListNav::Up, 3), Some(0));
        assert_eq!(go(Some(2), ListNav::Down, 3), Some(3));
        assert_eq!(go(Some(4), ListNav::Down, 3), Some(4));
        assert_eq!(go(Some(2), ListNav::Home, 3), Some(0));
        assert_eq!(go(Some(2), ListNav::End, 3), Some(4));
        assert_eq!(go(Some(4), ListNav::PageUp, 3), Some(1));
        assert_eq!(go(Some(1), ListNav::PageUp, 3), Some(0));
        assert_eq!(go(Some(1), ListNav::PageDown, 3), Some(4));
        assert_eq!(go(Some(3), ListNav::PageDown, 3), Some(4));
        // A zero page still moves one row.
        assert_eq!(go(Some(1), ListNav::PageDown, 0), Some(2));
        // A stale index past the end is pulled in.
        assert_eq!(go(Some(40), ListNav::Up, 3), Some(3));
    }

    #[test]
    fn navigation_without_a_selection_or_without_rows() {
        let (rows, _, _) = sample_rows();
        assert_eq!(navigate(&rows, None, ListNav::Down, 3), Some(0));
        assert_eq!(navigate(&rows, None, ListNav::Home, 3), Some(0));
        assert_eq!(navigate(&rows, None, ListNav::Up, 3), Some(4));
        assert_eq!(navigate(&rows, None, ListNav::End, 3), Some(4));
        assert_eq!(navigate(&[], Some(0), ListNav::Down, 3), None);
        assert_eq!(navigate(&[], None, ListNav::End, 3), None);
    }
}
