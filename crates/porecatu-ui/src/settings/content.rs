// SPDX-License-Identifier: GPL-3.0-or-later

//! O que o painel mostra e as medidas que dependem do texto: o modelo de
//! visão de um grupo (seções, linhas, o que cada controle desenha), já com o
//! texto truncado e as larguras medidas. É montado **quando o layout muda**
//! -- troca de grupo, de largura, de idioma ou de config -- e guardado:
//! medir texto por frame é a armadilha de desempenho do CLAUDE.md, e a
//! pintura, o hit-test e a árvore de acessibilidade só leem o que está aqui.
//!
//! Tudo é leitura nesta etapa: os valores vêm de um `Config` (o que o arquivo
//! diz), sem rascunho.

use porecatu_config::{Config, EditValue};
use porecatu_locale::Catalog;
use porecatu_render::{Color, TextMeasurer};
use porecatu_term::TermColor;

use super::catalog::{Control, Group, OptionDef, ReloadScope, Section, options_in};
use super::layout::{self, BlockSpec, Metrics, PanelGeometry};
use crate::messages::msg;
use crate::overlay::BODY_FONT;
use crate::palette::ResolvedTermPalette;

/// Quantos quadrados tem a amostra de um tema: fundo, texto e as oito ANSI
/// normais (RF-16.24).
pub(crate) const SWATCH_COUNT: usize = 10;
/// Vão entre os quadrados da amostra (ADR-0060 §3: `gap: 2`).
pub(crate) const SWATCH_GAP: f32 = 2.0;

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
    /// (`shell.env`, como `NOME=valor`): um campo por item e o item
    /// "Adicionar" no fim.
    List {
        items: Vec<String>,
        add_label: String,
    },
    /// Amostra de um tema.
    Themes {
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
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Block {
    Section(String),
    Row(RowView),
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
            .filter(|(_, block)| matches!(block, Block::Row(_)))
            .map(|(index, _)| index)
            .collect()
    }

    pub(crate) fn row(&self, block: usize) -> Option<&RowView> {
        match self.blocks.get(block)? {
            Block::Row(row) => Some(row),
            Block::Section(_) => None,
        }
    }
}

/// Monta o conteúdo de `group`: mede o texto uma vez e guarda o resultado.
pub(crate) fn build(
    key: ContentKey,
    config: &Config,
    catalog: &Catalog,
    m: &Metrics,
    measurer: &mut TextMeasurer,
) -> Content {
    let panel_width = f32::from_bits(key.panel_width_bits);
    let mut blocks = Vec::new();
    let mut current_section: Option<Section> = None;
    for option in options_in(key.group) {
        if current_section != Some(option.section) {
            blocks.push(Block::Section(option.section.label(catalog)));
            current_section = Some(option.section);
        }
        if option.control == Control::Theme {
            push_theme_rows(
                &mut blocks,
                option,
                config,
                catalog,
                m,
                panel_width,
                measurer,
            );
        } else {
            blocks.push(Block::Row(option_row(
                option,
                config,
                catalog,
                m,
                panel_width,
                measurer,
            )));
        }
    }

    let specs: Vec<BlockSpec> = blocks
        .iter()
        .map(|block| match block {
            Block::Section(_) => BlockSpec::Section,
            Block::Row(row) => BlockSpec::Row {
                control: row.control.size(m),
                two_lines: !row.description.is_empty(),
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

    Content {
        key,
        title: key.group.label(catalog),
        blocks,
        geometry,
        footer_widths,
    }
}

fn option_row(
    option: &OptionDef,
    config: &Config,
    catalog: &Catalog,
    m: &Metrics,
    panel_width: f32,
    measurer: &mut TextMeasurer,
) -> RowView {
    let value = option.read(config);
    let control = control_view(option, &value, catalog, m, measurer);
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

    RowView {
        option: Some(option.id),
        name,
        description,
        description_full,
        description_truncated,
        scope,
        value_text: value_text(option, &value, catalog),
        control,
    }
}

fn control_view(
    option: &OptionDef,
    value: &EditValue,
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
                field(text.clone(), m.text_field_width, false, m, measurer)
            }
        }
        (Control::Number { float, .. }, number) => field(
            number_text(number, float),
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
        (Control::StringList, EditValue::StringList(items)) => ControlView::List {
            items: items.clone(),
            add_label: msg::settings::button::add_item(catalog),
        },
        (Control::StringMap, EditValue::StringMap(map)) => ControlView::List {
            items: map
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect(),
            add_label: msg::settings::button::add_item(catalog),
        },
        (Control::GitPoll, EditValue::Integer(seconds)) => {
            let text = if *seconds > 0 {
                seconds.to_string()
            } else {
                String::new()
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
    // Tabulação e quebra de linha (os separadores de palavra os têm) não se
    // desenham: aparecem escritos.
    let text = visible_controls(&text);
    let text_width = measurer.measure_width(&text, BODY_FONT, m.field_font_size);
    ControlView::Field {
        text,
        width,
        right_aligned,
        text_width,
    }
}

/// O texto de um campo com os caracteres de controle escritos (barra e a
/// letra), para o campo mostrar o que o valor tem em vez de um espaço em
/// branco.
fn visible_controls(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
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
        other => other.to_owned(),
    }
}

/// Número como a tela o mostra: inteiro sem decimais, decimal sem zeros que
/// sobram (`14`, `1.2`, `0.05`).
fn number_text(value: &EditValue, float: bool) -> String {
    match value {
        EditValue::Integer(number) => number.to_string(),
        EditValue::Float(number) if float => {
            let text = format!("{number:.2}");
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        }
        EditValue::Float(number) => format!("{number}"),
        _ => String::new(),
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

/// RF-16.24: uma linha por tema -- "sem tema", depois os de `[[themes]]` na
/// ordem do ciclo -- com a amostra de dez quadrados. O escolhido é o do
/// arquivo.
fn push_theme_rows(
    blocks: &mut Vec<Block>,
    option: &OptionDef,
    config: &Config,
    catalog: &Catalog,
    m: &Metrics,
    panel_width: f32,
    measurer: &mut TextMeasurer,
) {
    let chosen = config.terminal.theme.as_str();
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
        let control = ControlView::Themes {
            colors: Box::new(colors),
            selected: name == chosen,
        };
        let budget = layout::row_left_width(m, panel_width, control.size(m).0);
        let (name_text, _) = measurer.truncate(&label, BODY_FONT, m.name_size, budget);
        blocks.push(Block::Row(RowView {
            option: Some(option.id),
            name: name_text,
            description: String::new(),
            description_full: String::new(),
            description_truncated: false,
            scope: None,
            value_text: label,
            control,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::test_support;

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
            config,
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
                Block::Row(row) => Some(row),
                Block::Section(_) => None,
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
                Block::Row(_) => None,
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
        assert_eq!(visible_controls("a\tb\nc"), "a\\tb\\nc");
        assert_eq!(visible_controls("plain"), "plain");
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
    fn numbers_are_shown_without_trailing_zeros() {
        assert_eq!(number_text(&EditValue::Float(14.0), true), "14");
        assert_eq!(number_text(&EditValue::Float(1.2), true), "1.2");
        assert_eq!(number_text(&EditValue::Float(0.05), true), "0.05");
        assert_eq!(number_text(&EditValue::Float(0.0), true), "0");
        assert_eq!(number_text(&EditValue::Integer(10_000), false), "10000");
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
            &Config::default(),
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
        assert_eq!(
            by_id("shell_args"),
            ControlView::List {
                items: vec!["-l".to_owned(), "-i".to_owned()],
                add_label: "Adicionar".to_owned()
            }
        );
        let ControlView::List { items, .. } = by_id("shell_env") else {
            panic!()
        };
        assert_eq!(items, ["EDITOR=vim"]);
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
}
