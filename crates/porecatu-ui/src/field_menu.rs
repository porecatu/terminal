// SPDX-License-Identifier: GPL-3.0-or-later

//! Right-click menu of a text field (ADR-0115 §7, PRD-010 RF-10.31): Cut,
//! Copy, Paste, a divider, Select all. The fourth context menu, with the
//! anatomy of the other three (`terminal_menu.rs` is the closest: items with
//! `enabled`, never absent). Pure state: the geometry and painting live in
//! `overlay.rs` (`layout_field_menu`/`paint_field_menu`), and the window that
//! owns the field decides what the actions do.
//!
//! The state is generic over its **target** because every window has its own
//! fields (a tab rename, the settings window's fields, the secret dialog...):
//! each window instantiates `FieldContextMenu<ItsFieldId>`.

use porecatu_locale::Catalog;
use winit::keyboard::{Key, NamedKey};

use crate::messages::msg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMenuAction {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl FieldMenuAction {
    /// Item label in the catalog's language, read every frame.
    pub fn label(self, catalog: &Catalog) -> String {
        match self {
            FieldMenuAction::Cut => msg::field_menu::cut(catalog),
            FieldMenuAction::Copy => msg::field_menu::copy(catalog),
            FieldMenuAction::Paste => msg::field_menu::paste(catalog),
            FieldMenuAction::SelectAll => msg::field_menu::select_all(catalog),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldMenuItem {
    pub action: FieldMenuAction,
    pub enabled: bool,
}

/// Number of items of the menu; the divider is not an item.
pub const FIELD_MENU_ITEM_COUNT: usize = 4;

/// The divider sits above this item index ("Select all"), between Paste and
/// Select all.
pub const FIELD_MENU_DIVIDER_ABOVE: usize = 3;

/// Cut, Copy, Paste, Select all. Cut and Copy are dimmed (never absent,
/// RF-10.20) without a selection and always in a masked field, whose content
/// never reaches the clipboard (ADR-0075 §1); Paste and Select all are always
/// enabled -- the menu does not read the clipboard to decide (ADR-0115 §7).
pub fn field_menu_items(
    has_selection: bool,
    masked: bool,
) -> [FieldMenuItem; FIELD_MENU_ITEM_COUNT] {
    let can_copy = has_selection && !masked;
    [
        FieldMenuItem {
            action: FieldMenuAction::Cut,
            enabled: can_copy,
        },
        FieldMenuItem {
            action: FieldMenuAction::Copy,
            enabled: can_copy,
        },
        FieldMenuItem {
            action: FieldMenuAction::Paste,
            enabled: true,
        },
        FieldMenuItem {
            action: FieldMenuAction::SelectAll,
            enabled: true,
        },
    ]
}

/// An open field menu over `target`.
///
/// Opens with **no** row highlighted (hover and keyboard focus are the same
/// highlight and mutually exclusive, espec §2.16: one at a time): the mouse
/// sets it by hovering, an arrow key by `move_highlight` -- `Down` lands on
/// the first enabled item and `Up` on the last. A caller that wants the
/// keyboard-first look of the terminal menu calls `highlight_first_enabled`.
/// The item list is passed in (recomputed from live state by the owner),
/// never stored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldContextMenu<T> {
    pub target: T,
    /// Logical window coordinates of the click that opened the menu.
    pub anchor: (f32, f32),
    highlighted: Option<usize>,
}

impl<T> FieldContextMenu<T> {
    pub const fn new(target: T, anchor: (f32, f32)) -> Self {
        Self {
            target,
            anchor,
            highlighted: None,
        }
    }

    pub const fn highlighted(&self) -> Option<usize> {
        self.highlighted
    }

    /// Hover: sets the highlight, or clears it with `None` (the mouse left
    /// the rows). Unlike `move_highlight` it does not look at `enabled`; the
    /// caller hit-tests rows and passes only an enabled one (a dimmed item
    /// cannot be highlighted by the mouse either -- see `set_highlight_item`).
    pub fn set_highlight(&mut self, index: Option<usize>) {
        self.highlighted = index;
    }

    /// Hover over `index`: highlights it only when it exists and is enabled,
    /// as `TerminalContextMenu::set_highlight` does.
    pub fn set_highlight_item(&mut self, index: usize, items: &[FieldMenuItem]) {
        if items.get(index).is_some_and(|item| item.enabled) {
            self.highlighted = Some(index);
        }
    }

    pub fn highlight_first_enabled(&mut self, items: &[FieldMenuItem]) {
        self.highlighted = items.iter().position(|item| item.enabled);
    }

    /// Arrow keys: moves by `delta` with wrap, skipping disabled items. From
    /// no highlight, a positive `delta` lands on the first enabled item and a
    /// negative one on the last. With nothing enabled it does nothing.
    pub fn move_highlight(&mut self, delta: isize, items: &[FieldMenuItem]) {
        let len = items.len() as isize;
        if len == 0 {
            return;
        }
        let mut idx = match self.highlighted {
            Some(current) => current as isize,
            None if delta >= 0 => -1,
            None => 0,
        };
        for _ in 0..len {
            idx = (idx + delta).rem_euclid(len);
            if items[idx as usize].enabled {
                self.highlighted = Some(idx as usize);
                return;
            }
        }
    }

    /// The highlighted action, only if it is enabled.
    pub fn selected(&self, items: &[FieldMenuItem]) -> Option<FieldMenuAction> {
        self.highlighted
            .and_then(|index| items.get(index))
            .filter(|item| item.enabled)
            .map(|item| item.action)
    }
}

/// What a key does to an open field menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMenuKey {
    /// The key means nothing to the menu. It is still **consumed** by the
    /// window: like the other context menus, an open field menu swallows the
    /// whole keyboard (a `Ctrl+V` while the menu is open does not paste).
    Nothing,
    /// `Esc`: close the menu and nothing else -- the rename, the search, the
    /// editor, the popover or the dialog that owns the field stays.
    Close,
    /// `Enter` on a highlighted, enabled item: close the menu and run it.
    Run(FieldMenuAction),
}

/// The keyboard of an open field menu, with the arrows and `Enter` of the
/// other menus (`handle_terminal_menu_key`): `Up`/`Down` move the highlight
/// over the enabled items, with wrap; `Enter` runs the highlighted one (and
/// does nothing with no highlight); `Esc` closes. Pure, so every window with
/// a field menu routes the key the same way.
pub fn field_menu_key<T>(
    menu: &mut FieldContextMenu<T>,
    key: &Key,
    items: &[FieldMenuItem],
) -> FieldMenuKey {
    match key {
        Key::Named(NamedKey::Escape) => FieldMenuKey::Close,
        Key::Named(NamedKey::ArrowUp) => {
            menu.move_highlight(-1, items);
            FieldMenuKey::Nothing
        }
        Key::Named(NamedKey::ArrowDown) => {
            menu.move_highlight(1, items);
            FieldMenuKey::Nothing
        }
        Key::Named(NamedKey::Enter) => match menu.selected(items) {
            Some(action) => FieldMenuKey::Run(action),
            None => FieldMenuKey::Nothing,
        },
        _ => FieldMenuKey::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled(items: &[FieldMenuItem]) -> Vec<bool> {
        items.iter().map(|item| item.enabled).collect()
    }

    #[test]
    fn items_are_in_menu_order() {
        let actions: Vec<_> = field_menu_items(true, false)
            .iter()
            .map(|item| item.action)
            .collect();
        assert_eq!(
            actions,
            [
                FieldMenuAction::Cut,
                FieldMenuAction::Copy,
                FieldMenuAction::Paste,
                FieldMenuAction::SelectAll,
            ]
        );
        assert_eq!(field_menu_items(true, false).len(), FIELD_MENU_ITEM_COUNT);
    }

    #[test]
    fn the_divider_is_above_select_all() {
        let items = field_menu_items(true, false);
        assert_eq!(
            items[FIELD_MENU_DIVIDER_ABOVE].action,
            FieldMenuAction::SelectAll
        );
    }

    #[test]
    fn enabling_in_the_four_combinations() {
        assert_eq!(enabled(&field_menu_items(true, false)), [true; 4]);
        assert_eq!(
            enabled(&field_menu_items(false, false)),
            [false, false, true, true]
        );
        assert_eq!(
            enabled(&field_menu_items(true, true)),
            [false, false, true, true]
        );
        assert_eq!(
            enabled(&field_menu_items(false, true)),
            [false, false, true, true]
        );
    }

    #[test]
    fn opens_without_a_highlight() {
        let menu = FieldContextMenu::new(7_u32, (3.0, 4.0));
        assert_eq!(menu.highlighted(), None);
        assert_eq!(menu.target, 7);
        assert_eq!(menu.anchor, (3.0, 4.0));
        assert_eq!(menu.selected(&field_menu_items(true, false)), None);
    }

    #[test]
    fn down_from_nothing_lands_on_the_first_enabled_item() {
        let items = field_menu_items(false, false);
        let mut menu = FieldContextMenu::new((), (0.0, 0.0));
        menu.move_highlight(1, &items);
        assert_eq!(menu.selected(&items), Some(FieldMenuAction::Paste));
    }

    #[test]
    fn up_from_nothing_lands_on_the_last_enabled_item() {
        let items = field_menu_items(true, false);
        let mut menu = FieldContextMenu::new((), (0.0, 0.0));
        menu.move_highlight(-1, &items);
        assert_eq!(menu.selected(&items), Some(FieldMenuAction::SelectAll));
    }

    #[test]
    fn navigation_skips_disabled_items_and_wraps() {
        let items = field_menu_items(false, false);
        let mut menu = FieldContextMenu::new((), (0.0, 0.0));
        menu.highlight_first_enabled(&items);
        assert_eq!(menu.selected(&items), Some(FieldMenuAction::Paste));
        menu.move_highlight(1, &items);
        assert_eq!(menu.selected(&items), Some(FieldMenuAction::SelectAll));
        // Past the end it wraps over the two dimmed items to Paste again.
        menu.move_highlight(1, &items);
        assert_eq!(menu.selected(&items), Some(FieldMenuAction::Paste));
        menu.move_highlight(-1, &items);
        assert_eq!(menu.selected(&items), Some(FieldMenuAction::SelectAll));
    }

    #[test]
    fn navigation_walks_every_item_when_all_are_enabled() {
        let items = field_menu_items(true, false);
        let mut menu = FieldContextMenu::new((), (0.0, 0.0));
        menu.highlight_first_enabled(&items);
        let mut seen = vec![menu.highlighted().unwrap()];
        for _ in 0..3 {
            menu.move_highlight(1, &items);
            seen.push(menu.highlighted().unwrap());
        }
        assert_eq!(seen, [0, 1, 2, 3]);
    }

    #[test]
    fn hover_ignores_disabled_items() {
        let items = field_menu_items(false, true);
        let mut menu = FieldContextMenu::new((), (0.0, 0.0));
        menu.set_highlight_item(0, &items);
        assert_eq!(menu.highlighted(), None);
        menu.set_highlight_item(2, &items);
        assert_eq!(menu.selected(&items), Some(FieldMenuAction::Paste));
        menu.set_highlight_item(9, &items);
        assert_eq!(menu.highlighted(), Some(2));
        menu.set_highlight(None);
        assert_eq!(menu.highlighted(), None);
    }

    /// The list can change under an open menu (the selection changes); a
    /// highlight left on an item that became disabled selects nothing.
    #[test]
    fn selected_never_returns_a_disabled_item() {
        let with_selection = field_menu_items(true, false);
        let mut menu = FieldContextMenu::new((), (0.0, 0.0));
        menu.set_highlight_item(0, &with_selection);
        assert_eq!(menu.selected(&with_selection), Some(FieldMenuAction::Cut));
        let without = field_menu_items(false, false);
        assert_eq!(menu.selected(&without), None);
    }

    #[test]
    fn escape_closes_only_the_menu() {
        let items = field_menu_items(true, false);
        let mut menu = FieldContextMenu::new((), (0.0, 0.0));
        menu.highlight_first_enabled(&items);
        let key = Key::Named(NamedKey::Escape);
        assert_eq!(field_menu_key(&mut menu, &key, &items), FieldMenuKey::Close);
    }

    #[test]
    fn arrows_move_and_enter_runs_the_highlighted_item() {
        let items = field_menu_items(true, false);
        let mut menu = FieldContextMenu::new((), (0.0, 0.0));
        menu.highlight_first_enabled(&items);
        let down = Key::Named(NamedKey::ArrowDown);
        let up = Key::Named(NamedKey::ArrowUp);
        let enter = Key::Named(NamedKey::Enter);
        assert_eq!(
            field_menu_key(&mut menu, &enter, &items),
            FieldMenuKey::Run(FieldMenuAction::Cut)
        );
        assert_eq!(
            field_menu_key(&mut menu, &down, &items),
            FieldMenuKey::Nothing
        );
        assert_eq!(
            field_menu_key(&mut menu, &enter, &items),
            FieldMenuKey::Run(FieldMenuAction::Copy)
        );
        // Up from the first item wraps to the last.
        field_menu_key(&mut menu, &up, &items);
        field_menu_key(&mut menu, &up, &items);
        assert_eq!(
            field_menu_key(&mut menu, &enter, &items),
            FieldMenuKey::Run(FieldMenuAction::SelectAll)
        );
    }

    #[test]
    fn enter_with_nothing_highlighted_or_enabled_runs_nothing() {
        let items = field_menu_items(false, false);
        let mut menu = FieldContextMenu::new((), (0.0, 0.0));
        let enter = Key::Named(NamedKey::Enter);
        assert_eq!(
            field_menu_key(&mut menu, &enter, &items),
            FieldMenuKey::Nothing
        );
        // A highlight left on an item that became disabled runs nothing.
        let with_selection = field_menu_items(true, false);
        menu.set_highlight_item(0, &with_selection);
        assert_eq!(
            field_menu_key(&mut menu, &enter, &items),
            FieldMenuKey::Nothing
        );
    }

    #[test]
    fn other_keys_mean_nothing_to_the_menu() {
        let items = field_menu_items(true, false);
        let mut menu = FieldContextMenu::new((), (0.0, 0.0));
        menu.highlight_first_enabled(&items);
        for key in [
            Key::Character("v".into()),
            Key::Named(NamedKey::Tab),
            Key::Named(NamedKey::Backspace),
        ] {
            assert_eq!(
                field_menu_key(&mut menu, &key, &items),
                FieldMenuKey::Nothing
            );
        }
        assert_eq!(menu.highlighted(), Some(0));
    }

    #[test]
    fn nothing_enabled_means_no_move() {
        let mut items = field_menu_items(false, true);
        items[2].enabled = false;
        items[3].enabled = false;
        let mut menu = FieldContextMenu::new((), (0.0, 0.0));
        menu.move_highlight(1, &items);
        assert_eq!(menu.highlighted(), None);
        menu.highlight_first_enabled(&items);
        assert_eq!(menu.highlighted(), None);
    }
}
