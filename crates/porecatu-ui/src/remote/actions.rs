// SPDX-License-Identifier: GPL-3.0-or-later

//! The decisions behind the remote actions on tabs, panes, groups and windows
//! (PRD-025 RF-25.23 to RF-25.28; ADR-0107 sections 1 to 5; spec 03 section
//! 7).
//!
//! Pure: no window, no runtime. What needs them -- resolving a protocol id to
//! the live objects, calling the same function the local gesture calls --
//! stays in `app_remote.rs`; this module holds what can be decided from plain
//! data and so is tested without a GPU: when the desktop is busy, which ids
//! and profiles are valid, the last-tab refusal, what a close asks, the
//! pending confirmations and their 60 s deadline, and, for the groups and the
//! moves between windows, the plan of each action (ids, `busy`, rules).
//!
//! A remote action does what the local gesture does, except that it never
//! activates a tab (ADR-0111, which restores ADR-0107 section 2): only the
//! removal of the active tab moves it, by the focus ladder. What no action does
//! is touch the window of the OS: no focus, no raise, no restore.

use std::collections::BTreeMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

use porecatu_core::{GroupColor, GroupId, TabId, Workspace};
use porecatu_remote::ClientId;
use porecatu_remote::protocol::message::{ConfirmKind, ErrorCode, NewTabGroup, PaneState, Profile};

use super::ids::ProtocolIds;
use crate::pinned::CloseAsk;

/// How long a `confirm-request` waits for the answer (spec 03 section 7,
/// ADR-0107 section 3).
pub(crate) const CONFIRM_TIMEOUT: Duration = Duration::from_secs(60);

// --- when the desktop is busy -------------------------------------------

/// Which capture modes a window is in (ADR-0107 section 4): everything that
/// makes the user's gesture own the window, so the ground must not move under
/// it. Plain flags: `WindowState::capture_modes` fills them from its fields,
/// [`CaptureModes::any`] is what `WindowState::is_capturing` answers, the one
/// predicate every remote action asks (with `App::cross_drag`, the drag
/// between windows, which belongs to no window).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CaptureModes {
    /// A modal dialog is open.
    pub dialog: bool,
    /// The masked field of the secret dialog is alive.
    pub secret_field: bool,
    /// The context menu of a tab.
    pub context_menu: bool,
    /// The context menu of a group.
    pub group_context_menu: bool,
    /// The profile menu hanging from a `+`.
    pub profile_menu: bool,
    /// The context menu of the terminal.
    pub terminal_context_menu: bool,
    /// The group editor.
    pub group_editor: bool,
    /// The "move to group" popover.
    pub move_to_group: bool,
    /// The "Move to window..." popover.
    pub move_to_window: bool,
    /// The named-sessions popover.
    pub session_picker: bool,
    /// The popover of the connected-devices button (ADR-0108 section 4).
    pub devices_popover: bool,
    /// The row menu of the task manager.
    pub task_menu: bool,
    /// An inline rename of a tab is being typed.
    pub rename: bool,
    /// A drag of this window is armed or going on (a tab press counts).
    pub drag: bool,
    /// A drag from another window has the cursor over this one.
    pub incoming_drag: bool,
}

impl CaptureModes {
    /// `true` when the window is in any capture mode. The destructuring is
    /// exhaustive on purpose: a mode added to the struct does not compile
    /// until it is counted here.
    pub(crate) const fn any(self) -> bool {
        let Self {
            dialog,
            secret_field,
            context_menu,
            group_context_menu,
            profile_menu,
            terminal_context_menu,
            group_editor,
            move_to_group,
            move_to_window,
            session_picker,
            devices_popover,
            task_menu,
            rename,
            drag,
            incoming_drag,
        } = self;
        dialog
            || secret_field
            || context_menu
            || group_context_menu
            || profile_menu
            || terminal_context_menu
            || group_editor
            || move_to_group
            || move_to_window
            || session_picker
            || devices_popover
            || task_menu
            || rename
            || drag
            || incoming_drag
    }
}

// --- new-tab ------------------------------------------------------------

/// Where a valid `new-tab` puts the tab (spec 03 section 7, `group`): where
/// `tab.new` would (absent), outside any group at the end of the track
/// (`null`) or at the end of a group of the window (an id, already resolved
/// to the internal one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewTabPlace {
    ActiveGroup,
    Ungrouped,
    Group(GroupId),
}

/// What a valid `new-tab` is: the window, the place and the profile (`None` =
/// the default; `Some("")` is the `[shell]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewTabPlan<W> {
    pub window: W,
    pub place: NewTabPlace,
    pub profile: Option<String>,
}

/// Whether `id` is a profile the phone was offered (the `profiles` message):
/// the `[shell]` is `""` there, so `Some("")` is known, not "unknown".
pub(crate) fn profile_known(offered: &[Profile], id: &str) -> bool {
    offered.iter().any(|profile| profile.id == id)
}

/// Validates a `new-tab` (spec 03 section 7): the window must exist
/// (`unknown-window`), the group must exist **in that window**
/// (`unknown-group`; a `null` group names no group, so it is never unknown)
/// and the profile must be one the phone was offered (`unknown-profile`),
/// checked in that order.
pub(crate) fn plan_new_tab<W: Copy + Eq + Hash>(
    ids: &ProtocolIds<W>,
    window: u64,
    group: NewTabGroup,
    profile: Option<&str>,
    offered: &[Profile],
    window_exists: impl Fn(W) -> bool,
    group_exists: impl Fn(W, GroupId) -> bool,
) -> Result<NewTabPlan<W>, ErrorCode> {
    let window = ids
        .window_of(window)
        .filter(|w| window_exists(*w))
        .ok_or(ErrorCode::UnknownWindow)?;
    let place = match group {
        NewTabGroup::Auto => NewTabPlace::ActiveGroup,
        NewTabGroup::Ungrouped => NewTabPlace::Ungrouped,
        NewTabGroup::Group(id) => ids
            .group_of(id)
            .filter(|(owner, group)| *owner == window && group_exists(window, *group))
            .map(|(_, group)| NewTabPlace::Group(group))
            .ok_or(ErrorCode::UnknownGroup)?,
    };
    if let Some(profile) = profile
        && !profile_known(offered, profile)
    {
        return Err(ErrorCode::UnknownProfile);
    }
    Ok(NewTabPlan {
        window,
        place,
        profile: profile.map(str::to_owned),
    })
}

// --- closing ------------------------------------------------------------

/// Closing the tab would close the last tab of the last window, ending the
/// host and with it the access (ADR-0107 section 5). `tabs_in_window` counts
/// the pinned tabs (RF-21.25: they are tabs, and closing the last common one
/// of a window that has them does not close the window).
pub(crate) const fn closes_last_tab(windows: usize, tabs_in_window: usize) -> bool {
    windows <= 1 && tabs_in_window <= 1
}

/// What a remote close does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseStep {
    /// Answer `action-result` with this error and do nothing.
    Refuse(ErrorCode),
    /// Send a `confirm-request` of this kind to the phone that asked.
    Ask(ConfirmKind),
    /// Close it.
    Close,
}

/// The kind a `confirm-request` carries: an open connection wins over a
/// foreground program.
pub(crate) const fn confirm_kind(open_connection: bool) -> ConfirmKind {
    if open_connection {
        ConfirmKind::SshConnection
    } else {
        ConfirmKind::RunningProcess
    }
}

/// `close-tab` (and the tab end of a `close-pane` on the last pane): the
/// last-tab refusal first, then the local rule -- the dialog the desktop would
/// open (`ask`, the one decision of `tab.close`) becomes a `confirm-request`.
/// `confirmed` is the answer already given: it skips the question and nothing
/// else (the refusal still applies, the world may have changed meanwhile).
pub(crate) fn close_tab_step(
    windows: usize,
    tabs_in_window: usize,
    ask: CloseAsk,
    open_connection: bool,
    confirmed: bool,
) -> CloseStep {
    if closes_last_tab(windows, tabs_in_window) {
        return CloseStep::Refuse(ErrorCode::LastTab);
    }
    match ask {
        CloseAsk::Direct => CloseStep::Close,
        CloseAsk::ActiveProcess | CloseAsk::Pinned { .. } if confirmed => CloseStep::Close,
        CloseAsk::ActiveProcess | CloseAsk::Pinned { .. } => {
            CloseStep::Ask(confirm_kind(open_connection))
        }
    }
}

/// What a `close-pane` is, before it is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneStep {
    /// The last pane of a tab: it is `close-tab`, whole -- a pinned tab
    /// included. The spec (03 section 7) says a pinned tab keeps its pane
    /// "restarted by the desktop rule", but the desktop has no such rule:
    /// closing the last pane of a pinned tab is closing the tab, which always
    /// asks and unpins (RF-21.22). The owner chose the desktop rule
    /// (2026-11-17); the spec text is to be fixed in porecatu-protocol.
    CloseTab,
    /// Not the last pane, and the local rule asks first.
    Ask(ConfirmKind),
    /// Not the last pane, nothing to ask: close it.
    Close,
}

/// `close-pane`: `needs_confirmation` is RF-1.6 over that pane
/// (`pane_close_needs_confirmation`), `confirmed` the answer already given.
pub(crate) fn pane_step(
    is_last_pane: bool,
    needs_confirmation: bool,
    open_connection: bool,
    confirmed: bool,
) -> PaneStep {
    if is_last_pane {
        PaneStep::CloseTab
    } else if needs_confirmation && !confirmed {
        PaneStep::Ask(confirm_kind(open_connection))
    } else {
        PaneStep::Close
    }
}

/// One pane, as the `confirm-request` lists it: the title it shows, whether
/// something is alive in it (a program in the foreground or an open
/// connection, whatever `confirm_close_with_process` says) and whether that
/// something is an open SSH connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LivePane {
    pub title: String,
    pub alive: bool,
    pub open_connection: bool,
}

/// The kind and the names of a `confirm-request`: the titles of the panes
/// that have something alive (the local dialog names no process; a title is
/// what the tree already tells the phone), `SshConnection` when any of them
/// is an open connection. A pinned tab with nothing alive asks with an empty
/// list: it always asks (RF-21.23).
pub(crate) fn confirm_summary(
    panes: impl IntoIterator<Item = LivePane>,
) -> (ConfirmKind, Vec<String>) {
    let mut open_connection = false;
    let mut names = Vec::new();
    for pane in panes.into_iter().filter(|pane| pane.alive) {
        open_connection |= pane.open_connection;
        names.push(pane.title);
    }
    (confirm_kind(open_connection), names)
}

// --- start-pane ---------------------------------------------------------

/// `start-pane` starts a `not-started` pane (a tab that was never started, or
/// a dormant SSH pane). Any other state is `invalid-argument`: the spec does
/// not say, and answering success for a pane that was not started (or is
/// `exited`) would be a lie the phone cannot tell from the tree.
pub(crate) fn start_step(state: PaneState) -> Result<(), ErrorCode> {
    if state == PaneState::NotStarted {
        Ok(())
    } else {
        Err(ErrorCode::InvalidArgument)
    }
}

// --- groups and windows -------------------------------------------------
//
// Stage 15 (ADR-0107 sections 1, 2 and 4; spec 03 section 7). Every plan
// below checks in the same order: the ids (`unknown-*`, in the order the
// fields appear in the message), then `busy` for **every window involved**,
// then the rules of the action (`invalid-argument`). The plans resolve; the
// execution is in `app_remote.rs`, through the function the local gesture
// calls.

/// The color of a `new-group`/`set-group-color`: an index into the six
/// colors of the palette (`theme.group_colors`), anything else is
/// `invalid-argument`.
pub(crate) fn parse_group_color(index: u64) -> Result<GroupColor, ErrorCode> {
    usize::try_from(index)
        .ok()
        .and_then(|index| GroupColor::ALL.get(index).copied())
        .ok_or(ErrorCode::InvalidArgument)
}

/// Where a `move-tab-to-group` puts the tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GroupMove {
    /// The end of that group (`move_tab_to_group`, as the popover).
    Into(GroupId),
    /// Out of its group (`group: null`).
    Leave,
}

/// What a valid `move-tab-to-group` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MoveToGroupPlan<W> {
    pub window: W,
    pub tab: TabId,
    pub target: GroupMove,
}

/// Validates a `move-tab-to-group`. `group_exists` is true for an **explicit**
/// group of that window (the implicit runs have no protocol id). A pinned tab
/// is `invalid-argument` (RF-21.12: it belongs to no group, the menu item is
/// dimmed), and so is a group of another window -- the spec is silent, moving
/// between windows is `move-*-to-window` and not a side effect of this.
pub(crate) fn plan_move_tab_to_group<W: Copy + Eq + Hash>(
    ids: &ProtocolIds<W>,
    tab: u64,
    group: Option<u64>,
    tab_exists: impl Fn(W, TabId) -> bool,
    group_exists: impl Fn(W, GroupId) -> bool,
    is_pinned: impl Fn(W, TabId) -> bool,
    is_busy: impl Fn(W) -> bool,
) -> Result<MoveToGroupPlan<W>, ErrorCode> {
    let (window, tab) = ids
        .tab_of(tab)
        .filter(|(window, tab)| tab_exists(*window, *tab))
        .ok_or(ErrorCode::UnknownTab)?;
    // A group of another window does not exist for this tab: `unknown-group`,
    // the same answer `new-tab` gives (owner's decision, 2026-11-17).
    let group = group
        .map(|id| {
            ids.group_of(id)
                .filter(|(owner, group)| *owner == window && group_exists(*owner, *group))
                .ok_or(ErrorCode::UnknownGroup)
        })
        .transpose()?;
    if is_busy(window) {
        return Err(ErrorCode::Busy);
    }
    if is_pinned(window, tab) {
        return Err(ErrorCode::InvalidArgument);
    }
    Ok(MoveToGroupPlan {
        window,
        tab,
        target: group.map_or(GroupMove::Leave, |(_, group)| GroupMove::Into(group)),
    })
}

/// Where a tab that leaves its group lands: the slot of the list of groups
/// right after the group it left, as a loose tab (the position of a drop
/// between groups, `move_tab_to_new_run`). `None` when the tab is in no
/// explicit group -- it is already out, and the request is a success that
/// changes nothing. The desktop has no "leave group" item; the drag out of
/// the group is the gesture, and it lands wherever the pointer is: the slot
/// after the group is the neutral one.
pub(crate) fn leave_group_slot(workspace: &Workspace, tab: TabId) -> Option<usize> {
    let group = workspace
        .group_of_tab(tab)
        .and_then(|id| workspace.group(id))
        .filter(|group| group.is_explicit())?;
    workspace
        .groups()
        .iter()
        .position(|candidate| candidate.id() == group.id())
        .map(|index| index + 1)
}

/// What a valid `new-group` is: the window, the tabs (deduplicated, in the
/// order given) and the color (`None` = the next automatic one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewGroupPlan<W> {
    pub window: W,
    pub tabs: Vec<TabId>,
    pub color: Option<GroupColor>,
}

/// Validates a `new-group`: an empty list is `invalid-argument`; an unknown
/// tab, `unknown-tab`; then `busy`; then tabs of different windows, any
/// pinned tab or a color outside the palette are `invalid-argument` (spec 03
/// section 7).
pub(crate) fn plan_new_group<W: Copy + Eq + Hash>(
    ids: &ProtocolIds<W>,
    tabs: &[u64],
    color: Option<u64>,
    tab_exists: impl Fn(W, TabId) -> bool,
    is_pinned: impl Fn(W, TabId) -> bool,
    is_busy: impl Fn(W) -> bool,
) -> Result<NewGroupPlan<W>, ErrorCode> {
    if tabs.is_empty() {
        return Err(ErrorCode::InvalidArgument);
    }
    let mut resolved: Vec<(W, TabId)> = Vec::with_capacity(tabs.len());
    for id in tabs {
        let found = ids
            .tab_of(*id)
            .filter(|(window, tab)| tab_exists(*window, *tab))
            .ok_or(ErrorCode::UnknownTab)?;
        if !resolved.contains(&found) {
            resolved.push(found);
        }
    }
    if resolved.iter().any(|(window, _)| is_busy(*window)) {
        return Err(ErrorCode::Busy);
    }
    let window = resolved[0].0;
    if resolved.iter().any(|(owner, _)| *owner != window)
        || resolved.iter().any(|(owner, tab)| is_pinned(*owner, *tab))
    {
        return Err(ErrorCode::InvalidArgument);
    }
    let color = color.map(parse_group_color).transpose()?;
    Ok(NewGroupPlan {
        window,
        tabs: resolved.into_iter().map(|(_, tab)| tab).collect(),
        color,
    })
}

/// Validates the target of `rename-group`, `set-group-color`,
/// `collapse-group` and `ungroup`: an explicit group that still exists
/// (`unknown-group`) in a window that is not busy.
pub(crate) fn plan_group_edit<W: Copy + Eq + Hash>(
    ids: &ProtocolIds<W>,
    group: u64,
    group_exists: impl Fn(W, GroupId) -> bool,
    is_busy: impl Fn(W) -> bool,
) -> Result<(W, GroupId), ErrorCode> {
    let (window, group) = ids
        .group_of(group)
        .filter(|(window, group)| group_exists(*window, *group))
        .ok_or(ErrorCode::UnknownGroup)?;
    if is_busy(window) {
        return Err(ErrorCode::Busy);
    }
    Ok((window, group))
}

// `collapse-group` has no plan of its own beyond `plan_group_edit`: it calls the
// model, and the active tab follows ADR-0111 section 1 -- collapsing the group
// of the active tab moves it by the focus ladder (RF-2.14), expanding does not
// change it: it calls `Workspace::expand_group_in_place`, which activates
// nothing (unlike `collapse_group`, which activates the first tab).

/// What a valid `move-tab-to-window` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabToWindowPlan<W> {
    pub from: W,
    pub tab: TabId,
    pub to: W,
}

/// Validates a `move-tab-to-window`: `unknown-tab`, `unknown-window`, `busy`
/// in either window, then the same window is `invalid-argument` (the spec is
/// silent; nothing moves, so it is not a success).
pub(crate) fn plan_tab_to_window<W: Copy + Eq + Hash>(
    ids: &ProtocolIds<W>,
    tab: u64,
    window: u64,
    tab_exists: impl Fn(W, TabId) -> bool,
    window_exists: impl Fn(W) -> bool,
    is_busy: impl Fn(W) -> bool,
) -> Result<TabToWindowPlan<W>, ErrorCode> {
    let (from, tab) = ids
        .tab_of(tab)
        .filter(|(window, tab)| tab_exists(*window, *tab))
        .ok_or(ErrorCode::UnknownTab)?;
    let to = ids
        .window_of(window)
        .filter(|window| window_exists(*window))
        .ok_or(ErrorCode::UnknownWindow)?;
    if is_busy(from) || is_busy(to) {
        return Err(ErrorCode::Busy);
    }
    if from == to {
        return Err(ErrorCode::InvalidArgument);
    }
    Ok(TabToWindowPlan { from, tab, to })
}

/// What a valid `move-group-to-window` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GroupToWindowPlan<W> {
    pub from: W,
    pub group: GroupId,
    pub to: W,
}

/// Validates a `move-group-to-window`, like [`plan_tab_to_window`]
/// (`group_exists` is true for an explicit group).
pub(crate) fn plan_group_to_window<W: Copy + Eq + Hash>(
    ids: &ProtocolIds<W>,
    group: u64,
    window: u64,
    group_exists: impl Fn(W, GroupId) -> bool,
    window_exists: impl Fn(W) -> bool,
    is_busy: impl Fn(W) -> bool,
) -> Result<GroupToWindowPlan<W>, ErrorCode> {
    let (from, group) = ids
        .group_of(group)
        .filter(|(window, group)| group_exists(*window, *group))
        .ok_or(ErrorCode::UnknownGroup)?;
    let to = ids
        .window_of(window)
        .filter(|window| window_exists(*window))
        .ok_or(ErrorCode::UnknownWindow)?;
    if is_busy(from) || is_busy(to) {
        return Err(ErrorCode::Busy);
    }
    if from == to {
        return Err(ErrorCode::InvalidArgument);
    }
    Ok(GroupToWindowPlan { from, group, to })
}

// --- pending confirmations ----------------------------------------------

/// What a pending confirmation is about, by the **protocol id** (ADR-0107
/// section 3): stable across windows, so a tab that moved while the phone
/// thought still resolves; one that is gone is `unknown-tab`/`unknown-pane`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfirmTarget {
    Tab(u64),
    Pane(u64),
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    target: ConfirmTarget,
    deadline: Instant,
}

/// The `confirm-request`s waiting for an answer, keyed by `(client, id of
/// the action)`. Process state, never per window. Time comes from outside.
#[derive(Debug, Default)]
pub(crate) struct Confirmations {
    pending: BTreeMap<(ClientId, u64), Pending>,
}

impl Confirmations {
    /// Registers a request sent at `now`. `false` when the client already has
    /// a pending request with that id (ids are unique among pending
    /// requests): nothing is registered.
    pub(crate) fn insert(
        &mut self,
        client: ClientId,
        id: u64,
        target: ConfirmTarget,
        now: Instant,
    ) -> bool {
        if self.pending.contains_key(&(client, id)) {
            return false;
        }
        self.pending.insert(
            (client, id),
            Pending {
                target,
                deadline: now + CONFIRM_TIMEOUT,
            },
        );
        true
    }

    /// The answer of the client: takes the request out and says what it was
    /// about. `None` for one that is unknown, already answered or expired.
    pub(crate) fn answer(&mut self, client: ClientId, id: u64) -> Option<ConfirmTarget> {
        self.pending.remove(&(client, id)).map(|p| p.target)
    }

    /// Takes out and returns the requests whose deadline has come: each is
    /// answered `confirm-timeout`.
    pub(crate) fn expire(&mut self, now: Instant) -> Vec<(ClientId, u64)> {
        let due: Vec<(ClientId, u64)> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.deadline <= now)
            .map(|(key, _)| *key)
            .collect();
        for key in &due {
            self.pending.remove(key);
        }
        due
    }

    /// The client is gone: its requests are dropped, with no answer (the
    /// action is cancelled, nobody is left to tell).
    pub(crate) fn drop_client(&mut self, client: ClientId) {
        self.pending.retain(|(owner, _), _| *owner != client);
    }

    /// Nothing survives the host.
    pub(crate) fn clear(&mut self) {
        self.pending.clear();
    }

    /// The earliest deadline: the `WaitUntil` of the event loop.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.pending.values().map(|pending| pending.deadline).min()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use porecatu_core::{PaneId, TabId};
    use porecatu_remote::protocol::message::ProfileKind;

    use super::*;

    fn profile(id: &str) -> Profile {
        Profile {
            id: id.to_owned(),
            name: id.to_owned(),
            kind: ProfileKind::Local,
        }
    }

    // --- busy ---------------------------------------------------------

    /// One mode at a time: each flag alone makes the window busy, and the
    /// list below is the list of the struct (the destructuring in `any`
    /// makes a forgotten one a compile error; this makes a mode that does not
    /// count a test failure).
    #[test]
    fn every_capture_mode_makes_the_window_busy() {
        assert!(!CaptureModes::default().any());
        type Set = fn(&mut CaptureModes);
        let modes: [(&str, Set); 15] = [
            ("dialog", |m| m.dialog = true),
            ("secret_field", |m| m.secret_field = true),
            ("context_menu", |m| m.context_menu = true),
            ("group_context_menu", |m| m.group_context_menu = true),
            ("profile_menu", |m| m.profile_menu = true),
            ("terminal_context_menu", |m| m.terminal_context_menu = true),
            ("group_editor", |m| m.group_editor = true),
            ("move_to_group", |m| m.move_to_group = true),
            ("move_to_window", |m| m.move_to_window = true),
            ("session_picker", |m| m.session_picker = true),
            ("devices_popover", |m| m.devices_popover = true),
            ("task_menu", |m| m.task_menu = true),
            ("rename", |m| m.rename = true),
            ("drag", |m| m.drag = true),
            ("incoming_drag", |m| m.incoming_drag = true),
        ];
        for (name, set) in modes {
            let mut window = CaptureModes::default();
            set(&mut window);
            assert!(window.any(), "{name}");
        }
    }

    // --- new-tab ------------------------------------------------------

    /// Window 7 (protocol 1) with the explicit group 3 (protocol 1); window 8
    /// (protocol 2) with the group 4 (protocol 2).
    fn ids() -> ProtocolIds<u8> {
        let mut ids = ProtocolIds::new();
        ids.window(7);
        ids.window(8);
        ids.group(7, GroupId::new(3));
        ids.group(8, GroupId::new(4));
        ids
    }

    fn plan(
        ids: &ProtocolIds<u8>,
        window: u64,
        group: Option<u64>,
        profile: Option<&str>,
    ) -> Result<NewTabPlan<u8>, ErrorCode> {
        let group = group.map_or(NewTabGroup::Auto, NewTabGroup::Group);
        plan_of(ids, window, group, profile)
    }

    fn plan_of(
        ids: &ProtocolIds<u8>,
        window: u64,
        group: NewTabGroup,
        profile: Option<&str>,
    ) -> Result<NewTabPlan<u8>, ErrorCode> {
        let offered = [
            profile_of("zsh"),
            profile_of(""),
            profile_of("auto:wsl:Debian"),
        ];
        plan_new_tab(
            ids,
            window,
            group,
            profile,
            &offered,
            |w| w == 7 || w == 8,
            |w, g| (w, g) == (7, GroupId::new(3)) || (w, g) == (8, GroupId::new(4)),
        )
    }

    fn profile_of(id: &str) -> Profile {
        profile(id)
    }

    #[test]
    fn a_new_tab_with_nothing_but_the_window_is_valid() {
        let plan = plan(&ids(), 1, None, None).unwrap();
        assert_eq!(
            plan,
            NewTabPlan {
                window: 7,
                place: NewTabPlace::ActiveGroup,
                profile: None
            }
        );
    }

    /// Spec 0.7: `group: null` is a loose tab. It names no group, so there is
    /// nothing to be unknown, but the window and the profile are still checked.
    #[test]
    fn a_null_group_is_a_loose_tab_and_never_an_unknown_group() {
        let loose = plan_of(&ids(), 1, NewTabGroup::Ungrouped, None).unwrap();
        assert_eq!((loose.window, loose.place), (7, NewTabPlace::Ungrouped));
        assert_eq!(
            plan_of(&ids(), 9, NewTabGroup::Ungrouped, None),
            Err(ErrorCode::UnknownWindow)
        );
        assert_eq!(
            plan_of(&ids(), 1, NewTabGroup::Ungrouped, Some("fish")),
            Err(ErrorCode::UnknownProfile)
        );
        // No group is looked up: even if none exists, it goes through.
        let none = plan_new_tab(
            &ids(),
            1,
            NewTabGroup::Ungrouped,
            None,
            &[],
            |_: u8| true,
            |_: u8, _: GroupId| false,
        );
        assert_eq!(none.unwrap().place, NewTabPlace::Ungrouped);
    }

    #[test]
    fn an_unknown_window_is_refused_first() {
        // Window 9 is not in the table; window 1 is, but it does not exist
        // any more.
        assert_eq!(
            plan(&ids(), 9, Some(99), Some("nope")),
            Err(ErrorCode::UnknownWindow)
        );
        let gone = plan_new_tab(
            &ids(),
            1,
            NewTabGroup::Auto,
            None,
            &[],
            |_: u8| false,
            |_: u8, _: GroupId| true,
        );
        assert_eq!(gone, Err(ErrorCode::UnknownWindow));
    }

    #[test]
    fn a_group_of_another_window_or_gone_is_an_unknown_group() {
        // Group 2 (protocol) belongs to window 8, not to window 7.
        assert_eq!(plan(&ids(), 1, Some(2), None), Err(ErrorCode::UnknownGroup));
        // Never seen.
        assert_eq!(
            plan(&ids(), 1, Some(50), None),
            Err(ErrorCode::UnknownGroup)
        );
        // In the table, but dissolved in the model since the last tree.
        let dissolved = plan_new_tab(
            &ids(),
            1,
            NewTabGroup::Group(1),
            None,
            &[],
            |_: u8| true,
            |_: u8, _: GroupId| false,
        );
        assert_eq!(dissolved, Err(ErrorCode::UnknownGroup));
        // The right one goes through, and comes back as the internal id.
        let ok = plan(&ids(), 1, Some(1), None).unwrap();
        assert_eq!(ok.place, NewTabPlace::Group(GroupId::new(3)));
        let other = plan(&ids(), 2, Some(2), None).unwrap();
        assert_eq!(
            (other.window, other.place),
            (8, NewTabPlace::Group(GroupId::new(4)))
        );
    }

    #[test]
    fn the_shell_is_a_known_profile_and_a_stranger_is_not() {
        // `""` is the `[shell]` of the `profiles` message.
        let ok = plan(&ids(), 1, None, Some("")).unwrap();
        assert_eq!(ok.profile.as_deref(), Some(""));
        assert!(plan(&ids(), 1, None, Some("zsh")).is_ok());
        assert!(plan(&ids(), 1, None, Some("auto:wsl:Debian")).is_ok());
        assert_eq!(
            plan(&ids(), 1, None, Some("fish")),
            Err(ErrorCode::UnknownProfile)
        );
        // The check is case sensitive, like the ids.
        assert_eq!(
            plan(&ids(), 1, None, Some("ZSH")),
            Err(ErrorCode::UnknownProfile)
        );
    }

    #[test]
    fn profile_known_looks_at_the_offered_ids() {
        let offered = [profile("a"), profile("")];
        assert!(profile_known(&offered, "a"));
        assert!(profile_known(&offered, ""));
        assert!(!profile_known(&offered, "b"));
        assert!(!profile_known(&[], ""));
    }

    // --- last tab and closing -----------------------------------------

    #[test]
    fn the_last_tab_of_the_last_window_is_never_closed() {
        // One window, one tab.
        assert!(closes_last_tab(1, 1));
        // One window with a pinned tab and a common one: they count both.
        assert!(!closes_last_tab(1, 2));
        // Two windows: the one that empties just closes.
        assert!(!closes_last_tab(2, 1));
        assert!(!closes_last_tab(3, 1));
        // No window, no tab: nothing to protect, and nothing to close.
        assert!(closes_last_tab(0, 0));
    }

    #[test]
    fn close_tab_refuses_the_last_tab_whatever_it_would_ask() {
        for ask in [
            CloseAsk::Direct,
            CloseAsk::ActiveProcess,
            CloseAsk::Pinned {
                active_process: false,
            },
        ] {
            for confirmed in [false, true] {
                assert_eq!(
                    close_tab_step(1, 1, ask, false, confirmed),
                    CloseStep::Refuse(ErrorCode::LastTab),
                    "{ask:?} {confirmed}"
                );
            }
        }
    }

    #[test]
    fn close_tab_asks_exactly_when_the_desktop_would_open_a_dialog() {
        assert_eq!(
            close_tab_step(2, 1, CloseAsk::Direct, false, false),
            CloseStep::Close
        );
        assert_eq!(
            close_tab_step(1, 3, CloseAsk::ActiveProcess, false, false),
            CloseStep::Ask(ConfirmKind::RunningProcess)
        );
        assert_eq!(
            close_tab_step(1, 3, CloseAsk::ActiveProcess, true, false),
            CloseStep::Ask(ConfirmKind::SshConnection)
        );
        // A pinned tab always asks, process or not (RF-21.23).
        assert_eq!(
            close_tab_step(
                1,
                3,
                CloseAsk::Pinned {
                    active_process: false
                },
                false,
                false
            ),
            CloseStep::Ask(ConfirmKind::RunningProcess)
        );
    }

    #[test]
    fn a_confirmed_close_does_not_ask_again() {
        for ask in [
            CloseAsk::ActiveProcess,
            CloseAsk::Pinned {
                active_process: true,
            },
        ] {
            assert_eq!(close_tab_step(2, 2, ask, true, true), CloseStep::Close);
        }
    }

    #[test]
    fn the_last_pane_of_a_common_tab_is_a_close_tab() {
        assert_eq!(pane_step(true, false, false, false), PaneStep::CloseTab);
        // Whatever the pane would ask on its own, it is the tab that decides.
        assert_eq!(pane_step(true, true, true, false), PaneStep::CloseTab);
    }

    #[test]
    fn a_pane_that_is_not_the_last_follows_its_own_runtime() {
        assert_eq!(pane_step(false, false, false, false), PaneStep::Close);
        assert_eq!(
            pane_step(false, true, false, false),
            PaneStep::Ask(ConfirmKind::RunningProcess)
        );
        assert_eq!(
            pane_step(false, true, true, false),
            PaneStep::Ask(ConfirmKind::SshConnection)
        );
        // Confirmed: closes.
        assert_eq!(pane_step(false, true, false, true), PaneStep::Close);
    }

    #[test]
    fn the_summary_lists_what_is_alive_and_an_ssh_connection_wins() {
        let pane = |title: &str, alive, open_connection| LivePane {
            title: title.to_owned(),
            alive,
            open_connection,
        };
        let (kind, names) = confirm_summary([pane("vim", true, false), pane("zsh", false, false)]);
        assert_eq!(kind, ConfirmKind::RunningProcess);
        assert_eq!(names, ["vim"]);

        let (kind, names) = confirm_summary([pane("vim", true, false), pane("me@srv", true, true)]);
        assert_eq!(kind, ConfirmKind::SshConnection);
        assert_eq!(names, ["vim", "me@srv"]);

        // A pinned tab with nothing alive still asks, with an empty list.
        let (kind, names) = confirm_summary([pane("zsh", false, false)]);
        assert_eq!(kind, ConfirmKind::RunningProcess);
        assert!(names.is_empty());
    }

    // --- start-pane ---------------------------------------------------

    #[test]
    fn only_a_not_started_pane_can_be_started() {
        assert_eq!(start_step(PaneState::NotStarted), Ok(()));
        for state in [
            PaneState::Running,
            PaneState::Exited,
            PaneState::Connecting,
            PaneState::Reconnecting,
            PaneState::Disconnected,
            PaneState::AwaitingDesktop,
        ] {
            assert_eq!(
                start_step(state),
                Err(ErrorCode::InvalidArgument),
                "{state:?}"
            );
        }
    }

    // --- groups and windows -------------------------------------------

    fn tab(n: u32) -> TabId {
        TabId::new(n)
    }

    /// Window 7 (protocol 1): tabs 0, 1 (protocol 1, 2), group 3 (protocol
    /// 1), pinned tab 2 (protocol 3). Window 8 (protocol 2): tab 0 (protocol
    /// 4) and group 4 (protocol 2).
    fn world_ids() -> ProtocolIds<u8> {
        let mut ids = ProtocolIds::new();
        ids.window(7);
        ids.window(8);
        ids.tab(7, tab(0));
        ids.tab(7, tab(1));
        ids.tab(7, tab(2));
        ids.tab(8, tab(0));
        ids.group(7, GroupId::new(3));
        ids.group(8, GroupId::new(4));
        ids
    }

    fn tab_exists(window: u8, tab: TabId) -> bool {
        matches!((window, tab.get()), (7, 0..=2) | (8, 0))
    }

    fn group_exists(window: u8, group: GroupId) -> bool {
        (window, group) == (7, GroupId::new(3)) || (window, group) == (8, GroupId::new(4))
    }

    fn is_pinned(window: u8, tab: TabId) -> bool {
        (window, tab) == (7, TabId::new(2))
    }

    fn never_busy(_: u8) -> bool {
        false
    }

    #[test]
    fn a_color_is_an_index_into_the_six_of_the_palette() {
        assert_eq!(parse_group_color(0), Ok(GroupColor::ALL[0]));
        assert_eq!(parse_group_color(5), Ok(GroupColor::ALL[5]));
        assert_eq!(parse_group_color(6), Err(ErrorCode::InvalidArgument));
        assert_eq!(parse_group_color(u64::MAX), Err(ErrorCode::InvalidArgument));
    }

    fn move_to_group(
        tab: u64,
        group: Option<u64>,
        busy: impl Fn(u8) -> bool,
    ) -> Result<MoveToGroupPlan<u8>, ErrorCode> {
        plan_move_tab_to_group(
            &world_ids(),
            tab,
            group,
            tab_exists,
            group_exists,
            is_pinned,
            busy,
        )
    }

    #[test]
    fn move_tab_to_group_resolves_the_tab_and_the_group_of_its_window() {
        let plan = move_to_group(1, Some(1), never_busy).unwrap();
        assert_eq!(
            plan,
            MoveToGroupPlan {
                window: 7,
                tab: tab(0),
                target: GroupMove::Into(GroupId::new(3)),
            }
        );
        let leave = move_to_group(2, None, never_busy).unwrap();
        assert_eq!(leave.target, GroupMove::Leave);
        assert_eq!((leave.window, leave.tab), (7, tab(1)));
    }

    #[test]
    fn move_tab_to_group_checks_the_ids_then_busy_then_the_rules() {
        // Unknown tab first, whatever the rest says.
        assert_eq!(
            move_to_group(99, Some(99), |_| true),
            Err(ErrorCode::UnknownTab)
        );
        // Then the unknown group, before busy.
        assert_eq!(
            move_to_group(1, Some(99), |_| true),
            Err(ErrorCode::UnknownGroup)
        );
        // A group of another window does not exist for this tab, like in
        // `new-tab` -- also before busy.
        assert_eq!(
            move_to_group(1, Some(2), |_| true),
            Err(ErrorCode::UnknownGroup)
        );
        // Then busy, in the window of the tab, before the rules: the pinned
        // tab is `invalid-argument` only when nothing is busy.
        assert_eq!(move_to_group(3, None, |w| w == 7), Err(ErrorCode::Busy));
        assert_eq!(
            move_to_group(3, None, never_busy),
            Err(ErrorCode::InvalidArgument)
        );
        // Pinned, even into a group of its own window.
        assert_eq!(
            move_to_group(3, Some(1), never_busy),
            Err(ErrorCode::InvalidArgument)
        );
    }

    #[test]
    fn an_implicit_or_dissolved_group_is_an_unknown_group() {
        let plan = plan_move_tab_to_group(
            &world_ids(),
            1,
            Some(1),
            tab_exists,
            |_, _| false,
            is_pinned,
            never_busy,
        );
        assert_eq!(plan, Err(ErrorCode::UnknownGroup));
    }

    #[test]
    fn a_tab_leaves_its_group_into_the_slot_right_after_it() {
        // `a [G: b c] [H: d] e`: the groups are `imp[a] G H imp[e]`.
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let d = ws.append_tab("zsh", None);
        let e = ws.append_tab("zsh", None);
        ws.group_tabs(&[b, c], "g", GroupColor::Blue).unwrap();
        ws.group_tabs(&[d], "h", GroupColor::Red).unwrap();
        // Loose tabs are in no explicit group: nothing to do.
        assert_eq!(leave_group_slot(&ws, a), None);
        assert_eq!(leave_group_slot(&ws, e), None);
        // `b` is in the group at index 1: the slot is 2, between G and H.
        assert_eq!(leave_group_slot(&ws, b), Some(2));
        assert_eq!(leave_group_slot(&ws, d), Some(3));
        assert_eq!(leave_group_slot(&ws, tab(99)), None);

        // And the order that results is fixed: `a | G[c] | b | H[d] | e`.
        assert!(ws.move_tab_to_new_run(b, 2));
        let order: Vec<TabId> = ws.track_order().collect();
        assert_eq!(order, [a, c, b, d, e]);
        let group_of: Vec<Option<bool>> = order
            .iter()
            .map(|t| {
                ws.group_of_tab(*t)
                    .and_then(|id| ws.group(id))
                    .map(porecatu_core::Group::is_explicit)
            })
            .collect();
        assert_eq!(
            group_of,
            [
                Some(false),
                Some(true),
                Some(false),
                Some(true),
                Some(false)
            ]
        );
    }

    #[test]
    fn a_single_tab_group_that_loses_its_tab_goes_away() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[b], "g", GroupColor::Blue).unwrap();
        let slot = leave_group_slot(&ws, b).unwrap();
        assert!(ws.move_tab_to_new_run(b, slot));
        assert!(ws.group(group).is_none());
        assert_eq!(ws.track_order().collect::<Vec<_>>(), [a, b]);
    }

    fn new_group(
        tabs: &[u64],
        color: Option<u64>,
        busy: impl Fn(u8) -> bool,
    ) -> Result<NewGroupPlan<u8>, ErrorCode> {
        plan_new_group(&world_ids(), tabs, color, tab_exists, is_pinned, busy)
    }

    #[test]
    fn new_group_takes_tabs_of_one_window_in_the_order_given() {
        let plan = new_group(&[2, 1], None, never_busy).unwrap();
        assert_eq!(
            plan,
            NewGroupPlan {
                window: 7,
                tabs: vec![tab(1), tab(0)],
                color: None,
            }
        );
        // A repeated id counts once.
        let plan = new_group(&[1, 1, 2], Some(3), never_busy).unwrap();
        assert_eq!(plan.tabs, [tab(0), tab(1)]);
        assert_eq!(plan.color, Some(GroupColor::ALL[3]));
    }

    #[test]
    fn new_group_refuses_what_the_spec_refuses() {
        // An empty list.
        assert_eq!(
            new_group(&[], None, never_busy),
            Err(ErrorCode::InvalidArgument)
        );
        // Tabs of different windows.
        assert_eq!(
            new_group(&[1, 4], None, never_busy),
            Err(ErrorCode::InvalidArgument)
        );
        // A pinned tab among them.
        assert_eq!(
            new_group(&[1, 3], None, never_busy),
            Err(ErrorCode::InvalidArgument)
        );
        // A color outside the palette.
        assert_eq!(
            new_group(&[1], Some(6), never_busy),
            Err(ErrorCode::InvalidArgument)
        );
    }

    #[test]
    fn new_group_checks_unknown_then_busy_then_the_rules() {
        assert_eq!(
            new_group(&[1, 99], Some(99), |_| true),
            Err(ErrorCode::UnknownTab)
        );
        // Busy in any window involved, before the rules.
        assert_eq!(new_group(&[1, 4], None, |w| w == 8), Err(ErrorCode::Busy));
        assert_eq!(new_group(&[3], Some(99), |w| w == 7), Err(ErrorCode::Busy));
    }

    fn group_edit(group: u64, busy: impl Fn(u8) -> bool) -> Result<(u8, GroupId), ErrorCode> {
        plan_group_edit(&world_ids(), group, group_exists, busy)
    }

    #[test]
    fn an_action_on_a_group_needs_it_to_exist_and_its_window_free() {
        assert_eq!(group_edit(2, never_busy), Ok((8, GroupId::new(4))));
        assert_eq!(group_edit(99, |_| true), Err(ErrorCode::UnknownGroup));
        assert_eq!(group_edit(1, |w| w == 7), Err(ErrorCode::Busy));
        // Busy elsewhere does not matter.
        assert_eq!(group_edit(1, |w| w == 8), Ok((7, GroupId::new(3))));
    }

    // --- collapse-group -----------------------------------

    /// `a [G: b c] d` with `a` active, G collapsed or not as asked.
    fn grouped(collapsed: bool) -> (Workspace, GroupId, [TabId; 4]) {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let d = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[b, c], "g", GroupColor::Blue).unwrap();
        assert!(ws.activate_tab(a));
        if collapsed {
            assert!(ws.collapse_group(group, true));
        }
        (ws, group, [a, b, c, d])
    }

    /// ADR-0111 section 1: a remote `collapse-group` expands in place (no tab
    /// is activated) and collapses like the click on the pill; the order that
    /// results is fixed. Collapsing the group of the active tab moves it by
    /// the ladder.
    #[test]
    fn expanding_keeps_the_active_tab_and_collapsing_its_group_follows_the_ladder() {
        // Collapsing a group that does not hold the active tab keeps it.
        let (mut ws, group, [a, b, c, d]) = grouped(false);
        assert!(ws.collapse_group(group, true));
        assert!(ws.group(group).unwrap().is_collapsed());
        assert_eq!(ws.active_tab(), Some(a));

        // Expanding: nothing is activated.
        assert!(ws.expand_group_in_place(group));
        assert!(!ws.group(group).unwrap().is_collapsed());
        assert_eq!(ws.active_tab(), Some(a));
        assert_eq!(ws.track_order().collect::<Vec<_>>(), [a, b, c, d]);

        // Expanding a group that is already expanded, with the active tab
        // inside it (not the first): the active tab stays.
        assert!(ws.activate_tab(c));
        assert!(ws.expand_group_in_place(group));
        assert_eq!(ws.active_tab(), Some(c));
        assert!(!ws.group(group).unwrap().is_collapsed());

        // Collapsing the group of the active tab: the ladder (RF-2.14); the
        // active tab leaves the group, to the nearest visible one.
        assert!(ws.collapse_group(group, true));
        assert_ne!(ws.active_tab(), Some(c));
        assert!(matches!(ws.active_tab(), Some(t) if t == a || t == d));
        assert_eq!(ws.track_order().collect::<Vec<_>>(), [a, b, c, d]);

        // Expanding it again leaves the active tab where the ladder put it.
        let landed = ws.active_tab();
        assert!(ws.expand_group_in_place(group));
        assert_eq!(ws.active_tab(), landed);
    }

    #[test]
    fn collapsing_an_unknown_or_implicit_group_changes_nothing() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let implicit = ws.group_of_tab(a).unwrap();
        assert!(!ws.collapse_group(implicit, true));
        assert!(!ws.collapse_group(GroupId::new(99), false));
        assert_eq!(ws.active_tab(), Some(a));
    }

    /// RF-2.17: the model's own `activate_tab` -- what the click on a tab
    /// calls -- expands the collapsed group of the tab, and nothing else moves.
    #[test]
    fn activating_a_tab_of_a_collapsed_group_expands_it() {
        let (mut ws, group, [a, b, c, d]) = grouped(true);
        assert!(ws.group(group).unwrap().is_collapsed());
        assert!(ws.activate_tab(c));
        assert!(!ws.group(group).unwrap().is_collapsed());
        assert_eq!(ws.active_tab(), Some(c));
        assert_eq!(ws.track_order().collect::<Vec<_>>(), [a, b, c, d]);
        // A tab outside any collapsed group: only the active one changes.
        assert!(ws.activate_tab(d));
        assert_eq!(ws.active_tab(), Some(d));
        assert!(!ws.group(group).unwrap().is_collapsed());
    }

    /// ADR-0111 section 1: a remote `new-tab` creates the tab where `tab.new`
    /// puts it (the end of the group of the active tab) but **does not
    /// activate** it, and it is born `Running` -- the process is spawned by
    /// `open_tab` whatever `activate` says, so it is not a `NotStarted` one.
    #[test]
    fn a_remote_new_tab_is_where_tab_new_puts_it_and_the_active_tab_stays() {
        let (mut ws, group, [a, b, c, d]) = grouped(false);
        assert!(ws.activate_tab(b));
        let created = ws.append_tab_with("zsh", None, false);
        assert_eq!(ws.active_tab(), Some(b));
        assert_eq!(ws.group_of_tab(created), Some(group));
        assert_eq!(ws.track_order().collect::<Vec<_>>(), [a, b, c, created, d]);
        assert!(!ws.tab(created).unwrap().is_not_started());
        // With a group asked for, at its end, the active tab stays as well.
        assert!(ws.activate_tab(a));
        let in_group = ws.new_tab_with(Some(group), "zsh", None, 3, false);
        assert_eq!(ws.active_tab(), Some(a));
        assert_eq!(
            ws.track_order().collect::<Vec<_>>(),
            [a, b, c, created, in_group, d]
        );
        assert!(!ws.tab(in_group).unwrap().is_not_started());
        // Without a group, in the window of a pinned active tab: at the end of
        // the track, and the pinned one stays active.
        let mut pinned = Workspace::new();
        let p = pinned.append_tab("zsh", None);
        assert!(pinned.pin_tab(p, 0));
        assert!(pinned.activate_tab(p));
        let loose = pinned.append_tab_with("zsh", None, false);
        assert_eq!(pinned.active_tab(), Some(p));
        assert_eq!(pinned.track_order().collect::<Vec<_>>(), [loose]);
    }

    /// Spec 0.7: `new-tab` with `group: null` goes through
    /// `append_ungrouped_tab_with` (`NewTabTarget::Ungrouped`): with the active
    /// tab inside the **last** group, the tab is born outside it, at the end of
    /// the track, and the active tab stays. (Without the `null`, it would join
    /// the group of the active tab: the bug this fixes.)
    #[test]
    fn a_loose_remote_new_tab_ignores_the_group_of_the_active_tab() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a, b], "last", GroupColor::Blue).unwrap();
        assert!(ws.activate_tab(b));
        let in_group = ws.append_tab_with("zsh", None, false);
        assert_eq!(ws.group_of_tab(in_group), Some(group));
        let loose = ws.append_ungrouped_tab_with("zsh", None, false);
        assert_eq!(ws.active_tab(), Some(b));
        assert_ne!(ws.group_of_tab(loose), Some(group));
        assert!(!ws.group(group).unwrap().tabs().contains(&loose));
        assert_eq!(ws.track_order().last(), Some(loose));
    }

    /// ADR-0111 section 1: a remote `start-pane` starts the tab without
    /// activating it. In the model, starting a `NotStarted` tab is a state
    /// change of the tab (`Tab::start`) and leaves the active tab alone.
    #[test]
    fn starting_a_tab_that_is_not_active_leaves_the_active_tab() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.new_tab_not_started(None, "zsh", None, 1);
        assert!(ws.activate_tab(a));
        assert!(ws.tab(b).unwrap().is_not_started());
        ws.tab_mut(b).unwrap().start();
        assert!(!ws.tab(b).unwrap().is_not_started());
        assert_eq!(ws.active_tab(), Some(a));
    }

    fn tab_to_window(
        tab: u64,
        window: u64,
        busy: impl Fn(u8) -> bool,
    ) -> Result<TabToWindowPlan<u8>, ErrorCode> {
        plan_tab_to_window(
            &world_ids(),
            tab,
            window,
            tab_exists,
            |w| w == 7 || w == 8,
            busy,
        )
    }

    #[test]
    fn move_tab_to_window_resolves_origin_tab_and_destination() {
        assert_eq!(
            tab_to_window(1, 2, never_busy),
            Ok(TabToWindowPlan {
                from: 7,
                tab: tab(0),
                to: 8
            })
        );
        // The last tab of a window, a pinned one: the plan lets them go.
        assert!(tab_to_window(4, 1, never_busy).is_ok());
        assert!(tab_to_window(3, 2, never_busy).is_ok());
    }

    #[test]
    fn move_tab_to_window_checks_the_ids_then_busy_then_the_window() {
        assert_eq!(tab_to_window(99, 99, |_| true), Err(ErrorCode::UnknownTab));
        assert_eq!(
            tab_to_window(1, 99, |_| true),
            Err(ErrorCode::UnknownWindow)
        );
        // Busy in the origin or in the destination.
        assert_eq!(tab_to_window(1, 2, |w| w == 7), Err(ErrorCode::Busy));
        assert_eq!(tab_to_window(1, 2, |w| w == 8), Err(ErrorCode::Busy));
        // The same window: nothing would move.
        assert_eq!(
            tab_to_window(1, 1, never_busy),
            Err(ErrorCode::InvalidArgument)
        );
        // A window that is in the table but is gone.
        let gone = plan_tab_to_window(&world_ids(), 1, 2, tab_exists, |w| w == 7, never_busy);
        assert_eq!(gone, Err(ErrorCode::UnknownWindow));
    }

    fn group_to_window(
        group: u64,
        window: u64,
        busy: impl Fn(u8) -> bool,
    ) -> Result<GroupToWindowPlan<u8>, ErrorCode> {
        plan_group_to_window(
            &world_ids(),
            group,
            window,
            group_exists,
            |w| w == 7 || w == 8,
            busy,
        )
    }

    #[test]
    fn move_group_to_window_resolves_and_checks_like_the_tab() {
        assert_eq!(
            group_to_window(1, 2, never_busy),
            Ok(GroupToWindowPlan {
                from: 7,
                group: GroupId::new(3),
                to: 8
            })
        );
        assert_eq!(
            group_to_window(99, 99, |_| true),
            Err(ErrorCode::UnknownGroup)
        );
        assert_eq!(
            group_to_window(1, 99, |_| true),
            Err(ErrorCode::UnknownWindow)
        );
        assert_eq!(group_to_window(1, 2, |w| w == 8), Err(ErrorCode::Busy));
        assert_eq!(
            group_to_window(1, 1, never_busy),
            Err(ErrorCode::InvalidArgument)
        );
    }

    // --- pending confirmations ----------------------------------------

    fn client(n: u64) -> ClientId {
        ClientId(n)
    }

    #[test]
    fn an_accepted_confirmation_hands_the_target_back_once() {
        let t0 = Instant::now();
        let mut pending = Confirmations::default();
        assert!(pending.insert(client(1), 10, ConfirmTarget::Tab(5), t0));
        assert_eq!(pending.answer(client(1), 10), Some(ConfirmTarget::Tab(5)));
        // Answered: gone. A second answer finds nothing.
        assert_eq!(pending.answer(client(1), 10), None);
        assert!(pending.is_empty());
    }

    #[test]
    fn a_request_is_keyed_by_client_and_id() {
        let t0 = Instant::now();
        let mut pending = Confirmations::default();
        assert!(pending.insert(client(1), 10, ConfirmTarget::Tab(5), t0));
        // The same id from another client is another request.
        assert!(pending.insert(client(2), 10, ConfirmTarget::Pane(6), t0));
        // The same id from the same client while pending is refused and
        // changes nothing.
        assert!(!pending.insert(client(1), 10, ConfirmTarget::Pane(9), t0));
        assert_eq!(pending.answer(client(1), 10), Some(ConfirmTarget::Tab(5)));
        assert_eq!(pending.answer(client(2), 10), Some(ConfirmTarget::Pane(6)));
        // A client cannot answer another one's request.
        pending.insert(client(1), 11, ConfirmTarget::Tab(1), t0);
        assert_eq!(pending.answer(client(2), 11), None);
        assert_eq!(pending.answer(client(1), 11), Some(ConfirmTarget::Tab(1)));
    }

    #[test]
    fn a_request_expires_after_sixty_seconds() {
        let t0 = Instant::now();
        let mut pending = Confirmations::default();
        pending.insert(client(1), 10, ConfirmTarget::Tab(5), t0);
        pending.insert(
            client(1),
            11,
            ConfirmTarget::Tab(6),
            t0 + Duration::from_secs(30),
        );
        assert_eq!(pending.deadline(), Some(t0 + CONFIRM_TIMEOUT));

        // Just before: nothing.
        assert!(pending.expire(t0 + Duration::from_secs(59)).is_empty());
        // At the deadline: the first, and only it.
        assert_eq!(pending.expire(t0 + CONFIRM_TIMEOUT), vec![(client(1), 10)]);
        assert_eq!(
            pending.deadline(),
            Some(t0 + Duration::from_secs(30) + CONFIRM_TIMEOUT)
        );
        // An expired request cannot be accepted any more.
        assert_eq!(pending.answer(client(1), 10), None);
        // Then the second.
        assert_eq!(
            pending.expire(t0 + Duration::from_secs(91)),
            vec![(client(1), 11)]
        );
        assert_eq!(pending.deadline(), None);
        assert_eq!(CONFIRM_TIMEOUT, Duration::from_secs(60));
    }

    #[test]
    fn a_client_that_leaves_takes_its_requests_with_it() {
        let t0 = Instant::now();
        let mut pending = Confirmations::default();
        pending.insert(client(1), 10, ConfirmTarget::Tab(5), t0);
        pending.insert(client(1), 11, ConfirmTarget::Pane(6), t0);
        pending.insert(client(2), 10, ConfirmTarget::Tab(7), t0);
        pending.drop_client(client(1));
        assert_eq!(pending.answer(client(1), 10), None);
        assert_eq!(pending.answer(client(1), 11), None);
        // No answer is owed: they do not come out as expired either.
        assert!(pending.expire(t0 + CONFIRM_TIMEOUT).len() == 1);
    }

    #[test]
    fn clearing_forgets_everything_and_the_deadline() {
        let t0 = Instant::now();
        let mut pending = Confirmations::default();
        pending.insert(client(1), 10, ConfirmTarget::Tab(5), t0);
        pending.clear();
        assert!(pending.is_empty());
        assert_eq!(pending.deadline(), None);
    }

    /// The target is the protocol id: it does not follow the internal ids,
    /// and a target that left the table is `unknown-*` at accept time, which
    /// is the job of the caller -- here, only that nothing is lost on the
    /// way.
    #[test]
    fn the_target_is_kept_as_the_protocol_id() {
        let mut ids = ProtocolIds::<u8>::new();
        let tab = ids.tab(1, TabId::new(0));
        let pane = ids.pane(1, TabId::new(0), PaneId::new(0));
        let t0 = Instant::now();
        let mut pending = Confirmations::default();
        pending.insert(client(1), 1, ConfirmTarget::Tab(tab), t0);
        pending.insert(client(1), 2, ConfirmTarget::Pane(pane), t0);
        // The tab moves to window 2 and gets a new internal id.
        ids.rekey_tabs(1, 2, &[(TabId::new(0), TabId::new(4))]);
        let Some(ConfirmTarget::Tab(id)) = pending.answer(client(1), 1) else {
            panic!("the tab request is pending");
        };
        assert_eq!(ids.tab_of(id), Some((2, TabId::new(4))));
        let Some(ConfirmTarget::Pane(id)) = pending.answer(client(1), 2) else {
            panic!("the pane request is pending");
        };
        assert_eq!(ids.pane_of(id), Some((2, TabId::new(4), PaneId::new(0))));
    }
}
