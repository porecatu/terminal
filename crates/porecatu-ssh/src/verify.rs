// SPDX-License-Identifier: GPL-3.0-or-later

//! Verificação da chave do host. **Nada aqui aceita chave sem verificar.**
//!
//! O caminho de produção é [`Unverified`], que recusa toda chave: o prompt 10
//! o troca pela leitura do `known_hosts` (ADR-0074). Só os testes
//! (`cfg(test)`) têm um verificador que aceita.

use russh::keys::PublicKey;

use crate::end::FailKind;

/// Decide se a chave que o servidor apresentou é a do host.
pub(crate) trait HostKeyVerifier: Send + Sync + 'static {
    fn verify(&self, host: &str, port: u16, key: &PublicKey) -> Result<(), FailKind>;
}

/// Produção, até o prompt 10: recusa tudo.
pub(crate) struct Unverified;

impl HostKeyVerifier for Unverified {
    fn verify(&self, _host: &str, _port: u16, _key: &PublicKey) -> Result<(), FailKind> {
        Err(FailKind::HostKeyUnverified)
    }
}
