// SPDX-License-Identifier: GPL-3.0-or-later

//! Test-only support for the modules that talk to the relay: an in-memory
//! [`FakeRelay`] on loopback and a bare-bones [`TestClient`].

mod client;
mod fake_relay;
mod memory_vault;

use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::protocol::envelope::{self, ClientEnvelope, CloseReason, Payload};
use crate::protocol::host_id::HostId;
use crate::protocol::pairing_uri::client_channel_url;

pub(crate) use client::{PhoneClient, Seen};
pub(crate) use fake_relay::FakeRelay;
pub(crate) use memory_vault::{MemoryVault, TOKEN};

/// Polls `condition` every few milliseconds for up to five seconds.
pub(crate) async fn wait_until(condition: impl Fn() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    condition()
}

/// What a [`TestClient`] sees.
#[derive(Debug)]
pub(crate) enum ClientEvent {
    /// A `data` envelope: one record from the host.
    Data(Bytes),
    /// The relay has no host under that `host_id`.
    HostUnavailable,
    /// The relay ended the channel.
    Closed(CloseReason),
    /// The WebSocket closed (with this code, if the relay sent one).
    WsClosed(Option<u16>),
    /// A frame that is not a client envelope.
    Undecodable,
}

/// A client of the relay with no protocol on top: it sends and receives raw
/// records (`Bytes`). Stage 08 puts a `ClientSession` over it.
pub(crate) struct TestClient {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl TestClient {
    /// Connects to `/v0/client/<host_id>` of a relay at `relay_url`.
    pub(crate) async fn try_connect(relay_url: &str, host_id: &HostId) -> Result<Self, WsError> {
        let url = client_channel_url(relay_url, host_id);
        let (ws, _response) = connect_async(url.as_str()).await?;
        Ok(Self { ws })
    }

    /// [`try_connect`](Self::try_connect), panicking on failure.
    pub(crate) async fn connect(relay_url: &str, host_id: &HostId) -> Self {
        Self::try_connect(relay_url, host_id)
            .await
            .expect("client connection")
    }

    /// Sends one record as a `data` envelope.
    pub(crate) async fn send(&mut self, record: Bytes) {
        let frame = envelope::encode_client(&ClientEnvelope::Data {
            payload: Payload::from_bytes(record),
        });
        self.send_raw(frame).await;
    }

    /// Sends a binary frame as is (to test relay behaviour on bad envelopes).
    pub(crate) async fn send_raw(&mut self, frame: Vec<u8>) {
        let _ = self.ws.send(Message::Binary(Bytes::from(frame))).await;
    }

    /// The next thing that happens, or a panic after five seconds.
    pub(crate) async fn recv(&mut self) -> ClientEvent {
        let next = tokio::time::timeout(Duration::from_secs(5), self.recv_inner());
        next.await.expect("client: nothing arrived in time")
    }

    async fn recv_inner(&mut self) -> ClientEvent {
        loop {
            match self.ws.next().await {
                None | Some(Err(_)) => return ClientEvent::WsClosed(None),
                Some(Ok(Message::Close(frame))) => {
                    return ClientEvent::WsClosed(frame.map(|f| u16::from(f.code)));
                }
                Some(Ok(Message::Binary(frame))) => {
                    return match envelope::decode_client(frame) {
                        Ok(ClientEnvelope::Data { payload }) => {
                            ClientEvent::Data(payload.into_bytes())
                        }
                        Ok(ClientEnvelope::HostUnavailable) => ClientEvent::HostUnavailable,
                        Ok(ClientEnvelope::Closed { reason }) => ClientEvent::Closed(reason),
                        Err(_) => ClientEvent::Undecodable,
                    };
                }
                Some(Ok(_)) => continue,
            }
        }
    }

    /// Closes the WebSocket (the relay tells the host `channel-closed`).
    pub(crate) async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}
