// SPDX-License-Identifier: GPL-3.0-or-later

//! Autenticação, na ordem de `SshSpawn::auth`, parando no primeiro método
//! aceito (ADR-0072 §6, §7). Método fora da lista não é tentado.
//!
//! Senha, passphrase e keyboard-interactive de um campo oculto passam pelo
//! ciclo do segredo (`crate::cycle`, ADR-0073 §3): cofre, pergunta, gravação só
//! depois do aceite. Aqui está só o que fala com o servidor.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use russh::MethodKind;
use russh::client::{AuthResult, Handle, Handler, KeyboardInteractiveAuthResponse};
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::AgentClient;
use russh::keys::{Algorithm, PrivateKey, PrivateKeyWithHashAlg, load_secret_key};

use crate::ask::{Asker, SecretTarget};
use crate::cycle::{self, Attempt, Attempter, Ctx, Preflight, Settled};
use crate::end::FailKind;
use crate::secret::Secret;
use crate::spawn::{AuthMethod, SshSpawn};
use crate::vault::{self, Destination, Vault};

/// Espera pelo `server-sig-algs`, que decide o hash de uma chave RSA. Servidor
/// que não o manda não pode travar a autenticação.
const RSA_HASH_WAIT: Duration = Duration::from_secs(2);

/// Quantas mensagens vazias seguidas um keyboard-interactive pode mandar antes
/// de pedir o campo. Passou disso, o servidor está fazendo algo que não
/// atendemos.
const MAX_INFO_ROUNDS: usize = 4;

#[cfg(windows)]
const OPENSSH_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

/// De onde vêm as perguntas e o cofre.
pub(crate) struct Secrets {
    pub(crate) vault: Arc<dyn Vault>,
    pub(crate) asker: Arc<dyn Asker>,
}

/// `Ok` quando um método foi aceito.
pub(crate) async fn authenticate<H>(
    handle: &mut Handle<H>,
    spawn: &SshSpawn,
    secrets: &Secrets,
) -> Result<(), FailKind>
where
    H: Handler + Send + 'static,
{
    let mut key_unreadable = false;
    let mut unsupported = false;
    for method in &spawn.auth {
        let accepted = match method {
            AuthMethod::Agent => agent(handle, &spawn.user).await,
            AuthMethod::Key => match &spawn.identity_file {
                Some(path) => match key(handle, spawn, path, secrets).await? {
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
            AuthMethod::Password => match password(handle, spawn, secrets).await? {
                PasswordOutcome::Accepted => true,
                PasswordOutcome::NotAccepted => false,
                PasswordOutcome::Unsupported => {
                    unsupported = true;
                    false
                }
            },
        };
        if accepted {
            return Ok(());
        }
    }
    Err(if unsupported {
        FailKind::AuthUnsupported
    } else if key_unreadable {
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

/// Chave de arquivo. Cifrada, pede a passphrase pelo ciclo do segredo.
async fn key<H>(
    handle: &mut Handle<H>,
    spawn: &SshSpawn,
    path: &Path,
    secrets: &Secrets,
) -> Result<KeyOutcome, FailKind>
where
    H: Handler + Send + 'static,
{
    match load_secret_key(path, None) {
        Ok(secret) => Ok(offer(handle, &spawn.user, secret).await),
        Err(russh::keys::Error::KeyIsEncrypted) => {
            let ctx = Ctx {
                vault: &secrets.vault,
                asker: &secrets.asker,
                account: vault::passphrase_account(path),
                target: SecretTarget::KeyPassphrase {
                    path: path.to_owned(),
                },
            };
            let mut attempter = PassphraseAttempter {
                handle,
                user: &spawn.user,
                path,
            };
            Ok(match cycle::run(&ctx, &mut attempter).await? {
                Settled::Accepted => KeyOutcome::Accepted,
                Settled::Declined => KeyOutcome::Rejected,
            })
        }
        // Ausente, ilegível ou em formato que não conhecemos.
        Err(_) => Ok(KeyOutcome::Unreadable),
    }
}

/// Oferece a chave já decifrada ao servidor.
async fn offer<H>(handle: &mut Handle<H>, user: &str, secret: PrivateKey) -> KeyOutcome
where
    H: Handler + Send + 'static,
{
    let hash = rsa_hash(handle, secret.algorithm()).await;
    let key = PrivateKeyWithHashAlg::new(Arc::new(secret), hash);
    match handle.authenticate_publickey(user, key).await {
        Ok(result) if result.success() => KeyOutcome::Accepted,
        _ => KeyOutcome::Rejected,
    }
}

/// A passphrase de uma chave cifrada. "Recusada" é local e inequívoco: a chave
/// não decifra com ela (ADR-0073 §3).
struct PassphraseAttempter<'a, H: Handler> {
    handle: &'a mut Handle<H>,
    user: &'a str,
    path: &'a Path,
}

#[allow(
    clippy::manual_async_fn,
    reason = "the future must be `Send`: it runs on a multi-thread runtime, and `async fn` in a trait cannot say so"
)]
impl<H> Attempter for PassphraseAttempter<'_, H>
where
    H: Handler + Send + 'static,
{
    fn attempt<'a>(
        &'a mut self,
        secret: &'a Secret,
    ) -> impl std::future::Future<Output = Attempt> + Send + 'a {
        async move {
            // A derivação da chave é cara, mas curta; rodar aqui evita copiar
            // a passphrase para fora do `Secret`, o que a deixaria sem zerar.
            let Ok(key) = load_secret_key(self.path, Some(secret.expose())) else {
                return Attempt::Refused;
            };
            match offer(self.handle, self.user, key).await {
                KeyOutcome::Accepted => Attempt::Accepted,
                // A passphrase estava certa; o servidor é que não quer a chave.
                KeyOutcome::Rejected | KeyOutcome::Unreadable => Attempt::Declined,
            }
        }
    }
}

enum PasswordOutcome {
    Accepted,
    /// O servidor não oferece o método, ou o recusou sem que a senha tenha
    /// sido o problema: o próximo da lista segue.
    NotAccepted,
    /// Keyboard-interactive além de um campo oculto (RF-20.11).
    Unsupported,
}

/// Senha, por `password` ou -- quando é só o que o servidor oferece -- por
/// keyboard-interactive de um campo oculto (RF-20.10).
async fn password<H>(
    handle: &mut Handle<H>,
    spawn: &SshSpawn,
    secrets: &Secrets,
) -> Result<PasswordOutcome, FailKind>
where
    H: Handler + Send + 'static,
{
    // O que o servidor oferece, como o `ssh` descobre: um `none`.
    let methods = match handle.authenticate_none(&spawn.user).await {
        Ok(AuthResult::Success) => return Ok(PasswordOutcome::Accepted),
        Ok(AuthResult::Failure {
            remaining_methods, ..
        }) => remaining_methods,
        // Derrubou a conexão: não é sobre a senha.
        Err(_) => return Err(FailKind::Refused),
    };

    let ctx = Ctx {
        vault: &secrets.vault,
        asker: &secrets.asker,
        account: Destination::new(&spawn.user, &spawn.host, spawn.port).password_account(),
        target: SecretTarget::Password {
            user: spawn.user.clone(),
            host: spawn.host.clone(),
        },
    };
    let settled = if methods.contains(&MethodKind::Password) {
        let mut attempter = PasswordAttempter {
            handle,
            user: &spawn.user,
        };
        cycle::run(&ctx, &mut attempter).await
    } else if methods.contains(&MethodKind::KeyboardInteractive) {
        let mut attempter = KeyboardInteractiveAttempter {
            handle,
            user: &spawn.user,
            waiting_for_field: false,
        };
        cycle::run(&ctx, &mut attempter).await
    } else {
        return Ok(PasswordOutcome::NotAccepted);
    };
    match settled {
        Ok(Settled::Accepted) => Ok(PasswordOutcome::Accepted),
        Ok(Settled::Declined) => Ok(PasswordOutcome::NotAccepted),
        Err(FailKind::AuthUnsupported) => Ok(PasswordOutcome::Unsupported),
        Err(kind) => Err(kind),
    }
}

struct PasswordAttempter<'a, H: Handler> {
    handle: &'a mut Handle<H>,
    user: &'a str,
}

#[allow(
    clippy::manual_async_fn,
    reason = "the future must be `Send`: it runs on a multi-thread runtime, and `async fn` in a trait cannot say so"
)]
impl<H> Attempter for PasswordAttempter<'_, H>
where
    H: Handler + Send + 'static,
{
    fn attempt<'a>(
        &'a mut self,
        secret: &'a Secret,
    ) -> impl std::future::Future<Output = Attempt> + Send + 'a {
        async move {
            // O `russh` pede um `String`: esta cópia é dele, e fica fora do
            // alcance do `Secret` (ADR-0073 §5, "defesa em profundidade").
            match self
                .handle
                .authenticate_password(self.user, secret.expose())
                .await
            {
                Ok(AuthResult::Success) => Attempt::Accepted,
                Ok(AuthResult::Failure {
                    remaining_methods, ..
                }) => after_failure(&remaining_methods, MethodKind::Password),
                Err(_) => Attempt::Failed(FailKind::Refused),
            }
        }
    }
}

/// O que uma resposta de falha do servidor diz sobre o segredo (ADR-0073 §3).
/// Estrito: só vale como recusa da credencial se o **método segue oferecido** --
/// foi este segredo que não passou.
fn after_failure(remaining: &russh::MethodSet, method: MethodKind) -> Attempt {
    if remaining.contains(&method) {
        Attempt::Refused
    } else if remaining.is_empty() {
        // O `russh` entrega "nenhum método sobrando" quando a conexão cai no
        // meio da troca (`fail2ban`, `MaxAuthTries`): não é a senha, e a
        // entrada fica. Um servidor de verdade nunca fecha a lista vazia.
        Attempt::Failed(FailKind::Refused)
    } else {
        // Método retirado: a senha não foi o problema.
        Attempt::Declined
    }
}

/// Keyboard-interactive com **exatamente um** campo oculto, atendido como senha.
/// O texto do campo vem do servidor e **nunca** chega ao usuário (ADR-0075 §2).
struct KeyboardInteractiveAttempter<'a, H: Handler> {
    handle: &'a mut Handle<H>,
    user: &'a str,
    /// O servidor já mostrou o campo e espera a resposta.
    waiting_for_field: bool,
}

/// Onde a troca parou.
enum Stage {
    /// Um campo oculto espera a resposta.
    Field,
    /// A troca acabou.
    Over(Attempt),
}

impl<H> KeyboardInteractiveAttempter<'_, H>
where
    H: Handler + Send + 'static,
{
    /// Anda até o servidor mostrar um campo, ou acabar.
    async fn settle(
        &mut self,
        mut response: Result<KeyboardInteractiveAuthResponse, russh::Error>,
    ) -> Stage {
        for _ in 0..MAX_INFO_ROUNDS {
            match response {
                Err(_) => return Stage::Over(Attempt::Failed(FailKind::Refused)),
                Ok(KeyboardInteractiveAuthResponse::Success) => {
                    return Stage::Over(Attempt::Accepted);
                }
                Ok(KeyboardInteractiveAuthResponse::Failure {
                    remaining_methods, ..
                }) => {
                    return Stage::Over(after_failure(
                        &remaining_methods,
                        MethodKind::KeyboardInteractive,
                    ));
                }
                Ok(KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. }) => {
                    match prompts.as_slice() {
                        // Só um texto informativo: acusa o recebimento.
                        [] => {
                            response = self
                                .handle
                                .authenticate_keyboard_interactive_respond(Vec::new())
                                .await;
                        }
                        [field] if !field.echo => return Stage::Field,
                        // Campo visível, ou mais de um: segundo fator, ou algo
                        // que não é senha (RF-20.11).
                        _ => return Stage::Over(Attempt::Failed(FailKind::AuthUnsupported)),
                    }
                }
            }
        }
        Stage::Over(Attempt::Failed(FailKind::AuthUnsupported))
    }

    async fn begin(&mut self) -> Stage {
        let response = self
            .handle
            .authenticate_keyboard_interactive_start(self.user, None)
            .await;
        self.settle(response).await
    }
}

#[allow(
    clippy::manual_async_fn,
    reason = "the future must be `Send`: it runs on a multi-thread runtime, and `async fn` in a trait cannot say so"
)]
impl<H> Attempter for KeyboardInteractiveAttempter<'_, H>
where
    H: Handler + Send + 'static,
{
    /// Começa a troca e olha o primeiro campo **antes** de pedir a senha: não
    /// adianta perguntar a quem vai ser barrado por um segundo fator.
    fn preflight(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Preflight, FailKind>> + Send {
        async move {
            match self.begin().await {
                Stage::Field => {
                    self.waiting_for_field = true;
                    Ok(Preflight::Ask)
                }
                Stage::Over(Attempt::Accepted) => Ok(Preflight::Done(Settled::Accepted)),
                Stage::Over(Attempt::Failed(kind)) => Err(kind),
                // Recusou ou retirou o método sem pedir nada.
                Stage::Over(Attempt::Refused | Attempt::Declined) => {
                    Ok(Preflight::Done(Settled::Declined))
                }
            }
        }
    }

    fn attempt<'a>(
        &'a mut self,
        secret: &'a Secret,
    ) -> impl std::future::Future<Output = Attempt> + Send + 'a {
        async move {
            if !self.waiting_for_field {
                match self.begin().await {
                    Stage::Field => {}
                    Stage::Over(over) => return over,
                }
            }
            self.waiting_for_field = false;
            let response = self
                .handle
                .authenticate_keyboard_interactive_respond(vec![secret.expose().to_owned()])
                .await;
            match self.settle(response).await {
                // Pedir outro campo depois da senha é segundo fator, e recusar
                // aqui não é recusa da credencial: a entrada fica (RF-20.11).
                Stage::Field => Attempt::Failed(FailKind::AuthUnsupported),
                Stage::Over(over) => over,
            }
        }
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
