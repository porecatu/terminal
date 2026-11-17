// SPDX-License-Identifier: GPL-3.0-or-later

//! Public contract between `porecatu-remote` and `porecatu-ui` (ADR-0104 §3).

use std::path::PathBuf;
use std::time::Instant;

use crate::protocol;

/// Parameters the UI builds from `Config` when remote access turns on.
#[derive(Debug, Clone)]
pub struct RemoteParams {
    /// `[remote] relay_url` as the user wrote it; normalising it is up to this
    /// crate.
    pub relay_url: String,
    /// Directory of `session.json`; `remote_devices.json` lives here too.
    pub data_dir: PathBuf,
    /// Machine name shown on the phone (RF-25.5); the crate truncates it to 32
    /// Unicode scalar values.
    pub host_name: String,
    /// `CARGO_PKG_VERSION` of the app, sent in the `welcome`.
    pub porecatu_version: String,
}

/// One per accepted channel; monotonic per [`crate::RemoteHost`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientId(pub u64);

/// One per pairing request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PairingId(pub u64);

/// State of the connection to the relay. "Off" is not a status: it is the
/// absence of a [`crate::RemoteHost`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayStatus {
    /// Connecting, or waiting to retry after a drop.
    Connecting,
    /// Registered and listening.
    Connected,
    /// Not connected, for the given reason.
    Failed(RelayFailure),
}

/// Why the relay connection is not up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayFailure {
    /// The URL is not `wss://` (nor local `ws://`) or does not normalise.
    InvalidUrl,
    /// DNS, TCP, TLS or a drop; enters the reconnection cycle (the status goes
    /// back to `Connecting`).
    Unreachable,
    /// No operator token in the vault: no connection is attempted.
    TokenMissing,
    /// HTTP 401: stops trying.
    TokenRejected,
    /// Close code 4003: backs off and tries again.
    RelayFull,
    /// HTTP 429: backs off and tries again.
    TooManyConnections,
    /// Close codes 4001, 4004, 4005 or an invalid envelope: stops.
    Protocol,
    /// Close code 4002: another instance with the same identity; long back-off.
    Replaced,
    /// The OS vault is unavailable: remote access does not turn on
    /// (ADR-0106 §1).
    VaultUnavailable,
}

/// Platform a paired device reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePlatform {
    /// Android.
    Android,
    /// iOS.
    Ios,
    /// Anything else.
    Other,
}

/// A paired device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Public static X25519 key of the device.
    pub key: [u8; 32],
    /// Name the device gave itself.
    pub name: String,
    /// Platform the device reported.
    pub platform: DevicePlatform,
    /// RFC 3339 UTC, for example `2026-11-15T14:03:00Z`.
    pub paired_at: String,
    /// RFC 3339 UTC of the last session, if any.
    pub last_seen: Option<String>,
}

/// The user's answer to a pairing request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingAnswer {
    /// Accept the device.
    Accept,
    /// Decline the device.
    Decline,
    /// The dialog was not answered in time.
    Timeout,
}

/// Why the host is stopping, sent to the clients as `goodbye`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// `goodbye{remote-disabled}`.
    Disabled,
    /// `goodbye{host-quitting}`.
    Quitting,
}

/// Input the crate already checked as "pane subscribed by this client".
#[derive(Debug, Clone)]
pub enum RemoteInput {
    /// A key press.
    Key(protocol::message::Key),
    /// Typed text.
    Text(protocol::message::TextInput),
    /// A paste.
    Paste(protocol::message::Paste),
}

/// Network to main thread. Arrives wrapped in `Wakeup::Remote(Box<RemoteEvent>)`.
#[derive(Debug)]
pub enum RemoteEvent {
    /// The relay connection changed state.
    Status(RelayStatus),
    /// The whole list of paired devices, whenever it changes (load, pairing,
    /// revocation, identity recreation, `last_seen`).
    Devices(Vec<DeviceInfo>),
    /// `remote_devices.json` was unreadable and was renamed to `path`
    /// (ADR-0106 §5): the UI shows a notice. A list written by a newer
    /// version stays read-only and raises no notice.
    DevicesQuarantined {
        /// Where the unreadable file was kept.
        path: PathBuf,
    },
    /// Answer to [`Command::ShowPairing`]: the QR URI (from
    /// `PairingUri::to_uri`) and when it stops being valid.
    PairingUri {
        /// The URI the QR encodes.
        uri: String,
        /// End of the token's validity.
        expires_at: Instant,
    },
    /// A valid token was presented; the UI opens the dialog and answers with
    /// [`Command::AnswerPairing`] within 120 s.
    PairingRequested {
        /// Identifies the request.
        id: PairingId,
        /// Name the device gave itself.
        device_name: String,
        /// Platform the device reported.
        platform: DevicePlatform,
        /// Fingerprint of the device key, as `ABCD EFGH IJKL MNOP`.
        fingerprint: String,
    },
    /// The request's channel died before it was answered: the UI closes the
    /// dialog.
    PairingClosed {
        /// Identifies the request.
        id: PairingId,
    },
    /// A client finished the handshake and is in session.
    ClientConnected {
        /// The client's channel.
        client: ClientId,
        /// Public static key of the device.
        device_key: [u8; 32],
        /// Name of the device.
        name: String,
        /// Platform of the device.
        platform: DevicePlatform,
    },
    /// A client's channel is gone.
    ClientDisconnected {
        /// The client's channel.
        client: ClientId,
    },
    /// Sorted, deduplicated union of the panes (protocol ids) some client
    /// subscribes to. Sent on every change.
    Subscriptions(Vec<u64>),
    /// Input for a pane the client subscribes to.
    Input {
        /// The client that sent it.
        client: ClientId,
        /// The input.
        input: RemoteInput,
    },
    /// An action request.
    Action {
        /// The client that sent it.
        client: ClientId,
        /// The action.
        action: protocol::message::Action,
    },
    /// A confirmation answer.
    Confirm {
        /// The client that sent it.
        client: ClientId,
        /// The answer.
        confirm: protocol::message::Confirm,
    },
    /// A scrollback page request.
    Scrollback {
        /// The client that sent it.
        client: ClientId,
        /// The request.
        request: protocol::message::ScrollbackRequest,
    },
}

/// Main thread to network. `RemoteHost::send` never blocks.
#[derive(Debug)]
pub enum Command {
    /// The whole tree. The crate keeps the last one, uses it in the `welcome`
    /// and sends it to every client in session, rewriting `rev` per session.
    /// Panes that left the tree end their subscriptions
    /// (`subscription-ended{pane-closed}`).
    SetWorkspace(protocol::message::Workspace),
    /// Kept, sent in the `welcome` and to everyone when it changes.
    SetTheme(protocol::message::Theme),
    /// Kept, sent in the `welcome` and to everyone when it changes.
    SetProfiles(protocol::message::Profiles),
    /// Live screen of a pane (the `pane` and `seq` fields are ignored; the
    /// crate sets them). Last one wins per pane: never piles up.
    Frame {
        /// Protocol id of the pane.
        pane: u64,
        /// The frame.
        frame: Box<protocol::message::GridSnapshot>,
    },
    /// A message to one client: action-result, confirm-request,
    /// scrollback-page, error.
    Send {
        /// The recipient.
        client: ClientId,
        /// The message.
        message: protocol::message::Message,
    },
    /// A message to every client in session (for example `bell`).
    Broadcast(protocol::message::Message),
    /// Generates a token (invalidating the previous one) and answers with
    /// [`RemoteEvent::PairingUri`].
    ShowPairing,
    /// Discards the live token.
    CancelPairing,
    /// The user's answer to a pairing request.
    AnswerPairing {
        /// Identifies the request.
        id: PairingId,
        /// The answer.
        answer: PairingAnswer,
    },
    /// `goodbye{normal}` and the channel closes.
    Disconnect(ClientId),
    /// Disconnects every client.
    DisconnectAll,
    /// `goodbye{revoked}` to the device's sessions, and removes it from the
    /// list.
    Revoke {
        /// Public static key of the device.
        key: [u8; 32],
    },
    /// Drops everyone, new keys, empty list, new registration.
    RecreateIdentity,
}
