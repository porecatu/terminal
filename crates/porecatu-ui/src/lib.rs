// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use porecatu_core::TabId;
use porecatu_render::{Frame, GpuContext, Layer, WindowSurface};
use porecatu_term::{
    GridSnapshot, Modifiers, PtySize, SpawnConfig, TermEvent, TermParams, Terminal,
};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

mod clipboard;
mod input;
mod paint;
mod palette;

use input::ClickTracker;
use paint::CellMetrics;

// docs/config/porecatu.example.toml [terminal.font]: size = 12.5 (RF-5.3),
// line_height = 1.75 (RF-5.6, "multiplicador das métricas naturais da
// fonte"). Simplificação desta etapa: aplicado direto sobre `size` em vez
// da métrica natural da fonte (ascent+descent+lineGap), que exigiria ler
// hhea/OS2 da face -- ajustar quando isso importar na prática.
const FONT_SIZE_PX: f32 = 12.5;
const LINE_HEIGHT_MULTIPLIER: f32 = 1.75;

/// Grade mínima -- uma célula em cada direção, no pior caso de janela
/// minúscula ou métrica de fonte falhando.
const MIN_GRID: usize = 1;

/// Evento de usuário do event loop: uma aba ficou suja (ADR-0007) e precisa
/// de redraw. Carrega `(WindowId, TabId)` desde a F1 mesmo com uma única
/// janela e uma única aba -- `TabId` sozinho não seria suficiente com mais
/// de uma janela, e corrigir isso depois mexeria no caminho quente
/// (ADR-0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wakeup {
    window: WindowId,
    tab: TabId,
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<GpuContext>,
    window_surface: Option<WindowSurface>,
    /// Pixels físicos por pixel lógico (`window.scale_factor()`). A grade
    /// e o `Frame` trabalham em lógico; só `WindowSurface` converte para
    /// físico, no único ponto que o ADR-0018 permite.
    scale: f32,
    proxy: EventLoopProxy<Wakeup>,
    tab: TabId,
    terminal: Option<Terminal>,
    snapshot: GridSnapshot,
    cell_metrics: CellMetrics,
    /// Estado corrente dos modificadores, mantido via `ModifiersChanged`
    /// -- o `KeyEvent` do `winit` não carrega isso junto.
    modifiers: Modifiers,
    /// Última posição conhecida do cursor, em pixels físicos -- `winit` só
    /// manda a posição em `CursorMoved`, não em `MouseInput`.
    cursor_position: (f64, f64),
    /// Botão pressionado no momento, se algum -- decide se `CursorMoved`
    /// é arraste (estende seleção / reporta modo 1002) ou movimento livre.
    mouse_button_down: Option<MouseButton>,
    click_tracker: ClickTracker,
}

impl App {
    fn new(proxy: EventLoopProxy<Wakeup>) -> Self {
        Self {
            window: None,
            gpu: None,
            window_surface: None,
            scale: 1.0,
            proxy,
            tab: TabId::new(0),
            terminal: None,
            snapshot: GridSnapshot::default(),
            // Substituído antes de qualquer render real, assim que o
            // `GpuContext` existe e a métrica de fonte pode ser medida.
            cell_metrics: CellMetrics {
                width: 1.0,
                height: 1.0,
            },
            modifiers: Modifiers::NONE,
            cursor_position: (0.0, 0.0),
            mouse_button_down: None,
            click_tracker: ClickTracker::default(),
        }
    }

    /// Spawna o único terminal da F1 na janela recém-criada, com a grade
    /// derivada da métrica de fonte medida (Etapa 4: "a grade é derivada
    /// da métrica de fonte, não o contrário"). Erro de spawn (ex.: shell
    /// inexistente) não derruba o app -- fica sem terminal, numa janela
    /// vazia. `porecatu-ui` ainda não tem superfície de aviso (ADR-0014,
    /// F2); `stderr` é o único canal disponível nesta fase.
    fn spawn_terminal(&mut self, window: &Arc<Window>, rows: usize, cols: usize) {
        let window_id = window.id();
        let tab = self.tab;
        let proxy = self.proxy.clone();

        let pty_config = SpawnConfig {
            program: None,
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            size: PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            },
        };

        match Terminal::spawn(pty_config, TermParams::default(), move || {
            // Chamado pela thread de leitura de `porecatu-term` a cada
            // lote de bytes aplicado ao grid -- só marca a aba como suja
            // (ADR-0007); quem decide se isso vira frame é `user_event`.
            let _ = proxy.send_event(Wakeup {
                window: window_id,
                tab,
            });
        }) {
            Ok(terminal) => self.terminal = Some(terminal),
            Err(err) => eprintln!("porecatu: falha ao iniciar terminal: {err}"),
        }
    }

    /// Recalcula linhas/colunas a partir do tamanho lógico (px físico /
    /// escala) e da métrica de célula já medida (também lógica), e
    /// propaga pra `WindowSurface` e pro terminal (motor + PTY) -- Etapa
    /// 5: "recalcular grade e propagar para o PTY". `WindowSurface` é quem
    /// converte de volta para físico (ADR-0018).
    fn resize_to(&mut self, width: u32, height: u32) {
        if let (Some(gpu), Some(window_surface)) = (&self.gpu, &mut self.window_surface) {
            window_surface.resize(gpu, width, height, self.scale);
        }
        let logical_width = width as f32 / self.scale;
        let logical_height = height as f32 / self.scale;
        let cols = ((logical_width / self.cell_metrics.width) as usize).max(MIN_GRID);
        let rows = ((logical_height / self.cell_metrics.height) as usize).max(MIN_GRID);
        if let Some(terminal) = &self.terminal {
            terminal.resize(rows, cols);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn cell_at_cursor(&self) -> input::CellPosition {
        input::cell_at(
            self.cursor_position.0,
            self.cursor_position.1,
            self.cell_metrics,
            self.snapshot.rows.max(MIN_GRID),
            self.snapshot.cols.max(MIN_GRID),
        )
    }
}

impl ApplicationHandler<Wakeup> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attributes = Window::default_attributes().with_title("Porecatu");
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("falha ao criar janela"),
        );
        // Acentuação por tecla morta e composição de IME (CJK) precisam
        // disso ligado -- sem `set_ime_allowed`, o SO não tenta compor
        // nada e a tecla morta chega crua (ADR-0008).
        window.set_ime_allowed(true);

        // `inner_size()` do `winit` é físico; `scale_factor()` converte
        // pra lógico, que é o que a grade e o `Frame` usam daqui em diante
        // -- só `WindowSurface` volta a físico (ADR-0018).
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let (mut gpu, mut window_surface) =
            GpuContext::new(Arc::clone(&window), size.width, size.height);
        window_surface.resize(&gpu, size.width, size.height, scale);

        let (cell_width, cell_height) = gpu
            .text_measurer()
            .measure_mono_cell(FONT_SIZE_PX, FONT_SIZE_PX * LINE_HEIGHT_MULTIPLIER);
        self.cell_metrics = CellMetrics {
            width: cell_width,
            height: cell_height,
        };
        self.scale = scale;

        let logical_width = size.width as f32 / scale;
        let logical_height = size.height as f32 / scale;
        let cols = ((logical_width / cell_width) as usize).max(MIN_GRID);
        let rows = ((logical_height / cell_height) as usize).max(MIN_GRID);

        self.spawn_terminal(&window, rows, cols);
        self.window = Some(window);
        self.gpu = Some(gpu);
        self.window_surface = Some(window_surface);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Wakeup) {
        let Some(window) = &self.window else {
            return;
        };
        // Aba suja que não é a visível: para aí, sem redraw (ADR-0007
        // ponto 2). Com uma janela e uma aba isso é sempre verdadeiro hoje,
        // mas o formato do evento já é o de multi-janela (ADR-0015).
        if event.window != window.id() || event.tab != self.tab {
            return;
        }

        if let Some(terminal) = &mut self.terminal {
            while let Some(term_event) = terminal.try_recv_event() {
                match term_event {
                    TermEvent::Title(Some(title)) => window.set_title(&title),
                    TermEvent::Title(None) => window.set_title("Porecatu"),
                    // RF-1.21 (indicador de campainha) é F2, quando existe
                    // barra de abas para carregar o indicador.
                    TermEvent::Bell => {}
                    // OSC 52 escrita: vai pro clipboard do sistema, sujeito
                    // ao teto de tamanho já aplicado em porecatu-term
                    // (RF-10.10). Leitura: só dispara quando
                    // `TermParams::osc52_read` permite -- `false` por
                    // default (RF-10.11), então na prática este braço só
                    // roda se isso um dia virar configurável (F4).
                    TermEvent::ClipboardWrite(text) => clipboard::copy(&text),
                    TermEvent::ClipboardRead(responder) => {
                        let content = clipboard::paste().unwrap_or_default();
                        terminal.write(responder.respond(&content).into_bytes());
                    }
                    // Depende de tema resolvido -- F4.
                    TermEvent::ColorQuery(_) => {}
                    TermEvent::Exit { .. } => {
                        if let Some(terminal) = self.terminal.take() {
                            terminal.shutdown();
                        }
                        event_loop.exit();
                        return;
                    }
                }
            }
        }

        // `request_redraw` coalesce chamadas repetidas antes do próximo
        // `RedrawRequested` num só evento -- é o que faz N wakeups de
        // saída rápida (ex.: `cargo build`) não virarem N frames
        // (ADR-0007 ponto 3).
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(terminal) = self.terminal.take() {
                    // Bloqueia até o processo morrer de verdade -- sem
                    // isso nada garante que a thread de observação do
                    // terminal rode seu próximo ciclo antes do processo do
                    // Porecatu já ter terminado, e o filho ficaria órfão.
                    terminal.shutdown();
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                self.resize_to(size.width, size.height);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale = scale_factor as f32;
                // O novo tamanho físico só é conhecido após o resize aplicado
                // pelo SO; reler `inner_size()` cobre o caso de plataformas que
                // não emitem `Resized` em seguida.
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    self.resize_to(size.width, size.height);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = input::modifiers_from(modifiers.state());
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                if let Some(terminal) = &self.terminal {
                    // Modos lidos agora, não do snapshot do último frame
                    // (que pode estar obsoleto -- ex.: o programa acabou
                    // de ligar DECCKM e ainda não houve redraw).
                    input::handle_keyboard_input(terminal, &terminal.modes(), &key, self.modifiers);
                }
            }
            // Composição de IME (CJK) e tecla morta já resolvida pelo SO
            // (ABNT2) -- passam direto pro terminal, sem consultar
            // keybind nenhum (ADR-0008). `Preedit`/`Enabled`/`Disabled`
            // não geram bytes: a composição em andamento não é texto
            // final ainda. Desenhar o preedit sobre o cursor fica para
            // quando houver render de chrome sobre o texto (F2+).
            WindowEvent::Ime(Ime::Commit(text)) => {
                if let Some(terminal) = &self.terminal {
                    terminal.write(text.into_bytes());
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(terminal) = &self.terminal {
                    let cell = self.cell_at_cursor();
                    input::handle_mouse_wheel(
                        terminal,
                        &terminal.modes(),
                        delta,
                        self.modifiers,
                        cell,
                    );
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = (position.x, position.y);
                if let Some(terminal) = &self.terminal {
                    let cell = self.cell_at_cursor();
                    input::handle_mouse_motion(
                        terminal,
                        &terminal.modes(),
                        cell,
                        self.modifiers,
                        self.mouse_button_down,
                    );
                    // Seleção mudou de forma sem passar pela thread de
                    // leitura -- ninguém mais vai pedir redraw sozinho.
                    if self.mouse_button_down.is_some()
                        && let Some(window) = &self.window
                    {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.mouse_button_down = match state {
                    ElementState::Pressed => Some(button),
                    ElementState::Released => None,
                };
                if let Some(terminal) = &self.terminal {
                    let cell = self.cell_at_cursor();
                    input::handle_mouse_button(
                        terminal,
                        &terminal.modes(),
                        button,
                        state == ElementState::Pressed,
                        cell,
                        self.modifiers,
                        &mut self.click_tracker,
                    );
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(terminal) = &self.terminal {
                    terminal.snapshot_into(&mut self.snapshot);
                }
                if let (Some(gpu), Some(window_surface)) = (&mut self.gpu, &mut self.window_surface)
                {
                    let primitives =
                        paint::build_primitives(&self.snapshot, self.cell_metrics, FONT_SIZE_PX);
                    let mut frame = Frame::new();
                    frame.set_layer(Layer::Grid, primitives);
                    window_surface.render(gpu, palette::TERM_BACKGROUND, &frame);
                }
            }
            _ => {}
        }
    }
}

/// Abre a janela principal do Porecatu e roda o event loop até ela fechar.
pub fn run() {
    let event_loop = EventLoop::<Wakeup>::with_user_event()
        .build()
        .expect("falha ao criar event loop");
    let proxy = event_loop.create_proxy();
    let mut app = App::new(proxy);
    event_loop
        .run_app(&mut app)
        .expect("event loop terminou com erro");
}
