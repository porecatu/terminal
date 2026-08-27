// SPDX-License-Identifier: GPL-3.0-or-later

//! Pipeline de texto: `glyphon`, atlas de glyphs em cache entre frames
//! (docs/arquitetura.md seção 5). Dividido pelo ADR-0018: `TextAtlas`,
//! `Cache` e `SwashCache` são recursos compartilhados de `GpuContext`, um
//! por processo; este módulo é o estado por janela -- um `TextRenderer`
//! por camada, reusado entre frames, que **empresta** o `FontSystem` de
//! [`crate::TextMeasurer`] em vez de possuir o seu próprio -- um só
//! `FontSystem` carregando as cinco faces embutidas (ADR-0016), nunca
//! dois.

use glyphon::{
    Buffer, FontSystem, Metrics, Shaping, SwashCache, TextArea, TextAtlas, TextBounds,
    TextRenderer, Viewport,
};

use crate::frame::{Layer, ResolvedText};
use crate::primitives::scale_rect;
use crate::text_measurer::attrs_for;

fn color_to_cosmic(color: wgpu::Color) -> glyphon::Color {
    glyphon::Color::rgba(
        (color.r * 255.0).round() as u8,
        (color.g * 255.0).round() as u8,
        (color.b * 255.0).round() as u8,
        (color.a * 255.0).round() as u8,
    )
}

/// Recorte da camada, já em pixels físicos, na forma que o `glyphon`
/// espera. Ausência de clip usa `TextBounds::default()` -- área
/// (praticamente) irrestrita, sem custo de conversão.
fn to_text_bounds(clip: Option<crate::primitives::Rect>, scale: f32) -> TextBounds {
    let Some(rect) = clip else {
        return TextBounds::default();
    };
    let physical = scale_rect(rect, scale);
    TextBounds {
        left: physical.x as i32,
        top: physical.y as i32,
        right: (physical.x + physical.width) as i32,
        bottom: (physical.y + physical.height) as i32,
    }
}

/// Um `TextRenderer` e os `Buffer`s shapados do frame corrente de uma
/// camada -- mantidos vivos até `render` usar as `TextArea`s que apontam
/// para eles.
struct TextLayerState {
    renderer: TextRenderer,
    buffers: Vec<Buffer>,
}

/// Estado por janela: um [`TextLayerState`] por camada.
pub(crate) struct TextWindowState {
    layers: [TextLayerState; 5],
}

impl TextWindowState {
    pub(crate) fn new(device: &wgpu::Device, atlas: &mut TextAtlas) -> Self {
        let layers = std::array::from_fn(|_| TextLayerState {
            renderer: TextRenderer::new(atlas, device, wgpu::MultisampleState::default(), None),
            buffers: Vec::new(),
        });
        Self { layers }
    }

    /// Shapa e prepara o texto de `layer` para o frame corrente. `scale`
    /// converte de pixels lógicos para físicos -- shapado direto no
    /// tamanho físico, nunca escalado depois de rasterizado (ADR-0018:
    /// glyph escalado depois de rasterizado sai borrado em HiDPI).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_layer(
        &mut self,
        layer: Layer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        font_system: &mut FontSystem,
        atlas: &mut TextAtlas,
        swash_cache: &mut SwashCache,
        viewport: &Viewport,
        resolved: &[ResolvedText],
        scale: f32,
    ) {
        let state = &mut self.layers[layer.index()];
        state.buffers.clear();
        state.buffers.reserve(resolved.len());

        for item in resolved {
            let size_px = item.run.size_px * scale;
            let metrics = Metrics::new(size_px, size_px * 1.2);
            let mut buffer = Buffer::new(font_system, metrics);
            // Sem limite de largura: cada `TextRun` é um trecho de mesmo
            // estilo numa linha só, não algo que deva quebrar.
            buffer.set_size(None, None);
            let attrs = attrs_for(item.run.font);
            buffer.set_text(&item.run.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(font_system, false);
            state.buffers.push(buffer);
        }

        let areas = resolved
            .iter()
            .zip(state.buffers.iter())
            .map(|(item, buffer)| TextArea {
                buffer,
                left: item.run.origin.0 * scale,
                top: item.run.origin.1 * scale,
                scale: 1.0,
                bounds: to_text_bounds(item.clip, scale),
                default_color: color_to_cosmic(item.run.color),
                custom_glyphs: &[],
            });

        let _ = state.renderer.prepare(
            device,
            queue,
            font_system,
            atlas,
            viewport,
            areas,
            swash_cache,
        );
    }

    pub(crate) fn render_layer<'pass>(
        &'pass self,
        layer: Layer,
        atlas: &'pass TextAtlas,
        viewport: &'pass Viewport,
        pass: &mut wgpu::RenderPass<'pass>,
    ) {
        let _ = self.layers[layer.index()]
            .renderer
            .render(atlas, viewport, pass);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Rect;

    #[test]
    fn no_clip_uses_default_bounds() {
        assert_eq!(to_text_bounds(None, 1.0), TextBounds::default());
    }

    #[test]
    fn clip_scales_and_converts_to_bounds() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let bounds = to_text_bounds(Some(rect), 2.0);
        assert_eq!(
            bounds,
            TextBounds {
                left: 20,
                top: 40,
                right: 80,
                bottom: 120,
            }
        );
    }
}
