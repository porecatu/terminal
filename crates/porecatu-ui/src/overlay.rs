// SPDX-License-Identifier: GPL-3.0-or-later

//! Pintura dos widgets de chrome que vivem fora das camadas Grid/Chrome
//! (ADR-0018): aviso do app (`Layer::Warning`), diálogo de confirmação
//! (`Layer::Modal`) e menu de contexto de aba + tooltip (`Layer::Popover`,
//! camada compartilhada por decisão explícita do ADR-0019). Como
//! `chrome.rs`, a geometria é calculada na hora de pintar -- sem função
//! pura de layout separada testável sem GPU, porque é pintura, não
//! geometria (mesma nota de `chrome.rs`/`paint.rs`); os tipos de estado
//! (`WarningStack`, `ConfirmDialog`, `ContextMenu`, `Hover`,
//! `GroupContextMenu`, `GroupEditor`, `MoveToGroupPopover`) continuam
//! puros e testados nos próprios módulos.
//!
//! Desde a F3 etapa 5 (ADR-0023), também o menu de contexto de grupo, o
//! editor de grupo -- quinto widget de chrome, também na camada popover --
//! e o popover de destino do `tab.move_to_group`. Os três nunca coexistem
//! entre si nem com o menu de aba/tooltip (mutuamente exclusivos,
//! garantido em `lib.rs`); o popover de destino é a primeira lista rolável
//! do chrome, mas sem gesto de roda do mouse nesta etapa -- só o realce
//! por teclado/clique arrasta a janela visível (ver nota da seção
//! correspondente). O editor não tem clip de linha múltipla para o nome:
//! como o campo de rename da aba, rola dentro de si mesmo mantendo o caret
//! visível, sem quebrar linha.
//!
//! Nenhuma sombra: `porecatu-render` não tem primitiva de sombra (só quad,
//! arredondado, texto e clip), e os widgets pedem uma na espec. Ausência
//! documentada, não esquecida -- mesma classe de simplificação do
//! `brightness` do fantasma de arraste (`chrome.rs`, Etapa 5). Corpo de
//! aviso e diálogo também não quebra linha (`TextRun` é sempre uma linha);
//! onde a espec pede várias linhas, o texto trunca em uma só com o
//! `TextMeasurer`, em vez do "três linhas com reticências" literal.

use porecatu_core::{GroupColor, Workspace};
use porecatu_render::{
    Color, FontFace, Primitive, Quad, Rect, RoundedQuad, SansWeight, TextMeasurer, TextRun, icon,
};

use crate::chrome::centered_glyph;
use crate::context_menu::{ContextMenu, TAB_MENU_ITEMS};
use crate::dialog::{ConfirmDialog, DialogButton};
use crate::group_editor::{EditorRegion, GroupEditor};
use crate::group_menu::{self, EDITOR_ACTION_ORDER, GroupActionItem, GroupContextMenu};
use crate::move_to_group::MoveToGroupPopover;
use crate::palette;
use crate::tab_bar::{self, rect_contains};
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
// Em, não desenho -- mesmo tamanho do botão de fechar da aba, que a
// espec. §2.15 diz ser "o mesmo botão". Ver `porecatu_render::icon`.
const WARNING_CLOSE_ICON_SIZE: f32 = crate::chrome::ICON_EM_SIZE;
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
            icon::X,
            entry.close_button,
            WARNING_CLOSE_ICON_SIZE,
            palette::CLOSE_BUTTON_ICON,
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

// ---- menu de contexto de grupo (espec §2.16, RF-2.22, ADR-0023) ----

/// Mesma anatomia do menu de aba (`layout_context_menu`/
/// `paint_context_menu`), duplicada em vez de generalizada -- os itens são
/// `GroupActionItem` (rótulo dinâmico, possivelmente destrutivo), não o
/// `MenuItem` estático de `TAB_MENU_ITEMS`, e a lista não é uma constante
/// global (`group_menu::group_action_items` resolve na hora, a partir do
/// estado do grupo).
pub struct GroupMenuLayout {
    pub menu_rect: Rect,
    pub item_rects: Vec<Rect>,
}

pub fn layout_group_menu(
    menu: &GroupContextMenu,
    item_count: usize,
    window_width: f32,
    window_height: f32,
) -> GroupMenuLayout {
    let height = MENU_PADDING * 2.0 + MENU_ITEM_HEIGHT * item_count as f32;
    let mut x = menu.anchor.0;
    let mut y = menu.anchor.1;
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
    let item_rects = (0..item_count)
        .map(|i| Rect {
            x: menu_rect.x + MENU_PADDING,
            y: menu_rect.y + MENU_PADDING + MENU_ITEM_HEIGHT * i as f32,
            width: menu_rect.width - MENU_PADDING * 2.0,
            height: MENU_ITEM_HEIGHT,
        })
        .collect();
    GroupMenuLayout {
        menu_rect,
        item_rects,
    }
}

pub fn paint_group_menu(
    layout: &GroupMenuLayout,
    items: &[GroupActionItem],
    highlighted: usize,
) -> Vec<Primitive> {
    let mut out = Vec::new();
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.menu_rect,
        radius: 8.0,
        color: palette::POPOVER_BACKGROUND,
        border_color: palette::POPOVER_BORDER,
        border_width: 1.0,
    }));
    for (index, item) in items.iter().enumerate() {
        let rect = layout.item_rects[index];
        if index == highlighted {
            let hover_color = if item.destructive {
                palette::MENU_ITEM_DESTRUCTIVE_HOVER
            } else {
                palette::MENU_ITEM_HOVER
            };
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: MENU_ITEM_RADIUS,
                color: hover_color,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let text_color = if item.destructive {
            palette::MENU_ITEM_DESTRUCTIVE_TEXT
        } else {
            palette::MENU_ITEM_TEXT
        };
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + MENU_ITEM_PADDING_X,
                rect.y + (rect.height - MENU_ITEM_TEXT_SIZE) / 2.0,
            ),
            text: item.label.clone(),
            font: BODY_FONT,
            size_px: MENU_ITEM_TEXT_SIZE,
            color: text_color,
        }));
    }
    out
}

pub fn group_menu_hit(layout: &GroupMenuLayout, point: (f32, f32)) -> Option<usize> {
    layout
        .item_rects
        .iter()
        .position(|&rect| rect_contains(rect, point))
}

// ---- editor de grupo (espec §2.10, ADR-0023) ----

const EDITOR_WIDTH: f32 = 286.0; // [appearance.group_editor] width
const EDITOR_PADDING: f32 = 14.0; // padding
const EDITOR_GAP: f32 = 13.0; // gap
const EDITOR_CORNER_RADIUS: f32 = 8.0; // corner_radius
const EDITOR_OFFSET_Y: f32 = 8.0; // offset_y
const EDITOR_SECTION_FONT_SIZE: f32 = 10.0; // section_font_size
/// Espaço entre a legenda de seção ("GRUPO"/"COR") e o conteúdo abaixo --
/// sem chave própria no TOML, mesma nota de `RENAME_FIELD_HEIGHT` em
/// `chrome.rs`.
const EDITOR_SECTION_CAPTION_GAP: f32 = 6.0;
/// Altura do campo de nome -- a espec. dá padding (7px 9px) e fonte (13px)
/// mas não a altura da caixa; valor de trabalho, mesma nota.
const EDITOR_INPUT_HEIGHT: f32 = 30.0;
const EDITOR_INPUT_CORNER_RADIUS: f32 = 5.0; // input_corner_radius
const EDITOR_INPUT_FONT_SIZE: f32 = 13.0; // input_font_size
const EDITOR_INPUT_PADDING_X: f32 = 9.0; // espec §2.10 item 1: "padding: 7px 9px"
const EDITOR_SWATCH_SIZE: f32 = 28.0; // swatch_size
const EDITOR_SWATCH_CORNER_RADIUS: f32 = 6.0; // swatch_corner_radius
const EDITOR_SWATCH_GAP: f32 = 8.0; // swatch_gap
const EDITOR_SWATCH_BORDER_WIDTH: f32 = 2.0; // swatch_border_width
/// Realce de foco por teclado/hover do swatch (espec: "hover e foco por
/// teclado são o mesmo realce", mesma regra do §2.16) -- sem tamanho
/// próprio na espec.; valor de trabalho, halo de 3px ao redor do swatch.
const EDITOR_SWATCH_HIGHLIGHT_PAD: f32 = 3.0;
const EDITOR_DIVIDER_HEIGHT: f32 = 1.0; // divider (espessura)
const EDITOR_ITEM_HEIGHT: f32 = MENU_ITEM_HEIGHT;
const EDITOR_ITEM_RADIUS: f32 = MENU_ITEM_RADIUS;
const EDITOR_ITEM_PADDING_X: f32 = MENU_ITEM_PADDING_X;
const EDITOR_ITEM_TEXT_SIZE: f32 = MENU_ITEM_TEXT_SIZE;
const EDITOR_SECTION_GROUP_LABEL: &str = "GRUPO";
const EDITOR_SECTION_COLOR_LABEL: &str = "COR";

pub struct GroupEditorLayout {
    pub popover_rect: Rect,
    pub name_caption_origin: (f32, f32),
    pub name_input_rect: Rect,
    pub color_caption_origin: (f32, f32),
    /// Seis, na ordem de `GroupColor::ALL`.
    pub swatch_rects: Vec<Rect>,
    pub divider_rect: Rect,
    /// Alinhado por índice com `EDITOR_ACTION_ORDER`.
    pub action_rects: Vec<Rect>,
}

/// `anchor_x`: canto esquerdo da pílula do grupo, em coordenadas de tela
/// (espec: "posicionado horizontalmente sobre o grupo que está sendo
/// editado"). `bar_bottom_y`: borda inferior da barra de abas -- o popover
/// nasce 8px abaixo dela (`EDITOR_OFFSET_Y`); com `tab_bar_position =
/// "bottom"` ele abriria acima, mas a config não existe ainda (F4), então
/// só o caso `top` (único alcançável) é implementado -- mesmo tipo de
/// simplificação que outras chaves de `[appearance]` já assumem enquanto
/// não há como configurá-las.
pub fn layout_group_editor(
    anchor_x: f32,
    bar_bottom_y: f32,
    window_width: f32,
    window_height: f32,
) -> GroupEditorLayout {
    let action_count = EDITOR_ACTION_ORDER.len();
    let content_height = EDITOR_SECTION_FONT_SIZE
        + EDITOR_SECTION_CAPTION_GAP
        + EDITOR_INPUT_HEIGHT
        + EDITOR_GAP
        + EDITOR_SECTION_FONT_SIZE
        + EDITOR_SECTION_CAPTION_GAP
        + EDITOR_SWATCH_SIZE
        + EDITOR_GAP
        + EDITOR_DIVIDER_HEIGHT
        + EDITOR_GAP
        + EDITOR_ITEM_HEIGHT * action_count as f32;
    let popover_height = EDITOR_PADDING * 2.0 + content_height;

    let mut x = anchor_x;
    if x + EDITOR_WIDTH > window_width {
        x = (window_width - EDITOR_WIDTH).max(0.0);
    }
    let mut y = bar_bottom_y + EDITOR_OFFSET_Y;
    if y + popover_height > window_height {
        y = (window_height - popover_height).max(0.0);
    }
    let popover_rect = Rect {
        x,
        y,
        width: EDITOR_WIDTH,
        height: popover_height,
    };

    let inner_x = popover_rect.x + EDITOR_PADDING;
    let inner_width = popover_rect.width - EDITOR_PADDING * 2.0;
    let mut cursor_y = popover_rect.y + EDITOR_PADDING;

    let name_caption_origin = (inner_x, cursor_y);
    cursor_y += EDITOR_SECTION_FONT_SIZE + EDITOR_SECTION_CAPTION_GAP;
    let name_input_rect = Rect {
        x: inner_x,
        y: cursor_y,
        width: inner_width,
        height: EDITOR_INPUT_HEIGHT,
    };
    cursor_y += EDITOR_INPUT_HEIGHT + EDITOR_GAP;

    let color_caption_origin = (inner_x, cursor_y);
    cursor_y += EDITOR_SECTION_FONT_SIZE + EDITOR_SECTION_CAPTION_GAP;
    let mut swatch_rects = Vec::with_capacity(GroupColor::ALL.len());
    let mut sx = inner_x;
    for _ in GroupColor::ALL {
        swatch_rects.push(Rect {
            x: sx,
            y: cursor_y,
            width: EDITOR_SWATCH_SIZE,
            height: EDITOR_SWATCH_SIZE,
        });
        sx += EDITOR_SWATCH_SIZE + EDITOR_SWATCH_GAP;
    }
    cursor_y += EDITOR_SWATCH_SIZE + EDITOR_GAP;

    let divider_rect = Rect {
        x: inner_x,
        y: cursor_y,
        width: inner_width,
        height: EDITOR_DIVIDER_HEIGHT,
    };
    cursor_y += EDITOR_DIVIDER_HEIGHT + EDITOR_GAP;

    let action_rects = (0..action_count)
        .map(|i| Rect {
            x: inner_x,
            y: cursor_y + EDITOR_ITEM_HEIGHT * i as f32,
            width: inner_width,
            height: EDITOR_ITEM_HEIGHT,
        })
        .collect();

    GroupEditorLayout {
        popover_rect,
        name_caption_origin,
        name_input_rect,
        color_caption_origin,
        swatch_rects,
        divider_rect,
        action_rects,
    }
}

fn expand(rect: Rect, amount: f32) -> Rect {
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        width: rect.width + amount * 2.0,
        height: rect.height + amount * 2.0,
    }
}

/// `current_color_index`/`is_collapsed`/`tab_count`: estado corrente do
/// grupo, lido fresco por quem chama (`lib.rs`) -- nunca guardado em
/// `GroupEditor` (módulo doc). O campo de nome desenha o **buffer ao
/// vivo** (`editor.name_buffer()`), não o nome real do grupo: é o mesmo
/// truque do campo de rename de aba (`chrome::paint_rename_field`) --
/// texto rola dentro do campo mantendo o caret visível, sem quebrar linha.
#[allow(clippy::too_many_arguments)]
pub fn paint_group_editor(
    layout: &GroupEditorLayout,
    editor: &GroupEditor,
    current_color_index: usize,
    is_collapsed: bool,
    tab_count: usize,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let mut out = Vec::new();
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.popover_rect,
        radius: EDITOR_CORNER_RADIUS,
        color: palette::POPOVER_BACKGROUND,
        border_color: palette::POPOVER_BORDER,
        border_width: 1.0,
    }));

    out.push(Primitive::Text(TextRun {
        origin: layout.name_caption_origin,
        text: EDITOR_SECTION_GROUP_LABEL.to_string(),
        font: TITLE_FONT,
        size_px: EDITOR_SECTION_FONT_SIZE,
        color: palette::EDITOR_SECTION_TEXT,
    }));
    let focused_name = editor.focus() == EditorRegion::Name;
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.name_input_rect,
        radius: EDITOR_INPUT_CORNER_RADIUS,
        color: palette::EDITOR_INPUT_BACKGROUND,
        border_color: if focused_name {
            palette::EDITOR_INPUT_BORDER_FOCUS
        } else {
            palette::EDITOR_INPUT_BORDER
        },
        border_width: 1.0,
    }));
    let buffer = editor.name_buffer();
    let available_text_width =
        (layout.name_input_rect.width - EDITOR_INPUT_PADDING_X * 2.0).max(0.0);
    let text_width = measurer.measure_width(buffer, BODY_FONT, EDITOR_INPUT_FONT_SIZE);
    let text_x = if text_width > available_text_width {
        layout.name_input_rect.x + EDITOR_INPUT_PADDING_X - (text_width - available_text_width)
    } else {
        layout.name_input_rect.x + EDITOR_INPUT_PADDING_X
    };
    let text_y =
        layout.name_input_rect.y + (layout.name_input_rect.height - EDITOR_INPUT_FONT_SIZE) / 2.0;
    out.push(Primitive::PushClip(layout.name_input_rect));
    out.push(Primitive::Text(TextRun {
        origin: (text_x, text_y),
        text: buffer.to_string(),
        font: BODY_FONT,
        size_px: EDITOR_INPUT_FONT_SIZE,
        color: palette::EDITOR_INPUT_TEXT,
    }));
    if focused_name {
        let caret_x = (text_x + text_width)
            .min(layout.name_input_rect.x + layout.name_input_rect.width - 1.0);
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: caret_x,
                y: layout.name_input_rect.y + 4.0,
                width: 1.0,
                height: layout.name_input_rect.height - 8.0,
            },
            color: palette::EDITOR_INPUT_TEXT,
        }));
    }
    out.push(Primitive::PopClip);

    out.push(Primitive::Text(TextRun {
        origin: layout.color_caption_origin,
        text: EDITOR_SECTION_COLOR_LABEL.to_string(),
        font: TITLE_FONT,
        size_px: EDITOR_SECTION_FONT_SIZE,
        color: palette::EDITOR_SECTION_TEXT,
    }));
    for (index, color) in GroupColor::ALL.iter().enumerate() {
        let rect = layout.swatch_rects[index];
        let is_highlighted =
            editor.focus() == EditorRegion::Swatches && editor.swatch_highlight() == index;
        if is_highlighted {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: expand(rect, EDITOR_SWATCH_HIGHLIGHT_PAD),
                radius: EDITOR_SWATCH_CORNER_RADIUS + EDITOR_SWATCH_HIGHLIGHT_PAD,
                color: palette::MENU_ITEM_HOVER,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let is_current = index == current_color_index;
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect,
            radius: EDITOR_SWATCH_CORNER_RADIUS,
            color: palette::group_color(*color),
            border_color: if is_current {
                palette::EDITOR_SWATCH_RING
            } else {
                palette::TRANSPARENT
            },
            border_width: EDITOR_SWATCH_BORDER_WIDTH,
        }));
    }

    out.push(Primitive::Quad(Quad {
        rect: layout.divider_rect,
        color: palette::EDITOR_DIVIDER,
    }));

    let items = group_menu::group_action_items(is_collapsed, tab_count);
    for (i, action) in EDITOR_ACTION_ORDER.iter().enumerate() {
        let item = items
            .iter()
            .find(|it| it.action == *action)
            .expect("EDITOR_ACTION_ORDER é subconjunto de GROUP_ACTION_ORDER");
        let rect = layout.action_rects[i];
        let is_highlighted =
            editor.focus() == EditorRegion::Actions && editor.action_highlight() == i;
        if is_highlighted {
            let hover = if item.destructive {
                palette::MENU_ITEM_DESTRUCTIVE_HOVER
            } else {
                palette::MENU_ITEM_HOVER
            };
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: EDITOR_ITEM_RADIUS,
                color: hover,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let text_color = if item.destructive {
            palette::MENU_ITEM_DESTRUCTIVE_TEXT
        } else {
            palette::MENU_ITEM_TEXT
        };
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + EDITOR_ITEM_PADDING_X,
                rect.y + (rect.height - EDITOR_ITEM_TEXT_SIZE) / 2.0,
            ),
            text: item.label.clone(),
            font: BODY_FONT,
            size_px: EDITOR_ITEM_TEXT_SIZE,
            color: text_color,
        }));
    }

    out
}

pub enum GroupEditorHit {
    NameField,
    Swatch(usize),
    Action(usize),
}

pub fn group_editor_hit(layout: &GroupEditorLayout, point: (f32, f32)) -> Option<GroupEditorHit> {
    if rect_contains(layout.name_input_rect, point) {
        return Some(GroupEditorHit::NameField);
    }
    for (i, &rect) in layout.swatch_rects.iter().enumerate() {
        if rect_contains(rect, point) {
            return Some(GroupEditorHit::Swatch(i));
        }
    }
    for (i, &rect) in layout.action_rects.iter().enumerate() {
        if rect_contains(rect, point) {
            return Some(GroupEditorHit::Action(i));
        }
    }
    None
}

// ---- popover de destino do tab.move_to_group (RF-2.20, ADR-0023 §4) ----

const MOVE_POPOVER_WIDTH: f32 = MENU_WIDTH;
const MOVE_ROW_HEIGHT: f32 = MENU_ITEM_HEIGHT;
/// Teto de linhas visíveis de uma vez -- sem valor de design pra altura
/// máxima do popover (a espec só fixa isso pro menu comum, que não rola);
/// 6 é o mesmo "meia dúzia de itens" que a §2.16 usa como referência de
/// tamanho típico de lista do v1.
const MOVE_MAX_VISIBLE_ROWS: usize = 6;
const MOVE_ROW_PADDING_X: f32 = MENU_ITEM_PADDING_X;
/// Menor que a pílula (§2.4) -- a linha do popover é mais apertada; sem
/// valor de design próprio, escolha de implementação. A pílula da barra
/// não tem mais um swatch para comparar (pintada com a cor cheia do
/// grupo, pedido do usuário); este é o único swatch pequeno que restou.
const MOVE_SWATCH_SIZE: f32 = 8.0;
const MOVE_SWATCH_GAP: f32 = 8.0;
const MOVE_NEW_GROUP_LABEL: &str = "Novo grupo";

pub struct MoveToGroupLayout {
    pub popover_rect: Rect,
    /// Só as linhas visíveis na janela de rolagem atual -- alinhadas por
    /// índice com `first_visible_index + i`, não com `targets()` direto.
    pub visible_row_rects: Vec<Rect>,
    pub first_visible_index: usize,
}

/// Deriva a rolagem a partir de `popover.highlighted()`, sem estado de
/// scroll próprio (nota do módulo `move_to_group.rs`): a janela visível
/// sempre contém o item realçado, arrastando o mínimo necessário --mesmo
/// princípio de `WindowState::ensure_active_tab_visible`.
pub fn layout_move_to_group(
    popover: &MoveToGroupPopover,
    window_width: f32,
    window_height: f32,
) -> MoveToGroupLayout {
    let total_rows = popover.row_count();
    let visible_rows = total_rows.min(MOVE_MAX_VISIBLE_ROWS);
    let max_first = total_rows.saturating_sub(visible_rows);
    let highlighted = popover.highlighted();
    let first_visible_index = highlighted
        .saturating_sub(visible_rows.saturating_sub(1))
        .min(max_first);

    let height = MENU_PADDING * 2.0 + MOVE_ROW_HEIGHT * visible_rows as f32;
    let mut x = popover.anchor.0;
    let mut y = popover.anchor.1;
    if x + MOVE_POPOVER_WIDTH > window_width {
        x = (window_width - MOVE_POPOVER_WIDTH).max(0.0);
    }
    if y + height > window_height {
        y = (window_height - height).max(0.0);
    }
    let popover_rect = Rect {
        x,
        y,
        width: MOVE_POPOVER_WIDTH,
        height,
    };

    let visible_row_rects = (0..visible_rows)
        .map(|i| Rect {
            x: popover_rect.x + MENU_PADDING,
            y: popover_rect.y + MENU_PADDING + MOVE_ROW_HEIGHT * i as f32,
            width: popover_rect.width - MENU_PADDING * 2.0,
            height: MOVE_ROW_HEIGHT,
        })
        .collect();

    MoveToGroupLayout {
        popover_rect,
        visible_row_rects,
        first_visible_index,
    }
}

pub fn paint_move_to_group(
    layout: &MoveToGroupLayout,
    popover: &MoveToGroupPopover,
    workspace: &Workspace,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let mut out = Vec::new();
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.popover_rect,
        radius: 8.0,
        color: palette::POPOVER_BACKGROUND,
        border_color: palette::POPOVER_BORDER,
        border_width: 1.0,
    }));
    for (visible_i, &rect) in layout.visible_row_rects.iter().enumerate() {
        let row_index = layout.first_visible_index + visible_i;
        if row_index == popover.highlighted() {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: MENU_ITEM_RADIUS,
                color: palette::MENU_ITEM_HOVER,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        if row_index < popover.targets().len() {
            let group_id = popover.targets()[row_index];
            let Some(group) = workspace.group(group_id) else {
                continue;
            };
            let color = group
                .color()
                .map(palette::group_color)
                .unwrap_or(palette::UNGROUPED_GROUP_COLOR);
            let swatch_rect = Rect {
                x: rect.x,
                y: rect.y + (rect.height - MOVE_SWATCH_SIZE) / 2.0,
                width: MOVE_SWATCH_SIZE,
                height: MOVE_SWATCH_SIZE,
            };
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: swatch_rect,
                radius: 2.0,
                color,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
            let count_text = group.tabs().len().to_string();
            let count_width = measurer.measure_width(
                &count_text,
                tab_bar::PILL_COUNT_FONT,
                tab_bar::PILL_COUNT_FONT_SIZE,
            );
            let name_max_width = (rect.width
                - MOVE_SWATCH_SIZE
                - MOVE_SWATCH_GAP
                - MOVE_ROW_PADDING_X
                - count_width
                - MOVE_SWATCH_GAP)
                .max(0.0);
            let (name, _) = measurer.truncate(
                group.name().unwrap_or_default(),
                BODY_FONT,
                MENU_ITEM_TEXT_SIZE,
                name_max_width,
            );
            out.push(Primitive::Text(TextRun {
                origin: (
                    swatch_rect.x + MOVE_SWATCH_SIZE + MOVE_SWATCH_GAP,
                    rect.y + (rect.height - MENU_ITEM_TEXT_SIZE) / 2.0,
                ),
                text: name,
                font: BODY_FONT,
                size_px: MENU_ITEM_TEXT_SIZE,
                color: palette::MENU_ITEM_TEXT,
            }));
            out.push(Primitive::Text(TextRun {
                origin: (
                    rect.x + rect.width - MOVE_ROW_PADDING_X - count_width,
                    rect.y + (rect.height - tab_bar::PILL_COUNT_FONT_SIZE) / 2.0,
                ),
                text: count_text,
                font: tab_bar::PILL_COUNT_FONT,
                size_px: tab_bar::PILL_COUNT_FONT_SIZE,
                color: palette::PILL_COUNT_TEXT,
            }));
        } else {
            out.push(Primitive::Text(TextRun {
                origin: (
                    rect.x + MOVE_ROW_PADDING_X,
                    rect.y + (rect.height - MENU_ITEM_TEXT_SIZE) / 2.0,
                ),
                text: MOVE_NEW_GROUP_LABEL.to_string(),
                font: BODY_FONT,
                size_px: MENU_ITEM_TEXT_SIZE,
                color: palette::MENU_ITEM_TEXT,
            }));
        }
    }
    out
}

/// Índice absoluto (não relativo à janela visível) da linha sob `point`,
/// se alguma -- mesmo espaço de índice de `MoveToGroupPopover::
/// set_highlight`/`selected`.
pub fn move_to_group_hit(layout: &MoveToGroupLayout, point: (f32, f32)) -> Option<usize> {
    layout
        .visible_row_rects
        .iter()
        .position(|&rect| rect_contains(rect, point))
        .map(|i| layout.first_visible_index + i)
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
