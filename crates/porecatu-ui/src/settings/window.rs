// SPDX-License-Identifier: GPL-3.0-or-later

//! `SettingsWindow`: a janela do SO da tela de configurações (ADR-0059 §1).
//! Guarda a própria `WindowSurface` -- o `GpuContext` e o atlas de glyphs são
//! os do processo, como para toda janela (ADR-0015) --, a geometria e o
//! estado de ponteiro; o desenho sai pela mesma pipeline de camadas das
//! janelas de terminal (ADR-0018), só que sem workspace do outro lado.
//!
//! Fora do macOS a janela não tem decoração nativa (ADR-0027): o cabeçalho
//! carrega o título, a drag region e os três botões de janela, com a mesma
//! geometria e o mesmo `resize_direction_at` da barra de abas. No macOS a
//! decoração é nativa e esta janela só desenha guia e painel.

use std::sync::Arc;
use std::time::Instant;

use porecatu_locale::Catalog;
use porecatu_render::{
    Frame, GpuContext, Layer, Primitive, Quad, Rect, TextRun, WindowSurface, icon,
};
use winit::dpi::PhysicalPosition;
use winit::window::{CursorIcon, Window, WindowId};

use super::layout::{self, Layout};
use super::{HEADER_GAP_PX, PANEL_BACKGROUND, SIDEBAR_WIDTH, TITLE_SIZE_PX};
use crate::messages::msg;
use crate::palette::ResolvedPalette;
use crate::tab_bar::{self, TabBarStyle, WindowButtonHit};
use crate::{DOUBLE_CLICK_THRESHOLD, bar_height, chrome, is_macos, overlay, titlebar};

/// O que um clique esquerdo pede a quem possui a janela. A janela não se
/// fecha sozinha: quem a guarda (`App.settings`) é quem a solta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Press {
    Nothing,
    /// Botão de fechar do cabeçalho.
    Close,
}

pub(crate) struct SettingsWindow {
    window: Arc<Window>,
    surface: WindowSurface,
    /// Pixels físicos por pixel lógico. Tudo aqui trabalha em lógico; só
    /// `WindowSurface` converte para físico (ADR-0018).
    scale: f32,
    logical_width: f32,
    logical_height: f32,
    /// Última posição do cursor, em pixels físicos -- `winit` só a entrega em
    /// `CursorMoved`, não em `MouseInput`.
    cursor_position: (f64, f64),
    /// Botão de janela sob o cursor, para o hover. Recalculado a cada
    /// `CursorMoved`; só decide se vale pedir um quadro.
    hovered_button: Option<WindowButtonHit>,
    /// Instante do último clique na drag region, para o duplo clique que
    /// maximiza/restaura -- o mesmo padrão de `WindowState::
    /// last_titlebar_click`.
    last_titlebar_click: Option<Instant>,
    /// O mesmo `Arc<Catalog>` do processo (`App::catalog`); a troca de idioma
    /// o substitui aqui junto com as janelas de terminal.
    catalog: Arc<Catalog>,
}

impl SettingsWindow {
    pub(crate) fn new(
        window: Arc<Window>,
        surface: WindowSurface,
        scale: f32,
        catalog: Arc<Catalog>,
    ) -> Self {
        let size = window.inner_size();
        Self {
            window,
            surface,
            scale,
            logical_width: size.width as f32 / scale,
            logical_height: size.height as f32 / scale,
            cursor_position: (0.0, 0.0),
            hovered_button: None,
            last_titlebar_click: None,
            catalog,
        }
    }

    pub(crate) fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// RF-16.2: a janela já aberta vem para a frente em vez de abrir outra.
    /// Uma janela minimizada é restaurada antes -- `focus_window` sozinho não
    /// a tira da barra de tarefas.
    pub(crate) fn bring_to_front(&self) {
        if self.window.is_minimized() == Some(true) {
            self.window.set_minimized(false);
        }
        self.window.focus_window();
    }

    /// Há alteração não gravada? É a pergunta que `App` faz antes de
    /// encerrar o processo (ADR-0059 §2) e que a tarefa de pendências
    /// responde de verdade. Hoje a tela não edita nada, então nunca.
    pub(crate) fn has_pending_changes(&self) -> bool {
        false
    }

    pub(crate) fn set_catalog(&mut self, catalog: &Arc<Catalog>) {
        self.catalog = Arc::clone(catalog);
    }

    pub(crate) fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Acompanha o tamanho físico novo da janela.
    pub(crate) fn resize(&mut self, gpu: &GpuContext, width: u32, height: u32) {
        self.surface.resize(gpu, width, height, self.scale);
        self.logical_width = width as f32 / self.scale;
        self.logical_height = height as f32 / self.scale;
        self.window.request_redraw();
    }

    /// A janela foi para um monitor de outra escala: o tamanho físico
    /// continua o que o SO deu, o lógico é que muda.
    pub(crate) fn rescale(&mut self, gpu: &GpuContext, scale: f32) {
        self.scale = scale;
        let size = self.window.inner_size();
        self.resize(gpu, size.width, size.height);
    }

    /// Altura do cabeçalho: a da barra de abas fora do macOS (ADR-0060 §1),
    /// zero onde a decoração é nativa.
    fn header_height(style: &TabBarStyle) -> f32 {
        if is_macos() { 0.0 } else { bar_height(style) }
    }

    fn layout(&self, style: &TabBarStyle) -> Layout {
        layout::layout(
            self.logical_width,
            self.logical_height,
            Self::header_height(style),
            SIDEBAR_WIDTH,
        )
    }

    fn cursor_logical(&self) -> (f32, f32) {
        (
            self.cursor_position.0 as f32 / self.scale,
            self.cursor_position.1 as f32 / self.scale,
        )
    }

    /// Borda de resize sob o ponto, se alguma: a mesma regra das janelas de
    /// terminal (ADR-0027) -- o botão de janela vence a borda no canto, e a
    /// janela maximizada não tem borda. Nunca no macOS (decoração nativa).
    fn resize_direction(
        &self,
        style: &TabBarStyle,
        resize_border: f32,
        point: (f32, f32),
    ) -> Option<winit::window::ResizeDirection> {
        if is_macos() || self.window.is_maximized() {
            return None;
        }
        let over_button = tab_bar::point_in_window_button(
            style,
            false,
            self.logical_width,
            bar_height(style),
            point,
        )
        .is_some();
        if over_button {
            return None;
        }
        titlebar::resize_direction_at(
            point,
            self.logical_width,
            self.logical_height,
            false,
            resize_border,
        )
    }

    /// Movimento do cursor: guarda a posição, escolhe a forma do cursor (setas
    /// de resize nas bordas) e pede quadro só se o hover de um botão mudou.
    pub(crate) fn cursor_moved(
        &mut self,
        position: PhysicalPosition<f64>,
        style: &TabBarStyle,
        resize_border: f32,
    ) {
        self.cursor_position = (position.x, position.y);
        let point = self.cursor_logical();
        let hovered = if is_macos() {
            None
        } else {
            tab_bar::point_in_window_button(
                style,
                false,
                self.logical_width,
                bar_height(style),
                point,
            )
        };
        let cursor = match self.resize_direction(style, resize_border, point) {
            Some(direction) => CursorIcon::from(direction),
            None => CursorIcon::Default,
        };
        self.window.set_cursor(cursor);
        if hovered != self.hovered_button {
            self.hovered_button = hovered;
            self.window.request_redraw();
        }
    }

    /// O cursor saiu da janela: some o hover do botão.
    pub(crate) fn cursor_left(&mut self) {
        if self.hovered_button.take().is_some() {
            self.window.request_redraw();
        }
    }

    /// Clique esquerdo (ADR-0027): botão de janela, borda de resize ou drag
    /// region do cabeçalho, nessa ordem. Tudo o que não é isso ainda não tem
    /// alvo nesta etapa.
    pub(crate) fn left_pressed(&mut self, style: &TabBarStyle, resize_border: f32) -> Press {
        if is_macos() {
            return Press::Nothing;
        }
        let point = self.cursor_logical();
        if let Some(hit) = tab_bar::point_in_window_button(
            style,
            false,
            self.logical_width,
            bar_height(style),
            point,
        ) {
            return match hit {
                WindowButtonHit::Minimize => {
                    self.window.set_minimized(true);
                    Press::Nothing
                }
                WindowButtonHit::MaximizeRestore => {
                    self.window.set_maximized(!self.window.is_maximized());
                    Press::Nothing
                }
                WindowButtonHit::Close => Press::Close,
            };
        }
        if let Some(direction) = self.resize_direction(style, resize_border, point) {
            let _ = self.window.drag_resize_window(direction);
            return Press::Nothing;
        }
        if point.1 < Self::header_height(style) {
            self.resolve_titlebar_drag();
        }
        Press::Nothing
    }

    /// Clique na drag region: duplo clique maximiza/restaura, e o resto
    /// arrasta a janela. Resolvido no *press*, antes de `drag_window` -- ela
    /// entrega o gesto ao loop modal do SO, sem garantia de ver o `Released`
    /// de volta (mesma razão de `WindowState::resolve_titlebar_drag`).
    fn resolve_titlebar_drag(&mut self) {
        let now = Instant::now();
        let is_double_click = self
            .last_titlebar_click
            .is_some_and(|at| now.duration_since(at) <= DOUBLE_CLICK_THRESHOLD);
        self.last_titlebar_click = if is_double_click { None } else { Some(now) };
        if is_double_click {
            self.window.set_maximized(!self.window.is_maximized());
        } else {
            let _ = self.window.drag_window();
        }
    }

    /// Pinta o quadro: painel ao fundo, guia, e -- fora do macOS -- o
    /// cabeçalho por cima. Tudo na camada `Chrome` (ADR-0060 §4); camada nova
    /// nenhuma.
    pub(crate) fn paint(&self, style: &TabBarStyle, pal: &ResolvedPalette) -> Frame {
        let layout = self.layout(style);
        let mut out = vec![
            quad(layout.panel, PANEL_BACKGROUND),
            quad(layout.sidebar, pal.bar_background),
            quad(layout.sidebar_separator, pal.editor_divider),
        ];
        if let Some(header) = layout.header {
            out.push(quad(header, pal.bar_background));
            self.paint_header_content(header, style, pal, &mut out);
            chrome::paint_window_buttons(
                style,
                pal,
                self.logical_width,
                header.height,
                self.window.is_maximized(),
                self.hovered_button,
                &mut out,
            );
        }
        let mut frame = Frame::new();
        frame.set_layer(Layer::Chrome, out);
        frame
    }

    /// Ícone e título à esquerda do cabeçalho (ADR-0060 §1): no
    /// `trilha_padding` mais o `padding_left` da aba, o ícone `SETTINGS` na
    /// em de ícone do chrome × `0.8` -- o multiplicador do botão de
    /// configurações da barra --, e o título 15px/500 depois de um `gap: 8`.
    fn paint_header_content(
        &self,
        header: Rect,
        style: &TabBarStyle,
        pal: &ResolvedPalette,
        out: &mut Vec<Primitive>,
    ) {
        let icon_size = style.icon_em_size * 0.8;
        let icon_x = style.trilha_padding + style.padding_left;
        let icon_width = icon::SETTINGS.ink_width(icon_size);
        out.push(chrome::centered_glyph(
            icon::SETTINGS,
            Rect {
                x: icon_x,
                y: header.y,
                width: icon_width,
                height: header.height,
            },
            icon_size,
            pal.chrome_icon,
        ));
        out.push(Primitive::Text(TextRun {
            origin: (
                icon_x + icon_width + HEADER_GAP_PX,
                header.y + (header.height - TITLE_SIZE_PX) / 2.0,
            ),
            text: msg::settings::window_title(&self.catalog),
            font: overlay::TITLE_FONT,
            size_px: TITLE_SIZE_PX,
            color: pal.dialog_title_text,
        }));
    }

    /// Submete o quadro. O fundo de limpeza é o do painel, que é o que
    /// ocupa a maior parte da janela.
    pub(crate) fn render(&mut self, gpu: &mut GpuContext, frame: &Frame) {
        self.surface.render(gpu, PANEL_BACKGROUND, frame);
    }
}

fn quad(rect: Rect, color: porecatu_render::Color) -> Primitive {
    Primitive::Quad(Quad { rect, color })
}
