// SPDX-License-Identifier: GPL-3.0-or-later

//! Pintura do corpo da janela de configurações: a guia, o painel (título,
//! seções, linhas e controles) e o rodapé (ADR-0060 §1 a §4). Só lê: a
//! geometria vem de `layout.rs`, o texto já cortado e medido de `content.rs`,
//! e nada aqui mede texto exceto o espaçamento do rótulo de seção, que passa
//! pelo cache de avanço do `TextMeasurer`.
//!
//! Tudo na camada `Chrome` (ADR-0060 §4); o tooltip, na `Popover`, é de quem
//! chama. Sem animação (ADR-0022).

use porecatu_config::Config;
use porecatu_locale::Catalog;
use porecatu_render::{
    Color, FontFace, Primitive, Quad, Rect, RoundedQuad, TextMeasurer, TextRun, icon,
};

use super::content::{Block, Content, ControlView, RowView, SWATCH_COUNT, SWATCH_GAP};
use super::layout::{
    BlockGeometry, FOOTER_BUTTONS, Focus, FooterButton, Hit, Layout, Metrics, RowGeometry,
};
use super::{Group, ROW_BACKGROUND, TOGGLE_OFF, TOGGLE_ON};
use crate::chrome::{ICON_FONT, centered_glyph};
use crate::messages::msg;
use crate::overlay::{BODY_FONT, TITLE_FONT};
use crate::palette::{self, ResolvedPalette};
use crate::tab_bar::TabBarStyle;
use crate::toggle::{TOGGLE_KNOB_COLOR, push_toggle};

/// Espaçamento entre letras do rótulo de seção: `letter-spacing: .8px`
/// (ADR-0060 §2).
const SECTION_LETTER_SPACING: f32 = 0.8;

/// Separador de 1px entre as opções e o rodapé: o "separador de barra"
/// `#23272f` (ADR-0060 §1, espec. §1.3). O binário não o pinta em nenhuma barra
/// e `ResolvedPalette` não o carrega.
const FOOTER_SEPARATOR: Color = palette::hex(0x23, 0x27, 0x2f);

/// Tudo o que a pintura do corpo lê.
pub(crate) struct Input<'a> {
    pub layout: &'a Layout,
    pub metrics: &'a Metrics,
    pub content: &'a Content,
    pub items: &'a [(Group, Rect)],
    pub footer: &'a [(FooterButton, Rect)],
    pub selected: Group,
    pub focus: Focus,
    pub hovered: Option<Hit>,
    pub scroll: f32,
    /// Grupos com alteração pendente (o ponto ao lado do nome); ainda nenhum.
    pub pending_groups: &'a [Group],
    /// Quais botões do rodapé estão disponíveis, na ordem de `FOOTER_BUTTONS`.
    pub footer_available: [bool; 3],
    pub style: &'a TabBarStyle,
    pub pal: &'a ResolvedPalette,
    pub config: &'a Config,
    pub catalog: &'a Catalog,
}

fn quad(rect: Rect, color: Color) -> Primitive {
    Primitive::Quad(Quad { rect, color })
}

fn rounded(rect: Rect, radius: f32, color: Color, border: Color, border_width: f32) -> Primitive {
    Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius,
        color,
        border_color: border,
        border_width,
    })
}

/// O anel de foco do teclado: `1px` no acento do diálogo (ADR-0060 §3).
fn ring(rect: Rect, radius: f32, pal: &ResolvedPalette) -> Primitive {
    rounded(
        rect,
        radius,
        palette::TRANSPARENT,
        pal.dialog_focus_ring,
        1.0,
    )
}

fn text(origin: (f32, f32), text: &str, font: FontFace, size_px: f32, color: Color) -> Primitive {
    Primitive::Text(TextRun {
        origin,
        text: text.to_owned(),
        font,
        size_px,
        color,
    })
}

/// Origem de um texto de `size_px` centrado na vertical de `rect`.
fn centered_y(rect: Rect, size_px: f32) -> f32 {
    rect.y + (rect.height - size_px) / 2.0
}

fn shift(rect: Rect, dx: f32, dy: f32) -> Rect {
    Rect {
        x: rect.x + dx,
        y: rect.y + dy,
        ..rect
    }
}

/// Guia, painel e rodapé, nessa ordem.
pub(crate) fn paint_body(input: &Input<'_>, measurer: &mut TextMeasurer) -> Vec<Primitive> {
    let mut out = Vec::new();
    paint_sidebar(input, &mut out);
    paint_panel(input, measurer, &mut out);
    paint_footer(input, &mut out);
    out
}

// ---- guia

fn paint_sidebar(input: &Input<'_>, out: &mut Vec<Primitive>) {
    let m = input.metrics;
    let pal = input.pal;
    let menu = &input.config.appearance.context_menu;
    let radius = menu.item_corner_radius as f32;
    let padding_x = menu.item_padding_x as f32;
    for (group, rect) in input.items {
        let selected = *group == input.selected;
        let hovered = input.hovered == Some(Hit::Group(*group));
        if selected {
            out.push(rounded(
                *rect,
                radius,
                pal.tab_active_background,
                palette::TRANSPARENT,
                0.0,
            ));
        } else if hovered {
            out.push(rounded(
                *rect,
                radius,
                pal.menu_item_hover,
                palette::TRANSPARENT,
                0.0,
            ));
        }
        let color = if selected {
            pal.tab_active_text
        } else {
            pal.menu_item_text
        };
        out.push(text(
            (rect.x + padding_x, centered_y(*rect, m.name_size)),
            &group.label(input.catalog),
            BODY_FONT,
            m.name_size,
            color,
        ));
        // O marcador de pendente: o ponto 6×6 do indicador da aba, em Acento,
        // à direita do nome.
        if input.pending_groups.contains(group) {
            out.push(rounded(
                Rect {
                    x: rect.x + rect.width - padding_x - m.dot_size,
                    y: rect.y + (rect.height - m.dot_size) / 2.0,
                    width: m.dot_size,
                    height: m.dot_size,
                },
                m.dot_size / 2.0,
                pal.dialog_focus_ring,
                palette::TRANSPARENT,
                0.0,
            ));
        }
        if selected && input.focus == Focus::Sidebar {
            out.push(ring(*rect, radius, pal));
        }
    }
}

// ---- painel

fn paint_panel(input: &Input<'_>, measurer: &mut TextMeasurer, out: &mut Vec<Primitive>) {
    let m = input.metrics;
    let pal = input.pal;
    let body = input.layout.panel_body;
    let (dx, dy) = (body.x, body.y - input.scroll);

    out.push(Primitive::PushClip(body));

    let title = input.content.geometry.title_origin;
    out.push(text(
        (title.0 + dx, title.1 + dy),
        &input.content.title,
        TITLE_FONT,
        m.title_size,
        pal.dialog_title_text,
    ));

    for (index, (block, geometry)) in input
        .content
        .blocks
        .iter()
        .zip(&input.content.geometry.blocks)
        .enumerate()
    {
        match (block, geometry) {
            (Block::Section(label), BlockGeometry::Section { label_origin }) => {
                paint_section_label(
                    label,
                    (label_origin.0 + dx, label_origin.1 + dy),
                    m.section_size,
                    pal.editor_section_text,
                    measurer,
                    out,
                );
            }
            (Block::Row(row), BlockGeometry::Row(geometry)) => {
                let rect = shift(geometry.rect, dx, dy);
                // Fora do corpo visível não pinta: a rolagem já corta, e
                // pular a linha poupa os primitivos dela.
                if rect.y + rect.height < body.y || rect.y > body.y + body.height {
                    continue;
                }
                paint_row(input, index, row, geometry, dx, dy, out);
            }
            _ => unreachable!("conteúdo e geometria saem do mesmo laço"),
        }
    }

    out.push(Primitive::PopClip);
}

/// Rótulo de seção em caixa alta com `letter-spacing: .8px`: um `TextRun` por
/// caractere, avançando pelo cache de avanço do `TextMeasurer`
/// (`advance_em`) -- nunca remedindo o texto inteiro.
fn paint_section_label(
    label: &str,
    origin: (f32, f32),
    size_px: f32,
    color: Color,
    measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let mut x = origin.0;
    for ch in label.chars() {
        let advance = measurer.advance_em(ch, TITLE_FONT) * size_px;
        if !ch.is_whitespace() {
            out.push(text(
                (x, origin.1),
                &ch.to_string(),
                TITLE_FONT,
                size_px,
                color,
            ));
        }
        x += advance + SECTION_LETTER_SPACING;
    }
}

fn paint_row(
    input: &Input<'_>,
    index: usize,
    row: &RowView,
    geometry: &RowGeometry,
    dx: f32,
    dy: f32,
    out: &mut Vec<Primitive>,
) {
    let m = input.metrics;
    let pal = input.pal;
    let rect = shift(geometry.rect, dx, dy);
    let chosen_theme = matches!(row.control, ControlView::Themes { selected: true, .. });
    // O tema escolhido ganha a borda de Acento (ADR-0060 §3).
    let border = if chosen_theme {
        pal.dialog_focus_ring
    } else {
        pal.dialog_cancel_border
    };
    out.push(rounded(rect, m.row_radius, ROW_BACKGROUND, border, 1.0));

    let name_origin = (geometry.name_origin.0 + dx, geometry.name_origin.1 + dy);
    out.push(text(
        name_origin,
        &row.name,
        BODY_FONT,
        m.name_size,
        pal.menu_item_text,
    ));
    if let Some((scope, x)) = &row.scope {
        out.push(text(
            (
                name_origin.0 + x,
                name_origin.1 + (m.name_size - m.description_size) / 2.0,
            ),
            scope,
            BODY_FONT,
            m.description_size,
            pal.status_bar_stale_cwd,
        ));
    }
    if !row.description.is_empty() {
        out.push(text(
            (
                geometry.description_origin.0 + dx,
                geometry.description_origin.1 + dy,
            ),
            &row.description,
            BODY_FONT,
            m.description_size,
            pal.editor_section_text,
        ));
    }

    paint_control(input, &row.control, shift(geometry.control, dx, dy), out);

    if input.focus == Focus::Row(index) {
        out.push(ring(rect, m.row_radius, pal));
    }
}

// ---- controles (só leitura nesta etapa)

fn paint_control(input: &Input<'_>, control: &ControlView, rect: Rect, out: &mut Vec<Primitive>) {
    let m = input.metrics;
    match control {
        ControlView::Toggle { on } => push_toggle(rect, *on, TOGGLE_ON, TOGGLE_OFF, out),
        ControlView::Field {
            text: value,
            right_aligned,
            text_width,
            ..
        } => {
            paint_field_box(input, rect, out);
            let x = if *right_aligned {
                rect.x + rect.width - m.field_padding_x - text_width
            } else {
                rect.x + m.field_padding_x
            };
            paint_field_text(input, value, x, rect, out);
        }
        ControlView::Segmented {
            labels,
            widths,
            selected,
        } => paint_segmented(input, labels, widths, *selected, rect, out),
        ControlView::Choice { text: value } => {
            paint_field_box(input, rect, out);
            paint_field_text(input, value, rect.x + m.field_padding_x, rect, out);
            paint_caret(input, rect, out);
        }
        ControlView::List { items, add_label } => paint_list(input, items, add_label, rect, out),
        ControlView::Themes { colors, .. } => {
            for (index, color) in colors.iter().enumerate().take(SWATCH_COUNT) {
                let x = rect.x + index as f32 * (m.swatch_size + SWATCH_GAP);
                out.push(rounded(
                    Rect {
                        x,
                        y: rect.y,
                        width: m.swatch_size,
                        height: m.swatch_size,
                    },
                    3.0,
                    *color,
                    palette::TRANSPARENT,
                    0.0,
                ));
            }
        }
        ControlView::GitPoll {
            on,
            seconds,
            text_width,
        } => {
            let toggle = Rect {
                x: rect.x,
                y: rect.y + (rect.height - crate::toggle::TOGGLE_TRACK_HEIGHT) / 2.0,
                width: crate::toggle::TOGGLE_TRACK_WIDTH,
                height: crate::toggle::TOGGLE_TRACK_HEIGHT,
            };
            push_toggle(toggle, *on, TOGGLE_ON, TOGGLE_OFF, out);
            let number = Rect {
                x: rect.x + rect.width - m.number_field_width,
                width: m.number_field_width,
                ..rect
            };
            paint_field_box(input, number, out);
            // O número vem alinhado à direita, como todo campo numérico.
            let x = number.x + number.width - m.field_padding_x - text_width;
            paint_field_text(input, seconds, x, number, out);
        }
    }
}

/// O campo de texto: fundo, borda e raio do editor de grupo (ADR-0060 §3).
fn paint_field_box(input: &Input<'_>, rect: Rect, out: &mut Vec<Primitive>) {
    let pal = input.pal;
    out.push(rounded(
        rect,
        input.metrics.field_radius,
        pal.editor_input_background,
        pal.editor_input_border,
        1.0,
    ));
}

fn paint_field_text(input: &Input<'_>, value: &str, x: f32, rect: Rect, out: &mut Vec<Primitive>) {
    let m = input.metrics;
    let inner = Rect {
        x: rect.x + m.field_padding_x,
        y: rect.y,
        width: (rect.width - m.field_padding_x * 2.0).max(0.0),
        height: rect.height,
    };
    out.push(Primitive::PushClip(inner));
    out.push(text(
        (x, centered_y(rect, m.field_font_size)),
        value,
        BODY_FONT,
        m.field_font_size,
        input.pal.editor_input_text,
    ));
    out.push(Primitive::PopClip);
}

/// O caret do botão de escolha, à direita do campo (ADR-0060 §3).
fn paint_caret(input: &Input<'_>, rect: Rect, out: &mut Vec<Primitive>) {
    let style = input.style;
    let width = style.icon_button_width(style.pill_caret_size);
    let caret = Rect {
        x: rect.x + rect.width - width - input.metrics.field_padding_x / 2.0,
        y: rect.y + (rect.height - style.pill_caret_size) / 2.0,
        width,
        height: style.pill_caret_size,
    };
    out.push(centered_glyph(
        icon::CHEVRON_DOWN,
        caret,
        style.icon_em_size,
        input.pal.chrome_icon,
    ));
}

/// Escolha de até três valores: os botões do diálogo colados, o escolhido com
/// o fundo da aba ativa (ADR-0060 §3). O raio de 5 fica só nas pontas: a
/// moldura inteira leva o raio e o escolhido de uma ponta o herda; um do meio
/// é reto.
fn paint_segmented(
    input: &Input<'_>,
    labels: &[String],
    widths: &[f32],
    selected: usize,
    rect: Rect,
    out: &mut Vec<Primitive>,
) {
    let m = input.metrics;
    let pal = input.pal;
    out.push(rounded(
        rect,
        m.button_radius,
        palette::TRANSPARENT,
        pal.dialog_cancel_border,
        1.0,
    ));
    let mut x = rect.x;
    for (index, (label, width)) in labels.iter().zip(widths).enumerate() {
        let segment = Rect {
            x,
            y: rect.y,
            width: *width,
            height: rect.height,
        };
        let is_end = index == 0 || index + 1 == labels.len();
        if index == selected {
            out.push(rounded(
                segment,
                if is_end { m.button_radius } else { 0.0 },
                pal.tab_active_background,
                pal.tab_active_border,
                1.0,
            ));
        } else if index > 0 && index != selected && index - 1 != selected {
            out.push(quad(
                Rect {
                    x,
                    y: rect.y + 1.0,
                    width: 1.0,
                    height: rect.height - 2.0,
                },
                pal.dialog_cancel_border,
            ));
        }
        out.push(text(
            (x + m.button_padding_x, centered_y(rect, m.button_font_size)),
            label,
            BODY_FONT,
            m.button_font_size,
            if index == selected {
                pal.tab_active_text
            } else {
                pal.dialog_cancel_text
            },
        ));
        x += width;
    }
}

/// Lista editável, inerte: um campo por item com o `X` à direita, e o item
/// "Adicionar" -- ícone `PLUS` e texto de item de menu -- logo abaixo
/// (ADR-0060 §3).
fn paint_list(
    input: &Input<'_>,
    items: &[String],
    add_label: &str,
    rect: Rect,
    out: &mut Vec<Primitive>,
) {
    let m = input.metrics;
    let pal = input.pal;
    let style = input.style;
    let mut y = rect.y;
    for item in items {
        let field = Rect {
            x: rect.x,
            y,
            width: m.text_field_width,
            height: m.field_height,
        };
        paint_field_box(input, field, out);
        paint_field_text(input, item, field.x + m.field_padding_x, field, out);
        let close = Rect {
            x: field.x + field.width + m.list_gap,
            y: y + (m.field_height - m.restore_height) / 2.0,
            width: m.restore_width,
            height: m.restore_height,
        };
        out.push(Primitive::Text(TextRun {
            origin: icon::X.centered_origin(close, style.icon_em_size),
            text: icon::X.glyph.to_string(),
            font: ICON_FONT,
            size_px: style.icon_em_size,
            color: pal.chrome_icon,
        }));
        y += m.field_height + m.list_gap;
    }
    let add = Rect {
        x: rect.x,
        y,
        width: m.text_field_width,
        height: m.sidebar_item_height,
    };
    let plus = Rect {
        x: add.x + m.field_padding_x,
        y: add.y,
        width: style.icon_button_width(style.pill_caret_size),
        height: add.height,
    };
    out.push(centered_glyph(
        icon::PLUS,
        plus,
        style.icon_em_size,
        pal.menu_item_text,
    ));
    out.push(text(
        (
            plus.x + plus.width + m.row_gap,
            centered_y(add, m.name_size),
        ),
        add_label,
        BODY_FONT,
        m.name_size,
        pal.menu_item_text,
    ));
}

// ---- rodapé

fn paint_footer(input: &Input<'_>, out: &mut Vec<Primitive>) {
    let m = input.metrics;
    let pal = input.pal;
    let footer = input.layout.footer;
    out.push(quad(
        Rect {
            height: 1.0_f32.min(footer.height),
            ..footer
        },
        FOOTER_SEPARATOR,
    ));
    for (index, (button, rect)) in input.footer.iter().enumerate() {
        let available = input.footer_available[index];
        let hovered = available && input.hovered == Some(Hit::Footer(*button));
        let label = match button {
            FooterButton::OpenFile => msg::settings::button::open_file(input.catalog),
            FooterButton::Discard => msg::settings::button::discard(input.catalog),
            FooterButton::Save => msg::settings::button::save(input.catalog),
        };
        let primary = *button == FooterButton::Save;
        let (fill, border, color) = match (primary, available) {
            // Salvar, o primeiro botão primário do app: o par "ligado" da
            // alternância, com hover por brilho (ADR-0060 §4).
            (true, true) => {
                let fill = if hovered {
                    crate::chrome::brighten(TOGGLE_ON, input.style.tab_hover_brightness)
                } else {
                    TOGGLE_ON
                };
                (fill, palette::TRANSPARENT, TOGGLE_KNOB_COLOR)
            }
            // Indisponível: texto esmaecido e sem fundo, como item de menu
            // indisponível.
            (true, false) => (
                palette::TRANSPARENT,
                palette::TRANSPARENT,
                pal.menu_item_disabled_text,
            ),
            // Estilo cancelar: borda e texto do botão do diálogo, hover no
            // tom da borda.
            (false, true) => (
                if hovered {
                    pal.dialog_cancel_border
                } else {
                    palette::TRANSPARENT
                },
                pal.dialog_cancel_border,
                pal.dialog_cancel_text,
            ),
            (false, false) => (
                palette::TRANSPARENT,
                pal.dialog_cancel_border,
                pal.menu_item_disabled_text,
            ),
        };
        out.push(rounded(*rect, m.button_radius, fill, border, 1.0));
        out.push(text(
            (
                rect.x + m.button_padding_x,
                centered_y(*rect, m.button_font_size),
            ),
            &label,
            BODY_FONT,
            m.button_font_size,
            color,
        ));
        if input.focus == Focus::Footer(*button) {
            out.push(ring(*rect, m.button_radius, pal));
        }
    }
    debug_assert_eq!(input.footer.len(), FOOTER_BUTTONS.len());
}
