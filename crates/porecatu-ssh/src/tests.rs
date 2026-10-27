// SPDX-License-Identifier: GPL-3.0-or-later

//! Testes de integração do crate: um servidor SSH em processo, com o lado
//! servidor do próprio `russh`, em `127.0.0.1` numa porta livre.
//!
//! Nada aqui aceita uma chave de host por atalho: cada teste usa um
//! `known_hosts` de verdade, num diretório temporário, e um `Asker` que
//! responde como o usuário responderia. O `~/.ssh` de quem roda os testes não
//! é lido nem gravado.

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

use crate::ask::{Asker, HostKeyQuestion, Question, Reply};
use crate::end::{End, FailKind, LostKind};
use crate::known_hosts::{KnownHostsFiles, ReadCause};
use crate::spawn::{AuthMethod, Connection, ConnectionId, OpenError, SshSpawn};
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

/// O usuário de mentira: responde às perguntas com o que o teste pede e
/// guarda o que foi perguntado.
struct ScriptedUser {
    reply: Reply,
    asked: Mutex<Vec<HostKeyQuestion>>,
}

impl ScriptedUser {
    fn new(reply: Reply) -> Arc<Self> {
        Arc::new(Self {
            reply,
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<HostKeyQuestion> {
        self.asked.lock().unwrap().clone()
    }
}

impl Asker for ScriptedUser {
    fn ask(&self, question: Question, on_reply: Box<dyn FnOnce(Reply) + Send>) {
        let Question::HostKey(question) = question;
        self.asked.lock().unwrap().push(question);
        on_reply(self.reply);
    }
}

/// Quem nunca deveria ser perguntado.
struct NeverAsked;

impl Asker for NeverAsked {
    fn ask(&self, _question: Question, _on_reply: Box<dyn FnOnce(Reply) + Send>) {
        panic!("this test must not ask anything");
    }
}

struct Fixture {
    /// Dono do servidor (e do proxy): precisa viver até o fim do teste.
    rt: Runtime,
    port: u16,
    log: Arc<ServerLog>,
    dir: PathBuf,
    key_file: PathBuf,
    known_hosts: PathBuf,
    server_key: PublicKey,
    server: JoinHandle<()>,
}

impl Fixture {
    /// Um servidor que autoriza a chave do cliente gerada aqui, **e cuja chave
    /// de host já está no `known_hosts`**.
    fn start(script: Script) -> Self {
        let fixture = Self::start_unregistered(script, true);
        fixture.register_server_key(&fixture.server_key.clone());
        fixture
    }

    /// Como `start`, mas o `known_hosts` não existe.
    fn start_unregistered(script: Script, authorize_client_key: bool) -> Self {
        let rt = Runtime::new().unwrap();
        let client_key = new_key();
        let authorized = if authorize_client_key {
            client_key.public_key().clone()
        } else {
            new_key().public_key().clone()
        };
        let host_key = new_key();
        let server_key = host_key.public_key().clone();
        let log = Arc::new(ServerLog::default());
        let config = Arc::new(server::Config {
            keys: vec![host_key],
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

        let dir =
            std::env::temp_dir().join(format!("porecatu-ssh-test-{}-{port}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key_file = dir.join("id_ed25519");
        let pem = client_key.to_openssh(LineEnding::LF).unwrap();
        std::fs::write(&key_file, pem.as_bytes()).unwrap();

        Self {
            rt,
            port,
            log,
            known_hosts: dir.join("dot_ssh").join("known_hosts"),
            dir,
            key_file,
            server_key,
            server,
        }
    }

    /// Os arquivos de `known_hosts` desta fixture, no lugar dos do usuário.
    fn files(&self) -> KnownHostsFiles {
        KnownHostsFiles {
            user: Some(self.known_hosts.clone()),
            read_only: Vec::new(),
        }
    }

    /// Grava uma linha de `known_hosts` para `[127.0.0.1]:porta` com `key`.
    fn register_server_key(&self, key: &PublicKey) {
        let line = format!("[127.0.0.1]:{} {}\n", self.port, key.to_openssh().unwrap());
        std::fs::create_dir_all(self.known_hosts.parent().unwrap()).unwrap();
        std::fs::write(&self.known_hosts, line).unwrap();
    }

    fn known_hosts_text(&self) -> Option<String> {
        std::fs::read_to_string(&self.known_hosts).ok()
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

    /// Abre um canal contra o servidor desta fixture: o `known_hosts` dela, e
    /// um usuário que nunca deveria ser perguntado.
    fn open(&self, spawn: SshSpawn) -> SshChannel {
        self.open_asking(spawn, Arc::new(NeverAsked))
    }

    fn open_asking(&self, spawn: SshSpawn, asker: Arc<dyn Asker>) -> SshChannel {
        SshChannel::open_with(spawn, asker, self.files()).unwrap()
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
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Um canal sem nenhum `known_hosts` e sem ninguém para perguntar: para os
/// testes em que a conexão nem chega à chave do host.
fn open_without_files(spawn: SshSpawn) -> SshChannel {
    let files = KnownHostsFiles {
        user: None,
        read_only: Vec::new(),
    };
    SshChannel::open_with(spawn, Arc::new(NeverAsked), files).unwrap()
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

// ---------------------------------------------------------------------------
// Autenticação, canal e fim (prompt 09)
// ---------------------------------------------------------------------------

#[test]
fn key_without_passphrase_authenticates_and_the_server_echoes() {
    let fixture = Fixture::start(Script::Echo);
    let channel = fixture.open(fixture.spawn());

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
    let channel = fixture.open(spawn);
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
    let channel = fixture.open(fixture.spawn());

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
    let channel = fixture.open(fixture.spawn());

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
    let channel = fixture.open(fixture.spawn());

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
    // The host is registered under the proxy's port, as the user would see it.
    let line = format!(
        "[127.0.0.1]:{proxy_port} {}\n",
        fixture.server_key.to_openssh().unwrap()
    );
    std::fs::write(&fixture.known_hosts, line).unwrap();
    let channel = fixture.open(fixture.spawn_to(proxy_port));
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
    let channel = open_without_files(spawn);
    let end = wait_end(&channel);

    assert_eq!(end, End::Failed(FailKind::Timeout));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_refused_connection_is_refused() {
    // Port 1 on loopback: nothing listens there.
    let channel = open_without_files(SshSpawn::new("127.0.0.1", 1, "tester"));

    assert_eq!(wait_end(&channel), End::Failed(FailKind::Refused));
}

#[test]
fn a_key_the_server_does_not_know_is_rejected() {
    let fixture = Fixture::start_unregistered(Script::Echo, false);
    fixture.register_server_key(&fixture.server_key.clone());
    let channel = fixture.open(fixture.spawn());

    assert_eq!(wait_end(&channel), End::Failed(FailKind::AuthRejected));
}

#[test]
fn a_missing_key_file_is_unreadable() {
    let fixture = Fixture::start(Script::Echo);
    let mut spawn = fixture.spawn();
    spawn.identity_file = Some(fixture.key_file.with_extension("missing"));
    let channel = fixture.open(spawn);

    assert_eq!(wait_end(&channel), End::Failed(FailKind::KeyUnreadable));
}

#[test]
fn an_absent_agent_is_skipped_and_the_next_method_authenticates() {
    let fixture = Fixture::start(Script::Echo);
    let mut spawn = fixture.spawn();
    // Agent first (RF-20.9). Whatever agent this machine has -- or none -- the
    // server only authorizes the key file, so the key is what gets us in.
    spawn.auth = vec![AuthMethod::Agent, AuthMethod::Key];
    let channel = fixture.open(spawn);

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
    let channel = fixture.open(spawn);

    assert_eq!(wait_end(&channel), End::Failed(FailKind::AuthRejected));
    assert_eq!(fixture.log.key_offers.load(Ordering::SeqCst), 0);
}

#[test]
fn sharing_a_connection_is_not_implemented_yet() {
    let mut spawn = SshSpawn::new("127.0.0.1", 22, "tester");
    spawn.connection = Connection::Share(ConnectionId::next());

    let result = SshChannel::open_with(
        spawn,
        Arc::new(NeverAsked),
        KnownHostsFiles {
            user: None,
            read_only: Vec::new(),
        },
    );

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

// ---------------------------------------------------------------------------
// Chave do host (prompt 10, ADR-0074)
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_host_asks_and_confirm_connects_and_remembers() {
    let fixture = Fixture::start_unregistered(Script::Echo, true);
    let user = ScriptedUser::new(Reply::Confirm);
    let channel = fixture.open_asking(fixture.spawn(), user.clone());

    channel.writer().write_all(b"hello\n").unwrap();
    assert!(read_until(&channel, "hello").contains("hello"));

    let asked = user.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].host, "127.0.0.1");
    assert_eq!(asked[0].port, fixture.port);
    assert_eq!(asked[0].algorithm, "ssh-ed25519");
    assert_eq!(
        asked[0].fingerprint,
        fixture
            .server_key
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string()
    );
    assert!(asked[0].other_types.is_empty());
    assert_eq!(asked[0].skipped_lines, 0);

    // The file did not exist: it is created with the one accepted line.
    let text = fixture.known_hosts_text().expect("known_hosts written");
    assert_eq!(
        text,
        format!(
            "[127.0.0.1]:{} {}\n",
            fixture.port,
            fixture.server_key.to_openssh().unwrap()
        )
    );
}

#[test]
fn the_second_connection_does_not_ask() {
    let fixture = Fixture::start_unregistered(Script::Echo, true);
    let first = fixture.open_asking(fixture.spawn(), ScriptedUser::new(Reply::Confirm));
    first.writer().write_all(b"one\n").unwrap();
    assert!(read_until(&first, "one").contains("one"));
    first.close();

    // `NeverAsked` panics if the host key is not recognized this time.
    let second = fixture.open(fixture.spawn());
    second.writer().write_all(b"two\n").unwrap();

    assert!(read_until(&second, "two").contains("two"));
}

#[test]
fn cancel_ends_with_rejected_by_the_user_and_writes_nothing() {
    let fixture = Fixture::start_unregistered(Script::Echo, true);
    let user = ScriptedUser::new(Reply::Cancel);
    let channel = fixture.open_asking(fixture.spawn(), user.clone());

    assert_eq!(
        wait_end(&channel),
        End::Failed(FailKind::HostKeyRejectedByUser)
    );
    assert_eq!(user.asked().len(), 1);
    assert_eq!(fixture.known_hosts_text(), None);
    // The key was never offered: authentication happens after the host key.
    assert_eq!(fixture.log.key_offers.load(Ordering::SeqCst), 0);
}

#[test]
fn a_changed_host_key_blocks_without_asking_and_leaves_the_file_alone() {
    let fixture = Fixture::start_unregistered(Script::Echo, true);
    // The file says this host has a *different* ed25519 key: the server's key
    // "changed".
    let old_key = new_key().public_key().clone();
    fixture.register_server_key(&old_key);
    let before = fixture.known_hosts_text().unwrap();

    let channel = fixture.open(fixture.spawn());

    match wait_end(&channel) {
        End::Failed(FailKind::HostKeyChanged {
            presented,
            expected,
        }) => {
            assert_eq!(presented.algorithm, "ssh-ed25519");
            assert_eq!(
                presented.fingerprint,
                fixture
                    .server_key
                    .fingerprint(russh::keys::HashAlg::Sha256)
                    .to_string()
            );
            assert_eq!(expected.len(), 1);
            assert_eq!(expected[0].file, fixture.known_hosts);
            assert_eq!(expected[0].line, 1);
            assert_eq!(
                expected[0].fingerprint,
                old_key
                    .fingerprint(russh::keys::HashAlg::Sha256)
                    .to_string()
            );
        }
        other => panic!("expected HostKeyChanged, got {other:?}"),
    }
    assert_eq!(fixture.known_hosts_text().unwrap(), before);
    assert_eq!(fixture.log.key_offers.load(Ordering::SeqCst), 0);
}

#[test]
fn a_revoked_host_key_blocks_and_leaves_the_file_alone() {
    let fixture = Fixture::start_unregistered(Script::Echo, true);
    std::fs::create_dir_all(fixture.known_hosts.parent().unwrap()).unwrap();
    std::fs::write(
        &fixture.known_hosts,
        format!(
            "@revoked [127.0.0.1]:{} {}\n",
            fixture.port,
            fixture.server_key.to_openssh().unwrap()
        ),
    )
    .unwrap();
    let before = fixture.known_hosts_text().unwrap();

    let channel = fixture.open(fixture.spawn());

    match wait_end(&channel) {
        End::Failed(FailKind::HostKeyRevoked { entry, .. }) => {
            assert_eq!(entry.line, 1);
            assert_eq!(entry.file, fixture.known_hosts);
        }
        other => panic!("expected HostKeyRevoked, got {other:?}"),
    }
    assert_eq!(fixture.known_hosts_text().unwrap(), before);
}

#[test]
fn an_unreadable_known_hosts_ends_without_asking_and_without_writing() {
    let fixture = Fixture::start_unregistered(Script::Echo, true);
    // UTF-16 with a BOM, as PowerShell 5 rewrites a file with `>`.
    std::fs::create_dir_all(fixture.known_hosts.parent().unwrap()).unwrap();
    let mut utf16 = vec![0xFF, 0xFE];
    for unit in "example.org ssh-ed25519 AAAA\n".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(&fixture.known_hosts, &utf16).unwrap();

    // `NeverAsked`: it must not even get as far as asking.
    let channel = fixture.open(fixture.spawn());

    assert_eq!(
        wait_end(&channel),
        End::Failed(FailKind::KnownHostsUnreadable {
            file: fixture.known_hosts.clone(),
            cause: ReadCause::NotText,
        })
    );
    assert_eq!(std::fs::read(&fixture.known_hosts).unwrap(), utf16);
}

#[test]
fn the_question_carries_the_types_already_registered_and_the_skipped_lines() {
    let fixture = Fixture::start_unregistered(Script::Echo, true);
    // The host already has an ECDSA key on file (another type), plus a line the
    // reader cannot understand.
    std::fs::create_dir_all(fixture.known_hosts.parent().unwrap()).unwrap();
    std::fs::write(
        &fixture.known_hosts,
        format!(
            "[127.0.0.1]:{port} ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBG/8AXfpnvyGve1W7+b287FqZXPL9t6MdX5HAU5N92+EjD2XSw/nhOrKlJFfbyj2VJXM+jBNlBwh1qWVK/QwvK0=\nthis is not a key\n",
            port = fixture.port
        ),
    )
    .unwrap();
    let user = ScriptedUser::new(Reply::Cancel);
    let channel = fixture.open_asking(fixture.spawn(), user.clone());

    assert_eq!(
        wait_end(&channel),
        End::Failed(FailKind::HostKeyRejectedByUser)
    );
    let asked = user.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].other_types, vec!["ecdsa-sha2-nistp256".to_owned()]);
    assert_eq!(asked[0].skipped_lines, 1);
}

#[test]
fn time_spent_on_the_host_key_question_does_not_count_against_the_connect_timeout() {
    /// A user who takes longer than the connect timeout to decide.
    struct SlowUser;
    impl Asker for SlowUser {
        fn ask(&self, _question: Question, on_reply: Box<dyn FnOnce(Reply) + Send>) {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(900));
                on_reply(Reply::Confirm);
            });
        }
    }

    let fixture = Fixture::start_unregistered(Script::Echo, true);
    let mut spawn = fixture.spawn();
    spawn.connect_timeout = Duration::from_millis(400);
    let channel = fixture.open_asking(spawn, Arc::new(SlowUser));

    channel.writer().write_all(b"patient\n").unwrap();

    assert!(read_until(&channel, "patient").contains("patient"));
    assert_eq!(channel.try_end(), None);
}
