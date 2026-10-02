// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.settings]` -- PRD-016, ADR-0060 §5. Dimensões da janela de
//! configurações e de dois controles dela; cores e tipografia vêm dos tokens
//! que o canvas já dava ao painel de configurações (ADR-0060 §2 e §3), nada
//! aqui é cor. Classe de recarga A para as de dentro da tela, aplicadas no
//! próximo quadro da janela aberta; as quatro de tamanho da janela valem na
//! próxima abertura da tela (classe C).

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Tamanho de abertura da janela, em pixels lógicos.
    pub window_width: i32,
    pub window_height: i32,
    /// Menor tamanho a que o usuário pode reduzir a janela.
    pub min_width: i32,
    pub min_height: i32,
    /// Largura da guia lateral.
    pub sidebar_width: i32,
    /// Largura do campo de texto e do botão de escolha.
    pub text_field_width: i32,
    /// Largura do campo numérico.
    pub number_field_width: i32,
    /// Espaço vertical entre linhas de opção.
    pub row_gap: i32,
    /// Lado de cada quadrado da amostra de um tema.
    pub theme_swatch_size: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            window_width: 900,
            window_height: 640,
            min_width: 640,
            min_height: 420,
            sidebar_width: 200,
            text_field_width: 240,
            number_field_width: 88,
            row_gap: 8,
            theme_swatch_size: 12,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_the_adr_0060_proposal() {
        let s = Settings::default();
        assert_eq!((s.window_width, s.window_height), (900, 640));
        assert_eq!((s.min_width, s.min_height), (640, 420));
        assert_eq!(s.sidebar_width, 200);
        assert_eq!((s.text_field_width, s.number_field_width), (240, 88));
        assert_eq!((s.row_gap, s.theme_swatch_size), (8, 12));
    }
}
