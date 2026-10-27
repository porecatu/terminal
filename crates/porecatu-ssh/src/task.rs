// SPDX-License-Identifier: GPL-3.0-or-later

//! A tarefa de um canal: conecta, autentica, abre o pseudo-terminal e o
//! shell, e depois leva bytes nos dois sentidos até o fim (ADR-0072 §3 a §8,
//! ADR-0076 §1). Roda no runtime de `crate::runtime`, nunca na main thread.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, DisconnectReason, Handle, Handler, Msg};
use russh::keys::PublicKeyOrCertificate;
use russh::{Channel, ChannelMsg, ChannelReadHalf, Disconnect, Sig};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::auth;
use crate::channel::{Cmd, EndSlot};
use crate::end::{End, FailKind, LostKind};
use crate::spawn::SshSpawn;
use crate::verify::HostKeyVerifier;

/// ADR-0076 §1: um `keepalive@openssh.com` a cada 15 s sem tráfego, e a
/// conexão é dada como morta depois de três sem resposta. Não configurável.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
const KEEPALIVE_MAX: usize = 3;

/// Quanto esperar pelo motivo da queda quando o canal acaba sem `Close`: o
/// `disconnected` do `russh` pode chegar um instante depois.
const DISCONNECT_REASON_WAIT: Duration = Duration::from_millis(500);
const DISCONNECT_REASON_POLL: Duration = Duration::from_millis(25);

/// O que o `Handler` do `russh` anota para a tarefa ler depois.
#[derive(Default)]
struct Shared {
    disconnect: Mutex<Option<LostKind>>,
    /// Por que a chave do host foi recusada, se foi.
    host_key: Mutex<Option<FailKind>>,
}

impl Shared {
    fn disconnect(&self) -> Option<LostKind> {
        *self
            .disconnect
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn host_key(&self) -> Option<FailKind> {
        *self
            .host_key
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

struct Client {
    host: String,
    port: u16,
    verifier: Arc<dyn HostKeyVerifier>,
    shared: Arc<Shared>,
}

impl Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let verdict = match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => {
                self.verifier.verify(&self.host, self.port, key)
            }
            // Certificado de host: fora do escopo desta leva (RF-20.18).
            PublicKeyOrCertificate::Certificate(_) => Err(FailKind::HostKeyUnverified),
        };
        match verdict {
            Ok(()) => Ok(true),
            Err(kind) => {
                *self
                    .shared
                    .host_key
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(kind);
                Ok(false)
            }
        }
    }

    async fn disconnected(
        &mut self,
        reason: DisconnectReason<Self::Error>,
    ) -> Result<(), Self::Error> {
        let kind = match &reason {
            DisconnectReason::ReceivedDisconnect(_) => LostKind::ServerDisconnect,
            DisconnectReason::Error(russh::Error::KeepaliveTimeout) => LostKind::KeepaliveTimeout,
            DisconnectReason::Error(_) => LostKind::Transport,
        };
        *self
            .shared
            .disconnect
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(kind);
        match reason {
            DisconnectReason::ReceivedDisconnect(_) => Ok(()),
            DisconnectReason::Error(error) => Err(error),
        }
    }
}

fn client_config() -> client::Config {
    client::Config {
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        keepalive_max: KEEPALIVE_MAX,
        // Sessão ociosa por horas é uso normal (ADR-0076 §1).
        inactivity_timeout: None,
        ..client::Config::default()
    }
}

/// Entrada da tarefa. Quando devolve, `data_tx` cai e a leitura do outro lado
/// vê o fim de arquivo.
pub(crate) async fn run(
    spawn: SshSpawn,
    verifier: Arc<dyn HostKeyVerifier>,
    data_tx: mpsc::Sender<Vec<u8>>,
    cmd_tx: UnboundedSender<Cmd>,
    mut cmd_rx: UnboundedReceiver<Cmd>,
    end: EndSlot,
) {
    let shared = Arc::new(Shared::default());
    let opened = match open(&spawn, verifier, &shared, &data_tx).await {
        Ok(opened) => opened,
        Err(kind) => {
            end.set(End::Failed(kind));
            return;
        }
    };
    let Opened { handle, channel } = opened;
    let (read_half, write_half) = channel.split();

    tokio::spawn(read_loop(read_half, data_tx, shared, end, cmd_tx));

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            Cmd::Data(bytes) => {
                if write_half.data_bytes(bytes).await.is_err() {
                    break;
                }
            }
            Cmd::Resize { rows, cols } => {
                let _ = write_half
                    .window_change(u32::from(cols), u32::from(rows), 0, 0)
                    .await;
            }
            Cmd::Close => break,
        }
    }
    let _ = write_half.close().await;
    let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
}

struct Opened {
    handle: Handle<Client>,
    channel: Channel<Msg>,
}

/// Conecta (com o teto de 15 s até a troca de chaves), autentica e deixa o
/// shell pronto.
async fn open(
    spawn: &SshSpawn,
    verifier: Arc<dyn HostKeyVerifier>,
    shared: &Arc<Shared>,
    data_tx: &mpsc::Sender<Vec<u8>>,
) -> Result<Opened, FailKind> {
    let handler = Client {
        host: spawn.host.clone(),
        port: spawn.port,
        verifier,
        shared: Arc::clone(shared),
    };
    let connecting = client::connect(
        Arc::new(client_config()),
        (spawn.host.as_str(), spawn.port),
        handler,
    );
    let mut handle = match tokio::time::timeout(spawn.connect_timeout, connecting).await {
        Err(_) => return Err(FailKind::Timeout),
        // Recusa da chave do host aparece como erro de protocolo; a causa
        // verdadeira é a que o `Handler` anotou.
        Ok(Err(_)) => return Err(shared.host_key().unwrap_or(FailKind::Refused)),
        Ok(Ok(handle)) => handle,
    };

    auth::authenticate(&mut handle, spawn).await?;

    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|_| FailKind::ChannelRefused)?;
    channel
        .request_pty(
            true,
            &spawn.term,
            u32::from(spawn.cols),
            u32::from(spawn.rows),
            0,
            0,
            &[],
        )
        .await
        .map_err(|_| FailKind::ChannelRefused)?;
    await_reply(&mut channel, data_tx).await?;
    // Servidor que recusa `setenv` não é erro (ADR-0072 §5): sem pedir resposta.
    let _ = channel.set_env(false, "COLORTERM", "truecolor").await;
    channel
        .request_shell(true)
        .await
        .map_err(|_| FailKind::ChannelRefused)?;
    await_reply(&mut channel, data_tx).await?;

    Ok(Opened { handle, channel })
}

/// Espera o `success` ou o `failure` de um pedido de canal. Dados que cheguem
/// no meio seguem para o leitor.
async fn await_reply(
    channel: &mut Channel<Msg>,
    data_tx: &mpsc::Sender<Vec<u8>>,
) -> Result<(), FailKind> {
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Success) => return Ok(()),
            Some(ChannelMsg::Failure) | Some(ChannelMsg::Close) | None => {
                return Err(FailKind::ChannelRefused);
            }
            Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                let _ = data_tx.send(data.to_vec());
            }
            Some(_) => {}
        }
    }
}

/// Empurra o que o servidor manda e decide como o canal terminou (ADR-0076 §1).
async fn read_loop(
    mut half: ChannelReadHalf,
    data_tx: mpsc::Sender<Vec<u8>>,
    shared: Arc<Shared>,
    end: EndSlot,
    cmd_tx: UnboundedSender<Cmd>,
) {
    let mut code = None;
    let mut signal = None;
    let finished = loop {
        match half.wait().await {
            Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                let _ = data_tx.send(data.to_vec());
            }
            Some(ChannelMsg::ExitStatus { exit_status }) => code = Some(exit_status),
            Some(ChannelMsg::ExitSignal { signal_name, .. }) => {
                signal = Some(signal_label(&signal_name));
            }
            // `exit-*` seguido de EOF já é o fim; sem `exit-*`, espera o `Close`.
            Some(ChannelMsg::Eof) if code.is_some() || signal.is_some() => {
                break End::RemoteEnded { code, signal };
            }
            Some(ChannelMsg::Close) => break End::RemoteEnded { code, signal },
            Some(_) => {}
            // Canal sem `Close`: a conexão caiu por baixo dele.
            None => break End::Lost(lost_kind(&shared).await),
        }
    };
    end.set(finished);
    // Acorda a tarefa de escrita, que fecha e solta a conexão.
    let _ = cmd_tx.send(Cmd::Close);
}

/// Nome do sinal, sem o prefixo `SIG` (RFC 4254 §6.10). O nome de um sinal
/// fora da lista vem do servidor: é cortado, para não virar texto sem teto.
fn signal_label(signal: &Sig) -> String {
    const MAX_CUSTOM: usize = 32;
    match signal {
        Sig::ABRT => "ABRT".to_owned(),
        Sig::ALRM => "ALRM".to_owned(),
        Sig::FPE => "FPE".to_owned(),
        Sig::HUP => "HUP".to_owned(),
        Sig::ILL => "ILL".to_owned(),
        Sig::INT => "INT".to_owned(),
        Sig::KILL => "KILL".to_owned(),
        Sig::PIPE => "PIPE".to_owned(),
        Sig::QUIT => "QUIT".to_owned(),
        Sig::SEGV => "SEGV".to_owned(),
        Sig::TERM => "TERM".to_owned(),
        Sig::USR1 => "USR1".to_owned(),
        Sig::Custom(name) => name.chars().take(MAX_CUSTOM).collect(),
    }
}

async fn lost_kind(shared: &Shared) -> LostKind {
    let mut waited = Duration::ZERO;
    loop {
        if let Some(kind) = shared.disconnect() {
            return kind;
        }
        if waited >= DISCONNECT_REASON_WAIT {
            return LostKind::Transport;
        }
        tokio::time::sleep(DISCONNECT_REASON_POLL).await;
        waited += DISCONNECT_REASON_POLL;
    }
}
