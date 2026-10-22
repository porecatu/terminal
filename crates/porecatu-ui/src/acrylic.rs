// SPDX-License-Identifier: GPL-3.0-or-later

//! Avisos do efeito acrílico -- PRD-019 (RF-19.8, RF-19.12, RF-19.22),
//! ADR-0068 §11.
//!
//! Só a decisão de **emitir ou não** e a escolha da frase vivem aqui, ambas
//! puras: quem empurra o aviso na pilha da janela é `App`. A regra de
//! disponibilidade em si (`acrylic_state`) é de `porecatu-config`, a mesma que
//! a pintura e a tela de configurações vão chamar.

use std::hash::{Hash, Hasher};

use porecatu_config::{
    Acrylic, AcrylicBlock, AcrylicState, Config, MAX_BLUR, MIN_BLUR, acrylic_state,
};
use porecatu_locale::Catalog;
use porecatu_render::{BlurSource, Color, Rect};

use crate::background_image::WindowImage;
use crate::messages::msg;
use crate::warning::Severity;

/// Chave como aparece no arquivo de exemplo: identificador, igual em todo
/// idioma (o mesmo papel do `[git] remote_poll_interval_secs` do aviso do
/// piso).
const BLUR_KEY: &str = "[terminal.acrylic] blur";

/// Um aviso do acrílico, ainda sem frase: o catálogo só entra em `compose`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AcrylicNotice {
    /// Ligado e sem uma das condições do RF-19.10. RF-19.12.
    Blocked(AcrylicBlock),
    /// `blur` fora de `1..=64` (ou NaN): o app usa o valor limitado. RF-19.8.
    BlurAdjusted(Acrylic),
}

impl AcrylicNotice {
    /// Severidade, título e corpo, pelo catálogo (ADR-0056). Uma frase por
    /// motivo, nunca concatenada: o aviso abre pelo que falta.
    pub(crate) fn compose(&self, catalog: &Catalog) -> (Severity, String, String) {
        match self {
            AcrylicNotice::Blocked(block) => {
                use msg::notice::acrylic as phrases;
                let body = match block {
                    AcrylicBlock::WindowOpacity => phrases::window_opacity(catalog),
                    AcrylicBlock::NoWindowImage => phrases::no_window_image(catalog),
                };
                (Severity::Warning, phrases::title(catalog), body)
            }
            AcrylicNotice::BlurAdjusted(acrylic) => {
                use msg::notice::acrylic_blur_adjusted as phrases;
                (
                    Severity::Info,
                    phrases::title(catalog),
                    phrases::body(
                        catalog,
                        BLUR_KEY,
                        MIN_BLUR,
                        MAX_BLUR,
                        acrylic.clamped_blur(),
                    ),
                )
            }
        }
    }
}

/// A fonte do desfoque desta janela, ou `None` quando o acrílico não vale
/// (ADR-0068 §2 e §7): é a camada **da janela** -- a cor da barra e, se há,
/// a imagem da janela já posta pelo `placement`, a mesma primitiva que abre
/// `Layer::Grid` (mesmo retângulo, mesmo `uv`, mesmo alfa). Nem abas, nem
/// barra de status, nem sombras, nem outros quadros entram nela: é o que a
/// torna estável quando uma aba muda de título.
///
/// Só `acrylic_state == Active` produz fonte -- a regra única de
/// `porecatu-config`, a mesma do aviso e da tela de configurações. A imagem
/// ainda carregando, ou que falhou, chega como `None` em `window_image`: a
/// fonte vai só com `background`, sem efeito visível (RF-19.11), e quando a
/// imagem fica pronta a `key` muda e o desfoque é refeito no frame da chegada.
pub(crate) fn blur_source(
    config: &Config,
    background: Color,
    window_image: Option<WindowImage>,
    window_rect: Rect,
) -> Option<BlurSource> {
    if acrylic_state(config) != AcrylicState::Active {
        return None;
    }
    let radius_logical = config.terminal.acrylic.clamped_blur();
    Some(BlurSource {
        background,
        primitives: window_image
            .map(|image| image.primitive(window_rect, 0.0))
            .into_iter()
            .collect(),
        radius_logical,
        key: blur_key(background, window_image, radius_logical),
    })
}

/// A chave de identidade da fonte (ADR-0068 §4): muda quando muda o que entra
/// no desfoque -- a imagem (qual, onde cai, com que alfa; o `mode` e a
/// `opacity` da config já estão nisso), o `blur` e a cor de fundo --, e **só**
/// então. Tamanho físico e escala a `WindowSurface` já guarda; saída do
/// terminal, cursor, hover e abas não passam por aqui. Estável dentro do
/// processo, que é o que a chave precisa.
pub(crate) fn blur_key(background: Color, window_image: Option<WindowImage>, radius: f32) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    radius.to_bits().hash(&mut hasher);
    for channel in [background.r, background.g, background.b, background.a] {
        channel.to_bits().hash(&mut hasher);
    }
    match window_image {
        None => 0_u8.hash(&mut hasher),
        Some(image) => {
            1_u8.hash(&mut hasher);
            image.image.hash(&mut hasher);
            for value in [
                image.rect.x,
                image.rect.y,
                image.rect.width,
                image.rect.height,
                image.uv.x,
                image.uv.y,
                image.uv.width,
                image.uv.height,
                image.alpha,
            ] {
                value.to_bits().hash(&mut hasher);
            }
            image.repeat.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Há aviso de bloqueio a emitir? Só a recarga que **cria** o bloqueio ou
/// **troca** o motivo emite: de `Active`/`Disabled` para `Blocked`, ou de um
/// motivo para outro. Continuar bloqueado pelo mesmo motivo não repete, e
/// voltar a `Active`/`Disabled` rearma. RF-19.12, ADR-0068 §11.
pub(crate) fn block_notice_due(
    previous: AcrylicState,
    current: AcrylicState,
) -> Option<AcrylicBlock> {
    match current {
        AcrylicState::Blocked(block) if previous != current => Some(block),
        _ => None,
    }
}

/// Há aviso de `blur` limitado a emitir? Só com o acrílico ligado -- desligado,
/// o número não faz nada --, e uma vez por valor: o mesmo número fora da faixa
/// que já foi avisado não repete a cada recarga. Comparado pelos bits, para o
/// NaN (que nunca é igual a si mesmo) não avisar sempre. RF-19.8.
pub(crate) fn blur_notice_due(previous: &Acrylic, current: &Acrylic) -> bool {
    current.enabled
        && current.blur_is_adjusted()
        && !(previous.enabled && previous.blur.to_bits() == current.blur.to_bits())
}

/// Os avisos que a passagem da config `previous` para `current` pede, na ordem
/// em que saem. No arranque `previous` é `Config::default()`: desligado, sem
/// nada avisado ainda.
pub(crate) fn notices(previous: &Config, current: &Config) -> Vec<AcrylicNotice> {
    let mut out = Vec::new();
    if let Some(block) = block_notice_due(acrylic_state(previous), acrylic_state(current)) {
        out.push(AcrylicNotice::Blocked(block));
    }
    if blur_notice_due(&previous.terminal.acrylic, &current.terminal.acrylic) {
        out.push(AcrylicNotice::BlurAdjusted(
            current.terminal.acrylic.clone(),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::test_support;

    const OPACITY: AcrylicState = AcrylicState::Blocked(AcrylicBlock::WindowOpacity);
    const NO_IMAGE: AcrylicState = AcrylicState::Blocked(AcrylicBlock::NoWindowImage);

    // ---- a fonte do desfoque -------------------------------------------

    const BAR: Color = Color {
        r: 0.1,
        g: 0.12,
        b: 0.15,
        a: 1.0,
    };

    fn window() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        }
    }

    fn image(id: u32, alpha: f32) -> WindowImage {
        WindowImage {
            rect: window(),
            uv: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            repeat: false,
            alpha,
            image: porecatu_render::ImageId::from_raw(id),
        }
    }

    fn active() -> Config {
        config(true, 1.0, "fundo.png")
    }

    #[test]
    fn only_an_active_acrylic_has_a_source() {
        assert!(blur_source(&active(), BAR, Some(image(1, 1.0)), window()).is_some());
        for blocked_or_off in [
            Config::default(),
            config(false, 1.0, "fundo.png"),
            config(true, 0.9, "fundo.png"),
            config(true, 1.0, ""),
        ] {
            assert!(
                blur_source(&blocked_or_off, BAR, Some(image(1, 1.0)), window()).is_none(),
                "{:?}",
                acrylic_state(&blocked_or_off)
            );
        }
    }

    #[test]
    fn the_source_is_the_bar_color_under_the_same_primitive_the_window_head_has() {
        let image = image(3, 0.7);
        let source = blur_source(&active(), BAR, Some(image), window()).unwrap();
        assert_eq!(source.background, BAR);
        assert_eq!(source.primitives.len(), 1);
        assert_eq!(
            format!("{:?}", source.primitives[0]),
            format!("{:?}", image.primitive(window(), 0.0))
        );
    }

    /// Imagem carregando ou falha: só a cor, sem efeito visível (RF-19.11).
    #[test]
    fn a_missing_window_image_leaves_just_the_background() {
        let source = blur_source(&active(), BAR, None, window()).unwrap();
        assert!(source.primitives.is_empty());
        assert_eq!(source.background, BAR);
    }

    #[test]
    fn the_radius_is_the_clamped_blur() {
        let mut config = active();
        for (configured, used) in [(24.0, 24.0), (500.0, 64.0), (0.0, 1.0), (f32::NAN, 24.0)] {
            config.terminal.acrylic.blur = configured;
            let source = blur_source(&config, BAR, None, window()).unwrap();
            assert_eq!(source.radius_logical, used, "{configured}");
        }
    }

    fn key(image: Option<WindowImage>, radius: f32, background: Color) -> u64 {
        blur_key(background, image, radius)
    }

    #[test]
    fn the_same_inputs_give_the_same_key() {
        assert_eq!(
            key(Some(image(1, 1.0)), 24.0, BAR),
            key(Some(image(1, 1.0)), 24.0, BAR)
        );
        assert_eq!(key(None, 24.0, BAR), key(None, 24.0, BAR));
    }

    #[test]
    fn each_input_the_blur_depends_on_changes_the_key() {
        let base = key(Some(image(1, 1.0)), 24.0, BAR);
        let other_bar = Color { r: 0.2, ..BAR };
        let mut moved = image(1, 1.0);
        moved.rect.width = 1000.0; // outro `mode`/tamanho: a imagem cai noutro lugar
        let mut tiled = image(1, 1.0);
        tiled.repeat = true;
        let mut cropped = image(1, 1.0);
        cropped.uv.width = 0.5;
        for (what, changed) in [
            ("a imagem trocada", key(Some(image(2, 1.0)), 24.0, BAR)),
            ("a opacity da imagem", key(Some(image(1, 0.5)), 24.0, BAR)),
            ("o blur", key(Some(image(1, 1.0)), 8.0, BAR)),
            ("a cor da barra", key(Some(image(1, 1.0)), 24.0, other_bar)),
            ("a posição", key(Some(moved), 24.0, BAR)),
            ("o repeat", key(Some(tiled), 24.0, BAR)),
            ("o uv", key(Some(cropped), 24.0, BAR)),
            ("a imagem pronta depois de carregar", key(None, 24.0, BAR)),
        ] {
            assert_ne!(changed, base, "{what} deveria mudar a chave");
        }
    }

    /// A chave **não** depende da config do acrílico além do raio: ligar o
    /// efeito, o que está no terminal ou o tamanho da janela (que a surface
    /// leva em conta) não a mexem.
    #[test]
    fn the_key_is_the_same_whatever_the_terminal_is_doing() {
        let first = blur_source(&active(), BAR, Some(image(1, 1.0)), window()).unwrap();
        let mut other = active();
        other.terminal.background_opacity = 0.3; // tingimento: não é da fonte
        other.terminal.font.size = 20.0;
        let second = blur_source(&other, BAR, Some(image(1, 1.0)), window()).unwrap();
        assert_eq!(first.key, second.key);
    }

    #[test]
    fn creating_the_block_emits_its_reason() {
        for from in [AcrylicState::Disabled, AcrylicState::Active] {
            assert_eq!(
                block_notice_due(from, OPACITY),
                Some(AcrylicBlock::WindowOpacity)
            );
            assert_eq!(
                block_notice_due(from, NO_IMAGE),
                Some(AcrylicBlock::NoWindowImage)
            );
        }
    }

    #[test]
    fn changing_the_reason_emits_the_new_one() {
        assert_eq!(
            block_notice_due(OPACITY, NO_IMAGE),
            Some(AcrylicBlock::NoWindowImage)
        );
        assert_eq!(
            block_notice_due(NO_IMAGE, OPACITY),
            Some(AcrylicBlock::WindowOpacity)
        );
    }

    #[test]
    fn staying_blocked_for_the_same_reason_is_silent() {
        assert_eq!(block_notice_due(OPACITY, OPACITY), None);
        assert_eq!(block_notice_due(NO_IMAGE, NO_IMAGE), None);
    }

    #[test]
    fn leaving_the_block_is_silent() {
        for to in [AcrylicState::Disabled, AcrylicState::Active] {
            assert_eq!(block_notice_due(OPACITY, to), None);
            assert_eq!(block_notice_due(NO_IMAGE, to), None);
        }
        assert_eq!(
            block_notice_due(AcrylicState::Disabled, AcrylicState::Active),
            None
        );
        assert_eq!(
            block_notice_due(AcrylicState::Active, AcrylicState::Disabled),
            None
        );
    }

    #[test]
    fn a_block_that_comes_back_after_a_clear_spell_emits_again() {
        // Bloqueado -> ativo -> bloqueado: cada recarga que cria o problema
        // avisa (RF-19.12), mesmo já tendo avisado antes.
        assert_eq!(block_notice_due(OPACITY, AcrylicState::Active), None);
        assert_eq!(
            block_notice_due(AcrylicState::Active, OPACITY),
            Some(AcrylicBlock::WindowOpacity)
        );
    }

    fn config(enabled: bool, window_opacity: f64, window_image: &str) -> Config {
        let mut config = Config::default();
        config.terminal.acrylic.enabled = enabled;
        config.appearance.window.opacity = window_opacity;
        config.appearance.window.background_image.path = window_image.to_owned();
        config
    }

    #[test]
    fn startup_with_the_default_config_is_silent() {
        assert!(notices(&Config::default(), &Config::default()).is_empty());
    }

    #[test]
    fn startup_blocked_by_opacity_warns_once_with_that_reason() {
        let current = config(true, 0.9, "fundo.png");
        assert_eq!(
            notices(&Config::default(), &current),
            [AcrylicNotice::Blocked(AcrylicBlock::WindowOpacity)]
        );
    }

    #[test]
    fn startup_with_both_conditions_missing_names_the_opacity_only() {
        let current = config(true, 0.9, "");
        assert_eq!(
            notices(&Config::default(), &current),
            [AcrylicNotice::Blocked(AcrylicBlock::WindowOpacity)]
        );
    }

    #[test]
    fn startup_blocked_by_the_missing_image_names_the_image() {
        let current = config(true, 1.0, "");
        assert_eq!(
            notices(&Config::default(), &current),
            [AcrylicNotice::Blocked(AcrylicBlock::NoWindowImage)]
        );
    }

    #[test]
    fn startup_with_the_effect_active_is_silent() {
        assert!(notices(&Config::default(), &config(true, 1.0, "fundo.png")).is_empty());
    }

    #[test]
    fn disabled_with_a_missing_condition_is_silent() {
        assert!(notices(&Config::default(), &config(false, 0.5, "")).is_empty());
    }

    #[test]
    fn a_reload_that_lowers_the_opacity_warns_and_the_next_identical_one_does_not() {
        let active = config(true, 1.0, "fundo.png");
        let blocked = config(true, 0.9, "fundo.png");
        assert_eq!(
            notices(&active, &blocked),
            [AcrylicNotice::Blocked(AcrylicBlock::WindowOpacity)]
        );
        assert!(notices(&blocked, &blocked).is_empty());
    }

    #[test]
    fn a_reload_that_turns_the_effect_on_into_a_block_warns() {
        let off = config(false, 0.9, "fundo.png");
        let on = config(true, 0.9, "fundo.png");
        assert_eq!(
            notices(&off, &on),
            [AcrylicNotice::Blocked(AcrylicBlock::WindowOpacity)]
        );
    }

    #[test]
    fn a_reload_that_swaps_the_reason_warns_with_the_new_one() {
        let no_image = config(true, 1.0, "");
        let translucent = config(true, 0.8, "");
        assert_eq!(
            notices(&no_image, &translucent),
            [AcrylicNotice::Blocked(AcrylicBlock::WindowOpacity)]
        );
    }

    fn with_blur(enabled: bool, blur: f32) -> Acrylic {
        Acrylic { enabled, blur }
    }

    #[test]
    fn blur_in_range_never_warns() {
        let default = Acrylic::default();
        assert!(!blur_notice_due(&default, &with_blur(true, 1.0)));
        assert!(!blur_notice_due(&default, &with_blur(true, 64.0)));
        assert!(!blur_notice_due(&default, &with_blur(true, 24.0)));
    }

    #[test]
    fn blur_out_of_range_warns_when_the_effect_is_on() {
        let default = Acrylic::default();
        for bad in [0.0, -5.0, 0.5, 64.5, 100.0, f32::NAN, f32::INFINITY] {
            assert!(blur_notice_due(&default, &with_blur(true, bad)), "{bad}");
        }
    }

    #[test]
    fn blur_out_of_range_is_silent_while_the_effect_is_off() {
        assert!(!blur_notice_due(
            &Acrylic::default(),
            &with_blur(false, 100.0)
        ));
    }

    #[test]
    fn turning_the_effect_on_with_a_bad_blur_warns() {
        assert!(blur_notice_due(
            &with_blur(false, 100.0),
            &with_blur(true, 100.0)
        ));
    }

    #[test]
    fn the_same_bad_blur_is_not_warned_twice() {
        let bad = with_blur(true, 100.0);
        assert!(!blur_notice_due(&bad, &bad));
        let nan = with_blur(true, f32::NAN);
        assert!(!blur_notice_due(&nan, &nan));
    }

    #[test]
    fn a_new_bad_blur_warns_again() {
        assert!(blur_notice_due(
            &with_blur(true, 100.0),
            &with_blur(true, 200.0)
        ));
        // Voltou à faixa, e quebrou de novo.
        assert!(blur_notice_due(
            &with_blur(true, 24.0),
            &with_blur(true, 100.0)
        ));
    }

    #[test]
    fn a_bad_blur_comes_after_the_block_notice() {
        let mut current = config(true, 0.9, "fundo.png");
        current.terminal.acrylic.blur = 100.0;
        assert_eq!(
            notices(&Config::default(), &current),
            [
                AcrylicNotice::Blocked(AcrylicBlock::WindowOpacity),
                AcrylicNotice::BlurAdjusted(current.terminal.acrylic.clone()),
            ]
        );
    }

    #[test]
    fn each_block_reason_has_its_own_phrase_in_every_language() {
        for locale in ["pt_BR", "en_US", "es_ES", "fr_FR", "de_DE"] {
            let catalog = test_support::catalog(locale);
            let (severity, title, opacity) =
                AcrylicNotice::Blocked(AcrylicBlock::WindowOpacity).compose(&catalog);
            let (_, same_title, image) =
                AcrylicNotice::Blocked(AcrylicBlock::NoWindowImage).compose(&catalog);
            assert_eq!(severity, Severity::Warning);
            assert_eq!(title, same_title);
            assert_ne!(opacity, image, "{locale}");
            for text in [&title, &opacity, &image] {
                assert!(!text.is_empty(), "{locale}");
                assert!(!text.starts_with("notice."), "{locale}: {text}");
            }
        }
    }

    #[test]
    fn the_notice_texts_in_portuguese() {
        let catalog = test_support::pt_br();
        let (_, title, body) =
            AcrylicNotice::Blocked(AcrylicBlock::WindowOpacity).compose(&catalog);
        assert_eq!(title, "Efeito acrílico não aplicado");
        assert!(body.starts_with("Opacidade da janela"), "{body}");
        let (_, _, body) = AcrylicNotice::Blocked(AcrylicBlock::NoWindowImage).compose(&catalog);
        assert!(body.starts_with("Sem imagem de fundo da janela"), "{body}");
        assert!(!body.contains("guia"), "terminologia: {body}");
    }

    #[test]
    fn the_blur_notice_names_the_key_the_range_and_the_value_used() {
        let catalog = test_support::pt_br();
        let (severity, title, body) =
            AcrylicNotice::BlurAdjusted(with_blur(true, 100.0)).compose(&catalog);
        assert_eq!(severity, Severity::Info);
        assert_eq!(title, "Desfoque do acrílico ajustado");
        assert_eq!(
            body,
            "Fora da faixa de 1 a 64: \"[terminal.acrylic] blur\" usa 64."
        );

        let (_, _, body) = AcrylicNotice::BlurAdjusted(with_blur(true, f32::NAN)).compose(&catalog);
        assert!(body.ends_with("usa 24."), "{body}");
    }

    #[test]
    fn the_blur_notice_in_english() {
        let catalog = test_support::en_us();
        let (_, title, body) = AcrylicNotice::BlurAdjusted(with_blur(true, 0.0)).compose(&catalog);
        assert_eq!(title, "Acrylic blur adjusted");
        assert_eq!(
            body,
            "Out of range 1 to 64: \"[terminal.acrylic] blur\" uses 1."
        );
    }
}
