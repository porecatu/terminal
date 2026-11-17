// SPDX-License-Identifier: GPL-3.0-or-later

//! One session per channel: handshake, pairing, `hello` and records.
//!
//! A [`Session`] wraps the sans-IO `HostChannel` of the protocol crate and
//! decides, with the state of the host ([`Core`]), what each thing that
//! happens on the channel means: who may connect, whether a token is good,
//! which version to speak. It never touches the relay: records and closes go
//! to the link through `Core::link_send`, and a session that wants its
//! channel closed only says so ([`Session::closing`]); the host does it.
//!
//! The life of a session:
//!
//! ```text
//! Handshake --IK, known device---------------------------> Hello --hello--> Active
//!     |  \--IK, unknown device: not-paired, close
//!     \--XX, token good--> Pairing --accept--> (pair-accepted) Hello
//!             \--XX, token bad: token-invalid, close       \--decline/timeout: close
//! ```
//!
//! Nothing here logs, and nothing carries free text: what the relay is told
//! is only `normal` or `host-rejected`, never why.

use std::collections::BTreeMap;
use std::time::SystemTime;

use bytes::Bytes;

use crate::api::{ClientId, DevicePlatform, PairingAnswer, PairingId, RemoteEvent, RemoteInput};
use crate::grid_flow::Subscription;
use crate::host::Core;
use crate::protocol::PROTOCOL_VERSION;
use crate::protocol::envelope::{ChannelId, CloseReason};
use crate::protocol::fingerprint::fingerprint;
use crate::protocol::message::{
    ErrorCode, ErrorMessage, Goodbye, GoodbyeReason, Hello, Incompatible, Message, PairAccepted,
    PairPlatform, PairRejectReason, PairRejected, PairRequest, SubscriptionEndReason,
    SubscriptionEnded, Version, Welcome, Workspace,
};
use crate::protocol::session::{HostChannel, HostEvent};

/// How many panes one client may subscribe to (`welcome.max_subscriptions`).
const MAX_SUBSCRIPTIONS: u64 = 4;

/// The wire version this host speaks.
const VERSION: Version = Version {
    major: PROTOCOL_VERSION[0],
    minor: PROTOCOL_VERSION[1],
};

enum Stage {
    /// Waiting for the Noise handshake to say who this is.
    Handshake,
    /// A device we know is connected (or was just paired): waiting for its
    /// `hello`.
    Hello {
        device_key: [u8; 32],
        deadline_ms: u64,
    },
    /// A device presented a good token: waiting for the user's answer.
    Pairing(Pairing),
    /// `hello` accepted: the client is in session.
    Active(Active),
    /// Over: the session only waits to be removed.
    Done,
}

struct Pairing {
    id: PairingId,
    device_key: [u8; 32],
    name: String,
    platform: DevicePlatform,
    deadline_ms: u64,
}

struct Active {
    client: ClientId,
    device_key: [u8; 32],
    /// Number of `workspace` messages sent to this client.
    rev: u64,
    /// The panes this client subscribes to, with the flow control of each
    /// (at most [`MAX_SUBSCRIPTIONS`]).
    subs: BTreeMap<u64, Subscription>,
}

fn platform_of(platform: PairPlatform) -> DevicePlatform {
    match platform {
        PairPlatform::Android => DevicePlatform::Android,
        PairPlatform::Ios => DevicePlatform::Ios,
        _ => DevicePlatform::Other,
    }
}

/// The host side of one channel.
pub(crate) struct Session {
    channel: ChannelId,
    channel_state: HostChannel,
    stage: Stage,
    /// Set when the channel must be closed with this reason.
    closing: Option<CloseReason>,
}

impl Session {
    /// A channel the relay just opened.
    pub(crate) fn new(channel: ChannelId, core: &Core) -> Session {
        Session {
            channel,
            channel_state: HostChannel::new(&core.static_keypair, &core.host_id, core.now_ms()),
            stage: Stage::Handshake,
            closing: None,
        }
    }

    /// Why the channel must be closed, if it must.
    pub(crate) fn closing(&self) -> Option<CloseReason> {
        self.closing
    }

    /// The client of a session in `Active`.
    pub(crate) fn client(&self) -> Option<ClientId> {
        match &self.stage {
            Stage::Active(active) => Some(active.client),
            _ => None,
        }
    }

    /// The device of a session that got past the handshake as a known device.
    pub(crate) fn device_key(&self) -> Option<[u8; 32]> {
        match &self.stage {
            Stage::Hello { device_key, .. } => Some(*device_key),
            Stage::Active(active) => Some(active.device_key),
            _ => None,
        }
    }

    /// The request this session waits for an answer to.
    pub(crate) fn pairing_id(&self) -> Option<PairingId> {
        match &self.stage {
            Stage::Pairing(pairing) => Some(pairing.id),
            _ => None,
        }
    }

    /// The nearest moment this session needs [`tick`](Self::tick): a stage
    /// deadline, the handshake's, or the moment a subscription may send again
    /// (the rate cap of the grid flow).
    pub(crate) fn next_deadline(&self, core: &Core) -> Option<u64> {
        let stage = match &self.stage {
            Stage::Hello { deadline_ms, .. } => Some(*deadline_ms),
            Stage::Pairing(pairing) => Some(pairing.deadline_ms),
            _ => None,
        };
        [
            stage,
            self.channel_state.next_deadline(),
            self.grid_deadline(core),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// When a subscription needs [`poll_all`](Self::poll_all) again.
    fn grid_deadline(&self, core: &Core) -> Option<u64> {
        let Stage::Active(active) = &self.stage else {
            return None;
        };
        let now = core.now_ms();
        active
            .subs
            .iter()
            .filter_map(|(pane, sub)| sub.next_deadline(core.latest_frame(*pane), now))
            .min()
    }

    // -- input ------------------------------------------------------------

    /// One record arrived on the channel.
    pub(crate) fn on_data(&mut self, core: &mut Core, payload: &[u8]) {
        if self.closing.is_some() {
            return;
        }
        match self.channel_state.receive(payload, core.now_ms()) {
            Ok(events) => self.on_events(core, events),
            Err(error) => self.closing = Some(error.close_reason()),
        }
    }

    /// A deadline may have passed.
    pub(crate) fn tick(&mut self, core: &mut Core) {
        if self.closing.is_some() {
            return;
        }
        let now = core.now_ms();
        let events = self.channel_state.tick(now);
        self.on_events(core, events);
        if self.closing.is_some() {
            return;
        }
        match &self.stage {
            Stage::Hello { deadline_ms, .. } if now >= *deadline_ms => {
                self.closing = Some(CloseReason::Normal);
            }
            Stage::Pairing(pairing) if now >= pairing.deadline_ms => {
                // The UI never answered: as if the dialog timed out.
                let id = pairing.id;
                self.reject(core, PairRejectReason::Timeout);
                core.emit(RemoteEvent::PairingClosed { id });
            }
            _ => {}
        }
        if matches!(self.stage, Stage::Active(_)) {
            self.poll_all(core);
        }
    }

    fn on_events(&mut self, core: &mut Core, events: Vec<HostEvent>) {
        for event in events {
            match event {
                // Always put on the wire, in order: an `error` may precede a
                // failure.
                HostEvent::Send(bytes) => self.push(core, vec![bytes]),
                _ if self.closing.is_some() => {}
                HostEvent::DeviceConnecting { device_key } => self.on_device(core, device_key),
                HostEvent::PairRequested {
                    device_key,
                    request,
                } => self.on_pair_request(core, device_key, request),
                HostEvent::Message(message) => self.on_message(core, message),
                HostEvent::Failed(error) => self.closing = Some(error.close_reason()),
            }
        }
    }

    /// `IK` finished: only a device on the list gets on (spec 02 §4).
    fn on_device(&mut self, core: &mut Core, device_key: [u8; 32]) {
        if !matches!(self.stage, Stage::Handshake) {
            return;
        }
        if core.devices.contains(&device_key) {
            self.stage = Stage::Hello {
                device_key,
                deadline_ms: core.now_ms().saturating_add(core.tuning.hello_ms),
            };
            return;
        }
        if let Ok(payloads) = self.channel_state.reject_not_paired(core.now_ms()) {
            self.push(core, payloads);
        }
        self.stage = Stage::Done;
        self.closing = Some(CloseReason::Normal);
    }

    /// `XX` finished: a device asks to pair.
    fn on_pair_request(&mut self, core: &mut Core, device_key: [u8; 32], request: PairRequest) {
        if !matches!(self.stage, Stage::Handshake) {
            return;
        }
        let now = core.now_ms();
        // One attempt per token, good or not (the protocol crate consumes it
        // either way).
        let good = core
            .token
            .take()
            .is_some_and(|mut token| token.matches_and_consume(&request.token, now));
        if !good {
            self.reject(core, PairRejectReason::TokenInvalid);
            return;
        }
        let id = core.next_pairing();
        let platform = platform_of(request.platform);
        self.stage = Stage::Pairing(Pairing {
            id,
            device_key,
            name: request.device.clone(),
            platform,
            deadline_ms: now.saturating_add(core.tuning.pairing_backstop_ms),
        });
        core.emit(RemoteEvent::PairingRequested {
            id,
            device_name: request.device.clone(),
            platform,
            fingerprint: fingerprint(&device_key).to_string(),
        });
    }

    /// The user answered the dialog.
    pub(crate) fn answer(&mut self, core: &mut Core, answer: PairingAnswer) {
        let Stage::Pairing(pairing) = &self.stage else {
            return;
        };
        let device_key = pairing.device_key;
        let name = pairing.name.clone();
        let platform = pairing.platform;
        match answer {
            PairingAnswer::Accept => {
                core.devices
                    .upsert(device_key, name, platform, SystemTime::now());
                core.persist_devices();
                let message = Message::PairAccepted(PairAccepted {
                    host_name: core.host_name.clone(),
                });
                self.stage = Stage::Hello {
                    device_key,
                    deadline_ms: core.now_ms().saturating_add(core.tuning.hello_ms),
                };
                self.send(core, &message);
            }
            PairingAnswer::Decline => self.reject(core, PairRejectReason::UserDeclined),
            PairingAnswer::Timeout => self.reject(core, PairRejectReason::Timeout),
        }
    }

    /// `pair-rejected` and the end of the session.
    fn reject(&mut self, core: &Core, reason: PairRejectReason) {
        self.send(core, &Message::PairRejected(PairRejected { reason }));
        self.stage = Stage::Done;
        self.closing = Some(CloseReason::Normal);
    }

    fn on_message(&mut self, core: &mut Core, message: Message) {
        match &self.stage {
            // Before the user answered, the client has nothing to say (the
            // `hello` comes after `pair-accepted`).
            Stage::Handshake | Stage::Pairing(_) | Stage::Done => {}
            Stage::Hello { .. } => match message {
                Message::Hello(hello) => self.on_hello(core, &hello),
                Message::Goodbye(_) => self.closing = Some(CloseReason::Normal),
                _ => {
                    self.send_error(core, ErrorCode::Protocol, None, None);
                    self.closing = Some(CloseReason::Normal);
                }
            },
            Stage::Active(_) => self.on_active_message(core, message),
        }
    }

    /// Version negotiation (spec 04): speak `[0, 1]` or say `incompatible`.
    fn on_hello(&mut self, core: &mut Core, hello: &Hello) {
        let Stage::Hello { device_key, .. } = self.stage else {
            return;
        };
        if hello.version.major != VERSION.major || hello.version.minor != VERSION.minor {
            self.send(
                core,
                &Message::Incompatible(Incompatible { version: VERSION }),
            );
            self.stage = Stage::Done;
            self.closing = Some(CloseReason::Normal);
            return;
        }
        let client = core.next_client();
        self.stage = Stage::Active(Active {
            client,
            device_key,
            rev: 0,
            subs: BTreeMap::new(),
        });
        self.send(
            core,
            &Message::Welcome(Welcome {
                version: VERSION,
                host_name: core.host_name.clone(),
                porecatu_version: core.porecatu_version.clone(),
                max_subscriptions: MAX_SUBSCRIPTIONS,
                host_instance: core.host_instance(),
            }),
        );
        // What the UI already told us, in this order; what has not arrived
        // goes when it does.
        if let Some(theme) = core.theme.clone() {
            self.send(core, &Message::Theme(theme));
        }
        if let Some(profiles) = core.profiles.clone() {
            self.send(core, &Message::Profiles(profiles));
        }
        if let Some(workspace) = core.workspace.clone() {
            self.send_workspace(core, &workspace);
        }
        let known = core
            .devices
            .list()
            .into_iter()
            .find(|device| device.key == device_key);
        let (name, platform) = known.map_or_else(
            || (String::new(), DevicePlatform::Other),
            |device| (device.name, device.platform),
        );
        core.emit(RemoteEvent::ClientConnected {
            client,
            device_key,
            name,
            platform,
        });
    }

    /// A message from a client in session.
    fn on_active_message(&mut self, core: &mut Core, message: Message) {
        let Some(client) = self.client() else {
            return;
        };
        match message {
            Message::Action(action) => core.emit(RemoteEvent::Action { client, action }),
            Message::Confirm(confirm) => core.emit(RemoteEvent::Confirm { client, confirm }),
            Message::Goodbye(_) => self.closing = Some(CloseReason::Normal),
            // A second `hello` is out of place.
            Message::Hello(_) => self.send_error(core, ErrorCode::Protocol, None, None),
            other => self.on_stream_message(core, other),
        }
    }

    /// Subscriptions, grid acknowledgements, scrollback and input
    /// (PRD-025 RF-25.18 to RF-25.22, spec 03 §5 and §6).
    fn on_stream_message(&mut self, core: &mut Core, message: Message) {
        let Some(client) = self.client() else {
            return;
        };
        match message {
            Message::Subscribe(m) => self.on_subscribe(core, m.pane),
            Message::Unsubscribe(m) => self.on_unsubscribe(core, m.pane),
            Message::GridAck(m) => self.on_grid_ack(core, m.pane, m.seq),
            Message::ScrollbackRequest(request) => {
                // Reading the history reads the engine of the pane, which the
                // host does only for a pane somebody subscribes (spec 03
                // §5.1): so the client must subscribe to it.
                if self.subscribes(request.pane) {
                    core.emit(RemoteEvent::Scrollback { client, request });
                } else {
                    self.send_error(
                        core,
                        ErrorCode::NotSubscribed,
                        Some(request.pane),
                        Some(request.id),
                    );
                }
            }
            Message::Key(key) => self.on_input(core, client, key.pane, RemoteInput::Key(key)),
            Message::TextInput(text) => {
                self.on_input(core, client, text.pane, RemoteInput::Text(text));
            }
            Message::Paste(paste) => {
                self.on_input(core, client, paste.pane, RemoteInput::Paste(paste));
            }
            // `error` from the client carries nothing to act on.
            _ => {}
        }
    }

    fn subscribes(&self, pane: u64) -> bool {
        matches!(&self.stage, Stage::Active(active) if active.subs.contains_key(&pane))
    }

    /// The panes this client subscribes to.
    pub(crate) fn subscribed(&self) -> Vec<u64> {
        match &self.stage {
            Stage::Active(active) => active.subs.keys().copied().collect(),
            _ => Vec::new(),
        }
    }

    fn on_subscribe(&mut self, core: &mut Core, pane: u64) {
        let Stage::Active(active) = &mut self.stage else {
            return;
        };
        if let Some(sub) = active.subs.get_mut(&pane) {
            // Again: a fresh snapshot, with the next `seq`.
            sub.resubscribe();
        } else if !core.has_pane(pane) {
            self.send_error(core, ErrorCode::UnknownPane, Some(pane), None);
            return;
        } else if active.subs.len() as u64 >= MAX_SUBSCRIPTIONS {
            self.send_error(core, ErrorCode::TooManySubscriptions, Some(pane), None);
            return;
        } else {
            active.subs.insert(pane, Subscription::new(pane));
        }
        // With a frame at hand the snapshot goes now; otherwise the next
        // frame brings it.
        self.poll_pane(core, pane);
    }

    fn on_unsubscribe(&mut self, core: &Core, pane: u64) {
        let removed = match &mut self.stage {
            Stage::Active(active) => active.subs.remove(&pane).is_some(),
            _ => return,
        };
        if !removed {
            self.send_error(core, ErrorCode::NotSubscribed, Some(pane), None);
        }
    }

    fn on_grid_ack(&mut self, core: &Core, pane: u64, seq: u64) {
        let Stage::Active(active) = &mut self.stage else {
            return;
        };
        let Some(sub) = active.subs.get_mut(&pane) else {
            self.send_error(core, ErrorCode::NotSubscribed, Some(pane), None);
            return;
        };
        if sub.ack(seq).is_err() {
            // An acknowledgement of nothing the host sent: protocol error
            // (spec 03 §1), the session ends.
            self.send_error(core, ErrorCode::Protocol, None, None);
            self.closing = Some(CloseReason::Normal);
            return;
        }
        // The window may have opened.
        self.poll_pane(core, pane);
    }

    /// Input of a pane this client subscribes to goes to the UI, which
    /// checks that the pane runs; any other is refused.
    fn on_input(&mut self, core: &Core, client: ClientId, pane: u64, input: RemoteInput) {
        if self.subscribes(pane) {
            core.emit(RemoteEvent::Input { client, input });
        } else {
            self.send_error(core, ErrorCode::NotSubscribed, Some(pane), None);
        }
    }

    // -- grid flow --------------------------------------------------------

    /// Sends what the subscription to `pane` has to send now, if any.
    pub(crate) fn poll_pane(&mut self, core: &Core, pane: u64) {
        let Stage::Active(active) = &mut self.stage else {
            return;
        };
        let Some(sub) = active.subs.get_mut(&pane) else {
            return;
        };
        let message = sub.poll(core.latest_frame(pane), core.now_ms());
        if let Some(message) = message {
            self.send(core, &message);
        }
    }

    /// [`poll_pane`](Self::poll_pane) for every subscription.
    fn poll_all(&mut self, core: &Core) {
        for pane in self.subscribed() {
            self.poll_pane(core, pane);
        }
    }

    /// Ends the subscriptions to panes that left the tree.
    pub(crate) fn end_gone_subscriptions(&mut self, core: &Core) {
        let gone: Vec<u64> = self
            .subscribed()
            .into_iter()
            .filter(|pane| !core.has_pane(*pane))
            .collect();
        let Stage::Active(active) = &mut self.stage else {
            return;
        };
        for pane in &gone {
            active.subs.remove(pane);
        }
        for pane in gone {
            self.send(
                core,
                &Message::SubscriptionEnded(SubscriptionEnded {
                    pane,
                    reason: SubscriptionEndReason::PaneClosed,
                }),
            );
        }
    }

    // -- output -----------------------------------------------------------

    /// Puts records on the link for this channel.
    fn push(&self, core: &Core, payloads: Vec<Bytes>) {
        for payload in payloads {
            core.link_send(crate::relay::LinkCommand::ChannelData(
                self.channel,
                payload,
            ));
        }
    }

    /// Sends one message. A channel that cannot send is a channel to close.
    pub(crate) fn send(&mut self, core: &Core, message: &Message) {
        match self.channel_state.send(message, core.now_ms()) {
            Ok(payloads) => self.push(core, payloads),
            Err(_) => {
                self.closing.get_or_insert(CloseReason::Normal);
            }
        }
    }

    fn send_error(&mut self, core: &Core, code: ErrorCode, pane: Option<u64>, id: Option<u64>) {
        self.send(core, &Message::Error(ErrorMessage { code, id, pane }));
    }

    /// Sends the whole tree with the next `rev` of this session.
    pub(crate) fn send_workspace(&mut self, core: &Core, workspace: &Workspace) {
        let Stage::Active(active) = &mut self.stage else {
            return;
        };
        active.rev += 1;
        let mut workspace = workspace.clone();
        workspace.rev = active.rev;
        self.send(core, &Message::Workspace(workspace));
    }

    /// `goodbye` and the end of the session. Also for a channel that is not
    /// open yet: the failed send just means there is nobody to tell.
    pub(crate) fn goodbye(&mut self, core: &Core, reason: GoodbyeReason) {
        self.send(core, &Message::Goodbye(Goodbye { reason }));
        self.closing = Some(CloseReason::Normal);
    }

    /// The session is over: tells the UI what ended and records the visit.
    pub(crate) fn end(self, core: &mut Core) {
        match self.stage {
            Stage::Active(active) => {
                core.emit(RemoteEvent::ClientDisconnected {
                    client: active.client,
                });
                core.touch_last_seen(&active.device_key);
            }
            Stage::Pairing(pairing) => core.emit(RemoteEvent::PairingClosed { id: pairing.id }),
            _ => {}
        }
    }
}
