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

use porecatu_core::{PaneId, PaneNode, PaneTree, SplitAxis};
use porecatu_render::Rect;

use crate::tab_bar::TabBarStyle;

/// Retângulo de cada painel, na ordem estável de `PaneTree::leaves_in_order`
/// (`first` antes de `second`, recursivamente).
pub fn layout(tree: &PaneTree, rect: Rect, style: &TabBarStyle) -> Vec<(PaneId, Rect)> {
    let mut out = Vec::with_capacity(tree.panes().len());
    collect(tree.root(), rect, style.terminal_frame_margin, &mut out);
    out
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
}
