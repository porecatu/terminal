// SPDX-License-Identifier: GPL-3.0-or-later

//! Pipeline de geometria: quads instanciados, cantos arredondados via SDF
//! no fragment shader (docs/arquitetura.md seção 5). Dividido em duas
//! partes (ADR-0018): [`QuadShared`] -- pipeline, shader, vértice estático
//! -- vive em `GpuContext`, um por processo; [`QuadWindowState`] -- buffer
//! de instâncias, uniforme de resolução, os batches do frame -- vive em
//! `WindowSurface`, um por janela.
//!
//! Um batch é um grupo de quads/arredondados contíguos no stream original
//! que compartilham o mesmo clip (`frame::resolve_layer`); o recorte vira
//! `set_scissor_rect` por batch, quebrando a instância única em vários
//! `draw` (ADR-0018: "quebrando o batch quando o clip muda").

use std::ops::Range;

use wgpu::util::DeviceExt;

use crate::frame::{GeometryBatch, GeometryPrimitive, Layer};
use crate::image::{GpuImage, ImageInstance, ImageRegistry, ImageShared};
use crate::primitives::{ImageId, Quad, Rect, RoundedQuad, scale_rect};
use crate::segment::{SegmentInstance, SegmentLayerBuffer, SegmentShared};

const SHADER: &str = concat!(include_str!("sdf.wgsl"), include_str!("quad.wgsl"));
const INITIAL_INSTANCE_CAPACITY: u64 = 64;
const INITIAL_IMAGE_CAPACITY: u64 = 4;

/// Vértice estático do quad unitário (-1..1), compartilhado por toda
/// instância -- a instância escala e posiciona via `Instance`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Vertex {
    corner: [f32; 2],
}

const VERTICES: [Vertex; 4] = [
    Vertex {
        corner: [-1.0, -1.0],
    },
    Vertex {
        corner: [1.0, -1.0],
    },
    Vertex {
        corner: [-1.0, 1.0],
    },
    Vertex { corner: [1.0, 1.0] },
];

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    rect_pos: [f32; 2],
    rect_size: [f32; 2],
    color: [f32; 4],
    radius: f32,
    border_width: f32,
    _pad: [f32; 2],
    border_color: [f32; 4],
}

/// Arredonda os dois cantos de `rect` (já em pixels físicos, pós-`scale`) ao
/// pixel inteiro mais próximo, e devolve posição/tamanho derivados desses
/// cantos -- não largura/altura arredondadas isoladamente, que ainda
/// deixaria dois quads vizinhos discordando sobre uma borda compartilhada
/// se ela vier de dois call sites com valores próximos mas não
/// bit-idênticos. `quad.wgsl` antisserrilha as quatro bordas de todo quad
/// via SDF (`smoothstep`); isso só fecha sem costura entre dois quads
/// sólidos adjacentes quando a borda comum cai num pixel físico inteiro --
/// do contrário o blend "over" sequencial de duas bordas parcialmente
/// transparentes não soma cobertura total, e sobra uma linha fina da cor de
/// trás (a costura de blocos/box-drawing que motivou isto; ver `paint.rs`).
/// Mesma razão que `text.rs` já arredonda a origem de todo `TextRun`.
pub(crate) fn snap_rect_to_physical_pixels(rect: Rect, scale: f32) -> ([f32; 2], [f32; 2]) {
    let x0 = (rect.x * scale).round();
    let y0 = (rect.y * scale).round();
    let x1 = ((rect.x + rect.width) * scale).round();
    let y1 = ((rect.y + rect.height) * scale).round();
    ([x0, y0], [x1 - x0, y1 - y0])
}

impl Instance {
    fn from_quad(quad: &Quad, scale: f32) -> Self {
        let (rect_pos, rect_size) = snap_rect_to_physical_pixels(quad.rect, scale);
        Self {
            rect_pos,
            rect_size,
            color: color_to_array(quad.color),
            radius: 0.0,
            border_width: 0.0,
            _pad: [0.0, 0.0],
            border_color: [0.0, 0.0, 0.0, 0.0],
        }
    }

    fn from_rounded_quad(quad: &RoundedQuad, scale: f32) -> Self {
        let (rect_pos, rect_size) = snap_rect_to_physical_pixels(quad.rect, scale);
        Self {
            rect_pos,
            rect_size,
            color: color_to_array(quad.color),
            radius: quad.radius * scale,
            border_width: quad.border_width * scale,
            _pad: [0.0, 0.0],
            border_color: color_to_array(quad.border_color),
        }
    }
}

fn color_to_array(color: wgpu::Color) -> [f32; 4] {
    [
        color.r as f32,
        color.g as f32,
        color.b as f32,
        color.a as f32,
    ]
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    resolution: [f32; 2],
    _pad: [f32; 2],
}

/// Pipeline e recursos que não variam por janela -- um por processo
/// (ADR-0018), dono de `GpuContext`.
pub(crate) struct QuadShared {
    pipeline: wgpu::RenderPipeline,
    /// Mesmo shader, `BlendState::REPLACE` (`Primitive::Backdrop`).
    backdrop_pipeline: wgpu::RenderPipeline,
    /// `Backdrop` arredondado, primeira metade: `dst * (1 - cobertura)`
    /// (`fs_erase`). A segunda é `add_pipeline`.
    erase_pipeline: wgpu::RenderPipeline,
    /// `Backdrop` arredondado, segunda metade: soma a cor premultiplicada pela
    /// cobertura (`dst + src`), o que fecha `lerp(dst, cor, cobertura)`.
    add_pipeline: wgpu::RenderPipeline,
    /// Grupo 0 (uniforme de resolução da janela): o pipeline de imagem usa o
    /// mesmo layout, e portanto o mesmo bind group de cada janela.
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) vertex_buffer: wgpu::Buffer,
    /// Pipeline de `Polyline` e `Area` (ADR-0094): mesmo layout de grupo 0 e
    /// mesmo vértice estático, outro shader e outro layout de instância.
    segment: SegmentShared,
}

impl QuadShared {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("porecatu-render/quad-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("porecatu-render/quad-vertices"),
            contents: bytemuck::cast_slice(&VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("porecatu-render/quad-bind-group-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // O fragmento do `BlurredBackdrop` (`fs_screen`, image.wgsl) lê
                // a resolução para amostrar pela coordenada de tela.
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("porecatu-render/quad-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
        };
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                1 => Float32x2, // rect_pos
                2 => Float32x2, // rect_size
                3 => Float32x4, // color
                4 => Float32,   // radius
                5 => Float32,   // border_width
                6 => Float32x2, // _pad
                7 => Float32x4, // border_color
            ],
        };

        let make_pipeline =
            |label: &'static str, fragment_entry: &'static str, blend: wgpu::BlendState| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        buffers: &[Some(vertex_layout.clone()), Some(instance_layout.clone())],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(fragment_entry),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            // O fragment shader devolve cor premultiplicada
                            // (`color.rgb * alpha, alpha`, quad.wgsl) -- o blend
                            // tem que ser o par certo, não `ALPHA_BLENDING`
                            // (straight). Com o par errado o alpha era aplicado em
                            // dobro na faixa de antialiasing do SDF, escurecendo um
                            // anel exatamente no contorno de todo canto arredondado
                            // -- mascarado enquanto havia borda ali, visível assim
                            // que a pílula do grupo (cor cheia, sem borda) passou a
                            // ficar sobre a cápsula da mesma cor.
                            blend: Some(blend),
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
                })
            };
        let pipeline = make_pipeline(
            "porecatu-render/quad-pipeline",
            "fs_main",
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        let backdrop_pipeline = make_pipeline(
            "porecatu-render/backdrop-pipeline",
            "fs_main",
            wgpu::BlendState::REPLACE,
        );
        // `Backdrop` de raio > 0 não usa `REPLACE`: ele escreveria o retângulo
        // inteiro, inclusive a cobertura zero dos cantos de fora do raio e a
        // faixa antialiasada, e ali o que estava embaixo (a imagem da janela, o
        // que está atrás dela) sumiria. São duas passadas, que juntas dão
        // `dst * (1 - c) + cor * c` -- igual ao `REPLACE` no miolo (`c = 1`),
        // intacto fora do raio (`c = 0`) e proporcional na borda.
        let erase_pipeline = make_pipeline(
            "porecatu-render/backdrop-erase-pipeline",
            "fs_erase",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            },
        );
        let add_pipeline = make_pipeline(
            "porecatu-render/backdrop-add-pipeline",
            "fs_main",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
            },
        );

        let segment = SegmentShared::new(device, format, &bind_group_layout);

        Self {
            pipeline,
            backdrop_pipeline,
            erase_pipeline,
            add_pipeline,
            bind_group_layout,
            vertex_buffer,
            segment,
        }
    }
}

/// Um retângulo de scissor já em pixels físicos, recortado às bordas da
/// surface -- nunca `None`: clip ausente vira "a surface inteira".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScissorRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ScissorRect {
    fn full(surface_width: u32, surface_height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width: surface_width,
            height: surface_height,
        }
    }

    /// `rect` já em pixels físicos. Recorta às bordas da surface -- um
    /// clip parcialmente fora da tela (ex.: trilha rolando) não pode virar
    /// `x + width > surface_width`, que o `wgpu` rejeita.
    fn clamped(rect: Rect, surface_width: u32, surface_height: u32) -> Self {
        let surface_width = surface_width as f32;
        let surface_height = surface_height as f32;
        let x0 = rect.x.clamp(0.0, surface_width);
        let y0 = rect.y.clamp(0.0, surface_height);
        let x1 = (rect.x + rect.width).clamp(x0, surface_width);
        let y1 = (rect.y + rect.height).clamp(y0, surface_height);
        Self {
            x: x0 as u32,
            y: y0 as u32,
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
        }
    }

    const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Instâncias e batches de uma única camada -- o que precisa sobreviver
/// entre `prepare` e `render` daquela camada especificamente, já que as
/// cinco desenham em sequência intercaladas com o texto de cada uma
/// (ordem "por cima" entre camadas, ADR-0018).
struct QuadLayerBuffer {
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    /// Instâncias das imagens da camada: outro layout de vértice, outro
    /// buffer (ADR-0061 §2).
    image_buffer: wgpu::Buffer,
    image_capacity: u64,
    /// Instâncias dos segmentos de polilinha e área da camada (ADR-0094 §4).
    segments: SegmentLayerBuffer,
    /// Na ordem do stream: quads, imagens e segmentos se intercalam.
    draws: Vec<Draw>,
}

/// Como uma execução de quads compõe com o destino.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuadMode {
    /// Blend premultiplicado normal.
    Blend,
    /// `Backdrop` de raio zero (fundo de célula): `REPLACE`. A forma é um
    /// retângulo alinhado ao pixel, sem cobertura parcial nem canto.
    Replace,
    /// `Backdrop` arredondado: apagar com cobertura e somar a cor, uma
    /// instância por vez para a ordem do stream valer (`erase`, depois `add`,
    /// de cada uma).
    RoundedBackdrop,
}

impl QuadMode {
    fn of(g: &GeometryPrimitive) -> Self {
        match g {
            GeometryPrimitive::Backdrop(q) if q.radius > 0.0 => Self::RoundedBackdrop,
            GeometryPrimitive::Backdrop(_) => Self::Replace,
            _ => Self::Blend,
        }
    }
}

/// Um `draw` da camada.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Draw {
    /// Execução contígua de quads de um batch, no mesmo modo de blend.
    Quads {
        scissor: ScissorRect,
        range: Range<u32>,
        mode: QuadMode,
    },
    /// Uma imagem: `index` é a posição dela em `image_buffer`.
    Image {
        scissor: ScissorRect,
        index: u32,
        image: ImageId,
        /// Qual dos dois bind groups da imagem (sampler `Repeat` ou
        /// `ClampToEdge`).
        repeat: bool,
    },
    /// Um `BlurredBackdrop`: `index` é a posição da instância em
    /// `image_buffer` (o mesmo layout de instância da imagem); a textura vem
    /// da fonte de desfoque da janela, não do registro.
    Blurred { scissor: ScissorRect, index: u32 },
    /// Execução contígua de segmentos de `Polyline`/`Area`: `range` indexa o
    /// buffer de segmentos da camada (ADR-0094 §4).
    Segments {
        scissor: ScissorRect,
        range: Range<u32>,
    },
}

/// Uma execução contígua de geometria que um único `draw` cobre: quads no
/// mesmo modo de blend, ou segmentos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    Quads(QuadMode),
    Segments,
}

impl Run {
    fn of(g: &GeometryPrimitive) -> Self {
        match g {
            GeometryPrimitive::Segment(_) => Self::Segments,
            other => Self::Quads(QuadMode::of(other)),
        }
    }
}

/// O que `prepare_layer` calcula de uma camada, sem tocar a GPU: os `draws`
/// na ordem do stream e as instâncias de cada pipeline. Separado do upload
/// para que ordem, lote e recorte sejam testáveis sem dispositivo.
#[derive(Debug, Default)]
struct LayerPlan {
    draws: Vec<Draw>,
    instances: Vec<Instance>,
    image_instances: Vec<ImageInstance>,
    segments: Vec<SegmentInstance>,
}

/// Fecha uma execução em `plan.draws`, se o scissor deixar algo à vista. As
/// instâncias já estão no plano de qualquer jeito.
fn close_run(plan: &mut LayerPlan, scissor: ScissorRect, run: Run, start: u32) {
    if scissor.is_empty() {
        return;
    }
    match run {
        Run::Quads(mode) => plan.draws.push(Draw::Quads {
            scissor,
            range: start..plan.instances.len() as u32,
            mode,
        }),
        Run::Segments => plan.draws.push(Draw::Segments {
            scissor,
            range: start..plan.segments.len() as u32,
        }),
    }
}

/// Monta o plano de uma camada a partir dos batches resolvidos. `has_image`
/// diz se o registro ainda tem o id (imagem removida não desenha) e
/// `blur_available` se a janela tem fonte de desfoque (ADR-0068 §5).
fn plan_layer(
    batches: &[GeometryBatch],
    has_image: impl Fn(ImageId) -> bool,
    blur_available: bool,
    scale: f32,
    surface_width: u32,
    surface_height: u32,
) -> LayerPlan {
    let mut plan = LayerPlan::default();

    for batch in batches {
        if batch.geometry.is_empty() {
            continue;
        }
        let scissor = match batch.clip {
            Some(rect) => {
                ScissorRect::clamped(scale_rect(rect, scale), surface_width, surface_height)
            }
            None => ScissorRect::full(surface_width, surface_height),
        };
        // `resolve_layer` põe cada imagem sozinha num batch (troca de
        // pipeline e de bind group): aqui ela vira um `draw` próprio, na
        // ordem do stream. Id de imagem já removida não desenha nada.
        if let [GeometryPrimitive::Image(image)] = batch.geometry.as_slice() {
            if !scissor.is_empty() && has_image(image.image) {
                plan.draws.push(Draw::Image {
                    scissor,
                    index: plan.image_instances.len() as u32,
                    image: image.image,
                    repeat: image.repeat,
                });
                plan.image_instances
                    .push(ImageInstance::from_primitive(image, scale));
            }
            continue;
        }
        // O desfoque também fica sozinho no batch, com a instância no mesmo
        // buffer das imagens.
        if let [GeometryPrimitive::Blurred(blurred)] = batch.geometry.as_slice() {
            if !scissor.is_empty() && blur_available {
                plan.draws.push(Draw::Blurred {
                    scissor,
                    index: plan.image_instances.len() as u32,
                });
                plan.image_instances.push(ImageInstance::screen_space(
                    blurred.mask,
                    blurred.mask_radius,
                    scale,
                ));
            }
            continue;
        }
        // Na ordem de chegada -- não por tipo. Separar quad e arredondado em
        // dois `extend` desenhava todo arredondado por cima de todo quad do
        // batch, não importa quem foi pushado primeiro (era o que escondia o
        // cursor atrás do quadro do terminal). Uma execução por pipeline e
        // por tipo de blend: `Backdrop` e segmentos trocam de pipeline, e a
        // ordem do stream continua valendo.
        let mut current: Option<(Run, u32)> = None;
        for g in &batch.geometry {
            let run = Run::of(g);
            if current.map(|(r, _)| r) != Some(run) {
                if let Some((open, start)) = current {
                    close_run(&mut plan, scissor, open, start);
                }
                let start = match run {
                    Run::Quads(_) => plan.instances.len(),
                    Run::Segments => plan.segments.len(),
                };
                current = Some((run, start as u32));
            }
            match g {
                GeometryPrimitive::Quad(q) => plan.instances.push(Instance::from_quad(q, scale)),
                GeometryPrimitive::Rounded(q) | GeometryPrimitive::Backdrop(q) => {
                    plan.instances.push(Instance::from_rounded_quad(q, scale));
                }
                GeometryPrimitive::Segment(s) => {
                    plan.segments.push(SegmentInstance::from_segment(s, scale));
                }
                // Nunca num batch com quads (ver acima); se algum dia
                // chegar, desenhar como quad seria errado, e calar também.
                GeometryPrimitive::Image(_) | GeometryPrimitive::Blurred(_) => {
                    unreachable!("imagem e desfoque sempre sozinhos no batch (resolve_layer)")
                }
            }
        }
        if let Some((open, start)) = current {
            close_run(&mut plan, scissor, open, start);
        }
    }

    plan
}

/// Qual buffer de instâncias está no slot 1 de vértice: cada pipeline lê o seu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstanceSlot {
    Quads,
    Images,
    Segments,
}

impl QuadLayerBuffer {
    fn new(device: &wgpu::Device) -> Self {
        let instance_capacity = INITIAL_INSTANCE_CAPACITY;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("porecatu-render/quad-instances"),
            size: std::mem::size_of::<Instance>() as u64 * instance_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let image_capacity = INITIAL_IMAGE_CAPACITY;
        let image_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("porecatu-render/image-instances"),
            size: std::mem::size_of::<ImageInstance>() as u64 * image_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            instance_buffer,
            instance_capacity,
            image_buffer,
            image_capacity,
            segments: SegmentLayerBuffer::new(device),
            draws: Vec::new(),
        }
    }
}

/// Estado por janela: uniforme de resolução e bind group compartilhados
/// pelas cinco camadas (a resolução é a mesma para todas), e um buffer de
/// instâncias por camada.
pub(crate) struct QuadWindowState {
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    layers: [QuadLayerBuffer; 5],
}

impl QuadWindowState {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        shared: &QuadShared,
        width: u32,
        height: u32,
    ) -> Self {
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("porecatu-render/quad-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("porecatu-render/quad-bind-group"),
            layout: &shared.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let layers = std::array::from_fn(|_| QuadLayerBuffer::new(device));

        let mut state = Self {
            uniform_buffer,
            bind_group,
            layers,
        };
        state.resize(queue, width, height);
        state
    }

    pub(crate) fn resize(&mut self, queue: &wgpu::Queue, width: u32, height: u32) {
        let uniforms = Uniforms {
            resolution: [width as f32, height as f32],
            _pad: [0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Monta as instâncias de todos os batches de uma camada, na ordem do
    /// stream, e sobe pra GPU num upload só. `scale` converte de pixels
    /// lógicos (contrato de [`Rect`]) para físicos -- o único ponto de
    /// conversão fica em `WindowSurface`, que é quem chama isto.
    ///
    /// `blur_available` diz se a janela tem fonte de desfoque: sem ela um
    /// `BlurredBackdrop` não vira `draw` nenhum (ADR-0068 §5).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_layer(
        &mut self,
        layer: Layer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: &[GeometryBatch],
        images: &ImageRegistry<GpuImage>,
        blur_available: bool,
        scale: f32,
        surface_width: u32,
        surface_height: u32,
    ) {
        let plan = plan_layer(
            batches,
            |id| images.contains(id),
            blur_available,
            scale,
            surface_width,
            surface_height,
        );
        let state = &mut self.layers[layer.index()];
        state.draws = plan.draws;

        let needed = plan.instances.len() as u64;
        if needed > state.instance_capacity {
            state.instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("porecatu-render/quad-instances"),
                contents: bytemuck::cast_slice(&plan.instances),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            state.instance_capacity = needed;
        } else if needed > 0 {
            queue.write_buffer(
                &state.instance_buffer,
                0,
                bytemuck::cast_slice(&plan.instances),
            );
        }

        let needed = plan.image_instances.len() as u64;
        if needed > state.image_capacity {
            state.image_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("porecatu-render/image-instances"),
                contents: bytemuck::cast_slice(&plan.image_instances),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            state.image_capacity = needed;
        } else if needed > 0 {
            queue.write_buffer(
                &state.image_buffer,
                0,
                bytemuck::cast_slice(&plan.image_instances),
            );
        }

        state.segments.upload(device, queue, &plan.segments);
    }

    /// Desenha o que `prepare_layer` montou para `layer`: um `draw` por
    /// batch, com seu próprio `set_scissor_rect` -- é isso que faz o
    /// recorte valer (ADR-0018). `blur` é o bind group da textura desfocada da
    /// janela (`BlurState::bind_group`); sem ele os `BlurredBackdrop` não
    /// desenham.
    pub(crate) fn render_layer<'pass>(
        &'pass self,
        layer: Layer,
        shared: &'pass QuadShared,
        image_shared: &'pass ImageShared,
        images: &'pass ImageRegistry<GpuImage>,
        blur: Option<&'pass wgpu::BindGroup>,
        pass: &mut wgpu::RenderPass<'pass>,
    ) {
        let state = &self.layers[layer.index()];
        if state.draws.is_empty() {
            return;
        }
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, shared.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, state.instance_buffer.slice(..));
        // O slot 1 de vértice é o buffer de instâncias do último pipeline: uma
        // imagem ou uma corrida de segmentos o troca, e o quad que vem depois
        // precisa do seu de volta.
        let mut slot = InstanceSlot::Quads;
        for draw in &state.draws {
            match draw {
                Draw::Quads {
                    scissor,
                    range,
                    mode,
                } => {
                    if slot != InstanceSlot::Quads {
                        pass.set_vertex_buffer(1, state.instance_buffer.slice(..));
                        slot = InstanceSlot::Quads;
                    }
                    pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
                    match mode {
                        QuadMode::Blend => {
                            pass.set_pipeline(&shared.pipeline);
                            pass.draw(0..4, range.clone());
                        }
                        QuadMode::Replace => {
                            pass.set_pipeline(&shared.backdrop_pipeline);
                            pass.draw(0..4, range.clone());
                        }
                        QuadMode::RoundedBackdrop => {
                            // Uma instância por vez: o erase e o add de cada
                            // uma, em ordem, antes da seguinte.
                            for index in range.clone() {
                                pass.set_pipeline(&shared.erase_pipeline);
                                pass.draw(0..4, index..index + 1);
                                pass.set_pipeline(&shared.add_pipeline);
                                pass.draw(0..4, index..index + 1);
                            }
                        }
                    }
                }
                Draw::Image {
                    scissor,
                    index,
                    image,
                    repeat,
                } => {
                    // Removida entre o `prepare` e o `render`: não desenha.
                    let Some(gpu_image) = images.get(*image) else {
                        continue;
                    };
                    pass.set_pipeline(&image_shared.pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_bind_group(1, gpu_image.bind_group(*repeat), &[]);
                    pass.set_vertex_buffer(1, state.image_buffer.slice(..));
                    slot = InstanceSlot::Images;
                    pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
                    pass.draw(0..4, *index..*index + 1);
                }
                Draw::Blurred { scissor, index } => {
                    let Some(blur_group) = blur else {
                        continue;
                    };
                    pass.set_pipeline(&image_shared.screen_pipeline);
                    pass.set_bind_group(0, &self.bind_group, &[]);
                    pass.set_bind_group(1, blur_group, &[]);
                    pass.set_vertex_buffer(1, state.image_buffer.slice(..));
                    slot = InstanceSlot::Images;
                    pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
                    pass.draw(0..4, *index..*index + 1);
                }
                Draw::Segments { scissor, range } => {
                    pass.set_pipeline(&shared.segment.pipeline);
                    if slot != InstanceSlot::Segments {
                        pass.set_vertex_buffer(1, state.segments.buffer.slice(..));
                        slot = InstanceSlot::Segments;
                    }
                    pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
                    pass.draw(0..4, range.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::resolve_layer;
    use crate::primitives::{Area, Color, Point, Polyline, Primitive};

    #[test]
    fn scale_rect_multiplies_every_field() {
        let rect = Rect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        let scaled = scale_rect(rect, 2.0);
        assert_eq!(
            scaled,
            Rect {
                x: 2.0,
                y: 4.0,
                width: 6.0,
                height: 8.0
            }
        );
    }

    #[test]
    fn snap_rect_to_physical_pixels_rounds_both_corners_not_width_in_isolation() {
        // Arredondar `width` isolado daria `round(4.3) = 4`; o certo é
        // `round(14.7) - round(10.4) = 15 - 10 = 5` -- é essa diferença que
        // fecha a costura entre dois quads vizinhos (ver o teste abaixo).
        let rect = Rect {
            x: 10.4,
            y: 0.0,
            width: 4.3,
            height: 1.0,
        };
        let (pos, size) = snap_rect_to_physical_pixels(rect, 1.0);
        assert_eq!(pos, [10.0, 0.0]);
        assert_eq!(size, [5.0, 1.0]);
    }

    #[test]
    fn adjacent_quads_share_an_exact_physical_pixel_boundary() {
        let a = Rect {
            x: 0.0,
            y: 0.0,
            width: 10.4,
            height: 1.0,
        };
        let b = Rect {
            x: 10.4,
            y: 0.0,
            width: 9.6,
            height: 1.0,
        };
        let (a_pos, a_size) = snap_rect_to_physical_pixels(a, 1.0);
        let (b_pos, _) = snap_rect_to_physical_pixels(b, 1.0);
        assert_eq!(a_pos[0] + a_size[0], b_pos[0]);
    }

    #[test]
    fn scissor_full_covers_surface() {
        let full = ScissorRect::full(800, 600);
        assert_eq!(
            full,
            ScissorRect {
                x: 0,
                y: 0,
                width: 800,
                height: 600
            }
        );
    }

    #[test]
    fn scissor_clamps_to_surface_bounds() {
        let rect = Rect {
            x: -10.0,
            y: 5.0,
            width: 50.0,
            height: 50.0,
        };
        let clamped = ScissorRect::clamped(rect, 30, 30);
        assert_eq!(clamped.x, 0);
        assert_eq!(clamped.y, 5);
        assert_eq!(clamped.width, 30);
        assert_eq!(clamped.height, 25);
    }

    #[test]
    fn scissor_fully_outside_surface_is_empty() {
        let rect = Rect {
            x: 1000.0,
            y: 1000.0,
            width: 10.0,
            height: 10.0,
        };
        let clamped = ScissorRect::clamped(rect, 30, 30);
        assert!(clamped.is_empty());
    }

    fn pt(x: f32, y: f32) -> Point {
        Point { x, y }
    }

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

    /// Uma polilinha opaca de `n` pontos (`n - 1` segmentos).
    fn polyline(n: usize) -> Primitive {
        Primitive::Polyline(Polyline {
            points: (0..n).map(|i| pt(i as f32, i as f32)).collect(),
            width: 2.0,
            color: Color::WHITE,
        })
    }

    fn plan(primitives: &[Primitive], scale: f32) -> LayerPlan {
        let resolved = resolve_layer(primitives);
        plan_layer(&resolved.batches, |_| true, false, scale, 100, 100)
    }

    fn kinds(plan: &LayerPlan) -> Vec<&'static str> {
        plan.draws
            .iter()
            .map(|d| match d {
                Draw::Quads { .. } => "quads",
                Draw::Image { .. } => "image",
                Draw::Blurred { .. } => "blurred",
                Draw::Segments { .. } => "segments",
            })
            .collect()
    }

    /// `n` pontos viram `n - 1` instâncias, e o draw as cobre todas.
    #[test]
    fn a_polyline_of_n_points_is_n_minus_1_instances_in_one_draw() {
        let plan = plan(&[polyline(5)], 1.0);
        assert_eq!(plan.segments.len(), 4);
        assert!(plan.instances.is_empty());
        assert_eq!(kinds(&plan), vec!["segments"]);
        let Draw::Segments { range, .. } = &plan.draws[0] else {
            unreachable!();
        };
        assert_eq!(*range, 0..4);
    }

    #[test]
    fn a_polyline_with_fewer_than_two_points_draws_nothing() {
        for n in 0..2 {
            let plan = plan(&[polyline(n)], 1.0);
            assert!(plan.segments.is_empty());
            assert!(plan.draws.is_empty());
        }
    }

    /// Quad, polilinha, quad: três draws, na ordem do stream, cada um no seu
    /// pipeline e no seu intervalo.
    #[test]
    fn a_polyline_between_two_quads_keeps_the_stream_order() {
        let plan = plan(&[quad(0.0), polyline(3), quad(1.0)], 1.0);
        assert_eq!(kinds(&plan), vec!["quads", "segments", "quads"]);
        let ranges: Vec<_> = plan
            .draws
            .iter()
            .map(|d| match d {
                Draw::Quads { range, .. } | Draw::Segments { range, .. } => range.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(ranges, vec![0..1, 0..2, 1..2]);
        assert_eq!(plan.instances.len(), 2);
        assert_eq!(plan.segments.len(), 2);
    }

    /// Uma área e polilinhas seguidas (a ordem do ADR-0096 §3: áreas, depois
    /// linhas) são uma corrida só: um `Draw::Segments`.
    #[test]
    fn contiguous_polylines_and_areas_are_one_segments_draw() {
        let area = Primitive::Area(Area {
            points: vec![pt(0.0, 1.0), pt(1.0, 1.0), pt(2.0, 1.0)],
            baseline_y: 9.0,
            color: Color::WHITE,
        });
        let plan = plan(&[area, polyline(3), polyline(4)], 1.0);
        assert_eq!(kinds(&plan), vec!["segments"]);
        let Draw::Segments { range, .. } = &plan.draws[0] else {
            unreachable!();
        };
        assert_eq!(*range, 0..(2 + 2 + 3));
        // Área primeiro (kind 1), linhas depois (kind 0), na ordem.
        let order: Vec<u32> = plan.segments.iter().map(|s| s.kind).collect();
        assert_eq!(order, vec![1, 1, 0, 0, 0, 0, 0]);
    }

    /// O recorte vira o scissor do `Draw::Segments`, em pixels físicos.
    #[test]
    fn the_clip_stack_applies_to_segments() {
        let clip = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let plan = plan(
            &[
                Primitive::PushClip(clip),
                polyline(3),
                Primitive::PopClip,
                polyline(3),
            ],
            2.0,
        );
        assert_eq!(kinds(&plan), vec!["segments", "segments"]);
        let scissors: Vec<_> = plan
            .draws
            .iter()
            .map(|d| match d {
                Draw::Segments { scissor, .. } => *scissor,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            scissors[0],
            ScissorRect {
                x: 20,
                y: 40,
                width: 60,
                height: 60
            }
        );
        assert_eq!(scissors[1], ScissorRect::full(100, 100));
    }

    #[test]
    fn a_clip_outside_the_surface_leaves_no_segments_draw() {
        let clip = Rect {
            x: 1000.0,
            y: 1000.0,
            width: 10.0,
            height: 10.0,
        };
        let plan = plan(
            &[Primitive::PushClip(clip), polyline(3), Primitive::PopClip],
            1.0,
        );
        assert!(plan.draws.is_empty());
    }

    #[test]
    fn scale_2_doubles_segment_coordinates_and_half_width() {
        let line = Primitive::Polyline(Polyline {
            points: vec![pt(1.0, 2.0), pt(3.0, 4.0)],
            width: 3.0,
            color: Color::WHITE,
        });
        let one = plan(std::slice::from_ref(&line), 1.0);
        let two = plan(std::slice::from_ref(&line), 2.0);
        assert_eq!(one.segments[0].p0, [1.0, 2.0]);
        assert_eq!(one.segments[0].half_width, 1.5);
        assert_eq!(two.segments[0].p0, [2.0, 4.0]);
        assert_eq!(two.segments[0].p1, [6.0, 8.0]);
        assert_eq!(two.segments[0].half_width, 3.0);
    }
}
