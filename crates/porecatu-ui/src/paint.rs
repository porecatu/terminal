// SPDX-License-Identifier: GPL-3.0-or-later

//! Traduz snapshot + paleta em primitivas de desenho (docs/arquitetura.md
//! seção 5) -- é aqui, não em `porecatu-render`, que "célula" e "grade"
//! deixam de existir e viram quad/run de texto.
//!
//! Simplificação assumida nesta etapa: um `TextRun` por trecho contíguo de
//! mesma cor/peso numa linha, deixado para o `glyphon`/`cosmic-text`
//! posicionar internamente -- não força cada glyph no `x` exato da célula.
//! Numa fonte monoespaçada isso costuma bater, mas pode acumular um desvio
//! de poucos pixels em linhas muito longas; corrigir (posicionar cada
//! glyph manualmente pela grade) fica para quando isso for visível na
//! prática.

use porecatu_render::{Color, FontFace, Primitive, Quad, Rect, TextRun};
use porecatu_term::{Cell, CellFlags, CellText, GridSnapshot};

use crate::palette;

#[derive(Debug, Clone, Copy)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

pub fn build_primitives(
    snapshot: &GridSnapshot,
    metrics: CellMetrics,
    font_size_px: f32,
) -> Vec<Primitive> {
    let cols = snapshot.cols;
    let mut primitives = Vec::new();

    for row in 0..snapshot.rows {
        let row_y = row as f32 * metrics.height;
        paint_row_backgrounds(snapshot, row, cols, row_y, metrics, &mut primitives);
        paint_row_text(
            snapshot,
            row,
            cols,
            row_y,
            metrics,
            font_size_px,
            &mut primitives,
        );
    }

    if let Some((row, col)) = snapshot.cursor.position
        && snapshot.cursor.visible
    {
        primitives.push(Primitive::Quad(Quad {
            rect: Rect {
                x: col as f32 * metrics.width,
                y: row as f32 * metrics.height,
                width: metrics.width,
                height: metrics.height,
            },
            color: palette::TERM_CURSOR,
        }));
    }

    primitives
}

fn paint_row_backgrounds(
    snapshot: &GridSnapshot,
    row: usize,
    cols: usize,
    row_y: f32,
    metrics: CellMetrics,
    out: &mut Vec<Primitive>,
) {
    for col in 0..cols {
        let cell = &snapshot.cells[row * cols + col];
        let (_, bg) = resolved_colors(cell);
        if bg != palette::TERM_BACKGROUND {
            out.push(Primitive::Quad(Quad {
                rect: Rect {
                    x: col as f32 * metrics.width,
                    y: row_y,
                    width: metrics.width,
                    height: metrics.height,
                },
                color: bg,
            }));
        }
    }
}

fn paint_row_text(
    snapshot: &GridSnapshot,
    row: usize,
    cols: usize,
    row_y: f32,
    metrics: CellMetrics,
    font_size_px: f32,
    out: &mut Vec<Primitive>,
) {
    let mut col = 0;
    while col < cols {
        let cell = &snapshot.cells[row * cols + col];
        if cell.flags.contains(CellFlags::WIDE_SPACER) {
            col += 1;
            continue;
        }

        let (fg, _) = resolved_colors(cell);
        let bold = cell.flags.contains(CellFlags::BOLD);
        let start_col = col;
        let mut text = String::new();

        while col < cols {
            let cell = &snapshot.cells[row * cols + col];
            if cell.flags.contains(CellFlags::WIDE_SPACER) {
                col += 1;
                continue;
            }
            let (cell_fg, _) = resolved_colors(cell);
            let cell_bold = cell.flags.contains(CellFlags::BOLD);
            if cell_fg != fg || cell_bold != bold {
                break;
            }
            push_cell_text(cell, &snapshot.clusters, &mut text);
            col += 1;
        }

        if !text.trim().is_empty() {
            out.push(Primitive::Text(TextRun {
                origin: (start_col as f32 * metrics.width, row_y),
                text,
                font: FontFace::Mono { bold },
                size_px: font_size_px,
                color: fg,
            }));
        }
    }
}

fn push_cell_text(cell: &Cell, clusters: &str, out: &mut String) {
    match cell.text {
        CellText::Char(c) => out.push(c),
        CellText::Cluster { start, end } => out.push_str(&clusters[start as usize..end as usize]),
    }
}

fn resolved_colors(cell: &Cell) -> (Color, Color) {
    let mut fg = palette::resolve(
        cell.fg,
        palette::TERM_FOREGROUND,
        palette::TERM_BACKGROUND,
        true,
    );
    let mut bg = palette::resolve(
        cell.bg,
        palette::TERM_FOREGROUND,
        palette::TERM_BACKGROUND,
        false,
    );
    if cell.flags.contains(CellFlags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    (fg, bg)
}
