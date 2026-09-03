// SPDX-License-Identifier: GPL-3.0-or-later

//! Abstração de PTY cross-platform. Encapsula `portable-pty` (ADR-0004) —
//! nenhum outro crate do workspace importa `portable-pty` diretamente.
//!
//! Superfície mínima: [`spawn`], e em [`PtyHandle`] leitura, escrita, resize
//! e encerramento. Crate agnóstico de GUI: nenhuma dependência de outro
//! crate do projeto, síncrono, sem opinião sobre threading — quem chama
//! decide (ADR-0007 decide isso em `porecatu-ui`/`porecatu-term`).
//!
//! [`ProcessGroup`] (ADR-0033) acompanha o `PtyHandle` que `spawn` devolve,
//! separado dele de propósito — ver a nota do módulo `job`.

mod error;
mod job;
mod shell;
mod spawn;

pub use error::PtyError;
pub use job::ProcessGroup;
pub use shell::{resolve_default_shell, search_path};
pub use spawn::{PtyExitStatus, PtyHandle, PtySize, SpawnConfig, spawn};
