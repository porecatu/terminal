// SPDX-License-Identifier: GPL-3.0-or-later

//! The pure half of the remote restore of a named session (ADR-0113
//! sections 1, 2 and 4; spec 03 sections 4.5 and 7).
//!
//! Nothing here touches a window, the `App` or the disk: [`named_sessions`]
//! turns the entries the disk gave (in the order of the popover) into the list
//! of the `sessions` message, and [`plan_restore_session`] decides, from plain
//! data and closures that stand for the world, what a `restore-session` is
//! (or why it is refused) and in which order the checks run. Reading the file
//! (`session-unreadable`) is not decided here: it comes from the read itself,
//! the last step, because it is the only one that touches the disk.

use std::hash::Hash;
use std::path::{Path, PathBuf};

use porecatu_remote::protocol::message::{ErrorCode, NamedSession, SessionsMode};
use porecatu_session::named::{EntryStatus, NamedSessionEntry};

use super::ids::ProtocolIds;

/// The list of the `sessions` message (ADR-0113 section 1): the order of
/// `entries` is kept (it is the order of the popover, newest first), each file
/// gets its protocol id (the same one while the process lives), and `readable`
/// is `true` only for an entry that reads clean. The `name` of an unreadable
/// entry is already the label of the file, as the popover shows it. The path
/// itself never leaves.
pub(crate) fn named_sessions<W: Copy + Eq + Hash>(
    entries: &[NamedSessionEntry],
    ids: &mut ProtocolIds<W>,
) -> Vec<NamedSession> {
    entries
        .iter()
        .map(|entry| NamedSession {
            session: ids.session(&entry.file),
            name: entry.name.clone(),
            readable: matches!(entry.status, EntryStatus::Ok),
        })
        .collect()
}

/// The `mode` of the `sessions` message, from the key
/// `[session] restore_named_in_current_window` read at the time of the answer.
pub(crate) const fn sessions_mode(restore_in_current_window: bool) -> SessionsMode {
    if restore_in_current_window {
        SessionsMode::Append
    } else {
        SessionsMode::NewWindow
    }
}

/// Where the restored session goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestoreTarget<W> {
    /// A new window, born without focus (ADR-0113 section 3).
    NewWindow,
    /// Appended to the end of the track of this window.
    Append(W),
}

/// What a valid `restore-session` is: the file (from the table of ids, never
/// from the phone) and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RestorePlan<W> {
    pub file: PathBuf,
    pub target: RestoreTarget<W>,
}

/// Validates a `restore-session` in the order of ADR-0113 section 4 (the
/// first that fails answers):
///
/// 1. `unknown-session`: the id is not in the table, or the file is gone;
/// 2. `unknown-window`: `window` came and does not resolve -- in both modes;
/// 3. `busy`: in `append`, with a window, `window_busy(window)`; in
///    `new-window`, `global_busy` (a drag between windows in progress);
/// 4. `invalid-argument`: `append` without `window`;
/// 5. otherwise the plan. In `new-window` a `window` that came is ignored.
///
/// `append_mode` is the key read at the time of the action. `session-unreadable`
/// is the read of the file, after this plan.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_restore_session<W: Copy>(
    session: u64,
    window: Option<u64>,
    append_mode: bool,
    session_file: impl Fn(u64) -> Option<PathBuf>,
    file_exists: impl Fn(&Path) -> bool,
    resolve_window: impl Fn(u64) -> Option<W>,
    window_busy: impl Fn(W) -> bool,
    global_busy: bool,
) -> Result<RestorePlan<W>, ErrorCode> {
    let file = session_file(session)
        .filter(|file| file_exists(file))
        .ok_or(ErrorCode::UnknownSession)?;
    let window = window
        .map(|id| resolve_window(id).ok_or(ErrorCode::UnknownWindow))
        .transpose()?;
    let target = if append_mode {
        match window {
            Some(window) if window_busy(window) => return Err(ErrorCode::Busy),
            Some(window) => RestoreTarget::Append(window),
            None => return Err(ErrorCode::InvalidArgument),
        }
    } else {
        if global_busy {
            return Err(ErrorCode::Busy);
        }
        RestoreTarget::NewWindow
    };
    Ok(RestorePlan { file, target })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, file: &str, status: EntryStatus) -> NamedSessionEntry {
        NamedSessionEntry {
            name: name.to_owned(),
            file: PathBuf::from(file),
            saved_at: Some(1),
            status,
        }
    }

    #[test]
    fn the_list_keeps_the_order_and_says_which_entries_are_readable() {
        let entries = [
            entry("zeta", "z.json", EntryStatus::Ok),
            entry("broken.json", "b.json", EntryStatus::Unreadable),
            entry("newer", "n.json", EntryStatus::NewerSchema { found: 9 }),
            entry("alpha", "a.json", EntryStatus::Ok),
        ];
        let mut ids = ProtocolIds::<u8>::new();
        let list = named_sessions(&entries, &mut ids);
        let shape: Vec<(&str, bool)> = list.iter().map(|s| (s.name.as_str(), s.readable)).collect();
        assert_eq!(
            shape,
            [
                ("zeta", true),
                ("broken.json", false),
                ("newer", false),
                ("alpha", true)
            ]
        );
        // First listed, first numbered; all different.
        let numbers: Vec<u64> = list.iter().map(|s| s.session).collect();
        assert_eq!(numbers, [1, 2, 3, 4]);
    }

    #[test]
    fn the_ids_are_stable_between_two_lists() {
        let mut ids = ProtocolIds::<u8>::new();
        let first = named_sessions(
            &[
                entry("a", "a.json", EntryStatus::Ok),
                entry("b", "b.json", EntryStatus::Ok),
            ],
            &mut ids,
        );
        // Another order, a new file in the middle, one file gone.
        let second = named_sessions(
            &[
                entry("b", "b.json", EntryStatus::Ok),
                entry("c", "c.json", EntryStatus::Ok),
                entry("a", "a.json", EntryStatus::Ok),
            ],
            &mut ids,
        );
        assert_eq!(second[0].session, first[1].session);
        assert_eq!(second[2].session, first[0].session);
        assert!(second[1].session > first[1].session);
        assert_eq!(ids.session_of(second[1].session), Some(Path::new("c.json")));
        assert!(named_sessions(&[], &mut ids).is_empty());
    }

    #[test]
    fn the_mode_follows_the_key() {
        assert_eq!(sessions_mode(true), SessionsMode::Append);
        assert_eq!(sessions_mode(false), SessionsMode::NewWindow);
    }

    // --- plan_restore_session -----------------------------------------------

    /// The world of the tests: session 1 is `a.json` (exists), 2 is `gone.json`
    /// (listed once, deleted since); windows 10 and 11 exist, 11 is busy.
    struct World {
        global_busy: bool,
    }

    impl World {
        fn plan(
            &self,
            session: u64,
            window: Option<u64>,
            append: bool,
        ) -> Result<RestorePlan<u64>, ErrorCode> {
            plan_restore_session(
                session,
                window,
                append,
                |id| match id {
                    1 => Some(PathBuf::from("a.json")),
                    2 => Some(PathBuf::from("gone.json")),
                    _ => None,
                },
                |file| file == Path::new("a.json"),
                |id| matches!(id, 10 | 11).then_some(id),
                |w| w == 11,
                self.global_busy,
            )
        }
    }

    fn calm() -> World {
        World { global_busy: false }
    }

    fn new_window_plan() -> RestorePlan<u64> {
        RestorePlan {
            file: PathBuf::from("a.json"),
            target: RestoreTarget::NewWindow,
        }
    }

    #[test]
    fn restore_session_checks_the_session_first() {
        let world = World { global_busy: true };
        // Unknown id and file gone: both `unknown-session`, ahead of the
        // unknown window, the busy window and the missing window.
        for append in [false, true] {
            assert_eq!(
                world.plan(99, Some(77), append),
                Err(ErrorCode::UnknownSession)
            );
            assert_eq!(
                world.plan(2, Some(77), append),
                Err(ErrorCode::UnknownSession)
            );
            assert_eq!(world.plan(2, None, append), Err(ErrorCode::UnknownSession));
        }
    }

    #[test]
    fn restore_session_checks_the_window_before_busy() {
        // An unknown window beats the busy ones, in both modes.
        let world = World { global_busy: true };
        assert_eq!(world.plan(1, Some(77), true), Err(ErrorCode::UnknownWindow));
        assert_eq!(
            world.plan(1, Some(77), false),
            Err(ErrorCode::UnknownWindow)
        );
    }

    #[test]
    fn restore_session_checks_busy_before_invalid_argument() {
        // Append with a busy window: busy.
        assert_eq!(calm().plan(1, Some(11), true), Err(ErrorCode::Busy));
        // New window while a drag between windows is going on: busy.
        let world = World { global_busy: true };
        assert_eq!(world.plan(1, None, false), Err(ErrorCode::Busy));
        assert_eq!(world.plan(1, Some(10), false), Err(ErrorCode::Busy));
        // Append without a window has no window to be busy: invalid, even
        // with the drag going on (that one only matters to a new window).
        assert_eq!(world.plan(1, None, true), Err(ErrorCode::InvalidArgument));
        assert_eq!(calm().plan(1, None, true), Err(ErrorCode::InvalidArgument));
    }

    #[test]
    fn a_busy_window_does_not_matter_to_a_new_window() {
        // The mode `new-window` ignores `window` (but still resolves it).
        assert_eq!(calm().plan(1, Some(11), false), Ok(new_window_plan()));
    }

    #[test]
    fn a_new_window_ignores_a_valid_window() {
        assert_eq!(calm().plan(1, Some(10), false), Ok(new_window_plan()));
        assert_eq!(calm().plan(1, None, false), Ok(new_window_plan()));
    }

    #[test]
    fn append_with_a_free_window_targets_it() {
        assert_eq!(
            calm().plan(1, Some(10), true),
            Ok(RestorePlan {
                file: PathBuf::from("a.json"),
                target: RestoreTarget::Append(10),
            })
        );
    }

    #[test]
    fn the_file_comes_from_the_table_not_from_the_phone() {
        // Whatever the id, the plan carries the path the table has.
        let plan = calm().plan(1, None, false).unwrap();
        assert_eq!(plan.file, PathBuf::from("a.json"));
    }
}
