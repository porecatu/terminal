// SPDX-License-Identifier: GPL-3.0-or-later

//! `GpuContext`: os recursos `wgpu`/`glyphon` que não variam por janela --
//! `Instance`, `Adapter`, `Device`, `Queue`, atlas de glyphs, pipeline de
//! quads, o `TextMeasurer` do processo (ADR-0018). Um por processo; cada
//! janela ganha sua [`crate::WindowSurface`], que reusa tudo isto.

use std::sync::Arc;

use wgpu::rwh::{HasDisplayHandle, HasWindowHandle};

use crate::quad::QuadShared;
use crate::text_measurer::{FontFamilies, TextMeasurer};
use crate::window_surface::WindowSurface;

/// Falha ao criar a surface de uma segunda janela contra o `Adapter` já
/// escolhido pela primeira -- cenário de máquina com duas GPUs. Nunca
/// `panic` (ADR-0018): quem chama decide o que fazer, tipicamente avisar
/// pela superfície do ADR-0014.
#[derive(Debug)]
pub enum SurfaceError {
    /// A plataforma não sabe criar uma surface para esta janela.
    Unsupported,
    /// O `Adapter` da primeira janela não sabe apresentar nesta surface.
    Incompatible,
}

impl std::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SurfaceError::Unsupported => write!(f, "surface não suportada nesta plataforma"),
            SurfaceError::Incompatible => {
                write!(f, "surface incompatível com o adapter da primeira janela")
            }
        }
    }
}

impl std::error::Error for SurfaceError {}

pub struct GpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    pub(crate) quad_shared: QuadShared,
    pub(crate) text_cache: glyphon::Cache,
    pub(crate) text_atlas: glyphon::TextAtlas,
    pub(crate) swash_cache: glyphon::SwashCache,
    pub(crate) text_measurer: TextMeasurer,
}

impl GpuContext {
    /// Cria o contexto do processo e a `WindowSurface` da primeira janela
    /// juntos -- o `Adapter` é escolhido compatível com a surface dela
    /// (ADR-0018) e reusado por qualquer janela seguinte.
    pub fn new<W>(
        window: Arc<W>,
        width: u32,
        height: u32,
        fonts: FontFamilies,
    ) -> (Self, WindowSurface)
    where
        W: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static,
    {
        pollster::block_on(Self::new_async(window, width, height, fonts))
    }

    async fn new_async<W>(
        window: Arc<W>,
        width: u32,
        height: u32,
        fonts: FontFamilies,
    ) -> (Self, WindowSurface)
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
        let format = config.format;
        surface.configure(&device, &config);

        let quad_shared = QuadShared::new(&device, format);
        let text_cache = glyphon::Cache::new(&device);
        let mut text_atlas = glyphon::TextAtlas::new(&device, &queue, &text_cache, format);
        let swash_cache = glyphon::SwashCache::new();
        let text_measurer = TextMeasurer::with_families(fonts);

        let window_surface = WindowSurface::new(
            &device,
            &queue,
            &text_cache,
            &mut text_atlas,
            &quad_shared,
            surface,
            config,
        );

        let gpu = Self {
            instance,
            adapter,
            device,
            queue,
            format,
            quad_shared,
            text_cache,
            text_atlas,
            swash_cache,
            text_measurer,
        };
        (gpu, window_surface)
    }

    /// Cria a `WindowSurface` de uma janela adicional, reusando `Adapter`,
    /// `Device` e atlas -- é o que evita duplicar VRAM por janela
    /// (ADR-0015, ADR-0018). Falha de compatibilidade devolve `Err`, nunca
    /// `panic`.
    pub fn create_window_surface<W>(
        &mut self,
        window: Arc<W>,
        width: u32,
        height: u32,
    ) -> Result<WindowSurface, SurfaceError>
    where
        W: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static,
    {
        let surface = self
            .instance
            .create_surface(window)
            .map_err(|_| SurfaceError::Unsupported)?;
        let mut config = surface
            .get_default_config(&self.adapter, width.max(1), height.max(1))
            .ok_or(SurfaceError::Incompatible)?;
        config.format = config.format.remove_srgb_suffix();
        if config.format != self.format {
            return Err(SurfaceError::Incompatible);
        }
        surface.configure(&self.device, &config);

        Ok(WindowSurface::new(
            &self.device,
            &self.queue,
            &self.text_cache,
            &mut self.text_atlas,
            &self.quad_shared,
            surface,
            config,
        ))
    }

    /// O medidor de texto do processo (ADR-0018) -- `porecatu-ui` o usa
    /// para layout puro (cell metrics, e a barra de abas a partir da
    /// Etapa 3); o `prepare` do pipeline empresta o mesmo `FontSystem`
    /// dele, para nunca existirem dois carregando as cinco faces.
    pub fn text_measurer(&mut self) -> &mut TextMeasurer {
        &mut self.text_measurer
    }
}
