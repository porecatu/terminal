// SPDX-License-Identifier: GPL-3.0-or-later

//! Barra de status -- faixa fixa no rodapé da janela (PRD-009, ADR-0048,
//! espec. §2.8). Camada `Chrome`, como a barra de busca: nenhuma camada
//! nova entra (ADR-0018).
//!
//! Diferente da busca, esta barra **encolhe a grade** em vez de sobrepor.
//! A razão está no ADR-0048 §1 e é o inverso do ADR-0041 pelo mesmo
//! argumento: o rodapé é onde o prompt ativo está, e a busca é
//! transitória enquanto esta barra é mobília -- sobrepor taparia o prompt
//! para sempre. Quem aplica isso é [`height`], consumida por
//! `paint::terminal_box_rect`.
//!
//! Layout puro e testável sem GPU, no molde de `search_bar`: geometria em
//! [`layout_status_bar`], pintura em [`paint_status_bar`], e nada aqui
//! chama `Instant::now` nem toca estado.

use porecatu_render::{Color, FontFace, Primitive, Quad, Rect, TextMeasurer, TextRun, icon};

use crate::chrome::ICON_FONT;
use crate::palette::ResolvedPalette;
use crate::tab_bar::TabBarStyle;

/// A barra usa a face **monoespaçada** (espec. §1.1, que já listava
/// "barra de status" entre os usos dela desde o ADR-0009).
const FONT: FontFace = FontFace::Mono { bold: false };

/// Codificação exibida na zona direita. Constante de propósito: o app
/// decodifica UTF-8 e só (ADR-0002), então um campo que consultasse algo
/// aqui mentiria sobre haver escolha.
const ENCODING: &str = "UTF-8";

/// Altura da barra em pixels lógicos, ou `0.0` quando ela está desligada
/// (`[appearance.status_bar] enabled = false`, RF-9.1).
///
/// **Fonte única.** Desligada, a barra não desenha *nem ocupa altura*, e
/// quem garante as duas metades disso é esta função -- `terminal_box_rect`
/// subtrai o que ela devolve, e `paint_status_bar` não é chamada quando
/// ela devolve zero. Recalcular `style.status_bar_height` na mão em outro
/// lugar reintroduz a cicatriz de `chrome::bar_height`, onde uma cópia
/// velha da fórmula deixou o recorte da trilha 12px curto.
pub fn height(style: &TabBarStyle) -> f32 {
    if style.status_bar_enabled {
        style.status_bar_height
    } else {
        0.0
    }
}

/// Papel de cada segmento -- decide a cor, e nada mais. A ordem de
/// desenho e a posição saem do layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentRole {
    /// Nome do shell: o único segmento colorido (§2.8), na cor de acento.
    Shell,
    /// Diretório atual. `stale` é o RF-9.4: o caminho é o de spawn, não
    /// veio de um OSC 7, e pode estar obsoleto -- sai com alfa reduzido.
    Cwd {
        stale: bool,
    },
    /// Branch do repositório do diretório da aba (ADR-0049). Único
    /// segmento com ícone -- e o ícone **é** o indicador de "há um
    /// repositório aqui": fora de um, o segmento inteiro some.
    GitBranch,
    /// Commits atrás/à frente do remoto (PRD-013, ADR-0052 §8). `clickable`
    /// é `false` quando a branch também tem commits locais à frente --
    /// `pull --ff-only` falharia por definição, e a barra não oferece um
    /// alvo que já sabe que não funciona (RF-13.9). O clique em si é a
    /// etapa 4; aqui é só o que muda a cor e a affordance de hover.
    AheadBehind {
        clickable: bool,
    },
    Group,
    /// RF-6.20/ADR-0053 §14: contagem de painéis da aba ativa, só com dois
    /// ou mais -- com um painel só, o segmento não existe (nem "1 painel",
    /// nem versão apagada dele). Fecha a zona esquerda. Não é clicável e
    /// não tem ícone.
    PaneCount,
    Encoding,
    System,
}

/// O que a barra mostra, resolvido a partir do estado **antes** do
/// layout. Manter isto separado é o que torna `layout_status_bar` pura:
/// ela não conhece `Workspace`, `Tab` nem `TabRuntime`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusBarContent {
    /// Nome do shell da aba ativa. Vazio esconde o segmento.
    pub shell: String,
    /// Diretório da aba ativa, já com `~` no lugar do home (RF-9.3) --
    /// ver [`abbreviate_home`]. Vazio esconde o segmento.
    pub cwd: String,
    /// RF-9.4: `true` quando `cwd` é o diretório de spawn porque nenhum
    /// OSC 7 chegou.
    pub cwd_is_stale: bool,
    /// Branch do repositório do diretório da aba, ou `None` fora de um
    /// repositório -- e aí nem o ícone aparece (ADR-0049 §5).
    pub git_branch: Option<String>,
    /// Commits atrás/à frente do remoto da branch acima (PRD-013,
    /// ADR-0052 §8), ou `None` sem commits novos -- e aí o segmento
    /// inteiro some, nem apagado nem "0 atrás" (RF-13.8).
    pub ahead_behind: Option<AheadBehindContent>,
    /// Nome do grupo da aba ativa. `None` em grupo implícito.
    pub group: Option<String>,
    /// RF-6.20: quantos painéis a aba ativa tem. `< 2` esconde o segmento
    /// por completo -- não há "1 painel" nem versão apagada dele, a mesma
    /// regra de ausência dos dois segmentos de Git.
    pub pane_count: usize,
    /// Sistema, ex. `"windows"`. Sem a versão do app: pedido do dono do
    /// produto depois de ver a barra em tela -- ela não muda entre
    /// execuções e não é o que se consulta de relance.
    pub system: String,
}

/// Conteúdo do segmento de commits atrás/à frente (PRD-013, ADR-0052
/// §8), já resolvido pelo layout puro do mapa de sincronização -- este
/// módulo não conhece o mapa nem o repositório, só o rótulo pronto e se
/// o clique (etapa 4) poderia integrar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AheadBehindContent {
    /// `"3 commits atrás"`, `"2 atrás, 1 à frente"` -- já com singular e
    /// plural corretos (`git::ahead_behind_label`).
    pub label: String,
    /// `false` quando há commits locais à frente: `pull --ff-only`
    /// falharia por definição (RF-13.9).
    pub clickable: bool,
}

/// Um segmento já posicionado, pronto para desenhar.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSegment {
    pub rect: Rect,
    /// Texto **já truncado** ao que cabe em `rect`.
    pub text: String,
    pub role: SegmentRole,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatusBarLayout {
    /// A faixa inteira, largura cheia da janela.
    pub bar_rect: Rect,
    /// Tamanho da fonte de todos os segmentos -- viaja no layout para o
    /// pintor não reler `TabBarStyle` e as duas metades não divergirem.
    pub font_size: f32,
    /// Em do ícone de repositório, pelo mesmo motivo do campo acima.
    pub icon_em: f32,
    pub segments: Vec<PlacedSegment>,
}

/// Substitui o prefixo do diretório home por `~` (RF-9.3). Função pura
/// para ser testável sem tocar o sistema de arquivos: quem descobre o
/// home é o chamador (`dirs::home_dir`).
///
/// Só troca quando o caminho **é** o home ou está dentro dele -- um
/// `/home/anabela` não vira `~bela` por o home ser `/home/ana`, e é por
/// isso que a comparação exige o separador logo depois do prefixo.
pub fn abbreviate_home(path: &str, home: Option<&str>) -> String {
    let Some(home) = home.filter(|h| !h.is_empty()) else {
        return path.to_owned();
    };
    let home = home.trim_end_matches(['/', '\\']);
    if !path.starts_with(home) {
        return path.to_owned();
    }
    match path[home.len()..].chars().next() {
        None => "~".to_owned(),
        Some(sep) if sep == '/' || sep == '\\' => format!("~{}", &path[home.len()..]),
        Some(_) => path.to_owned(),
    }
}

/// Em do ícone de repositório, em pixels. Não é valor novo: `0.8` é o
/// mesmo multiplicador que o botão de configurações já aplica sobre
/// `icon_em_size` (§1.1), e ele cai aqui porque a fonte da barra é
/// 10.5px contra os 13px do rótulo de aba -- `10.5 / 13 ≈ 0.8`.
///
/// É também a largura que o segmento reserva para o ícone: a glyph avança
/// **1 em** e o desenho preenche ~0.84 dela, então o resto vira a folga
/// entre o ícone e o nome da branch, sem `gap` inventado (ADR-0049 §4).
fn icon_em(style: &TabBarStyle) -> f32 {
    style.icon_em_size * 0.8
}

/// Largura de um texto na face mono, pela soma dos avanços por caractere.
///
/// **Não** usa `TextMeasurer::measure_width`, que shapa o texto inteiro a
/// cada chamada: isto roda por frame, e medir texto sem cache no caminho
/// quente é a armadilha de performance registrada do projeto (F3,
/// `fit_width`). `advance_em` é cacheada por `(char, face)` e satura
/// rápido -- o conjunto de caracteres de um caminho é pequeno e estável.
///
/// Somar avanços só divergiria de shapar o todo se houvesse ligadura ou
/// kerning entre caracteres vizinhos; as faces do projeto são recortadas
/// mantendo apenas `ccmp,locl,mark,mkmk`, então o avanço é aditivo por
/// construção -- a mesma garantia que `TextMeasurer::truncate` invoca.
fn text_width(measurer: &mut TextMeasurer, text: &str, size_px: f32) -> f32 {
    text.chars()
        .map(|ch| measurer.advance_em(ch, FONT) * size_px)
        .sum()
}

/// Corta `text` ao que cabe em `max_width`, pelo mesmo avanço acumulado de
/// [`text_width`]. Devolve o texto inteiro quando ele já cabe.
///
/// Corta **à direita**: num caminho longo, o começo (a raiz) é o que
/// situa, e o fim é o que se perde. Sem reticências -- elas custariam um
/// caractere da largura útil e o corte já é evidente.
fn truncate_to(measurer: &mut TextMeasurer, text: &str, size_px: f32, max_width: f32) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    let mut used = 0.0;
    let mut end = 0;
    for (i, ch) in text.char_indices() {
        let advance = measurer.advance_em(ch, FONT) * size_px;
        if used + advance > max_width {
            return text[..i].to_owned();
        }
        used += advance;
        end = i + ch.len_utf8();
    }
    text[..end].to_owned()
}

/// RF-6.20/ADR-0053 §14: rótulo por extenso da contagem de painéis, mesma
/// convenção de `git::ahead_behind_label`. Sempre plural por construção --
/// o corte em `< 2` já elimina o único caso em que o singular apareceria.
fn pane_count_label(count: usize) -> Option<String> {
    (count >= 2).then(|| format!("{count} painéis"))
}

/// Parâmetros de posicionamento comuns aos segmentos da zona esquerda --
/// agrupados para [`push_left`] não virar uma função de oito argumentos.
#[derive(Debug, Clone, Copy)]
struct Placement {
    /// `x` máximo que o segmento pode alcançar.
    limit: f32,
    bar_y: f32,
    bar_height: f32,
    size: f32,
    gap: f32,
}

/// Empurra um segmento da esquerda, truncado ao que cabe até
/// `place.limit`, e avança `x`. Segmento vazio ou sem espaço não entra na
/// lista -- um retângulo de largura zero atrapalharia o hit-test e a
/// árvore de acessibilidade sem desenhar nada.
fn push_left(
    segments: &mut Vec<PlacedSegment>,
    x: &mut f32,
    text: &str,
    role: SegmentRole,
    place: Placement,
    measurer: &mut TextMeasurer,
) {
    if text.is_empty() || *x >= place.limit {
        return;
    }
    let text = truncate_to(measurer, text, place.size, place.limit - *x);
    if text.is_empty() {
        return;
    }
    let width = text_width(measurer, &text, place.size);
    segments.push(PlacedSegment {
        rect: Rect {
            x: *x,
            y: place.bar_y,
            width,
            height: place.bar_height,
        },
        text,
        role,
    });
    *x += width + place.gap;
}

/// Posiciona a barra e os segmentos. Puro: nada aqui lê estado global.
///
/// A zona direita (`UTF-8`, sistema e versão) é medida primeiro e ancorada
/// à borda direita; a esquerda cresce a partir da borda esquerda, e o
/// **diretório é o segmento elástico** -- é ele que cede quando a janela
/// estreita, porque shell e grupo são curtos e identificam a aba.
pub fn layout_status_bar(
    content: &StatusBarContent,
    style: &TabBarStyle,
    logical_width: f32,
    logical_height: f32,
    measurer: &mut TextMeasurer,
) -> StatusBarLayout {
    let bar_height = height(style);
    let bar_rect = Rect {
        x: 0.0,
        y: logical_height - bar_height,
        width: logical_width,
        height: bar_height,
    };
    let size = style.status_bar_font_size;
    let gap = style.status_bar_gap;
    let padding = style.status_bar_padding_x;
    let mut segments = Vec::new();

    // Direita, da borda para dentro: sistema e versão, depois a
    // codificação. A ordem de desenho na tela fica `UTF-8` e então
    // sistema, que é a do mockup.
    let mut right_edge = logical_width - padding;
    for (text, role) in [
        (content.system.as_str(), SegmentRole::System),
        (ENCODING, SegmentRole::Encoding),
    ] {
        if text.is_empty() {
            continue;
        }
        let width = text_width(measurer, text, size);
        let x = right_edge - width;
        // Um segmento da direita que não caiba some por inteiro, em vez
        // de truncar: "UTF-" e "windows · 0." não informam nada.
        if x < padding {
            continue;
        }
        segments.push(PlacedSegment {
            rect: Rect {
                x,
                y: bar_rect.y,
                width,
                height: bar_height,
            },
            text: text.to_owned(),
            role,
        });
        right_edge = x - gap;
    }
    segments.reverse();

    // Esquerda: shell, diretório, grupo. O limite é onde a zona direita
    // começa, com um `gap` de folga entre as duas.
    let left_limit = (right_edge - gap).max(padding);
    let mut x = padding;

    push_left(
        &mut segments,
        &mut x,
        &content.shell,
        SegmentRole::Shell,
        Placement {
            limit: left_limit,
            bar_y: bar_rect.y,
            bar_height,
            size,
            gap,
        },
        measurer,
    );
    // O diretório cede espaço ao que vem depois dele -- a branch e o
    // grupo --, reservando o que os dois precisam antes de tomar o resto.
    // Sem isso, um caminho longo os empurraria para fora, e a aba
    // perderia tanto a identidade que a cápsula dá de relance quanto o
    // repositório em que está.
    let group = content.group.as_deref().filter(|g| !g.is_empty());
    let group_width = group
        .map(|g| text_width(measurer, g, size) + gap)
        .unwrap_or(0.0);
    let branch = content.git_branch.as_deref().filter(|b| !b.is_empty());
    let branch_width = branch
        .map(|b| icon_em(style) + text_width(measurer, b, size) + gap)
        .unwrap_or(0.0);
    let ahead_behind = content
        .ahead_behind
        .as_ref()
        .filter(|ab| !ab.label.is_empty());
    let ahead_behind_width = ahead_behind
        .map(|ab| icon_em(style) + text_width(measurer, &ab.label, size) + gap)
        .unwrap_or(0.0);
    let cwd_limit = (left_limit - group_width - branch_width - ahead_behind_width).max(x);
    push_left(
        &mut segments,
        &mut x,
        &content.cwd,
        SegmentRole::Cwd {
            stale: content.cwd_is_stale,
        },
        Placement {
            limit: cwd_limit,
            bar_y: bar_rect.y,
            bar_height,
            size,
            gap,
        },
        measurer,
    );
    // Branch logo depois do diretório: o repositório é propriedade dele,
    // e ler os dois juntos é o que faz sentido (ADR-0049 §4).
    if let Some(branch) = branch {
        let em = icon_em(style);
        let width = em + text_width(measurer, branch, size);
        if x + width <= left_limit {
            segments.push(PlacedSegment {
                rect: Rect {
                    x,
                    y: bar_rect.y,
                    width,
                    height: bar_height,
                },
                text: branch.to_owned(),
                role: SegmentRole::GitBranch,
            });
            x += width + gap;
        }
    }
    // Commits atrás/à frente logo depois da branch: a contagem é
    // propriedade dela, como a branch é propriedade do diretório
    // (ADR-0052 §8). Mesmo tratamento de "nunca truncado" da branch:
    // some por inteiro se não couber, em vez de mostrar metade de um
    // número.
    if let Some(ab) = ahead_behind {
        let em = icon_em(style);
        let width = em + text_width(measurer, &ab.label, size);
        if x + width <= left_limit {
            segments.push(PlacedSegment {
                rect: Rect {
                    x,
                    y: bar_rect.y,
                    width,
                    height: bar_height,
                },
                text: ab.label.clone(),
                role: SegmentRole::AheadBehind {
                    clickable: ab.clickable,
                },
            });
            x += width + gap;
        }
    }
    if let Some(group) = group {
        push_left(
            &mut segments,
            &mut x,
            group,
            SegmentRole::Group,
            Placement {
                limit: left_limit,
                bar_y: bar_rect.y,
                bar_height,
                size,
                gap,
            },
            measurer,
        );
    }
    // ADR-0053 §14: contagem de painéis fecha a zona esquerda, depois do
    // grupo.
    if let Some(label) = pane_count_label(content.pane_count) {
        push_left(
            &mut segments,
            &mut x,
            &label,
            SegmentRole::PaneCount,
            Placement {
                limit: left_limit,
                bar_y: bar_rect.y,
                bar_height,
                size,
                gap,
            },
            measurer,
        );
    }

    StatusBarLayout {
        bar_rect,
        font_size: size,
        icon_em: icon_em(style),
        segments,
    }
}

/// O que está sob um ponto da barra (ADR-0052 §9) -- irmã de
/// [`crate::search_bar::search_bar_hit`], e pelo mesmo motivo: projeta o
/// **mesmo** [`StatusBarLayout`] que [`paint_status_bar`] e a árvore de
/// acessibilidade (`access.rs`) consomem, para os três nunca discordarem
/// de onde o alvo está -- é a garantia de que o clique nunca acerta um
/// lugar e o desenho outro.
///
/// Só o indicador de commits atrás/à frente é alvo, e só quando clicável:
/// `clickable: false` (RF-13.9, branch divergida) não é alvo nenhum -- o
/// app não oferece um clique que já sabe que falha, mesmo sob o cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusBarHit {
    AheadBehind,
}

pub fn hit_test(layout: &StatusBarLayout, point: (f32, f32)) -> Option<StatusBarHit> {
    let (x, y) = point;
    layout.segments.iter().find_map(|s| {
        let inside = x >= s.rect.x
            && x < s.rect.x + s.rect.width
            && y >= s.rect.y
            && y < s.rect.y + s.rect.height;
        if inside && matches!(s.role, SegmentRole::AheadBehind { clickable: true }) {
            Some(StatusBarHit::AheadBehind)
        } else {
            None
        }
    })
}

/// Primitivas da barra, na camada `Chrome`. **Sem sombra**: a barra é
/// encostada e opaca, não flutua -- a lista de superfícies com sombra do
/// ADR-0032 §2 é exaustiva e não muda (mesma razão da barra de busca).
///
/// `ahead_behind_hovered` é a única entrada de mouse desta função pura:
/// `true` quando o cursor está sobre o retângulo do segmento clicável
/// (ADR-0052 §8, sublinhado sob o cursor -- a mesma affordance do
/// hyperlink OSC 8). Quem faz o hit-test é o chamador, com o layout já
/// pronto; esta etapa não mexe em cursor de mouse nem em precedência de
/// resize -- isso é a etapa 4, junto do clique em si.
pub fn paint_status_bar(
    layout: &StatusBarLayout,
    pal: &ResolvedPalette,
    ahead_behind_hovered: bool,
) -> Vec<Primitive> {
    if layout.bar_rect.height <= 0.0 {
        return Vec::new();
    }
    // O fundo é desenhado explicitamente mesmo quando coincide com a cor
    // de `clear` da janela: a config pode divergir das duas, e depender
    // da coincidência deixaria a barra invisível no primeiro tema que as
    // separasse.
    // **A barra não pinta fundo próprio, e isso é deliberado.** O `clear`
    // da janela já é a cor das barras, então um quad aqui seria redundante
    // -- e não só: ele cobriria a sombra do quadro do terminal, que é
    // desenhada na camada `Grid` (antes desta) e desce ~7,5px sobre a
    // faixa. Sem o quad, a sombra cai sobre a cor de fundo da janela e o
    // texto, que começa a 7,75px do topo, fica logo abaixo dela.
    //
    // Foi o que se viu em tela: com a barra encostada no quadro (§5 do
    // ADR-0048), o fundo opaco decepava a sombra numa linha reta.
    let mut out = Vec::new();

    // Mesma centragem vertical do contador da barra de busca: o texto é
    // ancorado pelo topo do `TextRun`, então a folga se divide em dois.
    let text_y = layout.bar_rect.y + (layout.bar_rect.height - layout.font_size) / 2.0;
    for segment in &layout.segments {
        let color = segment_color(segment.role, pal);
        // O ícone é centrado pela própria em (`centered_origin`, não a
        // mesma conta do texto: a face de ícones declara ascent = em e
        // descent = 0, e centrar como texto desenha o ícone baixo
        // demais), e o texto começa depois do avanço de 1 em -- mesmo
        // tratamento para os dois segmentos com ícone (ADR-0052 §8: a
        // mesma em do ícone de branch).
        let icon = match segment.role {
            SegmentRole::GitBranch => Some(&icon::GIT_BRANCH),
            SegmentRole::AheadBehind { .. } => Some(&icon::ARROW_DOWN),
            _ => None,
        };
        let text_x = if let Some(icon) = icon {
            let em = layout.icon_em;
            let icon_rect = Rect {
                width: em,
                ..segment.rect
            };
            out.push(Primitive::Text(TextRun {
                origin: icon.centered_origin(icon_rect, em),
                text: icon.glyph.to_owned(),
                font: ICON_FONT,
                size_px: em,
                color,
            }));
            segment.rect.x + em
        } else {
            segment.rect.x
        };
        out.push(Primitive::Text(TextRun {
            origin: (text_x, text_y),
            text: segment.text.clone(),
            font: FONT,
            size_px: layout.font_size,
            color,
        }));
        // ADR-0052 §8: sublinhado sob o cursor, a mesma affordance do
        // hyperlink OSC 8 (`paint_row_underlines`) -- mesma espessura,
        // ~10% da fonte, mínimo 1px. Só quando clicável: sem commits à
        // frente escondendo o botão, sublinhar prometeria um clique que
        // já se sabe que falha.
        if matches!(segment.role, SegmentRole::AheadBehind { clickable: true })
            && ahead_behind_hovered
        {
            let thickness = (layout.font_size * 0.1).max(1.0);
            out.push(Primitive::Quad(Quad {
                rect: Rect {
                    x: segment.rect.x,
                    y: text_y + layout.font_size,
                    width: segment.rect.width,
                    height: thickness,
                },
                color,
            }));
        }
    }
    out
}

fn segment_color(role: SegmentRole, pal: &ResolvedPalette) -> Color {
    match role {
        SegmentRole::Shell => pal.status_bar_shell,
        // RF-9.4: um degrau abaixo na escada de texto da §1.4, e não um
        // alfa sobre a cor de base -- a 10.5px o alfa apagava o caminho
        // em vez de marcá-lo (ADR-0048 §4).
        SegmentRole::Cwd { stale: true } => pal.status_bar_stale_cwd,
        // ADR-0052 §8: o segundo item colorido da barra -- clicável ou
        // não, o par de números é sempre acento; o que muda com
        // `clickable` é só a affordance de hover (sublinhado).
        SegmentRole::AheadBehind { .. } => pal.status_bar_ahead_behind,
        _ => pal.status_bar_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 900.0;
    const H: f32 = 600.0;

    fn content() -> StatusBarContent {
        StatusBarContent {
            shell: "pwsh".to_owned(),
            cwd: "~/projetos/porecatu".to_owned(),
            cwd_is_stale: false,
            git_branch: None,
            ahead_behind: None,
            group: Some("producao".to_owned()),
            pane_count: 1,
            system: "windows".to_owned(),
        }
    }

    fn pal() -> ResolvedPalette {
        ResolvedPalette::from_config(&porecatu_config::Config::default())
    }

    fn layout_with(content: &StatusBarContent, style: &TabBarStyle, width: f32) -> StatusBarLayout {
        let mut m = TextMeasurer::new();
        layout_status_bar(content, style, width, H, &mut m)
    }

    fn role_of(layout: &StatusBarLayout, role: SegmentRole) -> Option<&PlacedSegment> {
        layout.segments.iter().find(|s| s.role == role)
    }

    fn is_right(role: SegmentRole) -> bool {
        matches!(role, SegmentRole::Encoding | SegmentRole::System)
    }

    fn primitive_count_of_quads(layout: &StatusBarLayout) -> usize {
        paint_status_bar(layout, &pal(), false)
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(_)))
            .count()
    }

    fn text_count(layout: &StatusBarLayout) -> usize {
        paint_status_bar(layout, &pal(), false)
            .iter()
            .filter(|p| matches!(p, Primitive::Text(_)))
            .count()
    }

    #[test]
    fn disabled_bar_has_no_height_at_all() {
        let mut style = TabBarStyle::DEFAULT;
        style.status_bar_enabled = false;
        assert_eq!(height(&style), 0.0);
    }

    #[test]
    fn enabled_bar_height_comes_from_the_style() {
        assert_eq!(height(&TabBarStyle::DEFAULT), 26.0);
    }

    #[test]
    fn bar_sits_flush_against_the_bottom_of_the_window() {
        let layout = layout_with(&content(), &TabBarStyle::DEFAULT, W);
        let bar = layout.bar_rect;
        assert_eq!(bar.height, height(&TabBarStyle::DEFAULT));
        assert_eq!(bar.y + bar.height, H, "a barra encosta na base da janela");
        assert_eq!(bar.x, 0.0);
        assert_eq!(bar.width, W, "largura cheia");
        assert_eq!(
            primitive_count_of_quads(&layout),
            0,
            "nenhuma geometria: nem fundo, nem linha separadora (§2.8)"
        );
        assert_eq!(text_count(&layout), layout.segments.len());
    }

    #[test]
    fn the_bar_paints_no_background_so_the_terminal_shadow_survives() {
        // A faixa vive sobre o `clear` da janela, que já é a cor das
        // barras. Um quad opaco aqui cobriria a sombra do quadro do
        // terminal (camada `Grid`, desenhada antes desta), que desce sobre
        // o topo da faixa -- foi o que se viu em tela depois de o quadro
        // passar a encostar nela.
        let layout = layout_with(&content(), &TabBarStyle::DEFAULT, W);
        let primitives = paint_status_bar(&layout, &pal(), false);
        assert!(
            !primitives.iter().any(|p| matches!(p, Primitive::Quad(_))),
            "nenhuma geometria opaca: só os runs de texto"
        );
        assert!(
            primitives.iter().all(|p| matches!(p, Primitive::Text(_))),
            "a barra é texto e nada mais"
        );
    }

    #[test]
    fn text_clears_the_terminal_shadow() {
        // A sombra do quadro (`chrome::SHADOW_LAYERS`) desce
        // `spread + offset_y` abaixo dele -- 7,5px na camada mais externa.
        // O texto tem de começar depois disso, ou fica dentro da mancha.
        let shadow_reach = crate::chrome::SHADOW_LAYERS
            .iter()
            .map(|(spread, offset_y, _)| spread + offset_y)
            .fold(0.0_f32, f32::max);
        let layout = layout_with(&content(), &TabBarStyle::DEFAULT, W);
        let text_top = (layout.bar_rect.height - layout.font_size) / 2.0;
        assert!(
            text_top >= shadow_reach,
            "texto a {text_top}px do topo da faixa, sombra alcança {shadow_reach}px"
        );
    }

    #[test]
    fn disabled_bar_paints_nothing() {
        let mut style = TabBarStyle::DEFAULT;
        style.status_bar_enabled = false;
        let layout = layout_with(&content(), &style, W);
        assert!(paint_status_bar(&layout, &pal(), false).is_empty());
    }

    #[test]
    fn left_zone_keeps_shell_cwd_group_in_order_with_the_gap_between_them() {
        let style = TabBarStyle::DEFAULT;
        let layout = layout_with(&content(), &style, W);
        let left: Vec<_> = layout
            .segments
            .iter()
            .filter(|s| !is_right(s.role))
            .collect();
        assert_eq!(left.len(), 3);
        assert!(matches!(left[0].role, SegmentRole::Shell));
        assert!(matches!(left[1].role, SegmentRole::Cwd { .. }));
        assert!(matches!(left[2].role, SegmentRole::Group));
        assert_eq!(left[0].rect.x, style.status_bar_padding_x);
        for pair in left.windows(2) {
            let gap = pair[1].rect.x - (pair[0].rect.x + pair[0].rect.width);
            assert!(
                (gap - style.status_bar_gap).abs() < 0.01,
                "gap entre segmentos: {gap}"
            );
        }
    }

    #[test]
    fn right_zone_is_anchored_to_the_right_edge() {
        let style = TabBarStyle::DEFAULT;
        let layout = layout_with(&content(), &style, W);
        let system = role_of(&layout, SegmentRole::System).expect("segmento de sistema");
        assert!(
            ((system.rect.x + system.rect.width) - (W - style.status_bar_padding_x)).abs() < 0.01,
            "o último segmento encosta no padding da direita"
        );
        let encoding = role_of(&layout, SegmentRole::Encoding).expect("segmento de codificação");
        assert_eq!(encoding.text, ENCODING);
        assert!(
            encoding.rect.x < system.rect.x,
            "a codificação vem antes do sistema, como no mockup"
        );
    }

    #[test]
    fn narrow_window_truncates_the_cwd_and_keeps_the_group() {
        let mut long = content();
        long.cwd = "/um/caminho/absurdamente/longo/que/nao/cabe/de/jeito/nenhum".to_owned();
        let layout = layout_with(&long, &TabBarStyle::DEFAULT, 460.0);
        let cwd = role_of(&layout, SegmentRole::Cwd { stale: false }).expect("cwd");
        assert!(
            cwd.text.len() < long.cwd.len(),
            "o diretório é o segmento elástico e cede primeiro"
        );
        assert!(
            role_of(&layout, SegmentRole::Group).is_some(),
            "o grupo continua visível: ele identifica a aba e é curto"
        );
    }

    #[test]
    fn segments_never_overlap_the_right_zone() {
        let mut long = content();
        long.cwd = "/".to_owned() + &"x".repeat(400);
        let layout = layout_with(&long, &TabBarStyle::DEFAULT, 500.0);
        let leftmost_right = layout
            .segments
            .iter()
            .filter(|s| is_right(s.role))
            .map(|s| s.rect.x)
            .fold(f32::MAX, f32::min);
        for segment in layout.segments.iter().filter(|s| !is_right(s.role)) {
            assert!(
                segment.rect.x + segment.rect.width <= leftmost_right,
                "{:?} invade a zona direita",
                segment.role
            );
        }
    }

    #[test]
    fn empty_content_leaves_only_the_encoding() {
        // Sem aba ativa não há shell, diretório nem grupo a mostrar, e o
        // segmento de sistema é montado por quem chama. `UTF-8` fica:
        // é constante e continua verdade (ADR-0048 §3).
        let layout = layout_with(&StatusBarContent::default(), &TabBarStyle::DEFAULT, W);
        assert_eq!(layout.segments.len(), 1);
        assert_eq!(layout.segments[0].role, SegmentRole::Encoding);
        assert_eq!(
            layout.bar_rect.height,
            height(&TabBarStyle::DEFAULT),
            "a faixa continua ocupando altura mesmo sem nada a dizer"
        );
    }

    #[test]
    fn git_segment_carries_the_icon_and_the_branch_name() {
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        let style = TabBarStyle::DEFAULT;
        let layout = layout_with(&c, &style, W);
        let git = role_of(&layout, SegmentRole::GitBranch).expect("segmento de branch");
        assert_eq!(git.text, "main");

        // A largura reserva a em do ícone MAIS o nome -- o ícone avança 1
        // em, e é essa sobra sobre o desenho (~0.84 em) que faz a folga
        // entre os dois, sem `gap` inventado (ADR-0049 §4).
        let mut m = TextMeasurer::new();
        let name_width = text_width(&mut m, "main", style.status_bar_font_size);
        assert!((git.rect.width - (icon_em(&style) + name_width)).abs() < 0.01);
    }

    #[test]
    fn git_segment_sits_between_the_cwd_and_the_group() {
        // O repositório e propriedade do diretorio: ler os dois juntos e o
        // que faz sentido, e o grupo fecha a zona.
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);
        let order: Vec<_> = layout
            .segments
            .iter()
            .filter(|s| !is_right(s.role))
            .map(|s| s.role)
            .collect();
        assert_eq!(
            order,
            vec![
                SegmentRole::Shell,
                SegmentRole::Cwd { stale: false },
                SegmentRole::GitBranch,
                SegmentRole::Group,
            ]
        );
    }

    #[test]
    fn outside_a_repository_the_whole_segment_disappears() {
        // ADR-0049 §5: o ícone É a resposta a "estou num repositório?".
        // Nada de ícone apagado nem texto de "sem repositório".
        let mut c = content();
        c.git_branch = None;
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);
        assert!(role_of(&layout, SegmentRole::GitBranch).is_none());
        let primitives = paint_status_bar(&layout, &pal(), false);
        assert!(
            !primitives.iter().any(|p| matches!(
                p,
                Primitive::Text(run) if run.text == icon::GIT_BRANCH.glyph
            )),
            "nenhum ícone de repositório é desenhado"
        );
    }

    #[test]
    fn the_branch_paints_an_icon_run_and_a_text_run() {
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);
        let primitives = paint_status_bar(&layout, &pal(), false);
        let icon_run = primitives
            .iter()
            .find_map(|p| match p {
                Primitive::Text(run) if run.text == icon::GIT_BRANCH.glyph => Some(run),
                _ => None,
            })
            .expect("o ícone é desenhado");
        assert_eq!(icon_run.size_px, layout.icon_em, "a em, não o desenho");
        assert_eq!(
            icon_run.color,
            pal().status_bar_text,
            "a branch é cor de base -- o acento é do shell e do indicador de commits (§2.8)"
        );

        let git = role_of(&layout, SegmentRole::GitBranch).unwrap();
        let name_run = primitives
            .iter()
            .find_map(|p| match p {
                Primitive::Text(run) if run.text == "main" => Some(run),
                _ => None,
            })
            .expect("o nome é desenhado");
        assert!(
            (name_run.origin.0 - (git.rect.x + layout.icon_em)).abs() < 0.01,
            "o nome começa depois do avanço de 1 em do ícone"
        );
    }

    #[test]
    fn a_narrow_window_drops_the_branch_rather_than_truncating_it() {
        // Metade de um nome de branch nao informa nada -- e o cwd, que e o
        // elastico, ja cedeu antes de chegar aqui.
        let mut c = content();
        c.git_branch = Some("feat/uma-branch-de-nome-bem-longo".to_owned());
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, 400.0);
        if let Some(git) = role_of(&layout, SegmentRole::GitBranch) {
            assert_eq!(git.text, "feat/uma-branch-de-nome-bem-longo");
        }
    }

    #[test]
    fn ahead_behind_segment_carries_the_icon_and_the_label() {
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        c.ahead_behind = Some(AheadBehindContent {
            label: "3 commits atrás".to_owned(),
            clickable: true,
        });
        let style = TabBarStyle::DEFAULT;
        let layout = layout_with(&c, &style, W);
        let segment = role_of(&layout, SegmentRole::AheadBehind { clickable: true })
            .expect("segmento de commits atrás/à frente");
        assert_eq!(segment.text, "3 commits atrás");

        let mut m = TextMeasurer::new();
        let label_width = text_width(&mut m, "3 commits atrás", style.status_bar_font_size);
        assert!((segment.rect.width - (icon_em(&style) + label_width)).abs() < 0.01);
    }

    #[test]
    fn ahead_behind_sits_between_the_branch_and_the_group() {
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        c.ahead_behind = Some(AheadBehindContent {
            label: "3 commits atrás".to_owned(),
            clickable: true,
        });
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);
        let order: Vec<_> = layout
            .segments
            .iter()
            .filter(|s| !is_right(s.role))
            .map(|s| s.role)
            .collect();
        assert_eq!(
            order,
            vec![
                SegmentRole::Shell,
                SegmentRole::Cwd { stale: false },
                SegmentRole::GitBranch,
                SegmentRole::AheadBehind { clickable: true },
                SegmentRole::Group,
            ]
        );
    }

    #[test]
    fn no_new_commits_the_whole_segment_disappears() {
        // RF-13.8: nem apagado, nem "0 atrás" -- a ausência é a resposta,
        // como o ícone de repositório fora de um `.git` (ADR-0049 §5).
        let c = content();
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);
        assert!(role_of(&layout, SegmentRole::AheadBehind { clickable: true }).is_none());
        let primitives = paint_status_bar(&layout, &pal(), false);
        assert!(
            !primitives.iter().any(|p| matches!(
                p,
                Primitive::Text(run) if run.text == icon::ARROW_DOWN.glyph
            )),
            "sem commits novos, nem o ícone é desenhado"
        );
    }

    #[test]
    fn a_narrow_window_drops_ahead_behind_rather_than_truncating_it() {
        // Nunca truncado (ADR-0052 §8) -- mesma regra da branch.
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        c.ahead_behind = Some(AheadBehindContent {
            label: "2 atrás, 1 à frente".to_owned(),
            clickable: false,
        });
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, 380.0);
        if let Some(segment) = role_of(&layout, SegmentRole::AheadBehind { clickable: false }) {
            assert_eq!(segment.text, "2 atrás, 1 à frente");
        }
    }

    #[test]
    fn ahead_behind_is_accent_coloured_whether_clickable_or_not() {
        let pal = pal();
        assert_eq!(
            segment_color(SegmentRole::AheadBehind { clickable: true }, &pal),
            pal.status_bar_ahead_behind
        );
        assert_eq!(
            segment_color(SegmentRole::AheadBehind { clickable: false }, &pal),
            pal.status_bar_ahead_behind
        );
    }

    #[test]
    fn hover_underlines_the_clickable_segment_only() {
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        c.ahead_behind = Some(AheadBehindContent {
            label: "3 commits atrás".to_owned(),
            clickable: true,
        });
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);

        let not_hovered = paint_status_bar(&layout, &pal(), false);
        assert!(
            !not_hovered.iter().any(|p| matches!(p, Primitive::Quad(_))),
            "sem hover, sem sublinhado"
        );

        let hovered = paint_status_bar(&layout, &pal(), true);
        assert!(
            hovered.iter().any(|p| matches!(p, Primitive::Quad(_))),
            "sob o cursor, o sublinhado aparece"
        );
    }

    #[test]
    fn a_diverged_branch_is_not_underlined_even_when_hovered() {
        // RF-13.9: sem número à frente > 0, o `pull --ff-only` falharia
        // por definição -- sublinhar prometeria um clique que já se sabe
        // que não funciona.
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        c.ahead_behind = Some(AheadBehindContent {
            label: "2 atrás, 1 à frente".to_owned(),
            clickable: false,
        });
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);
        let primitives = paint_status_bar(&layout, &pal(), true);
        assert!(
            !primitives.iter().any(|p| matches!(p, Primitive::Quad(_))),
            "não clicável, sem sublinhado mesmo sob o cursor"
        );
    }

    #[test]
    fn hit_test_finds_the_clickable_indicator() {
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        c.ahead_behind = Some(AheadBehindContent {
            label: "3 commits atrás".to_owned(),
            clickable: true,
        });
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);
        let segment = role_of(&layout, SegmentRole::AheadBehind { clickable: true })
            .expect("indicador no layout");
        let inside = (
            segment.rect.x + segment.rect.width / 2.0,
            segment.rect.y + segment.rect.height / 2.0,
        );
        assert_eq!(hit_test(&layout, inside), Some(StatusBarHit::AheadBehind));
    }

    #[test]
    fn hit_test_misses_everywhere_else() {
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        c.ahead_behind = Some(AheadBehindContent {
            label: "3 commits atrás".to_owned(),
            clickable: true,
        });
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);
        assert_eq!(hit_test(&layout, (0.0, 0.0)), None, "fora da faixa");
        let branch = role_of(&layout, SegmentRole::GitBranch).expect("branch no layout");
        assert_eq!(
            hit_test(&layout, (branch.rect.x + 1.0, branch.rect.y + 1.0)),
            None,
            "a branch não é alvo, só o indicador"
        );
    }

    #[test]
    fn hit_test_never_targets_a_diverged_indicator() {
        // RF-13.9: sem número à frente > 0, o `pull --ff-only` falharia por
        // definição -- não há alvo, mesmo em cima do retângulo.
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        c.ahead_behind = Some(AheadBehindContent {
            label: "2 atrás, 1 à frente".to_owned(),
            clickable: false,
        });
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);
        let segment = role_of(&layout, SegmentRole::AheadBehind { clickable: false })
            .expect("indicador no layout");
        let inside = (
            segment.rect.x + segment.rect.width / 2.0,
            segment.rect.y + segment.rect.height / 2.0,
        );
        assert_eq!(hit_test(&layout, inside), None);
    }

    #[test]
    fn the_indicator_never_reaches_a_diagonal_resize_corner() {
        // ADR-0052 §9: "os cantos de redimensionamento diagonal nunca são
        // alcançados, porque o indicador nasce depois do padding e de três
        // segmentos" -- geometria provada, não raciocínio confiado. Os
        // quatro cantos do retângulo do indicador, testados contra a mesma
        // `resize_direction_at` que decide o cursor e o clique em `lib.rs`,
        // nunca podem cair numa das quatro direções diagonais.
        let mut c = content();
        c.git_branch = Some("main".to_owned());
        c.ahead_behind = Some(AheadBehindContent {
            label: "3 commits atrás".to_owned(),
            clickable: true,
        });
        let style = TabBarStyle::DEFAULT;
        let layout = layout_with(&c, &style, W);
        let segment = role_of(&layout, SegmentRole::AheadBehind { clickable: true })
            .expect("indicador no layout");
        let border = porecatu_config::Config::default()
            .appearance
            .window_controls
            .resize_border as f32;
        let corners = [
            (segment.rect.x, segment.rect.y),
            (segment.rect.x + segment.rect.width, segment.rect.y),
            (segment.rect.x, segment.rect.y + segment.rect.height),
            (
                segment.rect.x + segment.rect.width,
                segment.rect.y + segment.rect.height,
            ),
        ];
        for point in corners {
            let direction = crate::titlebar::resize_direction_at(point, W, H, false, border);
            assert!(
                !matches!(
                    direction,
                    Some(
                        winit::window::ResizeDirection::NorthWest
                            | winit::window::ResizeDirection::NorthEast
                            | winit::window::ResizeDirection::SouthWest
                            | winit::window::ResizeDirection::SouthEast
                    )
                ),
                "canto {point:?} do indicador cai em zona de resize diagonal: {direction:?}"
            );
        }
    }

    #[test]
    fn implicit_group_shows_no_group_segment() {
        let mut c = content();
        c.group = None;
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);
        assert!(role_of(&layout, SegmentRole::Group).is_none());
    }

    #[test]
    fn stale_cwd_is_dimmer_than_fresh_but_still_opaque() {
        // RF-9.4: se esta diferença sumir, a barra perde a razão de
        // existir. Mas o caminho apagado tem de continuar legível -- a
        // marca é um degrau na escada de texto, não um apagamento
        // (ADR-0048 §4).
        let pal = pal();
        let fresh = segment_color(SegmentRole::Cwd { stale: false }, &pal);
        let stale = segment_color(SegmentRole::Cwd { stale: true }, &pal);
        assert_ne!(fresh, stale, "os dois casos têm de se distinguir");
        assert_eq!(stale.a, 1.0, "opaco: nada de alfa apagando o caminho");
        assert!(
            relative_luminance(stale) < relative_luminance(fresh),
            "o apagado é o tom mais escuro dos dois"
        );
    }

    /// Luminância relativa WCAG, para os testes de contraste abaixo. Só
    /// existe em teste: nada no caminho de desenho precisa dela.
    fn relative_luminance(c: Color) -> f64 {
        fn ch(v: f64) -> f64 {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (x, y) = (relative_luminance(a), relative_luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn every_segment_clears_the_wcag_aa_floor_against_the_bar() {
        // A 10.5px, todo texto da barra é "texto pequeno": o mínimo é
        // 4.5:1. O tom que o desenho pedia (`#6b737e`, "Tênue") dava
        // 3.45:1 e ficava quase indistinguível do fundo -- relato do
        // dono do produto, e a mesma correção que o corpo do aviso já
        // tinha recebido (§2.14). Este teste é o que impede a regressão.
        let pal = pal();
        for role in [
            SegmentRole::Shell,
            SegmentRole::Cwd { stale: false },
            SegmentRole::Cwd { stale: true },
            SegmentRole::GitBranch,
            SegmentRole::AheadBehind { clickable: true },
            SegmentRole::Group,
            SegmentRole::PaneCount,
            SegmentRole::Encoding,
            SegmentRole::System,
        ] {
            // Contra `bar_background`: é o `clear` da janela, e portanto
            // o que de fato fica atrás da faixa agora que ela não pinta
            // fundo próprio.
            let ratio = contrast(segment_color(role, &pal), pal.bar_background);
            assert!(
                ratio >= 4.5,
                "{role:?} tem contraste {ratio:.2}:1 contra o fundo da barra, abaixo de 4.5:1"
            );
        }
    }

    #[test]
    fn shell_and_ahead_behind_are_the_only_coloured_segments() {
        // ADR-0052 §8 revisa o ADR-0048 §3: o nome do shell deixa de ser
        // o único item colorido -- o segmento de commits atrás/à frente
        // ganha o mesmo acento. A branch continua na cor de base.
        let pal = pal();
        assert_eq!(
            segment_color(SegmentRole::Shell, &pal),
            pal.status_bar_shell
        );
        assert_eq!(
            segment_color(SegmentRole::AheadBehind { clickable: true }, &pal),
            pal.status_bar_ahead_behind
        );
        for role in [
            SegmentRole::GitBranch,
            SegmentRole::Group,
            SegmentRole::PaneCount,
            SegmentRole::Encoding,
            SegmentRole::System,
        ] {
            assert_eq!(segment_color(role, &pal), pal.status_bar_text);
        }
    }

    #[test]
    fn pane_count_label_is_always_plural_and_absent_below_two() {
        assert_eq!(pane_count_label(0), None);
        assert_eq!(pane_count_label(1), None, "1 painel nunca aparece");
        assert_eq!(pane_count_label(2), Some("2 painéis".to_owned()));
        assert_eq!(pane_count_label(5), Some("5 painéis".to_owned()));
    }

    #[test]
    fn pane_count_segment_closes_the_left_zone_after_the_group() {
        let mut c = content();
        c.pane_count = 3;
        let layout = layout_with(&c, &TabBarStyle::DEFAULT, W);
        let order: Vec<_> = layout
            .segments
            .iter()
            .filter(|s| !is_right(s.role))
            .map(|s| s.role)
            .collect();
        assert_eq!(
            order,
            vec![
                SegmentRole::Shell,
                SegmentRole::Cwd { stale: false },
                SegmentRole::Group,
                SegmentRole::PaneCount,
            ]
        );
        let segment = role_of(&layout, SegmentRole::PaneCount).expect("segmento de contagem");
        assert_eq!(segment.text, "3 painéis");
    }

    #[test]
    fn a_single_pane_shows_no_pane_count_segment() {
        // RF-6.20: nem "1 painel", nem versão apagada -- a mesma regra dos
        // dois segmentos de Git.
        let layout = layout_with(&content(), &TabBarStyle::DEFAULT, W);
        assert!(role_of(&layout, SegmentRole::PaneCount).is_none());
    }

    #[test]
    fn home_becomes_tilde() {
        assert_eq!(abbreviate_home("/home/ana", Some("/home/ana")), "~");
        assert_eq!(
            abbreviate_home("/home/ana/projetos", Some("/home/ana")),
            "~/projetos"
        );
        assert_eq!(
            abbreviate_home(r"C:\Users\ana\projetos", Some(r"C:\Users\ana")),
            r"~\projetos"
        );
    }

    #[test]
    fn home_with_trailing_separator_still_matches() {
        assert_eq!(abbreviate_home("/home/ana/x", Some("/home/ana/")), "~/x");
    }

    #[test]
    fn sibling_of_home_is_not_abbreviated() {
        // `/home/anabela` não pode virar `~bela`: o prefixo bate como
        // string, mas não é o mesmo diretório.
        assert_eq!(
            abbreviate_home("/home/anabela/x", Some("/home/ana")),
            "/home/anabela/x"
        );
    }

    #[test]
    fn path_outside_home_and_unknown_home_are_untouched() {
        assert_eq!(
            abbreviate_home("/etc/hosts", Some("/home/ana")),
            "/etc/hosts"
        );
        assert_eq!(abbreviate_home("/etc/hosts", None), "/etc/hosts");
        assert_eq!(abbreviate_home("/etc/hosts", Some("")), "/etc/hosts");
    }
}
