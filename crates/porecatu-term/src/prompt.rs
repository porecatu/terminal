// SPDX-License-Identifier: GPL-3.0-or-later

//! Perguntas do backend à UI (ADR-0071 §5). O backend pergunta por um
//! [`PromptSink`], a UI recebe `TermEvent::Prompt` no wakeup e responde por
//! `Terminal::answer`. Quem espera a resposta é o backend, no contexto de
//! execução dele: nunca a thread de leitura do motor e nunca a main thread.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::event::TermEvent;

/// Identifica uma pergunta aberta, por `Terminal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PromptId(u64);

impl PromptId {
    /// Para quem monta uma [`Prompt`] à mão (testes de `porecatu-ui`): os
    /// identificadores de verdade vêm do `PromptSink`. Responder a um
    /// identificador que nenhuma pergunta aberta tem é ignorado.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }
}

/// Texto secreto digitado pelo usuário (senha, passphrase).
///
/// **Provisório:** o prompt 13 move este tipo para `porecatu-ssh` com
/// `zeroize::Zeroizing<String>` (ADR-0073 §5). Já nasce com a forma final: sem
/// `Clone`, sem `Display`, `Debug` opaco -- e atravessa `porecatu-term` por
/// movimento, sem ser guardado.
pub struct Secret(String);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// O texto, para quem vai usá-lo (a autenticação). Não copiar.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// De quem é o segredo pedido. Tipado, sem prosa: a frase é de `porecatu-ui`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretLabel {
    /// Senha de `usuário@host`.
    Password { user: String, host: String },
    /// Passphrase do arquivo de chave.
    KeyPassphrase { path: PathBuf },
}

/// O que se pergunta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
    /// Um segredo. `vault_available` diz se a caixa "Lembrar" faz sentido.
    Secret {
        label: SecretLabel,
        vault_available: bool,
    },
    /// Chave de host desconhecida (RF-20.16).
    HostKey {
        algorithm: String,
        fingerprint: String,
        /// Tipos de chave que o `known_hosts` já tem para este host.
        known_types: Vec<String>,
        /// Linhas do `known_hosts` que o leitor não entendeu: o diálogo
        /// avisa quantas (ADR-0074 §2). Um arquivo que existe e não pôde ser
        /// lido nunca chega a virar pergunta -- encerra a conexão.
        skipped_lines: usize,
    },
}

/// Uma pergunta aberta, com o identificador para responder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub id: PromptId,
    pub kind: PromptKind,
}

/// A resposta da UI.
#[derive(Debug)]
pub enum Answer {
    /// Segredo digitado, e se o usuário pediu para lembrá-lo no cofre.
    Secret { value: Secret, remember: bool },
    /// Confirmação (chave de host aceita).
    Confirm,
    /// Cancelou -- ou a aba fechou com a pergunta aberta.
    Cancel,
}

/// Perguntas abertas de um `Terminal`, e o contador de identificadores.
/// Quem recebe a resposta de uma pergunta aberta.
type Responder = Box<dyn FnOnce(Answer) + Send>;

#[derive(Default)]
struct PromptTable {
    next: AtomicU64,
    state: Mutex<TableState>,
}

#[derive(Default)]
struct TableState {
    open: HashMap<PromptId, Responder>,
    /// Depois de fechada, toda pergunta nova já nasce cancelada.
    closed: bool,
}

impl PromptTable {
    fn state(&self) -> std::sync::MutexGuard<'_, TableState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Como o backend pergunta. Barato de clonar; vai para dentro do backend.
#[derive(Clone)]
pub struct PromptSink {
    table: Arc<PromptTable>,
    events: mpsc::Sender<TermEvent>,
    on_wakeup: Arc<dyn Fn() + Send + Sync>,
}

impl PromptSink {
    /// Abre uma pergunta e devolve o canal da resposta. Não bloqueia: o
    /// backend decide como esperar (`recv` numa thread, ou uma ponte para a
    /// tarefa async). Sempre chega **uma** resposta -- `Cancel`, se o
    /// `Terminal` foi embora ou trocou de backend antes de a UI responder.
    pub fn ask(&self, kind: PromptKind) -> mpsc::Receiver<Answer> {
        let (tx, rx) = mpsc::channel();
        self.ask_with(kind, move |answer| {
            let _ = tx.send(answer);
        });
        rx
    }

    /// Como [`ask`](PromptSink::ask), mas a resposta chega a `on_answer` --
    /// de qualquer thread, uma vez --, em vez de a um `Receiver`. É o que
    /// deixa uma tarefa async esperar num `oneshot` sem ocupar uma thread
    /// enquanto o usuário decide (ADR-0072 §4). `on_answer` deve ser rápido e
    /// não bloquear: roda na thread de quem responde (a main thread, em
    /// `Terminal::answer`) ou na de quem fecha a aba.
    pub fn ask_with(&self, kind: PromptKind, on_answer: impl FnOnce(Answer) + Send + 'static) {
        let id = PromptId(self.table.next.fetch_add(1, Ordering::Relaxed));
        {
            let mut state = self.table.state();
            if state.closed {
                drop(state);
                on_answer(Answer::Cancel);
                return;
            }
            state.open.insert(id, Box::new(on_answer));
        }
        let _ = self.events.send(TermEvent::Prompt(Prompt { id, kind }));
        (self.on_wakeup)();
    }
}

/// Dono das perguntas de uma sessão de backend: quando cai (o `Terminal`
/// foi dropado, ou o backend foi trocado), cancela as que seguem abertas.
pub(crate) struct PromptGuard {
    table: Arc<PromptTable>,
}

impl PromptGuard {
    pub(crate) fn new() -> Self {
        Self {
            table: Arc::new(PromptTable::default()),
        }
    }

    pub(crate) fn sink(
        &self,
        events: mpsc::Sender<TermEvent>,
        on_wakeup: Arc<dyn Fn() + Send + Sync>,
    ) -> PromptSink {
        PromptSink {
            table: Arc::clone(&self.table),
            events,
            on_wakeup,
        }
    }

    /// Entrega a resposta. Identificador desconhecido ou já respondido é
    /// ignorado.
    pub(crate) fn answer(&self, id: PromptId, answer: Answer) {
        let responder = self.table.state().open.remove(&id);
        if let Some(responder) = responder {
            responder(answer);
        }
    }
}

impl Drop for PromptGuard {
    fn drop(&mut self) {
        let open = {
            let mut state = self.table.state();
            state.closed = true;
            std::mem::take(&mut state.open)
        };
        for (_, responder) in open {
            responder(Answer::Cancel);
        }
    }
}
