// SPDX-License-Identifier: GPL-3.0-or-later

//! The window-aware side of "Move to window..." (PRD-022 RF-22.20 to
//! RF-22.24, ADR-0090 §6): opening the popover, which needs every window of
//! the process to build its rows, and carrying out the choice, which is a
//! call to [`App::transfer`].
//!
//! What is moved and the rows are pure and live in `move_to_window.rs`; the
//! popover is drawn by `overlay.rs` with the geometry of the "Move to group"
//! one. The popover belongs to the window that opened it
//! (`WindowState::move_to_window`) and keeps, beside the popover state, the
//! [`TransferPayload`] decided **when it opened**: choosing a row only says
//! where it goes.

use super::*;
use crate::app_transfer::{NewWindowPlacement, TransferDest, TransferPayload};
use crate::move_to_window::{self, WindowTarget};

/// What a popover of "Move to window..." keeps for the window that owns it.
pub(crate) type MoveToWindowState = (TransferPayload, MoveToWindowPopover<WindowId>);

/// What running an action of the group list asks of the caller: everything
/// but "Move to window..." is carried out by [`WindowState::run_group_action`]
/// itself, and that one needs the other windows (`App`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub(crate) enum GroupActionOutcome {
    Done,
    /// The caller opens the popover of destinations for the whole group.
    OpenMoveToWindow,
}

/// Where the popover of "Move to window..." hangs when the group editor opens
/// it: the point the editor itself hangs from (horizontally on the group's
/// pill, just under the bar, `[appearance.group_editor] offset_y` below it).
pub(crate) fn group_editor_anchor(
    anchor_x: f32,
    style: &TabBarStyle,
    config: &porecatu_config::Config,
) -> (f32, f32) {
    (
        anchor_x,
        bar_height(style) + config.appearance.group_editor.offset_y as f32,
    )
}

impl App {
    /// Opens the popover in `window_id` for `payload`, hanging from `anchor`
    /// (window coordinates). The rows are a snapshot of the other windows at
    /// this moment, most recently used first, minimized ones included.
    pub(crate) fn open_move_to_window(
        &mut self,
        window_id: WindowId,
        payload: TransferPayload,
        anchor: (f32, f32),
    ) {
        let rows = move_to_window::window_rows(
            window_id,
            self.windows_by_recent_focus().into_iter().filter_map(|id| {
                let state = self.windows.get(&id)?;
                Some((
                    id,
                    move_to_window::active_tab_title(&state.workspace),
                    move_to_window::tab_total(&state.workspace),
                ))
            }),
        );
        let Some(state) = self.windows.get_mut(&window_id) else {
            return;
        };
        state.close_all_popovers();
        state.move_to_window = Some((payload, MoveToWindowPopover::new(anchor, rows)));
        state.window.request_redraw();
    }

    /// Carries out a row chosen in the popover (RF-22.22, RF-22.23): the
    /// payload lands at the end of the chosen window's track (a pinned tab at
    /// the end of its zone), or in a window born for it, in the cascade of
    /// `window.new`. A window that no longer exists only closes the popover,
    /// which the caller already did.
    pub(crate) fn run_move_to_window_choice(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        payload: TransferPayload,
        target: WindowTarget<WindowId>,
    ) {
        let to = match target {
            WindowTarget::Window(window) => {
                if !self.windows.contains_key(&window) {
                    return;
                }
                TransferDest::Window(window, move_to_window::destination_insert_at(&payload))
            }
            WindowTarget::NewWindow => TransferDest::NewWindow(NewWindowPlacement::Cascade),
        };
        self.transfer(event_loop, window_id, payload, to);
        if let Some(state) = self.windows.get(&window_id) {
            state.window.request_redraw();
        }
    }
}

impl WindowState {
    /// What `tab.move_to_window` (`group == false`) or `group.move_to_window`
    /// (`group == true`) opens, from the keyboard: the payload and the point
    /// the popover hangs from, the lower-left corner of the active tab on the
    /// bar (of the group's label for the group action). `None` when there is
    /// nothing to move -- always so for the group action outside an explicit
    /// group, which is inert (RF-22.24).
    pub(crate) fn move_to_window_by_key(
        &self,
        group: bool,
        gpu: &mut GpuContext,
        style: &TabBarStyle,
    ) -> Option<(TransferPayload, (f32, f32))> {
        let payload = if group {
            move_to_window::group_move_payload(&self.workspace)?
        } else {
            move_to_window::tab_move_payload(&self.workspace, &self.selection, None)?
        };
        let active = self.workspace.active_tab()?;
        let pinned = self.workspace.pinned();
        let rect = if let Some(index) = pinned.iter().position(|&tab| tab == active) {
            // The zone does not scroll (ADR-0087 §4).
            Some(tab_bar::pinned_rect(style, is_macos(), index))
        } else {
            let layout = tab_bar::fit_width(
                &self.workspace,
                style,
                self.logical_width,
                gpu.text_measurer(),
                is_macos(),
            );
            let group_label = match &payload {
                TransferPayload::Group(id) => tab_bar::pill_rect(&layout, *id),
                _ => None,
            };
            let rect = group_label.or_else(|| tab_bar::tab_rect(&layout, active))?;
            // A box scrolled out of the track still anchors at the track's
            // edge, never over the fixed buttons on the right.
            let track_left = tab_bar::track_shift(style, is_macos(), pinned.len());
            let track_width =
                tab_bar::trilha_width(style, self.logical_width, is_macos(), pinned.len());
            let x = track_left
                + (rect.x - self.scroll_offset).clamp(0.0, (track_width - rect.width).max(0.0));
            Some(Rect { x, ..rect })
        };
        let anchor = rect.map_or((0.0, bar_height(style)), |rect| {
            (rect.x, rect.y + rect.height)
        });
        Some((payload, anchor))
    }

    /// The keyboard of the popover (mirrors `handle_move_to_group_key`):
    /// arrows move the highlight, `Esc` closes, `Enter` closes and returns the
    /// payload with the chosen destination.
    pub(crate) fn handle_move_to_window_key(
        &mut self,
        event: &KeyEvent,
    ) -> Option<(TransferPayload, WindowTarget<WindowId>)> {
        if event.state != ElementState::Pressed {
            return None;
        }
        let (_, popover) = self.move_to_window.as_mut()?;
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.move_to_window = None;
                None
            }
            Key::Named(NamedKey::ArrowUp) => {
                popover.move_highlight(-1);
                None
            }
            Key::Named(NamedKey::ArrowDown) => {
                popover.move_highlight(1);
                None
            }
            Key::Named(NamedKey::Enter) => {
                let target = popover.selected();
                let (payload, _) = self.move_to_window.take()?;
                Some((payload, target))
            }
            _ => None,
        }
    }
}
