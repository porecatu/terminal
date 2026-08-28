// SPDX-License-Identifier: GPL-3.0-or-later

//! `Workspace` (ADR-0006, revisto pelo ADR-0020): `Vec<Group>` de
//! `Vec<TabId>`, contador de IDs monotônico, aba ativa. Uma janela == um
//! `Workspace` (ADR-0015).
//!
//! Grupo implícito deixou de ser único (ADR-0020 §1): um `Workspace` tem
//! zero ou mais, um por *run* contíguo de abas sem grupo. Colapso passa a
//! afetar a ordem navegável, não só o desenho -- `navigable_order()` ao
//! lado de `visual_order()`. `new_tab` recebe o grupo de destino em vez de
//! escrever em `groups[0]`.

use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::group::{Group, GroupColor};
use crate::id::{GroupId, TabId};
use crate::tab::Tab;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    groups: Vec<Group>,
    /// Dados das abas, sem relação com a ordem visual -- a ordem visual
    /// vive só em `Group::tabs`.
    tabs: Vec<Tab>,
    active_tab: Option<TabId>,
    next_tab_id: u32,
    next_group_id: u32,
}

impl Workspace {
    /// Workspace novo não tem grupo nenhum -- "zero ou mais" (ADR-0020
    /// §1) inclui zero. O primeiro `new_tab` cria o primeiro run
    /// implícito.
    pub fn new() -> Self {
        Self {
            groups: Vec::new(),
            tabs: Vec::new(),
            active_tab: None,
            next_tab_id: 0,
            next_group_id: 0,
        }
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.iter().find(|g| g.id() == id)
    }

    fn group_mut(&mut self, id: GroupId) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id() == id)
    }

    fn group_index(&self, id: GroupId) -> Option<usize> {
        self.groups.iter().position(|g| g.id() == id)
    }

    pub fn group_of_tab(&self, id: TabId) -> Option<GroupId> {
        self.groups
            .iter()
            .find(|g| g.position_of(id).is_some())
            .map(Group::id)
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id() == id)
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id() == id)
    }

    pub const fn active_tab(&self) -> Option<TabId> {
        self.active_tab
    }

    /// Ordem visual: grupos na ordem do `Vec`, abas na ordem dentro de cada
    /// grupo (ADR-0006: "a ordem visual é a ordem do modelo"). É o que a
    /// barra desenha e o que a sessão grava (ADR-0020 §2). **Não muda de
    /// definição** com a F3.
    pub fn visual_order(&self) -> impl Iterator<Item = TabId> + '_ {
        self.groups.iter().flat_map(|g| g.tabs().iter().copied())
    }

    /// `visual_order()` menos as abas de grupo colapsado (RF-2.15,
    /// ADR-0020 §2). Base de `tab.next`/`tab.prev`/`tab.goto_N`. Derivada
    /// por filtro, **nunca** construída em paralelo -- é o que garante que
    /// ela é sempre subsequência de `visual_order()`.
    pub fn navigable_order(&self) -> impl Iterator<Item = TabId> + '_ {
        self.groups
            .iter()
            .filter(|g| !g.is_collapsed())
            .flat_map(|g| g.tabs().iter().copied())
    }

    fn fresh_group_id(&mut self) -> GroupId {
        next_group_id(&mut self.next_group_id)
    }

    /// RF-1.1: cria aba no grupo dado, na posição dada, com o `cwd` que o
    /// chamador já resolveu (aba ativa -> OSC 7 -> `startup_directory`,
    /// ADR-0017). A aba nova se torna a ativa.
    ///
    /// `group`: destino existente (implícito ou explícito), como a tabela
    /// do ADR-0006 prevê (`new_tab(group, pos)`) -- o grupo da aba ativa
    /// como default é responsabilidade de quem chama, não deste método
    /// (ADR-0020 §1). `None`, ou um `GroupId` que não existe mais, cria um
    /// run implícito novo no fim do `Vec<Group>` -- é o caminho de
    /// bootstrap do primeiro tab de um workspace vazio.
    pub fn new_tab(
        &mut self,
        group: Option<GroupId>,
        shell_name: impl Into<String>,
        cwd: Option<PathBuf>,
        pos: usize,
    ) -> TabId {
        let id = TabId::new(self.next_tab_id);
        self.next_tab_id += 1;

        let mut tab = Tab::new(id, shell_name);
        if let Some(cwd) = cwd {
            tab.set_cwd(cwd);
        }
        self.tabs.push(tab);

        let group_index = match group.and_then(|g| self.group_index(g)) {
            Some(index) => index,
            None => {
                let fresh = Group::new_implicit(self.fresh_group_id());
                self.groups.push(fresh);
                self.groups.len() - 1
            }
        };
        self.groups[group_index].insert(pos, id);
        self.activate_tab(id);
        id
    }

    /// Conveniência de `tab.new`: sempre no fim do grupo da aba ativa, ou
    /// num run implícito novo se não houver aba ativa (workspace vazio).
    /// É o caminho que `porecatu-ui` (RF-1.1) usa.
    pub fn append_tab(&mut self, shell_name: impl Into<String>, cwd: Option<PathBuf>) -> TabId {
        let group = self.active_tab().and_then(|id| self.group_of_tab(id));
        let pos = group
            .and_then(|g| self.group(g))
            .map_or(0, |g| g.tabs().len());
        self.new_tab(group, shell_name, cwd, pos)
    }

    /// Cria uma aba **sem grupo**, no fim da barra: no último run
    /// implícito se a barra já terminar em um, num run implícito novo
    /// caso contrário. Ao contrário de [`Self::append_tab`], ignora o
    /// grupo da aba ativa -- é o caminho do botão de nova aba global
    /// (zona fixa à direita da barra), que existe justamente para dar
    /// uma aba fora de qualquer grupo explícito. O "+" de dentro de um
    /// grupo continua sendo o caminho de criar lá (`group.new_tab`).
    pub fn append_ungrouped_tab(
        &mut self,
        shell_name: impl Into<String>,
        cwd: Option<PathBuf>,
    ) -> TabId {
        let trailing_implicit = self
            .groups
            .last()
            .filter(|g| !g.is_explicit())
            .map(|g| (g.id(), g.tabs().len()));
        let (group, pos) = match trailing_implicit {
            Some((id, len)) => (Some(id), len),
            None => (None, 0),
        };
        self.new_tab(group, shell_name, cwd, pos)
    }

    /// Escada de foco do RF-1.5/RF-2.14 (ADR-0020 §3), numa função só,
    /// usada por `close_tab` e `collapse_group`. `group_index` é o grupo
    /// de origem (ainda presente em `self.groups`, possivelmente vazio);
    /// `next_sibling`/`prev_sibling` são a vizinha dentro dele, já
    /// resolvida pelo chamador -- os dois primeiros níveis da escada.
    /// Grupo colapsado é pulado, nunca expandido.
    fn focus_ladder(
        &self,
        group_index: usize,
        next_sibling: Option<TabId>,
        prev_sibling: Option<TabId>,
    ) -> Option<TabId> {
        if let Some(t) = next_sibling {
            return Some(t);
        }
        if let Some(t) = prev_sibling {
            return Some(t);
        }
        for g in self.groups.iter().skip(group_index + 1) {
            if !g.is_collapsed()
                && let Some(&first) = g.tabs().first()
            {
                return Some(first);
            }
        }
        for g in self.groups[..group_index.min(self.groups.len())]
            .iter()
            .rev()
        {
            if !g.is_collapsed()
                && let Some(&last) = g.tabs().last()
            {
                return Some(last);
            }
        }
        None
    }

    /// RF-1.2/RF-1.5: remove a aba. Devolve a aba ativa do workspace
    /// depois da remoção (`None` se ele ficou sem nenhuma aba alcançável).
    /// Não bloqueia em I/O nem em confirmação: quem decide se a aba pode
    /// fechar (RF-1.6, ADR-0017) e quem drena o PTY é o `ui`, antes de
    /// chamar isto. Se o grupo de origem ficar vazio, ele é removido
    /// (RF-2.7); se isso deixar dois runs implícitos lado a lado, eles se
    /// fundem (ADR-0020 §1).
    pub fn close_tab(&mut self, id: TabId) -> Option<TabId> {
        let tab_index = self.tabs.iter().position(|t| t.id() == id)?;
        let group_index = self
            .groups
            .iter()
            .position(|g| g.position_of(id).is_some())?;

        let removed_pos = self.groups[group_index]
            .remove(id)
            .expect("posição verificada acima");
        self.tabs.remove(tab_index);

        if self.active_tab == Some(id) {
            let group = &self.groups[group_index];
            let next_sibling = group.tabs().get(removed_pos).copied();
            let prev_sibling = if next_sibling.is_none() {
                removed_pos
                    .checked_sub(1)
                    .and_then(|p| group.tabs().get(p))
                    .copied()
            } else {
                None
            };
            match self.focus_ladder(group_index, next_sibling, prev_sibling) {
                Some(next) => {
                    self.activate_tab(next);
                }
                None => self.active_tab = None,
            }
        }

        self.normalize_groups();
        self.active_tab
    }

    /// RF-1.15 (arraste) e RF-1.17 (teclado): reordena dentro do próprio
    /// grupo. Mover entre grupos é o arraste/`tab.move_to_group` da etapa
    /// 6 -- fora do escopo deste método de propósito.
    pub fn move_tab(&mut self, id: TabId, pos: usize) -> bool {
        let Some(group) = self.groups.iter_mut().find(|g| g.position_of(id).is_some()) else {
            return false;
        };
        group.move_within(id, pos)
    }

    /// RF-1.13 (clique ativa) e base de `next_tab`/`prev_tab`/goto-índice.
    /// RF-1.22: visitar a aba limpa seus indicadores de atividade e
    /// campainha. Atualiza o MRU (`last_active`) do grupo da aba
    /// (ADR-0020 §6).
    pub fn activate_tab(&mut self, id: TabId) -> bool {
        if self.tab(id).is_none() {
            return false;
        }
        self.active_tab = Some(id);
        self.tab_mut(id).expect("checado acima").clear_indicators();
        if let Some(group) = self.groups.iter_mut().find(|g| g.position_of(id).is_some()) {
            group.set_last_active(id);
        }
        true
    }

    /// RF-1.11, revisto por RF-2.15: próxima aba na ordem **navegável**,
    /// circulando -- abas de grupo colapsado não participam.
    pub fn next_tab(&mut self) -> Option<TabId> {
        self.step_tab(1)
    }

    /// RF-1.11, revisto por RF-2.15: aba anterior na ordem navegável,
    /// circulando.
    pub fn prev_tab(&mut self) -> Option<TabId> {
        self.step_tab(-1)
    }

    fn step_tab(&mut self, delta: isize) -> Option<TabId> {
        let order: Vec<TabId> = self.navigable_order().collect();
        if order.is_empty() {
            return None;
        }
        let current = self.active_tab?;
        let idx = order.iter().position(|&t| t == current)?;
        let len = order.len() as isize;
        let next_idx = (idx as isize + delta).rem_euclid(len) as usize;
        let next = order[next_idx];
        self.activate_tab(next);
        Some(next)
    }

    /// RF-1.12: acesso direto por índice na ordem **navegável** da janela
    /// inteira (0-based; `tab.goto_1` do catálogo de ações é índice 0).
    /// Colapsar um grupo renumera -- deliberado, ADR-0020 §2.
    pub fn tab_at_navigable_index(&self, index: usize) -> Option<TabId> {
        self.navigable_order().nth(index)
    }

    /// `tab.goto_N`: resolve pelo índice navegável e ativa.
    pub fn activate_navigable_index(&mut self, index: usize) -> Option<TabId> {
        let id = self.tab_at_navigable_index(index)?;
        self.activate_tab(id);
        Some(id)
    }

    /// RF-2.13/RF-2.14: colapsa ou expande. `false` sobre grupo implícito
    /// ou `id` inexistente (ADR-0006: implícito não colapsa). Colapsar um
    /// grupo com a aba ativa move o foco pela mesma escada do RF-1.5, a
    /// partir do nível 3 (ADR-0020 §3) -- a vizinha dentro do próprio
    /// grupo não entra, porque o grupo inteiro está saindo de vista, nunca
    /// expandido de volta automaticamente.
    pub fn collapse_group(&mut self, id: GroupId, collapsed: bool) -> bool {
        let Some(group_index) = self.group_index(id) else {
            return false;
        };
        if !self.groups[group_index].set_collapsed(collapsed) {
            return false;
        }
        if collapsed
            && let Some(active) = self.active_tab
            && self.groups[group_index].position_of(active).is_some()
        {
            match self.focus_ladder(group_index, None, None) {
                Some(next) => {
                    self.activate_tab(next);
                }
                None => self.active_tab = None,
            }
        }
        true
    }

    /// RF-2.9. `false` sobre grupo implícito ou `id` inexistente.
    pub fn rename_group(&mut self, id: GroupId, name: impl Into<String>) -> bool {
        self.group_mut(id).is_some_and(|g| g.rename(name))
    }

    /// RF-2.10. `false` sobre grupo implícito ou `id` inexistente.
    pub fn set_group_color(&mut self, id: GroupId, color: GroupColor) -> bool {
        self.group_mut(id).is_some_and(|g| g.set_color(color))
    }

    /// ADR-0020 §5: conta o uso de cada cor entre os grupos existentes
    /// (cor escolhida à mão conta também) e devolve a menos usada; empate
    /// vai para o menor índice da paleta. Com seis grupos ou menos isso é
    /// sempre "a próxima cor ainda não usada" (RF-2.4); a regra de empate
    /// só se manifesta a partir do sétimo.
    pub fn next_auto_color(&self) -> GroupColor {
        let mut counts = [0u32; GroupColor::ALL.len()];
        for g in &self.groups {
            if let Some(color) = g.color() {
                counts[color.index()] += 1;
            }
        }
        let (index, _) = counts
            .iter()
            .enumerate()
            .min_by_key(|&(_, count)| *count)
            .expect("paleta não vazia");
        GroupColor::ALL[index]
    }

    /// RF-2.4/RF-2.5: cria grupo explícito com as abas dadas. A ordem
    /// interna do grupo novo é a ordem em que as abas aparecem **na
    /// barra**, não a ordem de seleção (correção de fato do ADR-0006,
    /// decidida no ADR-0020). `ids` vazio devolve `None` -- seleção vazia
    /// operar sobre a aba ativa é resolvido por quem chama (ADR-0021), não
    /// aqui.
    ///
    /// Um run **implícito** que perde abas para o grupo novo se divide de
    /// verdade em até dois -- o pedaço antes e o pedaço depois (ADR-0020
    /// §1) -- o que é o que permite ao grupo novo nascer no meio da barra,
    /// com abas soltas dos dois lados. Um grupo **explícito** de origem
    /// nunca se divide: ele só perde membro(s) e continua um objeto só
    /// (RF-2.7 remove se ficar vazio). Nenhum documento cobre selecionar
    /// abas que já pertencem a um grupo explícito diferente; esta é uma
    /// escolha de implementação, não uma decisão de produto.
    pub fn group_tabs(
        &mut self,
        ids: &[TabId],
        name: impl Into<String>,
        color: GroupColor,
    ) -> Option<GroupId> {
        if ids.is_empty() {
            return None;
        }
        let selected: HashSet<TabId> = ids.iter().copied().collect();
        if !selected.iter().all(|id| self.tab(*id).is_some()) {
            return None;
        }

        let new_group_id = self.fresh_group_id();
        let new_group_tabs: Vec<TabId> = self
            .visual_order()
            .filter(|t| selected.contains(t))
            .collect();
        let mut new_group_slot = {
            let mut g = Group::new_explicit(new_group_id, name, color);
            for t in &new_group_tabs {
                let p = g.tabs().len();
                g.insert(p, *t);
            }
            Some(g)
        };

        let old_groups = std::mem::take(&mut self.groups);
        let mut rebuilt: Vec<Group> = Vec::with_capacity(old_groups.len() + 1);

        for group in old_groups {
            if group.is_implicit() {
                let mut run: Vec<TabId> = Vec::new();
                for t in group.into_tabs() {
                    if selected.contains(&t) {
                        if !run.is_empty() {
                            let id = next_group_id(&mut self.next_group_id);
                            rebuilt.push(implicit_group_from(id, std::mem::take(&mut run)));
                        }
                        if let Some(g) = new_group_slot.take() {
                            rebuilt.push(g);
                        }
                    } else {
                        run.push(t);
                    }
                }
                if !run.is_empty() {
                    let id = next_group_id(&mut self.next_group_id);
                    rebuilt.push(implicit_group_from(id, run));
                }
            } else {
                // Grupo explícito não se divide (comentário do método) --
                // mas o grupo novo ainda precisa entrar do lado certo do
                // que sobrou, senão a ordem visual inverte: se a primeira
                // aba do grupo original é uma das selecionadas, a extração
                // "começou pela frente" e o grupo novo vai antes do que
                // sobrou; senão, vai depois. Bug real (F3 etapa 6):
                // empurrar sempre antes invertia a ordem sempre que a
                // aba extraída não era a primeira.
                let before = group.tabs().len();
                let starts_with_selected =
                    group.tabs().first().is_some_and(|t| selected.contains(t));
                let mut group = group;
                group.retain_tabs(|t| !selected.contains(&t));
                let shrunk = before != group.tabs().len();
                if shrunk
                    && starts_with_selected
                    && let Some(g) = new_group_slot.take()
                {
                    rebuilt.push(g);
                }
                if !group.is_empty() {
                    rebuilt.push(group);
                }
                if shrunk
                    && !starts_with_selected
                    && let Some(g) = new_group_slot.take()
                {
                    rebuilt.push(g);
                }
            }
        }
        // As abas selecionadas sempre existem em algum grupo (validado no
        // início), então a varredura sempre encontra pelo menos uma --
        // `new_group_slot` já foi consumido nesse ponto. Este `if` é só
        // uma rede de segurança para não perder o grupo silenciosamente.
        if let Some(g) = new_group_slot.take() {
            rebuilt.push(g);
        }

        self.groups = rebuilt;
        self.normalize_groups();
        Some(new_group_id)
    }

    /// RF-2.20 (`tab.move_to_group`): move `tab` pro **fim** de `group`, um
    /// grupo já existente. Conveniência de [`Self::move_tab_to_group_at`]
    /// com a posição resolvida aqui (o chamador do menu não tem como saber
    /// "quantas abas o grupo tem agora" sem consultar de volta).
    pub fn move_tab_to_group(&mut self, tab: TabId, group: GroupId) -> bool {
        let Some(g) = self.group(group) else {
            return false;
        };
        let pos = g.tabs().len();
        self.move_tab_to_group_at(tab, group, pos)
    }

    /// RF-1.16/RF-2.18 (arraste entre grupos, F3 etapa 6): move `tab` pra
    /// dentro de `group`, um grupo já existente (implícito ou explícito),
    /// na posição `pos` dentro dele -- mesma convenção de índice que
    /// `move_tab`/`Group::move_within` (posição entre as abas
    /// **restantes**, saturando no fim). Diferente de `move_tab` -- que só
    /// reordena **dentro** do mesmo grupo, de propósito (ADR-0006) --, este
    /// cruza fronteira de grupo: o grupo de origem some se ficar vazio e
    /// dois runs implícitos adjacentes se fundem, mesma limpeza de
    /// `close_tab` (`normalize_groups`). Soltar sobre grupo colapsado
    /// (ADR-0021 §4) não tem tratamento especial: a aba entra normalmente,
    /// só não aparece na trilha porque o grupo está colapsado. `false` se
    /// `tab` ou `group` não existem, ou se `tab` já está em `group` e a
    /// posição não mudaria (no-op, não erro).
    pub fn move_tab_to_group_at(&mut self, tab: TabId, group: GroupId, pos: usize) -> bool {
        if self.tab(tab).is_none() || self.group(group).is_none() {
            return false;
        }
        let Some(from_index) = self
            .groups
            .iter()
            .position(|g| g.position_of(tab).is_some())
        else {
            return false;
        };
        if self.groups[from_index].id() == group {
            // Mesma fronteira do `Group::move_within` -- reordenar dentro
            // do próprio grupo é `move_tab`, não isto, mas o arraste
            // resolve o alvo sem saber de antemão se cruzou fronteira ou
            // não, então aceitar e delegar aqui é o caminho mais simples.
            return self.groups[from_index].move_within(tab, pos);
        }
        self.groups[from_index].remove(tab);
        let to_index = self.group_index(group).expect("checado acima");
        self.groups[to_index].insert(pos, tab);
        self.normalize_groups();
        true
    }

    /// RF-1.16, segunda frase (soltar fora de qualquer wrapper): move
    /// `tab` pra um run implícito **novo**, inserido na posição
    /// `group_index` da lista de grupos -- entre os grupos que já estão
    /// lá, na mesma ordem que o layout da barra usou pra calcular
    /// `group_index`. Funde com run implícito adjacente automaticamente
    /// (`normalize_groups`), o mesmo mecanismo de `group_tabs`/`ungroup` --
    /// é o que evita dois runs implícitos ficarem lado a lado se o destino
    /// cair colado num run que já existe. `false` se `tab` não existe.
    pub fn move_tab_to_new_run(&mut self, tab: TabId, group_index: usize) -> bool {
        let Some(from_index) = self
            .groups
            .iter()
            .position(|g| g.position_of(tab).is_some())
        else {
            return false;
        };
        self.groups[from_index].remove(tab);
        let fresh_id = self.fresh_group_id();
        let mut new_group = Group::new_implicit(fresh_id);
        new_group.insert(0, tab);
        let insert_at = group_index.min(self.groups.len());
        self.groups.insert(insert_at, new_group);
        self.normalize_groups();
        true
    }

    /// RF-2.19 (arraste do rótulo do grupo, F3 etapa 6): reordena `id`
    /// para a posição `pos` na lista de grupos -- mesma convenção de
    /// índice de `move_tab_to_group_at`, posição entre os grupos
    /// **restantes**, saturando no fim. Grupos não aninham (ADR-0006):
    /// isto só reordena o `Vec<Group>` de nível superior, nunca move um
    /// grupo pra dentro de outro. Pode deixar dois runs implícitos
    /// adjacentes (ex.: mover um grupo explícito de entre dois implícitos
    /// pra fora deles) -- `normalize_groups` funde. `false` se `id` não
    /// existe.
    pub fn move_group(&mut self, id: GroupId, pos: usize) -> bool {
        let Some(from) = self.group_index(id) else {
            return false;
        };
        let pos = pos.min(self.groups.len() - 1);
        if from != pos {
            let group = self.groups.remove(from);
            self.groups.insert(pos, group);
        }
        self.normalize_groups();
        true
    }

    /// RF-2.6: desagrupa. As abas voltam a um run implícito na posição
    /// onde o grupo estava, fundido com vizinhos implícitos se houver
    /// (ADR-0020 §1, "Fusão"). `false` sobre grupo implícito ou `id`
    /// inexistente.
    pub fn ungroup(&mut self, id: GroupId) -> bool {
        let Some(index) = self.group_index(id) else {
            return false;
        };
        if self.groups[index].is_implicit() {
            return false;
        }
        let group = self.groups.remove(index);
        let tabs = group.into_tabs();
        if !tabs.is_empty() {
            let new_id = self.fresh_group_id();
            self.groups.insert(index, implicit_group_from(new_id, tabs));
        }
        self.normalize_groups();
        true
    }

    /// Invariantes de run implícito (ADR-0020 §1, riscos "run vazio
    /// sobrevivendo" e "dois runs adjacentes não se fundirem"): nenhum
    /// grupo (implícito ou explícito, RF-2.7) fica vazio depois de uma
    /// operação, e dois runs implícitos nunca ficam lado a lado. Chamada
    /// ao fim de toda operação que pode alterar a lista de grupos.
    fn normalize_groups(&mut self) {
        self.groups.retain(|g| !g.is_empty());
        let mut i = 0;
        while i + 1 < self.groups.len() {
            if self.groups[i].is_implicit() && self.groups[i + 1].is_implicit() {
                let right = self.groups.remove(i + 1);
                let right_last_active = right.last_active();
                let right_tabs = right.into_tabs();
                {
                    let left = &mut self.groups[i];
                    for t in right_tabs {
                        let p = left.tabs().len();
                        left.insert(p, t);
                    }
                }
                if self.groups[i].last_active().is_none()
                    && let Some(active) = right_last_active
                {
                    self.groups[i].set_last_active(active);
                }
                // não incrementa `i`: reexamina o mesmo índice, para o
                // caso raro de três runs ficarem adjacentes de uma vez.
            } else {
                i += 1;
            }
        }
    }
}

fn next_group_id(counter: &mut u32) -> GroupId {
    let id = GroupId::new(*counter);
    *counter += 1;
    id
}

fn implicit_group_from(id: GroupId, tabs: Vec<TabId>) -> Group {
    let mut g = Group::new_implicit(id);
    for t in tabs {
        let p = g.tabs().len();
        g.insert(p, t);
    }
    g
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab_ids(ws: &Workspace) -> Vec<TabId> {
        ws.tabs.iter().map(Tab::id).collect()
    }

    #[test]
    fn new_tab_becomes_active_and_appends() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        assert_eq!(ws.active_tab(), Some(b));
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, b]);
    }

    /// Bug relatado: o botão "+" global criava a aba dentro do grupo da
    /// aba ativa, porque ele passava por `append_tab`. A aba tem de
    /// nascer fora de qualquer grupo explícito, no fim da barra.
    #[test]
    fn append_ungrouped_tab_escapes_the_active_tabs_group() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a], "servidor", GroupColor::Red).unwrap();
        assert_eq!(ws.group_of_tab(a), Some(group));

        let b = ws.append_ungrouped_tab("zsh", None);
        assert_eq!(ws.active_tab(), Some(b));
        assert_ne!(ws.group_of_tab(b), Some(group));
        let b_group = ws.group_of_tab(b).unwrap();
        assert!(ws.group(b_group).unwrap().is_implicit());
        // Fim da barra, depois do grupo -- não antes dele.
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, b]);
    }

    /// Duas abas do botão global seguidas ficam no **mesmo** run
    /// implícito, não em um run novo cada -- senão a barra acumularia
    /// grupos vazios de sentido a cada clique.
    #[test]
    fn append_ungrouped_tab_reuses_the_trailing_implicit_run() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.group_tabs(&[a], "servidor", GroupColor::Red).unwrap();
        let b = ws.append_ungrouped_tab("zsh", None);
        let c = ws.append_ungrouped_tab("zsh", None);
        assert_eq!(ws.group_of_tab(b), ws.group_of_tab(c));
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, b, c]);
    }

    /// Sem grupo explícito nenhum, o botão global não deve fatiar o run
    /// implícito que já existe: a aba entra nele, no fim.
    #[test]
    fn append_ungrouped_tab_on_a_plain_workspace_stays_in_one_run() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_ungrouped_tab("zsh", None);
        assert_eq!(ws.group_of_tab(a), ws.group_of_tab(b));
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, b]);
    }

    #[test]
    fn new_tab_inherits_given_cwd() {
        let mut ws = Workspace::new();
        let id = ws.append_tab("zsh", Some(PathBuf::from("/home/user/projeto")));
        assert_eq!(
            ws.tab(id).unwrap().cwd(),
            Some(&PathBuf::from("/home/user/projeto"))
        );
    }

    // Invariante do ADR-0006: todo TabId está em exatamente um grupo.
    #[test]
    fn every_tab_is_in_exactly_one_group() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);

        let mut seen = Vec::new();
        for group in ws.groups() {
            seen.extend_from_slice(group.tabs());
        }
        seen.sort_by_key(|id| id.get());
        let mut expected = [a, b, c];
        expected.sort_by_key(|id| id.get());
        assert_eq!(seen, expected);
    }

    // Invariante do ADR-0006: ordem total, sem lacunas.
    #[test]
    fn order_is_total_and_gapless() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_of_tab(a);
        let b = ws.new_tab(group, "zsh", None, 1);
        let c = ws.new_tab(group, "zsh", None, 1); // inserida entre a e b
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, c, b]);
    }

    // Cenário de aceite do PRD-001: "foco após fechar".
    #[test]
    fn closing_active_tab_focuses_next_sibling() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        ws.activate_tab(b);

        let active = ws.close_tab(b);
        assert_eq!(active, Some(c));
        assert_eq!(tab_ids(&ws), [a, c]);
    }

    #[test]
    fn closing_active_last_tab_focuses_previous_sibling() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        ws.activate_tab(b);

        let active = ws.close_tab(b);
        assert_eq!(active, Some(a));
    }

    #[test]
    fn closing_last_tab_leaves_workspace_without_active_tab() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        assert_eq!(ws.close_tab(a), None);
        assert!(ws.tab(a).is_none());
        assert!(ws.groups().is_empty(), "run implícito vazio não sobrevive");
    }

    #[test]
    fn closing_inactive_tab_keeps_focus() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        ws.activate_tab(a);

        let active = ws.close_tab(b);
        assert_eq!(active, Some(a));
    }

    // ADR-0020 §3, nível 3/4: fechar a última aba de um grupo foca o
    // grupo adjacente, seguinte antes de anterior.
    #[test]
    fn closing_last_tab_of_group_focuses_next_group_first() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None); // um run implícito só: [a, b]
        // divide o run: "left"=[a] explícito, [b] continua implícito
        let group_a = ws.group_tabs(&[a], "left", GroupColor::Blue).unwrap();
        ws.activate_tab(a);

        // fecha a única aba do grupo "left": deve pular pro run seguinte (b)
        let active = ws.close_tab(a);
        assert_eq!(active, Some(b));
        assert!(ws.group(group_a).is_none());
    }

    #[test]
    fn closing_last_tab_of_last_group_focuses_previous_group() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let group_b = ws.group_tabs(&[b], "right", GroupColor::Blue).unwrap();
        ws.activate_tab(b);

        let active = ws.close_tab(b);
        assert_eq!(active, Some(a));
        assert!(ws.group(group_b).is_none());
    }

    #[test]
    fn move_tab_reorders_within_group() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);

        assert!(ws.move_tab(c, 0));
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [c, a, b]);
    }

    #[test]
    fn move_unknown_tab_is_noop() {
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        assert!(!ws.move_tab(TabId::new(999), 0));
    }

    #[test]
    fn next_and_prev_tab_wrap_around() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        ws.activate_tab(a);

        assert_eq!(ws.next_tab(), Some(b));
        assert_eq!(ws.next_tab(), Some(a));
        assert_eq!(ws.prev_tab(), Some(b));
    }

    #[test]
    fn activating_tab_clears_indicators() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        ws.tab_mut(b).unwrap().mark_activity();
        ws.tab_mut(b).unwrap().mark_bell();
        ws.activate_tab(a); // não é b: não deveria afetar b

        assert!(ws.tab(b).unwrap().activity());
        ws.activate_tab(b);
        assert!(!ws.tab(b).unwrap().activity());
        assert!(!ws.tab(b).unwrap().bell());
    }

    #[test]
    fn activating_tab_updates_group_last_active() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let group = ws.group_of_tab(a).unwrap();
        ws.activate_tab(b);
        assert_eq!(ws.group(group).unwrap().last_active(), Some(b));
        ws.activate_tab(a);
        assert_eq!(ws.group(group).unwrap().last_active(), Some(a));
    }

    #[test]
    fn goto_navigable_index_matches_tab_goto_n() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        assert_eq!(ws.tab_at_navigable_index(0), Some(a));
        assert_eq!(ws.activate_navigable_index(1), Some(b));
        assert_eq!(ws.active_tab(), Some(b));
        assert_eq!(ws.tab_at_navigable_index(2), None);
    }

    // Invariante do ADR-0020: navigable_order() é sempre subsequência de
    // visual_order().
    #[test]
    fn navigable_order_is_subsequence_of_visual_order() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[b, c], "g", GroupColor::Blue).unwrap();
        ws.collapse_group(group, true);

        let visual: Vec<TabId> = ws.visual_order().collect();
        let navigable: Vec<TabId> = ws.navigable_order().collect();
        assert_eq!(navigable, [a]);

        let mut vi = visual.iter();
        assert!(navigable.iter().all(|n| vi.any(|v| v == n)));
    }

    // Cenário de aceite (RF-2.15): next/prev pulam abas de grupo colapsado.
    #[test]
    fn next_and_prev_tab_skip_collapsed_group() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[b], "col", GroupColor::Blue).unwrap();
        ws.collapse_group(group, true);
        ws.activate_tab(a);

        assert_eq!(ws.next_tab(), Some(c));
        assert_eq!(ws.next_tab(), Some(a));
        assert_eq!(ws.prev_tab(), Some(c));
    }

    // Cenário de aceite: colapso remove da navegação.
    #[test]
    fn collapsed_group_tabs_are_not_navigable() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let d = ws.append_tab("zsh", None);
        let e = ws.append_tab("zsh", None);
        let expanded = ws.group_tabs(&[a, b, c], "exp", GroupColor::Blue).unwrap();
        let collapsed = ws.group_tabs(&[d, e], "col", GroupColor::Green).unwrap();
        ws.collapse_group(collapsed, true);

        assert!(ws.group(expanded).unwrap().tabs().len() == 3);
        assert_eq!(ws.navigable_order().collect::<Vec<_>>(), [a, b, c]);
    }

    // Cenário de aceite: colapso desloca o foco.
    #[test]
    fn collapsing_group_with_active_tab_moves_focus_out() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let group_bc = ws.group_tabs(&[b, c], "bc", GroupColor::Blue).unwrap();
        ws.activate_tab(b);

        assert!(ws.collapse_group(group_bc, true));
        assert_eq!(ws.active_tab(), Some(a));
    }

    #[test]
    fn implicit_group_cannot_collapse_rename_or_recolor() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_of_tab(a).unwrap();
        assert!(!ws.collapse_group(group, true));
        assert!(!ws.rename_group(group, "x"));
        assert!(!ws.set_group_color(group, GroupColor::Blue));
    }

    // Cenário de aceite: agrupar abas não adjacentes -- "grupo no meio da
    // barra", a divisão real de um run implícito (ADR-0020 §1).
    #[test]
    fn group_tabs_forms_group_in_the_middle_of_the_bar() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let d = ws.append_tab("zsh", None);
        let e = ws.append_tab("zsh", None);
        // seleciona 1, 3 e 5 (posições 0, 2, 4): a, c, e
        let group = ws.group_tabs(&[a, c, e], "api", GroupColor::Blue).unwrap();

        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, c, e, b, d]);
        assert_eq!(ws.groups().len(), 2);
        assert_eq!(ws.group(group).unwrap().tabs(), [a, c, e]);
        assert!(ws.group(group).unwrap().is_explicit());
        let remaining_group = ws.group_of_tab(b).unwrap();
        assert_eq!(ws.group(remaining_group).unwrap().tabs(), [b, d]);
        assert!(ws.group(remaining_group).unwrap().is_implicit());
    }

    // Bug real (F3 etapa 6, achado ao investigar animação de colapso que só
    // funcionava pro primeiro grupo): agrupar a partir de um grupo
    // EXPLÍCITO já existente empurrava o grupo novo sempre antes do que
    // sobrava, mesmo quando a aba extraída vinha depois -- invertendo a
    // ordem visual. Grupo explícito não se divide (RF-2.7), mas o novo
    // precisa entrar do lado certo do que sobrou.
    #[test]
    fn group_tabs_from_explicit_source_keeps_relative_order_when_extracting_the_last_tab() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        let source = ws.group_tabs(&[a, b], "source", GroupColor::Red).unwrap();

        // extrai a ÚLTIMA aba (b) -- ela vinha depois de a, então o grupo
        // novo tem que ficar depois do que sobrou (source, só com a).
        let extracted = ws.group_tabs(&[b], "extracted", GroupColor::Blue).unwrap();

        assert_eq!(
            ws.groups().iter().map(|g| g.id()).collect::<Vec<_>>(),
            [source, extracted]
        );
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, b]);
    }

    #[test]
    fn group_tabs_from_explicit_source_keeps_relative_order_when_extracting_the_first_tab() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        let source = ws.group_tabs(&[a, b], "source", GroupColor::Red).unwrap();

        // extrai a PRIMEIRA aba (a) -- o grupo novo fica antes do que
        // sobrou (source, só com b).
        let extracted = ws.group_tabs(&[a], "extracted", GroupColor::Blue).unwrap();

        assert_eq!(
            ws.groups().iter().map(|g| g.id()).collect::<Vec<_>>(),
            [extracted, source]
        );
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, b]);
    }

    // Cenário de aceite: cor automática não repete até a sexta.
    #[test]
    fn auto_color_does_not_repeat_below_seven_groups() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let first_color = ws.next_auto_color();
        ws.group_tabs(&[a], "g1", first_color).unwrap();

        let second_color = ws.next_auto_color();
        assert_ne!(second_color, first_color);
    }

    // ADR-0020 §5: passado o sexto grupo, repete a menos usada, empate no
    // menor índice.
    #[test]
    fn auto_color_repeats_least_used_after_palette_exhausted() {
        let mut ws = Workspace::new();
        for _ in 0..7 {
            // `None` força um run implícito novo a cada volta -- se
            // usasse `append_tab`, a aba cairia no grupo explícito que a
            // volta anterior acabou de criar (ela continua ativa).
            let t = ws.new_tab(None, "zsh", None, 0);
            let color = ws.next_auto_color();
            ws.group_tabs(&[t], "g", color).unwrap();
        }
        // 7 grupos, 6 cores: uma cor aparece 2x, as outras 5 uma vez cada.
        let mut counts = [0u32; 6];
        for g in ws.groups() {
            counts[g.color().unwrap().index()] += 1;
        }
        assert_eq!(counts.iter().filter(|&&c| c == 2).count(), 1);
        assert_eq!(counts.iter().filter(|&&c| c == 1).count(), 5);
    }

    // Cenário de aceite (RF-2.20): mover aba pra outro grupo cruza
    // fronteira e limpa o run implícito de origem se ele ficar vazio.
    #[test]
    fn move_tab_to_group_crosses_group_boundary() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        let dest = ws.group_tabs(&[b], "dest", GroupColor::Blue).unwrap();

        assert!(ws.move_tab_to_group(a, dest));
        assert_eq!(ws.group(dest).unwrap().tabs(), [b, a]);
        // o run implícito de origem ficou vazio e some.
        assert_eq!(ws.groups().len(), 1);
    }

    #[test]
    fn move_tab_to_group_moving_to_own_group_is_a_successful_noop() {
        // Diferente da versão da etapa 5: `move_tab_to_group` agora
        // delega a `move_tab_to_group_at`, que trata "mesmo grupo" como
        // reordenação (`Group::move_within`), não como erro -- mesma
        // convenção de `move_within`, que também devolve `true` quando
        // `from == pos`. Popover de destino já filtra o grupo atual da
        // aba (`move_to_group.rs`), então este caso não chega da UI.
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_of_tab(a).unwrap();
        assert!(ws.move_tab_to_group(a, group));
        assert_eq!(ws.group(group).unwrap().tabs(), [a]);
    }

    #[test]
    fn move_tab_to_group_fails_for_unknown_ids() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a], "g", GroupColor::Blue).unwrap();
        assert!(!ws.move_tab_to_group(TabId::new(999), group));
        assert!(!ws.move_tab_to_group(a, GroupId::new(999)));
    }

    // Cenário de aceite (RF-1.16/RF-2.18, F3 etapa 6): arraste pra posição
    // arbitrária dentro de um grupo já existente.
    #[test]
    fn move_tab_to_group_at_inserts_in_the_middle() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        let c = ws.append_tab("fish", None);
        let dest = ws.group_tabs(&[b, c], "dest", GroupColor::Blue).unwrap();

        assert!(ws.move_tab_to_group_at(a, dest, 1));
        assert_eq!(ws.group(dest).unwrap().tabs(), [b, a, c]);
    }

    #[test]
    fn move_tab_to_group_at_within_same_group_reorders() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        let c = ws.append_tab("fish", None);
        let group = ws.group_of_tab(a).unwrap();

        assert!(ws.move_tab_to_group_at(c, group, 0));
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [c, a, b]);
    }

    // Cenário de aceite: soltar depois do último grupo cria run implícito
    // novo ali -- o caso real que `tab_bar::drag_target` produz (fora da
    // trilha, à direita de tudo).
    #[test]
    fn move_tab_to_new_run_creates_run_after_the_last_group() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group_a = ws.group_tabs(&[a], "a", GroupColor::Blue).unwrap();
        let b = ws.new_tab(Some(group_a), "bash", None, 1); // dentro de "a" por ora

        assert!(ws.move_tab_to_new_run(b, ws.groups().len()));
        assert_eq!(ws.groups().len(), 2);
        let new_run = ws.group_of_tab(b).unwrap();
        assert!(ws.group(new_run).unwrap().is_implicit());
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, b]);
        assert_eq!(ws.group(group_a).unwrap().tabs(), [a]);
    }

    // Cenário de aceite: soltar antes do primeiro grupo, quando ele já é
    // implícito, funde no run existente em vez de deixar dois lado a lado.
    #[test]
    fn move_tab_to_new_run_merges_with_adjacent_implicit_run() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None); // implícito
        let group_a = ws.group_of_tab(a).unwrap();
        let group = ws.group_tabs(&[a], "g", GroupColor::Blue).unwrap();
        // agrupar "a" sozinho não deixa run implícito nenhum pra trás
        // (era o único); recria um segundo run pra ter o que fundir.
        assert!(group != group_a);
        let b = ws.new_tab(Some(group), "bash", None, 1); // dentro de "g" por ora

        // solta "b" na posição 0 (antes do único grupo, que é "g" com a
        // e b -- ainda sem run implícito). Cria um sozinho por ora.
        assert!(ws.move_tab_to_new_run(b, 0));
        assert_eq!(ws.groups().len(), 2);
        let new_run = ws.group_of_tab(b).unwrap();
        assert!(ws.group(new_run).unwrap().is_implicit());

        // solta de novo na posição 0: agora já existe um run implícito
        // ali (o "new_run" recém-criado) -- deve fundir, não duplicar.
        let c = ws.new_tab(Some(group), "fish", None, 0);
        assert!(ws.move_tab_to_new_run(c, 0));
        assert_eq!(ws.groups().len(), 2);
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [c, b, a]);
    }

    // Cenário de aceite (RF-2.19): arrastar o rótulo do grupo reordena a
    // lista de grupos.
    #[test]
    fn move_group_reorders_group_list() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        let g1 = ws.group_tabs(&[a], "g1", GroupColor::Blue).unwrap();
        let g2 = ws.group_tabs(&[b], "g2", GroupColor::Green).unwrap();

        assert!(ws.move_group(g2, 0));
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [b, a]);
        assert_eq!(
            ws.groups().iter().map(|g| g.id()).collect::<Vec<_>>(),
            [g2, g1]
        );
    }

    #[test]
    fn move_group_merges_adjacent_implicit_runs_left_behind() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None); // implícito
        let b = ws.append_tab("bash", None); // mesmo implícito de a
        let c = ws.new_tab(None, "fish", None, 0); // segundo run implícito
        let group = ws.group_tabs(&[c], "mid", GroupColor::Blue).unwrap();
        // ordem: [implícito: a,b] [mid: c] -- move "mid" pro início,
        // deixando os dois implícitos adjacentes de novo (devem fundir).
        assert!(ws.move_group(group, 0));
        assert_eq!(ws.groups().len(), 2);
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [c, a, b]);
    }

    #[test]
    fn move_group_unknown_id_is_noop() {
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        assert!(!ws.move_group(GroupId::new(999), 0));
    }

    // Cenário de aceite: desagrupar preserva ordem e posição.
    #[test]
    fn ungroup_preserves_order_and_position() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let d = ws.append_tab("zsh", None);
        let e = ws.append_tab("zsh", None);
        // grupo "api" com b,c,d na posição central: já contíguas, então
        // agrupar não reordena nada -- só rotula.
        let group = ws.group_tabs(&[b, c, d], "api", GroupColor::Blue).unwrap();
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, b, c, d, e]);

        assert!(ws.ungroup(group));
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, b, c, d, e]);
        assert!(ws.group(group).is_none());
        let now_implicit = ws.group_of_tab(b).unwrap();
        assert!(ws.group(now_implicit).unwrap().is_implicit());
    }

    // ADR-0020 §1 "Fusão": desagrupar entre dois runs implícitos produz
    // um run só, não dois.
    #[test]
    fn ungroup_merges_with_adjacent_implicit_runs() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[b], "solo", GroupColor::Blue).unwrap();
        assert_eq!(ws.groups().len(), 3); // [a] [solo:b] [c]

        assert!(ws.ungroup(group));
        assert_eq!(ws.groups().len(), 1); // [a,b,c] fundidos
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [a, b, c]);
    }

    // Invariante: nenhum grupo implícito vazio sobrevive, nenhum par
    // implícito fica adjacente, depois de uma sequência de operações.
    #[test]
    fn implicit_run_invariants_hold_after_operation_sequence() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("zsh", None);
        let c = ws.append_tab("zsh", None);
        let d = ws.append_tab("zsh", None);

        let g1 = ws.group_tabs(&[b], "g1", GroupColor::Blue).unwrap();
        let g2 = ws.group_tabs(&[d], "g2", GroupColor::Green).unwrap();
        assert_invariants(&ws);

        ws.close_tab(a);
        assert_invariants(&ws);
        ws.ungroup(g1);
        assert_invariants(&ws);
        ws.ungroup(g2);
        assert_invariants(&ws);
        assert_eq!(ws.groups().len(), 1);
        assert_eq!(ws.visual_order().collect::<Vec<_>>(), [b, c, d]);
    }

    fn assert_invariants(ws: &Workspace) {
        assert!(
            ws.groups().iter().all(|g| !g.is_empty()),
            "grupo vazio sobrevivendo: {:?}",
            ws.groups()
        );
        for pair in ws.groups().windows(2) {
            assert!(
                !(pair[0].is_implicit() && pair[1].is_implicit()),
                "dois runs implícitos adjacentes: {:?}",
                ws.groups()
            );
        }
        let visual: Vec<TabId> = ws.visual_order().collect();
        let navigable: Vec<TabId> = ws.navigable_order().collect();
        let mut vi = 0;
        for &n in &navigable {
            while vi < visual.len() && visual[vi] != n {
                vi += 1;
            }
            assert!(vi < visual.len(), "navigable_order não é subsequência");
            vi += 1;
        }
    }

    // Invariante do ADR-0006: round-trip Workspace -> JSON -> Workspace
    // preserva IDs, ordem e metadados -- incluindo grupo explícito e
    // implícito misturados.
    #[test]
    fn round_trips_through_json() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", Some(PathBuf::from("/home/user")));
        let b = ws.append_tab("bash", None);
        let c = ws.append_tab("fish", None);
        ws.tab_mut(a)
            .unwrap()
            .set_custom_title(Some("backend".to_string()));
        ws.tab_mut(b).unwrap().mark_activity();
        let group = ws.group_tabs(&[b], "api", GroupColor::Purple).unwrap();
        ws.rename_group(group, "backend-api");
        ws.collapse_group(group, true);
        ws.activate_tab(c);

        let json = serde_json::to_string(&ws).expect("serializa");
        let restored: Workspace = serde_json::from_str(&json).expect("deserializa");

        assert_eq!(ws, restored);
    }
}
