// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use porecatu_core::{GroupId, TabId, Workspace};
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

mod animation;
mod app_icon;
mod chrome;
mod clipboard;
mod context_menu;
mod dialog;
mod group_editor;
mod group_menu;
mod input;
mod move_to_group;
mod overlay;
mod paint;
mod palette;
mod rename;
mod selection;
mod tab_bar;
mod tooltip;
mod warning;

use animation::AnimationClock;
use chrome::{DragGhost, GroupDragGhost};
use context_menu::{ContextMenu, MenuAction};
use dialog::{ConfirmDialog, DialogAction, DialogButton};
use group_editor::{EditorRegion, GroupEditor};
use group_menu::{GroupAction, GroupContextMenu};
use input::ClickTracker;
use move_to_group::{MoveTarget, MoveToGroupPopover};
use paint::CellMetrics;
use porecatu_core::GroupColor;
use rename::RenameState;
use selection::Selection;
use tab_bar::{DragDrop, OverflowSide, TabBarHit, TabBarStyle};
use tooltip::Hover;
use warning::{Severity, WarningStack};

/// Janela de tempo entre o clique que colapsa/expande a pílula e um
/// segundo clique no mesmo grupo pra contar como duplo clique (RF-2.22:
/// "abrir o editor por duplo clique no rótulo"). Mesmo valor e mesma nota
/// de procedência de `input::MULTI_CLICK_THRESHOLD` -- convenção comum de
/// SO, não token de design.
const PILL_DOUBLE_CLICK_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(500);

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

/// ADR-0022, tabela de consumidores -- as únicas duas durações que o
/// relógio de animação usa.
const COLLAPSE_REFLOW_DURATION: std::time::Duration = std::time::Duration::from_millis(150);
const GROUP_CREATE_REFLOW_DURATION: std::time::Duration = std::time::Duration::from_millis(180);

/// Estado do arraste na barra (espec §2.19/§2.19.1, RF-1.15/RF-1.16/
/// RF-2.18/RF-2.19, F3 etapa 6). `TabPressed`/`GroupPressed` são o meio-
/// caminho entre o clique que já ativou a aba (ou vai decidir colapso/
/// duplo clique, no caso da pílula) e o arraste de verdade -- existem só
/// pra medir o limiar de 4px antes de comprometer.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    Idle,
    TabPressed {
        tab: TabId,
        start: (f32, f32),
        /// Deslocamento (em coordenadas de tela) entre o ponto do clique e
        /// o canto esquerdo da aba -- onde dentro da aba o usuário
        /// "segurou". Constante durante o arraste inteiro, mesmo que a
        /// trilha role (o fantasma segue o cursor, não o conteúdo).
        grab_offset: f32,
    },
    TabDragging {
        tab: TabId,
        grab_offset: f32,
        /// Alvo calculado no último redraw (`tab_bar::drag_target`,
        /// ADR-0021 §4) -- o que `lib.rs` aplica de verdade ao `Workspace`
        /// se o usuário soltar dentro da barra.
        target: DragDrop,
    },
    /// Arraste do rótulo do grupo (espec §2.19.1, RF-2.19): o fantasma é só
    /// a pílula, e o clique-e-espera nela ainda não decidiu entre colapso/
    /// duplo clique (`handle_pill_click`, resolvido em `finish_drag` se
    /// nunca virar `GroupDragging`).
    GroupPressed {
        group: GroupId,
        start: (f32, f32),
        grab_offset: f32,
    },
    GroupDragging {
        group: GroupId,
        grab_offset: f32,
        /// Mesma convenção de `tab_bar::group_drag_target_index`.
        preview_index: usize,
    },
}

// docs/config/porecatu.example.toml [terminal.font]: size = 14.0 (RF-5.3,
// 12.5 originalmente, +2px numa revisão, recalibrado a 13 -- pedido do
// usuário -- e por fim a 14 pelo motivo abaixo), line_height = 1.75
// (RF-5.6, "multiplicador das métricas naturais da fonte"). Simplificação
// desta etapa: aplicado direto sobre `size` em vez da métrica natural da
// fonte (ascent+descent+lineGap), que exigiria ler hhea/OS2 da face --
// ajustar quando isso importar na prática.
//
// Por que par: a Iosevka Fixed tem `unitsPerEm = 1000` e avanço 500 em
// **todo** glyph, ou seja avanço lógico = `size / 2`. Para
// `snap_cell_metrics_to_pixel_grid` ser no-op, `size / 2 * scale` tem de
// cair em pixel inteiro -- em escala 1.0 isso quer dizer `size` par. A 13
// o avanço era 6.5 e a célula arredondava para 7.0, deixando meio pixel de
// folga por célula; a 14 o glyph preenche a célula e a largura dela não
// muda (7.0 nos dois casos), então a contagem de colunas é a mesma.
//
// Isto é conforto, não correção: em 125% nenhum tamanho perto deste fecha,
// e `terminal.font.size` vira chave do usuário na F4. Quem garante que a
// grade continua sendo grade em qualquer tamanho e qualquer escala é o
// teste em em de `paint::fits_the_grid`, não esta constante.
const FONT_SIZE_PX: f32 = 14.0;
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

/// Arredonda a métrica de célula para que a origem de toda coluna
/// (`col as f32 * width`) caia num pixel **físico** inteiro depois da
/// conversão de `WindowSurface` (que multiplica por `scale`, ADR-0018).
///
/// Sem isto, `width`/`height` saem fracionários da medição em ponto
/// flutuante (`measure_mono_cell`) -- a Iosevka Fixed a 14.5px não bate
/// pixel inteiro -- e cada coluna começa numa fração de pixel física
/// diferente. O `glyphon`/`cosmic-text` cacheia glyph rasterizado por
/// posição subpixel; posições fracionárias distintas por coluna produzem
/// caixas delimitadoras que não se encostam perfeitamente, e a
/// antialiasing de cada glyph deixa uma costura de 1px entre caracteres --
/// visível em qualquer programa que desenhe grade cheia (`btop`, o
/// `claude` CLI). Arredondar a célula para o pixel físico garante que toda
/// coluna comece exatamente onde a anterior termina, sem resto.
pub(crate) fn snap_cell_metrics_to_pixel_grid(width: f32, height: f32, scale: f32) -> CellMetrics {
    let physical_width = (width * scale).round().max(1.0);
    let physical_height = (height * scale).round().max(1.0);
    CellMetrics {
        width: physical_width / scale,
        height: physical_height / scale,
    }
}

/// O que `WindowState::handle_tab_action_key` não pode resolver sozinho --
/// `window.new`/`window.close` (ADR-0015) tocam outras janelas, então
/// precisam voltar pra `App`. `WindowEmptied` é o mesmo caso: fechar a
/// última aba sem grupo nenhum e sem aba solta nenhuma sobrando fecha a
/// janela sozinha (pedido do usuário) -- `close_window_unconditionally`
/// só `App` tem.
enum ActionOutcome {
    Handled,
    OpenWindow,
    CloseWindowRequested,
    WindowEmptied,
    Unhandled,
}

/// Resultado de fechar uma aba por um caminho que pode ou não precisar de
/// confirmação (RF-1.6: tela alternativa ou reporte de mouse ligado pede
/// diálogo antes). `Closed { window_empty }` usa o mesmo contrato de
/// `WindowState::close_tab_unconditionally`: `true` quando não sobrou
/// grupo nenhum nem aba solta nenhuma, e quem chama fecha a janela.
enum TabCloseOutcome {
    Dialog(ConfirmDialog),
    Closed { window_empty: bool },
}

/// O que `WindowState::handle_group_editor_key` não pode resolver sozinho:
/// `Enter` na lista de ações pode disparar `group.new_tab` (precisa de
/// `cell_metrics`/`proxy`, que só `App` tem) -- as outras duas ações da
/// lista (`ToggleCollapse`/`Dissolve`) e as duas do campo/faixa
/// (`Rename`/`SetColor`) já se resolvem dentro do método.
enum GroupEditorOutcome {
    None,
    Action(GroupId, GroupAction),
}

/// O que `WindowState::handle_bar_click` não pode resolver sozinho: o
/// botão "+" de um grupo (pedido do usuário, fora da espec.) pede
/// `open_tab`, que precisa de `cell_metrics`/`proxy`/`startup_directory`
/// -- só `App` tem. `WindowEmptied`: mesmo caso de `ActionOutcome`, pelo
/// botão de fechar da aba em vez do atalho de teclado.
enum NewTabRequest {
    None,
    InGroup(GroupId),
    /// "+" ao fim da trilha, fora de qualquer wrapper.
    Ungrouped,
    WindowEmptied,
}

/// Onde uma aba nova nasce. As três origens querem coisas diferentes:
/// seguir a aba ativa levaria a aba nova para dentro do grupo dela, que é
/// certo para `tab.new` e errado para os dois botões da trilha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewTabTarget {
    /// `tab.new`/`window.new` (RF-1.1, ADR-0020 §1): o grupo da aba ativa.
    ActiveGroup,
    /// `group.new_tab` (RF-2.8/RF-2.22): fim de um grupo específico -- o
    /// "+" que fica dentro do wrapper do grupo.
    Group(GroupId),
    /// "+" ao fim da trilha: fim da barra, fora de qualquer grupo
    /// explícito.
    Ungrouped,
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
    drag: Drag,
    /// Relógio de animação (ADR-0022) -- reflui da trilha ao colapsar/
    /// expandir (`.15s`, `toggle_group_collapse`) e ao formar grupo
    /// (RF-2.5, `.18s`, `action_group_create`).
    animations: AnimationClock,
    /// Hover/tooltip do ADR-0019 -- RF-1.10, título truncado da aba.
    hover: Hover,
    /// Aviso do app (ADR-0014 canal 1, RF-10.15/RF-10.16).
    warnings: WarningStack,
    /// Diálogo de confirmação em andamento (ADR-0014, RF-10.18), no máximo
    /// um por janela -- "modal é por janela, não por app".
    dialog: Option<ConfirmDialog>,
    /// Menu de contexto de aba em andamento (ADR-0014 §2.16, RF-10.19).
    context_menu: Option<ContextMenu>,
    /// Menu de contexto de grupo (RF-2.22, ADR-0023) -- mutuamente
    /// exclusivo com `context_menu`/`group_editor`/`move_to_group`
    /// (`close_all_popovers`).
    group_context_menu: Option<GroupContextMenu>,
    /// Editor de grupo (ADR-0023), o quinto widget de chrome.
    group_editor: Option<GroupEditor>,
    /// Popover de destino do `tab.move_to_group` (RF-2.20, ADR-0023 §4).
    move_to_group: Option<MoveToGroupPopover>,
    /// Último clique numa pílula, pra detectar o duplo clique do RF-2.22 --
    /// mesmo padrão de `input::ClickTracker`, mas escopado à pílula (que
    /// não passa pelo roteamento de mouse do terminal).
    last_pill_click: Option<(GroupId, Instant)>,
    /// Seleção múltipla da barra (ADR-0021) -- efêmera como `rename`,
    /// `drag` e `hover`, ao lado dos quais fica; não persistida.
    selection: Selection,
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
            drag: Drag::Idle,
            animations: AnimationClock::default(),
            hover: Hover::default(),
            warnings: WarningStack::default(),
            dialog: None,
            context_menu: None,
            group_context_menu: None,
            group_editor: None,
            move_to_group: None,
            last_pill_click: None,
            selection: Selection::default(),
        }
    }

    /// Fecha os quatro widgets da camada popover (ADR-0023: "abrir
    /// qualquer um dos dois fecha o outro, num ponto só do código") --
    /// chamado antes de abrir qualquer um deles, garantindo que nunca
    /// coexistem.
    fn close_all_popovers(&mut self) {
        self.context_menu = None;
        self.group_context_menu = None;
        self.group_editor = None;
        self.move_to_group = None;
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

    /// `cwd` herdado por `group.new_tab` (RF-2.8): a última aba **do grupo
    /// alvo**, não a aba ativa da janela -- diferente de
    /// [`Self::resolve_new_tab_cwd`], porque `group.new_tab` sempre agrega
    /// em `group`, mesmo com a aba ativa em outro grupo (mesma regra de
    /// `open_tab`/`NewTabTarget::Group`). Pedido do usuário. Cai para a
    /// resolução de `tab.new` se o grupo não tiver aba com `cwd` conhecido.
    fn resolve_group_new_tab_cwd(
        &self,
        group: GroupId,
        startup_directory: &Option<PathBuf>,
    ) -> Option<PathBuf> {
        self.workspace
            .group(group)
            .and_then(|g| g.tabs().last())
            .and_then(|&id| self.workspace.tab(id))
            .and_then(|tab| tab.cwd().cloned())
            .or_else(|| self.resolve_new_tab_cwd(startup_directory))
    }

    /// Linhas/colunas atuais a partir do tamanho lógico da janela (descontada
    /// a barra de abas) e da métrica de célula já medida.
    fn grid_size(&self, cell_metrics: CellMetrics) -> (usize, usize) {
        let content =
            paint::terminal_content_rect(bar_height(), self.logical_width, self.logical_height);
        let cols = ((content.width / cell_metrics.width) as usize).max(MIN_GRID);
        let rows = ((content.height / cell_metrics.height) as usize).max(MIN_GRID);
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
        let trilha_width = tab_bar::trilha_width(&style, self.logical_width);
        let window_start = self.scroll_offset;
        let window_end = self.scroll_offset + trilha_width;
        if rect.x < window_start {
            self.scroll_offset = rect.x;
        } else if rect.x + rect.width > window_end {
            self.scroll_offset = rect.x + rect.width - trilha_width;
        }
    }

    /// Canto esquerdo, em coordenadas de tela, da pílula de `group` --
    /// posição horizontal do editor de grupo (espec. §2.10: "posicionado
    /// horizontalmente sobre o grupo que está sendo editado"). `0.0` se o
    /// grupo não existe mais (editor prestes a fechar) ou não tem pílula
    /// (grupo implícito -- inatingível aqui, o editor nunca abre sobre
    /// um).
    fn group_pill_screen_x(&self, group: GroupId, gpu: &mut GpuContext) -> f32 {
        let style = TabBarStyle::DEFAULT;
        let layout = tab_bar::fit_width(
            &self.workspace,
            &style,
            self.logical_width,
            gpu.text_measurer(),
        );
        layout
            .groups
            .iter()
            .find(|g| g.id == group)
            .and_then(|g| g.pill.as_ref())
            .map(|p| p.rect.x - self.scroll_offset)
            .unwrap_or(0.0)
    }

    fn active_runtime(&self) -> Option<&TabRuntime> {
        let id = self.workspace.active_tab()?;
        self.tabs.get(&id)
    }

    /// Cria uma aba, herdando `cwd` (RF-1.1, ADR-0017), e spawna a
    /// `Terminal` dela. Erro de spawn desfaz a criação e vira aviso do app
    /// (canal 1, ADR-0014) -- sem isso o `Workspace` acumularia abas sem
    /// `Terminal` nenhum atrás.
    fn open_tab(
        &mut self,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        cwd: Option<PathBuf>,
        now: Instant,
        target: NewTabTarget,
    ) {
        let (rows, cols) = self.grid_size(cell_metrics);
        let shell_name = Self::shell_display_name();
        let tab_id = match target {
            NewTabTarget::Group(id) => match self.workspace.group(id) {
                Some(g) => {
                    let pos = g.tabs().len();
                    self.workspace
                        .new_tab(Some(id), shell_name, cwd.clone(), pos)
                }
                // Grupo que sumiu entre o clique e aqui: cai no caminho
                // de `tab.new`, não num run implícito novo no fim.
                None => self.workspace.append_tab(shell_name, cwd.clone()),
            },
            NewTabTarget::ActiveGroup => self.workspace.append_tab(shell_name, cwd.clone()),
            NewTabTarget::Ungrouped => self.workspace.append_ungrouped_tab(shell_name, cwd.clone()),
        };

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
        // ADR-0021 §2: fechar aba selecionada tira-a da seleção; a ordem
        // precisa ser capturada antes de `workspace.close_tab` -- é nela que
        // "âncora mais próxima" faz sentido.
        let order: Vec<TabId> = self.workspace.visual_order().collect();
        self.selection.remove_tab(id, &order);
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
        self.open_tab(cell_metrics, proxy, cwd, now, NewTabTarget::ActiveGroup);
    }

    /// RF-1.6 (ADR-0017): fechar a aba ativa pede confirmação quando ela
    /// tem tela alternativa ou reporte de mouse ligado -- o proxy de
    /// "processo em primeiro plano" que o app pode observar sem varrer a
    /// árvore de processos. Sem isso, fecha direto -- e se isso esvaziou a
    /// janela (pedido do usuário), quem chama fecha a janela.
    fn action_close_tab(&mut self) -> Option<TabCloseOutcome> {
        let id = self.workspace.active_tab()?;
        let runtime = self.tabs.get(&id)?;
        let modes = runtime.terminal.modes();
        if modes.alt_screen || modes.mouse_reporting != MouseReporting::None {
            let title = self
                .workspace
                .tab(id)
                .map(|t| t.title().to_string())
                .unwrap_or_default();
            return Some(TabCloseOutcome::Dialog(ConfirmDialog::new(
                "Fechar aba?",
                format!("\"{title}\" tem um programa em primeiro plano. Fechar mesmo assim?"),
                "Fechar aba",
                DialogAction::CloseTab(id),
            )));
        }
        let window_empty = self.close_tab_unconditionally(id);
        Some(TabCloseOutcome::Closed { window_empty })
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

    /// `tab.goto_N`: índice sobre a ordem **navegável**, não a visual --
    /// aba de grupo colapsado sai da numeração, e colapsar renumera
    /// `Alt+1..9` (deliberado, ADR-0020 §2).
    fn action_goto(&mut self, navigable_index: usize, gpu: &mut GpuContext) {
        self.workspace.activate_navigable_index(navigable_index);
        self.sync_window_title();
        self.ensure_active_tab_visible(gpu);
    }

    /// RF-1.17: reordenação por teclado, uma posição por vez, dentro do
    /// próprio grupo (`Workspace::move_tab` nunca move entre grupos --
    /// isso é o arraste/`tab.move_to_group` da etapa 6). Por isso a
    /// posição-alvo vem da ordem *dentro do grupo* da aba ativa, não da
    /// ordem visual da janela inteira -- com mais de um grupo (F3) as duas
    /// divergem.
    fn action_move_tab(&mut self, delta: isize, gpu: &mut GpuContext) {
        let Some(active) = self.workspace.active_tab() else {
            return;
        };
        let Some(group_id) = self.workspace.group_of_tab(active) else {
            return;
        };
        let Some(group) = self.workspace.group(group_id) else {
            return;
        };
        let order = group.tabs();
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

    /// Espelha `handle_context_menu_key`, sobre `group_context_menu`.
    fn handle_group_menu_key(&mut self, event: &KeyEvent) -> Option<GroupAction> {
        if event.state != ElementState::Pressed {
            return None;
        }
        let Some(menu) = &mut self.group_context_menu else {
            return None;
        };
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.group_context_menu = None;
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
                self.group_context_menu = None;
                Some(action)
            }
            _ => None,
        }
    }

    /// Cadeia de teclado do editor de grupo (espec. §2.10, ADR-0023 §2):
    /// `Tab`/`Shift+Tab` trocam de região, setas movem o realce dentro da
    /// faixa/lista, `Enter` confirma o campo ou aciona o realçado, `Esc`
    /// fecha -- sem restaurar nada porque o modelo nunca mudou (nota do
    /// módulo `group_editor.rs`). `Rename`/`SetColor` nunca aparecem em
    /// `selected_action()` (`EDITOR_ACTION_ORDER` é o subconjunto de três
    /// -- ver `group_menu.rs`), então só a região `Actions` pode devolver
    /// `Action`; o campo e a faixa se resolvem aqui dentro.
    fn handle_group_editor_key(&mut self, event: &KeyEvent) -> GroupEditorOutcome {
        if event.state != ElementState::Pressed {
            return GroupEditorOutcome::None;
        }
        let shift = self.modifiers.shift;
        let Some(editor) = &mut self.group_editor else {
            return GroupEditorOutcome::None;
        };
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.group_editor = None;
                GroupEditorOutcome::None
            }
            Key::Named(NamedKey::Tab) => {
                editor.cycle_focus(!shift);
                GroupEditorOutcome::None
            }
            Key::Named(NamedKey::ArrowUp) => {
                editor.move_highlight(-1);
                GroupEditorOutcome::None
            }
            Key::Named(NamedKey::ArrowDown) => {
                editor.move_highlight(1);
                GroupEditorOutcome::None
            }
            Key::Named(NamedKey::Backspace) => {
                editor.backspace();
                GroupEditorOutcome::None
            }
            Key::Named(NamedKey::Enter) => match editor.focus() {
                EditorRegion::Name => {
                    let group = editor.group;
                    let name = editor.name_buffer().to_string();
                    self.workspace.rename_group(group, name);
                    self.group_editor = None;
                    GroupEditorOutcome::None
                }
                EditorRegion::Swatches => {
                    let group = editor.group;
                    let color = GroupColor::ALL[editor.swatch_highlight()];
                    self.workspace.set_group_color(group, color);
                    GroupEditorOutcome::None
                }
                EditorRegion::Actions => {
                    GroupEditorOutcome::Action(editor.group, editor.selected_action())
                }
            },
            _ => {
                if editor.focus() == EditorRegion::Name
                    && let Some(text) = &event.text
                {
                    for c in text.chars().filter(|c| !c.is_control()) {
                        editor.push_char(c);
                    }
                }
                GroupEditorOutcome::None
            }
        }
    }

    /// Espelha `handle_context_menu_key`, sobre `move_to_group`.
    fn handle_move_to_group_key(&mut self, event: &KeyEvent) -> Option<MoveTarget> {
        if event.state != ElementState::Pressed {
            return None;
        }
        let Some(popover) = &mut self.move_to_group else {
            return None;
        };
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.move_to_group = None;
                None
            }
            Key::Named(NamedKey::ArrowUp) => {
                popover.move_highlight(-1);
                None
            }
            Key::Named(NamedKey::ArrowDown) => {
                popover.move_highlight(1);
                None
            }
            Key::Named(NamedKey::Enter) => {
                let target = popover.selected();
                self.move_to_group = None;
                Some(target)
            }
            _ => None,
        }
    }

    /// Executa o destino escolhido no popover do RF-2.20.
    fn run_move_target(&mut self, tab: TabId, target: MoveTarget) {
        match target {
            MoveTarget::Group(group) => {
                self.workspace.move_tab_to_group(tab, group);
            }
            MoveTarget::NewGroup => {
                let color = self.workspace.next_auto_color();
                self.workspace.group_tabs(&[tab], "Novo grupo", color);
            }
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
                    return match self.action_close_tab() {
                        Some(TabCloseOutcome::Dialog(dialog)) => {
                            self.dialog = Some(dialog);
                            ActionOutcome::Handled
                        }
                        Some(TabCloseOutcome::Closed { window_empty: true }) => {
                            ActionOutcome::WindowEmptied
                        }
                        _ => ActionOutcome::Handled,
                    };
                }
                Key::Character(s) if s.eq_ignore_ascii_case("r") => {
                    self.action_rename_start();
                    return ActionOutcome::Handled;
                }
                // `docs/config/porecatu.example.toml` `[keybindings]`:
                // "ctrl+shift+g" = "group.create".
                Key::Character(s) if s.eq_ignore_ascii_case("g") => {
                    self.action_group_create(gpu);
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
    /// arma o possível arraste do RF-1.15 (`Drag::TabPressed`), resolvido
    /// de verdade só se o movimento passar do limiar em `CursorMoved`. Isso
    /// só vale para o clique **sem modificador**: `Ctrl`/`Cmd`+clique
    /// alterna a seleção e `Shift`+clique estende a partir da âncora
    /// (ADR-0021 §1/§2) -- nenhum dos dois ativa a aba nem arma arraste.
    /// `right_click` abre o menu de contexto (RF-1.1, RF-1.2, RF-2.20) em
    /// vez de ativar/arrastar. Devolve `NewTabRequest::None` na maioria dos
    /// casos -- só `Global` (botão de nova aba da zona fixa) ou
    /// `InGroup(id)` (botão "+" ao final de um grupo) pedem que se abra
    /// aba nova: `open_tab` precisa de `cell_metrics`/`proxy`/
    /// `startup_directory`, que são de `App`, não de `WindowState`, então
    /// quem chama (`App::dispatch_mouse_input`) é que abre a aba de
    /// verdade.
    fn handle_bar_click(
        &mut self,
        logical_point: (f32, f32),
        gpu: &mut GpuContext,
        right_click: bool,
    ) -> NewTabRequest {
        let style = TabBarStyle::DEFAULT;
        let bar_width = self.logical_width;
        let trilha_width = tab_bar::trilha_width(&style, bar_width);
        let h = bar_height();
        let layout = tab_bar::fit_width(&self.workspace, &style, trilha_width, gpu.text_measurer());
        let overflow = tab_bar::overflow_state(&layout, trilha_width, self.scroll_offset);
        self.scroll_offset = overflow.scroll_offset;

        if !right_click {
            // Botão de configurações: zona fixa à direita, fora da
            // trilha que rola -- resolvido em coordenadas de tela, como as
            // pílulas de overflow logo abaixo, não pelo hit-test de
            // conteúdo.
            //
            // **Inerte de propósito** (`config` é F4). O clique é
            // consumido aqui em vez de cair na trilha: o botão está
            // desenhado, e deixar o clique atravessar até a aba de baixo
            // seria pior que não responder.
            if tab_bar::point_in_settings_button(&style, bar_width, h, logical_point) {
                return NewTabRequest::None;
            }
            if overflow.hidden_left > 0
                && tab_bar::point_in_overflow_pill(
                    OverflowSide::Left,
                    trilha_width,
                    h,
                    logical_point,
                )
            {
                self.scroll_offset = (self.scroll_offset - tab_bar::OVERFLOW_SCROLL_STEP).max(0.0);
                return NewTabRequest::None;
            }
            if overflow.hidden_right > 0
                && tab_bar::point_in_overflow_pill(
                    OverflowSide::Right,
                    trilha_width,
                    h,
                    logical_point,
                )
            {
                self.scroll_offset += tab_bar::OVERFLOW_SCROLL_STEP;
                return NewTabRequest::None;
            }
        }

        let content_point = (logical_point.0 + self.scroll_offset, logical_point.1);
        let Some(hit) = tab_bar::hit_test(&layout, content_point) else {
            return NewTabRequest::None;
        };

        if right_click {
            match hit {
                TabBarHit::Tab(id) | TabBarHit::CloseButton(id) => {
                    self.close_all_popovers();
                    self.context_menu = Some(ContextMenu::new(id, logical_point));
                    self.hover.dismiss();
                }
                TabBarHit::Pill(id) => {
                    self.close_all_popovers();
                    self.group_context_menu = Some(GroupContextMenu::new(id, logical_point));
                    self.hover.dismiss();
                }
                TabBarHit::GroupNewTab(_) | TabBarHit::UngroupedNewTab => {}
            }
            return NewTabRequest::None;
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
                // ADR-0021 §3: modificador de seleção é `Ctrl` em Windows/
                // Linux e `Cmd` (`super_`) no macOS -- lá `Ctrl`+clique é o
                // clique secundário e já foi desviado pro menu de contexto
                // antes de chegar aqui (`WindowState::is_secondary_bar_click`).
                let select_modifier = if cfg!(target_os = "macos") {
                    self.modifiers.super_
                } else {
                    self.modifiers.ctrl
                };
                if self.modifiers.shift {
                    // ADR-0021 §2: intervalo sobre a ordem navegável --
                    // atravessa fronteira de grupo e já exclui abas de
                    // grupo colapsado.
                    let order: Vec<TabId> = self.workspace.navigable_order().collect();
                    self.selection.select_range(&order, id);
                } else if select_modifier {
                    self.selection.toggle(id);
                } else {
                    // RF-2.3: clique sem modificador limpa a seleção e ativa.
                    self.selection.clear();
                    self.workspace.activate_tab(id);
                    self.sync_window_title();
                    self.ensure_active_tab_visible(gpu);
                    if let Some(rect) = tab_bar::tab_rect(&layout, id) {
                        let screen_x = rect.x - self.scroll_offset;
                        self.drag = Drag::TabPressed {
                            tab: id,
                            start: logical_point,
                            grab_offset: logical_point.0 - screen_x,
                        };
                    }
                }
                NewTabRequest::None
            }
            TabBarHit::CloseButton(id) => match self.close_tab_via_button(id) {
                Some(TabCloseOutcome::Dialog(dialog)) => {
                    self.dialog = Some(dialog);
                    NewTabRequest::None
                }
                Some(TabCloseOutcome::Closed { window_empty: true }) => {
                    NewTabRequest::WindowEmptied
                }
                _ => NewTabRequest::None,
            },
            TabBarHit::Pill(id) => {
                // Espec §2.19.1/RF-2.19: arma o possível arraste do
                // rótulo -- se o gesto nunca passar do limiar de 4px,
                // `finish_drag` trata como clique de verdade (colapso ou
                // duplo clique, RF-2.13/RF-2.22).
                if let Some(rect) = tab_bar::pill_rect(&layout, id) {
                    let screen_x = rect.x - self.scroll_offset;
                    self.drag = Drag::GroupPressed {
                        group: id,
                        start: logical_point,
                        grab_offset: logical_point.0 - screen_x,
                    };
                }
                NewTabRequest::None
            }
            TabBarHit::GroupNewTab(id) => NewTabRequest::InGroup(id),
            TabBarHit::UngroupedNewTab => NewTabRequest::Ungrouped,
        }
    }

    /// RF-2.13/RF-2.22: o que um clique **de verdade** (sem cruzar o
    /// limiar de arraste) na pílula faz -- duplo clique abre o editor, no
    /// lugar do colapso que um clique simples faria (evita o flicker de
    /// colapsar-e-reabrir no segundo clique). Chamado por `finish_drag`
    /// quando o `Drag::GroupPressed` armado nunca virou `GroupDragging`.
    fn handle_pill_click(&mut self, id: GroupId, gpu: &mut GpuContext) {
        let now = Instant::now();
        let is_double_click = self.last_pill_click.is_some_and(|(last_id, at)| {
            last_id == id && now.duration_since(at) <= PILL_DOUBLE_CLICK_THRESHOLD
        });
        self.last_pill_click = Some((id, now));
        if is_double_click {
            self.last_pill_click = None;
            self.open_group_editor(id, EditorRegion::Name);
        } else {
            self.toggle_group_collapse(id, gpu);
        }
    }

    /// RF-1.6 aplicado ao botão de fechar de uma aba que não é
    /// necessariamente a ativa (o clique pode ser em qualquer aba da
    /// trilha) -- mesma condição de `action_close_tab`, mas sobre `id`
    /// explícito.
    fn close_tab_via_button(&mut self, id: TabId) -> Option<TabCloseOutcome> {
        let runtime = self.tabs.get(&id)?;
        let modes = runtime.terminal.modes();
        if modes.alt_screen || modes.mouse_reporting != MouseReporting::None {
            let title = self
                .workspace
                .tab(id)
                .map(|t| t.title().to_string())
                .unwrap_or_default();
            return Some(TabCloseOutcome::Dialog(ConfirmDialog::new(
                "Fechar aba?",
                format!("\"{title}\" tem um programa em primeiro plano. Fechar mesmo assim?"),
                "Fechar aba",
                DialogAction::CloseTab(id),
            )));
        }
        let window_empty = self.close_tab_unconditionally(id);
        Some(TabCloseOutcome::Closed { window_empty })
    }

    /// RF-2.13: clique na pílula alterna colapso. Ao **colapsar**, a
    /// seleção das abas que estão saindo de vista é invalidada primeiro
    /// (ADR-0021 §2, `Selection::invalidate_group`) -- ela usa a ordem
    /// visual de antes, que `collapse_group` ainda não mudou. `expandir`
    /// nunca invalida seleção: nenhuma aba fica menos selecionável por
    /// entrar em vista. `collapse_group` pode mover o foco (RF-2.14) --
    /// título da janela e posição de rolagem seguem a aba ativa, mesmo
    /// tratamento do clique que ativa uma aba.
    /// `group.create` (RF-2.4/RF-2.5): agrupa a seleção corrente, ou a aba
    /// ativa se nada estiver selecionado (ADR-0021 §1: "seleção vazia não
    /// é caso especial"). Nome default "Novo grupo" (não vazio -- o corpo
    /// do RF-2.4 só exige "nome em edição", que o editor aberto em seguida
    /// já cobre) e cor automática (`next_auto_color`). Captura o layout de
    /// antes pra animar a reordenação das abas que ficam contíguas
    /// (ADR-0022, `.18s`) -- primeiro gatilho de UI deste consumidor, que
    /// até aqui só existia testado direto no `AnimationClock`. Criar
    /// limpa a seleção (ADR-0021 §2) e abre o editor com foco no nome
    /// (RF-2.4: "nasce... em modo de edição").
    fn action_group_create(&mut self, gpu: &mut GpuContext) {
        if self.rename.editing_tab().is_some() {
            self.commit_rename();
        }
        let ids: Vec<TabId> = if self.selection.is_empty() {
            self.workspace.active_tab().into_iter().collect()
        } else {
            self.workspace
                .visual_order()
                .filter(|id| self.selection.is_selected(*id))
                .collect()
        };
        if ids.is_empty() {
            return;
        }

        let style = TabBarStyle::DEFAULT;
        let old_layout = tab_bar::fit_width(
            &self.workspace,
            &style,
            self.logical_width,
            gpu.text_measurer(),
        );
        let color = self.workspace.next_auto_color();
        let Some(group) = self.workspace.group_tabs(&ids, "Novo grupo", color) else {
            return;
        };
        self.animations
            .start_reflow(&old_layout, GROUP_CREATE_REFLOW_DURATION, Instant::now());
        self.selection.clear();
        self.open_group_editor(group, EditorRegion::Name);
        self.sync_window_title();
    }

    fn toggle_group_collapse(&mut self, id: GroupId, gpu: &mut GpuContext) {
        let Some(group) = self.workspace.group(id) else {
            return;
        };
        let collapsing = !group.is_collapsed();
        if collapsing {
            let order: Vec<TabId> = self.workspace.visual_order().collect();
            let group_tabs: Vec<TabId> = group.tabs().to_vec();
            self.selection.invalidate_group(&group_tabs, &order);
        }
        // ADR-0022: captura o layout ANTES do colapso -- é o que a
        // pintura interpola até a posição nova ao longo de `.15s`. Grupos
        // depois deste (ou antes, se expandindo) deslizam em vez de
        // saltar; o sublinhado/caret não têm equivalente animado (nota do
        // módulo `animation.rs`).
        let style = TabBarStyle::DEFAULT;
        let old_layout = tab_bar::fit_width(
            &self.workspace,
            &style,
            self.logical_width,
            gpu.text_measurer(),
        );
        self.workspace.collapse_group(id, collapsing);
        self.animations
            .start_reflow(&old_layout, COLLAPSE_REFLOW_DURATION, Instant::now());
        self.sync_window_title();
        self.ensure_active_tab_visible(gpu);
    }

    /// RF-2.9/RF-2.10/RF-2.22: abre o editor de grupo (ADR-0023) com o
    /// foco dado -- `group.rename` foca o nome, `group.set_color` a
    /// faixa, duplo clique na pílula o nome (mesmo default de
    /// `group.rename`). Sem efeito sobre grupo implícito ou `id`
    /// inexistente (`docs/reference/acoes.md`: "indisponível").
    fn open_group_editor(&mut self, group: GroupId, focus: EditorRegion) {
        let Some(g) = self.workspace.group(group) else {
            return;
        };
        if !g.is_explicit() {
            return;
        }
        let name = g.name().unwrap_or_default().to_string();
        let color_index = g.color().map(GroupColor::index).unwrap_or(0);
        self.close_all_popovers();
        self.group_editor = Some(GroupEditor::new(group, &name, color_index, focus));
    }

    /// Executa uma ação da lista única de grupo (RF-10.21,
    /// `group_menu::group_action_items`) -- chamado tanto pelo menu de
    /// contexto do grupo quanto pela lista de ações do editor
    /// (`EDITOR_ACTION_ORDER` é subconjunto, mas os dois caem aqui).
    /// `Rename`/`SetColor` abrem o editor em vez de executar direto
    /// (ADR-0023 §4); os demais tocam o `Workspace` na hora. `CloseAll`
    /// devolve o diálogo de confirmação pra `App` montar -- RF-2.23 exige
    /// confirmação sempre, e o diálogo é `Option<ConfirmDialog>` por
    /// janela, resolvido por quem já tem `self.dialog` em mãos.
    #[allow(clippy::too_many_arguments)]
    fn run_group_action(
        &mut self,
        group: GroupId,
        action: GroupAction,
        gpu: &mut GpuContext,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        startup_directory: &Option<PathBuf>,
        now: Instant,
    ) {
        match action {
            GroupAction::Rename => self.open_group_editor(group, EditorRegion::Name),
            GroupAction::SetColor => self.open_group_editor(group, EditorRegion::Swatches),
            GroupAction::ToggleCollapse => self.toggle_group_collapse(group, gpu),
            GroupAction::NewTab => {
                self.action_new_tab_in_group(group, cell_metrics, proxy, startup_directory, now);
            }
            GroupAction::CloseAll => {
                let count = self
                    .workspace
                    .group(group)
                    .map(|g| g.tabs().len())
                    .unwrap_or(0);
                let plural = if count == 1 { "" } else { "s" };
                self.dialog = Some(ConfirmDialog::new(
                    "Fechar grupo?",
                    format!("Isso fecha {count} aba{plural}."),
                    format!("Fechar grupo ({count} aba{plural})"),
                    DialogAction::CloseGroup(group),
                ));
            }
            GroupAction::Dissolve => {
                self.workspace.ungroup(group);
                if self.group_editor.as_ref().is_some_and(|e| e.group == group) {
                    self.group_editor = None;
                }
            }
        }
    }

    /// Conveniência de `group.new_tab` (RF-2.8/RF-2.22): sempre no fim de
    /// `group`, mesmo que a aba ativa esteja em outro grupo -- diferente
    /// de `action_new_tab`, que segue o grupo da aba ativa.
    /// "+" ao fim da trilha: aba **sem grupo**, no fim da barra.
    /// Separado de [`Self::action_new_tab`] de propósito -- o atalho
    /// `tab.new` continua seguindo o grupo da aba ativa (RF-1.1), que é o
    /// que o ADR-0020 §1 manda.
    fn action_new_tab_ungrouped(
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
        self.open_tab(cell_metrics, proxy, cwd, now, NewTabTarget::Ungrouped);
    }

    fn action_new_tab_in_group(
        &mut self,
        group: GroupId,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        startup_directory: &Option<PathBuf>,
        now: Instant,
    ) {
        if self.rename.editing_tab().is_some() {
            self.commit_rename();
        }
        let cwd = self.resolve_group_new_tab_cwd(group, startup_directory);
        self.open_tab(cell_metrics, proxy, cwd, now, NewTabTarget::Group(group));
    }

    /// `group.close_all` (RF-2.22/RF-2.23), já confirmado: fecha cada aba
    /// do grupo pelo mesmo caminho de `close_tab_via_button`/`tab.close`
    /// (sinaliza sem bloquear, remove do `Workspace`). Devolve `true` se a
    /// janela ficou sem abas -- mesmo contrato de `close_tab_unconditionally`.
    fn close_group_unconditionally(&mut self, group: GroupId) -> bool {
        let Some(g) = self.workspace.group(group) else {
            return false;
        };
        let tabs: Vec<TabId> = g.tabs().to_vec();
        let mut window_empty = false;
        for id in tabs {
            window_empty = self.close_tab_unconditionally(id);
        }
        if self.group_editor.as_ref().is_some_and(|e| e.group == group) {
            self.group_editor = None;
        }
        window_empty
    }

    /// Solta o botão do mouse com um arraste em andamento (espec §2.19/
    /// §2.19.1): aplica o alvo calculado no último redraw se o cursor
    /// ainda está dentro da barra, ou cancela em silêncio se não está --
    /// "soltar fora da trilha cancela também". `self.workspace` nunca foi
    /// mexido durante o arraste (só o preview clonado em `RedrawRequested`
    /// era), então cancelar é só voltar `drag` pra `Idle`, sem desfazer
    /// nada. `GroupPressed` que nunca virou `GroupDragging` é um clique de
    /// verdade na pílula -- resolvido aqui (`handle_pill_click`), não no
    /// `press`, porque só agora se sabe que não foi arraste. `TabPressed`
    /// sem `TabDragging` não precisa de nada: o clique já ativou a aba na
    /// hora do `press`.
    fn finish_drag(&mut self, gpu: &mut GpuContext) {
        let drag = std::mem::replace(&mut self.drag, Drag::Idle);
        match drag {
            Drag::TabDragging { tab, target, .. } if self.in_bar(self.cursor_position.1) => {
                match target {
                    DragDrop::IntoGroup { group, pos } => {
                        self.workspace.move_tab_to_group_at(tab, group, pos);
                    }
                    DragDrop::NewRun { group_index } => {
                        self.workspace.move_tab_to_new_run(tab, group_index);
                    }
                }
            }
            Drag::GroupDragging {
                group,
                preview_index,
                ..
            } if self.in_bar(self.cursor_position.1) => {
                self.workspace.move_group(group, preview_index);
            }
            Drag::GroupPressed { group, .. } => {
                self.handle_pill_click(group, gpu);
            }
            Drag::TabPressed { .. } | Drag::TabDragging { .. } | Drag::GroupDragging { .. } => {}
            Drag::Idle => {}
        }
        self.window.set_cursor(CursorIcon::Default);
    }

    /// `y` físico está dentro da faixa da barra de abas (topo da janela).
    fn in_bar(&self, physical_y: f64) -> bool {
        physical_y < (bar_height() * self.scale) as f64
    }

    /// O que abre o menu de contexto da barra (ADR-0021 §3): botão direito
    /// em qualquer plataforma, e `Ctrl`+clique esquerdo **só** no macOS --
    /// lá é o clique secundário da plataforma, e não toca a seleção.
    fn is_secondary_bar_click(&self, button: MouseButton) -> bool {
        button == MouseButton::Right
            || (cfg!(target_os = "macos") && button == MouseButton::Left && self.modifiers.ctrl)
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
        let content =
            paint::terminal_content_rect(bar_height(), self.logical_width, self.logical_height);
        let content_x = self.cursor_position.0 - (content.x * self.scale) as f64;
        let content_y = self.cursor_position.1 - (content.y * self.scale) as f64;
        let (rows, cols) = self.active_runtime().map_or((MIN_GRID, MIN_GRID), |rt| {
            (
                rt.snapshot.rows.max(MIN_GRID),
                rt.snapshot.cols.max(MIN_GRID),
            )
        });
        input::cell_at(
            content_x.max(0.0),
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
            && self.group_context_menu.is_none()
            && self.group_editor.is_none()
            && self.move_to_group.is_none()
            && matches!(self.drag, Drag::Idle)
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
        self.animations.tick(now);
    }

    fn next_wake(&self, now: Instant) -> Option<Instant> {
        [
            self.warnings.next_deadline(),
            self.hover.next_deadline(),
            self.animations.next_deadline(now),
        ]
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
        let mut attributes = Window::default_attributes()
            .with_title("Porecatu")
            .with_window_icon(Some(app_icon::load()));
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
            self.cell_metrics = snap_cell_metrics_to_pixel_grid(cell_width, cell_height, scale);
        }

        // ADR-0015: "a janela nova abre com uma aba no cwd da aba ativa no
        // momento da criação" -- da janela de ORIGEM, já que a nova ainda
        // não tem nenhuma aba.
        let cwd = origin_state
            .and_then(|s| s.resolve_new_tab_cwd(&self.startup_directory))
            .or_else(|| self.startup_directory.clone());

        let window_id = window.id();
        let mut state = WindowState::new(window, window_surface, scale);
        state.open_tab(
            self.cell_metrics,
            &self.proxy,
            cwd,
            Instant::now(),
            NewTabTarget::ActiveGroup,
        );
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
            DialogAction::CloseGroup(id) => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    let empty = state.close_group_unconditionally(id);
                    if empty {
                        self.close_window_unconditionally(window_id, event_loop);
                    } else {
                        state.window.request_redraw();
                    }
                }
            }
        }
    }

    /// Executa um item do menu de contexto de aba (RF-1.1, RF-1.2, RF-2.20).
    /// `menu` (não só `tab`) porque `MoveToGroup` precisa do `anchor` do
    /// menu que o abriu -- o popover de destino nasce no mesmo ponto, sem
    /// virar submenu (ADR-0023 §4).
    fn run_menu_action(
        &mut self,
        window_id: WindowId,
        menu: ContextMenu,
        action: MenuAction,
        event_loop: &ActiveEventLoop,
    ) {
        let tab = menu.tab;
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
                    match state.close_tab_via_button(tab) {
                        Some(TabCloseOutcome::Dialog(dialog)) => {
                            state.dialog = Some(dialog);
                            state.window.request_redraw();
                        }
                        // Pedido do usuário: fechar a última aba (sem
                        // grupo, sem solta) pelo menu de contexto fecha a
                        // janela sozinha, mesmo caminho do atalho e do
                        // botão de fechar.
                        Some(TabCloseOutcome::Closed { window_empty: true }) => {
                            self.close_window_unconditionally(window_id, event_loop);
                        }
                        _ => {
                            state.window.request_redraw();
                        }
                    }
                }
            }
            MenuAction::MoveToGroup => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    let current_group = state.workspace.group_of_tab(tab);
                    let targets: Vec<GroupId> = state
                        .workspace
                        .groups()
                        .iter()
                        .filter(|g| g.is_explicit() && Some(g.id()) != current_group)
                        .map(|g| g.id())
                        .collect();
                    state.close_all_popovers();
                    state.move_to_group = Some(MoveToGroupPopover::new(tab, menu.anchor, targets));
                    state.window.request_redraw();
                }
            }
        }
    }

    /// Agenda o próximo despertar via `ControlFlow::WaitUntil` -- o
    /// temporizador da informação (ADR-0014), o atraso do tooltip
    /// (ADR-0019) e o relógio de animação (ADR-0022) marcam sujeira, não
    /// rodam loop nenhum: quando não há nada pendente em nenhuma janela, o
    /// event loop dorme de verdade (`ControlFlow::Wait`).
    fn schedule_next_wake(&self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let next = self.windows.values().filter_map(|w| w.next_wake(now)).min();
        match next {
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    /// Roda o `tick` (expira avisos, promove tooltip pendente, avança/
    /// remove animação) em todas as janelas e redesenha as que mudaram --
    /// ou que têm animação em curso, já que ela precisa de um frame por
    /// intervalo enquanto ativa (ADR-0022).
    fn tick_all(&mut self) {
        let now = Instant::now();
        for state in self.windows.values_mut() {
            let had_warnings = !state.warnings.is_empty();
            let had_tooltip = state.hover.visible().is_some();
            let was_animating = !state.animations.is_empty();
            state.tick(now);
            if was_animating
                || had_warnings != !state.warnings.is_empty()
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
                // ADR-0019: tooltip some ao perder foco a janela. Menus,
                // editor de grupo e popover de destino: mesma regra do
                // ADR-0014/ADR-0023 ("perda de foco fecha").
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.hover.dismiss();
                    state.close_all_popovers();
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
        // ADR-0022: qualquer tecla descarta animação em curso e aplica o
        // estado final na hora -- a animação nunca bloqueia input.
        state.animations.clear();

        // Cadeia de captura (ADR-0008 passo 1): diálogo modal > menu de
        // contexto > cancelamento de arraste > rename > aviso > seleção
        // (ADR-0021 §2, `Esc` limpa) > ações de aba/janela > terminal. Cada
        // nível consome a tecla por inteiro -- "um binding que casa nunca
        // cai para o terminal" vale igual pros modos de captura.
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
            let menu = state.context_menu;
            if let Some(action) = state.handle_context_menu_key(&key)
                && let Some(menu) = menu
            {
                self.run_menu_action(window_id, menu, action, event_loop);
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }
        if state.group_context_menu.is_some() {
            let group = state.group_context_menu.map(|m| m.group);
            if let Some(action) = state.handle_group_menu_key(&key)
                && let Some(group) = group
                && let Some(gpu) = &mut self.gpu
            {
                state.run_group_action(
                    group,
                    action,
                    gpu,
                    self.cell_metrics,
                    &self.proxy,
                    &self.startup_directory,
                    Instant::now(),
                );
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }
        if state.group_editor.is_some() {
            let outcome = state.handle_group_editor_key(&key);
            if let GroupEditorOutcome::Action(group, action) = outcome
                && let Some(gpu) = &mut self.gpu
            {
                state.run_group_action(
                    group,
                    action,
                    gpu,
                    self.cell_metrics,
                    &self.proxy,
                    &self.startup_directory,
                    Instant::now(),
                );
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }
        if state.move_to_group.is_some() {
            let tab = state.move_to_group.as_ref().map(|p| p.tab);
            if let Some(target) = state.handle_move_to_group_key(&key)
                && let Some(tab) = tab
            {
                state.run_move_target(tab, target);
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }
        if matches!(
            state.drag,
            Drag::TabDragging { .. } | Drag::GroupDragging { .. }
        ) && key.state == ElementState::Pressed
            && matches!(key.logical_key, Key::Named(NamedKey::Escape))
        {
            state.drag = Drag::Idle;
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
        // ADR-0021 §2: `Esc` limpa a seleção -- depois do rename e do
        // diálogo (já resolvidos acima), antes da tabela de keybindings.
        if !state.selection.is_empty()
            && key.state == ElementState::Pressed
            && matches!(key.logical_key, Key::Named(NamedKey::Escape))
        {
            state.selection.clear();
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
            ActionOutcome::WindowEmptied => {
                self.close_window_unconditionally(window_id, event_loop);
            }
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

    /// Clique com o editor de grupo aberto: campo foca, swatch foca e
    /// **aplica** a cor (clique não segue a semântica "realça, `Enter`
    /// aciona" do teclado -- um clique num color picker já é a escolha,
    /// mesmo padrão de clicar um item do menu de contexto), ação foca e
    /// **executa**. Fora de tudo isso, ou botão que não é o esquerdo,
    /// fecha.
    fn dispatch_group_editor_click(
        &mut self,
        window_id: WindowId,
        logical_point: (f32, f32),
        button: MouseButton,
    ) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let Some(group) = state.group_editor.as_ref().map(|e| e.group) else {
            return;
        };
        let anchor_x = state.group_pill_screen_x(group, gpu);
        let layout = overlay::layout_group_editor(
            anchor_x,
            bar_height(),
            state.logical_width,
            state.logical_height,
        );
        let hit = overlay::group_editor_hit(&layout, logical_point);

        if button != MouseButton::Left {
            state.group_editor = None;
            return;
        }
        match hit {
            Some(overlay::GroupEditorHit::NameField) => {
                if let Some(editor) = &mut state.group_editor {
                    editor.set_focus(EditorRegion::Name);
                }
            }
            Some(overlay::GroupEditorHit::Swatch(index)) => {
                if let Some(editor) = &mut state.group_editor {
                    editor.set_focus(EditorRegion::Swatches);
                    editor.set_swatch_highlight(index);
                }
                state
                    .workspace
                    .set_group_color(group, GroupColor::ALL[index]);
            }
            Some(overlay::GroupEditorHit::Action(index)) => {
                if let Some(editor) = &mut state.group_editor {
                    editor.set_focus(EditorRegion::Actions);
                    editor.set_action_highlight(index);
                }
                let action = group_menu::EDITOR_ACTION_ORDER[index];
                state.run_group_action(
                    group,
                    action,
                    gpu,
                    self.cell_metrics,
                    &self.proxy,
                    &self.startup_directory,
                    Instant::now(),
                );
            }
            None => {
                state.group_editor = None;
            }
        }
    }

    fn dispatch_mouse_wheel(&mut self, window_id: WindowId, delta: MouseScrollDelta) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        if state.dialog.is_some()
            || state.context_menu.is_some()
            || state.group_context_menu.is_some()
            || state.group_editor.is_some()
            || state.move_to_group.is_some()
        {
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
        // Menu de contexto de grupo: mesmo tratamento do menu de aba acima.
        if let Some(menu) = &mut state.group_context_menu {
            let logical_point = (
                position.x as f32 / state.scale,
                position.y as f32 / state.scale,
            );
            let is_collapsed = state
                .workspace
                .group(menu.group)
                .is_some_and(|g| g.is_collapsed());
            let tab_count = state
                .workspace
                .group(menu.group)
                .map(|g| g.tabs().len())
                .unwrap_or(0);
            let item_count = group_menu::group_action_items(is_collapsed, tab_count).len();
            let layout = overlay::layout_group_menu(
                menu,
                item_count,
                state.logical_width,
                state.logical_height,
            );
            if let Some(index) = overlay::group_menu_hit(&layout, logical_point) {
                menu.set_highlight(index);
            }
            state.window.request_redraw();
            return;
        }
        // Editor de grupo: hover move o realce dentro da faixa de
        // swatches/lista de ações -- o campo de nome não tem realce
        // próprio (espec. §2.10: só faixa e lista navegam por realce).
        if let Some(group) = state.group_editor.as_ref().map(|e| e.group) {
            let logical_point = (
                position.x as f32 / state.scale,
                position.y as f32 / state.scale,
            );
            if let Some(gpu) = &mut self.gpu {
                let anchor_x = state.group_pill_screen_x(group, gpu);
                if let Some(editor) = &mut state.group_editor {
                    let layout = overlay::layout_group_editor(
                        anchor_x,
                        bar_height(),
                        state.logical_width,
                        state.logical_height,
                    );
                    match overlay::group_editor_hit(&layout, logical_point) {
                        Some(overlay::GroupEditorHit::Swatch(i)) => {
                            editor.set_focus(EditorRegion::Swatches);
                            editor.set_swatch_highlight(i);
                        }
                        Some(overlay::GroupEditorHit::Action(i)) => {
                            editor.set_focus(EditorRegion::Actions);
                            editor.set_action_highlight(i);
                        }
                        Some(overlay::GroupEditorHit::NameField) | None => {}
                    }
                }
            }
            state.window.request_redraw();
            return;
        }
        // Popover de destino: hover move o realce entre as linhas
        // visíveis -- sem rolar por hover, só o realce por teclado arrasta
        // a janela visível (nota de `overlay.rs`).
        if let Some(popover) = &mut state.move_to_group {
            let logical_point = (
                position.x as f32 / state.scale,
                position.y as f32 / state.scale,
            );
            let layout =
                overlay::layout_move_to_group(popover, state.logical_width, state.logical_height);
            if let Some(index) = overlay::move_to_group_hit(&layout, logical_point) {
                popover.set_highlight(index);
            }
            state.window.request_redraw();
            return;
        }

        let handled_by_drag = match &mut state.drag {
            Drag::TabPressed {
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
                    // Alvo provisório -- `redraw()` recalcula pelo cursor
                    // corrente antes de qualquer pintura ou solta usá-lo.
                    let dragging = Drag::TabDragging {
                        tab: *tab,
                        grab_offset: *grab_offset,
                        target: DragDrop::NewRun { group_index: 0 },
                    };
                    state.drag = dragging;
                    state.window.set_cursor(CursorIcon::Grabbing);
                    state.hover.dismiss();
                }
                true
            }
            Drag::TabDragging { .. } | Drag::GroupDragging { .. } => {
                let logical_x = position.x as f32 / state.scale;
                let trilha_width =
                    tab_bar::trilha_width(&TabBarStyle::DEFAULT, state.logical_width);
                if logical_x < DRAG_EDGE_ZONE_PX {
                    state.scroll_offset = (state.scroll_offset - DRAG_AUTOSCROLL_STEP_PX).max(0.0);
                } else if logical_x > trilha_width - DRAG_EDGE_ZONE_PX {
                    state.scroll_offset += DRAG_AUTOSCROLL_STEP_PX;
                }
                true
            }
            Drag::GroupPressed {
                group,
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
                    state.drag = Drag::GroupDragging {
                        group: *group,
                        grab_offset: *grab_offset,
                        preview_index: 0,
                    };
                    state.window.set_cursor(CursorIcon::Grabbing);
                    state.hover.dismiss();
                }
                true
            }
            Drag::Idle => false,
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
        if pressed {
            // ADR-0022: qualquer clique descarta animação em curso.
            state.animations.clear();
        }

        if !pressed {
            if button == MouseButton::Left && !matches!(state.drag, Drag::Idle) {
                if let Some(gpu) = &mut self.gpu {
                    state.finish_drag(gpu);
                }
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
                    menu,
                    context_menu::TAB_MENU_ITEMS[index].action,
                    event_loop,
                );
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }

        // Menu de contexto de grupo: mesmo padrão do menu de aba acima,
        // sem item desabilitado (`group_menu.rs`, nota do módulo).
        if let Some(menu) = state.group_context_menu {
            let is_collapsed = state
                .workspace
                .group(menu.group)
                .is_some_and(|g| g.is_collapsed());
            let tab_count = state
                .workspace
                .group(menu.group)
                .map(|g| g.tabs().len())
                .unwrap_or(0);
            let items = group_menu::group_action_items(is_collapsed, tab_count);
            let layout = overlay::layout_group_menu(
                &menu,
                items.len(),
                state.logical_width,
                state.logical_height,
            );
            let hit = overlay::group_menu_hit(&layout, logical_point);
            state.group_context_menu = None;
            if button == MouseButton::Left
                && let Some(index) = hit
                && let Some(gpu) = &mut self.gpu
            {
                state.run_group_action(
                    menu.group,
                    items[index].action,
                    gpu,
                    self.cell_metrics,
                    &self.proxy,
                    &self.startup_directory,
                    Instant::now(),
                );
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }

        // Editor de grupo: clique no campo/swatch/ação; fora fecha.
        if state.group_editor.is_some() {
            self.dispatch_group_editor_click(window_id, logical_point, button);
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }

        // Popover de destino do `tab.move_to_group`: clique numa linha
        // move, qualquer outro fecha.
        if let Some(popover) = &state.move_to_group {
            let layout =
                overlay::layout_move_to_group(popover, state.logical_width, state.logical_height);
            let hit = overlay::move_to_group_hit(&layout, logical_point);
            let tab = popover.tab;
            if button == MouseButton::Left
                && let Some(index) = hit
            {
                let mut popover_at_index = state.move_to_group.take().expect("checado acima");
                popover_at_index.set_highlight(index);
                let target = popover_at_index.selected();
                state.run_move_target(tab, target);
            } else {
                state.move_to_group = None;
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

        if state.is_secondary_bar_click(button) && state.in_bar(state.cursor_position.1) {
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
            match state.handle_bar_click(logical_point, gpu, false) {
                NewTabRequest::Ungrouped => {
                    state.action_new_tab_ungrouped(
                        self.cell_metrics,
                        &self.proxy,
                        &self.startup_directory,
                        Instant::now(),
                    );
                    state.window.request_redraw();
                }
                NewTabRequest::InGroup(group) => {
                    state.action_new_tab_in_group(
                        group,
                        self.cell_metrics,
                        &self.proxy,
                        &self.startup_directory,
                        Instant::now(),
                    );
                    state.window.request_redraw();
                }
                // Pedido do usuário: fechar a última aba (sem grupo, sem
                // solta) fecha a janela sozinha -- `state` não é mais
                // válido depois disto, então nada de `request_redraw`.
                NewTabRequest::WindowEmptied => {
                    self.close_window_unconditionally(window_id, event_loop);
                }
                NewTabRequest::None => {
                    state.window.request_redraw();
                }
            }
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
        let trilha_width = tab_bar::trilha_width(&style, bar_width);
        let base_layout =
            tab_bar::fit_width(&state.workspace, &style, bar_width, gpu.text_measurer());
        let overflow = tab_bar::overflow_state(&base_layout, trilha_width, state.scroll_offset);
        state.scroll_offset = overflow.scroll_offset;

        // Durante um arraste, o `Workspace` de verdade não é tocado -- só
        // um clone com o preview aplicado é usado pra desenhar (espec
        // §2.19/§2.19.1: as vizinhas "deslizam" mostrando onde a aba/o
        // grupo cairia; o `Workspace` real só recebe a troca ao soltar).
        let mut drag_ghost = None;
        let mut group_drag_ghost = None;
        let mut drag_highlight = None;
        let now = Instant::now();
        let paint_layout = match state.drag {
            Drag::TabDragging {
                tab, grab_offset, ..
            } => {
                let cursor_logical_x = state.cursor_position.0 as f32 / state.scale;
                let ghost_screen_x = cursor_logical_x - grab_offset;
                let ghost_content_x = ghost_screen_x + state.scroll_offset;
                // `base_layout` (não o preview): a fonte do conteúdo do
                // fantasma tem que existir sempre, mesmo quando o alvo
                // corrente é um grupo colapsado -- lá o preview não gera
                // `TabRect` nenhum pra esta aba (nota de `chrome::DragGhost`).
                let source = base_layout
                    .groups
                    .iter()
                    .flat_map(|g| &g.tabs)
                    .find(|t| t.id == tab)
                    .cloned();
                let width = source.as_ref().map(|t| t.rect.width).unwrap_or(0.0);
                let ghost_center = ghost_content_x + width / 2.0;
                let target = tab_bar::drag_target(&base_layout, tab, ghost_center);
                state.drag = Drag::TabDragging {
                    tab,
                    grab_offset,
                    target,
                };
                if let Some(source) = source {
                    drag_ghost = Some(DragGhost {
                        tab,
                        screen_x: ghost_screen_x,
                        source,
                    });
                }
                drag_highlight = tab_bar::drag_highlight_rect(&base_layout, target);

                let mut preview = state.workspace.clone();
                match target {
                    DragDrop::IntoGroup { group, pos } => {
                        preview.move_tab_to_group_at(tab, group, pos);
                    }
                    DragDrop::NewRun { group_index } => {
                        preview.move_tab_to_new_run(tab, group_index);
                    }
                }
                tab_bar::fit_width(&preview, &style, bar_width, gpu.text_measurer())
            }
            Drag::GroupDragging {
                group, grab_offset, ..
            } => {
                let cursor_logical_x = state.cursor_position.0 as f32 / state.scale;
                let ghost_screen_x = cursor_logical_x - grab_offset;
                let ghost_content_x = ghost_screen_x + state.scroll_offset;
                let width = tab_bar::pill_rect(&base_layout, group)
                    .map(|r| r.width)
                    .unwrap_or(0.0);
                let ghost_center = ghost_content_x + width / 2.0;
                let preview_index =
                    tab_bar::group_drag_target_index(&base_layout, group, ghost_center);
                state.drag = Drag::GroupDragging {
                    group,
                    grab_offset,
                    preview_index,
                };
                group_drag_ghost = Some(GroupDragGhost {
                    group,
                    screen_x: ghost_screen_x,
                });

                let mut preview = state.workspace.clone();
                preview.move_group(group, preview_index);
                tab_bar::fit_width(&preview, &style, bar_width, gpu.text_measurer())
            }
            _ => base_layout,
        };

        let chrome_primitives = chrome::paint(
            &paint_layout,
            &state.workspace,
            state.workspace.active_tab(),
            &state.rename,
            &state.selection,
            state.group_editor.as_ref(),
            &style,
            bar_width,
            overflow,
            drag_ghost,
            group_drag_ghost,
            drag_highlight,
            &state.animations,
            now,
            gpu.text_measurer(),
        );
        frame.set_layer(Layer::Chrome, chrome_primitives);

        let h = bar_height();
        if let Some(id) = state.workspace.active_tab()
            && let Some(runtime) = state.tabs.get_mut(&id)
        {
            runtime.terminal.snapshot_into(&mut runtime.snapshot);
            let box_rect = paint::terminal_box_rect(h, state.logical_width, state.logical_height);
            let primitives = paint::build_primitives(
                &runtime.snapshot,
                self.cell_metrics,
                FONT_SIZE_PX,
                box_rect,
                gpu.text_measurer(),
            );
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
        if let Some(menu) = &state.group_context_menu {
            let is_collapsed = state
                .workspace
                .group(menu.group)
                .is_some_and(|g| g.is_collapsed());
            let tab_count = state
                .workspace
                .group(menu.group)
                .map(|g| g.tabs().len())
                .unwrap_or(0);
            let items = group_menu::group_action_items(is_collapsed, tab_count);
            let layout = overlay::layout_group_menu(
                menu,
                items.len(),
                state.logical_width,
                state.logical_height,
            );
            popover.extend(overlay::paint_group_menu(
                &layout,
                &items,
                menu.highlighted(),
            ));
        }
        if let Some(editor) = &state.group_editor {
            let anchor_x = state.group_pill_screen_x(editor.group, gpu);
            let layout = overlay::layout_group_editor(
                anchor_x,
                h,
                state.logical_width,
                state.logical_height,
            );
            let current_color_index = state
                .workspace
                .group(editor.group)
                .and_then(|g| g.color())
                .map(GroupColor::index)
                .unwrap_or(0);
            let is_collapsed = state
                .workspace
                .group(editor.group)
                .is_some_and(|g| g.is_collapsed());
            let tab_count = state
                .workspace
                .group(editor.group)
                .map(|g| g.tabs().len())
                .unwrap_or(0);
            popover.extend(overlay::paint_group_editor(
                &layout,
                editor,
                current_color_index,
                is_collapsed,
                tab_count,
                gpu.text_measurer(),
            ));
        }
        if let Some(mv) = &state.move_to_group {
            let layout =
                overlay::layout_move_to_group(mv, state.logical_width, state.logical_height);
            popover.extend(overlay::paint_move_to_group(
                &layout,
                mv,
                &state.workspace,
                gpu.text_measurer(),
            ));
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

        // `BAR_BACKGROUND`, não `TERM_BACKGROUND`: a margem entre a borda da
        // janela e o box arredondado do terminal (`paint::TERMINAL_BOX_MARGIN`)
        // precisa de uma cor diferente da do box para o quadro aparecer --
        // a mesma da barra de abas, já que os dois formam o "quadro" do app.
        state
            .window_surface
            .render(gpu, palette::BAR_BACKGROUND, &frame);
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
