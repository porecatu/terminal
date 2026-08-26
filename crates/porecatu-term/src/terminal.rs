// SPDX-License-Identifier: GPL-3.0-or-later

//! Terminal completo: motor VT + PTY + threading (docs/arquitetura.md
//! seção 2, ADR-0007). `porecatu-term` é o único crate autorizado a
//! depender de `porecatu-pty` (ver tabela de dependências do CLAUDE.md) --
//! por isso é aqui, e não em `porecatu-ui`, que a thread de leitura mora.
//!
//! `porecatu-ui` nunca vê `porecatu_pty::PtyHandle` nem thread nenhuma:
//! só chama [`Terminal::spawn`], escreve bytes, pede snapshot e drena
//! eventos. A notificação de "algo mudou" sai por um closure genérico
//! (`on_wakeup`) para não este crate não precisar conhecer `winit`
//! (`porecatu-term` não conhece GUI) -- é `porecatu-ui` quem fecha esse
//! closure sobre o `EventLoopProxy` e o `Wakeup { window, tab }` dele.

use std::io::{Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::{fmt, io};

use porecatu_pty::{PtyError, PtyHandle, SpawnConfig};

use crate::engine::{TermEngine, TermSize};
use crate::event::TermEvent;
use crate::params::TermParams;
use crate::scroll::TermScroll;
use crate::snapshot::{GridSnapshot, TermModes};

/// Intervalo de checagem de `try_wait` na thread de observação do processo.
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Rede de segurança de [`Terminal::shutdown`]: tempo máximo de espera pela
/// confirmação de que o processo foi morto, antes de desistir e devolver
/// de qualquer jeito. Não é o mecanismo principal (esse é o canal
/// `killed`) -- é só para que uma regressão aqui feche o app em vez de
/// travá-lo para sempre, o mesmo bug que esta etapa corrigiu.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub enum TerminalSpawnError {
    Pty(PtyError),
}

impl fmt::Display for TerminalSpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TerminalSpawnError::Pty(err) => write!(f, "terminal: {err}"),
        }
    }
}

impl std::error::Error for TerminalSpawnError {}

impl From<PtyError> for TerminalSpawnError {
    fn from(err: PtyError) -> Self {
        TerminalSpawnError::Pty(err)
    }
}

/// Um terminal rodando: motor VT + PTY + as threads que os conectam.
/// `porecatu-ui` interage só por aqui -- `write`, `snapshot_into`,
/// `try_recv_event`.
pub struct Terminal {
    engine: Arc<Mutex<TermEngine>>,
    events: mpsc::Receiver<TermEvent>,
    writer: mpsc::Sender<Vec<u8>>,
    /// Confirmação de "processo morto", mandada pela thread de observação
    /// quando `shutdown` sinaliza desligamento -- ver o comentário lá.
    killed: mpsc::Receiver<()>,
    /// Novo tamanho para o PTY (linhas, colunas) -- a thread de
    /// observação, dona do `PtyHandle`, é quem aplica (ver `watch_loop`).
    /// O lado do motor (`engine`) é resizado direto e síncrono em
    /// `Terminal::resize`, sem passar por canal nenhum.
    resize: mpsc::Sender<(u16, u16)>,
    _shutdown: mpsc::Sender<()>,
    // Mantidos só para não desanexar as threads sem querer, não para dar
    // `join`: nenhuma delas tem retorno garantido -- ver `watch_loop`.
    _read_thread: JoinHandle<()>,
    _write_thread: JoinHandle<()>,
    _watch_thread: JoinHandle<()>,
}

impl Terminal {
    /// Spawna o PTY (dimensão vem de `pty_config.size`) e o motor VT, e
    /// sobe as três threads do terminal: leitura, escrita e observação do
    /// processo (ADR-0007 seção "Distribuição de trabalho").
    ///
    /// `on_wakeup` é chamado pela thread de leitura toda vez que bytes
    /// novos foram aplicados ao grid -- deve só marcar sujeira e devolver
    /// rápido, nunca bloquear (é chamado com a grade *destrancada*, mas
    /// ainda assim é código rodando fora da main thread).
    ///
    /// Para desligar de propósito (ex.: fechar a janela), chamar
    /// [`Terminal::shutdown`] -- ela garante que o processo foi morto antes
    /// de devolver. Simplesmente dropar mata o processo mais cedo ou mais
    /// tarde (a thread de observação nota o `Sender` de desligamento caído
    /// dentro de [`WATCH_POLL_INTERVAL`]), mas não espera por isso.
    pub fn spawn(
        pty_config: SpawnConfig,
        term_params: TermParams,
        on_wakeup: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, TerminalSpawnError> {
        let size = TermSize {
            rows: pty_config.size.rows as usize,
            cols: pty_config.size.cols as usize,
        };

        let pty = porecatu_pty::spawn(pty_config)?;
        let reader = pty.reader()?;
        let writer_handle = pty.writer()?;

        let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>();
        let write_thread = thread::spawn(move || write_loop(writer_handle, write_rx));

        let (events_tx, events_rx) = mpsc::channel();
        let engine = TermEngine::new(term_params, size, events_tx.clone(), write_tx.clone());
        let engine = Arc::new(Mutex::new(engine));

        let on_wakeup = Arc::new(on_wakeup);
        let read_thread = {
            let engine = Arc::clone(&engine);
            let on_wakeup = Arc::clone(&on_wakeup);
            thread::spawn(move || read_loop(reader, engine, on_wakeup))
        };

        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let (killed_tx, killed_rx) = mpsc::channel::<()>();
        let (resize_tx, resize_rx) = mpsc::channel::<(u16, u16)>();
        let watch_thread =
            thread::spawn(move || watch_loop(pty, events_tx, shutdown_rx, killed_tx, resize_rx));

        Ok(Self {
            engine,
            events: events_rx,
            writer: write_tx,
            killed: killed_rx,
            resize: resize_tx,
            _shutdown: shutdown_tx,
            _read_thread: read_thread,
            _write_thread: write_thread,
            _watch_thread: watch_thread,
        })
    }

    /// Envia bytes para o processo. Não passa pela thread de leitura
    /// (ADR-0007) -- vai por um canal próprio para a thread de escrita, que
    /// nunca bloqueia a main thread mesmo se o pipe do PTY estiver cheio.
    pub fn write(&self, bytes: Vec<u8>) {
        let _ = self.writer.send(bytes);
    }

    /// Preenche `out` com o estado atual do grid, travando o motor só
    /// durante a cópia (ADR-0007: "snapshot antes do desenho").
    pub fn snapshot_into(&self, out: &mut GridSnapshot) {
        let engine = lock(&self.engine);
        engine.snapshot_into(out);
    }

    /// Drena um evento pendente (título, bell, clipboard, fim de processo
    /// -- seção 4.3), se houver. Chamar em loop até `None` a cada wakeup.
    pub fn try_recv_event(&self) -> Option<TermEvent> {
        self.events.try_recv().ok()
    }

    /// Rola o scrollback (PRD-010 RF-10.12 a RF-10.14). Só o motor sabe
    /// disso -- não precisa do PTY, então é síncrono, sem canal.
    pub fn scroll(&self, scroll: TermScroll) {
        lock(&self.engine).scroll(scroll);
    }

    /// Modos atuais do terminal (ADR-0008/0013) -- para decidir como
    /// rotear teclado/roda sem depender do snapshot do último frame
    /// renderizado, que pode estar obsoleto.
    pub fn modes(&self) -> TermModes {
        lock(&self.engine).modes()
    }

    /// Redimensiona a grade. O lado do motor acontece agora, síncrono
    /// (travando o motor por um instante, igual a qualquer outro acesso);
    /// o lado do PTY (`ioctl`/`ResizePseudoConsole`, o que avisa o
    /// processo filho) é aplicado pela thread de observação, que é quem
    /// tem o `PtyHandle` -- ver `watch_loop`. Perder um resize em trânsito
    /// não é grave: o próximo `Resized` da janela manda o tamanho atual de
    /// novo.
    pub fn resize(&self, rows: usize, cols: usize) {
        lock(&self.engine).resize(rows, cols);
        let _ = self.resize.send((rows as u16, cols as u16));
    }

    /// Encerra o terminal: mata o processo e só devolve depois que a
    /// thread de observação confirmou isso -- ao contrário de simplesmente
    /// dropar (que dispara o mesmo sinal mas não espera por ele). Chamar
    /// explicitamente antes do processo do Porecatu sair de vez (ex.: ao
    /// fechar a janela), para não deixar o processo filho órfão.
    ///
    /// Não dá `join` em nenhuma das três threads: nenhuma tem retorno
    /// garantido. A de leitura nunca mais desbloqueia sozinha depois disso
    /// -- ver o comentário de [`watch_loop`] sobre por que este crate
    /// desistiu de fechar o pseudo-console -- e ela mantém viva a cópia do
    /// canal de escrita que `TermEngine` usa para respostas automáticas
    /// (DSR/DA/CPR), o que por sua vez faria a thread de escrita nunca ver
    /// o canal fechar. A confirmação de que o processo morreu vem por um
    /// canal dedicado (`killed`), não de esperar threads retornarem.
    pub fn shutdown(self) {
        drop(self._shutdown);
        let _ = self.killed.recv_timeout(SHUTDOWN_TIMEOUT);
    }
}

fn lock(engine: &Mutex<TermEngine>) -> std::sync::MutexGuard<'_, TermEngine> {
    engine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read_loop(
    mut reader: Box<dyn Read + Send>,
    engine: Arc<Mutex<TermEngine>>,
    on_wakeup: Arc<dyn Fn() + Send + Sync>,
) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                {
                    let mut engine = lock(&engine);
                    engine.advance(&buf[..n]);
                } // lock liberado antes do wakeup, nunca durante render (ADR-0007)
                on_wakeup();
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            // Na prática, quase nunca acontece: o PTY nunca é fechado por
            // este crate (ver `watch_loop`), então isto só dispara num
            // erro genuíno de I/O -- não no fim normal do processo, que
            // fica só bloqueado aqui para sempre (aceito, ver `leak_pty`).
            Err(_) => break,
        }
    }
}

fn write_loop(mut writer: Box<dyn Write + Send>, rx: mpsc::Receiver<Vec<u8>>) {
    while let Ok(bytes) = rx.recv() {
        if writer.write_all(&bytes).is_err() {
            break;
        }
    }
}

/// Nunca fecha o PTY (nunca dropa `pty` de verdade -- ver [`leak_pty`]).
///
/// A tentativa original era: detectar o fim do processo via `try_wait`
/// (ADR-0004 -- no Windows o ConPTY não emite EOF só porque o processo
/// hospedado saiu) e então fechar o pseudo-console para a thread de
/// leitura ver EOF. Bug real, encontrado no smoke test manual da Etapa 3:
/// fechar a janela travava o app inteiro, e só morria com kill externo.
///
/// Causa: `ClosePseudoConsole` (chamado pelo `Drop` do master do
/// `portable-pty`) bloqueia até a ponta de leitura clonada que a thread de
/// leitura segura ser liberada -- e não existe jeito seguro de fechar essa
/// handle de outra thread no Windows enquanto uma leitura síncrona está
/// parada nela (é a mesma classe de UB que fechar um fd em uso). Como as
/// duas threads (esta e a de leitura) esperam uma pela outra, é deadlock
/// de verdade, não só lento.
fn watch_loop(
    mut pty: PtyHandle,
    events: mpsc::Sender<TermEvent>,
    shutdown: mpsc::Receiver<()>,
    killed: mpsc::Sender<()>,
    resize: mpsc::Receiver<(u16, u16)>,
) {
    loop {
        match shutdown.recv_timeout(WATCH_POLL_INTERVAL) {
            // `Ok(())` nunca é enviado de propósito hoje -- só o `Sender`
            // ser derrubado no `Drop` do `Terminal` importa -- mas tratar os
            // dois casos junto deixa a intenção clara: qualquer sinal de
            // desligamento mata o processo.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                let _ = pty.kill();
                let _ = killed.send(());
                leak_pty(pty);
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                // Se chegou mais de um resize desde o último ciclo, só o
                // mais recente importa -- os intermediários já estão
                // obsoletos.
                let mut latest_resize = None;
                while let Ok(size) = resize.try_recv() {
                    latest_resize = Some(size);
                }
                if let Some((rows, cols)) = latest_resize {
                    let _ = pty.resize(rows, cols);
                }

                if let Ok(Some(status)) = pty.try_wait() {
                    let _ = events.send(TermEvent::Exit {
                        success: status.success,
                        code: status.code,
                    });
                    leak_pty(pty);
                    return;
                }
            }
        }
    }
}

/// Deliberadamente não fecha o PTY -- ver o porquê em [`watch_loop`].
/// `mem::forget` evita rodar o destrutor (`ClosePseudoConsole` no
/// Windows); o SO reclama a handle (e todas as outras do processo) quando
/// o `porecatu` inteiro sai. Consequência aceita: a thread de leitura fica
/// parada num `read()` que nunca mais retorna, pelo resto da vida do app
/// -- não é um recurso do SO vazando (o processo filho já foi morto por
/// `kill()` antes disso), só uma thread e sua pilha ociosas.
fn leak_pty(pty: PtyHandle) {
    std::mem::forget(pty);
}
