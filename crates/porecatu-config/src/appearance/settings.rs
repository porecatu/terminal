// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.settings]` -- PRD-016, ADR-0060 §5. Dimensões da janela de
//! configurações e de dois controles dela, mais as seis cores do corpo da
//! janela (painel, linha de opção, chip de atalho, separador do rodapé), que
//! têm por default os tokens que o canvas já dava ao painel (ADR-0060 §2 e
//! §3) e, desde a correção do tema, podem ser sobrescritas por `[[themes]]`
//! (ADR-0031 §1) -- sem isso um tema claro deixava o painel escuro ao lado
//! de uma guia clara. Classe de recarga A para as de dentro da tela,
//! aplicadas no próximo quadro da janela aberta; as quatro de tamanho da
//! janela valem na próxima abertura da tela (classe C).

use serde::Deserialize;

use crate::color::Color;

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
    /// Fundo do painel de opções: o token "Drawer" (espec. §1.2).
    pub panel_background: Color,
    /// Fundo da linha de opção: a linha de perfil do drawer. A borda é
    /// `dialog.button_border`, já tematizada.
    pub row_background: Color,
    /// Fundo do chip de atalho (espec. §2.12). A borda é `group_editor.divider`.
    pub chip_background: Color,
    /// Linha entre o painel e o rodapé.
    pub footer_separator: Color,
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
            panel_background: Color::hex("#171b21"),
            row_background: Color::hex("#1c2028"),
            chip_background: Color::hex("#1e232b"),
            footer_separator: Color::hex("#23272f"),
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

    #[test]
    fn default_colors_are_the_drawer_tokens_the_window_always_had() {
        let s = Settings::default();
        assert_eq!(s.panel_background, Color::hex("#171b21"));
        assert_eq!(s.row_background, Color::hex("#1c2028"));
        assert_eq!(s.chip_background, Color::hex("#1e232b"));
        assert_eq!(s.footer_separator, Color::hex("#23272f"));
    }
}
