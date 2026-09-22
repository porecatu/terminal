// SPDX-License-Identifier: GPL-3.0-or-later

//! Layout puro dos painéis de uma aba (ADR-0053 §5, a segunda função pura
//! da seção 7 de `docs/arquitetura.md`):
//!
//! ```text
//! (árvore, retângulo do quadro, style) -> Vec<(PaneId, Rect)>
//! ```
//!
//! `paint::terminal_box_rect`/`terminal_content_rect` continuam sendo a
//! fonte única do retângulo externo -- quem já sabe descontar a barra de
//! abas, a barra de status e as margens continua sabendo. Esta função só
//! **subdivide** o que elas devolvem, recursivamente, tirando o vão a cada
//! nó `Split`.
//!
//! O vão é `style.terminal_frame_margin` -- os mesmos 6px que já separam a
//! janela do terminal (ADR-0053 §3). **Nenhum valor novo.** Ele sai da
//! área útil antes do cálculo de linhas e colunas, como toda mobília
//! (espec. visual §2.7.1): o que cada painel recebe já é o retângulo dele
//! sem o vão, e é dele que saem as colunas e as linhas daquele PTY.

use porecatu_core::{PaneId, PaneNode, PaneTree, Side, SplitAxis};
use porecatu_render::Rect;

use crate::tab_bar::TabBarStyle;

/// Retângulo de cada painel, na ordem estável de `PaneTree::leaves_in_order`
/// (`first` antes de `second`, recursivamente).
pub fn layout(tree: &PaneTree, rect: Rect, style: &TabBarStyle) -> Vec<(PaneId, Rect)> {
    let mut out = Vec::with_capacity(tree.panes().len());
    collect(tree.root(), rect, style.terminal_frame_margin, &mut out);
    out
}

/// Painel sob `point` (etapa 4, hit-test do mouse) -- `None` quando o
/// ponto cai no vão entre dois painéis ou fora do retângulo inteiro. Os
/// retângulos de [`layout`] já excluem o vão (`split_rect` tira o `gap` da
/// área útil antes de dividir), então nunca há ambiguidade: o mesmo ponto
/// cai em no máximo um painel.
pub fn pane_at(panes: &[(PaneId, Rect)], point: (f32, f32)) -> Option<PaneId> {
    panes
        .iter()
        .find(|(_, rect)| point_in_rect(point, *rect))
        .map(|(id, _)| *id)
}

fn point_in_rect(point: (f32, f32), r: Rect) -> bool {
    point.0 >= r.x && point.0 < r.x + r.width && point.1 >= r.y && point.1 < r.y + r.height
}

/// Um divisor visível: o vão entre `first` e `second` de um `Split`,
/// endereçado por [`Side`] a partir da raiz (ADR-0053 §7/RF-6.13) --
/// mesmo caminho que `PaneTree::set_ratio_at_path`/`ratio_at_path`
/// esperam, para o arraste nunca discordar de qual divisor está sob o
/// cursor. `rect` é a faixa sensível ao mouse: o próprio vão, sem
/// crescer além dele (mesma medida da faixa de resize da janela,
/// ADR-0053 §7, coincidência de valor, não de origem). `container` é o
/// retângulo do `Split` inteiro (antes de tirar o vão) -- base para
/// converter a posição do cursor de volta em `ratio` durante o arraste.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneDivider {
    pub axis: SplitAxis,
    pub rect: Rect,
    pub container: Rect,
    pub path: [Side; MAX_DEPTH],
    pub depth: usize,
}

/// Profundidade máxima de aninhamento que um `path` pode guardar sem
/// alocar -- RF-6.2 não limita o aninhamento, mas 16 divisores no mesmo
/// ramo (65536 painéis nessa perna da árvore) já não cabe em nenhuma
/// janela real; um `Vec` por divisor a cada frame seria alocação no
/// caminho de pintura/hover para um caso que não acontece na prática.
pub const MAX_DEPTH: usize = 16;

/// Todos os divisores visíveis, na mesma travessia de [`layout`] -- usado
/// pelo hit-test do cursor/clique (precedência contra a borda de resize da
/// janela, RF-6.16) e pelo arraste (RF-6.13/RF-6.14).
pub fn dividers(tree: &PaneTree, rect: Rect, style: &TabBarStyle) -> Vec<PaneDivider> {
    let mut out = Vec::new();
    let mut path = [Side::First; MAX_DEPTH];
    collect_dividers(
        tree.root(),
        rect,
        style.terminal_frame_margin,
        &mut path,
        0,
        &mut out,
    );
    out
}

/// Divisor sob `point`, se algum -- primeiro que casa, sem prioridade
/// declarada entre divisores porque dois deles nunca se sobrepõem (cada
/// vão pertence a um único `Split`, e a subdivisão é estritamente
/// recursiva).
pub fn divider_at(dividers: &[PaneDivider], point: (f32, f32)) -> Option<&PaneDivider> {
    dividers.iter().find(|d| point_in_rect(point, d.rect))
}

fn collect_dividers(
    node: &PaneNode,
    rect: Rect,
    gap: f32,
    path: &mut [Side; MAX_DEPTH],
    depth: usize,
    out: &mut Vec<PaneDivider>,
) {
    let PaneNode::Split {
        axis,
        ratio,
        first,
        second,
    } = node
    else {
        return;
    };
    let (first_rect, second_rect) = split_rect(rect, *axis, *ratio, gap);
    let divider_rect = match axis {
        SplitAxis::Horizontal => Rect {
            x: rect.x,
            y: first_rect.y + first_rect.height,
            width: rect.width,
            height: gap,
        },
        SplitAxis::Vertical => Rect {
            x: first_rect.x + first_rect.width,
            y: rect.y,
            width: gap,
            height: rect.height,
        },
    };
    if depth < MAX_DEPTH {
        out.push(PaneDivider {
            axis: *axis,
            rect: divider_rect,
            container: rect,
            path: *path,
            depth,
        });
        path[depth] = Side::First;
        collect_dividers(first, first_rect, gap, path, depth + 1, out);
        path[depth] = Side::Second;
        collect_dividers(second, second_rect, gap, path, depth + 1, out);
    }
}

fn collect(node: &PaneNode, rect: Rect, gap: f32, out: &mut Vec<(PaneId, Rect)>) {
    match node {
        PaneNode::Leaf(id) => out.push((*id, rect)),
        PaneNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let (first_rect, second_rect) = split_rect(rect, *axis, *ratio, gap);
            collect(first, first_rect, gap, out);
            collect(second, second_rect, gap, out);
        }
    }
}

/// `SplitAxis::Horizontal` (divisor deitado, painéis empilhados) tira o
/// vão do eixo vertical; `SplitAxis::Vertical` (divisor em pé, lado a
/// lado) tira do eixo horizontal -- mesma convenção de nomes de
/// `porecatu_core::pane`.
fn split_rect(rect: Rect, axis: SplitAxis, ratio: f32, gap: f32) -> (Rect, Rect) {
    match axis {
        SplitAxis::Horizontal => {
            let available = (rect.height - gap).max(0.0);
            let first_height = available * ratio;
            let second_height = available - first_height;
            (
                Rect {
                    height: first_height,
                    ..rect
                },
                Rect {
                    y: rect.y + first_height + gap,
                    height: second_height,
                    ..rect
                },
            )
        }
        SplitAxis::Vertical => {
            let available = (rect.width - gap).max(0.0);
            let first_width = available * ratio;
            let second_width = available - first_width;
            (
                Rect {
                    width: first_width,
                    ..rect
                },
                Rect {
                    x: rect.x + first_width + gap,
                    width: second_width,
                    ..rect
                },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> TabBarStyle {
        TabBarStyle::from_config(&porecatu_config::Config::default())
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_single_pane_gets_the_whole_rect_untouched() {
        let tree = PaneTree::new("zsh");
        let outer = rect(6.0, 52.0, 800.0, 500.0);
        let panes = layout(&tree, outer, &style());
        assert_eq!(panes, vec![(tree.focused_id(), outer)]);
    }

    /// ADR-0053 §3/§5: a soma dos painéis mais os vãos é o retângulo
    /// original -- o vão não é somado a mais, é tirado da área útil.
    #[test]
    fn horizontal_split_sums_back_to_the_original_rect_with_the_gap_between() {
        let mut tree = PaneTree::new("zsh");
        let top = tree.focused_id();
        tree.split(top, SplitAxis::Horizontal, "zsh", None);
        let outer = rect(6.0, 52.0, 800.0, 500.0);
        let gap = style().terminal_frame_margin;

        let panes = layout(&tree, outer, &style());
        assert_eq!(panes.len(), 2);
        let (_, top_rect) = panes[0];
        let (_, bottom_rect) = panes[1];

        assert_eq!(top_rect.x, outer.x);
        assert_eq!(bottom_rect.x, outer.x);
        assert_eq!(top_rect.width, outer.width);
        assert_eq!(bottom_rect.width, outer.width);
        assert_eq!(top_rect.y, outer.y);
        assert_eq!(bottom_rect.y, top_rect.y + top_rect.height + gap);
        assert_eq!(
            bottom_rect.y + bottom_rect.height,
            outer.y + outer.height,
            "o fundo do painel de baixo bate com o fundo do retângulo original"
        );
        assert_eq!(
            top_rect.height + gap + bottom_rect.height,
            outer.height,
            "painéis mais o vão somam de volta a altura original"
        );
    }

    #[test]
    fn vertical_split_sums_back_to_the_original_rect_with_the_gap_between() {
        let mut tree = PaneTree::new("zsh");
        let left = tree.focused_id();
        tree.split(left, SplitAxis::Vertical, "zsh", None);
        let outer = rect(6.0, 52.0, 800.0, 500.0);
        let gap = style().terminal_frame_margin;

        let panes = layout(&tree, outer, &style());
        let (_, left_rect) = panes[0];
        let (_, right_rect) = panes[1];

        assert_eq!(left_rect.y, outer.y);
        assert_eq!(right_rect.y, outer.y);
        assert_eq!(left_rect.height, outer.height);
        assert_eq!(right_rect.height, outer.height);
        assert_eq!(right_rect.x, left_rect.x + left_rect.width + gap);
        assert_eq!(
            left_rect.width + gap + right_rect.width,
            outer.width,
            "painéis mais o vão somam de volta a largura original"
        );
    }

    /// Base do hit-test da etapa 4: o mesmo ponto lógico não pode cair em
    /// dois painéis -- os retângulos nunca se sobrepõem, mesmo com uma
    /// árvore de três folhas (dois splits aninhados).
    #[test]
    fn the_same_point_never_falls_in_two_panes() {
        let mut tree = PaneTree::new("zsh");
        let left = tree.focused_id();
        let right = tree.split(left, SplitAxis::Vertical, "zsh", None).unwrap();
        tree.split(right, SplitAxis::Horizontal, "zsh", None);
        let outer = rect(0.0, 0.0, 900.0, 600.0);

        let panes = layout(&tree, outer, &style());
        assert_eq!(panes.len(), 3);

        // Amostra o centro de cada painel: tem de cair em exatamente um.
        for &(id, r) in &panes {
            let point = (r.x + r.width / 2.0, r.y + r.height / 2.0);
            let containing: Vec<PaneId> = panes
                .iter()
                .filter(|&&(_, other)| point_in_rect(point, other))
                .map(|&(pid, _)| pid)
                .collect();
            assert_eq!(
                containing,
                vec![id],
                "o centro do painel {id:?} caiu em {} painéis",
                containing.len()
            );
        }
    }

    fn point_in_rect(point: (f32, f32), r: Rect) -> bool {
        point.0 >= r.x && point.0 < r.x + r.width && point.1 >= r.y && point.1 < r.y + r.height
    }

    #[test]
    fn pane_at_finds_the_containing_pane_and_none_in_the_gap() {
        let mut tree = PaneTree::new("zsh");
        let left = tree.focused_id();
        let right = tree.split(left, SplitAxis::Vertical, "zsh", None).unwrap();
        let outer = rect(0.0, 0.0, 900.0, 600.0);
        let gap = style().terminal_frame_margin;
        let panes = layout(&tree, outer, &style());

        assert_eq!(pane_at(&panes, (10.0, 10.0)), Some(left));
        assert_eq!(pane_at(&panes, (890.0, 10.0)), Some(right));
        let (_, left_rect) = panes.iter().find(|(id, _)| *id == left).unwrap();
        assert_eq!(
            pane_at(&panes, (left_rect.x + left_rect.width + gap / 2.0, 10.0)),
            None,
            "o meio do vão não pertence a nenhum painel"
        );
        assert_eq!(pane_at(&panes, (-5.0, -5.0)), None);
    }

    /// O divisor de um split de dois painéis é endereçável pelo `path`
    /// vazio, e move o mesmo `ratio` que `PaneTree::ratio_at_path` lê --
    /// base de que o arraste (etapa 4) escreve o `Split` certo.
    #[test]
    fn a_two_pane_split_has_one_divider_addressed_by_the_empty_path() {
        let mut tree = PaneTree::new("zsh");
        let left = tree.focused_id();
        tree.split(left, SplitAxis::Vertical, "zsh", None);
        let outer = rect(0.0, 0.0, 900.0, 600.0);
        let ds = dividers(&tree, outer, &style());

        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].depth, 0);
        assert_eq!(ds[0].axis, SplitAxis::Vertical);
        assert!(tree.ratio_at_path(&ds[0].path[..ds[0].depth]).is_some());
    }

    /// Layout em cruz (PRD-006, cenário de navegação): três divisores, e o
    /// externo -- entre a coluna esquerda e a direita -- é exatamente o
    /// caso que `PaneTree::set_ratio` (por painel vizinho) não alcança,
    /// porque os dois filhos imediatos da raiz já são splits.
    #[test]
    fn a_cross_layout_has_three_dividers_and_the_outer_one_has_an_empty_path() {
        let mut tree = PaneTree::new("zsh");
        let top_left = tree.focused_id();
        let right_half = tree
            .split(top_left, SplitAxis::Vertical, "zsh", None)
            .unwrap();
        tree.split(right_half, SplitAxis::Horizontal, "zsh", None);
        tree.split(top_left, SplitAxis::Horizontal, "zsh", None);
        let outer = rect(0.0, 0.0, 900.0, 600.0);
        let ds = dividers(&tree, outer, &style());

        assert_eq!(ds.len(), 3);
        let outer_divider = ds.iter().find(|d| d.depth == 0);
        assert!(
            outer_divider.is_some(),
            "o divisor externo precisa existir mesmo sem folha filha imediata"
        );
        assert_eq!(outer_divider.unwrap().axis, SplitAxis::Vertical);

        // Escrever o ratio de cada divisor por `path` reflete de volta em
        // `ratio_at_path` -- prova que `dividers` e `PaneTree` concordam
        // sobre o que cada `path` significa.
        for d in &ds {
            let path = &d.path[..d.depth];
            assert!(tree.set_ratio_at_path(path, 0.6));
            assert_eq!(tree.ratio_at_path(path), Some(0.6));
        }
    }

    /// Nenhum divisor se sobrepõe a nenhum painel -- a soma de painéis mais
    /// vãos já prova isso por construção (`horizontal_split_sums_back...`),
    /// mas aqui é a checagem direta ponto a ponto que `dispatch_cursor_moved`
    /// depende: o mesmo ponto nunca é "painel" e "divisor" ao mesmo tempo.
    #[test]
    fn no_divider_rect_overlaps_a_pane_rect() {
        let mut tree = PaneTree::new("zsh");
        let top_left = tree.focused_id();
        let right_half = tree
            .split(top_left, SplitAxis::Vertical, "zsh", None)
            .unwrap();
        tree.split(right_half, SplitAxis::Horizontal, "zsh", None);
        tree.split(top_left, SplitAxis::Horizontal, "zsh", None);
        let outer = rect(0.0, 0.0, 900.0, 600.0);
        let panes = layout(&tree, outer, &style());
        let ds = dividers(&tree, outer, &style());

        for d in &ds {
            let center = (
                d.rect.x + d.rect.width / 2.0,
                d.rect.y + d.rect.height / 2.0,
            );
            assert_eq!(pane_at(&panes, center), None);
        }
    }

    /// Repetir o gesto (PRD-006, cenário de aceite): dividir de novo o
    /// painel da direita não muda o retângulo do painel da esquerda.
    #[test]
    fn splitting_again_does_not_move_the_sibling_rect() {
        let mut tree = PaneTree::new("zsh");
        let left = tree.focused_id();
        let right = tree.split(left, SplitAxis::Vertical, "zsh", None).unwrap();
        let outer = rect(0.0, 0.0, 900.0, 600.0);

        let before = layout(&tree, outer, &style());
        let left_rect_before = before.iter().find(|(id, _)| *id == left).unwrap().1;

        tree.split(right, SplitAxis::Horizontal, "zsh", None);
        let after = layout(&tree, outer, &style());
        let left_rect_after = after.iter().find(|(id, _)| *id == left).unwrap().1;

        assert_eq!(left_rect_before, left_rect_after);
    }

    /// RF-6.16/ADR-0053 §7 (riscos): "as duas precedências mudam juntas" --
    /// geometria provada, não raciocínio confiado, mesmo padrão do
    /// `the_indicator_never_reaches_a_diagonal_resize_corner` do indicador
    /// de commits (`status_bar.rs`). Um divisor nunca nasce perto o
    /// bastante da borda da janela para os quatro cantos do seu retângulo
    /// caírem numa direção de resize diagonal.
    #[test]
    fn no_divider_corner_ever_reaches_a_diagonal_resize_corner() {
        let mut tree = PaneTree::new("zsh");
        let top_left = tree.focused_id();
        let right_half = tree
            .split(top_left, SplitAxis::Vertical, "zsh", None)
            .unwrap();
        tree.split(right_half, SplitAxis::Horizontal, "zsh", None);
        tree.split(top_left, SplitAxis::Horizontal, "zsh", None);

        let style = style();
        let window_width = 900.0;
        let window_height = 600.0;
        // Mesma janela que `terminal_box_rect` já desconta a barra de abas
        // e a margem -- não o retângulo cru da janela inteira.
        let outer = rect(
            style.terminal_frame_margin,
            52.0,
            window_width - style.terminal_frame_margin * 2.0,
            window_height - 52.0 - style.terminal_frame_margin,
        );
        let ds = dividers(&tree, outer, &style);
        assert_eq!(ds.len(), 3);

        let border = porecatu_config::Config::default()
            .appearance
            .window_controls
            .resize_border as f32;
        for d in &ds {
            let corners = [
                (d.rect.x, d.rect.y),
                (d.rect.x + d.rect.width, d.rect.y),
                (d.rect.x, d.rect.y + d.rect.height),
                (d.rect.x + d.rect.width, d.rect.y + d.rect.height),
            ];
            for point in corners {
                let direction = crate::titlebar::resize_direction_at(
                    point,
                    window_width,
                    window_height,
                    false,
                    border,
                );
                assert!(
                    !matches!(
                        direction,
                        Some(
                            winit::window::ResizeDirection::NorthWest
                                | winit::window::ResizeDirection::NorthEast
                                | winit::window::ResizeDirection::SouthWest
                                | winit::window::ResizeDirection::SouthEast
                        )
                    ),
                    "canto do divisor {point:?} caiu num canto diagonal de resize"
                );
            }
        }
    }
}
