// SPDX-License-Identifier: GPL-3.0-or-later

//! Painting of the task manager (PRD-024, ADR-0096 §1 to §5, ADR-0098 §5):
//! the layout of the frame, the three charts, the process list and its
//! footer with the background monitoring toggle, as primitives. Pure with
//! respect to the window: it takes data and a [`TextMeasurer`] and pushes
//! primitives, so it is testable without a GPU.
//!
//! What it does **not** paint is the background of the frame: the caller
//! (`App::redraw`, step 06) goes through the path of the panes' background
//! (color, opacity, image, acrylic) and calls [`paint`] over it, on the same
//! layer (`Layer::Grid`, one `Vec<Primitive>` per layer).
//!
//! The geometry lives in [`layout`] and is the **only** place it is computed:
//! the hit-test of step 07 reads the same [`TaskManagerLayout`] (CLAUDE.md,
//! "a formula is never copied").

use std::collections::{HashMap, HashSet};

use porecatu_config::Config;
use porecatu_locale::Catalog;
use porecatu_render::{
    Area, Color, FontFace, Point, Polyline, Primitive, Quad, Rect, RoundedQuad, SansWeight,
    TextMeasurer, TextRun,
};

use crate::messages::msg;
use crate::overlay::{BODY_FONT, TITLE_FONT};
use crate::palette::{self, ResolvedPalette};
use crate::tab_bar::TabBarStyle;
use crate::toggle::{TOGGLE_TRACK_HEIGHT, TOGGLE_TRACK_WIDTH, push_toggle};

use super::chart::{self, Metric};
use super::format::{self, ColumnHeaders, ColumnModels};
use super::rows::{Row, RowKey, RowKind};
use super::{TaskManagerHistory, TaskManagerView};

/// Font of the chart titles and of the tab rows' titles: the system font,
/// Medium (ADR-0096 §2, §4).
const TITLE_MEDIUM: FontFace = FontFace::System {
    weight: SansWeight::Medium,
};
/// Font of the numbers: Iosevka, because they change every second and
/// fixed-width digits keep the column still (ADR-0096 §4).
const NUMBER_FONT: FontFace = FontFace::Mono { bold: false };

/// Size of a note row's text (ADR-0096 §4, typography table of the
/// specification §1.1): the same 11 px of the description lines of the
/// settings screen. The other two sizes come from the config tokens the
/// popovers already use ([`TaskManagerStyle::from_config`]).
const NOTE_FONT_SIZE: f32 = 11.0;

/// Thickness of the frame border, the grid lines and the header rule
/// (ADR-0096 §2 to §4: "1 px").
const HAIRLINE: f32 = 1.0;

/// Cap of each name-width cache: a session that runs thousands of distinct
/// process names does not grow it without bound; past the cap it restarts.
const NAME_CACHE_LIMIT: usize = 2048;

// ---------------------------------------------------------------------------
// Style
// ---------------------------------------------------------------------------

/// The numbers of the painting: the `[appearance.task_manager]` dimensions
/// and the three font sizes. The **colors** are the five of the theme plus
/// the tokens the other widgets already use, all read from the
/// [`ResolvedPalette`] passed to [`paint`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TaskManagerStyle {
    /// Inner margin of the frame on the four sides.
    pub padding: f32,
    /// Space between the charts column and the list column.
    pub column_gap: f32,
    /// Space between two stacked charts.
    pub chart_gap: f32,
    /// Height of the chart headers, of the list header and of the rows.
    pub row_height: f32,
    /// Horizontal inset inside rows, between columns and of the scale label.
    pub row_padding_x: f32,
    /// Indent per level of the process tree.
    pub tree_indent: f32,
    /// Thickness of the chart line.
    pub line_width: f32,
    /// Alpha multiplier of the area under the line.
    pub fill_opacity: f64,
    /// 12.5: titles, names and numbers (`[appearance.context_menu]
    /// font_size`, the size of a menu item).
    pub text_size: f32,
    /// 11: the notes ([`NOTE_FONT_SIZE`]).
    pub note_size: f32,
    /// 10: the scale label and the column headers
    /// (`[appearance.group_editor] section_font_size`).
    pub label_size: f32,
    /// Radius of the selection and hover (`[appearance.context_menu]
    /// item_corner_radius`).
    pub row_radius: f32,
    /// Side of the group dot of a tab row (`TabBarStyle::indicator_dot_size`,
    /// the pinned tabs' dot, ADR-0087 §3).
    pub dot_size: f32,
    /// Space between that dot and the title (`TabBarStyle::internal_gap`,
    /// the one between a tab's dot and its label).
    pub dot_gap: f32,
}

impl TaskManagerStyle {
    /// Built from the live `Config` (after the theme) and the tab bar style.
    /// The font sizes follow the tokens the popovers use (the config values,
    /// not a scale of `[terminal.font] size`: only the tab bar scales, the
    /// menus, the popovers and the settings screen take 12.5/11/10 as they
    /// are).
    pub(crate) fn from_config(config: &Config, tab_style: &TabBarStyle) -> Self {
        let tm = &config.appearance.task_manager;
        let menu = &config.appearance.context_menu;
        let editor = &config.appearance.group_editor;
        let px = |value: i32| value.max(0) as f32;
        Self {
            padding: px(tm.padding),
            column_gap: px(tm.column_gap),
            chart_gap: px(tm.chart_gap),
            row_height: px(tm.row_height),
            row_padding_x: px(tm.row_padding_x),
            tree_indent: px(tm.tree_indent),
            line_width: tm.line_width.max(0.0) as f32,
            fill_opacity: tm.fill_opacity.clamp(0.0, 1.0),
            text_size: menu.font_size as f32,
            note_size: NOTE_FONT_SIZE,
            label_size: editor.section_font_size as f32,
            row_radius: menu.item_corner_radius as f32,
            dot_size: tab_style.indicator_dot_size,
            dot_gap: tab_style.internal_gap,
        }
    }
}

/// The color of one chart's series.
fn series_color(metric: Metric, pal: &ResolvedPalette) -> Color {
    match metric {
        Metric::Cpu => pal.task_manager_cpu,
        Metric::Memory => pal.task_manager_memory,
        Metric::Disk => pal.task_manager_disk,
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// One chart block (ADR-0096 §2), in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChartLayout {
    /// Title and current value: `row_height` tall.
    pub header: Rect,
    /// The frame, border included: the rest of the block.
    pub frame: Rect,
    /// The frame minus its 1 px border: where grid, area and line live, and
    /// the clip of all of them.
    pub inner: Rect,
}

/// The list column (ADR-0096 §4), in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ListLayout {
    /// The whole column.
    pub column: Rect,
    /// Column labels: `row_height` tall, does not scroll.
    pub header: Rect,
    /// The 1 px rule under the header.
    pub rule: Rect,
    /// Where the rows scroll (the clip of the rows), between the header's
    /// rule and the footer's.
    pub body: Rect,
    pub row_height: f32,
    /// How many **whole** rows fit in the body: the page of the keyboard
    /// navigation and the number of rows painted.
    pub visible_rows: usize,
    /// ADR-0098 §5: the 1 px rule on top of the footer, the mirror of the
    /// one under the header.
    pub footer_rule: Rect,
    /// The fixed footer at the end of the column: `row_height` tall, does
    /// not scroll.
    pub footer: Rect,
    /// Where the footer's label may go: from `row_padding_x` past the
    /// footer's left edge to `row_padding_x` before the toggle. The label is
    /// right-aligned in it and cut with an ellipsis when it does not fit.
    pub footer_label: Rect,
    /// The toggle's track (34×19), `row_padding_x` from the column's right
    /// edge, centered on the footer's vertical. Never cut: in a column too
    /// narrow for it, it sticks to the column's left edge.
    pub toggle: Rect,
}

impl ListLayout {
    /// The rectangle of the `slot`-th visible row (0 is the first visible
    /// one), as wide as the column.
    pub(crate) fn row_rect(&self, slot: usize) -> Rect {
        Rect {
            x: self.column.x,
            y: self.body.y + slot as f32 * self.row_height,
            width: self.column.width,
            height: self.row_height,
        }
    }

    /// The slot of the visible row under `point` (logical pixels), `None`
    /// outside the rows. Whole rows only, like [`Self::visible_rows`].
    pub(crate) fn row_at(&self, point: (f32, f32)) -> Option<usize> {
        let (x, y) = point;
        let inside_x = x >= self.body.x && x < self.body.x + self.body.width;
        let rows_bottom = self.body.y + self.visible_rows as f32 * self.row_height;
        if !inside_x || y < self.body.y || y >= rows_bottom || self.row_height <= 0.0 {
            return None;
        }
        Some((((y - self.body.y) / self.row_height) as usize).min(self.visible_rows - 1))
    }

    /// ADR-0098 §5: the clickable part of the footer -- the label as it was
    /// painted (`label_width`, the last frame's, see
    /// [`MeasureCache::footer_label_width`]) plus the track, the footer's
    /// height. Without a label (none painted yet, or no room) only the track.
    pub(crate) fn footer_hit(&self, label_width: f32) -> Rect {
        let label = label_width.clamp(0.0, self.footer_label.width);
        let left = if label > 0.0 {
            self.footer_label.x + self.footer_label.width - label
        } else {
            self.toggle.x
        };
        let right = self.toggle.x + self.toggle.width;
        Rect {
            x: left,
            y: self.footer.y,
            width: (right - left).max(0.0),
            height: self.footer.height,
        }
    }

    /// Whether `point` is on [`Self::footer_hit`].
    pub(crate) fn footer_contains(&self, point: (f32, f32), label_width: f32) -> bool {
        let hit = self.footer_hit(label_width);
        let (x, y) = point;
        hit.width > 0.0
            && hit.height > 0.0
            && x >= hit.x
            && x < hit.x + hit.width
            && y >= hit.y
            && y < hit.y + hit.height
    }
}

/// The whole frame: three chart blocks and the list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TaskManagerLayout {
    /// CPU, memory and disk, top to bottom ([`Metric::ALL`] order).
    pub charts: [ChartLayout; 3],
    pub list: ListLayout,
}

/// Inset of `rect` by `by` on the four sides, never negative.
fn inset(rect: Rect, by: f32) -> Rect {
    let width = (rect.width - by * 2.0).max(0.0);
    let height = (rect.height - by * 2.0).max(0.0);
    if width == 0.0 || height == 0.0 {
        return Rect {
            x: rect.x + rect.width / 2.0,
            y: rect.y + rect.height / 2.0,
            width: 0.0,
            height: 0.0,
        };
    }
    Rect {
        x: rect.x + by,
        y: rect.y + by,
        width,
        height,
    }
}

/// ADR-0096 §1: `padding` on the four sides; two columns of equal width
/// separated by `column_gap`; the left one holds three chart blocks of equal
/// height separated by `chart_gap`, the right one the list. `rect` is the
/// terminal box (`paint::terminal_box_rect`).
pub(crate) fn layout(rect: Rect, style: &TaskManagerStyle) -> TaskManagerLayout {
    let inner_x = rect.x + style.padding;
    let inner_y = rect.y + style.padding;
    let inner_width = (rect.width - style.padding * 2.0).max(0.0);
    let inner_height = (rect.height - style.padding * 2.0).max(0.0);
    let column_width = ((inner_width - style.column_gap) / 2.0).max(0.0);
    let right_x = inner_x + column_width + style.column_gap;

    let block_height = ((inner_height - style.chart_gap * 2.0) / 3.0).max(0.0);
    let chart = |index: usize| {
        let y = inner_y + index as f32 * (block_height + style.chart_gap);
        let header_height = style.row_height.min(block_height);
        let frame = Rect {
            x: inner_x,
            y: y + header_height,
            width: column_width,
            height: block_height - header_height,
        };
        ChartLayout {
            header: Rect {
                x: inner_x,
                y,
                width: column_width,
                height: header_height,
            },
            frame,
            inner: inset(frame, HAIRLINE),
        }
    };

    let column = Rect {
        x: right_x,
        y: inner_y,
        width: column_width,
        height: inner_height,
    };
    let header_height = style.row_height.min(inner_height);
    let rule_height = HAIRLINE.min(inner_height - header_height);
    let body_top = inner_y + header_height + rule_height;
    // ADR-0098 §5: the footer and its rule take the end of the column; the
    // rows scroll between the two rules.
    let below_header = (inner_y + inner_height - body_top).max(0.0);
    let footer_height = style.row_height.min(below_header);
    let footer_rule_height = HAIRLINE.min(below_header - footer_height);
    let footer_top = inner_y + inner_height - footer_height;
    let footer_rule_top = footer_top - footer_rule_height;
    let body_height = (footer_rule_top - body_top).max(0.0);
    let visible_rows = if style.row_height > 0.0 {
        (body_height / style.row_height).floor() as usize
    } else {
        0
    };
    let footer = Rect {
        x: right_x,
        y: footer_top,
        width: column_width,
        height: footer_height,
    };
    let toggle_x = (right_x + column_width - style.row_padding_x - TOGGLE_TRACK_WIDTH).max(right_x);
    let toggle = Rect {
        x: toggle_x,
        y: footer_top + (footer_height - TOGGLE_TRACK_HEIGHT) / 2.0,
        width: TOGGLE_TRACK_WIDTH,
        height: TOGGLE_TRACK_HEIGHT,
    };
    let label_x = right_x + style.row_padding_x;
    let footer_label = Rect {
        x: label_x,
        y: footer_top,
        width: (toggle_x - style.row_padding_x - label_x).max(0.0),
        height: footer_height,
    };

    TaskManagerLayout {
        charts: [chart(0), chart(1), chart(2)],
        list: ListLayout {
            column,
            header: Rect {
                x: right_x,
                y: inner_y,
                width: column_width,
                height: header_height,
            },
            rule: Rect {
                x: right_x,
                y: inner_y + header_height,
                width: column_width,
                height: rule_height,
            },
            body: Rect {
                x: right_x,
                y: body_top,
                width: column_width,
                height: body_height,
            },
            row_height: style.row_height,
            visible_rows,
            footer_rule: Rect {
                x: right_x,
                y: footer_rule_top,
                width: column_width,
                height: footer_rule_height,
            },
            footer,
            footer_label,
            toggle,
        },
    }
}

/// Widths of the four numeric columns (ADR-0096 §4): the widest text each
/// can show, measured once (see [`MeasureCache`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct ColumnWidths {
    pub pid: f32,
    pub cpu: f32,
    pub memory: f32,
    pub disk: f32,
}

/// Where the columns of a list row sit, as x coordinates. The numeric
/// columns are right-aligned to their `*_right` edge and are laid from the
/// right; the name takes what is left and shrinks to zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Columns {
    pub name_left: f32,
    /// Right edge of the name area (the budget of a name is up to here).
    pub name_right: f32,
    pub pid_right: f32,
    pub cpu_right: f32,
    pub memory_right: f32,
    pub disk_right: f32,
}

impl Columns {
    /// `column`: the list column; rows inset `row_padding_x` from its edges,
    /// and `row_padding_x` between columns.
    pub(crate) fn new(column: Rect, widths: ColumnWidths, row_padding_x: f32) -> Self {
        let name_left = column.x + row_padding_x;
        let disk_right = column.x + column.width - row_padding_x;
        let memory_right = disk_right - widths.disk - row_padding_x;
        let cpu_right = memory_right - widths.memory - row_padding_x;
        let pid_right = cpu_right - widths.cpu - row_padding_x;
        let name_right = pid_right - widths.pid - row_padding_x;
        Self {
            name_left,
            name_right: name_right.max(name_left),
            pid_right,
            cpu_right,
            memory_right,
            disk_right,
        }
    }
}

// ---------------------------------------------------------------------------
// Measurement caches (ADR-0096 §5)
// ---------------------------------------------------------------------------

/// What the numeric columns were measured against: when any of it changes
/// (font size, the mono face and its spacing, the language) the caches are
/// dropped and measured again.
#[derive(Debug, Clone, PartialEq)]
struct MeasureKey {
    text_size: u32,
    label_size: u32,
    /// Advance of `M` in the mono face, in em (bits): moves with the family,
    /// the letter spacing and the cell alignment.
    mono_advance_em: u32,
    models: ColumnModels,
    headers: ColumnHeaders,
}

/// The column measurements under one [`MeasureKey`].
#[derive(Debug, Clone)]
struct ColumnCache {
    key: MeasureKey,
    widths: ColumnWidths,
    /// Advance of one mono glyph at `text_size`, in pixels: the width of a
    /// right-aligned number is its glyph count times this, with no shaping.
    mono_advance: f32,
    /// The headers, upper-cased for the section-label style.
    headers: ColumnHeaders,
    /// Width of each header at `label_size`, in the order name, pid, cpu,
    /// memory, disk.
    header_widths: [f32; 5],
}

/// A name's width and its last cut.
#[derive(Debug, Clone)]
struct NameEntry {
    width: f32,
    /// `(budget bits, text with the ellipsis)` of the last cut: one string is
    /// cut once per budget, not once per frame.
    cut: Option<(u32, String)>,
}

/// One string per entry; one face and size per cache.
#[derive(Debug, Clone, Default)]
struct NameCache {
    entries: HashMap<String, NameEntry>,
}

impl NameCache {
    /// The measured width of `text`, if it was ever fitted.
    fn width_of(&self, text: &str) -> Option<f32> {
        self.entries.get(text).map(|entry| entry.width)
    }

    /// `text` fitted to `budget` pixels: the text itself, or cut with an
    /// ellipsis, and whether it was cut. A string is **measured once** (one
    /// shaping), and **cut once per budget** (one more, by
    /// [`TextMeasurer::truncate`], never one per prefix).
    fn fit(
        &mut self,
        text: &str,
        font: FontFace,
        size: f32,
        budget: f32,
        measurer: &mut TextMeasurer,
    ) -> (String, bool) {
        let width = match self.entries.get(text) {
            Some(entry) => entry.width,
            None => {
                if self.entries.len() >= NAME_CACHE_LIMIT {
                    self.entries.clear();
                }
                let width = measurer.measure_width(text, font, size);
                self.entries
                    .insert(text.to_owned(), NameEntry { width, cut: None });
                width
            }
        };
        if width <= budget {
            return (text.to_owned(), false);
        }
        if let Some(entry) = self.entries.get_mut(text) {
            if let Some((bits, cut)) = &entry.cut
                && *bits == budget.to_bits()
            {
                return (cut.clone(), true);
            }
            let (cut, _) = measurer.truncate(text, font, size, budget);
            entry.cut = Some((budget.to_bits(), cut.clone()));
            return (cut, true);
        }
        (text.to_owned(), false)
    }
}

/// Measurement caches of the painting (ADR-0096 §5), kept in the
/// [`TaskManagerView`]: the widths of the numeric columns, measured **once**
/// per font size and language; the per-string widths of tab titles, process
/// names and notes (a process that appears costs one measurement, the same
/// `node.exe` costs nothing more); and which rows were cut this frame, for
/// the tooltip.
///
/// Dropped on its own when the font size, the mono face or the language
/// change (see [`MeasureKey`]); the app also calls [`Self::clear`] when the
/// font or the scale change.
#[derive(Debug, Clone, Default)]
pub(crate) struct MeasureCache {
    columns: Option<ColumnCache>,
    tab_titles: NameCache,
    process_names: NameCache,
    notes: NameCache,
    /// The footer's label (ADR-0098 §5): one string per language.
    footer: NameCache,
    /// Width of the footer's label as the **last** frame painted it: the
    /// hit-test of the footer reads it ([`ListLayout::footer_hit`]).
    footer_label_width: f32,
    /// The rows whose name was cut with an ellipsis in the **last** painted
    /// frame, among the visible ones.
    truncated: HashSet<RowKey>,
    /// How many times the numeric columns were measured. A diagnostic: it
    /// only moves when the [`MeasureKey`] changes.
    pub(crate) column_measurements: usize,
}

impl MeasureCache {
    /// Width of the footer's label in the last painted frame (0 before the
    /// first one): the hit-test of the footer uses it.
    pub(crate) fn footer_label_width(&self) -> f32 {
        self.footer_label_width
    }

    /// Forgets everything measured (font or scale changed).
    pub(crate) fn clear(&mut self) {
        let measurements = self.column_measurements;
        *self = Self::default();
        self.column_measurements = measurements;
    }

    /// Makes `self.columns` current for `style`, the catalog and the
    /// measurer's mono face, measuring only when the key changed.
    fn ensure_columns(
        &mut self,
        style: &TaskManagerStyle,
        catalog: &Catalog,
        measurer: &mut TextMeasurer,
    ) -> &ColumnCache {
        let key = MeasureKey {
            text_size: style.text_size.to_bits(),
            label_size: style.label_size.to_bits(),
            mono_advance_em: measurer.advance_em('M', NUMBER_FONT).to_bits(),
            models: format::column_models(catalog),
            headers: format::column_headers(catalog),
        };
        let stale = self.columns.as_ref().is_none_or(|cache| cache.key != key);
        if stale {
            self.tab_titles = NameCache::default();
            self.process_names = NameCache::default();
            self.notes = NameCache::default();
            self.footer = NameCache::default();
            self.column_measurements += 1;
            self.columns = Some(measure_columns(key, style, measurer));
        }
        self.columns
            .as_ref()
            .expect("the column cache was just filled")
    }
}

fn measure_columns(
    key: MeasureKey,
    style: &TaskManagerStyle,
    measurer: &mut TextMeasurer,
) -> ColumnCache {
    let size = style.text_size;
    let headers = ColumnHeaders {
        name: key.headers.name.to_uppercase(),
        pid: key.headers.pid.to_uppercase(),
        cpu: key.headers.cpu.to_uppercase(),
        memory: key.headers.memory.to_uppercase(),
        disk: key.headers.disk.to_uppercase(),
    };
    let header_widths = [
        &headers.name,
        &headers.pid,
        &headers.cpu,
        &headers.memory,
        &headers.disk,
    ]
    .map(|text| measurer.measure_width(text, TITLE_FONT, style.label_size));
    // A column is as wide as its widest model or, if wider, its header.
    let model = |text: &str, header: f32, measurer: &mut TextMeasurer| {
        measurer.measure_width(text, NUMBER_FONT, size).max(header)
    };
    let widths = ColumnWidths {
        pid: model(&key.models.pid, header_widths[1], measurer),
        cpu: model(&key.models.cpu, header_widths[2], measurer),
        memory: model(&key.models.memory, header_widths[3], measurer),
        disk: model(&key.models.disk, header_widths[4], measurer),
    };
    ColumnCache {
        mono_advance: f32::from_bits(key.mono_advance_em) * size,
        key,
        widths,
        headers,
        header_widths,
    }
}

/// `true` when the name of the row was cut with an ellipsis in the last
/// painted frame: the tooltip of step 07 shows the whole name only then
/// (ADR-0096 §4, RF-24.20). Only rows that were visible have an answer.
pub(crate) fn name_truncated(view: &TaskManagerView, key: &RowKey) -> bool {
    view.measure.truncated.contains(key)
}

// ---------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------

fn with_alpha(color: Color, alpha: f64) -> Color {
    Color { a: alpha, ..color }
}

fn quad(out: &mut Vec<Primitive>, rect: Rect, color: Color) {
    if rect.width > 0.0 && rect.height > 0.0 {
        out.push(Primitive::Quad(Quad { rect, color }));
    }
}

/// A run of text whose em box is centered on the vertical of `rect` (the
/// render centers the glyphs in the em box, so the geometry does not
/// compensate, CLAUDE.md).
fn text_run(
    text: String,
    x: f32,
    rect: Rect,
    font: FontFace,
    size: f32,
    color: Color,
) -> Primitive {
    Primitive::Text(TextRun {
        origin: (x, rect.y + (rect.height - size) / 2.0),
        text,
        font,
        size_px: size,
        color,
    })
}

/// A number in the mono face, right-aligned at `right`: its width is the
/// glyph count times the cached advance -- no shaping per cell.
fn number_run(
    text: String,
    right: f32,
    rect: Rect,
    size: f32,
    mono_advance: f32,
    color: Color,
) -> Primitive {
    let width = text.chars().count() as f32 * mono_advance;
    text_run(text, right - width, rect, NUMBER_FONT, size, color)
}

/// ADR-0096 §1 to §5: paints the task manager over the frame's background,
/// on the layer `out` belongs to (`Layer::Grid`).
///
/// - `rect`: the terminal box (`paint::terminal_box_rect`), logical pixels;
///   the geometry is [`layout`]'s.
/// - `view`: the mode state; the painting reads `selection`, `hover` and
///   `first_visible`, and fills the measurement caches of `view.measure`
///   (hence `&mut`).
/// - `rows`: the list (`rows::build_rows`), in order; only
///   `rows[view.first_visible..]` up to the rows that fit are measured and
///   painted. Clamping `first_visible` is the caller's.
/// - `history` / `history_seconds`: the chart history and the effective
///   period (already clamped, `effective_history`).
/// - `background`: `[task_manager] background_monitoring` of the current
///   `Config`, the state the footer's toggle draws (ADR-0098 §4).
/// - `pal`: the theme colors and the group palette (tab rows' dots).
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint(
    out: &mut Vec<Primitive>,
    rect: Rect,
    view: &mut TaskManagerView,
    rows: &[Row],
    history: &TaskManagerHistory,
    history_seconds: u32,
    background: bool,
    style: &TaskManagerStyle,
    pal: &ResolvedPalette,
    catalog: &Catalog,
    measurer: &mut TextMeasurer,
) {
    let geometry = layout(rect, style);
    let (widths, mono_advance) = {
        let cache = view.measure.ensure_columns(style, catalog, measurer);
        (cache.widths, cache.mono_advance)
    };

    for (metric, chart_layout) in Metric::ALL.into_iter().zip(&geometry.charts) {
        paint_chart(
            out,
            chart_layout,
            metric,
            history,
            history_seconds,
            style,
            pal,
            catalog,
            mono_advance,
        );
    }

    let columns = Columns::new(geometry.list.column, widths, style.row_padding_x);
    if let Some(cache) = &view.measure.columns {
        paint_list_header(
            out,
            &geometry.list,
            &columns,
            &cache.headers,
            &cache.header_widths,
            style,
            pal,
        );
    }
    paint_rows(
        out,
        &geometry.list,
        &columns,
        view,
        rows,
        style,
        pal,
        catalog,
        mono_advance,
        measurer,
    );
    paint_footer(
        out,
        &geometry.list,
        &mut view.measure,
        background,
        style,
        pal,
        catalog,
        measurer,
    );
}

/// ADR-0098 §5: the rule, then the label right-aligned before the toggle
/// (system font, `text_size`, Secondary text; cut with an ellipsis when the
/// column is narrow), then the toggle in the settings screen's colors,
/// showing `on`. No hover of its own, like the settings screen's toggles.
#[allow(clippy::too_many_arguments)]
fn paint_footer(
    out: &mut Vec<Primitive>,
    list: &ListLayout,
    measure: &mut MeasureCache,
    on: bool,
    style: &TaskManagerStyle,
    pal: &ResolvedPalette,
    catalog: &Catalog,
    measurer: &mut TextMeasurer,
) {
    if list.footer.height <= 0.0 || list.footer.width <= 0.0 {
        measure.footer_label_width = 0.0;
        return;
    }
    quad(out, list.footer_rule, pal.task_manager_chart_border);
    let label = msg::task_manager::background_monitoring(catalog);
    let budget = list.footer_label.width;
    measure.footer_label_width = 0.0;
    if budget > 0.0 {
        let (text, cut) = measure
            .footer
            .fit(&label, BODY_FONT, style.text_size, budget, measurer);
        if !text.is_empty() {
            let width = if cut {
                budget
            } else {
                measure
                    .footer
                    .width_of(&label)
                    .unwrap_or(budget)
                    .min(budget)
            };
            let x = list.footer_label.x + budget - width;
            out.push(Primitive::PushClip(list.footer_label));
            out.push(text_run(
                text,
                x,
                list.footer,
                BODY_FONT,
                style.text_size,
                pal.status_bar_text,
            ));
            out.push(Primitive::PopClip);
            measure.footer_label_width = width;
        }
    }
    push_toggle(
        list.toggle,
        on,
        crate::settings::TOGGLE_ON,
        crate::settings::TOGGLE_OFF,
        out,
    );
}

/// One chart block (ADR-0096 §2, §3).
#[allow(clippy::too_many_arguments)]
fn paint_chart(
    out: &mut Vec<Primitive>,
    chart_layout: &ChartLayout,
    metric: Metric,
    history: &TaskManagerHistory,
    history_seconds: u32,
    style: &TaskManagerStyle,
    pal: &ResolvedPalette,
    catalog: &Catalog,
    mono_advance: f32,
) {
    let ChartLayout {
        header,
        frame,
        inner,
    } = *chart_layout;
    let size = style.text_size;

    // Header: the title on the left, the current value on the right.
    out.push(text_run(
        format::chart_title(catalog, metric),
        header.x,
        header,
        TITLE_MEDIUM,
        size,
        pal.menu_item_text,
    ));
    let current = history.latest().map(|point| match metric {
        Metric::Cpu => format::percent(catalog, point.cpu),
        Metric::Memory => format::bytes(catalog, point.memory),
        Metric::Disk => format::rate(catalog, point.disk_per_sec),
    });
    out.push(number_run(
        current.unwrap_or_else(|| msg::task_manager::no_value(catalog)),
        header.x + header.width,
        header,
        size,
        mono_advance,
        pal.status_bar_text,
    ));

    // The frame: four 1 px quads (no radius, transparent fill: the frame's
    // background shows through). The sides skip the corners the top and the
    // bottom already own, so a translucent border does not double there.
    let border = pal.task_manager_chart_border;
    if frame.width > 0.0 && frame.height > 0.0 {
        let hair = HAIRLINE.min(frame.height);
        quad(
            out,
            Rect {
                height: hair,
                ..frame
            },
            border,
        );
        if frame.height > hair {
            quad(
                out,
                Rect {
                    y: frame.y + frame.height - hair,
                    height: hair,
                    ..frame
                },
                border,
            );
        }
        let side_height = (frame.height - hair * 2.0).max(0.0);
        let side = Rect {
            y: frame.y + hair,
            width: HAIRLINE.min(frame.width),
            height: side_height,
            ..frame
        };
        quad(out, side, border);
        if frame.width > HAIRLINE {
            quad(
                out,
                Rect {
                    x: frame.x + frame.width - HAIRLINE,
                    ..side
                },
                border,
            );
        }
    }
    if inner.width <= 0.0 || inner.height <= 0.0 {
        return;
    }

    // Everything inside the frame, clipped to it, in the order of ADR-0096
    // §3: horizontal grid, vertical grid, areas, lines.
    out.push(Primitive::PushClip(inner));
    let grid = pal.task_manager_grid;
    for y in chart::horizontal_grid(inner) {
        quad(
            out,
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: HAIRLINE,
            },
            grid,
        );
    }
    for x in chart::vertical_grid(inner, history_seconds) {
        quad(
            out,
            Rect {
                x,
                y: inner.y,
                width: HAIRLINE,
                height: inner.height,
            },
            grid,
        );
    }

    let top = chart::scale_top(history, metric, history_seconds);
    // A run of fewer than two points draws neither area nor line.
    let runs: Vec<Vec<Point>> = chart::series(history, metric, inner, top, history_seconds)
        .into_iter()
        .filter(|run| run.len() >= 2)
        .map(|run| run.into_iter().map(|(x, y)| Point { x, y }).collect())
        .collect();
    let color = series_color(metric, pal);
    let baseline_y = inner.y + inner.height;
    for run in &runs {
        out.push(Primitive::Area(Area {
            points: run.clone(),
            baseline_y,
            color: with_alpha(color, color.a * style.fill_opacity),
        }));
    }
    for run in runs {
        out.push(Primitive::Polyline(Polyline {
            points: run,
            width: style.line_width,
            // The contract of `Polyline` (ADR-0094 §2): opaque.
            color: with_alpha(color, 1.0),
        }));
    }
    out.push(Primitive::PopClip);

    // The top of the scale, inside the frame, upper-left corner.
    out.push(Primitive::PushClip(inner));
    out.push(Primitive::Text(TextRun {
        origin: (frame.x + style.row_padding_x, frame.y + style.row_padding_x),
        text: format::top_label(catalog, metric, top),
        font: NUMBER_FONT,
        size_px: style.label_size,
        color: pal.editor_section_text,
    }));
    out.push(Primitive::PopClip);
}

/// The column labels and the rule under them (ADR-0096 §4).
fn paint_list_header(
    out: &mut Vec<Primitive>,
    list: &ListLayout,
    columns: &Columns,
    headers: &ColumnHeaders,
    header_widths: &[f32; 5],
    style: &TaskManagerStyle,
    pal: &ResolvedPalette,
) {
    let color = pal.editor_section_text;
    let size = style.label_size;
    out.push(Primitive::PushClip(list.header));
    out.push(text_run(
        headers.name.clone(),
        columns.name_left,
        list.header,
        TITLE_FONT,
        size,
        color,
    ));
    for (text, right, width) in [
        (&headers.pid, columns.pid_right, header_widths[1]),
        (&headers.cpu, columns.cpu_right, header_widths[2]),
        (&headers.memory, columns.memory_right, header_widths[3]),
        (&headers.disk, columns.disk_right, header_widths[4]),
    ] {
        out.push(text_run(
            text.clone(),
            right - width,
            list.header,
            TITLE_FONT,
            size,
            color,
        ));
    }
    out.push(Primitive::PopClip);
    quad(out, list.rule, pal.task_manager_chart_border);
}

/// The visible rows (ADR-0096 §4), under the clip of the body.
#[allow(clippy::too_many_arguments)]
fn paint_rows(
    out: &mut Vec<Primitive>,
    list: &ListLayout,
    columns: &Columns,
    view: &mut TaskManagerView,
    rows: &[Row],
    style: &TaskManagerStyle,
    pal: &ResolvedPalette,
    catalog: &Catalog,
    mono_advance: f32,
    measurer: &mut TextMeasurer,
) {
    let TaskManagerView {
        selection,
        hover,
        first_visible,
        measure,
        ..
    } = view;
    measure.truncated.clear();
    if list.visible_rows == 0 {
        return;
    }

    // The selection is the background of an active tab, opaque: the tab
    // colors carry the bar's `background_alpha`, which the list does not
    // want (ADR-0096 §4 names `#282e37`, the RGB of `active_background`).
    let selection_background = with_alpha(pal.tab_active_background, 1.0);
    let text = pal.menu_item_text;
    let number = pal.status_bar_text;
    let minimum = pal.editor_section_text;
    let size = style.text_size;

    out.push(Primitive::PushClip(list.body));
    for (slot, row) in rows
        .iter()
        .skip(*first_visible)
        .take(list.visible_rows)
        .enumerate()
    {
        let rect = list.row_rect(slot);
        let background = if selection.as_ref() == Some(&row.key) {
            Some(selection_background)
        } else if hover.as_ref() == Some(&row.key) {
            Some(pal.menu_item_hover)
        } else {
            None
        };
        if let Some(color) = background {
            out.push(Primitive::RoundedQuad(RoundedQuad {
                rect,
                radius: style.row_radius,
                color,
                border_color: palette::TRANSPARENT,
                border_width: 0.0,
            }));
        }

        let left = columns.name_left;
        match &row.kind {
            RowKind::Tab {
                title,
                group_color,
                totals,
                ..
            } => {
                let mut x = left;
                if let Some(group) = group_color {
                    out.push(Primitive::RoundedQuad(RoundedQuad {
                        rect: Rect {
                            x,
                            y: rect.y + (rect.height - style.dot_size) / 2.0,
                            width: style.dot_size,
                            height: style.dot_size,
                        },
                        radius: style.dot_size / 2.0,
                        color: pal.group_color(*group),
                        border_color: palette::TRANSPARENT,
                        border_width: 0.0,
                    }));
                    x += style.dot_size + style.dot_gap;
                }
                push_name(
                    out,
                    &mut measure.tab_titles,
                    &mut measure.truncated,
                    (&row.key, title),
                    (x, columns.name_right),
                    rect,
                    (TITLE_MEDIUM, size, text),
                    measurer,
                );
                for (value, right) in [
                    (format::percent(catalog, totals.cpu), columns.cpu_right),
                    (format::bytes(catalog, totals.memory), columns.memory_right),
                    (
                        format::rate(catalog, totals.disk_per_sec),
                        columns.disk_right,
                    ),
                ] {
                    out.push(number_run(value, right, rect, size, mono_advance, number));
                }
            }
            RowKind::Process { stat, depth, .. } => {
                let x = left + style.tree_indent * *depth as f32;
                push_name(
                    out,
                    &mut measure.process_names,
                    &mut measure.truncated,
                    (&row.key, &stat.name),
                    (x, columns.name_right),
                    rect,
                    (BODY_FONT, size, text),
                    measurer,
                );
                for (value, right) in [
                    (stat.pid.to_string(), columns.pid_right),
                    (format::percent(catalog, stat.cpu), columns.cpu_right),
                    (format::bytes(catalog, stat.memory), columns.memory_right),
                    (format::rate(catalog, stat.disk_per_sec), columns.disk_right),
                ] {
                    out.push(number_run(value, right, rect, size, mono_advance, number));
                }
            }
            RowKind::Note { kind, depth } => {
                let x = left + style.tree_indent * *depth as f32;
                push_name(
                    out,
                    &mut measure.notes,
                    &mut measure.truncated,
                    (&row.key, &format::note_text(catalog, *kind)),
                    (x, columns.name_right),
                    rect,
                    (BODY_FONT, style.note_size, minimum),
                    measurer,
                );
            }
        }
    }
    out.push(Primitive::PopClip);
}

/// Pushes the name of a row at `x`, fitted to the name area, and records
/// whether it was cut.
#[allow(clippy::too_many_arguments)]
fn push_name(
    out: &mut Vec<Primitive>,
    cache: &mut NameCache,
    truncated: &mut HashSet<RowKey>,
    (key, name): (&RowKey, &str),
    (x, name_right): (f32, f32),
    rect: Rect,
    (font, size, color): (FontFace, f32, Color),
    measurer: &mut TextMeasurer,
) {
    let budget = (name_right - x).max(0.0);
    let (text, cut) = cache.fit(name, font, size, budget, measurer);
    if cut {
        truncated.insert(*key);
    }
    // A name that shrank to nothing paints nothing (the numbers never give
    // way, the name does).
    if budget > 0.0 && !text.is_empty() {
        out.push(text_run(text, x, rect, font, size, color));
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use porecatu_core::{GroupColor, PaneId, TabId};
    use porecatu_term::ProcessStat;

    use super::*;
    use crate::messages::test_support::en_us;
    use crate::task_manager::rows::NoteKind;
    use crate::task_manager::{HistoryPoint, Totals};

    fn rect() -> Rect {
        Rect {
            x: 10.0,
            y: 40.0,
            width: 1000.0,
            height: 600.0,
        }
    }

    fn pal() -> ResolvedPalette {
        ResolvedPalette::from_config(&Config::default())
    }

    fn style() -> TaskManagerStyle {
        TaskManagerStyle::from_config(&Config::default(), &TabBarStyle::DEFAULT)
    }

    fn tab_row(n: u32, title: &str, group: Option<GroupColor>) -> Row {
        Row {
            key: RowKey::Tab(TabId::new(n)),
            kind: RowKind::Tab {
                tab: TabId::new(n),
                title: title.to_owned(),
                group_color: group,
                totals: Totals {
                    cpu: Some(0.25),
                    memory: 3 * 1024 * 1024,
                    disk_per_sec: 2048.0,
                },
            },
        }
    }

    fn process_row(pid: u32, name: &str, depth: usize) -> Row {
        Row {
            key: RowKey::Process {
                tab: TabId::new(1),
                pane: PaneId::new(0),
                pid,
                start_time: 1,
            },
            kind: RowKind::Process {
                stat: ProcessStat {
                    pid,
                    parent: None,
                    start_time: 1,
                    name: name.to_owned(),
                    cpu: Some(0.5),
                    memory: 100 * 1024 * 1024,
                    disk_per_sec: 0.0,
                },
                depth,
                is_root: depth == 1,
                descendants: 0,
            },
        }
    }

    fn paint_all(
        view: &mut TaskManagerView,
        rows: &[Row],
        history: &TaskManagerHistory,
        measurer: &mut TextMeasurer,
    ) -> Vec<Primitive> {
        let mut out = Vec::new();
        paint(
            &mut out,
            rect(),
            view,
            rows,
            history,
            60,
            false,
            &style(),
            &pal(),
            &en_us(),
            measurer,
        );
        out
    }

    fn texts(out: &[Primitive]) -> Vec<&TextRun> {
        out.iter()
            .filter_map(|p| match p {
                Primitive::Text(run) => Some(run),
                _ => None,
            })
            .collect()
    }

    // ---- layout

    #[test]
    fn the_columns_are_equal_and_padding_and_gaps_are_respected() {
        let s = style();
        let g = layout(rect(), &s);
        let r = rect();
        // Padding on the four sides.
        assert_eq!(g.charts[0].header.x, r.x + s.padding);
        assert_eq!(g.charts[0].header.y, r.y + s.padding);
        let list_right = g.list.column.x + g.list.column.width;
        assert_eq!(list_right, r.x + r.width - s.padding);
        assert_eq!(
            g.list.column.y + g.list.column.height,
            r.y + r.height - s.padding
        );
        // Two equal columns with the gap between them.
        assert_eq!(g.charts[0].header.width, g.list.column.width);
        let left_right = g.charts[0].header.x + g.charts[0].header.width;
        assert_eq!(g.list.column.x - left_right, s.column_gap);
    }

    #[test]
    fn the_three_charts_have_equal_height_separated_by_the_chart_gap() {
        let s = style();
        let g = layout(rect(), &s);
        let block = |c: &ChartLayout| c.header.height + c.frame.height;
        assert_eq!(block(&g.charts[0]), block(&g.charts[1]));
        assert_eq!(block(&g.charts[1]), block(&g.charts[2]));
        for pair in g.charts.windows(2) {
            let bottom = pair[0].frame.y + pair[0].frame.height;
            assert!((pair[1].header.y - bottom - s.chart_gap).abs() < 1e-3);
        }
        // The blocks fill the column exactly.
        let last = &g.charts[2];
        assert_eq!(
            last.frame.y + last.frame.height,
            g.list.column.y + g.list.column.height
        );
        // The header is `row_height`; the inner frame is the frame minus the
        // 1 px border.
        assert_eq!(g.charts[0].header.height, s.row_height);
        let f = g.charts[0].frame;
        let i = g.charts[0].inner;
        assert_eq!((i.x, i.y), (f.x + 1.0, f.y + 1.0));
        assert_eq!((i.width, i.height), (f.width - 2.0, f.height - 2.0));
    }

    #[test]
    fn the_list_has_a_header_a_rule_and_whole_visible_rows() {
        let s = style();
        let g = layout(rect(), &s);
        let l = g.list;
        assert_eq!(l.header.height, s.row_height);
        assert_eq!(l.rule.y, l.header.y + l.header.height);
        assert_eq!(l.rule.height, 1.0);
        assert_eq!(l.body.y, l.rule.y + 1.0);
        // 600 - 2 * 18 = 564 of column; minus 28 and 1 of header, 1 and 28
        // of footer (ADR-0098 §5) = 506 of body; 18 rows.
        assert_eq!(l.body.height, 506.0);
        assert_eq!(l.visible_rows, 18);
        assert_eq!(l.row_rect(0).y, l.body.y);
        assert_eq!(l.row_rect(1).y, l.body.y + 28.0);
        // Hit-test: the first row, the last whole row, and below it.
        let x = l.body.x + 5.0;
        assert_eq!(l.row_at((x, l.body.y + 1.0)), Some(0));
        assert_eq!(l.row_at((x, l.body.y + 28.0 * 18.0 - 1.0)), Some(17));
        assert_eq!(l.row_at((x, l.body.y + 28.0 * 18.0 + 1.0)), None);
        assert_eq!(l.row_at((l.body.x - 1.0, l.body.y + 1.0)), None);
        assert_eq!(l.row_at((x, l.header.y + 1.0)), None);
    }

    #[test]
    fn the_footer_ends_the_column_with_a_rule_and_the_toggle_on_the_right() {
        // ADR-0098 §5: the list shrinks by one row; the footer is
        // `row_height` tall under a 1 px rule; the toggle sits
        // `row_padding_x` from the right edge, centered; the label's room
        // ends `row_padding_x` before it.
        let s = style();
        let l = layout(rect(), &s).list;
        let column_bottom = l.column.y + l.column.height;
        assert_eq!(l.footer.height, s.row_height);
        assert_eq!(l.footer.y + l.footer.height, column_bottom);
        assert_eq!((l.footer.x, l.footer.width), (l.column.x, l.column.width));
        assert_eq!(l.footer_rule.height, 1.0);
        assert_eq!(l.footer_rule.y + 1.0, l.footer.y);
        assert_eq!(l.body.y + l.body.height, l.footer_rule.y);
        assert_eq!(
            (l.toggle.width, l.toggle.height),
            (TOGGLE_TRACK_WIDTH, TOGGLE_TRACK_HEIGHT)
        );
        assert_eq!(
            l.toggle.x + l.toggle.width,
            l.column.x + l.column.width - s.row_padding_x
        );
        assert_eq!(
            l.toggle.y - l.footer.y,
            l.footer.y + l.footer.height - (l.toggle.y + l.toggle.height)
        );
        assert_eq!(l.footer_label.x, l.column.x + s.row_padding_x);
        assert_eq!(
            l.footer_label.x + l.footer_label.width,
            l.toggle.x - s.row_padding_x
        );
        // A row slot never reaches the footer.
        let x = l.body.x + 5.0;
        assert_eq!(l.row_at((x, l.footer.y + 1.0)), None);
    }

    #[test]
    fn the_footer_hit_is_the_painted_label_plus_the_track() {
        let l = layout(rect(), &style()).list;
        let y = l.footer.y + l.footer.height / 2.0;
        let label_right = l.footer_label.x + l.footer_label.width;
        // No label painted yet: only the track.
        assert!(l.footer_contains((l.toggle.x + 1.0, y), 0.0));
        assert!(!l.footer_contains((l.toggle.x - 1.0, y), 0.0));
        // A 100 px label: from its start to the track's right edge.
        assert!(l.footer_contains((label_right - 99.0, y), 100.0));
        assert!(!l.footer_contains((label_right - 101.0, y), 100.0));
        assert!(!l.footer_contains((l.toggle.x + l.toggle.width + 1.0, y), 100.0));
        // Above the footer (the last row), nothing.
        assert!(!l.footer_contains((l.toggle.x + 1.0, l.footer.y - 1.0), 100.0));
    }

    #[test]
    fn in_a_narrow_column_the_toggle_is_never_cut_and_the_label_has_no_room() {
        let narrow = Rect {
            x: 0.0,
            y: 0.0,
            width: 2.0 * 18.0 + 18.0 + 2.0 * 40.0,
            height: 300.0,
        };
        let l = layout(narrow, &style()).list;
        assert_eq!(l.column.width, 40.0);
        assert_eq!(l.toggle.width, TOGGLE_TRACK_WIDTH);
        assert!(l.toggle.x >= l.column.x);
        assert_eq!(l.footer_label.width, 0.0);
    }

    #[test]
    fn the_footer_paints_the_rule_the_label_and_the_toggle_in_its_state() {
        let mut measurer = TextMeasurer::new();
        let history = TaskManagerHistory::default();
        let l = layout(rect(), &style()).list;
        for on in [false, true] {
            let mut view = TaskManagerView::new(None);
            let mut out = Vec::new();
            paint(
                &mut out,
                rect(),
                &mut view,
                &[],
                &history,
                60,
                on,
                &style(),
                &pal(),
                &en_us(),
                &mut measurer,
            );
            assert!(out.iter().any(|p| matches!(
                p,
                Primitive::Quad(q) if q.rect == l.footer_rule
            )));
            let label = texts(&out)
                .into_iter()
                .find(|run| run.text == "Monitor in the background")
                .expect("the footer label");
            assert_eq!(label.font, BODY_FONT);
            assert_eq!(label.color, pal().status_bar_text);
            let track = out
                .iter()
                .find_map(|p| match p {
                    Primitive::RoundedQuad(q) if q.rect == l.toggle => Some(*q),
                    _ => None,
                })
                .expect("the toggle track");
            let expected = if on {
                crate::settings::TOGGLE_ON
            } else {
                crate::settings::TOGGLE_OFF
            };
            assert_eq!(track.color, expected);
            // The painted width is what the hit-test reads.
            let width = view.measure.footer_label_width();
            assert!(width > 0.0);
            let label_right = l.footer_label.x + l.footer_label.width;
            assert!((label.origin.0 - (label_right - width)).abs() < 1e-3);
        }
    }

    #[test]
    fn a_tiny_rectangle_lays_out_without_negative_sizes() {
        let g = layout(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            &style(),
        );
        for chart in &g.charts {
            for r in [chart.header, chart.frame, chart.inner] {
                assert!(r.width >= 0.0 && r.height >= 0.0);
            }
        }
        assert_eq!(g.list.visible_rows, 0);
        assert_eq!(g.list.row_at((5.0, 5.0)), None);
    }

    #[test]
    fn the_columns_are_laid_from_the_right_and_the_name_takes_the_rest() {
        let column = Rect {
            x: 500.0,
            y: 0.0,
            width: 400.0,
            height: 100.0,
        };
        let widths = ColumnWidths {
            pid: 40.0,
            cpu: 30.0,
            memory: 60.0,
            disk: 70.0,
        };
        let c = Columns::new(column, widths, 8.0);
        assert_eq!(c.disk_right, 892.0);
        assert_eq!(c.memory_right, 892.0 - 70.0 - 8.0);
        assert_eq!(c.cpu_right, c.memory_right - 60.0 - 8.0);
        assert_eq!(c.pid_right, c.cpu_right - 30.0 - 8.0);
        assert_eq!(c.name_left, 508.0);
        assert_eq!(c.name_right, c.pid_right - 40.0 - 8.0);
        // A column too narrow shrinks the name to nothing, never the numbers.
        let narrow = Columns::new(
            Rect {
                width: 100.0,
                ..column
            },
            widths,
            8.0,
        );
        assert_eq!(narrow.name_right, narrow.name_left);
        assert_eq!(narrow.disk_right, 592.0);
    }

    // ---- charts

    fn two_run_history() -> TaskManagerHistory {
        // Runs at 0..=5 s, 10..=15 s (a gap in between) and a lone point at
        // 25 s: two runs with a line, one that draws nothing.
        let base = Instant::now();
        let mut history = TaskManagerHistory::default();
        for secs in (0..=5).chain(10..=15).chain([25]) {
            history.push(
                HistoryPoint {
                    at: base + Duration::from_secs(secs),
                    cpu: Some(0.4),
                    memory: 5 * 1024 * 1024,
                    disk_per_sec: 1024.0,
                },
                600,
            );
        }
        history
    }

    /// The primitives between the `PushClip(inner)` and its `PopClip` of the
    /// chart `index`.
    fn chart_clip(out: &[Primitive], index: usize) -> Vec<&Primitive> {
        let inner = layout(rect(), &style()).charts[index].inner;
        let start = out
            .iter()
            .position(|p| matches!(p, Primitive::PushClip(r) if *r == inner))
            .expect("the chart clips to its inner frame");
        let end = start
            + out[start..]
                .iter()
                .position(|p| matches!(p, Primitive::PopClip))
                .expect("and pops it");
        out[start + 1..end].iter().collect()
    }

    #[test]
    fn a_chart_paints_grid_then_areas_then_lines_inside_one_clip() {
        let mut m = TextMeasurer::new();
        let mut view = TaskManagerView::new(None);
        let out = paint_all(&mut view, &[], &two_run_history(), &mut m);
        let body = chart_clip(&out, 0);

        // Kinds in order: quads (grid), areas, polylines -- never mixed back.
        let rank = |p: &&Primitive| match p {
            Primitive::Quad(_) => 0,
            Primitive::Area(_) => 1,
            Primitive::Polyline(_) => 2,
            other => panic!("unexpected {other:?} inside the chart clip"),
        };
        let ranks: Vec<u8> = body.iter().map(rank).collect();
        assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "{ranks:?}");

        let quads: Vec<&Quad> = body
            .iter()
            .filter_map(|p| match p {
                Primitive::Quad(q) => Some(q),
                _ => None,
            })
            .collect();
        // 3 horizontal lines first, then 5 vertical (60 s), all the grid color.
        assert_eq!(quads.len(), 3 + 5);
        let inner = layout(rect(), &style()).charts[0].inner;
        assert!(quads[..3].iter().all(|q| q.rect.height == 1.0));
        assert!(quads[..3].iter().all(|q| q.rect.width == inner.width));
        assert!(quads[3..].iter().all(|q| q.rect.width == 1.0));
        assert!(quads[3..].iter().all(|q| q.rect.height == inner.height));
        assert!(quads.iter().all(|q| q.color == pal().task_manager_grid));

        let areas: Vec<&Area> = body
            .iter()
            .filter_map(|p| match p {
                Primitive::Area(a) => Some(a),
                _ => None,
            })
            .collect();
        let lines: Vec<&Polyline> = body
            .iter()
            .filter_map(|p| match p {
                Primitive::Polyline(l) => Some(l),
                _ => None,
            })
            .collect();
        // The lone point draws neither.
        assert_eq!(areas.len(), 2);
        assert_eq!(lines.len(), 2);
        assert_eq!(areas[0].points.len(), 6);
        assert_eq!(areas[1].points.len(), 6);
        assert_eq!(lines[0].points, areas[0].points);
    }

    #[test]
    fn the_area_is_the_series_color_at_the_fill_opacity_and_the_line_is_opaque() {
        let mut m = TextMeasurer::new();
        let mut view = TaskManagerView::new(None);
        let out = paint_all(&mut view, &[], &two_run_history(), &mut m);
        let pal = pal();
        let s = style();
        for (index, color) in [
            pal.task_manager_cpu,
            pal.task_manager_memory,
            pal.task_manager_disk,
        ]
        .into_iter()
        .enumerate()
        {
            for p in chart_clip(&out, index) {
                match p {
                    Primitive::Area(a) => {
                        assert_eq!(
                            (a.color.r, a.color.g, a.color.b),
                            (color.r, color.g, color.b)
                        );
                        assert!((a.color.a - color.a * 0.15).abs() < 1e-9);
                        let inner = layout(rect(), &s).charts[index].inner;
                        assert_eq!(a.baseline_y, inner.y + inner.height);
                    }
                    Primitive::Polyline(l) => {
                        assert_eq!(l.color.a, 1.0);
                        assert_eq!(
                            (l.color.r, l.color.g, l.color.b),
                            (color.r, color.g, color.b)
                        );
                        assert_eq!(l.width, 1.5);
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn the_chart_header_has_the_title_the_current_value_and_the_scale_label() {
        let mut m = TextMeasurer::new();
        let mut view = TaskManagerView::new(None);
        let out = paint_all(&mut view, &[], &two_run_history(), &mut m);
        let all = texts(&out);
        let has = |text: &str| all.iter().any(|run| run.text == text);
        // Titles, the latest values (40% CPU, 5.0 MB, 1.0 KB/s) and the tops
        // of the scales (100%, 5 MB, 1 KB/s).
        assert!(has("CPU") && has("Memory") && has("Disk"));
        assert!(has("40%") && has("5.0 MB") && has("1.0 KB/s"));
        assert!(has("100%") && has("5 MB") && has("1 KB/s"));
        let title = all.iter().find(|r| r.text == "CPU").unwrap();
        assert_eq!(
            title.font,
            FontFace::System {
                weight: SansWeight::Medium
            }
        );
        assert_eq!(title.color, pal().menu_item_text);
        let value = all.iter().find(|r| r.text == "40%").unwrap();
        assert_eq!(value.font, FontFace::Mono { bold: false });
        assert_eq!(value.color, pal().status_bar_text);
        // Right-aligned to the header edge.
        let header = layout(rect(), &style()).charts[0].header;
        let adv = m.advance_em('M', NUMBER_FONT) * style().text_size;
        assert!((value.origin.0 + 3.0 * adv - (header.x + header.width)).abs() < 0.05);
    }

    #[test]
    fn an_empty_history_paints_the_dash_and_no_series() {
        let mut m = TextMeasurer::new();
        let mut view = TaskManagerView::new(None);
        let out = paint_all(&mut view, &[], &TaskManagerHistory::default(), &mut m);
        assert!(
            !out.iter()
                .any(|p| matches!(p, Primitive::Area(_) | Primitive::Polyline(_)))
        );
        assert_eq!(texts(&out).iter().filter(|r| r.text == "—").count(), 3);
    }

    #[test]
    fn the_frame_border_is_four_hairlines_without_radius() {
        let mut m = TextMeasurer::new();
        let mut view = TaskManagerView::new(None);
        let out = paint_all(&mut view, &[], &TaskManagerHistory::default(), &mut m);
        let frame = layout(rect(), &style()).charts[0].frame;
        let border = pal().task_manager_chart_border;
        let edges: Vec<Rect> = out
            .iter()
            .filter_map(|p| match p {
                Primitive::Quad(q) if q.color == border => Some(q.rect),
                _ => None,
            })
            .filter(|r| {
                r.x >= frame.x
                    && r.x + r.width <= frame.x + frame.width
                    && r.y >= frame.y
                    && r.y + r.height <= frame.y + frame.height
            })
            .collect();
        assert_eq!(edges.len(), 4);
        let covered: f32 = edges.iter().map(|r| r.width * r.height).sum();
        // The ring of 1 px: no overlap, no gap.
        let ring = frame.width * frame.height - (frame.width - 2.0) * (frame.height - 2.0);
        assert!((covered - ring).abs() < 0.05, "{covered} vs {ring}");
        // The charts have no rounded corner (the footer's toggle is the
        // list's, ADR-0098 §5).
        let footer = layout(rect(), &style()).list.footer;
        assert!(!out.iter().any(|p| matches!(
            p,
            Primitive::RoundedQuad(q) if q.rect.y < footer.y
        )));
    }

    // ---- list

    /// The rounded quads above the footer: the rows' (and the dots'), without
    /// the footer's toggle (ADR-0098 §5).
    fn rows_rounded(out: &[Primitive]) -> Vec<RoundedQuad> {
        let footer = layout(rect(), &style()).list.footer;
        out.iter()
            .filter_map(|p| match p {
                Primitive::RoundedQuad(q) if q.rect.y < footer.y => Some(*q),
                _ => None,
            })
            .collect()
    }

    fn many_tabs(count: u32) -> Vec<Row> {
        (0..count)
            .map(|n| tab_row(n + 1, &format!("tab-{n}"), None))
            .collect()
    }

    #[test]
    fn only_the_visible_rows_generate_text() {
        let mut m = TextMeasurer::new();
        let mut view = TaskManagerView::new(None);
        view.first_visible = 10;
        let rows = many_tabs(100);
        let out = paint_all(&mut view, &rows, &TaskManagerHistory::default(), &mut m);
        let titles: Vec<&str> = texts(&out)
            .iter()
            .map(|r| r.text.as_str())
            .filter(|t| t.starts_with("tab-"))
            .collect();
        let expected: Vec<String> = (10..10 + 18).map(|n| format!("tab-{n}")).collect();
        assert_eq!(
            titles,
            expected.iter().map(String::as_str).collect::<Vec<_>>()
        );
        // The rows are under one clip of the body.
        let body = layout(rect(), &style()).list.body;
        assert!(
            out.iter()
                .any(|p| matches!(p, Primitive::PushClip(r) if *r == body))
        );
    }

    #[test]
    fn numeric_columns_are_right_aligned_on_the_same_edge() {
        let mut m = TextMeasurer::new();
        let mut view = TaskManagerView::new(None);
        let rows = vec![process_row(7, "a", 1), process_row(1234567, "long-name", 2)];
        let out = paint_all(&mut view, &rows, &TaskManagerHistory::default(), &mut m);
        let adv = m.advance_em('M', NUMBER_FONT) * style().text_size;
        let right_of = |text: &str| {
            let run = texts(&out)
                .into_iter()
                .find(|r| r.text == text)
                .unwrap_or_else(|| panic!("no run {text}"));
            assert_eq!(run.font, FontFace::Mono { bold: false });
            run.origin.0 + text.chars().count() as f32 * adv
        };
        assert!((right_of("7") - right_of("1234567")).abs() < 0.05);
        // Each column has its own right edge, PID the leftmost.
        let list = layout(rect(), &style()).list;
        let widths = {
            let cache = view.measure.columns.as_ref().unwrap();
            cache.widths
        };
        let columns = Columns::new(list.column, widths, style().row_padding_x);
        assert!((right_of("7") - columns.pid_right).abs() < 0.05);
        assert!((right_of("50%") - columns.cpu_right).abs() < 0.05);
        assert!((right_of("100.0 MB") - columns.memory_right).abs() < 0.05);
        assert!((right_of("0 B/s") - columns.disk_right).abs() < 0.05);
        assert!(columns.pid_right < columns.cpu_right);
        assert!(columns.cpu_right < columns.memory_right);
        assert!(columns.memory_right < columns.disk_right);
    }

    #[test]
    fn a_process_is_indented_by_depth_from_the_start_of_the_name() {
        let mut m = TextMeasurer::new();
        let mut view = TaskManagerView::new(None);
        let rows = vec![
            tab_row(1, "tab", None),
            process_row(2, "root", 1),
            process_row(3, "kid", 2),
        ];
        let out = paint_all(&mut view, &rows, &TaskManagerHistory::default(), &mut m);
        let x = |t: &str| {
            texts(&out)
                .into_iter()
                .find(|r| r.text == t)
                .unwrap()
                .origin
                .0
        };
        let s = style();
        assert_eq!(x("root") - x("tab"), s.tree_indent);
        assert_eq!(x("kid") - x("root"), s.tree_indent);
        let name = texts(&out).into_iter().find(|r| r.text == "root").unwrap();
        assert_eq!(
            name.font,
            FontFace::System {
                weight: SansWeight::Regular
            }
        );
        let tab = texts(&out).into_iter().find(|r| r.text == "tab").unwrap();
        assert_eq!(
            tab.font,
            FontFace::System {
                weight: SansWeight::Medium
            }
        );
    }

    #[test]
    fn a_note_is_small_minimum_text_at_the_level_one_indent() {
        let mut m = TextMeasurer::new();
        let mut view = TaskManagerView::new(None);
        let rows = vec![
            tab_row(1, "tab", None),
            Row {
                key: RowKey::Note(TabId::new(1), PaneId::new(0)),
                kind: RowKind::Note {
                    kind: NoteKind::Ssh,
                    depth: 1,
                },
            },
        ];
        let out = paint_all(&mut view, &rows, &TaskManagerHistory::default(), &mut m);
        let note = texts(&out)
            .into_iter()
            .find(|r| r.text == "Remote processes are not listed.")
            .unwrap();
        assert_eq!(note.size_px, 11.0);
        assert_eq!(note.color, pal().editor_section_text);
        let tab = texts(&out).into_iter().find(|r| r.text == "tab").unwrap();
        assert_eq!(note.origin.0 - tab.origin.0, style().tree_indent);
    }

    #[test]
    fn a_grouped_tab_has_a_dot_in_the_group_color_and_an_implicit_one_has_none() {
        let mut m = TextMeasurer::new();
        let rows = vec![
            tab_row(1, "grouped", Some(GroupColor::Blue)),
            tab_row(2, "implicit", None),
        ];
        let mut view = TaskManagerView::new(None);
        let out = paint_all(&mut view, &rows, &TaskManagerHistory::default(), &mut m);
        let dots = rows_rounded(&out);
        assert_eq!(dots.len(), 1);
        let dot = dots[0];
        assert_eq!((dot.rect.width, dot.rect.height), (6.0, 6.0));
        assert_eq!(dot.radius, 3.0);
        assert_eq!(dot.color, pal().group_color(GroupColor::Blue));
        // The title follows the dot; the implicit title starts at the edge.
        let x = |t: &str| {
            texts(&out)
                .into_iter()
                .find(|r| r.text == t)
                .unwrap()
                .origin
                .0
        };
        assert!(x("grouped") > x("implicit"));
    }

    fn background_quads(out: &[Primitive]) -> Vec<RoundedQuad> {
        rows_rounded(out)
    }

    #[test]
    fn selection_wins_over_hover_on_the_same_row() {
        let mut m = TextMeasurer::new();
        let rows = many_tabs(3);
        let pal = pal();

        let mut view = TaskManagerView::new(Some(rows[1].key));
        view.hover = Some(rows[1].key);
        let out = paint_all(&mut view, &rows, &TaskManagerHistory::default(), &mut m);
        let quads = background_quads(&out);
        assert_eq!(quads.len(), 1, "only the selection's");
        assert_eq!(quads[0].color, with_alpha(pal.tab_active_background, 1.0));
        assert_eq!(quads[0].radius, 5.0);
        assert_eq!(quads[0].border_width, 0.0);
        let list = layout(rect(), &style()).list;
        assert_eq!(quads[0].rect, list.row_rect(1));

        // Hover alone: the menu item hover color, same radius, another row.
        let mut view = TaskManagerView::new(Some(rows[1].key));
        view.hover = Some(rows[2].key);
        let out = paint_all(&mut view, &rows, &TaskManagerHistory::default(), &mut m);
        let quads = background_quads(&out);
        assert_eq!(quads.len(), 2);
        assert_eq!(quads[0].color, with_alpha(pal.tab_active_background, 1.0));
        assert_eq!(quads[1].color, pal.menu_item_hover);
        assert_eq!(quads[1].rect, list.row_rect(2));

        // Neither: no background at all.
        let mut view = TaskManagerView::new(None);
        let out = paint_all(&mut view, &rows, &TaskManagerHistory::default(), &mut m);
        assert!(background_quads(&out).is_empty());
    }

    #[test]
    fn the_header_has_upper_case_minimum_labels_and_a_rule() {
        let mut m = TextMeasurer::new();
        let mut view = TaskManagerView::new(None);
        let out = paint_all(&mut view, &[], &TaskManagerHistory::default(), &mut m);
        let all = texts(&out);
        for label in ["NAME", "PID", "CPU", "MEMORY", "DISK"] {
            let run = all
                .iter()
                .find(|r| r.text == label && r.size_px == 10.0)
                .unwrap_or_else(|| panic!("no header {label}"));
            assert_eq!(run.color, pal().editor_section_text);
        }
        let rule = layout(rect(), &style()).list.rule;
        assert!(out.iter().any(|p| matches!(
            p,
            Primitive::Quad(q) if q.rect == rule && q.color == pal().task_manager_chart_border
        )));
    }

    // ---- measurement

    #[test]
    fn the_columns_are_measured_once_per_size_and_language() {
        let mut m = TextMeasurer::new();
        let mut view = TaskManagerView::new(None);
        let rows = many_tabs(5);
        let history = TaskManagerHistory::default();
        paint_all(&mut view, &rows, &history, &mut m);
        assert_eq!(view.measure.column_measurements, 1);
        paint_all(&mut view, &rows, &history, &mut m);
        paint_all(&mut view, &rows, &history, &mut m);
        assert_eq!(view.measure.column_measurements, 1);

        // Another font size measures again, once.
        let mut bigger = style();
        bigger.text_size = 15.0;
        let mut out = Vec::new();
        for _ in 0..2 {
            paint(
                &mut out,
                rect(),
                &mut view,
                &rows,
                &history,
                60,
                false,
                &bigger,
                &pal(),
                &en_us(),
                &mut m,
            );
        }
        assert_eq!(view.measure.column_measurements, 2);

        // Another language measures again (the decimal separator moves the
        // models).
        let pt = crate::messages::test_support::pt_br();
        paint(
            &mut out,
            rect(),
            &mut view,
            &rows,
            &history,
            60,
            false,
            &bigger,
            &pal(),
            &pt,
            &mut m,
        );
        assert_eq!(view.measure.column_measurements, 3);

        // `clear` drops the name caches but keeps the diagnostic counter.
        view.measure.clear();
        assert_eq!(view.measure.column_measurements, 3);
        assert!(view.measure.columns.is_none());
    }

    #[test]
    fn a_long_name_is_cut_once_with_an_ellipsis_and_remembered_for_the_tooltip() {
        let mut m = TextMeasurer::new();
        let long = "a-very-long-process-name-that-cannot-possibly-fit-in-the-name-column.exe";
        let rows = vec![
            tab_row(1, "tab", None),
            process_row(2, long, 1),
            process_row(3, "ok", 1),
        ];
        let mut view = TaskManagerView::new(None);
        let out = paint_all(&mut view, &rows, &TaskManagerHistory::default(), &mut m);

        let cut = texts(&out)
            .into_iter()
            .find(|r| r.text.ends_with('…'))
            .expect("the long name is cut");
        assert!(long.starts_with(cut.text.trim_end_matches('…')));
        assert!(name_truncated(&view, &rows[1].key));
        assert!(!name_truncated(&view, &rows[2].key));
        assert!(!name_truncated(&view, &rows[0].key));

        // The cut fits the budget.
        let list = layout(rect(), &style()).list;
        let cols = Columns::new(
            list.column,
            view.measure.columns.as_ref().unwrap().widths,
            style().row_padding_x,
        );
        let width = m.measure_width(&cut.text, BODY_FONT, style().text_size);
        assert!(cut.origin.0 + width <= cols.name_right + 0.5);

        // A second frame reuses the cut: the string is in the cache with its
        // budget, and the answer is the same.
        let again = paint_all(&mut view, &rows, &TaskManagerHistory::default(), &mut m);
        assert!(texts(&again).iter().any(|r| r.text == cut.text));
        assert!(name_truncated(&view, &rows[1].key));
    }

    #[test]
    fn the_tooltip_of_a_row_is_only_for_a_cut_name_and_follows_the_scroll() {
        use crate::app_task_manager::row_tooltip;
        use crate::tooltip::HoverKey;

        let mut m = TextMeasurer::new();
        let long = "a-very-long-process-name-that-cannot-possibly-fit-in-the-name-column.exe";
        let rows = vec![
            tab_row(1, "tab", None),
            process_row(2, long, 1),
            process_row(3, "ok", 1),
        ];
        let mut view = TaskManagerView::new(None);
        paint_all(&mut view, &rows, &TaskManagerHistory::default(), &mut m);
        let list = layout(rect(), &style()).list;
        let center = |slot: usize| {
            let r = list.row_rect(slot);
            (r.x + r.width / 2.0, r.y + r.height / 2.0)
        };
        let en = en_us();

        let (key, anchor, text) =
            row_tooltip(&rows, &view, &list, center(1), &en).expect("the cut name has one");
        assert_eq!(key, HoverKey::TaskRow(rows[1].key));
        assert_eq!(anchor, list.row_rect(1));
        assert_eq!(text, long, "the whole name, not the cut one");

        // A name that fits has no tooltip, a tab row neither.
        assert!(row_tooltip(&rows, &view, &list, center(2), &en).is_none());
        assert!(row_tooltip(&rows, &view, &list, center(0), &en).is_none());
        // Nor does the space below the last row, nor the header.
        assert!(row_tooltip(&rows, &view, &list, center(5), &en).is_none());
        assert!(
            row_tooltip(
                &rows,
                &view,
                &list,
                (list.header.x + 4.0, list.header.y + 4.0),
                &en
            )
            .is_none()
        );

        // Scrolled by one row, the first slot is the cut row.
        view.first_visible = 1;
        let (key, anchor, _) =
            row_tooltip(&rows, &view, &list, center(0), &en).expect("slot 0 is the cut row now");
        assert_eq!(key, HoverKey::TaskRow(rows[1].key));
        assert_eq!(anchor, list.row_rect(0));
    }

    #[test]
    fn a_name_that_shrinks_to_nothing_paints_nothing_but_the_numbers_stay() {
        let mut m = TextMeasurer::new();
        let narrow = Rect {
            x: 0.0,
            y: 0.0,
            width: 360.0,
            height: 400.0,
        };
        let rows = vec![process_row(2, "node.exe", 1)];
        let mut view = TaskManagerView::new(None);
        let mut out = Vec::new();
        paint(
            &mut out,
            narrow,
            &mut view,
            &rows,
            &TaskManagerHistory::default(),
            60,
            false,
            &style(),
            &pal(),
            &en_us(),
            &mut m,
        );
        let all = texts(&out);
        assert!(!all.iter().any(|r| r.text.contains("node")));
        assert!(all.iter().any(|r| r.text == "2"));
        assert!(all.iter().any(|r| r.text == "50%"));
        assert!(name_truncated(&view, &rows[0].key));
    }

    #[test]
    fn the_style_is_built_from_the_config_tokens() {
        let s = style();
        assert_eq!(
            (s.padding, s.column_gap, s.chart_gap, s.row_height),
            (18.0, 18.0, 8.0, 28.0)
        );
        assert_eq!((s.row_padding_x, s.tree_indent), (8.0, 16.0));
        assert_eq!((s.line_width, s.fill_opacity), (1.5, 0.15));
        assert_eq!((s.text_size, s.note_size, s.label_size), (12.5, 11.0, 10.0));
        assert_eq!((s.row_radius, s.dot_size), (5.0, 6.0));
    }
}
