// SPDX-License-Identifier: GPL-3.0-or-later

//! A alternância do canvas (espec. §1.5, §2.12): trilho 34×19 de raio 10,
//! `padding: 2`, botão circular 15×15 deslocado 15px quando ligado. Nasceu na
//! barra de busca (ADR-0041 §6, o alternador de expressão regular) e passou a
//! um módulo compartilhado quando a janela de configurações virou o segundo
//! consumidor (ADR-0059 §4) -- uma só geometria, desenhada por um só código.
//!
//! As cores do trilho são de quem chama: a barra de busca as tira do tema
//! (`editor_input_border_focus`/`editor_border`) e a janela de configurações
//! usa as do canvas (ADR-0060 §3). Só o botão é fixo -- convenção comum de
//! manter o indicador sempre claro, qualquer que seja a cor do trilho.
//!
//! Sem a transição de `.15s` do canvas: ela não entra pela lista fechada de
//! consumidores do relógio de animação (ADR-0022, ADR-0060 §4).

use porecatu_render::{Color, Primitive, Rect, RoundedQuad};

use crate::palette;

pub(crate) const TOGGLE_TRACK_WIDTH: f32 = 34.0;
pub(crate) const TOGGLE_TRACK_HEIGHT: f32 = 19.0;
const TOGGLE_TRACK_RADIUS: f32 = 10.0;
const TOGGLE_TRACK_PADDING: f32 = 2.0;
const TOGGLE_KNOB_SIZE: f32 = 15.0;
pub(crate) const TOGGLE_KNOB_COLOR: Color = palette::hex(0xf0, 0xf3, 0xf6);

/// Desenha o trilho e o botão em `rect` (que deve ter o tamanho
/// `TOGGLE_TRACK_WIDTH` × `TOGGLE_TRACK_HEIGHT`).
pub(crate) fn push_toggle(
    rect: Rect,
    on: bool,
    on_color: Color,
    off_color: Color,
    out: &mut Vec<Primitive>,
) {
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: TOGGLE_TRACK_RADIUS,
        color: if on { on_color } else { off_color },
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));
    let knob_x = if on {
        rect.x + rect.width - TOGGLE_TRACK_PADDING - TOGGLE_KNOB_SIZE
    } else {
        rect.x + TOGGLE_TRACK_PADDING
    };
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: Rect {
            x: knob_x,
            y: rect.y + (rect.height - TOGGLE_KNOB_SIZE) / 2.0,
            width: TOGGLE_KNOB_SIZE,
            height: TOGGLE_KNOB_SIZE,
        },
        radius: TOGGLE_KNOB_SIZE / 2.0,
        color: TOGGLE_KNOB_COLOR,
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: Color = palette::hex(0x3f, 0x8f, 0x80);
    const OFF: Color = palette::hex(0x2a, 0x30, 0x38);

    fn rect() -> Rect {
        Rect {
            x: 10.0,
            y: 20.0,
            width: TOGGLE_TRACK_WIDTH,
            height: TOGGLE_TRACK_HEIGHT,
        }
    }

    fn knob_x(on: bool) -> f32 {
        let mut out = Vec::new();
        push_toggle(rect(), on, ON, OFF, &mut out);
        let Primitive::RoundedQuad(knob) = &out[1] else {
            panic!("o segundo primitivo é o botão");
        };
        knob.rect.x
    }

    #[test]
    fn the_knob_moves_fifteen_pixels_when_on() {
        assert_eq!(knob_x(true) - knob_x(false), 15.0);
    }

    #[test]
    fn the_track_takes_the_color_of_the_state() {
        let mut out = Vec::new();
        push_toggle(rect(), true, ON, OFF, &mut out);
        let Primitive::RoundedQuad(track) = &out[0] else {
            panic!()
        };
        assert_eq!(track.color, ON);
        let mut out = Vec::new();
        push_toggle(rect(), false, ON, OFF, &mut out);
        let Primitive::RoundedQuad(track) = &out[0] else {
            panic!()
        };
        assert_eq!(track.color, OFF);
    }

    #[test]
    fn the_knob_stays_inside_the_track_at_both_ends() {
        for on in [false, true] {
            let x = knob_x(on);
            assert!(x >= rect().x && x + TOGGLE_KNOB_SIZE <= rect().x + rect().width);
        }
    }
}
