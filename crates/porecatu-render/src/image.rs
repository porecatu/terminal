// SPDX-License-Identifier: GPL-3.0-or-later

//! Imagens: registro de texturas e o pipeline que as desenha (ADR-0061 §2 e
//! §3). Sem domínio -- uma imagem é uma textura RGBA8 com mips e um
//! [`ImageId`] opaco; quem decide o que é "fundo de terminal" é
//! `porecatu-ui`.
//!
//! Dividido como o resto do crate (ADR-0018): [`ImageShared`] (pipeline,
//! samplers, layouts) e o [`ImageRegistry`] (as texturas) vivem em
//! `GpuContext`, **um por processo** -- toda janela desenha com a mesma
//! textura (RF-17.18). O que é por janela e por camada (o buffer de instâncias)
//! mora em `quad.rs`, porque uma imagem se intercala com os quads na ordem do
//! stream.

use std::collections::HashMap;

use crate::frame::ImagePrimitive;
use crate::primitives::ImageId;
use crate::quad::{QuadShared, snap_rect_to_physical_pixels};

const SHADER: &str = concat!(include_str!("sdf.wgsl"), include_str!("image.wgsl"));

/// `Rgba8Unorm`, **nunca** `Rgba8UnormSrgb` (ADR-0061 §3): os bytes do arquivo
/// vão crus para a GPU e saem crus na surface, como as cores do design. É a
/// mesma decisão do `remove_srgb_suffix()` da surface e do `ColorMode::Web` do
/// `glyphon`; um formato sRGB decodificaria a curva que a surface nunca
/// recodifica, e a imagem sairia escura.
pub(crate) const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const BYTES_PER_TEXEL: usize = 4;

/// Registro de imagens do processo. Genérico no que guarda para que os
/// identificadores (criação, remoção, id velho) sejam testáveis sem GPU; o
/// `GpuContext` o usa com [`GpuImage`].
pub(crate) struct ImageRegistry<T> {
    next: u32,
    entries: HashMap<ImageId, T>,
}

impl<T> ImageRegistry<T> {
    pub(crate) fn new() -> Self {
        Self {
            next: 0,
            entries: HashMap::new(),
        }
    }

    /// Guarda `entry` e devolve o identificador novo. Um id nunca é reusado:
    /// o de uma imagem removida continua inválido, em vez de apontar para a
    /// próxima que entrar no lugar.
    pub(crate) fn insert(&mut self, entry: T) -> ImageId {
        let id = ImageId(self.next);
        self.next = self.next.wrapping_add(1);
        self.entries.insert(id, entry);
        id
    }

    /// Remove a imagem; id desconhecido (ou já removido) não faz nada.
    pub(crate) fn remove(&mut self, id: ImageId) -> Option<T> {
        self.entries.remove(&id)
    }

    pub(crate) fn get(&self, id: ImageId) -> Option<&T> {
        self.entries.get(&id)
    }

    pub(crate) fn contains(&self, id: ImageId) -> bool {
        self.entries.contains_key(&id)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// O que o registro guarda por imagem: a textura (dona dos mips) e **dois**
/// bind groups sobre ela, um com o sampler `ClampToEdge` e outro com o
/// `Repeat`; `draw` escolhe pelo `repeat` da primitiva ([`Self::bind_group`]).
/// O shader tem um sampler só (`image.wgsl`): uma textura amostrada por dois
/// samplers no mesmo shader é recusada pelo Vulkan/naga. A view fica dentro
/// dos bind groups.
pub(crate) struct GpuImage {
    _texture: wgpu::Texture,
    clamp: wgpu::BindGroup,
    repeat: wgpu::BindGroup,
}

impl GpuImage {
    pub(crate) fn bind_group(&self, repeat: bool) -> &wgpu::BindGroup {
        if repeat { &self.repeat } else { &self.clamp }
    }
}

/// Entrada recusada por [`check_levels`]. `create_image` trata como erro de
/// programação (`panic`), como o `PopClip` sem `PushClip`: quem chama é
/// `porecatu-ui`, com o tamanho que a thread de carga já reduziu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LevelsError {
    EmptyImage,
    TooLarge { max: u32 },
    NoLevels,
    TooManyLevels { max: usize },
    WrongLength { level: usize, expected: usize },
}

/// Lado do nível `level` de uma imagem de lado `size`: metade a cada nível,
/// piso 1.
pub(crate) fn level_size(size: u32, level: usize) -> u32 {
    (size >> level).max(1)
}

/// Número de níveis de uma cadeia completa até 1x1: `floor(log2(max(w, h))) + 1`.
pub(crate) fn full_chain_len(width: u32, height: u32) -> usize {
    (u32::BITS - width.max(height).max(1).leading_zeros()) as usize
}

/// Confere o que `create_image` recebe antes de tocar a GPU: dimensões entre 1
/// e o limite do `Device`, ao menos um nível, no máximo a cadeia completa, e
/// cada nível com exatamente `w * h * 4` bytes.
pub(crate) fn check_levels(
    width: u32,
    height: u32,
    max_dim: u32,
    levels: &[&[u8]],
) -> Result<(), LevelsError> {
    if width == 0 || height == 0 {
        return Err(LevelsError::EmptyImage);
    }
    if width > max_dim || height > max_dim {
        return Err(LevelsError::TooLarge { max: max_dim });
    }
    if levels.is_empty() {
        return Err(LevelsError::NoLevels);
    }
    let max = full_chain_len(width, height);
    if levels.len() > max {
        return Err(LevelsError::TooManyLevels { max });
    }
    for (level, bytes) in levels.iter().enumerate() {
        let expected = level_size(width, level) as usize
            * level_size(height, level) as usize
            * BYTES_PER_TEXEL;
        if bytes.len() != expected {
            return Err(LevelsError::WrongLength { level, expected });
        }
    }
    Ok(())
}

/// Uma imagem por instância: o retângulo desenhado, o pedaço da textura que o
/// cobre, e a máscara -- tudo já em pixels **físicos**, exceto `uv`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ImageInstance {
    rect_pos: [f32; 2],
    rect_size: [f32; 2],
    uv_origin: [f32; 2],
    uv_size: [f32; 2],
    mask_pos: [f32; 2],
    mask_size: [f32; 2],
    mask_radius: f32,
    alpha: f32,
    _pad: [f32; 2],
}

impl ImageInstance {
    /// A conversão de lógico para físico acontece aqui (chamado de
    /// `WindowSurface`, o único ponto que o ADR-0018 permite), e as bordas de
    /// `rect` e de `mask` saem do mesmo arredondamento dos quads
    /// (`snap_rect_to_physical_pixels`: os dois cantos, não largura isolada).
    pub(crate) fn from_primitive(image: &ImagePrimitive, scale: f32) -> Self {
        let (rect_pos, rect_size) = snap_rect_to_physical_pixels(image.rect, scale);
        let (mask_pos, mask_size) = snap_rect_to_physical_pixels(image.mask, scale);
        Self {
            rect_pos,
            rect_size,
            uv_origin: [image.uv.x, image.uv.y],
            uv_size: [image.uv.width, image.uv.height],
            mask_pos,
            mask_size,
            mask_radius: image.mask_radius * scale,
            alpha: image.alpha.clamp(0.0, 1.0),
            _pad: [0.0, 0.0],
        }
    }
}

/// Pipeline, samplers e layouts do processo (ADR-0018).
pub(crate) struct ImageShared {
    pub(crate) pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    clamp_sampler: wgpu::Sampler,
    repeat_sampler: wgpu::Sampler,
}

impl ImageShared {
    /// O grupo 0 (uniforme de resolução da janela) e o vértice estático são
    /// os do quad: o mesmo `bind_group` de `QuadWindowState` serve aos dois
    /// pipelines.
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        quad_shared: &QuadShared,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("porecatu-render/image-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("porecatu-render/image-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("porecatu-render/image-pipeline-layout"),
            bind_group_layouts: &[Some(&quad_shared.bind_group_layout), Some(&texture_layout)],
            immediate_size: 0,
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<[f32; 2]>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
        };
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                1 => Float32x2, // rect_pos
                2 => Float32x2, // rect_size
                3 => Float32x2, // uv_origin
                4 => Float32x2, // uv_size
                5 => Float32x2, // mask_pos
                6 => Float32x2, // mask_size
                7 => Float32,   // mask_radius
                8 => Float32,   // alpha
            ],
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("porecatu-render/image-pipeline"),
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
                    // O shader devolve `rgb * a, a` (image.wgsl): o par certo é
                    // o premultiplicado, o mesmo do quad -- `ALPHA_BLENDING`
                    // aplicaria o alfa em dobro (CLAUDE.md, "Blend mode do
                    // pipeline de quad").
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

        let sampler = |label: &'static str, address_mode: wgpu::AddressMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: address_mode,
                address_mode_v: address_mode,
                address_mode_w: address_mode,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            })
        };
        Self {
            pipeline,
            texture_layout,
            clamp_sampler: sampler(
                "porecatu-render/image-clamp",
                wgpu::AddressMode::ClampToEdge,
            ),
            repeat_sampler: sampler("porecatu-render/image-repeat", wgpu::AddressMode::Repeat),
        }
    }

    /// Cria a textura (um nível por entrada de `levels`) e o bind group.
    /// `levels` já passou por [`check_levels`].
    pub(crate) fn create_image(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        levels: &[&[u8]],
    ) -> GpuImage {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("porecatu-render/image"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IMAGE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, bytes) in levels.iter().enumerate() {
            let level_width = level_size(width, level);
            let level_height = level_size(height, level);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(level_width * BYTES_PER_TEXEL as u32),
                    rows_per_image: Some(level_height),
                },
                wgpu::Extent3d {
                    width: level_width,
                    height: level_height,
                    depth_or_array_layers: 1,
                },
            );
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = |label: &'static str, sampler: &wgpu::Sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        GpuImage {
            clamp: bind_group(
                "porecatu-render/image-bind-group-clamp",
                &self.clamp_sampler,
            ),
            repeat: bind_group(
                "porecatu-render/image-bind-group-repeat",
                &self.repeat_sampler,
            ),
            _texture: texture,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Rect;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn primitive() -> ImagePrimitive {
        ImagePrimitive {
            rect: rect(10.4, 20.0, 100.0, 50.0),
            uv: rect(0.0, 0.0, 2.0, 1.0),
            repeat: true,
            mask: rect(0.0, 0.0, 200.0, 100.0),
            mask_radius: 6.0,
            alpha: 0.5,
            image: ImageId(0),
        }
    }

    #[test]
    fn the_registry_hands_out_distinct_ids_and_never_reuses_them() {
        let mut registry = ImageRegistry::new();
        let a = registry.insert("a");
        let b = registry.insert("b");
        assert_ne!(a, b);
        assert_eq!(registry.get(a), Some(&"a"));
        assert_eq!(registry.remove(a), Some("a"));
        assert!(!registry.contains(a));
        let c = registry.insert("c");
        assert_ne!(c, a, "o id de uma imagem removida não volta");
        assert_ne!(c, b);
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn removing_an_unknown_or_already_removed_id_is_a_no_op() {
        let mut registry = ImageRegistry::new();
        let a = registry.insert(1);
        assert_eq!(registry.remove(a), Some(1));
        assert_eq!(registry.remove(a), None);
        assert_eq!(registry.remove(ImageId(999)), None);
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn level_sizes_halve_down_to_one() {
        assert_eq!(level_size(16, 0), 16);
        assert_eq!(level_size(16, 3), 2);
        assert_eq!(level_size(16, 4), 1);
        assert_eq!(level_size(16, 9), 1);
        assert_eq!(level_size(5, 1), 2);
    }

    #[test]
    fn the_full_chain_is_floor_log2_plus_one() {
        assert_eq!(full_chain_len(1, 1), 1);
        assert_eq!(full_chain_len(2, 1), 2);
        assert_eq!(full_chain_len(16, 16), 5);
        assert_eq!(full_chain_len(16, 8), 5);
        assert_eq!(full_chain_len(10, 6), 4);
        assert_eq!(full_chain_len(8192, 4096), 14);
    }

    fn chain(width: u32, height: u32, count: usize) -> Vec<Vec<u8>> {
        (0..count)
            .map(|level| {
                vec![0; level_size(width, level) as usize * level_size(height, level) as usize * 4]
            })
            .collect()
    }

    fn as_slices(levels: &[Vec<u8>]) -> Vec<&[u8]> {
        levels.iter().map(Vec::as_slice).collect()
    }

    #[test]
    fn a_complete_or_partial_chain_is_accepted() {
        for count in 1..=5 {
            let levels = chain(16, 16, count);
            assert_eq!(check_levels(16, 16, 8192, &as_slices(&levels)), Ok(()));
        }
    }

    #[test]
    fn bad_levels_are_refused_before_the_gpu() {
        let one = chain(4, 4, 1);
        assert_eq!(
            check_levels(0, 4, 8192, &as_slices(&one)),
            Err(LevelsError::EmptyImage)
        );
        assert_eq!(
            check_levels(4, 4, 2, &as_slices(&one)),
            Err(LevelsError::TooLarge { max: 2 })
        );
        assert_eq!(check_levels(4, 4, 8192, &[]), Err(LevelsError::NoLevels));
        let many = chain(4, 4, 4);
        assert_eq!(
            check_levels(4, 4, 8192, &as_slices(&many)),
            Err(LevelsError::TooManyLevels { max: 3 })
        );
        let mut wrong = chain(4, 4, 2);
        wrong[1].pop();
        assert_eq!(
            check_levels(4, 4, 8192, &as_slices(&wrong)),
            Err(LevelsError::WrongLength {
                level: 1,
                expected: 16
            })
        );
    }

    #[test]
    fn the_instance_is_in_physical_pixels_with_snapped_edges() {
        let instance = ImageInstance::from_primitive(&primitive(), 2.0);
        // 10.4 * 2 = 20.8 -> 21; (10.4 + 100) * 2 = 220.8 -> 221.
        assert_eq!(instance.rect_pos, [21.0, 40.0]);
        assert_eq!(instance.rect_size, [200.0, 100.0]);
        assert_eq!(instance.mask_pos, [0.0, 0.0]);
        assert_eq!(instance.mask_size, [400.0, 200.0]);
        assert_eq!(instance.mask_radius, 12.0);
        // `uv` é de textura, não de pixel: a escala não o toca.
        assert_eq!(instance.uv_origin, [0.0, 0.0]);
        assert_eq!(instance.uv_size, [2.0, 1.0]);
        assert_eq!(instance.alpha, 0.5);
    }

    #[test]
    fn alpha_is_clamped() {
        let mut image = primitive();
        image.alpha = 3.0;
        assert_eq!(ImageInstance::from_primitive(&image, 1.0).alpha, 1.0);
        image.alpha = -1.0;
        assert_eq!(ImageInstance::from_primitive(&image, 1.0).alpha, 0.0);
    }

    #[test]
    fn the_instance_layout_matches_the_vertex_attributes() {
        // 6 x vec2 + 2 x f32 + 8 bytes de padding = 64 bytes: o
        // `array_stride` do pipeline é `size_of`, e os `location`s 1..=8
        // cobrem 14 floats.
        assert_eq!(std::mem::size_of::<ImageInstance>(), 64);
    }

    /// O shader tem de usar **um** sampler (Vulkan/naga recusam uma textura
    /// amostrada por dois, e o pipeline não nascia no Linux). Valida o WGSL
    /// concatenado sem GPU: é o teste que teria pegado isto no Windows.
    #[test]
    fn the_image_shader_samples_with_a_single_sampler() {
        let source = SHADER;
        assert_eq!(
            source.matches(": sampler;").count(),
            1,
            "image.wgsl declara mais de um sampler"
        );
        assert!(!source.contains("textureSampleGrad"));
        let module = wgpu::naga::front::wgsl::parse_str(source).expect("o WGSL de imagem parseia");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("o WGSL de imagem valida");
    }
}

/// Testes com GPU de verdade, sem janela: um alvo 8x8 `Rgba8Unorm`, os mesmos
/// pipelines e o mesmo `QuadWindowState` do app, e os pixels lidos de volta.
/// Sem adapter nenhum (nem o de software) cada teste se **pula** e diz isso --
/// é o único jeito de provar o shader, o blend e os samplers sem a etapa 4.
#[cfg(test)]
mod gpu_tests {
    use super::*;
    use crate::frame::{Layer, resolve_layer};
    use crate::primitives::{Color, Primitive, Quad, Rect};
    use crate::quad::QuadWindowState;

    const SIZE: u32 = 8;

    struct Harness {
        device: wgpu::Device,
        queue: wgpu::Queue,
        quad_shared: QuadShared,
        image_shared: ImageShared,
        registry: ImageRegistry<GpuImage>,
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

        fn image(&mut self, width: u32, height: u32, levels: &[&[u8]]) -> ImageId {
            let image =
                self.image_shared
                    .create_image(&self.device, &self.queue, width, height, levels);
            self.registry.insert(image)
        }

        /// Desenha `primitives` na camada `Grid` sobre um alvo limpo a preto e
        /// devolve os `SIZE x SIZE` pixels, linha a linha.
        fn render(&self, primitives: &[Primitive]) -> Vec<[u8; 4]> {
            let target = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("teste/alvo"),
                size: wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
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
                QuadWindowState::new(&self.device, &self.queue, &self.quad_shared, SIZE, SIZE);
            let resolved = resolve_layer(primitives);
            state.prepare_layer(
                Layer::Grid,
                &self.device,
                &self.queue,
                &resolved.batches,
                &self.registry,
                1.0,
                SIZE,
                SIZE,
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
                    &mut pass,
                );
            }
            // `bytes_per_row` de uma cópia para buffer é múltiplo de 256.
            let row = 256;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("teste/leitura"),
                size: u64::from(row * SIZE),
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
                        rows_per_image: Some(SIZE),
                    },
                },
                wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
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
            let data = readback
                .slice(..)
                .get_mapped_range()
                .expect("intervalo mapeado");
            let mut pixels = Vec::new();
            for y in 0..SIZE as usize {
                for x in 0..SIZE as usize {
                    let at = y * row as usize + x * 4;
                    pixels.push([data[at], data[at + 1], data[at + 2], data[at + 3]]);
                }
            }
            pixels
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn full() -> Rect {
        rect(0.0, 0.0, SIZE as f32, SIZE as f32)
    }

    fn blue_backdrop() -> Primitive {
        Primitive::Quad(Quad {
            rect: full(),
            color: Color {
                r: 0.0,
                g: 0.0,
                b: 1.0,
                a: 1.0,
            },
        })
    }

    fn backdrop(radius: f32, color: Color) -> Primitive {
        Primitive::Backdrop(crate::primitives::RoundedQuad {
            rect: full(),
            radius,
            color,
            border_color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
            border_width: 0.0,
        })
    }

    /// `Backdrop` substitui o destino **dentro** da forma, e só dentro: os
    /// pixels de cobertura zero nos cantos de fora do raio ficam como estavam.
    /// Com `REPLACE` puro o fragmento de cobertura zero também era escrito, e
    /// o canto virava transparente (o desktop aparecia ali em janela
    /// transparente, onde devia aparecer o que está embaixo).
    #[test]
    fn a_backdrop_leaves_the_pixels_outside_its_radius_alone() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let clear = Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        let pixels = gpu.render(&[blue_backdrop(), backdrop(4.0, clear)]);
        // O canto (0,0) está fora do raio 4: continua o azul de embaixo.
        assert_near(at(&pixels, 0, 0), BLUE);
        assert_near(at(&pixels, 7, 0), BLUE);
        assert_near(at(&pixels, 0, 7), BLUE);
        assert_near(at(&pixels, 7, 7), BLUE);
        // O miolo é substituído: o furo transparente de sempre.
        assert_near(at(&pixels, 4, 4), [0, 0, 0, 0]);
        // Uma aresta reta, longe dos cantos: a borda antialiasada da forma
        // (o pixel está a meio pixel de dentro), quase toda apagada.
        assert!(at(&pixels, 4, 0)[3] < 64, "{:?}", at(&pixels, 4, 0));
    }

    /// A faixa antialiasada do raio apaga só em parte: nem o transparente
    /// inteiro de `REPLACE` (que deixava a faixa mostrar o que está atrás da
    /// janela) nem o destino intacto. Em algum pixel da borda o alfa fica
    /// entre os dois.
    #[test]
    fn a_backdrop_rim_erases_only_in_proportion_to_its_coverage() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let clear = Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        let pixels = gpu.render(&[blue_backdrop(), backdrop(4.0, clear)]);
        assert!(
            pixels.iter().any(|p| p[3] > 8 && p[3] < 247),
            "nenhum pixel de borda com apagamento parcial: {pixels:?}"
        );
        // Premultiplicado: onde o alfa é parcial, o azul que sobra é o mesmo.
        for p in &pixels {
            assert_eq!(p[0], 0);
            assert_eq!(p[1], 0);
            assert!(p[2] <= p[3].saturating_add(2), "{p:?}");
        }
    }

    /// Com cor (alfa 0.5 de verde): o miolo vira a cor premultiplicada, como
    /// com `REPLACE`, e o canto de fora do raio continua o que era.
    #[test]
    fn a_backdrop_with_a_color_replaces_the_inside_and_keeps_the_corner() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let half_green = Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 0.5,
        };
        let pixels = gpu.render(&[blue_backdrop(), backdrop(4.0, half_green)]);
        // Miolo: verde a 0.5 premultiplicado, o azul de baixo substituído.
        assert_near(at(&pixels, 4, 4), [0, 128, 0, 128]);
        assert_near(at(&pixels, 0, 0), BLUE);
        assert_near(at(&pixels, 7, 7), BLUE);
    }

    #[test]
    fn a_backdrop_with_radius_zero_still_replaces_every_pixel() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let clear = Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        let pixels = gpu.render(&[blue_backdrop(), backdrop(0.0, clear)]);
        assert_near(at(&pixels, 0, 0), [0, 0, 0, 0]);
        assert_near(at(&pixels, 7, 7), [0, 0, 0, 0]);
    }

    #[allow(clippy::too_many_arguments)]
    fn image_prim(
        id: ImageId,
        draw: Rect,
        uv: Rect,
        repeat: bool,
        mask: Rect,
        mask_radius: f32,
        alpha: f32,
    ) -> Primitive {
        Primitive::Image {
            rect: draw,
            uv,
            repeat,
            mask,
            mask_radius,
            alpha,
            image: id,
        }
    }

    fn whole(id: ImageId, alpha: f32) -> Primitive {
        image_prim(
            id,
            full(),
            rect(0.0, 0.0, 1.0, 1.0),
            false,
            full(),
            0.0,
            alpha,
        )
    }

    fn at(pixels: &[[u8; 4]], x: usize, y: usize) -> [u8; 4] {
        pixels[y * SIZE as usize + x]
    }

    fn near(actual: [u8; 4], expected: [u8; 4], tolerance: u8) -> bool {
        actual
            .iter()
            .zip(expected)
            .all(|(a, e)| a.abs_diff(e) <= tolerance)
    }

    #[track_caller]
    fn assert_near(actual: [u8; 4], expected: [u8; 4]) {
        assert!(
            near(actual, expected, 3),
            "pixel {actual:?}, esperado ~{expected:?}"
        );
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn solid_1x1(color: [u8; 4]) -> Vec<u8> {
        color.to_vec()
    }

    /// 2x2: vermelho e verde em cima, azul e branco embaixo.
    fn quadrants() -> Vec<u8> {
        [RED, GREEN, BLUE, WHITE].concat()
    }

    #[test]
    fn opaque_texel_at_half_alpha_blends_premultiplied_over_the_backdrop() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        let red = [RED; 4].concat();
        let id = gpu.image(2, 2, &[&red, &solid_1x1(RED)]);
        let pixels = gpu.render(&[blue_backdrop(), whole(id, 0.5)]);
        // Premultiplicado: vermelho * 0.5 sobre azul * 0.5 = (128, 0, 128).
        // Com o alfa aplicado em dobro sairia (64, 0, 191).
        assert_near(at(&pixels, 4, 4), [128, 0, 128, 255]);
        assert_near(at(&pixels, 0, 0), [128, 0, 128, 255]);
        assert_near(at(&pixels, 7, 7), [128, 0, 128, 255]);
    }

    #[test]
    fn the_file_alpha_multiplies_the_image_alpha() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        // Vermelho com alfa 128 (reto no arquivo), imagem a alfa 1.
        let translucent = [[255, 0, 0, 128]; 4].concat();
        let id = gpu.image(2, 2, &[&translucent, &[255, 0, 0, 128]]);
        let pixels = gpu.render(&[blue_backdrop(), whole(id, 1.0)]);
        assert_near(at(&pixels, 4, 4), [128, 0, 127, 255]);
        // E a 0.5 a conta é 0.5 * 0.502.
        let pixels = gpu.render(&[blue_backdrop(), whole(id, 0.5)]);
        assert_near(at(&pixels, 4, 4), [64, 0, 191, 255]);
    }

    #[test]
    fn alpha_zero_draws_nothing_and_alpha_one_covers_the_backdrop() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        let id = gpu.image(1, 1, &[&solid_1x1(RED)]);
        let none = gpu.render(&[blue_backdrop(), whole(id, 0.0)]);
        assert_near(at(&none, 3, 3), BLUE);
        let all = gpu.render(&[blue_backdrop(), whole(id, 1.0)]);
        assert_near(at(&all, 3, 3), RED);
    }

    #[test]
    fn the_mask_cuts_the_image_to_its_rect() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        let id = gpu.image(1, 1, &[&solid_1x1(RED)]);
        // Imagem no alvo inteiro, máscara só no miolo 4x4.
        let primitive = image_prim(
            id,
            full(),
            rect(0.0, 0.0, 1.0, 1.0),
            false,
            rect(2.0, 2.0, 4.0, 4.0),
            0.0,
            1.0,
        );
        let pixels = gpu.render(&[blue_backdrop(), primitive]);
        assert_near(at(&pixels, 0, 0), BLUE);
        assert_near(at(&pixels, 7, 3), BLUE);
        assert_near(at(&pixels, 1, 4), BLUE);
        assert_near(at(&pixels, 2, 2), RED);
        assert_near(at(&pixels, 5, 5), RED);
        assert_near(at(&pixels, 6, 6), BLUE);
    }

    #[test]
    fn a_rounded_mask_rounds_the_corners_and_keeps_the_middle() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        let id = gpu.image(1, 1, &[&solid_1x1(RED)]);
        let primitive = image_prim(
            id,
            full(),
            rect(0.0, 0.0, 1.0, 1.0),
            false,
            full(),
            3.0,
            1.0,
        );
        let pixels = gpu.render(&[blue_backdrop(), primitive]);
        // O canto (0,0) cai fora do arco de raio 3 (raio 4 num quadro de 8 px
        // seria quase um círculo, e nem o meio da borda cobriria por inteiro);
        // o miolo e o meio da borda ficam. O pixel do canto ainda pega a faixa
        // de antialiasing da SDF (~7% de vermelho), igual a todo canto
        // arredondado do app: "quase azul", não azul exato.
        for (x, y) in [(0, 0), (7, 7), (0, 7), (7, 0)] {
            let corner = at(&pixels, x, y);
            assert!(
                corner[0] < 40 && corner[2] > 215,
                "canto ({x},{y}) deveria ficar fora da máscara: {corner:?}"
            );
        }
        assert_near(at(&pixels, 4, 4), RED);
        assert_near(at(&pixels, 4, 0), RED);
        assert_near(at(&pixels, 0, 4), RED);
    }

    #[test]
    fn the_mask_is_independent_of_the_drawn_rect() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        let id = gpu.image(1, 1, &[&solid_1x1(RED)]);
        // Imagem menor que o quadro (4x4 no meio), máscara do quadro inteiro
        // com cantos arredondados: o desenho para onde o `rect` acaba, e os
        // cantos do quadro não ganham imagem.
        let primitive = image_prim(
            id,
            rect(2.0, 2.0, 4.0, 4.0),
            rect(0.0, 0.0, 1.0, 1.0),
            false,
            full(),
            4.0,
            1.0,
        );
        let pixels = gpu.render(&[blue_backdrop(), primitive]);
        assert_near(at(&pixels, 3, 3), RED);
        assert_near(at(&pixels, 1, 1), BLUE);
        assert_near(at(&pixels, 0, 0), BLUE);
        assert_near(at(&pixels, 6, 6), BLUE);
    }

    #[test]
    fn repeat_tiles_the_texture_texel_for_texel() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        let id = gpu.image(2, 2, &[&quadrants(), &solid_1x1(WHITE)]);
        // uv 0..4 em 8 pixels: dois texels por repetição, um texel por pixel.
        let primitive = image_prim(id, full(), rect(0.0, 0.0, 4.0, 4.0), true, full(), 0.0, 1.0);
        let pixels = gpu.render(&[primitive]);
        assert_near(at(&pixels, 0, 0), RED);
        assert_near(at(&pixels, 1, 0), GREEN);
        assert_near(at(&pixels, 0, 1), BLUE);
        assert_near(at(&pixels, 1, 1), WHITE);
        assert_near(at(&pixels, 2, 0), RED);
        assert_near(at(&pixels, 3, 3), WHITE);
        assert_near(at(&pixels, 4, 5), BLUE);
        assert_near(at(&pixels, 7, 6), GREEN);
    }

    #[test]
    fn clamp_extends_the_edge_texel_where_repeat_would_wrap() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        let id = gpu.image(2, 2, &[&quadrants(), &solid_1x1(WHITE)]);
        // uv 0..2: a metade de baixo/direita passa de 1 e, sem repetir, fica
        // no texel da borda (branco, canto inferior direito da imagem).
        let clamped = image_prim(
            id,
            full(),
            rect(0.0, 0.0, 2.0, 2.0),
            false,
            full(),
            0.0,
            1.0,
        );
        let pixels = gpu.render(&[clamped]);
        assert_near(at(&pixels, 7, 7), WHITE);
        assert_near(at(&pixels, 7, 0), GREEN);
        assert_near(at(&pixels, 0, 7), BLUE);
        // Com repeat o mesmo canto volta a dar a volta (não é branco puro).
        let repeated = image_prim(id, full(), rect(0.0, 0.0, 2.0, 2.0), true, full(), 0.0, 1.0);
        let pixels = gpu.render(&[repeated]);
        assert!(
            !near(at(&pixels, 7, 7), WHITE, 3),
            "o repeat deveria dar a volta: {:?}",
            at(&pixels, 7, 7)
        );
    }

    #[test]
    fn minification_samples_the_mip_chain() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        // 4x4 com um nível de cor por nível: vermelho, verde, azul.
        let level0 = [RED; 16].concat();
        let level1 = [GREEN; 4].concat();
        let level2 = solid_1x1(BLUE);
        let id = gpu.image(4, 4, &[&level0, &level1, &level2]);
        // A imagem inteira (4x4 texels) num pixel só: 4 texels por pixel, o
        // LOD é 2 e a amostra é o nível 2.
        let one_pixel = rect(3.0, 3.0, 1.0, 1.0);
        let primitive = image_prim(
            id,
            one_pixel,
            rect(0.0, 0.0, 1.0, 1.0),
            false,
            one_pixel,
            0.0,
            1.0,
        );
        let pixels = gpu.render(&[primitive]);
        assert_near(at(&pixels, 3, 3), BLUE);
        // Em tamanho natural (um texel por pixel) é o nível 0.
        let natural = rect(0.0, 0.0, 4.0, 4.0);
        let primitive = image_prim(
            id,
            natural,
            rect(0.0, 0.0, 1.0, 1.0),
            false,
            natural,
            0.0,
            1.0,
        );
        let pixels = gpu.render(&[primitive]);
        assert_near(at(&pixels, 1, 1), RED);
    }

    #[test]
    fn a_removed_image_draws_nothing_and_does_not_panic() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        let id = gpu.image(1, 1, &[&solid_1x1(RED)]);
        assert!(gpu.registry.remove(id).is_some());
        let pixels = gpu.render(&[blue_backdrop(), whole(id, 1.0)]);
        assert_near(at(&pixels, 3, 3), BLUE);
    }

    #[test]
    fn the_image_sits_between_the_quads_around_it_in_stream_order() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        let id = gpu.image(1, 1, &[&solid_1x1(RED)]);
        let green_corner = Primitive::Quad(Quad {
            rect: rect(0.0, 0.0, 4.0, 4.0),
            color: Color {
                r: 0.0,
                g: 1.0,
                b: 0.0,
                a: 1.0,
            },
        });
        // Fundo azul, imagem vermelha por cima, quad verde por cima da imagem:
        // o verde tem de aparecer sobre o vermelho, o vermelho sobre o azul.
        let pixels = gpu.render(&[blue_backdrop(), whole(id, 1.0), green_corner]);
        assert_near(at(&pixels, 1, 1), GREEN);
        assert_near(at(&pixels, 6, 6), RED);
    }

    #[test]
    fn a_clip_cuts_the_image_like_it_cuts_quads() {
        let Some(mut gpu) = Harness::new() else {
            return;
        };
        let id = gpu.image(1, 1, &[&solid_1x1(RED)]);
        let pixels = gpu.render(&[
            blue_backdrop(),
            Primitive::PushClip(rect(0.0, 0.0, 4.0, 8.0)),
            whole(id, 1.0),
            Primitive::PopClip,
        ]);
        assert_near(at(&pixels, 1, 4), RED);
        assert_near(at(&pixels, 6, 4), BLUE);
    }
}
