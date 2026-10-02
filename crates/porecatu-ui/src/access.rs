// SPDX-License-Identifier: GPL-3.0-or-later

//! Árvore de acessibilidade (ADR-0043): projeção **pura** do estado que já
//! produz o desenho -- reusa `tab_bar::layout`/`overflow_state` (as mesmas
//! funções que `chrome.rs` chama para pintar) em vez de montar uma segunda
//! fonte de verdade. Nada aqui toca `winit::window::Window`, `wgpu` nem
//! agenda redraw -- só monta `accesskit::TreeUpdate` a partir de
//! referências emprestadas. É essa ausência estrutural de qualquer
//! referência a janela/GPU que garante o §3 do ADR ("nunca dentro do
//! caminho de render, nunca como razão para redesenhar"): esta função não
//! tem como fazer isso, mesmo por engano.
//!
//! Escopo do que é montado, §4 do ADR: barra de abas completa (abas, pílula,
//! botões, overflow, configurações, janela), os cinco widgets (aviso,
//! diálogo, menu de contexto -- três instâncias concretas --, editor de
//! grupo, e o popover de mover-para-grupo, estruturalmente idêntico a um
//! menu) e a barra de busca. A grade do terminal fica fora (§5, RF-11.19) --
//! nenhuma função aqui recebe `GridSnapshot`.
//!
//! Simplificações registradas (não é ADR novo, é a mesma disciplina de
//! "dívida nomeada" que o ADR-0043 §5 usa para a grade): sem `bounds` por
//! nó (não essencial pra navegação por cursor virtual/teclado, que é o
//! caminho que o NVDA verifica), sem `TextSelection` de caractere nos
//! campos de texto (só `Value`) e sem despacho de `ActionRequest` de volta
//! ao app (a árvore expõe, não interage -- RF-11.17/18 pedem o primeiro).

use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use porecatu_core::{GroupColor, GroupId, PaneId, TabId, Workspace};
use porecatu_locale::Catalog;
use porecatu_render::TextMeasurer;

use crate::context_menu::{ContextMenu, TAB_MENU_ITEMS};
use crate::dialog::{ConfirmDialog, DialogButton};
use crate::group_editor::{EditorRegion, GroupEditor};
use crate::group_menu::{EDITOR_ACTION_ORDER, GroupContextMenu};
use crate::is_macos;
use crate::messages::msg;
use crate::move_to_group::MoveToGroupPopover;
use crate::search_bar::SearchBarState;
use crate::session_picker::{self, SessionPicker};
use crate::settings::{
    ControlView, FOOTER_BUTTONS, FooterButton, Group, Layout as SettingsLayout, RowView,
};
use crate::status_bar::{SegmentRole, StatusBarLayout};
use crate::tab_bar::{self, Indicator, TabBarStyle};
use crate::terminal_menu::{TerminalContextMenu, terminal_menu_items};
use crate::warning::{Severity, WarningStack};

/// Namespacing de `NodeId`: os fixos vivem abaixo de [`FIRST_DYNAMIC_ID`],
/// os derivados de identidade de domínio (aba, grupo, item de lista) vivem
/// acima, cada categoria num múltiplo que nunca colide com as outras --
/// nenhuma categoria chega perto de mil entradas.
const ROOT_ID: NodeId = NodeId(0);
const TAB_LIST_ID: NodeId = NodeId(1);
const UNGROUPED_NEW_TAB_ID: NodeId = NodeId(2);
const SETTINGS_BUTTON_ID: NodeId = NodeId(3);
const OVERFLOW_LEFT_ID: NodeId = NodeId(4);
const OVERFLOW_RIGHT_ID: NodeId = NodeId(5);
const WINDOW_MINIMIZE_ID: NodeId = NodeId(6);
const WINDOW_MAXIMIZE_ID: NodeId = NodeId(7);
const WINDOW_CLOSE_ID: NodeId = NodeId(8);
const SEARCH_BAR_ID: NodeId = NodeId(9);
const SEARCH_FIELD_ID: NodeId = NodeId(10);
const SEARCH_REGEX_TOGGLE_ID: NodeId = NodeId(11);
const WARNINGS_CONTAINER_ID: NodeId = NodeId(12);
const DIALOG_ID: NodeId = NodeId(13);
const DIALOG_CANCEL_ID: NodeId = NodeId(14);
const DIALOG_CONFIRM_ID: NodeId = NodeId(15);
const MENU_ID: NodeId = NodeId(16);
const GROUP_EDITOR_ID: NodeId = NodeId(17);
const GROUP_EDITOR_FIELD_ID: NodeId = NodeId(18);
const GROUP_EDITOR_SWATCHES_ID: NodeId = NodeId(19);
const GROUP_EDITOR_ACTIONS_ID: NodeId = NodeId(20);
const STATUS_BAR_ID: NodeId = NodeId(21);
const STATUS_BAR_FIRST_SEGMENT_ID: u64 = 22;
/// ADR-0054/ADR-0055: botão de sessões nomeadas. Longe da faixa dinâmica
/// de `STATUS_BAR_FIRST_SEGMENT_ID` (poucas unidades, um `id` por
/// segmento da barra de status) em vez de seguir logo depois dela, para
/// nunca colidir se essa faixa crescer.
const SESSIONS_BUTTON_ID: NodeId = NodeId(100);
/// ADR-0054/ADR-0055 §5: o popover em si -- mesma disciplina de
/// [`SESSIONS_BUTTON_ID`], longe da faixa dinâmica da barra de status.
const SESSION_PICKER_ID: NodeId = NodeId(101);
/// O item "Salvar esta janela..." em modo de edição vira o campo de
/// texto (mesmo molde de [`GROUP_EDITOR_FIELD_ID`]); em navegação, é
/// [`SESSION_PICKER_SAVE_ITEM_ID`] -- os dois nunca coexistem, então
/// dividir o `id` entre os dois estados não colide.
const SESSION_PICKER_FIELD_ID: NodeId = NodeId(102);
const SESSION_PICKER_SAVE_ITEM_ID: NodeId = NodeId(103);

const FIRST_DYNAMIC_ID: u64 = 1_000;
const TAB_STRIDE: u64 = 10;
const GROUP_STRIDE: u64 = 10;

fn tab_node_id(id: TabId) -> NodeId {
    NodeId(FIRST_DYNAMIC_ID + u64::from(id.get()) * TAB_STRIDE)
}

fn tab_close_button_id(id: TabId) -> NodeId {
    NodeId(FIRST_DYNAMIC_ID + u64::from(id.get()) * TAB_STRIDE + 1)
}

const GROUP_ID_BASE: u64 = 200_000;

fn group_pill_id(id: GroupId) -> NodeId {
    NodeId(GROUP_ID_BASE + u64::from(id.get()) * GROUP_STRIDE)
}

fn group_new_tab_id(id: GroupId) -> NodeId {
    NodeId(GROUP_ID_BASE + u64::from(id.get()) * GROUP_STRIDE + 1)
}

/// ADR-0053 §13: painéis viram nós filhos do nó da aba, numa faixa própria
/// -- mesma disciplina de [`TAB_STRIDE`]/[`GROUP_ID_BASE`]. `PaneId` só é
/// único **dentro** de uma aba (`id.rs`), então o `NodeId` precisa das
/// duas identidades: `PANE_TAB_STRIDE` reserva espaço de sobra por aba
/// para os painéis dela nunca colidirem com os da aba seguinte.
const PANE_ID_BASE: u64 = 600_000;
const PANE_TAB_STRIDE: u64 = 1_000;

fn pane_node_id(tab: TabId, pane: PaneId) -> NodeId {
    NodeId(PANE_ID_BASE + u64::from(tab.get()) * PANE_TAB_STRIDE + u64::from(pane.get()))
}

const WARNING_ITEM_BASE: u64 = 300_000;
const MENU_ITEM_BASE: u64 = 400_000;
const SWATCH_BASE: u64 = 500_000;
const EDITOR_ACTION_BASE: u64 = 500_100;
const MOVE_TARGET_BASE: u64 = 500_200;
const SESSION_PICKER_ROW_BASE: u64 = 500_300;

fn warning_item_id(index: usize) -> NodeId {
    NodeId(WARNING_ITEM_BASE + index as u64)
}

fn menu_item_id(index: usize) -> NodeId {
    NodeId(MENU_ITEM_BASE + index as u64)
}

fn swatch_id(index: usize) -> NodeId {
    NodeId(SWATCH_BASE + index as u64)
}

fn editor_action_id(index: usize) -> NodeId {
    NodeId(EDITOR_ACTION_BASE + index as u64)
}

fn move_target_id(index: usize) -> NodeId {
    NodeId(MOVE_TARGET_BASE + index as u64)
}

fn session_picker_row_id(index: usize) -> NodeId {
    NodeId(SESSION_PICKER_ROW_BASE + index as u64)
}

/// Nome falado da cor, do catálogo (tabela `color`) -- só rótulo acessível,
/// não valor de aparência (a regra do CLAUDE.md sobre "nenhuma cor
/// inventada" é sobre tokens visuais, não sobre o nome falado de uma cor já
/// escolhida).
fn color_name(catalog: &Catalog, color: GroupColor) -> String {
    match color {
        GroupColor::Red => msg::color::red(catalog),
        GroupColor::Yellow => msg::color::yellow(catalog),
        GroupColor::Cyan => msg::color::cyan(catalog),
        GroupColor::Blue => msg::color::blue(catalog),
        GroupColor::Purple => msg::color::purple(catalog),
        GroupColor::Green => msg::color::green(catalog),
    }
}

fn leaf(role: Role, label: impl Into<String>) -> Node {
    let mut node = Node::new(role);
    node.set_label(label.into());
    node
}

fn container(role: Role, children: Vec<NodeId>) -> Node {
    let mut node = Node::new(role);
    node.set_children(children);
    node
}

/// Entrada de todo o módulo: monta a árvore inteira do chrome de `state`,
/// no idioma `language` (BCP 47) do catálogo em uso,
/// sempre completa (nunca incremental) -- é o que `Adapter::update_if_
/// active` exige quando o adaptador foi criado com `with_event_loop_proxy`
/// (ver o comentário do próprio construtor).
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_tree(
    workspace: &Workspace,
    warnings: &WarningStack,
    dialog: &Option<ConfirmDialog>,
    context_menu: &Option<ContextMenu>,
    group_context_menu: &Option<GroupContextMenu>,
    terminal_context_menu: &Option<TerminalContextMenu>,
    group_editor: &Option<GroupEditor>,
    move_to_group: &Option<MoveToGroupPopover>,
    session_picker: &Option<SessionPicker>,
    search: &Option<SearchBarState>,
    status_bar: Option<&StatusBarLayout>,
    active_pane_order: Option<&[PaneId]>,
    style: &TabBarStyle,
    logical_width: f32,
    scroll_offset: f32,
    catalog: &Catalog,
    language: &str,
    measurer: &mut TextMeasurer,
) -> TreeUpdate {
    let is_mac = is_macos();
    let mut nodes: Vec<(NodeId, Node)> = Vec::new();
    let mut root_children: Vec<NodeId> = Vec::new();

    let trilha_width = tab_bar::trilha_width(style, logical_width, is_mac);
    let layout = tab_bar::layout(workspace, style, measurer);
    let overflow = tab_bar::overflow_state(&layout, trilha_width, scroll_offset);

    build_tab_list(
        workspace,
        &layout,
        active_pane_order,
        catalog,
        &mut nodes,
        &mut root_children,
    );

    if overflow.hidden_left > 0 {
        nodes.push((
            OVERFLOW_LEFT_ID,
            leaf(
                Role::Button,
                msg::access::tabs_hidden_left(catalog, overflow.hidden_left),
            ),
        ));
        root_children.push(OVERFLOW_LEFT_ID);
    }
    if overflow.hidden_right > 0 {
        nodes.push((
            OVERFLOW_RIGHT_ID,
            leaf(
                Role::Button,
                msg::access::tabs_hidden_right(catalog, overflow.hidden_right),
            ),
        ));
        root_children.push(OVERFLOW_RIGHT_ID);
    }

    if layout.ungrouped_new_tab_button.is_some() {
        nodes.push((
            UNGROUPED_NEW_TAB_ID,
            leaf(Role::Button, msg::access::new_tab_ungrouped(catalog)),
        ));
        root_children.push(UNGROUPED_NEW_TAB_ID);
    }

    // ADR-0054/ADR-0055: à esquerda da engrenagem na tela -- ordem do nó
    // na árvore segue a mesma ordem de leitura, como o resto da barra.
    nodes.push((
        SESSIONS_BUTTON_ID,
        leaf(Role::Button, msg::access::sessions_button(catalog)),
    ));
    root_children.push(SESSIONS_BUTTON_ID);

    nodes.push((
        SETTINGS_BUTTON_ID,
        leaf(Role::Button, msg::access::settings_button(catalog)),
    ));
    root_children.push(SETTINGS_BUTTON_ID);

    if !is_mac {
        nodes.push((
            WINDOW_MINIMIZE_ID,
            leaf(Role::Button, msg::access::window_minimize(catalog)),
        ));
        nodes.push((
            WINDOW_MAXIMIZE_ID,
            leaf(Role::Button, msg::access::window_maximize(catalog)),
        ));
        nodes.push((
            WINDOW_CLOSE_ID,
            leaf(Role::Button, msg::access::window_close(catalog)),
        ));
        root_children.push(WINDOW_MINIMIZE_ID);
        root_children.push(WINDOW_MAXIMIZE_ID);
        root_children.push(WINDOW_CLOSE_ID);
    }

    let mut focus = ROOT_ID;

    if let Some(state) = search {
        build_search_bar(state, catalog, &mut nodes, &mut root_children);
    }

    if let Some(layout) = status_bar {
        build_status_bar(layout, catalog, &mut nodes, &mut root_children);
    }

    if !warnings.is_empty() {
        build_warnings(warnings, catalog, &mut nodes, &mut root_children);
    }

    // No máximo um destes está `Some` de cada vez, por construção da
    // cadeia de captura do ADR-0008 -- mas a árvore só reflete o que
    // `state` de fato carrega, nunca presume exclusividade.
    if let Some(d) = dialog {
        focus = build_dialog(d, &mut nodes, &mut root_children);
    } else if let Some(m) = context_menu {
        focus = build_tab_menu(m, catalog, &mut nodes, &mut root_children);
    } else if let Some(m) = group_context_menu {
        focus = build_group_menu(m, workspace, catalog, &mut nodes, &mut root_children);
    } else if let Some(m) = terminal_context_menu {
        focus = build_terminal_menu(m, catalog, &mut nodes, &mut root_children);
    } else if let Some(e) = group_editor {
        focus = build_group_editor(e, workspace, catalog, &mut nodes, &mut root_children);
    } else if let Some(p) = move_to_group {
        focus = build_move_to_group(p, workspace, catalog, &mut nodes, &mut root_children);
    } else if let Some(p) = session_picker {
        focus = build_session_picker(p, catalog, &mut nodes, &mut root_children);
    }

    let mut root = Node::new(Role::Window);
    root.set_label("Porecatu");
    // ADR-0056 §11: sem o idioma na raiz, um leitor de tela em português
    // leria rótulos em inglês com a fonética portuguesa. `language` é o do
    // catálogo **efetivamente carregado**, em BCP 47.
    root.set_language(language);
    root.set_children(root_children);
    nodes.push((ROOT_ID, root));

    TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(ROOT_ID)),
        tree_id: TreeId::ROOT,
        focus,
    }
}

fn build_tab_list(
    workspace: &Workspace,
    layout: &tab_bar::TabBarLayout,
    active_pane_order: Option<&[PaneId]>,
    catalog: &Catalog,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) {
    let mut tab_list_children = Vec::new();
    let active_tab = workspace.active_tab();

    for group_wrapper in &layout.groups {
        let group = workspace.group(group_wrapper.id);

        if let Some(pill) = &group_wrapper.pill
            && let Some(group) = group
        {
            // Uma frase por combinação de colapsado e cor (a ordem das
            // palavras é do tradutor), em vez de pedaços colados.
            let name = group.name().unwrap_or(&pill.name);
            let label = match (group.is_collapsed(), group.color()) {
                (false, None) => msg::access::group(catalog, name),
                (true, None) => msg::access::group_collapsed(catalog, name),
                (false, Some(color)) => {
                    msg::access::group_colored(catalog, name, color_name(catalog, color))
                }
                (true, Some(color)) => {
                    msg::access::group_collapsed_colored(catalog, name, color_name(catalog, color))
                }
            };
            nodes.push((group_pill_id(group_wrapper.id), leaf(Role::Button, label)));
            tab_list_children.push(group_pill_id(group_wrapper.id));
        }

        for tab_rect in &group_wrapper.tabs {
            let Some(tab) = workspace.tab(tab_rect.id) else {
                continue;
            };
            let mut node = Node::new(Role::Tab);
            // O título vem primeiro; cada estado é uma peça do catálogo
            // (`access.state_*`) encaixada por `access.tab_state`, então a
            // ordem das palavras **dentro** de cada peça é do tradutor. A
            // ordem título-depois-estados e a soma de até quatro estados
            // ficam no código: as combinações passam de dez, e uma frase
            // por combinação seria pior que a ordem fixa.
            let mut label = tab.title().to_owned();
            let mut push_state = |state: String| {
                label.push_str(&msg::access::tab_state(catalog, state));
            };
            if Some(tab_rect.id) == active_tab {
                node.set_selected(true);
                push_state(msg::access::state_active(catalog));
            }
            if tab.is_not_started() {
                push_state(msg::access::state_not_started(catalog));
            }
            match tab_rect.indicator {
                Some(Indicator::Bell) => push_state(msg::access::state_bell(catalog)),
                Some(Indicator::Activity) => push_state(msg::access::state_activity(catalog)),
                None => {}
            }
            node.set_label(label);
            node.add_action(accesskit::Action::Focus);
            let close_id = tab_close_button_id(tab_rect.id);
            let mut children = vec![close_id];
            // ADR-0053 §13: painéis como filhos do nó da aba, projeção da
            // mesma ordem de travessia que `panes::layout` usa para
            // desenhar (nunca uma segunda descrição da árvore). Só para a
            // aba **ativa** -- é a única com painéis de verdade em tela; e
            // só com dois ou mais, mesma regra de ausência do segmento de
            // contagem da barra de status (RF-6.20): um painel só não diz
            // nada que o próprio nó da aba já não diga.
            if Some(tab_rect.id) == active_tab
                && let Some(order) = active_pane_order
                && order.len() > 1
            {
                let focused = tab.panes().focused_id();
                for &pane_id in order {
                    let Some(pane) = tab.panes().pane(pane_id) else {
                        continue;
                    };
                    let label = if pane_id == focused {
                        msg::access::pane_focused(catalog, pane.title())
                    } else {
                        pane.title().to_owned()
                    };
                    let id = pane_node_id(tab_rect.id, pane_id);
                    nodes.push((id, leaf(Role::GenericContainer, label)));
                    children.push(id);
                }
            }
            node.set_children(children);
            nodes.push((tab_node_id(tab_rect.id), node));
            nodes.push((
                close_id,
                leaf(Role::Button, msg::access::tab_close(catalog)),
            ));
            tab_list_children.push(tab_node_id(tab_rect.id));
        }

        if group_wrapper.new_tab_button.is_some() {
            let id = group_new_tab_id(group_wrapper.id);
            nodes.push((
                id,
                leaf(Role::Button, msg::access::new_tab_in_group(catalog)),
            ));
            tab_list_children.push(id);
        }
    }

    nodes.push((TAB_LIST_ID, container(Role::TabList, tab_list_children)));
    root_children.push(TAB_LIST_ID);
}

fn build_search_bar(
    state: &SearchBarState,
    catalog: &Catalog,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) {
    let (counter, _is_error) = state.counter_display(catalog);
    let mut field = Node::new(Role::SearchInput);
    field.set_value(state.field().text());
    if !counter.is_empty() {
        field.set_description(counter);
    }
    nodes.push((SEARCH_FIELD_ID, field));

    let mut toggle = Node::new(Role::Switch);
    toggle.set_label(msg::access::regex_toggle(catalog));
    toggle.set_toggled(state.is_regex().into());
    nodes.push((SEARCH_REGEX_TOGGLE_ID, toggle));

    nodes.push((
        SEARCH_BAR_ID,
        container(Role::Search, vec![SEARCH_FIELD_ID, SEARCH_REGEX_TOGGLE_ID]),
    ));
    root_children.push(SEARCH_BAR_ID);
}

/// Barra de status (ADR-0048 §11). Como todo o resto do chrome, é
/// **projeção do layout puro** -- os rótulos e a ordem saem do mesmo
/// `StatusBarLayout` que o pintor consome, nunca de uma segunda travessia
/// do estado: árvore construída à parte divergiria do desenho, e árvore
/// que mente é pior que ausente.
fn build_status_bar(
    layout: &StatusBarLayout,
    catalog: &Catalog,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) {
    let mut children = Vec::new();
    for (i, segment) in layout.segments.iter().enumerate() {
        let id = NodeId(STATUS_BAR_FIRST_SEGMENT_ID + i as u64);
        // ADR-0052 §9: o indicador de commits atrás/à frente é o
        // **primeiro nó de chrome com ação que não é da barra de abas** --
        // papel de botão, porque o que a cor e o sublinhado dizem (é
        // clicável, integra) não chega a quem não vê a tela.
        let role = match segment.role {
            SegmentRole::AheadBehind { .. } => Role::Button,
            _ => Role::Label,
        };
        let mut node = Node::new(role);
        node.set_value(segment.text.clone());
        node.set_label(segment_label(catalog, segment.role));
        // RF-9.4: sem isto, o leitor de tela lê o caminho como se fosse o
        // atual -- que é exatamente o mal-entendido que a barra existe
        // para desfazer. O alfa não chega a quem não vê a tela.
        if matches!(segment.role, SegmentRole::Cwd { stale: true }) {
            node.set_description(msg::access::cwd_stale(catalog));
        }
        if let SegmentRole::AheadBehind { clickable } = segment.role {
            node.set_description(if clickable {
                msg::access::ahead_behind_clickable(catalog)
            } else {
                msg::access::ahead_behind_blocked(catalog)
            });
        }
        nodes.push((id, node));
        children.push(id);
    }
    nodes.push((STATUS_BAR_ID, container(Role::Status, children)));
    root_children.push(STATUS_BAR_ID);
}

fn segment_label(catalog: &Catalog, role: SegmentRole) -> String {
    match role {
        SegmentRole::Shell => msg::access::segment_shell(catalog),
        SegmentRole::Cwd { .. } => msg::access::segment_cwd(catalog),
        SegmentRole::GitBranch => msg::access::segment_branch(catalog),
        SegmentRole::AheadBehind { .. } => msg::access::segment_ahead_behind(catalog),
        SegmentRole::Group => msg::access::segment_group(catalog),
        SegmentRole::PaneCount => msg::access::segment_pane_count(catalog),
        SegmentRole::Encoding => msg::access::segment_encoding(catalog),
        SegmentRole::System => msg::access::segment_system(catalog),
    }
}

fn build_warnings(
    warnings: &WarningStack,
    catalog: &Catalog,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) {
    let mut children = Vec::new();
    for (index, item) in warnings.items().iter().enumerate() {
        let severity = match item.severity {
            Severity::Error => msg::access::severity_error(catalog),
            Severity::Warning => msg::access::severity_warning(catalog),
            Severity::Info => msg::access::severity_info(catalog),
        };
        let id = warning_item_id(index);
        nodes.push((
            id,
            leaf(
                Role::Alert,
                msg::access::warning(catalog, severity, &item.title, &item.body),
            ),
        ));
        children.push(id);
    }
    nodes.push((
        WARNINGS_CONTAINER_ID,
        container(Role::GenericContainer, children),
    ));
    root_children.push(WARNINGS_CONTAINER_ID);
}

fn build_dialog(
    dialog: &ConfirmDialog,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) -> NodeId {
    // Qual dos dois está com foco não é propriedade de nó no accesskit --
    // é `TreeUpdate::focus`, devolvido por esta função (o valor de retorno
    // abaixo).
    let focused_id = match dialog.focused() {
        DialogButton::Cancel => DIALOG_CANCEL_ID,
        DialogButton::Confirm => DIALOG_CONFIRM_ID,
    };
    nodes.push((
        DIALOG_CANCEL_ID,
        leaf(Role::Button, dialog.cancel_label.clone()),
    ));
    nodes.push((
        DIALOG_CONFIRM_ID,
        leaf(Role::Button, dialog.confirm_label.clone()),
    ));

    let mut node = Node::new(Role::Dialog);
    node.set_modal();
    node.set_label(dialog.title.clone());
    node.set_description(dialog.body.clone());
    node.set_children(vec![DIALOG_CANCEL_ID, DIALOG_CONFIRM_ID]);
    nodes.push((DIALOG_ID, node));
    root_children.push(DIALOG_ID);
    focused_id
}

fn build_tab_menu(
    menu: &ContextMenu,
    catalog: &Catalog,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) -> NodeId {
    let mut children = Vec::new();
    let mut focus = MENU_ID;
    for (index, item) in TAB_MENU_ITEMS.iter().enumerate() {
        let id = menu_item_id(index);
        let mut node = Node::new(Role::MenuItem);
        node.set_label(item.action.label(catalog));
        if !item.enabled {
            node.set_disabled();
        }
        nodes.push((id, node));
        children.push(id);
        if index == menu.highlighted() {
            focus = id;
        }
    }
    nodes.push((MENU_ID, container(Role::Menu, children)));
    root_children.push(MENU_ID);
    focus
}

fn build_group_menu(
    menu: &GroupContextMenu,
    workspace: &Workspace,
    catalog: &Catalog,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) -> NodeId {
    let is_collapsed = workspace
        .group(menu.group)
        .is_some_and(|g| g.is_collapsed());
    let tab_count = workspace.group(menu.group).map_or(0, |g| g.tabs().len());
    let items = crate::group_menu::group_action_items(catalog, is_collapsed, tab_count);
    let mut children = Vec::new();
    let mut focus = MENU_ID;
    for (index, item) in items.iter().enumerate() {
        let id = menu_item_id(index);
        nodes.push((id, leaf(Role::MenuItem, item.label.clone())));
        children.push(id);
        if index == menu.highlighted() {
            focus = id;
        }
    }
    nodes.push((MENU_ID, container(Role::Menu, children)));
    root_children.push(MENU_ID);
    focus
}

fn build_terminal_menu(
    menu: &TerminalContextMenu,
    catalog: &Catalog,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) -> NodeId {
    // A composição real dos itens (com/sem seleção, com/sem link sob o
    // clique) depende de `Terminal`/hyperlink, que este módulo não tem à
    // mão -- usa a forma "sem seleção, sem link" como base estável; o rótulo
    // de cada item não muda por isso, só o estado habilitado de "Copiar"
    // poderia, e a árvore erraria esse único bit até a próxima ida por este
    // caminho com o estado certo (dívida registrada, sem risco de mentir
    // sobre o que existe -- só sobre um habilitado/desabilitado).
    let items = terminal_menu_items(false, false);
    let mut children = Vec::new();
    let mut focus = MENU_ID;
    for (index, item) in items.iter().enumerate() {
        let id = menu_item_id(index);
        let mut node = Node::new(Role::MenuItem);
        node.set_label(item.action.label(catalog));
        if !item.enabled {
            node.set_disabled();
        }
        nodes.push((id, node));
        children.push(id);
        if index == menu.highlighted() {
            focus = id;
        }
    }
    nodes.push((MENU_ID, container(Role::Menu, children)));
    root_children.push(MENU_ID);
    focus
}

fn build_group_editor(
    editor: &GroupEditor,
    workspace: &Workspace,
    catalog: &Catalog,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) -> NodeId {
    let mut field = Node::new(Role::TextInput);
    field.set_value(editor.name_buffer());
    nodes.push((GROUP_EDITOR_FIELD_ID, field));

    let mut swatch_children = Vec::new();
    for (index, color) in GroupColor::ALL.iter().enumerate() {
        let id = swatch_id(index);
        let mut node = Node::new(Role::RadioButton);
        node.set_label(color_name(catalog, *color));
        if index == editor.swatch_highlight() {
            node.set_toggled(accesskit::Toggled::True);
        }
        nodes.push((id, node));
        swatch_children.push(id);
    }
    nodes.push((
        GROUP_EDITOR_SWATCHES_ID,
        container(Role::RadioGroup, swatch_children),
    ));

    let mut action_children = Vec::new();
    for (index, action) in EDITOR_ACTION_ORDER.iter().enumerate() {
        let label = crate::group_menu::group_action_items(
            catalog,
            workspace
                .group(editor.group)
                .is_some_and(|g| g.is_collapsed()),
            workspace.group(editor.group).map_or(0, |g| g.tabs().len()),
        )
        .into_iter()
        .find(|item| item.action == *action)
        .map(|item| item.label)
        .unwrap_or_default();
        let id = editor_action_id(index);
        nodes.push((id, leaf(Role::MenuItem, label)));
        action_children.push(id);
    }
    nodes.push((
        GROUP_EDITOR_ACTIONS_ID,
        container(Role::Menu, action_children),
    ));

    let mut node = Node::new(Role::Group);
    node.set_label(msg::access::group_editor(catalog));
    node.set_children(vec![
        GROUP_EDITOR_FIELD_ID,
        GROUP_EDITOR_SWATCHES_ID,
        GROUP_EDITOR_ACTIONS_ID,
    ]);
    nodes.push((GROUP_EDITOR_ID, node));
    root_children.push(GROUP_EDITOR_ID);

    match editor.focus() {
        EditorRegion::Name => GROUP_EDITOR_FIELD_ID,
        EditorRegion::Swatches => swatch_id(editor.swatch_highlight()),
        EditorRegion::Actions => {
            editor_action_id(editor.action_highlight().min(EDITOR_ACTION_ORDER.len() - 1))
        }
    }
}

fn build_move_to_group(
    popover: &MoveToGroupPopover,
    workspace: &Workspace,
    catalog: &Catalog,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) -> NodeId {
    let mut children = Vec::new();
    let mut focus = MENU_ID;
    for (index, group_id) in popover.targets().iter().enumerate() {
        let name = workspace
            .group(*group_id)
            .and_then(porecatu_core::Group::name)
            .map_or_else(|| msg::access::unnamed_group(catalog), str::to_owned);
        let id = move_target_id(index);
        nodes.push((id, leaf(Role::MenuItem, name)));
        children.push(id);
        if index == popover.highlighted() {
            focus = id;
        }
    }
    let new_group_index = popover.targets().len();
    let id = move_target_id(new_group_index);
    nodes.push((
        id,
        leaf(Role::MenuItem, msg::move_to_group::new_group(catalog)),
    ));
    children.push(id);
    if popover.highlighted() == new_group_index {
        focus = id;
    }
    nodes.push((MENU_ID, container(Role::Menu, children)));
    root_children.push(MENU_ID);
    focus
}

/// ADR-0054/ADR-0055 §5: projeção do popover de sessões, a partir do
/// mesmo `SessionPicker` que `overlay::layout_session_picker`/
/// `paint_session_picker` consomem em `lib.rs` -- nunca uma segunda
/// árvore. Em modo de edição, o item de salvar vira o campo de texto
/// (mesmo molde de [`build_group_editor`]); em navegação, é um item de
/// menu com o rótulo fixo. Lista vazia projeta a linha "nenhuma sessão
/// salva" como um item sem alvo -- ela nunca é `Highlight::Row`
/// (`session_picker.rs`), então nunca ganha foco.
fn build_session_picker(
    picker: &SessionPicker,
    catalog: &Catalog,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) -> NodeId {
    let mut children = Vec::new();
    let mut focus = SESSION_PICKER_ID;

    let save_id = match picker.mode() {
        session_picker::Mode::EditingName(field) => {
            let mut node = Node::new(Role::TextInput);
            node.set_value(field.text());
            nodes.push((SESSION_PICKER_FIELD_ID, node));
            SESSION_PICKER_FIELD_ID
        }
        session_picker::Mode::Browsing => {
            nodes.push((
                SESSION_PICKER_SAVE_ITEM_ID,
                leaf(Role::MenuItem, msg::session_picker::save_item(catalog)),
            ));
            SESSION_PICKER_SAVE_ITEM_ID
        }
    };
    children.push(save_id);
    if picker.highlighted() == session_picker::Highlight::Save {
        focus = save_id;
    }

    if picker.entries().is_empty() {
        let id = session_picker_row_id(0);
        nodes.push((
            id,
            leaf(Role::MenuItem, msg::session_picker::empty_list(catalog)),
        ));
        children.push(id);
    } else {
        for (index, entry) in picker.entries().iter().enumerate() {
            let id = session_picker_row_id(index);
            nodes.push((id, leaf(Role::MenuItem, entry.name.clone())));
            children.push(id);
            if picker.highlighted() == session_picker::Highlight::Row(index) {
                focus = id;
            }
        }
    }

    nodes.push((SESSION_PICKER_ID, container(Role::Menu, children)));
    root_children.push(SESSION_PICKER_ID);
    focus
}

// ---- Janela de configurações (ADR-0059 §5)

/// Bloco de `NodeId` da janela de configurações: acima de tudo o que a árvore
/// da janela de terminal usa (a maior base dela é `PANE_ID_BASE` + 1_000 por
/// aba). Cada janela tem a sua árvore, então a colisão não seria um erro --
/// mas um bloco separado evita confundir um nó com o de outra janela em teste
/// e em log.
const SETTINGS_ID_BASE: u64 = 2_000_000;
const SETTINGS_ROOT_ID: NodeId = NodeId(SETTINGS_ID_BASE);
const SETTINGS_GROUP_LIST_ID: NodeId = NodeId(SETTINGS_ID_BASE + 1);
const SETTINGS_PANEL_ID: NodeId = NodeId(SETTINGS_ID_BASE + 2);
const SETTINGS_FOOTER_ID: NodeId = NodeId(SETTINGS_ID_BASE + 3);
const SETTINGS_FOOTER_BUTTON_BASE: u64 = SETTINGS_ID_BASE + 10;
const SETTINGS_WINDOW_MINIMIZE_ID: NodeId = NodeId(SETTINGS_ID_BASE + 20);
const SETTINGS_WINDOW_MAXIMIZE_ID: NodeId = NodeId(SETTINGS_ID_BASE + 21);
const SETTINGS_WINDOW_CLOSE_ID: NodeId = NodeId(SETTINGS_ID_BASE + 22);
const SETTINGS_GROUP_ITEM_BASE: u64 = SETTINGS_ID_BASE + 100;
/// Uma faixa de mil `NodeId` por linha de opção: o nó da linha, mais os dos
/// itens de uma lista ou das duas metades de `git.remote_poll_interval_secs`.
const SETTINGS_ROW_BASE: u64 = SETTINGS_ID_BASE + 10_000;
const SETTINGS_ROW_STRIDE: u64 = 1_000;

fn settings_group_item_id(index: usize) -> NodeId {
    NodeId(SETTINGS_GROUP_ITEM_BASE + index as u64)
}

fn settings_footer_button_id(button: FooterButton) -> NodeId {
    let index = FOOTER_BUTTONS
        .iter()
        .position(|candidate| *candidate == button)
        .expect("todo botão do rodapé está em FOOTER_BUTTONS");
    NodeId(SETTINGS_FOOTER_BUTTON_BASE + index as u64)
}

/// Monta a árvore da janela de configurações: projeção do mesmo `Layout` que
/// a pintura consome (ADR-0043 §2: árvore é projeção do layout, não uma
/// segunda descrição). Existem nesta etapa a janela -- com o idioma do
/// catálogo em uso --, a guia como lista com os nove grupos selecionáveis, o
/// painel e os três botões do rodapé; as opções entram com o layout delas.
///
/// `has_pending` diz se há alteração pendente: sem ela, Descartar e Salvar
/// são anunciados como indisponíveis (RF-16.14, "esmaecidos, nunca
/// ausentes"). `rows` são as linhas de opção do grupo em vista, as mesmas que
/// a pintura lê: cada uma vira um nó com o papel do controle, o nome, a
/// descrição inteira (a pintura a corta, a árvore não) e o valor.
pub(crate) fn build_settings_tree(
    layout: &SettingsLayout,
    groups: &[Group],
    selected: Group,
    has_pending: bool,
    rows: &[&RowView],
    catalog: &Catalog,
    language: &str,
) -> TreeUpdate {
    let mut nodes: Vec<(NodeId, Node)> = Vec::new();
    let mut root_children: Vec<NodeId> = Vec::new();

    // Onde a decoração é do sistema (macOS) não há cabeçalho nosso e, com
    // ele, não há botão de janela nosso.
    if layout.header.is_some() {
        for (id, label) in [
            (
                SETTINGS_WINDOW_MINIMIZE_ID,
                msg::access::window_minimize(catalog),
            ),
            (
                SETTINGS_WINDOW_MAXIMIZE_ID,
                msg::access::window_maximize(catalog),
            ),
            (SETTINGS_WINDOW_CLOSE_ID, msg::access::window_close(catalog)),
        ] {
            nodes.push((id, leaf(Role::Button, label)));
            root_children.push(id);
        }
    }

    let mut item_ids = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        let id = settings_group_item_id(index);
        let mut node = leaf(Role::ListBoxOption, group.label(catalog));
        node.set_selected(*group == selected);
        node.add_action(accesskit::Action::Focus);
        nodes.push((id, node));
        item_ids.push(id);
    }
    let mut list = container(Role::ListBox, item_ids);
    list.set_label(msg::access::settings_groups(catalog));
    nodes.push((SETTINGS_GROUP_LIST_ID, list));
    root_children.push(SETTINGS_GROUP_LIST_ID);

    let mut footer_children = Vec::new();
    for button in FOOTER_BUTTONS {
        let (label, available) = match button {
            FooterButton::OpenFile => (msg::settings::button::open_file(catalog), true),
            FooterButton::Discard => (msg::settings::button::discard(catalog), has_pending),
            FooterButton::Save => (msg::settings::button::save(catalog), has_pending),
        };
        let id = settings_footer_button_id(button);
        let mut node = leaf(Role::Button, label);
        if !available {
            node.set_disabled();
        }
        nodes.push((id, node));
        footer_children.push(id);
    }
    nodes.push((SETTINGS_FOOTER_ID, container(Role::Group, footer_children)));

    let mut panel_children = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        panel_children.push(settings_row_node(index, row, &mut nodes));
    }
    panel_children.push(SETTINGS_FOOTER_ID);

    // O painel é o do grupo escolhido -- o rótulo diz qual.
    let mut panel = container(Role::TabPanel, panel_children);
    panel.set_label(format!(
        "{}: {}",
        msg::access::settings_panel(catalog),
        selected.label(catalog)
    ));
    nodes.push((SETTINGS_PANEL_ID, panel));
    root_children.push(SETTINGS_PANEL_ID);

    let mut root = Node::new(Role::Window);
    root.set_label(msg::settings::window_title(catalog));
    // ADR-0056 §11: o idioma do catálogo **efetivamente carregado**.
    root.set_language(language);
    root.set_children(root_children);
    nodes.push((SETTINGS_ROOT_ID, root));

    TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(SETTINGS_ROOT_ID)),
        tree_id: TreeId::ROOT,
        focus: SETTINGS_ROOT_ID,
    }
}

/// Um nó por linha de opção, com o papel que o controle pede (ADR-0059 §5):
/// alternância é `Switch`, campo de texto `TextInput`, numérico `SpinButton`,
/// escolha `ComboBox`, lista `List`. Devolve o id do nó da linha e empilha os
/// nós dela em `nodes`.
fn settings_row_node(index: usize, row: &RowView, nodes: &mut Vec<(NodeId, Node)>) -> NodeId {
    let id = NodeId(SETTINGS_ROW_BASE + index as u64 * SETTINGS_ROW_STRIDE);
    // A descrição inteira, com o escopo de classe C depois dela: quem ouve o
    // leitor de tela não vê o corte nem a posição do escopo.
    let description = match &row.scope {
        Some((scope, _)) if !row.description_full.is_empty() => {
            format!("{} ({scope})", row.description_full)
        }
        Some((scope, _)) => scope.clone(),
        None => row.description_full.clone(),
    };
    let describe = |node: &mut Node| {
        if !description.is_empty() {
            node.set_description(description.clone());
        }
        // Valor recusado (RF-16.18): o leitor de tela o anuncia como inválido.
        if row.invalid.is_some() {
            node.set_invalid(accesskit::Invalid::True);
        }
    };
    match &row.control {
        ControlView::Toggle { on } => {
            let mut node = leaf(Role::Switch, row.name.clone());
            node.set_toggled(if *on {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            });
            describe(&mut node);
            nodes.push((id, node));
        }
        ControlView::Field {
            text,
            right_aligned,
            ..
        } => {
            let mut node = if *right_aligned {
                let mut node = leaf(Role::SpinButton, row.name.clone());
                if let Ok(number) = text.parse::<f64>() {
                    node.set_numeric_value(number);
                }
                node
            } else {
                leaf(Role::TextInput, row.name.clone())
            };
            node.set_value(text.clone());
            describe(&mut node);
            nodes.push((id, node));
        }
        ControlView::Segmented {
            labels, selected, ..
        } => {
            let mut node = leaf(Role::ComboBox, row.name.clone());
            node.set_value(labels.get(*selected).cloned().unwrap_or_default());
            describe(&mut node);
            nodes.push((id, node));
        }
        ControlView::Choice { text } => {
            let mut node = leaf(Role::ComboBox, row.name.clone());
            node.set_value(text.clone());
            describe(&mut node);
            nodes.push((id, node));
        }
        ControlView::List { items, .. } => {
            let mut children = Vec::new();
            for (item_index, item) in items.iter().enumerate() {
                let item_id = NodeId(id.0 + 1 + item_index as u64);
                nodes.push((item_id, leaf(Role::ListItem, item.clone())));
                children.push(item_id);
            }
            let mut node = container(Role::List, children);
            node.set_label(row.name.clone());
            describe(&mut node);
            nodes.push((id, node));
        }
        ControlView::Themes { selected, .. } => {
            let mut node = leaf(Role::ListBoxOption, row.name.clone());
            node.set_selected(*selected);
            nodes.push((id, node));
        }
        ControlView::GitPoll { on, seconds, .. } => {
            let switch_id = NodeId(id.0 + 1);
            let mut switch = leaf(Role::Switch, row.name.clone());
            switch.set_toggled(if *on {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            });
            nodes.push((switch_id, switch));
            let seconds_id = NodeId(id.0 + 2);
            let mut number = leaf(Role::SpinButton, row.name.clone());
            number.set_value(seconds.clone());
            if let Ok(value) = seconds.parse::<f64>() {
                number.set_numeric_value(value);
            }
            nodes.push((seconds_id, number));
            let mut node = container(Role::Group, vec![switch_id, seconds_id]);
            node.set_label(row.name.clone());
            describe(&mut node);
            nodes.push((id, node));
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use porecatu_core::{GroupColor, Workspace};

    use super::*;
    use crate::context_menu::{ContextMenu, MenuAction};
    use crate::dialog::DialogAction;

    fn measurer() -> TextMeasurer {
        TextMeasurer::new()
    }

    fn node(update: &TreeUpdate, id: NodeId) -> &Node {
        &update
            .nodes
            .iter()
            .find(|(n, _)| *n == id)
            .unwrap_or_else(|| panic!("nó {id:?} ausente da árvore"))
            .1
    }

    fn build(ws: &Workspace) -> TreeUpdate {
        build_in(ws, "pt-BR")
    }

    fn build_in(ws: &Workspace, language: &str) -> TreeUpdate {
        build_tree(
            ws,
            &WarningStack::default(),
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            None,
            None,
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &crate::messages::test_support::pt_br(),
            language,
            &mut measurer(),
        )
    }

    /// A afirmação central do ADR-0043 §3: montar a árvore é uma função
    /// pura de `(Workspace, ..., TextMeasurer)` -- nenhum parâmetro é uma
    /// janela ou um contexto de GPU, então não existe caminho por onde
    /// esta função possa pedir um frame ou desenhar algo. Chamá-la duas
    /// vezes com estado diferente e comparar a saída é o mais perto que dá
    /// de testar "não solicita frame" sem um `winit::window::Window` de
    /// verdade (fronteira que `WindowState`/`App` já não cruzam em teste
    /// nenhum do projeto).
    #[test]
    fn building_the_tree_never_touches_window_or_gpu() {
        let mut empty = Workspace::new();
        let tree_before = build(&empty);
        empty.append_tab("zsh", None);
        let tree_after = build(&empty);
        assert_ne!(
            tree_before, tree_after,
            "a árvore reflete a mudança de estado"
        );
    }

    /// ADR-0056 §11: a raiz declara o idioma do catálogo carregado, e a
    /// árvore montada de novo com outro idioma (a troca ao vivo) o atualiza.
    #[test]
    fn the_root_declares_the_language_of_the_loaded_catalog() {
        let ws = Workspace::new();
        assert_eq!(
            node(&build_in(&ws, "pt-BR"), ROOT_ID).language(),
            Some("pt-BR")
        );
        assert_eq!(
            node(&build_in(&ws, "en-US"), ROOT_ID).language(),
            Some("en-US")
        );
        // O rótulo da raiz é nome próprio: não muda com o idioma.
        assert_eq!(
            node(&build_in(&ws, "en-US"), ROOT_ID).label(),
            Some("Porecatu")
        );
    }

    #[test]
    fn tab_titles_and_active_state_are_exposed() {
        let mut ws = Workspace::new();
        // `append_tab` ativa a aba recém-criada -- a última é a ativa.
        ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        let update = build(&ws);
        let b_node = node(&update, tab_node_id(b));
        assert_eq!(b_node.role(), Role::Tab);
        assert!(b_node.label().unwrap().contains("bash"));
        assert!(b_node.is_selected().unwrap_or(false));
    }

    #[test]
    fn tab_list_order_matches_visual_order() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("first", None);
        let b = ws.append_tab("second", None);
        let update = build(&ws);
        let list = node(&update, TAB_LIST_ID);
        let children = list.children();
        let pos_a = children
            .iter()
            .position(|id| *id == tab_node_id(a))
            .unwrap();
        let pos_b = children
            .iter()
            .position(|id| *id == tab_node_id(b))
            .unwrap();
        assert!(pos_a < pos_b);
    }

    /// ADR-0053 §13: com dois ou mais painéis na aba ativa, cada um vira
    /// um nó filho do nó da aba, na ordem de `active_pane_order` (a mesma
    /// que `panes::layout` produz) -- e o focado carrega a marca no
    /// rótulo.
    #[test]
    fn active_tab_exposes_its_panes_as_children_with_focus_marked() {
        use porecatu_core::SplitAxis;

        let mut ws = Workspace::new();
        let tab_id = ws.append_tab("zsh", None);
        let left = ws.tab(tab_id).unwrap().panes().focused_id();
        let right = ws
            .tab_mut(tab_id)
            .unwrap()
            .panes_mut()
            .split(left, SplitAxis::Vertical, "bash", None)
            .unwrap();
        let order = [left, right];

        let update = build_tree(
            &ws,
            &WarningStack::default(),
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            None,
            Some(&order),
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &crate::messages::test_support::pt_br(),
            "pt-BR",
            &mut measurer(),
        );

        let tab_node = node(&update, tab_node_id(tab_id));
        let close_id = tab_close_button_id(tab_id);
        let pane_children: Vec<NodeId> = tab_node
            .children()
            .iter()
            .copied()
            .filter(|id| *id != close_id)
            .collect();
        assert_eq!(
            pane_children,
            vec![pane_node_id(tab_id, left), pane_node_id(tab_id, right)]
        );

        let focused_node = node(&update, pane_node_id(tab_id, right));
        assert!(focused_node.label().unwrap().contains("(foco)"));
        let unfocused_node = node(&update, pane_node_id(tab_id, left));
        assert!(!unfocused_node.label().unwrap().contains("(foco)"));
    }

    /// RF-6.20 (mesma regra do segmento de contagem na barra de status):
    /// um painel só não ganha nó filho -- o próprio nó da aba já diz tudo
    /// que haveria a dizer.
    #[test]
    fn a_single_pane_tab_gets_no_pane_children() {
        let mut ws = Workspace::new();
        let tab_id = ws.append_tab("zsh", None);
        let focused = ws.tab(tab_id).unwrap().panes().focused_id();
        let order = [focused];

        let update = build_tree(
            &ws,
            &WarningStack::default(),
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            None,
            Some(&order),
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &crate::messages::test_support::pt_br(),
            "pt-BR",
            &mut measurer(),
        );

        let tab_node = node(&update, tab_node_id(tab_id));
        assert_eq!(
            tab_node.children(),
            vec![tab_close_button_id(tab_id)],
            "sem painéis: só o botão de fechar"
        );
    }

    #[test]
    fn group_pill_names_color_and_collapsed_state() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("a", None);
        let group = ws.group_tabs(&[a], "api", GroupColor::Blue).unwrap();
        ws.set_group_color(group, GroupColor::Blue);
        let update = build(&ws);
        let pill = node(&update, group_pill_id(group));
        let label = pill.label().unwrap();
        assert!(label.contains("api"));
        assert!(label.contains("Azul"));
        assert!(!label.contains("colapsado"));
    }

    #[test]
    fn warnings_become_alert_nodes_with_severity_in_the_label() {
        let mut warnings = WarningStack::default();
        warnings.push(
            Severity::Error,
            "Config inválida",
            "detalhe",
            Instant::now(),
        );
        let ws = Workspace::new();
        let update = build_tree(
            &ws,
            &warnings,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            None,
            None,
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &crate::messages::test_support::pt_br(),
            "pt-BR",
            &mut measurer(),
        );
        let item = node(&update, warning_item_id(0));
        assert_eq!(item.role(), Role::Alert);
        let label = item.label().unwrap();
        assert!(label.starts_with("Erro:"));
        assert!(label.contains("Config inválida"));
    }

    #[test]
    fn status_bar_projects_the_layout_and_names_the_stale_cwd() {
        // ADR-0048 §11 e RF-9.4: o alfa que marca o diretório de origem
        // não chega a quem não vê a tela, então a distinção tem de estar
        // na descrição do nó -- sem ela, o leitor de tela apresenta um
        // caminho possivelmente velho como se fosse o atual.
        let ws = Workspace::new();
        let content = crate::status_bar::StatusBarContent {
            shell: "pwsh".to_owned(),
            cwd: "~/projetos".to_owned(),
            cwd_is_stale: true,
            git_branch: None,
            ahead_behind: None,
            group: None,
            pane_count_label: None,
            system: "windows - 0.7.0".to_owned(),
        };
        let layout = crate::status_bar::layout_status_bar(
            &content,
            &TabBarStyle::DEFAULT,
            800.0,
            600.0,
            &mut measurer(),
        );
        let update = build_tree(
            &ws,
            &WarningStack::default(),
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            Some(&layout),
            None,
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &crate::messages::test_support::pt_br(),
            "pt-BR",
            &mut measurer(),
        );

        let bar = node(&update, STATUS_BAR_ID);
        assert_eq!(bar.role(), Role::Status);
        assert_eq!(
            bar.children().len(),
            layout.segments.len(),
            "um nó por segmento desenhado, nem mais nem menos"
        );

        let cwd_index = layout
            .segments
            .iter()
            .position(|s| matches!(s.role, SegmentRole::Cwd { .. }))
            .expect("o diretório está no layout");
        let cwd = node(
            &update,
            NodeId(STATUS_BAR_FIRST_SEGMENT_ID + cwd_index as u64),
        );
        assert_eq!(cwd.label(), Some("diretório"));
        assert_eq!(cwd.value(), Some("~/projetos"));
        assert!(
            cwd.description()
                .is_some_and(|d| d.contains("não informa o atual")),
            "o RF-9.4 precisa ser audível, não só visível"
        );
    }

    #[test]
    fn ahead_behind_projects_as_a_button_with_the_count_in_the_description() {
        // ADR-0052 §9: primeiro nó de chrome com ação fora da barra de
        // abas -- o que a cor e o sublinhado dizem (é clicável, integra)
        // não chega a quem não vê a tela.
        let ws = Workspace::new();
        let content = crate::status_bar::StatusBarContent {
            git_branch: Some("main".to_owned()),
            ahead_behind: Some(crate::status_bar::AheadBehindContent {
                label: "3 commits atrás".to_owned(),
                clickable: true,
            }),
            ..Default::default()
        };
        let layout = crate::status_bar::layout_status_bar(
            &content,
            &TabBarStyle::DEFAULT,
            800.0,
            600.0,
            &mut measurer(),
        );
        let update = build_tree(
            &ws,
            &WarningStack::default(),
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            Some(&layout),
            None,
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &crate::messages::test_support::pt_br(),
            "pt-BR",
            &mut measurer(),
        );
        let index = layout
            .segments
            .iter()
            .position(|s| matches!(s.role, SegmentRole::AheadBehind { .. }))
            .expect("o indicador está no layout");
        let node = node(&update, NodeId(STATUS_BAR_FIRST_SEGMENT_ID + index as u64));
        assert_eq!(node.role(), Role::Button);
        assert_eq!(node.value(), Some("3 commits atrás"));
        assert!(
            node.description()
                .is_some_and(|d| d.contains("fast-forward"))
        );
    }

    #[test]
    fn fresh_cwd_carries_no_stale_description() {
        let ws = Workspace::new();
        let content = crate::status_bar::StatusBarContent {
            cwd: "~/projetos".to_owned(),
            cwd_is_stale: false,
            ..Default::default()
        };
        let layout = crate::status_bar::layout_status_bar(
            &content,
            &TabBarStyle::DEFAULT,
            800.0,
            600.0,
            &mut measurer(),
        );
        let update = build_tree(
            &ws,
            &WarningStack::default(),
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            Some(&layout),
            None,
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &crate::messages::test_support::pt_br(),
            "pt-BR",
            &mut measurer(),
        );
        let cwd_index = layout
            .segments
            .iter()
            .position(|s| matches!(s.role, SegmentRole::Cwd { .. }))
            .expect("o diretório está no layout");
        let cwd = node(
            &update,
            NodeId(STATUS_BAR_FIRST_SEGMENT_ID + cwd_index as u64),
        );
        assert_eq!(cwd.description(), None);
    }

    #[test]
    fn dialog_is_modal_and_focus_follows_the_focused_button() {
        let ws = Workspace::new();
        let dialog = Some(ConfirmDialog::new(
            "Fechar janela?",
            "Duas abas abertas.",
            "Fechar",
            "Cancelar",
            DialogAction::CloseWindow,
        ));
        let update = build_tree(
            &ws,
            &WarningStack::default(),
            &dialog,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            None,
            None,
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &crate::messages::test_support::pt_br(),
            "pt-BR",
            &mut measurer(),
        );
        let dialog_node = node(&update, DIALOG_ID);
        assert_eq!(dialog_node.role(), Role::Dialog);
        assert!(dialog_node.is_modal());
        // Foco inicial é o cancelar (ADR-0014).
        assert_eq!(update.focus, DIALOG_CANCEL_ID);
    }

    #[test]
    fn tab_menu_disabled_item_stays_in_the_tree_but_marked_disabled() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("a", None);
        let menu = Some(ContextMenu::new(a, (0.0, 0.0)));
        let update = build_tree(
            &ws,
            &WarningStack::default(),
            &None,
            &menu,
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            None,
            None,
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &crate::messages::test_support::pt_br(),
            "pt-BR",
            &mut measurer(),
        );
        let menu_node = node(&update, MENU_ID);
        assert_eq!(menu_node.role(), Role::Menu);
        assert_eq!(menu_node.children().len(), TAB_MENU_ITEMS.len());
        let move_index = TAB_MENU_ITEMS
            .iter()
            .position(|item| item.action == MenuAction::MoveToGroup)
            .unwrap();
        let move_item = node(&update, menu_item_id(move_index));
        assert_eq!(move_item.is_disabled(), !TAB_MENU_ITEMS[move_index].enabled);
    }

    /// ADR-0054/ADR-0055 §5: em navegação, o item de salvar é um item de
    /// menu com o rótulo fixo, e cada linha da lista vira um `MenuItem`
    /// com o nome da sessão -- a mesma lista que `overlay::
    /// layout_session_picker` desenharia, nunca uma segunda fonte.
    #[test]
    fn session_picker_browsing_projects_rows_and_focuses_the_highlighted_one() {
        use porecatu_session::named::{EntryStatus, NamedSessionEntry};
        use std::path::PathBuf;

        let ws = Workspace::new();
        let entries = vec![
            NamedSessionEntry {
                name: "api".to_string(),
                file: PathBuf::from("api.json"),
                saved_at: Some(2),
                status: EntryStatus::Ok,
            },
            NamedSessionEntry {
                name: "infra".to_string(),
                file: PathBuf::from("infra.json"),
                saved_at: Some(1),
                status: EntryStatus::Ok,
            },
        ];
        let picker = Some(SessionPicker::open_browsing(entries));
        let update = build_tree(
            &ws,
            &WarningStack::default(),
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &picker,
            &None,
            None,
            None,
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &crate::messages::test_support::pt_br(),
            "pt-BR",
            &mut measurer(),
        );

        let save_item = node(&update, SESSION_PICKER_SAVE_ITEM_ID);
        assert_eq!(save_item.role(), Role::MenuItem);
        let first_row = node(&update, session_picker_row_id(0));
        assert_eq!(first_row.label(), Some("api"));
        // Realce inicial de `open_browsing` é a primeira linha.
        assert_eq!(update.focus, session_picker_row_id(0));
    }

    /// Modo de edição vira campo de texto -- mesmo molde do editor de
    /// grupo, e o foco segue para lá (o item de salvar realçado é o
    /// estado de abertura de `session.save_named`, RF-14.1/RF-14.2).
    #[test]
    fn session_picker_editing_projects_a_text_field_in_focus() {
        let ws = Workspace::new();
        let picker = Some(SessionPicker::open_editing(vec![]));
        let update = build_tree(
            &ws,
            &WarningStack::default(),
            &None,
            &None,
            &None,
            &None,
            &None,
            &None,
            &picker,
            &None,
            None,
            None,
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &crate::messages::test_support::pt_br(),
            "pt-BR",
            &mut measurer(),
        );

        let field = node(&update, SESSION_PICKER_FIELD_ID);
        assert_eq!(field.role(), Role::TextInput);
        assert_eq!(update.focus, SESSION_PICKER_FIELD_ID);
        // Lista vazia: linha "nenhuma sessão salva" sem ser alvo de foco.
        let empty_row = node(&update, session_picker_row_id(0));
        assert_eq!(empty_row.label(), Some("nenhuma sessão salva"));
    }
}

#[cfg(test)]
mod settings_tree_tests {
    use super::*;
    use crate::messages::test_support;
    use crate::settings::{layout_for_test, rows_for_test};

    fn tree(selected: Group, has_pending: bool, with_header: bool, language: &str) -> TreeUpdate {
        tree_with_rows(selected, has_pending, with_header, language, &[])
    }

    fn tree_with_rows(
        selected: Group,
        has_pending: bool,
        with_header: bool,
        language: &str,
        rows: &[RowView],
    ) -> TreeUpdate {
        let layout = layout_for_test(with_header);
        let refs: Vec<&RowView> = rows.iter().collect();
        build_settings_tree(
            &layout,
            &Group::ALL,
            selected,
            has_pending,
            &refs,
            &test_support::pt_br(),
            language,
        )
    }

    fn node(update: &TreeUpdate, id: NodeId) -> &Node {
        &update
            .nodes
            .iter()
            .find(|(n, _)| *n == id)
            .expect("nó ausente")
            .1
    }

    #[test]
    fn the_root_is_a_window_with_the_language_of_the_catalog() {
        let update = tree(Group::General, false, true, "pt-BR");
        let root = node(&update, SETTINGS_ROOT_ID);
        assert_eq!(root.role(), Role::Window);
        assert_eq!(root.label(), Some("Configurações"));
        assert_eq!(root.language(), Some("pt-BR"));
        assert_eq!(update.tree.as_ref().unwrap().root, SETTINGS_ROOT_ID);
        assert_eq!(update.focus, SETTINGS_ROOT_ID);
    }

    #[test]
    fn the_sidebar_is_a_list_with_the_nine_groups_and_one_selected() {
        let update = tree(Group::Terminal, false, true, "pt-BR");
        let list = node(&update, SETTINGS_GROUP_LIST_ID);
        assert_eq!(list.role(), Role::ListBox);
        let items: Vec<&Node> = list
            .children()
            .iter()
            .map(|id| node(&update, *id))
            .collect();
        let labels: Vec<&str> = items.iter().map(|n| n.label().unwrap()).collect();
        assert_eq!(
            labels,
            [
                "Geral",
                "Shell",
                "Terminal",
                "Aparência",
                "Sessão",
                "Projeto",
                "Git",
                "Painéis",
                "Atalhos"
            ]
        );
        assert!(items.iter().all(|n| n.role() == Role::ListBoxOption));
        let selected: Vec<&str> = items
            .iter()
            .filter(|n| n.is_selected() == Some(true))
            .map(|n| n.label().unwrap())
            .collect();
        assert_eq!(selected, ["Terminal"]);
    }

    #[test]
    fn the_panel_names_the_selected_group_and_holds_the_footer() {
        let update = tree(Group::Git, false, true, "pt-BR");
        let panel = node(&update, SETTINGS_PANEL_ID);
        assert_eq!(panel.role(), Role::TabPanel);
        assert!(panel.label().unwrap().ends_with("Git"));
        assert_eq!(panel.children(), [SETTINGS_FOOTER_ID]);
        let footer = node(&update, SETTINGS_FOOTER_ID);
        let labels: Vec<&str> = footer
            .children()
            .iter()
            .map(|id| node(&update, *id).label().unwrap())
            .collect();
        assert_eq!(labels, ["Abrir arquivo no editor", "Descartar", "Salvar"]);
    }

    #[test]
    fn discard_and_save_are_disabled_without_pending_changes() {
        let disabled = |update: &TreeUpdate| -> Vec<bool> {
            node(update, SETTINGS_FOOTER_ID)
                .children()
                .iter()
                .map(|id| node(update, *id).is_disabled())
                .collect()
        };
        assert_eq!(
            disabled(&tree(Group::General, false, true, "pt-BR")),
            [false, true, true]
        );
        assert_eq!(
            disabled(&tree(Group::General, true, true, "pt-BR")),
            [false, false, false]
        );
    }

    #[test]
    fn window_buttons_exist_only_with_our_header() {
        let with = tree(Group::General, false, true, "pt-BR");
        let without = tree(Group::General, false, false, "pt-BR");
        let root_children =
            |update: &TreeUpdate| node(update, SETTINGS_ROOT_ID).children().to_vec();
        assert!(root_children(&with).contains(&SETTINGS_WINDOW_CLOSE_ID));
        assert!(!root_children(&without).contains(&SETTINGS_WINDOW_CLOSE_ID));
        assert_eq!(
            root_children(&with).len(),
            root_children(&without).len() + 3
        );
    }

    #[test]
    fn every_node_id_is_unique_and_inside_the_settings_block() {
        let update = tree(Group::General, true, true, "pt-BR");
        let mut ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert!(ids.iter().all(|id| *id >= SETTINGS_ID_BASE));
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total);
        // Acima de todo bloco da janela de terminal.
        assert!(SETTINGS_ID_BASE > PANE_ID_BASE + 1_000 * 1_000);
    }

    #[test]
    fn every_child_is_a_node_of_the_tree() {
        let update = tree(Group::General, true, true, "pt-BR");
        let ids: Vec<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
        for (_, node) in &update.nodes {
            for child in node.children() {
                assert!(ids.contains(child), "filho {child:?} fora da árvore");
            }
        }
    }

    #[test]
    fn a_language_switch_changes_the_labels_and_the_language() {
        let layout = layout_for_test(true);
        let en = build_settings_tree(
            &layout,
            &Group::ALL,
            Group::General,
            false,
            &[],
            &test_support::en_us(),
            "en-US",
        );
        let root = node(&en, SETTINGS_ROOT_ID);
        assert_eq!(root.label(), Some("Settings"));
        assert_eq!(root.language(), Some("en-US"));
        assert_eq!(
            node(&en, settings_group_item_id(0)).label(),
            Some("General")
        );
    }

    // ---- linhas de opção

    fn row_nodes(update: &TreeUpdate) -> Vec<&Node> {
        node(update, SETTINGS_PANEL_ID)
            .children()
            .iter()
            .filter(|id| **id != SETTINGS_FOOTER_ID)
            .map(|id| node(update, *id))
            .collect()
    }

    fn labelled<'a>(nodes: &[&'a Node], label: &str) -> &'a Node {
        nodes
            .iter()
            .find(|node| node.label() == Some(label))
            .unwrap_or_else(|| panic!("{label}"))
    }

    #[test]
    fn each_option_row_becomes_a_node_with_the_role_of_its_control() {
        let rows = rows_for_test(Group::Terminal);
        let update = tree_with_rows(Group::Terminal, false, true, "pt-BR", &rows);
        let nodes = row_nodes(&update);
        assert_eq!(nodes.len(), rows.len());
        // alternância, campo numérico, campo de texto, escolha.
        assert_eq!(labelled(&nodes, "Piscar").role(), Role::Switch);
        assert_eq!(labelled(&nodes, "Tamanho").role(), Role::SpinButton);
        assert_eq!(labelled(&nodes, "Família").role(), Role::TextInput);
        assert_eq!(labelled(&nodes, "Forma").role(), Role::ComboBox);
    }

    #[test]
    fn a_row_with_a_refused_value_is_announced_as_invalid() {
        let mut rows = rows_for_test(Group::Terminal);
        let size = rows
            .iter_mut()
            .find(|row| row.option == Some("font_size"))
            .unwrap();
        size.invalid = Some("Número inválido.".to_owned());
        let update = tree_with_rows(Group::Terminal, true, true, "pt-BR", &rows);
        let nodes = row_nodes(&update);
        assert_eq!(
            labelled(&nodes, "Tamanho").invalid(),
            Some(accesskit::Invalid::True)
        );
        assert_eq!(labelled(&nodes, "Piscar").invalid(), None);
    }

    #[test]
    fn rows_expose_name_description_and_value() {
        let rows = rows_for_test(Group::Terminal);
        let update = tree_with_rows(Group::Terminal, false, true, "pt-BR", &rows);
        let nodes = row_nodes(&update);
        let size = labelled(&nodes, "Tamanho");
        assert_eq!(size.value(), Some("14"));
        assert_eq!(size.numeric_value(), Some(14.0));
        assert_eq!(
            size.description(),
            Some("Tamanho da fonte, em pixels lógicos.")
        );
        assert_eq!(
            labelled(&nodes, "Piscar").toggled(),
            Some(accesskit::Toggled::False)
        );
        assert_eq!(labelled(&nodes, "Forma").value(), Some("Bloco"));
    }

    #[test]
    fn a_class_c_option_announces_its_scope_in_the_description() {
        let rows = rows_for_test(Group::Shell);
        let update = tree_with_rows(Group::Shell, false, true, "pt-BR", &rows);
        let nodes = row_nodes(&update);
        let program = labelled(&nodes, "Programa");
        assert!(
            program
                .description()
                .unwrap()
                .ends_with("(vale em aba nova)")
        );
    }

    #[test]
    fn a_list_row_has_its_items_as_children() {
        let mut rows = rows_for_test(Group::Shell);
        let args = rows
            .iter_mut()
            .find(|row| row.option == Some("shell_args"))
            .unwrap();
        args.control = ControlView::List {
            items: vec!["-l".to_owned(), "-i".to_owned()],
            add_label: "Adicionar".to_owned(),
        };
        let update = tree_with_rows(Group::Shell, false, true, "pt-BR", &rows);
        let nodes = row_nodes(&update);
        let list = labelled(&nodes, "Argumentos");
        assert_eq!(list.role(), Role::List);
        let items: Vec<&str> = list
            .children()
            .iter()
            .map(|id| node(&update, *id).label().unwrap())
            .collect();
        assert_eq!(items, ["-l", "-i"]);
    }

    #[test]
    fn theme_rows_are_selectable_options_and_the_git_row_has_two_halves() {
        let rows = rows_for_test(Group::Appearance);
        let update = tree_with_rows(Group::Appearance, false, true, "pt-BR", &rows);
        let nodes = row_nodes(&update);
        let themes: Vec<&&Node> = nodes
            .iter()
            .filter(|node| node.role() == Role::ListBoxOption)
            .collect();
        assert!(themes.len() >= 2);
        assert_eq!(
            themes
                .iter()
                .filter(|node| node.is_selected() == Some(true))
                .count(),
            1
        );

        let rows = rows_for_test(Group::Git);
        let update = tree_with_rows(Group::Git, false, true, "pt-BR", &rows);
        let nodes = row_nodes(&update);
        let git = nodes[0];
        assert_eq!(git.role(), Role::Group);
        let roles: Vec<Role> = git
            .children()
            .iter()
            .map(|id| node(&update, *id).role())
            .collect();
        assert_eq!(roles, [Role::Switch, Role::SpinButton]);
    }

    #[test]
    fn row_ids_stay_unique_across_every_group() {
        for group in Group::ALL {
            let rows = rows_for_test(group);
            let update = tree_with_rows(group, true, true, "pt-BR", &rows);
            let mut ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
            let total = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), total, "{group:?}");
            let known: Vec<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
            for (_, node) in &update.nodes {
                for child in node.children() {
                    assert!(known.contains(child), "{group:?}: filho {child:?} fora");
                }
            }
        }
    }
}
