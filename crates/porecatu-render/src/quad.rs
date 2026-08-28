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
use crate::primitives::{Quad, Rect, RoundedQuad, scale_rect};

const SHADER: &str = include_str!("quad.wgsl");
const INITIAL_INSTANCE_CAPACITY: u64 = 64;

/// Vértice estático do quad unitário (-1..1), compartilhado por toda
/// instância -- a instância escala e posiciona via `Instance`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
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

impl Instance {
    fn from_quad(quad: &Quad, scale: f32) -> Self {
        Self {
            rect_pos: [quad.rect.x * scale, quad.rect.y * scale],
            rect_size: [quad.rect.width * scale, quad.rect.height * scale],
            color: color_to_array(quad.color),
            radius: 0.0,
            border_width: 0.0,
            _pad: [0.0, 0.0],
            border_color: [0.0, 0.0, 0.0, 0.0],
        }
    }

    fn from_rounded_quad(quad: &RoundedQuad, scale: f32) -> Self {
        Self {
            rect_pos: [quad.rect.x * scale, quad.rect.y * scale],
            rect_size: [quad.rect.width * scale, quad.rect.height * scale],
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
    bind_group_layout: wgpu::BindGroupLayout,
    vertex_buffer: wgpu::Buffer,
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
                visibility: wgpu::ShaderStages::VERTEX,
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

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("porecatu-render/quad-pipeline"),
            layout: Some(&pipeline_layout),
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
                    // O fragment shader devolve cor premultiplicada
                    // (`color.rgb * alpha, alpha`, quad.wgsl) -- o blend
                    // tem que ser o par certo, não `ALPHA_BLENDING`
                    // (straight). Com o par errado o alpha era aplicado em
                    // dobro na faixa de antialiasing do SDF, escurecendo um
                    // anel exatamente no contorno de todo canto arredondado
                    // -- mascarado enquanto havia borda ali, visível assim
                    // que a pílula do grupo (cor cheia, sem borda) passou a
                    // ficar sobre a cápsula da mesma cor.
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

        Self {
            pipeline,
            bind_group_layout,
            vertex_buffer,
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
    draws: Vec<(ScissorRect, Range<u32>)>,
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
        Self {
            instance_buffer,
            instance_capacity,
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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_layer(
        &mut self,
        layer: Layer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: &[GeometryBatch],
        scale: f32,
        surface_width: u32,
        surface_height: u32,
    ) {
        let state = &mut self.layers[layer.index()];
        state.draws.clear();
        let mut instances: Vec<Instance> = Vec::new();

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
            let start = instances.len() as u32;
            // Na ordem de chegada -- não por tipo. Separar quad e
            // arredondado em dois `extend` desenhava todo arredondado por
            // cima de todo quad do batch, não importa quem foi pushado
            // primeiro (era o que escondia o cursor atrás do quadro do
            // terminal).
            instances.extend(batch.geometry.iter().map(|g| match g {
                GeometryPrimitive::Quad(q) => Instance::from_quad(q, scale),
                GeometryPrimitive::Rounded(q) => Instance::from_rounded_quad(q, scale),
            }));
            let end = instances.len() as u32;
            if !scissor.is_empty() {
                state.draws.push((scissor, start..end));
            }
        }

        if instances.is_empty() {
            return;
        }

        let needed = instances.len() as u64;
        if needed > state.instance_capacity {
            state.instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("porecatu-render/quad-instances"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            state.instance_capacity = needed;
        } else {
            queue.write_buffer(&state.instance_buffer, 0, bytemuck::cast_slice(&instances));
        }
    }

    /// Desenha o que `prepare_layer` montou para `layer`: um `draw` por
    /// batch, com seu próprio `set_scissor_rect` -- é isso que faz o
    /// recorte valer (ADR-0018).
    pub(crate) fn render_layer<'pass>(
        &'pass self,
        layer: Layer,
        shared: &'pass QuadShared,
        pass: &mut wgpu::RenderPass<'pass>,
    ) {
        let state = &self.layers[layer.index()];
        if state.draws.is_empty() {
            return;
        }
        pass.set_pipeline(&shared.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, shared.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, state.instance_buffer.slice(..));
        for (scissor, range) in &state.draws {
            pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
            pass.draw(0..4, range.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
