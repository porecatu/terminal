// SPDX-License-Identifier: GPL-3.0-or-later

//! The host handle and the actor behind it (ADR-0104 §2 and §3, ADR-0106).
//!
//! [`RemoteHost`] owns a `tokio` runtime with two workers and one task, the
//! **actor**, that owns every piece of state: the identity, the list of paired
//! devices, the pairing token, what the UI last said about the workspace, the
//! relay link and one [`Session`] per channel. The rest of the program talks
//! to it through a bounded command queue (`send` never blocks) and hears from
//! it through the `on_event` callback.
//!
//! # The actor loop
//!
//! One loop, one `await` point: [`next_wake`] races, by hand (the crate does
//! not enable the `tokio` `macros` feature), the stop order, the relay link's
//! events, the command queue, the "last frame wins" slots and the nearest
//! deadline of any session. Everything that follows is synchronous code on
//! [`Core`], except the few steps that talk to the OS vault (identity
//! recreation) and the flush before the link closes.
//!
//! # Channels never talk to the relay by themselves
//!
//! A session turns a payload into [`HostEvent`](crate::protocol::HostEvent)s
//! and says what it wants (a record to send, a close); the actor puts it on
//! the link. The relay is never told why a channel was refused: only
//! `host-rejected` or `normal`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::Poll;
use std::time::{Duration, Instant, SystemTime};

use ring::rand::{SecureRandom, SystemRandom};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::task::JoinHandle;
use zeroize::Zeroizing;

use crate::api::{
    ClientId, Command, PairingAnswer, PairingId, RelayFailure, RelayStatus, RemoteEvent,
    RemoteParams, StopReason,
};
use crate::devices::{DeviceStore, LoadNote};
use crate::grid_flow::Frame;
use crate::name::machine_name;
use crate::protocol::envelope::ChannelId;
use crate::protocol::host_id::HostId;
use crate::protocol::message::{
    GoodbyeReason, GridSnapshot, Item, Message, PaneNode, Profiles, Tab, Theme, Workspace,
};
use crate::protocol::pairing_uri::{PROTOCOL_MAJOR, PairingUri, normalize_relay_url};
use crate::protocol::session::{PairingToken, StaticKeypair, TOKEN_TTL_MS};
use crate::relay::{LinkCommand, LinkEvent, RelayConfig, RelayLink, Timing};
use crate::session::Session;
use crate::time::Clock;
use crate::vault::{
    HostIdentity, SystemVault, Vault, load_or_create_identity, operator_token, recreate_identity,
};

/// How many commands wait for the actor before `send` starts discarding.
const COMMAND_QUEUE: usize = 256;

/// Longest the name of the host shown on the phone may be, in scalar values.
const HOST_NAME_LIMIT: usize = 32;

/// Time given to the link to put the last records on the wire before it is
/// closed: the stop signal of the link wins over queued commands, so the
/// `goodbye`s need a moment to leave first.
const FLUSH_GRACE: Duration = Duration::from_millis(150);

/// How long `stop` lets the actor finish before the runtime is torn down.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// The waits of the host. Tests shrink them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tuning {
    /// Waits of the relay link.
    pub link: Timing,
    /// Time a channel of a known device has to send its `hello`.
    pub hello_ms: u64,
    /// Time the host waits for the user's answer to a pairing request, a
    /// little longer than the 120 s of the UI dialog (ADR-0106).
    pub pairing_backstop_ms: u64,
}

impl Tuning {
    /// The values the host runs with.
    pub(crate) const PRODUCTION: Tuning = Tuning {
        link: Timing::PRODUCTION,
        hello_ms: 15_000,
        pairing_backstop_ms: 130_000,
    };
}

/// 16 random bytes per **process** (not per host): the phone tells a restart
/// of the app from a reconnection by it.
fn host_instance() -> [u8; 16] {
    static INSTANCE: OnceLock<[u8; 16]> = OnceLock::new();
    *INSTANCE.get_or_init(|| {
        let mut bytes = [0u8; 16];
        // A failing system generator is not something to continue past, but
        // a constant would be worse than a weak value only if it leaked; the
        // instance is not a secret, only an identifier.
        let _ = SystemRandom::new().fill(&mut bytes);
        bytes
    })
}

/// "Last one wins" slots for the screens of the panes.
struct FrameSlots {
    frames: Mutex<HashMap<u64, Box<GridSnapshot>>>,
    wake: Notify,
}

impl FrameSlots {
    fn put(&self, pane: u64, frame: Box<GridSnapshot>) {
        self.frames
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(pane, frame);
        self.wake.notify_one();
    }

    fn take(&self) -> HashMap<u64, Box<GridSnapshot>> {
        std::mem::take(&mut *self.frames.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// Handle to the remote access host. Owns a multi-thread `tokio` runtime with
/// 2 workers; stopping or dropping it destroys the runtime without waiting
/// for the network.
pub struct RemoteHost {
    runtime: Option<tokio::runtime::Runtime>,
    commands: mpsc::Sender<Command>,
    frames: Arc<FrameSlots>,
    /// Commands discarded because the queue was full.
    dropped: Arc<AtomicU64>,
    stop: Option<oneshot::Sender<StopReason>>,
    task: Option<JoinHandle<()>>,
}

impl RemoteHost {
    /// Creates the runtime (2 workers, threads named `porecatu-remote`) and
    /// starts the actor, which loads the identity and the list of devices and
    /// connects to the relay off the calling thread. Fails only if the
    /// runtime cannot be created.
    pub fn start(
        params: RemoteParams,
        on_event: impl Fn(RemoteEvent) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        Self::start_inner(
            params,
            Arc::new(SystemVault),
            Arc::new(on_event),
            Tuning::PRODUCTION,
        )
    }

    /// [`start`](Self::start) with an explicit vault (tests).
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn start_with(
        params: RemoteParams,
        vault: Arc<dyn Vault>,
        on_event: impl Fn(RemoteEvent) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        Self::start_with_tuning(params, vault, on_event, test_tuning())
    }

    /// [`start_with`](Self::start_with) with explicit waits (tests).
    #[cfg(test)]
    pub(crate) fn start_with_tuning(
        params: RemoteParams,
        vault: Arc<dyn Vault>,
        on_event: impl Fn(RemoteEvent) + Send + Sync + 'static,
        tuning: Tuning,
    ) -> std::io::Result<Self> {
        Self::start_inner(params, vault, Arc::new(on_event), tuning)
    }

    fn start_inner(
        params: RemoteParams,
        vault: Arc<dyn Vault>,
        emit: Arc<dyn Fn(RemoteEvent) + Send + Sync>,
        tuning: Tuning,
    ) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("porecatu-remote")
            .enable_all()
            .build()?;
        let (commands, cmd_rx) = mpsc::channel(COMMAND_QUEUE);
        let (stop_tx, stop_rx) = oneshot::channel();
        let frames = Arc::new(FrameSlots {
            frames: Mutex::new(HashMap::new()),
            wake: Notify::new(),
        });
        let task = runtime.spawn(run(Setup {
            params,
            vault,
            emit,
            tuning,
            cmd_rx,
            stop_rx,
            frames: frames.clone(),
        }));
        Ok(Self {
            runtime: Some(runtime),
            commands,
            frames,
            dropped: Arc::new(AtomicU64::new(0)),
            stop: Some(stop_tx),
            task: Some(task),
        })
    }

    /// Hands a command to the host. Never blocks: a full queue discards the
    /// command (and counts it). A frame goes to its own slot, where it
    /// replaces the previous frame of the same pane.
    pub fn send(&self, command: Command) {
        match command {
            Command::Frame { pane, frame } => self.frames.put(pane, frame),
            other => {
                if self.commands.try_send(other).is_err() {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    /// Stops the host: tells every session goodbye (`reason`), closes the
    /// channels and the relay connection, and destroys the runtime. Returns
    /// at once; the runtime is torn down on a thread of its own after at most
    /// about two seconds of grace.
    pub fn stop(mut self, reason: StopReason) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(reason);
        }
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        let task = self.task.take();
        let slot = Arc::new(Mutex::new(Some(runtime)));
        let thread_slot = slot.clone();
        let spawned = std::thread::Builder::new()
            .name("porecatu-remote-stop".into())
            .spawn(move || {
                let runtime = thread_slot.lock().ok().and_then(|mut s| s.take());
                let Some(runtime) = runtime else { return };
                if let Some(task) = task {
                    // The timer must be created inside the runtime.
                    let _ = runtime.block_on(async { tokio::time::timeout(STOP_WAIT, task).await });
                }
                runtime.shutdown_timeout(Duration::from_millis(500));
            });
        if spawned.is_err() {
            // No thread to wait on: do not block this one.
            let runtime = slot.lock().ok().and_then(|mut s| s.take());
            if let Some(runtime) = runtime {
                runtime.shutdown_background();
            }
        }
    }

    /// How many commands were discarded because the queue was full.
    #[cfg(test)]
    pub(crate) fn dropped_commands(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Drop for RemoteHost {
    fn drop(&mut self) {
        // A runtime dropped normally waits for blocking tasks (the OS vault);
        // this may be the main thread.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// Short waits for the tests.
#[cfg(test)]
pub(crate) fn test_tuning() -> Tuning {
    Tuning {
        link: Timing {
            backoff_base: Duration::from_millis(20),
            backoff_cap: Duration::from_millis(80),
            long_wait: Duration::from_millis(80),
            replaced_wait: Duration::from_millis(120),
            register_timeout: Duration::from_secs(2),
            connect_timeout: Duration::from_secs(2),
            close_grace: Duration::from_millis(500),
        },
        hello_ms: 15_000,
        pairing_backstop_ms: 130_000,
    }
}

// ---------------------------------------------------------------------------
// The actor
// ---------------------------------------------------------------------------

/// What the actor task is started with.
struct Setup {
    params: RemoteParams,
    vault: Arc<dyn Vault>,
    emit: Arc<dyn Fn(RemoteEvent) + Send + Sync>,
    tuning: Tuning,
    cmd_rx: mpsc::Receiver<Command>,
    stop_rx: oneshot::Receiver<StopReason>,
    frames: Arc<FrameSlots>,
}

/// What start-up reads from the vault and from the disk.
struct Init {
    identity: HostIdentity,
    token: Option<Zeroizing<String>>,
    devices: DeviceStore,
    note: LoadNote,
}

/// Blocking start-up: the OS vault and the disk. Never on the main thread,
/// never on a worker (it runs in `spawn_blocking`).
fn init(vault: &dyn Vault, data_dir: &std::path::Path) -> Result<Init, RelayFailure> {
    let (identity, _origin) =
        load_or_create_identity(vault).map_err(|_| RelayFailure::VaultUnavailable)?;
    let token = operator_token(vault).map_err(|_| RelayFailure::VaultUnavailable)?;
    let (devices, note) = DeviceStore::load(data_dir);
    Ok(Init {
        identity,
        token,
        devices,
        note,
    })
}

/// The name shown on the phone: at most 32 scalar values, never empty.
fn display_name(name: &str) -> String {
    let truncated: String = name.chars().take(HOST_NAME_LIMIT).collect();
    if truncated.trim().is_empty() {
        machine_name().chars().take(HOST_NAME_LIMIT).collect()
    } else {
        truncated
    }
}

/// The task body.
async fn run(setup: Setup) {
    let Setup {
        params,
        vault,
        emit,
        tuning,
        cmd_rx,
        mut stop_rx,
        frames,
    } = setup;
    let mut cmd_rx = cmd_rx;

    let loaded = {
        let vault = vault.clone();
        let dir = params.data_dir.clone();
        tokio::task::spawn_blocking(move || init(vault.as_ref(), &dir)).await
    };
    let init = match loaded {
        Ok(Ok(init)) => init,
        Ok(Err(failure)) => {
            emit(RemoteEvent::Status(RelayStatus::Failed(failure)));
            return;
        }
        Err(_) => {
            emit(RemoteEvent::Status(RelayStatus::Failed(
                RelayFailure::VaultUnavailable,
            )));
            return;
        }
    };

    let mut core = Core::new(&params, vault, emit, tuning, init);
    core.start();

    loop {
        let delay = core
            .next_deadline()
            .map(|at| Duration::from_millis(at.saturating_sub(core.now_ms())));
        let wake = next_wake(&mut cmd_rx, &mut core.link_rx, &mut stop_rx, &frames, delay).await;
        match wake {
            Wake::Stop(reason) => {
                core.shutdown(reason).await;
                return;
            }
            Wake::Closed => return,
            Wake::Link(event) => core.on_link_event(event),
            Wake::Command(command) => core.on_command(command).await,
            Wake::Frames => core.on_frames(frames.take()),
            Wake::Tick => core.tick(),
        }
        core.sync_subscriptions();
    }
}

/// What the actor loop woke up for.
enum Wake {
    Stop(StopReason),
    Command(Command),
    Link(LinkEvent),
    Frames,
    Tick,
    /// The handle is gone without `stop`.
    Closed,
}

/// Waits for the next thing to do. Priority: stop, link, commands, frames,
/// deadline.
async fn next_wake(
    cmd_rx: &mut mpsc::Receiver<Command>,
    link_rx: &mut Option<mpsc::UnboundedReceiver<LinkEvent>>,
    stop_rx: &mut oneshot::Receiver<StopReason>,
    frames: &FrameSlots,
    delay: Option<Duration>,
) -> Wake {
    let notified = frames.wake.notified();
    let mut notified = std::pin::pin!(notified);
    let sleep = tokio::time::sleep(delay.unwrap_or(Duration::from_secs(86_400)));
    let mut sleep = std::pin::pin!(sleep);
    poll_fn(|cx| {
        if let Poll::Ready(stop) = Pin::new(&mut *stop_rx).poll(cx) {
            return Poll::Ready(match stop {
                Ok(reason) => Wake::Stop(reason),
                Err(_) => Wake::Closed,
            });
        }
        let polled = link_rx.as_mut().map(|rx| rx.poll_recv(cx));
        match polled {
            Some(Poll::Ready(Some(event))) => return Poll::Ready(Wake::Link(event)),
            // The link task ended on its own (a failure it does not retry).
            Some(Poll::Ready(None)) => *link_rx = None,
            _ => {}
        }
        match cmd_rx.poll_recv(cx) {
            Poll::Ready(Some(command)) => return Poll::Ready(Wake::Command(command)),
            Poll::Ready(None) => return Poll::Ready(Wake::Closed),
            Poll::Pending => {}
        }
        if notified.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Wake::Frames);
        }
        if delay.is_some() && sleep.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Wake::Tick);
        }
        Poll::Pending
    })
    .await
}

// ---------------------------------------------------------------------------
// The state
// ---------------------------------------------------------------------------

/// Everything the actor owns. Sessions reach into it through `&mut Core`
/// while they are out of the `sessions` map (see [`Core::with_session`]).
pub(crate) struct Core {
    vault: Arc<dyn Vault>,
    emit: Arc<dyn Fn(RemoteEvent) + Send + Sync>,
    pub(crate) tuning: Tuning,
    clock: Clock,
    relay_url: String,
    pub(crate) host_name: String,
    pub(crate) porecatu_version: String,
    identity: HostIdentity,
    pub(crate) static_keypair: StaticKeypair,
    static_public: [u8; 32],
    pub(crate) host_id: HostId,
    operator_token: Option<Zeroizing<String>>,
    link: Option<RelayLink>,
    link_rx: Option<mpsc::UnboundedReceiver<LinkEvent>>,
    /// Registered at the relay on the current connection.
    registered: bool,
    pub(crate) devices: DeviceStore,
    /// At most one live pairing token.
    pub(crate) token: Option<PairingToken>,
    pub(crate) theme: Option<Theme>,
    pub(crate) profiles: Option<Profiles>,
    pub(crate) workspace: Option<Workspace>,
    sessions: BTreeMap<u64, Session>,
    next_client: u64,
    next_pairing: u64,
    /// The last screen of every pane of the tree ("last one wins"): the one
    /// copy all the subscriptions of a pane read.
    latest_frames: HashMap<u64, Frame>,
    /// The panes of the last tree the UI sent.
    tree_panes: HashSet<u64>,
    /// The union of the panes subscribed, as the UI last heard it.
    reported_subscriptions: Vec<u64>,
    quarantine: Option<std::path::PathBuf>,
}

impl Core {
    fn new(
        params: &RemoteParams,
        vault: Arc<dyn Vault>,
        emit: Arc<dyn Fn(RemoteEvent) + Send + Sync>,
        tuning: Tuning,
        init: Init,
    ) -> Core {
        let Init {
            identity,
            token,
            devices,
            note,
        } = init;
        let quarantine = match note {
            LoadNote::Quarantined { path } => Some(path),
            _ => None,
        };
        let static_keypair = identity.static_keypair();
        let static_public = identity.static_public();
        let host_id = identity.host_id();
        Core {
            vault,
            emit,
            tuning,
            clock: Clock::new(),
            relay_url: params.relay_url.clone(),
            host_name: display_name(&params.host_name),
            porecatu_version: params.porecatu_version.clone(),
            identity,
            static_keypair,
            static_public,
            host_id,
            operator_token: token,
            link: None,
            link_rx: None,
            registered: false,
            devices,
            token: None,
            theme: None,
            profiles: None,
            workspace: None,
            sessions: BTreeMap::new(),
            next_client: 1,
            next_pairing: 1,
            latest_frames: HashMap::new(),
            tree_panes: HashSet::new(),
            reported_subscriptions: Vec::new(),
            quarantine,
        }
    }

    /// The first events and the first connection.
    fn start(&mut self) {
        self.emit(RemoteEvent::Devices(self.devices.list()));
        if let Some(path) = self.quarantine.take() {
            self.emit(RemoteEvent::DevicesQuarantined { path });
        }
        if self.operator_token.is_none() {
            self.emit(RemoteEvent::Status(RelayStatus::Failed(
                RelayFailure::TokenMissing,
            )));
            return;
        }
        self.start_link();
    }

    pub(crate) fn emit(&self, event: RemoteEvent) {
        (self.emit)(event);
    }

    pub(crate) fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    pub(crate) fn next_client(&mut self) -> ClientId {
        let id = ClientId(self.next_client);
        self.next_client += 1;
        id
    }

    pub(crate) fn next_pairing(&mut self) -> PairingId {
        let id = PairingId(self.next_pairing);
        self.next_pairing += 1;
        id
    }

    pub(crate) fn link_send(&self, command: LinkCommand) {
        if let Some(link) = &self.link {
            link.send(command);
        }
    }

    pub(crate) fn host_instance(&self) -> [u8; 16] {
        host_instance()
    }

    /// Writes the list of devices and tells the UI. A failed write is not
    /// fatal: the list stays right in memory and the next change writes it.
    pub(crate) fn persist_devices(&mut self) {
        let devices = &self.devices;
        let _ = tokio::task::block_in_place(|| devices.save());
        self.emit(RemoteEvent::Devices(self.devices.list()));
    }

    /// Records the end of a session of a paired device.
    pub(crate) fn touch_last_seen(&mut self, key: &[u8; 32]) {
        if !self.devices.contains(key) {
            return;
        }
        self.devices.touch_last_seen(key, SystemTime::now());
        self.persist_devices();
    }

    // -- the link ---------------------------------------------------------

    fn start_link(&mut self) {
        let Some(token) = self.operator_token.clone() else {
            return;
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let config = RelayConfig {
            url: self.relay_url.clone(),
            token,
            signing_key: self.identity.signing_key(),
        };
        self.link = Some(RelayLink::spawn_with_timing(
            &tokio::runtime::Handle::current(),
            config,
            tx,
            self.tuning.link,
        ));
        self.link_rx = Some(rx);
        self.registered = false;
    }

    async fn stop_link(&mut self) {
        self.link_rx = None;
        self.registered = false;
        if let Some(link) = self.link.take() {
            tokio::time::sleep(FLUSH_GRACE).await;
            link.shutdown().await;
        }
    }

    fn on_link_event(&mut self, event: LinkEvent) {
        match event {
            // `Connected` is reported once registered, so the UI never sees
            // it before `ShowPairing` can be answered.
            LinkEvent::Status(RelayStatus::Connected) => {}
            LinkEvent::Status(status) => self.emit(RemoteEvent::Status(status)),
            LinkEvent::Registered { host_id } => {
                self.registered = host_id == self.host_id;
                self.emit(RemoteEvent::Status(RelayStatus::Connected));
            }
            LinkEvent::ChannelOpened(channel) => {
                let session = Session::new(channel, self);
                self.sessions.insert(channel.0, session);
            }
            LinkEvent::ChannelData(channel, bytes) => {
                self.with_session(channel, |session, core| session.on_data(core, &bytes));
            }
            LinkEvent::ChannelClosed(channel, _reason) => {
                // The other side left: nothing to answer.
                if let Some(session) = self.sessions.remove(&channel.0) {
                    session.end(self);
                }
            }
            LinkEvent::Down => {
                self.registered = false;
                // The relay numbers channels per connection: nothing survives.
                for session in std::mem::take(&mut self.sessions).into_values() {
                    session.end(self);
                }
            }
        }
    }

    // -- sessions ---------------------------------------------------------

    /// Runs `f` on a session with the rest of the state, then keeps the
    /// session or, if it asked to end, closes its channel.
    fn with_session(&mut self, channel: ChannelId, f: impl FnOnce(&mut Session, &mut Core)) {
        let Some(mut session) = self.sessions.remove(&channel.0) else {
            return;
        };
        f(&mut session, self);
        self.settle(channel, session);
    }

    fn settle(&mut self, channel: ChannelId, session: Session) {
        match session.closing() {
            Some(reason) => {
                self.link_send(LinkCommand::CloseChannel(channel, reason));
                session.end(self);
            }
            None => {
                self.sessions.insert(channel.0, session);
            }
        }
    }

    fn channels(&self) -> Vec<ChannelId> {
        self.sessions.keys().copied().map(ChannelId).collect()
    }

    fn channel_of_client(&self, client: ClientId) -> Option<ChannelId> {
        self.sessions
            .iter()
            .find(|(_, s)| s.client() == Some(client))
            .map(|(&c, _)| ChannelId(c))
    }

    /// Runs `f` on every session in session (past the `hello`).
    fn each_active(&mut self, mut f: impl FnMut(&mut Session, &mut Core)) {
        for channel in self.channels() {
            self.with_session(channel, |session, core| {
                if session.client().is_some() {
                    f(session, core);
                }
            });
        }
    }

    fn next_deadline(&self) -> Option<u64> {
        self.sessions
            .values()
            .filter_map(|session| session.next_deadline(self))
            .min()
    }

    fn tick(&mut self) {
        for channel in self.channels() {
            self.with_session(channel, |session, core| session.tick(core));
        }
    }

    // -- the grid ---------------------------------------------------------

    /// Whether `pane` is in the last tree the UI sent.
    pub(crate) fn has_pane(&self, pane: u64) -> bool {
        self.tree_panes.contains(&pane)
    }

    /// The last screen of `pane`, if the UI sent one.
    pub(crate) fn latest_frame(&self, pane: u64) -> Option<&Frame> {
        self.latest_frames.get(&pane)
    }

    /// Frames from the UI, at most one per pane: kept as the latest of the
    /// pane (only for panes of the tree) and offered to its subscriptions.
    fn on_frames(&mut self, frames: HashMap<u64, Box<GridSnapshot>>) {
        for (pane, frame) in frames {
            if !self.has_pane(pane) {
                continue;
            }
            self.latest_frames.insert(pane, Frame::from(frame));
            for channel in self.channels() {
                self.with_session(channel, |session, core| session.poll_pane(core, pane));
            }
        }
    }

    /// A new tree: panes that left lose their frame and their subscriptions.
    fn set_workspace(&mut self, workspace: Workspace) {
        self.tree_panes = panes_of(&workspace);
        let tree = &self.tree_panes;
        self.latest_frames.retain(|pane, _| tree.contains(pane));
        self.workspace = Some(workspace.clone());
        self.each_active(|session, core| {
            session.send_workspace(core, &workspace);
            session.end_gone_subscriptions(core);
        });
    }

    /// Tells the UI when the union of the subscribed panes changed.
    fn sync_subscriptions(&mut self) {
        let union: BTreeSet<u64> = self
            .sessions
            .values()
            .flat_map(Session::subscribed)
            .collect();
        let union: Vec<u64> = union.into_iter().collect();
        if union != self.reported_subscriptions {
            self.reported_subscriptions.clone_from(&union);
            self.emit(RemoteEvent::Subscriptions(union));
        }
    }

    // -- commands ---------------------------------------------------------

    async fn on_command(&mut self, command: Command) {
        match command {
            Command::SetWorkspace(workspace) => self.set_workspace(workspace),
            Command::SetTheme(theme) => {
                self.theme = Some(theme.clone());
                let message = Message::Theme(theme);
                self.each_active(|session, core| session.send(core, &message));
            }
            Command::SetProfiles(profiles) => {
                self.profiles = Some(profiles.clone());
                let message = Message::Profiles(profiles);
                self.each_active(|session, core| session.send(core, &message));
            }
            // `RemoteHost::send` puts frames in their own slots; this arm is
            // for completeness.
            Command::Frame { pane, frame } => self.on_frames(HashMap::from([(pane, frame)])),
            Command::Send { client, message } => {
                if let Some(channel) = self.channel_of_client(client) {
                    self.with_session(channel, |session, core| session.send(core, &message));
                }
            }
            Command::Broadcast(message) => {
                self.each_active(|session, core| session.send(core, &message));
            }
            Command::ShowPairing => self.show_pairing(),
            Command::CancelPairing => self.token = None,
            Command::AnswerPairing { id, answer } => self.answer_pairing(id, answer),
            Command::Disconnect(client) => {
                if let Some(channel) = self.channel_of_client(client) {
                    self.with_session(channel, |session, core| {
                        session.goodbye(core, GoodbyeReason::Normal);
                    });
                }
            }
            Command::DisconnectAll => {
                self.each_active(|session, core| session.goodbye(core, GoodbyeReason::Normal));
            }
            Command::Revoke { key } => self.revoke(&key),
            Command::RecreateIdentity => self.recreate_identity().await,
        }
    }

    /// A new token, an invitation for the QR code.
    fn show_pairing(&mut self) {
        if !self.registered {
            return;
        }
        let Ok(relay) = normalize_relay_url(&self.relay_url) else {
            return;
        };
        let token = PairingToken::generate(self.now_ms());
        let uri = PairingUri {
            major: PROTOCOL_MAJOR,
            relay,
            host_id: self.host_id.clone(),
            host_key: self.static_public,
            token: token.bytes(),
            name: self.host_name.clone(),
        }
        .to_uri();
        // Replaces (and so invalidates) the previous token.
        self.token = Some(token);
        self.emit(RemoteEvent::PairingUri {
            uri,
            expires_at: Instant::now() + Duration::from_millis(TOKEN_TTL_MS),
        });
    }

    fn answer_pairing(&mut self, id: PairingId, answer: PairingAnswer) {
        let channel = self
            .sessions
            .iter()
            .find(|(_, s)| s.pairing_id() == Some(id))
            .map(|(&c, _)| ChannelId(c));
        if let Some(channel) = channel {
            self.with_session(channel, |session, core| session.answer(core, answer));
        }
    }

    fn revoke(&mut self, key: &[u8; 32]) {
        // Out of the list first: ending the sessions then writes nothing.
        let removed = self.devices.remove(key);
        for channel in self.channels() {
            self.with_session(channel, |session, core| {
                if session.device_key() == Some(*key) {
                    session.goodbye(core, GoodbyeReason::Revoked);
                }
            });
        }
        if removed {
            self.persist_devices();
        }
    }

    /// New keys, empty list, new registration (ADR-0106 §1).
    async fn recreate_identity(&mut self) {
        for channel in self.channels() {
            self.with_session(channel, |session, core| {
                session.goodbye(core, GoodbyeReason::Revoked);
            });
        }
        self.token = None;
        self.stop_link().await;

        let vault = self.vault.clone();
        let made = tokio::task::spawn_blocking(move || recreate_identity(vault.as_ref())).await;
        let Ok(Ok(identity)) = made else {
            self.emit(RemoteEvent::Status(RelayStatus::Failed(
                RelayFailure::VaultUnavailable,
            )));
            return;
        };
        self.static_keypair = identity.static_keypair();
        self.static_public = identity.static_public();
        self.host_id = identity.host_id();
        self.identity = identity;

        self.devices.clear();
        self.persist_devices();
        self.start_link();
    }

    /// Says goodbye to everyone, closes the channels and the connection.
    async fn shutdown(&mut self, reason: StopReason) {
        let goodbye = match reason {
            StopReason::Disabled => GoodbyeReason::RemoteDisabled,
            StopReason::Quitting => GoodbyeReason::HostQuitting,
        };
        for channel in self.channels() {
            self.with_session(channel, |session, core| session.goodbye(core, goodbye));
        }
        self.stop_link().await;
    }
}

/// Every pane id of the tree.
fn panes_of(workspace: &Workspace) -> HashSet<u64> {
    fn collect(node: &PaneNode, out: &mut HashSet<u64>) {
        match node {
            PaneNode::Pane(pane) => {
                out.insert(pane.id);
            }
            PaneNode::Split(split) => {
                collect(&split.first, out);
                collect(&split.second, out);
            }
        }
    }
    fn tab(tab: &Tab, out: &mut HashSet<u64>) {
        collect(&tab.layout, out);
    }
    let mut out = HashSet::new();
    for window in &workspace.windows {
        for pinned in &window.pinned {
            tab(pinned, &mut out);
        }
        for item in &window.items {
            match item {
                Item::Tab(t) => tab(t, &mut out),
                Item::Group(group) => group.tabs.iter().for_each(|t| tab(t, &mut out)),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
