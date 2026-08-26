// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;
use std::sync::mpsc;

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::Flags as AlacFlags;
use alacritty_terminal::term::{ClipboardType, Config as AlacConfig, Osc52, point_to_viewport};
use alacritty_terminal::vte::ansi::{CursorShape as AnsiCursorShape, Processor, Rgb as AnsiRgb};

use crate::event::{ClipboardResponder, ColorQueryResponder, TermEvent};
use crate::params::TermParams;
use crate::snapshot::{
    Cell, CellFlags, CellText, Cursor, CursorShape, GridSnapshot, MouseReporting, SelectionSpan,
    TermModes,
};

/// Dimensões do terminal em células. Tipo próprio -- `Dimensions` do
/// `alacritty_terminal` é implementado aqui, não exposto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    pub rows: usize,
    pub cols: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

struct EventProxy {
    sender: mpsc::Sender<TermEvent>,
    /// Canal de escrita do PTY (o mesmo que `Terminal::write` usa) -- é
    /// para lá, não para `sender`, que `Event::PtyWrite` vai. Diferente dos
    /// outros eventos da seção 4.3, uma resposta automática do motor
    /// (DSR/DA/CPR) não é decisão de `ui`: o motor já formatou os bytes
    /// certos, só falta escrever. Rotear por `sender` obrigaria todo
    /// consumidor de eventos a filtrar e repassar isso a cada wakeup --
    /// esquecer um `PtyWrite` pendente é o programa ficar parado esperando
    /// resposta que nunca chega (era exatamente o problema do teste de
    /// integração da Etapa 1, antes deste motor existir).
    pty_writer: mpsc::Sender<Vec<u8>>,
    clipboard_write_max_bytes: usize,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            let _ = self.pty_writer.send(text.into_bytes());
            return;
        }

        let mapped = match event {
            Event::PtyWrite(_) => unreachable!("tratado acima"),
            Event::Title(title) => Some(TermEvent::Title(Some(title))),
            Event::ResetTitle => Some(TermEvent::Title(None)),
            Event::Bell => Some(TermEvent::Bell),
            Event::ClipboardStore(ClipboardType::Clipboard, text) => {
                if text.len() > self.clipboard_write_max_bytes {
                    None
                } else {
                    Some(TermEvent::ClipboardWrite(text))
                }
            }
            // RF-10.9: a seleção PRIMARY não é suportada no v1.
            Event::ClipboardStore(ClipboardType::Selection, _) => None,
            Event::ClipboardLoad(_clipboard_type, formatter) => {
                Some(TermEvent::ClipboardRead(ClipboardResponder(formatter)))
            }
            Event::ColorRequest(index, formatter) => {
                Some(TermEvent::ColorQuery(ColorQueryResponder {
                    index,
                    format: Arc::new(move |r, g, b| formatter(AnsiRgb { r, g, b })),
                }))
            }
            // Sem consumidor no v1 (dica de cursor de mouse, blink de
            // cursor, wakeup de coalescência própria do motor -- usamos a
            // nossa, ver ADR-0007/0015 -- e tamanho de área de texto em
            // pixels). `Exit`/`ChildExit` também ficam de fora: o motor não
            // sabe de PTY, quem detecta o fim do processo é `Terminal`
            // (Etapa 3), via `porecatu-pty::PtyHandle::try_wait` -- é de lá
            // que `TermEvent::Exit` sai, não daqui.
            Event::MouseCursorDirty
            | Event::CursorBlinkingChange
            | Event::Wakeup
            | Event::TextAreaSizeRequest(_)
            | Event::Exit
            | Event::ChildExit(_) => None,
        };

        if let Some(event) = mapped {
            let _ = self.sender.send(event);
        }
    }
}

fn osc52_mode(read: bool, write: bool) -> Osc52 {
    match (write, read) {
        (false, false) => Osc52::Disabled,
        (true, false) => Osc52::OnlyCopy,
        (false, true) => Osc52::OnlyPaste,
        (true, true) => Osc52::CopyPaste,
    }
}

/// Motor VT encapsulado (ADR-0002). Uso de `alacritty_terminal` isolado
/// aqui dentro -- nenhum tipo dele atravessa a API pública deste crate.
pub struct TermEngine {
    term: Term<EventProxy>,
    parser: Processor,
}

impl TermEngine {
    /// Cria o motor. `events` é o canal para onde o motor manda os eventos
    /// traduzidos (docs/arquitetura.md seção 4.3) -- chamado de dentro de
    /// `advance`, no mesmo lock que a thread de leitura já segura
    /// (ADR-0007). Recebido de fora (em vez de criado aqui) porque
    /// `Terminal` (Etapa 3) precisa do mesmo `Sender` para injetar
    /// `TermEvent::Exit`, que não vem do motor -- vem do fim do processo.
    ///
    /// `pty_writer` é o canal para onde vão as respostas automáticas do
    /// motor (DSR/DA/CPR) -- ver o comentário em `EventProxy::pty_writer`.
    pub fn new(
        params: TermParams,
        size: TermSize,
        events: mpsc::Sender<TermEvent>,
        pty_writer: mpsc::Sender<Vec<u8>>,
    ) -> Self {
        let config = AlacConfig {
            scrolling_history: params.scrollback_lines,
            semantic_escape_chars: params.word_separators,
            osc52: osc52_mode(params.osc52_read, params.osc52_write),
            ..Default::default()
        };
        let proxy = EventProxy {
            sender: events,
            pty_writer,
            clipboard_write_max_bytes: params.clipboard_write_max_bytes,
        };

        let term = Term::new(config, &size, proxy);
        Self {
            term,
            parser: Processor::new(),
        }
    }

    /// Alimenta o parser VT com bytes crus do PTY.
    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// Redimensiona a grade.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.term.resize(TermSize { rows, cols });
    }

    /// Preenche `out` com o estado atual, reusando os buffers já alocados
    /// (`cells`/`clusters`) em vez de realocar (ADR-0007).
    pub fn snapshot_into(&self, out: &mut GridSnapshot) {
        let content = self.term.renderable_content();
        let cols = self.term.columns();
        let rows = self.term.screen_lines();
        let display_offset = content.display_offset;

        out.cols = cols;
        out.rows = rows;
        out.clusters.clear();
        out.cells.clear();
        out.cells.resize(rows * cols, Cell::default());

        for (index, indexed) in content.display_iter.enumerate() {
            if let Some(slot) = out.cells.get_mut(index) {
                *slot = convert_cell(indexed.cell, &mut out.clusters);
            }
        }

        let shape = convert_cursor_shape(content.cursor.shape);
        out.cursor = Cursor {
            position: point_to_viewport(display_offset, content.cursor.point)
                .map(|p| (p.line, p.column.0)),
            shape,
            visible: shape != CursorShape::Hidden,
        };
        out.scroll_offset = display_offset;
        out.selection = content
            .selection
            .map(|range| convert_selection(range, display_offset, rows, cols));
        out.modes = convert_modes(content.mode);
    }
}

fn convert_cell(cell: &alacritty_terminal::term::cell::Cell, clusters: &mut String) -> Cell {
    let text = match cell.zerowidth() {
        Some(zerowidth) if !zerowidth.is_empty() => {
            let start = clusters.len() as u32;
            clusters.push(cell.c);
            for &extra in zerowidth {
                clusters.push(extra);
            }
            let end = clusters.len() as u32;
            CellText::Cluster { start, end }
        }
        _ => CellText::Char(cell.c),
    };

    Cell {
        text,
        fg: crate::color::TermColor::from(cell.fg),
        bg: crate::color::TermColor::from(cell.bg),
        flags: convert_flags(cell.flags),
    }
}

fn convert_flags(flags: AlacFlags) -> CellFlags {
    let mut out = CellFlags::empty();
    if flags.contains(AlacFlags::BOLD) {
        out |= CellFlags::BOLD;
    }
    if flags.contains(AlacFlags::ITALIC) {
        out |= CellFlags::ITALIC;
    }
    if flags.intersects(AlacFlags::ALL_UNDERLINES) {
        out |= CellFlags::UNDERLINE;
    }
    if flags.contains(AlacFlags::INVERSE) {
        out |= CellFlags::INVERSE;
    }
    if flags.contains(AlacFlags::WIDE_CHAR) {
        out |= CellFlags::WIDE;
    }
    if flags.contains(AlacFlags::WIDE_CHAR_SPACER) {
        out |= CellFlags::WIDE_SPACER;
    }
    if flags.contains(AlacFlags::WRAPLINE) {
        out |= CellFlags::WRAPLINE;
    }
    if flags.intersects(AlacFlags::DIM) {
        out |= CellFlags::DIM;
    }
    if flags.contains(AlacFlags::STRIKEOUT) {
        out |= CellFlags::STRIKEOUT;
    }
    out
}

fn convert_cursor_shape(shape: AnsiCursorShape) -> CursorShape {
    match shape {
        AnsiCursorShape::Block => CursorShape::Block,
        AnsiCursorShape::Underline => CursorShape::Underline,
        AnsiCursorShape::Beam => CursorShape::Beam,
        AnsiCursorShape::HollowBlock => CursorShape::HollowBlock,
        AnsiCursorShape::Hidden => CursorShape::Hidden,
    }
}

fn resolve_viewport_point(
    point: Point,
    display_offset: usize,
    rows: usize,
    cols: usize,
) -> (usize, usize) {
    match point_to_viewport(display_offset, point) {
        Some(p) => (
            p.line.min(rows.saturating_sub(1)),
            p.column.0.min(cols.saturating_sub(1)),
        ),
        None => (0, 0),
    }
}

fn convert_selection(
    range: alacritty_terminal::selection::SelectionRange,
    display_offset: usize,
    rows: usize,
    cols: usize,
) -> SelectionSpan {
    let (start_row, start_col) = resolve_viewport_point(range.start, display_offset, rows, cols);
    let (end_row, end_col) = resolve_viewport_point(range.end, display_offset, rows, cols);
    SelectionSpan {
        start_row,
        start_col,
        end_row,
        end_col,
        is_block: range.is_block,
    }
}

fn convert_modes(mode: alacritty_terminal::term::TermMode) -> TermModes {
    use alacritty_terminal::term::TermMode as M;

    let mouse_reporting = if mode.contains(M::MOUSE_MOTION) {
        MouseReporting::AnyMotion
    } else if mode.contains(M::MOUSE_DRAG) {
        MouseReporting::ClickAndDrag
    } else if mode.contains(M::MOUSE_REPORT_CLICK) {
        MouseReporting::Click
    } else {
        MouseReporting::None
    };

    TermModes {
        alt_screen: mode.contains(M::ALT_SCREEN),
        bracketed_paste: mode.contains(M::BRACKETED_PASTE),
        mouse_reporting,
        sgr_mouse: mode.contains(M::SGR_MOUSE),
    }
}
