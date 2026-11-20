// SPDX-License-Identifier: GPL-3.0-or-later

//! The host against the fake relay and a phone made of `ClientSession`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::*;
use crate::api::{DeviceInfo, DevicePlatform};
use crate::protocol::fingerprint::fingerprint;
use crate::protocol::message::{
    Action, ActionBody, Confirm, ErrorCode, GoodbyeReason, Message, PairRejectReason, Profile,
    ProfileKind, Subscribe, ViewTab, Window,
};
use crate::test_support::{FakeRelay, MemoryVault, PhoneClient, Seen, wait_until};

/// A host under test with the relay it talks to.
struct Rig {
    relay: FakeRelay,
    host: RemoteHost,
    events: UnboundedReceiver<RemoteEvent>,
    dir: tempfile::TempDir,
}

fn params(relay: &FakeRelay, dir: &Path) -> RemoteParams {
    RemoteParams {
        relay_url: relay.url(),
        data_dir: dir.to_path_buf(),
        host_name: "Workstation".to_owned(),
        porecatu_version: "0.9.11".to_owned(),
    }
}

fn start_host(
    relay: &FakeRelay,
    dir: &Path,
    vault: MemoryVault,
    tuning: Tuning,
) -> (RemoteHost, UnboundedReceiver<RemoteEvent>) {
    let (tx, rx) = unbounded_channel();
    let host = RemoteHost::start_with_tuning(
        params(relay, dir),
        Arc::new(vault),
        move |event| {
            let _ = tx.send(event);
        },
        tuning,
    )
    .expect("host");
    (host, rx)
}

impl Rig {
    /// A host with a token in its vault, registered at the relay.
    async fn up() -> Rig {
        Self::up_with(test_tuning()).await
    }

    async fn up_with(tuning: Tuning) -> Rig {
        let relay = FakeRelay::start(crate::test_support::TOKEN).await;
        let dir = tempfile::tempdir().expect("temp dir");
        let (host, events) = start_host(&relay, dir.path(), MemoryVault::with_token(), tuning);
        let mut rig = Rig {
            relay,
            host,
            events,
            dir,
        };
        rig.wait(|e| matches!(e, RemoteEvent::Status(RelayStatus::Connected)).then_some(()))
            .await;
        rig
    }

    /// Discards events until `pick` takes one.
    async fn wait<T>(&mut self, pick: impl FnMut(RemoteEvent) -> Option<T>) -> T {
        wait_event(&mut self.events, pick).await
    }

    fn url(&self) -> String {
        self.relay.url()
    }

    /// The devices as the file has them.
    fn file_devices(&self) -> Vec<DeviceInfo> {
        DeviceStore::load(self.dir.path()).0.list()
    }

    /// `ShowPairing` and the invitation it yields.
    async fn invitation(&mut self) -> PairingUri {
        self.host.send(Command::ShowPairing);
        let uri = self
            .wait(|e| match e {
                RemoteEvent::PairingUri { uri, .. } => Some(uri),
                _ => None,
            })
            .await;
        PairingUri::parse(&uri).expect("the QR code URI parses")
    }

    /// Pairs `device`, accepting it; returns the phone with `pair-accepted`
    /// already read.
    async fn pair(&mut self, device: &StaticKeypair, name: &str) -> (PhoneClient, PairingUri) {
        let invite = self.invitation().await;
        let mut phone = PhoneClient::pair(&self.url(), &invite, device, name).await;
        let id = self.pairing_request().await;
        self.host.send(Command::AnswerPairing {
            id,
            answer: PairingAnswer::Accept,
        });
        assert!(matches!(phone.message().await, Message::PairAccepted(_)));
        (phone, invite)
    }

    async fn pairing_request(&mut self) -> PairingId {
        self.wait(|e| match e {
            RemoteEvent::PairingRequested { id, .. } => Some(id),
            _ => None,
        })
        .await
    }

    /// Pairs and runs the `hello`; returns the phone in session and its
    /// client id.
    async fn session(&mut self, device: &StaticKeypair) -> (PhoneClient, PairingUri, ClientId) {
        let (mut phone, invite) = self.pair(device, "Pixel 9").await;
        phone.hello(0, 1).await;
        let client = self.client_connected().await;
        assert!(matches!(phone.message().await, Message::Welcome(_)));
        (phone, invite, client)
    }

    async fn client_connected(&mut self) -> ClientId {
        self.wait(|e| match e {
            RemoteEvent::ClientConnected { client, .. } => Some(client),
            _ => None,
        })
        .await
    }

    async fn status_connected_again(&mut self) {
        self.wait(|e| matches!(e, RemoteEvent::Status(RelayStatus::Connected)).then_some(()))
            .await;
    }
}

async fn wait_event<T>(
    rx: &mut UnboundedReceiver<RemoteEvent>,
    mut pick: impl FnMut(RemoteEvent) -> Option<T>,
) -> T {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("no matching event in time")
            .expect("event channel closed");
        if let Some(found) = pick(event) {
            return found;
        }
    }
}

fn theme() -> Theme {
    Theme {
        fg: 0xcccccc,
        bg: 0x111111,
        cursor: 0xffffff,
        selection_bg: 0x333333,
        palette: [0; 16],
        group_colors: vec![0xff0000],
    }
}

fn profiles() -> Profiles {
    Profiles {
        list: vec![Profile {
            id: "default".to_owned(),
            name: "Shell".to_owned(),
            kind: ProfileKind::Local,
        }],
        default: "default".to_owned(),
    }
}

fn workspace() -> Workspace {
    Workspace {
        rev: 0,
        windows: vec![Window {
            id: 1,
            title: "Porecatu".to_owned(),
            pinned: vec![],
            items: vec![],
            active: None,
        }],
    }
}

fn device() -> StaticKeypair {
    StaticKeypair::generate()
}

// ---------------------------------------------------------------------------
// Start-up
// ---------------------------------------------------------------------------

#[tokio::test]
async fn start_reports_devices_connecting_and_connected() {
    let relay = FakeRelay::start(crate::test_support::TOKEN).await;
    let dir = tempfile::tempdir().unwrap();
    let (_host, mut rx) = start_host(&relay, dir.path(), MemoryVault::with_token(), test_tuning());

    let mut seen = Vec::new();
    while !matches!(
        seen.last(),
        Some(RemoteEvent::Status(RelayStatus::Connected))
    ) {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("no event in time")
            .expect("closed");
        seen.push(event);
    }
    assert!(matches!(&seen[0], RemoteEvent::Devices(list) if list.is_empty()));
    assert!(matches!(
        &seen[1],
        RemoteEvent::Status(RelayStatus::Connecting)
    ));
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert!(relay.has_host());
}

#[tokio::test]
async fn a_missing_token_reports_it_and_never_connects() {
    let relay = FakeRelay::start(crate::test_support::TOKEN).await;
    let dir = tempfile::tempdir().unwrap();
    let (host, mut rx) = start_host(&relay, dir.path(), MemoryVault::default(), test_tuning());

    wait_event(&mut rx, |e| {
        matches!(
            e,
            RemoteEvent::Status(RelayStatus::Failed(RelayFailure::TokenMissing))
        )
        .then_some(())
    })
    .await;
    // `ShowPairing` is ignored without a registration.
    host.send(Command::ShowPairing);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(relay.connections(), 0);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn a_failing_vault_reports_it_and_does_nothing_else() {
    let relay = FakeRelay::start(crate::test_support::TOKEN).await;
    let dir = tempfile::tempdir().unwrap();
    let (_host, mut rx) = start_host(&relay, dir.path(), MemoryVault::failing(), test_tuning());

    let first = wait_event(&mut rx, Some).await;
    assert!(
        matches!(
            first,
            RemoteEvent::Status(RelayStatus::Failed(RelayFailure::VaultUnavailable))
        ),
        "{first:?}"
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(relay.connections(), 0);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn an_unreadable_list_is_reported_as_quarantined() {
    let relay = FakeRelay::start(crate::test_support::TOKEN).await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("remote_devices.json"), b"not json").unwrap();
    let (_host, mut rx) = start_host(&relay, dir.path(), MemoryVault::with_token(), test_tuning());

    let path = wait_event(&mut rx, |e| match e {
        RemoteEvent::DevicesQuarantined { path } => Some(path),
        _ => None,
    })
    .await;
    assert!(path.exists());
    assert!(!dir.path().join("remote_devices.json").exists());
}

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn show_pairing_yields_an_invitation_that_points_at_this_host() {
    let mut rig = Rig::up().await;
    let invite = rig.invitation().await;
    assert_eq!(Some(invite.host_id.clone()), rig.relay.host_id());
    assert_eq!(invite.name, "Workstation");
    assert_eq!(invite.relay, rig.url());
}

#[tokio::test]
async fn a_long_host_name_is_cut_to_32_scalar_values() {
    assert_eq!(display_name(&"é".repeat(40)).chars().count(), 32);
    assert!(!display_name("").is_empty());
    assert!(!display_name("   ").trim().is_empty());
}

#[tokio::test]
async fn an_accepted_pairing_stores_the_device_and_opens_the_session() {
    let mut rig = Rig::up().await;
    rig.host.send(Command::SetTheme(theme()));
    rig.host.send(Command::SetProfiles(profiles()));
    rig.host.send(Command::SetWorkspace(workspace()));

    let key = device();
    let invite = rig.invitation().await;
    let mut phone = PhoneClient::pair(&rig.url(), &invite, &key, "Pixel 9").await;
    let (name, fp) = rig
        .wait(|e| match e {
            RemoteEvent::PairingRequested {
                device_name,
                platform,
                fingerprint,
                ..
            } => {
                assert_eq!(platform, DevicePlatform::Android);
                Some((device_name, fingerprint))
            }
            _ => None,
        })
        .await;
    assert_eq!(name, "Pixel 9");
    assert_eq!(fp, fingerprint(&key.public()).to_string());
    let id = PairingId(1);
    rig.host.send(Command::AnswerPairing {
        id,
        answer: PairingAnswer::Accept,
    });

    // The list, in memory and on disk, and `pair-accepted`.
    let list = rig
        .wait(|e| match e {
            RemoteEvent::Devices(list) => Some(list),
            _ => None,
        })
        .await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].key, key.public());
    assert_eq!(list[0].name, "Pixel 9");
    assert_eq!(rig.file_devices(), list);
    let Message::PairAccepted(accepted) = phone.message().await else {
        panic!("expected pair-accepted");
    };
    assert_eq!(accepted.host_name, "Workstation");

    // `hello`: welcome, then theme, profiles and workspace, in that order.
    phone.hello(0, 1).await;
    let Message::Welcome(welcome) = phone.message().await else {
        panic!("expected welcome");
    };
    assert_eq!(welcome.version.major, 0);
    assert_eq!(welcome.version.minor, 1);
    assert_eq!(welcome.host_name, "Workstation");
    assert_eq!(welcome.porecatu_version, "0.9.11");
    assert_eq!(welcome.max_subscriptions, 4);
    assert_eq!(welcome.host_instance, host_instance());
    assert!(matches!(phone.message().await, Message::Theme(_)));
    assert!(matches!(phone.message().await, Message::Profiles(_)));
    let Message::Workspace(ws) = phone.message().await else {
        panic!("expected workspace");
    };
    assert_eq!(ws.rev, 1);
    assert_eq!(ws.windows.len(), 1);

    let (client, device_key, name, platform) = rig
        .wait(|e| match e {
            RemoteEvent::ClientConnected {
                client,
                device_key,
                name,
                platform,
            } => Some((client, device_key, name, platform)),
            _ => None,
        })
        .await;
    assert_eq!(client, ClientId(1));
    assert_eq!(device_key, key.public());
    assert_eq!(name, "Pixel 9");
    assert_eq!(platform, DevicePlatform::Android);
}

#[tokio::test]
async fn what_the_ui_has_not_sent_yet_is_not_invented() {
    let mut rig = Rig::up().await;
    let (mut phone, _invite, _client) = rig.session(&device()).await;
    // Nothing was set: the next thing is only what arrives later.
    rig.host.send(Command::SetTheme(theme()));
    assert!(matches!(phone.message().await, Message::Theme(_)));
}

#[tokio::test]
async fn a_declined_or_timed_out_pairing_is_rejected_with_its_reason() {
    for (answer, expected) in [
        (PairingAnswer::Decline, PairRejectReason::UserDeclined),
        (PairingAnswer::Timeout, PairRejectReason::Timeout),
    ] {
        let mut rig = Rig::up().await;
        let invite = rig.invitation().await;
        let mut phone = PhoneClient::pair(&rig.url(), &invite, &device(), "Pixel 9").await;
        let id = rig.pairing_request().await;
        rig.host.send(Command::AnswerPairing { id, answer });

        let Message::PairRejected(rejected) = phone.message().await else {
            panic!("expected pair-rejected");
        };
        assert_eq!(rejected.reason, expected);
        assert!(matches!(phone.until_closed().await, Seen::Closed(_)));
        assert!(rig.file_devices().is_empty());
    }
}

#[tokio::test]
async fn an_unanswered_pairing_times_out_on_the_host_side() {
    let tuning = Tuning {
        pairing_backstop_ms: 150,
        ..test_tuning()
    };
    let mut rig = Rig::up_with(tuning).await;
    let invite = rig.invitation().await;
    let mut phone = PhoneClient::pair(&rig.url(), &invite, &device(), "Pixel 9").await;
    let id = rig.pairing_request().await;

    let Message::PairRejected(rejected) = phone.message().await else {
        panic!("expected pair-rejected");
    };
    assert_eq!(rejected.reason, PairRejectReason::Timeout);
    let closed = rig
        .wait(|e| match e {
            RemoteEvent::PairingClosed { id } => Some(id),
            _ => None,
        })
        .await;
    assert_eq!(closed, id);
}

#[tokio::test]
async fn the_pairing_dialog_is_closed_when_the_phone_leaves() {
    let mut rig = Rig::up().await;
    let invite = rig.invitation().await;
    let phone = PhoneClient::pair(&rig.url(), &invite, &device(), "Pixel 9").await;
    let id = rig.pairing_request().await;
    phone.close().await;

    let closed = rig
        .wait(|e| match e {
            RemoteEvent::PairingClosed { id } => Some(id),
            _ => None,
        })
        .await;
    assert_eq!(closed, id);
    assert!(rig.file_devices().is_empty());
}

#[tokio::test]
async fn a_token_works_once() {
    let mut rig = Rig::up().await;
    let (_phone, invite) = rig.pair(&device(), "Pixel 9").await;

    let mut second = PhoneClient::pair(&rig.url(), &invite, &device(), "Another").await;
    let Message::PairRejected(rejected) = second.message().await else {
        panic!("expected pair-rejected");
    };
    assert_eq!(rejected.reason, PairRejectReason::TokenInvalid);
    assert!(matches!(second.until_closed().await, Seen::Closed(_)));
    assert_eq!(rig.file_devices().len(), 1);
}

#[tokio::test]
async fn a_cancelled_pairing_invalidates_the_token() {
    let mut rig = Rig::up().await;
    let invite = rig.invitation().await;
    rig.host.send(Command::CancelPairing);
    // The queue is ordered: by the time the pairing arrives, it is cancelled.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut phone = PhoneClient::pair(&rig.url(), &invite, &device(), "Pixel 9").await;
    let Message::PairRejected(rejected) = phone.message().await else {
        panic!("expected pair-rejected");
    };
    assert_eq!(rejected.reason, PairRejectReason::TokenInvalid);
}

#[tokio::test]
async fn a_new_token_replaces_the_previous_one() {
    let mut rig = Rig::up().await;
    let first = rig.invitation().await;
    let _second = rig.invitation().await;
    let mut phone = PhoneClient::pair(&rig.url(), &first, &device(), "Pixel 9").await;
    let Message::PairRejected(rejected) = phone.message().await else {
        panic!("expected pair-rejected");
    };
    assert_eq!(rejected.reason, PairRejectReason::TokenInvalid);
}

// ---------------------------------------------------------------------------
// Connection of a paired device
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_paired_device_connects_again_without_a_token() {
    let mut rig = Rig::up().await;
    let key = device();
    let (phone, invite, _client) = rig.session(&key).await;
    phone.close().await;
    rig.wait(|e| matches!(e, RemoteEvent::ClientDisconnected { .. }).then_some(()))
        .await;

    let host_id = rig.relay.host_id().unwrap();
    let mut again = PhoneClient::connect(&rig.url(), &host_id, &invite.host_key, &key).await;
    again.hello(0, 1).await;
    let client = rig.client_connected().await;
    assert_eq!(client, ClientId(2));
    assert!(matches!(again.message().await, Message::Welcome(_)));
}

#[tokio::test]
async fn an_unknown_device_is_told_it_is_not_paired() {
    let mut rig = Rig::up().await;
    let (_phone, invite, _client) = rig.session(&device()).await;

    let host_id = rig.relay.host_id().unwrap();
    let mut stranger =
        PhoneClient::connect(&rig.url(), &host_id, &invite.host_key, &device()).await;
    let Message::Error(error) = stranger.message().await else {
        panic!("expected error");
    };
    assert_eq!(error.code, ErrorCode::NotPaired);
    assert!(matches!(stranger.until_closed().await, Seen::Closed(_)));
}

#[tokio::test]
async fn a_known_device_that_never_says_hello_is_dropped() {
    let tuning = Tuning {
        hello_ms: 150,
        ..test_tuning()
    };
    let mut rig = Rig::up_with(tuning).await;
    let key = device();
    let (phone, invite, _client) = rig.session(&key).await;
    phone.close().await;
    let host_id = rig.relay.host_id().unwrap();
    let mut silent = PhoneClient::connect(&rig.url(), &host_id, &invite.host_key, &key).await;
    assert!(matches!(silent.until_closed().await, Seen::Closed(_)));
}

#[tokio::test]
async fn an_incompatible_version_is_answered_and_closed() {
    for (major, minor) in [(1, 0), (0, 2), (0, 0)] {
        let mut rig = Rig::up().await;
        let (mut phone, _invite) = rig.pair(&device(), "Pixel 9").await;
        phone.hello(major, minor).await;
        let Message::Incompatible(incompatible) = phone.message().await else {
            panic!("expected incompatible for {major}.{minor}");
        };
        assert_eq!(incompatible.version.major, 0);
        assert_eq!(incompatible.version.minor, 1);
        assert!(matches!(phone.until_closed().await, Seen::Closed(_)));
    }
}

// ---------------------------------------------------------------------------
// In session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_workspace_reaches_an_open_session_with_a_growing_rev() {
    let mut rig = Rig::up().await;
    let (mut phone, _invite, _client) = rig.session(&device()).await;

    rig.host.send(Command::SetWorkspace(workspace()));
    rig.host.send(Command::SetWorkspace(workspace()));
    let mut revs = Vec::new();
    for _ in 0..2 {
        let Message::Workspace(ws) = phone.message().await else {
            panic!("expected workspace");
        };
        revs.push(ws.rev);
    }
    assert_eq!(revs, [1, 2]);

    // Another client counts on its own.
    let (mut other, _invite, _client) = rig.session(&device()).await;
    let Message::Workspace(ws) = other.message().await else {
        panic!("expected workspace");
    };
    assert_eq!(ws.rev, 1);
}

#[tokio::test]
async fn send_and_broadcast_reach_sessions_in_course() {
    let mut rig = Rig::up().await;
    let (mut a, _ia, client_a) = rig.session(&device()).await;
    let (mut b, _ib, _client_b) = rig.session(&device()).await;

    let bell = Message::Bell(crate::protocol::message::Bell { pane: 3 });
    rig.host.send(Command::Send {
        client: client_a,
        message: bell.clone(),
    });
    assert_eq!(a.message().await, bell);
    rig.host.send(Command::Broadcast(bell.clone()));
    assert_eq!(a.message().await, bell);
    assert_eq!(b.message().await, bell);
}

#[tokio::test]
async fn actions_and_confirmations_become_events_and_an_error_keeps_the_session() {
    let mut rig = Rig::up().await;
    let (mut phone, _invite, client) = rig.session(&device()).await;

    let action = Action {
        id: 9,
        action: ActionBody::CloseTab { tab: 4 },
    };
    phone.send(&Message::Action(action.clone())).await;
    let got = rig
        .wait(|e| match e {
            RemoteEvent::Action { client, action } => Some((client, action)),
            _ => None,
        })
        .await;
    assert_eq!(got, (client, action));

    let confirm = Confirm {
        id: 9,
        accept: true,
    };
    phone.send(&Message::Confirm(confirm)).await;
    let got = rig
        .wait(|e| match e {
            RemoteEvent::Confirm { client, confirm } => Some((client, confirm)),
            _ => None,
        })
        .await;
    assert_eq!(got, (client, confirm));

    // An error does not end the session.
    phone.send(&Message::Subscribe(Subscribe { pane: 5 })).await;
    let Message::Error(error) = phone.message().await else {
        panic!("expected error");
    };
    assert_eq!(error.code, ErrorCode::UnknownPane);
    assert_eq!(error.pane, Some(5));
    phone
        .send(&Message::Action(Action {
            id: 10,
            action: ActionBody::CloseTab { tab: 4 },
        }))
        .await;
    rig.wait(|e| matches!(e, RemoteEvent::Action { .. }).then_some(()))
        .await;
}

#[tokio::test]
async fn the_phone_saying_goodbye_ends_the_session() {
    let mut rig = Rig::up().await;
    let (mut phone, _invite, client) = rig.session(&device()).await;
    phone
        .send(&Message::Goodbye(crate::protocol::message::Goodbye {
            reason: GoodbyeReason::Normal,
        }))
        .await;
    let gone = rig
        .wait(|e| match e {
            RemoteEvent::ClientDisconnected { client } => Some(client),
            _ => None,
        })
        .await;
    assert_eq!(gone, client);
    assert!(matches!(phone.until_closed().await, Seen::Closed(_)));
}

#[tokio::test]
async fn disconnect_says_goodbye_and_closes() {
    let mut rig = Rig::up().await;
    let (mut a, _ia, client_a) = rig.session(&device()).await;
    let (mut b, _ib, _client_b) = rig.session(&device()).await;

    rig.host.send(Command::Disconnect(client_a));
    let Message::Goodbye(goodbye) = a.message().await else {
        panic!("expected goodbye");
    };
    assert_eq!(goodbye.reason, GoodbyeReason::Normal);
    assert!(matches!(a.until_closed().await, Seen::Closed(_)));
    let gone = rig
        .wait(|e| match e {
            RemoteEvent::ClientDisconnected { client } => Some(client),
            _ => None,
        })
        .await;
    assert_eq!(gone, client_a);

    rig.host.send(Command::DisconnectAll);
    let Message::Goodbye(goodbye) = b.message().await else {
        panic!("expected goodbye");
    };
    assert_eq!(goodbye.reason, GoodbyeReason::Normal);
    assert!(matches!(b.until_closed().await, Seen::Closed(_)));
}

#[tokio::test]
async fn last_seen_is_written_when_the_session_ends() {
    let mut rig = Rig::up().await;
    let key = device();
    let (phone, _invite, _client) = rig.session(&key).await;
    assert!(rig.file_devices()[0].last_seen.is_none());
    phone.close().await;

    let list = rig
        .wait(|e| match e {
            RemoteEvent::Devices(list) if list.iter().any(|d| d.last_seen.is_some()) => Some(list),
            _ => None,
        })
        .await;
    assert_eq!(list.len(), 1);
    assert_eq!(rig.file_devices(), list);
}

#[tokio::test]
async fn revoking_says_goodbye_and_forgets_the_device() {
    let mut rig = Rig::up().await;
    let key = device();
    let (mut phone, invite, _client) = rig.session(&key).await;

    rig.host.send(Command::Revoke { key: key.public() });
    let Message::Goodbye(goodbye) = phone.message().await else {
        panic!("expected goodbye");
    };
    assert_eq!(goodbye.reason, GoodbyeReason::Revoked);
    assert!(matches!(phone.until_closed().await, Seen::Closed(_)));
    let list = rig
        .wait(|e| match e {
            RemoteEvent::Devices(list) => Some(list),
            _ => None,
        })
        .await;
    assert!(list.is_empty());
    assert!(rig.file_devices().is_empty());

    // It cannot come back.
    let host_id = rig.relay.host_id().unwrap();
    let mut again = PhoneClient::connect(&rig.url(), &host_id, &invite.host_key, &key).await;
    let Message::Error(error) = again.message().await else {
        panic!("expected error");
    };
    assert_eq!(error.code, ErrorCode::NotPaired);
}

#[tokio::test]
async fn recreating_the_identity_drops_everyone_and_registers_anew() {
    let mut rig = Rig::up().await;
    let (mut phone, _invite, _client) = rig.session(&device()).await;
    let before = rig.relay.host_id().unwrap();
    // A live token must not survive.
    let stale = rig.invitation().await;

    rig.host.send(Command::RecreateIdentity);
    let Message::Goodbye(goodbye) = phone.message().await else {
        panic!("expected goodbye");
    };
    assert_eq!(goodbye.reason, GoodbyeReason::Revoked);
    assert!(matches!(phone.until_closed().await, Seen::Closed(_)));

    // (Ending the session first records its `last_seen`, then the list is
    // cleared.)
    rig.wait(|e| match e {
        RemoteEvent::Devices(list) if list.is_empty() => Some(()),
        _ => None,
    })
    .await;
    assert!(rig.file_devices().is_empty());
    rig.status_connected_again().await;
    let after = rig.relay.host_id().unwrap();
    assert_ne!(before, after);

    // The old invitation points at the old identity: nobody is there.
    let mut old = crate::test_support::TestClient::connect(&rig.url(), &stale.host_id).await;
    assert!(matches!(
        old.recv().await,
        crate::test_support::ClientEvent::HostUnavailable
    ));
    // A new one works.
    let (_phone, invite) = rig.pair(&device(), "New").await;
    assert_eq!(invite.host_id, after);
}

#[tokio::test]
async fn stopping_says_goodbye_with_the_reason_and_the_main_thread_does_not_wait() {
    for (reason, expected) in [
        (StopReason::Disabled, GoodbyeReason::RemoteDisabled),
        (StopReason::Quitting, GoodbyeReason::HostQuitting),
    ] {
        let mut rig = Rig::up().await;
        let (mut phone, _invite, _client) = rig.session(&device()).await;
        let started = Instant::now();
        rig.host.stop(reason);
        assert!(started.elapsed() < Duration::from_millis(100));

        let Message::Goodbye(goodbye) = phone.message().await else {
            panic!("expected goodbye");
        };
        assert_eq!(goodbye.reason, expected);
        assert!(matches!(phone.until_closed().await, Seen::Closed(_)));
        assert!(wait_until(|| !rig.relay.has_host()).await);
    }
}

#[tokio::test]
async fn a_dropped_relay_connection_ends_the_sessions_and_the_host_comes_back() {
    let mut rig = Rig::up().await;
    let (_phone, _invite, client) = rig.session(&device()).await;

    rig.relay.drop_host();
    let gone = rig
        .wait(|e| match e {
            RemoteEvent::ClientDisconnected { client } => Some(client),
            _ => None,
        })
        .await;
    assert_eq!(gone, client);
    rig.status_connected_again().await;
    assert!(wait_until(|| rig.relay.has_host()).await);
}

#[tokio::test]
async fn frames_never_pile_up_and_never_block() {
    let rig = Rig::up().await;
    for seq in 0..1000 {
        rig.host.send(Command::Frame {
            pane: seq % 3,
            frame: Box::new(frame(seq)),
        });
    }
    assert_eq!(rig.host.dropped_commands(), 0);
}

fn frame(seq: u64) -> crate::protocol::message::GridSnapshot {
    use crate::protocol::message::{Cursor, CursorShape, GridSnapshot};
    GridSnapshot {
        pane: 0,
        seq,
        cols: 1,
        rows: 0,
        history: 0,
        dropped: 0,
        styles: vec![],
        lines: vec![],
        cursor: Cursor {
            row: 0,
            col: 0,
            shape: CursorShape::Block,
            visible: false,
        },
        alt: false,
    }
}

// ---------------------------------------------------------------------------
// Subscriptions, grid flow, scrollback and input (stage 11)
// ---------------------------------------------------------------------------

use crate::api::RemoteInput;
use crate::protocol::message::{
    Color, Cursor, CursorShape, GridAck, GridSnapshot, Item, Key, KeyCode, Line, NamedKey, Pane,
    PaneNode, PaneState, Paste, ScrollbackPage, ScrollbackRequest, Span, SpanText, Style,
    SubscriptionEndReason, Tab, TextInput, Unsubscribe,
};

const QUIET: Duration = Duration::from_millis(200);

/// A tree with one tab of one running pane for each of `panes`.
fn workspace_with(panes: &[u64]) -> Workspace {
    Workspace {
        rev: 0,
        windows: vec![Window {
            id: 1,
            title: "Porecatu".to_owned(),
            pinned: vec![],
            items: panes
                .iter()
                .map(|&pane| {
                    Item::Tab(Tab {
                        id: pane + 100,
                        title: "tab".to_owned(),
                        custom: false,
                        profile: None,
                        remote: None,
                        activity: false,
                        bell: false,
                        layout: PaneNode::Pane(Pane {
                            id: pane,
                            title: "shell".to_owned(),
                            cwd: None,
                            state: PaneState::Running,
                            exit: None,
                            cols: 10,
                            rows: 1,
                        }),
                        focused: pane,
                    })
                })
                .collect(),
            active: None,
        }],
    }
}

/// A one-line screen showing `text`.
fn screen(text: &str) -> Box<GridSnapshot> {
    Box::new(GridSnapshot {
        pane: 0,
        seq: 0,
        cols: 10,
        rows: 1,
        history: 0,
        dropped: 0,
        styles: vec![Style {
            fg: Color::Default,
            bg: Color::Default,
            flags: 0,
        }],
        lines: vec![Line {
            wrapped: false,
            spans: vec![Span {
                col: 0,
                width: 1,
                style: 0,
                text: SpanText::Plain(text.to_owned()),
            }],
        }],
        cursor: Cursor {
            row: 0,
            col: 0,
            shape: CursorShape::Block,
            visible: true,
        },
        alt: false,
    })
}

fn text_of(line: &Line) -> String {
    line.spans
        .iter()
        .map(|span| match &span.text {
            SpanText::Plain(text) | SpanText::Grapheme(text) => text.clone(),
        })
        .collect()
}

impl Rig {
    /// A phone in session on a host whose tree has `panes`; the `workspace`
    /// message already read.
    async fn session_on(&mut self, panes: &[u64]) -> (PhoneClient, ClientId) {
        self.host.send(Command::SetWorkspace(workspace_with(panes)));
        let (mut phone, _invite, client) = self.session(&device()).await;
        assert!(matches!(phone.message().await, Message::Workspace(_)));
        (phone, client)
    }

    async fn subscriptions(&mut self) -> Vec<u64> {
        self.wait(|e| match e {
            RemoteEvent::Subscriptions(panes) => Some(panes),
            _ => None,
        })
        .await
    }

    fn frame(&self, pane: u64, text: &str) {
        self.host.send(Command::Frame {
            pane,
            frame: screen(text),
        });
    }
}

async fn subscribe(phone: &mut PhoneClient, pane: u64) {
    phone.send(&Message::Subscribe(Subscribe { pane })).await;
}

async fn unsubscribe(phone: &mut PhoneClient, pane: u64) {
    phone
        .send(&Message::Unsubscribe(Unsubscribe { pane }))
        .await;
}

async fn ack(phone: &mut PhoneClient, pane: u64, seq: u64) {
    phone.send(&Message::GridAck(GridAck { pane, seq })).await;
}

async fn expect_error(phone: &mut PhoneClient, code: ErrorCode, pane: Option<u64>) {
    let Message::Error(error) = phone.message().await else {
        panic!("expected error {code:?}");
    };
    assert_eq!(error.code, code);
    assert_eq!(error.pane, pane);
}

#[tokio::test]
async fn subscribing_tells_the_ui_and_the_first_frame_is_a_snapshot() {
    let mut rig = Rig::up().await;
    let (mut phone, _client) = rig.session_on(&[1, 2]).await;

    subscribe(&mut phone, 1).await;
    assert_eq!(rig.subscriptions().await, [1]);
    // No frame yet: nothing to send.
    phone.quiet(QUIET).await;

    rig.frame(1, "a");
    let Message::GridSnapshot(snapshot) = phone.message().await else {
        panic!("expected grid-snapshot");
    };
    assert_eq!((snapshot.pane, snapshot.seq), (1, 0));
    assert_eq!(text_of(&snapshot.lines[0]), "a");

    // A frame of a pane nobody subscribes to goes nowhere.
    rig.frame(2, "x");
    phone.quiet(QUIET).await;
}

#[tokio::test]
async fn a_frame_already_held_is_sent_on_subscribing_and_again_on_resubscribing() {
    let mut rig = Rig::up().await;
    let (mut phone, _client) = rig.session_on(&[1]).await;
    rig.frame(1, "held");
    // The frame lands in the slot; let the actor take it.
    tokio::time::sleep(Duration::from_millis(100)).await;

    subscribe(&mut phone, 1).await;
    let Message::GridSnapshot(first) = phone.message().await else {
        panic!("expected grid-snapshot");
    };
    assert_eq!((first.pane, first.seq), (1, 0));
    assert_eq!(text_of(&first.lines[0]), "held");

    // Again: a full snapshot with the next seq of the same subscription, and
    // no second entry in the union.
    subscribe(&mut phone, 1).await;
    let Message::GridSnapshot(again) = phone.message().await else {
        panic!("expected grid-snapshot");
    };
    assert_eq!(again.seq, 1);
    assert_eq!(text_of(&again.lines[0]), "held");
    assert_eq!(rig.subscriptions().await, [1]);
}

#[tokio::test]
async fn the_window_holds_two_and_an_ack_releases_the_newest_state() {
    let mut rig = Rig::up().await;
    let (mut phone, _client) = rig.session_on(&[1]).await;
    subscribe(&mut phone, 1).await;
    rig.subscriptions().await;

    rig.frame(1, "a");
    let Message::GridSnapshot(snapshot) = phone.message().await else {
        panic!("expected grid-snapshot");
    };
    assert_eq!(snapshot.seq, 0);
    rig.frame(1, "b");
    let Message::GridDiff(diff) = phone.message().await else {
        panic!("expected grid-diff");
    };
    assert_eq!(diff.seq, 1);
    assert_eq!(text_of(&diff.lines[0].1), "b");

    // Two in flight: the rest only replaces the held frame.
    rig.frame(1, "c");
    rig.frame(1, "d");
    phone.quiet(QUIET).await;

    // One ack opens one place: a diff with the newest state, not with "c".
    ack(&mut phone, 1, 0).await;
    let Message::GridDiff(diff) = phone.message().await else {
        panic!("expected grid-diff");
    };
    assert_eq!(diff.seq, 2);
    assert_eq!(diff.lines.len(), 1);
    assert_eq!(text_of(&diff.lines[0].1), "d");
    phone.quiet(QUIET).await;

    // Nothing changed: an equal frame sends nothing, even with room.
    ack(&mut phone, 1, 1).await;
    rig.frame(1, "d");
    phone.quiet(QUIET).await;
}

#[tokio::test]
async fn an_unknown_pane_and_a_fifth_subscription_are_refused() {
    let mut rig = Rig::up().await;
    let (mut phone, _client) = rig.session_on(&[1, 2, 3, 4, 5]).await;

    subscribe(&mut phone, 99).await;
    expect_error(&mut phone, ErrorCode::UnknownPane, Some(99)).await;

    for pane in 1..=4 {
        subscribe(&mut phone, pane).await;
    }
    subscribe(&mut phone, 5).await;
    expect_error(&mut phone, ErrorCode::TooManySubscriptions, Some(5)).await;
    // The last union the UI hears has the four.
    let mut last = Vec::new();
    while last != [1, 2, 3, 4] {
        last = rig.subscriptions().await;
    }

    // Subscribing again to one of the four is not a fifth.
    subscribe(&mut phone, 4).await;
    phone.quiet(QUIET).await;
    // Letting one go makes room.
    unsubscribe(&mut phone, 1).await;
    assert_eq!(rig.subscriptions().await, [2, 3, 4]);
    subscribe(&mut phone, 5).await;
    assert_eq!(rig.subscriptions().await, [2, 3, 4, 5]);
}

#[tokio::test]
async fn unsubscribing_and_acknowledging_need_a_subscription() {
    let mut rig = Rig::up().await;
    let (mut phone, _client) = rig.session_on(&[1]).await;

    unsubscribe(&mut phone, 1).await;
    expect_error(&mut phone, ErrorCode::NotSubscribed, Some(1)).await;
    ack(&mut phone, 1, 0).await;
    expect_error(&mut phone, ErrorCode::NotSubscribed, Some(1)).await;

    subscribe(&mut phone, 1).await;
    assert_eq!(rig.subscriptions().await, [1]);
    unsubscribe(&mut phone, 1).await;
    assert!(rig.subscriptions().await.is_empty());
    // Frames go nowhere now.
    rig.frame(1, "a");
    phone.quiet(QUIET).await;
}

#[tokio::test]
async fn an_ack_of_nothing_sent_is_a_protocol_error_that_ends_the_session() {
    let mut rig = Rig::up().await;
    let (mut phone, client) = rig.session_on(&[1]).await;
    subscribe(&mut phone, 1).await;
    rig.subscriptions().await;
    rig.frame(1, "a");
    assert!(matches!(phone.message().await, Message::GridSnapshot(_)));

    ack(&mut phone, 1, 7).await;
    expect_error(&mut phone, ErrorCode::Protocol, None).await;
    assert!(matches!(phone.until_closed().await, Seen::Closed(_)));
    let gone = rig
        .wait(|e| match e {
            RemoteEvent::ClientDisconnected { client } => Some(client),
            _ => None,
        })
        .await;
    assert_eq!(gone, client);
}

#[tokio::test]
async fn input_reaches_the_ui_only_for_a_subscribed_pane() {
    let mut rig = Rig::up().await;
    let (mut phone, client) = rig.session_on(&[1, 2]).await;
    subscribe(&mut phone, 1).await;
    rig.subscriptions().await;

    let key = Key {
        pane: 1,
        key: KeyCode::Named(NamedKey::Enter),
        mods: 0,
    };
    phone.send(&Message::Key(key)).await;
    let (who, input) = rig
        .wait(|e| match e {
            RemoteEvent::Input { client, input } => Some((client, input)),
            _ => None,
        })
        .await;
    assert_eq!(who, client);
    assert!(matches!(input, RemoteInput::Key(k) if k == key));

    phone
        .send(&Message::TextInput(TextInput {
            pane: 1,
            text: "ls".to_owned(),
        }))
        .await;
    let (_, input) = rig
        .wait(|e| match e {
            RemoteEvent::Input { client, input } => Some((client, input)),
            _ => None,
        })
        .await;
    assert!(matches!(input, RemoteInput::Text(t) if t.text == "ls"));
    phone
        .send(&Message::Paste(Paste {
            pane: 1,
            text: "x\ny".to_owned(),
        }))
        .await;
    let (_, input) = rig
        .wait(|e| match e {
            RemoteEvent::Input { client, input } => Some((client, input)),
            _ => None,
        })
        .await;
    assert!(matches!(input, RemoteInput::Paste(p) if p.text == "x\ny"));

    // A pane the client does not subscribe to: refused, and nothing reaches
    // the UI.
    for message in [
        Message::Key(Key { pane: 2, ..key }),
        Message::TextInput(TextInput {
            pane: 2,
            text: "no".to_owned(),
        }),
        Message::Paste(Paste {
            pane: 2,
            text: "no".to_owned(),
        }),
    ] {
        phone.send(&message).await;
        expect_error(&mut phone, ErrorCode::NotSubscribed, Some(2)).await;
    }
    while let Ok(event) = rig.events.try_recv() {
        assert!(
            !matches!(event, RemoteEvent::Input { .. }),
            "input of a pane not subscribed reached the UI"
        );
    }
}

#[tokio::test]
async fn scrollback_requests_reach_the_ui_and_its_page_reaches_the_phone() {
    let mut rig = Rig::up().await;
    let (mut phone, client) = rig.session_on(&[1, 2]).await;
    subscribe(&mut phone, 1).await;
    rig.subscriptions().await;

    let request = ScrollbackRequest {
        id: 7,
        pane: 1,
        offset: 0,
        count: 20,
    };
    phone.send(&Message::ScrollbackRequest(request)).await;
    let (who, got) = rig
        .wait(|e| match e {
            RemoteEvent::Scrollback { client, request } => Some((client, request)),
            _ => None,
        })
        .await;
    assert_eq!((who, got), (client, request));

    let page = Message::ScrollbackPage(ScrollbackPage {
        id: 7,
        pane: 1,
        history: 0,
        dropped: 0,
        offset: 0,
        styles: vec![],
        lines: vec![],
    });
    rig.host.send(Command::Send {
        client,
        message: page.clone(),
    });
    assert_eq!(phone.message().await, page);

    // Not subscribed: refused with the id of the request.
    phone
        .send(&Message::ScrollbackRequest(ScrollbackRequest {
            id: 8,
            pane: 2,
            ..request
        }))
        .await;
    let Message::Error(error) = phone.message().await else {
        panic!("expected error");
    };
    assert_eq!(error.code, ErrorCode::NotSubscribed);
    assert_eq!((error.id, error.pane), (Some(8), Some(2)));
}

#[tokio::test]
async fn a_pane_that_leaves_the_tree_ends_its_subscription() {
    let mut rig = Rig::up().await;
    let (mut phone, _client) = rig.session_on(&[1, 2]).await;
    subscribe(&mut phone, 1).await;
    subscribe(&mut phone, 2).await;
    assert_eq!(rig.subscriptions().await, [1]);
    assert_eq!(rig.subscriptions().await, [1, 2]);
    rig.frame(2, "kept");
    assert!(matches!(phone.message().await, Message::GridSnapshot(_)));

    rig.host.send(Command::SetWorkspace(workspace_with(&[1])));
    assert!(matches!(phone.message().await, Message::Workspace(_)));
    let Message::SubscriptionEnded(ended) = phone.message().await else {
        panic!("expected subscription-ended");
    };
    assert_eq!(ended.pane, 2);
    assert_eq!(ended.reason, SubscriptionEndReason::PaneClosed);
    assert_eq!(rig.subscriptions().await, [1]);

    // The pane is gone: nothing to subscribe to, and no stale frame when a
    // pane with that id is back in the tree.
    subscribe(&mut phone, 2).await;
    expect_error(&mut phone, ErrorCode::UnknownPane, Some(2)).await;
    rig.host
        .send(Command::SetWorkspace(workspace_with(&[1, 2])));
    assert!(matches!(phone.message().await, Message::Workspace(_)));
    subscribe(&mut phone, 2).await;
    phone.quiet(QUIET).await;
}

#[tokio::test]
async fn the_union_of_subscriptions_follows_every_client_including_one_that_drops() {
    let mut rig = Rig::up().await;
    let (mut a, _client_a) = rig.session_on(&[1, 2, 3]).await;
    let (mut b, _invite, _client_b) = rig.session(&device()).await;
    assert!(matches!(b.message().await, Message::Workspace(_)));

    subscribe(&mut a, 2).await;
    assert_eq!(rig.subscriptions().await, [2]);
    subscribe(&mut a, 1).await;
    assert_eq!(rig.subscriptions().await, [1, 2]);
    // The same pane from another client changes nothing: the next event is
    // the one that brings pane 3.
    subscribe(&mut b, 2).await;
    subscribe(&mut b, 3).await;
    assert_eq!(rig.subscriptions().await, [1, 2, 3]);

    a.close().await;
    assert_eq!(rig.subscriptions().await, [2, 3]);
    b.close().await;
    assert!(rig.subscriptions().await.is_empty());
}

// ---------------------------------------------------------------------------
// view-tab (spec 03 §4.4)
// ---------------------------------------------------------------------------

impl Rig {
    /// The next change of what some client shows.
    async fn viewing(&mut self) -> (ClientId, Option<u64>) {
        self.wait(|e| match e {
            RemoteEvent::ClientViewing { client, tab } => Some((client, tab)),
            _ => None,
        })
        .await
    }
}

async fn view_tab(phone: &mut PhoneClient, tab: Option<u64>) {
    phone.send(&Message::ViewTab(ViewTab { tab })).await;
}

#[tokio::test]
async fn view_tab_tells_the_ui_and_repeating_it_says_nothing() {
    let mut rig = Rig::up().await;
    // The tabs of this tree are 101 and 102.
    let (mut phone, client) = rig.session_on(&[1, 2]).await;

    view_tab(&mut phone, Some(101)).await;
    assert_eq!(rig.viewing().await, (client, Some(101)));
    // The same tab again: nothing. The next event is the one for 102.
    view_tab(&mut phone, Some(101)).await;
    view_tab(&mut phone, Some(102)).await;
    assert_eq!(rig.viewing().await, (client, Some(102)));
    view_tab(&mut phone, None).await;
    assert_eq!(rig.viewing().await, (client, None));
    // None again: nothing; the session is alive and silent.
    view_tab(&mut phone, None).await;
    phone.quiet(QUIET).await;
    while let Ok(event) = rig.events.try_recv() {
        assert!(!matches!(event, RemoteEvent::ClientViewing { .. }));
    }
}

#[tokio::test]
async fn view_tab_finds_tabs_pinned_loose_and_in_groups_of_any_window() {
    use crate::protocol::message::Group;
    let mut rig = Rig::up().await;
    let mut workspace = workspace_with(&[1, 2, 3]);
    let tabs = |workspace: &mut Workspace, window: usize| -> Vec<Tab> {
        workspace.windows[window]
            .items
            .drain(..)
            .map(|item| match item {
                Item::Tab(tab) => tab,
                Item::Group(_) => unreachable!(),
            })
            .collect()
    };
    let mut all = tabs(&mut workspace, 0);
    let third = all.pop().expect("three tabs");
    let second = all.pop().expect("three tabs");
    let first = all.pop().expect("three tabs");
    let mut other = workspace.windows[0].clone();
    other.id = 2;
    other.pinned = vec![];
    other.items = vec![Item::Tab(third)];
    workspace.windows[0].pinned = vec![first];
    workspace.windows[0].items = vec![Item::Group(Group {
        id: 9,
        name: "g".to_owned(),
        color: 0,
        collapsed: false,
        tabs: vec![second],
    })];
    workspace.windows.push(other);
    rig.host.send(Command::SetWorkspace(workspace));
    let (mut phone, _invite, client) = rig.session(&device()).await;
    assert!(matches!(phone.message().await, Message::Workspace(_)));

    for tab in [101, 102, 103] {
        view_tab(&mut phone, Some(tab)).await;
        assert_eq!(rig.viewing().await, (client, Some(tab)));
    }
}

#[tokio::test]
async fn an_unknown_tab_is_refused_and_clears_what_the_client_showed() {
    let mut rig = Rig::up().await;
    let (mut phone, client) = rig.session_on(&[1]).await;

    // Nothing was shown: the error, and no event (the state did not change).
    view_tab(&mut phone, Some(999)).await;
    expect_error(&mut phone, ErrorCode::UnknownTab, None).await;
    view_tab(&mut phone, Some(101)).await;
    assert_eq!(rig.viewing().await, (client, Some(101)));

    view_tab(&mut phone, Some(999)).await;
    expect_error(&mut phone, ErrorCode::UnknownTab, None).await;
    assert_eq!(rig.viewing().await, (client, None));
    // The session goes on.
    view_tab(&mut phone, Some(101)).await;
    assert_eq!(rig.viewing().await, (client, Some(101)));
}

#[tokio::test]
async fn a_tab_that_leaves_the_tree_is_forgotten() {
    let mut rig = Rig::up().await;
    let (mut phone, client) = rig.session_on(&[1, 2]).await;
    view_tab(&mut phone, Some(102)).await;
    assert_eq!(rig.viewing().await, (client, Some(102)));

    // Another tab leaves: nothing changes for this client.
    rig.host.send(Command::SetWorkspace(workspace_with(&[2])));
    assert!(matches!(phone.message().await, Message::Workspace(_)));
    // The tab it shows leaves.
    rig.host.send(Command::SetWorkspace(workspace_with(&[1])));
    assert!(matches!(phone.message().await, Message::Workspace(_)));
    assert_eq!(rig.viewing().await, (client, None));

    // And it can say it again later: it is a change from none.
    rig.host
        .send(Command::SetWorkspace(workspace_with(&[1, 2])));
    assert!(matches!(phone.message().await, Message::Workspace(_)));
    view_tab(&mut phone, Some(102)).await;
    assert_eq!(rig.viewing().await, (client, Some(102)));
}

#[tokio::test]
async fn view_tab_before_the_hello_is_a_protocol_error() {
    let mut rig = Rig::up().await;
    let (mut phone, _invite) = rig.pair(&device(), "Pixel 9").await;
    view_tab(&mut phone, Some(1)).await;
    expect_error(&mut phone, ErrorCode::Protocol, None).await;
    assert!(matches!(phone.until_closed().await, Seen::Closed(_)));
    while let Ok(event) = rig.events.try_recv() {
        assert!(!matches!(event, RemoteEvent::ClientViewing { .. }));
    }
}
