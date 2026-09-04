// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolução de `TermColor` (não resolvida, `porecatu-term`) para cor
//! concreta, e de `GroupColor`/cor de chrome (`porecatu-config`) para
//! `porecatu_render::Color`.
//!
//! Duas paletas resolvidas, construídas **uma vez** a partir de `Config` e
//! guardadas em `App` (`lib.rs`), no mesmo padrão de `TabBarStyle`:
//! [`ResolvedPalette`] (`[appearance.*]`, chrome) e [`ResolvedTermPalette`]
//! (`[terminal.colors]` e subseções, F4 etapa 3). `ResolvedTermPalette::
//! resolve` e `ResolvedPalette::group_color` são chamados no caminho quente
//! (`resolve` por célula da grade, `group_color` por grupo por frame) --
//! por isso recebem paleta já resolvida, nunca um `&Config` a consultar
//! ali. `[terminal.theme]`/`[[themes]]` ficam de fora (F4 etapa 6): vazio é
//! o único valor tratado, e vazio já cai direto em `[terminal.colors]`.

use porecatu_core::GroupColor;
use porecatu_render::Color;
use porecatu_term::TermColor;

// `pub(crate)`: `search_bar.rs` reusa para o toggle de regex (ADR-0041
// §6) -- trilho/botão sem chave no TOML, mesmo padrão de `TRANSPARENT`
// abaixo.
pub(crate) const fn hex(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: r as f64 / 255.0,
        g: g as f64 / 255.0,
        b: b as f64 / 255.0,
        a: 1.0,
    }
}

pub(crate) const fn hex_alpha(r: u8, g: u8, b: u8, a: f64) -> Color {
    Color {
        r: r as f64 / 255.0,
        g: g as f64 / 255.0,
        b: b as f64 / 255.0,
        a,
    }
}

/// Converte a cor de `porecatu-config` (RF-4.9: `#rrggbb`/`#rrggbbaa`/
/// `transparent`) para a cor de `porecatu-render` (canais `f64` 0.0..1.0).
fn cvt(c: porecatu_config::Color) -> Color {
    hex_alpha(c.r(), c.g(), c.b(), c.a() as f64 / 255.0)
}

/// Mesma conversão, com um alfa próprio no lugar do canal `a` da cor de
/// origem -- caso de `[appearance.tabs.colors] background_alpha`, que é uma
/// chave separada de `active_background`/`inactive_background` (RGB puro).
fn cvt_alpha(c: porecatu_config::Color, alpha: f64) -> Color {
    hex_alpha(c.r(), c.g(), c.b(), alpha)
}

// Nota no grid (RF-1.3, ADR-0017 item 5): "#5ed3bc, nunca imitando prompt"
// -- destaque fixo do ADR-0014, independente de tema e sem chave no TOML.
// `porecatu-term` não resolve cor (seção 4 da arquitetura); passado cru
// como RGB pro motor.
pub const NOTE_ACCENT_RGB: (u8, u8, u8) = (0x5e, 0xd3, 0xbc);

/// Transparente -- usado como preenchimento de um `RoundedQuad` que só
/// desenha borda (botão de nova aba, espec. §2.6, sem "fundo" listado), e
/// como `border_color` de quem não tem borda. Não é um token de design.
pub const TRANSPARENT: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

/// Texto da ocorrência ativa da busca (ADR-0041 §5): "o tom escuro que a
/// pílula de grupo já usa sobre cor cheia" (`#12151a`, §1.4) -- o fundo é
/// `ResolvedTermPalette::cursor` (mesmo acento, já `#5ed3bc`), sem par
/// próprio em `ResolvedTermPalette` porque os dois já existem alhures;
/// juntar os dois aqui evitaria alargar essa struct por duas constantes
/// fixas que não variam por tema.
pub const OCCURRENCE_ACTIVE_TEXT: Color = hex(0x12, 0x15, 0x1a);

fn cube_channel(n: u8) -> u8 {
    if n == 0 { 0 } else { 55 + 40 * n }
}

/// Cores de `[terminal.colors]` e subseções, resolvidas uma vez a partir de
/// `Config` e guardadas em `App` -- mesmo padrão de [`ResolvedPalette`],
/// nunca reconsultadas por célula pintada. Construído por
/// [`ResolvedTermPalette::from_config`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTermPalette {
    pub foreground: Color,
    pub background: Color,
    pub cursor: Color,
    pub cursor_text: Color,
    pub selection_background: Color,
    pub selection_foreground: Color,
    /// `[terminal.colors] prompt_secondary` -- segunda metade do prompt
    /// (caminho, via OSC 7). Resolvido para não deixar a chave sem
    /// procedência declarada, mas sem consumidor ainda: nenhum código deste
    /// projeto pinta metade de prompt numa cor diferente da outra --
    /// recurso à parte de PRD-005, não desta etapa (ver relato de fim de
    /// etapa).
    pub prompt_secondary: Color,
    normal: [Color; 8],
    bright: [Color; 8],
    /// `[terminal.font] bold_is_bright` -- RF-5.5.
    bold_is_bright: bool,
}

impl ResolvedTermPalette {
    pub fn from_config(config: &porecatu_config::Config) -> Self {
        let colors = &config.terminal.colors;
        Self {
            foreground: cvt(colors.foreground),
            background: cvt(colors.background),
            cursor: cvt(colors.cursor),
            cursor_text: cvt(colors.cursor_text),
            selection_background: cvt(colors.selection_background),
            selection_foreground: cvt(colors.selection_foreground),
            prompt_secondary: cvt(colors.prompt_secondary),
            normal: ansi_octet(&colors.normal),
            bright: ansi_octet(&colors.bright),
            bold_is_bright: config.terminal.font.bold_is_bright,
        }
    }

    /// Resolve `Default` para `foreground`/`background` (conforme
    /// `is_foreground`), índice ANSI/256 e RGB direto -- índices 16..256
    /// pelo cubo 6x6x6 + rampa de cinza (fórmula padrão xterm, não é valor
    /// de design: RF-5.17, sem procedência no mockup, convenção técnica
    /// universal do terminal). `bold` só importa para os oito primeiros
    /// índices, e só quando `is_foreground` -- RF-5.5 é uma convenção de
    /// **texto** ("negrito usa a versão brilhante"), nunca de fundo.
    pub fn resolve(&self, color: TermColor, is_foreground: bool, bold: bool) -> Color {
        match color {
            TermColor::Default => {
                if is_foreground {
                    self.foreground
                } else {
                    self.background
                }
            }
            TermColor::Indexed(index) => self.resolve_indexed(index, is_foreground && bold),
            TermColor::Rgb { r, g, b } => hex(r, g, b),
        }
    }

    fn resolve_indexed(&self, index: u8, promote_to_bright: bool) -> Color {
        match index {
            0..=7 => {
                if promote_to_bright && self.bold_is_bright {
                    self.bright[index as usize]
                } else {
                    self.normal[index as usize]
                }
            }
            8..=15 => self.bright[(index - 8) as usize],
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
}

fn ansi_octet(p: &porecatu_config::AnsiPalette) -> [Color; 8] {
    [
        cvt(p.black),
        cvt(p.red),
        cvt(p.green),
        cvt(p.yellow),
        cvt(p.blue),
        cvt(p.magenta),
        cvt(p.cyan),
        cvt(p.white),
    ]
}

/// Cores de chrome (`[appearance.*]`), resolvidas uma vez a partir de
/// `Config` e guardadas em `App` -- nunca reconsultadas por elemento
/// pintado. Construído por [`ResolvedPalette::from_config`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedPalette {
    // [appearance.tabs.colors] / barra (§1.2/§1.3/§2.2)
    pub bar_background: Color,
    pub tab_active_background: Color,
    pub tab_active_border: Color,
    pub tab_active_text: Color,
    pub tab_inactive_background: Color,
    pub tab_inactive_border: Color,
    pub tab_inactive_text: Color,
    /// `[appearance.tabs.colors] exited_foreground` -- aba `Exited`
    /// (ADR-0017).
    pub tab_exited_text: Color,
    /// `close_button_foreground` -- reaproveitado (pedido do usuário,
    /// "nenhuma cor nova") pelo "+" fora de cápsula, pelo chevron de
    /// overflow e pelo ícone de configurações: todos são o mesmo Lucide
    /// cinza-claro sobre a barra escura.
    pub chrome_icon: Color,
    pub selected_border: Color,
    pub activity_indicator: Color,
    pub bell_indicator: Color,
    pub rename_background: Color,
    pub rename_border: Color,
    pub rename_text: Color,

    // [appearance.tabs.overflow]
    pub overflow_count_background: Color,

    // [appearance.groups]
    /// `count_background` -- reaproveitado (mesma nota do TOML) como cor de
    /// nome/caret da pílula e do "+" **sobre a cápsula cheia**, onde o
    /// claro perde contraste.
    pub group_new_tab_icon: Color,
    pub glass_border: Color,
    /// `[appearance.groups] border` -- borda de 1px da cápsula/aba solta.
    /// Reaproveitada também como borda do botão de nova aba (`NEW_TAB_BORDER`
    /// de antes desta etapa): mesmo tom neutro de contorno, nenhuma chave
    /// própria para o botão.
    pub new_tab_border: Color,
    group_colors: [Color; 6],
    ungrouped_group_color: Color,

    // Botões de janela (ADR-0027, `[appearance.window_controls]`)
    pub window_button_hover_bg: Color,
    pub window_close_hover_bg: Color,
    pub window_close_hover_icon: Color,

    // [appearance.notices] -- fundo/borda do próprio widget de aviso.
    pub popover_background: Color,
    pub popover_border: Color,
    pub warning_title_text: Color,
    pub warning_body_text: Color,
    pub warning_severity_error: Color,
    pub warning_severity_warning: Color,
    pub warning_severity_info: Color,

    // [appearance.dialog]
    pub dialog_overlay: Color,
    pub dialog_background: Color,
    pub dialog_border: Color,
    pub dialog_title_text: Color,
    pub dialog_body_text: Color,
    pub dialog_cancel_border: Color,
    pub dialog_cancel_text: Color,
    pub dialog_confirm_background: Color,
    pub dialog_confirm_text: Color,
    pub dialog_focus_ring: Color,

    // [appearance.context_menu] (reaproveitado por group_menu.rs e pelo
    // popover de destino, `[appearance.move_to_group]`, que não tem chave
    // de cor própria -- só geometria).
    pub context_menu_background: Color,
    pub context_menu_border: Color,
    pub menu_item_text: Color,
    pub menu_item_disabled_text: Color,
    pub menu_item_hover: Color,
    pub menu_item_destructive_text: Color,
    pub menu_item_destructive_hover: Color,

    // [appearance.tooltip]
    pub tooltip_background: Color,
    pub tooltip_border: Color,
    pub tooltip_text: Color,

    // [appearance.group_editor]
    pub editor_background: Color,
    pub editor_border: Color,
    pub editor_section_text: Color,
    pub editor_input_background: Color,
    pub editor_input_border: Color,
    pub editor_input_border_focus: Color,
    pub editor_input_text: Color,
    pub editor_swatch_ring: Color,
    pub editor_divider: Color,
    pub editor_item_foreground: Color,
    pub editor_item_hover_background: Color,
    pub editor_destructive_foreground: Color,
    pub editor_destructive_hover_background: Color,

    // [appearance.groups] -- contador de abas do popover de destino.
    pub pill_count_text: Color,
}

impl ResolvedPalette {
    pub fn from_config(config: &porecatu_config::Config) -> Self {
        let tabs = &config.appearance.tabs;
        let groups = &config.appearance.groups;
        let window_controls = &config.appearance.window_controls;
        let notices = &config.appearance.notices;
        let dialog = &config.appearance.dialog;
        let context_menu = &config.appearance.context_menu;
        let tooltip = &config.appearance.tooltip;
        let editor = &config.appearance.group_editor;

        let mut group_colors = [TRANSPARENT; 6];
        for (slot, entry) in group_colors.iter_mut().zip(groups.palette.iter()) {
            *slot = cvt(entry.color);
        }

        Self {
            bar_background: cvt(tabs.colors.bar_background),
            tab_active_background: cvt_alpha(
                tabs.colors.active_background,
                tabs.colors.background_alpha,
            ),
            tab_active_border: cvt(tabs.colors.active_border),
            tab_active_text: cvt(tabs.colors.active_foreground),
            tab_inactive_background: cvt_alpha(
                tabs.colors.inactive_background,
                tabs.colors.background_alpha,
            ),
            tab_inactive_border: cvt(tabs.colors.inactive_border),
            tab_inactive_text: cvt(tabs.colors.inactive_foreground),
            tab_exited_text: cvt(tabs.colors.exited_foreground),
            chrome_icon: cvt(tabs.colors.close_button_foreground),
            selected_border: cvt(tabs.colors.selected_border),
            activity_indicator: cvt(tabs.colors.activity_indicator),
            bell_indicator: cvt(tabs.colors.bell_indicator),
            rename_background: cvt(tabs.colors.rename_background),
            rename_border: cvt(tabs.colors.rename_border),
            rename_text: cvt(tabs.colors.rename_foreground),

            overflow_count_background: cvt(tabs.overflow.indicator_background),

            group_new_tab_icon: cvt(groups.count_background),
            // `glass_border` (RGB) e `glass_border_alpha` são chaves
            // separadas -- o rim de vidro (espec. §1.2/§2.3/§2.4) é
            // translúcido (`.16`), não o branco opaco que `cvt` sozinho
            // daria (o alfa do próprio literal `"#ffffff"` é `0xff`).
            glass_border: cvt_alpha(groups.glass_border, groups.glass_border_alpha),
            new_tab_border: cvt(groups.border),
            group_colors,
            ungrouped_group_color: cvt(groups.ungrouped_color),

            window_button_hover_bg: cvt(window_controls.hover_background),
            window_close_hover_bg: cvt(window_controls.close_hover_background),
            window_close_hover_icon: cvt(window_controls.close_hover_foreground),

            popover_background: cvt(notices.background),
            popover_border: cvt(notices.border),
            warning_title_text: cvt(notices.foreground),
            warning_body_text: cvt(notices.body_foreground),
            warning_severity_error: cvt(notices.error),
            warning_severity_warning: cvt(notices.warning),
            warning_severity_info: cvt(notices.info),

            dialog_overlay: cvt(dialog.overlay),
            dialog_background: cvt(dialog.background),
            dialog_border: cvt(dialog.border),
            dialog_title_text: cvt(dialog.title_foreground),
            dialog_body_text: cvt(dialog.foreground),
            dialog_cancel_border: cvt(dialog.button_border),
            dialog_cancel_text: cvt(dialog.foreground),
            dialog_confirm_background: cvt(dialog.destructive_foreground),
            // Texto escuro sobre o botão destrutivo claro -- mesmo tom do
            // fundo do diálogo, sem chave própria (nenhuma cor nova:
            // reaproveita `dialog.background`).
            dialog_confirm_text: cvt(dialog.background),
            // Espec. §2.15: "o mesmo acento do campo de rename... aqui no
            // seu terceiro papel" -- o token é `selected_border`
            // (`[appearance.tabs.colors]`), não uma chave própria do
            // diálogo.
            dialog_focus_ring: cvt(tabs.colors.selected_border),

            context_menu_background: cvt(context_menu.background),
            context_menu_border: cvt(context_menu.border),
            menu_item_text: cvt(context_menu.foreground),
            menu_item_disabled_text: cvt(context_menu.disabled_foreground),
            menu_item_hover: cvt(context_menu.item_hover_background),
            menu_item_destructive_text: cvt(context_menu.destructive_foreground),
            menu_item_destructive_hover: cvt(dialog.destructive_hover_background),

            tooltip_background: cvt(tooltip.background),
            tooltip_border: cvt(tooltip.border),
            tooltip_text: cvt(tooltip.foreground),

            editor_background: cvt(editor.background),
            editor_border: cvt(editor.border),
            editor_section_text: cvt(editor.section_foreground),
            editor_input_background: cvt(editor.input_background),
            editor_input_border: cvt(editor.input_border),
            editor_input_border_focus: cvt(editor.input_border_focus),
            editor_input_text: cvt(editor.input_foreground),
            editor_swatch_ring: cvt(editor.swatch_ring_selected),
            editor_divider: cvt(editor.divider),
            editor_item_foreground: cvt(editor.item_foreground),
            editor_item_hover_background: cvt(editor.item_hover_background),
            editor_destructive_foreground: cvt(editor.destructive_foreground),
            editor_destructive_hover_background: cvt(editor.destructive_hover_background),

            pill_count_text: cvt(groups.count_foreground),
        }
    }

    /// Resolve `GroupColor` (não resolvida, `porecatu-core`) para a cor
    /// concreta -- array já resolvido de `[appearance.groups] palette`, na
    /// mesma ordem de `GroupColor::ALL` (ordem de atribuição automática,
    /// ADR-0020 §5). Chamado por grupo, por frame -- não por célula.
    pub fn group_color(&self, color: GroupColor) -> Color {
        match color {
            GroupColor::Red => self.group_colors[0],
            GroupColor::Yellow => self.group_colors[1],
            GroupColor::Cyan => self.group_colors[2],
            GroupColor::Blue => self.group_colors[3],
            GroupColor::Purple => self.group_colors[4],
            GroupColor::Green => self.group_colors[5],
        }
    }

    /// `[appearance.groups] ungrouped_color` -- a cor de grupo das abas de
    /// um run implícito (ADR-0006). Run implícito não pinta cápsula, então
    /// quem usa esta cor é o realce de fronteira do arraste sobre ele, o
    /// fantasma do arraste de grupo e a linha do popover de destino.
    pub fn ungrouped_group_color(&self) -> Color {
        self.ungrouped_group_color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_resolves_group_palette_in_order() {
        let pal = ResolvedPalette::from_config(&porecatu_config::Config::default());
        assert_eq!(pal.group_color(GroupColor::Red), hex(0xef, 0x8a, 0x8a));
        assert_eq!(pal.group_color(GroupColor::Green), hex(0x86, 0xc5, 0x6a));
        assert_eq!(pal.ungrouped_group_color(), hex(0x7b, 0x83, 0x8f));
    }

    #[test]
    fn tab_background_applies_background_alpha() {
        let pal = ResolvedPalette::from_config(&porecatu_config::Config::default());
        assert_eq!(pal.tab_active_background, hex_alpha(0x28, 0x2e, 0x37, 0.85));
    }

    /// Auditoria da etapa (ADR-0028): cada campo de `ResolvedPalette` para
    /// a config padrão tem de bater com o literal que o binário desenhava
    /// antes desta etapa (as `const` que existiam aqui mesmo, em
    /// `palette.rs`, antes de virarem campos resolvidos de `Config`).
    /// Pega divergência de fiação (chave errada, alfa esquecido) que os
    /// testes pontuais acima não cobrem.
    #[test]
    fn from_config_matches_every_pre_etapa_constant() {
        let pal = ResolvedPalette::from_config(&porecatu_config::Config::default());

        assert_eq!(pal.bar_background, hex(0x1b, 0x1f, 0x26));
        assert_eq!(pal.tab_active_background, hex_alpha(0x28, 0x2e, 0x37, 0.85));
        assert_eq!(pal.tab_active_border, hex(0x39, 0x40, 0x4b));
        assert_eq!(pal.tab_active_text, hex(0xea, 0xee, 0xf3));
        assert_eq!(
            pal.tab_inactive_background,
            hex_alpha(0x19, 0x1d, 0x23, 0.85)
        );
        assert_eq!(pal.tab_inactive_border, hex(0x22, 0x26, 0x2e));
        assert_eq!(pal.tab_inactive_text, hex(0x98, 0xa0, 0xab));
        assert_eq!(pal.tab_exited_text, hex(0x72, 0x7a, 0x86));
        assert_eq!(pal.chrome_icon, hex(0xe4, 0xe8, 0xee));
        assert_eq!(pal.selected_border, hex(0x5e, 0xd3, 0xbc));
        assert_eq!(pal.activity_indicator, hex(0x86, 0xc5, 0x6a));
        assert_eq!(pal.bell_indicator, hex(0xef, 0x8a, 0x8a));
        assert_eq!(pal.rename_background, hex(0x0e, 0x11, 0x16));
        assert_eq!(pal.rename_border, hex(0x5e, 0xd3, 0xbc));
        assert_eq!(pal.rename_text, hex(0xe4, 0xe8, 0xee));

        assert_eq!(pal.overflow_count_background, hex(0x12, 0x15, 0x1a));

        assert_eq!(pal.group_new_tab_icon, hex(0x12, 0x15, 0x1a));
        // Rim de vidro translúcido -- não branco opaco (`glass_border` e
        // `glass_border_alpha` são chaves separadas no TOML).
        assert_eq!(pal.glass_border, hex_alpha(0xff, 0xff, 0xff, 0.16));
        assert_eq!(pal.new_tab_border, hex(0x26, 0x2b, 0x34));

        assert_eq!(pal.window_button_hover_bg, hex(0x25, 0x2a, 0x33));
        assert_eq!(pal.window_close_hover_bg, hex(0xc4, 0x41, 0x3f));
        assert_eq!(pal.window_close_hover_icon, hex(0xff, 0xff, 0xff));

        assert_eq!(pal.popover_background, hex(0x1a, 0x1e, 0x25));
        assert_eq!(pal.popover_border, hex(0x2e, 0x34, 0x3e));
        assert_eq!(pal.warning_title_text, hex(0xdf, 0xe4, 0xea));
        assert_eq!(pal.warning_body_text, hex(0x6b, 0x73, 0x7e));
        assert_eq!(pal.warning_severity_error, hex(0xef, 0x8a, 0x8a));
        assert_eq!(pal.warning_severity_warning, hex(0xe0, 0xb0, 0x60));
        assert_eq!(pal.warning_severity_info, hex(0x5e, 0xd3, 0xbc));

        assert_eq!(
            pal.dialog_overlay,
            hex_alpha(0x06, 0x07, 0x09, 0x73 as f64 / 255.0)
        );
        assert_eq!(pal.dialog_background, hex(0x1a, 0x1e, 0x25));
        assert_eq!(pal.dialog_border, hex(0x2e, 0x34, 0x3e));
        assert_eq!(pal.dialog_title_text, hex(0xe6, 0xea, 0xef));
        assert_eq!(pal.dialog_body_text, hex(0xd7, 0xdc, 0xe3));
        assert_eq!(pal.dialog_cancel_border, hex(0x26, 0x2b, 0x34));
        assert_eq!(pal.dialog_cancel_text, hex(0xd7, 0xdc, 0xe3));
        assert_eq!(pal.dialog_confirm_background, hex(0xe0, 0x85, 0x85));
        assert_eq!(pal.dialog_confirm_text, hex(0x1a, 0x1e, 0x25));
        assert_eq!(pal.dialog_focus_ring, hex(0x5e, 0xd3, 0xbc));

        assert_eq!(pal.context_menu_background, hex(0x1a, 0x1e, 0x25));
        assert_eq!(pal.context_menu_border, hex(0x2e, 0x34, 0x3e));
        assert_eq!(pal.menu_item_text, hex(0xd7, 0xdc, 0xe3));
        assert_eq!(pal.menu_item_disabled_text, hex(0x5c, 0x64, 0x6f));
        assert_eq!(pal.menu_item_hover, hex(0x24, 0x2a, 0x33));
        assert_eq!(pal.menu_item_destructive_text, hex(0xe0, 0x85, 0x85));
        assert_eq!(pal.menu_item_destructive_hover, hex(0x2e, 0x22, 0x24));

        assert_eq!(pal.tooltip_background, hex(0x1a, 0x1e, 0x25));
        assert_eq!(pal.tooltip_border, hex(0x2e, 0x34, 0x3e));
        assert_eq!(pal.tooltip_text, hex(0xd7, 0xdc, 0xe3));

        assert_eq!(pal.editor_background, hex(0x1a, 0x1e, 0x25));
        assert_eq!(pal.editor_border, hex(0x2e, 0x34, 0x3e));
        assert_eq!(pal.editor_section_text, hex(0x5c, 0x64, 0x6f));
        assert_eq!(pal.editor_input_background, hex(0x0f, 0x12, 0x16));
        assert_eq!(pal.editor_input_border, hex(0x33, 0x3a, 0x45));
        assert_eq!(pal.editor_input_border_focus, hex(0x5e, 0xd3, 0xbc));
        assert_eq!(pal.editor_input_text, hex(0xe4, 0xe8, 0xee));
        assert_eq!(pal.editor_swatch_ring, hex(0xee, 0xf2, 0xf4));
        assert_eq!(pal.editor_divider, hex(0x2a, 0x2f, 0x38));
        assert_eq!(pal.editor_item_foreground, hex(0xd7, 0xdc, 0xe3));
        assert_eq!(pal.editor_item_hover_background, hex(0x24, 0x2a, 0x33));
        assert_eq!(pal.editor_destructive_foreground, hex(0xe0, 0x85, 0x85));
        assert_eq!(
            pal.editor_destructive_hover_background,
            hex(0x2e, 0x22, 0x24)
        );

        assert_eq!(pal.pill_count_text, hex(0x7b, 0x83, 0x8f));

        assert_eq!(pal.group_color(GroupColor::Red), hex(0xef, 0x8a, 0x8a));
        assert_eq!(pal.group_color(GroupColor::Yellow), hex(0xe0, 0xb0, 0x60));
        assert_eq!(pal.group_color(GroupColor::Cyan), hex(0x5e, 0xd3, 0xbc));
        assert_eq!(pal.group_color(GroupColor::Blue), hex(0x6f, 0xa8, 0xf5));
        assert_eq!(pal.group_color(GroupColor::Purple), hex(0xa6, 0x8c, 0xf0));
        assert_eq!(pal.group_color(GroupColor::Green), hex(0x86, 0xc5, 0x6a));
        assert_eq!(pal.ungrouped_group_color(), hex(0x7b, 0x83, 0x8f));
    }

    /// Auditoria da etapa (ADR-0028), mesmo espírito de
    /// `from_config_matches_every_pre_etapa_constant`: `ResolvedTermPalette`
    /// para a config padrão bate com cada `TERM_*`/`ANSI_*` que existia
    /// aqui como `const` antes desta etapa.
    #[test]
    fn term_palette_from_config_matches_every_pre_etapa_constant() {
        let pal = ResolvedTermPalette::from_config(&porecatu_config::Config::default());

        assert_eq!(pal.foreground, hex(0xc7, 0xcc, 0xd6));
        assert_eq!(pal.background, hex(0x0f, 0x12, 0x16));
        assert_eq!(pal.cursor, hex(0x5e, 0xd3, 0xbc));
        assert_eq!(pal.cursor_text, hex(0x0f, 0x12, 0x16));
        assert_eq!(pal.selection_background, hex(0x2e, 0x6b, 0x62));
        assert_eq!(pal.selection_foreground, hex(0xee, 0xf2, 0xf4));
        assert_eq!(pal.prompt_secondary, hex(0x6b, 0x73, 0x7e));

        let normal = [
            hex(0x3b, 0x43, 0x4f),
            hex(0xef, 0x8a, 0x8a),
            hex(0x86, 0xc5, 0x6a),
            hex(0xe0, 0xb0, 0x60),
            hex(0x6f, 0xa8, 0xf5),
            hex(0xa6, 0x8c, 0xf0),
            hex(0x5e, 0xd3, 0xbc),
            hex(0xc7, 0xcc, 0xd6),
        ];
        let bright = [
            hex(0x6f, 0x77, 0x83),
            hex(0xf5, 0xa3, 0xa3),
            hex(0x9b, 0xd4, 0x82),
            hex(0xec, 0xc3, 0x7c),
            hex(0x8d, 0xbc, 0xf8),
            hex(0xbd, 0xa6, 0xf5),
            hex(0x7f, 0xdf, 0xcc),
            hex(0xea, 0xee, 0xf3),
        ];
        for i in 0..8 {
            assert_eq!(
                pal.resolve(TermColor::Indexed(i), true, false),
                normal[i as usize],
                "normal[{i}]"
            );
            assert_eq!(
                pal.resolve(TermColor::Indexed(8 + i), true, false),
                bright[i as usize],
                "bright[{i}]"
            );
        }
    }

    #[test]
    fn resolve_default_picks_foreground_or_background() {
        let pal = ResolvedTermPalette::from_config(&porecatu_config::Config::default());
        assert_eq!(pal.resolve(TermColor::Default, true, false), pal.foreground);
        assert_eq!(
            pal.resolve(TermColor::Default, false, false),
            pal.background
        );
    }

    #[test]
    fn resolve_rgb_passes_through_untouched() {
        let pal = ResolvedTermPalette::from_config(&porecatu_config::Config::default());
        assert_eq!(
            pal.resolve(
                TermColor::Rgb {
                    r: 10,
                    g: 20,
                    b: 30
                },
                true,
                false
            ),
            hex(10, 20, 30)
        );
    }

    /// RF-5.17: cor de 256/true color emitida pelo programa não é afetada
    /// pela paleta -- só os índices 0..16 podem virar brilhante por
    /// `bold_is_bright`.
    #[test]
    fn resolve_256_cube_and_gray_ramp_are_not_bold_promoted() {
        let pal = ResolvedTermPalette::from_config(&porecatu_config::Config::default());
        // Índice 16: primeira célula do cubo 6x6x6, todos os canais no
        // nível 0 -- preto.
        assert_eq!(
            pal.resolve(TermColor::Indexed(16), true, true),
            hex(0, 0, 0)
        );
        // Índice 232: primeiro degrau da rampa de cinza (nível 8).
        assert_eq!(
            pal.resolve(TermColor::Indexed(232), true, true),
            hex(8, 8, 8)
        );
    }

    /// RF-5.5: negrito usa a versão brilhante da cor ANSI só quando
    /// `bold_is_bright` está ligado -- desligado (default), negrito não
    /// muda a cor.
    #[test]
    fn bold_is_bright_promotes_only_when_enabled_and_only_for_foreground() {
        let mut config = porecatu_config::Config::default();
        assert!(!config.terminal.font.bold_is_bright, "default é desligado");
        let pal_off = ResolvedTermPalette::from_config(&config);
        assert_eq!(
            pal_off.resolve(TermColor::Indexed(1), true, true),
            pal_off.resolve(TermColor::Indexed(1), true, false),
            "desligado: negrito não promove"
        );

        config.terminal.font.bold_is_bright = true;
        let pal_on = ResolvedTermPalette::from_config(&config);
        assert_eq!(
            pal_on.resolve(TermColor::Indexed(1), true, true),
            hex(0xf5, 0xa3, 0xa3),
            "ligado: vermelho negrito vira o vermelho brilhante"
        );
        assert_eq!(
            pal_on.resolve(TermColor::Indexed(1), false, true),
            hex(0xef, 0x8a, 0x8a),
            "RF-5.5 é convenção de texto -- fundo nunca promove, mesmo com bold_is_bright"
        );
    }
}
