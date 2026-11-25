// SPDX-License-Identifier: GPL-3.0-or-later

//! A tradução "tecla -> clipboard -> campo" numa função só (ADR-0115 §4).
//!
//! `apply_text_field_key` é puro e só diz o que fazer ([`FieldKey::Copy`],
//! `Cut`, `Paste`); aqui isso é executado contra um [`ClipboardIo`]. Todo
//! modo de captura que tem campo chama [`edit_field_key`] -- nenhum copia
//! esta lógica --, e o menu de clique direito do campo chama
//! [`run_field_op`], de modo que acionar um item faz exatamente o que a tecla
//! faz. Testável com um clipboard falso, sem `winit` nem `arboard`.

use porecatu_term::Modifiers;
use winit::keyboard::Key;
use zeroize::Zeroizing;

use crate::clipboard::ClipboardIo;
use crate::masked_field::MaskedFieldState;
use crate::text_field::{FieldKey, TextFieldState, apply_text_field_key};

/// Uma das três operações de clipboard de um campo. Tipo próprio, e não
/// [`FieldKey`], porque o menu do campo as pede sem passar por tecla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldOp {
    Copy,
    Cut,
    Paste,
}

impl FieldOp {
    /// A operação que uma tecla pede, ou `None` se ela não é de clipboard.
    pub const fn from_key(key: FieldKey) -> Option<Self> {
        match key {
            FieldKey::Copy => Some(Self::Copy),
            FieldKey::Cut => Some(Self::Cut),
            FieldKey::Paste => Some(Self::Paste),
            FieldKey::Ignored | FieldKey::Handled => None,
        }
    }
}

/// Executa `op` em `field` contra `io`. Copiar e recortar sem seleção não
/// fazem nada; colar com o clipboard vazio ou ilegível também não, sem aviso
/// (ADR-0115 §2). `max_chars` é o teto de comprimento do campo, se tiver.
pub fn run_field_op(
    field: &mut TextFieldState,
    op: FieldOp,
    io: &mut impl ClipboardIo,
    max_chars: Option<usize>,
) {
    match op {
        FieldOp::Copy => {
            if let Some(selected) = field.selected_text() {
                io.copy(selected);
            }
        }
        FieldOp::Cut => {
            if let Some(cut) = field.cut_selection() {
                io.copy(&cut);
            }
        }
        FieldOp::Paste => {
            if let Some(text) = io.paste()
                && !text.is_empty()
            {
                field.insert_str(&text, max_chars);
            }
        }
    }
}

/// A função única dos modos de captura: aplica a tecla ao campo e executa o
/// pedido de clipboard, se houver. Devolve `true` se a tecla foi consumida --
/// inclusive `Ctrl+C` sem seleção, que nunca segue para o terminal.
pub fn edit_field_key(
    field: &mut TextFieldState,
    key: &Key,
    text: Option<&str>,
    modifiers: Modifiers,
    max_chars: Option<usize>,
    io: &mut impl ClipboardIo,
) -> bool {
    let result = apply_text_field_key(field, key, text, modifiers);
    if let Some(op) = FieldOp::from_key(result) {
        run_field_op(field, op, io, max_chars);
    }
    result.consumed()
}

/// Cola o clipboard num campo mascarado. O texto lido vai para um
/// `Zeroizing<String>` e é apagado ao sair; clipboard vazio não faz nada.
pub fn paste_into_masked(field: &mut MaskedFieldState, io: &mut impl ClipboardIo) {
    if let Some(pasted) = io.paste().map(Zeroizing::new) {
        field.paste(&pasted);
    }
}

#[cfg(test)]
mod tests {
    use winit::keyboard::NamedKey;

    use super::*;

    #[derive(Default)]
    struct FakeClipboard {
        content: Option<String>,
        writes: Vec<String>,
    }

    impl FakeClipboard {
        fn holding(text: &str) -> Self {
            Self {
                content: Some(text.to_string()),
                writes: Vec::new(),
            }
        }
    }

    impl ClipboardIo for FakeClipboard {
        fn copy(&mut self, text: &str) {
            self.writes.push(text.to_string());
            self.content = Some(text.to_string());
        }

        fn paste(&mut self) -> Option<String> {
            self.content.clone()
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

    fn selecting(text: &str, from: usize, to: usize) -> TextFieldState {
        let mut field = TextFieldState::new(text);
        field.click_at(from);
        field.drag_to(to);
        field
    }

    #[test]
    fn copy_writes_the_selection_and_keeps_the_field() {
        let mut field = selecting("abcdef", 1, 4);
        let mut io = FakeClipboard::default();
        run_field_op(&mut field, FieldOp::Copy, &mut io, None);
        assert_eq!(io.writes, ["bcd"]);
        assert_eq!(field.text(), "abcdef");
        assert_eq!(field.selected_text(), Some("bcd"));
    }

    #[test]
    fn copy_without_selection_does_nothing() {
        let mut field = TextFieldState::new("abc");
        let mut io = FakeClipboard::holding("untouched");
        run_field_op(&mut field, FieldOp::Copy, &mut io, None);
        assert!(io.writes.is_empty());
        assert_eq!(io.content.as_deref(), Some("untouched"));
    }

    #[test]
    fn cut_copies_and_removes_the_selection() {
        let mut field = selecting("abcdef", 1, 4);
        let mut io = FakeClipboard::default();
        run_field_op(&mut field, FieldOp::Cut, &mut io, None);
        assert_eq!(io.writes, ["bcd"]);
        assert_eq!(field.text(), "aef");
        assert_eq!(field.cursor(), 1);
    }

    #[test]
    fn cut_without_selection_does_nothing() {
        let mut field = TextFieldState::new("abc");
        let mut io = FakeClipboard::holding("untouched");
        run_field_op(&mut field, FieldOp::Cut, &mut io, None);
        assert!(io.writes.is_empty());
        assert_eq!(field.text(), "abc");
    }

    #[test]
    fn paste_replaces_the_selection() {
        let mut field = selecting("abcdef", 1, 4);
        let mut io = FakeClipboard::holding("XY");
        run_field_op(&mut field, FieldOp::Paste, &mut io, None);
        assert_eq!(field.text(), "aXYef");
        assert_eq!(field.cursor(), 3);
    }

    #[test]
    fn paste_with_an_empty_or_unreadable_clipboard_keeps_the_selection() {
        for content in [None, Some("")] {
            let mut field = selecting("abcdef", 1, 4);
            let mut io = FakeClipboard {
                content: content.map(str::to_string),
                writes: Vec::new(),
            };
            run_field_op(&mut field, FieldOp::Paste, &mut io, None);
            assert_eq!(field.text(), "abcdef");
            assert_eq!(field.selected_text(), Some("bcd"), "nothing happened");
        }
    }

    #[test]
    fn paste_of_several_lines_becomes_one_line() {
        let mut field = TextFieldState::new("");
        let mut io = FakeClipboard::holding("one\r\ntwo\nthree\tfour");
        run_field_op(&mut field, FieldOp::Paste, &mut io, None);
        assert_eq!(field.text(), "one two three four");
    }

    #[test]
    fn paste_respects_the_ceiling() {
        let mut field = TextFieldState::new("ab");
        let mut io = FakeClipboard::holding("cdefgh");
        run_field_op(&mut field, FieldOp::Paste, &mut io, Some(5));
        assert_eq!(field.text(), "abcde");
    }

    #[test]
    fn edit_field_key_runs_the_clipboard_op_and_consumes() {
        let mut field = selecting("abcdef", 1, 4);
        let mut io = FakeClipboard::default();
        let consumed = edit_field_key(
            &mut field,
            &Key::Character("x".into()),
            Some("\u{18}"),
            primary(),
            None,
            &mut io,
        );
        assert!(consumed);
        assert_eq!(io.writes, ["bcd"]);
        assert_eq!(field.text(), "aef");

        let consumed = edit_field_key(
            &mut field,
            &Key::Character("v".into()),
            Some("\u{16}"),
            primary(),
            None,
            &mut io,
        );
        assert!(consumed);
        assert_eq!(field.text(), "abcdef");
    }

    #[test]
    fn copy_key_without_selection_is_still_consumed() {
        let mut field = TextFieldState::new("abc");
        let mut io = FakeClipboard::default();
        let consumed = edit_field_key(
            &mut field,
            &Key::Character("c".into()),
            Some("\u{3}"),
            primary(),
            None,
            &mut io,
        );
        assert!(consumed, "Ctrl+C never falls through to the terminal");
        assert!(io.writes.is_empty());
    }

    #[test]
    fn edit_field_key_leaves_unrelated_keys_to_the_caller() {
        let mut field = TextFieldState::new("abc");
        let mut io = FakeClipboard::default();
        assert!(!edit_field_key(
            &mut field,
            &Key::Named(NamedKey::Enter),
            Some("\r"),
            Modifiers::default(),
            None,
            &mut io,
        ));
        assert!(edit_field_key(
            &mut field,
            &Key::Character("d".into()),
            Some("d"),
            Modifiers::default(),
            None,
            &mut io,
        ));
        assert_eq!(field.text(), "abcd");
    }

    #[test]
    fn paste_into_masked_normalizes_and_ignores_an_empty_clipboard() {
        let mut field = MaskedFieldState::new();
        paste_into_masked(&mut field, &mut FakeClipboard::holding("a\nb"));
        assert_eq!(field.len(), 3);
        paste_into_masked(&mut field, &mut FakeClipboard::default());
        assert_eq!(field.len(), 3);
        assert_eq!(field.inspect(str::to_string), "a b");
    }
}
