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

use crate::ask::{Asker, HostKeyQuestion, Notice, Question, Reply, SecretQuestion, SecretTarget};
use crate::end::{End, FailKind, LostKind};
use crate::known_hosts::{KnownHostsFiles, ReadCause};
use crate::secret::Secret;
use crate::spawn::{AuthMethod, Connection, ConnectionId, OpenError, SshSpawn};
use crate::vault::fake::MemoryVault;
use crate::vault::{Destination, Vault, passphrase_account};
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

/// O keyboard-interactive que o servidor de mentira oferece.
#[derive(Clone, Copy)]
enum Kbd {
    /// Um campo oculto ("Password: "); aceita esta senha.
    OneHidden(&'static str),
    /// Dois campos ocultos.
    TwoFields,
    /// Um campo que ecoa.
    OneVisible,
    /// A senha certa abre um segundo campo (um código): segundo fator.
    PasswordThenCode(&'static str),
}

/// Como o servidor de mentira trata senha e keyboard-interactive. O default só
/// oferece chave pública.
#[derive(Clone, Copy, Default)]
struct ServerAuth {
    /// A senha que `password` aceita. `None`: o método nem é oferecido.
    password: Option<&'static str>,
    kbd: Option<Kbd>,
    /// Tentar `password` derruba a conexão (`fail2ban`, `MaxAuthTries`).
    drop_on_password: bool,
    /// Recusar uma senha errada **retirando** `password` da lista.
    withdraw_password_on_failure: bool,
}

impl ServerAuth {
    fn password(secret: &'static str) -> Self {
        Self {
            password: Some(secret),
            ..Self::default()
        }
    }

    fn kbd(kbd: Kbd) -> Self {
        Self {
            kbd: Some(kbd),
            ..Self::default()
        }
    }

    fn methods(&self) -> russh::MethodSet {
        let mut methods = vec![russh::MethodKind::PublicKey];
        if self.password.is_some() || self.drop_on_password {
            methods.push(russh::MethodKind::Password);
        }
        if self.kbd.is_some() {
            methods.push(russh::MethodKind::KeyboardInteractive);
        }
        russh::MethodSet::from(&methods[..])
    }
}

#[derive(Default)]
struct ServerLog {
    /// `(colunas, linhas)` de cada `pty-req` e `window-change`, em ordem.
    sizes: Mutex<Vec<(u32, u32)>>,
    /// Quantas chaves públicas o cliente ofereceu.
    key_offers: AtomicUsize,
    /// Quantas vezes o cliente tentou `password`.
    password_tries: AtomicUsize,
    /// Quantas autenticações o servidor aceitou: uma por **conexão**.
    auths: AtomicUsize,
    /// Quantos shells o servidor abriu: um por **canal**.
    shells: AtomicUsize,
}

struct ShellServer {
    script: Script,
    authorized: PublicKey,
    auth: ServerAuth,
    log: Arc<ServerLog>,
}

impl server::Server for ShellServer {
    type Handler = Self;

    fn new_client(&mut self, _peer: Option<SocketAddr>) -> Self {
        Self {
            script: self.script,
            authorized: self.authorized.clone(),
            auth: self.auth,
            log: Arc::clone(&self.log),
        }
    }
}

impl server::Handler for ShellServer {
    type Error = russh::Error;

    async fn auth_succeeded(&mut self, _session: &mut Session) -> Result<(), Self::Error> {
        self.log.auths.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn auth_password(&mut self, _user: &str, password: &str) -> Result<Auth, Self::Error> {
        self.log.password_tries.fetch_add(1, Ordering::SeqCst);
        if self.auth.drop_on_password {
            // A conexão cai no meio da autenticação, sem resposta.
            return Err(russh::Error::Disconnect);
        }
        if self.auth.password == Some(password) {
            return Ok(Auth::Accept);
        }
        // O `russh` tira `password` da lista quando se recusa com `reject()`;
        // o OpenSSH não: o método segue oferecido, e foi a senha que falhou.
        // O teste diz a lista de propósito.
        let proceed = if self.auth.withdraw_password_on_failure {
            russh::MethodSet::from(&[russh::MethodKind::PublicKey][..])
        } else {
            self.auth.methods()
        };
        Ok(Auth::Reject {
            proceed_with_methods: Some(proceed),
            partial_success: false,
        })
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        _user: &str,
        _submethods: &str,
        response: Option<server::Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        use std::borrow::Cow;
        let ask = |prompts: Vec<(&'static str, bool)>| Auth::Partial {
            name: Cow::Borrowed(""),
            instructions: Cow::Borrowed(""),
            prompts: prompts
                .into_iter()
                .map(|(text, echo)| (Cow::Borrowed(text), echo))
                .collect::<Vec<_>>()
                .into(),
        };
        let Some(kbd) = self.auth.kbd else {
            return Ok(Auth::reject());
        };
        let Some(mut response) = response else {
            return Ok(match kbd {
                Kbd::OneHidden(_) | Kbd::PasswordThenCode(_) => ask(vec![("Password: ", false)]),
                Kbd::TwoFields => ask(vec![("Password: ", false), ("Again: ", false)]),
                Kbd::OneVisible => ask(vec![("Login: ", true)]),
            });
        };
        let answer = response.next();
        Ok(match (kbd, answer) {
            (Kbd::OneHidden(good), Some(text)) if &text[..] == good.as_bytes() => Auth::Accept,
            (Kbd::PasswordThenCode(good), Some(text)) if &text[..] == good.as_bytes() => {
                ask(vec![("Verification code: ", false)])
            }
            _ => Auth::reject(),
        })
    }

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
        self.log.shells.fetch_add(1, Ordering::SeqCst);
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
    /// `Confirm` confirma; qualquer outra coisa cancela.
    confirm: bool,
    asked: Mutex<Vec<HostKeyQuestion>>,
}

impl ScriptedUser {
    fn new(reply: Reply) -> Arc<Self> {
        Arc::new(Self {
            confirm: matches!(reply, Reply::Confirm),
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<HostKeyQuestion> {
        self.asked.lock().unwrap().clone()
    }
}

impl Asker for ScriptedUser {
    fn ask(&self, question: Question, on_reply: Box<dyn FnOnce(Reply) + Send>) {
        let Question::HostKey(question) = question else {
            panic!("this user only answers the host key question");
        };
        self.asked.lock().unwrap().push(question);
        on_reply(if self.confirm {
            Reply::Confirm
        } else {
            Reply::Cancel
        });
    }
}

/// Quem nunca deveria ser perguntado.
struct NeverAsked;

impl Asker for NeverAsked {
    fn ask(&self, _question: Question, _on_reply: Box<dyn FnOnce(Reply) + Send>) {
        panic!("this test must not ask anything");
    }
}

/// O usuário que digita segredos: responde na ordem e guarda o que viu. A
/// chave do host, quando perguntada, ele confirma.
#[derive(Default)]
struct SecretUser {
    replies: Mutex<std::collections::VecDeque<(&'static str, bool)>>,
    asked: Mutex<Vec<SecretQuestion>>,
    notices: Mutex<Vec<Notice>>,
}

impl SecretUser {
    /// Cada resposta é `(texto, lembrar)`; acabando, o usuário cancela.
    fn typing(replies: &[(&'static str, bool)]) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.iter().copied().collect()),
            ..Self::default()
        })
    }

    fn asked(&self) -> Vec<SecretQuestion> {
        self.asked.lock().unwrap().clone()
    }

    fn notices(&self) -> Vec<Notice> {
        self.notices.lock().unwrap().clone()
    }
}

impl Asker for SecretUser {
    fn ask(&self, question: Question, on_reply: Box<dyn FnOnce(Reply) + Send>) {
        match question {
            Question::HostKey(_) => on_reply(Reply::Confirm),
            Question::Secret(question) => {
                self.asked.lock().unwrap().push(question);
                let next = self.replies.lock().unwrap().pop_front();
                on_reply(match next {
                    Some((text, remember)) => Reply::Secret {
                        value: Secret::new(text.to_owned()),
                        remember,
                    },
                    None => Reply::Cancel,
                });
            }
        }
    }

    fn notice(&self, notice: Notice) {
        self.notices.lock().unwrap().push(notice);
    }
}

struct Fixture {
    /// Dono do servidor (e do proxy): precisa viver até o fim do teste.
    rt: Runtime,
    port: u16,
    log: Arc<ServerLog>,
    dir: PathBuf,
    client_key: PrivateKey,
    key_file: PathBuf,
    known_hosts: PathBuf,
    server_key: PublicKey,
    server: JoinHandle<()>,
    /// O que `restart` precisa para subir o mesmo servidor de novo.
    host_key: PrivateKey,
    authorized: PublicKey,
    auth: ServerAuth,
    script: Script,
}

/// Sobe o servidor de mentira em `listener`; vive até a tarefa ser abortada ou o
/// runtime cair.
fn spawn_server(
    rt: &Runtime,
    listener: TcpListener,
    host_key: PrivateKey,
    authorized: PublicKey,
    auth: ServerAuth,
    script: Script,
    log: Arc<ServerLog>,
) -> JoinHandle<()> {
    let config = Arc::new(server::Config {
        keys: vec![host_key],
        auth_rejection_time: Duration::from_millis(1),
        auth_rejection_time_initial: Some(Duration::ZERO),
        methods: auth.methods(),
        ..server::Config::default()
    });
    let mut srv = ShellServer {
        script,
        authorized,
        auth,
        log,
    };
    rt.spawn(async move {
        // `run_on_socket` aceita conexões até a tarefa ser abortada.
        let _ = srv.run_on_socket(config, &listener).await;
    })
}

impl Fixture {
    /// Derruba o servidor **e todas as conexões dele**, como uma queda de rede
    /// ou um servidor que reinicia, e o sobe de novo **na mesma porta**.
    /// `new_host_key` troca a chave do host entre a queda e a volta.
    fn restart(&mut self, new_host_key: bool) {
        self.server.abort();
        // Derrubar o runtime encerra as sessões e fecha os sockets.
        drop(std::mem::replace(&mut self.rt, Runtime::new().unwrap()));
        if new_host_key {
            self.host_key = new_key();
            self.server_key = self.host_key.public_key().clone();
        }
        let mut listener = None;
        for _ in 0..100 {
            match self
                .rt
                .block_on(TcpListener::bind(("127.0.0.1", self.port)))
            {
                Ok(bound) => {
                    listener = Some(bound);
                    break;
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        self.server = spawn_server(
            &self.rt,
            listener.expect("the port came back"),
            self.host_key.clone(),
            self.authorized.clone(),
            self.auth,
            self.script,
            Arc::clone(&self.log),
        );
    }

    /// Um servidor que autoriza a chave do cliente gerada aqui, **e cuja chave
    /// de host já está no `known_hosts`**.
    fn start(script: Script) -> Self {
        Self::start_auth(script, ServerAuth::default())
    }

    /// Como `start`, com senha e/ou keyboard-interactive.
    fn start_auth(script: Script, auth: ServerAuth) -> Self {
        let fixture = Self::start_with(script, true, auth);
        fixture.register_server_key(&fixture.server_key.clone());
        fixture
    }

    /// Como `start`, mas o `known_hosts` não existe.
    fn start_unregistered(script: Script, authorize_client_key: bool) -> Self {
        Self::start_with(script, authorize_client_key, ServerAuth::default())
    }

    fn start_with(script: Script, authorize_client_key: bool, auth: ServerAuth) -> Self {
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
        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = spawn_server(
            &rt,
            listener,
            host_key.clone(),
            authorized.clone(),
            auth,
            script,
            Arc::clone(&log),
        );

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
            client_key,
            key_file,
            server_key,
            server,
            host_key,
            authorized,
            auth,
            script,
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
        self.open_in(spawn, asker, Arc::new(MemoryVault::default()))
    }

    /// Como `open_asking`, com o cofre (em memória) que o teste escolheu.
    fn open_in(&self, spawn: SshSpawn, asker: Arc<dyn Asker>, vault: Arc<dyn Vault>) -> SshChannel {
        SshChannel::open_with(spawn, asker, self.files(), vault).unwrap()
    }

    /// Reescreve o arquivo da chave do cliente cifrado com `passphrase`.
    fn encrypt_key_file(&self, passphrase: &str) {
        let encrypted = self
            .client_key
            .encrypt(&mut TestRng::new(), passphrase)
            .unwrap();
        std::fs::write(
            &self.key_file,
            encrypted.to_openssh(LineEnding::LF).unwrap().as_bytes(),
        )
        .unwrap();
    }

    /// A conta do cofre da senha de `tester` neste servidor.
    fn password_account(&self) -> String {
        Destination::new("tester", "127.0.0.1", self.port).password_account()
    }

    /// Spawn que usa só a senha.
    fn password_spawn(&self) -> SshSpawn {
        let mut spawn = self.spawn();
        spawn.auth = vec![AuthMethod::Password];
        spawn.identity_file = None;
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
    SshChannel::open_with(
        spawn,
        Arc::new(NeverAsked),
        files,
        Arc::new(MemoryVault::default()),
    )
    .unwrap()
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
fn sharing_a_connection_that_does_not_exist_is_an_error() {
    let mut spawn = SshSpawn::new("127.0.0.1", 22, "tester");
    spawn.connection = Connection::Share(ConnectionId::next());

    let result = SshChannel::open_with(
        spawn,
        Arc::new(NeverAsked),
        KnownHostsFiles {
            user: None,
            read_only: Vec::new(),
        },
        Arc::new(MemoryVault::default()),
    );

    assert_eq!(result.err(), Some(OpenError::ConnectionGone));
}

// ---------------------------------------------------------------------------
// Divisão e reconexão (prompt 14, ADR-0072 §5, ADR-0076)
// ---------------------------------------------------------------------------

/// Espera o canal abrir o shell (o servidor o viu) e ecoar.
fn wait_established(channel: &SshChannel) {
    wait_until("the shell to open", || channel.is_established());
}

#[test]
fn a_second_channel_on_the_same_connection_does_not_authenticate_again() {
    let fixture = Fixture::start(Script::Echo);
    let first = fixture.open(fixture.spawn());
    connected_and_echoing(&first);
    assert_eq!(fixture.log.auths.load(Ordering::SeqCst), 1);

    let mut spawn = fixture.spawn();
    spawn.connection = Connection::Share(first.connection().unwrap());
    // `NeverAsked`: no question, no password, no host key.
    let second = fixture.open(spawn);
    connected_and_echoing(&second);

    assert_eq!(
        fixture.log.auths.load(Ordering::SeqCst),
        1,
        "one authentication for two channels"
    );
    assert_eq!(fixture.log.shells.load(Ordering::SeqCst), 2);
    assert_eq!(second.connection(), first.connection());
}

#[test]
fn a_shared_channel_asks_nothing_even_when_the_first_one_needed_a_password() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password(PROOF));
    let first = fixture.open_asking(
        fixture.password_spawn(),
        SecretUser::typing(&[(PROOF, false)]),
    );
    connected_and_echoing(&first);

    let mut spawn = fixture.password_spawn();
    spawn.connection = Connection::Share(first.connection().unwrap());
    let second = fixture.open(spawn);

    connected_and_echoing(&second);
    assert_eq!(fixture.log.password_tries.load(Ordering::SeqCst), 1);
}

#[test]
fn a_new_tab_never_shares_a_connection() {
    let fixture = Fixture::start(Script::Echo);
    let first = fixture.open(fixture.spawn());
    connected_and_echoing(&first);

    let second = fixture.open(fixture.spawn());
    connected_and_echoing(&second);

    assert_ne!(first.connection(), second.connection());
    assert_eq!(fixture.log.auths.load(Ordering::SeqCst), 2);
}

#[test]
fn the_connection_closes_with_the_last_channel_and_not_before() {
    let fixture = Fixture::start(Script::Echo);
    let first = fixture.open(fixture.spawn());
    connected_and_echoing(&first);
    let id = first.connection().unwrap();
    let mut spawn = fixture.spawn();
    spawn.connection = Connection::Share(id);
    let second = fixture.open(spawn);
    connected_and_echoing(&second);

    first.close();
    std::thread::sleep(Duration::from_millis(300));
    // The second channel keeps the connection up: it did not end, and a third
    // channel can still join.
    assert_eq!(second.try_end(), None);
    let mut spawn = fixture.spawn();
    spawn.connection = Connection::Share(id);
    let third = fixture.open(spawn);
    connected_and_echoing(&third);
    third.close();

    second.close();
    wait_until("the connection to leave the registry", || {
        crate::task::lookup(id).is_none()
    });
    let mut spawn = fixture.spawn();
    spawn.connection = Connection::Share(id);
    let late = SshChannel::open_with(
        spawn,
        Arc::new(NeverAsked),
        fixture.files(),
        Arc::new(MemoryVault::default()),
    );
    assert_eq!(late.err(), Some(OpenError::ConnectionGone));
}

#[test]
fn a_dead_connection_refuses_a_shared_channel() {
    let mut fixture = Fixture::start(Script::Echo);
    let first = fixture.open(fixture.spawn());
    connected_and_echoing(&first);
    let id = first.connection().unwrap();

    fixture.restart(false);
    assert!(matches!(wait_end(&first), End::Lost(_)));

    let mut spawn = fixture.spawn();
    spawn.connection = Connection::Share(id);
    let result = SshChannel::open_with(
        spawn,
        Arc::new(NeverAsked),
        fixture.files(),
        Arc::new(MemoryVault::default()),
    );
    assert_eq!(result.err(), Some(OpenError::ConnectionGone));
}

#[test]
fn every_channel_of_a_connection_that_falls_is_lost() {
    let mut fixture = Fixture::start(Script::Echo);
    let first = fixture.open(fixture.spawn());
    connected_and_echoing(&first);
    let mut spawn = fixture.spawn();
    spawn.connection = Connection::Share(first.connection().unwrap());
    let second = fixture.open(spawn);
    connected_and_echoing(&second);

    fixture.restart(false);

    assert!(matches!(wait_end(&first), End::Lost(_)));
    assert!(matches!(wait_end(&second), End::Lost(_)));
}

#[test]
fn after_the_server_comes_back_a_new_connection_works_on_the_same_port() {
    let mut fixture = Fixture::start(Script::Echo);
    let before = fixture.open(fixture.spawn());
    connected_and_echoing(&before);

    fixture.restart(false);
    assert!(matches!(wait_end(&before), End::Lost(_)));

    // The known host key is the same: no question.
    let after = fixture.open(fixture.spawn());
    connected_and_echoing(&after);
    assert_ne!(after.connection(), before.connection());
}

#[test]
fn a_host_key_that_changed_between_the_drop_and_the_return_stops_at_once() {
    let mut fixture = Fixture::start(Script::Echo);
    let before = fixture.open(fixture.spawn());
    connected_and_echoing(&before);

    fixture.restart(true);
    assert!(matches!(wait_end(&before), End::Lost(_)));

    let after = fixture.open(fixture.spawn());
    assert!(
        matches!(
            wait_end(&after),
            End::Failed(FailKind::HostKeyChanged { .. })
        ),
        "the new key is not the registered one"
    );
}

#[test]
fn a_clean_exit_is_not_a_lost_connection() {
    let fixture = Fixture::start(Script::ExitStatus(0));
    let channel = fixture.open(fixture.spawn());

    assert!(matches!(
        wait_end(&channel),
        End::RemoteEnded { code: Some(0), .. }
    ));
}

#[test]
fn what_is_typed_before_a_reconnect_attempt_opens_is_discarded() {
    let fixture = Fixture::start(Script::Echo);
    let mut spawn = fixture.spawn();
    spawn.discard_input_until_open = true;
    let channel = fixture.open(spawn);
    // Typed while the connection is still being made.
    channel.writer().write_all(b"typed-too-early\n").unwrap();
    wait_established(&channel);
    std::thread::sleep(Duration::from_millis(200));

    channel.writer().write_all(b"typed-in-time\n").unwrap();
    let seen = read_until(&channel, "typed-in-time");

    assert!(seen.contains("typed-in-time"));
    assert!(
        !seen.contains("typed-too-early"),
        "the old shell's input never reaches the new one: {seen:?}"
    );
}

#[test]
fn without_the_flag_input_typed_while_connecting_is_still_delivered() {
    let fixture = Fixture::start(Script::Echo);
    let channel = fixture.open(fixture.spawn());
    channel.writer().write_all(b"early\n").unwrap();

    assert!(read_until(&channel, "early").contains("early"));
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

// ---------------------------------------------------------------------------
// Senha, passphrase e keyboard-interactive (prompt 13, ADR-0073, ADR-0075 §2)
// ---------------------------------------------------------------------------

/// O segredo de prova: nenhuma nota, erro ou pergunta pode contê-lo.
const PROOF: &str = "tr0ub4dor-proof-9f31";

fn connected_and_echoing(channel: &SshChannel) {
    channel.writer().write_all(b"hello\n").unwrap();
    assert!(read_until(channel, "hello").contains("hello"));
}

#[test]
fn a_password_typed_in_the_dialog_connects() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password(PROOF));
    let user = SecretUser::typing(&[(PROOF, false)]);

    let channel = fixture.open_asking(fixture.password_spawn(), user.clone());

    connected_and_echoing(&channel);
    let asked = user.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].target,
        SecretTarget::Password {
            user: "tester".to_owned(),
            host: "127.0.0.1".to_owned(),
        }
    );
    assert!(!asked[0].retry);
    assert!(asked[0].vault_available);
}

#[test]
fn a_wrong_password_reopens_the_dialog_with_the_error_line() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password(PROOF));
    let user = SecretUser::typing(&[("not-it", false), (PROOF, false)]);

    let channel = fixture.open_asking(fixture.password_spawn(), user.clone());

    connected_and_echoing(&channel);
    let retries: Vec<bool> = user.asked().iter().map(|q| q.retry).collect();
    assert_eq!(retries, [false, true]);
    assert_eq!(fixture.log.password_tries.load(Ordering::SeqCst), 2);
}

#[test]
fn cancelling_the_secret_dialog_ends_with_authentication_cancelled() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password(PROOF));
    let user = SecretUser::typing(&[]);

    let channel = fixture.open_asking(fixture.password_spawn(), user.clone());

    assert_eq!(wait_end(&channel), End::Failed(FailKind::AuthCancelled));
    assert_eq!(fixture.log.password_tries.load(Ordering::SeqCst), 0);
}

#[test]
fn a_server_that_only_offers_one_hidden_field_is_served_as_a_password() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::kbd(Kbd::OneHidden(PROOF)));
    let user = SecretUser::typing(&[(PROOF, false)]);

    let channel = fixture.open_asking(fixture.password_spawn(), user.clone());

    connected_and_echoing(&channel);
    // The dialog is the app's: the same question as for a plain password. The
    // server's "Password: " text has nowhere to travel.
    let asked = user.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].target,
        SecretTarget::Password {
            user: "tester".to_owned(),
            host: "127.0.0.1".to_owned(),
        }
    );
}

#[test]
fn a_wrong_keyboard_interactive_password_asks_again() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::kbd(Kbd::OneHidden(PROOF)));
    let user = SecretUser::typing(&[("not-it", false), (PROOF, false)]);

    let channel = fixture.open_asking(fixture.password_spawn(), user.clone());

    connected_and_echoing(&channel);
    let retries: Vec<bool> = user.asked().iter().map(|q| q.retry).collect();
    assert_eq!(retries, [false, true]);
}

#[test]
fn keyboard_interactive_with_two_fields_fails_without_asking_anything() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::kbd(Kbd::TwoFields));

    // `NeverAsked`: the user is not asked for a password the server will not
    // take on its own.
    let channel = fixture.open(fixture.password_spawn());

    assert_eq!(wait_end(&channel), End::Failed(FailKind::AuthUnsupported));
}

#[test]
fn keyboard_interactive_with_a_visible_field_fails_without_asking_anything() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::kbd(Kbd::OneVisible));

    let channel = fixture.open(fixture.password_spawn());

    assert_eq!(wait_end(&channel), End::Failed(FailKind::AuthUnsupported));
}

#[test]
fn a_second_factor_after_the_password_fails_and_keeps_the_saved_password() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::kbd(Kbd::PasswordThenCode(PROOF)));
    let vault = Arc::new(MemoryVault::with(&fixture.password_account(), PROOF));

    let channel = fixture.open_in(
        fixture.password_spawn(),
        Arc::new(NeverAsked),
        vault.clone(),
    );

    assert_eq!(wait_end(&channel), End::Failed(FailKind::AuthUnsupported));
    // The password was right: nothing is deleted for a server that wants more.
    assert!(vault.deleted.lock().unwrap().is_empty());
    assert_eq!(
        vault.stored(&fixture.password_account()).as_deref(),
        Some(PROOF)
    );
}

#[test]
fn an_encrypted_key_asks_for_its_passphrase_and_connects() {
    let fixture = Fixture::start(Script::Echo);
    fixture.encrypt_key_file(PROOF);
    let user = SecretUser::typing(&[(PROOF, false)]);

    let channel = fixture.open_asking(fixture.spawn(), user.clone());

    connected_and_echoing(&channel);
    let asked = user.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].target,
        SecretTarget::KeyPassphrase {
            path: fixture.key_file.clone(),
        }
    );
}

#[test]
fn a_wrong_passphrase_asks_again_with_the_error_line() {
    let fixture = Fixture::start(Script::Echo);
    fixture.encrypt_key_file(PROOF);
    let user = SecretUser::typing(&[("not-it", false), (PROOF, false)]);

    let channel = fixture.open_asking(fixture.spawn(), user.clone());

    connected_and_echoing(&channel);
    let retries: Vec<bool> = user.asked().iter().map(|q| q.retry).collect();
    assert_eq!(retries, [false, true]);
}

#[test]
fn a_remembered_passphrase_is_kept_only_after_the_server_accepts_the_key() {
    let fixture = Fixture::start(Script::Echo);
    fixture.encrypt_key_file(PROOF);
    let vault = Arc::new(MemoryVault::default());
    let user = SecretUser::typing(&[("not-it", true), (PROOF, true)]);

    let channel = fixture.open_in(fixture.spawn(), user.clone(), vault.clone());

    connected_and_echoing(&channel);
    let account = passphrase_account(&fixture.key_file);
    // The wrong one was never written: a single write, of the right one.
    assert_eq!(*vault.written.lock().unwrap(), [account.clone()]);
    assert_eq!(vault.stored(&account).as_deref(), Some(PROOF));
}

#[test]
fn a_saved_passphrase_is_used_without_asking() {
    let fixture = Fixture::start(Script::Echo);
    fixture.encrypt_key_file(PROOF);
    let vault = Arc::new(MemoryVault::with(
        &passphrase_account(&fixture.key_file),
        PROOF,
    ));

    let channel = fixture.open_in(fixture.spawn(), Arc::new(NeverAsked), vault);

    connected_and_echoing(&channel);
}

#[test]
fn a_stale_saved_passphrase_is_deleted_and_the_dialog_opens() {
    let fixture = Fixture::start(Script::Echo);
    fixture.encrypt_key_file(PROOF);
    let account = passphrase_account(&fixture.key_file);
    let vault = Arc::new(MemoryVault::with(&account, "old-passphrase"));
    let user = SecretUser::typing(&[(PROOF, false)]);

    let channel = fixture.open_in(fixture.spawn(), user.clone(), vault.clone());

    connected_and_echoing(&channel);
    assert_eq!(*vault.deleted.lock().unwrap(), [account]);
    assert!(user.asked()[0].retry);
}

#[test]
fn a_right_passphrase_for_a_key_the_server_does_not_know_is_not_saved() {
    // The server authorizes another key: the passphrase decrypts, nobody accepts.
    let fixture = Fixture::start_with(Script::Echo, false, ServerAuth::default());
    fixture.register_server_key(&fixture.server_key.clone());
    fixture.encrypt_key_file(PROOF);
    let vault = Arc::new(MemoryVault::default());
    let user = SecretUser::typing(&[(PROOF, true)]);

    let channel = fixture.open_in(fixture.spawn(), user, vault.clone());

    assert_eq!(wait_end(&channel), End::Failed(FailKind::AuthRejected));
    assert!(vault.is_empty());
}

#[test]
fn a_remembered_password_is_written_only_after_the_server_accepts() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password(PROOF));
    let vault = Arc::new(MemoryVault::default());
    let user = SecretUser::typing(&[("not-it", true), (PROOF, true)]);

    let channel = fixture.open_in(fixture.password_spawn(), user.clone(), vault.clone());

    connected_and_echoing(&channel);
    let account = fixture.password_account();
    assert_eq!(*vault.written.lock().unwrap(), [account.clone()]);
    assert_eq!(vault.stored(&account).as_deref(), Some(PROOF));
    assert!(user.notices().is_empty());
}

#[test]
fn a_wrong_password_with_remember_writes_nothing() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password(PROOF));
    let vault = Arc::new(MemoryVault::default());
    let user = SecretUser::typing(&[("not-it", true)]);

    let channel = fixture.open_in(fixture.password_spawn(), user, vault.clone());

    // The user gave up after the refusal.
    assert_eq!(wait_end(&channel), End::Failed(FailKind::AuthCancelled));
    assert!(vault.is_empty());
    assert!(vault.written.lock().unwrap().is_empty());
}

#[test]
fn the_next_connection_uses_the_remembered_password_without_asking() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password(PROOF));
    let vault = Arc::new(MemoryVault::default());
    let first = fixture.open_in(
        fixture.password_spawn(),
        SecretUser::typing(&[(PROOF, true)]),
        vault.clone(),
    );
    connected_and_echoing(&first);
    first.close();

    let second = fixture.open_in(fixture.password_spawn(), Arc::new(NeverAsked), vault);

    connected_and_echoing(&second);
}

#[test]
fn a_saved_password_the_server_refuses_is_deleted_and_asked_again() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password(PROOF));
    let account = fixture.password_account();
    let vault = Arc::new(MemoryVault::with(&account, "changed-on-the-server"));
    let user = SecretUser::typing(&[(PROOF, false)]);

    let channel = fixture.open_in(fixture.password_spawn(), user.clone(), vault.clone());

    connected_and_echoing(&channel);
    assert_eq!(*vault.deleted.lock().unwrap(), [account]);
    assert!(vault.is_empty());
    assert!(user.asked()[0].retry);
}

#[test]
fn a_connection_dropped_mid_authentication_keeps_the_saved_password() {
    let fixture = Fixture::start_auth(
        Script::Echo,
        ServerAuth {
            drop_on_password: true,
            ..ServerAuth::default()
        },
    );
    let account = fixture.password_account();
    let vault = Arc::new(MemoryVault::with(&account, PROOF));

    let channel = fixture.open_in(
        fixture.password_spawn(),
        Arc::new(NeverAsked),
        vault.clone(),
    );

    assert_eq!(wait_end(&channel), End::Failed(FailKind::Refused));
    assert!(vault.deleted.lock().unwrap().is_empty());
    assert_eq!(vault.stored(&account).as_deref(), Some(PROOF));
}

#[test]
fn a_method_the_server_withdraws_keeps_the_saved_password() {
    let fixture = Fixture::start_auth(
        Script::Echo,
        ServerAuth {
            password: Some("something-else"),
            withdraw_password_on_failure: true,
            ..ServerAuth::default()
        },
    );
    let account = fixture.password_account();
    let vault = Arc::new(MemoryVault::with(&account, PROOF));

    let channel = fixture.open_in(
        fixture.password_spawn(),
        Arc::new(NeverAsked),
        vault.clone(),
    );

    // `password` left the list: the secret was not what failed.
    assert_eq!(wait_end(&channel), End::Failed(FailKind::AuthRejected));
    assert!(vault.deleted.lock().unwrap().is_empty());
    assert_eq!(vault.stored(&account).as_deref(), Some(PROOF));
}

#[test]
fn without_a_vault_the_dialog_says_so_and_nothing_is_written() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password(PROOF));
    let vault = Arc::new(MemoryVault::unavailable());
    // Even a ticked box writes nothing when there is no vault.
    let user = SecretUser::typing(&[(PROOF, true)]);

    let channel = fixture.open_in(fixture.password_spawn(), user.clone(), vault.clone());

    connected_and_echoing(&channel);
    assert!(!user.asked()[0].vault_available);
    assert!(vault.written.lock().unwrap().is_empty());
}

#[test]
fn a_vault_that_refuses_the_write_only_raises_a_notice() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password(PROOF));
    let vault = Arc::new(MemoryVault::default());
    *vault.write_error.lock().unwrap() = Some(crate::vault::VaultError::Denied);
    let user = SecretUser::typing(&[(PROOF, true)]);

    let channel = fixture.open_in(fixture.password_spawn(), user.clone(), vault);

    // The connection was accepted and stays up.
    connected_and_echoing(&channel);
    assert_eq!(user.notices(), [Notice::SecretNotSaved]);
}

#[test]
fn the_default_order_still_tries_the_key_before_the_password() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password(PROOF));
    let mut spawn = fixture.spawn();
    spawn.auth = vec![AuthMethod::Key, AuthMethod::Password];

    // The key works: the password dialog never opens.
    let channel = fixture.open(spawn);

    connected_and_echoing(&channel);
    assert_eq!(fixture.log.password_tries.load(Ordering::SeqCst), 0);
}

// ---------------------------------------------------------------------------
// Varredura de vazamento (RF-20.34, ADR-0073 §5)
// ---------------------------------------------------------------------------

#[test]
fn no_error_variant_can_carry_a_secret() {
    // Every `FailKind` and `End` the crate can produce, formatted: the proof
    // secret is nowhere, because none of them has a field that could hold it.
    let kinds = [
        FailKind::Refused,
        FailKind::Timeout,
        FailKind::AuthRejected,
        FailKind::AuthCancelled,
        FailKind::AuthUnsupported,
        FailKind::KeyUnreadable,
        FailKind::HostKeyRejectedByUser,
        FailKind::ChannelRefused,
        FailKind::Runtime,
        FailKind::KnownHostsUnreadable {
            file: PathBuf::from("known_hosts"),
            cause: ReadCause::NotText,
        },
    ];
    for kind in kinds {
        let end = End::Failed(kind.clone());
        for text in [format!("{kind:?}"), format!("{end:?}"), format!("{end:#?}")] {
            assert!(!text.contains(PROOF), "{text}");
        }
    }
    for error in [
        crate::vault::VaultError::Unavailable,
        crate::vault::VaultError::Denied,
        crate::vault::VaultError::Failed,
    ] {
        assert!(!format!("{error:?} {error}").contains(PROOF));
    }
    assert!(!format!("{:?}", OpenError::ConnectionGone).contains(PROOF));
}

#[test]
fn a_secret_that_crosses_the_crate_never_shows_in_a_reply_or_question() {
    let reply = Reply::Secret {
        value: Secret::new(PROOF.to_owned()),
        remember: true,
    };
    assert!(!format!("{reply:?}").contains(PROOF));
    let question = SecretQuestion {
        target: SecretTarget::Password {
            user: "tester".to_owned(),
            host: "127.0.0.1".to_owned(),
        },
        vault_available: true,
        retry: true,
    };
    assert!(!format!("{question:?}").contains(PROOF));
}

#[test]
fn a_full_failed_connection_leaves_no_secret_in_what_the_channel_reports() {
    let fixture = Fixture::start_auth(Script::Echo, ServerAuth::password("another"));
    let user = SecretUser::typing(&[(PROOF, true)]);

    let channel = fixture.open_asking(fixture.password_spawn(), user.clone());

    let end = wait_end(&channel);
    assert!(!format!("{end:?}").contains(PROOF));
    for question in user.asked() {
        assert!(!format!("{question:?}").contains(PROOF));
    }
}
