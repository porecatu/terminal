// SPDX-License-Identifier: GPL-3.0-or-later

//! State of the "Move to window..." popover (ADR-0090 §6, RF-22.21,
//! RF-22.22): the destination popover of `tab.move_to_group` (`move_to_group`)
//! with another kind of destination. One row per other terminal window, in
//! most-recent-focus order, each with the title of its active tab and its tab
//! count, and "New window" always last.
//!
//! Generic in the window id (a winit `WindowId` cannot be built in a test).
//! *What* is moved (tab, selection, group, pinned) does not live here: the
//! caller keeps that payload beside the popover, and the pure decisions that
//! build it (what a menu or an action moves) and the rows live below.

use porecatu_core::{InsertAt, TabId, Workspace};

use crate::app_transfer::TransferPayload;
use crate::group_menu;
use crate::selection::Selection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowTarget<W> {
    Window(W),
    /// Always the last row.
    NewWindow,
}

/// One destination window of the list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WindowRow<W> {
    pub window: W,
    /// Title of the active tab of that window.
    pub title: String,
    pub tab_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MoveToWindowPopover<W> {
    /// Logical coordinates of the window that opened it, like
    /// `MoveToGroupPopover::anchor`.
    pub anchor: (f32, f32),
    /// The other terminal windows. "New window" is not here: it is always the
    /// row of index `rows.len()`, so the count lives in one place.
    rows: Vec<WindowRow<W>>,
    highlighted: usize,
}

impl<W: Copy> MoveToWindowPopover<W> {
    pub(crate) fn new(anchor: (f32, f32), rows: Vec<WindowRow<W>>) -> Self {
        Self {
            anchor,
            rows,
            highlighted: 0,
        }
    }

    pub(crate) fn rows(&self) -> &[WindowRow<W>] {
        &self.rows
    }

    pub(crate) const fn highlighted(&self) -> usize {
        self.highlighted
    }

    /// The windows plus "New window".
    pub(crate) fn row_count(&self) -> usize {
        self.rows.len() + 1
    }

    pub(crate) fn move_highlight(&mut self, delta: isize) {
        let len = self.row_count() as isize;
        self.highlighted = (self.highlighted as isize + delta).rem_euclid(len) as usize;
    }

    pub(crate) fn set_highlight(&mut self, index: usize) {
        if index < self.row_count() {
            self.highlighted = index;
        }
    }

    pub(crate) fn selected(&self) -> WindowTarget<W> {
        match self.rows.get(self.highlighted) {
            Some(row) => WindowTarget::Window(row.window),
            None => WindowTarget::NewWindow,
        }
    }
}

/// The selected tabs in the visual order of the bar (RF-22.24), whatever
/// order they were selected in. Pinned tabs are never selectable (RF-21.3), so
/// in practice these are track tabs.
pub(crate) fn selected_in_visual_order(workspace: &Workspace, selection: &Selection) -> Vec<TabId> {
    workspace
        .visual_order()
        .filter(|&id| selection.is_selected(id))
        .collect()
}

/// What "Move to window..." moves for a **tab** (RF-22.24, decision 2 of the
/// owner).
///
/// - `clicked = Some(t)`, the item of the context menu of tab `t`: when `t` is
///   part of a non-empty selection the whole selection moves (and `t` is the
///   one that becomes active over there); otherwise only `t` moves -- as a
///   pinned tab when it is one.
/// - `clicked = None`, the `tab.move_to_window` action: the selection when
///   there is one (the active tab is the grabbed one if it belongs to it),
///   else the active tab.
///
/// `None` when there is nothing to move (no active tab).
pub(crate) fn tab_move_payload(
    workspace: &Workspace,
    selection: &Selection,
    clicked: Option<TabId>,
) -> Option<TransferPayload> {
    let selected = selected_in_visual_order(workspace, selection);
    let single = |tab: TabId| {
        if workspace.is_pinned(tab) {
            TransferPayload::Pinned(tab)
        } else {
            TransferPayload::Tabs {
                ids: vec![tab],
                grabbed: Some(tab),
            }
        }
    };
    match clicked {
        Some(tab) if selected.contains(&tab) => Some(TransferPayload::Tabs {
            ids: selected,
            grabbed: Some(tab),
        }),
        Some(tab) => Some(single(tab)),
        None if !selected.is_empty() => {
            let grabbed = workspace
                .active_tab()
                .filter(|active| selected.contains(active));
            Some(TransferPayload::Tabs {
                ids: selected,
                grabbed,
            })
        }
        None => workspace.active_tab().map(single),
    }
}

/// What the `group.move_to_window` action moves: the **explicit** group of the
/// active tab. Outside one, nothing (the action is inert, RF-22.24).
pub(crate) fn group_move_payload(workspace: &Workspace) -> Option<TransferPayload> {
    group_menu::keyboard_target(workspace).map(TransferPayload::Group)
}

/// Where a payload lands in the window chosen in the popover (RF-22.23): the
/// end of the track for loose tabs and for a group, the end of the zone for a
/// pinned tab (which stays pinned).
pub(crate) fn destination_insert_at(payload: &TransferPayload) -> InsertAt {
    match payload {
        TransferPayload::Tabs { .. } | TransferPayload::Group(_) => InsertAt::TrackEnd,
        TransferPayload::Pinned(_) => InsertAt::Pinned { pos: usize::MAX },
    }
}

/// The rows of the popover from the windows in most-recent-focus order, each
/// as `(window, title of its active tab, tab count)`: every window but the
/// origin, in the order given (RF-22.21, minimized ones included).
pub(crate) fn window_rows<W: Copy + PartialEq>(
    origin: W,
    windows: impl IntoIterator<Item = (W, String, usize)>,
) -> Vec<WindowRow<W>> {
    windows
        .into_iter()
        .filter(|(window, _, _)| *window != origin)
        .map(|(window, title, tab_count)| WindowRow {
            window,
            title,
            tab_count,
        })
        .collect()
}

/// The title the bar shows for the active tab of a window (empty without
/// one) -- the same `Tab::title` the bar and the window title use.
pub(crate) fn active_tab_title(workspace: &Workspace) -> String {
    workspace
        .active_tab()
        .and_then(|id| workspace.tab(id))
        .map(|tab| tab.title().to_owned())
        .unwrap_or_default()
}

/// Every tab of a window: the pinned zone plus the track.
pub(crate) fn tab_total(workspace: &Workspace) -> usize {
    workspace.visual_order().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use porecatu_core::GroupColor;

    type Win = u8;

    fn row(window: Win, title: &str, tab_count: usize) -> WindowRow<Win> {
        WindowRow {
            window,
            title: title.to_owned(),
            tab_count,
        }
    }

    #[test]
    fn new_window_is_always_the_last_row() {
        let mut popover =
            MoveToWindowPopover::new((0.0, 0.0), vec![row(1, "a", 2), row(2, "b", 1)]);
        assert_eq!(popover.row_count(), 3);
        popover.set_highlight(2);
        assert_eq!(popover.selected(), WindowTarget::NewWindow);
    }

    #[test]
    fn the_rows_keep_their_order_and_data() {
        let popover =
            MoveToWindowPopover::new((3.0, 4.0), vec![row(7, "vim", 5), row(2, "zsh", 1)]);
        assert_eq!(popover.anchor, (3.0, 4.0));
        assert_eq!(popover.rows()[0], row(7, "vim", 5));
        assert_eq!(popover.rows()[1], row(2, "zsh", 1));
        assert_eq!(popover.highlighted(), 0);
        assert_eq!(popover.selected(), WindowTarget::Window(7));
    }

    #[test]
    fn no_rows_still_offers_new_window() {
        let popover = MoveToWindowPopover::<Win>::new((0.0, 0.0), vec![]);
        assert_eq!(popover.row_count(), 1);
        assert_eq!(popover.selected(), WindowTarget::NewWindow);
    }

    #[test]
    fn move_highlight_wraps_across_all_rows() {
        let mut popover = MoveToWindowPopover::new((0.0, 0.0), vec![row(1, "a", 1)]);
        assert_eq!(popover.selected(), WindowTarget::Window(1));
        popover.move_highlight(-1);
        assert_eq!(popover.selected(), WindowTarget::NewWindow);
        popover.move_highlight(1);
        assert_eq!(popover.selected(), WindowTarget::Window(1));
        popover.move_highlight(2);
        assert_eq!(popover.selected(), WindowTarget::Window(1), "a full turn");
    }

    #[test]
    fn set_highlight_ignores_out_of_range() {
        let mut popover = MoveToWindowPopover::new((0.0, 0.0), vec![row(1, "a", 1)]);
        popover.set_highlight(99);
        assert_eq!(popover.selected(), WindowTarget::Window(1));
        assert_eq!(popover.highlighted(), 0);
    }

    // ---- what is moved, and where it lands

    fn selection_of(tabs: &[TabId]) -> Selection {
        let mut selection = Selection::default();
        for &tab in tabs {
            selection.toggle(tab);
        }
        selection
    }

    /// Three loose tabs `a b c`, `b` active.
    fn three_tabs() -> (Workspace, TabId, TabId, TabId) {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        ws.activate_tab(b);
        (ws, a, b, c)
    }

    #[test]
    fn the_menu_of_a_tab_inside_the_selection_moves_the_whole_selection() {
        let (ws, a, b, c) = three_tabs();
        // Selected out of order; the payload is in the order of the bar.
        let selection = selection_of(&[c, a]);
        assert_eq!(
            tab_move_payload(&ws, &selection, Some(c)),
            Some(TransferPayload::Tabs {
                ids: vec![a, c],
                grabbed: Some(c)
            })
        );
        // The active tab (`b`) is not the clicked one: it plays no role here.
        assert_eq!(ws.active_tab(), Some(b));
    }

    #[test]
    fn the_menu_of_a_tab_outside_the_selection_moves_only_that_tab() {
        let (ws, a, b, c) = three_tabs();
        let selection = selection_of(&[a, c]);
        assert_eq!(
            tab_move_payload(&ws, &selection, Some(b)),
            Some(TransferPayload::Tabs {
                ids: vec![b],
                grabbed: Some(b)
            })
        );
        // With no selection at all, the same.
        assert_eq!(
            tab_move_payload(&ws, &Selection::default(), Some(a)),
            Some(TransferPayload::Tabs {
                ids: vec![a],
                grabbed: Some(a)
            })
        );
    }

    #[test]
    fn the_menu_of_a_pinned_tab_moves_it_as_pinned() {
        let (mut ws, a, _, c) = three_tabs();
        assert!(ws.pin_tab(a, 0));
        // A selection elsewhere does not drag the pinned tab along.
        let selection = selection_of(&[c]);
        assert_eq!(
            tab_move_payload(&ws, &selection, Some(a)),
            Some(TransferPayload::Pinned(a))
        );
    }

    #[test]
    fn the_action_moves_the_selection_with_the_active_tab_grabbed_if_in_it() {
        let (ws, a, b, c) = three_tabs();
        // Active `b` is in the selection: it is the grabbed one.
        assert_eq!(
            tab_move_payload(&ws, &selection_of(&[b, a]), None),
            Some(TransferPayload::Tabs {
                ids: vec![a, b],
                grabbed: Some(b)
            })
        );
        // Active `b` is not in it: nobody is grabbed.
        assert_eq!(
            tab_move_payload(&ws, &selection_of(&[c, a]), None),
            Some(TransferPayload::Tabs {
                ids: vec![a, c],
                grabbed: None
            })
        );
    }

    #[test]
    fn the_action_without_selection_moves_the_active_tab() {
        let (mut ws, a, b, _) = three_tabs();
        assert_eq!(
            tab_move_payload(&ws, &Selection::default(), None),
            Some(TransferPayload::Tabs {
                ids: vec![b],
                grabbed: Some(b)
            })
        );
        // A pinned active tab goes as pinned.
        assert!(ws.pin_tab(a, 0));
        ws.activate_tab(a);
        assert_eq!(
            tab_move_payload(&ws, &Selection::default(), None),
            Some(TransferPayload::Pinned(a))
        );
        // No active tab, nothing to move.
        assert_eq!(
            tab_move_payload(&Workspace::new(), &Selection::default(), None),
            None
        );
    }

    #[test]
    fn the_selection_is_read_in_the_visual_order() {
        let (mut ws, a, b, c) = three_tabs();
        assert!(ws.pin_tab(c, 0));
        // Pinned zone first, then the track, whatever the selection order.
        let ids = selected_in_visual_order(&ws, &selection_of(&[b, a, c]));
        assert_eq!(ids, vec![c, a, b]);
    }

    #[test]
    fn the_group_action_moves_the_explicit_group_of_the_active_tab_and_is_inert_elsewhere() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a], "api", GroupColor::Red).unwrap();
        ws.activate_tab(a);
        assert_eq!(group_move_payload(&ws), Some(TransferPayload::Group(group)));
        // The active tab is in an implicit run: nothing happens.
        ws.activate_tab(b);
        assert_eq!(group_move_payload(&ws), None);
        assert_eq!(group_move_payload(&Workspace::new()), None);
    }

    #[test]
    fn the_destination_is_the_end_of_the_track_or_of_the_zone() {
        let tabs = TransferPayload::Tabs {
            ids: vec![TabId::new(1)],
            grabbed: None,
        };
        let group = TransferPayload::Group(porecatu_core::GroupId::new(1));
        let pinned = TransferPayload::Pinned(TabId::new(1));
        assert_eq!(destination_insert_at(&tabs), InsertAt::TrackEnd);
        assert_eq!(destination_insert_at(&group), InsertAt::TrackEnd);
        assert_eq!(
            destination_insert_at(&pinned),
            InsertAt::Pinned { pos: usize::MAX }
        );
    }

    #[test]
    fn the_rows_follow_the_focus_order_and_leave_the_origin_out() {
        let windows = vec![
            (3u8, "vim".to_owned(), 2),
            (1u8, "origin".to_owned(), 9),
            (2u8, "zsh".to_owned(), 1),
        ];
        let rows = window_rows(1u8, windows);
        assert_eq!(rows, vec![row(3, "vim", 2), row(2, "zsh", 1)]);
    }

    #[test]
    fn with_only_the_origin_there_are_no_rows_but_new_window_remains() {
        let rows = window_rows(1u8, vec![(1u8, "a".to_owned(), 1)]);
        assert!(rows.is_empty());
        let popover = MoveToWindowPopover::new((0.0, 0.0), rows);
        assert_eq!(popover.row_count(), 1);
        assert_eq!(popover.selected(), WindowTarget::NewWindow);
    }

    #[test]
    fn a_window_row_reads_the_active_title_and_counts_pinned_and_track() {
        let (mut ws, a, b, _) = three_tabs();
        assert!(ws.pin_tab(a, 0));
        assert_eq!(tab_total(&ws), 3);
        ws.tab_mut(b)
            .unwrap()
            .set_custom_title(Some("build".into()));
        assert_eq!(active_tab_title(&ws), "build");
        assert_eq!(active_tab_title(&Workspace::new()), "");
        assert_eq!(tab_total(&Workspace::new()), 0);
    }
}
