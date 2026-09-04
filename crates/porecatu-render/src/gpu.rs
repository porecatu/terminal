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
    /// RF-11.26/ADR-0001: `true` quando o adapter escolhido não tem
    /// aceleração de hardware -- ou porque a primeira tentativa não achou
    /// nenhum adapter compatível e uma segunda, com
    /// `force_fallback_adapter`, achou; ou porque a primeira já devolveu
    /// direto um adapter `DeviceType::Cpu` (algumas VMs expõem só isso, sem
    /// a primeira tentativa falhar). Consultado uma vez, no primeiro
    /// `GpuContext::new` do processo.
    software_rendering: bool,
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
        // RF-11.26/ADR-0001 (tabela de riscos: "driver GPU ruim / VM sem
        // aceleração ... wgpu cai para backend software; detectar e avisar
        // no primeiro start"): a primeira tentativa pede hardware; se ela
        // não achar nenhum adapter compatível, uma segunda pede
        // explicitamente o adapter de fallback (`force_fallback_adapter`,
        // ex. WARP no Windows, llvmpipe/SwiftShader em Vulkan/GL) antes de
        // desistir -- só essa segunda falhando é motivo de `panic`,
        // equivalente a "sem GPU e sem software nenhum", cenário sem
        // recuperação possível.
        let (adapter, used_fallback_request) = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
        {
            Ok(adapter) => (adapter, false),
            Err(_) => {
                let adapter = instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        compatible_surface: Some(&surface),
                        force_fallback_adapter: true,
                        ..Default::default()
                    })
                    .await
                    .expect("nenhum adapter wgpu, nem por hardware nem por fallback de software");
                (adapter, true)
            }
        };
        // Algumas VMs já devolvem um adapter `Cpu` na primeira tentativa,
        // sem ela precisar falhar -- os dois caminhos contam como "sem
        // aceleração" pro aviso do RF-11.26.
        let software_rendering =
            used_fallback_request || adapter.get_info().device_type == wgpu::DeviceType::Cpu;
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
            software_rendering,
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

    /// RF-11.26: `true` quando o processo abriu sem aceleração de GPU
    /// (ver o campo). Vale para a vida inteira do processo -- o adapter
    /// nunca troca depois de escolhido.
    pub fn software_rendering(&self) -> bool {
        self.software_rendering
    }
}
