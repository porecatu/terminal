// SPDX-License-Identifier: GPL-3.0-or-later

//! O campo em edição na tela de configurações (ADR-0060 §3): o estado de
//! texto (`TextFieldState`, o mesmo do editor de grupo e da barra de busca),
//! de qual linha ele é, e as contas puras que o cercam -- o passo de `Up`/
//! `Down` num campo numérico e a escrita dos caracteres de controle.
//!
//! Um campo só tem um dono por vez: clicar em outro, `Tab`, `Enter` ou
//! Salvar **confirmam** o que está escrito (o valor vira pendência no
//! rascunho); `Esc` descarta a digitação e deixa a opção como estava.

use porecatu_config::EditValue;

use super::catalog::{Control, OptionDef};
use crate::app_field_menu::{FieldRef, apply_field_menu_action, place_caret_for_menu};
use crate::clipboard::ClipboardIo;
use crate::field_menu::{FIELD_MENU_ITEM_COUNT, FieldMenuAction, FieldMenuItem, field_menu_items};
use crate::text_field::TextFieldState;

/// Qual campo de uma linha está em edição. `git.remote_poll_interval_secs`
/// tem dois controles na mesma linha, e as listas têm um ou dois campos por
/// item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditPart {
    /// O campo de texto ou numérico da linha.
    Field,
    /// O número de segundos de `git.remote_poll_interval_secs`.
    GitSeconds,
    /// O primeiro campo do item `n` de uma lista: o texto, ou o nome de uma
    /// variável de ambiente.
    ListFirst(usize),
    /// O valor do item `n` da lista de nome e valor.
    ListSecond(usize),
    /// O filtro do grupo Atalhos (RF-16.28): vale a cada tecla.
    Filter,
}

impl EditPart {
    /// O item da lista que este campo é, se for de lista.
    pub(crate) fn list_item(self) -> Option<usize> {
        match self {
            EditPart::ListFirst(item) | EditPart::ListSecond(item) => Some(item),
            EditPart::Field | EditPart::GitSeconds | EditPart::Filter => None,
        }
    }

    /// O mesmo campo, no item `item`.
    pub(crate) fn at_item(self, item: usize) -> Self {
        match self {
            EditPart::ListFirst(_) => EditPart::ListFirst(item),
            EditPart::ListSecond(_) => EditPart::ListSecond(item),
            other => other,
        }
    }

    /// O campo seguinte (ou anterior) de uma lista de `count` itens, com
    /// `two_fields` campos por item -- a ordem de leitura: nome, valor,
    /// próximo item. `None` nas pontas: quem chama sai da lista.
    pub(crate) fn next_in_list(
        self,
        count: usize,
        two_fields: bool,
        backwards: bool,
    ) -> Option<Self> {
        let (item, second) = match self {
            EditPart::ListFirst(item) => (item, false),
            EditPart::ListSecond(item) => (item, true),
            EditPart::Field | EditPart::GitSeconds | EditPart::Filter => return None,
        };
        let per_item = if two_fields { 2 } else { 1 };
        let at = item * per_item + usize::from(second);
        let next = if backwards {
            at.checked_sub(1)?
        } else {
            at + 1
        };
        if next >= count * per_item {
            return None;
        }
        let (item, second) = (next / per_item, next % per_item == 1);
        Some(if second {
            EditPart::ListSecond(item)
        } else {
            EditPart::ListFirst(item)
        })
    }
}

/// Um campo em edição.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Editing {
    /// Índice do bloco da linha no painel.
    pub block: usize,
    pub part: EditPart,
    pub state: TextFieldState,
    /// O texto de quando a edição começou: confirmar sem tê-lo mudado não
    /// cria pendência.
    pub initial: String,
}

impl Editing {
    pub(crate) fn new(block: usize, part: EditPart, initial: String) -> Self {
        Self {
            block,
            part,
            state: TextFieldState::new(&initial),
            initial,
        }
    }

    /// A digitação mudou o texto desde que a edição começou.
    pub(crate) fn changed(&self) -> bool {
        self.state.text() != self.initial
    }

    /// The rows of the right-click menu over this field (ADR-0115 §7): Cut
    /// and Copy follow the selection.
    pub(crate) fn menu_items(&self) -> [FieldMenuItem; FIELD_MENU_ITEM_COUNT] {
        field_menu_items(self.state.selection_range().is_some(), false)
    }

    /// What a right click at `byte` does to the caret: inside the selection
    /// it stays, outside it the caret goes there (the rule the other windows
    /// follow, `app_field_menu::place_caret_for_menu`).
    pub(crate) fn place_caret_for_menu(&mut self, byte: usize) {
        place_caret_for_menu(&mut self.state, byte);
    }

    /// What an item of the menu does to this field: exactly what the key
    /// does (`field_clipboard::run_field_op`). These fields have no length
    /// cap of their own.
    pub(crate) fn run_menu_action(&mut self, action: FieldMenuAction, io: &mut impl ClipboardIo) {
        apply_field_menu_action(
            FieldRef::Plain {
                field: &mut self.state,
                max_chars: None,
            },
            action,
            io,
        );
    }
}

/// What the right-click menu of a settings window is over. Only a name: the
/// field is looked up in live state at every use, so the menu never outlives
/// it ([`field_menu_target_alive`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuField {
    /// The field being edited (`editing`): text, number, list item, filter.
    Editing,
    /// The masked field of the relay token (the screen only).
    Token,
}

/// Is the target of an open field menu still there? The menu closes when the
/// edit ends (confirmed, cancelled, discarded, the group changed) and when a
/// modal surface -- a dialog, the choice list, a shortcut capture, the profile
/// editor over the screen -- takes the window.
pub(crate) fn field_menu_target_alive(
    target: MenuField,
    editing: bool,
    token_focused: bool,
    modal_open: bool,
) -> bool {
    if modal_open {
        return false;
    }
    match target {
        MenuField::Editing => editing,
        MenuField::Token => token_focused,
    }
}

/// O campo desta opção escreve os caracteres de controle em vez de
/// desenhá-los: só os separadores de palavra, cujo padrão tem tabulação e
/// quebra de linha. Nos outros campos a barra invertida é um caractere como
/// outro -- um caminho do Windows não pode virar `C:\\Windows`.
pub(crate) fn writes_controls(option: &OptionDef) -> bool {
    matches!(option.default_value(), EditValue::String(text) if text.contains(['\t', '\n', '\r']))
}

/// O texto de um campo como a tela o mostra: com os caracteres de controle
/// escritos (barra e a letra) nas opções que os têm.
pub(crate) fn display_text(option: &OptionDef, text: &str) -> String {
    if writes_controls(option) {
        text.replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    } else {
        text.to_owned()
    }
}

/// O inverso de [`display_text`]: o que o usuário escreveu vira o valor. Uma
/// barra seguida de outra letra, ou solta no fim, fica como foi digitada.
pub(crate) fn parse_text(option: &OptionDef, text: &str) -> String {
    if !writes_controls(option) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.clone().next() {
            Some('\\') => {
                out.push('\\');
                chars.next();
            }
            Some('t') => {
                out.push('\t');
                chars.next();
            }
            Some('n') => {
                out.push('\n');
                chars.next();
            }
            Some('r') => {
                out.push('\r');
                chars.next();
            }
            _ => out.push('\\'),
        }
    }
    out
}

/// O texto de um número como o campo o mostra: inteiro sem decimais, decimal
/// sem os zeros que sobram (`14`, `1.2`, `0.05`).
pub(crate) fn number_text(value: &EditValue, float: bool) -> String {
    match value {
        EditValue::Integer(number) => number.to_string(),
        EditValue::Float(number) if float => {
            let text = format!("{number:.2}");
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        }
        EditValue::Float(number) => format!("{number}"),
        _ => String::new(),
    }
}

/// `Up`/`Down` num campo numérico: o texto com o passo da opção somado
/// (`direction` +1) ou subtraído (-1). `None` se o texto não é número -- não
/// há de onde partir -- ou a opção não é numérica. O resultado **não** é
/// recortado à faixa: passar do limite vira valor recusado, como digitado.
pub(crate) fn step_number(option: &OptionDef, text: &str, direction: i32) -> Option<String> {
    let Control::Number { step, float, .. } = option.control else {
        return None;
    };
    let current: f64 = text.trim().parse().ok().filter(|n: &f64| n.is_finite())?;
    // Soma em décimos de milésimo para `0.1 + 0.2` não virar `0.30000000000000004`.
    let next = ((current + f64::from(direction) * step) * 10_000.0).round() / 10_000.0;
    Some(if float {
        number_text(&EditValue::Float(next), true)
    } else {
        number_text(&EditValue::Integer(next as i64), false)
    })
}

#[cfg(test)]
mod tests {
    use super::super::catalog::option;
    use super::*;

    fn opt(id: &str) -> &'static OptionDef {
        option(id).unwrap()
    }

    #[test]
    fn only_the_word_separators_write_their_control_characters() {
        assert!(writes_controls(opt("word_separators")));
        assert!(!writes_controls(opt("font_family")));
        assert!(!writes_controls(opt("shell_program")));
    }

    #[test]
    fn a_windows_path_keeps_its_backslashes_in_a_plain_field() {
        let path = r"C:\temp\new";
        assert_eq!(display_text(opt("shell_program"), path), path);
        assert_eq!(parse_text(opt("shell_program"), path), path);
    }

    #[test]
    fn control_characters_round_trip_through_the_written_form() {
        let separators = opt("word_separators");
        let value = " \t\n,\\;";
        let shown = display_text(separators, value);
        assert_eq!(shown, " \\t\\n,\\\\;");
        assert_eq!(parse_text(separators, &shown), value);
    }

    #[test]
    fn a_stray_backslash_stays_as_typed() {
        let separators = opt("word_separators");
        assert_eq!(parse_text(separators, "a\\x"), "a\\x");
        assert_eq!(parse_text(separators, "a\\"), "a\\");
    }

    #[test]
    fn up_and_down_add_and_subtract_the_step_of_the_option() {
        // `font_size`: decimal, passo 0.5 (ver o catálogo).
        let size = opt("font_size");
        let Control::Number { step, .. } = size.control else {
            panic!()
        };
        let up = step_number(size, "14", 1).unwrap();
        assert_eq!(up.parse::<f64>().unwrap(), 14.0 + step);
        let down = step_number(size, "14", -1).unwrap();
        assert_eq!(down.parse::<f64>().unwrap(), 14.0 - step);
    }

    #[test]
    fn stepping_does_not_clamp_to_the_range() {
        let size = opt("font_size");
        let Control::Number { max, step, .. } = size.control else {
            panic!()
        };
        let past = step_number(size, &number_text(&EditValue::Float(max), true), 1).unwrap();
        assert_eq!(past.parse::<f64>().unwrap(), max + step);
    }

    #[test]
    fn stepping_text_that_is_not_a_number_does_nothing() {
        assert_eq!(step_number(opt("font_size"), "abc", 1), None);
        assert_eq!(step_number(opt("font_family"), "14", 1), None);
    }

    #[test]
    fn integer_options_step_without_a_decimal_part() {
        let scrollback = opt("scrollback_lines");
        let next = step_number(scrollback, "10000", 1).unwrap();
        assert!(!next.contains('.'));
        assert!(next.parse::<i64>().unwrap() > 10_000);
    }

    #[test]
    fn leaving_the_text_alone_is_not_a_change() {
        let mut editing = Editing::new(3, EditPart::Field, "14".to_owned());
        assert!(!editing.changed());
        editing.state.insert_char('5');
        assert!(editing.changed());
    }

    #[test]
    fn list_fields_walk_name_value_then_the_next_item() {
        use EditPart::{ListFirst as First, ListSecond as Second};
        // Dois itens de nome e valor: nome, valor, nome, valor.
        assert_eq!(First(0).next_in_list(2, true, false), Some(Second(0)));
        assert_eq!(Second(0).next_in_list(2, true, false), Some(First(1)));
        assert_eq!(Second(1).next_in_list(2, true, false), None);
        assert_eq!(First(1).next_in_list(2, true, true), Some(Second(0)));
        assert_eq!(First(0).next_in_list(2, true, true), None);
        // Uma lista de textos tem um campo por item.
        assert_eq!(First(0).next_in_list(3, false, false), Some(First(1)));
        assert_eq!(First(2).next_in_list(3, false, false), None);
        assert_eq!(First(2).next_in_list(3, false, true), Some(First(1)));
        // Campo que não é de lista não anda.
        assert_eq!(EditPart::Field.next_in_list(3, false, false), None);
    }

    #[test]
    fn a_list_field_moves_to_another_item_keeping_its_column() {
        assert_eq!(EditPart::ListSecond(1).at_item(4), EditPart::ListSecond(4));
        assert_eq!(EditPart::ListFirst(1).at_item(0), EditPart::ListFirst(0));
        assert_eq!(EditPart::Field.at_item(3), EditPart::Field);
        assert_eq!(EditPart::ListSecond(2).list_item(), Some(2));
        assert_eq!(EditPart::GitSeconds.list_item(), None);
    }

    // ---- the field menu (ADR-0115 §7)

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

    fn selecting(text: &str, from: usize, to: usize) -> Editing {
        let mut editing = Editing::new(0, EditPart::Field, text.to_owned());
        editing.state.click_at(from);
        editing.state.drag_to(to);
        editing
    }

    fn enabled(editing: &Editing) -> Vec<bool> {
        editing.menu_items().iter().map(|i| i.enabled).collect()
    }

    #[test]
    fn the_menu_dims_cut_and_copy_without_a_selection() {
        let mut editing = Editing::new(0, EditPart::Field, "14".to_owned());
        editing.state.click_at(0);
        assert_eq!(enabled(&editing), [false, false, true, true]);
        editing.state.select_all();
        assert_eq!(enabled(&editing), [true; 4]);
    }

    #[test]
    fn a_right_click_inside_the_selection_keeps_it_and_outside_moves_the_caret() {
        let mut editing = selecting("hello world", 2, 8);
        editing.place_caret_for_menu(5);
        assert_eq!(editing.state.selection_range(), Some((2, 8)));
        editing.place_caret_for_menu(10);
        assert_eq!(editing.state.selection_range(), None);
        assert_eq!(editing.state.cursor(), 10);
    }

    #[test]
    fn the_items_cut_copy_paste_and_select_all_act_on_the_field_in_edit() {
        let mut io = FakeClipboard::default();
        let mut editing = selecting("hello world", 0, 6);
        editing.run_menu_action(FieldMenuAction::Copy, &mut io);
        assert_eq!(io.writes, ["hello "]);
        assert_eq!(editing.state.text(), "hello world");

        editing.run_menu_action(FieldMenuAction::Cut, &mut io);
        assert_eq!(editing.state.text(), "world");
        assert_eq!(io.writes.len(), 2);

        let mut io = FakeClipboard::holding("a\r\nb\tc");
        editing.state.click_at(0);
        editing.run_menu_action(FieldMenuAction::Paste, &mut io);
        assert_eq!(editing.state.text(), "a b cworld");

        editing.run_menu_action(FieldMenuAction::SelectAll, &mut io);
        assert_eq!(
            editing.state.selection_range(),
            Some((0, editing.state.text().len()))
        );
    }

    #[test]
    fn an_empty_clipboard_pastes_nothing_into_the_field_in_edit() {
        let mut editing = Editing::new(0, EditPart::Field, "keep".to_owned());
        for mut io in [FakeClipboard::default(), FakeClipboard::holding("")] {
            editing.run_menu_action(FieldMenuAction::Paste, &mut io);
        }
        assert_eq!(editing.state.text(), "keep");
    }

    #[test]
    fn the_menu_lives_only_while_its_field_does() {
        use MenuField::{Editing as Edit, Token};
        // The field being edited: the edit has to be there.
        assert!(field_menu_target_alive(Edit, true, false, false));
        assert!(!field_menu_target_alive(Edit, false, false, false));
        // The token: its field has to hold the focus.
        assert!(field_menu_target_alive(Token, false, true, false));
        assert!(!field_menu_target_alive(Token, true, false, false));
        // Any modal surface takes the menu with it.
        assert!(!field_menu_target_alive(Edit, true, false, true));
        assert!(!field_menu_target_alive(Token, false, true, true));
    }
}
