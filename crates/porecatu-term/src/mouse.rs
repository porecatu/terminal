// SPDX-License-Identifier: GPL-3.0-or-later

//! Codificação do reporte de mouse ao programa (PRD-010 RF-10.1 a RF-10.3,
//! ADR-0013): modos 1000/1002/1003, encoding SGR (1006, preferido) ou X10
//! (fallback, quando o programa não negociou 1006). Função pura, só
//! depende de [`TermModes`] -- `porecatu-ui` decide, a partir da posição
//! do cursor e de `Shift`, se um evento de mouse chega até aqui ou vira
//! seleção local (a regra de conflito do ADR-0013 mora em `porecatu-ui`,
//! que é quem sabe de `Shift` e de pixel; aqui só existe protocolo).

use crate::keys::Modifiers;
use crate::snapshot::{MouseReporting, TermModes};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    /// Nenhum botão pressionado -- só faz sentido com `MouseAction::Motion`
    /// (modo 1003, `AnyMotion`: movimento reportado mesmo sem clique).
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    /// Movimento -- com botão pressionado (modo 1002) ou sem (modo 1003).
    Motion,
}

/// Reporta um evento de mouse ao programa, se o modo pedir. `None` quando
/// o programa não pediu esse tipo de evento (ex.: `Motion` sem botão fora
/// do modo `AnyMotion`) -- quem chama decide o que fazer nesse caso (RF-10:
/// sobra pra seleção local ou pro scrollback).
///
/// `col`/`row` são 0-based (convenção do resto do crate); o protocolo é
/// 1-based, a conversão acontece aqui.
pub fn encode_mouse_report(
    button: MouseButton,
    action: MouseAction,
    col: usize,
    row: usize,
    mods: Modifiers,
    modes: &TermModes,
) -> Option<Vec<u8>> {
    if modes.mouse_reporting == MouseReporting::None {
        return None;
    }
    // Modo 1000 (Click) não reporta movimento nenhum; modo 1002
    // (ClickAndDrag) só com botão pressionado; só `AnyMotion` (1003)
    // aceita movimento sem nenhum botão.
    if action == MouseAction::Motion {
        let no_button = button == MouseButton::None;
        let blocked = modes.mouse_reporting == MouseReporting::Click
            || (modes.mouse_reporting == MouseReporting::ClickAndDrag && no_button);
        if blocked {
            return None;
        }
    }

    let is_motion = action == MouseAction::Motion;
    let mut code = button_code(button, is_motion)?;
    if mods.shift {
        code += 4;
    }
    if mods.alt {
        code += 8;
    }
    if mods.ctrl {
        code += 16;
    }

    let col = col + 1;
    let row = row + 1;

    if modes.sgr_mouse {
        let suffix = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        Some(format!("\x1b[<{code};{col};{row}{suffix}").into_bytes())
    } else {
        // X10 legado (ADR-0013): um byte por campo, sem parâmetro --
        // limite de 223 colunas/linhas, é o próprio motivo do SGR ser
        // preferido. Fora do alcance, sem reporte -- silencioso, não é
        // erro do usuário.
        if col > 223 || row > 223 {
            return None;
        }
        // Soltar não distingue o botão no X10 clássico.
        let cb: u16 = if action == MouseAction::Release {
            3
        } else {
            code
        };
        Some(vec![
            0x1b,
            b'[',
            b'M',
            (32 + cb) as u8,
            (32 + col) as u8,
            (32 + row) as u8,
        ])
    }
}

fn button_code(button: MouseButton, is_motion: bool) -> Option<u16> {
    let base = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        // Convenção xterm: "nenhum botão" reusa o código 3 (o mesmo de
        // "soltar" no X10 legado) -- só válido em movimento puro.
        MouseButton::None if is_motion => 3,
        MouseButton::None => return None,
        // Roda: reportada como "botão" 64/65 (ADR-0013 tabela de modos
        // honrados) -- nunca em modo `Motion`.
        MouseButton::WheelUp if !is_motion => 64,
        MouseButton::WheelDown if !is_motion => 65,
        MouseButton::WheelUp | MouseButton::WheelDown => return None,
    };
    Some(if is_motion { base + 32 } else { base })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(reporting: MouseReporting, sgr: bool) -> TermModes {
        TermModes {
            mouse_reporting: reporting,
            sgr_mouse: sgr,
            ..TermModes::default()
        }
    }

    #[test]
    fn sem_modo_de_mouse_nao_reporta_nada() {
        let bytes = encode_mouse_report(
            MouseButton::Left,
            MouseAction::Press,
            0,
            0,
            Modifiers::NONE,
            &modes(MouseReporting::None, true),
        );
        assert_eq!(bytes, None);
    }

    #[test]
    fn clique_sgr_press_e_release() {
        let m = modes(MouseReporting::Click, true);
        assert_eq!(
            encode_mouse_report(
                MouseButton::Left,
                MouseAction::Press,
                4,
                9,
                Modifiers::NONE,
                &m
            ),
            Some(b"\x1b[<0;5;10M".to_vec())
        );
        assert_eq!(
            encode_mouse_report(
                MouseButton::Left,
                MouseAction::Release,
                4,
                9,
                Modifiers::NONE,
                &m
            ),
            Some(b"\x1b[<0;5;10m".to_vec())
        );
    }

    #[test]
    fn modo_click_nao_reporta_motion() {
        let m = modes(MouseReporting::Click, true);
        assert_eq!(
            encode_mouse_report(
                MouseButton::Left,
                MouseAction::Motion,
                0,
                0,
                Modifiers::NONE,
                &m
            ),
            None
        );
    }

    #[test]
    fn modo_clickanddrag_reporta_motion_com_botao() {
        let m = modes(MouseReporting::ClickAndDrag, true);
        // codigo do botao esquerdo (0) + 32 (motion) = 32
        assert_eq!(
            encode_mouse_report(
                MouseButton::Left,
                MouseAction::Motion,
                0,
                0,
                Modifiers::NONE,
                &m
            ),
            Some(b"\x1b[<32;1;1M".to_vec())
        );
    }

    #[test]
    fn modificadores_somam_ao_codigo() {
        let m = modes(MouseReporting::Click, true);
        let mods = Modifiers {
            shift: true,
            ctrl: true,
            ..Modifiers::NONE
        };
        // 0 (esquerdo) + 4 (shift) + 16 (ctrl) = 20
        assert_eq!(
            encode_mouse_report(MouseButton::Left, MouseAction::Press, 0, 0, mods, &m),
            Some(b"\x1b[<20;1;1M".to_vec())
        );
    }

    #[test]
    fn x10_usa_um_byte_por_campo() {
        let m = modes(MouseReporting::Click, false);
        let bytes = encode_mouse_report(
            MouseButton::Left,
            MouseAction::Press,
            0,
            0,
            Modifiers::NONE,
            &m,
        )
        .unwrap();
        assert_eq!(bytes, vec![0x1b, b'[', b'M', 32, 33, 33]);
    }

    #[test]
    fn x10_fora_do_alcance_nao_reporta() {
        let m = modes(MouseReporting::Click, false);
        assert_eq!(
            encode_mouse_report(
                MouseButton::Left,
                MouseAction::Press,
                300,
                0,
                Modifiers::NONE,
                &m
            ),
            None
        );
    }

    #[test]
    fn roda_vira_botao_64_65() {
        let m = modes(MouseReporting::Click, true);
        assert_eq!(
            encode_mouse_report(
                MouseButton::WheelUp,
                MouseAction::Press,
                0,
                0,
                Modifiers::NONE,
                &m
            ),
            Some(b"\x1b[<64;1;1M".to_vec())
        );
        assert_eq!(
            encode_mouse_report(
                MouseButton::WheelDown,
                MouseAction::Press,
                0,
                0,
                Modifiers::NONE,
                &m
            ),
            Some(b"\x1b[<65;1;1M".to_vec())
        );
    }

    #[test]
    fn movimento_puro_sem_botao_so_no_modo_anymotion() {
        let click_and_drag = modes(MouseReporting::ClickAndDrag, true);
        assert_eq!(
            encode_mouse_report(
                MouseButton::None,
                MouseAction::Motion,
                0,
                0,
                Modifiers::NONE,
                &click_and_drag
            ),
            None,
            "modo 1002 nao reporta movimento sem botao"
        );

        let any_motion = modes(MouseReporting::AnyMotion, true);
        assert_eq!(
            encode_mouse_report(
                MouseButton::None,
                MouseAction::Motion,
                0,
                0,
                Modifiers::NONE,
                &any_motion
            ),
            // codigo 3 (nenhum botao) + 32 (motion) = 35
            Some(b"\x1b[<35;1;1M".to_vec())
        );
    }
}
