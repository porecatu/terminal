// SPDX-License-Identifier: GPL-3.0-or-later

//! Camadas e recorte (ADR-0018). Cinco camadas fixas e enumeradas -- não
//! profundidade arbitrária -- e uma pilha de clip que intersecta ao
//! aninhar. A resolução (`resolve_layer`) é lógica pura: não toca `wgpu`,
//! não sabe de GPU, e por isso é testável sem janela. A conversão para
//! pixels físicos é responsabilidade exclusiva de `WindowSurface`, no
//! único ponto que o ADR exige.

use crate::primitives::{ImageId, Primitive, Quad, Rect, RoundedQuad, TextRun};
use crate::segment::{Segment, area_segments, polyline_segments};

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

/// Um quad reto ou arredondado, na ordem em que chegou no stream --
/// [`GeometryBatch`] precisa dessa ordem para desenhar por cima
/// corretamente, já que os dois tipos podem se sobrepor (o quadro do
/// terminal, um `RoundedQuad`, e o cursor, um `Quad`, disputam o mesmo
/// clip).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum GeometryPrimitive {
    Quad(Quad),
    Rounded(RoundedQuad),
    /// [`Primitive::Backdrop`]: desenhado por um pipeline que substitui.
    Backdrop(RoundedQuad),
    /// [`Primitive::Image`]: outro pipeline e outro bind group, então
    /// `resolve_layer` o põe **sozinho** num batch.
    Image(ImagePrimitive),
    /// [`Primitive::BlurredBackdrop`]: o mesmo pipeline de imagem com outro
    /// fragmento e outro bind group, então também fica **sozinho** num batch.
    Blurred(BlurredPrimitive),
    /// Um segmento de [`Primitive::Polyline`] ou uma faixa de
    /// [`Primitive::Area`] (ADR-0094): `resolve_layer` expande cada primitiva
    /// nos seus segmentos, e uma corrida contígua deles vira **um** draw do
    /// pipeline de segmentos. Não quebra o batch: troca de pipeline, mas não de
    /// bind group nem de recorte.
    Segment(Segment),
}

/// Os campos de [`Primitive::BlurredBackdrop`], como valor próprio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BlurredPrimitive {
    pub mask: Rect,
    pub mask_radius: f32,
}

/// Os campos de [`Primitive::Image`], como valor próprio para viajar na
/// geometria resolvida.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ImagePrimitive {
    pub rect: Rect,
    pub uv: Rect,
    pub repeat: bool,
    pub mask: Rect,
    pub mask_radius: f32,
    pub alpha: f32,
    pub image: ImageId,
}

/// Quads e retângulos arredondados que compartilham o mesmo clip e são
/// contíguos no stream original -- o suficiente para desenhar num só
/// `set_scissor_rect` (ADR-0018: "quebrando o batch quando o clip muda").
/// Duas execuções não-adjacentes com o mesmo clip **não** são mescladas: a
/// ordem do stream é o que preserva "por cima", e mesclar exigiria provar
/// que nada entre elas se sobrepõe.
///
/// Uma [`Primitive::Image`] **quebra** o batch: ela troca de pipeline e de
/// bind group, então fica sozinha num batch próprio e os quads antes e depois
/// dela ficam em batches separados (um quad, uma imagem e um quad, no mesmo
/// clip, são três batches). A ordem do stream é o que a mantém por cima do
/// que veio antes e por baixo do que veio depois.
///
/// `geometry` guarda os dois tipos misturados **na ordem de chegada** --
/// separá-los em dois `Vec` (um por tipo) foi o bug que escondia o cursor:
/// o quadro arredondado do terminal, mesmo pushado antes do cursor,
/// desenhava por cima dele porque todo `Vec<RoundedQuad>` saía depois de
/// todo `Vec<Quad>`, não importa a ordem original.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct GeometryBatch {
    pub clip: Option<Rect>,
    pub geometry: Vec<GeometryPrimitive>,
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
                    .geometry
                    .push(GeometryPrimitive::Quad(*quad));
            }
            Primitive::RoundedQuad(quad) => {
                batch_for(&mut resolved.batches, clip_stack.last().copied())
                    .geometry
                    .push(GeometryPrimitive::Rounded(*quad));
            }
            Primitive::Backdrop(quad) => {
                batch_for(&mut resolved.batches, clip_stack.last().copied())
                    .geometry
                    .push(GeometryPrimitive::Backdrop(*quad));
            }
            Primitive::Polyline(line) => {
                debug_assert!(
                    line.color.a == 1.0,
                    "Polyline exige cor opaca: a junção de duas cápsulas escureceria"
                );
                push_segments(
                    &mut resolved.batches,
                    clip_stack.last().copied(),
                    polyline_segments(line),
                );
            }
            Primitive::Area(area) => {
                debug_assert!(
                    area.points.windows(2).all(|pair| pair[1].x >= pair[0].x),
                    "Area exige x crescente nos pontos"
                );
                push_segments(
                    &mut resolved.batches,
                    clip_stack.last().copied(),
                    area_segments(area),
                );
            }
            Primitive::Image {
                rect,
                uv,
                repeat,
                mask,
                mask_radius,
                alpha,
                image,
            } => {
                let clip = clip_stack.last().copied();
                // Sempre um batch novo, e o próximo também (`batch_for` não
                // reaproveita um batch de imagem).
                resolved.batches.push(GeometryBatch {
                    clip,
                    geometry: vec![GeometryPrimitive::Image(ImagePrimitive {
                        rect: *rect,
                        uv: *uv,
                        repeat: *repeat,
                        mask: *mask,
                        mask_radius: *mask_radius,
                        alpha: *alpha,
                        image: *image,
                    })],
                });
            }
            Primitive::BlurredBackdrop { mask, mask_radius } => {
                // Como a imagem: troca de bind group, então batch próprio, na
                // ordem do stream.
                resolved.batches.push(GeometryBatch {
                    clip: clip_stack.last().copied(),
                    geometry: vec![GeometryPrimitive::Blurred(BlurredPrimitive {
                        mask: *mask,
                        mask_radius: *mask_radius,
                    })],
                });
            }
            Primitive::Text(run) => resolved.text.push(ResolvedText {
                run: run.clone(),
                clip: clip_stack.last().copied(),
            }),
        }
    }

    resolved
}

/// Põe os segmentos de uma primitiva no batch do clip vigente. Sem segmento
/// nenhum (polilinha de menos de dois pontos), não abre batch: um batch vazio
/// quebraria a corrida do vizinho sem desenhar nada.
fn push_segments(
    batches: &mut Vec<GeometryBatch>,
    clip: Option<Rect>,
    segments: impl Iterator<Item = Segment>,
) {
    let mut segments = segments.peekable();
    if segments.peek().is_none() {
        return;
    }
    batch_for(batches, clip)
        .geometry
        .extend(segments.map(GeometryPrimitive::Segment));
}

fn batch_for(batches: &mut Vec<GeometryBatch>, clip: Option<Rect>) -> &mut GeometryBatch {
    let needs_new = batches.last().is_none_or(|b| {
        b.clip != clip
            || matches!(
                b.geometry.first(),
                Some(GeometryPrimitive::Image(_) | GeometryPrimitive::Blurred(_))
            )
    });
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

    /// Regressão: `RoundedQuad` pushado antes de um `Quad`, mesmo clip
    /// (o quadro do terminal e o cursor) -- `geometry` precisa manter a
    /// ordem de chegada, senão o quadro desenha por cima do cursor.
    #[test]
    fn quad_and_rounded_quad_keep_stream_order_in_same_batch() {
        let rounded = Primitive::RoundedQuad(RoundedQuad {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            radius: 6.0,
            color: Color::BLACK,
            border_color: Color::BLACK,
            border_width: 0.0,
        });
        let resolved = resolve_layer(&[rounded, quad(0.0)]);
        assert_eq!(resolved.batches.len(), 1);
        assert_eq!(
            resolved.batches[0].geometry,
            vec![
                GeometryPrimitive::Rounded(RoundedQuad {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    },
                    radius: 6.0,
                    color: Color::BLACK,
                    border_color: Color::BLACK,
                    border_width: 0.0,
                }),
                GeometryPrimitive::Quad(Quad {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    color: Color::BLACK,
                }),
            ]
        );
    }

    fn image(x: f32) -> Primitive {
        let rect = Rect {
            x,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        Primitive::Image {
            rect,
            uv: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            repeat: false,
            mask: rect,
            mask_radius: 0.0,
            alpha: 1.0,
            image: ImageId(7),
        }
    }

    fn is_image(batch: &GeometryBatch) -> bool {
        matches!(batch.geometry.as_slice(), [GeometryPrimitive::Image(_)])
    }

    /// Um quad, uma imagem e um quad no mesmo clip: três batches, porque a
    /// imagem troca de pipeline e de bind group -- e na ordem do stream.
    #[test]
    fn an_image_between_two_quads_makes_three_batches_in_order() {
        let resolved = resolve_layer(&[quad(0.0), image(5.0), quad(1.0)]);
        assert_eq!(resolved.batches.len(), 3);
        assert!(matches!(
            resolved.batches[0].geometry.as_slice(),
            [GeometryPrimitive::Quad(_)]
        ));
        assert!(is_image(&resolved.batches[1]));
        assert!(matches!(
            resolved.batches[2].geometry.as_slice(),
            [GeometryPrimitive::Quad(_)]
        ));
        let GeometryPrimitive::Image(drawn) = resolved.batches[1].geometry[0] else {
            unreachable!();
        };
        assert_eq!(drawn.rect.x, 5.0);
        assert_eq!(drawn.image, ImageId(7));
    }

    #[test]
    fn consecutive_images_are_one_batch_each() {
        let resolved = resolve_layer(&[image(0.0), image(1.0)]);
        assert_eq!(resolved.batches.len(), 2);
        assert!(resolved.batches.iter().all(is_image));
    }

    #[test]
    fn an_image_alone_is_a_single_batch_with_no_clip() {
        let resolved = resolve_layer(&[image(0.0)]);
        assert_eq!(resolved.batches.len(), 1);
        assert_eq!(resolved.batches[0].clip, None);
    }

    #[test]
    fn an_image_respects_the_clip_stack() {
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
        let resolved = resolve_layer(&[
            Primitive::PushClip(outer),
            Primitive::PushClip(inner),
            image(0.0),
            Primitive::PopClip,
            image(1.0),
            Primitive::PopClip,
            image(2.0),
        ]);
        let clips: Vec<_> = resolved.batches.iter().map(|b| b.clip).collect();
        assert_eq!(
            clips,
            vec![
                Some(Rect {
                    x: 50.0,
                    y: 50.0,
                    width: 50.0,
                    height: 50.0
                }),
                Some(outer),
                None
            ]
        );
    }

    fn blurred(x: f32) -> Primitive {
        Primitive::BlurredBackdrop {
            mask: Rect {
                x,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            mask_radius: 6.0,
        }
    }

    fn is_blurred(batch: &GeometryBatch) -> bool {
        matches!(batch.geometry.as_slice(), [GeometryPrimitive::Blurred(_)])
    }

    /// Um quad, o desfoque e um quad: três batches, na ordem do stream -- o
    /// desfoque troca de bind group, como a imagem.
    #[test]
    fn a_blurred_backdrop_between_two_quads_makes_three_batches_in_order() {
        let resolved = resolve_layer(&[quad(0.0), blurred(5.0), quad(1.0)]);
        assert_eq!(resolved.batches.len(), 3);
        assert!(is_blurred(&resolved.batches[1]));
        assert!(matches!(
            resolved.batches[0].geometry.as_slice(),
            [GeometryPrimitive::Quad(_)]
        ));
        assert!(matches!(
            resolved.batches[2].geometry.as_slice(),
            [GeometryPrimitive::Quad(_)]
        ));
        let GeometryPrimitive::Blurred(drawn) = resolved.batches[1].geometry[0] else {
            unreachable!();
        };
        assert_eq!(drawn.mask.x, 5.0);
        assert_eq!(drawn.mask_radius, 6.0);
    }

    #[test]
    fn blurred_backdrops_and_images_never_share_a_batch() {
        let resolved = resolve_layer(&[blurred(0.0), blurred(1.0), image(0.0), blurred(2.0)]);
        assert_eq!(resolved.batches.len(), 4);
        assert!(resolved.batches.iter().all(|b| b.geometry.len() == 1));
    }

    #[test]
    fn a_blurred_backdrop_respects_the_clip_stack() {
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 5.0,
            height: 5.0,
        };
        let resolved = resolve_layer(&[
            Primitive::PushClip(clip),
            blurred(0.0),
            Primitive::PopClip,
            blurred(1.0),
        ]);
        let clips: Vec<_> = resolved.batches.iter().map(|b| b.clip).collect();
        assert_eq!(clips, vec![Some(clip), None]);
    }

    #[test]
    fn a_quad_after_an_image_does_not_join_the_image_batch() {
        let resolved = resolve_layer(&[image(0.0), quad(0.0), quad(1.0)]);
        assert_eq!(resolved.batches.len(), 2);
        assert!(is_image(&resolved.batches[0]));
        assert_eq!(resolved.batches[1].geometry.len(), 2);
    }

    #[test]
    fn no_clip_is_one_batch() {
        let resolved = resolve_layer(&[quad(0.0), quad(1.0), quad(2.0)]);
        assert_eq!(resolved.batches.len(), 1);
        assert_eq!(resolved.batches[0].clip, None);
        assert_eq!(resolved.batches[0].geometry.len(), 3);
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
        assert_eq!(resolved.batches[0].geometry.len(), 1);
        assert_eq!(resolved.batches[1].geometry.len(), 1);
        assert_eq!(resolved.batches[2].geometry.len(), 1);
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

    fn pt(x: f32, y: f32) -> crate::primitives::Point {
        crate::primitives::Point { x, y }
    }

    fn polyline(n: usize) -> Primitive {
        Primitive::Polyline(crate::primitives::Polyline {
            points: (0..n).map(|i| pt(i as f32, 0.0)).collect(),
            width: 2.0,
            color: Color::WHITE,
        })
    }

    fn segment_count(batch: &GeometryBatch) -> usize {
        batch
            .geometry
            .iter()
            .filter(|g| matches!(g, GeometryPrimitive::Segment(_)))
            .count()
    }

    #[test]
    fn a_polyline_of_n_points_resolves_to_n_minus_1_segments() {
        let resolved = resolve_layer(&[polyline(6)]);
        assert_eq!(resolved.batches.len(), 1);
        assert_eq!(segment_count(&resolved.batches[0]), 5);
    }

    /// Menos de dois pontos não abre batch, e portanto não quebra a corrida de
    /// quads de um lado e do outro.
    #[test]
    fn a_polyline_with_fewer_than_two_points_resolves_to_nothing() {
        let resolved = resolve_layer(&[polyline(0), polyline(1)]);
        assert!(resolved.batches.is_empty());
        let resolved = resolve_layer(&[quad(0.0), polyline(1), quad(1.0)]);
        assert_eq!(resolved.batches.len(), 1);
        assert_eq!(resolved.batches[0].geometry.len(), 2);
    }

    /// Quad, polilinha e quad no mesmo clip: um batch só (nada troca de bind
    /// group ou de recorte), com a ordem do stream.
    #[test]
    fn segments_keep_stream_order_among_quads_in_one_batch() {
        let resolved = resolve_layer(&[quad(0.0), polyline(3), quad(1.0)]);
        assert_eq!(resolved.batches.len(), 1);
        let order: Vec<_> = resolved.batches[0]
            .geometry
            .iter()
            .map(|g| match g {
                GeometryPrimitive::Quad(_) => 'q',
                GeometryPrimitive::Segment(_) => 's',
                _ => '?',
            })
            .collect();
        assert_eq!(order, vec!['q', 's', 's', 'q']);
    }

    #[test]
    fn text_between_geometry_does_not_split_the_segment_run() {
        let run = TextRun {
            origin: (0.0, 0.0),
            text: "a".to_string(),
            font: crate::primitives::FontFace::Sans {
                weight: crate::primitives::SansWeight::Regular,
            },
            size_px: 12.0,
            color: Color::BLACK,
        };
        let resolved = resolve_layer(&[quad(0.0), Primitive::Text(run), polyline(3), polyline(3)]);
        assert_eq!(resolved.batches.len(), 1);
        assert_eq!(resolved.text.len(), 1);
        assert_eq!(segment_count(&resolved.batches[0]), 4);
    }

    #[test]
    fn a_polyline_respects_the_clip_stack() {
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 5.0,
            height: 5.0,
        };
        let resolved = resolve_layer(&[
            Primitive::PushClip(clip),
            polyline(3),
            Primitive::PopClip,
            polyline(3),
        ]);
        let clips: Vec<_> = resolved.batches.iter().map(|b| b.clip).collect();
        assert_eq!(clips, vec![Some(clip), None]);
    }

    /// Uma imagem entre duas polilinhas quebra o batch, como entre quads.
    #[test]
    fn an_image_splits_segment_runs() {
        let resolved = resolve_layer(&[polyline(3), image(0.0), polyline(3)]);
        assert_eq!(resolved.batches.len(), 3);
        assert_eq!(segment_count(&resolved.batches[0]), 2);
        assert!(is_image(&resolved.batches[1]));
        assert_eq!(segment_count(&resolved.batches[2]), 2);
    }

    /// A função pura de geração descarta o par fora de ordem sem `debug_assert!`
    /// (roda em debug); `resolve_layer` só o descarta sem panicar em release.
    #[test]
    fn an_area_with_x_out_of_order_loses_the_bad_pair() {
        let area = crate::primitives::Area {
            points: vec![pt(0.0, 0.0), pt(2.0, 0.0), pt(1.0, 0.0), pt(3.0, 0.0)],
            baseline_y: 5.0,
            color: Color::WHITE,
        };
        assert_eq!(crate::segment::area_segments(&area).count(), 2);
        if !cfg!(debug_assertions) {
            let resolved = resolve_layer(&[Primitive::Area(area)]);
            assert_eq!(segment_count(&resolved.batches[0]), 2);
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Area exige x crescente")]
    fn an_area_with_x_out_of_order_panics_in_debug() {
        let area = crate::primitives::Area {
            points: vec![pt(2.0, 0.0), pt(1.0, 0.0)],
            baseline_y: 5.0,
            color: Color::WHITE,
        };
        resolve_layer(&[Primitive::Area(area)]);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Polyline exige cor opaca")]
    fn a_translucent_polyline_panics_in_debug() {
        resolve_layer(&[Primitive::Polyline(crate::primitives::Polyline {
            points: vec![pt(0.0, 0.0), pt(1.0, 0.0)],
            width: 1.0,
            color: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.5,
            },
        })]);
    }
}
