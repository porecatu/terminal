// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use porecatu_core::{Action, GroupId, TabId, Workspace};
use porecatu_render::{Color, Frame, GpuContext, Layer, Rect, WindowSurface};
use porecatu_term::{
    DEFAULT_SEARCH_LINES_PER_STEP, GridSnapshot, HyperlinkSpan, Modifiers, MouseReporting, PtySize,
    SpawnConfig, TermEvent, TermModes, TermParams, TermScroll, Terminal, resolve_default_shell,
    search_path,
};
use winit::application::ApplicationHandler;
use winit::event::{
    ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{CursorIcon, Window, WindowAttributes, WindowId};

mod animation;
mod app_icon;
mod chrome;
mod clipboard;
mod context_menu;
mod dialog;
mod group_editor;
mod group_menu;
mod hyperlink;
mod input;
mod keymap;
mod move_to_group;
mod overlay;
mod paint;
mod palette;
mod reload;
mod rename;
mod search_bar;
mod selection;
mod session_writer;
mod shell_integration;
mod tab_bar;
mod terminal_menu;
mod text_field;
mod titlebar;
mod tooltip;
mod warning;

use animation::AnimationClock;
use chrome::{DragGhost, GroupDragGhost};
use context_menu::{ContextMenu, MenuAction};
use dialog::{ConfirmDialog, DialogAction, DialogButton};
use group_editor::{EditorRegion, GroupEditor};
use group_menu::{GroupAction, GroupContextMenu};
use input::ClickTracker;
use keymap::Chord;
use move_to_group::{MoveTarget, MoveToGroupPopover};
use paint::CellMetrics;
use porecatu_core::GroupColor;
use reload::ConfigReload;
use rename::RenameState;
use search_bar::{SearchBarHit, SearchBarState};
use selection::Selection;
use session_writer::SessionScheduler;
use tab_bar::{DragDrop, OverflowSide, TabBarHit, TabBarStyle};
use terminal_menu::{
    TerminalContextMenu, TerminalMenuAction, TerminalMenuItem, terminal_menu_items,
};
use text_field::{TextFieldState, apply_text_field_key};
use tooltip::Hover;
use warning::{Severity, WarningStack};

/// Janela de tempo entre um clique e o próximo pra contar como duplo
/// clique. Nasceu só para a pílula (RF-2.22: "abrir o editor por duplo
/// clique no rótulo"), e desde o ADR-0027 serve também a drag region da
/// barra (duplo clique maximiza/restaura a janela, `WindowState::
/// resolve_titlebar_click_or_drag`) -- por isso o nome neutro. Mesmo
/// valor e mesma nota de procedência de `input::MULTI_CLICK_THRESHOLD` --
/// convenção comum de SO, não token de design.
const DOUBLE_CLICK_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(500);

/// `cfg!(target_os = "macos")` como valor comum, não atributo -- mesmo
/// padrão de `WindowState::is_secondary_bar_click`. Centraliza a
/// checagem de plataforma que o ADR-0027 espalha por `tab_bar`/`chrome`/
/// `titlebar` (semáforo nativo vs. botões de janela nossos).
fn is_macos() -> bool {
    cfg!(target_os = "macos")
}

/// Itens do menu de contexto do terminal, resolvidos do estado ao vivo da
/// aba (`menu.tab`) e da célula sob `menu.anchor` (RF-11.14/RF-11.15) --
/// mesmo padrão de `group_menu::group_action_items`: recomputado a cada
/// abertura/navegação/pintura, nunca guardado no `TerminalContextMenu`
/// (nota do módulo `terminal_menu.rs`). Função livre, não método de
/// `App`/`WindowState`, porque os dois às vezes precisam ser chamados com
/// `state.tabs` já emprestado por outro campo do mesmo `state`.
#[allow(clippy::too_many_arguments)]
fn terminal_menu_context_items(
    tabs: &HashMap<TabId, TabRuntime>,
    style: &TabBarStyle,
    cell_metrics: CellMetrics,
    logical_width: f32,
    logical_height: f32,
    scale: f32,
    tab: TabId,
    anchor: (f32, f32),
) -> Vec<TerminalMenuItem> {
    let Some(rt) = tabs.get(&tab) else {
        return terminal_menu_items(false, false);
    };
    let has_selection = rt.terminal.selection_text().is_some();
    let content =
        paint::terminal_content_rect(style, bar_height(style), logical_width, logical_height);
    let content_x = ((anchor.0 - content.x) * scale).max(0.0) as f64;
    let content_y = ((anchor.1 - content.y) * scale).max(0.0) as f64;
    let cell = input::cell_at(
        content_x,
        content_y,
        cell_metrics,
        rt.snapshot.rows.max(MIN_GRID),
        rt.snapshot.cols.max(MIN_GRID),
    );
    let over_link = hyperlink::uri_at(&rt.snapshot, cell.row, cell.col).is_some();
    terminal_menu_items(has_selection, over_link)
}

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

/// Atributos comuns a toda janela do Porecatu (ADR-0027: sem decoração
/// nativa fora do macOS, onde o semáforo continua nativo). Compartilhado
/// por [`App::open_window`] e [`App::open_window_from_session`] -- cada
/// um soma por cima só a geometria que decide sozinho (cascata da
/// origem, ou a gravada na sessão).
fn base_window_attributes() -> WindowAttributes {
    #[allow(unused_mut)]
    let mut attributes = Window::default_attributes()
        .with_title("Porecatu")
        .with_window_icon(Some(app_icon::load()))
        .with_decorations(false);
    // macOS: decorations volta a `true` -- o semáforo nativo (traffic
    // lights) é o controle de janela do ADR-0027 lá, não botões nossos
    // (ver `chrome::paint`/`tab_bar::left_inset`).
    // `with_titlebar_transparent`/`with_title_hidden`/
    // `with_fullsize_content_view` estendem nosso conteúdo por baixo da
    // titlebar nativa (que fica invisível, exceto o semáforo) -- é o
    // mesmo truque de app nativo "sem titlebar visível, com semáforo",
    // não decorations=false.
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attributes = attributes
            .with_decorations(true)
            .with_titlebar_transparent(true)
            .with_title_hidden(true)
            .with_fullsize_content_view(true);
    }
    attributes
}

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

/// Grade mínima -- uma célula em cada direção, no pior caso de janela
/// minúscula ou métrica de fonte falhando.
const MIN_GRID: usize = 1;

/// Evento de usuário do event loop -- o mesmo caminho serve os dois fatos
/// que só podem chegar de fora da main thread (ADR-0007): aba suja e
/// recarga de config pronta.
#[derive(Debug, Clone, PartialEq)]
enum Wakeup {
    /// Uma aba ficou suja e precisa de redraw. Carrega `(WindowId, TabId)`
    /// desde a F1 (ADR-0015): com mais de uma janela, `TabId` sozinho não
    /// diz qual `Workspace` sujou -- os contadores são por janela.
    TabDirty { window: WindowId, tab: TabId },
    /// A thread do watcher (F4 etapa 4, ADR-0030) já leu e parseou o
    /// arquivo -- nunca um caminho para a main thread abrir. `Box` porque
    /// `ConfigReload::Loaded` carrega uma `Config` inteira, bem maior que
    /// `TabDirty`.
    ConfigReloaded(Box<ConfigReload>),
}

/// Estado de execução de uma aba: o `Terminal` (motor+PTY+threads) e o
/// snapshot reusado entre frames. Vive fora de `porecatu_core::Tab`, que é
/// domínio puro sem I/O (docs/arquitetura.md seção 4) -- a fronteira entre
/// os dois é exatamente `TabId`.
struct TabRuntime {
    terminal: Terminal,
    snapshot: GridSnapshot,
    /// `cwd` já resolvido (RF-3.10) que foi de fato usado no `SpawnConfig`
    /// desta aba -- terceiro degrau da precedência do ADR-0038 §2 (depois
    /// de `Tab::cwd`/OSC 7 e de `Terminal::cwd_fallback`/`ProcessGroup`),
    /// consultado só na gravação da sessão. Guardado aqui porque
    /// `porecatu_core::Tab` não guarda o `cwd` de spawn -- só o que OSC 7
    /// reporta.
    spawn_cwd: Option<PathBuf>,
    /// `true` depois do primeiro `TermEvent::Cwd` desta instância (RF-3.1,
    /// ADR-0039 §2). Diferente de `porecatu_core::Tab::cwd().is_some()`:
    /// uma aba nova herda `cwd` do grupo (ADR-0017 item 1) sem nunca ter
    /// recebido OSC 7 -- este campo só o sinal de verdade, nunca herdado.
    received_osc7: bool,
    /// Instante do spawn -- base do gatilho temporal do convite no Windows
    /// (ADR-0039 §2: "não há fallback lá, então o gatilho é o tempo").
    /// Só existe no Windows porque só lá o gatilho é temporal; fora dele o
    /// gatilho é o fallback do ADR-0038, consultado na gravação.
    #[cfg(windows)]
    spawned_at: Instant,
}

/// Altura da barra de abas, em pixels lógicos -- não depende de estado de
/// janela nenhum, só do estilo (`TabBarStyle::from_config`, um por
/// processo em `App::style`).
fn bar_height(style: &TabBarStyle) -> f32 {
    chrome::bar_height(style)
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

/// `[general] startup_directory` (RF-1.1) -- `"home"` cai em
/// `dirs::home_dir()` (mesmo fallback de antes desta etapa); qualquer outro
/// valor é o caminho literal. Sem validação de existência/absolutidade
/// aqui: um caminho ruim vira "aba nova sem `cwd`" no pior caso (o SO
/// decide, tipicamente o diretório do processo), não um crash.
fn resolve_startup_directory(value: &str) -> Option<PathBuf> {
    if value == "home" {
        dirs::home_dir()
    } else {
        Some(PathBuf::from(value))
    }
}

/// Converte `[terminal.cursor] shape` (`porecatu_config::CursorShape`,
/// RF-5.22) para o enum equivalente do snapshot (`porecatu_term::
/// CursorShape`) -- a mesma tradução de "vocabulário da config" para
/// "vocabulário do motor" que `TabBarStyle::from_config`/`ResolvedPalette::
/// from_config` fazem para chrome.
fn cursor_shape_from_config(shape: porecatu_config::CursorShape) -> porecatu_term::CursorShape {
    match shape {
        porecatu_config::CursorShape::Block => porecatu_term::CursorShape::Block,
        porecatu_config::CursorShape::Beam => porecatu_term::CursorShape::Beam,
        porecatu_config::CursorShape::Underline => porecatu_term::CursorShape::Underline,
    }
}

/// Monta `TermParams` a partir de `Config` -- `porecatu-term` nunca importa
/// `porecatu-config` (docs/arquitetura.md seção 4.2), então esta tradução
/// mora aqui, não lá. Chamado uma vez em `App::new`; hot reload (F4 etapa 4)
/// reconstrói e reaplica.
fn term_params_from_config(config: &porecatu_config::Config) -> TermParams {
    let scrollback = &config.terminal.scrollback;
    let selection = &config.terminal.selection;
    let cursor = &config.terminal.cursor;
    let clipboard = &config.terminal.clipboard;
    TermParams {
        scrollback_lines: scrollback.lines as usize,
        word_separators: selection.word_separators.clone(),
        default_cursor_shape: cursor_shape_from_config(cursor.shape),
        cursor_blinking: cursor.blink,
        osc52_read: clipboard.osc52_read,
        osc52_write: clipboard.osc52_write,
        clipboard_write_max_bytes: clipboard.osc52_max_bytes as usize,
    }
}

/// Cor do grupo da aba ativa, se ela tiver uma -- usado por `[terminal.
/// cursor] follows_group_color` (RF-5.22 comentário do TOML: "cursor e
/// prompt assumem a cor do grupo da aba"). `None` sobre run implícito (sem
/// `GroupColor`) ou sem aba ativa -- quem chama cai para `term_pal.cursor`.
fn active_group_color(workspace: &Workspace, pal: &palette::ResolvedPalette) -> Option<Color> {
    let tab = workspace.active_tab()?;
    let group_id = workspace.group_of_tab(tab)?;
    let color = workspace.group(group_id)?.color()?;
    Some(pal.group_color(color))
}

/// Monta `FontFamilies` a partir de `Config` -- `porecatu-render` não pode
/// depender de `porecatu-config` (regra de dependência, CLAUDE.md), então a
/// família desce como parâmetro simples. `mono` vem de `[terminal.font]`
/// (RF-5.1/5.2/5.6); `sans` de `[appearance.tabs]` (RF-4.10) -- por default
/// o mesmo valor (ADR-0026), mas a config permite divergir.
fn font_families_from_config(config: &porecatu_config::Config) -> porecatu_render::FontFamilies {
    let font = &config.terminal.font;
    porecatu_render::FontFamilies {
        mono: font.family.clone(),
        mono_fallback: font.fallback.clone(),
        sans: config.appearance.tabs.font_family.clone(),
        mono_letter_spacing_em: font.letter_spacing as f32,
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
    /// `config.reload` (ADR-0003, F4 etapa 5): o `Arc<Config>` e o watcher
    /// são do processo, não da janela -- só `App` sabe relê-lo.
    ReloadConfig,
    /// `font.increase`/`decrease`/`reset` (RF-5.9, F4 etapa 6): a métrica
    /// de célula é do processo (ADR-0015), então só `App` recalcula e
    /// redimensiona todos os PTYs.
    Zoom(Zoom),
    /// `theme.cycle` (RF-5.21, F4 etapa 6): o ciclo é do processo
    /// (ADR-0031 §4), não da janela.
    CycleTheme,
    Unhandled,
}

/// As três ações de zoom (RF-5.9) -- `font.increase`/`decrease`/`reset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zoom {
    Increase,
    Decrease,
    Reset,
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
    /// Botão de fechar da janela (ADR-0027): precisa do diálogo de
    /// confirmação com múltiplas abas (`App::request_close_window`), que
    /// vive em `App` -- `WindowState` não pode resolver isto sozinho, mesmo
    /// motivo de `WindowEmptied`.
    CloseWindowRequested,
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
    /// Menu de contexto do terminal (PRD-011 RF-11.14, ADR-0042 §9) --
    /// mesma exclusão mútua de `close_all_popovers`.
    terminal_context_menu: Option<TerminalContextMenu>,
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
    /// Último clique na drag region da barra (ADR-0027), pra detectar o
    /// duplo clique que maximiza/restaura -- mesmo padrão de
    /// `last_pill_click`, mas sem `GroupId`: a drag region não tem alvo,
    /// só posição no tempo. Resolvido no *press*, não no *release* --
    /// `Window::drag_window()` entrega o gesto ao loop modal não-client
    /// do SO assim que chamado, sem garantia de vermos o
    /// `MouseInput::Released` de volta depois.
    last_titlebar_click: Option<Instant>,
    /// `WindowEvent::Focused` -- RF-5.24 (`unfocused_hollow`): o cursor sai
    /// vazado quando a janela não tem foco. `true` no nascimento: `winit`
    /// não garante um `Focused(true)` inicial em toda plataforma, e a
    /// janela recém-criada tipicamente já nasce em primeiro plano.
    focused: bool,
    /// RF-3.2 (ADR-0036): `true` quando alguma mudança estrutural desta
    /// janela ainda não entrou no debounce de gravação da sessão (`App::
    /// session`). Drenado a cada volta do event loop em
    /// `schedule_next_wake` -- nunca lido fora dali. `App` decide se
    /// aquilo vira de fato um agendamento (RF-3.6, `enabled = false`
    /// nunca marca dirty no scheduler); esta janela só registra o fato.
    session_dirty: bool,
    /// Barra de busca (ADR-0041), o sexto widget de chrome -- estado
    /// efêmero de janela, amarrado à aba em que foi aberta (mesma
    /// classificação de `selection`, ADR-0041 §8). `redraw` fecha sozinha
    /// se a aba ativa mudar.
    search: Option<search_bar::SearchBarState>,
}

/// RF-1.6 (ADR-0017, ADR-0034): decide se `tab.close`/o botão de fechar
/// devem confirmar antes de fechar -- dois sinais em OU, ambos atrás da
/// config `confirm_close_with_process`. Função livre e pura (sem
/// `Terminal`/`WindowState`), testável isoladamente nos quatro
/// quadrantes de `TermModes`/`has_extra_processes`.
fn should_confirm_tab_close(
    confirm_close_with_process: bool,
    modes: TermModes,
    has_extra_processes: bool,
) -> bool {
    confirm_close_with_process
        && (modes.alt_screen
            || modes.mouse_reporting != MouseReporting::None
            || has_extra_processes)
}

/// ADR-0034 §6, ADR-0037 §3: decide se `window.close`/`app.quit` devem
/// confirmar. `significant_tab_count` já vem contado sem aba `NotStarted`
/// (`request_close_window` conta `state.tabs`, o mapa de `TabRuntime` --
/// função livre e pura, testável sem `WindowState`.
fn window_close_needs_confirmation(significant_tab_count: usize, any_tab_busy: bool) -> bool {
    significant_tab_count > 1 || any_tab_busy
}

/// RF-3.14/RF-3.16 (ADR-0036 §5): texto do aviso de recuperação de sessão
/// (canal 1, ADR-0014) para cada `Notice` que `porecatu_session::load`
/// pode devolver -- puro, sem `WindowState`, só a tradução de dado para
/// texto.
fn session_notice_text(notice: &porecatu_session::Notice) -> (&'static str, String) {
    match notice {
        porecatu_session::Notice::Corrupt(path) => (
            "Sessão anterior corrompida",
            format!(
                "O arquivo foi preservado em \"{}\". Uma sessão nova foi iniciada.",
                path.display()
            ),
        ),
        porecatu_session::Notice::NewerSchema { found, supported } => (
            "Sessão de uma versão mais nova",
            format!(
                "O formato salvo (versão {found}) é mais novo que o suportado por esta versão do Porecatu (até {supported}). Uma sessão nova foi iniciada."
            ),
        ),
    }
}

#[cfg(test)]
mod session_notice_text_tests {
    use super::*;

    #[test]
    fn corrupt_notice_names_the_preserved_path() {
        let (title, body) = session_notice_text(&porecatu_session::Notice::Corrupt(
            "session.json.corrupt".into(),
        ));
        assert_eq!(title, "Sessão anterior corrompida");
        assert!(body.contains("session.json.corrupt"));
    }

    #[test]
    fn newer_schema_notice_names_both_versions() {
        let (title, body) = session_notice_text(&porecatu_session::Notice::NewerSchema {
            found: 99,
            supported: 1,
        });
        assert_eq!(title, "Sessão de uma versão mais nova");
        assert!(body.contains("99"));
        assert!(body.contains('1'));
    }
}

#[cfg(test)]
mod window_close_needs_confirmation_tests {
    use super::*;

    #[test]
    fn single_quiet_tab_never_confirms() {
        assert!(!window_close_needs_confirmation(1, false));
    }

    /// ADR-0037 §3: janela só com abas `NotStarted` tem contagem
    /// significativa zero -- não confirma por "mais de uma aba", mesmo
    /// que o `Workspace` tenha dez.
    #[test]
    fn zero_significant_tabs_never_confirms_by_count() {
        assert!(!window_close_needs_confirmation(0, false));
    }

    #[test]
    fn more_than_one_significant_tab_confirms() {
        assert!(window_close_needs_confirmation(2, false));
    }

    #[test]
    fn a_single_busy_tab_confirms() {
        assert!(window_close_needs_confirmation(1, true));
    }
}

/// RF-3.1 (ADR-0039 §4): três desligamentos independentes decidem se vale
/// a pena procurar o gatilho -- dispensa definitiva, convite já escolhido
/// nesta execução ("uma vez"), ou a config desligando antes de qualquer
/// detecção. Função livre e pura, testável sem `App`.
fn shell_integration_invite_eligible(
    dismissed: bool,
    claimed: bool,
    suggest_enabled: bool,
) -> bool {
    suggest_enabled && !dismissed && !claimed
}

/// RF-3.1 (ADR-0038 §5, ADR-0039 §2): gatilho fora do Windows. Uma aba
/// que já recebeu OSC 7 nunca dispara, mesmo que o fallback (que quem
/// chama já pode ter consultado) diga outra coisa -- o sinal de verdade é
/// `received_osc7`, não o valor do fallback em si. Sem OSC 7, um
/// fallback que devolve um `cwd` diferente do de spawn prova que o
/// diretório mudou e o app só soube pelo caminho caro.
fn fallback_reveals_missing_osc7(
    received_osc7: bool,
    fallback: Option<&std::path::Path>,
    spawn_cwd: Option<&std::path::Path>,
) -> bool {
    !received_osc7 && fallback.is_some_and(|f| Some(f) != spawn_cwd)
}

/// RF-3.1 (ADR-0039 §2), gatilho temporal do Windows: a aba ficou
/// interativa (`spawned_at`) por `interval` sem nenhum `TermEvent::Cwd`.
/// Só existe no Windows -- fora dele o gatilho é
/// `fallback_reveals_missing_osc7`, e esta função ficaria sem chamador
/// (`dead_code` do clippy, que só aparece no CI de Linux/macOS).
#[cfg(windows)]
fn windows_invite_timeout_due(
    received_osc7: bool,
    spawned_at: Instant,
    now: Instant,
    interval: Duration,
) -> bool {
    !received_osc7 && now >= spawned_at + interval
}

/// RF-3.1 (ADR-0039 §1): "não escrever com a tela alternativa ativa" --
/// só essas duas saídas, sem meio-termo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellIntegrationNoteTiming {
    WriteNow,
    Defer,
}

fn shell_integration_note_timing(alt_screen_active: bool) -> ShellIntegrationNoteTiming {
    if alt_screen_active {
        ShellIntegrationNoteTiming::Defer
    } else {
        ShellIntegrationNoteTiming::WriteNow
    }
}

#[cfg(test)]
mod shell_integration_trigger_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn eligible_when_all_three_gates_are_open() {
        assert!(shell_integration_invite_eligible(false, false, true));
    }

    #[test]
    fn dismissed_forever_is_never_eligible_again() {
        assert!(!shell_integration_invite_eligible(true, false, true));
    }

    /// RF-3.1: "uma vez por execução, não por aba" -- depois de escolhido,
    /// nenhuma outra aba dispara de novo nesta execução.
    #[test]
    fn already_claimed_this_run_is_not_eligible_again() {
        assert!(!shell_integration_invite_eligible(false, true, true));
    }

    #[test]
    fn suggest_shell_integration_false_disables_before_any_detection() {
        assert!(!shell_integration_invite_eligible(false, false, false));
    }

    /// Este é o teste mais importante da etapa: quem já tem OSC 7 nunca
    /// dispara, mesmo que o fallback (chamado por quem quer que seja)
    /// devolva outra coisa.
    #[test]
    fn a_tab_with_osc7_never_triggers_via_the_fallback() {
        assert!(!fallback_reveals_missing_osc7(
            true,
            Some(Path::new("/outro")),
            Some(Path::new("/spawn")),
        ));
    }

    #[test]
    fn fallback_matching_spawn_cwd_does_not_trigger() {
        assert!(!fallback_reveals_missing_osc7(
            false,
            Some(Path::new("/home/ana")),
            Some(Path::new("/home/ana")),
        ));
    }

    #[test]
    fn fallback_diverging_from_spawn_cwd_triggers() {
        assert!(fallback_reveals_missing_osc7(
            false,
            Some(Path::new("/home/ana/projeto")),
            Some(Path::new("/home/ana")),
        ));
    }

    #[test]
    fn no_fallback_available_does_not_trigger() {
        assert!(!fallback_reveals_missing_osc7(
            false,
            None,
            Some(Path::new("/home/ana"))
        ));
    }

    #[test]
    #[cfg(windows)]
    fn windows_timeout_does_not_fire_before_the_interval() {
        let spawned_at = Instant::now();
        let now = spawned_at + Duration::from_secs(1);
        assert!(!windows_invite_timeout_due(
            false,
            spawned_at,
            now,
            Duration::from_secs(3)
        ));
    }

    #[test]
    #[cfg(windows)]
    fn windows_timeout_fires_once_the_interval_elapses() {
        let spawned_at = Instant::now();
        let now = spawned_at + Duration::from_secs(3);
        assert!(windows_invite_timeout_due(
            false,
            spawned_at,
            now,
            Duration::from_secs(3)
        ));
    }

    /// Aba que recebeu OSC 7 entre o agendamento e o despertar não
    /// dispara mais, mesmo já vencido o intervalo.
    #[test]
    #[cfg(windows)]
    fn windows_timeout_never_fires_once_osc7_arrived() {
        let spawned_at = Instant::now();
        let now = spawned_at + Duration::from_secs(999);
        assert!(!windows_invite_timeout_due(
            true,
            spawned_at,
            now,
            Duration::from_secs(3)
        ));
    }

    #[test]
    fn nothing_is_written_while_the_alternate_screen_is_active() {
        assert_eq!(
            shell_integration_note_timing(true),
            ShellIntegrationNoteTiming::Defer
        );
    }

    #[test]
    fn the_note_writes_right_away_on_the_primary_screen() {
        assert_eq!(
            shell_integration_note_timing(false),
            ShellIntegrationNoteTiming::WriteNow
        );
    }
}

#[cfg(test)]
mod should_confirm_tab_close_tests {
    use super::*;

    fn quiet_modes() -> TermModes {
        TermModes::default()
    }

    #[test]
    fn quiet_terminal_without_extra_process_never_confirms() {
        assert!(!should_confirm_tab_close(true, quiet_modes(), false));
    }

    #[test]
    fn alt_screen_confirms() {
        let modes = TermModes {
            alt_screen: true,
            ..quiet_modes()
        };
        assert!(should_confirm_tab_close(true, modes, false));
    }

    #[test]
    fn extra_process_confirms_even_without_alt_screen() {
        assert!(should_confirm_tab_close(true, quiet_modes(), true));
    }

    #[test]
    fn both_signals_together_still_confirms_once() {
        let modes = TermModes {
            alt_screen: true,
            ..quiet_modes()
        };
        assert!(should_confirm_tab_close(true, modes, true));
    }

    #[test]
    fn config_disabled_suppresses_both_signals() {
        let modes = TermModes {
            alt_screen: true,
            ..quiet_modes()
        };
        assert!(!should_confirm_tab_close(false, modes, true));
    }
}

impl WindowState {
    fn new(window: Arc<Window>, window_surface: WindowSurface, scale: f32) -> Self {
        let size = window.inner_size();
        Self {
            window,
            window_surface,
            scale,
            focused: true,
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
            terminal_context_menu: None,
            group_editor: None,
            move_to_group: None,
            last_pill_click: None,
            selection: Selection::default(),
            last_titlebar_click: None,
            session_dirty: false,
            search: None,
        }
    }

    /// Ponto único de decisão de RF-3.2 para operações de `Workspace`: os
    /// chamadores de método mutante passam por aqui em vez de cada um
    /// decidir se a operação conta como mudança estrutural. Ativar aba
    /// conta (RF-3.2, "trocar a aba ativa"), então até `next_tab`/
    /// `prev_tab`/`activate_navigable_index` passam por cá -- não só
    /// `close_tab`/`group_tabs` e afins.
    fn touch_workspace<T>(&mut self, f: impl FnOnce(&mut Workspace) -> T) -> T {
        let result = f(&mut self.workspace);
        self.session_dirty = true;
        result
    }

    /// Segundo (e último) ponto de entrada de RF-3.2, para o que muda a
    /// sessão sem passar por um método de `Workspace`: título customizado
    /// via `tab_mut` (`commit_rename`), `cwd`/`Exited` via `TermEvent`
    /// (`App::user_event`), e geometria da janela (`WindowEvent::Moved`/
    /// `Resized`, `App::window_event`).
    fn mark_session_dirty(&mut self) {
        self.session_dirty = true;
    }

    /// Fecha os quatro widgets da camada popover (ADR-0023: "abrir
    /// qualquer um dos dois fecha o outro, num ponto só do código") --
    /// chamado antes de abrir qualquer um deles, garantindo que nunca
    /// coexistem.
    fn close_all_popovers(&mut self) {
        self.context_menu = None;
        self.group_context_menu = None;
        self.terminal_context_menu = None;
        self.group_editor = None;
        self.move_to_group = None;
    }

    /// Informa o resultado de agir sobre um hyperlink (RF-11.12/RF-11.13)
    /// pelo canal 1 de aviso (ADR-0014) -- abrir/revelar com sucesso é
    /// silencioso (o handler do sistema ou o gerenciador de arquivos já é
    /// a confirmação visível); as três outras saídas precisam de aviso,
    /// porque senão o clique parece não ter feito nada. Recusa e falha ao
    /// revelar copiam o URI (ADR-0042 §4, mesma vala do esquema recusado
    /// -- "nunca cai no handler por extensão como fallback").
    fn report_link_outcome(&mut self, outcome: hyperlink::LinkOutcome, uri: &str, now: Instant) {
        match outcome {
            hyperlink::LinkOutcome::Opened | hyperlink::LinkOutcome::Revealed => {}
            hyperlink::LinkOutcome::OpenFailed => {
                self.warnings.push(
                    Severity::Warning,
                    "Não foi possível abrir o link",
                    uri.to_string(),
                    now,
                );
            }
            hyperlink::LinkOutcome::RevealFailed => {
                clipboard::copy(uri);
                self.warnings.push(
                    Severity::Warning,
                    "Não foi possível revelar o arquivo",
                    format!("URI copiado: {uri}"),
                    now,
                );
            }
            hyperlink::LinkOutcome::Refused { normalized_scheme } => {
                clipboard::copy(uri);
                self.warnings.push(
                    Severity::Info,
                    "Esquema recusado",
                    format!("`{normalized_scheme}` não abre -- URI copiado"),
                    now,
                );
            }
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
    fn grid_size(&self, cell_metrics: CellMetrics, style: &TabBarStyle) -> (usize, usize) {
        let content = paint::terminal_content_rect(
            style,
            bar_height(style),
            self.logical_width,
            self.logical_height,
        );
        let cols = ((content.width / cell_metrics.width) as usize).max(MIN_GRID);
        let rows = ((content.height / cell_metrics.height) as usize).max(MIN_GRID);
        (rows, cols)
    }

    /// Nome de exibição do shell (fallback de última instância da
    /// precedência de título, RF-1.7/ADR-0017) -- `[shell] program`
    /// presente vence (mesma precedência de `SpawnConfig.program`); vazio
    /// cai na mesma resolução que `porecatu_pty::spawn` usa. Reduzido ao
    /// nome-base (sem caminho nem extensão) nos dois casos.
    fn shell_display_name(shell: &porecatu_config::Shell) -> String {
        let resolved = if shell.program.is_empty() {
            resolve_default_shell(std::env::var("SHELL").ok().as_deref(), search_path)
        } else {
            shell.program.clone()
        };
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
    fn ensure_active_tab_visible(&mut self, gpu: &mut GpuContext, style: &TabBarStyle) {
        let Some(active) = self.workspace.active_tab() else {
            return;
        };
        let layout = tab_bar::fit_width(
            &self.workspace,
            style,
            self.logical_width,
            gpu.text_measurer(),
            is_macos(),
        );
        let Some(rect) = tab_bar::tab_rect(&layout, active) else {
            return;
        };
        let trilha_width = tab_bar::trilha_width(style, self.logical_width, is_macos());
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
    fn group_pill_screen_x(
        &self,
        group: GroupId,
        gpu: &mut GpuContext,
        style: &TabBarStyle,
    ) -> f32 {
        let layout = tab_bar::fit_width(
            &self.workspace,
            style,
            self.logical_width,
            gpu.text_measurer(),
            is_macos(),
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
    /// `Terminal` dela via [`Self::spawn_tab_runtime`]. Aba nova nasce
    /// sempre `Running` (ADR-0037 §1) -- `spawn_tab_runtime` não muda
    /// estado nenhum, só cria o `TabRuntime`.
    #[allow(clippy::too_many_arguments)]
    fn open_tab(
        &mut self,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        cwd: Option<PathBuf>,
        now: Instant,
        target: NewTabTarget,
        style: &TabBarStyle,
        term_params: &TermParams,
        shell: &porecatu_config::Shell,
        startup_directory: &Option<PathBuf>,
    ) {
        let shell_name = Self::shell_display_name(shell);
        let tab_id = match target {
            NewTabTarget::Group(id) => match self.workspace.group(id) {
                Some(g) => {
                    let pos = g.tabs().len();
                    self.touch_workspace(|ws| ws.new_tab(Some(id), shell_name, cwd.clone(), pos))
                }
                // Grupo que sumiu entre o clique e aqui: cai no caminho
                // de `tab.new`, não num run implícito novo no fim.
                None => self.touch_workspace(|ws| ws.append_tab(shell_name, cwd.clone())),
            },
            NewTabTarget::ActiveGroup => {
                self.touch_workspace(|ws| ws.append_tab(shell_name, cwd.clone()))
            }
            NewTabTarget::Ungrouped => {
                self.touch_workspace(|ws| ws.append_ungrouped_tab(shell_name, cwd.clone()))
            }
        };
        self.spawn_tab_runtime(
            tab_id,
            cwd,
            cell_metrics,
            proxy,
            now,
            style,
            term_params,
            shell,
            startup_directory,
        );
    }

    /// Spawna o `Terminal` de uma aba que já existe no `Workspace` --
    /// extraído de `open_tab` (que a cria e spawna no mesmo gesto) para
    /// servir também [`Self::ensure_active_tab_started`] (ADR-0037 §2),
    /// que spawna uma aba `NotStarted` já existente no primeiro foco, e a
    /// restauração de sessão (F5 etapa 4, `App::open_window_from_session`).
    /// Erro de spawn desfaz a aba do `Workspace` e vira aviso do app
    /// (canal 1, ADR-0014) -- sem isso ficaria uma aba sem `Terminal`
    /// nenhum atrás.
    ///
    /// RF-3.10 (ADR-0017 §5): o `cwd` é verificado **aqui**, no momento em
    /// que a aba sobe de verdade -- nunca adiantado para toda aba
    /// `NotStarted` de uma restauração, que seria N chamadas de `exists()`
    /// no caminho do start para abas que talvez nunca sejam focadas. Sem o
    /// diretório, a nota entra **antes** de qualquer byte do PTY chegar
    /// (grade ainda em branco: é o que garante "primeira linha", a
    /// posição que o ADR-0017 §5 exige para este fato).
    #[allow(clippy::too_many_arguments)]
    fn spawn_tab_runtime(
        &mut self,
        tab_id: TabId,
        cwd: Option<PathBuf>,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        now: Instant,
        style: &TabBarStyle,
        term_params: &TermParams,
        shell: &porecatu_config::Shell,
        startup_directory: &Option<PathBuf>,
    ) {
        let (rows, cols) = self.grid_size(cell_metrics, style);
        let window_id = self.window.id();
        let tab = tab_id;
        let proxy = proxy.clone();
        let cwd_exists = cwd.as_ref().is_none_or(|p| p.is_dir());
        let (cwd, missing_cwd_note) =
            session_writer::resolve_tab_cwd(cwd, cwd_exists, startup_directory);
        let pty_config = SpawnConfig {
            // Vazio = detecta automaticamente (`porecatu_pty::spawn` cai em
            // `resolve_default_shell`) -- `[shell] program` presente tem
            // precedência total (doc de `SpawnConfig`).
            program: (!shell.program.is_empty()).then(|| shell.program.clone()),
            args: shell.args.clone(),
            // `[shell.env]` é aplicado **depois** do ambiente base
            // (TERM/COLORTERM/...) por `porecatu_pty::spawn` -- a config do
            // usuário pode sobrescrevê-lo (ADR-0012).
            env: shell
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            cwd: cwd.clone(),
            size: PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            },
        };

        match Terminal::spawn(pty_config, term_params.clone(), move || {
            let _ = proxy.send_event(Wakeup::TabDirty {
                window: window_id,
                tab,
            });
        }) {
            Ok(terminal) => {
                if let Some(note) = &missing_cwd_note {
                    terminal.inject_note(note, palette::NOTE_ACCENT_RGB);
                }
                self.tabs.insert(
                    tab_id,
                    TabRuntime {
                        terminal,
                        snapshot: GridSnapshot::default(),
                        spawn_cwd: cwd,
                        received_osc7: false,
                        #[cfg(windows)]
                        spawned_at: now,
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
                self.touch_workspace(|ws| ws.close_tab(tab_id));
            }
        }
        self.sync_window_title();
    }

    /// ADR-0037 §2: o shell sobe no primeiro foco, por qualquer caminho de
    /// ativação -- chamado uma vez por evento de janela, no fim de
    /// [`App::window_event`], depois que qualquer clique/atalho já
    /// ativou a aba no `Workspace`. Sem-op na maioria absoluta das
    /// chamadas (aba ativa já `Running`, ou nenhuma aba ativa) -- só faz
    /// algo quando encontra `NotStarted`, o que a restauração de sessão
    /// (F5 etapa 4, `lazy_restore = true`) agora produz de verdade em toda
    /// aba que não é a ativa de cada janela.
    ///
    /// Roda **antes** de qualquer tecla chegar ao terminal porque é
    /// chamado no mesmo evento que ativou a aba, antes do próximo evento
    /// (`KeyboardInput` seguinte) poder rotear uma tecla para ela -- não
    /// existe janela de tempo em que a aba ativa esteja `NotStarted` e
    /// receba tecla (ADR-0037 §2, risco da tabela).
    #[allow(clippy::too_many_arguments)]
    fn ensure_active_tab_started(
        &mut self,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        now: Instant,
        style: &TabBarStyle,
        term_params: &TermParams,
        shell: &porecatu_config::Shell,
        startup_directory: &Option<PathBuf>,
    ) {
        let Some(id) = self.workspace.active_tab() else {
            return;
        };
        let Some(tab) = self.workspace.tab(id) else {
            return;
        };
        if !tab.is_not_started() {
            return;
        }
        let cwd = tab.cwd().cloned();
        self.spawn_tab_runtime(
            id,
            cwd,
            cell_metrics,
            proxy,
            now,
            style,
            term_params,
            shell,
            startup_directory,
        );
        // Formaliza a transição no modelo só depois do spawn de verdade --
        // se `spawn_tab_runtime` desfez a aba (erro), `tab_mut` abaixo
        // devolve `None` e não há nada a marcar.
        if let Some(tab) = self.workspace.tab_mut(id) {
            tab.start();
        }
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
        self.touch_workspace(|ws| ws.close_tab(id));
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
        let RenameState::Editing { tab, field } = std::mem::take(&mut self.rename) else {
            return;
        };
        let trimmed = field.text().trim();
        let title = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        if let Some(t) = self.workspace.tab_mut(tab) {
            t.set_custom_title(title);
            self.mark_session_dirty();
        }
        self.sync_window_title();
    }

    #[allow(clippy::too_many_arguments)]
    fn action_new_tab(
        &mut self,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        startup_directory: &Option<PathBuf>,
        now: Instant,
        style: &TabBarStyle,
        term_params: &TermParams,
        shell: &porecatu_config::Shell,
    ) {
        if self.rename.editing_tab().is_some() {
            self.commit_rename();
        }
        let cwd = self.resolve_new_tab_cwd(startup_directory);
        self.open_tab(
            cell_metrics,
            proxy,
            cwd,
            now,
            NewTabTarget::ActiveGroup,
            style,
            term_params,
            shell,
            startup_directory,
        );
    }

    /// RF-1.6 (ADR-0017, ADR-0034): fechar a aba ativa pede confirmação
    /// quando ela tem tela alternativa/reporte de mouse ligado, **ou**
    /// (Windows, ADR-0034) mais de um processo no Job do shell -- os dois
    /// sinais que o app observa sem varrer a árvore de processos. A chave
    /// `confirm_close_with_process` governa os dois juntos. Sem
    /// confirmação, fecha direto -- e se isso esvaziou a janela (pedido do
    /// usuário), quem chama fecha a janela.
    fn action_close_tab(&mut self, confirm_close_with_process: bool) -> Option<TabCloseOutcome> {
        let id = self.workspace.active_tab()?;
        // ADR-0037 §3: checagem antes da detecção do ADR-0034, não uma
        // exceção dentro dela. Aba `NotStarted` não tem `ProcessGroup` --
        // sem isto, `self.tabs.get(&id)?` abaixo (que só existe para aba
        // já spawnada) devolveria `None` e o fechamento não faria nada.
        if self.workspace.tab(id).is_some_and(|t| t.is_not_started()) {
            let window_empty = self.close_tab_unconditionally(id);
            return Some(TabCloseOutcome::Closed { window_empty });
        }
        let runtime = self.tabs.get(&id)?;
        if should_confirm_tab_close(
            confirm_close_with_process,
            runtime.terminal.modes(),
            runtime.terminal.has_extra_processes(),
        ) {
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
            field: TextFieldState::new(tab.title()),
        };
    }

    fn action_next_tab(&mut self, gpu: &mut GpuContext, style: &TabBarStyle) {
        self.touch_workspace(|ws| ws.next_tab());
        self.sync_window_title();
        self.ensure_active_tab_visible(gpu, style);
    }

    fn action_prev_tab(&mut self, gpu: &mut GpuContext, style: &TabBarStyle) {
        self.touch_workspace(|ws| ws.prev_tab());
        self.sync_window_title();
        self.ensure_active_tab_visible(gpu, style);
    }

    /// RF-2.21 (`group.next`/`group.prev`): anda de grupo em grupo,
    /// caindo na última aba visitada do destino. O modelo faz o trabalho
    /// (`Workspace::next_group`, ADR-0020 §6); aqui só sobra o que é de
    /// janela -- título e trazer a aba nova para a vista, como em
    /// `tab.next`/`tab.prev`.
    ///
    /// Não há animação: grupo colapsado é pulado, então nada expande nem
    /// colapsa, e a trilha não reflui (ADR-0022 fecha a lista de
    /// consumidores do relógio em dois).
    fn action_group_step(&mut self, delta: isize, gpu: &mut GpuContext, style: &TabBarStyle) {
        let moved = if delta >= 0 {
            self.touch_workspace(Workspace::next_group)
        } else {
            self.touch_workspace(Workspace::prev_group)
        };
        if moved.is_some() {
            self.sync_window_title();
            self.ensure_active_tab_visible(gpu, style);
        }
    }

    /// `tab.goto_N`: índice sobre a ordem **navegável**, não a visual --
    /// aba de grupo colapsado sai da numeração, e colapsar renumera
    /// `Alt+1..9` (deliberado, ADR-0020 §2).
    fn action_goto(&mut self, navigable_index: usize, gpu: &mut GpuContext, style: &TabBarStyle) {
        self.touch_workspace(|ws| ws.activate_navigable_index(navigable_index));
        self.sync_window_title();
        self.ensure_active_tab_visible(gpu, style);
    }

    /// RF-1.17: reordenação por teclado, uma posição por vez, dentro do
    /// próprio grupo (`Workspace::move_tab` nunca move entre grupos --
    /// isso é o arraste/`tab.move_to_group` da etapa 6). Por isso a
    /// posição-alvo vem da ordem *dentro do grupo* da aba ativa, não da
    /// ordem visual da janela inteira -- com mais de um grupo (F3) as duas
    /// divergem.
    fn action_move_tab(&mut self, delta: isize, gpu: &mut GpuContext, style: &TabBarStyle) {
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
        self.touch_workspace(|ws| ws.move_tab(active, target));
        self.ensure_active_tab_visible(gpu, style);
    }

    /// Passo 1 do ADR-0008 (modo de captura): consome tudo, exceto `Esc` e
    /// `Enter` (que confirmam/cancelam), sem repassar nem pro roteamento de
    /// keybind nem pro terminal. `Ctrl+A`/`Cmd+A`, `Shift+seta`,
    /// `Home`/`End` e o texto digitado (com substituição de seleção) vêm
    /// de `apply_text_field_key` (ADR-0035), compartilhado com o editor de
    /// grupo.
    fn handle_rename_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        match &event.logical_key {
            Key::Named(NamedKey::Enter) => self.commit_rename(),
            Key::Named(NamedKey::Escape) => self.rename = RenameState::Idle,
            Key::Named(NamedKey::Backspace) => self.rename.backspace(),
            _ => {
                if let RenameState::Editing { field, .. } = &mut self.rename {
                    apply_text_field_key(
                        field,
                        &event.logical_key,
                        event.text.as_deref(),
                        self.modifiers,
                    );
                }
            }
        }
    }

    /// Captura parcial da busca (ADR-0041 §3): `true` se a tecla foi
    /// reivindicada (quem chama redesenha e para por aqui); `false` deixa
    /// cair pro passo 2 da cadeia -- é o que mantém `F3`/`Ctrl+Shift+F`/
    /// trocar de aba/fechar a janela funcionando com a busca aberta.
    /// `Backspace` entra por fora de `apply_text_field_key` (que não o
    /// trata -- mesma nota de `handle_rename_key`), mas ainda é uma das
    /// "teclas de edição do ADR-0035" que o ADR-0041 §3 reivindica por
    /// categoria.
    fn handle_search_key(&mut self, event: &KeyEvent, cell_metrics: CellMetrics) -> bool {
        if event.state != ElementState::Pressed {
            return false;
        }
        let Some(tab) = self.search.as_ref().map(SearchBarState::tab) else {
            return false;
        };
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.search = None;
                true
            }
            Key::Named(NamedKey::Enter) => {
                let forward = !self.modifiers.shift;
                self.advance_search(tab, forward, cell_metrics);
                true
            }
            Key::Named(NamedKey::Backspace) => {
                if let Some(search) = self.search.as_mut() {
                    search.field_mut().backspace();
                }
                if let Some(rt) = self.tabs.get(&tab)
                    && let Some(search) = self.search.as_mut()
                {
                    search.restart(&rt.terminal, DEFAULT_SEARCH_LINES_PER_STEP);
                }
                true
            }
            _ => {
                let modifiers = self.modifiers;
                let mut consumed = false;
                if let Some(search) = self.search.as_mut() {
                    consumed = apply_text_field_key(
                        search.field_mut(),
                        &event.logical_key,
                        event.text.as_deref(),
                        modifiers,
                    );
                }
                if consumed
                    && let Some(rt) = self.tabs.get(&tab)
                    && let Some(search) = self.search.as_mut()
                {
                    search.restart(&rt.terminal, DEFAULT_SEARCH_LINES_PER_STEP);
                }
                consumed
            }
        }
    }

    /// `search.next`/`search.prev` (RF-11.5), e `Enter`/`Shift+Enter` que
    /// fazem o mesmo sem binding (ADR-0041 §3/§10) -- avança a ocorrência
    /// ativa e rola até ela, reservando a altura da barra (ADR-0041 §1) e
    /// expandindo o grupo se a aba estiver num colapsado (RF-11.9/RF-2.17,
    /// `Workspace::activate_tab` já carrega a regra).
    fn advance_search(&mut self, tab: TabId, forward: bool, cell_metrics: CellMetrics) {
        let Some(search) = self.search.as_mut() else {
            return;
        };
        let Some(occurrence) = search.advance(forward) else {
            return;
        };
        let Some(rt) = self.tabs.get(&tab) else {
            return;
        };
        let reserved = search_bar::reserved_rows(cell_metrics.height);
        let delta = search_bar::scroll_delta_to_reveal(
            occurrence.start.line,
            rt.snapshot.scroll_offset,
            rt.snapshot.rows,
            reserved,
        );
        if delta != 0 {
            rt.terminal.scroll(TermScroll::Lines(delta));
        }
        self.touch_workspace(|ws| {
            ws.activate_tab(tab);
        });
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

    /// Espelha `handle_group_menu_key`, sobre `terminal_context_menu`.
    /// Recebe `style`/`cell_metrics` de fora -- `WindowState` não os guarda
    /// -- para recomputar a lista de itens a cada navegação
    /// (`terminal_menu_context_items`, mesmo padrão de `group_action_items`
    /// no clique/hover do menu de grupo).
    fn handle_terminal_menu_key(
        &mut self,
        event: &KeyEvent,
        style: &TabBarStyle,
        cell_metrics: CellMetrics,
    ) -> Option<TerminalMenuAction> {
        if event.state != ElementState::Pressed {
            return None;
        }
        let Some(menu) = &self.terminal_context_menu else {
            return None;
        };
        let items = terminal_menu_context_items(
            &self.tabs,
            style,
            cell_metrics,
            self.logical_width,
            self.logical_height,
            self.scale,
            menu.tab,
            menu.anchor,
        );
        let menu = self.terminal_context_menu.as_mut()?;
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.terminal_context_menu = None;
                None
            }
            Key::Named(NamedKey::ArrowUp) => {
                menu.move_highlight(-1, &items);
                None
            }
            Key::Named(NamedKey::ArrowDown) => {
                menu.move_highlight(1, &items);
                None
            }
            Key::Named(NamedKey::Enter) => {
                let action = menu.selected(&items);
                self.terminal_context_menu = None;
                action
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
        let modifiers = self.modifiers;
        let Some(editor) = &mut self.group_editor else {
            return GroupEditorOutcome::None;
        };
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.group_editor = None;
                GroupEditorOutcome::None
            }
            Key::Named(NamedKey::Tab) => {
                editor.cycle_focus(!modifiers.shift);
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
                    self.touch_workspace(|ws| ws.rename_group(group, name));
                    self.group_editor = None;
                    GroupEditorOutcome::None
                }
                EditorRegion::Swatches => {
                    let group = editor.group;
                    let color = GroupColor::ALL[editor.swatch_highlight()];
                    self.touch_workspace(|ws| ws.set_group_color(group, color));
                    GroupEditorOutcome::None
                }
                EditorRegion::Actions => {
                    GroupEditorOutcome::Action(editor.group, editor.selected_action())
                }
            },
            _ => {
                if let Some(field) = editor.name_field_mut() {
                    apply_text_field_key(
                        field,
                        &event.logical_key,
                        event.text.as_deref(),
                        modifiers,
                    );
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
                self.touch_workspace(|ws| ws.move_tab_to_group(tab, group));
            }
            MoveTarget::NewGroup => {
                let color = self.workspace.next_auto_color();
                self.touch_workspace(|ws| ws.group_tabs(&[tab], "Novo grupo", color));
            }
        }
    }

    /// Passo 2 do ADR-0008 para as ações de aba/janela -- defaults fixos
    /// de Windows/Linux (não há parser de `[keybindings]` até a F4,
    /// docs/reference/acoes.md). `window.new`/`window.close` (ADR-0015)
    /// devolvem `ActionOutcome` distinto porque tocam outras janelas, algo
    /// que só `App` resolve.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    /// Consulta `keymap` (ADR-0029) em vez do `match` fixo que existia até
    /// a F4 etapa 5 -- a cadeia de captura do `dispatch_keyboard_input`
    /// não muda, só esta etapa dela. `Chord::from_key` devolve `None` para
    /// o que não está no vocabulário da gramática (composição de IME
    /// multi-caractere, teclas de mídia); nesse caso, como para qualquer
    /// tecla sem binding, a tecla segue pro terminal (`ActionOutcome::
    /// Unhandled`).
    ///
    /// `scrollback.*` e `clipboard.*`/`selection.*` resolvem no mapa (o
    /// usuário pode remapeá-los e ver o resultado em `keymap`), mas quem
    /// os executa continua sendo `input.rs`, sem mudança nesta etapa
    /// (ver a nota do arquivo de exemplo). Devolver `Unhandled` para eles
    /// aqui é o que deixa `input::handle_keyboard_input` rodar como
    /// sempre rodou -- **não inverte** a ordem que a armadilha do
    /// ADR-0008 exige entre `ctrl+shift+pagedown` (`group.next`, tratado
    /// aqui) e `shift+pagedown` (`scrollback.page_down`, tratado lá).
    #[allow(clippy::too_many_arguments)]
    fn handle_tab_action_key(
        &mut self,
        event: &KeyEvent,
        gpu: &mut GpuContext,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        startup_directory: &Option<PathBuf>,
        now: Instant,
        style: &TabBarStyle,
        term_params: &TermParams,
        shell: &porecatu_config::Shell,
        keymap: &HashMap<Chord, Action>,
        confirm_close_with_process: bool,
    ) -> ActionOutcome {
        if event.state != ElementState::Pressed {
            return ActionOutcome::Unhandled;
        }
        let Some(chord) = Chord::from_key(&event.logical_key, self.modifiers) else {
            return ActionOutcome::Unhandled;
        };
        let Some(action) = keymap.get(&chord).copied() else {
            return ActionOutcome::Unhandled;
        };

        // As cinco ações de nível de grupo compartilham o mesmo alvo
        // (`group_menu::keyboard_target`, ADR-0020/ADR-0023) e o mesmo
        // executor (`run_group_action`, que também atende o menu de
        // contexto e o editor -- uma lista de ações só, ADR-0014).
        let group_action = match action {
            Action::GroupDissolve => Some(GroupAction::Dissolve),
            Action::GroupRename => Some(GroupAction::Rename),
            Action::GroupToggleCollapse => Some(GroupAction::ToggleCollapse),
            Action::GroupNewTab => Some(GroupAction::NewTab),
            Action::GroupCloseAll => Some(GroupAction::CloseAll),
            _ => None,
        };
        if let Some(group_action) = group_action {
            if let Some(group) = group_menu::keyboard_target(&self.workspace) {
                self.run_group_action(
                    group,
                    group_action,
                    gpu,
                    cell_metrics,
                    proxy,
                    startup_directory,
                    now,
                    style,
                    term_params,
                    shell,
                );
            }
            return ActionOutcome::Handled;
        }

        match action {
            Action::TabNew => {
                self.action_new_tab(
                    cell_metrics,
                    proxy,
                    startup_directory,
                    now,
                    style,
                    term_params,
                    shell,
                );
                ActionOutcome::Handled
            }
            Action::TabClose => match self.action_close_tab(confirm_close_with_process) {
                Some(TabCloseOutcome::Dialog(dialog)) => {
                    self.dialog = Some(dialog);
                    ActionOutcome::Handled
                }
                Some(TabCloseOutcome::Closed { window_empty: true }) => {
                    ActionOutcome::WindowEmptied
                }
                _ => ActionOutcome::Handled,
            },
            Action::TabNext => {
                self.action_next_tab(gpu, style);
                ActionOutcome::Handled
            }
            Action::TabPrev => {
                self.action_prev_tab(gpu, style);
                ActionOutcome::Handled
            }
            // O catálogo garante `1..=9` (`Action::from_str` rejeita o
            // resto) -- `action_goto` é 0-based.
            Action::TabGoto(n) => {
                self.action_goto((n - 1) as usize, gpu, style);
                ActionOutcome::Handled
            }
            Action::TabRename => {
                self.action_rename_start();
                ActionOutcome::Handled
            }
            Action::TabMoveLeft => {
                self.action_move_tab(-1, gpu, style);
                ActionOutcome::Handled
            }
            Action::TabMoveRight => {
                self.action_move_tab(1, gpu, style);
                ActionOutcome::Handled
            }
            Action::GroupCreate => {
                self.action_group_create(gpu, style);
                ActionOutcome::Handled
            }
            Action::GroupNext => {
                self.action_group_step(1, gpu, style);
                ActionOutcome::Handled
            }
            Action::GroupPrev => {
                self.action_group_step(-1, gpu, style);
                ActionOutcome::Handled
            }
            Action::WindowNew => ActionOutcome::OpenWindow,
            Action::WindowClose => ActionOutcome::CloseWindowRequested,
            // Sem default fora do macOS (`docs/reference/acoes.md`); o
            // efeito documentado é "o mesmo do RF-1.4 ao fechar a última
            // janela" -- mesmo caminho de `window.close` na janela atual,
            // que já cursa pelo diálogo de confirmação quando há mais de
            // uma aba.
            Action::AppQuit => ActionOutcome::CloseWindowRequested,
            // `config.reload` (ADR-0003) mexe em `App` (o watcher e o
            // `Arc<Config>` são do processo, não da janela) -- bubble
            // igual a `OpenWindow`/`CloseWindowRequested`.
            Action::ConfigReload => ActionOutcome::ReloadConfig,
            // RF-5.9: a métrica de célula é do processo (ADR-0015) --
            // bubble pra `App`, mesmo caminho de `ReloadConfig`.
            Action::FontIncrease => ActionOutcome::Zoom(Zoom::Increase),
            Action::FontDecrease => ActionOutcome::Zoom(Zoom::Decrease),
            Action::FontReset => ActionOutcome::Zoom(Zoom::Reset),
            // `theme.cycle`: temas nomeados são a etapa 6 também, mas
            // precisam do `Arc<Config>` de `App` -- mesmo caminho.
            Action::ThemeCycle => ActionOutcome::CycleTheme,
            // `scrollback.*`, `clipboard.*`: resolvidos no mapa,
            // executados em `input.rs` -- ver o comentário do método.
            Action::ScrollbackLineUp
            | Action::ScrollbackLineDown
            | Action::ScrollbackPageUp
            | Action::ScrollbackPageDown
            | Action::ScrollbackToTop
            | Action::ScrollbackToBottom
            | Action::ClipboardCopy
            | Action::ClipboardPaste => ActionOutcome::Unhandled,
            // RF-11.16 (menu do terminal, ADR-0041 §10): mesmo mecanismo
            // de `Terminal::start_selection`/`update_selection`, só com os
            // extremos já na tela visível mais o scrollback inteiro.
            Action::SelectionSelectAll => {
                if let Some(id) = self.workspace.active_tab()
                    && let Some(rt) = self.tabs.get(&id)
                {
                    rt.terminal.select_all();
                }
                ActionOutcome::Handled
            }
            // `search.open` (RF-11.1): abre na aba ativa com o campo já
            // focado -- idempotente se já estiver aberta para ela. Abrir
            // limpa a seleção de texto (ADR-0041 §5): sem isso, seleção e
            // ocorrência ficam indistinguíveis, mesma cor.
            Action::SearchOpen => {
                if let Some(id) = self.workspace.active_tab() {
                    if self.search.as_ref().map(SearchBarState::tab) != Some(id) {
                        self.search = Some(SearchBarState::new(id));
                    }
                    if let Some(rt) = self.tabs.get(&id) {
                        rt.terminal.clear_selection();
                    }
                }
                ActionOutcome::Handled
            }
            // `search.next`/`search.prev` (RF-11.5): sem efeito se a busca
            // não estiver aberta -- `F3` continua chegando aqui mesmo
            // então (captura parcial, ADR-0041 §3), e não deve vazar pro
            // terminal.
            Action::SearchNext => {
                if let Some(id) = self.search.as_ref().map(SearchBarState::tab) {
                    self.advance_search(id, true, cell_metrics);
                }
                ActionOutcome::Handled
            }
            Action::SearchPrev => {
                if let Some(id) = self.search.as_ref().map(SearchBarState::tab) {
                    self.advance_search(id, false, cell_metrics);
                }
                ActionOutcome::Handled
            }
            // `Arg`: `FromStr` as rejeita, então nunca entram no mapa
            // resolvido -- inalcançável na prática, mas o `match` precisa
            // ser exaustivo.
            Action::TabMoveToGroup(_) | Action::GroupSetColor(_) => ActionOutcome::Unhandled,
            // As cinco de grupo já retornaram acima.
            Action::GroupDissolve
            | Action::GroupRename
            | Action::GroupToggleCollapse
            | Action::GroupNewTab
            | Action::GroupCloseAll => unreachable!("tratadas em group_action acima"),
        }
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
        style: &TabBarStyle,
        confirm_close_with_process: bool,
    ) -> NewTabRequest {
        let bar_width = self.logical_width;
        let trilha_width = tab_bar::trilha_width(style, bar_width, is_macos());
        let h = bar_height(style);
        let layout = tab_bar::fit_width(
            &self.workspace,
            style,
            trilha_width,
            gpu.text_measurer(),
            is_macos(),
        );
        let overflow = tab_bar::overflow_state(&layout, trilha_width, self.scroll_offset);
        self.scroll_offset = overflow.scroll_offset;

        if !right_click {
            // Botões de janela (ADR-0027): zona fixa à direita, ainda mais
            // à direita que a de configurações. Sempre `None` no macOS
            // (semáforo nativo, o clique nunca chega até aqui).
            // Minimizar/maximizar-restaurar são síncronos, sem diálogo --
            // resolvidos aqui mesmo. Fechar precisa do diálogo de
            // confirmação com múltiplas abas, que só `App` sabe montar
            // (`request_close_window`), então sobe como `NewTabRequest`.
            if let Some(hit) =
                tab_bar::point_in_window_button(style, is_macos(), bar_width, h, logical_point)
            {
                return match hit {
                    tab_bar::WindowButtonHit::Minimize => {
                        self.window.set_minimized(true);
                        NewTabRequest::None
                    }
                    tab_bar::WindowButtonHit::MaximizeRestore => {
                        let maximized = self.window.is_maximized();
                        self.window.set_maximized(!maximized);
                        NewTabRequest::None
                    }
                    tab_bar::WindowButtonHit::Close => NewTabRequest::CloseWindowRequested,
                };
            }
            // Botão de configurações: zona fixa à direita, fora da
            // trilha que rola -- resolvido em coordenadas de tela, como as
            // pílulas de overflow logo abaixo, não pelo hit-test de
            // conteúdo.
            //
            // **Inerte de propósito** (`config` é F4). O clique é
            // consumido aqui em vez de cair na trilha: o botão está
            // desenhado, e deixar o clique atravessar até a aba de baixo
            // seria pior que não responder.
            if tab_bar::point_in_settings_button(style, bar_width, h, is_macos(), logical_point) {
                return NewTabRequest::None;
            }
            if overflow.hidden_left > 0
                && tab_bar::point_in_overflow_pill(
                    style,
                    OverflowSide::Left,
                    trilha_width,
                    h,
                    logical_point,
                )
            {
                self.scroll_offset = (self.scroll_offset - style.overflow_scroll_step).max(0.0);
                return NewTabRequest::None;
            }
            if overflow.hidden_right > 0
                && tab_bar::point_in_overflow_pill(
                    style,
                    OverflowSide::Right,
                    trilha_width,
                    h,
                    logical_point,
                )
            {
                self.scroll_offset += style.overflow_scroll_step;
                return NewTabRequest::None;
            }
        }

        let content_point = (logical_point.0 + self.scroll_offset, logical_point.1);
        let Some(hit) = tab_bar::hit_test(&layout, content_point) else {
            // Nem aba, nem pílula, nem botão de "+", nem (verificado acima)
            // botão de janela ou de configurações: o resto da barra é a
            // drag region (ADR-0027, estilo Firefox -- o mesmo "clicar no
            // vazio da barra move a janela"). Só no clique primário: botão
            // secundário aqui não abre menu nenhum (nenhum alvo sob o
            // cursor) nem arrasta.
            if !right_click {
                self.resolve_titlebar_drag();
            }
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
                // Clique dentro do próprio campo de rename (ADR-0035):
                // posiciona o cursor pelo caractere sob o ponto, sem
                // ativar a aba de novo nem armar arraste -- ela já está
                // ativa e em edição.
                if self.rename.editing_tab() == Some(id)
                    && let Some(tab_rect) = tab_bar::tab_rect(&layout, id)
                {
                    let field_content = chrome::rename_field_rect(tab_rect, style);
                    let field_screen = Rect {
                        x: field_content.x - self.scroll_offset,
                        y: field_content.y,
                        width: field_content.width,
                        height: field_content.height,
                    };
                    if tab_bar::rect_contains(field_screen, logical_point) {
                        if let Some(field) = self.rename.field() {
                            let buffer = field.text().to_string();
                            let text_area =
                                (field_content.width - style.rename_padding_x * 2.0).max(0.0);
                            let measurer = gpu.text_measurer();
                            let text_width = measurer.measure_width(
                                &buffer,
                                chrome::LABEL_FONT,
                                style.rename_font_size,
                            );
                            let text_x_screen = tab_bar::scrolled_text_x(
                                field_screen.x,
                                style.rename_padding_x,
                                text_width,
                                text_area,
                            );
                            let local_x = logical_point.0 - text_x_screen;
                            let byte_index = measurer.index_at_offset(
                                &buffer,
                                chrome::LABEL_FONT,
                                style.rename_font_size,
                                local_x,
                            );
                            self.rename.click_at(byte_index);
                        }
                        self.mouse_button_down = Some(MouseButton::Left);
                        return NewTabRequest::None;
                    }
                }
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
                    self.touch_workspace(|ws| ws.activate_tab(id));
                    self.sync_window_title();
                    self.ensure_active_tab_visible(gpu, style);
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
            TabBarHit::CloseButton(id) => {
                match self.close_tab_via_button(id, confirm_close_with_process) {
                    Some(TabCloseOutcome::Dialog(dialog)) => {
                        self.dialog = Some(dialog);
                        NewTabRequest::None
                    }
                    Some(TabCloseOutcome::Closed { window_empty: true }) => {
                        NewTabRequest::WindowEmptied
                    }
                    _ => NewTabRequest::None,
                }
            }
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

    /// ADR-0027: clique na drag region (nada sob o cursor na barra).
    /// Duplo clique maximiza/restaura, resolvido no *press* -- mesmo
    /// padrão de `handle_pill_click`/`last_pill_click`, mas sem
    /// `GroupId`, e sem passar por `finish_drag`: diferente do arraste de
    /// aba/pílula (que arma no press e só resolve no release),
    /// `Window::drag_window()` entrega o gesto ao loop modal não-client
    /// do SO assim que chamado, então não há garantia de ver o
    /// `MouseInput::Released` de volta -- resolver "foi duplo clique?"
    /// **antes** de chamar `drag_window` é o único jeito seguro.
    fn resolve_titlebar_drag(&mut self) {
        let now = Instant::now();
        let is_double_click = self
            .last_titlebar_click
            .is_some_and(|at| now.duration_since(at) <= DOUBLE_CLICK_THRESHOLD);
        self.last_titlebar_click = if is_double_click { None } else { Some(now) };
        if is_double_click {
            let maximized = self.window.is_maximized();
            self.window.set_maximized(!maximized);
        } else {
            let _ = self.window.drag_window();
        }
    }

    /// RF-2.13/RF-2.22: o que um clique **de verdade** (sem cruzar o
    /// limiar de arraste) na pílula faz -- duplo clique abre o editor, no
    /// lugar do colapso que um clique simples faria (evita o flicker de
    /// colapsar-e-reabrir no segundo clique). Chamado por `finish_drag`
    /// quando o `Drag::GroupPressed` armado nunca virou `GroupDragging`.
    fn handle_pill_click(&mut self, id: GroupId, gpu: &mut GpuContext, style: &TabBarStyle) {
        let now = Instant::now();
        let is_double_click = self.last_pill_click.is_some_and(|(last_id, at)| {
            last_id == id && now.duration_since(at) <= DOUBLE_CLICK_THRESHOLD
        });
        self.last_pill_click = Some((id, now));
        if is_double_click {
            self.last_pill_click = None;
            self.open_group_editor(id, EditorRegion::Name);
        } else {
            self.toggle_group_collapse(id, gpu, style);
        }
    }

    /// RF-1.6 aplicado ao botão de fechar de uma aba que não é
    /// necessariamente a ativa (o clique pode ser em qualquer aba da
    /// trilha) -- mesma condição de `action_close_tab`, mas sobre `id`
    /// explícito.
    fn close_tab_via_button(
        &mut self,
        id: TabId,
        confirm_close_with_process: bool,
    ) -> Option<TabCloseOutcome> {
        // ADR-0037 §3 -- mesma checagem de `action_close_tab`, antes da
        // detecção do ADR-0034.
        if self.workspace.tab(id).is_some_and(|t| t.is_not_started()) {
            let window_empty = self.close_tab_unconditionally(id);
            return Some(TabCloseOutcome::Closed { window_empty });
        }
        let runtime = self.tabs.get(&id)?;
        if should_confirm_tab_close(
            confirm_close_with_process,
            runtime.terminal.modes(),
            runtime.terminal.has_extra_processes(),
        ) {
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
    fn action_group_create(&mut self, gpu: &mut GpuContext, style: &TabBarStyle) {
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

        let old_layout = tab_bar::fit_width(
            &self.workspace,
            style,
            self.logical_width,
            gpu.text_measurer(),
            is_macos(),
        );
        let color = self.workspace.next_auto_color();
        let Some(group) = self.touch_workspace(|ws| ws.group_tabs(&ids, "Novo grupo", color))
        else {
            return;
        };
        self.animations
            .start_reflow(&old_layout, GROUP_CREATE_REFLOW_DURATION, Instant::now());
        self.selection.clear();
        self.open_group_editor(group, EditorRegion::Name);
        self.sync_window_title();
    }

    fn toggle_group_collapse(&mut self, id: GroupId, gpu: &mut GpuContext, style: &TabBarStyle) {
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
        let old_layout = tab_bar::fit_width(
            &self.workspace,
            style,
            self.logical_width,
            gpu.text_measurer(),
            is_macos(),
        );
        self.touch_workspace(|ws| ws.collapse_group(id, collapsing));
        self.animations
            .start_reflow(&old_layout, COLLAPSE_REFLOW_DURATION, Instant::now());
        self.sync_window_title();
        self.ensure_active_tab_visible(gpu, style);
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
        style: &TabBarStyle,
        term_params: &TermParams,
        shell: &porecatu_config::Shell,
    ) {
        match action {
            GroupAction::Rename => self.open_group_editor(group, EditorRegion::Name),
            GroupAction::SetColor => self.open_group_editor(group, EditorRegion::Swatches),
            GroupAction::ToggleCollapse => self.toggle_group_collapse(group, gpu, style),
            GroupAction::NewTab => {
                self.action_new_tab_in_group(
                    group,
                    cell_metrics,
                    proxy,
                    startup_directory,
                    now,
                    style,
                    term_params,
                    shell,
                );
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
                self.touch_workspace(|ws| ws.ungroup(group));
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
    #[allow(clippy::too_many_arguments)]
    fn action_new_tab_ungrouped(
        &mut self,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        startup_directory: &Option<PathBuf>,
        now: Instant,
        style: &TabBarStyle,
        term_params: &TermParams,
        shell: &porecatu_config::Shell,
    ) {
        if self.rename.editing_tab().is_some() {
            self.commit_rename();
        }
        let cwd = self.resolve_new_tab_cwd(startup_directory);
        self.open_tab(
            cell_metrics,
            proxy,
            cwd,
            now,
            NewTabTarget::Ungrouped,
            style,
            term_params,
            shell,
            startup_directory,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn action_new_tab_in_group(
        &mut self,
        group: GroupId,
        cell_metrics: CellMetrics,
        proxy: &EventLoopProxy<Wakeup>,
        startup_directory: &Option<PathBuf>,
        now: Instant,
        style: &TabBarStyle,
        term_params: &TermParams,
        shell: &porecatu_config::Shell,
    ) {
        if self.rename.editing_tab().is_some() {
            self.commit_rename();
        }
        let cwd = self.resolve_group_new_tab_cwd(group, startup_directory);
        self.open_tab(
            cell_metrics,
            proxy,
            cwd,
            now,
            NewTabTarget::Group(group),
            style,
            term_params,
            shell,
            startup_directory,
        );
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
    fn finish_drag(&mut self, gpu: &mut GpuContext, style: &TabBarStyle) {
        let drag = std::mem::replace(&mut self.drag, Drag::Idle);
        match drag {
            Drag::TabDragging { tab, target, .. } if self.in_bar(self.cursor_position.1, style) => {
                match target {
                    DragDrop::IntoGroup { group, pos } => {
                        self.touch_workspace(|ws| ws.move_tab_to_group_at(tab, group, pos));
                    }
                    DragDrop::NewRun { group_index } => {
                        self.touch_workspace(|ws| ws.move_tab_to_new_run(tab, group_index));
                    }
                }
            }
            Drag::GroupDragging {
                group,
                preview_index,
                ..
            } if self.in_bar(self.cursor_position.1, style) => {
                self.touch_workspace(|ws| ws.move_group(group, preview_index));
            }
            Drag::GroupPressed { group, .. } => {
                self.handle_pill_click(group, gpu, style);
            }
            Drag::TabPressed { .. } | Drag::TabDragging { .. } | Drag::GroupDragging { .. } => {}
            Drag::Idle => {}
        }
        self.window.set_cursor(CursorIcon::Default);
    }

    /// `y` físico está dentro da faixa da barra de abas (topo da janela).
    fn in_bar(&self, physical_y: f64, style: &TabBarStyle) -> bool {
        physical_y < (bar_height(style) * self.scale) as f64
    }

    /// O que abre o menu de contexto da barra (ADR-0021 §3): botão direito
    /// em qualquer plataforma, e `Ctrl`+clique esquerdo **só** no macOS --
    /// lá é o clique secundário da plataforma, e não toca a seleção.
    fn is_secondary_bar_click(&self, button: MouseButton) -> bool {
        button == MouseButton::Right
            || (cfg!(target_os = "macos") && button == MouseButton::Left && self.modifiers.ctrl)
    }

    /// Recalcula linhas/colunas a partir do tamanho lógico e propaga pra
    /// `WindowSurface` e pro terminal de **todas** as abas da janela
    /// (motor + PTY) -- não só a ativa: uma aba em segundo plano cujo PTY
    /// nunca foi redimensionado mostra a métrica errada assim que
    /// ativada. `WindowSurface` é quem converte de volta para físico
    /// (ADR-0018). `Terminal::resize` é barato de chamar em rajada -- o
    /// lado do PTY é assíncrono, perder um em trânsito não é grave (ver o
    /// comentário do método).
    fn resize_to(
        &mut self,
        width: u32,
        height: u32,
        gpu: &GpuContext,
        cell_metrics: CellMetrics,
        style: &TabBarStyle,
    ) {
        self.window_surface.resize(gpu, width, height, self.scale);
        self.logical_width = width as f32 / self.scale;
        self.logical_height = height as f32 / self.scale;
        let (rows, cols) = self.grid_size(cell_metrics, style);
        for runtime in self.tabs.values() {
            runtime.terminal.resize(rows, cols);
        }
        self.window.request_redraw();
    }

    fn cell_at_cursor(
        &self,
        cell_metrics: CellMetrics,
        style: &TabBarStyle,
    ) -> input::CellPosition {
        let content = paint::terminal_content_rect(
            style,
            bar_height(style),
            self.logical_width,
            self.logical_height,
        );
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
    fn update_hover(
        &mut self,
        gpu: &mut GpuContext,
        now: Instant,
        config: &porecatu_config::Config,
        style: &TabBarStyle,
    ) {
        let bar_point = (
            self.cursor_position.0 as f32 / self.scale,
            self.cursor_position.1 as f32 / self.scale,
        );
        let target = if self.in_bar(self.cursor_position.1, style)
            && self.dialog.is_none()
            && self.context_menu.is_none()
            && self.group_context_menu.is_none()
            && self.group_editor.is_none()
            && self.move_to_group.is_none()
            && matches!(self.drag, Drag::Idle)
        {
            let layout = tab_bar::fit_width(
                &self.workspace,
                style,
                self.logical_width,
                gpu.text_measurer(),
                is_macos(),
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

        let warning_layout = overlay::layout_warnings(
            &self.warnings,
            config,
            bar_height(style),
            self.logical_width,
        );
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
    /// Caminho resolvido no start (ADR-0003), o mesmo que o watcher do
    /// hot reload assiste -- `config.reload` (catálogo de ações) relê
    /// este caminho na hora, sem esperar o `notify`. `None` só se
    /// `dirs::config_dir` também falhar (mesma condição rara de
    /// `resolve_config_path`).
    config_path: Option<PathBuf>,
    /// Home do usuário -- fallback de `tab.new`/`window.new` quando a aba
    /// ativa ainda não tem `cwd` capturado por OSC 7 (ADR-0017 item 1).
    /// `None` só se `dirs::home_dir` falhar em resolvê-la.
    startup_directory: Option<PathBuf>,
    /// Métricas de célula em pixels lógicos -- DPI-independentes por
    /// definição (só `WindowSurface` converte pra físico), então uma só
    /// medição serve todas as janelas (ADR-0015).
    cell_metrics: CellMetrics,
    /// `font.increase`/`decrease`/`reset` (RF-5.9, F4 etapa 6): tamanho de
    /// fonte da **sessão**, nunca escrito no arquivo. `None` = usa
    /// `config.terminal.font.size`. Simplificação registrada: `zoom_scope`
    /// (RF-5.10, "só a aba ativa ou todas") não tem efeito ainda -- o
    /// zoom é sempre do processo inteiro, como se `zoom_scope = "all"`
    /// sempre valesse. Zoom por aba exigiria métricas de célula por
    /// `TabRuntime`, não só por processo -- fora do escopo desta etapa;
    /// reportado ao usuário.
    font_zoom_px: Option<f32>,
    /// `theme.cycle` (RF-5.21, ADR-0031 §4): tema da **sessão**, nunca
    /// escrito no arquivo. `None` = usa `config.terminal.theme`. `Some("")`
    /// é o estado "sem tema" quando ele não é o primeiro do ciclo (por
    /// exemplo, `config.terminal.theme` não vazio e o usuário ciclou de
    /// volta ao início). Sobrevive a uma recarga enquanto o tema ainda
    /// existir (ADR-0031 §4); se sumir, `apply_config_reload` o zera e
    /// avisa.
    session_theme: Option<String>,
    /// Config carregada uma vez por processo (docs/arquitetura.md, "Na
    /// implementação (F2, etapa 6)": `GpuContext`/`cell_metrics`/
    /// `startup_directory` são as coisas que não variam por janela --
    /// `config` entra na mesma lista nesta etapa). `Arc` porque o hot
    /// reload (F4 etapa 4) troca o valor inteiro por um novo `Arc`, sem
    /// lock (docs/arquitetura.md linha 118).
    config: Arc<porecatu_config::Config>,
    /// `TabBarStyle::from_config(&config)`, calculado uma vez -- evita
    /// reconstruir a cada frame só para ler campos de `Config`.
    style: TabBarStyle,
    /// `palette::ResolvedPalette::from_config(&config)`, mesma razão.
    pal: palette::ResolvedPalette,
    /// `palette::ResolvedTermPalette::from_config(&config)` --
    /// `[terminal.colors]` e subseções (F4 etapa 3).
    term_pal: palette::ResolvedTermPalette,
    /// `TermParams` montado a partir de `Config` (docs/arquitetura.md
    /// seção 4.2: `porecatu-term` nunca importa `porecatu-config`, quem
    /// preenche o struct de parâmetros dele é `ui`). Clonado a cada
    /// `Terminal::spawn` -- é uma `String` e alguns escalares, sem custo no
    /// caminho quente (spawn de aba não é por frame).
    term_params: TermParams,
    /// Resolvido de `config.keybindings` (F4 etapa 5, ADR-0029): defaults
    /// embutidos da plataforma atual -> tabela comum -> tabela da
    /// plataforma atual. Recalculado inteiro a cada hot reload, junto do
    /// resto (classe A) -- é barato e evita amarrar `diff` a mais uma
    /// árvore de comparação.
    keymap: HashMap<Chord, Action>,
    windows: HashMap<WindowId, WindowState>,
    /// Debounce da gravação de sessão (RF-3.3, ADR-0036) -- por
    /// **processo**, não por janela: o arquivo é um só para todas
    /// (RF-3.17). Drenado em `schedule_next_wake`, disparado em
    /// `tick_all`. `enabled = false` (RF-3.6) nunca marca isto sujo --
    /// ver `Self::session_persistence_enabled`.
    session: SessionScheduler,
    /// Resultado de `porecatu_session::load()` (RF-3.7, F5 etapa 4),
    /// carregado uma vez em `App::new` e consumido em `App::resumed` --
    /// não pode ser lido antes, porque `resumed` é o primeiro ponto em que
    /// há `ActiveEventLoop`/janela pra restaurar ou pra mostrar um aviso
    /// de recuperação (ADR-0036 §5). `None` só depois de consumido, ou se
    /// `[session] enabled = false` (aí nem chega a carregar).
    pending_session: Option<porecatu_session::LoadOutcome>,
    /// Caminho posicional da linha de comando (RF-3.12, ADR-0040 §2):
    /// `Some` desliga a leitura e a gravação da sessão para o processo
    /// inteiro (mesmo caminho de `[session] enabled = false`, ver
    /// `Self::session_persistence_enabled`) e vence `startup_directory` na
    /// única aba que a janela do arranque abre. `None` é o caso comum.
    positional_directory: Option<PathBuf>,
    /// Dispensa definitiva do convite de integração de shell (RF-3.1,
    /// ADR-0039 §4) -- gravada em `session.json`, carregada em `resumed`.
    /// `true` já basta para nunca detectar nem mostrar a nota, em
    /// qualquer execução futura.
    shell_integration_dismissed: bool,
    /// `true` a partir do instante em que o convite é escolhido para
    /// aparecer nesta execução (RF-3.1: "uma vez por execução, não por
    /// aba") -- mesmo sem ter sido escrito ainda de fato (`pending_shell_
    /// integration_note` cobre a tela alternativa). Reseta a cada
    /// execução do processo, diferente do campo acima.
    shell_integration_invite_claimed: bool,
    /// Convite escolhido, mas adiado porque a aba estava em tela
    /// alternativa no momento (ADR-0039 §1: "não escrever com a tela
    /// alternativa ativa") -- resolvido no próximo `Wakeup::TabDirty`
    /// desta mesma aba (`App::user_event`), quando ela pode ter voltado à
    /// tela primária.
    pending_shell_integration_note: Option<(WindowId, TabId)>,
    /// Nome do tema de sessão restaurado que não existe mais no arquivo
    /// (ADR-0031 §4) -- guardado por `apply_restored_session_state` e
    /// consumido por `resumed` assim que a primeira janela existe (é
    /// quando há onde empilhar o aviso).
    vanished_restored_theme: Option<String>,
    /// RF-11.24: avisos de config do **arranque** (RF-4.21 config
    /// inválida, RF-4.22 chave desconhecida, RF-5.18 tema desconhecido),
    /// montados em `App::new` -- que não tem janela nenhuma ainda para
    /// empilhar um `Warning` -- e entregues à primeira janela criada em
    /// `resumed`, mesmo padrão de `pending_session`/`Notice`.
    pending_startup_warnings: Vec<(Severity, &'static str, String)>,
}

/// Ordem de `theme.cycle` (RF-5.21, ADR-0031 §3): "" (sem tema) primeiro,
/// depois `declared` na ordem de declaração do arquivo -- nunca
/// alfabética, é a única ordem que o usuário controla sem renomear nada.
/// Função pura, testável sem `App`/GPU. `current` fora da lista (não
/// deveria acontecer: `active_theme_name` só devolve `""` ou um nome de
/// `config.themes`) cai no primeiro item, não trava.
fn next_theme_name<'a>(declared: &[&'a str], current: &str) -> &'a str {
    let mut names: Vec<&str> = vec![""];
    names.extend_from_slice(declared);
    let pos = names.iter().position(|&n| n == current).unwrap_or(0);
    names[(pos + 1) % names.len()]
}

#[cfg(test)]
mod theme_cycle_tests {
    use super::next_theme_name;

    #[test]
    fn no_theme_starts_the_cycle() {
        let declared = ["a", "b"];
        assert_eq!(next_theme_name(&declared, ""), "a");
    }

    #[test]
    fn cycle_advances_in_declaration_order() {
        let declared = ["a", "b"];
        assert_eq!(next_theme_name(&declared, "a"), "b");
    }

    #[test]
    fn cycle_wraps_back_to_no_theme() {
        let declared = ["a", "b"];
        assert_eq!(next_theme_name(&declared, "b"), "");
    }

    #[test]
    fn unknown_current_restarts_the_cycle() {
        let declared = ["a", "b"];
        assert_eq!(next_theme_name(&declared, "sumiu"), "a");
    }
}

/// Passo de `font.increase`/`decrease` (RF-5.9) -- sem valor no design nem
/// no PRD, escolhido para ficar perto do que outros terminais usam.
const FONT_ZOOM_STEP_PX: f32 = 1.0;
const FONT_ZOOM_MIN_PX: f32 = 6.0;
const FONT_ZOOM_MAX_PX: f32 = 96.0;

/// RF-3.1 (ADR-0039 §2), gatilho temporal do convite no Windows: uma
/// integração de shell configurada emite OSC 7 a cada prompt exibido, não
/// só em `cd` (os snippets de `docs/reference/integracao-de-shell.md`
/// disparam no `precmd`/`PROMPT_COMMAND`) -- então o primeiro prompt já
/// emite, tipicamente em bem menos de um segundo. Três segundos cobrem
/// shell lento pra subir (perfil grande, `oh-my-posh`) sem atrasar
/// perceptivelmente o convite pra quem não tem OSC 7 nenhum.
#[cfg(windows)]
const WINDOWS_SHELL_INTEGRATION_INVITE_INTERVAL: Duration = Duration::from_secs(3);

impl App {
    /// Tamanho de fonte efetivo (RF-5.9): o da sessão, se `font.increase`/
    /// `decrease` já mexeu nele, senão o do arquivo.
    fn effective_font_size_px(&self) -> f32 {
        self.font_zoom_px
            .unwrap_or(self.config.terminal.font.size as f32)
    }

    /// Nome do tema efetivo (ADR-0031 §4): o da sessão, se `theme.cycle`
    /// já mexeu nele, senão o declarado no arquivo. `""` é "sem tema".
    fn active_theme_name(&self) -> &str {
        self.session_theme
            .as_deref()
            .unwrap_or(&self.config.terminal.theme)
    }

    /// Recalcula as duas paletas resolvidas a partir do tema efetivo
    /// (`active_theme_name`) e devolve as chaves que "venceram" o tema
    /// (ADR-0031 §1) -- chamado por `cycle_theme` e por
    /// `apply_config_reload`, que também precisam repetir isto.
    fn recompute_palettes(&mut self) -> Vec<&'static str> {
        let name = self.active_theme_name().to_owned();
        let themed = porecatu_config::apply_theme(&self.config, &name);
        self.pal = palette::ResolvedPalette::from_config(&themed);
        self.term_pal = palette::ResolvedTermPalette::from_config(&themed);
        porecatu_config::theme_overridden_keys(&self.config, &name)
    }

    /// `theme.cycle` (RF-5.21): anda na ordem de declaração dos
    /// `[[themes]]`, com "sem tema" participando como primeiro item
    /// (ADR-0031 §3). Não escreve no arquivo (§4) e é classe A -- troca
    /// só as paletas resolvidas, sem tocar grade nem PTY.
    fn cycle_theme(&mut self, now: Instant) {
        if self.config.themes.is_empty() {
            return; // "sem nenhum [[themes]] declarado, ciclar não faz nada"
        }
        let declared: Vec<&str> = self.config.themes.iter().map(|t| t.name.as_str()).collect();
        let next = next_theme_name(&declared, self.active_theme_name());
        self.session_theme = Some(next.to_owned());

        let overridden = self.recompute_palettes();
        for state in self.windows.values_mut() {
            if !overridden.is_empty() {
                state.warnings.push(
                    Severity::Info,
                    "Cor fora do tema vencendo",
                    overridden.join(", "),
                    now,
                );
            }
            state.window.request_redraw();
        }
    }

    /// `cli_config`/`cli_directory` vêm do parse de `argv` em `src/main.rs`
    /// (ADR-0040 §4) -- `argv` é do processo, então só o binário pode lê-lo
    /// sem furar a regra de dependência; `run` só repassa.
    fn new(
        proxy: EventLoopProxy<Wakeup>,
        cli_config: Option<PathBuf>,
        cli_directory: Option<PathBuf>,
    ) -> Self {
        // RF-11.24: erro de parse/chave desconhecida/tema desconhecido no
        // arranque viajam para `pending_startup_warnings` em vez de
        // `stderr` -- a `App` (dona da pilha de avisos, uma por janela)
        // ainda não existe neste ponto, então são entregues à primeira
        // janela criada em `resumed` (mesmo caminho de `pending_session`,
        // que a etapa 4 da F5 abriu). Os defaults seguem valendo
        // (`LoadResult::config()` nunca falta).
        let mut pending_startup_warnings = Vec::new();
        let load_result = porecatu_config::load(cli_config.as_deref());
        match &load_result {
            porecatu_config::LoadResult::Invalid { error, .. } => {
                pending_startup_warnings.push((
                    Severity::Error,
                    "Config inválida",
                    error.to_string(),
                ));
            }
            porecatu_config::LoadResult::Loaded { unknown_keys, .. } => {
                // RF-4.22: chave desconhecida é aviso, não erro -- mesma
                // severidade/título do hot reload (`apply_config_reload`).
                for key in unknown_keys {
                    pending_startup_warnings.push((
                        Severity::Warning,
                        "Chave desconhecida na config",
                        key.clone(),
                    ));
                }
            }
            porecatu_config::LoadResult::Missing { .. } => {}
        }
        let config = Arc::new(load_result.config().clone());
        // RF-5.18/ADR-0031 §5: nome desconhecido em `[terminal] theme` é
        // aviso.
        if !config.terminal.theme.is_empty()
            && !config
                .themes
                .iter()
                .any(|t| t.name == config.terminal.theme)
        {
            pending_startup_warnings.push((
                Severity::Warning,
                "Tema desconhecido",
                format!(
                    "\"{}\" não está em [[themes]]; usando defaults.",
                    config.terminal.theme
                ),
            ));
        }
        let style = TabBarStyle::from_config(&config);
        let themed = porecatu_config::apply_theme(&config, &config.terminal.theme);
        let pal = palette::ResolvedPalette::from_config(&themed);
        let term_pal = palette::ResolvedTermPalette::from_config(&themed);
        let term_params = term_params_from_config(&config);
        // ADR-0029: erro de `[keybindings]` no start tem a mesma limitação
        // que erro de config acima -- sem `App` ainda, sem janela pra
        // avisar. Log em stderr; o mapa resolvido nunca falta (uma chave
        // malformada só descarta aquela linha).
        let keymap_resolved = keymap::resolve(&config.keybindings, keymap::Platform::current());
        for issue in &keymap_resolved.issues {
            eprintln!("keybinding inválido, ignorado: {issue}");
        }
        let keymap = keymap_resolved.bindings;

        // Hot reload (F4 etapa 4, ADR-0030): mesma precedência do start,
        // pra assistir o arquivo que `load` de fato leu. `None` (sem
        // diretório resolvido, ou ele ainda não existe) degrada para "sem
        // hot reload" -- não falha o start (ADR-0003 regra 1).
        let config_path = porecatu_config::resolve_config_path(cli_config.as_deref());
        if let Some(path) = config_path.clone() {
            let watcher_proxy = proxy.clone();
            reload::watch(path, move |reload| {
                let _ = watcher_proxy.send_event(Wakeup::ConfigReloaded(Box::new(reload)));
            });
        }

        // RF-3.7 (ADR-0036 §5): carregado aqui porque `enabled` já está à
        // mão, mas só **consumido** em `resumed` -- não há `ActiveEventLoop`
        // nem janela ainda para restaurar nela ou mostrar o aviso de
        // arquivo corrompido/schema mais novo (RF-3.14/RF-3.16). RF-3.12
        // (ADR-0040 §2): modo posicional nunca lê a sessão gravada, então
        // nem chega a carregar -- `cli_directory` vence `enabled` aqui.
        let pending_session =
            (config.session.enabled && cli_directory.is_none()).then(porecatu_session::load);

        Self {
            gpu: None,
            proxy,
            config_path,
            startup_directory: resolve_startup_directory(&config.general.startup_directory),
            cell_metrics: CellMetrics {
                width: 1.0,
                height: 1.0,
            },
            font_zoom_px: None,
            session_theme: None,
            config,
            style,
            pal,
            term_pal,
            term_params,
            keymap,
            windows: HashMap::new(),
            session: SessionScheduler::default(),
            pending_session,
            positional_directory: cli_directory,
            shell_integration_dismissed: false,
            shell_integration_invite_claimed: false,
            pending_shell_integration_note: None,
            vanished_restored_theme: None,
            pending_startup_warnings,
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
        let mut attributes = base_window_attributes();
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
        // ADR-0015: "a janela nova abre com uma aba no cwd da aba ativa no
        // momento da criação" -- da janela de ORIGEM, já que a nova ainda
        // não tem nenhuma aba. Resolvido **antes** de criar a janela: o
        // empréstimo de `origin_state` (que aponta para dentro de
        // `self.windows`) precisa terminar antes do empréstimo mutável de
        // `self` que `create_window_with_attributes` toma abaixo.
        let cwd = origin_state
            .and_then(|s| s.resolve_new_tab_cwd(&self.startup_directory))
            .or_else(|| self.startup_directory.clone());
        self.open_window_with(event_loop, attributes, cwd);
    }

    /// Núcleo comum a [`Self::open_window`] e ao arranque em modo
    /// posicional (RF-3.12, ADR-0040 §2, em `resumed`): cria a janela e a
    /// primeira aba com o `cwd` já resolvido por quem chama -- só ele sabe
    /// se veio da janela de origem (cascata), de `startup_directory`, ou
    /// do caminho posicional da linha de comando, que vence os dois.
    fn open_window_with(
        &mut self,
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
        cwd: Option<PathBuf>,
    ) {
        let Some(mut state) = self.create_window_with_attributes(event_loop, attributes) else {
            return;
        };

        let window_id = state.window.id();
        state.open_tab(
            self.cell_metrics,
            &self.proxy,
            cwd,
            Instant::now(),
            NewTabTarget::ActiveGroup,
            &self.style,
            &self.term_params,
            &self.config.shell,
            &self.startup_directory,
        );
        self.windows.insert(window_id, state);
    }

    /// RF-3.7/RF-3.17 (metade de restauração, F5 etapa 4): recria uma
    /// janela a partir de um `WindowV1` gravado -- geometria e monitor
    /// (RF-3.11) quando `restore_window_geometry` estiver ligado, grupos e
    /// abas via [`porecatu_session::convert::workspace_from_window`]
    /// (RF-3.8, restauração preguiçosa), e o shell só da(s) aba(s) que
    /// precisam subir no start (a ativa de cada janela, ou todas se
    /// `lazy_restore = false`) -- as demais ficam `NotStarted`, subindo ao
    /// primeiro foco pelo mesmo `Self::ensure_active_tab_started` de
    /// sempre.
    fn open_window_from_session(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_v1: &porecatu_session::WindowV1,
    ) {
        let mut attributes = base_window_attributes();
        if self.config.session.restore_window_geometry {
            let monitors: Vec<session_writer::MonitorInfo> = event_loop
                .available_monitors()
                .map(|m| session_writer::monitor_info(&m))
                .collect();
            let primary = event_loop
                .primary_monitor()
                .map(|m| session_writer::monitor_info(&m));
            let resolved = session_writer::resolve_restored_geometry(
                &window_v1.geometry,
                window_v1.monitor.as_ref(),
                &monitors,
                primary.as_ref(),
            );
            // `winit` recusa tamanho zero -- geometria degenerada (arquivo
            // editado à mão) cai no mínimo de 1px em vez de falhar a
            // criação da janela inteira.
            attributes = attributes
                .with_inner_size(winit::dpi::PhysicalSize::new(
                    resolved.size.0.max(1),
                    resolved.size.1.max(1),
                ))
                .with_position(winit::dpi::PhysicalPosition::new(
                    resolved.position.0,
                    resolved.position.1,
                ))
                .with_maximized(resolved.maximized);
        }

        let Some(mut state) = self.create_window_with_attributes(event_loop, attributes) else {
            return;
        };
        let window_id = state.window.id();

        let lazy_restore = self.config.session.lazy_restore;
        state.workspace = porecatu_session::convert::workspace_from_window(
            &window_v1.groups,
            &window_v1.tabs,
            window_v1.active_tab,
            lazy_restore,
        );

        // RF-3.8: só a aba ativa sobe agora; com `lazy_restore = false`,
        // `workspace_from_window` já não produziu nenhuma `NotStarted`, e
        // todas precisam de `TabRuntime` desde já.
        let to_start: Vec<TabId> = if lazy_restore {
            state.workspace.active_tab().into_iter().collect()
        } else {
            state.workspace.visual_order().collect()
        };
        let now = Instant::now();
        for tab_id in to_start {
            let cwd = state.workspace.tab(tab_id).and_then(|t| t.cwd().cloned());
            state.spawn_tab_runtime(
                tab_id,
                cwd,
                self.cell_metrics,
                &self.proxy,
                now,
                &self.style,
                &self.term_params,
                &self.config.shell,
                &self.startup_directory,
            );
            if let Some(tab) = state.workspace.tab_mut(tab_id) {
                tab.start();
            }
        }
        state.sync_window_title();
        self.windows.insert(window_id, state);
    }

    /// Núcleo comum de [`Self::open_window`]/[`Self::open_window_from_session`]:
    /// cria a janela do `winit`, a surface `wgpu` (a primeira também cria
    /// o `GpuContext` do processo) e mede a célula -- tudo que não depende
    /// de qual aba/workspace a janela vai receber. `None` só na falha rara
    /// de criar a surface (GPU incompatível numa segunda tela, ADR-0018);
    /// a primeira janela do processo continua um `.expect` (sem ela não há
    /// app, igual a antes desta etapa).
    fn create_window_with_attributes(
        &mut self,
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
    ) -> Option<WindowState> {
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
                    return None;
                }
            }
        } else {
            let (gpu, window_surface) = GpuContext::new(
                Arc::clone(&window),
                size.width,
                size.height,
                font_families_from_config(&self.config),
            );
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

        let font_size_px = self.effective_font_size_px();
        let line_height_px = font_size_px * self.config.terminal.font.line_height as f32;
        if let Some(gpu) = &mut self.gpu {
            let (cell_width, cell_height) = gpu
                .text_measurer()
                .measure_mono_cell(font_size_px, line_height_px);
            self.cell_metrics = snap_cell_metrics_to_pixel_grid(cell_width, cell_height, scale);
        }

        let mut state = WindowState::new(window, window_surface, scale);
        state
            .animations
            .set_enabled(self.config.appearance.window.animations);
        Some(state)
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
        if self.windows.is_empty() {
            // `app.quit`/RF-3.4: gravação síncrona da sessão, antes de
            // sair -- fecha o no-op documentado desde a F2 (ADR-0017 §7).
            // Captura `state` **antes** de `state.tabs` ser consumido
            // abaixo: `write_session_now` precisa de uma referência à
            // janela inteira (workspace + geometria), que um campo já
            // movido não permite mais emprestar. Falha de gravação nunca
            // trava a saída -- no máximo `eprintln!` (ver o método).
            self.write_session_now(std::iter::once(&state));
        }
        let waits: Vec<_> = state
            .tabs
            .into_values()
            .map(|runtime| runtime.terminal.close())
            .collect();
        for wait in waits {
            wait.wait();
        }
        if self.windows.is_empty() {
            self.session.clear();
            event_loop.exit();
        }
    }

    /// RF-10.23 (ADR-0015): fechar janela com mais de uma aba pede
    /// confirmação -- incondicional, não olha `confirm_close_with_process`.
    /// **Desde o ADR-0034**, também confirma com **uma aba só** se ela tiver
    /// processo ativo (mesmo critério de `should_confirm_tab_close`: modo
    /// do terminal ou contagem de processo na árvore) -- fechar a janela
    /// mata a árvore igual a fechar a aba (`close_window_unconditionally`
    /// chama `Terminal::close` para cada uma), e o usuário não tinha aviso
    /// nenhum nesse caminho. Essa segunda checagem respeita
    /// `confirm_close_with_process`, como o fechamento de aba.
    ///
    /// ADR-0037 §3: a contagem que decide "mais de uma aba aberta" conta
    /// `state.tabs` (o mapa de `TabRuntime`), não `workspace` -- aba
    /// `NotStarted` nunca tem entrada ali (nada rodou, nada a perder), e
    /// contar `workspace` faria uma janela só com abas `NotStarted`
    /// confirmar por "mais de uma aba", o que o ADR proíbe.
    fn request_close_window(&mut self, window_id: WindowId, event_loop: &ActiveEventLoop) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let significant_tab_count = state.tabs.len();
        let confirm_close_with_process = self.config.general.confirm_close_with_process;
        let any_tab_busy = state.tabs.values().any(|runtime| {
            should_confirm_tab_close(
                confirm_close_with_process,
                runtime.terminal.modes(),
                runtime.terminal.has_extra_processes(),
            )
        });
        if window_close_needs_confirmation(significant_tab_count, any_tab_busy) {
            let body = if significant_tab_count > 1 {
                "Esta janela tem mais de uma aba aberta."
            } else {
                "Esta janela tem um programa em primeiro plano."
            };
            state.dialog = Some(ConfirmDialog::new(
                "Fechar janela?",
                body,
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
                        &self.style,
                        &self.term_params,
                        &self.config.shell,
                    );
                    state.window.request_redraw();
                }
            }
            MenuAction::CloseTab => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    match state
                        .close_tab_via_button(tab, self.config.general.confirm_close_with_process)
                    {
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

    /// Espelha `run_menu_action`, para o menu de contexto do terminal
    /// (RF-11.14 a RF-11.16). Copiar/colar/selecionar tudo/buscar chamam
    /// exatamente as mesmas funções que a tecla usa (`input::
    /// clipboard_copy`/`clipboard_paste`, `Terminal::select_all`,
    /// `Action::SearchOpen`'s dispatch) -- tecla e menu não podem divergir
    /// (lição da F3, ADR-0042 §10). Abrir/copiar link reusam a mesma
    /// célula do clique que abriu o menu (`menu.anchor`), não a posição
    /// corrente do cursor -- o menu pode ter sido navegado por teclado.
    fn run_terminal_menu_action(
        &mut self,
        window_id: WindowId,
        menu: TerminalContextMenu,
        action: TerminalMenuAction,
    ) {
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let Some(runtime) = state.tabs.get(&menu.tab) else {
            return;
        };
        match action {
            TerminalMenuAction::Copy => input::clipboard_copy(&runtime.terminal),
            TerminalMenuAction::Paste => {
                let modes = runtime.terminal.modes();
                input::clipboard_paste(&runtime.terminal, &modes);
            }
            TerminalMenuAction::SelectAll => runtime.terminal.select_all(),
            TerminalMenuAction::OpenSearch => {
                runtime.terminal.clear_selection();
                if state.search.as_ref().map(SearchBarState::tab) != Some(menu.tab) {
                    state.search = Some(SearchBarState::new(menu.tab));
                }
            }
            TerminalMenuAction::OpenLink | TerminalMenuAction::CopyLink => {
                let content = paint::terminal_content_rect(
                    &self.style,
                    bar_height(&self.style),
                    state.logical_width,
                    state.logical_height,
                );
                let content_x = ((menu.anchor.0 - content.x) * state.scale).max(0.0) as f64;
                let content_y = ((menu.anchor.1 - content.y) * state.scale).max(0.0) as f64;
                let cell = input::cell_at(
                    content_x,
                    content_y,
                    self.cell_metrics,
                    runtime.snapshot.rows.max(MIN_GRID),
                    runtime.snapshot.cols.max(MIN_GRID),
                );
                let uri =
                    hyperlink::uri_at(&runtime.snapshot, cell.row, cell.col).map(str::to_string);
                if let Some(uri) = uri {
                    if action == TerminalMenuAction::CopyLink {
                        clipboard::copy(&uri);
                    } else {
                        let outcome = hyperlink::open_hyperlink(&uri);
                        state.report_link_outcome(outcome, &uri, Instant::now());
                    }
                }
            }
        }
        state.window.request_redraw();
    }

    /// RF-3.6: único ponto que decide se a sessão persiste -- o debounce
    /// e a gravação síncrona do exit consultam isto, nunca a config
    /// direto. RF-3.12/ADR-0040 §2: o modo de caminho posicional nunca
    /// grava, mesmo com `[session] enabled = true` -- os dois chamadores
    /// não mudaram.
    fn session_persistence_enabled(&self) -> bool {
        self.config.session.enabled && self.positional_directory.is_none()
    }

    /// RF-5.9: passos de zoom de sessão a gravar (ADR-0036 §1,
    /// `WindowV1::zoom_steps`) -- `font_zoom_px` guarda um tamanho
    /// absoluto, não uma contagem; `FONT_ZOOM_STEP_PX` é `1.0`, então a
    /// conversão é exata. Zoom é do processo inteiro hoje (dívida
    /// registrada em `font_zoom_px`), então o mesmo valor entra em toda
    /// janela do envelope.
    fn session_zoom_steps(&self) -> i32 {
        let base = self.config.terminal.font.size as f32;
        session_writer::zoom_px_to_steps(self.font_zoom_px, base, FONT_ZOOM_STEP_PX)
    }

    /// RF-3.1/RF-5.9/RF-5.21 (ADR-0036 §3, metade de restauração): tema e
    /// zoom de sessão são do processo (dívida registrada em
    /// `font_zoom_px`), mas o DTO é por janela -- toma a **primeira**
    /// janela do envelope como o estado do processo inteiro. Se duas
    /// janelas gravaram valores diferentes (usuário ciclou/zoomou uma sem
    /// tocar a outra antes de sair), a divergência não sobrevive à
    /// próxima gravação, que já grava as duas iguais -- aceito, é a
    /// mesma simplificação de `session_zoom_steps`. Chamado por
    /// `resumed`, **antes** de qualquer janela ser criada: `font_zoom_px`
    /// decide a métrica de célula que `create_window_with_attributes` usa
    /// para todas.
    fn apply_restored_session_state(&mut self, session: &porecatu_session::SessionFileV1) {
        self.shell_integration_dismissed = session.shell_integration_dismissed;
        let Some(first) = session.windows.first() else {
            return;
        };
        let base = self.config.terminal.font.size as f32;
        self.font_zoom_px = session_writer::steps_to_zoom_px(
            first.zoom_steps,
            base,
            FONT_ZOOM_STEP_PX,
            FONT_ZOOM_MIN_PX,
            FONT_ZOOM_MAX_PX,
        );
        // ADR-0031 §4: mesmo tratamento que o hot reload já dá a um tema
        // de sessão que sumiu -- zera e avisa, não aplica calado. O aviso
        // em si só tem onde aparecer depois que a primeira janela existir
        // (`resumed` o empilha ali, junto dos avisos de recuperação).
        let declared: Vec<&str> = self.config.themes.iter().map(|t| t.name.as_str()).collect();
        match session_writer::restore_theme(first.theme.as_deref(), &declared) {
            session_writer::RestoredTheme::Applied(theme) => self.session_theme = theme,
            session_writer::RestoredTheme::Vanished(name) => {
                self.vanished_restored_theme = Some(name);
            }
        }
        self.recompute_palettes();
    }

    /// Monta o envelope multi-janela (RF-3.17, metade de gravação) a
    /// partir das janelas dadas -- `self.windows.values()` no caminho
    /// comum (debounce), uma única janela no caminho de saída
    /// (`close_window_unconditionally`, que já a removeu do mapa).
    fn build_session_file<'a>(
        &self,
        windows: impl Iterator<Item = &'a WindowState>,
    ) -> porecatu_session::SessionFileV1 {
        let theme = self.session_theme.clone();
        let zoom_steps = self.session_zoom_steps();
        porecatu_session::SessionFileV1 {
            schema_version: porecatu_session::CURRENT_SCHEMA_VERSION,
            windows: windows
                .map(|state| {
                    session_writer::window_v1(
                        &state.workspace,
                        session_writer::window_geometry(&state.window),
                        session_writer::window_monitor(&state.window),
                        theme.clone(),
                        zoom_steps,
                        |tab_id| {
                            state.tabs.get(&tab_id).and_then(|rt| {
                                rt.terminal.cwd_fallback().or_else(|| rt.spawn_cwd.clone())
                            })
                        },
                    )
                })
                .collect(),
            shell_integration_dismissed: self.shell_integration_dismissed,
        }
    }

    /// Grava a sessão de verdade (debounce ou exit). RF-3.6: no-op se
    /// desligado. Falha de I/O nunca propaga -- no máximo `eprintln!`,
    /// nunca trava a saída nem abre diálogo (não há janela garantida
    /// nesse ponto no caminho de exit).
    fn write_session_now<'a>(&self, windows: impl Iterator<Item = &'a WindowState>) {
        if !self.session_persistence_enabled() {
            return;
        }
        let file = self.build_session_file(windows);
        if let Err(err) = porecatu_session::save(&file) {
            eprintln!("porecatu: falha ao gravar sessão: {err}");
        }
    }

    /// Agenda o próximo despertar via `ControlFlow::WaitUntil` -- o
    /// temporizador da informação (ADR-0014), o atraso do tooltip
    /// (ADR-0019), o relógio de animação (ADR-0022) e o debounce de
    /// gravação de sessão (RF-3.3) marcam sujeira, não rodam loop nenhum:
    /// quando não há nada pendente em nenhuma janela, o event loop dorme
    /// de verdade (`ControlFlow::Wait`).
    fn schedule_next_wake(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        // RF-3.2: qualquer janela suja (re)agenda o debounce único do
        // processo -- RF-3.17, um arquivo para todas. Drenado aqui, não
        // no ponto de origem, porque é aqui que `Instant::now()` e
        // `config.session.save_debounce_ms` já estão à mão (a regra do
        // projeto: quem chama `Instant::now()` é `lib.rs`, nunca o
        // estado).
        if self.session_persistence_enabled() && self.windows.values().any(|w| w.session_dirty) {
            for state in self.windows.values_mut() {
                state.session_dirty = false;
            }
            let debounce = Duration::from_millis(self.config.session.save_debounce_ms);
            self.session.mark_dirty(now, debounce);
        }
        let next = self
            .windows
            .values()
            .filter_map(|w| w.next_wake(now))
            .chain(self.session.next_deadline())
            .chain(self.shell_integration_invite_deadline())
            .min();
        match next {
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    /// RF-3.1 (ADR-0039 §2), gatilho temporal do Windows: mais próxima
    /// entre as abas ainda sem OSC 7 de `spawned_at + WINDOWS_SHELL_
    /// INTEGRATION_INVITE_INTERVAL`. `None` fora do Windows (lá o gatilho
    /// é o fallback do ADR-0038, consultado só na gravação -- ver
    /// `check_shell_integration_fallback_trigger`) ou quando o convite já
    /// foi escolhido/dispensado/desligado -- não há por que a janela
    /// continuar despertando por isto depois disso.
    #[cfg(windows)]
    fn shell_integration_invite_deadline(&self) -> Option<Instant> {
        if self.shell_integration_dismissed
            || self.shell_integration_invite_claimed
            || !self.config.session.suggest_shell_integration
        {
            return None;
        }
        self.windows
            .values()
            .flat_map(|w| w.tabs.values())
            .filter(|rt| !rt.received_osc7)
            .map(|rt| rt.spawned_at + WINDOWS_SHELL_INTEGRATION_INVITE_INTERVAL)
            .min()
    }

    #[cfg(not(windows))]
    fn shell_integration_invite_deadline(&self) -> Option<Instant> {
        None
    }

    /// Roda o `tick` (expira avisos, promove tooltip pendente, avança/
    /// remove animação) em todas as janelas e redesenha as que mudaram --
    /// ou que têm animação em curso, já que ela precisa de um frame por
    /// intervalo enquanto ativa (ADR-0022). Também dispara a gravação de
    /// sessão debounçada (RF-3.3), se o período de silêncio terminou.
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
        self.check_shell_integration_invite_timeout(now);
        if self.session.ready(now) {
            // RF-3.1 (ADR-0038 §5, ADR-0039 §2): fora do Windows, o
            // gatilho do convite é o mesmo sinal que decide consultar o
            // fallback -- roda só aqui, no momento da gravação, nunca por
            // frame. Antes de montar o arquivo: se o convite disparar, a
            // nota já entra no grid antes de `write_session_now` ler as
            // abas, mas a gravação em si não depende da nota (o campo é
            // `self.shell_integration_dismissed`, não o texto do grid).
            self.check_shell_integration_fallback_trigger();
            self.write_session_now(self.windows.values());
        }
    }

    /// RF-3.1 (ADR-0038 §5, ADR-0039 §2): gatilho não-Windows. Roda só na
    /// gravação da sessão -- para cada aba sem OSC 7 ainda, consulta o
    /// fallback do ADR-0038; se ele devolver um `cwd` diferente do de
    /// spawn, o diretório mudou e o app só soube pelo caminho caro, prova
    /// de que a aba não tem OSC 7. `cwd_fallback` devolve sempre `None`
    /// fora de Linux/macOS, então isto nunca dispara no Windows -- lá o
    /// gatilho é [`Self::shell_integration_invite_deadline`].
    fn check_shell_integration_fallback_trigger(&mut self) {
        if !shell_integration_invite_eligible(
            self.shell_integration_dismissed,
            self.shell_integration_invite_claimed,
            self.config.session.suggest_shell_integration,
        ) {
            return;
        }
        let candidate = self.windows.iter().find_map(|(window_id, state)| {
            state.tabs.iter().find_map(|(tab_id, rt)| {
                let fallback = rt.terminal.cwd_fallback();
                fallback_reveals_missing_osc7(
                    rt.received_osc7,
                    fallback.as_deref(),
                    rt.spawn_cwd.as_deref(),
                )
                .then_some((*window_id, *tab_id))
            })
        });
        if let Some((window_id, tab_id)) = candidate {
            self.claim_shell_integration_invite(window_id, tab_id);
        }
    }

    /// RF-3.1 (ADR-0039 §2), gatilho temporal do Windows: tick-driven
    /// (`schedule_next_wake` já agenda o `ControlFlow::WaitUntil` exato
    /// via `shell_integration_invite_deadline`) -- aqui só confirma que o
    /// critério ainda vale no instante em que o despertar chegou (a aba
    /// pode ter recebido OSC 7 entre o agendamento e agora).
    #[cfg(windows)]
    fn check_shell_integration_invite_timeout(&mut self, now: Instant) {
        if !shell_integration_invite_eligible(
            self.shell_integration_dismissed,
            self.shell_integration_invite_claimed,
            self.config.session.suggest_shell_integration,
        ) {
            return;
        }
        let candidate = self.windows.iter().find_map(|(window_id, state)| {
            state.tabs.iter().find_map(|(tab_id, rt)| {
                windows_invite_timeout_due(
                    rt.received_osc7,
                    rt.spawned_at,
                    now,
                    WINDOWS_SHELL_INTEGRATION_INVITE_INTERVAL,
                )
                .then_some((*window_id, *tab_id))
            })
        });
        if let Some((window_id, tab_id)) = candidate {
            self.claim_shell_integration_invite(window_id, tab_id);
        }
    }

    #[cfg(not(windows))]
    fn check_shell_integration_invite_timeout(&mut self, _now: Instant) {}

    /// RF-3.1 (ADR-0039 §4, "uma vez por execução"): marca o convite como
    /// escolhido -- a partir daqui nenhum outro gatilho tenta de novo,
    /// mesmo que a escrita em si seja adiada pela tela alternativa
    /// (ADR-0039 §1).
    fn claim_shell_integration_invite(&mut self, window_id: WindowId, tab_id: TabId) {
        self.shell_integration_invite_claimed = true;
        let alt_screen_active = self
            .windows
            .get(&window_id)
            .and_then(|s| s.tabs.get(&tab_id))
            .is_some_and(|rt| rt.terminal.modes().alt_screen);
        match shell_integration_note_timing(alt_screen_active) {
            ShellIntegrationNoteTiming::Defer => {
                self.pending_shell_integration_note = Some((window_id, tab_id));
            }
            ShellIntegrationNoteTiming::WriteNow => {
                self.write_shell_integration_note(window_id, tab_id)
            }
        }
    }

    /// Escreve a nota de verdade (`Terminal::inject_note`, canal 2 do
    /// ADR-0014) -- chamado só depois de confirmado que a aba não está em
    /// tela alternativa, seja na hora (`claim_shell_integration_invite`)
    /// seja depois, ao voltar dela (`try_write_pending_shell_integration_note`).
    fn write_shell_integration_note(&mut self, window_id: WindowId, tab_id: TabId) {
        self.pending_shell_integration_note = None;
        let Some(state) = self.windows.get(&window_id) else {
            return;
        };
        let Some(rt) = state.tabs.get(&tab_id) else {
            return;
        };
        let shell_name = state
            .workspace
            .tab(tab_id)
            .map(porecatu_core::Tab::shell_name)
            .unwrap_or_default();
        let text = shell_integration::invite_text(shell_name);
        rt.terminal.inject_note(&text, palette::NOTE_ACCENT_RGB);
    }

    /// Resolve um convite adiado por tela alternativa (ADR-0039 §1) --
    /// chamado a cada `Wakeup::TabDirty` da aba pendente, que é exatamente
    /// quando ela pode ter mudado de estado (`App::user_event`).
    fn try_write_pending_shell_integration_note(&mut self, window_id: WindowId, tab_id: TabId) {
        if self.pending_shell_integration_note != Some((window_id, tab_id)) {
            return;
        }
        let alt_screen_active = self
            .windows
            .get(&window_id)
            .and_then(|s| s.tabs.get(&tab_id))
            .is_some_and(|rt| rt.terminal.modes().alt_screen);
        if !alt_screen_active {
            self.write_shell_integration_note(window_id, tab_id);
        }
    }

    /// Aplica o resultado de uma recarga de config (F4 etapa 4,
    /// ADR-0030). Erro mantém a config anterior e só avisa (ADR-0003
    /// regra 2) -- sucesso troca o `Arc` inteiro (dono é o processo, não
    /// a janela) e aplica as classes A/B/C em todas as janelas.
    fn apply_config_reload(&mut self, outcome: ConfigReload, now: Instant) {
        let (new_config, unknown_keys) = match outcome {
            ConfigReload::Invalid { error } => {
                for state in self.windows.values_mut() {
                    state
                        .warnings
                        .push(Severity::Error, "Config inválida", error.to_string(), now);
                    state.window.request_redraw();
                }
                return;
            }
            ConfigReload::Loaded {
                config,
                unknown_keys,
            } => (config, unknown_keys),
        };

        let effects = reload::diff(&self.config, &new_config);
        self.config = Arc::new(*new_config);
        self.style = TabBarStyle::from_config(&self.config);

        // ADR-0031 §4: tema de sessão sobrevive à recarga enquanto
        // existir; se sumiu do arquivo, volta ao `theme` declarado (ou a
        // nenhum) e avisa qual sumiu -- mesmo raciocínio de "o arquivo é
        // do usuário" que o zoom de sessão já segue.
        let mut vanished_theme = None;
        if let Some(name) = &self.session_theme
            && !name.is_empty()
            && !self.config.themes.iter().any(|t| t.name == *name)
        {
            vanished_theme = self.session_theme.take();
        }
        // RF-4.22-like (ADR-0031 §5): `[terminal] theme` desconhecido é
        // aviso, não erro -- mesma severidade de chave desconhecida.
        let unknown_static_theme = (!self.config.terminal.theme.is_empty()
            && !self
                .config
                .themes
                .iter()
                .any(|t| t.name == self.config.terminal.theme))
        .then(|| self.config.terminal.theme.clone());
        let overridden = self.recompute_palettes();

        self.term_params = term_params_from_config(&self.config);
        // `[keybindings]` é classe A (ADR-0029 §3): o mapa novo vale
        // imediatamente. Um modo de captura em curso (rename, diálogo,
        // menu) não é afetado -- ele nem chega a consultar `keymap`,
        // porque a cadeia de captura do ADR-0008 intercepta antes.
        let keymap_resolved =
            keymap::resolve(&self.config.keybindings, keymap::Platform::current());
        self.keymap = keymap_resolved.bindings;

        // Classe B: recalcula a métrica de célula uma vez (ADR-0030: "a
        // métrica é a mesma [para toda janela]") -- a escala vem de uma
        // janela qualquer, mesma simplificação de `create_window`.
        // RF-5.9: o zoom da sessão sobrevive à recarga, como o tema
        // ciclado (ADR-0031 §4) -- mesmo raciocínio, "o arquivo é do
        // usuário, o app não desfaz uma escolha da sessão sozinho".
        let font_size_px = self.effective_font_size_px();
        let line_height_px = font_size_px * self.config.terminal.font.line_height as f32;
        if effects.grid_changed
            && let Some(gpu) = &mut self.gpu
        {
            let (cell_width, cell_height) = gpu
                .text_measurer()
                .measure_mono_cell(font_size_px, line_height_px);
            let scale = self.windows.values().next().map_or(1.0, |w| w.scale);
            self.cell_metrics = snap_cell_metrics_to_pixel_grid(cell_width, cell_height, scale);
        }

        let cell_metrics = self.cell_metrics;
        let style = self.style;
        let gpu = self.gpu.as_ref();
        let animations_enabled = self.config.appearance.window.animations;
        for state in self.windows.values_mut() {
            // Classe A (ADR-0022): `animations = false` desligada/ligada
            // na hora, sem esperar a próxima animação.
            state.animations.set_enabled(animations_enabled);
            // Classe B: colunas/linhas dependem da métrica nova e do
            // tamanho (por janela) -- resize de todos os PTYs da janela,
            // um por recarga (o debounce já coalesceu a rajada).
            if effects.grid_changed
                && let Some(gpu) = gpu
            {
                let size = state.window.inner_size();
                state.resize_to(size.width, size.height, gpu, cell_metrics, &style);
            }
            // Classe C: "mudei e não aconteceu nada" seria indistinguível
            // de bug (ADR-0030) -- por isso o aviso, severidade
            // informação, some sozinho.
            for message in &effects.deferred {
                state
                    .warnings
                    .push(Severity::Info, "Não aplicado agora", message.clone(), now);
            }
            // RF-4.22: chave desconhecida é aviso, não erro.
            for key in &unknown_keys {
                state.warnings.push(
                    Severity::Warning,
                    "Chave desconhecida na config",
                    key.clone(),
                    now,
                );
            }
            // ADR-0029 §4: tecla malformada, ação desconhecida ou
            // binding duplicado descartam só aquela linha de
            // `[keybindings]` -- o default embutido continua valendo,
            // e o resto do mapa aplica normalmente.
            for issue in &keymap_resolved.issues {
                state
                    .warnings
                    .push(Severity::Warning, "Keybinding inválido", issue.clone(), now);
            }
            // ADR-0031 §4/§5: tema de sessão que sumiu, tema declarado
            // desconhecido, e a cor fora do tema que "venceu" o tema
            // efetivo depois da recarga.
            if let Some(name) = &vanished_theme {
                state.warnings.push(
                    Severity::Info,
                    "Tema da sessão sumiu",
                    format!("\"{name}\" não existe mais no arquivo; voltando ao tema declarado."),
                    now,
                );
            }
            if let Some(name) = &unknown_static_theme {
                state.warnings.push(
                    Severity::Warning,
                    "Tema desconhecido",
                    format!("\"{name}\" não está em [[themes]]; usando defaults."),
                    now,
                );
            }
            if !overridden.is_empty() {
                state.warnings.push(
                    Severity::Info,
                    "Cor fora do tema vencendo",
                    overridden.join(", "),
                    now,
                );
            }
            // Classe A: o layout da barra é função pura de
            // `(Workspace, Config, largura)` -- só redesenhar já aplica.
            state.window.request_redraw();
        }
    }

    /// `config.reload` do catálogo (`docs/reference/acoes.md`, ADR-0003):
    /// relê o arquivo na hora, sem esperar o `notify` -- útil quando o
    /// watcher não disparou (editor que grava por outro caminho, arquivo
    /// em rede). Ligado a `ctrl+shift+comma`/`cmd+comma` desde a F4 etapa
    /// 5, via `ActionOutcome::ReloadConfig`.
    fn reload_config_now(&mut self, now: Instant) {
        let Some(path) = self.config_path.clone() else {
            return;
        };
        if let Some(outcome) = reload::read_and_parse(&path) {
            self.apply_config_reload(outcome, now);
        }
    }

    /// `font.increase`/`decrease`/`reset` (RF-5.9): recalcula a métrica de
    /// célula com o tamanho de sessão novo e redimensiona todos os PTYs
    /// de todas as janelas -- a mesma máquina da classe B do ADR-0030
    /// (etapas 3/4), acionada por atalho em vez de recarga. Nunca escreve
    /// no arquivo: `self.config` não muda, só `self.font_zoom_px`.
    fn apply_zoom(&mut self, delta: Zoom) {
        let base = self.config.terminal.font.size as f32;
        let current = self.effective_font_size_px();
        let new_size = match delta {
            Zoom::Increase => (current + FONT_ZOOM_STEP_PX).min(FONT_ZOOM_MAX_PX),
            Zoom::Decrease => (current - FONT_ZOOM_STEP_PX).max(FONT_ZOOM_MIN_PX),
            Zoom::Reset => base,
        };
        self.font_zoom_px = if new_size == base {
            None
        } else {
            Some(new_size)
        };

        if let Some(gpu) = &mut self.gpu {
            let font_size_px = self.font_zoom_px.unwrap_or(base);
            let line_height_px = font_size_px * self.config.terminal.font.line_height as f32;
            let (cell_width, cell_height) = gpu
                .text_measurer()
                .measure_mono_cell(font_size_px, line_height_px);
            let scale = self.windows.values().next().map_or(1.0, |w| w.scale);
            self.cell_metrics = snap_cell_metrics_to_pixel_grid(cell_width, cell_height, scale);
        }

        let cell_metrics = self.cell_metrics;
        let style = self.style;
        let gpu = self.gpu.as_ref();
        for state in self.windows.values_mut() {
            if let Some(gpu) = gpu {
                let size = state.window.inner_size();
                state.resize_to(size.width, size.height, gpu, cell_metrics, &style);
            }
            state.window.request_redraw();
        }
    }
}

impl ApplicationHandler<Wakeup> for App {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            self.tick_all();
        }
    }

    /// RF-3.7/RF-3.17 (F5 etapa 4): sessão gravada com ao menos uma janela
    /// restaura o conjunto inteiro; sem ela (arquivo ausente, `enabled =
    /// false`, ou `windows` vazio) cai no comportamento de sempre -- uma
    /// janela nova com uma aba no home. Os avisos de recuperação do
    /// ADR-0036 §5 (RF-3.14/RF-3.16) só têm onde aparecer depois que
    /// alguma janela existe -- por isso entram **depois** do bloco acima,
    /// na primeira janela que sobrou (as duas coisas nunca coincidem de
    /// verdade: `notices` não vem vazio junto com uma restauração bem
    /// sucedida, ver `porecatu_session::load_from`).
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        // RF-3.12/ADR-0040 §2: uma janela, uma aba, `cwd` no diretório
        // pedido -- vence `startup_directory` e nem consulta
        // `pending_session` (que já não foi carregado, ver `App::new`).
        // RF-11.24 é independente da sessão (a config é lida sempre, em
        // `App::new`) -- os avisos de arranque continuam sendo entregues
        // aqui, mesmo que a restauração de sessão nem rode neste modo.
        if let Some(dir) = self.positional_directory.clone() {
            self.open_window_with(event_loop, base_window_attributes(), Some(dir));
            self.deliver_pending_startup_warnings();
            return;
        }
        let outcome = self.pending_session.take().unwrap_or_default();
        match &outcome.session {
            Some(session) if !session.windows.is_empty() => {
                // RF-5.9/RF-5.21/RF-3.1 (ADR-0036 §3): tema, zoom e a
                // dispensa do convite entram **antes** de abrir qualquer
                // janela -- `font_zoom_px` decide a métrica de célula que
                // todas elas vão usar (ver o comentário do método).
                self.apply_restored_session_state(session);
                for window_v1 in &session.windows {
                    self.open_window_from_session(event_loop, window_v1);
                }
            }
            _ => self.open_window(event_loop, None),
        }
        if (!outcome.notices.is_empty() || self.vanished_restored_theme.is_some())
            && let Some(state) = self.windows.values_mut().next()
        {
            let now = Instant::now();
            for notice in &outcome.notices {
                let (title, body) = session_notice_text(notice);
                state.warnings.push(Severity::Error, title, body, now);
            }
            if let Some(name) = self.vanished_restored_theme.take() {
                state.warnings.push(
                    Severity::Info,
                    "Tema da sessão sumiu",
                    format!("\"{name}\" não existe mais no arquivo; usando o tema declarado."),
                    now,
                );
            }
            state.window.request_redraw();
        }
        self.deliver_pending_startup_warnings();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Wakeup) {
        let (window, tab_id) = match event {
            Wakeup::TabDirty { window, tab } => (window, tab),
            Wakeup::ConfigReloaded(outcome) => {
                self.apply_config_reload(*outcome, Instant::now());
                return;
            }
        };
        let Some(state) = self.windows.get_mut(&window) else {
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
                    // RF-3.1 (ADR-0039 §2): sinal de verdade de que esta
                    // instância tem OSC 7 -- diferente de `Tab::cwd()`,
                    // que uma aba nova já pode ter por herança (ver o
                    // comentário do campo em `TabRuntime`).
                    if let Some(rt) = state.tabs.get_mut(&tab_id) {
                        rt.received_osc7 = true;
                    }
                    if let Some(tab) = state.workspace.tab_mut(tab_id) {
                        tab.set_cwd(cwd);
                        state.mark_session_dirty();
                    }
                }
                // RF-3.1 (ADR-0039 §4): marcador digitado pelo usuário.
                // Só tem efeito depois do convite ter sido escolhido para
                // aparecer nesta execução -- ver o campo em `TabRuntime`/
                // `App`; um shell que por acaso emita a mesma string
                // (extremamente improvável) não dispensa nada sem o
                // convite ter chegado a existir.
                TermEvent::ShellIntegrationDismiss => {
                    if self.shell_integration_invite_claimed {
                        self.shell_integration_dismissed = true;
                        state.mark_session_dirty();
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
                            // Aba `Exited` não é gravada (ADR-0036 §3): a
                            // sessão precisa de uma escrita nova pra ela
                            // parar de aparecer no arquivo.
                            state.mark_session_dirty();
                        }
                    }
                }
            }
        }

        if window_should_close {
            self.close_window_unconditionally(window, event_loop);
            return;
        }

        // ADR-0039 §1: qualquer atividade nesta aba é o momento certo de
        // reconsiderar um convite adiado por tela alternativa -- é
        // exatamente quando ela pode ter mudado de estado.
        self.try_write_pending_shell_integration_note(window, tab_id);

        let Some(state) = self.windows.get(&window) else {
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
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.mark_session_dirty(); // RF-3.2: redimensionar a janela
                }
                if let (Some(state), Some(gpu)) = (self.windows.get_mut(&window_id), &self.gpu) {
                    state.resize_to(size.width, size.height, gpu, self.cell_metrics, &self.style);
                }
            }
            WindowEvent::Moved(_) => {
                // RF-3.2: mover a janela. A geometria em si não é lida
                // daqui -- `session_writer::window_geometry` consulta
                // `winit` de novo, fresca, no momento da gravação.
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.mark_session_dirty();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.scale = scale_factor as f32;
                    let size = state.window.inner_size();
                    if let Some(gpu) = &self.gpu {
                        state.resize_to(
                            size.width,
                            size.height,
                            gpu,
                            self.cell_metrics,
                            &self.style,
                        );
                    }
                }
            }
            WindowEvent::Focused(false) => {
                // ADR-0019: tooltip some ao perder foco a janela. Menus,
                // editor de grupo e popover de destino: mesma regra do
                // ADR-0014/ADR-0023 ("perda de foco fecha").
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.focused = false;
                    state.hover.dismiss();
                    state.close_all_popovers();
                    // Alt-tab com botão físico pressionado nunca gera o
                    // Released correspondente -- sem isto, o estado ficava
                    // preso até o próximo evento de mouse.
                    state.mouse_button_down = None;
                    state.window.request_redraw();
                }
            }
            WindowEvent::Focused(true) => {
                // RF-5.24: cursor volta de vazado para cheio ao ganhar foco.
                if let Some(state) = self.windows.get_mut(&window_id) {
                    state.focused = true;
                    state.window.request_redraw();
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
        // ADR-0037 §2: roda uma vez por evento de janela, depois de
        // qualquer clique/atalho que possa ter ativado uma aba `NotStarted`
        // (clique na aba, `Ctrl+Tab`, `Ctrl+PageDown`, índice, passo de
        // grupo, expansão de grupo colapsado) e antes do próximo evento --
        // sem-op quando a aba ativa já está `Running`, o caso comum.
        if let Some(state) = self.windows.get_mut(&window_id) {
            state.ensure_active_tab_started(
                self.cell_metrics,
                &self.proxy,
                Instant::now(),
                &self.style,
                &self.term_params,
                &self.config.shell,
                &self.startup_directory,
            );
        }
        self.schedule_next_wake(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.schedule_next_wake(event_loop);
    }
}

impl App {
    /// RF-11.24: entrega `pending_startup_warnings` (montados em
    /// `App::new`, antes de qualquer janela existir) à primeira janela do
    /// processo -- chamado nos dois caminhos de `resumed` (modo posicional
    /// e modo normal), porque a config é lida sempre, independente de
    /// sessão. Sem efeito se a lista já foi drenada (`resumed` só roda uma
    /// vez de verdade: a guarda `!self.windows.is_empty()` no topo garante
    /// isso).
    fn deliver_pending_startup_warnings(&mut self) {
        if self.pending_startup_warnings.is_empty() {
            return;
        }
        let Some(state) = self.windows.values_mut().next() else {
            return;
        };
        let now = Instant::now();
        for (severity, title, body) in self.pending_startup_warnings.drain(..) {
            state.warnings.push(severity, title, body, now);
        }
        state.window.request_redraw();
    }

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
                    &self.style,
                    &self.term_params,
                    &self.config.shell,
                );
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }
        if state.terminal_context_menu.is_some() {
            let menu = state.terminal_context_menu;
            if let Some(action) =
                state.handle_terminal_menu_key(&key, &self.style, self.cell_metrics)
                && let Some(menu) = menu
            {
                self.run_terminal_menu_action(window_id, menu, action);
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
                    &self.style,
                    &self.term_params,
                    &self.config.shell,
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

        // Barra de busca (ADR-0041 §3): captura **parcial**, a única não
        // modal do app. Reivindica só texto imprimível, `Enter`/`Shift+
        // Enter`/`Esc` e as teclas de edição do ADR-0035
        // (`handle_search_key`/`apply_text_field_key`) -- tudo o mais
        // (inclusive `F3`, que chega ao passo 2 abaixo) cai para a tabela
        // de keybindings normalmente, diferente dos modos acima.
        if state.search.is_some() && state.handle_search_key(&key, self.cell_metrics) {
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
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
            &self.style,
            &self.term_params,
            &self.config.shell,
            &self.keymap,
            self.config.general.confirm_close_with_process,
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
            ActionOutcome::ReloadConfig => {
                self.reload_config_now(Instant::now());
                if let Some(state) = self.windows.get(&window_id) {
                    state.window.request_redraw();
                }
            }
            ActionOutcome::Zoom(delta) => self.apply_zoom(delta),
            ActionOutcome::CycleTheme => self.cycle_theme(Instant::now()),
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
                        self.config.terminal.scrollback.scroll_on_input,
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
        let anchor_x = state.group_pill_screen_x(group, gpu, &self.style);
        let layout = overlay::layout_group_editor(
            anchor_x,
            bar_height(&self.style),
            &self.config,
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
                    let cfg = &self.config.appearance.group_editor;
                    let input_padding_x = cfg.input_padding_x as f32;
                    let input_font_size = cfg.input_font_size as f32;
                    let name_input_rect = layout.name_input_rect;
                    let buffer = editor.name_buffer().to_string();
                    let measurer = gpu.text_measurer();
                    let available_text_width =
                        (name_input_rect.width - input_padding_x * 2.0).max(0.0);
                    let text_width =
                        measurer.measure_width(&buffer, overlay::BODY_FONT, input_font_size);
                    let text_x = tab_bar::scrolled_text_x(
                        name_input_rect.x,
                        input_padding_x,
                        text_width,
                        available_text_width,
                    );
                    let local_x = logical_point.0 - text_x;
                    let byte_index = measurer.index_at_offset(
                        &buffer,
                        overlay::BODY_FONT,
                        input_font_size,
                        local_x,
                    );
                    editor.click_name_at(byte_index);
                    state.mouse_button_down = Some(button);
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
                    &self.style,
                    &self.term_params,
                    &self.config.shell,
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
            || state.terminal_context_menu.is_some()
            || state.group_editor.is_some()
        {
            return;
        }
        // ADR-0023: única lista rolável do chrome -- o tamanho é o número
        // de grupos do usuário, não conhecido em tempo de escrita, ao
        // contrário das listas de ação (que não rolam, §2.16). `overlay.rs`
        // deriva o deslocamento a partir de `highlighted`, então rolar é
        // só mover o realce -- mesmo efeito de `ArrowUp`/`ArrowDown`
        // (`handle_move_to_group_key`).
        if let Some(popover) = &mut state.move_to_group {
            let notches = match delta {
                MouseScrollDelta::LineDelta(_, y) => y,
                MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as f32,
            };
            if notches != 0.0 {
                popover.move_highlight(-notches.signum() as isize);
                state.window.request_redraw();
            }
            return;
        }
        if state.in_bar(state.cursor_position.1, &self.style) {
            // Espec §2.18: "roda do mouse sobre a barra rola a trilha na
            // horizontal, com ou sem Shift... passo de 90px por notch".
            let notches = match delta {
                MouseScrollDelta::LineDelta(_, y) => y,
                MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as f32,
            };
            if notches != 0.0 {
                state.scroll_offset -= notches.signum() * self.style.overflow_scroll_step;
                state.scroll_offset = state.scroll_offset.max(0.0);
                state.window.request_redraw();
            }
        } else if let Some(runtime) = state.active_runtime() {
            let cell = state.cell_at_cursor(self.cell_metrics, &self.style);
            input::handle_mouse_wheel(
                &runtime.terminal,
                &runtime.terminal.modes(),
                delta,
                state.modifiers,
                cell,
                self.config.terminal.scrollback.scroll_multiplier,
                self.config.terminal.scrollback.alternate_scroll,
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
            let layout = overlay::layout_context_menu(
                menu,
                &self.config,
                state.logical_width,
                state.logical_height,
            );
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
                &self.config,
                state.logical_width,
                state.logical_height,
            );
            if let Some(index) = overlay::group_menu_hit(&layout, logical_point) {
                menu.set_highlight(index);
            }
            state.window.request_redraw();
            return;
        }
        // Menu de contexto do terminal: mesmo tratamento dos dois acima.
        if let Some(menu) = &mut state.terminal_context_menu {
            let logical_point = (
                position.x as f32 / state.scale,
                position.y as f32 / state.scale,
            );
            let items = terminal_menu_context_items(
                &state.tabs,
                &self.style,
                self.cell_metrics,
                state.logical_width,
                state.logical_height,
                state.scale,
                menu.tab,
                menu.anchor,
            );
            let layout = overlay::layout_terminal_menu(
                menu,
                items.len(),
                &self.config,
                state.logical_width,
                state.logical_height,
            );
            if let Some(index) = overlay::terminal_menu_hit(&layout, logical_point) {
                menu.set_highlight(index, &items);
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
                let anchor_x = state.group_pill_screen_x(group, gpu, &self.style);
                if let Some(editor) = &mut state.group_editor {
                    let layout = overlay::layout_group_editor(
                        anchor_x,
                        bar_height(&self.style),
                        &self.config,
                        state.logical_width,
                        state.logical_height,
                    );
                    // Arraste dentro do campo de nome (ADR-0035): só entra
                    // aqui com o botão esquerdo já pressionado a partir de
                    // um clique no próprio campo (é o único lugar que arma
                    // `mouse_button_down` no editor de grupo, ver
                    // `dispatch_group_editor_click`) -- não conflita com o
                    // hover de swatch/ação abaixo, que não olha o botão.
                    if state.mouse_button_down == Some(MouseButton::Left)
                        && editor.focus() == EditorRegion::Name
                    {
                        let buffer = editor.name_buffer().to_string();
                        let cfg = &self.config.appearance.group_editor;
                        let input_padding_x = cfg.input_padding_x as f32;
                        let input_font_size = cfg.input_font_size as f32;
                        let name_input_rect = layout.name_input_rect;
                        let measurer = gpu.text_measurer();
                        let available_text_width =
                            (name_input_rect.width - input_padding_x * 2.0).max(0.0);
                        let text_width =
                            measurer.measure_width(&buffer, overlay::BODY_FONT, input_font_size);
                        let text_x = tab_bar::scrolled_text_x(
                            name_input_rect.x,
                            input_padding_x,
                            text_width,
                            available_text_width,
                        );
                        let local_x = logical_point.0 - text_x;
                        let byte_index = measurer.index_at_offset(
                            &buffer,
                            overlay::BODY_FONT,
                            input_font_size,
                            local_x,
                        );
                        editor.drag_name_to(byte_index);
                    } else {
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
            let layout = overlay::layout_move_to_group(
                popover,
                &self.config,
                state.logical_width,
                state.logical_height,
            );
            if let Some(index) = overlay::move_to_group_hit(&layout, logical_point) {
                popover.set_highlight(index);
            }
            state.window.request_redraw();
            return;
        }

        // Arraste dentro do campo da busca (ADR-0035/ADR-0041 §3): mesmo
        // padrão do editor de grupo -- só entra aqui com o botão esquerdo
        // já pressionado a partir de um clique no próprio campo (o único
        // lugar que arma `mouse_button_down` na busca).
        if state.mouse_button_down == Some(MouseButton::Left)
            && state.search.is_some()
            && let Some(gpu) = &mut self.gpu
        {
            let logical_point = (
                position.x as f32 / state.scale,
                position.y as f32 / state.scale,
            );
            let box_rect = paint::terminal_box_rect(
                &self.style,
                bar_height(&self.style),
                state.logical_width,
                state.logical_height,
            );
            let layout = search_bar::layout_search_bar(box_rect, &self.style);
            let measurer = gpu.text_measurer();
            if let Some(search) = state.search.as_mut() {
                let buffer = search.field().text().to_string();
                let text_width = measurer.measure_width(
                    &buffer,
                    chrome::LABEL_FONT,
                    search_bar::FIELD_FONT_SIZE,
                );
                let text_area =
                    (layout.field_rect.width - search_bar::FIELD_PADDING_X * 2.0).max(0.0);
                let text_x = tab_bar::scrolled_text_x(
                    layout.field_rect.x,
                    search_bar::FIELD_PADDING_X,
                    text_width,
                    text_area,
                );
                let local_x = logical_point.0 - text_x;
                let byte_index = measurer.index_at_offset(
                    &buffer,
                    chrome::LABEL_FONT,
                    search_bar::FIELD_FONT_SIZE,
                    local_x,
                );
                search.field_mut().drag_to(byte_index);
            }
            state.window.request_redraw();
            return;
        }

        // Arraste dentro do campo de rename (ADR-0035): estende a seleção
        // pelo caractere sob o cursor -- só quando o botão esquerdo já
        // está pressionado a partir de um clique dentro do próprio campo
        // (o único lugar que arma `mouse_button_down` durante o rename,
        // `handle_bar_click`).
        if state.mouse_button_down == Some(MouseButton::Left)
            && let Some(id) = state.rename.editing_tab()
            && let Some(gpu) = &mut self.gpu
        {
            let logical_point = (
                position.x as f32 / state.scale,
                position.y as f32 / state.scale,
            );
            let style = &self.style;
            let bar_width = state.logical_width;
            let trilha_width = tab_bar::trilha_width(style, bar_width, is_macos());
            let layout = tab_bar::fit_width(
                &state.workspace,
                style,
                trilha_width,
                gpu.text_measurer(),
                is_macos(),
            );
            if let Some(tab_rect) = tab_bar::tab_rect(&layout, id) {
                let field_content = chrome::rename_field_rect(tab_rect, style);
                let field_screen_x = field_content.x - state.scroll_offset;
                let buffer = state
                    .rename
                    .field()
                    .map(|f| f.text().to_string())
                    .unwrap_or_default();
                let text_area = (field_content.width - style.rename_padding_x * 2.0).max(0.0);
                let measurer = gpu.text_measurer();
                let text_width =
                    measurer.measure_width(&buffer, chrome::LABEL_FONT, style.rename_font_size);
                let text_x_screen = tab_bar::scrolled_text_x(
                    field_screen_x,
                    style.rename_padding_x,
                    text_width,
                    text_area,
                );
                let local_x = logical_point.0 - text_x_screen;
                let byte_index = measurer.index_at_offset(
                    &buffer,
                    chrome::LABEL_FONT,
                    style.rename_font_size,
                    local_x,
                );
                state.rename.drag_to(byte_index);
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
                    tab_bar::trilha_width(&self.style, state.logical_width, is_macos());
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

        // Cursor de resize por borda (ADR-0027): a janela inteira, não só
        // a barra -- `titlebar::resize_direction_at` já desliga sozinho
        // com a janela maximizada. Resolvido a cada `CursorMoved`, sempre
        // pro estado certo (ícone de resize ou `Default`): nada aqui
        // guarda "estava em resize antes", porque não há outro cursor
        // continuamente gerenciado no app pra colidir com isto (o resto
        // só muda cursor em transição de estado -- `Grabbing`/`Default` no
        // arraste).
        let resize_direction = titlebar::resize_direction_at(
            (
                position.x as f32 / state.scale,
                position.y as f32 / state.scale,
            ),
            state.logical_width,
            state.logical_height,
            state.window.is_maximized(),
            self.config.appearance.window_controls.resize_border as f32,
        );
        // Affordance de hyperlink (ADR-0042 §3, RF-11.11): "o cursor do
        // mouse muda de forma" -- resize de borda vence (não há como as
        // duas zonas colidirem na prática, mas a ordem segue a mesma
        // prioridade do `match` abaixo). Qualquer popover que capturaria o
        // clique também suprime isto -- mesma lista de `redraw`.
        let overlay_open = state.dialog.is_some()
            || state.context_menu.is_some()
            || state.group_context_menu.is_some()
            || state.terminal_context_menu.is_some()
            || state.group_editor.is_some()
            || state.move_to_group.is_some()
            || state.rename.editing_tab().is_some();
        let link_modifier = if is_macos() {
            state.modifiers.super_
        } else {
            state.modifiers.ctrl
        };
        let over_link = self.config.terminal.hyperlinks.enabled
            && link_modifier
            && resize_direction.is_none()
            && !overlay_open
            && !state.in_bar(position.y, &self.style)
            && state.active_runtime().is_some_and(|rt| {
                let content = paint::terminal_content_rect(
                    &self.style,
                    bar_height(&self.style),
                    state.logical_width,
                    state.logical_height,
                );
                let content_x = position.x - (content.x * state.scale) as f64;
                let content_y = position.y - (content.y * state.scale) as f64;
                let cell = input::cell_at(
                    content_x.max(0.0),
                    content_y.max(0.0),
                    self.cell_metrics,
                    rt.snapshot.rows.max(MIN_GRID),
                    rt.snapshot.cols.max(MIN_GRID),
                );
                hyperlink::uri_at(&rt.snapshot, cell.row, cell.col).is_some()
            });
        state.window.set_cursor(match resize_direction {
            Some(direction) => CursorIcon::from(direction),
            None if over_link => CursorIcon::Pointer,
            None => CursorIcon::Default,
        });

        if let Some(gpu) = &mut self.gpu {
            state.update_hover(gpu, Instant::now(), &self.config, &self.style);
        }

        if state.dialog.is_some() || state.context_menu.is_some() {
            return;
        }

        if !state.in_bar(position.y, &self.style)
            && let Some(runtime) = state.active_runtime()
        {
            let cell = state.cell_at_cursor(self.cell_metrics, &self.style);
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
            state.mouse_button_down = None;
            if button == MouseButton::Left && !matches!(state.drag, Drag::Idle) {
                if let Some(gpu) = &mut self.gpu {
                    state.finish_drag(gpu, &self.style);
                }
            } else if !state.in_bar(state.cursor_position.1, &self.style) {
                // Solta o botão sobre o terminal: repassa ao programa (SGR/X10
                // release) se ele pediu mouse reporting, senão é o fim de uma
                // seleção local -- mesmo caminho do press, ver lib.rs:2967+.
                let cell = state.cell_at_cursor(self.cell_metrics, &self.style);
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
                        self.config.terminal.selection.copy_on_select,
                    );
                }
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
                &self.config,
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
            let layout = overlay::layout_context_menu(
                &menu,
                &self.config,
                state.logical_width,
                state.logical_height,
            );
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
                &self.config,
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
                    &self.style,
                    &self.term_params,
                    &self.config.shell,
                );
            }
            if let Some(state) = self.windows.get(&window_id) {
                state.window.request_redraw();
            }
            return;
        }

        // Menu de contexto do terminal: mesmo padrão dos dois acima.
        if let Some(menu) = state.terminal_context_menu {
            let items = terminal_menu_context_items(
                &state.tabs,
                &self.style,
                self.cell_metrics,
                state.logical_width,
                state.logical_height,
                state.scale,
                menu.tab,
                menu.anchor,
            );
            let layout = overlay::layout_terminal_menu(
                &menu,
                items.len(),
                &self.config,
                state.logical_width,
                state.logical_height,
            );
            let hit = overlay::terminal_menu_hit(&layout, logical_point);
            state.terminal_context_menu = None;
            if button == MouseButton::Left
                && let Some(index) = hit
                && items[index].enabled
            {
                self.run_terminal_menu_action(window_id, menu, items[index].action);
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
            let layout = overlay::layout_move_to_group(
                popover,
                &self.config,
                state.logical_width,
                state.logical_height,
            );
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
        let warning_layout = overlay::layout_warnings(
            &state.warnings,
            &self.config,
            bar_height(&self.style),
            state.logical_width,
        );
        if let Some(hit) = overlay::hit_test_warnings(&warning_layout, logical_point) {
            if let overlay::WarningHit::Close(index) = hit {
                state.warnings.dismiss(index);
            }
            state.window.request_redraw();
            return;
        }

        // Barra de busca (ADR-0041 §3): captura parcial -- clique **dentro**
        // dela é dela (não é modal, mas a barra é opaca: o clique não vaza
        // pra grade por baixo); fora, cai pra barra de abas/grade como
        // sempre. Só o botão esquerdo age; qualquer outro é ignorado sem
        // fechar nada (mesma régua do editor de grupo).
        if let Some(search_tab) = state.search.as_ref().map(SearchBarState::tab) {
            let box_rect = paint::terminal_box_rect(
                &self.style,
                bar_height(&self.style),
                state.logical_width,
                state.logical_height,
            );
            let layout = search_bar::layout_search_bar(box_rect, &self.style);
            let inside_bar = logical_point.0 >= layout.bar_rect.x
                && logical_point.0 < layout.bar_rect.x + layout.bar_rect.width
                && logical_point.1 >= layout.bar_rect.y
                && logical_point.1 < layout.bar_rect.y + layout.bar_rect.height;
            if inside_bar {
                if button == MouseButton::Left {
                    match search_bar::search_bar_hit(&layout, logical_point) {
                        Some(SearchBarHit::Close) => state.search = None,
                        Some(SearchBarHit::Toggle) => {
                            if let Some(rt) = state.tabs.get(&search_tab)
                                && let Some(search) = state.search.as_mut()
                            {
                                search.toggle_regex(&rt.terminal, DEFAULT_SEARCH_LINES_PER_STEP);
                            }
                        }
                        Some(SearchBarHit::Prev) => {
                            state.advance_search(search_tab, false, self.cell_metrics);
                        }
                        Some(SearchBarHit::Next) => {
                            state.advance_search(search_tab, true, self.cell_metrics);
                        }
                        Some(SearchBarHit::Field) => {
                            if let Some(gpu) = &mut self.gpu {
                                let measurer = gpu.text_measurer();
                                if let Some(search) = state.search.as_mut() {
                                    let buffer = search.field().text().to_string();
                                    let text_width = measurer.measure_width(
                                        &buffer,
                                        chrome::LABEL_FONT,
                                        search_bar::FIELD_FONT_SIZE,
                                    );
                                    let text_area = (layout.field_rect.width
                                        - search_bar::FIELD_PADDING_X * 2.0)
                                        .max(0.0);
                                    let text_x = tab_bar::scrolled_text_x(
                                        layout.field_rect.x,
                                        search_bar::FIELD_PADDING_X,
                                        text_width,
                                        text_area,
                                    );
                                    let local_x = logical_point.0 - text_x;
                                    let byte_index = measurer.index_at_offset(
                                        &buffer,
                                        chrome::LABEL_FONT,
                                        search_bar::FIELD_FONT_SIZE,
                                        local_x,
                                    );
                                    search.field_mut().click_at(byte_index);
                                    state.mouse_button_down = Some(button);
                                }
                            }
                        }
                        None => {}
                    }
                }
                state.window.request_redraw();
                return;
            }
        }

        // Resize por borda (ADR-0027): janela inteira, fora de qualquer
        // popover/diálogo/menu (já resolvidos acima -- todos retornam
        // antes de chegar aqui). Mesma natureza bloqueante/modal de
        // `drag_window` (ver `WindowState::resolve_titlebar_drag`):
        // entrega o gesto ao loop não-client do SO, sem retorno síncrono.
        //
        // O canto onde o botão "fechar" encosta na borda direita
        // coincide com a zona de resize `NorthEast`/`East` -- o botão
        // vence: só considera resize se o ponto não está em cima de um
        // botão de janela (`handle_bar_click`, mais abaixo, resolve o
        // clique no botão do jeito de sempre).
        let over_window_button = tab_bar::point_in_window_button(
            &self.style,
            is_macos(),
            state.logical_width,
            bar_height(&self.style),
            logical_point,
        )
        .is_some();
        if button == MouseButton::Left
            && !over_window_button
            && let Some(direction) = titlebar::resize_direction_at(
                logical_point,
                state.logical_width,
                state.logical_height,
                state.window.is_maximized(),
                self.config.appearance.window_controls.resize_border as f32,
            )
        {
            let _ = state.window.drag_resize_window(direction);
            return;
        }

        if state.is_secondary_bar_click(button)
            && state.in_bar(state.cursor_position.1, &self.style)
        {
            state.hover.dismiss();
            if let Some(gpu) = &mut self.gpu {
                state.handle_bar_click(
                    logical_point,
                    gpu,
                    true,
                    &self.style,
                    self.config.general.confirm_close_with_process,
                );
            }
            state.window.request_redraw();
            return;
        }

        if button == MouseButton::Left && state.in_bar(state.cursor_position.1, &self.style) {
            state.hover.dismiss();
            let Some(gpu) = &mut self.gpu else {
                return;
            };
            match state.handle_bar_click(
                logical_point,
                gpu,
                false,
                &self.style,
                self.config.general.confirm_close_with_process,
            ) {
                NewTabRequest::Ungrouped => {
                    state.action_new_tab_ungrouped(
                        self.cell_metrics,
                        &self.proxy,
                        &self.startup_directory,
                        Instant::now(),
                        &self.style,
                        &self.term_params,
                        &self.config.shell,
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
                        &self.style,
                        &self.term_params,
                        &self.config.shell,
                    );
                    state.window.request_redraw();
                }
                // Pedido do usuário: fechar a última aba (sem grupo, sem
                // solta) fecha a janela sozinha -- `state` não é mais
                // válido depois disto, então nada de `request_redraw`.
                NewTabRequest::WindowEmptied => {
                    self.close_window_unconditionally(window_id, event_loop);
                }
                // ADR-0027: botão de fechar da janela -- mesmo caminho do
                // atalho/menu, com diálogo se houver mais de uma aba.
                NewTabRequest::CloseWindowRequested => {
                    self.request_close_window(window_id, event_loop);
                }
                NewTabRequest::None => {
                    state.window.request_redraw();
                }
            }
            return;
        }

        state.mouse_button_down = Some(button);
        if !state.in_bar(state.cursor_position.1, &self.style) {
            let cell = state.cell_at_cursor(self.cell_metrics, &self.style);
            let active_id = state.workspace.active_tab();

            // Affordance de hyperlink (ADR-0042 §2/§3): o mesmo modificador
            // que já desenha o sublinhado. Vence a prioridade do programa
            // como o `Shift` já vence pra seleção (ADR-0013) -- é a razão
            // de existir de um modificador dedicado: um clique com ele é
            // sempre "abrir o link", nunca reportado ao programa.
            let link_modifier = if is_macos() {
                state.modifiers.super_
            } else {
                state.modifiers.ctrl
            };
            if button == MouseButton::Left
                && self.config.terminal.hyperlinks.enabled
                && link_modifier
                && let Some(id) = active_id
                && let Some(uri) = state
                    .tabs
                    .get(&id)
                    .and_then(|rt| hyperlink::uri_at(&rt.snapshot, cell.row, cell.col))
                    .map(str::to_string)
            {
                let outcome = hyperlink::open_hyperlink(&uri);
                state.report_link_outcome(outcome, &uri, Instant::now());
                state.window.request_redraw();
                return;
            }

            // Clique secundário (RF-11.14): só quando o terminal não faria
            // nada com ele mesmo -- programa com mouse reporting pedido
            // (sem `Shift`) continua ganhando a prioridade de sempre
            // (ADR-0013), a mesma regra que `input::handle_mouse_button`
            // já aplica pros outros botões.
            let program_wants_mouse =
                active_id
                    .and_then(|id| state.tabs.get(&id))
                    .is_some_and(|rt| {
                        !state.modifiers.shift
                            && rt.terminal.modes().mouse_reporting != MouseReporting::None
                    });
            if button == MouseButton::Right
                && !program_wants_mouse
                && let Some(id) = active_id
            {
                state.close_all_popovers();
                let items = terminal_menu_context_items(
                    &state.tabs,
                    &self.style,
                    self.cell_metrics,
                    state.logical_width,
                    state.logical_height,
                    state.scale,
                    id,
                    logical_point,
                );
                state.terminal_context_menu =
                    Some(TerminalContextMenu::new(id, logical_point, &items));
                state.hover.dismiss();
                state.window.request_redraw();
                return;
            }

            if let Some(runtime) = active_id.and_then(|id| state.tabs.get(&id)) {
                input::handle_mouse_button(
                    &runtime.terminal,
                    &runtime.terminal.modes(),
                    button,
                    pressed,
                    cell,
                    state.modifiers,
                    &mut state.click_tracker,
                    self.config.terminal.selection.copy_on_select,
                );
            }
        }
        state.window.request_redraw();
    }

    fn redraw(&mut self, window_id: WindowId) {
        let font_size_px = self.effective_font_size_px();
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let mut frame = Frame::new();

        let style = &self.style;
        let pal = &self.pal;
        let config = &self.config;
        let bar_width = state.logical_width;
        let is_mac = is_macos();
        let trilha_width = tab_bar::trilha_width(style, bar_width, is_mac);
        let base_layout = tab_bar::fit_width(
            &state.workspace,
            style,
            bar_width,
            gpu.text_measurer(),
            is_mac,
        );
        let overflow = tab_bar::overflow_state(&base_layout, trilha_width, state.scroll_offset);
        state.scroll_offset = overflow.scroll_offset;

        // Hover dos botões de janela (ADR-0027): calculado do zero a cada
        // frame a partir da posição do cursor, mesmo padrão do resto do
        // hit-test da barra -- não é estado guardado, só a leitura de
        // `cursor_position` de sempre.
        let hover_window_button = if state.in_bar(state.cursor_position.1, style) {
            let cursor_logical = (
                state.cursor_position.0 as f32 / state.scale,
                state.cursor_position.1 as f32 / state.scale,
            );
            tab_bar::point_in_window_button(
                style,
                is_mac,
                bar_width,
                bar_height(style),
                cursor_logical,
            )
        } else {
            None
        };

        // Hover por brilho (F4 etapa 6, espec §1.10): mesmo padrão do
        // hover dos botões de janela acima -- calculado fresco a cada
        // frame a partir do cursor, nunca guardado. `None` durante
        // arraste: o alvo já tem o fantasma como feedback próprio, e a
        // pílula/aba de origem não deveria brilhar por baixo dele.
        let bar_hover =
            if matches!(state.drag, Drag::Idle) && state.in_bar(state.cursor_position.1, style) {
                let cursor_logical = (
                    state.cursor_position.0 as f32 / state.scale,
                    state.cursor_position.1 as f32 / state.scale,
                );
                let content_point = (cursor_logical.0 + state.scroll_offset, cursor_logical.1);
                tab_bar::hit_test(&base_layout, content_point)
            } else {
                None
            };

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
                tab_bar::fit_width(&preview, style, bar_width, gpu.text_measurer(), is_mac)
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
                tab_bar::fit_width(&preview, style, bar_width, gpu.text_measurer(), is_mac)
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
            style,
            pal,
            &self.term_pal,
            bar_width,
            overflow,
            drag_ghost,
            group_drag_ghost,
            drag_highlight,
            &state.animations,
            now,
            gpu.text_measurer(),
            is_mac,
            state.window.is_maximized(),
            hover_window_button,
            bar_hover,
        );
        frame.set_layer(Layer::Chrome, chrome_primitives);

        let h = bar_height(style);
        // A busca é por aba (RF-11.1) -- trocar de aba fecha a que estava
        // aberta, em vez de continuar mostrando o realce/estado de uma
        // aba que não está mais em tela.
        if let Some(search) = &state.search
            && Some(search.tab()) != state.workspace.active_tab()
        {
            state.search = None;
        }

        // Affordance de hyperlink (ADR-0042 §3): calculada do zero a cada
        // frame a partir do cursor e do modificador, mesmo padrão de
        // `bar_hover`/`hover_window_button` acima -- nada guardado, só
        // recomputado. `enabled = false` desliga a affordance junto com o
        // resto do recurso (ADR-0042 §5). Qualquer popover que capturaria
        // o clique também suprime o hover -- ele não seria acionável ali.
        let link_modifier = if is_mac {
            state.modifiers.super_
        } else {
            state.modifiers.ctrl
        };
        let hyperlink_overlay_blocks_hover = state.dialog.is_some()
            || state.context_menu.is_some()
            || state.group_context_menu.is_some()
            || state.terminal_context_menu.is_some()
            || state.group_editor.is_some()
            || state.move_to_group.is_some()
            || state.rename.editing_tab().is_some();
        let hovering_grid = !state.in_bar(state.cursor_position.1, style);
        let hover_content =
            paint::terminal_content_rect(style, h, state.logical_width, state.logical_height);
        let hover_content_x = state.cursor_position.0 - (hover_content.x * state.scale) as f64;
        let hover_content_y = state.cursor_position.1 - (hover_content.y * state.scale) as f64;

        if let Some(id) = state.workspace.active_tab()
            && let Some(runtime) = state.tabs.get_mut(&id)
        {
            runtime.terminal.snapshot_into(&mut runtime.snapshot);

            // Busca (ADR-0041): varre mais um lote por frame enquanto
            // houver, sem travar a UI (item 5 da etapa 1) -- pede outro
            // redraw imediatamente se ainda não terminou (não é o relógio
            // de animação do ADR-0022, que a §9 fecha em dois consumidores;
            // isto é o mesmo padrão de "saída do PTY continua chegando em
            // frames seguintes"). Corta as ocorrências pela vista e limpa
            // o buffer sem realocar (ADR-0007) -- `TermEngine::
            // snapshot_into` só zera (já aconteceu na chamada acima); quem
            // preenche é `porecatu-ui` (ADR-0041 §4).
            if let Some(search) = state.search.as_mut() {
                if search.step(&runtime.terminal) {
                    state.window.request_redraw();
                }
                let scroll_offset = runtime.snapshot.scroll_offset;
                let rows = runtime.snapshot.rows;
                search_bar::occurrences_in_view(
                    search.occurrences(),
                    search.active_index(),
                    scroll_offset,
                    rows,
                    &mut runtime.snapshot.occurrences,
                );
            }

            let box_rect =
                paint::terminal_box_rect(style, h, state.logical_width, state.logical_height);
            let cursor_config = &self.config.terminal.cursor;
            let cursor_color = if cursor_config.follows_group_color {
                active_group_color(&state.workspace, pal)
            } else {
                None
            }
            .unwrap_or(self.term_pal.cursor);
            let cursor = paint::CursorAppearance {
                color: cursor_color,
                width: cursor_config.width as f32,
                hollow: !state.focused && cursor_config.unfocused_hollow,
            };
            let hyperlink_hover: Vec<HyperlinkSpan> = if self.config.terminal.hyperlinks.enabled
                && link_modifier
                && hovering_grid
                && !hyperlink_overlay_blocks_hover
            {
                let cell = input::cell_at(
                    hover_content_x.max(0.0),
                    hover_content_y.max(0.0),
                    self.cell_metrics,
                    runtime.snapshot.rows.max(MIN_GRID),
                    runtime.snapshot.cols.max(MIN_GRID),
                );
                hyperlink::spans_sharing_id_at(&runtime.snapshot, cell.row, cell.col)
            } else {
                Vec::new()
            };
            let primitives = paint::build_primitives(
                &runtime.snapshot,
                self.cell_metrics,
                font_size_px,
                box_rect,
                style,
                &self.term_pal,
                cursor,
                gpu.text_measurer(),
                &hyperlink_hover,
            );
            frame.set_layer(Layer::Grid, primitives);

            if let Some(search) = &state.search {
                let search_layout = search_bar::layout_search_bar(box_rect, style);
                let cursor_logical = (
                    state.cursor_position.0 as f32 / state.scale,
                    state.cursor_position.1 as f32 / state.scale,
                );
                let search_hover = search_bar::search_bar_hit(&search_layout, cursor_logical);
                let search_primitives = search_bar::paint_search_bar(
                    &search_layout,
                    search,
                    style,
                    pal,
                    &self.term_pal,
                    search_hover,
                    gpu.text_measurer(),
                );
                for primitive in search_primitives {
                    frame.push(Layer::Chrome, primitive);
                }
            }
        }

        if !state.warnings.is_empty() {
            let warning_layout = overlay::layout_warnings(&state.warnings, config, h, bar_width);
            let primitives = overlay::paint_warnings(
                &warning_layout,
                &state.warnings,
                config,
                style,
                pal,
                gpu.text_measurer(),
            );
            frame.set_layer(Layer::Warning, primitives);
        }

        let mut popover = Vec::new();
        if let Some((anchor, text)) = state.hover.visible() {
            popover.extend(overlay::paint_tooltip(
                anchor,
                text,
                config,
                pal,
                state.logical_width,
                state.logical_height,
                gpu.text_measurer(),
            ));
        }
        if let Some(menu) = &state.context_menu {
            let layout = overlay::layout_context_menu(
                menu,
                config,
                state.logical_width,
                state.logical_height,
            );
            popover.extend(overlay::paint_context_menu(&layout, menu, config, pal));
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
                config,
                state.logical_width,
                state.logical_height,
            );
            popover.extend(overlay::paint_group_menu(
                &layout,
                &items,
                menu.highlighted(),
                config,
                pal,
            ));
        }
        if let Some(menu) = &state.terminal_context_menu {
            let items = terminal_menu_context_items(
                &state.tabs,
                style,
                self.cell_metrics,
                state.logical_width,
                state.logical_height,
                state.scale,
                menu.tab,
                menu.anchor,
            );
            let layout = overlay::layout_terminal_menu(
                menu,
                items.len(),
                config,
                state.logical_width,
                state.logical_height,
            );
            popover.extend(overlay::paint_terminal_menu(
                &layout,
                &items,
                menu.highlighted(),
                config,
                pal,
            ));
        }
        if let Some(editor) = &state.group_editor {
            let anchor_x = state.group_pill_screen_x(editor.group, gpu, style);
            let layout = overlay::layout_group_editor(
                anchor_x,
                h,
                config,
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
                config,
                pal,
                &self.term_pal,
                gpu.text_measurer(),
            ));
        }
        if let Some(mv) = &state.move_to_group {
            let layout = overlay::layout_move_to_group(
                mv,
                config,
                state.logical_width,
                state.logical_height,
            );
            popover.extend(overlay::paint_move_to_group(
                &layout,
                mv,
                &state.workspace,
                config,
                pal,
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
                config,
                gpu.text_measurer(),
            );
            let primitives = overlay::paint_dialog(
                &layout,
                dialog,
                config,
                pal,
                state.logical_width,
                state.logical_height,
                gpu.text_measurer(),
            );
            frame.set_layer(Layer::Modal, primitives);
        }

        // Fundo da margem entre a borda da janela e o box arredondado do
        // terminal (`style.terminal_frame_margin`): precisa de uma cor
        // diferente da do box para o quadro aparecer -- a mesma da barra de
        // abas, já que os dois formam o "quadro" do app.
        state.window_surface.render(gpu, pal.bar_background, &frame);
    }
}

/// Abre a janela principal do Porecatu e roda o event loop até todas as
/// janelas fecharem (ADR-0015). `cli_config`/`cli_directory` vêm do parse
/// de `argv` em `src/main.rs` (ADR-0040 §4) -- `--config` e o caminho
/// posicional do RF-3.12, já validados lá (caminho posicional inexistente
/// ou que não é diretório nunca chega até aqui).
pub fn run(cli_config: Option<PathBuf>, cli_directory: Option<PathBuf>) {
    let event_loop = EventLoop::<Wakeup>::with_user_event()
        .build()
        .expect("falha ao criar event loop");
    let proxy = event_loop.create_proxy();
    let mut app = App::new(proxy, cli_config, cli_directory);
    event_loop
        .run_app(&mut app)
        .expect("event loop terminou com erro");
}
