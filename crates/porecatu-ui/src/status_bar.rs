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

use porecatu_render::{Color, FontFace, Primitive, Rect, TextMeasurer, TextRun};

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
    Group,
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
    /// Nome do grupo da aba ativa. `None` em grupo implícito.
    pub group: Option<String>,
    /// Sistema, ex. `"windows"`. Sem a versão do app: pedido do dono do
    /// produto depois de ver a barra em tela -- ela não muda entre
    /// execuções e não é o que se consulta de relance.
    pub system: String,
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
    // O diretório cede espaço ao grupo, que vem depois dele: reserva o
    // que o grupo precisa antes de tomar o resto. Sem isso, um caminho
    // longo empurraria o grupo para fora e a aba perderia a identidade
    // que a cápsula na barra de abas dá de relance.
    let group = content.group.as_deref().filter(|g| !g.is_empty());
    let group_width = group
        .map(|g| text_width(measurer, g, size) + gap)
        .unwrap_or(0.0);
    let cwd_limit = (left_limit - group_width).max(x);
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

    StatusBarLayout {
        bar_rect,
        font_size: size,
        segments,
    }
}

/// Primitivas da barra, na camada `Chrome`. **Sem sombra**: a barra é
/// encostada e opaca, não flutua -- a lista de superfícies com sombra do
/// ADR-0032 §2 é exaustiva e não muda (mesma razão da barra de busca).
pub fn paint_status_bar(layout: &StatusBarLayout, pal: &ResolvedPalette) -> Vec<Primitive> {
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
        out.push(Primitive::Text(TextRun {
            origin: (segment.rect.x, text_y),
            text: segment.text.clone(),
            font: FONT,
            size_px: layout.font_size,
            color: segment_color(segment.role, pal),
        }));
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
            group: Some("producao".to_owned()),
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
        paint_status_bar(layout, &pal())
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(_)))
            .count()
    }

    fn text_count(layout: &StatusBarLayout) -> usize {
        paint_status_bar(layout, &pal())
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
        let primitives = paint_status_bar(&layout, &pal());
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
        assert!(paint_status_bar(&layout, &pal()).is_empty());
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
            SegmentRole::Group,
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
    fn shell_is_the_only_coloured_segment() {
        let pal = pal();
        assert_eq!(
            segment_color(SegmentRole::Shell, &pal),
            pal.status_bar_shell
        );
        for role in [
            SegmentRole::Group,
            SegmentRole::Encoding,
            SegmentRole::System,
        ] {
            assert_eq!(segment_color(role, &pal), pal.status_bar_text);
        }
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
