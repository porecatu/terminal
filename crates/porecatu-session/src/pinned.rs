// SPDX-License-Identifier: GPL-3.0-or-later

//! The pinned-tab list file, `pinned.json` (ADR-0086 §3, PRD-021).
//!
//! It sits next to `session.json` and its path is **derived** from the
//! resolved session path (the same way `named::named_sessions_dir` is), so
//! `PORECATU_SESSION` moves all of them together.
//!
//! Why it is its own file, with its own rules:
//!
//! - **Written only by a gesture** (pin, unpin, close a pinned tab, reorder
//!   the zone), never derived from live state. Quitting the app or closing
//!   a window does not touch it, so the file on disk is always the last
//!   gesture, not the last state. This module only offers the primitives;
//!   *when* to call [`save_pinned_to`] is the UI's rule.
//! - **Fails closed.** A file that cannot be read as a valid list goes to
//!   quarantine (`.corrupt`, `.corrupt.1`, ... -- never overwriting an
//!   earlier one) and the caller starts with an empty list, never a partial
//!   one. The pinned list is also what authorizes a `.porecatu` to run
//!   (ADR-0086 §6), so a half-read list must not exist.
//! - **Never overwrites a newer schema.** A file whose `schema_version` is
//!   newer than [`PINNED_SCHEMA_VERSION`] is reported and left byte for
//!   byte untouched; the caller must treat the registry as read-only.
//!
//! Each tab is the very same [`TabV1`] the session writes, so the session
//! and the pinned list change together when the tab schema does. Atomic
//! write and quarantine are the `session.json` ones (RF-3.5, RF-3.14).

use std::path::{Path, PathBuf};
use std::{fs, io};

use serde::{Deserialize, Serialize};

use crate::schema::v1::TabV1;
use crate::{path, rename_to_first_free_corrupt, write_json_atomic};

/// Schema version this binary writes and reads without migration.
pub const PINNED_SCHEMA_VERSION: u32 = 1;

/// File name of the list, next to `session.json`.
pub const PINNED_FILE_NAME: &str = "pinned.json";

/// The envelope of `pinned.json`: its own version, a list of the session's
/// own `TabV1` (with `panes` always present when written by this binary).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PinnedFileV1 {
    pub schema_version: u32,
    #[serde(default)]
    pub tabs: Vec<TabV1>,
}

impl Default for PinnedFileV1 {
    fn default() -> Self {
        Self {
            schema_version: PINNED_SCHEMA_VERSION,
            tabs: Vec::new(),
        }
    }
}

/// What [`load_pinned_from`] found.
#[derive(Debug, Clone, PartialEq)]
pub enum PinnedLoadOutcome {
    /// No file (or one that could not be read at all): an empty list, not
    /// an error (RF-21.29).
    Missing,
    /// A valid list.
    Loaded(PinnedFileV1),
    /// Invalid file: renamed to `path` (a `.corrupt[.N]` next to it) and
    /// kept there; the list starts empty.
    Quarantined { path: PathBuf },
    /// `schema_version` newer than this binary supports: the file is
    /// untouched, the list starts empty and read-only.
    NewerSchema { found: u32, supported: u32 },
}

/// Path of `pinned.json`, derived from the resolved session path
/// (ADR-0086 §3). `None` when no session path can be resolved.
pub fn pinned_path() -> Option<PathBuf> {
    pinned_path_from(&path::resolve_session_path()?)
}

/// Pure core of [`pinned_path`]: `pinned.json` in the directory of
/// `session_path`, so it can be tested without touching `std::env`.
pub(crate) fn pinned_path_from(session_path: &Path) -> Option<PathBuf> {
    Some(path::session_dir(session_path)?.join(PINNED_FILE_NAME))
}

/// [`load_pinned_from`] at [`pinned_path`]; `Missing` without a path.
pub fn load_pinned() -> PinnedLoadOutcome {
    match pinned_path() {
        Some(path) => load_pinned_from(&path),
        None => PinnedLoadOutcome::Missing,
    }
}

/// Reads `path`. Missing file, or an I/O error that is not "not found",
/// is [`PinnedLoadOutcome::Missing`] (as for the session: there is no good
/// file to preserve). A newer schema is checked before anything else is
/// parsed, so a file this binary does not understand is never judged
/// invalid.
pub fn load_pinned_from(path: &Path) -> PinnedLoadOutcome {
    let Ok(bytes) = fs::read(path) else {
        return PinnedLoadOutcome::Missing;
    };

    let Ok(raw) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return quarantine(path);
    };

    // A missing, non-numeric or zero version is invalid, not "old".
    let version = raw
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if version == 0 {
        return quarantine(path);
    }
    if version > u64::from(PINNED_SCHEMA_VERSION) {
        return PinnedLoadOutcome::NewerSchema {
            found: u32::try_from(version).unwrap_or(u32::MAX),
            supported: PINNED_SCHEMA_VERSION,
        };
    }

    match serde_json::from_value::<PinnedFileV1>(raw) {
        Ok(file) => PinnedLoadOutcome::Loaded(file),
        Err(_) => quarantine(path),
    }
}

/// Moves an invalid file aside. If even that fails (the file vanished
/// between the read and now) there is nothing to preserve: `Missing`.
fn quarantine(path: &Path) -> PinnedLoadOutcome {
    match rename_to_first_free_corrupt(path) {
        Ok(corrupt) => PinnedLoadOutcome::Quarantined { path: corrupt },
        Err(_) => PinnedLoadOutcome::Missing,
    }
}

/// [`save_pinned_to`] at [`pinned_path`].
pub fn save_pinned(file: &PinnedFileV1) -> io::Result<()> {
    let path = pinned_path().ok_or_else(|| io::Error::other("no resolvable pinned list path"))?;
    save_pinned_to(&path, file)
}

/// Writes the list atomically. Normalizes what is not the caller's to
/// decide: `TabV1::id` becomes the position (0..n, ignored on read, as in
/// the session) and `schema_version` becomes [`PINNED_SCHEMA_VERSION`].
/// Never call this when the load reported a newer schema -- the registry is
/// read-only then (ADR-0086 §3).
pub fn save_pinned_to(path: &Path, file: &PinnedFileV1) -> io::Result<()> {
    let mut normalized = file.clone();
    normalized.schema_version = PINNED_SCHEMA_VERSION;
    for (position, tab) in normalized.tabs.iter_mut().enumerate() {
        tab.id = u32::try_from(position).unwrap_or(u32::MAX);
    }
    write_json_atomic(path, &normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::v1::{PaneNodeV1, PaneTreeV1, PaneV1};

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("creates a temporary test directory")
    }

    fn tab(id: u32, shell: &str, cwd: &str) -> TabV1 {
        TabV1 {
            id,
            custom_title: Some(format!("title-{shell}")),
            cwd: Some(PathBuf::from(cwd)),
            spawn_program: Some(shell.to_string()),
            profile: None,
            panes: Some(PaneTreeV1 {
                root: PaneNodeV1::Leaf(PaneV1 {
                    id: 0,
                    cwd: Some(PathBuf::from(cwd)),
                    spawn_program: Some(shell.to_string()),
                    profile: None,
                }),
                focused: 0,
            }),
        }
    }

    fn file_of(tabs: Vec<TabV1>) -> PinnedFileV1 {
        PinnedFileV1 {
            schema_version: PINNED_SCHEMA_VERSION,
            tabs,
        }
    }

    #[test]
    fn a_missing_file_is_an_empty_list_not_an_error() {
        let dir = tempdir();
        let outcome = load_pinned_from(&dir.path().join(PINNED_FILE_NAME));
        assert_eq!(outcome, PinnedLoadOutcome::Missing);
    }

    #[test]
    fn save_then_load_round_trips_in_order() {
        let dir = tempdir();
        let path = dir.path().join(PINNED_FILE_NAME);
        let file = file_of(vec![tab(0, "zsh", "/a"), tab(1, "bash", "/b")]);
        save_pinned_to(&path, &file).unwrap();

        assert_eq!(load_pinned_from(&path), PinnedLoadOutcome::Loaded(file));
    }

    #[test]
    fn save_normalizes_tab_ids_to_the_position() {
        let dir = tempdir();
        let path = dir.path().join(PINNED_FILE_NAME);
        let file = file_of(vec![tab(40, "zsh", "/a"), tab(7, "bash", "/b")]);
        save_pinned_to(&path, &file).unwrap();

        let PinnedLoadOutcome::Loaded(loaded) = load_pinned_from(&path) else {
            panic!("a file just written must load");
        };
        let ids: Vec<u32> = loaded.tabs.iter().map(|t| t.id).collect();
        assert_eq!(ids, [0, 1]);
        let shells: Vec<_> = loaded
            .tabs
            .iter()
            .map(|t| t.spawn_program.as_deref())
            .collect();
        assert_eq!(shells, [Some("zsh"), Some("bash")], "order is kept");
        // The caller's value is not mutated.
        assert_eq!(file.tabs[0].id, 40);
    }

    #[test]
    fn save_creates_the_directory_and_leaves_no_tmp_behind() {
        let dir = tempdir();
        let path = dir.path().join("nested").join(PINNED_FILE_NAME);
        save_pinned_to(&path, &file_of(vec![tab(0, "zsh", "/a")])).unwrap();
        assert!(path.exists());
        assert!(!dir.path().join("nested/pinned.json.tmp").exists());
    }

    #[test]
    fn an_empty_list_round_trips() {
        let dir = tempdir();
        let path = dir.path().join(PINNED_FILE_NAME);
        save_pinned_to(&path, &PinnedFileV1::default()).unwrap();
        assert_eq!(
            load_pinned_from(&path),
            PinnedLoadOutcome::Loaded(PinnedFileV1::default())
        );
    }

    #[test]
    fn an_invalid_file_is_quarantined_and_preserved() {
        let dir = tempdir();
        let path = dir.path().join(PINNED_FILE_NAME);
        fs::write(&path, b"{ not json").unwrap();

        let corrupt = dir.path().join("pinned.json.corrupt");
        assert_eq!(
            load_pinned_from(&path),
            PinnedLoadOutcome::Quarantined {
                path: corrupt.clone()
            }
        );
        assert!(!path.exists());
        assert_eq!(fs::read(&corrupt).unwrap(), b"{ not json");
    }

    #[test]
    fn a_second_invalid_file_does_not_overwrite_the_first_quarantine() {
        let dir = tempdir();
        let path = dir.path().join(PINNED_FILE_NAME);

        fs::write(&path, b"first").unwrap();
        load_pinned_from(&path);
        fs::write(&path, b"second").unwrap();
        let outcome = load_pinned_from(&path);

        let second = dir.path().join("pinned.json.corrupt.1");
        assert_eq!(outcome, PinnedLoadOutcome::Quarantined { path: second });
        assert_eq!(
            fs::read(dir.path().join("pinned.json.corrupt")).unwrap(),
            b"first"
        );
        assert_eq!(
            fs::read(dir.path().join("pinned.json.corrupt.1")).unwrap(),
            b"second"
        );
    }

    #[test]
    fn a_missing_zero_or_non_numeric_version_is_invalid_not_old() {
        for (n, body) in [
            r#"{"tabs":[]}"#,
            r#"{"schema_version":0,"tabs":[]}"#,
            r#"{"schema_version":"1","tabs":[]}"#,
        ]
        .into_iter()
        .enumerate()
        {
            let dir = tempdir();
            let path = dir.path().join(PINNED_FILE_NAME);
            fs::write(&path, body).unwrap();
            assert!(
                matches!(
                    load_pinned_from(&path),
                    PinnedLoadOutcome::Quarantined { .. }
                ),
                "case {n}: {body}"
            );
            assert!(!path.exists(), "case {n}");
        }
    }

    #[test]
    fn a_valid_version_with_an_invalid_structure_is_quarantined() {
        let dir = tempdir();
        let path = dir.path().join(PINNED_FILE_NAME);
        fs::write(&path, br#"{"schema_version":1,"tabs":"nope"}"#).unwrap();
        assert!(matches!(
            load_pinned_from(&path),
            PinnedLoadOutcome::Quarantined { .. }
        ));
    }

    #[test]
    fn a_newer_schema_is_reported_and_the_file_is_untouched() {
        let dir = tempdir();
        let path = dir.path().join(PINNED_FILE_NAME);
        let original = br#"{"schema_version":99,"tabs":[{"future":true}],"extra":1}"#;
        fs::write(&path, original).unwrap();

        assert_eq!(
            load_pinned_from(&path),
            PinnedLoadOutcome::NewerSchema {
                found: 99,
                supported: PINNED_SCHEMA_VERSION,
            }
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!dir.path().join("pinned.json.corrupt").exists());
    }

    #[test]
    fn a_file_without_tabs_loads_as_an_empty_list() {
        let dir = tempdir();
        let path = dir.path().join(PINNED_FILE_NAME);
        fs::write(&path, br#"{"schema_version":1}"#).unwrap();
        assert_eq!(
            load_pinned_from(&path),
            PinnedLoadOutcome::Loaded(PinnedFileV1::default())
        );
    }

    /// The path is derived from the session path, so `PORECATU_SESSION`
    /// moves both together (tested on the pure core, no `std::env`).
    #[test]
    fn the_path_sits_next_to_the_session_file() {
        assert_eq!(
            pinned_path_from(Path::new("/state/porecatu/session.json")),
            Some(PathBuf::from("/state/porecatu").join(PINNED_FILE_NAME))
        );
        assert_eq!(
            pinned_path_from(Path::new("/custom/other-name.json")),
            Some(PathBuf::from("/custom").join(PINNED_FILE_NAME))
        );
    }
}
