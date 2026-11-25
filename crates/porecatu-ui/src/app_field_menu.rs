// SPDX-License-Identifier: GPL-3.0-or-later

//! The right-click menu of a text field in the terminal window (ADR-0115 §7,
//! PRD-010 RF-10.31): Cut, Copy, Paste, Select all over the five fields of the
//! main window -- the tab rename, the name of the group editor, the name of the
//! sessions popover, the search bar and the masked field of the secret dialog.
//!
//! The menu is the fourth context menu of the window and is wired **point by
//! point like `terminal_context_menu`**: state in `WindowState::field_context_menu`,
//! opened on the press of the secondary button, first in the key chain and in the
//! press cascade (above the dialog, because the secret field lives in one),
//! armed on the press and run on the release over the same item
//! (`OverlayTarget::FieldMenu`, ADR-0081), closed by a click elsewhere, by the
//! window losing focus (`close_all_popovers`) and by its field going away.
//!
//! What decides something without a window is a free function here, with a test:
//! the resolution of a [`MainField`] to the live field ([`resolve_field`]), the
//! caret a right click leaves ([`place_caret_for_menu`]), what an item does
//! ([`apply_field_menu_action`]). What needs a window stays in the
//! `impl WindowState` / `impl App` below.
//!
//! **The menu never outlives its field.** Its target is only a name
//! ([`MainField`]); every use (open, key, hover, paint, run) resolves it again
//! from live state, and `None` -- the rename ended, the editor closed, the
//! popover went back to browsing, a dialog opened over a non-secret field --
//! drops the menu (`WindowState::drop_stale_field_menu`, called at the start of
//! each event and of each frame). The items are recomputed the same way, never
//! stored.

use porecatu_session::named::MAX_NAME_CHARS;

use crate::clipboard::ClipboardIo;
use crate::field_clipboard::{FieldOp, paste_into_masked, run_field_op};
use crate::field_menu::{
    FIELD_MENU_ITEM_COUNT, FieldContextMenu, FieldMenuAction, FieldMenuItem, FieldMenuKey,
    field_menu_items, field_menu_key,
};
use crate::masked_field::{MASK_CHAR, MaskedFieldState};
use crate::text_field::TextFieldState;

use super::*;

/// The field of the terminal window a field menu is over. Only a name: the
/// field itself is looked up in live state at every use ([`resolve_field`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainField {
    /// The inline rename of a tab (`WindowState::rename`).
    Rename,
    /// The name field of the group editor.
    GroupEditorName,
    /// The name field of the sessions popover, while it edits a name.
    SessionPickerName,
    /// The field of the search bar.
    Search,
    /// The masked field of the secret dialog.
    Secret,
}

/// The live field a [`MainField`] stands for.
pub(crate) enum FieldRef<'a> {
    Plain {
        field: &'a mut TextFieldState,
        /// The length cap of the field, if it has one (the name of a named
        /// session, RF-14.3); a paste is cut at it.
        max_chars: Option<usize>,
    },
    /// The masked field: its content never reaches the clipboard (ADR-0075
    /// §1), so Cut and Copy are dimmed in its menu; Paste works.
    Masked(&'a mut MaskedFieldState),
}

impl FieldRef<'_> {
    /// The menu rows for this field's current state.
    pub(crate) fn menu_items(&self) -> [FieldMenuItem; FIELD_MENU_ITEM_COUNT] {
        match self {
            FieldRef::Plain { field, .. } => {
                field_menu_items(field.selection_range().is_some(), false)
            }
            FieldRef::Masked(field) => {
                field_menu_items(field.display().selection_range().is_some(), true)
            }
        }
    }
}

/// Where in a field's text a point landed: a byte of the text, or -- the masked
/// field draws one dot per scalar and its text is not measurable -- a scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldHit {
    Byte(usize),
    Scalar(usize),
}

/// The surfaces of a window that own a field, borrowed together so the
/// resolution is a function of them and not of a whole `WindowState` (which
/// cannot be built without an OS window, and so could not be tested).
pub(crate) struct Surfaces<'a> {
    pub rename: &'a mut RenameState,
    pub group_editor: &'a mut Option<GroupEditor>,
    pub session_picker: &'a mut Option<SessionPicker>,
    pub search: &'a mut Option<SearchBarState>,
    pub secret_field: &'a mut Option<MaskedFieldState>,
    pub dialog_open: bool,
    pub task_manager_open: bool,
}

/// The one function that turns a target into its live field; `None` means the
/// surface is gone and the menu has to close.
///
/// A dialog is modal: with one open only the secret field is editable, so every
/// other target resolves to `None` (a dialog that opens over a menu of the
/// search bar takes the menu with it). With the task manager open the search bar
/// is not on screen either.
pub(crate) fn resolve_field<'a>(surfaces: Surfaces<'a>, target: MainField) -> Option<FieldRef<'a>> {
    if (target == MainField::Secret) != surfaces.dialog_open {
        return None;
    }
    match target {
        MainField::Rename => match surfaces.rename {
            RenameState::Editing { field, .. } => Some(FieldRef::Plain {
                field,
                max_chars: None,
            }),
            RenameState::Idle => None,
        },
        MainField::GroupEditorName => {
            surfaces
                .group_editor
                .as_mut()
                .map(|editor| FieldRef::Plain {
                    field: editor.name_field_state_mut(),
                    max_chars: None,
                })
        }
        MainField::SessionPickerName => surfaces
            .session_picker
            .as_mut()
            .and_then(SessionPicker::name_field_mut)
            .map(|field| FieldRef::Plain {
                field,
                max_chars: Some(MAX_NAME_CHARS),
            }),
        MainField::Search => {
            if surfaces.task_manager_open {
                return None;
            }
            surfaces.search.as_mut().map(|search| FieldRef::Plain {
                field: search.field_mut(),
                max_chars: None,
            })
        }
        MainField::Secret => surfaces.secret_field.as_mut().map(FieldRef::Masked),
    }
}

/// What a right click does to the caret of a plain field: **inside** the
/// selection it keeps it (the menu then cuts or copies exactly what the user
/// sees selected); **outside** it, the caret goes under the point, as a left
/// click would, and the selection is gone.
pub(crate) fn place_caret_for_menu(field: &mut TextFieldState, byte: usize) {
    if field
        .selection_range()
        .is_some_and(|(start, end)| (start..=end).contains(&byte))
    {
        return;
    }
    field.click_at(byte);
}

/// [`place_caret_for_menu`] for the masked field, whose positions are scalars
/// (one dot each). The selection of its display field is in bytes of the dots.
pub(crate) fn place_masked_caret_for_menu(field: &mut MaskedFieldState, scalar: usize) {
    let dot = MASK_CHAR.len_utf8();
    if field
        .display()
        .selection_range()
        .is_some_and(|(start, end)| (start / dot..=end / dot).contains(&scalar))
    {
        return;
    }
    field.click_at_scalar(scalar);
}

/// What an item of the menu does to its field -- exactly what the key does:
/// Cut, Copy and Paste are `run_field_op` (the same function `edit_field_key`
/// calls), Select all is the field's own. In the masked field Paste is
/// `paste_into_masked` and Cut/Copy do nothing (their items are dimmed; this is
/// the guard if one gets here anyway).
pub(crate) fn apply_field_menu_action(
    field: FieldRef<'_>,
    action: FieldMenuAction,
    io: &mut impl ClipboardIo,
) {
    let op = match action {
        FieldMenuAction::Cut => FieldOp::Cut,
        FieldMenuAction::Copy => FieldOp::Copy,
        FieldMenuAction::Paste => FieldOp::Paste,
        FieldMenuAction::SelectAll => {
            match field {
                FieldRef::Plain { field, .. } => field.select_all(),
                FieldRef::Masked(field) => field.select_all(),
            }
            return;
        }
    };
    match field {
        FieldRef::Plain { field, max_chars } => run_field_op(field, op, io, max_chars),
        FieldRef::Masked(field) => {
            if op == FieldOp::Paste {
                paste_into_masked(field, io);
            }
        }
    }
}

/// Byte of `buffer` under `point_x` in the rename field of a tab.
pub(crate) fn rename_byte_at(
    buffer: &str,
    field_screen: Rect,
    style: &TabBarStyle,
    point_x: f32,
    measurer: &mut TextMeasurer,
) -> usize {
    tab_bar::text_index_at(
        measurer,
        buffer,
        chrome::LABEL_FONT,
        style.rename_font_size,
        field_screen.x,
        field_screen.width,
        style.rename_padding_x,
        point_x,
    )
}

/// Byte of `buffer` under `point_x` in the name field of the group editor and
/// of the sessions popover, which share the `[appearance.group_editor]` input.
pub(crate) fn overlay_input_byte_at(
    buffer: &str,
    input_rect: Rect,
    config: &porecatu_config::Config,
    point_x: f32,
    measurer: &mut TextMeasurer,
) -> usize {
    let cfg = &config.appearance.group_editor;
    tab_bar::text_index_at(
        measurer,
        buffer,
        overlay::BODY_FONT,
        cfg.input_font_size as f32,
        input_rect.x,
        input_rect.width,
        cfg.input_padding_x as f32,
        point_x,
    )
}

/// Byte of `buffer` under `point_x` in the field of the search bar.
pub(crate) fn search_byte_at(
    buffer: &str,
    field_rect: Rect,
    point_x: f32,
    measurer: &mut TextMeasurer,
) -> usize {
    tab_bar::text_index_at(
        measurer,
        buffer,
        search_bar::FIELD_FONT,
        search_bar::FIELD_FONT_SIZE,
        field_rect.x,
        field_rect.width,
        search_bar::FIELD_PADDING_X,
        point_x,
    )
}

impl WindowState {
    /// The surfaces that own a field, borrowed from this window.
    fn field_surfaces(&mut self) -> Surfaces<'_> {
        Surfaces {
            rename: &mut self.rename,
            group_editor: &mut self.group_editor,
            session_picker: &mut self.session_picker,
            search: &mut self.search,
            secret_field: &mut self.secret_field,
            dialog_open: self.dialog.is_some(),
            task_manager_open: self.task_manager.is_some(),
        }
    }

    /// The live field of `target`, or `None` if its surface is gone.
    pub(crate) fn field_ref(&mut self, target: MainField) -> Option<FieldRef<'_>> {
        resolve_field(self.field_surfaces(), target)
    }

    /// The rows of the menu of `target`, from the live field. Takes `&mut self`
    /// only because [`Self::field_ref`] does; it changes nothing.
    pub(crate) fn field_menu_items(
        &mut self,
        target: MainField,
    ) -> Option<[FieldMenuItem; FIELD_MENU_ITEM_COUNT]> {
        self.field_ref(target).map(|field| field.menu_items())
    }

    /// Closes the field menu if its field is gone (see the module doc). Called
    /// at the start of every event that looks at the menu and of every frame.
    pub(crate) fn drop_stale_field_menu(&mut self) {
        if let Some(menu) = self.field_context_menu
            && self.field_ref(menu.target).is_none()
        {
            self.field_context_menu = None;
        }
    }

    /// Another modal menu or popover that is not a field's own surface is open:
    /// a right click then belongs to it (it closes), never to a field behind it.
    fn other_menu_open(&self) -> bool {
        self.context_menu.is_some()
            || self.group_context_menu.is_some()
            || self.profile_menu.is_some()
            || self.terminal_context_menu.is_some()
            || self.move_to_group.is_some()
            || self.move_to_window.is_some()
            || self.devices_popover.is_some()
            || self
                .task_manager
                .as_ref()
                .is_some_and(|view| view.menu.is_some())
    }

    /// The screen rectangle of the rename field of tab `id` in `layout` (the
    /// track layout, in content coordinates), or `None` if the tab has no
    /// rectangle. The one geometry of the field for the click, the drag and the
    /// right click.
    pub(crate) fn rename_field_screen_rect(
        &self,
        layout: &tab_bar::TabBarLayout,
        id: TabId,
        style: &TabBarStyle,
        track_left: f32,
    ) -> Option<Rect> {
        let tab_rect = tab_bar::tab_rect(layout, id)?;
        let content = chrome::rename_field_rect(tab_rect, style);
        Some(Rect {
            x: content.x + track_left - self.scroll_offset,
            ..content
        })
    }

    /// [`Self::rename_field_screen_rect`] over the layout of the track as it is
    /// now, for the callers that have no layout at hand (the drag, the right
    /// click).
    pub(crate) fn rename_field_screen_rect_now(
        &self,
        id: TabId,
        style: &TabBarStyle,
        measurer: &mut TextMeasurer,
    ) -> Option<Rect> {
        let pinned_count = self.workspace.pinned().len();
        let trilha_width = tab_bar::trilha_width(
            style,
            self.logical_width,
            is_macos(),
            pinned_count,
            self.devices_button(),
        );
        let layout = tab_bar::fit_width(&self.workspace, style, trilha_width, measurer, is_macos());
        let track_left = tab_bar::track_shift(style, is_macos(), pinned_count);
        self.rename_field_screen_rect(&layout, id, style, track_left)
    }

    /// Restarts the search over the focused pane of its tab after the text of
    /// the field changed (typing, `Backspace`, a paste or a cut from the menu).
    pub(crate) fn restart_search(&mut self) {
        let Some(tab) = self.search.as_ref().map(SearchBarState::tab) else {
            return;
        };
        let pane = focused_pane(&self.workspace, tab);
        if let Some(rt) = pane.and_then(|p| self.panes.get(&(tab, p)))
            && let Some(search) = self.search.as_mut()
        {
            search.restart(&rt.terminal, DEFAULT_SEARCH_LINES_PER_STEP);
        }
    }

    /// Opens the menu over `target` at `point` (logical window coordinates):
    /// places the caret by the right click, gives the field the focus a left
    /// click would, and opens with the first enabled item highlighted, as the
    /// terminal menu does. `false` if the field is not there.
    fn open_field_menu(&mut self, target: MainField, hit: FieldHit, point: (f32, f32)) -> bool {
        // The same focus the left click gives: the editor's name region, the
        // secret dialog's field.
        match target {
            MainField::GroupEditorName => {
                if let Some(editor) = self.group_editor.as_mut() {
                    editor.set_focus(EditorRegion::Name);
                }
            }
            MainField::Secret => {
                if let Some(dialog) = self.dialog.as_mut() {
                    dialog.focus_secret(dialog::SecretFocus::Field);
                }
            }
            MainField::Rename | MainField::SessionPickerName | MainField::Search => {}
        }
        let Some(field) = self.field_ref(target) else {
            return false;
        };
        match (field, hit) {
            (FieldRef::Plain { field, .. }, FieldHit::Byte(byte)) => {
                place_caret_for_menu(field, byte);
            }
            (FieldRef::Masked(field), FieldHit::Scalar(scalar)) => {
                place_masked_caret_for_menu(field, scalar);
            }
            _ => {}
        }
        let Some(items) = self.field_menu_items(target) else {
            return false;
        };
        let mut menu = FieldContextMenu::new(target, point);
        menu.highlight_first_enabled(&items);
        self.field_context_menu = Some(menu);
        self.hover.dismiss();
        // The release of the press that opened the menu is not the field's.
        self.overlay_press_consumed = true;
        self.mouse_button_down = None;
        true
    }

    /// What a key does with the field menu open. The whole keyboard is the
    /// menu's: arrows and `Enter` as in the other menus, `Esc` closes **only
    /// the menu** (not the rename, the search, the editor, the popover nor the
    /// dialog), and every other key -- the clipboard keys included -- is
    /// swallowed, as the terminal menu swallows it. Returns the action to run.
    pub(crate) fn handle_field_menu_key(
        &mut self,
        event: &KeyEvent,
    ) -> Option<(MainField, FieldMenuAction)> {
        if event.state != ElementState::Pressed {
            return None;
        }
        let mut menu = self.field_context_menu?;
        let Some(items) = self.field_menu_items(menu.target) else {
            self.field_context_menu = None;
            return None;
        };
        let outcome = field_menu_key(&mut menu, &event.logical_key, &items);
        match outcome {
            FieldMenuKey::Nothing => {
                self.field_context_menu = Some(menu);
                None
            }
            FieldMenuKey::Close => {
                self.field_context_menu = None;
                None
            }
            FieldMenuKey::Run(action) => {
                self.field_context_menu = None;
                Some((menu.target, action))
            }
        }
    }

    /// Runs an item on the live field and, as the key does, restarts the search
    /// if its text changed. The menu was closed by the caller.
    pub(crate) fn run_field_menu_action(
        &mut self,
        target: MainField,
        action: FieldMenuAction,
        io: &mut impl ClipboardIo,
    ) {
        let before = (target == MainField::Search)
            .then(|| self.search.as_ref().map(|s| s.field().text().to_string()))
            .flatten();
        if let Some(field) = self.field_ref(target) {
            apply_field_menu_action(field, action, io);
        }
        if let Some(before) = before
            && self
                .search
                .as_ref()
                .is_some_and(|s| s.field().text() != before)
        {
            self.restart_search();
        }
    }
}

impl App {
    /// The press of the secondary button over a field of the window: opens the
    /// field menu and returns `true` (the press is consumed -- it opens neither
    /// the menu of a tab nor anything else). It runs **before** every other rule
    /// of the press, and only when no other menu is open (a click then closes
    /// that menu) and no field menu is open yet.
    pub(crate) fn try_open_field_menu(&mut self, window_id: WindowId, button: MouseButton) -> bool {
        let Some(state) = self.windows.get(&window_id) else {
            return false;
        };
        if !state.is_secondary_bar_click(button)
            || state.field_context_menu.is_some()
            || state.other_menu_open()
        {
            return false;
        }
        let point = (
            state.cursor_position.0 as f32 / state.scale,
            state.cursor_position.1 as f32 / state.scale,
        );
        let Some((target, hit)) = self.field_hit_at(window_id, point) else {
            return false;
        };
        let Some(state) = self.windows.get_mut(&window_id) else {
            return false;
        };
        let opened = state.open_field_menu(target, hit, point);
        if opened {
            state.window.request_redraw();
        }
        opened
    }

    /// The field under `point` and where in its text, with the same hit tests
    /// the left click uses (the layouts and `*_byte_at` above), in the order of
    /// the press cascade: the dialog (only its secret field), the editor of a
    /// group, the sessions popover, then -- with nothing modal open -- the
    /// search bar and the rename field. A point over any other part of a modal
    /// surface is no field (the click will close it).
    fn field_hit_at(
        &mut self,
        window_id: WindowId,
        point: (f32, f32),
    ) -> Option<(MainField, FieldHit)> {
        let gpu = self.gpu.as_mut()?;
        let state = self.windows.get_mut(&window_id)?;
        let (width, height) = (state.logical_width, state.logical_height);
        if let Some(dialog) = state.dialog.as_ref() {
            let field = state.secret_field.as_ref()?;
            let layout =
                overlay::layout_dialog(width, height, dialog, &self.config, gpu.text_measurer());
            if overlay::secret_hit(&layout, point) != Some(overlay::SecretHit::Field) {
                return None;
            }
            let scalar = overlay::secret_field_scalar_at(
                &layout,
                point.0,
                &self.config,
                field.len(),
                gpu.text_measurer(),
            );
            return Some((MainField::Secret, FieldHit::Scalar(scalar)));
        }
        if let Some(group) = state.group_editor.as_ref().map(|editor| editor.group) {
            let anchor_x = state.group_pill_screen_x(group, gpu, &self.style);
            let layout = overlay::layout_group_editor(
                anchor_x,
                bar_height(&self.style),
                &self.config,
                width,
                height,
            );
            if overlay::group_editor_hit(&layout, point) != Some(overlay::GroupEditorHit::NameField)
            {
                return None;
            }
            let buffer = state.group_editor.as_ref()?.name_buffer().to_string();
            let byte = overlay_input_byte_at(
                &buffer,
                layout.name_input_rect,
                &self.config,
                point.0,
                gpu.text_measurer(),
            );
            return Some((MainField::GroupEditorName, FieldHit::Byte(byte)));
        }
        if let Some(picker) = state.session_picker.as_ref() {
            let SessionMode::EditingName(field) = picker.mode() else {
                return None;
            };
            let buffer = field.text().to_string();
            let layout = overlay::layout_session_picker(
                picker,
                &self.config,
                &self.style,
                width,
                bar_height(&self.style),
                is_macos(),
                width,
                height,
            );
            if overlay::session_picker_hit(&layout, point)
                != Some(overlay::SessionPickerHit::SaveArea)
            {
                return None;
            }
            let byte = overlay_input_byte_at(
                &buffer,
                layout.save_rect,
                &self.config,
                point.0,
                gpu.text_measurer(),
            );
            return Some((MainField::SessionPickerName, FieldHit::Byte(byte)));
        }
        // No modal surface from here on. A warning over the point owns it.
        let warning_layout = overlay::layout_warnings(
            &state.warnings,
            &self.config,
            bar_height(&self.style),
            width,
            gpu.text_measurer(),
        );
        if overlay::hit_test_warnings(&warning_layout, point).is_some() {
            return None;
        }
        if state.task_manager.is_none()
            && let Some(search_pane) = state.search.as_ref().map(SearchBarState::pane)
        {
            let box_rect = state.pane_box_rect(&self.style, search_pane);
            let layout = search_bar::layout_search_bar(box_rect, &self.style);
            if search_bar::search_bar_hit(&layout, point) == Some(SearchBarHit::Field) {
                let buffer = state.search.as_ref()?.field().text().to_string();
                let byte = search_byte_at(&buffer, layout.field_rect, point.0, gpu.text_measurer());
                return Some((MainField::Search, FieldHit::Byte(byte)));
            }
        }
        if let Some(id) = state.rename.editing_tab()
            && state.in_bar(state.cursor_position.1, &self.style)
            && tab_bar::point_in_track(
                &self.style,
                width,
                is_macos(),
                state.workspace.pinned().len(),
                state.devices_button(),
                point.0,
            )
        {
            let field_screen =
                state.rename_field_screen_rect_now(id, &self.style, gpu.text_measurer())?;
            if tab_bar::rect_contains(field_screen, point) {
                let buffer = state.rename.field()?.text().to_string();
                let byte = rename_byte_at(
                    &buffer,
                    field_screen,
                    &self.style,
                    point.0,
                    gpu.text_measurer(),
                );
                return Some((MainField::Rename, FieldHit::Byte(byte)));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialog::SecretFocus;
    use crate::group_editor::EditorRegion;

    #[derive(Default)]
    struct FakeClipboard {
        content: Option<String>,
        writes: Vec<String>,
    }

    impl FakeClipboard {
        fn holding(text: &str) -> Self {
            Self {
                content: Some(text.to_string()),
                writes: Vec::new(),
            }
        }
    }

    impl ClipboardIo for FakeClipboard {
        fn copy(&mut self, text: &str) {
            self.writes.push(text.to_string());
            self.content = Some(text.to_string());
        }

        fn paste(&mut self) -> Option<String> {
            self.content.clone()
        }
    }

    /// Every surface closed.
    struct World {
        rename: RenameState,
        group_editor: Option<GroupEditor>,
        session_picker: Option<SessionPicker>,
        search: Option<SearchBarState>,
        secret_field: Option<MaskedFieldState>,
        dialog_open: bool,
        task_manager_open: bool,
    }

    impl World {
        fn closed() -> Self {
            Self {
                rename: RenameState::Idle,
                group_editor: None,
                session_picker: None,
                search: None,
                secret_field: None,
                dialog_open: false,
                task_manager_open: false,
            }
        }

        fn resolve(&mut self, target: MainField) -> Option<FieldRef<'_>> {
            resolve_field(
                Surfaces {
                    rename: &mut self.rename,
                    group_editor: &mut self.group_editor,
                    session_picker: &mut self.session_picker,
                    search: &mut self.search,
                    secret_field: &mut self.secret_field,
                    dialog_open: self.dialog_open,
                    task_manager_open: self.task_manager_open,
                },
                target,
            )
        }
    }

    fn rename_of(text: &str) -> RenameState {
        RenameState::Editing {
            tab: TabId::new(1),
            field: TextFieldState::new(text),
        }
    }

    fn editor_of(text: &str) -> GroupEditor {
        GroupEditor::new(
            porecatu_core::GroupId::new(0),
            text,
            0,
            EditorRegion::Swatches,
        )
    }

    fn selecting(text: &str, from: usize, to: usize) -> TextFieldState {
        let mut field = TextFieldState::new(text);
        field.click_at(from);
        field.drag_to(to);
        field
    }

    fn plain(field: &mut TextFieldState) -> FieldRef<'_> {
        FieldRef::Plain {
            field,
            max_chars: None,
        }
    }

    const ALL: [MainField; 5] = [
        MainField::Rename,
        MainField::GroupEditorName,
        MainField::SessionPickerName,
        MainField::Search,
        MainField::Secret,
    ];

    #[test]
    fn with_every_surface_closed_no_target_resolves() {
        let mut world = World::closed();
        for target in ALL {
            assert!(world.resolve(target).is_none(), "{target:?}");
        }
    }

    #[test]
    fn each_target_resolves_to_its_own_field() {
        let mut world = World::closed();
        world.rename = rename_of("rename");
        world.group_editor = Some(editor_of("group"));
        world.session_picker = Some(SessionPicker::open_editing(Vec::new()));
        world.search = Some(SearchBarState::new(
            TabId::new(1),
            porecatu_core::PaneId::new(0),
        ));
        for (target, expected) in [
            (MainField::Rename, "rename"),
            (MainField::GroupEditorName, "group"),
            (MainField::SessionPickerName, ""),
            (MainField::Search, ""),
        ] {
            match world.resolve(target) {
                Some(FieldRef::Plain { field, .. }) => assert_eq!(field.text(), expected),
                _ => panic!("{target:?} should be a plain field"),
            }
        }
        // The secret field needs the dialog, which is not open here.
        assert!(world.resolve(MainField::Secret).is_none());
    }

    #[test]
    fn only_the_sessions_name_has_a_length_cap() {
        let mut world = World::closed();
        world.rename = rename_of("a");
        world.group_editor = Some(editor_of("b"));
        world.session_picker = Some(SessionPicker::open_editing(Vec::new()));
        world.search = Some(SearchBarState::new(
            TabId::new(1),
            porecatu_core::PaneId::new(0),
        ));
        for target in [
            MainField::Rename,
            MainField::GroupEditorName,
            MainField::SessionPickerName,
            MainField::Search,
        ] {
            let Some(FieldRef::Plain { max_chars, .. }) = world.resolve(target) else {
                panic!("{target:?} should resolve");
            };
            let expected = (target == MainField::SessionPickerName).then_some(MAX_NAME_CHARS);
            assert_eq!(max_chars, expected, "{target:?}");
        }
    }

    #[test]
    fn the_sessions_name_exists_only_while_the_popover_edits_it() {
        let mut world = World::closed();
        world.session_picker = Some(SessionPicker::open_browsing(Vec::new()));
        assert!(world.resolve(MainField::SessionPickerName).is_none());
        world.session_picker = Some(SessionPicker::open_editing(Vec::new()));
        assert!(world.resolve(MainField::SessionPickerName).is_some());
        world.session_picker = None;
        assert!(world.resolve(MainField::SessionPickerName).is_none());
    }

    #[test]
    fn the_group_editors_name_resolves_whatever_region_has_the_focus() {
        let mut world = World::closed();
        // Opened on the swatches: `name_field_mut` would refuse it.
        world.group_editor = Some(editor_of("group"));
        assert_eq!(
            world.group_editor.as_ref().map(GroupEditor::focus),
            Some(EditorRegion::Swatches)
        );
        assert!(world.resolve(MainField::GroupEditorName).is_some());
    }

    #[test]
    fn the_menu_target_goes_when_its_surface_goes() {
        let mut world = World::closed();
        world.rename = rename_of("x");
        assert!(world.resolve(MainField::Rename).is_some());
        world.rename = RenameState::Idle;
        assert!(world.resolve(MainField::Rename).is_none());

        world.group_editor = Some(editor_of("x"));
        assert!(world.resolve(MainField::GroupEditorName).is_some());
        world.group_editor = None;
        assert!(world.resolve(MainField::GroupEditorName).is_none());

        world.search = Some(SearchBarState::new(
            TabId::new(1),
            porecatu_core::PaneId::new(0),
        ));
        assert!(world.resolve(MainField::Search).is_some());
        world.search = None;
        assert!(world.resolve(MainField::Search).is_none());
    }

    #[test]
    fn a_dialog_takes_every_field_but_the_secret_one() {
        let mut world = World::closed();
        world.rename = rename_of("x");
        world.group_editor = Some(editor_of("x"));
        world.session_picker = Some(SessionPicker::open_editing(Vec::new()));
        world.search = Some(SearchBarState::new(
            TabId::new(1),
            porecatu_core::PaneId::new(0),
        ));
        world.secret_field = Some(MaskedFieldState::new());
        world.dialog_open = true;
        for target in ALL {
            assert_eq!(
                world.resolve(target).is_some(),
                target == MainField::Secret,
                "{target:?}"
            );
        }
        // The secret field lives only with its dialog.
        world.dialog_open = false;
        assert!(world.resolve(MainField::Secret).is_none());
    }

    #[test]
    fn a_secret_field_resolves_masked_with_the_secret_focus_type_in_scope() {
        // `SecretFocus` is the focus `open_field_menu` gives the field.
        let _ = SecretFocus::Field;
        let mut world = World::closed();
        world.secret_field = Some(MaskedFieldState::new());
        world.dialog_open = true;
        assert!(matches!(
            world.resolve(MainField::Secret),
            Some(FieldRef::Masked(_))
        ));
    }

    #[test]
    fn the_task_manager_takes_the_search_bar_away() {
        let mut world = World::closed();
        world.search = Some(SearchBarState::new(
            TabId::new(1),
            porecatu_core::PaneId::new(0),
        ));
        world.task_manager_open = true;
        assert!(world.resolve(MainField::Search).is_none());
    }

    #[test]
    fn a_right_click_inside_the_selection_keeps_it() {
        let mut field = selecting("hello world", 2, 8);
        place_caret_for_menu(&mut field, 5);
        assert_eq!(field.selection_range(), Some((2, 8)));
        // The edges count as inside.
        place_caret_for_menu(&mut field, 2);
        assert_eq!(field.selection_range(), Some((2, 8)));
        place_caret_for_menu(&mut field, 8);
        assert_eq!(field.selection_range(), Some((2, 8)));
    }

    #[test]
    fn a_right_click_outside_the_selection_moves_the_caret_and_clears_it() {
        let mut field = selecting("hello world", 2, 8);
        place_caret_for_menu(&mut field, 10);
        assert_eq!(field.selection_range(), None);
        assert_eq!(field.cursor(), 10);
        let mut field = selecting("hello world", 2, 8);
        place_caret_for_menu(&mut field, 0);
        assert_eq!(field.selection_range(), None);
        assert_eq!(field.cursor(), 0);
    }

    #[test]
    fn a_right_click_in_a_field_with_no_selection_places_the_caret() {
        let mut field = TextFieldState::new("hello");
        place_caret_for_menu(&mut field, 3);
        assert_eq!(field.cursor(), 3);
        assert_eq!(field.selection_range(), None);
    }

    #[test]
    fn the_masked_right_click_works_in_scalars() {
        let mut field = MaskedFieldState::new();
        field.paste("abcdef");
        field.select_all();
        // Inside the (whole) selection: kept.
        place_masked_caret_for_menu(&mut field, 3);
        assert!(field.display().selection_range().is_some());
        // Outside: a fresh caret. Shrink the selection first.
        field.click_at_scalar(1);
        field.move_right(true);
        field.move_right(true);
        assert!(field.display().selection_range().is_some());
        place_masked_caret_for_menu(&mut field, 5);
        assert_eq!(field.display().selection_range(), None);
    }

    #[test]
    fn the_items_follow_the_selection_and_the_mask() {
        let mut field = selecting("hello", 1, 3);
        let enabled = |field: &FieldRef<'_>| -> Vec<bool> {
            field.menu_items().iter().map(|item| item.enabled).collect()
        };
        assert_eq!(enabled(&plain(&mut field)), [true; 4]);
        field.click_at(0);
        assert_eq!(enabled(&plain(&mut field)), [false, false, true, true]);

        // The masked field never enables Cut or Copy, selected or not.
        let mut masked = MaskedFieldState::new();
        masked.paste("secret");
        assert_eq!(
            enabled(&FieldRef::Masked(&mut masked)),
            [false, false, true, true]
        );
        masked.select_all();
        assert_eq!(
            enabled(&FieldRef::Masked(&mut masked)),
            [false, false, true, true]
        );
    }

    #[test]
    fn copy_puts_the_selection_on_the_clipboard_and_keeps_the_field() {
        let mut field = selecting("hello world", 0, 5);
        let mut io = FakeClipboard::default();
        apply_field_menu_action(plain(&mut field), FieldMenuAction::Copy, &mut io);
        assert_eq!(io.writes, ["hello"]);
        assert_eq!(field.text(), "hello world");
    }

    #[test]
    fn cut_moves_the_selection_to_the_clipboard() {
        let mut field = selecting("hello world", 0, 6);
        let mut io = FakeClipboard::default();
        apply_field_menu_action(plain(&mut field), FieldMenuAction::Cut, &mut io);
        assert_eq!(io.writes, ["hello "]);
        assert_eq!(field.text(), "world");
        assert_eq!(field.cursor(), 0);
    }

    #[test]
    fn copy_and_cut_without_a_selection_do_nothing() {
        let mut field = TextFieldState::new("hello");
        let mut io = FakeClipboard::holding("kept");
        apply_field_menu_action(plain(&mut field), FieldMenuAction::Copy, &mut io);
        apply_field_menu_action(plain(&mut field), FieldMenuAction::Cut, &mut io);
        assert_eq!(field.text(), "hello");
        assert!(io.writes.is_empty());
        assert_eq!(io.content.as_deref(), Some("kept"));
    }

    #[test]
    fn paste_replaces_the_selection_or_inserts_at_the_caret() {
        let mut field = selecting("hello world", 6, 11);
        let mut io = FakeClipboard::holding("there");
        apply_field_menu_action(plain(&mut field), FieldMenuAction::Paste, &mut io);
        assert_eq!(field.text(), "hello there");

        let mut field = TextFieldState::new("ac");
        field.click_at(1);
        apply_field_menu_action(plain(&mut field), FieldMenuAction::Paste, &mut io);
        assert_eq!(field.text(), "atherec");
    }

    #[test]
    fn paste_flattens_lines_and_tabs_to_spaces() {
        let mut field = TextFieldState::new("");
        let mut io = FakeClipboard::holding("a\r\nb\tc\rd\ne\u{7}");
        apply_field_menu_action(plain(&mut field), FieldMenuAction::Paste, &mut io);
        assert_eq!(field.text(), "a b c d e");
    }

    #[test]
    fn an_empty_or_unreadable_clipboard_pastes_nothing() {
        let mut field = TextFieldState::new("keep");
        for mut io in [FakeClipboard::default(), FakeClipboard::holding("")] {
            apply_field_menu_action(plain(&mut field), FieldMenuAction::Paste, &mut io);
        }
        assert_eq!(field.text(), "keep");
    }

    #[test]
    fn select_all_selects_the_whole_field() {
        let mut field = TextFieldState::new("hello");
        let mut io = FakeClipboard::default();
        apply_field_menu_action(plain(&mut field), FieldMenuAction::SelectAll, &mut io);
        assert_eq!(field.selection_range(), Some((0, 5)));
        assert!(io.writes.is_empty());
    }

    #[test]
    fn the_sessions_name_paste_stops_at_the_cap() {
        let mut field = TextFieldState::new("");
        let mut io = FakeClipboard::holding(&"x".repeat(MAX_NAME_CHARS + 20));
        apply_field_menu_action(
            FieldRef::Plain {
                field: &mut field,
                max_chars: Some(MAX_NAME_CHARS),
            },
            FieldMenuAction::Paste,
            &mut io,
        );
        assert_eq!(field.text().chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn the_masked_field_pastes_but_never_copies_or_cuts() {
        let mut field = MaskedFieldState::new();
        let mut io = FakeClipboard::holding("pa\nss");
        apply_field_menu_action(
            FieldRef::Masked(&mut field),
            FieldMenuAction::Paste,
            &mut io,
        );
        assert_eq!(field.len(), 5);
        field.inspect(|text| assert_eq!(text, "pa ss"));

        field.select_all();
        let mut io = FakeClipboard::holding("clip");
        apply_field_menu_action(FieldRef::Masked(&mut field), FieldMenuAction::Copy, &mut io);
        apply_field_menu_action(FieldRef::Masked(&mut field), FieldMenuAction::Cut, &mut io);
        assert!(io.writes.is_empty());
        assert_eq!(field.len(), 5);
    }

    #[test]
    fn the_masked_select_all_selects_every_dot() {
        let mut field = MaskedFieldState::new();
        field.paste("abc");
        let mut io = FakeClipboard::default();
        apply_field_menu_action(
            FieldRef::Masked(&mut field),
            FieldMenuAction::SelectAll,
            &mut io,
        );
        assert!(field.display().selection_range().is_some());
    }
}
