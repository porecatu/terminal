// SPDX-License-Identifier: GPL-3.0-or-later

//! `SettingsWindow`: a janela do SO da tela de configurações (ADR-0059 §1).
//! Guarda a própria `WindowSurface` -- o `GpuContext` e o atlas de glyphs são
//! os do processo, como para toda janela (ADR-0015) --, a geometria, o estado
//! de ponteiro e de teclado, e o conteúdo medido do grupo em vista; o desenho
//! sai pela mesma pipeline de camadas das janelas de terminal (ADR-0018), só
//! que sem workspace do outro lado.
//!
//! Fora do macOS a janela não tem decoração nativa (ADR-0027): o cabeçalho
//! carrega o título, a drag region e os três botões de janela, com a mesma
//! geometria e o mesmo `resize_direction_at` da barra de abas. No macOS a
//! decoração é nativa e esta janela só desenha guia e painel.
//!
//! **Medir texto é caro.** O conteúdo do painel (texto cortado, larguras de
//! botão e de segmento) é medido por `content::build` uma vez e guardado; só
//! volta a ser medido quando a chave muda -- grupo, largura do painel, idioma
//! ou config. Nenhum caminho de pintura ou de hit-test mede texto.

use std::sync::Arc;
use std::time::Instant;

use porecatu_config::Config;
use porecatu_locale::Catalog;
use porecatu_render::{
    Frame, GpuContext, Layer, Primitive, Quad, Rect, TextMeasurer, TextRun, WindowSurface, icon,
};
use winit::dpi::PhysicalPosition;
use winit::event::{KeyEvent, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use super::content::{self, Block, Content, ContentKey, RowView};
use super::layout::{
    self, BlockGeometry, Focus, FooterButton, Hit, Layout, Metrics, focus_order, footer_buttons,
    group_items, hit_test, next_focus,
};
use super::paint;
use super::{Group, HEADER_GAP_PX, PANEL_BACKGROUND, TITLE_SIZE_PX};
use crate::messages::msg;
use crate::palette::ResolvedPalette;
use crate::tab_bar::{self, TabBarStyle, WindowButtonHit};
use crate::tooltip::{Hover, HoverKey};
use crate::{DOUBLE_CLICK_THRESHOLD, access, bar_height, chrome, is_macos, overlay, titlebar};

/// O que a janela de configurações lê do processo, por chamada: o estilo da
/// barra (que ela reaproveita para o cabeçalho) e o `Config` em vigor.
#[derive(Clone, Copy)]
pub(crate) struct Env<'a> {
    pub style: &'a TabBarStyle,
    pub config: &'a Config,
}

/// O que um clique ou uma tecla pede a quem possui a janela. A janela não se
/// fecha nem abre arquivo sozinha: quem a guarda (`App`) é quem sabe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Press {
    Nothing,
    /// Botão de fechar do cabeçalho, ou `Esc`.
    Close,
    /// "Abrir arquivo no editor" (RF-16.14).
    OpenFile,
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
    /// Botão de janela sob o cursor, para o hover.
    hovered_button: Option<WindowButtonHit>,
    /// Instante do último clique na drag region, para o duplo clique que
    /// maximiza/restaura -- o mesmo padrão de `WindowState::
    /// last_titlebar_click`.
    last_titlebar_click: Option<Instant>,
    /// O mesmo `Arc<Catalog>` do processo (`App::catalog`); a troca de idioma
    /// o substitui aqui junto com as janelas de terminal.
    catalog: Arc<Catalog>,
    /// Adaptador de acessibilidade (ADR-0043 §1, ADR-0059 §5): um por janela,
    /// criado antes de ela ficar visível. Sem leitor de tela conectado, a
    /// árvore nem é montada (`update_if_active`).
    access_adapter: accesskit_winit::Adapter,
    /// O grupo escolhido na guia (RF-16.7). Começa no último escolhido na
    /// execução, que `App` guarda (RF-16.9).
    selected_group: Group,
    focus: Focus,
    /// Rolagem vertical do painel, em pixels lógicos de conteúdo.
    scroll: f32,
    /// `Shift` pressionado, para `Shift+Tab`.
    shift: bool,
    /// O alvo sob o cursor: realce de item e de botão.
    hovered: Option<Hit>,
    /// Tooltip da descrição cortada (ADR-0019, ADR-0060 §2).
    hover: Hover,
    /// O conteúdo medido do grupo em vista; refeito quando a chave muda.
    content: Option<Content>,
    /// Sobe a cada troca de catálogo e a cada recarga de config, e invalida o
    /// conteúdo guardado.
    generation: u64,
}

impl SettingsWindow {
    pub(crate) fn new(
        window: Arc<Window>,
        surface: WindowSurface,
        scale: f32,
        catalog: Arc<Catalog>,
        access_adapter: accesskit_winit::Adapter,
        initial_group: Group,
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
            access_adapter,
            selected_group: initial_group,
            focus: Focus::Sidebar,
            scroll: 0.0,
            shift: false,
            hovered: None,
            hover: Hover::default(),
            content: None,
            generation: 0,
        }
    }

    pub(crate) fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// O grupo escolhido, que `App` lembra até o fim da execução (RF-16.9).
    pub(crate) fn selected_group(&self) -> Group {
        self.selected_group
    }

    /// ADR-0043 §1: o adaptador precisa ver todo `WindowEvent` da janela,
    /// antes de ele ser tratado.
    pub(crate) fn process_access_event(&mut self, event: &WindowEvent) {
        self.access_adapter.process_event(&self.window, event);
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

    /// Troca de idioma ao vivo (ADR-0056 §9): o catálogo novo vale no próximo
    /// quadro, e o título da janela do SO -- que não é desenhado, é do
    /// sistema -- acompanha agora. O conteúdo medido é refeito, porque o texto
    /// mudou de largura, e a árvore de acessibilidade na próxima volta do
    /// event loop.
    pub(crate) fn set_catalog(&mut self, catalog: &Arc<Catalog>) {
        self.catalog = Arc::clone(catalog);
        self.window
            .set_title(&msg::settings::window_title(&self.catalog));
        self.invalidate();
    }

    /// A config mudou (recarga): os valores mostrados e a lista de temas
    /// podem ter mudado.
    pub(crate) fn invalidate(&mut self) {
        self.generation += 1;
        self.window.request_redraw();
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

    // ---- geometria

    /// Altura do cabeçalho: a da barra de abas fora do macOS (ADR-0060 §1),
    /// zero onde a decoração é nativa.
    fn header_height(style: &TabBarStyle) -> f32 {
        if is_macos() { 0.0 } else { bar_height(style) }
    }

    fn metrics(&self, env: Env<'_>) -> Metrics {
        Metrics::from_config(env.config, Self::header_height(env.style))
    }

    fn layout(&self, env: Env<'_>) -> Layout {
        let m = self.metrics(env);
        layout::layout(
            self.logical_width,
            self.logical_height,
            m.header_height,
            m.sidebar_width,
            m.footer_height(),
        )
    }

    fn cursor_logical(&self) -> (f32, f32) {
        (
            self.cursor_position.0 as f32 / self.scale,
            self.cursor_position.1 as f32 / self.scale,
        )
    }

    /// Garante o conteúdo do grupo em vista e a rolagem dentro do possível.
    /// É o único ponto que mede texto.
    fn ensure_content(&mut self, env: Env<'_>, measurer: &mut TextMeasurer) {
        let m = self.metrics(env);
        let layout = self.layout(env);
        let key = ContentKey {
            group: self.selected_group,
            panel_width_bits: layout.panel.width.to_bits(),
            generation: self.generation,
        };
        if self.content.as_ref().map(|c| c.key) != Some(key) {
            self.content = Some(content::build(key, env.config, &self.catalog, &m, measurer));
        }
        if let Some(content) = &self.content {
            self.scroll = layout::clamp_scroll(
                self.scroll,
                content.geometry.content_height,
                layout.panel_body.height,
            );
        }
    }

    fn content(&self) -> &Content {
        self.content
            .as_ref()
            .expect("ensure_content roda antes de qualquer leitura")
    }

    /// Quais botões do rodapé estão disponíveis, na ordem de
    /// `FOOTER_BUTTONS`: só Abrir arquivo, até haver pendência (RF-16.14).
    fn available_footer(&self) -> [bool; 3] {
        let pending = self.has_pending_changes();
        [true, pending, pending]
    }

    fn available_buttons(&self) -> Vec<FooterButton> {
        layout::FOOTER_BUTTONS
            .iter()
            .zip(self.available_footer())
            .filter(|(_, available)| *available)
            .map(|(button, _)| *button)
            .collect()
    }

    fn hit_at(&self, env: Env<'_>, point: (f32, f32)) -> Option<Hit> {
        let m = self.metrics(env);
        let layout = self.layout(env);
        let content = self.content();
        let items = group_items(layout.sidebar, m.sidebar_padding, m.sidebar_item_height);
        let footer = footer_buttons(&m, layout.footer, content.footer_widths);
        hit_test(
            &layout,
            &items,
            &footer,
            &content.geometry,
            self.scroll,
            point,
        )
    }

    // ---- ponteiro

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
    /// de resize nas bordas, mão sobre o que se clica), atualiza o realce e o
    /// tooltip, e pede quadro só se algo visível mudou.
    pub(crate) fn cursor_moved(
        &mut self,
        position: PhysicalPosition<f64>,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        now: Instant,
    ) {
        self.cursor_position = (position.x, position.y);
        self.ensure_content(env, measurer);
        let point = self.cursor_logical();
        let resize_border = env.config.appearance.window_controls.resize_border as f32;

        let hovered_button = if is_macos() {
            None
        } else {
            tab_bar::point_in_window_button(
                env.style,
                false,
                self.logical_width,
                bar_height(env.style),
                point,
            )
        };
        let hit = self.hit_at(env, point);
        // Descartar e Salvar indisponíveis não realçam.
        let hit = match hit {
            Some(Hit::Footer(button)) if !self.available_buttons().contains(&button) => None,
            other => other,
        };
        let cursor = match self.resize_direction(env.style, resize_border, point) {
            Some(direction) => CursorIcon::from(direction),
            None if matches!(hit, Some(Hit::Group(_) | Hit::Footer(_))) => CursorIcon::Pointer,
            None => CursorIcon::Default,
        };
        self.window.set_cursor(cursor);

        let before = (
            self.hovered,
            self.hovered_button,
            self.hover.visible().is_some(),
        );
        self.hovered = hit;
        self.hovered_button = hovered_button;
        let target = self.tooltip_target(env, point, hit);
        self.hover.update(target, now);
        let after = (
            self.hovered,
            self.hovered_button,
            self.hover.visible().is_some(),
        );
        if before != after {
            self.window.request_redraw();
        }
    }

    /// O alvo do tooltip: a descrição de uma linha cujo texto foi cortado, sob
    /// o cursor. Descrição que cabe inteira não tem tooltip (ADR-0019).
    fn tooltip_target(
        &self,
        env: Env<'_>,
        point: (f32, f32),
        hit: Option<Hit>,
    ) -> Option<(HoverKey, Rect, String)> {
        let Some(Hit::Row(index)) = hit else {
            return None;
        };
        let m = self.metrics(env);
        let layout = self.layout(env);
        let content = self.content();
        let row = content.row(index)?;
        if !row.description_truncated {
            return None;
        }
        let BlockGeometry::Row(geometry) = content.geometry.blocks.get(index)? else {
            return None;
        };
        let area = Rect {
            x: geometry.description_origin.0 + layout.panel_body.x,
            y: geometry.description_origin.1 + layout.panel_body.y - self.scroll,
            width: geometry.left_width,
            height: m.description_size,
        };
        tab_bar::rect_contains(area, point).then(|| {
            (
                HoverKey::SettingsRow(index),
                area,
                row.description_full.clone(),
            )
        })
    }

    /// O cursor saiu da janela: somem os realces e o tooltip.
    pub(crate) fn cursor_left(&mut self) {
        let changed = self.hovered.take().is_some()
            || self.hovered_button.take().is_some()
            || self.hover.visible().is_some();
        self.hover.dismiss();
        if changed {
            self.window.request_redraw();
        }
    }

    /// Roda do mouse sobre o painel: rola na vertical, limitada ao fim do
    /// conteúdo (RF-16.8).
    pub(crate) fn wheel(
        &mut self,
        delta: MouseScrollDelta,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
    ) {
        self.ensure_content(env, measurer);
        let layout = self.layout(env);
        if !tab_bar::rect_contains(layout.panel_body, self.cursor_logical()) {
            return;
        }
        let pixels = match delta {
            MouseScrollDelta::LineDelta(_, lines) => lines * env.style.overflow_scroll_step,
            MouseScrollDelta::PixelDelta(position) => position.y as f32 / self.scale,
        };
        let content_height = self.content().geometry.content_height;
        self.scroll = layout::clamp_scroll(
            self.scroll - pixels,
            content_height,
            layout.panel_body.height,
        );
        self.hover.dismiss();
        self.window.request_redraw();
    }

    /// Clique esquerdo (ADR-0027): botão de janela, borda de resize ou drag
    /// region do cabeçalho; depois o que o painel tem -- um grupo da guia, o
    /// rodapé, uma linha.
    pub(crate) fn left_pressed(&mut self, env: Env<'_>, measurer: &mut TextMeasurer) -> Press {
        self.ensure_content(env, measurer);
        self.hover.dismiss();
        let style = env.style;
        let resize_border = env.config.appearance.window_controls.resize_border as f32;
        let point = self.cursor_logical();
        if !is_macos() {
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
                return Press::Nothing;
            }
        }
        match self.hit_at(env, point) {
            Some(Hit::Group(group)) => {
                self.focus = Focus::Sidebar;
                self.select_group(group);
                Press::Nothing
            }
            Some(Hit::Footer(FooterButton::OpenFile)) => {
                self.focus = Focus::Footer(FooterButton::OpenFile);
                self.window.request_redraw();
                Press::OpenFile
            }
            // Descartar e Salvar existem, mas sem pendência não fazem nada.
            Some(Hit::Footer(_)) | None => Press::Nothing,
            Some(Hit::Row(index)) => {
                self.focus = Focus::Row(index);
                self.window.request_redraw();
                Press::Nothing
            }
        }
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

    // ---- teclado

    pub(crate) fn modifiers_changed(&mut self, shift: bool) {
        self.shift = shift;
    }

    /// Teclas da janela (RF-16.10, ADR-0059 §3): modo de captura -- o mapa de
    /// teclas do processo não é consultado aqui. `Tab`/`Shift+Tab` percorrem
    /// guia, linhas e rodapé; as setas andam na guia; `Enter`/`Espaço` aciona
    /// o botão focado; `Esc` fecha.
    pub(crate) fn key(
        &mut self,
        event: &KeyEvent,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
    ) -> Press {
        if event.state != winit::event::ElementState::Pressed {
            return Press::Nothing;
        }
        self.ensure_content(env, measurer);
        self.hover.dismiss();
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => Press::Close,
            Key::Named(NamedKey::Tab) => {
                self.move_focus(env, self.shift);
                Press::Nothing
            }
            Key::Named(NamedKey::ArrowDown) if self.focus == Focus::Sidebar => {
                self.step_group(1);
                Press::Nothing
            }
            Key::Named(NamedKey::ArrowUp) if self.focus == Focus::Sidebar => {
                self.step_group(-1);
                Press::Nothing
            }
            Key::Named(NamedKey::Enter | NamedKey::Space)
                if self.focus == Focus::Footer(FooterButton::OpenFile) =>
            {
                Press::OpenFile
            }
            _ => Press::Nothing,
        }
    }

    /// Passa o foco ao próximo ponto de parada e leva a linha à vista.
    fn move_focus(&mut self, env: Env<'_>, backwards: bool) {
        let content = self.content();
        let order = focus_order(&content.row_indices(), &self.available_buttons());
        self.focus = next_focus(&order, self.focus, backwards);
        if let Focus::Row(index) = self.focus {
            let layout = self.layout(env);
            let content = self.content();
            if let Some(BlockGeometry::Row(row)) = content.geometry.blocks.get(index) {
                self.scroll = layout::clamp_scroll(
                    layout::scroll_to_reveal(
                        self.scroll,
                        row.rect.y,
                        row.rect.height,
                        layout.panel_body.height,
                    ),
                    content.geometry.content_height,
                    layout.panel_body.height,
                );
            }
        }
        self.window.request_redraw();
    }

    /// Seta na guia: o grupo vizinho, parando nas pontas.
    fn step_group(&mut self, delta: i32) {
        let index = Group::ALL
            .iter()
            .position(|group| *group == self.selected_group)
            .unwrap_or(0) as i32;
        let next = (index + delta).clamp(0, Group::ALL.len() as i32 - 1) as usize;
        self.select_group(Group::ALL[next]);
    }

    /// Escolhe o grupo: o painel volta ao topo (RF-16.9).
    fn select_group(&mut self, group: Group) {
        if group != self.selected_group {
            self.selected_group = group;
            self.scroll = 0.0;
            self.hover.dismiss();
        }
        if matches!(self.focus, Focus::Row(_)) {
            self.focus = Focus::Sidebar;
        }
        self.window.request_redraw();
    }

    // ---- tooltip no relógio do processo

    pub(crate) fn next_wake(&self) -> Option<Instant> {
        self.hover.next_deadline()
    }

    pub(crate) fn tick(&mut self, now: Instant) {
        let was_visible = self.hover.visible().is_some();
        self.hover.tick(now);
        if was_visible != self.hover.visible().is_some() {
            self.window.request_redraw();
        }
    }

    // ---- acessibilidade

    /// Monta a árvore de acessibilidade e a entrega -- só se houver cliente
    /// conectado, e nunca pede quadro (ADR-0043 §3). Parte do mesmo layout e
    /// do mesmo conteúdo que a pintura lê.
    pub(crate) fn refresh_access_tree(
        &mut self,
        env: Env<'_>,
        measurer: &mut TextMeasurer,
        language: &str,
    ) {
        self.ensure_content(env, measurer);
        let m = self.metrics(env);
        let layout = self.layout(env);
        let groups: Vec<Group> =
            group_items(layout.sidebar, m.sidebar_padding, m.sidebar_item_height)
                .into_iter()
                .map(|(group, _)| group)
                .collect();
        let selected = self.selected_group;
        let has_pending = self.has_pending_changes();
        let catalog = &self.catalog;
        let rows: Vec<&RowView> = self
            .content
            .as_ref()
            .map(|content| {
                content
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Row(row) => Some(row),
                        Block::Section(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.access_adapter.update_if_active(|| {
            access::build_settings_tree(
                &layout,
                &groups,
                selected,
                has_pending,
                &rows,
                catalog,
                language,
            )
        });
    }

    // ---- pintura

    /// Pinta o quadro: painel ao fundo, guia, corpo e rodapé, e -- fora do
    /// macOS -- o cabeçalho por cima. Tudo na camada `Chrome` (ADR-0060 §4); o
    /// tooltip na `Popover`. Camada nova nenhuma.
    pub(crate) fn paint(
        &mut self,
        env: Env<'_>,
        pal: &ResolvedPalette,
        measurer: &mut TextMeasurer,
    ) -> Frame {
        self.ensure_content(env, measurer);
        let m = self.metrics(env);
        let layout = self.layout(env);
        let content = self.content();
        let items = group_items(layout.sidebar, m.sidebar_padding, m.sidebar_item_height);
        let footer = footer_buttons(&m, layout.footer, content.footer_widths);

        let mut out = vec![
            quad(layout.panel, PANEL_BACKGROUND),
            quad(layout.sidebar, pal.bar_background),
            quad(layout.sidebar_separator, pal.editor_divider),
        ];
        out.extend(paint::paint_body(
            &paint::Input {
                layout: &layout,
                metrics: &m,
                content,
                items: &items,
                footer: &footer,
                selected: self.selected_group,
                focus: self.focus,
                hovered: self.hovered,
                scroll: self.scroll,
                pending_groups: &[],
                footer_available: self.available_footer(),
                style: env.style,
                pal,
                config: env.config,
                catalog: &self.catalog,
            },
            measurer,
        ));
        if let Some(header) = layout.header {
            out.push(quad(header, pal.bar_background));
            self.paint_header_content(header, env.style, pal, &mut out);
            chrome::paint_window_buttons(
                env.style,
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
        if let Some((anchor, text)) = self.hover.visible() {
            frame.set_layer(
                Layer::Popover,
                overlay::paint_tooltip(
                    anchor,
                    text,
                    env.config,
                    pal,
                    self.logical_width,
                    self.logical_height,
                    measurer,
                ),
            );
        }
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
