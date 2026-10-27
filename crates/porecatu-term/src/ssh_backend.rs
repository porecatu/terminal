// SPDX-License-Identifier: GPL-3.0-or-later

//! `SshBackend`: o canal SSH de `porecatu-ssh` como backend de terminal
//! (ADR-0071 §1, ADR-0072 §4). Escrito sobre a API bloqueante daquele crate,
//! como o `PtyBackend` é sobre `porecatu-pty`.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use porecatu_ssh::{
    Asker, End, EndProbe, FailKind, HostKeyQuestion, KnownEntry, LostKind, Notice, OpenError,
    Question, ReadCause, Reply, SecretQuestion, SecretTarget, SshChannel, SshSpawn,
};

use crate::backend::{
    Backend, EndReason, FailCause, Finish, HostKeyEntry, HostKeyPresented, Inspect,
    KnownHostsCause, Liveness, LostCause,
};
use crate::prompt::{Answer, PromptKind, PromptSink, SecretLabel, TermNotice};

pub(crate) struct SshBackend {
    channel: SshChannel,
}

impl SshBackend {
    /// Devolve na hora: o canal conecta e autentica em segundo plano, e o
    /// resultado chega por `try_wait`. As perguntas (chave de host) saem pelo
    /// `prompts` do terminal.
    pub(crate) fn open(spawn: SshSpawn, prompts: PromptSink) -> Result<Self, OpenError> {
        Ok(Self {
            channel: SshChannel::open(spawn, Arc::new(PromptAsker(prompts)))?,
        })
    }
}

/// Faz a pergunta de `porecatu-ssh` chegar à UI como `TermEvent::Prompt`
/// (ADR-0071 §5). O tipo da pergunta mora em `porecatu-ssh` porque
/// `porecatu-term` depende dele, e não o contrário.
struct PromptAsker(PromptSink);

impl Asker for PromptAsker {
    fn ask(&self, question: Question, on_reply: Box<dyn FnOnce(Reply) + Send>) {
        match question {
            Question::HostKey(HostKeyQuestion {
                algorithm,
                fingerprint,
                other_types,
                skipped_lines,
                ..
            }) => self.0.ask_with(
                PromptKind::HostKey {
                    algorithm,
                    fingerprint,
                    known_types: other_types,
                    skipped_lines,
                },
                // Só `Confirm` confirma: `Cancel` e qualquer outra resposta (um
                // segredo, que aqui não faz sentido) recusam.
                move |answer| {
                    on_reply(match answer {
                        Answer::Confirm => Reply::Confirm,
                        Answer::Secret { .. } | Answer::Cancel => Reply::Cancel,
                    });
                },
            ),
            Question::Secret(SecretQuestion {
                target,
                vault_available,
                retry,
            }) => self.0.ask_with(
                PromptKind::Secret {
                    label: match target {
                        SecretTarget::Password { user, host } => {
                            SecretLabel::Password { user, host }
                        }
                        SecretTarget::KeyPassphrase { path } => SecretLabel::KeyPassphrase { path },
                    },
                    vault_available,
                    retry,
                },
                // O segredo atravessa por movimento. Confirmar um segredo
                // sem texto não faz sentido: cancela.
                move |answer| {
                    on_reply(match answer {
                        Answer::Secret { value, remember } => Reply::Secret { value, remember },
                        Answer::Confirm | Answer::Cancel => Reply::Cancel,
                    });
                },
            ),
        }
    }

    fn notice(&self, notice: Notice) {
        self.0.notice(match notice {
            Notice::SecretNotSaved => TermNotice::SecretNotSaved,
        });
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
                FailKind::HostKeyChanged {
                    presented,
                    expected,
                } => FailCause::HostKeyChanged {
                    presented: presented_key(presented),
                    expected: expected.into_iter().map(entry).collect(),
                },
                FailKind::HostKeyRevoked {
                    presented,
                    entry: revoked,
                } => FailCause::HostKeyRevoked {
                    presented: presented_key(presented),
                    entry: entry(revoked),
                },
                FailKind::KnownHostsUnreadable { file, cause } => FailCause::KnownHostsUnreadable {
                    file,
                    cause: match cause {
                        ReadCause::PermissionDenied => KnownHostsCause::PermissionDenied,
                        ReadCause::NotText => KnownHostsCause::NotText,
                        ReadCause::Other => KnownHostsCause::Other,
                    },
                },
                FailKind::ChannelRefused => FailCause::ChannelRefused,
                FailKind::Runtime => FailCause::Runtime,
            },
        },
    }
}

fn presented_key(presented: porecatu_ssh::HostKeyPresented) -> HostKeyPresented {
    HostKeyPresented {
        algorithm: presented.algorithm,
        fingerprint: presented.fingerprint,
    }
}

fn entry(entry: KnownEntry) -> HostKeyEntry {
    HostKeyEntry {
        file: entry.file,
        line: entry.line,
        fingerprint: entry.fingerprint,
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
        ];
        for (kind, cause) in pairs {
            assert_eq!(translate(End::Failed(kind)), EndReason::Failed { cause });
        }
    }

    #[test]
    fn host_key_failures_keep_what_the_note_needs() {
        use porecatu_ssh::HostKeyPresented as SshPresented;
        let presented = || SshPresented {
            algorithm: "ssh-ed25519".into(),
            fingerprint: "SHA256:abc".into(),
        };
        let known = || KnownEntry {
            file: PathBuf::from("known_hosts"),
            line: 4,
            fingerprint: "SHA256:old".into(),
        };
        let entry = || HostKeyEntry {
            file: PathBuf::from("known_hosts"),
            line: 4,
            fingerprint: "SHA256:old".into(),
        };
        let expected_presented = || HostKeyPresented {
            algorithm: "ssh-ed25519".into(),
            fingerprint: "SHA256:abc".into(),
        };

        assert_eq!(
            translate(End::Failed(FailKind::HostKeyChanged {
                presented: presented(),
                expected: vec![known()],
            })),
            EndReason::Failed {
                cause: FailCause::HostKeyChanged {
                    presented: expected_presented(),
                    expected: vec![entry()],
                }
            }
        );
        assert_eq!(
            translate(End::Failed(FailKind::HostKeyRevoked {
                presented: presented(),
                entry: known(),
            })),
            EndReason::Failed {
                cause: FailCause::HostKeyRevoked {
                    presented: expected_presented(),
                    entry: entry(),
                }
            }
        );
        assert_eq!(
            translate(End::Failed(FailKind::KnownHostsUnreadable {
                file: PathBuf::from("known_hosts"),
                cause: ReadCause::NotText,
            })),
            EndReason::Failed {
                cause: FailCause::KnownHostsUnreadable {
                    file: PathBuf::from("known_hosts"),
                    cause: KnownHostsCause::NotText,
                }
            }
        );
        assert_eq!(
            translate(End::Failed(FailKind::HostKeyRejectedByUser)),
            EndReason::Failed {
                cause: FailCause::HostKeyRejectedByUser
            }
        );
    }

    fn host_key_question() -> Question {
        Question::HostKey(HostKeyQuestion {
            host: "example.org".into(),
            port: 22,
            algorithm: "ssh-ed25519".into(),
            fingerprint: "SHA256:abc".into(),
            other_types: vec!["ecdsa-sha2-nistp256".into()],
            skipped_lines: 2,
        })
    }

    /// A pergunta de `porecatu-ssh` chega à UI como `TermEvent::Prompt` com os
    /// dados do diálogo, e a resposta volta ao `on_reply`.
    #[test]
    fn a_host_key_question_becomes_a_prompt_and_only_confirm_confirms() {
        use crate::event::TermEvent;
        use crate::prompt::PromptGuard;

        let answer_with = |answer: Answer| {
            let (events, unread) = std::sync::mpsc::channel();
            let guard = PromptGuard::new();
            let asker = PromptAsker(guard.sink(events, Arc::new(|| {})));
            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            asker.ask(
                host_key_question(),
                Box::new(move |reply| {
                    let _ = reply_tx.send(reply);
                }),
            );
            let TermEvent::Prompt(prompt) = unread.try_recv().unwrap() else {
                panic!("expected a prompt event");
            };
            assert_eq!(
                prompt.kind,
                PromptKind::HostKey {
                    algorithm: "ssh-ed25519".into(),
                    fingerprint: "SHA256:abc".into(),
                    known_types: vec!["ecdsa-sha2-nistp256".into()],
                    skipped_lines: 2,
                }
            );
            guard.answer(prompt.id, answer);
            reply_rx.recv().unwrap()
        };

        assert!(matches!(answer_with(Answer::Confirm), Reply::Confirm));
        assert!(matches!(answer_with(Answer::Cancel), Reply::Cancel));
        // A secret is not a confirmation, whatever it says.
        assert!(matches!(
            answer_with(Answer::Secret {
                value: crate::prompt::Secret::new("yes".into()),
                remember: false
            }),
            Reply::Cancel
        ));
    }

    fn secret_question(retry: bool) -> Question {
        Question::Secret(SecretQuestion {
            target: SecretTarget::Password {
                user: "deploy".into(),
                host: "example.org".into(),
            },
            vault_available: true,
            retry,
        })
    }

    /// A pergunta de segredo chega à UI com o destino, o cofre e a linha de
    /// erro; a resposta volta com o texto **movido**, sem cópia no caminho.
    #[test]
    fn a_secret_question_becomes_a_prompt_and_the_typed_secret_moves_back() {
        use crate::event::TermEvent;
        use crate::prompt::PromptGuard;

        let (events, unread) = std::sync::mpsc::channel();
        let guard = PromptGuard::new();
        let asker = PromptAsker(guard.sink(events, Arc::new(|| {})));
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        asker.ask(
            secret_question(true),
            Box::new(move |reply| {
                let _ = reply_tx.send(reply);
            }),
        );
        let TermEvent::Prompt(prompt) = unread.try_recv().unwrap() else {
            panic!("expected a prompt event");
        };
        assert_eq!(
            prompt.kind,
            PromptKind::Secret {
                label: SecretLabel::Password {
                    user: "deploy".into(),
                    host: "example.org".into(),
                },
                vault_available: true,
                retry: true,
            }
        );

        let value = crate::prompt::Secret::new("proof-secret".into());
        let address = value.expose().as_ptr();
        guard.answer(
            prompt.id,
            Answer::Secret {
                value,
                remember: true,
            },
        );

        match reply_rx.recv().unwrap() {
            Reply::Secret { value, remember } => {
                assert!(remember);
                assert_eq!(value.expose(), "proof-secret");
                assert_eq!(value.expose().as_ptr(), address, "moved, not copied");
            }
            other => panic!("unexpected reply: {other:?}"),
        }
    }

    #[test]
    fn a_secret_question_is_only_answered_by_a_secret() {
        use crate::prompt::PromptGuard;

        let reply_to = |answer: Answer| {
            let (events, unread) = std::sync::mpsc::channel();
            let guard = PromptGuard::new();
            let asker = PromptAsker(guard.sink(events, Arc::new(|| {})));
            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            asker.ask(
                secret_question(false),
                Box::new(move |reply| {
                    let _ = reply_tx.send(reply);
                }),
            );
            let crate::event::TermEvent::Prompt(prompt) = unread.try_recv().unwrap() else {
                panic!("expected a prompt event");
            };
            guard.answer(prompt.id, answer);
            reply_rx.recv().unwrap()
        };

        assert!(matches!(reply_to(Answer::Cancel), Reply::Cancel));
        // Trusting a host is not typing a password.
        assert!(matches!(reply_to(Answer::Confirm), Reply::Cancel));
    }

    #[test]
    fn a_vault_write_failure_reaches_the_ui_as_a_notice_event() {
        use crate::event::TermEvent;
        use crate::prompt::PromptGuard;

        let (events, unread) = std::sync::mpsc::channel();
        let guard = PromptGuard::new();
        let asker = PromptAsker(guard.sink(events, Arc::new(|| {})));

        asker.notice(Notice::SecretNotSaved);

        assert!(matches!(
            unread.try_recv().unwrap(),
            TermEvent::Notice(TermNotice::SecretNotSaved)
        ));
    }

    #[test]
    fn dropping_the_terminal_cancels_an_open_host_key_question() {
        use crate::prompt::PromptGuard;

        let (events, _unread) = std::sync::mpsc::channel();
        let guard = PromptGuard::new();
        let asker = PromptAsker(guard.sink(events, Arc::new(|| {})));
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        asker.ask(
            host_key_question(),
            Box::new(move |reply| {
                let _ = reply_tx.send(reply);
            }),
        );

        drop(guard);

        assert!(matches!(reply_rx.recv().unwrap(), Reply::Cancel));
    }

    /// Fim a fim, sem servidor: um destino que recusa a conexão aparece como
    /// `EndReason::Failed` pelo caminho do backend, e a consulta de
    /// "conexão aberta" cai de `OpenConnection` para `Idle`.
    #[test]
    fn a_refused_connection_ends_the_backend_and_closes_the_liveness() {
        // Porta 1 no loopback: nada escuta ali.
        let (events, _unread) = std::sync::mpsc::channel();
        let prompts = crate::prompt::PromptGuard::new().sink(events, Arc::new(|| {}));
        let mut backend =
            SshBackend::open(SshSpawn::new("127.0.0.1", 1, "tester"), prompts).unwrap();
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
