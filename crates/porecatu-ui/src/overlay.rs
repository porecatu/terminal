// SPDX-License-Identifier: GPL-3.0-or-later

//! Pintura dos quatro widgets de chrome que vivem fora das camadas Grid/
//! Chrome (ADR-0018): aviso do app (`Layer::Warning`), diálogo de
//! confirmação (`Layer::Modal`) e menu de contexto + tooltip
//! (`Layer::Popover`, camada compartilhada por decisão explícita do
//! ADR-0019). Como `chrome.rs`, a geometria é calculada na hora de pintar
//! -- sem função pura de layout separada testável sem GPU, porque é
//! pintura, não geometria (mesma nota de `chrome.rs`/`paint.rs`); os
//! quatro tipos de estado (`WarningStack`, `ConfirmDialog`, `ContextMenu`,
//! `Hover`) continuam puros e testados nos próprios módulos.
//!
//! Nenhuma sombra: `porecatu-render` não tem primitiva de sombra (só quad,
//! arredondado, texto e clip), e os quatro widgets pedem uma na espec.
//! Ausência documentada, não esquecida -- mesma classe de simplificação do
//! `brightness` do fantasma de arraste (`chrome.rs`, Etapa 5). Corpo de
//! aviso e diálogo também não quebra linha (`TextRun` é sempre uma linha);
//! onde a espec pede várias linhas, o texto trunca em uma só com o
//! `TextMeasurer`, em vez do "três linhas com reticências" literal.

use porecatu_render::{
    Color, FontFace, Primitive, Quad, Rect, RoundedQuad, SansWeight, TextMeasurer, TextRun,
};

use crate::chrome::centered_glyph;
use crate::context_menu::{ContextMenu, TAB_MENU_ITEMS};
use crate::dialog::{ConfirmDialog, DialogButton};
use crate::palette;
use crate::tab_bar::rect_contains;
use crate::warning::{Severity, WarningStack};

const TITLE_FONT: FontFace = FontFace::Sans {
    weight: SansWeight::Medium,
};
const BODY_FONT: FontFace = FontFace::Sans {
    weight: SansWeight::Regular,
};

fn union(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.width).max(b.x + b.width);
    let y1 = (a.y + a.height).max(b.y + b.height);
    Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

// ---- aviso do app (espec §2.14, ADR-0014 canal 1) ----

const WARNING_WIDTH: f32 = 320.0;
const WARNING_PADDING_X: f32 = 12.0;
const WARNING_PADDING_Y: f32 = 11.0;
const WARNING_GAP: f32 = 8.0;
const WARNING_STACK_MARGIN_RIGHT: f32 = 10.0;
const WARNING_STACK_MARGIN_TOP: f32 = 8.0;
const WARNING_SEVERITY_BAR_WIDTH: f32 = 2.0;
const WARNING_TITLE_SIZE: f32 = 12.5;
const WARNING_BODY_SIZE: f32 = 11.0;
const WARNING_CLOSE_SIZE: f32 = 17.0;
const WARNING_CLOSE_ICON_SIZE: f32 = 10.0;
/// Sem métrica natural de linha (mesma simplificação de `FONT_SIZE_PX`/
/// `LINE_HEIGHT_MULTIPLIER` em `lib.rs`): valor de trabalho pra separar
/// título e corpo.
const WARNING_LINE_HEIGHT: f32 = 16.0;

pub struct WarningEntry {
    /// Índice em `WarningStack::items()` -- é o que `App::dismiss` precisa
    /// pra remover o item certo.
    pub index: usize,
    pub rect: Rect,
    pub close_button: Rect,
}

pub struct WarningLayout {
    /// Em ordem de desenho: mais recente no topo (espec §2.14).
    pub entries: Vec<WarningEntry>,
    /// União dos retângulos -- usada por `lib.rs` pra saber se o cursor
    /// está sobre a pilha e pausar o temporizador da informação.
    pub stack_rect: Rect,
}

pub fn layout_warnings(stack: &WarningStack, bar_height: f32, content_width: f32) -> WarningLayout {
    let x = content_width - WARNING_STACK_MARGIN_RIGHT - WARNING_WIDTH;
    let mut y = bar_height + WARNING_STACK_MARGIN_TOP;
    let mut entries = Vec::new();
    for (index, _) in stack.items().iter().enumerate().rev() {
        let height = WARNING_PADDING_Y * 2.0 + WARNING_TITLE_SIZE.max(WARNING_LINE_HEIGHT);
        let rect = Rect {
            x,
            y,
            width: WARNING_WIDTH,
            height,
        };
        let close_button = Rect {
            x: rect.x + rect.width - WARNING_PADDING_X - WARNING_CLOSE_SIZE + 4.0,
            y: rect.y + WARNING_PADDING_Y - 2.0,
            width: WARNING_CLOSE_SIZE,
            height: WARNING_CLOSE_SIZE,
        };
        entries.push(WarningEntry {
            index,
            rect,
            close_button,
        });
        y += height + WARNING_GAP;
    }
    let stack_rect = entries
        .iter()
        .fold(None, |acc: Option<Rect>, e| {
            Some(acc.map_or(e.rect, |a| union(a, e.rect)))
        })
        .unwrap_or(Rect {
            x,
            y: bar_height + WARNING_STACK_MARGIN_TOP,
            width: 0.0,
            height: 0.0,
        });
    WarningLayout {
        entries,
        stack_rect,
    }
}

pub fn paint_warnings(
    layout: &WarningLayout,
    stack: &WarningStack,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let mut out = Vec::new();
    for entry in &layout.entries {
        let warning = &stack.items()[entry.index];
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: entry.rect,
            radius: 8.0,
            color: palette::POPOVER_BACKGROUND,
            border_color: palette::POPOVER_BORDER,
            border_width: 1.0,
        }));
        let severity_color = match warning.severity {
            Severity::Error => palette::WARNING_SEVERITY_ERROR,
            Severity::Warning => palette::WARNING_SEVERITY_WARNING,
            Severity::Info => palette::WARNING_SEVERITY_INFO,
        };
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: entry.rect.x,
                y: entry.rect.y,
                width: WARNING_SEVERITY_BAR_WIDTH,
                height: entry.rect.height,
            },
            color: severity_color,
        }));

        let text_x = entry.rect.x + WARNING_SEVERITY_BAR_WIDTH + WARNING_PADDING_X;
        let title_y = entry.rect.y + WARNING_PADDING_Y;
        out.push(Primitive::Text(TextRun {
            origin: (text_x, title_y),
            text: warning.title.clone(),
            font: TITLE_FONT,
            size_px: WARNING_TITLE_SIZE,
            color: palette::WARNING_TITLE_TEXT,
        }));

        let body_max_width = (entry.rect.width
            - WARNING_SEVERITY_BAR_WIDTH
            - WARNING_PADDING_X * 2.0
            - WARNING_CLOSE_SIZE)
            .max(0.0);
        let (body, _) =
            measurer.truncate(&warning.body, BODY_FONT, WARNING_BODY_SIZE, body_max_width);
        out.push(Primitive::Text(TextRun {
            origin: (text_x, title_y + WARNING_LINE_HEIGHT),
            text: body,
            font: BODY_FONT,
            size_px: WARNING_BODY_SIZE,
            color: palette::WARNING_BODY_TEXT,
        }));

        out.push(centered_glyph(
            "\u{2715}",
            entry.close_button,
            WARNING_CLOSE_ICON_SIZE,
            palette::CLOSE_BUTTON_ICON,
            measurer,
        ));
    }
    out
}

/// Acha, em ordem de desenho, o aviso ou o botão de fechar sob `point`
/// (coordenadas lógicas da janela). Botão de fechar tem prioridade -- o
/// mesmo critério do `TabBarHit` da barra de abas.
pub enum WarningHit {
    Close(usize),
    Body,
}

pub fn hit_test_warnings(layout: &WarningLayout, point: (f32, f32)) -> Option<WarningHit> {
    for entry in &layout.entries {
        if rect_contains(entry.close_button, point) {
            return Some(WarningHit::Close(entry.index));
        }
    }
    for entry in &layout.entries {
        if rect_contains(entry.rect, point) {
            return Some(WarningHit::Body);
        }
    }
    None
}

// ---- diálogo de confirmação (espec §2.15, ADR-0014) ----

const DIALOG_WIDTH: f32 = 380.0;
const DIALOG_PADDING: f32 = 16.0;
const DIALOG_TITLE_SIZE: f32 = 13.0;
const DIALOG_BODY_SIZE: f32 = 12.5;
const DIALOG_GAP: f32 = 14.0;
const DIALOG_BUTTON_HEIGHT: f32 = 30.0;
const DIALOG_BUTTON_GAP: f32 = 8.0;
const DIALOG_BUTTON_PADDING_X: f32 = 12.0;
const DIALOG_CANCEL_LABEL: &str = "Cancelar";

pub struct DialogLayout {
    pub modal_rect: Rect,
    pub cancel_rect: Rect,
    pub confirm_rect: Rect,
}

pub fn layout_dialog(
    window_width: f32,
    window_height: f32,
    dialog: &ConfirmDialog,
    measurer: &mut TextMeasurer,
) -> DialogLayout {
    let cancel_width = measurer.measure_width(DIALOG_CANCEL_LABEL, BODY_FONT, DIALOG_BODY_SIZE)
        + DIALOG_BUTTON_PADDING_X * 2.0;
    let confirm_width = measurer.measure_width(&dialog.confirm_label, BODY_FONT, DIALOG_BODY_SIZE)
        + DIALOG_BUTTON_PADDING_X * 2.0;

    let content_height =
        DIALOG_TITLE_SIZE + DIALOG_GAP + DIALOG_BODY_SIZE + DIALOG_GAP + DIALOG_BUTTON_HEIGHT;
    let modal_height = DIALOG_PADDING * 2.0 + content_height;
    let modal_rect = Rect {
        x: (window_width - DIALOG_WIDTH) / 2.0,
        y: (window_height - modal_height) / 2.0,
        width: DIALOG_WIDTH,
        height: modal_height,
    };

    let buttons_y = modal_rect.y + modal_rect.height - DIALOG_PADDING - DIALOG_BUTTON_HEIGHT;
    let confirm_rect = Rect {
        x: modal_rect.x + modal_rect.width - DIALOG_PADDING - confirm_width,
        y: buttons_y,
        width: confirm_width,
        height: DIALOG_BUTTON_HEIGHT,
    };
    let cancel_rect = Rect {
        x: confirm_rect.x - DIALOG_BUTTON_GAP - cancel_width,
        y: buttons_y,
        width: cancel_width,
        height: DIALOG_BUTTON_HEIGHT,
    };

    DialogLayout {
        modal_rect,
        cancel_rect,
        confirm_rect,
    }
}

pub fn paint_dialog(
    layout: &DialogLayout,
    dialog: &ConfirmDialog,
    window_width: f32,
    window_height: f32,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let mut out = Vec::new();
    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: window_width,
            height: window_height,
        },
        color: palette::DIALOG_OVERLAY,
    }));
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.modal_rect,
        radius: 10.0,
        color: palette::POPOVER_BACKGROUND,
        border_color: palette::POPOVER_BORDER,
        border_width: 1.0,
    }));

    let text_x = layout.modal_rect.x + DIALOG_PADDING;
    let mut y = layout.modal_rect.y + DIALOG_PADDING;
    out.push(Primitive::Text(TextRun {
        origin: (text_x, y),
        text: dialog.title.clone(),
        font: TITLE_FONT,
        size_px: DIALOG_TITLE_SIZE,
        color: palette::DIALOG_TITLE_TEXT,
    }));
    y += DIALOG_TITLE_SIZE + DIALOG_GAP;
    let body_max_width = layout.modal_rect.width - DIALOG_PADDING * 2.0;
    let (body, _) = measurer.truncate(&dialog.body, BODY_FONT, DIALOG_BODY_SIZE, body_max_width);
    out.push(Primitive::Text(TextRun {
        origin: (text_x, y),
        text: body,
        font: BODY_FONT,
        size_px: DIALOG_BODY_SIZE,
        color: palette::DIALOG_BODY_TEXT,
    }));

    paint_dialog_button(
        layout.cancel_rect,
        DIALOG_CANCEL_LABEL,
        palette::TRANSPARENT,
        palette::DIALOG_CANCEL_TEXT,
        if dialog.focused() == DialogButton::Cancel {
            palette::DIALOG_FOCUS_RING
        } else {
            palette::DIALOG_CANCEL_BORDER
        },
        measurer,
        &mut out,
    );
    paint_dialog_button(
        layout.confirm_rect,
        &dialog.confirm_label,
        palette::DIALOG_CONFIRM_BACKGROUND,
        palette::DIALOG_CONFIRM_TEXT,
        if dialog.focused() == DialogButton::Confirm {
            palette::DIALOG_FOCUS_RING
        } else {
            palette::TRANSPARENT
        },
        measurer,
        &mut out,
    );

    out
}

#[allow(clippy::too_many_arguments)]
fn paint_dialog_button(
    rect: Rect,
    label: &str,
    background: Color,
    text_color: Color,
    border_color: Color,
    measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: 5.0,
        color: background,
        border_color,
        border_width: 1.0,
    }));
    let width = measurer.measure_width(label, BODY_FONT, DIALOG_BODY_SIZE);
    out.push(Primitive::Text(TextRun {
        origin: (
            rect.x + (rect.width - width) / 2.0,
            rect.y + (rect.height - DIALOG_BODY_SIZE) / 2.0,
        ),
        text: label.to_string(),
        font: BODY_FONT,
        size_px: DIALOG_BODY_SIZE,
        color: text_color,
    }));
}

pub fn dialog_hit(layout: &DialogLayout, point: (f32, f32)) -> Option<DialogButton> {
    if rect_contains(layout.cancel_rect, point) {
        Some(DialogButton::Cancel)
    } else if rect_contains(layout.confirm_rect, point) {
        Some(DialogButton::Confirm)
    } else {
        None
    }
}

// ---- menu de contexto (espec §2.16, ADR-0014) ----

const MENU_WIDTH: f32 = 200.0;
const MENU_PADDING: f32 = 6.0;
/// Sem métrica natural de linha, mesma nota de `WARNING_LINE_HEIGHT`.
const MENU_ITEM_HEIGHT: f32 = 28.0;
const MENU_ITEM_TEXT_SIZE: f32 = 12.5;
const MENU_ITEM_RADIUS: f32 = 5.0;
const MENU_ITEM_PADDING_X: f32 = 8.0;

pub struct MenuLayout {
    pub menu_rect: Rect,
    /// Alinhado por índice com `TAB_MENU_ITEMS`.
    pub item_rects: Vec<Rect>,
}

pub fn layout_context_menu(
    menu: &ContextMenu,
    window_width: f32,
    window_height: f32,
) -> MenuLayout {
    let height = MENU_PADDING * 2.0 + MENU_ITEM_HEIGHT * TAB_MENU_ITEMS.len() as f32;
    let mut x = menu.anchor.0;
    let mut y = menu.anchor.1;
    // "Vira nos dois eixos para caber na tela" (espec §2.16) -- contra a
    // janela, não o monitor: multi-monitor físico é responsabilidade de
    // `winit`/SO posicionar a própria janela, isto só evita estourar a
    // borda da janela atual.
    if x + MENU_WIDTH > window_width {
        x = (window_width - MENU_WIDTH).max(0.0);
    }
    if y + height > window_height {
        y = (window_height - height).max(0.0);
    }
    let menu_rect = Rect {
        x,
        y,
        width: MENU_WIDTH,
        height,
    };
    let item_rects = (0..TAB_MENU_ITEMS.len())
        .map(|i| Rect {
            x: menu_rect.x + MENU_PADDING,
            y: menu_rect.y + MENU_PADDING + MENU_ITEM_HEIGHT * i as f32,
            width: menu_rect.width - MENU_PADDING * 2.0,
            height: MENU_ITEM_HEIGHT,
        })
        .collect();
    MenuLayout {
        menu_rect,
        item_rects,
    }
}

pub fn paint_context_menu(layout: &MenuLayout, menu: &ContextMenu) -> Vec<Primitive> {
    let mut out = Vec::new();
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.menu_rect,
        radius: 8.0,
        color: palette::POPOVER_BACKGROUND,
        border_color: palette::POPOVER_BORDER,
        border_width: 1.0,
    }));
    for (index, item) in TAB_MENU_ITEMS.iter().enumerate() {
        let rect = layout.item_rects[index];
        if index == menu.highlighted() {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: MENU_ITEM_RADIUS,
                color: palette::MENU_ITEM_HOVER,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let color = if item.enabled {
            palette::MENU_ITEM_TEXT
        } else {
            palette::MENU_ITEM_DISABLED_TEXT
        };
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + MENU_ITEM_PADDING_X,
                rect.y + (rect.height - MENU_ITEM_TEXT_SIZE) / 2.0,
            ),
            text: item.label.to_string(),
            font: BODY_FONT,
            size_px: MENU_ITEM_TEXT_SIZE,
            color,
        }));
    }
    out
}

/// Índice do item sob `point`, se algum (aceita item desabilitado -- quem
/// decide se aciona é `lib.rs`, olhando `TAB_MENU_ITEMS[index].enabled`).
pub fn context_menu_hit(layout: &MenuLayout, point: (f32, f32)) -> Option<usize> {
    layout
        .item_rects
        .iter()
        .position(|&rect| rect_contains(rect, point))
}

// ---- tooltip (espec §2.20, ADR-0019) ----

const TOOLTIP_MAX_WIDTH: f32 = 320.0;
const TOOLTIP_PADDING_X: f32 = 8.0;
const TOOLTIP_PADDING_Y: f32 = 7.0;
const TOOLTIP_TEXT_SIZE: f32 = 11.0;
const TOOLTIP_GAP: f32 = 6.0;

pub fn paint_tooltip(
    anchor: Rect,
    text: &str,
    window_width: f32,
    window_height: f32,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let available = (TOOLTIP_MAX_WIDTH - TOOLTIP_PADDING_X * 2.0).max(0.0);
    let (label, _) = measurer.truncate(text, BODY_FONT, TOOLTIP_TEXT_SIZE, available);
    let text_width = measurer.measure_width(&label, BODY_FONT, TOOLTIP_TEXT_SIZE);
    let width = text_width + TOOLTIP_PADDING_X * 2.0;
    let height = TOOLTIP_TEXT_SIZE + TOOLTIP_PADDING_Y * 2.0;

    let mut x = anchor.x;
    let mut y = anchor.y + anchor.height + TOOLTIP_GAP;
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    if y + height > window_height {
        y = anchor.y - TOOLTIP_GAP - height;
    }
    let rect = Rect {
        x,
        y,
        width,
        height,
    };

    vec![
        Primitive::RoundedQuad(RoundedQuad {
            rect,
            radius: 6.0,
            color: palette::POPOVER_BACKGROUND,
            border_color: palette::POPOVER_BORDER,
            border_width: 1.0,
        }),
        Primitive::Text(TextRun {
            origin: (rect.x + TOOLTIP_PADDING_X, rect.y + TOOLTIP_PADDING_Y),
            text: label,
            font: BODY_FONT,
            size_px: TOOLTIP_TEXT_SIZE,
            color: palette::TOOLTIP_TEXT,
        }),
    ]
}
