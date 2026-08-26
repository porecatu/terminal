// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use porecatu_core::TabId;
use porecatu_render::{Color, Renderer};
use porecatu_term::{PtySize, SpawnConfig, TermEvent, TermParams, Terminal};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

// docs/design/especificacao-visual.md secao 1.2 ("Janela"); mesmo valor de
// `[appearance.window] background` em docs/config/porecatu.example.toml.
const WINDOW_BACKGROUND: Color = Color {
    r: 0x15 as f64 / 255.0,
    g: 0x18 as f64 / 255.0,
    b: 0x1d as f64 / 255.0,
    a: 1.0,
};

// Grade provisória. Sem métricas de fonte (Etapa 4 da F1) não há como
// calcular linhas/colunas a partir do tamanho em pixels da janela --
// 80x24 é o tamanho clássico de terminal. Resize recalculando a grade de
// verdade é Etapa 5; até lá, redimensionar a janela não muda a grade.
const PROVISIONAL_ROWS: usize = 24;
const PROVISIONAL_COLS: usize = 80;

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
    renderer: Option<Renderer>,
    proxy: EventLoopProxy<Wakeup>,
    tab: TabId,
    terminal: Option<Terminal>,
    snapshot: porecatu_term::GridSnapshot,
}

impl App {
    fn new(proxy: EventLoopProxy<Wakeup>) -> Self {
        Self {
            window: None,
            renderer: None,
            proxy,
            tab: TabId::new(0),
            terminal: None,
            snapshot: porecatu_term::GridSnapshot::default(),
        }
    }

    /// Spawna o único terminal da F1 na janela recém-criada. Erro de spawn
    /// (ex.: shell inexistente) não derruba o app -- fica sem terminal,
    /// numa janela vazia. `porecatu-ui` ainda não tem superfície de aviso
    /// (ADR-0014, F2); `stderr` é o único canal disponível nesta fase.
    fn spawn_terminal(&mut self, window: &Arc<Window>) {
        let window_id = window.id();
        let tab = self.tab;
        let proxy = self.proxy.clone();

        let pty_config = SpawnConfig {
            program: None,
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            size: PtySize {
                rows: PROVISIONAL_ROWS as u16,
                cols: PROVISIONAL_COLS as u16,
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
        let size = window.inner_size();
        let renderer = Renderer::new(Arc::clone(&window), size.width, size.height);

        self.spawn_terminal(&window);
        self.window = Some(window);
        self.renderer = Some(renderer);
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
                    // `arboard` é Etapa 6.
                    TermEvent::ClipboardWrite(_) | TermEvent::ClipboardRead(_) => {}
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
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                // O novo tamanho físico só é conhecido após o resize aplicado
                // pelo SO; reler `inner_size()` cobre o caso de plataformas que
                // não emitem `Resized` em seguida.
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    if let Some(renderer) = &mut self.renderer {
                        renderer.resize(size.width, size.height);
                    }
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(terminal) = &self.terminal {
                    terminal.snapshot_into(&mut self.snapshot);
                }
                // Tradução do snapshot para primitivas de desenho (quads,
                // texto) é Etapa 4 -- por ora o frame só limpa a tela.
                if let Some(renderer) = &mut self.renderer {
                    renderer.render(WINDOW_BACKGROUND);
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
