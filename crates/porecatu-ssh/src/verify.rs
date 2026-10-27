// SPDX-License-Identifier: GPL-3.0-or-later

//! Verificação da chave do host contra o `known_hosts` (ADR-0074 §6): roda
//! dentro do callback de chave do servidor, antes de qualquer autenticação.
//! **Nenhum caminho aceita uma chave que não foi verificada ou confirmada.**

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use russh::keys::{HashAlg, PublicKey};

use crate::ask::{Asker, HostKeyQuestion, Question, Reply};
use crate::end::{FailKind, HostKeyPresented};
use crate::known_hosts::{
    self, HostKeyStatus, KnownHostsFiles, WriteError, algorithm_name, fingerprint,
};

/// `true` enquanto uma pergunta espera o usuário. O teto de conexão não conta
/// esse tempo: ninguém decide uma chave de host em 15 s.
#[derive(Default)]
pub(crate) struct Waiting(AtomicBool);

impl Waiting {
    pub(crate) fn is_waiting(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

pub(crate) struct HostKeyVerifier {
    pub(crate) files: KnownHostsFiles,
    pub(crate) asker: Arc<dyn Asker>,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) waiting: Arc<Waiting>,
}

impl HostKeyVerifier {
    /// `Ok` só se a chave está registrada, ou se o usuário a confirmou.
    pub(crate) async fn verify(
        &self,
        key: &PublicKey,
        hash_alg: Option<HashAlg>,
    ) -> Result<(), FailKind> {
        let presented = HostKeyPresented {
            algorithm: algorithm_name(key, hash_alg),
            fingerprint: fingerprint(key),
        };

        // Lido a cada conexão, fora da thread de trabalho.
        let files = self.files.clone();
        let texts = tokio::task::spawn_blocking(move || known_hosts::read_files(&files))
            .await
            .map_err(|_| FailKind::Runtime)?
            .map_err(|unreadable| FailKind::KnownHostsUnreadable {
                file: unreadable.file,
                cause: unreadable.cause,
            })?;

        let result = known_hosts::check(&texts, &self.host, self.port, key);
        match result.status {
            HostKeyStatus::Known => Ok(()),
            HostKeyStatus::Revoked { entry } => Err(FailKind::HostKeyRevoked { presented, entry }),
            HostKeyStatus::Changed { expected } => Err(FailKind::HostKeyChanged {
                presented,
                expected,
            }),
            HostKeyStatus::Unknown { other_types } => {
                let question = HostKeyQuestion {
                    host: self.host.clone(),
                    port: self.port,
                    algorithm: presented.algorithm,
                    fingerprint: presented.fingerprint,
                    other_types,
                    skipped_lines: result.skipped_lines.len(),
                };
                match self.ask(question).await {
                    Reply::Confirm => {
                        self.remember(key).await;
                        Ok(())
                    }
                    Reply::Cancel => Err(FailKind::HostKeyRejectedByUser),
                }
            }
        }
    }

    /// Pergunta e espera num `oneshot`, sem ocupar uma thread do runtime.
    /// Resposta que não vem (pergunta descartada) é `Cancel`.
    async fn ask(&self, question: HostKeyQuestion) -> Reply {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.waiting.0.store(true, Ordering::SeqCst);
        self.asker.ask(
            Question::HostKey(question),
            Box::new(move |reply| {
                let _ = tx.send(reply);
            }),
        );
        let reply = rx.await.unwrap_or(Reply::Cancel);
        self.waiting.0.store(false, Ordering::SeqCst);
        reply
    }

    /// Acrescenta a linha ao `known_hosts` do usuário (ADR-0074 §5). Falha ao
    /// gravar não derruba a conexão que o usuário acabou de aprovar -- o
    /// `ssh` também segue --, só não fica lembrada.
    async fn remember(&self, key: &PublicKey) {
        let Some(path) = self.files.user.clone() else {
            return;
        };
        let (host, port, key) = (self.host.clone(), self.port, key.clone());
        let written = tokio::task::spawn_blocking(move || {
            known_hosts::append_entry(&path, &host, port, &key)
        })
        .await;
        // Recusado por conter caractere perigoso, ou erro de I/O: nada foi
        // escrito e nada a fazer além de seguir.
        let _: Option<Result<(), WriteError>> = written.ok();
    }
}
