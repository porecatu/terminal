// SPDX-License-Identifier: GPL-3.0-or-later

//! `WindowSurface`: o que varia por janela (ADR-0018) -- surface `wgpu`,
//! `Viewport`, o `TextRenderer` de cada camada, e a escala de DPI. É a
//! **única** fronteira que converte de pixels lógicos (contrato de
//! [`crate::Primitive`]) para físicos: todo o resto do crate, e tudo o
//! que `porecatu-ui` monta, fica em lógico.

use crate::blit::Offscreen;
use crate::frame::{Frame, Layer, resolve_layer};
use crate::gpu::GpuContext;
use crate::primitives::Color;
use crate::quad::{QuadShared, QuadWindowState};
use crate::text::TextWindowState;

pub struct WindowSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    viewport: glyphon::Viewport,
    quad_state: QuadWindowState,
    text_state: TextWindowState,
    /// Pixels físicos por pixel lógico -- `window.scale_factor()` do
    /// `winit`. Aplicado uma vez aqui, nunca mais adiante.
    scale: f32,
    /// Textura onde o frame é desenhado quando a janela tem opacidade < 1
    /// (`blit.rs`); criada sob demanda e refeita no resize.
    offscreen: Option<Offscreen>,
}

impl WindowSurface {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cache: &glyphon::Cache,
        atlas: &mut glyphon::TextAtlas,
        quad_shared: &QuadShared,
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    ) -> Self {
        let mut viewport = glyphon::Viewport::new(device, cache);
        viewport.update(
            queue,
            glyphon::Resolution {
                width: config.width,
                height: config.height,
            },
        );
        let quad_state =
            QuadWindowState::new(device, queue, quad_shared, config.width, config.height);
        let text_state = TextWindowState::new(device, atlas);

        Self {
            surface,
            config,
            viewport,
            quad_state,
            text_state,
            scale: 1.0,
            offscreen: None,
        }
    }

    /// `true` se a surface compõe com o que está atrás da janela
    /// (`PreMultiplied`). Só então opacidade < 1 tem efeito; numa surface
    /// opaca o canal alfa é ignorado pelo compositor.
    pub fn is_transparent(&self) -> bool {
        self.config.alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied
    }

    /// Reconfigura a surface para o novo tamanho físico (px) e escala --
    /// resize de verdade e mudança de DPI passam pelo mesmo caminho.
    pub fn resize(&mut self, gpu: &GpuContext, width: u32, height: u32, scale: f32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.scale = scale;
        self.offscreen = None;
        self.surface.configure(&gpu.device, &self.config);
        self.quad_state.resize(&gpu.queue, width, height);
        self.viewport
            .update(&gpu.queue, glyphon::Resolution { width, height });
    }

    /// Limpa a tela com `clear_color` e desenha as cinco camadas de
    /// `frame`, na ordem de [`Layer::ORDER`] -- cada camada inteira cobre
    /// a anterior inteira, e dentro dela quads/arredondados desenham antes
    /// do texto (ADR-0018).
    ///
    /// `opacity` (0.0 a 1.0) é a opacidade da janela inteira; só vale em
    /// surface transparente ([`Self::is_transparent`]) e com `1.0` o frame
    /// vai direto à surface, como sempre foi.
    pub fn render(
        &mut self,
        gpu: &mut GpuContext,
        clear_color: Color,
        frame: &Frame,
        opacity: f32,
    ) {
        let opacity = if self.is_transparent() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let composited = opacity < 1.0;
        // Cada chamada usa caminhos de campo diretos (`gpu.device`,
        // `gpu.text_atlas`, ...) em vez de desestruturar `gpu` numa
        // variável só -- é o que deixa o borrow checker ver que os campos
        // emprestados (device/queue por valor, atlas/swash/measurer por
        // `&mut`) são disjuntos.
        for layer in Layer::ORDER {
            let resolved = resolve_layer(frame.primitives(layer));
            self.quad_state.prepare_layer(
                layer,
                &gpu.device,
                &gpu.queue,
                &resolved.batches,
                self.scale,
                self.config.width,
                self.config.height,
            );
            let (font_system, families) = gpu.text_measurer.font_system_and_families_mut();
            self.text_state.prepare_layer(
                layer,
                &gpu.device,
                &gpu.queue,
                font_system,
                families,
                &mut gpu.text_atlas,
                &mut gpu.swash_cache,
                &self.viewport,
                &resolved.text,
                self.scale,
            );
        }

        let frame_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&gpu.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => return,
        };

        let view = frame_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        if composited
            && !self
                .offscreen
                .as_ref()
                .is_some_and(|o| o.matches(self.config.width, self.config.height))
        {
            self.offscreen = Some(Offscreen::new(
                &gpu.device,
                &gpu.blit_shared,
                self.config.format,
                self.config.width,
                self.config.height,
            ));
        }
        let scene_view = match (&self.offscreen, composited) {
            (Some(offscreen), true) => offscreen.view(),
            _ => &view,
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("porecatu-render/frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: scene_view,
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
            for layer in Layer::ORDER {
                self.quad_state
                    .render_layer(layer, &gpu.quad_shared, &mut pass);
                // `QuadWindowState::render_layer` deixa o `scissor_rect` do
                // passe no último batch que desenhou -- se for um clip
                // estreito (a barra de severidade do aviso, 2px, é o caso
                // que expôs isto), ele fica em vigor pro texto que vem
                // logo depois. `glyphon::TextRenderer::render` nunca chama
                // `set_scissor_rect` (clip por `TextBounds` é feito no
                // shader, a partir do que `to_text_bounds` calculou por
                // `TextArea`) -- ele confia no scissor que já estiver
                // setado no passe, então sem isto o texto de uma camada
                // cujo último quad é clipado sai cortado pro mesmo
                // retângulo estreito, efetivamente invisível.
                pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
                self.text_state
                    .render_layer(layer, &gpu.text_atlas, &self.viewport, &mut pass);
            }
        }
        if composited && let Some(offscreen) = &self.offscreen {
            offscreen.set_opacity(&gpu.queue, opacity);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("porecatu-render/blit"),
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
            offscreen.draw(&gpu.blit_shared, &mut pass);
        }
        gpu.queue.submit(Some(encoder.finish()));
        gpu.queue.present(frame_texture);
    }
}
