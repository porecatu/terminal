// SPDX-License-Identifier: GPL-3.0-or-later

//! A janela de configurações (PRD-016, ADR-0059, ADR-0060): a primeira janela
//! do app que **não** é um workspace de terminal. Vive em `App.settings`,
//! fora de `App.windows` (ADR-0059 §1) -- nenhum handler de janela de
//! terminal a conhece.
//!
//! Nesta etapa é só o esqueleto: janela, cabeçalho, guia e painel vazios, e o
//! ciclo de vida (singleton, centrada sobre a janela de origem, fechada sem
//! encerrar o app). Itens, opções, rodapé, acessibilidade e pendências entram
//! com as tarefas seguintes.

mod actions;
mod catalog;
mod choice_list;
pub(crate) mod closing;
mod content;
mod draft;
mod field_edit;
mod file_state;
mod interact;
mod layout;
mod paint;
mod phrases;
pub(crate) mod save;
mod shortcuts;
mod window;

use porecatu_config::Config;
use porecatu_render::Color;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::window::WindowAttributes;

pub(crate) use catalog::Group;
pub(crate) use content::{BannerView, ControlView, RowView};
#[cfg(test)]
pub(crate) use content::{ChipTone, ChipView, ListItemView};
pub(crate) use file_state::Disk;
pub(crate) use layout::{FOOTER_BUTTONS, FooterButton, Layout};
pub(crate) use window::{DialogAnswer, Env, Press, SettingsWindow};

use crate::palette;

/// Trilho da alternância ligada, `#3f8f80` (espec. §1.5, ADR-0060 §3), e o
/// fundo do botão Salvar -- "o par ligado da alternância" (ADR-0060 §4).
pub(crate) const TOGGLE_ON: Color = palette::hex(0x3f, 0x8f, 0x80);
/// Trilho da alternância desligada, `#2a3038` (espec. §1.5, ADR-0060 §3).
pub(crate) const TOGGLE_OFF: Color = palette::hex(0x2a, 0x30, 0x38);

/// Ícone do botão de restaurar padrão: `#727a86` (ADR-0060 §2, o do botão de
/// fechar da aba, espec. §1.7 e §2.14). `ResolvedPalette` o carrega com outro
/// nome (`tab_exited_text`); aqui o nome diz para que ele serve.
pub(crate) const RESTORE_ICON: Color = palette::hex(0x72, 0x7a, 0x86);
/// Fundo do botão de restaurar sob o cursor: `#39404b` (ADR-0060 §2, espec.
/// §1.7).
pub(crate) const RESTORE_HOVER_BACKGROUND: Color = palette::hex(0x39, 0x40, 0x4b);
/// Ícone do botão de restaurar sob o cursor: `#e4e8ee` (ADR-0060 §2).
pub(crate) const RESTORE_HOVER_ICON: Color = palette::hex(0xe4, 0xe8, 0xee);
/// Raio do botão de restaurar: 4px, o do botão de fechar da aba (espec. §1.7).
pub(crate) const RESTORE_RADIUS: f32 = 4.0;

/// Tamanho do título do cabeçalho: o token "título do painel de
/// configurações" `15px / 500` (espec. §1.1, ADR-0060 §1).
pub(crate) const TITLE_SIZE_PX: f32 = 15.0;

/// Espaço entre o ícone e o título do cabeçalho: o `gap: 8` do ADR-0060 §1.
pub(crate) const HEADER_GAP_PX: f32 = 8.0;

/// Posição física de uma janela de `size` centrada sobre a de origem
/// (`origin_position`/`origin_size`), recortada ao monitor dela quando ele é
/// conhecido -- a mesma garantia da cascata de `window.new`
/// (`cascade_position`), para a janela nunca abrir fora da tela. Pura, para
/// ser testável sem janela.
pub(crate) fn centered_position(
    origin_position: (i32, i32),
    origin_size: (u32, u32),
    size: (u32, u32),
    monitor: Option<((i32, i32), (u32, u32))>,
) -> (i32, i32) {
    let mut x = origin_position.0 + (origin_size.0 as i32 - size.0 as i32) / 2;
    let mut y = origin_position.1 + (origin_size.1 as i32 - size.1 as i32) / 2;
    if let Some((mon_pos, mon_size)) = monitor {
        let max_x = mon_pos.0 + mon_size.0 as i32 - size.0 as i32;
        let max_y = mon_pos.1 + mon_size.1 as i32 - size.1 as i32;
        x = x.clamp(mon_pos.0, max_x.max(mon_pos.0));
        y = y.clamp(mon_pos.1, max_y.max(mon_pos.1));
    }
    (x, y)
}

/// Atributos da janela de configurações: os comuns a toda janela do app
/// (`base_window_attributes`: sem decoração nativa fora do macOS, ADR-0027)
/// mais título, tamanho e mínimo, lidos de `[appearance.settings]` (classe C:
/// valem na próxima abertura). `position` já vem centrada sobre a origem
/// (`centered_position`); `None` deixa o sistema escolher.
pub(crate) fn window_attributes(
    title: &str,
    position: Option<(i32, i32)>,
    config: &Config,
) -> WindowAttributes {
    let settings = &config.appearance.settings;
    #[allow(unused_mut)]
    let mut attributes = crate::base_window_attributes(false)
        .with_title(title)
        .with_inner_size(LogicalSize::new(
            settings.window_width as f32,
            settings.window_height as f32,
        ))
        .with_min_inner_size(LogicalSize::new(
            settings.min_width as f32,
            settings.min_height as f32,
        ));
    // macOS: decoração nativa **com** título (ADR-0059 §2) -- diferente da
    // janela de terminal, que esconde o título e estende o conteúdo por
    // baixo da barra para pôr as abas ali. Esta não tem barra nossa.
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attributes = attributes
            .with_title_hidden(false)
            .with_titlebar_transparent(false)
            .with_fullsize_content_view(false);
    }
    if let Some((x, y)) = position {
        attributes = attributes.with_position(PhysicalPosition::new(x, y));
    }
    attributes
}

/// As linhas de opção de `group`, montadas sobre o `Config` padrão e o
/// catálogo pt_BR, para os testes da árvore de acessibilidade.
#[cfg(test)]
pub(crate) fn rows_for_test(group: Group) -> Vec<RowView> {
    use porecatu_render::TextMeasurer;

    let config = porecatu_config::Config::default();
    let metrics = layout::Metrics::from_config(&config, 52.0);
    let key = content::ContentKey {
        group,
        panel_width_bits: (config.appearance.settings.window_width as f32 - metrics.sidebar_width)
            .to_bits(),
        generation: 0,
    };
    content::build(
        key,
        &draft::Draft::new(&config),
        &content::ViewExtras::default(),
        &crate::messages::test_support::pt_br(),
        &metrics,
        &mut TextMeasurer::new(),
    )
    .blocks
    .into_iter()
    .filter_map(|block| match block {
        content::Block::Row(row) => Some(*row),
        content::Block::Section(_)
        | content::Block::Note { .. }
        | content::Block::Filter { .. } => None,
    })
    .collect()
}

/// Um `Layout` de tamanho fixo para os testes da árvore de acessibilidade,
/// com ou sem o cabeçalho nosso (sem ele é o macOS).
#[cfg(test)]
pub(crate) fn layout_for_test(with_header: bool) -> Layout {
    let settings = porecatu_config::Config::default().appearance.settings;
    layout::layout(
        settings.window_width as f32,
        settings.window_height as f32,
        if with_header { 52.0 } else { 0.0 },
        settings.sidebar_width as f32,
        54.0,
    )
}

/// Uma faixa montada sobre o `Config` padrão e o catálogo pt_BR, para os testes
/// da árvore de acessibilidade: a de arquivo inválido (`invalid`) ou a de
/// conflito.
#[cfg(test)]
pub(crate) fn content_banner_for_test(invalid: bool) -> BannerView {
    use porecatu_render::TextMeasurer;

    let config = porecatu_config::Config::default();
    let metrics = layout::Metrics::from_config(&config, 52.0);
    let banner = if invalid {
        file_state::Banner::Invalid(
            porecatu_config::parse(
                "[terminal.font
",
            )
            .unwrap_err(),
        )
    } else {
        file_state::Banner::Conflict
    };
    let key = content::ContentKey {
        group: Group::General,
        panel_width_bits: 700.0_f32.to_bits(),
        generation: 0,
    };
    content::build(
        key,
        &draft::Draft::new(&config),
        &content::ViewExtras {
            banner: Some(&banner),
            ..content::ViewExtras::default()
        },
        &crate::messages::test_support::pt_br(),
        &metrics,
        &mut TextMeasurer::new(),
    )
    .banner
    .expect("a faixa")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_over_the_origin_window() {
        assert_eq!(
            centered_position((100, 100), (1000, 800), (900, 640), None),
            (150, 180)
        );
    }

    #[test]
    fn a_window_larger_than_the_origin_is_centered_outward() {
        assert_eq!(
            centered_position((100, 100), (400, 300), (900, 640), None),
            (-150, -70)
        );
    }

    #[test]
    fn is_clamped_to_the_monitor_of_the_origin() {
        // Origem colada ao canto inferior direito de um monitor 1920x1080: a
        // janela centrada vazaria; fica presa dentro dele.
        let monitor = Some(((0, 0), (1920, 1080)));
        assert_eq!(
            centered_position((1500, 900), (400, 300), (900, 640), monitor),
            (1020, 440)
        );
        // E no canto oposto, o recorte é pelo lado de lá.
        assert_eq!(
            centered_position((0, 0), (200, 200), (900, 640), monitor),
            (0, 0)
        );
    }

    #[test]
    fn respects_a_monitor_that_does_not_start_at_the_origin() {
        let monitor = Some(((-1920, 0), (1920, 1080)));
        let (x, y) = centered_position((-1900, 20), (300, 300), (900, 640), monitor);
        assert!(x >= -1920 && x + 900 <= 0);
        assert!(y >= 0 && y + 640 <= 1080);
    }

    #[test]
    fn a_monitor_smaller_than_the_window_pins_to_its_corner() {
        let monitor = Some(((10, 20), (500, 400)));
        assert_eq!(
            centered_position((10, 20), (500, 400), (900, 640), monitor),
            (10, 20)
        );
    }
}
