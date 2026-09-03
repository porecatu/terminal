// SPDX-License-Identifier: GPL-3.0-or-later

//! Estado puro de um campo de texto de uma linha, com cursor navegável e
//! seleção -- compartilhado pelo campo de nome do editor de grupo
//! (`group_editor.rs`) e pelo campo de rename de aba (`rename.rs`). Os dois
//! tinham, até o ADR-0035, a mesma simplificação ("sem posição de cursor no
//! meio da string -- sempre no fim") que o ADR-0023 e o ADR-0008
//! registravam; o ADR-0035 supera essa parte de cada um. Testável sem
//! `winit`, mesmo padrão de `context_menu.rs`/`dialog.rs`/`rename.rs`.
//!
//! Todos os índices são em bytes, sempre em fronteira de char UTF-8 --
//! nunca aritmética de byte crua sobre o meio de um caractere multibyte.

use porecatu_term::Modifiers;
use winit::keyboard::{Key, NamedKey};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextFieldState {
    buffer: String,
    cursor: usize,
    /// `Some(a)` com `a != cursor` é a única condição em que há seleção
    /// visível -- ver `selection_range`. Durante um arraste do mouse, o
    /// `anchor` fica armado mesmo passando momentaneamente por `a ==
    /// cursor` (o usuário pode continuar arrastando de volta); só a
    /// navegação por teclado sem `Shift` limpa `anchor` de propósito, para
    /// colapsar a seleção.
    anchor: Option<usize>,
}

impl TextFieldState {
    /// Cursor no fim, sem seleção -- o mesmo estado inicial que
    /// `GroupEditor::new`/`action_rename_start` já produziam com o buffer
    /// cru.
    pub fn new(initial: &str) -> Self {
        Self {
            buffer: initial.to_string(),
            cursor: initial.len(),
            anchor: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.buffer
    }

    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// `(início, fim)` em bytes, ou `None` sem seleção ativa.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        match self.anchor {
            Some(a) if a != self.cursor => Some(if a < self.cursor {
                (a, self.cursor)
            } else {
                (self.cursor, a)
            }),
            _ => None,
        }
    }

    fn clamp_to_boundary(&self, idx: usize) -> usize {
        let mut idx = idx.min(self.buffer.len());
        while idx > 0 && !self.buffer.is_char_boundary(idx) {
            idx -= 1;
        }
        idx
    }

    /// Remove o trecho selecionado, se houver, e posiciona o cursor no
    /// início dele. Devolve `true` se havia seleção (e portanto já apagou
    /// algo) -- quem chama (`insert_char`/`backspace`/`delete_forward`) usa
    /// isso pra saber se ainda precisa apagar mais um caractere.
    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        self.buffer.drain(start..end);
        self.cursor = start;
        self.anchor = None;
        true
    }

    pub fn insert_char(&mut self, c: char) {
        self.delete_selection();
        self.buffer.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor == 0 {
            return;
        }
        let mut prev = self.cursor - 1;
        while !self.buffer.is_char_boundary(prev) {
            prev -= 1;
        }
        self.buffer.drain(prev..self.cursor);
        self.cursor = prev;
    }

    pub fn delete_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut next = self.cursor + 1;
        while next < self.buffer.len() && !self.buffer.is_char_boundary(next) {
            next += 1;
        }
        self.buffer.drain(self.cursor..next);
    }

    /// `extend` arma `anchor` na posição atual antes de mover, se ainda não
    /// houver um -- é o comportamento universal de `Shift+seta`.
    fn set_cursor(&mut self, idx: usize, extend: bool) {
        let idx = self.clamp_to_boundary(idx);
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = idx;
    }

    /// Sem `Shift` sobre uma seleção ativa, colapsa nela (borda esquerda)
    /// em vez de mover mais um caractere -- o comportamento universal de
    /// qualquer editor de texto.
    pub fn move_left(&mut self, extend: bool) {
        if !extend && let Some((start, _)) = self.selection_range() {
            self.cursor = start;
            self.anchor = None;
            return;
        }
        if self.cursor == 0 {
            self.set_cursor(0, extend);
            return;
        }
        let mut prev = self.cursor - 1;
        while !self.buffer.is_char_boundary(prev) {
            prev -= 1;
        }
        self.set_cursor(prev, extend);
    }

    pub fn move_right(&mut self, extend: bool) {
        if !extend && let Some((_, end)) = self.selection_range() {
            self.cursor = end;
            self.anchor = None;
            return;
        }
        if self.cursor >= self.buffer.len() {
            self.set_cursor(self.buffer.len(), extend);
            return;
        }
        let mut next = self.cursor + 1;
        while next < self.buffer.len() && !self.buffer.is_char_boundary(next) {
            next += 1;
        }
        self.set_cursor(next, extend);
    }

    pub fn move_home(&mut self, extend: bool) {
        self.set_cursor(0, extend);
    }

    pub fn move_end(&mut self, extend: bool) {
        let end = self.buffer.len();
        self.set_cursor(end, extend);
    }

    pub fn select_all(&mut self) {
        if self.buffer.is_empty() {
            self.anchor = None;
            self.cursor = 0;
            return;
        }
        self.anchor = Some(0);
        self.cursor = self.buffer.len();
    }

    /// Clique simples: posiciona o cursor, sem seleção -- um novo clique
    /// sempre começa do zero, mesmo em cima de uma seleção existente.
    pub fn click_at(&mut self, byte_index: usize) {
        self.cursor = self.clamp_to_boundary(byte_index);
        self.anchor = None;
    }

    /// Clique + arraste: a primeira chamada depois de `click_at` arma
    /// `anchor` na posição do clique inicial (o `cursor` de então); as
    /// chamadas seguintes só movem `cursor`, mesmo que passem de volta pelo
    /// ponto de partida.
    pub fn drag_to(&mut self, byte_index: usize) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = self.clamp_to_boundary(byte_index);
    }
}

/// Traduz uma tecla em uma edição de `field`, para o modo de captura de um
/// campo de nome (editor de grupo ou rename de aba). `logical_key`/`text`
/// são os mesmos campos de `winit::event::KeyEvent` (que não é
/// publicamente construível fora do `winit` -- por isso a assinatura toma
/// os dois separados, em vez do evento inteiro, também testável sem
/// `winit::event`). Devolve `true` se a tecla foi consumida como edição de
/// texto -- `Enter`/`Esc`/`Tab`, que são exclusivos de cada widget
/// (confirmar/cancelar/trocar de região), não passam por aqui e continuam
/// tratados por quem chama. Não filtra `event.state`; quem chama já
/// garantiu `Pressed`, mesmo padrão dos outros `handle_*_key` de `lib.rs`.
pub fn apply_text_field_key(
    field: &mut TextFieldState,
    logical_key: &Key,
    text: Option<&str>,
    modifiers: Modifiers,
) -> bool {
    // Mesmo idioma do modificador de seleção múltipla de aba (ADR-0021 §3):
    // `Ctrl` em Windows/Linux, `Cmd` (`super_`) no macOS.
    let primary = if cfg!(target_os = "macos") {
        modifiers.super_
    } else {
        modifiers.ctrl
    };
    match logical_key {
        Key::Character(s) if primary && !modifiers.alt && s.eq_ignore_ascii_case("a") => {
            field.select_all();
            true
        }
        Key::Named(NamedKey::ArrowLeft) => {
            field.move_left(modifiers.shift);
            true
        }
        Key::Named(NamedKey::ArrowRight) => {
            field.move_right(modifiers.shift);
            true
        }
        Key::Named(NamedKey::Home) => {
            field.move_home(modifiers.shift);
            true
        }
        Key::Named(NamedKey::End) => {
            field.move_end(modifiers.shift);
            true
        }
        Key::Named(NamedKey::Delete) => {
            field.delete_forward();
            true
        }
        _ => {
            let Some(text) = text else {
                return false;
            };
            let mut consumed = false;
            for c in text.chars().filter(|c| !c.is_control()) {
                field.insert_char(c);
                consumed = true;
            }
            consumed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, shift: bool) -> Modifiers {
        Modifiers {
            shift,
            ctrl,
            alt: false,
            super_: false,
        }
    }

    /// O modificador "primário" que `apply_text_field_key` de fato lê pra
    /// `Ctrl+A`/`Cmd+A` -- `Ctrl` em Windows/Linux, `Cmd` (`super_`) no
    /// macOS. Testar sempre com `ctrl: true` fixo passava em duas das três
    /// plataformas e falhava silenciosamente no CI do macOS -- o próprio
    /// `cfg!(target_os = "macos")` que o código usa é o que o teste precisa
    /// espelhar, não um valor fixo.
    fn primary_mods(shift: bool) -> Modifiers {
        Modifiers {
            shift,
            ctrl: !cfg!(target_os = "macos"),
            alt: false,
            super_: cfg!(target_os = "macos"),
        }
    }

    #[test]
    fn new_starts_with_cursor_at_end_and_no_selection() {
        let field = TextFieldState::new("olá");
        assert_eq!(field.text(), "olá");
        assert_eq!(field.cursor(), "olá".len());
        assert_eq!(field.selection_range(), None);
    }

    #[test]
    fn insert_char_advances_cursor_by_utf8_len() {
        let mut field = TextFieldState::new("");
        field.insert_char('á');
        assert_eq!(field.text(), "á");
        assert_eq!(field.cursor(), 'á'.len_utf8());
    }

    #[test]
    fn insert_char_replaces_active_selection() {
        let mut field = TextFieldState::new("abcdef");
        field.click_at(1);
        field.drag_to(4);
        assert_eq!(field.selection_range(), Some((1, 4)));
        field.insert_char('X');
        assert_eq!(field.text(), "aXef");
        assert_eq!(field.cursor(), 2);
        assert_eq!(field.selection_range(), None);
    }

    #[test]
    fn backspace_removes_selection_instead_of_one_char() {
        let mut field = TextFieldState::new("abcdef");
        field.click_at(1);
        field.drag_to(4);
        field.backspace();
        assert_eq!(field.text(), "aef");
        assert_eq!(field.cursor(), 1);
        assert_eq!(field.selection_range(), None);
    }

    #[test]
    fn backspace_without_selection_removes_previous_char() {
        let mut field = TextFieldState::new("ab");
        field.backspace();
        assert_eq!(field.text(), "a");
        assert_eq!(field.cursor(), 1);
    }

    #[test]
    fn delete_forward_removes_next_char() {
        let mut field = TextFieldState::new("ab");
        field.click_at(0);
        field.delete_forward();
        assert_eq!(field.text(), "b");
        assert_eq!(field.cursor(), 0);
    }

    #[test]
    fn select_all_spans_whole_buffer() {
        let mut field = TextFieldState::new("abc");
        field.click_at(1);
        field.select_all();
        assert_eq!(field.selection_range(), Some((0, 3)));
        assert_eq!(field.cursor(), 3);
    }

    #[test]
    fn select_all_on_empty_buffer_has_no_selection() {
        let mut field = TextFieldState::new("");
        field.select_all();
        assert_eq!(field.selection_range(), None);
    }

    #[test]
    fn shift_arrow_extends_selection_one_char_at_a_time() {
        let mut field = TextFieldState::new("abcdef");
        field.move_home(false);
        field.move_right(true);
        field.move_right(true);
        assert_eq!(field.selection_range(), Some((0, 2)));
        field.move_left(true);
        assert_eq!(field.selection_range(), Some((0, 1)));
    }

    #[test]
    fn arrow_without_shift_collapses_selection_to_the_edge() {
        let mut field = TextFieldState::new("abcdef");
        field.click_at(1);
        field.drag_to(4);
        field.move_left(false);
        assert_eq!(field.cursor(), 1);
        assert_eq!(field.selection_range(), None);

        field.click_at(1);
        field.drag_to(4);
        field.move_right(false);
        assert_eq!(field.cursor(), 4);
        assert_eq!(field.selection_range(), None);
    }

    #[test]
    fn click_at_clears_any_existing_selection() {
        let mut field = TextFieldState::new("abcdef");
        field.click_at(1);
        field.drag_to(4);
        field.click_at(2);
        assert_eq!(field.cursor(), 2);
        assert_eq!(field.selection_range(), None);
    }

    #[test]
    fn drag_to_arms_anchor_on_first_call_and_survives_returning_to_start() {
        let mut field = TextFieldState::new("abcdef");
        field.click_at(2);
        field.drag_to(5);
        assert_eq!(field.selection_range(), Some((2, 5)));
        field.drag_to(2);
        assert_eq!(
            field.selection_range(),
            None,
            "de volta ao ponto de partida, sem seleção visível, mas o arraste continua armado"
        );
        field.drag_to(0);
        assert_eq!(
            field.selection_range(),
            Some((0, 2)),
            "o anchor continuou em 2, não foi perdido ao passar por cursor == anchor"
        );
    }

    #[test]
    fn clamp_to_boundary_never_lands_inside_a_multibyte_char() {
        let field = TextFieldState::new("á"); // 2 bytes em UTF-8
        assert_eq!(field.clamp_to_boundary(1), 0);
        assert_eq!(field.clamp_to_boundary(2), 2);
        assert_eq!(field.clamp_to_boundary(99), 2);
    }

    #[test]
    fn apply_text_field_key_select_all_on_primary_a() {
        let mut field = TextFieldState::new("abc");
        field.click_at(1);
        let logical_key = Key::Character("a".into());
        assert!(apply_text_field_key(
            &mut field,
            &logical_key,
            Some("a"),
            primary_mods(false)
        ));
        assert_eq!(field.selection_range(), Some((0, 3)));
    }

    #[test]
    fn apply_text_field_key_plain_a_inserts_char_not_select_all() {
        let mut field = TextFieldState::new("");
        let logical_key = Key::Character("a".into());
        assert!(apply_text_field_key(
            &mut field,
            &logical_key,
            Some("a"),
            mods(false, false)
        ));
        assert_eq!(field.text(), "a");
    }

    #[test]
    fn apply_text_field_key_shift_arrow_extends() {
        let mut field = TextFieldState::new("abc");
        field.move_home(false);
        let logical_key = Key::Named(NamedKey::ArrowRight);
        assert!(apply_text_field_key(
            &mut field,
            &logical_key,
            None,
            mods(false, true)
        ));
        assert_eq!(field.selection_range(), Some((0, 1)));
    }
}
