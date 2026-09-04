// SPDX-License-Identifier: GPL-3.0-or-later

//! Teste de integração do fallback de `cwd` (ADR-0038) -- só onde
//! `ProcessGroup::cwd()` existe. No Windows este arquivo inteiro some
//! (`#![cfg]` no topo), o que é exatamente a garantia que o ADR pede: a
//! ausência da função é erro de compilação, não algo que um teste possa
//! ver rodar e passar.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::io::Write;
use std::time::{Duration, Instant};

use porecatu_pty::{PtySize, SpawnConfig, spawn};

fn default_size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Sobe um shell interativo, drena a saída numa thread (sem isso o `cd`
/// nunca seria lido -- o pipe do PTY enche e o shell trava escrevendo o
/// eco/prompt) e faz `cd` para um diretório conhecido. `ProcessGroup::cwd`
/// lê o `cwd` do processo direto do SO -- sem evento para esperar, só o
/// tempo do shell processar a linha, daí o polling com timeout.
#[test]
fn process_group_cwd_reflects_a_cd_in_the_spawned_shell() {
    let target = std::env::temp_dir()
        .canonicalize()
        .expect("diretório temporário deveria existir e resolver");

    let (mut handle, group) = spawn(SpawnConfig {
        program: Some("/bin/sh".to_string()),
        args: Vec::new(),
        env: Vec::new(),
        cwd: None,
        size: default_size(),
    })
    .expect("spawn falhou");
    let group = group.expect("ProcessGroup deveria existir fora do Windows");

    let reader = handle.reader().expect("reader falhou");
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });

    let mut writer = handle.writer().expect("writer falhou");
    writer
        .write_all(format!("cd {}\n", target.display()).as_bytes())
        .expect("write do cd falhou");
    writer.flush().expect("flush falhou");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut observed = group.cwd();
    while observed.as_deref() != Some(target.as_path()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        observed = group.cwd();
    }
    assert_eq!(
        observed.as_deref(),
        Some(target.as_path()),
        "cwd do shell não refletiu o `cd` a tempo"
    );

    handle.kill().ok();
}
