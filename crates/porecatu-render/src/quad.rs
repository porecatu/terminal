// SPDX-License-Identifier: GPL-3.0-or-later

//! Pipeline de geometria: quads instanciados, cantos arredondados via SDF
//! no fragment shader (docs/arquitetura.md seção 5).
//!
//! Duas fases, como o `glyphon` já exige para texto: `prepare` monta e
//! sobe o buffer de instâncias para a GPU antes do render pass começar;
//! `render` só desenha, dentro do pass.

use wgpu::util::DeviceExt;

use crate::primitives::{Quad, RoundedQuad};

const SHADER: &str = include_str!("quad.wgsl");

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
    fn from_quad(quad: &Quad) -> Self {
        Self {
            rect_pos: [quad.rect.x, quad.rect.y],
            rect_size: [quad.rect.width, quad.rect.height],
            color: color_to_array(quad.color),
            radius: 0.0,
            border_width: 0.0,
            _pad: [0.0, 0.0],
            border_color: [0.0, 0.0, 0.0, 0.0],
        }
    }

    fn from_rounded_quad(quad: &RoundedQuad) -> Self {
        Self {
            rect_pos: [quad.rect.x, quad.rect.y],
            rect_size: [quad.rect.width, quad.rect.height],
            color: color_to_array(quad.color),
            radius: quad.radius,
            border_width: quad.border_width,
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

pub struct QuadPipeline {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_count: u32,
}

impl QuadPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("porecatu-render/quad-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("porecatu-render/quad-vertices"),
            contents: bytemuck::cast_slice(&VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("porecatu-render/quad-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("porecatu-render/quad-instances"),
            size: std::mem::size_of::<Instance>() as u64 * 64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("porecatu-render/quad-bind-group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
            vertex_buffer,
            uniform_buffer,
            bind_group,
            instance_buffer,
            instance_count: 0,
        }
    }

    pub fn resize(&mut self, queue: &wgpu::Queue, width: u32, height: u32) {
        let uniforms = Uniforms {
            resolution: [width as f32, height as f32],
            _pad: [0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Monta as instâncias de `quads` seguido de `rounded` e sobe pra GPU.
    /// Chamar antes de abrir o render pass -- recria o buffer quando a
    /// contagem excede a capacidade atual, do jeito que o `glyphon` faz
    /// para o dele.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        quads: &[Quad],
        rounded: &[RoundedQuad],
    ) {
        let mut instances: Vec<Instance> = Vec::with_capacity(quads.len() + rounded.len());
        instances.extend(quads.iter().map(Instance::from_quad));
        instances.extend(rounded.iter().map(Instance::from_rounded_quad));
        self.instance_count = instances.len() as u32;

        if instances.is_empty() {
            return;
        }

        let needed = (std::mem::size_of::<Instance>() * instances.len()) as u64;
        if needed > self.instance_buffer.size() {
            self.instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("porecatu-render/quad-instances"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
        } else {
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
        }
    }

    /// Desenha o que `prepare` montou. Chamar dentro do render pass.
    pub fn render<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        if self.instance_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        pass.draw(0..4, 0..self.instance_count);
    }
}
