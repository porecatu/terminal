// SPDX-License-Identifier: GPL-3.0-or-later

//! Tipos do snapshot de grade (docs/arquitetura.md seção 4.1). Nenhum tipo
//! do `alacritty_terminal` atravessa esta fronteira -- trocar o motor não
//! deve vazar para `porecatu-ui`.

use bitflags::bitflags;

use crate::color::TermColor;

bitflags! {
    /// Atributos de célula. Não inclui tudo que o motor rastreia (hyperlink,
    /// cor de sublinhado) -- só o que a especificação visual e o roadmap de
    /// F1 pedem; o resto entra quando tiver consumidor.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct CellFlags: u16 {
        const BOLD        = 1 << 0;
        const ITALIC      = 1 << 1;
        const UNDERLINE   = 1 << 2;
        const INVERSE     = 1 << 3;
        const WIDE        = 1 << 4;
        const WIDE_SPACER = 1 << 5;
        const WRAPLINE    = 1 << 6;
        const DIM         = 1 << 7;
        const STRIKEOUT   = 1 << 8;
    }
}

/// Texto de uma célula: char único no caminho comum, ou uma fatia na arena
/// [`GridSnapshot::clusters`] quando há grafema composto (base + combinantes,
/// ZWJ) -- decisão 3 da seção 4.1. `start`/`end` são offsets de byte UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellText {
    Char(char),
    Cluster { start: u32, end: u32 },
}

impl Default for CellText {
    fn default() -> Self {
        CellText::Char(' ')
    }
}

/// Uma célula da grade. `fg`/`bg` não resolvidos -- ver [`TermColor`].
/// Caractere de largura dupla ocupa duas células: a primeira leva o texto e
/// a flag `WIDE`; a segunda vem vazia com `WIDE_SPACER` (decisão 4 da seção
/// 4.1) -- sem isso a coluna de CJK desalinha e o hit-testing do mouse erra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cell {
    pub text: CellText,
    pub fg: TermColor,
    pub bg: TermColor,
    pub flags: CellFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Underline,
    Beam,
    HollowBlock,
    Hidden,
}

/// Posição do cursor relativa à viewport (não à grade inteira, que inclui
/// scrollback). `None` quando o cursor está fora da área visível --
/// acontece quando o usuário rolou para cima.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub position: Option<(usize, usize)>,
    pub shape: CursorShape,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseReporting {
    #[default]
    None,
    /// Modo 1000: pressiona e solta.
    Click,
    /// Modo 1002: clique mais arraste com botão pressionado.
    ClickAndDrag,
    /// Modo 1003: qualquer movimento, com ou sem botão.
    AnyMotion,
}

/// Subconjunto de `TermMode` que `porecatu-ui` precisa para rotear input
/// (ADR-0008, ADR-0013): tela alternativa, bracketed paste, modo de mouse,
/// e os dois modos que mudam a codificação de teclado (DECCKM e teclado
/// numérico de aplicação).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TermModes {
    pub alt_screen: bool,
    pub bracketed_paste: bool,
    pub mouse_reporting: MouseReporting,
    /// Encoding SGR (1006) preferido; quando falso e `mouse_reporting !=
    /// None`, o programa negociou só o encoding X10 legado (ADR-0013).
    pub sgr_mouse: bool,
    /// DECCKM: setas mandam `ESC O A` em vez de `ESC [ A` (ADR-0008).
    pub app_cursor_keys: bool,
    /// Teclado numérico de aplicação (ADR-0008). Sem consumidor ainda --
    /// F1 não emula o teclado numérico separado do principal.
    pub app_keypad: bool,
}

/// Span de seleção resolvido para pintura, em coordenadas de viewport.
///
/// Ponto fora da viewport (seleção que sobe no scrollback além do que está
/// visível) cai em `(0, 0)` -- resolução fina fica para a Etapa 6, quando
/// gestos de seleção existirem de verdade e houver caso para testar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionSpan {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub is_block: bool,
}

/// Ocorrência de busca (ADR-0041) resolvida para pintura, em coordenadas de
/// viewport -- mesma convenção de [`SelectionSpan`]. Quem corta a lista
/// bruta de `crate::search::Occurrence` (posição absoluta na grade) pela
/// vista e monta isto é `porecatu-ui`, que também resolve a cor (ADR-0041
/// §4: "a mesma divisão de trabalho que já vale para a paleta").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OccurrenceSpan {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
    /// Ocorrência ativa (RF-11.7): realce distinto das demais.
    pub active: bool,
}

/// Snapshot de grade de um frame -- tipo próprio de `porecatu-term`, seção
/// 4.1 da arquitetura. Reusado entre frames: [`GridSnapshot::default`] uma
/// vez, depois só `TermEngine::snapshot_into` sobre a mesma instância, sem
/// alocar no caminho quente após o primeiro frame (ADR-0007).
#[derive(Debug, Default)]
pub struct GridSnapshot {
    pub cols: usize,
    pub rows: usize,
    /// `rows * cols` células, row-major, só a área visível.
    pub cells: Vec<Cell>,
    /// Arena de texto do frame para [`CellText::Cluster`], reusada.
    pub clusters: String,
    pub cursor: Cursor,
    /// Linhas acima do fundo do scrollback.
    pub scroll_offset: usize,
    pub selection: Option<SelectionSpan>,
    /// Ocorrências de busca (ADR-0041) já cortadas pela vista -- vazio sem
    /// busca ativa. `TermEngine::snapshot_into` só limpa este buffer (sem
    /// realocar, ADR-0007); é `porecatu-ui` quem o preenche a partir do
    /// resultado de `TermEngine::search` e da cor resolvida.
    pub occurrences: Vec<OccurrenceSpan>,
    pub modes: TermModes,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            position: None,
            shape: CursorShape::Hidden,
            visible: false,
        }
    }
}
