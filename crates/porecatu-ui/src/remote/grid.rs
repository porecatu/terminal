// SPDX-License-Identifier: GPL-3.0-or-later

//! Conversion of the terminal grid snapshot to the protocol grid.
//!
//! Pure functions (ADR-0105 §3, spec 03 §5.2): the engine snapshot goes in, lines
//! and a style table of the protocol come out, following the span rules the
//! decoder of the phone relies on:
//!
//! - spans never overlap and come in column order;
//! - a cell with no span is a space with the default style, so such cells are
//!   omitted and an empty line is `Line { wrapped, spans: [] }`;
//! - a span never mixes widths: a wide cell is a span of width 2 and the
//!   `WIDE_SPACER` after it produces nothing;
//! - a plain span has one code point per cell, contiguous and of one style;
//! - a grapheme of several code points is alone in a `Grapheme` span;
//! - colors stay unresolved (default, indexed, direct).

use std::collections::HashMap;

use porecatu_remote::protocol::message::grid::style_flags;
use porecatu_remote::protocol::message::{
    Color, Cursor, CursorShape, GridSnapshot as ProtocolGrid, Line, Span, SpanText, Style,
};
use porecatu_term::{
    Cell, CellFlags, CellText, CursorShape as TermCursorShape, GridSnapshot, ScrollbackInfo,
    TermColor,
};

/// The live screen as a `grid-snapshot` of the protocol. `pane` and `seq` are
/// zero here: the host sets them per subscription.
pub(crate) fn live_frame(snap: &GridSnapshot, info: ScrollbackInfo) -> ProtocolGrid {
    let (styles, lines) = convert_rows(snap);
    ProtocolGrid {
        pane: 0,
        seq: 0,
        cols: snap.cols as u64,
        rows: snap.rows as u64,
        history: info.history as u64,
        dropped: info.dropped,
        styles,
        lines,
        cursor: convert_cursor(snap),
        alt: snap.modes.alt_screen,
    }
}

/// The screen of a pane that has no engine to read (a `NotStarted` tab, a
/// dormant SSH pane; ADR-0105 section 8): `rows` empty lines of `cols`
/// columns, no history, the cursor hidden. `pane` and `seq` are zero, as in
/// [`live_frame`].
pub(crate) fn empty_frame(cols: u64, rows: u64) -> ProtocolGrid {
    let line = Line {
        wrapped: false,
        spans: Vec::new(),
    };
    ProtocolGrid {
        pane: 0,
        seq: 0,
        cols,
        rows,
        history: 0,
        dropped: 0,
        styles: Vec::new(),
        lines: vec![line; usize::try_from(rows).unwrap_or(0)],
        cursor: Cursor {
            row: 0,
            col: 0,
            shape: CursorShape::Block,
            visible: false,
        },
        alt: false,
    }
}

/// The lines of a scrollback page, top to bottom, with their own style table
/// (the `scrollback-page` message carries `styles` and `lines`).
pub(crate) fn scrollback_lines(snap: &GridSnapshot) -> (Vec<Style>, Vec<Line>) {
    convert_rows(snap)
}

/// Every row of the snapshot, with one style table for all of them.
fn convert_rows(snap: &GridSnapshot) -> (Vec<Style>, Vec<Line>) {
    let mut table = StyleTable::default();
    let lines = (0..snap.rows)
        .map(|row| convert_line(snap, row, &mut table))
        .collect();
    (table.styles, lines)
}

/// Style table without duplicates; the index is what a span stores.
#[derive(Default)]
struct StyleTable {
    styles: Vec<Style>,
    index: HashMap<(u64, u64, u64), u64>,
}

impl StyleTable {
    fn intern(&mut self, style: Style) -> u64 {
        let key = (color_key(style.fg), color_key(style.bg), style.flags);
        if let Some(index) = self.index.get(&key) {
            return *index;
        }
        let index = self.styles.len() as u64;
        self.styles.push(style);
        self.index.insert(key, index);
        index
    }
}

fn color_key(color: Color) -> u64 {
    match color {
        Color::Default => 0,
        Color::Indexed(index) => (1 << 32) | u64::from(index),
        Color::Rgb(rgb) => (2 << 32) | u64::from(rgb),
    }
}

fn convert_color(color: TermColor) -> Color {
    match color {
        TermColor::Default => Color::Default,
        TermColor::Indexed(index) => Color::Indexed(index),
        TermColor::Rgb { r, g, b } => {
            Color::Rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
        }
    }
}

fn convert_flags(flags: CellFlags) -> u64 {
    let mut out = 0;
    for (cell_flag, wire_flag) in [
        (CellFlags::BOLD, style_flags::BOLD),
        (CellFlags::ITALIC, style_flags::ITALIC),
        (CellFlags::UNDERLINE, style_flags::UNDERLINE),
        (CellFlags::INVERSE, style_flags::INVERSE),
        (CellFlags::DIM, style_flags::DIM),
        (CellFlags::STRIKEOUT, style_flags::STRIKETHROUGH),
    ] {
        if flags.contains(cell_flag) {
            out |= wire_flag;
        }
    }
    out
}

fn cell_style(cell: &Cell) -> Style {
    Style {
        fg: convert_color(cell.fg),
        bg: convert_color(cell.bg),
        flags: convert_flags(cell.flags),
    }
}

/// What a cell writes, once the engine's representation is settled.
enum Glyph<'a> {
    /// One code point.
    Char(char),
    /// One grapheme of several code points.
    Grapheme(&'a str),
}

fn glyph<'a>(snap: &'a GridSnapshot, cell: &Cell) -> Glyph<'a> {
    match cell.text {
        // The engine's empty cell is a space; a NUL has no place on the wire.
        CellText::Char('\0') => Glyph::Char(' '),
        CellText::Char(c) => Glyph::Char(c),
        CellText::Cluster { start, end } => {
            let text = snap
                .clusters
                .get(start as usize..end as usize)
                .unwrap_or_default();
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (None, _) => Glyph::Char(' '),
                // A single code point is not a grapheme span (spec 03 §5.2).
                (Some(c), None) => Glyph::Char(c),
                _ => Glyph::Grapheme(text),
            }
        }
    }
}

/// A plain span being built: the next cell joins it only when it is
/// contiguous, of the same width and of the same style.
struct PlainRun {
    col: u64,
    width: u8,
    style: u64,
    /// The run is in the default style, so a space inside it is plain text
    /// and trailing spaces can be dropped.
    default_style: bool,
    text: String,
    next_col: u64,
}

impl PlainRun {
    fn into_span(mut self) -> Span {
        if self.default_style {
            // A trailing space in the default style is what a missing span means.
            self.text.truncate(self.text.trim_end_matches(' ').len());
        }
        Span {
            col: self.col,
            width: self.width,
            style: self.style,
            text: SpanText::Plain(self.text),
        }
    }
}

fn convert_line(snap: &GridSnapshot, row: usize, table: &mut StyleTable) -> Line {
    let cols = snap.cols;
    let cells = row
        .checked_mul(cols)
        .and_then(|start| snap.cells.get(start..start + cols))
        .unwrap_or_default();
    let wrapped = cells
        .last()
        .is_some_and(|cell| cell.flags.contains(CellFlags::WRAPLINE));

    let mut spans = Vec::new();
    let mut run: Option<PlainRun> = None;
    for (col, cell) in cells.iter().enumerate() {
        if cell.flags.contains(CellFlags::WIDE_SPACER) {
            continue;
        }
        let col = col as u64;
        let width: u8 = if cell.flags.contains(CellFlags::WIDE) {
            2
        } else {
            1
        };
        let style = cell_style(cell);
        let glyph = glyph(snap, cell);

        // A space in the default style is exactly what a missing span means.
        let default_style =
            style.fg == Color::Default && style.bg == Color::Default && style.flags == 0;
        if matches!(glyph, Glyph::Char(' ')) && default_style {
            // Inside a default-style run the space stays part of the span (fewer
            // spans); anywhere else it is omitted.
            match &mut run {
                Some(open) if open.default_style && open.width == 1 && open.next_col == col => {
                    open.text.push(' ');
                    open.next_col += 1;
                }
                _ => spans.extend(run.take().map(PlainRun::into_span)),
            }
            continue;
        }

        let style = table.intern(style);
        match glyph {
            Glyph::Char(c) => match &mut run {
                Some(open)
                    if open.width == width && open.style == style && open.next_col == col =>
                {
                    open.text.push(c);
                    open.next_col += u64::from(width);
                }
                _ => {
                    spans.extend(run.take().map(PlainRun::into_span));
                    run = Some(PlainRun {
                        col,
                        width,
                        style,
                        default_style,
                        text: c.to_string(),
                        next_col: col + u64::from(width),
                    });
                }
            },
            Glyph::Grapheme(text) => {
                spans.extend(run.take().map(PlainRun::into_span));
                spans.push(Span {
                    col,
                    width,
                    style,
                    text: SpanText::Grapheme(text.to_owned()),
                });
            }
        }
    }
    spans.extend(run.map(PlainRun::into_span));
    Line { wrapped, spans }
}

fn convert_cursor(snap: &GridSnapshot) -> Cursor {
    let cursor = &snap.cursor;
    let (row, col) = cursor.position.unwrap_or((0, 0));
    let shape = match cursor.shape {
        TermCursorShape::Underline => CursorShape::Underline,
        TermCursorShape::Beam => CursorShape::Beam,
        TermCursorShape::Block | TermCursorShape::HollowBlock | TermCursorShape::Hidden => {
            CursorShape::Block
        }
    };
    Cursor {
        row: row as u64,
        col: col as u64,
        shape,
        visible: cursor.visible
            && cursor.position.is_some()
            && cursor.shape != TermCursorShape::Hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use porecatu_remote::protocol::message::{self, Direction, Message};
    use porecatu_term::{Cursor as TermCursor, TermModes};

    fn plain(text: &str) -> Vec<Cell> {
        text.chars()
            .map(|c| Cell {
                text: CellText::Char(c),
                ..Cell::default()
            })
            .collect()
    }

    fn snapshot(cols: usize, rows: Vec<Vec<Cell>>) -> GridSnapshot {
        let mut cells = Vec::new();
        for mut row in rows.iter().cloned() {
            row.resize(cols, Cell::default());
            cells.extend(row);
        }
        GridSnapshot {
            cols,
            rows: rows.len(),
            cells,
            cursor: TermCursor {
                position: Some((0, 0)),
                shape: TermCursorShape::Block,
                visible: true,
                blinking: false,
            },
            ..GridSnapshot::default()
        }
    }

    fn text_of(span: &Span) -> &str {
        span.text.as_str()
    }

    /// The whole message goes through the protocol codec, which checks the
    /// domain (spans, ids, sizes) as the phone does.
    fn round_trip(frame: &ProtocolGrid) {
        let bytes = message::encode(&Message::GridSnapshot(frame.clone()));
        let decoded = message::decode(&bytes, Direction::HostToClient).expect("decodes");
        assert_eq!(decoded, Message::GridSnapshot(frame.clone()));
    }

    #[test]
    fn the_empty_frame_has_the_size_no_history_and_a_hidden_cursor() {
        let frame = empty_frame(80, 24);
        assert_eq!((frame.cols, frame.rows), (80, 24));
        assert_eq!(frame.lines.len(), 24);
        assert!(frame.lines.iter().all(|line| line.spans.is_empty()));
        assert_eq!((frame.history, frame.dropped), (0, 0));
        assert!(!frame.cursor.visible);
        assert!(!frame.alt);
        round_trip(&frame);
    }

    #[test]
    fn empty_line_has_no_spans() {
        let snap = snapshot(5, vec![vec![]]);
        let frame = live_frame(&snap, ScrollbackInfo::default());
        assert_eq!(frame.lines.len(), 1);
        assert_eq!(
            frame.lines[0],
            Line {
                wrapped: false,
                spans: vec![]
            }
        );
        assert!(frame.styles.is_empty());
        round_trip(&frame);
    }

    #[test]
    fn simple_text_is_one_plain_span_and_trailing_spaces_are_omitted() {
        let snap = snapshot(10, vec![plain("ls -la")]);
        let frame = live_frame(&snap, ScrollbackInfo::default());
        let spans = &frame.lines[0].spans;
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].col, 0);
        assert_eq!(spans[0].width, 1);
        assert_eq!(text_of(&spans[0]), "ls -la");
        assert!(matches!(spans[0].text, SpanText::Plain(_)));
        assert_eq!(frame.styles.len(), 1);
        round_trip(&frame);
    }

    #[test]
    fn interior_default_spaces_stay_in_the_span() {
        let snap = snapshot(10, vec![plain("ab  cd")]);
        let frame = live_frame(&snap, ScrollbackInfo::default());
        let spans = &frame.lines[0].spans;
        assert_eq!(spans.len(), 1);
        assert_eq!((spans[0].col, text_of(&spans[0])), (0, "ab  cd"));
        round_trip(&frame);
    }

    #[test]
    fn two_styles_make_two_spans_and_a_deduplicated_table() {
        let mut row = plain("abcd");
        for cell in &mut row[2..] {
            cell.fg = TermColor::Rgb {
                r: 0x12,
                g: 0x34,
                b: 0x56,
            };
            cell.flags = CellFlags::BOLD | CellFlags::STRIKEOUT;
        }
        row[1].bg = TermColor::Indexed(4);
        // Same style as the first cell, later in the line: reuses the entry.
        let mut tail = plain("e");
        tail[0].text = CellText::Char('e');
        row.extend(tail);
        let snap = snapshot(6, vec![row]);
        let frame = live_frame(&snap, ScrollbackInfo::default());
        let spans = &frame.lines[0].spans;
        assert_eq!(spans.len(), 4);
        assert_eq!(spans[0].style, 0);
        assert_eq!(frame.styles.len(), 3);
        assert_eq!(
            frame.styles[2],
            Style {
                fg: Color::Rgb(0x123456),
                bg: Color::Default,
                flags: style_flags::BOLD | style_flags::STRIKETHROUGH,
            }
        );
        assert_eq!(frame.styles[1].bg, Color::Indexed(4));
        round_trip(&frame);
    }

    #[test]
    fn flags_map_to_the_wire_bits() {
        let mut row = plain("x");
        row[0].flags = CellFlags::ITALIC
            | CellFlags::UNDERLINE
            | CellFlags::INVERSE
            | CellFlags::DIM
            | CellFlags::WRAPLINE;
        let snap = snapshot(1, vec![row]);
        let frame = live_frame(&snap, ScrollbackInfo::default());
        assert_eq!(
            frame.styles[0].flags,
            style_flags::ITALIC | style_flags::UNDERLINE | style_flags::INVERSE | style_flags::DIM
        );
    }

    #[test]
    fn wide_character_is_a_width_two_span_and_the_spacer_is_skipped() {
        let mut row = plain("a");
        row.push(Cell {
            text: CellText::Char('漢'),
            flags: CellFlags::WIDE,
            ..Cell::default()
        });
        row.push(Cell {
            flags: CellFlags::WIDE_SPACER,
            ..Cell::default()
        });
        row.push(Cell {
            text: CellText::Char('字'),
            flags: CellFlags::WIDE,
            ..Cell::default()
        });
        row.push(Cell {
            flags: CellFlags::WIDE_SPACER,
            ..Cell::default()
        });
        row.push(Cell {
            text: CellText::Char('z'),
            ..Cell::default()
        });
        let snap = snapshot(6, vec![row]);
        let frame = live_frame(&snap, ScrollbackInfo::default());
        let spans = &frame.lines[0].spans;
        assert_eq!(spans.len(), 3);
        assert_eq!(
            (spans[0].col, spans[0].width, text_of(&spans[0])),
            (0, 1, "a")
        );
        // The two wide cells are contiguous with the same style: one span,
        // the second code point at `col + 1 * width`.
        assert_eq!(
            (spans[1].col, spans[1].width, text_of(&spans[1])),
            (1, 2, "漢字")
        );
        assert_eq!(
            (spans[2].col, spans[2].width, text_of(&spans[2])),
            (5, 1, "z")
        );
        round_trip(&frame);
    }

    #[test]
    fn zwj_grapheme_is_alone_in_a_grapheme_span() {
        let family = "👨\u{200d}👩\u{200d}👧";
        let mut snap = snapshot(8, vec![plain("ab")]);
        snap.clusters.push_str(family);
        snap.cells[2] = Cell {
            text: CellText::Cluster {
                start: 0,
                end: family.len() as u32,
            },
            flags: CellFlags::WIDE,
            ..Cell::default()
        };
        snap.cells[3] = Cell {
            flags: CellFlags::WIDE_SPACER,
            ..Cell::default()
        };
        snap.cells[4] = Cell {
            text: CellText::Char('c'),
            ..Cell::default()
        };
        let frame = live_frame(&snap, ScrollbackInfo::default());
        let spans = &frame.lines[0].spans;
        assert_eq!(spans.len(), 3);
        assert_eq!(text_of(&spans[0]), "ab");
        assert_eq!(spans[1].col, 2);
        assert_eq!(spans[1].width, 2);
        assert_eq!(spans[1].text, SpanText::Grapheme(family.to_owned()));
        assert_eq!((spans[2].col, text_of(&spans[2])), (4, "c"));
        round_trip(&frame);
    }

    #[test]
    fn combining_mark_cluster_is_a_width_one_grapheme() {
        let mut snap = snapshot(3, vec![vec![]]);
        let text = "e\u{301}";
        snap.clusters.push_str(text);
        snap.cells[0].text = CellText::Cluster {
            start: 0,
            end: text.len() as u32,
        };
        let frame = live_frame(&snap, ScrollbackInfo::default());
        assert_eq!(
            frame.lines[0].spans,
            vec![Span {
                col: 0,
                width: 1,
                style: 0,
                text: SpanText::Grapheme(text.to_owned()),
            }]
        );
        round_trip(&frame);
    }

    #[test]
    fn space_with_colored_background_is_not_omitted() {
        let mut row = plain("a b");
        row[1].bg = TermColor::Indexed(1);
        let snap = snapshot(4, vec![row]);
        let frame = live_frame(&snap, ScrollbackInfo::default());
        let spans = &frame.lines[0].spans;
        assert_eq!(spans.len(), 3);
        assert_eq!((spans[1].col, text_of(&spans[1])), (1, " "));
        assert_eq!(frame.styles[spans[1].style as usize].bg, Color::Indexed(1));
        round_trip(&frame);
    }

    #[test]
    fn inverse_space_is_not_omitted() {
        let mut row = plain(" ");
        row[0].flags = CellFlags::INVERSE;
        let snap = snapshot(2, vec![row]);
        let frame = live_frame(&snap, ScrollbackInfo::default());
        assert_eq!(frame.lines[0].spans.len(), 1);
    }

    #[test]
    fn wrapped_comes_from_the_last_cell() {
        let mut first = plain("abc");
        first[2].flags = CellFlags::WRAPLINE;
        let second = plain("de");
        let snap = snapshot(3, vec![first, second]);
        let frame = live_frame(&snap, ScrollbackInfo::default());
        assert!(frame.lines[0].wrapped);
        assert!(!frame.lines[1].wrapped);
        round_trip(&frame);
    }

    #[test]
    fn hidden_cursor_and_missing_position_are_invisible() {
        let mut snap = snapshot(3, vec![vec![]]);
        snap.cursor = TermCursor {
            position: Some((0, 2)),
            shape: TermCursorShape::Hidden,
            visible: true,
            blinking: false,
        };
        let frame = live_frame(&snap, ScrollbackInfo::default());
        assert!(!frame.cursor.visible);
        assert_eq!(frame.cursor.col, 2);

        snap.cursor = TermCursor {
            position: None,
            shape: TermCursorShape::Beam,
            visible: true,
            blinking: false,
        };
        let frame = live_frame(&snap, ScrollbackInfo::default());
        assert!(!frame.cursor.visible);
        assert_eq!((frame.cursor.row, frame.cursor.col), (0, 0));
        round_trip(&frame);
    }

    #[test]
    fn cursor_shapes_and_visibility() {
        let mut snap = snapshot(5, vec![vec![], vec![]]);
        for (term, wire) in [
            (TermCursorShape::Block, CursorShape::Block),
            (TermCursorShape::HollowBlock, CursorShape::Block),
            (TermCursorShape::Underline, CursorShape::Underline),
            (TermCursorShape::Beam, CursorShape::Beam),
        ] {
            snap.cursor = TermCursor {
                position: Some((1, 3)),
                shape: term,
                visible: true,
                blinking: true,
            };
            let cursor = live_frame(&snap, ScrollbackInfo::default()).cursor;
            assert_eq!(cursor.shape, wire);
            assert!(cursor.visible);
            assert_eq!((cursor.row, cursor.col), (1, 3));
        }
        snap.cursor.visible = false;
        assert!(!live_frame(&snap, ScrollbackInfo::default()).cursor.visible);
    }

    #[test]
    fn frame_carries_size_scrollback_and_alt_screen() {
        let mut snap = snapshot(4, vec![vec![], vec![], vec![]]);
        snap.modes = TermModes {
            alt_screen: true,
            ..TermModes::default()
        };
        let frame = live_frame(
            &snap,
            ScrollbackInfo {
                history: 120,
                dropped: 7,
            },
        );
        assert_eq!((frame.pane, frame.seq), (0, 0));
        assert_eq!((frame.cols, frame.rows), (4, 3));
        assert_eq!((frame.history, frame.dropped), (120, 7));
        assert_eq!(frame.lines.len(), 3);
        assert!(frame.alt);
        round_trip(&frame);
    }

    #[test]
    fn scrollback_page_lines_use_their_own_table() {
        let mut row = plain("ab");
        row[1].fg = TermColor::Indexed(9);
        let snap = snapshot(2, vec![plain("xx"), row]);
        let (styles, lines) = scrollback_lines(&snap);
        assert_eq!(lines.len(), 2);
        assert_eq!(styles.len(), 2);
        let page = message::ScrollbackPage {
            id: 1,
            pane: 2,
            history: 10,
            dropped: 0,
            offset: 0,
            styles,
            lines,
        };
        let bytes = message::encode(&Message::ScrollbackPage(page.clone()));
        assert_eq!(
            message::decode(&bytes, Direction::HostToClient).expect("decodes"),
            Message::ScrollbackPage(page)
        );
    }

    #[test]
    fn mixed_line_round_trips() {
        let mut row = plain("ls ");
        row.push(Cell {
            text: CellText::Char('日'),
            flags: CellFlags::WIDE,
            fg: TermColor::Indexed(2),
            ..Cell::default()
        });
        row.push(Cell {
            flags: CellFlags::WIDE_SPACER,
            fg: TermColor::Indexed(2),
            ..Cell::default()
        });
        row.extend(plain(" end"));
        let last = row.len() - 1;
        row[last].flags = CellFlags::WRAPLINE;
        let snap = snapshot(12, vec![row, vec![]]);
        round_trip(&live_frame(&snap, ScrollbackInfo::default()));
    }
}
