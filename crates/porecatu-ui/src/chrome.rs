// SPDX-License-Identifier: GPL-3.0-or-later

//! Traduz `tab_bar::TabBarLayout` (mais estado efêmero que o layout puro
//! não conhece: aba ativa, aba `Exited`, edição de rename em andamento,
//! rolagem e arraste desde a Etapa 5, seleção múltipla desde a F3 etapa 2)
//! em `Primitive`s da camada `Chrome` (ADR-0018). Cores e dimensões: espec.
//! visual §1.2, §1.3, §2.3, §2.4, §2.5, §2.6, §2.17, §2.18, §2.19, como
//! constantes em `palette.rs`/`tab_bar.rs`, mesmo padrão de `paint.rs` para
//! a grade.
//!
//! Sem hover nesta etapa -- a barra não rastreia posição do mouse fora de
//! clique/arraste (`App::cursor_position` é da área do terminal); o estado
//! default de cada elemento já é o que a espec. descreve fora de hover,
//! então a barra fica correta sem ele -- é um refinamento, não uma etapa
//! 4/5/6. Pelo mesmo motivo, o `filter: brightness(1.18)` e a sombra de
//! popover do fantasma de arraste (espec. §2.19) não têm equivalente em
//! `porecatu-render` -- nenhuma primitiva de filtro ou sombra existe ainda
//! (nenhum hover em lugar nenhum do chrome usa isso hoje); o fantasma
//! reaproveita as cores normais da aba, sem o realce.
//!
//! Desde a F3 etapa 3, pílula e wrapper tingido (§2.3/§2.4) também entram
//! aqui -- mas só a geometria e a cor que já existem em `porecatu-core`
//! desde a etapa 1 (`Group::color`/`is_collapsed`). O caret também não gira:
//! `RoundedQuad`/`TextRun` não têm transform, então a troca de glyph
//! (`▶`/`▼`) é o equivalente estático, mesma lacuna já registrada acima para
//! `brightness`/sombra.
//!
//! Desde a F3 etapa 4, grupo colapsado não desenha abas (o layout já não as
//! gera, ver `tab_bar.rs`) e a pílula ganha o indicador agregado (RF-2.16),
//! pintado aqui com as mesmas cores de `Indicator` da seção 2.17. Clique na
//! pílula (`group.toggle_collapse`, RF-2.13) e duplo clique (editor, F3
//! etapa 5) são wiring de `lib.rs`, fora desta função de pintura pura.
//!
//! Desde a F3 etapa 6, três coisas a mais: o realce de fronteira do
//! arraste de aba (`drag_highlight`, espec. §2.19/ADR-0021 §4); o fantasma
//! do arraste de grupo (`group_drag`, espec. §2.19.1) -- só a pílula,
//! reaproveitando `paint_group_pill` deslocada pelo `dx` certo, e o
//! wrapper de origem inteiro pulado enquanto arrasta (`continue` no laço
//! principal, abrindo o "vão" que a espec pede); e a interpolação do
//! relógio de animação (`animations`, ADR-0022) -- a pílula e as abas de
//! um grupo deslizam (só posição, `dx`) da posição antiga até a que
//! `layout` calculou, quando há uma reflui ativa pra ele; a **cápsula**
//! por trás delas interpola posição **e largura** (`capsule_rect`), o que
//! é o que faz o próprio grupo encolher/crescer suave ao colapsar/
//! expandir, não só deslizar os vizinhos -- continua desenhada enquanto o
//! progresso não chega em 1, mesmo já colapsado no modelo (senão a
//! cápsula sumiria na hora e só as abas esmaeceriam por cima do nada).
//! `DragGhost` carrega o `TabRect` de origem (`base_layout`, não o
//! preview) desde esta etapa: soltar sobre um grupo colapsado faz o
//! preview não gerar `TabRect` nenhum pra aba arrastada, e o fantasma
//! precisa do conteúdo mesmo assim.
//!
//! Colapsar/expandir também esmaece as abas do próprio grupo em vez de
//! picá-las (espec §2.4: "o que anima de fato é o resto do colapso: as
//! abas desaparecendo da trilha"). Abas que sumiram do `layout` corrente
//! mas existiam em `old_layout` (`AnimationClock::old_tabs`) continuam
//! desenhadas na posição antiga, opacidade caindo; as que são novas ali
//! (`AnimationClock::had_tab` devolve falso) entram com opacidade
//! subindo, junto com o progresso da reflui do próprio wrapper delas.

use std::collections::HashSet;
use std::time::Instant;

use porecatu_core::{GroupId, TabId, Workspace};
use porecatu_render::{
    Color, FontFace, Primitive, Quad, Rect, RoundedQuad, SansWeight, TextRun, icon,
};

use crate::animation::AnimationClock;
use crate::group_editor::GroupEditor;
use crate::palette;
use crate::rename::RenameState;
use crate::selection::Selection;
use crate::tab_bar::{
    self, GroupPillRect, INDICATOR_DOT_SIZE, Indicator, Overflow, OverflowSide, PILL_COUNT_FONT,
    PILL_COUNT_FONT_SIZE, PILL_NAME_FONT, TabBarLayout, TabBarStyle,
};

/// Fonte dos ícones da barra (fechar, nova aba, chevron): a face Lucide
/// embutida, não a IBM Plex Sans do rótulo. Os glyphs Unicode que a espec.
/// usa para desenhar esses ícones no papel (✕ U+2715, ▶ U+25B6, ▼ U+25BC)
/// **não existem** na Plex Sans, e o `fontdb` do projeto não carrega fonte
/// do sistema (ADR-0016): sem face própria eles não desenhavam nada. Ver
/// `porecatu_render::icon`.
pub(crate) const ICON_FONT: FontFace = FontFace::Icon;
const LABEL_FONT: FontFace = FontFace::Sans {
    weight: SansWeight::Regular,
};

// Tamanhos de **em**, não de desenho: o Lucide preenche ~0.6 em, então a
// em tem de ser cerca do dobro do número da especificação para o desenho
// sair no tamanho que ela pede -- e para o traço (`2/24` da em) render
// sólido em vez de esmaecer contra o fundo. Ver `porecatu_render::icon`.
// Pedido do usuário depois de ver os 10px em tela.
pub(crate) const ICON_EM_SIZE: f32 = 20.0;
const CLOSE_ICON_SIZE: f32 = ICON_EM_SIZE; // espec §2.5: "✕ 10px" de desenho
const NEW_TAB_ICON_SIZE: f32 = ICON_EM_SIZE; // espec §2.6: "+ 15px" de desenho
// Espec §2.5 pede um sublinhado de 2px na cor do grupo na base de cada
// aba (`indicator_style = ["pill", "underline"]`, RF-4.19) -- removido a
// pedido do usuário. Ele nasceu para dizer a que grupo a aba pertence
// quando a pílula sai da vista por rolagem; desde que a cápsula passou a
// ser pintada com a cor cheia (F3 etapa 6), a cor do grupo já está atrás
// da aba inteira e o traço virou ruído na base dela. Divergência na
// seção 4.4 da especificação; quando `config` existir (F4), o default de
// `indicator_style` é a chave que governa isto.
const BAR_SEPARATOR_HEIGHT: f32 = 1.0;
// Borda da aba em todo estado. A espec. §2.5 desenha 1px; contra a cápsula
// de cor cheia (F3 etapa 6) 1px de `#22262e` não se lê, e o pedido do
// usuário foi "coloca um border nas abas". 2px é a espessura que o próprio
// arquivo de exemplo já usa para linha de chrome (`indicator_thickness`,
// `selected_border_width`) -- não um número novo. Cada estado mantém a cor
// dele, que é o que continua separando ativa de inativa.
const TAB_BORDER_WIDTH: f32 = 2.0;
// `[appearance.tabs] selected_border_width` -- espec §2.5: "2px por dentro"
// (`Primitive::RoundedQuad` não soma largura ao rect por causa da borda,
// então isto não reflui a aba). Mesma espessura da borda de base desde o
// ajuste acima: o que marca a seleção é a **cor**, o verde-água do token.
const SELECTED_BORDER_WIDTH: f32 = TAB_BORDER_WIDTH;

// Wrapper de grupo (espec §2.3, `[appearance.groups]`).
const WRAPPER_CORNER_RADIUS: f32 = 8.0; // wrapper_corner_radius
// Espec §2.3/RF-4.19 pede `tint_strength = 0.07` pro fundo do wrapper --
// superado por pedido direto do usuário (F3 etapa 6): o grupo é uma
// "cápsula" pintada com a cor cheia, não um tingimento sutil. Divergência
// registrada aqui, não na especificação visual (que continua descrevendo
// o v1 "de livro"; ver seção 4.4 dela pro registro formal de divergências
// já conhecidas -- esta é nova e ainda não está lá).
const GROUP_CAPSULE_FILL_STRENGTH: f64 = 1.0;

// Realce de fronteira do arraste de aba (espec §2.19, ADR-0021 §4).
// "Sobe o tingimento de .07 para .16 -- o mesmo badge_tint_strength que o
// arquivo de exemplo já usa" -- mas `badge_tint_strength` no TOML vale
// 0.14 (seção do badge de perfil, [v2]), não .16: a prosa da espec.
// arredonda, o TOML é a fonte numérica canônica deste projeto. Usa-se o
// valor do TOML, com a divergência registrada aqui em vez de inventar um
// terceiro número.
const DRAG_HIGHLIGHT_TINT_STRENGTH: f64 = 0.14; // badge_tint_strength
// "Borda 1px na cor do grupo com alfa .45" -- sem chave própria no TOML.
const DRAG_HIGHLIGHT_BORDER_ALPHA: f64 = 0.45;
const DRAG_HIGHLIGHT_BORDER_WIDTH: f32 = 1.0;

// Pílula de grupo (espec §2.4, `[appearance.groups]`).
const PILL_CORNER_RADIUS: f32 = 6.0; // label_corner_radius
const PILL_BORDER_WIDTH: f32 = 1.0; // espec §2.4: "borda 1px" -- sem chave própria
const PILL_SWATCH_CORNER_RADIUS: f32 = 2.0; // swatch_corner_radius
const PILL_COUNT_CORNER_RADIUS: f32 = 9.0; // count_corner_radius
// Espec §2.4, item 4: "▶ 8px, rotate(0deg) colapsado, rotate(90deg)
// expandido". Sem primitiva de rotação (ver nota do módulo) -- a troca de
// ícone é o equivalente estático. A em é a mesma dos outros ícones; o que
// o layout reserva (`style.pill_caret_size`) é a largura do **desenho**,
// que é menor -- ver `porecatu_render::icon`.
const PILL_CARET_COLLAPSED: icon::Icon = icon::CHEVRON_RIGHT;
const PILL_CARET_EXPANDED: icon::Icon = icon::CHEVRON_DOWN;
pub(crate) const PILL_CARET_ICON_SIZE: f32 = ICON_EM_SIZE;

// Campo de rename: espec §2.5 dá largura (120), padding (2px 5px) e fonte
// (12px), mas não a altura da caixa. Valor de trabalho: texto 12px +
// padding vertical 2px de cada lado + folga -- ajustar se ficar
// visualmente errado na prática (mesmo tipo de nota que F1 deixou em
// `FONT_SIZE_PX`/`LINE_HEIGHT_MULTIPLIER`).
const RENAME_FIELD_HEIGHT: f32 = 20.0;
const RENAME_FIELD_MAX_WIDTH: f32 = 120.0;
const RENAME_FONT_SIZE: f32 = 12.0;
const RENAME_PADDING_X: f32 = 5.0;

const OVERFLOW_CHEVRON_SIZE: f32 = ICON_EM_SIZE; // espec §2.18: "chevron ‹/› 10px"
const OVERFLOW_COUNT_FONT_SIZE: f32 = 10.0; // espec §2.18: "contagem em mono 10px"
const OVERFLOW_COUNT_RADIUS: f32 = 9.0; // espec §2.4 (mesmo contador da pílula)
const OVERFLOW_INNER_GAP: f32 = 3.0; // folga de trabalho entre chevron e contagem

/// A aba sendo arrastada (espec §2.19): desenhada como fantasma seguindo o
/// cursor no eixo X, presa ao Y da barra -- em vez de na posição que o
/// `layout` calculou para ela (que já reflete o preview de onde ela cairia,
/// e é onde o "buraco" fica: a aba não é desenhada na posição normal
/// enquanto isto está `Some`, deixando o fundo da barra aparecer).
#[derive(Debug, Clone, PartialEq)]
pub struct DragGhost {
    pub tab: TabId,
    /// Coordenada de tela (sem o deslocamento de rolagem) do canto
    /// esquerdo do fantasma.
    pub screen_x: f32,
    /// Retângulo/rótulo/indicador de `base_layout` (`lib.rs`), de antes de
    /// qualquer preview -- garante conteúdo mesmo quando o alvo do
    /// arraste (F3 etapa 6) é um grupo colapsado, cujo preview não gera
    /// `TabRect` nenhum pra essa aba (§2.4: "abas somem da barra").
    pub source: tab_bar::TabRect,
}

/// O grupo sendo arrastado pelo rótulo (espec §2.19.1): o fantasma é só a
/// pílula, seguindo o cursor no eixo X -- diferente do arraste de aba, o
/// grupo inteiro (wrapper + abas) some da posição em que o preview o
/// colocaria (`paint` pula o desenho dele por completo), abrindo o "vão"
/// que a espec descreve, em vez de renderizar o conteúdo normalmente ali.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupDragGhost {
    pub group: GroupId,
    pub screen_x: f32,
}

/// Monta as primitivas da barra inteira: fundo, separador, cada aba (fundo,
/// borda, sublinhado, indicador, rótulo ou campo de rename, botão de
/// fechar), o botão de nova aba, os indicadores de overflow (espec §2.18) e
/// o fantasma de arraste (espec §2.19), se algum estiver em andamento.
///
/// `layout` já reflete, durante um arraste, o preview de reordenação
/// (`lib.rs` monta um `Workspace` clonado com a troca aplicada antes de
/// chamar `tab_bar::fit_width`) -- esta função só desenha o que recebe, sem
/// saber dessa decisão. `fit_width` não encolhe rótulo nem nome de pílula
/// (nota do módulo `tab_bar.rs`): a rolagem (`overflow`, §2.18) é o único
/// jeito de a trilha "caber" quando estoura.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    layout: &TabBarLayout,
    workspace: &Workspace,
    active: Option<TabId>,
    rename: &RenameState,
    selection: &Selection,
    group_editor: Option<&GroupEditor>,
    style: &TabBarStyle,
    bar_width: f32,
    overflow: Overflow,
    drag: Option<DragGhost>,
    group_drag: Option<GroupDragGhost>,
    drag_highlight: Option<(GroupId, Rect)>,
    animations: &AnimationClock,
    now: Instant,
    measurer: &mut porecatu_render::TextMeasurer,
) -> Vec<Primitive> {
    // Nunca recalcular esta fórmula aqui: `bar_height` é a mesma altura
    // que `lib.rs` usa para deslocar a grade e converter clique. Uma cópia
    // local dela ficou para trás quando `trilha_padding` entrou na conta,
    // e o efeito foi o fundo da barra e o recorte da trilha pararem 12px
    // acima do fim dela -- o respiro de baixo simplesmente não podia
    // aparecer, porque o clip cortava as abas antes.
    let bar_height = bar_height(style);
    let mut out = Vec::new();

    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: bar_width,
            height: bar_height,
        },
        color: palette::BAR_BACKGROUND,
    }));
    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: 0.0,
            y: bar_height - BAR_SEPARATOR_HEIGHT,
            width: bar_width,
            height: BAR_SEPARATOR_HEIGHT,
        },
        color: palette::BAR_SEPARATOR,
    }));

    // Recorte de verdade da trilha (ADR-0018, espec §2.18: "um recorte só,
    // na camada de chrome; as abas fora da vista desaparecem pelo clip").
    // Tudo dentro deste par desloca pelo scroll -- inclusive o botão "+" de
    // cada grupo, que "acompanha o scroll" igual às abas dele (fora da
    // espec., mesmo raciocínio do §2.6 aplicado ao botão por grupo). Só vai
    // até `trilha_width` -- a zona fixa da direita (pedido do usuário) fica
    // de fora do recorte e do scroll.
    out.push(Primitive::PushClip(Rect {
        x: 0.0,
        y: 0.0,
        width: tab_bar::trilha_width(style, bar_width),
        height: bar_height,
    }));
    let scroll_dx = -overflow.scroll_offset;

    for group in &layout.groups {
        // Espec §2.19.1: "o wrapper de origem colapsa pra largura zero" --
        // o grupo inteiro some daqui enquanto o rótulo dele está sendo
        // arrastado; só o fantasma (pintado no fim) marca onde ele está.
        // `layout` já é o preview (posição de destino provisória), então
        // pular o desenho aqui é o que abre o "vão" que a espec descreve.
        if group_drag.is_some_and(|g| g.group == group.id) {
            continue;
        }

        // ADR-0022: enquanto o grupo tem uma reflui ativa (RF-2.5, ou o
        // colapso/expansão deste grupo ou de um vizinho antes/depois dele
        // na trilha), o retângulo do wrapper inteiro -- posição **e**
        // largura -- interpola do que era em `old_layout` pro que `layout`
        // já calculou. Pílula e abas dentro dele só deslizam (`dx`, sem
        // esticar) -- é a cápsula em volta delas que encolhe/cresce. Fora
        // de animação, `wrapper_progress` devolve `None` e isto pinta
        // exatamente onde sempre pintou.
        let wrapper_progress = animations.wrapper_progress(group.id, now);
        let anim_dx = wrapper_progress
            .map(|(old_rect, progress)| (old_rect.x - group.rect.x) * (1.0 - progress))
            .unwrap_or(0.0);
        let dx = scroll_dx + anim_dx;
        let capsule_rect = match wrapper_progress {
            Some((old_rect, progress)) => Rect {
                x: group.rect.x + anim_dx + scroll_dx,
                y: group.rect.y,
                width: old_rect.width + (group.rect.width - old_rect.width) * progress,
                height: group.rect.height,
            },
            None => shift(group.rect, dx),
        };

        let core_group = workspace.group(group.id);
        let is_collapsed = core_group.is_some_and(|g| g.is_collapsed());
        // Espec §2.5: "sublinhado de aba sem grupo" usa `ungrouped_color`;
        // grupo explícito usa a cor do grupo -- mesma resolução para o
        // tingimento do wrapper e o swatch da pílula abaixo.
        let group_color = core_group
            .and_then(|g| g.color())
            .map(palette::group_color)
            .unwrap_or(palette::UNGROUPED_UNDERLINE);

        // Ajuste pedido pelo usuário (F3 etapa 6, fora da espec.): o grupo
        // é uma "cápsula" pintada com a cor cheia -- não o tingimento de
        // 7% da espec §2.3, que ficava quase invisível atrás do fundo
        // opaco das abas. `TAB_ACTIVE_BACKGROUND`/`TAB_INACTIVE_BACKGROUND`
        // (`palette.rs`) agora têm alfa .85 pra deixar passar um indício
        // dela por cima. Fora de animação, "colapsado fica transparente"
        // continua valendo (RF-4.19); durante a animação, a cápsula segue
        // desenhada (encolhendo/crescendo) até o progresso chegar em 1 --
        // senão o colapso sumiria a cápsula na hora e só as abas
        // esmaeceriam por cima do nada. Abas sem grupo (`pill == None`)
        // nunca pintam cápsula.
        let mid_collapse_animation = wrapper_progress.is_some_and(|(_, progress)| progress < 1.0);
        if group.pill.is_some() && (!is_collapsed || mid_collapse_animation) {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: capsule_rect,
                radius: WRAPPER_CORNER_RADIUS,
                color: with_alpha(group_color, GROUP_CAPSULE_FILL_STRENGTH),
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        // Espec §2.19, ADR-0021 §4: "o wrapper que receberia a aba sobe o
        // tingimento... e ganha borda 1px na cor do grupo com alfa .45" --
        // por cima da cápsula (senão ela cobriria o realce por completo) e
        // por baixo da pílula/abas (senão o realce cobriria o conteúdo).
        // O run implícito também recebe realce, usando `ungrouped_color`
        // (já resolvida em `group_color`).
        if drag_highlight.is_some_and(|(id, _)| id == group.id) {
            let highlight_rect = drag_highlight.map(|(_, rect)| rect).expect("checado acima");
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: shift(highlight_rect, dx),
                radius: WRAPPER_CORNER_RADIUS,
                color: with_alpha(group_color, DRAG_HIGHLIGHT_TINT_STRENGTH),
                border_color: with_alpha(group_color, DRAG_HIGHLIGHT_BORDER_ALPHA),
                border_width: DRAG_HIGHLIGHT_BORDER_WIDTH,
            }));
        }

        if let Some(pill) = &group.pill {
            // Espec §2.10: "o nome muda na barra enquanto se digita" --
            // enquanto o editor deste grupo está aberto, a pílula mostra o
            // buffer ao vivo (`GroupEditor::name_buffer`) no lugar do nome
            // já commitado que `pill.name` carrega, mesmo truque do campo
            // de rename de aba (buffer preferido ao modelo na hora de
            // pintar, nunca escrito nele até confirmar).
            let live_name = group_editor
                .filter(|e| e.group == group.id)
                .map(GroupEditor::name_buffer);
            paint_group_pill(
                pill,
                group_color,
                is_collapsed,
                live_name,
                style.pill_font_size,
                dx,
                measurer,
                &mut out,
            );
        }

        for tab in &group.tabs {
            let is_ghost = drag.as_ref().is_some_and(|g| g.tab == tab.id);
            let tab_rect = shift(tab.rect, dx);

            if is_ghost {
                // O buraco (espec §2.19): fundo da barra já pintado acima
                // aparece por baixo -- nada a desenhar aqui, o fantasma vem
                // depois, fora do recorte.
                continue;
            }

            // ADR-0022: aba que não existia em `old_layout` (grupo estava
            // colapsado) mas já está no layout corrente (acabou de
            // expandir) entra esmaecendo -- opacidade sobe de 0 a 1 junto
            // com o progresso da mesma reflui que desliza o wrapper. Aba
            // que já existia antes (não é nova) desenha na opacidade
            // normal, animação ou não.
            let fade_in = if animations.had_tab(tab.id) {
                1.0
            } else {
                animations
                    .wrapper_progress(group.id, now)
                    .map_or(1.0, |(_, progress)| progress)
            };

            let exited = workspace.tab(tab.id).is_some_and(|t| t.is_exited());
            let is_active = active == Some(tab.id);
            let (bg, border, text_color) = tab_colors(exited, is_active);
            // RF-2.2/espec §2.5: selecionada é um modificador de borda, não
            // um quarto estado -- fundo e texto continuam vindo de
            // Ativa/Inativa acima.
            let (border, border_width) = if selection.is_selected(tab.id) {
                (palette::SELECTED_BORDER, SELECTED_BORDER_WIDTH)
            } else {
                (border, TAB_BORDER_WIDTH)
            };

            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: tab_rect,
                radius: 6.0,
                color: scale_alpha(bg, fade_in),
                border_color: scale_alpha(border, fade_in),
                border_width,
            }));

            let dot_reserve = if tab.indicator.is_some() {
                INDICATOR_DOT_SIZE + style.internal_gap
            } else {
                0.0
            };
            if let Some(indicator) = tab.indicator {
                let color = match indicator {
                    Indicator::Activity => palette::ACTIVITY_INDICATOR,
                    Indicator::Bell => palette::BELL_INDICATOR,
                };
                out.push(Primitive::RoundedQuad(RoundedQuad {
                    rect: Rect {
                        x: tab_rect.x + style.padding_left,
                        y: tab_rect.y + (tab_rect.height - INDICATOR_DOT_SIZE) / 2.0,
                        width: INDICATOR_DOT_SIZE,
                        height: INDICATOR_DOT_SIZE,
                    },
                    radius: INDICATOR_DOT_SIZE / 2.0,
                    color: scale_alpha(color, fade_in),
                    border_color: palette::TRANSPARENT,
                    border_width: 0.0,
                }));
            }

            if rename.editing_tab() == Some(tab.id) {
                paint_rename_field(tab_rect, style, rename.buffer(), measurer, &mut out);
            } else {
                let label_y = tab_rect.y + (tab_rect.height - style.font_size) / 2.0;
                out.push(Primitive::Text(TextRun {
                    origin: (tab_rect.x + style.padding_left + dot_reserve, label_y),
                    text: tab.label.clone(),
                    font: LABEL_FONT,
                    size_px: style.font_size,
                    color: scale_alpha(text_color, fade_in),
                }));
            }

            out.push(centered_glyph(
                icon::X,
                shift(tab.close_button, dx),
                CLOSE_ICON_SIZE,
                scale_alpha(palette::CLOSE_BUTTON_ICON, fade_in),
            ));
        }

        // Botão "+" do próprio grupo (pedido do usuário, fora da espec.):
        // desliza com o wrapper (`dx`) igual à pílula e às abas dele --
        // sem esticar, sem fade próprio -- mesmas cores do botão global
        // pra ler como "o mesmo botão, num segundo lugar".
        let group_button = shift(group.new_tab_button, dx);
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: group_button,
            radius: 6.0,
            color: palette::TRANSPARENT,
            border_color: palette::NEW_TAB_BORDER,
            border_width: 1.0,
        }));
        out.push(centered_glyph(
            icon::PLUS,
            group_button,
            NEW_TAB_ICON_SIZE,
            palette::NEW_TAB_ICON,
        ));
    }

    // ADR-0022: abas que existiam em `old_layout` mas sumiram do layout
    // corrente (grupo acabou de colapsar) continuam desenhadas, esmaecendo
    // na posição antiga em vez de sumir na hora -- "o que anima de fato é
    // o resto do colapso: as abas desaparecendo da trilha" (espec §2.4).
    // O wrapper delas não se move (nada antes dele mudou, só ele mesmo
    // encolheu), então só o deslocamento de rolagem se aplica aqui, não a
    // reflui de nenhum grupo.
    let current_tab_ids: HashSet<TabId> = layout
        .groups
        .iter()
        .flat_map(|g| &g.tabs)
        .map(|t| t.id)
        .collect();
    for (old_tab, progress) in animations.old_tabs(now) {
        if current_tab_ids.contains(&old_tab.id) {
            continue;
        }
        let fade_out = 1.0 - progress;
        let tab_rect = shift(old_tab.rect, scroll_dx);
        let exited = workspace.tab(old_tab.id).is_some_and(|t| t.is_exited());
        let is_active = active == Some(old_tab.id);
        let (bg, border, text_color) = tab_colors(exited, is_active);
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: tab_rect,
            radius: 6.0,
            color: scale_alpha(bg, fade_out),
            border_color: scale_alpha(border, fade_out),
            border_width: 1.0,
        }));
        let dot_reserve = if old_tab.indicator.is_some() {
            INDICATOR_DOT_SIZE + style.internal_gap
        } else {
            0.0
        };
        let label_y = tab_rect.y + (tab_rect.height - style.font_size) / 2.0;
        out.push(Primitive::Text(TextRun {
            origin: (tab_rect.x + style.padding_left + dot_reserve, label_y),
            text: old_tab.label.clone(),
            font: LABEL_FONT,
            size_px: style.font_size,
            color: scale_alpha(text_color, fade_out),
        }));
    }

    out.push(Primitive::PopClip);

    // Botão de nova aba global (pedido do usuário): zona fixa à direita da
    // barra, fora do recorte da trilha -- não rola com o conteúdo, ao
    // contrário do botão por grupo pintado acima.
    if let Some(button) = tab_bar::new_tab_button_rect(style, bar_width, bar_height) {
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: button,
            radius: 6.0,
            color: palette::TRANSPARENT,
            border_color: palette::NEW_TAB_BORDER,
            border_width: 1.0,
        }));
        out.push(centered_glyph(
            icon::PLUS,
            button,
            NEW_TAB_ICON_SIZE,
            palette::NEW_TAB_ICON,
        ));
    }

    // Pílulas de overflow (espec §2.18) ficam dentro da trilha rolável, não
    // da barra inteira -- senão a da direita cairia por cima da zona fixa
    // do botão global.
    let trilha_width = tab_bar::trilha_width(style, bar_width);
    if overflow.hidden_left > 0 {
        paint_overflow_pill(
            OverflowSide::Left,
            overflow.hidden_left,
            trilha_width,
            bar_height,
            measurer,
            &mut out,
        );
    }
    if overflow.hidden_right > 0 {
        paint_overflow_pill(
            OverflowSide::Right,
            overflow.hidden_right,
            trilha_width,
            bar_height,
            measurer,
            &mut out,
        );
    }

    if let Some(ghost) = &drag {
        let tab = &ghost.source;
        let exited = workspace.tab(tab.id).is_some_and(|t| t.is_exited());
        let is_active = active == Some(tab.id);
        let (bg, border, text_color) = tab_colors(exited, is_active);
        let (border, border_width) = if selection.is_selected(tab.id) {
            (palette::SELECTED_BORDER, SELECTED_BORDER_WIDTH)
        } else {
            (border, 1.0)
        };
        let ghost_rect = Rect {
            x: ghost.screen_x,
            y: tab.rect.y,
            width: tab.rect.width,
            height: tab.rect.height,
        };
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: ghost_rect,
            radius: 6.0,
            color: bg,
            border_color: border,
            border_width,
        }));
        let dot_reserve = if tab.indicator.is_some() {
            INDICATOR_DOT_SIZE + style.internal_gap
        } else {
            0.0
        };
        let label_y = ghost_rect.y + (ghost_rect.height - style.font_size) / 2.0;
        out.push(Primitive::Text(TextRun {
            origin: (ghost_rect.x + style.padding_left + dot_reserve, label_y),
            text: tab.label.clone(),
            font: LABEL_FONT,
            size_px: style.font_size,
            color: text_color,
        }));
    }

    // Espec §2.19.1: "o fantasma é a pílula sozinha" -- reaproveita a
    // geometria da pílula que `layout` já calculou pro grupo arrastado
    // (presente ali mesmo sendo pulado no laço principal acima, que só
    // filtra o desenho, não a busca) e desloca pelo `dx` que leva do X que
    // o layout deu a ela até o X do fantasma -- o mesmo mecanismo que já
    // move a pílula normal pela rolagem/animação, então funciona igual
    // esteja `layout` refletindo a posição antiga ou o preview de destino.
    if let Some(ghost) = group_drag
        && let Some(group) = layout.groups.iter().find(|g| g.id == ghost.group)
        && let Some(pill) = &group.pill
    {
        let color = workspace
            .group(ghost.group)
            .and_then(|g| g.color())
            .map(palette::group_color)
            .unwrap_or(palette::UNGROUPED_UNDERLINE);
        let is_collapsed = workspace
            .group(ghost.group)
            .is_some_and(|g| g.is_collapsed());
        let ghost_dx = ghost.screen_x - pill.rect.x;
        paint_group_pill(
            pill,
            color,
            is_collapsed,
            None,
            style.pill_font_size,
            ghost_dx,
            measurer,
            &mut out,
        );
    }

    out
}

fn tab_colors(exited: bool, is_active: bool) -> (Color, Color, Color) {
    if exited {
        (
            palette::TAB_INACTIVE_BACKGROUND,
            palette::TAB_INACTIVE_BORDER,
            palette::TAB_EXITED_TEXT,
        )
    } else if is_active {
        (
            palette::TAB_ACTIVE_BACKGROUND,
            palette::TAB_ACTIVE_BORDER,
            palette::TAB_ACTIVE_TEXT,
        )
    } else {
        (
            palette::TAB_INACTIVE_BACKGROUND,
            palette::TAB_INACTIVE_BORDER,
            palette::TAB_INACTIVE_TEXT,
        )
    }
}

fn shift(rect: Rect, dx: f32) -> Rect {
    Rect {
        x: rect.x + dx,
        ..rect
    }
}

fn with_alpha(color: Color, alpha: f64) -> Color {
    Color { a: alpha, ..color }
}

/// Multiplica o alfa que a cor já tem por `mult` -- diferente de
/// `with_alpha`, que substitui. É o que faz a aba esmaecer em cima do
/// alfa `.85` que `TAB_ACTIVE_BACKGROUND`/`TAB_INACTIVE_BACKGROUND` já
/// carregam (ADR-0022, fade de entrada/saída do colapso), sem perder o
/// "indício da cápsula" que esse `.85` existe pra deixar passar.
fn scale_alpha(color: Color, mult: f32) -> Color {
    Color {
        a: color.a * mult as f64,
        ..color
    }
}

/// Pílula de grupo (espec §2.4): fundo/borda, swatch, nome, contador e
/// caret, na cor do grupo já resolvida por quem chama (`group_color` em
/// [`paint`]). Contador desenhado sempre, colapsado ou não (§2.4: "sempre
/// desenhado, conteúdo idêntico").
#[allow(clippy::too_many_arguments)]
fn paint_group_pill(
    pill: &GroupPillRect,
    color: Color,
    is_collapsed: bool,
    live_name: Option<&str>,
    name_font_size: f32,
    dx: f32,
    measurer: &mut porecatu_render::TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: shift(pill.rect, dx),
        radius: PILL_CORNER_RADIUS,
        color: palette::PILL_BACKGROUND,
        border_color: palette::PILL_BORDER,
        border_width: PILL_BORDER_WIDTH,
    }));
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: shift(pill.swatch, dx),
        radius: PILL_SWATCH_CORNER_RADIUS,
        color,
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));
    if let Some(indicator) = pill.aggregate_indicator {
        let dot_color = match indicator {
            Indicator::Activity => palette::ACTIVITY_INDICATOR,
            Indicator::Bell => palette::BELL_INDICATOR,
        };
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: shift(
                Rect {
                    x: pill.aggregate_indicator_origin.0,
                    y: pill.aggregate_indicator_origin.1,
                    width: INDICATOR_DOT_SIZE,
                    height: INDICATOR_DOT_SIZE,
                },
                dx,
            ),
            radius: INDICATOR_DOT_SIZE / 2.0,
            color: dot_color,
            border_color: palette::TRANSPARENT,
            border_width: 0.0,
        }));
    }
    let name_text = match live_name {
        // Cap aproximado: o espaço que o nome já ocupava no layout
        // committed (`name_origin` até `count_rect`) -- não recalcula o
        // orçamento exato do indicador agregado (nota do módulo,
        // simplificação enquanto o editor está aberto).
        Some(buffer) => {
            let cap = (pill.count_rect.x - pill.name_origin.0).max(0.0);
            let (truncated, _) = measurer.truncate(buffer, PILL_NAME_FONT, name_font_size, cap);
            truncated
        }
        None => pill.name.clone(),
    };
    out.push(Primitive::Text(TextRun {
        origin: (pill.name_origin.0 + dx, pill.name_origin.1),
        text: name_text,
        font: PILL_NAME_FONT,
        size_px: name_font_size,
        color: palette::PILL_TEXT,
    }));
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: shift(pill.count_rect, dx),
        radius: PILL_COUNT_CORNER_RADIUS,
        color: palette::PILL_COUNT_BACKGROUND,
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));
    let count_rect = shift(pill.count_rect, dx);
    let count_width =
        measurer.measure_width(&pill.count_text, PILL_COUNT_FONT, PILL_COUNT_FONT_SIZE);
    out.push(Primitive::Text(TextRun {
        origin: (
            count_rect.x + (count_rect.width - count_width) / 2.0,
            count_rect.y + (count_rect.height - PILL_COUNT_FONT_SIZE) / 2.0,
        ),
        text: pill.count_text.clone(),
        font: PILL_COUNT_FONT,
        size_px: PILL_COUNT_FONT_SIZE,
        color: palette::PILL_COUNT_TEXT,
    }));
    let caret_glyph = if is_collapsed {
        PILL_CARET_COLLAPSED
    } else {
        PILL_CARET_EXPANDED
    };
    out.push(centered_glyph(
        caret_glyph,
        shift(pill.caret_rect, dx),
        PILL_CARET_ICON_SIZE,
        palette::PILL_CARET,
    ));
}

/// Indicador de abas fora da vista (espec §2.18, RF-1.19): chevron + a
/// mesma pílula de contagem da §2.4, ancorado por dentro da ponta da
/// trilha, fora do recorte de rolagem.
fn paint_overflow_pill(
    side: OverflowSide,
    count: usize,
    bar_width: f32,
    bar_height: f32,
    measurer: &mut porecatu_render::TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let rect = tab_bar::overflow_pill_rect(side, bar_width, bar_height);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: OVERFLOW_COUNT_RADIUS,
        color: palette::OVERFLOW_COUNT_BACKGROUND,
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));

    let chevron = match side {
        OverflowSide::Left => icon::CHEVRON_LEFT,
        OverflowSide::Right => icon::CHEVRON_RIGHT,
    };
    let count_text = count.to_string();
    // Largura do desenho, não o avanço de 1 em: o chevron preenche um
    // terço da em, e reservar a em inteira estouraria a pílula de 34px.
    let chevron_width = chevron.ink_width(OVERFLOW_CHEVRON_SIZE);
    // Espec §2.18: "contagem em mono 10px" -- a mesma face do contador da
    // pílula (§2.4), nunca a de ícones, que só tem ícone: dígito pedido a
    // ela não desenha nada.
    let count_width =
        measurer.measure_width(&count_text, PILL_COUNT_FONT, OVERFLOW_COUNT_FONT_SIZE);
    let content_width = chevron_width + OVERFLOW_INNER_GAP + count_width;
    let start_x = rect.x + (rect.width - content_width) / 2.0;
    let mid_y = rect.y + rect.height / 2.0;

    out.push(centered_glyph(
        chevron,
        Rect {
            x: start_x,
            y: rect.y,
            width: chevron_width,
            height: rect.height,
        },
        OVERFLOW_CHEVRON_SIZE,
        palette::NEW_TAB_ICON,
    ));
    out.push(Primitive::Text(TextRun {
        origin: (
            start_x + chevron_width + OVERFLOW_INNER_GAP,
            mid_y - OVERFLOW_COUNT_FONT_SIZE / 2.0,
        ),
        text: count_text,
        font: PILL_COUNT_FONT,
        size_px: OVERFLOW_COUNT_FONT_SIZE,
        color: palette::OVERFLOW_COUNT_TEXT,
    }));
}

/// Campo de rename (espec §2.5): substitui o rótulo no lugar, largura
/// `min(120, largura disponível)`. Texto rola dentro do campo mantendo o
/// caret (sempre no fim do buffer nesta etapa -- sem edição no meio da
/// string) visível: quando o texto não cabe, a origem desliza para a
/// esquerda, e um `PushClip`/`PopClip` contém o transbordo.
fn paint_rename_field(
    tab_rect: Rect,
    style: &TabBarStyle,
    buffer: &str,
    measurer: &mut porecatu_render::TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let available_width = (tab_rect.width - style.padding_left - style.padding_right).max(0.0);
    let field_width = RENAME_FIELD_MAX_WIDTH.min(available_width);
    let field_rect = Rect {
        x: tab_rect.x + style.padding_left,
        y: tab_rect.y + (tab_rect.height - RENAME_FIELD_HEIGHT) / 2.0,
        width: field_width,
        height: RENAME_FIELD_HEIGHT,
    };
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: field_rect,
        radius: 4.0,
        color: palette::RENAME_BACKGROUND,
        border_color: palette::RENAME_BORDER,
        border_width: 1.0,
    }));

    let text_area = (field_width - RENAME_PADDING_X * 2.0).max(0.0);
    let text_width = measurer.measure_width(buffer, LABEL_FONT, RENAME_FONT_SIZE);
    let text_x = if text_width > text_area {
        field_rect.x + RENAME_PADDING_X - (text_width - text_area)
    } else {
        field_rect.x + RENAME_PADDING_X
    };
    let text_y = field_rect.y + (RENAME_FIELD_HEIGHT - RENAME_FONT_SIZE) / 2.0;

    out.push(Primitive::PushClip(field_rect));
    out.push(Primitive::Text(TextRun {
        origin: (text_x, text_y),
        text: buffer.to_string(),
        font: LABEL_FONT,
        size_px: RENAME_FONT_SIZE,
        color: palette::RENAME_TEXT,
    }));
    let caret_x = (text_x + text_width).min(field_rect.x + field_width - 1.0);
    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: caret_x,
            y: field_rect.y + 3.0,
            width: 1.0,
            height: RENAME_FIELD_HEIGHT - 6.0,
        },
        color: palette::RENAME_TEXT,
    }));
    out.push(Primitive::PopClip);
}

/// Centraliza um ícone dentro de `rect`. `size_px` é a **em**, não o
/// tamanho do desenho -- ver `porecatu_render::icon`, que também explica
/// por que isto não mede texto.
pub(crate) fn centered_glyph(
    what: icon::Icon,
    rect: Rect,
    size_px: f32,
    color: Color,
) -> Primitive {
    Primitive::Text(TextRun {
        origin: what.centered_origin(rect, size_px),
        text: what.glyph.to_string(),
        font: ICON_FONT,
        size_px,
        color,
    })
}

/// Altura total da barra (espec §2.5/§2.3): abas + a folga do wrapper
/// acima e abaixo. Usado por `lib.rs` para deslocar a grade do terminal e
/// converter posição de clique.
pub fn bar_height(style: &TabBarStyle) -> f32 {
    style.tab_height + style.wrapper_padding * 2.0 + style.trilha_padding * 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use porecatu_render::TextMeasurer;

    /// Regressão: `paint` tinha uma cópia local da fórmula da altura da
    /// barra, que ficou para trás quando `trilha_padding` entrou na conta.
    /// O fundo e o recorte da trilha paravam 12px acima do fim da barra --
    /// o respiro de baixo não podia aparecer, porque o clip cortava as
    /// abas antes dele. Este teste amarra o que `paint` desenha ao que
    /// `bar_height` promete, que é o valor que `lib.rs` usa para deslocar
    /// a grade e converter clique.
    #[test]
    fn painted_background_and_clip_span_the_whole_bar() {
        let style = TabBarStyle::DEFAULT;
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = TextMeasurer::new();
        let bar_width = 800.0;
        let layout = tab_bar::fit_width(&ws, &style, bar_width, &mut m);
        let overflow = Overflow {
            scroll_offset: 0.0,
            hidden_left: 0,
            hidden_right: 0,
        };

        let out = paint(
            &layout,
            &ws,
            ws.active_tab(),
            &RenameState::Idle,
            &Selection::default(),
            None,
            &style,
            bar_width,
            overflow,
            None,
            None,
            None,
            &AnimationClock::default(),
            Instant::now(),
            &mut m,
        );

        let expected = bar_height(&style);
        let background = out
            .iter()
            .find_map(|p| match p {
                Primitive::Quad(q) if q.color == palette::BAR_BACKGROUND => Some(q.rect),
                _ => None,
            })
            .expect("fundo da barra");
        assert_eq!(background.height, expected, "fundo mais curto que a barra");

        let clip = out
            .iter()
            .find_map(|p| match p {
                Primitive::PushClip(rect) => Some(*rect),
                _ => None,
            })
            .expect("recorte da trilha");
        assert_eq!(clip.height, expected, "recorte mais curto que a barra");

        // E o conteúdo tem de caber dentro dele com o respiro de baixo
        // sobrando -- senão o clip corta o padding em vez de mostrá-lo.
        let tab = &layout.groups[0].tabs[0];
        assert!(
            tab.rect.y + tab.rect.height + style.trilha_padding <= expected,
            "aba sem respiro até o fim da barra"
        );
    }
}
