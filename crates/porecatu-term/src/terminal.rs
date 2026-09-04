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
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::{fmt, io};

use porecatu_pty::{ProcessGroup, PtyError, PtyHandle, SpawnConfig};

use crate::engine::{TermEngine, TermSize};
use crate::event::TermEvent;
use crate::params::TermParams;
use crate::scroll::TermScroll;
use crate::search::{InvalidPattern, SearchJob, SearchMode, SearchStep};
use crate::selection::{SelectionKind, SelectionSide};
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
    /// Cópia deste lado (thread da UI) do mesmo grupo de processos que
    /// `watch_loop` guarda (ADR-0033) -- `None` fora do Windows ou se o Job
    /// não pôde ser criado/atribuído. Só para consulta (`has_extra_processes`,
    /// ADR-0034): quem decide se droppa ou esquece a cópia de lá, matando
    /// ou não a árvore, é sempre `watch_loop`, nunca este campo.
    process_group: Option<ProcessGroup>,
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

        let (pty, process_group) = porecatu_pty::spawn(pty_config)?;
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
        // Cópia que se move para dentro de `watch_loop` -- a que decide,
        // por caminho de saída, se dropa (mata a árvore) ou esquece
        // (preserva processo destacado). A outra cópia (`process_group`,
        // abaixo) fica só do lado da UI, para consulta (ADR-0034).
        let process_group_for_watch = process_group.clone();
        let watch_thread = {
            let on_wakeup = Arc::clone(&on_wakeup);
            thread::spawn(move || {
                watch_loop(
                    pty,
                    process_group_for_watch,
                    events_tx,
                    shutdown_rx,
                    killed_tx,
                    resize_rx,
                    on_wakeup,
                )
            })
        };

        Ok(Self {
            engine,
            events: events_rx,
            writer: write_tx,
            killed: killed_rx,
            process_group,
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

    /// `true` se há mais processo vivo além do shell raiz na árvore
    /// (ADR-0034) -- ex.: `node server.js` rodando em primeiro plano.
    /// `false` também quando o grupo não existe (sem `process_id()`, ou
    /// fora do Windows) -- lado seguro do erro é não confirmar por um
    /// sinal que não se conseguiu ler; o modo do terminal (`modes()`)
    /// continua cobrindo esse caso. A contagem em si é a união de duas
    /// fontes (Job + varredura por `sysinfo`) -- ver `ProcessGroup::
    /// process_count` em `porecatu-pty` para o porquê de nenhuma das duas
    /// bastar sozinha.
    pub fn has_extra_processes(&self) -> bool {
        self.process_group
            .as_ref()
            .is_some_and(|group| group.process_count() > 1)
    }

    /// RF-3.10/ADR-0038, segundo degrau da precedência de `cwd` na
    /// gravação de sessão (o primeiro, `Tab::cwd` por OSC 7, é decisão de
    /// `porecatu-ui`, que não conhece `ProcessGroup`). Consulta pontual ao
    /// PID raiz, só em Linux/macOS -- `ProcessGroup::cwd` não existe no
    /// Windows (ADR-0038 §3), então esta função devolve sempre `None` lá
    /// em vez de expor o `#[cfg]` para quem chama.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn cwd_fallback(&self) -> Option<PathBuf> {
        self.process_group.as_ref().and_then(|group| group.cwd())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub fn cwd_fallback(&self) -> Option<PathBuf> {
        None
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
    /// tem o `PtyHandle` -- ver `watch_loop`. Perder um resize em trânsito
    /// não é grave: o próximo `Resized` da janela manda o tamanho atual de
    /// novo.
    pub fn resize(&self, rows: usize, cols: usize) {
        lock(&self.engine).resize(rows, cols);
        let _ = self.resize.send((rows as u16, cols as u16));
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
            killed: self.killed,
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
    process_group: Option<ProcessGroup>,
    events: mpsc::Sender<TermEvent>,
    shutdown: mpsc::Receiver<()>,
    killed: mpsc::Sender<()>,
    resize: mpsc::Receiver<(u16, u16)>,
    on_wakeup: Arc<dyn Fn() + Send + Sync>,
) {
    loop {
        match shutdown.recv_timeout(WATCH_POLL_INTERVAL) {
            // `Ok(())` nunca é enviado de propósito hoje -- só o `Sender`
            // ser derrubado no `Drop` do `Terminal` importa -- mas tratar os
            // dois casos junto deixa a intenção clara: qualquer sinal de
            // desligamento mata o processo.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                let _ = pty.kill();
                // ADR-0033: fechamento **pedido pelo usuário** -- mata a
                // árvore inteira. `kill_tree` fecha o handle do Job (mata
                // na hora quem propagou a associação, mesmo sobrevivendo
                // ao próprio pai intermediário) e varre os descendentes
                // vivos do PID raiz por `sysinfo` (cobre shells como
                // PowerShell 7, que não propagam a associação ao Job --
                // ver a nota do módulo `job.rs` em `porecatu-pty`).
                if let Some(group) = process_group {
                    group.kill_tree();
                }
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
                    // Diferente do `read_loop`, nada mais vai chamar
                    // `on_wakeup` depois disto -- a thread de leitura já
                    // pode estar parada num `read()` que nunca retorna
                    // (`leak_pty`). Sem isto, `TermEvent::Exit` fica no
                    // canal sem ninguém ser avisado para ir buscá-lo.
                    on_wakeup();
                    leak_pty(pty);
                    // ADR-0033: saída **natural** do shell (ex. `exit`
                    // digitado) -- ao contrário do fechamento pedido pelo
                    // usuário, esta cópia é esquecida (`mem::forget`), não
                    // dropada. Isso barra pra sempre esta referência de
                    // decrementar o `Arc`: mesmo que a cópia do lado da UI
                    // droppe depois (quando a aba `Exited` for finalmente
                    // fechada de verdade), a contagem nunca alcança zero
                    // por causa desta, e o Job nunca fecha -- um processo
                    // que o shell tenha deliberadamente destacado (`start
                    // /b algo & exit`) sobrevive, como hoje. Fora do
                    // Windows `ProcessGroup` não carrega nada que precise
                    // de `Drop` ainda (dívida Unix do ADR-0033), então o
                    // clippy vê este `forget` como não-operação nessa
                    // plataforma -- `allow` deliberado, para o dia em que
                    // a dívida for paga sem precisar lembrar de tirá-lo.
                    #[allow(clippy::forget_non_drop)]
                    std::mem::forget(process_group);
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
