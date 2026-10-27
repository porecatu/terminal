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

use crate::secret::Secret;

/// Uma pergunta ao usuário.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    /// Host sem chave registrada (RF-20.16).
    HostKey(HostKeyQuestion),
    /// Senha ou passphrase (RF-20.12).
    Secret(SecretQuestion),
}

/// De quem é o segredo pedido. Tipado, sem prosa: a frase é da UI. Nunca leva
/// o texto que o servidor mandou num keyboard-interactive (ADR-0075 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretTarget {
    /// Senha de `usuário@host`.
    Password { user: String, host: String },
    /// Passphrase do arquivo de chave.
    KeyPassphrase { path: std::path::PathBuf },
}

/// O que o diálogo de segredo mostra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretQuestion {
    pub target: SecretTarget,
    /// O cofre do sistema responde: a caixa "Lembrar" só faz sentido então
    /// (RF-20.14).
    pub vault_available: bool,
    /// O segredo anterior foi recusado: o diálogo mostra a linha de erro.
    pub retry: bool,
}

/// Algo que o app deve dizer ao usuário fora de qualquer pergunta (canal 1 do
/// ADR-0014). Tipado, sem texto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// A conexão foi aceita, mas o segredo não pôde ser gravado no cofre
    /// (ADR-0073 §3).
    SecretNotSaved,
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

/// A resposta do usuário. Sem `Clone`: pode carregar um segredo.
#[derive(Debug)]
pub enum Reply {
    /// Confiar no host.
    Confirm,
    Cancel,
    /// O segredo digitado, e se o usuário quer lembrá-lo no cofre.
    Secret {
        value: Secret,
        remember: bool,
    },
}

/// Quem faz a pergunta chegar ao usuário.
pub trait Asker: Send + Sync + 'static {
    /// Abre a pergunta e **volta na hora**. `on_reply` é chamado uma vez,
    /// de qualquer thread, quando o usuário responde -- com `Cancel` se a
    /// pergunta foi abandonada (a aba fechou). Se for descartado sem ser
    /// chamado, quem perguntou entende como `Cancel`.
    fn ask(&self, question: Question, on_reply: Box<dyn FnOnce(Reply) + Send>);

    /// Avisa o usuário de algo que não é pergunta. Quem não tem onde mostrar
    /// ignora.
    fn notice(&self, _notice: Notice) {}
}
