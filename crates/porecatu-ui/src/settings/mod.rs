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
mod draft;
mod layout;
mod phrases;
mod window;

use porecatu_render::Color;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::window::WindowAttributes;

pub(crate) use catalog::Group;
pub(crate) use layout::{FOOTER_BUTTONS, FooterButton, Layout};
pub(crate) use window::{Press, SettingsWindow};

use crate::palette;

// Tamanhos provisórios, com os valores propostos no ADR-0060 §5. Cada um
// vira chave de `[appearance.settings]` na tarefa 08, junto com o arquivo de
// exemplo e a especificação visual (`verify-docs.py` cobra os três lados).

/// Tamanho de abertura, em pixels lógicos.
// [appearance.settings] window_width — ADR-0060 §5, chave entra na tarefa 08
pub(crate) const WINDOW_WIDTH: f32 = 900.0;
// [appearance.settings] window_height — ADR-0060 §5, chave entra na tarefa 08
pub(crate) const WINDOW_HEIGHT: f32 = 640.0;
/// Tamanho mínimo, em pixels lógicos.
// [appearance.settings] min_width — ADR-0060 §5, chave entra na tarefa 08
pub(crate) const MIN_WIDTH: f32 = 640.0;
// [appearance.settings] min_height — ADR-0060 §5, chave entra na tarefa 08
pub(crate) const MIN_HEIGHT: f32 = 420.0;
/// Largura da guia lateral.
// [appearance.settings] sidebar_width — ADR-0060 §5, chave entra na tarefa 08
pub(crate) const SIDEBAR_WIDTH: f32 = 200.0;

/// Fundo do painel: o token "Drawer" `#171b21` (espec. §1.2, "painel de
/// configurações"), citado pelo ADR-0060 §1. É a única cor da janela que
/// `ResolvedPalette` não carrega: cabeçalho e guia usam `bar_background`
/// (`#1b1f26`), o separador `editor_divider` (`#2a2f38`) e o título
/// `dialog_title_text` (`#e6eaef`).
pub(crate) const PANEL_BACKGROUND: Color = palette::hex(0x17, 0x1b, 0x21);

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
/// mais título, tamanho e mínimo. `position` já vem centrada sobre a origem
/// (`centered_position`); `None` deixa o sistema escolher.
pub(crate) fn window_attributes(title: &str, position: Option<(i32, i32)>) -> WindowAttributes {
    #[allow(unused_mut)]
    let mut attributes = crate::base_window_attributes()
        .with_title(title)
        .with_inner_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
        .with_min_inner_size(LogicalSize::new(MIN_WIDTH, MIN_HEIGHT));
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

/// Um `Layout` de tamanho fixo para os testes da árvore de acessibilidade,
/// com ou sem o cabeçalho nosso (sem ele é o macOS).
#[cfg(test)]
pub(crate) fn layout_for_test(with_header: bool) -> Layout {
    layout::layout(
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
        if with_header { 52.0 } else { 0.0 },
        SIDEBAR_WIDTH,
    )
}
