// SPDX-License-Identifier: GPL-3.0-or-later

//! Task manager as a window mode (PRD-024, ADR-0095, ADR-0096): the pure
//! model. No window, no GPU and no thread in here -- the state the window
//! keeps while the mode is open ([`TaskManagerView`]), the chart history
//! ([`TaskManagerHistory`]), the row list as a function of the workspace and
//! the last sample ([`rows`]), the chart points and scales ([`chart`]), the
//! localized number formatting ([`format`]) and the row context menu
//! ([`menu`]). Painting, the key chain and the `App` wiring come in later
//! steps and only consume these types.
//!
//! Every timer-like input (`Instant`) comes from the caller, as everywhere
//! else in the UI (CLAUDE.md).

pub(crate) mod chart;
pub(crate) mod format;
pub(crate) mod menu;
pub(crate) mod paint;
pub(crate) mod rows;

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use porecatu_core::{PaneId, TabId};
use porecatu_term::ProcessStat;

pub(crate) use menu::TaskMenu;
/// The measurement caches of the painting (ADR-0096 §5), kept in
/// [`TaskManagerView::measure`].
pub(crate) use paint::MeasureCache;
pub(crate) use rows::RowKey;

/// Floor of `[task_manager] history_seconds` (RF-24.26).
pub(crate) const MIN_HISTORY_SECONDS: u32 = 30;
/// Ceiling of `[task_manager] history_seconds` (RF-24.26).
pub(crate) const MAX_HISTORY_SECONDS: u32 = 600;

/// What `[task_manager] history_seconds` becomes after the range of RF-24.26
/// (ADR-0095 §9), the same shape as `git::remote_sync::EffectivePollInterval`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EffectiveHistory {
    pub seconds: u32,
    /// `true` when the configured value was moved to the nearest limit: the
    /// app tells the user **once** (RF-24.26), not on every evaluation.
    pub adjusted: bool,
}

/// RF-24.26: a value outside 30..=600 goes to the nearest limit, flagged.
pub(crate) fn effective_history(raw: u32) -> EffectiveHistory {
    let seconds = raw.clamp(MIN_HISTORY_SECONDS, MAX_HISTORY_SECONDS);
    EffectiveHistory {
        seconds,
        adjusted: seconds != raw,
    }
}

/// The last sample **of this window** (ADR-0095 §1): the live processes of
/// every pane that has a local process, keyed like the sampler request (the
/// pane keys carry no window, one sample belongs to one window).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct WindowSample {
    /// When the sampler read the processes. `None` until the first answer
    /// ("waiting for the first measurement", RF-24.17).
    pub taken_at: Option<Instant>,
    pub panes: HashMap<(TabId, PaneId), Vec<ProcessStat>>,
}

impl WindowSample {
    /// ADR-0095 §4: a tab that left this window takes its processes out of
    /// the sample; they come back in the destination's next sample.
    pub(crate) fn remove_tabs(&mut self, tabs: &[TabId]) {
        self.panes.retain(|(tab, _), _| !tabs.contains(tab));
    }

    /// Sum of every process of the sample: the point the window's charts get
    /// (RF-24.8).
    pub(crate) fn totals(&self) -> Totals {
        totals(self.panes.values().flatten())
    }
}

/// Sum of a set of processes (CPU, memory, disk), RF-24.15 and RF-24.8.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Totals {
    /// `None` while no process of the set has a CPU reading (the first sample
    /// of the sampler, RF-24.9); the sum of the readings that exist otherwise.
    pub cpu: Option<f32>,
    pub memory: u64,
    pub disk_per_sec: f64,
}

/// RF-24.15: CPU is the sum of the `Some` values, `None` when there is none
/// (empty set included); memory and disk are plain sums.
pub(crate) fn totals<'a>(stats: impl IntoIterator<Item = &'a ProcessStat>) -> Totals {
    let mut cpu: Option<f32> = None;
    let mut memory: u64 = 0;
    let mut disk_per_sec = 0.0_f64;
    for stat in stats {
        if let Some(value) = stat.cpu {
            cpu = Some(cpu.unwrap_or(0.0) + value);
        }
        memory = memory.saturating_add(stat.memory);
        disk_per_sec += stat.disk_per_sec;
    }
    Totals {
        cpu,
        memory,
        disk_per_sec,
    }
}

/// One point of the chart history (ADR-0095 §1): the window's totals at the
/// instant the sampler read them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HistoryPoint {
    pub at: Instant,
    pub cpu: Option<f32>,
    pub memory: u64,
    pub disk_per_sec: f64,
}

impl HistoryPoint {
    pub(crate) fn from_totals(at: Instant, totals: Totals) -> Self {
        Self {
            at,
            cpu: totals.cpu,
            memory: totals.memory,
            disk_per_sec: totals.disk_per_sec,
        }
    }
}

/// The chart history of a window (ADR-0095 §1, RF-24.12). It outlives the
/// mode (closing the task manager keeps it, the gaps stay visible) and
/// disappears with the window; it is never written anywhere.
#[derive(Debug, Clone, Default)]
pub(crate) struct TaskManagerHistory {
    points: VecDeque<HistoryPoint>,
}

impl TaskManagerHistory {
    /// Appends a point and drops the ones older than `history_seconds`
    /// before it (counted from the new point, not from the clock).
    pub(crate) fn push(&mut self, point: HistoryPoint, history_seconds: u32) {
        self.points.push_back(point);
        self.trim(history_seconds);
    }

    /// Drops the points older than `history_seconds` before the last one.
    /// Separate from [`Self::push`] so a smaller period cuts the history at
    /// the next sample (RF-24.27) even when the sample adds nothing.
    pub(crate) fn trim(&mut self, history_seconds: u32) {
        let Some(last) = self.points.back().map(|point| point.at) else {
            return;
        };
        let period = Duration::from_secs(u64::from(history_seconds));
        while self
            .points
            .front()
            .is_some_and(|first| last.saturating_duration_since(first.at) > period)
        {
            self.points.pop_front();
        }
    }

    /// The points, oldest first.
    pub(crate) fn points(&self) -> &VecDeque<HistoryPoint> {
        &self.points
    }

    /// The most recent point: the "current value" of the chart headers.
    pub(crate) fn latest(&self) -> Option<&HistoryPoint> {
        self.points.back()
    }
}

/// The mode state of one window (ADR-0095 §1): `Some(TaskManagerView)` in
/// `WindowState` means the content area shows the task manager.
#[derive(Debug, Clone)]
pub(crate) struct TaskManagerView {
    /// The last sample of this window.
    pub sample: WindowSample,
    /// The selected row, by identity (RF-24.18), never by index.
    pub selection: Option<RowKey>,
    /// Index of the first visible row of the list.
    pub first_visible: usize,
    /// The row under the mouse.
    pub hover: Option<RowKey>,
    /// The row context menu, when open (ADR-0095 §7).
    pub menu: Option<TaskMenu>,
    /// The last left click on a row, for the double click ("Go to tab",
    /// RF-24.20). Lives and dies with the mode, like everything else here.
    pub last_click: Option<(RowKey, Instant)>,
    /// A left press on the footer's toggle (ADR-0098 §5): the release over
    /// it flips `background_monitoring`, like the settings screen's toggles.
    pub footer_pressed: bool,
    /// Measurement caches of [`paint::paint`]; dropped by
    /// [`MeasureCache::clear`] when the font or the scale change.
    pub measure: MeasureCache,
}

impl TaskManagerView {
    /// `initial_selection`: the row of the active tab, so opening the task
    /// manager starts where the user is (ADR-0095 §2).
    pub(crate) fn new(initial_selection: Option<RowKey>) -> Self {
        Self {
            sample: WindowSample::default(),
            selection: initial_selection,
            first_visible: 0,
            hover: None,
            menu: None,
            last_click: None,
            footer_pressed: false,
            measure: MeasureCache::default(),
        }
    }

    /// ADR-0095 §4: tabs that left the window leave the sample **and** every
    /// key of the view that points at them (a selection, a hover or a menu
    /// would be an orphan, ADR-0090). A lost selection falls to the first
    /// row when the list is next resolved.
    pub(crate) fn remove_tabs(&mut self, tabs: &[TabId]) {
        self.sample.remove_tabs(tabs);
        if self
            .selection
            .as_ref()
            .is_some_and(|key| tabs.contains(&key.tab()))
        {
            self.selection = None;
        }
        if self
            .hover
            .as_ref()
            .is_some_and(|key| tabs.contains(&key.tab()))
        {
            self.hover = None;
        }
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| tabs.contains(&menu.row.tab()))
        {
            self.menu = None;
        }
        if self
            .last_click
            .as_ref()
            .is_some_and(|(key, _)| tabs.contains(&key.tab()))
        {
            self.last_click = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(n: u32) -> TabId {
        TabId::new(n)
    }

    fn pane(n: u32) -> PaneId {
        PaneId::new(n)
    }

    fn stat(pid: u32, cpu: Option<f32>, memory: u64, disk: f64) -> ProcessStat {
        ProcessStat {
            pid,
            parent: None,
            start_time: 0,
            name: format!("p{pid}"),
            cpu,
            memory,
            disk_per_sec: disk,
        }
    }

    fn point(base: Instant, secs: u64, memory: u64) -> HistoryPoint {
        HistoryPoint {
            at: base + Duration::from_secs(secs),
            cpu: Some(0.1),
            memory,
            disk_per_sec: 0.0,
        }
    }

    #[test]
    fn effective_history_clamps_and_flags() {
        assert_eq!(
            effective_history(5),
            EffectiveHistory {
                seconds: 30,
                adjusted: true
            }
        );
        assert_eq!(
            effective_history(60),
            EffectiveHistory {
                seconds: 60,
                adjusted: false
            }
        );
        assert_eq!(
            effective_history(1000),
            EffectiveHistory {
                seconds: 600,
                adjusted: true
            }
        );
        // The limits themselves are not "adjusted".
        assert!(!effective_history(30).adjusted);
        assert!(!effective_history(600).adjusted);
        assert_eq!(effective_history(0).seconds, 30);
    }

    #[test]
    fn totals_sum_and_cpu_is_none_only_without_readings() {
        let stats = [
            stat(1, Some(0.25), 100, 1.0),
            stat(2, None, 50, 2.0),
            stat(3, Some(0.5), 10, 0.5),
        ];
        let sum = totals(&stats);
        assert_eq!(sum.cpu, Some(0.75));
        assert_eq!(sum.memory, 160);
        assert_eq!(sum.disk_per_sec, 3.5);

        let none = [stat(1, None, 1, 0.0), stat(2, None, 1, 0.0)];
        assert_eq!(totals(&none).cpu, None);
        assert_eq!(totals(&[]).cpu, None);
        assert_eq!(totals(&[]).memory, 0);
    }

    #[test]
    fn window_sample_totals_sum_every_pane() {
        let mut sample = WindowSample::default();
        sample
            .panes
            .insert((tab(1), pane(0)), vec![stat(1, Some(0.1), 10, 0.0)]);
        sample
            .panes
            .insert((tab(2), pane(0)), vec![stat(2, Some(0.2), 20, 0.0)]);
        let sum = sample.totals();
        assert_eq!(sum.memory, 30);
        assert!((sum.cpu.unwrap() - 0.3).abs() < 1e-6);
    }

    #[test]
    fn remove_tabs_drops_only_the_listed_tabs() {
        let mut sample = WindowSample::default();
        sample.panes.insert((tab(1), pane(0)), vec![]);
        sample.panes.insert((tab(1), pane(1)), vec![]);
        sample.panes.insert((tab(2), pane(0)), vec![]);
        sample.panes.insert((tab(3), pane(0)), vec![]);
        sample.remove_tabs(&[tab(1), tab(3)]);
        let mut keys: Vec<_> = sample.panes.keys().copied().collect();
        keys.sort();
        assert_eq!(keys, vec![(tab(2), pane(0))]);
    }

    #[test]
    fn view_remove_tabs_clears_every_key_that_points_at_a_departed_tab() {
        let mut view = TaskManagerView::new(Some(RowKey::Tab(tab(1))));
        view.hover = Some(RowKey::Note(tab(1), pane(0)));
        view.menu = Some(TaskMenu::new(RowKey::Tab(tab(1)), (0.0, 0.0), &[]));
        view.sample.panes.insert((tab(1), pane(0)), vec![]);
        view.sample.panes.insert((tab(2), pane(0)), vec![]);
        view.remove_tabs(&[tab(1)]);
        assert_eq!(view.selection, None);
        assert_eq!(view.hover, None);
        assert!(view.menu.is_none());
        assert_eq!(view.sample.panes.len(), 1);

        // Keys of other tabs survive.
        let mut view = TaskManagerView::new(Some(RowKey::Tab(tab(2))));
        view.remove_tabs(&[tab(1)]);
        assert_eq!(view.selection, Some(RowKey::Tab(tab(2))));
    }

    #[test]
    fn push_drops_the_points_older_than_the_period_before_the_last() {
        let base = Instant::now();
        let mut history = TaskManagerHistory::default();
        for secs in 0..=40 {
            history.push(point(base, secs, secs), 30);
        }
        // Last is at 40 s; the oldest kept is exactly 30 s before it.
        let first = history.points().front().unwrap();
        assert_eq!(first.memory, 10);
        assert_eq!(history.points().len(), 31);
        assert_eq!(history.latest().unwrap().memory, 40);
    }

    #[test]
    fn a_point_exactly_at_the_period_is_kept_and_one_second_older_is_not() {
        let base = Instant::now();
        let mut history = TaskManagerHistory::default();
        history.push(point(base, 0, 0), 30);
        history.push(point(base, 30, 1), 30);
        assert_eq!(history.points().len(), 2);
        history.push(point(base, 31, 2), 30);
        assert_eq!(history.points().len(), 2);
        assert_eq!(history.points().front().unwrap().memory, 1);
    }

    #[test]
    fn trim_cuts_the_history_when_the_period_shrinks() {
        let base = Instant::now();
        let mut history = TaskManagerHistory::default();
        for secs in 0..=100 {
            history.push(point(base, secs, secs), 600);
        }
        assert_eq!(history.points().len(), 101);
        history.trim(60);
        assert_eq!(history.points().len(), 61);
        assert_eq!(history.points().front().unwrap().memory, 40);
        // Growing back does not resurrect anything.
        history.trim(600);
        assert_eq!(history.points().len(), 61);
    }

    #[test]
    fn trim_on_an_empty_history_is_a_no_op() {
        let mut history = TaskManagerHistory::default();
        history.trim(30);
        assert!(history.points().is_empty());
        assert!(history.latest().is_none());
    }
}
