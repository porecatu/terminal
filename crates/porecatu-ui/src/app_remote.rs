// SPDX-License-Identifier: GPL-3.0-or-later

//! The remote access in the `App` (PRD-025, ADR-0104 sections 2 to 4, ADR-0105
//! sections 1, 2 and 6): the life of the [`RemoteHost`] by `[remote]`, the
//! events it sends back as `Wakeup::Remote`, and what the app sends to it --
//! the theme, the profiles and the workspace tree.
//!
//! The model is `sync_task_manager_sampler`: `sync_remote` runs on every turn
//! of the event loop, reads the current `Config` and reconciles. With
//! `enabled = false` nothing of `porecatu-remote` is called (RNF-25.1). The
//! main thread never waits for the network: `RemoteHost::send` does not block,
//! and every answer comes back as data through the proxy.
//!
//! The actions of a phone on tabs and panes (`new-tab`,
//! `close-tab`, `close-pane`, `rename-tab`, `start-pane`), on groups
//! (`move-tab-to-group`, `new-group`, `rename-group`, `set-group-color`,
//! `collapse-group`, `ungroup`) and between windows (`move-tab-to-window`,
//! `move-group-to-window`, only through `App::transfer_with`), and the
//! confirmations they ask of it, end here too (ADR-0107, ADR-0111): the same
//! functions the local gestures call, with an explicit target, no activation
//! of a tab (only the removal of the active one moves it, by the focus
//! ladder), and never a touch on the window of the OS (no focus, no raise, no
//! restore). Which tab a phone is *viewing* (`view-tab`) is information only:
//! the host keeps it per phone and projects it, per window, for the indicator
//! of the tab bar (ADR-0112 section 1); it never changes the workspace.
//!
//! What needs no window lives in `remote/` (the state, the ids, the tree, the
//! decisions of `remote/actions.rs`).

use porecatu_core::PaneState as ModelPaneState;
use porecatu_remote::protocol::message::grid::MAX_SCROLLBACK_COUNT;
use porecatu_remote::protocol::message::{
    Action, ActionBody, ActionResult, Confirm, ConfirmKind, ConfirmRequest, ErrorCode,
    ErrorMessage, GridSnapshot as WireGrid, Message, NewTabGroup, PaneState as WireState,
    ScrollbackPage, ScrollbackRequest, Sessions, Workspace as WireWorkspace,
};
use porecatu_remote::{
    ClientId, Command, PairingAnswer, PairingId, RemoteEvent, RemoteHost, RemoteInput,
    RemoteParams, StopReason, VaultError,
};

use super::*;
use crate::app_transfer::{TransferArrival, TransferDest, TransferPayload};
use crate::devices_popover::DeviceEntry;
use crate::move_to_window::destination_insert_at;
use crate::remote::actions::{
    CaptureModes, CloseStep, ConfirmTarget, GroupMove, LivePane, NewTabPlace, PaneStep,
    close_tab_step, confirm_summary, leave_group_slot, pane_step, parse_group_color,
    plan_group_edit, plan_group_to_window, plan_move_tab_to_group, plan_new_group, plan_new_tab,
    plan_tab_to_window, start_step,
};
use crate::remote::grid::{empty_frame, live_frame, scrollback_lines};
use crate::remote::ids::{ProtocolIds, SeenIds};
use crate::remote::input::remote_input_bytes;
use crate::remote::sessions::{RestoreTarget, named_sessions, plan_restore_session, sessions_mode};
use crate::remote::theme::{protocol_profiles, protocol_theme};
use crate::remote::tree::{
    PaneFacts, PaneSignals, TabFacts, WindowInput, build_workspace, pane_state,
};
use crate::remote::{
    Effect, HostPlan, MirrorDue, TreeDue, host_plan, remote_wanted, viewers_by_window,
    viewers_current,
};
use crate::settings::{RemoteRequest, TokenWrite, device_rows, token_change_restarts_host};

/// What a short vault thread found or did (ADR-0108 section 1); the payload of
/// `Wakeup::RemoteVault`. It carries no secret: only what the vault answered.
#[derive(Debug)]
pub(crate) enum VaultReport {
    /// The answer of `operator_token_present`.
    Presence(Result<bool, VaultError>),
    /// A write (or the deletion, `deleted`) of the token finished.
    Written {
        deleted: bool,
        result: Result<(), VaultError>,
    },
}

/// What the phone is told of a restore that went through the read of the file
/// (ADR-0113 section 4, step 5): success has no `created`; any failure to
/// read, whatever the cause, is `session-unreadable`.
fn restore_outcome(restored: Result<(), RestoreFailure>) -> Result<Option<u64>, ErrorCode> {
    restored
        .map(|()| None)
        .map_err(|_| ErrorCode::SessionUnreadable)
}

/// The size the protocol gets for a pane that has no grid yet (a `NotStarted`
/// tab, a pane whose first snapshot was not taken).
const DEFAULT_GRID: (u64, u64) = (80, 24);

/// What the remote access reads about one pane, in plain data: the signals of
/// [`pane_state`] plus the displayed text and the grid size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneInputs {
    pub(crate) signals: PaneSignals,
    pub(crate) title: String,
    pub(crate) cwd: Option<String>,
    /// `(cols, rows)` of the runtime's engine now; `None` without runtime.
    pub(crate) grid: Option<(usize, usize)>,
}

/// The facts of one pane for the tree (ADR-0105 section 2). The directory of
/// an SSH pane is never sent -- the model does not hold the server's one
/// either (RF-20.22), this keeps it that way whatever the model does.
pub(crate) fn assemble_pane_facts(inputs: PaneInputs) -> PaneFacts {
    let (state, exit) = pane_state(&inputs.signals);
    let (cols, rows) = match inputs.grid {
        Some((cols, rows)) if cols > 0 && rows > 0 => (cols as u64, rows as u64),
        _ => DEFAULT_GRID,
    };
    PaneFacts {
        state,
        exit,
        cols,
        rows,
        cwd: if inputs.signals.ssh { None } else { inputs.cwd },
        title: inputs.title,
    }
}

/// What the app knows about the state of pane `(tab, pane)` of a window, as
/// plain signals (no text: this is cheap, the input path asks it per key).
fn pane_signals(state: &WindowState, tab: TabId, pane: PaneId) -> PaneSignals {
    let model = state.workspace.tab(tab).and_then(|t| t.panes().pane(pane));
    let runtime = state.panes.get(&(tab, pane));
    PaneSignals {
        model: model.map_or(ModelPaneState::NotStarted, porecatu_core::Pane::state),
        has_runtime: runtime.is_some(),
        dormant: runtime.is_some_and(|rt| rt.dormant),
        ssh: runtime.is_some_and(|rt| rt.ssh_spawn.is_some()),
        reconnecting: runtime.is_some_and(|rt| rt.reconnecting),
        connected: runtime.is_some_and(|rt| rt.connected),
        link_dropped: runtime.is_some_and(|rt| rt.link_dropped),
        prompt_pending: state.prompt_pending(tab, pane),
    }
}

/// The signals and text of pane `(tab, pane)` of a window, as it is now.
fn pane_inputs(state: &WindowState, tab: TabId, pane: PaneId) -> PaneInputs {
    let model = state.workspace.tab(tab).and_then(|t| t.panes().pane(pane));
    let runtime = state.panes.get(&(tab, pane));
    PaneInputs {
        signals: pane_signals(state, tab, pane),
        title: model.map(|p| p.title().to_owned()).unwrap_or_default(),
        cwd: model
            .and_then(|p| p.cwd())
            .map(|path| path.to_string_lossy().into_owned()),
        // From the engine, not `rt.snapshot`: the snapshot is only refreshed
        // when the tab is painted, and the tree covers invisible tabs too.
        grid: runtime.map(|rt| rt.terminal.grid_size()),
    }
}

/// The pane (protocol id) a remote input is for.
fn input_pane(input: &RemoteInput) -> u64 {
    match input {
        RemoteInput::Key(key) => key.pane,
        RemoteInput::Text(text) => text.pane,
        RemoteInput::Paste(paste) => paste.pane,
    }
}

/// What a remote input turns into (RF-25.21 and RF-25.22, ADR-0105 section
/// 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InputAnswer {
    /// Write these bytes to the PTY (may be empty: a key this build does not
    /// know).
    Bytes(Vec<u8>),
    /// `error{pane-not-running}`.
    NotRunning,
}

/// The decision: only a pane in the `running` state takes input. `modes`
/// reads the pane's terminal modes and is only called for a running pane (it
/// locks the engine); `None` is a pane with nothing to write to.
pub(crate) fn input_answer(
    state: WireState,
    input: &RemoteInput,
    modes: impl FnOnce() -> Option<TermModes>,
) -> InputAnswer {
    if state != WireState::Running {
        return InputAnswer::NotRunning;
    }
    match modes() {
        Some(modes) => InputAnswer::Bytes(remote_input_bytes(input, &modes)),
        None => InputAnswer::NotRunning,
    }
}

/// `(offset, count)` of a scrollback request as the engine takes them: the
/// count is capped at the protocol's 500 (ADR-0105 section 5).
pub(crate) fn scrollback_window(request: &ScrollbackRequest) -> (usize, usize) {
    let count = request.count.min(MAX_SCROLLBACK_COUNT);
    (
        usize::try_from(request.offset).unwrap_or(usize::MAX),
        usize::try_from(count).unwrap_or(usize::MAX),
    )
}

/// The live screen of pane `(tab, pane)` as a `grid-snapshot` (ADR-0105
/// sections 3 and 8). A pane with no runtime (a `NotStarted` tab: decided
/// **before** any access to a runtime) or a dormant one has no engine to
/// read: it gets an empty screen of the size the tree already says (80x24
/// when nothing is known), the one source of "last known size".
fn mirror_frame(
    state: &WindowState,
    tab: TabId,
    pane: PaneId,
    buf: &mut porecatu_term::GridSnapshot,
) -> WireGrid {
    match state.panes.get(&(tab, pane)).filter(|rt| !rt.dormant) {
        Some(runtime) => {
            let info = runtime.terminal.snapshot_live_into(buf);
            live_frame(buf, info)
        }
        None => {
            let facts = assemble_pane_facts(pane_inputs(state, tab, pane));
            empty_frame(facts.cols, facts.rows)
        }
    }
}

/// The answer to one scrollback request: the page, an empty one for a pane
/// with nothing to read, or `error{unknown-pane}` (every request is answered,
/// ADR-0105 section 5).
fn scrollback_answer(
    windows: &HashMap<WindowId, WindowState>,
    ids: &ProtocolIds<WindowId>,
    request: &ScrollbackRequest,
    buf: &mut porecatu_term::GridSnapshot,
) -> Message {
    let resolved = ids
        .pane_of(request.pane)
        .and_then(|(window, tab, pane)| Some((windows.get(&window)?, tab, pane)));
    let Some((state, tab, pane)) = resolved else {
        return Message::Error(ErrorMessage {
            code: ErrorCode::UnknownPane,
            id: Some(request.id),
            pane: Some(request.pane),
        });
    };
    let page = |history, dropped, styles, lines| {
        Message::ScrollbackPage(ScrollbackPage {
            id: request.id,
            pane: request.pane,
            history,
            dropped,
            offset: request.offset,
            styles,
            lines,
        })
    };
    let Some(runtime) = state.panes.get(&(tab, pane)).filter(|rt| !rt.dormant) else {
        return page(0, 0, Vec::new(), Vec::new());
    };
    let (offset, count) = scrollback_window(request);
    let info = runtime.terminal.history_into(offset, count, buf);
    let (styles, lines) = scrollback_lines(buf);
    page(info.history as u64, info.dropped, styles, lines)
}

fn tab_facts(state: &WindowState, tab: TabId) -> TabFacts {
    let focused = state.workspace.tab(tab).map(|t| t.panes().focused());
    TabFacts {
        profile: focused.and_then(|p| p.profile()).map(str::to_owned),
        remote: state.pane_runtime(tab).and_then(|rt| rt.ssh_target.clone()),
    }
}

/// How an action ended on this turn (RF-25.23, ADR-0107 section 1).
enum ActionFlow {
    /// The `action-result` to send: `Ok(created)` is success (`created` is
    /// the protocol id of the new tab of a `new-tab`), `Err` the error code.
    Done(Result<Option<u64>, ErrorCode>),
    /// A `confirm-request` went to the phone; the result comes with its
    /// answer, or with the deadline.
    Asked,
}

/// What a close decided (the result of `close-tab`/`close-pane` before it is
/// told to the phone).
enum CloseOutcome {
    /// It was closed.
    Closed,
    /// The local rule asks first: this goes to the phone, not to the desktop.
    Ask {
        kind: ConfirmKind,
        processes: Vec<String>,
    },
}

impl WindowState {
    /// Which capture modes the window is in (ADR-0107 section 4).
    fn capture_modes(&self) -> CaptureModes {
        CaptureModes {
            dialog: self.dialog.is_some(),
            secret_field: self.secret_field.is_some(),
            context_menu: self.context_menu.is_some(),
            group_context_menu: self.group_context_menu.is_some(),
            profile_menu: self.profile_menu.is_some(),
            terminal_context_menu: self.terminal_context_menu.is_some(),
            group_editor: self.group_editor.is_some(),
            move_to_group: self.move_to_group.is_some(),
            move_to_window: self.move_to_window.is_some(),
            session_picker: self.session_picker.is_some(),
            devices_popover: self.devices_popover.is_some(),
            task_menu: self
                .task_manager
                .as_ref()
                .is_some_and(|view| view.menu.is_some()),
            rename: self.rename.editing_tab().is_some(),
            drag: !matches!(self.drag, Drag::Idle),
            incoming_drag: self.incoming.is_some(),
        }
    }

    /// The window is in a capture mode: the user is in the middle of a
    /// gesture there, and a remote action on it answers `busy` (ADR-0107
    /// section 4). One predicate for every remote action.
    fn is_capturing(&self) -> bool {
        self.capture_modes().any()
    }

    /// The panes of the tab `tab` listed in `panes`, as a `confirm-request`
    /// reads them: the title each shows, whether something is alive in it
    /// (a program in the foreground, an alternate screen, mouse reporting or
    /// an open connection -- whatever `confirm_close_with_process` says) and
    /// whether that is an open SSH connection. A pane with no runtime has
    /// nothing alive and is left out.
    fn live_panes(&self, tab: TabId, panes: &[PaneId]) -> Vec<LivePane> {
        let Some(model) = self.workspace.tab(tab) else {
            return Vec::new();
        };
        panes
            .iter()
            .filter_map(|pane| {
                let runtime = self.panes.get(&(tab, *pane))?;
                let liveness = runtime.liveness();
                Some(LivePane {
                    title: model.panes().pane(*pane)?.title().to_owned(),
                    alive: should_confirm_tab_close(
                        true,
                        runtime.terminal.modes(),
                        liveness_needs_confirmation(liveness),
                    ),
                    open_connection: matches!(liveness, Liveness::OpenConnection),
                })
            })
            .collect()
    }
}

impl App {
    /// Reconciles the host with the config and sends what is due (called from
    /// `schedule_next_wake`, every turn of the event loop, like the sampler).
    ///
    /// - wanted (`[remote] enabled` and a window) and no host: start one;
    /// - the `relay_url` changed under a live host: stop it and start another;
    /// - not wanted and a host: stop it (`goodbye{remote-disabled}`) and
    ///   forget the session state;
    /// - a host: theme, profiles and tree, when due.
    ///
    /// With no window nothing happens: the host is stopped on the way out
    /// (`stop_remote(Quitting)`), not by this.
    pub(crate) fn sync_remote(&mut self, now: Instant) {
        if self.windows.is_empty() {
            return;
        }
        let wanted = remote_wanted(self.config.remote.enabled, self.windows.len());
        if !wanted {
            // Turning the feature off and on again tries again.
            self.remote.start_failed_url = None;
        }
        let plan = host_plan(
            wanted,
            self.remote.started_url.as_deref(),
            &self.config.remote.relay_url,
            self.remote.start_failed_url.as_deref(),
        );
        match plan {
            HostPlan::Keep => {}
            HostPlan::Stop => self.stop_remote(StopReason::Disabled),
            HostPlan::Start => self.start_remote(),
            HostPlan::Restart => {
                self.stop_remote(StopReason::Disabled);
                self.start_remote();
            }
        }
        self.flush_remote(now);
    }

    /// Creates the host (the `tokio` runtime and its threads are born here,
    /// never before). The event closure only wraps the event and hands it to
    /// the proxy. A failure (no runtime, no data directory) leaves no host and
    /// is not retried for the same URL.
    fn start_remote(&mut self) {
        let url = self.config.remote.relay_url.clone();
        let session_path = porecatu_session::path::resolve_session_path();
        let Some(data_dir) = session_path
            .as_deref()
            .and_then(porecatu_session::path::session_dir)
            .map(Path::to_path_buf)
        else {
            self.remote.start_failed_url = Some(url);
            return;
        };
        // Computed once and kept (RF-25.5): it is a call to the OS.
        let host_name = self
            .remote
            .host_name
            .get_or_insert_with(porecatu_remote::machine_name)
            .clone();
        let proxy = self.proxy.clone();
        let params = RemoteParams {
            relay_url: url.clone(),
            data_dir,
            host_name,
            porecatu_version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        match RemoteHost::start(params, move |event| {
            let _ = proxy.send_event(Wakeup::Remote(Box::new(event)));
        }) {
            Ok(host) => {
                self.remote.host_started(url);
                self.remote.host = Some(host);
            }
            Err(_) => self.remote.start_failed_url = Some(url),
        }
        self.redraw_settings_and_windows();
    }

    /// Stops the host, if there is one: the phones are told why, the runtime
    /// dies without waiting for the network, and nothing of the session
    /// survives. Idempotent.
    pub(crate) fn stop_remote(&mut self, reason: StopReason) {
        let Some(host) = self.remote.host.take() else {
            return;
        };
        host.stop(reason);
        self.remote.host_stopped();
        if reason == StopReason::Disabled {
            self.redraw_settings_and_windows();
        }
    }

    /// The settings screen and every window show the state of the remote
    /// access (the connected-devices indicator): ask for a frame of all of them. The
    /// screen's "Estado" line is part of its content, so it is told the new
    /// state first (it rebuilds only when the state is not the one it has).
    fn redraw_settings_and_windows(&mut self) {
        self.push_remote_to_settings();
        self.sync_connected_devices();
        self.for_each_surface(|surface| surface.request_redraw());
    }

    /// Hands every window the phones with an open session, from
    /// `RemoteState::clients` (the one truth): what the connected-devices
    /// button, its tooltip, its accessibility node and its popover are made of
    /// (PRD-025 RF-25.12, ADR-0108 section 4). A window only asks for a frame
    /// when its copy changed, so this is cheap enough to run on every turn of
    /// the event loop -- which is what makes a window born while a phone is
    /// connected, and a host that went away without an event, both right.
    pub(crate) fn sync_connected_devices(&mut self) {
        self.sync_remote_viewers();
        // Runs on every turn of the loop: compare first, without allocating,
        // and build the list only when some window holds another one.
        let clients = &self.remote.clients;
        let same = |list: &[DeviceEntry]| {
            list.len() == clients.len()
                && list
                    .iter()
                    .zip(clients)
                    .all(|(entry, (client, connected))| {
                        entry.client == *client
                            && entry.name == connected.name
                            && entry.platform == connected.platform
                            && entry.connected_at == connected.connected_at
                    })
        };
        if self
            .windows
            .values()
            .all(|state| same(&state.connected_devices))
        {
            return;
        }
        let devices: Vec<DeviceEntry> = self
            .remote
            .clients
            .iter()
            .map(|(client, connected)| DeviceEntry {
                client: *client,
                name: connected.name.clone(),
                platform: connected.platform,
                connected_at: connected.connected_at,
            })
            .collect();
        for state in self.windows.values_mut() {
            if state.set_connected_devices(devices.clone()) {
                state.window.request_redraw();
            }
        }
    }

    /// Hands every window the names of the phones viewing each of its tabs, from
    /// `RemoteState::viewing` and `clients` through the protocol ids (ADR-0112
    /// section 1): what the indicator of the tab bar, its tooltip and its
    /// accessibility node are made of. A window with no viewed tab gets the
    /// empty map. Runs with [`Self::sync_connected_devices`], so on every turn
    /// of the event loop: it compares first, without allocating, and builds
    /// the projection only when some window holds another one. A tab that
    /// moved to another window by `App::transfer` shows up in the new one,
    /// because the protocol id does not change and the projection is redone.
    fn sync_remote_viewers(&mut self) {
        let remote = &self.remote;
        if self.windows.iter().all(|(window, state)| {
            viewers_current(
                &state.remote_viewers,
                *window,
                &remote.clients,
                &remote.viewing,
                &remote.ids,
            )
        }) {
            return;
        }
        let mut by_window = viewers_by_window(&remote.clients, &remote.viewing, &remote.ids);
        for (window, state) in &mut self.windows {
            let viewers = by_window.remove(window).unwrap_or_default();
            if state.set_remote_viewers(viewers) {
                state.window.request_redraw();
            }
        }
    }

    /// `remote.disconnect_all` and the "Disconnect all" of the popover
    /// (RF-25.13): every session ends with `goodbye{normal}`; the devices stay
    /// paired. With no host there is nothing to disconnect.
    pub(crate) fn remote_disconnect_all(&mut self) {
        if let Some(host) = &self.remote.host {
            host.send(Command::DisconnectAll);
        }
    }

    /// The "Disconnect" of one device in the popover (RF-25.13): its session
    /// ends with `goodbye{normal}`; the device stays paired.
    pub(crate) fn remote_disconnect(&mut self, client: ClientId) {
        if let Some(host) = &self.remote.host {
            host.send(Command::Disconnect(client));
        }
    }

    /// What the settings screen shows of the remote access, from the state of the
    /// process: the relay status and the list of paired devices, with the ones
    /// that have a session open marked (ADR-0108 section 1). Each one rebuilds
    /// the content only when it differs from what the screen has.
    pub(crate) fn push_remote_to_settings(&mut self) {
        let Some(settings) = &mut self.settings else {
            return;
        };
        settings.set_remote_status(self.remote.status);
        let client_keys: Vec<[u8; 32]> = self
            .remote
            .clients
            .values()
            .map(|client| client.device_key)
            .collect();
        settings.set_remote_devices(device_rows(&self.remote.devices, &client_keys));
    }

    // --- pairing and the paired devices (ADR-0106 sections 2, 3 and 6, ADR-0108) -----

    /// Does what the settings screen asked of the host since the last turn:
    /// show or cancel the QR, revoke a device, recreate the identity, answer a
    /// pairing request. They are immediate gestures, not pending edits: the
    /// command goes by `try_send` and the host answers with the new state. With
    /// no host (the feature is off) there is nothing to ask and the requests are
    /// dropped.
    pub(crate) fn service_remote_requests(&mut self) {
        let requests = match &mut self.settings {
            Some(settings) => settings.take_remote_requests(),
            None => return,
        };
        for request in requests {
            let Some(host) = &self.remote.host else {
                continue;
            };
            match request {
                RemoteRequest::ShowPairing => host.send(Command::ShowPairing),
                RemoteRequest::CancelPairing => {
                    host.send(Command::CancelPairing);
                    // The link carries the token: nothing of it stays behind.
                    self.remote.pairing_uri = None;
                }
                RemoteRequest::Revoke(key) => host.send(Command::Revoke { key }),
                RemoteRequest::RecreateIdentity => host.send(Command::RecreateIdentity),
                RemoteRequest::AnswerPairing { id, answer } => {
                    host.send(Command::AnswerPairing { id, answer });
                    self.remote.pending_pairing = None;
                }
            }
        }
    }

    /// `remote.pair` (ADR-0107 section 7): the settings screen on "Acesso
    /// remoto" and, with the relay connected, a QR code right away. Not
    /// connected, the screen opens with the button disabled and its reason.
    pub(crate) fn remote_pair_gesture(&mut self, event_loop: &ActiveEventLoop, origin: WindowId) {
        self.open_settings_at(event_loop, origin, crate::settings::Group::RemoteAccess);
        if self.remote.status == crate::remote::RemoteStatus::Connected
            && let Some(host) = &self.remote.host
        {
            host.send(Command::ShowPairing);
        }
    }

    // --- the relay token and the OS vault (ADR-0108 section 1) --------------
    //
    // The vault blocks (D-Bus, Keychain, the Credential Manager): it is only
    // ever asked from a short thread, and the answer comes back as
    // `Wakeup::RemoteVault`, data keyed by nothing but its kind -- there is one
    // token. What is typed never leaves the masked field except by movement
    // into the thread (a `Secret`, zeroed when it drops) and never is logged.

    /// Asks the vault, in a thread, whether it keeps a relay token.
    pub(crate) fn start_vault_probe(&self) {
        let proxy = self.proxy.clone();
        let spawned = std::thread::Builder::new()
            .name("porecatu-remote-vault-probe".into())
            .spawn(move || {
                let result = porecatu_remote::operator_token_present();
                let _ =
                    proxy.send_event(Wakeup::RemoteVault(Box::new(VaultReport::Presence(result))));
            });
        if spawned.is_err() {
            // No thread: the screen would wait for ever. The same answer as a
            // vault that did not respond.
            let _ = self
                .proxy
                .send_event(Wakeup::RemoteVault(Box::new(VaultReport::Presence(Err(
                    VaultError::Failed,
                )))));
        }
    }

    /// Writes (or deletes) the relay token in the vault, in a thread. The
    /// `Secret` moves into it and is zeroed when the thread is done.
    pub(crate) fn start_token_write(&self, write: TokenWrite) {
        let proxy = self.proxy.clone();
        let deleted = matches!(write, TokenWrite::Delete);
        let spawned = std::thread::Builder::new()
            .name("porecatu-remote-vault-write".into())
            .spawn(move || {
                let result = match &write {
                    TokenWrite::Store(secret) => {
                        porecatu_remote::store_operator_token(secret.expose())
                    }
                    // An empty string deletes (`store_operator_token`).
                    TokenWrite::Delete => porecatu_remote::store_operator_token(""),
                };
                drop(write);
                let _ = proxy.send_event(Wakeup::RemoteVault(Box::new(VaultReport::Written {
                    deleted,
                    result,
                })));
            });
        if spawned.is_err() {
            let _ = self
                .proxy
                .send_event(Wakeup::RemoteVault(Box::new(VaultReport::Written {
                    deleted,
                    result: Err(VaultError::Failed),
                })));
        }
    }

    /// What a vault thread found or did, kept in the settings screen (which may
    /// have been closed meanwhile). A token written while the host is stopped
    /// at a token failure (or still connecting, when the same Save turned the
    /// feature on) restarts it: the host reads the token only when it starts.
    pub(crate) fn apply_remote_vault(&mut self, report: VaultReport, now: Instant) {
        match report {
            VaultReport::Presence(result) => {
                if let Some(settings) = &mut self.settings {
                    settings.set_token_presence(result);
                    settings.request_redraw();
                }
            }
            VaultReport::Written { deleted, result } => {
                let ok = result.is_ok();
                if let Some(settings) = &mut self.settings {
                    settings.token_written(deleted, ok);
                    settings.request_redraw();
                }
                if !ok {
                    self.notify_focused(
                        Severity::Warning,
                        msg::notice::remote_token_save_failed::title(&self.catalog),
                        msg::notice::remote_token_save_failed::body(&self.catalog),
                        now,
                    );
                } else if !deleted {
                    self.restart_remote_for_token();
                }
            }
        }
    }

    /// Stops a live host that cannot use the token it started without; the
    /// `sync_remote` of the next turn starts a new one, which reads the vault
    /// again. No path of its own to start a host.
    fn restart_remote_for_token(&mut self) {
        if self.remote.host.is_some() && token_change_restarts_host(self.remote.status) {
            self.stop_remote(StopReason::Disabled);
        }
    }

    /// Sends the theme and the profiles when they changed (ADR-0105 section
    /// 6) and the tree when it is due (section 2). The tree is only built with
    /// a phone connected; without one this is a flag check.
    fn flush_remote(&mut self, now: Instant) {
        // Always, host or not: the lists must not pile up while the feature
        // is off.
        self.drain_remote_resizes();
        if self.remote.host.is_none() {
            return;
        }
        // ADR-0107 section 3: a `confirm-request` unanswered for 60 s ends
        // `confirm-timeout`.
        self.expire_remote_confirmations(now);
        if self.remote.appearance_dirty() {
            let theme = protocol_theme(&self.term_pal, &self.pal);
            let profiles = protocol_profiles(&self.config, &self.detected_profiles);
            let (theme, profiles) = self.remote.appearance_built(theme, profiles);
            if let Some(host) = &self.remote.host {
                if let Some(theme) = theme {
                    host.send(Command::SetTheme(theme));
                }
                if let Some(profiles) = profiles {
                    host.send(Command::SetProfiles(profiles));
                }
            }
        }
        if self.remote.tree_due(now) == TreeDue::Now {
            let (tree, seen) = self.build_remote_workspace();
            self.remote.ids.retain_seen(&seen);
            self.remote.forget_gone_panes();
            if let Some(tree) = self.remote.tree_built(now, tree)
                && let Some(host) = &self.remote.host
            {
                host.send(Command::SetWorkspace(tree));
            }
        }
        // After the tree: the host drops a frame of a pane it was not told
        // about.
        self.mirror_remote(now);
    }

    /// A pane was resized on the desktop (window, scale, font zoom, split,
    /// divider, reload): its screen changed, so a subscribed one is dirty for
    /// the remote (ADR-0105 sections 3 and 4). `Terminal::resize` is called
    /// from `WindowState`, which does not know the remote access: it leaves
    /// the pane in `remote_resized` and this marks them, from one place.
    /// Nothing here asks for a redraw.
    fn drain_remote_resizes(&mut self) {
        for (window, state) in &mut self.windows {
            for (tab, pane) in state.remote_resized.drain(..) {
                self.remote.mark_pane_dirty_at(*window, tab, pane);
            }
        }
    }

    /// The mirror pass (ADR-0105 sections 3 and 5): at most every 33 ms, and
    /// only with a dirty subscribed pane or a scrollback request waiting. It
    /// never calls `request_redraw`: a pane in an invisible tab, subscribed
    /// by a phone, is read here and makes no GPU frame.
    ///
    /// - every dirty and still subscribed pane becomes a `Command::Frame`;
    /// - **one** scrollback request is answered, the oldest.
    fn mirror_remote(&mut self, now: Instant) {
        if self.remote.host.is_none() || self.remote.mirror_due(now) != MirrorDue::Now {
            return;
        }
        let dirty = self.remote.begin_mirror_pass(now);
        let request = self.remote.next_scrollback();
        let remote = &mut self.remote;
        let Some(host) = remote.host.as_ref() else {
            return;
        };
        for pane in dirty {
            let Some((window, tab, pane_id)) = remote.ids.pane_of(pane) else {
                continue;
            };
            let Some(state) = self.windows.get(&window) else {
                continue;
            };
            let frame = mirror_frame(state, tab, pane_id, &mut remote.mirror_buf);
            host.send(Command::Frame {
                pane,
                frame: Box::new(frame),
            });
        }
        if let Some((client, request)) = request {
            let message =
                scrollback_answer(&self.windows, &remote.ids, &request, &mut remote.mirror_buf);
            host.send(Command::Send { client, message });
        }
    }

    /// Input from a phone (ADR-0105 section 7): through the key encoder, to
    /// the PTY, and nothing else -- no keymap, no capture mode of the window,
    /// no task manager: an open dialog or the manager on the desktop does not
    /// swallow it. Nothing of the input goes to a log.
    fn apply_remote_input(&mut self, client: ClientId, input: RemoteInput) {
        let pane_id = input_pane(&input);
        let scroll_on_input = self.config.terminal.scrollback.scroll_on_input;
        let Some(host) = &self.remote.host else {
            return;
        };
        let refuse = |code| {
            host.send(Command::Send {
                client,
                message: Message::Error(ErrorMessage {
                    code,
                    id: None,
                    pane: Some(pane_id),
                }),
            });
        };
        // The pane may have closed since the phone saw it.
        let Some((window, tab, pane)) = self.remote.ids.pane_of(pane_id) else {
            return refuse(ErrorCode::UnknownPane);
        };
        let Some(state) = self.windows.get(&window) else {
            return refuse(ErrorCode::UnknownPane);
        };
        let (wire_state, _) = pane_state(&pane_signals(state, tab, pane));
        let runtime = state.panes.get(&(tab, pane));
        match input_answer(wire_state, &input, || runtime.map(|rt| rt.terminal.modes())) {
            InputAnswer::NotRunning => refuse(ErrorCode::PaneNotRunning),
            InputAnswer::Bytes(bytes) => {
                let Some(runtime) = runtime else {
                    return;
                };
                if bytes.is_empty() {
                    return;
                }
                runtime.terminal.write(bytes);
                if scroll_on_input {
                    runtime.terminal.scroll(TermScroll::Bottom);
                    // The desktop view of that pane moved: redraw it, if it
                    // is on screen.
                    if state.workspace.active_tab() == Some(tab) {
                        state.window.request_redraw();
                    }
                }
            }
        }
    }

    /// The tree of every window, in the order of focus (most recent first),
    /// with the protocol ids (ADR-0105 sections 1 and 2).
    fn build_remote_workspace(&mut self) -> (WireWorkspace, SeenIds<WindowId>) {
        let order = self.windows_by_recent_focus();
        let states: Vec<(WindowId, &WindowState)> = order
            .iter()
            .filter_map(|id| self.windows.get(id).map(|state| (*id, state)))
            .collect();
        let tab_closures: Vec<_> = states
            .iter()
            .map(|&(_, state)| move |tab: TabId| tab_facts(state, tab))
            .collect();
        let pane_closures: Vec<_> = states
            .iter()
            .map(|&(_, state)| {
                move |tab: TabId, pane: PaneId| assemble_pane_facts(pane_inputs(state, tab, pane))
            })
            .collect();
        let inputs: Vec<WindowInput<'_, WindowId>> = states
            .iter()
            .zip(&tab_closures)
            .zip(&pane_closures)
            .map(|(((id, state), tab_facts), pane_facts)| WindowInput {
                id: *id,
                title: state.window_title().to_owned(),
                workspace: &state.workspace,
                tab_facts,
                pane_facts,
            })
            .collect();
        build_workspace(&inputs, &mut self.remote.ids)
    }

    /// An event from the host (PRD-025, ADR-0104 section 3). One that arrives
    /// after the host was stopped is stale and dropped.
    pub(crate) fn apply_remote_event(
        &mut self,
        event: RemoteEvent,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) {
        if self.remote.host.is_none() {
            return;
        }
        match self.remote.absorb(event, now) {
            Effect::None => {}
            Effect::StatusChanged => self.redraw_settings_and_windows(),
            Effect::Quarantined(path) => self.notify_focused(
                Severity::Warning,
                msg::notice::remote_devices_quarantined::title(&self.catalog),
                msg::notice::remote_devices_quarantined::body(
                    &self.catalog,
                    path.to_string_lossy(),
                ),
                now,
            ),
            Effect::ClientConnected { name } => {
                // ADR-0108 section 5: the user is told a phone got in. The
                // tree goes out at the end of this turn (`absorb` forced it).
                self.notify_focused(
                    Severity::Info,
                    msg::notice::remote_device_connected::title(&self.catalog),
                    msg::notice::remote_device_connected::body(&self.catalog, &name),
                    now,
                );
                self.redraw_settings_and_windows();
            }
            Effect::ClientDisconnected | Effect::DevicesChanged => {
                self.redraw_settings_and_windows();
            }
            Effect::ViewingChanged => self.sync_remote_viewers(),
            Effect::PairingUri => self.show_pairing_qr(now),
            Effect::PairingRequested { id } => self.show_pairing_request(id, now),
            Effect::PairingClosed { id } => {
                if let Some(settings) = &mut self.settings {
                    settings.pairing_closed(id);
                }
            }
            Effect::Unhandled(event) => self.apply_remote_request(event, event_loop, now),
        }
    }

    /// The host sent the QR: it goes to the settings screen, which shows it in
    /// place of the button with the countdown. The link carries the token, so
    /// the state keeps no copy. With no screen to show it, the token is
    /// cancelled.
    fn show_pairing_qr(&mut self, now: Instant) {
        let Some((uri, expires_at)) = self.remote.pairing_uri.take() else {
            return;
        };
        match &mut self.settings {
            Some(settings) => {
                settings.show_pairing_qr(uri, expires_at, now);
                settings.request_redraw();
            }
            None => {
                if let Some(host) = &self.remote.host {
                    host.send(Command::CancelPairing);
                }
            }
        }
    }

    /// A phone presented a good token (RF-25.7): the dialog opens in the settings
    /// screen, which is where the QR was. With the screen closed there is no one
    /// to ask, and the request is declined at once.
    fn show_pairing_request(&mut self, id: PairingId, now: Instant) {
        let Some(pending) = self.remote.pending_pairing.take() else {
            return;
        };
        match &mut self.settings {
            Some(settings) => {
                settings.pairing_requested(pending, now);
                settings.request_redraw();
            }
            None => {
                if let Some(host) = &self.remote.host {
                    host.send(Command::AnswerPairing {
                        id,
                        answer: PairingAnswer::Decline,
                    });
                }
            }
        }
    }

    /// What the phone asks for. The input goes to the PTY; the actions and the
    /// answers to a `confirm-request` are `apply_remote_action` and
    /// `apply_remote_confirm`. The scrollback is queued by `absorb` and
    /// answered by the mirror pass.
    fn apply_remote_request(
        &mut self,
        event: RemoteEvent,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) {
        match event {
            RemoteEvent::Input { client, input } => self.apply_remote_input(client, input),
            // ADR-0111 section 3: no one check of ADR-0095 around these. A
            // remote action never activates a tab, so a task manager open on
            // a window stays open (the focus ladder of a removed active tab
            // runs underneath it).
            RemoteEvent::Action { client, action } => {
                self.apply_remote_action(client, action, event_loop, now);
            }
            RemoteEvent::Confirm { client, confirm } => {
                self.apply_remote_confirm(client, confirm, event_loop, now);
            }
            // ADR-0113 section 1: read on demand, on the main thread, as the
            // popover reads it; a directory that cannot be read is an empty
            // list (`list_named` already says so).
            RemoteEvent::ListSessions { client, id } => {
                let entries = porecatu_session::named::list_named();
                let list = named_sessions(&entries, &mut self.remote.ids);
                let mode = sessions_mode(self.config.session.restore_named_in_current_window);
                self.remote_send(client, Message::Sessions(Sessions { id, mode, list }));
            }
            // `absorb` handles the rest; they never come back as unhandled.
            _ => {}
        }
    }

    // --- actions on tabs and panes (PRD-025 RF-25.23 to RF-25.27) ----------
    //
    // ADR-0107 section 1, with the rule of ADR-0111:
    // each action calls the **same function** the local gesture calls, with
    // the target spelled out, and **no remote action activates a tab**
    // (`new-tab`, `collapse-group`, `move-*-to-window` and `start-pane` ask
    // for "do not activate"). Only the removal of the active tab moves the
    // active one, by the focus ladder. What never happens is a touch on the
    // window of the OS -- no focus, no raise, no restore from the taskbar --
    // nor a change of the focused pane, nor the task manager of a window going
    // back to the terminal. What the desktop is in the middle of is `busy`.

    /// Sends a message to one phone.
    fn remote_send(&self, client: ClientId, message: Message) {
        if let Some(host) = &self.remote.host {
            host.send(Command::Send { client, message });
        }
    }

    /// The window is in a capture mode, or a drag between windows is going on
    /// (ADR-0107 section 4).
    fn remote_window_busy(&self, window: WindowId) -> bool {
        self.cross_drag.is_some()
            || self
                .windows
                .get(&window)
                .is_some_and(WindowState::is_capturing)
    }

    /// `restore-session` (ADR-0113 sections 3 and 4): the checks of
    /// `plan_restore_session` with the key read **now**, then the restore of the
    /// popover with explicit parameters -- a new window born without focus, or
    /// an append that activates, scrolls and closes nothing. A file that cannot
    /// be read is `session-unreadable` for the phone; the desktop shows nothing.
    fn remote_restore_session(
        &mut self,
        event_loop: &ActiveEventLoop,
        session: u64,
        window: Option<u64>,
    ) -> ActionFlow {
        let plan = plan_restore_session(
            session,
            window,
            self.config.session.restore_named_in_current_window,
            |id| self.remote.ids.session_of(id).map(Path::to_path_buf),
            Path::exists,
            |id| {
                self.remote
                    .ids
                    .window_of(id)
                    .filter(|window| self.windows.contains_key(window))
            },
            |window| self.remote_window_busy(window),
            self.cross_drag.is_some(),
        );
        let plan = match plan {
            Ok(plan) => plan,
            Err(code) => return ActionFlow::Done(Err(code)),
        };
        let append_to = match plan.target {
            RestoreTarget::NewWindow => None,
            RestoreTarget::Append(window) => Some(window),
        };
        let restored = self.restore_named_session_remote(event_loop, &plan.file, append_to);
        ActionFlow::Done(restore_outcome(restored))
    }

    /// The tab a protocol id names, if it is still a tab of a window.
    fn resolve_remote_tab(&self, id: u64) -> Option<(WindowId, TabId)> {
        self.remote.ids.tab_of(id).filter(|(window, tab)| {
            self.windows
                .get(window)
                .is_some_and(|state| state.workspace.tab(*tab).is_some())
        })
    }

    /// The pane a protocol id names, if it is still a pane of a tab.
    fn resolve_remote_pane(&self, id: u64) -> Option<(WindowId, TabId, PaneId)> {
        self.remote.ids.pane_of(id).filter(|(window, tab, pane)| {
            self.windows.get(window).is_some_and(|state| {
                state
                    .workspace
                    .tab(*tab)
                    .is_some_and(|t| t.panes().pane(*pane).is_some())
            })
        })
    }

    /// Tells the phone how the action ended -- success or any error, always
    /// as an `action-result` with the id of the request (spec 03 section 7).
    /// Success marks the tree: the workspace changed.
    fn finish_remote_action(&mut self, client: ClientId, id: u64, flow: ActionFlow) {
        let ActionFlow::Done(result) = flow else {
            return;
        };
        let (error, created) = match result {
            Ok(created) => (None, created),
            Err(code) => (Some(code), None),
        };
        if error.is_none() {
            self.remote.mark_tree_dirty();
        }
        self.remote_send(
            client,
            Message::ActionResult(ActionResult { id, error, created }),
        );
    }

    /// One `action` of a phone. Every one answers (RF-25.23).
    fn apply_remote_action(
        &mut self,
        client: ClientId,
        action: Action,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) {
        let Action { id, action } = action;
        let flow = match action {
            ActionBody::NewTab {
                window,
                group,
                profile,
            } => self.remote_new_tab(window, group, profile, now),
            ActionBody::CloseTab { tab } => {
                self.remote_close(client, id, ConfirmTarget::Tab(tab), false, event_loop, now)
            }
            ActionBody::ClosePane { pane } => self.remote_close(
                client,
                id,
                ConfirmTarget::Pane(pane),
                false,
                event_loop,
                now,
            ),
            ActionBody::RenameTab { tab, title } => self.remote_rename_tab(tab, title),
            ActionBody::StartPane { pane } => self.remote_start_pane(pane, now),
            ActionBody::MoveTabToGroup { tab, group } => self.remote_move_tab_to_group(tab, group),
            ActionBody::NewGroup { tabs, name, color } => {
                self.remote_new_group(&tabs, &name, color)
            }
            ActionBody::RenameGroup { group, name } => self.remote_rename_group(group, name),
            ActionBody::SetGroupColor { group, color } => self.remote_set_group_color(group, color),
            ActionBody::CollapseGroup { group, collapsed } => {
                self.remote_collapse_group(group, collapsed, now)
            }
            ActionBody::Ungroup { group } => self.remote_ungroup(group),
            ActionBody::MoveTabToWindow { tab, window } => {
                self.remote_move_tab_to_window(tab, window, event_loop, now)
            }
            ActionBody::MoveGroupToWindow { group, window } => {
                self.remote_move_group_to_window(group, window, event_loop, now)
            }
            ActionBody::RestoreSession { session, window } => {
                self.remote_restore_session(event_loop, session, window)
            }
        };
        self.finish_remote_action(client, id, flow);
    }

    /// The answer of a phone to a `confirm-request` (ADR-0107 section 3). One
    /// that is unknown, already answered or expired is ignored. `accept =
    /// false` is `cancelled`; `true` runs the action again from the target's
    /// protocol id (a target that is gone is `unknown-tab`/`unknown-pane`)
    /// and, this time, without the question.
    fn apply_remote_confirm(
        &mut self,
        client: ClientId,
        confirm: Confirm,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) {
        let Some(target) = self.remote.confirmations.answer(client, confirm.id) else {
            return;
        };
        let flow = if confirm.accept {
            self.remote_close(client, confirm.id, target, true, event_loop, now)
        } else {
            ActionFlow::Done(Err(ErrorCode::Cancelled))
        };
        self.finish_remote_action(client, confirm.id, flow);
    }

    /// The `confirm-request`s whose 60 s are up end `confirm-timeout`.
    fn expire_remote_confirmations(&mut self, now: Instant) {
        for (client, id) in self.remote.confirmations.expire(now) {
            self.remote_send(
                client,
                Message::ActionResult(ActionResult {
                    id,
                    error: Some(ErrorCode::ConfirmTimeout),
                    created: None,
                }),
            );
        }
    }

    /// `new-tab`: the creation of `tab.new` (no group), of the "+" that
    /// closes the track (`group: null`, a loose tab at the end of the track,
    /// whatever the active tab) or of `group.new_tab` (with an id, at the end
    /// of the group), the profile of the menu of profiles (none is
    /// the default; `Some("")` is the `[shell]`), the `cwd` they inherit. The
    /// tab is born **without being activated** (ADR-0111 section 1):
    /// `open_tab(.., activate = false)` still spawns its process, the active
    /// tab stays the same, and so do the title of the window and the scroll of
    /// the track (nothing scrolls to the new tab). The window of the OS is not
    /// touched.
    fn remote_new_tab(
        &mut self,
        window: u64,
        group: NewTabGroup,
        profile: Option<String>,
        now: Instant,
    ) -> ActionFlow {
        let offered = protocol_profiles(&self.config, &self.detected_profiles).list;
        let plan = match plan_new_tab(
            &self.remote.ids,
            window,
            group,
            profile.as_deref(),
            &offered,
            |window| self.windows.contains_key(&window),
            |window, group| {
                self.windows
                    .get(&window)
                    .is_some_and(|state| state.workspace.group(group).is_some())
            },
        ) {
            Ok(plan) => plan,
            Err(code) => return ActionFlow::Done(Err(code)),
        };
        if self.remote_window_busy(plan.window) {
            return ActionFlow::Done(Err(ErrorCode::Busy));
        }
        let Some(state) = self.windows.get_mut(&plan.window) else {
            return ActionFlow::Done(Err(ErrorCode::UnknownWindow));
        };
        let target = match plan.place {
            NewTabPlace::ActiveGroup => NewTabTarget::ActiveGroup,
            NewTabPlace::Ungrouped => NewTabTarget::Ungrouped,
            NewTabPlace::Group(group) => NewTabTarget::Group(group),
        };
        let cwd = state.new_tab_cwd(target, &self.startup_directory);
        let tab = state.open_tab(
            self.cell_metrics,
            &self.proxy,
            cwd,
            now,
            target,
            &self.style,
            &self.term_params,
            &self.config,
            &self.config.project_file,
            &self.startup_directory,
            false,
            plan.profile.as_deref(),
        );
        // Unlike the local `tab.new`, the title of the window and the scroll
        // of the track follow the active tab, which did not change.
        state.window.request_redraw();
        match tab {
            Some(tab) => ActionFlow::Done(Ok(Some(self.remote.ids.tab(plan.window, tab)))),
            // The spawn failed: the desktop got its own warning, and there is
            // no tab. The spec has no code for it; this one says the request
            // could not be carried out as asked.
            None => ActionFlow::Done(Err(ErrorCode::InvalidArgument)),
        }
    }

    /// `close-tab` and `close-pane`, new and confirmed (`confirmed` skips the
    /// question). A close the local rule would ask about becomes a
    /// `confirm-request` to the phone that asked, registered under
    /// `(client, id)` with the **protocol id** of the target.
    fn remote_close(
        &mut self,
        client: ClientId,
        id: u64,
        target: ConfirmTarget,
        confirmed: bool,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) -> ActionFlow {
        let outcome = match target {
            ConfirmTarget::Tab(tab) => self.remote_close_tab(tab, confirmed, event_loop, now),
            ConfirmTarget::Pane(pane) => self.remote_close_pane(pane, confirmed, event_loop, now),
        };
        match outcome {
            Err(code) => ActionFlow::Done(Err(code)),
            Ok(CloseOutcome::Closed) => ActionFlow::Done(Ok(None)),
            Ok(CloseOutcome::Ask { kind, processes }) => {
                if !self.remote.confirmations.insert(client, id, target, now) {
                    // An id still waiting for its answer cannot be asked twice.
                    return ActionFlow::Done(Err(ErrorCode::InvalidArgument));
                }
                self.remote_send(
                    client,
                    Message::ConfirmRequest(ConfirmRequest {
                        id,
                        kind,
                        processes,
                    }),
                );
                ActionFlow::Asked
            }
        }
    }

    fn remote_close_tab(
        &mut self,
        tab: u64,
        confirmed: bool,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) -> Result<CloseOutcome, ErrorCode> {
        let (window, tab) = self.resolve_remote_tab(tab).ok_or(ErrorCode::UnknownTab)?;
        if self.remote_window_busy(window) {
            return Err(ErrorCode::Busy);
        }
        self.close_resolved_tab(window, tab, confirmed, event_loop, now)
    }

    fn remote_close_pane(
        &mut self,
        pane: u64,
        confirmed: bool,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) -> Result<CloseOutcome, ErrorCode> {
        let (window, tab, pane) = self
            .resolve_remote_pane(pane)
            .ok_or(ErrorCode::UnknownPane)?;
        if self.remote_window_busy(window) {
            return Err(ErrorCode::Busy);
        }
        let confirm_close_with_process = self.config.general.confirm_close_with_process;
        let state = self.windows.get(&window).ok_or(ErrorCode::UnknownPane)?;
        let model = state.workspace.tab(tab).ok_or(ErrorCode::UnknownPane)?;
        let is_last = model.panes().leaves_in_order().len() <= 1;
        let needs_confirmation =
            !is_last && state.pane_close_needs_confirmation(tab, pane, confirm_close_with_process);
        let live = state.live_panes(tab, &[pane]);
        let open_connection = live.iter().any(|pane| pane.alive && pane.open_connection);
        let step = pane_step(is_last, needs_confirmation, open_connection, confirmed);
        match step {
            PaneStep::CloseTab => self.close_resolved_tab(window, tab, confirmed, event_loop, now),
            PaneStep::Ask(kind) => Ok(CloseOutcome::Ask {
                kind,
                processes: confirm_summary(live).1,
            }),
            PaneStep::Close => {
                if let Some(state) = self.windows.get_mut(&window) {
                    state.close_pane_unconditionally(tab, pane, self.cell_metrics, &self.style);
                    state.window.request_redraw();
                }
                Ok(CloseOutcome::Closed)
            }
        }
    }

    /// The close of a tab that was resolved and is not busy: the last-tab
    /// refusal, then the decision the desktop's `tab.close` takes
    /// (`tab_close_ask`), and the closing the dialog would have done.
    fn close_resolved_tab(
        &mut self,
        window: WindowId,
        tab: TabId,
        confirmed: bool,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) -> Result<CloseOutcome, ErrorCode> {
        let confirm_close_with_process = self.config.general.confirm_close_with_process;
        let windows = self.windows.len();
        let state = self.windows.get(&window).ok_or(ErrorCode::UnknownTab)?;
        let ask = state
            .tab_close_ask(tab, confirm_close_with_process)
            .ok_or(ErrorCode::UnknownTab)?;
        let tabs_in_window = state.workspace.visual_order().count();
        let panes = state
            .workspace
            .tab(tab)
            .map(|t| t.panes().leaves_in_order())
            .unwrap_or_default();
        let live = state.live_panes(tab, &panes);
        let open_connection = live.iter().any(|pane| pane.alive && pane.open_connection);
        match close_tab_step(windows, tabs_in_window, ask, open_connection, confirmed) {
            CloseStep::Refuse(code) => Err(code),
            CloseStep::Ask(kind) => Ok(CloseOutcome::Ask {
                kind,
                processes: confirm_summary(live).1,
            }),
            CloseStep::Close => {
                // What the dialog (or the gesture with nothing to ask) ends
                // in: a pinned tab leaves the list with it (RF-21.22), and a
                // window left with no tab closes.
                if state.workspace.is_pinned(tab) {
                    self.close_pinned_tab_confirmed(window, tab, event_loop);
                } else {
                    self.close_tab_now(window, tab, event_loop);
                }
                // The focus ladder may have landed on a tab that was never
                // started: it starts, as on any activation (ADR-0037 section
                // 2). The phone did not choose it, the desktop did, by the
                // usual rule.
                if let Some(state) = self.windows.get_mut(&window) {
                    state.ensure_active_tab_started(
                        self.cell_metrics,
                        &self.proxy,
                        now,
                        &self.style,
                        &self.term_params,
                        &self.config,
                        &self.config.project_file,
                        &self.startup_directory,
                    );
                }
                Ok(CloseOutcome::Closed)
            }
        }
    }

    /// `rename-tab`: the same setter as the inline rename, with no inline
    /// rename (the window is `busy` while one is being typed). `None` clears
    /// the custom title; a blank one does too, as when typed.
    fn remote_rename_tab(&mut self, tab: u64, title: Option<String>) -> ActionFlow {
        let Some((window, tab)) = self.resolve_remote_tab(tab) else {
            return ActionFlow::Done(Err(ErrorCode::UnknownTab));
        };
        if self.remote_window_busy(window) {
            return ActionFlow::Done(Err(ErrorCode::Busy));
        }
        if let Some(state) = self.windows.get_mut(&window) {
            state.set_tab_title(tab, title.as_deref().and_then(normalized_title));
            state.window.request_redraw();
        }
        ActionFlow::Done(Ok(None))
    }

    /// `start-pane`: a `NotStarted` pane starts (the tab is started **without
    /// being activated**), and a dormant SSH pane connects by the trigger of
    /// the click (`connect_dormant_pane`). Questions of the backend open on
    /// the desktop, by the usual surfaces; meanwhile the pane is
    /// `awaiting-desktop` in the tree.
    fn remote_start_pane(&mut self, pane: u64, now: Instant) -> ActionFlow {
        let Some((window, tab, pane)) = self.resolve_remote_pane(pane) else {
            return ActionFlow::Done(Err(ErrorCode::UnknownPane));
        };
        if self.remote_window_busy(window) {
            return ActionFlow::Done(Err(ErrorCode::Busy));
        }
        let Some(state) = self.windows.get_mut(&window) else {
            return ActionFlow::Done(Err(ErrorCode::UnknownPane));
        };
        let (wire_state, _) = pane_state(&pane_signals(state, tab, pane));
        if let Err(code) = start_step(wire_state) {
            return ActionFlow::Done(Err(code));
        }
        // Starting a pane from the phone does not activate its tab (ADR-0111
        // section 1): the tab is started by id, by the rule of ADR-0037, and
        // the active tab, the selection and the title of the window stay as
        // they are. The window of the OS is never touched.
        state.start_tab(
            tab,
            self.cell_metrics,
            &self.proxy,
            now,
            &self.style,
            &self.term_params,
            &self.config,
            &self.config.project_file,
            &self.startup_directory,
        );
        // A restored SSH pane waits for the first gesture on it, and so does
        // one whose tab was just started: this is that gesture.
        if state.is_dormant(tab, pane) {
            state.connect_dormant_pane(tab, pane, &self.config, &self.startup_directory, now);
        }
        state.window.request_redraw();
        let started = state
            .panes
            .get(&(tab, pane))
            .is_some_and(|runtime| !runtime.dormant);
        if started {
            ActionFlow::Done(Ok(None))
        } else {
            ActionFlow::Done(Err(ErrorCode::InvalidArgument))
        }
    }

    // --- actions on groups and between windows (PRD-025 RF-25.25, RF-25.27,
    // RF-25.28) ------------------------------------------------------------
    //
    // The plans (`remote/actions.rs`) resolve the ids and say `busy` for every
    // window involved; what follows calls the function the local gesture
    // calls, on the explicit target, but the activation the gesture does is
    // left out (an expand, a move: ADR-0111 section 1); collapsing the group
    // of the active tab still moves it, by the focus ladder. The window of the
    // OS and the focused pane are never touched.

    /// The group exists in that window and is explicit (the implicit runs
    /// have no protocol id).
    fn has_explicit_group(&self, window: WindowId, group: GroupId) -> bool {
        self.windows.get(&window).is_some_and(|state| {
            state
                .workspace
                .group(group)
                .is_some_and(porecatu_core::Group::is_explicit)
        })
    }

    /// `move-tab-to-group`: the item of the popover "Move to group"
    /// (`run_move_target`, the end of the group); `group: null` is the drop
    /// between groups (`move_tab_to_run`) in the slot right after the group
    /// the tab leaves, and a tab already outside any group is a success with
    /// nothing to do. Nothing changes in the active tab (ADR-0111 section 1).
    fn remote_move_tab_to_group(&mut self, tab: u64, group: Option<u64>) -> ActionFlow {
        let plan = match plan_move_tab_to_group(
            &self.remote.ids,
            tab,
            group,
            |window, tab| {
                self.windows
                    .get(&window)
                    .is_some_and(|state| state.workspace.tab(tab).is_some())
            },
            |window, group| self.has_explicit_group(window, group),
            |window, tab| {
                self.windows
                    .get(&window)
                    .is_some_and(|state| state.workspace.is_pinned(tab))
            },
            |window| self.remote_window_busy(window),
        ) {
            Ok(plan) => plan,
            Err(code) => return ActionFlow::Done(Err(code)),
        };
        let Some(state) = self.windows.get_mut(&plan.window) else {
            return ActionFlow::Done(Err(ErrorCode::UnknownTab));
        };
        match plan.target {
            GroupMove::Into(group) => {
                state.run_move_target(plan.tab, MoveTarget::Group(group), &self.catalog);
            }
            GroupMove::Leave => {
                if let Some(slot) = leave_group_slot(&state.workspace, plan.tab) {
                    state.move_tab_to_run(plan.tab, slot);
                }
            }
        }
        state.window.request_redraw();
        ActionFlow::Done(Ok(None))
    }

    /// `new-group`: what `group.create` does once it has the tabs -- the
    /// color of `next_auto_color` unless one is given, the animation of the
    /// reflow -- but **without** opening the group editor on the desktop and
    /// without clearing the user's selection. The group is born where the
    /// model puts it: at the first of the tabs **in the order of the bar**
    /// (which is the order of the list when the phone sends them so). The id
    /// of the group is born here and goes back in `created`.
    fn remote_new_group(&mut self, tabs: &[u64], name: &str, color: Option<u64>) -> ActionFlow {
        let plan = match plan_new_group(
            &self.remote.ids,
            tabs,
            color,
            |window, tab| {
                self.windows
                    .get(&window)
                    .is_some_and(|state| state.workspace.tab(tab).is_some())
            },
            |window, tab| {
                self.windows
                    .get(&window)
                    .is_some_and(|state| state.workspace.is_pinned(tab))
            },
            |window| self.remote_window_busy(window),
        ) {
            Ok(plan) => plan,
            Err(code) => return ActionFlow::Done(Err(code)),
        };
        let Some(state) = self.windows.get_mut(&plan.window) else {
            return ActionFlow::Done(Err(ErrorCode::UnknownTab));
        };
        let color = plan
            .color
            .unwrap_or_else(|| state.workspace.next_auto_color());
        let created = state.create_group(&plan.tabs, name, color, self.gpu.as_mut(), &self.style);
        state.window.request_redraw();
        match created {
            Some(group) => ActionFlow::Done(Ok(Some(self.remote.ids.group(plan.window, group)))),
            // The model refused (nothing groupable is left): not reachable
            // after the plan, but the phone still gets an answer.
            None => ActionFlow::Done(Err(ErrorCode::InvalidArgument)),
        }
    }

    /// The target of an action on one group: the window and the group, or
    /// the error to answer.
    fn plan_remote_group(&self, group: u64) -> Result<(WindowId, GroupId), ErrorCode> {
        plan_group_edit(
            &self.remote.ids,
            group,
            |window, group| self.has_explicit_group(window, group),
            |window| self.remote_window_busy(window),
        )
    }

    /// `rename-group`: the rename the editor does on Enter.
    fn remote_rename_group(&mut self, group: u64, name: String) -> ActionFlow {
        let (window, group) = match self.plan_remote_group(group) {
            Ok(target) => target,
            Err(code) => return ActionFlow::Done(Err(code)),
        };
        if let Some(state) = self.windows.get_mut(&window) {
            state.rename_group_now(group, name);
            state.window.request_redraw();
        }
        ActionFlow::Done(Ok(None))
    }

    /// `set-group-color`: the color the editor sets on Enter or on a click
    /// on a swatch. An index outside the palette is `invalid-argument`, after
    /// `unknown-group` and `busy`.
    fn remote_set_group_color(&mut self, group: u64, color: u64) -> ActionFlow {
        let (window, group) = match self.plan_remote_group(group) {
            Ok(target) => target,
            Err(code) => return ActionFlow::Done(Err(code)),
        };
        let color = match parse_group_color(color) {
            Ok(color) => color,
            Err(code) => return ActionFlow::Done(Err(code)),
        };
        if let Some(state) = self.windows.get_mut(&window) {
            state.set_group_color_now(group, color);
            state.window.request_redraw();
        }
        ActionFlow::Done(Ok(None))
    }

    /// `collapse-group`: what the click on the pill does
    /// (`set_group_collapsed`), both ways, but expanding does **not** change
    /// the active tab (ADR-0111 section 1): the group expands in place
    /// (`Workspace::expand_group_in_place`), with the reflow animation and no
    /// scroll of the track, and no tab is activated, no indicator cleared, no
    /// group MRU changed. Collapsing the group of the active tab moves the
    /// focus by the ladder (RF-2.14), and the tab it lands on starts if it
    /// never did, like any activation.
    fn remote_collapse_group(&mut self, group: u64, collapsed: bool, now: Instant) -> ActionFlow {
        let (window, group) = match self.plan_remote_group(group) {
            Ok(target) => target,
            Err(code) => return ActionFlow::Done(Err(code)),
        };
        let Some(state) = self.windows.get_mut(&window) else {
            return ActionFlow::Done(Err(ErrorCode::UnknownGroup));
        };
        let before = state.workspace.active_tab();
        // Expanding is not a visit: `visit` is off, so the model expands in
        // place and the bar keeps its scroll; collapsing is the click on the
        // pill.
        state.set_group_collapsed(group, collapsed, false, self.gpu.as_mut(), &self.style);
        if state.workspace.active_tab() != before {
            state.ensure_active_tab_started(
                self.cell_metrics,
                &self.proxy,
                now,
                &self.style,
                &self.term_params,
                &self.config,
                &self.config.project_file,
                &self.startup_directory,
            );
        }
        state.window.request_redraw();
        ActionFlow::Done(Ok(None))
    }

    /// `ungroup`: `group.dissolve`.
    fn remote_ungroup(&mut self, group: u64) -> ActionFlow {
        let (window, group) = match self.plan_remote_group(group) {
            Ok(target) => target,
            Err(code) => return ActionFlow::Done(Err(code)),
        };
        if let Some(state) = self.windows.get_mut(&window) {
            state.dissolve_group(group);
            state.window.request_redraw();
        }
        ActionFlow::Done(Ok(None))
    }

    /// `move-tab-to-window`: **only** `App::transfer_with`, with the arrival
    /// that activates nothing and leaves the window of the OS alone
    /// (`TransferArrival::Passive`). A pinned tab goes as a pinned one
    /// (RF-22.23); the last tab of
    /// a window may go, and the empty origin closes with no dialog
    /// (ADR-0107 section 5).
    fn remote_move_tab_to_window(
        &mut self,
        tab: u64,
        window: u64,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) -> ActionFlow {
        let plan = match plan_tab_to_window(
            &self.remote.ids,
            tab,
            window,
            |window, tab| {
                self.windows
                    .get(&window)
                    .is_some_and(|state| state.workspace.tab(tab).is_some())
            },
            |window| self.windows.contains_key(&window),
            |window| self.remote_window_busy(window),
        ) {
            Ok(plan) => plan,
            Err(code) => return ActionFlow::Done(Err(code)),
        };
        let pinned = self
            .windows
            .get(&plan.from)
            .is_some_and(|state| state.workspace.is_pinned(plan.tab));
        let payload = if pinned {
            TransferPayload::Pinned(plan.tab)
        } else {
            TransferPayload::Tabs {
                ids: vec![plan.tab],
                grabbed: None,
            }
        };
        self.remote_transfer(event_loop, plan.from, payload, plan.to, now)
    }

    /// `move-group-to-window`: the whole explicit group, at the end of the
    /// track of the destination (what the popover does), **only** through
    /// `App::transfer_with`.
    fn remote_move_group_to_window(
        &mut self,
        group: u64,
        window: u64,
        event_loop: &ActiveEventLoop,
        now: Instant,
    ) -> ActionFlow {
        let plan = match plan_group_to_window(
            &self.remote.ids,
            group,
            window,
            |window, group| self.has_explicit_group(window, group),
            |window| self.windows.contains_key(&window),
            |window| self.remote_window_busy(window),
        ) {
            Ok(plan) => plan,
            Err(code) => return ActionFlow::Done(Err(code)),
        };
        self.remote_transfer(
            event_loop,
            plan.from,
            TransferPayload::Group(plan.group),
            plan.to,
            now,
        )
    }

    /// The transfer of a remote move: where the popover would put the payload
    /// (`destination_insert_at`), the arrival **not** activated (the active
    /// tab of the destination stays) and the window of the OS not touched
    /// (`TransferArrival::Passive`, ADR-0111 section 1). The ids of the
    /// protocol follow the objects (`rekey_tab_state`). A refusal of
    /// `transfer_with` (nothing it could move) is `invalid-argument`. The
    /// active tab of each end starts if it never did: at the origin, when its
    /// active tab left, the one the focus ladder landed on; at the destination
    /// the lookup is a no-op unless it had no active tab. The origin may be
    /// gone (a window left with no tab closes), which the lookup tolerates.
    fn remote_transfer(
        &mut self,
        event_loop: &ActiveEventLoop,
        from: WindowId,
        payload: TransferPayload,
        to: WindowId,
        now: Instant,
    ) -> ActionFlow {
        let at = destination_insert_at(&payload);
        let moved = self.transfer_with(
            event_loop,
            from,
            payload,
            TransferDest::Window(to, at),
            TransferArrival::Passive,
        );
        let Some(moved) = moved else {
            return ActionFlow::Done(Err(ErrorCode::InvalidArgument));
        };
        for window in [from, moved.window] {
            if let Some(state) = self.windows.get_mut(&window) {
                state.ensure_active_tab_started(
                    self.cell_metrics,
                    &self.proxy,
                    now,
                    &self.style,
                    &self.term_params,
                    &self.config,
                    &self.config.project_file,
                    &self.startup_directory,
                );
                state.window.request_redraw();
            }
        }
        ActionFlow::Done(Ok(None))
    }

    /// An information or warning notice on the window that has the focus.
    fn notify_focused(&mut self, severity: Severity, title: String, body: String, now: Instant) {
        let Some(id) = self.windows_by_recent_focus().first().copied() else {
            return;
        };
        if let Some(state) = self.windows.get_mut(&id) {
            state.warnings.push(severity, title, body, now);
            state.window.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use porecatu_remote::protocol::message::PaneState as WireState;

    use super::*;

    #[test]
    fn a_restore_that_read_the_file_has_no_created_tab() {
        assert_eq!(restore_outcome(Ok(())), Ok(None));
    }

    #[test]
    fn every_failure_to_read_is_session_unreadable() {
        for failure in [
            RestoreFailure::Corrupt(PathBuf::from("a.json")),
            RestoreFailure::NewerSchema {
                found: 9,
                supported: 1,
            },
            RestoreFailure::Unreadable,
        ] {
            assert_eq!(
                restore_outcome(Err(failure)),
                Err(ErrorCode::SessionUnreadable)
            );
        }
    }

    fn signals() -> PaneSignals {
        PaneSignals {
            model: ModelPaneState::Running,
            has_runtime: true,
            dormant: false,
            ssh: false,
            reconnecting: false,
            connected: false,
            link_dropped: false,
            prompt_pending: false,
        }
    }

    fn inputs(signals: PaneSignals) -> PaneInputs {
        PaneInputs {
            signals,
            title: "zsh".to_owned(),
            cwd: Some("/home/me".to_owned()),
            grid: Some((120, 30)),
        }
    }

    fn text_input(text: &str) -> RemoteInput {
        RemoteInput::Text(porecatu_remote::protocol::message::TextInput {
            pane: 4,
            text: text.to_owned(),
        })
    }

    #[test]
    fn only_a_running_pane_takes_input() {
        let modes = || Some(TermModes::default());
        assert_eq!(
            input_answer(WireState::Running, &text_input("ls"), modes),
            InputAnswer::Bytes(b"ls".to_vec())
        );
        for state in [
            WireState::NotStarted,
            WireState::Connecting,
            WireState::AwaitingDesktop,
            WireState::Reconnecting,
            WireState::Disconnected,
            WireState::Exited,
        ] {
            assert_eq!(
                input_answer(state, &text_input("ls"), modes),
                InputAnswer::NotRunning,
                "{state:?}"
            );
        }
    }

    #[test]
    fn the_modes_are_only_read_for_a_running_pane() {
        let input = text_input("x");
        let answer = input_answer(WireState::Exited, &input, || {
            panic!("an exited pane has no modes to read")
        });
        assert_eq!(answer, InputAnswer::NotRunning);
        // A running pane with nothing to write to is not running either.
        assert_eq!(
            input_answer(WireState::Running, &input, || None),
            InputAnswer::NotRunning
        );
    }

    #[test]
    fn a_paste_is_wrapped_by_the_modes_of_the_pane() {
        let paste = RemoteInput::Paste(porecatu_remote::protocol::message::Paste {
            pane: 1,
            text: "a b".to_owned(),
        });
        let plain = input_answer(WireState::Running, &paste, || Some(TermModes::default()));
        assert_eq!(plain, InputAnswer::Bytes(b"a b".to_vec()));
        let bracketed = input_answer(WireState::Running, &paste, || {
            Some(TermModes {
                bracketed_paste: true,
                ..TermModes::default()
            })
        });
        assert_eq!(
            bracketed,
            InputAnswer::Bytes(b"\x1b[200~a b\x1b[201~".to_vec())
        );
    }

    #[test]
    fn the_pane_of_an_input_is_the_one_it_names() {
        use porecatu_remote::protocol::message::{Key, KeyCode, NamedKey};
        assert_eq!(input_pane(&text_input("x")), 4);
        let key = RemoteInput::Key(Key {
            pane: 9,
            key: KeyCode::Named(NamedKey::Enter),
            mods: 0,
        });
        assert_eq!(input_pane(&key), 9);
    }

    #[test]
    fn a_scrollback_request_is_capped_at_five_hundred_lines() {
        let request = |offset, count| ScrollbackRequest {
            id: 1,
            pane: 1,
            offset,
            count,
        };
        assert_eq!(scrollback_window(&request(10, 20)), (10, 20));
        assert_eq!(scrollback_window(&request(0, 500)), (0, 500));
        assert_eq!(scrollback_window(&request(0, 501)), (0, 500));
        assert_eq!(scrollback_window(&request(7, u64::MAX)), (7, 500));
    }

    #[test]
    fn a_running_local_pane_carries_its_grid_title_and_directory() {
        let facts = assemble_pane_facts(inputs(signals()));
        assert_eq!(facts.state, WireState::Running);
        assert_eq!(facts.exit, None);
        assert_eq!((facts.cols, facts.rows), (120, 30));
        assert_eq!(facts.title, "zsh");
        assert_eq!(facts.cwd.as_deref(), Some("/home/me"));
    }

    #[test]
    fn a_pane_without_a_grid_gets_the_default_size() {
        let mut data = inputs(signals());
        data.grid = None;
        let facts = assemble_pane_facts(data);
        assert_eq!((facts.cols, facts.rows), DEFAULT_GRID);

        // A first snapshot that is still empty counts as no grid.
        let mut data = inputs(signals());
        data.grid = Some((0, 0));
        let facts = assemble_pane_facts(data);
        assert_eq!((facts.cols, facts.rows), DEFAULT_GRID);
    }

    #[test]
    fn an_ssh_pane_never_sends_a_directory() {
        let mut s = signals();
        s.ssh = true;
        s.connected = true;
        let facts = assemble_pane_facts(inputs(s));
        assert_eq!(facts.state, WireState::Running);
        assert_eq!(facts.cwd, None);
    }

    #[test]
    fn an_ssh_pane_that_did_not_open_the_shell_is_connecting() {
        let mut s = signals();
        s.ssh = true;
        assert_eq!(assemble_pane_facts(inputs(s)).state, WireState::Connecting);
        s.connected = true;
        assert_eq!(assemble_pane_facts(inputs(s)).state, WireState::Running);
    }

    #[test]
    fn the_exit_code_and_the_dropped_link_come_from_the_runtime_signals() {
        let mut s = signals();
        s.model = ModelPaneState::Exited { exit_code: 3 };
        let facts = assemble_pane_facts(inputs(s));
        assert_eq!(facts.state, WireState::Exited);
        assert_eq!(facts.exit, Some(3));

        // An SSH pane whose link dropped is `Disconnected`, with no code.
        s.ssh = true;
        s.link_dropped = true;
        let facts = assemble_pane_facts(inputs(s));
        assert_eq!(facts.state, WireState::Disconnected);
        assert_eq!(facts.exit, None);
    }

    #[test]
    fn a_pane_waiting_for_the_desktop_or_reconnecting_says_so() {
        let mut s = signals();
        s.ssh = true;
        s.prompt_pending = true;
        assert_eq!(
            assemble_pane_facts(inputs(s)).state,
            WireState::AwaitingDesktop
        );
        s.prompt_pending = false;
        s.reconnecting = true;
        assert_eq!(
            assemble_pane_facts(inputs(s)).state,
            WireState::Reconnecting
        );
    }

    #[test]
    fn a_pane_without_runtime_or_dormant_is_not_started() {
        let mut s = signals();
        s.has_runtime = false;
        let mut data = inputs(s);
        data.grid = None;
        assert_eq!(assemble_pane_facts(data).state, WireState::NotStarted);

        let mut s = signals();
        s.ssh = true;
        s.dormant = true;
        assert_eq!(assemble_pane_facts(inputs(s)).state, WireState::NotStarted);
    }
}
