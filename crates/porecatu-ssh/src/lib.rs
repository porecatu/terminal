// SPDX-License-Identifier: GPL-3.0-or-later

//! Cliente SSH embutido (ADR-0072): `russh` sobre um runtime `tokio` criado
//! sob demanda **dentro deste crate**, atrás de uma API síncrona. É o único
//! crate do workspace que depende de `russh` e de `tokio`, e não depende de
//! nenhum crate do projeto: `porecatu-term` o usa, nunca o contrário.
//!
//! Nenhum tipo de `russh` nem de `tokio` aparece na API pública, e nenhuma
//! função pública é `async`.
//!
//! **Nenhum erro deste crate carrega segredo ou texto livre vindo do usuário**
//! (ADR-0073 §5): as causas de fim são enums sem campo de texto.

mod ask;
mod auth;
mod channel;
mod end;
mod known_hosts;
mod runtime;
mod spawn;
mod task;
mod verify;

#[cfg(test)]
mod tests;

pub use ask::{Asker, HostKeyQuestion, Question, Reply};
pub use channel::{ChannelReader, ChannelWriter, EndProbe, SshChannel};
pub use end::{End, FailKind, HostKeyPresented, LostKind};
pub use known_hosts::{KnownEntry, ReadCause};
pub use spawn::{
    AuthMethod, Connection, ConnectionId, DEFAULT_CONNECT_TIMEOUT, OpenError, SshSpawn,
};
