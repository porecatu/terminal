// SPDX-License-Identifier: GPL-3.0-or-later

//! `snapshot_live_into`, `history_into` e o contador `dropped` (ADR-0105 §3 e
//! §5, spec do protocolo §5.5). Motor real alimentado com bytes, sem PTY.

use porecatu_term::{
    CellText, GridSnapshot, ScrollbackInfo, SelectionKind, SelectionSide, TermEngine, TermEvent,
    TermParams, TermScroll, TermSize,
};

fn engine_with(
    rows: usize,
    cols: usize,
    scrollback_lines: usize,
) -> (TermEngine, std::sync::mpsc::Receiver<TermEvent>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let (pty_writer, _pty_writer_rx) = std::sync::mpsc::channel();
    let params = TermParams {
        scrollback_lines,
        ..TermParams::default()
    };
    let engine = TermEngine::new(params, TermSize { rows, cols }, tx, pty_writer);
    (engine, rx)
}

fn row_text(snap: &GridSnapshot, row: usize) -> String {
    let mut text = String::new();
    for col in 0..snap.cols {
        let cell = snap.cells[row * snap.cols + col];
        if cell.flags.contains(porecatu_term::CellFlags::WIDE_SPACER) {
            continue;
        }
        match cell.text {
            CellText::Char(c) => text.push(c),
            CellText::Cluster { start, end } => {
                text.push_str(&snap.clusters[start as usize..end as usize]);
            }
        }
    }
    text.trim_end().to_owned()
}

fn lines(snap: &GridSnapshot) -> Vec<String> {
    (0..snap.rows).map(|row| row_text(snap, row)).collect()
}

/// `N0000\r\nN0001\r\n...` -- a linha `k` tem o índice absoluto `k`.
fn numbered(from: usize, to: usize) -> Vec<u8> {
    (from..to)
        .flat_map(|n| format!("N{n:04}\r\n").into_bytes())
        .collect()
}

/// Alimenta o motor em pedaços de tamanhos variados, como as leituras do PTY
/// (nunca mais que 4096 bytes).
fn feed_in_chunks(engine: &mut TermEngine, bytes: &[u8]) {
    let sizes = [1usize, 7, 4096, 33, 500, 2, 1024, 90];
    let mut rest = bytes;
    let mut index = 0;
    while !rest.is_empty() {
        let take = sizes[index % sizes.len()].min(rest.len());
        engine.advance(&rest[..take]);
        rest = &rest[take..];
        index += 1;
    }
}

fn history_lines(engine: &TermEngine) -> (Vec<String>, ScrollbackInfo) {
    let mut snap = GridSnapshot::default();
    let info = engine.history_into(0, usize::MAX, &mut snap);
    (lines(&snap), info)
}

#[test]
fn live_screen_ignores_the_desktop_scroll() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    engine.advance(&numbered(0, 20));

    let mut at_bottom = GridSnapshot::default();
    engine.snapshot_into(&mut at_bottom);
    assert_eq!(at_bottom.scroll_offset, 0);

    engine.scroll(TermScroll::Lines(3));
    let mut view = GridSnapshot::default();
    engine.snapshot_into(&mut view);
    assert_eq!(view.scroll_offset, 3);
    assert_ne!(lines(&view), lines(&at_bottom));

    let mut live = GridSnapshot::default();
    let info = engine.snapshot_live_into(&mut live);
    assert_eq!(lines(&live), lines(&at_bottom));
    assert_eq!(live.scroll_offset, 0);
    assert_eq!(live.cursor, at_bottom.cursor);
    assert_eq!(info.history, 20 + 1 - 5);

    // Ler a tela viva não mexe na rolagem do desktop.
    let mut again = GridSnapshot::default();
    engine.snapshot_into(&mut again);
    assert_eq!(again.scroll_offset, 3);
    assert_eq!(lines(&again), lines(&view));
}

#[test]
fn live_screen_has_no_selection_and_no_occurrences() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    engine.advance(&numbered(0, 3));
    engine.start_selection(SelectionKind::Simple, 0, 0, SelectionSide::Left);
    engine.update_selection(1, 3, SelectionSide::Right);

    let mut view = GridSnapshot::default();
    engine.snapshot_into(&mut view);
    assert!(view.selection.is_some(), "a vista do desktop tem seleção");

    let mut live = GridSnapshot::default();
    live.occurrences.push(porecatu_term::OccurrenceSpan {
        start_row: 0,
        start_col: 0,
        end_row: 0,
        end_col: 1,
        active: true,
    });
    engine.snapshot_live_into(&mut live);
    assert!(live.selection.is_none());
    assert!(live.occurrences.is_empty());
}

#[test]
fn history_pages_come_top_to_bottom_ending_at_the_offset() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    // 30 linhas + a final em branco: a tela guarda as 4 últimas e o histórico
    // as 26 primeiras (0..=25).
    engine.advance(&numbered(0, 30));

    let mut page = GridSnapshot::default();
    let info = engine.history_into(0, 3, &mut page);
    assert_eq!(
        info,
        ScrollbackInfo {
            history: 26,
            dropped: 0
        }
    );
    assert_eq!(page.rows, 3);
    assert_eq!(page.cols, 10);
    assert_eq!(lines(&page), ["N0023", "N0024", "N0025"]);
    assert!(page.selection.is_none());
    assert!(page.cursor.position.is_none());
    assert_eq!(page.scroll_offset, 0);

    // Segunda página: termina uma linha acima da primeira.
    engine.history_into(3, 4, &mut page);
    assert_eq!(lines(&page), ["N0019", "N0020", "N0021", "N0022"]);

    // O limite é o que existe: pedir além do topo devolve só o que sobra.
    let info = engine.history_into(24, 10, &mut page);
    assert_eq!(lines(&page), ["N0000", "N0001"]);
    assert_eq!(info.history, 26);

    // Deslocamento fora do histórico: nenhuma linha.
    engine.history_into(26, 5, &mut page);
    assert_eq!(page.rows, 0);
    assert!(page.cells.is_empty());
    engine.history_into(0, 0, &mut page);
    assert_eq!(page.rows, 0);

    // O histórico inteiro de uma vez.
    let (all, _) = history_lines(&engine);
    assert_eq!(all.len(), 26);
    assert_eq!(all.first().map(String::as_str), Some("N0000"));
    assert_eq!(all.last().map(String::as_str), Some("N0025"));
}

#[test]
fn dropped_is_exact_across_chunk_sizes() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    let total = 1000;
    feed_in_chunks(&mut engine, &numbered(0, total));

    let (all, info) = history_lines(&engine);
    // Tela: 4 últimas linhas + a em branco do cursor. Histórico: o teto.
    assert_eq!(info.history, 100);
    assert_eq!(all.len(), 100);
    assert_eq!(info.dropped, (total - 4 - 100) as u64);

    // Índice absoluto = dropped + (history - 1 - offset), e o número na linha
    // é o próprio índice.
    let mut page = GridSnapshot::default();
    for offset in [0usize, 1, 17, 98, 99] {
        let info = engine.history_into(offset, 1, &mut page);
        let absolute = info.dropped as usize + (info.history - 1 - offset);
        assert_eq!(
            row_text(&page, 0),
            format!("N{absolute:04}"),
            "offset {offset}"
        );
    }
    assert_eq!(all[0], "N0896");
    assert_eq!(all[99], "N0995");

    // A contagem continua de onde parou.
    feed_in_chunks(&mut engine, &numbered(total, total + 50));
    let (_, info) = history_lines(&engine);
    assert_eq!(info.history, 100);
    assert_eq!(info.dropped, (total + 50 - 4 - 100) as u64);
}

#[test]
fn dropped_is_exact_for_a_chunk_of_4096_newlines() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    engine.advance(&numbered(0, 600));
    // O pior lote que a folga cobre: 4096 bytes, uma linha empurrada por byte.
    engine.advance(&vec![b'\n'; 4096]);
    let mut snap = GridSnapshot::default();
    let info = engine.snapshot_live_into(&mut snap);
    assert_eq!(info.history, 100);
    // 600 linhas numeradas + 4096 quebras: cada quebra empurra uma linha.
    assert_eq!(info.dropped, (600 + 4096 - 4 - 100) as u64);
}

#[test]
fn the_desktop_never_sees_more_than_scrollback_lines() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    feed_in_chunks(&mut engine, &numbered(0, 1000));

    engine.scroll(TermScroll::Top);
    let mut view = GridSnapshot::default();
    engine.snapshot_into(&mut view);
    assert_eq!(view.scroll_offset, 100);
    // O topo da vista é a linha mais antiga que ainda existe.
    assert_eq!(row_text(&view, 0), "N0896");
}

#[test]
fn csi_3_j_and_ris_zero_dropped_and_history() {
    // O CSI 3 J deixa a tela cheia (cada linha nova empurra uma ao histórico); o
    // RIS também limpa a tela (as 4 últimas linhas ficam nela).
    for (sequence, refill_dropped) in [(&b"\x1b[3J"[..], 100u64), (&b"\x1bc"[..], 96)] {
        let (mut engine, _rx) = engine_with(5, 10, 100);
        feed_in_chunks(&mut engine, &numbered(0, 300));
        let (_, before) = history_lines(&engine);
        assert!(before.dropped > 0);

        engine.advance(sequence);
        let (all, info) = history_lines(&engine);
        assert_eq!(
            info,
            ScrollbackInfo {
                history: 0,
                dropped: 0
            },
            "{sequence:?}"
        );
        assert!(all.is_empty());

        // E a contagem recomeça do zero.
        feed_in_chunks(&mut engine, &numbered(0, 200));
        let (_, info) = history_lines(&engine);
        assert_eq!(info.history, 100);

        assert_eq!(info.dropped, refill_dropped, "{sequence:?}");
    }
}

#[test]
fn a_clear_is_seen_even_when_the_same_chunk_refills_the_history() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    feed_in_chunks(&mut engine, &numbered(0, 300));

    // Limpa e reenche no mesmo lote, até passar do teto de novo.
    let mut chunk = b"\x1b[3J".to_vec();
    chunk.extend(numbered(0, 150));
    engine.advance(&chunk);
    let (all, info) = history_lines(&engine);
    assert_eq!(info.history, 100);
    // A tela continua cheia depois da limpeza: 150 linhas empurram 150 ao histórico
    // (as 4 da tela antiga primeiro, depois N0000..N0145).
    assert_eq!(info.dropped, (150 - 100) as u64);
    assert_eq!(all[0], "N0046");
}

#[test]
fn a_clear_sequence_split_between_reads_is_still_a_clear() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    feed_in_chunks(&mut engine, &numbered(0, 300));

    engine.advance(b"\x1b[");
    engine.advance(b"3");
    let (_, info) = history_lines(&engine);
    assert!(info.dropped > 0, "ainda não terminou");
    engine.advance(b"J");
    let (_, info) = history_lines(&engine);
    assert_eq!(
        info,
        ScrollbackInfo {
            history: 0,
            dropped: 0
        }
    );
}

#[test]
fn the_alternate_screen_has_no_history_and_repeats_dropped() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    feed_in_chunks(&mut engine, &numbered(0, 300));
    let (_, normal) = history_lines(&engine);
    assert_eq!(normal.history, 100);
    assert_eq!(normal.dropped, (300 - 4 - 100) as u64);

    engine.advance(b"\x1b[?1049h");
    // Escrever na alternativa não conta nada e não cria histórico.
    feed_in_chunks(&mut engine, &numbered(0, 200));
    let mut page = GridSnapshot::default();
    let info = engine.history_into(0, 50, &mut page);
    assert_eq!(
        info,
        ScrollbackInfo {
            history: 0,
            dropped: normal.dropped
        }
    );
    assert_eq!(page.rows, 0);
    let mut live = GridSnapshot::default();
    let info = engine.snapshot_live_into(&mut live);
    assert_eq!(
        info,
        ScrollbackInfo {
            history: 0,
            dropped: normal.dropped
        }
    );
    assert!(live.modes.alt_screen);

    // Ao sair, o histórico volta e a contagem continua.
    engine.advance(b"\x1b[?1049l");
    let (_, info) = history_lines(&engine);
    assert_eq!(info, normal);
    feed_in_chunks(&mut engine, &numbered(300, 320));
    let (_, info) = history_lines(&engine);
    assert_eq!(info.history, 100);
    assert_eq!(info.dropped, (320 - 4 - 100) as u64);
}

#[test]
fn ris_inside_the_alternate_screen_clears_the_history() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    feed_in_chunks(&mut engine, &numbered(0, 300));
    engine.advance(b"\x1b[?1049h");
    engine.advance(b"\x1bc");
    let (_, info) = history_lines(&engine);
    assert_eq!(
        info,
        ScrollbackInfo {
            history: 0,
            dropped: 0
        }
    );
}

#[test]
fn resize_keeps_dropped() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    feed_in_chunks(&mut engine, &numbered(0, 300));
    let (_, before) = history_lines(&engine);

    // Crescer a tela puxa linhas do histórico: não é limpeza.
    assert!(engine.resize(9, 10));
    let (_, grown) = history_lines(&engine);
    assert_eq!(grown.dropped, before.dropped);
    assert_eq!(grown.history, 100 - 4);

    // Ao voltar, as linhas voltam ao histórico, no mesmo índice absoluto.
    assert!(engine.resize(5, 10));
    let (all, shrunk) = history_lines(&engine);
    assert_eq!(shrunk.dropped, before.dropped);
    assert_eq!(all.last().map(String::as_str), Some("N0295"));
}

#[test]
fn shrinking_scrollback_lines_counts_what_it_cuts() {
    let (mut engine, _rx) = engine_with(5, 10, 100);
    feed_in_chunks(&mut engine, &numbered(0, 300));
    let (_, before) = history_lines(&engine);

    engine.set_scrollback_lines(40);
    let (all, info) = history_lines(&engine);
    assert_eq!(info.history, 40);
    assert_eq!(info.dropped, before.dropped + 60);
    assert_eq!(all[0], "N0256");
    engine.scroll(TermScroll::Top);
    let mut view = GridSnapshot::default();
    engine.snapshot_into(&mut view);
    assert_eq!(view.scroll_offset, 40);

    // Crescer de novo mantém a folga e continua contando exato.
    engine.set_scrollback_lines(80);
    feed_in_chunks(&mut engine, &numbered(300, 500));
    let (_, info) = history_lines(&engine);
    assert_eq!(info.history, 80);
    assert_eq!(info.dropped, (500 - 4 - 80) as u64);
}

#[test]
fn live_screen_is_identical_to_the_snapshot_without_scroll() {
    let (mut engine, _rx) = engine_with(4, 12, 100);
    // Grafema de vários code points (e + acento combinante), caractere largo e
    // um hyperlink.
    engine.advance("e\u{0301} \u{65e5}\u{672c} x\r\n".as_bytes());
    engine.advance(b"\x1b]8;;http://example.com\x1b\\link\x1b]8;;\x1b\\\r\n");
    engine.advance(b"\x1b[1;31mred\x1b[0m");

    let mut view = GridSnapshot::default();
    engine.snapshot_into(&mut view);
    let mut live = GridSnapshot::default();
    engine.snapshot_live_into(&mut live);

    assert_eq!(live.cols, view.cols);
    assert_eq!(live.rows, view.rows);
    assert_eq!(live.cells, view.cells);
    assert_eq!(live.clusters, view.clusters);
    assert!(!live.clusters.is_empty(), "o grafema virou cluster");
    assert_eq!(live.cursor, view.cursor);
    assert_eq!(live.modes, view.modes);
    assert_eq!(live.hyperlinks, view.hyperlinks);
    assert_eq!(live.hyperlink_spans, view.hyperlink_spans);
    assert!(!live.hyperlink_spans.is_empty());
    assert_eq!(live.scroll_offset, 0);
    assert!(live.selection.is_none());
}

#[test]
fn history_keeps_graphemes_and_wide_characters() {
    let (mut engine, _rx) = engine_with(3, 12, 100);
    engine.advance("e\u{0301} \u{65e5}\u{672c}\r\n".as_bytes());
    engine.advance(&numbered(0, 5));

    let (all, info) = history_lines(&engine);
    assert_eq!(info.history, 4);
    assert_eq!(all[0], "e\u{0301} \u{65e5}\u{672c}");

    let mut page = GridSnapshot::default();
    engine.history_into(3, 1, &mut page);
    assert!(
        page.cells
            .iter()
            .any(|cell| cell.flags.contains(porecatu_term::CellFlags::WIDE))
    );
    assert!(
        page.cells
            .iter()
            .any(|cell| matches!(cell.text, CellText::Cluster { .. }))
    );
}
