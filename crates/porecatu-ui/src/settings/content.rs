// SPDX-License-Identifier: GPL-3.0-or-later

//! O que o painel mostra e as medidas que dependem do texto: o modelo de
//! visão de um grupo (seções, linhas, o que cada controle desenha), já com o
//! texto truncado e as larguras medidas. É montado **quando o layout muda**
//! -- troca de grupo, de largura, de idioma ou de config -- e guardado:
//! medir texto por frame é a armadilha de desempenho do CLAUDE.md, e a
//! pintura, o hit-test e a árvore de acessibilidade só leem o que está aqui.
//!
//! Os valores vêm do rascunho (`Draft`): o que o arquivo diz, com as pendências
//! por cima. Cada mudança no rascunho sobe a geração e refaz o conteúdo -- é
//! uma mudança de chave, não de quadro, então o custo de medir fica fora do
//! caminho de pintura.

use porecatu_config::EditValue;
use porecatu_core::Action;
use porecatu_locale::Catalog;
use porecatu_render::{Color, FontFace, Rect, TextMeasurer};
use porecatu_term::TermColor;

use super::actions::label as action_label;
use super::catalog::{Control, Group, OptionDef, ReloadScope, Section, options_in};
use super::draft::{Draft, ValueError};
use super::field_edit::{display_text, number_text};
use super::file_state::Banner;
use super::layout::{self, BannerButton, BlockSpec, ControlPart, Metrics, PanelGeometry};
use super::shortcuts::{Capturing, Shortcuts};
use crate::keymap::Chord;
use crate::messages::msg;
use crate::overlay::BODY_FONT;
use crate::palette::ResolvedTermPalette;
use crate::tab_bar::rect_contains;

/// Quantos quadrados tem a amostra de um tema: fundo, texto e as oito ANSI
/// normais (RF-16.24).
pub(crate) const SWATCH_COUNT: usize = 10;
/// Vão entre os quadrados da amostra (ADR-0060 §3: `gap: 2`).
pub(crate) const SWATCH_GAP: f32 = 2.0;

/// A fonte do chip de atalho: mono, 400 (ADR-0060 §3).
pub(crate) const CHIP_FONT: FontFace = FontFace::Mono { bold: false };

/// Como um chip de atalho se desenha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChipTone {
    /// Uma combinação: o texto do chip.
    Normal,
    /// "Nenhum atalho", esmaecido.
    Muted,
    /// Em captura (RF-16.29): borda Acento e "pressione as teclas…".
    Capturing,
}

/// Um chip de atalho: o texto, a largura já medida e o tom.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChipView {
    pub text: String,
    pub width: f32,
    pub tone: ChipTone,
}

/// O que mais o conteúdo lê além do rascunho: o tema da sessão (RF-16.25), o
/// grupo Atalhos e a faixa de arquivo (ADR-0060 §2). Tudo opcional: sem nada
/// é o conteúdo de um grupo de opções com o arquivo em ordem.
#[derive(Default)]
pub(crate) struct ViewExtras<'a> {
    /// O tema que `theme.cycle` pôs na sessão, se pôs.
    pub session_theme: Option<&'a str>,
    pub shortcuts: Option<&'a ShortcutsView<'a>>,
    pub banner: Option<&'a Banner>,
    /// O arquivo de configuração em uso: a base de um caminho relativo da
    /// imagem de fundo (RF-17.3), para a nota de "não encontrado" (RF-17.21).
    pub config_path: Option<&'a std::path::Path>,
}

/// Um botão da faixa, com a largura já medida.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BannerButtonView {
    pub button: BannerButton,
    pub label: String,
    pub width: f32,
}

/// A faixa como a tela a desenha: o tom, o título, o corpo (o erro, na de
/// arquivo inválido) já cortados ao que cabe, e os botões.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BannerView {
    /// Erro (arquivo inválido) ou aviso (arquivo alterado fora).
    pub error: bool,
    pub title: String,
    pub body: Option<String>,
    pub buttons: Vec<BannerButtonView>,
}

/// O que o grupo Atalhos lê para montar o conteúdo: o estado da edição, o
/// filtro e a captura em curso.
pub(crate) struct ShortcutsView<'a> {
    pub state: &'a Shortcuts,
    pub filter: &'a str,
    pub capturing: Option<&'a Capturing>,
}

/// Um item de lista como a tela o mostra.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ListItemView {
    /// O texto, ou o nome da variável.
    pub first: String,
    /// O valor da variável; vazio numa lista de textos.
    pub second: String,
    /// Nome vazio ou repetido (RF-16.18): a borda do campo fica em Erro.
    pub invalid: bool,
}

/// O desenho do controle de uma linha, com o valor em vigor.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ControlView {
    Toggle {
        on: bool,
    },
    /// Campo de texto ou numérico (este alinhado à direita, ADR-0060 §3).
    Field {
        text: String,
        width: f32,
        right_aligned: bool,
        /// Largura do texto, medida uma vez quando o conteúdo é montado: o
        /// campo numérico alinha à direita e não pode medir por quadro.
        text_width: f32,
    },
    /// Escolha de até três valores: botões colados.
    Segmented {
        labels: Vec<String>,
        widths: Vec<f32>,
        selected: usize,
    },
    /// Escolha com mais valores: botão com caret.
    Choice {
        text: String,
    },
    /// Lista de textos (`shell.args`, `trusted_paths`) ou de nome e valor
    /// (`shell.env`): um campo -- dois, na de nome e valor -- por item, com o
    /// `X` à direita, e o item "Adicionar" no fim.
    List {
        items: Vec<ListItemView>,
        /// Dois campos por item: nome e valor.
        two_fields: bool,
        add_label: String,
    },
    /// Amostra de um tema.
    Themes {
        /// O nome do tema como o arquivo o grava; vazio é "sem tema".
        name: String,
        colors: Box<[Color; SWATCH_COUNT]>,
        selected: bool,
    },
    /// `git.remote_poll_interval_secs`: alternância mais o número.
    GitPoll {
        on: bool,
        seconds: String,
        /// Largura do número, medida uma vez (o campo alinha à direita).
        text_width: f32,
    },
    /// Os atalhos de uma ação, lado a lado (RF-16.28).
    Chips {
        chips: Vec<ChipView>,
    },
}

impl ControlView {
    /// O tamanho do controle, que o layout do painel precisa antes de saber
    /// onde pô-lo.
    pub(crate) fn size(&self, m: &Metrics) -> (f32, f32) {
        match self {
            ControlView::Toggle { .. } => (
                crate::toggle::TOGGLE_TRACK_WIDTH,
                crate::toggle::TOGGLE_TRACK_HEIGHT,
            ),
            ControlView::Field { width, .. } => (*width, m.field_height),
            ControlView::Segmented { widths, .. } => (widths.iter().sum(), m.button_height),
            ControlView::Choice { .. } => (m.text_field_width, m.field_height),
            ControlView::List { items, .. } => (
                m.text_field_width + m.list_gap + m.restore_width,
                items.len() as f32 * (m.field_height + m.list_gap) + m.sidebar_item_height,
            ),
            ControlView::Themes { .. } => (
                SWATCH_COUNT as f32 * m.swatch_size + (SWATCH_COUNT - 1) as f32 * SWATCH_GAP,
                m.swatch_size,
            ),
            ControlView::GitPoll { .. } => (
                crate::toggle::TOGGLE_TRACK_WIDTH + m.row_gap + m.number_field_width,
                m.field_height,
            ),
            ControlView::Chips { chips } => (
                chips.iter().map(|chip| chip.width).sum::<f32>()
                    + chips.len().saturating_sub(1) as f32 * m.list_gap,
                m.chip_height,
            ),
        }
    }

    /// A parte do controle sob `point`, com o controle em `rect` (coordenadas
    /// de janela). `None` fora dele, e no tema, que é a linha inteira.
    pub(crate) fn part_at(
        &self,
        rect: Rect,
        m: &Metrics,
        point: (f32, f32),
    ) -> Option<ControlPart> {
        if !rect_contains(rect, point) {
            return None;
        }
        match self {
            ControlView::Toggle { .. } | ControlView::Field { .. } | ControlView::Choice { .. } => {
                Some(ControlPart::Whole)
            }
            ControlView::Segmented { widths, .. } => {
                let mut x = rect.x;
                for (index, width) in widths.iter().enumerate() {
                    if point.0 < x + width {
                        return Some(ControlPart::Segment(index));
                    }
                    x += width;
                }
                None
            }
            ControlView::GitPoll { .. } => {
                if point.0 < rect.x + crate::toggle::TOGGLE_TRACK_WIDTH {
                    Some(ControlPart::GitToggle)
                } else if point.0 >= rect.x + rect.width - m.number_field_width {
                    Some(ControlPart::GitNumber)
                } else {
                    None
                }
            }
            ControlView::List {
                items, two_fields, ..
            } => {
                let geometry = layout::list_geometry(rect, m, items.len(), *two_fields);
                if rect_contains(geometry.add, point) {
                    return Some(ControlPart::ListAdd);
                }
                geometry.items.iter().enumerate().find_map(|(item, rects)| {
                    if rect_contains(rects.remove, point) {
                        Some(ControlPart::ListRemove(item))
                    } else if rect_contains(rects.first, point) {
                        Some(ControlPart::ListField {
                            item,
                            second: false,
                        })
                    } else if rects
                        .second
                        .is_some_and(|second| rect_contains(second, point))
                    {
                        Some(ControlPart::ListField { item, second: true })
                    } else {
                        None
                    }
                })
            }
            ControlView::Themes { .. } => None,
            ControlView::Chips { chips } => {
                let mut x = rect.x;
                for (index, chip) in chips.iter().enumerate() {
                    if point.0 < x {
                        // No vão entre dois chips.
                        return None;
                    }
                    if point.0 < x + chip.width {
                        return Some(ControlPart::Chip(index));
                    }
                    x += chip.width + m.list_gap;
                }
                None
            }
        }
    }
}

/// Uma linha de opção, com o texto já cortado ao orçamento dela.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowView {
    /// O identificador da opção (`font_size`); `None` na linha de um tema.
    pub option: Option<&'static str>,
    pub name: String,
    /// Descrição cortada com reticências; vazia numa linha de tema.
    pub description: String,
    /// A descrição inteira -- para o tooltip e para a árvore de
    /// acessibilidade --, e se o corte aconteceu.
    pub description_full: String,
    pub description_truncated: bool,
    /// O escopo de classe C ("vale em aba nova") e onde ele começa, em x
    /// relativo à origem do nome (RF-16.13, ADR-0060 §2).
    pub scope: Option<(String, f32)>,
    pub control: ControlView,
    /// O valor em vigor como texto, para a árvore de acessibilidade.
    pub value_text: String,
    /// A opção tem alteração pendente: o ponto 6×6 na linha (ADR-0060 §2).
    pub pending: bool,
    /// O valor digitado foi recusado: a razão, já cortada ao orçamento da
    /// linha, vai abaixo da descrição e a borda do controle fica em Erro
    /// (RF-16.18).
    pub invalid: Option<String>,
    /// "Restaurar padrão" está disponível: o valor em vista difere do padrão.
    pub can_reset: bool,
    /// A ação de uma linha do grupo Atalhos; `None` nas linhas de opção.
    pub action: Option<Action>,
    /// Uma linha de aviso em 11px na cor de Aviso, abaixo do nome: o conflito
    /// de atalho (RF-16.30) e a combinação que o terminal perde (RF-16.29).
    /// Cortada ao orçamento da linha, como a razão de um valor recusado.
    pub notice: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Block {
    Section(String),
    Row(Box<RowView>),
    /// Texto solto entre as linhas, já quebrado nas linhas que cabem.
    Note {
        lines: Vec<String>,
        tone: NoteTone,
    },
    /// O campo de filtro do grupo Atalhos: o texto digitado e a frase que
    /// aparece enquanto ele está vazio.
    Filter {
        text: String,
        placeholder: String,
    },
}

/// De que cor é uma nota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoteTone {
    /// O aviso dos diretórios autorizados (RF-16.27): o tom de Aviso.
    Warning,
    /// A linha do tema da sessão (RF-16.25): o Terciário `#828a96`.
    Muted,
}

/// O que invalida o conteúdo guardado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentKey {
    pub group: Group,
    /// Largura do painel, em bits (comparar `f32` por igualdade exata é o
    /// que se quer: o mesmo número, a mesma medida).
    pub panel_width_bits: u32,
    /// Sobe a cada troca de catálogo (idioma) e a cada recarga de config.
    pub generation: u64,
}

pub(crate) struct Content {
    pub key: ContentKey,
    pub title: String,
    pub blocks: Vec<Block>,
    /// A faixa do topo do painel, se a há.
    pub banner: Option<BannerView>,
    pub geometry: PanelGeometry,
    /// A largura de cada botão do rodapé, na ordem de `FOOTER_BUTTONS`.
    pub footer_widths: [f32; 3],
}

impl Content {
    /// Os índices dos blocos que são linhas -- a ordem do `Tab`.
    pub(crate) fn row_indices(&self) -> Vec<usize> {
        self.blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| matches!(block, Block::Row(_) | Block::Filter { .. }))
            .map(|(index, _)| index)
            .collect()
    }

    pub(crate) fn row(&self, block: usize) -> Option<&RowView> {
        match self.blocks.get(block)? {
            Block::Row(row) => Some(row),
            Block::Section(_) | Block::Note { .. } | Block::Filter { .. } => None,
        }
    }
}

/// Quebra `text` em linhas de no máximo `width` px, entre palavras. Uma
/// palavra mais larga que a linha fica sozinha, inteira: o corte seria pior
/// que o estouro de uma palavra.
fn wrap(text: &str, size: f32, width: f32, measurer: &mut TextMeasurer) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if current.is_empty() || measurer.measure_width(&candidate, BODY_FONT, size) <= width {
            current = candidate;
        } else {
            lines.push(std::mem::replace(&mut current, word.to_owned()));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Monta o conteúdo de `group`: mede o texto uma vez e guarda o resultado.
pub(crate) fn build(
    key: ContentKey,
    draft: &Draft,
    extras: &ViewExtras<'_>,
    catalog: &Catalog,
    m: &Metrics,
    measurer: &mut TextMeasurer,
) -> Content {
    let session_theme = extras.session_theme;
    let shortcuts = extras.shortcuts;
    let panel_width = f32::from_bits(key.panel_width_bits);
    let mut blocks = Vec::new();
    let mut current_section: Option<Section> = None;
    if let (Group::Shortcuts, Some(view)) = (key.group, shortcuts) {
        push_shortcut_blocks(&mut blocks, view, catalog, m, panel_width, measurer);
    }
    for option in options_in(key.group) {
        if current_section != Some(option.section) {
            blocks.push(Block::Section(option.section.label(catalog)));
            current_section = Some(option.section);
        }
        if option.id == "trusted_paths" {
            // RF-16.27: o aviso fica acima da lista, onde ela é editada.
            let width = (panel_width - m.panel_padding * 2.0).max(0.0);
            let text = msg::settings::option::trusted_paths_warning(catalog);
            blocks.push(Block::Note {
                lines: wrap(&text, m.description_size, width, measurer),
                tone: NoteTone::Warning,
            });
        }
        if option.control == Control::Theme {
            push_theme_rows(
                &mut blocks,
                option,
                draft,
                session_theme,
                catalog,
                m,
                panel_width,
                measurer,
            );
        } else {
            blocks.push(Block::Row(Box::new(option_row(
                option,
                draft,
                catalog,
                m,
                panel_width,
                measurer,
            ))));
        }
        if option.id == "background_image"
            && let EditValue::String(raw) = draft.value(option)
            && let Some(missing) = crate::background_image::missing_file(extras.config_path, &raw)
        {
            // RF-17.21: abaixo do campo, no estilo e no lugar do aviso de
            // `trusted_paths` (nota de Aviso) -- e **não** na razão de um valor
            // recusado: ela não impede o Salvar. Só a falta do arquivo se sabe
            // sem decodificar; formato e corrupção só saem no aviso da recarga.
            let width = (panel_width - m.panel_padding * 2.0).max(0.0);
            let text =
                msg::settings::option::background_image_not_found(catalog, missing.display());
            blocks.push(Block::Note {
                lines: wrap(&text, m.description_size, width, measurer),
                tone: NoteTone::Warning,
            });
        }
    }

    let specs: Vec<BlockSpec> = blocks
        .iter()
        .map(|block| match block {
            Block::Section(_) => BlockSpec::Section,
            Block::Note { lines, .. } => BlockSpec::Note { lines: lines.len() },
            Block::Filter { .. } => BlockSpec::Filter,
            Block::Row(row) => BlockSpec::Row {
                control: row.control.size(m),
                two_lines: !row.description.is_empty(),
                reason: row.invalid.is_some() || row.notice.is_some(),
            },
        })
        .collect();
    let geometry = layout::panel_geometry(m, panel_width, &specs);

    let footer_widths = [
        msg::settings::button::open_file(catalog),
        msg::settings::button::discard(catalog),
        msg::settings::button::save(catalog),
    ]
    .map(|label| {
        measurer.measure_width(&label, BODY_FONT, m.button_font_size) + m.button_padding_x * 2.0
    });

    let banner = extras
        .banner
        .map(|banner| banner_view(banner, catalog, m, panel_width, measurer));

    Content {
        key,
        title: key.group.label(catalog),
        blocks,
        banner,
        geometry,
        footer_widths,
    }
}

/// A faixa de `banner`: título e corpo cortados ao que sobra entre a barra e o
/// primeiro botão, e os botões medidos como os do diálogo.
fn banner_view(
    banner: &Banner,
    catalog: &Catalog,
    m: &Metrics,
    panel_width: f32,
    measurer: &mut TextMeasurer,
) -> BannerView {
    let (error, title, body, buttons) = match banner {
        Banner::Conflict => (
            false,
            msg::settings::banner::file_changed(catalog),
            None,
            vec![
                (BannerButton::Reload, msg::settings::banner::reload(catalog)),
                (
                    BannerButton::Keep,
                    msg::settings::banner::keep_mine(catalog),
                ),
            ],
        ),
        Banner::Invalid(error) => (
            true,
            msg::settings::banner::invalid_file(catalog),
            Some(crate::messages::config_error(catalog, error)),
            vec![(
                BannerButton::OpenFile,
                msg::settings::button::open_file(catalog),
            )],
        ),
    };
    let buttons: Vec<BannerButtonView> = buttons
        .into_iter()
        .map(|(button, label)| BannerButtonView {
            width: measurer.measure_width(&label, BODY_FONT, m.button_font_size)
                + m.button_padding_x * 2.0,
            button,
            label,
        })
        .collect();
    // O texto cabe entre a barra e o primeiro botão: a mesma conta de
    // `layout::banner_geometry`, sobre a largura que a faixa terá.
    let widths: Vec<f32> = buttons.iter().map(|button| button.width).collect();
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: (panel_width - m.panel_padding * 2.0).max(0.0),
        height: m.banner_height(body.is_some()),
    };
    let budget = layout::banner_geometry(m, rect, &widths, body.is_some()).text_width;
    let title = measurer.truncate(&title, BODY_FONT, m.name_size, budget).0;
    let body = body.map(|text| {
        measurer
            .truncate(&text, BODY_FONT, m.description_size, budget)
            .0
    });
    BannerView {
        error,
        title,
        body,
        buttons,
    }
}

fn option_row(
    option: &OptionDef,
    draft: &Draft,
    catalog: &Catalog,
    m: &Metrics,
    panel_width: f32,
    measurer: &mut TextMeasurer,
) -> RowView {
    let value = draft.value(option);
    let rows = match option.control {
        Control::StringList | Control::StringMap => draft.rows(option),
        _ => Vec::new(),
    };
    let control = control_view(
        option,
        &value,
        draft.raw(option),
        &rows,
        catalog,
        m,
        measurer,
    );
    let budget = layout::row_left_width(m, panel_width, control.size(m).0);

    let scope_text = match option.reload_scope() {
        ReloadScope::Live => None,
        ReloadScope::NewTab => Some(msg::settings::scope::new_tab(catalog)),
        ReloadScope::NextWindow => Some(msg::settings::scope::next_window(catalog)),
        ReloadScope::Restart => Some(msg::settings::scope::restart(catalog)),
    };
    // O escopo vem logo depois do nome e come do orçamento dele; o nome
    // cede, o escopo não (RF-16.13: "diz o escopo antes de o usuário mudar").
    let scope_width = scope_text.as_deref().map_or(0.0, |text| {
        measurer.measure_width(text, BODY_FONT, m.description_size) + m.row_gap
    });
    let (name, _) = measurer.truncate(
        &(option.label)(catalog),
        BODY_FONT,
        m.name_size,
        (budget - scope_width).max(0.0),
    );
    let scope = scope_text.map(|text| {
        let name_width = measurer.measure_width(&name, BODY_FONT, m.name_size);
        (text, name_width + m.row_gap)
    });

    let description_full = (option.description)(catalog);
    let (description, description_truncated) =
        measurer.truncate(&description_full, BODY_FONT, m.description_size, budget);

    // A razão de um valor recusado: a do rascunho (o que o usuário digitou)
    // ou, se o valor do arquivo é que está fora da faixa, a marca do
    // RF-16.18 -- só marca, nunca corrige.
    let reason = match draft.invalid(option) {
        Some(error) => Some(error.clone()),
        None if !draft.is_pending(option) => draft.file_issue(option),
        None => None,
    };
    let invalid = reason.map(|error| {
        let text = validation_text(catalog, &error);
        measurer
            .truncate(&text, BODY_FONT, m.description_size, budget)
            .0
    });

    RowView {
        option: Some(option.id),
        name,
        description,
        description_full,
        description_truncated,
        scope,
        value_text: value_text(option, &value, catalog),
        control,
        pending: draft.is_pending(option),
        invalid,
        can_reset: draft.can_reset(option),
        action: None,
        notice: None,
    }
}

/// A razão de uma recusa, composta do erro tipado pelo catálogo de textos
/// (ADR-0056 §2).
fn validation_text(catalog: &Catalog, error: &ValueError) -> String {
    use msg::settings::validation as v;
    match error {
        ValueError::NotANumber => v::not_a_number(catalog),
        ValueError::NotAnInteger => v::not_an_integer(catalog),
        ValueError::OutOfRange { min, max } => {
            v::out_of_range(catalog, plain_number(*min), plain_number(*max))
        }
        ValueError::NotAChoice | ValueError::WrongType => v::not_a_choice(catalog),
        ValueError::EmptyName => v::empty_name(catalog),
        ValueError::DuplicateName(name) => v::duplicate_name(catalog, name),
    }
}

/// Um limite de faixa como a tela o escreve: sem parte decimal quando não há.
fn plain_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// `raw` é o texto de um valor recusado: o campo continua mostrando o que o
/// usuário digitou, em vez do valor que está valendo. `rows` são as linhas de
/// uma lista, na ordem em que a tela as mostra (o rascunho as guarda).
fn control_view(
    option: &OptionDef,
    value: &EditValue,
    raw: Option<&str>,
    rows: &[(String, String)],
    catalog: &Catalog,
    m: &Metrics,
    measurer: &mut TextMeasurer,
) -> ControlView {
    match (option.control, value) {
        (Control::Toggle, EditValue::Bool(on)) => ControlView::Toggle { on: *on },
        (Control::Text | Control::Language, EditValue::String(text)) => {
            if option.control == Control::Language {
                ControlView::Choice { text: text.clone() }
            } else {
                field(
                    display_text(option, raw.unwrap_or(text)),
                    m.text_field_width,
                    false,
                    m,
                    measurer,
                )
            }
        }
        (Control::Number { float, .. }, number) => field(
            raw.map_or_else(|| number_text(number, float), str::to_owned),
            m.number_field_width,
            true,
            m,
            measurer,
        ),
        (Control::Choice(choices), EditValue::String(current)) => {
            let labels: Vec<String> = choices
                .iter()
                .map(|choice| choice_label(catalog, choice))
                .collect();
            if choices.len() <= 3 {
                let widths = labels
                    .iter()
                    .map(|label| {
                        measurer.measure_width(label, BODY_FONT, m.button_font_size)
                            + m.button_padding_x * 2.0
                    })
                    .collect();
                ControlView::Segmented {
                    selected: choices
                        .iter()
                        .position(|choice| *choice == current)
                        .unwrap_or(0),
                    labels,
                    widths,
                }
            } else {
                let text = choices
                    .iter()
                    .position(|choice| *choice == current)
                    .map(|index| labels[index].clone())
                    .unwrap_or_else(|| current.clone());
                ControlView::Choice { text }
            }
        }
        (Control::StringList | Control::StringMap, _) => {
            let invalid = Draft::invalid_rows(option, rows);
            ControlView::List {
                items: rows
                    .iter()
                    .zip(invalid)
                    .map(|((first, second), invalid)| ListItemView {
                        first: first.clone(),
                        second: second.clone(),
                        invalid,
                    })
                    .collect(),
                two_fields: option.control == Control::StringMap,
                add_label: msg::settings::button::add_item(catalog),
            }
        }
        (Control::GitPoll, EditValue::Integer(seconds)) => {
            let text = match raw {
                Some(raw) => raw.to_owned(),
                None if *seconds > 0 => seconds.to_string(),
                None => String::new(),
            };
            ControlView::GitPoll {
                on: *seconds > 0,
                text_width: measurer.measure_width(&text, BODY_FONT, m.field_font_size),
                seconds: text,
            }
        }
        (control, value) => unreachable!("controle {control:?} sobre valor {value:?}"),
    }
}

fn field(
    text: String,
    width: f32,
    right_aligned: bool,
    m: &Metrics,
    measurer: &mut TextMeasurer,
) -> ControlView {
    let text_width = measurer.measure_width(&text, BODY_FONT, m.field_font_size);
    ControlView::Field {
        text,
        width,
        right_aligned,
        text_width,
    }
}

/// A frase de um valor nomeado de enum (`block`, `hover`, `top`, ...). Valor
/// sem frase volta como veio: o arquivo é quem o escreveu.
pub(crate) fn choice_label(catalog: &Catalog, choice: &str) -> String {
    use msg::settings::choice as c;
    match choice {
        "block" => c::block(catalog),
        "beam" => c::beam(catalog),
        "underline" => c::underline(catalog),
        "always" => c::always(catalog),
        "hover" => c::hover(catalog),
        "never" => c::never(catalog),
        "top" => c::top(catalog),
        "bottom" => c::bottom(catalog),
        "stretch" => c::stretch(catalog),
        "tile" => c::tile(catalog),
        "center" => c::center(catalog),
        other => other.to_owned(),
    }
}

/// O valor como texto, para a árvore de acessibilidade.
fn value_text(option: &OptionDef, value: &EditValue, catalog: &Catalog) -> String {
    match (option.control, value) {
        (Control::Choice(_), EditValue::String(choice)) => choice_label(catalog, choice),
        (_, EditValue::Bool(on)) => on.to_string(),
        (Control::Number { float, .. }, number) => number_text(number, float),
        (_, EditValue::Integer(number)) => number.to_string(),
        (_, EditValue::Float(number)) => number.to_string(),
        (_, EditValue::String(text)) => text.clone(),
        (_, EditValue::StringList(items)) => items.join(", "),
        (_, EditValue::StringMap(map)) => map
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Um chip de `text` com a largura já medida: o texto mais o `padding: 3px 7px`
/// e a borda de 1px (ADR-0060 §3).
fn chip(text: String, tone: ChipTone, m: &Metrics, measurer: &mut TextMeasurer) -> ChipView {
    let width =
        measurer.measure_width(&text, CHIP_FONT, m.chip_font_size) + m.chip_padding_x * 2.0 + 2.0;
    ChipView { text, width, tone }
}

/// RF-16.28 a RF-16.30: o filtro no topo e, por domínio, um cabeçalho e uma
/// linha por ação -- o nome legível e os atalhos efetivos como chips ou
/// "Nenhum atalho". A linha em captura mostra os atalhos de quando a captura
/// começou, com o chip em captura no lugar do substituído; com um conflito
/// pendente, a razão no tom de Aviso e Substituir/Cancelar no lugar dos chips.
fn push_shortcut_blocks(
    blocks: &mut Vec<Block>,
    view: &ShortcutsView<'_>,
    catalog: &Catalog,
    m: &Metrics,
    panel_width: f32,
    measurer: &mut TextMeasurer,
) {
    blocks.push(Block::Filter {
        text: view.filter.to_owned(),
        placeholder: msg::settings::shortcut::filter_placeholder(catalog),
    });
    let pending = view.state.pending_actions();
    for (domain, actions) in view.state.rows(catalog, view.filter) {
        blocks.push(Block::Section(domain.title(catalog)));
        for action in actions {
            let capturing = view
                .capturing
                .filter(|capturing| capturing.action == action);
            let chords = match capturing {
                Some(capturing) => capturing.frozen.clone(),
                None => view.state.chords(action),
            };
            let name_full = action_label(catalog, action).unwrap_or_else(|| action.to_string());
            let mut notice_text = None;
            let control = match capturing.and_then(|capturing| capturing.conflict) {
                Some(conflict) => {
                    let other = action_label(catalog, conflict.other)
                        .unwrap_or_else(|| conflict.other.to_string());
                    notice_text = Some(msg::settings::shortcut::conflict(catalog, &other));
                    let labels = vec![
                        msg::settings::shortcut::replace(catalog),
                        msg::settings::dialog::cancel(catalog),
                    ];
                    let widths = labels
                        .iter()
                        .map(|label| {
                            measurer.measure_width(label, BODY_FONT, m.button_font_size)
                                + m.button_padding_x * 2.0
                        })
                        .collect();
                    // Nenhum dos dois é "o escolhido": são botões, não uma
                    // escolha.
                    ControlView::Segmented {
                        labels,
                        widths,
                        selected: usize::MAX,
                    }
                }
                None => {
                    let mut chips: Vec<ChipView> = chords
                        .iter()
                        .map(|chord| {
                            let replaced = capturing
                                .is_some_and(|capturing| capturing.replacing == Some(*chord));
                            if replaced {
                                chip(
                                    msg::settings::shortcut::press_keys(catalog),
                                    ChipTone::Capturing,
                                    m,
                                    measurer,
                                )
                            } else {
                                chip(chord.label(), ChipTone::Normal, m, measurer)
                            }
                        })
                        .collect();
                    if let Some(capturing) = capturing
                        && capturing.replacing.is_none()
                    {
                        chips.push(chip(
                            msg::settings::shortcut::press_keys(catalog),
                            ChipTone::Capturing,
                            m,
                            measurer,
                        ));
                    }
                    if chips.is_empty() {
                        chips.push(chip(
                            msg::settings::shortcut::none(catalog),
                            ChipTone::Muted,
                            m,
                            measurer,
                        ));
                    }
                    ControlView::Chips { chips }
                }
            };
            // O terminal deixa de receber `Ctrl+<letra>` sozinho: aceito, com
            // a advertência (RF-16.29).
            if notice_text.is_none()
                && capturing.is_none()
                && let Some(reserved) = view.state.reserved_chords(action).first()
            {
                notice_text = Some(msg::settings::shortcut::reserved(catalog, reserved.label()));
            }
            let budget = layout::row_left_width(m, panel_width, control.size(m).0);
            let (name, _) = measurer.truncate(&name_full, BODY_FONT, m.name_size, budget);
            let notice = notice_text.map(|text| {
                measurer
                    .truncate(&text, BODY_FONT, m.description_size, budget)
                    .0
            });
            let value_text = if chords.is_empty() {
                msg::settings::shortcut::none(catalog)
            } else {
                chords
                    .iter()
                    .map(Chord::label)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            blocks.push(Block::Row(Box::new(RowView {
                option: None,
                name,
                description: String::new(),
                description_full: String::new(),
                description_truncated: false,
                scope: None,
                control,
                value_text,
                pending: pending.contains(&action),
                invalid: None,
                can_reset: view.state.can_reset(action),
                action: Some(action),
                notice,
            })));
        }
    }
}

/// RF-16.24: uma linha por tema -- "sem tema", depois os de `[[themes]]` na
/// ordem do ciclo -- com a amostra de dez quadrados. O escolhido é o do
/// rascunho (o do arquivo, até o usuário mexer). O tema de sessão
/// (`theme.cycle`) **não** é escolha nem pendência (RF-16.25): se difere do
/// arquivo, uma linha abaixo da lista diz qual está em uso.
#[allow(clippy::too_many_arguments)]
fn push_theme_rows(
    blocks: &mut Vec<Block>,
    option: &OptionDef,
    draft: &Draft,
    session_theme: Option<&str>,
    catalog: &Catalog,
    m: &Metrics,
    panel_width: f32,
    measurer: &mut TextMeasurer,
) {
    let config = draft.file_config();
    let chosen = match draft.value(option) {
        EditValue::String(name) => name,
        _ => String::new(),
    };
    let pending = draft.is_pending(option);
    let mut names: Vec<(String, &str)> = vec![(msg::settings::theme::none(catalog), "")];
    names.extend(
        config
            .themes
            .iter()
            .map(|theme| (theme.name.clone(), theme.name.as_str())),
    );
    for (label, name) in names {
        let themed = porecatu_config::apply_theme(config, name);
        let palette = ResolvedTermPalette::from_config(&themed);
        let mut colors = [palette.background; SWATCH_COUNT];
        colors[1] = palette.foreground;
        for (index, slot) in colors.iter_mut().skip(2).enumerate() {
            *slot = palette.resolve(TermColor::Indexed(index as u8), true, false);
        }
        let selected = name == chosen;
        let control = ControlView::Themes {
            name: name.to_owned(),
            colors: Box::new(colors),
            selected,
        };
        let budget = layout::row_left_width(m, panel_width, control.size(m).0);
        let (name_text, _) = measurer.truncate(&label, BODY_FONT, m.name_size, budget);
        blocks.push(Block::Row(Box::new(RowView {
            option: Some(option.id),
            name: name_text,
            description: String::new(),
            description_full: String::new(),
            description_truncated: false,
            scope: None,
            value_text: label,
            control,
            // O ponto de pendente fica na linha escolhida: é ela que vai ser
            // gravada.
            pending: pending && selected,
            invalid: None,
            // Sem botão de restaurar por linha (ADR-0060 §3).
            can_reset: false,
            action: None,
            notice: None,
        })));
    }
    if let Some(session) = session_theme
        && session != config.terminal.theme
    {
        let shown = if session.is_empty() {
            msg::settings::theme::none(catalog)
        } else {
            session.to_owned()
        };
        let width = (panel_width - m.panel_padding * 2.0).max(0.0);
        blocks.push(Block::Note {
            lines: wrap(
                &msg::settings::theme::session_using(catalog, &shown),
                m.description_size,
                width,
                measurer,
            ),
            tone: NoteTone::Muted,
        });
    }
}

#[cfg(test)]
mod tests {
    use porecatu_config::Config;

    use super::*;
    use crate::messages::test_support;
    use crate::palette;

    fn metrics() -> Metrics {
        Metrics::from_config(&Config::default(), 52.0)
    }

    fn key(group: Group) -> ContentKey {
        ContentKey {
            group,
            panel_width_bits: 700.0_f32.to_bits(),
            generation: 0,
        }
    }

    fn content(group: Group, config: &Config) -> Content {
        build(
            key(group),
            &Draft::new(config),
            &ViewExtras::default(),
            &test_support::pt_br(),
            &metrics(),
            &mut TextMeasurer::new(),
        )
    }

    fn rows(content: &Content) -> Vec<&RowView> {
        content
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Row(row) => Some(&**row),
                Block::Section(_) | Block::Note { .. } | Block::Filter { .. } => None,
            })
            .collect()
    }

    #[test]
    fn the_general_group_has_its_three_sections_and_four_options() {
        let c = content(Group::General, &Config::default());
        assert_eq!(c.title, "Geral");
        let sections: Vec<&str> = c
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Section(label) => Some(label.as_str()),
                Block::Row(_) | Block::Note { .. } | Block::Filter { .. } => None,
            })
            .collect();
        assert_eq!(sections, ["IDIOMA", "INÍCIO", "CONFIRMAÇÕES"]);
        assert_eq!(rows(&c).len(), 4);
    }

    #[test]
    fn every_group_builds_with_a_row_per_option() {
        for group in Group::ALL {
            let c = content(group, &Config::default());
            let expected = if group == Group::Appearance {
                // o tema vira uma linha por tema (ver o teste próprio).
                options_in(group).count() - 1 + 1 + Config::default().themes.len()
            } else {
                options_in(group).count()
            };
            assert_eq!(rows(&c).len(), expected, "{group:?}");
            assert_eq!(c.geometry.blocks.len(), c.blocks.len(), "{group:?}");
        }
    }

    #[test]
    fn the_shortcuts_group_has_only_its_title() {
        let c = content(Group::Shortcuts, &Config::default());
        assert!(c.blocks.is_empty());
        assert_eq!(c.title, "Atalhos");
    }

    #[test]
    fn controls_show_the_value_in_force() {
        let mut config = Config::default();
        config.terminal.font.size = 18.5;
        config.terminal.cursor.blink = true;
        config.terminal.cursor.shape = porecatu_config::CursorShape::Beam;
        let c = content(Group::Terminal, &config);
        let by_id = |id: &str| {
            rows(&c)
                .into_iter()
                .find(|row| row.option == Some(id))
                .unwrap_or_else(|| panic!("{id}"))
                .clone()
        };
        let ControlView::Field {
            text,
            width,
            right_aligned,
            text_width,
        } = by_id("font_size").control
        else {
            panic!("o tamanho da fonte é um campo numérico")
        };
        assert_eq!((text.as_str(), width, right_aligned), ("18.5", 88.0, true));
        assert!(text_width > 0.0);
        assert_eq!(
            by_id("cursor_blink").control,
            ControlView::Toggle { on: true }
        );
        let ControlView::Segmented {
            labels,
            selected,
            widths,
        } = by_id("cursor_shape").control
        else {
            panic!("a forma do cursor é um segmentado")
        };
        assert_eq!(labels, ["Bloco", "Barra", "Sublinhado"]);
        assert_eq!(selected, 1);
        assert!(widths.iter().all(|width| *width > 24.0));
    }

    #[test]
    fn control_characters_in_a_field_are_shown_written_out() {
        // Os separadores de palavra padrão têm tabulação e quebra de linha.
        let c = content(Group::Terminal, &Config::default());
        let separators = rows(&c)
            .into_iter()
            .find(|row| row.option == Some("word_separators"))
            .unwrap();
        let ControlView::Field { text, .. } = &separators.control else {
            panic!()
        };
        assert!(!text.contains('\t') && !text.contains('\n'));
        assert!(text.contains("\\t") && text.contains("\\n"));
        // A árvore de acessibilidade segue com o valor cru.
        assert!(separators.value_text.contains('\t'));
    }

    #[test]
    fn class_c_options_say_their_scope_after_the_name() {
        let c = content(Group::Shell, &Config::default());
        let program = rows(&c)
            .into_iter()
            .find(|row| row.option == Some("shell_program"))
            .unwrap();
        let (text, x) = program.scope.as_ref().expect("shell vale em aba nova");
        assert_eq!(text, "vale em aba nova");
        assert!(*x > 0.0);
        // Uma opção ao vivo não diz escopo nenhum.
        let t = content(Group::Terminal, &Config::default());
        let size = rows(&t)
            .into_iter()
            .find(|row| row.option == Some("font_size"))
            .unwrap();
        assert!(size.scope.is_none());
    }

    #[test]
    fn a_long_description_is_cut_with_an_ellipsis_and_keeps_the_full_text() {
        // Painel estreito: o orçamento da descrição é pequeno.
        let narrow = ContentKey {
            panel_width_bits: 520.0_f32.to_bits(),
            ..key(Group::Terminal)
        };
        let c = build(
            narrow,
            &Draft::new(&Config::default()),
            &ViewExtras::default(),
            &test_support::pt_br(),
            &metrics(),
            &mut TextMeasurer::new(),
        );
        let all = rows(&c);
        let truncated: Vec<&RowView> = all
            .into_iter()
            .filter(|row| row.description_truncated)
            .collect();
        assert!(
            !truncated.is_empty(),
            "alguma descrição deveria ser cortada"
        );
        for row in truncated {
            assert!(row.description.ends_with('…'));
            assert!(row.description_full.len() > row.description.len());
        }
    }

    #[test]
    fn a_description_that_fits_is_untouched() {
        let c = content(Group::Terminal, &Config::default());
        for row in rows(&c) {
            if !row.description_truncated {
                assert_eq!(row.description, row.description_full);
            }
        }
    }

    #[test]
    fn the_theme_option_becomes_one_row_per_theme_with_ten_swatches() {
        let config = Config::default();
        let c = content(Group::Appearance, &config);
        let themes: Vec<&RowView> = rows(&c)
            .into_iter()
            .filter(|row| matches!(row.control, ControlView::Themes { .. }))
            .collect();
        assert_eq!(themes.len(), 1 + config.themes.len());
        assert_eq!(themes[0].name, "Sem tema");
        // nenhum tema escolhido: "sem tema" é o escolhido.
        let ControlView::Themes { selected, .. } = themes[0].control else {
            panic!()
        };
        assert!(selected);
        let ControlView::Themes { selected, .. } = themes[1].control else {
            panic!()
        };
        assert!(!selected);
        let size = themes[0].control.size(&metrics());
        assert_eq!(size, (10.0 * 12.0 + 9.0 * 2.0, 12.0));
    }

    #[test]
    fn the_chosen_theme_is_the_one_in_the_file() {
        let mut config = Config::default();
        let name = config
            .themes
            .first()
            .expect("há temas embutidos")
            .name
            .clone();
        config.terminal.theme = name.clone();
        let c = content(Group::Appearance, &config);
        let chosen: Vec<&str> = rows(&c)
            .into_iter()
            .filter(|row| matches!(row.control, ControlView::Themes { selected: true, .. }))
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(chosen, [name.as_str()]);
    }

    #[test]
    fn lists_show_their_items_and_an_add_entry() {
        let mut config = Config::default();
        config.shell.args = vec!["-l".to_owned(), "-i".to_owned()];
        config
            .shell
            .env
            .insert("EDITOR".to_owned(), "vim".to_owned());
        let c = content(Group::Shell, &config);
        let by_id = |id: &str| {
            rows(&c)
                .into_iter()
                .find(|row| row.option == Some(id))
                .unwrap()
                .control
                .clone()
        };
        let item = |first: &str, second: &str| ListItemView {
            first: first.to_owned(),
            second: second.to_owned(),
            invalid: false,
        };
        assert_eq!(
            by_id("shell_args"),
            ControlView::List {
                items: vec![item("-l", ""), item("-i", "")],
                two_fields: false,
                add_label: "Adicionar".to_owned()
            }
        );
        let ControlView::List {
            items, two_fields, ..
        } = by_id("shell_env")
        else {
            panic!()
        };
        assert!(two_fields, "a lista de variáveis tem nome e valor");
        assert_eq!(items, [item("EDITOR", "vim")]);
    }

    #[test]
    fn the_git_poll_shows_the_toggle_and_the_seconds() {
        let c = content(Group::Git, &Config::default());
        let row = rows(&c)[0].clone();
        let ControlView::GitPoll { on, seconds, .. } = row.control else {
            panic!()
        };
        assert!(on);
        assert_eq!(seconds, "300");
        let mut off = Config::default();
        off.git.remote_poll_interval_secs = 0;
        let c = content(Group::Git, &off);
        let ControlView::GitPoll { on, seconds, .. } = rows(&c)[0].control.clone() else {
            panic!()
        };
        assert!(!on);
        assert!(seconds.is_empty());
    }

    #[test]
    fn language_is_a_choice_with_the_locale_code() {
        let c = content(Group::General, &Config::default());
        let row = rows(&c)[0].clone();
        assert_eq!(
            row.control,
            ControlView::Choice {
                text: "en_US".to_owned()
            }
        );
    }

    #[test]
    fn the_footer_buttons_are_measured_with_their_padding() {
        let c = content(Group::General, &Config::default());
        // "Abrir arquivo no editor" é o mais largo; todos com padding de 12.
        assert!(c.footer_widths[0] > c.footer_widths[1]);
        assert!(c.footer_widths.iter().all(|width| *width > 24.0));
    }

    #[test]
    fn geometry_has_one_entry_per_block() {
        for group in Group::ALL {
            let c = content(group, &Config::default());
            assert_eq!(c.geometry.blocks.len(), c.blocks.len());
            assert!(c.geometry.content_height > 0.0);
        }
    }

    #[test]
    fn row_indices_point_at_rows_only() {
        let c = content(Group::Terminal, &Config::default());
        for index in c.row_indices() {
            assert!(c.row(index).is_some());
        }
        assert!(c.row(0).is_none(), "o primeiro bloco é um rótulo de seção");
    }

    // ---- rascunho: pendência, restaurar e recusa

    fn draft_content(group: Group, draft: &Draft) -> Content {
        build(
            key(group),
            draft,
            &ViewExtras::default(),
            &test_support::pt_br(),
            &metrics(),
            &mut TextMeasurer::new(),
        )
    }

    fn row_of<'a>(content: &'a Content, id: &str) -> &'a RowView {
        rows(content)
            .into_iter()
            .find(|row| row.option == Some(id))
            .unwrap_or_else(|| panic!("{id}"))
    }

    #[test]
    fn a_row_with_nothing_changed_has_no_dot_no_reason_and_nothing_to_restore() {
        let c = content(Group::Terminal, &Config::default());
        for row in rows(&c) {
            assert!(!row.pending && row.invalid.is_none() && !row.can_reset);
        }
    }

    #[test]
    fn a_pending_option_shows_the_new_value_the_dot_and_the_restore_button() {
        let mut draft = Draft::new(&Config::default());
        draft
            .set(
                super::super::catalog::option("font_size").unwrap(),
                EditValue::Float(18.0),
            )
            .unwrap();
        let c = draft_content(Group::Terminal, &draft);
        let size = row_of(&c, "font_size");
        let ControlView::Field { text, .. } = &size.control else {
            panic!()
        };
        assert_eq!(text, "18");
        assert!(size.pending && size.can_reset);
        // Só ela: as outras seguem limpas.
        assert!(!row_of(&c, "line_height").pending);
    }

    #[test]
    fn an_option_that_differs_from_the_default_in_the_file_can_be_restored_without_a_dot() {
        let file = porecatu_config::parse("[terminal.font]\nsize = 18.0\n")
            .unwrap()
            .0;
        let c = draft_content(Group::Terminal, &Draft::new(&file));
        let size = row_of(&c, "font_size");
        assert!(size.can_reset);
        assert!(!size.pending, "o arquivo já diz isso: não é pendência");
    }

    #[test]
    fn a_refused_value_keeps_the_typed_text_and_says_why() {
        let mut draft = Draft::new(&Config::default());
        let option = super::super::catalog::option("font_size").unwrap();
        draft.set_raw(option, "900").unwrap_err();
        let c = draft_content(Group::Terminal, &draft);
        let size = row_of(&c, "font_size");
        let ControlView::Field { text, .. } = &size.control else {
            panic!()
        };
        assert_eq!(text, "900", "o campo mostra o que foi digitado");
        let reason = size.invalid.as_deref().expect("a razão");
        assert!(reason.starts_with("Valor fora da faixa"), "{reason}");
        // E a razão entra na geometria: a linha ganha uma linha de texto.
        let clean = content(Group::Terminal, &Config::default());
        assert!(c.geometry.content_height > clean.geometry.content_height);
    }

    #[test]
    fn a_malformed_number_says_it_is_not_a_number() {
        let mut draft = Draft::new(&Config::default());
        let option = super::super::catalog::option("font_size").unwrap();
        draft.set_raw(option, "abc").unwrap_err();
        let c = draft_content(Group::Terminal, &draft);
        assert_eq!(
            row_of(&c, "font_size").invalid.as_deref(),
            Some("Número inválido.")
        );
    }

    #[test]
    fn a_value_in_the_file_outside_the_range_is_marked_but_is_not_pending() {
        let file = porecatu_config::parse("[terminal.font]\nsize = 900.0\n")
            .unwrap()
            .0;
        let c = draft_content(Group::Terminal, &Draft::new(&file));
        let size = row_of(&c, "font_size");
        assert!(size.invalid.is_some(), "RF-16.18: marcado");
        assert!(!size.pending, "e nunca corrigido sozinho");
        let ControlView::Field { text, .. } = &size.control else {
            panic!()
        };
        assert_eq!(text, "900");
    }

    #[test]
    fn a_windows_path_in_a_plain_field_is_shown_with_its_single_backslashes() {
        let file = porecatu_config::parse("[shell]\nprogram = 'C:\\Windows\\cmd.exe'\n")
            .unwrap()
            .0;
        let c = draft_content(Group::Shell, &Draft::new(&file));
        let ControlView::Field { text, .. } = &row_of(&c, "shell_program").control else {
            panic!()
        };
        assert_eq!(text, "C:\\Windows\\cmd.exe");
    }

    // ---- que parte do controle recebe o clique

    #[test]
    fn a_toggle_a_field_and_a_choice_are_clicked_as_a_whole() {
        let m = metrics();
        let rect = Rect {
            x: 100.0,
            y: 50.0,
            width: 88.0,
            height: 30.0,
        };
        let inside = (120.0, 60.0);
        for control in [
            ControlView::Toggle { on: false },
            ControlView::Choice {
                text: String::new(),
            },
        ] {
            assert_eq!(control.part_at(rect, &m, inside), Some(ControlPart::Whole));
            assert_eq!(control.part_at(rect, &m, (10.0, 60.0)), None);
        }
    }

    #[test]
    fn a_segmented_control_reports_the_segment_under_the_point() {
        let m = metrics();
        let control = ControlView::Segmented {
            labels: vec!["a".into(), "b".into(), "c".into()],
            widths: vec![50.0, 60.0, 70.0],
            selected: 0,
        };
        let rect = Rect {
            x: 100.0,
            y: 50.0,
            width: 180.0,
            height: 30.0,
        };
        assert_eq!(
            control.part_at(rect, &m, (101.0, 60.0)),
            Some(ControlPart::Segment(0))
        );
        assert_eq!(
            control.part_at(rect, &m, (150.0, 60.0)),
            Some(ControlPart::Segment(1))
        );
        assert_eq!(
            control.part_at(rect, &m, (279.0, 60.0)),
            Some(ControlPart::Segment(2))
        );
    }

    #[test]
    fn the_git_control_tells_the_switch_from_the_number() {
        let m = metrics();
        let control = ControlView::GitPoll {
            on: true,
            seconds: "300".into(),
            text_width: 20.0,
        };
        let (width, height) = control.size(&m);
        let rect = Rect {
            x: 100.0,
            y: 50.0,
            width,
            height,
        };
        assert_eq!(
            control.part_at(rect, &m, (105.0, 60.0)),
            Some(ControlPart::GitToggle)
        );
        assert_eq!(
            control.part_at(rect, &m, (rect.x + rect.width - 5.0, 60.0)),
            Some(ControlPart::GitNumber)
        );
    }

    fn list_of(count: usize, two_fields: bool) -> (ControlView, Rect) {
        let m = metrics();
        let item = ListItemView {
            first: "a".to_owned(),
            second: "b".to_owned(),
            invalid: false,
        };
        let list = ControlView::List {
            items: vec![item; count],
            two_fields,
            add_label: String::new(),
        };
        let (width, height) = list.size(&m);
        (
            list,
            Rect {
                x: 100.0,
                y: 50.0,
                width,
                height,
            },
        )
    }

    #[test]
    fn a_list_click_finds_the_field_the_remove_button_and_the_add_item() {
        let m = metrics();
        let (list, rect) = list_of(2, false);
        let at = |x: f32, y: f32| list.part_at(rect, &m, (x, y));
        // Primeiro item: campo e `X`.
        assert_eq!(
            at(110.0, 50.0 + m.field_height / 2.0),
            Some(ControlPart::ListField {
                item: 0,
                second: false
            })
        );
        let remove_x = rect.x + m.text_field_width + m.list_gap + m.restore_width / 2.0;
        assert_eq!(
            at(remove_x, 50.0 + m.field_height / 2.0),
            Some(ControlPart::ListRemove(0))
        );
        // Segundo item.
        let second_y = 50.0 + (m.field_height + m.list_gap) + m.field_height / 2.0;
        assert_eq!(
            at(110.0, second_y),
            Some(ControlPart::ListField {
                item: 1,
                second: false
            })
        );
        assert_eq!(at(remove_x, second_y), Some(ControlPart::ListRemove(1)));
        // O item "Adicionar" logo abaixo.
        let add_y = 50.0 + 2.0 * (m.field_height + m.list_gap) + m.sidebar_item_height / 2.0;
        assert_eq!(at(110.0, add_y), Some(ControlPart::ListAdd));
        // O vão entre o campo e o `X` não é nada.
        assert_eq!(
            at(
                rect.x + m.text_field_width + m.list_gap / 2.0,
                50.0 + m.field_height / 2.0
            ),
            None
        );
    }

    #[test]
    fn an_env_list_splits_the_field_into_name_and_value() {
        let m = metrics();
        let (list, rect) = list_of(1, true);
        let y = 50.0 + m.field_height / 2.0;
        assert_eq!(
            list.part_at(rect, &m, (rect.x + 4.0, y)),
            Some(ControlPart::ListField {
                item: 0,
                second: false
            })
        );
        assert_eq!(
            list.part_at(rect, &m, (rect.x + m.text_field_width - 4.0, y)),
            Some(ControlPart::ListField {
                item: 0,
                second: true
            })
        );
    }

    #[test]
    fn an_empty_list_still_has_the_add_item() {
        let m = metrics();
        let (list, rect) = list_of(0, false);
        assert_eq!(
            list.part_at(rect, &m, (rect.x + 10.0, rect.y + 5.0)),
            Some(ControlPart::ListAdd)
        );
    }

    #[test]
    fn a_theme_row_is_the_target_as_a_whole() {
        let m = metrics();
        let themes = ControlView::Themes {
            name: "nord".to_owned(),
            colors: Box::new([palette::TRANSPARENT; SWATCH_COUNT]),
            selected: false,
        };
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 12.0,
        };
        assert_eq!(themes.part_at(rect, &m, (10.0, 5.0)), None);
    }

    // ---- listas, temas e notas

    #[test]
    fn a_list_in_the_draft_shows_the_order_the_user_built_and_marks_refused_rows() {
        let mut draft = Draft::new(&Config::default());
        let env = super::super::catalog::option("shell_env").unwrap();
        let row = |name: &str, value: &str| (name.to_owned(), value.to_owned());
        let _ = draft.set_rows(env, &[row("B", "2"), row("", ""), row("A", "1")]);
        let c = draft_content(Group::Shell, &draft);
        let r = row_of(&c, "shell_env");
        let ControlView::List { items, .. } = &r.control else {
            panic!()
        };
        let names: Vec<&str> = items.iter().map(|item| item.first.as_str()).collect();
        assert_eq!(names, ["B", "", "A"]);
        let flags: Vec<bool> = items.iter().map(|item| item.invalid).collect();
        assert_eq!(flags, [false, true, false]);
        assert!(r.invalid.is_some(), "a razão aparece na linha");
        assert!(r.pending);
    }

    #[test]
    fn the_chosen_theme_follows_the_draft_and_carries_the_pending_dot() {
        let mut config = Config::default();
        let first = config.themes[0].name.clone();
        let second = config.themes[1].name.clone();
        config.terminal.theme = first.clone();
        let mut draft = Draft::new(&config);
        let theme = super::super::catalog::option("theme").unwrap();
        let c = draft_content(Group::Appearance, &draft);
        let chosen = |c: &Content| -> Vec<(String, bool)> {
            rows(c)
                .into_iter()
                .filter_map(|row| match &row.control {
                    ControlView::Themes { name, selected, .. } if *selected => {
                        Some((name.clone(), row.pending))
                    }
                    _ => None,
                })
                .collect()
        };
        assert_eq!(chosen(&c), [(first.clone(), false)]);
        draft.set(theme, EditValue::String(second.clone())).unwrap();
        let c = draft_content(Group::Appearance, &draft);
        assert_eq!(chosen(&c), [(second, true)], "o ponto fica na escolhida");
        // Uma linha de tema não tem botão de restaurar.
        assert!(
            rows(&c).iter().all(|row| {
                !matches!(row.control, ControlView::Themes { .. }) || !row.can_reset
            })
        );
    }

    #[test]
    fn the_session_theme_is_a_note_under_the_list_only_when_it_differs_from_the_file() {
        let config = Config::default();
        let theme_notes = |session: Option<&str>| -> Vec<(Vec<String>, NoteTone)> {
            let c = build(
                key(Group::Appearance),
                &Draft::new(&config),
                &ViewExtras {
                    session_theme: session,
                    ..ViewExtras::default()
                },
                &test_support::pt_br(),
                &metrics(),
                &mut TextMeasurer::new(),
            );
            c.blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Note { lines, tone } => Some((lines.clone(), *tone)),
                    _ => None,
                })
                .collect()
        };
        assert!(theme_notes(None).is_empty());
        // Igual ao do arquivo: nada a dizer.
        assert!(theme_notes(Some("")).is_empty());
        let notes = theme_notes(Some("nord"));
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].1, NoteTone::Muted);
        assert_eq!(notes[0].0.join(" "), "Esta sessão está usando o tema nord.");
        // "Sem tema" na sessão, com um tema no arquivo, também é dito.
        let mut named = Config::default();
        named.terminal.theme = named.themes[0].name.clone();
        let c = build(
            key(Group::Appearance),
            &Draft::new(&named),
            &ViewExtras {
                session_theme: Some(""),
                ..ViewExtras::default()
            },
            &test_support::pt_br(),
            &metrics(),
            &mut TextMeasurer::new(),
        );
        let text: String = c
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Note { lines, .. } => Some(lines.join(" ")),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Esta sessão está usando o tema Sem tema.");
    }

    #[test]
    fn the_session_theme_is_never_a_pending_change() {
        let config = Config::default();
        let draft = Draft::new(&config);
        let c = build(
            key(Group::Appearance),
            &draft,
            &ViewExtras {
                session_theme: Some("nord"),
                ..ViewExtras::default()
            },
            &test_support::pt_br(),
            &metrics(),
            &mut TextMeasurer::new(),
        );
        assert!(rows(&c).iter().all(|row| !row.pending));
        assert!(!draft.is_dirty());
    }

    #[test]
    fn the_trusted_paths_warning_sits_above_the_list_in_the_project_group() {
        let c = content(Group::Project, &Config::default());
        let note = c
            .blocks
            .iter()
            .position(|block| matches!(block, Block::Note { .. }))
            .expect("o aviso do RF-16.27");
        let list = c
            .blocks
            .iter()
            .position(|block| match block {
                Block::Row(row) => row.option == Some("trusted_paths"),
                _ => false,
            })
            .unwrap();
        assert!(note < list, "acima da lista");
        let Block::Note { lines, tone } = &c.blocks[note] else {
            unreachable!()
        };
        assert_eq!(*tone, NoteTone::Warning);
        assert!(
            lines.len() > 1,
            "o aviso é longo e quebra em mais de uma linha"
        );
        let joined = lines.join(" ");
        assert!(joined.contains(".porecatu"));
    }

    // ---- imagem de fundo (RF-17.20, RF-17.21)

    fn image_group(path: &str, config_path: Option<&std::path::Path>) -> Content {
        let mut draft = Draft::new(&Config::default());
        draft
            .set(
                crate::settings::catalog::option("background_image").unwrap(),
                EditValue::String(path.to_owned()),
            )
            .unwrap();
        build(
            key(Group::Terminal),
            &draft,
            &ViewExtras {
                config_path,
                ..ViewExtras::default()
            },
            &test_support::pt_br(),
            &metrics(),
            &mut TextMeasurer::new(),
        )
    }

    fn notes(content: &Content) -> Vec<(String, NoteTone)> {
        content
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Note { lines, tone } => Some((lines.join(" "), *tone)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_three_image_rows_come_after_the_background_opacity_row() {
        let c = content(Group::Terminal, &Config::default());
        let ids: Vec<_> = rows(&c).iter().filter_map(|row| row.option).collect();
        let at = ids
            .iter()
            .position(|id| *id == "background_opacity")
            .unwrap();
        assert_eq!(
            &ids[at..at + 4],
            [
                "background_opacity",
                "background_image",
                "background_image_mode",
                "background_image_opacity"
            ]
        );
        let mode = rows(&c)
            .into_iter()
            .find(|row| row.option == Some("background_image_mode"))
            .unwrap();
        // Três valores: botões colados, com os rótulos do catálogo.
        let ControlView::Segmented {
            labels, selected, ..
        } = &mode.control
        else {
            panic!("modo deveria ser segmentado: {:?}", mode.control);
        };
        assert_eq!(labels, &["Esticar", "Ladrilho", "Centralizar"]);
        assert_eq!(*selected, 0);
    }

    #[test]
    fn a_missing_file_shows_a_warning_note_right_under_the_path_field() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("porecatu.toml");
        let c = image_group("nao-existe.png", Some(&config));
        let path_row = c
            .blocks
            .iter()
            .position(
                |block| matches!(block, Block::Row(row) if row.option == Some("background_image")),
            )
            .unwrap();
        let Block::Note { lines, tone } = &c.blocks[path_row + 1] else {
            panic!(
                "a nota vem logo abaixo do campo: {:?}",
                c.blocks[path_row + 1]
            );
        };
        // No estilo e no lugar do aviso de `trusted_paths`, não na razão de
        // recusa: tom de Aviso, e a linha do campo não ganha `invalid`.
        assert_eq!(*tone, NoteTone::Warning);
        let text = lines.join(" ");
        assert!(text.contains("não foi encontrado"), "{text}");
        // O caminho resolvido contra a pasta do config, não o texto cru.
        let resolved = dir.path().join("nao-existe.png");
        assert!(text.contains(&resolved.display().to_string()), "{text}");
        let Block::Row(row) = &c.blocks[path_row] else {
            unreachable!()
        };
        assert!(row.invalid.is_none(), "a nota não é uma recusa");
    }

    #[test]
    fn the_note_never_blocks_saving() {
        let mut draft = Draft::new(&Config::default());
        let option = crate::settings::catalog::option("background_image").unwrap();
        draft
            .set(
                option,
                EditValue::String("/nao/existe/fundo.png".to_owned()),
            )
            .unwrap();
        // Pendência válida (vira edição), sem recusa.
        assert!(!draft.has_invalid());
        assert_eq!(draft.edits().len(), 1);
    }

    #[test]
    fn an_existing_file_or_an_empty_path_shows_no_note() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("porecatu.toml");
        std::fs::write(dir.path().join("fundo.png"), b"x").unwrap();
        assert!(notes(&image_group("fundo.png", Some(&config))).is_empty());
        assert!(notes(&image_group("", Some(&config))).is_empty());
        assert!(notes(&image_group("   ", Some(&config))).is_empty());
    }

    #[test]
    fn the_note_follows_the_draft_on_every_edit() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("porecatu.toml");
        std::fs::write(dir.path().join("existe.png"), b"x").unwrap();
        assert_eq!(notes(&image_group("falta.png", Some(&config))).len(), 1);
        assert!(notes(&image_group("existe.png", Some(&config))).is_empty());
        assert_eq!(notes(&image_group("falta.png", Some(&config))).len(), 1);
    }

    #[test]
    fn the_note_is_composed_by_the_catalog_in_every_language() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("porecatu.toml");
        for locale in ["en_US", "pt_BR", "es_ES", "fr_FR", "de_DE"] {
            let mut draft = Draft::new(&Config::default());
            draft
                .set(
                    crate::settings::catalog::option("background_image").unwrap(),
                    EditValue::String("x.png".to_owned()),
                )
                .unwrap();
            let c = build(
                key(Group::Terminal),
                &draft,
                &ViewExtras {
                    config_path: Some(&config),
                    ..ViewExtras::default()
                },
                &test_support::catalog(locale),
                &metrics(),
                &mut TextMeasurer::new(),
            );
            let text = notes(&c).into_iter().next().expect(locale).0;
            assert!(!text.contains("settings.option"), "{locale}: {text}");
            assert!(text.contains("x.png"), "{locale}: {text}");
        }
    }

    #[test]
    fn wrapping_breaks_between_words_and_never_loses_one() {
        let mut measurer = TextMeasurer::new();
        let text = "um dois tres quatro cinco seis sete oito nove dez";
        let lines = wrap(text, 11.0, 90.0, &mut measurer);
        assert!(lines.len() > 1);
        assert_eq!(lines.join(" "), text);
        for line in &lines {
            let words = line.split(' ').count();
            assert!(
                words == 1 || measurer.measure_width(line, BODY_FONT, 11.0) <= 90.0,
                "{line}"
            );
        }
        // Uma palavra mais larga que a linha fica inteira, sozinha.
        let lines = wrap("pneumoultramicroscopico fim", 11.0, 20.0, &mut measurer);
        assert_eq!(lines[0], "pneumoultramicroscopico");
        // Texto vazio não gera linha.
        assert!(wrap("", 11.0, 90.0, &mut measurer).is_empty());
    }

    // ---- grupo Atalhos

    use crate::keymap::Platform;
    use crate::settings::shortcuts::{Capturing, Conflict};

    fn shortcut_content(state: &Shortcuts, filter: &str, capturing: Option<&Capturing>) -> Content {
        build(
            key(Group::Shortcuts),
            &Draft::new(&Config::default()),
            &ViewExtras {
                shortcuts: Some(&ShortcutsView {
                    state,
                    filter,
                    capturing,
                }),
                ..ViewExtras::default()
            },
            &test_support::pt_br(),
            &metrics(),
            &mut TextMeasurer::new(),
        )
    }

    fn windows() -> Shortcuts {
        Shortcuts::new(&Config::default(), Platform::Windows)
    }

    fn row_for(content: &Content, action: Action) -> &RowView {
        rows(content)
            .into_iter()
            .find(|row| row.action == Some(action))
            .unwrap_or_else(|| panic!("{action}"))
    }

    fn chip_texts(row: &RowView) -> Vec<(String, ChipTone)> {
        let ControlView::Chips { chips } = &row.control else {
            panic!("{:?}", row.control)
        };
        chips.iter().map(|c| (c.text.clone(), c.tone)).collect()
    }

    #[test]
    fn the_shortcuts_group_starts_with_the_filter_then_a_header_per_domain() {
        let c = shortcut_content(&windows(), "", None);
        assert!(matches!(&c.blocks[0], Block::Filter { text, .. } if text.is_empty()));
        let Block::Filter { placeholder, .. } = &c.blocks[0] else {
            unreachable!()
        };
        assert_eq!(placeholder, "Filtrar por nome ou tecla…");
        let headers: Vec<&str> = c
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Section(label) => Some(label.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(headers.len(), crate::settings::actions::Domain::ALL.len());
        assert_eq!(headers[0], "Abas");
        // O filtro e as linhas são paradas do `Tab`.
        let stops = c.row_indices();
        assert_eq!(stops[0], 0);
        assert_eq!(stops.len(), 1 + rows(&c).len());
    }

    #[test]
    fn a_row_shows_the_name_and_the_effective_chips_or_no_shortcut() {
        let c = shortcut_content(&windows(), "", None);
        let tab_new = row_for(&c, Action::TabNew);
        assert_eq!(tab_new.name, "Nova aba");
        assert_eq!(
            chip_texts(tab_new),
            [("Ctrl+Shift+T".to_owned(), ChipTone::Normal)]
        );
        let none = row_for(&c, Action::GroupNewTab);
        assert_eq!(
            chip_texts(none),
            [("Nenhum atalho".to_owned(), ChipTone::Muted)]
        );
        assert!(!tab_new.pending && !tab_new.can_reset);
        assert!(tab_new.notice.is_none() && tab_new.invalid.is_none());
    }

    #[test]
    fn the_filter_leaves_only_the_matching_rows_and_their_headers() {
        let c = shortcut_content(&windows(), "ir para a aba 3", None);
        let actions: Vec<Option<Action>> = rows(&c).iter().map(|r| r.action).collect();
        assert_eq!(actions, [Some(Action::TabGoto(3))]);
        let headers = c
            .blocks
            .iter()
            .filter(|block| matches!(block, Block::Section(_)))
            .count();
        assert_eq!(headers, 1);
        let Block::Filter { text, .. } = &c.blocks[0] else {
            panic!()
        };
        assert_eq!(text, "ir para a aba 3");
    }

    #[test]
    fn a_changed_row_is_pending_and_can_be_restored() {
        let mut state = windows();
        let chord = |t: &str| Chord::parse(t).unwrap();
        state.bind(
            Action::TabNew,
            Some(chord("ctrl+shift+t")),
            chord("ctrl+shift+j"),
        );
        let c = shortcut_content(&state, "", None);
        let row = row_for(&c, Action::TabNew);
        assert!(row.pending && row.can_reset);
        assert_eq!(
            chip_texts(row),
            [("Ctrl+Shift+J".to_owned(), ChipTone::Normal)]
        );
    }

    #[test]
    fn the_capturing_row_shows_the_capture_chip_in_place_of_the_replaced_one() {
        let state = windows();
        let chord = Chord::parse("ctrl+shift+t").unwrap();
        let capturing = Capturing {
            action: Action::TabNew,
            replacing: Some(chord),
            frozen: vec![chord],
            conflict: None,
        };
        let c = shortcut_content(&state, "", Some(&capturing));
        assert_eq!(
            chip_texts(row_for(&c, Action::TabNew)),
            [(
                "Pressione a combinação de teclas…".to_owned(),
                ChipTone::Capturing
            )]
        );
        // As outras linhas seguem como estavam.
        assert_eq!(
            chip_texts(row_for(&c, Action::TabClose))[0].1,
            ChipTone::Normal
        );
    }

    #[test]
    fn adding_a_shortcut_to_an_action_without_one_replaces_the_no_shortcut_chip() {
        let state = windows();
        let capturing = Capturing {
            action: Action::GroupNewTab,
            replacing: None,
            frozen: vec![],
            conflict: None,
        };
        let c = shortcut_content(&state, "", Some(&capturing));
        assert_eq!(
            chip_texts(row_for(&c, Action::GroupNewTab)),
            [(
                "Pressione a combinação de teclas…".to_owned(),
                ChipTone::Capturing
            )]
        );
        // Com um atalho já, o chip em captura vem depois dele.
        let chord = Chord::parse("ctrl+shift+t").unwrap();
        let capturing = Capturing {
            action: Action::TabNew,
            replacing: None,
            frozen: vec![chord],
            conflict: None,
        };
        let c = shortcut_content(&state, "", Some(&capturing));
        let chips = chip_texts(row_for(&c, Action::TabNew));
        assert_eq!(chips.len(), 2);
        assert_eq!(chips[1].1, ChipTone::Capturing);
    }

    #[test]
    fn a_conflict_names_the_other_action_and_offers_replace_and_cancel() {
        let state = windows();
        let chord = Chord::parse("ctrl+shift+r").unwrap();
        let capturing = Capturing {
            action: Action::SearchOpen,
            replacing: Some(Chord::parse("ctrl+shift+f").unwrap()),
            frozen: vec![Chord::parse("ctrl+shift+f").unwrap()],
            conflict: Some(Conflict {
                chord,
                other: Action::TabRename,
            }),
        };
        let c = shortcut_content(&state, "", Some(&capturing));
        let row = row_for(&c, Action::SearchOpen);
        assert_eq!(row.notice.as_deref(), Some("Já em uso por Renomear aba."));
        let ControlView::Segmented {
            labels, selected, ..
        } = &row.control
        else {
            panic!("{:?}", row.control)
        };
        assert_eq!(labels, &["Substituir", "Cancelar"]);
        assert_eq!(*selected, usize::MAX, "nenhum dos dois é o escolhido");
    }

    #[test]
    fn a_reserved_key_adds_the_terminal_notice_and_a_clean_row_has_none() {
        let mut state = windows();
        state.bind(
            Action::SearchOpen,
            Some(Chord::parse("ctrl+shift+f").unwrap()),
            Chord::parse("ctrl+r").unwrap(),
        );
        let c = shortcut_content(&state, "", None);
        assert_eq!(
            row_for(&c, Action::SearchOpen).notice.as_deref(),
            Some("O terminal deixa de receber Ctrl+R.")
        );
        assert!(row_for(&c, Action::TabNew).notice.is_none());
    }

    #[test]
    fn a_reload_during_the_capture_does_not_change_the_capturing_row() {
        let mut state = windows();
        let chord = Chord::parse("ctrl+shift+t").unwrap();
        let capturing = Capturing {
            action: Action::TabNew,
            replacing: Some(chord),
            frozen: vec![chord],
            conflict: None,
        };
        // O arquivo passa a dar outra tecla a `tab.new`.
        let reloaded = porecatu_config::parse(
            "[keybindings.windows]\n\"ctrl+shift+t\" = \"none\"\n\"ctrl+shift+j\" = \"tab.new\"\n",
        )
        .unwrap()
        .0;
        state.rebase(&reloaded);
        let c = shortcut_content(&state, "", Some(&capturing));
        // A linha em captura ainda tem o chip em captura no lugar de antes...
        assert_eq!(
            chip_texts(row_for(&c, Action::TabNew)),
            [(
                "Pressione a combinação de teclas…".to_owned(),
                ChipTone::Capturing
            )]
        );
        // ...e a de fora da captura já mostra o arquivo novo.
        let c = shortcut_content(&state, "", None);
        assert_eq!(
            chip_texts(row_for(&c, Action::TabNew)),
            [("Ctrl+Shift+J".to_owned(), ChipTone::Normal)]
        );
    }

    #[test]
    fn a_chip_click_finds_the_chip_under_the_point() {
        let m = metrics();
        let chips = ControlView::Chips {
            chips: vec![
                ChipView {
                    text: "Ctrl+A".to_owned(),
                    width: 50.0,
                    tone: ChipTone::Normal,
                },
                ChipView {
                    text: "Ctrl+B".to_owned(),
                    width: 50.0,
                    tone: ChipTone::Normal,
                },
            ],
        };
        let (width, height) = chips.size(&m);
        assert_eq!(width, 50.0 + m.list_gap + 50.0);
        let rect = Rect {
            x: 10.0,
            y: 10.0,
            width,
            height,
        };
        assert_eq!(
            chips.part_at(rect, &m, (20.0, 15.0)),
            Some(ControlPart::Chip(0))
        );
        assert_eq!(
            chips.part_at(rect, &m, (10.0 + 50.0 + m.list_gap + 5.0, 15.0)),
            Some(ControlPart::Chip(1))
        );
        assert_eq!(chips.part_at(rect, &m, (10.0 + 50.0 + 2.0, 15.0)), None);
    }

    #[test]
    fn the_shortcuts_group_without_a_view_is_empty_like_before() {
        let c = content(Group::Shortcuts, &Config::default());
        assert!(rows(&c).is_empty());
    }

    // ---- faixa

    fn banner_content(banner: &Banner) -> Content {
        build(
            key(Group::General),
            &Draft::new(&Config::default()),
            &ViewExtras {
                banner: Some(banner),
                ..ViewExtras::default()
            },
            &test_support::pt_br(),
            &metrics(),
            &mut TextMeasurer::new(),
        )
    }

    #[test]
    fn no_banner_means_no_banner_view() {
        assert!(content(Group::General, &Config::default()).banner.is_none());
    }

    #[test]
    fn the_conflict_banner_is_a_warning_with_reload_and_keep() {
        let c = banner_content(&Banner::Conflict);
        let banner = c.banner.expect("a faixa");
        assert!(!banner.error, "conflito é aviso, não erro");
        assert_eq!(banner.title, "O arquivo mudou fora daqui.");
        assert!(banner.body.is_none());
        let buttons: Vec<(BannerButton, &str)> = banner
            .buttons
            .iter()
            .map(|b| (b.button, b.label.as_str()))
            .collect();
        assert_eq!(
            buttons,
            [
                (BannerButton::Reload, "Recarregar"),
                (BannerButton::Keep, "Manter minhas alterações")
            ]
        );
        // Larguras medidas como as do diálogo: texto + `padding: 0 12`.
        assert!(banner.buttons.iter().all(|b| b.width > 24.0));
    }

    #[test]
    fn the_invalid_banner_is_an_error_with_the_position_and_the_open_file_button() {
        let error = porecatu_config::parse("[terminal.font\nsize = 14.0\n").unwrap_err();
        let c = banner_content(&Banner::Invalid(error));
        let banner = c.banner.expect("a faixa");
        assert!(banner.error);
        assert!(
            banner
                .title
                .starts_with("O arquivo de configuração é inválido")
        );
        let body = banner.body.expect("o erro");
        assert!(body.starts_with("linha 1, coluna"), "{body}");
        assert_eq!(banner.buttons.len(), 1);
        assert_eq!(banner.buttons[0].button, BannerButton::OpenFile);
        assert_eq!(banner.buttons[0].label, "Abrir arquivo no editor");
    }

    #[test]
    fn a_long_banner_text_is_cut_to_what_fits_before_the_buttons() {
        // Painel estreito: o texto não cabe inteiro e termina em reticências.
        let narrow = ContentKey {
            panel_width_bits: 560.0_f32.to_bits(),
            ..key(Group::General)
        };
        let error = porecatu_config::parse("[terminal.font\nsize = 14.0\n").unwrap_err();
        let c = build(
            narrow,
            &Draft::new(&Config::default()),
            &ViewExtras {
                banner: Some(&Banner::Invalid(error)),
                ..ViewExtras::default()
            },
            &test_support::pt_br(),
            &metrics(),
            &mut TextMeasurer::new(),
        );
        let banner = c.banner.unwrap();
        assert!(banner.title.ends_with('…'), "{}", banner.title);
    }
}
