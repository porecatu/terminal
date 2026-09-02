// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.*]` -- PRD-004 (chrome) e PRD-010/ADR-0014 (aviso,
//! diálogo, menu de contexto). Ver `docs/config/porecatu.example.toml`.

mod context_menu;
mod dialog;
mod group_editor;
mod groups;
mod move_to_group;
mod notices;
mod tabs;
mod terminal_frame;
mod tooltip;
mod window;
mod window_controls;

pub use context_menu::ContextMenu;
pub use dialog::Dialog;
pub use group_editor::GroupEditor;
pub use groups::{GroupPaletteEntry, Groups};
pub use move_to_group::MoveToGroup;
pub use notices::Notices;
pub use tabs::{CloseButtonVisibility, Tabs, TabsColors, TabsOverflow, TabsRename};
pub use terminal_frame::TerminalFrame;
pub use tooltip::Tooltip;
pub use window::{TabBarPosition, Window};
pub use window_controls::WindowControls;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct Appearance {
    pub window: Window,
    pub window_controls: WindowControls,
    pub tabs: Tabs,
    pub groups: Groups,
    pub notices: Notices,
    pub dialog: Dialog,
    pub context_menu: ContextMenu,
    pub tooltip: Tooltip,
    pub group_editor: GroupEditor,
    pub move_to_group: MoveToGroup,
    pub terminal_frame: TerminalFrame,
}
