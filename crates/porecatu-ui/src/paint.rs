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
use porecatu_term::{
    Cell, CellFlags, CellText, CursorShape, GridSnapshot, HyperlinkSpan, OccurrenceSpan,
    SelectionSpan,
};

use crate::box_glyphs;
use crate::chrome::push_shadow;
use crate::palette::{self, ResolvedTermPalette, TRANSPARENT};
use crate::tab_bar::TabBarStyle;

#[derive(Debug, Clone, Copy)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

/// Cursor já resolvido por `lib.rs` -- cor (`[terminal.colors] cursor`, ou a
/// cor do grupo da aba ativa se `follows_group_color`, RF-5.22 comentário),
/// espessura de traço (`[terminal.cursor] width`, usada por beam/cursor-
/// underline/contorno do bloco vazado -- **não** pelo sublinhado SGR, que
/// deriva a espessura do `font_size_px`) e se o formato bloco deve sair
/// vazado (`unfocused_hollow`, RF-5.24: janela sem foco). `paint.rs` não
/// conhece `Workspace` nem foco de janela -- por isso a decisão chega já
/// tomada, não como `&Config`.
#[derive(Debug, Clone, Copy)]
pub struct CursorAppearance {
    pub color: Color,
    pub width: f32,
    pub hollow: bool,
}

/// Altura do cursor bloco, em fração de `font_size_px` -- **não**
/// `metrics.height` (a altura de linha, com o `line_height` configurável
/// embutido pra dar respiro entre linhas). Um cursor do tamanho da linha
/// inteira sobra bem abaixo do glyph, na folga que o line-height reserva
/// pra próxima linha -- visível assim que o quadro do terminal parou de
/// escondê-lo atrás de si (`frame::GeometryPrimitive`).
///
/// Vem do mockup (`docs/design/mockup-estatico.html`, `.caret-blk`):
/// 15px de cursor sobre 12.5px de fonte -- a proporção 15/12.5, não os
/// 15px em si (o mockup ainda é IBM Plex, seção 4.4 da espec. visual). E
/// bate com o `1.2` que `text.rs` usa como line-height do **glyph**
/// (`Metrics::new(size_px, size_px * 1.2)`) -- não coincidência, é a
/// mesma caixa. Por isso o cursor fica colado no topo da linha (`row_y`),
/// não centralizado em `metrics.height`: a caixa do glyph também começa
/// ali, não no meio da folga que o line-height reserva embaixo.
const CURSOR_HEIGHT_RATIO: f32 = 1.2;

/// Retângulo do box arredondado do terminal: a área abaixo da barra
/// (`bar_height`), colado nela em cima e recuado por
/// `style.terminal_frame_margin` (`[appearance.terminal_frame] margin`) nos
/// outros três lados -- pedido do usuário, "mesmo espaço que tem entre as
/// abas e a borda da trilha do topo do app". **Só nos três lados que não
/// encostam na barra** (esquerda, direita, base): em cima o box começa
/// colado em `bar_height`, sem gap -- um gap ali é uma linha visível entre
/// a trilha e o terminal, pedido do usuário para eliminar.
///
/// `status_bar_height` (`status_bar::height`, ADR-0048 §5) é a faixa do
/// rodapé, e quando ela existe o box **encosta nela**, sem `margin` --
/// exatamente como já encosta na barra de abas em cima, e pela mesma
/// razão: a faixa é outra barra de chrome, não a borda da janela, e um
/// vão entre as duas lê como uma linha a mais. Com a barra desligada o
/// `margin` da base volta a valer contra a borda da janela, e a conta é
/// byte a byte a de antes de a barra existir.
///
/// A barra **encolhe** a grade: é esta linha que tira as linhas do PTY, e
/// é a diferença deliberada contra a barra de busca, que sobrepõe
/// (ADR-0041) por ser transitória.
pub fn terminal_box_rect(
    style: &TabBarStyle,
    bar_height: f32,
    status_bar_height: f32,
    logical_width: f32,
    logical_height: f32,
) -> Rect {
    let margin = style.terminal_frame_margin;
    let bottom_margin = if status_bar_height > 0.0 { 0.0 } else { margin };
    Rect {
        x: margin,
        y: bar_height,
        width: (logical_width - margin * 2.0).max(0.0),
        height: (logical_height - bar_height - status_bar_height - bottom_margin).max(0.0),
    }
}

/// Retângulo onde a grade em si é desenhada: [`terminal_box_rect`] recuado
/// por `style.terminal_frame_padding` (`[appearance.terminal_frame]
/// padding`) nos quatro lados -- fonte única para `build_primitives` e para
/// quem precisa saber onde a grade começa/termina fora dele
/// (`grid_size`/`cell_at_cursor` em `lib.rs`), pra não duplicar a conta em
/// dois lugares (a mesma fórmula copiada em dois lugares é a armadilha
/// registrada em `chrome::bar_height`).
pub fn terminal_content_rect(
    style: &TabBarStyle,
    bar_height: f32,
    status_bar_height: f32,
    logical_width: f32,
    logical_height: f32,
) -> Rect {
    let box_rect = terminal_box_rect(
        style,
        bar_height,
        status_bar_height,
        logical_width,
        logical_height,
    );
    let padding = style.terminal_frame_padding;
    Rect {
        x: box_rect.x + padding,
        y: box_rect.y + padding,
        width: (box_rect.width - padding * 2.0).max(0.0),
        height: (box_rect.height - padding * 2.0).max(0.0),
    }
}

/// Topo lógico da linha `row` -- sempre esta mesma expressão, nunca
/// recomputada como "`row_y` da linha anterior + `metrics.height`": duas
/// contas matematicamente iguais podem divergir por 1 ULP de ponto
/// flutuante dependendo do valor, e depois do arredondamento por quad em
/// `quad.rs` (`snap_rect_to_physical_pixels`, que arredonda os dois cantos
/// de **um** quad, sem saber que o vizinho deveria compartilhar a mesma
/// borda) isso vira uma costura de 1px entre duas linhas -- só numa linha
/// específica, a que por acaso cai bem na fronteira de arredondamento, e
/// que muda de lugar se `y_offset` mudar (barra de status ligada/desligada,
/// redimensionamento). Usar a mesma função para "o fim da linha `row`" e "o
/// início da linha `row + 1`" garante que os dois cálculos cheguem no
/// mesmo `f32`, então `quad.rs` arredonda os dois pro mesmo pixel físico.
fn row_top(y_offset: f32, row: usize, metrics: CellMetrics) -> f32 {
    y_offset + row as f32 * metrics.height
}

/// Mesma razão de [`row_top`], eixo X -- já não tinha sintoma visível (o
/// avanço de coluna passa por `align_mono_advance_to`, que aparentemente
/// evita a fronteira de arredondamento na prática), mas nada garante isso
/// pra todo tamanho de fonte/escala configurável, então `paint_row_backgrounds`
/// usa esta função nos dois lados de cada célula pela mesma razão.
fn col_left(x_offset: f32, col: usize, metrics: CellMetrics) -> f32 {
    x_offset + col as f32 * metrics.width
}

/// Constrói as primitivas do box arredondado do terminal e da grade lá
/// dentro. `box_rect`: [`terminal_box_rect`] -- a grade começa
/// `style.terminal_frame_padding` adiante da borda do box, nos dois eixos.
#[allow(clippy::too_many_arguments)]
pub fn build_primitives(
    snapshot: &GridSnapshot,
    metrics: CellMetrics,
    font_size_px: f32,
    box_rect: Rect,
    style: &TabBarStyle,
    term_pal: &ResolvedTermPalette,
    cursor: CursorAppearance,
    measurer: &mut TextMeasurer,
    hyperlink_hover: &[HyperlinkSpan],
) -> Vec<Primitive> {
    let cols = snapshot.cols;
    let mut primitives = Vec::new();

    if style.terminal_frame_shadow_enabled {
        push_shadow(
            &mut primitives,
            box_rect,
            style.terminal_frame_corner_radius,
        );
    }
    primitives.push(Primitive::RoundedQuad(RoundedQuad {
        rect: box_rect,
        radius: style.terminal_frame_corner_radius,
        color: term_pal.background,
        border_color: TRANSPARENT,
        border_width: 0.0,
    }));

    let x_offset = box_rect.x + style.terminal_frame_padding;
    let y_offset = box_rect.y + style.terminal_frame_padding;

    for row in 0..snapshot.rows {
        let row_y = row_top(y_offset, row, metrics);
        // Fim desta linha == início da próxima, pela mesma função --
        // ver o comentário de `row_top`.
        let row_bottom = row_top(y_offset, row + 1, metrics);
        paint_row_backgrounds(
            snapshot,
            row,
            cols,
            x_offset,
            row_y,
            row_bottom,
            metrics,
            term_pal,
            &mut primitives,
        );
        paint_row_text(
            snapshot,
            row,
            cols,
            x_offset,
            row_y,
            row_bottom,
            metrics,
            font_size_px,
            term_pal,
            measurer,
            &mut primitives,
        );
        paint_row_underlines(
            snapshot,
            row,
            cols,
            x_offset,
            row_y,
            metrics,
            font_size_px,
            term_pal,
            hyperlink_hover,
            &mut primitives,
        );
    }

    if let Some((row, col)) = snapshot.cursor.position
        && snapshot.cursor.visible
    {
        // Sem centralizar em `metrics.height`: o glyph não ocupa a linha
        // inteira (ela tem `line_height` de folga, toda embaixo). `text.rs`
        // monta o buffer do glyph com `Metrics::new(size_px, size_px *
        // 1.2)` -- a caixa do texto começa em `row_y`, não no meio da linha
        // -- e é esse `1.2` que `CURSOR_HEIGHT_RATIO` replica.
        let cursor_height = font_size_px * CURSOR_HEIGHT_RATIO;
        let row_y = row_top(y_offset, row, metrics);
        let cell_x = col_left(x_offset, col, metrics);
        primitives.push(cursor_primitive(
            snapshot.cursor.shape,
            cursor,
            Rect {
                x: cell_x,
                y: row_y,
                width: metrics.width,
                height: cursor_height,
            },
        ));
    }

    primitives
}

/// Formato do cursor (`snapshot.cursor.shape`, RF-5.22/RF-5.25 -- DECSCUSR
/// do programa já venceu o default da config dentro do motor, ver
/// `porecatu-term::TermEngine`) sobre a célula `cell_rect`. `Hidden` nunca
/// chega aqui -- `build_primitives` só chama isto com `snapshot.cursor.
/// visible`, e `visible` é `shape != Hidden` (`porecatu-term::engine`).
fn cursor_primitive(shape: CursorShape, cursor: CursorAppearance, cell_rect: Rect) -> Primitive {
    // `HollowBlock` é o que o próprio DECSCUSR pode pedir (motor completo,
    // mesmo sem uso hoje) -- vazado independente de `cursor.hollow`
    // (RF-5.24 é só o efeito de "sem foco" sobre a forma configurada).
    match shape {
        CursorShape::Hidden => unreachable!("chamado só com `visible == true`"),
        CursorShape::Block if !cursor.hollow => Primitive::Quad(Quad {
            rect: cell_rect,
            color: cursor.color,
        }),
        CursorShape::Block | CursorShape::HollowBlock => Primitive::RoundedQuad(RoundedQuad {
            rect: cell_rect,
            radius: 0.0,
            color: TRANSPARENT,
            border_color: cursor.color,
            border_width: cursor.width,
        }),
        CursorShape::Beam => Primitive::Quad(Quad {
            rect: Rect {
                width: cursor.width,
                ..cell_rect
            },
            color: cursor.color,
        }),
        CursorShape::Underline => Primitive::Quad(Quad {
            rect: Rect {
                y: cell_rect.y + cell_rect.height - cursor.width,
                height: cursor.width,
                ..cell_rect
            },
            color: cursor.color,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_row_backgrounds(
    snapshot: &GridSnapshot,
    row: usize,
    cols: usize,
    x_offset: f32,
    row_y: f32,
    row_bottom: f32,
    metrics: CellMetrics,
    term_pal: &ResolvedTermPalette,
    out: &mut Vec<Primitive>,
) {
    for col in 0..cols {
        let cell = &snapshot.cells[row * cols + col];
        let selected = is_selected(snapshot.selection, row, col);
        let occurrence = occurrence_at(&snapshot.occurrences, row, col);
        let (_, bg) = resolved_colors(cell, selected, occurrence, term_pal);
        if bg != term_pal.background {
            // `col_left`/`row_y`/`row_bottom`, não `col as f32 *
            // metrics.width`/`metrics.height` soltos -- a célula vizinha
            // (linha de baixo, coluna ao lado) usa a mesma função pro
            // mesmo limite, então os dois fecham no mesmo pixel físico
            // depois do arredondamento em `quad.rs` (ver `row_top`).
            let x0 = col_left(x_offset, col, metrics);
            let x1 = col_left(x_offset, col + 1, metrics);
            out.push(Primitive::Quad(Quad {
                rect: Rect {
                    x: x0,
                    y: row_y,
                    width: x1 - x0,
                    height: row_bottom - row_y,
                },
                color: bg,
            }));
        }
    }
}

/// Folga na comparação de avanço, **em fração da em** -- não em pixels.
/// O avanço vem do shaping em ponto flutuante; um caractere da própria
/// face mono bate na casa do centésimo, um de fallback erra por muito
/// mais que isto.
///
/// A unidade importa: comparar em pixels obriga a escolher entre a
/// largura de célula **natural** e a **arredondada ao pixel físico** por
/// `snap_cell_metrics_to_pixel_grid` (`lib.rs`), e as duas diferem por até
/// meio pixel -- dez vezes esta folga. Foi o que aconteceu quando o
/// snapping entrou: toda célula passou a reprovar, cada caractere virou um
/// `TextRun` próprio e a grade inteira foi re-shapada por frame. Em em a
/// pergunta não depende de tamanho de fonte nem de escala de janela.
const ADVANCE_TOLERANCE: f32 = 0.05;

/// Caractere de referência da grade: o mesmo que
/// `TextMeasurer::measure_mono_cell` usa para derivar a largura de célula.
/// Comparar contra o avanço **dele** é o que torna o teste independente do
/// arredondamento aplicado depois à célula.
const GRID_REFERENCE_CHAR: char = 'M';

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
fn fits_the_grid(cell: &Cell, bold: bool, measurer: &mut TextMeasurer) -> bool {
    let CellText::Char(ch) = cell.text else {
        return false;
    };
    let font = FontFace::Mono { bold };
    // Os dois lados saem do cache de `advance_em`: depois do warmup isto
    // não shapa nada, e é o caminho percorrido uma vez por célula.
    let expected = measurer.advance_em(GRID_REFERENCE_CHAR, font) * cell_span(cell);
    (measurer.advance_em(ch, font) - expected).abs() <= ADVANCE_TOLERANCE
}

#[allow(clippy::too_many_arguments)]
fn paint_row_text(
    snapshot: &GridSnapshot,
    row: usize,
    cols: usize,
    x_offset: f32,
    row_y: f32,
    row_bottom: f32,
    metrics: CellMetrics,
    font_size_px: f32,
    term_pal: &ResolvedTermPalette,
    measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    // Mesma conta de `paint_row_underlines`: ~10% do em, mínimo 1px --
    // espessura de traço leve pro box-drawing (`box_glyphs::push`).
    let box_glyph_thickness = (font_size_px * 0.1).round().max(1.0);

    let mut col = 0;
    while col < cols {
        let cell = &snapshot.cells[row * cols + col];
        if cell.flags.contains(CellFlags::WIDE_SPACER) {
            col += 1;
            continue;
        }

        let selected = is_selected(snapshot.selection, row, col);
        let occurrence = occurrence_at(&snapshot.occurrences, row, col);
        let (fg, _) = resolved_colors(cell, selected, occurrence, term_pal);
        let bold = cell.flags.contains(CellFlags::BOLD);

        // Bloco/box-drawing (U+2580-259F, núcleo reto de U+2500-254B): sai
        // como `Quad` nosso, não como glyph da fonte -- ver
        // `box_glyphs`, motivado por `text_measurer.rs`
        // (`full_block_glyph_ink_coverage_of_its_advance_box`) mostrar que
        // a tinta rasterizada não bate com a caixa de linha que `text.rs`
        // usa pra posicionar cada linha.
        if let CellText::Char(ch) = cell.text
            && box_glyphs::is_covered(ch)
        {
            // `col_left`/`row_y`/`row_bottom`: mesma razão de
            // `paint_row_backgrounds` -- um bloco precisa fechar sem
            // costura contra o vizinho de cima/baixo/lado, mesmo quando
            // esse vizinho é outro bloco ou um fundo de célula comum.
            let x0 = col_left(x_offset, col, metrics);
            let x1 = col_left(x_offset, col + cell_span(cell) as usize, metrics);
            let cell_rect = Rect {
                x: x0,
                y: row_y,
                width: x1 - x0,
                height: row_bottom - row_y,
            };
            box_glyphs::push(ch, cell_rect, fg, box_glyph_thickness, out);
            col += 1;
            continue;
        }

        // Caractere que não avança o que a grade reservou sai sozinho,
        // ancorado no `x` da célula e encolhido para caber nela -- senão
        // empurraria o resto do run e transbordaria para a célula
        // seguinte.
        if !fits_the_grid(cell, bold, measurer) {
            let mut text = String::new();
            push_cell_text(cell, &snapshot.clusters, &mut text);
            if !text.trim().is_empty() {
                let target = metrics.width * cell_span(cell);
                let size_px = fitted_size(cell, &text, bold, font_size_px, target, measurer);
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
            let cell_occurrence = occurrence_at(&snapshot.occurrences, row, col);
            let (cell_fg, _) = resolved_colors(cell, cell_selected, cell_occurrence, term_pal);
            let cell_bold = cell.flags.contains(CellFlags::BOLD);
            if cell_fg != fg || cell_bold != bold {
                break;
            }
            if let CellText::Char(ch) = cell.text
                && box_glyphs::is_covered(ch)
            {
                // Sai do run pra ser tratado como quad no início do laço
                // externo -- `col` não avança, a próxima volta pega esta
                // mesma célula de novo.
                break;
            }
            if !fits_the_grid(cell, bold, measurer) {
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
///
/// `target_width` é a largura de célula **arredondada ao pixel físico**
/// (`snap_cell_metrics_to_pixel_grid` em `lib.rs`), ao contrário do teste
/// de `fits_the_grid`: aqui a pergunta é "cabe na célula desenhada?", e a
/// célula desenhada é a arredondada.
///
/// Caractere único resolve pelo cache de `advance_em`; só cluster (grafema
/// composto, raro) paga um shaping -- este caminho roda por célula de
/// fallback por frame, e uma tela de braille do `btop` é feita inteira
/// dele.
fn fitted_size(
    cell: &Cell,
    text: &str,
    bold: bool,
    font_size_px: f32,
    target_width: f32,
    measurer: &mut TextMeasurer,
) -> f32 {
    let font = FontFace::Mono { bold };
    let advance = match cell.text {
        CellText::Char(ch) => measurer.advance_em(ch, font) * font_size_px,
        CellText::Cluster { .. } => measurer.measure_width(text, font, font_size_px),
    };
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

/// Se `(row, col)` cai numa ocorrência de busca (ADR-0041 §4/§5) --
/// `Some(true)`/`Some(false)` conforme ela é a ativa ou não, `None` fora de
/// qualquer ocorrência. Mesma lógica de `is_selected` (linha única vs.
/// multi-linha), sem o caso `is_block` -- ocorrência nunca é retangular.
fn occurrence_at(occurrences: &[OccurrenceSpan], row: usize, col: usize) -> Option<bool> {
    occurrences.iter().find_map(|occ| {
        if row < occ.start_row || row > occ.end_row {
            return None;
        }
        let inside = if occ.start_row == occ.end_row {
            col >= occ.start_col && col <= occ.end_col
        } else if row == occ.start_row {
            col >= occ.start_col
        } else if row == occ.end_row {
            col <= occ.end_col
        } else {
            true
        };
        inside.then_some(occ.active)
    })
}

/// Sublinhado real (SGR, flag `UNDERLINE` -- nunca desenhado antes desta
/// etapa, achado ao implementar a affordance do ADR-0042 §3: a espec
/// citava \"o pintor já desenha\", e não havia primitiva nenhuma para isso)
/// e a affordance de hyperlink sob o modificador de abertura (mesma flag
/// visual, RF-11.11) -- um traço por trecho contíguo de mesma cor.
///
/// Espessura: ~10% de `font_size_px`, arredondado para o pixel físico mais
/// próximo, mínimo 1px. Antes reusava `cursor.width` (7 px por default),
/// que invadia o glyph (50% da fonte a 14 px). O cursor beam/underline-
/// cursor continua usando `cursor.width` -- só o sublinhado de texto SGR
/// é que agora usa essa espessura fina derivada da fonte.
#[allow(clippy::too_many_arguments)]
fn paint_row_underlines(
    snapshot: &GridSnapshot,
    row: usize,
    cols: usize,
    x_offset: f32,
    row_y: f32,
    metrics: CellMetrics,
    font_size_px: f32,
    term_pal: &ResolvedTermPalette,
    hyperlink_hover: &[HyperlinkSpan],
    out: &mut Vec<Primitive>,
) {
    // ~10% do em, arredondado para 1px mínimo -- convencional em terminais.
    let thickness = (font_size_px * 0.1).round().max(1.0);

    let underlined_at = |col: usize| -> bool {
        snapshot.cells[row * cols + col]
            .flags
            .contains(CellFlags::UNDERLINE)
            || hyperlink_hover_at(hyperlink_hover, row, col)
    };

    let mut col = 0;
    while col < cols {
        if !underlined_at(col) {
            col += 1;
            continue;
        }
        let start_col = col;
        let cell = &snapshot.cells[row * cols + col];
        let selected = is_selected(snapshot.selection, row, col);
        let occurrence = occurrence_at(&snapshot.occurrences, row, col);
        let (fg, _) = resolved_colors(cell, selected, occurrence, term_pal);

        while col < cols && underlined_at(col) {
            let cell = &snapshot.cells[row * cols + col];
            let cell_selected = is_selected(snapshot.selection, row, col);
            let cell_occurrence = occurrence_at(&snapshot.occurrences, row, col);
            let (cell_fg, _) = resolved_colors(cell, cell_selected, cell_occurrence, term_pal);
            if cell_fg != fg {
                break;
            }
            col += 1;
        }

        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: x_offset + start_col as f32 * metrics.width,
                y: row_y + metrics.height - thickness,
                width: (col - start_col) as f32 * metrics.width,
                height: thickness,
            },
            color: fg,
        }));
    }
}

/// Se `(row, col)` cai num dos spans de hover (ADR-0042 §3): já é o
/// subconjunto com o mesmo id do span sob o cursor, resolvido por
/// `lib.rs` -- aqui só testa contenção, uma linha por span (hyperlink
/// nunca cruza quebra de linha, diferente de seleção/ocorrência).
fn hyperlink_hover_at(spans: &[HyperlinkSpan], row: usize, col: usize) -> bool {
    spans
        .iter()
        .any(|s| s.row == row && col >= s.start_col && col <= s.end_col)
}

fn resolved_colors(
    cell: &Cell,
    selected: bool,
    occurrence: Option<bool>,
    term_pal: &ResolvedTermPalette,
) -> (Color, Color) {
    if selected || occurrence == Some(false) {
        // Seleção (ou ocorrência não ativa, mesma cor -- ADR-0041 §5)
        // domina a cor da célula -- não combina com `INVERSE` nem com a
        // cor original; é o mesmo comportamento da maioria dos terminais
        // (destaque uniforme, independente do que estava sob ele).
        return (term_pal.selection_foreground, term_pal.selection_background);
    }
    if occurrence == Some(true) {
        // Ocorrência ativa (RF-11.7): acento e o escuro que a pílula de
        // grupo já usa sobre cor cheia -- nenhuma cor nova.
        return (palette::OCCURRENCE_ACTIVE_TEXT, term_pal.cursor);
    }

    let bold = cell.flags.contains(CellFlags::BOLD);
    let mut fg = term_pal.resolve(cell.fg, true, bold);
    let mut bg = term_pal.resolve(cell.bg, false, bold);
    if cell.flags.contains(CellFlags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    (fg, bg)
}

#[cfg(test)]
mod status_bar_geometry_tests {
    use super::*;
    use crate::status_bar;
    use crate::tab_bar::TabBarStyle;

    const BAR: f32 = 52.0;
    const W: f32 = 900.0;
    const H: f32 = 600.0;

    /// O que a conta era antes da barra de status existir -- escrito à
    /// mão de propósito, para o teste abaixo comparar contra algo que
    /// não vem da mesma função que ele verifica.
    fn height_before_the_status_bar(style: &TabBarStyle) -> f32 {
        H - BAR - style.terminal_frame_margin
    }

    #[test]
    fn disabled_status_bar_gives_back_exactly_the_old_rect() {
        // Regressão zero: com `enabled = false`, a área do terminal tem
        // de ser byte a byte a de antes desta entrega. É o que torna o
        // opt-out real, em vez de "quase igual".
        let mut style = TabBarStyle::DEFAULT;
        style.status_bar_enabled = false;
        let rect = terminal_box_rect(&style, BAR, status_bar::height(&style), W, H);
        assert_eq!(rect.height, height_before_the_status_bar(&style));
        assert_eq!(rect.y, BAR);
        assert_eq!(rect.x, style.terminal_frame_margin);
    }

    #[test]
    fn enabled_status_bar_takes_its_height_and_gives_back_the_bottom_margin() {
        // A barra tira a própria altura e **devolve** o `margin` da base,
        // porque com ela o box encosta na faixa em vez de na borda da
        // janela. Líquido: a grade perde `altura - margin`, não `altura`.
        let style = TabBarStyle::DEFAULT;
        let h = status_bar::height(&style);
        assert!(h > 0.0, "ligada por padrão (RF-9.1)");
        let rect = terminal_box_rect(&style, BAR, h, W, H);
        assert_eq!(
            rect.height,
            height_before_the_status_bar(&style) - h + style.terminal_frame_margin
        );
    }

    #[test]
    fn the_box_touches_the_status_bar_with_no_gap() {
        // ADR-0048 §5, revisto depois de ver a barra em tela: o box
        // encosta na faixa, como já encostava na barra de abas em cima --
        // um vão entre duas barras de chrome lê como uma linha a mais,
        // que é o mesmo motivo pelo qual o topo nunca teve gap.
        let style = TabBarStyle::DEFAULT;
        let h = status_bar::height(&style);
        let rect = terminal_box_rect(&style, BAR, h, W, H);
        assert_eq!(
            rect.y + rect.height,
            H - h,
            "sem folga entre o box e a faixa"
        );
    }

    #[test]
    fn content_rect_inherits_the_status_bar_inset() {
        let style = TabBarStyle::DEFAULT;
        let h = status_bar::height(&style);
        let box_rect = terminal_box_rect(&style, BAR, h, W, H);
        let content = terminal_content_rect(&style, BAR, h, W, H);
        assert_eq!(
            content.height,
            box_rect.height - style.terminal_frame_padding * 2.0,
            "a grade herda o recuo sem lógica própria -- a fórmula mora numa função só"
        );
    }

    #[test]
    fn a_window_shorter_than_its_own_chrome_never_goes_negative() {
        let style = TabBarStyle::DEFAULT;
        let rect = terminal_box_rect(&style, BAR, status_bar::height(&style), W, 10.0);
        assert_eq!(rect.height, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

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
    /// resultante seja só `style.terminal_frame_padding`, sem a margem
    /// (já descontada por quem monta `box_rect` em runtime).
    fn test_box_rect() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 1000.0,
        }
    }

    fn test_term_pal() -> ResolvedTermPalette {
        ResolvedTermPalette::from_config(&porecatu_config::Config::default())
    }

    fn test_cursor() -> CursorAppearance {
        CursorAppearance {
            color: TRANSPARENT,
            width: 7.0,
            hollow: false,
        }
    }

    /// Regressão: `row_y + metrics.height` (fim de uma linha) e `y_offset +
    /// (row + 1) as f32 * metrics.height` (início da próxima) são duas
    /// contas matematicamente iguais que podem divergir por 1 ULP de ponto
    /// flutuante -- depois do arredondamento por quad em
    /// `porecatu-render/quad.rs` (que arredonda os dois cantos de **um**
    /// quad, sem saber que o vizinho deveria compartilhar a mesma borda),
    /// isso vira uma costura de 1px numa linha específica, que muda de
    /// lugar conforme `y_offset` (ex.: barra de status ligada/desligada).
    /// `row_top`/`col_left` fecham isso por construção -- este teste varre
    /// várias combinações de tamanho de fonte/offset pra pegar de volta se
    /// alguém reintroduzir a conta duplicada em vez de reusar a função.
    #[test]
    fn adjacent_row_and_column_backgrounds_share_an_exact_edge() {
        let mut m = porecatu_render::TextMeasurer::new();
        let cols = 3;
        let rows = 6;
        let cells = vec![
            Cell {
                bg: porecatu_term::TermColor::Rgb {
                    r: 200,
                    g: 60,
                    b: 60,
                },
                ..Cell::default()
            };
            rows * cols
        ];
        let snap = GridSnapshot {
            cols,
            rows,
            cells,
            ..GridSnapshot::default()
        };

        for size in [13.0_f32, 14.0, 14.3, 15.7, 16.0, 18.25] {
            for y in [0.0_f32, 1.0, 33.0, 52.3, 100.7] {
                let (width, height) = m.measure_mono_cell(size, size * 1.2);
                let cell_metrics = CellMetrics { width, height };
                let box_rect = Rect {
                    x: 0.0,
                    y,
                    width: 1000.0,
                    height: 1000.0,
                };
                let out = build_primitives(
                    &snap,
                    cell_metrics,
                    size,
                    box_rect,
                    &TabBarStyle::DEFAULT,
                    &test_term_pal(),
                    test_cursor(),
                    &mut m,
                    &[],
                );

                let mut by_col: HashMap<i64, Vec<(f32, f32)>> = HashMap::new();
                for p in &out {
                    if let Primitive::Quad(q) = p {
                        by_col
                            .entry((q.rect.x * 1024.0).round() as i64)
                            .or_default()
                            .push((q.rect.y, q.rect.y + q.rect.height));
                    }
                }
                for edges in by_col.values_mut() {
                    edges.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                    for w in edges.windows(2) {
                        assert_eq!(
                            w[0].1, w[1].0,
                            "costura entre linhas em size={size} y_offset={y}"
                        );
                    }
                }
            }
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
            &TabBarStyle::DEFAULT,
            &test_term_pal(),
            test_cursor(),
            &mut m,
            &[],
        );
        let runs = runs(&out);
        assert_eq!(runs.len(), 1, "esperava um run só, veio {runs:?}");
        assert_eq!(runs[0].1, "cargo build");
        assert_eq!(runs[0].2, SIZE);
    }

    /// A regressão que esta correção desfaz. `metrics.width` chega à
    /// pintura **arredondado ao pixel físico**
    /// (`snap_cell_metrics_to_pixel_grid`), e o teste de grade comparava
    /// contra ele: em escala 1.0 a diferença é meio pixel, dez vezes a
    /// tolerância, então toda célula reprovava. Cada caractere virava um
    /// `TextRun` próprio, com um shaping sem cache cada -- ~4000 por frame
    /// numa grade 80x24, contra ~24. O helper `cell` acima não arredonda,
    /// e foi por isso que os testes existentes não pegaram nada.
    ///
    /// Toda escala aqui, não só a do desenvolvedor: é justamente onde o
    /// arredondamento cai que o bug aparecia ou não.
    #[test]
    fn snapped_cell_metrics_still_yield_one_run_per_line() {
        let mut m = porecatu_render::TextMeasurer::new();
        let (width, height) = m.measure_mono_cell(SIZE, SIZE * 1.75);
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let cell = crate::snap_cell_metrics_to_pixel_grid(width, height, scale);
            let out = build_primitives(
                &snapshot("cargo build --workspace"),
                cell,
                SIZE,
                test_box_rect(),
                &TabBarStyle::DEFAULT,
                &test_term_pal(),
                test_cursor(),
                &mut m,
                &[],
            );
            let runs = runs(&out);
            assert_eq!(
                runs.len(),
                1,
                "escala {scale}: celula {} contra avanco natural {width},                  a linha quebrou em {} runs",
                cell.width,
                runs.len()
            );
        }
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
            &TabBarStyle::DEFAULT,
            &test_term_pal(),
            test_cursor(),
            &mut m,
            &[],
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
            &TabBarStyle::DEFAULT,
            &test_term_pal(),
            test_cursor(),
            &mut m,
            &[],
        );
        let runs = runs(&out);
        assert_eq!(runs.len(), 3, "esperava tres runs, veio {runs:?}");

        let padding = TabBarStyle::DEFAULT.terminal_frame_padding;
        assert_eq!(runs[0].1, "ab");
        assert_eq!(runs[0].0, padding);

        assert_eq!(runs[1].1, "\u{1F600}");
        assert_eq!(
            runs[1].0,
            padding + 2.0 * cell.width,
            "glyph de fallback fora da celula dele"
        );

        assert_eq!(runs[2].1, "cd");
        assert_eq!(
            runs[2].0,
            padding + 3.0 * cell.width,
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
        let out = build_primitives(
            &snapshot("\u{1F600}"),
            cell,
            SIZE,
            test_box_rect(),
            &TabBarStyle::DEFAULT,
            &test_term_pal(),
            test_cursor(),
            &mut m,
            &[],
        );
        let runs = runs(&out);
        assert_eq!(runs.len(), 1);
        let size = runs[0].2;
        let width = m.measure_width(&runs[0].1, FontFace::Mono { bold: false }, size);
        // Folga em pixels, não `ADVANCE_TOLERANCE` -- essa constante é em
        // fração da em, e aqui se compara largura desenhada com largura de
        // célula. Meio pixel cobre o arredondamento do shaping.
        assert!(
            width <= cell.width + 0.5,
            "glyph de fallback com {width} numa celula de {}",
            cell.width
        );
    }

    fn snapshot_with_cursor(shape: CursorShape) -> GridSnapshot {
        let mut snap = snapshot("x");
        snap.cursor = porecatu_term::Cursor {
            position: Some((0, 0)),
            shape,
            visible: true,
        };
        snap
    }

    fn cursor_primitives(snapshot: &GridSnapshot, cursor: CursorAppearance) -> Vec<Primitive> {
        let mut m = porecatu_render::TextMeasurer::new();
        let cell = cell(&mut m);
        build_primitives(
            snapshot,
            cell,
            SIZE,
            test_box_rect(),
            &TabBarStyle::DEFAULT,
            &test_term_pal(),
            cursor,
            &mut m,
            &[],
        )
    }

    /// RF-5.22: formato bloco, com foco, é um `Quad` preenchido cobrindo a
    /// célula inteira -- o desenho de sempre, sem regressão.
    #[test]
    fn block_cursor_focused_is_a_filled_quad() {
        let out = cursor_primitives(
            &snapshot_with_cursor(CursorShape::Block),
            CursorAppearance {
                color: TRANSPARENT,
                width: 7.0,
                hollow: false,
            },
        );
        assert!(matches!(out.last(), Some(Primitive::Quad(_))));
    }

    /// RF-5.24: sem foco e `unfocused_hollow` ligado, o bloco sai vazado --
    /// contorno, não preenchimento.
    #[test]
    fn block_cursor_hollow_is_an_outlined_rounded_quad() {
        let out = cursor_primitives(
            &snapshot_with_cursor(CursorShape::Block),
            CursorAppearance {
                color: TRANSPARENT,
                width: 7.0,
                hollow: true,
            },
        );
        match out.last() {
            Some(Primitive::RoundedQuad(q)) => {
                assert_eq!(q.border_width, 7.0);
                assert_eq!(q.color, TRANSPARENT);
            }
            other => panic!("esperava RoundedQuad vazado, veio {other:?}"),
        }
    }

    /// RF-5.22: DECSCUSR pedindo `HollowBlock` sai vazado mesmo com a
    /// janela em foco -- `hollow: false` não sobrepõe o formato do motor.
    #[test]
    fn decscusr_hollow_block_is_outlined_even_when_focused() {
        let out = cursor_primitives(
            &snapshot_with_cursor(CursorShape::HollowBlock),
            CursorAppearance {
                color: TRANSPARENT,
                width: 7.0,
                hollow: false,
            },
        );
        assert!(matches!(out.last(), Some(Primitive::RoundedQuad(_))));
    }

    /// RF-5.22: barra vertical usa `cursor.width` como largura do traço, não
    /// a largura da célula inteira.
    #[test]
    fn beam_cursor_uses_configured_stroke_width() {
        let out = cursor_primitives(
            &snapshot_with_cursor(CursorShape::Beam),
            CursorAppearance {
                color: TRANSPARENT,
                width: 3.0,
                hollow: false,
            },
        );
        match out.last() {
            Some(Primitive::Quad(q)) => assert_eq!(q.rect.width, 3.0),
            other => panic!("esperava Quad da barra, veio {other:?}"),
        }
    }

    /// RF-5.22: sublinhado usa `cursor.width` como altura do traço, colado
    /// na base da célula.
    #[test]
    fn underline_cursor_sits_at_the_bottom_of_the_cell() {
        let mut m = porecatu_render::TextMeasurer::new();
        let cell = cell(&mut m);
        let out = build_primitives(
            &snapshot_with_cursor(CursorShape::Underline),
            cell,
            SIZE,
            test_box_rect(),
            &TabBarStyle::DEFAULT,
            &test_term_pal(),
            CursorAppearance {
                color: TRANSPARENT,
                width: 2.0,
                hollow: false,
            },
            &mut m,
            &[],
        );
        let cursor_height = SIZE * CURSOR_HEIGHT_RATIO;
        match out.last() {
            Some(Primitive::Quad(q)) => {
                assert_eq!(q.rect.height, 2.0);
                assert_eq!(
                    q.rect.y,
                    TabBarStyle::DEFAULT.terminal_frame_padding + cursor_height - 2.0
                );
            }
            other => panic!("esperava Quad do sublinhado, veio {other:?}"),
        }
    }
}
