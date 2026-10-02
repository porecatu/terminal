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
}
