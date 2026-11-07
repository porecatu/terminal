// SPDX-License-Identifier: GPL-3.0-or-later

//! The drag of tabs, a selection, a pinned tab or a group **across windows**:
//! the part that is not pure geometry (ADR-0091 §1 to §3 and §7, PRD-022
//! RF-22.7 to RF-22.9 and RF-22.16).
//!
//! The origin window owns the `Drag` -- it is the one that gets the mouse --
//! and `App` owns what crosses windows: [`CrossDrag`], built once when the
//! 4 px threshold promotes the press to a drag, holds the detached **copies**
//! the preview of another window needs. The real workspaces are never touched
//! during the gesture; every window that shows something draws a clone with
//! the operation applied, as the local drag always did.
//!
//! What is pure (target resolution, the previews, the ghost of the zone) is a
//! free function with a test, shared by the local paint of `redraw` and by the
//! incoming one: the same function builds the preview whether the tab is in
//! the window or comes from another. What needs a window is in the
//! `impl App` at the end.

use std::rc::Rc;

use porecatu_core::{DetachedGroup, DetachedTab, InsertAt};
use porecatu_render::TextMeasurer;

use super::*;
use crate::app_transfer::{NewWindowPlacement, TransferDest, TransferPayload};
use crate::cross_window::{Candidate, DropDecision, MonitorRect, PointOver, WindowKind};
use crate::pinned::PinnedKey;
use crate::tab_bar::TabBarLayout;

/// A tab id no window has (ids count up from 0): what the target functions
/// receive as `dragged` when the grabbed tab is **not in the layout** -- the
/// destination of a drag that comes from another window. The id of the origin
/// would not do: it may be numerically equal to a tab of the destination, and
/// the target functions would skip that tab.
pub(crate) const ABSENT_TAB: TabId = TabId::new(u32::MAX);
/// Same, for the group.
pub(crate) const ABSENT_GROUP: GroupId = GroupId::new(u32::MAX);

/// What is being dragged (ADR-0091 §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DragPayload {
    /// Track tabs: one, or a selection. `all` is in the visual order of the
    /// track and includes `grabbed`; without a selection it is `[grabbed]`.
    Tabs { grabbed: TabId, all: Vec<TabId> },
    /// A whole explicit group, dragged by its label.
    Group(GroupId),
    /// A box of the pinned zone.
    Pinned(TabId),
}

/// The copies the preview of another window inserts, taken from a clone of
/// the origin workspace when the drag starts (the real one is not touched).
#[derive(Debug, Clone)]
pub(crate) enum Detached {
    /// In the visual order of the origin.
    Tabs(Vec<DetachedTab>),
    Group(DetachedGroup),
}

/// What a window needs to draw the ghost of something that is **not in its
/// workspace** (ADR-0091 §3): built once, shared by reference.
#[derive(Debug, Clone)]
pub(crate) struct GhostContent {
    pub payload: DragPayload,
    pub detached: Detached,
    /// Index of the grabbed tab in the copies of [`Detached::Tabs`] (0 for a
    /// group, which has no grabbed tab).
    pub grabbed_index: usize,
    /// The visual form of the ghost -- the same `GhostSource` the local ghost
    /// takes: a track tab keeps the tab, a pinned box keeps the box. `None`
    /// for a group: the pill is read from the layout of the preview.
    pub source: Option<GhostSource>,
    /// Logical width of the ghost (the tab, the box or the pill).
    pub width: f32,
    /// Where inside the ghost the user holds it, logical, constant.
    pub grab_offset: f32,
    /// Logical `y` of the press inside the bar of the origin. The detach uses
    /// it to place the new window with the grabbed point under the cursor
    /// (ADR-0091 §5).
    pub press_y: f32,
    /// The pinned entry of a dragged pinned tab, for the ghost to draw its
    /// icon at the destination too (ADR-0088 §5); `None` for anything else, or
    /// a pinned tab that has no entry.
    pub icon_key: Option<PinnedKey>,
}

/// The drag across windows (ADR-0091 §3), one per `App`: `Some` from the
/// moment the press is promoted to a drag until the drop or the cancel.
pub(crate) struct CrossDrag {
    pub origin: WindowId,
    /// The window that shows the ghost now, if it is not the origin.
    pub target: Option<WindowId>,
    pub ghost: Rc<GhostContent>,
}

/// What a window that is the **target** of a drag from another window keeps
/// (ADR-0091 §3): written and cleared only by `App`.
pub(crate) struct IncomingDrag {
    pub content: Rc<GhostContent>,
    /// The cursor in the logical coordinates of **this** window (its scale).
    pub logical: (f32, f32),
    /// The target resolved by the last redraw of this window; the drop reads
    /// it (and resolves it on the spot when it is still `None`, before the
    /// first redraw).
    pub target: Option<IncomingTarget>,
}

/// Where the drag lands in the bar of the window it is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IncomingTarget {
    /// Tabs, a selection or a pinned box: the target of the bar, zone
    /// included.
    Tabs(DragDrop),
    /// A group: an index in the group list (never the zone, RF-22.7).
    Group { index: usize },
}

impl IncomingTarget {
    /// The `Workspace::insert_detached_*` destination of this target. For a
    /// group, the position is `index` of the group list (`NewRun`).
    pub(crate) fn insert_at(self, from_zone: bool) -> InsertAt {
        match self {
            IncomingTarget::Tabs(drop) => {
                tab_bar::incoming_insert_at(tab_bar::drop_op(from_zone, drop))
            }
            IncomingTarget::Group { index } => InsertAt::NewRun { group_index: index },
        }
    }
}

// --- the content of the ghost ----------------------------------------------

/// The content for a drag of tabs: a pinned box (`tab` is in the zone), or the
/// track tab `tab` with the rest of its selection (`block`, the **other**
/// selected ones; empty for a lone tab). Copies come from a clone of
/// `workspace`. `None` when there is nothing to copy or the tab has no
/// rectangle in `base_layout` (it sits in a collapsed group): the drag then
/// stays local.
// Eight facts of the press and of the window: a struct would only rename them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_tab_content(
    workspace: &Workspace,
    tab: TabId,
    block: &[TabId],
    grab_offset: f32,
    press_y: f32,
    base_layout: &TabBarLayout,
    style: &TabBarStyle,
    icon_key: Option<PinnedKey>,
) -> Option<GhostContent> {
    let mut clone = workspace.clone();
    if workspace.is_pinned(tab) {
        let copy = clone.take_pinned(tab)?;
        return Some(GhostContent {
            payload: DragPayload::Pinned(tab),
            detached: Detached::Tabs(vec![copy]),
            grabbed_index: 0,
            source: Some(GhostSource::Pinned),
            width: tab_bar::app_menu_button_size(style),
            grab_offset,
            press_y,
            icon_key,
        });
    }
    let all: Vec<TabId> = workspace
        .track_order()
        .filter(|&id| id == tab || block.contains(&id))
        .collect();
    let rect = base_layout
        .groups
        .iter()
        .flat_map(|g| &g.tabs)
        .find(|t| t.id == tab)?
        .clone();
    let taken = clone.take_tabs(&all);
    let grabbed_index = taken.iter().position(|(id, _)| *id == tab)?;
    let width = rect.rect.width;
    Some(GhostContent {
        payload: DragPayload::Tabs { grabbed: tab, all },
        detached: Detached::Tabs(taken.into_iter().map(|(_, copy)| copy).collect()),
        grabbed_index,
        source: Some(GhostSource::Track(rect)),
        width,
        grab_offset,
        press_y,
        icon_key: None,
    })
}

/// The content for a drag of `group` by its label. `None` for a group the
/// layout has no pill for, and for an implicit one (`take_group` refuses it).
pub(crate) fn build_group_content(
    workspace: &Workspace,
    group: GroupId,
    grab_offset: f32,
    press_y: f32,
    base_layout: &TabBarLayout,
) -> Option<GhostContent> {
    let width = tab_bar::pill_rect(base_layout, group)?.width;
    let (copy, _) = workspace.clone().take_group(group)?;
    Some(GhostContent {
        payload: DragPayload::Group(group),
        detached: Detached::Group(copy),
        grabbed_index: 0,
        source: None,
        width,
        grab_offset,
        press_y,
        icon_key: None,
    })
}

// --- previews, shared by the local paint and the incoming one ---------------

/// A tab drag applied to a clone of a workspace.
pub(crate) struct TabsPreview {
    pub workspace: Workspace,
    /// The grabbed tab **in the preview** (the ghost is it).
    pub grabbed: TabId,
    /// Every tab the drag carries, in the preview, grabbed included.
    pub all: Vec<TabId>,
}

/// How the dragged tabs reach the workspace the bar belongs to.
pub(crate) enum TabsArrival<'a> {
    /// A tab of this window, moved by the operation `drop_op` names.
    Local { tab: TabId },
    /// The selection of this window, moved as one block (`move_tabs`).
    LocalBlock { grabbed: TabId, all: &'a [TabId] },
    /// Copies from another window, inserted as one block. The workspace has
    /// none of them: it gives them new ids.
    Incoming {
        copies: &'a [DetachedTab],
        grabbed_index: usize,
    },
}

/// The preview of dropping on `target` (ADR-0021 §4, ADR-0087 §5, ADR-0091
/// §3, §6): `base` cloned with the operation applied. `from_zone`: the dragged
/// tab is a pinned one. `base` is the real workspace, never changed.
pub(crate) fn preview_tabs(
    base: &Workspace,
    arrival: TabsArrival<'_>,
    from_zone: bool,
    target: DragDrop,
) -> TabsPreview {
    let mut preview = base.clone();
    match arrival {
        TabsArrival::Local { tab } => {
            match tab_bar::drop_op(from_zone, target) {
                DropOp::IntoGroup { group, pos } => {
                    preview.move_tab_to_group_at(tab, group, pos);
                }
                DropOp::NewRun { group_index } => {
                    preview.move_tab_to_new_run(tab, group_index);
                }
                DropOp::Pin { pos } => {
                    preview.pin_tab(tab, pos);
                }
                DropOp::ReorderPinned { pos } => {
                    preview.move_pinned(tab, pos);
                }
                DropOp::Unpin(unpin_target) => {
                    preview.unpin_tab(tab, unpin_target);
                }
            }
            TabsPreview {
                workspace: preview,
                grabbed: tab,
                all: vec![tab],
            }
        }
        TabsArrival::LocalBlock { grabbed, all } => {
            // The block lands as one unit (`Workspace::move_tabs`).
            if let Some(at) = tab_bar::block_insert_at(tab_bar::drop_op(false, target)) {
                preview.move_tabs(all, at);
            }
            TabsPreview {
                workspace: preview,
                grabbed,
                all: all.to_vec(),
            }
        }
        TabsArrival::Incoming {
            copies,
            grabbed_index,
        } => {
            let at = tab_bar::incoming_insert_at(tab_bar::drop_op(from_zone, target));
            let all = preview.insert_detached_tabs(copies.to_vec(), at);
            let grabbed = all
                .get(grabbed_index)
                .or(all.first())
                .copied()
                .unwrap_or(ABSENT_TAB);
            TabsPreview {
                workspace: preview,
                grabbed,
                all,
            }
        }
    }
}

/// How the dragged group reaches the workspace the bar belongs to.
pub(crate) enum GroupArrival<'a> {
    Local(GroupId),
    Incoming(&'a DetachedGroup),
}

/// The preview of dropping a group at `index` of the group list (the
/// convention of `group_drag_target_index`): the clone, and the id of the
/// group **in it**.
pub(crate) fn preview_group(
    base: &Workspace,
    arrival: GroupArrival<'_>,
    index: usize,
) -> (Workspace, GroupId) {
    let mut preview = base.clone();
    let group = match arrival {
        GroupArrival::Local(group) => {
            preview.move_group(group, index);
            group
        }
        GroupArrival::Incoming(copy) => preview.insert_detached_group(copy.clone(), index).0,
    };
    (preview, group)
}

/// The boxes of the pinned zone as the drag previews them for `target`, with
/// the slide of `.15s` started when that differs from what `previous` showed
/// (ADR-0087 §5). `dragged` is every tab the drag carries, in the ids of the
/// window the zone belongs to. Returns the order after.
// Eight facts of one preview: a struct would only rename them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn preview_zone(
    animations: &mut AnimationClock,
    now: Instant,
    style: &TabBarStyle,
    is_mac: bool,
    pinned: &[TabId],
    dragged: &[TabId],
    previous: DragDrop,
    target: DragDrop,
) -> Vec<TabId> {
    let zone_before = tab_bar::pinned_preview_order_block(pinned, dragged, previous);
    let zone_after = tab_bar::pinned_preview_order_block(pinned, dragged, target);
    if zone_before != zone_after {
        let from = zone_before
            .iter()
            .enumerate()
            .map(|(slot, &id)| {
                let slot_x = tab_bar::pinned_rect(style, is_mac, slot).x;
                (id, animations.pinned_x(id, slot_x, now))
            })
            .collect();
        animations.start_pinned_slide(from, PINNED_SLIDE_DURATION, now);
    }
    zone_after
}

/// The zone highlight (ADR-0087 §5): only while the target is the zone, over
/// the `slots` boxes of the preview.
pub(crate) fn zone_highlight(
    style: &TabBarStyle,
    is_mac: bool,
    target: DragDrop,
    slots: usize,
) -> Option<Rect> {
    matches!(target, DragDrop::Pinned { .. })
        .then(|| tab_bar::pinned_zone_rect(style, is_mac, slots))
}

// --- the target and the view in a window that receives the drag --------------

/// What the paint and the target of a bar need, besides the workspace.
pub(crate) struct ViewEnv<'a> {
    pub style: &'a TabBarStyle,
    pub is_mac: bool,
    pub bar_width: f32,
    pub now: Instant,
}

/// The center of the ghost, in the coordinates of the bar it is over. The
/// cursor is **already in the logical coordinates of that window**
/// (`cross_window::to_window_logical`, its scale); the grab offset and the
/// width are logical sizes of the ghost, the same in every window.
pub(crate) fn ghost_center(cursor_x: f32, grab_offset: f32, width: f32) -> f32 {
    cursor_x - grab_offset + width / 2.0
}

/// ADR-0091 §3, RF-22.7: the target of `content` in the bar of the window
/// whose layout is `base_layout` and whose pinned zone is `pinned`, with the
/// cursor at `cursor_x` (logical, that window's). The dragged tab is not in
/// the layout, so the target functions get [`ABSENT_TAB`]. A group reads only
/// a border between groups -- never the zone.
///
/// Pure of the redraw: stage 08 calls it on the drop, when no redraw has
/// happened yet, with `fit_width` of the workspace of the window.
pub(crate) fn resolve_incoming_target(
    content: &GhostContent,
    cursor_x: f32,
    base_layout: &TabBarLayout,
    pinned: &[TabId],
    scroll_offset: f32,
    style: &TabBarStyle,
    is_mac: bool,
) -> IncomingTarget {
    let center = ghost_center(cursor_x, content.grab_offset, content.width);
    match content.payload {
        DragPayload::Group(_) => {
            let track_left = tab_bar::track_shift(style, is_mac, pinned.len());
            IncomingTarget::Group {
                index: tab_bar::group_drag_target_index(
                    base_layout,
                    ABSENT_GROUP,
                    center - track_left + scroll_offset,
                ),
            }
        }
        DragPayload::Tabs { .. } | DragPayload::Pinned(_) => {
            IncomingTarget::Tabs(tab_bar::tab_drag_target(
                base_layout,
                style,
                is_mac,
                pinned,
                ABSENT_TAB,
                center,
                scroll_offset,
            ))
        }
    }
}

/// What the redraw of a window that receives a drag paints: the layout of the
/// preview (the gap open where the drag lands), the workspace of the preview
/// (the ghost and the arrivals are in it, not in the real one), the ghost and
/// the highlight.
pub(crate) struct IncomingView {
    pub layout: TabBarLayout,
    pub workspace: Workspace,
    pub ghost: Option<DragGhost>,
    pub group_ghost: Option<GroupDragGhost>,
    pub highlight: Option<(GroupId, Rect)>,
}

/// The paint of the bar of `workspace` (the real one of the destination)
/// receiving `content` at `target` (ADR-0091 §3): preview = a clone with the
/// copies inserted, ghost = the grabbed arrival, kept in the **form of the
/// origin** (a pinned box over the track, a tab over the zone), the other
/// arrivals of a selection hidden as holes. `previous` is the target of the
/// last frame, for the slide of the zone.
// Eight inputs of one paint: a struct would only rename them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn incoming_view(
    content: &GhostContent,
    cursor_x: f32,
    target: IncomingTarget,
    previous: Option<IncomingTarget>,
    workspace: &Workspace,
    animations: &mut AnimationClock,
    env: &ViewEnv<'_>,
    measurer: &mut TextMeasurer,
) -> IncomingView {
    let screen_x = cursor_x - content.grab_offset;
    match (&content.detached, target) {
        (Detached::Tabs(copies), IncomingTarget::Tabs(drop)) => {
            let from_zone = matches!(content.payload, DragPayload::Pinned(_));
            let preview = preview_tabs(
                workspace,
                TabsArrival::Incoming {
                    copies,
                    grabbed_index: content.grabbed_index,
                },
                from_zone,
                drop,
            );
            let layout = tab_bar::fit_width(
                &preview.workspace,
                env.style,
                env.bar_width,
                measurer,
                env.is_mac,
            );
            let pinned: Vec<TabId> = workspace.pinned().to_vec();
            let previous_drop = match previous {
                Some(IncomingTarget::Tabs(previous)) => previous,
                _ => DragDrop::NewRun { group_index: 0 },
            };
            let zone = preview_zone(
                animations,
                env.now,
                env.style,
                env.is_mac,
                &pinned,
                &preview.all,
                previous_drop,
                drop,
            );
            let source = content.source.clone().map(|source| match source {
                GhostSource::Track(mut rect) => {
                    rect.id = preview.grabbed;
                    GhostSource::Track(rect)
                }
                other => other,
            });
            let highlight = tab_bar::drag_highlight_rect(&layout, drop);
            let ghost = source.map(|source| DragGhost {
                tab: preview.grabbed,
                screen_x,
                source,
                zone_highlight: zone_highlight(env.style, env.is_mac, drop, zone.len()),
                pinned_order: zone,
                hidden: preview
                    .all
                    .iter()
                    .copied()
                    .filter(|&id| id != preview.grabbed)
                    .collect(),
                base_pinned: Some(pinned.len()),
            });
            IncomingView {
                layout,
                workspace: preview.workspace,
                ghost,
                group_ghost: None,
                highlight,
            }
        }
        (Detached::Group(copy), IncomingTarget::Group { index }) => {
            let (preview, group) = preview_group(workspace, GroupArrival::Incoming(copy), index);
            let layout =
                tab_bar::fit_width(&preview, env.style, env.bar_width, measurer, env.is_mac);
            IncomingView {
                layout,
                workspace: preview,
                ghost: None,
                group_ghost: Some(GroupDragGhost { group, screen_x }),
                highlight: None,
            }
        }
        // A target of the wrong kind cannot come from `resolve_incoming_target`;
        // paint the bar as it is rather than guess.
        _ => IncomingView {
            layout: tab_bar::fit_width(workspace, env.style, env.bar_width, measurer, env.is_mac),
            workspace: workspace.clone(),
            ghost: None,
            group_ghost: None,
            highlight: None,
        },
    }
}

// --- the origin, while the drag is elsewhere ---------------------------------

/// What the origin draws while the ghost is over the bar of another window:
/// only the **hole** (ADR-0091 §3), no ghost, no gap, no highlight.
pub(crate) struct HoleView {
    pub layout: TabBarLayout,
    /// `Some` only when a pinned box left: the zone keeps its slot as a hole
    /// (like the local drag), and nothing else is painted.
    pub ghost: Option<DragGhost>,
}

/// The origin bar with the dragged payload gone: the preview is the real
/// workspace minus the tabs or the group (a clone plus `take_*`; the real one
/// is not touched). Pinned boxes are the exception to "the neighbours close
/// the gap": they stay as an empty slot, as in the local drag, so the track
/// does not move under the cursor.
pub(crate) fn hole_view(
    payload: &DragPayload,
    workspace: &Workspace,
    base_layout: &TabBarLayout,
    env: &ViewEnv<'_>,
    measurer: &mut TextMeasurer,
) -> HoleView {
    let mut without = workspace.clone();
    match payload {
        DragPayload::Pinned(tab) => {
            return HoleView {
                layout: base_layout.clone(),
                ghost: Some(DragGhost {
                    tab: *tab,
                    screen_x: 0.0,
                    source: GhostSource::Hole,
                    pinned_order: workspace.pinned().to_vec(),
                    zone_highlight: None,
                    hidden: Vec::new(),
                    base_pinned: None,
                }),
            };
        }
        DragPayload::Tabs { all, .. } => {
            without.take_tabs(all);
        }
        DragPayload::Group(group) => {
            without.take_group(*group);
        }
    }
    HoleView {
        layout: tab_bar::fit_width(&without, env.style, env.bar_width, measurer, env.is_mac),
        ghost: None,
    }
}

// --- the detach -----------------------------------------------------------------

/// ADR-0091 §5: the logical `x`, in the bar of the window a detach creates, of
/// the element the user holds (the grabbed tab, the pinned box or the group
/// pill) -- known before the window exists, because the layout is pure. The
/// copies go into an empty `Workspace` the way `App::transfer` inserts them in
/// a window born for them (RF-22.14: the end of the track for tabs, the first
/// slot of the zone for a pinned tab, the first slot of the group list for a
/// group), and the position comes from the layout functions, never from a
/// formula of its own. `bar_width` is the logical width of the new window.
pub(crate) fn detached_element_x(
    content: &GhostContent,
    style: &TabBarStyle,
    is_mac: bool,
    bar_width: f32,
    measurer: &mut TextMeasurer,
) -> f32 {
    // The track starts after the app button; nothing is pinned in a window
    // that is born for tabs or for a group.
    let track_left = tab_bar::track_shift(style, is_mac, 0);
    let mut workspace = Workspace::new();
    match (&content.detached, &content.payload) {
        // A pinned box is the first of the fixed row.
        (Detached::Tabs(_), DragPayload::Pinned(_)) => tab_bar::pinned_rect(style, is_mac, 0).x,
        (Detached::Tabs(copies), _) => {
            let ids = workspace.insert_detached_tabs(copies.clone(), InsertAt::TrackEnd);
            let grabbed = ids.get(content.grabbed_index).or(ids.first()).copied();
            let layout = tab_bar::fit_width(&workspace, style, bar_width, measurer, is_mac);
            grabbed
                .and_then(|id| tab_bar::tab_rect(&layout, id))
                .map_or(track_left, |rect| rect.x + track_left)
        }
        (Detached::Group(copy), _) => {
            let (group, _) = workspace.insert_detached_group(copy.clone(), 0);
            let layout = tab_bar::fit_width(&workspace, style, bar_width, measurer, is_mac);
            tab_bar::pill_rect(&layout, group).map_or(track_left, |rect| rect.x + track_left)
        }
    }
}

// --- App ----------------------------------------------------------------------

/// The global point of the cursor of `state` (ADR-0091 §1): the position of
/// the client area plus the cursor, both physical. `None` where the position
/// of the window is unknown (Wayland: `inner_position()` errs), and the
/// gesture then behaves as it did before windows could be a target
/// (RF-22.29).
fn drag_global_point(state: &WindowState) -> Option<(f64, f64)> {
    let position = state.window.inner_position().ok()?;
    cross_window::global_point(Some((position.x, position.y)), state.cursor_position)
}

/// The geometry of `window` as a candidate of `cross_window::locate`; `None`
/// when its position is unknown.
fn candidate_of(id: WindowId, window: &Window, kind: WindowKind) -> Option<Candidate<WindowId>> {
    let position = window.inner_position().ok()?;
    let size = window.inner_size();
    Some(Candidate {
        id,
        kind,
        client_origin: (position.x, position.y),
        client_size: (size.width, size.height),
        scale: window.scale_factor(),
        minimized: window.is_minimized() == Some(true),
    })
}

impl App {
    /// The windows the cursor may be over, in the order of the most recent
    /// focus, the origin included (it is on top, being the focused one): the
    /// terminal windows, then -- as `Other`, never a target but still "inside
    /// a Porecatu window" -- the settings window and the profile editor.
    fn cross_candidates(&self, origin: WindowId) -> Vec<Candidate<WindowId>> {
        let mut order = self.windows_by_recent_focus();
        if !order.contains(&origin) {
            order.insert(0, origin);
        }
        let mut candidates: Vec<Candidate<WindowId>> = order
            .into_iter()
            .filter_map(|id| {
                let state = self.windows.get(&id)?;
                candidate_of(id, &state.window, WindowKind::Terminal)
            })
            .collect();
        if let Some(settings) = &self.settings {
            candidates.extend(candidate_of(
                settings.window_id(),
                settings.window(),
                WindowKind::Other,
            ));
            if let (Some(id), Some(window)) = (
                settings.profile_editor_id(),
                settings.profile_editor_window(),
            ) {
                candidates.extend(candidate_of(id, window, WindowKind::Other));
            }
        }
        candidates
    }

    /// The press of `origin` just crossed the threshold (`press_start` is the
    /// logical point of the press): builds the content the other windows will
    /// draw, once. Without it -- a tab in a collapsed group, an implicit
    /// group -- the drag stays local.
    fn begin_cross_drag(&mut self, origin: WindowId, press_start: (f32, f32)) {
        self.cross_drag = None;
        let (Some(gpu), Some(state)) = (self.gpu.as_mut(), self.windows.get(&origin)) else {
            return;
        };
        let base_layout = tab_bar::fit_width(
            &state.workspace,
            &self.style,
            state.logical_width,
            gpu.text_measurer(),
            is_macos(),
        );
        let content = match state.drag {
            Drag::TabDragging {
                tab, grab_offset, ..
            } => build_tab_content(
                &state.workspace,
                tab,
                &state.drag_block,
                grab_offset,
                press_start.1,
                &base_layout,
                &self.style,
                self.pinned.key_of(origin, tab),
            ),
            Drag::GroupDragging {
                group, grab_offset, ..
            } => build_group_content(
                &state.workspace,
                group,
                grab_offset,
                press_start.1,
                &base_layout,
            ),
            _ => None,
        };
        self.cross_drag = content.map(|content| CrossDrag {
            origin,
            target: None,
            ghost: Rc::new(content),
        });
    }

    /// Every `CursorMoved` of `origin` with a tab or group drag alive (ADR-0091
    /// §1, §2): `press_start` is `Some` on the very move that promoted the
    /// press to a drag. Finds the window under the cursor, hands the ghost to
    /// the bar it is over (or takes it back), and scrolls the track of the bar
    /// the ghost is on when the cursor is at its edge.
    pub(super) fn cross_drag_moved(&mut self, origin: WindowId, press_start: Option<(f32, f32)>) {
        if let Some(start) = press_start {
            self.begin_cross_drag(origin, start);
        }
        let style = self.style;
        let over = match (
            &self.cross_drag,
            self.windows.get(&origin).and_then(drag_global_point),
        ) {
            (Some(cross), Some(global)) if cross.origin == origin => cross_window::locate(
                global,
                origin,
                &self.cross_candidates(origin),
                bar_height(&style),
            ),
            // No drag across windows, or no global point (RF-22.29): the ghost
            // stays on the bar of the origin, as before.
            _ => PointOver::Outside,
        };
        let previous = self.cross_drag.as_ref().and_then(|cross| cross.target);
        let step = cross_window::retarget(previous, &over, origin);

        if let Some(window) = step.clear
            && let Some(state) = self.windows.get_mut(&window)
        {
            state.incoming = None;
            // A slide of the zone the ghost started dies with it, as the
            // origin's does on a drop.
            state.animations.clear();
        }
        if let Some((window, logical)) = step.set
            && let Some(content) = self.cross_drag.as_ref().map(|cross| cross.ghost.clone())
            && let Some(state) = self.windows.get_mut(&window)
        {
            match &mut state.incoming {
                Some(incoming) => incoming.logical = logical,
                None => {
                    state.incoming = Some(IncomingDrag {
                        content,
                        logical,
                        target: None,
                    });
                }
            }
            state.drag_autoscroll(logical.0, &style);
        }
        if let Some(cross) = &mut self.cross_drag {
            cross.target = step.set.map(|(window, _)| window);
        }
        if let Some(state) = self.windows.get_mut(&origin) {
            state.drag_elsewhere = step.elsewhere;
            if step.set.is_none() && press_start.is_none() {
                let logical_x = state.cursor_position.0 as f32 / state.scale;
                state.drag_autoscroll(logical_x, &style);
            }
        }
        for window in step.redraw {
            if let Some(state) = self.windows.get(&window) {
                state.window.request_redraw();
            }
        }
    }

    /// The drag ended -- a drop, `Esc`, the origin lost the focus (ADR-0091
    /// §7): no window keeps an incoming ghost or shows only the hole, and each
    /// one that did redraws to erase it.
    pub(super) fn cancel_cross_drag(&mut self) {
        self.cross_drag = None;
        for state in self.windows.values_mut() {
            let had_ghost = state.incoming.take().is_some();
            let was_hole = std::mem::take(&mut state.drag_elsewhere);
            if had_ghost {
                state.animations.clear();
            }
            if had_ghost || was_hole {
                state.window.request_redraw();
            }
        }
    }

    /// The left button went up with a drag alive in `window_id` (ADR-0091 §4):
    /// decides by the global point of the cursor, **now**, where the drop
    /// goes -- the bar of the origin (the local drop of always), the bar of
    /// another window (`App::transfer` to it), anywhere else inside a Porecatu
    /// window (cancels) or outside every one (a window is born for the
    /// payload). Without a global point, or with nothing to carry across
    /// windows, only the local drop exists. Every path ends with no window
    /// showing an incoming ghost or a hole, and the cursor back to normal.
    pub(super) fn release_drag(&mut self, window_id: WindowId, event_loop: &ActiveEventLoop) {
        let style = self.style;
        let content = self
            .cross_drag
            .as_ref()
            .filter(|cross| cross.origin == window_id)
            .map(|cross| cross.ghost.clone());
        let Some(state) = self.windows.get(&window_id) else {
            return;
        };
        let global = content.as_ref().and_then(|_| drag_global_point(state));
        let over = global.map(|global| {
            cross_window::locate(
                global,
                window_id,
                &self.cross_candidates(window_id),
                bar_height(&style),
            )
        });
        let cursor = (
            state.cursor_position.0 as f32 / state.scale,
            state.cursor_position.1 as f32 / state.scale,
        );
        let in_origin_bar =
            cross_window::point_in_bar(cursor, state.logical_width, bar_height(&style));
        let decision = cross_window::decide_drop(over, in_origin_bar);

        let transfer = match (decision, content, global) {
            (DropDecision::Transfer(target), Some(content), _) => {
                let cursor_x = match over {
                    Some(PointOver::OtherBar { logical, .. }) => logical.0,
                    _ => cursor.0,
                };
                self.resolve_drop_at(target, &content, cursor_x)
                    .map(|at| (content, TransferDest::Window(target, at)))
            }
            (DropDecision::Detach, Some(content), Some(global)) => {
                let placement = self.detach_placement(event_loop, window_id, global, &content);
                Some((content, TransferDest::NewWindow(placement)))
            }
            _ => None,
        };

        let Some((content, dest)) = transfer else {
            // The local drop, or a cancel (the drop decided to cancel, or the
            // transfer has nowhere valid to go): `finish_drag` applies the
            // target of the last frame only when `on_bar`.
            let mut zone_drop = None;
            if let (Some(gpu), Some(state)) = (self.gpu.as_mut(), self.windows.get_mut(&window_id))
            {
                zone_drop = state.finish_drag(gpu, &style, decision == DropDecision::Local);
                state.window.request_redraw();
            }
            self.cancel_cross_drag();
            if let Some((tabs, op)) = zone_drop {
                self.apply_zone_drop(window_id, &tabs, op);
            }
            return;
        };

        // A transfer: the origin goes to `Idle` with no local operation.
        if let Some(state) = self.windows.get_mut(&window_id) {
            state.drag = Drag::Idle;
            state.drag_block.clear();
            state.animations.clear();
            state.window.set_cursor(CursorIcon::Default);
            state.window.request_redraw();
        }
        self.cancel_cross_drag();
        let payload = match &content.payload {
            DragPayload::Tabs { grabbed, all } => TransferPayload::Tabs {
                ids: all.clone(),
                grabbed: Some(*grabbed),
            },
            DragPayload::Group(group) => TransferPayload::Group(*group),
            DragPayload::Pinned(tab) => TransferPayload::Pinned(*tab),
        };
        // `None` is a refusal with no effect (ADR-0091 §7): nothing moved, and
        // the tab is where it was.
        let _ = self.transfer(event_loop, window_id, payload, dest);
    }

    /// Where `content` lands in the bar of `target` for a drop with the
    /// cursor at `cursor_x` (that window's logical): the target the last
    /// redraw of `target` resolved, or -- when no redraw came between the last
    /// move and the release -- resolved now, against the layout of `target`.
    fn resolve_drop_at(
        &mut self,
        target: WindowId,
        content: &GhostContent,
        cursor_x: f32,
    ) -> Option<InsertAt> {
        let style = self.style;
        let from_zone = matches!(content.payload, DragPayload::Pinned(_));
        let drawn = self
            .windows
            .get(&target)
            .and_then(|state| state.incoming.as_ref())
            .and_then(|incoming| incoming.target);
        let resolved = match drawn {
            Some(target) => target,
            None => {
                let (gpu, dest) = (self.gpu.as_mut()?, self.windows.get(&target)?);
                let layout = tab_bar::fit_width(
                    &dest.workspace,
                    &style,
                    dest.logical_width,
                    gpu.text_measurer(),
                    is_macos(),
                );
                resolve_incoming_target(
                    content,
                    cursor_x,
                    &layout,
                    dest.workspace.pinned(),
                    dest.scroll_offset,
                    &style,
                    is_macos(),
                )
            }
        };
        Some(resolved.insert_at(from_zone))
    }

    /// ADR-0091 §5: where and how big the window a detach creates is. The
    /// size is the one `detach_size` gives (`None`: the platform default); the
    /// position puts the grabbed point under the cursor, on the monitor that
    /// holds `global` (the one of the origin if none does, and no clamping if
    /// the origin has none either).
    fn detach_placement(
        &mut self,
        event_loop: &ActiveEventLoop,
        origin: WindowId,
        global: (f64, f64),
        content: &GhostContent,
    ) -> NewWindowPlacement {
        let style = self.style;
        let (Some(state), Some(gpu)) = (self.windows.get(&origin), self.gpu.as_mut()) else {
            return NewWindowPlacement::Cascade;
        };
        let size = state.detach_size();
        // With the platform default size, the current one only bounds the
        // strip of the bar (ADR-0091 §5).
        let inner = state.window.inner_size();
        let window_size = size.unwrap_or(inner);

        let monitor_of = |monitor: &winit::monitor::MonitorHandle| {
            let (position, size) = (monitor.position(), monitor.size());
            (
                ((position.x, position.y), (size.width, size.height)),
                monitor.scale_factor(),
            )
        };
        let monitors: Vec<(MonitorRect, f64)> = event_loop
            .available_monitors()
            .map(|monitor| monitor_of(&monitor))
            .collect();
        let rects: Vec<MonitorRect> = monitors.iter().map(|&(rect, _)| rect).collect();
        let (monitor, scale) = cross_window::monitor_containing(global, &rects)
            .map(|index| monitors[index])
            .or_else(|| state.window.current_monitor().map(|m| monitor_of(&m)))
            .map_or((None, state.window.scale_factor()), |(rect, scale)| {
                (Some(rect), scale)
            });

        let element_x = detached_element_x(
            content,
            &style,
            is_macos(),
            window_size.width as f32 / scale as f32,
            gpu.text_measurer(),
        );
        let (x, y) = cross_window::detach_position(
            global,
            element_x,
            content.grab_offset,
            content.press_y,
            scale,
            (window_size.width, window_size.height),
            monitor,
            bar_height(&style),
        );
        NewWindowPlacement::At {
            position: winit::dpi::PhysicalPosition::new(x, y),
            size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use porecatu_core::GroupColor;

    static STYLE: TabBarStyle = TabBarStyle::DEFAULT;

    fn ws_of(n: usize) -> (Workspace, Vec<TabId>) {
        let mut ws = Workspace::new();
        let tabs: Vec<TabId> = (0..n)
            .map(|_| ws.append_ungrouped_tab("zsh", None))
            .collect();
        (ws, tabs)
    }

    fn layout_of(ws: &Workspace, m: &mut TextMeasurer) -> TabBarLayout {
        tab_bar::fit_width(ws, &STYLE, 800.0, m, false)
    }

    fn env() -> ViewEnv<'static> {
        ViewEnv {
            style: &STYLE,
            is_mac: false,
            bar_width: 800.0,
            now: Instant::now(),
        }
    }

    /// Origin with six tabs: `a = [0,1]`, `b = [2,3]`, `c = [4,5]` (explicit
    /// groups) and the content of a drag of `t[3]` alone.
    fn origin_with_groups() -> (Workspace, Vec<TabId>, [GroupId; 3]) {
        let (mut ws, t) = ws_of(6);
        let a = ws.group_tabs(&t[0..2], "a", GroupColor::Cyan).unwrap();
        let b = ws.group_tabs(&t[2..4], "b", GroupColor::Green).unwrap();
        let c = ws.group_tabs(&t[4..6], "c", GroupColor::Red).unwrap();
        (ws, t, [a, b, c])
    }

    /// A destination window whose own ids collide with the origin's: both
    /// start at 0.
    fn destination() -> (Workspace, Vec<TabId>, GroupId) {
        let (mut ws, t) = ws_of(3);
        let g = ws.group_tabs(&t[0..2], "g", GroupColor::Yellow).unwrap();
        (ws, t, g)
    }

    fn tab_content(ws: &Workspace, tab: TabId, block: &[TabId]) -> GhostContent {
        let mut m = TextMeasurer::new();
        let layout = layout_of(ws, &mut m);
        build_tab_content(ws, tab, block, 10.0, 12.0, &layout, &STYLE, None).unwrap()
    }

    fn cursor_for_center(content: &GhostContent, center: f32) -> f32 {
        center + content.grab_offset - content.width / 2.0
    }

    // --- ghost_center ---

    #[test]
    fn the_ghost_center_is_the_cursor_minus_the_grab_plus_half_the_width() {
        assert_eq!(ghost_center(200.0, 30.0, 100.0), 220.0);
    }

    /// The cursor reaches the destination already converted by the scale of
    /// the **destination**: the same global point is a different logical `x`
    /// in a window at another scale, and so is the center of the ghost, whose
    /// grab offset and width are the same logical sizes.
    #[test]
    fn the_ghost_center_in_a_destination_of_another_scale() {
        let global = (1500.0, 10.0);
        // The origin at (0,0), scale 1.0; the destination at (1200,0), 1.5.
        let in_origin = cross_window::to_window_logical(global, (0, 0), 1.0).0;
        let in_destination = cross_window::to_window_logical(global, (1200, 0), 1.5).0;
        assert_eq!((in_origin, in_destination), (1500.0, 200.0));
        assert_eq!(ghost_center(in_destination, 30.0, 100.0), 220.0);
        assert_ne!(
            ghost_center(in_origin, 30.0, 100.0),
            ghost_center(in_destination, 30.0, 100.0)
        );
    }

    // --- the content ---

    #[test]
    fn the_content_of_a_lone_track_tab() {
        let (ws, t, _) = origin_with_groups();
        let content = tab_content(&ws, t[3], &[]);
        assert_eq!(
            content.payload,
            DragPayload::Tabs {
                grabbed: t[3],
                all: vec![t[3]]
            }
        );
        let Detached::Tabs(copies) = &content.detached else {
            panic!("tabs expected");
        };
        assert_eq!(copies.len(), 1);
        assert_eq!(content.grabbed_index, 0);
        assert!(matches!(content.source, Some(GhostSource::Track(_))));
        assert_eq!((content.grab_offset, content.press_y), (10.0, 12.0));
        // The real workspace is untouched.
        assert_eq!(ws.track_order().collect::<Vec<_>>(), t);
    }

    #[test]
    fn the_content_of_a_selection_keeps_the_visual_order_and_the_grabbed_index() {
        let (ws, t, _) = origin_with_groups();
        // The grabbed tab is the middle one in the visual order.
        let content = tab_content(&ws, t[3], &[t[5], t[0]]);
        assert_eq!(
            content.payload,
            DragPayload::Tabs {
                grabbed: t[3],
                all: vec![t[0], t[3], t[5]]
            }
        );
        let Detached::Tabs(copies) = &content.detached else {
            panic!("tabs expected");
        };
        assert_eq!(copies.len(), 3);
        assert_eq!(content.grabbed_index, 1);
    }

    #[test]
    fn the_content_of_a_pinned_box_is_the_box() {
        let (mut ws, t) = ws_of(3);
        ws.pin_tab(t[1], 0);
        let content = tab_content(&ws, t[1], &[]);
        assert_eq!(content.payload, DragPayload::Pinned(t[1]));
        assert!(matches!(content.source, Some(GhostSource::Pinned)));
        assert_eq!(content.width, tab_bar::app_menu_button_size(&STYLE));
        assert_eq!(ws.pinned(), [t[1]], "the real workspace keeps it");
    }

    #[test]
    fn the_content_of_a_group_and_of_what_is_not_draggable() {
        let (mut ws, t, [a, ..]) = origin_with_groups();
        let mut m = TextMeasurer::new();
        let layout = layout_of(&ws, &mut m);
        let content = build_group_content(&ws, a, 5.0, 9.0, &layout).unwrap();
        assert_eq!(content.payload, DragPayload::Group(a));
        let Detached::Group(copy) = &content.detached else {
            panic!("a group expected");
        };
        assert_eq!(copy.tabs().len(), 2);
        assert!(content.source.is_none());
        assert_eq!(content.width, tab_bar::pill_rect(&layout, a).unwrap().width);
        // An implicit run has no pill: the drag stays local.
        let (loose, _) = ws_of(2);
        let loose_layout = layout_of(&loose, &mut m);
        let run = loose.groups()[0].id();
        assert!(build_group_content(&loose, run, 0.0, 0.0, &loose_layout).is_none());
        // A tab inside a collapsed group has no rectangle: stays local too.
        ws.collapse_group(a, true);
        let layout = layout_of(&ws, &mut m);
        assert!(build_tab_content(&ws, t[0], &[], 0.0, 0.0, &layout, &STYLE, None).is_none());
    }

    // --- preview_tabs: the order that results ---

    #[test]
    fn a_tab_arriving_into_a_group_lands_where_the_target_says() {
        let (origin, ot, _) = origin_with_groups();
        let (dest, dt, g) = destination();
        let content = tab_content(&origin, ot[3], &[]);
        let Detached::Tabs(copies) = &content.detached else {
            panic!("tabs expected");
        };
        let arrival = |at| {
            preview_tabs(
                &dest,
                TabsArrival::Incoming {
                    copies,
                    grabbed_index: 0,
                },
                false,
                at,
            )
        };
        // Between the two tabs of `g`.
        let p = arrival(DragDrop::IntoGroup { group: g, pos: 1 });
        let order: Vec<TabId> = p.workspace.track_order().collect();
        assert_eq!(order.len(), 4);
        assert_eq!((order[0], order[2], order[3]), (dt[0], dt[1], dt[2]));
        assert_eq!(order[1], p.grabbed, "the arrival is between them");
        assert_eq!(p.all, [p.grabbed]);
        assert_eq!(p.workspace.group_of_tab(p.grabbed), Some(g));
        assert!(
            !dt.contains(&p.grabbed),
            "the id is the destination's, never the origin's"
        );
        // The real destination is not touched.
        assert_eq!(dest.track_order().count(), 3);

        // A new run at the front: loose, before everything.
        let p = arrival(DragDrop::NewRun { group_index: 0 });
        assert_eq!(p.workspace.track_order().next(), Some(p.grabbed));
        assert_eq!(
            p.workspace.group_of_tab(p.grabbed).map(|id| p
                .workspace
                .group(id)
                .unwrap()
                .is_implicit()),
            Some(true)
        );
    }

    #[test]
    fn a_selection_arriving_lands_contiguous_in_its_order() {
        let (origin, ot, _) = origin_with_groups();
        let (dest, dt, g) = destination();
        let content = tab_content(&origin, ot[3], &[ot[5], ot[0]]);
        let Detached::Tabs(copies) = &content.detached else {
            panic!("tabs expected");
        };
        let p = preview_tabs(
            &dest,
            TabsArrival::Incoming {
                copies,
                grabbed_index: content.grabbed_index,
            },
            false,
            DragDrop::IntoGroup { group: g, pos: 1 },
        );
        let order: Vec<TabId> = p.workspace.track_order().collect();
        // `dt[0]`, the three arrivals in the visual order of the origin,
        // `dt[1]`, then the loose `dt[2]`.
        assert_eq!(order.len(), 6);
        assert_eq!(order[0], dt[0]);
        assert_eq!(&order[1..4], p.all.as_slice());
        assert_eq!((order[4], order[5]), (dt[1], dt[2]));
        assert_eq!(p.grabbed, p.all[1], "the grabbed one is the middle arrival");
    }

    #[test]
    fn a_pinned_box_arriving_keeps_its_form_by_the_target() {
        let (mut origin, ot) = ws_of(2);
        origin.pin_tab(ot[0], 0);
        let content = tab_content(&origin, ot[0], &[]);
        let Detached::Tabs(copies) = &content.detached else {
            panic!("tabs expected");
        };
        let (mut dest, dt, g) = destination();
        dest.pin_tab(dt[2], 0);
        let arrive = |dest: &Workspace, at| {
            preview_tabs(
                dest,
                TabsArrival::Incoming {
                    copies,
                    grabbed_index: 0,
                },
                true,
                at,
            )
        };
        // On the zone: arrives pinned, at the position.
        let p = arrive(&dest, DragDrop::Pinned { pos: 0 });
        assert_eq!(p.workspace.pinned(), [p.grabbed, dt[2]]);
        let p = arrive(&dest, DragDrop::Pinned { pos: 1 });
        assert_eq!(p.workspace.pinned(), [dt[2], p.grabbed]);
        // On the track: arrives loose.
        let p = arrive(&dest, DragDrop::IntoGroup { group: g, pos: 0 });
        assert_eq!(p.workspace.pinned(), [dt[2]]);
        assert_eq!(p.workspace.group_of_tab(p.grabbed), Some(g));
        assert_eq!(p.workspace.group(g).unwrap().tabs()[0], p.grabbed);
    }

    #[test]
    fn the_local_arrivals_are_the_operations_the_bar_always_did() {
        let (ws, t, [a, b, _]) = origin_with_groups();
        // A lone tab, `drop_op` decides.
        let p = preview_tabs(
            &ws,
            TabsArrival::Local { tab: t[0] },
            false,
            DragDrop::IntoGroup { group: b, pos: 2 },
        );
        assert_eq!(p.grabbed, t[0]);
        assert_eq!(p.workspace.group(b).unwrap().tabs(), [t[2], t[3], t[0]]);
        assert_eq!(p.workspace.group(a).unwrap().tabs(), [t[1]]);
        // A block, `move_tabs`: ids kept.
        let p = preview_tabs(
            &ws,
            TabsArrival::LocalBlock {
                grabbed: t[1],
                all: &[t[0], t[1]],
            },
            false,
            DragDrop::IntoGroup { group: b, pos: 0 },
        );
        assert_eq!(p.all, [t[0], t[1]]);
        assert_eq!(
            p.workspace.group(b).unwrap().tabs(),
            [t[0], t[1], t[2], t[3]]
        );
        // The real workspace is never the one changed.
        assert_eq!(ws.group(a).unwrap().tabs(), [t[0], t[1]]);
    }

    #[test]
    fn a_group_arriving_falls_on_a_border_between_groups() {
        let (origin, _, [a, ..]) = origin_with_groups();
        let mut m = TextMeasurer::new();
        let layout = layout_of(&origin, &mut m);
        let content = build_group_content(&origin, a, 0.0, 0.0, &layout).unwrap();
        let Detached::Group(copy) = &content.detached else {
            panic!("a group expected");
        };
        let (dest, dt, g) = destination();
        // Destination: `g = [0,1]` then the loose `2`. Index 1: between them.
        let (preview, id) = preview_group(&dest, GroupArrival::Incoming(copy), 1);
        let ids: Vec<GroupId> = preview.groups().iter().map(|group| group.id()).collect();
        assert_eq!(ids.len(), 3);
        assert_eq!((ids[0], ids[1]), (g, id));
        assert_eq!(preview.group(id).unwrap().name(), Some("a"));
        assert_eq!(preview.group(id).unwrap().tabs().len(), 2);
        assert_eq!(preview.track_order().count(), 5);
        assert_eq!(preview.group(g).unwrap().tabs(), [dt[0], dt[1]]);
        // Local: the move the bar always did.
        let (moved, same) = preview_group(&origin, GroupArrival::Local(a), 2);
        assert_eq!(same, a);
        assert_eq!(moved.groups().last().unwrap().id(), a);
    }

    // --- resolve_incoming_target ---

    #[test]
    fn the_target_in_the_destination_reads_the_layout_with_the_grabbed_tab_absent() {
        let (origin, ot, _) = origin_with_groups();
        let content = tab_content(&origin, ot[3], &[]);
        let (dest, _, g) = destination();
        let mut m = TextMeasurer::new();
        let layout = layout_of(&dest, &mut m);
        let pinned = dest.pinned().to_vec();
        let shift = tab_bar::track_shift(&STYLE, false, 0);
        let resolve = |center: f32| {
            resolve_incoming_target(
                &content,
                cursor_for_center(&content, center),
                &layout,
                &pinned,
                0.0,
                &STYLE,
                false,
            )
        };
        let first = layout.groups[0].tabs[0].rect;
        let second = layout.groups[0].tabs[1].rect;
        // Left half of the first tab of `g`: the front. The numeric id of the
        // grabbed tab (`TabId(3)` at the origin) skips nothing here.
        assert_eq!(
            resolve(shift + first.x + 1.0),
            IncomingTarget::Tabs(DragDrop::IntoGroup { group: g, pos: 0 })
        );
        // Right of the center of the first tab, left of the second's.
        assert_eq!(
            resolve(shift + first.x + first.width / 2.0 + 1.0),
            IncomingTarget::Tabs(DragDrop::IntoGroup { group: g, pos: 1 })
        );
        assert_eq!(
            resolve(shift + second.x + second.width / 2.0 + 1.0),
            IncomingTarget::Tabs(DragDrop::IntoGroup { group: g, pos: 2 })
        );
        // Past the last wrapper: a new run at the end.
        let end = layout.groups.last().unwrap().rect;
        assert_eq!(
            resolve(shift + end.x + end.width + 50.0),
            IncomingTarget::Tabs(DragDrop::NewRun {
                group_index: layout.groups.len()
            })
        );
    }

    #[test]
    fn tabs_and_boxes_reach_the_zone_but_a_group_never_does() {
        let (mut origin, ot) = ws_of(2);
        origin.pin_tab(ot[0], 0);
        let boxed = tab_content(&origin, ot[0], &[]);
        let (origin2, ot2, [a, ..]) = origin_with_groups();
        let mut m = TextMeasurer::new();
        let group =
            build_group_content(&origin2, a, 0.0, 0.0, &layout_of(&origin2, &mut m)).unwrap();
        let tab = tab_content(&origin2, ot2[3], &[]);
        let (dest, _, _) = destination();
        let layout = layout_of(&dest, &mut m);
        // The ghost center over the app menu button, left of the track.
        for content in [&boxed, &tab] {
            let target = resolve_incoming_target(
                content,
                cursor_for_center(content, 5.0),
                &layout,
                &[],
                0.0,
                &STYLE,
                false,
            );
            assert_eq!(
                target,
                IncomingTarget::Tabs(DragDrop::Pinned { pos: 0 }),
                "{:?}",
                content.payload
            );
        }
        let target = resolve_incoming_target(
            &group,
            cursor_for_center(&group, 5.0),
            &layout,
            &[],
            0.0,
            &STYLE,
            false,
        );
        assert_eq!(
            target,
            IncomingTarget::Group { index: 0 },
            "a group over the zone is the front border"
        );
    }

    #[test]
    fn the_group_target_counts_every_wrapper_of_the_destination() {
        let (origin, _, [a, ..]) = origin_with_groups();
        let mut m = TextMeasurer::new();
        let content =
            build_group_content(&origin, a, 0.0, 0.0, &layout_of(&origin, &mut m)).unwrap();
        let (dest, _, _) = destination();
        let layout = layout_of(&dest, &mut m);
        assert_eq!(layout.groups.len(), 2, "`g` and the loose run");
        let shift = tab_bar::track_shift(&STYLE, false, 0);
        let index_at = |center: f32| {
            resolve_incoming_target(
                &content,
                cursor_for_center(&content, center),
                &layout,
                &[],
                0.0,
                &STYLE,
                false,
            )
        };
        let (g, run) = (&layout.groups[0], &layout.groups[1]);
        assert_eq!(
            index_at(shift + g.rect.x + 1.0),
            IncomingTarget::Group { index: 0 }
        );
        assert_eq!(
            index_at(shift + run.rect.x + 1.0),
            IncomingTarget::Group { index: 1 }
        );
        assert_eq!(
            index_at(shift + run.rect.x + run.rect.width + 99.0),
            IncomingTarget::Group { index: 2 }
        );
    }

    #[test]
    fn the_scroll_of_the_destination_moves_the_target() {
        let (origin, ot, _) = origin_with_groups();
        let content = tab_content(&origin, ot[3], &[]);
        let (dest, _, g) = destination();
        let mut m = TextMeasurer::new();
        let layout = layout_of(&dest, &mut m);
        let shift = tab_bar::track_shift(&STYLE, false, 0);
        let second = layout.groups[0].tabs[1].rect;
        // The cursor points at the content of the first tab; scrolled by the
        // width of the first, it points at the second: the target moves.
        let target_at = |scroll: f32| {
            resolve_incoming_target(
                &content,
                cursor_for_center(&content, shift + second.x + 1.0),
                &layout,
                &[],
                scroll,
                &STYLE,
                false,
            )
        };
        assert_eq!(
            target_at(0.0),
            IncomingTarget::Tabs(DragDrop::IntoGroup { group: g, pos: 1 })
        );
        assert_eq!(
            target_at(second.width / 2.0 + 5.0),
            IncomingTarget::Tabs(DragDrop::IntoGroup { group: g, pos: 2 })
        );
    }

    #[test]
    fn insert_at_is_the_translation_the_drop_will_use() {
        let group = GroupId::new(4);
        let tabs = |drop| IncomingTarget::Tabs(drop);
        assert_eq!(
            tabs(DragDrop::IntoGroup { group, pos: 1 }).insert_at(false),
            InsertAt::IntoGroup { group, pos: 1 }
        );
        assert_eq!(
            tabs(DragDrop::Pinned { pos: 2 }).insert_at(true),
            InsertAt::Pinned { pos: 2 }
        );
        assert_eq!(
            tabs(DragDrop::NewRun { group_index: 3 }).insert_at(true),
            InsertAt::NewRun { group_index: 3 }
        );
        assert_eq!(
            IncomingTarget::Group { index: 5 }.insert_at(false),
            InsertAt::NewRun { group_index: 5 }
        );
    }

    // --- the views ---

    #[test]
    fn the_incoming_view_is_the_destination_with_the_gap_and_the_ghost() {
        let (origin, ot, _) = origin_with_groups();
        let content = tab_content(&origin, ot[3], &[ot[5], ot[0]]);
        let (mut dest, dt, g) = destination();
        dest.pin_tab(dt[2], 0);
        let mut m = TextMeasurer::new();
        let mut animations = AnimationClock::default();
        let target = IncomingTarget::Tabs(DragDrop::IntoGroup { group: g, pos: 1 });
        let view = incoming_view(
            &content,
            200.0,
            target,
            None,
            &dest,
            &mut animations,
            &env(),
            &mut m,
        );
        let ghost = view.ghost.expect("a ghost");
        assert_eq!(ghost.screen_x, 200.0 - content.grab_offset);
        assert_eq!(ghost.hidden.len(), 2, "the other two of the selection");
        assert!(!ghost.hidden.contains(&ghost.tab));
        assert!(
            view.workspace.tab(ghost.tab).is_some(),
            "the ghost is a tab of the preview"
        );
        assert!(
            dest.tab(ghost.tab).is_none(),
            "and not of the real destination"
        );
        assert_eq!(
            ghost.base_pinned,
            Some(1),
            "the track shift reads the real zone"
        );
        match &ghost.source {
            GhostSource::Track(rect) => assert_eq!(rect.id, ghost.tab),
            other => panic!("a track tab keeps the tab: {other:?}"),
        }
        assert_eq!(ghost.zone_highlight, None, "the target is a group");
        assert_eq!(ghost.pinned_order, dest.pinned(), "the zone is as it is");
        let (highlighted, _) = view.highlight.expect("the group is highlighted");
        assert_eq!(highlighted, g);
        assert!(view.group_ghost.is_none());
        // The layout is the preview's: the gap holds one hole per arrival.
        let tabs_in_g = view.layout.groups[0].tabs.len();
        assert_eq!(tabs_in_g, 2 + 3);
    }

    #[test]
    fn the_incoming_view_of_a_pinned_box_keeps_the_box_and_opens_the_gap_in_the_zone() {
        let (mut origin, ot) = ws_of(2);
        origin.pin_tab(ot[0], 0);
        let content = tab_content(&origin, ot[0], &[]);
        let (mut dest, dt, _) = destination();
        dest.pin_tab(dt[2], 0);
        let mut m = TextMeasurer::new();
        let mut animations = AnimationClock::default();
        let view = incoming_view(
            &content,
            3.0,
            IncomingTarget::Tabs(DragDrop::Pinned { pos: 0 }),
            None,
            &dest,
            &mut animations,
            &env(),
            &mut m,
        );
        let ghost = view.ghost.expect("a ghost");
        assert_eq!(ghost.source, GhostSource::Pinned);
        assert_eq!(
            ghost.pinned_order,
            [ghost.tab, dt[2]],
            "the gap is at the front"
        );
        assert!(ghost.zone_highlight.is_some());
        assert_eq!(view.workspace.pinned(), [ghost.tab, dt[2]]);
        assert_eq!(
            ghost.base_pinned,
            Some(1),
            "not 2: the zone does not resize"
        );
    }

    #[test]
    fn the_incoming_view_of_a_group_has_the_preview_group_in_the_ghost() {
        let (origin, _, [a, ..]) = origin_with_groups();
        let mut m = TextMeasurer::new();
        let content =
            build_group_content(&origin, a, 7.0, 0.0, &layout_of(&origin, &mut m)).unwrap();
        let (dest, _, g) = destination();
        let mut animations = AnimationClock::default();
        let view = incoming_view(
            &content,
            150.0,
            IncomingTarget::Group { index: 1 },
            None,
            &dest,
            &mut animations,
            &env(),
            &mut m,
        );
        let ghost = view.group_ghost.expect("a group ghost");
        assert_eq!(ghost.screen_x, 143.0);
        assert!(view.workspace.group(ghost.group).is_some());
        assert!(dest.group(ghost.group).is_none());
        assert!(view.ghost.is_none() && view.highlight.is_none());
        assert_eq!(view.layout.groups[0].id, g);
        assert_eq!(view.layout.groups[1].id, ghost.group);
    }

    #[test]
    fn the_hole_of_the_origin_has_neither_the_tabs_nor_a_ghost() {
        let (origin, ot, [a, b, c]) = origin_with_groups();
        let mut m = TextMeasurer::new();
        let base = layout_of(&origin, &mut m);
        let payload = DragPayload::Tabs {
            grabbed: ot[3],
            all: vec![ot[0], ot[3]],
        };
        let hole = hole_view(&payload, &origin, &base, &env(), &mut m);
        assert!(hole.ghost.is_none());
        let ids: Vec<TabId> = hole
            .layout
            .groups
            .iter()
            .flat_map(|g| g.tabs.iter().map(|t| t.id))
            .collect();
        assert_eq!(ids, [ot[1], ot[2], ot[4], ot[5]]);
        // A group: gone with its tabs.
        let hole = hole_view(&DragPayload::Group(b), &origin, &base, &env(), &mut m);
        let groups: Vec<GroupId> = hole.layout.groups.iter().map(|g| g.id).collect();
        assert_eq!(groups, [a, c]);
        // The real workspace is untouched.
        assert_eq!(origin.track_order().count(), 6);
    }

    #[test]
    fn the_hole_of_a_pinned_box_keeps_its_slot() {
        let (mut origin, ot) = ws_of(3);
        origin.pin_tab(ot[0], 0);
        origin.pin_tab(ot[1], 1);
        let mut m = TextMeasurer::new();
        let base = layout_of(&origin, &mut m);
        let hole = hole_view(&DragPayload::Pinned(ot[0]), &origin, &base, &env(), &mut m);
        let ghost = hole.ghost.expect("the zone keeps a hole");
        assert_eq!(ghost.source, GhostSource::Hole);
        assert_eq!(ghost.pinned_order, [ot[0], ot[1]], "the slot stays");
        assert_eq!(ghost.tab, ot[0]);
        assert_eq!(ghost.zone_highlight, None);
        assert_eq!(ghost.base_pinned, None, "the workspace is the real one");
    }

    // --- detached_element_x: where the grabbed element is in the new bar ---

    fn element_x(content: &GhostContent, m: &mut TextMeasurer) -> f32 {
        detached_element_x(content, &STYLE, false, 800.0, m)
    }

    /// The `x` of the first tab of a window that holds only loose tabs: the
    /// start of the track plus the left margin of the first box.
    fn first_loose_tab_x(m: &mut TextMeasurer) -> f32 {
        let (ws, t) = ws_of(1);
        let layout = layout_of(&ws, m);
        tab_bar::tab_rect(&layout, t[0]).unwrap().x + tab_bar::track_shift(&STYLE, false, 0)
    }

    #[test]
    fn a_detached_lone_tab_is_the_first_of_the_track() {
        let (ws, t, _) = origin_with_groups();
        let mut m = TextMeasurer::new();
        let content = tab_content(&ws, t[3], &[]);
        assert_eq!(element_x(&content, &mut m), first_loose_tab_x(&mut m));
    }

    #[test]
    fn a_detached_selection_puts_the_grabbed_tab_where_it_falls_in_the_block() {
        let (ws, t, _) = origin_with_groups();
        let mut m = TextMeasurer::new();
        // The grabbed tab is the middle one of three, in the visual order.
        let content = tab_content(&ws, t[3], &[t[5], t[0]]);
        assert_eq!(content.grabbed_index, 1);
        let x = element_x(&content, &mut m);

        // The same three tabs, loose, in a window of their own: the second
        // one is the grabbed tab.
        let (loose, lt) = ws_of(3);
        let layout = layout_of(&loose, &mut m);
        let second = tab_bar::tab_rect(&layout, lt[1]).unwrap().x;
        let first = tab_bar::tab_rect(&layout, lt[0]).unwrap().x;
        assert!(second > first);
        assert_eq!(x, second + tab_bar::track_shift(&STYLE, false, 0));
        assert!(x > first_loose_tab_x(&mut m), "not the first of the track");
    }

    #[test]
    fn a_detached_pinned_box_is_the_first_of_the_fixed_row() {
        let (mut ws, t) = ws_of(3);
        ws.pin_tab(t[1], 0);
        let mut m = TextMeasurer::new();
        let content = tab_content(&ws, t[1], &[]);
        assert_eq!(
            element_x(&content, &mut m),
            tab_bar::pinned_rect(&STYLE, false, 0).x
        );
    }

    #[test]
    fn a_detached_group_is_the_first_pill_of_the_bar() {
        let (ws, _, [_, b, _]) = origin_with_groups();
        let mut m = TextMeasurer::new();
        let layout = layout_of(&ws, &mut m);
        let content = build_group_content(&ws, b, 5.0, 9.0, &layout).unwrap();
        let x = element_x(&content, &mut m);

        // The group alone in a window: its pill is the first element.
        let (mut alone, t) = ws_of(2);
        let g = alone.group_tabs(&t, "b", GroupColor::Green).unwrap();
        let layout = layout_of(&alone, &mut m);
        let pill = tab_bar::pill_rect(&layout, g).unwrap().x;
        assert_eq!(x, pill + tab_bar::track_shift(&STYLE, false, 0));
    }
}
