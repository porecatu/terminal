// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

/// Qual operação de PTY falhou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyErrorKind {
    OpenPty,
    SpawnCommand,
    TryCloneReader,
    TakeWriter,
    Resize,
    TryWait,
    Wait,
    Kill,
}

impl PtyErrorKind {
    /// Nome técnico da operação (o da chamada do `portable_pty`), não frase
    /// de interface: aparece em `Display` e em depuração.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenPty => "openpty",
            Self::SpawnCommand => "spawn_command",
            Self::TryCloneReader => "try_clone_reader",
            Self::TakeWriter => "take_writer",
            Self::Resize => "resize",
            Self::TryWait => "try_wait",
            Self::Wait => "wait",
            Self::Kill => "kill",
        }
    }
}

/// Erro de operação de PTY. Envolve a causa original (`portable_pty` devolve
/// `anyhow::Error`) sem expor o tipo de terceiro na assinatura pública.
#[derive(Debug)]
pub struct PtyError {
    kind: PtyErrorKind,
    cause: String,
}

impl PtyError {
    pub(crate) fn new(kind: PtyErrorKind, cause: impl fmt::Display) -> Self {
        Self {
            kind,
            cause: cause.to_string(),
        }
    }

    /// A operação que falhou.
    pub fn kind(&self) -> PtyErrorKind {
        self.kind
    }

    /// Texto da causa, do sistema operacional ou da biblioteca de PTY,
    /// como chegou.
    pub fn cause(&self) -> &str {
        &self.cause
    }
}

impl fmt::Display for PtyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pty: {}: {}", self.kind.as_str(), self.cause)
    }
}

impl std::error::Error for PtyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_operation_and_keeps_the_cause() {
        let err = PtyError::new(PtyErrorKind::SpawnCommand, "file not found");
        assert_eq!(err.kind(), PtyErrorKind::SpawnCommand);
        assert_eq!(err.cause(), "file not found");
        assert_eq!(err.to_string(), "pty: spawn_command: file not found");
    }
}
