// SPDX-License-Identifier: GPL-3.0-or-later

//! Non-client area da janela (ADR-0027): a borda de resize da janela
//! inteira, não só a faixa de 52px do topo -- essa é `tab_bar`/`chrome`.
//! Hit-test de botão de janela e da zona de arrasto continuam em
//! `tab_bar::window_button_rect`/`point_in_window_button` (fazem parte da
//! geometria da barra); este módulo cobre só o que a barra NÃO cobre.
//!
//! Puro e testável sem `Window` real, mesma cultura de teste do resto do
//! crate -- a chamada de verdade a `Window::drag_resize_window` mora em
//! `lib.rs`, fora daqui.

use winit::window::ResizeDirection;

/// Qual borda/canto da janela um ponto em coordenadas lógicas de janela
/// atinge, se algum. `None` fora de toda borda -- inclusive dentro da
/// barra de abas, mas isto não filtra por `y` da barra: quem chama decide
/// se o ponto está numa região onde resize faz sentido (ex.: não checar
/// isto para pontos dentro de um botão da barra).
///
/// `resize_border_px`: `[appearance.window_controls] resize_border` --
/// espessura da zona de resize em cada borda, em pixels lógicos. Não vem
/// de nenhum token de design (a espec nunca cobriu resize sem decoração
/// nativa, era `[v2]` até o ADR-0027) -- 6px é o valor comum em apps sem
/// decoração (Electron/Chromium), ponto de partida razoável, não medido.
///
/// Janela maximizada nunca tem borda de resize, em nenhuma plataforma --
/// `is_maximized` desliga a função inteira.
pub fn resize_direction_at(
    point: (f32, f32),
    window_width: f32,
    window_height: f32,
    is_maximized: bool,
    resize_border_px: f32,
) -> Option<ResizeDirection> {
    if is_maximized {
        return None;
    }
    let (x, y) = point;
    let left = x < resize_border_px;
    let right = x > window_width - resize_border_px;
    let top = y < resize_border_px;
    let bottom = y > window_height - resize_border_px;
    match (top, bottom, left, right) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (true, _, _, true) => Some(ResizeDirection::NorthEast),
        (_, true, true, _) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::North),
        (_, true, _, _) => Some(ResizeDirection::South),
        (_, _, true, _) => Some(ResizeDirection::West),
        (_, _, _, true) => Some(ResizeDirection::East),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 800.0;
    const H: f32 = 600.0;
    /// `[appearance.window_controls] resize_border`, default do TOML.
    const BORDER: f32 = 6.0;

    #[test]
    fn point_away_from_every_border_is_none() {
        assert_eq!(
            resize_direction_at((400.0, 300.0), W, H, false, BORDER),
            None
        );
    }

    #[test]
    fn maximized_window_has_no_resize_border() {
        assert_eq!(resize_direction_at((0.0, 0.0), W, H, true, BORDER), None);
        assert_eq!(resize_direction_at((W, H), W, H, true, BORDER), None);
    }

    #[test]
    fn each_straight_edge_resolves_to_its_direction() {
        assert_eq!(
            resize_direction_at((400.0, 0.0), W, H, false, BORDER),
            Some(ResizeDirection::North)
        );
        assert_eq!(
            resize_direction_at((400.0, H), W, H, false, BORDER),
            Some(ResizeDirection::South)
        );
        assert_eq!(
            resize_direction_at((0.0, 300.0), W, H, false, BORDER),
            Some(ResizeDirection::West)
        );
        assert_eq!(
            resize_direction_at((W, 300.0), W, H, false, BORDER),
            Some(ResizeDirection::East)
        );
    }

    #[test]
    fn corners_take_priority_over_the_straight_edge() {
        assert_eq!(
            resize_direction_at((0.0, 0.0), W, H, false, BORDER),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_direction_at((W, 0.0), W, H, false, BORDER),
            Some(ResizeDirection::NorthEast)
        );
        assert_eq!(
            resize_direction_at((0.0, H), W, H, false, BORDER),
            Some(ResizeDirection::SouthWest)
        );
        assert_eq!(
            resize_direction_at((W, H), W, H, false, BORDER),
            Some(ResizeDirection::SouthEast)
        );
    }

    #[test]
    fn tiny_window_still_resolves_a_corner_without_panicking() {
        // largura/altura menor que o dobro da borda -- as faixas de borda
        // se sobrepõem, mas a função não deve entrar em pânico nem
        // devolver algo fora do enum.
        let tiny = BORDER; // == largura/altura da janela
        assert_eq!(
            resize_direction_at((0.0, 0.0), tiny, tiny, false, BORDER),
            Some(ResizeDirection::NorthWest)
        );
    }
}
