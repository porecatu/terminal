// SPDX-License-Identifier: GPL-3.0-or-later

//! Layout da janela de configurações (ADR-0060 §1): as faixas em que ela se
//! divide, como função pura de tamanho. Consumido pela pintura e pelo
//! hit-test -- e, a partir da tarefa de acessibilidade, pela árvore
//! (ADR-0059 §4) -- para os três nunca discordarem de onde cada faixa está.
//!
//! Só as faixas existem aqui. Itens da guia, linhas do painel e rodapé entram
//! com as tarefas que os desenham.

use porecatu_render::Rect;

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
    /// Painel, ocupando o resto.
    pub panel: Rect,
}

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
    Layout {
        header,
        sidebar,
        sidebar_separator,
        panel,
    }
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
    fn a_window_narrower_than_the_sidebar_never_goes_negative() {
        let l = layout(120.0, 30.0, 52.0, 200.0);
        assert!(l.sidebar.width >= 0.0 && l.panel.width >= 0.0);
        assert!(l.sidebar.height >= 0.0 && l.panel.height >= 0.0);
        assert!(l.header.unwrap().height <= 30.0);
    }
}
