// SPDX-License-Identifier: GPL-3.0-or-later

//! Traduz `tab_bar::TabBarLayout` (mais estado efêmero que o layout puro
//! não conhece: aba ativa, aba `Exited`, edição de rename em andamento,
//! rolagem e arraste desde a Etapa 5, seleção múltipla desde a F3 etapa 2)
//! em `Primitive`s da camada `Chrome` (ADR-0018). Cores e dimensões: espec.
//! visual §1.2, §1.3, §2.3, §2.4, §2.5, §2.6, §2.17, §2.18, §2.19, como
//! campos de `palette::ResolvedPalette`/`tab_bar::TabBarStyle`, resolvidos
//! de `Config` (F4 etapa 2) e passados por `paint` -- mesmo padrão de
//! `paint.rs` para a grade (que continua com `[terminal.*]` fixo até a
//! etapa 3).
//!
//! **Hover por brilho (F4 etapa 6, ADR-0032 §2, espec §1.10).** `paint`
//! recebe `hover: Option<tab_bar::TabBarHit>`, calculado fresco a cada
//! frame em `lib.rs` a partir do cursor -- mesmo padrão de
//! `hover_window_button` -- e usado aqui só pra multiplicar (`brighten`)
//! o fundo da aba (`1.18`) ou da pílula (`1.25`) sob o cursor, clampando
//! em 1.0. Sem primitiva de filtro em `porecatu-render`: é CPU, de
//! propósito (ADR-0032 §2 proíbe criar uma só pra isto). O fantasma de
//! arraste usa a mesma técnica, sempre ligada (não condicionada a hover:
//! o fantasma segue o cursor, não há "estar em cima dele").
//!
//! Desde a F3 etapa 3, pílula e wrapper tingido (§2.3/§2.4) também entram
//! aqui -- mas só a geometria e a cor que já existem em `porecatu-core`
//! desde a etapa 1 (`Group::color`/`is_collapsed`). O caret também não gira:
//! `RoundedQuad`/`TextRun` não têm transform, então a troca de glyph
//! (`▶`/`▼`) é o equivalente estático -- isso não muda na F4 etapa 6: só
//! brilho e sombra são as duas mudanças aprovadas (ADR-0032 §2), rotação
//! continua fora de escopo.
//!
//! Desde a F3 etapa 4, grupo colapsado não desenha abas (o layout já não as
//! gera, ver `tab_bar.rs`) e a pílula ganha um ponto por aba (RF-2.16), o da
//! aba com atividade pintado aqui com as cores de `Indicator` da seção 2.17. Clique na
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
//! preview não gerar `TabRect` nenhum pra essa aba, e o fantasma precisa do
//! conteúdo mesmo assim.
//!
//! Colapsar/expandir também esmaece as abas do próprio grupo em vez de
//! picá-las (espec §2.4: "o que anima de fato é o resto do colapso: as
//! abas desaparecendo da trilha"). Abas que sumiram do `layout` corrente
//! mas existiam em `old_layout` (`AnimationClock::old_tabs`) continuam
//! desenhadas na posição antiga, opacidade caindo; as que são novas ali
//! (`AnimationClock::had_tab` devolve falso) entram com opacidade
//! subindo, junto com o progresso da reflui do próprio wrapper delas.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use porecatu_core::{GroupId, TabId, Workspace};
use porecatu_render::{
    Color, FontFace, ImageId, Primitive, Quad, Rect, RoundedQuad, SansWeight, TextRun, icon,
};

use crate::animation::AnimationClock;
use crate::group_editor::GroupEditor;
use crate::palette::{self, ResolvedPalette};
use crate::pinned_icon::{self, IconTexture};
use crate::rename::RenameState;
use crate::selection::Selection;
use crate::tab_bar::{
    self, GroupPillRect, Indicator, Overflow, PILL_NAME_FONT, TabBarLayout, TabBarStyle,
};

/// Fonte dos ícones da barra (fechar, nova aba, chevron): a face Lucide
/// embutida, não a IBM Plex Sans do rótulo. Os glyphs Unicode que a espec.
/// usa para desenhar esses ícones no papel (✕ U+2715, ▶ U+25B6, ▼ U+25BC)
/// **não existem** na Plex Sans, e o `fontdb` do projeto não carrega fonte
/// do sistema (ADR-0016): sem face própria eles não desenhavam nada. Ver
/// `porecatu_render::icon`.
pub(crate) const ICON_FONT: FontFace = FontFace::Icon;
// `pub(crate)`: `lib.rs::handle_bar_click` mede o mesmo texto com a mesma
// face pra achar o índice de caractere sob o clique no campo de rename
// (ADR-0035) -- tem que ser a mesma constante que pinta.
pub(crate) const LABEL_FONT: FontFace = FontFace::System {
    weight: SansWeight::Regular,
};

// Não há sublinhado de grupo na base da aba, e não há como ligá-lo. Ele
// nasceu para dizer a que grupo a aba pertence quando a pílula sai da
// vista por rolagem; desde que a cápsula passou a ser pintada com a cor
// cheia (F3 etapa 6), a cor do grupo já está atrás da aba inteira e o
// traço virou ruído. O indicador de grupo é a pílula mais a cápsula, e a
// chave `indicator_style` deixou de existir junto com os estilos
// `left-bar` e `outline` -- ADR-0032, seção 4.4 da especificação.

// Realce de fronteira do arraste de aba (espec §2.19, ADR-0021 §4):
// "borda 1px na cor do grupo com alfa .45" -- sem chave própria no TOML
// (o tingimento em si é `[appearance.groups] badge_tint_strength`, ver
// `TabBarStyle::drag_highlight_tint_strength`).
const DRAG_HIGHLIGHT_BORDER_ALPHA: f64 = 0.45;
const DRAG_HIGHLIGHT_BORDER_WIDTH: f32 = 1.0;

/// RF-3.9 (ADR-0037 §5): rótulo de aba `NotStarted` esmaecido. Mesmo `.45`
/// de cima -- **não é valor novo**, é o já usado no realce de borda do
/// arraste (espec. §1.8, "Tingimentos"); só o consumidor é outro.
const NOT_STARTED_LABEL_ALPHA: f64 = DRAG_HIGHLIGHT_BORDER_ALPHA;

// Espec §2.4, item 4: "▶ 8px, rotate(0deg) colapsado, rotate(90deg)
// expandido". Sem primitiva de rotação (ver nota do módulo) -- a troca de
// ícone é o equivalente estático.

/// Sombra da cápsula de grupo e da aba solta (pedido do usuário,
/// `[appearance.groups] shadow` liga/desliga -- ver `TabBarStyle::
/// group_shadow_enabled`). `porecatu-render` não tem primitiva de sombra
/// (nota do módulo, espec §4.4) -- aproximada aqui com `RoundedQuad`s
/// pretos semitransparentes empilhados, crescendo de raio e caindo de
/// alfa, a mesma técnica de "drop shadow em camadas" usada fora de um
/// passo de blur de verdade. Suficiente para o respiro visual que o
/// pedido descreve; não é o `box-shadow` de popover da espec
/// (`0 18px 44px rgba(0,0,0,.55)`), que precisaria de blur real para não
/// aliasear numa mancha desse tamanho. Sem chave individual por camada no
/// TOML -- é a técnica documentada na espec §1.7, não um valor de design
/// por si.
pub(crate) const SHADOW_LAYERS: [(f32, f32, f64); 3] = [
    // (spread, offset_y, alpha)
    (1.0, 1.0, 0.16),
    (2.5, 2.0, 0.10),
    (4.5, 3.0, 0.06),
];

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
    /// De onde o fantasma tira a forma e o conteúdo (ver [`GhostSource`]).
    pub source: GhostSource,
    /// The boxes of the pinned zone as the drag previews them, in slot order
    /// (`tab_bar::pinned_preview_order`, ADR-0087 §5): the gap that opens for
    /// an incoming tab, the hole of a box that leaves. `dragged` is skipped
    /// when painting.
    pub pinned_order: Vec<TabId>,
    /// The zone highlight (ADR-0087 §5), in screen coordinates: `Some` only
    /// while the target is the zone.
    pub zone_highlight: Option<Rect>,
}

/// O fantasma mantém a forma de **origem** durante todo o gesto (ADR-0087
/// §5): trocá-la quando o centro cruza a fronteira faria o fantasma pular
/// sob o cursor.
#[derive(Debug, Clone, PartialEq)]
pub enum GhostSource {
    /// Arrastada da trilha: o fantasma de sempre. Retângulo/rótulo/indicador
    /// de `base_layout` (`lib.rs`), de antes de qualquer preview -- garante
    /// conteúdo mesmo quando o alvo do arraste (F3 etapa 6) é um grupo
    /// colapsado, cujo preview não gera `TabRect` nenhum pra essa aba (§2.4:
    /// "abas somem da barra").
    Track(tab_bar::TabRect),
    /// Arrastada da zona de fixadas: a caixa quadrada, pintada como a
    /// fixada, mesmo sobre a trilha.
    Pinned,
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
/// fechar), o botão de nova aba e o fantasma de arraste (espec §2.19), se
/// algum estiver em andamento.
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
    pal: &ResolvedPalette,
    term_pal: &palette::ResolvedTermPalette,
    bar_width: f32,
    overflow: Overflow,
    drag: Option<DragGhost>,
    group_drag: Option<GroupDragGhost>,
    drag_highlight: Option<(GroupId, Rect)>,
    animations: &AnimationClock,
    now: Instant,
    measurer: &mut porecatu_render::TextMeasurer,
    is_macos: bool,
    // ADR-0050: `true` também em tela cheia, não só maximizada -- as duas
    // mostram o ícone de restaurar (chamar isto `is_maximized` seria
    // enganoso: o botão nunca "maximiza-por-cima-do-fullscreen").
    show_restore_icon: bool,
    hover_window_button: Option<tab_bar::WindowButtonHit>,
    // ADR-0066: botão de janela armado pelo press, ainda sem release.
    pressed_window_button: Option<tab_bar::WindowButtonHit>,
    // Hover por brilho (F4 etapa 6, ADR-0032 §2, espec §1.10) -- `None`
    // durante arraste/rename/qualquer modo de captura, mesma regra do
    // resto do hover da barra: calculado fresco por frame a partir do
    // cursor, nunca por dentro de um modo que já consome o clique.
    hover: Option<tab_bar::TabBarHit>,
    // ADR-0055 §1: hover dos botões da zona fixa (sessões, configurações),
    // mesmo padrão de `hover_window_button` acima -- fora da `TabBarLayout`
    // (não rola com a trilha), por isso parâmetros à parte, não uma
    // variante de `TabBarHit`.
    hover_zone_button: Option<tab_bar::ZoneButtonHit>,
    // Botão da zona fixa armado pelo press, ainda sem release.
    pressed_zone_button: Option<tab_bar::ZoneButtonHit>,
    // "+" armado pelo press, ainda sem release: aparece pressionado enquanto
    // o cursor segue sobre ele (o release é quem abre a aba).
    pressed_bar_button: Option<tab_bar::TabBarHit>,
    // ADR-0062 §3: a imagem de fundo da janela está **exibida** (pronta e com
    // opacidade > 0). A barra deixa de pintar o `Quad` opaco de
    // `bar_background` sobre ela; sem imagem, o `Quad` continua e o binário é
    // o de hoje. Parâmetro explícito, nunca estado lido daqui.
    window_image_shown: bool,
    // Textura do ícone do app no botão de menu (zona fixa à esquerda). `None`
    // enquanto não subiu para a GPU, ou se a decodificação falhou: o botão
    // pinta igual, só sem o ícone.
    app_icon: Option<ImageId>,
    // ADR-0088 §5: the own texture of each pinned tab that resolved one (by
    // `TabId`, of this window). A pinned tab without an entry -- not yet
    // resolved, or resolved to the app icon -- draws `app_icon`.
    pinned_icons: &HashMap<TabId, IconTexture>,
) -> Vec<Primitive> {
    // Nunca recalcular esta fórmula aqui: `bar_height` é a mesma altura
    // que `lib.rs` usa para deslocar a grade e converter clique. Uma cópia
    // local dela ficou para trás quando `trilha_padding` entrou na conta,
    // e o efeito foi o fundo da barra e o recorte da trilha pararem 12px
    // acima do fim dela -- o respiro de baixo simplesmente não podia
    // aparecer, porque o clip cortava as abas antes.
    let bar_height = bar_height(style);
    let mut out = Vec::new();

    // Tamanhos de **em**, não de desenho: o Lucide preenche ~0.6 em, então a
    // em tem de ser cerca do dobro do número da especificação para o
    // desenho sair no tamanho que ela pede -- e para o traço (`2/24` da em)
    // render sólido em vez de esmaecer contra o fundo. Ver
    // `porecatu_render::icon`. `[appearance.tabs] icon_em_size`.
    let close_icon_size = style.icon_em_size; // espec §2.5: "✕ 10px" de desenho
    let new_tab_icon_size = style.icon_em_size; // espec §2.6: "+ 15px" de desenho
    // A engrenagem preenche 0.84 em contra 0.68 do "+", então a mesma em a
    // desenharia bem maior que os outros ícones. Reduzida para o desenho
    // bater com o do "+" -- ver `porecatu_render::icon`. Sem chave própria.
    let settings_icon_size = style.icon_em_size * 0.8;

    // Com a imagem da janela exibida o fundo opaco cobriria a imagem; o
    // `clear` do passe tem a mesma cor, então nada fora da imagem muda
    // (ADR-0062 §3).
    if !window_image_shown {
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: bar_width,
                height: bar_height,
            },
            color: pal.bar_background,
        }));
    }
    // Sem separador de 1px na base da barra (espec §2.2: "borda #23272f").
    // Pedido do usuário: o box arredondado do terminal (`paint.rs`) começa
    // colado em `bar_height` para ficar seamless contra a barra, e um
    // separador ali desenharia de volta a linha que a colagem existe pra
    // tirar. Divergência da espec, mesma classe das já registradas na
    // seção 4.4 dela.

    // Botão de menu do app: zona fixa à esquerda, antes da trilha, fora do
    // recorte e do scroll. Sem ação por ora -- só hover e pressed, como o
    // resto da barra (o pressed é o da aba ativa apertada, sem o brilho do
    // hover). `show_app_menu_button = false`: nada é desenhado.
    if style.show_app_menu_button {
        paint_app_menu_button(
            tab_bar::app_menu_button_rect(style, is_macos),
            (
                hover_zone_button == Some(tab_bar::ZoneButtonHit::AppMenu),
                pressed_zone_button == Some(tab_bar::ZoneButtonHit::AppMenu)
                    && hover_zone_button == Some(tab_bar::ZoneButtonHit::AppMenu),
            ),
            app_icon,
            style,
            pal,
            &mut out,
        );
    }

    // Abas fixadas (ADR-0087 §1/§2): a mesma fileira do botão do app, à direita
    // dele, fora do recorte e do scroll. Sem fixadas nada é desenhado -- a
    // barra é a de antes, primitiva por primitiva.
    //
    // During a tab drag the zone is the preview of it (ADR-0087 §5): the slots
    // follow `pinned_order` (the gap for an incoming tab opens, the neighbours
    // slide there with the `.15s` reflow), the dragged box is a hole, and the
    // zone highlight -- the implicit run's, `ungrouped_color` -- goes under the
    // boxes, over the rectangle of the slots.
    if let Some(rect) = drag.as_ref().and_then(|ghost| ghost.zone_highlight) {
        let color = pal.ungrouped_group_color();
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect,
            radius: style.wrapper_corner_radius,
            color: with_alpha(color, style.drag_highlight_tint_strength),
            border_color: with_alpha(color, DRAG_HIGHLIGHT_BORDER_ALPHA),
            border_width: DRAG_HIGHLIGHT_BORDER_WIDTH,
        }));
    }
    let zone_order: &[TabId] = drag
        .as_ref()
        .map_or(workspace.pinned(), |ghost| &ghost.pinned_order);
    for (i, &id) in zone_order.iter().enumerate() {
        if drag.as_ref().is_some_and(|ghost| ghost.tab == id) {
            continue;
        }
        let mut slot = tab_bar::pinned_rect(style, is_macos, i);
        slot.x = animations.pinned_x(id, slot.x, now);
        let Some(paint) = pinned_paint(
            workspace,
            id,
            slot,
            hover == Some(tab_bar::TabBarHit::Tab(id)),
            active,
            pinned_icons.get(&id).copied(),
            app_icon,
        ) else {
            continue;
        };
        paint_pinned_tab(&paint, style, pal, &mut out);
    }

    // Recorte de verdade da trilha (ADR-0018, espec §2.18: "um recorte só,
    // na camada de chrome; as abas fora da vista desaparecem pelo clip").
    // Tudo dentro deste par desloca pelo scroll -- inclusive o botão "+" de
    // cada grupo, que "acompanha o scroll" igual às abas dele (fora da
    // espec., mesmo raciocínio do §2.6 aplicado ao botão por grupo). Vai de
    // `track_shift` (depois do botão de menu) até `trilha_width` -- as zonas
    // fixas das duas pontas ficam de fora do recorte e do scroll.
    let pinned_count = workspace.pinned().len();
    let track_left = tab_bar::track_shift(style, is_macos, pinned_count);
    out.push(Primitive::PushClip(Rect {
        x: track_left,
        y: 0.0,
        width: tab_bar::trilha_width(style, bar_width, is_macos, pinned_count),
        height: bar_height,
    }));
    // Conteúdo -> tela: a origem do conteúdo é o início da trilha.
    let scroll_dx = track_left - overflow.scroll_offset;

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
        // Cor de grupo: `ungrouped_color` para run implícito (ADR-0006),
        // a cor do grupo para explícito. Mesma resolução para a cápsula, o
        // fundo da pílula e o realce de fronteira do arraste.
        let group_color = core_group
            .and_then(|g| g.color())
            .map(|c| pal.group_color(c))
            .unwrap_or_else(|| pal.ungrouped_group_color());
        // Ajuste pedido pelo usuário (F3 etapa 6, fora da espec.): o grupo
        // é uma "cápsula" pintada com a cor cheia -- não o tingimento de
        // 7% da espec §2.3, que ficava quase invisível atrás do fundo
        // opaco das abas. `tab_active_background`/`tab_inactive_background`
        // (`palette.rs`) agora têm alfa `background_alpha` pra deixar
        // passar um indício dela por cima. Abas sem grupo (`pill == None`)
        // nunca pintam cápsula.
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
            if style.group_shadow_enabled {
                push_shadow(&mut out, capsule_rect, style.wrapper_corner_radius);
            }
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: capsule_rect,
                radius: style.wrapper_corner_radius,
                color: with_alpha(group_color, style.capsule_alpha),
                border_color: pal.glass_border,
                border_width: style.glass_border_width,
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
                radius: style.wrapper_corner_radius,
                color: with_alpha(group_color, style.drag_highlight_tint_strength),
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
                pal,
                style,
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
            let not_started = workspace.tab(tab.id).is_some_and(|t| t.is_not_started());
            let is_active = active == Some(tab.id);
            let (bg, border, border_width, text_color) = tab_colors(pal, style, exited, is_active);
            let text_color = label_color(text_color, not_started);
            // Hover por brilho no corpo da aba (espec §1.10, `1.18`).
            let bg = if hover == Some(tab_bar::TabBarHit::Tab(tab.id)) {
                brighten(bg, style.tab_hover_brightness)
            } else {
                bg
            };
            // RF-2.2/espec §2.5: selecionada é um modificador de borda, não
            // um quarto estado -- fundo e texto continuam vindo de
            // Ativa/Inativa acima.
            // Aba de dentro de grupo (cápsula atrás) não leva borda
            // (experimento visual); a de seleção continua, é feedback.
            let (border, border_width) = if selection.is_selected(tab.id) {
                (pal.selected_border, style.selected_border_width)
            } else if group.pill.is_some() {
                (palette::TRANSPARENT, 0.0)
            } else {
                (border, border_width)
            };

            // Sombra só na aba solta (sem cápsula atrás) -- aba de dentro
            // de um grupo já leva o respiro da cápsula, e pedido do
            // usuário foi sombra na cápsula e na aba solta, não nas duas.
            if group.pill.is_none() && style.group_shadow_enabled {
                push_shadow(&mut out, tab_rect, style.tab_corner_radius);
            }
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: tab_rect,
                radius: style.tab_corner_radius,
                color: scale_alpha(bg, fade_in),
                border_color: scale_alpha(border, fade_in),
                border_width,
            }));

            // Order: badge, indicator dot, label (ADR-0070 §7).
            push_tab_badge(
                &mut out,
                tab,
                tab_rect,
                style,
                group_color,
                fade_in,
                measurer,
            );
            if let Some(indicator) = tab.indicator {
                let color = match indicator {
                    Indicator::Activity => pal.activity_indicator,
                    Indicator::Bell => pal.bell_indicator,
                };
                out.push(Primitive::RoundedQuad(RoundedQuad {
                    rect: Rect {
                        x: tab_rect.x + tab.dot_offset(style),
                        y: tab_rect.y + (tab_rect.height - style.indicator_dot_size) / 2.0,
                        width: style.indicator_dot_size,
                        height: style.indicator_dot_size,
                    },
                    radius: style.indicator_dot_size / 2.0,
                    color: scale_alpha(color, fade_in),
                    border_color: palette::TRANSPARENT,
                    border_width: 0.0,
                }));
            }

            if rename.editing_tab() == Some(tab.id)
                && let Some(field) = rename.field()
            {
                paint_rename_field(tab_rect, style, pal, term_pal, field, measurer, &mut out);
            } else {
                let label_y = tab_rect.y + (tab_rect.height - style.font_size) / 2.0;
                out.push(Primitive::Text(TextRun {
                    origin: (tab_rect.x + tab.label_offset(style), label_y),
                    text: tab.label.clone(),
                    font: LABEL_FONT,
                    size_px: style.font_size,
                    color: scale_alpha(text_color, fade_in),
                }));
            }

            let show_close = match style.show_close_button {
                porecatu_config::CloseButtonVisibility::Always => true,
                porecatu_config::CloseButtonVisibility::Never => false,
                porecatu_config::CloseButtonVisibility::Hover => matches!(
                    hover,
                    Some(tab_bar::TabBarHit::Tab(id) | tab_bar::TabBarHit::CloseButton(id))
                        if id == tab.id
                ),
            };
            if show_close {
                // Mesmo hover/pressed do "+" (`paint_zone_icon_button`).
                let close_hovered = hover == Some(tab_bar::TabBarHit::CloseButton(tab.id));
                paint_zone_icon_button(
                    shift(tab.close_button, dx),
                    icon::X,
                    close_icon_size,
                    (close_hovered, close_hovered && pressed_bar_button == hover),
                    scale_alpha(pal.chrome_icon, fade_in),
                    style,
                    pal,
                    &mut out,
                );
            }
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
                pal.group_new_tab_icon
            } else {
                pal.chrome_icon
            };
            let hovered =
                matches!(hover, Some(tab_bar::TabBarHit::GroupNewTab(id)) if id == group.id);
            paint_zone_icon_button(
                group_button,
                icon::PLUS,
                new_tab_icon_size,
                (hovered, hovered && pressed_bar_button == hover),
                icon_color,
                style,
                pal,
                &mut out,
            );
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
        let not_started = workspace
            .tab(old_tab.id)
            .is_some_and(|t| t.is_not_started());
        let is_active = active == Some(old_tab.id);
        let (bg, border, _border_width, text_color) = tab_colors(pal, style, exited, is_active);
        let text_color = label_color(text_color, not_started);
        // Mesma regra da aba parada: dentro de grupo explícito, sem borda.
        let in_group = workspace
            .group_of_tab(old_tab.id)
            .and_then(|id| workspace.group(id))
            .is_some_and(|g| g.is_explicit());
        let (border, border_width) = if in_group {
            (palette::TRANSPARENT, 0.0)
        } else {
            (border, 1.0)
        };
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: tab_rect,
            radius: style.tab_corner_radius,
            color: scale_alpha(bg, fade_out),
            border_color: scale_alpha(border, fade_out),
            border_width,
        }));
        let group_color = tab_group_color(workspace, pal, old_tab.id);
        push_tab_badge(
            &mut out,
            old_tab,
            tab_rect,
            style,
            group_color,
            fade_out,
            measurer,
        );
        let label_y = tab_rect.y + (tab_rect.height - style.font_size) / 2.0;
        out.push(Primitive::Text(TextRun {
            origin: (tab_rect.x + old_tab.label_offset(style), label_y),
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
        let hovered = hover == Some(tab_bar::TabBarHit::UngroupedNewTab);
        paint_zone_icon_button(
            button,
            icon::PLUS,
            new_tab_icon_size,
            (hovered, hovered && pressed_bar_button == hover),
            pal.chrome_icon,
            style,
            pal,
            &mut out,
        );
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
    // configurações (RF-11.27): abre o arquivo de config no editor padrão
    // do sistema, ver `handle_bar_click`.
    // ADR-0054/ADR-0055: botão de sessões nomeadas, imediatamente à
    // esquerda da engrenagem -- mesma anatomia, pintado pelo mesmo
    // helper para os dois nunca divergirem (`paint_zone_icon_button`).
    // Hover e pressed como os botões de janela, só que num quadrado
    // arredondado que não ocupa a altura da barra. Pressed só com o cursor
    // ainda sobre o botão armado -- a condição em que o release dispara.
    for (hit, glyph) in [
        (tab_bar::ZoneButtonHit::Sessions, icon::BOOKMARK),
        (tab_bar::ZoneButtonHit::Settings, icon::SETTINGS),
    ] {
        let rect = match hit {
            tab_bar::ZoneButtonHit::Sessions => {
                tab_bar::sessions_button_rect(style, bar_width, bar_height, is_macos)
            }
            tab_bar::ZoneButtonHit::Settings => {
                tab_bar::settings_button_rect(style, bar_width, bar_height, is_macos)
            }
            // Not an icon button: painted before the track.
            tab_bar::ZoneButtonHit::AppMenu => continue,
        };
        let hovered = hover_zone_button == Some(hit);
        let pressed = hovered && pressed_zone_button == Some(hit);
        paint_zone_icon_button(
            rect,
            glyph,
            settings_icon_size,
            (hovered, pressed),
            pal.chrome_icon,
            style,
            pal,
            &mut out,
        );
    }

    // Botões de janela (ADR-0027): minimizar/maximizar-restaurar/fechar,
    // colados na borda direita. Não existem no macOS -- lá é o semáforo
    // nativo (`left_inset` antes do botão de menu, não zona de botão aqui).
    if !is_macos {
        paint_window_buttons(
            style,
            pal,
            bar_width,
            bar_height,
            show_restore_icon,
            hover_window_button,
            pressed_window_button,
            &mut out,
        );
    }

    // Sem botões de overflow (experimento visual): a trilha só rola pela
    // roda do mouse e pelo arraste.

    // A box dragged out of the zone stays the square box, on the track too:
    // drawn here, past the clip of the track and over the zones (ADR-0087 §5).
    if let Some(ghost) = &drag
        && ghost.source == GhostSource::Pinned
    {
        let side = tab_bar::app_menu_button_size(style);
        let rect = Rect {
            x: ghost.screen_x,
            y: tab_bar::pinned_rect(style, is_macos, 0).y,
            width: side,
            height: side,
        };
        // `hovered`: the ghost always carries the hover brightness, like the
        // track one (it follows the cursor; there is no "being over it").
        if let Some(paint) = pinned_paint(
            workspace,
            ghost.tab,
            rect,
            true,
            active,
            pinned_icons.get(&ghost.tab).copied(),
            app_icon,
        ) {
            paint_pinned_tab(&paint, style, pal, &mut out);
        }
    }

    if let Some(ghost) = &drag
        && let GhostSource::Track(tab) = &ghost.source
    {
        let exited = workspace.tab(tab.id).is_some_and(|t| t.is_exited());
        let not_started = workspace.tab(tab.id).is_some_and(|t| t.is_not_started());
        let is_active = active == Some(tab.id);
        let (bg, border, _border_width, text_color) = tab_colors(pal, style, exited, is_active);
        let text_color = label_color(text_color, not_started);
        // Espec §2.19: "hoje o fantasma sai no mesmo tom da aba parada; o
        // filter: brightness(1.18) e a sombra... estão nas duas mudanças
        // aprovadas para a F4" -- sempre ligado durante o arraste, não
        // condicionado a hover (o fantasma segue o cursor, não há "estar
        // em cima dele").
        let bg = brighten(bg, style.tab_hover_brightness);
        let (border, border_width) = if selection.is_selected(tab.id) {
            (pal.selected_border, style.selected_border_width)
        } else {
            (border, 1.0)
        };
        let ghost_rect = Rect {
            x: ghost.screen_x,
            y: tab.rect.y,
            width: tab.rect.width,
            height: tab.rect.height,
        };
        if style.group_shadow_enabled {
            push_shadow(&mut out, ghost_rect, style.tab_corner_radius);
        }
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: ghost_rect,
            radius: style.tab_corner_radius,
            color: bg,
            border_color: border,
            border_width,
        }));
        let group_color = tab_group_color(workspace, pal, tab.id);
        push_tab_badge(&mut out, tab, ghost_rect, style, group_color, 1.0, measurer);
        let label_y = ghost_rect.y + (ghost_rect.height - style.font_size) / 2.0;
        out.push(Primitive::Text(TextRun {
            origin: (ghost_rect.x + tab.label_offset(style), label_y),
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
            .map(|c| pal.group_color(c))
            .unwrap_or_else(|| pal.ungrouped_group_color());
        let ghost_dx = ghost.screen_x - pill.rect.x;
        // ADR-0032 §2: sombra em camadas "nos cinco widgets de chrome e
        // no fantasma de arraste" -- este é o fantasma do RF-2.19 (pílula
        // sozinha, §2.19.1).
        if style.group_shadow_enabled {
            push_shadow(
                &mut out,
                shift(pill.rect, ghost_dx),
                style.pill_corner_radius,
            );
        }
        // O rótulo em si não tem fundo (experimento visual), mas o fantasma
        // flutua sobre a barra e sem fundo ficaria só o texto: ele mantém o
        // preenchimento na cor do grupo, sem borda.
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: shift(pill.rect, ghost_dx),
            radius: style.pill_corner_radius,
            color: with_alpha(color, style.pill_glass_alpha),
            border_color: palette::TRANSPARENT,
            border_width: 0.0,
        }));
        paint_group_pill(
            pill,
            pal,
            style,
            None,
            style.pill_font_size,
            ghost_dx,
            measurer,
            &mut out,
        );
    }

    out
}

/// Cor de fundo/borda/texto de uma aba, conforme o estado -- e a largura de
/// borda correspondente (`[appearance.tabs.colors] active_border_width`/
/// `inactive_border_width`; hoje o mesmo valor no arquivo de exemplo, mas
/// chaves distintas, então mantidas distintas aqui).
fn tab_colors(
    pal: &ResolvedPalette,
    style: &TabBarStyle,
    exited: bool,
    is_active: bool,
) -> (Color, Color, f32, Color) {
    if exited {
        (
            pal.tab_inactive_background,
            pal.tab_inactive_border,
            style.inactive_border_width,
            pal.tab_exited_text,
        )
    } else if is_active {
        (
            pal.tab_active_background,
            pal.tab_active_border,
            style.active_border_width,
            pal.tab_active_text,
        )
    } else {
        (
            pal.tab_inactive_background,
            pal.tab_inactive_border,
            style.inactive_border_width,
            pal.tab_inactive_text,
        )
    }
}

/// Empilha as camadas de `SHADOW_LAYERS` atrás de `rect` (raio `radius`).
/// Ver nota em `SHADOW_LAYERS`. Chamado só quando `style.group_shadow_enabled`
/// (`[appearance.groups] shadow`).
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

/// Botão de ícone (engrenagem, sessões -- ADR-0055 §1 -- e "+" de nova aba): o
/// ícone centrado num quadrado de cantos arredondados, sem borda. Em
/// repouso só o ícone aparece; hover e pressed "revelam" o botão com o
/// fundo dos botões de janela (`window_controls.hover_background` /
/// `pressed_background`), e o ícone só muda de cor onde o fundo exige (o "+"
/// sobre a cápsula de um grupo). Extraído aqui para
/// os botões (sessões, engrenagem, "+") nunca divergirem entre si.
#[allow(clippy::too_many_arguments)]
fn paint_zone_icon_button(
    rect: Rect,
    icon: icon::Icon,
    icon_size: f32,
    (hovered, pressed): (bool, bool),
    icon_color: Color,
    style: &TabBarStyle,
    pal: &ResolvedPalette,
    out: &mut Vec<Primitive>,
) {
    let bg = match (hovered, pressed) {
        (_, true) => pal.window_button_pressed_bg,
        (true, false) => pal.window_button_hover_bg,
        (false, false) => palette::TRANSPARENT,
    };
    // O retângulo do botão é mais largo que alto (respiro lateral,
    // `icon_button_padding_x`); o fundo revelado é o quadrado de lado
    // `height` centrado nele.
    let square = Rect {
        x: rect.x + (rect.width - rect.height) / 2.0,
        y: rect.y,
        width: rect.height,
        height: rect.height,
    };
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: square,
        radius: style.tab_corner_radius,
        color: bg,
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));
    out.push(centered_glyph(icon, rect, icon_size, icon_color));
}

/// Botão de menu do app: a caixa de uma aba solta **ativa** (mesmas cores,
/// raio, borda e sombra de [`tab_colors`]/`push_shadow`) com o ícone do app
/// no meio. Hover clareia o fundo como o da aba ativa
/// (`tab_hover_brightness`). Pressed é o da aba ativa apertada: a aba não
/// pinta um estado próprio, mas o press arma `Drag::TabPressed`, que
/// suprime o hover da barra (`bar_hover`, em `lib.rs`) -- ela volta ao fundo
/// ativo sem o brilho, mais escuro que no hover. O botão faz o mesmo: sem
/// brilho enquanto pressionado. `app_icon` ausente: só a caixa.
fn paint_app_menu_button(
    rect: Rect,
    (hovered, pressed): (bool, bool),
    app_icon: Option<ImageId>,
    style: &TabBarStyle,
    pal: &ResolvedPalette,
    out: &mut Vec<Primitive>,
) {
    let (bg, border, border_width, _text) = tab_colors(pal, style, false, true);
    let bg = if hovered && !pressed {
        brighten(bg, style.tab_hover_brightness)
    } else {
        bg
    };
    if style.group_shadow_enabled {
        push_shadow(out, rect, style.tab_corner_radius);
    }
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: style.tab_corner_radius,
        color: bg,
        border_color: border,
        border_width,
    }));
    if let Some(image) = app_icon {
        let side = style.icon_em_size;
        let icon_rect = Rect {
            x: rect.x + (rect.width - side) / 2.0,
            y: rect.y + (rect.height - side) / 2.0,
            width: side,
            height: side,
        };
        out.push(Primitive::Image {
            rect: icon_rect,
            uv: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            repeat: false,
            mask: icon_rect,
            mask_radius: 0.0,
            alpha: 1.0,
            image,
        });
    }
}

/// What one pinned tab box needs to be painted (ADR-0087 §2). A plain struct
/// instead of a long argument list.
pub(crate) struct PinnedTabPaint {
    /// The box, in screen coordinates (`tab_bar::pinned_rect`).
    pub rect: Rect,
    pub active: bool,
    /// `Exited` (RF-21.24): inactive colors even when active, icon at `.45`.
    pub exited: bool,
    /// `NotStarted` (dormant SSH, ADR-0037): icon at `.45`.
    pub not_started: bool,
    /// The cursor is over the box and no press or drag suppresses the hover.
    pub hovered: bool,
    pub indicator: Option<Indicator>,
    /// The icon texture; `None` paints the box alone.
    pub icon: Option<ImageId>,
    /// Size in pixels of level 0 of `icon`, for the *contain* fit (ADR-0088
    /// §6). A square (`(1, 1)`) fills the whole square of `icon_em_size`.
    pub icon_size: (u32, u32),
}

/// The paint of pinned tab `id` in `rect`: its state from the workspace, the
/// one place the zone and the drag ghost take it from. `None` if the tab is
/// gone from the workspace.
///
/// The icon (ADR-0088 §4, RF-21.21): the tab's own texture once its result
/// arrived, the app icon until then and whenever the chain ended there. The
/// app icon is drawn as a square, as the menu button does.
fn pinned_paint(
    workspace: &Workspace,
    id: TabId,
    rect: Rect,
    hovered: bool,
    active: Option<TabId>,
    own_icon: Option<IconTexture>,
    app_icon: Option<ImageId>,
) -> Option<PinnedTabPaint> {
    let tab = workspace.tab(id)?;
    let (icon, icon_size) = match own_icon {
        Some(texture) => (Some(texture.id), texture.size),
        None => (app_icon, (1, 1)),
    };
    Some(PinnedTabPaint {
        rect,
        active: active == Some(id),
        exited: tab.is_exited(),
        not_started: tab.is_not_started(),
        hovered,
        indicator: tab_bar::tab_indicator(tab),
        icon,
        icon_size,
    })
}

/// A pinned tab (ADR-0087 §2): the box of a loose tab -- same radius, border,
/// shadow and colors as [`tab_colors`] gives it by state -- with the icon
/// centered where the label would be. Hover brightens the background like any
/// tab; a press arms `Drag::TabPressed`, which clears `hovered` upstream, so
/// the pressed box is the one without the brightness. `NotStarted` and
/// `Exited` dim the icon to `NOT_STARTED_LABEL_ALPHA` (the `.45` of the spec),
/// and `Exited` also takes the inactive background. The activity/bell dot
/// (spec 2.17) sits over the top-right corner, `padding_right` from the top
/// and right edges, and never on an exited box.
pub(crate) fn paint_pinned_tab(
    tab: &PinnedTabPaint,
    style: &TabBarStyle,
    pal: &ResolvedPalette,
    out: &mut Vec<Primitive>,
) {
    let rect = tab.rect;
    let (bg, border, border_width, _text) = tab_colors(pal, style, tab.exited, tab.active);
    let bg = if tab.hovered {
        brighten(bg, style.tab_hover_brightness)
    } else {
        bg
    };
    if style.group_shadow_enabled {
        push_shadow(out, rect, style.tab_corner_radius);
    }
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: style.tab_corner_radius,
        color: bg,
        border_color: border,
        border_width,
    }));
    if let Some(image) = tab.icon {
        // ADR-0088 §6: *contain* in the square of `icon_em_size`, centered in
        // the box; a non-square image leaves transparency on the two sides.
        let (width, height) = pinned_icon::contain(style.icon_em_size, tab.icon_size);
        let icon_rect = Rect {
            x: rect.x + (rect.width - width) / 2.0,
            y: rect.y + (rect.height - height) / 2.0,
            width,
            height,
        };
        let alpha = if tab.exited || tab.not_started {
            NOT_STARTED_LABEL_ALPHA as f32
        } else {
            1.0
        };
        out.push(Primitive::Image {
            rect: icon_rect,
            uv: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            repeat: false,
            mask: icon_rect,
            mask_radius: 0.0,
            alpha,
            image,
        });
    }
    if let Some(indicator) = tab.indicator.filter(|_| !tab.exited) {
        let color = match indicator {
            Indicator::Activity => pal.activity_indicator,
            Indicator::Bell => pal.bell_indicator,
        };
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: Rect {
                x: rect.x + rect.width - style.padding_right - style.indicator_dot_size,
                y: rect.y + style.padding_right,
                width: style.indicator_dot_size,
                height: style.indicator_dot_size,
            },
            radius: style.indicator_dot_size / 2.0,
            color,
            border_color: palette::TRANSPARENT,
            border_width: 0.0,
        }));
    }
}

/// `filter: brightness(mult)` (espec §1.10), resolvido em CPU: multiplica
/// os três canais e clampa em 1.0 -- `porecatu-render` não tem primitiva
/// de filtro (ADR-0032 §2), e não ganha uma só por isto. Alfa intocado.
/// `pub(crate)`: `search_bar.rs` reusa para o hover dos três botões de
/// ícone (ADR-0041 §6: "mesmo brilho `1.18` dos demais botões da barra").
pub(crate) fn brighten(color: Color, mult: f64) -> Color {
    Color {
        r: (color.r * mult).min(1.0),
        g: (color.g * mult).min(1.0),
        b: (color.b * mult).min(1.0),
        a: color.a,
    }
}

/// Espessura da borda de janela, em pixels lógicos.
pub(crate) const WINDOW_BORDER_WIDTH: f32 = 1.0;

/// Fator que escurece o fundo da janela para a cor da borda dela.
const WINDOW_BORDER_DARKEN: f64 = 0.7;

/// A borda de 1px em volta de uma janela de `width` x `height` pixels
/// lógicos: quatro quads nas bordas, de uma cor um pouco mais escura que
/// `background`. Vai por cima de tudo, então a camada mais alta é quem a
/// recebe.
pub(crate) fn window_border(width: f32, height: f32, background: Color) -> Vec<Primitive> {
    let color = Color {
        r: background.r * WINDOW_BORDER_DARKEN,
        g: background.g * WINDOW_BORDER_DARKEN,
        b: background.b * WINDOW_BORDER_DARKEN,
        a: 1.0,
    };
    let t = WINDOW_BORDER_WIDTH;
    let edge = |x: f32, y: f32, w: f32, h: f32| {
        Primitive::Quad(Quad {
            rect: Rect {
                x,
                y,
                width: w,
                height: h,
            },
            color,
        })
    };
    vec![
        edge(0.0, 0.0, width, t),
        edge(0.0, height - t, width, t),
        edge(0.0, t, t, height - 2.0 * t),
        edge(width - t, t, t, height - 2.0 * t),
    ]
}

/// Multiplica o alfa que a cor já tem por `mult` -- diferente de
/// `with_alpha`, que substitui. É o que faz a aba esmaecer em cima do
/// alfa `background_alpha` que `tab_active_background`/
/// `tab_inactive_background` já carregam (ADR-0022, fade de entrada/saída
/// do colapso), sem perder o "indício da cápsula" que esse alfa existe pra
/// deixar passar.
fn scale_alpha(color: Color, mult: f32) -> Color {
    Color {
        a: color.a * mult as f64,
        ..color
    }
}

/// RF-3.9 (ADR-0037 §5): sobre a cor de texto do estado de base (ativa ou
/// inativa, já resolvida por `tab_colors`), aba `NotStarted` esmaece para
/// `NOT_STARTED_LABEL_ALPHA`; qualquer outra fica no alfa cheio que já
/// tinha. Nada mais muda -- fundo, borda, largura e cápsula atrás ficam
/// exatamente como `tab_colors` decidiu.
fn label_color(text_color: Color, not_started: bool) -> Color {
    if not_started {
        with_alpha(text_color, NOT_STARTED_LABEL_ALPHA)
    } else {
        text_color
    }
}

// Profile badge (spec §2.5 item 1): mono 9px/500, radius 3, `padding: 2px 4px`,
// `letter-spacing: .4px`. Text in the group color, background the same color at
// `[appearance.groups] badge_tint_strength`. Spec values with no TOML key.
const BADGE_FONT: FontFace = FontFace::Mono { bold: true };
const BADGE_FONT_SIZE: f32 = 9.0;
const BADGE_RADIUS: f32 = 3.0;
const BADGE_PADDING_X: f32 = 4.0;
const BADGE_PADDING_Y: f32 = 2.0;
const BADGE_LETTER_SPACING: f32 = 0.4;

/// Width and height of the badge for `text`. `TextRun` has no letter spacing,
/// so [`push_badge`] places one run per character; the mono advance is the
/// same for every character, so the width is `n * (advance + spacing)`.
pub(crate) fn badge_size(text: &str, measurer: &mut porecatu_render::TextMeasurer) -> (f32, f32) {
    let advance = measurer.measure_width("M", BADGE_FONT, BADGE_FONT_SIZE);
    let chars = text.chars().count() as f32;
    (
        chars * (advance + BADGE_LETTER_SPACING) + BADGE_PADDING_X * 2.0,
        BADGE_FONT_SIZE * 1.2 + BADGE_PADDING_Y * 2.0,
    )
}

/// Paints the badge with its top-left corner at `origin` and returns its
/// rectangle. Reused by the profile menu now and by the tab (prompt 06).
pub(crate) fn push_badge(
    out: &mut Vec<Primitive>,
    origin: (f32, f32),
    text: &str,
    color: Color,
    tint_strength: f64,
    measurer: &mut porecatu_render::TextMeasurer,
) -> Rect {
    let (width, height) = badge_size(text, measurer);
    let rect = Rect {
        x: origin.0,
        y: origin.1,
        width,
        height,
    };
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: BADGE_RADIUS,
        color: with_alpha(color, tint_strength),
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));
    let advance = measurer.measure_width("M", BADGE_FONT, BADGE_FONT_SIZE);
    let mut x = rect.x + BADGE_PADDING_X;
    for ch in text.chars() {
        out.push(Primitive::Text(TextRun {
            origin: (x, rect.y + BADGE_PADDING_Y),
            text: ch.to_string(),
            font: BADGE_FONT,
            size_px: BADGE_FONT_SIZE,
            color,
        }));
        x += advance + BADGE_LETTER_SPACING;
    }
    rect
}

/// Color of the group a tab belongs to (the ungrouped color for an implicit
/// run), the one its badge takes.
fn tab_group_color(workspace: &Workspace, pal: &ResolvedPalette, tab: TabId) -> Color {
    workspace
        .group_of_tab(tab)
        .and_then(|id| workspace.group(id))
        .and_then(|g| g.color())
        .map(|c| pal.group_color(c))
        .unwrap_or_else(|| pal.ungrouped_group_color())
}

/// Paints the tab's profile badge, vertically centered at the left padding.
/// Nothing when the tab has none. `fade` is the tab's own opacity (the
/// reflow and the close animation).
fn push_tab_badge(
    out: &mut Vec<Primitive>,
    tab: &tab_bar::TabRect,
    tab_rect: Rect,
    style: &TabBarStyle,
    group_color: Color,
    fade: f32,
    measurer: &mut porecatu_render::TextMeasurer,
) {
    let Some(badge) = &tab.badge else {
        return;
    };
    let (_, height) = badge_size(&badge.text, measurer);
    push_badge(
        out,
        (
            tab_rect.x + style.padding_left,
            tab_rect.y + (tab_rect.height - height) / 2.0,
        ),
        &badge.text,
        scale_alpha(group_color, fade),
        // `[appearance.groups] badge_tint_strength`, the value the drag
        // highlight already carries.
        style.drag_highlight_tint_strength * f64::from(fade),
        measurer,
    );
}

#[cfg(test)]
mod badge_tests {
    use super::*;
    use porecatu_render::TextMeasurer;

    #[test]
    fn badge_grows_by_one_advance_plus_spacing_per_character() {
        let mut m = TextMeasurer::new();
        let (one, h1) = badge_size("P", &mut m);
        let (three, h3) = badge_size("WSL", &mut m);
        let advance = m.measure_width("M", BADGE_FONT, BADGE_FONT_SIZE);
        assert!((three - one - 2.0 * (advance + BADGE_LETTER_SPACING)).abs() < 1e-3);
        assert_eq!(h1, h3);
    }

    #[test]
    fn badge_paints_a_tinted_box_and_one_run_per_character() {
        let mut m = TextMeasurer::new();
        let color = Color {
            r: 0.4,
            g: 0.6,
            b: 0.9,
            a: 1.0,
        };
        let mut out = Vec::new();
        let rect = push_badge(&mut out, (10.0, 20.0), "WSL", color, 0.14, &mut m);
        assert_eq!((rect.x, rect.y), (10.0, 20.0));
        let Primitive::RoundedQuad(quad) = &out[0] else {
            panic!("the box comes first");
        };
        assert_eq!(quad.color.a, 0.14);
        assert_eq!(quad.radius, 3.0);
        let runs: Vec<_> = out
            .iter()
            .filter_map(|p| match p {
                Primitive::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(runs.len(), 3);
        assert!(runs.iter().all(|t| t.color == color));
        assert!(runs[1].origin.0 > runs[0].origin.0);
    }
}

#[cfg(test)]
mod label_color_tests {
    use super::*;

    const OPAQUE_WHITE: Color = Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    /// RF-3.9: o alfa é o mesmo já usado no realce de borda do arraste --
    /// nenhum valor inventado (espec. §1.8).
    #[test]
    fn not_started_label_uses_the_shared_45_alpha() {
        assert_eq!(
            label_color(OPAQUE_WHITE, true).a,
            DRAG_HIGHLIGHT_BORDER_ALPHA
        );
    }

    #[test]
    fn started_label_keeps_the_base_color_untouched() {
        assert_eq!(label_color(OPAQUE_WHITE, false), OPAQUE_WHITE);
    }
}

/// Rótulo de grupo (espec §2.4, divergente): só o nome e, com o grupo
/// colapsado, um ponto por aba escondida à direita dele. Sem fundo nem
/// borda (experimento visual): quem dá a cor do grupo é a cápsula atrás.
/// Texto e pontos usam `pal.group_new_tab_icon` -- o mesmo escuro do "+" do
/// grupo, pela mesma razão: sobre a cor cheia do grupo o claro perde
/// contraste.
#[allow(clippy::too_many_arguments)]
fn paint_group_pill(
    pill: &GroupPillRect,
    pal: &ResolvedPalette,
    style: &TabBarStyle,
    live_name: Option<&str>,
    name_font_size: f32,
    dx: f32,
    measurer: &mut porecatu_render::TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let name_text = match live_name {
        Some(buffer) => {
            let cap = style.pill_name_max_width;
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
        color: pal.group_new_tab_icon,
    }));
    // Um ponto por aba colapsada (`dot_count` é 0 com o grupo expandido); o
    // da aba com atividade/campainha leva a cor do indicador.
    for i in 0..pill.dot_count {
        let dot_color = match pill.dot_indicators.get(i).copied().flatten() {
            Some(Indicator::Activity) => pal.activity_indicator,
            Some(Indicator::Bell) => pal.bell_indicator,
            None => pal.group_new_tab_icon,
        };
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: shift(
                Rect {
                    x: pill.dots_origin.0 + i as f32 * (style.indicator_dot_size + style.tab_gap),
                    y: pill.dots_origin.1,
                    width: style.indicator_dot_size,
                    height: style.indicator_dot_size,
                },
                dx,
            ),
            radius: style.indicator_dot_size / 2.0,
            color: dot_color,
            border_color: palette::TRANSPARENT,
            border_width: 0.0,
        }));
    }
}

/// Retângulo do campo de rename dentro de `tab_rect` -- função pura,
/// compartilhada pela pintura (`paint_rename_field`) e pelo hit-test de
/// clique/arraste em `lib.rs` (ADR-0035). Mesma nota de `scrolled_text_x`:
/// nunca duas cópias da mesma geometria.
pub(crate) fn rename_field_rect(tab_rect: Rect, style: &TabBarStyle) -> Rect {
    let available_width = (tab_rect.width - style.padding_left - style.padding_right).max(0.0);
    let field_width = style.rename_field_width.min(available_width);
    Rect {
        x: tab_rect.x + style.padding_left,
        y: tab_rect.y + (tab_rect.height - style.rename_field_height) / 2.0,
        width: field_width,
        height: style.rename_field_height,
    }
}

/// Campo de rename (espec §2.5): substitui o rótulo no lugar, largura
/// `min(120, largura disponível)`. Texto rola dentro do campo mantendo o
/// caret visível: quando o texto não cabe, a origem desliza para a
/// esquerda, e um `PushClip`/`PopClip` contém o transbordo. **Desde o
/// ADR-0035**, `field` carrega cursor navegável e seleção -- o caret usa a
/// posição de `field.cursor()`, não mais sempre o fim do buffer, e uma
/// seleção ativa pinta um destaque (`term_pal.selection_background`) em
/// vez do caret.
fn paint_rename_field(
    tab_rect: Rect,
    style: &TabBarStyle,
    pal: &ResolvedPalette,
    term_pal: &palette::ResolvedTermPalette,
    field: &crate::text_field::TextFieldState,
    measurer: &mut porecatu_render::TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let field_rect = rename_field_rect(tab_rect, style);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: field_rect,
        // Espec. sem chave de raio para o campo de rename -- valor de
        // trabalho, mesma nota histórica de `RENAME_FIELD_HEIGHT`.
        radius: 4.0,
        color: pal.rename_background,
        border_color: pal.rename_border,
        border_width: 1.0,
    }));

    let buffer = field.text();
    let text_area = (field_rect.width - style.rename_padding_x * 2.0).max(0.0);
    let text_width = measurer.measure_width(buffer, LABEL_FONT, style.rename_font_size);
    let text_x =
        tab_bar::scrolled_text_x(field_rect.x, style.rename_padding_x, text_width, text_area);
    let text_y = field_rect.y + (style.rename_field_height - style.rename_font_size) / 2.0;

    out.push(Primitive::PushClip(field_rect));
    let selection_range = field.selection_range();
    if let Some((start, end)) = selection_range {
        let sel_x0 =
            text_x + measurer.measure_width(&buffer[..start], LABEL_FONT, style.rename_font_size);
        let sel_x1 =
            text_x + measurer.measure_width(&buffer[..end], LABEL_FONT, style.rename_font_size);
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: sel_x0,
                y: field_rect.y + 3.0,
                width: sel_x1 - sel_x0,
                height: style.rename_field_height - 6.0,
            },
            color: term_pal.selection_background,
        }));
    }
    out.push(Primitive::Text(TextRun {
        origin: (text_x, text_y),
        text: buffer.to_string(),
        font: LABEL_FONT,
        size_px: style.rename_font_size,
        color: pal.rename_text,
    }));
    if selection_range.is_none() {
        let cursor_width = measurer.measure_width(
            &buffer[..field.cursor()],
            LABEL_FONT,
            style.rename_font_size,
        );
        let caret_x = (text_x + cursor_width).min(field_rect.x + field_rect.width - 1.0);
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: caret_x,
                y: field_rect.y + 3.0,
                width: 1.0,
                height: style.rename_field_height - 6.0,
            },
            color: pal.rename_text,
        }));
    }
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

/// Os três botões de janela do ADR-0027 (minimizar, maximizar/restaurar,
/// fechar), colados na borda direita de uma faixa de `bar_width` ×
/// `bar_height`. Compartilhado pela barra de abas e pelo cabeçalho da janela
/// de configurações (ADR-0059 §2): é a mesma geometria
/// (`tab_bar::window_button_rect`), o mesmo hover e o mesmo desenho, então
/// os dois nunca divergem. Fora do macOS -- lá o semáforo é nativo e quem
/// chama não chega aqui.
///
/// `pressed_window_button` é o botão armado pelo press (ADR-0066): ele só
/// pinta o fundo de pressionado com o cursor ainda sobre ele, a mesma
/// condição em que o release dispara (`tab_bar::window_button_release`).
/// Arrastado para fora, volta ao repouso.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_window_buttons(
    style: &TabBarStyle,
    pal: &ResolvedPalette,
    bar_width: f32,
    bar_height: f32,
    show_restore_icon: bool,
    hover_window_button: Option<tab_bar::WindowButtonHit>,
    pressed_window_button: Option<tab_bar::WindowButtonHit>,
    out: &mut Vec<Primitive>,
) {
    paint_window_buttons_of(
        style,
        pal,
        bar_width,
        bar_height,
        show_restore_icon,
        hover_window_button,
        pressed_window_button,
        &tab_bar::WINDOW_BUTTONS,
        out,
    );
}

/// [`paint_window_buttons`] só com os botões de `shown`; os outros não são
/// desenhados nem ocupam lugar (ADR-0080 §3).
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_window_buttons_of(
    style: &TabBarStyle,
    pal: &ResolvedPalette,
    bar_width: f32,
    bar_height: f32,
    show_restore_icon: bool,
    hover_window_button: Option<tab_bar::WindowButtonHit>,
    pressed_window_button: Option<tab_bar::WindowButtonHit>,
    shown: &[tab_bar::WindowButtonHit],
    out: &mut Vec<Primitive>,
) {
    // Menor que o "+"/configurações -- convenção Windows: glyphs finos de
    // minimizar/maximizar/fechar (ADR-0027). Sem chave própria.
    let window_button_icon_size = style.icon_em_size * 0.7;
    let buttons = [
        (0u8, tab_bar::WindowButtonHit::Minimize, icon::MINUS, false),
        (
            1u8,
            tab_bar::WindowButtonHit::MaximizeRestore,
            if show_restore_icon {
                icon::RESTORE
            } else {
                icon::MAXIMIZE
            },
            false,
        ),
        (2u8, tab_bar::WindowButtonHit::Close, icon::X, true),
    ];
    for (index, hit, glyph, is_close) in buttons {
        if !shown.contains(&hit) {
            continue;
        }
        let rect = tab_bar::window_button_rect(style, index, bar_width, bar_height);
        let hovered = hover_window_button == Some(hit);
        let pressed = hovered && pressed_window_button == Some(hit);
        let bg = match (is_close, hovered, pressed) {
            (true, _, true) => pal.window_close_pressed_bg,
            (false, _, true) => pal.window_button_pressed_bg,
            (true, true, false) => pal.window_close_hover_bg,
            (false, true, false) => pal.window_button_hover_bg,
            (_, false, _) => palette::TRANSPARENT,
        };
        out.push(Primitive::Quad(Quad { rect, color: bg }));
        let icon_color = if is_close && hovered {
            pal.window_close_hover_icon
        } else {
            pal.chrome_icon
        };
        out.push(centered_glyph(
            glyph,
            rect,
            window_button_icon_size,
            icon_color,
        ));
    }
}

/// Altura total da barra (espec §2.5/§2.3): abas + a folga do wrapper
/// acima e abaixo. Usado por `lib.rs` para deslocar a grade do terminal e
/// converter posição de clique.
pub fn bar_height(style: &TabBarStyle) -> f32 {
    style.tab_height + style.wrapper_padding * 2.0 + style.trilha_padding * 2.0
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_border_is_four_one_pixel_quads_darker_than_the_background() {
        let bg = Color {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let border = window_border(100.0, 50.0, bg);
        assert_eq!(border.len(), 4);
        for primitive in &border {
            let Primitive::Quad(quad) = primitive else {
                panic!("a borda é feita de quads");
            };
            assert!(quad.color.r < bg.r && quad.color.g < bg.g && quad.color.b < bg.b);
            assert!(quad.rect.width == 1.0 || quad.rect.height == 1.0);
        }
    }

    use super::*;
    use porecatu_core::GroupColor;
    use porecatu_render::TextMeasurer;

    fn default_palette() -> ResolvedPalette {
        ResolvedPalette::from_config(&porecatu_config::Config::default())
    }

    fn default_term_palette() -> palette::ResolvedTermPalette {
        palette::ResolvedTermPalette::from_config(&porecatu_config::Config::default())
    }

    /// Fundo pintado de cada um dos três botões de janela, na ordem
    /// minimizar, maximizar/restaurar, fechar.
    fn window_button_backgrounds(
        hover: Option<tab_bar::WindowButtonHit>,
        pressed: Option<tab_bar::WindowButtonHit>,
    ) -> Vec<Color> {
        let mut out = Vec::new();
        paint_window_buttons(
            &TabBarStyle::DEFAULT,
            &default_palette(),
            800.0,
            52.0,
            false,
            hover,
            pressed,
            &mut out,
        );
        out.iter()
            .filter_map(|p| match p {
                Primitive::Quad(q) => Some(q.color),
                _ => None,
            })
            .collect()
    }

    /// ADR-0066: pressionado só com o cursor sobre o botão armado.
    #[test]
    fn pressed_window_button_paints_only_while_hovered() {
        use tab_bar::WindowButtonHit::{Close, Minimize};
        let pal = default_palette();

        let bgs = window_button_backgrounds(Some(Minimize), Some(Minimize));
        assert_eq!(bgs[0], pal.window_button_pressed_bg);
        assert_eq!(bgs[2], palette::TRANSPARENT);

        let bgs = window_button_backgrounds(Some(Close), Some(Close));
        assert_eq!(bgs[2], pal.window_close_pressed_bg);

        // Armado e arrastado para fora: volta ao repouso.
        let bgs = window_button_backgrounds(None, Some(Close));
        assert_eq!(bgs[2], palette::TRANSPARENT);

        // Armado num, cursor noutro: o outro só tem hover.
        let bgs = window_button_backgrounds(Some(Minimize), Some(Close));
        assert_eq!(bgs[0], pal.window_button_hover_bg);
        assert_eq!(bgs[2], palette::TRANSPARENT);
    }

    /// O quadrado arredondado de bookmark/engrenagem: o retângulo e o
    /// fundo pintados para um (hover, pressed).
    fn zone_button_quad(hovered: bool, pressed: bool) -> RoundedQuad {
        let style = TabBarStyle::DEFAULT;
        let rect = tab_bar::settings_button_rect(&style, 800.0, 52.0, false);
        let mut out = Vec::new();
        paint_zone_icon_button(
            rect,
            icon::SETTINGS,
            style.icon_em_size,
            (hovered, pressed),
            default_palette().chrome_icon,
            &style,
            &default_palette(),
            &mut out,
        );
        out.into_iter()
            .find_map(|p| match p {
                Primitive::RoundedQuad(q) => Some(q),
                _ => None,
            })
            .expect("o botão pinta um quad arredondado")
    }

    #[test]
    fn zone_button_is_a_borderless_rounded_square_revealed_by_hover_and_pressed() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();

        let rest = zone_button_quad(false, false);
        assert_eq!(rest.color, palette::TRANSPARENT, "em repouso some");
        let hover = zone_button_quad(true, false);
        assert_eq!(hover.color, pal.window_button_hover_bg);
        let pressed = zone_button_quad(true, true);
        assert_eq!(pressed.color, pal.window_button_pressed_bg);

        for quad in [rest, hover, pressed] {
            assert_eq!(quad.radius, style.tab_corner_radius);
            assert_eq!(quad.border_width, 0.0, "sem borda");
            assert_eq!(quad.rect.width, quad.rect.height, "quadrado");
            assert!(quad.rect.height < 52.0, "não ocupa a altura da barra");
        }
    }

    /// Pressed só vale com o cursor sobre o botão armado, como nos botões
    /// de janela (ADR-0066): o chamador passa `pressed = hovered && armado`.
    #[test]
    fn zone_buttons_paint_hover_only_on_the_hovered_one() {
        let style = TabBarStyle::DEFAULT;
        let rect_of = |hit| match hit {
            tab_bar::ZoneButtonHit::Sessions => {
                tab_bar::sessions_button_rect(&style, 800.0, 52.0, false)
            }
            tab_bar::ZoneButtonHit::Settings => {
                tab_bar::settings_button_rect(&style, 800.0, 52.0, false)
            }
            tab_bar::ZoneButtonHit::AppMenu => tab_bar::app_menu_button_rect(&style, false),
        };
        let sessions = rect_of(tab_bar::ZoneButtonHit::Sessions);
        let settings = rect_of(tab_bar::ZoneButtonHit::Settings);
        assert_eq!(sessions.width, settings.width);
        assert!(
            sessions.x + sessions.width <= settings.x,
            "lado a lado, sem sobrepor"
        );
    }

    /// Pedido do usuário, contra o "colapsado fica transparente" do
    /// RF-4.19: a cápsula é o que diz de que cor o grupo é, e sumir com
    /// ela no colapso tirava a única marca de cor justo quando o nome do
    /// grupo é tudo o que resta na barra.
    #[test]
    fn collapsed_group_still_paints_its_colored_capsule() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let term_pal = default_term_palette();
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
                &pal,
                &term_pal,
                bar_width,
                Overflow { scroll_offset: 0.0 },
                None,
                None,
                None,
                &AnimationClock::default(),
                Instant::now(),
                m,
                false,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                false,
                None,
                &HashMap::new(),
            );
            // `with_alpha` -- efeito de vidro (`style.capsule_alpha`) não
            // pinta mais a cor cheia do grupo, e sim ela com o alfa da
            // cápsula.
            let cor = with_alpha(pal.group_color(GroupColor::Cyan), style.capsule_alpha);
            out.iter()
                .filter(|p| match p {
                    Primitive::RoundedQuad(q) => {
                        q.radius == style.wrapper_corner_radius && q.color == cor
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

    /// Pinta a barra de `ws` (800px, sem arraste nem rename) com o hover e o
    /// botão armado dados; devolve o layout usado e as primitivas.
    fn paint_workspace(
        ws: &Workspace,
        hover: Option<tab_bar::TabBarHit>,
        pressed_bar_button: Option<tab_bar::TabBarHit>,
    ) -> (TabBarLayout, Vec<Primitive>) {
        let style = TabBarStyle::DEFAULT;
        let mut m = TextMeasurer::new();
        let layout = tab_bar::fit_width(ws, &style, 800.0, &mut m, false);
        let out = paint(
            &layout,
            ws,
            ws.active_tab(),
            &RenameState::Idle,
            &Selection::default(),
            None,
            &style,
            &default_palette(),
            &default_term_palette(),
            800.0,
            Overflow { scroll_offset: 0.0 },
            None,
            None,
            None,
            &AnimationClock::default(),
            Instant::now(),
            &mut m,
            false,
            false,
            None,
            None,
            hover,
            None,
            None,
            pressed_bar_button,
            false,
            None,
            &HashMap::new(),
        );
        (layout, out)
    }

    fn rounded_quads(out: &[Primitive]) -> Vec<&RoundedQuad> {
        out.iter()
            .filter_map(|p| match p {
                Primitive::RoundedQuad(q) => Some(q),
                _ => None,
            })
            .collect()
    }

    /// O rótulo do grupo é só o nome: sem o fundo da pílula (a cor do grupo
    /// vem da cápsula) e sem o chevron.
    #[test]
    fn the_group_label_paints_no_background_and_no_chevron() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let group = ws.group_tabs(&[a], "srv", GroupColor::Cyan).unwrap();

        for collapsed in [false, true] {
            ws.collapse_group(group, collapsed);
            let (_, out) = paint_workspace(&ws, None, None);
            let pill_fill = with_alpha(pal.group_color(GroupColor::Cyan), style.pill_glass_alpha);
            assert!(
                rounded_quads(&out).iter().all(|q| q.color != pill_fill),
                "sem fundo de pílula (colapsado: {collapsed})"
            );
            let chevrons = [icon::CHEVRON_DOWN.glyph, icon::CHEVRON_RIGHT.glyph];
            assert!(
                out.iter().all(|p| !matches!(
                    p,
                    Primitive::Text(t) if t.font == ICON_FONT && chevrons.contains(&t.text.as_str())
                )),
                "sem chevron (colapsado: {collapsed})"
            );
        }
    }

    /// Aba de dentro de grupo não leva borda; a solta mantém a dela.
    #[test]
    fn tabs_inside_a_group_have_no_border_and_loose_tabs_keep_theirs() {
        let style = TabBarStyle::DEFAULT;
        let mut ws = Workspace::new();
        let grouped = ws.append_tab("zsh", None);
        let loose = ws.append_tab("bash", None);
        ws.group_tabs(&[grouped], "srv", GroupColor::Cyan).unwrap();

        let (layout, out) = paint_workspace(&ws, None, None);
        let border_of = |id: TabId| {
            let rect = layout
                .groups
                .iter()
                .flat_map(|g| &g.tabs)
                .find(|t| t.id == id)
                .unwrap()
                .rect;
            // A trilha pinta deslocada pela origem dela na barra.
            let rect = Rect {
                x: rect.x + tab_bar::track_shift(&style, false, 0),
                ..rect
            };
            rounded_quads(&out)
                .into_iter()
                .find(|q| q.rect == rect && q.radius == style.tab_corner_radius)
                .map(|q| q.border_width)
                .expect("a aba pinta o próprio retângulo")
        };
        assert_eq!(border_of(grouped), 0.0);
        assert!(border_of(loose) > 0.0);
    }

    /// Colapsado: um ponto por aba, na cor do texto do rótulo; expandido,
    /// nenhum.
    #[test]
    fn a_collapsed_group_paints_one_dot_per_tab() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let mut ws = Workspace::new();
        let ids: Vec<TabId> = (0..3).map(|_| ws.append_tab("zsh", None)).collect();
        let group = ws.group_tabs(&ids, "srv", GroupColor::Cyan).unwrap();

        let dots = |out: &[Primitive]| {
            rounded_quads(out)
                .iter()
                .filter(|q| {
                    q.rect.width == style.indicator_dot_size
                        && q.rect.height == style.indicator_dot_size
                        && q.color == pal.group_new_tab_icon
                })
                .count()
        };
        let (_, expanded) = paint_workspace(&ws, None, None);
        assert_eq!(dots(&expanded), 0, "expandido");

        ws.collapse_group(group, true);
        let (_, collapsed) = paint_workspace(&ws, None, None);
        assert_eq!(dots(&collapsed), 3, "colapsado");
    }

    /// O "x" da aba revela um quadrado no hover e outro, mais escuro, no
    /// pressed -- como o "+".
    #[test]
    fn the_tab_close_button_reveals_a_square_on_hover_and_pressed() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let mut ws = Workspace::new();
        let id = ws.append_tab("zsh", None);
        let hit = Some(tab_bar::TabBarHit::CloseButton(id));

        let square_color = |hover, pressed| {
            let (layout, out) = paint_workspace(&ws, hover, pressed);
            let close = layout.groups[0].tabs[0].close_button;
            assert_eq!(close.height, style.close_hover_size());
            let square = Rect {
                x: close.x
                    + tab_bar::track_shift(&style, false, 0)
                    + (close.width - close.height) / 2.0,
                y: close.y,
                width: close.height,
                height: close.height,
            };
            rounded_quads(&out)
                .into_iter()
                .find(|q| q.rect == square)
                .map(|q| q.color)
                .expect("o botão pinta o quadrado")
        };
        assert_eq!(square_color(None, None), palette::TRANSPARENT);
        assert_eq!(square_color(hit, None), pal.window_button_hover_bg);
        assert_eq!(square_color(hit, hit), pal.window_button_pressed_bg);
        // Armado e arrastado para fora: volta ao repouso.
        assert_eq!(square_color(None, hit), palette::TRANSPARENT);
    }

    /// Sem botões de overflow: mesmo com a trilha estourada, nenhum chevron
    /// é pintado na barra.
    #[test]
    fn an_overflowing_track_paints_no_scroll_buttons() {
        let mut ws = Workspace::new();
        for _ in 0..30 {
            ws.append_tab("zsh", None);
        }
        let (layout, out) = paint_workspace(&ws, None, None);
        assert!(layout.content_width > 800.0, "a trilha tem de estourar");
        let chevrons = [icon::CHEVRON_LEFT.glyph, icon::CHEVRON_RIGHT.glyph];
        assert!(out.iter().all(|p| !matches!(
            p,
            Primitive::Text(t) if t.font == ICON_FONT && chevrons.contains(&t.text.as_str())
        )));
    }

    /// ADR-0062 §3, RF-18.10: com a imagem da janela exibida a barra não pinta
    /// o `Quad` opaco de `bar_background` sobre ela; sem imagem, pinta (o
    /// binário é o de hoje). O resto da barra -- aba, cápsula, ícones -- não
    /// depende da flag.
    #[test]
    fn the_bar_background_is_skipped_only_when_the_window_image_is_shown() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let term_pal = default_term_palette();
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = TextMeasurer::new();
        let bar_width = 800.0;
        let layout = tab_bar::fit_width(&ws, &style, bar_width, &mut m, false);

        let mut paint_bar = |window_image_shown: bool| {
            paint(
                &layout,
                &ws,
                ws.active_tab(),
                &RenameState::Idle,
                &Selection::default(),
                None,
                &style,
                &pal,
                &term_pal,
                bar_width,
                Overflow { scroll_offset: 0.0 },
                None,
                None,
                None,
                &AnimationClock::default(),
                Instant::now(),
                &mut m,
                false,
                false,
                None,
                None,
                None,
                None,
                None,
                None,
                window_image_shown,
                None,
                &HashMap::new(),
            )
        };
        let whole_bar_quads = |out: &[Primitive]| {
            out.iter()
                .filter(|p| {
                    matches!(
                        p,
                        Primitive::Quad(q)
                            if q.color == pal.bar_background
                                && q.rect.width == bar_width
                                && q.rect.height == bar_height(&style)
                    )
                })
                .count()
        };

        let without_image = paint_bar(false);
        let with_image = paint_bar(true);
        assert_eq!(whole_bar_quads(&without_image), 1, "sem imagem");
        assert_eq!(whole_bar_quads(&with_image), 0, "com imagem");
        // Só esse `Quad` some; todo o resto da barra continua igual.
        assert_eq!(without_image.len(), with_image.len() + 1);
        // `Primitive` não tem `PartialEq`: compara pelo `Debug`.
        assert_eq!(
            format!("{:?}", &without_image[1..]),
            format!("{:?}", &with_image[..])
        );
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
        let pal = default_palette();
        let term_pal = default_term_palette();
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = TextMeasurer::new();
        let bar_width = 800.0;
        let layout = tab_bar::fit_width(&ws, &style, bar_width, &mut m, false);
        let overflow = Overflow { scroll_offset: 0.0 };

        let out = paint(
            &layout,
            &ws,
            ws.active_tab(),
            &RenameState::Idle,
            &Selection::default(),
            None,
            &style,
            &pal,
            &term_pal,
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
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            &HashMap::new(),
        );

        let expected = bar_height(&style);
        let background = out
            .iter()
            .find_map(|p| match p {
                Primitive::Quad(q) if q.color == pal.bar_background => Some(q.rect),
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

    /// The `RoundedQuad` the app menu button paints for a (hover, pressed).
    fn app_menu_quad(hovered: bool, pressed: bool) -> (RoundedQuad, Vec<Primitive>) {
        let style = TabBarStyle::DEFAULT;
        let mut out = Vec::new();
        paint_app_menu_button(
            tab_bar::app_menu_button_rect(&style, false),
            (hovered, pressed),
            Some(ImageId::from_raw(1)),
            &style,
            &default_palette(),
            &mut out,
        );
        let quad = out
            .iter()
            .rev()
            .find_map(|p| match p {
                Primitive::RoundedQuad(q) => Some(*q),
                _ => None,
            })
            .expect("o botão pinta um quad arredondado");
        (quad, out)
    }

    #[test]
    fn app_menu_button_is_a_loose_tab_sized_square_with_active_tab_colors() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let (rest, _) = app_menu_quad(false, false);
        assert_eq!(
            rest.rect.width,
            style.tab_height + style.wrapper_padding * 2.0
        );
        assert_eq!(rest.rect.width, rest.rect.height, "quadrado");
        assert_eq!(rest.radius, style.tab_corner_radius);
        assert_eq!(rest.color, pal.tab_active_background);
        assert_eq!(rest.border_color, pal.tab_active_border);
        // Same top and height as a loose tab in the layout.
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = TextMeasurer::new();
        let layout = tab_bar::layout(&ws, &style, &mut m);
        let tab = &layout.groups[0].tabs[0];
        assert_eq!(rest.rect.y, tab.rect.y);
        assert_eq!(rest.rect.height, tab.rect.height);
    }

    #[test]
    fn app_menu_button_hover_brightens_and_pressed_drops_the_brightness_like_the_active_tab() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let (rest, _) = app_menu_quad(false, false);
        let (hover, _) = app_menu_quad(true, false);
        assert_eq!(
            hover.color,
            brighten(pal.tab_active_background, style.tab_hover_brightness)
        );
        assert_ne!(hover.color, rest.color);
        assert_eq!(hover.border_color, pal.tab_active_border);

        // Pressed: the bar's hover is suppressed while a tab is pressed, so
        // the active tab falls back to its plain (darker) background.
        let (pressed, _) = app_menu_quad(true, true);
        assert_eq!(pressed.color, pal.tab_active_background);
        assert_eq!(pressed.color, rest.color);
        assert_eq!(pressed.border_color, pal.tab_active_border);
    }

    #[test]
    fn app_menu_button_paints_the_icon_centered_on_top_only_when_loaded() {
        let style = TabBarStyle::DEFAULT;
        let (_, with_icon) = app_menu_quad(false, false);
        let Some(Primitive::Image { rect, image, .. }) = with_icon.last() else {
            panic!("o ícone vem por último, sobre o fundo");
        };
        assert_eq!(*image, ImageId::from_raw(1));
        let button = tab_bar::app_menu_button_rect(&style, false);
        assert_eq!(rect.width, style.icon_em_size);
        assert_eq!(rect.x + rect.width / 2.0, button.x + button.width / 2.0);
        assert_eq!(rect.y + rect.height / 2.0, button.y + button.height / 2.0);

        let mut without = Vec::new();
        paint_app_menu_button(
            button,
            (false, false),
            None,
            &style,
            &default_palette(),
            &mut without,
        );
        assert!(
            !without.iter().any(|p| matches!(p, Primitive::Image { .. })),
            "sem textura, só a caixa"
        );
    }

    /// The track starts after the fixed app menu button: the clip's left
    /// edge is `track_shift`, a `trilha_padding` past the button (the margin
    /// the button has on its other sides), and the first tab sits on it.
    #[test]
    fn the_track_clip_starts_after_the_app_menu_button() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let term_pal = default_term_palette();
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = TextMeasurer::new();
        let bar_width = 800.0;
        let layout = tab_bar::fit_width(&ws, &style, bar_width, &mut m, false);
        let out = paint(
            &layout,
            &ws,
            ws.active_tab(),
            &RenameState::Idle,
            &Selection::default(),
            None,
            &style,
            &pal,
            &term_pal,
            bar_width,
            Overflow { scroll_offset: 0.0 },
            None,
            None,
            None,
            &AnimationClock::default(),
            Instant::now(),
            &mut m,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            &HashMap::new(),
        );
        let clip = out
            .iter()
            .find_map(|p| match p {
                Primitive::PushClip(rect) => Some(*rect),
                _ => None,
            })
            .expect("recorte da trilha");
        let button = tab_bar::app_menu_button_rect(&style, false);
        assert_eq!(clip.x, tab_bar::track_shift(&style, false, 0));
        assert_eq!(
            clip.x,
            button.x + button.width + style.trilha_padding,
            "respiro entre o botão e o recorte"
        );
        assert_eq!(
            clip.width,
            tab_bar::trilha_width(&style, bar_width, false, 0)
        );

        // First tab: on the clip edge, so at rest it sits a `trilha_padding`
        // past the button, as it did before the margin moved into the clip.
        let first_tab = layout.groups[0].tabs[0].rect;
        let tab_quad = out
            .iter()
            .find_map(|p| match p {
                Primitive::RoundedQuad(q)
                    if q.color == pal.tab_active_background && q.rect.width == first_tab.width =>
                {
                    Some(q.rect)
                }
                _ => None,
            })
            .expect("fundo da aba ativa");
        assert_eq!(tab_quad.x, clip.x);
        assert_eq!(tab_quad.x, button.x + button.width + style.trilha_padding);
    }

    /// The whole bar for `ws`, with the cursor over `hover` and the app icon
    /// loaded -- the frame the pinned-tab tests read.
    fn paint_bar(
        ws: &Workspace,
        style: &TabBarStyle,
        hover: Option<tab_bar::TabBarHit>,
    ) -> Vec<Primitive> {
        let pal = default_palette();
        let term_pal = default_term_palette();
        let mut m = TextMeasurer::new();
        let bar_width = 800.0;
        let layout = tab_bar::fit_width(ws, style, bar_width, &mut m, false);
        paint(
            &layout,
            ws,
            ws.active_tab(),
            &RenameState::Idle,
            &Selection::default(),
            None,
            style,
            &pal,
            &term_pal,
            bar_width,
            Overflow { scroll_offset: 0.0 },
            None,
            None,
            None,
            &AnimationClock::default(),
            Instant::now(),
            &mut m,
            false,
            false,
            None,
            None,
            hover,
            None,
            None,
            None,
            false,
            Some(ImageId::from_raw(1)),
            &HashMap::new(),
        )
    }

    /// What `paint_pinned_tab` draws for one state: the box quad (the one on
    /// the rect itself, not a shadow layer), the icon alpha and the dot.
    struct PinnedPainted {
        quad: RoundedQuad,
        icon_alpha: Option<f32>,
        dot: Option<RoundedQuad>,
    }

    fn paint_pinned(tab: &PinnedTabPaint) -> PinnedPainted {
        let style = TabBarStyle::DEFAULT;
        let mut out = Vec::new();
        paint_pinned_tab(tab, &style, &default_palette(), &mut out);
        let quad = out
            .iter()
            .find_map(|p| match p {
                Primitive::RoundedQuad(q) if q.rect == tab.rect => Some(*q),
                _ => None,
            })
            .expect("a caixa da fixada");
        let icon_alpha = out.iter().find_map(|p| match p {
            Primitive::Image { alpha, .. } => Some(*alpha),
            _ => None,
        });
        let dot = out.iter().find_map(|p| match p {
            Primitive::RoundedQuad(q) if q.rect.width == style.indicator_dot_size => Some(*q),
            _ => None,
        });
        PinnedPainted {
            quad,
            icon_alpha,
            dot,
        }
    }

    fn pinned_state(rect: Rect) -> PinnedTabPaint {
        PinnedTabPaint {
            rect,
            active: false,
            exited: false,
            not_started: false,
            hovered: false,
            indicator: None,
            icon: Some(ImageId::from_raw(1)),
            icon_size: (1, 1),
        }
    }

    /// ADR-0088 §6: a rectangular icon is fitted (*contain*) in the square of
    /// `icon_em_size`, centered in the box, aspect kept, no mask radius.
    #[test]
    fn a_rectangular_pinned_icon_is_contained_and_centered() {
        let style = TabBarStyle::DEFAULT;
        let rect = tab_bar::pinned_rect(&style, false, 0);
        for (size, expected) in [
            ((200, 100), (style.icon_em_size, style.icon_em_size / 2.0)),
            ((100, 200), (style.icon_em_size / 2.0, style.icon_em_size)),
        ] {
            let mut out = Vec::new();
            let tab = PinnedTabPaint {
                icon_size: size,
                ..pinned_state(rect)
            };
            paint_pinned_tab(&tab, &style, &default_palette(), &mut out);
            let Some(Primitive::Image {
                rect: icon,
                mask,
                mask_radius,
                ..
            }) = out.last()
            else {
                panic!("the icon comes last");
            };
            assert_eq!((icon.width, icon.height), expected, "{size:?}");
            assert_eq!(icon.x + icon.width / 2.0, rect.x + rect.width / 2.0);
            assert_eq!(icon.y + icon.height / 2.0, rect.y + rect.height / 2.0);
            assert_eq!(mask, icon);
            assert_eq!(*mask_radius, 0.0);
        }
    }

    /// RF-21.21: the own texture of a pinned tab wins; without one, the app
    /// icon (drawn as a square).
    #[test]
    fn a_pinned_tab_draws_its_own_texture_or_the_app_icon() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("a", None);
        ws.pin_tab(a, 0);
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 30.0,
            height: 30.0,
        };
        let own = IconTexture {
            id: ImageId::from_raw(7),
            size: (64, 32),
        };
        let app = Some(ImageId::from_raw(1));

        let with_own = pinned_paint(&ws, a, rect, false, None, Some(own), app).unwrap();
        assert_eq!(with_own.icon, Some(ImageId::from_raw(7)));
        assert_eq!(with_own.icon_size, (64, 32));

        let fallback = pinned_paint(&ws, a, rect, false, None, None, app).unwrap();
        assert_eq!(fallback.icon, app);
        assert_eq!(fallback.icon_size, (1, 1));

        let nothing = pinned_paint(&ws, a, rect, false, None, None, None).unwrap();
        assert_eq!(nothing.icon, None);
    }

    /// ADR-0087 §2, the table of states: colors and border by state, hover
    /// brightens, `Exited` is inactive even when active, and `NotStarted` and
    /// `Exited` dim the icon to `.45`.
    #[test]
    fn a_pinned_box_paints_the_state_table_of_the_adr() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let rect = tab_bar::pinned_rect(&style, false, 0);
        let base = pinned_state(rect);

        let inactive = paint_pinned(&base);
        assert_eq!(inactive.quad.color, pal.tab_inactive_background);
        assert_eq!(inactive.quad.border_color, pal.tab_inactive_border);
        assert_eq!(inactive.quad.border_width, style.inactive_border_width);
        assert_eq!(inactive.quad.radius, style.tab_corner_radius);
        assert_eq!(inactive.icon_alpha, Some(1.0));
        assert!(inactive.dot.is_none());

        let active = paint_pinned(&PinnedTabPaint {
            active: true,
            ..pinned_state(rect)
        });
        assert_eq!(active.quad.color, pal.tab_active_background);
        assert_eq!(active.quad.border_color, pal.tab_active_border);
        assert_eq!(active.quad.border_width, style.active_border_width);
        assert_eq!(active.icon_alpha, Some(1.0));

        let hover = paint_pinned(&PinnedTabPaint {
            hovered: true,
            ..pinned_state(rect)
        });
        assert_eq!(
            hover.quad.color,
            brighten(pal.tab_inactive_background, style.tab_hover_brightness)
        );
        assert_eq!(hover.quad.border_color, pal.tab_inactive_border);

        // Dormant: the base state's box, the icon at `.45`.
        let dormant = paint_pinned(&PinnedTabPaint {
            active: true,
            not_started: true,
            ..pinned_state(rect)
        });
        assert_eq!(dormant.quad.color, pal.tab_active_background);
        assert_eq!(dormant.icon_alpha, Some(NOT_STARTED_LABEL_ALPHA as f32));
        assert_eq!(dormant.icon_alpha, Some(0.45));

        // Exited: inactive background **even when active**, the icon at `.45`.
        let exited = paint_pinned(&PinnedTabPaint {
            active: true,
            exited: true,
            ..pinned_state(rect)
        });
        assert_eq!(exited.quad.color, pal.tab_inactive_background);
        assert_eq!(exited.quad.border_color, pal.tab_inactive_border);
        assert_eq!(exited.icon_alpha, Some(0.45));
    }

    #[test]
    fn a_pinned_box_has_the_icon_centered_and_a_shadow_like_a_loose_tab() {
        let style = TabBarStyle::DEFAULT;
        let rect = tab_bar::pinned_rect(&style, false, 1);
        let mut out = Vec::new();
        paint_pinned_tab(&pinned_state(rect), &style, &default_palette(), &mut out);
        let Some(Primitive::Image {
            rect: icon,
            mask_radius,
            ..
        }) = out.last()
        else {
            panic!("o ícone vem por último, sobre o fundo");
        };
        assert_eq!(icon.width, style.icon_em_size);
        assert_eq!(icon.x + icon.width / 2.0, rect.x + rect.width / 2.0);
        assert_eq!(icon.y + icon.height / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(*mask_radius, 0.0);
        // The shadow layers sit behind the box, like the loose tab's.
        let box_at = out
            .iter()
            .position(|p| matches!(p, Primitive::RoundedQuad(q) if q.rect == rect))
            .unwrap();
        assert_eq!(
            box_at,
            SHADOW_LAYERS.len(),
            "as camadas da sombra vêm antes da caixa"
        );
        // No texture yet: the box alone.
        let mut bare = Vec::new();
        paint_pinned_tab(
            &PinnedTabPaint {
                icon: None,
                ..pinned_state(rect)
            },
            &style,
            &default_palette(),
            &mut bare,
        );
        assert!(!bare.iter().any(|p| matches!(p, Primitive::Image { .. })));
    }

    /// ADR-0087 §3: the dot is 6x6 in the top-right corner, `padding_right`
    /// from the top and right edges; bell wins (decided by `tab_indicator`)
    /// and an exited box never has one.
    #[test]
    fn the_pinned_box_dot_sits_in_the_top_right_corner() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let rect = tab_bar::pinned_rect(&style, false, 0);
        for (indicator, color) in [
            (Indicator::Activity, pal.activity_indicator),
            (Indicator::Bell, pal.bell_indicator),
        ] {
            let painted = paint_pinned(&PinnedTabPaint {
                indicator: Some(indicator),
                ..pinned_state(rect)
            });
            let dot = painted.dot.expect("o ponto");
            assert_eq!(dot.color, color);
            assert_eq!(dot.rect.width, 6.0);
            assert_eq!(dot.rect.height, 6.0);
            assert_eq!(dot.rect.y - rect.y, style.padding_right);
            assert_eq!(
                (rect.x + rect.width) - (dot.rect.x + dot.rect.width),
                style.padding_right
            );
            assert_eq!(dot.radius, 3.0, "círculo");
        }
        let exited = paint_pinned(&PinnedTabPaint {
            indicator: Some(Indicator::Bell),
            exited: true,
            ..pinned_state(rect)
        });
        assert!(exited.dot.is_none(), "nunca em caixa encerrada");

        // Through the whole bar: a tab with activity and bell paints the bell.
        let mut ws = Workspace::new();
        let a = ws.append_tab("a", None);
        ws.append_tab("b", None);
        ws.pin_tab(a, 0);
        ws.tab_mut(a).unwrap().mark_activity();
        ws.tab_mut(a).unwrap().mark_bell();
        let out = paint_bar(&ws, &style, None);
        assert!(
            out.iter().any(|p| matches!(
                p,
                Primitive::RoundedQuad(q)
                    if q.color == pal.bell_indicator && q.rect.width == style.indicator_dot_size
            )),
            "campainha vence atividade"
        );
        assert!(!out.iter().any(|p| matches!(
            p,
            Primitive::RoundedQuad(q) if q.color == pal.activity_indicator
        )));
    }

    /// The pinned boxes are painted in the fixed zone, outside the track's
    /// clip, one per pinned tab and in the order of the zone; the clip starts
    /// after them. Without pinned tabs the frame holds one icon only -- the
    /// app button's -- and the clip is where it always was.
    #[test]
    fn pinned_boxes_are_painted_outside_the_clip_and_the_bar_without_them_is_unchanged() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();

        let mut plain = Workspace::new();
        plain.append_tab("a", None);
        plain.append_tab("b", None);
        let before = paint_bar(&plain, &style, None);
        let icons = |out: &[Primitive]| {
            out.iter()
                .filter(|p| matches!(p, Primitive::Image { .. }))
                .count()
        };
        assert_eq!(icons(&before), 1);
        let clip_of = |out: &[Primitive]| {
            out.iter()
                .find_map(|p| match p {
                    Primitive::PushClip(rect) => Some(*rect),
                    _ => None,
                })
                .expect("recorte da trilha")
        };
        assert_eq!(clip_of(&before).x, tab_bar::track_shift(&style, false, 0));

        let mut ws = Workspace::new();
        let a = ws.append_tab("a", None);
        let b = ws.append_tab("b", None);
        ws.append_tab("c", None);
        ws.pin_tab(a, 0);
        ws.pin_tab(b, 1);
        let out = paint_bar(&ws, &style, None);
        assert_eq!(icons(&out), 3, "botão do app e duas fixadas");
        let clip_at = out
            .iter()
            .position(|p| matches!(p, Primitive::PushClip(_)))
            .unwrap();
        let clip = clip_of(&out);
        assert_eq!(clip.x, tab_bar::track_shift(&style, false, 2));
        assert_eq!(
            clip.width,
            tab_bar::trilha_width(&style, 800.0, false, 2),
            "a trilha encolhe pelas fixadas"
        );
        for i in 0..2 {
            let rect = tab_bar::pinned_rect(&style, false, i);
            let at = out
                .iter()
                .position(|p| matches!(p, Primitive::RoundedQuad(q) if q.rect == rect))
                .unwrap_or_else(|| panic!("caixa da fixada {i}"));
            assert!(at < clip_at, "fixada {i} fora do recorte");
            assert!(rect.x + rect.width <= clip.x, "fixada {i} antes da trilha");
        }
        // The last appended tab is the active one, so both boxes are inactive.
        let first = tab_bar::pinned_rect(&style, false, 0);
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::RoundedQuad(q) if q.rect == first && q.color == pal.tab_inactive_background
        )));

        // The cursor over the second box brightens it, and only it.
        let hovered = paint_bar(&ws, &style, Some(tab_bar::TabBarHit::Tab(b)));
        let second = tab_bar::pinned_rect(&style, false, 1);
        let bg_of = |out: &[Primitive], rect: Rect| {
            out.iter().find_map(|p| match p {
                Primitive::RoundedQuad(q) if q.rect == rect => Some(q.color),
                _ => None,
            })
        };
        assert_eq!(
            bg_of(&hovered, second),
            Some(brighten(
                pal.tab_inactive_background,
                style.tab_hover_brightness
            ))
        );
        assert_eq!(bg_of(&hovered, first), bg_of(&out, first));
    }

    /// The bar of `ws` with a tab drag in progress (`drag`) and `animations`
    /// at `now`.
    fn paint_bar_dragging(
        ws: &Workspace,
        style: &TabBarStyle,
        drag: DragGhost,
        animations: &AnimationClock,
        now: Instant,
    ) -> Vec<Primitive> {
        let pal = default_palette();
        let term_pal = default_term_palette();
        let mut m = TextMeasurer::new();
        let bar_width = 800.0;
        let layout = tab_bar::fit_width(ws, style, bar_width, &mut m, false);
        paint(
            &layout,
            ws,
            ws.active_tab(),
            &RenameState::Idle,
            &Selection::default(),
            None,
            style,
            &pal,
            &term_pal,
            bar_width,
            Overflow { scroll_offset: 0.0 },
            Some(drag),
            None,
            None,
            animations,
            now,
            &mut m,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            Some(ImageId::from_raw(1)),
            &HashMap::new(),
        )
    }

    /// Two pinned tabs (`a`, `b`) and a track tab `c`, the last one active.
    fn pinned_pair_and_a_track_tab() -> (Workspace, TabId, TabId, TabId) {
        let mut ws = Workspace::new();
        let a = ws.append_tab("a", None);
        let b = ws.append_tab("b", None);
        let c = ws.append_tab("c", None);
        ws.pin_tab(a, 0);
        ws.pin_tab(b, 1);
        (ws, a, b, c)
    }

    fn box_quad_at(out: &[Primitive], rect: Rect) -> Option<usize> {
        out.iter()
            .position(|p| matches!(p, Primitive::RoundedQuad(q) if q.rect == rect))
    }

    /// ADR-0087 §5: a box dragged out of the zone is the square box (the same
    /// paint, brightened like the track ghost), drawn **after** the clip of the
    /// track so it does not vanish on the border, and its slot is a hole.
    #[test]
    fn a_pinned_ghost_is_the_square_box_painted_past_the_clip() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let (ws, a, b, _c) = pinned_pair_and_a_track_tab();
        let ghost = DragGhost {
            tab: a,
            screen_x: 300.0,
            source: GhostSource::Pinned,
            pinned_order: ws.pinned().to_vec(),
            zone_highlight: None,
        };
        let out = paint_bar_dragging(
            &ws,
            &style,
            ghost,
            &AnimationClock::default(),
            Instant::now(),
        );

        let side = tab_bar::app_menu_button_size(&style);
        let ghost_rect = Rect {
            x: 300.0,
            y: tab_bar::pinned_rect(&style, false, 0).y,
            width: side,
            height: side,
        };
        let ghost_at = box_quad_at(&out, ghost_rect).expect("o fantasma quadrado");
        let pop_at = out
            .iter()
            .position(|p| matches!(p, Primitive::PopClip))
            .unwrap();
        assert!(ghost_at > pop_at, "fora do recorte da trilha");
        let Primitive::RoundedQuad(quad) = &out[ghost_at] else {
            unreachable!()
        };
        assert_eq!(quad.radius, style.tab_corner_radius);
        assert_eq!(
            quad.color,
            brighten(pal.tab_inactive_background, style.tab_hover_brightness),
            "o brilho do fantasma"
        );
        // Its own slot is the hole; the neighbour stays where it was.
        assert_eq!(
            box_quad_at(&out, tab_bar::pinned_rect(&style, false, 0)),
            None
        );
        assert!(box_quad_at(&out, tab_bar::pinned_rect(&style, false, 1)).is_some());
        let _ = b;
    }

    /// With the zone as the target: the highlight of the implicit run (its
    /// tint and a border at `.45`) goes under the boxes, the gap is the slot of
    /// the incoming tab and the neighbours sit past it.
    #[test]
    fn the_zone_target_paints_the_highlight_under_the_boxes_and_opens_the_gap() {
        let style = TabBarStyle::DEFAULT;
        let pal = default_palette();
        let (ws, a, b, c) = pinned_pair_and_a_track_tab();
        let order =
            tab_bar::pinned_preview_order(ws.pinned(), c, tab_bar::DragDrop::Pinned { pos: 1 });
        assert_eq!(order, [a, c, b]);
        let highlight = tab_bar::pinned_zone_rect(&style, false, order.len());
        let ghost = DragGhost {
            tab: c,
            screen_x: 90.0,
            source: GhostSource::Track(track_rect_of(&ws, &style, c)),
            pinned_order: order,
            zone_highlight: Some(highlight),
        };
        let out = paint_bar_dragging(
            &ws,
            &style,
            ghost,
            &AnimationClock::default(),
            Instant::now(),
        );

        let highlight_at = box_quad_at(&out, highlight).expect("o realce da zona");
        let Primitive::RoundedQuad(quad) = &out[highlight_at] else {
            unreachable!()
        };
        let implicit = pal.ungrouped_group_color();
        assert_eq!(quad.color.a, style.drag_highlight_tint_strength);
        assert_eq!(
            (quad.color.r, quad.color.g, quad.color.b),
            (implicit.r, implicit.g, implicit.b)
        );
        assert_eq!(quad.border_color.a, 0.45);
        assert_eq!(quad.border_width, 1.0);

        // Under every box of the zone.
        let first = box_quad_at(&out, tab_bar::pinned_rect(&style, false, 0)).unwrap();
        let last = box_quad_at(&out, tab_bar::pinned_rect(&style, false, 2)).unwrap();
        assert!(highlight_at < first && highlight_at < last);
        // `a` keeps slot 0, `b` slid to slot 2, and slot 1 is the gap: the
        // incoming tab has no box in the zone before the drop.
        assert!(box_quad_at(&out, tab_bar::pinned_rect(&style, false, 1)).is_none());
        let _ = (a, b);
    }

    /// Without a zone target the frame has no highlight, and the zone is the
    /// real one.
    #[test]
    fn a_track_target_paints_no_zone_highlight() {
        let style = TabBarStyle::DEFAULT;
        let (ws, _a, _b, c) = pinned_pair_and_a_track_tab();
        let ghost = DragGhost {
            tab: c,
            screen_x: 200.0,
            source: GhostSource::Track(track_rect_of(&ws, &style, c)),
            pinned_order: ws.pinned().to_vec(),
            zone_highlight: None,
        };
        let out = paint_bar_dragging(
            &ws,
            &style,
            ghost,
            &AnimationClock::default(),
            Instant::now(),
        );
        let zone = tab_bar::pinned_zone_rect(&style, false, 2);
        assert_eq!(box_quad_at(&out, zone), None);
        for i in 0..2 {
            assert!(box_quad_at(&out, tab_bar::pinned_rect(&style, false, i)).is_some());
        }
    }

    /// The neighbours slide (ADR-0087 §5): mid-slide a box is drawn between
    /// where it was and its slot.
    #[test]
    fn a_sliding_pinned_box_is_drawn_between_its_old_x_and_its_slot() {
        let style = TabBarStyle::DEFAULT;
        let (ws, a, b, c) = pinned_pair_and_a_track_tab();
        // The gap opened at slot 1: `b` goes from slot 1 to slot 2.
        let order =
            tab_bar::pinned_preview_order(ws.pinned(), c, tab_bar::DragDrop::Pinned { pos: 1 });
        let slot1 = tab_bar::pinned_rect(&style, false, 1);
        let slot2 = tab_bar::pinned_rect(&style, false, 2);
        let mut clock = AnimationClock::default();
        let now = Instant::now();
        clock.start_pinned_slide(
            std::collections::HashMap::from([
                (a, tab_bar::pinned_rect(&style, false, 0).x),
                (b, slot1.x),
            ]),
            std::time::Duration::from_millis(100),
            now,
        );
        let ghost = DragGhost {
            tab: c,
            screen_x: 90.0,
            source: GhostSource::Track(track_rect_of(&ws, &style, c)),
            pinned_order: order,
            zone_highlight: None,
        };
        let out = paint_bar_dragging(
            &ws,
            &style,
            ghost,
            &clock,
            now + std::time::Duration::from_millis(50),
        );
        // At the start `b` would be at slot 1 and at the end at slot 2: halfway
        // it is on neither.
        assert!(box_quad_at(&out, slot2).is_none());
        let halfway = Rect {
            x: slot1.x + (slot2.x - slot1.x) / 2.0,
            ..slot1
        };
        let drawn = out.iter().any(|p| {
            matches!(p, Primitive::RoundedQuad(q)
                if (q.rect.x - halfway.x).abs() < 0.5 && q.rect.width == halfway.width && q.rect.y == halfway.y)
        });
        assert!(drawn, "b is halfway between its old x and its slot");
    }

    /// The `TabRect` of track tab `id`, as `lib.rs` takes it from `base_layout`.
    fn track_rect_of(ws: &Workspace, style: &TabBarStyle, id: TabId) -> tab_bar::TabRect {
        let mut m = TextMeasurer::new();
        tab_bar::fit_width(ws, style, 800.0, &mut m, false)
            .groups
            .iter()
            .flat_map(|g| g.tabs.iter())
            .find(|t| t.id == id)
            .cloned()
            .expect("track tab")
    }

    /// `show_app_menu_button = false`: nada do botão é pintado e o recorte da
    /// trilha começa na borda da barra.
    #[test]
    fn a_hidden_app_menu_button_paints_nothing_and_the_clip_starts_at_the_edge() {
        let style = TabBarStyle {
            show_app_menu_button: false,
            ..TabBarStyle::DEFAULT
        };
        let pal = default_palette();
        let term_pal = default_term_palette();
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let mut m = TextMeasurer::new();
        let bar_width = 800.0;
        let layout = tab_bar::fit_width(&ws, &style, bar_width, &mut m, false);
        let out = paint(
            &layout,
            &ws,
            ws.active_tab(),
            &RenameState::Idle,
            &Selection::default(),
            None,
            &style,
            &pal,
            &term_pal,
            bar_width,
            Overflow { scroll_offset: 0.0 },
            None,
            None,
            None,
            &AnimationClock::default(),
            Instant::now(),
            &mut m,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            None,
            &HashMap::new(),
        );
        let button = tab_bar::app_menu_button_rect(&TabBarStyle::DEFAULT, false);
        assert!(
            !out.iter()
                .any(|p| matches!(p, Primitive::RoundedQuad(q) if q.rect == button)),
            "nenhum quad na caixa do botão"
        );
        assert!(
            !out.iter().any(|p| matches!(p, Primitive::Image { .. })),
            "nenhum ícone do app"
        );
        let clip = out
            .iter()
            .find_map(|p| match p {
                Primitive::PushClip(rect) => Some(*rect),
                _ => None,
            })
            .expect("recorte da trilha");
        assert_eq!(clip.x, 0.0);
        assert_eq!(
            clip.width,
            tab_bar::trilha_width(&style, bar_width, false, 0)
        );
    }
}
