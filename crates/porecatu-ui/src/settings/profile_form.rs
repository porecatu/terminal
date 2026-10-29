// SPDX-License-Identifier: GPL-3.0-or-later

//! O formulário do editor de perfil, em coluna (ADR-0080 §4): uma linha por
//! campo, o rótulo **acima** do controle, o campo na largura útil da janela e a
//! nota de erro **abaixo** dele.
//!
//! As linhas são as do painel do grupo "Perfis" (`content::profile_form_blocks`,
//! a mesma função) e os controles são os da tela -- o desenho de
//! `paint::paint_control`, o `ControlView::part_at` do clique, o
//! `layout::list_geometry` das listas. O que muda é a disposição, que sai de
//! `layout::column_geometry`: uma função pura de tamanho, lida pela pintura,
//! pelo hit-test **e** pela árvore de acessibilidade, para os três nunca
//! discordarem de onde cada linha está.
//!
//! Medir texto é caro: o conteúdo (texto cortado, larguras de botão) é medido
//! uma vez em [`FormView::build`] e guardado; só volta a ser medido quando a
//! largura da janela, o idioma ou a cópia mudam.

use std::collections::BTreeMap;

use porecatu_config::ProfileProblemKind;
use porecatu_locale::Catalog;
use porecatu_render::{Primitive, Rect, TextMeasurer};

use super::content::{self, Block, ControlView, ProfileTarget, RowView};
use super::layout::{self, ColumnGeometry, ColumnSpec, Hit, Metrics};
use super::paint::{self, ControlCtx};
use super::profiles_draft::{Field, Form};
use crate::overlay::BODY_FONT;
use crate::tab_bar::rect_contains;

/// As linhas do formulário medidas e posicionadas.
pub(crate) struct FormView {
    pub rows: Vec<RowView>,
    pub geometry: ColumnGeometry,
    /// As métricas das **listas**: o campo e o `X` de um item enchem a largura
    /// útil (`text_field_width` é a do campo, sem o vão e o `X`). Os outros
    /// controles leem as métricas da tela como estão.
    list_metrics: Metrics,
}

impl FormView {
    /// Monta as linhas de `form` para uma janela de `width` de largura.
    /// `problems` é o que está recusado.
    pub(crate) fn build(
        form: &Form,
        problems: &BTreeMap<Field, ProfileProblemKind>,
        catalog: &Catalog,
        m: &Metrics,
        width: f32,
        measurer: &mut TextMeasurer,
    ) -> Self {
        let rows: Vec<RowView> =
            content::profile_form_blocks(form, problems, catalog, m, width, measurer)
                .into_iter()
                .filter_map(|block| match block {
                    Block::Row(row) => Some(*row),
                    Block::Section(_) | Block::Note { .. } | Block::Filter { .. } => None,
                })
                .collect();
        let useful = (width - m.panel_padding * 2.0).max(0.0);
        let list_metrics = Metrics {
            text_field_width: (useful - m.list_gap - m.restore_width).max(0.0),
            ..*m
        };
        let specs: Vec<ColumnSpec> = rows
            .iter()
            .map(|row| {
                let (new_line, fixed) = placement(row.profile, m);
                ColumnSpec {
                    control_height: match &row.control {
                        control @ ControlView::List { .. } => control.size(&list_metrics).1,
                        control => control.size(m).1,
                    },
                    reason: row.invalid.is_some(),
                    new_line,
                    fixed,
                }
            })
            .collect();
        let geometry = layout::column_geometry(m, width, &specs);
        Self {
            rows,
            geometry,
            list_metrics,
        }
    }

    /// As métricas com que a linha `row` é desenhada e acertada: as das listas
    /// para uma lista, as da tela para o resto.
    pub(crate) fn metrics_for<'a>(&'a self, m: &'a Metrics, row: &RowView) -> &'a Metrics {
        match row.control {
            ControlView::List { .. } => &self.list_metrics,
            _ => m,
        }
    }

    /// A linha do campo `field` (ou da autenticação), se o tipo a tem.
    pub(crate) fn row_of(&self, target: ProfileTarget) -> Option<usize> {
        self.rows.iter().position(|row| row.profile == Some(target))
    }

    /// O retângulo do controle da linha `index`, em coordenadas de janela, com
    /// o corpo rolável em `body` rolado `scroll`.
    pub(crate) fn control_rect(&self, index: usize, body: Rect, scroll: f32) -> Option<Rect> {
        Some(shift(self.geometry.rows.get(index)?.control, body, scroll))
    }

    /// O retângulo do campo `part` da linha `index` (o campo de texto, ou um
    /// campo de um item de lista), em coordenadas de janela.
    pub(crate) fn field_rect(
        &self,
        index: usize,
        part: super::field_edit::EditPart,
        body: Rect,
        scroll: f32,
    ) -> Option<Rect> {
        use super::field_edit::EditPart;
        let control = self.control_rect(index, body, scroll)?;
        match (&self.rows.get(index)?.control, part) {
            (ControlView::Field { .. }, EditPart::Field) => Some(control),
            (
                ControlView::List {
                    items, two_fields, ..
                },
                EditPart::ListFirst(item) | EditPart::ListSecond(item),
            ) => {
                let geometry =
                    layout::list_geometry(control, &self.list_metrics, items.len(), *two_fields);
                let rects = geometry.items.get(item)?;
                if matches!(part, EditPart::ListSecond(_)) {
                    rects.second
                } else {
                    Some(rects.first)
                }
            }
            _ => None,
        }
    }

    /// O que está sob `point` (coordenadas de janela): uma parte do controle de
    /// uma linha, ou o resto da linha -- o rótulo --, que só leva o foco a ela.
    /// Só dentro do corpo visível: uma linha rolada para baixo do rodapé não
    /// responde.
    pub(crate) fn hit(
        &self,
        m: &Metrics,
        body: Rect,
        scroll: f32,
        point: (f32, f32),
    ) -> Option<Hit> {
        if !rect_contains(body, point) {
            return None;
        }
        for (index, (row, geometry)) in self.rows.iter().zip(&self.geometry.rows).enumerate() {
            if !rect_contains(shift(geometry.rect, body, scroll), point) {
                continue;
            }
            let control = shift(geometry.control, body, scroll);
            return Some(
                match row
                    .control
                    .part_at(control, self.metrics_for(m, row), point)
                {
                    Some(part) => Hit::Control(index, part),
                    None => Hit::Row(index),
                },
            );
        }
        None
    }

    /// Pinta as linhas: o rótulo acima, o controle (o desenho da tela), a nota
    /// de erro abaixo e, na linha com o foco do teclado, o anel de foco. Tudo
    /// recortado ao corpo.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(
        &self,
        ctx: &ControlCtx<'_>,
        body: Rect,
        scroll: f32,
        focused: Option<usize>,
        measurer: &mut TextMeasurer,
        out: &mut Vec<Primitive>,
    ) {
        let m = ctx.metrics;
        let pal = ctx.pal;
        out.push(Primitive::PushClip(body));
        for (index, (row, geometry)) in self.rows.iter().zip(&self.geometry.rows).enumerate() {
            let rect = shift(geometry.rect, body, scroll);
            // Fora do corpo visível não pinta: a rolagem já corta, e pular a
            // linha poupa os primitivos dela.
            if rect.y + rect.height < body.y || rect.y > body.y + body.height {
                continue;
            }
            out.push(paint::text(
                (
                    geometry.label_origin.0 + body.x,
                    geometry.label_origin.1 + body.y - scroll,
                ),
                &row.name,
                BODY_FONT,
                m.name_size,
                pal.menu_item_text,
            ));
            let control = shift(geometry.control, body, scroll);
            let ctx = ControlCtx {
                metrics: self.metrics_for(m, row),
                ..*ctx
            };
            paint::paint_control(&ctx, index, row, control, measurer, out);
            if let (Some(reason), Some(origin)) = (&row.invalid, geometry.reason_origin) {
                out.push(paint::text(
                    (origin.0 + body.x, origin.1 + body.y - scroll),
                    reason,
                    BODY_FONT,
                    m.description_size,
                    pal.warning_severity_error,
                ));
            }
            if focused == Some(index) {
                out.push(paint::ring(control, m.field_radius, pal));
            }
        }
        out.push(Primitive::PopClip);
    }
}

/// Onde o campo de `target` fica na linha do formulário: se abre uma linha e, se
/// a largura é fixa, qual. **Nome, identificador e badge** dividem uma linha, e
/// **host e porta** outra; o nome e o identificador (e o host) dividem a largura
/// que sobra por igual -- são os maiores --, e o badge e a porta, mais curtos,
/// ficam com a largura do campo numérico. Todo o resto ocupa uma linha sozinho.
fn placement(target: Option<ProfileTarget>, m: &Metrics) -> (bool, Option<f32>) {
    match target {
        Some(ProfileTarget::Field(Field::Id)) => (false, None),
        Some(ProfileTarget::Field(Field::Badge | Field::Port)) => {
            (false, Some(m.number_field_width))
        }
        _ => (true, None),
    }
}

/// `rect` (coordenadas de conteúdo) em coordenadas de janela: soma a origem do
/// corpo e subtrai a rolagem.
fn shift(rect: Rect, body: Rect, scroll: f32) -> Rect {
    Rect {
        x: rect.x + body.x,
        y: rect.y + body.y - scroll,
        ..rect
    }
}

/// O controle de `row` é um campo de texto de uma linha (não uma lista nem uma
/// escolha): `Enter` nele é OK, e o foco nele já é edição.
pub(crate) fn is_text_field(row: &RowView) -> bool {
    matches!(row.control, ControlView::Field { .. })
}

/// Um rascunho com um perfil local (`wsl`) e um SSH (`prod`), para os testes do
/// formulário e da árvore de acessibilidade.
#[cfg(test)]
pub(super) fn fixture_draft() -> super::profiles_draft::ProfilesDraft {
    const FILE: &str = r#"[[profiles.list]]
id = "wsl"
name = "WSL Ubuntu"
badge = "WSL"
program = "wsl.exe"
args = ["-d", "Ubuntu"]

[[profiles.list]]
id = "prod"
name = "prod-web-01"
type = "ssh"
host = "10.4.1.20"
user = "deploy"
identity_file = "~/.ssh/id_ed25519"
"#;
    let parsed = porecatu_config::parse_checked(FILE).expect("parses");
    super::profiles_draft::ProfilesDraft::new(&parsed.config.profiles.list)
}

/// O formulário do perfil local (`ssh == false`) ou SSH da fixture, com `width`
/// de largura. Com `refuse`, um campo está inválido -- o programa do local, o
/// host do SSH --, e só ele.
#[cfg(test)]
pub(super) fn fixture_view(ssh: bool, refuse: bool, width: f32) -> FormView {
    let draft = fixture_draft();
    let mut editor = draft.open_editor(usize::from(ssh)).expect("the profile");
    if refuse {
        let (field, value) = if ssh {
            (Field::Host, "a b")
        } else {
            (Field::Program, "")
        };
        editor.set_text(&draft, field, value);
    }
    let problems = draft.editor_problems(&editor).fields;
    let config = porecatu_config::Config::default();
    FormView::build(
        editor.form(),
        &problems,
        &crate::messages::test_support::pt_br(),
        &Metrics::from_config(&config, 52.0),
        width,
        &mut TextMeasurer::new(),
    )
}

/// As linhas de [`fixture_view`] numa janela de abertura, sem cofre.
#[cfg(test)]
pub(crate) fn rows_for_test(ssh: bool, refuse: bool) -> Vec<RowView> {
    let width = porecatu_config::Config::default()
        .appearance
        .settings
        .profile_window_width as f32;
    fixture_view(ssh, refuse, width).rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::layout::{ControlPart, NAME_DESCRIPTION_GAP};
    use crate::settings::profiles_draft::ListKind;

    fn m() -> Metrics {
        Metrics::from_config(&porecatu_config::Config::default(), 52.0)
    }

    fn width() -> f32 {
        porecatu_config::Config::default()
            .appearance
            .settings
            .profile_window_width as f32
    }

    fn targets(view: &FormView) -> Vec<ProfileTarget> {
        view.rows.iter().filter_map(|row| row.profile).collect()
    }

    /// O corpo rolável de uma janela de `height` de altura: tudo o que não é
    /// cabeçalho nem rodapé.
    fn body(height: f32) -> Rect {
        let m = m();
        layout::layout_with_banner(
            width(),
            height,
            m.header_height,
            0.0,
            m.footer_height(),
            0.0,
        )
        .panel_body
    }

    #[test]
    fn a_local_form_has_its_fields_then_the_two_lists() {
        let view = fixture_view(false, false, width());
        assert_eq!(
            targets(&view),
            [
                ProfileTarget::Field(Field::Name),
                ProfileTarget::Field(Field::Id),
                ProfileTarget::Field(Field::Badge),
                ProfileTarget::Field(Field::Program),
                ProfileTarget::Field(Field::Cwd),
                ProfileTarget::List(ListKind::Args),
                ProfileTarget::List(ListKind::Env),
            ],
            "a local profile shows no host and no vault buttons"
        );
    }

    #[test]
    fn an_ssh_form_puts_the_authentication_between_the_user_and_the_key_file() {
        let view = fixture_view(true, false, width());
        assert_eq!(
            targets(&view),
            [
                ProfileTarget::Field(Field::Name),
                ProfileTarget::Field(Field::Id),
                ProfileTarget::Field(Field::Badge),
                ProfileTarget::Field(Field::Host),
                ProfileTarget::Field(Field::Port),
                ProfileTarget::Field(Field::User),
                ProfileTarget::Auth,
                ProfileTarget::Field(Field::IdentityFile),
            ],
            "an SSH profile shows no program and no lists"
        );
    }

    #[test]
    fn the_label_sits_above_its_control() {
        let m = m();
        for ssh in [false, true] {
            let view = fixture_view(ssh, false, width());
            for (row, geometry) in view.rows.iter().zip(&view.geometry.rows) {
                assert_eq!(
                    geometry.control.y,
                    geometry.label_origin.1 + m.name_size + NAME_DESCRIPTION_GAP,
                    "{}: the control starts right below the label",
                    row.name
                );
                assert_eq!(
                    geometry.label_origin.0, geometry.control.x,
                    "{}: label over its field",
                    row.name
                );
            }
        }
    }

    #[test]
    fn a_field_alone_on_its_line_takes_the_useful_width_of_the_window() {
        let m = m();
        let useful = width() - m.panel_padding * 2.0;
        let view = fixture_view(true, false, width());
        for (row, geometry) in view.rows.iter().zip(&view.geometry.rows) {
            let shared = matches!(
                row.profile,
                Some(ProfileTarget::Field(
                    Field::Name | Field::Id | Field::Badge | Field::Host | Field::Port
                ))
            );
            if !shared {
                assert_eq!(geometry.control.width, useful, "{}", row.name);
                assert_eq!(geometry.control.x, m.panel_padding, "{}", row.name);
            }
        }
    }

    #[test]
    fn widening_the_window_widens_the_flexible_fields_and_not_the_fixed_ones() {
        let narrow = fixture_view(true, false, width());
        let wide = fixture_view(true, false, width() + 100.0);
        let width_of = |view: &FormView, field| {
            view.geometry.rows[view.row_of(ProfileTarget::Field(field)).unwrap()]
                .control
                .width
        };
        // Um campo sozinho cresce o tanto que a janela cresceu; o que divide a
        // linha com outro, parte disso; badge e porta não crescem.
        assert_eq!(
            width_of(&wide, Field::User) - width_of(&narrow, Field::User),
            100.0
        );
        assert_eq!(
            width_of(&wide, Field::Host) - width_of(&narrow, Field::Host),
            100.0
        );
        assert_eq!(
            width_of(&wide, Field::Name) - width_of(&narrow, Field::Name),
            50.0
        );
        assert_eq!(
            width_of(&wide, Field::Badge),
            width_of(&narrow, Field::Badge)
        );
        assert_eq!(width_of(&wide, Field::Port), width_of(&narrow, Field::Port));
    }

    #[test]
    fn the_error_note_sits_below_the_line_of_the_field_and_nowhere_else() {
        let m = m();
        let clean = fixture_view(true, false, width());
        assert!(
            clean
                .geometry
                .rows
                .iter()
                .all(|row| row.reason_origin.is_none()),
            "no refusal, no note"
        );
        let view = fixture_view(true, true, width());
        let refused: Vec<usize> = view
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.invalid.is_some())
            .map(|(index, _)| index)
            .collect();
        assert_eq!(refused.len(), 1);
        let geometry = view.geometry.rows[refused[0]];
        let origin = geometry.reason_origin.expect("the note");
        assert_eq!(
            origin.1,
            geometry.control.y + geometry.control.height + NAME_DESCRIPTION_GAP,
            "right below the line's controls"
        );
        assert_eq!(origin.0, m.panel_padding);
        for (index, other) in view.geometry.rows.iter().enumerate() {
            if index != refused[0] {
                assert!(other.reason_origin.is_none(), "row {index}");
            }
        }
    }

    #[test]
    fn lines_are_stacked_with_the_row_gap_and_the_content_ends_with_the_padding() {
        let m = m();
        for refuse in [false, true] {
            let view = fixture_view(true, refuse, width());
            let lines = lines(&view);
            let rows = &view.geometry.rows;
            assert_eq!(rows[lines[0][0]].rect.y, m.panel_padding);
            // A linha acaba na base do que ela tem de mais baixo -- o controle,
            // ou a última nota --, e a seguinte começa `row_gap` depois.
            let bottom = |line: &Vec<usize>| {
                line.iter()
                    .map(|i| {
                        let row = rows[*i];
                        row.reason_origin
                            .map_or(row.control.y + row.control.height, |o| {
                                o.1 + m.description_size
                            })
                    })
                    .fold(f32::MIN, f32::max)
            };
            for pair in lines.windows(2) {
                assert_eq!(rows[pair[1][0]].rect.y, bottom(&pair[0]) + m.row_gap);
            }
            assert_eq!(
                view.geometry.content_height,
                bottom(lines.last().unwrap()) + m.panel_padding
            );
        }
    }

    /// As linhas do formulário como a geometria as pôs: os campos que ficam na
    /// mesma altura (o mesmo `y` de rótulo) são da mesma linha.
    fn lines(view: &FormView) -> Vec<Vec<usize>> {
        let mut lines: Vec<Vec<usize>> = Vec::new();
        for (index, row) in view.geometry.rows.iter().enumerate() {
            match lines.last_mut() {
                Some(line) if view.geometry.rows[line[0]].label_origin.1 == row.label_origin.1 => {
                    line.push(index);
                }
                _ => lines.push(vec![index]),
            }
        }
        lines
    }

    fn targets_of(view: &FormView, line: &[usize]) -> Vec<ProfileTarget> {
        line.iter().filter_map(|i| view.rows[*i].profile).collect()
    }

    #[test]
    fn name_identifier_and_badge_share_a_line_and_so_do_host_and_port() {
        let ssh = fixture_view(true, false, width());
        let rows_of_ssh = lines(&ssh);
        assert_eq!(
            targets_of(&ssh, &rows_of_ssh[0]),
            [
                ProfileTarget::Field(Field::Name),
                ProfileTarget::Field(Field::Id),
                ProfileTarget::Field(Field::Badge),
            ]
        );
        assert_eq!(
            targets_of(&ssh, &rows_of_ssh[1]),
            [
                ProfileTarget::Field(Field::Host),
                ProfileTarget::Field(Field::Port),
            ]
        );
        // O resto, um por linha: usuário, autenticação e arquivo da chave.
        assert_eq!(
            rows_of_ssh[2..].iter().map(Vec::len).collect::<Vec<_>>(),
            [1, 1, 1]
        );

        // O perfil local não tem host: só a primeira linha é compartilhada.
        let local = fixture_view(false, false, width());
        let rows_of_local = lines(&local);
        assert_eq!(rows_of_local[0].len(), 3, "name, identifier and badge");
        assert!(
            rows_of_local[1..].iter().all(|line| line.len() == 1),
            "program, directory and the two lists have a line each"
        );
    }

    #[test]
    fn name_and_identifier_are_wide_and_equal_and_the_badge_is_the_narrow_number_field() {
        let m = m();
        let useful = width() - m.panel_padding * 2.0;
        let view = fixture_view(true, false, width());
        let at =
            |field| view.geometry.rows[view.row_of(ProfileTarget::Field(field)).unwrap()].control;
        let (name, id, badge) = (at(Field::Name), at(Field::Id), at(Field::Badge));
        assert_eq!(
            badge.width, m.number_field_width,
            "the badge is the small one"
        );
        assert_eq!(name.width, id.width, "the two big ones share what is left");
        assert!(name.width > badge.width && id.width > badge.width);
        // Da esquerda para a direita, com o vão das listas, e a linha inteira
        // ocupa a largura útil.
        assert_eq!(name.x, m.panel_padding);
        assert_eq!(id.x, name.x + name.width + m.list_gap);
        assert_eq!(badge.x, id.x + id.width + m.list_gap);
        assert_eq!(badge.x + badge.width, m.panel_padding + useful);
        // Na mesma altura.
        assert_eq!((name.y, id.y), (badge.y, badge.y));
    }

    #[test]
    fn host_is_wide_and_the_port_is_the_narrow_number_field_at_the_right() {
        let m = m();
        let useful = width() - m.panel_padding * 2.0;
        let view = fixture_view(true, false, width());
        let at =
            |field| view.geometry.rows[view.row_of(ProfileTarget::Field(field)).unwrap()].control;
        let (host, port) = (at(Field::Host), at(Field::Port));
        assert_eq!(port.width, m.number_field_width);
        assert_eq!(host.width, useful - m.list_gap - m.number_field_width);
        assert_eq!(port.x, host.x + host.width + m.list_gap);
        assert_eq!(port.x + port.width, m.panel_padding + useful);
        assert_eq!(host.y, port.y);
    }

    #[test]
    fn each_cell_of_a_line_has_its_own_label_above_its_own_field() {
        let m = m();
        let view = fixture_view(true, false, width());
        for line in lines(&view).iter().filter(|line| line.len() > 1) {
            for index in line {
                let row = view.geometry.rows[*index];
                assert_eq!(row.label_origin.0, row.control.x, "label over its field");
                assert_eq!(
                    row.control.y,
                    row.label_origin.1 + m.name_size + NAME_DESCRIPTION_GAP
                );
                assert_eq!(
                    row.rect.width, row.control.width,
                    "the cell is the field's width"
                );
            }
        }
    }

    #[test]
    fn a_click_on_a_shared_line_hits_the_cell_under_the_pointer() {
        let m = m();
        let view = fixture_view(true, false, width());
        let body = body(900.0);
        let name = view.row_of(ProfileTarget::Field(Field::Name)).unwrap();
        let badge = view.row_of(ProfileTarget::Field(Field::Badge)).unwrap();
        let centre = |index: usize| {
            let control = view.geometry.rows[index].control;
            (
                body.x + control.x + control.width / 2.0,
                body.y + control.y + control.height / 2.0,
            )
        };
        assert_eq!(
            view.hit(&m, body, 0.0, centre(name)),
            Some(Hit::Control(name, ControlPart::Whole))
        );
        assert_eq!(
            view.hit(&m, body, 0.0, centre(badge)),
            Some(Hit::Control(badge, ControlPart::Whole))
        );
    }

    #[test]
    fn the_error_notes_of_a_shared_line_stack_below_it_in_full_width() {
        let m = m();
        // Nome vazio e porta inválida na mesma tela: um erro na primeira linha
        // compartilhada e outro na segunda.
        let draft = fixture_draft();
        let mut editor = draft.open_editor(1).unwrap();
        editor.set_text(&draft, Field::Host, "a b");
        editor.set_text(&draft, Field::Port, "70000");
        let problems = draft.editor_problems(&editor).fields;
        let view = FormView::build(
            editor.form(),
            &problems,
            &crate::messages::test_support::pt_br(),
            &m,
            width(),
            &mut TextMeasurer::new(),
        );
        let host = view.row_of(ProfileTarget::Field(Field::Host)).unwrap();
        let port = view.row_of(ProfileTarget::Field(Field::Port)).unwrap();
        let (hg, pg) = (view.geometry.rows[host], view.geometry.rows[port]);
        // O parser dá um problema por vez; a nota, quando só um campo da linha
        // erra, fica abaixo da linha toda, alinhada à esquerda do formulário.
        let refused = [hg, pg]
            .into_iter()
            .filter_map(|g| g.reason_origin)
            .collect::<Vec<_>>();
        assert!(!refused.is_empty(), "the refused field has a note");
        for origin in refused {
            assert_eq!(origin.0, m.panel_padding, "left-aligned, full width");
            assert!(
                origin.1 > hg.control.y + hg.control.height,
                "below the line"
            );
        }
    }

    #[test]
    fn a_form_taller_than_the_window_scrolls_and_the_scroll_reveals_the_last_row() {
        let view = fixture_view(true, false, width());
        let m = m();
        let body_height = body(m.header_height + m.footer_height() + 120.0).height;
        let content = view.geometry.content_height;
        assert!(
            content > body_height,
            "the SSH form does not fit a short window"
        );
        let last = view.geometry.rows.last().unwrap();
        let scroll = layout::clamp_scroll(
            layout::scroll_to_reveal(0.0, last.rect.y, last.rect.height, body_height),
            content,
            body_height,
        );
        assert!(scroll > 0.0);
        assert!(last.rect.y >= scroll && last.rect.y + last.rect.height <= scroll + body_height);
        // E nunca passa do fim do conteúdo.
        assert_eq!(
            layout::clamp_scroll(10_000.0, content, body_height),
            content - body_height
        );
        // Numa janela alta o bastante nada rola.
        assert_eq!(layout::max_scroll(content, content + 1.0), 0.0);
    }

    #[test]
    fn a_click_on_a_control_hits_it_and_on_the_label_only_the_row() {
        let m = m();
        let view = fixture_view(true, false, width());
        let body = body(600.0);
        let host = view.row_of(ProfileTarget::Field(Field::Host)).unwrap();
        let geometry = view.geometry.rows[host];
        let at = |x: f32, y: f32| view.hit(&m, body, 0.0, (body.x + x, body.y + y));
        let center = (
            geometry.control.x + geometry.control.width / 2.0,
            geometry.control.y + geometry.control.height / 2.0,
        );
        assert_eq!(
            at(center.0, center.1),
            Some(Hit::Control(host, ControlPart::Whole))
        );
        assert_eq!(
            at(geometry.label_origin.0 + 1.0, geometry.label_origin.1 + 1.0),
            Some(Hit::Row(host)),
            "the label only takes the focus to the row"
        );
        // O vão entre duas linhas não é de nenhuma.
        let gap_y = geometry.rect.y + geometry.rect.height + m.row_gap / 2.0;
        assert_eq!(at(center.0, gap_y), None);
        // Fora do corpo -- sobre o cabeçalho -- nada responde.
        assert_eq!(
            view.hit(&m, body, 0.0, (body.x + center.0, body.y - 1.0)),
            None
        );
    }

    #[test]
    fn the_scroll_moves_what_is_under_the_pointer() {
        let m = m();
        let view = fixture_view(true, false, width());
        let body = body(600.0);
        let host = view.row_of(ProfileTarget::Field(Field::Host)).unwrap();
        let control = view.geometry.rows[host].control;
        let point = (body.x + control.x + 1.0, body.y + control.y + 1.0);
        assert_eq!(
            view.hit(&m, body, 0.0, point),
            Some(Hit::Control(host, ControlPart::Whole))
        );
        // Rolado até o campo sair do corpo, o ponto cai em outra linha.
        let scrolled = view.hit(&m, body, control.y + 200.0, point);
        assert_ne!(scrolled, Some(Hit::Control(host, ControlPart::Whole)));
    }

    #[test]
    fn a_list_fills_the_useful_width_with_its_field_and_the_remove_button() {
        use crate::settings::field_edit::EditPart;
        let m = m();
        let view = fixture_view(false, false, width());
        let args = view.row_of(ProfileTarget::List(ListKind::Args)).unwrap();
        let geometry = view.geometry.rows[args];
        let body = body(900.0);
        let ControlView::List {
            items, two_fields, ..
        } = &view.rows[args].control
        else {
            panic!("a list");
        };
        assert!(!two_fields);
        assert_eq!(items.len(), 2, "the profile has two arguments");
        let control = view.control_rect(args, body, 0.0).unwrap();
        let first = view
            .field_rect(args, EditPart::ListFirst(0), body, 0.0)
            .unwrap();
        assert_eq!(
            first.width + m.list_gap + m.restore_width,
            geometry.control.width
        );
        assert_eq!(geometry.control.width, width() - m.panel_padding * 2.0);
        assert_eq!(first.x, control.x);
        // Cada parte da lista responde: o campo, o `X` e o "Adicionar".
        let at = |rect: Rect| view.hit(&m, body, 0.0, (rect.x + 1.0, rect.y + 1.0));
        assert_eq!(
            at(first),
            Some(Hit::Control(
                args,
                ControlPart::ListField {
                    item: 0,
                    second: false
                }
            ))
        );
        let list = layout::list_geometry(control, view.metrics_for(&m, &view.rows[args]), 2, false);
        assert_eq!(
            at(list.items[1].remove),
            Some(Hit::Control(args, ControlPart::ListRemove(1)))
        );
        assert_eq!(at(list.add), Some(Hit::Control(args, ControlPart::ListAdd)));
    }

    #[test]
    fn the_environment_list_has_a_name_and_a_value_field_per_item() {
        let view = fixture_view(false, false, width());
        let env = view.row_of(ProfileTarget::List(ListKind::Env)).unwrap();
        let ControlView::List { two_fields, .. } = &view.rows[env].control else {
            panic!("a list");
        };
        assert!(two_fields);
    }

    /// O formulário de um perfil de `text` (o primeiro do arquivo), depois de
    /// `edit` mexer na cópia.
    fn view_of(
        text: &str,
        edit: impl FnOnce(
            &mut crate::settings::profile_editor::ProfileEditor,
            &crate::settings::profiles_draft::ProfilesDraft,
        ),
    ) -> FormView {
        let list = porecatu_config::parse(text).unwrap().0.profiles.list;
        let draft = crate::settings::profiles_draft::ProfilesDraft::new(&list);
        let mut editor = draft.open_editor(0).unwrap();
        edit(&mut editor, &draft);
        let problems = draft.editor_problems(&editor).fields;
        FormView::build(
            editor.form(),
            &problems,
            &crate::messages::test_support::pt_br(),
            &m(),
            width(),
            &mut TextMeasurer::new(),
        )
    }

    const SSH: &str = "[[profiles.list]]\nid = \"prod\"\nname = \"prod\"\ntype = \"ssh\"\nhost = \"h\"\nuser = \"u\"\n";

    #[test]
    fn an_empty_badge_of_an_ssh_profile_shows_the_default_as_placeholder() {
        let view = view_of(SSH, |_, _| {});
        let badge = view.row_of(ProfileTarget::Field(Field::Badge)).unwrap();
        let ControlView::Field {
            text, placeholder, ..
        } = &view.rows[badge].control
        else {
            panic!("a text field");
        };
        assert_eq!((text.as_str(), placeholder.as_str()), ("", "SSH"));
        // A local profile's badge has no hint, and neither has the name.
        let local = fixture_view(false, false, width());
        for field in [Field::Name, Field::Badge] {
            let row = local.row_of(ProfileTarget::Field(field)).unwrap();
            let ControlView::Field { placeholder, .. } = &local.rows[row].control else {
                panic!("a text field");
            };
            assert!(placeholder.is_empty(), "{field:?}");
        }
    }

    #[test]
    fn an_authentication_that_is_no_preset_reads_as_a_custom_order() {
        let text = "[[profiles.list]]\nid=\"c\"\nname=\"c\"\ntype=\"ssh\"\nhost=\"h\"\nuser=\"u\"\nauth=[\"password\",\"agent\"]\n";
        let view = view_of(text, |_, _| {});
        let auth = view.row_of(ProfileTarget::Auth).unwrap();
        let ControlView::Choice { text } = &view.rows[auth].control else {
            panic!("a choice");
        };
        assert_eq!(text, "Ordem personalizada");
        // E o preset de sempre mostra o nome dele.
        let view = fixture_view(true, false, width());
        let auth = view.row_of(ProfileTarget::Auth).unwrap();
        let ControlView::Choice { text } = &view.rows[auth].control else {
            panic!("a choice");
        };
        assert_eq!(text, "Agente, chave, senha");
    }

    #[test]
    fn a_refused_field_carries_the_reason_of_the_parser_on_its_own_row() {
        let view = fixture_view(true, true, width());
        let host = view.row_of(ProfileTarget::Field(Field::Host)).unwrap();
        assert!(
            view.rows[host]
                .invalid
                .as_deref()
                .is_some_and(|reason| reason.contains("host"))
        );
        let name = view.row_of(ProfileTarget::Field(Field::Name)).unwrap();
        assert!(view.rows[name].invalid.is_none(), "only the host is wrong");
    }

    #[test]
    fn the_lists_show_the_arguments_and_the_environment_with_the_bad_name_marked() {
        let text = "[[profiles.list]]\nid = \"wsl\"\nname = \"WSL\"\nbadge = \"WSL\"\nprogram = \"wsl.exe\"\n";
        let view = view_of(text, |editor, _| {
            editor.set_args(vec!["-d".into(), "Debian".into()]);
            editor.set_env(vec![("A".into(), "1".into()), ("A".into(), "2".into())]);
        });
        let args = view.row_of(ProfileTarget::List(ListKind::Args)).unwrap();
        let ControlView::List { items, .. } = &view.rows[args].control else {
            panic!("args");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].first, "Debian");
        let env = view.row_of(ProfileTarget::List(ListKind::Env)).unwrap();
        let ControlView::List { items, .. } = &view.rows[env].control else {
            panic!("env");
        };
        assert!(items.iter().all(|item| item.invalid), "both names repeat");
    }
}
