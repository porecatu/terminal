// SPDX-License-Identifier: GPL-3.0-or-later

//! `[terminal.font]` -- RF-5.1 a RF-5.10. Classe de recarga B: métrica de
//! fonte decide a largura de célula, logo colunas e linhas (RF-5.28).

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ZoomScope {
    #[default]
    All,
    Active,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Font {
    pub family: String,
    /// Cadeia de fallback, em ordem. Glyph ausente na família principal é
    /// procurado aqui antes de virar retângulo vazio. RF-5.2.
    pub fallback: Vec<String>,
    /// A Iosevka Fixed avança 0.5 em em todo glyph -- ver a armadilha
    /// conhecida em CLAUDE.md sobre `size / 2 * scale` caindo em pixel
    /// inteiro. O default é 14.0, não 13.0 (usado numa revisão anterior):
    /// a 13 o avanço era 6.5 e a célula arredondava para 7.0, deixando meio
    /// pixel de folga por célula; a 14 o glyph preenche a célula sem sobra.
    /// Conforto de calibração, não correção -- em 125% nenhum tamanho perto
    /// deste fecha do mesmo jeito, e é o teste em em de
    /// `porecatu_ui::paint::fits_the_grid` que garante a grade em qualquer
    /// tamanho e escala, não este valor.
    pub size: f64,
    /// Vazio = deriva da família principal. RF-5.4.
    pub bold_family: String,
    pub italic_family: String,
    pub bold_italic_family: String,
    pub synthesize_bold: bool,
    pub synthesize_italic: bool,
    /// Texto em negrito usa a versão brilhante da cor ANSI. RF-5.5.
    pub bold_is_bright: bool,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub ligatures: bool,
    pub zoom_scope: ZoomScope,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: "Iosevka Fixed".to_owned(),
            fallback: vec![
                "Symbols Nerd Font Mono".to_owned(),
                "Noto Color Emoji".to_owned(),
                "Noto Sans CJK".to_owned(),
            ],
            size: 14.0,
            bold_family: String::new(),
            italic_family: String::new(),
            bold_italic_family: String::new(),
            synthesize_bold: true,
            synthesize_italic: true,
            bold_is_bright: false,
            line_height: 1.75,
            letter_spacing: 0.0,
            ligatures: false,
            zoom_scope: ZoomScope::All,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let font = Font::default();
        assert_eq!(font.family, "Iosevka Fixed");
        assert_eq!(font.size, 14.0);
        assert_eq!(
            font.fallback,
            vec![
                "Symbols Nerd Font Mono".to_owned(),
                "Noto Color Emoji".to_owned(),
                "Noto Sans CJK".to_owned(),
            ]
        );
        assert_eq!(font.zoom_scope, ZoomScope::All);
    }
}
