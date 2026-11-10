// SPDX-License-Identifier: GPL-3.0-or-later

//! Polilinha e área (ADR-0094): uma instância por segmento, desenhada por um
//! pipeline próprio (`segment.wgsl` sobre `sdf.wgsl`). Este módulo tem as três
//! partes que não dependem do fluxo de camadas: a geração **pura** de segmentos
//! a partir das primitivas (lógico, sem GPU), a instância de GPU (físico) e o
//! pipeline e o buffer de instâncias.
//!
//! O render não sabe o que um segmento representa: recebe pontos, espessura,
//! base e cor. Escala, tempo, lacuna e grade são de `porecatu-ui`.

use crate::primitives::{Area, Point, Polyline};
use crate::quad::Vertex;

const SHADER: &str = concat!(include_str!("sdf.wgsl"), include_str!("segment.wgsl"));
/// Capacidade inicial do buffer de instâncias de uma camada: três gráficos de
/// 60 pontos sem crescer (ADR-0094 §4).
pub(crate) const INITIAL_SEGMENT_CAPACITY: u64 = 256;

/// O que um [`Segment`] desenha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SegmentKind {
    /// Cápsula de espessura `width` (pixels lógicos) entre `p0` e `p1`.
    Line { width: f32 },
    /// Faixa vertical entre `p0.x` e `p1.x`, do segmento `p0`-`p1` até a base
    /// `baseline_y` (pixels lógicos).
    Area { baseline_y: f32 },
}

/// Um segmento em pixels lógicos, já com a cor: o que viaja em
/// `GeometryPrimitive::Segment` até `prepare_layer`, onde vira
/// [`SegmentInstance`] (físico).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Segment {
    pub p0: Point,
    pub p1: Point,
    pub kind: SegmentKind,
    pub color: wgpu::Color,
}

/// Os segmentos de uma [`Polyline`]: `n - 1` para `n` pontos, nenhum para
/// menos de dois. Função pura; o `debug_assert!` de cor opaca é de quem chama
/// (`resolve_layer`).
pub(crate) fn polyline_segments(line: &Polyline) -> impl Iterator<Item = Segment> + '_ {
    line.points.windows(2).map(|pair| Segment {
        p0: pair[0],
        p1: pair[1],
        kind: SegmentKind::Line { width: line.width },
        color: line.color,
    })
}

/// As faixas de uma [`Area`]: uma por par de pontos consecutivos **em ordem**
/// de `x`. Um par com `x` decrescente (ou `NaN`) é pulado, e um par de mesmo
/// `x` também (faixa sem largura). Função pura, sem `debug_assert!`: o aviso
/// de contrato violado é de quem chama (`resolve_layer`), e assim o descarte
/// de release é testável em debug.
pub(crate) fn area_segments(area: &Area) -> impl Iterator<Item = Segment> + '_ {
    area.points.windows(2).filter_map(|pair| {
        // `>` (e não `!(<=)`): com `NaN` a comparação dá falso e o par cai.
        (pair[1].x > pair[0].x).then_some(Segment {
            p0: pair[0],
            p1: pair[1],
            kind: SegmentKind::Area {
                baseline_y: area.baseline_y,
            },
            color: area.color,
        })
    })
}

/// A instância de GPU (ADR-0094 §2), em pixels **físicos**: 48 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SegmentInstance {
    pub p0: [f32; 2],
    pub p1: [f32; 2],
    /// Linha: meia espessura. Área: o `y` da base.
    pub half_width: f32,
    /// 0 = linha, 1 = área.
    pub kind: u32,
    pub _pad: [f32; 2],
    /// Premultiplicada.
    pub color: [f32; 4],
}

impl SegmentInstance {
    /// Lógico para físico pelo fator de escala. Os pontos **não** são
    /// arredondados ao pixel: uma linha inclinada não tem pixel a encaixar, e
    /// as faixas de uma área precisam do mesmo `x` exato nos dois lados da
    /// emenda.
    pub(crate) fn from_segment(segment: &Segment, scale: f32) -> Self {
        let (half_width, kind) = match segment.kind {
            SegmentKind::Line { width } => (width * scale * 0.5, 0),
            SegmentKind::Area { baseline_y } => (baseline_y * scale, 1),
        };
        let a = segment.color.a as f32;
        Self {
            p0: [segment.p0.x * scale, segment.p0.y * scale],
            p1: [segment.p1.x * scale, segment.p1.y * scale],
            half_width,
            kind,
            _pad: [0.0, 0.0],
            color: [
                segment.color.r as f32 * a,
                segment.color.g as f32 * a,
                segment.color.b as f32 * a,
                a,
            ],
        }
    }
}

/// O pipeline de segmentos: um por processo, ao lado dos de quad
/// (`QuadShared`). Usa o mesmo layout de grupo 0 (uniforme de resolução) e o
/// mesmo vértice estático (quad unitário), então o bind group de cada janela
/// serve.
pub(crate) struct SegmentShared {
    pub(crate) pipeline: wgpu::RenderPipeline,
}

impl SegmentShared {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("porecatu-render/segment-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("porecatu-render/segment-pipeline-layout"),
            bind_group_layouts: &[Some(bind_group_layout)],
            immediate_size: 0,
        });
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
        };
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SegmentInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                1 => Float32x2, // p0
                2 => Float32x2, // p1
                3 => Float32,   // half_width
                4 => Uint32,    // kind
                5 => Float32x2, // _pad
                6 => Float32x4, // color
            ],
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("porecatu-render/segment-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(vertex_layout), Some(instance_layout)],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // O fragmento devolve cor premultiplicada (`cor * cobertura`,
                    // segment.wgsl): o par certo é o premultiplicado, nunca
                    // `ALPHA_BLENDING` (CLAUDE.md, "Render e texto").
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline }
    }
}

/// O buffer de instâncias de segmentos de **uma camada**: cresce como o de
/// quads (recriado com o tamanho pedido quando não cabe, senão reescrito por
/// `write_buffer`).
pub(crate) struct SegmentLayerBuffer {
    pub(crate) buffer: wgpu::Buffer,
    capacity: u64,
}

impl SegmentLayerBuffer {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("porecatu-render/segment-instances"),
                size: std::mem::size_of::<SegmentInstance>() as u64 * INITIAL_SEGMENT_CAPACITY,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            capacity: INITIAL_SEGMENT_CAPACITY,
        }
    }

    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[SegmentInstance],
    ) {
        use wgpu::util::DeviceExt;

        let needed = instances.len() as u64;
        if needed > self.capacity {
            self.buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("porecatu-render/segment-instances"),
                contents: bytemuck::cast_slice(instances),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            self.capacity = needed;
        } else if needed > 0 {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(instances));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Color;

    fn pt(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    fn opaque(r: f64, g: f64, b: f64) -> Color {
        Color { r, g, b, a: 1.0 }
    }

    #[test]
    fn a_segment_instance_is_48_bytes() {
        assert_eq!(std::mem::size_of::<SegmentInstance>(), 48);
    }

    #[test]
    fn a_polyline_of_n_points_makes_n_minus_1_segments() {
        for n in 2..8 {
            let line = Polyline {
                points: (0..n).map(|i| pt(i as f32, 0.0)).collect(),
                width: 2.0,
                color: Color::WHITE,
            };
            assert_eq!(polyline_segments(&line).count(), n - 1);
        }
    }

    #[test]
    fn a_polyline_with_fewer_than_two_points_makes_nothing() {
        for n in 0..2 {
            let line = Polyline {
                points: (0..n).map(|i| pt(i as f32, 0.0)).collect(),
                width: 2.0,
                color: Color::WHITE,
            };
            assert_eq!(polyline_segments(&line).count(), 0);
        }
    }

    #[test]
    fn polyline_segments_follow_the_point_order() {
        let line = Polyline {
            points: vec![pt(0.0, 0.0), pt(1.0, 5.0), pt(2.0, 3.0)],
            width: 2.0,
            color: Color::WHITE,
        };
        let segments: Vec<_> = polyline_segments(&line).collect();
        assert_eq!(
            (segments[0].p0, segments[0].p1),
            (pt(0.0, 0.0), pt(1.0, 5.0))
        );
        assert_eq!(
            (segments[1].p0, segments[1].p1),
            (pt(1.0, 5.0), pt(2.0, 3.0))
        );
        assert_eq!(segments[0].kind, SegmentKind::Line { width: 2.0 });
    }

    /// Sem `debug_assert!`: o par fora de ordem some, e os outros ficam, em
    /// debug e em release.
    #[test]
    fn an_area_skips_a_pair_whose_x_goes_backwards() {
        let area = Area {
            points: vec![pt(0.0, 1.0), pt(2.0, 1.0), pt(1.0, 1.0), pt(3.0, 1.0)],
            baseline_y: 10.0,
            color: Color::WHITE,
        };
        let segments: Vec<_> = area_segments(&area).collect();
        // (0,2) fica, (2,1) cai, (1,3) fica.
        assert_eq!(segments.len(), 2);
        assert_eq!((segments[0].p0.x, segments[0].p1.x), (0.0, 2.0));
        assert_eq!((segments[1].p0.x, segments[1].p1.x), (1.0, 3.0));
    }

    #[test]
    fn an_area_skips_a_zero_width_pair_and_a_nan_pair() {
        let area = Area {
            points: vec![pt(0.0, 1.0), pt(0.0, 2.0), pt(f32::NAN, 2.0), pt(4.0, 2.0)],
            baseline_y: 10.0,
            color: Color::WHITE,
        };
        assert_eq!(area_segments(&area).count(), 0);
    }

    #[test]
    fn an_area_of_n_ordered_points_makes_n_minus_1_strips() {
        let area = Area {
            points: (0..61).map(|i| pt(i as f32, 3.0)).collect(),
            baseline_y: 10.0,
            color: Color::WHITE,
        };
        assert_eq!(area_segments(&area).count(), 60);
        let none = Area {
            points: vec![pt(0.0, 0.0)],
            baseline_y: 10.0,
            color: Color::WHITE,
        };
        assert_eq!(area_segments(&none).count(), 0);
    }

    #[test]
    fn scale_2_doubles_coordinates_and_half_width() {
        let segment = Segment {
            p0: pt(1.0, 2.0),
            p1: pt(3.0, 4.0),
            kind: SegmentKind::Line { width: 3.0 },
            color: opaque(1.0, 0.0, 0.0),
        };
        let one = SegmentInstance::from_segment(&segment, 1.0);
        let two = SegmentInstance::from_segment(&segment, 2.0);
        assert_eq!(one.p0, [1.0, 2.0]);
        assert_eq!(one.p1, [3.0, 4.0]);
        assert_eq!(one.half_width, 1.5);
        assert_eq!(one.kind, 0);
        assert_eq!(two.p0, [2.0, 4.0]);
        assert_eq!(two.p1, [6.0, 8.0]);
        assert_eq!(two.half_width, 3.0);
    }

    /// Os pontos não são arredondados ao pixel físico.
    #[test]
    fn instance_points_are_not_snapped_to_pixels() {
        let segment = Segment {
            p0: pt(0.3, 0.7),
            p1: pt(10.1, 5.9),
            kind: SegmentKind::Line { width: 1.0 },
            color: Color::WHITE,
        };
        let inst = SegmentInstance::from_segment(&segment, 1.5);
        assert_eq!(inst.p0, [0.3 * 1.5, 0.7 * 1.5]);
        assert_eq!(inst.p1, [10.1 * 1.5, 5.9 * 1.5]);
    }

    #[test]
    fn an_area_instance_carries_the_physical_baseline() {
        let segment = Segment {
            p0: pt(0.0, 2.0),
            p1: pt(4.0, 2.0),
            kind: SegmentKind::Area { baseline_y: 7.5 },
            color: Color::WHITE,
        };
        let inst = SegmentInstance::from_segment(&segment, 2.0);
        assert_eq!(inst.kind, 1);
        assert_eq!(inst.half_width, 15.0);
    }

    #[test]
    fn the_instance_color_is_premultiplied() {
        let segment = Segment {
            p0: pt(0.0, 0.0),
            p1: pt(1.0, 0.0),
            kind: SegmentKind::Area { baseline_y: 4.0 },
            color: Color {
                r: 1.0,
                g: 0.5,
                b: 0.0,
                a: 0.5,
            },
        };
        let inst = SegmentInstance::from_segment(&segment, 1.0);
        assert_eq!(inst.color, [0.5, 0.25, 0.0, 0.5]);
    }

    /// A cor de uma polilinha opaca fica como está; a de uma área translúcida
    /// é só alfa-multiplicada, uma vez.
    #[test]
    fn an_opaque_color_is_unchanged_by_premultiplication() {
        let segment = Segment {
            p0: pt(0.0, 0.0),
            p1: pt(1.0, 0.0),
            kind: SegmentKind::Line { width: 1.0 },
            color: opaque(0.25, 0.5, 0.75),
        };
        let inst = SegmentInstance::from_segment(&segment, 1.0);
        assert_eq!(inst.color, [0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn the_segment_wgsl_validates() {
        let module =
            wgpu::naga::front::wgsl::parse_str(SHADER).expect("o WGSL de segmento parseia");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("o WGSL de segmento valida");
    }
}

/// Testes com GPU de verdade, sem janela (o mesmo esquema de `image.rs`): um
/// alvo `Rgba8Unorm` de tamanho e escala escolhidos, os mesmos pipelines e o
/// mesmo `QuadWindowState` do app, e os pixels lidos de volta. Sem adaptador
/// nenhum (nem o de software) cada teste se **pula** e diz isso.
#[cfg(test)]
mod gpu_tests {
    use super::*;
    use crate::frame::{Layer, resolve_layer};
    use crate::image::{GpuImage, IMAGE_FORMAT, ImageRegistry, ImageShared};
    use crate::primitives::{Color, Primitive, Quad, Rect};
    use crate::quad::{QuadShared, QuadWindowState};

    struct Harness {
        device: wgpu::Device,
        queue: wgpu::Queue,
        quad_shared: QuadShared,
        image_shared: ImageShared,
        registry: ImageRegistry<GpuImage>,
    }

    /// Os pixels de um alvo, linha a linha.
    struct Pixels {
        width: usize,
        data: Vec<[u8; 4]>,
    }

    impl Pixels {
        fn at(&self, x: usize, y: usize) -> [u8; 4] {
            self.data[y * self.width + x]
        }
    }

    impl Harness {
        fn new() -> Option<Self> {
            let instance = wgpu::Instance::default();
            let adapter = ["hardware", "software"].iter().find_map(|kind| {
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    force_fallback_adapter: *kind == "software",
                    ..Default::default()
                }))
                .ok()
            });
            let Some(adapter) = adapter else {
                eprintln!("sem adapter wgpu nesta máquina: teste de GPU pulado");
                return None;
            };
            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                    .ok()?;
            let quad_shared = QuadShared::new(&device, IMAGE_FORMAT);
            let image_shared = ImageShared::new(&device, IMAGE_FORMAT, &quad_shared);
            Some(Self {
                device,
                queue,
                quad_shared,
                image_shared,
                registry: ImageRegistry::new(),
            })
        }

        /// Desenha `primitives` (em pixels lógicos) na camada `Grid`, sobre um
        /// alvo preto de `width x height` **físicos**, com o fator `scale`.
        fn render(&self, primitives: &[Primitive], width: u32, height: u32, scale: f32) -> Pixels {
            let target = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("teste/alvo"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: IMAGE_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = target.create_view(&wgpu::TextureViewDescriptor::default());
            let mut state =
                QuadWindowState::new(&self.device, &self.queue, &self.quad_shared, width, height);
            let resolved = resolve_layer(primitives);
            state.prepare_layer(
                Layer::Grid,
                &self.device,
                &self.queue,
                &resolved.batches,
                &self.registry,
                false,
                scale,
                width,
                height,
            );
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("teste/passe"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                state.render_layer(
                    Layer::Grid,
                    &self.quad_shared,
                    &self.image_shared,
                    &self.registry,
                    None,
                    &mut pass,
                );
            }
            // `bytes_per_row` de uma cópia para buffer é múltiplo de 256.
            let row = (width * 4).div_ceil(256) * 256;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("teste/leitura"),
                size: u64::from(row * height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &target,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(height),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(Some(encoder.finish()));
            readback.slice(..).map_async(wgpu::MapMode::Read, |result| {
                result.expect("mapeamento da leitura");
            });
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("poll");
            let mapped = readback
                .slice(..)
                .get_mapped_range()
                .expect("intervalo mapeado");
            let mut data = Vec::new();
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let at = y * row as usize + x * 4;
                    data.push([mapped[at], mapped[at + 1], mapped[at + 2], mapped[at + 3]]);
                }
            }
            Pixels {
                width: width as usize,
                data,
            }
        }
    }

    fn pt(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    fn line(points: &[Point], width: f32) -> Primitive {
        Primitive::Polyline(Polyline {
            points: points.to_vec(),
            width,
            color: Color::WHITE,
        })
    }

    fn close(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tolerance)
    }

    /// Linha horizontal de espessura 2 centrada numa fronteira de pixel: as
    /// duas linhas de pixel de dentro cheias, as de fora vazias, nenhuma faixa
    /// cinza. Longe das pontas, para a ponta redonda não interferir.
    #[test]
    fn a_horizontal_line_of_width_2_paints_the_expected_rows() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let pixels = gpu.render(&[line(&[pt(-4.0, 4.0), pt(20.0, 4.0)], 2.0)], 16, 8, 1.0);
        for x in 2..14 {
            for y in 0..8 {
                let want = if y == 3 || y == 4 { WHITE } else { BLACK };
                assert!(
                    close(pixels.at(x, y), want, 2),
                    "({x},{y}) = {:?}",
                    pixels.at(x, y)
                );
            }
        }
    }

    /// Fora da fronteira de pixel a borda é antisserrilhada: o centro em
    /// `y = 3.75` cobre `[2.75, 4.75]`, então a linha 2 recebe ~16% e a 4 ~84%.
    #[test]
    fn a_horizontal_line_off_the_pixel_grid_has_an_antialiased_edge() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let pixels = gpu.render(&[line(&[pt(-4.0, 3.75), pt(20.0, 3.75)], 2.0)], 16, 8, 1.0);
        let top = pixels.at(8, 2)[0];
        let bottom = pixels.at(8, 4)[0];
        assert!((20..70).contains(&top), "linha 2 = {top}");
        assert!((190..236).contains(&bottom), "linha 4 = {bottom}");
        assert!(close(pixels.at(8, 3), WHITE, 2));
        assert!(close(pixels.at(8, 1), BLACK, 2));
        assert!(close(pixels.at(8, 5), BLACK, 2));
    }

    /// Cada ponto de uma polilinha dobrada, nos segmentos e nos vértices, cai
    /// num pixel pintado: nenhum buraco entre segmentos.
    #[test]
    fn a_bent_polyline_has_no_gap_between_segments() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let points = [pt(2.0, 2.0), pt(8.0, 12.0), pt(14.0, 4.0), pt(20.0, 14.0)];
        let pixels = gpu.render(&[line(&points, 2.0)], 24, 16, 1.0);
        for pair in points.windows(2) {
            for step in 0..=40 {
                let t = step as f32 / 40.0;
                let x = pair[0].x + (pair[1].x - pair[0].x) * t;
                let y = pair[0].y + (pair[1].y - pair[0].y) * t;
                let value = pixels.at(x.floor() as usize, y.floor() as usize)[0];
                assert!(value >= 128, "buraco em ({x:.2},{y:.2}): {value}");
            }
        }
    }

    /// Uma diagonal partida em vários segmentos colineares se pinta como a
    /// inteira, a menos da leve soma de cobertura nas bordas das junções.
    #[test]
    fn a_diagonal_split_in_segments_matches_the_unsplit_line() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let whole = gpu.render(&[line(&[pt(0.0, 0.0), pt(16.0, 16.0)], 2.0)], 16, 16, 1.0);
        let split = gpu.render(
            &[line(
                &[
                    pt(0.0, 0.0),
                    pt(4.0, 4.0),
                    pt(8.0, 8.0),
                    pt(12.0, 12.0),
                    pt(16.0, 16.0),
                ],
                2.0,
            )],
            16,
            16,
            1.0,
        );
        for i in 1..15 {
            assert!(split.at(i, i)[0] > 200, "buraco na diagonal em {i}");
        }
        for y in 0..16 {
            for x in 0..16 {
                assert!(
                    close(whole.at(x, y), split.at(x, y), 48),
                    "({x},{y}): {:?} contra {:?}",
                    whole.at(x, y),
                    split.at(x, y)
                );
            }
        }
    }

    /// Com fator 2 a espessura lógica 1 vira 2 pixels físicos, na linha lógica
    /// 2 (`y` físico 4): as mesmas linhas de pixel do caso de fator 1.
    #[test]
    fn scale_2_draws_the_same_line_in_physical_pixels() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let pixels = gpu.render(&[line(&[pt(-2.0, 2.0), pt(10.0, 2.0)], 1.0)], 16, 8, 2.0);
        for x in 2..14 {
            for y in 0..8 {
                let want = if y == 3 || y == 4 { WHITE } else { BLACK };
                assert!(
                    close(pixels.at(x, y), want, 2),
                    "({x},{y}) = {:?}",
                    pixels.at(x, y)
                );
            }
        }
    }

    /// O alvo de uma área: `n` faixas de largura não binária, a cor translúcida.
    fn area(first_x: f32, step: f32, n: usize, y_at: impl Fn(f32) -> f32, base: f32) -> Primitive {
        Primitive::Area(Area {
            points: (0..=n)
                .map(|i| {
                    let x = first_x + step * i as f32;
                    pt(x, y_at(x))
                })
                .collect(),
            baseline_y: base,
            color: Color {
                r: 1.0,
                g: 0.5,
                b: 0.0,
                a: 0.4,
            },
        })
    }

    /// Abaixo da borda de cima, toda coluna da área tem exatamente a mesma cor:
    /// sem costura entre faixas, e a cor translúcida só uma vez por pixel.
    #[test]
    fn an_area_of_60_strips_has_no_seam_below_its_edge() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        for (y_at, label) in [
            ((|_: f32| 10.3) as fn(f32) -> f32, "borda horizontal"),
            (
                (|x: f32| 8.3 + (x - 2.0) * 0.1) as fn(f32) -> f32,
                "borda inclinada",
            ),
        ] {
            let pixels = gpu.render(&[area(2.0, 0.987, 60, y_at, 30.0)], 64, 32, 1.0);
            let first = pixels.at(4, 25);
            // 0.4 de alfa sobre preto opaco: vermelho ~ 255 * 0.4, alfa do alvo 255.
            assert!(
                close(first, [102, 51, 0, 255], 3),
                "{label}: cor da área = {first:?}"
            );
            let last_x = (2.0_f32 + 0.987 * 60.0 - 0.5).floor() as usize;
            for x in 3..last_x {
                for y in 20..30 {
                    assert_eq!(pixels.at(x, y), first, "{label}: costura em ({x},{y})");
                }
            }
            // Fora da base e fora do `x` da área: nada.
            assert_eq!(pixels.at(10, 31), BLACK, "{label}: abaixo da base");
            assert_eq!(pixels.at(0, 25), BLACK, "{label}: à esquerda");
            assert_eq!(pixels.at(63, 25), BLACK, "{label}: à direita");
        }
    }

    /// A borda de cima da área é antisserrilhada na vertical e igual em toda
    /// coluna quando a linha é horizontal: com `y = 10.3`, o centro da linha 10
    /// está em 10.5, 0.2 abaixo da borda, e recebe 70% de cobertura; a linha 9
    /// não recebe nada.
    #[test]
    fn an_area_top_edge_is_antialiased_vertically_and_uniform_across_columns() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let pixels = gpu.render(&[area(2.0, 0.987, 60, |_| 10.3, 30.0)], 64, 32, 1.0);
        let edge = pixels.at(4, 10);
        // cobertura = 10.5 - 10.3 + 0.5 = 0.7 de 0.4 de alfa.
        assert!(close(edge, [71, 36, 0, 255], 4), "borda = {edge:?}");
        for x in 3..60 {
            assert_eq!(pixels.at(x, 10), edge, "borda diferente em {x}");
            assert_eq!(pixels.at(x, 9), BLACK, "acima da borda em {x}");
        }
    }

    /// Com fator 2 a área também fica sem costura, e a base lógica 15 vira o
    /// `y` físico 30.
    #[test]
    fn an_area_at_scale_2_has_no_seam_and_stops_at_the_physical_baseline() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let pixels = gpu.render(&[area(1.0, 0.4935, 60, |_| 5.15, 15.0)], 64, 32, 2.0);
        let first = pixels.at(4, 25);
        assert!(close(first, [102, 51, 0, 255], 3), "cor = {first:?}");
        for x in 3..60 {
            for y in 20..30 {
                assert_eq!(pixels.at(x, y), first, "costura em ({x},{y})");
            }
        }
        assert_eq!(pixels.at(10, 30), BLACK);
        assert_eq!(pixels.at(10, 31), BLACK);
    }

    /// A ordem do stream vale entre pipelines: um quad antes da linha fica por
    /// baixo, e um depois, por cima.
    #[test]
    fn segments_respect_the_stream_order_against_quads() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let red = Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let cover = |x: f32| {
            Primitive::Quad(Quad {
                rect: Rect {
                    x,
                    y: 0.0,
                    width: 4.0,
                    height: 8.0,
                },
                color: red,
            })
        };
        let pixels = gpu.render(
            &[
                cover(0.0),
                line(&[pt(-4.0, 4.0), pt(20.0, 4.0)], 2.0),
                cover(8.0),
            ],
            16,
            8,
            1.0,
        );
        // Quad antes: a linha branca por cima. Quad depois: o vermelho por cima.
        assert!(close(pixels.at(2, 4), WHITE, 2), "{:?}", pixels.at(2, 4));
        assert!(
            close(pixels.at(9, 4), [255, 0, 0, 255], 2),
            "{:?}",
            pixels.at(9, 4)
        );
    }
}
