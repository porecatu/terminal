// SPDX-License-Identifier: GPL-3.0-or-later

//! An in-memory relay on `127.0.0.1` for tests (spec 01).
//!
//! **What it covers** (the relay side of spec 01, enough for the host and for
//! clients): the `/v0/host` upgrade with the `Authorization: Bearer` check
//! (HTTP 401); the registration challenge with a random nonce, the Ed25519
//! check of the signature and `Registered { host_id }`; per-connection channel
//! numbering (from 0, never reused during a connection); `/v0/client/<host_id>`
//! with `channel-opened`, `data` <-> `channel-data`, `channel-closed { normal }`
//! when the client leaves, `closed { reason }` plus the WebSocket close when
//! the host sends `close-channel`, `closed { host-gone }` when the host
//! connection drops, `closed { host-replaced }` plus `4002` for the old host
//! when the same routing key registers again, `host-unavailable` for an
//! unknown `host_id`, and `4004` when the host talks to a channel that was
//! never opened or sends an envelope it must not.
//!
//! **Hooks**: answer the next host connection with an HTTP status
//! ([`reject_next_host`](FakeRelay::reject_next_host)); close the next
//! registration with a code ([`fail_next_registration`](FakeRelay::fail_next_registration));
//! close the live host connection with a code ([`close_host`](FakeRelay::close_host)),
//! or drop it with no close frame ([`drop_host`](FakeRelay::drop_host));
//! inject a raw binary or text frame towards the host; counters and the
//! registered `host_id`.
//!
//! **What it does not cover**: TLS (plain `ws://` on loopback only), the
//! presence query, the 25 s ping and 60 s pong deadline, the registration
//! deadline (`4005` is only available through the hook), the limits of the
//! relay (channels per host, bandwidth, frame size), the max number of hosts
//! (`4003` is only available through the hook), the operator tokens list (one
//! expected token) and the client-side envelope errors (`protocol-error`).
//! One host at a time per routing key; no logging.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use ring::rand::{SecureRandom, SystemRandom};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message};
use tokio_tungstenite::{WebSocketStream, accept_hdr_async};

use crate::protocol::envelope::{
    self, ChannelId, ClientEnvelope, CloseReason, HostEnvelope, NONCE_LEN, Payload, close_code,
    register_message,
};
use crate::protocol::host_id::{HostId, host_id};

type Ws = WebSocketStream<TcpStream>;

/// Something to do on the host connection.
enum HostOut {
    Frame(Vec<u8>),
    Text(String),
    /// Send a close frame with this code and end.
    Close(u16),
    /// End without a close frame.
    Drop,
}

/// Something to do on a client connection.
enum ClientOut {
    Data(Bytes),
    Closed(CloseReason),
}

/// One live host connection.
struct HostConn {
    tx: mpsc::UnboundedSender<HostOut>,
    clients: Mutex<HashMap<u64, mpsc::UnboundedSender<ClientOut>>>,
    next_channel: AtomicU64,
    /// What the clients are told when this connection ends.
    end_reason: Mutex<CloseReason>,
}

#[derive(Default)]
struct State {
    connections: usize,
    registrations: usize,
    client_connections: usize,
    invalid_envelopes: usize,
    reject_next: VecDeque<u16>,
    fail_next_registration: VecDeque<u16>,
    host: Option<Arc<HostConn>>,
    host_id: Option<HostId>,
    host_close_codes: Vec<Option<u16>>,
}

struct Shared {
    token: String,
    state: Mutex<State>,
}

impl Shared {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The fake relay. Dropping it stops accepting connections.
pub(crate) struct FakeRelay {
    addr: SocketAddr,
    shared: Arc<Shared>,
    accept: JoinHandle<()>,
}

impl Drop for FakeRelay {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

impl FakeRelay {
    /// Starts listening on `127.0.0.1:0`; hosts must present `token`.
    pub(crate) async fn start(token: &str) -> FakeRelay {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let shared = Arc::new(Shared {
            token: token.to_owned(),
            state: Mutex::new(State::default()),
        });
        let accept = tokio::spawn({
            let shared = shared.clone();
            async move {
                while let Ok((socket, _)) = listener.accept().await {
                    tokio::spawn(connection(shared.clone(), socket));
                }
            }
        });
        FakeRelay {
            addr,
            shared,
            accept,
        }
    }

    /// The base URL, `ws://127.0.0.1:<port>`.
    pub(crate) fn url(&self) -> String {
        format!("ws://{}", self.addr)
    }

    /// The `host_id` of the last registered host.
    pub(crate) fn host_id(&self) -> Option<HostId> {
        self.shared.state().host_id.clone()
    }

    /// Whether a host connection is registered right now.
    pub(crate) fn has_host(&self) -> bool {
        self.shared.state().host.is_some()
    }

    /// Host connections accepted at the HTTP level, rejected ones included.
    pub(crate) fn connections(&self) -> usize {
        self.shared.state().connections
    }

    /// Registrations that ended in `Registered`.
    pub(crate) fn registrations(&self) -> usize {
        self.shared.state().registrations
    }

    /// Client connections that reached the upgrade.
    #[allow(dead_code)]
    pub(crate) fn client_connections(&self) -> usize {
        self.shared.state().client_connections
    }

    /// Host connections closed with `4004` for an invalid envelope.
    pub(crate) fn invalid_envelopes(&self) -> usize {
        self.shared.state().invalid_envelopes
    }

    /// Channels open on the live host connection.
    pub(crate) fn open_channels(&self) -> usize {
        let host = self.shared.state().host.clone();
        host.map_or(0, |h| h.clients.lock().map_or(0, |c| c.len()))
    }

    /// The close codes the host sent, one entry per close frame it sent
    /// (`None` for a close frame without a code).
    pub(crate) fn host_close_codes(&self) -> Vec<Option<u16>> {
        self.shared.state().host_close_codes.clone()
    }

    /// Answers the next host connection with this HTTP status (401, 429...).
    pub(crate) fn reject_next_host(&self, status: u16) {
        self.shared.state().reject_next.push_back(status);
    }

    /// Closes the next registration with this WebSocket code, right after the
    /// host answers the challenge (4003 relay full, 4001, 4005...).
    pub(crate) fn fail_next_registration(&self, code: u16) {
        self.shared.state().fail_next_registration.push_back(code);
    }

    /// Closes the live host connection with `code`.
    pub(crate) fn close_host(&self, code: u16) {
        self.to_host(HostOut::Close(code));
    }

    /// Drops the live host connection without a close frame.
    pub(crate) fn drop_host(&self) {
        self.to_host(HostOut::Drop);
    }

    /// Sends this binary frame to the live host as is.
    pub(crate) fn inject_host_frame(&self, frame: Vec<u8>) {
        self.to_host(HostOut::Frame(frame));
    }

    /// Sends this text frame to the live host.
    pub(crate) fn inject_host_text(&self, text: &str) {
        self.to_host(HostOut::Text(text.to_owned()));
    }

    fn to_host(&self, out: HostOut) {
        let host = self.shared.state().host.clone();
        if let Some(host) = host {
            let _ = host.tx.send(out);
        }
    }
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

enum Route {
    Host,
    Client(HostId),
}

fn http_error(status: u16) -> ErrorResponse {
    http::Response::builder()
        .status(status)
        .body(None)
        .expect("response")
}

/// Upgrade, route and hand the socket to the right handler.
// `tungstenite`'s `Callback` fixes the error type (`ErrorResponse`).
#[allow(clippy::result_large_err)]
async fn connection(shared: Arc<Shared>, socket: TcpStream) {
    let route: Arc<Mutex<Option<Route>>> = Arc::new(Mutex::new(None));
    let callback = {
        let shared = shared.clone();
        let route = route.clone();
        move |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
            let path = request.uri().path().to_owned();
            if path == "/v0/host" {
                let mut state = shared.state();
                state.connections += 1;
                if let Some(status) = state.reject_next.pop_front() {
                    return Err(http_error(status));
                }
                drop(state);
                let expected = format!("Bearer {}", shared.token);
                let presented = request
                    .headers()
                    .get("Authorization")
                    .and_then(|v| v.to_str().ok());
                if presented != Some(expected.as_str()) {
                    return Err(http_error(401));
                }
                *route.lock().unwrap() = Some(Route::Host);
                Ok(response)
            } else if let Some(id) = path.strip_prefix("/v0/client/") {
                match HostId::parse(id) {
                    Ok(id) => {
                        *route.lock().unwrap() = Some(Route::Client(id));
                        Ok(response)
                    }
                    Err(_) => Err(http_error(400)),
                }
            } else {
                Err(http_error(404))
            }
        }
    };
    let Ok(ws) = accept_hdr_async(socket, callback).await else {
        return;
    };
    let route = route.lock().unwrap().take();
    match route {
        Some(Route::Host) => host_connection(shared, ws).await,
        Some(Route::Client(id)) => {
            shared.state().client_connections += 1;
            client_connection(shared, ws, id).await;
        }
        None => {}
    }
}

fn binary(frame: Vec<u8>) -> Message {
    Message::Binary(Bytes::from(frame))
}

fn close_message(code: u16) -> Message {
    Message::Close(Some(CloseFrame {
        code: CloseCode::from(code),
        reason: "".into(),
    }))
}

/// Sends a close frame and reads until the peer answers (or a short while
/// passes), so the frame is not lost to a reset.
async fn close_with(ws: &mut Ws, code: u16) {
    let _ = ws.send(close_message(code)).await;
    let drain = async { while let Some(Ok(_)) = ws.next().await {} };
    let _ = tokio::time::timeout(Duration::from_millis(300), drain).await;
}

/// The host side: challenge, registration, then channels.
async fn host_connection(shared: Arc<Shared>, mut ws: Ws) {
    // Challenge.
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new().fill(&mut nonce).expect("random");
    if ws
        .send(binary(envelope::encode_host(&HostEnvelope::Challenge {
            nonce,
        })))
        .await
        .is_err()
    {
        return;
    }

    // Register, within a generous deadline (the real one is the relay's).
    let Ok(Some(Ok(message))) = tokio::time::timeout(Duration::from_secs(10), ws.next()).await
    else {
        return;
    };
    let Message::Binary(frame) = message else {
        close_with(&mut ws, close_code::INVALID_ENVELOPE).await;
        return;
    };
    let Ok(HostEnvelope::Register {
        routing_key,
        signature,
    }) = envelope::decode_host(frame)
    else {
        shared.state().invalid_envelopes += 1;
        close_with(&mut ws, close_code::INVALID_ENVELOPE).await;
        return;
    };
    let signature_ok = VerifyingKey::from_bytes(&routing_key)
        .and_then(|key| {
            key.verify(
                &register_message(&nonce),
                &Signature::from_bytes(&signature),
            )
        })
        .is_ok();
    if !signature_ok {
        close_with(&mut ws, close_code::REGISTER_REJECTED).await;
        return;
    }
    let forced = shared.state().fail_next_registration.pop_front();
    if let Some(code) = forced {
        close_with(&mut ws, code).await;
        return;
    }

    // Registered; the last registration wins.
    let id = host_id(&routing_key);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let conn = Arc::new(HostConn {
        tx,
        clients: Mutex::new(HashMap::new()),
        next_channel: AtomicU64::new(0),
        end_reason: Mutex::new(CloseReason::HostGone),
    });
    let old = {
        let mut state = shared.state();
        state.registrations += 1;
        state.host_id = Some(id.clone());
        state.host.replace(conn.clone())
    };
    if let Some(old) = old {
        *old.end_reason.lock().unwrap() = CloseReason::HostReplaced;
        let _ = old.tx.send(HostOut::Close(close_code::REPLACED));
    }
    let registered = envelope::encode_host(&HostEnvelope::Registered { host_id: id });
    if ws.send(binary(registered)).await.is_err() {
        finish_host(&shared, &conn);
        return;
    }

    // Channels.
    loop {
        tokio::select! {
            incoming = ws.next() => {
                let Some(Ok(message)) = incoming else { break };
                match message {
                    Message::Binary(frame) => {
                        if !on_host_frame(&conn, frame) {
                            shared.state().invalid_envelopes += 1;
                            close_with(&mut ws, close_code::INVALID_ENVELOPE).await;
                            break;
                        }
                    }
                    Message::Text(_) => {
                        close_with(&mut ws, close_code::UNSUPPORTED_DATA).await;
                        break;
                    }
                    Message::Close(frame) => {
                        shared
                            .state()
                            .host_close_codes
                            .push(frame.map(|f| u16::from(f.code)));
                        break;
                    }
                    _ => {}
                }
            }
            out = rx.recv() => {
                match out {
                    Some(HostOut::Frame(frame)) => {
                        if ws.send(binary(frame)).await.is_err() { break }
                    }
                    Some(HostOut::Text(text)) => {
                        if ws.send(Message::text(text)).await.is_err() { break }
                    }
                    Some(HostOut::Close(code)) => {
                        close_with(&mut ws, code).await;
                        break;
                    }
                    Some(HostOut::Drop) | None => break,
                }
            }
        }
    }
    finish_host(&shared, &conn);
}

/// Handles one binary frame from the host. `false` means "invalid envelope".
fn on_host_frame(conn: &HostConn, frame: Bytes) -> bool {
    let Ok(envelope) = envelope::decode_host(frame) else {
        return false;
    };
    let (channel, action) = match envelope {
        HostEnvelope::ChannelData { channel, payload } => {
            (channel, ClientOut::Data(payload.into_bytes()))
        }
        HostEnvelope::CloseChannel { channel, reason } => (channel, ClientOut::Closed(reason)),
        _ => return false,
    };
    let closing = matches!(action, ClientOut::Closed(_));
    let mut clients = conn.clients.lock().unwrap();
    match clients.get(&channel.0) {
        Some(client) => {
            let _ = client.send(action);
            if closing {
                clients.remove(&channel.0);
            }
            true
        }
        // Already closed (a race with the client): ignored. Never opened:
        // invalid envelope.
        None => channel.0 < conn.next_channel.load(Ordering::SeqCst),
    }
}

/// The host connection ended: leave the registry and tell the clients.
fn finish_host(shared: &Shared, conn: &Arc<HostConn>) {
    {
        let mut state = shared.state();
        if state.host.as_ref().is_some_and(|h| Arc::ptr_eq(h, conn)) {
            state.host = None;
        }
    }
    let reason = *conn.end_reason.lock().unwrap();
    let clients: Vec<_> = conn.clients.lock().unwrap().drain().collect();
    for (_, client) in clients {
        let _ = client.send(ClientOut::Closed(reason));
    }
}

/// The client side: one channel for the life of the WebSocket.
async fn client_connection(shared: Arc<Shared>, mut ws: Ws, wanted: HostId) {
    let host = {
        let state = shared.state();
        match (&state.host, &state.host_id) {
            (Some(host), Some(id)) if *id == wanted => Some(host.clone()),
            _ => None,
        }
    };
    let Some(host) = host else {
        let _ = ws
            .send(binary(envelope::encode_client(
                &ClientEnvelope::HostUnavailable,
            )))
            .await;
        close_with(&mut ws, close_code::NORMAL).await;
        return;
    };

    let channel = host.next_channel.fetch_add(1, Ordering::SeqCst);
    let (tx, mut rx) = mpsc::unbounded_channel();
    host.clients.lock().unwrap().insert(channel, tx);
    let _ = host.tx.send(HostOut::Frame(envelope::encode_host(
        &HostEnvelope::ChannelOpened {
            channel: ChannelId(channel),
        },
    )));

    loop {
        tokio::select! {
            incoming = ws.next() => {
                let message = match incoming {
                    Some(Ok(message)) => message,
                    _ => {
                        leave(&host, channel);
                        break;
                    }
                };
                match message {
                    Message::Binary(frame) => match envelope::decode_client(frame) {
                        Ok(ClientEnvelope::Data { payload }) => {
                            let _ = host.tx.send(HostOut::Frame(envelope::encode_host(
                                &HostEnvelope::ChannelData {
                                    channel: ChannelId(channel),
                                    payload,
                                },
                            )));
                        }
                        _ => {
                            let _ = ws
                                .send(binary(envelope::encode_client(&ClientEnvelope::Closed {
                                    reason: CloseReason::ProtocolError,
                                })))
                                .await;
                            close_with(&mut ws, close_code::INVALID_ENVELOPE).await;
                            leave_with(&host, channel, CloseReason::ProtocolError);
                            break;
                        }
                    },
                    Message::Close(_) => {
                        leave(&host, channel);
                        break;
                    }
                    Message::Text(_) => {
                        close_with(&mut ws, close_code::UNSUPPORTED_DATA).await;
                        leave(&host, channel);
                        break;
                    }
                    _ => {}
                }
            }
            out = rx.recv() => {
                match out {
                    Some(ClientOut::Data(payload)) => {
                        let frame = envelope::encode_client(&ClientEnvelope::Data {
                            payload: Payload::from_bytes(payload),
                        });
                        if ws.send(binary(frame)).await.is_err() {
                            leave(&host, channel);
                            break;
                        }
                    }
                    Some(ClientOut::Closed(reason)) => {
                        let frame = envelope::encode_client(&ClientEnvelope::Closed { reason });
                        let _ = ws.send(binary(frame)).await;
                        close_with(&mut ws, reason.client_close_code()).await;
                        break;
                    }
                    None => break,
                }
            }
        }
    }
}

/// The client left: tell the host `channel-closed { normal }`, unless the
/// channel was already closed from the host's side.
fn leave(host: &HostConn, channel: u64) {
    leave_with(host, channel, CloseReason::Normal);
}

fn leave_with(host: &HostConn, channel: u64, reason: CloseReason) {
    let still_open = host.clients.lock().unwrap().remove(&channel).is_some();
    if still_open {
        let _ = host.tx.send(HostOut::Frame(envelope::encode_host(
            &HostEnvelope::ChannelClosed {
                channel: ChannelId(channel),
                reason,
            },
        )));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ed25519_dalek::{Signer, SigningKey};

    use super::*;
    use crate::test_support::{ClientEvent, TestClient, wait_until};

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// A bare host: speaks the host half of spec 01 by hand.
    async fn raw_host(relay: &FakeRelay, key: &SigningKey, token: &str) -> Ws2 {
        let mut request =
            tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
                format!("{}/v0/host", relay.url()).as_str(),
            )
            .unwrap();
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {token}").parse().unwrap());
        let (mut ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let Message::Binary(frame) = ws.next().await.unwrap().unwrap() else {
            panic!("expected a binary challenge");
        };
        let HostEnvelope::Challenge { nonce } = envelope::decode_host(frame).unwrap() else {
            panic!("expected Challenge");
        };
        let signature = key.sign(&register_message(&nonce)).to_bytes();
        let register = HostEnvelope::Register {
            routing_key: key.verifying_key().to_bytes(),
            signature,
        };
        ws.send(binary(envelope::encode_host(&register)))
            .await
            .unwrap();
        ws
    }

    type Ws2 = WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

    async fn next_host_envelope(ws: &mut Ws2) -> HostEnvelope {
        loop {
            match ws.next().await.unwrap().unwrap() {
                Message::Binary(frame) => return envelope::decode_host(frame).unwrap(),
                Message::Ping(_) | Message::Pong(_) => continue,
                other => panic!("unexpected message {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn registers_a_host_and_routes_one_channel() {
        let relay = FakeRelay::start(TOKEN).await;
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut host = raw_host(&relay, &key, TOKEN).await;
        let HostEnvelope::Registered { host_id: id } = next_host_envelope(&mut host).await else {
            panic!("expected Registered");
        };
        assert_eq!(id, host_id(&key.verifying_key().to_bytes()));
        assert_eq!(relay.host_id(), Some(id.clone()));

        let mut client = TestClient::connect(&relay.url(), &id).await;
        let HostEnvelope::ChannelOpened { channel } = next_host_envelope(&mut host).await else {
            panic!("expected ChannelOpened");
        };
        assert_eq!(channel, ChannelId(0));

        client.send(Bytes::from_static(b"hi")).await;
        let HostEnvelope::ChannelData {
            channel: c,
            payload,
        } = next_host_envelope(&mut host).await
        else {
            panic!("expected ChannelData");
        };
        assert_eq!(c, channel);
        assert_eq!(payload.as_bytes(), b"hi");

        host.send(binary(envelope::encode_host(&HostEnvelope::ChannelData {
            channel,
            payload: Payload::from_bytes(Bytes::from_static(b"yo")),
        })))
        .await
        .unwrap();
        assert!(matches!(client.recv().await, ClientEvent::Data(b) if &b[..] == b"yo"));
        assert_eq!(relay.open_channels(), 1);
    }

    #[tokio::test]
    async fn refuses_a_wrong_token_and_a_bad_signature() {
        let relay = FakeRelay::start(TOKEN).await;
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut request =
            tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
                format!("{}/v0/host", relay.url()).as_str(),
            )
            .unwrap();
        request
            .headers_mut()
            .insert("Authorization", "Bearer nope".parse().unwrap());
        let error = tokio_tungstenite::connect_async(request)
            .await
            .err()
            .unwrap();
        assert!(matches!(
            error,
            tokio_tungstenite::tungstenite::Error::Http(r) if r.status() == 401
        ));

        // A signature made with a different key than the one announced.
        let mut ws = raw_host(&relay, &key, TOKEN).await;
        let _ = next_host_envelope(&mut ws).await; // registered with the good key
        let other = SigningKey::from_bytes(&[2u8; 32]);
        let mut request =
            tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
                format!("{}/v0/host", relay.url()).as_str(),
            )
            .unwrap();
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {TOKEN}").parse().unwrap());
        let (mut bad, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let Message::Binary(frame) = bad.next().await.unwrap().unwrap() else {
            panic!()
        };
        let HostEnvelope::Challenge { nonce } = envelope::decode_host(frame).unwrap() else {
            panic!()
        };
        let register = HostEnvelope::Register {
            routing_key: key.verifying_key().to_bytes(),
            signature: other.sign(&register_message(&nonce)).to_bytes(),
        };
        bad.send(binary(envelope::encode_host(&register)))
            .await
            .unwrap();
        match bad.next().await {
            Some(Ok(Message::Close(Some(frame)))) => assert_eq!(u16::from(frame.code), 4001),
            other => panic!("expected close 4001, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_host_gets_host_unavailable() {
        let relay = FakeRelay::start(TOKEN).await;
        let id = host_id(&[9u8; 32]);
        let mut client = TestClient::connect(&relay.url(), &id).await;
        assert!(matches!(client.recv().await, ClientEvent::HostUnavailable));
        assert!(matches!(
            client.recv().await,
            ClientEvent::WsClosed(Some(1000))
        ));
    }

    #[tokio::test]
    async fn a_second_registration_replaces_the_first() {
        let relay = FakeRelay::start(TOKEN).await;
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut first = raw_host(&relay, &key, TOKEN).await;
        let HostEnvelope::Registered { host_id: id } = next_host_envelope(&mut first).await else {
            panic!()
        };
        let mut client = TestClient::connect(&relay.url(), &id).await;
        let _ = next_host_envelope(&mut first).await; // channel-opened

        let mut second = raw_host(&relay, &key, TOKEN).await;
        let _ = next_host_envelope(&mut second).await; // registered

        assert!(matches!(
            client.recv().await,
            ClientEvent::Closed(CloseReason::HostReplaced)
        ));
        match first.next().await {
            Some(Ok(Message::Close(Some(frame)))) => assert_eq!(u16::from(frame.code), 4002),
            other => panic!("expected close 4002, got {other:?}"),
        }
        assert!(wait_until(|| relay.registrations() == 2 && relay.has_host()).await);
    }

    #[tokio::test]
    async fn data_for_a_never_opened_channel_drops_the_host_with_4004() {
        let relay = FakeRelay::start(TOKEN).await;
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut host = raw_host(&relay, &key, TOKEN).await;
        let _ = next_host_envelope(&mut host).await;
        host.send(binary(envelope::encode_host(&HostEnvelope::ChannelData {
            channel: ChannelId(5),
            payload: Payload::from_bytes(Bytes::from_static(b"x")),
        })))
        .await
        .unwrap();
        match tokio::time::timeout(Duration::from_secs(5), host.next())
            .await
            .unwrap()
        {
            Some(Ok(Message::Close(Some(frame)))) => assert_eq!(u16::from(frame.code), 4004),
            other => panic!("expected close 4004, got {other:?}"),
        }
        assert!(wait_until(|| relay.invalid_envelopes() == 1 && !relay.has_host()).await);
    }

    #[tokio::test]
    async fn a_dropped_host_tells_clients_host_gone() {
        let relay = FakeRelay::start(TOKEN).await;
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut host = raw_host(&relay, &key, TOKEN).await;
        let HostEnvelope::Registered { host_id: id } = next_host_envelope(&mut host).await else {
            panic!()
        };
        let mut client = TestClient::connect(&relay.url(), &id).await;
        let _ = next_host_envelope(&mut host).await;
        relay.drop_host();
        assert!(matches!(
            client.recv().await,
            ClientEvent::Closed(CloseReason::HostGone)
        ));
        assert!(wait_until(|| !relay.has_host()).await);
    }
}
