// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{Read, Write};
use std::path::PathBuf;

use portable_pty::{Child, CommandBuilder, MasterPty, native_pty_system};

use crate::error::PtyError;
use crate::job::ProcessGroup;
use crate::shell::{resolve_default_shell, search_path};

/// Dimensão da viewport em células, mais o tamanho em pixels quando
/// disponível (0 quando desconhecido — nem toda plataforma reporta).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl From<PtySize> for portable_pty::PtySize {
    fn from(size: PtySize) -> Self {
        portable_pty::PtySize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: size.pixel_width,
            pixel_height: size.pixel_height,
        }
    }
}

/// Parâmetros de um `spawn`. `program` ausente cai na resolução default de
/// [`crate::shell::resolve_default_shell`] (ADR-0004); quando presente —
/// tipicamente `config.shell.program` — tem precedência total.
///
/// `env` são as variáveis de `[shell.env]` do usuário: aplicadas **depois**
/// do ambiente base (`TERM`, `COLORTERM`, `TERM_PROGRAM`,
/// `TERM_PROGRAM_VERSION`), portanto podem sobrescrevê-lo (ADR-0012).
#[derive(Debug, Clone, Default)]
pub struct SpawnConfig {
    pub program: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub size: PtySize,
}

/// Status de saída do processo filho. Tipo próprio — `portable_pty::ExitStatus`
/// não atravessa esta fronteira (mesma disciplina do `alacritty_terminal`
/// em `porecatu-term`, ADR-0002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyExitStatus {
    pub success: bool,
    pub code: u32,
}

impl From<portable_pty::ExitStatus> for PtyExitStatus {
    fn from(status: portable_pty::ExitStatus) -> Self {
        Self {
            success: status.success(),
            code: status.exit_code(),
        }
    }
}

/// Ambiente injetado por todo shell spawnado, antes de `SpawnConfig.env`
/// (ADR-0012). `TERM=xterm-256color`: ver o ADR para o porquê de não usar
/// terminfo próprio — resumo, funciona sob SSH em host que não conhece o
/// Porecatu.
///
/// Nota sobre `TERM_PROGRAM_VERSION` no Windows: forçar UTF-8 no ConPTY não
/// exige nenhuma chamada ao Win32 (e portanto nenhum `unsafe`, vedado no
/// workspace). O canal de I/O do pseudo-console é UTF-8 por contrato da API
/// — o code page do console hospedado nunca entra em jogo para o que trafega
/// no pipe. Ver a documentação de Pseudoconsoles da Microsoft.
fn base_env() -> [(&'static str, String); 4] {
    [
        ("TERM", "xterm-256color".to_string()),
        ("COLORTERM", "truecolor".to_string()),
        ("TERM_PROGRAM", "porecatu".to_string()),
        (
            "TERM_PROGRAM_VERSION",
            env!("CARGO_PKG_VERSION").to_string(),
        ),
    ]
}

/// Handle de PTY: spawn já aconteceu, o processo está rodando.
/// API síncrona e agnóstica de GUI (ADR-0004) — quem chama decide threading.
pub struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl PtyHandle {
    /// Novo handle de leitura, consumido pela thread de leitura dedicada
    /// (ADR-0007). Chamar mais de uma vez não é o uso pretendido — uma
    /// thread de leitura por terminal — mas a chamada em si não falha.
    pub fn reader(&self) -> Result<Box<dyn Read + Send>, PtyError> {
        self.master
            .try_clone_reader()
            .map_err(|e| PtyError::new("try_clone_reader", e))
    }

    /// Novo handle de escrita. A escrita não passa pela thread de leitura
    /// (ADR-0007) — quem chama decide como serializar chamadas de escrita.
    pub fn writer(&self) -> Result<Box<dyn Write + Send>, PtyError> {
        self.master
            .take_writer()
            .map_err(|e| PtyError::new("take_writer", e))
    }

    /// Redimensiona o PTY. Tamanho em pixels não é rastreado pelo handle —
    /// `0` sinaliza "desconhecido", que é o que a maioria dos programas já
    /// trata (nem todo PTY Unix reporta pixels; ConPTY também não).
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        let size = portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        self.master
            .resize(size)
            .map_err(|e| PtyError::new("resize", e))
    }

    /// Consulta não bloqueante: `None` enquanto o processo segue vivo.
    ///
    /// É o sinal de morte do processo — não a leitura retornando `Ok(0)`.
    /// No Unix os dois coincidem; no Windows, o pipe do ConPTY **não** emite
    /// EOF só porque o processo hospedado saiu — ele só fecha quando o
    /// pseudo-console é fechado (`drop` deste `PtyHandle`, que derruba o
    /// `master`). A regra "ler até EOF antes de marcar a aba como encerrada"
    /// (ADR-0004) continua valendo, mas a ordem prática é: `try_wait`
    /// detecta a morte, e só então o chamador fecha o handle para liberar
    /// a thread de leitura que ficaria bloqueada esperando um EOF que nunca
    /// viria sozinho.
    pub fn try_wait(&mut self) -> Result<Option<PtyExitStatus>, PtyError> {
        self.child
            .try_wait()
            .map(|status| status.map(PtyExitStatus::from))
            .map_err(|e| PtyError::new("try_wait", e))
    }

    /// Bloqueia até o processo encerrar. Mesma ressalva de EOF que
    /// [`PtyHandle::try_wait`] — ver ali.
    pub fn wait(&mut self) -> Result<PtyExitStatus, PtyError> {
        self.child
            .wait()
            .map(PtyExitStatus::from)
            .map_err(|e| PtyError::new("wait", e))
    }

    /// Encerra o processo à força. Usado no fechamento de aba com processo
    /// que não respondeu — não substitui `wait` no caminho normal.
    pub fn kill(&mut self) -> Result<(), PtyError> {
        self.child.kill().map_err(|e| PtyError::new("kill", e))
    }
}

/// Spawna um shell num novo PTY. Ver [`SpawnConfig`] para a resolução de
/// `program` e a precedência de ambiente.
///
/// O `ProcessGroup` (ADR-0033) vem **separado** do `PtyHandle` -- é
/// `None` fora do Windows e em qualquer falha de criação/atribuição do
/// Job Object (degradação silenciosa, ver `job.rs`). Quem chama decide
/// sozinho se droppa (mata a árvore) ou esquece (`mem::forget`, preserva
/// processo destacado) essa segunda peça, conforme o caminho de saída --
/// ver `porecatu-term::terminal::watch_loop`.
pub fn spawn(config: SpawnConfig) -> Result<(PtyHandle, Option<ProcessGroup>), PtyError> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(config.size.into())
        .map_err(|e| PtyError::new("openpty", e))?;

    let program = config.program.unwrap_or_else(|| {
        resolve_default_shell(std::env::var("SHELL").ok().as_deref(), search_path)
    });

    let mut cmd = CommandBuilder::new(program);
    cmd.args(config.args);
    if let Some(cwd) = config.cwd {
        cmd.cwd(cwd);
    }
    for (key, value) in base_env() {
        cmd.env(key, value);
    }
    for (key, value) in config.env {
        cmd.env(key, value);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| PtyError::new("spawn_command", e))?;
    // O slave em si (a ponta que o filho herdou) não precisa continuar
    // aberto neste processo; o filho já tem sua própria cópia.
    drop(pair.slave);

    let process_group = ProcessGroup::for_child(child.as_ref());

    Ok((
        PtyHandle {
            master: pair.master,
            child,
        },
        process_group,
    ))
}
