// SPDX-License-Identifier: GPL-3.0-or-later

//! Medição de texto sem GPU (ADR-0018). `TextMeasurer` é dono do
//! `FontSystem` e do `fontdb` com as cinco faces embutidas (ADR-0016),
//! construível sem `wgpu::Device` nem `wgpu::Queue` -- é o que torna o
//! layout da barra de abas a função pura que a seção 7 da arquitetura
//! promete: um teste constrói o seu próprio `TextMeasurer` sem abrir
//! janela.
//!
//! Em runtime, `porecatu-ui` guarda **um** `TextMeasurer` por processo
//! (dentro de [`crate::GpuContext`]) e empresta o `FontSystem` dele ao
//! pipeline de texto no `prepare` -- um `FontSystem` só, nunca dois
//! carregando as mesmas cinco faces em paralelo.

use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, fontdb};

use crate::primitives::{FontFace, SansWeight};

const MONO_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexMono-Regular.ttf");
const MONO_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexMono-Medium.ttf");
const SANS_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexSans-Regular.ttf");
const SANS_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexSans-Medium.ttf");
const SANS_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexSans-SemiBold.ttf");

const MONO_FAMILY: &str = "IBM Plex Mono";
const SANS_FAMILY: &str = "IBM Plex Sans";

/// Reticências usadas pelo truncamento (RF-1.10).
const ELLIPSIS: char = '…';

pub(crate) fn attrs_for(font: FontFace) -> Attrs<'static> {
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

pub struct TextMeasurer {
    font_system: FontSystem,
}

impl TextMeasurer {
    /// Registra as cinco faces embutidas num `fontdb` que **nunca** chama
    /// `load_system_fonts` -- é o que garante a precedência do ADR-0016
    /// sem lógica de desempate: não existe cópia do sistema para competir.
    pub fn new() -> Self {
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
        Self { font_system }
    }

    pub(crate) fn font_system_mut(&mut self) -> &mut FontSystem {
        &mut self.font_system
    }

    /// Largura de avanço de `text` numa face e tamanho, numa linha só, sem
    /// quebra -- é o avanço que se quer medir, não um layout.
    pub fn measure_width(&mut self, text: &str, font: FontFace, size_px: f32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let metrics = Metrics::new(size_px, size_px * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(None, None);
        let attrs = attrs_for(font);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        buffer
            .layout_runs()
            .next()
            .map(|run| run.glyphs.iter().map(|glyph| glyph.w).sum())
            .unwrap_or(0.0)
    }

    /// Largura de avanço de `M` na mono e altura de linha pedida -- decide
    /// a largura/altura de célula da grade (a grade é derivada da métrica
    /// de fonte, não o contrário).
    pub fn measure_mono_cell(&mut self, size_px: f32, line_height_px: f32) -> (f32, f32) {
        let width = self.measure_width("M", FontFace::Mono { bold: false }, size_px);
        (width, line_height_px)
    }

    /// RF-1.10: trunca `text` para caber em `max_width`, cortando por
    /// caractere e acrescentando reticências. Devolve o texto (truncado ou
    /// não) e se houve corte -- o booleano é o que decide se a aba mostra
    /// tooltip.
    pub fn truncate(
        &mut self,
        text: &str,
        font: FontFace,
        size_px: f32,
        max_width: f32,
    ) -> (String, bool) {
        if self.measure_width(text, font, size_px) <= max_width {
            return (text.to_string(), false);
        }

        let ellipsis_width = self.measure_width(&ELLIPSIS.to_string(), font, size_px);
        let budget = (max_width - ellipsis_width).max(0.0);

        let mut result = String::new();
        for ch in text.chars() {
            let mut candidate = result.clone();
            candidate.push(ch);
            if self.measure_width(&candidate, font, size_px) > budget {
                break;
            }
            result = candidate;
        }
        result.push(ELLIPSIS);
        (result, true)
    }
}

impl Default for TextMeasurer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: f32 = 12.5;

    #[test]
    fn empty_string_measures_zero() {
        let mut m = TextMeasurer::new();
        assert_eq!(
            m.measure_width("", FontFace::Mono { bold: false }, SIZE),
            0.0
        );
    }

    #[test]
    fn longer_text_measures_wider() {
        let mut m = TextMeasurer::new();
        let short = m.measure_width(
            "a",
            FontFace::Sans {
                weight: SansWeight::Regular,
            },
            SIZE,
        );
        let long = m.measure_width(
            "a longer title",
            FontFace::Sans {
                weight: SansWeight::Regular,
            },
            SIZE,
        );
        assert!(long > short);
    }

    #[test]
    fn mono_cell_has_positive_size() {
        let mut m = TextMeasurer::new();
        let (width, height) = m.measure_mono_cell(SIZE, SIZE * 1.75);
        assert!(width > 0.0);
        assert_eq!(height, SIZE * 1.75);
    }

    #[test]
    fn truncate_leaves_short_text_untouched() {
        let mut m = TextMeasurer::new();
        let font = FontFace::Sans {
            weight: SansWeight::Regular,
        };
        let (text, truncated) = m.truncate("vim", font, SIZE, 1000.0);
        assert_eq!(text, "vim");
        assert!(!truncated);
    }

    #[test]
    fn truncate_cuts_long_text_with_ellipsis() {
        let mut m = TextMeasurer::new();
        let font = FontFace::Sans {
            weight: SansWeight::Regular,
        };
        let full_width = m.measure_width("vim: a very long file name.rs", font, SIZE);
        let (text, truncated) = m.truncate(
            "vim: a very long file name.rs",
            font,
            SIZE,
            full_width / 2.0,
        );
        assert!(truncated);
        assert!(text.ends_with('…'));
        assert!(text.len() < "vim: a very long file name.rs".len());
        assert!(m.measure_width(&text, font, SIZE) <= full_width / 2.0);
    }

    #[test]
    fn truncate_tiny_budget_still_returns_ellipsis() {
        let mut m = TextMeasurer::new();
        let font = FontFace::Sans {
            weight: SansWeight::Regular,
        };
        let (text, truncated) = m.truncate("qualquer coisa", font, SIZE, 0.0);
        assert!(truncated);
        assert_eq!(text, "…");
    }
}
