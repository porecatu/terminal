// SPDX-License-Identifier: GPL-3.0-or-later

//! O que sai do terminal além de pixels (docs/arquitetura.md seção 4.3).
//! `porecatu-term` nunca age sobre estes eventos -- só traduz e repassa;
//! `porecatu-ui` decide o que fazer com cada um.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

/// Evento traduzido de `alacritty_terminal::event::Event` -- nenhum tipo do
/// motor atravessa esta fronteira (mesma disciplina do snapshot, ADR-0002).
#[derive(Debug, Clone)]
pub enum TermEvent {
    /// OSC 0 / OSC 2. `None` = reset para o título default (RF-1.7).
    Title(Option<String>),
    /// BEL (RF-1.21).
    Bell,
    /// OSC 52, escrita para a área de transferência `Clipboard`. Já
    /// decodificado de base64 e dentro do teto de tamanho de
    /// `TermParams::clipboard_write_max_bytes` -- payloads maiores são
    /// descartados antes de chegar aqui. Escrita para a seleção `PRIMARY`
    /// nunca gera este evento: não é suportada (RF-10.9).
    ClipboardWrite(String),
    /// OSC 52, leitura. Só dispara quando `TermParams::osc52_read` permite
    /// -- por default o motor nem chega a emitir o evento (ADR-0013).
    ClipboardRead(ClipboardResponder),
    /// OSC 4 / 10 / 11, consulta de cor (frente, fundo, paleta indexada).
    ColorQuery(ColorQueryResponder),
    /// Fim do processo (RF-1.3). Não vem do motor VT -- vem de
    /// `porecatu-pty::PtyHandle::try_wait`, detectado por `Terminal`
    /// (Etapa 3) e injetado neste mesmo canal.
    Exit { success: bool, code: u32 },
    /// OSC 7 (ADR-0017 item 1). Captura própria de `porecatu-term`, fora do
    /// motor -- ver `crate::osc7`. Herdado por `tab.new`/`window.new`;
    /// `None` continua sendo o fallback de `startup_directory` até o
    /// primeiro OSC 7 chegar.
    Cwd(PathBuf),
    /// Marcador de dispensa do convite de integração de shell digitado
    /// pelo usuário (ADR-0039 §4). Captura própria, fora do motor -- ver
    /// `crate::dismiss`.
    ShellIntegrationDismiss,
}

/// Formata o conteúdo do clipboard (fornecido por quem responde, via
/// `porecatu-ui` → `arboard`) na sequência OSC 52 esperada pelo programa.
#[derive(Clone)]
pub struct ClipboardResponder(pub(crate) Arc<dyn Fn(&str) -> String + Send + Sync>);

impl ClipboardResponder {
    /// Devolve os bytes a escrever no PTY para responder à leitura.
    pub fn respond(&self, clipboard_content: &str) -> String {
        (self.0)(clipboard_content)
    }
}

impl fmt::Debug for ClipboardResponder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClipboardResponder(..)")
    }
}

/// Formata uma cor RGB resolvida (por `porecatu-ui`, a partir do tema) na
/// sequência OSC de resposta esperada pelo programa.
#[derive(Clone)]
pub struct ColorQueryResponder {
    pub(crate) index: usize,
    pub(crate) format: Arc<dyn Fn(u8, u8, u8) -> String + Send + Sync>,
}

impl ColorQueryResponder {
    /// Índice de cor consultado (0..256 = paleta, 256 = frente, 257 = fundo
    /// -- ver `alacritty_terminal::term::color::Colors`).
    pub fn index(&self) -> usize {
        self.index
    }

    /// Devolve os bytes a escrever no PTY para responder à consulta.
    pub fn respond(&self, r: u8, g: u8, b: u8) -> String {
        (self.format)(r, g, b)
    }
}

impl fmt::Debug for ColorQueryResponder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ColorQueryResponder({})", self.index)
    }
}
