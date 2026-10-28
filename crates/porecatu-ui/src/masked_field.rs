// SPDX-License-Identifier: GPL-3.0-or-later

//! O campo mascarado do diálogo de segredo (ADR-0075 §1, RF-20.35).
//!
//! Um tipo **à parte**, e não uma flag em [`TextFieldState`], que deriva `Debug`
//! e `Clone` e guarda o texto num `String` comum: um *dump* de estado ou uma
//! cópia acidental levaria a senha junto. Aqui:
//!
//! - o texto vive num `Zeroizing<String>` com a **capacidade reservada de
//!   antemão** (1 KiB) e teto de comprimento nela, para que inserir nunca
//!   realoque e deixe uma cópia antiga, não zerada, na memória; é zerado no
//!   `Drop`;
//! - **sem `Clone`**, com `Debug` opaco;
//! - cursor e seleção vêm de um [`TextFieldState`] **só de pontos** (`•`, um por
//!   escalar), por composição: a lógica do ADR-0035 anda sobre o que é
//!   desenhado, e cada edição é repetida no texto de verdade pelos índices de
//!   escalar;
//! - **copiar e recortar não fazem nada**; **colar funciona**;
//! - o texto sai por **movimento** para o [`Secret`], e o campo fica vazio.

use std::fmt;

use porecatu_term::{Modifiers, Secret};
use winit::keyboard::{Key, NamedKey};
use zeroize::Zeroizing;

use crate::text_field::TextFieldState;

/// O que se desenha no lugar de cada escalar (U+2022, da própria Iosevka).
pub(crate) const MASK_CHAR: char = '•';

/// Capacidade reservada, em bytes. Passou disso, a tecla é ignorada.
const CAPACITY: usize = 1024;

pub struct MaskedFieldState {
    secret: Zeroizing<String>,
    /// Teto de comprimento: a capacidade que o `String` realmente recebeu.
    ceiling: usize,
    /// Só pontos. Dá o cursor e a seleção; nunca o texto.
    dots: TextFieldState,
}

impl fmt::Debug for MaskedFieldState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MaskedFieldState(..)")
    }
}

impl Default for MaskedFieldState {
    fn default() -> Self {
        Self::new()
    }
}

impl MaskedFieldState {
    pub fn new() -> Self {
        Self::with_capacity(CAPACITY)
    }

    fn with_capacity(bytes: usize) -> Self {
        let secret = Zeroizing::new(String::with_capacity(bytes));
        let ceiling = secret.capacity();
        Self {
            secret,
            ceiling,
            dots: TextFieldState::default(),
        }
    }

    /// Quantos escalares há.
    pub fn len(&self) -> usize {
        self.secret.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.secret.is_empty()
    }

    /// O que se desenha: pontos. Cursor e seleção estão aqui, em bytes **dos
    /// pontos** (`MASK_CHAR.len_utf8()` cada).
    pub fn display(&self) -> &TextFieldState {
        &self.dots
    }

    /// Capacidade atual do buffer; não muda enquanto se digita até o teto.
    #[cfg(test)]
    fn capacity(&self) -> usize {
        self.secret.capacity()
    }

    /// Byte do texto de verdade onde começa o escalar `scalar`.
    fn byte_of(&self, scalar: usize) -> usize {
        self.secret
            .char_indices()
            .nth(scalar)
            .map_or(self.secret.len(), |(byte, _)| byte)
    }

    fn scalar_of_dot_byte(dot_byte: usize) -> usize {
        dot_byte / MASK_CHAR.len_utf8()
    }

    /// A seleção, em bytes do texto de verdade.
    fn selected_bytes(&self) -> Option<(usize, usize)> {
        let (a, b) = self.dots.selection_range()?;
        Some((
            self.byte_of(Self::scalar_of_dot_byte(a)),
            self.byte_of(Self::scalar_of_dot_byte(b)),
        ))
    }

    fn cursor_byte(&self) -> usize {
        self.byte_of(Self::scalar_of_dot_byte(self.dots.cursor()))
    }

    pub fn insert_char(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        let selected = self.selected_bytes();
        let removed = selected.map_or(0, |(start, end)| end - start);
        // Sem espaço: ignora, e nunca realoca.
        if self.secret.len() - removed + c.len_utf8() > self.ceiling {
            return;
        }
        let at = selected.map_or_else(|| self.cursor_byte(), |(start, _)| start);
        if let Some((start, end)) = selected {
            self.secret.drain(start..end);
        }
        self.secret.insert(at, c);
        self.dots.insert_char(MASK_CHAR);
    }

    /// Cola: cada escalar que cabe entra, os controles não.
    pub fn paste(&mut self, text: &str) {
        for c in text.chars() {
            self.insert_char(c);
        }
    }

    pub fn backspace(&mut self) {
        if let Some((start, end)) = self.selected_bytes() {
            self.secret.drain(start..end);
        } else {
            let scalar = Self::scalar_of_dot_byte(self.dots.cursor());
            if scalar == 0 {
                return;
            }
            let (start, end) = (self.byte_of(scalar - 1), self.byte_of(scalar));
            self.secret.drain(start..end);
        }
        self.dots.backspace();
    }

    pub fn delete_forward(&mut self) {
        if let Some((start, end)) = self.selected_bytes() {
            self.secret.drain(start..end);
        } else {
            let scalar = Self::scalar_of_dot_byte(self.dots.cursor());
            if scalar >= self.len() {
                return;
            }
            let (start, end) = (self.byte_of(scalar), self.byte_of(scalar + 1));
            self.secret.drain(start..end);
        }
        self.dots.delete_forward();
    }

    pub fn move_left(&mut self, extend: bool) {
        self.dots.move_left(extend);
    }

    pub fn move_right(&mut self, extend: bool) {
        self.dots.move_right(extend);
    }

    pub fn move_home(&mut self, extend: bool) {
        self.dots.move_home(extend);
    }

    pub fn move_end(&mut self, extend: bool) {
        self.dots.move_end(extend);
    }

    pub fn select_all(&mut self) {
        self.dots.select_all();
    }

    /// Clique: o cursor vai para antes do `scalar`-ésimo ponto (limitado ao
    /// fim).
    pub fn click_at_scalar(&mut self, scalar: usize) {
        self.dots
            .click_at(scalar.min(self.len()) * MASK_CHAR.len_utf8());
    }

    /// O texto sai **por movimento**: o buffer inteiro, com a capacidade e sem
    /// cópia, vai para o [`Secret`]; o campo recomeça vazio.
    pub fn take(&mut self) -> Secret {
        let fresh = Zeroizing::new(String::with_capacity(CAPACITY));
        self.ceiling = fresh.capacity();
        self.dots = TextFieldState::default();
        Secret::from_zeroizing(std::mem::replace(&mut self.secret, fresh))
    }

    #[cfg(test)]
    fn reveal_for_test(&self) -> &str {
        &self.secret
    }
}

/// O que uma tecla fez ao campo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskedKey {
    /// Não é de edição: quem chama decide (`Enter`, `Esc`, `Tab`).
    Ignored,
    /// Consumida. Inclui `Ctrl+C` e `Ctrl+X`, que **não fazem nada**.
    Handled,
    /// Colar: quem chama busca o texto da área de transferência e chama
    /// [`MaskedFieldState::paste`].
    Paste,
}

/// Traduz uma tecla numa edição do campo. Mesma assinatura de
/// `apply_text_field_key`, pelo mesmo motivo (`KeyEvent` não é construível fora
/// do `winit`).
pub fn apply_masked_key(
    field: &mut MaskedFieldState,
    logical_key: &Key,
    text: Option<&str>,
    modifiers: Modifiers,
) -> MaskedKey {
    let primary = if cfg!(target_os = "macos") {
        modifiers.super_
    } else {
        modifiers.ctrl
    };
    match logical_key {
        Key::Character(s) if primary && !modifiers.alt => {
            if s.eq_ignore_ascii_case("a") {
                field.select_all();
                MaskedKey::Handled
            } else if s.eq_ignore_ascii_case("v") {
                MaskedKey::Paste
            } else if s.eq_ignore_ascii_case("c") || s.eq_ignore_ascii_case("x") {
                // RF-20.35: o campo mascarado não copia nem recorta.
                MaskedKey::Handled
            } else {
                MaskedKey::Ignored
            }
        }
        Key::Named(NamedKey::Insert) if modifiers.shift => MaskedKey::Paste,
        // `Ctrl+Insert` é "copiar", e `Shift+Delete` é "recortar".
        Key::Named(NamedKey::Insert) if modifiers.ctrl => MaskedKey::Handled,
        Key::Named(NamedKey::Delete) if modifiers.shift => MaskedKey::Handled,
        Key::Named(NamedKey::ArrowLeft) => {
            field.move_left(modifiers.shift);
            MaskedKey::Handled
        }
        Key::Named(NamedKey::ArrowRight) => {
            field.move_right(modifiers.shift);
            MaskedKey::Handled
        }
        Key::Named(NamedKey::Home) => {
            field.move_home(modifiers.shift);
            MaskedKey::Handled
        }
        Key::Named(NamedKey::End) => {
            field.move_end(modifiers.shift);
            MaskedKey::Handled
        }
        Key::Named(NamedKey::Backspace) => {
            field.backspace();
            MaskedKey::Handled
        }
        Key::Named(NamedKey::Delete) => {
            field.delete_forward();
            MaskedKey::Handled
        }
        // Espaço **entra**: senha pode tê-lo.
        Key::Named(NamedKey::Enter | NamedKey::Escape | NamedKey::Tab) => MaskedKey::Ignored,
        _ => {
            let Some(text) = text else {
                return MaskedKey::Ignored;
            };
            if primary || modifiers.alt {
                return MaskedKey::Ignored;
            }
            let mut consumed = false;
            for c in text.chars().filter(|c| !c.is_control()) {
                field.insert_char(c);
                consumed = true;
            }
            if consumed {
                MaskedKey::Handled
            } else {
                MaskedKey::Ignored
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROOF: &str = "tr0ub4dor-proof";

    fn typed(text: &str) -> MaskedFieldState {
        let mut field = MaskedFieldState::new();
        field.paste(text);
        field
    }

    fn no_mods() -> Modifiers {
        Modifiers {
            shift: false,
            ctrl: false,
            alt: false,
            super_: false,
        }
    }

    fn primary() -> Modifiers {
        Modifiers {
            shift: false,
            ctrl: !cfg!(target_os = "macos"),
            alt: false,
            super_: cfg!(target_os = "macos"),
        }
    }

    #[test]
    fn debug_never_carries_the_text() {
        let field = typed(PROOF);
        assert_eq!(format!("{field:?}"), "MaskedFieldState(..)");
        assert!(!format!("{field:#?}").contains("tr0ub4dor"));
    }

    #[test]
    fn what_is_drawn_is_one_dot_per_scalar_and_never_the_text() {
        let field = typed("aé😀");
        assert_eq!(field.len(), 3);
        assert_eq!(field.display().text(), "•••");
        assert!(!field.display().text().contains('a'));
    }

    #[test]
    fn the_capacity_never_changes_while_typing_up_to_the_ceiling() {
        let mut field = MaskedFieldState::new();
        let capacity = field.capacity();
        assert!(capacity >= CAPACITY);
        for _ in 0..capacity * 2 {
            field.insert_char('x');
        }
        assert_eq!(field.capacity(), capacity, "inserting never reallocates");
        assert_eq!(field.len(), capacity, "and stops at the ceiling");
        // A multibyte scalar that does not fit is ignored whole.
        field.backspace();
        field.insert_char('😀');
        assert_eq!(field.len(), capacity - 1);
        assert_eq!(field.capacity(), capacity);
    }

    #[test]
    fn editing_in_the_middle_keeps_the_text_and_the_dots_in_step() {
        let mut field = typed("héllo");
        field.move_home(false);
        field.move_right(false);
        field.move_right(false);
        field.insert_char('X');
        assert_eq!(field.reveal_for_test(), "héXllo");
        field.backspace();
        field.backspace();
        assert_eq!(field.reveal_for_test(), "hllo");
        field.delete_forward();
        assert_eq!(field.reveal_for_test(), "hlo");
        assert_eq!(field.display().text(), "•••");
    }

    #[test]
    fn typing_over_a_selection_replaces_it() {
        let mut field = typed("abcdef");
        field.move_home(false);
        field.move_right(false);
        field.move_right(true);
        field.move_right(true);
        field.insert_char('Z');
        assert_eq!(field.reveal_for_test(), "aZdef");
        field.select_all();
        field.backspace();
        assert!(field.is_empty());
        assert_eq!(field.display().text(), "");
    }

    #[test]
    fn control_characters_never_enter() {
        let mut field = MaskedFieldState::new();
        field.paste("a\nb\tc\u{7f}\u{1b}");
        assert_eq!(field.reveal_for_test(), "abc");
    }

    #[test]
    fn taking_the_text_moves_the_buffer_into_the_secret_and_empties_the_field() {
        let mut field = typed(PROOF);
        let address = field.reveal_for_test().as_ptr();

        let secret = field.take();

        assert_eq!(secret.expose(), PROOF);
        assert_eq!(secret.expose().as_ptr(), address, "moved, not copied");
        assert!(field.is_empty());
        assert_eq!(field.display().text(), "");
        assert!(field.capacity() >= CAPACITY, "ready for another try");
    }

    #[test]
    fn copy_and_cut_do_nothing_and_leave_the_text_alone() {
        let mut field = typed(PROOF);
        field.select_all();
        for key in ["c", "C", "x", "X"] {
            let result = apply_masked_key(&mut field, &Key::Character(key.into()), None, primary());
            assert_eq!(result, MaskedKey::Handled, "{key} is swallowed");
        }
        for (key, mods) in [
            (
                NamedKey::Insert,
                Modifiers {
                    ctrl: true,
                    ..no_mods()
                },
            ),
            (
                NamedKey::Delete,
                Modifiers {
                    shift: true,
                    ..no_mods()
                },
            ),
        ] {
            assert_eq!(
                apply_masked_key(&mut field, &Key::Named(key), None, mods),
                MaskedKey::Handled
            );
        }
        assert_eq!(field.reveal_for_test(), PROOF, "nothing was cut");
    }

    #[test]
    fn paste_is_asked_for_and_works() {
        let mut field = MaskedFieldState::new();
        assert_eq!(
            apply_masked_key(&mut field, &Key::Character("v".into()), None, primary()),
            MaskedKey::Paste
        );
        let shift_insert = Modifiers {
            shift: true,
            ..no_mods()
        };
        assert_eq!(
            apply_masked_key(
                &mut field,
                &Key::Named(NamedKey::Insert),
                None,
                shift_insert
            ),
            MaskedKey::Paste
        );
        field.paste("from-the-password-manager");
        assert_eq!(field.reveal_for_test(), "from-the-password-manager");
    }

    #[test]
    fn typing_goes_through_the_key_function() {
        let mut field = MaskedFieldState::new();
        let key = Key::Character("a".into());
        assert_eq!(
            apply_masked_key(&mut field, &key, Some("a"), no_mods()),
            MaskedKey::Handled
        );
        assert_eq!(
            apply_masked_key(
                &mut field,
                &Key::Named(NamedKey::Enter),
                Some("\r"),
                no_mods()
            ),
            MaskedKey::Ignored
        );
        assert_eq!(field.reveal_for_test(), "a");
    }

    #[test]
    fn clicking_puts_the_cursor_before_a_dot_and_clamps_to_the_end() {
        let mut field = typed("abc");
        field.click_at_scalar(1);
        field.insert_char('X');
        assert_eq!(field.reveal_for_test(), "aXbc");
        field.click_at_scalar(99);
        field.insert_char('Y');
        assert_eq!(field.reveal_for_test(), "aXbcY");
    }
}
