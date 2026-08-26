// SPDX-License-Identifier: GPL-3.0-or-later

//! Traduz eventos de teclado/roda do `winit` para `porecatu_term`
//! (ADR-0008) -- `porecatu-term` não conhece GUI, então a tradução mora
//! aqui, não lá.
//!
//! Cadeia de resolução do ADR-0008: (1) modo de captura -- não existe em
//! F1, sem rename/busca ainda; (2) keybind de aplicação -- hoje só
//! rolagem do scrollback (`Shift+PageUp`/`PageDown`, ADR-0013), o único
//! default que não depende de `porecatu-config` (F4); (3) terminal --
//! codifica em bytes e escreve no PTY.

use porecatu_term::{
    Modifiers, TermKey, TermModes, TermScroll, Terminal, encode_ctrl_char, encode_key, encode_text,
};
use winit::event::{ElementState, KeyEvent, MouseScrollDelta};
use winit::keyboard::{Key, ModifiersState, NamedKey};

// docs/config/porecatu.example.toml [terminal.scrollback] scroll_multiplier
// = 3 (RF-5.27, RF-10.13): linhas por notch da roda.
const SCROLL_MULTIPLIER: i32 = 3;

pub fn modifiers_from(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        ctrl: state.control_key(),
        alt: state.alt_key(),
        super_: state.super_key(),
    }
}

fn named_term_key(named: NamedKey) -> Option<TermKey> {
    Some(match named {
        NamedKey::Enter => TermKey::Enter,
        NamedKey::Backspace => TermKey::Backspace,
        NamedKey::Tab => TermKey::Tab,
        NamedKey::Escape => TermKey::Escape,
        NamedKey::ArrowUp => TermKey::ArrowUp,
        NamedKey::ArrowDown => TermKey::ArrowDown,
        NamedKey::ArrowLeft => TermKey::ArrowLeft,
        NamedKey::ArrowRight => TermKey::ArrowRight,
        NamedKey::Home => TermKey::Home,
        NamedKey::End => TermKey::End,
        NamedKey::PageUp => TermKey::PageUp,
        NamedKey::PageDown => TermKey::PageDown,
        NamedKey::Insert => TermKey::Insert,
        NamedKey::Delete => TermKey::Delete,
        NamedKey::F1 => TermKey::Function(1),
        NamedKey::F2 => TermKey::Function(2),
        NamedKey::F3 => TermKey::Function(3),
        NamedKey::F4 => TermKey::Function(4),
        NamedKey::F5 => TermKey::Function(5),
        NamedKey::F6 => TermKey::Function(6),
        NamedKey::F7 => TermKey::Function(7),
        NamedKey::F8 => TermKey::Function(8),
        NamedKey::F9 => TermKey::Function(9),
        NamedKey::F10 => TermKey::Function(10),
        NamedKey::F11 => TermKey::Function(11),
        NamedKey::F12 => TermKey::Function(12),
        // Espaço, CapsLock, teclas de mídia etc.: sem codificação própria
        // de terminal -- cai para `event.text` (Espaço) ou é ignorada.
        _ => return None,
    })
}

/// Ponto de entrada do teclado. Eventos de IME (`Ime::Preedit`/`Commit`)
/// **não** passam por aqui -- vão direto pro terminal, sem consultar nada
/// (ADR-0008); ver `window_event` em `lib.rs`.
pub fn handle_keyboard_input(
    terminal: &Terminal,
    modes: &TermModes,
    event: &KeyEvent,
    modifiers: Modifiers,
) {
    if event.state != ElementState::Pressed {
        return; // sem key-up reporting no v1
    }

    // Passo 2: keybind de aplicação. Único default sem config: rolagem do
    // scrollback. Tela alternativa -> ação não faz nada, mas ainda assim
    // não cai pro terminal (um binding que casa nunca cai, ADR-0008).
    if modifiers.shift && !modifiers.ctrl && !modifiers.alt {
        match &event.logical_key {
            Key::Named(NamedKey::PageUp) => {
                if !modes.alt_screen {
                    terminal.scroll(TermScroll::PageUp);
                }
                return;
            }
            Key::Named(NamedKey::PageDown) => {
                if !modes.alt_screen {
                    terminal.scroll(TermScroll::PageDown);
                }
                return;
            }
            _ => {}
        }
    }

    // Passo 3: terminal.
    if let Key::Named(named) = &event.logical_key
        && let Some(term_key) = named_term_key(*named)
    {
        terminal.write(encode_key(term_key, modifiers, modes));
        return;
    }

    if modifiers.ctrl
        && !modifiers.alt
        && let Key::Character(s) = &event.logical_key
        && let Some(c) = s.chars().next()
        && let Some(byte) = encode_ctrl_char(c)
    {
        terminal.write(vec![byte]);
        return;
    }

    if let Some(text) = &event.text {
        terminal.write(encode_text(text, modifiers));
    }
}

/// Roda do mouse: rola o scrollback, ou -- na tela alternativa, sem
/// scrollback -- vira setas (RF-10.14, `alternate_scroll` default `true`
/// no `porecatu.example.toml`, sem chave para desligar ainda por não
/// existir `porecatu-config`).
pub fn handle_mouse_wheel(terminal: &Terminal, modes: &TermModes, delta: MouseScrollDelta) {
    let notches = match delta {
        MouseScrollDelta::LineDelta(_, y) => y,
        // Trackpad: sem notch discreto: 20px lógicos por linha é uma
        // aproximação de trabalho, sem procedência de design (não é valor
        // de aparência) -- ajustar se ficar ruim na prática.
        MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as f32,
    };
    if notches == 0.0 {
        return;
    }
    let lines = (notches.signum() as i32) * SCROLL_MULTIPLIER;

    if modes.alt_screen {
        let key = if lines > 0 {
            TermKey::ArrowUp
        } else {
            TermKey::ArrowDown
        };
        let mut bytes = Vec::new();
        for _ in 0..lines.abs() {
            bytes.extend(encode_key(key, Modifiers::NONE, modes));
        }
        terminal.write(bytes);
    } else {
        terminal.scroll(TermScroll::Lines(lines));
    }
}
