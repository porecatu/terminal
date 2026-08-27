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

fn long_running_command() -> (String, Vec<String>) {
    if cfg!(target_os = "windows") {
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
    }
}

fn spawn_long_running() -> Terminal {
    let (program, args) = long_running_command();
    Terminal::spawn(
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
    .expect("spawn falhou")
}

/// Verifica que `Terminal::spawn` + `drop` não travam nem entram em pânico
/// com um processo de longa duração. Não há, de dentro deste crate, uma
/// forma cross-platform barata de confirmar que o processo foi mesmo
/// encerrado (exigiria listar processos do SO, fora do escopo de uma
/// dependência de teste) -- coberto por inspeção manual durante o smoke
/// test da fase (Gerenciador de Tarefas / `ps`).
#[test]
fn terminal_de_processo_longo_nao_trava_ao_dropar() {
    let terminal = spawn_long_running();
    std::thread::sleep(Duration::from_millis(200));
    drop(terminal);
}

/// Regressão: fechar a janela com um processo de longa duração rodando
/// (ex.: um shell interativo esperando o prompt) travava o app inteiro --
/// `Terminal::shutdown` acabava bloqueado dentro de `ClosePseudoConsole`
/// esperando a thread de leitura, que por sua vez esperava o PTY fechar
/// (achado no smoke test manual da Etapa 3, confirmado por usuário rodando
/// `cargo run` numa sessão desktop de verdade). `shutdown` roda numa thread
/// à parte e o teste falha se não voltar dentro do timeout -- do jeito que
/// o bug original faria travar para sempre.
#[test]
fn terminal_shutdown_nao_trava_com_processo_de_longa_duracao() {
    let terminal = spawn_long_running();
    std::thread::sleep(Duration::from_millis(200));

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        terminal.shutdown();
        let _ = done_tx.send(());
    });

    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("Terminal::shutdown travou com um processo de longa duracao rodando");
}

/// ADR-0017 item 4: fechar aba não bloqueia a main thread. `close` devolve
/// bem antes de `SHUTDOWN_TIMEOUT` (2s) mesmo com o processo ainda vivo --
/// ao contrário de `shutdown`, que espera a confirmação.
#[test]
fn terminal_close_devolve_na_hora_sem_esperar_confirmacao() {
    let terminal = spawn_long_running();
    std::thread::sleep(Duration::from_millis(200));

    let start = Instant::now();
    let wait = terminal.close();
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "close() bloqueou esperando confirmacao"
    );

    // A confirmação ainda chega, pra quem quiser esperar por ela.
    wait.wait();
}

#[test]
fn inject_note_escreve_no_grid_como_saida_do_programa() {
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

    terminal.inject_note("processo encerrado (codigo 1)", (0x5e, 0xd3, 0xbc));

    let mut snap = GridSnapshot::default();
    terminal.snapshot_into(&mut snap);
    assert!(snapshot_text(&snap).contains("processo encerrado (codigo 1)"));
}
