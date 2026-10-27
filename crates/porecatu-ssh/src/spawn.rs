// SPDX-License-Identifier: GPL-3.0-or-later

//! O que `porecatu-ui` entrega para abrir um canal (ADR-0072 §4, §6): tipos
//! do projeto, nenhum do `russh`.

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Teto de TCP mais troca de chaves (ADR-0072 §8).
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Método de autenticação. A lista `SshSpawn::auth` é a regra: a ordem dada é
/// a ordem tentada, e método fora dela não é tentado (ADR-0072 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    Agent,
    Key,
    Password,
}

/// Identifica uma conexão viva (ADR-0072 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(u64);

impl ConnectionId {
    pub(crate) fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Conexão nova, ou canal numa conexão que já existe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    New,
    /// Dividir um painel SSH (RF-20.23). Ainda não implementado.
    Share(ConnectionId),
}

/// Destino e parâmetros de um canal SSH.
#[derive(Debug, Clone)]
pub struct SshSpawn {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: Vec<AuthMethod>,
    /// Arquivo da chave, já resolvido (sem `~`). A chave só é usada se
    /// `AuthMethod::Key` está em `auth`.
    pub identity_file: Option<PathBuf>,
    pub rows: u16,
    pub cols: u16,
    /// Valor de `TERM` do pseudo-terminal remoto (ADR-0012).
    pub term: String,
    pub connection: Connection,
    /// Teto até a troca de chaves; injetável para os testes não esperarem 15 s.
    pub connect_timeout: Duration,
}

impl SshSpawn {
    /// Ordem default de autenticação (RF-20.9), tamanho 24x80 e
    /// `TERM=xterm-256color`.
    pub fn new(host: impl Into<String>, port: u16, user: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port,
            user: user.into(),
            auth: vec![AuthMethod::Agent, AuthMethod::Key, AuthMethod::Password],
            identity_file: None,
            rows: 24,
            cols: 80,
            term: "xterm-256color".to_owned(),
            connection: Connection::New,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }
}

/// O canal nem chegou a ser criado. Fora disso, toda falha é um [`End`] do
/// canal, nunca um erro de `open`.
///
/// [`End`]: crate::End
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// `Connection::Share` entra com a divisão de painel.
    ShareUnsupported,
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::ShareUnsupported => f.write_str("ssh: shared connection not implemented"),
        }
    }
}

impl std::error::Error for OpenError {}
