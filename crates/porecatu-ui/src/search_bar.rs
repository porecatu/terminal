// SPDX-License-Identifier: GPL-3.0-or-later

//! Barra de busca (ADR-0041, PRD-011 RF-11.1 a RF-11.9), o sexto widget de
//! chrome -- a primeira superfície não modal do app (captura de teclado
//! **parcial**, ADR-0041 §3): diferente dos outros cinco widgets
//! (`dialog.rs`, `context_menu.rs`, `group_menu.rs`, `rename.rs`,
//! `group_editor.rs`), que consomem toda tecla enquanto abertos, esta só
//! reivindica um conjunto nomeado -- o resto cai para o passo 2 da cadeia
//! do ADR-0008 (`lib.rs::dispatch_keyboard_input`).
//!
//! Estado por janela (`WindowState::search`), amarrado à aba em que foi
//! aberta -- mesma classificação de `Selection` (ADR-0021 §8: "estado
//! efêmero de janela"), não persistido na sessão (ADR-0036 não ganha
//! campo). Geometria pura (`layout_search_bar`), pintura
//! (`paint_search_bar`) e hit-test (`search_bar_hit`) -- mesmo padrão de
//! `overlay.rs`/`group_editor.rs`, testável sem GPU e sem janela.
//!
//! Camada `Chrome` (ADR-0018 -- ver `frame.rs`), não `Popover`: a busca
//! convive com tooltip e menu de contexto por cima dela, mas fica acima da
//! grade.

use porecatu_core::TabId;
use porecatu_render::{Color, Primitive, Quad, Rect, RoundedQuad, TextMeasurer, TextRun, icon};
use porecatu_term::{Occurrence, OccurrenceSpan, SearchJob, SearchMode, SearchStep, Terminal};

use crate::chrome::{ICON_FONT, LABEL_FONT};
use crate::palette::{self, ResolvedPalette};
use crate::tab_bar::TabBarStyle;
use crate::text_field::TextFieldState;

/// Altura da barra (espec §2.21): o `input_height` do editor de grupo
/// (§2.10) -- mesmo valor, sem chave própria (a barra reusa o do editor).
pub const BAR_HEIGHT: f32 = 30.0;

/// Trilho/botão do alternador de regex (espec §1.5/§2.21 item 3): "a
/// primeira vez que esse token desenha no v1" -- já estava na tabela, sem
/// consumidor, e por isso sem chave no TOML (mesmo padrão de
/// `chrome::SHADOW_LAYERS`/`DRAG_HIGHLIGHT_BORDER_ALPHA`: valor de tabela
/// de tokens, não configurável por si).
const TOGGLE_TRACK_WIDTH: f32 = 34.0;
const TOGGLE_TRACK_HEIGHT: f32 = 19.0;
const TOGGLE_TRACK_RADIUS: f32 = 10.0;
const TOGGLE_TRACK_PADDING: f32 = 2.0;
const TOGGLE_KNOB_SIZE: f32 = 15.0;
const TOGGLE_ON_COLOR: Color = palette::hex(0x3f, 0x8f, 0x80);
const TOGGLE_OFF_COLOR: Color = palette::hex(0x2a, 0x30, 0x38);
const TOGGLE_KNOB_COLOR: Color = palette::hex(0xf0, 0xf3, 0xf6);

/// Largura de trabalho reservada para o contador ("3/17", "nenhum
/// resultado", "padrão inválido") -- sem chave de aparência, mesma nota de
/// `chrome::RENAME_FIELD_HEIGHT`-like: valor de layout, não de desenho.
const COUNTER_WIDTH: f32 = 92.0;

/// Espec §2.21 item 1 (mesmo componente do editor de grupo, §2.10).
/// `pub(crate)`: `lib.rs` reusa os dois pra achar o índice de caractere
/// sob o clique (mesmo padrão de `chrome::LABEL_FONT` para o rename).
pub(crate) const FIELD_FONT_SIZE: f32 = 13.0;
pub(crate) const FIELD_PADDING_X: f32 = 9.0;

/// Estado de uma busca aberta numa aba. `WindowState::search` guarda no
/// máximo um -- só a aba em que a busca foi aberta tem o widget; trocar de
/// aba fecha a busca (`lib.rs::redraw` confere `tab` a cada frame).
#[derive(Debug)]
pub struct SearchBarState {
    tab: TabId,
    field: TextFieldState,
    regex: bool,
    /// Erro do padrão atual (RF-11.4). `job`/`active` continuam com o
    /// último resultado válido enquanto isto é `Some` -- "padrão de regex
    /// inválido não apaga o último resultado válido".
    error: Option<String>,
    job: Option<SearchJob>,
    active: usize,
}

impl SearchBarState {
    /// Campo vazio, sem busca em andamento -- `restart` faz o primeiro
    /// disparo assim que o usuário digitar algo.
    pub fn new(tab: TabId) -> Self {
        Self {
            tab,
            field: TextFieldState::new(""),
            regex: false,
            error: None,
            job: None,
            active: 0,
        }
    }

    pub const fn tab(&self) -> TabId {
        self.tab
    }

    pub fn field(&self) -> &TextFieldState {
        &self.field
    }

    pub fn field_mut(&mut self) -> &mut TextFieldState {
        &mut self.field
    }

    pub const fn is_regex(&self) -> bool {
        self.regex
    }

    pub fn occurrences(&self) -> &[Occurrence] {
        self.job.as_ref().map_or(&[], |j| j.occurrences())
    }

    pub const fn active_index(&self) -> usize {
        self.active
    }

    /// RF-11.8: tela alternativa reduz o escopo à tela visível. `false`
    /// sem busca em andamento ainda (campo vazio).
    pub fn scope_reduced(&self) -> bool {
        self.job.as_ref().is_some_and(SearchJob::scope_reduced)
    }

    /// (Re)inicia a busca a partir do termo/modo atuais -- chamado a cada
    /// tecla que muda o campo ou o alternador de regex (RF-11.2:
    /// "recalcula a cada tecla"). Padrão inválido não descarta o último
    /// `job` válido (RF-11.4); campo vazio também não roda nada
    /// (`SearchJob::new` já trata isso como sucesso vazio, mas aqui o
    /// campo vazio é estado próprio do contador -- ver `counter_display`).
    pub fn restart(&mut self, terminal: &Terminal, lines_per_step: usize) {
        let mode = if self.regex {
            SearchMode::Regex
        } else {
            SearchMode::Literal
        };
        match terminal.start_search(self.field.text(), mode, lines_per_step) {
            Ok(job) => {
                self.error = None;
                self.job = Some(job);
                self.active = 0;
            }
            Err(err) => {
                self.error = Some(err.message().to_string());
            }
        }
    }

    pub fn toggle_regex(&mut self, terminal: &Terminal, lines_per_step: usize) {
        self.regex = !self.regex;
        self.restart(terminal, lines_per_step);
    }

    /// Varre um lote (ADR-0041 §"Riscos"). Devolve `true` se ainda há lote
    /// por varrer depois deste -- quem chama pede outro redraw nesse caso,
    /// sem precisar do relógio de animação (ADR-0041 §9 fecha a lista de
    /// consumidores dele em dois; isto não é animação, é o mesmo padrão de
    /// "saída do PTY continua chegando em frames seguintes").
    pub fn step(&mut self, terminal: &Terminal) -> bool {
        let Some(job) = &mut self.job else {
            return false;
        };
        if job.is_done() {
            return false;
        }
        terminal.step_search(job) == SearchStep::InProgress
    }

    /// `search.next`/`search.prev` (RF-11.5): circula nas duas pontas.
    /// `None` sem ocorrência nenhuma -- não há o que ativar.
    pub fn advance(&mut self, forward: bool) -> Option<Occurrence> {
        let total = self.occurrences().len();
        if total == 0 {
            return None;
        }
        self.active = if forward {
            (self.active + 1) % total
        } else {
            (self.active + total - 1) % total
        };
        Some(self.occurrences()[self.active])
    }

    /// RF-11.6: o texto do contador, e se ele deve sair na cor de erro.
    /// Campo vazio é um estado próprio, distinto de "nenhuma ocorrência" --
    /// o slot fica em branco em vez de "nenhum resultado". Padrão inválido
    /// (RF-11.4) mostra o rótulo fixo da espec, não a mensagem técnica do
    /// compilador de regex -- `self.error` guarda essa mensagem só para
    /// depuração, sem consumidor de UI nesta etapa.
    pub fn counter_display(&self) -> (String, bool) {
        if self.error.is_some() {
            return (PATTERN_INVALID_LABEL.to_string(), true);
        }
        if self.field.text().is_empty() {
            return (String::new(), false);
        }
        let total = self.occurrences().len();
        if total == 0 {
            (NO_RESULTS_LABEL.to_string(), false)
        } else {
            (format!("{}/{}", self.active + 1, total), false)
        }
    }
}

/// Texto exibido no lugar de "padrão inválido"/"nenhum resultado" -- fixo,
/// não vem de tokens de aparência (é conteúdo, não desenho).
pub const PATTERN_INVALID_LABEL: &str = "padrão inválido";
pub const NO_RESULTS_LABEL: &str = "nenhum resultado";
pub const ALT_SCREEN_SUFFIX: &str = " (tela alternativa)";

/// Corta uma posição absoluta de grade (`GridPos::line`, pode ser negativa
/// no scrollback) para a linha de viewport -- mesma fórmula de
/// `alacritty_terminal::term::point_to_viewport` (`line + display_offset`),
/// sem o tipo do motor atravessar a fronteira: `scroll_offset` e `rows` já
/// são campos próprios de `GridSnapshot`. `None` fora da vista.
fn to_viewport_row(line: i32, scroll_offset: usize, rows: usize) -> Option<usize> {
    let row = line + scroll_offset as i32;
    usize::try_from(row).ok().filter(|&r| r < rows)
}

/// ADR-0041 §4: "`porecatu-ui` recebe a lista, corta pela vista e resolve
/// a cor". Aqui só o corte -- a cor (par ativo/inativo) é resolvida no
/// pintor da grade (`paint.rs`), a partir do `active` desta função.
/// Ocorrência com qualquer ponta fora da vista não entra: um match que
/// atravessa a borda da tela é raro (só quebra de linha) e não vale a
/// complexidade de desenhar parcialmente.
///
/// Escreve direto em `out` (`GridSnapshot::occurrences`) em vez de devolver
/// um `Vec` novo -- mesma disciplina de reuso de buffer do ADR-0007 que o
/// resto do snapshot já segue; quem chama já limpou `out` (`TermEngine::
/// snapshot_into`) antes desta chamada.
pub fn occurrences_in_view(
    occurrences: &[Occurrence],
    active_index: usize,
    scroll_offset: usize,
    rows: usize,
    out: &mut Vec<OccurrenceSpan>,
) {
    for (index, occ) in occurrences.iter().enumerate() {
        let Some(start_row) = to_viewport_row(occ.start.line, scroll_offset, rows) else {
            continue;
        };
        let Some(end_row) = to_viewport_row(occ.end.line, scroll_offset, rows) else {
            continue;
        };
        out.push(OccurrenceSpan {
            start_row,
            start_col: occ.start.column,
            end_row,
            end_col: occ.end.column,
            active: index == active_index,
        });
    }
}

/// Quantas linhas da grade a barra cobre (ADR-0041 §1: "a ocorrência nunca
/// para numa linha coberta"). Arredonda para cima -- melhor reservar uma
/// linha a mais do que deixar a ativa meio coberta pela barra.
pub fn reserved_rows(cell_height: f32) -> usize {
    (BAR_HEIGHT / cell_height).ceil() as usize
}

/// Quantas linhas rolar (`TermScroll::Lines`) para `target_line` ficar
/// visível abaixo da barra. Positivo sobe no histórico -- mesma convenção
/// de `TermScroll::Lines` (`porecatu_term::scroll`). Zero se já visível.
pub fn scroll_delta_to_reveal(
    target_line: i32,
    current_scroll_offset: usize,
    rows: usize,
    reserved: usize,
) -> i32 {
    let visible_row = target_line + current_scroll_offset as i32;
    let reserved = reserved as i32;
    let rows = rows as i32;
    if visible_row >= reserved && visible_row < rows {
        return 0;
    }
    let desired_offset = reserved - target_line;
    desired_offset - current_scroll_offset as i32
}

/// Geometria pura da barra (espec §2.21) -- testável sem GPU/janela, e o
/// que a etapa 5 (acessibilidade) vai projetar (ADR-0043).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchBarLayout {
    pub bar_rect: Rect,
    pub field_rect: Rect,
    pub counter_rect: Rect,
    pub toggle_rect: Rect,
    pub prev_button: Rect,
    pub next_button: Rect,
    pub close_button: Rect,
}

/// `box_rect`: `paint::terminal_box_rect` -- a barra encosta no topo dele,
/// ocupando a largura inteira (os cantos superiores acompanham o raio do
/// quadro, espec §2.21). Botões/toggle/contador saem da direita para a
/// esquerda com o `internal_gap` ("gap: 8 da aba", `tab_bar.rs`); o campo
/// ocupa o que sobra, com o mesmo `padding_left`/`padding_right` da aba
/// nas duas pontas (sem valor novo: são os mesmos tokens da §2.5).
pub fn layout_search_bar(box_rect: Rect, style: &TabBarStyle) -> SearchBarLayout {
    let bar_rect = Rect {
        x: box_rect.x,
        y: box_rect.y,
        width: box_rect.width,
        height: BAR_HEIGHT,
    };
    let gap = style.internal_gap;
    let button_size = style.icon_button_width(style.icon_em_size);

    let right_edge = bar_rect.x + bar_rect.width - style.padding_right;
    let close_button = Rect {
        x: right_edge - button_size,
        y: bar_rect.y + (BAR_HEIGHT - button_size) / 2.0,
        width: button_size,
        height: button_size,
    };
    let next_button = Rect {
        x: close_button.x - button_size,
        ..close_button
    };
    let prev_button = Rect {
        x: next_button.x - button_size,
        ..close_button
    };

    let toggle_rect = Rect {
        x: prev_button.x - gap - TOGGLE_TRACK_WIDTH,
        y: bar_rect.y + (BAR_HEIGHT - TOGGLE_TRACK_HEIGHT) / 2.0,
        width: TOGGLE_TRACK_WIDTH,
        height: TOGGLE_TRACK_HEIGHT,
    };

    let counter_rect = Rect {
        x: toggle_rect.x - gap - COUNTER_WIDTH,
        y: bar_rect.y,
        width: COUNTER_WIDTH,
        height: BAR_HEIGHT,
    };

    let field_x = bar_rect.x + style.padding_left;
    let field_rect = Rect {
        x: field_x,
        y: bar_rect.y,
        width: (counter_rect.x - gap - field_x).max(0.0),
        height: BAR_HEIGHT,
    };

    SearchBarLayout {
        bar_rect,
        field_rect,
        counter_rect,
        toggle_rect,
        prev_button,
        next_button,
        close_button,
    }
}

fn push_toggle(rect: Rect, on: bool, out: &mut Vec<Primitive>) {
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: TOGGLE_TRACK_RADIUS,
        color: if on {
            TOGGLE_ON_COLOR
        } else {
            TOGGLE_OFF_COLOR
        },
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));
    let knob_x = if on {
        rect.x + rect.width - TOGGLE_TRACK_PADDING - TOGGLE_KNOB_SIZE
    } else {
        rect.x + TOGGLE_TRACK_PADDING
    };
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: Rect {
            x: knob_x,
            y: rect.y + (rect.height - TOGGLE_KNOB_SIZE) / 2.0,
            width: TOGGLE_KNOB_SIZE,
            height: TOGGLE_KNOB_SIZE,
        },
        radius: TOGGLE_KNOB_SIZE / 2.0,
        color: TOGGLE_KNOB_COLOR,
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));
}

#[allow(clippy::too_many_arguments)]
fn push_icon_button(
    rect: Rect,
    glyph: icon::Icon,
    icon_size: f32,
    hovered: bool,
    bar_background: Color,
    hover_brightness: f64,
    icon_color: Color,
    out: &mut Vec<Primitive>,
) {
    if hovered {
        out.push(Primitive::Quad(Quad {
            rect,
            color: crate::chrome::brighten(bar_background, hover_brightness),
        }));
    }
    out.push(Primitive::Text(TextRun {
        origin: glyph.centered_origin(rect, icon_size),
        text: glyph.glyph.to_string(),
        font: ICON_FONT,
        size_px: icon_size,
        color: icon_color,
    }));
}

/// Que botão/região da barra está sob o cursor -- `lib.rs` usa isto tanto
/// para hover (brilho) quanto para clique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchBarHit {
    Field,
    Toggle,
    Prev,
    Next,
    Close,
}

pub fn search_bar_hit(layout: &SearchBarLayout, point: (f32, f32)) -> Option<SearchBarHit> {
    let (x, y) = point;
    let hit = |r: Rect| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;
    if hit(layout.close_button) {
        Some(SearchBarHit::Close)
    } else if hit(layout.next_button) {
        Some(SearchBarHit::Next)
    } else if hit(layout.prev_button) {
        Some(SearchBarHit::Prev)
    } else if hit(layout.toggle_rect) {
        Some(SearchBarHit::Toggle)
    } else if hit(layout.field_rect) {
        Some(SearchBarHit::Field)
    } else {
        None
    }
}

/// Pinta a barra inteira (espec §2.21), camada `Chrome`. `hover`: qual
/// botão de ícone está sob o cursor, calculado fresco por frame (mesmo
/// padrão de `chrome::paint`'s `hover`) -- `None` durante qualquer outro
/// modo de captura.
#[allow(clippy::too_many_arguments)]
pub fn paint_search_bar(
    layout: &SearchBarLayout,
    state: &SearchBarState,
    style: &TabBarStyle,
    pal: &ResolvedPalette,
    term_pal: &palette::ResolvedTermPalette,
    hover: Option<SearchBarHit>,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let mut out = Vec::new();

    // Fundo e borda inferior (espec §2.21): os do aviso e do popover
    // (§1.2) -- `pal.editor_background`/`editor_border` já são esses
    // valores (`#1a1e25`/`#2e343e`), mesmo componente do editor de grupo.
    // Cantos superiores acompanham o raio do quadro; os inferiores retos --
    // sem primitiva de "raio por canto", aproxima com um raio só e um
    // retângulo reto por baixo cobrindo os dois cantos de baixo.
    let radius = style.terminal_frame_corner_radius;
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.bar_rect,
        radius,
        color: pal.editor_background,
        border_color: palette::TRANSPARENT,
        border_width: 0.0,
    }));
    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: layout.bar_rect.x,
            y: layout.bar_rect.y + radius,
            width: layout.bar_rect.width,
            height: (layout.bar_rect.height - radius).max(0.0),
        },
        color: pal.editor_background,
    }));
    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: layout.bar_rect.x,
            y: layout.bar_rect.y + layout.bar_rect.height - 1.0,
            width: layout.bar_rect.width,
            height: 1.0,
        },
        color: pal.editor_border,
    }));

    // Campo de texto -- mesmo componente do editor de grupo (§2.10),
    // sempre com foco automático (RF-11.1): não há outra região que tire
    // o foco de teclado dele, só o mouse pode clicar num botão sem digitar.
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.field_rect,
        radius: 5.0,
        color: pal.editor_input_background,
        border_color: pal.editor_input_border_focus,
        border_width: 1.0,
    }));
    let field = state.field();
    let buffer = field.text();
    let text_area = (layout.field_rect.width - FIELD_PADDING_X * 2.0).max(0.0);
    let text_width = measurer.measure_width(buffer, LABEL_FONT, FIELD_FONT_SIZE);
    let text_x = crate::tab_bar::scrolled_text_x(
        layout.field_rect.x,
        FIELD_PADDING_X,
        text_width,
        text_area,
    );
    let text_y = layout.field_rect.y + (layout.field_rect.height - FIELD_FONT_SIZE) / 2.0;
    out.push(Primitive::PushClip(layout.field_rect));
    let selection_range = field.selection_range();
    if let Some((start, end)) = selection_range {
        let sel_x0 = text_x + measurer.measure_width(&buffer[..start], LABEL_FONT, FIELD_FONT_SIZE);
        let sel_x1 = text_x + measurer.measure_width(&buffer[..end], LABEL_FONT, FIELD_FONT_SIZE);
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: sel_x0,
                y: layout.field_rect.y + 4.0,
                width: sel_x1 - sel_x0,
                height: layout.field_rect.height - 8.0,
            },
            color: term_pal.selection_background,
        }));
    }
    out.push(Primitive::Text(TextRun {
        origin: (text_x, text_y),
        text: buffer.to_string(),
        font: LABEL_FONT,
        size_px: FIELD_FONT_SIZE,
        color: pal.editor_input_text,
    }));
    if selection_range.is_none() {
        let cursor_width =
            measurer.measure_width(&buffer[..field.cursor()], LABEL_FONT, FIELD_FONT_SIZE);
        let caret_x =
            (text_x + cursor_width).min(layout.field_rect.x + layout.field_rect.width - 1.0);
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: caret_x,
                y: layout.field_rect.y + 4.0,
                width: 1.0,
                height: layout.field_rect.height - 8.0,
            },
            color: pal.editor_input_text,
        }));
    }
    out.push(Primitive::PopClip);

    // Contador -- RF-11.6, cor de erro (RF-11.4) ou tênue (§1.4).
    let (counter_text, is_error) = state.counter_display();
    let counter_text = if !is_error && state.scope_reduced() && !counter_text.is_empty() {
        format!("{counter_text}{ALT_SCREEN_SUFFIX}")
    } else {
        counter_text
    };
    const COUNTER_FONT_SIZE: f32 = 11.0;
    let counter_color = if is_error {
        pal.warning_severity_error
    } else {
        pal.warning_body_text
    };
    let counter_y = layout.counter_rect.y + (layout.counter_rect.height - COUNTER_FONT_SIZE) / 2.0;
    out.push(Primitive::Text(TextRun {
        origin: (layout.counter_rect.x, counter_y),
        text: counter_text,
        font: LABEL_FONT,
        size_px: COUNTER_FONT_SIZE,
        color: counter_color,
    }));

    // Alternador de regex (espec §1.5/§2.21 item 3).
    push_toggle(layout.toggle_rect, state.is_regex(), &mut out);

    // Três botões de ícone (espec §2.21 item 4).
    let icon_size = style.icon_em_size;
    let bar_bg = pal.editor_background;
    push_icon_button(
        layout.prev_button,
        icon::CHEVRON_LEFT,
        icon_size,
        hover == Some(SearchBarHit::Prev),
        bar_bg,
        style.tab_hover_brightness,
        pal.chrome_icon,
        &mut out,
    );
    push_icon_button(
        layout.next_button,
        icon::CHEVRON_RIGHT,
        icon_size,
        hover == Some(SearchBarHit::Next),
        bar_bg,
        style.tab_hover_brightness,
        pal.chrome_icon,
        &mut out,
    );
    push_icon_button(
        layout.close_button,
        icon::X,
        icon_size,
        hover == Some(SearchBarHit::Close),
        bar_bg,
        style.tab_hover_brightness,
        pal.chrome_icon,
        &mut out,
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_viewport_row_converts_and_clips() {
        assert_eq!(to_viewport_row(0, 0, 24), Some(0));
        assert_eq!(to_viewport_row(-5, 10, 24), Some(5));
        assert_eq!(to_viewport_row(-5, 2, 24), None, "acima do topo da vista");
        assert_eq!(to_viewport_row(30, 0, 24), None, "abaixo do fundo da vista");
    }

    #[test]
    fn occurrences_in_view_marks_the_active_one_and_drops_out_of_view() {
        let occurrences = vec![
            Occurrence {
                start: porecatu_term::GridPos {
                    line: -3,
                    column: 0,
                },
                end: porecatu_term::GridPos {
                    line: -3,
                    column: 2,
                },
            },
            Occurrence {
                start: porecatu_term::GridPos {
                    line: -100,
                    column: 0,
                },
                end: porecatu_term::GridPos {
                    line: -100,
                    column: 2,
                },
            },
        ];
        let mut spans = Vec::new();
        occurrences_in_view(&occurrences, 0, 10, 24, &mut spans);
        assert_eq!(spans.len(), 1, "a segunda está muito acima da vista");
        assert_eq!(spans[0].start_row, 7);
        assert!(spans[0].active);
    }

    #[test]
    fn scroll_delta_is_zero_when_already_visible_below_the_bar() {
        // linha 5, offset 0, reservado 2 linhas -- visível em row 5, dentro
        // de [2, 24).
        assert_eq!(scroll_delta_to_reveal(5, 0, 24, 2), 0);
    }

    #[test]
    fn scroll_delta_reveals_a_line_covered_by_the_bar() {
        // linha 0 cairia em row 0, coberta pelas duas linhas reservadas --
        // precisa subir 2 linhas no histórico para landar em row 2.
        assert_eq!(scroll_delta_to_reveal(0, 0, 24, 2), 2);
    }

    #[test]
    fn scroll_delta_reveals_a_line_above_the_current_viewport() {
        // linha -50 com offset 0 está bem acima da vista (row -50) --
        // precisa rolar até ela cair em row == reserved.
        assert_eq!(scroll_delta_to_reveal(-50, 0, 24, 2), 52);
    }

    #[test]
    fn layout_places_buttons_right_to_left_and_field_fills_the_rest() {
        let style = TabBarStyle::DEFAULT;
        let box_rect = Rect {
            x: 6.0,
            y: 52.0,
            width: 800.0,
            height: 400.0,
        };
        let layout = layout_search_bar(box_rect, &style);
        assert_eq!(layout.bar_rect.height, BAR_HEIGHT);
        assert!(layout.close_button.x > layout.next_button.x);
        assert!(layout.next_button.x > layout.prev_button.x);
        assert!(layout.prev_button.x > layout.toggle_rect.x);
        assert!(layout.toggle_rect.x > layout.counter_rect.x);
        assert!(layout.field_rect.width > 0.0);
        assert!(layout.field_rect.x + layout.field_rect.width <= layout.counter_rect.x);
    }

    #[test]
    fn hit_test_resolves_each_region() {
        let style = TabBarStyle::DEFAULT;
        let box_rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 400.0,
        };
        let layout = layout_search_bar(box_rect, &style);
        let center = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
        assert_eq!(
            search_bar_hit(&layout, center(layout.field_rect)),
            Some(SearchBarHit::Field)
        );
        assert_eq!(
            search_bar_hit(&layout, center(layout.toggle_rect)),
            Some(SearchBarHit::Toggle)
        );
        assert_eq!(
            search_bar_hit(&layout, center(layout.prev_button)),
            Some(SearchBarHit::Prev)
        );
        assert_eq!(
            search_bar_hit(&layout, center(layout.next_button)),
            Some(SearchBarHit::Next)
        );
        assert_eq!(
            search_bar_hit(&layout, center(layout.close_button)),
            Some(SearchBarHit::Close)
        );
        assert_eq!(search_bar_hit(&layout, (-10.0, -10.0)), None);
    }

    #[test]
    fn state_new_has_no_error_and_blank_counter() {
        let state = SearchBarState::new(TabId::new(0));
        assert_eq!(state.counter_display(), (String::new(), false));
        assert!(state.occurrences().is_empty());
    }

    #[test]
    fn advance_without_occurrences_is_none() {
        let mut state = SearchBarState::new(TabId::new(0));
        assert_eq!(state.advance(true), None);
    }
}
