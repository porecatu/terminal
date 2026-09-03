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
//! **Sombra em camadas nos cinco widgets** (F4 etapa 6, ADR-0032 §2):
//! `chrome::push_shadow` -- a mesma técnica de três `RoundedQuad` pretos
//! empilhados que já sombreia a cápsula de grupo, a aba solta e o quadro
//! do terminal, sem primitiva nova em `porecatu-render`. Sem chave de
//! config própria (nenhuma seção `[appearance.*]` destes widgets tem
//! `shadow`, ao contrário de `[appearance.groups]`/`[appearance.
//! terminal_frame]`): a decisão do ADR-0032 é fixa, não alternável.
//! Corpo de aviso e diálogo também não quebra linha (`TextRun` é sempre uma linha);
//! onde a espec pede várias linhas, o texto trunca em uma só com o
//! `TextMeasurer`, em vez do "três linhas com reticências" literal.
//!
//! F4 etapa 2: toda geometria e cor daqui vem de `&porecatu_config::Config`
//! (parâmetro `config`) e `&palette::ResolvedPalette` (parâmetro `pal`),
//! passados por `lib.rs` -- não há um "OverlayStyle" próprio análogo a
//! `TabBarStyle`, porque nenhuma destas funções corre no caminho quente
//! (popover pinta no máximo uma vez por frame, quando visível): ler os
//! sub-structs de `Config` direto é suficiente, sem precisar de um cache
//! de campos "achatados". `style: &TabBarStyle` entra só onde o valor é
//! `icon_em_size` (`[appearance.tabs]`) -- o mesmo token de em de ícone que
//! `chrome.rs` usa, não duplicado aqui.

use porecatu_core::{GroupColor, Workspace};
use porecatu_render::{
    Color, FontFace, Primitive, Quad, Rect, RoundedQuad, SansWeight, TextMeasurer, TextRun, icon,
};

use crate::chrome::{centered_glyph, push_shadow};
use crate::context_menu::{ContextMenu, TAB_MENU_ITEMS};
use crate::dialog::{ConfirmDialog, DialogButton};
use crate::group_editor::{EditorRegion, GroupEditor};
use crate::group_menu::{self, EDITOR_ACTION_ORDER, GroupActionItem, GroupContextMenu};
use crate::move_to_group::MoveToGroupPopover;
use crate::palette::{self, ResolvedPalette};
use crate::tab_bar::{self, TabBarStyle, rect_contains};
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

// ---- aviso do app (espec §2.14, ADR-0014 canal 1, `[appearance.notices]`) ----

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

pub fn layout_warnings(
    stack: &WarningStack,
    config: &porecatu_config::Config,
    bar_height: f32,
    content_width: f32,
) -> WarningLayout {
    let notices = &config.appearance.notices;
    let width = notices.width as f32;
    let padding_x = notices.padding_x as f32;
    let padding_y = notices.padding_y as f32;
    let gap = notices.gap as f32;
    let margin_right = notices.stack_margin_right as f32;
    let margin_top = notices.stack_margin_top as f32;
    let title_size = notices.font_size as f32;
    let line_height = notices.line_height as f32;
    let close_size = notices.close_button_size as f32;

    let x = content_width - margin_right - width;
    let mut y = bar_height + margin_top;
    let mut entries = Vec::new();
    for (index, _) in stack.items().iter().enumerate().rev() {
        let height = padding_y * 2.0 + title_size.max(line_height);
        let rect = Rect {
            x,
            y,
            width,
            height,
        };
        let close_button = Rect {
            x: rect.x + rect.width - padding_x - close_size + 4.0,
            y: rect.y + padding_y - 2.0,
            width: close_size,
            height: close_size,
        };
        entries.push(WarningEntry {
            index,
            rect,
            close_button,
        });
        y += height + gap;
    }
    let stack_rect = entries
        .iter()
        .fold(None, |acc: Option<Rect>, e| {
            Some(acc.map_or(e.rect, |a| union(a, e.rect)))
        })
        .unwrap_or(Rect {
            x,
            y: bar_height + margin_top,
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
    config: &porecatu_config::Config,
    style: &TabBarStyle,
    pal: &ResolvedPalette,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let notices = &config.appearance.notices;
    let corner_radius = notices.corner_radius as f32;
    let severity_bar_width = notices.severity_bar_width as f32;
    let padding_x = notices.padding_x as f32;
    let title_size = notices.font_size as f32;
    let body_size = notices.body_font_size as f32;
    let close_size = notices.close_button_size as f32;
    let line_height = notices.line_height as f32;
    // Em, não desenho -- mesmo tamanho do botão de fechar da aba, que a
    // espec. §2.15 diz ser "o mesmo botão". Ver `porecatu_render::icon`.
    let close_icon_size = style.icon_em_size;

    let mut out = Vec::new();
    for entry in &layout.entries {
        let warning = &stack.items()[entry.index];
        push_shadow(&mut out, entry.rect, corner_radius);
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: entry.rect,
            radius: corner_radius,
            color: pal.popover_background,
            border_color: pal.popover_border,
            border_width: 1.0,
        }));
        let severity_color = match warning.severity {
            Severity::Error => pal.warning_severity_error,
            Severity::Warning => pal.warning_severity_warning,
            Severity::Info => pal.warning_severity_info,
        };
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: entry.rect.x,
                y: entry.rect.y,
                width: severity_bar_width,
                height: entry.rect.height,
            },
            color: severity_color,
        }));

        let text_x = entry.rect.x + severity_bar_width + padding_x;
        let title_y = entry.rect.y + notices.padding_y as f32;
        out.push(Primitive::Text(TextRun {
            origin: (text_x, title_y),
            text: warning.title.clone(),
            font: TITLE_FONT,
            size_px: title_size,
            color: pal.warning_title_text,
        }));

        let body_max_width =
            (entry.rect.width - severity_bar_width - padding_x * 2.0 - close_size).max(0.0);
        let (body, _) = measurer.truncate(&warning.body, BODY_FONT, body_size, body_max_width);
        out.push(Primitive::Text(TextRun {
            origin: (text_x, title_y + line_height),
            text: body,
            font: BODY_FONT,
            size_px: body_size,
            color: pal.warning_body_text,
        }));

        out.push(centered_glyph(
            icon::X,
            entry.close_button,
            close_icon_size,
            pal.chrome_icon,
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

// ---- diálogo de confirmação (espec §2.15, ADR-0014, `[appearance.dialog]`) ----

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
    config: &porecatu_config::Config,
    measurer: &mut TextMeasurer,
) -> DialogLayout {
    let cfg = &config.appearance.dialog;
    let width = cfg.width as f32;
    let padding = cfg.padding as f32;
    let title_size = cfg.title_font_size as f32;
    let body_size = cfg.font_size as f32;
    let gap = cfg.gap as f32;
    let button_height = cfg.button_height as f32;
    let button_gap = cfg.button_gap as f32;
    let button_padding_x = cfg.button_padding_x as f32;

    let cancel_width =
        measurer.measure_width(DIALOG_CANCEL_LABEL, BODY_FONT, body_size) + button_padding_x * 2.0;
    let confirm_width = measurer.measure_width(&dialog.confirm_label, BODY_FONT, body_size)
        + button_padding_x * 2.0;

    let content_height = title_size + gap + body_size + gap + button_height;
    let modal_height = padding * 2.0 + content_height;
    let modal_rect = Rect {
        x: (window_width - width) / 2.0,
        y: (window_height - modal_height) / 2.0,
        width,
        height: modal_height,
    };

    let buttons_y = modal_rect.y + modal_rect.height - padding - button_height;
    let confirm_rect = Rect {
        x: modal_rect.x + modal_rect.width - padding - confirm_width,
        y: buttons_y,
        width: confirm_width,
        height: button_height,
    };
    let cancel_rect = Rect {
        x: confirm_rect.x - button_gap - cancel_width,
        y: buttons_y,
        width: cancel_width,
        height: button_height,
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
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    window_width: f32,
    window_height: f32,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let cfg = &config.appearance.dialog;
    let padding = cfg.padding as f32;
    let title_size = cfg.title_font_size as f32;
    let body_size = cfg.font_size as f32;
    let gap = cfg.gap as f32;
    let corner_radius = cfg.corner_radius as f32;
    let button_corner_radius = cfg.button_corner_radius as f32;

    let mut out = Vec::new();
    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: window_width,
            height: window_height,
        },
        color: pal.dialog_overlay,
    }));
    push_shadow(&mut out, layout.modal_rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.modal_rect,
        radius: corner_radius,
        color: pal.dialog_background,
        border_color: pal.dialog_border,
        border_width: 1.0,
    }));

    let text_x = layout.modal_rect.x + padding;
    let mut y = layout.modal_rect.y + padding;
    out.push(Primitive::Text(TextRun {
        origin: (text_x, y),
        text: dialog.title.clone(),
        font: TITLE_FONT,
        size_px: title_size,
        color: pal.dialog_title_text,
    }));
    y += title_size + gap;
    let body_max_width = layout.modal_rect.width - padding * 2.0;
    let (body, _) = measurer.truncate(&dialog.body, BODY_FONT, body_size, body_max_width);
    out.push(Primitive::Text(TextRun {
        origin: (text_x, y),
        text: body,
        font: BODY_FONT,
        size_px: body_size,
        color: pal.dialog_body_text,
    }));

    paint_dialog_button(
        layout.cancel_rect,
        DIALOG_CANCEL_LABEL,
        palette::TRANSPARENT,
        pal.dialog_cancel_text,
        if dialog.focused() == DialogButton::Cancel {
            pal.dialog_focus_ring
        } else {
            pal.dialog_cancel_border
        },
        body_size,
        button_corner_radius,
        measurer,
        &mut out,
    );
    paint_dialog_button(
        layout.confirm_rect,
        &dialog.confirm_label,
        pal.dialog_confirm_background,
        pal.dialog_confirm_text,
        if dialog.focused() == DialogButton::Confirm {
            pal.dialog_focus_ring
        } else {
            palette::TRANSPARENT
        },
        body_size,
        button_corner_radius,
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
    font_size: f32,
    corner_radius: f32,
    measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: corner_radius,
        color: background,
        border_color,
        border_width: 1.0,
    }));
    let width = measurer.measure_width(label, BODY_FONT, font_size);
    out.push(Primitive::Text(TextRun {
        origin: (
            rect.x + (rect.width - width) / 2.0,
            rect.y + (rect.height - font_size) / 2.0,
        ),
        text: label.to_string(),
        font: BODY_FONT,
        size_px: font_size,
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

// ---- menu de contexto (espec §2.16, ADR-0014, `[appearance.context_menu]`) ----

pub struct MenuLayout {
    pub menu_rect: Rect,
    /// Alinhado por índice com `TAB_MENU_ITEMS`.
    pub item_rects: Vec<Rect>,
}

pub fn layout_context_menu(
    menu: &ContextMenu,
    config: &porecatu_config::Config,
    window_width: f32,
    window_height: f32,
) -> MenuLayout {
    let cfg = &config.appearance.context_menu;
    let width = cfg.width as f32;
    let padding = cfg.padding as f32;
    let item_height = cfg.item_height as f32;

    let height = padding * 2.0 + item_height * TAB_MENU_ITEMS.len() as f32;
    let mut x = menu.anchor.0;
    let mut y = menu.anchor.1;
    // "Vira nos dois eixos para caber na tela" (espec §2.16) -- contra a
    // janela, não o monitor: multi-monitor físico é responsabilidade de
    // `winit`/SO posicionar a própria janela, isto só evita estourar a
    // borda da janela atual.
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    if y + height > window_height {
        y = (window_height - height).max(0.0);
    }
    let menu_rect = Rect {
        x,
        y,
        width,
        height,
    };
    let item_rects = (0..TAB_MENU_ITEMS.len())
        .map(|i| Rect {
            x: menu_rect.x + padding,
            y: menu_rect.y + padding + item_height * i as f32,
            width: menu_rect.width - padding * 2.0,
            height: item_height,
        })
        .collect();
    MenuLayout {
        menu_rect,
        item_rects,
    }
}

pub fn paint_context_menu(
    layout: &MenuLayout,
    menu: &ContextMenu,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
) -> Vec<Primitive> {
    let cfg = &config.appearance.context_menu;
    let corner_radius = cfg.corner_radius as f32;
    let item_radius = cfg.item_corner_radius as f32;
    let item_padding_x = cfg.item_padding_x as f32;
    let font_size = cfg.font_size as f32;

    let mut out = Vec::new();
    push_shadow(&mut out, layout.menu_rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.menu_rect,
        radius: corner_radius,
        color: pal.context_menu_background,
        border_color: pal.context_menu_border,
        border_width: 1.0,
    }));
    for (index, item) in TAB_MENU_ITEMS.iter().enumerate() {
        let rect = layout.item_rects[index];
        if index == menu.highlighted() {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: item_radius,
                color: pal.menu_item_hover,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let color = if item.enabled {
            pal.menu_item_text
        } else {
            pal.menu_item_disabled_text
        };
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + item_padding_x,
                rect.y + (rect.height - font_size) / 2.0,
            ),
            text: item.label.to_string(),
            font: BODY_FONT,
            size_px: font_size,
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
/// estado do grupo). Mesmas chaves de `[appearance.context_menu]`.
pub struct GroupMenuLayout {
    pub menu_rect: Rect,
    pub item_rects: Vec<Rect>,
}

pub fn layout_group_menu(
    menu: &GroupContextMenu,
    item_count: usize,
    config: &porecatu_config::Config,
    window_width: f32,
    window_height: f32,
) -> GroupMenuLayout {
    let cfg = &config.appearance.context_menu;
    let width = cfg.width as f32;
    let padding = cfg.padding as f32;
    let item_height = cfg.item_height as f32;

    let height = padding * 2.0 + item_height * item_count as f32;
    let mut x = menu.anchor.0;
    let mut y = menu.anchor.1;
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    if y + height > window_height {
        y = (window_height - height).max(0.0);
    }
    let menu_rect = Rect {
        x,
        y,
        width,
        height,
    };
    let item_rects = (0..item_count)
        .map(|i| Rect {
            x: menu_rect.x + padding,
            y: menu_rect.y + padding + item_height * i as f32,
            width: menu_rect.width - padding * 2.0,
            height: item_height,
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
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
) -> Vec<Primitive> {
    let cfg = &config.appearance.context_menu;
    let corner_radius = cfg.corner_radius as f32;
    let item_radius = cfg.item_corner_radius as f32;
    let item_padding_x = cfg.item_padding_x as f32;
    let font_size = cfg.font_size as f32;

    let mut out = Vec::new();
    push_shadow(&mut out, layout.menu_rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.menu_rect,
        radius: corner_radius,
        color: pal.context_menu_background,
        border_color: pal.context_menu_border,
        border_width: 1.0,
    }));
    for (index, item) in items.iter().enumerate() {
        let rect = layout.item_rects[index];
        if index == highlighted {
            let hover_color = if item.destructive {
                pal.menu_item_destructive_hover
            } else {
                pal.menu_item_hover
            };
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: item_radius,
                color: hover_color,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let text_color = if item.destructive {
            pal.menu_item_destructive_text
        } else {
            pal.menu_item_text
        };
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + item_padding_x,
                rect.y + (rect.height - font_size) / 2.0,
            ),
            text: item.label.clone(),
            font: BODY_FONT,
            size_px: font_size,
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

// ---- editor de grupo (espec §2.10, ADR-0023, `[appearance.group_editor]`) ----

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
/// nasce `offset_y` abaixo dela; com `tab_bar_position = "bottom"` ele
/// abriria acima, mas essa chave é lida por `chrome.rs`/`lib.rs`, não
/// aqui -- só o caso `top` (o único que `layout_group_editor` recebe hoje)
/// está implementado.
pub fn layout_group_editor(
    anchor_x: f32,
    bar_bottom_y: f32,
    config: &porecatu_config::Config,
    window_width: f32,
    window_height: f32,
) -> GroupEditorLayout {
    let cfg = &config.appearance.group_editor;
    let width = cfg.width as f32;
    let padding = cfg.padding as f32;
    let gap = cfg.gap as f32;
    let offset_y = cfg.offset_y as f32;
    let section_font_size = cfg.section_font_size as f32;
    let section_caption_gap = cfg.section_caption_gap as f32;
    let input_height = cfg.input_height as f32;
    let swatch_size = cfg.swatch_size as f32;
    let swatch_gap = cfg.swatch_gap as f32;
    let divider_height = cfg.divider_height as f32;
    let item_height = cfg.item_height as f32;

    let action_count = EDITOR_ACTION_ORDER.len();
    let content_height = section_font_size
        + section_caption_gap
        + input_height
        + gap
        + section_font_size
        + section_caption_gap
        + swatch_size
        + gap
        + divider_height
        + gap
        + item_height * action_count as f32;
    let popover_height = padding * 2.0 + content_height;

    let mut x = anchor_x;
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    let mut y = bar_bottom_y + offset_y;
    if y + popover_height > window_height {
        y = (window_height - popover_height).max(0.0);
    }
    let popover_rect = Rect {
        x,
        y,
        width,
        height: popover_height,
    };

    let inner_x = popover_rect.x + padding;
    let inner_width = popover_rect.width - padding * 2.0;
    let mut cursor_y = popover_rect.y + padding;

    let name_caption_origin = (inner_x, cursor_y);
    cursor_y += section_font_size + section_caption_gap;
    let name_input_rect = Rect {
        x: inner_x,
        y: cursor_y,
        width: inner_width,
        height: input_height,
    };
    cursor_y += input_height + gap;

    let color_caption_origin = (inner_x, cursor_y);
    cursor_y += section_font_size + section_caption_gap;
    let mut swatch_rects = Vec::with_capacity(GroupColor::ALL.len());
    let mut sx = inner_x;
    for _ in GroupColor::ALL {
        swatch_rects.push(Rect {
            x: sx,
            y: cursor_y,
            width: swatch_size,
            height: swatch_size,
        });
        sx += swatch_size + swatch_gap;
    }
    cursor_y += swatch_size + gap;

    let divider_rect = Rect {
        x: inner_x,
        y: cursor_y,
        width: inner_width,
        height: divider_height,
    };
    cursor_y += divider_height + gap;

    let action_rects = (0..action_count)
        .map(|i| Rect {
            x: inner_x,
            y: cursor_y + item_height * i as f32,
            width: inner_width,
            height: item_height,
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
/// A largura/raio dos itens de ação e o tamanho de fonte deles reaproveitam
/// `[appearance.context_menu]` (`item_corner_radius`/`item_padding_x`/
/// `font_size`) -- `GroupEditor` não tem chave própria pra isso, mesma
/// anatomia de lista que o menu de contexto.
#[allow(clippy::too_many_arguments)]
pub fn paint_group_editor(
    layout: &GroupEditorLayout,
    editor: &GroupEditor,
    current_color_index: usize,
    is_collapsed: bool,
    tab_count: usize,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let cfg = &config.appearance.group_editor;
    let menu_cfg = &config.appearance.context_menu;
    let corner_radius = cfg.corner_radius as f32;
    let section_font_size = cfg.section_font_size as f32;
    let input_corner_radius = cfg.input_corner_radius as f32;
    let input_font_size = cfg.input_font_size as f32;
    let input_padding_x = cfg.input_padding_x as f32;
    let swatch_corner_radius = cfg.swatch_corner_radius as f32;
    let swatch_border_width = cfg.swatch_border_width as f32;
    let swatch_highlight_pad = cfg.swatch_highlight_pad as f32;
    let item_radius = menu_cfg.item_corner_radius as f32;
    let item_padding_x = menu_cfg.item_padding_x as f32;
    let item_text_size = menu_cfg.font_size as f32;

    let mut out = Vec::new();
    push_shadow(&mut out, layout.popover_rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.popover_rect,
        radius: corner_radius,
        color: pal.editor_background,
        border_color: pal.editor_border,
        border_width: 1.0,
    }));

    out.push(Primitive::Text(TextRun {
        origin: layout.name_caption_origin,
        text: EDITOR_SECTION_GROUP_LABEL.to_string(),
        font: TITLE_FONT,
        size_px: section_font_size,
        color: pal.editor_section_text,
    }));
    let focused_name = editor.focus() == EditorRegion::Name;
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.name_input_rect,
        radius: input_corner_radius,
        color: pal.editor_input_background,
        border_color: if focused_name {
            pal.editor_input_border_focus
        } else {
            pal.editor_input_border
        },
        border_width: 1.0,
    }));
    let buffer = editor.name_buffer();
    let available_text_width = (layout.name_input_rect.width - input_padding_x * 2.0).max(0.0);
    let text_width = measurer.measure_width(buffer, BODY_FONT, input_font_size);
    let text_x = if text_width > available_text_width {
        layout.name_input_rect.x + input_padding_x - (text_width - available_text_width)
    } else {
        layout.name_input_rect.x + input_padding_x
    };
    let text_y = layout.name_input_rect.y + (layout.name_input_rect.height - input_font_size) / 2.0;
    out.push(Primitive::PushClip(layout.name_input_rect));
    out.push(Primitive::Text(TextRun {
        origin: (text_x, text_y),
        text: buffer.to_string(),
        font: BODY_FONT,
        size_px: input_font_size,
        color: pal.editor_input_text,
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
            color: pal.editor_input_text,
        }));
    }
    out.push(Primitive::PopClip);

    out.push(Primitive::Text(TextRun {
        origin: layout.color_caption_origin,
        text: EDITOR_SECTION_COLOR_LABEL.to_string(),
        font: TITLE_FONT,
        size_px: section_font_size,
        color: pal.editor_section_text,
    }));
    for (index, color) in GroupColor::ALL.iter().enumerate() {
        let rect = layout.swatch_rects[index];
        let is_highlighted =
            editor.focus() == EditorRegion::Swatches && editor.swatch_highlight() == index;
        if is_highlighted {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: expand(rect, swatch_highlight_pad),
                radius: swatch_corner_radius + swatch_highlight_pad,
                color: pal.editor_item_hover_background,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let is_current = index == current_color_index;
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect,
            radius: swatch_corner_radius,
            color: pal.group_color(*color),
            border_color: if is_current {
                pal.editor_swatch_ring
            } else {
                palette::TRANSPARENT
            },
            border_width: swatch_border_width,
        }));
    }

    out.push(Primitive::Quad(Quad {
        rect: layout.divider_rect,
        color: pal.editor_divider,
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
                pal.editor_destructive_hover_background
            } else {
                pal.editor_item_hover_background
            };
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: item_radius,
                color: hover,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let text_color = if item.destructive {
            pal.editor_destructive_foreground
        } else {
            pal.editor_item_foreground
        };
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + item_padding_x,
                rect.y + (rect.height - item_text_size) / 2.0,
            ),
            text: item.label.clone(),
            font: BODY_FONT,
            size_px: item_text_size,
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
// `[appearance.move_to_group]` -- só geometria; cor reaproveita
// `[appearance.context_menu]` (comentário do TOML: "width = context_menu.
// width", "row_height = context_menu.item_height").

const MOVE_NEW_GROUP_LABEL: &str = "Novo grupo";
/// Raio do swatch pequeno do popover -- menor que o das outras superfícies
/// de cor (28px no editor, 6px de raio); sem chave própria, valor de
/// trabalho já existente antes desta etapa.
const MOVE_SWATCH_RADIUS: f32 = 2.0;

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
    config: &porecatu_config::Config,
    window_width: f32,
    window_height: f32,
) -> MoveToGroupLayout {
    let cfg = &config.appearance.move_to_group;
    let width = cfg.width as f32;
    let row_height = cfg.row_height as f32;
    let max_visible_rows = cfg.max_visible_rows as usize;
    let padding = config.appearance.context_menu.padding as f32;

    let total_rows = popover.row_count();
    let visible_rows = total_rows.min(max_visible_rows);
    let max_first = total_rows.saturating_sub(visible_rows);
    let highlighted = popover.highlighted();
    let first_visible_index = highlighted
        .saturating_sub(visible_rows.saturating_sub(1))
        .min(max_first);

    let height = padding * 2.0 + row_height * visible_rows as f32;
    let mut x = popover.anchor.0;
    let mut y = popover.anchor.1;
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    if y + height > window_height {
        y = (window_height - height).max(0.0);
    }
    let popover_rect = Rect {
        x,
        y,
        width,
        height,
    };

    let visible_row_rects = (0..visible_rows)
        .map(|i| Rect {
            x: popover_rect.x + padding,
            y: popover_rect.y + padding + row_height * i as f32,
            width: popover_rect.width - padding * 2.0,
            height: row_height,
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
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let move_cfg = &config.appearance.move_to_group;
    let menu_cfg = &config.appearance.context_menu;
    let corner_radius = menu_cfg.corner_radius as f32;
    let item_radius = menu_cfg.item_corner_radius as f32;
    let row_padding_x = move_cfg.row_padding_x as f32;
    let swatch_size = move_cfg.swatch_size as f32;
    let swatch_gap = move_cfg.swatch_gap as f32;
    let item_text_size = menu_cfg.font_size as f32;

    let mut out = Vec::new();
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.popover_rect,
        radius: corner_radius,
        color: pal.context_menu_background,
        border_color: pal.context_menu_border,
        border_width: 1.0,
    }));
    for (visible_i, &rect) in layout.visible_row_rects.iter().enumerate() {
        let row_index = layout.first_visible_index + visible_i;
        if row_index == popover.highlighted() {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: item_radius,
                color: pal.menu_item_hover,
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
                .map(|c| pal.group_color(c))
                .unwrap_or_else(|| pal.ungrouped_group_color());
            let swatch_rect = Rect {
                x: rect.x,
                y: rect.y + (rect.height - swatch_size) / 2.0,
                width: swatch_size,
                height: swatch_size,
            };
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: swatch_rect,
                radius: MOVE_SWATCH_RADIUS,
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
            let name_max_width =
                (rect.width - swatch_size - swatch_gap - row_padding_x - count_width - swatch_gap)
                    .max(0.0);
            let (name, _) = measurer.truncate(
                group.name().unwrap_or_default(),
                BODY_FONT,
                item_text_size,
                name_max_width,
            );
            out.push(Primitive::Text(TextRun {
                origin: (
                    swatch_rect.x + swatch_size + swatch_gap,
                    rect.y + (rect.height - item_text_size) / 2.0,
                ),
                text: name,
                font: BODY_FONT,
                size_px: item_text_size,
                color: pal.menu_item_text,
            }));
            out.push(Primitive::Text(TextRun {
                origin: (
                    rect.x + rect.width - row_padding_x - count_width,
                    rect.y + (rect.height - tab_bar::PILL_COUNT_FONT_SIZE) / 2.0,
                ),
                text: count_text,
                font: tab_bar::PILL_COUNT_FONT,
                size_px: tab_bar::PILL_COUNT_FONT_SIZE,
                color: pal.pill_count_text,
            }));
        } else {
            out.push(Primitive::Text(TextRun {
                origin: (
                    rect.x + row_padding_x,
                    rect.y + (rect.height - item_text_size) / 2.0,
                ),
                text: MOVE_NEW_GROUP_LABEL.to_string(),
                font: BODY_FONT,
                size_px: item_text_size,
                color: pal.menu_item_text,
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

// ---- tooltip (espec §2.20, ADR-0019, `[appearance.tooltip]`) ----

pub fn paint_tooltip(
    anchor: Rect,
    text: &str,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    window_width: f32,
    window_height: f32,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let cfg = &config.appearance.tooltip;
    let max_width = cfg.max_width as f32;
    let padding_x = cfg.padding_x as f32;
    let padding_y = cfg.padding_y as f32;
    let text_size = cfg.font_size as f32;
    let gap = cfg.gap as f32;
    let corner_radius = cfg.corner_radius as f32;

    let available = (max_width - padding_x * 2.0).max(0.0);
    let (label, _) = measurer.truncate(text, BODY_FONT, text_size, available);
    let text_width = measurer.measure_width(&label, BODY_FONT, text_size);
    let width = text_width + padding_x * 2.0;
    let height = text_size + padding_y * 2.0;

    let mut x = anchor.x;
    let mut y = anchor.y + anchor.height + gap;
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    if y + height > window_height {
        y = anchor.y - gap - height;
    }
    let rect = Rect {
        x,
        y,
        width,
        height,
    };

    let mut out = Vec::new();
    push_shadow(&mut out, rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: corner_radius,
        color: pal.tooltip_background,
        border_color: pal.tooltip_border,
        border_width: 1.0,
    }));
    out.push(Primitive::Text(TextRun {
        origin: (rect.x + padding_x, rect.y + padding_y),
        text: label,
        font: BODY_FONT,
        size_px: text_size,
        color: pal.tooltip_text,
    }));
    out
}
