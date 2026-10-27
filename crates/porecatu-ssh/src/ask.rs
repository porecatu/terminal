// SPDX-License-Identifier: GPL-3.0-or-later

//! Como este crate pergunta algo ao usuário, sem conhecer a UI nem
//! `porecatu-term` (ADR-0071 §5, ADR-0072 §4).
//!
//! O ADR escreve `open(spawn, prompts: PromptSink)`, mas `PromptSink` é um tipo
//! de `porecatu-term`, que depende deste crate: o tipo da pergunta tem de
//! morar **aqui**, e `porecatu-term` o adapta ao seu `PromptSink`.
//!
//! A resposta chega por um *callback*, e não por um `Receiver`, para que a
//! tarefa async a espere num `oneshot` sem ocupar uma thread do runtime
//! enquanto o usuário decide.

/// Uma pergunta ao usuário.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    /// Host sem chave registrada (RF-20.16).
    HostKey(HostKeyQuestion),
}

/// O que o diálogo de host desconhecido mostra (ADR-0074 §3, §4). Tudo é dado,
/// não frase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyQuestion {
    pub host: String,
    pub port: u16,
    /// `ssh-ed25519`, `ecdsa-sha2-nistp256`, `rsa-sha2-512`...
    pub algorithm: String,
    /// `SHA256:` e o hash em base64 sem preenchimento.
    pub fingerprint: String,
    /// Tipos de chave que o host já tem registrados, quando tem: o diálogo
    /// avisa que não é um host nunca visto.
    pub other_types: Vec<String>,
    /// Linhas do `known_hosts` que o leitor não entendeu.
    pub skipped_lines: usize,
}

/// A resposta do usuário.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Confirm,
    Cancel,
}

/// Quem faz a pergunta chegar ao usuário.
pub trait Asker: Send + Sync + 'static {
    /// Abre a pergunta e **volta na hora**. `on_reply` é chamado uma vez,
    /// de qualquer thread, quando o usuário responde -- com `Cancel` se a
    /// pergunta foi abandonada (a aba fechou). Se for descartado sem ser
    /// chamado, quem perguntou entende como `Cancel`.
    fn ask(&self, question: Question, on_reply: Box<dyn FnOnce(Reply) + Send>);
}
