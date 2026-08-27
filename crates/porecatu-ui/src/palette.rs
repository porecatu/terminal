// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolução de `TermColor` (não resolvida, `porecatu-term`) para cor
//! concreta. `porecatu-config` não existe ainda (F1) -- valores vêm de
//! `docs/config/porecatu.example.toml`, seção `[terminal.colors]` e
//! subseções, como constantes com a chave de origem no comentário (mesmo
//! padrão de `WINDOW_BACKGROUND`).

use porecatu_core::GroupColor;
use porecatu_render::Color;
use porecatu_term::TermColor;

const fn hex(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: r as f64 / 255.0,
        g: g as f64 / 255.0,
        b: b as f64 / 255.0,
        a: 1.0,
    }
}

const fn hex_alpha(r: u8, g: u8, b: u8, a: f64) -> Color {
    Color {
        r: r as f64 / 255.0,
        g: g as f64 / 255.0,
        b: b as f64 / 255.0,
        a,
    }
}

// [terminal.colors]
pub const TERM_FOREGROUND: Color = hex(0xc7, 0xcc, 0xd6); // foreground, RF-5.12
pub const TERM_BACKGROUND: Color = hex(0x0f, 0x12, 0x16); // background
pub const TERM_CURSOR: Color = hex(0x5e, 0xd3, 0xbc); // cursor, RF-5.13
#[allow(dead_code)] // usado quando o glyph sob o cursor for repintado (F2+)
pub const TERM_CURSOR_TEXT: Color = hex(0x0f, 0x12, 0x16); // cursor_text
pub const TERM_SELECTION_BACKGROUND: Color = hex(0x2e, 0x6b, 0x62); // selection_background, RF-5.14
pub const TERM_SELECTION_FOREGROUND: Color = hex(0xee, 0xf2, 0xf4); // selection_foreground

// Nota no grid (RF-1.3, ADR-0017 item 5): "#5ed3bc, nunca imitando prompt"
// -- destaque fixo do ADR-0014, independente de tema. `porecatu-term` não
// resolve cor (seção 4 da arquitetura); passado cru como RGB pro motor.
pub const NOTE_ACCENT_RGB: (u8, u8, u8) = (0x5e, 0xd3, 0xbc);

// Espec. visual §1.2/§1.3 -- barra de abas.
pub const BAR_BACKGROUND: Color = hex(0x1b, 0x1f, 0x26); // Barras
pub const BAR_SEPARATOR: Color = hex(0x23, 0x27, 0x2f); // Separador de barra

// Espec. visual §2.5 -- aba.
pub const TAB_ACTIVE_BACKGROUND: Color = hex(0x28, 0x2e, 0x37);
pub const TAB_ACTIVE_BORDER: Color = hex(0x39, 0x40, 0x4b);
pub const TAB_ACTIVE_TEXT: Color = hex(0xea, 0xee, 0xf3);
pub const TAB_INACTIVE_BACKGROUND: Color = hex(0x19, 0x1d, 0x23);
pub const TAB_INACTIVE_BORDER: Color = hex(0x22, 0x26, 0x2e);
pub const TAB_INACTIVE_TEXT: Color = hex(0x98, 0xa0, 0xab);
// Aba `Exited` (ADR-0017): fundo/borda de inativa, texto no tom do botão
// de fechar -- "o token de inerte da barra".
pub const TAB_EXITED_TEXT: Color = hex(0x72, 0x7a, 0x86);
pub const CLOSE_BUTTON_ICON: Color = hex(0x72, 0x7a, 0x86);
// `[appearance.groups] ungrouped_color` -- sublinhado das abas do grupo
// implícito (ADR-0006) e cor de tingimento do wrapper delas (que fica
// sempre transparente, espec §2.3 -- este token só serve o sublinhado).
pub const UNGROUPED_UNDERLINE: Color = hex(0x7b, 0x83, 0x8f);

// `[appearance.groups] palette` -- RF-2.4, RF-4.18. Seis cores, na mesma
// ordem de `GroupColor::ALL` (`porecatu-core`) -- é a ordem de atribuição
// automática (ADR-0020 §5).
const GROUP_RED: Color = hex(0xef, 0x8a, 0x8a);
const GROUP_YELLOW: Color = hex(0xe0, 0xb0, 0x60);
const GROUP_CYAN: Color = hex(0x5e, 0xd3, 0xbc);
const GROUP_BLUE: Color = hex(0x6f, 0xa8, 0xf5);
const GROUP_PURPLE: Color = hex(0xa6, 0x8c, 0xf0);
const GROUP_GREEN: Color = hex(0x86, 0xc5, 0x6a);

/// Resolve `GroupColor` (não resolvida, `porecatu-core`) para a cor
/// concreta -- mesma separação que este módulo já faz para `TermColor`.
pub const fn group_color(color: GroupColor) -> Color {
    match color {
        GroupColor::Red => GROUP_RED,
        GroupColor::Yellow => GROUP_YELLOW,
        GroupColor::Cyan => GROUP_CYAN,
        GroupColor::Blue => GROUP_BLUE,
        GroupColor::Purple => GROUP_PURPLE,
        GroupColor::Green => GROUP_GREEN,
    }
}

// Pílula de grupo (espec §2.4, `[appearance.groups]`).
pub const PILL_BACKGROUND: Color = hex(0x1f, 0x24, 0x2c); // label_background
pub const PILL_BORDER: Color = hex(0x2b, 0x31, 0x3b); // label_border
pub const PILL_TEXT: Color = hex(0xc3, 0xca, 0xd3); // label_foreground
pub const PILL_COUNT_BACKGROUND: Color = hex(0x12, 0x15, 0x1a); // count_background
pub const PILL_COUNT_TEXT: Color = hex(0x7b, 0x83, 0x8f); // count_foreground
pub const PILL_CARET: Color = hex(0x6b, 0x73, 0x7e); // caret_foreground

// `[appearance.tabs] selected_border` -- ADR-0021, RF-2.2: aba em seleção
// múltipla. Modificador sobre o estado de base (não é um quarto estado,
// espec. §2.5) -- mesmo acento do campo de rename e do anel de foco.
pub const SELECTED_BORDER: Color = hex(0x5e, 0xd3, 0xbc);

// Espec. visual §2.5 -- campo de rename.
pub const RENAME_BACKGROUND: Color = hex(0x0e, 0x11, 0x16);
pub const RENAME_BORDER: Color = hex(0x5e, 0xd3, 0xbc);
pub const RENAME_TEXT: Color = hex(0xe4, 0xe8, 0xee);

// Espec. visual §2.6 -- botão de nova aba.
pub const NEW_TAB_ICON: Color = hex(0x9a, 0xa2, 0xae);
pub const NEW_TAB_BORDER: Color = hex(0x26, 0x2b, 0x34);

// `[appearance.tabs] activity_indicator` / `bell_indicator` -- espec §2.17,
// RF-1.20/RF-1.21. Mesmos tons de sucesso/erro da seção 1.5.
pub const ACTIVITY_INDICATOR: Color = hex(0x86, 0xc5, 0x6a);
pub const BELL_INDICATOR: Color = hex(0xef, 0x8a, 0x8a);

// Espec. visual §2.18 -- indicador de abas fora da vista, mesmos tokens do
// contador da pílula de grupo (§2.4).
pub const OVERFLOW_COUNT_BACKGROUND: Color = hex(0x12, 0x15, 0x1a);
pub const OVERFLOW_COUNT_TEXT: Color = hex(0x7b, 0x83, 0x8f);

/// Transparente -- usado como preenchimento de um `RoundedQuad` que só
/// desenha borda (botão de nova aba, espec. §2.6, sem "fundo" listado).
pub const TRANSPARENT: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

// [terminal.colors.normal], RF-5.11 -- indices 0..8
const ANSI_NORMAL: [Color; 8] = [
    hex(0x3b, 0x43, 0x4f), // black
    hex(0xef, 0x8a, 0x8a), // red
    hex(0x86, 0xc5, 0x6a), // green
    hex(0xe0, 0xb0, 0x60), // yellow
    hex(0x6f, 0xa8, 0xf5), // blue
    hex(0xa6, 0x8c, 0xf0), // magenta
    hex(0x5e, 0xd3, 0xbc), // cyan
    hex(0xc7, 0xcc, 0xd6), // white
];

// [terminal.colors.bright], RF-5.11 -- indices 8..16
const ANSI_BRIGHT: [Color; 8] = [
    hex(0x6f, 0x77, 0x83), // black
    hex(0xf5, 0xa3, 0xa3), // red
    hex(0x9b, 0xd4, 0x82), // green
    hex(0xec, 0xc3, 0x7c), // yellow
    hex(0x8d, 0xbc, 0xf8), // blue
    hex(0xbd, 0xa6, 0xf5), // magenta
    hex(0x7f, 0xdf, 0xcc), // cyan
    hex(0xea, 0xee, 0xf3), // white
];

/// Resolve `Default` para `default_fg`/`default_bg` (frente ou fundo,
/// conforme o campo da célula) e índices/RGB conforme a paleta acima.
/// Índices 16..256: cubo 6x6x6 + rampa de cinza -- fórmula padrão xterm
/// 256 cores, não é valor de design (não tem procedência no mockup, é
/// convenção técnica universal do terminal).
pub fn resolve(
    color: TermColor,
    default_fg: Color,
    default_bg: Color,
    is_foreground: bool,
) -> Color {
    match color {
        TermColor::Default => {
            if is_foreground {
                default_fg
            } else {
                default_bg
            }
        }
        TermColor::Indexed(index) => resolve_indexed(index),
        TermColor::Rgb { r, g, b } => hex(r, g, b),
    }
}

// Espec. visual §2.9/§2.14/§2.15/§2.16/§2.20 -- fundo, borda e sombra
// (sombra não tem equivalente em `porecatu-render`, ver `overlay.rs`)
// compartilhados por aviso, diálogo, menu de contexto e tooltip: os
// "quatro widgets de chrome" do ADR-0014/ADR-0019 leem da mesma família de
// popover, de propósito -- nenhuma cor nova por widget.
pub const POPOVER_BACKGROUND: Color = hex(0x1a, 0x1e, 0x25);
pub const POPOVER_BORDER: Color = hex(0x2e, 0x34, 0x3e);

// Espec. §2.14 -- aviso do app.
pub const WARNING_TITLE_TEXT: Color = hex(0xdf, 0xe4, 0xea);
pub const WARNING_BODY_TEXT: Color = hex(0x6b, 0x73, 0x7e);
pub const WARNING_SEVERITY_ERROR: Color = hex(0xef, 0x8a, 0x8a);
pub const WARNING_SEVERITY_WARNING: Color = hex(0xe0, 0xb0, 0x60);
pub const WARNING_SEVERITY_INFO: Color = hex(0x5e, 0xd3, 0xbc);

// Espec. §2.15 -- diálogo de confirmação.
pub const DIALOG_OVERLAY: Color = hex_alpha(0x06, 0x07, 0x09, 0.45);
pub const DIALOG_TITLE_TEXT: Color = hex(0xe6, 0xea, 0xef);
pub const DIALOG_BODY_TEXT: Color = hex(0xd7, 0xdc, 0xe3);
pub const DIALOG_CANCEL_BORDER: Color = hex(0x26, 0x2b, 0x34);
pub const DIALOG_CANCEL_TEXT: Color = hex(0xd7, 0xdc, 0xe3);
pub const DIALOG_CONFIRM_BACKGROUND: Color = hex(0xe0, 0x85, 0x85);
pub const DIALOG_CONFIRM_TEXT: Color = hex(0x1a, 0x1e, 0x25);
// "O botão focado leva borda 1px #5ed3bc" (§2.15) -- mesmo acento do campo
// de rename.
pub const DIALOG_FOCUS_RING: Color = hex(0x5e, 0xd3, 0xbc);

// Espec. §2.9/§2.16 -- item de menu de contexto (reaproveita os tokens do
// menu de perfis, `[v2]`, por decisão do ADR-0014).
pub const MENU_ITEM_TEXT: Color = hex(0xd7, 0xdc, 0xe3);
pub const MENU_ITEM_DISABLED_TEXT: Color = hex(0x5c, 0x64, 0x6f);
pub const MENU_ITEM_HOVER: Color = hex(0x24, 0x2a, 0x33);

// Espec. §2.20 -- tooltip.
pub const TOOLTIP_TEXT: Color = hex(0xd7, 0xdc, 0xe3);

fn resolve_indexed(index: u8) -> Color {
    match index {
        0..=7 => ANSI_NORMAL[index as usize],
        8..=15 => ANSI_BRIGHT[(index - 8) as usize],
        16..=231 => {
            let i = index - 16;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            hex(cube_channel(r), cube_channel(g), cube_channel(b))
        }
        232..=255 => {
            let level = 8 + 10 * (index - 232) as u16;
            hex(level as u8, level as u8, level as u8)
        }
    }
}

fn cube_channel(n: u8) -> u8 {
    if n == 0 { 0 } else { 55 + 40 * n }
}
