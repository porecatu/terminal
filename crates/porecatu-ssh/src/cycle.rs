// SPDX-License-Identifier: GPL-3.0-or-later

//! O ciclo de vida de um segredo, todo dentro da conexão (ADR-0073 §3):
//!
//! 1. lê a entrada do cofre; achou, tenta sem perguntar;
//! 2. se o servidor **recusou o segredo como credencial**, apaga a entrada e
//!    pergunta -- qualquer outra falha (conexão derrubada, método retirado,
//!    tempo esgotado) **mantém** a entrada;
//! 3. se o usuário marcou "Lembrar", o segredo fica **em memória** e só vai
//!    para o cofre depois de o servidor aceitá-lo;
//! 4. falha ao gravar vira aviso do app, sem derrubar a conexão; falha ao ler
//!    vale como entrada ausente.
//!
//! Aqui não há `russh`: quem fala com o servidor é um [`Attempter`], o que deixa
//! o ciclo inteiro testável com um cofre e um servidor de mentira.

use std::future::Future;
use std::sync::Arc;

use crate::ask::{Asker, Notice, Question, Reply, SecretQuestion, SecretTarget};
use crate::end::FailKind;
use crate::secret::Secret;
use crate::vault::Vault;

/// O que o servidor respondeu a uma tentativa com um segredo.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Attempt {
    /// A autenticação passou.
    Accepted,
    /// O servidor recusou **este** segredo e continua oferecendo o método -- ou
    /// a chave não decifra com esta passphrase. É o único caso que apaga uma
    /// entrada salva e que reabre o diálogo.
    Refused,
    /// O segredo está certo, mas o método não levou à autenticação (a chave
    /// decifrou e o servidor não a aceita; o método foi retirado da lista).
    /// Nada se apaga, nada se grava, e o próximo método da lista segue.
    Declined,
    /// A tentativa não pôde terminar: conexão derrubada, resposta inesperada,
    /// método sem suporte. Encerra com a causa, e a entrada fica como estava.
    Failed(FailKind),
}

/// Como o ciclo terminou quando não falhou.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Settled {
    Accepted,
    Declined,
}

/// O que o servidor mostrou antes de qualquer segredo.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Preflight {
    /// Pede um segredo: siga com o ciclo.
    Ask,
    /// Já terminou sem pedir nada (aceitou sem campo, ou nem oferece o método).
    Done(Settled),
}

/// Quem tenta o segredo no servidor.
pub(crate) trait Attempter {
    /// Antes de ler o cofre e de perguntar qualquer coisa: o servidor pede algo
    /// que um único campo oculto atende? Não adianta pedir uma senha para um
    /// servidor que vai exigir um segundo fator.
    fn preflight(&mut self) -> impl Future<Output = Result<Preflight, FailKind>> + Send {
        async { Ok(Preflight::Ask) }
    }

    fn attempt<'a>(&'a mut self, secret: &'a Secret) -> impl Future<Output = Attempt> + Send + 'a;
}

/// Tudo que o ciclo precisa saber do que está pedindo.
pub(crate) struct Ctx<'a> {
    pub(crate) vault: &'a Arc<dyn Vault>,
    pub(crate) asker: &'a Arc<dyn Asker>,
    /// A conta da entrada no cofre (`vault::passphrase_account`, ...).
    pub(crate) account: String,
    pub(crate) target: SecretTarget,
}

pub(crate) async fn run<A: Attempter>(
    ctx: &Ctx<'_>,
    attempter: &mut A,
) -> Result<Settled, FailKind> {
    if let Preflight::Done(settled) = attempter.preflight().await? {
        return Ok(settled);
    }

    let mut retry = false;
    if let Some(saved) = read_saved(ctx).await {
        match attempter.attempt(&saved).await {
            Attempt::Accepted => return Ok(Settled::Accepted),
            Attempt::Declined => return Ok(Settled::Declined),
            Attempt::Failed(kind) => return Err(kind),
            // Um segredo salvo que não funciona mais não pode ficar sendo
            // tentado a cada conexão (RF-20.13). Quem o recusou foi o servidor,
            // e o usuário precisa saber por que está sendo perguntado.
            Attempt::Refused => {
                forget_saved(ctx).await;
                retry = true;
            }
        }
    }

    let available = vault_available(ctx).await;
    loop {
        match ask(ctx, available, retry).await {
            Reply::Secret { value, remember } => match attempter.attempt(&value).await {
                Attempt::Accepted => {
                    if remember && available {
                        save(ctx, value).await;
                    }
                    return Ok(Settled::Accepted);
                }
                Attempt::Refused => retry = true,
                Attempt::Declined => return Ok(Settled::Declined),
                Attempt::Failed(kind) => return Err(kind),
            },
            Reply::Confirm | Reply::Cancel => return Err(FailKind::AuthCancelled),
        }
    }
}

/// Falha ao ler vale como entrada ausente (ADR-0073 §3).
async fn read_saved(ctx: &Ctx<'_>) -> Option<Secret> {
    let (vault, account) = (Arc::clone(ctx.vault), ctx.account.clone());
    tokio::task::spawn_blocking(move || vault.read(&account))
        .await
        .ok()?
        .ok()?
}

/// Não conseguir apagar não muda o que acontece: o diálogo abre de qualquer
/// jeito.
async fn forget_saved(ctx: &Ctx<'_>) {
    let (vault, account) = (Arc::clone(ctx.vault), ctx.account.clone());
    let _ = tokio::task::spawn_blocking(move || vault.delete(&account)).await;
}

async fn vault_available(ctx: &Ctx<'_>) -> bool {
    let vault = Arc::clone(ctx.vault);
    tokio::task::spawn_blocking(move || vault.available())
        .await
        .unwrap_or(false)
}

/// Grava depois de o servidor aceitar. A conexão já foi aceita: falhar aqui só
/// vira aviso. O segredo morre (e é zerado) ao fim desta função.
async fn save(ctx: &Ctx<'_>, secret: Secret) {
    let (vault, account) = (Arc::clone(ctx.vault), ctx.account.clone());
    let written = tokio::task::spawn_blocking(move || vault.write(&account, &secret)).await;
    if !matches!(written, Ok(Ok(()))) {
        ctx.asker.notice(Notice::SecretNotSaved);
    }
}

/// Pergunta e espera num `oneshot`, sem ocupar uma thread do runtime. Pergunta
/// descartada é `Cancel`.
async fn ask(ctx: &Ctx<'_>, vault_available: bool, retry: bool) -> Reply {
    let (tx, rx) = tokio::sync::oneshot::channel();
    ctx.asker.ask(
        Question::Secret(SecretQuestion {
            target: ctx.target.clone(),
            vault_available,
            retry,
        }),
        Box::new(move |reply| {
            let _ = tx.send(reply);
        }),
    );
    rx.await.unwrap_or(Reply::Cancel)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use tokio::runtime::Runtime;

    use super::*;
    use crate::vault::fake::MemoryVault;

    const ACCOUNT: &str = "password:me@host:22";

    fn target() -> SecretTarget {
        SecretTarget::Password {
            user: "me".to_owned(),
            host: "host".to_owned(),
        }
    }

    /// O usuário de mentira: responde na ordem e guarda o que viu.
    #[derive(Default)]
    struct FakeUser {
        replies: Mutex<VecDeque<Reply>>,
        asked: Mutex<Vec<SecretQuestion>>,
        notices: Mutex<Vec<Notice>>,
    }

    impl FakeUser {
        fn saying(replies: Vec<Reply>) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(replies.into()),
                ..Self::default()
            })
        }

        fn never_asked() -> Arc<Self> {
            Self::saying(Vec::new())
        }

        fn asked(&self) -> Vec<SecretQuestion> {
            self.asked.lock().unwrap().clone()
        }

        fn notices(&self) -> Vec<Notice> {
            self.notices.lock().unwrap().clone()
        }
    }

    impl Asker for FakeUser {
        fn ask(&self, question: Question, on_reply: Box<dyn FnOnce(Reply) + Send>) {
            let Question::Secret(question) = question else {
                panic!("only secrets are asked here");
            };
            self.asked.lock().unwrap().push(question);
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("the test did not expect another question");
            on_reply(reply);
        }

        fn notice(&self, notice: Notice) {
            self.notices.lock().unwrap().push(notice);
        }
    }

    fn typed(text: &str, remember: bool) -> Reply {
        Reply::Secret {
            value: Secret::new(text.to_owned()),
            remember,
        }
    }

    /// O servidor de mentira: o que responder a cada segredo.
    struct FakeServer<F> {
        answer: F,
        preflight: Option<FailKind>,
        tried: Vec<String>,
    }

    impl<F: FnMut(&str) -> Attempt + Send> FakeServer<F> {
        fn new(answer: F) -> Self {
            Self {
                answer,
                preflight: None,
                tried: Vec::new(),
            }
        }
    }

    impl<F: FnMut(&str) -> Attempt + Send> Attempter for FakeServer<F> {
        fn preflight(&mut self) -> impl Future<Output = Result<Preflight, FailKind>> + Send {
            let result = self.preflight.clone().map_or(Ok(Preflight::Ask), Err);
            async move { result }
        }

        fn attempt<'a>(
            &'a mut self,
            secret: &'a Secret,
        ) -> impl Future<Output = Attempt> + Send + 'a {
            self.tried.push(secret.expose().to_owned());
            let outcome = (self.answer)(secret.expose());
            async move { outcome }
        }
    }

    fn run_cycle<A: Attempter>(
        vault: &Arc<MemoryVault>,
        user: &Arc<FakeUser>,
        server: &mut A,
    ) -> Result<Settled, FailKind> {
        let vault: Arc<dyn Vault> = vault.clone();
        let asker: Arc<dyn Asker> = user.clone();
        let ctx = Ctx {
            vault: &vault,
            asker: &asker,
            account: ACCOUNT.to_owned(),
            target: target(),
        };
        Runtime::new().unwrap().block_on(run(&ctx, server))
    }

    fn accepts(good: &'static str) -> impl FnMut(&str) -> Attempt + Send {
        move |tried| {
            if tried == good {
                Attempt::Accepted
            } else {
                Attempt::Refused
            }
        }
    }

    #[test]
    fn a_saved_secret_the_server_accepts_is_used_without_asking() {
        let vault = Arc::new(MemoryVault::with(ACCOUNT, "saved"));
        let user = FakeUser::never_asked();
        let mut server = FakeServer::new(accepts("saved"));

        let settled = run_cycle(&vault, &user, &mut server);

        assert_eq!(settled, Ok(Settled::Accepted));
        assert_eq!(server.tried, ["saved"]);
        assert!(user.asked().is_empty());
        assert!(vault.deleted.lock().unwrap().is_empty());
        assert!(vault.written.lock().unwrap().is_empty());
        assert_eq!(vault.stored(ACCOUNT).as_deref(), Some("saved"));
    }

    #[test]
    fn a_saved_secret_the_server_refuses_is_deleted_and_then_asked() {
        let vault = Arc::new(MemoryVault::with(ACCOUNT, "stale"));
        let user = FakeUser::saying(vec![typed("fresh", false)]);
        let mut server = FakeServer::new(accepts("fresh"));

        let settled = run_cycle(&vault, &user, &mut server);

        assert_eq!(settled, Ok(Settled::Accepted));
        assert_eq!(server.tried, ["stale", "fresh"]);
        assert_eq!(*vault.deleted.lock().unwrap(), [ACCOUNT]);
        assert!(
            vault.is_empty(),
            "nothing remembered: the box was not ticked"
        );
        let asked = user.asked();
        assert_eq!(asked.len(), 1);
        assert!(asked[0].retry, "the dialog says the server refused");
        assert_eq!(asked[0].target, target());
    }

    #[test]
    fn a_connection_dropped_mid_authentication_keeps_the_saved_secret() {
        let vault = Arc::new(MemoryVault::with(ACCOUNT, "valid"));
        let user = FakeUser::never_asked();
        let mut server = FakeServer::new(|_| Attempt::Failed(FailKind::Refused));

        let settled = run_cycle(&vault, &user, &mut server);

        assert_eq!(settled, Err(FailKind::Refused));
        assert!(vault.deleted.lock().unwrap().is_empty());
        assert_eq!(vault.stored(ACCOUNT).as_deref(), Some("valid"));
        assert!(
            user.asked().is_empty(),
            "the user is not asked about a drop"
        );
    }

    #[test]
    fn a_method_the_server_withdrew_keeps_the_saved_secret_and_moves_on() {
        let vault = Arc::new(MemoryVault::with(ACCOUNT, "valid"));
        let user = FakeUser::never_asked();
        let mut server = FakeServer::new(|_| Attempt::Declined);

        let settled = run_cycle(&vault, &user, &mut server);

        assert_eq!(settled, Ok(Settled::Declined));
        assert!(vault.deleted.lock().unwrap().is_empty());
        assert_eq!(vault.stored(ACCOUNT).as_deref(), Some("valid"));
    }

    #[test]
    fn remember_with_the_wrong_secret_writes_nothing_and_asks_again_with_the_error() {
        let vault = Arc::new(MemoryVault::default());
        let user = FakeUser::saying(vec![typed("wrong", true), Reply::Cancel]);
        let mut server = FakeServer::new(accepts("right"));

        let settled = run_cycle(&vault, &user, &mut server);

        assert_eq!(settled, Err(FailKind::AuthCancelled));
        assert!(vault.is_empty());
        assert!(vault.written.lock().unwrap().is_empty());
        let retries: Vec<bool> = user.asked().iter().map(|q| q.retry).collect();
        assert_eq!(retries, [false, true]);
    }

    #[test]
    fn remember_with_the_right_secret_writes_only_after_the_server_accepts() {
        let vault = Arc::new(MemoryVault::default());
        let user = FakeUser::saying(vec![typed("wrong", true), typed("right", true)]);
        let seen_by_server = Arc::clone(&vault);
        let mut server = FakeServer::new(move |tried| {
            assert!(
                seen_by_server.is_empty(),
                "nothing is written before the server answers: {tried}"
            );
            if tried == "right" {
                Attempt::Accepted
            } else {
                Attempt::Refused
            }
        });

        let settled = run_cycle(&vault, &user, &mut server);

        assert_eq!(settled, Ok(Settled::Accepted));
        assert_eq!(server.tried, ["wrong", "right"]);
        assert_eq!(*vault.written.lock().unwrap(), [ACCOUNT]);
        assert_eq!(vault.stored(ACCOUNT).as_deref(), Some("right"));
        assert!(user.notices().is_empty());
    }

    #[test]
    fn without_a_vault_nothing_is_remembered_and_the_dialog_is_told() {
        let vault = Arc::new(MemoryVault::unavailable());
        // Mesmo que a caixa chegue marcada, sem cofre não se grava.
        let user = FakeUser::saying(vec![typed("right", true)]);
        let mut server = FakeServer::new(accepts("right"));

        let settled = run_cycle(&vault, &user, &mut server);

        assert_eq!(settled, Ok(Settled::Accepted));
        assert!(!user.asked()[0].vault_available);
        assert!(vault.written.lock().unwrap().is_empty());
        assert!(user.notices().is_empty(), "no vault is not a write failure");
    }

    #[test]
    fn a_write_failure_after_acceptance_is_a_notice_and_not_a_failure() {
        let vault = Arc::new(MemoryVault::default());
        *vault.write_error.lock().unwrap() = Some(crate::vault::VaultError::Denied);
        let user = FakeUser::saying(vec![typed("right", true)]);
        let mut server = FakeServer::new(accepts("right"));

        let settled = run_cycle(&vault, &user, &mut server);

        assert_eq!(settled, Ok(Settled::Accepted));
        assert_eq!(user.notices(), [Notice::SecretNotSaved]);
        assert!(vault.is_empty());
    }

    #[test]
    fn the_dialog_is_told_the_vault_is_there_when_it_is() {
        let vault = Arc::new(MemoryVault::default());
        let user = FakeUser::saying(vec![typed("right", false)]);
        let mut server = FakeServer::new(accepts("right"));

        run_cycle(&vault, &user, &mut server).unwrap();

        let asked = user.asked();
        assert!(asked[0].vault_available);
        assert!(!asked[0].retry);
    }

    #[test]
    fn cancelling_the_dialog_ends_the_attempt_as_cancelled() {
        let vault = Arc::new(MemoryVault::default());
        let user = FakeUser::saying(vec![Reply::Cancel]);
        let mut server = FakeServer::new(accepts("right"));

        assert_eq!(
            run_cycle(&vault, &user, &mut server),
            Err(FailKind::AuthCancelled)
        );
        assert!(server.tried.is_empty());
    }

    #[test]
    fn a_server_that_cannot_be_served_fails_before_the_cycle_asks_or_reads() {
        let vault = Arc::new(MemoryVault::with(ACCOUNT, "saved"));
        let user = FakeUser::never_asked();
        let mut server = FakeServer::new(accepts("saved"));
        server.preflight = Some(FailKind::AuthUnsupported);

        let settled = run_cycle(&vault, &user, &mut server);

        assert_eq!(settled, Err(FailKind::AuthUnsupported));
        assert!(server.tried.is_empty(), "nothing was tried");
        assert!(user.asked().is_empty());
    }

    #[test]
    fn a_read_failure_counts_as_no_entry() {
        // Cofre indisponível: a leitura falha e o usuário é perguntado.
        let vault = Arc::new(MemoryVault::unavailable());
        let user = FakeUser::saying(vec![typed("right", false)]);
        let mut server = FakeServer::new(accepts("right"));

        assert_eq!(run_cycle(&vault, &user, &mut server), Ok(Settled::Accepted));
        assert_eq!(server.tried, ["right"]);
    }
}
