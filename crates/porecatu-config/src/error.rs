// SPDX-License-Identifier: GPL-3.0-or-later

//! Erro localizado (ADR-0003 regra 3): linha, coluna e o **motivo tipado**,
//! nunca `String` de prosa. A frase de interface é composta em
//! `porecatu-ui` a partir de [`ConfigErrorKind`] (ADR-0056 §2); o `Display`
//! daqui é só para a saída de erro e depuração.

use std::fmt;
use std::path::PathBuf;

/// Por que a config não carregou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigErrorKind {
    /// Texto do crate `toml` (sintaxe, tipo errado, cor inválida), mostrado
    /// como chegou -- é detalhe técnico, não frase do app.
    Toml { detail: String },
    /// O arquivo existe mas não pôde ser lido. `cause` é o texto do
    /// `io::Error`, do sistema operacional.
    Unreadable { path: PathBuf, cause: String },
    /// Dois `[[themes]]` com o mesmo `name`.
    DuplicateThemeName { name: String },
}

/// Erro de parse ou de validação semântica de uma config.
///
/// `line`/`column` são `None` para erros que não vêm de uma posição no texto
/// fonte -- por exemplo nome de tema duplicado, que é uma checagem entre
/// duas tabelas `[[themes]]` já deserializadas, não uma posição única.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub kind: ConfigErrorKind,
}

impl ConfigError {
    pub fn new(kind: ConfigErrorKind) -> Self {
        Self {
            line: None,
            column: None,
            kind,
        }
    }

    pub fn at(line: usize, column: usize, kind: ConfigErrorKind) -> Self {
        Self {
            line: Some(line),
            column: Some(column),
            kind,
        }
    }

    /// Converte um `toml::de::Error` em erro localizado, usando o span que o
    /// crate `toml` fornece para calcular linha e coluna no texto original.
    pub(crate) fn from_toml(source: &str, err: toml::de::Error) -> Self {
        let kind = ConfigErrorKind::Toml {
            detail: err.message().to_owned(),
        };
        let Some(span) = err.span() else {
            return Self::new(kind);
        };
        let (line, column) = line_column_at(source, span.start);
        Self::at(line, column, kind)
    }
}

/// Linha e coluna (ambas contadas a partir de 1) do byte offset `pos` em
/// `source`.
fn line_column_at(source: &str, pos: usize) -> (usize, usize) {
    let pos = pos.min(source.len());
    let prefix = &source[..pos];
    let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
    let column = match prefix.rfind('\n') {
        Some(last_newline) => pos - last_newline,
        None => pos + 1,
    };
    (line, column)
}

impl fmt::Display for ConfigErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml { detail } => f.write_str(detail),
            Self::Unreadable { path, cause } => {
                write!(f, "não foi possível ler \"{}\": {cause}", path.display())
            }
            Self::DuplicateThemeName { name } => {
                write!(f, "nome de tema duplicado: \"{name}\"")
            }
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => {
                write!(f, "linha {line}, coluna {column}: {}", self.kind)
            }
            _ => write!(f, "{}", self.kind),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_column_at_start() {
        assert_eq!(line_column_at("abc", 0), (1, 1));
    }

    #[test]
    fn line_column_after_newline() {
        assert_eq!(line_column_at("a\nbc", 2), (2, 1));
        assert_eq!(line_column_at("a\nbc", 3), (2, 2));
    }

    #[test]
    fn display_with_position() {
        let err = ConfigError::at(
            3,
            5,
            ConfigErrorKind::Toml {
                detail: "chave desconhecida".to_owned(),
            },
        );
        assert_eq!(err.to_string(), "linha 3, coluna 5: chave desconhecida");
    }

    #[test]
    fn display_without_position() {
        let err = ConfigError::new(ConfigErrorKind::DuplicateThemeName {
            name: "x".to_owned(),
        });
        assert_eq!(err.to_string(), "nome de tema duplicado: \"x\"");
    }
}
