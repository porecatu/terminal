// SPDX-License-Identifier: GPL-3.0-or-later

//! O que alimenta um [`Terminal`](crate::Terminal): o trait interno
//! [`Backend`] e a implementação sobre PTY, [`PtyBackend`] (ADR-0071 §1).
//!
//! O trait é `pub(crate)`: quem está fora de `porecatu-term` nunca o vê nem o
//! implementa. O que atravessa a fronteira é [`BackendSpec`] (qual backend
//! subir), [`EndReason`] (por que terminou) e [`Liveness`] (o que a UI
//! consulta antes de fechar).

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use porecatu_pty::{ProcessGroup, PtyError, PtyHandle, SpawnConfig};
use porecatu_ssh::SshSpawn;

use crate::prompt::PromptSink;

/// Qual backend alimenta o terminal (ADR-0071 §2).
#[derive(Debug, Clone)]
pub enum BackendSpec {
    /// Lança um programa local num pseudo-terminal.
    Local(SpawnConfig),
    /// Abre um canal SSH com pseudo-terminal (ADR-0072). `Terminal::spawn`
    /// devolve na hora: conectar e autenticar acontecem fora da main thread.
    Ssh(SshSpawn),
    /// Só o motor VT, nenhum backend: a grade existe e aceita notas, nada
    /// roda. É o painel SSH restaurado que espera o primeiro gesto do
    /// usuário (RF-20.33); `Terminal::reattach` liga o backend de verdade.
    Dormant { rows: u16, cols: u16 },
}

impl BackendSpec {
    /// Tamanho inicial da grade, em linhas e colunas.
    pub(crate) fn grid_size(&self) -> (usize, usize) {
        match self {
            BackendSpec::Local(config) => (config.size.rows as usize, config.size.cols as usize),
            BackendSpec::Ssh(spawn) => (spawn.rows as usize, spawn.cols as usize),
            BackendSpec::Dormant { rows, cols } => (*rows as usize, *cols as usize),
        }
    }
}

/// Por que o terminal parou de ser alimentado (ADR-0071 §3). `Exited` vem do
/// PTY; as outras três, do backend SSH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndReason {
    /// Processo local saiu (PTY).
    Exited { success: bool, code: u32 },
    /// O servidor encerrou a sessão: exit-status, ou exit-signal sem código.
    RemoteEnded {
        code: Option<u32>,
        signal: Option<String>,
    },
    /// A conexão caiu sem o servidor encerrar a sessão (RF-20.27).
    Lost { cause: LostCause },
    /// Não chegou a abrir: recusa, autenticação, chave de host.
    Failed { cause: FailCause },
}

/// Causa de uma conexão perdida. Lista fechada pelo ADR-0071 §3; variante
/// nova pede revisão daquela seção.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LostCause {
    KeepaliveTimeout,
    Transport,
    ServerDisconnect,
}

/// Causa de uma conexão que nem chegou a abrir. Lista fechada pelo
/// ADR-0071 §3. As de chave de host carregam os dados da nota (ADR-0074 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailCause {
    Refused,
    Timeout,
    AuthRejected,
    AuthCancelled,
    AuthUnsupported,
    KeyUnreadable,
    HostKeyRejectedByUser,
    /// Há chave do mesmo tipo registrada e nenhuma casa (RF-20.17).
    HostKeyChanged {
        presented: HostKeyPresented,
        expected: Vec<HostKeyEntry>,
    },
    /// `@revoked` casa com a chave apresentada (RF-20.18).
    HostKeyRevoked {
        presented: HostKeyPresented,
        entry: HostKeyEntry,
    },
    /// Um `known_hosts` que existe e não pôde ser lido (ADR-0074 §1).
    KnownHostsUnreadable {
        file: PathBuf,
        cause: KnownHostsCause,
    },
    ChannelRefused,
    Runtime,
}

/// A chave que o servidor apresentou, para a nota de bloqueio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyPresented {
    pub algorithm: String,
    /// `SHA256:` e o hash em base64 sem preenchimento.
    pub fingerprint: String,
}

/// Uma entrada do `known_hosts`: onde está e qual chave guarda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyEntry {
    pub file: PathBuf,
    /// A partir de 1.
    pub line: usize,
    pub fingerprint: String,
}

/// Por que um `known_hosts` não pôde ser lido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownHostsCause {
    PermissionDenied,
    /// Não é texto UTF-8 (um arquivo regravado em UTF-16, por exemplo).
    NotText,
    Other,
}

/// Sinal de consulta da UI para a confirmação de fechamento (ADR-0034,
/// ADR-0071 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// PTY: só o shell vivo.
    Idle,
    /// PTY: mais de um processo na árvore (ADR-0034).
    ExtraProcesses,
    /// SSH: o canal deste painel está aberto ou abrindo. (O ciclo de
    /// reconexão, ADR-0076, é do prompt 14.)
    OpenConnection,
}

/// Como a thread de observação terminou (ADR-0071 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Finish {
    /// Encerramento pedido pelo usuário.
    Killed,
    /// O backend terminou sozinho; o motivo já saiu como evento.
    Ended,
}

/// Consultas que a UI faz **depois** de a thread de observação ter levado o
/// backend. Cópia leve e `Sync` do que o backend sabe responder.
pub(crate) trait Inspect: Send + Sync {
    fn liveness(&self) -> Liveness;
    /// RF-3.10/ADR-0038, segundo degrau da precedência de `cwd` na gravação
    /// de sessão.
    fn cwd_fallback(&self) -> Option<PathBuf>;
}

/// O que alimenta o motor com bytes e recebe os que ele escreve (ADR-0071 §1).
pub(crate) trait Backend: Send {
    /// Chamado uma vez, antes de `reader`: o canal por onde o backend pergunta
    /// à UI (ADR-0071 §5). O PTY nunca pergunta nada.
    fn set_prompts(&mut self, _prompts: PromptSink) {}
    fn reader(&mut self) -> io::Result<Box<dyn Read + Send>>;
    fn writer(&mut self) -> io::Result<Box<dyn Write + Send>>;
    fn resize(&mut self, rows: u16, cols: u16) -> io::Result<()>;
    /// Não bloqueia. `None` = ainda vivo.
    fn try_wait(&mut self) -> Option<EndReason>;
    /// Encerramento pedido pelo usuário.
    fn kill(&mut self);
    /// Último ato da thread de observação.
    fn finish(self: Box<Self>, how: Finish);
    /// Sinais de consulta da UI (ADR-0034, ADR-0038): `liveness` e
    /// `cwd_fallback`. Saem numa cópia à parte porque a thread de observação
    /// leva o backend, e a UI continua precisando perguntar. Ver [`Inspect`].
    fn inspector(&self) -> Arc<dyn Inspect>;
}

/// Backend sobre `porecatu-pty`. Guarda o que era específico do PTY em
/// `watch_loop`: o `PtyHandle`, o `ProcessGroup` e o porquê de nenhum dos
/// dois ser fechado de verdade.
pub(crate) struct PtyBackend {
    /// Nunca dropado de verdade -- ver [`PtyBackend::finish`].
    pty: PtyHandle,
    /// Cópia que decide, por caminho de saída, se dropa (mata a árvore) ou
    /// esquece (preserva processo destacado). A outra cópia, de consulta,
    /// está em [`PtyInspect`] (ADR-0034).
    group: Option<ProcessGroup>,
    reader: Option<Box<dyn Read + Send>>,
    writer: Option<Box<dyn Write + Send>>,
}

impl PtyBackend {
    /// Lança o processo e já tira do PTY o leitor e o escritor, para que uma
    /// falha deles continue chegando como [`PtyError`].
    pub(crate) fn spawn(config: SpawnConfig) -> Result<Self, PtyError> {
        let (pty, group) = porecatu_pty::spawn(config)?;
        let reader = pty.reader()?;
        let writer = pty.writer()?;
        Ok(Self {
            pty,
            group,
            reader: Some(reader),
            writer: Some(writer),
        })
    }
}

/// Cópia de consulta do `ProcessGroup` (ADR-0034): quem decide se droppa ou
/// esquece a outra cópia, matando ou não a árvore, é sempre `PtyBackend`.
struct PtyInspect {
    group: Option<ProcessGroup>,
}

impl Inspect for PtyInspect {
    /// `Idle` também quando o grupo não existe (sem `process_id()`, ou fora
    /// do Windows) -- lado seguro do erro é não confirmar por um sinal que
    /// não se conseguiu ler; o modo do terminal continua cobrindo esse caso.
    /// A contagem em si é a união de duas fontes (Job + varredura por
    /// `sysinfo`) -- ver `ProcessGroup::process_count` em `porecatu-pty`.
    fn liveness(&self) -> Liveness {
        if self
            .group
            .as_ref()
            .is_some_and(|group| group.process_count() > 1)
        {
            Liveness::ExtraProcesses
        } else {
            Liveness::Idle
        }
    }

    /// Consulta pontual ao PID raiz, só em Linux/macOS -- `ProcessGroup::cwd`
    /// não existe no Windows (ADR-0038 §3), então devolve sempre `None` lá em
    /// vez de expor o `#[cfg]` para quem chama.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn cwd_fallback(&self) -> Option<PathBuf> {
        self.group.as_ref().and_then(|group| group.cwd())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn cwd_fallback(&self) -> Option<PathBuf> {
        None
    }
}

/// O backend que não faz nada ([`BackendSpec::Dormant`]): leitor vazio,
/// escritor que descarta, nunca termina, nada a perder ao fechar.
pub(crate) struct DormantBackend;

struct DormantInspect;

impl Inspect for DormantInspect {
    fn liveness(&self) -> Liveness {
        Liveness::Idle
    }

    fn cwd_fallback(&self) -> Option<PathBuf> {
        None
    }
}

impl Backend for DormantBackend {
    fn reader(&mut self) -> io::Result<Box<dyn Read + Send>> {
        Ok(Box::new(io::empty()))
    }

    fn writer(&mut self) -> io::Result<Box<dyn Write + Send>> {
        Ok(Box::new(io::sink()))
    }

    fn resize(&mut self, _rows: u16, _cols: u16) -> io::Result<()> {
        Ok(())
    }

    fn try_wait(&mut self) -> Option<EndReason> {
        None
    }

    fn kill(&mut self) {}

    fn finish(self: Box<Self>, _how: Finish) {}

    fn inspector(&self) -> Arc<dyn Inspect> {
        Arc::new(DormantInspect)
    }
}

fn already_taken() -> io::Error {
    io::Error::other("backend stream already taken")
}

impl Backend for PtyBackend {
    fn reader(&mut self) -> io::Result<Box<dyn Read + Send>> {
        self.reader.take().ok_or_else(already_taken)
    }

    fn writer(&mut self) -> io::Result<Box<dyn Write + Send>> {
        self.writer.take().ok_or_else(already_taken)
    }

    fn resize(&mut self, rows: u16, cols: u16) -> io::Result<()> {
        self.pty.resize(rows, cols).map_err(io::Error::other)
    }

    fn try_wait(&mut self) -> Option<EndReason> {
        match self.pty.try_wait() {
            Ok(Some(status)) => Some(EndReason::Exited {
                success: status.success,
                code: status.code,
            }),
            _ => None,
        }
    }

    fn kill(&mut self) {
        let _ = self.pty.kill();
        // ADR-0033: fechamento **pedido pelo usuário** -- mata a árvore
        // inteira. `kill_tree` fecha o handle do Job (mata na hora quem
        // propagou a associação, mesmo sobrevivendo ao próprio pai
        // intermediário) e varre os descendentes vivos do PID raiz por
        // `sysinfo` (cobre shells como PowerShell 7, que não propagam a
        // associação ao Job -- ver a nota do módulo `job.rs` em
        // `porecatu-pty`).
        if let Some(group) = self.group.take() {
            group.kill_tree();
        }
    }

    /// Nunca fecha o PTY, nos dois caminhos -- ver o porquê em
    /// [`watch_loop`](crate::terminal). `mem::forget` evita rodar o destrutor
    /// (`ClosePseudoConsole` no Windows); o SO reclama a handle (e todas as
    /// outras do processo) quando o `porecatu` inteiro sai. Consequência
    /// aceita: a thread de leitura fica parada num `read()` que nunca mais
    /// retorna, pelo resto da vida do app -- não é um recurso do SO vazando
    /// (o processo filho já foi morto por `kill()` antes disso), só uma
    /// thread e sua pilha ociosas.
    fn finish(mut self: Box<Self>, how: Finish) {
        let group = self.group.take();
        std::mem::forget(self);
        if how == Finish::Ended {
            // ADR-0033: saída **natural** do shell (ex. `exit` digitado) --
            // ao contrário do fechamento pedido pelo usuário, esta cópia é
            // esquecida (`mem::forget`), não dropada. Isso barra pra sempre
            // esta referência de decrementar o `Arc`: mesmo que a cópia do
            // lado da UI droppe depois (quando a aba `Exited` for finalmente
            // fechada de verdade), a contagem nunca alcança zero por causa
            // desta, e o Job nunca fecha -- um processo que o shell tenha
            // deliberadamente destacado (`start /b algo & exit`) sobrevive,
            // como hoje. Fora do Windows `ProcessGroup` não carrega nada que
            // precise de `Drop` ainda (dívida Unix do ADR-0033), então o
            // clippy vê este `forget` como não-operação nessa plataforma --
            // `allow` deliberado, para o dia em que a dívida for paga sem
            // precisar lembrar de tirá-lo.
            #[allow(clippy::forget_non_drop)]
            std::mem::forget(group);
        }
    }

    fn inspector(&self) -> Arc<dyn Inspect> {
        Arc::new(PtyInspect {
            group: self.group.clone(),
        })
    }
}
