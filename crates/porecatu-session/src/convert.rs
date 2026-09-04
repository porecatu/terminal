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

use porecatu_core::{GroupColor, TabId, Workspace};

use crate::schema::v1::{GroupV1, TabV1};

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
pub fn workspace_from_window(
    groups: &[GroupV1],
    tabs: &[TabV1],
    active_tab: Option<u32>,
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
            let shell = tab.spawn_program.clone().unwrap_or_default();
            let new_id = ws.new_tab(group_id, shell, tab.cwd.clone(), pos);
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use porecatu_core::TabState;

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

        let rebuilt = workspace_from_window(&groups2, &tabs2, active_tab2);

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

    /// Aba `Exited` não aparece no JSON gravado.
    #[test]
    fn exited_tab_is_not_written() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        ws.tab_mut(b).unwrap().mark_exited(1);
        assert!(ws.tab(b).unwrap().state() != TabState::Running);

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

        // Gravado (via TabV1): id (via TabId, fora da struct), custom_title,
        // cwd, shell_name (-> spawn_program).
        // Descartado (ADR-0036 §3): process_title, state, activity, bell.
        let mut expected = [
            "id",
            "custom_title",
            "process_title",
            "shell_name",
            "cwd",
            "state",
            "activity",
            "bell",
        ];
        expected.sort_unstable();
        assert_eq!(
            keys, expected,
            "campo novo em Tab não foi classificado em convert.rs"
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
