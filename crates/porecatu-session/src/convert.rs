// SPDX-License-Identifier: GPL-3.0-or-later

//! Conversão explícita entre `porecatu_core::Workspace` e o schema v1
//! (ADR-0036 §3), nos dois sentidos. `Workspace -> WindowV1` na gravação,
//! `WindowV1 -> Workspace` na leitura -- é a explicitação que torna o
//! teste de cobertura de campo escrevível.
//!
//! **Descartado na gravação:** `Group::last_active`, `Tab::activity`,
//! `Tab::bell`, `Tab::process_title`, `Tab::state`. Aba `Exited` é
//! filtrada aqui, não na leitura -- o arquivo não guarda o que não deve
//! voltar (ADR-0017 §6). Grupo que fica vazio depois desse filtro não é
//! gravado.
//!
//! IDs do arquivo não sobrevivem à leitura: `Workspace` gera identidade
//! nova para cada aba e grupo. O `GroupId` de grupo implícito nunca foi
//! identidade estável entre sessões (ADR-0006), e o de grupo explícito
//! também não precisa ser -- nada fora deste arquivo referencia o `id`
//! gravado.

use std::collections::HashMap;

use porecatu_core::{
    ExternalNode, GroupColor, Pane, PaneNode, PaneTree, SplitAxis, TabId, Workspace,
};

use crate::schema::v1::{GroupV1, PaneNodeV1, PaneTreeV1, PaneV1, SplitAxisV1, TabV1};

/// Extrai grupos, abas e aba ativa de um `Workspace`, na forma que
/// `WindowV1::groups`/`WindowV1::tabs`/`WindowV1::active_tab` esperam.
pub fn window_from_workspace(ws: &Workspace) -> (Vec<GroupV1>, Vec<TabV1>, Option<u32>) {
    let mut groups = Vec::new();
    for group in ws.groups() {
        let alive: Vec<u32> = group
            .tabs()
            .iter()
            .copied()
            .filter(|&id| ws.tab(id).is_some_and(|t| !t.is_exited()))
            .map(TabId::get)
            .collect();
        if alive.is_empty() {
            continue;
        }
        groups.push(GroupV1 {
            id: group.id().get(),
            name: group.name().map(str::to_string),
            color: group.color().map(color_to_str).map(str::to_string),
            collapsed: group.is_collapsed(),
            tabs: alive,
        });
    }

    let mut tabs = Vec::new();
    for id in ws.visual_order() {
        let Some(tab) = ws.tab(id) else { continue };
        if tab.is_exited() {
            continue;
        }
        tabs.push(TabV1 {
            id: id.get(),
            custom_title: tab.has_custom_title().then(|| tab.title().to_string()),
            cwd: tab.cwd().cloned(),
            spawn_program: Some(tab.shell_name().to_string()),
            // ADR-0070 §4: with `panes` present, the profile lives in each leaf.
            profile: None,
            panes: Some(pane_tree_v1(tab.panes())),
        });
    }

    let active_tab = ws
        .active_tab()
        .filter(|&id| ws.tab(id).is_some_and(|t| !t.is_exited()))
        .map(TabId::get);

    (groups, tabs, active_tab)
}

/// Reconstrói um `Workspace` a partir de `groups`/`tabs`/`active_tab` de
/// um `WindowV1`. Referência órfã (`id` num grupo sem `TabV1`
/// correspondente, ou `active_tab` sem aba criada) é ignorada em vez de
/// falhar -- defensivo contra arquivo editado à mão, que não é o caminho
/// normal, mas não deve travar a restauração.
///
/// `lazy_restore` (RF-3.8, ADR-0037 §1/§6, F5 etapa 4): com `true`, toda
/// aba que não é a `active_tab` da janela nasce `NotStarted` -- só a ativa
/// nasce `Running`. Com `false`, todas nascem `Running`, e a chamada é
/// idêntica ao comportamento de antes desta etapa. A decisão é por aba
/// individual, não por lote -- salvo painel SSH, que ignora tudo isso (abaixo):
/// nenhuma verificação de disco (`cwd` inexistente,
/// RF-3.10) acontece aqui -- essa checagem é de quem sobe o shell de
/// verdade (`WindowState::spawn_tab_runtime`), no momento em que a aba sobe,
/// não na reconstrução do modelo.
///
/// `is_ssh_profile` (RF-20.33, ADR-0075 §5): todo painel cujo perfil gravado
/// ela reconhece como SSH nasce `NotStarted`, **mesmo com `lazy_restore =
/// false` e mesmo na aba ativa** -- uma conexão só abre por gesto do usuário.
/// A decisão é por painel, não por aba; os locais seguem a regra acima sem
/// mudança. O crate não conhece perfis (só o identificador gravado), então
/// quem restaura diz quais identificadores são SSH.
pub fn workspace_from_window(
    groups: &[GroupV1],
    tabs: &[TabV1],
    active_tab: Option<u32>,
    lazy_restore: bool,
    is_ssh_profile: &dyn Fn(&str) -> bool,
) -> Workspace {
    let by_id: HashMap<u32, &TabV1> = tabs.iter().map(|t| (t.id, t)).collect();
    let mut ws = Workspace::new();
    let mut created: HashMap<u32, TabId> = HashMap::new();

    for group in groups {
        let mut group_id = None;
        for (pos, &file_tab_id) in group.tabs.iter().enumerate() {
            let Some(tab) = by_id.get(&file_tab_id) else {
                continue;
            };
            let is_active = active_tab == Some(file_tab_id);
            let not_started = lazy_restore && !is_active;
            let new_id = if let Some(tree) = &tab.panes {
                let core_tree = pane_tree_from_v1(tree, not_started, is_ssh_profile);
                ws.insert_tab_with_panes(group_id, core_tree, pos)
            } else {
                // Arquivo gravado por uma versão anterior ao ADR-0053 §11:
                // sem árvore, a aba volta como um painel só (ADR-0036 §3,
                // "perda de layout, não corrupção").
                let shell = tab.spawn_program.clone().unwrap_or_default();
                let ssh = is_ssh_saved(tab.profile.as_deref(), is_ssh_profile);
                let not_started = not_started || ssh;
                let cwd = tab.cwd.clone().filter(|_| !ssh);
                let id = if not_started {
                    ws.new_tab_not_started(group_id, shell, cwd, pos)
                } else {
                    ws.new_tab(group_id, shell, cwd, pos)
                };
                ws.tab_mut(id)
                    .expect("acabou de ser criada")
                    .panes_mut()
                    .focused_mut()
                    .set_profile(profile_from_v1(tab.profile.as_deref()));
                id
            };
            if group_id.is_none() {
                group_id = ws.group_of_tab(new_id);
            }
            if let Some(title) = &tab.custom_title {
                ws.tab_mut(new_id)
                    .expect("acabou de ser criada")
                    .set_custom_title(Some(title.clone()));
            }
            created.insert(file_tab_id, new_id);
        }

        if group_id.is_none() {
            continue;
        }
        if let Some(color) = group.color.as_deref().and_then(color_from_str) {
            let name = group.name.clone().unwrap_or_default();
            let ids: Vec<TabId> = group
                .tabs
                .iter()
                .filter_map(|id| created.get(id).copied())
                .collect();
            if let Some(new_group_id) = ws.group_tabs(&ids, name, color)
                && group.collapsed
            {
                ws.collapse_group(new_group_id, true);
            }
        }
    }

    if let Some(active) = active_tab.and_then(|id| created.get(&id).copied()) {
        ws.activate_tab(active);
    }

    ws
}

fn color_to_str(color: GroupColor) -> &'static str {
    match color {
        GroupColor::Red => "red",
        GroupColor::Yellow => "yellow",
        GroupColor::Cyan => "cyan",
        GroupColor::Blue => "blue",
        GroupColor::Purple => "purple",
        GroupColor::Green => "green",
    }
}

fn color_from_str(s: &str) -> Option<GroupColor> {
    Some(match s {
        "red" => GroupColor::Red,
        "yellow" => GroupColor::Yellow,
        "cyan" => GroupColor::Cyan,
        "blue" => GroupColor::Blue,
        "purple" => GroupColor::Purple,
        "green" => GroupColor::Green,
        _ => return None,
    })
}

/// ADR-0053 §11: `PaneTree` (domínio) -> `PaneTreeV1` (disco), estrutura,
/// `ratio` e `cwd`/`spawn_program` de cada folha, mais o `id` do painel
/// focado.
fn pane_tree_v1(tree: &PaneTree) -> PaneTreeV1 {
    PaneTreeV1 {
        root: pane_node_v1(tree.root(), tree),
        focused: tree.focused_id().get(),
    }
}

fn pane_node_v1(node: &PaneNode, tree: &PaneTree) -> PaneNodeV1 {
    match node {
        PaneNode::Leaf(id) => {
            let pane = tree
                .pane(*id)
                .expect("todo Leaf de PaneTree referencia um painel existente");
            PaneNodeV1::Leaf(PaneV1 {
                id: id.get(),
                cwd: pane.cwd().cloned(),
                spawn_program: Some(pane.shell_name().to_string()),
                profile: pane.profile().map(str::to_string),
            })
        }
        PaneNode::Split {
            axis,
            ratio,
            first,
            second,
        } => PaneNodeV1::Split {
            axis: axis_to_v1(*axis),
            ratio: *ratio,
            first: Box::new(pane_node_v1(first, tree)),
            second: Box::new(pane_node_v1(second, tree)),
        },
    }
}

/// A stored `""` is never written (ADR-0070 §4); read it as the `[shell]`
/// anyway rather than as a profile with an empty id.
fn profile_from_v1(profile: Option<&str>) -> Option<String> {
    profile.filter(|p| !p.is_empty()).map(str::to_string)
}

fn axis_to_v1(axis: SplitAxis) -> SplitAxisV1 {
    match axis {
        SplitAxis::Horizontal => SplitAxisV1::Horizontal,
        SplitAxis::Vertical => SplitAxisV1::Vertical,
    }
}

fn axis_from_v1(axis: SplitAxisV1) -> SplitAxis {
    match axis {
        SplitAxisV1::Horizontal => SplitAxis::Horizontal,
        SplitAxisV1::Vertical => SplitAxis::Vertical,
    }
}

/// `PaneNodeV1` (disco) -> `ExternalNode<PaneV1>` (o que
/// `PaneTree::from_external` consome) -- puramente estrutural, sem decidir
/// estado nenhum ainda.
fn external_from_v1(node: &PaneNodeV1) -> ExternalNode<PaneV1> {
    match node {
        PaneNodeV1::Leaf(pane) => ExternalNode::Leaf(pane.clone()),
        PaneNodeV1::Split {
            axis,
            ratio,
            first,
            second,
        } => ExternalNode::Split {
            axis: axis_from_v1(*axis),
            ratio: *ratio,
            first: Box::new(external_from_v1(first)),
            second: Box::new(external_from_v1(second)),
        },
    }
}

fn is_ssh_saved(profile: Option<&str>, is_ssh_profile: &dyn Fn(&str) -> bool) -> bool {
    profile_from_v1(profile).is_some_and(|id| is_ssh_profile(&id))
}

/// ADR-0053 §11: `PaneTreeV1` (disco) -> `PaneTree` (domínio) --
/// `not_started` vem de fora porque o gatilho de restauração preguiçosa é
/// por **aba** (RF-6.23): todo painel da árvore nasce no mesmo estado, nunca
/// decidido folha a folha.
fn pane_tree_from_v1(
    tree: &PaneTreeV1,
    not_started: bool,
    is_ssh_profile: &dyn Fn(&str) -> bool,
) -> PaneTree {
    let shape = external_from_v1(&tree.root);
    PaneTree::from_external(
        shape,
        |id, leaf: &PaneV1| {
            let shell = leaf.spawn_program.clone().unwrap_or_default();
            // RF-20.33: the exception to the tab-wide rule above -- an SSH pane
            // waits for a gesture whatever the tab does.
            let ssh = is_ssh_saved(leaf.profile.as_deref(), is_ssh_profile);
            let not_started = not_started || ssh;
            let mut pane = if not_started {
                Pane::new_not_started(id, shell)
            } else {
                Pane::new(id, shell)
            };
            // A directory saved for an SSH pane (by a version that did not know
            // better) is not a place on this machine: RF-20.22.
            if let Some(cwd) = leaf.cwd.clone().filter(|_| !ssh) {
                pane.set_cwd(cwd);
            }
            pane.set_profile(profile_from_v1(leaf.profile.as_deref()));
            pane
        },
        |leaf: &PaneV1| leaf.id == tree.focused,
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use porecatu_core::PaneState;

    use super::*;

    /// Round-trip pelo DTO com grupo explícito nomeado/colorido/colapsado,
    /// aba com título customizado, `cwd` e aba ativa -- compara o
    /// `Workspace` reconstruído com o original **e** a ordem visual.
    #[test]
    fn round_trip_preserves_workspace_and_visual_order() {
        let mut ws = Workspace::new();
        let solta = ws.append_tab("zsh", Some(PathBuf::from("/home/user")));
        let a = ws.append_tab("bash", Some(PathBuf::from("/srv/api")));
        let b = ws.append_tab("bash", None);
        let grupo = ws.group_tabs(&[a, b], "api", GroupColor::Blue).unwrap();
        ws.tab_mut(a)
            .unwrap()
            .set_custom_title(Some("backend".to_string()));
        ws.collapse_group(grupo, true);
        ws.activate_tab(solta);

        let (groups, tabs, active_tab) = window_from_workspace(&ws);
        let json = serde_json::to_string(&(&groups, &tabs, active_tab)).unwrap();
        let (groups2, tabs2, active_tab2): (Vec<GroupV1>, Vec<TabV1>, Option<u32>) =
            serde_json::from_str(&json).unwrap();

        let rebuilt = workspace_from_window(&groups2, &tabs2, active_tab2, false, &|_| false);

        // Ordem visual: uma aba solta seguida do grupo colapsado (que
        // continua gravado -- colapso não tira aba do arquivo).
        let solta2 = rebuilt.visual_order().next().unwrap();
        assert_eq!(rebuilt.visual_order().count(), 3);
        assert!(rebuilt.tab(solta2).unwrap().cwd() == Some(&PathBuf::from("/home/user")));
        assert_eq!(rebuilt.active_tab(), Some(solta2));

        let rebuilt_group = rebuilt
            .group_of_tab(rebuilt.visual_order().nth(1).unwrap())
            .unwrap();
        let g = rebuilt.group(rebuilt_group).unwrap();
        assert_eq!(g.name(), Some("api"));
        assert_eq!(g.color(), Some(GroupColor::Blue));
        assert!(g.is_collapsed());
        assert_eq!(g.tabs().len(), 2);

        let backend_tab = rebuilt.tab(*g.tabs().first().unwrap()).unwrap();
        assert_eq!(backend_tab.title(), "backend");
        assert_eq!(backend_tab.cwd(), Some(&PathBuf::from("/srv/api")));
    }

    /// Sem `panes` -- o caminho de um arquivo gravado antes do ADR-0053
    /// §11, usado por vários testes de restauração preguiçosa que não
    /// precisam de árvore nenhuma.
    fn tab_v1(id: u32, shell: &str) -> TabV1 {
        TabV1 {
            id,
            custom_title: None,
            cwd: None,
            spawn_program: Some(shell.to_string()),
            profile: None,
            panes: None,
        }
    }

    /// ADR-0053 §11: round-trip de sessão com três painéis -- estrutura
    /// (split externo em pé com um split interno deitado do lado direito),
    /// `ratio` de cada divisor, `cwd` de cada folha e qual delas estava
    /// focada sobrevivem ao DTO.
    #[test]
    fn round_trip_preserves_pane_tree_structure_ratios_and_focus() {
        let mut ws = Workspace::new();
        let tab_id = ws.append_tab("zsh", Some(PathBuf::from("/srv/api")));
        let top = ws.tab(tab_id).unwrap().panes().focused_id();
        let right = ws
            .tab_mut(tab_id)
            .unwrap()
            .panes_mut()
            .split(
                top,
                SplitAxis::Vertical,
                "bash",
                Some(PathBuf::from("/srv/api/logs")),
            )
            .unwrap();
        ws.tab_mut(tab_id)
            .unwrap()
            .panes_mut()
            .set_ratio(right, 0.3);
        let bottom_right = ws
            .tab_mut(tab_id)
            .unwrap()
            .panes_mut()
            .split(
                right,
                SplitAxis::Horizontal,
                "bash",
                Some(PathBuf::from("/srv/api/logs")),
            )
            .unwrap();
        ws.tab_mut(tab_id).unwrap().panes_mut().focus(top);

        let (groups, tabs, active_tab) = window_from_workspace(&ws);
        let json = serde_json::to_string(&(&groups, &tabs, active_tab)).unwrap();
        let (groups2, tabs2, active_tab2): (Vec<GroupV1>, Vec<TabV1>, Option<u32>) =
            serde_json::from_str(&json).unwrap();
        let rebuilt = workspace_from_window(&groups2, &tabs2, active_tab2, false, &|_| false);

        let rebuilt_id = rebuilt.visual_order().next().unwrap();
        let tree = rebuilt.tab(rebuilt_id).unwrap().panes();
        assert_eq!(tree.leaves_in_order().len(), 3);

        let PaneNode::Split {
            axis: outer_axis,
            ratio: outer_ratio,
            first,
            second,
        } = tree.root()
        else {
            panic!("split externo esperado");
        };
        assert_eq!(*outer_axis, SplitAxis::Vertical);
        assert!((*outer_ratio - 0.3).abs() < 0.001);

        let PaneNode::Leaf(rebuilt_top) = **first else {
            panic!("primeiro filho deveria continuar folha (o painel de origem)");
        };
        // O painel focado (o de origem, `top`) sobrevive à volta.
        assert_eq!(tree.focused_id(), rebuilt_top);
        assert_eq!(
            tree.pane(rebuilt_top).unwrap().cwd(),
            Some(&PathBuf::from("/srv/api"))
        );

        let PaneNode::Split {
            axis: inner_axis,
            first: inner_first,
            second: inner_second,
            ..
        } = second.as_ref()
        else {
            panic!("segundo filho deveria ser o split interno");
        };
        assert_eq!(*inner_axis, SplitAxis::Horizontal);
        let PaneNode::Leaf(rebuilt_right) = **inner_first else {
            panic!("split interno deveria ter duas folhas");
        };
        let PaneNode::Leaf(rebuilt_bottom_right) = **inner_second else {
            panic!("split interno deveria ter duas folhas");
        };
        for id in [rebuilt_right, rebuilt_bottom_right] {
            assert_eq!(
                tree.pane(id).unwrap().cwd(),
                Some(&PathBuf::from("/srv/api/logs"))
            );
        }
        let _ = bottom_right; // usado só para montar o cenário original
    }

    /// ADR-0036 §3/ADR-0053 §11: um arquivo gravado por uma versão anterior
    /// a esta -- sem a chave `panes` sequer presente no JSON, não só
    /// `null` -- restaura a aba como um painel só, no `cwd` que o arquivo
    /// antigo gravava.
    #[test]
    fn legacy_file_without_a_panes_key_restores_as_a_single_pane() {
        let groups_json = r#"[{"id":0,"name":null,"color":null,"collapsed":false,"tabs":[0]}]"#;
        let tabs_json =
            r#"[{"id":0,"custom_title":null,"cwd":"/home/user","spawn_program":"zsh"}]"#;
        let groups: Vec<GroupV1> = serde_json::from_str(groups_json).unwrap();
        let tabs: Vec<TabV1> = serde_json::from_str(tabs_json).unwrap();
        assert!(tabs[0].panes.is_none(), "chave ausente vira None");

        let ws = workspace_from_window(&groups, &tabs, Some(0), false, &|_| false);
        let id = ws.visual_order().next().unwrap();
        let tab = ws.tab(id).unwrap();
        assert_eq!(tab.panes().leaves_in_order().len(), 1);
        assert_eq!(tab.cwd(), Some(&PathBuf::from("/home/user")));
    }

    /// ADR-0053 §10/§11: aba com o painel **focado** em `Exited` continua
    /// sendo descartada na gravação, como hoje -- mesmo com painéis
    /// saudáveis ao lado. A regra não mudou de forma com os painéis; ela
    /// já olhava o painel focado antes deles existirem (`Tab::is_exited`).
    #[test]
    fn tab_with_the_focused_pane_exited_is_still_discarded_with_split_panes() {
        let mut ws = Workspace::new();
        let tab_id = ws.append_tab("zsh", None);
        let top = ws.tab(tab_id).unwrap().panes().focused_id();
        ws.tab_mut(tab_id)
            .unwrap()
            .panes_mut()
            .split(top, SplitAxis::Vertical, "bash", None)
            .unwrap();
        // O painel focado (o novo, à direita) sai com código != 0: fica
        // `Exited`, mas não é removido da árvore (RF-6.11).
        ws.tab_mut(tab_id).unwrap().mark_exited(1);

        let (groups, tabs, _) = window_from_workspace(&ws);
        assert!(!tabs.iter().any(|t| t.id == tab_id.get()));
        assert!(
            groups
                .iter()
                .flat_map(|g| g.tabs.iter())
                .all(|&id| id != tab_id.get())
        );
    }

    /// RF-3.8/ADR-0037 §1 (F5 etapa 4): com `lazy_restore = true`, só a
    /// aba ativa da janela nasce `Running` -- as outras, mesmo no mesmo
    /// grupo dela, nascem `NotStarted`.
    #[test]
    fn lazy_restore_starts_only_the_active_tab() {
        let groups = vec![GroupV1 {
            id: 0,
            name: None,
            color: None,
            collapsed: false,
            tabs: vec![0, 1],
        }];
        let tabs = vec![tab_v1(0, "zsh"), tab_v1(1, "bash")];

        let ws = workspace_from_window(&groups, &tabs, Some(1), true, &|_| false);

        let ids: Vec<TabId> = ws.visual_order().collect();
        assert_eq!(
            ws.tab(ids[0]).unwrap().panes().focused().state(),
            PaneState::NotStarted
        );
        assert_eq!(
            ws.tab(ids[1]).unwrap().panes().focused().state(),
            PaneState::Running
        );
        assert_eq!(ws.active_tab(), Some(ids[1]));
    }

    /// `lazy_restore = false`: nenhuma aba nasce `NotStarted`, nem a que
    /// não é a ativa da janela.
    #[test]
    fn lazy_restore_false_starts_every_tab() {
        let groups = vec![GroupV1 {
            id: 0,
            name: None,
            color: None,
            collapsed: false,
            tabs: vec![0, 1],
        }];
        let tabs = vec![tab_v1(0, "zsh"), tab_v1(1, "bash")];

        let ws = workspace_from_window(&groups, &tabs, Some(1), false, &|_| false);

        assert!(
            ws.visual_order()
                .all(|id| ws.tab(id).unwrap().panes().focused().state() == PaneState::Running)
        );
    }

    /// RF-2.17 ponta a ponta (dívida registrada desde a F3): restaurar uma
    /// sessão cuja aba ativa está dentro de um grupo gravado como
    /// colapsado precisa expandir o grupo -- o primeiro caminho real do
    /// app que ativa uma aba oculta. O mecanismo já está em
    /// `Workspace::activate_tab`; este teste é o cenário que faltava.
    #[test]
    fn restoring_the_active_tab_inside_a_collapsed_group_expands_it() {
        let groups = vec![GroupV1 {
            id: 0,
            name: Some("api".to_string()),
            color: Some("blue".to_string()),
            collapsed: true,
            tabs: vec![0, 1],
        }];
        let tabs = vec![tab_v1(0, "zsh"), tab_v1(1, "bash")];

        let ws = workspace_from_window(&groups, &tabs, Some(1), true, &|_| false);

        let active = ws.active_tab().expect("aba ativa restaurada");
        let group = ws.group_of_tab(active).expect("aba pertence ao grupo");
        assert!(
            !ws.group(group).unwrap().is_collapsed(),
            "grupo colapsado que contém a aba ativa restaurada precisa expandir"
        );
        assert!(ws.navigable_order().any(|id| id == active));
    }

    /// Aba `Exited` não aparece no JSON gravado.
    #[test]
    fn exited_tab_is_not_written() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        ws.tab_mut(b).unwrap().mark_exited(1);
        assert!(ws.tab(b).unwrap().panes().focused().state() != PaneState::Running);

        let (groups, tabs, _) = window_from_workspace(&ws);
        assert!(!tabs.iter().any(|t| t.id == b.get()));
        assert!(
            groups
                .iter()
                .flat_map(|g| g.tabs.iter())
                .all(|&id| id != b.get())
        );
        assert!(
            groups
                .iter()
                .flat_map(|g| g.tabs.iter())
                .any(|&id| id == a.get())
        );
    }

    /// Grupo que fica vazio depois do filtro de `Exited` não é gravado.
    #[test]
    fn group_left_empty_by_exited_filter_is_not_written() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let grupo = ws.group_tabs(&[a], "solo", GroupColor::Red).unwrap();
        ws.tab_mut(a).unwrap().mark_exited(0);

        let (groups, _, _) = window_from_workspace(&ws);
        assert!(!groups.iter().any(|g| g.id == grupo.get()));
    }

    /// Cobertura de campo: reprova quando um campo novo de `Tab` não foi
    /// classificado como gravado ou explicitamente descartado. O domínio
    /// deriva `Serialize` (ADR-0006), então introspeccionar as chaves do
    /// JSON pega campo novo sem depender de acesso a campo privado.
    ///
    /// ADR-0053 §2: os seis campos que descreviam um shell (`process_title`,
    /// `cwd`, `shell_name`, o estado de vida, `activity`, `bell`) saíram de
    /// `Tab` e foram para dentro de `panes` (a árvore de painéis, ADR-0053
    /// §1). A árvore inteira é gravada via `TabV1::panes` (`PaneTreeV1`/
    /// `PaneV1`, ADR-0053 §11) -- cada folha carrega `cwd`/`shell_name`
    /// dela; o estado de vida (`process_title`, `activity`, `bell`,
    /// `state`) continua descartado, como sempre foi.
    #[test]
    fn tab_field_coverage() {
        let tab = porecatu_core::Tab::new(porecatu_core::TabId::new(0), "zsh");
        let value = serde_json::to_value(&tab).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();

        // Gravado: id (via TabId, fora da struct), custom_title, e panes
        // (a árvore inteira, via `PaneTreeV1`).
        let mut expected = ["id", "custom_title", "panes"];
        expected.sort_unstable();
        assert_eq!(
            keys, expected,
            "campo novo em Tab não foi classificado em convert.rs"
        );
    }

    /// ADR-0070 §4: cada painel grava o perfil dele (ou nada, no caso do
    /// `[shell]`), e a restauração devolve cada um ao painel certo. Aba de
    /// um painel só e aba dividida, no mesmo arquivo.
    #[test]
    fn round_trip_preserves_the_profile_of_each_pane() {
        let mut ws = Workspace::new();
        let single = ws.append_tab("wsl", None);
        ws.tab_mut(single)
            .unwrap()
            .panes_mut()
            .focused_mut()
            .set_profile(Some("wsl".to_string()));
        let split = ws.append_tab("zsh", None);
        let left = ws.tab(split).unwrap().panes().focused_id();
        let right = ws
            .tab_mut(split)
            .unwrap()
            .panes_mut()
            .split(left, SplitAxis::Vertical, "pwsh", None)
            .unwrap();
        let panes = ws.tab_mut(split).unwrap().panes_mut();
        panes
            .pane_mut(left)
            .unwrap()
            .set_profile(Some("auto:shell:pwsh.exe".to_string()));
        panes
            .pane_mut(right)
            .unwrap()
            .set_profile(Some("prod".to_string()));

        let (groups, tabs, active) = window_from_workspace(&ws);
        let json = serde_json::to_string(&(&groups, &tabs, active)).unwrap();
        let (groups2, tabs2, active2): (Vec<GroupV1>, Vec<TabV1>, Option<u32>) =
            serde_json::from_str(&json).unwrap();
        let rebuilt = workspace_from_window(&groups2, &tabs2, active2, false, &|_| false);

        let profiles_of = |ws: &Workspace| -> Vec<Vec<Option<String>>> {
            ws.visual_order()
                .map(|id| {
                    let tree = ws.tab(id).unwrap().panes();
                    tree.leaves_in_order()
                        .into_iter()
                        .map(|p| tree.pane(p).unwrap().profile().map(str::to_string))
                        .collect()
                })
                .collect()
        };
        assert_eq!(
            profiles_of(&rebuilt),
            vec![
                vec![Some("wsl".to_string())],
                vec![
                    Some("auto:shell:pwsh.exe".to_string()),
                    Some("prod".to_string())
                ],
            ]
        );
        assert_eq!(profiles_of(&rebuilt), profiles_of(&ws));
    }

    /// ADR-0070 §4: o painel do `[shell]` grava o campo **ausente**, e a
    /// aba nunca grava `profile` quando há `panes`.
    #[test]
    fn shell_pane_and_tab_do_not_emit_profile() {
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let profiled = ws.append_tab("wsl", None);
        ws.tab_mut(profiled)
            .unwrap()
            .panes_mut()
            .focused_mut()
            .set_profile(Some("wsl".to_string()));
        let (_, tabs, _) = window_from_workspace(&ws);

        let json: Vec<serde_json::Value> = tabs
            .iter()
            .map(|t| serde_json::to_value(t).unwrap())
            .collect();
        let leaf = |tab: &serde_json::Value| tab["panes"]["root"]["Leaf"].clone();
        assert!(json[0].get("profile").is_none());
        assert!(json[1].get("profile").is_none(), "{}", json[1]);
        assert!(leaf(&json[0]).get("profile").is_none(), "{}", json[0]);
        assert_eq!(leaf(&json[1])["profile"], "wsl");
    }

    /// Sessão gravada antes do ADR-0070: nenhum `profile` em lugar nenhum, e
    /// tudo volta como `[shell]` (`None`).
    #[test]
    fn session_without_the_field_reads_as_shell_everywhere() {
        let with_panes = r#"{"id":0,"custom_title":null,"cwd":null,"spawn_program":"zsh","panes":{"root":{"Split":{"axis":"Vertical","ratio":0.5,"first":{"Leaf":{"id":0,"cwd":null,"spawn_program":"zsh"}},"second":{"Leaf":{"id":1,"cwd":null,"spawn_program":"zsh"}}}},"focused":0}}"#;
        let legacy = r#"{"id":1,"custom_title":null,"cwd":null,"spawn_program":"zsh"}"#;
        let tabs: Vec<TabV1> = [with_panes, legacy]
            .iter()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert!(tabs.iter().all(|t| t.profile.is_none()));

        let groups = [GroupV1 {
            id: 0,
            name: None,
            color: None,
            collapsed: false,
            tabs: vec![0, 1],
        }];
        let ws = workspace_from_window(&groups, &tabs, Some(0), false, &|_| false);
        let mut count = 0;
        for id in ws.visual_order() {
            let tree = ws.tab(id).unwrap().panes();
            for leaf in tree.leaves_in_order() {
                assert_eq!(tree.pane(leaf).unwrap().profile(), None);
                count += 1;
            }
        }
        assert_eq!(count, 3);
    }

    /// RF-20.33: painel SSH nasce `NotStarted` com `lazy_restore = false` e na
    /// aba ativa; o painel local da mesma aba segue a regra de sempre.
    #[test]
    fn an_ssh_pane_is_never_started_by_the_restore_not_even_the_active_one() {
        let leaf = |id, profile: Option<&str>| {
            PaneNodeV1::Leaf(PaneV1 {
                id,
                cwd: Some(PathBuf::from("/srv")),
                spawn_program: Some("zsh".to_string()),
                profile: profile.map(str::to_string),
            })
        };
        let mixed = TabV1 {
            id: 0,
            panes: Some(PaneTreeV1 {
                root: PaneNodeV1::Split {
                    axis: SplitAxisV1::Vertical,
                    ratio: 0.5,
                    first: Box::new(leaf(0, None)),
                    second: Box::new(leaf(1, Some("prod"))),
                },
                focused: 1,
            }),
            ..tab_v1(0, "zsh")
        };
        let mut legacy = tab_v1(1, "zsh");
        legacy.profile = Some("prod".to_string());
        legacy.cwd = Some(PathBuf::from("/srv"));
        let mut local_legacy = tab_v1(2, "zsh");
        local_legacy.profile = Some("wsl".to_string());
        let group = GroupV1 {
            id: 0,
            name: None,
            color: None,
            collapsed: false,
            tabs: vec![0, 1, 2],
        };
        let is_ssh = |id: &str| id == "prod";

        for lazy in [false, true] {
            let ws = workspace_from_window(
                &[group.clone()],
                &[mixed.clone(), legacy.clone(), local_legacy.clone()],
                Some(0),
                lazy,
                &is_ssh,
            );
            let order: Vec<TabId> = ws.visual_order().collect();
            let states = |id: TabId| -> Vec<PaneState> {
                let tree = ws.tab(id).unwrap().panes();
                tree.leaves_in_order()
                    .into_iter()
                    .map(|p| tree.pane(p).unwrap().state())
                    .collect()
            };
            // The active tab: the local pane runs, the SSH one waits -- focused
            // or not.
            assert_eq!(
                states(order[0]),
                [PaneState::Running, PaneState::NotStarted],
                "lazy_restore = {lazy}"
            );
            assert!(ws.tab(order[0]).unwrap().is_not_started(), "SSH is focused");
            // A saved tab without the pane tree, SSH profile: waits as well.
            assert_eq!(states(order[1]), [PaneState::NotStarted]);
            // The same shape with a local profile follows `lazy_restore`.
            let expected = if lazy {
                PaneState::NotStarted
            } else {
                PaneState::Running
            };
            assert_eq!(states(order[2]), [expected]);

            // RF-20.22/RF-20.32: a directory saved for an SSH pane is dropped,
            // the local pane keeps its own.
            let tree = ws.tab(order[0]).unwrap().panes();
            let leaves = tree.leaves_in_order();
            assert_eq!(
                tree.pane(leaves[0]).unwrap().cwd(),
                Some(&PathBuf::from("/srv"))
            );
            assert_eq!(tree.pane(leaves[1]).unwrap().cwd(), None);
            assert_eq!(ws.tab(order[1]).unwrap().cwd(), None);
        }
    }

    /// Caminho sem `panes`: `TabV1::profile` vale para o painel único.
    #[test]
    fn legacy_tab_profile_reaches_the_single_pane() {
        let mut tab = tab_v1(0, "wsl");
        tab.profile = Some("wsl".to_string());
        let group = GroupV1 {
            id: 0,
            name: None,
            color: None,
            collapsed: false,
            tabs: vec![0],
        };
        let ws = workspace_from_window(&[group], &[tab], None, false, &|_| false);
        let id = ws.visual_order().next().unwrap();
        assert_eq!(ws.tab(id).unwrap().panes().focused().profile(), Some("wsl"));
    }

    /// ADR-0070 §4, ADR-0037: a aba `NotStarted` guarda o perfil até o
    /// primeiro foco, nos dois formatos de arquivo.
    #[test]
    fn not_started_panes_carry_the_profile() {
        let leaf = |id, profile: &str| {
            PaneNodeV1::Leaf(PaneV1 {
                id,
                cwd: None,
                spawn_program: Some("zsh".to_string()),
                profile: Some(profile.to_string()),
            })
        };
        let split_tab = TabV1 {
            id: 1,
            panes: Some(PaneTreeV1 {
                root: PaneNodeV1::Split {
                    axis: SplitAxisV1::Vertical,
                    ratio: 0.5,
                    first: Box::new(leaf(0, "a")),
                    second: Box::new(leaf(1, "b")),
                },
                focused: 0,
            }),
            ..tab_v1(1, "zsh")
        };
        let mut legacy_tab = tab_v1(2, "zsh");
        legacy_tab.profile = Some("c".to_string());
        let group = GroupV1 {
            id: 0,
            name: None,
            color: None,
            collapsed: false,
            tabs: vec![0, 1, 2],
        };
        let active = tab_v1(0, "zsh");
        let ws = workspace_from_window(
            &[group],
            &[active, split_tab, legacy_tab],
            Some(0),
            true,
            &|_| false,
        );

        let order: Vec<TabId> = ws.visual_order().collect();
        let profiles_of = |id: TabId| -> Vec<Option<String>> {
            let tree = ws.tab(id).unwrap().panes();
            tree.leaves_in_order()
                .into_iter()
                .map(|p| {
                    let pane = tree.pane(p).unwrap();
                    assert!(pane.is_not_started() || id == order[0]);
                    pane.profile().map(str::to_string)
                })
                .collect()
        };
        assert_eq!(profiles_of(order[1]), [Some("a".into()), Some("b".into())]);
        assert_eq!(profiles_of(order[2]), [Some("c".into())]);
    }

    /// Um `""` gravado à mão volta como `[shell]`, não como perfil de id vazio.
    #[test]
    fn empty_stored_profile_reads_as_shell() {
        assert_eq!(profile_from_v1(Some("")), None);
        assert_eq!(profile_from_v1(None), None);
        assert_eq!(profile_from_v1(Some("wsl")), Some("wsl".to_string()));
    }

    /// Cobertura de campo de `Pane`: `cwd`, `shell_name` e `profile` são
    /// gravados em `PaneV1` (`id` vem do `PaneId` da folha); `process_title`,
    /// `state`, `activity` e `bell` são estado de vida, descartado.
    #[test]
    fn pane_field_coverage() {
        let pane = Pane::new(porecatu_core::PaneId::new(0), "zsh");
        let value = serde_json::to_value(&pane).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = [
            "id",
            "cwd",
            "shell_name",
            "profile",
            "process_title",
            "state",
            "activity",
            "bell",
        ];
        expected.sort_unstable();
        assert_eq!(
            keys, expected,
            "campo novo em Pane não foi classificado em convert.rs"
        );
    }

    /// Cobertura de campo de `Group`: `kind` carrega nome/cor/colapso
    /// (gravados via `GroupV1`), `tabs` é a ordem (gravada), `last_active`
    /// é descartado (ADR-0036 §3). `id` não é campo serializado à parte --
    /// vem do método `Group::id()`, mas `#[derive(Serialize)]` o inclui
    /// como campo normal.
    #[test]
    fn group_field_coverage() {
        let group = porecatu_core::Group::new_implicit(porecatu_core::GroupId::new(0));
        let value = serde_json::to_value(&group).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = ["id", "kind", "tabs", "last_active"];
        expected.sort_unstable();
        assert_eq!(
            keys, expected,
            "campo novo em Group não foi classificado em convert.rs"
        );
    }

    /// Cobertura de campo de `GroupMeta` (nome/cor/colapso, dentro de
    /// `GroupKind::Explicit`) -- os três são gravados via `GroupV1`.
    #[test]
    fn group_meta_field_coverage() {
        let group = porecatu_core::Group::new_explicit(
            porecatu_core::GroupId::new(0),
            "api",
            GroupColor::Red,
        );
        let porecatu_core::GroupKind::Explicit(meta) = group.kind() else {
            unreachable!()
        };
        let value = serde_json::to_value(meta).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = ["name", "color", "collapsed"];
        expected.sort_unstable();
        assert_eq!(
            keys, expected,
            "campo novo em GroupMeta não foi classificado em convert.rs"
        );
    }

    /// Cobertura de campo de `Workspace`: `groups`/`tabs`/`active_tab` são
    /// gravados (via `WindowV1`); `next_tab_id`/`next_group_id` são
    /// descartados -- a reconstrução gera identidade nova (comentário no
    /// topo do módulo).
    #[test]
    fn workspace_field_coverage() {
        let ws = Workspace::new();
        let value = serde_json::to_value(&ws).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = [
            "groups",
            "tabs",
            "active_tab",
            "next_tab_id",
            "next_group_id",
        ];
        expected.sort_unstable();
        assert_eq!(
            keys, expected,
            "campo novo em Workspace não foi classificado em convert.rs"
        );
    }
}
