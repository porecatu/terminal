// SPDX-License-Identifier: GPL-3.0-or-later

//! Struct de parâmetros do próprio `porecatu-term` (docs/arquitetura.md
//! seção 4.2). `porecatu-config` não existe ainda nesta fase e, mesmo
//! quando existir, este crate nunca vai importá-lo -- `porecatu-ui` lê
//! `Config` e monta este struct. Hot reload é o mesmo caminho: `ui` remonta
//! os parâmetros e reaplica.

use alacritty_terminal::term::SEMANTIC_ESCAPE_CHARS;

use crate::snapshot::CursorShape;

/// Parâmetros que `TermEngine::new` precisa e que vêm de `[scrollback]`,
/// `[selection]`, `[terminal.cursor]` e `[terminal.clipboard]` na config do
/// usuário.
#[derive(Debug, Clone)]
pub struct TermParams {
    /// `scrollback.lines`.
    pub scrollback_lines: usize,
    /// `selection.word_separators` -- caracteres que terminam seleção
    /// semântica (duplo clique).
    pub word_separators: String,
    /// `terminal.cursor.shape` -- forma que o motor reseta para quando
    /// nenhum DECSCUSR está em vigor (RF-5.22). `HollowBlock`/`Hidden` não
    /// são formas de config (RF-5.22 só lista block/beam/underline); ficam
    /// no enum porque é o mesmo `CursorShape` do snapshot, que precisa
    /// representar as duas (vazado por DECSCUSR e por `unfocused_hollow`,
    /// este resolvido em `porecatu-ui`, não aqui).
    pub default_cursor_shape: CursorShape,
    /// `terminal.cursor.blink` -- default do motor; programa que emite
    /// DECSCUSR com o bit de blink tem precedência enquanto durar (RF-5.25),
    /// igual à forma.
    pub cursor_blinking: bool,
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
            default_cursor_shape: CursorShape::Block,
            cursor_blinking: false,
            osc52_read: false,
            osc52_write: true,
            // 1 MiB. Valor de trabalho para a F1; o teto real vem de
            // `porecatu-config` na F4 (`[terminal.clipboard]`).
            clipboard_write_max_bytes: 1 << 20,
        }
    }
}
