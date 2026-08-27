// SPDX-License-Identifier: GPL-3.0-or-later

//! Traduz `tab_bar::TabBarLayout` (mais estado efêmero que o layout puro
//! não conhece: aba ativa, aba `Exited`, edição de rename em andamento,
//! rolagem e arraste desde a Etapa 5) em `Primitive`s da camada `Chrome`
//! (ADR-0018). Cores e dimensões: espec. visual §1.2, §1.3, §2.5, §2.6,
//! §2.17, §2.18, §2.19, como constantes em `palette.rs`/`tab_bar.rs`,
//! mesmo padrão de `paint.rs` para a grade.
//!
//! Sem hover nesta etapa -- a barra não rastreia posição do mouse fora de
//! clique/arraste (`App::cursor_position` é da área do terminal); o estado
//! default de cada elemento já é o que a espec. descreve fora de hover,
//! então a barra fica correta sem ele -- é um refinamento, não uma etapa
//! 4/5/6. Pelo mesmo motivo, o `filter: brightness(1.18)` e a sombra de
//! popover do fantasma de arraste (espec. §2.19) não têm equivalente em
//! `porecatu-render` -- nenhuma primitiva de filtro ou sombra existe ainda
//! (nenhum hover em lugar nenhum do chrome usa isso hoje); o fantasma
//! reaproveita as cores normais da aba, sem o realce.

use porecatu_core::{TabId, Workspace};
use porecatu_render::{Color, FontFace, Primitive, Quad, Rect, RoundedQuad, SansWeight, TextRun};

use crate::palette;
use crate::rename::RenameState;
use crate::tab_bar::{
    self, INDICATOR_DOT_SIZE, Indicator, Overflow, OverflowSide, TabBarLayout, TabBarStyle,
};

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

const OVERFLOW_CHEVRON_SIZE: f32 = 10.0; // espec §2.18: "chevron ‹/› 10px"
const OVERFLOW_COUNT_FONT_SIZE: f32 = 10.0; // espec §2.18: "contagem em mono 10px"
const OVERFLOW_COUNT_RADIUS: f32 = 9.0; // espec §2.4 (mesmo contador da pílula)
const OVERFLOW_INNER_GAP: f32 = 3.0; // folga de trabalho entre chevron e contagem

/// A aba sendo arrastada (espec §2.19): desenhada como fantasma seguindo o
/// cursor no eixo X, presa ao Y da barra -- em vez de na posição que o
/// `layout` calculou para ela (que já reflete o preview de onde ela cairia,
/// e é onde o "buraco" fica: a aba não é desenhada na posição normal
/// enquanto isto está `Some`, deixando o fundo da barra aparecer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragGhost {
    pub tab: TabId,
    /// Coordenada de tela (sem o deslocamento de rolagem) do canto
    /// esquerdo do fantasma.
    pub screen_x: f32,
}

/// Monta as primitivas da barra inteira: fundo, separador, cada aba (fundo,
/// borda, sublinhado, indicador, rótulo ou campo de rename, botão de
/// fechar), o botão de nova aba, os indicadores de overflow (espec §2.18) e
/// o fantasma de arraste (espec §2.19), se algum estiver em andamento.
///
/// `layout` já reflete o encolhimento do §2.18 (`tab_bar::fit_width`) e,
/// durante um arraste, o preview de reordenação (`lib.rs` monta um
/// `Workspace` clonado com a troca aplicada antes de chamar `fit_width`) --
/// esta função só desenha o que recebe, sem saber de nenhuma das duas
/// decisões.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    layout: &TabBarLayout,
    workspace: &Workspace,
    active: Option<TabId>,
    rename: &RenameState,
    style: &TabBarStyle,
    bar_width: f32,
    overflow: Overflow,
    drag: Option<DragGhost>,
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

    // Recorte de verdade da trilha (ADR-0018, espec §2.18: "um recorte só,
    // na camada de chrome; as abas fora da vista desaparecem pelo clip").
    // Tudo dentro deste par desloca pelo scroll -- inclusive o botão de
    // nova aba, que "acompanha o scroll" (espec §2.6).
    out.push(Primitive::PushClip(Rect {
        x: 0.0,
        y: 0.0,
        width: bar_width,
        height: bar_height,
    }));
    let dx = -overflow.scroll_offset;

    for group in &layout.groups {
        for tab in &group.tabs {
            let is_ghost = drag.is_some_and(|g| g.tab == tab.id);
            let tab_rect = shift(tab.rect, dx);

            if is_ghost {
                // O buraco (espec §2.19): fundo da barra já pintado acima
                // aparece por baixo -- nada a desenhar aqui, o fantasma vem
                // depois, fora do recorte.
                continue;
            }

            let exited = workspace.tab(tab.id).is_some_and(|t| t.is_exited());
            let is_active = active == Some(tab.id);
            let (bg, border, text_color) = tab_colors(exited, is_active);

            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: tab_rect,
                radius: 6.0,
                color: bg,
                border_color: border,
                border_width: 1.0,
            }));

            // Sublinhado de grupo (espec §2.5): F2 só tem o grupo implícito,
            // então é sempre `ungrouped_color` -- grupo de verdade é F3.
            out.push(Primitive::Quad(Quad {
                rect: Rect {
                    x: tab_rect.x,
                    y: tab_rect.y + tab_rect.height - TAB_UNDERLINE_HEIGHT,
                    width: tab_rect.width,
                    height: TAB_UNDERLINE_HEIGHT,
                },
                color: palette::UNGROUPED_UNDERLINE,
            }));

            let dot_reserve = if tab.indicator.is_some() {
                INDICATOR_DOT_SIZE + style.internal_gap
            } else {
                0.0
            };
            if let Some(indicator) = tab.indicator {
                let color = match indicator {
                    Indicator::Activity => palette::ACTIVITY_INDICATOR,
                    Indicator::Bell => palette::BELL_INDICATOR,
                };
                out.push(Primitive::RoundedQuad(RoundedQuad {
                    rect: Rect {
                        x: tab_rect.x + style.padding_left,
                        y: tab_rect.y + (tab_rect.height - INDICATOR_DOT_SIZE) / 2.0,
                        width: INDICATOR_DOT_SIZE,
                        height: INDICATOR_DOT_SIZE,
                    },
                    radius: INDICATOR_DOT_SIZE / 2.0,
                    color,
                    border_color: palette::TRANSPARENT,
                    border_width: 0.0,
                }));
            }

            if rename.editing_tab() == Some(tab.id) {
                paint_rename_field(tab_rect, style, rename.buffer(), measurer, &mut out);
            } else {
                let label_y = tab_rect.y + (tab_rect.height - style.font_size) / 2.0;
                out.push(Primitive::Text(TextRun {
                    origin: (tab_rect.x + style.padding_left + dot_reserve, label_y),
                    text: tab.label.clone(),
                    font: LABEL_FONT,
                    size_px: style.font_size,
                    color: text_color,
                }));
            }

            out.push(centered_glyph(
                "\u{2715}",
                shift(tab.close_button, dx),
                CLOSE_ICON_SIZE,
                palette::CLOSE_BUTTON_ICON,
                measurer,
            ));
        }
    }

    if let Some(button) = layout.new_tab_button {
        let button = shift(button, dx);
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

    out.push(Primitive::PopClip);

    if overflow.hidden_left > 0 {
        paint_overflow_pill(
            OverflowSide::Left,
            overflow.hidden_left,
            bar_width,
            bar_height,
            measurer,
            &mut out,
        );
    }
    if overflow.hidden_right > 0 {
        paint_overflow_pill(
            OverflowSide::Right,
            overflow.hidden_right,
            bar_width,
            bar_height,
            measurer,
            &mut out,
        );
    }

    if let Some(ghost) = drag
        && let Some(tab) = layout
            .groups
            .iter()
            .flat_map(|g| &g.tabs)
            .find(|t| t.id == ghost.tab)
    {
        let exited = workspace.tab(tab.id).is_some_and(|t| t.is_exited());
        let is_active = active == Some(tab.id);
        let (bg, border, text_color) = tab_colors(exited, is_active);
        let ghost_rect = Rect {
            x: ghost.screen_x,
            y: tab.rect.y,
            width: tab.rect.width,
            height: tab.rect.height,
        };
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: ghost_rect,
            radius: 6.0,
            color: bg,
            border_color: border,
            border_width: 1.0,
        }));
        let dot_reserve = if tab.indicator.is_some() {
            INDICATOR_DOT_SIZE + style.internal_gap
        } else {
            0.0
        };
        let label_y = ghost_rect.y + (ghost_rect.height - style.font_size) / 2.0;
        out.push(Primitive::Text(TextRun {
            origin: (ghost_rect.x + style.padding_left + dot_reserve, label_y),
            text: tab.label.clone(),
            font: LABEL_FONT,
            size_px: style.font_size,
            color: text_color,
        }));
    }

    out
}

fn tab_colors(exited: bool, is_active: bool) -> (Color, Color, Color) {
    if exited {
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
    }
}

fn shift(rect: Rect, dx: f32) -> Rect {
    Rect {
        x: rect.x + dx,
        ..rect
    }
}

/// Indicador de abas fora da vista (espec §2.18, RF-1.19): chevron + a
/// mesma pílula de contagem da §2.4, ancorado por dentro da ponta da
/// trilha, fora do recorte de rolagem.
fn paint_overflow_pill(
    side: OverflowSide,
    count: usize,
    bar_width: f32,
    bar_height: f32,
    measurer: &mut porecatu_render::TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let rect = tab_bar::overflow_pill_rect(side, bar_width, bar_height);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: OVERFLOW_COUNT_RADIUS,
        color: palette::OVERFLOW_COUNT_BACKGROUND,
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));

    let chevron = match side {
        OverflowSide::Left => "\u{2039}",
        OverflowSide::Right => "\u{203a}",
    };
    let count_text = count.to_string();
    let chevron_width = measurer.measure_width(chevron, ICON_FONT, OVERFLOW_CHEVRON_SIZE);
    let count_width = measurer.measure_width(&count_text, ICON_FONT, OVERFLOW_COUNT_FONT_SIZE);
    let content_width = chevron_width + OVERFLOW_INNER_GAP + count_width;
    let start_x = rect.x + (rect.width - content_width) / 2.0;
    let mid_y = rect.y + rect.height / 2.0;

    out.push(Primitive::Text(TextRun {
        origin: (start_x, mid_y - OVERFLOW_CHEVRON_SIZE / 2.0),
        text: chevron.to_string(),
        font: ICON_FONT,
        size_px: OVERFLOW_CHEVRON_SIZE,
        color: palette::NEW_TAB_ICON,
    }));
    out.push(Primitive::Text(TextRun {
        origin: (
            start_x + chevron_width + OVERFLOW_INNER_GAP,
            mid_y - OVERFLOW_COUNT_FONT_SIZE / 2.0,
        ),
        text: count_text,
        font: ICON_FONT,
        size_px: OVERFLOW_COUNT_FONT_SIZE,
        color: palette::OVERFLOW_COUNT_TEXT,
    }));
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
