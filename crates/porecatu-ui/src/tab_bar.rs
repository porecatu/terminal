// SPDX-License-Identifier: GPL-3.0-or-later

//! Layout e hit-testing da barra de abas -- função pura, sem `wgpu` e sem
//! janela (docs/arquitetura.md seção 7): `(Workspace, TabBarStyle,
//! TextMeasurer) -> TabBarLayout`. Cobre a geometria de trilha da espec.
//! visual §2.2, §2.3, §2.5, §2.6 e, desde a Etapa 5, o encolhimento de
//! rótulo e a rolagem do §2.18 (`fit_width`/`overflow_state`) e a geometria
//! de arraste do §2.19 (`drag_target_index`). Pintura (`chrome.rs`) e
//! wiring de clique/rename/arraste (`lib.rs`) ficam do outro lado da
//! fronteira -- este módulo não sabe de `wgpu` nem de `winit`.
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

/// Espec. §2.17: "ponto circular 6×6". Consome largura do rótulo (mais o
/// `internal_gap` reaproveitado como o `gap: 8` da mesma seção) -- não é
/// chrome extra somado ao teto de 180px, é orçamento tirado dele. Visível a
/// `chrome.rs` para desenhar o ponto na mesma posição que este módulo
/// reservou.
pub(crate) const INDICATOR_DOT_SIZE: f32 = 6.0;

/// Qual dos dois indicadores da aba (espec. §2.17, RF-1.20/RF-1.21) mostrar.
/// Só um por vez -- campainha vence atividade quando os dois são
/// verdadeiros (a regra "um ponto só" da espec.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    Activity,
    Bell,
}

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
    /// `[appearance.tabs] min_width` -- piso do encolhimento (espec. §2.18)
    /// antes de a trilha ganhar rolagem. `41px` de cromo fixo (padding +
    /// gap + botão de fechar) sobram `49px` de rótulo no piso, calculado
    /// por [`fit_width`] a partir deste valor, não hardcoded direto.
    pub min_width: f32,
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
        min_width: 90.0,
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
    /// Indicador de atividade/campainha (espec. §2.17), já resolvido a
    /// partir do estado da aba -- `None` para aba `Exited` ou sem nenhum
    /// dos dois fatos pendentes.
    pub indicator: Option<Indicator>,
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

            // Espec. §2.17: aba `Exited` não mostra indicador nenhum;
            // campainha vence atividade quando as duas são verdadeiras.
            let indicator = if tab.is_exited() {
                None
            } else if tab.bell() {
                Some(Indicator::Bell)
            } else if tab.activity() {
                Some(Indicator::Activity)
            } else {
                None
            };
            // O ponto consome largura do rótulo, não soma chrome novo
            // (§2.17: "a aba não muda de largura por causa do
            // indicador") -- o teto de rótulo encolhe pelo tamanho do
            // ponto mais o mesmo `gap: 8` que já separa rótulo e botão de
            // fechar.
            let dot_reserve = if indicator.is_some() {
                INDICATOR_DOT_SIZE + style.internal_gap
            } else {
                0.0
            };
            let label_cap = (style.label_max_width - dot_reserve).max(0.0);

            let (label, label_truncated) =
                measurer.truncate(tab.title(), LABEL_FONT, style.font_size, label_cap);
            let label_width = measurer.measure_width(&label, LABEL_FONT, style.font_size);
            let content_width = style.padding_left
                + dot_reserve
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
                indicator,
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

/// Piso do rótulo (espec. §2.18: "sobram 49px de rótulo") derivado do piso
/// de aba inteira (`min_width`) menos o cromo fixo que nunca cede: padding,
/// `internal_gap` e botão de fechar.
fn label_floor(style: &TabBarStyle) -> f32 {
    (style.min_width
        - style.padding_left
        - style.internal_gap
        - style.close_button_size
        - style.padding_right)
        .max(0.0)
}

/// Encolhe o teto do rótulo até caber em `available_width`, sem passar do
/// piso (espec. §2.18: "quem encolhe é só o rótulo... nada mais cede").
/// Abaixo do piso a trilha continua largando o mesmo `content_width` de
/// antes -- é o sinal para [`overflow_state`] ativar a rolagem, não
/// responsabilidade desta função (que permanece pura e sem estado de
/// scroll, como o resto do módulo).
///
/// Busca binária sobre `label_max_width`: o `content_width` resultante de
/// [`layout`] é não decrescente nesse parâmetro (nenhuma aba fica mais
/// estreita ao aumentar o teto), então a busca converge para o maior teto
/// que ainda cabe.
pub fn fit_width(
    workspace: &Workspace,
    style: &TabBarStyle,
    available_width: f32,
    measurer: &mut TextMeasurer,
) -> TabBarLayout {
    let full = layout(workspace, style, measurer);
    if full.content_width <= available_width {
        return full;
    }

    let floor = label_floor(style);
    let floor_style = TabBarStyle {
        label_max_width: floor,
        ..*style
    };
    let mut best = layout(workspace, &floor_style, measurer);
    if best.content_width >= available_width {
        // Nem no piso cabe -- devolve o layout do piso; rolar é com
        // `overflow_state`.
        return best;
    }

    let mut lo = floor;
    let mut hi = style.label_max_width;
    for _ in 0..12 {
        let mid = (lo + hi) / 2.0;
        let mid_style = TabBarStyle {
            label_max_width: mid,
            ..*style
        };
        let mid_layout = layout(workspace, &mid_style, measurer);
        if mid_layout.content_width <= available_width {
            lo = mid;
            best = mid_layout;
        } else {
            hi = mid;
        }
    }
    best
}

/// Estado de rolagem da trilha (espec. §2.18) para um `content_width` e uma
/// largura disponível dados: deslocamento já saturado em
/// `[0, content_width - available_width]`, e a contagem de abas inteiramente
/// fora da janela visível de cada lado (RF-1.19 -- "um indicador... com
/// contagem"). Aba parcialmente visível não conta como oculta.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overflow {
    pub scroll_offset: f32,
    pub hidden_left: usize,
    pub hidden_right: usize,
}

pub fn overflow_state(layout: &TabBarLayout, available_width: f32, scroll_offset: f32) -> Overflow {
    let max_scroll = (layout.content_width - available_width).max(0.0);
    let offset = scroll_offset.clamp(0.0, max_scroll);
    let window_start = offset;
    let window_end = offset + available_width;

    let mut hidden_left = 0;
    let mut hidden_right = 0;
    for group in &layout.groups {
        for tab in &group.tabs {
            let tab_end = tab.rect.x + tab.rect.width;
            if tab_end <= window_start {
                hidden_left += 1;
            } else if tab.rect.x >= window_end {
                hidden_right += 1;
            }
        }
    }
    Overflow {
        scroll_offset: offset,
        hidden_left,
        hidden_right,
    }
}

/// Lado da trilha em que um indicador de overflow (espec. §2.18) aparece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowSide {
    Left,
    Right,
}

/// Largura de trabalho da pílula de overflow: a espec. não fixa um valor --
/// o contador da pílula de grupo (§2.4) é largura variável -- então este é
/// um valor fixo generoso o bastante para chevron + contagem de até dois
/// dígitos com o padding `1px 6px` da mesma pílula. Mesmo tipo de nota que
/// `RENAME_FIELD_HEIGHT` em `chrome.rs`.
pub const OVERFLOW_PILL_WIDTH: f32 = 34.0;
pub const OVERFLOW_PILL_HEIGHT: f32 = 18.0;
/// "Nas duas pontas da trilha, por dentro" (espec. §2.18).
pub const OVERFLOW_EDGE_INSET: f32 = 4.0;
/// "Passo de 90px -- uma aba no piso -- por notch" (espec. §2.18); também o
/// passo do clique no indicador ("clique rola uma aba").
pub const OVERFLOW_SCROLL_STEP: f32 = 90.0;

/// Retângulo da pílula de overflow, em coordenadas de tela da barra (não
/// rolam com a trilha -- ficam "por dentro" das pontas, sempre visíveis).
pub fn overflow_pill_rect(side: OverflowSide, bar_width: f32, bar_height: f32) -> Rect {
    let x = match side {
        OverflowSide::Left => OVERFLOW_EDGE_INSET,
        OverflowSide::Right => bar_width - OVERFLOW_EDGE_INSET - OVERFLOW_PILL_WIDTH,
    };
    Rect {
        x,
        y: (bar_height - OVERFLOW_PILL_HEIGHT) / 2.0,
        width: OVERFLOW_PILL_WIDTH,
        height: OVERFLOW_PILL_HEIGHT,
    }
}

pub fn point_in_overflow_pill(
    side: OverflowSide,
    bar_width: f32,
    bar_height: f32,
    point: (f32, f32),
) -> bool {
    rect_contains(overflow_pill_rect(side, bar_width, bar_height), point)
}

/// Acha o retângulo (coordenadas de conteúdo, sem rolagem) de uma aba pelo
/// `id` -- usado por `lib.rs` para calcular o deslocamento de tela no início
/// do arraste (espec. §2.19) e a largura do fantasma.
pub fn tab_rect(layout: &TabBarLayout, id: TabId) -> Option<Rect> {
    layout
        .groups
        .iter()
        .flat_map(|g| &g.tabs)
        .find(|t| t.id == id)
        .map(|t| t.rect)
}

/// Índice de inserção (o mesmo que [`Group::move_within`]/
/// `Workspace::move_tab` esperam: posição entre as abas restantes, já sem a
/// arrastada) para onde `ghost_center_x` cairia entre as demais abas do
/// mesmo grupo da arrastada, comparando contra o centro de cada uma no
/// layout corrente. Espec. §2.19: "o buraco é o marcador" -- isto só monta
/// o preview de onde a aba cairia; mover de verdade é decisão de `lib.rs`
/// ao soltar.
pub fn drag_target_index(layout: &TabBarLayout, dragged: TabId, ghost_center_x: f32) -> usize {
    for group in &layout.groups {
        if group.tabs.iter().any(|t| t.id == dragged) {
            let others: Vec<&TabRect> = group.tabs.iter().filter(|t| t.id != dragged).collect();
            for (index, tab) in others.iter().enumerate() {
                let center = tab.rect.x + tab.rect.width / 2.0;
                if ghost_center_x < center {
                    return index;
                }
            }
            return others.len();
        }
    }
    0
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

pub(crate) fn rect_contains(rect: Rect, point: (f32, f32)) -> bool {
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
    fn activity_indicator_shows_when_backgrounded_activity() {
        let mut ws = Workspace::new();
        let id = ws.new_tab("zsh", None, 0);
        ws.tab_mut(id).unwrap().mark_activity();
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(
            layout.groups[0].tabs[0].indicator,
            Some(Indicator::Activity)
        );
    }

    #[test]
    fn bell_wins_over_activity() {
        let mut ws = Workspace::new();
        let id = ws.new_tab("zsh", None, 0);
        ws.tab_mut(id).unwrap().mark_activity();
        ws.tab_mut(id).unwrap().mark_bell();
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(layout.groups[0].tabs[0].indicator, Some(Indicator::Bell));
    }

    #[test]
    fn exited_tab_never_shows_indicator() {
        let mut ws = Workspace::new();
        let id = ws.new_tab("zsh", None, 0);
        ws.tab_mut(id).unwrap().mark_activity();
        ws.tab_mut(id).unwrap().mark_exited(1);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(layout.groups[0].tabs[0].indicator, None);
    }

    #[test]
    fn indicator_does_not_widen_a_truncated_tab() {
        // Espec. §2.17: "a aba não muda de largura por causa do
        // indicador" -- vale para o caso truncado, onde o teto reduzido
        // compensa o espaço do ponto. Tolerância: o truncamento decide por
        // caractere inteiro (medido, não interpolado), então os dois tetos
        // (180 e 166) podem cada um sobrar uma fração de glyph diferente
        // abaixo do próprio teto -- a garantia é "não estoura", não
        // igualdade exata ao pixel.
        let long_title = "um titulo bem comprido que estoura o maximo de 180px de largura reservado ao rotulo da aba";
        let mut ws = Workspace::new();
        let plain = ws.new_tab("zsh", None, 0);
        ws.tab_mut(plain)
            .unwrap()
            .set_custom_title(Some(long_title.to_string()));
        let with_indicator = ws.new_tab("zsh", None, 1);
        ws.tab_mut(with_indicator)
            .unwrap()
            .set_custom_title(Some(long_title.to_string()));
        ws.tab_mut(with_indicator).unwrap().mark_activity();

        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let plain_width = layout.groups[0].tabs[0].rect.width;
        let indicator_width = layout.groups[0].tabs[1].rect.width;
        assert!(
            (plain_width - indicator_width).abs() <= 10.0,
            "plain={plain_width}, com indicador={indicator_width}"
        );
    }

    #[test]
    fn fit_width_no_shrink_when_it_already_fits() {
        let mut ws = Workspace::new();
        ws.new_tab("zsh", None, 0);
        let mut m = measurer();
        let unfit = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let fit = fit_width(&ws, &TabBarStyle::DEFAULT, 2000.0, &mut m);
        assert_eq!(unfit, fit);
    }

    #[test]
    fn fit_width_shrinks_labels_to_fit_before_scrolling() {
        let mut ws = Workspace::new();
        for i in 0..3 {
            let id = ws.new_tab("zsh", None, i);
            ws.tab_mut(id)
                .unwrap()
                .set_custom_title(Some("um titulo razoavelmente longo".to_string()));
        }
        let mut m = measurer();
        let full = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        // Descobre o piso real (todas as abas no `label_floor`) pedindo um
        // `available_width` que nem ele cabe, e usa uma folga acima disso
        // -- evita fazer conta de geometria de cromo duplicada aqui.
        let floor_total = fit_width(&ws, &TabBarStyle::DEFAULT, 1.0, &mut m).content_width;
        let available = floor_total + 60.0;
        assert!(
            full.content_width > available,
            "título precisa estourar `available` pra este teste fazer sentido"
        );

        let fitted = fit_width(&ws, &TabBarStyle::DEFAULT, available, &mut m);
        assert!(fitted.content_width <= available + 1.0);
        assert!(fitted.content_width < full.content_width);
    }

    #[test]
    fn fit_width_stays_at_floor_when_it_still_does_not_fit() {
        let mut ws = Workspace::new();
        for i in 0..50 {
            ws.new_tab("zsh", None, i);
        }
        let mut m = measurer();
        let fitted = fit_width(&ws, &TabBarStyle::DEFAULT, 300.0, &mut m);
        assert!(fitted.content_width > 300.0);
        for tab in &fitted.groups[0].tabs {
            let width = m.measure_width(&tab.label, LABEL_FONT, TabBarStyle::DEFAULT.font_size);
            assert!(width <= label_floor(&TabBarStyle::DEFAULT) + 0.5);
        }
    }

    #[test]
    fn overflow_state_clamps_scroll_and_counts_hidden_tabs() {
        let mut ws = Workspace::new();
        for i in 0..10 {
            ws.new_tab("zsh", None, i);
        }
        let mut m = measurer();
        let fitted = fit_width(&ws, &TabBarStyle::DEFAULT, 300.0, &mut m);
        assert!(fitted.content_width > 300.0);

        let none = overflow_state(&fitted, 300.0, 0.0);
        assert_eq!(none.hidden_left, 0);
        assert!(none.hidden_right > 0);

        let max_scroll = fitted.content_width - 300.0;
        let clamped = overflow_state(&fitted, 300.0, max_scroll + 500.0);
        assert_eq!(clamped.scroll_offset, max_scroll);
        assert_eq!(clamped.hidden_right, 0);
        assert!(clamped.hidden_left > 0);
    }

    #[test]
    fn drag_target_index_finds_insertion_point_by_ghost_center() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        ws.new_tab("bash", None, 1);
        ws.new_tab("fish", None, 2);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let last = layout.groups[0].tabs[2].rect;
        let ghost_center_past_everything = last.x + last.width + 1000.0;
        assert_eq!(
            drag_target_index(&layout, a, ghost_center_past_everything),
            2 // duas abas restantes (b, c) depois de tirar a
        );

        let first_after_removal_center =
            layout.groups[0].tabs[1].rect.x + layout.groups[0].tabs[1].rect.width / 2.0 - 1.0;
        assert_eq!(drag_target_index(&layout, a, first_after_removal_center), 0);
    }

    #[test]
    fn tab_rect_finds_by_id() {
        let mut ws = Workspace::new();
        let a = ws.new_tab("zsh", None, 0);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(tab_rect(&layout, a), Some(layout.groups[0].tabs[0].rect));
        assert_eq!(tab_rect(&layout, TabId::new(999)), None);
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
