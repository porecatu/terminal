// SPDX-License-Identifier: GPL-3.0-or-later

//! O runtime `tokio` do processo (ADR-0072 §3): criado na primeira conexão,
//! duas threads de trabalho para todas as conexões, nunca na main thread.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Runtime};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Nome das threads de trabalho.
const THREAD_NAME: &str = "porecatu-ssh";

/// `None` quando o SO não deixou criar as threads; a próxima chamada tenta de
/// novo.
fn runtime() -> Option<&'static Runtime> {
    if let Some(runtime) = RUNTIME.get() {
        return Some(runtime);
    }
    let built = Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name(THREAD_NAME)
        .enable_io()
        .enable_time()
        .build()
        .ok()?;
    // Se outra thread chegou primeiro, o runtime que sobrou cai aqui, sem
    // `block_on` nem espera: um runtime sem tarefa se desfaz na hora.
    let _ = RUNTIME.set(built);
    RUNTIME.get()
}

/// Entrega `task` ao runtime. `false` se o runtime não pôde ser criado
/// (`FailKind::Runtime`).
pub(crate) fn spawn(task: impl Future<Output = ()> + Send + 'static) -> bool {
    match runtime() {
        Some(runtime) => {
            runtime.spawn(task);
            true
        }
        None => false,
    }
}
