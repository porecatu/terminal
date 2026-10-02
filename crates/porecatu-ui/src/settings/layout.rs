// SPDX-License-Identifier: GPL-3.0-or-later

//! Layout da janela de configurações (ADR-0060 §1): as faixas em que ela se
//! divide, como função pura de tamanho. Consumido pela pintura e pelo
//! hit-test -- e, a partir da tarefa de acessibilidade, pela árvore
//! (ADR-0059 §4) -- para os três nunca discordarem de onde cada faixa está.
//!
//! Existem aqui as faixas, os itens da guia (um por grupo), os botões do
//! rodapé, as linhas de opção do painel com a rolagem delas, o hit-test e a
//! ordem do foco por `Tab`. Medir texto **não** é daqui: quem chama mede (uma
//! vez, quando o layout muda) e entrega larguras e alturas prontas -- a
//! armadilha de medição por frame do CLAUDE.md.

use porecatu_config::Config;
use porecatu_render::Rect;

use super::catalog::Group;

/// As faixas da janela, em coordenadas lógicas de janela.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Layout {
    /// Faixa do cabeçalho, de ponta a ponta. `None` onde a decoração é
    /// nativa (macOS) e o título mora na barra do sistema.
    pub header: Option<Rect>,
    /// Guia lateral, à esquerda, abaixo do cabeçalho. Inclui o separador de
    /// 1px da borda direita.
    pub sidebar: Rect,
    /// Separador de 1px entre a guia e o painel -- a borda direita da guia.
    pub sidebar_separator: Rect,
    /// Painel (opções e rodapé), ocupando o resto.
    pub panel: Rect,
    /// A parte do painel que rola: o painel menos o rodapé.
    pub panel_body: Rect,
    /// Rodapé fixo na base do painel (ADR-0060 §1).
    pub footer: Rect,
}

/// Padding vertical do rodapé: o `12` de `padding: 12px 18px` (ADR-0060 §1).
pub(crate) const FOOTER_PADDING_Y: f32 = 12.0;

/// Espaço entre itens da guia (ADR-0060 §2: `gap: 2`).
pub(crate) const SIDEBAR_ITEM_GAP: f32 = 2.0;

/// Os botões do rodapé, da esquerda para a direita (RF-16.14): é a ordem de
/// leitura e de `Tab`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FooterButton {
    OpenFile,
    Discard,
    Save,
}

pub(crate) const FOOTER_BUTTONS: [FooterButton; 3] = [
    FooterButton::OpenFile,
    FooterButton::Discard,
    FooterButton::Save,
];

/// Largura do separador entre a guia e o painel (ADR-0060 §1: "1px").
const SEPARATOR_WIDTH: f32 = 1.0;

/// Divide uma janela de `width` × `height` em cabeçalho, guia e painel.
/// `header_height` é zero quando não há cabeçalho nosso. `sidebar_width` é
/// grampeada à largura da janela: uma janela mais estreita que a guia (o
/// mínimo é maior que ela, mas o SO pode entregar menos) não produz retângulo
/// de largura negativa.
pub(crate) fn layout(
    width: f32,
    height: f32,
    header_height: f32,
    sidebar_width: f32,
    footer_height: f32,
) -> Layout {
    let header_height = header_height.min(height).max(0.0);
    let sidebar_width = sidebar_width.min(width).max(0.0);
    let body_height = height - header_height;
    let header = (header_height > 0.0).then_some(Rect {
        x: 0.0,
        y: 0.0,
        width,
        height: header_height,
    });
    let sidebar = Rect {
        x: 0.0,
        y: header_height,
        width: sidebar_width,
        height: body_height,
    };
    let separator_width = SEPARATOR_WIDTH.min(sidebar_width);
    let sidebar_separator = Rect {
        x: sidebar_width - separator_width,
        y: header_height,
        width: separator_width,
        height: body_height,
    };
    let panel = Rect {
        x: sidebar_width,
        y: header_height,
        width: width - sidebar_width,
        height: body_height,
    };
    let footer_height = footer_height.min(body_height).max(0.0);
    let panel_body = Rect {
        height: body_height - footer_height,
        ..panel
    };
    let footer = Rect {
        y: panel.y + panel_body.height,
        height: footer_height,
        ..panel
    };
    Layout {
        header,
        sidebar,
        sidebar_separator,
        panel,
        panel_body,
        footer,
    }
}

/// Os itens da guia: um retângulo por grupo, de cima para baixo, dentro do
/// `padding` da guia (ADR-0060 §1: 6) e com `SIDEBAR_ITEM_GAP` entre eles.
/// `item_height` é a do item de menu (ADR-0060 §2: a guia é uma lista de itens
/// de menu). O retângulo do último pode passar da base numa janela baixa --
/// a guia não rola (RF-16.7), o mínimo de altura da janela garante que cabe.
pub(crate) fn group_items(sidebar: Rect, padding: f32, item_height: f32) -> Vec<(Group, Rect)> {
    Group::ALL
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let rect = Rect {
                x: sidebar.x + padding,
                y: sidebar.y + padding + index as f32 * (item_height + SIDEBAR_ITEM_GAP),
                width: (sidebar.width - padding * 2.0 - 1.0).max(0.0),
                height: item_height,
            };
            (*group, rect)
        })
        .collect()
}

// ---- métricas

/// Todas as dimensões do painel, de um lugar só. As que têm token vêm de
/// `Config` (o que o ADR-0060 chama de "widgets que o app já desenha": o
/// campo do editor de grupo, o botão do diálogo, o item do menu); as que o
/// ADR dá em número entram como constante com a citação; e as de
/// `[appearance.settings]` (ADR-0060 §5) vêm de `Config`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Metrics {
    /// Altura do cabeçalho; zero onde a decoração é nativa.
    pub header_height: f32,
    /// Largura da guia lateral (`[appearance.settings] sidebar_width`).
    pub sidebar_width: f32,
    /// `padding` da guia e altura do item: os do menu de contexto (ADR-0060
    /// §1, §2).
    pub sidebar_padding: f32,
    pub sidebar_item_height: f32,
    /// `padding: 18` do painel (ADR-0060 §1), que é também o das laterais do
    /// rodapé.
    pub panel_padding: f32,
    /// Altura do título do grupo: 15px (ADR-0060 §2).
    pub title_size: f32,
    /// Rótulo de seção: 10px (ADR-0060 §2; é o `section_font_size` do editor
    /// de grupo).
    pub section_size: f32,
    /// `gap: 24` entre seções (ADR-0060 §2).
    pub section_gap: f32,
    /// Entre linhas de opção: `row_gap` (§5).
    pub row_gap: f32,
    /// Linha de opção: `padding: 9px 11px`, raio 6 (ADR-0060 §2).
    pub row_padding_x: f32,
    pub row_padding_y: f32,
    pub row_radius: f32,
    /// Nome 12.5px, descrição e escopo 11px (ADR-0060 §2).
    pub name_size: f32,
    pub description_size: f32,
    /// O `font_size` do campo do editor de grupo, 13px (ADR-0060 §3).
    pub field_font_size: f32,
    pub field_height: f32,
    pub field_padding_x: f32,
    pub field_radius: f32,
    /// Botão do diálogo (ADR-0060 §3, §4): altura 30, `padding: 0 12`, raio 5,
    /// `gap: 8`, texto 12.5px.
    pub button_height: f32,
    pub button_padding_x: f32,
    pub button_gap: f32,
    pub button_radius: f32,
    pub button_font_size: f32,
    /// `text_field_width`, `number_field_width`, `theme_swatch_size` (§5).
    pub text_field_width: f32,
    pub number_field_width: f32,
    pub swatch_size: f32,
    /// Restaurar padrão: alvo 25×17 (ADR-0060 §2).
    pub restore_width: f32,
    pub restore_height: f32,
    /// Ponto de pendente 6×6 (ADR-0060 §2).
    pub dot_size: f32,
    /// `gap: 6` entre itens de lista e entre campos de nome e valor
    /// (ADR-0060 §3: o `trilha_gap`).
    pub list_gap: f32,
}

/// Espaço entre o nome e a descrição dentro da linha. O ADR-0060 §2 posiciona
/// "nome sobre descrição" sem dar o vão; é o `gap: 2` do menu (§2.16), o mesmo
/// que o ADR usa entre itens da guia.
// Fora do ADR-0060: valor derivado, a confirmar -- ver o resumo da tarefa 07.
pub(crate) const NAME_DESCRIPTION_GAP: f32 = 2.0;
/// Do título do grupo à primeira seção: o `gap: 24` entre seções (§2), que é o
/// único espaçamento vertical grande que o ADR dá.
// Fora do ADR-0060: valor derivado, a confirmar -- ver o resumo da tarefa 07.
pub(crate) const TITLE_GAP: f32 = 24.0;

impl Metrics {
    pub(crate) fn from_config(config: &Config, header_height: f32) -> Self {
        let menu = &config.appearance.context_menu;
        let editor = &config.appearance.group_editor;
        let dialog = &config.appearance.dialog;
        let settings = &config.appearance.settings;
        Self {
            header_height,
            sidebar_width: settings.sidebar_width as f32,
            sidebar_padding: menu.padding as f32,
            sidebar_item_height: menu.item_height as f32,
            panel_padding: 18.0,
            title_size: 15.0,
            section_size: editor.section_font_size as f32,
            section_gap: 24.0,
            row_gap: settings.row_gap as f32,
            row_padding_x: 11.0,
            row_padding_y: 9.0,
            row_radius: 6.0,
            name_size: menu.font_size as f32,
            description_size: 11.0,
            field_font_size: editor.input_font_size as f32,
            field_height: editor.input_height as f32,
            field_padding_x: editor.input_padding_x as f32,
            field_radius: editor.input_corner_radius as f32,
            button_height: dialog.button_height as f32,
            button_padding_x: dialog.button_padding_x as f32,
            button_gap: dialog.button_gap as f32,
            button_radius: dialog.button_corner_radius as f32,
            button_font_size: dialog.font_size as f32,
            text_field_width: settings.text_field_width as f32,
            number_field_width: settings.number_field_width as f32,
            swatch_size: settings.theme_swatch_size as f32,
            restore_width: 25.0,
            restore_height: 17.0,
            dot_size: 6.0,
            list_gap: 6.0,
        }
    }

    /// Altura do rodapé: `padding: 12px` em cima e embaixo de um botão.
    pub(crate) fn footer_height(&self) -> f32 {
        FOOTER_PADDING_Y * 2.0 + self.button_height
    }
}

// ---- painel

/// O que o layout precisa saber de cada bloco do painel -- só o que muda a
/// geometria, nada de texto.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BlockSpec {
    /// Rótulo de seção.
    Section,
    /// Linha de opção. `control` é o tamanho do controle (já medido por quem
    /// chama); `two_lines` diz se há descrição sob o nome e `reason` se há,
    /// abaixo dela, a razão de um valor recusado (RF-16.18).
    Row {
        control: (f32, f32),
        two_lines: bool,
        reason: bool,
    },
    /// Uma nota de `lines` linhas de 11px, fora de qualquer linha de opção: o
    /// aviso dos diretórios autorizados (RF-16.27) e a linha do tema da
    /// sessão (RF-16.25).
    Note { lines: usize },
}

/// A geometria de uma linha de opção, em coordenadas de **conteúdo** (ver
/// [`PanelGeometry`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RowGeometry {
    pub rect: Rect,
    pub name_origin: (f32, f32),
    pub description_origin: (f32, f32),
    /// Onde a razão de um valor recusado começa, abaixo da descrição.
    pub reason_origin: (f32, f32),
    pub control: Rect,
    /// Ponto de pendente, entre o controle e o botão de restaurar.
    pub dot: Rect,
    pub restore: Rect,
    /// Largura que sobra à esquerda para nome e descrição: o orçamento do
    /// truncamento com reticências.
    pub left_width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BlockGeometry {
    Section {
        label_origin: (f32, f32),
    },
    Row(RowGeometry),
    /// Uma nota: onde a primeira linha começa e a altura de cada uma.
    Note {
        origin: (f32, f32),
        line_height: f32,
    },
}

/// O painel inteiro, sem rolagem: `x` é relativo à borda esquerda do painel e
/// `y` ao topo do corpo rolável. Quem pinta ou testa o mouse soma a origem do
/// corpo e subtrai a rolagem.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PanelGeometry {
    pub title_origin: (f32, f32),
    pub blocks: Vec<BlockGeometry>,
    /// Altura total, com o `padding` de baixo.
    pub content_height: f32,
}

/// Largura que sobra para nome e descrição de uma linha cujo controle tem
/// `control_width`: a linha menos os dois `padding`, o grupo de restaurar e
/// ponto, e o vão até o controle. É a conta de [`panel_geometry`], exposta
/// para o truncamento ser decidido **antes** de a geometria existir.
pub(crate) fn row_left_width(m: &Metrics, panel_width: f32, control_width: f32) -> f32 {
    let row_width = panel_width - m.panel_padding * 2.0;
    let right_cluster =
        control_width + m.row_gap + m.dot_size + m.row_gap + m.restore_width + m.row_padding_x;
    (row_width - m.row_padding_x - right_cluster - m.row_gap).max(0.0)
}

/// Posiciona título, rótulos de seção e linhas, de cima para baixo.
pub(crate) fn panel_geometry(m: &Metrics, panel_width: f32, specs: &[BlockSpec]) -> PanelGeometry {
    let x = m.panel_padding;
    let width = (panel_width - m.panel_padding * 2.0).max(0.0);
    let title_origin = (x, m.panel_padding);
    let mut y = m.panel_padding + m.title_size + TITLE_GAP;
    let mut blocks = Vec::with_capacity(specs.len());
    let mut previous_was_row = false;
    let mut first = true;
    for spec in specs {
        match *spec {
            BlockSpec::Section => {
                if !first {
                    y += m.section_gap;
                }
                blocks.push(BlockGeometry::Section {
                    label_origin: (x, y),
                });
                y += m.section_size + m.row_gap;
                previous_was_row = false;
            }
            BlockSpec::Row {
                control,
                two_lines,
                reason,
            } => {
                if previous_was_row {
                    y += m.row_gap;
                }
                // Nome, depois a descrição e a razão, cada uma sob a
                // anterior com o vão do nome.
                let description_y = m.name_size + NAME_DESCRIPTION_GAP;
                let mut left_height = m.name_size;
                if two_lines {
                    left_height = description_y + m.description_size;
                }
                let reason_y = left_height + NAME_DESCRIPTION_GAP;
                if reason {
                    left_height = reason_y + m.description_size;
                }
                let inner = left_height.max(control.1).max(m.restore_height);
                let height = m.row_padding_y * 2.0 + inner;
                let rect = Rect {
                    x,
                    y,
                    width,
                    height,
                };
                let inner_x = x + m.row_padding_x;
                let right = x + width - m.row_padding_x;
                let center_y = y + height / 2.0;
                let restore = Rect {
                    x: right - m.restore_width,
                    y: center_y - m.restore_height / 2.0,
                    width: m.restore_width,
                    height: m.restore_height,
                };
                let dot = Rect {
                    x: restore.x - m.row_gap - m.dot_size,
                    y: center_y - m.dot_size / 2.0,
                    width: m.dot_size,
                    height: m.dot_size,
                };
                let control_rect = Rect {
                    x: dot.x - m.row_gap - control.0,
                    y: center_y - control.1 / 2.0,
                    width: control.0,
                    height: control.1,
                };
                let top = center_y - left_height / 2.0;
                blocks.push(BlockGeometry::Row(RowGeometry {
                    rect,
                    name_origin: (inner_x, top),
                    description_origin: (inner_x, top + description_y),
                    reason_origin: (inner_x, top + reason_y),
                    control: control_rect,
                    dot,
                    restore,
                    left_width: (control_rect.x - m.row_gap - inner_x).max(0.0),
                }));
                y += height;
                previous_was_row = true;
            }
            BlockSpec::Note { lines } => {
                // Como uma linha de opção para o espaçamento: o vão do que
                // vem antes e do que vem depois é o `row_gap`.
                if previous_was_row {
                    y += m.row_gap;
                }
                let line_height = m.description_size + NAME_DESCRIPTION_GAP;
                blocks.push(BlockGeometry::Note {
                    origin: (x, y),
                    line_height,
                });
                y += lines as f32 * line_height;
                previous_was_row = true;
            }
        }
        first = false;
    }
    PanelGeometry {
        title_origin,
        blocks,
        content_height: y + m.panel_padding,
    }
}

// ---- rolagem

/// O maior deslocamento de rolagem: o que passa da altura visível.
pub(crate) fn max_scroll(content_height: f32, viewport_height: f32) -> f32 {
    (content_height - viewport_height).max(0.0)
}

/// Rolagem recortada ao intervalo possível.
pub(crate) fn clamp_scroll(scroll: f32, content_height: f32, viewport_height: f32) -> f32 {
    scroll.clamp(0.0, max_scroll(content_height, viewport_height))
}

/// A menor rolagem que deixa `[top, top + height]` (coordenadas de conteúdo)
/// inteiro à vista: o painel acompanha o foco do teclado (ADR-0060 §1).
pub(crate) fn scroll_to_reveal(scroll: f32, top: f32, height: f32, viewport_height: f32) -> f32 {
    if top < scroll {
        top
    } else if top + height > scroll + viewport_height {
        top + height - viewport_height
    } else {
        scroll
    }
}

// ---- rodapé

/// Os retângulos dos três botões do rodapé: Abrir arquivo à esquerda, Descartar
/// e Salvar à direita (RF-16.14, ADR-0060 §4). `widths` é a largura de cada um,
/// na ordem de `FOOTER_BUTTONS`.
pub(crate) fn footer_buttons(
    m: &Metrics,
    footer: Rect,
    widths: [f32; 3],
) -> [(FooterButton, Rect); 3] {
    let y = footer.y + FOOTER_PADDING_Y;
    let rect = |x: f32, width: f32| Rect {
        x,
        y,
        width,
        height: m.button_height,
    };
    let open_file = rect(footer.x + m.panel_padding, widths[0]);
    let save = rect(
        footer.x + footer.width - m.panel_padding - widths[2],
        widths[2],
    );
    let discard = rect(save.x - m.button_gap - widths[1], widths[1]);
    [
        (FooterButton::OpenFile, open_file),
        (FooterButton::Discard, discard),
        (FooterButton::Save, save),
    ]
}

// ---- hit-test e foco

/// O que está sob um ponto da janela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hit {
    Group(Group),
    /// Índice do bloco no painel (sempre uma linha de opção): o fundo da
    /// linha, fora de controle e de botão de restaurar.
    Row(usize),
    /// Uma parte do controle da linha.
    Control(usize, ControlPart),
    /// O botão de restaurar padrão da linha.
    Restore(usize),
    Footer(FooterButton),
}

impl Hit {
    /// A linha de opção que este alvo toca, se tocar alguma: o realce do
    /// fundo e o botão de restaurar valem para a linha inteira.
    pub(crate) fn row_index(self) -> Option<usize> {
        match self {
            Hit::Row(index) | Hit::Control(index, _) | Hit::Restore(index) => Some(index),
            Hit::Group(_) | Hit::Footer(_) => None,
        }
    }
}

/// A parte de um controle que recebe o clique. Os controles de uma peça só
/// (alternância, campo, botão de escolha) são `Whole`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlPart {
    Whole,
    /// O segmento `n` de uma escolha de até três valores.
    Segment(usize),
    /// A alternância de `git.remote_poll_interval_secs`.
    GitToggle,
    /// O número de `git.remote_poll_interval_secs`.
    GitNumber,
    /// O campo `second` (o valor, não o nome) do item `item` de uma lista.
    ListField {
        item: usize,
        second: bool,
    },
    /// O `X` que remove o item.
    ListRemove(usize),
    /// O item "Adicionar" no fim da lista.
    ListAdd,
}

/// Onde ficam os campos e o `X` de um item de lista.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ListItemRects {
    /// O campo do texto, ou o do nome da variável.
    pub first: Rect,
    /// O campo do valor, só na lista de nome e valor.
    pub second: Option<Rect>,
    pub remove: Rect,
}

/// A geometria de uma lista dentro do retângulo do controle dela: um item por
/// linha -- campo de `text_field_width` (dividido em nome, `text_field_width
/// / 2`, e valor, o resto, quando `two_fields`) e o `X` à direita do campo,
/// com `list_gap` entre as partes -- e o item "Adicionar" logo abaixo
/// (ADR-0060 §3). A conta de [`ControlView::size`](super::content::ControlView::size).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ListGeometry {
    pub items: Vec<ListItemRects>,
    pub add: Rect,
}

pub(crate) fn list_geometry(
    control: Rect,
    m: &Metrics,
    count: usize,
    two_fields: bool,
) -> ListGeometry {
    let mut items = Vec::with_capacity(count);
    for index in 0..count {
        let y = control.y + index as f32 * (m.field_height + m.list_gap);
        let (first, second) = if two_fields {
            let name_width = (m.text_field_width / 2.0).floor();
            let value_width = (m.text_field_width - name_width - m.list_gap).max(0.0);
            (
                Rect {
                    x: control.x,
                    y,
                    width: name_width,
                    height: m.field_height,
                },
                Some(Rect {
                    x: control.x + name_width + m.list_gap,
                    y,
                    width: value_width,
                    height: m.field_height,
                }),
            )
        } else {
            (
                Rect {
                    x: control.x,
                    y,
                    width: m.text_field_width,
                    height: m.field_height,
                },
                None,
            )
        };
        let remove = Rect {
            x: control.x + m.text_field_width + m.list_gap,
            y: y + (m.field_height - m.restore_height) / 2.0,
            width: m.restore_width,
            height: m.restore_height,
        };
        items.push(ListItemRects {
            first,
            second,
            remove,
        });
    }
    let add = Rect {
        x: control.x,
        y: control.y + count as f32 * (m.field_height + m.list_gap),
        width: m.text_field_width,
        height: m.sidebar_item_height,
    };
    ListGeometry { items, add }
}

/// O alvo sob `point` (coordenadas lógicas de janela): guia, rodapé ou linha
/// do painel -- nessa ordem, e a linha só se o ponto está no corpo visível
/// (uma linha rolada para baixo do rodapé não responde).
pub(crate) fn hit_test(
    layout: &Layout,
    items: &[(Group, Rect)],
    footer: &[(FooterButton, Rect)],
    geometry: &PanelGeometry,
    scroll: f32,
    point: (f32, f32),
) -> Option<Hit> {
    if let Some((group, _)) = items.iter().find(|(_, rect)| contains(*rect, point)) {
        return Some(Hit::Group(*group));
    }
    if let Some((button, _)) = footer.iter().find(|(_, rect)| contains(*rect, point)) {
        return Some(Hit::Footer(*button));
    }
    if contains(layout.panel_body, point) {
        let (dx, dy) = (layout.panel_body.x, layout.panel_body.y - scroll);
        for (index, block) in geometry.blocks.iter().enumerate() {
            if let BlockGeometry::Row(row) = block {
                let rect = Rect {
                    x: row.rect.x + dx,
                    y: row.rect.y + dy,
                    ..row.rect
                };
                if contains(rect, point) {
                    return Some(Hit::Row(index));
                }
            }
        }
    }
    None
}

fn contains(rect: Rect, (x, y): (f32, f32)) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

/// Um ponto de parada do `Tab` (RF-16.10): a guia (uma parada só -- as setas
/// andam dentro dela), cada linha de opção, e os botões do rodapé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    Sidebar,
    Row(usize),
    Footer(FooterButton),
}

/// A ordem do foco: guia, linhas do painel, rodapé. `available` são os
/// botões do rodapé que podem receber foco -- um botão indisponível não é
/// parada (esmaecido, nunca ausente, mas também nunca focável).
pub(crate) fn focus_order(rows: &[usize], available: &[FooterButton]) -> Vec<Focus> {
    let mut order = vec![Focus::Sidebar];
    order.extend(rows.iter().map(|index| Focus::Row(*index)));
    order.extend(
        FOOTER_BUTTONS
            .iter()
            .filter(|button| available.contains(button))
            .map(|button| Focus::Footer(*button)),
    );
    order
}

/// O próximo foco a partir de `current`, dando a volta nas pontas. Foco fora
/// da ordem (uma linha que sumiu ao trocar de grupo) recomeça pela guia.
pub(crate) fn next_focus(order: &[Focus], current: Focus, backwards: bool) -> Focus {
    let Some(position) = order.iter().position(|focus| *focus == current) else {
        return Focus::Sidebar;
    };
    let len = order.len();
    let next = if backwards {
        (position + len - 1) % len
    } else {
        (position + 1) % len
    };
    order[next]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_the_window_into_header_sidebar_and_panel() {
        let l = layout(900.0, 640.0, 52.0, 200.0, 54.0);
        let header = l.header.unwrap();
        assert_eq!(
            (header.x, header.y, header.width, header.height),
            (0.0, 0.0, 900.0, 52.0)
        );
        assert_eq!((l.sidebar.x, l.sidebar.y), (0.0, 52.0));
        assert_eq!((l.sidebar.width, l.sidebar.height), (200.0, 588.0));
        assert_eq!((l.panel.x, l.panel.y), (200.0, 52.0));
        assert_eq!((l.panel.width, l.panel.height), (700.0, 588.0));
    }

    #[test]
    fn without_a_header_the_body_takes_the_whole_height() {
        let l = layout(900.0, 640.0, 0.0, 200.0, 54.0);
        assert!(l.header.is_none());
        assert_eq!((l.sidebar.y, l.sidebar.height), (0.0, 640.0));
        assert_eq!((l.panel.y, l.panel.height), (0.0, 640.0));
    }

    #[test]
    fn the_separator_is_the_right_edge_of_the_sidebar() {
        let l = layout(900.0, 640.0, 52.0, 200.0, 54.0);
        assert_eq!(l.sidebar_separator.x + l.sidebar_separator.width, 200.0);
        assert_eq!(l.sidebar_separator.width, 1.0);
        assert_eq!(l.sidebar_separator.height, l.sidebar.height);
    }

    #[test]
    fn panel_and_sidebar_tile_the_width_exactly() {
        for width in [640.0, 777.5, 900.0, 1920.0] {
            let l = layout(width, 500.0, 52.0, 200.0, 54.0);
            assert_eq!(l.sidebar.width + l.panel.width, width);
        }
    }

    #[test]
    fn the_footer_is_fixed_at_the_base_of_the_panel() {
        let l = layout(900.0, 640.0, 52.0, 200.0, 54.0);
        assert_eq!(l.footer.height, 54.0);
        assert_eq!(l.footer.y + l.footer.height, 640.0);
        assert_eq!(l.panel_body.y + l.panel_body.height, l.footer.y);
        assert_eq!(l.footer.x, l.panel.x);
        assert_eq!(l.footer.width, l.panel.width);
    }

    #[test]
    fn a_window_shorter_than_the_footer_never_goes_negative() {
        let l = layout(900.0, 60.0, 52.0, 200.0, 54.0);
        assert!(l.panel_body.height >= 0.0);
        assert!(l.footer.height <= 8.0);
    }

    #[test]
    fn the_sidebar_lists_the_nine_groups_in_order_with_a_gap() {
        let l = layout(900.0, 640.0, 52.0, 200.0, 54.0);
        let items = group_items(l.sidebar, 6.0, 28.0);
        let groups: Vec<Group> = items.iter().map(|(group, _)| *group).collect();
        assert_eq!(groups, Group::ALL);
        assert_eq!(items[0].1.y, 52.0 + 6.0);
        assert_eq!(items[1].1.y - items[0].1.y, 28.0 + 2.0);
        assert_eq!(items[0].1.x, 6.0);
        for (_, rect) in &items {
            assert!(rect.x + rect.width <= l.sidebar_separator.x);
        }
        // Os nove cabem na altura mínima da janela (420) com folga.
        let last = items.last().unwrap().1;
        let min_height = Config::default().appearance.settings.min_height as f32;
        assert!(last.y + last.height <= min_height);
    }

    #[test]
    fn a_window_narrower_than_the_sidebar_never_goes_negative() {
        let l = layout(120.0, 30.0, 52.0, 200.0, 54.0);
        assert!(l.sidebar.width >= 0.0 && l.panel.width >= 0.0);
        assert!(l.sidebar.height >= 0.0 && l.panel.height >= 0.0);
        assert!(l.header.unwrap().height <= 30.0);
    }

    // ---- painel, rolagem, rodapé, hit-test, foco

    fn metrics() -> Metrics {
        Metrics::from_config(&Config::default(), 52.0)
    }

    const PANEL_WIDTH: f32 = 700.0;

    fn row(control: (f32, f32), two_lines: bool) -> BlockSpec {
        BlockSpec::Row {
            control,
            two_lines,
            reason: false,
        }
    }

    fn row_of(block: &BlockGeometry) -> RowGeometry {
        match block {
            BlockGeometry::Row(row) => *row,
            other => panic!("esperava linha, veio {other:?}"),
        }
    }

    #[test]
    fn metrics_come_from_the_tokens_the_adr_cites() {
        let m = metrics();
        assert_eq!(m.sidebar_padding, 6.0);
        assert_eq!(m.sidebar_item_height, 28.0);
        assert_eq!(m.name_size, 12.5);
        assert_eq!(m.description_size, 11.0);
        assert_eq!(m.section_size, 10.0);
        assert_eq!(m.field_height, 30.0);
        assert_eq!(m.field_font_size, 13.0);
        assert_eq!(m.button_height, 30.0);
        assert_eq!(m.button_padding_x, 12.0);
        assert_eq!(m.button_gap, 8.0);
        assert_eq!(m.row_padding_x, 11.0);
        assert_eq!(m.row_padding_y, 9.0);
        assert_eq!(m.row_radius, 6.0);
        assert_eq!(m.footer_height(), 54.0);
    }

    #[test]
    fn the_first_block_starts_below_the_title() {
        let m = metrics();
        let g = panel_geometry(&m, PANEL_WIDTH, &[BlockSpec::Section]);
        assert_eq!(g.title_origin, (18.0, 18.0));
        let BlockGeometry::Section { label_origin } = g.blocks[0] else {
            panic!()
        };
        assert_eq!(label_origin, (18.0, 18.0 + 15.0 + TITLE_GAP));
    }

    #[test]
    fn a_refused_value_adds_a_reason_line_below_the_description() {
        let m = metrics();
        // Controle baixo: a altura da linha é a do texto à esquerda.
        let control = (88.0, 10.0);
        let plain = panel_geometry(&m, PANEL_WIDTH, &[row(control, true)]);
        let with_reason = panel_geometry(
            &m,
            PANEL_WIDTH,
            &[BlockSpec::Row {
                control,
                two_lines: true,
                reason: true,
            }],
        );
        let plain = row_of(&plain.blocks[0]);
        let with_reason = row_of(&with_reason.blocks[0]);
        // A linha cresce o vão do nome mais a altura de uma linha de 11px, e a
        // razão começa abaixo da descrição.
        assert_eq!(
            with_reason.rect.height - plain.rect.height,
            NAME_DESCRIPTION_GAP + m.description_size
        );
        assert_eq!(
            with_reason.reason_origin.1,
            with_reason.description_origin.1 + m.description_size + NAME_DESCRIPTION_GAP
        );
        assert_eq!(with_reason.reason_origin.0, with_reason.name_origin.0);
    }

    #[test]
    fn a_reason_with_no_description_sits_right_under_the_name() {
        let m = metrics();
        let geometry = panel_geometry(
            &m,
            PANEL_WIDTH,
            &[BlockSpec::Row {
                control: (88.0, 30.0),
                two_lines: false,
                reason: true,
            }],
        );
        let row = row_of(&geometry.blocks[0]);
        assert_eq!(
            row.reason_origin.1,
            row.name_origin.1 + m.name_size + NAME_DESCRIPTION_GAP
        );
    }

    #[test]
    fn a_row_with_a_description_is_taller_than_one_without() {
        let m = metrics();
        let toggle = (34.0, 19.0);
        let g = panel_geometry(&m, PANEL_WIDTH, &[row(toggle, true), row(toggle, false)]);
        let with = row_of(&g.blocks[0]);
        let without = row_of(&g.blocks[1]);
        assert!(with.rect.height > without.rect.height);
        // padding de 9 em cima e embaixo em volta do bloco nome + descrição.
        assert_eq!(
            with.rect.height,
            9.0 * 2.0 + 12.5 + NAME_DESCRIPTION_GAP + 11.0
        );
        assert_eq!(without.rect.height, 9.0 * 2.0 + 19.0);
    }

    #[test]
    fn a_tall_control_decides_the_height_of_the_row() {
        let m = metrics();
        let g = panel_geometry(&m, PANEL_WIDTH, &[row((240.0, 30.0), true)]);
        assert_eq!(row_of(&g.blocks[0]).rect.height, 9.0 * 2.0 + 30.0);
        let list = panel_geometry(&m, PANEL_WIDTH, &[row((240.0, 140.0), true)]);
        assert_eq!(row_of(&list.blocks[0]).rect.height, 9.0 * 2.0 + 140.0);
    }

    #[test]
    fn the_right_cluster_runs_control_dot_restore_from_the_edge_inward() {
        let m = metrics();
        let g = panel_geometry(&m, PANEL_WIDTH, &[row((88.0, 30.0), true)]);
        let r = row_of(&g.blocks[0]);
        // restaurar encosta no padding de 11 da linha
        assert_eq!(
            r.restore.x + r.restore.width,
            r.rect.x + r.rect.width - 11.0
        );
        assert_eq!((r.restore.width, r.restore.height), (25.0, 17.0));
        assert_eq!((r.dot.width, r.dot.height), (6.0, 6.0));
        assert_eq!(r.restore.x - (r.dot.x + r.dot.width), m.row_gap);
        assert_eq!(r.dot.x - (r.control.x + r.control.width), m.row_gap);
        assert_eq!(r.control.width, 88.0);
        // tudo centrado na vertical da linha
        let center = r.rect.y + r.rect.height / 2.0;
        for rect in [r.control, r.dot, r.restore] {
            assert!((rect.y + rect.height / 2.0 - center).abs() < 0.001);
        }
    }

    #[test]
    fn the_name_budget_is_what_is_left_of_the_control() {
        let m = metrics();
        let g = panel_geometry(&m, PANEL_WIDTH, &[row((240.0, 30.0), true)]);
        let r = row_of(&g.blocks[0]);
        assert_eq!(r.left_width, row_left_width(&m, PANEL_WIDTH, 240.0));
        assert_eq!(r.name_origin.0 + r.left_width + m.row_gap, r.control.x);
        // controle mais largo, orçamento menor.
        assert!(row_left_width(&m, PANEL_WIDTH, 300.0) < r.left_width);
        // painel estreito demais nunca dá orçamento negativo.
        assert_eq!(row_left_width(&m, 100.0, 240.0), 0.0);
    }

    #[test]
    fn rows_are_separated_by_row_gap_and_sections_by_twenty_four() {
        let m = metrics();
        let toggle = (34.0, 19.0);
        let g = panel_geometry(
            &m,
            PANEL_WIDTH,
            &[
                BlockSpec::Section,
                row(toggle, true),
                row(toggle, true),
                BlockSpec::Section,
                row(toggle, true),
            ],
        );
        let first = row_of(&g.blocks[1]);
        let second = row_of(&g.blocks[2]);
        assert_eq!(
            second.rect.y - (first.rect.y + first.rect.height),
            m.row_gap
        );
        let BlockGeometry::Section {
            label_origin: label,
        } = g.blocks[3]
        else {
            panic!()
        };
        assert_eq!(
            label.1 - (second.rect.y + second.rect.height),
            m.section_gap
        );
        let third = row_of(&g.blocks[4]);
        assert_eq!(third.rect.y - (label.1 + m.section_size), m.row_gap);
    }

    #[test]
    fn content_height_includes_the_bottom_padding() {
        let m = metrics();
        let g = panel_geometry(&m, PANEL_WIDTH, &[row((34.0, 19.0), true)]);
        let r = row_of(&g.blocks[0]);
        assert_eq!(g.content_height, r.rect.y + r.rect.height + 18.0);
        let empty = panel_geometry(&m, PANEL_WIDTH, &[]);
        assert_eq!(empty.content_height, 18.0 + 15.0 + TITLE_GAP + 18.0);
    }

    #[test]
    fn scroll_stops_at_the_end_and_at_the_top() {
        assert_eq!(max_scroll(1000.0, 400.0), 600.0);
        assert_eq!(max_scroll(300.0, 400.0), 0.0);
        assert_eq!(clamp_scroll(9999.0, 1000.0, 400.0), 600.0);
        assert_eq!(clamp_scroll(-50.0, 1000.0, 400.0), 0.0);
        assert_eq!(clamp_scroll(120.0, 1000.0, 400.0), 120.0);
        // conteúdo que cabe nunca rola.
        assert_eq!(clamp_scroll(80.0, 300.0, 400.0), 0.0);
    }

    #[test]
    fn scroll_follows_the_focused_row() {
        // já à vista: não mexe.
        assert_eq!(scroll_to_reveal(100.0, 150.0, 40.0, 300.0), 100.0);
        // acima da vista: sobe até o topo da linha.
        assert_eq!(scroll_to_reveal(100.0, 60.0, 40.0, 300.0), 60.0);
        // abaixo da vista: desce até a base da linha.
        assert_eq!(scroll_to_reveal(100.0, 450.0, 40.0, 300.0), 190.0);
    }

    #[test]
    fn the_footer_buttons_sit_open_file_left_and_discard_save_right() {
        let m = metrics();
        let l = layout(900.0, 640.0, 52.0, 200.0, m.footer_height());
        let buttons = footer_buttons(&m, l.footer, [160.0, 80.0, 70.0]);
        let rect = |button| buttons.iter().find(|(b, _)| *b == button).unwrap().1;
        let (open, discard, save) = (
            rect(FooterButton::OpenFile),
            rect(FooterButton::Discard),
            rect(FooterButton::Save),
        );
        assert_eq!(open.x, l.footer.x + 18.0);
        assert_eq!(save.x + save.width, l.footer.x + l.footer.width - 18.0);
        assert_eq!(discard.x + discard.width + m.button_gap, save.x);
        for r in [open, discard, save] {
            assert_eq!(r.y, l.footer.y + 12.0);
            assert_eq!(r.height, 30.0);
        }
        assert!(open.x + open.width < discard.x);
    }

    #[test]
    fn hit_test_finds_sidebar_footer_and_rows() {
        let m = metrics();
        let l = layout(900.0, 640.0, 52.0, 200.0, m.footer_height());
        let items = group_items(l.sidebar, m.sidebar_padding, m.sidebar_item_height);
        let footer = footer_buttons(&m, l.footer, [160.0, 80.0, 70.0]);
        let geometry = panel_geometry(
            &m,
            l.panel.width,
            &[BlockSpec::Section, row((34.0, 19.0), true)],
        );
        let hit = |point| hit_test(&l, &items, &footer, &geometry, 0.0, point);

        let terminal = items[2].1;
        assert_eq!(
            hit((terminal.x + 5.0, terminal.y + 5.0)),
            Some(Hit::Group(Group::Terminal))
        );
        let save = footer[2].1;
        assert_eq!(
            hit((save.x + 1.0, save.y + 1.0)),
            Some(Hit::Footer(FooterButton::Save))
        );
        let r = row_of(&geometry.blocks[1]);
        let on_row = (
            l.panel_body.x + r.rect.x + 5.0,
            l.panel_body.y + r.rect.y + 5.0,
        );
        assert_eq!(hit(on_row), Some(Hit::Row(1)));
        // entre a guia e o painel, e no cabeçalho: nada.
        assert_eq!(hit((l.sidebar_separator.x, 300.0)), None);
        assert_eq!(hit((300.0, 10.0)), None);
    }

    #[test]
    fn a_row_scrolled_under_the_footer_does_not_answer() {
        let m = metrics();
        let l = layout(900.0, 640.0, 52.0, 200.0, m.footer_height());
        let items = group_items(l.sidebar, m.sidebar_padding, m.sidebar_item_height);
        let footer = footer_buttons(&m, l.footer, [160.0, 80.0, 70.0]);
        let specs: Vec<BlockSpec> = (0..30).map(|_| row((34.0, 19.0), true)).collect();
        let geometry = panel_geometry(&m, l.panel.width, &specs);
        let last = row_of(geometry.blocks.last().unwrap());
        let scroll = clamp_scroll(f32::MAX, geometry.content_height, l.panel_body.height);
        // com a rolagem no fim, a última linha está à vista e responde...
        let point = (
            l.panel_body.x + last.rect.x + 4.0,
            l.panel_body.y + last.rect.y - scroll + 4.0,
        );
        assert_eq!(
            hit_test(&l, &items, &footer, &geometry, scroll, point),
            Some(Hit::Row(29))
        );
        // ...e sem rolagem ela está muito abaixo do corpo: não responde.
        let below = (point.0, l.panel_body.y + last.rect.y + 4.0);
        assert_eq!(hit_test(&l, &items, &footer, &geometry, 0.0, below), None);
    }

    #[test]
    fn tab_walks_sidebar_then_rows_then_footer_and_wraps() {
        let order = focus_order(&[1, 2, 5], &[FooterButton::OpenFile]);
        assert_eq!(
            order,
            [
                Focus::Sidebar,
                Focus::Row(1),
                Focus::Row(2),
                Focus::Row(5),
                Focus::Footer(FooterButton::OpenFile)
            ]
        );
        let mut focus = Focus::Sidebar;
        let mut seen = vec![focus];
        for _ in 0..5 {
            focus = next_focus(&order, focus, false);
            seen.push(focus);
        }
        assert_eq!(seen[4], Focus::Footer(FooterButton::OpenFile));
        // dá a volta para a guia
        assert_eq!(focus, Focus::Sidebar);
    }

    #[test]
    fn shift_tab_walks_backwards_and_wraps_to_the_footer() {
        let order = focus_order(&[1], &[FooterButton::OpenFile]);
        assert_eq!(
            next_focus(&order, Focus::Sidebar, true),
            Focus::Footer(FooterButton::OpenFile)
        );
        assert_eq!(next_focus(&order, Focus::Row(1), true), Focus::Sidebar);
    }

    #[test]
    fn an_unavailable_footer_button_is_not_a_tab_stop() {
        let order = focus_order(&[], &[FooterButton::OpenFile]);
        assert!(!order.contains(&Focus::Footer(FooterButton::Save)));
        assert!(!order.contains(&Focus::Footer(FooterButton::Discard)));
        let with_pending = focus_order(&[], &FOOTER_BUTTONS);
        assert_eq!(with_pending.len(), 4);
    }

    #[test]
    fn a_focus_that_vanished_restarts_at_the_sidebar() {
        let order = focus_order(&[1], &[FooterButton::OpenFile]);
        assert_eq!(next_focus(&order, Focus::Row(9), false), Focus::Sidebar);
    }

    // ---- listas e notas

    fn control_rect() -> Rect {
        Rect {
            x: 300.0,
            y: 100.0,
            width: 280.0,
            height: 200.0,
        }
    }

    #[test]
    fn list_items_stack_with_the_gap_and_the_remove_button_sits_beside_the_field() {
        let m = metrics();
        let geometry = list_geometry(control_rect(), &m, 3, false);
        assert_eq!(geometry.items.len(), 3);
        for (index, item) in geometry.items.iter().enumerate() {
            assert_eq!(
                item.first.y,
                100.0 + index as f32 * (m.field_height + m.list_gap)
            );
            assert_eq!(item.first.width, m.text_field_width);
            assert_eq!(item.second, None);
            // O `X` fica depois do campo, com o vão, e centrado nele.
            assert_eq!(item.remove.x, item.first.x + item.first.width + m.list_gap);
            assert_eq!(
                item.remove.y + item.remove.height / 2.0,
                item.first.y + item.first.height / 2.0
            );
            assert_eq!(
                (item.remove.width, item.remove.height),
                (m.restore_width, m.restore_height)
            );
        }
        // O "Adicionar" vem logo depois do último item, na largura do campo.
        let last = geometry.items[2].first;
        assert_eq!(geometry.add.y, last.y + m.field_height + m.list_gap);
        assert_eq!(geometry.add.width, m.text_field_width);
        assert_eq!(geometry.add.height, m.sidebar_item_height);
    }

    #[test]
    fn the_list_geometry_matches_the_height_the_control_reserves() {
        let m = metrics();
        for count in [0, 1, 4] {
            let geometry = list_geometry(control_rect(), &m, count, true);
            let bottom = geometry.add.y + geometry.add.height;
            let reserved = count as f32 * (m.field_height + m.list_gap) + m.sidebar_item_height;
            assert_eq!(bottom - control_rect().y, reserved);
        }
    }

    #[test]
    fn an_env_item_splits_name_half_and_value_the_rest_with_the_gap_between() {
        let m = metrics();
        let item = list_geometry(control_rect(), &m, 1, true).items[0];
        let second = item.second.expect("o valor");
        assert_eq!(item.first.width, (m.text_field_width / 2.0).floor());
        assert_eq!(second.x, item.first.x + item.first.width + m.list_gap);
        // Nome, vão e valor ocupam exatamente a largura do campo.
        assert_eq!(
            second.x + second.width,
            control_rect().x + m.text_field_width
        );
        assert_eq!(second.y, item.first.y);
    }

    #[test]
    fn a_note_takes_its_lines_and_spaces_like_a_row() {
        let m = metrics();
        let row = BlockSpec::Row {
            control: (m.text_field_width, m.field_height),
            two_lines: true,
            reason: false,
        };
        let specs = [
            BlockSpec::Section,
            BlockSpec::Note { lines: 3 },
            row,
            row,
            BlockSpec::Note { lines: 1 },
        ];
        let geometry = panel_geometry(&m, 700.0, &specs);
        let BlockGeometry::Note {
            origin,
            line_height,
        } = geometry.blocks[1]
        else {
            panic!()
        };
        assert_eq!(line_height, m.description_size + NAME_DESCRIPTION_GAP);
        assert_eq!(origin.0, m.panel_padding);
        let (BlockGeometry::Row(first), BlockGeometry::Row(second)) =
            (geometry.blocks[2], geometry.blocks[3])
        else {
            panic!()
        };
        // A linha vem depois das três linhas da nota, com o vão entre linhas.
        assert_eq!(first.rect.y, origin.1 + 3.0 * line_height + m.row_gap);
        assert_eq!(second.rect.y, first.rect.y + first.rect.height + m.row_gap);
        // A nota depois da última linha também guarda o vão.
        let BlockGeometry::Note { origin: last, .. } = geometry.blocks[4] else {
            panic!()
        };
        assert_eq!(last.1, second.rect.y + second.rect.height + m.row_gap);
        assert_eq!(
            geometry.content_height,
            last.1 + line_height + m.panel_padding
        );
    }
}
