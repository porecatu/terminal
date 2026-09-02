// SPDX-License-Identifier: GPL-3.0-or-later

//! Cor de config (RF-4.9): `"#rrggbb"`, `"#rrggbbaa"` ou `"transparent"`.
//! Tipo próprio em vez de `String` crua -- valor inválido vira erro
//! localizado pelo `toml::de::Error` do campo, com a mesma mensagem que
//! qualquer outro erro de tipo (ADR-0003 regra 3).

use std::fmt;

use serde::{Deserialize, Deserializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 0xff)
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn r(self) -> u8 {
        self.r
    }

    pub const fn g(self) -> u8 {
        self.g
    }

    pub const fn b(self) -> u8 {
        self.b
    }

    pub const fn a(self) -> u8 {
        self.a
    }

    /// Transcreve um literal `#rrggbb`/`#rrggbbaa`/`transparent` conhecido em
    /// tempo de escrita. Usado só pelos `Default` deste crate -- entrada do
    /// usuário passa por `Color::parse`, que devolve erro em vez de `panic`.
    pub(crate) fn hex(s: &str) -> Self {
        Self::parse(s).unwrap_or_else(|err| panic!("literal de cor interno inválido {s:?}: {err}"))
    }

    pub fn parse(s: &str) -> Result<Self, ColorParseError> {
        if s == "transparent" {
            return Ok(Self::TRANSPARENT);
        }
        let Some(hex) = s.strip_prefix('#') else {
            return Err(ColorParseError::MissingHash(s.to_owned()));
        };
        let byte = |slice: &str| -> Result<u8, ColorParseError> {
            u8::from_str_radix(slice, 16).map_err(|_| ColorParseError::NotHex(s.to_owned()))
        };
        match hex.len() {
            6 => Ok(Self::rgb(
                byte(&hex[0..2])?,
                byte(&hex[2..4])?,
                byte(&hex[4..6])?,
            )),
            8 => Ok(Self::rgba(
                byte(&hex[0..2])?,
                byte(&hex[2..4])?,
                byte(&hex[4..6])?,
                byte(&hex[6..8])?,
            )),
            _ => Err(ColorParseError::WrongLength(s.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorParseError {
    MissingHash(String),
    WrongLength(String),
    NotHex(String),
}

impl fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::MissingHash(v) | Self::WrongLength(v) | Self::NotHex(v) => v,
        };
        write!(
            f,
            "cor inválida {value:?}: use \"#rrggbb\", \"#rrggbbaa\" ou \"transparent\""
        )
    }
}

impl std::error::Error for ColorParseError {}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rgb() {
        assert_eq!(
            Color::parse("#15181d").unwrap(),
            Color::rgb(0x15, 0x18, 0x1d)
        );
    }

    #[test]
    fn parses_rgba() {
        assert_eq!(
            Color::parse("#06070973").unwrap(),
            Color::rgba(0x06, 0x07, 0x09, 0x73)
        );
    }

    #[test]
    fn parses_transparent() {
        assert_eq!(Color::parse("transparent").unwrap(), Color::TRANSPARENT);
    }

    #[test]
    fn rejects_missing_hash() {
        assert!(Color::parse("15181d").is_err());
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(Color::parse("#1518").is_err());
    }

    #[test]
    fn rejects_non_hex() {
        assert!(Color::parse("#gggggg").is_err());
    }

    #[test]
    fn trusted_hex_matches_parse() {
        assert_eq!(Color::hex("#5ed3bc"), Color::parse("#5ed3bc").unwrap());
    }
}
