// SPDX-License-Identifier: GPL-3.0-or-later

//! Camadas e recorte (ADR-0018). Cinco camadas fixas e enumeradas -- não
//! profundidade arbitrária -- e uma pilha de clip que intersecta ao
//! aninhar. A resolução (`resolve_layer`) é lógica pura: não toca `wgpu`,
//! não sabe de GPU, e por isso é testável sem janela. A conversão para
//! pixels físicos é responsabilidade exclusiva de `WindowSurface`, no
//! único ponto que o ADR exige.

use crate::primitives::{Primitive, Quad, Rect, RoundedQuad, TextRun};

/// As cinco camadas do frame, na ordem em que desenham (ADR-0018): cada
/// uma inteira cobre a anterior inteira. Camada vazia não custa nada --
/// nenhum passe é emitido para ela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// Fundo de célula, glyphs, cursor, seleção.
    Grid,
    /// Barra de abas, trilha, pílulas, botões.
    Chrome,
    /// Pilha de avisos do app.
    Warning,
    /// Menu de contexto e tooltip.
    Popover,
    /// Overlay e diálogo de confirmação.
    Modal,
}

impl Layer {
    /// Ordem de desenho: `Grid` primeiro, `Modal` por cima de tudo.
    pub const ORDER: [Layer; 5] = [
        Layer::Grid,
        Layer::Chrome,
        Layer::Warning,
        Layer::Popover,
        Layer::Modal,
    ];

    pub(crate) const fn index(self) -> usize {
        match self {
            Layer::Grid => 0,
            Layer::Chrome => 1,
            Layer::Warning => 2,
            Layer::Popover => 3,
            Layer::Modal => 4,
        }
    }
}

/// O frame inteiro: uma lista de primitivas por camada. `porecatu-ui`
/// monta um `Frame` por redraw; `porecatu-render` desenha as camadas na
/// ordem de [`Layer::ORDER`], sem conhecer o que cada uma representa.
#[derive(Debug, Clone)]
pub struct Frame {
    layers: [Vec<Primitive>; 5],
}

impl Frame {
    pub fn new() -> Self {
        Self {
            layers: std::array::from_fn(|_| Vec::new()),
        }
    }

    /// Substitui o conteúdo inteiro de uma camada.
    pub fn set_layer(&mut self, layer: Layer, primitives: Vec<Primitive>) {
        self.layers[layer.index()] = primitives;
    }

    /// Adiciona uma primitiva ao fim de uma camada, preservando a ordem
    /// (relevante para `PushClip`/`PopClip`, que são posicionais).
    pub fn push(&mut self, layer: Layer, primitive: Primitive) {
        self.layers[layer.index()].push(primitive);
    }

    pub(crate) fn primitives(&self, layer: Layer) -> &[Primitive] {
        &self.layers[layer.index()]
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

/// Um retângulo com recorte já resolvido em texto: um `TextRun` e o clip
/// (interseção de toda a pilha) em vigor quando ele foi emitido, ou `None`
/// se nenhum `PushClip` estava aberto. `porecatu-render`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedText {
    pub run: TextRun,
    pub clip: Option<Rect>,
}

/// Quads e retângulos arredondados que compartilham o mesmo clip e são
/// contíguos no stream original -- o suficiente para desenhar num só
/// `set_scissor_rect` (ADR-0018: "quebrando o batch quando o clip muda").
/// Duas execuções não-adjacentes com o mesmo clip **não** são mescladas: a
/// ordem do stream é o que preserva "por cima", e mesclar exigiria provar
/// que nada entre elas se sobrepõe.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct GeometryBatch {
    pub clip: Option<Rect>,
    pub quads: Vec<Quad>,
    pub rounded: Vec<RoundedQuad>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ResolvedLayer {
    pub batches: Vec<GeometryBatch>,
    pub text: Vec<ResolvedText>,
}

/// Resolve uma camada: percorre as primitivas mantendo a pilha de clip
/// (interseção ao aninhar) e agrupa geometria contígua de mesmo clip em
/// batches. Texto não precisa de batch -- `TextBounds` é por `TextArea`,
/// já granular o bastante (ADR-0018).
///
/// `PopClip` sem `PushClip` correspondente é erro de programação (ADR-0018);
/// isso entra sempre por `porecatu-ui`, nunca por dado externo, então o
/// contrato é `panic`, não um `Result` que ninguém trataria.
pub(crate) fn resolve_layer(primitives: &[Primitive]) -> ResolvedLayer {
    let mut clip_stack: Vec<Rect> = Vec::new();
    let mut resolved = ResolvedLayer::default();

    for primitive in primitives {
        match primitive {
            Primitive::PushClip(rect) => {
                let next = clip_stack
                    .last()
                    .map_or(*rect, |top| intersect(*top, *rect));
                clip_stack.push(next);
            }
            Primitive::PopClip => {
                clip_stack
                    .pop()
                    .expect("PopClip sem PushClip correspondente");
            }
            Primitive::Quad(quad) => {
                batch_for(&mut resolved.batches, clip_stack.last().copied())
                    .quads
                    .push(*quad);
            }
            Primitive::RoundedQuad(quad) => {
                batch_for(&mut resolved.batches, clip_stack.last().copied())
                    .rounded
                    .push(*quad);
            }
            Primitive::Text(run) => resolved.text.push(ResolvedText {
                run: run.clone(),
                clip: clip_stack.last().copied(),
            }),
        }
    }

    resolved
}

fn batch_for(batches: &mut Vec<GeometryBatch>, clip: Option<Rect>) -> &mut GeometryBatch {
    let needs_new = batches.last().is_none_or(|b| b.clip != clip);
    if needs_new {
        batches.push(GeometryBatch {
            clip,
            ..Default::default()
        });
    }
    batches.last_mut().expect("acabou de inserir")
}

fn intersect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    Rect {
        x: x0,
        y: y0,
        width: (x1 - x0).max(0.0),
        height: (y1 - y0).max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Color;

    fn quad(x: f32) -> Primitive {
        Primitive::Quad(Quad {
            rect: Rect {
                x,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            color: Color::BLACK,
        })
    }

    #[test]
    fn no_clip_is_one_batch() {
        let resolved = resolve_layer(&[quad(0.0), quad(1.0), quad(2.0)]);
        assert_eq!(resolved.batches.len(), 1);
        assert_eq!(resolved.batches[0].clip, None);
        assert_eq!(resolved.batches[0].quads.len(), 3);
    }

    #[test]
    fn clip_change_splits_batch() {
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let primitives = [
            quad(0.0),
            Primitive::PushClip(clip),
            quad(1.0),
            Primitive::PopClip,
            quad(2.0),
        ];
        let resolved = resolve_layer(&primitives);
        // Três batches: o terceiro (clip=None de novo) NÃO se funde com o
        // primeiro, mesmo com o mesmo valor de clip -- não são adjacentes.
        assert_eq!(resolved.batches.len(), 3);
        assert_eq!(resolved.batches[0].clip, None);
        assert_eq!(resolved.batches[1].clip, Some(clip));
        assert_eq!(resolved.batches[2].clip, None);
        assert_eq!(resolved.batches[0].quads.len(), 1);
        assert_eq!(resolved.batches[1].quads.len(), 1);
        assert_eq!(resolved.batches[2].quads.len(), 1);
    }

    #[test]
    fn nested_clip_intersects() {
        let outer = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let inner = Rect {
            x: 50.0,
            y: 50.0,
            width: 100.0,
            height: 100.0,
        };
        let primitives = [
            Primitive::PushClip(outer),
            Primitive::PushClip(inner),
            quad(0.0),
            Primitive::PopClip,
            Primitive::PopClip,
        ];
        let resolved = resolve_layer(&primitives);
        assert_eq!(resolved.batches.len(), 1);
        assert_eq!(
            resolved.batches[0].clip,
            Some(Rect {
                x: 50.0,
                y: 50.0,
                width: 50.0,
                height: 50.0,
            })
        );
    }

    #[test]
    fn disjoint_nested_clip_becomes_empty_rect() {
        let a = Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let b = Rect {
            x: 20.0,
            y: 20.0,
            width: 10.0,
            height: 10.0,
        };
        let primitives = [Primitive::PushClip(a), Primitive::PushClip(b)];
        let resolved = resolve_layer(&primitives);
        // Nenhuma geometria emitida, mas a pilha não deve estourar --
        // interseção vazia é um retângulo de área zero, não um erro.
        assert!(resolved.batches.is_empty());
    }

    #[test]
    fn pop_restores_previous_clip() {
        let outer = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let inner = Rect {
            x: 10.0,
            y: 10.0,
            width: 10.0,
            height: 10.0,
        };
        let primitives = [
            Primitive::PushClip(outer),
            Primitive::PushClip(inner),
            Primitive::PopClip,
            quad(0.0),
            Primitive::PopClip,
        ];
        let resolved = resolve_layer(&primitives);
        assert_eq!(resolved.batches.len(), 1);
        assert_eq!(resolved.batches[0].clip, Some(outer));
    }

    #[test]
    #[should_panic(expected = "PopClip sem PushClip correspondente")]
    fn unbalanced_pop_panics() {
        resolve_layer(&[Primitive::PopClip]);
    }

    #[test]
    fn text_carries_its_own_clip() {
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let run = TextRun {
            origin: (0.0, 0.0),
            text: "a".to_string(),
            font: crate::primitives::FontFace::Sans {
                weight: crate::primitives::SansWeight::Regular,
            },
            size_px: 12.0,
            color: Color::BLACK,
        };
        let primitives = [
            Primitive::Text(run.clone()),
            Primitive::PushClip(clip),
            Primitive::Text(run.clone()),
            Primitive::PopClip,
        ];
        let resolved = resolve_layer(&primitives);
        assert_eq!(resolved.text.len(), 2);
        assert_eq!(resolved.text[0].clip, None);
        assert_eq!(resolved.text[1].clip, Some(clip));
    }

    #[test]
    fn frame_stores_layers_independently() {
        let mut frame = Frame::new();
        frame.push(Layer::Grid, quad(0.0));
        frame.push(Layer::Modal, quad(1.0));
        assert_eq!(frame.primitives(Layer::Grid).len(), 1);
        assert_eq!(frame.primitives(Layer::Modal).len(), 1);
        assert_eq!(frame.primitives(Layer::Chrome).len(), 0);
    }
}
