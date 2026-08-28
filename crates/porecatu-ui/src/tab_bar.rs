// SPDX-License-Identifier: GPL-3.0-or-later

//! Layout e hit-testing da barra de abas -- função pura, sem `wgpu` e sem
//! janela (docs/arquitetura.md seção 7): `(Workspace, TabBarStyle,
//! TextMeasurer) -> TabBarLayout`. Cobre a geometria de trilha da espec.
//! visual §2.2, §2.3, §2.5, §2.6, a rolagem do §2.18 (`overflow_state`) e a
//! geometria de arraste do §2.19 (`drag_target`). Desde a F3 etapa 3,
//! também a geometria da pílula de grupo (§2.4). Desde a F3 etapa 4, grupo
//! colapsado não gera `TabRect` nenhum (§2.4: "suas abas somem da barra")
//! e a pílula ganha o indicador agregado (RF-2.16) e um alvo de hit-test
//! próprio (`TabBarHit::Pill`). Desde a F3 etapa 6, `drag_target` cruza
//! fronteira de grupo (ADR-0021 §4) e `group_drag_target_index`/
//! `pill_rect` cobrem o arraste do rótulo do grupo inteiro (espec.
//! §2.19.1). Pintura (`chrome.rs`) e wiring de clique/rename/arraste
//! (`lib.rs`) ficam do outro lado da fronteira -- este módulo não sabe de
//! `wgpu` nem de `winit`.
//!
//! **`fit_width` não encolhe mais rótulo nem nome de pílula** (mudança de
//! performance pós-F3, fora da espec. §2.18 -- ver o comentário da função):
//! ela é hoje um sinônimo de [`layout`]. Rótulo e nome de pílula ficam
//! sempre no teto; a trilha rola inteira (`overflow_state`) quando não
//! cabe.
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

/// **A mesma do rótulo da aba**, por pedido do usuário: a espec. §2.4 dá
/// ao nome do grupo peso 500 contra o 400 da aba (§1.1), e a diferença de
/// peso lado a lado na barra lê como duas fontes diferentes, não como
/// hierarquia. O que separa o grupo da aba já são a cápsula de cor, o
/// swatch e o contador. O tamanho acompanha pelo mesmo motivo -- ver
/// `TabBarStyle::pill_font_size`.
pub(crate) const PILL_NAME_FONT: FontFace = LABEL_FONT;
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
    /// Respiro horizontal **dentro** de cada botão de ícone -- fechar da
    /// aba, "+" (do grupo e o de aba solta), caret da pílula e o botão da
    /// zona fixa à direita. Some de cada lado, deixando o botão mais
    /// largo que alto; a altura não muda.
    ///
    /// Sem origem na espec., que desenha esses botões quadrados: pedido
    /// do usuário. Valor de trabalho, no mesmo espírito de
    /// `RENAME_FIELD_HEIGHT` -- ajustar se ficar visualmente errado.
    pub icon_button_padding_x: f32,
    /// `[appearance.groups] wrapper_padding`
    pub wrapper_padding: f32,
    /// `[appearance.groups] gap` -- entre grupos, e também o gap da
    /// trilha antes do botão de nova aba (espec. §2.2: "Trilha ...
    /// gap: 6" é o mesmo valor, aplicado aos mesmos filhos diretos).
    pub trilha_gap: f32,
    /// Respiro entre o conteúdo da trilha e as bordas da barra, nos
    /// quatro lados. A espec. §2.2 não dá padding à trilha: o primeiro
    /// wrapper encosta na esquerda e todos encostam no topo e na base da
    /// barra. Pedido do usuário ("deixe-os um pouco afastados das
    /// bordas"). Entra na altura da barra ([`bar_height`]) e desloca todo
    /// o conteúdo do layout -- inclusive o hit-testing, que trabalha nas
    /// mesmas coordenadas.
    ///
    /// O valor é o **da própria espec. §2.5**, que descreve a barra como
    /// "aba h30 + padding 6px da barra": os 6px que ela dá à barra e a
    /// implementação nunca teve. Uma tentativa com 4px pareceu não ter
    /// respiro nenhum embaixo, mas o valor não era a causa -- o recorte da
    /// trilha em `chrome::paint` vinha de uma cópia velha da altura da
    /// barra e cortava as abas antes do respiro. Uma tentativa de reduzir
    /// só o horizontal em 2px (campo à parte do vertical) não ficou bem em
    /// tela -- revertida, os dois eixos voltam a compartilhar este valor.
    pub trilha_padding: f32,
    /// Lado do botão da zona fixa à direita. Herda o "30×30" que a espec.
    /// §2.6 dava ao botão de nova aba global, que ocupava esta zona antes
    /// -- hoje quem mora aqui é o botão de configurações. Sem chave
    /// própria no TOML. O botão é centrado na barra, não colado à altura
    /// da aba.
    pub right_zone_button_size: f32,
    /// `[appearance.tabs] show_new_tab_button` -- governa o "+" de cada
    /// grupo, que desde a remoção do botão global é o único botão de nova
    /// aba da barra.
    pub show_new_tab_button: bool,
    /// `[appearance.tabs] font_size`
    pub font_size: f32,
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
    /// Igual a `font_size` (o rótulo da aba) por pedido do usuário; a
    /// espec. §2.4 pede 12.0 contra os 12.5 da aba, e meio pixel de
    /// diferença lado a lado só faz o nome parecer de outra fonte. Ver
    /// [`PILL_NAME_FONT`].
    pub pill_font_size: f32,
    /// `[appearance.groups] label_max_width` -- teto do nome (RF-2.12): o
    /// da aba (180) menos os 41px de cromo da §2.18, valor citado direto da
    /// espec, não recalculado (a nota do TOML já registra a derivação).
    pub pill_name_max_width: f32,
    /// Largura reservada ao caret no flex da pílula. **Não** é o tamanho
    /// de fonte com que ele é desenhado: a face de ícones avança 1 em e
    /// desenha menos que isso, então quem pinta usa uma em maior (ver
    /// `chrome::PILL_CARET_ICON_SIZE`) e este valor cobre só o desenho.
    /// Vem do caret mais largo dos dois, para a pílula não mudar de
    /// largura ao colapsar. Espec. §2.4, item 4: "▶ 8px" -- o desenho de
    /// hoje é maior, junto com o resto dos ícones. Sem chave no TOML.
    pub pill_caret_size: f32,
}

impl TabBarStyle {
    pub const DEFAULT: Self = Self {
        tab_height: 34.0,
        max_width: 260.0,
        label_max_width: 180.0,
        padding_left: 10.0,
        padding_right: 6.0,
        tab_gap: 4.0,
        internal_gap: 8.0,
        close_button_size: 17.0,
        close_button_hit_slop: 2.0,
        icon_button_padding_x: 4.0,
        wrapper_padding: 3.0,
        trilha_gap: 6.0,
        trilha_padding: 6.0,
        right_zone_button_size: 30.0,
        show_new_tab_button: true,
        font_size: 12.5,
        pill_padding_left: 8.0,
        pill_padding_right: 9.0,
        pill_gap: 7.0,
        pill_swatch_size: 8.0,
        pill_font_size: 12.5, // = `font_size`
        pill_name_max_width: 140.0,
        // `icon::WIDEST_CARET_INK_EM * chrome::PILL_CARET_ICON_SIZE`,
        // fixado aqui porque `TabBarStyle::DEFAULT` é `const` e o
        // tamanho vive do outro lado, em `chrome.rs`; o teste
        // `pill_caret_slot_fits_the_widest_caret` amarra os dois.
        pill_caret_size: 11.8,
    };
}

impl TabBarStyle {
    /// Largura de um botão de ícone de lado `size`: o desenho continua
    /// centrado num quadrado desse lado, e o respiro entra só na largura.
    pub fn icon_button_width(&self, size: f32) -> f32 {
        size + self.icon_button_padding_x * 2.0
    }

    /// Largura **fixa** de toda aba (pedido do usuário, fora da espec.):
    /// o teto de rótulo mais o cromo que sempre o acompanha, saturado em
    /// `max_width`. Não depende do título, do indicador nem de quantas
    /// abas existem -- aba não muda de largura por nada.
    ///
    /// Antes disso a largura era `conteúdo.min(max_width)`, então cada
    /// aba tinha a largura do próprio título; trocar de aba, renomear ou
    /// abrir um programa que muda o título refluía a trilha inteira. Com
    /// largura fixa, o rótulo trunca dentro de uma caixa que já estava no
    /// teto -- é o mesmo teto de 180px da espec. §2.5, só que agora ele
    /// também é o piso.
    pub fn tab_width(&self) -> f32 {
        (self.padding_left
            + self.label_max_width
            + self.internal_gap
            + self.icon_button_width(self.close_button_size)
            + self.padding_right)
            .min(self.max_width)
    }

    /// Quanto de `tab_width` sobra para o rótulo depois do cromo fixo da
    /// aba e de `dot_reserve` (espec. §2.17: o indicador consome largura
    /// do rótulo, não soma largura à aba -- invariante que a largura fixa
    /// agora garante sozinha).
    fn label_cap(&self, dot_reserve: f32) -> f32 {
        (self.tab_width()
            - self.padding_left
            - dot_reserve
            - self.internal_gap
            - self.icon_button_width(self.close_button_size)
            - self.padding_right)
            .max(0.0)
    }
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
    /// Botão "+" ao final do grupo (`group.new_tab`) -- pedido do usuário,
    /// fora da espec.: nenhum grupo (implícito ou explícito) fica sem um,
    /// sempre logo depois do último elemento do wrapper (pílula sozinha
    /// se colapsado, última aba se não).
    /// `None` quando `show_new_tab_button` está desligado. Desde a
    /// remoção do botão global, é este o botão que a chave governa -- e a
    /// largura dele sai do wrapper junto, senão sobraria um vão.
    pub new_tab_button: Option<Rect>,
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
    pub caret_rect: Rect,
}

/// O layout inteiro da trilha: um por redraw da barra, construído por
/// [`layout`]. Sem o botão de nova aba global -- ele mora numa zona fixa
/// à direita da barra, fora do componente que rola (`settings_button_rect`),
/// pedido do usuário; só o botão **por grupo** (`GroupWrapperRect::
/// new_tab_button`) faz parte da trilha.
#[derive(Debug, Clone, PartialEq)]
pub struct TabBarLayout {
    /// Grupos vazios não aparecem aqui -- um wrapper sem aba nenhuma não
    /// é desenhável (ver `layout`).
    pub groups: Vec<GroupWrapperRect>,
    /// Largura total ocupada pela trilha, sem clamping à largura
    /// disponível da janela -- overflow (encolher, rolar) é a Etapa 5.
    pub content_width: f32,
    /// "+" ao fim da trilha, que cria uma aba **fora de todo grupo**.
    ///
    /// Só existe quando o último grupo da barra é **explícito**. Se a
    /// barra termina num run de abas soltas, o "+" daquele run já cria
    /// exatamente isso, no mesmo lugar -- dois botões idênticos lado a
    /// lado foi o que condenou o antigo botão global.
    ///
    /// É ele que cobre o caso em que **toda** aba está em grupo: sem
    /// isso, um workspace com um único grupo (colapsado, ainda por cima)
    /// não tem nenhum gesto que crie uma aba solta.
    pub ungrouped_new_tab_button: Option<Rect>,
}

/// O que um ponto da trilha atinge, em prioridade: botão de fechar antes
/// do corpo da aba (eles se sobrepõem), corpo da aba, pílula, botão de
/// nova aba do grupo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabBarHit {
    Tab(TabId),
    CloseButton(TabId),
    /// Espec. §2.4: "clique alterna colapso; duplo clique abre o editor" --
    /// o alvo é a pílula inteira, sem sub-região própria pro caret
    /// (RF-2.13, `docs/reference/acoes.md`: "o alvo é a pílula clicada").
    Pill(GroupId),
    /// Botão "+" ao final de um grupo -- `group.new_tab` nesse grupo
    /// específico.
    GroupNewTab(GroupId),
    /// "+" ao fim da trilha -- cria aba fora de todo grupo. Ver
    /// [`TabBarLayout::ungrouped_new_tab_button`].
    UngroupedNewTab,
}

/// Constrói a geometria da trilha: um wrapper por grupo não-vazio, abas
/// dentro dele, e o botão de nova aba do próprio grupo ao final de cada
/// um. Não sabe qual aba está ativa, em hover ou sendo renomeada -- isso é
/// estado de `porecatu-ui` que colore o resultado deste layout, não
/// entrada dele.
pub fn layout(
    workspace: &Workspace,
    style: &TabBarStyle,
    measurer: &mut TextMeasurer,
) -> TabBarLayout {
    let mut groups = Vec::new();
    // Todo o conteúdo da trilha nasce deslocado pelo respiro das bordas
    // (`trilha_padding`), não em (0, 0).
    let mut x = style.trilha_padding;
    let track_top = style.trilha_padding;

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
        // Aba solta ocupa a **caixa inteira** do wrapper, e não a altura
        // de aba com o respiro do wrapper sobrando em volta (pedido do
        // usuário). Dentro de um grupo a aba é menor de propósito: o que
        // a encolhe é o bloco do grupo em volta dela. Sem grupo não há
        // bloco, então não há o que ceder -- e as duas ficam com o mesmo
        // topo e a mesma base na barra, que é o alinhamento que se quer.
        // `pill.is_none()` é o mesmo teste de "run implícito" que decide
        // a cápsula em `chrome.rs`.
        let loose = !group.is_explicit();
        let tab_top = if loose {
            track_top
        } else {
            track_top + style.wrapper_padding
        };
        let tab_h = if loose {
            style.tab_height + style.wrapper_padding * 2.0
        } else {
            style.tab_height
        };

        let pill = if group.is_explicit() {
            let pill_y = track_top + style.wrapper_padding;
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

            let caret_width = style.icon_button_width(style.pill_caret_size);
            let caret_rect = Rect {
                x: px,
                y: pill_y + (pill_height - style.pill_caret_size) / 2.0,
                width: caret_width,
                height: style.pill_caret_size,
            };
            px += caret_width + style.pill_padding_right;

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
            let label_cap = style.label_cap(dot_reserve);

            let (label, label_truncated) =
                measurer.truncate(tab.title(), LABEL_FONT, style.font_size, label_cap);
            // Largura fixa: o título já não entra na conta (ver
            // `TabBarStyle::tab_width`). O `label` acima só decide o que
            // cabe *dentro* dela.
            let tab_width = style.tab_width();

            let rect = Rect {
                x: inner_x,
                y: tab_top,
                width: tab_width,
                height: tab_h,
            };
            let close_width = style.icon_button_width(style.close_button_size);
            let close_button = Rect {
                x: inner_x + tab_width - style.padding_right - close_width,
                y: tab_top + (tab_h - style.close_button_size) / 2.0,
                width: close_width,
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

        // Botão "+" do próprio grupo (pedido do usuário, fora da espec.):
        // logo depois da última aba, separado pelo mesmo `tab_gap` que já
        // separa abas entre si. Centrado na mesma caixa que as abas do
        // grupo: a do wrapper quando o run é solto, a da aba quando há
        // bloco.
        //
        // **Grupo colapsado não tem botão** (pedido do usuário): o
        // wrapper colapsado é a pílula e mais nada (§2.4: "suas abas
        // somem da barra"), e um "+" ao lado dela criaria aba num grupo
        // cujas abas não estão à vista -- sem contar que o wrapper
        // encolhe para caber só a pílula, que é o que faz o colapso
        // parecer colapso.
        let new_tab_button = if style.show_new_tab_button && !collapsed {
            if pill.is_some() || !tabs.is_empty() {
                inner_x += style.tab_gap;
            }
            let rect = Rect {
                x: inner_x,
                y: tab_top + (tab_h - style.close_button_size) / 2.0,
                width: style.icon_button_width(style.close_button_size),
                height: style.close_button_size,
            };
            inner_x += rect.width;
            Some(rect)
        } else {
            None
        };

        inner_x += style.wrapper_padding;
        let wrapper_rect = Rect {
            x: group_start_x,
            y: track_top,
            width: inner_x - group_start_x,
            height: style.tab_height + style.wrapper_padding * 2.0,
        };
        groups.push(GroupWrapperRect {
            id: group.id(),
            rect: wrapper_rect,
            pill,
            tabs,
            new_tab_button,
        });
        x = inner_x;
    }

    // "+" de aba solta, ao fim da trilha. Só quando o último grupo é
    // explícito: se a barra já termina num run de abas soltas, o "+"
    // daquele run cria a mesma coisa, no mesmo lugar. Fica **fora** de
    // qualquer wrapper, sobre o fundo da barra -- é o que o distingue,
    // à vista, do "+" que cria dentro de um grupo.
    let ungrouped_new_tab_button = if style.show_new_tab_button
        && workspace
            .groups()
            .iter()
            .rfind(|g| !g.tabs().is_empty())
            .is_some_and(|g| g.is_explicit())
    {
        x += style.trilha_gap;
        let rect = Rect {
            x,
            y: track_top
                + (style.tab_height + style.wrapper_padding * 2.0 - style.close_button_size) / 2.0,
            width: style.icon_button_width(style.close_button_size),
            height: style.close_button_size,
        };
        x += rect.width;
        Some(rect)
    } else {
        None
    };

    // O respiro também fecha a trilha à direita, senão o último wrapper
    // encostaria na zona fixa ao rolar até o fim. Barra sem grupo nenhum
    // não tem conteúdo a padear: largura zero, senão o `overflow_state`
    // passaria a ver conteúdo onde não há.
    let content_width = if groups.is_empty() {
        0.0
    } else {
        x + style.trilha_padding
    };

    TabBarLayout {
        groups,
        content_width,
        ungrouped_new_tab_button,
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

/// Layout da trilha pra uma largura disponível. **Não encolhe mais nada**
/// (mudança de performance, fora da espec. §2.18 -- ver nota abaixo): rótulo
/// de aba e nome de pílula ficam sempre no teto (`label_max_width`/
/// `pill_name_max_width`), e a trilha inteira rola como um componente só
/// quando não cabe -- é [`overflow_state`], chamado à parte por quem pinta,
/// que decide isso a partir do `content_width` que este `layout` já dá.
///
/// A espec. §2.18 descreve uma "ordem de cedência" (encolhe rótulo, depois
/// nome da pílula, só então rola) que este projeto implementou até aqui via
/// busca binária sobre [`layout`] -- até 24 recálculos completos da trilha
/// **por frame**, cada um remedindo texto de toda aba com `cosmic-text`
/// (`TextMeasurer::measure_width`/`truncate`, sem cache). Com a barra em
/// overflow (o caso comum de "muitas abas", que é justamente quando isto
/// roda), o custo cresce com o número de abas e virou perceptível o
/// suficiente pro app parecer travado ao trocar de aba ou mexer na janela.
/// Descartado por pedido direto do usuário: divergência da espec.
/// registrada aqui, não nela -- a barra ainda cabe tudo, só que rolando em
/// vez de encolhendo. `available_width` continua no parâmetro pra não mudar
/// a assinatura que todo chamador já tem em mãos, mesmo sem uso aqui dentro.
pub fn fit_width(
    workspace: &Workspace,
    style: &TabBarStyle,
    _available_width: f32,
    measurer: &mut TextMeasurer,
) -> TabBarLayout {
    layout(workspace, style, measurer)
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

/// Largura da zona fixa à direita da barra (pedido do usuário, fora da
/// espec.): só o suficiente pra sempre caber o botão de nova aba global,
/// com o mesmo `trilha_gap` que separa grupos como respiro nas duas
/// pontas -- reaproveitado em vez de inventar um padding próprio. Zero
/// Não depende de config: a zona é do botão de configurações, que existe
/// sempre. Ela nasceu para o botão de nova aba global (que saía de vista
/// com a trilha rolando), e sobreviveu a ele -- é o bloco reservado para
/// o que a barra ganhar à direita daqui em diante.
pub fn right_zone_width(style: &TabBarStyle) -> f32 {
    style.trilha_gap * 2.0 + style.icon_button_width(style.right_zone_button_size)
}

/// Largura da trilha rolável: a barra inteira menos a zona fixa da
/// direita -- é o `available_width` que [`overflow_state`] e o cálculo de
/// arraste na borda devem usar, não a largura total da janela.
pub fn trilha_width(style: &TabBarStyle, bar_width: f32) -> f32 {
    (bar_width - right_zone_width(style)).max(0.0)
}

/// Retângulo do botão de nova aba global, em coordenadas de tela da barra
/// (zona fixa à direita -- não rola com a trilha, ao contrário do botão
/// por grupo dentro de [`GroupWrapperRect::new_tab_button`]).
pub fn settings_button_rect(style: &TabBarStyle, bar_width: f32, bar_height: f32) -> Rect {
    Rect {
        x: bar_width - right_zone_width(style) + style.trilha_gap,
        y: (bar_height - style.right_zone_button_size) / 2.0,
        width: style.icon_button_width(style.right_zone_button_size),
        height: style.right_zone_button_size,
    }
}

pub fn point_in_settings_button(
    style: &TabBarStyle,
    bar_width: f32,
    bar_height: f32,
    point: (f32, f32),
) -> bool {
    rect_contains(settings_button_rect(style, bar_width, bar_height), point)
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
    for group in &layout.groups {
        if group
            .new_tab_button
            .is_some_and(|rect| rect_contains(rect, point))
        {
            return Some(TabBarHit::GroupNewTab(group.id));
        }
    }
    if layout
        .ungrouped_new_tab_button
        .is_some_and(|rect| rect_contains(rect, point))
    {
        return Some(TabBarHit::UngroupedNewTab);
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
    fn empty_workspace_has_no_wrapper() {
        let ws = Workspace::new();
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert!(layout.groups.is_empty());
        assert_eq!(layout.content_width, 0.0);
    }

    /// Pedido do usuário: a trilha não encosta nas bordas da barra. Vale
    /// nos quatro lados -- o wrapper é o que o usuário vê tocando o topo e
    /// a base, não a aba dentro dele.
    #[test]
    fn track_content_never_touches_the_bar_edges() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.group_tabs(&[a], "servidor", GroupColor::Cyan).unwrap();
        ws.append_ungrouped_tab("bash", None);

        let style = TabBarStyle::DEFAULT;
        let pad = style.trilha_padding;
        assert!(pad > 0.0);
        let bar = crate::chrome::bar_height(&style);
        let mut m = measurer();
        let layout = layout(&ws, &style, &mut m);

        assert_eq!(layout.groups[0].rect.x, pad, "primeiro wrapper na borda");
        for group in &layout.groups {
            assert_eq!(group.rect.y, pad, "wrapper encostando no topo");
            assert_eq!(
                group.rect.y + group.rect.height,
                bar - pad,
                "wrapper encostando na base"
            );
        }
        let last = layout.groups.last().unwrap();
        assert_eq!(
            layout.content_width,
            last.rect.x + last.rect.width + pad,
            "trilha sem respiro à direita"
        );
    }

    /// Pedido do usuário: aba solta ocupa a caixa inteira do wrapper, a
    /// mesma que a cápsula de um grupo ocuparia; aba dentro de um grupo
    /// fica menor, porque o bloco do grupo é que a encolhe. As duas
    /// alinham topo e base na barra.
    #[test]
    fn loose_tab_is_as_tall_as_a_group_block() {
        let mut ws = Workspace::new();
        let grouped = ws.append_tab("zsh", None);
        ws.group_tabs(&[grouped], "servidor", GroupColor::Blue)
            .unwrap();
        let loose = ws.append_ungrouped_tab("bash", None);

        let style = TabBarStyle::DEFAULT;
        let mut m = measurer();
        let layout = layout(&ws, &style, &mut m);

        let find = |id| {
            layout
                .groups
                .iter()
                .flat_map(|g| &g.tabs)
                .find(|t| t.id == id)
                .unwrap()
                .rect
        };
        let inside = find(grouped);
        let outside = find(loose);

        assert_eq!(inside.height, style.tab_height);
        assert_eq!(
            outside.height,
            style.tab_height + style.wrapper_padding * 2.0,
            "aba solta deveria ocupar a caixa do wrapper"
        );
        // O wrapper de cada uma é a mesma caixa -- é isso que faz as duas
        // lerem como "um bloco" na barra.
        for group in &layout.groups {
            assert_eq!(group.rect.y, style.trilha_padding);
            assert_eq!(
                group.rect.height,
                style.tab_height + style.wrapper_padding * 2.0
            );
        }
        assert_eq!(outside.y, layout.groups[1].rect.y);
        assert_eq!(
            outside.y + outside.height,
            layout.groups[1].rect.y + layout.groups[1].rect.height
        );
        // A aba agrupada fica centrada dentro do bloco dela.
        assert_eq!(inside.y, layout.groups[0].rect.y + style.wrapper_padding);
    }

    /// O botão "+" do grupo acompanha a caixa das abas dele -- senão ele
    /// ficaria descentrado justo no run solto, que é o mais alto.
    #[test]
    fn group_new_tab_button_is_centered_on_the_same_box_as_its_tabs() {
        let mut ws = Workspace::new();
        ws.append_ungrouped_tab("bash", None);
        let style = TabBarStyle::DEFAULT;
        let mut m = measurer();
        let layout = layout(&ws, &style, &mut m);

        let group = &layout.groups[0];
        let tab = &group.tabs[0];
        let button = group.new_tab_button.expect("botão ligado no default");
        let button_mid = button.y + button.height / 2.0;
        assert_eq!(button_mid, tab.rect.y + tab.rect.height / 2.0);
    }

    /// A altura da barra tem de acompanhar os dois respiros, senão o
    /// conteúdo deslocado vaza por baixo dela -- e é `bar_height` que
    /// `lib.rs` usa para deslocar a grade e converter clique.
    #[test]
    fn bar_height_accounts_for_both_paddings() {
        let style = TabBarStyle::DEFAULT;
        assert_eq!(
            crate::chrome::bar_height(&style),
            style.tab_height + style.wrapper_padding * 2.0 + style.trilha_padding * 2.0
        );
    }

    /// Desde a remoção do botão global, `show_new_tab_button` governa o
    /// "+" **de cada grupo** -- e a largura dele sai do wrapper junto,
    /// senão a chave deixaria um vão no lugar do botão.
    #[test]
    fn show_new_tab_button_disables_the_per_group_button() {
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = measurer();

        let ligado = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let desligado = layout(
            &ws,
            &TabBarStyle {
                show_new_tab_button: false,
                ..TabBarStyle::DEFAULT
            },
            &mut m,
        );
        assert!(ligado.groups[0].new_tab_button.is_some());
        assert!(desligado.groups[0].new_tab_button.is_none());
        assert!(
            desligado.groups[0].rect.width < ligado.groups[0].rect.width,
            "wrapper deveria encolher sem o botão"
        );
    }

    /// A zona fixa da direita **não** depende de config: ela é do botão de
    /// configurações, que existe sempre. Nasceu para o botão de nova aba
    /// global e sobreviveu a ele.
    #[test]
    fn settings_button_sits_in_the_fixed_right_zone() {
        let style = TabBarStyle::DEFAULT;
        let bar_width = 400.0;
        let bar_height = crate::chrome::bar_height(&style);
        let button = settings_button_rect(&style, bar_width, bar_height);
        let width = style.icon_button_width(style.right_zone_button_size);
        assert_eq!(button.x, bar_width - style.trilha_gap - width);
        assert_eq!(button.width, width);
        assert_eq!(
            button.height, style.right_zone_button_size,
            "o respiro entra só na largura"
        );
        assert_eq!(
            trilha_width(&style, bar_width),
            bar_width - right_zone_width(&style)
        );

        let sem_botao_de_aba = TabBarStyle {
            show_new_tab_button: false,
            ..style
        };
        assert_eq!(
            right_zone_width(&sem_botao_de_aba),
            right_zone_width(&style),
            "a zona da direita não é do botão de nova aba"
        );
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
        assert_eq!(
            tab.rect.x,
            TabBarStyle::DEFAULT.trilha_padding + TabBarStyle::DEFAULT.wrapper_padding
        );

        // botão de nova aba do grupo vem depois da aba, com o tab_gap
        assert_eq!(
            group.new_tab_button.unwrap().x,
            tab.rect.x + tab.rect.width + TabBarStyle::DEFAULT.tab_gap
        );
        assert_eq!(
            layout.content_width,
            group.rect.x + group.rect.width + TabBarStyle::DEFAULT.trilha_padding
        );
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

    /// Pedido do usuário: aba tem largura fixa. Título curto, título
    /// longo truncado e aba com indicador (que rouba orçamento do
    /// rótulo, espec. §2.17) precisam todos dar a mesma largura.
    #[test]
    fn every_tab_has_the_same_fixed_width() {
        let mut ws = Workspace::new();
        let short = ws.append_tab("a", None);
        let long = ws.append_tab("zsh", None);
        ws.tab_mut(long).unwrap().set_custom_title(Some(
            "um titulo bem comprido que estoura o teto do rotulo".to_string(),
        ));
        let with_dot = ws.append_tab("bash", None);
        ws.tab_mut(with_dot).unwrap().mark_activity();

        let style = TabBarStyle::DEFAULT;
        let mut m = measurer();
        let layout = layout(&ws, &style, &mut m);
        for tab in &layout.groups[0].tabs {
            assert_eq!(
                tab.rect.width,
                style.tab_width(),
                "aba {:?} fugiu da largura fixa",
                tab.id
            );
        }
        assert_eq!(layout.groups[0].tabs[0].id, short);
    }

    /// A largura fixa sai dos tokens da espec. (§2.5: rótulo 180px,
    /// padding 10/6, gap 8, botão de fechar 17 mais o respiro horizontal
    /// dele), nunca de um número inventado -- e continua saturada em
    /// `max_width`.
    #[test]
    fn fixed_tab_width_is_derived_from_the_spec_tokens() {
        let style = TabBarStyle::DEFAULT;
        assert_eq!(
            style.tab_width(),
            10.0 + 180.0 + 8.0 + style.icon_button_width(17.0) + 6.0
        );
        let tight = TabBarStyle {
            max_width: 100.0,
            ..style
        };
        assert_eq!(tight.tab_width(), 100.0);
    }

    /// O slot do caret na pílula tem de caber o **desenho** do caret mais
    /// largo dos dois, na em com que `chrome.rs` o pinta. Os dois valores
    /// vivem em módulos diferentes (`const` de `TabBarStyle` não alcança
    /// `chrome`), e é este teste que os mantém casados: se a em dos ícones
    /// mudar, ele reprova em vez de o caret vazar por cima do contador.
    #[test]
    fn pill_caret_slot_fits_the_widest_caret() {
        let needed =
            porecatu_render::icon::WIDEST_CARET_INK_EM * crate::chrome::PILL_CARET_ICON_SIZE;
        let slot = TabBarStyle::DEFAULT.pill_caret_size;
        assert!(
            slot >= needed - 0.05,
            "slot do caret ({slot}) menor que o desenho ({needed})"
        );
        // E não folgado a ponto de virar buraco na pílula.
        assert!(
            slot <= needed + 1.0,
            "slot do caret ({slot}) folgado demais para o desenho ({needed})"
        );
    }

    /// Pedido do usuário: os botões de ícone ganham respiro **só na
    /// largura**. A altura continua a do quadrado da espec., senão o
    /// botão de fechar deixaria de caber na aba.
    #[test]
    fn icon_buttons_are_wider_than_tall() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.group_tabs(&[a], "servidor", GroupColor::Green).unwrap();

        let style = TabBarStyle::DEFAULT;
        assert!(style.icon_button_padding_x > 0.0);
        let mut m = measurer();
        let layout = layout(&ws, &style, &mut m);
        let group = &layout.groups[0];

        for (nome, rect) in [
            ("fechar", group.tabs[0].close_button),
            ("+ do grupo", group.new_tab_button.unwrap()),
            ("+ de aba solta", layout.ungrouped_new_tab_button.unwrap()),
            ("caret", group.pill.as_ref().unwrap().caret_rect),
        ] {
            assert!(
                rect.width > rect.height,
                "{nome} deveria ter respiro horizontal: {rect:?}"
            );
        }
        assert_eq!(
            group.tabs[0].close_button.height, style.close_button_size,
            "a altura do botão de fechar não muda"
        );
    }

    /// Espec. §2.17: "a aba não muda de largura por causa do indicador" --
    /// o ponto sai do orçamento do rótulo.
    #[test]
    fn indicator_shrinks_the_label_cap_not_the_tab() {
        let style = TabBarStyle::DEFAULT;
        assert_eq!(style.label_cap(0.0), style.label_max_width);
        assert_eq!(
            style.label_cap(INDICATOR_DOT_SIZE + style.internal_gap),
            style.label_max_width - INDICATOR_DOT_SIZE - style.internal_gap
        );
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
    fn hit_test_group_new_tab_button() {
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let group = &layout.groups[0];
        let button = group.new_tab_button.expect("botão ligado no default");
        let center = (
            button.x + button.width / 2.0,
            button.y + button.height / 2.0,
        );
        assert_eq!(
            hit_test(&layout, center),
            Some(TabBarHit::GroupNewTab(group.id))
        );
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
    fn fit_width_matches_layout_when_it_already_fits() {
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = measurer();
        let unfit = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let fit = fit_width(&ws, &TabBarStyle::DEFAULT, 2000.0, &mut m);
        assert_eq!(unfit, fit);
    }

    // Cenário de aceite (mudança de performance pedida pelo usuário, F3
    // pós-fase: "dolorosamente lento" com a barra em overflow -- a busca
    // binária de até 24 recálculos de `layout` por frame, cada um
    // remedindo texto de toda aba, virou perceptível com muitas abas).
    // `fit_width` não encolhe mais rótulo nem nome de pílula -- ela é
    // exatamente `layout`, `available_width` não influencia o resultado
    // nenhum; rolar a trilha inteira (`overflow_state`, chamado à parte)
    // é o único jeito de "caber" agora.
    #[test]
    fn fit_width_never_shrinks_labels_regardless_of_available_width() {
        let mut ws = Workspace::new();
        for _ in 0..50 {
            let id = ws.append_tab("zsh", None);
            ws.tab_mut(id)
                .unwrap()
                .set_custom_title(Some("um titulo razoavelmente longo".to_string()));
        }
        let mut m = measurer();
        let full = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let fitted = fit_width(&ws, &TabBarStyle::DEFAULT, 300.0, &mut m);
        assert_eq!(full, fitted, "available_width não deve mudar o resultado");
        for tab in &fitted.groups[0].tabs {
            let width = m.measure_width(&tab.label, LABEL_FONT, TabBarStyle::DEFAULT.font_size);
            // Rótulo continua no teto (180px), nunca encolhido pro piso
            // antigo de 49px -- é exatamente o comportamento que sumiu.
            assert!(width > 60.0, "rótulo não deveria ter encolhido: {width}");
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
        assert!(pill.name_origin.0 < pill.caret_rect.x);
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

    // Cenário de aceite (mesma mudança de performance): nome de pílula
    // também não encolhe mais, nem sob overflow severo.
    #[test]
    fn fit_width_never_shrinks_pill_name_regardless_of_available_width() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.group_tabs(
            &[a],
            "um nome de grupo razoavelmente longo",
            GroupColor::Purple,
        );
        let mut m = measurer();

        let full = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let fitted = fit_width(&ws, &TabBarStyle::DEFAULT, 1.0, &mut m);
        assert_eq!(full, fitted);
        let pill = fitted.groups[0].pill.as_ref().unwrap();
        let name_width = m.measure_width(
            &pill.name,
            PILL_NAME_FONT,
            TabBarStyle::DEFAULT.pill_font_size,
        );
        // Continua no teto (140px), nunca encolhido pro piso antigo (60px).
        assert!(
            name_width > 60.0,
            "nome não deveria ter encolhido: {name_width}"
        );
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
    fn collapsed_group_wrapper_hugs_the_pill_alone() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a], "col", GroupColor::Cyan).unwrap();
        ws.collapse_group(group, true);

        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let wrapper = &layout.groups[0];
        let pill = wrapper.pill.as_ref().unwrap();
        let style = TabBarStyle::DEFAULT;
        // Pedido do usuário: colapsado não tem "+", então o wrapper é a
        // pílula e o respiro dele, e nada mais.
        assert!(wrapper.new_tab_button.is_none());
        assert_eq!(
            wrapper.rect.width,
            pill.rect.width + style.wrapper_padding * 2.0
        );
    }

    /// O caso do relato: um único grupo, colapsado, sem aba solta
    /// nenhuma. Sem este botão não sobra gesto nenhum que crie uma aba
    /// fora do grupo -- o "+" do grupo está escondido pelo colapso, e o
    /// atalho `tab.new` cria dentro do grupo da aba ativa.
    #[test]
    fn lone_collapsed_group_still_offers_an_ungrouped_new_tab() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a], "col", GroupColor::Cyan).unwrap();
        ws.collapse_group(group, true);

        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert!(layout.groups[0].new_tab_button.is_none(), "grupo colapsado");
        let button = layout
            .ungrouped_new_tab_button
            .expect("sem este botão não há como criar aba solta");
        assert!(
            button.x > layout.groups[0].rect.x + layout.groups[0].rect.width,
            "deveria ficar depois do wrapper, fora dele"
        );
        assert_eq!(
            hit_test(&layout, (button.x + 1.0, button.y + 1.0)),
            Some(TabBarHit::UngroupedNewTab)
        );
    }

    /// E ele não aparece quando a barra já termina num run de abas
    /// soltas: o "+" daquele run cria exatamente a mesma coisa, no mesmo
    /// lugar. Dois botões idênticos lado a lado foi o que condenou o
    /// antigo botão global.
    #[test]
    fn no_ungrouped_button_when_the_bar_already_ends_in_loose_tabs() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.group_tabs(&[a], "servidor", GroupColor::Red).unwrap();
        ws.append_ungrouped_tab("bash", None);

        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert!(layout.ungrouped_new_tab_button.is_none());
        // O run solto, esse, tem o dele.
        assert!(layout.groups[1].new_tab_button.is_some());
    }

    /// Barra sem grupo explícito nenhum também não precisa dele.
    #[test]
    fn no_ungrouped_button_on_a_bar_without_explicit_groups() {
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = measurer();
        let layout = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        assert!(layout.ungrouped_new_tab_button.is_none());
    }

    /// `show_new_tab_button` desliga os dois botões, não só o do grupo.
    #[test]
    fn show_new_tab_button_also_disables_the_ungrouped_button() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        ws.group_tabs(&[a], "servidor", GroupColor::Red).unwrap();
        let mut m = measurer();
        let layout = layout(
            &ws,
            &TabBarStyle {
                show_new_tab_button: false,
                ..TabBarStyle::DEFAULT
            },
            &mut m,
        );
        assert!(layout.ungrouped_new_tab_button.is_none());
        assert!(layout.groups[0].new_tab_button.is_none());
    }

    /// Expandir devolve o botão -- o colapso esconde, não desliga.
    #[test]
    fn expanding_brings_the_group_new_tab_button_back() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a], "col", GroupColor::Cyan).unwrap();
        let mut m = measurer();

        ws.collapse_group(group, true);
        assert!(
            layout(&ws, &TabBarStyle::DEFAULT, &mut m).groups[0]
                .new_tab_button
                .is_none()
        );

        ws.collapse_group(group, false);
        assert!(
            layout(&ws, &TabBarStyle::DEFAULT, &mut m).groups[0]
                .new_tab_button
                .is_some()
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

    // Cenário de aceite: muitos grupos com nome longo -- o caso que antes
    // acionava os dois estágios de encolhimento -- continuam todos no
    // teto, e `fit_width` não faz mais nenhuma busca (não recalcula
    // `layout` de novo por trás das cenas pra descobrir isso).
    #[test]
    fn fit_width_with_many_long_named_groups_keeps_every_pill_name_at_ceiling() {
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
        let full = layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        let fitted = fit_width(&ws, &TabBarStyle::DEFAULT, 300.0, &mut m);
        assert_eq!(full, fitted);
        for group in &fitted.groups {
            let pill = group.pill.as_ref().unwrap();
            let width = m.measure_width(
                &pill.name,
                PILL_NAME_FONT,
                TabBarStyle::DEFAULT.pill_font_size,
            );
            assert!(width > 60.0, "nome não deveria ter encolhido: {width}");
        }
    }
}
