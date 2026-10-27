// SPDX-License-Identifier: GPL-3.0-or-later

//! `SshBackend`: o canal SSH de `porecatu-ssh` como backend de terminal
//! (ADR-0071 §1, ADR-0072 §4). Escrito sobre a API bloqueante daquele crate,
//! como o `PtyBackend` é sobre `porecatu-pty`.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use porecatu_ssh::{End, EndProbe, FailKind, LostKind, OpenError, SshChannel, SshSpawn};

use crate::backend::{Backend, EndReason, FailCause, Finish, Inspect, Liveness, LostCause};

pub(crate) struct SshBackend {
    channel: SshChannel,
}

impl SshBackend {
    /// Devolve na hora: o canal conecta e autentica em segundo plano, e o
    /// resultado chega por `try_wait`.
    pub(crate) fn open(spawn: SshSpawn) -> Result<Self, OpenError> {
        Ok(Self {
            channel: SshChannel::open(spawn)?,
        })
    }
}

/// `OpenConnection` enquanto o canal está aberto ou abrindo; depois do fim, o
/// painel é um painel encerrado e fecha sem perguntar (ADR-0071 §6).
struct SshInspect {
    probe: EndProbe,
}

impl Inspect for SshInspect {
    fn liveness(&self) -> Liveness {
        if self.probe.is_open() {
            Liveness::OpenConnection
        } else {
            Liveness::Idle
        }
    }

    /// Não há processo local para consultar, e o `cwd` de OSC 7 numa aba SSH
    /// é remoto (RF-20.22).
    fn cwd_fallback(&self) -> Option<PathBuf> {
        None
    }
}

impl Backend for SshBackend {
    fn reader(&mut self) -> io::Result<Box<dyn Read + Send>> {
        Ok(Box::new(self.channel.reader()))
    }

    fn writer(&mut self) -> io::Result<Box<dyn Write + Send>> {
        Ok(Box::new(self.channel.writer()))
    }

    fn resize(&mut self, rows: u16, cols: u16) -> io::Result<()> {
        self.channel.resize(rows, cols);
        Ok(())
    }

    fn try_wait(&mut self) -> Option<EndReason> {
        self.channel.try_end().map(translate)
    }

    /// Sem processo local para matar: `finish` fecha o canal logo em seguida.
    fn kill(&mut self) {}

    /// Fecha o canal e solta a conexão normalmente -- aqui não há deadlock de
    /// ConPTY a contornar, e vazar uma conexão TCP seria vazar um recurso de
    /// verdade (ADR-0071 §1).
    fn finish(self: Box<Self>, _how: Finish) {
        self.channel.close();
    }

    fn inspector(&self) -> Arc<dyn Inspect> {
        Arc::new(SshInspect {
            probe: self.channel.probe(),
        })
    }
}

/// O fim de `porecatu-ssh` na razão de `porecatu-term` (ADR-0071 §3).
fn translate(end: End) -> EndReason {
    match end {
        End::RemoteEnded { code, signal } => EndReason::RemoteEnded { code, signal },
        End::Lost(kind) => EndReason::Lost {
            cause: match kind {
                LostKind::KeepaliveTimeout => LostCause::KeepaliveTimeout,
                LostKind::Transport => LostCause::Transport,
                LostKind::ServerDisconnect => LostCause::ServerDisconnect,
            },
        },
        End::Failed(kind) => EndReason::Failed {
            cause: match kind {
                FailKind::Refused => FailCause::Refused,
                FailKind::Timeout => FailCause::Timeout,
                FailKind::AuthRejected => FailCause::AuthRejected,
                FailKind::AuthCancelled => FailCause::AuthCancelled,
                FailKind::AuthUnsupported => FailCause::AuthUnsupported,
                FailKind::KeyUnreadable => FailCause::KeyUnreadable,
                FailKind::HostKeyRejectedByUser => FailCause::HostKeyRejectedByUser,
                FailKind::HostKeyChanged => FailCause::HostKeyChanged {},
                FailKind::HostKeyRevoked => FailCause::HostKeyRevoked {},
                FailKind::KnownHostsUnreadable => FailCause::KnownHostsUnreadable {},
                FailKind::ChannelRefused => FailCause::ChannelRefused,
                FailKind::Runtime => FailCause::Runtime,
                FailKind::HostKeyUnverified => FailCause::HostKeyUnverified,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_end_keeps_its_code_and_signal() {
        assert_eq!(
            translate(End::RemoteEnded {
                code: Some(3),
                signal: None
            }),
            EndReason::RemoteEnded {
                code: Some(3),
                signal: None
            }
        );
        assert_eq!(
            translate(End::RemoteEnded {
                code: None,
                signal: Some("TERM".into())
            }),
            EndReason::RemoteEnded {
                code: None,
                signal: Some("TERM".into())
            }
        );
    }

    #[test]
    fn a_lost_connection_is_lost_and_never_a_remote_end() {
        assert_eq!(
            translate(End::Lost(LostKind::KeepaliveTimeout)),
            EndReason::Lost {
                cause: LostCause::KeepaliveTimeout
            }
        );
        assert_eq!(
            translate(End::Lost(LostKind::Transport)),
            EndReason::Lost {
                cause: LostCause::Transport
            }
        );
        assert_eq!(
            translate(End::Lost(LostKind::ServerDisconnect)),
            EndReason::Lost {
                cause: LostCause::ServerDisconnect
            }
        );
    }

    #[test]
    fn failures_map_one_to_one() {
        let pairs = [
            (FailKind::Refused, FailCause::Refused),
            (FailKind::Timeout, FailCause::Timeout),
            (FailKind::AuthRejected, FailCause::AuthRejected),
            (FailKind::KeyUnreadable, FailCause::KeyUnreadable),
            (FailKind::ChannelRefused, FailCause::ChannelRefused),
            (FailKind::Runtime, FailCause::Runtime),
            (FailKind::HostKeyUnverified, FailCause::HostKeyUnverified),
        ];
        for (kind, cause) in pairs {
            assert_eq!(translate(End::Failed(kind)), EndReason::Failed { cause });
        }
    }

    /// Fim a fim, sem servidor: um destino que recusa a conexão aparece como
    /// `EndReason::Failed` pelo caminho do backend, e a consulta de
    /// "conexão aberta" cai de `OpenConnection` para `Idle`.
    #[test]
    fn a_refused_connection_ends_the_backend_and_closes_the_liveness() {
        // Porta 1 no loopback: nada escuta ali.
        let mut backend = SshBackend::open(SshSpawn::new("127.0.0.1", 1, "tester")).unwrap();
        let inspector = backend.inspector();

        let start = std::time::Instant::now();
        let reason = loop {
            if let Some(reason) = backend.try_wait() {
                break reason;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            std::thread::sleep(std::time::Duration::from_millis(10));
        };

        assert_eq!(
            reason,
            EndReason::Failed {
                cause: FailCause::Refused
            }
        );
        assert_eq!(inspector.liveness(), Liveness::Idle);
        assert_eq!(inspector.cwd_fallback(), None);
    }
}
