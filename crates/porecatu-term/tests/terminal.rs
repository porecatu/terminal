// SPDX-License-Identifier: GPL-3.0-or-later

//! Teste de integração ponta a ponta: `Terminal::spawn` -> PTY real ->
//! motor VT -> snapshot, e o ciclo de vida do processo (docs/roadmap.md,
//! critério de saída da F1: "última linha de saída não se perde").
//!
//! Ao contrário do teste equivalente em `porecatu-pty` (Etapa 1), aqui
//! ninguém responde a DSR na mão -- é exatamente o que este teste verifica:
//! o motor real resolve isso sozinho, escrevendo a resposta de volta no
//! PTY por conta própria.

use std::time::{Duration, Instant};

use porecatu_pty::{PtySize, SpawnConfig};
use porecatu_term::{CellText, GridSnapshot, TermEvent, TermParams, Terminal};

fn trivial_command() -> (Option<String>, Vec<String>) {
    if cfg!(target_os = "windows") {
        (
            Some("cmd.exe".to_string()),
            vec!["/C".to_string(), "echo hello-terminal".to_string()],
        )
    } else {
        (
            Some("/bin/sh".to_string()),
            vec!["-c".to_string(), "echo hello-terminal".to_string()],
        )
    }
}

fn default_size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn snapshot_text(snap: &GridSnapshot) -> String {
    let mut text = String::new();
    for cell in &snap.cells {
        match cell.text {
            CellText::Char(c) => text.push(c),
            CellText::Cluster { start, end } => {
                text.push_str(&snap.clusters[start as usize..end as usize])
            }
        }
    }
    text
}

#[test]
fn terminal_spawna_escreve_saida_no_snapshot_e_responde_dsr_sozinho() {
    let (program, args) = trivial_command();
    let terminal = Terminal::spawn(
        SpawnConfig {
            program,
            args,
            env: Vec::new(),
            cwd: None,
            size: default_size(),
        },
        TermParams::default(),
        || {},
    )
    .expect("spawn falhou");

    let mut snap = GridSnapshot::default();
    let start = Instant::now();
    let mut found = false;
    while start.elapsed() < Duration::from_secs(5) {
        terminal.snapshot_into(&mut snap);
        if snapshot_text(&snap).contains("hello-terminal") {
            found = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(found, "saida esperada nao apareceu no snapshot a tempo");

    let start = Instant::now();
    let mut exited = None;
    while start.elapsed() < Duration::from_secs(5) {
        if let Some(TermEvent::Exit { success, .. }) = terminal.try_recv_event() {
            exited = Some(success);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        exited,
        Some(true),
        "esperava TermEvent::Exit {{ success: true }}"
    );
}

/// Verifica que `Terminal::spawn` + `drop` não travam nem entram em pânico
/// com um processo de longa duração. Não há, de dentro deste crate, uma
/// forma cross-platform barata de confirmar que o processo foi mesmo
/// encerrado (exigiria listar processos do SO, fora do escopo de uma
/// dependência de teste) -- coberto por inspeção manual durante o smoke
/// test da fase (Gerenciador de Tarefas / `ps`).
#[test]
fn terminal_de_processo_longo_nao_trava_ao_dropar() {
    let (program, args) = if cfg!(target_os = "windows") {
        (
            "cmd.exe".to_string(),
            vec![
                "/C".to_string(),
                "ping".to_string(),
                "-t".to_string(),
                "127.0.0.1".to_string(),
            ],
        )
    } else {
        (
            "/bin/sh".to_string(),
            vec!["-c".to_string(), "sleep 60".to_string()],
        )
    };

    let terminal = Terminal::spawn(
        SpawnConfig {
            program: Some(program),
            args,
            env: Vec::new(),
            cwd: None,
            size: default_size(),
        },
        TermParams::default(),
        || {},
    )
    .expect("spawn falhou");

    std::thread::sleep(Duration::from_millis(200));
    drop(terminal);
}
