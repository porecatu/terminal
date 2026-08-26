// SPDX-License-Identifier: GPL-3.0-or-later

//! Golden-style: sequência de escape na entrada, snapshot/eventos
//! esperados na saída (docs/arquitetura.md seção 7). Alimenta o parser
//! diretamente, sem PTY real -- ADR-0004 pede exatamente isso, porque o
//! ConPTY reemite bytes de um jeito que não é portável entre plataformas.

use porecatu_term::{
    CellFlags, CellText, MouseReporting, TermColor, TermEngine, TermEvent, TermParams, TermSize,
};

fn engine(rows: usize, cols: usize) -> (TermEngine, std::sync::mpsc::Receiver<TermEvent>) {
    let (tx, rx) = std::sync::mpsc::channel();
    // Respostas automáticas do motor (PtyWrite) não passam por `TermEvent`
    // -- ver o teste `dsr_posicao_do_cursor_responde_via_ptywrite`, que
    // monta seu próprio motor para inspecionar esse canal.
    let (pty_writer, _pty_writer_rx) = std::sync::mpsc::channel();
    let engine = TermEngine::new(
        TermParams::default(),
        TermSize { rows, cols },
        tx,
        pty_writer,
    );
    (engine, rx)
}

fn cell_char(cells: &porecatu_term::GridSnapshot, row: usize, col: usize) -> char {
    match cells.cells[row * cells.cols + col].text {
        CellText::Char(c) => c,
        CellText::Cluster { .. } => panic!("celula ({row},{col}) e' um cluster, nao char simples"),
    }
}

#[test]
fn texto_simples_preenche_celulas() {
    let (mut term, _rx) = engine(3, 10);
    term.advance(b"abc");

    let mut snap = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snap);

    assert_eq!(cell_char(&snap, 0, 0), 'a');
    assert_eq!(cell_char(&snap, 0, 1), 'b');
    assert_eq!(cell_char(&snap, 0, 2), 'c');
    assert_eq!(snap.cursor.position, Some((0, 3)));
    assert!(snap.cursor.visible);
}

#[test]
fn sgr_negrito_italico_sublinhado_marca_flags() {
    let (mut term, _rx) = engine(2, 10);
    term.advance(b"\x1b[1mB\x1b[0m\x1b[3mI\x1b[0m\x1b[4mU");

    let mut snap = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snap);

    assert!(snap.cells[0].flags.contains(CellFlags::BOLD));
    assert!(snap.cells[1].flags.contains(CellFlags::ITALIC));
    assert!(snap.cells[2].flags.contains(CellFlags::UNDERLINE));
}

#[test]
fn sgr_cor_indexada_e_rgb_nao_resolvidas() {
    let (mut term, _rx) = engine(2, 10);
    term.advance(b"\x1b[38;5;196mA\x1b[0m\x1b[38;2;10;20;30mB");

    let mut snap = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snap);

    assert_eq!(snap.cells[0].fg, TermColor::Indexed(196));
    assert_eq!(
        snap.cells[1].fg,
        TermColor::Rgb {
            r: 10,
            g: 20,
            b: 30
        }
    );
}

#[test]
fn caractere_largura_dupla_ocupa_duas_celulas() {
    let (mut term, _rx) = engine(2, 10);
    // U+4E2D ("meio"), East Asian Wide.
    term.advance("中".as_bytes());

    let mut snap = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snap);

    assert_eq!(cell_char(&snap, 0, 0), '中');
    assert!(snap.cells[0].flags.contains(CellFlags::WIDE));
    assert!(snap.cells[1].flags.contains(CellFlags::WIDE_SPACER));
    assert_eq!(snap.cursor.position, Some((0, 2)));
}

#[test]
fn grafema_composto_vai_para_arena_de_clusters() {
    let (mut term, _rx) = engine(2, 10);
    // 'e' + combining acute (U+0301) -- uma celula, zerowidth anexado.
    term.advance("e\u{0301}".as_bytes());

    let mut snap = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snap);

    match snap.cells[0].text {
        CellText::Cluster { start, end } => {
            let text = &snap.clusters[start as usize..end as usize];
            assert_eq!(text, "e\u{0301}");
        }
        CellText::Char(c) => panic!("esperava cluster, veio char simples {c:?}"),
    }
}

#[test]
fn quebra_de_linha_automatica_marca_wrapline() {
    let (mut term, _rx) = engine(3, 5);
    term.advance(b"abcdef");

    let mut snap = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snap);

    assert!(snap.cells[4].flags.contains(CellFlags::WRAPLINE));
    assert_eq!(cell_char(&snap, 1, 0), 'f');
}

#[test]
fn buffers_sao_reusados_entre_frames() {
    let (mut term, _rx) = engine(2, 5);
    term.advance(b"abc");

    let mut snap = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snap);
    let cells_ptr_before = snap.cells.as_ptr();
    let clusters_cap_before = snap.clusters.capacity();

    term.advance(b"\r\ndef");
    term.snapshot_into(&mut snap);

    // Mesmo Vec, sem realocar -- mesmo ponteiro de backing store.
    assert_eq!(snap.cells.as_ptr(), cells_ptr_before);
    assert!(snap.clusters.capacity() >= clusters_cap_before);
    assert_eq!(cell_char(&snap, 0, 0), 'a');
    assert_eq!(cell_char(&snap, 1, 0), 'd');
}

#[test]
fn titulo_osc_0() {
    let (mut term, rx) = engine(2, 10);
    term.advance(b"\x1b]0;ola\x07");
    assert!(matches!(rx.try_recv(), Ok(TermEvent::Title(Some(t))) if t == "ola"));

    // OSC 0 com titulo vazio ainda manda `Some("")`, nao `None` -- o motor
    // so' emite `ResetTitle` (nosso `Title(None)`) por outro caminho (ex.:
    // RIS), que nao faz parte deste teste.
    term.advance(b"\x1b]0;\x07");
    assert!(matches!(rx.try_recv(), Ok(TermEvent::Title(Some(t))) if t.is_empty()));
}

#[test]
fn bell_gera_evento() {
    let (mut term, rx) = engine(2, 10);
    term.advance(b"\x07");
    assert!(matches!(rx.try_recv(), Ok(TermEvent::Bell)));
}

#[test]
fn osc_52_escrita_decodifica_base64() {
    let (mut term, rx) = engine(2, 10);
    // "Man" em base64 e' "TWFu" -- exemplo classico, sem padding.
    term.advance(b"\x1b]52;c;TWFu\x07");

    match rx.try_recv() {
        Ok(TermEvent::ClipboardWrite(text)) => assert_eq!(text, "Man"),
        other => panic!("esperava ClipboardWrite(\"Man\"), veio {other:?}"),
    }
}

#[test]
fn osc_52_leitura_negada_por_default_nao_gera_evento() {
    let (mut term, rx) = engine(2, 10);
    term.advance(b"\x1b]52;c;?\x07");
    assert!(
        rx.try_recv().is_err(),
        "leitura de OSC 52 deveria ser negada por default (ADR-0013)"
    );
}

#[test]
fn osc_52_leitura_liberada_gera_evento_respondivel() {
    let params = TermParams {
        osc52_read: true,
        ..TermParams::default()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let (pty_writer, _pty_writer_rx) = std::sync::mpsc::channel();
    let mut term = TermEngine::new(params, TermSize { rows: 2, cols: 10 }, tx, pty_writer);
    term.advance(b"\x1b]52;c;?\x07");

    match rx.try_recv() {
        Ok(TermEvent::ClipboardRead(responder)) => {
            let reply = responder.respond("segredo");
            // "segredo" em base64 e' "c2VncmVkbw==".
            assert_eq!(reply, "\x1b]52;c;c2VncmVkbw==\x07");
        }
        other => panic!("esperava ClipboardRead, veio {other:?}"),
    }
}

#[test]
fn dsr_posicao_do_cursor_responde_via_ptywrite() {
    let (events_tx, _events_rx) = std::sync::mpsc::channel();
    let (pty_writer, pty_writer_rx) = std::sync::mpsc::channel();
    let mut term = TermEngine::new(
        TermParams::default(),
        TermSize { rows: 5, cols: 10 },
        events_tx,
        pty_writer,
    );
    term.advance(b"\x1b[6n");

    match pty_writer_rx.try_recv() {
        Ok(bytes) => assert_eq!(bytes, b"\x1b[1;1R"),
        other => panic!("esperava resposta de CPR no canal de escrita, veio {other:?}"),
    }
}

#[test]
fn modos_tela_alternativa_bracketed_paste_e_mouse() {
    let (mut term, _rx) = engine(5, 10);
    term.advance(b"\x1b[?1049h\x1b[?2004h\x1b[?1002h\x1b[?1006h");

    let mut snap = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snap);

    assert!(snap.modes.alt_screen);
    assert!(snap.modes.bracketed_paste);
    assert_eq!(snap.modes.mouse_reporting, MouseReporting::ClickAndDrag);
    assert!(snap.modes.sgr_mouse);
}
