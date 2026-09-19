// SPDX-License-Identifier: GPL-3.0-or-later

//! `Pane` e a árvore binária de painéis (ADR-0053 §1). Uma aba passa a
//! conter uma `PaneTree`: os nós são folhas (`PaneId`) ou splits, e cada
//! split guarda o eixo do divisor e a posição dele (`ratio`, sempre no
//! intervalo aberto `(0, 1)`).
//!
//! **Nomenclatura dos eixos**: segue o nome da ação que produz o split, não
//! um sinônimo visual -- "horizontal" é ambíguo entre emuladores de
//! terminal, e o PRD-006 (RF-6.1) fixa qual lado vale:
//!
//! | Ação                     | O divisor fica | O painel novo nasce |
//! |---------------------------|-----------------|----------------------|
//! | `pane.split_horizontal`   | deitado          | abaixo do focado     |
//! | `pane.split_vertical`     | em pé            | à direita do focado  |
//!
//! `SplitAxis::Horizontal` é o divisor deitado (painéis empilhados, um
//! embaixo do outro); `SplitAxis::Vertical` é o divisor em pé (painéis lado
//! a lado). O nome da variante casa com o nome da ação que o produz.
//!
//! Como o resto do domínio, as operações são puras e testáveis sem janela
//! -- molde dos métodos de `Workspace`: `split`, `close`, `focus`,
//! `focus_in_direction`, `set_ratio`, `leaves_in_order`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::id::PaneId;

/// Estado de vida do painel (ADR-0017 item 6, ADR-0037 §1, ADR-0053 §10 --
/// desceu de `Tab` para cá). Mesma escada de decisão de antes, um nível
/// abaixo: `NotStarted -> Running` no primeiro foco da aba, `Running ->
/// Exited` quando o processo morre. Sem volta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneState {
    NotStarted,
    Running,
    Exited { exit_code: i32 },
}

/// Um painel: um terminal completo, com PTY, grade, scrollback, diretório e
/// ciclo de vida próprios (RF-6.5). Não carrega PTY nem motor VT -- isso é
/// `porecatu-term`, do outro lado da fronteira da seção 4 da arquitetura.
/// `Pane` só guarda o que o domínio precisa para derivar o que a aba
/// mostra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pane {
    id: PaneId,
    /// Último título recebido por OSC 0 / OSC 2.
    process_title: Option<String>,
    /// Nome do shell spawnado -- fallback de última instância, sempre
    /// presente.
    shell_name: String,
    /// Diretório de trabalho conhecido, capturado por OSC 7. `None` até o
    /// primeiro OSC 7 chegar; quem decide o fallback (`startup_directory`)
    /// é o chamador, não este tipo.
    cwd: Option<PathBuf>,
    state: PaneState,
    /// Indicador de atividade (RF-1.20 / RF-6.18): saída nova enquanto o
    /// painel não é o focado da aba visível.
    activity: bool,
    /// Indicador de campainha (RF-1.21 / RF-6.18), distinto do de
    /// atividade.
    bell: bool,
}

impl Pane {
    pub fn new(id: PaneId, shell_name: impl Into<String>) -> Self {
        Self {
            id,
            process_title: None,
            shell_name: shell_name.into(),
            cwd: None,
            state: PaneState::Running,
            activity: false,
            bell: false,
        }
    }

    /// ADR-0037 §1/ADR-0053 §10: único jeito de nascer `NotStarted` --
    /// reservado à restauração de sessão. Todo outro caminho (split,
    /// `tab.new`, `group.new_tab`, `window.new`) usa [`Self::new`], que
    /// nasce `Running`.
    pub fn new_not_started(id: PaneId, shell_name: impl Into<String>) -> Self {
        Self {
            state: PaneState::NotStarted,
            ..Self::new(id, shell_name)
        }
    }

    pub const fn id(&self) -> PaneId {
        self.id
    }

    /// Título do painel, na mesma precedência que `Tab::title` aplicava
    /// antes de existir painel: OSC 0/2 -> nome do shell. `custom_title` é
    /// da aba, não entra aqui (RF-6.17) -- `Tab::title` o aplica por cima.
    pub fn title(&self) -> &str {
        self.process_title.as_deref().unwrap_or(&self.shell_name)
    }

    /// Aplica um título vindo de OSC 0 / OSC 2.
    pub fn set_process_title(&mut self, title: Option<String>) {
        self.process_title = title;
    }

    pub fn cwd(&self) -> Option<&PathBuf> {
        self.cwd.as_ref()
    }

    pub fn shell_name(&self) -> &str {
        &self.shell_name
    }

    /// Captura de OSC 7.
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = Some(cwd);
    }

    pub const fn state(&self) -> PaneState {
        self.state
    }

    pub const fn is_exited(&self) -> bool {
        matches!(self.state, PaneState::Exited { .. })
    }

    pub const fn is_not_started(&self) -> bool {
        matches!(self.state, PaneState::NotStarted)
    }

    pub const fn accepts_input(&self) -> bool {
        matches!(self.state, PaneState::Running)
    }

    /// Primeiro foco da aba que contém este painel, ainda sem shell
    /// (ADR-0037 §2). Sem volta: chamar fora de `NotStarted` não faz nada.
    pub fn start(&mut self) {
        if self.state == PaneState::NotStarted {
            self.state = PaneState::Running;
        }
    }

    /// RF-6.11: código zero fecha o painel (a aba, se ele era o último);
    /// código diferente de zero mantém o painel aberto com a nota de
    /// saída, como a aba fazia antes de existir painel.
    pub fn mark_exited(&mut self, exit_code: i32) {
        self.state = PaneState::Exited { exit_code };
        self.activity = false;
        self.bell = false;
    }

    pub const fn activity(&self) -> bool {
        self.activity
    }

    pub fn mark_activity(&mut self) {
        self.activity = true;
    }

    pub const fn bell(&self) -> bool {
        self.bell
    }

    pub fn mark_bell(&mut self) {
        self.bell = true;
    }

    /// Chamado por `Tab::clear_indicators` para cada painel da árvore --
    /// RF-1.22/RF-6.18: visitar a aba limpa os indicadores agregados dela,
    /// que dependem de nenhum painel ter indicador aceso.
    pub(crate) fn clear_indicators(&mut self) {
        self.activity = false;
        self.bell = false;
    }
}

/// Eixo do divisor -- ver a nota de nomenclatura no topo do módulo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitAxis {
    /// Divisor deitado: painéis empilhados, um acima do outro
    /// (`pane.split_horizontal`).
    Horizontal,
    /// Divisor em pé: painéis lado a lado (`pane.split_vertical`).
    Vertical,
}

/// Direção geométrica de `PaneTree::focus_in_direction` (RF-6.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Lado descido a partir de um nó `Split`, um passo do caminho até ele a
/// partir da raiz -- endereça um divisor por **posição estrutural**, não
/// por painel vizinho. `PaneTree::set_ratio` (por `PaneId` de uma folha
/// filha imediata) não alcança um split cujos dois filhos já são splits --
/// um layout em cruz de quatro painéis (RF-6.2, aninhamento sem limite) tem
/// exatamente essa forma para o divisor externo. `path` é a mesma
/// travessia `first`/`second` que `porecatu_ui::panes::layout` já faz para
/// desenhar o vão, então sempre existe um caminho válido para qualquer
/// divisor que apareça em tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    First,
    Second,
}

/// Nó da árvore binária: folha (um painel) ou split (dois filhos, um eixo e
/// a posição do divisor). `ratio` é a fração do retângulo que `first`
/// recebe -- sempre no intervalo aberto `(0, 1)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PaneNode {
    Leaf(PaneId),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

/// Menor distância de `ratio` até 0 ou 1 -- garante o intervalo aberto do
/// invariante sem precisar de um mínimo de config aqui (RF-6.4/RF-6.14 são
/// quem gateia o split e clampa o arraste de verdade, com `[panes]
/// min_columns`/`min_rows`; isto só evita o degenerado matemático de um
/// painel de largura/altura zero na representação).
const RATIO_EPSILON: f32 = 0.001;

/// Retângulo normalizado (0..1 nos dois eixos), usado só para achar o
/// vizinho geométrico de `focus_in_direction` -- não é layout de pixel
/// (isso é `porecatu-ui::panes`, etapa 3), é geometria relativa da árvore.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl Rect {
    const UNIT: Self = Self {
        x0: 0.0,
        y0: 0.0,
        x1: 1.0,
        y1: 1.0,
    };

    fn split(self, axis: SplitAxis, ratio: f32) -> (Self, Self) {
        match axis {
            SplitAxis::Horizontal => {
                let mid = self.y0 + (self.y1 - self.y0) * ratio;
                (Self { y1: mid, ..self }, Self { y0: mid, ..self })
            }
            SplitAxis::Vertical => {
                let mid = self.x0 + (self.x1 - self.x0) * ratio;
                (Self { x1: mid, ..self }, Self { x0: mid, ..self })
            }
        }
    }
}

/// Resultado de [`PaneTree::close`] (RF-6.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneCloseOutcome {
    /// O painel fechou; carrega o painel que ficou focado (o mesmo de
    /// antes, se o fechado não era o focado).
    Closed(PaneId),
    /// Era o último painel da árvore -- ela não muda. Quem chama fecha a
    /// aba inteira.
    WasLastPane,
}

/// A árvore de uma aba: os nós (`PaneNode`), os dados de cada painel
/// (`Pane`, numa lista plana -- mesmo desenho de `Workspace.tabs` guardando
/// dado separado da ordem/estrutura) e qual painel está focado. Sempre tem
/// pelo menos um painel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneTree {
    root: PaneNode,
    panes: Vec<Pane>,
    focused: PaneId,
    next_pane_id: u32,
}

impl PaneTree {
    fn new_with_root_pane(pane: impl FnOnce(PaneId) -> Pane) -> Self {
        let mut next_pane_id = 0;
        let id = fresh_pane_id(&mut next_pane_id);
        Self {
            root: PaneNode::Leaf(id),
            panes: vec![pane(id)],
            focused: id,
            next_pane_id,
        }
    }

    /// Árvore nova de uma aba nova: uma folha só, focada.
    pub fn new(shell_name: impl Into<String>) -> Self {
        let shell_name = shell_name.into();
        Self::new_with_root_pane(|id| Pane::new(id, shell_name))
    }

    /// ADR-0037 §1: a única folha nasce `NotStarted` -- reservado à
    /// restauração de sessão.
    pub fn new_not_started(shell_name: impl Into<String>) -> Self {
        let shell_name = shell_name.into();
        Self::new_with_root_pane(|id| Pane::new_not_started(id, shell_name))
    }

    pub const fn root(&self) -> &PaneNode {
        &self.root
    }

    pub fn panes(&self) -> &[Pane] {
        &self.panes
    }

    pub fn panes_mut(&mut self) -> impl Iterator<Item = &mut Pane> {
        self.panes.iter_mut()
    }

    pub const fn focused_id(&self) -> PaneId {
        self.focused
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id() == id)
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|p| p.id() == id)
    }

    /// O painel focado -- sempre existe: todo `PaneTree` tem pelo menos um
    /// painel, e `focused` é sempre folha da própria árvore (invariante).
    pub fn focused(&self) -> &Pane {
        self.pane(self.focused)
            .expect("focused é sempre folha válida")
    }

    pub fn focused_mut(&mut self) -> &mut Pane {
        self.pane_mut(self.focused)
            .expect("focused é sempre folha válida")
    }

    fn fresh_pane_id(&mut self) -> PaneId {
        fresh_pane_id(&mut self.next_pane_id)
    }

    /// Todas as folhas, na ordem estável de `first` antes de `second`,
    /// recursivamente -- sem repetir, cobrindo todas.
    pub fn leaves_in_order(&self) -> Vec<PaneId> {
        let mut out = Vec::with_capacity(self.panes.len());
        collect_leaves(&self.root, &mut out);
        out
    }

    /// RF-6.1/RF-6.2: divide o painel `id` -- o painel novo nasce `second`
    /// (abaixo, para `Horizontal`; à direita, para `Vertical`), herda o
    /// `cwd` que o chamador já resolveu (mesma convenção de
    /// `Workspace::new_tab`) e fica focado. `None` se `id` não existe.
    pub fn split(
        &mut self,
        id: PaneId,
        axis: SplitAxis,
        shell_name: impl Into<String>,
        cwd: Option<PathBuf>,
    ) -> Option<PaneId> {
        self.pane(id)?;
        let new_id = self.fresh_pane_id();
        let mut new_pane = Pane::new(new_id, shell_name);
        if let Some(cwd) = cwd {
            new_pane.set_cwd(cwd);
        }
        self.panes.push(new_pane);
        let old_root = std::mem::replace(&mut self.root, PaneNode::Leaf(id));
        self.root = replace_leaf_with_split(old_root, id, axis, new_id);
        self.focused = new_id;
        Some(new_id)
    }

    /// RF-6.10: fecha `id`, devolvendo o espaço ao irmão. Se `id` era o
    /// focado, o foco vai para a primeira folha (em `leaves_in_order`) do
    /// irmão que absorveu o espaço -- não para qualquer folha da árvore
    /// inteira, que poderia estar do lado oposto de um split não
    /// relacionado. `None` se `id` não existe.
    pub fn close(&mut self, id: PaneId) -> Option<PaneCloseOutcome> {
        self.pane(id)?;
        if matches!(self.root, PaneNode::Leaf(only) if only == id) {
            return Some(PaneCloseOutcome::WasLastPane);
        }
        let new_focus = if self.focused == id {
            sibling_of(&self.root, id).map(first_leaf)
        } else {
            None
        };
        let old_root = std::mem::replace(&mut self.root, PaneNode::Leaf(id));
        self.root = remove_leaf(old_root, id)
            .into_kept()
            .expect("id existe e não é a raiz sozinha, checado acima");
        self.panes.retain(|p| p.id() != id);
        if let Some(focus) = new_focus {
            self.focused = focus;
        }
        Some(PaneCloseOutcome::Closed(self.focused))
    }

    /// RF-6.6/RF-6.7: muda o painel focado. `false` se `id` não existe --
    /// o foco não muda.
    pub fn focus(&mut self, id: PaneId) -> bool {
        if self.pane(id).is_none() {
            return false;
        }
        self.focused = id;
        true
    }

    /// RF-6.8: move o foco para o vizinho geométrico na direção dada.
    /// `false` sem vizinho naquela direção -- o foco não dá a volta.
    pub fn focus_in_direction(&mut self, direction: Direction) -> bool {
        let mut rects = Vec::with_capacity(self.panes.len());
        collect_rects(&self.root, Rect::UNIT, &mut rects);
        let Some(&(_, focused_rect)) = rects.iter().find(|(id, _)| *id == self.focused) else {
            return false;
        };

        const EPS: f32 = 1e-4;
        let target = rects
            .iter()
            .filter(|(id, _)| *id != self.focused)
            .filter_map(|&(id, rect)| {
                let overlaps_vertically =
                    rect.y0 < focused_rect.y1 - EPS && rect.y1 > focused_rect.y0 + EPS;
                let overlaps_horizontally =
                    rect.x0 < focused_rect.x1 - EPS && rect.x1 > focused_rect.x0 + EPS;
                match direction {
                    Direction::Left if overlaps_vertically && rect.x1 <= focused_rect.x0 + EPS => {
                        Some((id, focused_rect.x0 - rect.x1))
                    }
                    Direction::Right if overlaps_vertically && rect.x0 >= focused_rect.x1 - EPS => {
                        Some((id, rect.x0 - focused_rect.x1))
                    }
                    Direction::Up if overlaps_horizontally && rect.y1 <= focused_rect.y0 + EPS => {
                        Some((id, focused_rect.y0 - rect.y1))
                    }
                    Direction::Down
                        if overlaps_horizontally && rect.y0 >= focused_rect.y1 - EPS =>
                    {
                        Some((id, rect.y0 - focused_rect.y1))
                    }
                    _ => None,
                }
            })
            .min_by(|(_, a), (_, b)| a.abs().partial_cmp(&b.abs()).expect("distância é finita"))
            .map(|(id, _)| id);

        match target {
            Some(id) => {
                self.focused = id;
                true
            }
            None => false,
        }
    }

    /// RF-6.13/RF-6.14: escreve o `ratio` do split cujo filho imediato é
    /// `id` -- é o divisor entre `id` e o irmão dele, e faz sentido chamar
    /// com qualquer um dos dois lados. Clampado ao intervalo aberto
    /// (`RATIO_EPSILON`); o mínimo útil de `[panes]` é responsabilidade de
    /// quem chama (`porecatu-ui`, etapa 4), não desta árvore. `false` se
    /// `id` não existe ou é a raiz sozinha (sem divisor para mover).
    pub fn set_ratio(&mut self, id: PaneId, ratio: f32) -> bool {
        set_ratio(
            &mut self.root,
            id,
            ratio.clamp(RATIO_EPSILON, 1.0 - RATIO_EPSILON),
        )
    }

    /// RF-6.13/RF-6.14, endereçado por `path` em vez de painel vizinho --
    /// ver a nota de [`Side`]. `porecatu_ui::panes::dividers` produz o
    /// mesmo `path` para cada vão em tela, então este método alcança
    /// qualquer divisor visível, inclusive o de um layout em cruz. `false`
    /// se `path` não leva a um `Split` (árvore mudou sob o arraste --
    /// painel fechado no meio do gesto, por exemplo).
    pub fn set_ratio_at_path(&mut self, path: &[Side], ratio: f32) -> bool {
        set_ratio_at_path(
            &mut self.root,
            path,
            ratio.clamp(RATIO_EPSILON, 1.0 - RATIO_EPSILON),
        )
    }

    /// O `ratio` do `Split` em `path`, se ele existir -- usado para
    /// resolver o `ratio` inicial ao armar o arraste (`Drag::
    /// DividerPressed`), sem duplicar a travessia de `set_ratio_at_path`.
    pub fn ratio_at_path(&self, path: &[Side]) -> Option<f32> {
        ratio_at_path(&self.root, path)
    }
}

fn fresh_pane_id(counter: &mut u32) -> PaneId {
    let id = PaneId::new(*counter);
    *counter += 1;
    id
}

fn collect_leaves(node: &PaneNode, out: &mut Vec<PaneId>) {
    match node {
        PaneNode::Leaf(id) => out.push(*id),
        PaneNode::Split { first, second, .. } => {
            collect_leaves(first, out);
            collect_leaves(second, out);
        }
    }
}

fn collect_rects(node: &PaneNode, rect: Rect, out: &mut Vec<(PaneId, Rect)>) {
    match node {
        PaneNode::Leaf(id) => out.push((*id, rect)),
        PaneNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let (r1, r2) = rect.split(*axis, *ratio);
            collect_rects(first, r1, out);
            collect_rects(second, r2, out);
        }
    }
}

fn replace_leaf_with_split(
    node: PaneNode,
    target: PaneId,
    axis: SplitAxis,
    new_id: PaneId,
) -> PaneNode {
    match node {
        PaneNode::Leaf(id) if id == target => PaneNode::Split {
            axis,
            ratio: 0.5,
            first: Box::new(PaneNode::Leaf(id)),
            second: Box::new(PaneNode::Leaf(new_id)),
        },
        PaneNode::Leaf(id) => PaneNode::Leaf(id),
        PaneNode::Split {
            axis: a,
            ratio,
            first,
            second,
        } => PaneNode::Split {
            axis: a,
            ratio,
            first: Box::new(replace_leaf_with_split(*first, target, axis, new_id)),
            second: Box::new(replace_leaf_with_split(*second, target, axis, new_id)),
        },
    }
}

/// Devolve o nó irmão de `target` (o outro filho do split que é o pai
/// imediato dele) -- usado por `close` para achar quem herda o foco.
fn sibling_of(node: &PaneNode, target: PaneId) -> Option<&PaneNode> {
    match node {
        PaneNode::Leaf(_) => None,
        PaneNode::Split { first, second, .. } => {
            if matches!(**first, PaneNode::Leaf(id) if id == target) {
                Some(second.as_ref())
            } else if matches!(**second, PaneNode::Leaf(id) if id == target) {
                Some(first.as_ref())
            } else {
                sibling_of(first, target).or_else(|| sibling_of(second, target))
            }
        }
    }
}

fn first_leaf(node: &PaneNode) -> PaneId {
    match node {
        PaneNode::Leaf(id) => *id,
        PaneNode::Split { first, .. } => first_leaf(first),
    }
}

/// Resultado interno de remover uma folha da árvore -- `Removed` bolha até
/// o pai imediato, que vira `Kept` do irmão (o split colapsa); os ancestrais
/// acima só reconstroem o resto da árvore, inalterados na forma.
enum RemoveOutcome {
    Removed,
    Kept(PaneNode),
}

impl RemoveOutcome {
    fn into_kept(self) -> Option<PaneNode> {
        match self {
            Self::Kept(node) => Some(node),
            Self::Removed => None,
        }
    }
}

fn remove_leaf(node: PaneNode, target: PaneId) -> RemoveOutcome {
    match node {
        PaneNode::Leaf(id) if id == target => RemoveOutcome::Removed,
        PaneNode::Leaf(id) => RemoveOutcome::Kept(PaneNode::Leaf(id)),
        PaneNode::Split {
            axis,
            ratio,
            first,
            second,
        } => match remove_leaf(*first, target) {
            RemoveOutcome::Removed => RemoveOutcome::Kept(*second),
            RemoveOutcome::Kept(new_first) => match remove_leaf(*second, target) {
                RemoveOutcome::Removed => RemoveOutcome::Kept(new_first),
                RemoveOutcome::Kept(new_second) => RemoveOutcome::Kept(PaneNode::Split {
                    axis,
                    ratio,
                    first: Box::new(new_first),
                    second: Box::new(new_second),
                }),
            },
        },
    }
}

fn set_ratio_at_path(node: &mut PaneNode, path: &[Side], ratio: f32) -> bool {
    match (node, path.split_first()) {
        (PaneNode::Split { ratio: r, .. }, None) => {
            *r = ratio;
            true
        }
        (PaneNode::Split { first, .. }, Some((Side::First, rest))) => {
            set_ratio_at_path(first, rest, ratio)
        }
        (PaneNode::Split { second, .. }, Some((Side::Second, rest))) => {
            set_ratio_at_path(second, rest, ratio)
        }
        (PaneNode::Leaf(_), _) => false,
    }
}

fn ratio_at_path(node: &PaneNode, path: &[Side]) -> Option<f32> {
    match (node, path.split_first()) {
        (PaneNode::Split { ratio, .. }, None) => Some(*ratio),
        (PaneNode::Split { first, .. }, Some((Side::First, rest))) => ratio_at_path(first, rest),
        (PaneNode::Split { second, .. }, Some((Side::Second, rest))) => ratio_at_path(second, rest),
        (PaneNode::Leaf(_), _) => None,
    }
}

fn set_ratio(node: &mut PaneNode, target: PaneId, ratio: f32) -> bool {
    match node {
        PaneNode::Leaf(_) => false,
        PaneNode::Split {
            ratio: r,
            first,
            second,
            ..
        } => {
            let is_immediate_parent = matches!(**first, PaneNode::Leaf(id) if id == target)
                || matches!(**second, PaneNode::Leaf(id) if id == target);
            if is_immediate_parent {
                *r = ratio;
                true
            } else {
                set_ratio(first, target, ratio) || set_ratio(second, target, ratio)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf_count(tree: &PaneTree) -> usize {
        tree.leaves_in_order().len()
    }

    // -----------------------------------------------------------------
    // Invariantes
    // -----------------------------------------------------------------

    #[test]
    fn new_tree_has_exactly_one_pane_and_it_is_focused() {
        let tree = PaneTree::new("zsh");
        assert_eq!(leaf_count(&tree), 1);
        assert_eq!(tree.panes().len(), 1);
        assert_eq!(tree.focused_id(), tree.leaves_in_order()[0]);
        assert!(matches!(tree.root(), PaneNode::Leaf(_)));
    }

    #[test]
    fn focused_is_always_a_leaf_of_the_tree() {
        let mut tree = PaneTree::new("zsh");
        let a = tree.focused_id();
        let b = tree.split(a, SplitAxis::Vertical, "zsh", None).unwrap();
        let c = tree.split(b, SplitAxis::Horizontal, "zsh", None).unwrap();
        for candidate in [a, b, c] {
            tree.focus(candidate);
            assert!(tree.leaves_in_order().contains(&tree.focused_id()));
        }
    }

    #[test]
    fn ratio_stays_within_the_open_interval() {
        let mut tree = PaneTree::new("zsh");
        let a = tree.focused_id();
        let b = tree.split(a, SplitAxis::Vertical, "zsh", None).unwrap();
        assert!(tree.set_ratio(b, 0.0));
        assert!(tree.set_ratio(b, 1.0));
        assert!(tree.set_ratio(b, -5.0));
        assert!(tree.set_ratio(b, 5.0));
        let PaneNode::Split { ratio, .. } = tree.root() else {
            panic!("split esperado");
        };
        assert!(*ratio > 0.0 && *ratio < 1.0);
    }

    #[test]
    fn closing_the_second_to_last_pane_collapses_the_split() {
        let mut tree = PaneTree::new("zsh");
        let a = tree.focused_id();
        let b = tree.split(a, SplitAxis::Vertical, "zsh", None).unwrap();
        assert_eq!(tree.close(b), Some(PaneCloseOutcome::Closed(a)));
        assert!(matches!(tree.root(), PaneNode::Leaf(id) if *id == a));
        assert_eq!(leaf_count(&tree), 1);
    }

    #[test]
    fn closing_the_last_pane_reports_it_instead_of_touching_the_tree() {
        let mut tree = PaneTree::new("zsh");
        let a = tree.focused_id();
        assert_eq!(tree.close(a), Some(PaneCloseOutcome::WasLastPane));
        assert_eq!(leaf_count(&tree), 1);
    }

    #[test]
    fn leaves_in_order_is_stable_and_covers_every_leaf_without_repeats() {
        let mut tree = PaneTree::new("zsh");
        let a = tree.focused_id();
        let b = tree.split(a, SplitAxis::Horizontal, "zsh", None).unwrap();
        let c = tree.split(a, SplitAxis::Vertical, "zsh", None).unwrap();
        let order = tree.leaves_in_order();
        assert_eq!(order.len(), 3);
        let mut sorted = order.clone();
        sorted.sort_by_key(|id| id.get());
        let mut expected = [a, b, c];
        expected.sort_by_key(|id| id.get());
        assert_eq!(sorted, expected);
        // Estável: chamar de novo produz exatamente a mesma sequência.
        assert_eq!(tree.leaves_in_order(), order);
    }

    // -----------------------------------------------------------------
    // Resultado fixado, não só invariante (lição da F3, CLAUDE.md)
    // -----------------------------------------------------------------

    /// `pane.split_horizontal`: divisor deitado, painel novo **abaixo**.
    /// Testa o resultado geométrico via `focus_in_direction`, não só que a
    /// árvore continua válida.
    #[test]
    fn split_horizontal_places_the_new_pane_below() {
        let mut tree = PaneTree::new("zsh");
        let top = tree.focused_id();
        let bottom = tree.split(top, SplitAxis::Horizontal, "zsh", None).unwrap();
        assert_eq!(tree.focused_id(), bottom);

        tree.focus(top);
        assert!(tree.focus_in_direction(Direction::Down));
        assert_eq!(tree.focused_id(), bottom);
        assert!(!tree.focus_in_direction(Direction::Down), "não dá a volta");

        assert!(tree.focus_in_direction(Direction::Up));
        assert_eq!(tree.focused_id(), top);
    }

    /// `pane.split_vertical`: divisor em pé, painel novo **à direita**.
    #[test]
    fn split_vertical_places_the_new_pane_to_the_right() {
        let mut tree = PaneTree::new("zsh");
        let left = tree.focused_id();
        let right = tree.split(left, SplitAxis::Vertical, "zsh", None).unwrap();
        assert_eq!(tree.focused_id(), right);

        tree.focus(left);
        assert!(tree.focus_in_direction(Direction::Right));
        assert_eq!(tree.focused_id(), right);
        assert!(!tree.focus_in_direction(Direction::Right), "não dá a volta");

        assert!(tree.focus_in_direction(Direction::Left));
        assert_eq!(tree.focused_id(), left);
    }

    /// Cenário de aceite do PRD-006: quatro painéis em cruz, `Alt+Direita`
    /// do superior esquerdo vai para o superior direito, e de novo não
    /// acontece nada (sem volta).
    #[test]
    fn navigating_a_cross_layout_goes_to_the_geometric_neighbor() {
        let mut tree = PaneTree::new("zsh");
        let top_left = tree.focused_id();
        // Divide em pé: [top_left | right_half]
        let right_half = tree
            .split(top_left, SplitAxis::Vertical, "zsh", None)
            .unwrap();
        // Divide o lado direito deitado: right_half vira [top_right / bottom_right]
        let bottom_right = tree
            .split(right_half, SplitAxis::Horizontal, "zsh", None)
            .unwrap();
        let top_right = right_half;
        // Divide o lado esquerdo deitado: top_left vira [top_left / bottom_left]
        let bottom_left = tree
            .split(top_left, SplitAxis::Horizontal, "zsh", None)
            .unwrap();

        tree.focus(top_left);
        assert!(tree.focus_in_direction(Direction::Right));
        assert_eq!(tree.focused_id(), top_right);
        assert!(
            !tree.focus_in_direction(Direction::Right),
            "sem vizinho mais à direita"
        );

        tree.focus(top_left);
        assert!(tree.focus_in_direction(Direction::Down));
        assert_eq!(tree.focused_id(), bottom_left);

        tree.focus(top_right);
        assert!(tree.focus_in_direction(Direction::Down));
        assert_eq!(tree.focused_id(), bottom_right);

        tree.focus(bottom_left);
        assert!(tree.focus_in_direction(Direction::Right));
        assert_eq!(tree.focused_id(), bottom_right);
    }

    #[test]
    fn focus_in_direction_without_a_neighbor_does_nothing() {
        let mut tree = PaneTree::new("zsh");
        assert!(!tree.focus_in_direction(Direction::Left));
        assert!(!tree.focus_in_direction(Direction::Right));
        assert!(!tree.focus_in_direction(Direction::Up));
        assert!(!tree.focus_in_direction(Direction::Down));
    }

    /// "Repetir o gesto" do PRD-006: dividir de novo o painel da direita
    /// não muda o tamanho do painel da esquerda -- o split alvo é sempre o
    /// painel indicado, nunca a árvore inteira.
    #[test]
    fn splitting_again_does_not_disturb_the_sibling() {
        let mut tree = PaneTree::new("zsh");
        let left = tree.focused_id();
        let right = tree.split(left, SplitAxis::Vertical, "zsh", None).unwrap();
        tree.set_ratio(right, 0.3);
        let PaneNode::Split {
            ratio: outer_ratio, ..
        } = tree.root()
        else {
            panic!("split esperado");
        };
        let outer_ratio = *outer_ratio;

        tree.split(right, SplitAxis::Horizontal, "zsh", None);

        let PaneNode::Split { ratio, .. } = tree.root() else {
            panic!("split esperado");
        };
        assert_eq!(*ratio, outer_ratio, "painel da esquerda não deve mudar");
        assert_eq!(leaf_count(&tree), 3);
    }

    // -----------------------------------------------------------------
    // Herança de cwd (RF-6.3)
    // -----------------------------------------------------------------

    #[test]
    fn split_inherits_the_cwd_the_caller_resolved() {
        let mut tree = PaneTree::new("zsh");
        let a = tree.focused_id();
        let cwd = PathBuf::from("/home/user/projeto");
        let b = tree
            .split(a, SplitAxis::Vertical, "zsh", Some(cwd.clone()))
            .unwrap();
        assert_eq!(tree.pane(b).unwrap().cwd(), Some(&cwd));
    }

    // -----------------------------------------------------------------
    // Ciclo de vida do painel
    // -----------------------------------------------------------------

    #[test]
    fn new_pane_is_always_running() {
        let pane = Pane::new(PaneId::new(0), "zsh");
        assert_eq!(pane.state(), PaneState::Running);
    }

    #[test]
    fn not_started_pane_rejects_input_until_started() {
        let mut pane = Pane::new_not_started(PaneId::new(0), "zsh");
        assert!(!pane.accepts_input());
        pane.start();
        assert!(pane.accepts_input());
    }

    #[test]
    fn exiting_clears_indicators() {
        let mut pane = Pane::new(PaneId::new(0), "zsh");
        pane.mark_activity();
        pane.mark_bell();
        pane.mark_exited(0);
        assert!(!pane.activity());
        assert!(!pane.bell());
        assert!(pane.is_exited());
    }

    // -----------------------------------------------------------------
    // Divisor endereçado por caminho (RF-6.13/RF-6.14)
    // -----------------------------------------------------------------

    /// O caso que `set_ratio(id, ..)` não alcança: layout em cruz, os dois
    /// filhos imediatos da raiz já são `Split` -- nenhuma folha é filha
    /// direta dela. `set_ratio_at_path([], ..)` move o divisor externo
    /// mesmo assim.
    #[test]
    fn set_ratio_at_path_reaches_a_split_whose_children_are_both_splits() {
        let mut tree = PaneTree::new("zsh");
        let top_left = tree.focused_id();
        let right_half = tree
            .split(top_left, SplitAxis::Vertical, "zsh", None)
            .unwrap();
        tree.split(right_half, SplitAxis::Horizontal, "zsh", None);
        tree.split(top_left, SplitAxis::Horizontal, "zsh", None);
        let PaneNode::Split { first, second, .. } = tree.root() else {
            panic!("split esperado");
        };
        assert!(
            matches!(**first, PaneNode::Split { .. }) && matches!(**second, PaneNode::Split { .. }),
            "pré-condição do teste: os dois filhos da raiz já são splits"
        );

        assert_eq!(tree.ratio_at_path(&[]), Some(0.5));
        assert!(tree.set_ratio_at_path(&[], 0.3));
        assert_eq!(tree.ratio_at_path(&[]), Some(0.3));

        // O divisor interno (`First`) continua endereçável e independente.
        assert!(tree.set_ratio_at_path(&[Side::First], 0.7));
        assert_eq!(tree.ratio_at_path(&[Side::First]), Some(0.7));
        assert_eq!(tree.ratio_at_path(&[]), Some(0.3), "não deve se mexer");
    }

    #[test]
    fn set_ratio_at_path_clamps_to_the_open_interval() {
        let mut tree = PaneTree::new("zsh");
        let a = tree.focused_id();
        tree.split(a, SplitAxis::Vertical, "zsh", None);
        assert!(tree.set_ratio_at_path(&[], -5.0));
        let ratio = tree.ratio_at_path(&[]).unwrap();
        assert!(ratio > 0.0 && ratio < 1.0);
    }

    #[test]
    fn set_ratio_at_path_is_false_for_a_path_that_does_not_lead_to_a_split() {
        let mut tree = PaneTree::new("zsh");
        assert!(!tree.set_ratio_at_path(&[Side::First], 0.5), "raiz é folha");
        assert!(tree.ratio_at_path(&[Side::First]).is_none());
    }

    #[test]
    fn title_falls_back_to_shell_name() {
        let mut pane = Pane::new(PaneId::new(0), "zsh");
        assert_eq!(pane.title(), "zsh");
        pane.set_process_title(Some("vim: main.rs".to_string()));
        assert_eq!(pane.title(), "vim: main.rs");
    }
}
