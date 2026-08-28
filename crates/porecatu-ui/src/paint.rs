// SPDX-License-Identifier: GPL-3.0-or-later

//! Traduz snapshot + paleta em primitivas de desenho (docs/arquitetura.md
//! seção 5) -- é aqui, não em `porecatu-render`, que "célula" e "grade"
//! deixam de existir e viram quad/run de texto.
//!
//! Um `TextRun` por trecho contíguo de mesma cor/peso numa linha, deixado
//! para o `glyphon`/`cosmic-text` posicionar internamente -- **enquanto
//! todo caractere do trecho avançar uma célula**, que é o que a face mono
//! garante para o que ela cobre.
//!
//! O que ela não cobre vem por fallback do sistema (ADR-0016), e aí o
//! avanço é o da fonte de fallback: 1.26 célula num braille, 2.29 num
//! triângulo geométrico. Num run compartilhado isso empurra todo o resto
//! da linha para a direita -- os gráficos de braille do `btop` saem
//! escorrendo. Então o trecho **quebra** nesses caracteres: cada um vira
//! um `TextRun` próprio, ancorado no `x` da célula dele e reduzido para
//! caber nela. É o "posicionar cada glyph manualmente pela grade" que
//! estava anotado aqui como pendente até virar visível na prática.
//!
//! A quebra é por caractere que precisa dela, não por célula: uma linha de
//! ASCII continua sendo um run só.

use porecatu_render::{Color, FontFace, Primitive, Quad, Rect, RoundedQuad, TextMeasurer, TextRun};
use porecatu_term::{Cell, CellFlags, CellText, GridSnapshot, SelectionSpan};

use crate::palette;
use crate::tab_bar::TabBarStyle;

#[derive(Debug, Clone, Copy)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

/// Respiro entre a borda do quadro (janela) e o box arredondado do
/// terminal -- pedido do usuário, "mesmo espaço que tem entre as abas e a
/// borda da trilha do topo do app", ou seja o mesmo `trilha_padding` da
/// barra (§2.2/§2.5), não um valor novo. **Só nos três lados que não
/// encostam na barra** (esquerda, direita, base): em cima o box começa
/// colado em `bar_height`, sem gap -- um gap ali é uma linha visível entre
/// a trilha e o terminal, pedido do usuário para eliminar (antes desenhava
/// a margem nos quatro lados).
pub const TERMINAL_BOX_MARGIN: f32 = TabBarStyle::DEFAULT.trilha_padding;
/// Padding extra **dentro** do box, entre a borda dele e a grade em si, nos
/// quatro lados -- pedido do usuário ("mais um padding antes do bloco
/// interno"), dobrado a pedido do usuário na revisão seguinte. Base:
/// `wrapper_padding` (§2.3), no mesmo espírito de `TERMINAL_BOX_MARGIN`:
/// nada de valor novo, só multiplicado pelo fator que o usuário pediu.
pub const TERMINAL_BOX_PADDING: f32 = TabBarStyle::DEFAULT.wrapper_padding * 2.0;
/// Espec §2.5: "raio 6" -- o mesmo raio das abas ("os blocos das abas"),
/// pedido do usuário para o box do terminal.
pub const TERMINAL_BOX_CORNER_RADIUS: f32 = 6.0;

/// Retângulo do box arredondado do terminal: a área abaixo da barra
/// (`bar_height`), colado nela em cima e recuado por [`TERMINAL_BOX_MARGIN`]
/// nos outros três lados.
pub fn terminal_box_rect(bar_height: f32, logical_width: f32, logical_height: f32) -> Rect {
    Rect {
        x: TERMINAL_BOX_MARGIN,
        y: bar_height,
        width: (logical_width - TERMINAL_BOX_MARGIN * 2.0).max(0.0),
        height: (logical_height - bar_height - TERMINAL_BOX_MARGIN).max(0.0),
    }
}

/// Retângulo onde a grade em si é desenhada: [`terminal_box_rect`] recuado
/// por [`TERMINAL_BOX_PADDING`] nos quatro lados -- fonte única para
/// `build_primitives` e para quem precisa saber onde a grade começa/termina
/// fora dele (`grid_size`/`cell_at_cursor` em `lib.rs`), pra não duplicar a
/// conta em dois lugares (a mesma fórmula copiada em dois lugares é a
/// armadilha registrada em `chrome::bar_height`).
pub fn terminal_content_rect(bar_height: f32, logical_width: f32, logical_height: f32) -> Rect {
    let box_rect = terminal_box_rect(bar_height, logical_width, logical_height);
    Rect {
        x: box_rect.x + TERMINAL_BOX_PADDING,
        y: box_rect.y + TERMINAL_BOX_PADDING,
        width: (box_rect.width - TERMINAL_BOX_PADDING * 2.0).max(0.0),
        height: (box_rect.height - TERMINAL_BOX_PADDING * 2.0).max(0.0),
    }
}

/// Constrói as primitivas do box arredondado do terminal e da grade lá
/// dentro. `box_rect`: [`terminal_box_rect`] -- a grade começa
/// [`TERMINAL_BOX_PADDING`] adiante da borda do box, nos dois eixos.
pub fn build_primitives(
    snapshot: &GridSnapshot,
    metrics: CellMetrics,
    font_size_px: f32,
    box_rect: Rect,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let cols = snapshot.cols;
    let mut primitives = Vec::new();

    primitives.push(Primitive::RoundedQuad(RoundedQuad {
        rect: box_rect,
        radius: TERMINAL_BOX_CORNER_RADIUS,
        color: palette::TERM_BACKGROUND,
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));

    let x_offset = box_rect.x + TERMINAL_BOX_PADDING;
    let y_offset = box_rect.y + TERMINAL_BOX_PADDING;

    for row in 0..snapshot.rows {
        let row_y = y_offset + row as f32 * metrics.height;
        paint_row_backgrounds(
            snapshot,
            row,
            cols,
            x_offset,
            row_y,
            metrics,
            &mut primitives,
        );
        paint_row_text(
            snapshot,
            row,
            cols,
            x_offset,
            row_y,
            metrics,
            font_size_px,
            measurer,
            &mut primitives,
        );
    }

    if let Some((row, col)) = snapshot.cursor.position
        && snapshot.cursor.visible
    {
        primitives.push(Primitive::Quad(Quad {
            rect: Rect {
                x: x_offset + col as f32 * metrics.width,
                y: y_offset + row as f32 * metrics.height,
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
    x_offset: f32,
    row_y: f32,
    metrics: CellMetrics,
    out: &mut Vec<Primitive>,
) {
    for col in 0..cols {
        let cell = &snapshot.cells[row * cols + col];
        let selected = is_selected(snapshot.selection, row, col);
        let (_, bg) = resolved_colors(cell, selected);
        if bg != palette::TERM_BACKGROUND {
            out.push(Primitive::Quad(Quad {
                rect: Rect {
                    x: x_offset + col as f32 * metrics.width,
                    y: row_y,
                    width: metrics.width,
                    height: metrics.height,
                },
                color: bg,
            }));
        }
    }
}

/// Folga na comparação entre o avanço de um caractere e a largura da
/// célula. O avanço vem do shaping em ponto flutuante; um caractere da
/// própria face mono bate na casa do centésimo, um de fallback erra por
/// muito mais que isto.
const ADVANCE_TOLERANCE: f32 = 0.05;

/// Quantas células este caractere deveria ocupar segundo a grade -- duas
/// para largura dupla (a segunda vem como `WIDE_SPACER`, decisão 4 da
/// seção 4.1 do `porecatu-term`), uma para o resto.
fn cell_span(cell: &Cell) -> f32 {
    if cell.flags.contains(CellFlags::WIDE) {
        2.0
    } else {
        1.0
    }
}

/// Se este caractere pode viajar num run compartilhado: o avanço dele tem
/// de ser o que a grade reservou para ele. Cluster (grafema composto)
/// nunca pode -- o avanço dele não é o de um caractere só, e o cache de
/// `advance_em` é por caractere.
fn fits_the_grid(
    cell: &Cell,
    bold: bool,
    metrics: CellMetrics,
    font_size_px: f32,
    measurer: &mut TextMeasurer,
) -> bool {
    let CellText::Char(ch) = cell.text else {
        return false;
    };
    let expected = metrics.width * cell_span(cell);
    let advance = measurer.advance_em(ch, FontFace::Mono { bold }) * font_size_px;
    (advance - expected).abs() <= ADVANCE_TOLERANCE
}

#[allow(clippy::too_many_arguments)]
fn paint_row_text(
    snapshot: &GridSnapshot,
    row: usize,
    cols: usize,
    x_offset: f32,
    row_y: f32,
    metrics: CellMetrics,
    font_size_px: f32,
    measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let mut col = 0;
    while col < cols {
        let cell = &snapshot.cells[row * cols + col];
        if cell.flags.contains(CellFlags::WIDE_SPACER) {
            col += 1;
            continue;
        }

        let selected = is_selected(snapshot.selection, row, col);
        let (fg, _) = resolved_colors(cell, selected);
        let bold = cell.flags.contains(CellFlags::BOLD);

        // Caractere que não avança o que a grade reservou sai sozinho,
        // ancorado no `x` da célula e encolhido para caber nela -- senão
        // empurraria o resto do run e transbordaria para a célula
        // seguinte.
        if !fits_the_grid(cell, bold, metrics, font_size_px, measurer) {
            let mut text = String::new();
            push_cell_text(cell, &snapshot.clusters, &mut text);
            if !text.trim().is_empty() {
                let target = metrics.width * cell_span(cell);
                let size_px = fitted_size(&text, bold, font_size_px, target, measurer);
                out.push(Primitive::Text(TextRun {
                    origin: (x_offset + col as f32 * metrics.width, row_y),
                    text,
                    font: FontFace::Mono { bold },
                    size_px,
                    color: fg,
                }));
            }
            col += 1;
            continue;
        }

        let start_col = col;
        let mut text = String::new();

        while col < cols {
            let cell = &snapshot.cells[row * cols + col];
            if cell.flags.contains(CellFlags::WIDE_SPACER) {
                col += 1;
                continue;
            }
            let cell_selected = is_selected(snapshot.selection, row, col);
            let (cell_fg, _) = resolved_colors(cell, cell_selected);
            let cell_bold = cell.flags.contains(CellFlags::BOLD);
            if cell_fg != fg || cell_bold != bold {
                break;
            }
            if !fits_the_grid(cell, bold, metrics, font_size_px, measurer) {
                break;
            }
            push_cell_text(cell, &snapshot.clusters, &mut text);
            col += 1;
        }

        if !text.trim().is_empty() {
            out.push(Primitive::Text(TextRun {
                origin: (x_offset + start_col as f32 * metrics.width, row_y),
                text,
                font: FontFace::Mono { bold },
                size_px: font_size_px,
                color: fg,
            }));
        }
    }
}

/// Tamanho em que `text` cabe em `target_width`. Só encolhe: um glyph de
/// fallback mais estreito que a célula fica onde está, porque esticá-lo
/// deformaria o desenho sem ganhar alinhamento nenhum -- o `x` da próxima
/// célula não depende dele.
fn fitted_size(
    text: &str,
    bold: bool,
    font_size_px: f32,
    target_width: f32,
    measurer: &mut TextMeasurer,
) -> f32 {
    let font = FontFace::Mono { bold };
    let advance = measurer.measure_width(text, font, font_size_px);
    if advance <= target_width || advance <= f32::EPSILON {
        return font_size_px;
    }
    font_size_px * (target_width / advance)
}

fn push_cell_text(cell: &Cell, clusters: &str, out: &mut String) {
    match cell.text {
        CellText::Char(c) => out.push(c),
        CellText::Cluster { start, end } => out.push_str(&clusters[start as usize..end as usize]),
    }
}

/// Se `(row, col)` está dentro do span de seleção -- retangular pra
/// `is_block`, por linha lógica (início/meio/fim) pros outros três modos.
fn is_selected(selection: Option<SelectionSpan>, row: usize, col: usize) -> bool {
    let Some(sel) = selection else {
        return false;
    };
    if row < sel.start_row || row > sel.end_row {
        return false;
    }
    if sel.is_block || sel.start_row == sel.end_row {
        return col >= sel.start_col && col <= sel.end_col;
    }
    if row == sel.start_row {
        return col >= sel.start_col;
    }
    if row == sel.end_row {
        return col <= sel.end_col;
    }
    true // linha inteira entre a primeira e a última da seleção
}

fn resolved_colors(cell: &Cell, selected: bool) -> (Color, Color) {
    if selected {
        // Seleção domina a cor da célula -- não combina com `INVERSE`
        // nem com a cor original; é o mesmo comportamento da maioria dos
        // terminais (destaque uniforme, independente do que estava sob
        // ele).
        return (
            palette::TERM_SELECTION_FOREGROUND,
            palette::TERM_SELECTION_BACKGROUND,
        );
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use porecatu_term::GridSnapshot;

    const SIZE: f32 = 14.0;

    /// A célula sai da métrica da face mono, como em runtime -- fixar um
    /// número aqui amarraria o teste à largura de avanço de uma fonte
    /// específica, e foi exatamente o que quebrou os quatro testes na
    /// troca da IBM Plex pela Iosevka (ADR-0025), que é mais estreita.
    fn cell(m: &mut porecatu_render::TextMeasurer) -> CellMetrics {
        let (width, height) = m.measure_mono_cell(SIZE, SIZE * 1.75);
        CellMetrics { width, height }
    }

    fn snapshot(text: &str) -> GridSnapshot {
        let chars: Vec<char> = text.chars().collect();
        let cells = chars
            .iter()
            .map(|&c| Cell {
                text: CellText::Char(c),
                ..Cell::default()
            })
            .collect();
        GridSnapshot {
            cols: chars.len(),
            rows: 1,
            cells,
            ..GridSnapshot::default()
        }
    }

    fn runs(primitives: &[Primitive]) -> Vec<(f32, String, f32)> {
        primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Text(run) => Some((run.origin.0, run.text.clone(), run.size_px)),
                _ => None,
            })
            .collect()
    }

    /// Linha de ASCII continua sendo um run só -- a quebra por caractere
    /// existe para o que sai da face mono, e não pode custar um run por
    /// célula no caso comum.
    /// Retângulo de teste do box do terminal -- grande o bastante para não
    /// recortar nada, com origem em (0, 0) para que o `x_offset`/`y_offset`
    /// resultante seja só [`TERMINAL_BOX_PADDING`], sem o `TERMINAL_BOX_MARGIN`
    /// (já descontado por quem monta `box_rect` em runtime).
    fn test_box_rect() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 1000.0,
        }
    }

    #[test]
    fn plain_ascii_line_stays_a_single_run() {
        let mut m = porecatu_render::TextMeasurer::new();
        let cell = cell(&mut m);
        let out = build_primitives(
            &snapshot("cargo build"),
            cell,
            SIZE,
            test_box_rect(),
            &mut m,
        );
        let runs = runs(&out);
        assert_eq!(runs.len(), 1, "esperava um run só, veio {runs:?}");
        assert_eq!(runs[0].1, "cargo build");
        assert_eq!(runs[0].2, SIZE);
    }

    /// Box drawing, blocos e **braille** a Iosevka cobre inteiros, no
    /// avanço da célula (ADR-0025) -- nenhum deles pode disparar a
    /// quebra, senão as molduras do Claude Code e os gráficos do `btop`
    /// virariam um run por caractere.
    #[test]
    fn box_drawing_and_braille_travel_in_the_shared_run() {
        let mut m = porecatu_render::TextMeasurer::new();
        let out = build_primitives(
            &snapshot("\u{250C}\u{2500}\u{2510}\u{2588}\u{283F}\u{28FF}"),
            cell(&mut porecatu_render::TextMeasurer::new()),
            SIZE,
            test_box_rect(),
            &mut m,
        );
        assert_eq!(runs(&out).len(), 1);
    }

    /// Um caractere fora da face embutida no meio da linha. Com a IBM
    /// Plex Mono esse era o caso do braille dos gráficos do `btop`; a
    /// Iosevka cobre braille (ADR-0025), então o teste usa um emoji, que
    /// segue vindo do sistema por decisão (nem a Iosevka nem o recorte o
    /// incluem). O que se verifica é o mecanismo: run próprio, ancorado
    /// no `x` da célula, com o texto seguinte de volta na célula certa.
    #[test]
    fn fallback_char_gets_its_own_run_anchored_to_its_cell() {
        let mut m = porecatu_render::TextMeasurer::new();
        let cell = cell(&mut m);
        let out = build_primitives(
            &snapshot("ab\u{1F600}cd"),
            cell,
            SIZE,
            test_box_rect(),
            &mut m,
        );
        let runs = runs(&out);
        assert_eq!(runs.len(), 3, "esperava tres runs, veio {runs:?}");

        assert_eq!(runs[0].1, "ab");
        assert_eq!(runs[0].0, TERMINAL_BOX_PADDING);

        assert_eq!(runs[1].1, "\u{1F600}");
        assert_eq!(
            runs[1].0,
            TERMINAL_BOX_PADDING + 2.0 * cell.width,
            "glyph de fallback fora da celula dele"
        );

        assert_eq!(runs[2].1, "cd");
        assert_eq!(
            runs[2].0,
            TERMINAL_BOX_PADDING + 3.0 * cell.width,
            "texto depois do fallback saiu da grade"
        );
    }

    /// E o glyph de fallback é encolhido para não invadir a célula
    /// seguinte -- com o avanço da fonte de onde ele veio, ocuparia mais
    /// de uma.
    #[test]
    fn fallback_char_is_shrunk_to_fit_its_cell() {
        let mut m = porecatu_render::TextMeasurer::new();
        let cell = cell(&mut m);
        let out = build_primitives(&snapshot("\u{1F600}"), cell, SIZE, test_box_rect(), &mut m);
        let runs = runs(&out);
        assert_eq!(runs.len(), 1);
        let size = runs[0].2;
        let width = m.measure_width(&runs[0].1, FontFace::Mono { bold: false }, size);
        assert!(
            width <= cell.width + ADVANCE_TOLERANCE,
            "glyph de fallback com {width} numa celula de {}",
            cell.width
        );
    }
}
