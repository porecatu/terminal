// SPDX-License-Identifier: GPL-3.0-or-later

//! Blocos (U+2580-259F) e o núcleo reto de box-drawing (U+2500-254B)
//! desenhados como `Quad`s nossos, não pedidos à fonte -- mesmo precedente
//! dos ícones Lucide (`porecatu_render::icon`) e do braille/box-drawing que
//! já quebram o `TextRun` compartilhado em `paint::fits_the_grid`, aqui
//! levado ao próximo passo: nem o glyph que a fonte desenha.
//!
//! Motivo: medição direta (`text_measurer.rs`,
//! `full_block_glyph_ink_coverage_of_its_advance_box`) mostra que a tinta
//! rasterizada de U+2588 na Iosevka Fixed é mais alta que a caixa de linha
//! que `text.rs` usa pra posicionar cada linha (`Metrics::new(size_px,
//! size_px * 1.2)`) -- blocos empilhados em linhas/`TextRun`s separados não
//! se alinham pixel a pixel com essa caixa, deixando costura entre linhas.
//! Um retângulo nosso, alinhado à célula já arredondada ao pixel físico
//! (`porecatu-render/src/quad.rs`), não depende de nenhuma dessas duas
//! caixas -- só da célula.
//!
//! **Escopo desta primeira leva**, deliberado: cantos/T/cruz de peso **puro**
//! (só leve, só pesado, ou só dupla) e as duas linhas retas. Variantes de
//! peso misto (ex. U+250D "DOWN LIGHT AND RIGHT HEAVY", ~40 caracteres),
//! tracejadas (U+2504-250B) e arcos/diagonais (U+256D-2573) ficam de fora --
//! caem no caminho de texto normal, sem regressão do que já havia. É corte
//! de escopo investigado, não lacuna escondida: cobre o caso que motivou a
//! correção (blocos de pixel art) e o núcleo de box-drawing mais comum em
//! TUIs (btop, lazygit, ranger), com uma tabela pequena o bastante para
//! conferir à mão em vez de arriscar erro de transcrição numa tabela de
//! ~75 entradas.

use porecatu_render::{Color, Primitive, Quad, Rect};

/// Peso de um segmento de box-drawing -- decide a espessura do traço e, na
/// dupla, que vira dois traços em vez de um.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Weight {
    Light,
    Heavy,
    Double,
}

/// Quais dos quatro lados um caractere de box-drawing puxa a partir do
/// centro da célula, e com que peso -- `None` no lado que ele não toca.
#[derive(Debug, Clone, Copy, Default)]
struct Segments {
    up: Option<Weight>,
    down: Option<Weight>,
    left: Option<Weight>,
    right: Option<Weight>,
}

/// Traço pesado é o dobro do leve -- proporção convencional de
/// box-drawing, não um token de aparência (mesma categoria de constante que
/// `CURSOR_HEIGHT_RATIO`/o `0.1` de `paint_row_underlines`: fidelidade de
/// desenho da grade, não decisão de chrome).
const HEAVY_WEIGHT_MULTIPLIER: f32 = 2.0;

/// Se `ch` é um bloco ou box-drawing coberto por este módulo -- usado por
/// `paint::paint_row_text` pra desviar a célula do caminho de texto *antes*
/// de testar `fits_the_grid`.
pub(crate) fn is_covered(ch: char) -> bool {
    block_rects(ch).is_some() || box_drawing_segments(ch).is_some()
}

/// Gera os `Primitive::Quad` de `ch` dentro de `cell` (já em pixels lógicos,
/// canto superior esquerdo + tamanho -- a mesma célula que
/// `paint_row_backgrounds` pintou). `fg` já resolvido
/// (`paint::resolved_colors`) -- `bg` não entra: o fundo da célula já foi
/// pintado por `paint_row_backgrounds` antes disto rodar, então meio-bloco e
/// sombreado só precisam somar `fg` por cima. `thickness` é a espessura de
/// traço leve, em pixels lógicos -- mesma conta de `paint_row_underlines`
/// (`font_size_px * 0.1`, mínimo 1px).
///
/// Não faz nada (não empurra primitiva) se `ch` não é coberto -- o chamador
/// já checou com [`is_covered`] antes de decidir não desenhar texto, então
/// isto nunca deveria disparar em produção; existe pra não duplicar a
/// checagem com `debug_assert` silencioso.
pub(crate) fn push(ch: char, cell: Rect, fg: Color, thickness: f32, out: &mut Vec<Primitive>) {
    if let Some(rects) = block_rects(ch) {
        for &(x0, y0, x1, y1) in rects {
            let color = if let Some(alpha) = shade_alpha(ch) {
                Color {
                    a: fg.a * alpha,
                    ..fg
                }
            } else {
                fg
            };
            push_frac(out, cell, color, x0, y0, x1, y1);
        }
        return;
    }
    if let Some(segments) = box_drawing_segments(ch) {
        push_segments(cell, segments, thickness, fg, out);
    }
}

fn push_frac(
    out: &mut Vec<Primitive>,
    cell: Rect,
    color: Color,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
) {
    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: cell.x + cell.width * x0,
            y: cell.y + cell.height * y0,
            width: cell.width * (x1 - x0),
            height: cell.height * (y1 - y0),
        },
        color,
    }));
}

/// `Some(alpha)` para os três sombreados (U+2591-2593): um quad da célula
/// inteira com `fg` nesse alfa sobre o `bg` já pintado -- mais fiel a "massa
/// sólida" do que o dither por glyph que a fonte faria, e sem Moiré em área
/// grande.
fn shade_alpha(ch: char) -> Option<f64> {
    match ch {
        '\u{2591}' => Some(0.25),
        '\u{2592}' => Some(0.50),
        '\u{2593}' => Some(0.75),
        _ => None,
    }
}

/// Retângulos fracionários `(x0, y0, x1, y1)` de `cell`, um por bloco
/// preenchido -- até três nos quatro caracteres de quadrante combinado. A
/// ordem numérica do bloco Unicode "Block Elements" é sistemática (metades,
/// oitavos decrescentes, oitavos, sombras, oitavo de borda, quadrantes); a
/// tabela segue essa ordem.
fn block_rects(ch: char) -> Option<&'static [(f32, f32, f32, f32)]> {
    const E1: f32 = 1.0 / 8.0;
    const E3: f32 = 3.0 / 8.0;
    const E5: f32 = 5.0 / 8.0;
    const E7: f32 = 7.0 / 8.0;
    const H: f32 = 0.5;
    Some(match ch {
        '\u{2580}' => &[(0.0, 0.0, 1.0, H)],    // UPPER HALF BLOCK
        '\u{2581}' => &[(0.0, E7, 1.0, 1.0)],   // LOWER ONE EIGHTH BLOCK
        '\u{2582}' => &[(0.0, 0.75, 1.0, 1.0)], // LOWER ONE QUARTER BLOCK
        '\u{2583}' => &[(0.0, E5, 1.0, 1.0)],   // LOWER THREE EIGHTHS BLOCK
        '\u{2584}' => &[(0.0, H, 1.0, 1.0)],    // LOWER HALF BLOCK
        '\u{2585}' => &[(0.0, E3, 1.0, 1.0)],   // LOWER FIVE EIGHTHS BLOCK
        '\u{2586}' => &[(0.0, 0.25, 1.0, 1.0)], // LOWER THREE QUARTERS BLOCK
        '\u{2587}' => &[(0.0, E1, 1.0, 1.0)],   // LOWER SEVEN EIGHTHS BLOCK
        '\u{2588}' => &[(0.0, 0.0, 1.0, 1.0)],  // FULL BLOCK
        '\u{2589}' => &[(0.0, 0.0, E7, 1.0)],   // LEFT SEVEN EIGHTHS BLOCK
        '\u{258A}' => &[(0.0, 0.0, 0.75, 1.0)], // LEFT THREE QUARTERS BLOCK
        '\u{258B}' => &[(0.0, 0.0, E5, 1.0)],   // LEFT FIVE EIGHTHS BLOCK
        '\u{258C}' => &[(0.0, 0.0, H, 1.0)],    // LEFT HALF BLOCK
        '\u{258D}' => &[(0.0, 0.0, E3, 1.0)],   // LEFT THREE EIGHTHS BLOCK
        '\u{258E}' => &[(0.0, 0.0, 0.25, 1.0)], // LEFT ONE QUARTER BLOCK
        '\u{258F}' => &[(0.0, 0.0, E1, 1.0)],   // LEFT ONE EIGHTH BLOCK
        '\u{2590}' => &[(H, 0.0, 1.0, 1.0)],    // RIGHT HALF BLOCK
        '\u{2591}' | '\u{2592}' | '\u{2593}' => &[(0.0, 0.0, 1.0, 1.0)], // shades, alfa em `shade_alpha`
        '\u{2594}' => &[(0.0, 0.0, 1.0, E1)],                            // UPPER ONE EIGHTH BLOCK
        '\u{2595}' => &[(E7, 0.0, 1.0, 1.0)],                            // RIGHT ONE EIGHTH BLOCK
        '\u{2596}' => &[(0.0, H, H, 1.0)],                               // QUADRANT LOWER LEFT
        '\u{2597}' => &[(H, H, 1.0, 1.0)],                               // QUADRANT LOWER RIGHT
        '\u{2598}' => &[(0.0, 0.0, H, H)],                               // QUADRANT UPPER LEFT
        '\u{2599}' => &[(0.0, 0.0, H, 1.0), (H, H, 1.0, 1.0)],           // UL+LL+LR
        '\u{259A}' => &[(0.0, 0.0, H, H), (H, H, 1.0, 1.0)],             // UL+LR
        '\u{259B}' => &[(0.0, 0.0, 1.0, H), (0.0, H, H, 1.0)],           // UL+UR+LL
        '\u{259C}' => &[(0.0, 0.0, 1.0, H), (H, H, 1.0, 1.0)],           // UL+UR+LR
        '\u{259D}' => &[(H, 0.0, 1.0, H)],                               // QUADRANT UPPER RIGHT
        '\u{259E}' => &[(H, 0.0, 1.0, H), (0.0, H, H, 1.0)],             // UR+LL
        '\u{259F}' => &[(0.0, H, 1.0, 1.0), (H, 0.0, 1.0, H)],           // UR+LL+LR
        _ => return None,
    })
}

/// Segmentos de `ch`, só o núcleo de peso puro (ver o escopo no comentário
/// do módulo). `None` pra tudo que não é reto, é peso misto, é tracejado ou
/// é arco/diagonal -- cai em texto normal, sem regressão.
fn box_drawing_segments(ch: char) -> Option<Segments> {
    use Weight::{Double, Heavy, Light};
    let s = |up, down, left, right| {
        Some(Segments {
            up,
            down,
            left,
            right,
        })
    };
    match ch {
        // Linhas retas.
        '\u{2500}' => s(None, None, Some(Light), Some(Light)), // ─
        '\u{2501}' => s(None, None, Some(Heavy), Some(Heavy)), // ━
        '\u{2502}' => s(Some(Light), Some(Light), None, None), // │
        '\u{2503}' => s(Some(Heavy), Some(Heavy), None, None), // ┃
        // Cantos leves.
        '\u{250C}' => s(None, Some(Light), None, Some(Light)), // ┌
        '\u{2510}' => s(None, Some(Light), Some(Light), None), // ┐
        '\u{2514}' => s(Some(Light), None, None, Some(Light)), // └
        '\u{2518}' => s(Some(Light), None, Some(Light), None), // ┘
        // Cantos pesados.
        '\u{250F}' => s(None, Some(Heavy), None, Some(Heavy)), // ┏
        '\u{2513}' => s(None, Some(Heavy), Some(Heavy), None), // ┓
        '\u{2517}' => s(Some(Heavy), None, None, Some(Heavy)), // ┗
        '\u{251B}' => s(Some(Heavy), None, Some(Heavy), None), // ┛
        // Tês leves + cruz.
        '\u{251C}' => s(Some(Light), Some(Light), None, Some(Light)), // ├
        '\u{2524}' => s(Some(Light), Some(Light), Some(Light), None), // ┤
        '\u{252C}' => s(None, Some(Light), Some(Light), Some(Light)), // ┬
        '\u{2534}' => s(Some(Light), None, Some(Light), Some(Light)), // ┴
        '\u{253C}' => s(Some(Light), Some(Light), Some(Light), Some(Light)), // ┼
        // Tês pesadas + cruz.
        '\u{2523}' => s(Some(Heavy), Some(Heavy), None, Some(Heavy)), // ┣
        '\u{252B}' => s(Some(Heavy), Some(Heavy), Some(Heavy), None), // ┫
        '\u{2533}' => s(None, Some(Heavy), Some(Heavy), Some(Heavy)), // ┳
        '\u{253B}' => s(Some(Heavy), None, Some(Heavy), Some(Heavy)), // ┻
        '\u{254B}' => s(Some(Heavy), Some(Heavy), Some(Heavy), Some(Heavy)), // ╋
        // Duplas.
        '\u{2550}' => s(None, None, Some(Double), Some(Double)), // ═
        '\u{2551}' => s(Some(Double), Some(Double), None, None), // ║
        '\u{2554}' => s(None, Some(Double), None, Some(Double)), // ╔
        '\u{2557}' => s(None, Some(Double), Some(Double), None), // ╗
        '\u{255A}' => s(Some(Double), None, None, Some(Double)), // ╚
        '\u{255D}' => s(Some(Double), None, Some(Double), None), // ╝
        '\u{2560}' => s(Some(Double), Some(Double), None, Some(Double)), // ╠
        '\u{2563}' => s(Some(Double), Some(Double), Some(Double), None), // ╣
        '\u{2566}' => s(None, Some(Double), Some(Double), Some(Double)), // ╦
        '\u{2569}' => s(Some(Double), None, Some(Double), Some(Double)), // ╩
        '\u{256C}' => s(Some(Double), Some(Double), Some(Double), Some(Double)), // ╬
        _ => None,
    }
}

/// Desenha os quatro segmentos de `segments`, cada um do centro de `cell`
/// até a borda do lado que ele ocupa. Dupla vira duas barras paralelas
/// (banda de `3 * thickness`, vão de `thickness` no meio) em vez de uma --
/// simplificação deliberada nos cantos/tês duplos: a junção não é
/// meticulosamente encaixada como um terminal dedicado a box-drawing faria,
/// mas não regride o que a fonte já desenhava, e as linhas retas duplas
/// (as mais comuns) saem corretas.
fn push_segments(
    cell: Rect,
    segments: Segments,
    thickness: f32,
    color: Color,
    out: &mut Vec<Primitive>,
) {
    let cx = cell.x + cell.width / 2.0;
    let cy = cell.y + cell.height / 2.0;

    if let Some(weight) = segments.left {
        push_horizontal_run(cell.x, cx, cy, weight, thickness, color, out);
    }
    if let Some(weight) = segments.right {
        push_horizontal_run(cx, cell.x + cell.width, cy, weight, thickness, color, out);
    }
    if let Some(weight) = segments.up {
        push_vertical_run(cell.y, cy, cx, weight, thickness, color, out);
    }
    if let Some(weight) = segments.down {
        push_vertical_run(cy, cell.y + cell.height, cx, weight, thickness, color, out);
    }
}

fn push_horizontal_run(
    x0: f32,
    x1: f32,
    center_y: f32,
    weight: Weight,
    light_thickness: f32,
    color: Color,
    out: &mut Vec<Primitive>,
) {
    let t = stroke_thickness(weight, light_thickness);
    for y in bar_centers(center_y, weight, light_thickness) {
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: x0,
                y: y - t / 2.0,
                width: x1 - x0,
                height: t,
            },
            color,
        }));
    }
}

fn push_vertical_run(
    y0: f32,
    y1: f32,
    center_x: f32,
    weight: Weight,
    light_thickness: f32,
    color: Color,
    out: &mut Vec<Primitive>,
) {
    let t = stroke_thickness(weight, light_thickness);
    for x in bar_centers(center_x, weight, light_thickness) {
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: x - t / 2.0,
                y: y0,
                width: t,
                height: y1 - y0,
            },
            color,
        }));
    }
}

/// Centro(s) do(s) traço(s), perpendicular ao eixo do segmento: um só para
/// leve/pesada (a espessura já reflete o peso), dois para dupla (banda de
/// `3 * thickness` em volta do centro, vão de `thickness` no meio).
fn bar_centers(center: f32, weight: Weight, thickness: f32) -> Vec<f32> {
    match weight {
        Weight::Light | Weight::Heavy => vec![center],
        Weight::Double => vec![center - thickness, center + thickness],
    }
}

/// Espessura efetiva de um traço, dado o peso -- pesada é o dobro da leve
/// (`HEAVY_WEIGHT_MULTIPLIER`); dupla usa a espessura leve em cada uma das
/// duas barras (`bar_centers` já as separa).
fn stroke_thickness(weight: Weight, light_thickness: f32) -> f32 {
    match weight {
        Weight::Light | Weight::Double => light_thickness,
        Weight::Heavy => light_thickness * HEAVY_WEIGHT_MULTIPLIER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell() -> Rect {
        Rect {
            x: 100.0,
            y: 200.0,
            width: 10.0,
            height: 20.0,
        }
    }

    const FG: Color = Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    #[test]
    fn full_block_covers_the_whole_cell() {
        let mut out = Vec::new();
        push('\u{2588}', cell(), FG, 1.0, &mut out);
        assert_eq!(out.len(), 1);
        let Primitive::Quad(q) = &out[0] else {
            panic!("esperava Quad")
        };
        assert_eq!(q.rect, cell());
        assert_eq!(q.color, FG);
    }

    #[test]
    fn upper_half_block_covers_only_the_top_half() {
        let mut out = Vec::new();
        push('\u{2580}', cell(), FG, 1.0, &mut out);
        let Primitive::Quad(q) = &out[0] else {
            panic!("esperava Quad")
        };
        assert_eq!(q.rect.y, cell().y);
        assert_eq!(q.rect.height, cell().height / 2.0);
        assert_eq!(q.rect.width, cell().width);
    }

    #[test]
    fn quadrant_combo_emits_two_rects_covering_three_quarters() {
        let mut out = Vec::new();
        push('\u{2599}', cell(), FG, 1.0, &mut out);
        assert_eq!(out.len(), 2);
        let area: f32 = out
            .iter()
            .map(|p| {
                let Primitive::Quad(q) = p else {
                    panic!("esperava Quad")
                };
                q.rect.width * q.rect.height
            })
            .sum();
        assert!((area - 0.75 * cell().width * cell().height).abs() < 0.001);
    }

    #[test]
    fn shades_scale_foreground_alpha_and_cover_the_whole_cell() {
        let mut out = Vec::new();
        push('\u{2592}', cell(), FG, 1.0, &mut out);
        assert_eq!(out.len(), 1);
        let Primitive::Quad(q) = &out[0] else {
            panic!("esperava Quad")
        };
        assert_eq!(q.rect, cell());
        assert!((q.color.a - 0.5).abs() < 0.001);
    }

    #[test]
    fn light_horizontal_line_spans_the_full_width_centered_vertically() {
        let mut out = Vec::new();
        push('\u{2500}', cell(), FG, 2.0, &mut out);
        assert_eq!(out.len(), 2, "esquerda + direita, um quad cada");
        let total_width: f32 = out
            .iter()
            .map(|p| {
                let Primitive::Quad(q) = p else {
                    panic!("esperava Quad")
                };
                q.rect.width
            })
            .sum();
        assert!((total_width - cell().width).abs() < 0.001);
        for p in &out {
            let Primitive::Quad(q) = p else {
                panic!("esperava Quad")
            };
            assert_eq!(q.rect.height, 2.0);
            assert!((q.rect.y - (cell().y + cell().height / 2.0 - 1.0)).abs() < 0.001);
        }
    }

    #[test]
    fn heavy_line_is_twice_as_thick_as_light() {
        assert_eq!(
            stroke_thickness(Weight::Heavy, 2.0),
            stroke_thickness(Weight::Light, 2.0) * HEAVY_WEIGHT_MULTIPLIER
        );
    }

    #[test]
    fn heavy_horizontal_line_renders_at_double_thickness() {
        let mut light = Vec::new();
        push('\u{2500}', cell(), FG, 2.0, &mut light);
        let mut heavy = Vec::new();
        push('\u{2501}', cell(), FG, 2.0, &mut heavy);

        let height_of = |out: &[Primitive]| {
            let Primitive::Quad(q) = &out[0] else {
                panic!("esperava Quad")
            };
            q.rect.height
        };
        assert_eq!(
            height_of(&heavy),
            height_of(&light) * HEAVY_WEIGHT_MULTIPLIER
        );
    }

    #[test]
    fn double_vertical_line_emits_two_parallel_bars() {
        let mut out = Vec::new();
        push('\u{2551}', cell(), FG, 1.0, &mut out);
        // Duas barras pra cima do centro + duas pra baixo = quatro no
        // total (up e down são segmentos independentes, cada um dupla).
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn corner_light_touches_only_down_and_right() {
        let mut out = Vec::new();
        push('\u{250C}', cell(), FG, 1.0, &mut out);
        assert_eq!(out.len(), 2, "só down + right, sem up nem left");
    }

    #[test]
    fn dashed_and_mixed_weight_characters_are_not_covered() {
        // Escopo desta leva: cortados deliberadamente, caem em texto normal.
        assert!(!is_covered('\u{2504}')); // dashed
        assert!(!is_covered('\u{250D}')); // mixed weight
        assert!(!is_covered('\u{256D}')); // arco/canto arredondado
    }

    #[test]
    fn is_covered_agrees_with_push_producing_geometry() {
        for ch in ['\u{2588}', '\u{2591}', '\u{2500}', '\u{254B}', '\u{256C}'] {
            assert!(is_covered(ch));
            let mut out = Vec::new();
            push(ch, cell(), FG, 1.0, &mut out);
            assert!(!out.is_empty(), "`{ch}` deveria gerar geometria");
        }
    }
}
