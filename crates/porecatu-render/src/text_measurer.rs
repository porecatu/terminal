// SPDX-License-Identifier: GPL-3.0-or-later

//! Medição de texto sem GPU (ADR-0018). `TextMeasurer` é dono do
//! `FontSystem` e do `fontdb` com as faces embutidas (ADR-0025),
//! construível sem `wgpu::Device` nem `wgpu::Queue` -- é o que torna o
//! layout da barra de abas a função pura que a seção 7 da arquitetura
//! promete: um teste constrói o seu próprio `TextMeasurer` sem abrir
//! janela.
//!
//! Em runtime, `porecatu-ui` guarda **um** `TextMeasurer` por processo
//! (dentro de [`crate::GpuContext`]) e empresta o `FontSystem` dele ao
//! pipeline de texto no `prepare` -- um `FontSystem` só, nunca dois
//! carregando as mesmas faces em paralelo.

use std::collections::HashMap;

use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, fontdb};

use crate::primitives::{FontFace, SansWeight};

// Face do design (ADR-0026, supersede ADR-0025): **Iosevka Fixed** para
// terminal e chrome -- uma família só. O ADR-0025 tinha introduzido a
// Iosevka Aile para o chrome; ela e a Fixed são variantes desenhadas
// diferente dentro da mesma superfamília (Aile é a proporcional/humanista,
// Fixed é a monoespaçada sem ligadura), e lado a lado na barra a diferença
// de desenho lia como duas fontes, não como uma identidade só -- pedido do
// usuário para unificar. Recortada por `scripts/subset-fonts.py`; o
// recorte é permitido porque a OFL da Iosevka **não** tem cláusula de
// Reserved Font Name.
const MONO_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/IosevkaFixed-Regular.ttf");
const MONO_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/IosevkaFixed-Medium.ttf");
/// Sexta face: os ícones do chrome (ver [`crate::icon`]). Licença ISC,
/// compatível com a GPLv3.
const ICONS: &[u8] = include_bytes!("../../../assets/fonts/Lucide.ttf");

const MONO_FAMILY: &str = "Iosevka Fixed";
/// Chrome usa a mesma família do terminal (ADR-0026): `Sans` aqui é o nome
/// do papel (título de aba, rótulo de grupo, menu), não de uma família
/// proporcional separada -- a face por trás é `MONO_FAMILY`.
const SANS_FAMILY: &str = MONO_FAMILY;
/// Nome interno (`name` ID 1) do `Lucide.ttf` -- minúsculo, como o arquivo
/// declara; `Family::Name` é o que casa a face no `fontdb`.
const ICON_FAMILY: &str = "lucide";

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
                // A Iosevka Fixed só embute 400/500 (ADR-0026); sem um
                // arquivo 600 próprio, o `fontdb` casa pelo peso
                // registrado mais próximo (Medium). Ninguém pede
                // `SemiBold` hoje -- se um widget vier a precisar de um
                // 600 de verdade, `scripts/subset-fonts.py` precisa de
                // `IosevkaFixed-SemiBold.ttf` na lista de faces.
                SansWeight::SemiBold => Weight::SEMIBOLD,
            };
            Attrs::new()
                .family(Family::Name(SANS_FAMILY))
                .weight(weight)
        }
        FontFace::Icon => Attrs::new()
            .family(Family::Name(ICON_FAMILY))
            .weight(Weight::NORMAL),
    }
}

/// Tamanho em que [`TextMeasurer::advance_em`] mede, antes de normalizar
/// pela em. Grande de propósito: o avanço vem do shaping em ponto
/// flutuante, e medir num tamanho de tela deixaria o arredondamento
/// grande perto da tolerância de quem compara com a largura da célula.
const ADVANCE_PROBE_SIZE: f32 = 64.0;

pub struct TextMeasurer {
    font_system: FontSystem,
    /// Avanço por caractere, em múltiplos da em. Existe porque a grade
    /// consulta isto **por célula desenhada**, e shapar um caractere por
    /// célula por frame é exatamente a armadilha de performance que o
    /// CLAUDE.md registra. O conjunto de caracteres distintos numa sessão
    /// é pequeno e estável, então o cache satura rápido.
    advance_cache: HashMap<(char, FontFace), f32>,
}

impl TextMeasurer {
    /// Registra as faces embutidas e **depois** as do sistema.
    ///
    /// A ordem é a decisão inteira. O ADR-0016 quer as duas coisas ao
    /// mesmo tempo: as faces do design vencem para as famílias que ele
    /// declara, *e* a cadeia de fallback do RF-5.2 continua vindo do
    /// sistema ("permanece fora do binário"). Carregar as embutidas
    /// primeiro entrega as duas -- o `fontdb` resolve empate de família
    /// pela ordem de registro, então uma cópia do sistema da Iosevka
    /// nunca ganha, e todo o resto do sistema fica disponível para o que
    /// nenhuma face embutida cobre (emoji e CJK, hoje).
    ///
    /// Sem a segunda metade não há fallback **nenhum**: um codepoint fora
    /// das faces embutidas não desenha, sem erro nem tofu. Foi o que
    /// aconteceu na época da IBM Plex com o braille dos gráficos do `btop`
    /// (ela não tinha um só dos 256), com os geométricos e com os
    /// dingbats. A Iosevka cobre os três (ADR-0025), mas a cadeia
    /// continua sendo o que segura o resto do Unicode.
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        for bytes in [MONO_REGULAR, MONO_MEDIUM, ICONS] {
            db.load_font_data(bytes.to_vec());
        }
        db.load_system_fonts();
        let font_system = FontSystem::new_with_locale_and_db("en-US".to_string(), db);
        Self {
            font_system,
            advance_cache: HashMap::new(),
        }
    }

    /// Avanço de `ch` na face `font`, em múltiplos de `size_px`.
    ///
    /// Serve à grade do terminal para saber se um caractere é desenhado
    /// pela face mono pedida ou por uma de fallback: na mono, todo
    /// caractere avança exatamente uma célula; um glyph que veio do
    /// sistema avança o que a fonte dele mandar -- 1.26 célula num
    /// braille, 2.29 num triângulo geométrico. Sem essa informação, um
    /// caractere de fallback empurra todo o resto da linha para a
    /// direita, e a grade deixa de ser grade.
    pub fn advance_em(&mut self, ch: char, font: FontFace) -> f32 {
        if let Some(&cached) = self.advance_cache.get(&(ch, font)) {
            return cached;
        }
        let mut buffer = [0u8; 4];
        let advance = self.measure_width(ch.encode_utf8(&mut buffer), font, ADVANCE_PROBE_SIZE)
            / ADVANCE_PROBE_SIZE;
        self.advance_cache.insert((ch, font), advance);
        advance
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
    ///
    /// **Um shaping**, não um por caractere. A versão anterior remedia o
    /// prefixo a cada caractere (`Buffer` novo + `shape_until_scroll` por
    /// candidato, mais um `String::clone`), e isso roda por aba a cada
    /// layout da barra -- que acontece por frame e por `CursorMoved`. Como
    /// a largura de aba virou fixa, todo título mais longo que o teto caía
    /// nesse laço. Aqui o texto é shapado uma vez e o corte sai do avanço
    /// acumulado dos glyphs.
    ///
    /// Cortar por glyph difere de remedir o prefixo só se houver ligadura
    /// ou kerning entre o último caractere mantido e o primeiro
    /// descartado. As faces do projeto são recortadas mantendo apenas
    /// `ccmp,locl,mark,mkmk` (`scripts/subset-fonts.py`) -- sem `liga`,
    /// `calt` ou `kern`, o avanço é aditivo. Mesma garantia por construção
    /// que o ADR-0025 usa para a variante Fixed.
    pub fn truncate(
        &mut self,
        text: &str,
        font: FontFace,
        size_px: f32,
        max_width: f32,
    ) -> (String, bool) {
        if text.is_empty() {
            return (String::new(), false);
        }

        let metrics = Metrics::new(size_px, size_px * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(None, None);
        let attrs = attrs_for(font);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let glyphs: Vec<(usize, f32)> = buffer
            .layout_runs()
            .next()
            .map(|run| run.glyphs.iter().map(|g| (g.start, g.w)).collect())
            .unwrap_or_default();

        let total: f32 = glyphs.iter().map(|&(_, w)| w).sum();
        if total <= max_width {
            return (text.to_string(), false);
        }

        let ellipsis_width = self.measure_width(&ELLIPSIS.to_string(), font, size_px);
        let budget = (max_width - ellipsis_width).max(0.0);

        // O corte é o menor `start` entre os glyphs descartados, não o
        // `start` do primeiro deles: sob BiDi a ordem visual dos glyphs não
        // é a ordem de byte do texto, e o mínimo é o único ponto que não
        // deixa para trás um trecho já orçado.
        let mut used = 0.0;
        let mut cut = text.len();
        for &(start, w) in &glyphs {
            if used + w > budget {
                cut = cut.min(start);
            } else {
                used += w;
            }
        }

        let mut result = String::with_capacity(cut + ELLIPSIS.len_utf8());
        result.push_str(&text[..cut]);
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

    /// A troca do laço por prefixo pelo caminho de um shaping só não pode
    /// mudar onde o corte cai. A referência aqui é a implementação
    /// anterior, escrita à mão: mede cada prefixo e para no primeiro que
    /// estoura o orçamento.
    #[test]
    fn truncate_cuts_where_the_per_prefix_loop_would() {
        let mut m = TextMeasurer::new();
        let font = FontFace::Sans {
            weight: SansWeight::Regular,
        };
        let titles = [
            "cargo build --workspace",
            "vim: a very long file name.rs",
            r"C:\Projetos\_study\porecatu -- powershell",
            "acentuação e cedilha não podem cortar no meio do byte",
        ];
        for title in titles {
            let full = m.measure_width(title, font, SIZE);
            for divisor in [1.5_f32, 2.0, 3.0, 6.0] {
                let cap = full / divisor;

                let ellipsis_width = m.measure_width(&ELLIPSIS.to_string(), font, SIZE);
                let budget = (cap - ellipsis_width).max(0.0);
                let mut reference = String::new();
                for ch in title.chars() {
                    let mut candidate = reference.clone();
                    candidate.push(ch);
                    if m.measure_width(&candidate, font, SIZE) > budget {
                        break;
                    }
                    reference = candidate;
                }
                reference.push(ELLIPSIS);

                let (got, truncated) = m.truncate(title, font, SIZE, cap);
                assert!(truncated, "{title:?} em {cap} deveria cortar");
                assert_eq!(got, reference, "{title:?} em {cap}");
            }
        }
    }

    /// Corte que cai dentro de um caractere multibyte tem de sair num
    /// limite de caractere -- `LayoutGlyph::start` é indice de byte, e
    /// fatiar no lugar errado é panic, não texto torto.
    #[test]
    fn truncate_respects_char_boundaries() {
        let mut m = TextMeasurer::new();
        let font = FontFace::Sans {
            weight: SansWeight::Regular,
        };
        let text = "áéíóú çãõ ünïcödé";
        let full = m.measure_width(text, font, SIZE);
        for steps in 1..=20 {
            let (got, _) = m.truncate(text, font, SIZE, full * steps as f32 / 20.0);
            assert!(got.ends_with(ELLIPSIS) || got == text);
        }
    }

    /// Pina `BASELINE_FROM_TOP` na metrica real da face: shapa um icone,
    /// le onde o `cosmic-text` pos a baseline e refaz a conta que a
    /// constante documenta. Quebra se a face de icones trocar de metrica
    /// vertical ou se a altura de linha do projeto sair de `1.2`.
    #[test]
    fn icon_baseline_matches_the_shaped_metrics() {
        let mut m = TextMeasurer::new();
        let size = 10.0_f32;
        let metrics = Metrics::new(size, size * 1.2);
        let mut buffer = Buffer::new(&mut m.font_system, metrics);
        buffer.set_size(None, None);
        let attrs = attrs_for(FontFace::Icon);
        buffer.set_text(crate::icon::X.glyph, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut m.font_system, false);

        let run = buffer.layout_runs().next().expect("icone deveria shapar");
        let baseline = (run.line_y - run.line_top) / size;
        assert!(
            (baseline - 1.1).abs() < 0.001,
            "BASELINE_FROM_TOP desatualizado: medido {baseline}"
        );
    }

    /// Regressao: os icones do chrome eram glyphs Unicode (U+2715, U+25B6,
    /// U+25BC) que a IBM Plex Sans nao tem no `cmap`. Como o `fontdb`
    /// nunca carrega fonte do sistema, nao havia fallback e eles nao
    /// desenhavam nada. Medir cada codepoint e o que pega a face sumindo
    /// do binario ou o codepoint mudando de valor.
    #[test]
    fn every_named_icon_has_a_glyph_in_the_icon_face() {
        let mut m = TextMeasurer::new();
        for (name, icon) in crate::icon::ALL {
            let width = m.measure_width(icon.glyph, FontFace::Icon, SIZE);
            assert!(width > 0.0, "icone `{name}` nao tem glyph na face");
        }
    }

    /// Toda glyph desta face avanca 1 em, largura de tinta a parte -- e a
    /// premissa de `Icon::centered_origin` nao medir texto. Se cair, o
    /// centramento horizontal do chrome sai errado em silencio.
    #[test]
    fn every_icon_advances_exactly_one_em() {
        let mut m = TextMeasurer::new();
        for (name, icon) in crate::icon::ALL {
            let advance = m.measure_width(icon.glyph, FontFace::Icon, SIZE);
            assert!(
                (advance - SIZE).abs() < 0.01,
                "icone `{name}` avanca {advance}, nao 1 em ({SIZE})"
            );
        }
    }

    /// Pina a caixa de tinta de cada icone contra a rasterizacao real: o
    /// `swash` devolve onde os pixels de fato caem em volta da origem, e e
    /// dai que saem `ink_width_em`/`ink_height_em`. Sem isto as duas
    /// constantes seriam numero escolhido a olho -- exatamente o que o
    /// CLAUDE.md proibe.
    #[test]
    fn ink_box_of_every_icon_matches_the_rasterized_glyph() {
        use glyphon::{SwashCache, SwashContent};

        let mut m = TextMeasurer::new();
        let mut swash = SwashCache::new();
        // Grande o bastante para o erro de arredondamento do rasterizador
        // (~1px) ficar abaixo da tolerancia de 0.02 em.
        let size = 200.0_f32;

        for (name, icon) in crate::icon::ALL {
            let metrics = Metrics::new(size, size * 1.2);
            let mut buffer = Buffer::new(&mut m.font_system, metrics);
            buffer.set_size(None, None);
            let attrs = attrs_for(FontFace::Icon);
            buffer.set_text(icon.glyph, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut m.font_system, false);

            let run = buffer.layout_runs().next().expect("icone deveria shapar");
            let glyph = run.glyphs.first().expect("um glyph");
            let physical = glyph.physical((0.0, 0.0), 1.0);
            let image = swash
                .get_image_uncached(&mut m.font_system, physical.cache_key)
                .expect("icone deveria rasterizar");
            assert_ne!(
                image.content,
                SwashContent::Color,
                "icone `{name}` deveria ser mascara, nao bitmap colorido"
            );
            let placement = image.placement;

            let width_em = placement.width as f32 / size;
            let height_em = placement.height as f32 / size;
            assert!(
                (width_em - icon.ink_width_em).abs() < 0.02,
                "ink_width_em de `{name}` desatualizado: medido {width_em}"
            );
            assert!(
                (height_em - icon.ink_height_em).abs() < 0.02,
                "ink_height_em de `{name}` desatualizado: medido {height_em}"
            );
            // O desenho e centrado na em nos dois eixos -- e o que
            // permite `centered_origin` centrar a em e acertar o desenho
            // com uma constante so, sem dado por icone.
            let center_x = (placement.left as f32 + placement.width as f32 / 2.0) / size;
            let center_y_above_baseline =
                (placement.top as f32 - placement.height as f32 / 2.0) / size;
            assert!(
                (center_x - 0.5).abs() < 0.02,
                "desenho de `{name}` nao e centrado na em em X: {center_x}"
            );
            assert!(
                (center_y_above_baseline - 0.5).abs() < 0.02,
                "desenho de `{name}` nao e centrado na em em Y: {center_y_above_baseline}"
            );
        }
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
