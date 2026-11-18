// SPDX-License-Identifier: GPL-3.0-or-later

//! Modal mask of a blocked window (ADR-0110): while a modal window is open in
//! front of it -- the settings screen in front of the terminal windows, the
//! profile editor in front of the settings screen -- the window behind is
//! covered by the dialogs' overlay color (`[dialog] overlay`), so it reads as
//! blocked. It fades in and out over `[appearance.window]
//! animation_collapse_ms` when `animations` is on, and switches at once when
//! it is off.
//!
//! Pure state with the `Instant` handed in from outside, like
//! [`crate::animation::AnimationClock`]: whoever paints calls
//! [`ModalMask::advance`] with the target and the time, and reads
//! [`ModalMask::opacity`]. While a fade runs, the window chains a redraw and
//! [`ModalMask::next_deadline`] goes into the `WaitUntil` (ADR-0022, ADR-0007:
//! no fade, no frame).

use std::time::{Duration, Instant};

use porecatu_render::{Color, Primitive, Quad, Rect};

use crate::animation::FRAME_INTERVAL;

#[derive(Debug, Clone, Copy)]
struct Fade {
    /// Opacity (0..=1) the fade started from: a fade reversed halfway starts
    /// where the mask is, not from the end.
    from: f32,
    started_at: Instant,
    duration: Duration,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ModalMask {
    /// The target: the window is blocked.
    on: bool,
    fade: Option<Fade>,
    /// The opacity of the last [`Self::advance`], the one painted.
    opacity: f32,
    /// A first `advance` already happened. The first one never fades: a
    /// window created while a modal is open is born masked.
    synced: bool,
}

impl ModalMask {
    /// Moves the mask toward `on` at `now`. `fade` is the duration of the
    /// transition (`Duration::ZERO` with `animations` off): a target that
    /// changes starts a fade from the current opacity.
    pub(crate) fn advance(&mut self, on: bool, now: Instant, fade: Duration) {
        let target = if on { 1.0 } else { 0.0 };
        if !self.synced {
            self.synced = true;
            self.on = on;
            self.opacity = target;
            return;
        }
        if on != self.on {
            let from = self.opacity;
            self.on = on;
            self.fade = (!fade.is_zero()).then_some(Fade {
                from,
                started_at: now,
                duration: fade,
            });
        }
        self.opacity = match self.fade {
            Some(f) => {
                let elapsed = now.saturating_duration_since(f.started_at);
                if elapsed >= f.duration {
                    self.fade = None;
                    target
                } else {
                    let progress = elapsed.as_secs_f32() / f.duration.as_secs_f32();
                    f.from + (target - f.from) * progress
                }
            }
            None => target,
        };
    }

    /// 0 (no mask) to 1 (the full overlay color), as of the last
    /// [`Self::advance`]. Painting goes through [`Self::paint`].
    #[cfg(test)]
    fn opacity(&self) -> f32 {
        self.opacity
    }

    /// A fade is running: the window asks for the next frame.
    pub(crate) fn is_fading(&self) -> bool {
        self.fade.is_some()
    }

    /// One frame ahead while a fade runs; `None` otherwise.
    pub(crate) fn next_deadline(&self, now: Instant) -> Option<Instant> {
        self.is_fading().then_some(now + FRAME_INTERVAL)
    }

    /// The mask over the whole window (logical size), in `color` scaled by
    /// the opacity; `None` with nothing to paint.
    pub(crate) fn paint(&self, width: f32, height: f32, color: Color) -> Option<Primitive> {
        (self.opacity > 0.0).then(|| {
            Primitive::Quad(Quad {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height,
                },
                color: Color {
                    a: color.a * f64::from(self.opacity),
                    ..color
                },
            })
        })
    }
}

/// Duration of the mask's fade: `[appearance.window] animation_collapse_ms`
/// (the app's short transition), zero with `animations = false`.
pub(crate) fn fade_duration(config: &porecatu_config::Config) -> Duration {
    let window = &config.appearance.window;
    if window.animations {
        Duration::from_millis(window.animation_collapse_ms)
    } else {
        Duration::ZERO
    }
}

/// The terminal windows are blocked by the open settings screen (ADR-0064):
/// only on Windows, where the OS disables them. Elsewhere they keep taking
/// input, and a mask would say they do not.
pub(crate) fn terminal_windows_blocked(settings_open: bool) -> bool {
    cfg!(windows) && settings_open
}

#[cfg(test)]
mod tests {
    use super::*;

    const FADE: Duration = Duration::from_millis(150);

    #[test]
    fn the_first_advance_is_instant() {
        let now = Instant::now();
        let mut mask = ModalMask::default();
        mask.advance(true, now, FADE);
        assert_eq!(mask.opacity(), 1.0);
        assert!(!mask.is_fading());
        assert_eq!(mask.next_deadline(now), None);
    }

    #[test]
    fn turning_on_fades_in_over_the_duration() {
        let now = Instant::now();
        let mut mask = ModalMask::default();
        mask.advance(false, now, FADE);
        assert_eq!(mask.opacity(), 0.0);
        mask.advance(true, now, FADE);
        assert_eq!(mask.opacity(), 0.0);
        assert_eq!(mask.next_deadline(now), Some(now + FRAME_INTERVAL));
        mask.advance(true, now + FADE / 2, FADE);
        assert!((mask.opacity() - 0.5).abs() < 1e-3);
        mask.advance(true, now + FADE, FADE);
        assert_eq!(mask.opacity(), 1.0);
        assert!(!mask.is_fading());
    }

    #[test]
    fn a_fade_reversed_halfway_starts_where_the_mask_is() {
        let now = Instant::now();
        let mut mask = ModalMask::default();
        mask.advance(false, now, FADE);
        mask.advance(true, now, FADE);
        mask.advance(true, now + FADE / 2, FADE);
        let halfway = mask.opacity();
        mask.advance(false, now + FADE / 2, FADE);
        assert!((mask.opacity() - halfway).abs() < 1e-6, "no jump");
        mask.advance(false, now + FADE, FADE);
        assert!(mask.opacity() < halfway && mask.opacity() > 0.0);
        mask.advance(false, now + FADE * 2, FADE);
        assert_eq!(mask.opacity(), 0.0);
    }

    #[test]
    fn without_animations_the_mask_switches_at_once() {
        let now = Instant::now();
        let mut mask = ModalMask::default();
        mask.advance(false, now, Duration::ZERO);
        mask.advance(true, now, Duration::ZERO);
        assert_eq!(mask.opacity(), 1.0);
        assert!(!mask.is_fading());
        mask.advance(false, now, Duration::ZERO);
        assert_eq!(mask.opacity(), 0.0);
    }

    #[test]
    fn the_paint_scales_the_overlay_alpha_and_is_absent_at_zero() {
        let now = Instant::now();
        let color = Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.4,
        };
        let mut mask = ModalMask::default();
        mask.advance(false, now, FADE);
        assert!(mask.paint(800.0, 600.0, color).is_none());
        mask.advance(true, now, FADE);
        mask.advance(true, now + FADE / 2, FADE);
        let Some(Primitive::Quad(quad)) = mask.paint(800.0, 600.0, color) else {
            panic!("a quad over the window");
        };
        assert_eq!((quad.rect.width, quad.rect.height), (800.0, 600.0));
        assert!((quad.color.a - 0.2).abs() < 1e-3);
    }

    #[test]
    fn fade_duration_follows_the_collapse_duration_and_the_animations_switch() {
        let mut config = porecatu_config::Config::default();
        assert_eq!(
            fade_duration(&config),
            Duration::from_millis(config.appearance.window.animation_collapse_ms)
        );
        config.appearance.window.animations = false;
        assert_eq!(fade_duration(&config), Duration::ZERO);
    }

    #[test]
    fn only_windows_masks_the_terminal_windows() {
        assert!(!terminal_windows_blocked(false));
        assert_eq!(terminal_windows_blocked(true), cfg!(windows));
    }
}
