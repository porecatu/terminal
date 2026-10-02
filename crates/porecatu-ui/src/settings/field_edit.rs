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
use crate::text_field::TextFieldState;

/// Qual campo de uma linha está em edição. Só `git.remote_poll_interval_secs`
/// tem dois controles na mesma linha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditPart {
    /// O campo de texto ou numérico da linha.
    Field,
    /// O número de segundos de `git.remote_poll_interval_secs`.
    GitSeconds,
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
}
