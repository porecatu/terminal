// SPDX-License-Identifier: GPL-3.0-or-later

//! Busca incremental por lotes (ADR-0041 §"Riscos e mitigação", item 5 da
//! Etapa 1 do roadmap F6): `SearchJob::step` varre no máximo
//! `lines_per_step` linhas por chamada. Estes testes fixam que fatiar em
//! vários lotes pequenos dá o **mesmo resultado** que um lote grande o
//! bastante para a busca inteira, e que uma busca sem ocorrência nenhuma
//! não escapa do lote (a razão de o lote ser por linha varrida, não por
//! ocorrência achada).

use porecatu_term::{
    Occurrence, SearchMode, SearchStep, TermEngine, TermEvent, TermParams, TermSize,
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

fn run_to_completion(
    term: &TermEngine,
    pattern: &str,
    mode: SearchMode,
    lines_per_step: usize,
) -> (Vec<Occurrence>, usize) {
    let mut job = term.start_search(pattern, mode, lines_per_step).unwrap();
    let mut steps = 0;
    loop {
        steps += 1;
        match term.step_search(&mut job) {
            SearchStep::Done => break,
            SearchStep::InProgress => continue,
        }
    }
    (job.occurrences().to_vec(), steps)
}

/// Grade de teste: `lines` linhas de scrollback, uma marca em algumas
/// delas, texto de enchimento nas outras -- mais que qualquer
/// `lines_per_step` usado nos testes abaixo, pra forçar mais de um lote.
fn fill(term: &mut TermEngine, lines: usize) {
    for i in 0..lines {
        if i % 37 == 0 {
            term.advance(b"marca\r\n");
        } else {
            term.advance(b"linha de enchimento sem a palavra\r\n");
        }
    }
}

#[test]
fn varios_lotes_pequenos_acham_as_mesmas_ocorrencias_que_um_lote_so() {
    let (mut term_a, _rx_a) = engine(5, 30);
    fill(&mut term_a, 400);
    let (occurrences_um_lote, steps_um_lote) =
        run_to_completion(&term_a, "marca", SearchMode::Literal, 1_000_000);

    let (mut term_b, _rx_b) = engine(5, 30);
    fill(&mut term_b, 400);
    let (occurrences_varios_lotes, steps_varios_lotes) =
        run_to_completion(&term_b, "marca", SearchMode::Literal, 3);

    assert_eq!(occurrences_um_lote, occurrences_varios_lotes);
    assert_eq!(steps_um_lote, 1);
    assert!(
        steps_varios_lotes > 1,
        "esperava mais de um passo com lote pequeno, veio {steps_varios_lotes}"
    );
}

#[test]
fn sem_ocorrencia_nenhuma_o_lote_ainda_limita_o_passo() {
    let (mut term, _rx) = engine(5, 30);
    fill(&mut term, 400);

    let mut job = term
        .start_search("palavra-que-nao-existe", SearchMode::Literal, 50)
        .unwrap();

    // Nenhuma ocorrência existe -- sem o limite por linha varrida (em vez
    // de por ocorrência achada), um único `step` varreria a grade inteira
    // de uma vez, que é exatamente o caso que o item 5 mediu como caro.
    assert_eq!(term.step_search(&mut job), SearchStep::InProgress);
    assert!(!job.is_done());
    assert!(job.occurrences().is_empty());

    let mut steps = 1;
    while term.step_search(&mut job) == SearchStep::InProgress {
        steps += 1;
    }
    assert!(job.occurrences().is_empty());
    assert!(
        steps > 1,
        "esperava mais de um passo para varrer 400 linhas em lotes de 50, veio {steps}"
    );
}

#[test]
fn step_apos_done_e_idempotente() {
    let (mut term, _rx) = engine(3, 20);
    term.advance(b"marca");

    let mut job = term
        .start_search("marca", SearchMode::Literal, 1_000_000)
        .unwrap();
    assert_eq!(term.step_search(&mut job), SearchStep::Done);
    let occurrences_no_fim = job.occurrences().to_vec();

    assert_eq!(term.step_search(&mut job), SearchStep::Done);
    assert_eq!(job.occurrences(), occurrences_no_fim.as_slice());
}
