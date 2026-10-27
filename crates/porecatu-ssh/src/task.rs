// SPDX-License-Identifier: GPL-3.0-or-later

//! A tarefa de um canal: conecta, autentica, abre o pseudo-terminal e o
//! shell, e depois leva bytes nos dois sentidos até o fim (ADR-0072 §3 a §8,
//! ADR-0076 §1). Roda no runtime de `crate::runtime`, nunca na main thread.

use std::borrow::Cow;
use std::future::Future;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::Preferred;
use russh::client::{self, DisconnectReason, Handle, Handler, Msg};
use russh::keys::{Algorithm, PublicKeyOrCertificate};
use russh::{Channel, ChannelMsg, ChannelReadHalf, Disconnect, Sig};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::auth::{self, Secrets};
use crate::channel::{Cmd, EndSlot};
use crate::end::{End, FailKind, LostKind};
use crate::known_hosts::{self, KnownHostsFiles};
use crate::spawn::SshSpawn;
use crate::vault::Vault;
use crate::verify::{HostKeyVerifier, Waiting};

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
        self.host_key
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

struct Client {
    verifier: Arc<HostKeyVerifier>,
    shared: Arc<Shared>,
}

impl Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let verdict = match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, hash_alg } => {
                self.verifier.verify(key, *hash_alg).await
            }
            // Certificado de host: fora do escopo (RF-20.18). Quem depende só
            // de `@cert-authority` cai no diálogo de host desconhecido -- e
            // aqui, sem chave comum para comparar, não há o que perguntar.
            PublicKeyOrCertificate::Certificate(_) => Err(FailKind::Refused),
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

fn client_config(registered: &[Algorithm]) -> client::Config {
    client::Config {
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        keepalive_max: KEEPALIVE_MAX,
        // Sessão ociosa por horas é uso normal (ADR-0076 §1).
        inactivity_timeout: None,
        preferred: Preferred {
            key: Cow::Owned(prefer_registered(&Preferred::DEFAULT.key, registered)),
            ..Preferred::DEFAULT
        },
        ..client::Config::default()
    }
}

/// ADR-0074 §3: os tipos de chave que o host já tem registrados vão primeiro,
/// para o servidor não apresentar outro e dar um falso "chave mudou". A ordem
/// de cada grupo é a de `default`. Todas as variantes de RSA contam como o
/// mesmo tipo.
fn prefer_registered(default: &[Algorithm], registered: &[Algorithm]) -> Vec<Algorithm> {
    let is_registered = |algorithm: &Algorithm| {
        registered.iter().any(|known| {
            known == algorithm
                || (matches!(known, Algorithm::Rsa { .. })
                    && matches!(algorithm, Algorithm::Rsa { .. }))
        })
    };
    let (first, rest): (Vec<Algorithm>, Vec<Algorithm>) = default
        .iter()
        .cloned()
        .partition(|algorithm| is_registered(algorithm));
    first.into_iter().chain(rest).collect()
}

/// Passo da espera do teto de conexão.
const BUDGET_TICK: Duration = Duration::from_millis(50);

/// Espera `future` por até `budget`, **sem contar** o tempo em que uma pergunta
/// espera o usuário (`waiting`): o teto de 15 s é para o servidor, não para
/// quem decide uma chave de host. `None` quando o orçamento acaba.
async fn within_budget<F: Future>(
    future: F,
    budget: Duration,
    waiting: &Waiting,
) -> Option<F::Output> {
    let mut future = std::pin::pin!(future);
    let mut left = budget;
    loop {
        let started = Instant::now();
        let tick = BUDGET_TICK.min(left.max(Duration::from_millis(1)));
        if let Ok(output) = tokio::time::timeout(tick, future.as_mut()).await {
            return Some(output);
        }
        if !waiting.is_waiting() {
            left = left.saturating_sub(started.elapsed());
            if left.is_zero() {
                return None;
            }
        }
    }
}

/// Entrada da tarefa. Quando devolve, `data_tx` cai e a leitura do outro lado
/// vê o fim de arquivo.
#[allow(
    clippy::too_many_arguments,
    reason = "one entry point that the channel feeds with everything the task owns"
)]
pub(crate) async fn run(
    spawn: SshSpawn,
    files: KnownHostsFiles,
    asker: Arc<dyn crate::ask::Asker>,
    vault: Arc<dyn Vault>,
    data_tx: mpsc::Sender<Vec<u8>>,
    cmd_tx: UnboundedSender<Cmd>,
    mut cmd_rx: UnboundedReceiver<Cmd>,
    end: EndSlot,
) {
    let shared = Arc::new(Shared::default());
    let opened = match open(&spawn, files, asker, vault, &shared, &data_tx).await {
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
    files: KnownHostsFiles,
    asker: Arc<dyn crate::ask::Asker>,
    vault: Arc<dyn Vault>,
    shared: &Arc<Shared>,
    data_tx: &mpsc::Sender<Vec<u8>>,
) -> Result<Opened, FailKind> {
    // Antes de conectar: um `known_hosts` ilegível encerra sem nem tocar a
    // rede (ADR-0074 §1), e os tipos já registrados decidem a preferência de
    // algoritmo (§3). A verificação lê o arquivo de novo, na hora de decidir.
    let read_files = files.clone();
    let texts = tokio::task::spawn_blocking(move || known_hosts::read_files(&read_files))
        .await
        .map_err(|_| FailKind::Runtime)?
        .map_err(|unreadable| FailKind::KnownHostsUnreadable {
            file: unreadable.file,
            cause: unreadable.cause,
        })?;
    let registered = known_hosts::registered_algorithms(&texts, &spawn.host, spawn.port);

    let waiting = Arc::new(Waiting::default());
    let secrets = Secrets {
        vault,
        asker: Arc::clone(&asker),
    };
    let verifier = Arc::new(HostKeyVerifier {
        files,
        asker,
        host: spawn.host.clone(),
        port: spawn.port,
        waiting: Arc::clone(&waiting),
    });
    let handler = Client {
        verifier,
        shared: Arc::clone(shared),
    };
    let connecting = client::connect(
        Arc::new(client_config(&registered)),
        (spawn.host.as_str(), spawn.port),
        handler,
    );
    let mut handle = match within_budget(connecting, spawn.connect_timeout, &waiting).await {
        None => return Err(FailKind::Timeout),
        // Recusa da chave do host aparece como erro de protocolo; a causa
        // verdadeira é a que o `Handler` anotou.
        Some(Err(_)) => return Err(shared.host_key().unwrap_or(FailKind::Refused)),
        Some(Ok(handle)) => handle,
    };

    auth::authenticate(&mut handle, spawn, &secrets).await?;

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

#[cfg(test)]
mod tests {
    use russh::keys::{EcdsaCurve, HashAlg};

    use super::*;

    fn default_order() -> Vec<Algorithm> {
        Preferred::DEFAULT.key.to_vec()
    }

    #[test]
    fn nothing_registered_keeps_the_default_order() {
        assert_eq!(prefer_registered(&default_order(), &[]), default_order());
    }

    #[test]
    fn a_registered_type_is_offered_first_and_the_rest_keep_their_order() {
        let ecdsa = Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP256,
        };
        assert!(default_order().contains(&ecdsa), "default offers ECDSA");

        let ordered = prefer_registered(&default_order(), std::slice::from_ref(&ecdsa));

        assert_eq!(ordered.len(), default_order().len());
        assert_eq!(ordered[0], ecdsa);
        let rest: Vec<Algorithm> = default_order()
            .into_iter()
            .filter(|algorithm| *algorithm != ecdsa)
            .collect();
        assert_eq!(ordered[1..], rest[..]);
    }

    #[test]
    fn every_rsa_variant_counts_as_the_registered_rsa_type() {
        let rsa_sha2 = Algorithm::Rsa {
            hash: Some(HashAlg::Sha512),
        };
        let registered = [Algorithm::Rsa { hash: None }];

        let ordered = prefer_registered(&default_order(), &registered);

        let rsa_count = default_order()
            .iter()
            .filter(|algorithm| matches!(algorithm, Algorithm::Rsa { .. }))
            .count();
        assert!(default_order().contains(&rsa_sha2));
        assert!(
            ordered[..rsa_count]
                .iter()
                .all(|algorithm| matches!(algorithm, Algorithm::Rsa { .. }))
        );
    }
}
