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
use crate::snapshot::GridSnapshot;

/// Intervalo de checagem de `try_wait` na thread de observação do processo.
/// Não precisa ser fino: só existe para fechar o PTY quando o processo
/// morre e a thread de leitura ficaria bloqueada esperando um EOF que, no
/// Windows, o ConPTY não emite sozinho (ver o teste de integração da
/// Etapa 1 em `porecatu-pty`).
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Folga entre detectar o fim do processo e fechar o PTY. Dá tempo dos
/// últimos bytes já escritos no pipe chegarem à thread de leitura antes do
/// fechamento cortar o canal -- ADR-0004: "ler até EOF antes de considerar
/// a aba encerrada", adaptado à realidade do ConPTY (ver `PtyHandle::wait`).
const DRAIN_GRACE_PERIOD: Duration = Duration::from_millis(50);

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
    // Mantidos só para não desanexar as threads sem querer; o encerramento
    // em si acontece sozinho quando os campos abaixo (e os `Sender`s acima)
    // são derrubados no `Drop` -- ver o comentário em `spawn`.
    _shutdown: mpsc::Sender<()>,
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
    /// Não é preciso desligar nada explicitamente: soltar o `Terminal`
    /// (`Drop`) já derruba `_shutdown` e o `Sender` de escrita, o que basta
    /// para as três threads perceberem e encerrarem sozinhas -- a thread de
    /// leitura desbloqueia quando a de observação mata o processo e fecha
    /// o PTY.
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
        let watch_thread = thread::spawn(move || watch_loop(pty, events_tx, shutdown_rx));

        Ok(Self {
            engine,
            events: events_rx,
            writer: write_tx,
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

    /// Encerra o terminal e só devolve depois que o processo foi morto e as
    /// três threads saíram -- ao contrário de simplesmente dropar (que
    /// dispara o mesmo desligamento mas não espera por ele). Chamar
    /// explicitamente antes do processo do Porecatu sair de vez (ex.: ao
    /// fechar a janela) -- sem isso, nada garante que a thread de
    /// observação tenha a chance de rodar seu próximo ciclo (até
    /// [`WATCH_POLL_INTERVAL`]) antes do processo inteiro já ter
    /// terminado, o que deixaria o processo filho órfão.
    pub fn shutdown(self) {
        drop(self._shutdown);
        drop(self.writer);
        // Não junta `_read_thread`. Ela só desbloqueia quando o PTY fecha
        // de fato (drop do master dentro de `watch_loop`, depois que a
        // thread de observação processa o sinal de desligamento acima) --
        // e verificado na prática (smoke test manual desta etapa) que essa
        // espera pode passar muito de `WATCH_POLL_INTERVAL`. Bloquear a
        // main thread nisso violaria a regra central do ADR-0007 ("a main
        // thread nunca faz I/O bloqueante") bem no caminho de fechar a
        // janela. A thread de leitura se encerra sozinha, mais cedo ou mais
        // tarde -- e o processo do Porecatu inteiro saindo (que é o que
        // `shutdown` antecede) derruba qualquer handle que ainda esteja
        // aberta de qualquer forma.
        let _ = self._write_thread.join();
        let _ = self._watch_thread.join();
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
            // Inclui o erro que aparece quando a thread de observação fecha
            // o PTY após detectar o fim do processo (ver `watch_loop`).
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

fn watch_loop(mut pty: PtyHandle, events: mpsc::Sender<TermEvent>, shutdown: mpsc::Receiver<()>) {
    loop {
        match shutdown.recv_timeout(WATCH_POLL_INTERVAL) {
            // `Ok(())` nunca é enviado de propósito hoje -- só o `Sender`
            // ser derrubado no `Drop` do `Terminal` importa -- mas tratar os
            // dois casos junto deixa a intenção clara: qualquer sinal de
            // desligamento mata o processo e libera a thread de leitura.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                let _ = pty.kill();
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Ok(Some(status)) = pty.try_wait() {
                    let _ = events.send(TermEvent::Exit {
                        success: status.success,
                        code: status.code,
                    });
                    thread::sleep(DRAIN_GRACE_PERIOD);
                    // Fecha o pseudo-console -- no Windows é o único jeito
                    // de a thread de leitura ver EOF, porque o ConPTY não
                    // fecha o pipe só porque o processo hospedado saiu.
                    drop(pty);
                    break;
                }
            }
        }
    }
}
