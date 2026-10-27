// SPDX-License-Identifier: GPL-3.0-or-later

//! Testes de integração do crate: um servidor SSH em processo, com o lado
//! servidor do próprio `russh`, em `127.0.0.1` numa porta livre. Ficam dentro
//! do crate (`cfg(test)`) porque precisam do verificador que aceita qualquer
//! chave de host, que **só existe aqui** -- nenhum código compilado fora de
//! teste aceita chave sem verificar.

use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey, PublicKey};
use russh::server::{self, Auth, Msg, Server as _, Session};
use russh::{Channel, ChannelId, Sig};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

use crate::end::{End, FailKind, LostKind};
use crate::spawn::{AuthMethod, Connection, ConnectionId, OpenError, SshSpawn};
use crate::verify::HostKeyVerifier;
use crate::{SshChannel, runtime};

/// O que o shell de mentira faz quando o canal abre.
#[derive(Clone, Copy)]
enum Script {
    /// Devolve o que recebe.
    Echo,
    /// Escreve uma linha e sai com este código (`exit-status`).
    ExitStatus(u32),
    /// Sai por sinal (`exit-signal`), sem código.
    ExitSignal,
    /// Fecha o canal sem nenhum `exit-*`.
    CloseOnly,
}

#[derive(Default)]
struct ServerLog {
    /// `(colunas, linhas)` de cada `pty-req` e `window-change`, em ordem.
    sizes: Mutex<Vec<(u32, u32)>>,
    /// Quantas chaves públicas o cliente ofereceu.
    key_offers: AtomicUsize,
}

struct ShellServer {
    script: Script,
    authorized: PublicKey,
    log: Arc<ServerLog>,
}

impl server::Server for ShellServer {
    type Handler = Self;

    fn new_client(&mut self, _peer: Option<SocketAddr>) -> Self {
        Self {
            script: self.script,
            authorized: self.authorized.clone(),
            log: Arc::clone(&self.log),
        }
    }
}

impl server::Handler for ShellServer {
    type Error = russh::Error;

    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        _offer: server::PublicKeyOffer<'_>,
    ) -> Result<Auth, Self::Error> {
        self.log.key_offers.fetch_add(1, Ordering::SeqCst);
        Ok(Auth::Accept)
    }

    async fn auth_publickey(
        &mut self,
        _user: &str,
        key: server::VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        if key.public_key.key_data() == self.authorized.key_data() {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.log.sizes.lock().unwrap().push((col_width, row_height));
        session.channel_success(channel)
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        match self.script {
            Script::Echo => {}
            Script::ExitStatus(code) => {
                session.data(channel, b"bye\r\n".to_vec())?;
                session.exit_status_request(channel, code)?;
                session.eof(channel)?;
                session.close(channel)?;
            }
            Script::ExitSignal => {
                session.exit_signal_request(channel, Sig::TERM, false, "", "")?;
                session.close(channel)?;
            }
            Script::CloseOnly => session.close(channel)?,
        }
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if matches!(self.script, Script::Echo) {
            session.data(channel, data.to_vec())?;
        }
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.log.sizes.lock().unwrap().push((col_width, row_height));
        Ok(())
    }
}

/// O verificador de teste: aceita qualquer chave de host.
struct AcceptAny;

impl HostKeyVerifier for AcceptAny {
    fn verify(&self, _host: &str, _port: u16, _key: &PublicKey) -> Result<(), FailKind> {
        Ok(())
    }
}

/// Gerador só para chaves **descartáveis de teste** (nenhuma toca o disco do
/// usuário nem um servidor de verdade). Não é criptograficamente seguro e não
/// tenta ser: o crate não puxa `getrandom` nem `rand` só para isto.
struct TestRng(u64);

impl TestRng {
    fn new() -> Self {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        Self(hasher.finish() | 1)
    }
}

impl russh::keys::ssh_key::rand_core::TryRng for TestRng {
    type Error = std::convert::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        self.try_next_u64().map(|v| (v >> 32) as u32)
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Ok(z ^ (z >> 31))
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        for chunk in dest.chunks_mut(8) {
            let bytes = self.try_next_u64()?.to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
        Ok(())
    }
}

impl russh::keys::ssh_key::rand_core::TryCryptoRng for TestRng {}

fn new_key() -> PrivateKey {
    PrivateKey::random(&mut TestRng::new(), Algorithm::Ed25519).expect("generate key")
}

struct Fixture {
    /// Dono do servidor (e do proxy): precisa viver até o fim do teste.
    rt: Runtime,
    port: u16,
    log: Arc<ServerLog>,
    key_file: PathBuf,
    server: JoinHandle<()>,
}

impl Fixture {
    /// Sobe um servidor que autoriza a chave do cliente gerada aqui.
    fn start(script: Script) -> Self {
        Self::start_authorizing(script, true)
    }

    fn start_authorizing(script: Script, authorize_client_key: bool) -> Self {
        let rt = Runtime::new().unwrap();
        let client_key = new_key();
        let authorized = if authorize_client_key {
            client_key.public_key().clone()
        } else {
            new_key().public_key().clone()
        };
        let log = Arc::new(ServerLog::default());
        let config = Arc::new(server::Config {
            keys: vec![new_key()],
            auth_rejection_time: Duration::from_millis(1),
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..server::Config::default()
        });
        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut srv = ShellServer {
            script,
            authorized,
            log: Arc::clone(&log),
        };
        let server = rt.spawn(async move {
            // `run_on_socket` aceita conexões até a tarefa ser abortada.
            let _ = srv.run_on_socket(config, &listener).await;
        });

        let key_file = std::env::temp_dir().join(format!(
            "porecatu-ssh-test-{}-{port}.key",
            std::process::id()
        ));
        let pem = client_key.to_openssh(LineEnding::LF).unwrap();
        std::fs::write(&key_file, pem.as_bytes()).unwrap();

        Self {
            rt,
            port,
            log,
            key_file,
            server,
        }
    }

    fn spawn(&self) -> SshSpawn {
        self.spawn_to(self.port)
    }

    fn spawn_to(&self, port: u16) -> SshSpawn {
        let mut spawn = SshSpawn::new("127.0.0.1", port, "tester");
        spawn.auth = vec![AuthMethod::Key];
        spawn.identity_file = Some(self.key_file.clone());
        spawn
    }

    /// Um proxy TCP na frente do servidor: abortá-lo derruba a conexão como
    /// uma queda de rede, sem que o servidor diga nada.
    fn proxy(&self) -> (u16, JoinHandle<()>) {
        let listener = self.rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let proxy_port = listener.local_addr().unwrap().port();
        let target = self.port;
        let task = self.rt.spawn(async move {
            // As conexões vivem num `JoinSet` dentro desta tarefa: abortá-la
            // derruba também os sockets, que é a queda que o teste quer.
            let mut connections = tokio::task::JoinSet::new();
            while let Ok((mut client, _)) = listener.accept().await {
                connections.spawn(async move {
                    if let Ok(mut upstream) = TcpStream::connect(("127.0.0.1", target)).await {
                        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
                    }
                });
            }
        });
        (proxy_port, task)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_file(&self.key_file);
    }
}

fn open(spawn: SshSpawn) -> SshChannel {
    SshChannel::open_with(spawn, Arc::new(AcceptAny)).unwrap()
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for: {what}");
}

fn wait_end(channel: &SshChannel) -> End {
    let mut found = None;
    wait_until("the channel to end", || {
        found = channel.try_end();
        found.is_some()
    });
    found.unwrap()
}

/// Lê até o fim de arquivo ou até `needle` aparecer.
fn read_until(channel: &SshChannel, needle: &str) -> String {
    let mut reader = channel.reader();
    let mut seen = Vec::new();
    let mut buf = [0u8; 256];
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                seen.extend_from_slice(&buf[..n]);
                if String::from_utf8_lossy(&seen).contains(needle) {
                    break;
                }
            }
        }
    }
    String::from_utf8_lossy(&seen).into_owned()
}

#[test]
fn key_without_passphrase_authenticates_and_the_server_echoes() {
    let fixture = Fixture::start(Script::Echo);
    let channel = open(fixture.spawn());

    channel.writer().write_all(b"hello\n").unwrap();

    let seen = read_until(&channel, "hello");
    assert!(seen.contains("hello"), "{seen:?}");
    assert_eq!(channel.try_end(), None);
}

#[test]
fn the_pty_gets_the_size_and_a_resize_reaches_the_server() {
    let fixture = Fixture::start(Script::Echo);
    let mut spawn = fixture.spawn();
    spawn.rows = 30;
    spawn.cols = 100;
    let channel = open(spawn);
    wait_until("the pty request", || {
        !fixture.log.sizes.lock().unwrap().is_empty()
    });
    assert_eq!(fixture.log.sizes.lock().unwrap()[0], (100, 30));

    channel.resize(50, 132);

    wait_until("the window change", || {
        fixture.log.sizes.lock().unwrap().contains(&(132, 50))
    });
}

#[test]
fn exit_status_is_a_remote_end_with_the_code() {
    let fixture = Fixture::start(Script::ExitStatus(7));
    let channel = open(fixture.spawn());

    assert_eq!(
        wait_end(&channel),
        End::RemoteEnded {
            code: Some(7),
            signal: None
        }
    );
    // What the server wrote before exiting still reached the reader.
    assert!(read_until(&channel, "bye").contains("bye"));
}

#[test]
fn exit_signal_is_a_remote_end_with_the_signal_and_no_code() {
    let fixture = Fixture::start(Script::ExitSignal);
    let channel = open(fixture.spawn());

    assert_eq!(
        wait_end(&channel),
        End::RemoteEnded {
            code: None,
            signal: Some("TERM".to_owned())
        }
    );
}

#[test]
fn a_channel_closed_without_exit_is_a_remote_end_with_nothing() {
    let fixture = Fixture::start(Script::CloseOnly);
    let channel = open(fixture.spawn());

    assert_eq!(
        wait_end(&channel),
        End::RemoteEnded {
            code: None,
            signal: None
        }
    );
}

#[test]
fn a_dropped_tcp_connection_is_lost_not_ended() {
    let fixture = Fixture::start(Script::Echo);
    let (proxy_port, proxy) = fixture.proxy();
    let channel = open(fixture.spawn_to(proxy_port));
    channel.writer().write_all(b"up\n").unwrap();
    assert!(read_until(&channel, "up").contains("up"));

    proxy.abort();

    assert_eq!(wait_end(&channel), End::Lost(LostKind::Transport));
}

#[test]
fn a_server_that_never_answers_times_out() {
    let rt = Runtime::new().unwrap();
    let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    // Accepts the TCP connection and keeps it open, saying nothing.
    let _silent = rt.spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });
    let mut spawn = SshSpawn::new("127.0.0.1", port, "tester");
    spawn.connect_timeout = Duration::from_millis(300);

    let start = Instant::now();
    let channel = open(spawn);
    let end = wait_end(&channel);

    assert_eq!(end, End::Failed(FailKind::Timeout));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn production_refuses_a_host_key_it_cannot_verify() {
    let fixture = Fixture::start(Script::Echo);

    // `SshChannel::open` is the production path: no test verifier.
    let channel = SshChannel::open(fixture.spawn()).unwrap();

    assert_eq!(wait_end(&channel), End::Failed(FailKind::HostKeyUnverified));
    // The key was never even offered to the server.
    assert_eq!(fixture.log.key_offers.load(Ordering::SeqCst), 0);
}

#[test]
fn a_refused_connection_is_refused() {
    // Port 1 on loopback: nothing listens there.
    let channel = open(SshSpawn::new("127.0.0.1", 1, "tester"));

    assert_eq!(wait_end(&channel), End::Failed(FailKind::Refused));
}

#[test]
fn a_key_the_server_does_not_know_is_rejected() {
    let fixture = Fixture::start_authorizing(Script::Echo, false);
    let channel = open(fixture.spawn());

    assert_eq!(wait_end(&channel), End::Failed(FailKind::AuthRejected));
}

#[test]
fn a_missing_key_file_is_unreadable() {
    let fixture = Fixture::start(Script::Echo);
    let mut spawn = fixture.spawn();
    spawn.identity_file = Some(fixture.key_file.with_extension("missing"));
    let channel = open(spawn);

    assert_eq!(wait_end(&channel), End::Failed(FailKind::KeyUnreadable));
}

#[test]
fn an_absent_agent_is_skipped_and_the_next_method_authenticates() {
    let fixture = Fixture::start(Script::Echo);
    let mut spawn = fixture.spawn();
    // Agent first (RF-20.9). Whatever agent this machine has -- or none -- the
    // server only authorizes the key file, so the key is what gets us in.
    spawn.auth = vec![AuthMethod::Agent, AuthMethod::Key];
    let channel = open(spawn);

    channel.writer().write_all(b"hi\n").unwrap();

    assert!(read_until(&channel, "hi").contains("hi"));
    assert_eq!(channel.try_end(), None);
}

#[test]
fn a_method_outside_the_auth_list_is_never_tried() {
    let fixture = Fixture::start(Script::Echo);
    let mut spawn = fixture.spawn();
    // The key file is there, but "key" is not in the list.
    spawn.auth = vec![AuthMethod::Password];
    let channel = open(spawn);

    assert_eq!(wait_end(&channel), End::Failed(FailKind::AuthRejected));
    assert_eq!(fixture.log.key_offers.load(Ordering::SeqCst), 0);
}

#[test]
fn sharing_a_connection_is_not_implemented_yet() {
    let mut spawn = SshSpawn::new("127.0.0.1", 22, "tester");
    spawn.connection = Connection::Share(ConnectionId::next());

    let result = SshChannel::open_with(spawn, Arc::new(AcceptAny));

    assert_eq!(result.err(), Some(OpenError::ShareUnsupported));
}

#[test]
fn the_runtime_is_not_created_until_the_first_connection() {
    // Cheap guard for ADR-0072 §3: nothing in this module touches the runtime
    // by merely existing. (A test binary shares one process, so this can only
    // assert that spawning works and returns -- the on-demand creation is
    // `OnceLock` itself.)
    assert!(runtime::spawn(async {}));
}
