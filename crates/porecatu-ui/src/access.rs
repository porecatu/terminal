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
use porecatu_core::{GroupColor, GroupId, TabId, Workspace};
use porecatu_render::TextMeasurer;

use crate::context_menu::{ContextMenu, TAB_MENU_ITEMS};
use crate::dialog::{ConfirmDialog, DialogButton};
use crate::group_editor::{EditorRegion, GroupEditor};
use crate::group_menu::{EDITOR_ACTION_ORDER, GroupContextMenu};
use crate::is_macos;
use crate::move_to_group::MoveToGroupPopover;
use crate::search_bar::SearchBarState;
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

const WARNING_ITEM_BASE: u64 = 300_000;
const MENU_ITEM_BASE: u64 = 400_000;
const SWATCH_BASE: u64 = 500_000;
const EDITOR_ACTION_BASE: u64 = 500_100;
const MOVE_TARGET_BASE: u64 = 500_200;

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

/// Nome em português da cor -- só rótulo acessível, não valor de aparência
/// (a regra do CLAUDE.md sobre "nenhuma cor inventada" é sobre tokens
/// visuais, não sobre o nome falado de uma cor já escolhida).
fn color_name(color: GroupColor) -> &'static str {
    match color {
        GroupColor::Red => "Vermelho",
        GroupColor::Yellow => "Amarelo",
        GroupColor::Cyan => "Ciano",
        GroupColor::Blue => "Azul",
        GroupColor::Purple => "Roxo",
        GroupColor::Green => "Verde",
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
    search: &Option<SearchBarState>,
    style: &TabBarStyle,
    logical_width: f32,
    scroll_offset: f32,
    measurer: &mut TextMeasurer,
) -> TreeUpdate {
    let is_mac = is_macos();
    let mut nodes: Vec<(NodeId, Node)> = Vec::new();
    let mut root_children: Vec<NodeId> = Vec::new();

    let trilha_width = tab_bar::trilha_width(style, logical_width, is_mac);
    let layout = tab_bar::layout(workspace, style, measurer);
    let overflow = tab_bar::overflow_state(&layout, trilha_width, scroll_offset);

    build_tab_list(workspace, &layout, &mut nodes, &mut root_children);

    if overflow.hidden_left > 0 {
        nodes.push((
            OVERFLOW_LEFT_ID,
            leaf(
                Role::Button,
                format!("{} abas ocultas à esquerda", overflow.hidden_left),
            ),
        ));
        root_children.push(OVERFLOW_LEFT_ID);
    }
    if overflow.hidden_right > 0 {
        nodes.push((
            OVERFLOW_RIGHT_ID,
            leaf(
                Role::Button,
                format!("{} abas ocultas à direita", overflow.hidden_right),
            ),
        ));
        root_children.push(OVERFLOW_RIGHT_ID);
    }

    if layout.ungrouped_new_tab_button.is_some() {
        nodes.push((
            UNGROUPED_NEW_TAB_ID,
            leaf(Role::Button, "Nova aba fora de grupo"),
        ));
        root_children.push(UNGROUPED_NEW_TAB_ID);
    }

    nodes.push((SETTINGS_BUTTON_ID, leaf(Role::Button, "Configurações")));
    root_children.push(SETTINGS_BUTTON_ID);

    if !is_mac {
        nodes.push((WINDOW_MINIMIZE_ID, leaf(Role::Button, "Minimizar")));
        nodes.push((
            WINDOW_MAXIMIZE_ID,
            leaf(Role::Button, "Maximizar ou restaurar"),
        ));
        nodes.push((WINDOW_CLOSE_ID, leaf(Role::Button, "Fechar janela")));
        root_children.push(WINDOW_MINIMIZE_ID);
        root_children.push(WINDOW_MAXIMIZE_ID);
        root_children.push(WINDOW_CLOSE_ID);
    }

    let mut focus = ROOT_ID;

    if let Some(state) = search {
        build_search_bar(state, &mut nodes, &mut root_children);
    }

    if !warnings.is_empty() {
        build_warnings(warnings, &mut nodes, &mut root_children);
    }

    // No máximo um destes está `Some` de cada vez, por construção da
    // cadeia de captura do ADR-0008 -- mas a árvore só reflete o que
    // `state` de fato carrega, nunca presume exclusividade.
    if let Some(d) = dialog {
        focus = build_dialog(d, &mut nodes, &mut root_children);
    } else if let Some(m) = context_menu {
        focus = build_tab_menu(m, &mut nodes, &mut root_children);
    } else if let Some(m) = group_context_menu {
        focus = build_group_menu(m, workspace, &mut nodes, &mut root_children);
    } else if let Some(m) = terminal_context_menu {
        focus = build_terminal_menu(m, &mut nodes, &mut root_children);
    } else if let Some(e) = group_editor {
        focus = build_group_editor(e, workspace, &mut nodes, &mut root_children);
    } else if let Some(p) = move_to_group {
        focus = build_move_to_group(p, workspace, &mut nodes, &mut root_children);
    }

    let mut root = Node::new(Role::Window);
    root.set_label("Porecatu");
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
            let mut label = format!("Grupo {}", group.name().unwrap_or(&pill.name));
            if group.is_collapsed() {
                label.push_str(", colapsado");
            }
            if let Some(color) = group.color() {
                label.push_str(", cor ");
                label.push_str(color_name(color));
            }
            nodes.push((group_pill_id(group_wrapper.id), leaf(Role::Button, label)));
            tab_list_children.push(group_pill_id(group_wrapper.id));
        }

        for tab_rect in &group_wrapper.tabs {
            let Some(tab) = workspace.tab(tab_rect.id) else {
                continue;
            };
            let mut node = Node::new(Role::Tab);
            let mut label = tab.title().to_owned();
            if Some(tab_rect.id) == active_tab {
                node.set_selected(true);
                label.push_str(" (ativa)");
            }
            if tab.is_not_started() {
                label.push_str(" (não iniciada)");
            }
            match tab_rect.indicator {
                Some(Indicator::Bell) => label.push_str(" (campainha)"),
                Some(Indicator::Activity) => label.push_str(" (atividade)"),
                None => {}
            }
            node.set_label(label);
            node.add_action(accesskit::Action::Focus);
            let close_id = tab_close_button_id(tab_rect.id);
            node.set_children(vec![close_id]);
            nodes.push((tab_node_id(tab_rect.id), node));
            nodes.push((close_id, leaf(Role::Button, "Fechar aba")));
            tab_list_children.push(tab_node_id(tab_rect.id));
        }

        if group_wrapper.new_tab_button.is_some() {
            let id = group_new_tab_id(group_wrapper.id);
            nodes.push((id, leaf(Role::Button, "Nova aba neste grupo")));
            tab_list_children.push(id);
        }
    }

    nodes.push((TAB_LIST_ID, container(Role::TabList, tab_list_children)));
    root_children.push(TAB_LIST_ID);
}

fn build_search_bar(
    state: &SearchBarState,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) {
    let (counter, _is_error) = state.counter_display();
    let mut field = Node::new(Role::SearchInput);
    field.set_value(state.field().text());
    if !counter.is_empty() {
        field.set_description(counter);
    }
    nodes.push((SEARCH_FIELD_ID, field));

    let mut toggle = Node::new(Role::Switch);
    toggle.set_label("Expressão regular");
    toggle.set_toggled(state.is_regex().into());
    nodes.push((SEARCH_REGEX_TOGGLE_ID, toggle));

    nodes.push((
        SEARCH_BAR_ID,
        container(Role::Search, vec![SEARCH_FIELD_ID, SEARCH_REGEX_TOGGLE_ID]),
    ));
    root_children.push(SEARCH_BAR_ID);
}

fn build_warnings(
    warnings: &WarningStack,
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) {
    let mut children = Vec::new();
    for (index, item) in warnings.items().iter().enumerate() {
        let severity = match item.severity {
            Severity::Error => "Erro",
            Severity::Warning => "Aviso",
            Severity::Info => "Informação",
        };
        let id = warning_item_id(index);
        nodes.push((
            id,
            leaf(
                Role::Alert,
                format!("{severity}: {}: {}", item.title, item.body),
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
    nodes.push((DIALOG_CANCEL_ID, leaf(Role::Button, "Cancelar")));
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
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) -> NodeId {
    let mut children = Vec::new();
    let mut focus = MENU_ID;
    for (index, item) in TAB_MENU_ITEMS.iter().enumerate() {
        let id = menu_item_id(index);
        let mut node = Node::new(Role::MenuItem);
        node.set_label(item.label);
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
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) -> NodeId {
    let is_collapsed = workspace
        .group(menu.group)
        .is_some_and(|g| g.is_collapsed());
    let tab_count = workspace.group(menu.group).map_or(0, |g| g.tabs().len());
    let items = crate::group_menu::group_action_items(is_collapsed, tab_count);
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
        node.set_label(item.label);
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
        node.set_label(color_name(*color));
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
    node.set_label("Editor de grupo");
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
    nodes: &mut Vec<(NodeId, Node)>,
    root_children: &mut Vec<NodeId>,
) -> NodeId {
    let mut children = Vec::new();
    let mut focus = MENU_ID;
    for (index, group_id) in popover.targets().iter().enumerate() {
        let name = workspace
            .group(*group_id)
            .and_then(porecatu_core::Group::name)
            .unwrap_or("Grupo");
        let id = move_target_id(index);
        nodes.push((id, leaf(Role::MenuItem, name)));
        children.push(id);
        if index == popover.highlighted() {
            focus = id;
        }
    }
    let new_group_index = popover.targets().len();
    let id = move_target_id(new_group_index);
    nodes.push((id, leaf(Role::MenuItem, "Novo grupo")));
    children.push(id);
    if popover.highlighted() == new_group_index {
        focus = id;
    }
    nodes.push((MENU_ID, container(Role::Menu, children)));
    root_children.push(MENU_ID);
    focus
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
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
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
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
            &mut measurer(),
        );
        let item = node(&update, warning_item_id(0));
        assert_eq!(item.role(), Role::Alert);
        let label = item.label().unwrap();
        assert!(label.starts_with("Erro:"));
        assert!(label.contains("Config inválida"));
    }

    #[test]
    fn dialog_is_modal_and_focus_follows_the_focused_button() {
        let ws = Workspace::new();
        let dialog = Some(ConfirmDialog::new(
            "Fechar janela?",
            "Duas abas abertas.",
            "Fechar",
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
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
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
            &TabBarStyle::DEFAULT,
            800.0,
            0.0,
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
}
