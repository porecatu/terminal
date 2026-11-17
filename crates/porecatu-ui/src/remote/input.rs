// SPDX-License-Identifier: GPL-3.0-or-later

//! Remote input: keys and text from the device, through the key encoder.
//!
//! The device sends keys, text and pastes; the bytes come out of the same
//! encoder as the local keyboard (`porecatu_term::keys`, and
//! [`crate::input::encode_character_key`] for a character with modifiers), so
//! the two never diverge (ADR-0105 §7, PRD-025 RF-25.21 and RF-25.22). No
//! Porecatu shortcut and no keymap is consulted: remote input is step 3 of the
//! ADR-0008 chain only.

use porecatu_remote::RemoteInput;
use porecatu_remote::protocol::message::input::mods;
use porecatu_remote::protocol::message::{KeyCode, NamedKey};
use porecatu_term::{Modifiers, TermKey, TermModes, encode_key, encode_text, wrap_paste};

use crate::input::encode_character_key;

/// The bytes the PTY gets for a remote input, given the modes of the pane.
pub(crate) fn remote_input_bytes(input: &RemoteInput, modes: &TermModes) -> Vec<u8> {
    match input {
        RemoteInput::Key(key) => {
            let modifiers = modifiers_from_bits(key.mods);
            match key.key {
                KeyCode::Named(named) => term_key(named)
                    .map(|key| encode_key(key, modifiers, modes))
                    .unwrap_or_default(),
                KeyCode::Char(c) => {
                    encode_character_key(Some(c), Some(c.encode_utf8(&mut [0; 4])), modifiers)
                        .unwrap_or_default()
                }
            }
        }
        RemoteInput::Text(text) => encode_text(&text.text, Modifiers::NONE),
        RemoteInput::Paste(paste) => wrap_paste(&paste.text, modes),
    }
}

/// `mods` bits of the protocol to the modifiers of the encoder; unknown bits
/// are ignored.
fn modifiers_from_bits(bits: u64) -> Modifiers {
    Modifiers {
        ctrl: bits & mods::CTRL != 0,
        alt: bits & mods::ALT != 0,
        shift: bits & mods::SHIFT != 0,
        super_: bits & mods::META != 0,
    }
}

/// `None` for a key this build does not know (the enum is non-exhaustive).
fn term_key(named: NamedKey) -> Option<TermKey> {
    match named {
        NamedKey::Enter => Some(TermKey::Enter),
        NamedKey::Tab => Some(TermKey::Tab),
        NamedKey::Backspace => Some(TermKey::Backspace),
        NamedKey::Escape => Some(TermKey::Escape),
        NamedKey::Up => Some(TermKey::ArrowUp),
        NamedKey::Down => Some(TermKey::ArrowDown),
        NamedKey::Left => Some(TermKey::ArrowLeft),
        NamedKey::Right => Some(TermKey::ArrowRight),
        NamedKey::Home => Some(TermKey::Home),
        NamedKey::End => Some(TermKey::End),
        NamedKey::PageUp => Some(TermKey::PageUp),
        NamedKey::PageDown => Some(TermKey::PageDown),
        NamedKey::Insert => Some(TermKey::Insert),
        NamedKey::Delete => Some(TermKey::Delete),
        NamedKey::F1 => Some(TermKey::Function(1)),
        NamedKey::F2 => Some(TermKey::Function(2)),
        NamedKey::F3 => Some(TermKey::Function(3)),
        NamedKey::F4 => Some(TermKey::Function(4)),
        NamedKey::F5 => Some(TermKey::Function(5)),
        NamedKey::F6 => Some(TermKey::Function(6)),
        NamedKey::F7 => Some(TermKey::Function(7)),
        NamedKey::F8 => Some(TermKey::Function(8)),
        NamedKey::F9 => Some(TermKey::Function(9)),
        NamedKey::F10 => Some(TermKey::Function(10)),
        NamedKey::F11 => Some(TermKey::Function(11)),
        NamedKey::F12 => Some(TermKey::Function(12)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use porecatu_remote::protocol::message::{Key, Paste, TextInput};

    fn key(key: KeyCode, mods: u64) -> RemoteInput {
        RemoteInput::Key(Key { pane: 1, key, mods })
    }

    fn named(named: NamedKey, mods: u64) -> RemoteInput {
        key(KeyCode::Named(named), mods)
    }

    #[test]
    fn arrows_follow_the_application_cursor_mode() {
        let normal = TermModes::default();
        let application = TermModes {
            app_cursor_keys: true,
            ..TermModes::default()
        };
        assert_eq!(
            remote_input_bytes(&named(NamedKey::Up, 0), &normal),
            b"\x1b[A"
        );
        assert_eq!(
            remote_input_bytes(&named(NamedKey::Up, 0), &application),
            b"\x1bOA"
        );
        assert_eq!(
            remote_input_bytes(&named(NamedKey::Left, 0), &application),
            b"\x1bOD"
        );
        // With a modifier the CSI form wins, as in the local keyboard.
        assert_eq!(
            remote_input_bytes(&named(NamedKey::Right, mods::SHIFT), &application),
            b"\x1b[1;2C"
        );
    }

    #[test]
    fn named_keys_match_the_local_encoder() {
        let modes = TermModes::default();
        for (wire, term) in [
            (NamedKey::Enter, TermKey::Enter),
            (NamedKey::Tab, TermKey::Tab),
            (NamedKey::Backspace, TermKey::Backspace),
            (NamedKey::Escape, TermKey::Escape),
            (NamedKey::PageUp, TermKey::PageUp),
            (NamedKey::PageDown, TermKey::PageDown),
            (NamedKey::Insert, TermKey::Insert),
            (NamedKey::Delete, TermKey::Delete),
            (NamedKey::Home, TermKey::Home),
            (NamedKey::End, TermKey::End),
            (NamedKey::F1, TermKey::Function(1)),
            (NamedKey::F12, TermKey::Function(12)),
        ] {
            assert_eq!(
                remote_input_bytes(&named(wire, 0), &modes),
                encode_key(term, Modifiers::NONE, &modes)
            );
        }
        assert_eq!(
            remote_input_bytes(&named(NamedKey::Enter, 0), &modes),
            b"\r"
        );
    }

    #[test]
    fn function_key_with_ctrl() {
        let modes = TermModes::default();
        assert_eq!(
            remote_input_bytes(&named(NamedKey::F5, mods::CTRL), &modes),
            b"\x1b[15;5~"
        );
    }

    #[test]
    fn modifier_bits_map_and_unknown_bits_are_ignored() {
        let modifiers = modifiers_from_bits(mods::CTRL | mods::ALT | mods::SHIFT | mods::META);
        assert!(modifiers.ctrl && modifiers.alt && modifiers.shift && modifiers.super_);
        assert_eq!(modifiers_from_bits(0xF0), Modifiers::NONE);
        let modes = TermModes::default();
        assert_eq!(
            remote_input_bytes(&named(NamedKey::Up, 0x100), &modes),
            b"\x1b[A"
        );
    }

    #[test]
    fn ctrl_c_is_etx() {
        let modes = TermModes::default();
        assert_eq!(
            remote_input_bytes(&key(KeyCode::Char('c'), mods::CTRL), &modes),
            vec![0x03]
        );
        assert_eq!(
            remote_input_bytes(&key(KeyCode::Char('C'), mods::CTRL), &modes),
            vec![0x03]
        );
    }

    #[test]
    fn alt_x_prefixes_escape() {
        let modes = TermModes::default();
        assert_eq!(
            remote_input_bytes(&key(KeyCode::Char('x'), mods::ALT), &modes),
            b"\x1bx"
        );
        // Ctrl+Alt takes the text path, as the local keyboard does.
        assert_eq!(
            remote_input_bytes(&key(KeyCode::Char('x'), mods::CTRL | mods::ALT), &modes),
            b"\x1bx"
        );
    }

    #[test]
    fn plain_character_key_is_its_utf8() {
        let modes = TermModes::default();
        assert_eq!(
            remote_input_bytes(&key(KeyCode::Char('a'), 0), &modes),
            b"a"
        );
        assert_eq!(
            remote_input_bytes(&key(KeyCode::Char('é'), 0), &modes),
            "é".as_bytes()
        );
    }

    #[test]
    fn text_with_accent_goes_through_untouched() {
        let modes = TermModes::default();
        let input = RemoteInput::Text(TextInput {
            pane: 1,
            text: "ação".to_owned(),
        });
        assert_eq!(remote_input_bytes(&input, &modes), "ação".as_bytes());
    }

    #[test]
    fn paste_is_bracketed_only_when_the_mode_is_on() {
        let input = RemoteInput::Paste(Paste {
            pane: 1,
            text: "a\nb".to_owned(),
        });
        assert_eq!(remote_input_bytes(&input, &TermModes::default()), b"a\nb");
        let bracketed = TermModes {
            bracketed_paste: true,
            ..TermModes::default()
        };
        assert_eq!(
            remote_input_bytes(&input, &bracketed),
            b"\x1b[200~a\nb\x1b[201~"
        );
    }
}
