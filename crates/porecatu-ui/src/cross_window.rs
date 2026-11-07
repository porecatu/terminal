// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure geometry of the drag across windows (ADR-0091 §1, §2, §5): the global
//! point of the cursor, the window under it, the focus order, and where a
//! detached window is born.
//!
//! Nothing here touches a window: everything that talks about a window is
//! generic in `W` (a winit `WindowId` cannot be built in a test), like
//! `pinned::PinnedRegistry`. "Physical" coordinates are screen pixels
//! (`i32`/`f64`); "logical" ones belong to the window in question (`f32`).

/// ADR-0091 §2: the windows, most recently focused first. That order stands
/// in for the Z order the OS does not tell (and lists the windows of the
/// "Move to window..." popover, ADR-0090 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FocusOrder<W> {
    order: Vec<W>,
}

impl<W: Copy + Eq> FocusOrder<W> {
    pub(crate) fn new() -> Self {
        Self { order: Vec::new() }
    }

    /// `w` was focused: it moves (or enters) at the front, never twice.
    pub(crate) fn touch(&mut self, w: W) {
        self.order.retain(|&other| other != w);
        self.order.insert(0, w);
    }

    /// `w` is gone.
    pub(crate) fn remove(&mut self, w: W) {
        self.order.retain(|&other| other != w);
    }

    pub(crate) fn as_slice(&self) -> &[W] {
        &self.order
    }
}

impl<W: Copy + Eq> Default for FocusOrder<W> {
    fn default() -> Self {
        Self::new()
    }
}

/// ADR-0091 §1: global = `inner_position` + `cursor_position`, both physical.
/// `None` when the window position is unknown (Wayland: `inner_position()`
/// errs), which makes the gesture behave as it does without this feature.
pub(crate) fn global_point(
    inner_position: Option<(i32, i32)>,
    cursor: (f64, f64),
) -> Option<(f64, f64)> {
    let (x, y) = inner_position?;
    Some((f64::from(x) + cursor.0, f64::from(y) + cursor.1))
}

/// Converts the global point to the logical coordinate of **one** window:
/// subtracts the physical origin of its client area and divides by **its**
/// scale (mixed DPI, ADR-0091 §1).
pub(crate) fn to_window_logical(
    global: (f64, f64),
    client_origin: (i32, i32),
    scale: f64,
) -> (f32, f32) {
    (
        ((global.0 - f64::from(client_origin.0)) / scale) as f32,
        ((global.1 - f64::from(client_origin.1)) / scale) as f32,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowKind {
    Terminal,
    /// Settings or profile editor: never a drop target, but its area counts
    /// as "inside a Porecatu window" (ADR-0091 §4, case 3).
    Other,
}

/// One window the cursor may be over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Candidate<W> {
    pub id: W,
    pub kind: WindowKind,
    /// Physical origin of the client area on screen.
    pub client_origin: (i32, i32),
    /// Physical size of the client area.
    pub client_size: (u32, u32),
    pub scale: f64,
    pub minimized: bool,
}

impl<W> Candidate<W> {
    fn contains(&self, global: (f64, f64)) -> bool {
        let (ox, oy) = (
            f64::from(self.client_origin.0),
            f64::from(self.client_origin.1),
        );
        global.0 >= ox
            && global.0 < ox + f64::from(self.client_size.0)
            && global.1 >= oy
            && global.1 < oy + f64::from(self.client_size.1)
    }
}

/// Where the global point is, for the drop (ADR-0091 §4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PointOver<W> {
    /// Case 1: on the bar of the origin window.
    OriginBar { logical: (f32, f32) },
    /// Case 2: on the bar of another terminal window.
    OtherBar { window: W, logical: (f32, f32) },
    /// Case 3: inside a Porecatu window, off its bar.
    InsideWindow(W),
    /// Case 4: outside every window.
    Outside,
}

/// ADR-0091 §2/§4. `candidates` come in most-recent-focus order (the origin
/// included; it is the focused window, so it is on top). `bar_height` is
/// logical (`chrome::bar_height`); the bar spans the whole client width.
///
/// 1. Walk `candidates` in order, skipping minimized and `Other` windows. The
///    **first** whose client area holds the point decides: the origin gives
///    `OriginBar` on its bar strip (`0 <= y < bar_height`), else
///    `InsideWindow`; another gives `OtherBar` or `InsideWindow` likewise.
/// 2. If no terminal window holds the point, a non-minimized `Other` window
///    that does gives `InsideWindow`.
/// 3. Otherwise `Outside`.
///
/// The lower bound of the bar matters: a point **above** the window (negative
/// logical y) is not on the bar -- the defect of today's `in_bar`.
pub(crate) fn locate<W: Copy + Eq>(
    global: (f64, f64),
    origin: W,
    candidates: &[Candidate<W>],
    bar_height: f32,
) -> PointOver<W> {
    let live = || candidates.iter().filter(|c| !c.minimized);
    let hit = live()
        .filter(|c| c.kind == WindowKind::Terminal)
        .find(|c| c.contains(global));
    if let Some(c) = hit {
        let logical = to_window_logical(global, c.client_origin, c.scale);
        let on_bar = (0.0..bar_height).contains(&logical.1);
        return match (c.id == origin, on_bar) {
            (true, true) => PointOver::OriginBar { logical },
            (false, true) => PointOver::OtherBar {
                window: c.id,
                logical,
            },
            (_, false) => PointOver::InsideWindow(c.id),
        };
    }
    live()
        .filter(|c| c.kind == WindowKind::Other)
        .find(|c| c.contains(global))
        .map_or(PointOver::Outside, |c| PointOver::InsideWindow(c.id))
}

/// What the release of a tab or group drag does (ADR-0091 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropDecision<W> {
    /// Case 1: the drop of today, on the bar of the origin.
    Local,
    /// Case 2: `App::transfer` to the window whose bar is under the cursor.
    Transfer(W),
    /// Case 3 (and every drop that has nowhere valid to go): nothing changes.
    Cancel,
    /// Case 4: outside every window, a new window is born for the payload.
    Detach,
}

/// ADR-0091 §4: what a release does. `over` is `None` when there is no global
/// point (Wayland) or the drag has no content to carry across windows (a tab
/// in a collapsed group): only the local drop then exists, and it needs the
/// cursor on the bar of the origin (`in_origin_bar`, [`point_in_bar`]); every
/// other release cancels.
pub(crate) fn decide_drop<W: Copy>(
    over: Option<PointOver<W>>,
    in_origin_bar: bool,
) -> DropDecision<W> {
    match over {
        None if in_origin_bar => DropDecision::Local,
        None => DropDecision::Cancel,
        Some(PointOver::OriginBar { .. }) => DropDecision::Local,
        Some(PointOver::OtherBar { window, .. }) => DropDecision::Transfer(window),
        Some(PointOver::InsideWindow(_)) => DropDecision::Cancel,
        Some(PointOver::Outside) => DropDecision::Detach,
    }
}

/// ADR-0091 §4, case 1: `point` (logical, in the window) is on the bar strip
/// of a window `width` wide -- `0 <= y < bar_height` **and** `0 <= x < width`.
/// Unlike the `y < bar_height` of `WindowState::in_bar` (which clicks and
/// hover still use), a point above or beside the window is not on the bar.
pub(crate) fn point_in_bar(point: (f32, f32), width: f32, bar_height: f32) -> bool {
    (0.0..width).contains(&point.0) && (0.0..bar_height).contains(&point.1)
}

/// What a change of the window under a drag's cursor does (ADR-0091 §3):
/// who loses the incoming ghost, who gets it, whether the origin shows only
/// the hole, and who needs another frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Retarget<W> {
    /// The window that was the target and no longer is: its `incoming` is
    /// dropped, and it redraws one last time to erase the ghost.
    pub clear: Option<W>,
    /// The window whose bar the cursor is over, with the cursor in **its**
    /// logical coordinates. Its `incoming` is created or updated.
    pub set: Option<(W, (f32, f32))>,
    /// The origin draws only the hole (`WindowState::drag_elsewhere`).
    pub elsewhere: bool,
    /// The windows to redraw, besides what the event already redraws: the
    /// origin shows up when it flips between ghost and hole.
    pub redraw: Vec<W>,
}

/// ADR-0091 §3: `prev` is the window that shows the ghost now (`None`: the
/// ghost is on the origin bar, or nowhere); `now` is where the cursor is.
/// Only [`PointOver::OtherBar`] makes another window the target; every other
/// place -- the origin bar, a body, the desktop -- puts the ghost back on the
/// origin, which is also what a cancel does (`PointOver::Outside`).
pub(crate) fn retarget<W: Copy + Eq>(
    prev: Option<W>,
    now: &PointOver<W>,
    origin: W,
) -> Retarget<W> {
    let set = match *now {
        PointOver::OtherBar { window, logical } => Some((window, logical)),
        _ => None,
    };
    let next = set.map(|(window, _)| window);
    let clear = prev.filter(|&window| Some(window) != next);
    let mut redraw = Vec::new();
    if prev.is_some() != next.is_some() {
        redraw.push(origin);
    }
    redraw.extend(clear);
    redraw.extend(next);
    Retarget {
        clear,
        set,
        elsewhere: next.is_some(),
        redraw,
    }
}

/// A monitor: physical origin and physical size.
pub(crate) type MonitorRect = ((i32, i32), (u32, u32));

/// ADR-0091 §5: the physical position of the client area of the detached
/// window so the grabbed point stays under the cursor, clamped so the bar
/// strip fits whole in the monitor.
///
/// `pos = global - (element_x + grab_offset, press_y) * scale`; then, in x,
/// the whole window inside the monitor (wider than it: left-aligned to the
/// monitor); in y, the strip `[pos.y, pos.y + bar_height * scale]` inside the
/// monitor. Without a monitor the position is returned unclamped.
// The eight inputs are the eight facts of ADR-0091 §5; a struct would only
// rename them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn detach_position(
    global: (f64, f64),
    element_x: f32,
    grab_offset: f32,
    press_y: f32,
    scale: f64,
    window_size: (u32, u32),
    monitor: Option<MonitorRect>,
    bar_height: f32,
) -> (i32, i32) {
    let x = global.0 - f64::from(element_x + grab_offset) * scale;
    let y = global.1 - f64::from(press_y) * scale;
    let Some(((mx, my), (mw, mh))) = monitor else {
        return (x.round() as i32, y.round() as i32);
    };
    let (mx, my) = (i64::from(mx), i64::from(my));
    let (mw, mh) = (i64::from(mw), i64::from(mh));
    let x = x.round() as i64;
    let y = y.round() as i64;
    let window_w = i64::from(window_size.0);
    let bar_px = (f64::from(bar_height) * scale).ceil() as i64;

    let max_x = (mx + mw - window_w).max(mx);
    let max_y = (my + mh - bar_px).max(my);
    (x.clamp(mx, max_x) as i32, y.clamp(my, max_y) as i32)
}

/// The index of the monitor (physical origin, physical size) that holds the
/// point; `None` if none does.
pub(crate) fn monitor_containing(global: (f64, f64), monitors: &[MonitorRect]) -> Option<usize> {
    monitors.iter().position(|&((x, y), (w, h))| {
        let (x, y) = (f64::from(x), f64::from(y));
        global.0 >= x && global.0 < x + f64::from(w) && global.1 >= y && global.1 < y + f64::from(h)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    type Win = u8;

    // --- FocusOrder ---

    #[test]
    fn touch_moves_to_the_front_without_duplicating() {
        let mut order = FocusOrder::<Win>::new();
        order.touch(1);
        order.touch(2);
        order.touch(3);
        assert_eq!(order.as_slice(), [3, 2, 1]);
        order.touch(1);
        assert_eq!(order.as_slice(), [1, 3, 2]);
        order.touch(1);
        assert_eq!(order.as_slice(), [1, 3, 2], "no duplicate");
    }

    #[test]
    fn remove_drops_the_window_and_ignores_a_stranger() {
        let mut order = FocusOrder::<Win>::new();
        order.touch(1);
        order.touch(2);
        order.remove(1);
        assert_eq!(order.as_slice(), [2]);
        order.remove(9);
        assert_eq!(order.as_slice(), [2]);
    }

    // --- global_point, to_window_logical ---

    #[test]
    fn the_global_point_is_the_window_position_plus_the_cursor() {
        assert_eq!(
            global_point(Some((100, 50)), (10.5, -4.0)),
            Some((110.5, 46.0))
        );
    }

    #[test]
    fn an_unknown_window_position_has_no_global_point() {
        assert_eq!(global_point(None, (10.0, 10.0)), None);
    }

    #[test]
    fn the_same_global_point_is_two_logical_points_in_two_scales() {
        let global = (1500.0, 300.0);
        // A at (0, 0), scale 1.0; B at (1200, 0), scale 1.5.
        assert_eq!(to_window_logical(global, (0, 0), 1.0), (1500.0, 300.0));
        assert_eq!(to_window_logical(global, (1200, 0), 1.5), (200.0, 200.0));
    }

    #[test]
    fn a_point_above_or_left_of_the_window_is_negative() {
        assert_eq!(
            to_window_logical((50.0, 90.0), (100, 100), 2.0),
            (-25.0, -5.0)
        );
    }

    // --- locate ---

    const BAR: f32 = 40.0;

    fn window(id: Win, origin: (i32, i32), size: (u32, u32)) -> Candidate<Win> {
        Candidate {
            id,
            kind: WindowKind::Terminal,
            client_origin: origin,
            client_size: size,
            scale: 1.0,
            minimized: false,
        }
    }

    /// Origin `1` at (0,0) 800x600; `2` at (1000,0) 800x600.
    fn two() -> Vec<Candidate<Win>> {
        vec![
            window(1, (0, 0), (800, 600)),
            window(2, (1000, 0), (800, 600)),
        ]
    }

    #[test]
    fn a_point_on_the_origin_bar_is_the_origin_bar() {
        assert_eq!(
            locate((100.0, 10.0), 1, &two(), BAR),
            PointOver::OriginBar {
                logical: (100.0, 10.0)
            }
        );
    }

    #[test]
    fn a_point_in_the_origin_body_is_inside_the_origin() {
        assert_eq!(
            locate((100.0, 300.0), 1, &two(), BAR),
            PointOver::InsideWindow(1)
        );
    }

    #[test]
    fn the_bar_ends_at_its_height() {
        assert_eq!(
            locate((100.0, 40.0), 1, &two(), BAR),
            PointOver::InsideWindow(1)
        );
        assert!(matches!(
            locate((100.0, 39.9), 1, &two(), BAR),
            PointOver::OriginBar { .. }
        ));
    }

    #[test]
    fn a_point_above_the_origin_is_not_on_its_bar() {
        let only_origin = [window(1, (0, 100), (800, 600))];
        assert_eq!(
            locate((100.0, 90.0), 1, &only_origin, BAR),
            PointOver::Outside
        );
        // Above both windows of `two()`.
        assert_eq!(locate((1100.0, -5.0), 1, &two(), BAR), PointOver::Outside);
    }

    #[test]
    fn a_point_beside_the_origin_is_outside() {
        assert_eq!(locate((850.0, 10.0), 1, &two(), BAR), PointOver::Outside);
    }

    #[test]
    fn a_point_on_the_bar_of_another_window_is_that_bars() {
        assert_eq!(
            locate((1100.0, 10.0), 1, &two(), BAR),
            PointOver::OtherBar {
                window: 2,
                logical: (100.0, 10.0)
            }
        );
    }

    #[test]
    fn a_point_in_the_body_of_another_window_is_inside_it() {
        assert_eq!(
            locate((1100.0, 300.0), 1, &two(), BAR),
            PointOver::InsideWindow(2)
        );
    }

    #[test]
    fn the_other_bar_uses_the_scale_of_that_window() {
        let mut candidates = two();
        candidates[1].scale = 2.0;
        // 60 physical px down is 30 logical px in `2`: still on the bar.
        assert_eq!(
            locate((1100.0, 60.0), 1, &candidates, BAR),
            PointOver::OtherBar {
                window: 2,
                logical: (50.0, 30.0)
            }
        );
        // 90 physical px is 45 logical px: off the bar.
        assert_eq!(
            locate((1100.0, 90.0), 1, &candidates, BAR),
            PointOver::InsideWindow(2)
        );
    }

    #[test]
    fn of_two_overlapping_others_the_most_recently_focused_wins() {
        // Origin far away; `3` was focused after `2`: it comes first.
        let candidates = [
            window(1, (0, 0), (500, 500)),
            window(3, (1000, 0), (800, 600)),
            window(2, (1100, 0), (800, 600)),
        ];
        assert_eq!(
            locate((1200.0, 10.0), 1, &candidates, BAR),
            PointOver::OtherBar {
                window: 3,
                logical: (200.0, 10.0)
            }
        );
        // Only `2` holds this one.
        assert!(matches!(
            locate((1850.0, 10.0), 1, &candidates, BAR),
            PointOver::OtherBar { window: 2, .. }
        ));
    }

    #[test]
    fn a_minimized_window_is_ignored() {
        let mut candidates = two();
        candidates[1].minimized = true;
        assert_eq!(
            locate((1100.0, 10.0), 1, &candidates, BAR),
            PointOver::Outside
        );
    }

    #[test]
    fn a_minimized_window_does_not_hide_the_one_below_it() {
        let mut candidates = vec![window(3, (0, 0), (900, 700))];
        candidates[0].minimized = true;
        candidates.push(window(2, (0, 0), (800, 600)));
        assert!(matches!(
            locate((100.0, 10.0), 1, &candidates, BAR),
            PointOver::OtherBar { window: 2, .. }
        ));
    }

    #[test]
    fn an_other_window_gives_inside_even_on_its_top_strip() {
        let mut settings = window(9, (1000, 0), (800, 600));
        settings.kind = WindowKind::Other;
        let candidates = [window(1, (0, 0), (800, 600)), settings];
        assert_eq!(
            locate((1100.0, 10.0), 1, &candidates, BAR),
            PointOver::InsideWindow(9)
        );
    }

    #[test]
    fn a_minimized_other_window_is_ignored() {
        let mut settings = window(9, (1000, 0), (800, 600));
        settings.kind = WindowKind::Other;
        settings.minimized = true;
        let candidates = [window(1, (0, 0), (800, 600)), settings];
        assert_eq!(
            locate((1100.0, 10.0), 1, &candidates, BAR),
            PointOver::Outside
        );
    }

    #[test]
    fn a_terminal_window_beats_an_other_window_under_the_same_point() {
        let mut settings = window(9, (0, 0), (800, 600));
        settings.kind = WindowKind::Other;
        // `Other` first in focus order, but terminals decide first (rule 1).
        let candidates = [settings, window(2, (0, 0), (800, 600))];
        assert!(matches!(
            locate((100.0, 10.0), 1, &candidates, BAR),
            PointOver::OtherBar { window: 2, .. }
        ));
    }

    #[test]
    fn the_origin_on_top_of_another_window_wins_the_shared_point() {
        let candidates = [
            window(1, (0, 0), (800, 600)),
            window(2, (400, 0), (800, 600)),
        ];
        assert!(matches!(
            locate((500.0, 10.0), 1, &candidates, BAR),
            PointOver::OriginBar { .. }
        ));
        // Where the origin does not reach, `2` answers.
        assert!(matches!(
            locate((900.0, 10.0), 1, &candidates, BAR),
            PointOver::OtherBar { window: 2, .. }
        ));
    }

    #[test]
    fn nothing_under_the_point_is_outside() {
        assert_eq!(locate((5000.0, 5000.0), 1, &two(), BAR), PointOver::Outside);
        assert_eq!(locate((5.0, 5.0), 1, &[], BAR), PointOver::Outside);
    }

    // --- retarget ---

    fn bar_of(window: Win) -> PointOver<Win> {
        PointOver::OtherBar {
            window,
            logical: (12.0, 5.0),
        }
    }

    #[test]
    fn entering_another_bar_gives_it_the_ghost_and_leaves_the_origin_a_hole() {
        let r = retarget(None, &bar_of(2), 1);
        assert_eq!(r.clear, None);
        assert_eq!(r.set, Some((2, (12.0, 5.0))));
        assert!(r.elsewhere);
        assert_eq!(r.redraw, [1, 2], "the origin turns into a hole");
    }

    #[test]
    fn moving_inside_the_same_bar_only_redraws_it() {
        let r = retarget(Some(2), &bar_of(2), 1);
        assert_eq!(r.clear, None, "it stays the target");
        assert_eq!(r.set, Some((2, (12.0, 5.0))));
        assert!(r.elsewhere);
        assert_eq!(r.redraw, [2], "the origin is already a hole");
    }

    #[test]
    fn jumping_to_another_bar_takes_the_ghost_from_the_old_one() {
        let r = retarget(Some(2), &bar_of(3), 1);
        assert_eq!(r.clear, Some(2), "the old one loses `incoming`");
        assert_eq!(r.set, Some((3, (12.0, 5.0))));
        assert!(r.elsewhere);
        assert_eq!(r.redraw, [2, 3], "the old one redraws to erase the ghost");
    }

    #[test]
    fn leaving_for_the_desktop_or_a_body_brings_the_ghost_back_to_the_origin() {
        for now in [
            PointOver::Outside,
            PointOver::InsideWindow(2),
            PointOver::InsideWindow(1),
        ] {
            let r = retarget(Some(2), &now, 1);
            assert_eq!(r.clear, Some(2), "{now:?}");
            assert_eq!(r.set, None, "{now:?}");
            assert!(!r.elsewhere, "{now:?}");
            assert_eq!(r.redraw, [1, 2], "the origin shows the ghost again");
        }
    }

    #[test]
    fn coming_back_to_the_origin_bar_is_leaving_the_other_one() {
        let now = PointOver::OriginBar {
            logical: (3.0, 3.0),
        };
        let r = retarget(Some(2), &now, 1);
        assert_eq!(r.clear, Some(2));
        assert_eq!(r.set, None);
        assert!(!r.elsewhere);
        assert_eq!(r.redraw, [1, 2]);
    }

    #[test]
    fn nothing_changes_while_the_ghost_stays_on_the_origin() {
        let r = retarget(None, &PointOver::Outside, 1);
        assert_eq!(
            r,
            Retarget {
                clear: None,
                set: None,
                elsewhere: false,
                redraw: Vec::new(),
            }
        );
        let r = retarget(
            None,
            &PointOver::OriginBar {
                logical: (1.0, 1.0),
            },
            1,
        );
        assert!(r.redraw.is_empty() && r.set.is_none() && !r.elsewhere);
    }

    /// A cancel (`Esc`, focus lost) is the cursor leaving every bar: whoever
    /// had the ghost clears and redraws, and the origin is no longer a hole.
    #[test]
    fn a_cancel_clears_whoever_had_the_ghost() {
        let r = retarget(Some(3), &PointOver::Outside, 1);
        assert_eq!(r.clear, Some(3));
        assert!(!r.elsewhere);
        assert_eq!(r.redraw, [1, 3]);
        // With no target, a cancel has nothing to clear.
        assert_eq!(retarget(None, &PointOver::Outside, 1).clear, None);
    }

    // --- detach_position ---

    const MONITOR: MonitorRect = ((0, 0), (1920, 1080));

    #[test]
    fn detach_puts_the_grabbed_point_under_the_cursor() {
        // Cursor at (800, 500); the element is at x=10 on the new bar and was
        // held 30 px in; the press was 20 px down.
        let pos = detach_position(
            (800.0, 500.0),
            10.0,
            30.0,
            20.0,
            1.0,
            (1000, 700),
            Some(MONITOR),
            40.0,
        );
        assert_eq!(pos, (760, 480));
    }

    #[test]
    fn detach_without_a_monitor_is_not_clamped() {
        let pos = detach_position((10.0, 5.0), 10.0, 30.0, 20.0, 1.0, (1000, 700), None, 40.0);
        assert_eq!(pos, (-30, -15));
    }

    #[test]
    fn detach_is_clamped_at_the_right_edge_of_the_monitor() {
        let pos = detach_position(
            (1900.0, 500.0),
            10.0,
            30.0,
            20.0,
            1.0,
            (1000, 700),
            Some(MONITOR),
            40.0,
        );
        assert_eq!(pos, (920, 480), "the whole window stays inside");
    }

    #[test]
    fn detach_is_clamped_at_the_left_edge_of_the_monitor() {
        let pos = detach_position(
            (5.0, 500.0),
            10.0,
            30.0,
            20.0,
            1.0,
            (1000, 700),
            Some(((100, 0), (1920, 1080))),
            40.0,
        );
        assert_eq!(pos.0, 100);
    }

    #[test]
    fn detach_keeps_the_bar_from_leaving_the_top() {
        // Dropped 5 px from the top, held 20 px down: the raw y would be -15.
        let pos = detach_position(
            (800.0, 5.0),
            10.0,
            30.0,
            20.0,
            1.0,
            (1000, 700),
            Some(MONITOR),
            40.0,
        );
        assert_eq!(pos, (760, 0));
    }

    #[test]
    fn detach_keeps_the_bar_strip_inside_at_the_bottom() {
        // Dropped at y=1070: the raw y is 1050 and the strip would end at 1090.
        let pos = detach_position(
            (800.0, 1070.0),
            10.0,
            30.0,
            20.0,
            1.0,
            (1000, 700),
            Some(MONITOR),
            40.0,
        );
        assert_eq!(
            pos,
            (760, 1040),
            "the strip ends exactly at the monitor edge"
        );
    }

    #[test]
    fn detach_uses_the_scale_of_the_monitor_under_the_cursor() {
        // Scale 2.0: offsets are logical, the position is physical.
        let pos = detach_position(
            (1000.0, 800.0),
            10.0,
            30.0,
            20.0,
            2.0,
            (2000, 1400),
            Some(((0, 0), (3840, 2160))),
            40.0,
        );
        assert_eq!(pos, (1000 - 80, 800 - 40));
        // And the bar strip is 80 physical px: clamped at the bottom.
        let pos = detach_position(
            (1000.0, 2150.0),
            10.0,
            30.0,
            20.0,
            2.0,
            (2000, 1400),
            Some(((0, 0), (3840, 2160))),
            40.0,
        );
        assert_eq!(pos.1, 2160 - 80);
    }

    #[test]
    fn a_window_wider_than_the_monitor_is_left_aligned_to_it() {
        let pos = detach_position(
            (800.0, 500.0),
            10.0,
            30.0,
            20.0,
            1.0,
            (3000, 700),
            Some(((200, 0), (1920, 1080))),
            40.0,
        );
        assert_eq!(pos.0, 200);
    }

    #[test]
    fn detach_on_a_second_monitor_to_the_left_with_negative_origin() {
        let monitor = ((-1920, 0), (1920, 1080));
        let pos = detach_position(
            (-1000.0, 500.0),
            10.0,
            30.0,
            20.0,
            1.0,
            (1000, 700),
            Some(monitor),
            40.0,
        );
        assert_eq!(pos, (-1040, 480));
    }

    // --- monitor_containing ---

    #[test]
    fn the_monitor_holding_the_point_is_found() {
        let monitors = [((0, 0), (1920, 1080)), ((1920, 0), (2560, 1440))];
        assert_eq!(monitor_containing((10.0, 10.0), &monitors), Some(0));
        assert_eq!(monitor_containing((1919.9, 10.0), &monitors), Some(0));
        assert_eq!(monitor_containing((1920.0, 10.0), &monitors), Some(1));
        assert_eq!(monitor_containing((3000.0, 1400.0), &monitors), Some(1));
        assert_eq!(monitor_containing((3000.0, 1500.0), &monitors), None);
        assert_eq!(monitor_containing((-1.0, 10.0), &monitors), None);
        assert_eq!(monitor_containing((10.0, 10.0), &[]), None);
    }

    // --- decide_drop ---

    #[test]
    fn the_drop_is_decided_by_where_the_point_is() {
        use DropDecision::{Cancel, Detach, Local, Transfer};
        let logical = (10.0, 10.0);
        // 1. the bar of the origin: the local drop, whatever `in_origin_bar`
        //    says (the global point is the better witness).
        assert_eq!(
            decide_drop::<Win>(Some(PointOver::OriginBar { logical }), false),
            Local
        );
        // 2. the bar of another window: a transfer to it.
        assert_eq!(
            decide_drop(Some(PointOver::OtherBar { window: 7, logical }), false),
            Transfer(7)
        );
        // 3. inside a Porecatu window, off its bar: cancels.
        assert_eq!(decide_drop(Some(PointOver::InsideWindow(1)), true), Cancel);
        // 4. outside every window: detach.
        assert_eq!(decide_drop::<Win>(Some(PointOver::Outside), true), Detach);
    }

    #[test]
    fn without_a_global_point_only_the_local_drop_exists() {
        assert_eq!(decide_drop::<Win>(None, true), DropDecision::Local);
        assert_eq!(decide_drop::<Win>(None, false), DropDecision::Cancel);
    }

    // --- point_in_bar ---

    #[test]
    fn the_bar_of_the_drop_has_four_edges() {
        let (width, height) = (800.0, 40.0);
        assert!(point_in_bar((100.0, 10.0), width, height), "inside");
        assert!(point_in_bar((0.0, 0.0), width, height), "top-left corner");
        assert!(!point_in_bar((100.0, -1.0), width, height), "above");
        assert!(!point_in_bar((-1.0, 10.0), width, height), "left");
        assert!(!point_in_bar((800.0, 10.0), width, height), "right");
        assert!(!point_in_bar((100.0, 40.0), width, height), "below");
    }
}
