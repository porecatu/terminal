// SPDX-License-Identifier: GPL-3.0-or-later

//! Struct de parâmetros do próprio `porecatu-term` (docs/arquitetura.md
//! seção 4.2). `porecatu-config` não existe ainda nesta fase e, mesmo
//! quando existir, este crate nunca vai importá-lo -- `porecatu-ui` lê
//! `Config` e monta este struct. Hot reload é o mesmo caminho: `ui` remonta
//! os parâmetros e reaplica.

use alacritty_terminal::term::SEMANTIC_ESCAPE_CHARS;

/// Parâmetros que `TermEngine::new` precisa e que vêm de `[scrollback]`,
/// `[selection]` e `[terminal.clipboard]` na config do usuário.
#[derive(Debug, Clone)]
pub struct TermParams {
    /// `scrollback.lines`.
    pub scrollback_lines: usize,
    /// `selection.word_separators` -- caracteres que terminam seleção
    /// semântica (duplo clique).
    pub word_separators: String,
    /// `terminal.clipboard.osc52_read` -- default `false` (ADR-0013: negado
    /// por default, processo remoto lendo o clipboard local é vetor de
    /// exfiltração).
    pub osc52_read: bool,
    /// `terminal.clipboard.osc52_write` -- default `true` (ADR-0013).
    pub osc52_write: bool,
    /// Teto de tamanho do payload de escrita OSC 52, em bytes. Sem limite,
    /// uma sequência vinda de saída não confiável escreve megabytes no
    /// clipboard do usuário (ADR-0013).
    pub clipboard_write_max_bytes: usize,
}

impl Default for TermParams {
    fn default() -> Self {
        Self {
            // Mesmo default do alacritty_terminal; o default real do produto
            // vem de `porecatu-config` na F4 (`[scrollback] lines`).
            scrollback_lines: 10_000,
            word_separators: SEMANTIC_ESCAPE_CHARS.to_owned(),
            osc52_read: false,
            osc52_write: true,
            // 1 MiB. Valor de trabalho para a F1; o teto real vem de
            // `porecatu-config` na F4 (`[terminal.clipboard]`).
            clipboard_write_max_bytes: 1 << 20,
        }
    }
}
