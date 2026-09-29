// SPDX-License-Identifier: GPL-3.0-or-later

//! Nome de idioma (ADR-0056 §6): `^[a-z]{2,3}_[A-Z]{2}$`, caixa exata.
//! O nome vira parte de um caminho de arquivo, então a gramática fechada é o
//! que barra `../x` antes de qualquer acesso a disco.

use std::fmt;

/// Idioma de reserva e default (RF-15.2).
pub const FALLBACK_LOCALE: &str = "en_US";

/// Nome de idioma já validado.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocaleName(String);

/// Valor que não segue a gramática do nome de idioma.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidLocaleName {
    pub value: String,
}

impl fmt::Display for InvalidLocaleName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid locale name {:?}", self.value)
    }
}

impl std::error::Error for InvalidLocaleName {}

impl LocaleName {
    /// Valida `value` contra `^[a-z]{2,3}_[A-Z]{2}$`, sem regex.
    pub fn parse(value: &str) -> Result<Self, InvalidLocaleName> {
        if is_valid(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(InvalidLocaleName {
                value: value.to_owned(),
            })
        }
    }

    /// O idioma de reserva, `en_US`.
    pub fn fallback() -> Self {
        Self(FALLBACK_LOCALE.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Forma BCP 47 (`pt_BR` -> `pt-BR`), para a árvore de acessibilidade.
    pub fn bcp47(&self) -> String {
        self.0.replace('_', "-")
    }
}

impl fmt::Display for LocaleName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    // Idioma de 2 ou 3 letras, `_`, região de 2 letras.
    let lang_len = match bytes.len() {
        5 => 2,
        6 => 3,
        _ => return false,
    };
    let (lang, rest) = bytes.split_at(lang_len);
    let [b'_', region @ ..] = rest else {
        return false;
    };
    lang.iter().all(u8::is_ascii_lowercase) && region.iter().all(u8::is_ascii_uppercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        for name in ["pt_BR", "en_US", "ast_ES"] {
            assert_eq!(LocaleName::parse(name).unwrap().as_str(), name);
        }
    }

    #[test]
    fn invalid_names() {
        for name in [
            "pt-BR", "pt_br", "PT_BR", "../x", "pt_BRA", "", "p_BR", "abcd_BR", "pt_B", "pt_",
            "_BR", "pt BR", "pt_BR ", "pé_BR", "..\\x_YY",
        ] {
            assert_eq!(
                LocaleName::parse(name),
                Err(InvalidLocaleName {
                    value: name.to_owned()
                }),
                "{name:?} deveria ser inválido"
            );
        }
    }

    #[test]
    fn bcp47_swaps_underscore() {
        assert_eq!(LocaleName::parse("pt_BR").unwrap().bcp47(), "pt-BR");
        assert_eq!(LocaleName::parse("ast_ES").unwrap().bcp47(), "ast-ES");
        assert_eq!(LocaleName::fallback().bcp47(), "en-US");
    }
}
