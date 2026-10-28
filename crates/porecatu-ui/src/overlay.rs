// SPDX-License-Identifier: GPL-3.0-or-later

//! Pintura dos widgets de chrome que vivem fora das camadas Grid/Chrome
//! (ADR-0018): aviso do app (`Layer::Warning`), diálogo de confirmação
//! (`Layer::Modal`) e menu de contexto de aba + tooltip (`Layer::Popover`,
//! camada compartilhada por decisão explícita do ADR-0019). Como
//! `chrome.rs`, a geometria é calculada na hora de pintar -- sem função
//! pura de layout separada testável sem GPU, porque é pintura, não
//! geometria (mesma nota de `chrome.rs`/`paint.rs`); os tipos de estado
//! (`WarningStack`, `ConfirmDialog`, `ContextMenu`, `Hover`,
//! `GroupContextMenu`, `GroupEditor`, `MoveToGroupPopover`) continuam
//! puros e testados nos próprios módulos.
//!
//! Desde a F3 etapa 5 (ADR-0023), também o menu de contexto de grupo, o
//! editor de grupo -- quinto widget de chrome, também na camada popover --
//! e o popover de destino do `tab.move_to_group`. Os três nunca coexistem
//! entre si nem com o menu de aba/tooltip (mutuamente exclusivos,
//! garantido em `lib.rs`); o popover de destino é a primeira lista rolável
//! do chrome, mas sem gesto de roda do mouse nesta etapa -- só o realce
//! por teclado/clique arrasta a janela visível (ver nota da seção
//! correspondente). O editor não tem clip de linha múltipla para o nome:
//! como o campo de rename da aba, rola dentro de si mesmo mantendo o caret
//! visível, sem quebrar linha.
//!
//! **Sombra em camadas nos cinco widgets** (F4 etapa 6, ADR-0032 §2):
//! `chrome::push_shadow` -- a mesma técnica de três `RoundedQuad` pretos
//! empilhados que já sombreia a cápsula de grupo, a aba solta e o quadro
//! do terminal, sem primitiva nova em `porecatu-render`. Sem chave de
//! config própria (nenhuma seção `[appearance.*]` destes widgets tem
//! `shadow`, ao contrário de `[appearance.groups]`/`[appearance.
//! terminal_frame]`): a decisão do ADR-0032 é fixa, não alternável.
//! Corpo de aviso e diálogo também não quebra linha (`TextRun` é sempre uma linha);
//! onde a espec pede várias linhas, o texto trunca em uma só com o
//! `TextMeasurer`, em vez do "três linhas com reticências" literal.
//!
//! F4 etapa 2: toda geometria e cor daqui vem de `&porecatu_config::Config`
//! (parâmetro `config`) e `&palette::ResolvedPalette` (parâmetro `pal`),
//! passados por `lib.rs` -- não há um "OverlayStyle" próprio análogo a
//! `TabBarStyle`, porque nenhuma destas funções corre no caminho quente
//! (popover pinta no máximo uma vez por frame, quando visível): ler os
//! sub-structs de `Config` direto é suficiente, sem precisar de um cache
//! de campos "achatados". `style: &TabBarStyle` entra só onde o valor é
//! `icon_em_size` (`[appearance.tabs]`) -- o mesmo token de em de ícone que
//! `chrome.rs` usa, não duplicado aqui.

use porecatu_core::{GroupColor, Workspace};
use porecatu_locale::Catalog;
use porecatu_render::{
    Color, FontFace, Primitive, Quad, Rect, RoundedQuad, SansWeight, TextMeasurer, TextRun, icon,
};
use porecatu_session::named::EntryStatus;

use crate::chrome::{badge_size, centered_glyph, push_badge, push_shadow};
use crate::context_menu::ContextMenu;
use crate::dialog::{ConfirmDialog, DialogButton, LineTone, SecretFocus};
use crate::group_editor::{EditorRegion, GroupEditor};
use crate::group_menu::{self, EDITOR_ACTION_ORDER, GroupActionItem, GroupContextMenu};
use crate::messages::msg;
use crate::move_to_group::MoveToGroupPopover;
use crate::palette::{self, ResolvedPalette};
use crate::profile_menu::ProfileMenu;
use crate::session_picker::{Highlight, Mode, SessionPicker};
use crate::tab_bar::{self, TabBarStyle, rect_contains};
use crate::terminal_menu::{TerminalContextMenu, TerminalMenuItem};
use crate::toggle::{TOGGLE_TRACK_HEIGHT, TOGGLE_TRACK_WIDTH};
use crate::warning::{Severity, WarningStack};

pub(crate) const TITLE_FONT: FontFace = FontFace::Sans {
    weight: SansWeight::Medium,
};
// `pub(crate)`: `lib.rs::dispatch_group_editor_click` mede o mesmo texto
// com a mesma face pra achar o índice de caractere sob o clique
// (ADR-0035) -- tem que ser a mesma constante que pinta, nunca uma cópia.
pub(crate) const BODY_FONT: FontFace = FontFace::Sans {
    weight: SansWeight::Regular,
};

/// Rótulo de item de menu que cabe no orçamento de largura, ou truncado com
/// reticências (ADR-0056 §12). O texto vem de um arquivo que o usuário pode
/// escrever, e os menus têm largura fixa: sem o corte, um rótulo longo
/// invadiria a borda. **Só chama `truncate` quando a largura medida passa do
/// orçamento** -- todo rótulo dos arquivos do projeto cabe, e desenha
/// exatamente como antes; medir é um shaping, cortar é outro, e a armadilha
/// de medição por frame do CLAUDE.md pede que o segundo não rode à toa.
fn fit_label(label: String, budget: f32, font_size: f32, measurer: &mut TextMeasurer) -> String {
    if measurer.measure_width(&label, BODY_FONT, font_size) <= budget {
        return label;
    }
    measurer.truncate(&label, BODY_FONT, font_size, budget).0
}

/// Largura que sobra ao rótulo de um item: a do item menos o respiro dos dois
/// lados. Os menus que truncam não têm chip de atalho.
fn menu_label_budget(item_rect: Rect, item_padding_x: f32) -> f32 {
    (item_rect.width - item_padding_x * 2.0).max(0.0)
}

fn union(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.width).max(b.x + b.width);
    let y1 = (a.y + a.height).max(b.y + b.height);
    Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

// ---- aviso do app (espec §2.14, ADR-0014 canal 1, `[appearance.notices]`) ----

pub struct WarningEntry {
    /// Índice em `WarningStack::items()` -- é o que `App::dismiss` precisa
    /// pra remover o item certo.
    pub index: usize,
    pub rect: Rect,
    pub close_button: Rect,
}

pub struct WarningLayout {
    /// Em ordem de desenho: mais recente no topo (espec §2.14).
    pub entries: Vec<WarningEntry>,
    /// União dos retângulos -- usada por `lib.rs` pra saber se o cursor
    /// está sobre a pilha e pausar o temporizador da informação.
    pub stack_rect: Rect,
}

pub fn layout_warnings(
    stack: &WarningStack,
    config: &porecatu_config::Config,
    bar_height: f32,
    content_width: f32,
) -> WarningLayout {
    let notices = &config.appearance.notices;
    let width = notices.width as f32;
    let padding_x = notices.padding_x as f32;
    let padding_y = notices.padding_y as f32;
    let gap = notices.gap as f32;
    let margin_right = notices.stack_margin_right as f32;
    let margin_top = notices.stack_margin_top as f32;
    let title_size = notices.font_size as f32;
    let line_height = notices.line_height as f32;
    let close_size = notices.close_button_size as f32;

    let x = content_width - margin_right - width;
    let mut y = bar_height + margin_top;
    let mut entries = Vec::new();
    for (index, _) in stack.items().iter().enumerate().rev() {
        // Duas linhas -- título e corpo, mesma anatomia de
        // `layout_dialog` (`title_size + gap + body_size + gap +
        // button_height`) -- `paint_warnings` desenha o corpo em
        // `title_y + line_height`, então a altura precisa reservar essa
        // segunda linha também, não só a primeira.
        let height = padding_y * 2.0 + title_size.max(line_height) + line_height;
        let rect = Rect {
            x,
            y,
            width,
            height,
        };
        let close_button = Rect {
            x: rect.x + rect.width - padding_x - close_size + 4.0,
            y: rect.y + padding_y - 2.0,
            width: close_size,
            height: close_size,
        };
        entries.push(WarningEntry {
            index,
            rect,
            close_button,
        });
        y += height + gap;
    }
    let stack_rect = entries
        .iter()
        .fold(None, |acc: Option<Rect>, e| {
            Some(acc.map_or(e.rect, |a| union(a, e.rect)))
        })
        .unwrap_or(Rect {
            x,
            y: bar_height + margin_top,
            width: 0.0,
            height: 0.0,
        });
    WarningLayout {
        entries,
        stack_rect,
    }
}

pub fn paint_warnings(
    layout: &WarningLayout,
    stack: &WarningStack,
    config: &porecatu_config::Config,
    style: &TabBarStyle,
    pal: &ResolvedPalette,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let notices = &config.appearance.notices;
    let corner_radius = notices.corner_radius as f32;
    let severity_bar_width = notices.severity_bar_width as f32;
    let padding_x = notices.padding_x as f32;
    let title_size = notices.font_size as f32;
    let body_size = notices.body_font_size as f32;
    let close_size = notices.close_button_size as f32;
    let line_height = notices.line_height as f32;
    // Em, não desenho -- mesmo tamanho do botão de fechar da aba, que a
    // espec. §2.15 diz ser "o mesmo botão". Ver `porecatu_render::icon`.
    let close_icon_size = style.icon_em_size;

    let mut out = Vec::new();
    for entry in &layout.entries {
        let warning = &stack.items()[entry.index];
        push_shadow(&mut out, entry.rect, corner_radius);
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: entry.rect,
            radius: corner_radius,
            color: pal.popover_background,
            border_color: pal.popover_border,
            border_width: 1.0,
        }));
        let severity_color = match warning.severity {
            Severity::Error => pal.warning_severity_error,
            Severity::Warning => pal.warning_severity_warning,
            Severity::Info => pal.warning_severity_info,
        };
        // Recortada com o mesmo raio do card (não um `Quad` reto), pra
        // curvar junto com os dois cantos esquerdos em vez de cruzá-los.
        out.push(Primitive::PushClip(Rect {
            x: entry.rect.x,
            y: entry.rect.y,
            width: severity_bar_width,
            height: entry.rect.height,
        }));
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect: entry.rect,
            radius: corner_radius,
            color: severity_color,
            border_color: palette::TRANSPARENT,
            border_width: 0.0,
        }));
        out.push(Primitive::PopClip);

        let text_x = entry.rect.x + severity_bar_width + padding_x;
        let title_y = entry.rect.y + notices.padding_y as f32;
        out.push(Primitive::Text(TextRun {
            origin: (text_x, title_y),
            text: warning.title.clone(),
            font: TITLE_FONT,
            size_px: title_size,
            color: pal.warning_title_text,
        }));

        let body_max_width =
            (entry.rect.width - severity_bar_width - padding_x * 2.0 - close_size).max(0.0);
        let (body, _) = measurer.truncate(&warning.body, BODY_FONT, body_size, body_max_width);
        out.push(Primitive::Text(TextRun {
            origin: (text_x, title_y + line_height),
            text: body,
            font: BODY_FONT,
            size_px: body_size,
            color: pal.warning_body_text,
        }));

        out.push(centered_glyph(
            icon::X,
            entry.close_button,
            close_icon_size,
            pal.chrome_icon,
        ));
    }
    out
}

/// Acha, em ordem de desenho, o aviso ou o botão de fechar sob `point`
/// (coordenadas lógicas da janela). Botão de fechar tem prioridade -- o
/// mesmo critério do `TabBarHit` da barra de abas.
pub enum WarningHit {
    Close(usize),
    Body,
}

pub fn hit_test_warnings(layout: &WarningLayout, point: (f32, f32)) -> Option<WarningHit> {
    for entry in &layout.entries {
        if rect_contains(entry.close_button, point) {
            return Some(WarningHit::Close(entry.index));
        }
    }
    for entry in &layout.entries {
        if rect_contains(entry.rect, point) {
            return Some(WarningHit::Body);
        }
    }
    None
}

// ---- diálogo de confirmação (espec §2.15, ADR-0014, `[appearance.dialog]`) ----

pub struct DialogLayout {
    pub modal_rect: Rect,
    pub cancel_rect: Rect,
    /// O botão do meio do diálogo de três saídas (RF-16.4).
    pub discard_rect: Option<Rect>,
    pub confirm_rect: Rect,
    /// O campo mascarado, só no diálogo de segredo (ADR-0075 §2).
    pub field_rect: Option<Rect>,
    /// A linha da caixa "Lembrar" inteira (a área do clique) e o trilho da
    /// alternância dentro dela.
    pub remember_rect: Option<Rect>,
    pub toggle_rect: Option<Rect>,
    /// A nota esmaecida abaixo da caixa, sem cofre (RF-20.14).
    pub note_rect: Option<Rect>,
}

/// O que `paint_dialog` precisa do diálogo de segredo além do que o
/// `ConfirmDialog` carrega (foco e caixa já estão em `dialog.secret`).
pub struct SecretView<'a> {
    /// O campo mascarado: o único pedaço do diálogo que não é dado puro.
    pub field: &'a crate::masked_field::MaskedFieldState,
    /// A cor da seleção do campo é a do terminal (a mesma da tela de
    /// configurações).
    pub selection_color: Color,
}

/// Fonte do campo mascarado: os pontos são texto da Iosevka Fixed, com o mesmo
/// avanço de célula (ADR-0075 §1).
const MASK_FONT: FontFace = FontFace::Mono { bold: false };

/// Corta `text` no **meio** com reticências até caber em `max_width`: o começo e
/// o fim de um caminho são o que o identifica (ADR-0075 §2).
fn truncate_middle(
    text: &str,
    font: FontFace,
    size: f32,
    max_width: f32,
    measurer: &mut TextMeasurer,
) -> String {
    if measurer.measure_width(text, font, size) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let build = |keep: usize| {
        let head = keep.div_ceil(2);
        let tail = keep / 2;
        let mut out: String = chars[..head].iter().collect();
        out.push('…');
        out.extend(&chars[chars.len() - tail..]);
        out
    };
    // O maior número de caracteres mantidos que ainda cabe.
    let (mut low, mut high) = (0, chars.len().saturating_sub(1));
    while low < high {
        let mid = (low + high).div_ceil(2);
        if measurer.measure_width(&build(mid), font, size) <= max_width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    build(low)
}

pub fn layout_dialog(
    window_width: f32,
    window_height: f32,
    dialog: &ConfirmDialog,
    config: &porecatu_config::Config,
    measurer: &mut TextMeasurer,
) -> DialogLayout {
    let cfg = &config.appearance.dialog;
    // Nunca mais larga que a janela (a largura configurada é 760).
    let width = (cfg.width as f32).min(window_width - cfg.padding as f32 * 2.0);
    let padding = cfg.padding as f32;
    let title_size = cfg.title_font_size as f32;
    let body_size = cfg.font_size as f32;
    let gap = cfg.gap as f32;
    let button_height = cfg.button_height as f32;
    let button_gap = cfg.button_gap as f32;
    let button_padding_x = cfg.button_padding_x as f32;

    let cancel_width =
        measurer.measure_width(&dialog.cancel_label, BODY_FONT, body_size) + button_padding_x * 2.0;
    let confirm_width = measurer.measure_width(&dialog.confirm_label, BODY_FONT, body_size)
        + button_padding_x * 2.0;

    let discard_width = dialog
        .discard_label
        .as_ref()
        .map(|label| measurer.measure_width(label, BODY_FONT, body_size) + button_padding_x * 2.0);
    // Com três botões a fila pode passar da largura do diálogo (frases mais
    // longas em outro idioma): o diálogo cresce o que for preciso, nunca
    // corta um botão. Com dois, a largura é sempre a configurada.
    let buttons_width = cancel_width
        + confirm_width
        + button_gap
        + discard_width.map_or(0.0, |discard| discard + button_gap);
    // As linhas de dado nunca são truncadas (ADR-0075 §3): se alguma não
    // coube na largura configurada -- outra fonte, outra escala --, o diálogo
    // cresce o que for preciso, como já faz para os botões.
    let widest_data = dialog
        .lines
        .iter()
        .filter(|line| line.tone == LineTone::Data)
        .map(|line| measurer.measure_width(&line.text, BODY_FONT, body_size))
        .fold(0.0_f32, f32::max);
    // A linha da caixa "Lembrar" (trilho, vão, rótulo) também não é cortada.
    let remember_width = dialog.secret.as_ref().map_or(0.0, |facts| {
        TOGGLE_TRACK_WIDTH
            + button_gap
            + measurer.measure_width(&facts.remember_label, BODY_FONT, body_size)
    });
    // A caixa de marcar do diálogo de excluir perfil, na mesma anatomia.
    let remember_width = remember_width.max(dialog.checkbox.as_ref().map_or(0.0, |checkbox| {
        TOGGLE_TRACK_WIDTH
            + button_gap
            + measurer.measure_width(&checkbox.label, BODY_FONT, body_size)
    }));
    let width = width
        .max(buttons_width + padding * 2.0)
        .max(widest_data + padding * 2.0)
        .max(remember_width + padding * 2.0);

    // Cada linha extra custa o `gap` e a própria altura, como o corpo.
    let lines_height = dialog.lines.len() as f32 * (gap + body_size);
    // O diálogo de segredo ganha o campo (a altura do campo do editor), a
    // linha da caixa e, sem cofre, a nota.
    let field_height = config.appearance.group_editor.input_height as f32;
    let secret_height = dialog.secret.as_ref().map_or(0.0, |facts| {
        let note = if facts.vault_available {
            0.0
        } else {
            gap + body_size
        };
        gap + field_height + gap + TOGGLE_TRACK_HEIGHT + note
    });
    // A caixa sozinha (sem campo) custa o `gap` e a altura da alternância.
    let checkbox_height = if dialog.checkbox.is_some() {
        gap + TOGGLE_TRACK_HEIGHT
    } else {
        0.0
    };
    let content_height = title_size
        + gap
        + body_size
        + lines_height
        + secret_height
        + checkbox_height
        + gap
        + button_height;
    let modal_height = padding * 2.0 + content_height;
    let modal_rect = Rect {
        x: (window_width - width) / 2.0,
        y: (window_height - modal_height) / 2.0,
        width,
        height: modal_height,
    };

    let buttons_y = modal_rect.y + modal_rect.height - padding - button_height;
    let confirm_rect = Rect {
        x: modal_rect.x + modal_rect.width - padding - confirm_width,
        y: buttons_y,
        width: confirm_width,
        height: button_height,
    };
    let discard_rect = discard_width.map(|discard| Rect {
        x: confirm_rect.x - button_gap - discard,
        y: buttons_y,
        width: discard,
        height: button_height,
    });
    let cancel_right = discard_rect.map_or(confirm_rect.x, |discard| discard.x);
    let cancel_rect = Rect {
        x: cancel_right - button_gap - cancel_width,
        y: buttons_y,
        width: cancel_width,
        height: button_height,
    };

    // Campo, caixa e nota, empilhados abaixo do corpo e das linhas.
    let mut field_rect = None;
    let mut remember_rect = None;
    let mut toggle_rect = None;
    let mut note_rect = None;
    if let Some(facts) = &dialog.secret {
        let content_x = modal_rect.x + padding;
        let content_width = modal_rect.width - padding * 2.0;
        let below_body = modal_rect.y + padding + title_size + gap + body_size + lines_height;
        let field = Rect {
            x: content_x,
            y: below_body + gap,
            width: content_width,
            height: field_height,
        };
        let row = Rect {
            x: content_x,
            y: field.y + field.height + gap,
            width: content_width,
            height: TOGGLE_TRACK_HEIGHT,
        };
        field_rect = Some(field);
        remember_rect = Some(row);
        toggle_rect = Some(Rect {
            x: row.x,
            y: row.y,
            width: TOGGLE_TRACK_WIDTH,
            height: TOGGLE_TRACK_HEIGHT,
        });
        if !facts.vault_available {
            note_rect = Some(Rect {
                x: content_x,
                y: row.y + row.height + gap,
                width: content_width,
                height: body_size,
            });
        }
    }
    if dialog.checkbox.is_some() {
        let below_body = modal_rect.y + padding + title_size + gap + body_size + lines_height;
        let row = Rect {
            x: modal_rect.x + padding,
            y: below_body + gap,
            width: modal_rect.width - padding * 2.0,
            height: TOGGLE_TRACK_HEIGHT,
        };
        remember_rect = Some(row);
        toggle_rect = Some(Rect {
            x: row.x,
            y: row.y,
            width: TOGGLE_TRACK_WIDTH,
            height: TOGGLE_TRACK_HEIGHT,
        });
    }

    DialogLayout {
        modal_rect,
        cancel_rect,
        discard_rect,
        confirm_rect,
        field_rect,
        remember_rect,
        toggle_rect,
        note_rect,
    }
}

/// O campo mascarado: a anatomia do campo de texto da tela de configurações
/// (§2.12), com pontos no lugar do texto. Foco no anel de Acento.
fn paint_secret_field(
    rect: Rect,
    view: &SecretView<'_>,
    focus: SecretFocus,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    let editor = &config.appearance.group_editor;
    let padding_x = editor.input_padding_x as f32;
    let size = editor.input_font_size as f32;
    let focused = focus == SecretFocus::Field;
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: editor.input_corner_radius as f32,
        color: pal.editor_input_background,
        border_color: if focused {
            pal.dialog_focus_ring
        } else {
            pal.editor_input_border
        },
        border_width: 1.0,
    }));
    let dots = view.field.display();
    let text = dots.text();
    let text_width = measurer.measure_width(text, MASK_FONT, size);
    let text_x = secret_text_x(rect, padding_x, text_width);
    let inner = Rect {
        x: rect.x + padding_x,
        y: rect.y,
        width: (rect.width - padding_x * 2.0).max(0.0),
        height: rect.height,
    };
    let bar_y = rect.y + 3.0;
    let bar_height = (rect.height - 6.0).max(0.0);
    out.push(Primitive::PushClip(inner));
    if let Some((start, end)) = dots.selection_range() {
        let x0 = text_x + measurer.measure_width(&text[..start], MASK_FONT, size);
        let x1 = text_x + measurer.measure_width(&text[..end], MASK_FONT, size);
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: x0,
                y: bar_y,
                width: x1 - x0,
                height: bar_height,
            },
            color: view.selection_color,
        }));
    }
    out.push(Primitive::Text(TextRun {
        origin: (text_x, rect.y + (rect.height - size) / 2.0),
        text: text.to_owned(),
        font: MASK_FONT,
        size_px: size,
        color: pal.editor_input_text,
    }));
    if focused && dots.selection_range().is_none() {
        let before = measurer.measure_width(&text[..dots.cursor()], MASK_FONT, size);
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: (text_x + before).min(inner.x + inner.width - 1.0),
                y: bar_y,
                width: 1.0,
                height: bar_height,
            },
            color: pal.editor_input_text,
        }));
    }
    out.push(Primitive::PopClip);
}

/// Onde os pontos começam em x: o campo rola para manter o cursor à vista, como
/// o de rename e os da tela de configurações.
fn secret_text_x(field: Rect, padding_x: f32, text_width: f32) -> f32 {
    tab_bar::scrolled_text_x(
        field.x,
        padding_x,
        text_width,
        (field.width - padding_x * 2.0).max(0.0),
    )
}

/// Qual ponto está sob o clique em `x`: o cursor vai para antes dele.
pub fn secret_field_scalar_at(
    layout: &DialogLayout,
    x: f32,
    config: &porecatu_config::Config,
    dot_count: usize,
    measurer: &mut TextMeasurer,
) -> usize {
    let Some(field) = layout.field_rect else {
        return 0;
    };
    let editor = &config.appearance.group_editor;
    let size = editor.input_font_size as f32;
    let dot_width = measurer.measure_width("•", MASK_FONT, size);
    if dot_width <= 0.0 {
        return dot_count;
    }
    let total = dot_width * dot_count as f32;
    let text_x = secret_text_x(field, editor.input_padding_x as f32, total);
    (((x - text_x) / dot_width).round().max(0.0) as usize).min(dot_count)
}

/// Os cliques de um diálogo de segredo: dentro do campo, na caixa, ou nos
/// botões (`dialog_hit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretHit {
    Field,
    Remember,
}

pub fn secret_hit(layout: &DialogLayout, point: (f32, f32)) -> Option<SecretHit> {
    if layout
        .field_rect
        .is_some_and(|field| rect_contains(field, point))
    {
        Some(SecretHit::Field)
    } else if layout
        .remember_rect
        .is_some_and(|row| rect_contains(row, point))
    {
        Some(SecretHit::Remember)
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
pub fn paint_dialog(
    layout: &DialogLayout,
    dialog: &ConfirmDialog,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    window_width: f32,
    window_height: f32,
    secret: Option<&SecretView<'_>>,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let cfg = &config.appearance.dialog;
    let padding = cfg.padding as f32;
    let title_size = cfg.title_font_size as f32;
    let body_size = cfg.font_size as f32;
    let gap = cfg.gap as f32;
    let corner_radius = cfg.corner_radius as f32;
    let button_corner_radius = cfg.button_corner_radius as f32;
    // No diálogo de segredo o foco anda também pelo campo e pela caixa: o anel
    // dos botões só aparece com o foco neles.
    let buttons_have_focus = dialog
        .secret
        .as_ref()
        .is_none_or(|facts| facts.focus == SecretFocus::Buttons);
    let focused_button = |button: DialogButton| buttons_have_focus && dialog.focused() == button;

    let mut out = Vec::new();
    out.push(Primitive::Quad(Quad {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: window_width,
            height: window_height,
        },
        color: pal.dialog_overlay,
    }));
    push_shadow(&mut out, layout.modal_rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.modal_rect,
        radius: corner_radius,
        color: pal.dialog_background,
        border_color: pal.dialog_border,
        border_width: 1.0,
    }));

    let text_x = layout.modal_rect.x + padding;
    let mut y = layout.modal_rect.y + padding;
    out.push(Primitive::Text(TextRun {
        origin: (text_x, y),
        text: dialog.title.clone(),
        font: TITLE_FONT,
        size_px: title_size,
        color: pal.dialog_title_text,
    }));
    y += title_size + gap;
    let body_max_width = layout.modal_rect.width - padding * 2.0;
    // O caminho de uma chave se corta no meio; o resto, no fim.
    let body = if dialog
        .secret
        .as_ref()
        .is_some_and(|facts| facts.body_is_path)
    {
        truncate_middle(&dialog.body, BODY_FONT, body_size, body_max_width, measurer)
    } else {
        measurer
            .truncate(&dialog.body, BODY_FONT, body_size, body_max_width)
            .0
    };
    out.push(Primitive::Text(TextRun {
        origin: (text_x, y),
        text: body,
        font: BODY_FONT,
        size_px: body_size,
        // A linha de erro de um segredo recusado fica na cor de erro (§2.14).
        color: if dialog.body_is_error {
            pal.warning_severity_error
        } else {
            pal.dialog_body_text
        },
    }));
    // Linhas de dado e de aviso (ADR-0075 §3): dado em "Secundário" sem
    // truncar, aviso na cor de aviso da §2.14 e cortado ao conteúdo.
    for line in &dialog.lines {
        y += body_size + gap;
        let (text, color) = match line.tone {
            LineTone::Data => (line.text.clone(), pal.warning_body_text),
            LineTone::Warning => (
                measurer
                    .truncate(&line.text, BODY_FONT, body_size, body_max_width)
                    .0,
                pal.warning_severity_warning,
            ),
        };
        out.push(Primitive::Text(TextRun {
            origin: (text_x, y),
            text,
            font: BODY_FONT,
            size_px: body_size,
            color,
        }));
    }

    // Campo mascarado e caixa "Lembrar" (ADR-0075 §2).
    if let (Some(view), Some(facts)) = (secret, dialog.secret.as_ref()) {
        if let Some(field) = layout.field_rect {
            paint_secret_field(field, view, facts.focus, config, pal, measurer, &mut out);
        }
        if let Some(toggle) = layout.toggle_rect {
            // Sem cofre a alternância fica desligada e o rótulo esmaecido.
            let on = facts.remember && facts.vault_available;
            crate::toggle::push_toggle(
                toggle,
                on,
                crate::settings::TOGGLE_ON,
                crate::settings::TOGGLE_OFF,
                &mut out,
            );
            if facts.focus == SecretFocus::Remember {
                out.push(Primitive::RoundedQuad(RoundedQuad {
                    rect: toggle,
                    radius: TOGGLE_TRACK_HEIGHT / 2.0,
                    color: palette::TRANSPARENT,
                    border_color: pal.dialog_focus_ring,
                    border_width: 1.0,
                }));
            }
            out.push(Primitive::Text(TextRun {
                origin: (
                    toggle.x + toggle.width + cfg.button_gap as f32,
                    toggle.y + (toggle.height - body_size) / 2.0,
                ),
                text: facts.remember_label.clone(),
                font: BODY_FONT,
                size_px: body_size,
                color: if facts.vault_available {
                    pal.dialog_body_text
                } else {
                    pal.menu_item_disabled_text
                },
            }));
        }
        if let Some(note) = layout.note_rect {
            out.push(Primitive::Text(TextRun {
                origin: (note.x, note.y),
                text: measurer
                    .truncate(&facts.unavailable_note, BODY_FONT, body_size, note.width)
                    .0,
                font: BODY_FONT,
                size_px: body_size,
                color: pal.menu_item_disabled_text,
            }));
        }
    }
    // A caixa de marcar do diálogo de excluir perfil: a alternância da tela de
    // configurações, ligada quando marcada.
    if let (Some(checkbox), Some(toggle)) = (dialog.checkbox.as_ref(), layout.toggle_rect) {
        crate::toggle::push_toggle(
            toggle,
            checkbox.checked,
            crate::settings::TOGGLE_ON,
            crate::settings::TOGGLE_OFF,
            &mut out,
        );
        out.push(Primitive::Text(TextRun {
            origin: (
                toggle.x + toggle.width + cfg.button_gap as f32,
                toggle.y + (toggle.height - body_size) / 2.0,
            ),
            text: checkbox.label.clone(),
            font: BODY_FONT,
            size_px: body_size,
            color: pal.dialog_body_text,
        }));
    }

    if let (Some(discard_rect), Some(discard_label)) =
        (layout.discard_rect, dialog.discard_label.as_ref())
    {
        // Três saídas (RF-16.4, ADR-0060 §4): Cancelar no estilo cancelar,
        // Descartar e fechar destrutivo -- texto `#e08585`, hover `#2e2224`,
        // as cores do item destrutivo do menu --, e Salvar e fechar
        // primário, o par ligado da alternância com hover por brilho.
        let hovered = dialog.hovered();
        let ring_or = |button: DialogButton, border: Color| {
            if dialog.focused() == button {
                pal.dialog_focus_ring
            } else {
                border
            }
        };
        paint_dialog_button(
            layout.cancel_rect,
            &dialog.cancel_label,
            if hovered == Some(DialogButton::Cancel) {
                pal.dialog_cancel_border
            } else {
                palette::TRANSPARENT
            },
            pal.dialog_cancel_text,
            ring_or(DialogButton::Cancel, pal.dialog_cancel_border),
            body_size,
            button_corner_radius,
            measurer,
            &mut out,
        );
        paint_dialog_button(
            discard_rect,
            discard_label,
            if hovered == Some(DialogButton::Discard) {
                pal.menu_item_destructive_hover
            } else {
                palette::TRANSPARENT
            },
            pal.dialog_confirm_background,
            ring_or(DialogButton::Discard, pal.dialog_cancel_border),
            body_size,
            button_corner_radius,
            measurer,
            &mut out,
        );
        paint_dialog_button(
            layout.confirm_rect,
            &dialog.confirm_label,
            if hovered == Some(DialogButton::Confirm) {
                crate::chrome::brighten(
                    crate::settings::TOGGLE_ON,
                    config.appearance.tabs.colors.hover_brightness,
                )
            } else {
                crate::settings::TOGGLE_ON
            },
            crate::toggle::TOGGLE_KNOB_COLOR,
            ring_or(DialogButton::Confirm, palette::TRANSPARENT),
            body_size,
            button_corner_radius,
            measurer,
            &mut out,
        );
        return out;
    }
    paint_dialog_button(
        layout.cancel_rect,
        &dialog.cancel_label,
        palette::TRANSPARENT,
        pal.dialog_cancel_text,
        if focused_button(DialogButton::Cancel) {
            pal.dialog_focus_ring
        } else {
            pal.dialog_cancel_border
        },
        body_size,
        button_corner_radius,
        measurer,
        &mut out,
    );
    paint_dialog_button(
        layout.confirm_rect,
        &dialog.confirm_label,
        // Conectar é o primário do rodapé da tela de configurações, não o
        // destrutivo (ADR-0075 §2).
        if dialog.secret.is_some() {
            crate::settings::TOGGLE_ON
        } else {
            pal.dialog_confirm_background
        },
        if dialog.secret.is_some() {
            crate::toggle::TOGGLE_KNOB_COLOR
        } else {
            pal.dialog_confirm_text
        },
        if focused_button(DialogButton::Confirm) {
            pal.dialog_focus_ring
        } else {
            palette::TRANSPARENT
        },
        body_size,
        button_corner_radius,
        measurer,
        &mut out,
    );

    out
}

/// O clique caiu na linha da caixa de marcar de um diálogo sem campo.
pub fn checkbox_hit(layout: &DialogLayout, point: (f32, f32)) -> bool {
    layout.field_rect.is_none()
        && layout
            .remember_rect
            .is_some_and(|row| rect_contains(row, point))
}

#[allow(clippy::too_many_arguments)]
fn paint_dialog_button(
    rect: Rect,
    label: &str,
    background: Color,
    text_color: Color,
    border_color: Color,
    font_size: f32,
    corner_radius: f32,
    measurer: &mut TextMeasurer,
    out: &mut Vec<Primitive>,
) {
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: corner_radius,
        color: background,
        border_color,
        border_width: 1.0,
    }));
    let width = measurer.measure_width(label, BODY_FONT, font_size);
    out.push(Primitive::Text(TextRun {
        origin: (
            rect.x + (rect.width - width) / 2.0,
            rect.y + (rect.height - font_size) / 2.0,
        ),
        text: label.to_string(),
        font: BODY_FONT,
        size_px: font_size,
        color: text_color,
    }));
}

pub fn dialog_hit(layout: &DialogLayout, point: (f32, f32)) -> Option<DialogButton> {
    if rect_contains(layout.cancel_rect, point) {
        Some(DialogButton::Cancel)
    } else if layout
        .discard_rect
        .is_some_and(|discard| rect_contains(discard, point))
    {
        Some(DialogButton::Discard)
    } else if rect_contains(layout.confirm_rect, point) {
        Some(DialogButton::Confirm)
    } else {
        None
    }
}

// ---- menu de contexto (espec §2.16, ADR-0014, `[appearance.context_menu]`) ----

pub struct MenuLayout {
    pub menu_rect: Rect,
    /// Alinhado por índice com `TAB_MENU_ITEMS`.
    pub item_rects: Vec<Rect>,
}

pub fn layout_context_menu(
    menu: &ContextMenu,
    config: &porecatu_config::Config,
    window_width: f32,
    window_height: f32,
) -> MenuLayout {
    let cfg = &config.appearance.context_menu;
    let width = cfg.width as f32;
    let padding = cfg.padding as f32;
    let item_height = cfg.item_height as f32;

    let height = padding * 2.0 + item_height * menu.items().len() as f32;
    let mut x = menu.anchor.0;
    let mut y = menu.anchor.1;
    // "Vira nos dois eixos para caber na tela" (espec §2.16) -- contra a
    // janela, não o monitor: multi-monitor físico é responsabilidade de
    // `winit`/SO posicionar a própria janela, isto só evita estourar a
    // borda da janela atual.
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    if y + height > window_height {
        y = (window_height - height).max(0.0);
    }
    let menu_rect = Rect {
        x,
        y,
        width,
        height,
    };
    let item_rects = (0..menu.items().len())
        .map(|i| Rect {
            x: menu_rect.x + padding,
            y: menu_rect.y + padding + item_height * i as f32,
            width: menu_rect.width - padding * 2.0,
            height: item_height,
        })
        .collect();
    MenuLayout {
        menu_rect,
        item_rects,
    }
}

pub fn paint_context_menu(
    layout: &MenuLayout,
    menu: &ContextMenu,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    catalog: &Catalog,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let cfg = &config.appearance.context_menu;
    let corner_radius = cfg.corner_radius as f32;
    let item_radius = cfg.item_corner_radius as f32;
    let item_padding_x = cfg.item_padding_x as f32;
    let font_size = cfg.font_size as f32;

    let mut out = Vec::new();
    push_shadow(&mut out, layout.menu_rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.menu_rect,
        radius: corner_radius,
        color: pal.context_menu_background,
        border_color: pal.context_menu_border,
        border_width: 1.0,
    }));
    for (index, item) in menu.items().iter().enumerate() {
        let rect = layout.item_rects[index];
        if index == menu.highlighted() {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: item_radius,
                color: pal.menu_item_hover,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let color = if item.enabled {
            pal.menu_item_text
        } else {
            pal.menu_item_disabled_text
        };
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + item_padding_x,
                rect.y + (rect.height - font_size) / 2.0,
            ),
            text: fit_label(
                item.action.label(catalog),
                menu_label_budget(rect, item_padding_x),
                font_size,
                measurer,
            ),
            font: BODY_FONT,
            size_px: font_size,
            color,
        }));
    }
    out
}

/// Índice do item sob `point`, se algum (aceita item desabilitado -- quem
/// decide se aciona é `lib.rs`, olhando `TAB_MENU_ITEMS[index].enabled`).
pub fn context_menu_hit(layout: &MenuLayout, point: (f32, f32)) -> Option<usize> {
    layout
        .item_rects
        .iter()
        .position(|&rect| rect_contains(rect, point))
}

// ---- menu de contexto de grupo (espec §2.16, RF-2.22, ADR-0023) ----

/// Mesma anatomia do menu de aba (`layout_context_menu`/
/// `paint_context_menu`), duplicada em vez de generalizada -- os itens são
/// `GroupActionItem` (rótulo dinâmico, possivelmente destrutivo), não o
/// `MenuItem` estático de `TAB_MENU_ITEMS`, e a lista não é uma constante
/// global (`group_menu::group_action_items` resolve na hora, a partir do
/// estado do grupo). Mesmas chaves de `[appearance.context_menu]`.
pub struct GroupMenuLayout {
    pub menu_rect: Rect,
    pub item_rects: Vec<Rect>,
}

pub fn layout_group_menu(
    menu: &GroupContextMenu,
    item_count: usize,
    config: &porecatu_config::Config,
    window_width: f32,
    window_height: f32,
) -> GroupMenuLayout {
    let cfg = &config.appearance.context_menu;
    let width = cfg.width as f32;
    let padding = cfg.padding as f32;
    let item_height = cfg.item_height as f32;

    let height = padding * 2.0 + item_height * item_count as f32;
    let mut x = menu.anchor.0;
    let mut y = menu.anchor.1;
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    if y + height > window_height {
        y = (window_height - height).max(0.0);
    }
    let menu_rect = Rect {
        x,
        y,
        width,
        height,
    };
    let item_rects = (0..item_count)
        .map(|i| Rect {
            x: menu_rect.x + padding,
            y: menu_rect.y + padding + item_height * i as f32,
            width: menu_rect.width - padding * 2.0,
            height: item_height,
        })
        .collect();
    GroupMenuLayout {
        menu_rect,
        item_rects,
    }
}

pub fn paint_group_menu(
    layout: &GroupMenuLayout,
    items: &[GroupActionItem],
    highlighted: usize,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let cfg = &config.appearance.context_menu;
    let corner_radius = cfg.corner_radius as f32;
    let item_radius = cfg.item_corner_radius as f32;
    let item_padding_x = cfg.item_padding_x as f32;
    let font_size = cfg.font_size as f32;

    let mut out = Vec::new();
    push_shadow(&mut out, layout.menu_rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.menu_rect,
        radius: corner_radius,
        color: pal.context_menu_background,
        border_color: pal.context_menu_border,
        border_width: 1.0,
    }));
    for (index, item) in items.iter().enumerate() {
        let rect = layout.item_rects[index];
        if index == highlighted {
            let hover_color = if item.destructive {
                pal.menu_item_destructive_hover
            } else {
                pal.menu_item_hover
            };
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: item_radius,
                color: hover_color,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let text_color = if item.destructive {
            pal.menu_item_destructive_text
        } else {
            pal.menu_item_text
        };
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + item_padding_x,
                rect.y + (rect.height - font_size) / 2.0,
            ),
            text: fit_label(
                item.label.clone(),
                menu_label_budget(rect, item_padding_x),
                font_size,
                measurer,
            ),
            font: BODY_FONT,
            size_px: font_size,
            color: text_color,
        }));
    }
    out
}

pub fn group_menu_hit(layout: &GroupMenuLayout, point: (f32, f32)) -> Option<usize> {
    layout
        .item_rects
        .iter()
        .position(|&rect| rect_contains(rect, point))
}

// ---- menu de perfis (espec §2.9/§2.16, ADR-0070 §6) ----

/// The context-menu anatomy again, anchored on a button instead of the cursor,
/// with a divider before the last row ("new tab group") and a badge column.
/// Same `[appearance.context_menu]` keys.
pub(crate) struct ProfileMenuLayout {
    pub(crate) menu_rect: Rect,
    pub(crate) item_rects: Vec<Rect>,
}

/// Top-left corner of a `size` popover hanging from `anchor`: below it, with
/// `offset` between them, left edges aligned. "Flips on both axes": when it
/// does not fit to the right it aligns to the anchor's right edge, and when it
/// does not fit below it opens above. Last resort is a clamp into the window.
pub(crate) fn anchored_origin(
    anchor: Rect,
    size: (f32, f32),
    window: (f32, f32),
    offset: f32,
) -> (f32, f32) {
    let (width, height) = size;
    let mut x = anchor.x;
    if x + width > window.0 {
        x = anchor.x + anchor.width - width;
    }
    let max_x = (window.0 - width).max(0.0);
    x = x.clamp(0.0, max_x);
    let mut y = anchor.y + anchor.height + offset;
    if y + height > window.1 {
        y = anchor.y - offset - height;
    }
    let max_y = (window.1 - height).max(0.0);
    y = y.clamp(0.0, max_y);
    (x, y)
}

/// Width of the profile menu: the content's, between the context menu's width
/// (the minimum, §2.16) and the notice's `width` (the 320 maximum, §2.16).
/// Measured once, when the menu opens; a label that still does not fit is cut
/// with an ellipsis like in every other menu.
pub(crate) fn profile_menu_width(
    rows: &[crate::profile_menu::ProfileRow],
    config: &porecatu_config::Config,
    measurer: &mut TextMeasurer,
) -> f32 {
    let cfg = &config.appearance.context_menu;
    let minimum = cfg.width as f32;
    let maximum = (config.appearance.notices.width as f32).max(minimum);
    let item_padding_x = cfg.item_padding_x as f32;
    let font_size = cfg.font_size as f32;
    let column = rows
        .iter()
        .filter_map(|row| row.badge.as_deref())
        .map(|badge| badge_size(badge, measurer).0)
        .fold(0.0_f32, f32::max);
    let label_offset = if column > 0.0 {
        column + SAVE_ITEM_ICON_GAP
    } else {
        0.0
    };
    let widest_row = rows
        .iter()
        .map(|row| {
            let label = measurer.measure_width(&row.label, BODY_FONT, font_size);
            let chip = row.chip.as_deref().map_or(0.0, |chip| {
                SAVE_ITEM_ICON_GAP + measurer.measure_width(chip, CHIP_FONT, CHIP_FONT_SIZE)
            });
            label_offset + label + chip
        })
        .fold(0.0_f32, f32::max);
    let content = cfg.padding as f32 * 2.0 + item_padding_x * 2.0 + widest_row;
    content.clamp(minimum, maximum)
}

pub(crate) fn layout_profile_menu(
    menu: &ProfileMenu,
    config: &porecatu_config::Config,
    window_width: f32,
    window_height: f32,
) -> ProfileMenuLayout {
    let cfg = &config.appearance.context_menu;
    let width = menu.width;
    let padding = cfg.padding as f32;
    let item_height = cfg.item_height as f32;
    // The distance the other anchored popovers keep from the bar (the group
    // editor's `offset_y`); no value of its own.
    let offset = config.appearance.group_editor.offset_y as f32;

    let count = menu.rows().len();
    let divider_block = SAVE_DIVIDER_MARGIN_Y * 2.0 + SAVE_DIVIDER_HEIGHT;
    // A divider above every `separated` row but the first.
    let dividers_above = |i: usize| {
        menu.rows()
            .iter()
            .take(i + 1)
            .enumerate()
            .filter(|(index, row)| *index > 0 && row.separated)
            .count() as f32
    };
    let height = padding * 2.0
        + item_height * count as f32
        + divider_block * dividers_above(count.saturating_sub(1));
    let (x, y) = anchored_origin(
        menu.anchor,
        (width, height),
        (window_width, window_height),
        offset,
    );
    let menu_rect = Rect {
        x,
        y,
        width,
        height,
    };
    let item_rects = (0..count)
        .map(|i| Rect {
            x: menu_rect.x + padding,
            y: menu_rect.y + padding + item_height * i as f32 + divider_block * dividers_above(i),
            width: menu_rect.width - padding * 2.0,
            height: item_height,
        })
        .collect();
    ProfileMenuLayout {
        menu_rect,
        item_rects,
    }
}

pub(crate) fn paint_profile_menu(
    layout: &ProfileMenuLayout,
    menu: &ProfileMenu,
    badge_color: Color,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let cfg = &config.appearance.context_menu;
    let corner_radius = cfg.corner_radius as f32;
    let item_radius = cfg.item_corner_radius as f32;
    let item_padding_x = cfg.item_padding_x as f32;
    let font_size = cfg.font_size as f32;
    let tint = config.appearance.groups.badge_tint_strength;

    let mut out = Vec::new();
    push_shadow(&mut out, layout.menu_rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.menu_rect,
        radius: corner_radius,
        color: pal.context_menu_background,
        border_color: pal.context_menu_border,
        border_width: 1.0,
    }));

    // The badge column is as wide as the widest badge, so the labels line up
    // whether or not their row has one (the `[shell]` row has none).
    let column = menu
        .rows()
        .iter()
        .filter_map(|row| row.badge.as_deref())
        .map(|badge| badge_size(badge, measurer).0)
        .fold(0.0_f32, f32::max);
    let label_offset = if column > 0.0 {
        column + SAVE_ITEM_ICON_GAP
    } else {
        0.0
    };

    for (index, row) in menu.rows().iter().enumerate() {
        let rect = layout.item_rects[index];
        if index == menu.highlighted() {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: item_radius,
                color: pal.menu_item_hover,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let left = rect.x + item_padding_x;
        if let Some(badge) = &row.badge {
            let (_, badge_height) = badge_size(badge, measurer);
            push_badge(
                &mut out,
                (left, rect.y + (rect.height - badge_height) / 2.0),
                badge,
                badge_color,
                tint,
                measurer,
            );
        }
        let chip_width = row.chip.as_deref().map_or(0.0, |chip| {
            measurer.measure_width(chip, CHIP_FONT, CHIP_FONT_SIZE)
        });
        let chip_gap = if chip_width > 0.0 {
            SAVE_ITEM_ICON_GAP
        } else {
            0.0
        };
        let label_x = left + label_offset;
        let budget =
            (rect.x + rect.width - item_padding_x - chip_width - chip_gap - label_x).max(0.0);
        out.push(Primitive::Text(TextRun {
            origin: (label_x, rect.y + (rect.height - font_size) / 2.0),
            text: fit_label(row.label.clone(), budget, font_size, measurer),
            font: BODY_FONT,
            size_px: font_size,
            color: pal.menu_item_text,
        }));
        if let Some(chip) = &row.chip {
            out.push(Primitive::Text(TextRun {
                origin: (
                    rect.x + rect.width - item_padding_x - chip_width,
                    rect.y + (rect.height - CHIP_FONT_SIZE) / 2.0,
                ),
                text: chip.clone(),
                font: CHIP_FONT,
                size_px: CHIP_FONT_SIZE,
                color: pal.menu_item_disabled_text,
            }));
        }
    }

    let padding = cfg.padding as f32;
    for (index, row) in menu.rows().iter().enumerate() {
        if index == 0 || !row.separated {
            continue;
        }
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: layout.menu_rect.x + padding + SAVE_DIVIDER_MARGIN_X,
                y: layout.item_rects[index].y - SAVE_DIVIDER_MARGIN_Y - SAVE_DIVIDER_HEIGHT,
                width: layout.menu_rect.width - padding * 2.0 - SAVE_DIVIDER_MARGIN_X * 2.0,
                height: SAVE_DIVIDER_HEIGHT,
            },
            color: pal.editor_divider,
        }));
    }
    out
}

pub(crate) fn profile_menu_hit(layout: &ProfileMenuLayout, point: (f32, f32)) -> Option<usize> {
    layout
        .item_rects
        .iter()
        .position(|&rect| rect_contains(rect, point))
}

// ---- menu de contexto do terminal (espec §2.16, PRD-011 RF-11.14, ADR-0042 §9) ----

/// Terceira cópia da mesma anatomia (`layout_context_menu`/
/// `layout_group_menu`) -- `TerminalMenuItem` tem `enabled` como
/// `context_menu::MenuItem`, não rótulo dinâmico/destrutivo como
/// `GroupActionItem`, mas a contagem de itens varia (`terminal_menu.rs`,
/// nota do módulo), então segue o padrão do menu de grupo: item count e
/// lista vêm de fora, nunca de uma constante.
pub struct TerminalMenuLayout {
    pub menu_rect: Rect,
    pub item_rects: Vec<Rect>,
}

pub fn layout_terminal_menu(
    menu: &TerminalContextMenu,
    item_count: usize,
    config: &porecatu_config::Config,
    window_width: f32,
    window_height: f32,
) -> TerminalMenuLayout {
    let cfg = &config.appearance.context_menu;
    let width = cfg.width as f32;
    let padding = cfg.padding as f32;
    let item_height = cfg.item_height as f32;

    let height = padding * 2.0 + item_height * item_count as f32;
    let mut x = menu.anchor.0;
    let mut y = menu.anchor.1;
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    if y + height > window_height {
        y = (window_height - height).max(0.0);
    }
    let menu_rect = Rect {
        x,
        y,
        width,
        height,
    };
    let item_rects = (0..item_count)
        .map(|i| Rect {
            x: menu_rect.x + padding,
            y: menu_rect.y + padding + item_height * i as f32,
            width: menu_rect.width - padding * 2.0,
            height: item_height,
        })
        .collect();
    TerminalMenuLayout {
        menu_rect,
        item_rects,
    }
}

pub fn paint_terminal_menu(
    layout: &TerminalMenuLayout,
    items: &[TerminalMenuItem],
    highlighted: usize,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    catalog: &Catalog,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let cfg = &config.appearance.context_menu;
    let corner_radius = cfg.corner_radius as f32;
    let item_radius = cfg.item_corner_radius as f32;
    let item_padding_x = cfg.item_padding_x as f32;
    let font_size = cfg.font_size as f32;

    let mut out = Vec::new();
    push_shadow(&mut out, layout.menu_rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.menu_rect,
        radius: corner_radius,
        color: pal.context_menu_background,
        border_color: pal.context_menu_border,
        border_width: 1.0,
    }));
    for (index, item) in items.iter().enumerate() {
        let rect = layout.item_rects[index];
        if index == highlighted {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: item_radius,
                color: pal.menu_item_hover,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let color = if item.enabled {
            pal.menu_item_text
        } else {
            pal.menu_item_disabled_text
        };
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + item_padding_x,
                rect.y + (rect.height - font_size) / 2.0,
            ),
            text: fit_label(
                item.action.label(catalog),
                menu_label_budget(rect, item_padding_x),
                font_size,
                measurer,
            ),
            font: BODY_FONT,
            size_px: font_size,
            color,
        }));
    }
    out
}

pub fn terminal_menu_hit(layout: &TerminalMenuLayout, point: (f32, f32)) -> Option<usize> {
    layout
        .item_rects
        .iter()
        .position(|&rect| rect_contains(rect, point))
}

// ---- editor de grupo (espec §2.10, ADR-0023, `[appearance.group_editor]`) ----

pub struct GroupEditorLayout {
    pub popover_rect: Rect,
    pub name_caption_origin: (f32, f32),
    pub name_input_rect: Rect,
    pub color_caption_origin: (f32, f32),
    /// Seis, na ordem de `GroupColor::ALL`.
    pub swatch_rects: Vec<Rect>,
    pub divider_rect: Rect,
    /// Alinhado por índice com `EDITOR_ACTION_ORDER`.
    pub action_rects: Vec<Rect>,
}

/// `anchor_x`: canto esquerdo da pílula do grupo, em coordenadas de tela
/// (espec: "posicionado horizontalmente sobre o grupo que está sendo
/// editado"). `bar_bottom_y`: borda inferior da barra de abas -- o popover
/// nasce `offset_y` abaixo dela; com `tab_bar_position = "bottom"` ele
/// abriria acima, mas essa chave é lida por `chrome.rs`/`lib.rs`, não
/// aqui -- só o caso `top` (o único que `layout_group_editor` recebe hoje)
/// está implementado.
pub fn layout_group_editor(
    anchor_x: f32,
    bar_bottom_y: f32,
    config: &porecatu_config::Config,
    window_width: f32,
    window_height: f32,
) -> GroupEditorLayout {
    let cfg = &config.appearance.group_editor;
    let width = cfg.width as f32;
    let padding = cfg.padding as f32;
    let gap = cfg.gap as f32;
    let offset_y = cfg.offset_y as f32;
    let section_font_size = cfg.section_font_size as f32;
    let section_caption_gap = cfg.section_caption_gap as f32;
    let input_height = cfg.input_height as f32;
    let swatch_size = cfg.swatch_size as f32;
    let swatch_gap = cfg.swatch_gap as f32;
    let divider_height = cfg.divider_height as f32;
    let item_height = cfg.item_height as f32;

    let action_count = EDITOR_ACTION_ORDER.len();
    let content_height = section_font_size
        + section_caption_gap
        + input_height
        + gap
        + section_font_size
        + section_caption_gap
        + swatch_size
        + gap
        + divider_height
        + gap
        + item_height * action_count as f32;
    let popover_height = padding * 2.0 + content_height;

    let mut x = anchor_x;
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    let mut y = bar_bottom_y + offset_y;
    if y + popover_height > window_height {
        y = (window_height - popover_height).max(0.0);
    }
    let popover_rect = Rect {
        x,
        y,
        width,
        height: popover_height,
    };

    let inner_x = popover_rect.x + padding;
    let inner_width = popover_rect.width - padding * 2.0;
    let mut cursor_y = popover_rect.y + padding;

    let name_caption_origin = (inner_x, cursor_y);
    cursor_y += section_font_size + section_caption_gap;
    let name_input_rect = Rect {
        x: inner_x,
        y: cursor_y,
        width: inner_width,
        height: input_height,
    };
    cursor_y += input_height + gap;

    let color_caption_origin = (inner_x, cursor_y);
    cursor_y += section_font_size + section_caption_gap;
    let mut swatch_rects = Vec::with_capacity(GroupColor::ALL.len());
    let mut sx = inner_x;
    for _ in GroupColor::ALL {
        swatch_rects.push(Rect {
            x: sx,
            y: cursor_y,
            width: swatch_size,
            height: swatch_size,
        });
        sx += swatch_size + swatch_gap;
    }
    cursor_y += swatch_size + gap;

    let divider_rect = Rect {
        x: inner_x,
        y: cursor_y,
        width: inner_width,
        height: divider_height,
    };
    cursor_y += divider_height + gap;

    let action_rects = (0..action_count)
        .map(|i| Rect {
            x: inner_x,
            y: cursor_y + item_height * i as f32,
            width: inner_width,
            height: item_height,
        })
        .collect();

    GroupEditorLayout {
        popover_rect,
        name_caption_origin,
        name_input_rect,
        color_caption_origin,
        swatch_rects,
        divider_rect,
        action_rects,
    }
}

fn expand(rect: Rect, amount: f32) -> Rect {
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        width: rect.width + amount * 2.0,
        height: rect.height + amount * 2.0,
    }
}

/// `current_color_index`/`is_collapsed`/`tab_count`: estado corrente do
/// grupo, lido fresco por quem chama (`lib.rs`) -- nunca guardado em
/// `GroupEditor` (módulo doc). O campo de nome desenha o **buffer ao
/// vivo** (`editor.name_buffer()`), não o nome real do grupo: é o mesmo
/// truque do campo de rename de aba (`chrome::paint_rename_field`) --
/// texto rola dentro do campo mantendo o caret visível, sem quebrar linha.
/// A largura/raio dos itens de ação e o tamanho de fonte deles reaproveitam
/// `[appearance.context_menu]` (`item_corner_radius`/`item_padding_x`/
/// `font_size`) -- `GroupEditor` não tem chave própria pra isso, mesma
/// anatomia de lista que o menu de contexto.
#[allow(clippy::too_many_arguments)]
pub fn paint_group_editor(
    layout: &GroupEditorLayout,
    editor: &GroupEditor,
    current_color_index: usize,
    is_collapsed: bool,
    tab_count: usize,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    term_pal: &palette::ResolvedTermPalette,
    catalog: &Catalog,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let cfg = &config.appearance.group_editor;
    let menu_cfg = &config.appearance.context_menu;
    let corner_radius = cfg.corner_radius as f32;
    let section_font_size = cfg.section_font_size as f32;
    let input_corner_radius = cfg.input_corner_radius as f32;
    let input_font_size = cfg.input_font_size as f32;
    let input_padding_x = cfg.input_padding_x as f32;
    let swatch_corner_radius = cfg.swatch_corner_radius as f32;
    let swatch_border_width = cfg.swatch_border_width as f32;
    let swatch_highlight_pad = cfg.swatch_highlight_pad as f32;
    let item_radius = menu_cfg.item_corner_radius as f32;
    let item_padding_x = menu_cfg.item_padding_x as f32;
    let item_text_size = menu_cfg.font_size as f32;

    let mut out = Vec::new();
    push_shadow(&mut out, layout.popover_rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.popover_rect,
        radius: corner_radius,
        color: pal.editor_background,
        border_color: pal.editor_border,
        border_width: 1.0,
    }));

    out.push(Primitive::Text(TextRun {
        origin: layout.name_caption_origin,
        text: msg::group_editor::section_group(catalog),
        font: TITLE_FONT,
        size_px: section_font_size,
        color: pal.editor_section_text,
    }));
    let focused_name = editor.focus() == EditorRegion::Name;
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.name_input_rect,
        radius: input_corner_radius,
        color: pal.editor_input_background,
        border_color: if focused_name {
            pal.editor_input_border_focus
        } else {
            pal.editor_input_border
        },
        border_width: 1.0,
    }));
    let buffer = editor.name_buffer();
    let available_text_width = (layout.name_input_rect.width - input_padding_x * 2.0).max(0.0);
    let text_width = measurer.measure_width(buffer, BODY_FONT, input_font_size);
    let text_x = tab_bar::scrolled_text_x(
        layout.name_input_rect.x,
        input_padding_x,
        text_width,
        available_text_width,
    );
    let text_y = layout.name_input_rect.y + (layout.name_input_rect.height - input_font_size) / 2.0;
    out.push(Primitive::PushClip(layout.name_input_rect));
    let selection_range = editor.name_field().selection_range();
    if let Some((start, end)) = selection_range {
        let sel_x0 = text_x + measurer.measure_width(&buffer[..start], BODY_FONT, input_font_size);
        let sel_x1 = text_x + measurer.measure_width(&buffer[..end], BODY_FONT, input_font_size);
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: sel_x0,
                y: layout.name_input_rect.y + 4.0,
                width: sel_x1 - sel_x0,
                height: layout.name_input_rect.height - 8.0,
            },
            color: term_pal.selection_background,
        }));
    }
    out.push(Primitive::Text(TextRun {
        origin: (text_x, text_y),
        text: buffer.to_string(),
        font: BODY_FONT,
        size_px: input_font_size,
        color: pal.editor_input_text,
    }));
    if focused_name && selection_range.is_none() {
        let cursor_width = measurer.measure_width(
            &buffer[..editor.name_field().cursor()],
            BODY_FONT,
            input_font_size,
        );
        let caret_x = (text_x + cursor_width)
            .min(layout.name_input_rect.x + layout.name_input_rect.width - 1.0);
        out.push(Primitive::Quad(Quad {
            rect: Rect {
                x: caret_x,
                y: layout.name_input_rect.y + 4.0,
                width: 1.0,
                height: layout.name_input_rect.height - 8.0,
            },
            color: pal.editor_input_text,
        }));
    }
    out.push(Primitive::PopClip);

    out.push(Primitive::Text(TextRun {
        origin: layout.color_caption_origin,
        text: msg::group_editor::section_color(catalog),
        font: TITLE_FONT,
        size_px: section_font_size,
        color: pal.editor_section_text,
    }));
    for (index, color) in GroupColor::ALL.iter().enumerate() {
        let rect = layout.swatch_rects[index];
        let is_highlighted =
            editor.focus() == EditorRegion::Swatches && editor.swatch_highlight() == index;
        if is_highlighted {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: expand(rect, swatch_highlight_pad),
                radius: swatch_corner_radius + swatch_highlight_pad,
                color: pal.editor_item_hover_background,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let is_current = index == current_color_index;
        out.push(Primitive::RoundedQuad(RoundedQuad {
            rect,
            radius: swatch_corner_radius,
            color: pal.group_color(*color),
            border_color: if is_current {
                pal.editor_swatch_ring
            } else {
                palette::TRANSPARENT
            },
            border_width: swatch_border_width,
        }));
    }

    out.push(Primitive::Quad(Quad {
        rect: layout.divider_rect,
        color: pal.editor_divider,
    }));

    let items = group_menu::group_action_items(catalog, is_collapsed, tab_count);
    for (i, action) in EDITOR_ACTION_ORDER.iter().enumerate() {
        let item = items
            .iter()
            .find(|it| it.action == *action)
            .expect("EDITOR_ACTION_ORDER é subconjunto de GROUP_ACTION_ORDER");
        let rect = layout.action_rects[i];
        let is_highlighted =
            editor.focus() == EditorRegion::Actions && editor.action_highlight() == i;
        if is_highlighted {
            let hover = if item.destructive {
                pal.editor_destructive_hover_background
            } else {
                pal.editor_item_hover_background
            };
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: item_radius,
                color: hover,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let text_color = if item.destructive {
            pal.editor_destructive_foreground
        } else {
            pal.editor_item_foreground
        };
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + item_padding_x,
                rect.y + (rect.height - item_text_size) / 2.0,
            ),
            text: item.label.clone(),
            font: BODY_FONT,
            size_px: item_text_size,
            color: text_color,
        }));
    }

    out
}

pub enum GroupEditorHit {
    NameField,
    Swatch(usize),
    Action(usize),
}

pub fn group_editor_hit(layout: &GroupEditorLayout, point: (f32, f32)) -> Option<GroupEditorHit> {
    if rect_contains(layout.name_input_rect, point) {
        return Some(GroupEditorHit::NameField);
    }
    for (i, &rect) in layout.swatch_rects.iter().enumerate() {
        if rect_contains(rect, point) {
            return Some(GroupEditorHit::Swatch(i));
        }
    }
    for (i, &rect) in layout.action_rects.iter().enumerate() {
        if rect_contains(rect, point) {
            return Some(GroupEditorHit::Action(i));
        }
    }
    None
}

// ---- popover de destino do tab.move_to_group (RF-2.20, ADR-0023 §4) ----
// `[appearance.move_to_group]` -- só geometria; cor reaproveita
// `[appearance.context_menu]` (comentário do TOML: "width = context_menu.
// width", "row_height = context_menu.item_height").

/// Raio do swatch pequeno do popover -- menor que o das outras superfícies
/// de cor (28px no editor, 6px de raio); sem chave própria, valor de
/// trabalho já existente antes desta etapa.
const MOVE_SWATCH_RADIUS: f32 = 2.0;

pub struct MoveToGroupLayout {
    pub popover_rect: Rect,
    /// Só as linhas visíveis na janela de rolagem atual -- alinhadas por
    /// índice com `first_visible_index + i`, não com `targets()` direto.
    pub visible_row_rects: Vec<Rect>,
    pub first_visible_index: usize,
}

/// Deriva a rolagem a partir de `popover.highlighted()`, sem estado de
/// scroll próprio (nota do módulo `move_to_group.rs`): a janela visível
/// sempre contém o item realçado, arrastando o mínimo necessário --mesmo
/// princípio de `WindowState::ensure_active_tab_visible`.
pub fn layout_move_to_group(
    popover: &MoveToGroupPopover,
    config: &porecatu_config::Config,
    window_width: f32,
    window_height: f32,
) -> MoveToGroupLayout {
    let cfg = &config.appearance.move_to_group;
    let width = cfg.width as f32;
    let row_height = cfg.row_height as f32;
    let max_visible_rows = cfg.max_visible_rows as usize;
    let padding = config.appearance.context_menu.padding as f32;

    let total_rows = popover.row_count();
    let visible_rows = total_rows.min(max_visible_rows);
    let max_first = total_rows.saturating_sub(visible_rows);
    let highlighted = popover.highlighted();
    let first_visible_index = highlighted
        .saturating_sub(visible_rows.saturating_sub(1))
        .min(max_first);

    let height = padding * 2.0 + row_height * visible_rows as f32;
    let mut x = popover.anchor.0;
    let mut y = popover.anchor.1;
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    if y + height > window_height {
        y = (window_height - height).max(0.0);
    }
    let popover_rect = Rect {
        x,
        y,
        width,
        height,
    };

    let visible_row_rects = (0..visible_rows)
        .map(|i| Rect {
            x: popover_rect.x + padding,
            y: popover_rect.y + padding + row_height * i as f32,
            width: popover_rect.width - padding * 2.0,
            height: row_height,
        })
        .collect();

    MoveToGroupLayout {
        popover_rect,
        visible_row_rects,
        first_visible_index,
    }
}

pub fn paint_move_to_group(
    layout: &MoveToGroupLayout,
    popover: &MoveToGroupPopover,
    workspace: &Workspace,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    catalog: &Catalog,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let move_cfg = &config.appearance.move_to_group;
    let menu_cfg = &config.appearance.context_menu;
    let corner_radius = menu_cfg.corner_radius as f32;
    let item_radius = menu_cfg.item_corner_radius as f32;
    let row_padding_x = move_cfg.row_padding_x as f32;
    let swatch_size = move_cfg.swatch_size as f32;
    let swatch_gap = move_cfg.swatch_gap as f32;
    let item_text_size = menu_cfg.font_size as f32;

    let mut out = Vec::new();
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.popover_rect,
        radius: corner_radius,
        color: pal.context_menu_background,
        border_color: pal.context_menu_border,
        border_width: 1.0,
    }));
    for (visible_i, &rect) in layout.visible_row_rects.iter().enumerate() {
        let row_index = layout.first_visible_index + visible_i;
        if row_index == popover.highlighted() {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: item_radius,
                color: pal.menu_item_hover,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        if row_index < popover.targets().len() {
            let group_id = popover.targets()[row_index];
            let Some(group) = workspace.group(group_id) else {
                continue;
            };
            let color = group
                .color()
                .map(|c| pal.group_color(c))
                .unwrap_or_else(|| pal.ungrouped_group_color());
            let swatch_rect = Rect {
                x: rect.x,
                y: rect.y + (rect.height - swatch_size) / 2.0,
                width: swatch_size,
                height: swatch_size,
            };
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: swatch_rect,
                radius: MOVE_SWATCH_RADIUS,
                color,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
            let count_text = group.tabs().len().to_string();
            let count_width = measurer.measure_width(
                &count_text,
                tab_bar::PILL_COUNT_FONT,
                tab_bar::PILL_COUNT_FONT_SIZE,
            );
            let name_max_width =
                (rect.width - swatch_size - swatch_gap - row_padding_x - count_width - swatch_gap)
                    .max(0.0);
            let (name, _) = measurer.truncate(
                group.name().unwrap_or_default(),
                BODY_FONT,
                item_text_size,
                name_max_width,
            );
            out.push(Primitive::Text(TextRun {
                origin: (
                    swatch_rect.x + swatch_size + swatch_gap,
                    rect.y + (rect.height - item_text_size) / 2.0,
                ),
                text: name,
                font: BODY_FONT,
                size_px: item_text_size,
                color: pal.menu_item_text,
            }));
            out.push(Primitive::Text(TextRun {
                origin: (
                    rect.x + rect.width - row_padding_x - count_width,
                    rect.y + (rect.height - tab_bar::PILL_COUNT_FONT_SIZE) / 2.0,
                ),
                text: count_text,
                font: tab_bar::PILL_COUNT_FONT,
                size_px: tab_bar::PILL_COUNT_FONT_SIZE,
                color: pal.pill_count_text,
            }));
        } else {
            out.push(Primitive::Text(TextRun {
                origin: (
                    rect.x + row_padding_x,
                    rect.y + (rect.height - item_text_size) / 2.0,
                ),
                text: fit_label(
                    msg::move_to_group::new_group(catalog),
                    (rect.width - row_padding_x * 2.0).max(0.0),
                    item_text_size,
                    measurer,
                ),
                font: BODY_FONT,
                size_px: item_text_size,
                color: pal.menu_item_text,
            }));
        }
    }
    out
}

/// Índice absoluto (não relativo à janela visível) da linha sob `point`,
/// se alguma -- mesmo espaço de índice de `MoveToGroupPopover::
/// set_highlight`/`selected`.
pub fn move_to_group_hit(layout: &MoveToGroupLayout, point: (f32, f32)) -> Option<usize> {
    layout
        .visible_row_rects
        .iter()
        .position(|&rect| rect_contains(rect, point))
        .map(|i| layout.first_visible_index + i)
}

// ---- popover de sessões nomeadas (PRD-014, ADR-0054, ADR-0055 §2) ----
// Sétimo widget de chrome. Superfície e "chip de tecla à direita" do
// item de salvar são os tokens gerais do menu de contexto (§2.16) que
// nenhum widget até aqui tinha consumido de verdade -- `[appearance.
// context_menu]` não tem chave própria pra `gap`/divisor/chip porque
// nada os desenhava. O campo de nome do modo de edição é o mesmo
// componente do editor de grupo (`[appearance.group_editor]`), inclusive
// o `offset_y` de 8px abaixo da barra (ADR-0055 §2: "a regra do editor
// de grupo").

/// Espec §2.16 ("Itens: ... `gap: 10`") -- primeiro item do chrome com
/// ícone à esquerda do rótulo; token geral sem chave própria.
const SAVE_ITEM_ICON_GAP: f32 = 10.0;
/// Espec §2.16 ("Divisor `1px #2a2f38` com `margin: 5px 4px`") -- token
/// geral, sem chave própria (o divisor do editor de grupo usa `gap` em
/// vez de margem em volta do dele).
const SAVE_DIVIDER_HEIGHT: f32 = 1.0;
const SAVE_DIVIDER_MARGIN_Y: f32 = 5.0;
const SAVE_DIVIDER_MARGIN_X: f32 = 4.0;
const CHIP_FONT: FontFace = FontFace::Mono { bold: false };
const CHIP_FONT_SIZE: f32 = 9.5;

pub struct SessionPickerLayout {
    pub popover_rect: Rect,
    /// Item "Salvar esta janela..." (navegação) ou o campo de nome
    /// (edição) -- fixo no topo, não rola.
    pub save_rect: Rect,
    pub divider_rect: Rect,
    /// `Some` só com `entries` vazio: a linha "nenhuma sessão salva",
    /// sem alvo de clique (espec §2.16, "sem hover e sem alvo").
    pub empty_row_rect: Option<Rect>,
    /// Só as linhas visíveis na janela de rolagem atual -- alinhadas por
    /// índice com `first_visible_index + i`, mesmo contrato de
    /// `MoveToGroupLayout`.
    pub visible_row_rects: Vec<Rect>,
    /// O `X` de excluir de cada linha visível, alinhado por índice com
    /// `visible_row_rects` -- mesma anatomia do botão de fechar da aba
    /// (17×17 de desenho, 25×17 de alvo, §1.7).
    pub delete_button_rects: Vec<Rect>,
    pub first_visible_index: usize,
}

/// `bar_width`/`is_macos`: para achar `sessions_button_rect`, de onde a
/// borda direita do popover nasce (ADR-0055 §2: "borda direita alinhada
/// à borda direita do botão"). A janela de rolagem vem de
/// `picker.scroll_top()` direto (não derivada de `highlighted` como em
/// `layout_move_to_group`): a nota do módulo `session_picker.rs` explica
/// por quê -- aqui a rolagem por roda é um gesto próprio.
#[allow(clippy::too_many_arguments)]
pub fn layout_session_picker(
    picker: &SessionPicker,
    config: &porecatu_config::Config,
    style: &TabBarStyle,
    bar_width: f32,
    bar_bottom_y: f32,
    is_macos: bool,
    window_width: f32,
    window_height: f32,
) -> SessionPickerLayout {
    let picker_cfg = &config.appearance.session_picker;
    let menu_cfg = &config.appearance.context_menu;
    let editor_cfg = &config.appearance.group_editor;
    let width = picker_cfg.width as f32;
    let padding = menu_cfg.padding as f32;
    let row_height = picker_cfg.row_height as f32;
    let max_visible_rows = picker_cfg.max_visible_rows as usize;

    let save_height = match picker.mode() {
        Mode::Browsing => menu_cfg.item_height as f32,
        Mode::EditingName(_) => editor_cfg.input_height as f32,
    };

    let total_rows = picker.entries().len();
    let visible_rows = if total_rows == 0 {
        1
    } else {
        total_rows.min(max_visible_rows)
    };
    let rows_height = row_height * visible_rows as f32;
    let content_height =
        save_height + SAVE_DIVIDER_MARGIN_Y * 2.0 + SAVE_DIVIDER_HEIGHT + rows_height;
    let height = padding * 2.0 + content_height;

    let button_rect = tab_bar::sessions_button_rect(style, bar_width, bar_bottom_y, is_macos);
    let mut x = button_rect.x + button_rect.width - width;
    if x < 0.0 {
        x = 0.0;
    }
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    let mut y = bar_bottom_y + editor_cfg.offset_y as f32;
    if y + height > window_height {
        y = (window_height - height).max(0.0);
    }
    let popover_rect = Rect {
        x,
        y,
        width,
        height,
    };

    let inner_x = popover_rect.x + padding;
    let inner_width = popover_rect.width - padding * 2.0;
    let save_rect = Rect {
        x: inner_x,
        y: popover_rect.y + padding,
        width: inner_width,
        height: save_height,
    };
    let divider_rect = Rect {
        x: inner_x + SAVE_DIVIDER_MARGIN_X,
        y: save_rect.y + save_height + SAVE_DIVIDER_MARGIN_Y,
        width: inner_width - SAVE_DIVIDER_MARGIN_X * 2.0,
        height: SAVE_DIVIDER_HEIGHT,
    };
    let rows_top = divider_rect.y + SAVE_DIVIDER_HEIGHT + SAVE_DIVIDER_MARGIN_Y;

    let mut empty_row_rect = None;
    let mut visible_row_rects = Vec::new();
    let mut delete_button_rects = Vec::new();
    let mut first_visible_index = 0;

    if total_rows == 0 {
        empty_row_rect = Some(Rect {
            x: inner_x,
            y: rows_top,
            width: inner_width,
            height: row_height,
        });
    } else {
        let max_first = total_rows.saturating_sub(visible_rows);
        first_visible_index = picker.scroll_top().min(max_first);
        let row_padding_x = picker_cfg.row_padding_x as f32;
        let delete_target_width = style.icon_button_width(style.close_button_size);
        for i in 0..visible_rows {
            let rect = Rect {
                x: inner_x,
                y: rows_top + row_height * i as f32,
                width: inner_width,
                height: row_height,
            };
            let delete_rect = Rect {
                x: rect.x + rect.width - row_padding_x - delete_target_width,
                y: rect.y + (rect.height - style.close_button_size) / 2.0,
                width: delete_target_width,
                height: style.close_button_size,
            };
            visible_row_rects.push(rect);
            delete_button_rects.push(delete_rect);
        }
    }

    SessionPickerLayout {
        popover_rect,
        save_rect,
        divider_rect,
        empty_row_rect,
        visible_row_rects,
        delete_button_rects,
        first_visible_index,
    }
}

/// Rótulo truncado de uma linha, com reticências (espec §2.16) -- função
/// única para pintura e para o alvo do tooltip (`lib.rs::update_hover`),
/// para as duas nunca discordarem de onde o nome foi cortado (a lição do
/// `bar_height` da F3, CLAUDE.md). `max_width` já deve descontar o
/// espaço do `X` de excluir -- reservado sempre, não só sob hover, pra o
/// rótulo não pular de posição quando o `X` aparece/some.
pub fn session_picker_row_label(
    name: &str,
    row_rect: Rect,
    row_padding_x: f32,
    delete_rect: Rect,
    font_size: f32,
    measurer: &mut TextMeasurer,
) -> (String, bool) {
    let max_width = row_label_max_width(row_rect, row_padding_x, delete_rect);
    measurer.truncate(name, BODY_FONT, font_size, max_width)
}

/// Largura útil do rótulo de uma linha: a largura da linha menos o
/// respiro à esquerda e o espaço do `X` de excluir à direita, **sempre**
/// descontado -- ver a nota de [`session_picker_row_label`].
fn row_label_max_width(row_rect: Rect, row_padding_x: f32, delete_rect: Rect) -> f32 {
    (delete_rect.x - row_padding_x - (row_rect.x + row_padding_x)).max(0.0)
}

#[allow(clippy::too_many_arguments)]
pub fn paint_session_picker(
    layout: &SessionPickerLayout,
    picker: &SessionPicker,
    save_chip_label: Option<&str>,
    hovered_delete: Option<usize>,
    config: &porecatu_config::Config,
    style: &TabBarStyle,
    pal: &ResolvedPalette,
    term_pal: &palette::ResolvedTermPalette,
    catalog: &Catalog,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let picker_cfg = &config.appearance.session_picker;
    let menu_cfg = &config.appearance.context_menu;
    let editor_cfg = &config.appearance.group_editor;
    let row_padding_x = picker_cfg.row_padding_x as f32;
    let item_padding_x = menu_cfg.item_padding_x as f32;
    let item_text_size = menu_cfg.font_size as f32;

    let mut out = Vec::new();
    push_shadow(&mut out, layout.popover_rect, menu_cfg.corner_radius as f32);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect: layout.popover_rect,
        radius: menu_cfg.corner_radius as f32,
        color: pal.context_menu_background,
        border_color: pal.context_menu_border,
        border_width: 1.0,
    }));

    // Item "Salvar esta janela..." (navegação) ou campo de nome (edição)
    // -- mesmo retângulo fixo `layout.save_rect`, anatomia diferente por
    // modo (ADR-0055 §2, item 1).
    match picker.mode() {
        Mode::Browsing => {
            let is_highlighted = picker.highlighted() == Highlight::Save;
            if is_highlighted {
                out.push(Primitive::RoundedQuad(RoundedQuad {
                    rect: layout.save_rect,
                    radius: menu_cfg.item_corner_radius as f32,
                    color: pal.menu_item_hover,
                    border_color: palette::TRANSPARENT,
                    border_width: 0.0,
                }));
            }
            let icon_rect = Rect {
                x: layout.save_rect.x + item_padding_x,
                y: layout.save_rect.y,
                width: style.icon_em_size,
                height: layout.save_rect.height,
            };
            out.push(centered_glyph(
                icon::PLUS,
                icon_rect,
                style.icon_em_size,
                pal.menu_item_text,
            ));
            let label_x =
                icon_rect.x + icon::PLUS.ink_width(style.icon_em_size) + SAVE_ITEM_ICON_GAP;
            out.push(Primitive::Text(TextRun {
                origin: (
                    label_x,
                    layout.save_rect.y + (layout.save_rect.height - item_text_size) / 2.0,
                ),
                text: msg::session_picker::save_item(catalog),
                font: BODY_FONT,
                size_px: item_text_size,
                color: pal.menu_item_text,
            }));
            if let Some(chip) = save_chip_label {
                let chip_width = measurer.measure_width(chip, CHIP_FONT, CHIP_FONT_SIZE);
                out.push(Primitive::Text(TextRun {
                    origin: (
                        layout.save_rect.x + layout.save_rect.width - item_padding_x - chip_width,
                        layout.save_rect.y + (layout.save_rect.height - CHIP_FONT_SIZE) / 2.0,
                    ),
                    text: chip.to_string(),
                    font: CHIP_FONT,
                    size_px: CHIP_FONT_SIZE,
                    color: pal.menu_item_disabled_text,
                }));
            }
        }
        Mode::EditingName(field) => {
            // Foco automático (ADR-0055 §2): diferente do editor de
            // grupo, aqui não há outra região pra `Tab` ceder o foco --
            // o campo é sempre o foco enquanto o modo de edição existir.
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect: layout.save_rect,
                radius: editor_cfg.input_corner_radius as f32,
                color: pal.editor_input_background,
                border_color: pal.editor_input_border_focus,
                border_width: 1.0,
            }));
            let input_padding_x = editor_cfg.input_padding_x as f32;
            let input_font_size = editor_cfg.input_font_size as f32;
            let buffer = field.text();
            let available_width = (layout.save_rect.width - input_padding_x * 2.0).max(0.0);
            let text_width = measurer.measure_width(buffer, BODY_FONT, input_font_size);
            let text_x = tab_bar::scrolled_text_x(
                layout.save_rect.x,
                input_padding_x,
                text_width,
                available_width,
            );
            let text_y = layout.save_rect.y + (layout.save_rect.height - input_font_size) / 2.0;
            out.push(Primitive::PushClip(layout.save_rect));
            if buffer.is_empty() {
                out.push(Primitive::Text(TextRun {
                    origin: (text_x, text_y),
                    text: msg::session_picker::name_placeholder(catalog),
                    font: BODY_FONT,
                    size_px: input_font_size,
                    color: pal.menu_item_disabled_text,
                }));
            } else {
                let selection_range = field.selection_range();
                if let Some((start, end)) = selection_range {
                    let sel_x0 = text_x
                        + measurer.measure_width(&buffer[..start], BODY_FONT, input_font_size);
                    let sel_x1 =
                        text_x + measurer.measure_width(&buffer[..end], BODY_FONT, input_font_size);
                    out.push(Primitive::Quad(Quad {
                        rect: Rect {
                            x: sel_x0,
                            y: layout.save_rect.y + 4.0,
                            width: sel_x1 - sel_x0,
                            height: layout.save_rect.height - 8.0,
                        },
                        color: term_pal.selection_background,
                    }));
                }
                out.push(Primitive::Text(TextRun {
                    origin: (text_x, text_y),
                    text: buffer.to_string(),
                    font: BODY_FONT,
                    size_px: input_font_size,
                    color: pal.editor_input_text,
                }));
                if selection_range.is_none() {
                    let cursor_width = measurer.measure_width(
                        &buffer[..field.cursor()],
                        BODY_FONT,
                        input_font_size,
                    );
                    let caret_x = (text_x + cursor_width)
                        .min(layout.save_rect.x + layout.save_rect.width - 1.0);
                    out.push(Primitive::Quad(Quad {
                        rect: Rect {
                            x: caret_x,
                            y: layout.save_rect.y + 4.0,
                            width: 1.0,
                            height: layout.save_rect.height - 8.0,
                        },
                        color: pal.editor_input_text,
                    }));
                }
            }
            out.push(Primitive::PopClip);
        }
    }

    out.push(Primitive::Quad(Quad {
        // `context_menu.separator` não tem campo próprio em
        // `ResolvedPalette` (nada desenhava um até aqui) -- mesmo
        // `#2a2f38` de `editor_divider` (`[appearance.group_editor]
        // divider`), token reaproveitado, não novo.
        rect: layout.divider_rect,
        color: pal.editor_divider,
    }));

    if let Some(rect) = layout.empty_row_rect {
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + row_padding_x,
                rect.y + (rect.height - item_text_size) / 2.0,
            ),
            text: msg::session_picker::empty_list(catalog),
            font: BODY_FONT,
            size_px: item_text_size,
            color: pal.menu_item_disabled_text,
        }));
    }

    for (visible_i, &rect) in layout.visible_row_rects.iter().enumerate() {
        let row_index = layout.first_visible_index + visible_i;
        let Some(entry) = picker.entries().get(row_index) else {
            continue;
        };
        let is_highlighted = picker.highlighted() == Highlight::Row(row_index);
        if is_highlighted {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: menu_cfg.item_corner_radius as f32,
                color: pal.menu_item_hover,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }
        let text_color = if entry.status == EntryStatus::Ok {
            pal.menu_item_text
        } else {
            pal.menu_item_disabled_text
        };
        let delete_rect = layout.delete_button_rects[visible_i];
        let (label, _) = session_picker_row_label(
            &entry.name,
            rect,
            row_padding_x,
            delete_rect,
            item_text_size,
            measurer,
        );
        out.push(Primitive::Text(TextRun {
            origin: (
                rect.x + row_padding_x,
                rect.y + (rect.height - item_text_size) / 2.0,
            ),
            text: label,
            font: BODY_FONT,
            size_px: item_text_size,
            color: text_color,
        }));
        // O `X` só aparece na linha sob o cursor ou realçada pelo
        // teclado -- com um `X` em toda linha, a lista viraria uma
        // coluna de botões destrutivos (ADR-0055 §2).
        if is_highlighted {
            let icon_color = if hovered_delete == Some(row_index) {
                pal.menu_item_destructive_text
            } else {
                pal.menu_item_text
            };
            out.push(centered_glyph(
                icon::X,
                delete_rect,
                style.icon_em_size,
                icon_color,
            ));
        }
    }

    out
}

/// O que está sob `point`, se algo -- checado na mesma ordem de
/// precedência do `X` da aba contra o corpo dela
/// (`hit_test_close_button_wins_over_tab_body`): o `X` primeiro, porque
/// fica **dentro** do retângulo da linha.
pub enum SessionPickerHit {
    /// Item de salvar (navegação) ou campo de nome (edição) -- o mesmo
    /// retângulo fixo, `lib.rs` decide o que fazer por `picker.mode()`.
    SaveArea,
    Row(usize),
    DeleteRow(usize),
}

pub fn session_picker_hit(
    layout: &SessionPickerLayout,
    point: (f32, f32),
) -> Option<SessionPickerHit> {
    if rect_contains(layout.save_rect, point) {
        return Some(SessionPickerHit::SaveArea);
    }
    for (visible_i, &delete_rect) in layout.delete_button_rects.iter().enumerate() {
        if rect_contains(delete_rect, point) {
            return Some(SessionPickerHit::DeleteRow(
                layout.first_visible_index + visible_i,
            ));
        }
    }
    for (visible_i, &rect) in layout.visible_row_rects.iter().enumerate() {
        if rect_contains(rect, point) {
            return Some(SessionPickerHit::Row(
                layout.first_visible_index + visible_i,
            ));
        }
    }
    None
}

// ---- tooltip (espec §2.20, ADR-0019, `[appearance.tooltip]`) ----

pub fn paint_tooltip(
    anchor: Rect,
    text: &str,
    config: &porecatu_config::Config,
    pal: &ResolvedPalette,
    window_width: f32,
    window_height: f32,
    measurer: &mut TextMeasurer,
) -> Vec<Primitive> {
    let cfg = &config.appearance.tooltip;
    let max_width = cfg.max_width as f32;
    let padding_x = cfg.padding_x as f32;
    let padding_y = cfg.padding_y as f32;
    let text_size = cfg.font_size as f32;
    let gap = cfg.gap as f32;
    let corner_radius = cfg.corner_radius as f32;

    let available = (max_width - padding_x * 2.0).max(0.0);
    let (label, _) = measurer.truncate(text, BODY_FONT, text_size, available);
    let text_width = measurer.measure_width(&label, BODY_FONT, text_size);
    let width = text_width + padding_x * 2.0;
    let height = text_size + padding_y * 2.0;

    let mut x = anchor.x;
    let mut y = anchor.y + anchor.height + gap;
    if x + width > window_width {
        x = (window_width - width).max(0.0);
    }
    if y + height > window_height {
        y = anchor.y - gap - height;
    }
    let rect = Rect {
        x,
        y,
        width,
        height,
    };

    let mut out = Vec::new();
    push_shadow(&mut out, rect, corner_radius);
    out.push(Primitive::RoundedQuad(RoundedQuad {
        rect,
        radius: corner_radius,
        color: pal.tooltip_background,
        border_color: pal.tooltip_border,
        border_width: 1.0,
    }));
    out.push(Primitive::Text(TextRun {
        origin: (rect.x + padding_x, rect.y + padding_y),
        text: label,
        font: BODY_FONT,
        size_px: text_size,
        color: pal.tooltip_text,
    }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::test_support;

    fn width_of(text: &str, size: f32, measurer: &mut TextMeasurer) -> f32 {
        measurer.measure_width(text, BODY_FONT, size)
    }

    const ANCHOR: Rect = Rect {
        x: 100.0,
        y: 4.0,
        width: 25.0,
        height: 30.0,
    };

    /// The profile menu hangs below its button with the left edges aligned.
    #[test]
    fn an_anchored_popover_opens_below_and_left_aligned() {
        let (x, y) = anchored_origin(ANCHOR, (200.0, 100.0), (800.0, 600.0), 8.0);
        assert_eq!((x, y), (100.0, 42.0));
    }

    /// Flip on both axes: no room to the right aligns the right edges, no
    /// room below opens above the button.
    #[test]
    fn an_anchored_popover_flips_on_both_axes() {
        let near_corner = Rect {
            x: 700.0,
            y: 560.0,
            ..ANCHOR
        };
        let (x, y) = anchored_origin(near_corner, (200.0, 100.0), (800.0, 600.0), 8.0);
        assert_eq!(x, near_corner.x + near_corner.width - 200.0);
        assert_eq!(y, near_corner.y - 8.0 - 100.0);
    }

    /// Without room on either side the popover is clamped into the window.
    #[test]
    fn an_anchored_popover_that_fits_nowhere_is_clamped() {
        let (x, y) = anchored_origin(ANCHOR, (200.0, 100.0), (150.0, 90.0), 8.0);
        assert_eq!((x, y), (0.0, 0.0));
    }

    /// Between the context menu's width and the notice's: short rows keep the
    /// minimum, long ones grow to fit and stop at the maximum.
    #[test]
    fn the_profile_menu_width_grows_with_its_content_within_the_limits() {
        use crate::profile_menu::{ProfileRow, ProfileRowKind};

        let config = porecatu_config::Config::default();
        let mut measurer = TextMeasurer::new();
        let row = |label: &str, chip: Option<&str>| ProfileRow {
            kind: ProfileRowKind::NewGroup,
            label: label.to_owned(),
            badge: Some("CMD".to_owned()),
            chip: chip.map(str::to_owned),
            separated: false,
        };
        let minimum = config.appearance.context_menu.width as f32;
        let maximum = config.appearance.notices.width as f32;

        let short = profile_menu_width(&[row("sh", None)], &config, &mut measurer);
        assert_eq!(short, minimum);

        let medium = profile_menu_width(
            &[row("Command Prompt", Some("Ctrl+Alt+2"))],
            &config,
            &mut measurer,
        );
        assert!(medium > minimum && medium < maximum, "{medium}");

        let long = "a very long profile name that no menu could ever hold in one line";
        let capped = profile_menu_width(&[row(long, Some("Ctrl+Alt+2"))], &config, &mut measurer);
        assert_eq!(capped, maximum);
    }

    /// The divider sits before the last row and adds its block to the height.
    #[test]
    fn the_profile_menu_layout_separates_the_last_row() {
        use crate::profile_menu::{ProfileMenuTarget, profile_menu_rows};

        let config = porecatu_config::Config::default();
        let rows = profile_menu_rows(&config, &test_support::pt_br(), &Default::default(), &[]);
        let count = rows.len();
        let menu = ProfileMenu::new(ProfileMenuTarget::Ungrouped, ANCHOR, 200.0, rows);
        let layout = layout_profile_menu(&menu, &config, 800.0, 600.0);
        let cfg = &config.appearance.context_menu;
        let item = cfg.item_height as f32;
        let gap = layout.item_rects[count - 1].y - layout.item_rects[count - 2].y;
        assert_eq!(
            gap,
            item + SAVE_DIVIDER_MARGIN_Y * 2.0 + SAVE_DIVIDER_HEIGHT
        );
        assert_eq!(
            layout.menu_rect.height,
            cfg.padding as f32 * 2.0
                + item * count as f32
                + SAVE_DIVIDER_MARGIN_Y * 2.0
                + SAVE_DIVIDER_HEIGHT
        );
    }

    /// Um rótulo mais largo que o orçamento é cortado com reticências, e o
    /// resultado cabe (ADR-0056 §12).
    #[test]
    fn a_label_wider_than_the_budget_is_cut_with_an_ellipsis() {
        let mut measurer = TextMeasurer::new();
        let size = 12.0;
        let long = "Um rótulo artificialmente longo que nenhum menu comportaria".to_owned();
        let budget = 120.0;
        assert!(width_of(&long, size, &mut measurer) > budget);
        let fitted = fit_label(long.clone(), budget, size, &mut measurer);
        assert_ne!(fitted, long);
        assert!(fitted.ends_with('…'), "{fitted}");
        assert!(width_of(&fitted, size, &mut measurer) <= budget + 0.5);
    }

    /// Rótulo que cabe sai idêntico -- e sem ter passado por `truncate`.
    #[test]
    fn a_label_that_fits_is_returned_untouched() {
        let mut measurer = TextMeasurer::new();
        assert_eq!(
            fit_label("Nova aba".to_owned(), 200.0, 12.0, &mut measurer),
            "Nova aba"
        );
        assert_eq!(fit_label(String::new(), 0.0, 12.0, &mut measurer), "");
    }

    /// Todo rótulo de menu dos dois arquivos do projeto cabe no espaço que o
    /// menu de contexto, o de grupo e o popover de destino oferecem, com a
    /// configuração padrão -- então nenhum é cortado, e o desenho de hoje não
    /// muda.
    #[test]
    fn no_menu_label_of_the_project_files_is_cut() {
        let config = porecatu_config::Config::default();
        let menu = &config.appearance.context_menu;
        let size = menu.font_size as f32;
        let move_cfg = &config.appearance.move_to_group;
        let mut measurer = TextMeasurer::new();

        for catalog in [test_support::pt_br(), test_support::en_us()] {
            let mut labels: Vec<String> = Vec::new();
            for item in crate::context_menu::TAB_MENU_ITEMS {
                labels.push(item.action.label(&catalog));
            }
            // O plural mais longo: o da forma `other`, com contagem grande.
            for count in [1, 2, 10, 100] {
                for item in group_menu::group_action_items(&catalog, false, count) {
                    labels.push(item.label);
                }
            }
            labels.push(
                group_menu::group_action_items(&catalog, true, 3)[2]
                    .label
                    .clone(),
            );
            for label in labels {
                let budget = menu.width as f32
                    - menu.padding as f32 * 2.0
                    - menu.item_padding_x as f32 * 2.0;
                assert!(
                    width_of(&label, size, &mut measurer) <= budget,
                    "{label:?} não cabe em {budget}"
                );
                assert_eq!(fit_label(label.clone(), budget, size, &mut measurer), label);
            }
            let new_group = msg::move_to_group::new_group(&catalog);
            let move_budget = menu.width as f32 - move_cfg.row_padding_x as f32 * 2.0;
            assert!(width_of(&new_group, size, &mut measurer) <= move_budget);
        }
    }

    // ---- diálogo de três saídas (RF-16.4, ADR-0060 §4)

    use crate::dialog::{ConfirmDialog, DialogAction};

    fn two_button_dialog() -> ConfirmDialog {
        ConfirmDialog::new(
            "Fechar aba?",
            "Corpo",
            "Fechar aba",
            "Cancelar",
            DialogAction::CloseWindow,
        )
    }

    fn center(rect: Rect) -> (f32, f32) {
        (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    // ---- diálogo de host desconhecido (RF-20.16, ADR-0075 §3)

    /// Uma impressão digital no formato real: `SHA256:` e 43 caracteres de
    /// base64 sem preenchimento (50 no total).
    const REAL_FINGERPRINT: &str = "SHA256:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdE";

    fn host_key_dialog(
        catalog: &porecatu_locale::Catalog,
        known_types: &[&str],
        skipped_lines: usize,
    ) -> ConfirmDialog {
        use crate::dialog::HostKeyFacts;
        use porecatu_core::{PaneId, TabId};
        use porecatu_term::PromptId;
        ConfirmDialog::host_key(
            catalog,
            &HostKeyFacts {
                host: "10.4.1.20".to_owned(),
                algorithm: "ecdsa-sha2-nistp256".to_owned(),
                fingerprint: REAL_FINGERPRINT.to_owned(),
                known_types: known_types.iter().map(|t| (*t).to_owned()).collect(),
                skipped_lines,
            },
            DialogAction::HostKey(TabId::new(0), PaneId::new(0), PromptId::new(0)),
        )
    }

    #[test]
    fn the_host_key_data_lines_fit_the_configured_dialog_in_every_language() {
        let config = porecatu_config::Config::default();
        let cfg = &config.appearance.dialog;
        let mut measurer = TextMeasurer::new();
        for locale in ["en_US", "pt_BR", "es_ES", "fr_FR", "de_DE"] {
            let catalog = test_support::catalog(locale);
            // Every line there can be: another type on file, and skipped lines.
            let dialog = host_key_dialog(&catalog, &["ssh-ed25519"], 12);
            let layout = layout_dialog(1000.0, 700.0, &dialog, &config, &mut measurer);

            // The dialog did not have to grow past the configured width...
            assert_eq!(layout.modal_rect.width, cfg.width as f32, "{locale}");
            // ...and no data line is wider than the content box (so none is cut).
            let budget = layout.modal_rect.width - cfg.padding as f32 * 2.0;
            for line in &dialog.lines {
                let width = measurer.measure_width(&line.text, BODY_FONT, cfg.font_size as f32);
                assert!(
                    width <= budget,
                    "{locale}: {:?} is {width}px, budget {budget}px",
                    line.text
                );
            }
        }
    }

    #[test]
    fn each_extra_line_makes_the_dialog_one_line_taller() {
        let config = porecatu_config::Config::default();
        let cfg = &config.appearance.dialog;
        let catalog = test_support::pt_br();
        let mut measurer = TextMeasurer::new();
        let height = |dialog: &ConfirmDialog, measurer: &mut TextMeasurer| {
            layout_dialog(1000.0, 700.0, dialog, &config, measurer)
                .modal_rect
                .height
        };

        let plain = height(&two_button_dialog(), &mut measurer);
        let basic = host_key_dialog(&catalog, &[], 0);
        let with_type = host_key_dialog(&catalog, &["ssh-ed25519"], 0);
        let with_both = host_key_dialog(&catalog, &["ssh-ed25519"], 3);

        let line = cfg.gap as f32 + cfg.font_size as f32;
        // Type, fingerprint label and fingerprint: three data lines.
        assert_eq!(basic.lines.len(), 3);
        assert_eq!(height(&basic, &mut measurer), plain + 3.0 * line);
        assert_eq!(height(&with_type, &mut measurer), plain + 4.0 * line);
        assert_eq!(height(&with_both, &mut measurer), plain + 5.0 * line);
    }

    #[test]
    fn the_dialog_stays_centered_and_inside_the_window() {
        let config = porecatu_config::Config::default();
        let mut measurer = TextMeasurer::new();
        let dialog = host_key_dialog(&test_support::pt_br(), &["ssh-ed25519"], 2);

        let layout = layout_dialog(1000.0, 700.0, &dialog, &config, &mut measurer);

        let modal = layout.modal_rect;
        assert!(modal.x >= 0.0 && modal.x + modal.width <= 1000.0);
        assert!(modal.y >= 0.0 && modal.y + modal.height <= 700.0);
        assert_eq!(modal.x, (1000.0 - modal.width) / 2.0);
        assert_eq!(modal.y, (700.0 - modal.height) / 2.0);
        // The buttons are still inside the modal, below the last line.
        assert!(layout.confirm_rect.y + layout.confirm_rect.height <= modal.y + modal.height);
    }

    #[test]
    fn the_host_key_dialog_paints_data_in_secondary_and_warnings_in_the_warning_color() {
        let config = porecatu_config::Config::default();
        let pal = ResolvedPalette::from_config(&config);
        let mut measurer = TextMeasurer::new();
        let catalog = test_support::pt_br();
        let dialog = host_key_dialog(&catalog, &["ssh-ed25519"], 2);
        let layout = layout_dialog(1000.0, 700.0, &dialog, &config, &mut measurer);

        let primitives = paint_dialog(
            &layout,
            &dialog,
            &config,
            &pal,
            1000.0,
            700.0,
            None,
            &mut measurer,
        );
        let texts = text_colors(&primitives);
        let color_of = |label: &str| {
            texts
                .iter()
                .find(|(text, _)| text == label)
                .unwrap_or_else(|| panic!("no text {label:?} in {texts:?}"))
                .1
        };

        // "Secundário" #a8b0bb, verbatim: the fingerprint is never truncated.
        let secondary = palette::hex(0xa8, 0xb0, 0xbb);
        assert_eq!(color_of(REAL_FINGERPRINT), secondary);
        assert_eq!(color_of("Tipo: ecdsa-sha2-nistp256"), secondary);
        assert_eq!(color_of("Impressão digital:"), secondary);
        // The warning color of §2.14 (#e0b060).
        let warning = palette::hex(0xe0, 0xb0, 0x60);
        for line in dialog.lines.iter().skip(3) {
            assert_eq!(color_of(&line.text), warning, "{}", line.text);
        }
        // Title and the one-line body keep their own colors.
        assert_eq!(
            color_of("Host desconhecido: 10.4.1.20"),
            palette::hex(0xe6, 0xea, 0xef)
        );
        assert_eq!(
            color_of("A autenticidade deste host não pode ser confirmada."),
            palette::hex(0xd7, 0xdc, 0xe3)
        );
    }

    #[test]
    fn a_two_button_dialog_keeps_its_layout_and_has_no_discard_button() {
        let config = porecatu_config::Config::default();
        let mut measurer = TextMeasurer::new();
        let layout = layout_dialog(1000.0, 700.0, &two_button_dialog(), &config, &mut measurer);
        assert_eq!(layout.discard_rect, None);
        assert_eq!(
            layout.modal_rect.width,
            config.appearance.dialog.width as f32
        );
        // Cancelar à esquerda de Confirmar, com o vão do botão entre eles.
        assert_eq!(
            layout.confirm_rect.x - (layout.cancel_rect.x + layout.cancel_rect.width),
            config.appearance.dialog.button_gap as f32
        );
        assert_eq!(
            dialog_hit(&layout, center(layout.cancel_rect)),
            Some(DialogButton::Cancel)
        );
        assert_eq!(
            dialog_hit(&layout, center(layout.confirm_rect)),
            Some(DialogButton::Confirm)
        );
    }

    #[test]
    fn the_three_button_dialog_lines_up_cancel_discard_save_on_the_right() {
        let config = porecatu_config::Config::default();
        let cfg = &config.appearance.dialog;
        let mut measurer = TextMeasurer::new();
        let dialog = ConfirmDialog::settings_pending(&test_support::pt_br(), 2);
        let layout = layout_dialog(1000.0, 700.0, &dialog, &config, &mut measurer);
        let discard = layout.discard_rect.expect("o botão do meio");
        let gap = cfg.button_gap as f32;
        assert!(layout.cancel_rect.x < discard.x && discard.x < layout.confirm_rect.x);
        assert_eq!(
            discard.x - (layout.cancel_rect.x + layout.cancel_rect.width),
            gap
        );
        assert_eq!(layout.confirm_rect.x - (discard.x + discard.width), gap);
        // O último fica rente ao padding da direita, como no diálogo de dois.
        assert_eq!(
            layout.modal_rect.x + layout.modal_rect.width - cfg.padding as f32,
            layout.confirm_rect.x + layout.confirm_rect.width
        );
        // E os três cabem dentro do diálogo, com a largura configurada.
        assert!(layout.cancel_rect.x >= layout.modal_rect.x + cfg.padding as f32);
        for (rect, button) in [
            (layout.cancel_rect, DialogButton::Cancel),
            (discard, DialogButton::Discard),
            (layout.confirm_rect, DialogButton::Confirm),
        ] {
            assert_eq!(dialog_hit(&layout, center(rect)), Some(button));
        }
        assert_eq!(dialog_hit(&layout, (0.0, 0.0)), None);
    }

    #[test]
    fn the_three_button_dialog_grows_instead_of_cutting_a_long_button() {
        let config = porecatu_config::Config::default();
        let cfg = &config.appearance.dialog;
        let mut measurer = TextMeasurer::new();
        let mut dialog = ConfirmDialog::settings_pending(&test_support::pt_br(), 1);
        dialog.discard_label = Some("Descartar tudo e fechar a janela de configurações ".repeat(3));
        let layout = layout_dialog(1400.0, 700.0, &dialog, &config, &mut measurer);
        assert!(layout.modal_rect.width > cfg.width as f32);
        assert!(layout.cancel_rect.x >= layout.modal_rect.x + cfg.padding as f32);
        // Centrado na janela, como o de largura normal.
        let (mx, _) = center(layout.modal_rect);
        assert!((mx - 700.0).abs() < 0.5);
    }

    fn colors_of(primitives: &[Primitive]) -> Vec<(Rect, Color)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::RoundedQuad(quad) => Some((quad.rect, quad.color)),
                _ => None,
            })
            .collect()
    }

    fn text_colors(primitives: &[Primitive]) -> Vec<(String, Color)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text(run) => Some((run.text.clone(), run.color)),
                _ => None,
            })
            .collect()
    }

    fn secret_dialog(vault_available: bool, retry: bool) -> ConfirmDialog {
        use porecatu_core::{PaneId, TabId};
        use porecatu_term::{PromptId, SecretLabel};
        ConfirmDialog::secret(
            &test_support::pt_br(),
            &SecretLabel::Password {
                user: "deploy".to_owned(),
                host: "10.4.1.20".to_owned(),
            },
            vault_available,
            retry,
            DialogAction::Secret(TabId::new(0), PaneId::new(0), PromptId::new(0)),
        )
    }

    #[test]
    fn every_dialog_has_the_configured_width_and_never_more_than_the_window() {
        let config = porecatu_config::Config::default();
        let mut measurer = TextMeasurer::new();
        let delete =
            ConfirmDialog::delete_profile(&crate::messages::test_support::pt_br(), "prod", 0, true);
        for dialog in [delete, two_button_dialog()] {
            let layout = layout_dialog(1000.0, 700.0, &dialog, &config, &mut measurer);
            assert_eq!(layout.modal_rect.width, 760.0);
            let narrow = layout_dialog(500.0, 700.0, &dialog, &config, &mut measurer);
            assert!(
                narrow.modal_rect.width <= 500.0,
                "{}",
                narrow.modal_rect.width
            );
        }
    }

    #[test]
    fn the_secret_dialog_stacks_field_box_and_buttons_inside_the_configured_modal() {
        let config = porecatu_config::Config::default();
        let mut measurer = TextMeasurer::new();
        let layout = layout_dialog(
            1000.0,
            700.0,
            &secret_dialog(true, false),
            &config,
            &mut measurer,
        );

        let modal = layout.modal_rect;
        let field = layout.field_rect.expect("field");
        let row = layout.remember_rect.expect("box row");
        let toggle = layout.toggle_rect.expect("toggle");
        assert_eq!(modal.width, 760.0);
        assert_eq!(
            field.height,
            config.appearance.group_editor.input_height as f32
        );
        assert!(field.y + field.height <= row.y, "field above the box");
        assert!(
            row.y + row.height <= layout.confirm_rect.y,
            "box above buttons"
        );
        assert!(field.x >= modal.x && field.x + field.width <= modal.x + modal.width);
        assert_eq!(toggle.width, TOGGLE_TRACK_WIDTH);
        assert_eq!(toggle.height, TOGGLE_TRACK_HEIGHT);
        assert!(layout.note_rect.is_none(), "no note when there is a vault");
    }

    #[test]
    fn without_a_vault_the_secret_dialog_grows_by_one_note_line() {
        let config = porecatu_config::Config::default();
        let cfg = &config.appearance.dialog;
        let mut measurer = TextMeasurer::new();
        let with = layout_dialog(
            1000.0,
            700.0,
            &secret_dialog(true, false),
            &config,
            &mut measurer,
        );
        let without = layout_dialog(
            1000.0,
            700.0,
            &secret_dialog(false, false),
            &config,
            &mut measurer,
        );

        assert_eq!(
            without.modal_rect.height,
            with.modal_rect.height + cfg.gap as f32 + cfg.font_size as f32
        );
        let note = without.note_rect.expect("note");
        let row = without.remember_rect.unwrap();
        assert!(note.y >= row.y + row.height);
        assert!(note.y + note.height <= without.confirm_rect.y);
    }

    #[test]
    fn a_secret_dialog_is_taller_than_the_plain_one_by_the_field_and_the_box() {
        let config = porecatu_config::Config::default();
        let cfg = &config.appearance.dialog;
        let mut measurer = TextMeasurer::new();
        let plain = layout_dialog(1000.0, 700.0, &two_button_dialog(), &config, &mut measurer);
        let secret = layout_dialog(
            1000.0,
            700.0,
            &secret_dialog(true, false),
            &config,
            &mut measurer,
        );

        let gap = cfg.gap as f32;
        assert_eq!(
            secret.modal_rect.height,
            plain.modal_rect.height
                + gap
                + config.appearance.group_editor.input_height as f32
                + gap
                + TOGGLE_TRACK_HEIGHT
        );
    }

    #[test]
    fn the_masked_field_paints_dots_and_never_the_typed_text() {
        use crate::masked_field::MaskedFieldState;
        let config = porecatu_config::Config::default();
        let pal = ResolvedPalette::from_config(&config);
        let mut measurer = TextMeasurer::new();
        let dialog = secret_dialog(true, false);
        let layout = layout_dialog(1000.0, 700.0, &dialog, &config, &mut measurer);
        let mut field = MaskedFieldState::new();
        field.paste("tr0ub4dor-proof");
        let view = SecretView {
            field: &field,
            selection_color: palette::hex(1, 2, 3),
        };

        let primitives = paint_dialog(
            &layout,
            &dialog,
            &config,
            &pal,
            1000.0,
            700.0,
            Some(&view),
            &mut measurer,
        );

        let texts: Vec<String> = text_colors(&primitives)
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        assert!(texts.iter().any(|t| *t == "•".repeat(15)), "{texts:?}");
        assert!(
            texts.iter().all(|t| !t.contains("tr0ub4dor")),
            "the typed text is never drawn: {texts:?}"
        );
    }

    #[test]
    fn a_refused_secret_paints_the_error_line_in_the_error_color() {
        let config = porecatu_config::Config::default();
        let pal = ResolvedPalette::from_config(&config);
        let mut measurer = TextMeasurer::new();
        let field = crate::masked_field::MaskedFieldState::new();
        let view = SecretView {
            field: &field,
            selection_color: palette::hex(1, 2, 3),
        };
        let paint = |retry: bool, measurer: &mut TextMeasurer| {
            let dialog = secret_dialog(true, retry);
            let layout = layout_dialog(1000.0, 700.0, &dialog, &config, measurer);
            text_colors(&paint_dialog(
                &layout,
                &dialog,
                &config,
                &pal,
                1000.0,
                700.0,
                Some(&view),
                measurer,
            ))
        };

        let refused = paint(true, &mut measurer);
        let fresh = paint(false, &mut measurer);

        let error = refused
            .iter()
            .find(|(t, _)| t == "O servidor recusou a senha.")
            .expect("the error line");
        assert_eq!(error.1, pal.warning_severity_error);
        let body = fresh.iter().find(|(t, _)| t == "deploy@10.4.1.20").unwrap();
        assert_eq!(body.1, pal.dialog_body_text);
    }

    #[test]
    fn connect_is_the_primary_button_and_not_the_destructive_one() {
        let config = porecatu_config::Config::default();
        let pal = ResolvedPalette::from_config(&config);
        let mut measurer = TextMeasurer::new();
        let field = crate::masked_field::MaskedFieldState::new();
        let view = SecretView {
            field: &field,
            selection_color: palette::hex(1, 2, 3),
        };
        let dialog = secret_dialog(true, false);
        let layout = layout_dialog(1000.0, 700.0, &dialog, &config, &mut measurer);

        let primitives = paint_dialog(
            &layout,
            &dialog,
            &config,
            &pal,
            1000.0,
            700.0,
            Some(&view),
            &mut measurer,
        );

        let confirm = colors_of(&primitives)
            .into_iter()
            .find(|(rect, _)| *rect == layout.confirm_rect)
            .map(|(_, color)| color)
            .expect("the connect button");
        assert_eq!(confirm, crate::settings::TOGGLE_ON);
        assert_ne!(confirm, pal.dialog_confirm_background);
        let label = text_colors(&primitives)
            .into_iter()
            .find(|(text, _)| text == "Conectar")
            .expect("label");
        assert_eq!(label.1, crate::toggle::TOGGLE_KNOB_COLOR);
    }

    #[test]
    fn the_button_focus_ring_only_shows_when_the_buttons_have_the_focus() {
        use crate::dialog::SecretFocus;
        let config = porecatu_config::Config::default();
        let pal = ResolvedPalette::from_config(&config);
        let mut measurer = TextMeasurer::new();
        let field = crate::masked_field::MaskedFieldState::new();
        let view = SecretView {
            field: &field,
            selection_color: palette::hex(1, 2, 3),
        };
        let rings = |dialog: &ConfirmDialog, measurer: &mut TextMeasurer| {
            let layout = layout_dialog(1000.0, 700.0, dialog, &config, measurer);
            paint_dialog(
                &layout,
                dialog,
                &config,
                &pal,
                1000.0,
                700.0,
                Some(&view),
                measurer,
            )
            .iter()
            .filter(|p| {
                matches!(p, Primitive::RoundedQuad(q)
                    if q.border_color == pal.dialog_focus_ring && q.border_width == 1.0)
            })
            .count()
        };

        let mut dialog = secret_dialog(true, false);
        // The field has the focus at the start: one ring, on the field.
        assert_eq!(rings(&dialog, &mut measurer), 1);
        dialog.focus_secret(SecretFocus::Remember);
        assert_eq!(
            rings(&dialog, &mut measurer),
            1,
            "the ring moves to the box"
        );
        dialog.focus_secret(SecretFocus::Buttons);
        assert_eq!(rings(&dialog, &mut measurer), 1, "and then to the button");
    }

    #[test]
    fn the_box_is_off_and_dim_without_a_vault_even_if_it_was_ticked() {
        let config = porecatu_config::Config::default();
        let pal = ResolvedPalette::from_config(&config);
        let mut measurer = TextMeasurer::new();
        let field = crate::masked_field::MaskedFieldState::new();
        let view = SecretView {
            field: &field,
            selection_color: palette::hex(1, 2, 3),
        };
        let mut dialog = secret_dialog(false, false);
        if let Some(facts) = dialog.secret.as_mut() {
            facts.remember = true;
        }
        let layout = layout_dialog(1000.0, 700.0, &dialog, &config, &mut measurer);

        let primitives = paint_dialog(
            &layout,
            &dialog,
            &config,
            &pal,
            1000.0,
            700.0,
            Some(&view),
            &mut measurer,
        );

        let track = layout.toggle_rect.unwrap();
        let track_color = colors_of(&primitives)
            .into_iter()
            .find(|(rect, _)| *rect == track)
            .map(|(_, color)| color)
            .expect("track");
        assert_eq!(track_color, crate::settings::TOGGLE_OFF);
        let texts = text_colors(&primitives);
        let label = texts
            .iter()
            .find(|(t, _)| t == "Lembrar no cofre do sistema")
            .unwrap();
        assert_eq!(label.1, pal.menu_item_disabled_text);
        let note = texts
            .iter()
            .find(|(t, _)| t == "O cofre do sistema não está disponível.")
            .expect("the dim note");
        assert_eq!(note.1, pal.menu_item_disabled_text);
    }

    #[test]
    fn clicks_land_on_the_field_the_box_or_nothing() {
        let config = porecatu_config::Config::default();
        let mut measurer = TextMeasurer::new();
        let layout = layout_dialog(
            1000.0,
            700.0,
            &secret_dialog(true, false),
            &config,
            &mut measurer,
        );

        assert_eq!(
            secret_hit(&layout, center(layout.field_rect.unwrap())),
            Some(SecretHit::Field)
        );
        assert_eq!(
            secret_hit(&layout, center(layout.remember_rect.unwrap())),
            Some(SecretHit::Remember)
        );
        assert_eq!(secret_hit(&layout, center(layout.confirm_rect)), None);
        // A dialog with no secret has neither.
        let plain = layout_dialog(1000.0, 700.0, &two_button_dialog(), &config, &mut measurer);
        assert_eq!(secret_hit(&plain, center(plain.modal_rect)), None);
    }

    #[test]
    fn a_click_in_the_field_picks_the_dot_under_it() {
        let config = porecatu_config::Config::default();
        let mut measurer = TextMeasurer::new();
        let layout = layout_dialog(
            1000.0,
            700.0,
            &secret_dialog(true, false),
            &config,
            &mut measurer,
        );
        let field = layout.field_rect.unwrap();
        let editor = &config.appearance.group_editor;
        let dot = measurer.measure_width("•", MASK_FONT, editor.input_font_size as f32);
        let start = field.x + editor.input_padding_x as f32;

        let at = |x: f32, measurer: &mut TextMeasurer| {
            secret_field_scalar_at(&layout, x, &config, 5, measurer)
        };

        assert_eq!(at(start - 20.0, &mut measurer), 0, "left of the text");
        assert_eq!(at(start + dot * 2.1, &mut measurer), 2);
        assert_eq!(
            at(start + dot * 99.0, &mut measurer),
            5,
            "clamped to the end"
        );
    }

    #[test]
    fn a_long_key_path_is_cut_in_the_middle_and_keeps_both_ends() {
        let mut measurer = TextMeasurer::new();
        let path = "/home/someone/.ssh/work/projects/very/long/folder/names/id_ed25519";
        let full = measurer.measure_width(path, BODY_FONT, 12.5);

        let cut = truncate_middle(path, BODY_FONT, 12.5, full / 2.0, &mut measurer);

        assert!(cut.contains('…'));
        assert!(cut.starts_with("/home"), "{cut}");
        assert!(
            cut.ends_with("id_ed25519") || cut.ends_with("ed25519"),
            "{cut}"
        );
        assert!(measurer.measure_width(&cut, BODY_FONT, 12.5) <= full / 2.0);
        // What fits is left alone.
        assert_eq!(
            truncate_middle("short", BODY_FONT, 12.5, 1000.0, &mut measurer),
            "short"
        );
    }

    #[test]
    fn the_secret_dialog_fits_in_every_language() {
        let config = porecatu_config::Config::default();
        let mut measurer = TextMeasurer::new();
        for catalog in test_support::all_catalogs() {
            let dialog = ConfirmDialog::secret(
                &catalog,
                &porecatu_term::SecretLabel::Password {
                    user: "deploy".to_owned(),
                    host: "10.4.1.20".to_owned(),
                },
                false,
                true,
                DialogAction::CloseWindow,
            );
            let layout = layout_dialog(1000.0, 700.0, &dialog, &config, &mut measurer);
            let facts = dialog.secret.as_ref().unwrap();
            let row = layout.remember_rect.unwrap();
            let label = measurer.measure_width(
                &facts.remember_label,
                BODY_FONT,
                config.appearance.dialog.font_size as f32,
            );
            assert!(
                TOGGLE_TRACK_WIDTH + config.appearance.dialog.button_gap as f32 + label
                    <= row.width,
                "the box label fits its row: {}",
                facts.remember_label
            );
            assert!(layout.confirm_rect.x >= layout.modal_rect.x);
        }
    }

    #[test]
    fn the_three_button_dialog_paints_save_primary_and_discard_destructive() {
        let config = porecatu_config::Config::default();
        let pal = ResolvedPalette::from_config(&config);
        let mut measurer = TextMeasurer::new();
        let mut dialog = ConfirmDialog::settings_pending(&test_support::pt_br(), 2);
        let layout = layout_dialog(1000.0, 700.0, &dialog, &config, &mut measurer);
        let paint = |dialog: &ConfirmDialog, measurer: &mut TextMeasurer| {
            paint_dialog(
                &layout, dialog, &config, &pal, 1000.0, 700.0, None, measurer,
            )
        };
        let primitives = paint(&dialog, &mut measurer);
        let fills = colors_of(&primitives);
        let fill_of = |rect: Rect| fills.iter().find(|(r, _)| *r == rect).unwrap().1;
        // Salvar e fechar: o par ligado da alternância (`#3f8f80`, texto `#f0f3f6`).
        assert_eq!(fill_of(layout.confirm_rect), palette::hex(0x3f, 0x8f, 0x80));
        let texts = text_colors(&primitives);
        let text_of = |label: &str| texts.iter().find(|(t, _)| t == label).unwrap().1;
        assert_eq!(text_of("Salvar e fechar"), palette::hex(0xf0, 0xf3, 0xf6));
        // Descartar e fechar: texto destrutivo `#e08585`, sem fundo.
        assert_eq!(
            text_of("Descartar e fechar"),
            palette::hex(0xe0, 0x85, 0x85)
        );
        assert_eq!(fill_of(layout.discard_rect.unwrap()), palette::TRANSPARENT);
        // Cancelar: o texto do botão cancelar.
        assert_eq!(text_of("Cancelar"), pal.dialog_cancel_text);

        // Hover: o destrutivo ganha `#2e2224`; o primário clareia.
        dialog.set_hovered(Some(DialogButton::Discard));
        let fills = colors_of(&paint(&dialog, &mut measurer));
        let discard = fills
            .iter()
            .find(|(r, _)| *r == layout.discard_rect.unwrap())
            .unwrap()
            .1;
        assert_eq!(discard, palette::hex(0x2e, 0x22, 0x24));
        dialog.set_hovered(Some(DialogButton::Confirm));
        let fills = colors_of(&paint(&dialog, &mut measurer));
        let save = fills
            .iter()
            .find(|(r, _)| *r == layout.confirm_rect)
            .unwrap()
            .1;
        assert_eq!(
            save,
            crate::chrome::brighten(
                palette::hex(0x3f, 0x8f, 0x80),
                config.appearance.tabs.colors.hover_brightness
            )
        );
    }

    #[test]
    fn a_two_button_dialog_ignores_hover_and_paints_the_destructive_confirm_as_before() {
        let config = porecatu_config::Config::default();
        let pal = ResolvedPalette::from_config(&config);
        let mut measurer = TextMeasurer::new();
        let mut dialog = two_button_dialog();
        let layout = layout_dialog(1000.0, 700.0, &dialog, &config, &mut measurer);
        let plain = paint_dialog(
            &layout,
            &dialog,
            &config,
            &pal,
            1000.0,
            700.0,
            None,
            &mut measurer,
        );
        dialog.set_hovered(Some(DialogButton::Confirm));
        let hovered = paint_dialog(
            &layout,
            &dialog,
            &config,
            &pal,
            1000.0,
            700.0,
            None,
            &mut measurer,
        );
        assert_eq!(plain.len(), hovered.len());
        let fills = colors_of(&plain);
        // O confirmar segue cheio de `#e08585`, o destrutivo de sempre.
        assert_eq!(
            fills
                .iter()
                .find(|(r, _)| *r == layout.confirm_rect)
                .unwrap()
                .1,
            pal.dialog_confirm_background
        );
        assert_eq!(colors_of(&plain), colors_of(&hovered));
    }
}
