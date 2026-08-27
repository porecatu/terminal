// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use porecatu_core::{TabId, Workspace};
use porecatu_render::{Frame, GpuContext, Layer, Rect, WindowSurface};
use porecatu_term::{
    GridSnapshot, Modifiers, MouseReporting, PtySize, SpawnConfig, TermEvent, TermParams, Terminal,
    resolve_default_shell, search_path,
};
use winit::application::ApplicationHandler;
use winit::event::{
    ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

mod chrome;
mod clipboard;
mod context_menu;
mod dialog;
mod input;
mod overlay;
mod paint;
mod palette;
mod rename;
mod tab_bar;
mod tooltip;
mod warning;

use chrome::DragGhost;
use context_menu::{ContextMenu, MenuAction};
use dialog::{ConfirmDialog, DialogAction, DialogButton};
use input::ClickTracker;
use paint::CellMetrics;
use rename::RenameState;
use tab_bar::{OverflowSide, TabBarHit, TabBarStyle};
use tooltip::Hover;
use warning::{Severity, WarningStack};

/// Espec. §2.19: "limiar de 4px de movimento" -- abaixo disso o gesto é
/// clique (RF-1.13), não arraste.
const DRAG_THRESHOLD_PX: f32 = 4.0;
/// Espec. §2.19: "cursor a menos de 30px de uma ponta da trilha rola
/// naquela direção". A cadência real é "uma aba a cada .15s" -- simplificado
/// pra andar por evento de `CursorMoved` dentro da zona, não por intervalo
/// de tempo real (ver a nota completa na Etapa 5, `docs/arquitetura.md`).
const DRAG_EDGE_ZONE_PX: f32 = 30.0;
const DRAG_AUTOSCROLL_STEP_PX: f32 = 12.0;

/// ADR-0015: deslocamento de cascata da janela nova em relação à que a
/// criou, em pixels físicos.
const NEW_WINDOW_CASCADE_PX: i32 = 30;

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
/// de redraw. Carrega `(WindowId, TabId)` desde a F1 (ADR-0015): com mais
/// de uma janela, `TabId` sozinho não diz qual `Workspace` sujou -- os
/// contadores são por janela.
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

/// Altura da barra de abas, em pixels lógicos -- não depende de estado de
/// janela nenhum, só do estilo default (`porecatu-config` não existe
/// ainda).
fn bar_height() -> f32 {
    chrome::bar_height(&TabBarStyle::DEFAULT)
}

/// O que `WindowState::handle_tab_action_key` não pode resolver sozinho --
/// `window.new`/`window.close` (ADR-0015) tocam outras janelas, então
/// precisam voltar pra `App`.
enum ActionOutcome {
    Handled,
    OpenWindow,
    CloseWindowRequested,
    Unhandled,
}

/// Estado por janela (ADR-0015: "um `Workspace` independente por janela").
/// `App` guarda um `HashMap<WindowId, WindowState>`; o que não varia por
/// janela (GPU do processo, diretório de início, métricas de célula --
/// DPI-independentes em pixels lógicos) fica em `App`.
struct WindowState {
    window: Arc<Window>,
    window_surface: WindowSurface,
    /// Pixels físicos por pixel lógico (`window.scale_factor()`). A grade,
    /// a barra de abas e o `Frame` trabalham em lógico; só `WindowSurface`
    /// converte para físico, no único ponto que o ADR-0018 permite.
    scale: f32,
    logical_width: f32,
    logical_height: f32,
    workspace: Workspace,
    tabs: HashMap<TabId, TabRuntime>,
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
    /// Hover/tooltip do ADR-0019 -- RF-1.10, título truncado da aba.
    hover: Hover,
    /// Aviso do app (ADR-0014 canal 1, RF-10.15/RF-10.16).
    warnings: WarningStack,
    /// Diálogo de confirmação em andamento (ADR-0014, RF-10.18), no máximo
    /// um por janela -- "modal é por janela, não por app".
    dialog: Option<ConfirmDialog>,
    /// Menu de contexto de aba em andamento (ADR-0014 §2.16, RF-10.19).
    context_menu: Option<ContextMenu>,
}

impl WindowState {
    fn new(window: Arc<Window>, window_surface: WindowSurface, scale: f32) -> Self {
        let size = window.inner_size();
        Self {
            window,
            window_surface,
            scale,
            logical_width: size.width as f32 / scale,
            logical_height: size.height as f32 / scale,
            workspace: Workspace::new(),
            tabs: HashMap::new(),
            modifiers: Modifiers::NONE,
            cursor_position: (0.0, 0.0),
            mouse_button_down: None,
            click_tracker: ClickTracker::default(),
            rename: RenameState::Idle,
            scroll_offset: 0.0,
            drag: TabDrag::Idle,
            hover: Hover::default(),
            warnings: WarningStack::default(),
            dialog: None,
            context_menu: None,
        }
    }

    /// `cwd` herdado por `tab.new`/`window.new` (ADR-0017 item 1, ADR-0015):
    /// aba ativa -> diretório de início do app -> nenhum (o SO decide,
    /// tipicamente home).
    fn resolve_new_tab_cwd(&self, startup_directory: &Option<PathBuf>) -> Option<PathBuf> {
        self.workspace
            .active_tab()
            .and_then(|id| self.workspace.tab(id))
            .and_then(|tab| tab.cwd().cloned())
            .or_else(|| startup_directory.clone())
    }

    /// Linhas/colunas atuais a partir do tamanho lógico da janela (descontada
    /// a barra de abas) e da métrica de célula já medida.
    fn grid_size(&self, cell_metrics: CellMetrics) -> (usize, usize) {
        let content_height = (self.logical_height - bar_height()).max(0.0);
        let cols = ((self.logical_width / cell_metrics.width) as usize).max(MIN_GRID);
        let rows = ((content_height / cell_metrics.height) as usize).max(MIN_GRID);
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
    fn ensure_active_tab_visible(&mut self, gpu: &mut GpuContext) {
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
    /// e spawna a `Terminal` dela. Erro de spawn desfaz a criação e vira
    /// aviso do app (canal 1, ADR-0014) -- sem isso o `Workspace`
    /// acumularia abas sem `Terminal` nenhum atrás.
    fn open_tab(
        &mut self,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        cwd: Option<PathBuf>,
        now: Instant,
    ) {
        let (rows, cols) = self.grid_size(cell_metrics);
        let shell_name = Self::shell_display_name();
        let pos = self
            .workspace
            .groups()
            .first()
            .map_or(0, |g| g.tabs().len());
        let tab_id = self.workspace.new_tab(shell_name, cwd.clone(), pos);

        let window_id = self.window.id();
        let tab = tab_id;
        let proxy = proxy.clone();
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
            Err(err) => {
                self.warnings.push(
                    Severity::Error,
                    "Falha ao iniciar terminal",
                    err.to_string(),
                    now,
                );
                self.workspace.close_tab(tab_id);
            }
        }
        self.sync_window_title();
    }

    /// Fecha uma aba sem perguntar: sinaliza o processo sem bloquear
    /// (ADR-0017 item 4 -- `Terminal::close` devolve na hora) e remove do
    /// `Workspace`. Mesmo caminho serve tanto para `tab.close` já
    /// confirmado (ou sem necessidade de confirmar) quanto para o
    /// encerramento natural do processo com código zero (RF-1.3). Devolve
    /// `true` se a janela ficou sem abas -- quem chama decide o que fazer
    /// (fechar a janela, sair do app).
    fn close_tab_unconditionally(&mut self, id: TabId) -> bool {
        if self.rename.editing_tab() == Some(id) {
            self.rename = RenameState::Idle;
        }
        if self.context_menu.is_some_and(|m| m.tab == id) {
            self.context_menu = None;
        }
        if let Some(runtime) = self.tabs.remove(&id) {
            let _ = runtime.terminal.close();
        }
        self.workspace.close_tab(id);
        self.sync_window_title();
        self.workspace.active_tab().is_none()
    }

    /// Sincroniza o título da janela do SO com o título da aba ativa
    /// (RF-1.7, já com a precedência do ADR-0017 aplicada por
    /// `Tab::title`). Chamado depois de qualquer mudança que possa afetar
    /// o resultado: nova aba, fechar, ativar, título vindo do processo,
    /// renomear.
    fn sync_window_title(&self) {
        let title = self
            .workspace
            .active_tab()
            .and_then(|id| self.workspace.tab(id))
            .map(|tab| tab.title())
            .unwrap_or("Porecatu");
        self.window.set_title(title);
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

    fn action_new_tab(
        &mut self,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        startup_directory: &Option<PathBuf>,
        now: Instant,
    ) {
        if self.rename.editing_tab().is_some() {
            self.commit_rename();
        }
        let cwd = self.resolve_new_tab_cwd(startup_directory);
        self.open_tab(cell_metrics, proxy, cwd, now);
    }

    /// RF-1.6 (ADR-0017): fechar a aba ativa pede confirmação quando ela
    /// tem tela alternativa ou reporte de mouse ligado -- o proxy de
    /// "processo em primeiro plano" que o app pode observar sem varrer a
    /// árvore de processos. Sem isso, fecha direto.
    fn action_close_tab(&mut self) -> Option<ConfirmDialog> {
        let id = self.workspace.active_tab()?;
        let runtime = self.tabs.get(&id)?;
        let modes = runtime.terminal.modes();
        if modes.alt_screen || modes.mouse_reporting != MouseReporting::None {
            let title = self
                .workspace
                .tab(id)
                .map(|t| t.title().to_string())
                .unwrap_or_default();
            return Some(ConfirmDialog::new(
                "Fechar aba?",
                format!("\"{title}\" tem um programa em primeiro plano. Fechar mesmo assim?"),
                "Fechar aba",
                DialogAction::CloseTab(id),
            ));
        }
        self.close_tab_unconditionally(id);
        None
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

    fn action_next_tab(&mut self, gpu: &mut GpuContext) {
        self.workspace.next_tab();
        self.sync_window_title();
        self.ensure_active_tab_visible(gpu);
    }

    fn action_prev_tab(&mut self, gpu: &mut GpuContext) {
        self.workspace.prev_tab();
        self.sync_window_title();
        self.ensure_active_tab_visible(gpu);
    }

    fn action_goto(&mut self, visual_index: usize, gpu: &mut GpuContext) {
        self.workspace.activate_visual_index(visual_index);
        self.sync_window_title();
        self.ensure_active_tab_visible(gpu);
    }

    /// RF-1.17: reordenação por teclado, uma posição por vez. F2 só tem o
    /// grupo implícito (ADR-0006), então a ordem visual inteira é a ordem
    /// do grupo -- `visual_order` já dá a posição certa pra `move_tab`.
    fn action_move_tab(&mut self, delta: isize, gpu: &mut GpuContext) {
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
        self.ensure_active_tab_visible(gpu);
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

    /// `Enter` aciona o botão focado, `Esc` cancela (RF-10.18); `Left`/
    /// `Right`/`Tab` trocam o foco entre os dois botões. Devolve a ação
    /// resolvida se o usuário confirmou -- `lib.rs` executa de verdade,
    /// porque o diálogo em si não guarda referência pra `Terminal`/`App`.
    fn handle_dialog_key(&mut self, event: &KeyEvent) -> Option<DialogAction> {
        if event.state != ElementState::Pressed {
            return None;
        }
        let Some(dialog) = &mut self.dialog else {
            return None;
        };
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.dialog = None;
                None
            }
            Key::Named(NamedKey::Enter) => {
                let focused = dialog.focused();
                let action = dialog.action;
                if focused == DialogButton::Confirm {
                    self.dialog = None;
                    Some(action)
                } else {
                    self.dialog = None;
                    None
                }
            }
            Key::Named(NamedKey::Tab | NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
                dialog.toggle_focus();
                None
            }
            _ => None,
        }
    }

    /// Navegação por setas, `Enter` aciona, `Esc` fecha (espec §2.16).
    /// Devolve a ação selecionada quando `Enter` aciona um item habilitado.
    fn handle_context_menu_key(&mut self, event: &KeyEvent) -> Option<MenuAction> {
        if event.state != ElementState::Pressed {
            return None;
        }
        let Some(menu) = &mut self.context_menu else {
            return None;
        };
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.context_menu = None;
                None
            }
            Key::Named(NamedKey::ArrowUp) => {
                menu.move_highlight(-1);
                None
            }
            Key::Named(NamedKey::ArrowDown) => {
                menu.move_highlight(1);
                None
            }
            Key::Named(NamedKey::Enter) => {
                let action = menu.selected();
                self.context_menu = None;
                Some(action)
            }
            _ => None,
        }
    }

    /// Passo 2 do ADR-0008 para as ações de aba/janela -- defaults fixos
    /// de Windows/Linux (não há parser de `[keybindings]` até a F4,
    /// docs/reference/acoes.md). `window.new`/`window.close` (ADR-0015)
    /// devolvem `ActionOutcome` distinto porque tocam outras janelas, algo
    /// que só `App` resolve.
    fn handle_tab_action_key(
        &mut self,
        event: &KeyEvent,
        gpu: &mut GpuContext,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        startup_directory: &Option<PathBuf>,
        now: Instant,
    ) -> ActionOutcome {
        if event.state != ElementState::Pressed {
            return ActionOutcome::Unhandled;
        }
        let m = self.modifiers;
        if m.ctrl && m.shift && !m.alt {
            match &event.logical_key {
                Key::Character(s) if s.eq_ignore_ascii_case("t") => {
                    self.action_new_tab(cell_metrics, proxy, startup_directory, now);
                    return ActionOutcome::Handled;
                }
                Key::Character(s) if s.eq_ignore_ascii_case("w") => {
                    if let Some(dialog) = self.action_close_tab() {
                        self.dialog = Some(dialog);
                    }
                    return ActionOutcome::Handled;
                }
                Key::Character(s) if s.eq_ignore_ascii_case("r") => {
                    self.action_rename_start();
                    return ActionOutcome::Handled;
                }
                // ADR-0015: `window.new`/`window.close`.
                Key::Character(s) if s.eq_ignore_ascii_case("n") => {
                    return ActionOutcome::OpenWindow;
                }
                Key::Character(s) if s.eq_ignore_ascii_case("q") => {
                    return ActionOutcome::CloseWindowRequested;
                }
                // `Ctrl+Shift+Tab`: única exceção ao padrão `Ctrl+Shift`
                // (ADR-0008), convenção universal de troca de aba.
                Key::Named(NamedKey::Tab) => {
                    self.action_prev_tab(gpu);
                    return ActionOutcome::Handled;
                }
                // RF-1.17, `docs/config/porecatu.example.toml`
                // `[keybindings]`: "ctrl+shift+left" = "tab.move_left".
                Key::Named(NamedKey::ArrowLeft) => {
                    self.action_move_tab(-1, gpu);
                    return ActionOutcome::Handled;
                }
                Key::Named(NamedKey::ArrowRight) => {
                    self.action_move_tab(1, gpu);
                    return ActionOutcome::Handled;
                }
                _ => {}
            }
        }
        if m.ctrl && !m.shift && !m.alt && matches!(event.logical_key, Key::Named(NamedKey::Tab)) {
            self.action_next_tab(gpu);
            return ActionOutcome::Handled;
        }
        if m.alt
            && !m.ctrl
            && !m.shift
            && let Key::Character(s) = &event.logical_key
            && let Some(digit) = s.chars().next().and_then(|c| c.to_digit(10))
            && (1..=9).contains(&digit)
        {
            self.action_goto((digit - 1) as usize, gpu);
            return ActionOutcome::Handled;
        }
        ActionOutcome::Unhandled
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
    /// `right_click` abre o menu de contexto (RF-1.1, RF-1.2, RF-2.20) em
    /// vez de ativar/arrastar. Devolve `true` só quando o botão de nova
    /// aba foi clicado -- `open_tab` precisa de `cell_metrics`/`proxy`/
    /// `startup_directory`, que são de `App`, não de `WindowState`, então
    /// quem chama (`App::dispatch_mouse_input`) é que abre a aba de
    /// verdade.
    fn handle_bar_click(
        &mut self,
        logical_point: (f32, f32),
        gpu: &mut GpuContext,
        right_click: bool,
    ) -> bool {
        let style = TabBarStyle::DEFAULT;
        let bar_width = self.logical_width;
        let h = bar_height();
        let layout = tab_bar::fit_width(&self.workspace, &style, bar_width, gpu.text_measurer());
        let overflow = tab_bar::overflow_state(&layout, bar_width, self.scroll_offset);
        self.scroll_offset = overflow.scroll_offset;

        if !right_click {
            if overflow.hidden_left > 0
                && tab_bar::point_in_overflow_pill(OverflowSide::Left, bar_width, h, logical_point)
            {
                self.scroll_offset = (self.scroll_offset - tab_bar::OVERFLOW_SCROLL_STEP).max(0.0);
                return false;
            }
            if overflow.hidden_right > 0
                && tab_bar::point_in_overflow_pill(OverflowSide::Right, bar_width, h, logical_point)
            {
                self.scroll_offset += tab_bar::OVERFLOW_SCROLL_STEP;
                return false;
            }
        }

        let content_point = (logical_point.0 + self.scroll_offset, logical_point.1);
        let Some(hit) = tab_bar::hit_test(&layout, content_point) else {
            return false;
        };

        if right_click {
            if let TabBarHit::Tab(id) | TabBarHit::CloseButton(id) = hit {
                self.context_menu = Some(ContextMenu::new(id, logical_point));
                self.hover.dismiss();
            }
            return false;
        }

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
                self.ensure_active_tab_visible(gpu);
                if let Some(rect) = tab_bar::tab_rect(&layout, id) {
                    let screen_x = rect.x - self.scroll_offset;
                    self.drag = TabDrag::Pressed {
                        tab: id,
                        start: logical_point,
                        grab_offset: logical_point.0 - screen_x,
                    };
                }
                false
            }
            TabBarHit::CloseButton(id) => {
                if let Some(dialog) = self.close_tab_via_button(id) {
                    self.dialog = Some(dialog);
                }
                false
            }
            TabBarHit::NewTabButton => true,
        }
    }

    /// RF-1.6 aplicado ao botão de fechar de uma aba que não é
    /// necessariamente a ativa (o clique pode ser em qualquer aba da
    /// trilha) -- mesma condição de `action_close_tab`, mas sobre `id`
    /// explícito.
    fn close_tab_via_button(&mut self, id: TabId) -> Option<ConfirmDialog> {
        let runtime = self.tabs.get(&id)?;
        let modes = runtime.terminal.modes();
        if modes.alt_screen || modes.mouse_reporting != MouseReporting::None {
            let title = self
                .workspace
                .tab(id)
                .map(|t| t.title().to_string())
                .unwrap_or_default();
            return Some(ConfirmDialog::new(
                "Fechar aba?",
                format!("\"{title}\" tem um programa em primeiro plano. Fechar mesmo assim?"),
                "Fechar aba",
                DialogAction::CloseTab(id),
            ));
        }
        self.close_tab_unconditionally(id);
        None
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
        self.window.set_cursor(CursorIcon::Default);
    }

    /// `y` físico está dentro da faixa da barra de abas (topo da janela).
    fn in_bar(&self, physical_y: f64) -> bool {
        physical_y < (bar_height() * self.scale) as f64
    }

    /// Recalcula linhas/colunas a partir do tamanho lógico e propaga pra
    /// `WindowSurface` e pro terminal da aba ativa (motor + PTY).
    /// `WindowSurface` é quem converte de volta para físico (ADR-0018).
    fn resize_to(&mut self, width: u32, height: u32, gpu: &GpuContext, cell_metrics: CellMetrics) {
        self.window_surface.resize(gpu, width, height, self.scale);
        self.logical_width = width as f32 / self.scale;
        self.logical_height = height as f32 / self.scale;
        let (rows, cols) = self.grid_size(cell_metrics);
        if let Some(runtime) = self.active_runtime() {
            runtime.terminal.resize(rows, cols);
        }
        self.window.request_redraw();
    }

    fn cell_at_cursor(&self, cell_metrics: CellMetrics) -> input::CellPosition {
        let content_y = self.cursor_position.1 - (bar_height() * self.scale) as f64;
        let (rows, cols) = self.active_runtime().map_or((MIN_GRID, MIN_GRID), |rt| {
            (
                rt.snapshot.rows.max(MIN_GRID),
                rt.snapshot.cols.max(MIN_GRID),
            )
        });
        input::cell_at(
            self.cursor_position.0,
            content_y.max(0.0),
            cell_metrics,
            rows,
            cols,
        )
    }

    /// Atualiza o hover da barra (ADR-0019) a partir da posição corrente do
    /// cursor -- só considera abas com rótulo truncado (`label_truncated`);
    /// as demais não têm nada a revelar. Também atualiza `warnings.hovered`
    /// (pausa do temporizador da informação, espec §2.14): os dois vivem
    /// no mesmo evento de `CursorMoved` porque os dois dependem de "onde
    /// está o cursor agora".
    fn update_hover(&mut self, gpu: &mut GpuContext, now: Instant) {
        let bar_point = (
            self.cursor_position.0 as f32 / self.scale,
            self.cursor_position.1 as f32 / self.scale,
        );
        let target = if self.in_bar(self.cursor_position.1)
            && self.dialog.is_none()
            && self.context_menu.is_none()
            && matches!(self.drag, TabDrag::Idle)
        {
            let style = TabBarStyle::DEFAULT;
            let layout = tab_bar::fit_width(
                &self.workspace,
                &style,
                self.logical_width,
                gpu.text_measurer(),
            );
            let content_point = (bar_point.0 + self.scroll_offset, bar_point.1);
            layout
                .groups
                .into_iter()
                .flat_map(|g| g.tabs)
                .find(|t| t.label_truncated && tab_bar_rect_contains(t.rect, content_point))
                .and_then(|t| {
                    let title = self.workspace.tab(t.id)?.title().to_string();
                    let screen_rect = Rect {
                        x: t.rect.x - self.scroll_offset,
                        ..t.rect
                    };
                    Some((t.id, screen_rect, title))
                })
        } else {
            None
        };
        self.hover.update(target, now);

        let warning_layout =
            overlay::layout_warnings(&self.warnings, bar_height(), self.logical_width);
        let over_warnings = tab_bar::rect_contains(warning_layout.stack_rect, bar_point);
        self.warnings.set_hovered(over_warnings, now);
    }

    fn tick(&mut self, now: Instant) {
        self.warnings.tick(now);
        self.hover.tick(now);
    }

    fn next_wake(&self) -> Option<Instant> {
        [self.warnings.next_deadline(), self.hover.next_deadline()]
            .into_iter()
            .flatten()
            .min()
    }
}

fn tab_bar_rect_contains(rect: Rect, point: (f32, f32)) -> bool {
    tab_bar::rect_contains(rect, point)
}

struct App {
    gpu: Option<GpuContext>,
    proxy: EventLoopProxy<Wakeup>,
    /// `cwd` do processo do Porecatu no momento em que ele iniciou --
    /// fallback de `tab.new`/`window.new` quando a aba ativa ainda não tem
    /// `cwd` capturado por OSC 7 (ADR-0017 item 1). `None` só se
    /// `current_dir` falhar.
    startup_directory: Option<PathBuf>,
    /// Métricas de célula em pixels lógicos -- DPI-independentes por
    /// definição (só `WindowSurface` converte pra físico), então uma só
    /// medição serve todas as janelas (ADR-0015).
    cell_metrics: CellMetrics,
    windows: HashMap<WindowId, WindowState>,
}

impl App {
    fn new(proxy: EventLoopProxy<Wakeup>) -> Self {
        Self {
            gpu: None,
            proxy,
            startup_directory: std::env::current_dir().ok(),
            cell_metrics: CellMetrics {
                width: 1.0,
                height: 1.0,
            },
            windows: HashMap::new(),
        }
    }

    /// ADR-0015: cria uma janela nova com uma aba, herdando o `cwd` da aba
    /// ativa da janela de origem. Geometria de cascata: tamanho da janela
    /// que a criou, deslocada 30px físicos em X e Y, presa aos limites do
    /// monitor dessa janela -- decisão do roadmap (seção F2), já que
    /// nenhum ADR fixa isso. Sem janela de origem (nunca acontece aqui,
    /// sempre há ao menos uma pra disparar `window.new`), cairia no
    /// default da plataforma.
    ///
    /// Simplificação: `winit::monitor::MonitorHandle` só expõe os limites
    /// físicos do monitor inteiro, não a área útil descontada da barra de
    /// tarefas/dock (nenhuma API de plataforma para isso é exposta pelo
    /// `winit`) -- a cascata prende à tela inteira, não à área útil que o
    /// roadmap descreve.
    fn open_window(&mut self, event_loop: &ActiveEventLoop, origin: Option<WindowId>) {
        let origin_state = origin.and_then(|id| self.windows.get(&id));
        let mut attributes = Window::default_attributes().with_title("Porecatu");
        if let Some(origin_window) = origin_state.map(|s| &s.window)
            && let Ok(origin_position) = origin_window.outer_position()
        {
            let size = origin_window.inner_size();
            let mut position = winit::dpi::PhysicalPosition::new(
                origin_position.x + NEW_WINDOW_CASCADE_PX,
                origin_position.y + NEW_WINDOW_CASCADE_PX,
            );
            if let Some(monitor) = origin_window.current_monitor() {
                let mon_pos = monitor.position();
                let mon_size = monitor.size();
                let max_x = mon_pos.x + mon_size.width as i32 - size.width as i32;
                let max_y = mon_pos.y + mon_size.height as i32 - size.height as i32;
                position.x = position.x.clamp(mon_pos.x, max_x.max(mon_pos.x));
                position.y = position.y.clamp(mon_pos.y, max_y.max(mon_pos.y));
            }
            attributes = attributes.with_inner_size(size).with_position(position);
        }

        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("falha ao criar janela"),
        );
        window.set_ime_allowed(true);

        let size = window.inner_size();
        let scale = window.scale_factor() as f32;

        let window_surface = if let Some(gpu) = &mut self.gpu {
            match gpu.create_window_surface(Arc::clone(&window), size.width, size.height) {
                Ok(surface) => surface,
                Err(err) => {
                    // Sem `WindowState` ainda pra guardar um aviso -- é a
                    // própria criação da superfície que falhou. `stderr` é
                    // o canal disponível neste caso extremo (GPU
                    // incompatível numa segunda tela, ADR-0018).
                    eprintln!("porecatu: falha ao criar surface da janela nova: {err}");
                    return;
                }
            }
        } else {
            let (gpu, window_surface) =
                GpuContext::new(Arc::clone(&window), size.width, size.height);
            self.gpu = Some(gpu);
            window_surface
        };

        let mut window_surface = window_surface;
        window_surface.resize(
            self.gpu.as_ref().expect("acabou de criar"),
            size.width,
            size.height,
            scale,
        );

        if let Some(gpu) = &mut self.gpu {
            let (cell_width, cell_height) = gpu
                .text_measurer()
                .measure_mono_cell(FONT_SIZE_PX, FONT_SIZE_PX * LINE_HEIGHT_MULTIPLIER);
            self.cell_metrics = CellMetrics {
                width: cell_width,
                height: cell_height,
            };
        }

        // ADR-0015: "a janela nova abre com uma aba no cwd da aba ativa no
        // momento da criação" -- da janela de ORIGEM, já que a nova ainda
        // não tem nenhuma aba.
        let cwd = origin_state
            .and_then(|s| s.resolve_new_tab_cwd(&self.startup_directory))
            .or_else(|| self.startup_directory.clone());

        let window_id = window.id();
        let mut state = WindowState::new(window, window_surface, scale);
        state.open_tab(self.cell_metrics, &self.proxy, cwd, Instant::now());
        self.windows.insert(window_id, state);
    }

    /// Fecha uma janela sem perguntar: sinaliza todas as abas primeiro,
    /// espera depois -- custo de uma volta de sinalização, não N × timeout
    /// (ADR-0017 item 4). Remove a janela do mapa; se não sobrou nenhuma,
    /// encerra o event loop (RF-1.4: fechar a última janela encerra o
    /// app).
    fn close_window_unconditionally(&mut self, window_id: WindowId, event_loop: &ActiveEventLoop) {
        let Some(state) = self.windows.remove(&window_id) else {
            return;
        };
        let waits: Vec<_> = state
            .tabs
            .into_values()
            .map(|runtime| runtime.terminal.close())
            .collect();
        for wait in waits {
            wait.wait();
        }
        if self.windows.is_empty() {
            // `app.quit`/RF-3.4 (gravação síncrona da sessão): no-op
            // documentado até a F5, que é quando `porecatu-session` passa
            // a existir (ADR-0017, docs/reference/acoes.md).
            event_loop.exit();
        }
    }

    /// RF-10.23 (ADR-0015): fechar janela com mais de uma aba pede
    /// confirmação. Com uma aba só (ou nenhuma), fecha direto.
    fn request_close_window(&mut self, window_id: WindowId, event_loop: &ActiveEventLoop) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        if state
            .workspace
            .groups()
            .iter()
            .map(|g| g.tabs().len())
            .sum::<usize>()
            > 1
        {
            state.dialog = Some(ConfirmDialog::new(
                "Fechar janela?",
                "Esta janela tem mais de uma aba aberta.",
                "Fechar janela",
                DialogAction::CloseWindow,
            ));
            state.window.request_redraw();
            return;
        }
        self.close_window_unconditionally(window_id, event_loop);
    }

    /// Executa a ação de um diálogo confirmado (`Enter` no botão de
    /// confirmar, ou clique nele) -- resolvido aqui, não em `WindowState`,
    /// porque `CloseWindow` precisa remover a janela do mapa de `App`.
    fn run_dialog_action(
        &mut self,
        window_id: WindowId,
        action: DialogAction,
        event_loop: &ActiveEventLoop,
    ) {
        match action {
            DialogAction::CloseTab(id) => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    let empty = state.close_tab_unconditionally(id);
                    if empty {
                        self.close_window_unconditionally(window_id, event_loop);
                    } else {
                        state.window.request_redraw();
                    }
                }
            }
            DialogAction::CloseWindow => {
                self.close_window_unconditionally(window_id, event_loop);
            }
        }
    }

    /// Executa um item do menu de contexto de aba (RF-1.1, RF-1.2, RF-2.20
    /// -- este último sempre desabilitado em F2, `MenuAction::MoveToGroup`
    /// não tem braço porque `ContextMenu::selected` nunca devolve um item
    /// desabilitado).
    fn run_menu_action(&mut self, window_id: WindowId, tab: TabId, action: MenuAction) {
        match action {
            MenuAction::NewTab => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.action_new_tab(
                        self.cell_metrics,
                        &self.proxy,
                        &self.startup_directory,
                        Instant::now(),
                    );
                    state.window.request_redraw();
                }
            }
            MenuAction::CloseTab => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    if let Some(dialog) = state.close_tab_via_button(tab) {
                        state.dialog = Some(dialog);
                    }
                    state.window.request_redraw();
                }
            }
            MenuAction::MoveToGroup => {}
        }
    }

    /// Agenda o próximo despertar via `ControlFlow::WaitUntil` -- o
    /// temporizador da informação (ADR-0014) e o atraso do tooltip
    /// (ADR-0019) marcam sujeira, não rodam loop nenhum: quando não há
    /// nada pendente em nenhuma janela, o event loop dorme de verdade
    /// (`ControlFlow::Wait`).
    fn schedule_next_wake(&self, event_loop: &ActiveEventLoop) {
        let next = self.windows.values().filter_map(|w| w.next_wake()).min();
        match next {
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    /// Roda o `tick` (expira avisos, promove tooltip pendente) em todas as
    /// janelas e redesenha as que mudaram.
    fn tick_all(&mut self) {
        let now = Instant::now();
        for state in self.windows.values_mut() {
            let had_warnings = !state.warnings.is_empty();
            let had_tooltip = state.hover.visible().is_some();
            state.tick(now);
            if had_warnings != !state.warnings.is_empty()
                || had_tooltip != state.hover.visible().is_some()
            {
                state.window.request_redraw();
            }
        }
    }
}

impl ApplicationHandler<Wakeup> for App {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            self.tick_all();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        self.open_window(event_loop, None);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Wakeup) {
        let tab_id = event.tab;
        let Some(state) = self.windows.get_mut(&event.window) else {
            return;
        };

        // Aba suja que não é a visível: só marca o indicador de atividade
        // (RF-1.20) -- sem redraw, ela não está na tela (ADR-0007 ponto 2).
        if state.workspace.active_tab() != Some(tab_id)
            && let Some(tab) = state.workspace.tab_mut(tab_id)
            && !tab.is_exited()
        {
            tab.mark_activity();
        }

        let mut pending = Vec::new();
        if let Some(runtime) = state.tabs.get(&tab_id) {
            while let Some(term_event) = runtime.terminal.try_recv_event() {
                pending.push(term_event);
            }
        }

        let mut window_should_close = false;
        for term_event in pending {
            match term_event {
                TermEvent::Title(title) => {
                    if let Some(tab) = state.workspace.tab_mut(tab_id) {
                        tab.set_process_title(title);
                    }
                }
                // RF-1.21 (indicador de campainha): só sinaliza em segundo
                // plano -- em primeiro plano o usuário já está vendo.
                TermEvent::Bell => {
                    if state.workspace.active_tab() != Some(tab_id)
                        && let Some(tab) = state.workspace.tab_mut(tab_id)
                    {
                        tab.mark_bell();
                    }
                }
                TermEvent::ClipboardWrite(text) => clipboard::copy(&text),
                TermEvent::ClipboardRead(responder) => {
                    if let Some(runtime) = state.tabs.get(&tab_id) {
                        let content = clipboard::paste().unwrap_or_default();
                        runtime
                            .terminal
                            .write(responder.respond(&content).into_bytes());
                    }
                }
                // Depende de tema resolvido -- F4.
                TermEvent::ColorQuery(_) => {}
                TermEvent::Cwd(cwd) => {
                    if let Some(tab) = state.workspace.tab_mut(tab_id) {
                        tab.set_cwd(cwd);
                    }
                }
                TermEvent::Exit { success, code } => {
                    if success {
                        // RF-1.3: código zero não deixa rastro -- a aba
                        // fecha (mesmo caminho de `tab.close`).
                        if state.close_tab_unconditionally(tab_id) {
                            window_should_close = true;
                        }
                    } else {
                        if let Some(runtime) = state.tabs.get(&tab_id) {
                            runtime.terminal.inject_note(
                                &format!("processo encerrado (código {code})"),
                                palette::NOTE_ACCENT_RGB,
                            );
                        }
                        if let Some(tab) = state.workspace.tab_mut(tab_id) {
                            tab.mark_exited(code as i32);
                        }
                    }
                }
            }
        }

        if window_should_close {
            self.close_window_unconditionally(event.window, event_loop);
            return;
        }

        let Some(state) = self.windows.get(&event.window) else {
            return;
        };
        state.sync_window_title();
        // `request_redraw` coalesce chamadas repetidas antes do próximo
        // `RedrawRequested` num só evento -- é o que faz N wakeups de
        // saída rápida (ex.: `cargo build`) não virarem N frames
        // (ADR-0007 ponto 3).
        state.window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.request_close_window(window_id, event_loop);
            }
            WindowEvent::Resized(size) => {
                if let (Some(state), Some(gpu)) = (self.windows.get_mut(&window_id), &self.gpu) {
                    state.resize_to(size.width, size.height, gpu, self.cell_metrics);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.scale = scale_factor as f32;
                    let size = state.window.inner_size();
                    if let Some(gpu) = &self.gpu {
                        state.resize_to(size.width, size.height, gpu, self.cell_metrics);
                    }
                }
            }
            WindowEvent::Focused(false) => {
                // ADR-0019: tooltip some ao perder foco a janela. Menu de
                // contexto: mesma regra do ADR-0014 ("perda de foco
                // fecha").
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.hover.dismiss();
                    state.context_menu = None;
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.modifiers = input::modifiers_from(modifiers.state());
                }
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                self.dispatch_keyboard_input(window_id, key, event_loop);
            }
            // Composição de IME (CJK) e tecla morta já resolvida pelo SO
            // (ABNT2) -- passam direto pro terminal, sem consultar
            // keybind nenhum (ADR-0008).
            WindowEvent::Ime(Ime::Commit(text)) => {
                if let Some(state) = self.windows.get(&window_id)
                    && let Some(runtime) = state.active_runtime()
                {
                    runtime.terminal.write(text.into_bytes());
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.dispatch_mouse_wheel(window_id, delta);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.dispatch_cursor_moved(window_id, position);
            }
            WindowEvent::MouseInput {
                state: pressed,
                button,
                ..
            } => {
                self.dispatch_mouse_input(window_id, pressed, button, event_loop);
            }
            WindowEvent::RedrawRequested => {
                self.redraw(window_id);
            }
            _ => {}
        }
        self.schedule_next_wake(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.schedule_next_wake(event_loop);
    }
}

impl App {
    fn dispatch_keyboard_input(
        &mut self,
        window_id: WindowId,
        key: KeyEvent,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };

        // Cadeia de captura (ADR-0008 passo 1): diálogo modal > menu de
        // contexto > cancelamento de arraste > rename > ações de aba/
        // janela > terminal. Cada nível consome a tecla por inteiro -- "um
        // binding que casa nunca cai para o terminal" vale igual pros
        // modos de captura.
        if state.dialog.is_some() {
            if let Some(action) = state.handle_dialog_key(&key) {
                self.run_dialog_action(window_id, action, event_loop);
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }
        if state.context_menu.is_some() {
            let tab = state.context_menu.map(|m| m.tab);
            if let Some(action) = state.handle_context_menu_key(&key)
                && let Some(tab) = tab
            {
                self.run_menu_action(window_id, tab, action);
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }
        if matches!(state.drag, TabDrag::Dragging { .. })
            && key.state == ElementState::Pressed
            && matches!(key.logical_key, Key::Named(NamedKey::Escape))
        {
            state.drag = TabDrag::Idle;
            state.window.set_cursor(CursorIcon::Default);
            state.window.request_redraw();
            return;
        }
        if state.rename.editing_tab().is_some() {
            state.hover.dismiss();
            state.handle_rename_key(&key);
            state.window.request_redraw();
            return;
        }
        // RF-10.16: `Esc` dispensa o aviso do topo, antes de qualquer
        // outra coisa -- só quando há um aviso pra dispensar; sem isso
        // `Esc` segue pro terminal como sempre.
        if !state.warnings.is_empty()
            && key.state == ElementState::Pressed
            && matches!(key.logical_key, Key::Named(NamedKey::Escape))
        {
            state.warnings.dismiss_top();
            state.window.request_redraw();
            return;
        }

        state.hover.dismiss();
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let outcome = state.handle_tab_action_key(
            &key,
            gpu,
            self.cell_metrics,
            &self.proxy,
            &self.startup_directory,
            Instant::now(),
        );
        match outcome {
            ActionOutcome::Handled => {
                if let Some(state) = self.windows.get(&window_id) {
                    state.window.request_redraw();
                }
            }
            ActionOutcome::OpenWindow => self.open_window(event_loop, Some(window_id)),
            ActionOutcome::CloseWindowRequested => self.request_close_window(window_id, event_loop),
            ActionOutcome::Unhandled => {
                if let Some(runtime) = state.active_runtime() {
                    // Modos lidos agora, não do snapshot do último frame
                    // (que pode estar obsoleto -- ex.: o programa acabou
                    // de ligar DECCKM e ainda não houve redraw).
                    input::handle_keyboard_input(
                        &runtime.terminal,
                        &runtime.terminal.modes(),
                        &key,
                        state.modifiers,
                    );
                }
            }
        }
    }

    fn dispatch_mouse_wheel(&mut self, window_id: WindowId, delta: MouseScrollDelta) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        if state.dialog.is_some() || state.context_menu.is_some() {
            return;
        }
        if state.in_bar(state.cursor_position.1) {
            // Espec §2.18: "roda do mouse sobre a barra rola a trilha na
            // horizontal, com ou sem Shift... passo de 90px por notch".
            let notches = match delta {
                MouseScrollDelta::LineDelta(_, y) => y,
                MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as f32,
            };
            if notches != 0.0 {
                state.scroll_offset -= notches.signum() * tab_bar::OVERFLOW_SCROLL_STEP;
                state.scroll_offset = state.scroll_offset.max(0.0);
                state.window.request_redraw();
            }
        } else if let Some(runtime) = state.active_runtime() {
            let cell = state.cell_at_cursor(self.cell_metrics);
            input::handle_mouse_wheel(
                &runtime.terminal,
                &runtime.terminal.modes(),
                delta,
                state.modifiers,
                cell,
            );
        }
    }

    fn dispatch_cursor_moved(
        &mut self,
        window_id: WindowId,
        position: winit::dpi::PhysicalPosition<f64>,
    ) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        state.cursor_position = (position.x, position.y);

        // Espec §2.16: "hover e foco por teclado são o mesmo estado
        // visual, mutuamente exclusivos" -- mover o mouse sobre o menu
        // move o realce. Menu aberto não faz mais nada com o movimento do
        // mouse (sem hover de barra, sem seleção de terminal).
        if let Some(menu) = &mut state.context_menu {
            let logical_point = (
                position.x as f32 / state.scale,
                position.y as f32 / state.scale,
            );
            let layout =
                overlay::layout_context_menu(menu, state.logical_width, state.logical_height);
            if let Some(index) = overlay::context_menu_hit(&layout, logical_point) {
                menu.set_highlight(index);
            }
            state.window.request_redraw();
            return;
        }

        let handled_by_drag = match &mut state.drag {
            TabDrag::Pressed {
                tab,
                start,
                grab_offset,
            } => {
                let logical = (
                    position.x as f32 / state.scale,
                    position.y as f32 / state.scale,
                );
                let past_threshold = (logical.0 - start.0).abs() > DRAG_THRESHOLD_PX
                    || (logical.1 - start.1).abs() > DRAG_THRESHOLD_PX;
                if past_threshold {
                    let dragging = TabDrag::Dragging {
                        tab: *tab,
                        grab_offset: *grab_offset,
                        preview_index: 0,
                    };
                    state.drag = dragging;
                    state.window.set_cursor(CursorIcon::Grabbing);
                    state.hover.dismiss();
                }
                true
            }
            TabDrag::Dragging { .. } => {
                let logical_x = position.x as f32 / state.scale;
                if logical_x < DRAG_EDGE_ZONE_PX {
                    state.scroll_offset = (state.scroll_offset - DRAG_AUTOSCROLL_STEP_PX).max(0.0);
                } else if logical_x > state.logical_width - DRAG_EDGE_ZONE_PX {
                    state.scroll_offset += DRAG_AUTOSCROLL_STEP_PX;
                }
                true
            }
            TabDrag::Idle => false,
        };

        if handled_by_drag {
            state.window.request_redraw();
            return;
        }

        if let Some(gpu) = &mut self.gpu {
            state.update_hover(gpu, Instant::now());
        }

        if state.dialog.is_some() || state.context_menu.is_some() {
            return;
        }

        if !state.in_bar(position.y)
            && let Some(runtime) = state.active_runtime()
        {
            let cell = state.cell_at_cursor(self.cell_metrics);
            input::handle_mouse_motion(
                &runtime.terminal,
                &runtime.terminal.modes(),
                cell,
                state.modifiers,
                state.mouse_button_down,
            );
            // Seleção mudou de forma sem passar pela thread de leitura --
            // ninguém mais vai pedir redraw sozinho.
            if state.mouse_button_down.is_some() {
                state.window.request_redraw();
            }
        }
        state.window.request_redraw();
    }

    fn dispatch_mouse_input(
        &mut self,
        window_id: WindowId,
        element_state: ElementState,
        button: MouseButton,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let pressed = element_state == ElementState::Pressed;

        if !pressed {
            if button == MouseButton::Left && !matches!(state.drag, TabDrag::Idle) {
                state.finish_drag();
            } else if !state.in_bar(state.cursor_position.1) {
                state.mouse_button_down = None;
            }
            state.window.request_redraw();
            return;
        }

        let logical_point = (
            state.cursor_position.0 as f32 / state.scale,
            state.cursor_position.1 as f32 / state.scale,
        );

        // Diálogo modal captura todo clique enquanto aberto.
        if let Some(layout) = state.dialog.as_ref().and_then(|dialog| {
            let gpu = self.gpu.as_mut()?;
            Some(overlay::layout_dialog(
                state.logical_width,
                state.logical_height,
                dialog,
                gpu.text_measurer(),
            ))
        }) {
            if button == MouseButton::Left {
                if let Some(clicked) = overlay::dialog_hit(&layout, logical_point) {
                    if clicked == DialogButton::Confirm {
                        let action = state.dialog.as_ref().map(|d| d.action);
                        state.dialog = None;
                        if let Some(action) = action {
                            self.run_dialog_action(window_id, action, event_loop);
                        }
                    } else {
                        state.dialog = None;
                    }
                } else {
                    state.dialog = None;
                }
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }

        // Menu de contexto: clique em item aciona, qualquer outro fecha.
        if let Some(menu) = state.context_menu {
            let layout =
                overlay::layout_context_menu(&menu, state.logical_width, state.logical_height);
            let hit = overlay::context_menu_hit(&layout, logical_point);
            state.context_menu = None;
            if button == MouseButton::Left
                && let Some(index) = hit
                && context_menu::TAB_MENU_ITEMS[index].enabled
            {
                self.run_menu_action(
                    window_id,
                    menu.tab,
                    context_menu::TAB_MENU_ITEMS[index].action,
                );
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }

        // Aviso: botão de fechar dispensa; corpo não faz nada além de
        // consumir o clique (não deixa passar pro que está atrás).
        let warning_layout =
            overlay::layout_warnings(&state.warnings, bar_height(), state.logical_width);
        if let Some(hit) = overlay::hit_test_warnings(&warning_layout, logical_point) {
            if let overlay::WarningHit::Close(index) = hit {
                state.warnings.dismiss(index);
            }
            state.window.request_redraw();
            return;
        }

        if button == MouseButton::Right && state.in_bar(state.cursor_position.1) {
            state.hover.dismiss();
            if let Some(gpu) = &mut self.gpu {
                state.handle_bar_click(logical_point, gpu, true);
            }
            state.window.request_redraw();
            return;
        }

        if button == MouseButton::Left && state.in_bar(state.cursor_position.1) {
            state.hover.dismiss();
            let Some(gpu) = &mut self.gpu else {
                return;
            };
            let new_tab_requested = state.handle_bar_click(logical_point, gpu, false);
            if new_tab_requested {
                state.action_new_tab(
                    self.cell_metrics,
                    &self.proxy,
                    &self.startup_directory,
                    Instant::now(),
                );
            }
            state.window.request_redraw();
            return;
        }

        state.mouse_button_down = Some(button);
        if !state.in_bar(state.cursor_position.1) {
            let cell = state.cell_at_cursor(self.cell_metrics);
            let active_id = state.workspace.active_tab();
            if let Some(runtime) = active_id.and_then(|id| state.tabs.get(&id)) {
                input::handle_mouse_button(
                    &runtime.terminal,
                    &runtime.terminal.modes(),
                    button,
                    pressed,
                    cell,
                    state.modifiers,
                    &mut state.click_tracker,
                );
            }
        }
        state.window.request_redraw();
    }

    fn redraw(&mut self, window_id: WindowId) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let mut frame = Frame::new();

        let style = TabBarStyle::DEFAULT;
        let bar_width = state.logical_width;
        let base_layout =
            tab_bar::fit_width(&state.workspace, &style, bar_width, gpu.text_measurer());
        let overflow = tab_bar::overflow_state(&base_layout, bar_width, state.scroll_offset);
        state.scroll_offset = overflow.scroll_offset;

        // Durante um arraste, o `Workspace` de verdade não é tocado -- só
        // um clone com o preview de reordenação aplicado é usado pra
        // desenhar (espec §2.19: as vizinhas "deslizam" mostrando onde a
        // aba cairia; o `Workspace` real só recebe a troca ao soltar).
        let mut drag_ghost = None;
        let paint_layout = if let TabDrag::Dragging {
            tab, grab_offset, ..
        } = state.drag
        {
            let cursor_logical_x = state.cursor_position.0 as f32 / state.scale;
            let ghost_screen_x = cursor_logical_x - grab_offset;
            let ghost_content_x = ghost_screen_x + state.scroll_offset;
            let width = tab_bar::tab_rect(&base_layout, tab)
                .map(|r| r.width)
                .unwrap_or(0.0);
            let ghost_center = ghost_content_x + width / 2.0;
            let preview_index = tab_bar::drag_target_index(&base_layout, tab, ghost_center);
            state.drag = TabDrag::Dragging {
                tab,
                grab_offset,
                preview_index,
            };
            drag_ghost = Some(DragGhost {
                tab,
                screen_x: ghost_screen_x,
            });

            let mut preview = state.workspace.clone();
            preview.move_tab(tab, preview_index);
            tab_bar::fit_width(&preview, &style, bar_width, gpu.text_measurer())
        } else {
            base_layout
        };

        let chrome_primitives = chrome::paint(
            &paint_layout,
            &state.workspace,
            state.workspace.active_tab(),
            &state.rename,
            &style,
            bar_width,
            overflow,
            drag_ghost,
            gpu.text_measurer(),
        );
        frame.set_layer(Layer::Chrome, chrome_primitives);

        let h = bar_height();
        if let Some(id) = state.workspace.active_tab()
            && let Some(runtime) = state.tabs.get_mut(&id)
        {
            runtime.terminal.snapshot_into(&mut runtime.snapshot);
            let primitives =
                paint::build_primitives(&runtime.snapshot, self.cell_metrics, FONT_SIZE_PX, h);
            frame.set_layer(Layer::Grid, primitives);
        }

        if !state.warnings.is_empty() {
            let warning_layout = overlay::layout_warnings(&state.warnings, h, bar_width);
            let primitives =
                overlay::paint_warnings(&warning_layout, &state.warnings, gpu.text_measurer());
            frame.set_layer(Layer::Warning, primitives);
        }

        let mut popover = Vec::new();
        if let Some((anchor, text)) = state.hover.visible() {
            popover.extend(overlay::paint_tooltip(
                anchor,
                text,
                state.logical_width,
                state.logical_height,
                gpu.text_measurer(),
            ));
        }
        if let Some(menu) = &state.context_menu {
            let layout =
                overlay::layout_context_menu(menu, state.logical_width, state.logical_height);
            popover.extend(overlay::paint_context_menu(&layout, menu));
        }
        if !popover.is_empty() {
            frame.set_layer(Layer::Popover, popover);
        }

        if let Some(dialog) = &state.dialog {
            let layout = overlay::layout_dialog(
                state.logical_width,
                state.logical_height,
                dialog,
                gpu.text_measurer(),
            );
            let primitives = overlay::paint_dialog(
                &layout,
                dialog,
                state.logical_width,
                state.logical_height,
                gpu.text_measurer(),
            );
            frame.set_layer(Layer::Modal, primitives);
        }

        state
            .window_surface
            .render(gpu, palette::TERM_BACKGROUND, &frame);
    }
}

/// Abre a janela principal do Porecatu e roda o event loop até todas as
/// janelas fecharem (ADR-0015).
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
