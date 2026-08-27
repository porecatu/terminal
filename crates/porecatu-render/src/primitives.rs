// SPDX-License-Identifier: GPL-3.0-or-later

//! Primitivas de desenho (docs/arquitetura.md seção 5). `porecatu-render`
//! não conhece domínio: recebe quad, retângulo arredondado, run de texto e
//! clip rect, nada sobre aba ou grupo. Quem traduz snapshot + config em
//! primitivas é `porecatu-ui`.

/// Cor RGBA em `[0.0, 1.0]`.
pub type Color = wgpu::Color;

/// Retângulo em pixels lógicos, origem no canto superior esquerdo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Retângulo sólido, sem cantos arredondados nem borda -- o caso comum
/// (fundo de célula do terminal).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub rect: Rect,
    pub color: Color,
}

/// Retângulo com cantos arredondados e borda opcional -- pílula de grupo,
/// wrapper, popover (F2+). Cantos via SDF no fragment shader.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundedQuad {
    pub rect: Rect,
    pub radius: f32,
    pub color: Color,
    /// `width <= 0.0` = sem borda.
    pub border_color: Color,
    pub border_width: f32,
}

/// Qual das cinco faces embutidas usar (ADR-0016) e com que peso sintético.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFace {
    /// Conteúdo do terminal.
    Mono { bold: bool },
    /// Chrome: título de aba, rótulo, menu.
    Sans { weight: SansWeight },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SansWeight {
    Regular,
    Medium,
    SemiBold,
}

/// Um run de texto no mesmo estilo -- um por trecho de mesma fonte/cor, não
/// um por caractere (docs/arquitetura.md seção 5).
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub origin: (f32, f32),
    pub text: String,
    pub font: FontFace,
    pub size_px: f32,
    pub color: Color,
}

/// Uma primitiva de desenho, na ordem em que deve ser processada.
#[derive(Debug, Clone)]
pub enum Primitive {
    Quad(Quad),
    RoundedQuad(RoundedQuad),
    Text(TextRun),
    PushClip(Rect),
    PopClip,
}

/// Converte de pixels lógicos (contrato deste módulo) para físicos --
/// usado só na fronteira `WindowSurface`, o único ponto de conversão que o
/// ADR-0018 exige.
pub(crate) fn scale_rect(rect: Rect, scale: f32) -> Rect {
    Rect {
        x: rect.x * scale,
        y: rect.y * scale,
        width: rect.width * scale,
        height: rect.height * scale,
    }
}
