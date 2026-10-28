// SPDX-License-Identifier: GPL-3.0-or-later

//! Como um canal terminou, classificado pela tabela do ADR-0076 §1.
//! `porecatu-term` traduz isto para `EndReason`; os enums espelham, um para
//! um, as listas fechadas do ADR-0071 §3.

use std::path::PathBuf;

use crate::known_hosts::{KnownEntry, ReadCause};

/// Fim de um canal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// `exit-status` ou `exit-signal` no canal, ou o servidor fechou o canal
    /// de propósito (sem `exit-*`, os dois campos vêm vazios). Não reconecta.
    RemoteEnded {
        code: Option<u32>,
        signal: Option<String>,
    },
    /// A conexão caiu sem o servidor encerrar a sessão. Reconecta.
    Lost(LostKind),
    /// Não chegou a abrir.
    Failed(FailKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "closed list of ADR-0071 §3; some are produced by later prompts"
)]
pub enum LostKind {
    KeepaliveTimeout,
    Transport,
    ServerDisconnect,
}

/// A chave que o servidor apresentou, para a nota de bloqueio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyPresented {
    pub algorithm: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "closed list of ADR-0071 §3; some are produced by later prompts"
)]
pub enum FailKind {
    Refused,
    Timeout,
    AuthRejected,
    AuthCancelled,
    AuthUnsupported,
    KeyUnreadable,
    /// O usuário cancelou o diálogo de host desconhecido.
    HostKeyRejectedByUser,
    /// Há chave do mesmo tipo registrada e nenhuma casa (RF-20.17). Leva as
    /// entradas divergentes: arquivo e linha, para a nota.
    HostKeyChanged {
        presented: HostKeyPresented,
        expected: Vec<KnownEntry>,
    },
    /// `@revoked` casa com a chave apresentada (RF-20.18).
    HostKeyRevoked {
        presented: HostKeyPresented,
        entry: KnownEntry,
    },
    /// Um `known_hosts` que existe e não pôde ser lido (ADR-0074 §1).
    KnownHostsUnreadable {
        file: PathBuf,
        cause: ReadCause,
    },
    ChannelRefused,
    Runtime,
}
