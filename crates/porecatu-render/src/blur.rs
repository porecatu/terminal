// SPDX-License-Identifier: GPL-3.0-or-later

//! Passe de desfoque (ADR-0068 §1 a §5): a fonte que a `porecatu-ui` dá à
//! `WindowSurface` -- cor de fundo, lista de primitivas, raio lógico e uma
//! chave --, a gaussiana separável sobre uma cópia **reduzida** dela, e o cache
//! que só refaz o resultado quando a entrada muda.
//!
//! O render não sabe de onde a fonte veio: "imagem da janela" e "acrílico" são
//! da `ui` (ADR-0068 §2). O resultado é lido por `Primitive::BlurredBackdrop`,
//! que amostra a textura pela coordenada de tela (`image.wgsl`, `fs_screen`).
//!
//! Tudo `Unorm` -- o formato da surface, que já perdeu o sufixo `Srgb`
//! (`gpu.rs`) -- e premultiplicado de ponta a ponta, a mesma disciplina do
//! ADR-0061 §3: nenhuma conversão sRGB em lugar nenhum do caminho.

use wgpu::util::DeviceExt;

use crate::frame::{Layer, resolve_layer};
use crate::image::{GpuImage, ImageRegistry, ImageShared};
use crate::primitives::{Color, Primitive};
use crate::quad::{QuadShared, QuadWindowState};

const SHADER: &str = include_str!("blur.wgsl");

/// Teto de amostras por passe (ADR-0068 §3 e a tabela de riscos: "cópia
/// reduzida com teto de 32 amostras por passe").
pub(crate) const MAX_SAMPLES: u32 = 32;
/// Amostras de cada lado do centro que cabem no teto: `2 * 15 + 1 = 31`.
pub(crate) const MAX_TAPS: u32 = (MAX_SAMPLES - 1) / 2;
/// Quanto a cópia é reduzida, do menor para o maior (ADR-0068 §3).
const FACTORS: [u32; 2] = [2, 4];
/// Piso do sigma, em texels da cópia: raio minúsculo vira quase uma passagem
/// direta, em vez de uma divisão por zero.
const MIN_SIGMA: f32 = 0.1;

/// A fonte do desfoque: o que **está atrás** do que será desfocado, em
/// pixels lógicos como o resto das primitivas. `porecatu-ui` passa a imagem da
/// janela já posta pelo `placement` e a cor da barra; o render só desenha a
/// lista sobre a cor, desfoca e guarda.
#[derive(Debug, Clone)]
pub struct BlurSource {
    /// Cor sobre a qual `primitives` desenham. Cor reta, como `clear_color`.
    pub background: Color,
    /// Desenhadas, na ordem, sobre `background`. Só geometria e imagens
    /// contam: texto, e um `BlurredBackdrop` dentro da própria fonte, não
    /// desenham.
    pub primitives: Vec<Primitive>,
    /// Raio do desfoque em pixels lógicos; o físico é `radius_logical * scale`
    /// (RF-19.9).
    pub radius_logical: f32,
    /// Identidade do conteúdo. Mesma chave, mesmo tamanho e mesma escala: o
    /// resultado guardado vale e nada é refeito (ADR-0068 §4). Quem muda a
    /// fonte muda a chave -- imagem pronta ou trocada, `mode`, `opacity`,
    /// `blur`, cor de fundo.
    pub key: u64,
}

/// Como um raio vira trabalho: o fator de redução da cópia e a forma do passe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BlurPlan {
    /// A cópia tem `ceil(largura / factor)` por `ceil(altura / factor)`.
    pub factor: u32,
    /// Desvio padrão, em texels **da cópia**: `raio_físico / factor / 3`.
    pub sigma: f32,
    /// Amostras de cada lado do centro (no máximo [`MAX_TAPS`]).
    pub taps: u32,
    /// Distância em texels entre uma amostra e a seguinte: 1 até o raio caber
    /// no teto, depois o que for preciso para o alcance de `3 * sigma` ainda
    /// ser coberto.
    pub stride: u32,
}

impl BlurPlan {
    /// Amostras de um passe: o centro mais `taps` de cada lado.
    pub(crate) const fn samples(self) -> u32 {
        2 * self.taps + 1
    }
}

/// O plano de `radius_logical` em uma janela de escala `scale` (ADR-0068 §3):
/// o **menor** fator, 2 ou 4, que mantém o passe em até [`MAX_SAMPLES`]
/// amostras; se nem 4 basta (raio físico acima de 60 texels), fica em 4 e as
/// amostras se espaçam (`stride`) -- com sigma de 10 texels ou mais, uma
/// amostra a cada dois ou três continua sendo uma gaussiana.
///
/// O alcance do passe é `3 * sigma = raio_físico / factor` texels, que é o que
/// "sigma = raio / 3" quer dizer: o raio é onde a gaussiana acaba.
pub(crate) fn plan(radius_logical: f32, scale: f32) -> BlurPlan {
    let physical = radius_logical * scale;
    let physical = if physical.is_finite() && physical > 0.0 {
        physical
    } else {
        0.0
    };
    let reach = |factor: u32| (physical / factor as f32).ceil().max(1.0) as u32;
    let factor = FACTORS
        .into_iter()
        .find(|&factor| reach(factor) <= MAX_TAPS)
        .unwrap_or(FACTORS[FACTORS.len() - 1]);
    let half = reach(factor);
    let taps = half.min(MAX_TAPS);
    BlurPlan {
        factor,
        sigma: (physical / factor as f32 / 3.0).max(MIN_SIGMA),
        taps,
        stride: half.div_ceil(taps),
    }
}

/// Pesos do passe, `[0]` o centro, normalizados: `w0 + 2 * soma(w1..=w_taps) =
/// 1`, então uma cor lisa passa sem mudar. Em 16 floats, o que cabe em quatro
/// `vec4` do uniform (`blur.wgsl`).
pub(crate) fn weights(plan: BlurPlan) -> [f32; 16] {
    let two_sigma_squared = 2.0 * plan.sigma * plan.sigma;
    let mut out = [0.0_f32; 16];
    let mut sum = 0.0;
    for (i, weight) in out.iter_mut().enumerate().take(plan.taps as usize + 1) {
        let distance = (i as u32 * plan.stride) as f32;
        *weight = (-(distance * distance) / two_sigma_squared).exp();
        sum += if i == 0 { *weight } else { 2.0 * *weight };
    }
    for w in &mut out {
        *w /= sum;
    }
    out
}

/// Tudo de que o resultado guardado depende (ADR-0068 §4): se qualquer campo
/// muda, ele é refeito; se nenhum muda, nada é. Saída do terminal, cursor,
/// hover e abas não entram -- não são da fonte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlurStamp {
    key: u64,
    width: u32,
    height: u32,
    scale_bits: u32,
    radius_bits: u32,
}

impl BlurStamp {
    pub(crate) fn new(source: &BlurSource, width: u32, height: u32, scale: f32) -> Self {
        Self {
            key: source.key,
            width,
            height,
            scale_bits: scale.to_bits(),
            radius_bits: source.radius_logical.to_bits(),
        }
    }
}

/// O resultado guardado e a etiqueta do que o produziu. Genérico no que guarda
/// para que a decisão "refaz ou não" seja testável sem GPU; a `WindowSurface` o
/// usa com [`BlurState`].
pub(crate) struct BlurCache<T> {
    stamp: Option<BlurStamp>,
    value: Option<T>,
}

impl<T> BlurCache<T> {
    pub(crate) fn new() -> Self {
        Self {
            stamp: None,
            value: None,
        }
    }

    /// Garante que o guardado corresponde a `wanted`: igual, não chama
    /// `build`; diferente, ou vazio, chama e guarda. Devolve se refez.
    pub(crate) fn ensure(&mut self, wanted: BlurStamp, build: impl FnOnce() -> T) -> bool {
        if self.value.is_some() && self.stamp == Some(wanted) {
            return false;
        }
        self.value = Some(build());
        self.stamp = Some(wanted);
        true
    }

    pub(crate) fn get(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// Descarta o resultado (e as texturas dele): a fonte foi retirada.
    pub(crate) fn clear(&mut self) {
        self.stamp = None;
        self.value = None;
    }
}

/// Uniform de um passe, o espelho de `Params` em `blur.wgsl`: 80 bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurParams {
    step: [f32; 2],
    taps: u32,
    stride: f32,
    weights: [[f32; 4]; 4],
}

impl BlurParams {
    fn new(plan: BlurPlan, step: [f32; 2]) -> Self {
        let w = weights(plan);
        Self {
            step,
            taps: plan.taps,
            stride: plan.stride as f32,
            weights: [
                [w[0], w[1], w[2], w[3]],
                [w[4], w[5], w[6], w[7]],
                [w[8], w[9], w[10], w[11]],
                [w[12], w[13], w[14], w[15]],
            ],
        }
    }
}

/// Pipeline do passe de gaussiana e o sampler, do processo (ADR-0018).
pub(crate) struct BlurShared {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl BlurShared {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("porecatu-render/blur-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("porecatu-render/blur-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("porecatu-render/blur-pipeline-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("porecatu-render/blur-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // O passe escreve o alvo inteiro com a média já
                    // premultiplicada: nada a misturar com o que havia.
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("porecatu-render/blur-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            bind_group_layout,
            sampler,
        }
    }
}

/// O que o desfoque precisa do `GpuContext` para ser refeito.
pub(crate) struct BlurDeps<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub blur_shared: &'a BlurShared,
    pub quad_shared: &'a QuadShared,
    pub image_shared: &'a ImageShared,
    pub images: &'a ImageRegistry<GpuImage>,
}

/// O resultado guardado: a cópia reduzida e desfocada, e o bind group com que
/// `Primitive::BlurredBackdrop` a amostra (textura + sampler linear de
/// `ClampToEdge`, o layout das imagens).
pub(crate) struct BlurState {
    _textures: [wgpu::Texture; 2],
    sample_group: wgpu::BindGroup,
}

impl BlurState {
    pub(crate) fn bind_group(&self) -> &wgpu::BindGroup {
        &self.sample_group
    }

    /// Desenha a fonte na cópia reduzida, aplica os dois passes e guarda o
    /// resultado. Grava no `encoder` do frame que já ia ser desenhado: não
    /// submete nem agenda nada (ADR-0068 §4).
    ///
    /// A fonte é desenhada **direto** na cópia reduzida -- as primitivas com
    /// `scale / factor` --, sem uma textura cheia no meio: o resultado é o
    /// mesmo para um conteúdo que vai ser desfocado de qualquer jeito, e a
    /// janela não paga uma textura do tamanho dela só para reduzir.
    pub(crate) fn build(
        deps: &BlurDeps<'_>,
        encoder: &mut wgpu::CommandEncoder,
        source: &BlurSource,
        width: u32,
        height: u32,
        scale: f32,
        format: wgpu::TextureFormat,
    ) -> Self {
        debug_assert!(!format.is_srgb(), "o desfoque é Unorm, nunca Srgb");
        let plan = plan(source.radius_logical, scale);
        debug_assert!(plan.samples() <= MAX_SAMPLES);
        let reduced_width = width.div_ceil(plan.factor).max(1);
        let reduced_height = height.div_ceil(plan.factor).max(1);

        let texture = |label: &'static str| {
            deps.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: reduced_width,
                    height: reduced_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let texture_a = texture("porecatu-render/blur-a");
        let texture_b = texture("porecatu-render/blur-b");
        let view_a = texture_a.create_view(&wgpu::TextureViewDescriptor::default());
        let view_b = texture_b.create_view(&wgpu::TextureViewDescriptor::default());

        // 1. A fonte, em `a`, na escala reduzida.
        {
            let mut state = QuadWindowState::new(
                deps.device,
                deps.queue,
                deps.quad_shared,
                reduced_width,
                reduced_height,
            );
            let resolved = resolve_layer(&source.primitives);
            state.prepare_layer(
                Layer::Grid,
                deps.device,
                deps.queue,
                &resolved.batches,
                deps.images,
                false,
                scale / plan.factor as f32,
                reduced_width,
                reduced_height,
            );
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("porecatu-render/blur-source"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view_a,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(premultiplied(source.background)),
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
                deps.quad_shared,
                deps.image_shared,
                deps.images,
                None,
                &mut pass,
            );
        }

        // 2. Horizontal, `a` -> `b`; vertical, `b` -> `a`. O resultado fica em `a`.
        let pass_group = |params: BlurParams, view: &wgpu::TextureView| {
            let buffer = deps
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("porecatu-render/blur-params"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            deps.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("porecatu-render/blur-bind-group"),
                layout: &deps.blur_shared.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&deps.blur_shared.sampler),
                    },
                ],
            })
        };
        let horizontal = pass_group(
            BlurParams::new(plan, [1.0 / reduced_width as f32, 0.0]),
            &view_a,
        );
        let vertical = pass_group(
            BlurParams::new(plan, [0.0, 1.0 / reduced_height as f32]),
            &view_b,
        );
        for (label, target, group) in [
            ("porecatu-render/blur-horizontal", &view_b, &horizontal),
            ("porecatu-render/blur-vertical", &view_a, &vertical),
        ] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&deps.blur_shared.pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }

        // 3. Como o `BlurredBackdrop` lê o resultado.
        let sample_group = deps.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("porecatu-render/blur-sample-bind-group"),
            layout: deps.image_shared.texture_layout(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view_a),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(deps.image_shared.clamp_sampler()),
                },
            ],
        });
        Self {
            _textures: [texture_a, texture_b],
            sample_group,
        }
    }
}

/// Cor reta para o valor de `Clear` premultiplicado, que é o que a textura
/// guarda: com alfa 1 é a própria cor.
fn premultiplied(color: Color) -> Color {
    Color {
        r: color.r * color.a,
        g: color.g * color.a,
        b: color.b * color.a,
        a: color.a,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALES: [f32; 4] = [1.0, 1.25, 1.5, 2.0];

    #[test]
    fn the_params_match_the_wgsl_uniform() {
        // vec2 (8) + u32 (4) + f32 (4) + array<vec4, 4> (64).
        assert_eq!(std::mem::size_of::<BlurParams>(), 80);
    }

    #[test]
    fn the_blur_shader_parses_and_validates() {
        let module =
            wgpu::naga::front::wgsl::parse_str(SHADER).expect("o WGSL do desfoque parseia");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("o WGSL do desfoque valida");
    }

    #[test]
    fn the_shader_samples_with_one_sampler() {
        assert_eq!(SHADER.matches(": sampler;").count(), 1);
    }

    #[test]
    fn the_default_radius_at_100_percent_halves_the_copy() {
        // 24 px físicos: o alcance de 12 texels na cópia pela metade cabe.
        let plan = plan(24.0, 1.0);
        assert_eq!(plan.factor, 2);
        assert_eq!(plan.taps, 12);
        assert_eq!(plan.stride, 1);
        assert_eq!(plan.samples(), 25);
        assert!((plan.sigma - 4.0).abs() < 1e-6, "{}", plan.sigma);
    }

    #[test]
    fn the_four_window_scales_at_the_default_radius() {
        // (escala, fator, amostras): o raio físico é `24 * escala`.
        for (scale, factor, samples) in [(1.0, 2, 25), (1.25, 2, 31), (1.5, 4, 19), (2.0, 4, 25)] {
            let plan = plan(24.0, scale);
            assert_eq!(plan.factor, factor, "escala {scale}");
            assert_eq!(plan.samples(), samples, "escala {scale}");
            assert_eq!(plan.stride, 1, "escala {scale}");
            assert!(plan.samples() <= MAX_SAMPLES);
        }
    }

    #[test]
    fn the_physical_radius_follows_the_scale() {
        // sigma * fator * 3 = raio físico = `blur * escala`, em qualquer escala.
        for scale in SCALES {
            for blur in [1.0_f32, 8.0, 24.0, 40.0, 64.0] {
                let plan = plan(blur, scale);
                let physical = plan.sigma * plan.factor as f32 * 3.0;
                let expected = (blur * scale).max(MIN_SIGMA * 2.0 * 3.0);
                assert!(
                    (physical - expected).abs() < 1e-3,
                    "blur {blur} escala {scale}: {physical} contra {expected}"
                );
            }
        }
    }

    #[test]
    fn the_largest_radius_at_the_largest_scale_stays_at_the_sample_ceiling() {
        // 64 * 2.0 = 128 px físicos: nem o fator 4 (32 texels) cabe em 15 de
        // cada lado, então as amostras se espaçam.
        let plan = plan(64.0, 2.0);
        assert_eq!(plan.factor, 4);
        assert_eq!(plan.taps, MAX_TAPS);
        assert_eq!(plan.stride, 3);
        assert_eq!(plan.samples(), 31);
        assert!(plan.samples() <= MAX_SAMPLES);
        // O alcance de 3 sigma (32 texels) continua coberto: 15 * 3 = 45.
        assert!(plan.taps * plan.stride >= 32);
    }

    #[test]
    fn every_radius_at_every_scale_respects_the_ceiling_and_the_reach() {
        for scale in SCALES {
            for blur in 1..=64 {
                let plan = plan(blur as f32, scale);
                assert!(
                    plan.samples() <= MAX_SAMPLES,
                    "blur {blur} escala {scale}: {} amostras",
                    plan.samples()
                );
                assert!(FACTORS.contains(&plan.factor));
                assert!(plan.taps >= 1 && plan.stride >= 1);
                // O passe alcança os 3 sigma da gaussiana.
                let reach = (plan.taps * plan.stride) as f32;
                assert!(
                    reach + 1e-3 >= plan.sigma * 3.0,
                    "blur {blur} escala {scale}: alcance {reach}, 3 sigma {}",
                    plan.sigma * 3.0
                );
            }
        }
    }

    #[test]
    fn the_smallest_factor_that_fits_wins() {
        // 30 px físicos: o fator 2 dá 15 de cada lado, cabe; 31 já não cabe.
        assert_eq!(plan(30.0, 1.0).factor, 2);
        assert_eq!(plan(31.0, 1.0).factor, 4);
        assert_eq!(plan(1.0, 1.0).factor, 2);
    }

    #[test]
    fn a_nonsense_radius_still_has_a_plan() {
        for radius in [0.0, -5.0, f32::NAN, f32::INFINITY] {
            let plan = plan(radius, 1.0);
            assert_eq!(plan.factor, 2);
            assert!(plan.samples() <= MAX_SAMPLES);
            assert!(plan.sigma > 0.0 && plan.sigma.is_finite());
        }
    }

    #[test]
    fn the_weights_sum_to_one_and_fall_off() {
        for scale in SCALES {
            for blur in [1.0_f32, 24.0, 64.0] {
                let plan = plan(blur, scale);
                let w = weights(plan);
                let sum: f32 = w[0] + 2.0 * w[1..=plan.taps as usize].iter().sum::<f32>();
                assert!(
                    (sum - 1.0).abs() < 1e-5,
                    "blur {blur} escala {scale}: {sum}"
                );
                for pair in w[..=plan.taps as usize].windows(2) {
                    assert!(pair[0] >= pair[1], "{w:?}");
                }
                // Nada além das `taps` amostras.
                assert!(w[plan.taps as usize + 1..].iter().all(|&x| x == 0.0));
            }
        }
    }

    fn source(key: u64, radius: f32) -> BlurSource {
        BlurSource {
            background: Color::BLACK,
            primitives: Vec::new(),
            radius_logical: radius,
            key,
        }
    }

    #[test]
    fn the_same_key_size_and_scale_does_not_rebuild() {
        let mut cache = BlurCache::new();
        let mut builds = 0;
        let stamp = BlurStamp::new(&source(1, 24.0), 800, 600, 1.0);
        assert!(cache.ensure(stamp, || {
            builds += 1;
            builds
        }));
        for _ in 0..5 {
            assert!(!cache.ensure(stamp, || {
                builds += 1;
                builds
            }));
        }
        assert_eq!(builds, 1);
        assert_eq!(cache.get(), Some(&1));
    }

    #[test]
    fn each_input_that_changes_forces_one_rebuild() {
        let base = source(1, 24.0);
        let stamps = [
            BlurStamp::new(&base, 800, 600, 1.0),
            BlurStamp::new(&source(2, 24.0), 800, 600, 1.0), // a chave
            BlurStamp::new(&source(2, 24.0), 801, 600, 1.0), // a largura
            BlurStamp::new(&source(2, 24.0), 801, 601, 1.0), // a altura
            BlurStamp::new(&source(2, 24.0), 801, 601, 1.25), // a escala
            BlurStamp::new(&source(2, 8.0), 801, 601, 1.25), // o raio
        ];
        let mut cache = BlurCache::new();
        let mut builds = 0;
        for stamp in stamps {
            assert!(cache.ensure(stamp, || {
                builds += 1;
            }));
            assert!(!cache.ensure(stamp, || {
                builds += 1;
            }));
        }
        assert_eq!(builds, stamps.len());
    }

    #[test]
    fn clearing_drops_the_result_and_the_next_ensure_rebuilds() {
        let mut cache = BlurCache::new();
        let stamp = BlurStamp::new(&source(1, 24.0), 800, 600, 1.0);
        assert!(cache.ensure(stamp, || "pronto"));
        cache.clear();
        assert_eq!(cache.get(), None);
        assert!(cache.ensure(stamp, || "de novo"));
        assert_eq!(cache.get(), Some(&"de novo"));
    }

    #[test]
    fn premultiplying_the_clear_color_leaves_an_opaque_one_alone() {
        let opaque = Color {
            r: 0.2,
            g: 0.4,
            b: 0.6,
            a: 1.0,
        };
        assert_eq!(premultiplied(opaque), opaque);
        let half = premultiplied(Color {
            r: 0.8,
            g: 0.4,
            b: 0.2,
            a: 0.5,
        });
        assert_eq!((half.r, half.g, half.b, half.a), (0.4, 0.2, 0.1, 0.5));
    }
}

/// Testes com GPU de verdade, sem janela (o mesmo molde de `image.rs`): um alvo
/// `Rgba8Unorm`, o mesmo `BlurState`, o mesmo `QuadWindowState` e os mesmos
/// pipelines do app, e os pixels lidos de volta. Sem adapter nenhum (nem o de
/// software) cada teste se **pula** e diz isso.
#[cfg(test)]
mod gpu_tests {
    use super::*;
    use crate::image::IMAGE_FORMAT;
    use crate::primitives::{Quad, Rect};

    struct Harness {
        device: wgpu::Device,
        queue: wgpu::Queue,
        quad_shared: QuadShared,
        image_shared: ImageShared,
        blur_shared: BlurShared,
        registry: ImageRegistry<GpuImage>,
    }

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
            let blur_shared = BlurShared::new(&device, IMAGE_FORMAT);
            Some(Self {
                device,
                queue,
                quad_shared,
                image_shared,
                blur_shared,
                registry: ImageRegistry::new(),
            })
        }

        /// Desenha `primitives` (na camada `Grid`) num alvo `width x height`
        /// limpo a transparente, com a fonte de desfoque dada, e devolve os
        /// pixels. `scale` é o da janela.
        fn render(
            &self,
            width: u32,
            height: u32,
            scale: f32,
            source: Option<&BlurSource>,
            primitives: &[Primitive],
        ) -> Pixels {
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
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let deps = BlurDeps {
                device: &self.device,
                queue: &self.queue,
                blur_shared: &self.blur_shared,
                quad_shared: &self.quad_shared,
                image_shared: &self.image_shared,
                images: &self.registry,
            };
            let blur = source.map(|s| {
                BlurState::build(&deps, &mut encoder, s, width, height, scale, IMAGE_FORMAT)
            });
            let mut state =
                QuadWindowState::new(&self.device, &self.queue, &self.quad_shared, width, height);
            let resolved = resolve_layer(primitives);
            state.prepare_layer(
                Layer::Grid,
                &self.device,
                &self.queue,
                &resolved.batches,
                &self.registry,
                source.is_some(),
                scale,
                width,
                height,
            );
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("teste/passe"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
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
                    blur.as_ref().map(BlurState::bind_group),
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
            let data = readback
                .slice(..)
                .get_mapped_range()
                .expect("intervalo mapeado");
            let mut pixels = Vec::new();
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let at = y * row as usize + x * 4;
                    pixels.push([data[at], data[at + 1], data[at + 2], data[at + 3]]);
                }
            }
            Pixels {
                width: width as usize,
                data: pixels,
            }
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

    fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color {
            r: r as f64,
            g: g as f64,
            b: b as f64,
            a: 1.0,
        }
    }

    fn quad(r: Rect, color: Color) -> Primitive {
        Primitive::Quad(Quad { rect: r, color })
    }

    fn blurred(mask: Rect, radius: f32) -> Primitive {
        Primitive::BlurredBackdrop {
            mask,
            mask_radius: radius,
        }
    }

    fn solid_source(background: Color, radius: f32) -> BlurSource {
        BlurSource {
            background,
            primitives: Vec::new(),
            radius_logical: radius,
            key: 1,
        }
    }

    #[track_caller]
    fn assert_near(actual: [u8; 4], expected: [u8; 4], tolerance: u8) {
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(a, e)| a.abs_diff(e) <= tolerance),
            "pixel {actual:?}, esperado ~{expected:?} (±{tolerance})"
        );
    }

    /// A cor lisa atravessa o passe sem mudar, em cada canto e no miolo: pega a
    /// dupla conversão sRGB (uma cor média sairia escura ou lavada), erro de
    /// normalização dos pesos e a borda que escurece rumo ao vazio (o *clamp*).
    #[test]
    fn a_solid_color_survives_the_pass_unchanged() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let color = rgb(0.2, 0.4, 0.6);
        for (width, height, scale, radius) in [
            (64, 48, 1.0, 24.0),
            (45, 33, 1.5, 24.0),
            (64, 48, 2.0, 64.0),
            (64, 48, 1.0, 1.0),
        ] {
            let source = solid_source(color, radius);
            let full = rect(0.0, 0.0, width as f32 / scale, height as f32 / scale);
            let pixels = gpu.render(width, height, scale, Some(&source), &[blurred(full, 0.0)]);
            for (x, y) in [
                (0, 0),
                (width as usize - 1, 0),
                (0, height as usize - 1),
                (width as usize - 1, height as usize - 1),
                (width as usize / 2, height as usize / 2),
                (3, height as usize / 2),
            ] {
                assert_near(pixels.at(x, y), [51, 102, 153, 255], 1);
            }
        }
    }

    /// Cor de fundo translúcida: o resultado é premultiplicado como a textura o
    /// guarda, e a composição sobre o alvo transparente o devolve igual.
    #[test]
    fn a_translucent_background_stays_premultiplied() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let source = solid_source(
            Color {
                r: 0.8,
                g: 0.4,
                b: 0.2,
                a: 0.5,
            },
            12.0,
        );
        let pixels = gpu.render(
            32,
            32,
            1.0,
            Some(&source),
            &[blurred(rect(0.0, 0.0, 32.0, 32.0), 0.0)],
        );
        assert_near(pixels.at(16, 16), [102, 51, 26, 128], 2);
        assert_near(pixels.at(0, 0), [102, 51, 26, 128], 2);
    }

    /// Uma cor lisa de uma primitiva da fonte, não só do fundo, passa igual.
    #[test]
    fn a_source_primitive_covering_everything_is_the_blurred_color() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let source = BlurSource {
            background: Color::BLACK,
            primitives: vec![quad(rect(0.0, 0.0, 64.0, 48.0), rgb(0.9, 0.1, 0.3))],
            radius_logical: 20.0,
            key: 7,
        };
        let pixels = gpu.render(
            64,
            48,
            1.0,
            Some(&source),
            &[blurred(rect(0.0, 0.0, 64.0, 48.0), 0.0)],
        );
        assert_near(pixels.at(0, 0), [230, 26, 77, 255], 2);
        assert_near(pixels.at(32, 24), [230, 26, 77, 255], 2);
        assert_near(pixels.at(63, 47), [230, 26, 77, 255], 2);
    }

    /// Branco à esquerda e preto à direita: o passe de fato desfoca, só
    /// perto da emenda, e as pontas ficam como estavam.
    #[test]
    fn the_pass_blurs_an_edge_and_leaves_the_far_sides_alone() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let source = BlurSource {
            background: Color::BLACK,
            primitives: vec![quad(rect(0.0, 0.0, 32.0, 16.0), rgb(1.0, 1.0, 1.0))],
            radius_logical: 8.0,
            key: 3,
        };
        let pixels = gpu.render(
            64,
            16,
            1.0,
            Some(&source),
            &[blurred(rect(0.0, 0.0, 64.0, 16.0), 0.0)],
        );
        assert_near(pixels.at(0, 8), [255, 255, 255, 255], 3);
        assert_near(pixels.at(63, 8), [0, 0, 0, 255], 3);
        // Na emenda, o cinza do meio; três pixels para cada lado, entre o
        // extremo e o meio.
        let middle = pixels.at(31, 8)[0];
        assert!((90..=165).contains(&middle), "emenda: {middle}");
        let left = pixels.at(29, 8)[0];
        let right = pixels.at(34, 8)[0];
        assert!((185..=250).contains(&left), "esquerda: {left}");
        assert!((5..=75).contains(&right), "direita: {right}");
    }

    /// Faixas de 1px com um degradê horizontal de vermelho: a fonte onde dois
    /// retângulos vizinhos precisam mostrar pedaços contínuos.
    fn gradient_source(width: u32, height: u32) -> BlurSource {
        let primitives = (0..width)
            .map(|x| {
                quad(
                    rect(x as f32, 0.0, 1.0, height as f32),
                    rgb(x as f32 / (width - 1) as f32, 0.2, 0.8),
                )
            })
            .collect();
        BlurSource {
            background: Color::BLACK,
            primitives,
            radius_logical: 6.0,
            key: 11,
        }
    }

    /// Dois `BlurredBackdrop` em retângulos vizinhos mostram o que um retângulo
    /// único mostraria: a amostra é pela coordenada de tela, então a emenda
    /// não existe (RF-19.4).
    #[test]
    fn two_adjacent_masks_equal_one_mask_across_the_seam() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let source = gradient_source(64, 16);
        let one = gpu.render(
            64,
            16,
            1.0,
            Some(&source),
            &[blurred(rect(0.0, 0.0, 64.0, 16.0), 0.0)],
        );
        let two = gpu.render(
            64,
            16,
            1.0,
            Some(&source),
            &[
                blurred(rect(0.0, 0.0, 32.0, 16.0), 0.0),
                blurred(rect(32.0, 0.0, 32.0, 16.0), 0.0),
            ],
        );
        for y in 0..16 {
            for x in 0..64 {
                assert_near(two.at(x, y), one.at(x, y), 1);
            }
        }
        // E o degradê atravessa a emenda sem degrau: a diferença entre os dois
        // pixels do vão é a de qualquer par vizinho.
        for y in [0, 8, 15] {
            let step = two.at(32, y)[0].abs_diff(two.at(31, y)[0]);
            assert!(step <= 8, "degrau na emenda, linha {y}: {step}");
        }
    }

    /// Dois quadros com um vão: o que está no vão é o que o resto da cena
    /// desenha, e as duas metades continuam a mesma imagem.
    #[test]
    fn a_gap_between_two_masks_is_not_painted_and_the_halves_match_the_single_mask() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let source = gradient_source(64, 16);
        let one = gpu.render(
            64,
            16,
            1.0,
            Some(&source),
            &[blurred(rect(0.0, 0.0, 64.0, 16.0), 0.0)],
        );
        let gap = gpu.render(
            64,
            16,
            1.0,
            Some(&source),
            &[
                blurred(rect(0.0, 0.0, 30.0, 16.0), 0.0),
                blurred(rect(34.0, 0.0, 30.0, 16.0), 0.0),
            ],
        );
        for y in [0, 8, 15] {
            for x in [0, 10, 29, 34, 50, 63] {
                assert_near(gap.at(x, y), one.at(x, y), 1);
            }
            for x in 30..34 {
                assert_eq!(gap.at(x, y), [0, 0, 0, 0], "vão ({x},{y})");
            }
        }
    }

    /// O raio é físico: com a mesma emenda e o mesmo raio **lógico**, a faixa
    /// de transição em escala 2.0 é cerca do dobro da de 1.0 (RF-19.9).
    #[test]
    fn the_transition_width_follows_the_window_scale() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let source = BlurSource {
            background: Color::BLACK,
            primitives: vec![quad(rect(0.0, 0.0, 32.0, 16.0), rgb(1.0, 1.0, 1.0))],
            radius_logical: 8.0,
            key: 5,
        };
        let width_of = |scale: f32| {
            let (w, h) = ((64.0 * scale) as u32, (16.0 * scale) as u32);
            let pixels = gpu.render(
                w,
                h,
                scale,
                Some(&source),
                &[blurred(rect(0.0, 0.0, 64.0, 16.0), 0.0)],
            );
            let row = h as usize / 2;
            (0..w as usize)
                .filter(|&x| (26..=230).contains(&pixels.at(x, row)[0]))
                .count() as f32
        };
        let one = width_of(1.0);
        let two = width_of(2.0);
        assert!(one >= 4.0, "transição de 1.0 estreita demais: {one}");
        let ratio = two / one;
        assert!(
            (1.6..=2.5).contains(&ratio),
            "{one} px contra {two} px: {ratio}"
        );
    }

    /// Sem fonte de desfoque a primitiva não desenha nada, nem pânico: o que já
    /// estava no alvo fica.
    #[test]
    fn without_a_source_a_blurred_backdrop_draws_nothing() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let blue = quad(rect(0.0, 0.0, 16.0, 16.0), rgb(0.0, 0.0, 1.0));
        let pixels = gpu.render(
            16,
            16,
            1.0,
            None,
            &[blue, blurred(rect(0.0, 0.0, 16.0, 16.0), 0.0)],
        );
        assert_near(pixels.at(8, 8), [0, 0, 255, 255], 1);
        assert_near(pixels.at(0, 0), [0, 0, 255, 255], 1);
    }

    /// A ordem da lista vale, como a das outras primitivas: o quad depois do
    /// desfoque o cobre; o quad antes é coberto por ele (a fonte é opaca).
    #[test]
    fn the_list_order_decides_what_covers_what() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let source = solid_source(rgb(0.0, 1.0, 0.0), 10.0);
        let mask = rect(0.0, 0.0, 16.0, 16.0);
        let red = quad(rect(4.0, 4.0, 8.0, 8.0), rgb(1.0, 0.0, 0.0));

        let after = gpu.render(
            16,
            16,
            1.0,
            Some(&source),
            &[blurred(mask, 0.0), red.clone()],
        );
        assert_near(after.at(8, 8), [255, 0, 0, 255], 1);
        assert_near(after.at(1, 1), [0, 255, 0, 255], 1);

        let before = gpu.render(16, 16, 1.0, Some(&source), &[red, blurred(mask, 0.0)]);
        assert_near(before.at(8, 8), [0, 255, 0, 255], 1);
    }

    /// A máscara usa a SDF compartilhada: canto arredondado de fora do raio
    /// fica sem o desfoque, o miolo e o meio da borda ficam com ele.
    #[test]
    fn a_rounded_mask_cuts_the_corners_and_keeps_the_middle() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let source = solid_source(rgb(0.0, 1.0, 0.0), 10.0);
        let pixels = gpu.render(
            16,
            16,
            1.0,
            Some(&source),
            &[blurred(rect(0.0, 0.0, 16.0, 16.0), 6.0)],
        );
        for (x, y) in [(0, 0), (15, 0), (0, 15), (15, 15)] {
            assert!(
                pixels.at(x, y)[3] < 64,
                "canto ({x},{y}): {:?}",
                pixels.at(x, y)
            );
        }
        assert_near(pixels.at(8, 8), [0, 255, 0, 255], 1);
        assert_near(pixels.at(8, 0), [0, 255, 0, 255], 1);
        assert_near(pixels.at(0, 8), [0, 255, 0, 255], 1);
    }

    /// O recorte de uma máscara menor que o alvo não vaza: fora dela o pixel
    /// continua como estava.
    #[test]
    fn the_mask_cuts_the_blurred_texture_to_its_rect() {
        let Some(gpu) = Harness::new() else {
            return;
        };
        let source = solid_source(rgb(0.0, 1.0, 0.0), 10.0);
        let pixels = gpu.render(
            16,
            16,
            1.0,
            Some(&source),
            &[blurred(rect(4.0, 4.0, 8.0, 8.0), 0.0)],
        );
        assert_eq!(pixels.at(2, 2), [0, 0, 0, 0]);
        assert_eq!(pixels.at(13, 8), [0, 0, 0, 0]);
        assert_near(pixels.at(4, 4), [0, 255, 0, 255], 1);
        assert_near(pixels.at(11, 11), [0, 255, 0, 255], 1);
        assert_eq!(pixels.at(12, 12), [0, 0, 0, 0]);
    }
}
