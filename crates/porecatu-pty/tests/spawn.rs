// SPDX-License-Identifier: GPL-3.0-or-later

//! Teste de integracao por plataforma (docs/arquitetura.md secao 7):
//! spawn de um comando trivial, ler saida, encerrar sem vazar processo.
//!
//! No Windows, o ConPTY manda uma sequencia de handshake ao anexar um novo
//! console (win32-input-mode, focus reporting, e um Device Status Report
//! `ESC [ 6 n` pedindo a posicao do cursor) antes de qualquer saida do
//! programa. Um terminal de verdade responde via `porecatu-term`
//! (`alacritty_terminal` cuida disso a partir da Etapa 2); aqui, sem motor
//! VT, o teste faz o papel minimo de responder ao DSR para o shell nao
//! ficar esperando eternamente por uma resposta que nunca chega
//! (ADR-0004: "ConPTY re-renderiza a tela e injeta sequencias proprias").

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::Duration;

use porecatu_pty::{PtySize, SpawnConfig, spawn};

const DSR_CURSOR_POSITION: &[u8] = b"\x1b[6n";
const DSR_CURSOR_POSITION_REPLY: &[u8] = b"\x1b[1;1R";

fn trivial_command() -> (Option<String>, Vec<String>) {
    if cfg!(target_os = "windows") {
        (
            Some("cmd.exe".to_string()),
            vec!["/C".to_string(), "echo hello-porecatu".to_string()],
        )
    } else {
        (
            Some("/bin/sh".to_string()),
            vec!["-c".to_string(), "echo hello-porecatu".to_string()],
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

/// Le ate EOF numa thread separada, respondendo a DSR de posicao de cursor
/// como um terminal minimo faria, e devolve os bytes recebidos pelo canal
/// assim que a leitura termina.
fn read_to_end_answering_dsr(
    mut reader: Box<dyn Read + Send>,
    mut writer: Box<dyn Write + Send>,
) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut collected = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    collected.extend_from_slice(&buf[..n]);
                    if collected
                        .windows(DSR_CURSOR_POSITION.len())
                        .any(|w| w == DSR_CURSOR_POSITION)
                    {
                        let _ = writer.write_all(DSR_CURSOR_POSITION_REPLY);
                        let _ = writer.flush();
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(collected);
    });
    rx
}

fn wait_for_exit(handle: &mut porecatu_pty::PtyHandle, timeout: Duration) {
    let start = std::time::Instant::now();
    loop {
        if handle.try_wait().expect("try_wait falhou").is_some() {
            return;
        }
        if start.elapsed() > timeout {
            panic!("processo nao encerrou dentro do timeout");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn spawn_le_saida_e_encerra_sem_vazar_processo() {
    let (program, args) = trivial_command();
    let (mut handle, _process_group) = spawn(SpawnConfig {
        program,
        args,
        env: Vec::new(),
        cwd: None,
        size: default_size(),
    })
    .expect("spawn falhou");

    let reader = handle.reader().expect("reader falhou");
    let writer = handle.writer().expect("writer falhou");
    let output_rx = read_to_end_answering_dsr(reader, writer);

    // No Windows, o pipe do ConPTY nao emite EOF so porque o processo
    // hospedado encerrou -- ele so fecha quando o pseudo-console em si eh
    // fechado (drop do master). O sinal correto de "processo morreu" eh
    // `try_wait`, nao EOF de leitura; ADR-0004 pede "ler ate EOF antes de
    // marcar a aba como encerrada", e isso continua valendo -- so que aqui
    // "ler ate EOF" vira "drenar o que ja chegou, depois fechar para
    // liberar a thread de leitura", em vez de esperar um EOF que so chega
    // se alguem fechar o pty.
    wait_for_exit(&mut handle, Duration::from_secs(5));
    let status = handle
        .try_wait()
        .expect("try_wait falhou")
        .expect("processo deveria ter encerrado");
    assert!(status.success, "processo trivial deveria sair com sucesso");

    drop(handle);
    let output = output_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("leitura ate EOF nao terminou a tempo apos fechar o pty");
    let output = String::from_utf8_lossy(&output);
    assert!(
        output.contains("hello-porecatu"),
        "saida inesperada: {output:?}"
    );
}

#[test]
fn kill_encerra_processo_de_longa_duracao() {
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

    let (mut handle, _process_group) = spawn(SpawnConfig {
        program: Some(program),
        args,
        env: Vec::new(),
        cwd: None,
        size: default_size(),
    })
    .expect("spawn falhou");

    // Drena a saida (e responde DSR) numa thread para o processo nao travar
    // esperando resposta enquanto o teste segue.
    let reader = handle.reader().expect("reader falhou");
    let writer = handle.writer().expect("writer falhou");
    let _drain_rx = read_to_end_answering_dsr(reader, writer);

    std::thread::sleep(Duration::from_millis(200));
    assert!(
        handle.try_wait().expect("try_wait falhou").is_none(),
        "processo de longa duracao nao deveria ja ter encerrado"
    );

    handle.kill().expect("kill falhou");
    wait_for_exit(&mut handle, Duration::from_secs(5));
}
