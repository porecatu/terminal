// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

/// Erro de operação de PTY. Envolve a causa original (`portable_pty` devolve
/// `anyhow::Error`) sem expor o tipo de terceiro na assinatura pública.
#[derive(Debug)]
pub struct PtyError {
    action: &'static str,
    cause: String,
}

impl PtyError {
    pub(crate) fn new(action: &'static str, cause: impl fmt::Display) -> Self {
        Self {
            action,
            cause: cause.to_string(),
        }
    }
}

impl fmt::Display for PtyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pty: {}: {}", self.action, self.cause)
    }
}

impl std::error::Error for PtyError {}
