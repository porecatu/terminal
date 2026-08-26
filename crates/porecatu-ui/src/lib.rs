// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use porecatu_render::{Color, Renderer};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

// docs/design/especificacao-visual.md secao 1.2 ("Janela"); mesmo valor de
// `[appearance.window] background` em docs/config/porecatu.example.toml.
const WINDOW_BACKGROUND: Color = Color {
    r: 0x15 as f64 / 255.0,
    g: 0x18 as f64 / 255.0,
    b: 0x1d as f64 / 255.0,
    a: 1.0,
};

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
}

impl ApplicationHandler for App {
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

        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
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
    let event_loop = EventLoop::new().expect("falha ao criar event loop");
    let mut app = App::default();
    event_loop
        .run_app(&mut app)
        .expect("event loop terminou com erro");
}
