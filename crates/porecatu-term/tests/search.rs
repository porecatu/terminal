// SPDX-License-Identifier: GPL-3.0-or-later

//! Busca no motor (ADR-0041, RF-11.1 a RF-11.9) -- função pura, sem GPU e
//! sem janela: alimenta o parser diretamente (mesmo padrão de
//! `vt_sequences.rs`) e roda `TermEngine::start_search`/`step_search` até
//! terminar.

use porecatu_term::{
    GridPos, Occurrence, SearchMode, SearchStep, TermEngine, TermEvent, TermParams, TermSize,
};

fn engine(rows: usize, cols: usize) -> (TermEngine, std::sync::mpsc::Receiver<TermEvent>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let (pty_writer, _pty_writer_rx) = std::sync::mpsc::channel();
    let engine = TermEngine::new(
        TermParams::default(),
        TermSize { rows, cols },
        tx,
        pty_writer,
    );
    (engine, rx)
}

fn pos(line: i32, column: usize) -> GridPos {
    GridPos { line, column }
}

/// Roda a busca até o fim, num lote (`lines_per_step` grande o bastante
/// para a grade de teste inteira caber num passo só) -- os testes de lote
/// de verdade (mais de um `step`) estão em `search_batching.rs`.
fn search_to_completion(
    term: &TermEngine,
    pattern: &str,
    mode: SearchMode,
) -> (Vec<Occurrence>, bool) {
    let mut job = term.start_search(pattern, mode, 1_000_000).unwrap();
    loop {
        match term.step_search(&mut job) {
            SearchStep::Done => break,
            SearchStep::InProgress => continue,
        }
    }
    (job.occurrences().to_vec(), job.scope_reduced())
}

#[test]
fn literal_encontra_todas_as_ocorrencias_na_tela_visivel() {
    let (mut term, _rx) = engine(3, 20);
    term.advance(b"abcabc\r\nxabcx");

    let (occurrences, scope_reduced) = search_to_completion(&term, "abc", SearchMode::Literal);

    assert_eq!(
        occurrences,
        vec![
            Occurrence {
                start: pos(0, 0),
                end: pos(0, 2)
            },
            Occurrence {
                start: pos(0, 3),
                end: pos(0, 5)
            },
            Occurrence {
                start: pos(1, 1),
                end: pos(1, 3)
            },
        ]
    );
    assert!(!scope_reduced);
}

#[test]
fn regex_encontra_ocorrencias_de_padrao() {
    let (mut term, _rx) = engine(2, 20);
    term.advance(b"foo1 foo22 foo333");

    let (occurrences, _) = search_to_completion(&term, r"foo\d+", SearchMode::Regex);

    assert_eq!(occurrences.len(), 3);
    assert_eq!(occurrences[0].start, pos(0, 0));
    assert_eq!(occurrences[0].end, pos(0, 3));
    assert_eq!(occurrences[1].start, pos(0, 5));
    assert_eq!(occurrences[1].end, pos(0, 9));
    assert_eq!(occurrences[2].start, pos(0, 11));
    assert_eq!(occurrences[2].end, pos(0, 16));
}

#[test]
fn literal_nao_trata_metacaracteres_do_padrao_como_regex() {
    let (mut term, _rx) = engine(2, 20);
    term.advance(b"a.b aXb");

    // Em modo literal, "." casa só com "." -- não com qualquer caractere.
    let (occurrences, _) = search_to_completion(&term, "a.b", SearchMode::Literal);

    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].start, pos(0, 0));
}

#[test]
fn padrao_de_regex_invalido_e_erro_nunca_panic() {
    let (term, _rx) = engine(2, 20);

    let err = term
        .start_search("(", SearchMode::Regex, 1_000)
        .expect_err("regex desbalanceado deveria falhar a compilar");

    assert!(!err.message().is_empty());
}

#[test]
fn padrao_vazio_termina_na_hora_sem_erro_e_sem_ocorrencia() {
    let (term, _rx) = engine(2, 20);

    let mut job = term.start_search("", SearchMode::Literal, 1_000).unwrap();

    assert!(job.is_done());
    assert_eq!(term.step_search(&mut job), SearchStep::Done);
    assert!(job.occurrences().is_empty());
    assert!(!job.scope_reduced());
}

#[test]
fn escopo_alcanca_scrollback_com_linha_negativa() {
    // scrollback default (10_000 linhas) -- rolar bastante conteúdo para
    // fora da tela visível de 3 linhas antes de procurar por algo que só
    // existe lá em cima.
    let (mut term, _rx) = engine(3, 20);
    term.advance(b"marca-no-topo\r\n");
    for _ in 0..500 {
        term.advance(b"linha de enchimento\r\n");
    }

    let (occurrences, scope_reduced) =
        search_to_completion(&term, "marca-no-topo", SearchMode::Literal);

    assert_eq!(occurrences.len(), 1);
    assert!(
        occurrences[0].start.line < 0,
        "esperava posição no scrollback (linha negativa), veio {:?}",
        occurrences[0].start
    );
    assert!(!scope_reduced);
}

#[test]
fn tela_alternativa_reduz_escopo_a_tela_visivel() {
    let (mut term, _rx) = engine(3, 20);
    term.advance(b"conteudo-normal\r\n");
    // DECSET 1049: entra na tela alternativa (RF-11.8, ADR-0041 §7).
    term.advance(b"\x1b[?1049h");
    term.advance(b"conteudo-alt");

    let (occurrences, scope_reduced) = search_to_completion(&term, "conteudo", SearchMode::Literal);

    assert!(scope_reduced);
    // Só o que está na tela alternativa -- o conteúdo da tela normal, que
    // ficou para trás na outra grade, não é alcançado (achasse os dois,
    // "conteudo" apareceria duas vezes).
    assert_eq!(occurrences.len(), 1);
}

#[test]
fn snapshot_into_limpa_ocorrencias_sem_realocar_a_cada_frame() {
    let (mut term, _rx) = engine(2, 10);
    term.advance(b"abc");

    let mut snap = porecatu_term::GridSnapshot::default();
    snap.occurrences.push(porecatu_term::OccurrenceSpan {
        start_row: 0,
        start_col: 0,
        end_row: 0,
        end_col: 0,
        active: true,
    });
    let capacity_before = snap.occurrences.capacity();

    term.snapshot_into(&mut snap);

    assert!(snap.occurrences.is_empty());
    assert_eq!(snap.occurrences.capacity(), capacity_before);
}
