// SPDX-License-Identifier: GPL-3.0-or-later

//! Frase, marcadores e plural (ADR-0056 §5).
//!
//! Marcador é `{ident}` com `ident` em `[a-z_][a-z0-9_]*`; `{{` e `}}` são
//! chaves literais. A substituição é literal e **não recursiva**: o valor
//! vem de fora (um título de aba, que vem de um programa) e nunca é lido de
//! novo como modelo.

/// Uma frase do catálogo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Simple(String),
    /// `one` é opcional; sem ele, `other` serve a todo `n`.
    Plural {
        one: Option<String>,
        other: String,
    },
}

/// Forma de plural escolhida para uma contagem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluralForm {
    One,
    Other,
}

/// `one` quando `n == 1`, `other` em todo o resto.
pub fn plural_form(n: u64) -> PluralForm {
    if n == 1 {
        PluralForm::One
    } else {
        PluralForm::Other
    }
}

impl Message {
    /// Modelo da frase para a contagem `n`. `n` é ignorado numa frase
    /// simples.
    pub fn select(&self, n: u64) -> &str {
        match self {
            Message::Simple(text) => text,
            Message::Plural { one, other } => match (plural_form(n), one) {
                (PluralForm::One, Some(one)) => one,
                _ => other,
            },
        }
    }
}

/// Um pedaço de um modelo de frase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Piece<'a> {
    /// Texto literal, já com `{{`/`}}` resolvidos para uma chave só.
    Text(&'a str),
    Placeholder(&'a str),
    /// `{` ou `}` que não forma marcador nem escape.
    Malformed(&'a str),
}

pub(crate) struct Pieces<'a> {
    rest: &'a str,
}

pub(crate) fn pieces(template: &str) -> Pieces<'_> {
    Pieces { rest: template }
}

impl<'a> Iterator for Pieces<'a> {
    type Item = Piece<'a>;

    fn next(&mut self) -> Option<Piece<'a>> {
        let rest = self.rest;
        let &first = rest.as_bytes().first()?;
        let second = rest.as_bytes().get(1).copied();
        match first {
            b'{' if second == Some(b'{') => {
                self.rest = &rest[2..];
                Some(Piece::Text("{"))
            }
            b'}' if second == Some(b'}') => {
                self.rest = &rest[2..];
                Some(Piece::Text("}"))
            }
            b'{' => {
                if let Some(end) = rest.find('}')
                    && is_ident(&rest[1..end])
                {
                    self.rest = &rest[end + 1..];
                    return Some(Piece::Placeholder(&rest[1..end]));
                }
                self.rest = &rest[1..];
                Some(Piece::Malformed(&rest[..1]))
            }
            b'}' => {
                self.rest = &rest[1..];
                Some(Piece::Malformed(&rest[..1]))
            }
            _ => {
                let end = rest.find(['{', '}']).unwrap_or(rest.len());
                self.rest = &rest[end..];
                Some(Piece::Text(&rest[..end]))
            }
        }
    }
}

fn is_ident(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'_'))
        && bytes.all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// Substitui os marcadores de `template` pelos valores de `args`, em uma
/// passada só. Marcador sem argumento e chave mal formada saem como estão
/// escritos, para o defeito aparecer em vez de sumir.
pub fn format(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    for piece in pieces(template) {
        match piece {
            Piece::Text(text) | Piece::Malformed(text) => out.push_str(text),
            Piece::Placeholder(name) => match args.iter().find(|(key, _)| *key == name) {
                Some((_, value)) => out.push_str(value),
                None => {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
            },
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_placeholders() {
        assert_eq!(
            format("{a} and {b_2}", &[("a", "x"), ("b_2", "y")]),
            "x and y"
        );
    }

    #[test]
    fn doubled_braces_are_literal() {
        assert_eq!(format("{{count}}", &[("count", "3")]), "{count}");
        assert_eq!(format("a {{ b }} c", &[]), "a { b } c");
        assert_eq!(format("{{{count}}}", &[("count", "3")]), "{3}");
    }

    #[test]
    fn substitution_is_not_recursive() {
        assert_eq!(
            format("{title} ({count})", &[("title", "{count}"), ("count", "2")]),
            "{count} (2)"
        );
        // O valor também não é lido como escape.
        assert_eq!(format("{title}", &[("title", "{{x}}")]), "{{x}}");
    }

    #[test]
    fn unknown_or_malformed_stays_as_written() {
        assert_eq!(format("{missing}", &[]), "{missing}");
        assert_eq!(format("a { b", &[]), "a { b");
        assert_eq!(format("a } b", &[]), "a } b");
        assert_eq!(format("{Count}", &[("Count", "1")]), "{Count}");
        assert_eq!(format("{}", &[]), "{}");
    }

    #[test]
    fn omitted_placeholder_is_fine() {
        assert_eq!(format("no markers", &[("count", "3")]), "no markers");
    }

    #[test]
    fn keeps_non_ascii_text() {
        assert_eq!(
            format("Fechar “{title}” — já?", &[("title", "aç")]),
            "Fechar “aç” — já?"
        );
    }

    #[test]
    fn plural_zero_one_two() {
        let message = Message::Plural {
            one: Some("one tab".to_owned()),
            other: "many tabs".to_owned(),
        };
        assert_eq!(message.select(0), "many tabs");
        assert_eq!(message.select(1), "one tab");
        assert_eq!(message.select(2), "many tabs");
    }

    #[test]
    fn plural_without_one_uses_other() {
        let message = Message::Plural {
            one: None,
            other: "tabs".to_owned(),
        };
        assert_eq!(message.select(0), "tabs");
        assert_eq!(message.select(1), "tabs");
        assert_eq!(message.select(5), "tabs");
    }

    #[test]
    fn simple_ignores_count() {
        assert_eq!(Message::Simple("x".to_owned()).select(1), "x");
    }
}
