// SPDX-License-Identifier: GPL-3.0-or-later

//! Golden-style: sequência de escape na entrada, snapshot/eventos
//! esperados na saída (docs/arquitetura.md seção 7). Alimenta o parser
//! diretamente, sem PTY real -- ADR-0004 pede exatamente isso, porque o
//! ConPTY reemite bytes de um jeito que não é portável entre plataformas.

use porecatu_term::{
    CellFlags, CellText, MouseReporting, TermColor, TermEngine, TermEvent, TermParams, TermScroll,
    TermSize,
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

#[test]
fn decckm_liga_e_desliga_app_cursor_keys() {
    let (mut term, _rx) = engine(5, 10);

    term.advance(b"\x1b[?1h");
    let mut snap = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snap);
    assert!(snap.modes.app_cursor_keys);

    term.advance(b"\x1b[?1l");
    term.snapshot_into(&mut snap);
    assert!(!snap.modes.app_cursor_keys);
}

#[test]
fn scroll_move_scroll_offset_na_tela_principal() {
    let (mut term, _rx) = engine(3, 10);
    // 3 linhas de tela + 7 no scrollback.
    for i in 0..10 {
        term.advance(format!("linha {i}\r\n").as_bytes());
    }

    let mut snap = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snap);
    assert_eq!(snap.scroll_offset, 0, "comeca no fundo do scrollback");

    term.scroll(TermScroll::Lines(2));
    term.snapshot_into(&mut snap);
    assert_eq!(snap.scroll_offset, 2);

    term.scroll(TermScroll::Top);
    term.snapshot_into(&mut snap);
    assert!(snap.scroll_offset > 2, "Top sobe ate o inicio do historico");

    term.scroll(TermScroll::Bottom);
    term.snapshot_into(&mut snap);
    assert_eq!(snap.scroll_offset, 0, "Bottom volta ao fundo");
}

#[test]
fn tela_alternativa_nao_tem_scrollback_para_rolar() {
    let (mut term, _rx) = engine(3, 10);
    for i in 0..10 {
        term.advance(format!("linha {i}\r\n").as_bytes());
    }
    term.advance(b"\x1b[?1049h"); // entra na tela alternativa

    let mut snap = porecatu_term::GridSnapshot::default();
    term.scroll(TermScroll::Top);
    term.snapshot_into(&mut snap);

    assert!(snap.modes.alt_screen);
    assert_eq!(
        snap.scroll_offset, 0,
        "tela alternativa nao tem historico -- rolar nao faz nada (ADR-0013)"
    );
}

#[test]
fn selecao_simples_recorta_espaco_a_direita() {
    use porecatu_term::{SelectionKind, SelectionSide};

    let (mut term, _rx) = engine(3, 10);
    term.advance(b"hi\r\n"); // "hi" seguido de espacos em branco ate' a borda

    term.start_selection(SelectionKind::Simple, 0, 0, SelectionSide::Left);
    term.update_selection(0, 9, SelectionSide::Right);

    let text = term.selection_text().expect("esperava selecao ativa");
    assert_eq!(
        text, "hi",
        "espaco em branco a direita deveria ser cortado (RF-10.6)"
    );
}

#[test]
fn selecao_de_linha_remonta_wrapline_sem_quebra() {
    use porecatu_term::{SelectionKind, SelectionSide};

    let (mut term, _rx) = engine(3, 5);
    // "abcdef" com grade de 5 colunas quebra em "abcde" + "f" (WRAPLINE).
    term.advance(b"abcdef");

    term.start_selection(SelectionKind::Lines, 0, 0, SelectionSide::Left);
    term.update_selection(1, 0, SelectionSide::Right);

    let text = term.selection_text().expect("esperava selecao ativa");
    // SelectionType::Lines sempre fecha com um \n de fim de linha logica
    // selecionada -- o que importa pro RF-10.6 e' nao ter \n NO MEIO,
    // entre "abcde" e "f", que reconstituiria a quebra que so' existe por
    // causa da largura da janela.
    assert_eq!(
        text, "abcdef\n",
        "linha so' quebrada pela largura nao pode virar dois comandos ao colar (RF-10.6)"
    );
}

#[test]
fn rolagem_preserva_selecao() {
    use porecatu_term::{SelectionKind, SelectionSide};

    let (mut term, _rx) = engine(3, 10);
    for i in 0..10 {
        term.advance(format!("linha {i}\r\n").as_bytes());
    }

    term.start_selection(SelectionKind::Simple, 0, 0, SelectionSide::Left);
    term.update_selection(0, 5, SelectionSide::Right);
    assert!(term.selection_text().is_some());

    term.scroll(TermScroll::Lines(2));

    assert!(
        term.selection_text().is_some(),
        "rolagem pura preserva a selecao (ADR-0013, RF-10.7)"
    );
}

/// RF-5.22: `[terminal.cursor] shape` chega via `TermParams::
/// default_cursor_shape` e aparece no snapshot sem nenhum DECSCUSR emitido
/// -- é o default do motor, não uma sequência do programa.
#[test]
fn default_cursor_shape_vem_de_term_params() {
    use porecatu_term::CursorShape;

    let (tx, _rx) = std::sync::mpsc::channel();
    let (pty_writer, _pty_writer_rx) = std::sync::mpsc::channel();
    let params = TermParams {
        default_cursor_shape: CursorShape::Beam,
        ..TermParams::default()
    };
    let mut term = TermEngine::new(params, TermSize { rows: 2, cols: 10 }, tx, pty_writer);
    term.advance(b"x");

    let mut snapshot = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snapshot);
    assert_eq!(snapshot.cursor.shape, CursorShape::Beam);
}

/// RF-5.25: DECSCUSR emitido pelo programa tem precedência sobre o default
/// da config enquanto durar.
#[test]
fn decscusr_do_programa_sobrepoe_o_default_da_config() {
    use porecatu_term::CursorShape;

    let (tx, _rx) = std::sync::mpsc::channel();
    let (pty_writer, _pty_writer_rx) = std::sync::mpsc::channel();
    let params = TermParams {
        default_cursor_shape: CursorShape::Block,
        ..TermParams::default()
    };
    let mut term = TermEngine::new(params, TermSize { rows: 2, cols: 10 }, tx, pty_writer);
    // DECSCUSR: barra piscante (CSI 5 SP q).
    term.advance(b"\x1b[5 q");

    let mut snapshot = porecatu_term::GridSnapshot::default();
    term.snapshot_into(&mut snapshot);
    assert_eq!(snapshot.cursor.shape, CursorShape::Beam);
}
