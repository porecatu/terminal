// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.tabs]`, `[appearance.tabs.colors]`, `[appearance.tabs.rename]`
//! e `[appearance.tabs.overflow]` -- RF-4.3 a RF-4.12, RF-1.19. Classe de
//! recarga A, exceto as três alturas marcadas abaixo (mudam `bar_height`).

use serde::Deserialize;

use crate::color::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CloseButtonVisibility {
    #[default]
    Always,
    Hover,
    Never,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Tabs {
    /// [B] altura da barra, em px lógicos -- derivada, não livre (ver
    /// comentário do arquivo de exemplo).
    pub height: i32,
    /// [B] altura da aba DENTRO de um grupo.
    pub tab_height: i32,
    /// [B] respiro entre a trilha e as bordas da barra, nos quatro lados.
    pub trilha_padding: i32,
    /// Largura da aba é fixa (RF-4.3, RF-4.5 emendados): hoje `max_width`
    /// e `min_width` descrevem o mesmo valor.
    pub max_width: i32,
    pub min_width: i32,
    pub padding_left: i32,
    pub padding_right: i32,
    pub gap: i32,
    /// 0 = abas retangulares. RF-4.4.
    pub corner_radius: i32,
    pub icon_button_padding_x: i32,
    /// Tamanho de EM dos ícones do chrome, não do desenho. Ver ADR-0024.
    pub icon_em_size: i32,
    pub font_family: String,
    pub font_size: f64,
    pub show_close_button: CloseButtonVisibility,
    pub show_index: bool,
    pub show_activity_indicator: bool,
    pub show_bell_indicator: bool,
    pub show_new_tab_button: bool,
    /// `[v2]` badge de perfil na aba (PRD-007). Reserva de nome.
    pub show_profile_badge: bool,
    pub hide_when_single_tab: bool,
    pub colors: TabsColors,
    pub rename: TabsRename,
    pub overflow: TabsOverflow,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            height: 52,
            tab_height: 34,
            trilha_padding: 6,
            max_width: 260,
            min_width: 229,
            padding_left: 10,
            padding_right: 6,
            gap: 4,
            corner_radius: 6,
            icon_button_padding_x: 4,
            icon_em_size: 20,
            font_family: "Iosevka Fixed".to_owned(),
            font_size: 13.0,
            show_close_button: CloseButtonVisibility::Always,
            show_index: false,
            show_activity_indicator: true,
            show_bell_indicator: true,
            show_new_tab_button: true,
            show_profile_badge: true,
            hide_when_single_tab: false,
            colors: TabsColors::default(),
            rename: TabsRename::default(),
            overflow: TabsOverflow::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct TabsColors {
    pub bar_background: Color,
    /// Não pintado com `tab_bar_position = "top"` -- ver comentário do
    /// arquivo de exemplo e a seção 4.4 da espec.
    pub bar_border: Color,
    pub background_alpha: f64,
    pub active_background: Color,
    pub active_foreground: Color,
    pub inactive_background: Color,
    pub inactive_foreground: Color,
    pub hover_brightness: f64,
    pub close_button_foreground: Color,
    pub close_button_hover_background: Color,
    pub close_button_hover_foreground: Color,
    /// Rótulo da aba `Exited` (ADR-0017, espec. §2.5) -- o binário já
    /// desenhava este tom antes de esta chave existir.
    pub exited_foreground: Color,
    pub active_border: Color,
    pub inactive_border: Color,
    pub active_border_width: i32,
    pub inactive_border_width: i32,
    pub selected_border: Color,
    pub selected_border_width: i32,
    pub activity_indicator: Color,
    pub bell_indicator: Color,
    pub rename_background: Color,
    pub rename_border: Color,
    pub rename_foreground: Color,
}

impl Default for TabsColors {
    fn default() -> Self {
        Self {
            bar_background: Color::hex("#1b1f26"),
            bar_border: Color::hex("#23272f"),
            background_alpha: 0.85,
            active_background: Color::hex("#282e37"),
            active_foreground: Color::hex("#eaeef3"),
            inactive_background: Color::hex("#191d23"),
            inactive_foreground: Color::hex("#98a0ab"),
            hover_brightness: 1.18,
            close_button_foreground: Color::hex("#e4e8ee"),
            close_button_hover_background: Color::hex("#39404b"),
            close_button_hover_foreground: Color::hex("#e4e8ee"),
            exited_foreground: Color::hex("#727a86"),
            active_border: Color::hex("#39404b"),
            inactive_border: Color::hex("#22262e"),
            active_border_width: 2,
            inactive_border_width: 2,
            selected_border: Color::hex("#5ed3bc"),
            selected_border_width: 2,
            activity_indicator: Color::hex("#86c56a"),
            bell_indicator: Color::hex("#ef8a8a"),
            rename_background: Color::hex("#0e1116"),
            rename_border: Color::hex("#5ed3bc"),
            rename_foreground: Color::hex("#e4e8ee"),
        }
    }
}

/// Geometria do campo de rename (espec. 2.5). Cores em `TabsColors`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct TabsRename {
    pub width: i32,
    pub height: i32,
    pub font_size: f64,
    pub padding_x: i32,
}

impl Default for TabsRename {
    fn default() -> Self {
        Self {
            width: 120,
            height: 20,
            font_size: 12.0,
            padding_x: 5,
        }
    }
}

/// Indicador de abas fora da vista (RF-1.19).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct TabsOverflow {
    pub indicator_size: i32,
    pub indicator_background: Color,
    pub edge_inset: i32,
    pub scroll_step: i32,
}

impl Default for TabsOverflow {
    fn default() -> Self {
        Self {
            indicator_size: 18,
            indicator_background: Color::hex("#12151a"),
            edge_inset: 4,
            scroll_step: 90,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let tabs = Tabs::default();
        assert_eq!(tabs.height, 52);
        assert_eq!(tabs.font_family, "Iosevka Fixed");
        assert_eq!(tabs.show_close_button, CloseButtonVisibility::Always);
        assert_eq!(tabs.colors.bar_background, Color::hex("#1b1f26"));
        assert_eq!(tabs.rename.width, 120);
        assert_eq!(tabs.overflow.indicator_size, 18);
    }
}
