// SPDX-License-Identifier: GPL-3.0-or-later

//! Medição das cinco métricas do PRD-000 (etapa 6 da F6), por `Instant` nos
//! pontos que já existem -- atrás de `PORECATU_TRACE` (variável de
//! ambiente, não flag nova: a superfície de `argv` do ADR-0040 é pequena de
//! propósito, e a costura de medição não é caso de uso que justifique
//! crescê-la, mesmo raciocínio de `PORECATU_SESSION`). Sem a variável, todo
//! ponto de chamada confere [`enabled`] primeiro -- a aritmética de
//! `Instant` nem roda, e nenhum `eprintln!` acontece.
//!
//! Três pontos instrumentados (os outros dois -- CPU ociosa e reconstrução
//! de contexto -- não são intervalo de `Instant`, ver roadmap):
//! `main` -> primeiro byte do PTY -> primeiro frame (tempo até o primeiro
//! prompt utilizável); evento de teclado -> submissão do frame (latência de
//! tecla até pixel); início de `resumed` -> primeiro frame da janela
//! restaurada (restauração de sessão).

use std::sync::OnceLock;
use std::time::Instant;

/// Lida uma vez por processo -- `OnceLock` em vez de reler
/// `std::env::var_os` a cada chamada, que aconteceria em todo `RedrawRequested`
/// se `report` conferisse a variável direto.
pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("PORECATU_TRACE").is_some())
}

/// Imprime `label: N.N ms` em `stderr` -- só chamado depois de [`enabled`]
/// já ter sido conferido pelo chamador, então o `elapsed()` só roda com a
/// variável setada.
pub(crate) fn report(label: &str, since: Instant) {
    eprintln!(
        "[porecatu-trace] {label}: {:.2} ms",
        since.elapsed().as_secs_f64() * 1000.0
    );
}
