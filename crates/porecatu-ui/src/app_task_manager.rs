// SPDX-License-Identifier: GPL-3.0-or-later

//! The task manager wired into the `App` (PRD-024, ADR-0093 §4 to §6,
//! ADR-0095 §1, §2, §4, §9): opening, closing and toggling the window mode,
//! the sampler thread and its clock, the answer applied per window, the
//! painting that takes the panes' place, and the notice of an adjusted period.
//!
//! The model (rows, selection, history, layout) is [`crate::task_manager`] and
//! knows no window. What decides something without needing a window is a free
//! function here, with a test: the ticket of the request/answer pair, the
//! roots of a request, the split of an answer by window, the rule of a pane's
//! fact, the view's selection and scroll after a new list, the notice rule.
//! What needs a window stays in the `impl App` / `impl WindowState` below.
//!
//! Step 07a, also here: the key chain link ([`task_manager_key_route`] and
//! `App::run_task_manager_route`), the mouse in the content area (press,
//! hover and wheel, with the pure hit-test [`row_index_at`]), "Go to tab"
//! ([`go_to_tab`]) and the return to the terminal by activation or creation
//! of a tab ([`should_leave`], the one check of ADR-0095 §2).
//!
//! Step 07b, also here: the row context menu (`App::open_task_manager_menu`,
//! its keys and its actions), the "End process" confirmation, the kill
//! request ([`kill_route`]) and the tooltip of a cut name ([`row_tooltip`]).
//!
//! Step 08, also here: the actions a screen reader asks for on the manager's
//! nodes (`App::run_task_manager_access_action`). The tree itself is
//! [`crate::access`]; the settings screen group is `settings/catalog.rs`.
//!
//! ADR-0098, also here: `background_monitoring` in the sampler's life
//! ([`sampler_needed`], [`window_measured`], [`apply_to_history`]) and the
//! footer's toggle, which writes the key through `porecatu-config::edit`
//! ([`save_background_monitoring`]) and lets the hot reload apply it.

use std::time::{Duration, Instant};

use std::path::Path;

use porecatu_config::{Config, ConfigDocument, Edit, EditError, EditValue, KeyPath};
use porecatu_locale::Catalog;
use porecatu_render::{Primitive, Rect, TextMeasurer};
use porecatu_term::{
    KillRequest, ProcessStat, SampleRequest, SampleResult, Sampler, SamplerRequest,
};

use crate::access::{TaskAccessAction, task_manager_access_route};
use crate::app_reconnect::PaneKey;
use crate::palette::{ResolvedPalette, ResolvedTermPalette};
use crate::tab_bar::TabBarStyle;
use crate::task_manager::menu::{
    TaskMenuAction, TaskMenuItem, TaskMenuKey, TaskMenuOutcome, run_menu_action, task_menu_items,
    task_menu_key,
};
use crate::task_manager::paint::{
    self as tm_paint, ListLayout, TaskManagerLayout, TaskManagerStyle,
};
use crate::task_manager::rows::{
    self, ListNav, PaneFact, PaneFacts, Row, clamp_scroll, keep_visible, resolve_selection,
};
use crate::task_manager::{
    HistoryPoint, RowKey, TaskManagerHistory, TaskManagerView, TaskMenu, WindowSample,
    effective_history,
};

use super::*;

/// The sampling clock (ADR-0093 §4, RF-24.24): once a second.
pub(crate) const SAMPLE_PERIOD: Duration = Duration::from_secs(1);

/// An answer that has not come back after this long is taken as lost (the
/// thread died with a request in hand): the sampler is dropped and recreated
/// at the next deadline. Without it a panicked thread would leave "the
/// previous answer has not arrived" true forever and the manager would stop
/// being fed. Far above the time of any real scan.
const ANSWER_LOST_AFTER: Duration = Duration::from_secs(10);

/// The window's key of a pane inside a sample: the sampler's key without the
/// window, since one sample belongs to one window.
type WindowPaneKey = (TabId, PaneId);

// ---------------------------------------------------------------------------
// Pure rules
// ---------------------------------------------------------------------------

/// The `seq` of the last request and whether its answer is still pending
/// (ADR-0093 §4: "if the previous answer has not arrived, this tick sends
/// nothing"; §6: "drops the answer whose `seq` is not the last one sent").
#[derive(Debug, Default)]
pub(crate) struct SampleTicket {
    last: u64,
    pending_since: Option<Instant>,
}

impl SampleTicket {
    /// Whether the answer to the last request is still on its way.
    pub(crate) fn is_pending(&self) -> bool {
        self.pending_since.is_some()
    }

    /// The `seq` of the next request.
    pub(crate) fn next_seq(&self) -> u64 {
        self.last.wrapping_add(1)
    }

    /// The request `seq` was delivered to the sampler.
    pub(crate) fn sent(&mut self, seq: u64, now: Instant) {
        self.last = seq;
        self.pending_since = Some(now);
    }

    /// `true` when `seq` is the answer being waited for, and then nothing is
    /// pending anymore. A stale `seq` -- or any answer when none is pending
    /// (the sampler was dropped in the meantime) -- is refused.
    pub(crate) fn accept(&mut self, seq: u64) -> bool {
        if self.pending_since.is_some() && seq == self.last {
            self.pending_since = None;
            true
        } else {
            false
        }
    }

    /// Whether the pending answer is older than [`ANSWER_LOST_AFTER`].
    pub(crate) fn is_lost(&self, now: Instant) -> bool {
        self.pending_since
            .is_some_and(|since| now.saturating_duration_since(since) >= ANSWER_LOST_AFTER)
    }

    /// The sampler went away (dropped, or found dead): nothing is pending
    /// and any answer still in flight, whatever its `seq`, is refused.
    pub(crate) fn forget_pending(&mut self) {
        self.pending_since = None;
    }
}

/// Whether the sampler must exist (ADR-0098 §2): `[task_manager]
/// background_monitoring` is on, or some window has the manager open.
pub(crate) fn sampler_needed(background: bool, open: impl IntoIterator<Item = bool>) -> bool {
    background || open.into_iter().any(|is_open| is_open)
}

/// Whether a window is measured (ADR-0098 §3): with the manager open, or in
/// any case with `background_monitoring` on. Its panes go in the request and
/// its history gets the answer's point.
pub(crate) fn window_measured(open: bool, background: bool) -> bool {
    open || background
}

/// The roots of a sample request (ADR-0093 §5, ADR-0098 §3): for **every
/// measured window** ([`window_measured`]), one item per pane that has a
/// runtime **and** a process group. `windows` yields `(window, measured,
/// panes)`, the panes being the window's runtimes (a `NotStarted` tab has
/// none, ADR-0037) with the group `Terminal::process_group` gave (`None` for
/// SSH).
pub(crate) fn sample_roots<G>(
    windows: impl IntoIterator<Item = (WindowId, bool, Vec<(WindowPaneKey, Option<G>)>)>,
) -> Vec<(PaneKey, G)> {
    windows
        .into_iter()
        .filter(|(_, measured, _)| *measured)
        .flat_map(|(window, _, panes)| {
            panes.into_iter().filter_map(move |((tab, pane), group)| {
                group.map(|group| ((window, tab, pane), group))
            })
        })
        .collect()
}

/// Splits an answer by window (ADR-0093 §6), keeping only the keys `keep`
/// accepts: the caller says a key is good when its window still exists and
/// is measured ([`window_measured`]) and its pane still exists in that
/// window. A key of a
/// closed window or of a tab that left or closed is dropped -- no old `TabId`
/// is looked up or fixed, the next request already carries the new keys.
pub(crate) fn partition_sample(
    panes: Vec<(PaneKey, Vec<ProcessStat>)>,
    mut keep: impl FnMut(&PaneKey) -> bool,
) -> HashMap<WindowId, HashMap<WindowPaneKey, Vec<ProcessStat>>> {
    let mut by_window: HashMap<WindowId, HashMap<WindowPaneKey, Vec<ProcessStat>>> = HashMap::new();
    for (key, stats) in panes {
        if !keep(&key) {
            continue;
        }
        let (window, tab, pane) = key;
        by_window
            .entry(window)
            .or_default()
            .entry((tab, pane))
            .or_insert(stats);
    }
    by_window
}

/// Gives one window its part of the answer: the sample the list reads, and
/// one chart point with the totals at the instant the sampler read the
/// processes (ADR-0093 §6). A window with the manager open whose part is
/// empty (every pane is a note) still gets its point: the totals are zero,
/// not a gap -- the gap is for the time the manager was closed (RF-24.12).
pub(crate) fn apply_to_window(
    view: &mut TaskManagerView,
    history: &mut TaskManagerHistory,
    taken_at: Instant,
    panes: HashMap<WindowPaneKey, Vec<ProcessStat>>,
    history_seconds: u32,
) {
    view.sample = WindowSample {
        taken_at: Some(taken_at),
        panes,
    };
    history.push(
        HistoryPoint::from_totals(taken_at, view.sample.totals()),
        history_seconds,
    );
}

/// ADR-0098 §3: the part of a window **without** the manager open, with
/// `background_monitoring` on. Only the chart point is kept -- the totals of
/// the part, like [`apply_to_window`]'s -- and no `TaskManagerView` is made:
/// the list is built from the first answer after the manager opens.
pub(crate) fn apply_to_history(
    history: &mut TaskManagerHistory,
    taken_at: Instant,
    panes: &HashMap<WindowPaneKey, Vec<ProcessStat>>,
    history_seconds: u32,
) {
    history.push(
        HistoryPoint::from_totals(
            taken_at,
            crate::task_manager::totals(panes.values().flatten()),
        ),
        history_seconds,
    );
}

/// The fact the list needs about a pane that has a runtime
/// ([`PaneFact`]'s rule, in the order it documents): SSH first (dormant or
/// connected, there is no local process), then an exited pane, then a local
/// pane with the root of its `ProcessGroup`.
pub(crate) fn pane_fact(is_ssh: bool, exited: bool, root_pid: Option<u32>) -> PaneFact {
    if is_ssh {
        PaneFact::Ssh
    } else if exited {
        PaneFact::Exited
    } else {
        PaneFact::Local { root_pid }
    }
}

/// Re-resolves the selection and the scroll of `view` against a freshly
/// built list (RF-24.18, ADR-0095 §5): the selection is looked up by key and,
/// when it is gone, falls to the tab row and then to the first row. The
/// scroll is kept as is and clamped to the list; it follows the selection
/// only when `follow` is set or when the selection **moved on its own** (the
/// row it was on died), so a list the user scrolled away from the selection
/// is not pulled back every second.
pub(crate) fn resolve_view(
    view: &mut TaskManagerView,
    rows: &[Row],
    visible_rows: usize,
    follow: bool,
) {
    let before = view.selection;
    let index = resolve_selection(rows, before.as_ref());
    view.selection = index.map(|index| rows[index].key);
    if let Some(index) = index
        && (follow || view.selection != before)
    {
        view.first_visible = keep_visible(view.first_visible, index, visible_rows);
    }
    view.first_visible = clamp_scroll(view.first_visible, rows.len(), visible_rows);
    if view
        .hover
        .as_ref()
        .is_some_and(|key| !rows.iter().any(|row| &row.key == key))
    {
        view.hover = None;
    }
    // A menu is of the row it was opened on: a process that died under it
    // takes the menu along, rather than leaving items that act on nothing.
    if view
        .menu
        .as_ref()
        .is_some_and(|menu| !rows.iter().any(|row| row.key == menu.row))
    {
        view.menu = None;
    }
}

/// RF-24.26: whether the "period adjusted" notice is due for the loaded value
/// `raw`, and what is remembered afterwards. Once per loaded value: the same
/// bad value does not repeat it, another bad value does, and a good value
/// forgets, so going back to a bad one later is told again.
pub(crate) fn history_notice_step(raw: u32, warned: Option<u32>) -> (bool, Option<u32>) {
    if !effective_history(raw).adjusted {
        return (false, None);
    }
    if warned == Some(raw) {
        return (false, warned);
    }
    (true, Some(raw))
}

// --- the footer's toggle (ADR-0098 §4) ---------------------------------------

/// The key the footer's toggle writes.
pub(crate) const BACKGROUND_MONITORING_KEY: &str = "task_manager.background_monitoring";

/// The one edit of the toggle: the key set to the opposite of `current`, the
/// value of the `Config` in use (the toggle never keeps a state of its own).
pub(crate) fn background_monitoring_edit(current: bool) -> Edit {
    Edit::Set(
        KeyPath::parse(BACKGROUND_MONITORING_KEY).expect("a valid key path"),
        EditValue::Bool(!current),
    )
}

/// Writes `edit` to `path` (ADR-0098 §4): over `base`, the file's text as
/// just read, or -- `None`, no file yet -- over `template`, the embedded
/// example (RF-16.21). A file that changed between the read and the write
/// (`EditError::Changed`) is rebased on its new text and the edit is applied
/// **once** more: an edit by key lands on whatever the file now holds. A
/// second `Changed`, like any other error, is returned and nothing was
/// written. Who applies the value is the hot reload, never this function.
pub(crate) fn save_background_monitoring(
    path: &Path,
    base: Option<&str>,
    template: &str,
    edit: &Edit,
) -> Result<(), EditError> {
    let mut document = match base {
        Some(text) => ConfigDocument::parse(text),
        None => ConfigDocument::from_template(template),
    }?;
    let edits = std::slice::from_ref(edit);
    match document.save(path, edits) {
        Err(EditError::Changed { current }) => {
            document.rebase(&current)?;
            document.save(path, edits).map(|_| ())
        }
        other => other.map(|_| ()),
    }
}

// --- keyboard (ADR-0095 §3) -------------------------------------------------

/// Rows the list moves per wheel notch. An interaction constant, not an
/// appearance value: the usual three lines of a notch.
const WHEEL_ROWS_PER_NOTCH: f32 = 3.0;

/// What the key chain link does with a key while the manager is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskKeyRoute {
    /// Move the selection (and keep it in view).
    Navigate(ListNav),
    /// "Go to tab" on the selected row (`Enter`).
    GoToTab,
    /// Back to the terminal (`Esc`).
    Leave,
    /// Open the row menu on the selected row (menu key, `Shift+F10`).
    OpenMenu,
    /// An action that does not target a pane: the caller hands the key to
    /// `handle_tab_action_key`, which resolves and runs it as always.
    Dispatch(Action),
    /// Consumed with no effect: an action that targets a pane (RF-24.5), a
    /// key that matched nothing, a key release. **Never** reaches the pane,
    /// nor the dormant SSH pane that `Enter` would connect.
    Swallow,
}

/// The decision of the key chain link (ADR-0095 §3), pure so the "nothing
/// leaks to the PTY" risk of the ADR is tested without a window. `action` is
/// what the keymap resolves the chord to, if anything.
///
/// The navigation keys count only **bare**: `Ctrl+PageDown` is `tab.next`
/// and `Shift+PageUp` is `scrollback.page_up` (swallowed, it targets the
/// pane), and neither may be taken for a list move.
pub(crate) fn task_manager_key_route(
    pressed: bool,
    key: &Key,
    modifiers: Modifiers,
    action: Option<Action>,
) -> TaskKeyRoute {
    if !pressed {
        return TaskKeyRoute::Swallow;
    }
    let bare = !(modifiers.shift || modifiers.ctrl || modifiers.alt || modifiers.super_);
    if bare && let Key::Named(named) = key {
        match named {
            NamedKey::ArrowUp => return TaskKeyRoute::Navigate(ListNav::Up),
            NamedKey::ArrowDown => return TaskKeyRoute::Navigate(ListNav::Down),
            NamedKey::Home => return TaskKeyRoute::Navigate(ListNav::Home),
            NamedKey::End => return TaskKeyRoute::Navigate(ListNav::End),
            NamedKey::PageUp => return TaskKeyRoute::Navigate(ListNav::PageUp),
            NamedKey::PageDown => return TaskKeyRoute::Navigate(ListNav::PageDown),
            NamedKey::Enter => return TaskKeyRoute::GoToTab,
            NamedKey::Escape => return TaskKeyRoute::Leave,
            _ => {}
        }
    }
    let shift_only = modifiers.shift && !(modifiers.ctrl || modifiers.alt || modifiers.super_);
    if matches!(key, Key::Named(NamedKey::ContextMenu))
        || (shift_only && matches!(key, Key::Named(NamedKey::F10)))
    {
        return TaskKeyRoute::OpenMenu;
    }
    match action {
        Some(action) if !action.targets_pane() => TaskKeyRoute::Dispatch(action),
        _ => TaskKeyRoute::Swallow,
    }
}

/// A selection asked for by key (a screen reader's `Focus` on a row,
/// ADR-0095 §10): the row becomes the selection and the scroll keeps it in
/// view, as a keyboard move does. A key that is not in the list does nothing
/// and returns `false`.
pub(crate) fn select_row(
    view: &mut TaskManagerView,
    rows: &[Row],
    key: RowKey,
    visible_rows: usize,
) -> bool {
    let Some(index) = rows.iter().position(|row| row.key == key) else {
        return false;
    };
    view.selection = Some(key);
    view.first_visible = keep_visible(view.first_visible, index, visible_rows);
    true
}

/// A keyboard move of the selection over the freshly built `rows`, and the
/// scroll that keeps it in view (RF-24.19). A selection that is not in the
/// list any more counts as none.
pub(crate) fn apply_navigation(
    view: &mut TaskManagerView,
    rows: &[Row],
    nav: ListNav,
    visible_rows: usize,
) {
    let current = view
        .selection
        .as_ref()
        .and_then(|key| rows.iter().position(|row| &row.key == key));
    if let Some(index) = rows::navigate(rows, current, nav, visible_rows) {
        view.selection = Some(rows[index].key);
        view.first_visible = keep_visible(view.first_visible, index, visible_rows);
    }
}

// --- mouse (ADR-0095 §4) -----------------------------------------------------

/// The index, in the **whole** list, of the row under `point` (logical
/// pixels): the visible slot [`ListLayout::row_at`] gives plus the scroll.
/// `None` over the header, the rule, the charts, the gap between the columns
/// and the empty space below the last row.
pub(crate) fn row_index_at(
    list: &ListLayout,
    point: (f32, f32),
    first_visible: usize,
    total_rows: usize,
) -> Option<usize> {
    let index = first_visible + list.row_at(point)?;
    (index < total_rows).then_some(index)
}

/// The scroll after `notches` of wheel (positive = wheel up = towards the
/// first row), clamped to the list: never past the end, never before 0.
pub(crate) fn wheel_scroll(
    first_visible: usize,
    notches: f32,
    total_rows: usize,
    visible_rows: usize,
) -> usize {
    let max_first = total_rows.saturating_sub(visible_rows);
    let delta = (-notches * WHEEL_ROWS_PER_NOTCH).round() as i64;
    let next = (first_visible.min(max_first) as i64 + delta).clamp(0, max_first as i64);
    next as usize
}

/// Registers a left click on row `key` and says whether it is the second one
/// of a double click: the same row, within `threshold` of the last. A double
/// click resets, so a third click starts over.
pub(crate) fn register_row_click(
    last: &mut Option<(RowKey, Instant)>,
    key: &RowKey,
    now: Instant,
    threshold: Duration,
) -> bool {
    let double = last.as_ref().is_some_and(|(previous, at)| {
        previous == key && now.saturating_duration_since(*at) <= threshold
    });
    *last = if double { None } else { Some((*key, now)) };
    double
}

// --- go to tab and the return to the terminal (ADR-0095 §2, §7) -----------------

/// "Go to tab" on the core (RF-24.20): activates `tab` by the same call as a
/// click on the bar -- `Workspace::activate_tab`, which expands a collapsed
/// group (ADR-0020's focus ladder) -- and focuses `pane` when the row is of a
/// pane. A tab or pane that is gone does **nothing** and returns `false`
/// (the caller then stays in the manager); the check comes before any change.
pub(crate) fn go_to_tab(ws: &mut Workspace, tab: TabId, pane: Option<PaneId>) -> bool {
    let Some(found) = ws.tab(tab) else {
        return false;
    };
    if let Some(pane) = pane
        && found.panes().pane(pane).is_none()
    {
        return false;
    }
    if !ws.activate_tab(tab) {
        return false;
    }
    if let Some(pane) = pane
        && let Some(found) = ws.tab_mut(tab)
    {
        found.panes_mut().focus(pane);
    }
    true
}

/// What the check of ADR-0095 §2 sees of a window: the active tab and how
/// many tabs it has.
pub(crate) type ActivationSnapshot = (Option<TabId>, usize);

/// Whether the dispatch that went from `before` to `after` was a **closing**:
/// the tab count went down. Closing the active tab changes the active `TabId`,
/// so it has to be told apart from an activation -- and the count is what
/// tells (a gesture that activates or creates never shrinks it; the ones that
/// close are `tab.close`, the "x", the tab and group menus, a confirmed
/// dialog, a pane exit that took its tab, a tab moved to another window).
pub(crate) fn is_closing(before: ActivationSnapshot, after: ActivationSnapshot) -> bool {
    after.1 < before.1
}

/// ADR-0095 §2, "one check only": the manager goes back to the terminal when
/// the dispatch changed the active `TabId` **or** grew the tab count -- unless
/// it was a closing (RF-24.6: closing tabs with the manager open does not
/// close it, even when the active one is the one that went).
pub(crate) fn should_leave(
    before: ActivationSnapshot,
    after: ActivationSnapshot,
    closing: bool,
) -> bool {
    !closing && (after.0 != before.0 || after.1 > before.1)
}

// --- row menu, tooltip and "End process" (step 07b) ---------------------------

/// Where a menu opened from the keyboard hangs (ADR-0095 §7: "on the selected
/// row"): under the row's left edge, so the row stays readable. `index` is the
/// row's place in the **whole** list; `None` when the row is not among the
/// visible ones (the caller scrolls it into view first).
pub(crate) fn keyboard_menu_anchor(
    list: &ListLayout,
    first_visible: usize,
    index: usize,
) -> Option<(f32, f32)> {
    let slot = index.checked_sub(first_visible)?;
    if slot >= list.visible_rows {
        return None;
    }
    let rect = list.row_rect(slot);
    Some((rect.x, rect.y + rect.height))
}

/// The tooltip of the row under `point` (RF-24.16, ADR-0095 §7): the whole
/// name, **only** when the last painted frame cut it. A tab row shows the
/// title, a process row its name, a note row its sentence. The anchor is the
/// row, in the window's logical pixels like the layout.
pub(crate) fn row_tooltip(
    rows: &[Row],
    view: &TaskManagerView,
    list: &ListLayout,
    point: (f32, f32),
    catalog: &Catalog,
) -> Option<(HoverKey, Rect, String)> {
    let index = row_index_at(list, point, view.first_visible, rows.len())?;
    let row = &rows[index];
    if !tm_paint::name_truncated(view, &row.key) {
        return None;
    }
    let text = match &row.kind {
        rows::RowKind::Tab { title, .. } => title.clone(),
        rows::RowKind::Process { stat, .. } => stat.name.clone(),
        rows::RowKind::Note { kind, .. } => crate::task_manager::format::note_text(catalog, *kind),
    };
    let rect = list.row_rect(index - view.first_visible);
    Some((HoverKey::TaskRow(row.key), rect, text))
}

/// How a confirmed "End process" reaches the OS (ADR-0093 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KillRoute {
    /// The sampler thread, which already has a `System`.
    Sampler,
    /// A single-use thread with a fresh `System`: there is no sampler, or it
    /// had already gone when the request was sent.
    Detached,
}

/// The route of a kill. `sampler` is `None` without a sampler and otherwise
/// whether its `send` was accepted.
pub(crate) fn kill_route(sampler: Option<bool>) -> KillRoute {
    match sampler {
        Some(true) => KillRoute::Sampler,
        Some(false) | None => KillRoute::Detached,
    }
}

// ---------------------------------------------------------------------------
// The window's side
// ---------------------------------------------------------------------------

impl WindowState {
    /// The active tab and the tab count, for [`should_leave`].
    pub(crate) fn activation_snapshot(&self) -> ActivationSnapshot {
        (
            self.workspace.active_tab(),
            self.workspace.visual_order().count(),
        )
    }

    /// The wheel over the list (RF-24.19): scrolls it, clamped. `false` when
    /// nothing moved (the point is not over the list, or the list is at the
    /// end it was pushed towards), so no frame is asked for.
    pub(crate) fn task_manager_wheel(
        &mut self,
        point: (f32, f32),
        notches: f32,
        style: &TabBarStyle,
        config: &Config,
    ) -> bool {
        if notches == 0.0 {
            return false;
        }
        let layout = self.task_manager_layout(style, config);
        if !tab_bar::rect_contains(layout.list.column, point) {
            return false;
        }
        let total = self.task_manager_rows().len();
        let Some(view) = self.task_manager.as_mut() else {
            return false;
        };
        let next = wheel_scroll(view.first_visible, notches, total, layout.list.visible_rows);
        let changed = next != view.first_visible;
        view.first_visible = next;
        changed
    }

    /// The row under the mouse becomes the view's hover. `true` when it
    /// changed (the only case that asks for a frame).
    pub(crate) fn task_manager_hover_at(
        &mut self,
        point: (f32, f32),
        style: &TabBarStyle,
        config: &Config,
    ) -> bool {
        let layout = self.task_manager_layout(style, config);
        let first_visible = self
            .task_manager
            .as_ref()
            .map_or(0, |view| view.first_visible);
        // The list is only built when the point is over a row's slot: the
        // movement over the charts and the margins costs nothing.
        let over_a_slot = layout.list.row_at(point).is_some();
        let hovered = if over_a_slot {
            let rows = self.task_manager_rows();
            row_index_at(&layout.list, point, first_visible, rows.len())
                .map(|index| rows[index].key)
        } else {
            None
        };
        let Some(view) = self.task_manager.as_mut() else {
            return false;
        };
        if view.hover == hovered {
            return false;
        }
        view.hover = hovered;
        true
    }

    /// Clears the tab selection of the bar and applies [`go_to_tab`], as the
    /// click on a tab does (RF-2.3), then the title of the window follows the
    /// focused pane. `false` when the row's tab or pane is gone.
    fn go_to_tab_row(&mut self, tab: TabId, pane: Option<PaneId>) -> bool {
        let went = go_to_tab(&mut self.workspace, tab, pane);
        if went {
            self.selection.clear();
            self.mark_session_dirty();
            self.sync_window_title();
        }
        went
    }

    /// What the list knows about each pane of the window (ADR-0095 §5): one
    /// entry per pane **with a runtime** -- a pane of a `NotStarted` tab has
    /// none and is absent, which the list shows as the "starts on first
    /// focus" note. Step 07 reuses this for the actions of a row.
    pub(crate) fn task_manager_pane_facts(&self) -> PaneFacts {
        self.panes
            .iter()
            .map(|(&(tab, pane), runtime)| {
                let exited = self
                    .workspace
                    .tab(tab)
                    .and_then(|t| t.panes().pane(pane))
                    .is_some_and(porecatu_core::Pane::is_exited);
                let root_pid = runtime.terminal.process_group().map(|g| g.root_pid());
                (
                    (tab, pane),
                    pane_fact(runtime.ssh_host.is_some(), exited, root_pid),
                )
            })
            .collect()
    }

    /// The list as it is now: the workspace, the facts and the window's last
    /// sample. Empty without the mode.
    pub(crate) fn task_manager_rows(&self) -> Vec<Row> {
        match &self.task_manager {
            Some(view) => rows::build_rows(
                &self.workspace,
                &self.task_manager_pane_facts(),
                &view.sample,
            ),
            None => Vec::new(),
        }
    }

    /// The geometry of the manager in this window: the one [`tm_paint::layout`]
    /// of the terminal box (`paint::terminal_box_rect`, the same box the panes
    /// use, so toggling the mode resizes nothing). The painting, the sample
    /// application and the hit-test of step 07 all read it from here.
    pub(crate) fn task_manager_layout(
        &self,
        style: &TabBarStyle,
        config: &Config,
    ) -> TaskManagerLayout {
        let box_rect = crate::paint::terminal_box_rect(
            style,
            bar_height(style),
            status_bar_height(style),
            self.logical_width,
            self.logical_height,
        );
        tm_paint::layout(box_rect, &TaskManagerStyle::from_config(config, style))
    }

    /// Rebuilds the list and re-resolves the view's selection and scroll
    /// ([`resolve_view`]). No-op without the mode.
    pub(crate) fn refresh_task_manager_view(
        &mut self,
        style: &TabBarStyle,
        config: &Config,
        follow: bool,
    ) {
        if self.task_manager.is_none() {
            return;
        }
        let rows = self.task_manager_rows();
        let visible_rows = self.task_manager_layout(style, config).list.visible_rows;
        if let Some(view) = self.task_manager.as_mut() {
            resolve_view(view, &rows, visible_rows, follow);
        }
    }

    /// The items of the open row menu, recomputed from the live row (the menu
    /// only keeps the row's identity, like the terminal menu). `None` without
    /// a menu, and when its row is gone.
    pub(crate) fn open_task_menu_items(&self) -> Option<Vec<TaskMenuItem>> {
        let menu = self.task_manager.as_ref()?.menu?;
        let rows = self.task_manager_rows();
        let row = rows.iter().find(|row| row.key == menu.row)?;
        Some(task_menu_items(row))
    }

    /// The tooltip target under `point`, if any (RF-24.16). Nothing while a
    /// menu is open. The list is only built when the point is over a row's
    /// slot, like the hover.
    pub(crate) fn task_manager_tooltip_at(
        &self,
        point: (f32, f32),
        style: &TabBarStyle,
        config: &Config,
    ) -> Option<(HoverKey, Rect, String)> {
        let view = self.task_manager.as_ref()?;
        if view.menu.is_some() {
            return None;
        }
        let layout = self.task_manager_layout(style, config);
        layout.list.row_at(point)?;
        let rows = self.task_manager_rows();
        row_tooltip(&rows, view, &layout.list, point, &self.catalog)
    }
}

/// ADR-0095 §4, ADR-0096 §1: paints the task manager where the panes would
/// be. The frame background goes first, by the same path as a pane's (color,
/// opacity, image, acrylic, no cells), then the manager over it, both in
/// `out` (`Layer::Grid`). `rect` is the terminal box. The scroll is clamped
/// to the list before painting (the painting trusts it, `paint` documents
/// that), and a selection that no longer exists is re-resolved, so what is
/// drawn is always consistent with the list of this frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_task_manager(
    out: &mut Vec<Primitive>,
    state: &mut WindowState,
    rect: Rect,
    style: &TabBarStyle,
    config: &Config,
    pal: &ResolvedPalette,
    term_pal: &ResolvedTermPalette,
    background_image: Option<crate::paint::BackgroundImagePaint>,
    catalog: &Catalog,
    measurer: &mut TextMeasurer,
) {
    crate::paint::push_frame_background(out, rect, style, term_pal, background_image);

    let rows = state.task_manager_rows();
    let tm_style = TaskManagerStyle::from_config(config, style);
    let geometry = tm_paint::layout(rect, &tm_style);
    // RF-24.26/RF-24.27: read from the current config on every painting.
    let history_seconds = effective_history(config.task_manager.history_seconds).seconds;

    let WindowState {
        task_manager,
        task_manager_history,
        ..
    } = state;
    let Some(view) = task_manager.as_mut() else {
        return;
    };
    resolve_view(view, &rows, geometry.list.visible_rows, false);
    tm_paint::paint(
        out,
        rect,
        view,
        &rows,
        task_manager_history,
        history_seconds,
        config.task_manager.background_monitoring,
        &tm_style,
        pal,
        catalog,
        measurer,
    );
}

// ---------------------------------------------------------------------------
// The App's side
// ---------------------------------------------------------------------------

impl App {
    /// ADR-0095 §2, release of the app button and `task_manager.toggle`.
    pub(crate) fn toggle_task_manager(&mut self, window_id: WindowId) {
        let open = self
            .windows
            .get(&window_id)
            .is_some_and(|state| state.task_manager.is_some());
        if open {
            self.leave_task_manager(window_id);
        } else {
            self.enter_task_manager(window_id);
        }
    }

    /// ADR-0095 §2: opens the task manager in `window_id`. The selection
    /// starts on the active tab's row; the popovers, the terminal menu and the
    /// search (the frame it floated over is no longer in view) are closed;
    /// the sampler is created if no other window had one. No-op if already
    /// open. The first sample arrives within a second (a second window joins
    /// the running clock, it does not force an extra scan: a CPU reading
    /// between two scans too close in time is noise).
    pub(crate) fn enter_task_manager(&mut self, window_id: WindowId) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        if state.task_manager.is_some() {
            return;
        }
        state.close_all_popovers();
        state.search = None;
        state.hover.dismiss();
        let selection = state.workspace.active_tab().map(RowKey::Tab);
        state.task_manager = Some(TaskManagerView::new(selection));
        // Start where the user is: the active tab's row in view.
        state.refresh_task_manager_view(&self.style, &self.config, true);
        state.window.request_redraw();

        self.ensure_task_manager_sampler();
        if self.task_manager_next_sample.is_none() {
            self.task_manager_next_sample = Some(Instant::now());
        }
    }

    /// ADR-0095 §2: back to the terminal. The chart history stays (RF-24.12);
    /// the sampler goes if no other window has the manager open. The one
    /// function every gesture that returns to the terminal calls (step 07).
    pub(crate) fn leave_task_manager(&mut self, window_id: WindowId) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        if state.task_manager.take().is_none() {
            return;
        }
        state.window.request_redraw();
        self.stop_task_manager_sampler_if_unused();
    }

    /// Drops the sampler and the clock when no window has the manager open
    /// (ADR-0093 §4). Dropping the `Sampler` closes its channel: the thread
    /// ends by itself and releases its `System`; it is **not** joined here, the
    /// main thread never waits for it. Idempotent.
    pub(crate) fn stop_task_manager_sampler_if_unused(&mut self) {
        if self.task_manager_sampler_needed() {
            return;
        }
        self.task_manager_sampler = None;
        self.task_manager_next_sample = None;
        self.task_manager_ticket.forget_pending();
    }

    /// [`sampler_needed`] for the app as it is now.
    fn task_manager_sampler_needed(&self) -> bool {
        sampler_needed(
            self.config.task_manager.background_monitoring,
            self.windows.values().map(|w| w.task_manager.is_some()),
        )
    }

    /// ADR-0098 §2, from `schedule_next_wake`: with `background_monitoring`
    /// on, the sampling clock runs from the startup on -- and from the reload
    /// that turned it on -- without waiting for a manager to open; turned off
    /// with every manager closed, the sampler goes now rather than at the
    /// next deadline. Only the deadline is set here: the thread is created by
    /// [`Self::task_manager_tick`], as when the first manager opens.
    pub(crate) fn sync_task_manager_sampler(&mut self, now: Instant) {
        if self.windows.is_empty() {
            return;
        }
        if self.task_manager_sampler_needed() {
            if self.task_manager_next_sample.is_none() {
                self.task_manager_next_sample = Some(now);
            }
        } else if self.task_manager_next_sample.is_some() || self.task_manager_sampler.is_some() {
            self.stop_task_manager_sampler_if_unused();
        }
    }

    /// Starts the sampler thread if there is none. The callback runs **on the
    /// thread** and only sends the answer, as data, through the proxy
    /// (`Wakeup::ProcessSample`). A failure to spawn the thread leaves no
    /// sampler: the deadline tries again at its next turn.
    fn ensure_task_manager_sampler(&mut self) {
        if self.task_manager_sampler.is_some() {
            return;
        }
        let proxy = self.proxy.clone();
        self.task_manager_sampler = Sampler::spawn(move |result| {
            let _ = proxy.send_event(Wakeup::ProcessSample(Box::new(result)));
        })
        .ok();
    }

    /// ADR-0093 §4: runs at the sampling deadline, from `tick_all`.
    ///
    /// - A previous answer still on its way: nothing is sent, only the next
    ///   deadline is set (no queue forms, a slow machine samples less often).
    /// - Otherwise one request with the roots of **every** measured window
    ///   ([`window_measured`]: the manager open, or `background_monitoring`
    ///   on, ADR-0098 §3).
    /// - A `send` that fails (the thread is gone) drops the sampler; it is
    ///   recreated at the next deadline, one second later. A thread that
    ///   cannot be created at all would retry once a second, which costs
    ///   nothing noticeable and is only the error case.
    pub(crate) fn task_manager_tick(&mut self, now: Instant) {
        let Some(due) = self.task_manager_next_sample else {
            return;
        };
        if now < due {
            return;
        }
        if !self.task_manager_sampler_needed() {
            self.stop_task_manager_sampler_if_unused();
            return;
        }
        if self.task_manager_ticket.is_lost(now) {
            self.task_manager_sampler = None;
            self.task_manager_ticket.forget_pending();
        }
        self.task_manager_next_sample = Some(now + SAMPLE_PERIOD);
        if self.task_manager_ticket.is_pending() {
            return;
        }
        self.ensure_task_manager_sampler();
        let background = self.config.task_manager.background_monitoring;
        let roots = sample_roots(self.windows.iter().map(|(&id, state)| {
            let measured = window_measured(state.task_manager.is_some(), background);
            let panes = if measured {
                state
                    .panes
                    .iter()
                    .map(|(&key, runtime)| (key, runtime.terminal.process_group()))
                    .collect()
            } else {
                Vec::new()
            };
            (id, measured, panes)
        }));
        let seq = self.task_manager_ticket.next_seq();
        let delivered = self.task_manager_sampler.as_ref().is_some_and(|sampler| {
            sampler
                .send(SamplerRequest::Sample(SampleRequest { seq, roots }))
                .is_ok()
        });
        if delivered {
            self.task_manager_ticket.sent(seq, now);
        } else {
            self.task_manager_sampler = None;
            self.task_manager_ticket.forget_pending();
        }
    }

    /// ADR-0093 §6, ADR-0098 §3: applies the sampler's answer. Dropped when
    /// its `seq` is not the one waited for. Each window that still exists and
    /// still has the manager open gets its part (`partition_sample`,
    /// `apply_to_window`), re-resolves its selection and scroll, and
    /// repaints. With `background_monitoring` on, a window with the manager
    /// closed gets only the chart point (`apply_to_history`), no view and no
    /// frame. A closed window and a key whose pane is gone are ignored.
    pub(crate) fn apply_process_sample(&mut self, result: SampleResult<PaneKey>) {
        if !self.task_manager_ticket.accept(result.seq) {
            return;
        }
        let history_seconds = effective_history(self.config.task_manager.history_seconds).seconds;
        let background = self.config.task_manager.background_monitoring;
        let SampleResult {
            taken_at, panes, ..
        } = result;
        let mut parts = partition_sample(panes, |&(window, tab, pane)| {
            self.windows.get(&window).is_some_and(|state| {
                window_measured(state.task_manager.is_some(), background)
                    && state
                        .workspace
                        .tab(tab)
                        .is_some_and(|t| t.panes().pane(pane).is_some())
            })
        });
        let (style, config) = (&self.style, &self.config);
        for (id, state) in self.windows.iter_mut() {
            let WindowState {
                task_manager,
                task_manager_history,
                ..
            } = state;
            let part = parts.remove(id).unwrap_or_default();
            let Some(view) = task_manager.as_mut() else {
                if background {
                    apply_to_history(task_manager_history, taken_at, &part, history_seconds);
                }
                continue;
            };
            apply_to_window(view, task_manager_history, taken_at, part, history_seconds);
            state.refresh_task_manager_view(style, config, false);
            state.window.request_redraw();
        }
    }

    /// RF-24.26: tells, once per loaded value, that `history_seconds` was
    /// moved to the nearest limit. Same shape and window as the Git interval
    /// notice (`Severity::Info` in every window), checked from
    /// `schedule_next_wake`.
    pub(crate) fn check_task_manager_history_notice(&mut self, now: Instant) {
        let raw = self.config.task_manager.history_seconds;
        let (due, warned) = history_notice_step(raw, self.task_manager_history_warned);
        self.task_manager_history_warned = warned;
        if !due {
            return;
        }
        let used = effective_history(raw).seconds;
        for state in self.windows.values_mut() {
            state.warnings.push(
                Severity::Info,
                msg::notice::task_manager_history_adjusted::title(&self.catalog),
                msg::notice::task_manager_history_adjusted::body(&self.catalog, raw, used),
                now,
            );
            state.window.request_redraw();
        }
    }

    // --- step 07a: entry, mouse and the return to the terminal ---------------

    /// ADR-0095 §2, the beginning of the one check: the snapshot of every
    /// window that has the manager open, taken before an event is handled.
    /// Every window, not only the event's, so a tab that **arrives** from
    /// another window (a transfer is made from the origin's event) is seen at
    /// its destination too. Empty -- no allocation -- when no window has it.
    pub(crate) fn task_manager_guard_begin(&self) -> Vec<(WindowId, ActivationSnapshot)> {
        self.windows
            .iter()
            .filter(|(_, state)| state.task_manager.is_some())
            .map(|(&id, state)| (id, state.activation_snapshot()))
            .collect()
    }

    /// ADR-0095 §2, the end of the one check: runs once per window event,
    /// after any keyboard, mouse, menu, popover, restore or transfer path
    /// had its say. A window whose active `TabId` changed or whose tab count
    /// grew goes back to the terminal ([`should_leave`]); a closing does not
    /// ([`is_closing`], RF-24.6). The click on a tab of the bar or on a pinned
    /// box leaves even when the tab was already the active one (the flag
    /// `task_manager_tab_click`), the one activation this check cannot see.
    pub(crate) fn task_manager_guard_end(
        &mut self,
        event_window: WindowId,
        guard: Vec<(WindowId, ActivationSnapshot)>,
    ) {
        let clicked = self
            .windows
            .get_mut(&event_window)
            .is_some_and(|state| std::mem::take(&mut state.task_manager_tab_click));
        for (id, before) in guard {
            let Some(state) = self.windows.get(&id) else {
                continue;
            };
            if state.task_manager.is_none() {
                continue;
            }
            let after = state.activation_snapshot();
            let explicit = clicked && id == event_window;
            if explicit || should_leave(before, after, is_closing(before, after)) {
                self.leave_task_manager(id);
            }
        }
    }

    /// Runs what the key chain link decided ([`task_manager_key_route`]);
    /// `Dispatch` is not here, the caller sends it down the chain.
    pub(crate) fn run_task_manager_route(&mut self, window_id: WindowId, route: TaskKeyRoute) {
        match route {
            TaskKeyRoute::Navigate(nav) => self.task_manager_navigate(window_id, nav),
            TaskKeyRoute::GoToTab => self.task_manager_go_to_selected(window_id),
            TaskKeyRoute::Leave => self.leave_task_manager(window_id),
            TaskKeyRoute::OpenMenu => self.open_task_manager_menu(window_id, None),
            TaskKeyRoute::Dispatch(_) | TaskKeyRoute::Swallow => {}
        }
        if let Some(state) = self.windows.get(&window_id) {
            state.window.request_redraw();
        }
    }

    /// `↑ ↓ Home End PageUp PageDown`: the page is the rows the list shows.
    fn task_manager_navigate(&mut self, window_id: WindowId, nav: ListNav) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let rows = state.task_manager_rows();
        let visible_rows = state
            .task_manager_layout(&self.style, &self.config)
            .list
            .visible_rows;
        if let Some(view) = state.task_manager.as_mut() {
            apply_navigation(view, &rows, nav, visible_rows);
        }
    }

    /// `Enter`: "Go to tab" on the selected row.
    fn task_manager_go_to_selected(&mut self, window_id: WindowId) {
        let selected = self
            .windows
            .get(&window_id)
            .and_then(|state| state.task_manager.as_ref())
            .and_then(|view| view.selection);
        if let Some(key) = selected {
            self.task_manager_go_to(window_id, &key);
        }
    }

    /// "Go to tab" (RF-24.20, ADR-0095 §7): activates the row's tab by the
    /// same path as a click on the bar, focuses the pane of the row (a tab row
    /// only activates), scrolls the track to the tab, and goes back to the
    /// terminal. A tab or pane that is gone: nothing happens, the manager
    /// stays. The double click, `Enter` and (step 07b) the menu item all end
    /// here.
    pub(crate) fn task_manager_go_to(&mut self, window_id: WindowId, key: &RowKey) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        if !state.go_to_tab_row(key.tab(), key.pane()) {
            return;
        }
        if let Some(gpu) = self.gpu.as_mut() {
            state.ensure_active_tab_visible(gpu, &self.style);
        }
        state.window.request_redraw();
        self.leave_task_manager(window_id);
    }

    /// A press on the content area while the manager is open (ADR-0095 §4).
    /// Left: selects the row under the point; a second click on it within
    /// the double-click time is "Go to tab". Right: selects the row and asks
    /// for its menu at the point. Over the header, the charts or the empty
    /// space: nothing (a left click forgets the pending double click).
    pub(crate) fn task_manager_press(
        &mut self,
        window_id: WindowId,
        point: (f32, f32),
        button: MouseButton,
    ) {
        if !matches!(button, MouseButton::Left | MouseButton::Right) {
            return;
        }
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        // A click dismisses the tooltip (ADR-0019).
        state.hover.dismiss();
        let rows = state.task_manager_rows();
        let layout = state.task_manager_layout(&self.style, &self.config);
        let Some(view) = state.task_manager.as_mut() else {
            return;
        };
        // ADR-0098 §5: the footer's toggle arms on the press; the release
        // over it flips the key ([`Self::task_manager_release`]).
        view.footer_pressed = button == MouseButton::Left
            && layout
                .list
                .footer_contains(point, view.measure.footer_label_width());
        if view.footer_pressed {
            view.last_click = None;
            return;
        }
        let Some(index) = row_index_at(&layout.list, point, view.first_visible, rows.len()) else {
            if button == MouseButton::Left {
                view.last_click = None;
            }
            return;
        };
        let key = rows[index].key;
        view.selection = Some(key);
        match button {
            MouseButton::Left => {
                let double = register_row_click(
                    &mut view.last_click,
                    &key,
                    Instant::now(),
                    DOUBLE_CLICK_THRESHOLD,
                );
                if double {
                    self.task_manager_go_to(window_id, &key);
                }
            }
            _ => self.open_task_manager_menu(window_id, Some(point)),
        }
    }

    /// The left release of a press the manager consumed (ADR-0098 §5): over
    /// the footer's toggle that was armed by the press, it flips
    /// `background_monitoring`. Anywhere else nothing happens.
    pub(crate) fn task_manager_release(&mut self, window_id: WindowId, point: (f32, f32)) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let layout = state.task_manager_layout(&self.style, &self.config);
        let Some(view) = state.task_manager.as_mut() else {
            return;
        };
        let armed = std::mem::take(&mut view.footer_pressed);
        if armed
            && layout
                .list
                .footer_contains(point, view.measure.footer_label_width())
        {
            self.toggle_background_monitoring(window_id);
        }
    }

    /// ADR-0098 §4: writes the opposite of the `Config`'s
    /// `background_monitoring` to the config file in use, by key. Nothing
    /// changes in memory: the watcher sees the file and the hot reload brings
    /// the value, which the toggle then draws. A failure is a warning in
    /// `window_id`, and the toggle stays as it was.
    pub(crate) fn toggle_background_monitoring(&mut self, window_id: WindowId) {
        let now = Instant::now();
        let edit = background_monitoring_edit(self.config.task_manager.background_monitoring);
        let result = match self.config_path.clone() {
            None => Err(None),
            Some(path) => {
                let base = match std::fs::read_to_string(&path) {
                    Ok(text) => Ok(Some(text)),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(err) => Err(EditError::Io {
                        path: path.clone(),
                        kind: err.kind(),
                        cause: err.to_string(),
                    }),
                };
                let created = matches!(base, Ok(None));
                base.and_then(|base| {
                    save_background_monitoring(&path, base.as_deref(), EXAMPLE_CONFIG_TOML, &edit)
                })
                .map(|()| {
                    // A file born from the example gets the theme files next
                    // to it, as the settings screen's first save does
                    // (ADR-0067). Failing here does not undo the save.
                    if created && let Some(dir) = path.parent() {
                        let _ = write_missing_theme_files(dir);
                    }
                })
                .map_err(Some)
            }
        };
        let Err(error) = result else {
            return;
        };
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let (title, body) = match error {
            None => (
                msg::notice::config_path_unresolved::title(&self.catalog),
                msg::notice::config_path_unresolved::body(&self.catalog),
            ),
            Some(error) => (
                msg::notice::task_manager_background_save_failed::title(&self.catalog),
                messages::settings_save_error(
                    &self.catalog,
                    &crate::settings::save::SaveError::Edit(error),
                ),
            ),
        };
        state.warnings.push(Severity::Warning, title, body, now);
        state.window.request_redraw();
    }

    /// ADR-0095 §10: an action a screen reader asked for on a node of the
    /// task manager (the app button's `Click`, a row's `Focus` and
    /// `ShowContextMenu`), taken to the same paths as the click and the
    /// keyboard. [`task_manager_access_route`] decides what it means (pure,
    /// tested); a node that is not the manager's, or a window that is not a
    /// terminal window, does nothing.
    pub(crate) fn run_task_manager_access_action(
        &mut self,
        window_id: WindowId,
        target: accesskit::NodeId,
        action: accesskit::Action,
    ) {
        let Some(state) = self.windows.get(&window_id) else {
            return;
        };
        let rows = state.task_manager_rows();
        match task_manager_access_route(target, action, &rows) {
            Some(TaskAccessAction::Toggle) => self.toggle_task_manager(window_id),
            Some(TaskAccessAction::ToggleBackground) => {
                self.toggle_background_monitoring(window_id);
            }
            Some(TaskAccessAction::SelectRow(key)) => {
                self.select_task_manager_row(window_id, key);
            }
            Some(TaskAccessAction::OpenRowMenu(key)) => {
                self.select_task_manager_row(window_id, key);
                self.open_task_manager_menu(window_id, None);
            }
            None => {}
        }
    }

    /// Selects `key` and scrolls it into view, as a key move does.
    fn select_task_manager_row(&mut self, window_id: WindowId, key: RowKey) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let rows = state.task_manager_rows();
        let visible_rows = state
            .task_manager_layout(&self.style, &self.config)
            .list
            .visible_rows;
        if let Some(view) = state.task_manager.as_mut() {
            select_row(view, &rows, key, visible_rows);
        }
        state.window.request_redraw();
    }

    /// Opens the row menu (ADR-0095 §7). `anchor` is the logical point of the
    /// right click, or `None` when it comes from the keyboard (the menu key,
    /// `Shift+F10`): the menu then hangs under the selected row, which is
    /// scrolled into view first. The row is already selected when this is
    /// called. Any other menu or popover closes (they exclude each other), as
    /// does the tooltip.
    pub(crate) fn open_task_manager_menu(
        &mut self,
        window_id: WindowId,
        anchor: Option<(f32, f32)>,
    ) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        if state.task_manager.is_none() {
            return;
        }
        state.close_all_popovers();
        state.hover.dismiss();
        let rows = state.task_manager_rows();
        let layout = state.task_manager_layout(&self.style, &self.config);
        let Some(view) = state.task_manager.as_mut() else {
            return;
        };
        let Some(selection) = view.selection else {
            return;
        };
        let Some(index) = rows.iter().position(|row| row.key == selection) else {
            return;
        };
        let anchor = match anchor {
            Some(point) => point,
            None => {
                view.first_visible =
                    keep_visible(view.first_visible, index, layout.list.visible_rows);
                let Some(point) = keyboard_menu_anchor(&layout.list, view.first_visible, index)
                else {
                    return;
                };
                point
            }
        };
        let items = task_menu_items(&rows[index]);
        view.menu = Some(TaskMenu::new(selection, anchor, &items));
        state.window.request_redraw();
    }

    /// A key while the row menu is open (ADR-0095 §3, item 1): the menu
    /// consumes it, like the other context menus. `Esc` closes only the menu,
    /// not the manager.
    pub(crate) fn task_manager_menu_key(&mut self, window_id: WindowId, event: &KeyEvent) {
        let route = task_menu_key(event.state == ElementState::Pressed, &event.logical_key);
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let items = state.open_task_menu_items();
        let Some(view) = state.task_manager.as_mut() else {
            return;
        };
        let Some(menu) = view.menu else {
            return;
        };
        // The row is gone (a sample took it): the menu has nothing to act on.
        let Some(items) = items else {
            view.menu = None;
            state.window.request_redraw();
            return;
        };
        let mut chosen = None;
        match route {
            TaskMenuKey::Close => view.menu = None,
            TaskMenuKey::Move(delta) => {
                if let Some(open) = view.menu.as_mut() {
                    open.move_highlight(delta, &items);
                }
            }
            TaskMenuKey::Activate => {
                view.menu = None;
                chosen = menu.selected(&items);
            }
            TaskMenuKey::Swallow => {}
        }
        state.window.request_redraw();
        if let Some(action) = chosen {
            self.run_task_menu_action(window_id, menu.row, action);
        }
    }

    /// Runs `action` of the menu of `row` on the **live** row: "Go to tab" is
    /// the path of the double click, "End process" opens the confirmation
    /// dialog (RF-24.21) -- never the kill itself. A row that is gone, or that
    /// the action does not apply to, does nothing.
    pub(crate) fn run_task_menu_action(
        &mut self,
        window_id: WindowId,
        row: RowKey,
        action: TaskMenuAction,
    ) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let rows = state.task_manager_rows();
        let Some(found) = rows.iter().find(|candidate| candidate.key == row) else {
            return;
        };
        match run_menu_action(action, found, &self.catalog) {
            TaskMenuOutcome::GoTo(key) => self.task_manager_go_to(window_id, &key),
            TaskMenuOutcome::Confirm(dialog) => {
                state.hover.dismiss();
                state.dialog = Some(*dialog);
                state.window.request_redraw();
            }
            TaskMenuOutcome::Nothing => {}
        }
    }

    /// "End process" confirmed (RF-24.21, ADR-0093 §7). Looks the pane up in
    /// **this** window: a pane that is gone, or that has no process group,
    /// does nothing. The request goes to the sampler thread; without it (or
    /// when it had already gone) to a single-use thread. The identity check
    /// (PID, start time, membership in the group, the root refused) is done
    /// there, and a process that already died is not an error (RF-24.23): no
    /// notice, no dialog. The main thread never lists processes.
    pub(crate) fn end_process_confirmed(
        &mut self,
        window_id: WindowId,
        tab: TabId,
        pane: PaneId,
        pid: u32,
        start_time: u64,
    ) {
        let Some(group) = self
            .windows
            .get(&window_id)
            .and_then(|state| state.panes.get(&(tab, pane)))
            .and_then(|runtime| runtime.terminal.process_group())
        else {
            return;
        };
        let request = KillRequest {
            group,
            pid,
            start_time,
        };
        let delivered = self
            .task_manager_sampler
            .as_ref()
            .map(|sampler| sampler.send(SamplerRequest::Kill(request.clone())).is_ok());
        match kill_route(delivered) {
            KillRoute::Sampler => {}
            KillRoute::Detached => {
                if delivered == Some(false) {
                    // The thread is gone: recreated at the next deadline.
                    self.task_manager_sampler = None;
                    self.task_manager_ticket.forget_pending();
                }
                porecatu_term::kill_detached(request);
            }
        }
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

    fn window(n: u64) -> WindowId {
        WindowId::from(n)
    }

    fn stat(pid: u32, cpu: Option<f32>, memory: u64) -> ProcessStat {
        ProcessStat {
            pid,
            parent: None,
            start_time: 1,
            name: format!("p{pid}"),
            cpu,
            memory,
            disk_per_sec: 0.0,
        }
    }

    // --- ticket -----------------------------------------------------------

    #[test]
    fn a_stale_seq_is_refused_and_the_right_one_clears_the_pending_state() {
        let now = Instant::now();
        let mut ticket = SampleTicket::default();
        assert!(!ticket.is_pending());
        let first = ticket.next_seq();
        ticket.sent(first, now);
        assert!(ticket.is_pending());

        // The next request: a new seq; the answer to the old one is stale.
        ticket.forget_pending();
        let second = ticket.next_seq();
        assert_ne!(first, second);
        ticket.sent(second, now);
        assert!(!ticket.accept(first), "an old seq is dropped");
        assert!(ticket.is_pending(), "and does not clear the pending state");
        assert!(ticket.accept(second));
        assert!(!ticket.is_pending());
        // The same answer twice is not applied twice.
        assert!(!ticket.accept(second));
    }

    #[test]
    fn an_answer_after_the_sampler_was_dropped_is_refused() {
        let now = Instant::now();
        let mut ticket = SampleTicket::default();
        let seq = ticket.next_seq();
        ticket.sent(seq, now);
        ticket.forget_pending();
        assert!(!ticket.accept(seq));
        // The next request carries a new seq, so even a late answer of the
        // old sampler cannot be taken for it.
        assert_ne!(ticket.next_seq(), seq);
    }

    #[test]
    fn a_pending_answer_is_lost_only_after_the_limit() {
        let now = Instant::now();
        let mut ticket = SampleTicket::default();
        assert!(!ticket.is_lost(now));
        ticket.sent(1, now);
        assert!(!ticket.is_lost(now + Duration::from_secs(9)));
        assert!(ticket.is_lost(now + ANSWER_LOST_AFTER));
        ticket.forget_pending();
        assert!(!ticket.is_lost(now + Duration::from_secs(60)));
    }

    // --- sampler needed ---------------------------------------------------

    #[test]
    fn the_sampler_is_needed_while_any_window_has_the_manager_open() {
        assert!(!sampler_needed(false, []));
        assert!(!sampler_needed(false, [false, false]));
        assert!(sampler_needed(false, [false, true]));
        // Leaving the last open window is what makes it unneeded.
        assert!(sampler_needed(false, [true, false]));
        assert!(!sampler_needed(false, [false, false]));
    }

    #[test]
    fn with_background_monitoring_the_sampler_is_needed_with_every_manager_closed() {
        // ADR-0098 §2: on, the sampler lives from the startup on.
        assert!(sampler_needed(true, []));
        assert!(sampler_needed(true, [false, false]));
        assert!(sampler_needed(true, [true]));
        // A window is measured with the manager open or with the key on.
        assert!(window_measured(true, false));
        assert!(window_measured(false, true));
        assert!(!window_measured(false, false));
    }

    // --- roots of a request -----------------------------------------------

    #[test]
    fn roots_hold_only_panes_with_a_group_of_windows_with_the_manager_open() {
        let roots = sample_roots([
            // Open: pane (1,0) local, pane (1,1) SSH (no group).
            (
                window(1),
                true,
                vec![((tab(1), pane(0)), Some("a")), ((tab(1), pane(1)), None)],
            ),
            // Closed window: nothing, whatever it holds.
            (window(2), false, vec![((tab(7), pane(0)), Some("b"))]),
            // Open, a tab with no runtime has no entry at all.
            (window(3), true, vec![((tab(2), pane(0)), Some("c"))]),
            // Open with no pane measurable.
            (window(4), true, vec![]),
        ]);
        assert_eq!(
            roots,
            vec![
                ((window(1), tab(1), pane(0)), "a"),
                ((window(3), tab(2), pane(0)), "c"),
            ]
        );
    }

    #[test]
    fn with_background_monitoring_the_roots_include_windows_with_the_manager_closed() {
        let windows = |background: bool| {
            [
                (
                    window(1),
                    window_measured(true, background),
                    vec![((tab(1), pane(0)), Some("a"))],
                ),
                (
                    window(2),
                    window_measured(false, background),
                    vec![((tab(7), pane(0)), Some("b"))],
                ),
            ]
        };
        assert_eq!(
            sample_roots(windows(false)),
            vec![((window(1), tab(1), pane(0)), "a")]
        );
        assert_eq!(
            sample_roots(windows(true)),
            vec![
                ((window(1), tab(1), pane(0)), "a"),
                ((window(2), tab(7), pane(0)), "b"),
            ]
        );
    }

    // --- the footer's toggle (ADR-0098 §4) ---------------------------------

    const EXAMPLE: &str = EXAMPLE_CONFIG_TOML;

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "porecatu-tm-background-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn the_toggle_edit_sets_the_key_to_the_opposite_value() {
        let key = KeyPath::parse("task_manager.background_monitoring").unwrap();
        assert_eq!(
            background_monitoring_edit(false),
            Edit::Set(key.clone(), EditValue::Bool(true))
        );
        assert_eq!(
            background_monitoring_edit(true),
            Edit::Set(key, EditValue::Bool(false))
        );
    }

    #[test]
    fn saving_writes_only_the_key_and_loads_back_with_it() {
        let original = "# meu arquivo\n[task_manager]\nhistory_seconds = 120   # meu\n";
        let path = scratch("only-key.toml");
        std::fs::write(&path, original).unwrap();
        save_background_monitoring(
            &path,
            Some(original),
            EXAMPLE,
            &background_monitoring_edit(false),
        )
        .unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            written.starts_with("# meu arquivo\n[task_manager]\nhistory_seconds = 120   # meu\n")
        );
        let (config, _) = porecatu_config::parse(&written).unwrap();
        assert!(config.task_manager.background_monitoring);
        assert_eq!(config.task_manager.history_seconds, 120);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_file_that_changed_since_the_read_is_rebased_once_and_keeps_both_changes() {
        // The base is stale: someone wrote another key in the meantime. The
        // edit by key lands on the new text, which keeps its own change.
        let stale = "[task_manager]\nhistory_seconds = 60\n";
        let now_on_disk = "[task_manager]\nhistory_seconds = 300\n";
        let path = scratch("rebased.toml");
        std::fs::write(&path, now_on_disk).unwrap();
        save_background_monitoring(
            &path,
            Some(stale),
            EXAMPLE,
            &background_monitoring_edit(false),
        )
        .unwrap();
        let (config, _) = porecatu_config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(config.task_manager.background_monitoring);
        assert_eq!(config.task_manager.history_seconds, 300);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_missing_file_is_born_from_the_example_with_the_key() {
        let path = scratch("born.toml");
        save_background_monitoring(&path, None, EXAMPLE, &background_monitoring_edit(false))
            .unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        let (config, _) = porecatu_config::parse(&written).unwrap();
        assert!(config.task_manager.background_monitoring);
        // The example's documentation came along.
        assert!(written.contains("# Continua medindo os processos"));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn an_invalid_file_on_disk_is_an_error_and_is_left_alone() {
        let broken = "[task_manager\n";
        let path = scratch("broken.toml");
        std::fs::write(&path, broken).unwrap();
        assert!(
            save_background_monitoring(
                &path,
                Some(broken),
                EXAMPLE,
                &background_monitoring_edit(false)
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
        std::fs::remove_file(&path).unwrap();
    }

    // --- partition and apply ----------------------------------------------

    #[test]
    fn a_closed_manager_gets_only_the_chart_point() {
        // ADR-0098 §3: the history grows, no view is made (the caller has
        // none to give), and an empty part is a zero point, not a gap.
        let base = Instant::now();
        let mut history = TaskManagerHistory::default();
        let mut part = HashMap::new();
        part.insert(
            (tab(1), pane(0)),
            vec![stat(1, Some(0.25), 100), stat(2, Some(0.5), 50)],
        );
        apply_to_history(&mut history, base, &part, 60);
        let point = history.latest().unwrap();
        assert_eq!((point.at, point.memory, point.cpu), (base, 150, Some(0.75)));

        let later = base + Duration::from_secs(1);
        apply_to_history(&mut history, later, &HashMap::new(), 60);
        assert_eq!(history.points().len(), 2);
        let point = history.latest().unwrap();
        assert_eq!((point.at, point.memory, point.cpu), (later, 0, None));
    }

    #[test]
    fn the_answer_is_split_by_window_and_drops_keys_that_are_not_good() {
        let panes = vec![
            ((window(1), tab(1), pane(0)), vec![stat(10, None, 1)]),
            ((window(1), tab(9), pane(0)), vec![stat(11, None, 1)]), // tab gone
            ((window(2), tab(1), pane(0)), vec![stat(20, None, 2)]),
            ((window(3), tab(1), pane(0)), vec![stat(30, None, 3)]), // closed / no manager
        ];
        let parts = partition_sample(panes, |&(w, t, _)| w != window(3) && t != tab(9));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[&window(1)].len(), 1);
        assert_eq!(parts[&window(1)][&(tab(1), pane(0))][0].pid, 10);
        assert_eq!(parts[&window(2)][&(tab(1), pane(0))][0].pid, 20);
        assert!(!parts.contains_key(&window(3)));
    }

    #[test]
    fn the_same_tab_id_in_two_windows_is_two_different_panes() {
        // Tab ids are per workspace (ADR-0015): the window decides.
        let panes = vec![
            ((window(1), tab(1), pane(0)), vec![stat(10, None, 1)]),
            ((window(2), tab(1), pane(0)), vec![stat(20, None, 2)]),
        ];
        let parts = partition_sample(panes, |_| true);
        assert_eq!(parts[&window(1)][&(tab(1), pane(0))][0].pid, 10);
        assert_eq!(parts[&window(2)][&(tab(1), pane(0))][0].pid, 20);
    }

    #[test]
    fn applying_a_part_sets_the_sample_and_adds_a_point_with_the_taken_at() {
        let base = Instant::now();
        let mut view = TaskManagerView::new(None);
        let mut history = TaskManagerHistory::default();
        let mut part = HashMap::new();
        part.insert(
            (tab(1), pane(0)),
            vec![stat(1, Some(0.25), 100), stat(2, Some(0.25), 50)],
        );
        part.insert((tab(2), pane(0)), vec![stat(3, None, 10)]);

        let at = base + Duration::from_secs(5);
        apply_to_window(&mut view, &mut history, at, part, 60);

        assert_eq!(view.sample.taken_at, Some(at));
        assert_eq!(view.sample.panes.len(), 2);
        assert_eq!(history.points().len(), 1);
        let point = history.latest().unwrap();
        assert_eq!(point.at, at);
        assert_eq!(point.memory, 160);
        assert_eq!(point.cpu, Some(0.5));
    }

    #[test]
    fn a_window_whose_part_is_empty_still_gets_a_zero_point() {
        let at = Instant::now();
        let mut view = TaskManagerView::new(None);
        let mut history = TaskManagerHistory::default();
        apply_to_window(&mut view, &mut history, at, HashMap::new(), 60);
        assert_eq!(view.sample.taken_at, Some(at));
        let point = history.latest().unwrap();
        assert_eq!((point.memory, point.cpu), (0, None));
        assert_eq!(point.disk_per_sec, 0.0);
    }

    #[test]
    fn a_second_part_replaces_the_sample_and_trims_the_history_by_the_period() {
        let base = Instant::now();
        let mut view = TaskManagerView::new(None);
        let mut history = TaskManagerHistory::default();
        for secs in [0_u64, 20, 40, 75] {
            let mut part = HashMap::new();
            part.insert((tab(1), pane(0)), vec![stat(1, Some(0.1), secs)]);
            apply_to_window(
                &mut view,
                &mut history,
                base + Duration::from_secs(secs),
                part,
                30,
            );
        }
        // 0 s and 20 s are 75 s and 55 s before the last one: older than
        // 30 s and gone. 40 s (35 s back) is the single point kept past the
        // period (the chart's left edge) and 75 s is the last.
        assert_eq!(history.points().len(), 2);
        assert_eq!(history.points().front().unwrap().memory, 40);
        assert_eq!(view.sample.panes[&(tab(1), pane(0))][0].memory, 75);
    }

    // --- pane facts -------------------------------------------------------

    #[test]
    fn the_fact_of_a_pane_follows_the_documented_order() {
        assert_eq!(pane_fact(true, false, Some(5)), PaneFact::Ssh);
        // SSH wins over exited: the note says why there is nothing to list.
        assert_eq!(pane_fact(true, true, None), PaneFact::Ssh);
        assert_eq!(pane_fact(false, true, Some(5)), PaneFact::Exited);
        assert_eq!(
            pane_fact(false, false, Some(5)),
            PaneFact::Local { root_pid: Some(5) }
        );
        assert_eq!(
            pane_fact(false, false, None),
            PaneFact::Local { root_pid: None }
        );
    }

    // --- selection and scroll after a new list ------------------------------

    fn workspace_of(n: usize) -> (Workspace, Vec<TabId>) {
        let mut ws = Workspace::new();
        let ids = (0..n)
            .map(|_| ws.append_ungrouped_tab("pwsh", None))
            .collect();
        (ws, ids)
    }

    fn rows_without_sample(ws: &Workspace) -> Vec<Row> {
        rows::build_rows(ws, &PaneFacts::new(), &WindowSample::default())
    }

    #[test]
    fn a_new_list_keeps_the_selection_by_key_and_the_scroll_the_user_set() {
        let (ws, ids) = workspace_of(10);
        let rows = rows_without_sample(&ws);
        // 10 tabs, one note each: 20 rows, 5 visible.
        let mut view = TaskManagerView::new(Some(RowKey::Tab(ids[1])));
        view.first_visible = 12; // scrolled away from the selection
        resolve_view(&mut view, &rows, 5, false);
        assert_eq!(view.selection, Some(RowKey::Tab(ids[1])));
        assert_eq!(view.first_visible, 12, "not pulled back every second");
    }

    #[test]
    fn opening_follows_the_selection_into_view() {
        let (ws, ids) = workspace_of(10);
        let rows = rows_without_sample(&ws);
        let mut view = TaskManagerView::new(Some(RowKey::Tab(ids[8])));
        resolve_view(&mut view, &rows, 5, true);
        // Tab 8's row is at index 16 of 20; the page ends on it.
        assert_eq!(view.first_visible, 16 + 1 - 5);
    }

    #[test]
    fn a_selection_that_vanished_falls_to_its_tab_then_to_the_first_row_in_view() {
        let (ws, ids) = workspace_of(3);
        let rows = rows_without_sample(&ws);
        // A process row of a tab that exists: falls to the tab row.
        let mut view = TaskManagerView::new(Some(RowKey::Process {
            tab: ids[2],
            pane: pane(0),
            pid: 99,
            start_time: 1,
        }));
        resolve_view(&mut view, &rows, 2, false);
        assert_eq!(view.selection, Some(RowKey::Tab(ids[2])));
        // The tab row is at index 4 of 6; the page of 2 ends on it.
        assert_eq!(
            view.first_visible,
            4 + 1 - 2,
            "moved on its own: kept in view"
        );

        // A selection of a tab that left: the first row.
        let mut view = TaskManagerView::new(Some(RowKey::Tab(tab(99))));
        view.first_visible = 3;
        resolve_view(&mut view, &rows, 2, false);
        assert_eq!(view.selection, Some(RowKey::Tab(ids[0])));
        assert_eq!(view.first_visible, 0);

        // No selection at all (cleared by a transfer): the first row too.
        let mut view = TaskManagerView::new(None);
        resolve_view(&mut view, &rows, 2, false);
        assert_eq!(view.selection, Some(RowKey::Tab(ids[0])));
    }

    #[test]
    fn a_list_that_shrank_never_leaves_the_view_past_its_end() {
        let (ws, ids) = workspace_of(2); // 4 rows
        let rows = rows_without_sample(&ws);
        let mut view = TaskManagerView::new(Some(RowKey::Tab(ids[0])));
        view.first_visible = 30;
        resolve_view(&mut view, &rows, 3, false);
        assert_eq!(view.first_visible, 1);
    }

    #[test]
    fn a_hover_on_a_row_that_died_is_dropped() {
        let (ws, ids) = workspace_of(1);
        let rows = rows_without_sample(&ws);
        let mut view = TaskManagerView::new(Some(RowKey::Tab(ids[0])));
        view.hover = Some(RowKey::Note(ids[0], pane(5)));
        resolve_view(&mut view, &rows, 5, false);
        assert_eq!(view.hover, None);
        view.hover = Some(RowKey::Tab(ids[0]));
        resolve_view(&mut view, &rows, 5, false);
        assert_eq!(view.hover, Some(RowKey::Tab(ids[0])));
    }

    // --- transfer (ADR-0095 §4, "Riscos") ---------------------------------

    /// Every key the view holds that names a tab.
    fn view_tabs(view: &TaskManagerView) -> Vec<TabId> {
        let mut tabs: Vec<TabId> = view.sample.panes.keys().map(|(tab, _)| *tab).collect();
        tabs.extend(view.selection.iter().map(RowKey::tab));
        tabs.extend(view.hover.iter().map(RowKey::tab));
        tabs.extend(view.menu.iter().map(|menu| menu.row.tab()));
        tabs
    }

    #[test]
    fn after_a_transfer_no_key_of_the_view_points_outside_the_workspace() {
        let (mut origin, ids) = workspace_of(3);
        let mut view = TaskManagerView::new(Some(RowKey::Tab(ids[1])));
        for &id in &ids {
            view.sample
                .panes
                .insert((id, pane(0)), vec![stat(id.get() + 1, None, 1)]);
        }
        view.hover = Some(RowKey::Note(ids[1], pane(0)));

        // The same two calls `App::transfer` makes: the core takes the tab out,
        // the window drops what points at the old id.
        let moved = [ids[1]];
        let taken = origin.take_tabs(&moved);
        assert_eq!(taken.len(), 1);
        view.remove_tabs(&moved);

        for tab in view_tabs(&view) {
            assert!(origin.tab(tab).is_some(), "{tab:?} is not in the workspace");
        }
        assert_eq!(view.sample.panes.len(), 2);

        // The selection fell away; the next resolution picks the first row.
        let rows = rows_without_sample(&origin);
        resolve_view(&mut view, &rows, 5, false);
        assert_eq!(view.selection, Some(RowKey::Tab(ids[0])));
    }

    // --- the notice -------------------------------------------------------

    #[test]
    fn the_adjusted_notice_is_given_once_per_loaded_value() {
        // In range: never.
        assert_eq!(history_notice_step(60, None), (false, None));
        // Out of range: due, and remembered.
        let (due, warned) = history_notice_step(5, None);
        assert!(due);
        assert_eq!(warned, Some(5));
        // The same value again: not repeated.
        assert_eq!(history_notice_step(5, warned), (false, Some(5)));
        // Another out-of-range value: a new fact.
        let (due, warned) = history_notice_step(9000, warned);
        assert!(due);
        assert_eq!(warned, Some(9000));
        // Back to a good value forgets; a bad one later is told again.
        let (due, warned) = history_notice_step(60, warned);
        assert!(!due);
        assert_eq!(warned, None);
        assert!(history_notice_step(5, warned).0);
    }

    // --- the catalog ------------------------------------------------------

    #[test]
    fn the_notice_text_carries_the_loaded_and_the_used_values() {
        for catalog in crate::messages::test_support::all_catalogs() {
            let body = msg::notice::task_manager_history_adjusted::body(&catalog, 5_u32, 30_u32);
            assert!(body.contains('5') && body.contains("30"), "{body}");
            assert!(!msg::notice::task_manager_history_adjusted::title(&catalog).is_empty());
        }
    }

    // --- the key chain link (ADR-0095 §3, "Riscos") ---------------------------

    const NO_MODS: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        super_: false,
    };

    fn named(key: NamedKey) -> Key {
        Key::Named(key)
    }

    fn text(s: &str) -> Key {
        Key::Character(s.into())
    }

    fn route(key: &Key, modifiers: Modifiers, action: Option<Action>) -> TaskKeyRoute {
        task_manager_key_route(true, key, modifiers, action)
    }

    #[test]
    fn a_printable_key_and_enter_never_reach_the_pane() {
        // Every printable character, shifted or not, with or without a
        // binding that targets a pane, is swallowed -- the leak ADR-0095
        // names as the highest risk.
        for c in ["a", "z", "0", " ", "/", "ç", "é"] {
            for shift in [false, true] {
                let mods = Modifiers { shift, ..NO_MODS };
                assert_eq!(route(&text(c), mods, None), TaskKeyRoute::Swallow, "{c}");
            }
        }
        // Ctrl+C (and any control chord that matched no action) is swallowed
        // too: it must not become `\x03` in the PTY.
        let ctrl = Modifiers {
            ctrl: true,
            ..NO_MODS
        };
        assert_eq!(route(&text("c"), ctrl, None), TaskKeyRoute::Swallow);
        // A bare `Enter` is "Go to tab", never a key for a dormant SSH pane
        // (which would connect on it) and never a carriage return.
        assert_eq!(
            route(&named(NamedKey::Enter), NO_MODS, None),
            TaskKeyRoute::GoToTab
        );
        // Even bound to a pane action by the user, a printable key is
        // swallowed rather than dispatched.
        assert_eq!(
            route(&text("v"), ctrl, Some(Action::ClipboardPaste)),
            TaskKeyRoute::Swallow
        );
        // Other named keys that match nothing: swallowed.
        for key in [
            NamedKey::Tab,
            NamedKey::Backspace,
            NamedKey::Delete,
            NamedKey::Space,
            NamedKey::F5,
            NamedKey::ArrowLeft,
            NamedKey::ArrowRight,
        ] {
            assert_eq!(
                route(&named(key), NO_MODS, None),
                TaskKeyRoute::Swallow,
                "{key:?}"
            );
        }
    }

    #[test]
    fn a_release_is_swallowed_whatever_the_key() {
        for key in [named(NamedKey::Enter), named(NamedKey::Escape), text("a")] {
            assert_eq!(
                task_manager_key_route(false, &key, NO_MODS, None),
                TaskKeyRoute::Swallow
            );
        }
        assert_eq!(
            task_manager_key_route(false, &named(NamedKey::ArrowDown), NO_MODS, None),
            TaskKeyRoute::Swallow
        );
    }

    #[test]
    fn the_navigation_keys_go_to_the_list() {
        let cases = [
            (NamedKey::ArrowUp, ListNav::Up),
            (NamedKey::ArrowDown, ListNav::Down),
            (NamedKey::Home, ListNav::Home),
            (NamedKey::End, ListNav::End),
            (NamedKey::PageUp, ListNav::PageUp),
            (NamedKey::PageDown, ListNav::PageDown),
        ];
        for (key, nav) in cases {
            assert_eq!(
                route(&named(key), NO_MODS, None),
                TaskKeyRoute::Navigate(nav),
                "{key:?}"
            );
        }
    }

    #[test]
    fn escape_leaves_and_a_modified_navigation_key_is_not_a_list_move() {
        assert_eq!(
            route(&named(NamedKey::Escape), NO_MODS, None),
            TaskKeyRoute::Leave
        );
        let ctrl = Modifiers {
            ctrl: true,
            ..NO_MODS
        };
        let shift = Modifiers {
            shift: true,
            ..NO_MODS
        };
        // `Ctrl+PageDown` is `tab.next` (a window action, dispatched);
        // `Shift+PageUp` is `scrollback.page_up` (a pane action, swallowed).
        assert_eq!(
            route(&named(NamedKey::PageDown), ctrl, Some(Action::TabNext)),
            TaskKeyRoute::Dispatch(Action::TabNext)
        );
        assert_eq!(
            route(
                &named(NamedKey::PageUp),
                shift,
                Some(Action::ScrollbackPageUp)
            ),
            TaskKeyRoute::Swallow
        );
        // Without a binding they are swallowed, not taken for navigation.
        assert_eq!(
            route(&named(NamedKey::PageDown), ctrl, None),
            TaskKeyRoute::Swallow
        );
        assert_eq!(
            route(&named(NamedKey::Escape), ctrl, None),
            TaskKeyRoute::Swallow
        );
        assert_eq!(
            route(&named(NamedKey::Enter), shift, None),
            TaskKeyRoute::Swallow
        );
    }

    #[test]
    fn the_menu_keys_open_the_row_menu() {
        assert_eq!(
            route(&named(NamedKey::ContextMenu), NO_MODS, None),
            TaskKeyRoute::OpenMenu
        );
        let shift = Modifiers {
            shift: true,
            ..NO_MODS
        };
        assert_eq!(
            route(&named(NamedKey::F10), shift, None),
            TaskKeyRoute::OpenMenu
        );
        // A bare F10 is not it.
        assert_eq!(
            route(&named(NamedKey::F10), NO_MODS, None),
            TaskKeyRoute::Swallow
        );
    }

    #[test]
    fn an_action_that_targets_a_pane_does_nothing_and_the_others_run() {
        let ctrl_shift = Modifiers {
            ctrl: true,
            shift: true,
            ..NO_MODS
        };
        for action in [
            Action::SearchOpen,
            Action::PaneSplitVertical,
            Action::PaneClose,
            Action::ScrollbackToTop,
            Action::ClipboardCopy,
            Action::ClipboardPaste,
            Action::SelectionSelectAll,
            Action::FontIncrease,
            Action::SshReconnect,
        ] {
            assert!(action.targets_pane(), "{action:?}");
            assert_eq!(
                route(&text("x"), ctrl_shift, Some(action)),
                TaskKeyRoute::Swallow,
                "{action:?}"
            );
        }
        for action in [
            Action::TabNew,
            Action::TabClose,
            Action::TabNext,
            Action::TabGoto(3),
            Action::GroupNext,
            Action::GroupNewTab,
            Action::WindowNew,
            Action::SessionOpenList,
            Action::SettingsOpen,
            Action::TaskManagerToggle,
        ] {
            assert!(!action.targets_pane(), "{action:?}");
            assert_eq!(
                route(&text("x"), ctrl_shift, Some(action)),
                TaskKeyRoute::Dispatch(action),
                "{action:?}"
            );
        }
    }

    // --- navigation -------------------------------------------------------

    #[test]
    fn navigation_moves_the_selection_and_keeps_it_in_view() {
        let (ws, ids) = workspace_of(5); // 10 rows: a tab row and a note each
        let rows = rows_without_sample(&ws);
        let mut view = TaskManagerView::new(Some(RowKey::Tab(ids[0])));

        apply_navigation(&mut view, &rows, ListNav::Down, 3);
        assert_eq!(view.selection, Some(RowKey::Note(ids[0], pane(0))));
        assert_eq!(view.first_visible, 0);

        apply_navigation(&mut view, &rows, ListNav::PageDown, 3);
        // From row 1, a page of 3 lands on row 4; the view moves to hold it.
        assert_eq!(view.selection, Some(rows[4].key));
        assert_eq!(view.first_visible, 4 + 1 - 3);

        apply_navigation(&mut view, &rows, ListNav::End, 3);
        assert_eq!(view.selection, Some(rows[9].key));
        assert_eq!(view.first_visible, 7);

        apply_navigation(&mut view, &rows, ListNav::Home, 3);
        assert_eq!(view.selection, Some(rows[0].key));
        assert_eq!(view.first_visible, 0);

        // No wrap-around at either end.
        apply_navigation(&mut view, &rows, ListNav::Up, 3);
        assert_eq!(view.selection, Some(rows[0].key));
    }

    #[test]
    fn navigation_from_a_selection_that_is_gone_starts_at_an_end() {
        let (ws, ids) = workspace_of(2);
        let rows = rows_without_sample(&ws);
        let mut view = TaskManagerView::new(Some(RowKey::Tab(tab(99))));
        apply_navigation(&mut view, &rows, ListNav::Down, 5);
        assert_eq!(view.selection, Some(RowKey::Tab(ids[0])));
        let mut view = TaskManagerView::new(None);
        apply_navigation(&mut view, &rows, ListNav::Up, 5);
        assert_eq!(view.selection, Some(rows[3].key));
        // An empty list changes nothing.
        let mut view = TaskManagerView::new(None);
        apply_navigation(&mut view, &[], ListNav::Down, 5);
        assert_eq!(view.selection, None);
    }

    /// ADR-0095 §10: a screen reader's `Focus` on a row selects it and brings
    /// it into view; the access route hands the key, `select_row` applies it.
    #[test]
    fn an_access_focus_on_a_row_changes_the_selection_and_scrolls_to_it() {
        let (ws, ids) = workspace_of(5);
        let rows = rows_without_sample(&ws);
        let mut view = TaskManagerView::new(Some(RowKey::Tab(ids[0])));

        let target = rows[8].key;
        let ids_of_rows = crate::access::task_row_ids(&rows);
        let route = task_manager_access_route(ids_of_rows[8], accesskit::Action::Focus, &rows);
        assert_eq!(route, Some(TaskAccessAction::SelectRow(target)));

        assert!(select_row(&mut view, &rows, target, 3));
        assert_eq!(view.selection, Some(target));
        assert_eq!(view.first_visible, 8 + 1 - 3);

        // A key that is not in the list changes nothing.
        let before = (view.selection, view.first_visible);
        assert!(!select_row(&mut view, &rows, RowKey::Tab(tab(99)), 3));
        assert_eq!((view.selection, view.first_visible), before);
    }

    // --- hit-test and wheel ----------------------------------------------------

    fn list_layout() -> TaskManagerLayout {
        let style = TaskManagerStyle::from_config(&Config::default(), &TabBarStyle::DEFAULT);
        tm_paint::layout(
            Rect {
                x: 0.0,
                y: 40.0,
                width: 1000.0,
                height: 600.0,
            },
            &style,
        )
    }

    fn middle_of_slot(list: &ListLayout, slot: usize) -> (f32, f32) {
        let rect = list.row_rect(slot);
        (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    #[test]
    fn a_point_on_a_row_gives_its_index_counting_the_scroll() {
        let layout = list_layout();
        let list = layout.list;
        assert!(list.visible_rows >= 3);
        for slot in 0..3 {
            let point = middle_of_slot(&list, slot);
            assert_eq!(row_index_at(&list, point, 0, 100), Some(slot));
            assert_eq!(row_index_at(&list, point, 17, 100), Some(17 + slot));
        }
    }

    #[test]
    fn a_point_outside_the_rows_gives_no_row() {
        let layout = list_layout();
        let list = layout.list;
        // The list header and the rule under it.
        let header = (
            list.header.x + 5.0,
            list.header.y + list.header.height / 2.0,
        );
        assert_eq!(row_index_at(&list, header, 0, 100), None);
        // The charts column (left of the list).
        let chart = layout.charts[1].inner;
        assert_eq!(
            row_index_at(&list, (chart.x + chart.width / 2.0, chart.y + 4.0), 0, 100),
            None
        );
        // Above the box and below the last visible row.
        assert_eq!(row_index_at(&list, (list.body.x + 4.0, 1.0), 0, 100), None);
        let below = list.body.y + list.body.height + 50.0;
        assert_eq!(
            row_index_at(&list, (list.body.x + 4.0, below), 0, 100),
            None
        );
    }

    #[test]
    fn the_empty_space_under_a_short_list_gives_no_row() {
        let layout = list_layout();
        let list = layout.list;
        // Two rows in a list that could show many: slot 2 is empty.
        assert_eq!(row_index_at(&list, middle_of_slot(&list, 1), 0, 2), Some(1));
        assert_eq!(row_index_at(&list, middle_of_slot(&list, 2), 0, 2), None);
        // With the scroll, the last row is the last index.
        assert_eq!(row_index_at(&list, middle_of_slot(&list, 0), 1, 2), Some(1));
        assert_eq!(row_index_at(&list, middle_of_slot(&list, 1), 1, 2), None);
    }

    #[test]
    fn the_wheel_scrolls_by_notches_and_clamps_at_both_ends() {
        // 50 rows, 10 visible: the scroll goes from 0 to 40.
        assert_eq!(wheel_scroll(0, -1.0, 50, 10), 3, "wheel down one notch");
        assert_eq!(wheel_scroll(10, 1.0, 50, 10), 7, "wheel up one notch");
        assert_eq!(
            wheel_scroll(1, 5.0, 50, 10),
            0,
            "never before the first row"
        );
        assert_eq!(wheel_scroll(0, 1.0, 50, 10), 0);
        assert_eq!(wheel_scroll(39, -1.0, 50, 10), 40, "never past the end");
        assert_eq!(wheel_scroll(40, -3.0, 50, 10), 40);
        // A list that fits does not scroll.
        assert_eq!(wheel_scroll(0, -4.0, 8, 10), 0);
        // A scroll left past the end by a list that shrank comes back in.
        assert_eq!(wheel_scroll(30, 0.0, 20, 10), 10);
        // A tiny high-resolution notch (touchpad) rounds to nothing.
        assert_eq!(wheel_scroll(5, -0.1, 50, 10), 5);
    }

    // --- the double click -------------------------------------------------

    #[test]
    fn a_second_click_on_the_same_row_in_time_is_a_double_click() {
        let threshold = Duration::from_millis(500);
        let now = Instant::now();
        let a = RowKey::Tab(tab(1));
        let b = RowKey::Tab(tab(2));
        let mut last = None;
        assert!(!register_row_click(&mut last, &a, now, threshold));
        assert!(register_row_click(
            &mut last,
            &a,
            now + Duration::from_millis(300),
            threshold
        ));
        // A double click resets: the third starts over.
        assert!(!register_row_click(
            &mut last,
            &a,
            now + Duration::from_millis(350),
            threshold
        ));
        // Another row, or too late, is a first click.
        let mut last = None;
        register_row_click(&mut last, &a, now, threshold);
        assert!(!register_row_click(
            &mut last,
            &b,
            now + Duration::from_millis(100),
            threshold
        ));
        let mut last = None;
        register_row_click(&mut last, &a, now, threshold);
        assert!(!register_row_click(
            &mut last,
            &a,
            now + Duration::from_millis(600),
            threshold
        ));
    }

    // --- go to tab ------------------------------------------------------------

    #[test]
    fn go_to_tab_activates_the_tab_and_a_missing_tab_or_pane_does_nothing() {
        let (mut ws, ids) = workspace_of(3);
        ws.activate_tab(ids[2]);

        assert!(go_to_tab(&mut ws, ids[0], None));
        assert_eq!(ws.active_tab(), Some(ids[0]));

        // A pane that is not in the tab: nothing changes, nothing is activated.
        assert!(!go_to_tab(&mut ws, ids[1], Some(pane(77))));
        assert_eq!(ws.active_tab(), Some(ids[0]));
        // A tab that is gone.
        assert!(!go_to_tab(&mut ws, tab(99), None));
        assert_eq!(ws.active_tab(), Some(ids[0]));

        // The pane of the row is focused.
        let only = ws.tab(ids[1]).unwrap().panes().focused_id();
        assert!(go_to_tab(&mut ws, ids[1], Some(only)));
        assert_eq!(ws.active_tab(), Some(ids[1]));
        assert_eq!(ws.tab(ids[1]).unwrap().panes().focused_id(), only);
    }

    #[test]
    fn go_to_tab_focuses_the_pane_of_the_row_in_a_split_tab() {
        let (mut ws, ids) = workspace_of(2);
        let first = ws.tab(ids[0]).unwrap().panes().focused_id();
        let second = ws
            .tab_mut(ids[0])
            .unwrap()
            .panes_mut()
            .split(first, porecatu_core::SplitAxis::Vertical, "pwsh", None)
            .expect("split");
        ws.tab_mut(ids[0]).unwrap().panes_mut().focus(second);
        ws.activate_tab(ids[1]);

        assert!(go_to_tab(&mut ws, ids[0], Some(first)));
        assert_eq!(ws.active_tab(), Some(ids[0]));
        assert_eq!(ws.tab(ids[0]).unwrap().panes().focused_id(), first);
    }

    #[test]
    fn go_to_tab_expands_a_collapsed_group_by_the_same_call_as_the_bar() {
        let (mut ws, ids) = workspace_of(3);
        let color = ws.next_auto_color();
        let group = ws.group_tabs(&[ids[0], ids[1]], "g", color).expect("group");
        ws.activate_tab(ids[2]);
        assert!(ws.collapse_group(group, true));
        assert!(ws.group(group).unwrap().is_collapsed());

        assert!(go_to_tab(&mut ws, ids[0], None));
        assert_eq!(ws.active_tab(), Some(ids[0]));
        assert!(
            !ws.group(group).unwrap().is_collapsed(),
            "ADR-0020's ladder"
        );
    }

    // --- the return to the terminal ---------------------------------------------

    fn snap(active: u32, count: usize) -> ActivationSnapshot {
        (Some(tab(active)), count)
    }

    #[test]
    fn activating_another_tab_leaves() {
        // tab.next / prev / goto / group.next / a click on another tab.
        assert!(should_leave(snap(1, 3), snap(2, 3), false));
        assert!(should_leave(snap(3, 3), snap(1, 3), false));
    }

    #[test]
    fn creating_a_tab_leaves() {
        // tab.new, group.new_tab, "+", the profile menu, a restore appended in
        // the current window, a tab that arrived by transfer: the count grows
        // and the new tab is the active one.
        assert!(should_leave(snap(1, 3), snap(4, 4), false));
        // Grown with the same active tab (an arrival that was not activated,
        // or a tab appended in the background) also leaves: "created".
        assert!(should_leave(snap(1, 3), snap(1, 4), false));
        // From a window with no active tab.
        assert!(should_leave((None, 0), snap(1, 1), false));
    }

    #[test]
    fn closing_the_active_tab_does_not_leave() {
        // The next tab becomes the active one: the id changed, the count fell.
        let (before, after) = (snap(2, 3), snap(3, 2));
        assert!(is_closing(before, after));
        assert!(!should_leave(before, after, is_closing(before, after)));
        // Closing the last one: no active tab at all.
        let (before, after) = (snap(1, 1), (None, 0));
        assert!(is_closing(before, after));
        assert!(!should_leave(before, after, true));
    }

    #[test]
    fn closing_another_tab_does_not_leave() {
        let (before, after) = (snap(2, 3), snap(2, 2));
        assert!(is_closing(before, after));
        assert!(!should_leave(before, after, true));
        // Not closing and nothing else changed: also stays.
        assert!(!should_leave(snap(2, 3), snap(2, 3), false));
    }

    #[test]
    fn nothing_changed_does_not_leave() {
        assert!(!should_leave(snap(1, 3), snap(1, 3), false));
        assert!(!should_leave((None, 0), (None, 0), false));
        assert!(!is_closing(snap(1, 3), snap(1, 3)));
        // A tab created while another closed in the same dispatch is not a
        // closing by the count (net zero) and, the active one having changed,
        // it is an activation.
        assert!(!is_closing(snap(1, 3), snap(4, 3)));
        assert!(should_leave(snap(1, 3), snap(4, 3), false));
    }

    #[test]
    fn the_snapshot_counts_every_tab_even_in_a_collapsed_group() {
        let (mut ws, ids) = workspace_of(3);
        let color = ws.next_auto_color();
        let group = ws.group_tabs(&[ids[0], ids[1]], "g", color).expect("group");
        let count = |ws: &Workspace| ws.visual_order().count();
        assert_eq!(count(&ws), 3);
        // Collapsing hides tabs from the navigable order but they are still
        // tabs: the count the check watches does not move.
        ws.activate_tab(ids[2]);
        assert!(ws.collapse_group(group, true));
        assert_eq!(count(&ws), 3);
    }

    // --- row menu, kill route (step 07b) ----------------------------------

    fn test_list() -> ListLayout {
        let box_rect = Rect {
            x: 10.0,
            y: 40.0,
            width: 1000.0,
            height: 600.0,
        };
        let style = TaskManagerStyle::from_config(&Config::default(), &TabBarStyle::DEFAULT);
        tm_paint::layout(box_rect, &style).list
    }

    #[test]
    fn a_keyboard_menu_hangs_under_the_left_edge_of_the_selected_row() {
        let list = test_list();
        assert!(list.visible_rows > 3);
        // Index 3 with the list scrolled by 1 is the third visible slot.
        let row = list.row_rect(2);
        assert_eq!(
            keyboard_menu_anchor(&list, 1, 3),
            Some((row.x, row.y + row.height))
        );
        // The first row of an unscrolled list.
        let first = list.row_rect(0);
        assert_eq!(
            keyboard_menu_anchor(&list, 0, 0),
            Some((first.x, first.y + first.height))
        );
    }

    #[test]
    fn a_selected_row_out_of_view_has_no_anchor_until_it_is_scrolled_in() {
        let list = test_list();
        // Above the first visible row, and past the last one.
        assert_eq!(keyboard_menu_anchor(&list, 5, 4), None);
        assert_eq!(keyboard_menu_anchor(&list, 0, list.visible_rows), None);
        // The scroll the caller applies makes it visible.
        let first = keep_visible(0, list.visible_rows + 3, list.visible_rows);
        assert!(keyboard_menu_anchor(&list, first, list.visible_rows + 3).is_some());
    }

    #[test]
    fn the_kill_goes_to_the_sampler_unless_there_is_none_or_it_is_gone() {
        assert_eq!(kill_route(Some(true)), KillRoute::Sampler);
        // No sampler: the single-use thread.
        assert_eq!(kill_route(None), KillRoute::Detached);
        // A sampler whose thread had already gone: the same fallback.
        assert_eq!(kill_route(Some(false)), KillRoute::Detached);
    }

    #[test]
    fn a_menu_whose_row_died_goes_away_with_the_next_list() {
        let (ws, ids) = workspace_of(2);
        let rows = rows_without_sample(&ws);
        let items = task_menu_items(&rows[0]);
        let mut view = TaskManagerView::new(Some(RowKey::Tab(ids[0])));
        view.menu = Some(TaskMenu::new(RowKey::Tab(ids[0]), (1.0, 2.0), &items));
        resolve_view(&mut view, &rows, 5, false);
        assert!(view.menu.is_some(), "its row is still there");

        // The process row the menu was opened on is not in the new list.
        let dead = RowKey::Process {
            tab: ids[0],
            pane: pane(0),
            pid: 99,
            start_time: 1,
        };
        view.menu = Some(TaskMenu::new(dead, (1.0, 2.0), &items));
        resolve_view(&mut view, &rows, 5, false);
        assert!(view.menu.is_none());
    }
}
