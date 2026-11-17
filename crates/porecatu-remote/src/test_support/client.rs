// SPDX-License-Identifier: GPL-3.0-or-later

//! A phone for the tests: a [`ClientSession`] of the protocol crate over the
//! WebSocket of a [`TestClient`].

use std::time::Instant;

use bytes::Bytes;

use super::{ClientEvent, TestClient};
use crate::protocol::envelope::{self, ClientEnvelope, CloseReason, Payload};
use crate::protocol::host_id::HostId;
use crate::protocol::message::{Hello, Message, PairPlatform, Version};
use crate::protocol::pairing_uri::PairingUri;
use crate::protocol::session::{ClientError, ClientEvent as Session, ClientSession, StaticKeypair};

/// What a [`PhoneClient`] observes. The payloads are for the `Debug` output
/// of a failing test.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) enum Seen {
    /// The Noise handshake finished.
    Ready,
    /// An application message from the host.
    Message(Message),
    /// The relay has no host under that `host_id`.
    HostUnavailable,
    /// The relay ended the channel.
    Closed(CloseReason),
    /// The WebSocket closed.
    WsClosed,
    /// The session failed.
    Failed(ClientError),
}

/// A client with the protocol on top: pairs or connects, then sends and
/// receives messages.
pub(crate) struct PhoneClient {
    ws: TestClient,
    session: ClientSession,
    origin: Instant,
    /// Events produced but not handed out yet.
    queue: std::collections::VecDeque<Seen>,
}

impl PhoneClient {
    fn now(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Pairs with the host an invitation points at; the first record is sent.
    pub(crate) async fn pair(
        relay_url: &str,
        invite: &PairingUri,
        device: &StaticKeypair,
        name: &str,
    ) -> PhoneClient {
        let mut ws = TestClient::connect(relay_url, &invite.host_id).await;
        let (session, frames) =
            ClientSession::pair(invite, device, name, PairPlatform::Android, 0).expect("pair");
        for frame in frames {
            ws.send_raw(frame.to_vec()).await;
        }
        let mut phone = PhoneClient {
            ws,
            session,
            origin: Instant::now(),
            queue: Default::default(),
        };
        phone.until_ready().await;
        phone
    }

    /// Connects as an already paired device.
    pub(crate) async fn connect(
        relay_url: &str,
        host_id: &HostId,
        host_key: &[u8; 32],
        device: &StaticKeypair,
    ) -> PhoneClient {
        let mut ws = TestClient::connect(relay_url, host_id).await;
        let (session, frames) =
            ClientSession::connect(host_id, host_key, device, 0).expect("connect");
        for frame in frames {
            ws.send_raw(frame.to_vec()).await;
        }
        let mut phone = PhoneClient {
            ws,
            session,
            origin: Instant::now(),
            queue: Default::default(),
        };
        phone.until_ready().await;
        phone
    }

    /// Drives the handshake to its end. Pairing needs the phone to answer
    /// the host's `msg2`, so a test cannot just wait on the host side.
    async fn until_ready(&mut self) {
        match self.next().await {
            Seen::Ready => {}
            other => panic!("expected the handshake to finish, got {other:?}"),
        }
    }

    /// Sends a message (the handshake must be done).
    pub(crate) async fn send(&mut self, message: &Message) {
        let now = self.now();
        let frames = self.session.send(message, now).expect("send");
        for frame in frames {
            self.ws.send_raw(frame.to_vec()).await;
        }
    }

    /// Sends `hello` with the given version.
    pub(crate) async fn hello(&mut self, major: u64, minor: u64) {
        self.send(&Message::Hello(Hello {
            version: Version { major, minor },
            app: "porecatu-remote".to_owned(),
            app_version: "0.0.0".to_owned(),
        }))
        .await;
    }

    /// The next observation, or a panic after five seconds.
    pub(crate) async fn next(&mut self) -> Seen {
        loop {
            if let Some(seen) = self.queue.pop_front() {
                return seen;
            }
            let event = self.ws.recv().await;
            let now = self.now();
            match event {
                ClientEvent::Data(payload) => {
                    let frame = Bytes::from(envelope::encode_client(&ClientEnvelope::Data {
                        payload: Payload::from_bytes(payload),
                    }));
                    match self.session.receive(frame, now) {
                        Ok(events) => self.absorb(events).await,
                        Err(error) => self.queue.push_back(Seen::Failed(error)),
                    }
                }
                ClientEvent::HostUnavailable => self.queue.push_back(Seen::HostUnavailable),
                ClientEvent::Closed(reason) => self.queue.push_back(Seen::Closed(reason)),
                ClientEvent::WsClosed(_) => self.queue.push_back(Seen::WsClosed),
                ClientEvent::Undecodable => panic!("the relay sent an undecodable frame"),
            }
        }
    }

    async fn absorb(&mut self, events: Vec<Session>) {
        for event in events {
            match event {
                Session::Send(frame) => self.ws.send_raw(frame.to_vec()).await,
                Session::HandshakeComplete => self.queue.push_back(Seen::Ready),
                Session::Message(message) => self.queue.push_back(Seen::Message(message)),
                Session::HostUnavailable => self.queue.push_back(Seen::HostUnavailable),
                Session::Closed(reason) => self.queue.push_back(Seen::Closed(reason)),
                Session::Failed(error) => self.queue.push_back(Seen::Failed(error)),
            }
        }
    }

    /// The next message, skipping the end of the handshake; panics on
    /// anything else.
    pub(crate) async fn message(&mut self) -> Message {
        loop {
            match self.next().await {
                Seen::Ready => continue,
                Seen::Message(message) => return message,
                other => panic!("expected a message, got {other:?}"),
            }
        }
    }

    /// The next observation if one comes within `wait`.
    pub(crate) async fn try_next(&mut self, wait: std::time::Duration) -> Option<Seen> {
        tokio::time::timeout(wait, self.next()).await.ok()
    }

    /// Panics if anything arrives within `wait`.
    pub(crate) async fn quiet(&mut self, wait: std::time::Duration) {
        if let Some(seen) = self.try_next(wait).await {
            panic!("expected silence, got {seen:?}");
        }
    }

    /// Waits until the channel ends (relay `closed`, a failure or the
    /// WebSocket going away), skipping messages; returns what ended it.
    pub(crate) async fn until_closed(&mut self) -> Seen {
        loop {
            match self.next().await {
                Seen::Ready | Seen::Message(_) => continue,
                other => return other,
            }
        }
    }

    /// Closes the WebSocket.
    pub(crate) async fn close(self) {
        self.ws.close().await;
    }
}
