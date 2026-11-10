// SPDX-License-Identifier: GPL-3.0-or-later

//! `App::transfer`: the **one** path that takes tabs, a group or a pinned tab
//! out of a window and puts them in another existing window or in a new one,
//! without restarting any process (ADR-0090 §3, PRD-022 RF-22.2 to RF-22.6).
//!
//! Everything runs on the main thread with no frame in between: the steps of
//! the ADR are one straight line, and the order matters --
//!
//! 1. the runtimes move **and** their wakeup address is rewritten in the same
//!    loop ([`move_runtimes`]), so no later step can produce a frame with the
//!    old address in a cell;
//! 2. only then the state keyed by tab is re-keyed ([`rekey_tab_state`]);
//! 3. only then the two windows are marked dirty.
//!
//! What is pure (the decisions, the re-keying, the runtime move) is a free
//! function with a test; what needs a window stays in the `impl App` below.

use porecatu_core::{DetachedGroup, DetachedTab, InsertAt};

use super::*;
use crate::app_reconnect::PaneKey;
use crate::dialog::NoticeSlot;
use crate::notice_queue::PendingNotices;
use crate::pinned::PinnedRegistry;
use crate::prompt_queue::PendingPrompts;
use crate::reconnect::Cycle;

/// What leaves the origin window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TransferPayload {
    /// Tabs of the track (one, or a selection). `grabbed` is the one that
    /// becomes active at the destination (RF-22.10); `None` = the first that
    /// arrived. Pinned tabs in `ids` are ignored.
    Tabs {
        ids: Vec<TabId>,
        grabbed: Option<TabId>,
    },
    /// A whole explicit group.
    Group(GroupId),
    /// A tab of the pinned zone.
    Pinned(TabId),
}

/// Where a window created by the transfer opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewWindowPlacement {
    /// The cascade of `window.new` (the popover, RF-22.22).
    Cascade,
    /// Detach by drag (ADR-0091 §5): physical position of the client area and
    /// physical size (`None` = the platform default).
    At {
        position: winit::dpi::PhysicalPosition<i32>,
        size: Option<winit::dpi::PhysicalSize<u32>>,
    },
}

/// Where the payload lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferDest {
    Window(WindowId, InsertAt),
    NewWindow(NewWindowPlacement),
}

/// What [`App::transfer`] hands back: the window the payload is in now and
/// the new `TabId`s, in the order they arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransferResult {
    pub window: WindowId,
    pub tabs: Vec<TabId>,
}

/// One tab that changed address: the id it had in the origin and the one the
/// destination gave it. The two may be numerically equal (every window starts
/// at `TabId(0)`), which is why every key below also carries the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabMove {
    pub old: TabId,
    pub new: TabId,
}

// --- pure decisions -------------------------------------------------------

/// The origin's tabs the payload names, in the origin's visual order. Empty
/// means the transfer is refused: nothing exists to move, the tabs are only
/// pinned ones (`Tabs`), the group is implicit or unknown, or the tab is not
/// pinned (`Pinned`).
fn moved_ids(workspace: &Workspace, payload: &TransferPayload) -> Vec<TabId> {
    match payload {
        TransferPayload::Tabs { ids, .. } => workspace
            .track_order()
            .filter(|tab| ids.contains(tab))
            .collect(),
        TransferPayload::Group(group) => workspace
            .group(*group)
            .filter(|group| group.is_explicit())
            .map(|group| group.tabs().to_vec())
            .unwrap_or_default(),
        TransferPayload::Pinned(tab) => {
            if workspace.is_pinned(*tab) {
                vec![*tab]
            } else {
                Vec::new()
            }
        }
    }
}

/// Where a payload lands in a window that is born for it: the end of the
/// track for tabs, the first slot of the group list for a group, the first
/// slot of the zone for a pinned tab (RF-22.14).
fn new_window_insert_at(payload: &TransferPayload) -> InsertAt {
    match payload {
        TransferPayload::Tabs { .. } => InsertAt::TrackEnd,
        TransferPayload::Group(_) => InsertAt::NewRun { group_index: 0 },
        TransferPayload::Pinned(_) => InsertAt::Pinned { pos: 0 },
    }
}

/// A group only falls on a **border between groups** (RF-22.7): never into a
/// group, never into the pinned zone. Tabs and pinned tabs fit anywhere.
fn insert_at_fits(payload: &TransferPayload, at: InsertAt) -> bool {
    match payload {
        TransferPayload::Group(_) => {
            matches!(at, InsertAt::NewRun { .. } | InsertAt::TrackEnd)
        }
        TransferPayload::Tabs { .. } | TransferPayload::Pinned(_) => true,
    }
}

/// The position of a group in the destination's group list for a border drop:
/// `NewRun { group_index }` is that index and `TrackEnd` the end (core
/// saturates). `None` for the destinations a group never takes.
fn group_position(at: InsertAt) -> Option<usize> {
    match at {
        InsertAt::NewRun { group_index } => Some(group_index),
        InsertAt::TrackEnd => Some(usize::MAX),
        InsertAt::IntoGroup { .. } | InsertAt::Pinned { .. } => None,
    }
}

/// What the transfer does to the process-wide list of pinned tabs
/// (ADR-0090 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PinEffect {
    /// Nothing: loose tabs landing on the track.
    None,
    /// A pinned tab lands in a zone: the entry keeps its snapshot and goes to
    /// the position the ADR-0086 §2 rule gives it (RF-22.6, RF-22.26).
    Relocate,
    /// A pinned tab lands on the track: it is unpinned, as in RF-21.10.
    Unpin,
    /// Loose tabs land in a zone: each becomes a new entry, as the pin gesture
    /// does it.
    Pin,
}

fn pin_effect(payload: &TransferPayload, at: InsertAt) -> PinEffect {
    let into_zone = matches!(at, InsertAt::Pinned { .. });
    match payload {
        TransferPayload::Pinned(_) if into_zone => PinEffect::Relocate,
        TransferPayload::Pinned(_) => PinEffect::Unpin,
        TransferPayload::Tabs { .. } if into_zone => PinEffect::Pin,
        TransferPayload::Tabs { .. } | TransferPayload::Group(_) => PinEffect::None,
    }
}

/// The tab that is active at the destination (RF-22.10): the one the user
/// held, else the group's last active (an index into the group's tabs), else
/// the first that arrived. `moves` is in the order the tabs arrived.
fn activation_target(
    grabbed: Option<TabId>,
    group_last_active: Option<usize>,
    moves: &[TabMove],
) -> Option<TabId> {
    grabbed
        .and_then(|grabbed| moves.iter().find(|m| m.old == grabbed))
        .or_else(|| group_last_active.and_then(|index| moves.get(index)))
        .or_else(|| moves.first())
        .map(|m| m.new)
}

// --- the runtime move -----------------------------------------------------

/// Moves every runtime of the moved tabs from `origin` to `dest` and rewrites
/// the wakeup address of each **in the same step** (ADR-0090 §3 step 4): there
/// is no moment where a runtime sits in `dest` with the cell still pointing at
/// the origin. A tab without runtime (`NotStarted`) has nothing to move, and
/// the keys are read from the map, so no runtime is left behind under an old
/// key whatever the pane tree says.
fn move_runtimes(
    origin: &mut HashMap<(TabId, PaneId), PaneRuntime>,
    dest: &mut HashMap<(TabId, PaneId), PaneRuntime>,
    to: WindowId,
    moves: &[TabMove],
) {
    for m in moves {
        let keys: Vec<(TabId, PaneId)> = origin
            .keys()
            .filter(|(tab, _)| *tab == m.old)
            .copied()
            .collect();
        for key in keys {
            let pane = key.1;
            if let Some(runtime) = origin.remove(&key) {
                runtime.set_address(PaneAddress {
                    window: to,
                    tab: m.new,
                    pane,
                });
                dest.insert((m.new, pane), runtime);
            }
        }
    }
}

// --- the re-keying --------------------------------------------------------

/// The state keyed by tab that lives **in a window** (the two sides of a
/// transfer each lend theirs).
pub(crate) struct TabKeyed<'a> {
    pub prompts: &'a mut PendingPrompts,
    pub notices: &'a mut PendingNotices,
    pub dismissed: &'a mut Vec<NoticeSlot>,
}

/// The state keyed by `(window, tab)` that lives **in the process**.
pub(crate) struct ProcessTabKeyed<'a, C> {
    /// `App.reconnects`: a cycle per connection, its members are panes.
    pub reconnects: &'a mut HashMap<C, Cycle<PaneKey>>,
    pub pinned: &'a mut PinnedRegistry<WindowId>,
    /// `App.pending_shell_integration_note`.
    pub shell_note: &'a mut Option<(WindowId, TabId)>,
}

/// The tab a dismissed-notice slot is about.
fn slot_tab(slot: &NoticeSlot) -> TabId {
    match slot {
        NoticeSlot::Reconnect(tab, _)
        | NoticeSlot::Connecting(tab, _)
        | NoticeSlot::Dormant(tab, _) => *tab,
    }
}

/// ADR-0090 §3 step 5: re-keys every piece of state **outside** the
/// `PaneRuntime` that names a moved tab, so that nothing keyed by an old
/// `(window, tab)` is left behind:
///
/// - pending prompts and notices (and the `TabId` inside the notice's dialog)
///   go to the destination window;
/// - the dismissed-notice slots of the tab go too, rewritten;
/// - in every reconnection cycle the key of each pane of the tab is replaced
///   **in place**, so a cycle owner stays the owner;
/// - the live binding of a pinned entry is bound to `(to, new)` (the order of
///   the list is decided afterwards, by `App::transfer`);
/// - the pending shell-integration note follows the tab.
///
/// The `.porecatu` command pending in a pane lives in the `PaneRuntime` and
/// travels with it (RF-12.9); nothing here recreates it.
pub(crate) fn rekey_tab_state<C>(
    from: WindowId,
    to: WindowId,
    moves: &[TabMove],
    origin: TabKeyed<'_>,
    dest: TabKeyed<'_>,
    process: ProcessTabKeyed<'_, C>,
) {
    for &TabMove { old, new } in moves {
        dest.prompts.adopt(origin.prompts.take_tab(old), new);
        dest.notices.adopt(origin.notices.take_tab(old), new);

        let (carried, kept): (Vec<NoticeSlot>, Vec<NoticeSlot>) = std::mem::take(origin.dismissed)
            .into_iter()
            .partition(|slot| slot_tab(slot) == old);
        *origin.dismissed = kept;
        for mut slot in carried {
            slot.retarget_tab(old, new);
            dest.dismissed.push(slot);
        }

        for cycle in process.reconnects.values_mut() {
            let panes: Vec<PaneKey> = cycle
                .members()
                .iter()
                .copied()
                .filter(|&(window, tab, _)| window == from && tab == old)
                .collect();
            for (_, _, pane) in panes {
                cycle.replace_member((from, old, pane), (to, new, pane));
            }
        }

        if let Some(key) = process.pinned.key_of(from, old) {
            process.pinned.bind_live(key, to, new);
        }

        if *process.shell_note == Some((from, old)) {
            *process.shell_note = Some((to, new));
        }
    }
}

impl WindowState {
    /// ADR-0090 §3 step 6: drops the gesture state of the **origin** that
    /// points at the tabs that left (RF-22.4). A rename of a moved tab was
    /// committed before the extraction (so the title travels in the `Tab`),
    /// and a dialog cannot be open (a modal refuses the transfer).
    fn discard_gesture_state_of(&mut self, moved: &[TabId]) {
        if self
            .search
            .as_ref()
            .is_some_and(|search| moved.contains(&search.tab()))
        {
            self.search = None;
        }
        if let Some(TabBarHit::Tab(tab) | TabBarHit::CloseButton(tab)) = self.pressed_bar_button
            && moved.contains(&tab)
        {
            self.pressed_bar_button = None;
        }
        self.close_all_popovers();
        self.hover.dismiss();
        self.animations.clear();
        self.drag = Drag::Idle;
        self.last_pill_click = None;
        // PRD-024, ADR-0095 §4: the task manager keeps its last sample keyed
        // by `TabId`; what points at a tab that left is an orphan (ADR-0090).
        // The chart history is the window's and does not change (RF-24.25).
        if let Some(view) = self.task_manager.as_mut() {
            view.remove_tabs(moved);
        }
    }
}

/// What came out of the origin.
enum Extracted {
    /// Tabs (or the one pinned tab), with the id each had.
    Tabs(Vec<(TabId, DetachedTab)>),
    Group(DetachedGroup, Vec<TabId>),
}

impl App {
    /// ADR-0090 §3: moves `payload` from `from` to `to`, restarting nothing.
    /// Returns the destination window and the new `TabId`s, or `None` **with
    /// no effect** when the transfer is refused: `from` unknown; destination
    /// equal to the origin or not a terminal window (settings and the profile
    /// editor are not in `windows`); a payload that is empty, does not exist
    /// in the origin, is an implicit group, is only pinned tabs (`Tabs`), or
    /// lands inside a group or the pinned zone (a group); a modal dialog open
    /// in the origin; pinning refused by a read-only `pinned.json` (that one
    /// leaves the usual warning).
    ///
    /// Deviation from the order of the ADR (reported in stage 04): the window
    /// of a `NewWindow` is created **before** the extraction, not after. The
    /// result is the same, and a failed creation then needs no giving back --
    /// nothing was taken yet, so the origin keeps its ids and its focus.
    pub(crate) fn transfer(
        &mut self,
        event_loop: &ActiveEventLoop,
        from: WindowId,
        payload: TransferPayload,
        to: TransferDest,
    ) -> Option<TransferResult> {
        // Step 0: refuse, before anything is taken.
        let (existing, at) = match to {
            TransferDest::Window(id, at) => (Some(id), at),
            TransferDest::NewWindow(_) => (None, new_window_insert_at(&payload)),
        };
        let origin = self.windows.get(&from)?;
        if existing == Some(from)
            || existing.is_some_and(|id| !self.windows.contains_key(&id))
            || origin.dialog.is_some()
            || moved_ids(&origin.workspace, &payload).is_empty()
            || !insert_at_fits(&payload, at)
        {
            return None;
        }
        let effect = pin_effect(&payload, at);
        if effect == PinEffect::Pin && !self.pinned.is_writable() {
            // The same warning, and the same reason, as the pin gesture.
            let warn_in = existing.unwrap_or(from);
            if let Some(state) = self.windows.get_mut(&warn_in) {
                state.warnings.push(
                    Severity::Warning,
                    msg::notice::pinned_refused::title(&self.catalog),
                    msg::notice::pinned_refused::body(&self.catalog),
                    Instant::now(),
                );
                state.window.request_redraw();
            }
            return None;
        }

        // Step 2 (before the extraction, see the note above): the window.
        let created = match to {
            TransferDest::Window(..) => None,
            TransferDest::NewWindow(placement) => {
                Some(self.open_empty_window(event_loop, from, placement)?)
            }
        };
        let to_id = created.or(existing)?;

        let moved = moved_ids(&self.windows.get(&from)?.workspace, &payload);
        let grabbed = match &payload {
            TransferPayload::Tabs { grabbed, .. } => *grabbed,
            TransferPayload::Group(_) | TransferPayload::Pinned(_) => None,
        };

        // Both windows at once: the extraction needs the origin and the
        // insertion the destination, and the runtimes cross between them.
        let [Some(origin), Some(dest)] = self.windows.get_disjoint_mut([&from, &to_id]) else {
            // Cannot happen after step 0; nothing was taken.
            if let Some(id) = created {
                self.close_window_unconditionally(id, event_loop);
            }
            return None;
        };

        // A rename in course of a moved tab is committed first: the custom
        // title lives in the `Tab`, and it is the `Tab` that travels.
        if origin
            .rename
            .editing_tab()
            .is_some_and(|tab| moved.contains(&tab))
        {
            origin.commit_rename();
        }

        // Step 1: extract.
        let extracted = match &payload {
            TransferPayload::Tabs { .. } => Extracted::Tabs(origin.workspace.take_tabs(&moved)),
            TransferPayload::Pinned(tab) => Extracted::Tabs(
                origin
                    .workspace
                    .take_pinned(*tab)
                    .map(|detached| (*tab, detached))
                    .into_iter()
                    .collect(),
            ),
            TransferPayload::Group(group) => match origin.workspace.take_group(*group) {
                Some((detached, olds)) => Extracted::Group(detached, olds),
                None => Extracted::Tabs(Vec::new()),
            },
        };

        // Step 3: insert at the destination, and learn the new ids.
        let mut group_last_active = None;
        let (olds, news): (Vec<TabId>, Vec<TabId>) = match extracted {
            Extracted::Tabs(pairs) => {
                let (olds, tabs): (Vec<TabId>, Vec<DetachedTab>) = pairs.into_iter().unzip();
                let news = dest.workspace.insert_detached_tabs(tabs, at);
                (olds, news)
            }
            Extracted::Group(group, olds) => {
                group_last_active = group.last_active();
                let position = group_position(at).unwrap_or(usize::MAX);
                let (_, news) = dest.workspace.insert_detached_group(group, position);
                (olds, news)
            }
        };
        let moves: Vec<TabMove> = olds
            .iter()
            .zip(&news)
            .map(|(&old, &new)| TabMove { old, new })
            .collect();
        if moves.is_empty() {
            // Cannot happen after step 0.
            if let Some(id) = created {
                self.close_window_unconditionally(id, event_loop);
            }
            return None;
        }

        // Step 4: the runtimes, with their wakeup address, in one go. Nothing
        // below can make a frame with the old address.
        move_runtimes(&mut origin.panes, &mut dest.panes, to_id, &moves);

        // Step 5: the state keyed by tab outside the runtime.
        rekey_tab_state(
            from,
            to_id,
            &moves,
            TabKeyed {
                prompts: &mut origin.pending_prompts,
                notices: &mut origin.notices,
                dismissed: &mut origin.dismissed_notices,
            },
            TabKeyed {
                prompts: &mut dest.pending_prompts,
                notices: &mut dest.notices,
                dismissed: &mut dest.dismissed_notices,
            },
            ProcessTabKeyed {
                reconnects: &mut self.reconnects,
                pinned: &mut self.pinned,
                shell_note: &mut self.pending_shell_integration_note,
            },
        );

        // Step 6: the gesture state of the origin.
        origin.discard_gesture_state_of(&olds);
        // PRD-024, ADR-0095 §4: the arrivals have **new** ids here (counters
        // are per workspace and never reused), so the destination's sample
        // cannot hold them yet and this removes nothing -- it is the same
        // rule applied to both windows, and it costs nothing if a counter
        // ever did hand an old id back. Their processes show up in the next
        // sample, under the new keys.
        if let Some(view) = dest.task_manager.as_mut() {
            view.remove_tabs(&news);
        }

        // Step 7: activate and focus at the destination; the selection goes
        // in both windows (RF-22.10).
        if let Some(target) = activation_target(grabbed, group_last_active, &moves) {
            let _ = dest.touch_workspace(|ws| ws.activate_tab(target));
        }
        origin.selection.clear();
        dest.selection.clear();
        if dest.window.is_minimized() == Some(true) {
            dest.window.set_minimized(false);
        }
        dest.window.focus_window();

        // Step 8: each tab that arrived to the frame of the destination. A
        // `NotStarted` tab has no runtime: it only changes place.
        for m in &moves {
            if dest
                .workspace
                .tab(m.new)
                .is_some_and(porecatu_core::Tab::is_not_started)
            {
                continue;
            }
            dest.resize_tab_panes(m.new, self.cell_metrics, &self.style);
        }

        // Step 9: both sides dirty (RF-22.25), after the cells are rewritten.
        origin.mark_session_dirty();
        dest.mark_session_dirty();
        dest.profile_badges_dirty = true;
        origin.sync_window_title();
        dest.sync_window_title();
        origin.window.request_redraw();
        dest.window.request_redraw();
        let origin_is_empty = origin.workspace.is_empty();

        // The list of pinned tabs (ADR-0090 §5). The live bindings are
        // already on the new keys (`rekey_tab_state`); here goes the order of
        // the list, and the file when -- only when -- it changes.
        self.apply_pin_effect(effect, to_id, &moves);

        self.reveal_active_tab(to_id);
        self.refresh_access_tree(to_id);

        // Step 10: an origin with nothing left closes, with no dialog
        // (RF-22.19). A pinned tab counts as a tab: it does not close.
        if origin_is_empty {
            self.close_window_unconditionally(from, event_loop);
        } else {
            self.refresh_access_tree(from);
        }

        Some(TransferResult {
            window: to_id,
            tabs: news,
        })
    }

    /// A window with nothing in it, for a transfer (RF-22.14): no shell tab,
    /// no pinned tabs. Goes through the same creation path as every window and
    /// enters the focus order like them. `None` if the surface could not be
    /// created (nothing was taken from anywhere yet).
    fn open_empty_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        origin: WindowId,
        placement: NewWindowPlacement,
    ) -> Option<WindowId> {
        let attributes = match placement {
            NewWindowPlacement::Cascade => self.cascade_attributes(Some(origin)),
            NewWindowPlacement::At { position, size } => {
                let attributes =
                    base_window_attributes(wants_transparent(&self.config)).with_position(position);
                match size {
                    Some(size) => attributes.with_inner_size(size),
                    None => attributes,
                }
            }
        };
        let state = self.create_window_with_attributes(event_loop, attributes)?;
        let id = state.window.id();
        self.windows.insert(id, state);
        self.focus_order.touch(id);
        Some(id)
    }

    /// The left neighbour of `tab` in the pinned zone of `window`, as a key
    /// of the registry (the rule of `pin_tab_gesture`).
    fn left_pinned_neighbor_key(&self, window: WindowId, tab: TabId) -> Option<pinned::PinnedKey> {
        let zone = self.windows.get(&window)?.workspace.pinned();
        let position = zone.iter().position(|&t| t == tab)?;
        let neighbor = pinned::left_neighbor_in_zone(zone, position)?;
        self.pinned.key_of(window, neighbor)
    }

    /// ADR-0090 §5 / RF-22.6 / RF-22.26: what the transfer does to the list
    /// of pinned tabs, once the tabs are in `window`.
    fn apply_pin_effect(&mut self, effect: PinEffect, window: WindowId, moves: &[TabMove]) {
        match effect {
            PinEffect::None => {}
            PinEffect::Relocate => {
                let mut order_changed = false;
                for m in moves {
                    let Some(key) = self.pinned.key_of(window, m.new) else {
                        continue;
                    };
                    let left = self.left_pinned_neighbor_key(window, m.new);
                    order_changed |= self.pinned.relocate(key, window, m.new, left);
                }
                if order_changed {
                    self.save_pinned_list(window);
                }
            }
            PinEffect::Unpin => {
                let mut changed = false;
                for m in moves {
                    if let Some(key) = self.pinned.key_of(window, m.new) {
                        self.pinned.remove(key);
                        self.forget_pinned_icon(key);
                        changed = true;
                    }
                }
                if changed {
                    self.save_pinned_list(window);
                }
            }
            PinEffect::Pin => {
                let mut icon_requests = Vec::new();
                let mut changed = false;
                // In the order they arrived: the left neighbour of each is a
                // tab that was already pinned, or the one pinned just before.
                for m in moves {
                    let Some(saved) = self
                        .windows
                        .get(&window)
                        .and_then(|state| state.pinned_snapshot(m.new))
                    else {
                        continue;
                    };
                    let left = self.left_pinned_neighbor_key(window, m.new);
                    let icon_dir = self.pinned_icon_dir(&saved);
                    let key = self.pinned.insert_for_pin(window, m.new, saved, left);
                    if let Some(dir) = icon_dir {
                        icon_requests.push((key, dir));
                    }
                    changed = true;
                }
                if !icon_requests.is_empty() {
                    self.dispatch_pinned_icon_requests(icon_requests);
                }
                if changed {
                    self.save_pinned_list(window);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use porecatu_core::GroupColor;
    use porecatu_session::{PinnedFileV1, PinnedLoadOutcome, TabV1};
    use porecatu_term::{PromptId, PromptKind};

    use super::*;
    use crate::messages::test_support;
    use crate::prompt_queue::Pending as PendingPrompt;

    fn tab(n: u32) -> TabId {
        TabId::new(n)
    }

    fn pane(n: u32) -> PaneId {
        PaneId::new(n)
    }

    fn win(n: u64) -> WindowId {
        WindowId::from(n)
    }

    fn mv(old: u32, new: u32) -> TabMove {
        TabMove {
            old: tab(old),
            new: tab(new),
        }
    }

    fn tabs_payload(ids: &[TabId]) -> TransferPayload {
        TransferPayload::Tabs {
            ids: ids.to_vec(),
            grabbed: None,
        }
    }

    // --- the decisions ---------------------------------------------------

    /// `a b | [G: c d] | e`, with `p` pinned.
    fn workspace_with_a_group() -> (Workspace, [TabId; 5], TabId, GroupId) {
        let mut ws = Workspace::new();
        let p = ws.append_tab("zsh", None);
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let d = ws.append_tab("zsh", None);
        let e = ws.append_tab("zsh", None);
        let group = ws
            .group_tabs(&[c, d], "infra", GroupColor::Blue)
            .expect("explicit group");
        assert!(ws.pin_tab(p, 0));
        (ws, [a, b, c, d, e], p, group)
    }

    #[test]
    fn tabs_resolve_in_the_visual_order_whatever_the_order_asked() {
        let (ws, [a, _, c, _, e], ..) = workspace_with_a_group();

        let ids = moved_ids(&ws, &tabs_payload(&[e, a, c]));

        assert_eq!(ids, [a, c, e]);
    }

    #[test]
    fn only_pinned_tabs_or_unknown_tabs_resolve_to_nothing() {
        let (ws, _, p, _) = workspace_with_a_group();

        assert!(moved_ids(&ws, &tabs_payload(&[p])).is_empty());
        assert!(moved_ids(&ws, &tabs_payload(&[tab(99)])).is_empty());
        assert!(moved_ids(&ws, &tabs_payload(&[])).is_empty());
    }

    #[test]
    fn mixed_tabs_drop_the_pinned_one_and_keep_the_rest() {
        let (ws, [a, ..], p, _) = workspace_with_a_group();

        assert_eq!(moved_ids(&ws, &tabs_payload(&[p, a])), [a]);
    }

    #[test]
    fn a_group_resolves_to_its_tabs_in_order_and_an_implicit_one_to_nothing() {
        let (ws, [_, _, c, d, _], _, group) = workspace_with_a_group();
        assert_eq!(moved_ids(&ws, &TransferPayload::Group(group)), [c, d]);

        let implicit = ws
            .groups()
            .iter()
            .find(|g| g.is_implicit())
            .expect("an implicit run")
            .id();
        assert!(moved_ids(&ws, &TransferPayload::Group(implicit)).is_empty());
        assert!(moved_ids(&ws, &TransferPayload::Group(GroupId::new(99))).is_empty());
    }

    #[test]
    fn a_pinned_payload_needs_a_tab_that_is_pinned() {
        let (ws, [a, ..], p, _) = workspace_with_a_group();

        assert_eq!(moved_ids(&ws, &TransferPayload::Pinned(p)), [p]);
        assert!(moved_ids(&ws, &TransferPayload::Pinned(a)).is_empty());
    }

    #[test]
    fn a_group_only_falls_on_a_border_between_groups() {
        let group = TransferPayload::Group(GroupId::new(0));
        assert!(insert_at_fits(&group, InsertAt::NewRun { group_index: 2 }));
        assert!(insert_at_fits(&group, InsertAt::TrackEnd));
        assert!(!insert_at_fits(
            &group,
            InsertAt::IntoGroup {
                group: GroupId::new(1),
                pos: 0
            }
        ));
        assert!(!insert_at_fits(&group, InsertAt::Pinned { pos: 0 }));

        for payload in [tabs_payload(&[tab(0)]), TransferPayload::Pinned(tab(0))] {
            for at in [
                InsertAt::IntoGroup {
                    group: GroupId::new(1),
                    pos: 0,
                },
                InsertAt::NewRun { group_index: 0 },
                InsertAt::Pinned { pos: 0 },
                InsertAt::TrackEnd,
            ] {
                assert!(insert_at_fits(&payload, at));
            }
        }
    }

    #[test]
    fn a_group_border_becomes_a_position_in_the_group_list() {
        assert_eq!(group_position(InsertAt::NewRun { group_index: 3 }), Some(3));
        assert_eq!(group_position(InsertAt::TrackEnd), Some(usize::MAX));
        assert_eq!(group_position(InsertAt::Pinned { pos: 0 }), None);
        assert_eq!(
            group_position(InsertAt::IntoGroup {
                group: GroupId::new(0),
                pos: 0
            }),
            None
        );
    }

    #[test]
    fn a_new_window_takes_each_payload_where_the_prd_says() {
        assert_eq!(
            new_window_insert_at(&tabs_payload(&[tab(0)])),
            InsertAt::TrackEnd
        );
        assert_eq!(
            new_window_insert_at(&TransferPayload::Group(GroupId::new(0))),
            InsertAt::NewRun { group_index: 0 }
        );
        assert_eq!(
            new_window_insert_at(&TransferPayload::Pinned(tab(0))),
            InsertAt::Pinned { pos: 0 }
        );
        // And every default fits its own payload.
        for payload in [
            tabs_payload(&[tab(0)]),
            TransferPayload::Group(GroupId::new(0)),
            TransferPayload::Pinned(tab(0)),
        ] {
            assert!(insert_at_fits(&payload, new_window_insert_at(&payload)));
        }
    }

    #[test]
    fn the_pin_effect_follows_where_a_payload_lands() {
        let zone = InsertAt::Pinned { pos: 0 };
        let track = InsertAt::TrackEnd;
        let pinned = TransferPayload::Pinned(tab(0));
        let loose = tabs_payload(&[tab(0)]);
        let group = TransferPayload::Group(GroupId::new(0));

        assert_eq!(pin_effect(&pinned, zone), PinEffect::Relocate);
        assert_eq!(pin_effect(&pinned, track), PinEffect::Unpin);
        assert_eq!(
            pin_effect(&pinned, InsertAt::NewRun { group_index: 0 }),
            PinEffect::Unpin
        );
        assert_eq!(pin_effect(&loose, zone), PinEffect::Pin);
        assert_eq!(pin_effect(&loose, track), PinEffect::None);
        assert_eq!(pin_effect(&group, track), PinEffect::None);
    }

    #[test]
    fn the_grabbed_tab_is_the_active_one_translated_to_its_new_id() {
        let moves = [mv(4, 0), mv(7, 1), mv(9, 2)];

        assert_eq!(activation_target(Some(tab(7)), None, &moves), Some(tab(1)));
        // The grabbed tab wins over the group's last active.
        assert_eq!(
            activation_target(Some(tab(9)), Some(0), &moves),
            Some(tab(2))
        );
    }

    #[test]
    fn without_a_grabbed_tab_the_groups_last_active_then_the_first_arrived() {
        let moves = [mv(4, 5), mv(7, 6), mv(9, 7)];

        assert_eq!(activation_target(None, Some(2), &moves), Some(tab(7)));
        assert_eq!(activation_target(None, None, &moves), Some(tab(5)));
        // A grabbed tab that did not move, or an index out of range, falls
        // down the ladder.
        assert_eq!(activation_target(Some(tab(1)), None, &moves), Some(tab(5)));
        assert_eq!(activation_target(None, Some(9), &moves), Some(tab(5)));
        assert_eq!(activation_target(None, None, &[]), None);
    }

    // --- the runtime move ------------------------------------------------

    #[test]
    fn the_runtimes_move_and_their_wakeup_address_is_rewritten_with_them() {
        use crate::pane_runtime_map_key_tests::pane_runtime;

        let (from, to) = (win(1), win(2));
        let mut origin: HashMap<(TabId, PaneId), PaneRuntime> = HashMap::new();
        origin.insert((tab(3), pane(0)), pane_runtime(None));
        origin.insert((tab(3), pane(1)), pane_runtime(None));
        origin.insert((tab(4), pane(0)), pane_runtime(None));
        for ((tab, pane), runtime) in &origin {
            runtime.set_address(PaneAddress {
                window: from,
                tab: *tab,
                pane: *pane,
            });
        }
        let mut dest: HashMap<(TabId, PaneId), PaneRuntime> = HashMap::new();
        dest.insert((tab(0), pane(0)), pane_runtime(None));

        // Tab 3 goes and lands as tab 0 -- the same number the destination's
        // own first tab has, and its panes must not collide with it.
        move_runtimes(&mut origin, &mut dest, to, &[mv(3, 1)]);

        assert!(origin.keys().all(|(tab, _)| *tab != self::tab(3)));
        assert!(origin.contains_key(&(tab(4), pane(0))), "the other stays");
        let mut keys: Vec<_> = dest.keys().copied().collect();
        keys.sort();
        assert_eq!(
            keys,
            [(tab(0), pane(0)), (tab(1), pane(0)), (tab(1), pane(1))]
        );
        for pane_id in [pane(0), pane(1)] {
            let address = *dest[&(tab(1), pane_id)].address.lock().unwrap();
            assert_eq!(
                address,
                PaneAddress {
                    window: to,
                    tab: tab(1),
                    pane: pane_id,
                }
            );
        }
        // The untouched runtime keeps its own address.
        let stay = *origin[&(tab(4), pane(0))].address.lock().unwrap();
        assert_eq!(stay.window, from);
    }

    // --- the re-keying ---------------------------------------------------

    fn prompt(n: u64) -> porecatu_term::Prompt {
        porecatu_term::Prompt {
            id: PromptId::new(n),
            kind: PromptKind::HostKey {
                algorithm: "ssh-ed25519".into(),
                fingerprint: "SHA256:abc".into(),
                known_types: Vec::new(),
                skipped_lines: 0,
            },
        }
    }

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

    fn registry(tags: &[&str]) -> PinnedRegistry<WindowId> {
        let file = PinnedFileV1 {
            tabs: tags.iter().map(|t| saved(t)).collect(),
            ..PinnedFileV1::default()
        };
        PinnedRegistry::from_load(PinnedLoadOutcome::Loaded(file), None).0
    }

    /// The state of two windows, a registry, a cycle map and the shell note,
    /// with the tab `3` of the origin carrying everything, and the tab `4`
    /// carrying the same kinds of state as a control that must not move.
    struct World {
        origin_prompts: PendingPrompts,
        origin_notices: PendingNotices,
        origin_dismissed: Vec<NoticeSlot>,
        dest_prompts: PendingPrompts,
        dest_notices: PendingNotices,
        dest_dismissed: Vec<NoticeSlot>,
        cycles: HashMap<u32, Cycle<PaneKey>>,
        registry: PinnedRegistry<WindowId>,
        shell_note: Option<(WindowId, TabId)>,
    }

    const FROM: u64 = 1;
    const TO: u64 = 2;

    fn world() -> World {
        let catalog = test_support::pt_br();
        let mut origin_prompts = PendingPrompts::default();
        origin_prompts.push(tab(3), pane(0), prompt(0));
        origin_prompts.push(tab(3), pane(1), prompt(1));
        origin_prompts.push(tab(4), pane(0), prompt(2));

        let mut origin_notices = PendingNotices::default();
        origin_notices.push(
            tab(3),
            pane(1),
            ConfirmDialog::reconnect(&catalog, tab(3), pane(1), "waiting"),
        );
        origin_notices.push(
            tab(4),
            pane(0),
            ConfirmDialog::reconnect(&catalog, tab(4), pane(0), "control"),
        );

        // The owner of the cycle is the moved pane, and a follower is the
        // pane of another tab of the same window.
        let t0 = Instant::now();
        let mut cycle = Cycle::new(t0, (win(FROM), tab(3), pane(1)));
        cycle.add((win(FROM), tab(4), pane(0)));
        let mut cycles = HashMap::new();
        cycles.insert(7, cycle);

        let mut registry = registry(&["moved", "stays"]);
        let moved_key = registry.entries()[0].key;
        let stays_key = registry.entries()[1].key;
        registry.bind_live(moved_key, win(FROM), tab(3));
        registry.bind_live(stays_key, win(FROM), tab(4));

        World {
            origin_prompts,
            origin_notices,
            origin_dismissed: vec![
                NoticeSlot::Dormant(tab(3), pane(0)),
                NoticeSlot::Reconnect(tab(4), pane(0)),
            ],
            dest_prompts: PendingPrompts::default(),
            dest_notices: PendingNotices::default(),
            dest_dismissed: Vec::new(),
            cycles,
            registry,
            shell_note: Some((win(FROM), tab(3))),
        }
    }

    fn run(world: &mut World, moves: &[TabMove]) {
        rekey_tab_state(
            win(FROM),
            win(TO),
            moves,
            TabKeyed {
                prompts: &mut world.origin_prompts,
                notices: &mut world.origin_notices,
                dismissed: &mut world.origin_dismissed,
            },
            TabKeyed {
                prompts: &mut world.dest_prompts,
                notices: &mut world.dest_notices,
                dismissed: &mut world.dest_dismissed,
            },
            ProcessTabKeyed {
                reconnects: &mut world.cycles,
                pinned: &mut world.registry,
                shell_note: &mut world.shell_note,
            },
        );
    }

    /// The `TabId` of the origin is `3`, and the destination gives it `0`
    /// (its own first tab's number is taken by another tab there already).
    #[test]
    fn nothing_keyed_by_the_old_tab_is_left_in_the_origin_or_the_process() {
        let mut w = world();

        run(&mut w, &[mv(3, 0)]);

        // The origin's queues and slots hold no trace of tab 3.
        assert!(w.origin_prompts.take_tab(tab(3)).is_empty());
        assert!(w.origin_notices.take_tab(tab(3)).is_empty());
        assert!(w.origin_dismissed.iter().all(|s| slot_tab(s) != tab(3)));
        // No cycle member, no live binding and no note names (FROM, 3).
        for cycle in w.cycles.values() {
            assert!(
                cycle
                    .members()
                    .iter()
                    .all(|&(window, tab_id, _)| !(window == win(FROM) && tab_id == tab(3)))
            );
        }
        assert_eq!(w.registry.key_of(win(FROM), tab(3)), None);
        assert_ne!(w.shell_note, Some((win(FROM), tab(3))));
    }

    #[test]
    fn what_was_keyed_by_the_old_tab_arrives_keyed_by_the_new_one() {
        let mut w = world();

        run(&mut w, &[mv(3, 0)]);

        // Prompts: both panes, in arrival order, under the new tab.
        let items: Vec<PendingPrompt> = w.dest_prompts.take_tab(tab(0));
        let got: Vec<_> = items.iter().map(|p| (p.pane, p.prompt.id)).collect();
        assert_eq!(
            got,
            [(pane(0), PromptId::new(0)), (pane(1), PromptId::new(1))]
        );
        // Notices: the tab inside the pending item, the dialog's action and
        // its slot all say the new tab.
        let notices = w.dest_notices.take_tab(tab(0));
        assert_eq!(notices.len(), 1);
        assert_eq!((notices[0].tab, notices[0].pane), (tab(0), pane(1)));
        assert_eq!(
            notices[0].dialog.action,
            DialogAction::ReconnectNow(tab(0), pane(1))
        );
        assert_eq!(
            notices[0].dialog.slot,
            Some(NoticeSlot::Reconnect(tab(0), pane(1)))
        );
        // The dismissed slot moved, rewritten; the control stayed.
        assert_eq!(w.dest_dismissed, [NoticeSlot::Dormant(tab(0), pane(0))]);
        assert_eq!(w.origin_dismissed, [NoticeSlot::Reconnect(tab(4), pane(0))]);
        // The pinned entry is live in the destination under the new tab.
        let key = w.registry.entries()[0].key;
        assert_eq!(w.registry.live_of(key), Some((win(TO), tab(0))));
        // The shell note follows the tab.
        assert_eq!(w.shell_note, Some((win(TO), tab(0))));
    }

    #[test]
    fn the_control_tab_keeps_every_key_it_had() {
        let mut w = world();

        run(&mut w, &[mv(3, 0)]);

        let control_prompts = w.origin_prompts.take_tab(tab(4));
        assert_eq!(control_prompts.len(), 1);
        assert_eq!(w.origin_notices.take_tab(tab(4)).len(), 1);
        let stays = w.registry.entries()[1].key;
        assert_eq!(w.registry.live_of(stays), Some((win(FROM), tab(4))));
        assert!(
            w.cycles[&7]
                .members()
                .contains(&(win(FROM), tab(4), pane(0)))
        );
    }

    /// The reconnection cycle continues where it was: the moved pane is still
    /// the one that tries, even though its new key sorts **after** the
    /// follower's (the ADR-0076 order is by key).
    #[test]
    fn the_owner_of_a_cycle_stays_the_owner_after_its_pane_moves() {
        let mut w = world();

        run(&mut w, &[mv(3, 9)]);

        let cycle = &w.cycles[&7];
        assert_eq!(cycle.owner(), Some((win(TO), tab(9), pane(1))));
        assert_eq!(
            cycle.followers().collect::<Vec<_>>(),
            [(win(FROM), tab(4), pane(0))]
        );
    }

    #[test]
    fn several_tabs_move_together_each_to_its_own_new_id() {
        let mut w = world();

        // Tab 4 goes too; the destination gives them 0 and 1.
        run(&mut w, &[mv(3, 0), mv(4, 1)]);

        assert!(w.origin_prompts.is_empty());
        assert!(w.origin_notices.is_empty());
        assert!(w.origin_dismissed.is_empty());
        assert_eq!(w.dest_prompts.take_tab(tab(0)).len(), 2);
        assert_eq!(w.dest_prompts.take_tab(tab(1)).len(), 1);
        let members = w.cycles[&7].members().to_vec();
        assert_eq!(
            members,
            [(win(TO), tab(0), pane(1)), (win(TO), tab(1), pane(0))]
        );
        assert_eq!(
            w.registry.live_of(w.registry.entries()[1].key),
            Some((win(TO), tab(1)))
        );
    }

    #[test]
    fn a_cycle_of_another_window_is_not_touched() {
        let mut w = world();
        // A pane of the destination window with the same numbers as the moved
        // one, in another cycle: its key is `(TO, 3, 1)`.
        let mut other = Cycle::new(Instant::now(), (win(TO), tab(3), pane(1)));
        other.add((win(9), tab(3), pane(1)));
        w.cycles.insert(8, other);

        run(&mut w, &[mv(3, 0)]);

        assert_eq!(
            w.cycles[&8].members(),
            [(win(TO), tab(3), pane(1)), (win(9), tab(3), pane(1))]
        );
    }

    #[test]
    fn a_tab_without_any_state_changes_nothing() {
        let mut w = world();

        run(&mut w, &[mv(42, 0)]);

        assert_eq!(w.origin_prompts.take_tab(tab(3)).len(), 2);
        assert!(w.dest_prompts.is_empty());
        assert_eq!(w.shell_note, Some((win(FROM), tab(3))));
    }

    #[test]
    fn a_dialog_that_carries_the_tab_is_a_slot_the_helper_reads() {
        // `slot_tab` and `NoticeSlot::retarget_tab` must agree on every
        // variant: whatever one calls "the tab", the other rewrites.
        for slot in [
            NoticeSlot::Reconnect(tab(3), pane(0)),
            NoticeSlot::Connecting(tab(3), pane(0)),
            NoticeSlot::Dormant(tab(3), pane(0)),
        ] {
            let mut rewritten = slot;
            rewritten.retarget_tab(tab(3), tab(8));
            assert_eq!(slot_tab(&slot), tab(3));
            assert_eq!(slot_tab(&rewritten), tab(8));
        }
    }
}
