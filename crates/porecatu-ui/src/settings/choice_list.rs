// SPDX-License-Identifier: GPL-3.0-or-later

//! A lista que o botão de escolha abre (ADR-0060 §3): "escolha com mais
//! opções" -- hoje só o idioma -- abre a lista no menu de contexto (§2.16)
//! ancorada sob o botão, com o item escolhido no realce `#242a33`. Mesma
//! anatomia e mesmas chaves de `[appearance.context_menu]` do menu de aba:
//! largura mínima, `padding`, altura de item, raios, sombra. Camada
//! `Popover` da janela (ADR-0060 §4).
//!
//! Estado puro mais layout, pintura e hit-test como funções de dados já
//! prontos: o texto dos itens é cortado **uma vez**, na abertura, e nada aqui
//! mede texto por quadro.

use porecatu_config::Config;
use porecatu_render::{Primitive, Rect, RoundedQuad, TextMeasurer, TextRun};

use crate::chrome::push_shadow;
use crate::overlay::BODY_FONT;
use crate::palette::{self, ResolvedPalette};
use crate::tab_bar::rect_contains;

/// Um item da lista: o valor que a opção grava e o texto que a lista mostra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChoiceItem {
    pub value: String,
    pub label: String,
}

/// A lista aberta sobre uma linha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChoiceList {
    /// Índice do bloco da linha dona da lista.
    pub block: usize,
    pub items: Vec<ChoiceItem>,
    pub highlighted: usize,
}

impl ChoiceList {
    /// Abre sobre a linha `block`, com `current` realçado -- o item que o
    /// valor em vista escolhe -- ou o primeiro, se `current` não está entre
    /// os itens.
    pub(crate) fn open(block: usize, items: Vec<ChoiceItem>, current: &str) -> Self {
        let highlighted = items
            .iter()
            .position(|item| item.value == current)
            .unwrap_or(0);
        Self {
            block,
            items,
            highlighted,
        }
    }

    /// Move o realce `delta` itens, parando nas pontas.
    pub(crate) fn move_highlight(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() as isize - 1;
        self.highlighted = (self.highlighted as isize + delta).clamp(0, last) as usize;
    }

    pub(crate) fn highlighted_value(&self) -> Option<&str> {
        self.items
            .get(self.highlighted)
            .map(|item| item.value.as_str())
    }
}

/// Onde a lista fica: o retângulo do menu e o de cada item.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChoiceLayout {
    pub menu: Rect,
    pub items: Vec<Rect>,
}

/// A largura do texto de um item, para quem corta os rótulos na abertura.
pub(crate) fn label_budget(config: &Config, anchor_width: f32) -> f32 {
    let menu = &config.appearance.context_menu;
    let width = anchor_width.max(menu.width as f32);
    (width - menu.padding as f32 * 2.0 - menu.item_padding_x as f32 * 2.0).max(0.0)
}

/// Corta `label` ao orçamento de um item (uma medição só, na abertura).
pub(crate) fn fit_label(
    label: &str,
    config: &Config,
    anchor_width: f32,
    measurer: &mut TextMeasurer,
) -> String {
    let size = config.appearance.context_menu.font_size as f32;
    measurer
        .truncate(label, BODY_FONT, size, label_budget(config, anchor_width))
        .0
}

/// Posiciona a lista sob `anchor` (o botão de escolha, em coordenadas de
/// janela), com a largura do botão ou a mínima do menu, a que for maior. Vira
/// para cima se não cabe embaixo e recua para dentro da janela nos dois eixos
/// -- "vira nos dois eixos para caber na tela" (§2.16).
pub(crate) fn layout(
    list: &ChoiceList,
    anchor: Rect,
    config: &Config,
    window_width: f32,
    window_height: f32,
) -> ChoiceLayout {
    let menu = &config.appearance.context_menu;
    let padding = menu.padding as f32;
    let item_height = menu.item_height as f32;
    let width = anchor.width.max(menu.width as f32);
    let height = padding * 2.0 + item_height * list.items.len() as f32;

    let mut x = anchor.x;
    let mut y = anchor.y + anchor.height;
    if y + height > window_height {
        // Não cabe embaixo: abre para cima, se couber, ou colada à base.
        let above = anchor.y - height;
        y = if above >= 0.0 {
            above
        } else {
            (window_height - height).max(0.0)
        };
    }
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    let rect = Rect {
        x,
        y,
        width,
        height,
    };
    let items = (0..list.items.len())
        .map(|index| Rect {
            x: rect.x + padding,
            y: rect.y + padding + item_height * index as f32,
            width: rect.width - padding * 2.0,
            height: item_height,
        })
        .collect();
    ChoiceLayout { menu: rect, items }
}

/// Pinta a lista: sombra, fundo e borda do menu de contexto, o item
/// realçado, e o texto de cada item já cortado.
pub(crate) fn paint(
    list: &ChoiceList,
    layout: &ChoiceLayout,
    config: &Config,
    pal: &ResolvedPalette,
) -> Vec<Primitive> {
    let cfg = &config.appearance.context_menu;
    let corner_radius = cfg.corner_radius as f32;
    let item_radius = cfg.item_corner_radius as f32;
    let item_padding_x = cfg.item_padding_x as f32;
    let font_size = cfg.font_size as f32;

    let mut out = Vec::new();
    push_shadow(&mut out, layout.menu, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.menu,
        radius: corner_radius,
        color: pal.context_menu_background,
        border_color: pal.context_menu_border,
        border_width: 1.0,
    }));
    for (index, (item, rect)) in list.items.iter().zip(&layout.items).enumerate() {
        if index == list.highlighted {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: *rect,
                radius: item_radius,
                color: pal.menu_item_hover,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + item_padding_x,
                rect.y + (rect.height - font_size) / 2.0,
            ),
            text: item.label.clone(),
            font: BODY_FONT,
            size_px: font_size,
            color: pal.menu_item_text,
        }));
    }
    out
}

/// O item sob `point`, se algum.
pub(crate) fn hit(layout: &ChoiceLayout, point: (f32, f32)) -> Option<usize> {
    layout
        .items
        .iter()
        .position(|rect| rect_contains(*rect, point))
}

/// O ponto está dentro do menu (item ou respiro): um clique ali não fecha a
/// lista.
pub(crate) fn contains(layout: &ChoiceLayout, point: (f32, f32)) -> bool {
    rect_contains(layout.menu, point)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(values: &[&str]) -> Vec<ChoiceItem> {
        values
            .iter()
            .map(|value| ChoiceItem {
                value: (*value).to_owned(),
                label: (*value).to_owned(),
            })
            .collect()
    }

    fn anchor() -> Rect {
        Rect {
            x: 400.0,
            y: 100.0,
            width: 240.0,
            height: 30.0,
        }
    }

    #[test]
    fn opens_with_the_current_value_highlighted() {
        let list = ChoiceList::open(2, items(&["de_DE", "en_US", "pt_BR"]), "en_US");
        assert_eq!(list.highlighted, 1);
        assert_eq!(list.highlighted_value(), Some("en_US"));
        // Valor que não está na lista: o primeiro.
        let list = ChoiceList::open(2, items(&["de_DE", "en_US"]), "xx_XX");
        assert_eq!(list.highlighted, 0);
    }

    #[test]
    fn the_highlight_stops_at_both_ends() {
        let mut list = ChoiceList::open(0, items(&["a", "b", "c"]), "a");
        list.move_highlight(-1);
        assert_eq!(list.highlighted, 0);
        list.move_highlight(5);
        assert_eq!(list.highlighted, 2);
        list.move_highlight(-1);
        assert_eq!(list.highlighted, 1);
    }

    #[test]
    fn the_list_sits_right_under_the_button_with_the_menu_geometry() {
        let config = Config::default();
        let list = ChoiceList::open(0, items(&["a", "b", "c"]), "a");
        let l = layout(&list, anchor(), &config, 900.0, 640.0);
        let menu = &config.appearance.context_menu;
        assert_eq!((l.menu.x, l.menu.y), (400.0, 130.0));
        assert_eq!(l.menu.width, 240.0);
        let padding = menu.padding as f32;
        let item_height = menu.item_height as f32;
        assert_eq!(l.menu.height, padding * 2.0 + item_height * 3.0);
        assert_eq!(l.items.len(), 3);
        assert_eq!(l.items[0].y, l.menu.y + padding);
        assert_eq!(l.items[1].y - l.items[0].y, item_height);
    }

    #[test]
    fn a_button_narrower_than_the_menu_minimum_gets_the_minimum() {
        let config = Config::default();
        let narrow = Rect {
            width: 88.0,
            ..anchor()
        };
        let list = ChoiceList::open(0, items(&["a"]), "a");
        let l = layout(&list, narrow, &config, 900.0, 640.0);
        assert_eq!(l.menu.width, config.appearance.context_menu.width as f32);
    }

    #[test]
    fn it_flips_above_the_button_when_it_does_not_fit_below() {
        let config = Config::default();
        let list = ChoiceList::open(0, items(&["a", "b", "c", "d", "e"]), "a");
        let low = Rect {
            y: 560.0,
            ..anchor()
        };
        let l = layout(&list, low, &config, 900.0, 640.0);
        assert!(l.menu.y + l.menu.height <= low.y);
    }

    #[test]
    fn it_never_leaves_the_window_sideways() {
        let config = Config::default();
        let list = ChoiceList::open(0, items(&["a"]), "a");
        let right = Rect {
            x: 800.0,
            ..anchor()
        };
        let l = layout(&list, right, &config, 900.0, 640.0);
        assert!(l.menu.x + l.menu.width <= 900.0);
    }

    #[test]
    fn hit_finds_the_item_and_the_menu_swallows_clicks_on_its_padding() {
        let config = Config::default();
        let list = ChoiceList::open(0, items(&["a", "b"]), "a");
        let l = layout(&list, anchor(), &config, 900.0, 640.0);
        let second = l.items[1];
        assert_eq!(hit(&l, (second.x + 2.0, second.y + 2.0)), Some(1));
        let on_padding = (l.menu.x + 1.0, l.menu.y + 1.0);
        assert_eq!(hit(&l, on_padding), None);
        assert!(contains(&l, on_padding));
        assert!(!contains(&l, (0.0, 0.0)));
    }

    #[test]
    fn the_highlighted_item_is_painted_over_the_menu() {
        let config = Config::default();
        let pal = ResolvedPalette::from_config(&config);
        let list = ChoiceList::open(0, items(&["a", "b"]), "b");
        let l = layout(&list, anchor(), &config, 900.0, 640.0);
        let out = paint(&list, &l, &config, &pal);
        let highlights = out
            .iter()
            .filter(|p| matches!(p, Primitive::RoundedQuad(q) if q.color == pal.menu_item_hover))
            .count();
        assert_eq!(highlights, 1);
        let texts: Vec<&str> = out
            .iter()
            .filter_map(|p| match p {
                Primitive::Text(run) => Some(run.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["a", "b"]);
    }
}
