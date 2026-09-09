// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.status_bar]` -- PRD-009 e ADR-0048. Faixa fixa no rodapé
//! da janela, seção 2.8 da especificação visual. Classe de recarga B:
//! `enabled`, `height` e `font_size` mudam a área útil do terminal, logo
//! colunas e linhas. O resto é classe A.

use serde::Deserialize;

use crate::color::Color;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct StatusBar {
    /// RF-9.1. Ligada por padrão: desligada, ela anula o RF-9.4, porque
    /// quem não sabe que o OSC 7 pode faltar nunca vai ligá-la para
    /// descobrir (ADR-0048 §6). Desligada, não desenha nem ocupa altura.
    pub enabled: bool,
    pub height: i32,
    pub padding_x: i32,
    /// Folga entre segmentos, dentro de cada zona.
    pub gap: i32,
    pub font_size: f64,
    /// Sem `background`: a faixa não pinta fundo próprio. O `clear` da
    /// janela já é a cor das barras, e um quad opaco aqui cobriria a
    /// sombra do quadro do terminal, que desce sobre o topo dela
    /// (ADR-0048 §10).
    pub foreground: Color,
    /// Nome do shell -- o único segmento colorido, é o que distingue a
    /// aba de relance.
    pub shell: Color,
    /// RF-9.4: cor do diretório quando ele não veio de um OSC 7, ou seja,
    /// quando é o de spawn e pode estar obsoleto. Um degrau abaixo de
    /// `foreground` na escada de texto da §1.4 -- e não um alfa sobre
    /// ela, que a esta altura de fonte apagaria o caminho em vez de
    /// marcá-lo (ADR-0048 §4).
    pub stale_cwd: Color,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self {
            enabled: true,
            height: 26,
            padding_x: 12,
            gap: 16,
            font_size: 10.5,
            foreground: Color::hex("#a8b0bb"),
            shell: Color::hex("#5ed3bc"),
            stale_cwd: Color::hex("#828a96"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        assert!(StatusBar::default().enabled);
        assert_eq!(StatusBar::default().height, 26);
        assert_eq!(StatusBar::default().stale_cwd, Color::hex("#828a96"));
    }
}
