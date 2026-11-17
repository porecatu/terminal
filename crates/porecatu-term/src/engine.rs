// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::selection::Selection as AlacSelection;
use alacritty_terminal::term::cell::{
    Cell as AlacCell, Flags as AlacFlags, Hyperlink as AlacHyperlink,
};
use alacritty_terminal::term::{
    ClipboardType, Config as AlacConfig, Osc52, TermMode, point_to_viewport, viewport_to_point,
};
use alacritty_terminal::vte::ansi::{
    CursorShape as AnsiCursorShape, CursorStyle as AnsiCursorStyle, Processor, Rgb as AnsiRgb,
};

use crate::dismiss::DismissWatcher;
use crate::event::{ClipboardResponder, ColorQueryResponder, TermEvent};
use crate::osc7::Osc7Watcher;
use crate::params::TermParams;
use crate::scroll::TermScroll;
use crate::search::{InvalidPattern, SearchJob, SearchMode, SearchStep};
use crate::selection::{SelectionKind, SelectionSide};
use crate::snapshot::{
    Cell, CellFlags, CellText, Cursor, CursorShape, GridSnapshot, HyperlinkSpan, MouseReporting,
    ScrollbackInfo, SelectionSpan, TermModes,
};

/// Folga do teto **interno** do histórico da grade primária, acima de
/// `scrollback.lines`.
///
/// O `alacritty_terminal` descarta as linhas mais antigas em silêncio quando o
/// histórico enche e não diz quantas. Para contar `dropped` com exatidão o
/// motor roda com o teto em `scrollback.lines + HISTORY_SLACK` e, depois de
/// cada lote, corta o excesso por conta própria (`trim_history`), somando-o a
/// `dropped`. O usuário nunca vê o excesso: o corte acontece sob o mesmo lock
/// do `advance`, antes de qualquer leitura.
///
/// O valor cobre um lote de 4096 bytes (o buffer de `read_loop`) em que cada
/// byte rola uma linha (`\n`, `ESC D`, quebra automática): nenhum outro byte
/// empurra mais de uma linha ao histórico, salvo `CSI n S`, que rola até
/// `rows` linhas em 3 bytes ou mais. Um lote com mais de `HISTORY_SLACK`
/// linhas empurradas ao histórico transborda o teto interno e o motor perde
/// as excedentes sem contá-las -- só subconta, nunca sobreconta. A folga custa
/// memória só enquanto o lote roda: as linhas existem de fato apenas depois de
/// escritas e o corte as solta no fim do lote.
const HISTORY_SLACK: usize = 4096;

/// Reconhecedor, por byte, dos dois jeitos de limpar o histórico: `ESC [ 3 J`
/// (CSI 3 J) e `ESC c` (RIS). Estado preservado entre lotes, porque a sequência
/// pode chegar partida em duas leituras do PTY. Só serve para **partir o lote**
/// logo depois da sequência (ver `TermEngine::feed`); quem decide se o histórico
/// foi limpo é a comparação com `history_seen`. Falso positivo (um `ESC c`
/// dentro de outra sequência) só parte o lote sem efeito.
#[derive(Debug, Clone, Copy, Default)]
enum ClearScan {
    #[default]
    Ground,
    Esc,
    Csi,
    Csi3,
}

impl ClearScan {
    /// Consome um byte. `true` quando ele fecha uma sequência de limpeza.
    fn step(&mut self, byte: u8) -> bool {
        let (next, hit) = match (*self, byte) {
            (Self::Esc, b'c') | (Self::Csi3, b'J') => (Self::Ground, true),
            (_, 0x1b) => (Self::Esc, false),
            (Self::Esc, b'[') => (Self::Csi, false),
            (Self::Csi, b'3') => (Self::Csi3, false),
            _ => (Self::Ground, false),
        };
        *self = next;
        hit
    }
}

/// Dimensões do terminal em células. Tipo próprio -- `Dimensions` do
/// `alacritty_terminal` é implementado aqui, não exposto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    pub rows: usize,
    pub cols: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

struct EventProxy {
    sender: mpsc::Sender<TermEvent>,
    /// Canal de escrita do PTY (o mesmo que `Terminal::write` usa) -- é
    /// para lá, não para `sender`, que `Event::PtyWrite` vai. Diferente dos
    /// outros eventos da seção 4.3, uma resposta automática do motor
    /// (DSR/DA/CPR) não é decisão de `ui`: o motor já formatou os bytes
    /// certos, só falta escrever. Rotear por `sender` obrigaria todo
    /// consumidor de eventos a filtrar e repassar isso a cada wakeup --
    /// esquecer um `PtyWrite` pendente é o programa ficar parado esperando
    /// resposta que nunca chega (era exatamente o problema do teste de
    /// integração da Etapa 1, antes deste motor existir).
    ///
    /// Compartilhado com `TermEngine`, que o troca em `set_pty_writer`
    /// quando o backend é trocado (`Terminal::reattach`).
    pty_writer: Arc<Mutex<mpsc::Sender<Vec<u8>>>>,
    clipboard_write_max_bytes: usize,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            let writer = self
                .pty_writer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _ = writer.send(text.into_bytes());
            return;
        }

        let mapped = match event {
            Event::PtyWrite(_) => unreachable!("tratado acima"),
            Event::Title(title) => Some(TermEvent::Title(Some(title))),
            Event::ResetTitle => Some(TermEvent::Title(None)),
            Event::Bell => Some(TermEvent::Bell),
            Event::ClipboardStore(ClipboardType::Clipboard, text) => {
                if text.len() > self.clipboard_write_max_bytes {
                    None
                } else {
                    Some(TermEvent::ClipboardWrite(text))
                }
            }
            // RF-10.9: a seleção PRIMARY não é suportada no v1.
            Event::ClipboardStore(ClipboardType::Selection, _) => None,
            Event::ClipboardLoad(_clipboard_type, formatter) => {
                Some(TermEvent::ClipboardRead(ClipboardResponder(formatter)))
            }
            Event::ColorRequest(index, formatter) => {
                Some(TermEvent::ColorQuery(ColorQueryResponder {
                    index,
                    format: Arc::new(move |r, g, b| formatter(AnsiRgb { r, g, b })),
                }))
            }
            // Sem consumidor no v1 (dica de cursor de mouse, blink de
            // cursor, wakeup de coalescência própria do motor -- usamos a
            // nossa, ver ADR-0007/0015 -- e tamanho de área de texto em
            // pixels). `Exit`/`ChildExit` também ficam de fora: o motor não
            // sabe de PTY, quem detecta o fim do processo é `Terminal`
            // (Etapa 3), via `porecatu-pty::PtyHandle::try_wait` -- é de lá
            // que `TermEvent::Exit` sai, não daqui.
            Event::MouseCursorDirty
            | Event::CursorBlinkingChange
            | Event::Wakeup
            | Event::TextAreaSizeRequest(_)
            | Event::Exit
            | Event::ChildExit(_) => None,
        };

        if let Some(event) = mapped {
            let _ = self.sender.send(event);
        }
    }
}

fn osc52_mode(read: bool, write: bool) -> Osc52 {
    match (write, read) {
        (false, false) => Osc52::Disabled,
        (true, false) => Osc52::OnlyCopy,
        (false, true) => Osc52::OnlyPaste,
        (true, true) => Osc52::CopyPaste,
    }
}

/// Motor VT encapsulado (ADR-0002). Uso de `alacritty_terminal` isolado
/// aqui dentro -- nenhum tipo dele atravessa a API pública deste crate.
pub struct TermEngine {
    term: Term<EventProxy>,
    parser: Processor,
    /// Segundo parser, independente do de `term` -- ver `crate::osc7`.
    osc7: Osc7Watcher,
    /// Observador do marcador de dispensa (ADR-0039 §4) -- ver
    /// `crate::dismiss`. Roda sobre o mesmo lote, sem depender do motor.
    dismiss: DismissWatcher,
    /// Cópia do canal de eventos: `TermEvent::Cwd` não vem do motor (OSC 7
    /// não é despachado a nenhum método de `Handler`, ver `crate::osc7`),
    /// então `advance` manda direto por aqui, fora de `EventProxy`.
    events: mpsc::Sender<TermEvent>,
    /// Mesmo canal que `EventProxy::pty_writer` usa, para trocá-lo.
    pty_writer: Arc<Mutex<mpsc::Sender<Vec<u8>>>>,
    /// Config atual do motor, guardada para `set_default_cursor_style`:
    /// `Term::set_options` troca a config inteira, não um campo.
    config: AlacConfig,
    /// `scrollback.lines`: o teto **visível** do histórico. O teto interno da
    /// grade é este valor mais [`HISTORY_SLACK`].
    scrollback_lines: usize,
    /// Linhas que saíram do topo do histórico da grade primária, desde que o
    /// painel existe (zera só quando o histórico é limpo). Ver
    /// [`ScrollbackInfo`].
    dropped: u64,
    /// Tamanho do histórico da primária no último ponto de contagem na tela
    /// normal. Um histórico menor que isto, sem o motor ter cortado e sem
    /// resize nem volta da tela alternativa no meio, é histórico limpo.
    history_seen: usize,
    /// A contagem anterior ocorreu na tela alternativa (o histórico da
    /// primária ficou fora de alcance e pode ter mudado por resize), então a
    /// primeira contagem na normal só ressincroniza.
    was_alt: bool,
    clear_scan: ClearScan,
}

/// O que `TermEngine::soft_reset` manda ao parser: desliga o que uma sessão
/// pode ter deixado ligado, sem apagar a grade nem o scrollback. Equivale ao
/// efeito visível de um `DECSTR` (que o motor não implementa) nos modos que
/// importam para quem entra depois.
const SOFT_RESET: &str = concat!(
    "\x1b[?1049l",                       // tela alternativa
    "\x1b[?1000l\x1b[?1002l\x1b[?1003l", // reporte de mouse
    "\x1b[?1005l\x1b[?1006l\x1b[?1015l", // encodings de mouse
    "\x1b[?2004l",                       // bracketed paste
    "\x1b[?1004l",                       // eventos de foco
    "\x1b[?1l\x1b>",                     // setas e teclado numérico de aplicação (DECCKM, DECKPNM)
    "\x1b[?25h",                         // cursor visível
    "\x1b[0 q",                          // forma de cursor do usuário (DECSCUSR)
    "\x1b[0m",                           // atributos
);

impl TermEngine {
    /// Cria o motor. `events` é o canal para onde o motor manda os eventos
    /// traduzidos (docs/arquitetura.md seção 4.3) -- chamado de dentro de
    /// `advance`, no mesmo lock que a thread de leitura já segura
    /// (ADR-0007). Recebido de fora (em vez de criado aqui) porque
    /// `Terminal` (Etapa 3) precisa do mesmo `Sender` para injetar
    /// `TermEvent::Exit`, que não vem do motor -- vem do fim do processo.
    ///
    /// `pty_writer` é o canal para onde vão as respostas automáticas do
    /// motor (DSR/DA/CPR) -- ver o comentário em `EventProxy::pty_writer`.
    pub fn new(
        params: TermParams,
        size: TermSize,
        events: mpsc::Sender<TermEvent>,
        pty_writer: mpsc::Sender<Vec<u8>>,
    ) -> Self {
        let config = AlacConfig {
            // Teto interno: ver `HISTORY_SLACK`.
            scrolling_history: params.scrollback_lines.saturating_add(HISTORY_SLACK),
            semantic_escape_chars: params.word_separators,
            default_cursor_style: AnsiCursorStyle {
                shape: ansi_cursor_shape(params.default_cursor_shape),
                blinking: params.cursor_blinking,
            },
            osc52: osc52_mode(params.osc52_read, params.osc52_write),
            ..Default::default()
        };
        let pty_writer = Arc::new(Mutex::new(pty_writer));
        let proxy = EventProxy {
            sender: events.clone(),
            pty_writer: Arc::clone(&pty_writer),
            clipboard_write_max_bytes: params.clipboard_write_max_bytes,
        };

        let term = Term::new(config.clone(), &size, proxy);
        Self {
            term,
            parser: Processor::new(),
            osc7: Osc7Watcher::new(),
            dismiss: DismissWatcher::new(),
            events,
            pty_writer,
            config,
            scrollback_lines: params.scrollback_lines,
            dropped: 0,
            history_seen: 0,
            was_alt: false,
            clear_scan: ClearScan::default(),
        }
    }

    /// Linhas e colunas da grade de agora.
    pub fn size(&self) -> (usize, usize) {
        (self.term.screen_lines(), self.term.columns())
    }

    /// Aponta as respostas automáticas (DSR/DA/CPR) para outro canal de
    /// escrita -- o do backend novo, em `Terminal::reattach`. Resposta que
    /// já estava no canal antigo morre com ele.
    pub fn set_pty_writer(&mut self, writer: mpsc::Sender<Vec<u8>>) {
        *self
            .pty_writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = writer;
    }

    /// Desliga os modos que a sessão anterior deixou ligados (tela
    /// alternativa, mouse, bracketed paste...) pelo caminho do próprio motor,
    /// mantendo grade e scrollback. Usado por `Terminal::reattach`: sem isto,
    /// um `vim` aberto na hora de uma queda prenderia a aba nova na tela
    /// alternativa (ADR-0071 §4).
    pub fn soft_reset(&mut self) {
        self.feed(SOFT_RESET.as_bytes());
    }

    /// Troca o cursor de **default** do motor (hot reload de
    /// `[terminal.cursor]`, classe A). DECSCUSR emitido pelo programa
    /// continua valendo: `Term::set_options` não toca o estilo que o
    /// programa definiu (RF-5.25).
    pub fn set_default_cursor_style(&mut self, shape: CursorShape, blinking: bool) {
        self.config.default_cursor_style = AnsiCursorStyle {
            shape: ansi_cursor_shape(shape),
            blinking,
        };
        self.term.set_options(self.config.clone());
    }

    /// Alimenta o parser VT com bytes crus do PTY, e em paralelo os dois
    /// scanners independentes que observam o mesmo lote: OSC 7
    /// (`crate::osc7`) e o marcador de dispensa do convite de integração
    /// de shell (`crate::dismiss`).
    pub fn advance(&mut self, bytes: &[u8]) {
        self.feed(bytes);
        if let Some(cwd) = self.osc7.advance(bytes) {
            let _ = self.events.send(TermEvent::Cwd {
                path: cwd.path,
                host: cwd.host,
            });
        }
        if self.dismiss.advance(bytes) {
            let _ = self.events.send(TermEvent::ShellIntegrationDismiss);
        }
    }

    /// Injeta uma nota estilizada no grid, como se fosse saída do programa
    /// -- passa pelo mesmo parser, nunca escreve na grade por fora dele
    /// (ADR-0017 item 5). `rgb` é resolvido por quem chama: este motor não
    /// conhece paleta nem tema.
    ///
    /// Cada quebra de linha do `text` vira `\r\n`: um `\n` sozinho só desce o
    /// cursor, e uma nota de várias linhas (a de chave alterada, a do convite
    /// de shell) viraria uma escada para a direita.
    pub fn inject_note(&mut self, text: &str, rgb: (u8, u8, u8)) {
        let (r, g, b) = rgb;
        let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        let styled = format!("\r\n\x1b[38;2;{r};{g};{b}m{text}\x1b[0m\r\n");
        self.feed(styled.as_bytes());
    }

    /// Único ponto em que bytes entram no parser: alimenta o motor e mantém a
    /// contagem de `dropped`.
    ///
    /// O lote é partido logo depois de cada sequência que limpa o histórico
    /// (`ClearScan`), e a contagem roda ao fim de cada pedaço. Sem a partida,
    /// um `CSI 3 J` seguido de saída que reenche o histórico no mesmo lote
    /// passaria despercebido (o tamanho não teria diminuído). O caso comum, sem
    /// limpeza, é um pedaço só e custa uma leitura de `history_size`.
    fn feed(&mut self, bytes: &[u8]) {
        let mut start = 0;
        for (index, &byte) in bytes.iter().enumerate() {
            if self.clear_scan.step(byte) {
                self.parser.advance(&mut self.term, &bytes[start..=index]);
                self.account_history(true);
                start = index + 1;
            }
        }
        if start < bytes.len() {
            self.parser.advance(&mut self.term, &bytes[start..]);
            self.account_history(false);
        }
    }

    /// Contagem de `dropped` depois de um pedaço de bytes (ver
    /// [`TermEngine::feed`]). `ends_with_clear`: o pedaço termina numa
    /// sequência que limpa o histórico.
    ///
    /// - Tela alternativa: não conta. A alternativa não tem histórico e o da
    ///   primária fica fora de alcance (`history_size()` lê a grade ativa); só
    ///   anota que esteve lá.
    /// - Histórico **menor** que o da última contagem: foi limpo (CSI 3 J,
    ///   RIS), `dropped` volta a zero. Exceção: logo depois de voltar da tela
    ///   alternativa o histórico pode ter mudado por resize feito lá dentro, e
    ///   só uma limpeza reconhecida pelo `ClearScan` conta como limpeza.
    /// - Histórico **maior** que `scrollback.lines`: o excesso saiu do topo,
    ///   soma a `dropped` e é cortado (`trim_history`).
    fn account_history(&mut self, ends_with_clear: bool) {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.was_alt = true;
            return;
        }
        let was_alt = std::mem::replace(&mut self.was_alt, false);
        if self.term.history_size() < self.history_seen && (ends_with_clear || !was_alt) {
            self.dropped = 0;
        }
        self.trim_history();
    }

    /// Corta o histórico da grade ativa (a primária, na tela normal) em
    /// `scrollback.lines`, contando o excesso em `dropped`, e anota o tamanho
    /// resultante. `update_history` descarta as linhas **mais antigas**
    /// (`Storage::shrink_lines` encurta pelo topo) e mantém o
    /// `display_offset` dentro do novo teto; a segunda chamada devolve o teto
    /// interno à folga. Nenhuma das duas move a vista do usuário, que nunca
    /// passa de `scrollback.lines`.
    fn trim_history(&mut self) {
        let lines = self.scrollback_lines;
        let history = self.term.history_size();
        if history > lines {
            self.dropped += (history - lines) as u64;
            let grid = self.term.grid_mut();
            grid.update_history(lines);
            grid.update_history(lines.saturating_add(HISTORY_SLACK));
        }
        self.history_seen = self.term.history_size();
    }

    /// Troca o `scrollback.lines` (recarga da configuração). Mantém a folga do
    /// teto interno e conta como descartado o que o encolhimento cortar.
    pub fn set_scrollback_lines(&mut self, lines: usize) {
        self.scrollback_lines = lines;
        let internal = lines.saturating_add(HISTORY_SLACK);
        self.config.scrolling_history = internal;
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            // O histórico da primária não é legível daqui; `history_seen` é o
            // último tamanho conhecido e ninguém o altera na alternativa.
            // `set_options` corta a primária no teto interno, e o resto do
            // excesso é cortado e contado na volta (`was_alt`).
            let cut = self.history_seen.saturating_sub(internal);
            self.dropped += cut as u64;
            self.history_seen -= cut;
            self.term.set_options(self.config.clone());
        } else {
            // Corta antes, para o excesso passar por `dropped`; só então o
            // `set_options` fixa o teto interno novo.
            self.trim_history();
            self.term.set_options(self.config.clone());
            self.history_seen = self.term.history_size();
        }
    }

    /// Ressincroniza a contagem depois de um resize. Reflow e crescimento da
    /// tela puxam linhas do histórico (ou o enchem) sem ser limpeza, então o
    /// tamanho menor não pode ser lido como histórico limpo; os índices
    /// absolutos das linhas que ficam não mudam (só as mais novas se movem).
    fn resync_history(&mut self) {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.was_alt = true;
            return;
        }
        self.was_alt = false;
        self.trim_history();
    }

    /// Redimensiona a grade. Devolve `false` sem tocar nada quando o
    /// tamanho pedido já é o atual -- quem chama usa isso para não
    /// repassar ao PTY um resize que não muda nada (o ConPTY repinta a
    /// tela inteira a cada resize, mesmo de tamanho igual).
    pub fn resize(&mut self, rows: usize, cols: usize) -> bool {
        if self.term.screen_lines() == rows && self.term.columns() == cols {
            return false;
        }
        self.term.resize(TermSize { rows, cols });
        self.resync_history();
        true
    }

    /// Rola o scrollback (PRD-010 RF-10.12 a RF-10.14). Sem efeito na tela
    /// alternativa -- ela não tem histórico; quem chama decide se traduz
    /// isso em outra coisa (ADR-0013: roda vira setas com
    /// `alternate_scroll`), este método só reflete o que o motor já faz
    /// (nada) nesse caso.
    pub fn scroll(&mut self, scroll: TermScroll) {
        self.term.scroll_display(scroll.into());
    }

    /// Modos atuais, sem montar o resto do snapshot -- mais barato que
    /// `snapshot_into` para quem só precisa decidir como rotear um evento
    /// de input (ADR-0008/0013), onde o snapshot do último frame já
    /// poderia estar obsoleto.
    pub fn modes(&self) -> TermModes {
        convert_modes(*self.term.mode())
    }

    /// Inicia uma seleção no ponto (linha/coluna de viewport, 0-based).
    /// Usa o `Selection` do motor -- os quatro modos, `Alt`+arraste
    /// incluso, já vêm de lá (ADR-0002/0013: não reimplementar).
    pub fn start_selection(
        &mut self,
        kind: SelectionKind,
        row: usize,
        col: usize,
        side: SelectionSide,
    ) {
        let point = self.viewport_point(row, col);
        self.term.selection = Some(AlacSelection::new(kind.into(), point, side.into()));
    }

    /// Estende a seleção em andamento até o ponto dado. Sem efeito se não
    /// há seleção ativa.
    pub fn update_selection(&mut self, row: usize, col: usize, side: SelectionSide) {
        let point = self.viewport_point(row, col);
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, side.into());
        }
    }

    /// Limpa a seleção. Rolagem pura (`scroll`) nunca chama isto --
    /// `scroll_display` não toca `selection` (ADR-0013: "rolagem pura
    /// preserva"). O motor já limpa sozinho quando o programa escreve em
    /// cima da região selecionada ou muda de tela alternativa.
    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    /// Texto selecionado, já com o espaço à direita cortado e linhas
    /// `WRAPLINE` remontadas sem quebra -- `Term::selection_to_string` do
    /// motor já faz isso (RF-10.6), não precisa reimplementar aqui.
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string()
    }

    /// Seleciona a tela visível inteira e o scrollback (RF-11.16, menu de
    /// contexto do terminal) -- mesmo `Selection` do motor que
    /// `start_selection`/`update_selection` usam, só com os dois pontos já
    /// nos extremos da grade em vez de vir do mouse.
    pub fn select_all(&mut self) {
        let start = Point::new(self.term.topmost_line(), Column(0));
        let end = Point::new(self.term.bottommost_line(), self.term.last_column());
        let mut selection = AlacSelection::new(
            SelectionKind::Simple.into(),
            start,
            SelectionSide::Left.into(),
        );
        selection.update(end, SelectionSide::Right.into());
        self.term.selection = Some(selection);
    }

    /// Prepara uma busca no scrollback (ADR-0041, RF-11.1 a RF-11.9):
    /// literal ou regex (`mode`), sobre a tela visível mais o scrollback
    /// inteiro, reduzida à tela visível na tela alternativa. Nada é varrido
    /// ainda -- ver [`TermEngine::step_search`]. Padrão de regex inválido é
    /// erro devolvido, nunca `panic`. Nenhum tipo do `alacritty_terminal`
    /// sai daqui -- ver `crate::search`; `porecatu-ui` corta o resultado
    /// pela vista e resolve a cor.
    pub fn start_search(
        &self,
        pattern: &str,
        mode: SearchMode,
        lines_per_step: usize,
    ) -> Result<SearchJob, InvalidPattern> {
        SearchJob::new(&self.term, pattern, mode, lines_per_step)
    }

    /// Varre um lote de `job` (ADR-0041 §"Riscos e mitigação": busca
    /// incremental por lotes -- item 5 da Etapa 1 mediu que uma busca de
    /// uma vez só estoura o orçamento de frame num scrollback cheio).
    /// Chamar de novo a cada frame enquanto devolver `InProgress`.
    pub fn step_search(&self, job: &mut SearchJob) -> SearchStep {
        job.step(&self.term)
    }

    fn viewport_point(&self, row: usize, col: usize) -> Point {
        let display_offset = self.term.grid().display_offset();
        viewport_to_point(display_offset, Point::new(row, Column(col)))
    }

    /// Preenche `out` com o estado atual, reusando os buffers já alocados
    /// (`cells`/`clusters`) em vez de realocar (ADR-0007).
    pub fn snapshot_into(&self, out: &mut GridSnapshot) {
        let content = self.term.renderable_content();
        let cols = self.term.columns();
        let rows = self.term.screen_lines();
        let display_offset = content.display_offset;

        fill_cells(
            out,
            rows,
            cols,
            content.display_iter.map(|indexed| indexed.cell),
        );

        out.cursor = self.snapshot_cursor(
            content.cursor.point,
            content.cursor.shape,
            display_offset,
            rows,
        );
        out.scroll_offset = display_offset;
        out.selection = content
            .selection
            .map(|range| convert_selection(range, display_offset, rows, cols));
        out.modes = convert_modes(content.mode);
    }

    /// A tela viva: as `rows` linhas do fundo da grade ativa
    /// (`Line(0)..Line(rows)`), **sem** aplicar o `display_offset` -- o que o
    /// desktop rolou não muda o que o espelhamento remoto vê (ADR-0105 §3).
    /// Mesmo formato de [`TermEngine::snapshot_into`] (a conversão de célula,
    /// de grafema e de hyperlink é a mesma função), com o cursor na posição
    /// real, `scroll_offset = 0`, sem seleção e sem ocorrências de busca. Não
    /// mexe no `display_offset`. Na tela alternativa lê a grade alternativa e
    /// `history` é 0.
    pub fn snapshot_live_into(&self, out: &mut GridSnapshot) -> ScrollbackInfo {
        let content = self.term.renderable_content();
        let grid = self.term.grid();
        let cols = self.term.columns();
        let rows = self.term.screen_lines();

        fill_cells(
            out,
            rows,
            cols,
            (0..rows).flat_map(move |row| {
                (0..cols).map(move |col| &grid[Point::new(Line(row as i32), Column(col))])
            }),
        );

        out.cursor = self.snapshot_cursor(content.cursor.point, content.cursor.shape, 0, rows);
        out.scroll_offset = 0;
        out.selection = None;
        out.modes = convert_modes(content.mode);
        self.scrollback_info()
    }

    /// Uma página de histórico: até `count` linhas, **de cima para baixo**, em
    /// que a última é a de `offset` (0 = a linha logo acima do topo da tela),
    /// limitadas ao que existe -- `out.rows` é quantas vieram e `out.cols` as
    /// colunas da grade. Cursor oculto, sem seleção nem ocorrências, `modes`
    /// de agora. Na tela alternativa devolve 0 linhas e `history = 0`.
    ///
    /// Não limita `count`: quem chama (o protocolo remoto) o limita a 500.
    pub fn history_into(
        &self,
        offset: usize,
        count: usize,
        out: &mut GridSnapshot,
    ) -> ScrollbackInfo {
        let grid = self.term.grid();
        let cols = self.term.columns();
        let history = self.term.history_size();
        let returned = count.min(history.saturating_sub(offset));
        // A linha de histórico de deslocamento `k` é `Line(-1 - k)`; a mais
        // alta da página tem deslocamento `offset + returned - 1`.
        let top = -1 - (offset + returned).saturating_sub(1) as i32;

        fill_cells(
            out,
            returned,
            cols,
            (0..returned).flat_map(move |row| {
                (0..cols).map(move |col| &grid[Point::new(Line(top + row as i32), Column(col))])
            }),
        );

        out.cursor = Cursor::default();
        out.scroll_offset = 0;
        out.selection = None;
        out.modes = convert_modes(*self.term.mode());
        self.scrollback_info()
    }

    fn scrollback_info(&self) -> ScrollbackInfo {
        ScrollbackInfo {
            history: self.term.history_size(),
            dropped: self.dropped,
        }
    }

    fn snapshot_cursor(
        &self,
        point: Point,
        shape: AnsiCursorShape,
        display_offset: usize,
        rows: usize,
    ) -> Cursor {
        let shape = convert_cursor_shape(shape);
        Cursor {
            // `point_to_viewport` só recusa linha acima da vista: com o
            // scrollback rolado, o cursor da última linha vira `line >=
            // rows` e era desenhado abaixo da grade, por cima da barra de
            // status. Cursor fora da vista não tem posição.
            position: point_to_viewport(display_offset, point)
                .filter(|p| p.line < rows)
                .map(|p| (p.line, p.column.0)),
            shape,
            visible: shape != CursorShape::Hidden,
            blinking: self.term.cursor_style().blinking,
        }
    }
}

/// Esvazia os buffers reusados de `out` e os preenche com `rows * cols`
/// células de `cells` (row-major): conversão de célula, arena de grafemas e
/// spans de hyperlink. Comum a `snapshot_into`, `snapshot_live_into` e
/// `history_into` -- o que muda entre elas é só de onde vêm as células.
fn fill_cells<'a>(
    out: &mut GridSnapshot,
    rows: usize,
    cols: usize,
    cells: impl Iterator<Item = &'a AlacCell>,
) {
    out.cols = cols;
    out.rows = rows;
    out.clusters.clear();
    out.cells.clear();
    out.cells.resize(rows * cols, Cell::default());
    // `porecatu-ui` repopula a cada frame com busca ativa (ADR-0041 §4)
    // -- sem realocar aqui evita que um frame sem busca herde as
    // ocorrências do anterior.
    out.occurrences.clear();
    out.hyperlinks.clear();
    out.hyperlink_spans.clear();

    // Span de hyperlink em aberto (ADR-0042 §1): nunca atravessa
    // quebra de linha -- por isso é fechado à força a cada `col == 0`,
    // mesmo que o link continue na célula seguinte (é justamente o
    // caso que o `id` existe para amarrar de volta).
    let mut open_link: Option<(usize, usize, AlacHyperlink)> = None;

    for (index, cell) in cells.enumerate() {
        let row = index / cols;
        let col = index % cols;

        if col == 0
            && let Some((link_row, link_start, link)) = open_link.take()
        {
            push_hyperlink_span(out, link_row, link_start, cols.saturating_sub(1), &link);
        }

        let link = cell.hyperlink();
        let continues = matches!((&open_link, &link), (Some((_, _, cur)), Some(new)) if cur == new);
        if !continues {
            if let Some((link_row, link_start, cur)) = open_link.take() {
                push_hyperlink_span(out, link_row, link_start, col.saturating_sub(1), &cur);
            }
            if let Some(new) = link {
                open_link = Some((row, col, new));
            }
        }

        if let Some(slot) = out.cells.get_mut(index) {
            *slot = convert_cell(cell, &mut out.clusters);
        }
    }
    if let Some((link_row, link_start, link)) = open_link.take() {
        push_hyperlink_span(out, link_row, link_start, cols.saturating_sub(1), &link);
    }
}

/// Fecha um span de hyperlink em `row`, de `start_col` a `end_col`
/// (inclusive), gravando id e uri na arena reusada (ADR-0042 §1).
fn push_hyperlink_span(
    out: &mut GridSnapshot,
    row: usize,
    start_col: usize,
    end_col: usize,
    link: &AlacHyperlink,
) {
    let id_start = out.hyperlinks.len() as u32;
    out.hyperlinks.push_str(link.id());
    let id_end = out.hyperlinks.len() as u32;
    let uri_start = out.hyperlinks.len() as u32;
    out.hyperlinks.push_str(link.uri());
    let uri_end = out.hyperlinks.len() as u32;
    out.hyperlink_spans.push(HyperlinkSpan {
        row,
        start_col,
        end_col,
        id_start,
        id_end,
        uri_start,
        uri_end,
    });
}

fn convert_cell(cell: &alacritty_terminal::term::cell::Cell, clusters: &mut String) -> Cell {
    let text = match cell.zerowidth() {
        Some(zerowidth) if !zerowidth.is_empty() => {
            let start = clusters.len() as u32;
            clusters.push(cell.c);
            for &extra in zerowidth {
                clusters.push(extra);
            }
            let end = clusters.len() as u32;
            CellText::Cluster { start, end }
        }
        _ => CellText::Char(cell.c),
    };

    Cell {
        text,
        fg: crate::color::TermColor::from(cell.fg),
        bg: crate::color::TermColor::from(cell.bg),
        flags: convert_flags(cell.flags),
    }
}

fn convert_flags(flags: AlacFlags) -> CellFlags {
    let mut out = CellFlags::empty();
    if flags.contains(AlacFlags::BOLD) {
        out |= CellFlags::BOLD;
    }
    if flags.contains(AlacFlags::ITALIC) {
        out |= CellFlags::ITALIC;
    }
    if flags.intersects(AlacFlags::ALL_UNDERLINES) {
        out |= CellFlags::UNDERLINE;
    }
    if flags.contains(AlacFlags::INVERSE) {
        out |= CellFlags::INVERSE;
    }
    if flags.contains(AlacFlags::WIDE_CHAR) {
        out |= CellFlags::WIDE;
    }
    if flags.contains(AlacFlags::WIDE_CHAR_SPACER) {
        out |= CellFlags::WIDE_SPACER;
    }
    if flags.contains(AlacFlags::WRAPLINE) {
        out |= CellFlags::WRAPLINE;
    }
    if flags.intersects(AlacFlags::DIM) {
        out |= CellFlags::DIM;
    }
    if flags.contains(AlacFlags::STRIKEOUT) {
        out |= CellFlags::STRIKEOUT;
    }
    out
}

fn convert_cursor_shape(shape: AnsiCursorShape) -> CursorShape {
    match shape {
        AnsiCursorShape::Block => CursorShape::Block,
        AnsiCursorShape::Underline => CursorShape::Underline,
        AnsiCursorShape::Beam => CursorShape::Beam,
        AnsiCursorShape::HollowBlock => CursorShape::HollowBlock,
        AnsiCursorShape::Hidden => CursorShape::Hidden,
    }
}

/// Sentido inverso de [`convert_cursor_shape`] -- `TermParams::
/// default_cursor_shape` (RF-5.22) vira o `default_cursor_style.shape` que
/// `Term::cursor_style()` devolve quando nenhum DECSCUSR está em vigor
/// (`self.cursor_style` é `None`, ver `alacritty_terminal::Term`).
fn ansi_cursor_shape(shape: CursorShape) -> AnsiCursorShape {
    match shape {
        CursorShape::Block => AnsiCursorShape::Block,
        CursorShape::Underline => AnsiCursorShape::Underline,
        CursorShape::Beam => AnsiCursorShape::Beam,
        CursorShape::HollowBlock => AnsiCursorShape::HollowBlock,
        CursorShape::Hidden => AnsiCursorShape::Hidden,
    }
}

fn resolve_viewport_point(
    point: Point,
    display_offset: usize,
    rows: usize,
    cols: usize,
) -> (usize, usize) {
    match point_to_viewport(display_offset, point) {
        Some(p) => (
            p.line.min(rows.saturating_sub(1)),
            p.column.0.min(cols.saturating_sub(1)),
        ),
        None => (0, 0),
    }
}

fn convert_selection(
    range: alacritty_terminal::selection::SelectionRange,
    display_offset: usize,
    rows: usize,
    cols: usize,
) -> SelectionSpan {
    let (start_row, start_col) = resolve_viewport_point(range.start, display_offset, rows, cols);
    let (end_row, end_col) = resolve_viewport_point(range.end, display_offset, rows, cols);
    SelectionSpan {
        start_row,
        start_col,
        end_row,
        end_col,
        is_block: range.is_block,
    }
}

fn convert_modes(mode: alacritty_terminal::term::TermMode) -> TermModes {
    use alacritty_terminal::term::TermMode as M;

    let mouse_reporting = if mode.contains(M::MOUSE_MOTION) {
        MouseReporting::AnyMotion
    } else if mode.contains(M::MOUSE_DRAG) {
        MouseReporting::ClickAndDrag
    } else if mode.contains(M::MOUSE_REPORT_CLICK) {
        MouseReporting::Click
    } else {
        MouseReporting::None
    };

    TermModes {
        alt_screen: mode.contains(M::ALT_SCREEN),
        bracketed_paste: mode.contains(M::BRACKETED_PASTE),
        mouse_reporting,
        sgr_mouse: mode.contains(M::SGR_MOUSE),
        app_cursor_keys: mode.contains(M::APP_CURSOR),
        app_keypad: mode.contains(M::APP_KEYPAD),
    }
}
