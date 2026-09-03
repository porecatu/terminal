// SPDX-License-Identifier: GPL-3.0-or-later

//! Erro localizado (ADR-0003 regra 3): linha, coluna e mensagem, nunca
//! `String` solta. A etapa 4 formata isto num aviso de UI (ADR-0014).

use std::fmt;

/// Erro de parse ou de validação semântica de uma config.
///
/// `line`/`column` são `None` para erros que não vêm de uma posição no texto
/// fonte -- por exemplo nome de tema duplicado, que é uma checagem entre
/// duas tabelas `[[themes]]` já deserializadas, não uma posição única.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

impl ConfigError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            line: None,
            column: None,
            message: message.into(),
        }
    }

    pub fn at(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            line: Some(line),
            column: Some(column),
            message: message.into(),
        }
    }

    /// Converte um `toml::de::Error` em erro localizado, usando o span que o
    /// crate `toml` fornece para calcular linha e coluna no texto original.
    pub(crate) fn from_toml(source: &str, err: toml::de::Error) -> Self {
        let message = err.message().to_owned();
        let Some(span) = err.span() else {
            return Self::new(message);
        };
        let (line, column) = line_column_at(source, span.start);
        Self::at(line, column, message)
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

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => {
                write!(f, "linha {line}, coluna {column}: {}", self.message)
            }
            _ => write!(f, "{}", self.message),
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
        let err = ConfigError::at(3, 5, "chave desconhecida");
        assert_eq!(err.to_string(), "linha 3, coluna 5: chave desconhecida");
    }

    #[test]
    fn display_without_position() {
        let err = ConfigError::new("nome de tema duplicado");
        assert_eq!(err.to_string(), "nome de tema duplicado");
    }
}
