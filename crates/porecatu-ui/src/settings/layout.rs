// SPDX-License-Identifier: GPL-3.0-or-later

//! Layout da janela de configurações (ADR-0060 §1): as faixas em que ela se
//! divide, como função pura de tamanho. Consumido pela pintura e pelo
//! hit-test -- e, a partir da tarefa de acessibilidade, pela árvore
//! (ADR-0059 §4) -- para os três nunca discordarem de onde cada faixa está.
//!
//! Existem aqui as faixas, os itens da guia (um por grupo) e a ordem dos
//! botões do rodapé. As linhas de opção do painel entram com a tarefa que as
//! desenha.

use porecatu_render::Rect;

use super::catalog::Group;

/// As faixas da janela, em coordenadas lógicas de janela.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Layout {
    /// Faixa do cabeçalho, de ponta a ponta. `None` onde a decoração é
    /// nativa (macOS) e o título mora na barra do sistema.
    pub header: Option<Rect>,
    /// Guia lateral, à esquerda, abaixo do cabeçalho. Inclui o separador de
    /// 1px da borda direita.
    pub sidebar: Rect,
    /// Separador de 1px entre a guia e o painel -- a borda direita da guia.
    pub sidebar_separator: Rect,
    /// Painel (opções e rodapé), ocupando o resto.
    pub panel: Rect,
    /// A parte do painel que rola: o painel menos o rodapé.
    pub panel_body: Rect,
    /// Rodapé fixo na base do painel (ADR-0060 §1).
    pub footer: Rect,
}

/// Altura do rodapé: o `padding: 12px 18px` dele (ADR-0060 §1) em volta de um
/// botão de 30 (ADR-0060 §4, anatomia do botão do diálogo).
const FOOTER_PADDING_Y: f32 = 12.0;
const FOOTER_BUTTON_HEIGHT: f32 = 30.0;
pub(crate) const FOOTER_HEIGHT: f32 = FOOTER_PADDING_Y * 2.0 + FOOTER_BUTTON_HEIGHT;

/// Espaço entre itens da guia (ADR-0060 §2: `gap: 2`).
pub(crate) const SIDEBAR_ITEM_GAP: f32 = 2.0;

/// Os botões do rodapé, da esquerda para a direita (RF-16.14): é a ordem de
/// leitura e de `Tab`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FooterButton {
    OpenFile,
    Discard,
    Save,
}

pub(crate) const FOOTER_BUTTONS: [FooterButton; 3] = [
    FooterButton::OpenFile,
    FooterButton::Discard,
    FooterButton::Save,
];

/// Largura do separador entre a guia e o painel (ADR-0060 §1: "1px").
const SEPARATOR_WIDTH: f32 = 1.0;

/// Divide uma janela de `width` × `height` em cabeçalho, guia e painel.
/// `header_height` é zero quando não há cabeçalho nosso. `sidebar_width` é
/// grampeada à largura da janela: uma janela mais estreita que a guia (o
/// mínimo é maior que ela, mas o SO pode entregar menos) não produz retângulo
/// de largura negativa.
pub(crate) fn layout(width: f32, height: f32, header_height: f32, sidebar_width: f32) -> Layout {
    let header_height = header_height.min(height).max(0.0);
    let sidebar_width = sidebar_width.min(width).max(0.0);
    let body_height = height - header_height;
    let header = (header_height > 0.0).then_some(Rect {
        x: 0.0,
        y: 0.0,
        width,
        height: header_height,
    });
    let sidebar = Rect {
        x: 0.0,
        y: header_height,
        width: sidebar_width,
        height: body_height,
    };
    let separator_width = SEPARATOR_WIDTH.min(sidebar_width);
    let sidebar_separator = Rect {
        x: sidebar_width - separator_width,
        y: header_height,
        width: separator_width,
        height: body_height,
    };
    let panel = Rect {
        x: sidebar_width,
        y: header_height,
        width: width - sidebar_width,
        height: body_height,
    };
    let footer_height = FOOTER_HEIGHT.min(body_height);
    let panel_body = Rect {
        height: body_height - footer_height,
        ..panel
    };
    let footer = Rect {
        y: panel.y + panel_body.height,
        height: footer_height,
        ..panel
    };
    Layout {
        header,
        sidebar,
        sidebar_separator,
        panel,
        panel_body,
        footer,
    }
}

/// Os itens da guia: um retângulo por grupo, de cima para baixo, dentro do
/// `padding` da guia (ADR-0060 §1: 6) e com `SIDEBAR_ITEM_GAP` entre eles.
/// `item_height` é a do item de menu (ADR-0060 §2: a guia é uma lista de itens
/// de menu). O retângulo do último pode passar da base numa janela baixa --
/// a guia não rola (RF-16.7), o mínimo de altura da janela garante que cabe.
pub(crate) fn group_items(sidebar: Rect, padding: f32, item_height: f32) -> Vec<(Group, Rect)> {
    Group::ALL
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let rect = Rect {
                x: sidebar.x + padding,
                y: sidebar.y + padding + index as f32 * (item_height + SIDEBAR_ITEM_GAP),
                width: (sidebar.width - padding * 2.0 - 1.0).max(0.0),
                height: item_height,
            };
            (*group, rect)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_the_window_into_header_sidebar_and_panel() {
        let l = layout(900.0, 640.0, 52.0, 200.0);
        let header = l.header.unwrap();
        assert_eq!(
            (header.x, header.y, header.width, header.height),
            (0.0, 0.0, 900.0, 52.0)
        );
        assert_eq!((l.sidebar.x, l.sidebar.y), (0.0, 52.0));
        assert_eq!((l.sidebar.width, l.sidebar.height), (200.0, 588.0));
        assert_eq!((l.panel.x, l.panel.y), (200.0, 52.0));
        assert_eq!((l.panel.width, l.panel.height), (700.0, 588.0));
    }

    #[test]
    fn without_a_header_the_body_takes_the_whole_height() {
        let l = layout(900.0, 640.0, 0.0, 200.0);
        assert!(l.header.is_none());
        assert_eq!((l.sidebar.y, l.sidebar.height), (0.0, 640.0));
        assert_eq!((l.panel.y, l.panel.height), (0.0, 640.0));
    }

    #[test]
    fn the_separator_is_the_right_edge_of_the_sidebar() {
        let l = layout(900.0, 640.0, 52.0, 200.0);
        assert_eq!(l.sidebar_separator.x + l.sidebar_separator.width, 200.0);
        assert_eq!(l.sidebar_separator.width, 1.0);
        assert_eq!(l.sidebar_separator.height, l.sidebar.height);
    }

    #[test]
    fn panel_and_sidebar_tile_the_width_exactly() {
        for width in [640.0, 777.5, 900.0, 1920.0] {
            let l = layout(width, 500.0, 52.0, 200.0);
            assert_eq!(l.sidebar.width + l.panel.width, width);
        }
    }

    #[test]
    fn the_footer_is_fixed_at_the_base_of_the_panel() {
        let l = layout(900.0, 640.0, 52.0, 200.0);
        assert_eq!(l.footer.height, 54.0);
        assert_eq!(l.footer.y + l.footer.height, 640.0);
        assert_eq!(l.panel_body.y + l.panel_body.height, l.footer.y);
        assert_eq!(l.footer.x, l.panel.x);
        assert_eq!(l.footer.width, l.panel.width);
    }

    #[test]
    fn a_window_shorter_than_the_footer_never_goes_negative() {
        let l = layout(900.0, 60.0, 52.0, 200.0);
        assert!(l.panel_body.height >= 0.0);
        assert!(l.footer.height <= 8.0);
    }

    #[test]
    fn the_sidebar_lists_the_nine_groups_in_order_with_a_gap() {
        let l = layout(900.0, 640.0, 52.0, 200.0);
        let items = group_items(l.sidebar, 6.0, 28.0);
        let groups: Vec<Group> = items.iter().map(|(group, _)| *group).collect();
        assert_eq!(groups, Group::ALL);
        assert_eq!(items[0].1.y, 52.0 + 6.0);
        assert_eq!(items[1].1.y - items[0].1.y, 28.0 + 2.0);
        assert_eq!(items[0].1.x, 6.0);
        for (_, rect) in &items {
            assert!(rect.x + rect.width <= l.sidebar_separator.x);
        }
        // Os nove cabem na altura mínima da janela (420) com folga.
        let last = items.last().unwrap().1;
        assert!(last.y + last.height <= super::super::MIN_HEIGHT);
    }

    #[test]
    fn a_window_narrower_than_the_sidebar_never_goes_negative() {
        let l = layout(120.0, 30.0, 52.0, 200.0);
        assert!(l.sidebar.width >= 0.0 && l.panel.width >= 0.0);
        assert!(l.sidebar.height >= 0.0 && l.panel.height >= 0.0);
        assert!(l.header.unwrap().height <= 30.0);
    }
}
