// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.task_manager]` -- PRD-024, ADR-0096 §7 e §8. Anatomia do
//! gerenciador de tarefas: dimensões (classe de recarga A) e as cinco cores
//! de série, grade e moldura dos gráficos, que moram nos temas
//! (`[task_manager]` de `thm_*.toml`, ADR-0067).
//!
//! The type is named `TaskManagerAppearance` because the crate root also
//! exports `TaskManager` (the `[task_manager]` behaviour section).

use serde::Deserialize;

use crate::color::Color;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct TaskManagerAppearance {
    /// Inner margin of the task manager frame on every side. Origin: the
    /// settings panel `padding` (ADR-0096 §8).
    pub padding: i32,
    /// Space between the charts column and the list column. Origin: the
    /// same 18 as `padding`.
    pub column_gap: i32,
    /// Space between two stacked charts. Origin: `settings.row_gap`.
    pub chart_gap: i32,
    /// Height of list rows and chart headers. Origin:
    /// `session_picker.row_height`.
    pub row_height: i32,
    /// Horizontal padding inside list rows and chart scale labels. Origin:
    /// `session_picker.row_padding_x`.
    pub row_padding_x: i32,
    /// Indent per tree level in the process list, in logical pixels. New in
    /// ADR-0096 §8 (about two 12.5 px digits); owner's visual approval
    /// pending.
    pub tree_indent: i32,
    /// Chart line thickness, in logical pixels. New in ADR-0096 §8; owner's
    /// visual approval pending.
    pub line_width: f64,
    /// Alpha multiplier of the area under the chart line, from 0.0 to 1.0.
    /// New in ADR-0096 §8; owner's visual approval pending.
    pub fill_opacity: f64,
    /// CPU series color (theme key `[task_manager] cpu`).
    #[serde(skip_deserializing)]
    pub cpu: Color,
    /// Memory series color (theme key `[task_manager] memory`).
    #[serde(skip_deserializing)]
    pub memory: Color,
    /// Disk series color (theme key `[task_manager] disk`).
    #[serde(skip_deserializing)]
    pub disk: Color,
    /// Chart grid lines (theme key `[task_manager] grid`).
    #[serde(skip_deserializing)]
    pub grid: Color,
    /// Chart frame border (theme key `[task_manager] chart_border`).
    #[serde(skip_deserializing)]
    pub chart_border: Color,
}

impl Default for TaskManagerAppearance {
    fn default() -> Self {
        Self {
            padding: 18,
            column_gap: 18,
            chart_gap: 8,
            row_height: 28,
            row_padding_x: 8,
            tree_indent: 16,
            line_width: 1.5,
            fill_opacity: 0.15,
            cpu: Color::hex("#6fa8f5"),
            memory: Color::hex("#a68cf0"),
            disk: Color::hex("#86c56a"),
            grid: Color::hex("#1c2027"),
            chart_border: Color::hex("#262b34"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let d = TaskManagerAppearance::default();
        assert_eq!(d.padding, 18);
        assert_eq!(d.column_gap, 18);
        assert_eq!(d.chart_gap, 8);
        assert_eq!(d.row_height, 28);
        assert_eq!(d.row_padding_x, 8);
        assert_eq!(d.tree_indent, 16);
        assert_eq!(d.line_width, 1.5);
        assert_eq!(d.fill_opacity, 0.15);
        assert_eq!(d.cpu, Color::hex("#6fa8f5"));
        assert_eq!(d.chart_border, Color::hex("#262b34"));
    }
}
