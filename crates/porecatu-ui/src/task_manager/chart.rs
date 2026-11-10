// SPDX-License-Identifier: GPL-3.0-or-later

//! Chart points, grids and scales from the history (ADR-0095 §6, ADR-0096
//! §2 and §3, PRD-024 RF-24.7 to RF-24.12). Pure geometry in logical pixels
//! over an inner rectangle; nothing here draws.

use std::time::Instant;

use porecatu_render::Rect;

use super::{HistoryPoint, TaskManagerHistory};

/// Two consecutive points further apart than this break the series
/// (RF-24.12, ADR-0095 §6): the sampler beats at 1 s, so 1.5 s means a sample
/// was missed or the task manager was closed.
pub(crate) const GAP_SECONDS: f64 = 1.5;

/// Distance between the vertical grid lines (ADR-0096 §3).
pub(crate) const GRID_STEP_SECONDS: u32 = 10;

/// The CPU scale is fixed at the whole machine (RF-24.9).
pub(crate) const CPU_TOP: f64 = 1.0;

/// The smallest step of the automatic scales, in bytes: 1 KB for memory and
/// 1 KB/s for disk, so an all-zero period keeps the line on the floor
/// instead of the middle (ADR-0095 §6).
pub(crate) const MIN_TOP: f64 = 1024.0;

/// A point further left than the rectangle by less than this still counts as
/// on the edge (float noise of `right - width`).
const EDGE_EPSILON: f32 = 0.01;

/// Display units of the memory and disk scales, base 1024: B, KB, MB, GB.
const UNITS: [f64; 4] = [1.0, 1024.0, 1_048_576.0, 1_073_741_824.0];

/// The three charts, in screen order (RF-24.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Metric {
    Cpu,
    Memory,
    Disk,
}

impl Metric {
    pub(crate) const ALL: [Metric; 3] = [Metric::Cpu, Metric::Memory, Metric::Disk];

    /// The value of a history point for this chart. A point without a CPU
    /// reading is a **gap**, not a zero (RF-24.9, RF-24.12).
    pub(crate) fn value(self, point: &HistoryPoint) -> Option<f64> {
        match self {
            Self::Cpu => point.cpu.map(f64::from),
            Self::Memory => Some(point.memory as f64),
            Self::Disk => Some(point.disk_per_sec),
        }
    }
}

/// Seconds between `point` and the reference instant, never negative.
fn age_seconds(reference: Instant, at: Instant) -> f64 {
    reference.saturating_duration_since(at).as_secs_f64()
}

/// The polyline points of one chart, one `Vec` per continuous run (ADR-0095
/// §6).
///
/// - The reference of the time axis is the `at` of the **last** point, not the
///   clock: the chart only moves when data arrives.
/// - `x = right - (last.at - p.at) / history_seconds * width`;
///   `y = bottom - min(v / top, 1) * height`.
/// - Two consecutive points more than [`GAP_SECONDS`] apart, or a point with
///   no value, end the run (RF-24.12). A run with a single point is kept; the
///   painting draws no line with fewer than two.
/// - Points left of the rectangle are cut, except the nearest one when it
///   connects to the next point, so the line enters from the left edge (the
///   painting's clip trims it).
pub(crate) fn series(
    history: &TaskManagerHistory,
    metric: Metric,
    rect: Rect,
    top_value: f64,
    history_seconds: u32,
) -> Vec<Vec<(f32, f32)>> {
    let Some(last) = history.latest() else {
        return Vec::new();
    };
    let period = f64::from(history_seconds.max(1));
    let width = f64::from(rect.width);
    let height = f64::from(rect.height);
    let right = f64::from(rect.x) + width;
    let bottom = f64::from(rect.y) + height;

    let mut runs: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    let mut previous: Option<Instant> = None;

    for point in history.points() {
        let gap = previous.is_some_and(|before| age_seconds(point.at, before) > GAP_SECONDS);
        let value = metric.value(point);
        if (gap || value.is_none()) && !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
        previous = Some(point.at);
        let Some(value) = value else { continue };

        let ratio = if top_value > 0.0 {
            (value / top_value).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let x = right - age_seconds(last.at, point.at) / period * width;
        let y = bottom - ratio * height;
        current.push((x as f32, y as f32));
    }
    if !current.is_empty() {
        runs.push(current);
    }

    // Left edge: keep at most one point beyond it per run.
    let left = rect.x - EDGE_EPSILON;
    for run in &mut runs {
        while run.len() >= 2 && run[1].0 < left {
            run.remove(0);
        }
    }
    runs.retain(|run| !(run.len() == 1 && run[0].0 < left));
    runs
}

/// X of the vertical grid lines: one every [`GRID_STEP_SECONDS`] counting
/// from the right edge (the instant of the last sample) to the left, strictly
/// inside the rectangle (the frame draws the edges). Five lines for 60 s.
pub(crate) fn vertical_grid(rect: Rect, history_seconds: u32) -> Vec<f32> {
    let period = history_seconds.max(1);
    let right = rect.x + rect.width;
    (1..)
        .map(|step| step * GRID_STEP_SECONDS)
        .take_while(|seconds| *seconds < period)
        .map(|seconds| right - seconds as f32 / period as f32 * rect.width)
        .collect()
}

/// Y of the horizontal grid lines at 25%, 50% and 75% of the height, top to
/// bottom.
pub(crate) fn horizontal_grid(rect: Rect) -> [f32; 3] {
    [0.25, 0.5, 0.75].map(|fraction| rect.y + rect.height * fraction)
}

/// The largest value of the visible period for `metric`: the points at most
/// `history_seconds` before the last one.
pub(crate) fn visible_max(
    history: &TaskManagerHistory,
    metric: Metric,
    history_seconds: u32,
) -> f64 {
    let Some(last) = history.latest() else {
        return 0.0;
    };
    let period = f64::from(history_seconds.max(1));
    history
        .points()
        .iter()
        .filter(|point| age_seconds(last.at, point.at) <= period)
        .filter_map(|point| metric.value(point))
        .fold(0.0, f64::max)
}

/// The top of the scale of one chart: fixed for CPU, [`nice_top`] of the
/// visible maximum for memory and disk (ADR-0095 §6).
pub(crate) fn scale_top(history: &TaskManagerHistory, metric: Metric, history_seconds: u32) -> f64 {
    match metric {
        Metric::Cpu => CPU_TOP,
        Metric::Memory | Metric::Disk => nice_top(visible_max(history, metric, history_seconds)),
    }
}

/// The top of an automatic scale for `max_visible` bytes (or bytes/s):
/// rounded **up** to the next `1`, `2` or `5` times a power of ten **in the
/// display unit** (base 1024).
///
/// The unit is the largest in which the value is at least 1; the number in
/// that unit goes up to 1/2/5 x 10^n; when that passes 1024 the unit moves to
/// the next one and the **original** value is rounded again there (1020 MB
/// would round to 2000 MB, so it becomes 1 GB, not 2000/1024 GB). Nothing
/// goes below [`MIN_TOP`], and GB is the last unit.
///
/// 1.3 GB gives 2 GB, 700 MB gives 1000 MB, 1020 MB gives 1 GB, 3 KB gives
/// 5 KB and an all-zero period gives 1 KB.
pub(crate) fn nice_top(max_visible: f64) -> f64 {
    let max = if max_visible.is_finite() {
        max_visible
    } else {
        0.0
    };
    if max <= MIN_TOP {
        return MIN_TOP;
    }
    let mut unit = UNITS.iter().rposition(|size| max >= *size).unwrap_or(0);
    let mut rounded = round_up_125(max / UNITS[unit]);
    if rounded > 1024.0 && unit + 1 < UNITS.len() {
        unit += 1;
        rounded = round_up_125(max / UNITS[unit]);
    }
    (rounded * UNITS[unit]).max(MIN_TOP)
}

/// Smallest `1`, `2`, `5` or `10` times a power of ten that is at least `x`
/// (`x > 0`).
fn round_up_125(x: f64) -> f64 {
    debug_assert!(x > 0.0 && x.is_finite());
    let mut exp = x.log10().floor() as i32;
    // `log10` can land one off next to a power of ten.
    if 10f64.powi(exp + 1) <= x {
        exp += 1;
    } else if 10f64.powi(exp) > x {
        exp -= 1;
    }
    let base = 10f64.powi(exp);
    let mantissa = x / base;
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .find(|step| mantissa <= step * (1.0 + 1e-9))
        .unwrap_or(10.0);
    step * base
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const GB: f64 = 1_073_741_824.0;
    const MB: f64 = 1_048_576.0;
    const KB: f64 = 1024.0;

    fn rect() -> Rect {
        Rect {
            x: 100.0,
            y: 50.0,
            width: 600.0,
            height: 200.0,
        }
    }

    fn history(
        base: Instant,
        points: &[(f64, Option<f32>, u64, f64)],
        seconds: u32,
    ) -> TaskManagerHistory {
        let mut history = TaskManagerHistory::default();
        for (at, cpu, memory, disk) in points {
            history.push(
                HistoryPoint {
                    at: base + Duration::from_secs_f64(*at),
                    cpu: *cpu,
                    memory: *memory,
                    disk_per_sec: *disk,
                },
                seconds,
            );
        }
        history
    }

    fn cpu_at(secs: &[f64], cpu: f32) -> Vec<(f64, Option<f32>, u64, f64)> {
        secs.iter().map(|s| (*s, Some(cpu), 0, 0.0)).collect()
    }

    #[test]
    fn the_last_point_is_on_the_right_edge_and_thirty_seconds_back_is_the_middle() {
        let base = Instant::now();
        let secs: Vec<f64> = (0..=60).map(f64::from).collect();
        let h = history(base, &cpu_at(&secs, 0.5), 60);
        let runs = series(&h, Metric::Cpu, rect(), 1.0, 60);
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.len(), 61);
        let (last_x, last_y) = *run.last().unwrap();
        assert!((last_x - 700.0).abs() < 1e-3, "last x {last_x}");
        assert!((run[30].0 - 400.0).abs() < 1e-3, "middle x {}", run[30].0);
        assert!((run[0].0 - 100.0).abs() < 1e-3, "first x {}", run[0].0);
        // CPU 0.5 of a 1.0 top is the middle of the height.
        assert!((last_y - 150.0).abs() < 1e-3, "y {last_y}");
    }

    #[test]
    fn the_reference_is_the_last_point_not_the_clock() {
        let base = Instant::now();
        let secs: Vec<f64> = (0..=10).map(f64::from).collect();
        let h = history(base, &cpu_at(&secs, 0.0), 60);
        let runs = series(&h, Metric::Cpu, rect(), 1.0, 60);
        // The newest point sits on the right edge no matter how old it is.
        assert!((runs[0].last().unwrap().0 - 700.0).abs() < 1e-3);
        // Ten seconds of data cover a sixth of the width.
        assert!((runs[0][0].0 - 600.0).abs() < 1e-3);
    }

    #[test]
    fn a_gap_over_one_and_a_half_seconds_breaks_the_series() {
        let base = Instant::now();
        let h = history(base, &cpu_at(&[0.0, 1.0, 2.0, 12.0, 13.0], 0.2), 60);
        let runs = series(&h, Metric::Cpu, rect(), 1.0, 60);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len(), 3);
        assert_eq!(runs[1].len(), 2);
        // Exactly 1.5 s is still contiguous; just above is not.
        let h = history(base, &cpu_at(&[0.0, 1.5, 3.0], 0.2), 60);
        assert_eq!(series(&h, Metric::Cpu, rect(), 1.0, 60).len(), 1);
        let h = history(base, &cpu_at(&[0.0, 1.6], 0.2), 60);
        assert_eq!(series(&h, Metric::Cpu, rect(), 1.0, 60).len(), 2);
    }

    #[test]
    fn a_single_point_run_is_kept_for_the_painting_to_skip() {
        let base = Instant::now();
        let h = history(base, &cpu_at(&[0.0, 10.0], 0.2), 60);
        let runs = series(&h, Metric::Cpu, rect(), 1.0, 60);
        assert_eq!(runs.iter().map(Vec::len).collect::<Vec<_>>(), vec![1, 1]);
    }

    #[test]
    fn a_cpu_none_is_a_gap_not_a_zero() {
        let base = Instant::now();
        let h = history(
            base,
            &[
                (0.0, Some(0.1), 5, 0.0),
                (1.0, Some(0.1), 5, 0.0),
                (2.0, None, 5, 0.0),
                (3.0, Some(0.1), 5, 0.0),
                (4.0, Some(0.1), 5, 0.0),
            ],
            60,
        );
        let cpu = series(&h, Metric::Cpu, rect(), 1.0, 60);
        assert_eq!(cpu.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 2]);
        // The same history has no gap for memory.
        let memory = series(&h, Metric::Memory, rect(), 10.0, 60);
        assert_eq!(memory.len(), 1);
        assert_eq!(memory[0].len(), 5);
    }

    #[test]
    fn a_value_over_the_top_is_clamped_and_zero_is_the_floor() {
        let base = Instant::now();
        let h = history(
            base,
            &[(0.0, Some(2.0), 0, 0.0), (1.0, Some(0.0), 0, 0.0)],
            60,
        );
        let run = &series(&h, Metric::Cpu, rect(), 1.0, 60)[0];
        assert!((run[0].1 - 50.0).abs() < 1e-3, "clamped to the top");
        assert!((run[1].1 - 250.0).abs() < 1e-3, "zero on the bottom");
        // A zero top never divides.
        let run = &series(&h, Metric::Cpu, rect(), 0.0, 60)[0];
        assert!((run[0].1 - 250.0).abs() < 1e-3);
    }

    #[test]
    fn points_left_of_the_rectangle_are_cut_but_one_stays_to_enter_the_line() {
        let base = Instant::now();
        // 30 s of data in a 10 s window: only the last ~10 s are inside.
        let secs: Vec<f64> = (0..=30).map(f64::from).collect();
        let mut h = TaskManagerHistory::default();
        for s in &secs {
            h.push(
                HistoryPoint {
                    at: base + Duration::from_secs_f64(*s),
                    cpu: Some(0.5),
                    memory: 0,
                    disk_per_sec: 0.0,
                },
                600,
            );
        }
        let runs = series(&h, Metric::Cpu, rect(), 1.0, 10);
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        // The point 10 s back is on the left edge (x = 100) and the one
        // before it, 11 s back, is the single point kept outside to enter
        // the line; nothing older survives. 12 points: ages 11 down to 0.
        assert_eq!(run.len(), 12);
        assert!(run[0].0 < 100.0);
        assert!((run[1].0 - 100.0).abs() < 1e-3);

        // Same data, an 11 s window: the edge point is now the 11 s one and
        // the 12 s one is the extra.
        let runs = series(&h, Metric::Cpu, rect(), 1.0, 11);
        assert_eq!(runs[0].len(), 13);
        assert!(runs[0][0].0 < 100.0);
        assert!((runs[0][1].0 - 100.0).abs() < 1e-3);
    }

    #[test]
    fn a_lone_point_beyond_the_left_edge_is_dropped() {
        let base = Instant::now();
        let mut h = TaskManagerHistory::default();
        for s in [0.0, 30.0, 31.0] {
            h.push(
                HistoryPoint {
                    at: base + Duration::from_secs_f64(s),
                    cpu: Some(0.5),
                    memory: 0,
                    disk_per_sec: 0.0,
                },
                600,
            );
        }
        // Window of 10 s: the point at 0 s is far outside and alone.
        let runs = series(&h, Metric::Cpu, rect(), 1.0, 10);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len(), 2);
    }

    #[test]
    fn an_empty_history_has_no_series() {
        let h = TaskManagerHistory::default();
        assert!(series(&h, Metric::Cpu, rect(), 1.0, 60).is_empty());
        assert_eq!(visible_max(&h, Metric::Memory, 60), 0.0);
        assert_eq!(scale_top(&h, Metric::Memory, 60), MIN_TOP);
    }

    #[test]
    fn the_vertical_grid_has_five_lines_at_sixty_seconds() {
        let lines = vertical_grid(rect(), 60);
        assert_eq!(lines.len(), 5);
        // 10 s back from the right edge (700) is a sixth of the width.
        assert!((lines[0] - 600.0).abs() < 1e-3);
        assert!((lines[4] - 200.0).abs() < 1e-3);
        assert!(lines.iter().all(|x| *x > 100.0 && *x < 700.0));
        assert_eq!(vertical_grid(rect(), 30).len(), 2);
        assert_eq!(vertical_grid(rect(), 120).len(), 11);
        assert_eq!(vertical_grid(rect(), 10).len(), 0);
    }

    #[test]
    fn the_horizontal_grid_is_at_the_quarters() {
        assert_eq!(horizontal_grid(rect()), [100.0, 150.0, 200.0]);
    }

    #[test]
    fn nice_top_rounds_up_in_the_display_unit() {
        // The cases of the step: 1.3 GB, 700 MB, 1020 MB, 3 KB, zero.
        assert_eq!(nice_top(1.3 * GB), 2.0 * GB);
        assert_eq!(nice_top(700.0 * MB), 1000.0 * MB);
        assert_eq!(nice_top(1020.0 * MB), GB);
        assert_eq!(nice_top(3.0 * KB), 5.0 * KB);
        assert_eq!(nice_top(0.0), KB);
    }

    #[test]
    fn nice_top_edges() {
        // Everything at or below 1 KB (and junk) is the smallest step.
        assert_eq!(nice_top(1.0), KB);
        assert_eq!(nice_top(KB), KB);
        assert_eq!(nice_top(-5.0), KB);
        assert_eq!(nice_top(f64::NAN), KB);
        assert_eq!(nice_top(f64::INFINITY), KB);
        // Exact steps stay put.
        assert_eq!(nice_top(2.0 * GB), 2.0 * GB);
        assert_eq!(nice_top(5.0 * MB), 5.0 * MB);
        assert_eq!(nice_top(1.0 * MB), MB);
        // Just above a step goes to the next one.
        assert_eq!(nice_top(2.0 * GB + MB), 5.0 * GB);
        assert_eq!(nice_top(1.0 * MB + 1.0), 2.0 * MB);
        // 10 x 10^n is allowed as long as it stays within 1024.
        assert_eq!(nice_top(600.0 * KB), 1000.0 * KB);
        assert_eq!(nice_top(11.0 * KB), 20.0 * KB);
        // Past 1024 in the unit the original value is rounded again in the
        // next one: 1020 KB would be 2000 KB, so it is 1 MB; 1100 KB is
        // already 1.07 MB, so 2 MB.
        assert_eq!(nice_top(1020.0 * KB), MB);
        assert_eq!(nice_top(1100.0 * KB), 2.0 * MB);
        // GB is the last unit: no TB.
        assert_eq!(nice_top(3000.0 * GB), 5000.0 * GB);
        assert_eq!(nice_top(0.3 * GB), 500.0 * MB);
    }

    #[test]
    fn scale_top_is_fixed_for_cpu_and_follows_the_visible_maximum_otherwise() {
        let base = Instant::now();
        let h = history(
            base,
            &[
                (0.0, Some(0.9), 800 * MB as u64, 4.0 * MB),
                (1.0, Some(0.1), 100 * MB as u64, 2.0 * MB),
            ],
            60,
        );
        assert_eq!(scale_top(&h, Metric::Cpu, 60), 1.0);
        assert_eq!(scale_top(&h, Metric::Memory, 60), 1000.0 * MB);
        assert_eq!(scale_top(&h, Metric::Disk, 60), 5.0 * MB);
    }

    #[test]
    fn visible_max_ignores_points_older_than_the_period() {
        let base = Instant::now();
        let mut h = TaskManagerHistory::default();
        for (s, memory) in [(0.0, 900_u64), (50.0, 10), (60.0, 20)] {
            h.push(
                HistoryPoint {
                    at: base + Duration::from_secs_f64(s),
                    cpu: None,
                    memory,
                    disk_per_sec: 0.0,
                },
                600,
            );
        }
        assert_eq!(visible_max(&h, Metric::Memory, 30), 20.0);
        assert_eq!(visible_max(&h, Metric::Memory, 100), 900.0);
        assert_eq!(visible_max(&h, Metric::Cpu, 100), 0.0);
    }
}
