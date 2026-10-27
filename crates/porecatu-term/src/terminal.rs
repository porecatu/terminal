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
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::{fmt, io};

use porecatu_pty::PtyError;

use crate::backend::{Backend, BackendSpec, Finish, Inspect, Liveness, PtyBackend};
use crate::engine::{TermEngine, TermSize};
use crate::event::TermEvent;
use crate::params::TermParams;
use crate::prompt::{Answer, PromptGuard, PromptId, PromptSink};
use crate::scroll::TermScroll;
use crate::search::{InvalidPattern, SearchJob, SearchMode, SearchStep};
use crate::selection::{SelectionKind, SelectionSide};
use crate::snapshot::{CursorShape, GridSnapshot, TermModes};

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
    /// O backend subiu mas não entregou o leitor ou o escritor.
    Stream(io::Error),
}

impl fmt::Display for TerminalSpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TerminalSpawnError::Pty(err) => write!(f, "terminal: {err}"),
            TerminalSpawnError::Stream(err) => write!(f, "terminal: stream: {err}"),
        }
    }
}

impl std::error::Error for TerminalSpawnError {}

impl From<io::Error> for TerminalSpawnError {
    fn from(err: io::Error) -> Self {
        TerminalSpawnError::Stream(err)
    }
}

impl From<PtyError> for TerminalSpawnError {
    fn from(err: PtyError) -> Self {
        TerminalSpawnError::Pty(err)
    }
}

/// Qual sessão de backend é a corrente (`Terminal::reattach` troca a
/// corrente). A thread de leitura e a de observação de uma sessão antiga só
/// agem sobre o motor e sobre os eventos enquanto a sessão delas é a corrente.
#[derive(Clone)]
struct Generation {
    current: Arc<AtomicU64>,
    mine: u64,
}

impl Generation {
    fn is_current(&self) -> bool {
        self.current.load(Ordering::SeqCst) == self.mine
    }
}

/// Um backend pronto para subir: já com o canal de perguntas e com o leitor e
/// o escritor tirados. Tudo o que pode falhar acontece aqui, antes de
/// `reattach` encerrar o backend anterior.
struct Prepared {
    backend: Box<dyn Backend>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
}

impl Prepared {
    fn new(mut backend: Box<dyn Backend>, prompts: PromptSink) -> io::Result<Self> {
        backend.set_prompts(prompts);
        let reader = backend.reader()?;
        let writer = backend.writer()?;
        Ok(Self {
            backend,
            reader,
            writer,
        })
    }

    fn from_spec(spec: BackendSpec, prompts: PromptSink) -> Result<Self, TerminalSpawnError> {
        let backend: Box<dyn Backend> = match spec {
            BackendSpec::Local(config) => Box::new(PtyBackend::spawn(config)?),
        };
        Ok(Self::new(backend, prompts)?)
    }
}

/// As três threads de um backend e os canais que as ligam ao resto: o que
/// `reattach` troca por inteiro.
struct Session {
    writer: mpsc::Sender<Vec<u8>>,
    /// Confirmação de "processo morto", mandada pela thread de observação
    /// quando `shutdown` sinaliza desligamento -- ver o comentário lá.
    killed: mpsc::Receiver<()>,
    /// Cópia deste lado (thread da UI) do que o backend sabe responder
    /// (`liveness`, `cwd_fallback`; ADR-0034, ADR-0038) -- a thread de
    /// observação leva o backend em si. Só para consulta: quem decide como o
    /// backend termina é sempre `watch_loop`.
    inspect: Arc<dyn Inspect>,
    /// Novo tamanho para o backend (linhas, colunas) -- a thread de
    /// observação, dona dele, é quem aplica (ver `watch_loop`).
    /// O lado do motor (`engine`) é resizado direto e síncrono em
    /// `Terminal::resize`, sem passar por canal nenhum.
    resize: mpsc::Sender<(u16, u16)>,
    /// Dropar isto é o sinal de desligamento de `watch_loop`.
    _shutdown: mpsc::Sender<()>,
    // Mantidos só para não desanexar as threads sem querer, não para dar
    // `join`: nenhuma delas tem retorno garantido -- ver `watch_loop`.
    _read_thread: JoinHandle<()>,
    _write_thread: JoinHandle<()>,
    _watch_thread: JoinHandle<()>,
}

impl Session {
    /// Sobe as três threads sobre `prepared`. `write_rx` é a ponta de leitura
    /// do canal de escrita cujo `Sender` o motor já tem (ver `reattach`).
    fn start(
        prepared: Prepared,
        write_tx: mpsc::Sender<Vec<u8>>,
        write_rx: mpsc::Receiver<Vec<u8>>,
        engine: &Arc<Mutex<TermEngine>>,
        events_tx: mpsc::Sender<TermEvent>,
        on_wakeup: &Arc<dyn Fn() + Send + Sync>,
        generation: Generation,
    ) -> Self {
        let Prepared {
            backend,
            reader,
            writer: writer_handle,
        } = prepared;
        let inspect = backend.inspector();

        let write_thread = thread::spawn(move || write_loop(writer_handle, write_rx));

        let read_thread = {
            let engine = Arc::clone(engine);
            let on_wakeup = Arc::clone(on_wakeup);
            let generation = generation.clone();
            thread::spawn(move || read_loop(reader, engine, on_wakeup, generation))
        };

        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let (killed_tx, killed_rx) = mpsc::channel::<()>();
        let (resize_tx, resize_rx) = mpsc::channel::<(u16, u16)>();
        let watch_thread = {
            let on_wakeup = Arc::clone(on_wakeup);
            thread::spawn(move || {
                watch_loop(
                    backend,
                    events_tx,
                    shutdown_rx,
                    killed_tx,
                    resize_rx,
                    on_wakeup,
                    generation,
                )
            })
        };

        Self {
            writer: write_tx,
            killed: killed_rx,
            inspect,
            resize: resize_tx,
            _shutdown: shutdown_tx,
            _read_thread: read_thread,
            _write_thread: write_thread,
            _watch_thread: watch_thread,
        }
    }
}

/// Um terminal rodando: motor VT + backend + as threads que os conectam.
/// `porecatu-ui` interage só por aqui -- `write`, `snapshot_into`,
/// `try_recv_event`.
pub struct Terminal {
    engine: Arc<Mutex<TermEngine>>,
    events: mpsc::Receiver<TermEvent>,
    /// Cópia do canal de eventos, para as threads de um backend novo.
    events_tx: mpsc::Sender<TermEvent>,
    on_wakeup: Arc<dyn Fn() + Send + Sync>,
    generation: Arc<AtomicU64>,
    /// Perguntas abertas da sessão corrente; cancela as que sobram quando
    /// cai (ADR-0071 §5).
    prompts: PromptGuard,
    session: Session,
}

impl Terminal {
    /// Spawna o backend (dimensão vem de `pty_config.size`) e o motor VT, e
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
        spec: BackendSpec,
        term_params: TermParams,
        on_wakeup: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, TerminalSpawnError> {
        let (rows, cols) = spec.grid_size();
        Self::spawn_with(
            move |prompts| Prepared::from_spec(spec, prompts),
            TermSize { rows, cols },
            term_params,
            Arc::new(on_wakeup),
        )
    }

    /// `spawn` sobre um backend já escolhido -- o caminho dos testes, que
    /// sobem um backend falso.
    fn spawn_with(
        prepare: impl FnOnce(PromptSink) -> Result<Prepared, TerminalSpawnError>,
        size: TermSize,
        term_params: TermParams,
        on_wakeup: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, TerminalSpawnError> {
        let (events_tx, events_rx) = mpsc::channel();
        let prompts = PromptGuard::new();
        let prepared = prepare(prompts.sink(events_tx.clone(), Arc::clone(&on_wakeup)))?;

        let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>();
        let engine = TermEngine::new(term_params, size, events_tx.clone(), write_tx.clone());
        let engine = Arc::new(Mutex::new(engine));

        let generation = Arc::new(AtomicU64::new(0));
        let session = Session::start(
            prepared,
            write_tx,
            write_rx,
            &engine,
            events_tx.clone(),
            &on_wakeup,
            Generation {
                current: Arc::clone(&generation),
                mine: 0,
            },
        );

        Ok(Self {
            engine,
            events: events_rx,
            events_tx,
            on_wakeup,
            generation,
            prompts,
            session,
        })
    }

    /// Encerra o backend atual (se ainda vivo) e liga outro ao **mesmo
    /// motor**: grade, scrollback, seleção e busca ficam (ADR-0071 §4). É o
    /// que a reconexão SSH usa.
    ///
    /// O backend novo é preparado primeiro: se não subir, o atual segue como
    /// estava e o erro volta. Depois, numa só seção sob o lock do motor, a
    /// geração é trocada (a leitura antiga deixa de aplicar bytes), os modos
    /// que a sessão antiga ligou são desligados (`TermEngine::soft_reset`) e o
    /// motor recebe o canal de escrita novo -- tudo antes de a primeira
    /// thread nova ler. As threads antigas terminam pelo caminho de sempre
    /// (sinal de desligamento, `finish(Killed)`), sem esperar; as perguntas
    /// que ficaram abertas recebem `Cancel`.
    pub fn reattach(&mut self, spec: BackendSpec) -> Result<(), TerminalSpawnError> {
        let prompts = PromptGuard::new();
        let prepared = Prepared::from_spec(
            spec,
            prompts.sink(self.events_tx.clone(), Arc::clone(&self.on_wakeup)),
        )?;
        self.install(prepared, prompts);
        Ok(())
    }

    fn install(&mut self, prepared: Prepared, prompts: PromptGuard) {
        let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>();
        let (rows, cols) = {
            let mut engine = lock(&self.engine);
            self.generation.fetch_add(1, Ordering::SeqCst);
            engine.soft_reset();
            engine.set_pty_writer(write_tx.clone());
            engine.size()
        };
        let generation = Generation {
            current: Arc::clone(&self.generation),
            mine: self.generation.load(Ordering::SeqCst),
        };

        // Cancela as perguntas da sessão que sai.
        self.prompts = prompts;
        let session = Session::start(
            prepared,
            write_tx,
            write_rx,
            &self.engine,
            self.events_tx.clone(),
            &self.on_wakeup,
            generation,
        );
        // O backend novo nasce com o tamanho do spawn, que pode não ser o da
        // grade de agora.
        let _ = session.resize.send((rows as u16, cols as u16));
        // A sessão antiga cai aqui: o `Sender` de desligamento dela é o sinal.
        drop(std::mem::replace(&mut self.session, session));
    }

    /// Responde a uma pergunta do backend (`TermEvent::Prompt`). Id
    /// desconhecido ou já respondido é ignorado.
    pub fn answer(&self, id: PromptId, answer: Answer) {
        self.prompts.answer(id, answer);
    }

    /// Envia bytes para o processo. Não passa pela thread de leitura
    /// (ADR-0007) -- vai por um canal próprio para a thread de escrita, que
    /// nunca bloqueia a main thread mesmo se o pipe do PTY estiver cheio.
    pub fn write(&self, bytes: Vec<u8>) {
        let _ = self.session.writer.send(bytes);
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

    /// O que o backend tem de vivo além do shell (ADR-0034, ADR-0071 §6):
    /// `ExtraProcesses` quando há mais processo na árvore do PTY -- ex.:
    /// `node server.js` rodando em primeiro plano --, `Idle` quando não, e
    /// também quando o sinal não pôde ser lido: o lado seguro do erro é não
    /// confirmar, e o modo do terminal (`modes()`) continua cobrindo esse
    /// caso. A contagem em si é a união de duas fontes (Job + varredura por
    /// `sysinfo`) -- ver `ProcessGroup::process_count` em `porecatu-pty`.
    pub fn liveness(&self) -> Liveness {
        self.session.inspect.liveness()
    }

    /// RF-3.10/ADR-0038, segundo degrau da precedência de `cwd` na
    /// gravação de sessão (o primeiro, `Tab::cwd` por OSC 7, é decisão de
    /// `porecatu-ui`, que não conhece o backend). Ver
    /// `PtyBackend::cwd_fallback` para o que cada plataforma devolve.
    pub fn cwd_fallback(&self) -> Option<PathBuf> {
        self.session.inspect.cwd_fallback()
    }

    /// Inicia uma seleção (PRD-010 RF-10.4).
    pub fn start_selection(
        &self,
        kind: SelectionKind,
        row: usize,
        col: usize,
        side: SelectionSide,
    ) {
        lock(&self.engine).start_selection(kind, row, col, side);
    }

    /// Estende a seleção em andamento.
    pub fn update_selection(&self, row: usize, col: usize, side: SelectionSide) {
        lock(&self.engine).update_selection(row, col, side);
    }

    /// Limpa a seleção.
    pub fn clear_selection(&self) {
        lock(&self.engine).clear_selection();
    }

    /// Texto selecionado, pronto para o clipboard (RF-10.6). `None` sem
    /// seleção ativa.
    pub fn selection_text(&self) -> Option<String> {
        lock(&self.engine).selection_text()
    }

    /// Seleciona a tela visível inteira e o scrollback (RF-11.16). Ver
    /// `TermEngine::select_all`.
    pub fn select_all(&self) {
        lock(&self.engine).select_all();
    }

    /// Prepara uma busca no scrollback (ADR-0041). Ver
    /// `TermEngine::start_search`.
    pub fn start_search(
        &self,
        pattern: &str,
        mode: SearchMode,
        lines_per_step: usize,
    ) -> Result<SearchJob, InvalidPattern> {
        lock(&self.engine).start_search(pattern, mode, lines_per_step)
    }

    /// Varre um lote de uma busca em andamento (ADR-0041). Ver
    /// `TermEngine::step_search`.
    pub fn step_search(&self, job: &mut SearchJob) -> SearchStep {
        lock(&self.engine).step_search(job)
    }

    /// Redimensiona a grade. O lado do motor acontece agora, síncrono
    /// (travando o motor por um instante, igual a qualquer outro acesso);
    /// o lado do PTY (`ioctl`/`ResizePseudoConsole`, o que avisa o
    /// processo filho) é aplicado pela thread de observação, que é quem
    /// tem o `PtyHandle` -- ver `watch_loop`. Tamanho igual ao atual do
    /// motor não chega ao PTY: o ConPTY repinta a tela a cada resize, e
    /// restaurar uma janela minimizada manda de novo o tamanho que ela já
    /// tinha.
    pub fn resize(&self, rows: usize, cols: usize) {
        if lock(&self.engine).resize(rows, cols) {
            let _ = self.session.resize.send((rows as u16, cols as u16));
        }
    }

    /// Hot reload de `[terminal.cursor]`: troca o cursor de default sem
    /// tocar o que o programa definiu por DECSCUSR (RF-5.25).
    pub fn set_default_cursor_style(&self, shape: CursorShape, blinking: bool) {
        lock(&self.engine).set_default_cursor_style(shape, blinking);
    }

    /// Injeta uma nota estilizada no grid, como se fosse saída do programa
    /// (RF-1.3, ADR-0017 item 5) -- ex.: código de saída de um processo que
    /// terminou com erro. `rgb` normalmente é o destaque do ADR-0014.
    pub fn inject_note(&self, text: &str, rgb: (u8, u8, u8)) {
        lock(&self.engine).inject_note(text, rgb);
    }

    /// Sinaliza o processo para morrer e devolve **na hora**, sem esperar
    /// confirmação -- é o caminho de `tab.close` (ADR-0017 item 4: fechar
    /// aba não bloqueia a main thread). Dropar `self` sem chamar nada
    /// teria o mesmo efeito de sinalização (é a queda do `_shutdown`
    /// dentro que `watch_loop` nota), mas `close` deixa a intenção
    /// explícita e devolve o [`ShutdownWait`] para quem eventualmente
    /// precisar da confirmação (ex.: fechamento de janela).
    ///
    /// Não dá `join` em nenhuma das três threads: nenhuma tem retorno
    /// garantido. A de leitura nunca mais desbloqueia sozinha depois disso
    /// -- ver o comentário de [`watch_loop`] sobre por que este crate
    /// desistiu de fechar o pseudo-console -- e ela mantém viva a cópia do
    /// canal de escrita que `TermEngine` usa para respostas automáticas
    /// (DSR/DA/CPR), o que por sua vez faria a thread de escrita nunca ver
    /// o canal fechar.
    pub fn close(self) -> ShutdownWait {
        // `self._shutdown` (e o resto de `self`) dropam ao fim desta
        // função -- é essa queda do sender que `watch_loop` nota como
        // sinal de desligamento, no braço `Err(RecvTimeoutError::Disconnected)`.
        ShutdownWait {
            killed: self.session.killed,
        }
    }

    /// Encerra o terminal e só devolve depois que a thread de observação
    /// confirmou o processo morto (ou o timeout de segurança vencer) --
    /// equivalente a `self.close().wait()`. Usar só quando o chamador
    /// precisa garantir o processo morto antes de seguir -- ex.: fechar a
    /// janela, onde bloquear por uma volta de sinalização é aceitável e
    /// necessário para não deixar o filho órfão. `tab.close` (caminho
    /// interativo, alta frequência) usa [`Terminal::close`] sem esperar.
    pub fn shutdown(self) {
        self.close().wait();
    }
}

/// Confirmação pendente de [`Terminal::close`] de que o processo morreu.
/// Aguardar é opcional -- o processo morre de qualquer jeito, quem não
/// chama [`ShutdownWait::wait`] simplesmente não sabe quando isso
/// aconteceu.
pub struct ShutdownWait {
    killed: mpsc::Receiver<()>,
}

impl ShutdownWait {
    /// Bloqueia até a confirmação chegar ou o timeout de segurança vencer.
    pub fn wait(self) {
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
    generation: Generation,
) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                {
                    let mut engine = lock(&engine);
                    // `reattach` troca a geração sob este mesmo lock: depois
                    // dele, nenhum byte de uma sessão antiga chega ao motor.
                    if !generation.is_current() {
                        return;
                    }
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

/// Nunca fecha o PTY (nunca dropa o `PtyHandle` de verdade -- ver
/// `PtyBackend::finish`, onde mora o que é específico do PTY).
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
    mut backend: Box<dyn Backend>,
    events: mpsc::Sender<TermEvent>,
    shutdown: mpsc::Receiver<()>,
    killed: mpsc::Sender<()>,
    resize: mpsc::Receiver<(u16, u16)>,
    on_wakeup: Arc<dyn Fn() + Send + Sync>,
    generation: Generation,
) {
    loop {
        match shutdown.recv_timeout(WATCH_POLL_INTERVAL) {
            // `Ok(())` nunca é enviado de propósito hoje -- só o `Sender`
            // ser derrubado no `Drop` do `Terminal` importa -- mas tratar os
            // dois casos junto deixa a intenção clara: qualquer sinal de
            // desligamento mata o processo.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                backend.kill();
                let _ = killed.send(());
                backend.finish(Finish::Killed);
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
                    let _ = backend.resize(rows, cols);
                }

                if let Some(reason) = backend.try_wait() {
                    // Sessão já trocada por `reattach`: o fim dela não é
                    // notícia para quem está na sessão nova.
                    if !generation.is_current() {
                        backend.finish(Finish::Ended);
                        return;
                    }
                    let _ = events.send(TermEvent::Exit(reason));
                    // Diferente do `read_loop`, nada mais vai chamar
                    // `on_wakeup` depois disto -- a thread de leitura já
                    // pode estar parada num `read()` que nunca retorna
                    // (ver `PtyBackend::finish`). Sem isto, `TermEvent::Exit`
                    // fica no canal sem ninguém ser avisado para ir buscá-lo.
                    on_wakeup();
                    backend.finish(Finish::Ended);
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::backend::EndReason;

    /// Backend de teste: termina depois de `exit_after` chamadas de
    /// `try_wait` (nunca, se `None`) e anota o que o `watch_loop` fez.
    struct FakeBackend {
        log: Arc<Mutex<Vec<String>>>,
        exit_after: Option<usize>,
        polls: usize,
    }

    impl FakeBackend {
        fn boxed(log: &Arc<Mutex<Vec<String>>>, exit_after: Option<usize>) -> Box<dyn Backend> {
            Box::new(Self {
                log: Arc::clone(log),
                exit_after,
                polls: 0,
            })
        }

        fn note(&self, entry: impl Into<String>) {
            self.log.lock().unwrap().push(entry.into());
        }
    }

    impl Backend for FakeBackend {
        fn reader(&mut self) -> io::Result<Box<dyn Read + Send>> {
            Ok(Box::new(io::empty()))
        }

        fn writer(&mut self) -> io::Result<Box<dyn Write + Send>> {
            Ok(Box::new(io::sink()))
        }

        fn resize(&mut self, rows: u16, cols: u16) -> io::Result<()> {
            self.note(format!("resize {rows}x{cols}"));
            Ok(())
        }

        fn try_wait(&mut self) -> Option<EndReason> {
            self.polls += 1;
            match self.exit_after {
                Some(n) if self.polls >= n => Some(EndReason::Exited {
                    success: true,
                    code: 0,
                }),
                _ => None,
            }
        }

        fn kill(&mut self) {
            self.note("kill");
        }

        fn finish(self: Box<Self>, how: Finish) {
            self.note(format!("finish {how:?}"));
        }

        fn inspector(&self) -> Arc<dyn Inspect> {
            Arc::new(FakeInspect)
        }
    }

    struct FakeInspect;

    impl Inspect for FakeInspect {
        fn liveness(&self) -> Liveness {
            Liveness::Idle
        }

        fn cwd_fallback(&self) -> Option<PathBuf> {
            None
        }
    }

    fn first_generation() -> Generation {
        Generation {
            current: Arc::new(AtomicU64::new(0)),
            mine: 0,
        }
    }

    /// Lado do teste de um [`ScriptedBackend`]: empurra bytes para o leitor
    /// dele, lê o que o terminal escreveu e alcança o canal de perguntas.
    struct Script {
        feed: mpsc::Sender<Vec<u8>>,
        written: Arc<Mutex<Vec<u8>>>,
        prompts: Arc<Mutex<Option<PromptSink>>>,
    }

    impl Script {
        fn feed(&self, bytes: &str) {
            self.feed.send(bytes.as_bytes().to_vec()).unwrap();
        }

        fn written(&self) -> Vec<u8> {
            self.written.lock().unwrap().clone()
        }

        fn sink(&self) -> PromptSink {
            self.prompts.lock().unwrap().clone().expect("no sink given")
        }
    }

    struct ChannelReader {
        rx: mpsc::Receiver<Vec<u8>>,
        pending: Vec<u8>,
    }

    impl Read for ChannelReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pending.is_empty() {
                match self.rx.recv() {
                    Ok(bytes) => self.pending = bytes,
                    Err(_) => return Ok(0),
                }
            }
            let n = self.pending.len().min(buf.len());
            buf[..n].copy_from_slice(&self.pending[..n]);
            self.pending.drain(..n);
            Ok(n)
        }
    }

    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Backend que nunca termina: o que o teste empurra por [`Script::feed`]
    /// é o que o motor lê.
    struct ScriptedBackend {
        rx: Option<mpsc::Receiver<Vec<u8>>>,
        written: Arc<Mutex<Vec<u8>>>,
        prompts: Arc<Mutex<Option<PromptSink>>>,
    }

    fn scripted() -> (Box<dyn Backend>, Script) {
        let (feed, rx) = mpsc::channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let prompts = Arc::new(Mutex::new(None));
        let backend = ScriptedBackend {
            rx: Some(rx),
            written: Arc::clone(&written),
            prompts: Arc::clone(&prompts),
        };
        (
            Box::new(backend),
            Script {
                feed,
                written,
                prompts,
            },
        )
    }

    impl Backend for ScriptedBackend {
        fn set_prompts(&mut self, prompts: PromptSink) {
            *self.prompts.lock().unwrap() = Some(prompts);
        }

        fn reader(&mut self) -> io::Result<Box<dyn Read + Send>> {
            Ok(Box::new(ChannelReader {
                rx: self.rx.take().expect("reader taken once"),
                pending: Vec::new(),
            }))
        }

        fn writer(&mut self) -> io::Result<Box<dyn Write + Send>> {
            Ok(Box::new(SharedWriter(Arc::clone(&self.written))))
        }

        fn resize(&mut self, _rows: u16, _cols: u16) -> io::Result<()> {
            Ok(())
        }

        fn try_wait(&mut self) -> Option<EndReason> {
            None
        }

        fn kill(&mut self) {}

        fn finish(self: Box<Self>, _how: Finish) {}

        fn inspector(&self) -> Arc<dyn Inspect> {
            Arc::new(FakeInspect)
        }
    }

    fn start() -> (Terminal, Script) {
        let (backend, script) = scripted();
        let terminal = Terminal::spawn_with(
            |prompts| Ok(Prepared::new(backend, prompts)?),
            TermSize { rows: 10, cols: 40 },
            TermParams::default(),
            Arc::new(|| {}),
        )
        .unwrap();
        (terminal, script)
    }

    /// `reattach` com um backend de teste.
    fn reattach(terminal: &mut Terminal) -> Script {
        let (backend, script) = scripted();
        let prompts = PromptGuard::new();
        let sink = prompts.sink(terminal.events_tx.clone(), Arc::clone(&terminal.on_wakeup));
        terminal.install(Prepared::new(backend, sink).unwrap(), prompts);
        script
    }

    fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if condition() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for: {what}");
    }

    fn screen_text(terminal: &Terminal) -> String {
        let mut snapshot = GridSnapshot::default();
        terminal.snapshot_into(&mut snapshot);
        let mut text = String::new();
        for cell in &snapshot.cells {
            match cell.text {
                crate::snapshot::CellText::Char(c) => text.push(c),
                crate::snapshot::CellText::Cluster { start, end } => {
                    text.push_str(&snapshot.clusters[start as usize..end as usize])
                }
            }
        }
        text
    }

    fn next_prompt(terminal: &Terminal) -> crate::prompt::Prompt {
        let mut found = None;
        wait_until("a prompt event", || {
            while let Some(event) = terminal.try_recv_event() {
                if let TermEvent::Prompt(prompt) = event {
                    found = Some(prompt);
                    return true;
                }
            }
            false
        });
        found.unwrap()
    }

    fn password_prompt() -> crate::prompt::PromptKind {
        crate::prompt::PromptKind::Secret {
            label: crate::prompt::SecretLabel::Password {
                user: "me".into(),
                host: "example.org".into(),
            },
            vault_available: true,
        }
    }

    #[test]
    fn reattach_keeps_the_grid_and_the_scrollback() {
        let (mut terminal, script) = start();
        let mut lines = String::from("first-line\r\n");
        for n in 0..60 {
            lines.push_str(&format!("scroll-{n}\r\n"));
        }
        script.feed(&lines);
        wait_until("old session output", || {
            screen_text(&terminal).contains("scroll-59")
        });

        let new = reattach(&mut terminal);
        new.feed("after-reattach\r\n");
        wait_until("new session output", || {
            screen_text(&terminal).contains("after-reattach")
        });

        assert!(screen_text(&terminal).contains("scroll-59"));
        terminal.scroll(TermScroll::Top);
        assert!(screen_text(&terminal).contains("first-line"));
    }

    #[test]
    fn bytes_from_the_old_session_after_reattach_never_reach_the_engine() {
        let (mut terminal, old) = start();
        let new = reattach(&mut terminal);

        old.feed("stale-bytes\r\n");
        new.feed("fresh-bytes\r\n");
        wait_until("new session output", || {
            screen_text(&terminal).contains("fresh-bytes")
        });
        // Give the old reader every chance to misbehave.
        thread::sleep(Duration::from_millis(150));

        assert!(!screen_text(&terminal).contains("stale-bytes"));
    }

    #[test]
    fn reattach_turns_off_the_modes_the_old_session_left_on() {
        let (mut terminal, old) = start();
        old.feed("\x1b[?1049h\x1b[?1000h\x1b[?1006h\x1b[?2004h");
        wait_until("modes on", || terminal.modes().alt_screen);
        let before = terminal.modes();
        assert!(before.bracketed_paste);
        assert_ne!(
            before.mouse_reporting,
            crate::snapshot::MouseReporting::None
        );

        let _new = reattach(&mut terminal);

        let after = terminal.modes();
        assert!(!after.alt_screen);
        assert!(!after.bracketed_paste);
        assert_eq!(after.mouse_reporting, crate::snapshot::MouseReporting::None);
    }

    #[test]
    fn a_device_status_reply_after_reattach_goes_to_the_new_backend() {
        let (mut terminal, old) = start();
        let new = reattach(&mut terminal);

        new.feed("\x1b[6n");
        wait_until("cursor position report", || !new.written().is_empty());

        let reply = String::from_utf8(new.written()).unwrap();
        assert!(
            reply.starts_with("\x1b[") && reply.ends_with('R'),
            "{reply:?}"
        );
        assert!(old.written().is_empty());
    }

    #[test]
    fn a_question_reaches_the_ui_and_the_answer_reaches_the_backend() {
        let (terminal, script) = start();
        let replies = script.sink().ask(password_prompt());

        let prompt = next_prompt(&terminal);
        assert_eq!(prompt.kind, password_prompt());
        terminal.answer(
            prompt.id,
            Answer::Secret {
                value: crate::prompt::Secret::new("hunter2".into()),
                remember: true,
            },
        );

        match replies.recv_timeout(Duration::from_secs(5)).unwrap() {
            Answer::Secret { value, remember } => {
                assert_eq!(value.expose(), "hunter2");
                assert!(remember);
            }
            other => panic!("unexpected answer: {other:?}"),
        }
        // The secret is never printable.
        assert_eq!(
            format!("{:?}", crate::prompt::Secret::new("hunter2".into())),
            "Secret(..)"
        );
    }

    #[test]
    fn dropping_the_terminal_cancels_an_open_question() {
        let (terminal, script) = start();
        let replies = script.sink().ask(password_prompt());
        let _ = next_prompt(&terminal);

        drop(terminal);

        assert!(matches!(
            replies.recv_timeout(Duration::from_secs(5)),
            Ok(Answer::Cancel)
        ));
        // A question asked after the terminal is gone is born cancelled.
        let late = script.sink().ask(password_prompt());
        assert!(matches!(
            late.recv_timeout(Duration::from_secs(5)),
            Ok(Answer::Cancel)
        ));
    }

    #[test]
    fn reattach_cancels_the_questions_of_the_session_it_replaces() {
        let (mut terminal, old) = start();
        let replies = old.sink().ask(password_prompt());
        let _ = next_prompt(&terminal);

        let _new = reattach(&mut terminal);

        assert!(matches!(
            replies.recv_timeout(Duration::from_secs(5)),
            Ok(Answer::Cancel)
        ));
    }

    #[test]
    fn watch_loop_emits_the_end_wakes_up_and_finishes_ended() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (events_tx, events_rx) = mpsc::channel();
        let (_shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let (killed_tx, killed_rx) = mpsc::channel();
        let (resize_tx, resize_rx) = mpsc::channel();
        let wakeups = Arc::new(Mutex::new(0usize));
        let on_wakeup: Arc<dyn Fn() + Send + Sync> = {
            let wakeups = Arc::clone(&wakeups);
            Arc::new(move || *wakeups.lock().unwrap() += 1)
        };

        // The newest of two queued resizes is the only one applied.
        resize_tx.send((10, 20)).unwrap();
        resize_tx.send((30, 40)).unwrap();

        watch_loop(
            FakeBackend::boxed(&log, Some(1)),
            events_tx,
            shutdown_rx,
            killed_tx,
            resize_rx,
            on_wakeup,
            first_generation(),
        );

        assert!(matches!(
            events_rx.try_recv(),
            Ok(TermEvent::Exit(EndReason::Exited {
                success: true,
                code: 0
            }))
        ));
        assert_eq!(*wakeups.lock().unwrap(), 1);
        assert_eq!(*log.lock().unwrap(), ["resize 30x40", "finish Ended"]);
        // Natural end is not a requested shutdown: nothing to confirm.
        assert!(killed_rx.try_recv().is_err());
    }

    #[test]
    fn watch_loop_kills_confirms_and_finishes_killed_on_shutdown() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (events_tx, events_rx) = mpsc::channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
        let (killed_tx, killed_rx) = mpsc::channel();
        let (_resize_tx, resize_rx) = mpsc::channel();

        drop(shutdown_tx);
        watch_loop(
            FakeBackend::boxed(&log, None),
            events_tx,
            shutdown_rx,
            killed_tx,
            resize_rx,
            Arc::new(|| {}),
            first_generation(),
        );

        assert_eq!(*log.lock().unwrap(), ["kill", "finish Killed"]);
        assert!(killed_rx.try_recv().is_ok());
        assert!(events_rx.try_recv().is_err());
    }
}
