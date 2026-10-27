// SPDX-License-Identifier: GPL-3.0-or-later

//! Como um canal terminou, classificado pela tabela do ADR-0076 §1.
//! `porecatu-term` traduz isto para `EndReason`; os enums espelham, um para
//! um, as listas fechadas do ADR-0071 §3.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    HostKeyRejectedByUser,
    HostKeyChanged,
    HostKeyRevoked,
    KnownHostsUnreadable,
    ChannelRefused,
    Runtime,
    /// **Provisória (prompt 09).** A chave do host ainda não é verificada, e
    /// por isso toda conexão de produção que chega até ela falha aqui. O
    /// prompt 10 (`known_hosts`) remove esta variante; ela não pode chegar a
    /// `main`.
    HostKeyUnverified,
}
