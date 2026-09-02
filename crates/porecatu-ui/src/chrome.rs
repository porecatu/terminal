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
//! expandir, não só deslizar os vizinhos. A cápsula é desenhada em
//! qualquer estado, colapsado inclusive (pedido do usuário), o que
//! dispensa o caso especial que a segurava até o fim da animação.
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
    self, GroupPillRect, INDICATOR_DOT_SIZE, Indicator, Overflow, OverflowSide, PILL_NAME_FONT,
    TabBarLayout, TabBarStyle,
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
// A engrenagem preenche 0.84 em contra 0.68 do "+", então a mesma em a
// desenharia bem maior que os outros ícones. Reduzida para o desenho
// bater com o do "+" -- ver `porecatu_render::icon`.
const SETTINGS_ICON_SIZE: f32 = ICON_EM_SIZE * 0.8;
// Menor que o "+"/configurações -- convenção Windows: glyphs finos de
// minimizar/maximizar/fechar (ADR-0027).
const WINDOW_BUTTON_ICON_SIZE: f32 = ICON_EM_SIZE * 0.7;
// Não há sublinhado de grupo na base da aba, e não há como ligá-lo. Ele
// nasceu para dizer a que grupo a aba pertence quando a pílula sai da
// vista por rolagem; desde que a cápsula passou a ser pintada com a cor
// cheia (F3 etapa 6), a cor do grupo já está atrás da aba inteira e o
// traço virou ruído. O indicador de grupo é a pílula mais a cápsula, e a
// chave `indicator_style` deixou de existir junto com os estilos
// `left-bar` e `outline` -- ADR-0032, seção 4.4 da especificação.
// Borda da aba em todo estado. A espec. §2.5 desenha 1px; contra a cápsula
// de cor cheia (F3 etapa 6) 1px de `#22262e` não se lê, e o pedido do
// usuário foi "coloca um border nas abas". 2px é a espessura que o próprio
// arquivo de exemplo já usa para linha de chrome
// (`active_border_width`/`selected_border_width`) -- não um número novo.
// Cada estado mantém a cor dele, que é o que continua separando ativa de
// inativa.
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
// Efeito de vidro (pedido do usuário, fora da espec.: "muito chapada"):
// deixa passar um traço do que está atrás -- `BAR_BACKGROUND` no espaço
// entre pílula/abas e a borda da cápsula, a própria cápsula onde a pílula
// fica por cima. Sem primitiva de blur em `porecatu-render` (nota do
// módulo) -- não há como turvar o que passa por trás, só deixar passar
// menos dele; ainda assim já lê como painel translúcido, não como o
// tingimento de .07 que a espec original pedia e o usuário rejeitou na F3
// (aquele desaparecia contra o fundo da aba; este fica atrás dela, contra
// `BAR_BACKGROUND`, sólido e escuro, então não some do mesmo jeito). Valor
// de trabalho -- ajustar se ficar fraco ou forte demais em tela.
const GROUP_CAPSULE_FILL_STRENGTH: f64 = 0.85;
// Borda clara e translúcida (`GLASS_BORDER`) no lugar do tom neutro escuro
// de antes -- é o "rim light" que lê como borda de vidro; um traço escuro
// contra uma cápsula agora semitransparente ficava opaco demais e quebrava
// o efeito. Pedido do usuário, mesmo valor de trabalho.
const CAPSULE_BORDER_WIDTH: f32 = 1.0;

// Sombra da cápsula de grupo e da aba solta (pedido do usuário).
// `porecatu-render` não tem primitiva de sombra (nota do módulo, espec
// §4.4) -- aproximada aqui com `RoundedQuad`s pretos semitransparentes
// empilhados, crescendo de raio e caindo de alfa, a mesma técnica de
// "drop shadow em camadas" usada fora de um passo de blur de verdade.
// Suficiente para o respiro visual que o pedido descreve; não é o
// `box-shadow` de popover da espec (`0 18px 44px rgba(0,0,0,.55)`), que
// precisaria de blur real para não aliasear numa mancha desse tamanho.
pub(crate) const SHADOW_LAYERS: [(f32, f32, f64); 3] = [
    // (spread, offset_y, alpha)
    (1.0, 1.0, 0.16),
    (2.5, 2.0, 0.10),
    (4.5, 3.0, 0.06),
];

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
// Espec §2.4 pede borda 1px (`label_border`); removida a pedido do
// usuário na F3 -- a pílula já era a cor cheia do grupo, e a borda neutra
// por cima virava um contorno cinza sem função contra ela. **Volta** com o
// efeito de vidro (pedido do usuário, fora da espec.): não é mais neutra,
// é `GLASS_BORDER` -- o mesmo rim translúcido da cápsula, propósito
// diferente do que foi removido (ali era contorno sem função; aqui é a
// borda que lê como vidro). Preenchimento também ganha leve
// transparência, mesma lógica de `GROUP_CAPSULE_FILL_STRENGTH` -- a
// pílula fica por cima da cápsula (mesmo tom), então o que passa por trás
// dela é a própria cápsula, não `BAR_BACKGROUND`: duas camadas
// translúcidas empilhadas, a leitura de "vidro sobre vidro".
const PILL_GLASS_FILL_STRENGTH: f64 = 0.92;
const PILL_BORDER_WIDTH: f32 = 1.0;
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
    is_macos: bool,
    is_maximized: bool,
    hover_window_button: Option<tab_bar::WindowButtonHit>,
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
    // Sem separador de 1px na base da barra (espec §2.2: "borda #23272f").
    // Pedido do usuário: o box arredondado do terminal (`paint.rs`) começa
    // colado em `bar_height` para ficar seamless contra a barra, e um
    // separador ali desenharia de volta a linha que a colagem existe pra
    // tirar. Divergência da espec, mesma classe das já registradas na
    // seção 4.4 dela.

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
        width: tab_bar::trilha_width(style, bar_width, is_macos),
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
        // Cor de grupo: `ungrouped_color` para run implícito (ADR-0006),
        // a cor do grupo para explícito. Mesma resolução para a cápsula, o
        // fundo da pílula e o realce de fronteira do arraste.
        let group_color = core_group
            .and_then(|g| g.color())
            .map(palette::group_color)
            .unwrap_or(palette::UNGROUPED_GROUP_COLOR);

        // Ajuste pedido pelo usuário (F3 etapa 6, fora da espec.): o grupo
        // é uma "cápsula" pintada com a cor cheia -- não o tingimento de
        // 7% da espec §2.3, que ficava quase invisível atrás do fundo
        // opaco das abas. `TAB_ACTIVE_BACKGROUND`/`TAB_INACTIVE_BACKGROUND`
        // (`palette.rs`) agora têm alfa .85 pra deixar passar um indício
        // dela por cima. Abas sem grupo (`pill == None`) nunca pintam
        // cápsula.
        //
        // **Colapsado também pinta** (pedido do usuário), contra o
        // "colapsado fica transparente" do RF-4.19: é a cápsula que
        // aparece em volta do conteúdo e diz de que cor o grupo é, e
        // fazê-la sumir no colapso tirava a única marca de cor do grupo
        // justo quando o nome dele é tudo o que resta na barra. Ela passa
        // a abraçar a pílula sozinha. De quebra, some o caso especial de
        // "continua desenhada durante a animação para não sumir na hora":
        // agora ela nunca some, então não há o que segurar.
        if group.pill.is_some() {
            push_shadow(&mut out, capsule_rect, WRAPPER_CORNER_RADIUS);
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: capsule_rect,
                radius: WRAPPER_CORNER_RADIUS,
                color: with_alpha(group_color, GROUP_CAPSULE_FILL_STRENGTH),
                border_color: palette::GLASS_BORDER,
                border_width: CAPSULE_BORDER_WIDTH,
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

            // Sombra só na aba solta (sem cápsula atrás) -- aba de dentro
            // de um grupo já leva o respiro da cápsula, e pedido do
            // usuário foi sombra na cápsula e na aba solta, não nas duas.
            if group.pill.is_none() {
                push_shadow(&mut out, tab_rect, 6.0);
            }
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
        // sem esticar, sem fade próprio.
        //
        // A cor do ícone depende do que está **atrás** dele, e é a única
        // coisa no chrome que decide cor assim. Num grupo explícito ele
        // cai sobre a cápsula pintada com a cor cheia, onde o claro
        // perde contraste; num run de abas soltas não há cápsula
        // (`pill == None`, mesma condição que decide pintá-la acima) e
        // ele fica sobre a barra escura, onde o escuro é que some. Ter um
        // tom só deixava o "+" preto no fundo preto sempre que a barra
        // não tinha grupo nenhum.
        if let Some(rect) = group.new_tab_button {
            let group_button = shift(rect, dx);
            let icon_color = if group.pill.is_some() {
                palette::GROUP_NEW_TAB_ICON
            } else {
                palette::NEW_TAB_ICON
            };
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
                icon_color,
            ));
        }
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

    // "+" de aba solta, ao fim da trilha e **fora** de qualquer wrapper.
    // Rola com o conteúdo, como tudo aqui dentro. Cor clara: ele fica
    // sobre o fundo da barra, nunca sobre a cápsula de um grupo -- e é
    // isso, à vista, que o distingue do "+" que cria dentro de um grupo.
    if let Some(rect) = layout.ungrouped_new_tab_button {
        let button = shift(rect, scroll_dx);
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

    out.push(Primitive::PopClip);

    // Zona fixa à direita da barra, fora do recorte da trilha -- não rola
    // com o conteúdo, ao contrário do botão por grupo pintado acima.
    //
    // Ela existia para o botão de nova aba **global**, que foi removido:
    // com um "+" por grupo, e todo run de abas soltas sendo um grupo
    // implícito, o global era um segundo botão para a mesma ação, a um
    // palmo do primeiro. O bloco fica, reservado para o que a barra
    // ganhar à direita daqui em diante, e por ora carrega o botão de
    // configurações -- **inerte de propósito** (`config` é F4): ele
    // desenha e não responde a clique nenhum, ver `handle_bar_click`.
    let settings = tab_bar::settings_button_rect(style, bar_width, bar_height, is_macos);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: settings,
        radius: 6.0,
        color: palette::TRANSPARENT,
        border_color: palette::NEW_TAB_BORDER,
        border_width: 1.0,
    }));
    out.push(centered_glyph(
        icon::SETTINGS,
        settings,
        SETTINGS_ICON_SIZE,
        palette::NEW_TAB_ICON,
    ));

    // Botões de janela (ADR-0027): minimizar/maximizar-restaurar/fechar,
    // colados na borda direita. Não existem no macOS -- lá é o semáforo
    // nativo (`left_inset` na trilha, não zona de botão aqui).
    if !is_macos {
        let buttons = [
            (0u8, tab_bar::WindowButtonHit::Minimize, icon::MINUS, false),
            (
                1u8,
                tab_bar::WindowButtonHit::MaximizeRestore,
                if is_maximized {
                    icon::RESTORE
                } else {
                    icon::MAXIMIZE
                },
                false,
            ),
            (2u8, tab_bar::WindowButtonHit::Close, icon::X, true),
        ];
        for (index, hit, glyph, is_close) in buttons {
            let rect = tab_bar::window_button_rect(index, bar_width, bar_height);
            let hovered = hover_window_button == Some(hit);
            let bg = match (is_close, hovered) {
                (true, true) => palette::WINDOW_CLOSE_HOVER_BG,
                (false, true) => palette::WINDOW_BUTTON_HOVER_BG,
                (_, false) => palette::TRANSPARENT,
            };
            out.push(Primitive::Quad(Quad { rect, color: bg }));
            let icon_color = if is_close && hovered {
                palette::WINDOW_CLOSE_HOVER_ICON
            } else {
                palette::NEW_TAB_ICON
            };
            out.push(centered_glyph(
                glyph,
                rect,
                WINDOW_BUTTON_ICON_SIZE,
                icon_color,
            ));
        }
    }

    // Pílulas de overflow (espec §2.18) ficam dentro da trilha rolável, não
    // da barra inteira -- senão a da direita cairia por cima da zona fixa
    // da direita.
    let trilha_width = tab_bar::trilha_width(style, bar_width, is_macos);
    if overflow.hidden_left > 0 {
        paint_overflow_pill(OverflowSide::Left, trilha_width, bar_height, &mut out);
    }
    if overflow.hidden_right > 0 {
        paint_overflow_pill(OverflowSide::Right, trilha_width, bar_height, &mut out);
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
            .unwrap_or(palette::UNGROUPED_GROUP_COLOR);
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

/// Empilha as camadas de `SHADOW_LAYERS` atrás de `rect` (raio `radius`).
/// Ver nota em `SHADOW_LAYERS`.
pub(crate) fn push_shadow(out: &mut Vec<Primitive>, rect: Rect, radius: f32) {
    for (spread, offset_y, alpha) in SHADOW_LAYERS {
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: Rect {
                x: rect.x - spread,
                y: rect.y - spread + offset_y,
                width: rect.width + spread * 2.0,
                height: rect.height + spread * 2.0,
            },
            radius: radius + spread,
            color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: alpha,
            },
            border_color: palette::TRANSPARENT,
            border_width: 0.0,
        }));
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

/// Pílula de grupo (espec §2.4, divergente): fundo, nome e caret, na cor
/// do grupo já resolvida por quem chama (`group_color` em [`paint`]). Sem
/// contador de abas -- pedido do usuário. Sem swatch tampouco -- também
/// pedido do usuário: a pílula inteira é pintada com `color`, no lugar do
/// pequeno quadrado que a marcava antes (o fundo era `palette::PILL_BACKGROUND`,
/// neutro). Texto e caret usam `palette::GROUP_NEW_TAB_ICON` -- o mesmo
/// escuro do "+" do grupo, pela mesma razão: sobre a cor cheia do grupo o
/// claro perde contraste.
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
        color: with_alpha(color, PILL_GLASS_FILL_STRENGTH),
        border_color: palette::GLASS_BORDER,
        border_width: PILL_BORDER_WIDTH,
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
        // committed (`name_origin` até `caret_rect`) -- não recalcula o
        // orçamento exato do indicador agregado (nota do módulo,
        // simplificação enquanto o editor está aberto).
        Some(buffer) => {
            let cap = (pill.caret_rect.x - pill.name_origin.0).max(0.0);
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
        color: palette::GROUP_NEW_TAB_ICON,
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
        palette::GROUP_NEW_TAB_ICON,
    ));
}

/// Indicador de abas fora da vista (espec §2.18, RF-1.19): círculo com
/// chevron, ancorado por dentro da ponta da trilha, fora do recorte de
/// rolagem. Divergência da espec (que pedia cápsula com contagem, §2.18),
/// a pedido do usuário -- registrar na seção 4.4.
fn paint_overflow_pill(
    side: OverflowSide,
    bar_width: f32,
    bar_height: f32,
    out: &mut Vec<Primitive>,
) {
    let rect = tab_bar::overflow_pill_rect(side, bar_width, bar_height);
    // Pedido do usuário: círculo (raio = metade da largura = metade da
    // altura), só o chevron -- a contagem saiu.
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: rect.width / 2.0,
        color: palette::OVERFLOW_COUNT_BACKGROUND,
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));

    let chevron = match side {
        OverflowSide::Left => icon::CHEVRON_LEFT,
        OverflowSide::Right => icon::CHEVRON_RIGHT,
    };
    out.push(centered_glyph(
        chevron,
        rect,
        OVERFLOW_CHEVRON_SIZE,
        palette::NEW_TAB_ICON,
    ));
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
    use porecatu_core::GroupColor;
    use porecatu_render::TextMeasurer;

    /// Pedido do usuário, contra o "colapsado fica transparente" do
    /// RF-4.19: a cápsula é o que diz de que cor o grupo é, e sumir com
    /// ela no colapso tirava a única marca de cor justo quando o nome do
    /// grupo é tudo o que resta na barra.
    #[test]
    fn collapsed_group_still_paints_its_colored_capsule() {
        let style = TabBarStyle::DEFAULT;
        let mut m = TextMeasurer::new();
        let bar_width = 800.0;

        let paint_capsules = |ws: &Workspace, m: &mut TextMeasurer| {
            let layout = tab_bar::fit_width(ws, &style, bar_width, m, false);
            let out = paint(
                &layout,
                ws,
                ws.active_tab(),
                &RenameState::Idle,
                &Selection::default(),
                None,
                &style,
                bar_width,
                Overflow {
                    scroll_offset: 0.0,
                    hidden_left: 0,
                    hidden_right: 0,
                },
                None,
                None,
                None,
                &AnimationClock::default(),
                Instant::now(),
                m,
                false,
                false,
                None,
            );
            // `with_alpha` -- efeito de vidro (`GROUP_CAPSULE_FILL_STRENGTH`)
            // não pinta mais a cor cheia do grupo, e sim ela com o alfa da
            // cápsula.
            let cor = with_alpha(
                palette::group_color(GroupColor::Cyan),
                GROUP_CAPSULE_FILL_STRENGTH,
            );
            out.iter()
                .filter(|p| match p {
                    Primitive::RoundedQuad(q) => {
                        q.radius == WRAPPER_CORNER_RADIUS && q.color == cor
                    }
                    _ => false,
                })
                .count()
        };

        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a], "col", GroupColor::Cyan).unwrap();
        assert_eq!(paint_capsules(&ws, &mut m), 1, "expandido");

        ws.collapse_group(group, true);
        assert_eq!(paint_capsules(&ws, &mut m), 1, "colapsado");
    }

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
        let layout = tab_bar::fit_width(&ws, &style, bar_width, &mut m, false);
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
            false,
            false,
            None,
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
