// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing **one key** of the config file by a gesture that is not the
//! settings screen's Save (ADR-0098 section 4, ADR-0107 section 7): the
//! task manager's footer toggle and the `remote.toggle` action.
//!
//! The value is never kept in memory by the gesture: the key is set to the
//! opposite of what the `Config` in use says, the file is written through
//! `porecatu-config::edit` (so the user's comments and order stay), and the
//! hot reload brings the value in -- which the gesture's surface then draws.
//! One path for every such gesture, so what happens to a file that changed in
//! the meantime, a file that does not exist yet and a failure is the same for
//! all of them.

use std::path::Path;

use porecatu_config::{ConfigDocument, Edit, EditError, EditValue, KeyPath};

use super::*;

/// The key `remote.toggle` writes (PRD-025 RF-25.15).
pub(crate) const REMOTE_ENABLED_KEY: &str = "remote.enabled";

/// The one edit of `remote.toggle`: the key set to the opposite of `current`,
/// the value of the `Config` in use (the action never keeps a state of its
/// own).
pub(crate) fn remote_enabled_edit(current: bool) -> Edit {
    Edit::Set(
        KeyPath::parse(REMOTE_ENABLED_KEY).expect("a valid key path"),
        EditValue::Bool(!current),
    )
}

/// Writes `edit` to `path`: over `base`, the file's text as just read, or --
/// `None`, no file yet -- over `template`, the embedded example (RF-16.21). A
/// file that changed between the read and the write (`EditError::Changed`) is
/// rebased on its new text and the edit is applied **once** more: an edit by
/// key lands on whatever the file now holds. A second `Changed`, like any
/// other error, is returned and nothing was written. Who applies the value is
/// the hot reload, never this function.
pub(crate) fn save_single_key(
    path: &Path,
    base: Option<&str>,
    template: &str,
    edit: &Edit,
) -> Result<(), EditError> {
    let mut document = match base {
        Some(text) => ConfigDocument::parse(text),
        None => ConfigDocument::from_template(template),
    }?;
    let edits = std::slice::from_ref(edit);
    match document.save(path, edits) {
        Err(EditError::Changed { current }) => {
            document.rebase(&current)?;
            document.save(path, edits).map(|_| ())
        }
        other => other.map(|_| ()),
    }
}

impl App {
    /// Writes `edit` to the config file in use, by key, and tells the user in
    /// `window_id` when it could not be done: with `failed_title` as the title
    /// of the warning, and the file error (or "no config path") as its body.
    /// Nothing changes in memory; a failure leaves the gesture's surface as
    /// it was.
    pub(crate) fn save_key_gesture(
        &mut self,
        window_id: WindowId,
        edit: &Edit,
        failed_title: fn(&Catalog) -> String,
    ) {
        let now = Instant::now();
        let result = match self.config_path.clone() {
            None => Err(None),
            Some(path) => {
                let base = match std::fs::read_to_string(&path) {
                    Ok(text) => Ok(Some(text)),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(err) => Err(EditError::Io {
                        path: path.clone(),
                        kind: err.kind(),
                        cause: err.to_string(),
                    }),
                };
                let created = matches!(base, Ok(None));
                base.and_then(|base| {
                    save_single_key(&path, base.as_deref(), EXAMPLE_CONFIG_TOML, edit)
                })
                .map(|()| {
                    // A file born from the example gets the theme files next
                    // to it, as the settings screen's first save does
                    // (ADR-0067). Failing here does not undo the save.
                    if created && let Some(dir) = path.parent() {
                        let _ = write_missing_theme_files(dir);
                    }
                })
                .map_err(Some)
            }
        };
        let Err(error) = result else {
            return;
        };
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        let (title, body) = match error {
            None => (
                msg::notice::config_path_unresolved::title(&self.catalog),
                msg::notice::config_path_unresolved::body(&self.catalog),
            ),
            Some(error) => (
                failed_title(&self.catalog),
                messages::settings_save_error(
                    &self.catalog,
                    &crate::settings::save::SaveError::Edit(error),
                ),
            ),
        };
        state.warnings.push(Severity::Warning, title, body, now);
        state.window.request_redraw();
    }

    /// `remote.toggle` (PRD-025 RF-25.15, ADR-0107 section 7): writes
    /// `[remote] enabled` as the opposite of the `Config`'s, by key. Nothing
    /// changes in memory: the watcher sees the file, the hot reload brings the
    /// value, and `sync_remote` starts or stops the host on the next turn.
    pub(crate) fn toggle_remote_enabled(&mut self, window_id: WindowId) {
        let edit = remote_enabled_edit(self.config.remote.enabled);
        self.save_key_gesture(
            window_id,
            &edit,
            msg::notice::remote_toggle_save_failed::title,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = EXAMPLE_CONFIG_TOML;

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "porecatu-key-edit-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn background(current: bool) -> Edit {
        crate::app_task_manager::background_monitoring_edit(current)
    }

    #[test]
    fn the_remote_toggle_edit_sets_the_key_to_the_opposite_value() {
        let key = KeyPath::parse("remote.enabled").unwrap();
        assert_eq!(
            remote_enabled_edit(false),
            Edit::Set(key.clone(), EditValue::Bool(true))
        );
        assert_eq!(
            remote_enabled_edit(true),
            Edit::Set(key, EditValue::Bool(false))
        );
    }

    #[test]
    fn saving_writes_only_the_key_and_loads_back_with_it() {
        let original = "# meu arquivo\n[task_manager]\nhistory_seconds = 120   # meu\n";
        let path = scratch("only-key.toml");
        std::fs::write(&path, original).unwrap();
        save_single_key(&path, Some(original), EXAMPLE, &background(false)).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            written.starts_with("# meu arquivo\n[task_manager]\nhistory_seconds = 120   # meu\n")
        );
        let (config, _) = porecatu_config::parse(&written).unwrap();
        assert!(config.task_manager.background_monitoring);
        assert_eq!(config.task_manager.history_seconds, 120);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn the_remote_key_is_written_the_same_way_and_leaves_the_rest_alone() {
        let original = "# meu arquivo\n[remote]\nrelay_url = \"wss://relay.example\"   # meu\n";
        let path = scratch("remote-key.toml");
        std::fs::write(&path, original).unwrap();
        save_single_key(&path, Some(original), EXAMPLE, &remote_enabled_edit(false)).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("# meu arquivo\n[remote]\n"));
        assert!(written.contains("relay_url = \"wss://relay.example\"   # meu"));
        let (config, _) = porecatu_config::parse(&written).unwrap();
        assert!(config.remote.enabled);
        assert_eq!(config.remote.relay_url, "wss://relay.example");
        // And back: the edit is built from the value in use, not kept.
        save_single_key(&path, Some(&written), EXAMPLE, &remote_enabled_edit(true)).unwrap();
        let (config, _) = porecatu_config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(!config.remote.enabled);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_file_that_changed_since_the_read_is_rebased_once_and_keeps_both_changes() {
        // The base is stale: someone wrote another key in the meantime. The
        // edit by key lands on the new text, which keeps its own change.
        let stale = "[task_manager]\nhistory_seconds = 60\n";
        let now_on_disk = "[task_manager]\nhistory_seconds = 300\n";
        let path = scratch("rebased.toml");
        std::fs::write(&path, now_on_disk).unwrap();
        save_single_key(&path, Some(stale), EXAMPLE, &background(false)).unwrap();
        let (config, _) = porecatu_config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(config.task_manager.background_monitoring);
        assert_eq!(config.task_manager.history_seconds, 300);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_missing_file_is_born_from_the_example_with_the_key() {
        let path = scratch("born.toml");
        save_single_key(&path, None, EXAMPLE, &background(false)).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        let (config, _) = porecatu_config::parse(&written).unwrap();
        assert!(config.task_manager.background_monitoring);
        // The example's documentation came along.
        assert!(written.contains("# Continua medindo os processos"));
        std::fs::remove_file(&path).unwrap();

        let path = scratch("born-remote.toml");
        save_single_key(&path, None, EXAMPLE, &remote_enabled_edit(false)).unwrap();
        let (config, _) = porecatu_config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(config.remote.enabled);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn an_invalid_file_on_disk_is_an_error_and_is_left_alone() {
        let broken = "[task_manager\n";
        let path = scratch("broken.toml");
        std::fs::write(&path, broken).unwrap();
        assert!(save_single_key(&path, Some(broken), EXAMPLE, &background(false)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
        std::fs::remove_file(&path).unwrap();
    }
}
