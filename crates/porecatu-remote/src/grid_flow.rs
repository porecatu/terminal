// SPDX-License-Identifier: GPL-3.0-or-later

//! Grid flow: last frame per pane, snapshots and diffs sent to subscribers.
//!
//! Pure and synchronous: no I/O, no `tokio`, no clock of its own (every call
//! takes `now_ms`). One [`Subscription`] exists per (client, pane) pair and
//! decides, each time it is polled, whether to send a `grid-snapshot`, a
//! `grid-diff` or nothing (PRD-025 RF-25.18, RF-25.19; ADR-0105 §3; protocol
//! spec 03 §5.3 to §5.5).
//!
//! # `sent` versus `latest`
//!
//! Two frames are in play and they are not the same thing:
//!
//! - `latest` is the newest frame the UI handed over for the pane. It is
//!   owned by the caller (one per pane, shared by all subscriptions) and is
//!   replaced at will: while the window is full, intermediate frames are
//!   simply dropped ("the last state wins", nothing is queued).
//! - `sent` is the state the *client* will hold once it has applied every
//!   message already sent on this subscription (ADR-0105 calls it "last
//!   sent"). It only changes when a message goes out.
//!
//! A diff is always computed from `sent` to `latest`, never from the previous
//! `latest`: the client applies diffs on top of what it was sent, so only the
//! difference to that state is correct. A diff that was never sent must not
//! be assumed applied.

use std::collections::VecDeque;
use std::sync::Arc;

use porecatu_protocol::message::{Color, GridDiff, GridSnapshot, Line, Message, Span, Style};

/// The latest frame of a pane, as built by the UI (`pane` and `seq` are
/// ignored: the subscription owns both).
pub(crate) type Frame = Arc<GridSnapshot>;

/// Most grid messages a subscription may have without a `grid-ack`.
pub(crate) const MAX_UNACKED: usize = 2;

/// Smallest gap between two grid messages of a subscription:
/// `ceil(1000 / 30)`, so at most 30 messages per second.
pub(crate) const MIN_INTERVAL_MS: u64 = 34;

/// A `grid-ack` the subscription cannot match to a message it sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AckError {
    /// The `seq` was never sent, or was already acknowledged: a protocol
    /// error of the client (the caller decides to close the session).
    UnknownSeq,
}

/// The state of one client's subscription to one pane.
#[derive(Debug)]
pub(crate) struct Subscription {
    pane: u64,
    /// `seq` of the next message; starts at 0 and never goes back.
    next_seq: u64,
    /// What the client holds after applying everything sent so far.
    sent: Option<Frame>,
    /// `seq` of messages sent and not yet acknowledged, oldest first.
    unacked: VecDeque<u64>,
    /// When the last grid message went out.
    last_send_ms: Option<u64>,
    /// The next message must be a snapshot (first send, or `resubscribe`).
    force_snapshot: bool,
}

impl Subscription {
    /// A fresh subscription: the first message will be a snapshot, `seq` 0.
    pub(crate) fn new(pane: u64) -> Self {
        Subscription {
            pane,
            next_seq: 0,
            sent: None,
            unacked: VecDeque::new(),
            last_send_ms: None,
            force_snapshot: true,
        }
    }

    /// The pane of this subscription.
    #[cfg(test)]
    pub(crate) fn pane(&self) -> u64 {
        self.pane
    }

    /// The client subscribed again to a pane it already subscribes: the next
    /// message is a snapshot with the next `seq` of the same subscription
    /// (spec 03 §5.1). The `seq` count and the unacknowledged window are kept.
    pub(crate) fn resubscribe(&mut self) {
        self.force_snapshot = true;
    }

    /// Decides what to send now, given the newest frame of the pane.
    ///
    /// Returns `None` when there is no frame, when two messages are already
    /// unacknowledged, when the last send was less than [`MIN_INTERVAL_MS`]
    /// ago, or when `latest` equals what the client already holds.
    pub(crate) fn poll(&mut self, latest: Option<&Frame>, now_ms: u64) -> Option<Message> {
        let latest = latest?;
        if self.unacked.len() >= MAX_UNACKED {
            return None;
        }
        if self.throttled(now_ms) {
            return None;
        }
        let message = match (&self.sent, self.force_snapshot) {
            (Some(sent), false) => {
                if same_state(sent, latest) {
                    return None;
                }
                if needs_snapshot(sent, latest) {
                    Message::GridSnapshot(self.snapshot(latest))
                } else {
                    Message::GridDiff(self.diff(sent, latest))
                }
            }
            _ => Message::GridSnapshot(self.snapshot(latest)),
        };
        self.unacked.push_back(self.next_seq);
        self.next_seq += 1;
        self.sent = Some(Arc::clone(latest));
        self.last_send_ms = Some(now_ms);
        self.force_snapshot = false;
        Some(message)
    }

    /// Records a `grid-ack`. Acknowledging is cumulative: the `seq` must be
    /// one of the pending ones and removes it together with every older one
    /// (the client acks each message in order, so normally it is the oldest).
    pub(crate) fn ack(&mut self, seq: u64) -> Result<(), AckError> {
        if !self.unacked.contains(&seq) {
            return Err(AckError::UnknownSeq);
        }
        while let Some(&front) = self.unacked.front() {
            if front > seq {
                break;
            }
            self.unacked.pop_front();
        }
        Ok(())
    }

    /// When to call [`poll`](Self::poll) again because of the rate cap: there
    /// is something to send and the window is open, but the last send was too
    /// recent. `None` when nothing is pending, or when the window is full (the
    /// `grid-ack` triggers the next poll, not the clock).
    pub(crate) fn next_deadline(&self, latest: Option<&Frame>, now_ms: u64) -> Option<u64> {
        let latest = latest?;
        if self.unacked.len() >= MAX_UNACKED {
            return None;
        }
        if let (Some(sent), false) = (&self.sent, self.force_snapshot)
            && same_state(sent, latest)
        {
            return None;
        }
        let earliest = self
            .last_send_ms
            .map_or(0, |last| last.saturating_add(MIN_INTERVAL_MS));
        Some(earliest.max(now_ms))
    }

    fn throttled(&self, now_ms: u64) -> bool {
        self.last_send_ms
            .is_some_and(|last| now_ms < last.saturating_add(MIN_INTERVAL_MS))
    }

    fn snapshot(&self, frame: &GridSnapshot) -> GridSnapshot {
        GridSnapshot {
            pane: self.pane,
            seq: self.next_seq,
            ..frame.clone()
        }
    }

    /// The diff from `sent` to `latest`: only the rows whose resolved form
    /// changed, with a styles table of its own.
    fn diff(&self, sent: &GridSnapshot, latest: &GridSnapshot) -> GridDiff {
        let mut styles: Vec<Style> = Vec::new();
        // Index in `latest.styles` -> index in the diff table.
        let mut remap: Vec<Option<u64>> = vec![None; latest.styles.len()];
        let mut lines = Vec::new();
        for (row, (old, new)) in sent.lines.iter().zip(&latest.lines).enumerate() {
            if line_eq(old, &sent.styles, new, &latest.styles) {
                continue;
            }
            lines.push((
                row as u64,
                reindex_line(new, &latest.styles, &mut remap, &mut styles),
            ));
        }
        GridDiff {
            pane: self.pane,
            seq: self.next_seq,
            history: latest.history,
            dropped: latest.dropped,
            styles,
            lines,
            cursor: latest.cursor,
            alt: latest.alt,
        }
    }
}

/// `true` when a diff cannot express the change: resize, alternate screen
/// toggled, or the history was cleared (spec 03 §5.3, §5.5).
fn needs_snapshot(sent: &GridSnapshot, latest: &GridSnapshot) -> bool {
    sent.cols != latest.cols
        || sent.rows != latest.rows
        || sent.alt != latest.alt
        || latest.dropped < sent.dropped
        || (latest.dropped == sent.dropped && latest.history < sent.history)
}

/// `true` when the client already shows `latest`, in everything a message
/// carries.
fn same_state(sent: &Frame, latest: &Frame) -> bool {
    if Arc::ptr_eq(sent, latest) {
        return true;
    }
    sent.cols == latest.cols
        && sent.rows == latest.rows
        && sent.history == latest.history
        && sent.dropped == latest.dropped
        && sent.alt == latest.alt
        && sent.cursor == latest.cursor
        && sent.lines.len() == latest.lines.len()
        && sent
            .lines
            .iter()
            .zip(&latest.lines)
            .all(|(a, b)| line_eq(a, &sent.styles, b, &latest.styles))
}

/// The style of a cell no span covers: default colors, no flags.
const DEFAULT_STYLE: Style = Style {
    fg: Color::Default,
    bg: Color::Default,
    flags: 0,
};

/// Resolves a span's style index; an out-of-range index (a bug of the
/// producer) resolves to the default style instead of panicking.
fn resolve(styles: &[Style], index: u64) -> Style {
    usize::try_from(index)
        .ok()
        .and_then(|i| styles.get(i))
        .copied()
        .unwrap_or(DEFAULT_STYLE)
}

fn span_eq(a: &Span, a_styles: &[Style], b: &Span, b_styles: &[Style]) -> bool {
    a.col == b.col
        && a.width == b.width
        && a.text == b.text
        && resolve(a_styles, a.style) == resolve(b_styles, b.style)
}

/// Lines are equal when their *resolved* form is: spans compared with the
/// style looked up in each message's own table, never by index.
fn line_eq(a: &Line, a_styles: &[Style], b: &Line, b_styles: &[Style]) -> bool {
    a.wrapped == b.wrapped
        && a.spans.len() == b.spans.len()
        && a.spans
            .iter()
            .zip(&b.spans)
            .all(|(x, y)| span_eq(x, a_styles, y, b_styles))
}

/// Copies `line` with its spans pointing into the diff's own styles table,
/// adding each used style on first use.
fn reindex_line(
    line: &Line,
    source: &[Style],
    remap: &mut [Option<u64>],
    table: &mut Vec<Style>,
) -> Line {
    let spans = line
        .spans
        .iter()
        .map(|span| {
            let cached = usize::try_from(span.style)
                .ok()
                .and_then(|i| remap.get(i).copied().flatten());
            let style = cached.unwrap_or_else(|| {
                let resolved = resolve(source, span.style);
                let index = table
                    .iter()
                    .position(|s| *s == resolved)
                    .unwrap_or_else(|| {
                        table.push(resolved);
                        table.len() - 1
                    }) as u64;
                if let Some(slot) = usize::try_from(span.style)
                    .ok()
                    .and_then(|i| remap.get_mut(i))
                {
                    *slot = Some(index);
                }
                index
            });
            Span {
                style,
                ..span.clone()
            }
        })
        .collect();
    Line {
        wrapped: line.wrapped,
        spans,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use porecatu_protocol::message::{Cursor, CursorShape, SpanText};

    const COLS: u64 = 10;
    const ROWS: u64 = 3;

    fn style(n: u8) -> Style {
        Style {
            fg: Color::Indexed(n),
            bg: Color::Default,
            flags: 0,
        }
    }

    fn span(col: u64, text: &str, style: u64) -> Span {
        Span {
            col,
            width: 1,
            style,
            text: SpanText::Plain(text.to_owned()),
        }
    }

    fn empty() -> Line {
        Line {
            wrapped: false,
            spans: Vec::new(),
        }
    }

    fn cursor(row: u64, col: u64) -> Cursor {
        Cursor {
            row,
            col,
            shape: CursorShape::Block,
            visible: true,
        }
    }

    fn base() -> GridSnapshot {
        GridSnapshot {
            pane: 0,
            seq: 0,
            cols: COLS,
            rows: ROWS,
            history: 0,
            dropped: 0,
            styles: vec![style(1)],
            lines: vec![
                Line {
                    wrapped: false,
                    spans: vec![span(0, "ab", 0)],
                },
                empty(),
                empty(),
            ],
            cursor: cursor(0, 2),
            alt: false,
        }
    }

    fn frame(f: impl FnOnce(&mut GridSnapshot)) -> Frame {
        let mut g = base();
        f(&mut g);
        Arc::new(g)
    }

    fn snapshot_of(m: Option<Message>) -> GridSnapshot {
        match m {
            Some(Message::GridSnapshot(s)) => s,
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }

    fn diff_of(m: Option<Message>) -> GridDiff {
        match m {
            Some(Message::GridDiff(d)) => d,
            other => panic!("expected a diff, got {other:?}"),
        }
    }

    /// A subscription past its first snapshot (seq 0 sent and acknowledged at
    /// t = 0), plus the frame the client holds.
    fn primed() -> (Subscription, Frame) {
        let mut sub = Subscription::new(7);
        let f = frame(|_| {});
        snapshot_of(sub.poll(Some(&f), 0));
        sub.ack(0).unwrap();
        (sub, f)
    }

    #[test]
    fn first_message_is_snapshot_seq_zero() {
        let mut sub = Subscription::new(7);
        let f = frame(|_| {});
        let s = snapshot_of(sub.poll(Some(&f), 100));
        assert_eq!((s.pane, s.seq), (7, 0));
        assert_eq!(s.lines, f.lines);
        assert_eq!(s.styles, f.styles);
        assert_eq!(s.cursor, f.cursor);
        assert_eq!((s.cols, s.rows, s.history, s.dropped), (COLS, ROWS, 0, 0));
        assert_eq!(sub.pane(), 7);
    }

    #[test]
    fn no_frame_no_message() {
        let mut sub = Subscription::new(1);
        assert!(sub.poll(None, 0).is_none());
        assert!(sub.next_deadline(None, 0).is_none());
    }

    #[test]
    fn equal_frame_sends_nothing() {
        let (mut sub, _) = primed();
        let same = frame(|_| {});
        assert!(sub.poll(Some(&same), 1000).is_none());
        assert!(sub.next_deadline(Some(&same), 1000).is_none());
    }

    #[test]
    fn style_index_renumbering_alone_is_not_a_change() {
        let (mut sub, _) = primed();
        // Same resolved content, different table layout.
        let other = frame(|g| {
            g.styles = vec![style(9), style(1)];
            g.lines[0].spans[0].style = 1;
        });
        assert!(sub.poll(Some(&other), 1000).is_none());
    }

    #[test]
    fn one_changed_line_gives_a_diff_with_only_that_line() {
        let (mut sub, _) = primed();
        let changed = frame(|g| {
            g.styles = vec![style(1), style(2), style(3)];
            // Row 1 uses the third style of the source table only.
            g.lines[1] = Line {
                wrapped: true,
                spans: vec![span(0, "x", 2), span(1, "y", 2)],
            };
            g.cursor = cursor(1, 2);
        });
        let d = diff_of(sub.poll(Some(&changed), 1000));
        assert_eq!((d.pane, d.seq), (7, 1));
        assert_eq!(d.lines.len(), 1);
        let (row, line) = &d.lines[0];
        assert_eq!(*row, 1);
        assert!(line.wrapped);
        // Table of its own: just the one style used, reindexed to 0.
        assert_eq!(d.styles, vec![style(3)]);
        assert!(line.spans.iter().all(|s| s.style == 0));
        assert_eq!(d.cursor, cursor(1, 2));
    }

    #[test]
    fn diff_styles_are_deduplicated_and_ordered_by_use() {
        let (mut sub, _) = primed();
        let changed = frame(|g| {
            g.styles = vec![style(1), style(2), style(3)];
            g.lines[1] = Line {
                wrapped: false,
                spans: vec![span(0, "a", 2), span(1, "b", 1), span(2, "c", 2)],
            };
        });
        let d = diff_of(sub.poll(Some(&changed), 1000));
        assert_eq!(d.styles, vec![style(3), style(2)]);
        let idx: Vec<u64> = d.lines[0].1.spans.iter().map(|s| s.style).collect();
        assert_eq!(idx, vec![0, 1, 0]);
    }

    #[test]
    fn cursor_only_change_is_a_diff_without_lines() {
        let (mut sub, _) = primed();
        let moved = frame(|g| g.cursor = cursor(2, 5));
        let d = diff_of(sub.poll(Some(&moved), 1000));
        assert!(d.lines.is_empty());
        assert!(d.styles.is_empty());
        assert_eq!(d.cursor, cursor(2, 5));
    }

    #[test]
    fn history_and_dropped_growth_are_diffs() {
        let (mut sub, _) = primed();
        let grown = frame(|g| {
            g.history = 5;
            g.dropped = 2;
        });
        let d = diff_of(sub.poll(Some(&grown), 1000));
        assert_eq!((d.history, d.dropped), (5, 2));
        assert!(d.lines.is_empty());
    }

    #[test]
    fn full_window_holds_and_ack_releases_one_diff_with_everything() {
        let (mut sub, _) = primed();
        // seq 1 and seq 2 go out, nothing acknowledged.
        let f1 = frame(|g| {
            g.lines[1] = Line {
                wrapped: false,
                spans: vec![span(0, "1", 0)],
            }
        });
        let f2 = frame(|g| {
            g.lines[2] = Line {
                wrapped: false,
                spans: vec![span(0, "2", 0)],
            }
        });
        assert_eq!(diff_of(sub.poll(Some(&f1), 100)).seq, 1);
        assert_eq!(diff_of(sub.poll(Some(&f2), 200)).seq, 2);
        // Window full: later frames are held, whatever the time.
        let f3 = frame(|g| {
            g.lines[0] = Line {
                wrapped: false,
                spans: vec![span(0, "A", 0)],
            };
            g.lines[1] = Line {
                wrapped: false,
                spans: vec![span(0, "1", 0)],
            };
            g.lines[2] = Line {
                wrapped: false,
                spans: vec![span(0, "2", 0)],
            };
        });
        let f4 = frame(|g| {
            g.lines[0] = Line {
                wrapped: false,
                spans: vec![span(0, "A", 0)],
            };
            g.lines[1] = Line {
                wrapped: false,
                spans: vec![span(0, "B", 0)],
            };
            g.lines[2] = Line {
                wrapped: false,
                spans: vec![span(0, "2", 0)],
            };
        });
        assert!(sub.poll(Some(&f3), 10_000).is_none());
        assert!(sub.poll(Some(&f4), 20_000).is_none());
        assert!(sub.next_deadline(Some(&f4), 20_000).is_none());
        // One ack opens one slot: one diff against `sent` (= f2), from the
        // newest frame only (f4), covering rows 0 and 1.
        sub.ack(1).unwrap();
        let d = diff_of(sub.poll(Some(&f4), 20_000));
        assert_eq!(d.seq, 3);
        let rows: Vec<u64> = d.lines.iter().map(|(r, _)| *r).collect();
        assert_eq!(rows, vec![0, 1]);
        // Window is full again (2 and 3 pending).
        assert!(sub.poll(Some(&f4), 30_000).is_none());
    }

    #[test]
    fn rate_cap_blocks_two_frames_ten_ms_apart() {
        let (mut sub, _) = primed();
        let f1 = frame(|g| g.cursor = cursor(1, 1));
        let f2 = frame(|g| g.cursor = cursor(2, 2));
        assert_eq!(diff_of(sub.poll(Some(&f1), 1000)).seq, 1);
        sub.ack(1).unwrap();
        // 10 ms later: held, and the deadline is exactly last send + 34 ms.
        assert!(sub.poll(Some(&f2), 1010).is_none());
        assert_eq!(sub.next_deadline(Some(&f2), 1010), Some(1034));
        assert!(sub.poll(Some(&f2), 1033).is_none());
        let d = diff_of(sub.poll(Some(&f2), 1034));
        assert_eq!((d.seq, d.cursor), (2, cursor(2, 2)));
    }

    #[test]
    fn deadline_is_now_when_the_interval_has_passed() {
        let (sub, _) = primed();
        let f = frame(|g| g.cursor = cursor(1, 1));
        assert_eq!(sub.next_deadline(Some(&f), 5000), Some(5000));
        // A new subscription has no last send: due right away.
        let fresh = Subscription::new(1);
        assert_eq!(fresh.next_deadline(Some(&f), 12), Some(12));
    }

    #[test]
    fn at_most_thirty_messages_in_one_second() {
        let (mut sub, _) = primed();
        let mut sent = 0;
        for t in 0..1000u64 {
            let f = frame(|g| g.cursor = cursor(t % ROWS, t % COLS));
            if sub.poll(Some(&f), 10_000 + t).is_some() {
                sent += 1;
                sub.ack(sub.next_seq - 1).unwrap();
            }
        }
        assert!(sent <= 30, "sent {sent}");
        assert!(sent >= 29, "sent {sent}");
    }

    #[test]
    fn resize_sends_a_snapshot() {
        let (mut sub, _) = primed();
        let bigger = frame(|g| {
            g.cols = 20;
            g.rows = 4;
            g.lines.push(empty());
        });
        let s = snapshot_of(sub.poll(Some(&bigger), 1000));
        assert_eq!((s.seq, s.cols, s.rows), (1, 20, 4));
    }

    #[test]
    fn alternate_screen_toggle_sends_a_snapshot() {
        let (mut sub, _) = primed();
        let alt = frame(|g| g.alt = true);
        assert_eq!(snapshot_of(sub.poll(Some(&alt), 1000)).seq, 1);
        sub.ack(1).unwrap();
        let back = frame(|_| {});
        assert_eq!(snapshot_of(sub.poll(Some(&back), 2000)).seq, 2);
    }

    #[test]
    fn cleared_history_sends_a_snapshot() {
        let mut sub = Subscription::new(1);
        let f = frame(|g| {
            g.history = 10;
            g.dropped = 4;
        });
        snapshot_of(sub.poll(Some(&f), 0));
        sub.ack(0).unwrap();
        // `dropped` went back: history cleared.
        let cleared = frame(|g| {
            g.history = 0;
            g.dropped = 0;
        });
        assert_eq!(snapshot_of(sub.poll(Some(&cleared), 100)).seq, 1);
        sub.ack(1).unwrap();
        // `dropped` equal and `history` smaller: also cleared.
        let g1 = frame(|g| g.history = 8);
        assert_eq!(diff_of(sub.poll(Some(&g1), 200)).seq, 2);
        sub.ack(2).unwrap();
        let g2 = frame(|g| g.history = 3);
        assert_eq!(snapshot_of(sub.poll(Some(&g2), 300)).seq, 3);
    }

    #[test]
    fn history_shrinking_with_more_dropped_is_a_diff() {
        // Scrollback at its cap: lines scroll off the top, `history` stays
        // and `dropped` grows. Not a clear.
        let mut sub = Subscription::new(1);
        let f = frame(|g| {
            g.history = 10;
            g.dropped = 4;
        });
        snapshot_of(sub.poll(Some(&f), 0));
        sub.ack(0).unwrap();
        let next = frame(|g| {
            g.history = 10;
            g.dropped = 5;
        });
        assert_eq!(diff_of(sub.poll(Some(&next), 100)).dropped, 5);
    }

    #[test]
    fn resubscribe_sends_a_snapshot_with_the_next_seq() {
        let (mut sub, f) = primed();
        sub.resubscribe();
        // Same frame as the client holds: still a snapshot, because it is
        // the way the client asks for the full state again.
        let s = snapshot_of(sub.poll(Some(&f), 1000));
        assert_eq!((s.pane, s.seq), (7, 1));
        assert_eq!(s.lines, f.lines);
        // Back to normal afterwards.
        sub.ack(1).unwrap();
        assert!(sub.poll(Some(&f), 2000).is_none());
        let moved = frame(|g| g.cursor = cursor(1, 1));
        assert_eq!(diff_of(sub.poll(Some(&moved), 3000)).seq, 2);
    }

    #[test]
    fn resubscribe_keeps_the_pending_window() {
        let (mut sub, f) = primed();
        let f1 = frame(|g| g.cursor = cursor(1, 1));
        let f2 = frame(|g| g.cursor = cursor(2, 2));
        sub.poll(Some(&f1), 100).unwrap();
        sub.poll(Some(&f2), 200).unwrap();
        sub.resubscribe();
        assert!(sub.poll(Some(&f), 5000).is_none());
        sub.ack(1).unwrap();
        assert_eq!(snapshot_of(sub.poll(Some(&f), 5000)).seq, 3);
    }

    #[test]
    fn unknown_ack_is_an_error() {
        let mut sub = Subscription::new(1);
        assert_eq!(sub.ack(0), Err(AckError::UnknownSeq));
        let f = frame(|_| {});
        sub.poll(Some(&f), 0).unwrap();
        assert_eq!(sub.ack(5), Err(AckError::UnknownSeq));
        assert_eq!(sub.ack(0), Ok(()));
        // Already acknowledged.
        assert_eq!(sub.ack(0), Err(AckError::UnknownSeq));
    }

    #[test]
    fn ack_is_cumulative() {
        let (mut sub, _) = primed();
        let f1 = frame(|g| g.cursor = cursor(1, 1));
        let f2 = frame(|g| g.cursor = cursor(2, 2));
        sub.poll(Some(&f1), 100).unwrap();
        sub.poll(Some(&f2), 200).unwrap();
        // Acking the newest clears both.
        assert_eq!(sub.ack(2), Ok(()));
        assert_eq!(sub.ack(1), Err(AckError::UnknownSeq));
        let f3 = frame(|g| g.cursor = cursor(0, 0));
        assert_eq!(diff_of(sub.poll(Some(&f3), 300)).seq, 3);
    }

    #[test]
    fn diff_only_copies_lines_of_the_frame() {
        // Span rules are the producer's: the diff keeps wrapped, width, text
        // (including grapheme spans) and columns exactly as in the frame.
        let (mut sub, _) = primed();
        let changed = frame(|g| {
            g.lines[2] = Line {
                wrapped: true,
                spans: vec![
                    Span {
                        col: 0,
                        width: 2,
                        style: 0,
                        text: SpanText::Plain("\u{4e2d}\u{6587}".to_owned()),
                    },
                    Span {
                        col: 4,
                        width: 2,
                        style: 0,
                        text: SpanText::Grapheme("\u{1f468}\u{200d}\u{1f469}".to_owned()),
                    },
                ],
            };
        });
        let d = diff_of(sub.poll(Some(&changed), 1000));
        assert_eq!(d.lines.len(), 1);
        assert_eq!(d.lines[0].0, 2);
        let mut expected = changed.lines[2].clone();
        for s in &mut expected.spans {
            s.style = 0; // only style used is `styles[0]`, already index 0
        }
        assert_eq!(d.lines[0].1, expected);
        assert_eq!(d.styles, vec![style(1)]);
    }

    #[test]
    fn same_arc_is_equal_without_comparing() {
        let (mut sub, f) = primed();
        assert!(sub.poll(Some(&f), 1000).is_none());
    }
}
