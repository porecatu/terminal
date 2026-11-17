// SPDX-License-Identifier: GPL-3.0-or-later

//! Connection to the relay: registration, reconnection and channel routing.
//!
//! One [`RelayLink`] is one outbound WebSocket to `/v0/host` (spec 01), kept
//! alive for as long as the link lives: it connects, answers the registration
//! challenge with the Ed25519 routing key, multiplexes the channels the relay
//! opens, and reconnects with a growing wait when the connection drops. It
//! knows nothing about what travels in a channel (Noise records, owned by
//! `HostChannel`) and reports only typed events.
//!
//! Rules that hold here (PRD-025 RF-25.3 and RF-25.4, ADR-0103 §2, spec 01):
//!
//! - TLS is `rustls` with the `ring` provider and the operating system roots;
//!   `ws://` only reaches a local relay ([`host_endpoint`]).
//! - Nothing that identifies a payload, token, nonce or key is ever logged.
//!   This module does not log at all; [`LinkEvent`]'s `Debug` prints lengths,
//!   never bytes.
//! - Data for a channel that is not open on the *current* connection is
//!   dropped, never sent: the relay drops a host that talks to a channel it
//!   never opened (`4004`).
//! - No `tokio::select!`: the crate only enables the `macros` feature of
//!   `tokio` for tests, so the loops race their futures by hand (`Stop::run`
//!   and `poll_fn`).
//!
//! # Events around a drop
//!
//! `Down` is emitted whenever a connection whose upgrade succeeded ends, for
//! any reason (including before the registration finished), and always before
//! the status that explains why. A failure before the upgrade (DNS, TCP, TLS,
//! HTTP 401/429) emits no `Down`: no channel could exist. After `Down` the
//! consumer must forget every channel: the relay numbers channels per
//! connection, so a channel id comes back on the next one.

use std::collections::HashSet;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use ed25519_dalek::{Signer, SigningKey};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message, WebSocketConfig};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
};
use zeroize::Zeroizing;

use crate::api::{RelayFailure, RelayStatus};
use crate::protocol::envelope::{
    self, ChannelId, CloseReason, HostEnvelope, MAX_FRAME, PATH_VERSION, Payload, close_code,
    register_message,
};
use crate::protocol::host_id::{HostId, host_id};
use crate::protocol::pairing_uri::{is_secure_relay_url, normalize_relay_url};

/// What a link needs to connect.
pub(crate) struct RelayConfig {
    /// The relay base URL as the user wrote it.
    pub url: String,
    /// The operator token (`Authorization: Bearer`), zeroed on drop.
    pub token: Zeroizing<String>,
    /// The Ed25519 routing key that signs the registration.
    pub signing_key: SigningKey,
}

/// What a link reports. `Down` is explained in the module documentation.
pub(crate) enum LinkEvent {
    /// The relay connection state changed.
    Status(RelayStatus),
    /// The relay accepted the registration; `host_id` is the host address.
    Registered { host_id: HostId },
    /// A client opened a channel.
    ChannelOpened(ChannelId),
    /// One record (opaque to this layer) arrived on a channel.
    ChannelData(ChannelId, Bytes),
    /// A channel closed (the client left, or the relay ended it).
    ChannelClosed(ChannelId, CloseReason),
    /// The connection dropped: every channel died. Comes before a new attempt.
    Down,
}

impl std::fmt::Debug for LinkEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Status(s) => f.debug_tuple("Status").field(s).finish(),
            Self::Registered { host_id } => f
                .debug_struct("Registered")
                .field("host_id", host_id)
                .finish(),
            Self::ChannelOpened(c) => f.debug_tuple("ChannelOpened").field(c).finish(),
            Self::ChannelData(c, b) => f
                .debug_struct("ChannelData")
                .field("channel", c)
                .field("len", &b.len())
                .finish(),
            Self::ChannelClosed(c, r) => f.debug_tuple("ChannelClosed").field(c).field(r).finish(),
            Self::Down => f.write_str("Down"),
        }
    }
}

/// What the host asks of a link.
pub(crate) enum LinkCommand {
    /// Send one record on a channel (dropped if the channel is not open on
    /// the current connection).
    ChannelData(ChannelId, Bytes),
    /// Close a channel.
    CloseChannel(ChannelId, CloseReason),
}

impl std::fmt::Debug for LinkCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChannelData(c, b) => f
                .debug_struct("ChannelData")
                .field("channel", c)
                .field("len", &b.len())
                .finish(),
            Self::CloseChannel(c, r) => f.debug_tuple("CloseChannel").field(c).field(r).finish(),
        }
    }
}

/// Waits and deadlines. Production values in [`Timing::PRODUCTION`]; tests
/// shrink them through [`RelayLink::spawn_with_timing`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    /// First wait of the `Unreachable` cycle; doubles on each failure.
    pub backoff_base: Duration,
    /// Ceiling of the `Unreachable` cycle.
    pub backoff_cap: Duration,
    /// Wait after HTTP 429, close `4003` and close `4005`.
    pub long_wait: Duration,
    /// Wait after close `4002` (another instance holds the identity).
    pub replaced_wait: Duration,
    /// Time to finish the registration, from the upgrade (spec 01 §3).
    pub register_timeout: Duration,
    /// Time to connect (DNS, TCP, TLS, upgrade).
    pub connect_timeout: Duration,
    /// Time a close frame has to leave before the socket is dropped.
    pub close_grace: Duration,
}

impl Timing {
    /// The values the host runs with: 1, 2, 4, 8, 16, 30 s, then 30 s.
    pub(crate) const PRODUCTION: Timing = Timing {
        backoff_base: Duration::from_secs(1),
        backoff_cap: Duration::from_secs(30),
        long_wait: Duration::from_secs(60),
        replaced_wait: Duration::from_secs(300),
        register_timeout: Duration::from_millis(envelope::REGISTER_TIMEOUT_MS),
        connect_timeout: Duration::from_secs(15),
        close_grace: Duration::from_secs(1),
    };
}

/// Longest `shutdown` waits for the task before aborting it.
const SHUTDOWN_LIMIT: Duration = Duration::from_millis(1500);

/// A running relay connection (one task on the host's runtime).
pub(crate) struct RelayLink {
    commands: mpsc::UnboundedSender<LinkCommand>,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl RelayLink {
    /// Starts connecting. Events go to `events` (unbounded: the consumer is
    /// the host loop, which never blocks). Dropping the link without
    /// [`shutdown`](Self::shutdown) also stops the task.
    // The host always passes its own `Tuning`; this is the production default.
    #[allow(dead_code)]
    pub(crate) fn spawn(
        rt: &tokio::runtime::Handle,
        cfg: RelayConfig,
        events: mpsc::UnboundedSender<LinkEvent>,
    ) -> RelayLink {
        Self::spawn_with_timing(rt, cfg, events, Timing::PRODUCTION)
    }

    /// [`spawn`](Self::spawn) with explicit waits (tests).
    pub(crate) fn spawn_with_timing(
        rt: &tokio::runtime::Handle,
        cfg: RelayConfig,
        events: mpsc::UnboundedSender<LinkEvent>,
        timing: Timing,
    ) -> RelayLink {
        let (commands, cmd_rx) = mpsc::unbounded_channel();
        let (stop_tx, stop_rx) = oneshot::channel();
        let ctx = Ctx {
            cfg,
            events,
            cmd_rx,
            stop: Stop {
                rx: stop_rx,
                fired: false,
            },
            timing,
            tls: None,
            step: 0,
        };
        let task = rt.spawn(run(ctx));
        RelayLink {
            commands,
            stop: Some(stop_tx),
            task,
        }
    }

    /// Queues a command without blocking. The queue is unbounded on purpose:
    /// dropping a record silently would break the Noise nonce counter of its
    /// session, so back-pressure is the caller's job (the grid flow control
    /// already limits what is in flight), and a full bounded queue would have
    /// no safe answer. Commands queued while no connection is up are
    /// discarded by the task.
    pub(crate) fn send(&self, cmd: LinkCommand) {
        let _ = self.commands.send(cmd);
    }

    /// Closes the WebSocket with `1000` and ends the task, waiting for the
    /// network no more than about a second and a half.
    pub(crate) async fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let abort = self.task.abort_handle();
        if tokio::time::timeout(SHUTDOWN_LIMIT, &mut self.task)
            .await
            .is_err()
        {
            abort.abort();
        }
    }
}

/// The WebSocket endpoint of the host: the normalised relay base URL plus
/// `/v0/host`. Refuses what is not `wss://` (or `ws://` to a local relay).
pub(crate) fn host_endpoint(relay_url: &str) -> Result<String, RelayFailure> {
    if !is_secure_relay_url(relay_url) {
        return Err(RelayFailure::InvalidUrl);
    }
    let base = normalize_relay_url(relay_url).map_err(|_| RelayFailure::InvalidUrl)?;
    Ok(format!("{base}/{PATH_VERSION}/host"))
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsRead = SplitStream<Ws>;
type WsWrite = SplitSink<Ws, Message>;

/// Shutdown signal: fires when `shutdown` sends, or when the link is dropped.
struct Stop {
    rx: oneshot::Receiver<()>,
    fired: bool,
}

impl Stop {
    /// Runs `fut` until it completes (`Some`) or the shutdown fires (`None`).
    async fn run<F: Future>(&mut self, fut: F) -> Option<F::Output> {
        let mut fut = std::pin::pin!(fut);
        poll_fn(|cx| {
            if self.fired {
                return Poll::Ready(None);
            }
            if Pin::new(&mut self.rx).poll(cx).is_ready() {
                self.fired = true;
                return Poll::Ready(None);
            }
            fut.as_mut().poll(cx).map(Some)
        })
        .await
    }
}

/// Everything one link task owns.
struct Ctx {
    cfg: RelayConfig,
    events: mpsc::UnboundedSender<LinkEvent>,
    cmd_rx: mpsc::UnboundedReceiver<LinkCommand>,
    stop: Stop,
    timing: Timing,
    /// Loaded once, on the first `wss://` attempt.
    tls: Option<Arc<rustls::ClientConfig>>,
    /// Failures in a row of the `Unreachable` cycle; zeroed on registration.
    step: u32,
}

impl Ctx {
    /// `false` when the consumer is gone.
    fn emit(&self, event: LinkEvent) -> bool {
        self.events.send(event).is_ok()
    }

    /// Throws away every queued command: they refer to a connection that is
    /// gone, and the relay numbers channels per connection.
    fn drain_commands(&mut self) {
        while self.cmd_rx.try_recv().is_ok() {}
    }

    /// The wait before the next attempt.
    fn wait_for(&mut self, wait: Wait) -> Duration {
        match wait {
            Wait::Backoff => {
                let factor = 1u32.checked_shl(self.step).unwrap_or(u32::MAX);
                self.step = self.step.saturating_add(1);
                self.timing
                    .backoff_base
                    .saturating_mul(factor)
                    .min(self.timing.backoff_cap)
            }
            Wait::Long => self.timing.long_wait,
            Wait::Replaced => self.timing.replaced_wait,
        }
    }

    /// Sleeps `duration`, discarding commands meanwhile. `false` on shutdown.
    async fn sleep(&mut self, duration: Duration) -> bool {
        let Ctx { stop, cmd_rx, .. } = self;
        stop.run(async {
            let sleep = tokio::time::sleep(duration);
            let mut sleep = std::pin::pin!(sleep);
            poll_fn(|cx| {
                while let Poll::Ready(Some(_)) = cmd_rx.poll_recv(cx) {}
                sleep.as_mut().poll(cx)
            })
            .await;
        })
        .await
        .is_some()
    }
}

/// How long to wait before the next attempt.
#[derive(Debug, Clone, Copy)]
enum Wait {
    Backoff,
    Long,
    Replaced,
}

/// What ended a connection attempt.
#[derive(Debug, Clone, Copy)]
enum Outcome {
    Shutdown,
    /// Report the failure and stop for good.
    Stop(RelayFailure),
    /// Report the failure, wait, and try again.
    Retry(RelayFailure, Wait),
}

/// Why a connection that was open ended.
#[derive(Debug, Clone, Copy)]
enum Ended {
    /// The consumer asked to stop (shutdown, dropped link or dropped events).
    Shutdown,
    /// The relay sent a close frame (with this code, if any).
    PeerClose(Option<u16>),
    /// Read error or end of stream without a close frame.
    Lost,
    /// A frame we cannot use (text, too big, undecodable): we close with this
    /// code and reconnect.
    Garbled(u16),
    /// The relay broke the protocol (envelope out of place, wrong `host_id`):
    /// we close with `1002` and stop.
    RelayBug,
    /// The registration did not finish in time: we close and reconnect.
    RegisterTimeout,
}

/// Close code for our own protocol complaints.
const PROTOCOL_ERROR: u16 = 1002;

fn outcome_for(ended: Ended) -> Outcome {
    match ended {
        Ended::Shutdown => Outcome::Shutdown,
        Ended::Lost | Ended::Garbled(_) | Ended::RegisterTimeout => {
            Outcome::Retry(RelayFailure::Unreachable, Wait::Backoff)
        }
        Ended::RelayBug => Outcome::Stop(RelayFailure::Protocol),
        Ended::PeerClose(code) => match code {
            Some(close_code::REGISTER_REJECTED) | Some(close_code::INVALID_ENVELOPE) => {
                Outcome::Stop(RelayFailure::Protocol)
            }
            Some(close_code::REPLACED) => Outcome::Retry(RelayFailure::Replaced, Wait::Replaced),
            Some(close_code::LIMIT_EXCEEDED) => Outcome::Retry(RelayFailure::RelayFull, Wait::Long),
            Some(close_code::REGISTER_TIMEOUT) => {
                Outcome::Retry(RelayFailure::Protocol, Wait::Long)
            }
            _ => Outcome::Retry(RelayFailure::Unreachable, Wait::Backoff),
        },
    }
}

/// The task body: attempt, report, wait, repeat.
async fn run(mut ctx: Ctx) {
    let endpoint = match host_endpoint(&ctx.cfg.url) {
        Ok(endpoint) => endpoint,
        Err(failure) => {
            ctx.emit(LinkEvent::Status(RelayStatus::Failed(failure)));
            return;
        }
    };
    loop {
        if ctx.events.is_closed() {
            return;
        }
        if !ctx.emit(LinkEvent::Status(RelayStatus::Connecting)) {
            return;
        }
        match attempt(&mut ctx, &endpoint).await {
            Outcome::Shutdown => return,
            Outcome::Stop(failure) => {
                ctx.emit(LinkEvent::Status(RelayStatus::Failed(failure)));
                return;
            }
            Outcome::Retry(failure, wait) => {
                if !ctx.emit(LinkEvent::Status(RelayStatus::Failed(failure))) {
                    return;
                }
                let duration = ctx.wait_for(wait);
                if !ctx.sleep(duration).await {
                    return;
                }
            }
        }
    }
}

/// Command the writer task executes.
enum Out {
    Frame(Bytes),
    Close(u16),
}

/// Owns the write half, so a slow socket never blocks the read loop.
async fn writer_task(mut sink: WsWrite, mut rx: mpsc::UnboundedReceiver<Out>) {
    while let Some(out) = rx.recv().await {
        match out {
            Out::Frame(bytes) => {
                if sink.send(Message::Binary(bytes)).await.is_err() {
                    return;
                }
            }
            Out::Close(code) => {
                let frame = CloseFrame {
                    code: CloseCode::from(code),
                    reason: "".into(),
                };
                let _ = sink.send(Message::Close(Some(frame))).await;
                return;
            }
        }
    }
    let _ = sink.close().await;
}

/// One connection attempt, from `connect` to the end of the connection.
async fn attempt(ctx: &mut Ctx, endpoint: &str) -> Outcome {
    let secure = endpoint.starts_with("wss://");
    let connector = if secure {
        if ctx.tls.is_none() {
            ctx.tls = tls_config().await;
        }
        match &ctx.tls {
            Some(tls) => Some(Connector::Rustls(tls.clone())),
            // No usable root certificate: nothing we could trust.
            None => return Outcome::Retry(RelayFailure::Unreachable, Wait::Backoff),
        }
    } else {
        None
    };

    let request = match build_request(endpoint, &ctx.cfg.token) {
        Ok(request) => request,
        Err(outcome) => return outcome,
    };
    let ws_config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME));

    let timeout = ctx.timing.connect_timeout;
    let connect = tokio::time::timeout(
        timeout,
        connect_async_tls_with_config(request, Some(ws_config), true, connector),
    );
    let ws = match ctx.stop.run(connect).await {
        None => return Outcome::Shutdown,
        Some(Err(_elapsed)) => return Outcome::Retry(RelayFailure::Unreachable, Wait::Backoff),
        Some(Ok(Err(error))) => return outcome_for_connect_error(&error),
        Some(Ok(Ok((ws, _response)))) => ws,
    };

    let (sink, mut stream) = ws.split();
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let writer = tokio::spawn(writer_task(sink, out_rx));

    let ended = session(ctx, &mut stream, &out_tx).await;
    ctx.emit(LinkEvent::Down);

    // Say goodbye when the socket is still usable, then let go.
    let close = match ended {
        Ended::Shutdown => Some(close_code::NORMAL),
        Ended::Garbled(code) => Some(code),
        Ended::RelayBug => Some(PROTOCOL_ERROR),
        Ended::RegisterTimeout => Some(close_code::GOING_AWAY),
        Ended::PeerClose(_) | Ended::Lost => None,
    };
    match close {
        Some(code) => {
            let started = tokio::time::Instant::now();
            let _ = out_tx.send(Out::Close(code));
            drop(out_tx);
            let grace = ctx.timing.close_grace;
            let abort = writer.abort_handle();
            if tokio::time::timeout(grace, writer).await.is_err() {
                abort.abort();
            }
            if code == close_code::MESSAGE_TOO_BIG {
                // The rest of the oversized frame is still unread in the
                // socket, and closing a socket with unread data sends a reset
                // on some systems (Windows): the relay may then lose our close
                // frame before reading it. Hold the read half for the rest of
                // the grace so the 1009 gets there first.
                let left = grace.saturating_sub(started.elapsed());
                let _ = ctx.sleep(left).await;
            }
            drop(stream);
        }
        None => {
            drop(out_tx);
            writer.abort();
        }
    }
    outcome_for(ended)
}

/// Loads the system roots and builds the TLS configuration (`ring` provider,
/// TLS 1.2 and 1.3). `None` when no root certificate could be used.
async fn tls_config() -> Option<Arc<rustls::ClientConfig>> {
    tokio::task::spawn_blocking(|| {
        let loaded = rustls_native_certs::load_native_certs();
        let mut roots = rustls::RootCertStore::empty();
        let (added, _ignored) = roots.add_parsable_certificates(loaded.certs);
        if added == 0 {
            return None;
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(rustls::DEFAULT_VERSIONS)
            .ok()?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Some(Arc::new(config))
    })
    .await
    .ok()
    .flatten()
}

/// The upgrade request with the operator token.
fn build_request(
    endpoint: &str,
    token: &str,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, Outcome> {
    let mut request = endpoint
        .into_client_request()
        .map_err(|_| Outcome::Stop(RelayFailure::InvalidUrl))?;
    // A token that cannot be a header value cannot be accepted either.
    let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| Outcome::Stop(RelayFailure::TokenRejected))?;
    value.set_sensitive(true);
    request.headers_mut().insert("Authorization", value);
    Ok(request)
}

fn outcome_for_connect_error(error: &WsError) -> Outcome {
    match error {
        WsError::Http(response) => match response.status().as_u16() {
            401 => Outcome::Stop(RelayFailure::TokenRejected),
            429 => Outcome::Retry(RelayFailure::TooManyConnections, Wait::Long),
            _ => Outcome::Retry(RelayFailure::Unreachable, Wait::Backoff),
        },
        WsError::Url(_) => Outcome::Stop(RelayFailure::InvalidUrl),
        _ => Outcome::Retry(RelayFailure::Unreachable, Wait::Backoff),
    }
}

/// Polls the next WebSocket message, mapping the ways reading ends.
fn poll_message(stream: &mut WsRead, cx: &mut Context<'_>) -> Poll<Result<Message, Ended>> {
    match stream.poll_next_unpin(cx) {
        Poll::Pending => Poll::Pending,
        Poll::Ready(None) => Poll::Ready(Err(Ended::Lost)),
        Poll::Ready(Some(Ok(message))) => Poll::Ready(Ok(message)),
        Poll::Ready(Some(Err(WsError::Capacity(_)))) => {
            Poll::Ready(Err(Ended::Garbled(close_code::MESSAGE_TOO_BIG)))
        }
        Poll::Ready(Some(Err(_))) => Poll::Ready(Err(Ended::Lost)),
    }
}

/// Turns a message into an envelope. `Ok(None)` for what the library already
/// handled (ping, pong) or what carries nothing.
fn inspect(message: Message) -> Result<Option<HostEnvelope>, Ended> {
    match message {
        Message::Binary(frame) => {
            if frame.len() > MAX_FRAME {
                return Err(Ended::Garbled(close_code::MESSAGE_TOO_BIG));
            }
            match envelope::decode_host(frame) {
                Ok(envelope) => Ok(Some(envelope)),
                Err(_) => Err(Ended::Garbled(PROTOCOL_ERROR)),
            }
        }
        Message::Text(_) => Err(Ended::Garbled(close_code::UNSUPPORTED_DATA)),
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => Ok(None),
        Message::Close(frame) => Err(Ended::PeerClose(frame.map(|f| u16::from(f.code)))),
    }
}

/// What the main loop woke up for.
enum Next {
    Frame(Result<Message, Ended>),
    Command(LinkCommand),
    CommandsGone,
}

/// Registration, then the routing loop, until the connection ends.
async fn session(ctx: &mut Ctx, stream: &mut WsRead, out_tx: &mpsc::UnboundedSender<Out>) -> Ended {
    // Registration (spec 01 §3), under a single deadline from the upgrade.
    let registration = tokio::time::timeout(
        ctx.timing.register_timeout,
        register(&ctx.cfg.signing_key, stream, out_tx),
    );
    let registered = match ctx.stop.run(registration).await {
        None => return Ended::Shutdown,
        Some(Err(_elapsed)) => return Ended::RegisterTimeout,
        Some(Ok(Err(ended))) => return ended,
        Some(Ok(Ok(registered))) => registered,
    };

    ctx.step = 0;
    ctx.drain_commands();
    if !ctx.emit(LinkEvent::Status(RelayStatus::Connected))
        || !ctx.emit(LinkEvent::Registered {
            host_id: registered,
        })
    {
        return Ended::Shutdown;
    }

    let mut open: HashSet<u64> = HashSet::new();
    loop {
        let Ctx { stop, cmd_rx, .. } = &mut *ctx;
        let next = stop
            .run(poll_fn(|cx| {
                if let Poll::Ready(message) = poll_message(stream, cx) {
                    return Poll::Ready(Next::Frame(message));
                }
                match cmd_rx.poll_recv(cx) {
                    Poll::Ready(Some(command)) => Poll::Ready(Next::Command(command)),
                    Poll::Ready(None) => Poll::Ready(Next::CommandsGone),
                    Poll::Pending => Poll::Pending,
                }
            }))
            .await;
        let Some(next) = next else {
            return Ended::Shutdown;
        };
        match next {
            Next::CommandsGone => return Ended::Shutdown,
            Next::Frame(Err(ended)) => return ended,
            Next::Frame(Ok(message)) => {
                let envelope = match inspect(message) {
                    Ok(Some(envelope)) => envelope,
                    Ok(None) => continue,
                    Err(ended) => return ended,
                };
                if let Err(ended) = on_envelope(ctx, &mut open, envelope) {
                    return ended;
                }
            }
            Next::Command(command) => {
                if let Err(ended) = on_command(ctx, &mut open, out_tx, command) {
                    return ended;
                }
            }
        }
    }
}

/// Answers the challenge and waits for `Registered`.
async fn register(
    key: &SigningKey,
    stream: &mut WsRead,
    out_tx: &mpsc::UnboundedSender<Out>,
) -> Result<HostId, Ended> {
    let routing_key = key.verifying_key().to_bytes();
    let expected = host_id(&routing_key);
    let mut challenged = false;
    loop {
        let message = poll_fn(|cx| poll_message(stream, cx)).await?;
        let Some(envelope) = inspect(message)? else {
            continue;
        };
        match envelope {
            HostEnvelope::Challenge { nonce } if !challenged => {
                challenged = true;
                let signature = key.sign(&register_message(&nonce)).to_bytes();
                let frame = envelope::encode_host(&HostEnvelope::Register {
                    routing_key,
                    signature,
                });
                if out_tx.send(Out::Frame(Bytes::from(frame))).is_err() {
                    return Err(Ended::Lost);
                }
            }
            HostEnvelope::Registered { host_id } if challenged => {
                return if host_id == expected {
                    Ok(host_id)
                } else {
                    Err(Ended::RelayBug)
                };
            }
            // Anything else before the registration is the relay's mistake.
            _ => return Err(Ended::RelayBug),
        }
    }
}

/// Handles one envelope of the routing loop.
fn on_envelope(ctx: &Ctx, open: &mut HashSet<u64>, envelope: HostEnvelope) -> Result<(), Ended> {
    match envelope {
        HostEnvelope::ChannelOpened { channel } => {
            // The relay never reuses a number during a connection.
            if !open.insert(channel.0) {
                return Err(Ended::RelayBug);
            }
            send_event(ctx, LinkEvent::ChannelOpened(channel))
        }
        HostEnvelope::ChannelData { channel, payload } => {
            // Data in flight for a channel we already closed: drop it.
            if !open.contains(&channel.0) {
                return Ok(());
            }
            send_event(ctx, LinkEvent::ChannelData(channel, payload.into_bytes()))
        }
        HostEnvelope::ChannelClosed { channel, reason } => {
            // The relay does not answer our own `CloseChannel`, but a close
            // from the client can cross it: only report what was open.
            if !open.remove(&channel.0) {
                return Ok(());
            }
            send_event(ctx, LinkEvent::ChannelClosed(channel, reason))
        }
        // The relay never sends these after the registration.
        HostEnvelope::Challenge { .. }
        | HostEnvelope::Registered { .. }
        | HostEnvelope::Register { .. }
        | HostEnvelope::CloseChannel { .. } => Err(Ended::RelayBug),
    }
}

fn send_event(ctx: &Ctx, event: LinkEvent) -> Result<(), Ended> {
    if ctx.emit(event) {
        Ok(())
    } else {
        Err(Ended::Shutdown)
    }
}

/// Handles one command of the host.
fn on_command(
    ctx: &Ctx,
    open: &mut HashSet<u64>,
    out_tx: &mpsc::UnboundedSender<Out>,
    command: LinkCommand,
) -> Result<(), Ended> {
    let write = |envelope: &HostEnvelope| -> Result<(), Ended> {
        let frame = Bytes::from(envelope::encode_host(envelope));
        out_tx.send(Out::Frame(frame)).map_err(|_| Ended::Lost)
    };
    match command {
        LinkCommand::ChannelData(channel, bytes) => {
            // Never talk to a channel that is not open on this connection:
            // the relay would drop the host with `4004`.
            if !open.contains(&channel.0) {
                return Ok(());
            }
            if envelope::channel_data_len(channel, bytes.len()) > MAX_FRAME {
                // A record that cannot fit a frame cannot be sent, and
                // sending a partial or no record would corrupt the session:
                // end the channel and say so.
                open.remove(&channel.0);
                write(&HostEnvelope::CloseChannel {
                    channel,
                    reason: CloseReason::ProtocolError,
                })?;
                return send_event(
                    ctx,
                    LinkEvent::ChannelClosed(channel, CloseReason::ProtocolError),
                );
            }
            write(&HostEnvelope::ChannelData {
                channel,
                payload: Payload::from_bytes(bytes),
            })
        }
        LinkCommand::CloseChannel(channel, reason) => {
            if !open.remove(&channel.0) {
                return Ok(());
            }
            write(&HostEnvelope::CloseChannel { channel, reason })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::test_support::{ClientEvent, FakeRelay, TestClient, wait_until};

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn timing() -> Timing {
        Timing {
            backoff_base: Duration::from_millis(20),
            backoff_cap: Duration::from_millis(80),
            long_wait: Duration::from_millis(80),
            replaced_wait: Duration::from_millis(120),
            register_timeout: Duration::from_secs(2),
            connect_timeout: Duration::from_secs(2),
            close_grace: Duration::from_millis(500),
        }
    }

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn config(relay: &FakeRelay) -> RelayConfig {
        RelayConfig {
            url: relay.url(),
            token: Zeroizing::new(TOKEN.to_owned()),
            signing_key: key(),
        }
    }

    fn start(relay: &FakeRelay) -> (RelayLink, mpsc::UnboundedReceiver<LinkEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let link = RelayLink::spawn_with_timing(
            &tokio::runtime::Handle::current(),
            config(relay),
            tx,
            timing(),
        );
        (link, rx)
    }

    async fn next(rx: &mut mpsc::UnboundedReceiver<LinkEvent>) -> LinkEvent {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("no event in time")
            .expect("event channel closed")
    }

    async fn expect_status(rx: &mut mpsc::UnboundedReceiver<LinkEvent>, status: RelayStatus) {
        match next(rx).await {
            LinkEvent::Status(got) => assert_eq!(got, status),
            other => panic!("expected {status:?}, got {other:?}"),
        }
    }

    async fn expect_down(rx: &mut mpsc::UnboundedReceiver<LinkEvent>) {
        match next(rx).await {
            LinkEvent::Down => {}
            other => panic!("expected Down, got {other:?}"),
        }
    }

    /// Connecting, Connected and Registered, in that order.
    async fn expect_up(rx: &mut mpsc::UnboundedReceiver<LinkEvent>) -> HostId {
        expect_status(rx, RelayStatus::Connecting).await;
        expect_status(rx, RelayStatus::Connected).await;
        match next(rx).await {
            LinkEvent::Registered { host_id } => host_id,
            other => panic!("expected Registered, got {other:?}"),
        }
    }

    async fn expect_channel_opened(rx: &mut mpsc::UnboundedReceiver<LinkEvent>) -> ChannelId {
        match next(rx).await {
            LinkEvent::ChannelOpened(channel) => channel,
            other => panic!("expected ChannelOpened, got {other:?}"),
        }
    }

    #[test]
    fn host_endpoint_accepts_wss_and_local_ws() {
        assert_eq!(host_endpoint("wss://x"), Ok("wss://x/v0/host".to_owned()));
        assert_eq!(
            host_endpoint("WSS://Relay.Example.com/base/"),
            Ok("wss://relay.example.com/base/v0/host".to_owned())
        );
        assert_eq!(
            host_endpoint("ws://localhost:1"),
            Ok("ws://localhost:1/v0/host".to_owned())
        );
        assert_eq!(
            host_endpoint("ws://127.0.0.1:9000"),
            Ok("ws://127.0.0.1:9000/v0/host".to_owned())
        );
    }

    #[test]
    fn host_endpoint_refuses_insecure_and_malformed_urls() {
        assert_eq!(
            host_endpoint("ws://exemplo.com"),
            Err(RelayFailure::InvalidUrl)
        );
        assert_eq!(
            host_endpoint("http://exemplo.com"),
            Err(RelayFailure::InvalidUrl)
        );
        assert_eq!(host_endpoint(""), Err(RelayFailure::InvalidUrl));
        assert_eq!(
            host_endpoint("wss://user@exemplo.com"),
            Err(RelayFailure::InvalidUrl)
        );
    }

    #[tokio::test]
    async fn invalid_url_fails_without_trying() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let link = RelayLink::spawn_with_timing(
            &tokio::runtime::Handle::current(),
            RelayConfig {
                url: "ws://exemplo.com".to_owned(),
                token: Zeroizing::new(TOKEN.to_owned()),
                signing_key: key(),
            },
            tx,
            timing(),
        );
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::InvalidUrl)).await;
        // The task ended and dropped its sender: no further event.
        assert!(rx.recv().await.is_none());
        link.shutdown().await;
    }

    #[tokio::test]
    async fn registration_emits_connecting_connected_and_registered() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        let id = expect_up(&mut rx).await;
        assert_eq!(id, host_id(&key().verifying_key().to_bytes()));
        assert_eq!(Some(id), relay.host_id());
        assert_eq!(relay.connections(), 1);
        assert_eq!(relay.registrations(), 1);
        link.shutdown().await;
    }

    #[tokio::test]
    async fn rejected_token_stops_without_retrying() {
        let relay = FakeRelay::start(TOKEN).await;
        relay.reject_next_host(401);
        let (link, mut rx) = start(&relay);
        expect_status(&mut rx, RelayStatus::Connecting).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::TokenRejected)).await;
        // The task ended: its sender is gone, and no new attempt came.
        assert!(rx.recv().await.is_none());
        assert_eq!(relay.connections(), 1);
        assert_eq!(relay.registrations(), 0);
        link.shutdown().await;
    }

    #[tokio::test]
    async fn wrong_token_is_a_401() {
        let relay = FakeRelay::start("a-completely-different-token-value-1234").await;
        let (link, mut rx) = start(&relay);
        expect_status(&mut rx, RelayStatus::Connecting).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::TokenRejected)).await;
        assert!(rx.recv().await.is_none());
        link.shutdown().await;
    }

    #[tokio::test]
    async fn too_many_connections_waits_and_tries_again() {
        let relay = FakeRelay::start(TOKEN).await;
        relay.reject_next_host(429);
        let (link, mut rx) = start(&relay);
        expect_status(&mut rx, RelayStatus::Connecting).await;
        expect_status(
            &mut rx,
            RelayStatus::Failed(RelayFailure::TooManyConnections),
        )
        .await;
        expect_up(&mut rx).await;
        assert_eq!(relay.connections(), 2);
        link.shutdown().await;
    }

    #[tokio::test]
    async fn relay_full_reports_and_tries_again() {
        let relay = FakeRelay::start(TOKEN).await;
        relay.fail_next_registration(4003);
        let (link, mut rx) = start(&relay);
        expect_status(&mut rx, RelayStatus::Connecting).await;
        expect_down(&mut rx).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::RelayFull)).await;
        expect_up(&mut rx).await;
        assert_eq!(relay.connections(), 2);
        assert_eq!(relay.registrations(), 1);
        link.shutdown().await;
    }

    #[tokio::test]
    async fn rejected_registration_is_a_protocol_failure_and_stops() {
        let relay = FakeRelay::start(TOKEN).await;
        relay.fail_next_registration(4001);
        let (link, mut rx) = start(&relay);
        expect_status(&mut rx, RelayStatus::Connecting).await;
        expect_down(&mut rx).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Protocol)).await;
        assert!(rx.recv().await.is_none());
        assert_eq!(relay.connections(), 1);
        link.shutdown().await;
    }

    #[tokio::test]
    async fn registration_timeout_code_waits_and_tries_again() {
        let relay = FakeRelay::start(TOKEN).await;
        relay.fail_next_registration(4005);
        let (link, mut rx) = start(&relay);
        expect_status(&mut rx, RelayStatus::Connecting).await;
        expect_down(&mut rx).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Protocol)).await;
        expect_up(&mut rx).await;
        link.shutdown().await;
    }

    #[tokio::test]
    async fn replaced_waits_and_tries_again() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        expect_up(&mut rx).await;
        relay.close_host(4002);
        expect_down(&mut rx).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Replaced)).await;
        expect_up(&mut rx).await;
        assert_eq!(relay.registrations(), 2);
        link.shutdown().await;
    }

    #[tokio::test]
    async fn invalid_envelope_code_is_a_protocol_failure_and_stops() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        expect_up(&mut rx).await;
        relay.close_host(4004);
        expect_down(&mut rx).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Protocol)).await;
        assert!(rx.recv().await.is_none());
        link.shutdown().await;
    }

    #[tokio::test]
    async fn a_drop_emits_down_and_reconnects() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        expect_up(&mut rx).await;
        relay.drop_host();
        expect_down(&mut rx).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Unreachable)).await;
        expect_up(&mut rx).await;
        assert_eq!(relay.registrations(), 2);
        link.shutdown().await;
    }

    #[tokio::test]
    async fn going_away_close_reconnects() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        expect_up(&mut rx).await;
        relay.close_host(1001);
        expect_down(&mut rx).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Unreachable)).await;
        expect_up(&mut rx).await;
        link.shutdown().await;
    }

    #[tokio::test]
    async fn unreachable_relay_backs_off_and_recovers() {
        // Nothing listens on the port while the link tries.
        let relay = FakeRelay::start(TOKEN).await;
        let url = relay.url();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let port = url.rsplit(':').next().unwrap().to_owned();
        drop(relay);
        // Let the aborted accept task release the listener.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let link = RelayLink::spawn_with_timing(
            &tokio::runtime::Handle::current(),
            RelayConfig {
                url: format!("ws://127.0.0.1:{port}"),
                token: Zeroizing::new(TOKEN.to_owned()),
                signing_key: key(),
            },
            tx,
            timing(),
        );
        for _ in 0..3 {
            expect_status(&mut rx, RelayStatus::Connecting).await;
            expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Unreachable)).await;
        }
        link.shutdown().await;
    }

    #[tokio::test]
    async fn channels_flow_both_ways() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        let id = expect_up(&mut rx).await;

        let mut client = TestClient::connect(&relay.url(), &id).await;
        let channel = expect_channel_opened(&mut rx).await;

        client.send(Bytes::from_static(b"from the phone")).await;
        match next(&mut rx).await {
            LinkEvent::ChannelData(got, bytes) => {
                assert_eq!(got, channel);
                assert_eq!(&bytes[..], b"from the phone");
            }
            other => panic!("expected ChannelData, got {other:?}"),
        }

        link.send(LinkCommand::ChannelData(
            channel,
            Bytes::from_static(b"from the desktop"),
        ));
        match client.recv().await {
            ClientEvent::Data(bytes) => assert_eq!(&bytes[..], b"from the desktop"),
            other => panic!("expected Data, got {other:?}"),
        }

        // The host closes: the client is told, and its socket closes.
        link.send(LinkCommand::CloseChannel(
            channel,
            CloseReason::HostRejected,
        ));
        match client.recv().await {
            ClientEvent::Closed(reason) => assert_eq!(reason, CloseReason::HostRejected),
            other => panic!("expected Closed, got {other:?}"),
        }
        assert!(matches!(client.recv().await, ClientEvent::WsClosed(_)));
        link.shutdown().await;
    }

    #[tokio::test]
    async fn a_client_leaving_closes_the_channel() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        let id = expect_up(&mut rx).await;

        let client = TestClient::connect(&relay.url(), &id).await;
        let channel = expect_channel_opened(&mut rx).await;
        client.close().await;
        match next(&mut rx).await {
            LinkEvent::ChannelClosed(got, reason) => {
                assert_eq!(got, channel);
                assert_eq!(reason, CloseReason::Normal);
            }
            other => panic!("expected ChannelClosed, got {other:?}"),
        }
        link.shutdown().await;
    }

    #[tokio::test]
    async fn several_channels_are_independent() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        let id = expect_up(&mut rx).await;

        let mut a = TestClient::connect(&relay.url(), &id).await;
        let ca = expect_channel_opened(&mut rx).await;
        let mut b = TestClient::connect(&relay.url(), &id).await;
        let cb = expect_channel_opened(&mut rx).await;
        assert_ne!(ca, cb);

        link.send(LinkCommand::ChannelData(cb, Bytes::from_static(b"to b")));
        link.send(LinkCommand::ChannelData(ca, Bytes::from_static(b"to a")));
        match (a.recv().await, b.recv().await) {
            (ClientEvent::Data(x), ClientEvent::Data(y)) => {
                assert_eq!(&x[..], b"to a");
                assert_eq!(&y[..], b"to b");
            }
            other => panic!("unexpected {other:?}"),
        }
        link.shutdown().await;
    }

    #[tokio::test]
    async fn data_for_a_channel_that_is_not_open_is_not_sent() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        let id = expect_up(&mut rx).await;

        // Never opened: the fake (like the real relay) would drop the host.
        link.send(LinkCommand::ChannelData(
            ChannelId(99),
            Bytes::from_static(b"nobody"),
        ));
        link.send(LinkCommand::CloseChannel(
            ChannelId(98),
            CloseReason::Normal,
        ));

        // Opened, then closed by us: late data is dropped too.
        let mut client = TestClient::connect(&relay.url(), &id).await;
        let channel = expect_channel_opened(&mut rx).await;
        link.send(LinkCommand::CloseChannel(channel, CloseReason::Normal));
        link.send(LinkCommand::ChannelData(
            channel,
            Bytes::from_static(b"too late"),
        ));
        match client.recv().await {
            ClientEvent::Closed(CloseReason::Normal) => {}
            other => panic!("expected Closed, got {other:?}"),
        }

        // The connection survived all of it, and still works.
        let mut second = TestClient::connect(&relay.url(), &id).await;
        let channel = expect_channel_opened(&mut rx).await;
        link.send(LinkCommand::ChannelData(channel, Bytes::from_static(b"ok")));
        match second.recv().await {
            ClientEvent::Data(bytes) => assert_eq!(&bytes[..], b"ok"),
            other => panic!("expected Data, got {other:?}"),
        }
        assert_eq!(relay.invalid_envelopes(), 0);
        assert_eq!(relay.connections(), 1);
        link.shutdown().await;
    }

    #[tokio::test]
    async fn a_drop_ends_every_channel_and_stale_commands_do_not_leak() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        let id = expect_up(&mut rx).await;

        let mut client = TestClient::connect(&relay.url(), &id).await;
        let channel = expect_channel_opened(&mut rx).await;
        relay.drop_host();
        expect_down(&mut rx).await;
        // The client of the dead connection is told the host is gone.
        match client.recv().await {
            ClientEvent::Closed(CloseReason::HostGone) => {}
            other => panic!("expected Closed(HostGone), got {other:?}"),
        }
        // Commands for the dead connection, sent while the link waits.
        link.send(LinkCommand::ChannelData(
            channel,
            Bytes::from_static(b"stale"),
        ));
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Unreachable)).await;
        expect_up(&mut rx).await;

        // A new client gets channel 0 again; the stale record must not show.
        let mut fresh = TestClient::connect(&relay.url(), &id).await;
        let fresh_channel = expect_channel_opened(&mut rx).await;
        link.send(LinkCommand::ChannelData(
            fresh_channel,
            Bytes::from_static(b"fresh"),
        ));
        match fresh.recv().await {
            ClientEvent::Data(bytes) => assert_eq!(&bytes[..], b"fresh"),
            other => panic!("expected Data, got {other:?}"),
        }
        link.shutdown().await;
    }

    #[tokio::test]
    async fn a_text_frame_closes_with_1003_and_reconnects() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        expect_up(&mut rx).await;
        relay.inject_host_text("hello");
        expect_down(&mut rx).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Unreachable)).await;
        expect_up(&mut rx).await;
        assert!(wait_until(|| relay.host_close_codes().contains(&Some(1003))).await);
        link.shutdown().await;
    }

    #[tokio::test]
    async fn an_oversized_frame_closes_with_1009_and_reconnects() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        expect_up(&mut rx).await;
        relay.inject_host_frame(vec![0u8; MAX_FRAME + 1]);
        expect_down(&mut rx).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Unreachable)).await;
        expect_up(&mut rx).await;
        assert!(wait_until(|| relay.host_close_codes().contains(&Some(1009))).await);
        link.shutdown().await;
    }

    #[tokio::test]
    async fn an_undecodable_envelope_closes_and_reconnects() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        expect_up(&mut rx).await;
        relay.inject_host_frame(vec![0xff, 0xff, 0xff]);
        expect_down(&mut rx).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Unreachable)).await;
        expect_up(&mut rx).await;
        link.shutdown().await;
    }

    #[tokio::test]
    async fn an_envelope_out_of_place_is_a_protocol_failure_and_stops() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        let id = expect_up(&mut rx).await;
        relay.inject_host_frame(envelope::encode_host(&HostEnvelope::Registered {
            host_id: id,
        }));
        expect_down(&mut rx).await;
        expect_status(&mut rx, RelayStatus::Failed(RelayFailure::Protocol)).await;
        assert!(rx.recv().await.is_none());
        link.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_is_quick_and_leaves_the_relay() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        let id = expect_up(&mut rx).await;
        let mut client = TestClient::connect(&relay.url(), &id).await;
        expect_channel_opened(&mut rx).await;

        let started = std::time::Instant::now();
        link.shutdown().await;
        assert!(started.elapsed() < Duration::from_millis(1600));

        assert!(wait_until(|| !relay.has_host()).await);
        match client.recv().await {
            ClientEvent::Closed(CloseReason::HostGone) => {}
            other => panic!("expected Closed(HostGone), got {other:?}"),
        }
        assert!(relay.host_close_codes().contains(&Some(1000)));
    }

    #[tokio::test]
    async fn shutdown_interrupts_a_long_wait() {
        let relay = FakeRelay::start(TOKEN).await;
        relay.reject_next_host(429);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut slow = timing();
        slow.long_wait = Duration::from_secs(3600);
        let link = RelayLink::spawn_with_timing(
            &tokio::runtime::Handle::current(),
            config(&relay),
            tx,
            slow,
        );
        expect_status(&mut rx, RelayStatus::Connecting).await;
        expect_status(
            &mut rx,
            RelayStatus::Failed(RelayFailure::TooManyConnections),
        )
        .await;
        let started = std::time::Instant::now();
        link.shutdown().await;
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn dropping_the_link_stops_the_task() {
        let relay = FakeRelay::start(TOKEN).await;
        let (link, mut rx) = start(&relay);
        expect_up(&mut rx).await;
        drop(link);
        assert!(wait_until(|| !relay.has_host()).await);
    }

    #[test]
    fn backoff_doubles_up_to_the_cap_and_resets() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_stop_tx, stop_rx) = oneshot::channel();
        let (_cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let mut ctx = Ctx {
            cfg: RelayConfig {
                url: String::new(),
                token: Zeroizing::new(String::new()),
                signing_key: key(),
            },
            events: tx,
            cmd_rx,
            stop: Stop {
                rx: stop_rx,
                fired: false,
            },
            timing: Timing::PRODUCTION,
            tls: None,
            step: 0,
        };
        let secs: Vec<u64> = (0..8)
            .map(|_| ctx.wait_for(Wait::Backoff).as_secs())
            .collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(ctx.wait_for(Wait::Long), Duration::from_secs(60));
        assert_eq!(ctx.wait_for(Wait::Replaced), Duration::from_secs(300));
        ctx.step = 0;
        assert_eq!(ctx.wait_for(Wait::Backoff), Duration::from_secs(1));
        // A very long streak does not overflow.
        ctx.step = u32::MAX;
        assert_eq!(ctx.wait_for(Wait::Backoff), Duration::from_secs(30));
    }

    #[test]
    fn close_codes_map_to_failures() {
        let case = |code| match outcome_for(Ended::PeerClose(Some(code))) {
            Outcome::Stop(f) => (f, None),
            Outcome::Retry(f, w) => (f, Some(w)),
            Outcome::Shutdown => panic!("shutdown"),
        };
        assert_eq!(case(4001).0, RelayFailure::Protocol);
        assert!(case(4001).1.is_none());
        assert_eq!(case(4004).0, RelayFailure::Protocol);
        assert!(case(4004).1.is_none());
        assert_eq!(case(4002).0, RelayFailure::Replaced);
        assert_eq!(case(4003).0, RelayFailure::RelayFull);
        assert_eq!(case(4005).0, RelayFailure::Protocol);
        assert!(case(4005).1.is_some());
        assert_eq!(case(1001).0, RelayFailure::Unreachable);
        assert_eq!(case(1000).0, RelayFailure::Unreachable);
    }
}
