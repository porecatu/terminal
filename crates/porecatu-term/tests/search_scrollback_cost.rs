// SPDX-License-Identifier: GPL-3.0-or-later

//! Teto do custo de um lote de busca (ADR-0041, item 5 da Etapa 1 do
//! roadmap F6): a busca roda na main thread, então o que importa é um
//! único `SearchJob::step` fechar dentro de um intervalo de frame -- medido
//! aqui, antes de existir barra, exatamente como o ADR pede.
//!
//! **Antes da busca ser incremental por lotes**, medir a busca inteira de
//! uma vez (scrollback default de 10.000 linhas) deu 22ms/18.6ms em
//! release e 125ms em debug -- acima do orçamento de frame já no caso
//! default, sem precisar do teto configurável. Essa medição continua
//! reproduzível: `git log` desta etapa, ou rode `RegexIter` sem lote sobre
//! o mesmo conteúdo. A busca em lotes (`DEFAULT_SEARCH_LINES_PER_STEP`)
//! existe por causa desse número; os testes abaixo medem o **lote**, que é
//! o que passa a rodar por frame de verdade.

use std::time::Instant;

use porecatu_term::{
    DEFAULT_SEARCH_LINES_PER_STEP, SearchMode, SearchStep, TermEngine, TermEvent, TermParams,
    TermSize,
};

/// ~60 fps -- orçamento de frame que o ADR-0041 usa como referência
/// ("busca em scrollback grande travar a UI" -- risco do §"Riscos e
/// mitigação").
const FRAME_BUDGET_MS: u128 = 16;

fn engine(
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

/// Enche o scrollback com `lines` linhas de texto realista (80 colunas,
/// palavras variadas, para o DFA ter trabalho de verdade por linha em vez
/// de casar de cara).
fn fill_scrollback(term: &mut TermEngine, lines: usize) {
    let words = [
        "porecatu",
        "terminal",
        "grupo",
        "aba",
        "scrollback",
        "regex",
        "motor",
        "grade",
        "busca",
        "sessao",
    ];
    for i in 0..lines {
        let w1 = words[i % words.len()];
        let w2 = words[(i * 7 + 3) % words.len()];
        term.advance(
            format!("{i:06} {w1} lorem ipsum {w2} dolor sit amet consectetur\r\n").as_bytes(),
        );
    }
}

/// Mede o **pior passo** (o mais lento entre todos os lotes até `Done`) de
/// uma busca sobre um scrollback cheio -- é o número que importa: um só
/// lote lento já é um frame perdido, não importa quantos rápidos vieram
/// antes.
fn measure_worst_step(
    scrollback_lines: usize,
    rows: usize,
    cols: usize,
    pattern: &str,
    mode: SearchMode,
) -> u128 {
    let (mut term, _rx) = engine(rows, cols, scrollback_lines);
    // Uma linha a mais que a capacidade força o scrollback a ficar cheio de
    // verdade (a mais antiga sai, as `scrollback_lines` seguintes ficam).
    fill_scrollback(&mut term, scrollback_lines + rows + 1);

    let mut job = term
        .start_search(pattern, mode, DEFAULT_SEARCH_LINES_PER_STEP)
        .unwrap();

    let mut worst = 0u128;
    let mut steps = 0;
    loop {
        steps += 1;
        let start = Instant::now();
        let step = term.step_search(&mut job);
        worst = worst.max(start.elapsed().as_millis());
        if step == SearchStep::Done {
            break;
        }
    }

    println!(
        "scrollback_lines={scrollback_lines} pattern={pattern:?} mode={mode:?} \
         lines_per_step={DEFAULT_SEARCH_LINES_PER_STEP} passos={steps} \
         ocorrencias={} pior_passo={worst}ms",
        job.occurrences().len()
    );

    worst
}

#[test]
fn pior_lote_no_default_de_scrollback_fecha_dentro_do_orcamento_de_frame() {
    let literal = measure_worst_step(10_000, 50, 80, "porecatu", SearchMode::Literal);
    let regex = measure_worst_step(10_000, 50, 80, r"\d{6} \w+ lorem", SearchMode::Regex);
    // Padrão sem ocorrência nenhuma -- o caso que um lote por ocorrência
    // não protegeria (teria que varrer a grade inteira pra concluir "zero
    // ocorrências").
    let no_match = measure_worst_step(
        10_000,
        50,
        80,
        "esta-string-nao-existe",
        SearchMode::Literal,
    );

    assert!(
        literal < FRAME_BUDGET_MS,
        "pior lote da busca literal levou {literal}ms, acima do orçamento de {FRAME_BUDGET_MS}ms"
    );
    assert!(
        regex < FRAME_BUDGET_MS,
        "pior lote da busca regex levou {regex}ms, acima do orçamento de {FRAME_BUDGET_MS}ms"
    );
    assert!(
        no_match < FRAME_BUDGET_MS,
        "pior lote sem ocorrência nenhuma levou {no_match}ms, acima do orçamento de {FRAME_BUDGET_MS}ms"
    );
}

#[test]
fn pior_lote_em_100_mil_linhas_fecha_dentro_do_orcamento_de_frame() {
    // 100_000 -- dez vezes o default, como teto configurável plausível
    // (`[terminal.scrollback] lines` não impõe um máximo em `porecatu-config`).
    let literal = measure_worst_step(100_000, 50, 80, "porecatu", SearchMode::Literal);
    let regex = measure_worst_step(100_000, 50, 80, r"\d{6} \w+ lorem", SearchMode::Regex);
    let no_match = measure_worst_step(
        100_000,
        50,
        80,
        "esta-string-nao-existe",
        SearchMode::Literal,
    );

    assert!(
        literal < FRAME_BUDGET_MS,
        "pior lote da busca literal em 100_000 linhas levou {literal}ms, acima do orçamento de {FRAME_BUDGET_MS}ms"
    );
    assert!(
        regex < FRAME_BUDGET_MS,
        "pior lote da busca regex em 100_000 linhas levou {regex}ms, acima do orçamento de {FRAME_BUDGET_MS}ms"
    );
    assert!(
        no_match < FRAME_BUDGET_MS,
        "pior lote sem ocorrência nenhuma em 100_000 linhas levou {no_match}ms, acima do orçamento de {FRAME_BUDGET_MS}ms"
    );
}
