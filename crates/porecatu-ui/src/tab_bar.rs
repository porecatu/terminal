// SPDX-License-Identifier: GPL-3.0-or-later

//! Layout e hit-testing da barra de abas -- função pura, sem `wgpu` e sem
//! janela (docs/arquitetura.md seção 7): `(Workspace, TabBarStyle,
//! TextMeasurer) -> TabBarLayout`. Cobre a geometria de trilha da espec.
//! visual §2.2, §2.3, §2.5, §2.6 e, desde a Etapa 5 da F2, o encolhimento de
//! rótulo e a rolagem do §2.18 (`fit_width`/`overflow_state`) e a geometria
//! de arraste do §2.19 (`drag_target`). Desde a F3 etapa 3, também a
//! geometria da pílula de grupo (§2.4) e a participação dela na ordem de
//! cedência do overflow. Desde a F3 etapa 4, grupo colapsado não gera
//! `TabRect` nenhum (§2.4: "suas abas somem da barra") e a pílula ganha o
//! indicador agregado (RF-2.16) e um alvo de hit-test próprio
//! (`TabBarHit::Pill`). Desde a F3 etapa 6, `drag_target` cruza fronteira
//! de grupo (ADR-0021 §4) e `group_drag_target_index`/`pill_rect` cobrem o
//! arraste do rótulo do grupo inteiro (espec. §2.19.1). Pintura
//! (`chrome.rs`) e wiring de clique/rename/arraste (`lib.rs`) ficam do
//! outro lado da fronteira -- este módulo não sabe de `wgpu` nem de
//! `winit`.
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

/// Espec. §2.4, `[appearance.groups] label_font_size = 12.0`, peso 500
/// (item 2 da pílula: "Nome 12px/500").
pub(crate) const PILL_NAME_FONT: FontFace = FontFace::Sans {
    weight: SansWeight::Medium,
};
/// Espec. §2.4, item 3: "contador mono 10px". Sem chave própria de fonte no
/// TOML -- só cor/fundo/raio (`count_*`) têm chave.
pub(crate) const PILL_COUNT_FONT: FontFace = FontFace::Mono { bold: false };
pub(crate) const PILL_COUNT_FONT_SIZE: f32 = 10.0;

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
    /// `[appearance.groups] label_padding_left` -- pílula (espec. §2.4).
    pub pill_padding_left: f32,
    /// `[appearance.groups] label_padding_right`
    pub pill_padding_right: f32,
    /// Espec. §2.4: "gap: 7" entre swatch/nome/contador/caret da pílula.
    /// Sem chave própria no TOML (mesmo padrão de `internal_gap`).
    pub pill_gap: f32,
    /// `[appearance.groups] swatch_size`
    pub pill_swatch_size: f32,
    /// `[appearance.groups] label_font_size` -- fonte do nome da pílula.
    pub pill_font_size: f32,
    /// `[appearance.groups] label_max_width` -- teto do nome (RF-2.12): o
    /// da aba (180) menos os 41px de cromo da §2.18, valor citado direto da
    /// espec, não recalculado (a nota do TOML já registra a derivação).
    pub pill_name_max_width: f32,
    /// `[appearance.groups] label_min_width` -- piso do nome, segundo
    /// degrau da ordem de cedência do §2.18 (depois do rótulo da aba,
    /// antes da rolagem da trilha).
    pub pill_name_min_width: f32,
    /// Espec. §2.4, item 3: "padding: 1px 6px" do contador. Sem chave
    /// própria (só cor/raio do contador têm chave no TOML).
    pub pill_count_padding_x: f32,
    pub pill_count_padding_y: f32,
    /// Espec. §2.4, item 4: "▶ 8px". Usado como largura reservada no flex
    /// da pílula e como tamanho de fonte do glyph -- o caret não tem caixa
    /// de acerto própria distinta do glyph (diferente do botão de fechar da
    /// aba). Sem chave própria no TOML.
    pub pill_caret_size: f32,
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
        pill_padding_left: 8.0,
        pill_padding_right: 9.0,
        pill_gap: 7.0,
        pill_swatch_size: 8.0,
        pill_font_size: 12.0,
        pill_name_max_width: 140.0,
        pill_name_min_width: 60.0,
        pill_count_padding_x: 6.0,
        pill_count_padding_y: 1.0,
        pill_caret_size: 8.0,
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
/// §2.3). `pill` é `None` para o grupo implícito -- "abas sem grupo usam um
/// wrapper sem pílula" (espec. §2.3) -- e `Some` para grupo explícito.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupWrapperRect {
    pub id: GroupId,
    pub rect: Rect,
    pub pill: Option<GroupPillRect>,
    pub tabs: Vec<TabRect>,
}

/// Geometria da pílula de grupo (espec. §2.4): swatch, nome (já truncado),
/// contador de abas e caret de colapso. A cor resolvida (swatch, tingimento
/// do wrapper) não mora aqui -- só geometria; `chrome.rs` resolve
/// `GroupColor` via `palette::group_color` no momento de pintar, no mesmo
/// padrão de `TabRect` (que também não carrega cor).
#[derive(Debug, Clone, PartialEq)]
pub struct GroupPillRect {
    pub rect: Rect,
    pub swatch: Rect,
    /// Indicador agregado (espec. §2.4, RF-2.16): só `Some` com o grupo
    /// **colapsado** e alguma aba dele com atividade/campainha pendente --
    /// "com o grupo expandido, cada aba mostra o seu próprio ponto e um
    /// agregado seria redundante". Mesma regra de "um ponto só, campainha
    /// vence" da §2.17.
    pub aggregate_indicator: Option<Indicator>,
    /// Origem do ponto agregado -- só significativa quando
    /// `aggregate_indicator` é `Some`, mesmo padrão de `name_origin`.
    pub aggregate_indicator_origin: (f32, f32),
    /// Origem do texto do nome (já truncado com reticências, RF-2.12) --
    /// sem retângulo próprio, mesmo padrão do rótulo da aba.
    pub name_origin: (f32, f32),
    pub name: String,
    /// Decide o tooltip do ADR-0019 (nome completo), mesmo padrão de
    /// `TabRect::label_truncated`.
    pub name_truncated: bool,
    pub count_rect: Rect,
    /// Contagem de abas do grupo, já formatada (espec. §2.4: "sempre
    /// desenhado, conteúdo idêntico" -- não some quando expandido).
    pub count_text: String,
    pub caret_rect: Rect,
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
    /// Espec. §2.4: "clique alterna colapso; duplo clique abre o editor" --
    /// o alvo é a pílula inteira, sem sub-região própria pro caret
    /// (RF-2.13, `docs/reference/acoes.md`: "o alvo é a pílula clicada").
    Pill(GroupId),
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
        let collapsed = group.is_collapsed();

        // Pílula (espec. §2.3/§2.4): só grupo explícito -- "abas sem grupo
        // usam um wrapper sem pílula". Fica antes das abas no flex do
        // wrapper, com o mesmo `gap` que separa as abas entre si (§2.3:
        // "gap: 4" é o único gap do wrapper, aplicado a todo filho direto).
        let pill = if group.is_explicit() {
            let pill_y = style.wrapper_padding;
            let pill_height = style.tab_height;
            let mut px = inner_x + style.pill_padding_left;

            let swatch = Rect {
                x: px,
                y: pill_y + (pill_height - style.pill_swatch_size) / 2.0,
                width: style.pill_swatch_size,
                height: style.pill_swatch_size,
            };
            px += style.pill_swatch_size + style.pill_gap;

            // Indicador agregado (espec. §2.4, RF-2.16): só colapsado, e só
            // se houver atividade/campainha pendente entre as abas do
            // grupo -- que continuam existindo em `group.tabs()` mesmo sem
            // `TabRect` (elas "somem da barra", não do modelo). Consome
            // largura do **nome**, não da pílula -- por isso o `dot_reserve`
            // é subtraído do teto de nome abaixo, no mesmo padrão do
            // indicador de aba (`dot_reserve` no laço de abas).
            let aggregate_indicator = if collapsed {
                aggregate_indicator(workspace, group.tabs())
            } else {
                None
            };
            let aggregate_indicator_origin = if aggregate_indicator.is_some() {
                let origin = (px, pill_y + (pill_height - INDICATOR_DOT_SIZE) / 2.0);
                px += INDICATOR_DOT_SIZE + style.pill_gap;
                origin
            } else {
                (0.0, 0.0)
            };
            let name_dot_reserve = if aggregate_indicator.is_some() {
                INDICATOR_DOT_SIZE + style.pill_gap
            } else {
                0.0
            };
            let name_cap = (style.pill_name_max_width - name_dot_reserve).max(0.0);

            let (name, name_truncated) = measurer.truncate(
                group.name().unwrap_or_default(),
                PILL_NAME_FONT,
                style.pill_font_size,
                name_cap,
            );
            let name_width = measurer.measure_width(&name, PILL_NAME_FONT, style.pill_font_size);
            let name_origin = (px, pill_y + (pill_height - style.pill_font_size) / 2.0);
            px += name_width + style.pill_gap;

            // Espec. §2.4: "sempre desenhado, conteúdo idêntico" -- a
            // contagem não muda com colapso, `show_tab_count_when_collapsed`
            // só existiria a partir de `porecatu-config` (F4).
            let count_text = group.tabs().len().to_string();
            let count_text_width =
                measurer.measure_width(&count_text, PILL_COUNT_FONT, PILL_COUNT_FONT_SIZE);
            let count_width = count_text_width + style.pill_count_padding_x * 2.0;
            let count_height = PILL_COUNT_FONT_SIZE + style.pill_count_padding_y * 2.0;
            let count_rect = Rect {
                x: px,
                y: pill_y + (pill_height - count_height) / 2.0,
                width: count_width,
                height: count_height,
            };
            px += count_width + style.pill_gap;

            let caret_rect = Rect {
                x: px,
                y: pill_y + (pill_height - style.pill_caret_size) / 2.0,
                width: style.pill_caret_size,
                height: style.pill_caret_size,
            };
            px += style.pill_caret_size + style.pill_padding_right;

            let pill_rect = Rect {
                x: inner_x,
                y: pill_y,
                width: px - inner_x,
                height: pill_height,
            };
            inner_x = px;
            Some(GroupPillRect {
                rect: pill_rect,
                swatch,
                aggregate_indicator,
                aggregate_indicator_origin,
                name_origin,
                name,
                name_truncated,
                count_rect,
                count_text,
                caret_rect,
            })
        } else {
            None
        };
        // Espec. §2.4: "grupo colapsado... suas abas somem da barra" -- o
        // `tab_gap` que normalmente separa a pílula da primeira aba não
        // entra sem nenhuma aba pra separar (RF-2.13).
        if pill.is_some() && !collapsed {
            inner_x += style.tab_gap;
        }

        let mut tabs = Vec::with_capacity(if collapsed { 0 } else { group.tabs().len() });

        for (index, &tab_id) in group.tabs().iter().enumerate() {
            if collapsed {
                continue;
            }
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
            pill,
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

/// Indicador agregado (espec. §2.4, RF-2.16) sobre as abas de um grupo
/// colapsado: campainha vence atividade (mesma regra da §2.17), aba
/// `Exited` nunca contribui (mesma exclusão da §2.17 -- o motivo de ela
/// ficar aberta já está escrito no grid, não num indicador). `None` quando
/// nenhuma aba tem nada pendente.
fn aggregate_indicator(workspace: &Workspace, tabs: &[TabId]) -> Option<Indicator> {
    let mut any_activity = false;
    for &id in tabs {
        let Some(tab) = workspace.tab(id) else {
            continue;
        };
        if tab.is_exited() {
            continue;
        }
        if tab.bell() {
            return Some(Indicator::Bell);
        }
        if tab.activity() {
            any_activity = true;
        }
    }
    any_activity.then_some(Indicator::Activity)
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

/// Busca binária: o maior valor de um teto em `[floor, ceiling]` cujo
/// `layout` resultante ainda cabe em `available_width`. Extraída de
/// [`fit_width`] porque a ordem de cedência do §2.18 aplica o mesmo
/// algoritmo duas vezes -- primeiro no rótulo da aba, depois no nome da
/// pílula -- sobre parâmetros diferentes de [`TabBarStyle`]. `apply` recebe
/// o teto candidato e devolve o `TabBarStyle` com ele aplicado; `content_width`
/// de [`layout`] é não decrescente nesse parâmetro em ambos os casos
/// (nenhum elemento fica mais estreito ao aumentar o próprio teto), então a
/// busca converge para o maior teto que ainda cabe.
fn shrink_to_fit(
    workspace: &Workspace,
    available_width: f32,
    measurer: &mut TextMeasurer,
    floor: f32,
    ceiling: f32,
    apply: impl Fn(f32) -> TabBarStyle,
) -> TabBarLayout {
    let mut lo = floor;
    let mut hi = ceiling;
    let mut best = layout(workspace, &apply(floor), measurer);
    for _ in 0..12 {
        let mid = (lo + hi) / 2.0;
        let mid_layout = layout(workspace, &apply(mid), measurer);
        if mid_layout.content_width <= available_width {
            lo = mid;
            best = mid_layout;
        } else {
            hi = mid;
        }
    }
    best
}

/// Piso do nome da pílula (espec. §2.4, `[appearance.groups]
/// label_min_width`): ao contrário de [`label_floor`], já é o valor final --
/// a espec dá o piso do nome direto, não um piso de pílula inteira a
/// decompor.
fn pill_name_floor(style: &TabBarStyle) -> f32 {
    style.pill_name_min_width.max(0.0)
}

/// Encolhe a trilha para caber em `available_width`, na ordem de cedência
/// do §2.18: primeiro o rótulo da aba (teto 180, piso [`label_floor`]),
/// depois -- só se o piso do rótulo ainda não bastar -- o nome da pílula de
/// grupo (teto `pill_name_max_width`, piso [`pill_name_floor`]). Abaixo dos
/// dois pisos a trilha continua largando o mesmo `content_width` de antes --
/// é o sinal para [`overflow_state`] ativar a rolagem, não responsabilidade
/// desta função (que permanece pura e sem estado de scroll).
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

    let label_floor = label_floor(style);
    let labels_at_floor_style = TabBarStyle {
        label_max_width: label_floor,
        ..*style
    };
    let labels_at_floor = layout(workspace, &labels_at_floor_style, measurer);
    if labels_at_floor.content_width <= available_width {
        // Estágio 1 sozinho resolve: o rótulo cede em algum ponto entre o
        // teto (180) e o piso, nome da pílula fica no teto o tempo todo.
        return shrink_to_fit(
            workspace,
            available_width,
            measurer,
            label_floor,
            style.label_max_width,
            |label_max_width| TabBarStyle {
                label_max_width,
                ..*style
            },
        );
    }

    // Rótulo no piso ainda não basta -- pinado lá, o nome da pílula cede em
    // seguida (espec §2.18: segundo degrau da ordem de cedência).
    let pill_floor = pill_name_floor(style);
    let both_at_floor_style = TabBarStyle {
        label_max_width: label_floor,
        pill_name_max_width: pill_floor,
        ..*style
    };
    let both_at_floor = layout(workspace, &both_at_floor_style, measurer);
    if both_at_floor.content_width > available_width {
        // Nem nos dois pisos cabe -- devolve o layout no fundo da ordem de
        // cedência; rolar é com `overflow_state`.
        return both_at_floor;
    }

    shrink_to_fit(
        workspace,
        available_width,
        measurer,
        pill_floor,
        style.pill_name_max_width,
        |pill_name_max_width| TabBarStyle {
            label_max_width: label_floor,
            pill_name_max_width,
            ..*style
        },
    )
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

/// Acha o retângulo da pílula de um grupo pelo `id` -- mesmo papel de
/// [`tab_rect`], usado pelo arraste do rótulo do grupo (espec. §2.19.1,
/// F3 etapa 6). `None` para grupo implícito (sem pílula).
pub fn pill_rect(layout: &TabBarLayout, id: GroupId) -> Option<Rect> {
    layout
        .groups
        .iter()
        .find(|g| g.id == id)
        .and_then(|g| g.pill.as_ref())
        .map(|p| p.rect)
}

/// Alvo de um arraste de aba (ADR-0021 §4, F3 etapa 6): dentro de um grupo
/// já existente, numa posição entre as abas restantes dele -- mesma
/// convenção de índice que [`Group::move_within`]/`Workspace::move_tab`
/// esperam --, ou fora de qualquer wrapper, criando um run implícito novo
/// na posição dada da lista de grupos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragDrop {
    IntoGroup { group: GroupId, pos: usize },
    NewRun { group_index: usize },
}

/// Resolve onde `ghost_center_x` cairia (ADR-0021 §4): dentro do wrapper
/// que ele sobrepõe -- sobre a pílula ou no meio das abas, comparando
/// contra o centro de cada uma, igual ao arraste dentro do próprio grupo
/// já fazia --, ou no `gap` entre dois wrappers, que **pertence ao grupo
/// da esquerda** (soltar ali entra no fim daquele grupo -- os 6px do gap
/// são estreitos demais pra mirar como zona própria). Só fora de **todos**
/// os wrappers -- antes do primeiro ou depois do último -- é que cria um
/// run implícito novo. Espec. §2.19: "o buraco é o marcador" -- isto só
/// resolve o alvo; mover de verdade é decisão de `lib.rs` ao soltar.
pub fn drag_target(layout: &TabBarLayout, dragged: TabId, ghost_center_x: f32) -> DragDrop {
    for (index, group) in layout.groups.iter().enumerate() {
        let start = group.rect.x;
        let end = group.rect.x + group.rect.width;
        if ghost_center_x < start {
            return if index == 0 {
                DragDrop::NewRun { group_index: 0 }
            } else {
                into_group_end(&layout.groups[index - 1], dragged)
            };
        }
        if ghost_center_x < end {
            return resolve_within_group(group, dragged, ghost_center_x);
        }
    }
    DragDrop::NewRun {
        group_index: layout.groups.len(),
    }
}

fn into_group_end(group: &GroupWrapperRect, dragged: TabId) -> DragDrop {
    let count = group.tabs.iter().filter(|t| t.id != dragged).count();
    DragDrop::IntoGroup {
        group: group.id,
        pos: count,
    }
}

fn resolve_within_group(group: &GroupWrapperRect, dragged: TabId, ghost_center_x: f32) -> DragDrop {
    // ADR-0021 §4: "soltar sobre a pílula entra no início do grupo" --
    // também cobre grupo colapsado, cujo wrapper é só a pílula (sem abas
    // na trilha), então qualquer ponto dentro dele já cai aqui.
    if let Some(pill) = &group.pill
        && ghost_center_x < pill.rect.x + pill.rect.width
    {
        return DragDrop::IntoGroup {
            group: group.id,
            pos: 0,
        };
    }
    let others: Vec<&TabRect> = group.tabs.iter().filter(|t| t.id != dragged).collect();
    for (index, tab) in others.iter().enumerate() {
        let center = tab.rect.x + tab.rect.width / 2.0;
        if ghost_center_x < center {
            return DragDrop::IntoGroup {
                group: group.id,
                pos: index,
            };
        }
    }
    DragDrop::IntoGroup {
        group: group.id,
        pos: others.len(),
    }
}

/// Retângulo a realçar durante o arraste de aba (espec. §2.19: "o wrapper
/// que receberia a aba" -- ou só a pílula, se o grupo estiver colapsado,
/// já que não há trilha pra realçar ali). `None` quando o alvo é um run
/// implícito novo fora de qualquer wrapper existente (`DragDrop::NewRun`
/// nas pontas da trilha) -- não há retângulo pra desenhar.
pub fn drag_highlight_rect(layout: &TabBarLayout, target: DragDrop) -> Option<(GroupId, Rect)> {
    let DragDrop::IntoGroup { group, .. } = target else {
        return None;
    };
    let wrapper = layout.groups.iter().find(|g| g.id == group)?;
    let rect = match &wrapper.pill {
        Some(pill) if wrapper.tabs.is_empty() => pill.rect,
        _ => wrapper.rect,
    };
    Some((group, rect))
}

/// Índice de inserção (mesma convenção de [`drag_target`]/`Workspace::
/// move_group`: posição entre os grupos **restantes**, já sem o
/// arrastado) para onde o fantasma da pílula (espec. §2.19.1) cairia,
/// comparando `ghost_center_x` contra o centro de cada wrapper que não é
/// o arrastado. Grupos não aninham (ADR-0006): o alvo é sempre uma
/// fronteira entre grupos, nunca o interior de outro -- por isso não há
/// equivalente de "soltar sobre a pílula" aqui.
pub fn group_drag_target_index(
    layout: &TabBarLayout,
    dragged: GroupId,
    ghost_center_x: f32,
) -> usize {
    let others: Vec<&GroupWrapperRect> = layout.groups.iter().filter(|g| g.id != dragged).collect();
    for (index, group) in others.iter().enumerate() {
        let center = group.rect.x + group.rect.width / 2.0;
        if ghost_center_x < center {
            return index;
        }
    }
    others.len()
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
    for group in &layout.groups {
        if let Some(pill) = &group.pill
            && rect_contains(pill.rect, point)
        {
            return Some(TabBarHit::Pill(group.id));
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
    use porecatu_core::GroupColor;

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
        ws.append_tab("zsh", None);
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
        let id = ws.append_tab("zsh", None);
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
        ws.append_tab("zsh", None);
        ws.append_tab("bash", None);
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
        let id = ws.append_tab("zsh", None);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let close = layout.groups[0].tabs[0].close_button;
        let center = (close.x + close.width / 2.0, close.y + close.height / 2.0);
        assert_eq!(hit_test(&layout, center), Some(TabBarHit::CloseButton(id)));
    }

    #[test]
    fn hit_test_close_button_slop_extends_hit_area() {
        let mut ws = Workspace::new();
        let id = ws.append_tab("zsh", None);
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
        let id = ws.append_tab("zsh", None);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let tab = &layout.groups[0].tabs[0];
        let point = (tab.rect.x + 2.0, tab.rect.y + 2.0);
        assert_eq!(hit_test(&layout, point), Some(TabBarHit::Tab(id)));
    }

    #[test]
    fn hit_test_gap_boundary_splits_at_midpoint() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
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
        let id = ws.append_tab("zsh", None);
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
        let id = ws.append_tab("zsh", None);
        ws.tab_mut(id).unwrap().mark_activity();
        ws.tab_mut(id).unwrap().mark_bell();
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(layout.groups[0].tabs[0].indicator, Some(Indicator::Bell));
    }

    #[test]
    fn exited_tab_never_shows_indicator() {
        let mut ws = Workspace::new();
        let id = ws.append_tab("zsh", None);
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
        let plain = ws.append_tab("zsh", None);
        ws.tab_mut(plain)
            .unwrap()
            .set_custom_title(Some(long_title.to_string()));
        let with_indicator = ws.append_tab("zsh", None);
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
        ws.append_tab("zsh", None);
        let mut m = measurer();
        let unfit = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let fit = fit_width(&ws, &TabBarStyle::DEFAULT, 2000.0, &mut m);
        assert_eq!(unfit, fit);
    }

    #[test]
    fn fit_width_shrinks_labels_to_fit_before_scrolling() {
        let mut ws = Workspace::new();
        for _ in 0..3 {
            let id = ws.append_tab("zsh", None);
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
        for _ in 0..50 {
            ws.append_tab("zsh", None);
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
        for _ in 0..10 {
            ws.append_tab("zsh", None);
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
    fn drag_target_within_own_group_finds_insertion_point_by_ghost_center() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_of_tab(a).unwrap();
        ws.append_tab("bash", None);
        ws.append_tab("fish", None);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        // Dentro do wrapper, depois da última aba -- diferente de "depois
        // de tudo", que agora é `NewRun` (ADR-0021 §4: só o gap **entre**
        // wrappers, ou fora de qualquer um, tem essa regra especial; a
        // ponta livre do único wrapper existente ainda é o próprio grupo).
        let wrapper = &layout.groups[0];
        let ghost_center_at_wrapper_end = wrapper.rect.x + wrapper.rect.width - 1.0;
        assert_eq!(
            drag_target(&layout, a, ghost_center_at_wrapper_end),
            // duas abas restantes (b, c) depois de tirar a
            DragDrop::IntoGroup { group, pos: 2 }
        );

        let first_after_removal_center =
            layout.groups[0].tabs[1].rect.x + layout.groups[0].tabs[1].rect.width / 2.0 - 1.0;
        assert_eq!(
            drag_target(&layout, a, first_after_removal_center),
            DragDrop::IntoGroup { group, pos: 0 }
        );
    }

    #[test]
    fn drag_target_over_pill_of_other_group_enters_at_its_start() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        let dest = ws.group_tabs(&[b], "dest", GroupColor::Blue).unwrap();
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let pill = layout.groups[1].pill.as_ref().unwrap();
        let over_pill = pill.rect.x + pill.rect.width / 2.0;
        assert_eq!(
            drag_target(&layout, a, over_pill),
            DragDrop::IntoGroup {
                group: dest,
                pos: 0
            }
        );
    }

    #[test]
    fn drag_target_in_gap_between_wrappers_enters_end_of_left_group() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        let dest = ws.group_tabs(&[b], "dest", GroupColor::Blue).unwrap();
        // "a" fica sozinha num run implícito novo depois do split acima --
        // o id dela mudou, então pega de novo em vez de reusar o de antes.
        let group_a = ws.group_of_tab(a).unwrap();
        // arrasta uma terceira aba, de fora dos dois grupos envolvidos.
        let c = ws.new_tab(None, "fish", None, 0);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        // meio do `trilha_gap` entre os dois primeiros wrappers.
        let gap_center = layout.groups[0].rect.x
            + layout.groups[0].rect.width
            + TabBarStyle::DEFAULT.trilha_gap / 2.0;
        assert_eq!(
            drag_target(&layout, c, gap_center),
            DragDrop::IntoGroup {
                group: group_a,
                pos: 1
            }
        );
        assert!(dest != group_a);
    }

    #[test]
    fn drag_target_before_first_group_creates_new_run() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(
            drag_target(&layout, a, -100.0),
            DragDrop::NewRun { group_index: 0 }
        );
    }

    #[test]
    fn drag_target_after_last_group_creates_new_run() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let far_right = layout.content_width + 1000.0;
        assert_eq!(
            drag_target(&layout, a, far_right),
            DragDrop::NewRun { group_index: 1 }
        );
    }

    #[test]
    fn drag_target_over_collapsed_group_enters_it() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        let dest = ws.group_tabs(&[b], "dest", GroupColor::Blue).unwrap();
        ws.collapse_group(dest, true);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let wrapper = &layout.groups[1];
        assert!(wrapper.tabs.is_empty());
        let over = wrapper.rect.x + wrapper.rect.width / 2.0;
        assert_eq!(
            drag_target(&layout, a, over),
            DragDrop::IntoGroup {
                group: dest,
                pos: 0
            }
        );
    }

    #[test]
    fn drag_highlight_rect_is_none_for_new_run() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(
            drag_highlight_rect(&layout, DragDrop::NewRun { group_index: 0 }),
            None
        );
        let _ = a;
    }

    #[test]
    fn drag_highlight_rect_uses_pill_when_collapsed() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a], "g", GroupColor::Blue).unwrap();
        ws.collapse_group(group, true);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let pill_rect = layout.groups[0].pill.as_ref().unwrap().rect;
        let (highlighted_group, rect) =
            drag_highlight_rect(&layout, DragDrop::IntoGroup { group, pos: 0 }).unwrap();
        assert_eq!(highlighted_group, group);
        assert_eq!(rect, pill_rect);
    }

    #[test]
    fn group_drag_target_index_finds_insertion_point_by_ghost_center() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let g1 = ws.group_tabs(&[a], "g1", GroupColor::Red).unwrap();
        let b = ws.new_tab(None, "bash", None, 0);
        let g2 = ws.group_tabs(&[b], "g2", GroupColor::Blue).unwrap();
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let g2_rect = layout.groups[1].rect;
        let over_g2_center = g2_rect.x + g2_rect.width / 2.0;
        // arrastando g1 pra cima de g2: entra depois dele (índice 1, já
        // sem g1 na lista de "outros").
        assert_eq!(group_drag_target_index(&layout, g1, over_g2_center), 1);
        let far_left = -100.0;
        assert_eq!(group_drag_target_index(&layout, g2, far_left), 0);
    }

    #[test]
    fn tab_rect_finds_by_id() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(tab_rect(&layout, a), Some(layout.groups[0].tabs[0].rect));
        assert_eq!(tab_rect(&layout, TabId::new(999)), None);
    }

    #[test]
    fn hit_test_outside_everything_is_none() {
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(hit_test(&layout, (-100.0, -100.0)), None);
        assert_eq!(hit_test(&layout, (100_000.0, 100_000.0)), None);
    }

    #[test]
    fn implicit_group_wrapper_has_no_pill() {
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert!(layout.groups[0].pill.is_none());
    }

    #[test]
    fn explicit_group_wrapper_has_pill_before_first_tab() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        ws.group_tabs(&[a, b], "trabalho", GroupColor::Blue);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let wrapper = &layout.groups[0];
        let pill = wrapper.pill.as_ref().expect("grupo explícito tem pílula");
        assert_eq!(pill.name, "trabalho");
        assert!(!pill.name_truncated);
        assert_eq!(pill.count_text, "2");
        assert_eq!(
            pill.rect.x,
            wrapper.rect.x + TabBarStyle::DEFAULT.wrapper_padding
        );
        // primeira aba começa depois da pílula + o mesmo gap das abas entre
        // si (espec. §2.3: "gap: 4" é o único gap do wrapper).
        let first_tab = &wrapper.tabs[0];
        assert_eq!(
            first_tab.rect.x,
            pill.rect.x + pill.rect.width + TabBarStyle::DEFAULT.tab_gap
        );
    }

    #[test]
    fn pill_elements_are_ordered_left_to_right_within_bounds() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.group_tabs(&[a], "x", GroupColor::Red);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let pill = layout.groups[0].pill.as_ref().unwrap();
        assert!(pill.swatch.x < pill.name_origin.0);
        assert!(pill.name_origin.0 < pill.count_rect.x);
        assert!(pill.count_rect.x < pill.caret_rect.x);
        assert!(pill.caret_rect.x + pill.caret_rect.width <= pill.rect.x + pill.rect.width);
    }

    #[test]
    fn long_group_name_is_truncated_with_ellipsis() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.group_tabs(
            &[a],
            "um nome de grupo bem comprido que estoura o teto de 140px da pilula",
            GroupColor::Green,
        );
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let pill = layout.groups[0].pill.as_ref().unwrap();
        assert!(pill.name_truncated);
        assert!(pill.name.ends_with('…'));
        let width = m.measure_width(
            &pill.name,
            PILL_NAME_FONT,
            TabBarStyle::DEFAULT.pill_font_size,
        );
        assert!(width <= TabBarStyle::DEFAULT.pill_name_max_width);
    }

    #[test]
    fn pill_group_and_implicit_group_coexist_with_trilha_gap_between() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.group_tabs(&[a], "g1", GroupColor::Red);
        ws.new_tab(None, "bash", None, 0); // força um segundo run implícito

        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(layout.groups.len(), 2);
        assert!(layout.groups[0].pill.is_some());
        assert!(layout.groups[1].pill.is_none());
        let g0_end = layout.groups[0].rect.x + layout.groups[0].rect.width;
        assert_eq!(
            layout.groups[1].rect.x,
            g0_end + TabBarStyle::DEFAULT.trilha_gap
        );
    }

    #[test]
    fn fit_width_shrinks_pill_name_after_label_floor_is_reached() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.group_tabs(
            &[a],
            "um nome de grupo razoavelmente longo",
            GroupColor::Purple,
        );
        let mut m = measurer();

        let full = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        // Piso duplo (rótulo da aba E nome da pílula no piso): descobre
        // pedindo um `available_width` que nem ele cabe, mesma técnica de
        // `fit_width_shrinks_labels_to_fit_before_scrolling`.
        let both_floors_total = fit_width(&ws, &TabBarStyle::DEFAULT, 1.0, &mut m).content_width;
        let available = both_floors_total + 20.0;
        assert!(
            full.content_width > available,
            "nome do grupo precisa estourar `available` pra este teste fazer sentido"
        );

        let fitted = fit_width(&ws, &TabBarStyle::DEFAULT, available, &mut m);
        assert!(fitted.content_width <= available + 1.0);
        let pill = fitted.groups[0].pill.as_ref().unwrap();
        let name_width = m.measure_width(
            &pill.name,
            PILL_NAME_FONT,
            TabBarStyle::DEFAULT.pill_font_size,
        );
        assert!(name_width < TabBarStyle::DEFAULT.pill_name_max_width);
    }

    #[test]
    fn collapsed_group_produces_no_tab_rects() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        let group = ws.group_tabs(&[a, b], "col", GroupColor::Cyan).unwrap();
        ws.collapse_group(group, true);

        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert!(layout.groups[0].tabs.is_empty());
        assert!(layout.groups[0].pill.is_some());
    }

    #[test]
    fn collapsed_group_wrapper_hugs_the_pill_without_trailing_tab_gap() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a], "col", GroupColor::Cyan).unwrap();
        ws.collapse_group(group, true);

        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let wrapper = &layout.groups[0];
        let pill = wrapper.pill.as_ref().unwrap();
        assert_eq!(
            wrapper.rect.width,
            pill.rect.width + TabBarStyle::DEFAULT.wrapper_padding * 2.0
        );
    }

    #[test]
    fn expanded_group_has_no_aggregate_indicator() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.tab_mut(a).unwrap().mark_activity();
        ws.group_tabs(&[a], "g", GroupColor::Cyan).unwrap();

        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(
            layout.groups[0].pill.as_ref().unwrap().aggregate_indicator,
            None
        );
    }

    #[test]
    fn collapsed_group_shows_aggregate_activity_indicator() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        ws.tab_mut(b).unwrap().mark_activity();
        let group = ws.group_tabs(&[a, b], "g", GroupColor::Cyan).unwrap();
        ws.collapse_group(group, true);

        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(
            layout.groups[0].pill.as_ref().unwrap().aggregate_indicator,
            Some(Indicator::Activity)
        );
    }

    #[test]
    fn collapsed_group_aggregate_bell_wins_over_activity() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let b = ws.append_tab("bash", None);
        ws.tab_mut(a).unwrap().mark_activity();
        ws.tab_mut(b).unwrap().mark_bell();
        let group = ws.group_tabs(&[a, b], "g", GroupColor::Cyan).unwrap();
        ws.collapse_group(group, true);

        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(
            layout.groups[0].pill.as_ref().unwrap().aggregate_indicator,
            Some(Indicator::Bell)
        );
    }

    #[test]
    fn collapsed_group_exited_tab_does_not_trigger_aggregate_indicator() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.tab_mut(a).unwrap().mark_activity();
        ws.tab_mut(a).unwrap().mark_exited(0);
        let group = ws.group_tabs(&[a], "g", GroupColor::Cyan).unwrap();
        ws.collapse_group(group, true);

        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert_eq!(
            layout.groups[0].pill.as_ref().unwrap().aggregate_indicator,
            None
        );
    }

    #[test]
    fn hit_test_pill_returns_group_id() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a], "g", GroupColor::Cyan).unwrap();
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let pill = layout.groups[0].pill.as_ref().unwrap();
        let center = (
            pill.rect.x + pill.rect.width / 2.0,
            pill.rect.y + pill.rect.height / 2.0,
        );
        assert_eq!(hit_test(&layout, center), Some(TabBarHit::Pill(group)));
    }

    #[test]
    fn fit_width_stays_at_pill_name_floor_when_it_still_does_not_fit() {
        let mut ws = Workspace::new();
        for i in 0..8 {
            let a = ws.append_tab("zsh", None);
            ws.group_tabs(
                &[a],
                format!("grupo numero {i} com nome bem longo"),
                GroupColor::Yellow,
            );
        }
        let mut m = measurer();
        let fitted = fit_width(&ws, &TabBarStyle::DEFAULT, 300.0, &mut m);
        assert!(fitted.content_width > 300.0);
        for group in &fitted.groups {
            let pill = group.pill.as_ref().unwrap();
            let width = m.measure_width(
                &pill.name,
                PILL_NAME_FONT,
                TabBarStyle::DEFAULT.pill_font_size,
            );
            assert!(width <= TabBarStyle::DEFAULT.pill_name_min_width + 0.5);
        }
    }
}
