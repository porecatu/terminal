// SPDX-License-Identifier: GPL-3.0-or-later

//! Popover de sessões nomeadas (PRD-014, ADR-0054, ADR-0055), sétimo
//! widget de chrome: estado puro, testável sem `winit` -- mesmo padrão de
//! `move_to_group.rs`/`group_editor.rs`. Layout, pintura e hit test moram
//! em `overlay.rs`; a ligação com o botão, as duas ações
//! (`session.save_named`/`session.open_list`) e a exclusão mútua com os
//! outros widgets é `lib.rs`.
//!
//! Diferente do popover de destino (`move_to_group.rs`, que deriva a
//! janela visível de `highlighted` sem guardar rolagem própria), este
//! guarda `scroll_top`: a roda do mouse rola a lista sem mexer no realce
//! (`scroll_by`), um gesto independente da navegação por teclado
//! (`move_highlight`) -- não o "roda = seta" que o popover de destino usa.
//!
//! Este módulo só decide **o quê** aconteceu (`PickerOutcome`); nenhuma
//! variante chama `porecatu_session::named` -- isso é `App::
//! restore_named_session`/`save_named_session`/`delete_named`, todos
//! ligados a partir de `App::resolve_session_picker_outcome` em `lib.rs`
//! (ADR-0055 §3).

use porecatu_session::named::{EntryStatus, MAX_NAME_CHARS, NamedSessionEntry};
use porecatu_term::Modifiers;
use winit::keyboard::{Key, NamedKey};

use crate::text_field::{TextFieldState, apply_text_field_key};
use std::path::PathBuf;

/// O que está realçado -- o item fixo "Salvar esta janela..." ou uma
/// linha da lista, por índice em [`SessionPicker::entries`]. A linha
/// "nenhuma sessão salva" (lista vazia) nunca é um `Row`: ela não é alvo
/// (ADR-0055 §2, "sem hover e sem alvo").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Highlight {
    Save,
    Row(usize),
}

/// O item fixo do topo em navegação, ou o campo de nome no lugar dele
/// (ADR-0055 §2, item 1 "em modo de edição, o item vira o campo de
/// texto").
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Browsing,
    EditingName(TextFieldState),
}

/// Efeito pedido por [`SessionPicker::handle_key`], sem nenhum efeito
/// colateral -- quem chama (`lib.rs`) decide o que fazer com ele.
#[derive(Debug, Clone, PartialEq)]
pub enum PickerOutcome {
    None,
    Close,
    Restore(PathBuf),
    RequestDelete(NamedSessionEntry),
    SubmitName(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionPicker {
    entries: Vec<NamedSessionEntry>,
    highlighted: Highlight,
    mode: Mode,
    /// Índice da primeira linha visível da lista -- só rolagem por roda
    /// (`scroll_by`); `move_highlight` ajusta isto por
    /// `ensure_highlight_visible`, mas não é o mesmo estado.
    scroll_top: usize,
}

impl SessionPicker {
    /// Abre em modo navegação (clique no botão, `session.open_list`):
    /// realce na primeira linha, ou no item de salvar se a lista estiver
    /// vazia -- RF-14.7/RF-14.9.
    pub fn open_browsing(entries: Vec<NamedSessionEntry>) -> Self {
        let highlighted = if entries.is_empty() {
            Highlight::Save
        } else {
            Highlight::Row(0)
        };
        Self {
            entries,
            highlighted,
            mode: Mode::Browsing,
            scroll_top: 0,
        }
    }

    /// Abre em modo edição (`session.save_named`, RF-14.1/RF-14.2): campo
    /// em foco, vazio, item de salvar realçado.
    pub fn open_editing(entries: Vec<NamedSessionEntry>) -> Self {
        Self {
            entries,
            highlighted: Highlight::Save,
            mode: Mode::EditingName(TextFieldState::new("")),
            scroll_top: 0,
        }
    }

    pub fn entries(&self) -> &[NamedSessionEntry] {
        &self.entries
    }

    pub const fn highlighted(&self) -> Highlight {
        self.highlighted
    }

    pub const fn mode(&self) -> &Mode {
        &self.mode
    }

    pub const fn scroll_top(&self) -> usize {
        self.scroll_top
    }

    /// Clique no item "Salvar esta janela..." em modo navegação
    /// (`lib.rs::dispatch_mouse_input`): entra em edição, mesmo efeito de
    /// `Enter` com o item de salvar realçado -- clique num item de menu
    /// já é a escolha (nota do módulo), não só realce.
    pub fn enter_editing(&mut self) {
        self.mode = Mode::EditingName(TextFieldState::new(""));
    }

    /// Clique numa linha: realça e devolve o mesmo efeito de `Enter`
    /// sobre ela (RF-14.17: linha ruim nunca restaura).
    pub fn click_row(&mut self, index: usize, max_visible_rows: usize) -> PickerOutcome {
        self.set_highlight(Highlight::Row(index), max_visible_rows);
        match self.entries.get(index) {
            Some(entry) if entry.status == EntryStatus::Ok => {
                PickerOutcome::Restore(entry.file.clone())
            }
            _ => PickerOutcome::None,
        }
    }

    /// Clique no `X` de uma linha: pede exclusão sem mexer no realce --
    /// excluir não é "escolher" a linha.
    pub fn click_delete(&mut self, index: usize) -> PickerOutcome {
        self.entries
            .get(index)
            .cloned()
            .map(PickerOutcome::RequestDelete)
            .unwrap_or(PickerOutcome::None)
    }

    /// Clique dentro do campo em modo de edição -- posiciona o cursor
    /// (ADR-0035), mesmo padrão de `GroupEditor::click_name_at`. No-op
    /// fora desse modo (não deveria acontecer: o campo só existe
    /// enquanto `EditingName`).
    pub fn click_name_at(&mut self, byte_index: usize) {
        if let Mode::EditingName(field) = &mut self.mode {
            field.click_at(byte_index);
        }
    }

    /// Arraste dentro do campo -- mesmo padrão de
    /// `GroupEditor::drag_name_to`.
    pub fn drag_name_to(&mut self, byte_index: usize) {
        if let Mode::EditingName(field) = &mut self.mode {
            field.drag_to(byte_index);
        }
    }

    /// `Up`/`Down` (delta `-1`/`1`): move entre o item de salvar e as
    /// linhas, pulando "nenhuma sessão salva" -- lista vazia sempre
    /// realça o item de salvar, sem para onde mover. `max_visible_rows`
    /// vem de `[appearance.session_picker]`, lido por quem chama
    /// (`lib.rs`) -- este módulo não conhece `porecatu-config`.
    pub fn move_highlight(&mut self, delta: isize, max_visible_rows: usize) {
        if self.entries.is_empty() {
            self.highlighted = Highlight::Save;
            return;
        }
        let len = self.entries.len() as isize;
        // Anel de `len + 1` posições: 0 é o item de salvar, 1..=len são
        // as linhas (índice `n - 1`).
        let total = len + 1;
        let current = match self.highlighted {
            Highlight::Save => 0,
            Highlight::Row(i) => i as isize + 1,
        };
        let next = (current + delta).rem_euclid(total);
        self.highlighted = if next == 0 {
            Highlight::Save
        } else {
            Highlight::Row((next - 1) as usize)
        };
        self.ensure_highlight_visible(max_visible_rows);
    }

    /// Hover do mouse: mesmo realce do teclado, mutuamente exclusivo
    /// (ADR-0055 §3) -- clicar ou passar o mouse sobre uma linha realça
    /// ela. Índice fora de `entries` é ignorado (defesa contra um layout
    /// obsoleto entre frames).
    pub fn set_highlight(&mut self, highlight: Highlight, max_visible_rows: usize) {
        match highlight {
            Highlight::Save => self.highlighted = Highlight::Save,
            Highlight::Row(i) if i < self.entries.len() => self.highlighted = Highlight::Row(i),
            Highlight::Row(_) => return,
        }
        self.ensure_highlight_visible(max_visible_rows);
    }

    /// Arrasta `scroll_top` o mínimo necessário para `highlighted` (se
    /// for uma linha) caber na janela de `max_visible_rows` -- mesmo
    /// espírito de `MoveToGroupPopover`/`WindowState::
    /// ensure_active_tab_visible`, mas guardado em vez de derivado (nota
    /// do módulo).
    fn ensure_highlight_visible(&mut self, max_visible_rows: usize) {
        let Highlight::Row(i) = self.highlighted else {
            return;
        };
        if max_visible_rows == 0 {
            return;
        }
        if i < self.scroll_top {
            self.scroll_top = i;
        } else if i >= self.scroll_top + max_visible_rows {
            self.scroll_top = i + 1 - max_visible_rows;
        }
    }

    /// RF-14.16: depois de excluir de verdade (`App::
    /// commit_named_session_delete`), recarrega a lista inteira e mantém
    /// o popover aberto com o realce numa **linha vizinha válida** --
    /// posição de `deleted_file` na lista **antiga** (não o realce atual:
    /// o `X` clicado não precisa ser o da linha realçada, RF-14.16 "visível
    /// sob o cursor **ou** na linha realçada"), clampada ao novo tamanho --
    /// aterrissa na linha que ocupava o lugar da excluída, ou na nova
    /// última se era a última. Sem `deleted_file` na lista antiga (não
    /// deveria acontecer) ou lista vazia depois: primeira linha, ou o
    /// item de salvar se não sobrou nenhuma.
    pub fn reload_after_delete(
        &mut self,
        entries: Vec<NamedSessionEntry>,
        deleted_file: &std::path::Path,
        max_visible_rows: usize,
    ) {
        let deleted_index = self.entries.iter().position(|e| e.file == deleted_file);
        self.entries = entries;
        self.highlighted = match deleted_index {
            Some(i) if !self.entries.is_empty() => Highlight::Row(i.min(self.entries.len() - 1)),
            _ if !self.entries.is_empty() => Highlight::Row(0),
            _ => Highlight::Save,
        };
        self.ensure_highlight_visible(max_visible_rows);
    }

    /// Roda do mouse: rola a lista sem tocar o realce -- gesto
    /// independente do teclado (nota do módulo). `delta` em linhas,
    /// positivo rola para baixo.
    pub fn scroll_by(&mut self, delta: isize, max_visible_rows: usize) {
        let max_top = self.entries.len().saturating_sub(max_visible_rows);
        let next = (self.scroll_top as isize + delta).clamp(0, max_top as isize);
        self.scroll_top = next as usize;
    }

    /// Tradução de uma tecla pressionada em efeito, sem nenhum efeito
    /// colateral fora deste `SessionPicker` -- `lib.rs` decide o que
    /// fazer com o `PickerOutcome` (ADR-0055 §3).
    ///
    /// Em modo de edição, `Esc` não fecha o popover: ele só sai da
    /// edição, voltando à navegação com o item de salvar realçado. A
    /// reatribuição de `self.mode`/`self.highlighted` fica **fora** do
    /// `if let` que empresta `field` (flag `cancel_editing` em vez de
    /// mexer em `self.mode` com o empréstimo ainda vivo) -- mais simples
    /// que provar pro borrow checker que o último uso de `field` termina
    /// antes da reatribuição.
    pub fn handle_key(
        &mut self,
        key: &Key,
        text: Option<&str>,
        modifiers: Modifiers,
        max_visible_rows: usize,
    ) -> PickerOutcome {
        let mut cancel_editing = false;
        if let Mode::EditingName(field) = &mut self.mode {
            match key {
                Key::Named(NamedKey::Enter) => {
                    let name = field.text().trim().to_string();
                    return if name.is_empty() {
                        PickerOutcome::None
                    } else {
                        PickerOutcome::SubmitName(name)
                    };
                }
                Key::Named(NamedKey::Escape) => cancel_editing = true,
                // Não é `apply_text_field_key`: essa função não trata
                // `Backspace` (mesmo motivo de `GroupEditor::backspace`
                // tratar a tecla à parte).
                Key::Named(NamedKey::Backspace) => {
                    field.backspace();
                    return PickerOutcome::None;
                }
                _ => {
                    // RF-14.3: "para de aceitar texto no teto, sem
                    // aviso" -- suprimir só a inserção (`text: None`)
                    // deixa a navegação do campo (setas, `Home`/`End`,
                    // `Ctrl+A`) intacta, porque nenhuma delas lê `text`.
                    let capped_text = if field.text().chars().count() >= MAX_NAME_CHARS {
                        None
                    } else {
                        text
                    };
                    apply_text_field_key(field, key, capped_text, modifiers);
                    return PickerOutcome::None;
                }
            }
        }
        if cancel_editing {
            self.mode = Mode::Browsing;
            self.highlighted = Highlight::Save;
            return PickerOutcome::None;
        }

        match key {
            Key::Named(NamedKey::Escape) => PickerOutcome::Close,
            Key::Named(NamedKey::ArrowUp) => {
                self.move_highlight(-1, max_visible_rows);
                PickerOutcome::None
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.move_highlight(1, max_visible_rows);
                PickerOutcome::None
            }
            Key::Named(NamedKey::Enter) => match self.highlighted {
                Highlight::Save => {
                    self.mode = Mode::EditingName(TextFieldState::new(""));
                    PickerOutcome::None
                }
                Highlight::Row(i) => match self.entries.get(i) {
                    // RF-14.17: linha ruim nunca restaura, mesmo
                    // realçada e mesmo com `Enter`.
                    Some(entry) if entry.status == EntryStatus::Ok => {
                        PickerOutcome::Restore(entry.file.clone())
                    }
                    _ => PickerOutcome::None,
                },
            },
            Key::Named(NamedKey::Delete) => match self.highlighted {
                Highlight::Save => PickerOutcome::None,
                Highlight::Row(i) => self
                    .entries
                    .get(i)
                    .cloned()
                    .map(PickerOutcome::RequestDelete)
                    .unwrap_or(PickerOutcome::None),
            },
            _ => PickerOutcome::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_entry(name: &str) -> NamedSessionEntry {
        NamedSessionEntry {
            name: name.to_string(),
            file: PathBuf::from(format!("{name}.json")),
            saved_at: Some(1),
            status: EntryStatus::Ok,
        }
    }

    fn bad_entry(name: &str) -> NamedSessionEntry {
        NamedSessionEntry {
            name: name.to_string(),
            file: PathBuf::from(format!("{name}.json")),
            saved_at: None,
            status: EntryStatus::Unreadable,
        }
    }

    fn no_mods() -> Modifiers {
        Modifiers {
            shift: false,
            ctrl: false,
            alt: false,
            super_: false,
        }
    }

    fn key(picker: &mut SessionPicker, k: Key) -> PickerOutcome {
        picker.handle_key(&k, None, no_mods(), 6)
    }

    #[test]
    fn open_browsing_highlights_first_row_when_entries_exist() {
        let picker = SessionPicker::open_browsing(vec![ok_entry("a")]);
        assert_eq!(picker.highlighted(), Highlight::Row(0));
        assert_eq!(picker.mode(), &Mode::Browsing);
    }

    #[test]
    fn open_browsing_highlights_save_item_when_list_is_empty() {
        let picker = SessionPicker::open_browsing(vec![]);
        assert_eq!(picker.highlighted(), Highlight::Save);
    }

    #[test]
    fn open_editing_starts_with_empty_focused_field() {
        let picker = SessionPicker::open_editing(vec![ok_entry("a")]);
        assert_eq!(picker.highlighted(), Highlight::Save);
        match picker.mode() {
            Mode::EditingName(field) => assert_eq!(field.text(), ""),
            Mode::Browsing => panic!("esperava EditingName"),
        }
    }

    #[test]
    fn move_highlight_cycles_between_save_item_and_rows() {
        let mut picker = SessionPicker::open_browsing(vec![ok_entry("a"), ok_entry("b")]);
        assert_eq!(picker.highlighted(), Highlight::Row(0));
        picker.move_highlight(-1, 6);
        assert_eq!(
            picker.highlighted(),
            Highlight::Save,
            "subir a partir da primeira linha vai pro item de salvar"
        );
        picker.move_highlight(-1, 6);
        assert_eq!(
            picker.highlighted(),
            Highlight::Row(1),
            "subir a partir do item de salvar volta pra última linha (anel)"
        );
        picker.move_highlight(1, 6);
        assert_eq!(picker.highlighted(), Highlight::Save);
        picker.move_highlight(1, 6);
        assert_eq!(picker.highlighted(), Highlight::Row(0));
    }

    #[test]
    fn move_highlight_on_empty_list_always_stays_on_save() {
        let mut picker = SessionPicker::open_browsing(vec![]);
        picker.move_highlight(1, 6);
        assert_eq!(picker.highlighted(), Highlight::Save);
        picker.move_highlight(-1, 6);
        assert_eq!(picker.highlighted(), Highlight::Save);
    }

    #[test]
    fn ensure_highlight_visible_scrolls_down_past_the_visible_window() {
        let entries = (0..10).map(|i| ok_entry(&i.to_string())).collect();
        let mut picker = SessionPicker::open_browsing(entries);
        for _ in 0..5 {
            picker.move_highlight(1, 3);
        }
        assert_eq!(picker.highlighted(), Highlight::Row(5));
        assert_eq!(
            picker.scroll_top(),
            3,
            "janela de 3 linhas arrasta o mínimo pra linha 5 caber (3,4,5)"
        );
    }

    #[test]
    fn ensure_highlight_visible_scrolls_up_when_highlight_moves_above_window() {
        let entries = (0..10).map(|i| ok_entry(&i.to_string())).collect();
        let mut picker = SessionPicker::open_browsing(entries);
        for _ in 0..5 {
            picker.move_highlight(1, 3);
        }
        assert_eq!(picker.scroll_top(), 3);
        for _ in 0..5 {
            picker.move_highlight(-1, 3);
        }
        assert_eq!(picker.highlighted(), Highlight::Row(0));
        assert_eq!(picker.scroll_top(), 0);
    }

    #[test]
    fn scroll_by_moves_the_window_without_touching_highlight() {
        let entries = (0..10).map(|i| ok_entry(&i.to_string())).collect();
        let mut picker = SessionPicker::open_browsing(entries);
        picker.scroll_by(2, 3);
        assert_eq!(picker.scroll_top(), 2);
        assert_eq!(
            picker.highlighted(),
            Highlight::Row(0),
            "roda não mexe no realce (nota do módulo)"
        );
    }

    #[test]
    fn scroll_by_clamps_to_the_valid_range() {
        let entries = (0..10).map(|i| ok_entry(&i.to_string())).collect();
        let mut picker = SessionPicker::open_browsing(entries);
        picker.scroll_by(-5, 3);
        assert_eq!(picker.scroll_top(), 0);
        picker.scroll_by(999, 3);
        assert_eq!(picker.scroll_top(), 7, "máximo: 10 linhas - 3 visíveis");
    }

    #[test]
    fn enter_on_save_item_opens_editing() {
        let mut picker = SessionPicker::open_browsing(vec![]);
        let outcome = key(&mut picker, Key::Named(NamedKey::Enter));
        assert_eq!(outcome, PickerOutcome::None);
        assert!(matches!(picker.mode(), Mode::EditingName(_)));
    }

    #[test]
    fn enter_on_ok_row_restores() {
        let mut picker = SessionPicker::open_browsing(vec![ok_entry("a")]);
        let outcome = key(&mut picker, Key::Named(NamedKey::Enter));
        assert_eq!(outcome, PickerOutcome::Restore(PathBuf::from("a.json")));
    }

    #[test]
    fn enter_on_bad_row_never_restores() {
        let mut picker = SessionPicker::open_browsing(vec![bad_entry("quebrada")]);
        let outcome = key(&mut picker, Key::Named(NamedKey::Enter));
        assert_eq!(outcome, PickerOutcome::None);
    }

    #[test]
    fn delete_on_any_row_status_requests_delete() {
        let mut picker = SessionPicker::open_browsing(vec![bad_entry("quebrada")]);
        let outcome = key(&mut picker, Key::Named(NamedKey::Delete));
        assert_eq!(
            outcome,
            PickerOutcome::RequestDelete(bad_entry("quebrada")),
            "linha ruim também pode ser excluída"
        );
    }

    #[test]
    fn delete_on_save_item_is_a_no_op() {
        let mut picker = SessionPicker::open_browsing(vec![]);
        let outcome = key(&mut picker, Key::Named(NamedKey::Delete));
        assert_eq!(outcome, PickerOutcome::None);
    }

    #[test]
    fn escape_in_browsing_closes() {
        let mut picker = SessionPicker::open_browsing(vec![ok_entry("a")]);
        let outcome = key(&mut picker, Key::Named(NamedKey::Escape));
        assert_eq!(outcome, PickerOutcome::Close);
    }

    #[test]
    fn escape_while_editing_returns_to_browsing_instead_of_closing() {
        let mut picker = SessionPicker::open_editing(vec![ok_entry("a")]);
        let outcome = key(&mut picker, Key::Named(NamedKey::Escape));
        assert_eq!(outcome, PickerOutcome::None);
        assert_eq!(picker.mode(), &Mode::Browsing);
        assert_eq!(picker.highlighted(), Highlight::Save);
    }

    #[test]
    fn enter_with_blank_name_does_nothing() {
        let mut picker = SessionPicker::open_editing(vec![]);
        let outcome = picker.handle_key(&Key::Named(NamedKey::Enter), None, no_mods(), 6);
        assert_eq!(outcome, PickerOutcome::None);
    }

    #[test]
    fn enter_with_trimmed_name_submits() {
        let mut picker = SessionPicker::open_editing(vec![]);
        for c in "  api ".chars() {
            picker.handle_key(
                &Key::Character(c.to_string().into()),
                Some(&c.to_string()),
                no_mods(),
                6,
            );
        }
        let outcome = key(&mut picker, Key::Named(NamedKey::Enter));
        assert_eq!(outcome, PickerOutcome::SubmitName("api".to_string()));
    }

    #[test]
    fn name_field_stops_accepting_text_at_the_char_ceiling() {
        let mut picker = SessionPicker::open_editing(vec![]);
        for _ in 0..MAX_NAME_CHARS + 5 {
            picker.handle_key(&Key::Character("a".into()), Some("a"), no_mods(), 6);
        }
        match picker.mode() {
            Mode::EditingName(field) => assert_eq!(field.text().chars().count(), MAX_NAME_CHARS),
            Mode::Browsing => panic!("esperava EditingName"),
        }
    }

    #[test]
    fn arrows_still_work_at_the_char_ceiling() {
        let mut picker = SessionPicker::open_editing(vec![]);
        for _ in 0..MAX_NAME_CHARS {
            picker.handle_key(&Key::Character("a".into()), Some("a"), no_mods(), 6);
        }
        picker.handle_key(&Key::Named(NamedKey::ArrowLeft), None, no_mods(), 6);
        match picker.mode() {
            Mode::EditingName(field) => {
                assert_eq!(
                    field.cursor(),
                    MAX_NAME_CHARS - 1,
                    "seta ainda move o cursor no teto"
                );
            }
            Mode::Browsing => panic!("esperava EditingName"),
        }
    }

    #[test]
    fn backspace_works_while_editing() {
        let mut picker = SessionPicker::open_editing(vec![]);
        picker.handle_key(&Key::Character("x".into()), Some("x"), no_mods(), 6);
        picker.handle_key(&Key::Named(NamedKey::Backspace), None, no_mods(), 6);
        match picker.mode() {
            Mode::EditingName(field) => assert_eq!(field.text(), ""),
            Mode::Browsing => panic!("esperava EditingName"),
        }
    }

    #[test]
    fn click_row_highlights_and_requests_restore() {
        let mut picker = SessionPicker::open_browsing(vec![ok_entry("a"), ok_entry("b")]);
        let outcome = picker.click_row(1, 6);
        assert_eq!(picker.highlighted(), Highlight::Row(1));
        assert_eq!(outcome, PickerOutcome::Restore(PathBuf::from("b.json")));
    }

    #[test]
    fn click_delete_does_not_change_highlight() {
        let mut picker = SessionPicker::open_browsing(vec![ok_entry("a"), ok_entry("b")]);
        let outcome = picker.click_delete(1);
        assert_eq!(picker.highlighted(), Highlight::Row(0), "realce não mudou");
        assert_eq!(outcome, PickerOutcome::RequestDelete(ok_entry("b")));
    }

    #[test]
    fn enter_editing_switches_mode_with_empty_field() {
        let mut picker = SessionPicker::open_browsing(vec![ok_entry("a")]);
        picker.enter_editing();
        match picker.mode() {
            Mode::EditingName(field) => assert_eq!(field.text(), ""),
            Mode::Browsing => panic!("esperava EditingName"),
        }
    }

    #[test]
    fn reload_after_delete_highlights_the_row_that_took_the_deleted_ones_place() {
        let mut picker =
            SessionPicker::open_browsing(vec![ok_entry("a"), ok_entry("b"), ok_entry("c")]);
        picker.set_highlight(Highlight::Row(1), 6);
        // "b" (índice 1) foi excluída; "c" tomou o lugar dela.
        let remaining = vec![ok_entry("a"), ok_entry("c")];
        picker.reload_after_delete(remaining, &PathBuf::from("b.json"), 6);
        assert_eq!(picker.highlighted(), Highlight::Row(1));
        assert_eq!(picker.entries()[1].name, "c");
    }

    #[test]
    fn reload_after_delete_clamps_when_the_last_row_was_deleted() {
        let mut picker = SessionPicker::open_browsing(vec![ok_entry("a"), ok_entry("b")]);
        picker.set_highlight(Highlight::Row(1), 6);
        let remaining = vec![ok_entry("a")];
        picker.reload_after_delete(remaining, &PathBuf::from("b.json"), 6);
        assert_eq!(
            picker.highlighted(),
            Highlight::Row(0),
            "última linha excluída cai na nova última"
        );
    }

    #[test]
    fn reload_after_delete_of_the_only_entry_highlights_save() {
        let mut picker = SessionPicker::open_browsing(vec![ok_entry("a")]);
        picker.reload_after_delete(vec![], &PathBuf::from("a.json"), 6);
        assert_eq!(picker.highlighted(), Highlight::Save);
    }

    #[test]
    fn reload_after_delete_ignores_the_current_highlight_and_follows_the_deleted_file() {
        // Excluir pelo `X` de uma linha que não é a realçada (RF-14.16:
        // "visível sob o cursor **ou** na linha realçada") -- o realce
        // segue a excluída, não o realce anterior.
        let mut picker =
            SessionPicker::open_browsing(vec![ok_entry("a"), ok_entry("b"), ok_entry("c")]);
        picker.set_highlight(Highlight::Row(0), 6);
        let remaining = vec![ok_entry("a"), ok_entry("b")];
        picker.reload_after_delete(remaining, &PathBuf::from("c.json"), 6);
        assert_eq!(
            picker.highlighted(),
            Highlight::Row(1),
            "segue onde \"c\" estava (índice 2), clampado ao novo tamanho"
        );
    }
}
