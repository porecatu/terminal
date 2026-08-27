// SPDX-License-Identifier: GPL-3.0-or-later

//! Layout e hit-testing da barra de abas -- função pura, sem `wgpu` e sem
//! janela (docs/arquitetura.md seção 7): `(Workspace, TabBarStyle,
//! TextMeasurer) -> TabBarLayout`. Cobre só o que a espec. visual §2.2,
//! §2.3, §2.5 e §2.6 descrevem como geometria de trilha -- nada de
//! encolhimento/rolagem (Etapa 5), indicadores (Etapa 5) nem arraste
//! (Etapa 5). Pintura (`chrome.rs`) e wiring de clique/rename (`lib.rs`)
//! chegam na Etapa 4.
//!
//! `porecatu-config` ainda não existe: os valores de [`TabBarStyle`] são
//! constantes com a chave TOML de origem no comentário, no mesmo padrão de
//! `palette.rs` (F1). Valor sem chave é geometria fixa da espec. visual,
//! citada por seção.

use porecatu_core::{GroupId, TabId, Workspace};
use porecatu_render::{FontFace, Rect, SansWeight, TextMeasurer};

/// Fonte de rótulo de aba (espec. §1.1: "12.5px, rótulo da aba").
const LABEL_FONT: FontFace = FontFace::Sans {
    weight: SansWeight::Regular,
};

/// Valores geométricos da barra, hoje fixos no código (ver módulo).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabBarStyle {
    /// `[appearance.tabs] tab_height`
    pub tab_height: f32,
    /// `[appearance.tabs] max_width` -- teto do conjunto padding+rótulo+
    /// botão de fechar; na prática nunca é atingido em F2 porque o
    /// rótulo já trunca em `label_max_width` antes disso.
    pub max_width: f32,
    /// Espec. §2.5: "Rótulo 12.5px, max-width: 180px". Sem chave própria
    /// no TOML -- é um comentário sobre `max_width`, não uma chave.
    pub label_max_width: f32,
    /// `[appearance.tabs] padding_left`
    pub padding_left: f32,
    /// `[appearance.tabs] padding_right`
    pub padding_right: f32,
    /// `[appearance.tabs] gap` -- entre abas do mesmo grupo.
    pub tab_gap: f32,
    /// Espec. §2.5: "Aba ... gap: 8" entre o rótulo e o botão de fechar.
    /// Sem chave própria no TOML.
    pub internal_gap: f32,
    /// Espec. §1.7: "Botão de fechar da aba 17×17". Sem chave própria.
    pub close_button_size: f32,
    /// Espec. §2.2: "hit-testing dá 2px de folga em volta do botão de
    /// fechar". Sem chave própria.
    pub close_button_hit_slop: f32,
    /// `[appearance.groups] wrapper_padding`
    pub wrapper_padding: f32,
    /// `[appearance.groups] gap` -- entre grupos, e também o gap da
    /// trilha antes do botão de nova aba (espec. §2.2: "Trilha ...
    /// gap: 6" é o mesmo valor, aplicado aos mesmos filhos diretos).
    pub trilha_gap: f32,
    /// Espec. §2.6: "30×30". Sem chave própria -- usa o mesmo valor de
    /// `tab_height`.
    pub new_tab_button_size: f32,
    /// `[appearance.tabs] show_new_tab_button`
    pub show_new_tab_button: bool,
    /// `[appearance.tabs] font_size`
    pub font_size: f32,
}

impl TabBarStyle {
    pub const DEFAULT: Self = Self {
        tab_height: 30.0,
        max_width: 260.0,
        label_max_width: 180.0,
        padding_left: 10.0,
        padding_right: 6.0,
        tab_gap: 4.0,
        internal_gap: 8.0,
        close_button_size: 17.0,
        close_button_hit_slop: 2.0,
        wrapper_padding: 3.0,
        trilha_gap: 6.0,
        new_tab_button_size: 30.0,
        show_new_tab_button: true,
        font_size: 12.5,
    };
}

impl Default for TabBarStyle {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Geometria de uma aba dentro da trilha, em coordenadas relativas ao
/// topo-esquerda da trilha (a posição da barra na janela, e o padding `6px
/// 10px` dela, são responsabilidade de quem pinta -- não deste layout).
#[derive(Debug, Clone, PartialEq)]
pub struct TabRect {
    pub id: TabId,
    /// Retângulo visual (fundo, borda).
    pub rect: Rect,
    /// Retângulo do botão de fechar, sem a folga de acerto.
    pub close_button: Rect,
    /// Rótulo já truncado (RF-1.10), com reticências se `label_truncated`.
    pub label: String,
    /// Decide o tooltip do ADR-0019 -- calculado aqui porque o
    /// `TextMeasurer` já está em mãos, consumido só a partir da etapa que
    /// desenha o tooltip.
    pub label_truncated: bool,
    /// Área de hit-test do corpo da aba: o retângulo visual estendido até
    /// a metade do `gap` para a aba vizinha do mesmo grupo (espec. §2.2:
    /// "a fronteira entre abas vizinhas parte o gap ao meio"). Não se
    /// estende para além dos limites do wrapper -- o padding do wrapper e
    /// o gap entre grupos não têm essa regra.
    hit_rect: Rect,
    /// Retângulo de hit-test do botão de fechar, já com a folga de
    /// acerto de 2px.
    close_hit_rect: Rect,
}

/// Abas de um grupo, com o retângulo do wrapper que as envolve (espec.
/// §2.3). Na F2 só existe o grupo implícito -- sem pílula, sem nome, sem
/// cor -- mas a estrutura já suporta múltiplos grupos porque
/// `Workspace::groups()` já suporta (F3 os preenche).
#[derive(Debug, Clone, PartialEq)]
pub struct GroupWrapperRect {
    pub id: GroupId,
    pub rect: Rect,
    pub tabs: Vec<TabRect>,
}

/// O layout inteiro da trilha: um por redraw da barra, construído por
/// [`layout`].
#[derive(Debug, Clone, PartialEq)]
pub struct TabBarLayout {
    /// Grupos vazios não aparecem aqui -- um wrapper sem aba nenhuma não
    /// é desenhável (ver `layout`).
    pub groups: Vec<GroupWrapperRect>,
    /// `None` quando `style.show_new_tab_button` é `false`.
    pub new_tab_button: Option<Rect>,
    /// Largura total ocupada pela trilha, sem clamping à largura
    /// disponível da janela -- overflow (encolher, rolar) é a Etapa 5.
    pub content_width: f32,
}

/// O que um ponto da trilha atinge, em prioridade: botão de fechar antes
/// do corpo da aba (eles se sobrepõem), corpo da aba, botão de nova aba.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabBarHit {
    Tab(TabId),
    CloseButton(TabId),
    NewTabButton,
}

/// Constrói a geometria da trilha: um wrapper por grupo não-vazio, abas
/// dentro dele, e o botão de nova aba ao final. Não sabe qual aba está
/// ativa, em hover ou sendo renomeada -- isso é estado de `porecatu-ui`
/// que colore o resultado deste layout, não entrada dele.
pub fn layout(
    workspace: &Workspace,
    style: &TabBarStyle,
    measurer: &mut TextMeasurer,
) -> TabBarLayout {
    let mut groups = Vec::new();
    let mut x = 0.0f32;

    for group in workspace.groups() {
        if group.tabs().is_empty() {
            continue;
        }
        if !groups.is_empty() {
            x += style.trilha_gap;
        }
        let group_start_x = x;
        let mut inner_x = x + style.wrapper_padding;
        let mut tabs = Vec::with_capacity(group.tabs().len());

        for (index, &tab_id) in group.tabs().iter().enumerate() {
            let Some(tab) = workspace.tab(tab_id) else {
                continue;
            };
            if index > 0 {
                inner_x += style.tab_gap;
            }

            let (label, label_truncated) = measurer.truncate(
                tab.title(),
                LABEL_FONT,
                style.font_size,
                style.label_max_width,
            );
            let label_width = measurer.measure_width(&label, LABEL_FONT, style.font_size);
            let content_width = style.padding_left
                + label_width
                + style.internal_gap
                + style.close_button_size
                + style.padding_right;
            let tab_width = content_width.min(style.max_width);

            let rect = Rect {
                x: inner_x,
                y: style.wrapper_padding,
                width: tab_width,
                height: style.tab_height,
            };
            let close_button = Rect {
                x: inner_x + tab_width - style.padding_right - style.close_button_size,
                y: style.wrapper_padding + (style.tab_height - style.close_button_size) / 2.0,
                width: style.close_button_size,
                height: style.close_button_size,
            };
            let close_hit_rect = expand(close_button, style.close_button_hit_slop);

            let hit_left = if index == 0 {
                rect.x
            } else {
                rect.x - style.tab_gap / 2.0
            };
            let is_last = index + 1 == group.tabs().len();
            let hit_right = if is_last {
                rect.x + rect.width
            } else {
                rect.x + rect.width + style.tab_gap / 2.0
            };
            let hit_rect = Rect {
                x: hit_left,
                y: rect.y,
                width: hit_right - hit_left,
                height: rect.height,
            };

            tabs.push(TabRect {
                id: tab_id,
                rect,
                close_button,
                label,
                label_truncated,
                hit_rect,
                close_hit_rect,
            });
            inner_x += tab_width;
        }

        inner_x += style.wrapper_padding;
        let wrapper_rect = Rect {
            x: group_start_x,
            y: 0.0,
            width: inner_x - group_start_x,
            height: style.tab_height + style.wrapper_padding * 2.0,
        };
        groups.push(GroupWrapperRect {
            id: group.id(),
            rect: wrapper_rect,
            tabs,
        });
        x = inner_x;
    }

    let new_tab_button = if style.show_new_tab_button {
        if !groups.is_empty() {
            x += style.trilha_gap;
        }
        let button = Rect {
            x,
            y: (style.tab_height + style.wrapper_padding * 2.0 - style.new_tab_button_size) / 2.0,
            width: style.new_tab_button_size,
            height: style.new_tab_button_size,
        };
        x += style.new_tab_button_size;
        Some(button)
    } else {
        None
    };

    TabBarLayout {
        groups,
        new_tab_button,
        content_width: x,
    }
}

/// Resolve o que `point` (coordenadas relativas ao topo-esquerda da
/// trilha, as mesmas de [`layout`]) atinge. Botão de fechar tem
/// prioridade sobre o corpo da aba onde os dois se sobrepõem.
pub fn hit_test(layout: &TabBarLayout, point: (f32, f32)) -> Option<TabBarHit> {
    for group in &layout.groups {
        for tab in &group.tabs {
            if rect_contains(tab.close_hit_rect, point) {
                return Some(TabBarHit::CloseButton(tab.id));
            }
        }
    }
    for group in &layout.groups {
        for tab in &group.tabs {
            if rect_contains(tab.hit_rect, point) {
                return Some(TabBarHit::Tab(tab.id));
            }
        }
    }
    if let Some(button) = layout.new_tab_button
        && rect_contains(button, point)
    {
        return Some(TabBarHit::NewTabButton);
    }
    None
}

fn expand(rect: Rect, amount: f32) -> Rect {
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        width: rect.width + amount * 2.0,
        height: rect.height + amount * 2.0,
    }
}

fn rect_contains(rect: Rect, point: (f32, f32)) -> bool {
    let (x, y) = point;
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measurer() -> TextMeasurer {
        TextMeasurer::new()
    }

    #[test]
    fn empty_workspace_has_only_new_tab_button() {
        let ws = Workspace::new();
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert!(layout.groups.is_empty());
        let button = layout
            .new_tab_button
            .expect("botão de nova aba visível por default");
        assert_eq!(button.x, 0.0);
        assert_eq!(layout.content_width, button.x + button.width);
    }

    #[test]
    fn hides_new_tab_button_when_disabled() {
        let ws = Workspace::new();
        let mut m = measurer();
        let style = TabBarStyle {
            show_new_tab_button: false,
            ..TabBarStyle::DEFAULT
        };
        let layout = layout(&ws, &style, &mut m);
        assert_eq!(layout.new_tab_button, None);
        assert_eq!(layout.content_width, 0.0);
    }

    #[test]
    fn single_tab_lays_out_left_to_right() {
        let mut ws = Workspace::new();
        ws.new_tab("zsh", None, 0);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);

        assert_eq!(layout.groups.len(), 1);
        let group = &layout.groups[0];
        assert_eq!(group.tabs.len(), 1);
        let tab = &group.tabs[0];
        assert_eq!(tab.label, "zsh");
        assert!(!tab.label_truncated);
        assert_eq!(tab.rect.x, TabBarStyle::DEFAULT.wrapper_padding);

        // botão de nova aba vem depois do wrapper, com o gap da trilha
        let button = layout.new_tab_button.unwrap();
        assert_eq!(
            button.x,
            group.rect.x + group.rect.width + TabBarStyle::DEFAULT.trilha_gap
        );
        assert_eq!(layout.content_width, button.x + button.width);
    }

    #[test]
    fn long_title_is_truncated_with_ellipsis() {
        let mut ws = Workspace::new();
        let id = ws.new_tab("zsh", None, 0);
        ws.tab_mut(id)
            .unwrap()
            .set_custom_title(Some("um titulo bem comprido que estoura o maximo de 180px de largura reservado ao rotulo da aba".to_string()));
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let tab = &layout.groups[0].tabs[0];
        assert!(tab.label_truncated);
        assert!(tab.label.ends_with('…'));
        let width = m.measure_width(&tab.label, LABEL_FONT, TabBarStyle::DEFAULT.font_size);
        assert!(width <= TabBarStyle::DEFAULT.label_max_width);
    }

    #[test]
    fn two_tabs_same_group_are_gapped() {
        let mut ws = Workspace::new();
        ws.new_tab("zsh", None, 0);
        ws.new_tab("bash", None, 1);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let tabs = &layout.groups[0].tabs;
        assert_eq!(tabs.len(), 2);
        assert_eq!(
            tabs[1].rect.x,
            tabs[0].rect.x + tabs[0].rect.width + TabBarStyle::DEFAULT.tab_gap
        );
    }

    #[test]
    fn empty_group_produces_no_wrapper() {
        // Grupo implícito sem abas: `Workspace::new()` já cobre isso --
        // reforça que nenhum wrapper vazio aparece no layout.
        let ws = Workspace::new();
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert!(layout.groups.is_empty());
    }

    #[test]
    fn hit_test_close_button_wins_over_tab_body() {
        let mut ws = Workspace::new();
        let id = ws.new_tab("zsh", None, 0);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let close = layout.groups[0].tabs[0].close_button;
        let center = (close.x + close.width / 2.0, close.y + close.height / 2.0);
        assert_eq!(hit_test(&layout, center), Some(TabBarHit::CloseButton(id)));
    }

    #[test]
    fn hit_test_close_button_slop_extends_hit_area() {
        let mut ws = Workspace::new();
        let id = ws.new_tab("zsh", None, 0);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let close = layout.groups[0].tabs[0].close_button;
        // 1px fora do botão visual, mas dentro da folga de 2px.
        let just_outside = (close.x - 1.0, close.y + close.height / 2.0);
        assert_eq!(
            hit_test(&layout, just_outside),
            Some(TabBarHit::CloseButton(id))
        );
    }

    #[test]
    fn hit_test_tab_body_away_from_close_button() {
        let mut ws = Workspace::new();
        let id = ws.new_tab("zsh", None, 0);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let tab = &layout.groups[0].tabs[0];
        let point = (tab.rect.x + 2.0, tab.rect.y + 2.0);
        assert_eq!(hit_test(&layout, point), Some(TabBarHit::Tab(id)));
    }

    #[test]
    fn hit_test_gap_boundary_splits_at_midpoint() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let b = ws.new_tab("bash", None, 1);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let first = &layout.groups[0].tabs[0];
        let gap_start = first.rect.x + first.rect.width;
        let midpoint = gap_start + TabBarStyle::DEFAULT.tab_gap / 2.0;

        // Um pouco antes do meio do gap: ainda pertence à primeira aba.
        let just_before = (midpoint - 0.5, first.rect.y + 1.0);
        assert_eq!(hit_test(&layout, just_before), Some(TabBarHit::Tab(a)));

        // Um pouco depois: já pertence à segunda.
        let just_after = (midpoint + 0.5, first.rect.y + 1.0);
        assert_eq!(hit_test(&layout, just_after), Some(TabBarHit::Tab(b)));
    }

    #[test]
    fn hit_test_new_tab_button() {
        let ws = Workspace::new();
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let button = layout.new_tab_button.unwrap();
        let center = (
            button.x + button.width / 2.0,
            button.y + button.height / 2.0,
        );
        assert_eq!(hit_test(&layout, center), Some(TabBarHit::NewTabButton));
    }

    #[test]
    fn hit_test_outside_everything_is_none() {
        let mut ws = Workspace::new();
        ws.new_tab("zsh", None, 0);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(hit_test(&layout, (-100.0, -100.0)), None);
        assert_eq!(hit_test(&layout, (100_000.0, 100_000.0)), None);
    }
}
