// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use wgpu::rwh::{HasDisplayHandle, HasWindowHandle};

mod primitives;
mod quad;
mod text;

pub use primitives::{Color, FontFace, Primitive, Quad, Rect, RoundedQuad, SansWeight, TextRun};

use quad::QuadPipeline;
use text::TextPipeline;

/// Superfície `wgpu` de uma janela, com os pipelines de geometria e de
/// texto (docs/arquitetura.md seção 5): duas passadas por frame, quads
/// instanciados e texto via `glyphon` com atlas em cache entre frames.
/// Não conhece domínio -- recebe [`Primitive`]s prontas, nada sobre aba ou
/// grupo; quem traduz é `porecatu-ui`.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    quad_pipeline: QuadPipeline,
    text_pipeline: TextPipeline,
}

impl Renderer {
    pub fn new<W>(window: Arc<W>, width: u32, height: u32) -> Self
    where
        W: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static,
    {
        pollster::block_on(Self::new_async(window, width, height))
    }

    async fn new_async<W>(window: Arc<W>, width: u32, height: u32) -> Self
    where
        W: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static,
    {
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .expect("criação da surface wgpu falhou");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("nenhum adapter wgpu compatível encontrado");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("falha ao criar device wgpu");

        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .expect("surface sem configuração padrão suportada pelo adapter");
        // Formato *Srgb faz a GPU reaplicar a curva sRGB em cima de valores
        // que já escrevemos em espaço sRGB (nossas cores vêm direto de hex
        // do design) -- dupla conversão que lava as cores escuras. Unorm
        // grava os bytes como pedimos, sem reinterpretar.
        config.format = config.format.remove_srgb_suffix();
        surface.configure(&device, &config);

        let mut quad_pipeline = QuadPipeline::new(&device, config.format);
        quad_pipeline.resize(&queue, config.width, config.height);

        let glyphon_cache = glyphon::Cache::new(&device);
        let mut text_pipeline = TextPipeline::new(&device, &queue, &glyphon_cache, config.format);
        text_pipeline.resize(&queue, config.width, config.height);

        Self {
            surface,
            device,
            queue,
            config,
            quad_pipeline,
            text_pipeline,
        }
    }

    /// Reconfigura a surface para o novo tamanho físico (px), inclusive por mudança de DPI.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.quad_pipeline.resize(&self.queue, width, height);
        self.text_pipeline.resize(&self.queue, width, height);
    }

    /// Largura de avanço de `M` na mono e altura de linha, no tamanho
    /// pedido -- usado para derivar a largura/altura de célula da grade
    /// (Etapa 4: a métrica de fonte decide a grade, não o contrário).
    pub fn measure_mono_cell(&mut self, size_px: f32, line_height_px: f32) -> (f32, f32) {
        self.text_pipeline
            .measure_mono_cell(size_px, line_height_px)
    }

    /// Limpa a tela com `clear_color` e desenha `primitives` por cima.
    ///
    /// `PushClip`/`PopClip` ainda não recortam nada -- sem consumidor no
    /// v1 até a F2 trazer overflow de barra de abas. Quads (com e sem
    /// cantos) são desenhados antes do texto, em vez de respeitar a ordem
    /// de `primitives`: para a grade do terminal (única consumidora nesta
    /// fase) isso já é correto -- texto de uma célula nunca precisa ficar
    /// atrás do fundo de outra -- e evita alternar pipeline em UM render
    /// pass, que o `glyphon` não foi desenhado para fazer (seu `prepare`
    /// já assume "todo o texto do frame" de uma vez).
    pub fn render(&mut self, clear_color: Color, primitives: &[Primitive]) {
        let mut quads = Vec::new();
        let mut rounded = Vec::new();
        let mut text_runs = Vec::new();
        for primitive in primitives {
            match primitive {
                Primitive::Quad(quad) => quads.push(*quad),
                Primitive::RoundedQuad(quad) => rounded.push(*quad),
                Primitive::Text(run) => text_runs.push(run.clone()),
                Primitive::PushClip(_) | Primitive::PopClip => {}
            }
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => return,
        };

        self.quad_pipeline
            .prepare(&self.device, &self.queue, &quads, &rounded);
        self.text_pipeline
            .prepare(&self.device, &self.queue, &text_runs);

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("porecatu-render/frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.quad_pipeline.render(&mut pass);
            self.text_pipeline.render(&mut pass);
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
    }
}
