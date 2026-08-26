// SPDX-License-Identifier: GPL-3.0-or-later

//! Codificação de teclas para o terminal (ADR-0008). Funções puras: só
//! dependem de [`TermModes`], nada de estado do motor e nada de `winit`.
//! `porecatu-ui` traduz o evento de teclado da GUI para [`TermKey`] /
//! [`Modifiers`] e chama [`encode_key`] -- é o passo 3 da cadeia de
//! resolução do ADR-0008 ("codifica em bytes e escreve no PTY"), depois
//! que o modo de captura (F2+) e o keybind de app (F4+) já disseram que
//! não é com eles.

use crate::snapshot::TermModes;

/// Estado dos modificadores no momento da tecla. Tipo próprio -- não é o
/// `ModifiersState` do `winit` (`porecatu-term` não conhece GUI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        super_: false,
    };

    fn is_none(self) -> bool {
        self == Self::NONE
    }

    /// Parâmetro de modificador do CSI/SS3, convenção xterm: `1 + soma de
    /// bits` (shift=1, alt=2, ctrl=4, super=8).
    fn param(self) -> u8 {
        1 + self.shift as u8
            + (self.alt as u8) * 2
            + (self.ctrl as u8) * 4
            + (self.super_ as u8) * 8
    }
}

/// Teclas com codificação própria de terminal -- setas, edição, função.
/// Tecla sem significado de terminal (ex.: `CapsLock`) não pertence aqui;
/// `porecatu-ui` decide o que sobra para [`encode_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermKey {
    Enter,
    Backspace,
    Tab,
    Escape,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// 1..=12.
    Function(u8),
}

/// Codifica uma tecla nomeada em bytes, conforme os modos do terminal --
/// DECCKM muda as setas/Home/End entre `ESC O` e `ESC [` (ADR-0008);
/// modificador vira parâmetro CSI, convenção xterm.
pub fn encode_key(key: TermKey, mods: Modifiers, modes: &TermModes) -> Vec<u8> {
    match key {
        TermKey::Enter => with_alt(b"\r".to_vec(), mods),
        TermKey::Backspace => with_alt(b"\x7f".to_vec(), mods),
        TermKey::Tab => {
            if mods.shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        TermKey::Escape => b"\x1b".to_vec(),
        TermKey::ArrowUp => cursor_key(b'A', mods, modes),
        TermKey::ArrowDown => cursor_key(b'B', mods, modes),
        TermKey::ArrowRight => cursor_key(b'C', mods, modes),
        TermKey::ArrowLeft => cursor_key(b'D', mods, modes),
        TermKey::Home => cursor_key(b'H', mods, modes),
        TermKey::End => cursor_key(b'F', mods, modes),
        TermKey::PageUp => tilde_key(5, mods),
        TermKey::PageDown => tilde_key(6, mods),
        TermKey::Insert => tilde_key(2, mods),
        TermKey::Delete => tilde_key(3, mods),
        TermKey::Function(n) => function_key(n, mods),
    }
}

fn with_alt(mut bytes: Vec<u8>, mods: Modifiers) -> Vec<u8> {
    if mods.alt {
        let mut out = vec![0x1b];
        out.append(&mut bytes);
        out
    } else {
        bytes
    }
}

/// Setas e Home/End: sem modificador, `ESC O <letra>` em DECCKM ou
/// `ESC [ <letra>` fora dele -- é o par que o xterm real emite (TERM
/// anunciado, ADR-0012). Com modificador, xterm sempre usa a forma CSI com
/// parâmetro, mesmo em modo de aplicação.
fn cursor_key(letter: u8, mods: Modifiers, modes: &TermModes) -> Vec<u8> {
    if mods.is_none() {
        let prefix: &[u8] = if modes.app_cursor_keys {
            b"\x1bO"
        } else {
            b"\x1b["
        };
        let mut out = prefix.to_vec();
        out.push(letter);
        out
    } else {
        format!("\x1b[1;{}{}", mods.param(), letter as char).into_bytes()
    }
}

/// PageUp/PageDown/Insert/Delete: `ESC [ N ~`, ou `ESC [ N ; M ~` com
/// modificador.
fn tilde_key(code: u8, mods: Modifiers) -> Vec<u8> {
    if mods.is_none() {
        format!("\x1b[{code}~").into_bytes()
    } else {
        format!("\x1b[{code};{}~", mods.param()).into_bytes()
    }
}

/// F1..F4 usam `SS3` como as setas; F5..F12 usam a forma `~` com código
/// próprio (tabela xterm, com os buracos conhecidos em 16 e 22).
fn function_key(n: u8, mods: Modifiers) -> Vec<u8> {
    match n {
        1..=4 => {
            let letter = b'P' + (n - 1);
            if mods.is_none() {
                vec![0x1b, b'O', letter]
            } else {
                format!("\x1b[1;{}{}", mods.param(), letter as char).into_bytes()
            }
        }
        5..=12 => {
            let code = match n {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                12 => 24,
                _ => unreachable!(),
            };
            tilde_key(code, mods)
        }
        _ => Vec::new(),
    }
}

/// Byte de controle para `Ctrl+<letra>`. `None` quando `c` não tem um byte
/// de controle padrão associado -- quem chama decide o que fazer (hoje,
/// `porecatu-ui` cai para [`encode_text`]).
pub fn encode_ctrl_char(c: char) -> Option<u8> {
    let lower = c.to_ascii_lowercase();
    match lower {
        'a'..='z' => Some((lower as u8) - b'a' + 1),
        '@' => Some(0x00),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        '?' => Some(0x7f),
        _ => None,
    }
}

/// Texto normal -- char já composto por tecla morta/IME, ou ASCII simples.
/// `Alt` prefixa `ESC` (convenção "meta manda escape" do xterm/readline;
/// é o que faz Alt+F/Alt+B navegar por palavra na maioria dos shells).
pub fn encode_text(text: &str, mods: Modifiers) -> Vec<u8> {
    with_alt(text.as_bytes().to_vec(), mods)
}

/// Envolve `text` em bracketed paste (`ESC[200~`/`ESC[201~`) quando o modo
/// está ativo -- nunca opcional quando ativo: é o que impede que uma
/// colagem com quebra de linha seja executada comando a comando
/// (ADR-0008). Sem consumidor ainda nesta etapa -- colar de verdade
/// (clipboard) é Etapa 6; a mecânica já existe e é testável isolada.
pub fn wrap_paste(text: &str, modes: &TermModes) -> Vec<u8> {
    if modes.bracketed_paste {
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.as_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(app_cursor_keys: bool, bracketed_paste: bool) -> TermModes {
        TermModes {
            app_cursor_keys,
            bracketed_paste,
            ..TermModes::default()
        }
    }

    #[test]
    fn seta_sem_decckm_usa_csi() {
        let bytes = encode_key(TermKey::ArrowUp, Modifiers::NONE, &modes(false, false));
        assert_eq!(bytes, b"\x1b[A");
    }

    #[test]
    fn seta_com_decckm_usa_ss3() {
        let bytes = encode_key(TermKey::ArrowUp, Modifiers::NONE, &modes(true, false));
        assert_eq!(bytes, b"\x1bOA");
    }

    #[test]
    fn seta_com_modificador_sempre_usa_csi_mesmo_em_decckm() {
        let mods = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        let bytes = encode_key(TermKey::ArrowRight, mods, &modes(true, false));
        // ctrl -> param = 1 + 4 = 5
        assert_eq!(bytes, b"\x1b[1;5C");
    }

    #[test]
    fn home_end_seguem_decckm_como_as_setas() {
        assert_eq!(
            encode_key(TermKey::Home, Modifiers::NONE, &modes(false, false)),
            b"\x1b[H"
        );
        assert_eq!(
            encode_key(TermKey::End, Modifiers::NONE, &modes(true, false)),
            b"\x1bOF"
        );
    }

    #[test]
    fn page_up_down_com_e_sem_modificador() {
        assert_eq!(
            encode_key(TermKey::PageUp, Modifiers::NONE, &modes(false, false)),
            b"\x1b[5~"
        );
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            encode_key(TermKey::PageDown, shift, &modes(false, false)),
            b"\x1b[6;2~"
        );
    }

    #[test]
    fn f1_a_f4_usam_ss3_f5_em_diante_usa_til() {
        assert_eq!(
            encode_key(TermKey::Function(1), Modifiers::NONE, &modes(false, false)),
            b"\x1bOP"
        );
        assert_eq!(
            encode_key(TermKey::Function(4), Modifiers::NONE, &modes(false, false)),
            b"\x1bOS"
        );
        assert_eq!(
            encode_key(TermKey::Function(5), Modifiers::NONE, &modes(false, false)),
            b"\x1b[15~"
        );
        assert_eq!(
            encode_key(TermKey::Function(12), Modifiers::NONE, &modes(false, false)),
            b"\x1b[24~"
        );
    }

    #[test]
    fn tab_e_shift_tab() {
        assert_eq!(
            encode_key(TermKey::Tab, Modifiers::NONE, &modes(false, false)),
            b"\t"
        );
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            encode_key(TermKey::Tab, shift, &modes(false, false)),
            b"\x1b[Z"
        );
    }

    #[test]
    fn enter_e_backspace_com_alt_prefixam_escape() {
        let alt = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            encode_key(TermKey::Enter, Modifiers::NONE, &modes(false, false)),
            b"\r"
        );
        assert_eq!(
            encode_key(TermKey::Enter, alt, &modes(false, false)),
            b"\x1b\r"
        );
        assert_eq!(
            encode_key(TermKey::Backspace, Modifiers::NONE, &modes(false, false)),
            b"\x7f"
        );
        assert_eq!(
            encode_key(TermKey::Backspace, alt, &modes(false, false)),
            b"\x1b\x7f"
        );
    }

    #[test]
    fn ctrl_letra_vira_byte_de_controle() {
        assert_eq!(encode_ctrl_char('c'), Some(0x03)); // Ctrl+C
        assert_eq!(encode_ctrl_char('C'), Some(0x03));
        assert_eq!(encode_ctrl_char('d'), Some(0x04)); // Ctrl+D
        assert_eq!(encode_ctrl_char('a'), Some(0x01));
        assert_eq!(encode_ctrl_char('['), Some(0x1b));
        assert_eq!(encode_ctrl_char('9'), None);
    }

    #[test]
    fn texto_normal_com_alt_prefixa_escape() {
        let alt = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert_eq!(encode_text("a", Modifiers::NONE), b"a");
        assert_eq!(encode_text("a", alt), b"\x1ba");
        // Acento composto por tecla morta em ABNT2 (ex.: ´ + a = á) chega
        // já pronto em `text` -- passa direto, sem tabela.
        assert_eq!(encode_text("á", Modifiers::NONE), "á".as_bytes());
    }

    #[test]
    fn bracketed_paste_envolve_so_quando_o_modo_esta_ativo() {
        let ativo = modes(false, true);
        let inativo = modes(false, false);
        assert_eq!(
            wrap_paste("oi\nmundo", &ativo),
            b"\x1b[200~oi\nmundo\x1b[201~"
        );
        assert_eq!(wrap_paste("oi\nmundo", &inativo), b"oi\nmundo");
    }
}
