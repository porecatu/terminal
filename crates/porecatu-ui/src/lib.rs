// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use porecatu_core::{TabId, Workspace};
use porecatu_render::{Frame, GpuContext, Layer, WindowSurface};
use porecatu_term::{
    GridSnapshot, Modifiers, PtySize, SpawnConfig, TermEvent, TermParams, Terminal,
    resolve_default_shell, search_path,
};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

mod chrome;
mod clipboard;
mod input;
mod paint;
mod palette;
mod rename;
mod tab_bar;

use chrome::DragGhost;
use input::ClickTracker;
use paint::CellMetrics;
use rename::RenameState;
use tab_bar::{OverflowSide, TabBarHit, TabBarStyle};

/// Espec. §2.19: "limiar de 4px de movimento" -- abaixo disso o gesto é
/// clique (RF-1.13), não arraste.
const DRAG_THRESHOLD_PX: f32 = 4.0;
/// Espec. §2.19: "cursor a menos de 30px de uma ponta da trilha rola
/// naquela direção". A cadência real é "uma aba a cada .15s" -- não há
/// relógio de animação nesta etapa (ADR-0007 é sobre frame ocioso, não
/// sobre um temporizador de UI durante um gesto ativo, mas implementar um
/// de verdade exigiria fiar `ControlFlow::WaitUntil`, fora do escopo desta
/// etapa). Simplificação: o passo acontece por evento de `CursorMoved`
/// dentro da zona, não por intervalo de tempo real -- em uso normal
/// (arraste com o mouse em movimento) o efeito prático é parecido.
const DRAG_EDGE_ZONE_PX: f32 = 30.0;
const DRAG_AUTOSCROLL_STEP_PX: f32 = 12.0;

/// Estado do arraste de reordenação (espec §2.19, RF-1.15). `Pressed` é o
/// meio-caminho entre o clique que já ativou a aba (RF-1.13) e o arraste de
/// verdade -- existe só para medir o limiar de 4px antes de comprometer.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TabDrag {
    Idle,
    Pressed {
        tab: TabId,
        start: (f32, f32),
        /// Deslocamento (em coordenadas de tela) entre o ponto do clique e
        /// o canto esquerdo da aba -- onde dentro da aba o usuário
        /// "segurou". Constante durante o arraste inteiro, mesmo que a
        /// trilha role (o fantasma segue o cursor, não o conteúdo).
        grab_offset: f32,
    },
    Dragging {
        tab: TabId,
        grab_offset: f32,
        /// Índice de inserção calculado no último redraw
        /// (`tab_bar::drag_target_index`) -- é o que `lib.rs` aplica de
        /// verdade ao `Workspace` se o usuário soltar dentro da barra.
        preview_index: usize,
    },
}

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
/// de redraw. Carrega `(WindowId, TabId)` desde a F1: com mais de uma aba
/// (a partir desta etapa) `TabId` sozinho já não seria suficiente para
/// achar a `Terminal` certa dentro de `App::tabs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wakeup {
    window: WindowId,
    tab: TabId,
}

/// Estado de execução de uma aba: o `Terminal` (motor+PTY+threads) e o
/// snapshot reusado entre frames. Vive fora de `porecatu_core::Tab`, que é
/// domínio puro sem I/O (docs/arquitetura.md seção 4) -- a fronteira entre
/// os dois é exatamente `TabId`.
struct TabRuntime {
    terminal: Terminal,
    snapshot: GridSnapshot,
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<GpuContext>,
    window_surface: Option<WindowSurface>,
    /// Pixels físicos por pixel lógico (`window.scale_factor()`). A grade,
    /// a barra de abas e o `Frame` trabalham em lógico; só `WindowSurface`
    /// converte para físico, no único ponto que o ADR-0018 permite.
    scale: f32,
    logical_width: f32,
    logical_height: f32,
    proxy: EventLoopProxy<Wakeup>,
    /// Um `Workspace` por janela (ADR-0015); F2 só abre uma janela, a
    /// segunda fica pra Etapa 6.
    workspace: Workspace,
    tabs: HashMap<TabId, TabRuntime>,
    /// `cwd` do processo do Porecatu no momento em que a janela abriu --
    /// fallback de `tab.new`/`window.new` quando a aba ativa ainda não tem
    /// `cwd` capturado por OSC 7 (ADR-0017 item 1: "aba ativa ->
    /// startup_directory -> home"). `None` só se `current_dir` falhar.
    startup_directory: Option<PathBuf>,
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
    /// Modo de captura do ADR-0008 passo 1 -- rename inline (RF-1.8).
    rename: RenameState,
    /// Deslocamento horizontal da trilha (espec §2.18) -- saturado a cada
    /// redraw por `tab_bar::overflow_state`, nunca lido cru fora dali.
    scroll_offset: f32,
    /// Arraste de reordenação em andamento (espec §2.19), se algum.
    drag: TabDrag,
}

impl App {
    fn new(proxy: EventLoopProxy<Wakeup>) -> Self {
        Self {
            window: None,
            gpu: None,
            window_surface: None,
            scale: 1.0,
            logical_width: 0.0,
            logical_height: 0.0,
            proxy,
            workspace: Workspace::new(),
            tabs: HashMap::new(),
            startup_directory: std::env::current_dir().ok(),
            cell_metrics: CellMetrics {
                width: 1.0,
                height: 1.0,
            },
            modifiers: Modifiers::NONE,
            cursor_position: (0.0, 0.0),
            mouse_button_down: None,
            click_tracker: ClickTracker::default(),
            rename: RenameState::Idle,
            scroll_offset: 0.0,
            drag: TabDrag::Idle,
        }
    }

    /// Altura da barra de abas, em pixels lógicos -- a grade do terminal
    /// começa abaixo dela (Etapa 4).
    fn bar_height(&self) -> f32 {
        chrome::bar_height(&TabBarStyle::DEFAULT)
    }

    /// `cwd` herdado por `tab.new` (ADR-0017 item 1): aba ativa -> diretório
    /// de início do app -> nenhum (o SO decide, tipicamente home).
    fn resolve_new_tab_cwd(&self) -> Option<PathBuf> {
        self.workspace
            .active_tab()
            .and_then(|id| self.workspace.tab(id))
            .and_then(|tab| tab.cwd().cloned())
            .or_else(|| self.startup_directory.clone())
    }

    /// Linhas/colunas atuais a partir do tamanho lógico da janela (descontada
    /// a barra de abas) e da métrica de célula já medida.
    fn grid_size(&self) -> (usize, usize) {
        let content_height = (self.logical_height - self.bar_height()).max(0.0);
        let cols = ((self.logical_width / self.cell_metrics.width) as usize).max(MIN_GRID);
        let rows = ((content_height / self.cell_metrics.height) as usize).max(MIN_GRID);
        (rows, cols)
    }

    /// Nome de exibição do shell (fallback de última instância da
    /// precedência de título, RF-1.7/ADR-0017) -- a mesma resolução que
    /// `porecatu_pty::spawn` usa quando `SpawnConfig.program` é `None`,
    /// reduzida ao nome-base (sem caminho nem extensão).
    fn shell_display_name() -> String {
        let resolved = resolve_default_shell(std::env::var("SHELL").ok().as_deref(), search_path);
        std::path::Path::new(&resolved)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or(resolved)
    }

    /// RF-1.18: rola a trilha o mínimo necessário para que a aba ativa
    /// fique visível -- alinha à borda esquerda se ela está à esquerda da
    /// janela visível, à direita se está à direita, nunca centraliza
    /// (espec §2.18: "centralizar move mais que o necessário").
    fn ensure_active_tab_visible(&mut self) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let Some(active) = self.workspace.active_tab() else {
            return;
        };
        let style = TabBarStyle::DEFAULT;
        let layout = tab_bar::fit_width(
            &self.workspace,
            &style,
            self.logical_width,
            gpu.text_measurer(),
        );
        let Some(rect) = tab_bar::tab_rect(&layout, active) else {
            return;
        };
        let window_start = self.scroll_offset;
        let window_end = self.scroll_offset + self.logical_width;
        if rect.x < window_start {
            self.scroll_offset = rect.x;
        } else if rect.x + rect.width > window_end {
            self.scroll_offset = rect.x + rect.width - self.logical_width;
        }
    }

    fn active_runtime(&self) -> Option<&TabRuntime> {
        let id = self.workspace.active_tab()?;
        self.tabs.get(&id)
    }

    /// Cria uma aba no grupo implícito, herdando `cwd` (RF-1.1, ADR-0017),
    /// e spawna a `Terminal` dela. Erro de spawn desfaz a criação -- sem
    /// isso o `Workspace` acumularia abas sem `Terminal` nenhum atrás.
    fn open_tab(&mut self, window: &Arc<Window>) {
        let (rows, cols) = self.grid_size();
        let cwd = self.resolve_new_tab_cwd();
        let shell_name = Self::shell_display_name();
        let pos = self
            .workspace
            .groups()
            .first()
            .map_or(0, |g| g.tabs().len());
        let tab_id = self.workspace.new_tab(shell_name, cwd.clone(), pos);

        let window_id = window.id();
        let tab = tab_id;
        let proxy = self.proxy.clone();
        let pty_config = SpawnConfig {
            program: None,
            args: Vec::new(),
            env: Vec::new(),
            cwd,
            size: PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            },
        };

        match Terminal::spawn(pty_config, TermParams::default(), move || {
            let _ = proxy.send_event(Wakeup {
                window: window_id,
                tab,
            });
        }) {
            Ok(terminal) => {
                self.tabs.insert(
                    tab_id,
                    TabRuntime {
                        terminal,
                        snapshot: GridSnapshot::default(),
                    },
                );
            }
            // `porecatu-ui` ainda não tem superfície de aviso (ADR-0014, F2);
            // `stderr` é o único canal disponível nesta fase. Desfaz a aba
            // que o `Workspace` acabou de criar -- sem `Terminal`, ela não
            // serve pra nada.
            Err(err) => {
                eprintln!("porecatu: falha ao iniciar terminal: {err}");
                self.workspace.close_tab(tab_id);
            }
        }
        self.sync_window_title();
        self.ensure_active_tab_visible();
    }

    /// Fecha uma aba: sinaliza o processo sem bloquear (ADR-0017 item 4 --
    /// `Terminal::close` devolve na hora, ninguém espera a confirmação
    /// aqui) e remove do `Workspace`. Mesmo caminho serve tanto para
    /// `tab.close` interativo quanto para o encerramento natural do
    /// processo com código zero (RF-1.3: só código ≠ 0 mantém a aba
    /// aberta).
    ///
    /// RF-1.6 (confirmar se a aba tem processo em primeiro plano) ainda não
    /// tem diálogo pra mostrar -- o widget é da Etapa 6 (ADR-0014). Fecha
    /// sempre, sem perguntar; é uma lacuna documentada, não um esquecimento.
    fn close_tab(&mut self, id: TabId, event_loop: &ActiveEventLoop) {
        if self.rename.editing_tab() == Some(id) {
            self.rename = RenameState::Idle;
        }
        if let Some(runtime) = self.tabs.remove(&id) {
            let _ = runtime.terminal.close();
        }
        self.workspace.close_tab(id);
        self.sync_window_title();
        self.ensure_active_tab_visible();

        if self.workspace.active_tab().is_none() {
            // `window.close`/RF-1.4 são Etapa 6 -- sem uma segunda aba ou
            // janela pra focar, sair é o comportamento de trabalho até lá.
            event_loop.exit();
            return;
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Sincroniza o título da janela do SO com o título da aba ativa
    /// (RF-1.7, já com a precedência do ADR-0017 aplicada por
    /// `Tab::title`). Chamado depois de qualquer mudança que possa afetar
    /// o resultado: nova aba, fechar, ativar, título vindo do processo,
    /// renomear.
    fn sync_window_title(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let title = self
            .workspace
            .active_tab()
            .and_then(|id| self.workspace.tab(id))
            .map(|tab| tab.title())
            .unwrap_or("Porecatu");
        window.set_title(title);
    }

    /// Confirma o rename em andamento, se algum (RF-1.8/RF-1.9): buffer
    /// vazio limpa o título customizado (volta ao automático); não-vazio
    /// vira o novo `custom_title`. Chamado por `Enter` e por "blur" --
    /// clicar em outra aba, ou fechar a que está sendo renomeada, também
    /// confirma (espec. §2.5: "Confirma em Enter e no blur").
    fn commit_rename(&mut self) {
        let RenameState::Editing { tab, buffer } = std::mem::take(&mut self.rename) else {
            return;
        };
        let trimmed = buffer.trim();
        let title = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        if let Some(t) = self.workspace.tab_mut(tab) {
            t.set_custom_title(title);
        }
        self.sync_window_title();
    }

    fn action_new_tab(&mut self) {
        if self.rename.editing_tab().is_some() {
            self.commit_rename();
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        self.open_tab(&window);
    }

    fn action_close_tab(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(id) = self.workspace.active_tab() {
            self.close_tab(id, event_loop);
        }
    }

    fn action_rename_start(&mut self) {
        let Some(id) = self.workspace.active_tab() else {
            return;
        };
        let Some(tab) = self.workspace.tab(id) else {
            return;
        };
        self.rename = RenameState::Editing {
            tab: id,
            buffer: tab.title().to_string(),
        };
    }

    fn action_next_tab(&mut self) {
        self.workspace.next_tab();
        self.sync_window_title();
        self.ensure_active_tab_visible();
    }

    fn action_prev_tab(&mut self) {
        self.workspace.prev_tab();
        self.sync_window_title();
        self.ensure_active_tab_visible();
    }

    fn action_goto(&mut self, visual_index: usize) {
        self.workspace.activate_visual_index(visual_index);
        self.sync_window_title();
        self.ensure_active_tab_visible();
    }

    /// RF-1.17: reordenação por teclado, uma posição por vez. F2 só tem o
    /// grupo implícito (ADR-0006), então a ordem visual inteira é a ordem
    /// do grupo -- `visual_order` já dá a posição certa pra `move_tab`.
    fn action_move_tab(&mut self, delta: isize) {
        let Some(active) = self.workspace.active_tab() else {
            return;
        };
        let order: Vec<TabId> = self.workspace.visual_order().collect();
        let Some(index) = order.iter().position(|&id| id == active) else {
            return;
        };
        let Some(target) = index.checked_add_signed(delta) else {
            return;
        };
        if target >= order.len() {
            return;
        }
        self.workspace.move_tab(active, target);
        self.ensure_active_tab_visible();
    }

    /// Passo 1 do ADR-0008 (modo de captura): consome tudo, exceto `Esc` e
    /// `Enter` (que confirmam/cancelam), sem repassar nem pro roteamento de
    /// keybind nem pro terminal.
    fn handle_rename_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        match &event.logical_key {
            Key::Named(NamedKey::Enter) => self.commit_rename(),
            Key::Named(NamedKey::Escape) => self.rename = RenameState::Idle,
            Key::Named(NamedKey::Backspace) => self.rename.backspace(),
            _ => {
                if let Some(text) = &event.text {
                    for c in text.chars().filter(|c| !c.is_control()) {
                        self.rename.push_char(c);
                    }
                }
            }
        }
    }

    /// Passo 2 do ADR-0008 para as ações de aba que a Etapa 4 introduz --
    /// defaults fixos de Windows/Linux (não há parser de `[keybindings]`
    /// até a F4, docs/reference/acoes.md). `Ctrl+Shift+C`/`V` (copiar/
    /// colar) e a rolagem continuam resolvidos por `input::handle_keyboard_input`,
    /// chamado depois se isto devolver `false`. Devolve se a tecla foi
    /// consumida -- um binding que casa nunca cai para o terminal.
    fn handle_tab_action_key(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) -> bool {
        if event.state != ElementState::Pressed {
            return false;
        }
        let m = self.modifiers;
        if m.ctrl && m.shift && !m.alt {
            match &event.logical_key {
                Key::Character(s) if s.eq_ignore_ascii_case("t") => {
                    self.action_new_tab();
                    return true;
                }
                Key::Character(s) if s.eq_ignore_ascii_case("w") => {
                    self.action_close_tab(event_loop);
                    return true;
                }
                Key::Character(s) if s.eq_ignore_ascii_case("r") => {
                    self.action_rename_start();
                    return true;
                }
                // `Ctrl+Shift+Tab`: única exceção ao padrão `Ctrl+Shift`
                // (ADR-0008), convenção universal de troca de aba.
                Key::Named(NamedKey::Tab) => {
                    self.action_prev_tab();
                    return true;
                }
                // RF-1.17, `docs/config/porecatu.example.toml`
                // `[keybindings]`: "ctrl+shift+left" = "tab.move_left".
                Key::Named(NamedKey::ArrowLeft) => {
                    self.action_move_tab(-1);
                    return true;
                }
                Key::Named(NamedKey::ArrowRight) => {
                    self.action_move_tab(1);
                    return true;
                }
                _ => {}
            }
        }
        if m.ctrl && !m.shift && !m.alt && matches!(event.logical_key, Key::Named(NamedKey::Tab)) {
            self.action_next_tab();
            return true;
        }
        if m.alt
            && !m.ctrl
            && !m.shift
            && let Key::Character(s) = &event.logical_key
            && let Some(digit) = s.chars().next().and_then(|c| c.to_digit(10))
            && (1..=9).contains(&digit)
        {
            self.action_goto((digit - 1) as usize);
            return true;
        }
        false
    }

    /// Resolve o que um clique na área da barra de abas atinge e dispara a
    /// ação correspondente. Indicadores de overflow (espec §2.18) primeiro
    /// -- ficam em coordenadas de tela, fora do recorte que rola com a
    /// trilha; só então o clique é convertido pra coordenadas de conteúdo
    /// (somando `scroll_offset`) e testado contra `tab_bar::hit_test`, que
    /// espera as mesmas coordenadas não-roladas de `fit_width`. Clicar numa
    /// aba diferente da que está sendo renomeada confirma o rename primeiro
    /// (espec. §2.5: "blur" confirma"); clicar no corpo de uma aba também
    /// arma o possível arraste do RF-1.15 (`TabDrag::Pressed`), resolvido
    /// de verdade só se o movimento passar do limiar em `CursorMoved`.
    fn handle_bar_click(&mut self, logical_point: (f32, f32), event_loop: &ActiveEventLoop) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let style = TabBarStyle::DEFAULT;
        let bar_width = self.logical_width;
        let bar_height = chrome::bar_height(&style);
        let layout = tab_bar::fit_width(&self.workspace, &style, bar_width, gpu.text_measurer());
        let overflow = tab_bar::overflow_state(&layout, bar_width, self.scroll_offset);
        self.scroll_offset = overflow.scroll_offset;

        if overflow.hidden_left > 0
            && tab_bar::point_in_overflow_pill(
                OverflowSide::Left,
                bar_width,
                bar_height,
                logical_point,
            )
        {
            self.scroll_offset = (self.scroll_offset - tab_bar::OVERFLOW_SCROLL_STEP).max(0.0);
            self.request_redraw();
            return;
        }
        if overflow.hidden_right > 0
            && tab_bar::point_in_overflow_pill(
                OverflowSide::Right,
                bar_width,
                bar_height,
                logical_point,
            )
        {
            self.scroll_offset += tab_bar::OVERFLOW_SCROLL_STEP;
            self.request_redraw();
            return;
        }

        let content_point = (logical_point.0 + self.scroll_offset, logical_point.1);
        let Some(hit) = tab_bar::hit_test(&layout, content_point) else {
            return;
        };
        match hit {
            TabBarHit::Tab(id) => {
                if self
                    .rename
                    .editing_tab()
                    .is_some_and(|current| current != id)
                {
                    self.commit_rename();
                }
                self.workspace.activate_tab(id);
                self.sync_window_title();
                self.ensure_active_tab_visible();
                if let Some(rect) = tab_bar::tab_rect(&layout, id) {
                    let screen_x = rect.x - self.scroll_offset;
                    self.drag = TabDrag::Pressed {
                        tab: id,
                        start: logical_point,
                        grab_offset: logical_point.0 - screen_x,
                    };
                }
            }
            TabBarHit::CloseButton(id) => self.close_tab(id, event_loop),
            TabBarHit::NewTabButton => self.action_new_tab(),
        }
        self.request_redraw();
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Solta o botão do mouse com um arraste em andamento (espec §2.19):
    /// aplica o `preview_index` calculado no último redraw se o cursor
    /// ainda está dentro da barra, ou cancela em silêncio se não está --
    /// "soltar fora da trilha cancela também". `self.workspace` nunca foi
    /// mexido durante o arraste (só o preview clonado em `RedrawRequested`
    /// era), então cancelar é só voltar `drag` pra `Idle`, sem desfazer
    /// nada. Também cobre soltar um `Pressed` que nunca virou arraste --
    /// não há o que aplicar, o clique já ativou a aba na hora do `press`.
    fn finish_drag(&mut self) {
        let drag = std::mem::replace(&mut self.drag, TabDrag::Idle);
        if let TabDrag::Dragging {
            tab, preview_index, ..
        } = drag
            && self.in_bar(self.cursor_position.1)
        {
            self.workspace.move_tab(tab, preview_index);
        }
        if let Some(window) = &self.window {
            window.set_cursor(CursorIcon::Default);
        }
    }

    /// `y` físico está dentro da faixa da barra de abas (topo da janela).
    fn in_bar(&self, physical_y: f64) -> bool {
        physical_y < (self.bar_height() * self.scale) as f64
    }

    /// Recalcula linhas/colunas a partir do tamanho lógico e propaga pra
    /// `WindowSurface` e pro terminal da aba ativa (motor + PTY).
    /// `WindowSurface` é quem converte de volta para físico (ADR-0018).
    fn resize_to(&mut self, width: u32, height: u32) {
        if let (Some(gpu), Some(window_surface)) = (&self.gpu, &mut self.window_surface) {
            window_surface.resize(gpu, width, height, self.scale);
        }
        self.logical_width = width as f32 / self.scale;
        self.logical_height = height as f32 / self.scale;
        let (rows, cols) = self.grid_size();
        if let Some(runtime) = self.active_runtime() {
            runtime.terminal.resize(rows, cols);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn cell_at_cursor(&self) -> input::CellPosition {
        let content_y = self.cursor_position.1 - (self.bar_height() * self.scale) as f64;
        let (rows, cols) = self.active_runtime().map_or((MIN_GRID, MIN_GRID), |rt| {
            (
                rt.snapshot.rows.max(MIN_GRID),
                rt.snapshot.cols.max(MIN_GRID),
            )
        });
        input::cell_at(
            self.cursor_position.0,
            content_y.max(0.0),
            self.cell_metrics,
            rows,
            cols,
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
        // pra lógico, que é o que a grade, a barra de abas e o `Frame`
        // usam daqui em diante -- só `WindowSurface` volta a físico
        // (ADR-0018).
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
        self.logical_width = size.width as f32 / scale;
        self.logical_height = size.height as f32 / scale;

        self.gpu = Some(gpu);
        self.window_surface = Some(window_surface);
        // Antes de `open_tab`: `sync_window_title` (chamado por ela) só
        // acha a janela pra escrever o título se `self.window` já estiver
        // preenchido.
        self.window = Some(Arc::clone(&window));
        self.open_tab(&window);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Wakeup) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if event.window != window.id() {
            return;
        }
        let tab_id = event.tab;

        // Aba suja que não é a visível: só marca o indicador de atividade
        // (RF-1.20) -- sem redraw, ela não está na tela (ADR-0007 ponto 2).
        if self.workspace.active_tab() != Some(tab_id)
            && let Some(tab) = self.workspace.tab_mut(tab_id)
            && !tab.is_exited()
        {
            tab.mark_activity();
        }

        let mut pending = Vec::new();
        if let Some(runtime) = self.tabs.get(&tab_id) {
            while let Some(term_event) = runtime.terminal.try_recv_event() {
                pending.push(term_event);
            }
        }

        for term_event in pending {
            match term_event {
                TermEvent::Title(title) => {
                    if let Some(tab) = self.workspace.tab_mut(tab_id) {
                        tab.set_process_title(title);
                    }
                }
                // RF-1.21 (indicador de campainha): só sinaliza em segundo
                // plano -- em primeiro plano o usuário já está vendo.
                TermEvent::Bell => {
                    if self.workspace.active_tab() != Some(tab_id)
                        && let Some(tab) = self.workspace.tab_mut(tab_id)
                    {
                        tab.mark_bell();
                    }
                }
                // OSC 52 escrita: vai pro clipboard do sistema, sujeito
                // ao teto de tamanho já aplicado em porecatu-term
                // (RF-10.10). Leitura: só dispara quando
                // `TermParams::osc52_read` permite -- `false` por
                // default (RF-10.11), então na prática este braço só
                // roda se isso um dia virar configurável (F4).
                TermEvent::ClipboardWrite(text) => clipboard::copy(&text),
                TermEvent::ClipboardRead(responder) => {
                    if let Some(runtime) = self.tabs.get(&tab_id) {
                        let content = clipboard::paste().unwrap_or_default();
                        runtime
                            .terminal
                            .write(responder.respond(&content).into_bytes());
                    }
                }
                // Depende de tema resolvido -- F4.
                TermEvent::ColorQuery(_) => {}
                TermEvent::Cwd(cwd) => {
                    if let Some(tab) = self.workspace.tab_mut(tab_id) {
                        tab.set_cwd(cwd);
                    }
                }
                TermEvent::Exit { success, code } => {
                    if success {
                        // RF-1.3: código zero não deixa rastro -- a aba
                        // fecha (mesmo caminho de `tab.close`).
                        self.close_tab(tab_id, event_loop);
                    } else {
                        if let Some(runtime) = self.tabs.get(&tab_id) {
                            runtime.terminal.inject_note(
                                &format!("processo encerrado (código {code})"),
                                palette::NOTE_ACCENT_RGB,
                            );
                        }
                        if let Some(tab) = self.workspace.tab_mut(tab_id) {
                            tab.mark_exited(code as i32);
                        }
                    }
                }
            }
        }

        self.sync_window_title();
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
                // Sinaliza todas as abas primeiro, espera depois -- o
                // custo é uma volta de sinalização, não N × timeout
                // (ADR-0017 item 4, critério de saída da F2: "fechar uma
                // janela com 50 abas não bloqueia a main thread").
                let waits: Vec<_> = self
                    .tabs
                    .drain()
                    .map(|(_, runtime)| runtime.terminal.close())
                    .collect();
                for wait in waits {
                    wait.wait();
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
                // `Esc` cancela o arraste em andamento (espec §2.19: "Esc
                // cancela e a aba volta à origem") antes de qualquer outro
                // roteamento -- o `Workspace` real nunca foi tocado, só
                // solta o estado.
                if matches!(self.drag, TabDrag::Dragging { .. })
                    && key.state == ElementState::Pressed
                    && matches!(key.logical_key, Key::Named(NamedKey::Escape))
                {
                    self.drag = TabDrag::Idle;
                    if let Some(window) = &self.window {
                        window.set_cursor(CursorIcon::Default);
                        window.request_redraw();
                    }
                } else if self.rename.editing_tab().is_some() {
                    self.handle_rename_key(&key);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                } else if self.handle_tab_action_key(&key, event_loop) {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                } else if let Some(runtime) = self.active_runtime() {
                    // Modos lidos agora, não do snapshot do último frame
                    // (que pode estar obsoleto -- ex.: o programa acabou
                    // de ligar DECCKM e ainda não houve redraw).
                    input::handle_keyboard_input(
                        &runtime.terminal,
                        &runtime.terminal.modes(),
                        &key,
                        self.modifiers,
                    );
                }
            }
            // Composição de IME (CJK) e tecla morta já resolvida pelo SO
            // (ABNT2) -- passam direto pro terminal, sem consultar
            // keybind nenhum (ADR-0008). `Preedit`/`Enabled`/`Disabled`
            // não geram bytes: a composição em andamento não é texto
            // final ainda. Desenhar o preedit sobre o cursor fica para
            // quando houver render de chrome sobre o texto (F2+).
            WindowEvent::Ime(Ime::Commit(text)) => {
                if let Some(runtime) = self.active_runtime() {
                    runtime.terminal.write(text.into_bytes());
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.in_bar(self.cursor_position.1) {
                    // Espec §2.18: "roda do mouse sobre a barra rola a
                    // trilha na horizontal, com ou sem Shift... passo de
                    // 90px por notch". Sinal da roda decide a direção; sem
                    // inércia nem easing (mesma razão do indicador que não
                    // pisca -- rolagem contínua seria um frame por quadro
                    // de animação).
                    let notches = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as f32,
                    };
                    if notches != 0.0 {
                        self.scroll_offset -= notches.signum() * tab_bar::OVERFLOW_SCROLL_STEP;
                        self.scroll_offset = self.scroll_offset.max(0.0);
                        self.request_redraw();
                    }
                } else if let Some(runtime) = self.active_runtime() {
                    let cell = self.cell_at_cursor();
                    input::handle_mouse_wheel(
                        &runtime.terminal,
                        &runtime.terminal.modes(),
                        delta,
                        self.modifiers,
                        cell,
                    );
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = (position.x, position.y);
                let handled_by_drag = match &mut self.drag {
                    TabDrag::Pressed {
                        tab,
                        start,
                        grab_offset,
                    } => {
                        let logical = (
                            position.x as f32 / self.scale,
                            position.y as f32 / self.scale,
                        );
                        let past_threshold = (logical.0 - start.0).abs() > DRAG_THRESHOLD_PX
                            || (logical.1 - start.1).abs() > DRAG_THRESHOLD_PX;
                        if past_threshold {
                            let dragging = TabDrag::Dragging {
                                tab: *tab,
                                grab_offset: *grab_offset,
                                preview_index: 0,
                            };
                            self.drag = dragging;
                            if let Some(window) = &self.window {
                                window.set_cursor(CursorIcon::Grabbing);
                            }
                        }
                        true
                    }
                    TabDrag::Dragging { .. } => {
                        // Auto-scroll nas bordas (espec §2.19) -- ver nota
                        // de `DRAG_EDGE_ZONE_PX` sobre a simplificação da
                        // cadência.
                        let logical_x = position.x as f32 / self.scale;
                        if logical_x < DRAG_EDGE_ZONE_PX {
                            self.scroll_offset =
                                (self.scroll_offset - DRAG_AUTOSCROLL_STEP_PX).max(0.0);
                        } else if logical_x > self.logical_width - DRAG_EDGE_ZONE_PX {
                            self.scroll_offset += DRAG_AUTOSCROLL_STEP_PX;
                        }
                        true
                    }
                    TabDrag::Idle => false,
                };
                if handled_by_drag {
                    self.request_redraw();
                } else if !self.in_bar(position.y)
                    && let Some(runtime) = self.active_runtime()
                {
                    let cell = self.cell_at_cursor();
                    input::handle_mouse_motion(
                        &runtime.terminal,
                        &runtime.terminal.modes(),
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
                let pressed = state == ElementState::Pressed;
                if button == MouseButton::Left && !pressed && !matches!(self.drag, TabDrag::Idle) {
                    self.finish_drag();
                } else if pressed
                    && button == MouseButton::Left
                    && self.in_bar(self.cursor_position.1)
                {
                    let logical_point = (
                        self.cursor_position.0 as f32 / self.scale,
                        self.cursor_position.1 as f32 / self.scale,
                    );
                    self.handle_bar_click(logical_point, event_loop);
                } else {
                    self.mouse_button_down = if pressed { Some(button) } else { None };
                    if !self.in_bar(self.cursor_position.1) {
                        let cell = self.cell_at_cursor();
                        let active_id = self.workspace.active_tab();
                        if let Some(runtime) = active_id.and_then(|id| self.tabs.get(&id)) {
                            input::handle_mouse_button(
                                &runtime.terminal,
                                &runtime.terminal.modes(),
                                button,
                                pressed,
                                cell,
                                self.modifiers,
                                &mut self.click_tracker,
                            );
                        }
                    }
                }
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let mut frame = Frame::new();

                if let Some(gpu) = &mut self.gpu {
                    let style = TabBarStyle::DEFAULT;
                    let bar_width = self.logical_width;
                    let base_layout =
                        tab_bar::fit_width(&self.workspace, &style, bar_width, gpu.text_measurer());
                    let overflow =
                        tab_bar::overflow_state(&base_layout, bar_width, self.scroll_offset);
                    self.scroll_offset = overflow.scroll_offset;

                    // Durante um arraste, o `Workspace` de verdade não é
                    // tocado -- só um clone com o preview de reordenação
                    // aplicado é usado pra desenhar (espec §2.19: as
                    // vizinhas "deslizam" mostrando onde a aba cairia; o
                    // `Workspace` real só recebe a troca ao soltar, em
                    // `finish_drag`).
                    let mut drag_ghost = None;
                    let paint_layout = if let TabDrag::Dragging {
                        tab, grab_offset, ..
                    } = self.drag
                    {
                        let cursor_logical_x = self.cursor_position.0 as f32 / self.scale;
                        let ghost_screen_x = cursor_logical_x - grab_offset;
                        let ghost_content_x = ghost_screen_x + self.scroll_offset;
                        let width = tab_bar::tab_rect(&base_layout, tab)
                            .map(|r| r.width)
                            .unwrap_or(0.0);
                        let ghost_center = ghost_content_x + width / 2.0;
                        let preview_index =
                            tab_bar::drag_target_index(&base_layout, tab, ghost_center);
                        self.drag = TabDrag::Dragging {
                            tab,
                            grab_offset,
                            preview_index,
                        };
                        drag_ghost = Some(DragGhost {
                            tab,
                            screen_x: ghost_screen_x,
                        });

                        let mut preview = self.workspace.clone();
                        preview.move_tab(tab, preview_index);
                        tab_bar::fit_width(&preview, &style, bar_width, gpu.text_measurer())
                    } else {
                        base_layout
                    };

                    let chrome_primitives = chrome::paint(
                        &paint_layout,
                        &self.workspace,
                        self.workspace.active_tab(),
                        &self.rename,
                        &style,
                        bar_width,
                        overflow,
                        drag_ghost,
                        gpu.text_measurer(),
                    );
                    frame.set_layer(Layer::Chrome, chrome_primitives);
                }

                let bar_height = self.bar_height();
                if let Some(id) = self.workspace.active_tab()
                    && let Some(runtime) = self.tabs.get_mut(&id)
                {
                    runtime.terminal.snapshot_into(&mut runtime.snapshot);
                    let primitives = paint::build_primitives(
                        &runtime.snapshot,
                        self.cell_metrics,
                        FONT_SIZE_PX,
                        bar_height,
                    );
                    frame.set_layer(Layer::Grid, primitives);
                }

                if let (Some(gpu), Some(window_surface)) = (&mut self.gpu, &mut self.window_surface)
                {
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
