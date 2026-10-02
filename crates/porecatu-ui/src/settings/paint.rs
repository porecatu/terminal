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

use super::content::{
    Block, CHIP_FONT, ChipTone, ChipView, Content, ControlView, ListItemView, NoteTone, RowView,
    SWATCH_COUNT, SWATCH_GAP,
};
use super::field_edit::{EditPart, Editing};
use super::layout::{
    BlockGeometry, ControlPart, FOOTER_BUTTONS, Focus, FooterButton, Hit, Layout, Metrics,
    RowGeometry, list_geometry,
};
use super::{
    CHIP_BACKGROUND, CHIP_BORDER, Group, RESTORE_HOVER_BACKGROUND, RESTORE_HOVER_ICON,
    RESTORE_ICON, RESTORE_RADIUS, ROW_BACKGROUND, TOGGLE_OFF, TOGGLE_ON,
};
use crate::chrome::centered_glyph;
use crate::messages::msg;
use crate::overlay::{BODY_FONT, TITLE_FONT};
use crate::palette::{self, ResolvedPalette};
use crate::tab_bar::{TabBarStyle, scrolled_text_x};
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
    /// Grupos com alteração pendente: o ponto ao lado do nome.
    pub pending_groups: &'a [Group],
    /// O campo em edição, se algum.
    pub editing: Option<&'a Editing>,
    /// Cor do fundo da seleção de texto de um campo em edição.
    pub selection_color: Color,
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
                paint_row(input, index, row, geometry, dx, dy, measurer, out);
            }
            (Block::Filter { text, placeholder }, BlockGeometry::Filter { rect }) => {
                let rect = shift(*rect, dx, dy);
                if rect.y + rect.height < body.y || rect.y > body.y + body.height {
                    continue;
                }
                paint_filter(input, index, rect, (text, placeholder), measurer, out);
            }
            (
                Block::Note { lines, tone },
                BlockGeometry::Note {
                    origin,
                    line_height,
                },
            ) => {
                let color = match tone {
                    NoteTone::Warning => pal.warning_severity_warning,
                    NoteTone::Muted => pal.status_bar_stale_cwd,
                };
                for (line_index, line) in lines.iter().enumerate() {
                    out.push(text(
                        (
                            origin.0 + dx,
                            origin.1 + dy + line_index as f32 * line_height,
                        ),
                        line,
                        BODY_FONT,
                        m.description_size,
                        color,
                    ));
                }
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

#[allow(clippy::too_many_arguments)]
fn paint_row(
    input: &Input<'_>,
    index: usize,
    row: &RowView,
    geometry: &RowGeometry,
    dx: f32,
    dy: f32,
    measurer: &mut TextMeasurer,
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
    // A razão de um valor recusado, abaixo da descrição, em Erro (RF-16.18).
    if let Some(reason) = &row.invalid {
        out.push(text(
            (geometry.reason_origin.0 + dx, geometry.reason_origin.1 + dy),
            reason,
            BODY_FONT,
            m.description_size,
            pal.warning_severity_error,
        ));
    }

    // O aviso de uma linha de atalho -- conflito, ou a combinação que o
    // terminal perde --, abaixo do nome, no tom de Aviso.
    if let Some(notice) = &row.notice {
        out.push(text(
            (geometry.reason_origin.0 + dx, geometry.reason_origin.1 + dy),
            notice,
            BODY_FONT,
            m.description_size,
            pal.warning_severity_warning,
        ));
    }

    paint_control(
        input,
        index,
        row,
        shift(geometry.control, dx, dy),
        measurer,
        out,
    );

    // O ponto de pendente 6×6, em Acento, entre o controle e o botão de
    // restaurar (ADR-0060 §2).
    if row.pending {
        out.push(rounded(
            shift(geometry.dot, dx, dy),
            m.dot_size / 2.0,
            pal.dialog_focus_ring,
            palette::TRANSPARENT,
            0.0,
        ));
    }
    paint_restore(input, index, row, shift(geometry.restore, dx, dy), out);

    if input.focus == Focus::Row(index) {
        out.push(ring(rect, m.row_radius, pal));
    }
}

/// "Restaurar padrão": botão de ícone `rotate-ccw` com a anatomia do botão de
/// fechar da aba -- 25×17 de alvo, raio 4, ícone `#727a86`, e no hover fundo
/// `#39404b` com o ícone `#e4e8ee` --, visível só com a linha sob o cursor ou
/// focada, e só quando a opção difere do padrão (ADR-0060 §2).
fn paint_restore(
    input: &Input<'_>,
    index: usize,
    row: &RowView,
    rect: Rect,
    out: &mut Vec<Primitive>,
) {
    let row_active =
        input.hovered.and_then(Hit::row_index) == Some(index) || input.focus == Focus::Row(index);
    if !row.can_reset || !row_active {
        return;
    }
    let hovered = input.hovered == Some(Hit::Restore(index));
    if hovered {
        out.push(rounded(
            rect,
            RESTORE_RADIUS,
            RESTORE_HOVER_BACKGROUND,
            palette::TRANSPARENT,
            0.0,
        ));
    }
    out.push(centered_glyph(
        icon::ROTATE_CCW,
        rect,
        input.style.icon_em_size,
        if hovered {
            RESTORE_HOVER_ICON
        } else {
            RESTORE_ICON
        },
    ));
}

// ---- controles

fn paint_control(
    input: &Input<'_>,
    index: usize,
    row: &RowView,
    rect: Rect,
    measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let m = input.metrics;
    let invalid = row.invalid.is_some();
    let editing = |part: EditPart| {
        input
            .editing
            .filter(|editing| editing.block == index && editing.part == part)
    };
    match &row.control {
        ControlView::Toggle { on } => push_toggle(rect, *on, TOGGLE_ON, TOGGLE_OFF, out),
        ControlView::Field {
            text: value,
            right_aligned,
            text_width,
            ..
        } => {
            if let Some(editing) = editing(EditPart::Field) {
                paint_editing_field(input, rect, editing, invalid, measurer, out);
            } else {
                paint_field_box(input, rect, invalid, out);
                let x = if *right_aligned {
                    rect.x + rect.width - m.field_padding_x - text_width
                } else {
                    rect.x + m.field_padding_x
                };
                paint_field_text(input, value, x, rect, out);
            }
        }
        ControlView::Segmented {
            labels,
            widths,
            selected,
        } => paint_segmented(input, labels, widths, *selected, rect, out),
        ControlView::Choice { text: value } => {
            paint_field_box(input, rect, false, out);
            paint_field_text(input, value, rect.x + m.field_padding_x, rect, out);
            paint_caret(input, rect, out);
        }
        ControlView::List {
            items,
            two_fields,
            add_label,
        } => paint_list(
            input,
            index,
            (items, *two_fields, add_label),
            rect,
            measurer,
            out,
        ),
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
        ControlView::Chips { chips } => paint_chips(input, chips, rect, measurer, out),
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
            if let Some(editing) = editing(EditPart::GitSeconds) {
                paint_editing_field(input, number, editing, invalid, measurer, out);
            } else {
                paint_field_box(input, number, invalid, out);
                // O número vem alinhado à direita, como todo campo numérico.
                let x = number.x + number.width - m.field_padding_x - text_width;
                paint_field_text(input, seconds, x, number, out);
            }
        }
    }
}

/// O campo de texto: fundo, borda e raio do editor de grupo (ADR-0060 §3). A
/// borda fica em Erro quando o valor foi recusado (RF-16.18).
fn paint_field_box(input: &Input<'_>, rect: Rect, invalid: bool, out: &mut Vec<Primitive>) {
    let pal = input.pal;
    out.push(rounded(
        rect,
        input.metrics.field_radius,
        pal.editor_input_background,
        if invalid {
            pal.warning_severity_error
        } else {
            pal.editor_input_border
        },
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

/// Onde o texto de um campo em edição começa em x: o campo rola para manter o
/// cursor à vista (`scrolled_text_x`, o mesmo do rename e da busca). Compartilhado
/// com o clique, que converte o x do mouse em posição de cursor -- os dois nunca
/// discordam.
pub(crate) fn editing_text_x(m: &Metrics, field: Rect, text_width: f32) -> f32 {
    let text_area = (field.width - m.field_padding_x * 2.0).max(0.0);
    scrolled_text_x(field.x, m.field_padding_x, text_width, text_area)
}

/// Campo em edição: foco no anel de Acento (ou Erro, se o valor recusado
/// continua), texto à esquerda, seleção e cursor do ADR-0035. Mede o texto
/// deste campo por quadro -- um só, o que está recebendo teclas, como o campo
/// de rename da barra de abas.
fn paint_editing_field(
    input: &Input<'_>,
    rect: Rect,
    editing: &Editing,
    invalid: bool,
    measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let m = input.metrics;
    let pal = input.pal;
    out.push(rounded(
        rect,
        m.field_radius,
        pal.editor_input_background,
        if invalid {
            pal.warning_severity_error
        } else {
            pal.dialog_focus_ring
        },
        1.0,
    ));
    let buffer = editing.state.text();
    let size = m.field_font_size;
    let text_width = measurer.measure_width(buffer, BODY_FONT, size);
    let text_x = editing_text_x(m, rect, text_width);
    let inner = Rect {
        x: rect.x + m.field_padding_x,
        y: rect.y,
        width: (rect.width - m.field_padding_x * 2.0).max(0.0),
        height: rect.height,
    };
    let bar_y = rect.y + 3.0;
    let bar_height = (rect.height - 6.0).max(0.0);
    out.push(Primitive::PushClip(inner));
    let selection = editing.state.selection_range();
    if let Some((start, end)) = selection {
        let x0 = text_x + measurer.measure_width(&buffer[..start], BODY_FONT, size);
        let x1 = text_x + measurer.measure_width(&buffer[..end], BODY_FONT, size);
        out.push(quad(
            Rect {
                x: x0,
                y: bar_y,
                width: x1 - x0,
                height: bar_height,
            },
            input.selection_color,
        ));
    }
    out.push(text(
        (text_x, centered_y(rect, size)),
        buffer,
        BODY_FONT,
        size,
        pal.editor_input_text,
    ));
    if selection.is_none() {
        let before = measurer.measure_width(&buffer[..editing.state.cursor()], BODY_FONT, size);
        let caret_x = (text_x + before).min(inner.x + inner.width - 1.0);
        out.push(quad(
            Rect {
                x: caret_x,
                y: bar_y,
                width: 1.0,
                height: bar_height,
            },
            pal.editor_input_text,
        ));
    }
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

/// O campo de filtro do grupo Atalhos: o campo de texto da tela, com a frase
/// esmaecida enquanto está vazio e o anel de foco do teclado (ADR-0060 §3).
fn paint_filter(
    input: &Input<'_>,
    index: usize,
    rect: Rect,
    (value, placeholder): (&str, &str),
    measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let m = input.metrics;
    let pal = input.pal;
    let editing = input
        .editing
        .filter(|editing| editing.block == index && editing.part == EditPart::Filter);
    if let Some(editing) = editing {
        paint_editing_field(input, rect, editing, false, measurer, out);
    } else {
        paint_field_box(input, rect, false, out);
        if value.is_empty() {
            let inner = Rect {
                x: rect.x + m.field_padding_x,
                y: rect.y,
                width: (rect.width - m.field_padding_x * 2.0).max(0.0),
                height: rect.height,
            };
            out.push(Primitive::PushClip(inner));
            out.push(text(
                (inner.x, centered_y(rect, m.field_font_size)),
                placeholder,
                BODY_FONT,
                m.field_font_size,
                pal.editor_section_text,
            ));
            out.push(Primitive::PopClip);
        } else {
            paint_field_text(input, value, rect.x + m.field_padding_x, rect, out);
        }
    }
    if input.focus == Focus::Row(index) {
        out.push(ring(rect, m.field_radius, pal));
    }
}

/// Os atalhos de uma ação: o chip do drawer -- mono 10.5px sobre `#1e232b`,
/// borda `#2a2f38`, raio 4 -- lado a lado com o `gap: 6`; "Nenhum atalho"
/// esmaecido; e o chip em captura com a borda Acento e a frase esmaecida
/// (ADR-0060 §3).
fn paint_chips(
    input: &Input<'_>,
    chips: &[ChipView],
    rect: Rect,
    _measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let m = input.metrics;
    let pal = input.pal;
    let mut x = rect.x;
    for chip in chips {
        let chip_rect = Rect {
            x,
            y: rect.y + (rect.height - m.chip_height) / 2.0,
            width: chip.width,
            height: m.chip_height,
        };
        let (border, color) = match chip.tone {
            ChipTone::Normal => (CHIP_BORDER, pal.menu_item_text),
            ChipTone::Muted => (CHIP_BORDER, pal.editor_section_text),
            ChipTone::Capturing => (pal.dialog_focus_ring, pal.editor_section_text),
        };
        out.push(rounded(
            chip_rect,
            m.chip_radius,
            CHIP_BACKGROUND,
            border,
            1.0,
        ));
        out.push(text(
            (
                chip_rect.x + 1.0 + m.chip_padding_x,
                centered_y(chip_rect, m.chip_font_size),
            ),
            &chip.text,
            CHIP_FONT,
            m.chip_font_size,
            color,
        ));
        x += chip.width + m.list_gap;
    }
}

/// Lista editável: um campo por item -- dois, na de nome e valor -- com o `X`
/// do botão de fechar da aba à direita, e o item "Adicionar" -- ícone `PLUS`
/// e texto de item de menu -- logo abaixo (ADR-0060 §3). `list` é o que a
/// linha mostra: os itens, se há dois campos por item, e o texto do
/// "Adicionar".
fn paint_list(
    input: &Input<'_>,
    index: usize,
    list: (&[ListItemView], bool, &str),
    rect: Rect,
    measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let (items, two_fields, add_label) = list;
    let m = input.metrics;
    let pal = input.pal;
    let style = input.style;
    let geometry = list_geometry(rect, m, items.len(), two_fields);
    let editing = |part: EditPart| {
        input
            .editing
            .filter(|editing| editing.block == index && editing.part == part)
    };
    let hovered_part = match input.hovered {
        Some(Hit::Control(hit_index, part)) if hit_index == index => Some(part),
        _ => None,
    };
    for (item_index, (item, rects)) in items.iter().zip(&geometry.items).enumerate() {
        let fields = [
            (rects.first, &item.first, EditPart::ListFirst(item_index)),
            (
                rects.second.unwrap_or(rects.first),
                &item.second,
                EditPart::ListSecond(item_index),
            ),
        ];
        for (field_index, (field, value, part)) in fields.into_iter().enumerate() {
            if field_index == 1 && rects.second.is_none() {
                break;
            }
            // O nome é o que está errado (vazio ou repetido): só a borda do
            // campo do nome fica em Erro, a do valor não.
            let invalid = item.invalid && field_index == 0;
            if let Some(editing) = editing(part) {
                paint_editing_field(input, field, editing, invalid, measurer, out);
            } else {
                paint_field_box(input, field, invalid, out);
                paint_field_text(input, value, field.x + m.field_padding_x, field, out);
            }
        }
        // O `X`: a anatomia do botão de fechar da aba -- ícone `#727a86`, e
        // no hover fundo `#39404b` com o ícone `#e4e8ee`.
        let hovered = hovered_part == Some(ControlPart::ListRemove(item_index));
        if hovered {
            out.push(rounded(
                rects.remove,
                RESTORE_RADIUS,
                RESTORE_HOVER_BACKGROUND,
                palette::TRANSPARENT,
                0.0,
            ));
        }
        out.push(centered_glyph(
            icon::X,
            rects.remove,
            style.icon_em_size,
            if hovered {
                RESTORE_HOVER_ICON
            } else {
                RESTORE_ICON
            },
        ));
    }
    let add = geometry.add;
    if hovered_part == Some(ControlPart::ListAdd) {
        out.push(rounded(
            add,
            input.config.appearance.context_menu.item_corner_radius as f32,
            pal.menu_item_hover,
            palette::TRANSPARENT,
            0.0,
        ));
    }
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

#[cfg(test)]
mod tests {
    use porecatu_render::TextMeasurer;

    use super::super::catalog::{self, option};
    use super::super::content::{self, ContentKey};
    use super::super::draft::Draft;
    use super::super::field_edit::{EditPart, Editing};
    use super::super::interact;
    use super::super::layout::{self, footer_buttons, group_items};
    use super::*;
    use crate::messages::test_support;

    struct Fixture {
        config: Config,
        pal: ResolvedPalette,
        style: TabBarStyle,
        catalog: Catalog,
    }

    fn fixture() -> Fixture {
        let config = Config::default();
        Fixture {
            pal: ResolvedPalette::from_config(&config),
            style: TabBarStyle::from_config(&config),
            catalog: test_support::pt_br(),
            config,
        }
    }

    /// Pinta o grupo Terminal sobre `draft`, com o foco, o realce e a edição
    /// dados. Devolve também o índice do bloco de `font_size`.
    fn paint_terminal(
        f: &Fixture,
        draft: &Draft,
        focus_font_size: bool,
        hovered: impl Fn(usize) -> Option<Hit>,
        editing: Option<&Editing>,
    ) -> (Vec<Primitive>, usize) {
        let m = Metrics::from_config(&f.config, 52.0);
        let layout = layout::layout(900.0, 640.0, 52.0, 200.0, m.footer_height());
        let key = ContentKey {
            group: Group::Terminal,
            panel_width_bits: layout.panel.width.to_bits(),
            generation: 0,
        };
        let mut measurer = TextMeasurer::new();
        let content = content::build(key, draft, None, None, &f.catalog, &m, &mut measurer);
        let index = content
            .blocks
            .iter()
            .position(|block| matches!(block, Block::Row(row) if row.option == Some("font_size")))
            .unwrap();
        let items = group_items(layout.sidebar, m.sidebar_padding, m.sidebar_item_height);
        let footer = footer_buttons(&m, layout.footer, content.footer_widths);
        let out = paint_body(
            &Input {
                layout: &layout,
                metrics: &m,
                content: &content,
                items: &items,
                footer: &footer,
                selected: Group::Terminal,
                focus: if focus_font_size {
                    Focus::Row(index)
                } else {
                    Focus::Sidebar
                },
                hovered: hovered(index),
                scroll: 0.0,
                pending_groups: &[],
                editing,
                selection_color: palette::hex(1, 2, 3),
                footer_available: [true, false, false],
                style: &f.style,
                pal: &f.pal,
                config: &f.config,
                catalog: &f.catalog,
            },
            &mut measurer,
        );
        (out, index)
    }

    fn pending_size() -> Draft {
        let mut draft = Draft::new(&Config::default());
        interact::commit_text(&mut draft, option("font_size").unwrap(), "18");
        draft
    }

    fn has_restore_icon(out: &[Primitive]) -> bool {
        out.iter()
            .any(|p| matches!(p, Primitive::Text(run) if run.text == icon::ROTATE_CCW.glyph))
    }

    fn dots(out: &[Primitive], f: &Fixture) -> usize {
        out.iter()
            .filter(|p| {
                matches!(p, Primitive::RoundedQuad(q)
                    if q.rect.width == 6.0 && q.rect.height == 6.0 && q.color == f.pal.dialog_focus_ring)
            })
            .count()
    }

    #[test]
    fn a_pending_option_paints_the_accent_dot_and_a_clean_one_does_not() {
        let f = fixture();
        let (clean, _) = paint_terminal(&f, &Draft::new(&f.config), false, |_| None, None);
        assert_eq!(dots(&clean, &f), 0);
        let (pending, _) = paint_terminal(&f, &pending_size(), false, |_| None, None);
        assert_eq!(dots(&pending, &f), 1);
    }

    #[test]
    fn restore_shows_only_on_a_row_that_differs_and_is_hovered_or_focused() {
        let f = fixture();
        let clean = Draft::new(&f.config);
        let pending = pending_size();
        // Nada a restaurar: nunca aparece, nem com foco e cursor.
        let (out, _) = paint_terminal(&f, &clean, true, |i| Some(Hit::Row(i)), None);
        assert!(!has_restore_icon(&out));
        // Diferente do padrão, mas sem cursor nem foco: escondido.
        let (out, _) = paint_terminal(&f, &pending, false, |_| None, None);
        assert!(!has_restore_icon(&out));
        // Sob o cursor: aparece.
        let (out, _) = paint_terminal(&f, &pending, false, |i| Some(Hit::Row(i)), None);
        assert!(has_restore_icon(&out));
        // Com o cursor num controle da mesma linha, também.
        let (out, _) = paint_terminal(
            &f,
            &pending,
            false,
            |i| Some(Hit::Control(i, layout::ControlPart::Whole)),
            None,
        );
        assert!(has_restore_icon(&out));
        // Com o foco do teclado: aparece.
        let (out, _) = paint_terminal(&f, &pending, true, |_| None, None);
        assert!(has_restore_icon(&out));
        // Cursor numa outra linha: não aparece.
        let (out, index) = paint_terminal(&f, &pending, false, |i| Some(Hit::Row(i + 2)), None);
        assert!(!has_restore_icon(&out), "linha {index}");
    }

    #[test]
    fn hovering_the_restore_button_gives_it_the_hover_background_and_icon() {
        let f = fixture();
        let (out, _) = paint_terminal(&f, &pending_size(), false, |i| Some(Hit::Restore(i)), None);
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::RoundedQuad(q) if q.color == RESTORE_HOVER_BACKGROUND
        )));
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == icon::ROTATE_CCW.glyph && run.color == RESTORE_HOVER_ICON
        )));
        // Sem o cursor em cima do botão, o ícone é o do repouso.
        let (out, _) = paint_terminal(&f, &pending_size(), false, |i| Some(Hit::Row(i)), None);
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == icon::ROTATE_CCW.glyph && run.color == RESTORE_ICON
        )));
        assert!(!out.iter().any(|p| matches!(
            p,
            Primitive::RoundedQuad(q) if q.color == RESTORE_HOVER_BACKGROUND
        )));
    }

    #[test]
    fn a_refused_value_paints_the_error_border_and_the_reason_in_error_color() {
        let f = fixture();
        let mut draft = Draft::new(&f.config);
        interact::commit_text(&mut draft, option("font_size").unwrap(), "900");
        let (out, _) = paint_terminal(&f, &draft, false, |_| None, None);
        let error = f.pal.warning_severity_error;
        assert!(
            out.iter().any(|p| matches!(
                p,
                Primitive::RoundedQuad(q) if q.border_color == error && q.border_width == 1.0
            )),
            "borda do controle em Erro"
        );
        let reason = out.iter().find_map(|p| match p {
            Primitive::Text(run) if run.color == error => Some(run),
            _ => None,
        });
        let reason = reason.expect("a razão em Erro");
        assert_eq!(reason.size_px, 11.0);
        assert!(reason.text.starts_with("Valor fora da faixa"));
    }

    #[test]
    fn an_edited_field_paints_the_focus_border_the_text_and_a_caret() {
        let f = fixture();
        let draft = Draft::new(&f.config);
        let mut editing = Editing::new(0, EditPart::Field, "14".to_owned());
        // O índice do bloco de `font_size` vem do conteúdo.
        let (_, index) = paint_terminal(&f, &draft, false, |_| None, None);
        editing.block = index;
        editing.state.insert_char('5');
        let (out, _) = paint_terminal(&f, &draft, false, |_| None, Some(&editing));
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::RoundedQuad(q) if q.border_color == f.pal.dialog_focus_ring
                && q.rect.width == 88.0
        )));
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == "145"
        )));
        // Cursor de 1px de largura, na cor do texto.
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Quad(q) if q.rect.width == 1.0 && q.color == f.pal.editor_input_text
        )));
    }

    #[test]
    fn a_selection_in_the_edited_field_paints_the_selection_background() {
        let f = fixture();
        let draft = Draft::new(&f.config);
        let (_, index) = paint_terminal(&f, &draft, false, |_| None, None);
        let mut editing = Editing::new(index, EditPart::Field, "1234".to_owned());
        editing.state.select_all();
        let (out, _) = paint_terminal(&f, &draft, false, |_| None, Some(&editing));
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Quad(q) if q.color == palette::hex(1, 2, 3)
        )));
        // Com seleção não há cursor de 1px.
        assert!(!out.iter().any(|p| matches!(
            p,
            Primitive::Quad(q) if q.rect.width == 1.0 && q.color == f.pal.editor_input_text
        )));
    }

    #[test]
    fn every_catalog_option_still_paints_with_a_pending_value() {
        // Nenhum controle quebra a pintura com um valor pendente.
        let f = fixture();
        let mut draft = Draft::new(&f.config);
        for option in catalog::OPTIONS {
            let _ = draft.set(option, catalog::probe_value(option));
        }
        let (out, _) = paint_terminal(&f, &draft, false, |_| None, None);
        assert!(!out.is_empty());
    }

    // ---- listas, temas e notas

    /// Pinta `group` sobre `draft`. `hovered` recebe o índice da linha de
    /// `option` e devolve o alvo sob o cursor. Devolve a pintura e o conteúdo.
    fn paint_group(
        f: &Fixture,
        group: Group,
        draft: &Draft,
        session_theme: Option<&str>,
        option_id: &str,
        hovered: impl Fn(usize) -> Option<Hit>,
        editing: Option<&Editing>,
    ) -> (Vec<Primitive>, Content, usize) {
        let m = Metrics::from_config(&f.config, 52.0);
        let layout = layout::layout(900.0, 2000.0, 52.0, 200.0, m.footer_height());
        let key = ContentKey {
            group,
            panel_width_bits: layout.panel.width.to_bits(),
            generation: 0,
        };
        let mut measurer = TextMeasurer::new();
        let content = content::build(
            key,
            draft,
            session_theme,
            None,
            &f.catalog,
            &m,
            &mut measurer,
        );
        let index = content
            .blocks
            .iter()
            .position(|block| matches!(block, Block::Row(row) if row.option == Some(option_id)))
            .unwrap();
        let items = group_items(layout.sidebar, m.sidebar_padding, m.sidebar_item_height);
        let footer = footer_buttons(&m, layout.footer, content.footer_widths);
        let out = paint_body(
            &Input {
                layout: &layout,
                metrics: &m,
                content: &content,
                items: &items,
                footer: &footer,
                selected: group,
                focus: Focus::Sidebar,
                hovered: hovered(index),
                scroll: 0.0,
                pending_groups: &[],
                editing,
                selection_color: palette::hex(1, 2, 3),
                footer_available: [true, false, false],
                style: &f.style,
                pal: &f.pal,
                config: &f.config,
                catalog: &f.catalog,
            },
            &mut measurer,
        );
        (out, content, index)
    }

    fn env_draft(f: &Fixture, rows: &[(&str, &str)]) -> Draft {
        let mut draft = Draft::new(&f.config);
        let rows: Vec<(String, String)> = rows
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        let _ = draft.set_rows(option("shell_env").unwrap(), &rows);
        draft
    }

    #[test]
    fn a_refused_env_name_paints_that_field_with_the_error_border_only() {
        let f = fixture();
        let draft = env_draft(&f, &[("A", "1"), ("", "2")]);
        let (out, ..) = paint_group(&f, Group::Shell, &draft, None, "shell_env", |_| None, None);
        let error = f.pal.warning_severity_error;
        let red_fields = out
            .iter()
            .filter(|p| {
                matches!(p, Primitive::RoundedQuad(q)
                    if q.border_color == error && q.rect.height == 30.0 && q.rect.width < 240.0)
            })
            .count();
        // Só o campo do nome da linha recusada: a do valor e a outra linha
        // seguem na borda comum.
        assert_eq!(red_fields, 1);
    }

    #[test]
    fn a_list_item_paints_two_fields_for_env_and_one_for_args() {
        let f = fixture();
        let m = Metrics::from_config(&f.config, 52.0);
        let field_boxes = |out: &[Primitive]| {
            out.iter()
                .filter(|p| {
                    matches!(p, Primitive::RoundedQuad(q)
                        if q.color == f.pal.editor_input_background && q.rect.height == m.field_height)
                })
                .count()
        };
        let mut draft = env_draft(&f, &[("A", "1")]);
        let args = option("shell_args").unwrap();
        let _ = draft.set_rows(args, &[("-l".to_owned(), String::new())]);
        let (out, ..) = paint_group(&f, Group::Shell, &draft, None, "shell_env", |_| None, None);
        // `shell.program` (1 campo), `shell.args` (1) e `shell.env` (2).
        let text_fields = out
            .iter()
            .filter(|p| matches!(p, Primitive::RoundedQuad(q) if q.rect.width == m.text_field_width && q.color == f.pal.editor_input_background))
            .count();
        assert_eq!(text_fields, 2, "program e o item de args");
        assert_eq!(field_boxes(&out), 4);
    }

    #[test]
    fn hovering_a_list_x_gives_it_the_close_button_hover_and_the_add_item_the_menu_hover() {
        let f = fixture();
        let draft = env_draft(&f, &[("A", "1")]);
        let hovered = |part: layout::ControlPart| move |i| Some(Hit::Control(i, part));
        let (out, ..) = paint_group(
            &f,
            Group::Shell,
            &draft,
            None,
            "shell_env",
            hovered(layout::ControlPart::ListRemove(0)),
            None,
        );
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::RoundedQuad(q) if q.color == RESTORE_HOVER_BACKGROUND
        )));
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == icon::X.glyph && run.color == RESTORE_HOVER_ICON
        )));
        // Sem o cursor no `X`, ele é o ícone do repouso `#727a86`.
        let (out, ..) = paint_group(&f, Group::Shell, &draft, None, "shell_env", |_| None, None);
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == icon::X.glyph && run.color == RESTORE_ICON
        )));
        let (out, ..) = paint_group(
            &f,
            Group::Shell,
            &draft,
            None,
            "shell_env",
            hovered(layout::ControlPart::ListAdd),
            None,
        );
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::RoundedQuad(q) if q.color == f.pal.menu_item_hover
        )));
    }

    #[test]
    fn an_edited_list_field_paints_the_accent_ring_and_the_cursor() {
        let f = fixture();
        let draft = env_draft(&f, &[("A", "1")]);
        let (_, _, index) =
            paint_group(&f, Group::Shell, &draft, None, "shell_env", |_| None, None);
        let editing = Editing::new(index, EditPart::ListSecond(0), "1".to_owned());
        let (out, ..) = paint_group(
            &f,
            Group::Shell,
            &draft,
            None,
            "shell_env",
            |_| None,
            Some(&editing),
        );
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::RoundedQuad(q) if q.border_color == f.pal.dialog_focus_ring
                && q.color == f.pal.editor_input_background
        )));
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Quad(q) if q.rect.width == 1.0 && q.color == f.pal.editor_input_text
        )));
    }

    #[test]
    fn the_trusted_paths_warning_paints_in_the_warning_color_line_by_line() {
        let f = fixture();
        let draft = Draft::new(&f.config);
        let (out, content, _) = paint_group(
            &f,
            Group::Project,
            &draft,
            None,
            "trusted_paths",
            |_| None,
            None,
        );
        let lines = content
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Note { lines, .. } => Some(lines.clone()),
                _ => None,
            })
            .unwrap();
        for line in &lines {
            assert!(
                out.iter().any(|p| matches!(
                    p,
                    Primitive::Text(run) if &run.text == line
                        && run.color == f.pal.warning_severity_warning
                )),
                "{line}"
            );
        }
    }

    #[test]
    fn the_session_theme_note_paints_muted_and_the_chosen_theme_gets_the_accent_border() {
        let f = fixture();
        let mut draft = Draft::new(&f.config);
        let second = f.config.themes[1].name.clone();
        draft
            .set(
                option("theme").unwrap(),
                porecatu_config::EditValue::String(second),
            )
            .unwrap();
        let (out, content, _) = paint_group(
            &f,
            Group::Appearance,
            &draft,
            Some("nord"),
            "theme",
            |_| None,
            None,
        );
        let note = content
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Note { lines, .. } => Some(lines.join(" ")),
                _ => None,
            })
            .unwrap();
        assert_eq!(note, "Esta sessão está usando o tema nord.");
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == note && run.color == f.pal.status_bar_stale_cwd
        )));
        // Uma linha só leva a borda de Acento: a do tema escolhido.
        let accent_rows = out
            .iter()
            .filter(|p| {
                matches!(p, Primitive::RoundedQuad(q)
                if q.color == ROW_BACKGROUND && q.border_color == f.pal.dialog_focus_ring)
            })
            .count();
        assert_eq!(accent_rows, 1);
        // E o ponto de pendente fica nela.
        assert_eq!(dots(&out, &f), 1);
    }

    // ---- grupo Atalhos

    use super::super::content::ShortcutsView;
    use super::super::shortcuts::{Capturing, Conflict, Shortcuts};
    use crate::keymap::{Chord, Platform};
    use porecatu_core::Action;

    fn paint_shortcuts(
        f: &Fixture,
        state: &Shortcuts,
        filter: &str,
        capturing: Option<&Capturing>,
        editing: Option<&Editing>,
        focus: Focus,
    ) -> Vec<Primitive> {
        let m = Metrics::from_config(&f.config, 52.0);
        let layout = layout::layout(900.0, 4000.0, 52.0, 200.0, m.footer_height());
        let key = ContentKey {
            group: Group::Shortcuts,
            panel_width_bits: layout.panel.width.to_bits(),
            generation: 0,
        };
        let mut measurer = TextMeasurer::new();
        let view = ShortcutsView {
            state,
            filter,
            capturing,
        };
        let content = content::build(
            key,
            &Draft::new(&f.config),
            None,
            Some(&view),
            &f.catalog,
            &m,
            &mut measurer,
        );
        let items = group_items(layout.sidebar, m.sidebar_padding, m.sidebar_item_height);
        let footer = footer_buttons(&m, layout.footer, content.footer_widths);
        paint_body(
            &Input {
                layout: &layout,
                metrics: &m,
                content: &content,
                items: &items,
                footer: &footer,
                selected: Group::Shortcuts,
                focus,
                hovered: None,
                scroll: 0.0,
                pending_groups: &[],
                editing,
                selection_color: palette::hex(1, 2, 3),
                footer_available: [true, false, false],
                style: &f.style,
                pal: &f.pal,
                config: &f.config,
                catalog: &f.catalog,
            },
            &mut measurer,
        )
    }

    fn chip_boxes(out: &[Primitive]) -> Vec<(Rect, Color)> {
        out.iter()
            .filter_map(|p| match p {
                Primitive::RoundedQuad(q) if q.color == CHIP_BACKGROUND => {
                    Some((q.rect, q.border_color))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_shortcut_row_paints_its_chips_with_the_drawer_chip_and_the_chord_label() {
        let f = fixture();
        let state = Shortcuts::new(&f.config, Platform::Windows);
        let out = paint_shortcuts(&f, &state, "", None, None, Focus::Sidebar);
        assert!(!chip_boxes(&out).is_empty());
        // `Nova aba` leva o chip `Ctrl+Shift+T` em mono 10.5px.
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == "Ctrl+Shift+T"
                && run.font == CHIP_FONT
                && run.size_px == 10.5
                && run.color == f.pal.menu_item_text
        )));
        // O chip fica na borda comum; sem captura nenhuma leva o Acento.
        assert!(
            chip_boxes(&out)
                .iter()
                .all(|(_, border)| *border == CHIP_BORDER)
        );
        // "Nenhum atalho" esmaecido.
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == "Nenhum atalho"
                && run.color == f.pal.editor_section_text
        )));
    }

    #[test]
    fn the_capturing_chip_takes_the_accent_border_and_the_dim_phrase() {
        let f = fixture();
        let state = Shortcuts::new(&f.config, Platform::Windows);
        let chord = Chord::parse("ctrl+shift+t").unwrap();
        let capturing = Capturing {
            action: Action::TabNew,
            replacing: Some(chord),
            frozen: vec![chord],
            conflict: None,
        };
        let out = paint_shortcuts(&f, &state, "", Some(&capturing), None, Focus::Sidebar);
        let accent: Vec<_> = chip_boxes(&out)
            .into_iter()
            .filter(|(_, border)| *border == f.pal.dialog_focus_ring)
            .collect();
        assert_eq!(accent.len(), 1);
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == "Pressione a combinação de teclas…"
                && run.color == f.pal.editor_section_text
        )));
        assert!(!out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == "Ctrl+Shift+T"
        )));
    }

    #[test]
    fn a_conflict_paints_the_warning_line_and_the_two_buttons() {
        let f = fixture();
        let state = Shortcuts::new(&f.config, Platform::Windows);
        let old = Chord::parse("ctrl+shift+f").unwrap();
        let capturing = Capturing {
            action: Action::SearchOpen,
            replacing: Some(old),
            frozen: vec![old],
            conflict: Some(Conflict {
                chord: Chord::parse("ctrl+shift+r").unwrap(),
                other: Action::TabRename,
            }),
        };
        let out = paint_shortcuts(&f, &state, "", Some(&capturing), None, Focus::Sidebar);
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == "Já em uso por Renomear aba."
                && run.color == f.pal.warning_severity_warning
                && run.size_px == 11.0
        )));
        for label in ["Substituir", "Cancelar"] {
            assert!(
                out.iter()
                    .any(|p| matches!(p, Primitive::Text(run) if run.text == label)),
                "{label}"
            );
        }
    }

    #[test]
    fn the_filter_paints_its_placeholder_then_its_text_and_the_focus_ring() {
        let f = fixture();
        let state = Shortcuts::new(&f.config, Platform::Windows);
        let out = paint_shortcuts(&f, &state, "", None, None, Focus::Sidebar);
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == "Filtrar por nome ou tecla…"
                && run.color == f.pal.editor_section_text
        )));
        let out = paint_shortcuts(&f, &state, "aba", None, None, Focus::Sidebar);
        assert!(!out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == "Filtrar por nome ou tecla…"
        )));
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Text(run) if run.text == "aba" && run.color == f.pal.editor_input_text
        )));
        // Foco do teclado no filtro (bloco 0): o anel de Acento.
        let focused = paint_shortcuts(&f, &state, "aba", None, None, Focus::Row(0));
        assert!(focused.iter().any(|p| matches!(
            p,
            Primitive::RoundedQuad(q) if q.color == palette::TRANSPARENT
                && q.border_color == f.pal.dialog_focus_ring
        )));
        // Em edição: o campo com a borda de Acento e o cursor.
        let editing = Editing::new(0, EditPart::Filter, "aba".to_owned());
        let editing_out = paint_shortcuts(&f, &state, "aba", None, Some(&editing), Focus::Sidebar);
        assert!(editing_out.iter().any(|p| matches!(
            p,
            Primitive::Quad(q) if q.rect.width == 1.0 && q.color == f.pal.editor_input_text
        )));
    }
}
