// SPDX-License-Identifier: GPL-3.0-or-later

//! Autenticação, na ordem de `SshSpawn::auth`, parando no primeiro método
//! aceito (ADR-0072 §6, §7). Método fora da lista não é tentado.
//!
//! `Password` ainda é pulado: o diálogo de senha e o cofre são do prompt 13.
//! Chave cifrada é tratada como ilegível pelo mesmo motivo.

use std::sync::Arc;
use std::time::Duration;

use russh::client::{Handle, Handler};
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::AgentClient;
use russh::keys::{Algorithm, PrivateKeyWithHashAlg, load_secret_key};

use crate::end::FailKind;
use crate::spawn::{AuthMethod, SshSpawn};

/// Espera pelo `server-sig-algs`, que decide o hash de uma chave RSA. Servidor
/// que não o manda não pode travar a autenticação.
const RSA_HASH_WAIT: Duration = Duration::from_secs(2);

#[cfg(windows)]
const OPENSSH_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

/// `Ok` quando um método foi aceito.
pub(crate) async fn authenticate<H>(
    handle: &mut Handle<H>,
    spawn: &SshSpawn,
) -> Result<(), FailKind>
where
    H: Handler + Send + 'static,
{
    let mut key_unreadable = false;
    for method in &spawn.auth {
        let accepted = match method {
            AuthMethod::Agent => agent(handle, &spawn.user).await,
            AuthMethod::Key => match &spawn.identity_file {
                Some(path) => match key(handle, &spawn.user, path).await {
                    KeyOutcome::Accepted => true,
                    KeyOutcome::Rejected => false,
                    KeyOutcome::Unreadable => {
                        key_unreadable = true;
                        false
                    }
                },
                // A chave só entra com `identity_file` (ADR-0072 §6).
                None => false,
            },
            // Prompt 13.
            AuthMethod::Password => false,
        };
        if accepted {
            return Ok(());
        }
    }
    Err(if key_unreadable {
        FailKind::KeyUnreadable
    } else {
        FailKind::AuthRejected
    })
}

enum KeyOutcome {
    Accepted,
    Rejected,
    Unreadable,
}

/// Chave de arquivo, **sem** passphrase.
async fn key<H>(handle: &mut Handle<H>, user: &str, path: &std::path::Path) -> KeyOutcome
where
    H: Handler + Send + 'static,
{
    // Cifrada, ausente ou ilegível dão o mesmo resultado.
    let Ok(secret) = load_secret_key(path, None) else {
        return KeyOutcome::Unreadable;
    };
    let hash = rsa_hash(handle, secret.algorithm()).await;
    let key = PrivateKeyWithHashAlg::new(Arc::new(secret), hash);
    match handle.authenticate_publickey(user, key).await {
        Ok(result) if result.success() => KeyOutcome::Accepted,
        _ => KeyOutcome::Rejected,
    }
}

async fn rsa_hash<H>(handle: &Handle<H>, algorithm: Algorithm) -> Option<russh::keys::HashAlg>
where
    H: Handler + Send + 'static,
{
    if !matches!(algorithm, Algorithm::Rsa { .. }) {
        return None;
    }
    tokio::time::timeout(RSA_HASH_WAIT, handle.best_supported_rsa_hash())
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
        .flatten()
}

/// Agente ausente é pulado sem erro (RF-20.9). Encaminhamento de agente nunca
/// é pedido (RF-20.36).
async fn agent<H>(handle: &mut Handle<H>, user: &str) -> bool
where
    H: Handler + Send + 'static,
{
    #[cfg(windows)]
    {
        if let Ok(client) = AgentClient::connect_named_pipe(OPENSSH_AGENT_PIPE).await
            && sign_with(handle, user, client).await
        {
            return true;
        }
        if let Ok(client) = AgentClient::connect_pageant().await
            && sign_with(handle, user, client).await
        {
            return true;
        }
        false
    }
    #[cfg(unix)]
    {
        match AgentClient::connect_env().await {
            Ok(client) => sign_with(handle, user, client).await,
            Err(_) => false,
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (handle, user);
        false
    }
}

async fn sign_with<H, S>(handle: &mut Handle<H>, user: &str, mut agent: AgentClient<S>) -> bool
where
    H: Handler + Send + 'static,
    S: russh::keys::agent::client::AgentStream + Unpin + Send + 'static,
{
    let Ok(identities) = agent.request_identities().await else {
        return false;
    };
    for identity in identities {
        let AgentIdentity::PublicKey { key, .. } = identity else {
            // Certificado de usuário: fora do escopo desta leva.
            continue;
        };
        let hash = rsa_hash(handle, key.algorithm()).await;
        match handle
            .authenticate_publickey_with(user, key, hash, &mut agent)
            .await
        {
            Ok(result) if result.success() => return true,
            Ok(_) => {}
            // O agente recusou assinar ou caiu: não adianta insistir nele.
            Err(_) => return false,
        }
    }
    false
}
