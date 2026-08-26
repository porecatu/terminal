// SPDX-License-Identifier: GPL-3.0-or-later

//! Cor de célula não resolvida (docs/arquitetura.md secao 4.1). Quem resolve
//! para RGBA concreto é `porecatu-ui`, que tem a paleta e o tema (`Config`)
//! — este crate não conhece nenhum dos dois.

use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

/// Cor de célula antes da resolução de paleta/tema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TermColor {
    /// Cor padrão do terminal (frente ou fundo, conforme o campo da célula).
    #[default]
    Default,
    /// Índice na paleta de 256 cores (0..16 = ANSI nomeada, 16..256 = cubo/grayscale).
    Indexed(u8),
    /// Cor RGB explícita (true color, CSI 38/48;2).
    Rgb { r: u8, g: u8, b: u8 },
}

impl From<AnsiColor> for TermColor {
    fn from(color: AnsiColor) -> Self {
        match color {
            AnsiColor::Named(NamedColor::Foreground) | AnsiColor::Named(NamedColor::Background) => {
                TermColor::Default
            }
            // As 16 cores nomeadas (Black..BrightWhite) mapeiam 1:1 para os
            // primeiros índices da paleta -- é a mesma conversão que
            // `NamedColor as usize` já usa internamente no alacritty_terminal.
            AnsiColor::Named(named) if (named as usize) < 16 => TermColor::Indexed(named as u8),
            // Variantes nomeadas exóticas (Cursor, Dim*, BrightForeground,
            // DimBackground) não aparecem em fg/bg de célula na prática --
            // são usadas internamente pelo motor para efeitos de render.
            // Caem em `Default` por segurança.
            AnsiColor::Named(_) => TermColor::Default,
            AnsiColor::Indexed(index) => TermColor::Indexed(index),
            AnsiColor::Spec(rgb) => TermColor::Rgb {
                r: rgb.r,
                g: rgb.g,
                b: rgb.b,
            },
        }
    }
}
