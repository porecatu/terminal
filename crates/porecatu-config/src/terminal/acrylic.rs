// SPDX-License-Identifier: GPL-3.0-or-later

//! `[terminal.acrylic]` -- PRD-019 (RF-19.1, RF-19.8, RF-19.10, RF-19.19),
//! ADR-0068 §7 e §8. Classe de recarga A.
//!
//! Só a configuração e a regra de disponibilidade vivem aqui. Desfocar e
//! desenhar é de `porecatu-ui` e `porecatu-render`. A seção não tem cor, então
//! não entra em tema (ADR-0067).

use serde::Deserialize;

use crate::Config;

/// Menor raio de desfoque aceito, em pixels lógicos. RF-19.8.
pub const MIN_BLUR: f32 = 1.0;
/// Maior raio de desfoque aceito, em pixels lógicos. RF-19.8.
pub const MAX_BLUR: f32 = 64.0;
/// Raio de desfoque padrão, em pixels lógicos. RF-19.8.
pub const DEFAULT_BLUR: f32 = 24.0;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Acrylic {
    /// Liga o efeito. RF-19.1. Só desenha com as duas condições do
    /// RF-19.10; veja [`acrylic_state`].
    pub enabled: bool,
    /// Raio do desfoque, em pixels lógicos, de 1 a 64. Fora da faixa não é
    /// erro de config -- como `BackgroundImage::opacity`, quem consome limita
    /// (`clamped_blur`) e a recarga avisa. RF-19.8.
    pub blur: f32,
}

impl Default for Acrylic {
    fn default() -> Self {
        Self {
            enabled: false,
            blur: DEFAULT_BLUR,
        }
    }
}

impl Acrylic {
    /// `blur` limitado a `MIN_BLUR..=MAX_BLUR`; NaN vira `DEFAULT_BLUR`.
    pub fn clamped_blur(&self) -> f32 {
        if self.blur.is_nan() {
            DEFAULT_BLUR
        } else {
            self.blur.clamp(MIN_BLUR, MAX_BLUR)
        }
    }

    /// `blur` não é o que `clamped_blur` devolve: está fora da faixa, ou é
    /// NaN. É o que a recarga avisa (RF-19.8).
    pub fn blur_is_adjusted(&self) -> bool {
        self.blur.is_nan() || !(MIN_BLUR..=MAX_BLUR).contains(&self.blur)
    }
}

/// Por que o acrílico ligado não vale. RF-19.10.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcrylicBlock {
    /// `[appearance.window] opacity` diferente de `1.0`: com a janela
    /// translúcida, o desktop apareceria nítido atrás do que o app desfoca.
    WindowOpacity,
    /// `[appearance.window.background_image] path` vazio: sem imagem, desfocar
    /// o fundo liso não muda nada.
    NoWindowImage,
}

/// O que a configuração diz sobre o acrílico. RF-19.10, ADR-0068 §7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcrylicState {
    /// `enabled = false`.
    Disabled,
    /// Ligado, mas uma das condições falta. Desenha-se como se estivesse
    /// desligado, e avisa-se o motivo (RF-19.12).
    Blocked(AcrylicBlock),
    /// Ligado e com as duas condições: vale desenhar.
    Active,
}

/// A regra de disponibilidade do acrílico (RF-19.10), uma só para a pintura,
/// o aviso e a tela de configurações (ADR-0068 §7). Função pura sobre
/// `&Config`: a tela a chama sobre a config com as pendências aplicadas
/// (RF-19.20).
///
/// - a opacidade da janela é comparada **exatamente** com `1.0`, que é o
///   padrão e o valor que a tela grava;
/// - a imagem da janela conta pelo `path` da **configuração** -- vazio depois
///   de `trim`, como o resto do app decide -, nunca pelo estado de carga
///   (RF-19.11, ADR-0063 §1);
/// - com as duas condições faltando, `WindowOpacity` vence (RF-19.19).
pub fn acrylic_state(config: &Config) -> AcrylicState {
    if !config.terminal.acrylic.enabled {
        return AcrylicState::Disabled;
    }
    if config.appearance.window.opacity != 1.0 {
        return AcrylicState::Blocked(AcrylicBlock::WindowOpacity);
    }
    if config
        .appearance
        .window
        .background_image
        .path
        .trim()
        .is_empty()
    {
        return AcrylicState::Blocked(AcrylicBlock::NoWindowImage);
    }
    AcrylicState::Active
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn config(enabled: bool, window_opacity: f64, window_image: &str) -> Config {
        let mut config = Config::default();
        config.terminal.acrylic.enabled = enabled;
        config.appearance.window.opacity = window_opacity;
        config.appearance.window.background_image.path = window_image.to_owned();
        config
    }

    #[test]
    fn default_matches_example_toml() {
        let acrylic = Acrylic::default();
        assert!(!acrylic.enabled);
        assert_eq!(acrylic.blur, 24.0);
    }

    #[test]
    fn parse_without_the_table_is_defaults() {
        let (config, unknown) = parse("[terminal]\nbackground_opacity = 0.5\n").unwrap();
        assert_eq!(config.terminal.acrylic, Acrylic::default());
        assert!(unknown.is_empty());
    }

    #[test]
    fn parse_with_the_table() {
        let (config, unknown) = parse("[terminal.acrylic]\nenabled = true\nblur = 8\n").unwrap();
        assert!(unknown.is_empty());
        assert!(config.terminal.acrylic.enabled);
        assert_eq!(config.terminal.acrylic.blur, 8.0);
    }

    #[test]
    fn blur_accepts_a_fraction() {
        let (config, _) = parse("[terminal.acrylic]\nblur = 2.5\n").unwrap();
        assert_eq!(config.terminal.acrylic.clamped_blur(), 2.5);
    }

    #[test]
    fn partial_table_keeps_the_other_default() {
        let (config, _) = parse("[terminal.acrylic]\nenabled = true\n").unwrap();
        assert!(config.terminal.acrylic.enabled);
        assert_eq!(config.terminal.acrylic.blur, DEFAULT_BLUR);
        assert_ne!(Config::default().terminal.acrylic, config.terminal.acrylic);

        let (config, _) = parse("[terminal.acrylic]\nblur = 40\n").unwrap();
        assert!(!config.terminal.acrylic.enabled);
        assert_eq!(config.terminal.acrylic.blur, 40.0);
    }

    #[test]
    fn blur_out_of_range_parses_and_is_clamped_by_the_consumer() {
        let (config, _) = parse("[terminal.acrylic]\nblur = 100\n").unwrap();
        let acrylic = config.terminal.acrylic;
        assert_eq!(acrylic.blur, 100.0);
        assert_eq!(acrylic.clamped_blur(), MAX_BLUR);
        assert!(acrylic.blur_is_adjusted());

        for low in ["0", "-3", "0.5"] {
            let (config, _) = parse(&format!("[terminal.acrylic]\nblur = {low}\n")).unwrap();
            assert_eq!(config.terminal.acrylic.clamped_blur(), MIN_BLUR, "{low}");
            assert!(config.terminal.acrylic.blur_is_adjusted(), "{low}");
        }
    }

    #[test]
    fn the_range_limits_are_not_adjusted() {
        for edge in [MIN_BLUR, DEFAULT_BLUR, MAX_BLUR] {
            let acrylic = Acrylic {
                blur: edge,
                ..Acrylic::default()
            };
            assert_eq!(acrylic.clamped_blur(), edge);
            assert!(!acrylic.blur_is_adjusted(), "{edge}");
        }
    }

    #[test]
    fn nan_is_the_default_and_counts_as_adjusted() {
        let (config, _) = parse("[terminal.acrylic]\nblur = nan\n").unwrap();
        let acrylic = config.terminal.acrylic;
        assert!(acrylic.blur.is_nan());
        assert_eq!(acrylic.clamped_blur(), DEFAULT_BLUR);
        assert!(acrylic.blur_is_adjusted());
    }

    #[test]
    fn infinities_are_clamped_to_the_extremes() {
        let (config, _) = parse("[terminal.acrylic]\nblur = inf\n").unwrap();
        assert_eq!(config.terminal.acrylic.clamped_blur(), MAX_BLUR);
        let (config, _) = parse("[terminal.acrylic]\nblur = -inf\n").unwrap();
        assert_eq!(config.terminal.acrylic.clamped_blur(), MIN_BLUR);
    }

    #[test]
    fn fields_of_the_wrong_type_are_errors() {
        assert!(parse("[terminal.acrylic]\nenabled = \"sim\"\n").is_err());
        assert!(parse("[terminal.acrylic]\nblur = \"x\"\n").is_err());
    }

    #[test]
    fn unknown_key_is_reported() {
        let (_, unknown) = parse("[terminal.acrylic]\nstrength = 3\n").unwrap();
        assert_eq!(unknown, ["terminal.acrylic.strength"]);
    }

    #[test]
    fn disabled_whatever_the_rest() {
        assert_eq!(
            acrylic_state(&config(false, 1.0, "fundo.png")),
            AcrylicState::Disabled
        );
        // Desligado não é bloqueado: nada a avisar.
        assert_eq!(
            acrylic_state(&config(false, 0.5, "")),
            AcrylicState::Disabled
        );
    }

    #[test]
    fn the_default_config_is_disabled() {
        assert_eq!(acrylic_state(&Config::default()), AcrylicState::Disabled);
    }

    #[test]
    fn both_conditions_met_is_active() {
        assert_eq!(
            acrylic_state(&config(true, 1.0, "fundo.png")),
            AcrylicState::Active
        );
    }

    #[test]
    fn a_translucent_window_blocks_by_opacity() {
        assert_eq!(
            acrylic_state(&config(true, 0.9, "fundo.png")),
            AcrylicState::Blocked(AcrylicBlock::WindowOpacity)
        );
    }

    #[test]
    fn a_missing_window_image_blocks_by_image() {
        assert_eq!(
            acrylic_state(&config(true, 1.0, "")),
            AcrylicState::Blocked(AcrylicBlock::NoWindowImage)
        );
    }

    #[test]
    fn a_blank_window_image_path_is_no_image() {
        for blank in ["   ", "\t", " \t "] {
            assert_eq!(
                acrylic_state(&config(true, 1.0, blank)),
                AcrylicState::Blocked(AcrylicBlock::NoWindowImage),
                "{blank:?}"
            );
        }
    }

    #[test]
    fn window_opacity_wins_when_both_conditions_are_missing() {
        assert_eq!(
            acrylic_state(&config(true, 0.9, "")),
            AcrylicState::Blocked(AcrylicBlock::WindowOpacity)
        );
    }

    #[test]
    fn the_opacity_comparison_is_exact() {
        // Sem tolerância a esconder: um fio abaixo de 1.0 já bloqueia.
        let just_below = 1.0 - f64::EPSILON;
        assert_eq!(
            acrylic_state(&config(true, just_below, "fundo.png")),
            AcrylicState::Blocked(AcrylicBlock::WindowOpacity)
        );
        // Acima de 1.0 também não é 1.0 (o consumidor limita, a regra não).
        assert_eq!(
            acrylic_state(&config(true, 1.5, "fundo.png")),
            AcrylicState::Blocked(AcrylicBlock::WindowOpacity)
        );
        assert_eq!(
            acrylic_state(&config(true, f64::NAN, "fundo.png")),
            AcrylicState::Blocked(AcrylicBlock::WindowOpacity)
        );
    }

    #[test]
    fn the_terminal_image_does_not_stand_in_for_the_window_image() {
        let mut config = config(true, 1.0, "");
        config.terminal.background_image.path = "terminal.png".to_owned();
        assert_eq!(
            acrylic_state(&config),
            AcrylicState::Blocked(AcrylicBlock::NoWindowImage)
        );
    }

    #[test]
    fn the_rule_reads_a_parsed_config() {
        let text = "[appearance.window]\nopacity = 1.0\n\
                    [appearance.window.background_image]\npath = \"a.png\"\n\
                    [terminal.acrylic]\nenabled = true\n";
        let (config, _) = parse(text).unwrap();
        assert_eq!(acrylic_state(&config), AcrylicState::Active);
    }
}
