// SPDX-License-Identifier: GPL-3.0-or-later

//! Pipeline de texto: `glyphon`, atlas de glyphs em cache entre frames
//! (docs/arquitetura.md seção 5). Fontes do design embutidas no binário
//! (ADR-0016) -- nenhuma delas depende do sistema, e vencem qualquer cópia
//! instalada porque o `fontdb` deste crate nunca chama
//! `load_system_fonts`: só existem as cinco faces abaixo.

use glyphon::{
    Attrs, Buffer, Cache, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, Weight, fontdb,
};

use crate::primitives::{FontFace, SansWeight, TextRun};

const MONO_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexMono-Regular.ttf");
const MONO_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexMono-Medium.ttf");
const SANS_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexSans-Regular.ttf");
const SANS_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexSans-Medium.ttf");
const SANS_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexSans-SemiBold.ttf");

const MONO_FAMILY: &str = "IBM Plex Mono";
const SANS_FAMILY: &str = "IBM Plex Sans";

fn attrs_for(font: FontFace) -> Attrs<'static> {
    match font {
        FontFace::Mono { bold } => Attrs::new()
            .family(Family::Name(MONO_FAMILY))
            .weight(if bold { Weight::MEDIUM } else { Weight::NORMAL }),
        FontFace::Sans { weight } => {
            let weight = match weight {
                SansWeight::Regular => Weight::NORMAL,
                SansWeight::Medium => Weight::MEDIUM,
                SansWeight::SemiBold => Weight::SEMIBOLD,
            };
            Attrs::new()
                .family(Family::Name(SANS_FAMILY))
                .weight(weight)
        }
    }
}

fn color_to_cosmic(color: wgpu::Color) -> glyphon::Color {
    glyphon::Color::rgba(
        (color.r * 255.0).round() as u8,
        (color.g * 255.0).round() as u8,
        (color.b * 255.0).round() as u8,
        (color.a * 255.0).round() as u8,
    )
}

pub struct TextPipeline {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    renderer: TextRenderer,
    viewport: Viewport,
    // Um `Buffer` por `TextRun` do frame, reconstruído a cada `prepare` --
    // mantido vivo até `render` usar as `TextArea`s que apontam para eles.
    // Reuso de buffer entre frames (só re-shape quando o texto muda) fica
    // para uma otimização futura; por ora, corretude primeiro.
    buffers: Vec<Buffer>,
}

impl TextPipeline {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cache: &Cache,
        format: wgpu::TextureFormat,
    ) -> Self {
        let mut db = fontdb::Database::new();
        for bytes in [
            MONO_REGULAR,
            MONO_MEDIUM,
            SANS_REGULAR,
            SANS_MEDIUM,
            SANS_SEMIBOLD,
        ] {
            db.load_font_data(bytes.to_vec());
        }
        let font_system = FontSystem::new_with_locale_and_db("en-US".to_string(), db);

        let swash_cache = SwashCache::new();
        let mut atlas = TextAtlas::new(device, queue, cache, format);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let viewport = Viewport::new(device, cache);

        Self {
            font_system,
            swash_cache,
            atlas,
            renderer,
            viewport,
            buffers: Vec::new(),
        }
    }

    pub fn resize(&mut self, queue: &wgpu::Queue, width: u32, height: u32) {
        self.viewport.update(queue, Resolution { width, height });
    }

    /// Largura de avanço de `M` na mono e altura de linha pedida -- é o que
    /// define a largura/altura de célula da grade (cuidado do roadmap da
    /// Etapa 4: a grade é derivada da métrica de fonte, não o contrário).
    pub fn measure_mono_cell(&mut self, size_px: f32, line_height_px: f32) -> (f32, f32) {
        let metrics = Metrics::new(size_px, line_height_px.max(1.0));
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(Some(size_px * 4.0), Some(line_height_px * 2.0));
        let attrs = attrs_for(FontFace::Mono { bold: false });
        buffer.set_text("M", &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let width = buffer
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first())
            .map(|glyph| glyph.w)
            .unwrap_or(size_px * 0.6);

        (width, line_height_px)
    }

    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, runs: &[TextRun]) {
        self.buffers.clear();
        self.buffers.reserve(runs.len());

        for run in runs {
            let line_height = run.size_px * 1.2;
            let metrics = Metrics::new(run.size_px, line_height);
            let mut buffer = Buffer::new(&mut self.font_system, metrics);
            buffer.set_size(
                Some(run.text.len() as f32 * run.size_px * 2.0 + 1.0),
                Some(line_height),
            );
            let attrs = attrs_for(run.font);
            buffer.set_text(&run.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut self.font_system, false);
            self.buffers.push(buffer);
        }

        let areas = runs
            .iter()
            .zip(self.buffers.iter())
            .map(|(run, buffer)| TextArea {
                buffer,
                left: run.origin.0,
                top: run.origin.1,
                scale: 1.0,
                bounds: TextBounds::default(),
                default_color: color_to_cosmic(run.color),
                custom_glyphs: &[],
            });

        let _ = self.renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            areas,
            &mut self.swash_cache,
        );
    }

    pub fn render<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        let _ = self.renderer.render(&self.atlas, &self.viewport, pass);
    }
}
