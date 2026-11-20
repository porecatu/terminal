// SPDX-License-Identifier: GPL-3.0-or-later

//! Translates between the domain (workspace, grid, theme, keys) and the types of
//! the remote access protocol (PRD-025, ADR-0105), and keeps the state of the
//! remote access that belongs to the **process** (ADR-0104 section 4).
//!
//! [`RemoteState`] is the pure half of the app's remote access: it holds what
//! the host told the main thread (status, devices, connected clients), the map
//! of protocol ids and the bookkeeping of what was already sent. Everything
//! that needs a window (build the tree, show a notice) is `App::*` in
//! `app_remote.rs`.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::Hash;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use porecatu_core::{PaneId, TabId};
use porecatu_remote::protocol::message::{
    Bell, Message, Profiles, ScrollbackRequest, Theme, Workspace,
};
use porecatu_remote::{
    ClientId, Command, DeviceInfo, DevicePlatform, PairingId, RelayFailure, RelayStatus,
    RemoteEvent, RemoteHost,
};
use porecatu_term::GridSnapshot as TermGrid;
use winit::window::WindowId;

pub(crate) mod actions;
pub(crate) mod grid;
pub(crate) mod ids;
pub(crate) mod input;
pub(crate) mod qr;
pub(crate) mod sessions;
pub(crate) mod theme;
pub(crate) mod tree;

use actions::Confirmations;
use ids::ProtocolIds;

/// The tree is sent at most four times a second (ADR-0105 section 2).
pub(crate) const TREE_MIN_INTERVAL: Duration = Duration::from_millis(250);

/// A pane is mirrored at most thirty times a second (ADR-0105 section 3).
pub(crate) const MIRROR_MIN_INTERVAL: Duration = Duration::from_millis(33);

/// When the mirror pass is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MirrorDue {
    /// Nothing to mirror and nothing to answer: no deadline at all.
    Idle,
    /// Run the pass now.
    Now,
    /// Not before this instant (the 30 per second ceiling).
    At(Instant),
}

/// The decision behind [`RemoteState::mirror_due`]: with work (a dirty
/// subscribed pane or a scrollback request waiting) the pass is due
/// [`MIRROR_MIN_INTERVAL`] after the last one, or now if that has passed.
pub(crate) fn mirror_due(has_work: bool, last_pass: Option<Instant>, now: Instant) -> MirrorDue {
    if !has_work {
        return MirrorDue::Idle;
    }
    match last_pass {
        None => MirrorDue::Now,
        Some(last) => {
            let at = last + MIRROR_MIN_INTERVAL;
            if now >= at {
                MirrorDue::Now
            } else {
                MirrorDue::At(at)
            }
        }
    }
}

/// How the remote access stands, as the settings screen (and the indicator)
/// reads it. `Off` is "there is no host": the feature is turned off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteStatus {
    Off,
    Connecting,
    Connected,
    Failed(RelayFailure),
}

impl From<RelayStatus> for RemoteStatus {
    fn from(status: RelayStatus) -> Self {
        match status {
            RelayStatus::Connecting => Self::Connecting,
            RelayStatus::Connected => Self::Connected,
            RelayStatus::Failed(failure) => Self::Failed(failure),
        }
    }
}

/// A phone with an open session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectedClient {
    pub(crate) device_key: [u8; 32],
    pub(crate) name: String,
    pub(crate) platform: DevicePlatform,
    pub(crate) connected_at: Instant,
}

/// What the phones view, per window (ADR-0112 section 1): for each tab, the
/// names of the devices viewing it, in the order they connected
/// (`connected_at`, then the client). A phone that is not connected, a tab
/// the table does not know and a window that has no viewed tab leave no trace
/// (a window with none is absent: the caller gives it the empty map).
pub(crate) fn viewers_by_window<W: Copy + Eq + Hash>(
    clients: &BTreeMap<ClientId, ConnectedClient>,
    viewing: &BTreeMap<ClientId, u64>,
    ids: &ProtocolIds<W>,
) -> HashMap<W, HashMap<TabId, Vec<String>>> {
    let mut seen: Vec<_> = viewing
        .iter()
        .filter_map(|(client, &tab)| {
            let connected = clients.get(client)?;
            let (window, tab) = ids.tab_of(tab)?;
            Some((
                connected.connected_at,
                *client,
                window,
                tab,
                &connected.name,
            ))
        })
        .collect();
    seen.sort_by_key(|&(at, client, ..)| (at, client));
    let mut out: HashMap<W, HashMap<TabId, Vec<String>>> = HashMap::new();
    for (_, _, window, tab, name) in seen {
        out.entry(window)
            .or_default()
            .entry(tab)
            .or_default()
            .push(name.clone());
    }
    out
}

/// Whether `current` is what [`viewers_by_window`] would give `window`,
/// answered without building it: the check that runs on every turn of the
/// event loop must not allocate when nothing changed. (It walks the phones in
/// client order, which is the order they connected in; if that ever differed
/// from `connected_at` the answer would be a harmless `false`.)
pub(crate) fn viewers_current<W: Copy + Eq + Hash>(
    current: &HashMap<TabId, Vec<String>>,
    window: W,
    clients: &BTreeMap<ClientId, ConnectedClient>,
    viewing: &BTreeMap<ClientId, u64>,
    ids: &ProtocolIds<W>,
) -> bool {
    let expected = |only: Option<TabId>| {
        viewing.iter().filter_map(move |(client, &tab)| {
            let connected = clients.get(client)?;
            let (of, tab) = ids.tab_of(tab)?;
            (of == window && only.is_none_or(|wanted| wanted == tab))
                .then_some((tab, connected.name.as_str()))
        })
    };
    if current.values().map(Vec::len).sum::<usize>() != expected(None).count() {
        return false;
    }
    current.iter().all(|(&tab, names)| {
        !names.is_empty()
            && expected(Some(tab))
                .map(|(_, name)| name)
                .eq(names.iter().map(String::as_str))
    })
}

/// A pairing request waiting for the user's answer (RF-25.8). The settings
/// screen takes it (`App::apply_remote_event`) and shows the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingPairing {
    pub(crate) id: PairingId,
    pub(crate) device_name: String,
    pub(crate) platform: DevicePlatform,
    pub(crate) fingerprint: String,
}

/// What to do with the host right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostPlan {
    /// Nothing: already as wanted.
    Keep,
    Start,
    Stop,
    /// The relay URL changed under a live host: stop it and start a new one.
    Restart,
}

/// Remote access is wanted when the key is on and there is a window (the
/// host starts with the first window, not before).
pub(crate) fn remote_wanted(enabled: bool, windows: usize) -> bool {
    enabled && windows > 0
}

/// The decision of `App::sync_remote`: `running_url` is the URL the live host
/// was started with (`None` = no host), `wanted_url` the one in the config.
/// `start_failed_url` is the URL a start already failed for: it is not tried
/// again until the URL changes or the feature is turned off and on.
pub(crate) fn host_plan(
    wanted: bool,
    running_url: Option<&str>,
    wanted_url: &str,
    start_failed_url: Option<&str>,
) -> HostPlan {
    match (wanted, running_url) {
        (false, None) => HostPlan::Keep,
        (false, Some(_)) => HostPlan::Stop,
        (true, Some(running)) if running == wanted_url => HostPlan::Keep,
        (true, Some(_)) => HostPlan::Restart,
        (true, None) if start_failed_url == Some(wanted_url) => HostPlan::Keep,
        (true, None) => HostPlan::Start,
    }
}

/// When the tree is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeDue {
    /// Nothing to send (no client, or nothing changed).
    Idle,
    /// Build and compare now.
    Now,
    /// Not before this instant (the 4 per second ceiling).
    At(Instant),
}

/// What [`RemoteState::absorb`] leaves for the app to act on.
#[derive(Debug)]
pub(crate) enum Effect {
    None,
    /// The relay status (or its absence) changed.
    StatusChanged,
    /// `remote_devices.json` was unreadable and was kept at `path`.
    Quarantined(PathBuf),
    ClientConnected {
        name: String,
    },
    ClientDisconnected,
    /// A phone now views another tab, or none (`RemoteState::viewing`): the
    /// windows' projection of it is due.
    ViewingChanged,
    /// The list of paired devices changed.
    DevicesChanged,
    /// The QR URI arrived (`RemoteState::pairing_uri`; the app moves it to the
    /// settings screen and leaves none behind).
    PairingUri,
    PairingRequested {
        id: PairingId,
    },
    /// The channel of a pairing request died before the answer.
    PairingClosed {
        id: PairingId,
    },
    /// Something the state does not keep: `Action`, `Confirm`, `Input`.
    Unhandled(RemoteEvent),
}

/// The remote access of the process (ADR-0104 section 4): one per process,
/// never per window.
pub(crate) struct RemoteState {
    /// `Some` only while the feature is on.
    pub(crate) host: Option<RemoteHost>,
    /// The `relay_url` the live host was started with.
    pub(crate) started_url: Option<String>,
    /// The URL a `RemoteHost::start` failed for (the runtime could not be
    /// created): not retried until the URL changes or the feature is toggled.
    pub(crate) start_failed_url: Option<String>,
    /// The machine name, computed once, when the host first starts.
    pub(crate) host_name: Option<String>,
    pub(crate) status: RemoteStatus,
    pub(crate) devices: Vec<DeviceInfo>,
    pub(crate) clients: BTreeMap<ClientId, ConnectedClient>,
    /// The tab (protocol id) each connected phone says it views (`view-tab`,
    /// ADR-0112 section 1): process state, as the host reports it. A phone
    /// that views none has no entry. [`viewers_by_window`] projects it, per
    /// window, for the tab bar.
    pub(crate) viewing: BTreeMap<ClientId, u64>,
    /// Protocol ids of windows, groups, tabs and panes (ADR-0105 section 1).
    pub(crate) ids: ProtocolIds<WindowId>,
    /// The panes (protocol ids) some client is subscribed to.
    pub(crate) subscriptions: BTreeSet<u64>,
    /// The subscribed panes whose screen changed since the last mirror pass
    /// (protocol ids, so a transfer between windows does not touch them).
    /// Marking one never asks for a redraw (ADR-0105 section 3).
    mirror_dirty: BTreeSet<u64>,
    last_mirror: Option<Instant>,
    /// Scrollback requests, in arrival order; one is answered per pass
    /// (ADR-0105 section 5).
    scrollback_queue: VecDeque<(ClientId, ScrollbackRequest)>,
    /// The buffer every mirror snapshot and every scrollback page is read
    /// into: allocated once, reused.
    pub(crate) mirror_buf: TermGrid,
    /// The `confirm-request`s waiting for the phone that asked (ADR-0107
    /// section 3): process state, never per window; the 60 s deadline is a
    /// `WaitUntil` of the event loop.
    pub(crate) confirmations: Confirmations,
    /// The QR URI and the end of its validity (stage 17 shows it).
    pub(crate) pairing_uri: Option<(String, Instant)>,
    pub(crate) pending_pairing: Option<PendingPairing>,
    /// Something that may change the tree happened since the last build.
    tree_dirty: bool,
    /// Build now whatever the ceiling and the comparison say (a client just
    /// connected).
    tree_forced: bool,
    last_tree_built: Option<Instant>,
    last_tree: Option<Workspace>,
    /// Theme or profiles may have changed since the last send.
    appearance_dirty: bool,
    last_theme: Option<Theme>,
    last_profiles: Option<Profiles>,
}

impl RemoteState {
    pub(crate) fn new() -> Self {
        Self {
            host: None,
            started_url: None,
            start_failed_url: None,
            host_name: None,
            status: RemoteStatus::Off,
            devices: Vec::new(),
            clients: BTreeMap::new(),
            viewing: BTreeMap::new(),
            ids: ProtocolIds::new(),
            subscriptions: BTreeSet::new(),
            mirror_dirty: BTreeSet::new(),
            last_mirror: None,
            scrollback_queue: VecDeque::new(),
            mirror_buf: TermGrid::default(),
            confirmations: Confirmations::default(),
            pairing_uri: None,
            pending_pairing: None,
            tree_dirty: false,
            tree_forced: false,
            last_tree_built: None,
            last_tree: None,
            appearance_dirty: false,
            last_theme: None,
            last_profiles: None,
        }
    }

    /// A new host is born: what it has not been told yet is everything.
    pub(crate) fn host_started(&mut self, url: String) {
        self.started_url = Some(url);
        self.start_failed_url = None;
        self.status = RemoteStatus::Connecting;
        self.forget_sent();
    }

    /// The host is gone (stopped or never started): nothing of its session
    /// survives. Devices stay: they are the host's list, resent by the next
    /// one.
    pub(crate) fn host_stopped(&mut self) {
        self.host = None;
        self.started_url = None;
        self.status = RemoteStatus::Off;
        self.clients.clear();
        self.viewing.clear();
        self.subscriptions.clear();
        self.mirror_dirty.clear();
        self.last_mirror = None;
        self.scrollback_queue.clear();
        self.confirmations.clear();
        self.pairing_uri = None;
        self.pending_pairing = None;
        self.forget_sent();
    }

    /// What was sent belongs to a host that no longer exists.
    fn forget_sent(&mut self) {
        self.tree_forced = false;
        self.last_tree_built = None;
        self.last_tree = None;
        self.last_theme = None;
        self.last_profiles = None;
        self.appearance_dirty = true;
    }

    // --- the tree -----------------------------------------------------

    /// Something that may have changed the tree happened. Cheap: a flag.
    pub(crate) fn mark_tree_dirty(&mut self) {
        self.tree_dirty = true;
    }

    /// The tree is due when there is a client and either something changed or
    /// a build was forced; never more than once per [`TREE_MIN_INTERVAL`],
    /// except the forced one.
    pub(crate) fn tree_due(&self, now: Instant) -> TreeDue {
        // `clients` is empty whenever there is no host (`host_stopped`).
        if self.clients.is_empty() {
            return TreeDue::Idle;
        }
        if self.tree_forced {
            return TreeDue::Now;
        }
        if !self.tree_dirty {
            return TreeDue::Idle;
        }
        match self.last_tree_built {
            None => TreeDue::Now,
            Some(built) => {
                let at = built + TREE_MIN_INTERVAL;
                if now >= at {
                    TreeDue::Now
                } else {
                    TreeDue::At(at)
                }
            }
        }
    }

    /// The deadline the event loop must wake for, if the tree is waiting for
    /// the ceiling.
    pub(crate) fn tree_deadline(&self, now: Instant) -> Option<Instant> {
        match self.tree_due(now) {
            TreeDue::At(at) => Some(at),
            TreeDue::Idle | TreeDue::Now => None,
        }
    }

    /// A tree was built at `now`: the next one waits the ceiling. Returns it
    /// back when it differs from the last one sent (or the build was forced),
    /// `None` when there is nothing to send. `rev` is 0 in both (the host
    /// rewrites it), so plain equality ignores it.
    pub(crate) fn tree_built(&mut self, now: Instant, tree: Workspace) -> Option<Workspace> {
        let forced = std::mem::take(&mut self.tree_forced);
        self.tree_dirty = false;
        self.last_tree_built = Some(now);
        if !forced && self.last_tree.as_ref() == Some(&tree) {
            return None;
        }
        self.last_tree = Some(tree.clone());
        Some(tree)
    }

    // --- the grid mirror ------------------------------------------------

    /// The screen of the pane with protocol id `pane` may have changed. Only
    /// a subscribed pane is kept: for the others this is a lookup and
    /// nothing else (idle costs zero).
    pub(crate) fn mark_pane_dirty(&mut self, pane: u64) {
        if self.subscriptions.contains(&pane) {
            self.mirror_dirty.insert(pane);
        }
    }

    /// The same, by the internal key (a `TabDirty` wakeup, a resize). A pane
    /// that never had a protocol id was never in a tree: no one can be
    /// subscribed to it.
    pub(crate) fn mark_pane_dirty_at(&mut self, window: WindowId, tab: TabId, pane: PaneId) {
        if self.subscriptions.is_empty() {
            return;
        }
        if let Some(id) = self.ids.pane_id_of(window, tab, pane) {
            self.mark_pane_dirty(id);
        }
    }

    /// When the mirror pass is due (see [`mirror_due`]).
    pub(crate) fn mirror_due(&self, now: Instant) -> MirrorDue {
        let has_work = !self.mirror_dirty.is_empty() || !self.scrollback_queue.is_empty();
        mirror_due(has_work, self.last_mirror, now)
    }

    /// The deadline the event loop must wake for, if the pass waits for the
    /// ceiling.
    pub(crate) fn mirror_deadline(&self, now: Instant) -> Option<Instant> {
        match self.mirror_due(now) {
            MirrorDue::At(at) => Some(at),
            MirrorDue::Idle | MirrorDue::Now => None,
        }
    }

    /// A pass starts at `now`: the next one waits the ceiling. Returns the
    /// dirty panes that are still subscribed and clears the dirt.
    pub(crate) fn begin_mirror_pass(&mut self, now: Instant) -> Vec<u64> {
        self.last_mirror = Some(now);
        let dirty = std::mem::take(&mut self.mirror_dirty);
        dirty
            .into_iter()
            .filter(|pane| self.subscriptions.contains(pane))
            .collect()
    }

    /// The oldest scrollback request waiting, if any.
    pub(crate) fn next_scrollback(&mut self) -> Option<(ClientId, ScrollbackRequest)> {
        self.scrollback_queue.pop_front()
    }

    /// A pane rang the bell: every connected phone is told (RF-25.20). With
    /// no client, or for a pane that never was in a tree, nothing happens.
    pub(crate) fn send_bell(&self, window: WindowId, tab: TabId, pane: PaneId) {
        if self.clients.is_empty() {
            return;
        }
        let (Some(host), Some(id)) = (&self.host, self.ids.pane_id_of(window, tab, pane)) else {
            return;
        };
        host.send(Command::Broadcast(Message::Bell(Bell { pane: id })));
    }

    /// A tree was built: the dirt of a pane that is no longer in it goes.
    pub(crate) fn forget_gone_panes(&mut self) {
        let ids = &self.ids;
        self.mirror_dirty
            .retain(|pane| ids.pane_of(*pane).is_some());
    }

    // --- theme and profiles -------------------------------------------

    /// The theme or the profiles may have changed.
    pub(crate) fn mark_appearance_dirty(&mut self) {
        self.appearance_dirty = true;
    }

    pub(crate) fn appearance_dirty(&self) -> bool {
        self.appearance_dirty
    }

    /// Compares with what was last sent: the pieces to send now.
    pub(crate) fn appearance_built(
        &mut self,
        theme: Theme,
        profiles: Profiles,
    ) -> (Option<Theme>, Option<Profiles>) {
        self.appearance_dirty = false;
        let theme = (self.last_theme.as_ref() != Some(&theme)).then(|| {
            self.last_theme = Some(theme.clone());
            theme
        });
        let profiles = (self.last_profiles.as_ref() != Some(&profiles)).then(|| {
            self.last_profiles = Some(profiles.clone());
            profiles
        });
        (theme, profiles)
    }

    // --- the events ---------------------------------------------------

    /// Keeps what the state keeps and says what the app has to do.
    pub(crate) fn absorb(&mut self, event: RemoteEvent, now: Instant) -> Effect {
        match event {
            RemoteEvent::Status(status) => {
                let status = RemoteStatus::from(status);
                if self.status == status {
                    return Effect::None;
                }
                self.status = status;
                Effect::StatusChanged
            }
            RemoteEvent::Devices(devices) => {
                self.devices = devices;
                Effect::DevicesChanged
            }
            RemoteEvent::DevicesQuarantined { path } => Effect::Quarantined(path),
            RemoteEvent::PairingUri { uri, expires_at } => {
                self.pairing_uri = Some((uri, expires_at));
                Effect::PairingUri
            }
            RemoteEvent::PairingRequested {
                id,
                device_name,
                platform,
                fingerprint,
            } => {
                self.pending_pairing = Some(PendingPairing {
                    id,
                    device_name,
                    platform,
                    fingerprint,
                });
                Effect::PairingRequested { id }
            }
            RemoteEvent::PairingClosed { id } => {
                if self.pending_pairing.as_ref().is_some_and(|p| p.id == id) {
                    self.pending_pairing = None;
                }
                // The screen decides: it is the one holding the dialog.
                Effect::PairingClosed { id }
            }
            RemoteEvent::ClientConnected {
                client,
                device_key,
                name,
                platform,
            } => {
                self.clients.insert(
                    client,
                    ConnectedClient {
                        device_key,
                        name: name.clone(),
                        platform,
                        connected_at: now,
                    },
                );
                // ADR-0105 section 2: the new client gets the tree now.
                self.tree_forced = true;
                Effect::ClientConnected { name }
            }
            RemoteEvent::ClientDisconnected { client } => {
                self.clients.remove(&client);
                self.viewing.remove(&client);
                // A client that left is owed no answer.
                self.scrollback_queue.retain(|(from, _)| *from != client);
                // Its pending confirmations cancel the action (ADR-0107
                // section 3): dropped, with nobody to tell.
                self.confirmations.drop_client(client);
                Effect::ClientDisconnected
            }
            RemoteEvent::ClientViewing { client, tab } => {
                // Only a phone in session views anything.
                if !self.clients.contains_key(&client) {
                    return Effect::None;
                }
                let before = match tab {
                    Some(tab) => self.viewing.insert(client, tab),
                    None => self.viewing.remove(&client),
                };
                if before == tab {
                    Effect::None
                } else {
                    Effect::ViewingChanged
                }
            }
            RemoteEvent::Subscriptions(panes) => {
                let panes: BTreeSet<u64> = panes.into_iter().collect();
                // A pane that just got a subscriber needs a frame now
                // (ADR-0105 section 3); one that lost them all needs none.
                self.mirror_dirty
                    .extend(panes.difference(&self.subscriptions).copied());
                self.mirror_dirty.retain(|pane| panes.contains(pane));
                self.subscriptions = panes;
                Effect::None
            }
            RemoteEvent::Scrollback { client, request } => {
                self.scrollback_queue.push_back((client, request));
                Effect::None
            }
            other @ (RemoteEvent::Input { .. }
            | RemoteEvent::Action { .. }
            | RemoteEvent::Confirm { .. }
            | RemoteEvent::ListSessions { .. }) => Effect::Unhandled(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    fn connected(client: u64, name: &str) -> RemoteEvent {
        RemoteEvent::ClientConnected {
            client: ClientId(client),
            device_key: [client as u8; 32],
            name: name.to_owned(),
            platform: DevicePlatform::Android,
        }
    }

    fn empty_tree() -> Workspace {
        Workspace {
            rev: 0,
            windows: Vec::new(),
        }
    }

    /// A state with one connected client (the connection forces the tree).
    fn with_client() -> (RemoteState, Instant) {
        let mut state = RemoteState::new();
        let t0 = now();
        state.absorb(connected(1, "phone"), t0);
        (state, t0)
    }

    #[test]
    fn wanted_needs_the_key_and_a_window() {
        assert!(remote_wanted(true, 1));
        assert!(!remote_wanted(true, 0));
        assert!(!remote_wanted(false, 3));
    }

    #[test]
    fn the_host_plan_follows_the_config() {
        let url = "wss://relay.example";
        assert_eq!(host_plan(false, None, url, None), HostPlan::Keep);
        assert_eq!(host_plan(false, Some(url), url, None), HostPlan::Stop);
        assert_eq!(host_plan(true, None, url, None), HostPlan::Start);
        assert_eq!(host_plan(true, Some(url), url, None), HostPlan::Keep);
        // Another URL under a live host: restart.
        assert_eq!(
            host_plan(true, Some("wss://old.example"), url, None),
            HostPlan::Restart
        );
    }

    #[test]
    fn a_failed_start_is_not_retried_for_the_same_url() {
        let url = "wss://relay.example";
        assert_eq!(host_plan(true, None, url, Some(url)), HostPlan::Keep);
        assert_eq!(
            host_plan(true, None, "wss://other.example", Some(url)),
            HostPlan::Start
        );
    }

    #[test]
    fn status_events_become_the_status_and_only_a_change_is_an_effect() {
        let mut state = RemoteState::new();
        assert_eq!(state.status, RemoteStatus::Off);
        let effect = state.absorb(RemoteEvent::Status(RelayStatus::Connecting), now());
        assert!(matches!(effect, Effect::StatusChanged));
        assert_eq!(state.status, RemoteStatus::Connecting);
        let effect = state.absorb(RemoteEvent::Status(RelayStatus::Connecting), now());
        assert!(matches!(effect, Effect::None));
        let effect = state.absorb(
            RemoteEvent::Status(RelayStatus::Failed(RelayFailure::TokenMissing)),
            now(),
        );
        assert!(matches!(effect, Effect::StatusChanged));
        assert_eq!(
            state.status,
            RemoteStatus::Failed(RelayFailure::TokenMissing)
        );
    }

    #[test]
    fn a_client_comes_and_goes_and_the_connection_forces_the_tree() {
        let mut state = RemoteState::new();
        let t0 = now();
        let effect = state.absorb(connected(4, "phone"), t0);
        assert!(matches!(effect, Effect::ClientConnected { ref name } if name == "phone"));
        assert_eq!(state.clients[&ClientId(4)].connected_at, t0);
        assert_eq!(state.clients[&ClientId(4)].name, "phone");
        assert!(state.tree_forced);

        let effect = state.absorb(
            RemoteEvent::ClientDisconnected {
                client: ClientId(4),
            },
            t0,
        );
        assert!(matches!(effect, Effect::ClientDisconnected));
        assert!(state.clients.is_empty());
    }

    #[test]
    fn subscriptions_and_pairing_are_kept() {
        let mut state = RemoteState::new();
        state.absorb(RemoteEvent::Subscriptions(vec![3, 1, 3]), now());
        assert_eq!(
            state.subscriptions.iter().copied().collect::<Vec<_>>(),
            [1, 3]
        );

        let expires = now() + Duration::from_secs(60);
        state.absorb(
            RemoteEvent::PairingUri {
                uri: "porecatu://pair".to_owned(),
                expires_at: expires,
            },
            now(),
        );
        assert_eq!(
            state.pairing_uri,
            Some(("porecatu://pair".to_owned(), expires))
        );

        let effect = state.absorb(
            RemoteEvent::PairingRequested {
                id: PairingId(9),
                device_name: "phone".to_owned(),
                platform: DevicePlatform::Ios,
                fingerprint: "ABCD EFGH IJKL MNOP".to_owned(),
            },
            now(),
        );
        assert!(matches!(effect, Effect::PairingRequested { id } if id == PairingId(9)));
        assert_eq!(state.pending_pairing.as_ref().unwrap().device_name, "phone");
        // The wrong id does not close the pending one; the right one does.
        state.absorb(RemoteEvent::PairingClosed { id: PairingId(1) }, now());
        assert!(state.pending_pairing.is_some());
        state.absorb(RemoteEvent::PairingClosed { id: PairingId(9) }, now());
        assert!(state.pending_pairing.is_none());
    }

    #[test]
    fn the_devices_and_the_pairing_events_say_what_the_app_does() {
        let mut state = RemoteState::new();
        let effect = state.absorb(RemoteEvent::Devices(Vec::new()), now());
        assert!(matches!(effect, Effect::DevicesChanged));
        let effect = state.absorb(
            RemoteEvent::PairingUri {
                uri: "porecatu://pair".to_owned(),
                expires_at: now(),
            },
            now(),
        );
        assert!(matches!(effect, Effect::PairingUri));
        // The channel of any request closing is the screen's to judge.
        let effect = state.absorb(RemoteEvent::PairingClosed { id: PairingId(4) }, now());
        assert!(matches!(effect, Effect::PairingClosed { id } if id == PairingId(4)));
    }

    #[test]
    fn stopping_the_host_clears_the_session_state() {
        let mut state = RemoteState::new();
        state.started_url = Some("wss://relay.example".to_owned());
        state.status = RemoteStatus::Connected;
        state.absorb(connected(1, "phone"), now());
        state.absorb(RemoteEvent::Subscriptions(vec![1]), now());
        state.absorb(
            RemoteEvent::PairingUri {
                uri: "u".to_owned(),
                expires_at: now(),
            },
            now(),
        );
        state.absorb(
            RemoteEvent::PairingRequested {
                id: PairingId(1),
                device_name: "p".to_owned(),
                platform: DevicePlatform::Other,
                fingerprint: "f".to_owned(),
            },
            now(),
        );

        state.host_stopped();

        assert_eq!(state.status, RemoteStatus::Off);
        assert_eq!(state.started_url, None);
        assert!(state.clients.is_empty());
        assert!(state.subscriptions.is_empty());
        assert!(state.pairing_uri.is_none());
        assert!(state.pending_pairing.is_none());
        assert!(state.appearance_dirty());
    }

    #[test]
    fn a_client_that_disconnects_cancels_its_pending_confirmations() {
        use actions::ConfirmTarget;
        let (mut state, t0) = with_client();
        state.absorb(connected(2, "tablet"), t0);
        state
            .confirmations
            .insert(ClientId(1), 7, ConfirmTarget::Tab(3), t0);
        state
            .confirmations
            .insert(ClientId(2), 7, ConfirmTarget::Tab(3), t0);

        state.absorb(
            RemoteEvent::ClientDisconnected {
                client: ClientId(1),
            },
            t0,
        );

        // The one of the client that left is gone, with no answer owed; the
        // other client's is untouched.
        assert_eq!(state.confirmations.answer(ClientId(1), 7), None);
        assert_eq!(
            state.confirmations.answer(ClientId(2), 7),
            Some(ConfirmTarget::Tab(3))
        );
    }

    #[test]
    fn stopping_the_host_forgets_the_pending_confirmations() {
        use actions::ConfirmTarget;
        let (mut state, t0) = with_client();
        state
            .confirmations
            .insert(ClientId(1), 7, ConfirmTarget::Pane(3), t0);
        state.host_stopped();
        assert!(state.confirmations.is_empty());
        assert_eq!(state.confirmations.deadline(), None);
    }

    #[test]
    fn without_a_client_nothing_is_due() {
        let mut state = RemoteState::new();
        let t0 = now();
        state.mark_tree_dirty();
        assert_eq!(state.tree_due(t0), TreeDue::Idle);
        assert_eq!(state.tree_deadline(t0), None);
    }

    #[test]
    fn the_tree_waits_for_a_change_and_for_the_ceiling() {
        let (mut state, t0) = with_client();
        // The connection forces it.
        assert_eq!(state.tree_due(t0), TreeDue::Now);
        assert!(state.tree_built(t0, empty_tree()).is_some());

        // Nothing changed: idle.
        assert_eq!(state.tree_due(t0), TreeDue::Idle);

        // A change right after a build waits for the ceiling.
        state.mark_tree_dirty();
        let soon = t0 + Duration::from_millis(100);
        assert_eq!(state.tree_due(soon), TreeDue::At(t0 + TREE_MIN_INTERVAL));
        let later = t0 + TREE_MIN_INTERVAL;
        assert_eq!(state.tree_due(later), TreeDue::Now);
    }

    #[test]
    fn an_equal_tree_is_not_sent_again_unless_forced() {
        let (mut state, t0) = with_client();
        assert!(state.tree_built(t0, empty_tree()).is_some());

        state.mark_tree_dirty();
        let t1 = t0 + TREE_MIN_INTERVAL;
        assert!(state.tree_built(t1, empty_tree()).is_none());
        // The dirty flag is spent and the build counts for the ceiling.
        assert!(!state.tree_dirty);
        assert_eq!(state.last_tree_built, Some(t1));

        // A new client forces the send even with an equal tree.
        state.absorb(connected(2, "tablet"), t1);
        assert!(state.tree_built(t1, empty_tree()).is_some());
        assert!(!state.tree_forced);
    }

    #[test]
    fn a_different_tree_is_sent() {
        let (mut state, t0) = with_client();
        state.tree_built(t0, empty_tree());
        state.mark_tree_dirty();
        let mut other = empty_tree();
        other.rev = 0;
        other
            .windows
            .push(porecatu_remote::protocol::message::Window {
                id: 1,
                title: "w".to_owned(),
                pinned: Vec::new(),
                items: Vec::new(),
                active: None,
            });
        assert_eq!(
            state.tree_built(t0 + TREE_MIN_INTERVAL, other.clone()),
            Some(other)
        );
    }

    fn theme(fg: u32) -> Theme {
        Theme {
            fg,
            bg: 0,
            cursor: 0,
            selection_bg: 0,
            palette: [0; 16],
            group_colors: Vec::new(),
        }
    }

    fn profiles(default: &str) -> Profiles {
        Profiles {
            list: Vec::new(),
            default: default.to_owned(),
        }
    }

    #[test]
    fn theme_and_profiles_are_sent_only_when_they_change() {
        let mut state = RemoteState::new();
        state.host_started("wss://relay.example".to_owned());
        assert!(state.appearance_dirty());

        let (t, p) = state.appearance_built(theme(1), profiles("a"));
        assert!(t.is_some() && p.is_some());
        assert!(!state.appearance_dirty());

        // Nothing changed.
        state.mark_appearance_dirty();
        let (t, p) = state.appearance_built(theme(1), profiles("a"));
        assert!(t.is_none() && p.is_none());

        // Only the theme changed.
        state.mark_appearance_dirty();
        let (t, p) = state.appearance_built(theme(2), profiles("a"));
        assert_eq!(t, Some(theme(2)));
        assert!(p.is_none());

        // A new host is told everything again.
        state.host_stopped();
        state.host_started("wss://relay.example".to_owned());
        let (t, p) = state.appearance_built(theme(2), profiles("a"));
        assert!(t.is_some() && p.is_some());
    }

    fn scrollback(client: u64, id: u64) -> RemoteEvent {
        RemoteEvent::Scrollback {
            client: ClientId(client),
            request: ScrollbackRequest {
                id,
                pane: 1,
                offset: 0,
                count: 1,
            },
        }
    }

    #[test]
    fn what_the_state_does_not_keep_goes_back_to_the_app() {
        let mut state = RemoteState::new();
        let effect = state.absorb(
            RemoteEvent::Input {
                client: ClientId(1),
                input: porecatu_remote::RemoteInput::Text(
                    porecatu_remote::protocol::message::TextInput {
                        pane: 1,
                        text: "x".to_owned(),
                    },
                ),
            },
            now(),
        );
        assert!(matches!(
            effect,
            Effect::Unhandled(RemoteEvent::Input { .. })
        ));
    }

    // --- the mirror -----------------------------------------------------

    #[test]
    fn the_mirror_pass_is_due_only_with_work_and_at_most_thirty_times_a_second() {
        let t0 = now();
        // No work: no deadline whatever the clock says.
        assert_eq!(mirror_due(false, None, t0), MirrorDue::Idle);
        assert_eq!(mirror_due(false, Some(t0), t0), MirrorDue::Idle);
        // Work and no pass yet: now.
        assert_eq!(mirror_due(true, None, t0), MirrorDue::Now);
        // Right after a pass: wait the interval.
        let soon = t0 + Duration::from_millis(10);
        assert_eq!(
            mirror_due(true, Some(t0), soon),
            MirrorDue::At(t0 + MIRROR_MIN_INTERVAL)
        );
        // The interval has passed (exactly, or long ago): now.
        assert_eq!(
            mirror_due(true, Some(t0), t0 + MIRROR_MIN_INTERVAL),
            MirrorDue::Now
        );
        assert_eq!(
            mirror_due(true, Some(t0), t0 + Duration::from_secs(5)),
            MirrorDue::Now
        );
        assert_eq!(MIRROR_MIN_INTERVAL, Duration::from_millis(33));
    }

    #[test]
    fn only_a_subscribed_pane_is_marked_dirty() {
        let mut state = RemoteState::new();
        state.mark_pane_dirty(7);
        assert_eq!(state.mirror_due(now()), MirrorDue::Idle);

        state.absorb(RemoteEvent::Subscriptions(vec![7]), now());
        // The new subscription is dirty by itself: it needs a frame now.
        assert_eq!(state.mirror_due(now()), MirrorDue::Now);
        assert_eq!(state.begin_mirror_pass(now()), [7]);
        assert_eq!(state.mirror_due(now()), MirrorDue::Idle);

        state.mark_pane_dirty(7);
        state.mark_pane_dirty(8);
        assert_eq!(state.begin_mirror_pass(now()), [7]);
    }

    #[test]
    fn a_pane_seen_again_in_subscriptions_is_not_dirty_but_a_new_one_is() {
        let mut state = RemoteState::new();
        state.absorb(RemoteEvent::Subscriptions(vec![1]), now());
        state.begin_mirror_pass(now());

        // A second client subscribes to pane 2: only pane 2 needs a frame.
        state.absorb(RemoteEvent::Subscriptions(vec![1, 2]), now());
        assert_eq!(state.begin_mirror_pass(now()), [2]);

        // Pane 1 loses its subscribers while dirty: it leaves the dirt.
        state.mark_pane_dirty(1);
        state.absorb(RemoteEvent::Subscriptions(vec![2]), now());
        assert!(state.begin_mirror_pass(now()).is_empty());
    }

    #[test]
    fn a_pass_without_a_dirty_pane_still_starts_the_interval() {
        let mut state = RemoteState::new();
        let t0 = now();
        state.absorb(scrollback(1, 1), t0);
        assert_eq!(state.mirror_due(t0), MirrorDue::Now);
        assert!(state.begin_mirror_pass(t0).is_empty());
        // A second request now waits for the ceiling.
        state.absorb(scrollback(1, 2), t0);
        assert_eq!(state.mirror_deadline(t0), Some(t0 + MIRROR_MIN_INTERVAL));
    }

    #[test]
    fn scrollback_requests_are_answered_in_arrival_order() {
        let mut state = RemoteState::new();
        for id in [3, 1, 2] {
            let effect = state.absorb(scrollback(1, id), now());
            assert!(matches!(effect, Effect::None));
        }
        let order: Vec<u64> = std::iter::from_fn(|| state.next_scrollback())
            .map(|(_, request)| request.id)
            .collect();
        assert_eq!(order, [3, 1, 2]);
        assert_eq!(state.mirror_due(now()), MirrorDue::Idle);
    }

    #[test]
    fn a_client_that_leaves_takes_its_scrollback_requests_with_it() {
        let mut state = RemoteState::new();
        state.absorb(scrollback(1, 10), now());
        state.absorb(scrollback(2, 20), now());
        state.absorb(scrollback(1, 11), now());
        state.absorb(
            RemoteEvent::ClientDisconnected {
                client: ClientId(1),
            },
            now(),
        );
        let left: Vec<(ClientId, u64)> = std::iter::from_fn(|| state.next_scrollback())
            .map(|(client, request)| (client, request.id))
            .collect();
        assert_eq!(left, [(ClientId(2), 20)]);
    }

    #[test]
    fn stopping_the_host_forgets_the_mirror() {
        let mut state = RemoteState::new();
        state.absorb(RemoteEvent::Subscriptions(vec![1]), now());
        state.absorb(scrollback(1, 1), now());
        state.begin_mirror_pass(now());
        state.host_stopped();
        assert_eq!(state.mirror_due(now()), MirrorDue::Idle);
        assert!(state.next_scrollback().is_none());
        assert_eq!(state.last_mirror, None);
    }

    #[test]
    fn the_dirt_of_a_pane_gone_from_the_tree_is_forgotten() {
        // A real `WindowId` needs a window, so the id table is left empty:
        // pane 5 is subscribed (dirty by itself) but is not in the table.
        let mut state = RemoteState::new();
        state.absorb(RemoteEvent::Subscriptions(vec![5]), now());
        assert_eq!(state.mirror_due(now()), MirrorDue::Now);
        state.forget_gone_panes();
        assert!(state.begin_mirror_pass(now()).is_empty());
    }
    // --- the tabs the phones view (ADR-0112 section 1) ---------------------

    fn viewing(client: u64, tab: Option<u64>) -> RemoteEvent {
        RemoteEvent::ClientViewing {
            client: ClientId(client),
            tab,
        }
    }

    fn tid(n: u32) -> TabId {
        TabId::new(n)
    }

    /// Windows 7 and 8; protocol ids 1 and 2 are tabs 0 and 1 of window 7, 3 is
    /// tab 0 of window 8.
    fn two_windows() -> ProtocolIds<u8> {
        let mut ids = ProtocolIds::new();
        ids.window(7);
        ids.window(8);
        ids.tab(7, tid(0));
        ids.tab(7, tid(1));
        ids.tab(8, tid(0));
        ids
    }

    /// Phones 1, 2 and 3, connected in that order, a moment apart.
    fn three_phones() -> (RemoteState, Instant) {
        let mut state = RemoteState::new();
        let t0 = now();
        for (n, name) in [(1, "Pixel"), (2, "Galaxy"), (3, "iPhone")] {
            let at = t0 + Duration::from_millis(n);
            state.absorb(connected(n, name), at);
        }
        (state, t0)
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn view_tab_events_are_kept_per_phone_and_only_a_change_is_an_effect() {
        let (mut state, t0) = with_client();
        let effect = state.absorb(viewing(1, Some(5)), t0);
        assert!(matches!(effect, Effect::ViewingChanged));
        assert_eq!(state.viewing.get(&ClientId(1)), Some(&5));
        // The same tab again: nothing.
        assert!(matches!(
            state.absorb(viewing(1, Some(5)), t0),
            Effect::None
        ));
        let effect = state.absorb(viewing(1, Some(6)), t0);
        assert!(matches!(effect, Effect::ViewingChanged));
        assert_eq!(state.viewing.get(&ClientId(1)), Some(&6));
        let effect = state.absorb(viewing(1, None), t0);
        assert!(matches!(effect, Effect::ViewingChanged));
        assert!(state.viewing.is_empty());
        assert!(matches!(state.absorb(viewing(1, None), t0), Effect::None));
        // A phone that is not in session views nothing.
        assert!(matches!(
            state.absorb(viewing(9, Some(5)), t0),
            Effect::None
        ));
        assert!(state.viewing.is_empty());
    }

    #[test]
    fn a_disconnection_or_a_stopped_host_clears_what_was_viewed() {
        let (mut state, t0) = three_phones();
        state.absorb(viewing(1, Some(1)), t0);
        state.absorb(viewing(2, Some(2)), t0);
        state.absorb(
            RemoteEvent::ClientDisconnected {
                client: ClientId(1),
            },
            t0,
        );
        assert_eq!(state.viewing, BTreeMap::from([(ClientId(2), 2)]));
        state.host_stopped();
        assert!(state.viewing.is_empty());
    }

    #[test]
    fn two_phones_on_different_tabs_are_two_entries_in_their_windows() {
        let (mut state, t0) = three_phones();
        state.absorb(viewing(1, Some(1)), t0);
        state.absorb(viewing(2, Some(3)), t0);
        let ids = two_windows();
        let by_window = viewers_by_window(&state.clients, &state.viewing, &ids);
        assert_eq!(by_window.len(), 2);
        assert_eq!(by_window[&7], HashMap::from([(tid(0), names(&["Pixel"]))]));
        assert_eq!(by_window[&8], HashMap::from([(tid(0), names(&["Galaxy"]))]));
    }

    #[test]
    fn two_phones_on_the_same_tab_are_one_entry_in_connection_order() {
        let (mut state, t0) = three_phones();
        // The later phone says it first: the list is still by connection.
        state.absorb(viewing(3, Some(2)), t0);
        state.absorb(viewing(1, Some(2)), t0);
        let ids = two_windows();
        let by_window = viewers_by_window(&state.clients, &state.viewing, &ids);
        assert_eq!(
            by_window[&7],
            HashMap::from([(tid(1), names(&["Pixel", "iPhone"]))])
        );
        assert!(!by_window.contains_key(&8));
    }

    #[test]
    fn a_phone_that_stopped_viewing_or_left_leaves_no_trace() {
        let (mut state, t0) = three_phones();
        let ids = two_windows();
        state.absorb(viewing(1, Some(1)), t0);
        state.absorb(viewing(2, Some(1)), t0);
        state.absorb(viewing(1, None), t0);
        let by_window = viewers_by_window(&state.clients, &state.viewing, &ids);
        assert_eq!(by_window[&7], HashMap::from([(tid(0), names(&["Galaxy"]))]));
        state.absorb(
            RemoteEvent::ClientDisconnected {
                client: ClientId(2),
            },
            t0,
        );
        assert!(viewers_by_window(&state.clients, &state.viewing, &ids).is_empty());
    }

    #[test]
    fn an_unknown_tab_id_is_ignored() {
        let (mut state, t0) = three_phones();
        let ids = two_windows();
        state.absorb(viewing(1, Some(99)), t0);
        state.absorb(viewing(2, Some(1)), t0);
        let by_window = viewers_by_window(&state.clients, &state.viewing, &ids);
        assert_eq!(by_window.len(), 1);
        assert_eq!(by_window[&7], HashMap::from([(tid(0), names(&["Galaxy"]))]));
    }

    #[test]
    fn a_tab_that_changed_window_shows_up_in_the_new_one() {
        let (mut state, t0) = three_phones();
        let mut ids = two_windows();
        state.absorb(viewing(1, Some(2)), t0);
        // Tab 1 of window 7 becomes tab 5 of window 8: same protocol id.
        ids.rekey_tabs(7, 8, &[(tid(1), tid(5))]);
        let by_window = viewers_by_window(&state.clients, &state.viewing, &ids);
        assert!(!by_window.contains_key(&7));
        assert_eq!(by_window[&8], HashMap::from([(tid(5), names(&["Pixel"]))]));
    }

    #[test]
    fn the_check_without_allocation_agrees_with_the_projection() {
        let (mut state, t0) = three_phones();
        let ids = two_windows();
        let check = |state: &RemoteState, window: u8, held: &HashMap<TabId, Vec<String>>| {
            viewers_current(held, window, &state.clients, &state.viewing, &ids)
        };
        let empty = HashMap::new();
        // Nobody views anything: the empty map is current.
        assert!(check(&state, 7, &empty));
        state.absorb(viewing(1, Some(1)), t0);
        state.absorb(viewing(2, Some(1)), t0);
        state.absorb(viewing(3, Some(3)), t0);
        let by_window = viewers_by_window(&state.clients, &state.viewing, &ids);
        assert!(check(&state, 7, &by_window[&7]));
        assert!(check(&state, 8, &by_window[&8]));
        // The empty map is stale, and so is the map of the other window.
        assert!(!check(&state, 7, &empty));
        assert!(!check(&state, 7, &by_window[&8]));
        // Another order, another name, an empty entry.
        let reversed = HashMap::from([(tid(0), names(&["Galaxy", "Pixel"]))]);
        assert!(!check(&state, 7, &reversed));
        let renamed = HashMap::from([(tid(0), names(&["Pixel", "Other"]))]);
        assert!(!check(&state, 7, &renamed));
        let empty_entry = HashMap::from([(tid(1), Vec::new())]);
        assert!(!check(&state, 8, &empty_entry));
        // Someone stops viewing: the old projection is stale.
        state.absorb(viewing(3, None), t0);
        assert!(!check(&state, 8, &by_window[&8]));
        assert!(check(&state, 8, &empty));
    }
}
