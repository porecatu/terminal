// SPDX-License-Identifier: GPL-3.0-or-later

//! Context menu of a list row (ADR-0095 §7, PRD-024 RF-24.20 to RF-24.22).
//! Same anatomy as `terminal_menu.rs`: the items are recomputed from the live
//! row, the menu only keeps the identity of the row and the highlight.

use porecatu_locale::Catalog;
use winit::keyboard::{Key, NamedKey};

use crate::dialog::{ConfirmDialog, EndProcessTarget};
use crate::messages::msg;

use super::rows::{Row, RowKey, RowKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskMenuAction {
    /// Back to the terminal with the row's tab active and its pane focused.
    GoToTab,
    /// Opens the confirmation dialog for the process of the row.
    EndProcess,
}

impl TaskMenuAction {
    /// Item label in the catalog's language, read on every frame.
    pub(crate) fn label(self, catalog: &Catalog) -> String {
        match self {
            Self::GoToTab => msg::task_manager::menu_go_to_tab(catalog),
            Self::EndProcess => msg::task_manager::menu_end_process(catalog),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskMenuItem {
    pub action: TaskMenuAction,
    pub enabled: bool,
}

/// The items of the menu of `row`, always the same two in the same order
/// (RF-24.22: an unavailable item is dimmed, never absent).
///
/// "Go to tab" is always enabled. "End process" is enabled only on a process
/// that is **not** the root of its pane; on a tab row, a note row and the pane
/// root it is present and disabled.
pub(crate) fn task_menu_items(row: &Row) -> Vec<TaskMenuItem> {
    let can_end = matches!(row.kind, RowKind::Process { is_root: false, .. });
    vec![
        TaskMenuItem {
            action: TaskMenuAction::GoToTab,
            enabled: true,
        },
        TaskMenuItem {
            action: TaskMenuAction::EndProcess,
            enabled: can_end,
        },
    ]
}

/// What a key does to an open row menu (ADR-0095 §3, item 1): the same set
/// as the other context menus -- arrows move, `Enter` runs, `Esc` closes --
/// and every other key is swallowed, never reaching the list nor the pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskMenuKey {
    Close,
    Move(isize),
    Activate,
    Swallow,
}

pub(crate) fn task_menu_key(pressed: bool, key: &Key) -> TaskMenuKey {
    if !pressed {
        return TaskMenuKey::Swallow;
    }
    match key {
        Key::Named(NamedKey::Escape) => TaskMenuKey::Close,
        Key::Named(NamedKey::ArrowUp) => TaskMenuKey::Move(-1),
        Key::Named(NamedKey::ArrowDown) => TaskMenuKey::Move(1),
        Key::Named(NamedKey::Enter) => TaskMenuKey::Activate,
        _ => TaskMenuKey::Swallow,
    }
}

/// What running a menu action asks of the app.
#[derive(Debug, PartialEq)]
pub(crate) enum TaskMenuOutcome {
    /// "Go to tab": the same path as the double click.
    GoTo(RowKey),
    /// "End process": the confirmation dialog, ready to open.
    Confirm(Box<ConfirmDialog>),
    /// Nothing: the action does not apply to the row (a disabled item no
    /// path may run, RF-24.22).
    Nothing,
}

/// Runs `action` on the live `row`. Pure, so the rule "End process never
/// opens a dialog on a tab, a note or a pane root" is tested without a
/// window, whatever the path (keyboard or mouse) that reached it.
pub(crate) fn run_menu_action(
    action: TaskMenuAction,
    row: &Row,
    catalog: &Catalog,
) -> TaskMenuOutcome {
    match action {
        TaskMenuAction::GoToTab => TaskMenuOutcome::GoTo(row.key),
        TaskMenuAction::EndProcess => end_process_dialog(catalog, row)
            .map_or(TaskMenuOutcome::Nothing, |dialog| {
                TaskMenuOutcome::Confirm(Box::new(dialog))
            }),
    }
}

/// The confirmation dialog of "End process" for `row` (RF-24.21), or `None`
/// when the row has nothing to end: a tab row, a note row and the pane root
/// (RF-24.22). The count of children is the one of the last sample.
pub(crate) fn end_process_dialog(catalog: &Catalog, row: &Row) -> Option<ConfirmDialog> {
    let RowKey::Process {
        tab,
        pane,
        pid,
        start_time,
    } = row.key
    else {
        return None;
    };
    let RowKind::Process {
        stat,
        is_root: false,
        descendants,
        ..
    } = &row.kind
    else {
        return None;
    };
    Some(ConfirmDialog::end_process(
        catalog,
        &stat.name,
        *descendants,
        EndProcessTarget {
            tab,
            pane,
            pid,
            start_time,
        },
    ))
}

/// The open menu: the row it was opened on (by identity, so a new sample does
/// not retarget it), where it is anchored and which item is highlighted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TaskMenu {
    pub row: RowKey,
    /// Logical window coordinates: the click point, or the selected row when
    /// opened from the keyboard.
    pub anchor: (f32, f32),
    highlighted: usize,
}

impl TaskMenu {
    pub(crate) fn new(row: RowKey, anchor: (f32, f32), items: &[TaskMenuItem]) -> Self {
        Self {
            row,
            anchor,
            highlighted: first_enabled_index(items),
        }
    }

    pub(crate) const fn highlighted(&self) -> usize {
        self.highlighted
    }

    /// Arrow keys: moves the highlight by `delta`, wrapping and skipping
    /// disabled items.
    pub(crate) fn move_highlight(&mut self, delta: isize, items: &[TaskMenuItem]) {
        let len = items.len() as isize;
        if len == 0 {
            return;
        }
        let mut idx = self.highlighted as isize;
        for _ in 0..len {
            idx = (idx + delta).rem_euclid(len);
            if items[idx as usize].enabled {
                self.highlighted = idx as usize;
                return;
            }
        }
    }

    /// Mouse hover: ignores a disabled item.
    pub(crate) fn set_highlight(&mut self, index: usize, items: &[TaskMenuItem]) {
        if items.get(index).is_some_and(|item| item.enabled) {
            self.highlighted = index;
        }
    }

    /// The action under the highlight; `None` when it is on a disabled item
    /// (which a highlight never rests on unless every item is disabled).
    pub(crate) fn selected(&self, items: &[TaskMenuItem]) -> Option<TaskMenuAction> {
        items
            .get(self.highlighted)
            .filter(|item| item.enabled)
            .map(|item| item.action)
    }
}

fn first_enabled_index(items: &[TaskMenuItem]) -> usize {
    items.iter().position(|item| item.enabled).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::test_support::{en_us, pt_br};
    use crate::task_manager::Totals;
    use crate::task_manager::rows::NoteKind;
    use porecatu_core::{PaneId, TabId};
    use porecatu_term::ProcessStat;

    fn tab_row() -> Row {
        Row {
            key: RowKey::Tab(TabId::new(0)),
            kind: RowKind::Tab {
                tab: TabId::new(0),
                title: "api".into(),
                group_color: None,
                totals: Totals {
                    cpu: None,
                    memory: 0,
                    disk_per_sec: 0.0,
                },
            },
        }
    }

    fn note_row() -> Row {
        Row {
            key: RowKey::Note(TabId::new(0), PaneId::new(0)),
            kind: RowKind::Note {
                kind: NoteKind::Ssh,
                depth: 1,
            },
        }
    }

    fn process_row(is_root: bool) -> Row {
        Row {
            key: RowKey::Process {
                tab: TabId::new(0),
                pane: PaneId::new(0),
                pid: 7,
                start_time: 1,
            },
            kind: RowKind::Process {
                stat: ProcessStat {
                    pid: 7,
                    parent: None,
                    start_time: 1,
                    name: "node.exe".into(),
                    cpu: None,
                    memory: 0,
                    disk_per_sec: 0.0,
                },
                depth: if is_root { 1 } else { 2 },
                is_root,
                descendants: 0,
            },
        }
    }

    #[test]
    fn end_process_is_present_but_disabled_on_tab_note_and_root_rows() {
        for row in [tab_row(), note_row(), process_row(true)] {
            let items = task_menu_items(&row);
            assert_eq!(items.len(), 2, "never absent");
            assert_eq!(items[0].action, TaskMenuAction::GoToTab);
            assert!(items[0].enabled);
            assert_eq!(items[1].action, TaskMenuAction::EndProcess);
            assert!(!items[1].enabled, "{row:?}");
        }
    }

    #[test]
    fn end_process_is_enabled_on_a_child_process() {
        let items = task_menu_items(&process_row(false));
        assert!(items[1].enabled);
    }

    #[test]
    fn opens_on_the_first_item_and_navigation_skips_disabled_ones() {
        let items = task_menu_items(&tab_row());
        let mut menu = TaskMenu::new(RowKey::Tab(TabId::new(0)), (1.0, 2.0), &items);
        assert_eq!(menu.selected(&items), Some(TaskMenuAction::GoToTab));
        menu.move_highlight(1, &items);
        assert_eq!(menu.selected(&items), Some(TaskMenuAction::GoToTab));
        menu.move_highlight(-1, &items);
        assert_eq!(menu.highlighted(), 0);

        let items = task_menu_items(&process_row(false));
        let mut menu = TaskMenu::new(RowKey::Tab(TabId::new(0)), (0.0, 0.0), &items);
        menu.move_highlight(1, &items);
        assert_eq!(menu.selected(&items), Some(TaskMenuAction::EndProcess));
        menu.move_highlight(1, &items);
        assert_eq!(menu.selected(&items), Some(TaskMenuAction::GoToTab));
        menu.move_highlight(-1, &items);
        assert_eq!(menu.selected(&items), Some(TaskMenuAction::EndProcess));
    }

    #[test]
    fn hover_ignores_a_disabled_item() {
        let items = task_menu_items(&process_row(true));
        let mut menu = TaskMenu::new(RowKey::Tab(TabId::new(0)), (0.0, 0.0), &items);
        menu.set_highlight(1, &items);
        assert_eq!(menu.selected(&items), Some(TaskMenuAction::GoToTab));
        let items = task_menu_items(&process_row(false));
        menu.set_highlight(1, &items);
        assert_eq!(menu.selected(&items), Some(TaskMenuAction::EndProcess));
    }

    fn process_row_with(pid: u32, is_root: bool, descendants: usize) -> Row {
        let mut row = process_row(is_root);
        row.key = RowKey::Process {
            tab: TabId::new(3),
            pane: PaneId::new(2),
            pid,
            start_time: 55,
        };
        if let RowKind::Process {
            stat,
            descendants: count,
            ..
        } = &mut row.kind
        {
            stat.pid = pid;
            *count = descendants;
        }
        row
    }

    #[test]
    fn the_end_process_dialog_of_a_child_carries_the_identity_and_the_children() {
        let en = en_us();
        let dialog = end_process_dialog(&en, &process_row_with(77, false, 2)).expect("a child");
        assert_eq!(
            dialog.action,
            crate::dialog::DialogAction::EndProcess {
                tab: TabId::new(3),
                pane: PaneId::new(2),
                pid: 77,
                start_time: 55,
            }
        );
        assert_eq!(
            dialog.body,
            "node.exe (PID 77) and 2 child processes will be ended."
        );
        let leaf = end_process_dialog(&en, &process_row_with(78, false, 0)).expect("a leaf");
        assert_eq!(leaf.body, "node.exe (PID 78) will be ended.");
    }

    #[test]
    fn no_dialog_for_a_tab_a_note_or_the_pane_root() {
        let en = en_us();
        for row in [tab_row(), note_row(), process_row(true)] {
            assert!(end_process_dialog(&en, &row).is_none(), "{row:?}");
            // Whatever path reached the action, it runs on nothing.
            assert_eq!(
                run_menu_action(TaskMenuAction::EndProcess, &row, &en),
                TaskMenuOutcome::Nothing
            );
        }
    }

    #[test]
    fn go_to_tab_runs_on_every_kind_of_row() {
        let en = en_us();
        for row in [tab_row(), note_row(), process_row(true), process_row(false)] {
            assert_eq!(
                run_menu_action(TaskMenuAction::GoToTab, &row, &en),
                TaskMenuOutcome::GoTo(row.key)
            );
        }
        match run_menu_action(TaskMenuAction::EndProcess, &process_row(false), &en) {
            TaskMenuOutcome::Confirm(dialog) => {
                assert_eq!(dialog.focused(), crate::dialog::DialogButton::Cancel);
            }
            other => panic!("expected a dialog, got {other:?}"),
        }
    }

    #[test]
    fn menu_keys_are_the_ones_of_the_other_menus_and_the_rest_is_swallowed() {
        let named = |key| Key::Named(key);
        assert_eq!(
            task_menu_key(true, &named(NamedKey::Escape)),
            TaskMenuKey::Close
        );
        assert_eq!(
            task_menu_key(true, &named(NamedKey::ArrowUp)),
            TaskMenuKey::Move(-1)
        );
        assert_eq!(
            task_menu_key(true, &named(NamedKey::ArrowDown)),
            TaskMenuKey::Move(1)
        );
        assert_eq!(
            task_menu_key(true, &named(NamedKey::Enter)),
            TaskMenuKey::Activate
        );
        assert_eq!(
            task_menu_key(true, &named(NamedKey::PageDown)),
            TaskMenuKey::Swallow
        );
        assert_eq!(
            task_menu_key(true, &Key::Character("x".into())),
            TaskMenuKey::Swallow
        );
        // A release never acts, `Esc` included.
        assert_eq!(
            task_menu_key(false, &named(NamedKey::Escape)),
            TaskMenuKey::Swallow
        );
    }

    #[test]
    fn labels_come_from_the_catalog() {
        assert_eq!(TaskMenuAction::GoToTab.label(&pt_br()), "Ir para a aba");
        assert_eq!(
            TaskMenuAction::EndProcess.label(&pt_br()),
            "Encerrar processo"
        );
        assert_eq!(TaskMenuAction::GoToTab.label(&en_us()), "Go to tab");
        assert_eq!(TaskMenuAction::EndProcess.label(&en_us()), "End process");
    }
}
