// SPDX-License-Identifier: GPL-3.0-or-later

//! O que cada gesto sobre um controle faz ao rascunho (RF-16.12, RF-16.15):
//! alternar, escolher um segmento, ligar e desligar o Git, confirmar o texto
//! de um campo, somar o passo de um número. Funções sobre o `Draft`, sem
//! janela: a janela decide **qual** controle recebeu o gesto, e aqui está o
//! que ele muda -- o que se testa sem `winit`.
//!
//! Cada função devolve se o rascunho foi tocado; quem chama refaz o conteúdo
//! medido quando foi. Um valor recusado **fica** no rascunho, marcado na linha
//! (RF-16.18), e conta como tocado.

use porecatu_config::EditValue;

use super::catalog::{Control, GIT_POLL_MIN, OptionDef};
use super::draft::Draft;
use super::field_edit::{parse_text, step_number};

/// Alterna uma opção booleana.
pub(crate) fn toggle(draft: &mut Draft, option: &OptionDef) -> bool {
    let EditValue::Bool(on) = draft.value(option) else {
        return false;
    };
    let _ = draft.set(option, EditValue::Bool(!on));
    true
}

/// Escolhe o valor `segment` de uma escolha de até três valores.
pub(crate) fn choose_segment(draft: &mut Draft, option: &OptionDef, segment: usize) -> bool {
    let Control::Choice(choices) = option.control else {
        return false;
    };
    let Some(choice) = choices.get(segment) else {
        return false;
    };
    let _ = draft.set(option, EditValue::String((*choice).to_owned()));
    true
}

/// Escolhe um valor por nome -- o item de uma lista de escolha.
pub(crate) fn choose_value(draft: &mut Draft, option: &OptionDef, value: &str) -> bool {
    let _ = draft.set(option, EditValue::String(value.to_owned()));
    true
}

/// A alternância do Git: desligar grava `0`; ligar volta ao padrão (ou ao
/// piso, se o padrão fosse desligado).
pub(crate) fn toggle_git(draft: &mut Draft, option: &OptionDef) -> bool {
    let EditValue::Integer(seconds) = draft.value(option) else {
        return false;
    };
    let next = if seconds > 0 {
        0
    } else {
        match option.default_value() {
            EditValue::Integer(default) if default > 0 => default,
            _ => GIT_POLL_MIN,
        }
    };
    let _ = draft.set(option, EditValue::Integer(next));
    true
}

/// Confirma o texto de um campo: lê o que foi escrito (os caracteres de
/// controle escritos viram os caracteres) e o põe no rascunho.
pub(crate) fn commit_text(draft: &mut Draft, option: &OptionDef, text: &str) -> bool {
    let raw = parse_text(option, text);
    let _ = draft.set_raw(option, &raw);
    true
}

/// Muda o texto de um campo de um item de lista (RF-16.26): o primeiro campo
/// é o texto -- ou o nome da variável --, o segundo o valor dela. Texto igual
/// ao que já estava não toca em nada.
pub(crate) fn list_set_text(
    draft: &mut Draft,
    option: &OptionDef,
    item: usize,
    second: bool,
    text: &str,
) -> bool {
    let mut rows = draft.rows(option);
    let Some(row) = rows.get_mut(item) else {
        return false;
    };
    let field = if second { &mut row.1 } else { &mut row.0 };
    if field == text {
        return false;
    }
    *field = text.to_owned();
    let _ = draft.set_rows(option, &rows);
    true
}

/// Acrescenta um item vazio no fim da lista e devolve o índice dele. Numa
/// lista de nome e valor ele nasce recusado (nome vazio) até receber um nome.
pub(crate) fn list_add(draft: &mut Draft, option: &OptionDef) -> Option<usize> {
    if !matches!(option.control, Control::StringList | Control::StringMap) {
        return None;
    }
    let mut rows = draft.rows(option);
    rows.push((String::new(), String::new()));
    let _ = draft.set_rows(option, &rows);
    Some(rows.len() - 1)
}

/// Remove o item `item` da lista.
pub(crate) fn list_remove(draft: &mut Draft, option: &OptionDef, item: usize) -> bool {
    let mut rows = draft.rows(option);
    if item >= rows.len() {
        return false;
    }
    rows.remove(item);
    let _ = draft.set_rows(option, &rows);
    true
}

/// Move o item `item` uma posição (`delta` -1 sobe, 1 desce) e devolve o
/// índice novo, ou `None` na ponta da lista (`Alt+Up`/`Alt+Down`, RF-16.26).
pub(crate) fn list_move(
    draft: &mut Draft,
    option: &OptionDef,
    item: usize,
    delta: i32,
) -> Option<usize> {
    let mut rows = draft.rows(option);
    let target = item.checked_add_signed(delta as isize)?;
    if item >= rows.len() || target >= rows.len() {
        return None;
    }
    rows.swap(item, target);
    let _ = draft.set_rows(option, &rows);
    Some(target)
}

/// `Up`/`Down` num campo numérico: o passo da opção sobre `text`, posto no
/// rascunho. Devolve o texto novo, ou `None` se `text` não é número -- não há
/// de onde partir -- ou a opção não é numérica.
pub(crate) fn step_text(
    draft: &mut Draft,
    option: &OptionDef,
    text: &str,
    direction: i32,
) -> Option<String> {
    let next = step_number(option, text, direction)?;
    let _ = draft.set_raw(option, &next);
    Some(next)
}

#[cfg(test)]
mod tests {
    use porecatu_config::{Config, ConfigDocument, Edit, KeyPath};

    use super::super::catalog::option;
    use super::super::draft::ValueError;
    use super::*;

    fn opt(id: &str) -> &'static OptionDef {
        option(id).unwrap()
    }

    fn draft() -> Draft {
        Draft::new(&Config::default())
    }

    // ---- uma pendência por tipo de controle

    #[test]
    fn toggling_a_switch_makes_a_pending_edit_and_toggling_back_clears_it() {
        let mut d = draft();
        let blink = opt("cursor_blink");
        let default = d.value(blink);
        assert!(toggle(&mut d, blink));
        assert!(d.is_pending(blink));
        assert_ne!(d.value(blink), default);
        assert!(toggle(&mut d, blink));
        assert!(!d.is_pending(blink));
        assert!(!d.is_dirty());
    }

    #[test]
    fn choosing_a_segment_writes_the_named_value() {
        let mut d = draft();
        let shape = opt("cursor_shape");
        assert!(choose_segment(&mut d, shape, 1));
        assert_eq!(d.value(shape), EditValue::String("beam".to_owned()));
        assert!(d.is_pending(shape));
        // Um segmento que não existe não toca em nada.
        assert!(!choose_segment(&mut d, shape, 9));
        assert_eq!(d.value(shape), EditValue::String("beam".to_owned()));
        // E voltar ao do arquivo apaga a pendência.
        assert!(choose_segment(&mut d, shape, 0));
        assert!(!d.is_pending(shape));
    }

    #[test]
    fn a_text_field_commits_a_pending_string() {
        let mut d = draft();
        let family = opt("font_family");
        assert!(commit_text(&mut d, family, "Cascadia Code"));
        assert_eq!(
            d.value(family),
            EditValue::String("Cascadia Code".to_owned())
        );
        assert!(d.is_pending(family));
    }

    #[test]
    fn a_numeric_field_commits_a_float_and_keeps_a_refused_text() {
        let mut d = draft();
        let size = opt("font_size");
        commit_text(&mut d, size, "16");
        assert_eq!(d.value(size), EditValue::Float(16.0));
        // Fora da faixa: fica marcado, com o texto digitado.
        commit_text(&mut d, size, "900");
        assert!(matches!(
            d.invalid(size),
            Some(ValueError::OutOfRange { .. })
        ));
        assert_eq!(d.raw(size), Some("900"));
        assert!(d.has_invalid());
        // Malformado também.
        commit_text(&mut d, size, "abc");
        assert_eq!(d.invalid(size), Some(&ValueError::NotANumber));
    }

    #[test]
    fn the_word_separators_round_trip_their_control_characters() {
        let mut d = draft();
        let separators = opt("word_separators");
        // O campo mostra `\t` e `\n` escritos; o valor volta a ter os dois.
        commit_text(&mut d, separators, " \\t\\n,");
        assert_eq!(d.value(separators), EditValue::String(" \t\n,".to_owned()));
    }

    #[test]
    fn the_git_switch_turns_off_to_zero_and_back_to_the_default() {
        let mut d = draft();
        let poll = opt("git_remote_poll");
        assert_eq!(d.value(poll), EditValue::Integer(300));
        assert!(toggle_git(&mut d, poll));
        assert_eq!(d.value(poll), EditValue::Integer(0));
        assert!(d.is_pending(poll));
        assert!(toggle_git(&mut d, poll));
        assert_eq!(d.value(poll), EditValue::Integer(300));
        assert!(!d.is_pending(poll));
    }

    #[test]
    fn the_git_seconds_field_refuses_a_value_under_the_floor() {
        let mut d = draft();
        let poll = opt("git_remote_poll");
        commit_text(&mut d, poll, "10");
        assert!(matches!(
            d.invalid(poll),
            Some(ValueError::OutOfRange { .. })
        ));
        commit_text(&mut d, poll, "60");
        assert_eq!(d.value(poll), EditValue::Integer(60));
        assert!(!d.has_invalid());
    }

    #[test]
    fn choosing_a_value_by_name_makes_a_pending_string() {
        let mut d = draft();
        let language = opt("language");
        assert!(choose_value(&mut d, language, "pt_BR"));
        assert_eq!(d.value(language), EditValue::String("pt_BR".to_owned()));
        assert!(d.is_pending(language));
    }

    #[test]
    fn stepping_a_number_adds_the_step_and_a_text_that_is_not_a_number_does_nothing() {
        let mut d = draft();
        let size = opt("font_size");
        let next = step_text(&mut d, size, "14", 1).unwrap();
        assert_eq!(d.value(size), EditValue::Float(next.parse().unwrap()));
        assert!(d.is_pending(size));
        assert_eq!(step_text(&mut d, size, "abc", 1), None);
    }

    // ---- salvar e restaurar

    const FILE: &str = "\
# arquivo do usuário
[terminal.font]
# tamanho em pixels lógicos
size = 14.0   # RF-5.3

[terminal.cursor]
shape = \"block\"

[terminal.selection]
copy_on_select = false
";

    fn apply(edits: &[Edit]) -> String {
        ConfigDocument::parse(FILE).unwrap().apply(edits).unwrap()
    }

    fn changed_lines(before: &str, after: &str) -> Vec<String> {
        before
            .lines()
            .zip(after.lines())
            .filter(|(a, b)| a != b)
            .map(|(_, b)| b.to_owned())
            .collect()
    }

    #[test]
    fn save_produces_exactly_the_edits_of_the_three_changed_options() {
        let file = porecatu_config::parse(FILE).unwrap().0;
        let mut d = Draft::new(&file);
        commit_text(&mut d, opt("font_size"), "16");
        toggle(&mut d, opt("copy_on_select"));
        choose_segment(&mut d, opt("cursor_shape"), 1);

        // Na ordem do catálogo, uma edição por pendência, nada mais.
        let edits = d.edits();
        assert_eq!(
            edits,
            [
                Edit::Set(
                    KeyPath::parse("terminal.font.size").unwrap(),
                    EditValue::Float(16.0)
                ),
                Edit::Set(
                    KeyPath::parse("terminal.cursor.shape").unwrap(),
                    EditValue::String("beam".to_owned())
                ),
                Edit::Set(
                    KeyPath::parse("terminal.selection.copy_on_select").unwrap(),
                    EditValue::Bool(true)
                ),
            ]
        );
        // E aplicadas ao arquivo, só essas três linhas mudam.
        let after = apply(&edits);
        assert_eq!(
            changed_lines(FILE, &after),
            [
                "size = 16.0   # RF-5.3",
                "shape = \"beam\"",
                "copy_on_select = true"
            ]
        );
        assert_eq!(FILE.lines().count(), after.lines().count());
    }

    #[test]
    fn restoring_the_default_is_a_remove_that_keeps_the_comment_above() {
        let file = porecatu_config::parse(FILE).unwrap().0;
        let mut d = Draft::new(&file);
        let blink = opt("copy_on_select");
        // O arquivo diz `false`, que é o padrão: nada a restaurar.
        d.reset(blink);
        assert!(!d.is_dirty());

        // `size = 14.0` é o padrão também; mexo e restauro.
        let size = opt("font_size");
        commit_text(&mut d, size, "20");
        assert!(d.can_reset(size));
        d.reset(size);
        // O arquivo já diz o padrão: restaurar só desfaz a pendência.
        assert!(!d.is_pending(size));

        // Num arquivo que NÃO diz o padrão, restaurar vira `Remove`.
        let custom = "[terminal.font]\n# tamanho em pixels lógicos\nsize = 18.0\nfamily = \"X\"\n";
        let file = porecatu_config::parse(custom).unwrap().0;
        let mut d = Draft::new(&file);
        d.reset(size);
        let edits = d.edits();
        assert_eq!(
            edits,
            [Edit::Remove(KeyPath::parse("terminal.font.size").unwrap())]
        );
        let after = ConfigDocument::parse(custom)
            .unwrap()
            .apply(&edits)
            .unwrap();
        assert_eq!(
            after,
            "[terminal.font]\n# tamanho em pixels lógicos\nfamily = \"X\"\n"
        );
    }

    #[test]
    fn a_refused_value_is_never_an_edit() {
        let mut d = draft();
        commit_text(&mut d, opt("font_size"), "900");
        toggle(&mut d, opt("cursor_blink"));
        assert_eq!(d.edits().len(), 1, "só a alternância vira edição");
        assert!(d.has_invalid());
    }

    #[test]
    fn discard_brings_back_what_the_file_says() {
        let mut d = draft();
        commit_text(&mut d, opt("font_size"), "16");
        toggle(&mut d, opt("cursor_blink"));
        d.discard();
        assert!(!d.is_dirty());
        assert!(d.edits().is_empty());
        assert_eq!(d.value(opt("font_size")), EditValue::Float(14.0));
    }

    // ---- listas

    fn row(first: &str, second: &str) -> (String, String) {
        (first.to_owned(), second.to_owned())
    }

    #[test]
    fn adding_an_env_item_creates_a_refused_row_until_it_has_a_name() {
        let mut d = draft();
        let env = opt("shell_env");
        assert_eq!(list_add(&mut d, env), Some(0));
        assert!(d.has_invalid());
        assert!(d.edits().is_empty());
        assert!(list_set_text(&mut d, env, 0, false, "EDITOR"));
        assert!(!d.has_invalid());
        assert!(list_set_text(&mut d, env, 0, true, "vim"));
        let edits = d.edits();
        assert_eq!(edits.len(), 1);
        let Edit::Set(path, EditValue::StringMap(map)) = &edits[0] else {
            panic!("esperava um Set de mapa: {edits:?}");
        };
        assert_eq!(*path, env.key_path());
        assert_eq!(map.get("EDITOR").map(String::as_str), Some("vim"));
    }

    #[test]
    fn a_repeated_env_name_is_refused_on_the_row_and_blocks_save() {
        let mut d = draft();
        let env = opt("shell_env");
        list_add(&mut d, env);
        list_set_text(&mut d, env, 0, false, "A");
        list_add(&mut d, env);
        list_set_text(&mut d, env, 1, false, "A");
        assert!(d.has_invalid());
        assert_eq!(
            d.invalid(env),
            Some(&ValueError::DuplicateName("A".to_owned()))
        );
        assert_eq!(Draft::invalid_rows(env, &d.rows(env)), [true, true]);
        list_set_text(&mut d, env, 1, false, "B");
        assert!(!d.has_invalid());
    }

    #[test]
    fn removing_the_last_item_of_a_file_list_is_a_pending_edit() {
        let mut d = Draft::new(
            &porecatu_config::parse("[shell]\nargs = [\"-l\"]\n")
                .unwrap()
                .0,
        );
        let args = opt("shell_args");
        assert!(list_remove(&mut d, args, 0));
        assert!(d.is_pending(args));
        assert_eq!(
            d.edits(),
            [Edit::Set(args.key_path(), EditValue::StringList(vec![]))]
        );
        assert!(!list_remove(&mut d, args, 0), "não há mais itens");
    }

    #[test]
    fn moving_an_item_swaps_it_with_its_neighbour_and_stops_at_the_ends() {
        let mut d = draft();
        let args = opt("shell_args");
        d.set_rows(args, &[row("a", ""), row("b", ""), row("c", "")])
            .unwrap();
        assert_eq!(list_move(&mut d, args, 1, -1), Some(0));
        assert_eq!(d.rows(args), [row("b", ""), row("a", ""), row("c", "")]);
        assert_eq!(list_move(&mut d, args, 0, -1), None);
        assert_eq!(list_move(&mut d, args, 2, 1), None);
        assert_eq!(list_move(&mut d, args, 1, 1), Some(2));
        assert_eq!(d.rows(args), [row("b", ""), row("c", ""), row("a", "")]);
        assert_eq!(
            d.edits(),
            [Edit::Set(
                args.key_path(),
                EditValue::StringList(vec!["b".into(), "c".into(), "a".into()])
            )]
        );
    }

    #[test]
    fn an_unchanged_item_text_touches_nothing() {
        let mut d = draft();
        let args = opt("shell_args");
        list_add(&mut d, args);
        list_set_text(&mut d, args, 0, false, "x");
        assert!(!list_set_text(&mut d, args, 0, false, "x"));
        assert!(!list_set_text(&mut d, args, 7, false, "x"));
    }

    #[test]
    fn a_trusted_paths_item_is_a_text_list_item() {
        let mut d = draft();
        let paths = opt("trusted_paths");
        assert_eq!(list_add(&mut d, paths), Some(0));
        list_set_text(&mut d, paths, 0, false, "C:\\Projetos");
        assert_eq!(
            d.edits(),
            [Edit::Set(
                paths.key_path(),
                EditValue::StringList(vec!["C:\\Projetos".into()])
            )]
        );
    }

    #[test]
    fn a_non_list_option_cannot_get_an_item() {
        let mut d = draft();
        assert_eq!(list_add(&mut d, opt("font_size")), None);
    }

    #[test]
    fn choosing_a_theme_is_a_pending_string_and_choosing_the_file_one_clears_it() {
        let mut d = draft();
        let theme = opt("theme");
        assert!(choose_value(&mut d, theme, "nord"));
        assert!(d.is_pending(theme));
        assert_eq!(d.value(theme), EditValue::String("nord".to_owned()));
        assert!(choose_value(&mut d, theme, ""));
        assert!(!d.is_pending(theme));
    }
}
