// SPDX-License-Identifier: GPL-3.0-or-later

//! Traduz `tab_bar::TabBarLayout` (mais estado efêmero que o layout puro
//! não conhece: aba ativa, aba `Exited`, edição de rename em andamento) em
//! `Primitive`s da camada `Chrome` (ADR-0018). Cores e dimensões: espec.
//! visual §1.2, §1.3, §2.5, §2.6, como constantes em `palette.rs`, mesmo
//! padrão de `paint.rs` para a grade.
//!
//! Sem hover nesta etapa -- a barra não rastreia posição do mouse fora de
//! clique (`App::cursor_position` é da área do terminal); o estado default
//! de cada elemento já é o que a espec. descreve fora de hover, então a
//! barra fica correta sem ele -- é um refinamento, não uma etapa 4/6.

use porecatu_core::{TabId, Workspace};
use porecatu_render::{Color, FontFace, Primitive, Quad, Rect, RoundedQuad, SansWeight, TextRun};

use crate::palette;
use crate::rename::RenameState;
use crate::tab_bar::{TabBarLayout, TabBarStyle};

/// Fonte dos ícones da barra (fechar, nova aba) -- mesma família do
/// rótulo, peso regular (espec. não distingue peso pra glyphs de ícone).
const ICON_FONT: FontFace = FontFace::Sans {
    weight: SansWeight::Regular,
};
const LABEL_FONT: FontFace = ICON_FONT;

const CLOSE_ICON_SIZE: f32 = 10.0; // espec §2.5: "✕ 10px"
const NEW_TAB_ICON_SIZE: f32 = 15.0; // espec §2.6: "+ 15px"
const TAB_UNDERLINE_HEIGHT: f32 = 2.0; // espec §2.5: "inset 0 -2px 0"
const BAR_SEPARATOR_HEIGHT: f32 = 1.0;

// Campo de rename: espec §2.5 dá largura (120), padding (2px 5px) e fonte
// (12px), mas não a altura da caixa. Valor de trabalho: texto 12px +
// padding vertical 2px de cada lado + folga -- ajustar se ficar
// visualmente errado na prática (mesmo tipo de nota que F1 deixou em
// `FONT_SIZE_PX`/`LINE_HEIGHT_MULTIPLIER`).
const RENAME_FIELD_HEIGHT: f32 = 20.0;
const RENAME_FIELD_MAX_WIDTH: f32 = 120.0;
const RENAME_FONT_SIZE: f32 = 12.0;
const RENAME_PADDING_X: f32 = 5.0;

/// Monta as primitivas da barra inteira: fundo, separador, cada aba (fundo,
/// borda, sublinhado, rótulo ou campo de rename, botão de fechar) e o
/// botão de nova aba.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    layout: &TabBarLayout,
    workspace: &Workspace,
    active: Option<TabId>,
    rename: &RenameState,
    style: &TabBarStyle,
    bar_width: f32,
    measurer: &mut porecatu_render::TextMeasurer,
) -> Vec<Primitive> {
    let bar_height = style.tab_height + style.wrapper_padding * 2.0;
    let mut out = Vec::new();

    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: bar_width,
            height: bar_height,
        },
        color: palette::BAR_BACKGROUND,
    }));
    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: 0.0,
            y: bar_height - BAR_SEPARATOR_HEIGHT,
            width: bar_width,
            height: BAR_SEPARATOR_HEIGHT,
        },
        color: palette::BAR_SEPARATOR,
    }));

    for group in &layout.groups {
        for tab in &group.tabs {
            let exited = workspace.tab(tab.id).is_some_and(|t| t.is_exited());
            let is_active = active == Some(tab.id);
            let (bg, border, text_color) = if exited {
                (
                    palette::TAB_INACTIVE_BACKGROUND,
                    palette::TAB_INACTIVE_BORDER,
                    palette::TAB_EXITED_TEXT,
                )
            } else if is_active {
                (
                    palette::TAB_ACTIVE_BACKGROUND,
                    palette::TAB_ACTIVE_BORDER,
                    palette::TAB_ACTIVE_TEXT,
                )
            } else {
                (
                    palette::TAB_INACTIVE_BACKGROUND,
                    palette::TAB_INACTIVE_BORDER,
                    palette::TAB_INACTIVE_TEXT,
                )
            };

            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: tab.rect,
                radius: 6.0,
                color: bg,
                border_color: border,
                border_width: 1.0,
            }));

            // Sublinhado de grupo (espec §2.5): F2 só tem o grupo implícito,
            // então é sempre `ungrouped_color` -- grupo de verdade é F3.
            out.push(Primitive::Quad(Quad {
                rect: Rect {
                    x: tab.rect.x,
                    y: tab.rect.y + tab.rect.height - TAB_UNDERLINE_HEIGHT,
                    width: tab.rect.width,
                    height: TAB_UNDERLINE_HEIGHT,
                },
                color: palette::UNGROUPED_UNDERLINE,
            }));

            if rename.editing_tab() == Some(tab.id) {
                paint_rename_field(tab.rect, style, rename.buffer(), measurer, &mut out);
            } else {
                let label_y = tab.rect.y + (tab.rect.height - style.font_size) / 2.0;
                out.push(Primitive::Text(TextRun {
                    origin: (tab.rect.x + style.padding_left, label_y),
                    text: tab.label.clone(),
                    font: LABEL_FONT,
                    size_px: style.font_size,
                    color: text_color,
                }));
            }

            out.push(centered_glyph(
                "\u{2715}",
                tab.close_button,
                CLOSE_ICON_SIZE,
                palette::CLOSE_BUTTON_ICON,
                measurer,
            ));
        }
    }

    if let Some(button) = layout.new_tab_button {
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: button,
            radius: 6.0,
            color: palette::TRANSPARENT,
            border_color: palette::NEW_TAB_BORDER,
            border_width: 1.0,
        }));
        out.push(centered_glyph(
            "+",
            button,
            NEW_TAB_ICON_SIZE,
            palette::NEW_TAB_ICON,
            measurer,
        ));
    }

    out
}

/// Campo de rename (espec §2.5): substitui o rótulo no lugar, largura
/// `min(120, largura disponível)`. Texto rola dentro do campo mantendo o
/// caret (sempre no fim do buffer nesta etapa -- sem edição no meio da
/// string) visível: quando o texto não cabe, a origem desliza para a
/// esquerda, e um `PushClip`/`PopClip` contém o transbordo.
fn paint_rename_field(
    tab_rect: Rect,
    style: &TabBarStyle,
    buffer: &str,
    measurer: &mut porecatu_render::TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let available_width = (tab_rect.width - style.padding_left - style.padding_right).max(0.0);
    let field_width = RENAME_FIELD_MAX_WIDTH.min(available_width);
    let field_rect = Rect {
        x: tab_rect.x + style.padding_left,
        y: tab_rect.y + (tab_rect.height - RENAME_FIELD_HEIGHT) / 2.0,
        width: field_width,
        height: RENAME_FIELD_HEIGHT,
    };
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: field_rect,
        radius: 4.0,
        color: palette::RENAME_BACKGROUND,
        border_color: palette::RENAME_BORDER,
        border_width: 1.0,
    }));

    let text_area = (field_width - RENAME_PADDING_X * 2.0).max(0.0);
    let text_width = measurer.measure_width(buffer, LABEL_FONT, RENAME_FONT_SIZE);
    let text_x = if text_width > text_area {
        field_rect.x + RENAME_PADDING_X - (text_width - text_area)
    } else {
        field_rect.x + RENAME_PADDING_X
    };
    let text_y = field_rect.y + (RENAME_FIELD_HEIGHT - RENAME_FONT_SIZE) / 2.0;

    out.push(Primitive::PushClip(field_rect));
    out.push(Primitive::Text(TextRun {
        origin: (text_x, text_y),
        text: buffer.to_string(),
        font: LABEL_FONT,
        size_px: RENAME_FONT_SIZE,
        color: palette::RENAME_TEXT,
    }));
    let caret_x = (text_x + text_width).min(field_rect.x + field_width - 1.0);
    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: caret_x,
            y: field_rect.y + 3.0,
            width: 1.0,
            height: RENAME_FIELD_HEIGHT - 6.0,
        },
        color: palette::RENAME_TEXT,
    }));
    out.push(Primitive::PopClip);
}

/// Centraliza um glyph de ícone dentro de `rect`, medindo a largura real
/// pra não depender de estimativa (`TextMeasurer` já está em mãos de quem
/// pinta a barra).
fn centered_glyph(
    glyph: &str,
    rect: Rect,
    size_px: f32,
    color: Color,
    measurer: &mut porecatu_render::TextMeasurer,
) -> Primitive {
    let width = measurer.measure_width(glyph, ICON_FONT, size_px);
    let origin = (
        rect.x + (rect.width - width) / 2.0,
        rect.y + (rect.height - size_px) / 2.0,
    );
    Primitive::Text(TextRun {
        origin,
        text: glyph.to_string(),
        font: ICON_FONT,
        size_px,
        color,
    })
}

/// Altura total da barra (espec §2.5/§2.3): abas + a folga do wrapper
/// acima e abaixo. Usado por `lib.rs` para deslocar a grade do terminal e
/// converter posição de clique.
pub fn bar_height(style: &TabBarStyle) -> f32 {
    style.tab_height + style.wrapper_padding * 2.0
}
