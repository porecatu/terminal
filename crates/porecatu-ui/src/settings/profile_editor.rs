// SPDX-License-Identifier: GPL-3.0-or-later

//! The profile editor's session state (ADR-0080 §6): a **copy** of one profile,
//! edited apart from the `ProfilesDraft`. Pure state, no `winit` and no disk.
//!
//! - the copy remembers where it came from: an item of the draft by its stable
//!   `key` (never the position, which moves when another profile is deleted),
//!   or nothing, for a profile created by the editor;
//! - every operation is `Form`'s own (`profiles_draft`): the editor adds only
//!   the bookkeeping the draft cannot give it, which is the `id` link to the
//!   name;
//! - opening, validating and applying (OK) live on `ProfilesDraft`, because
//!   that is where "the draft with the copy in place" exists. Cancelling is
//!   dropping the editor: the draft never saw it.

use std::collections::BTreeMap;

use porecatu_config::{ProfileKind, ProfileProblemKind};

use super::profiles_draft::{Field, Forget, Form, ListKind, ProfilesDraft, SecretKind};

/// Where the copy came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    /// An item of the draft, by its `key`. If the item is gone when OK comes
    /// (a reload, a deletion), the copy goes back in as a new profile.
    Existing { key: u64 },
    /// A profile the editor creates; nothing exists until OK.
    New,
}

impl Target {
    pub(super) fn key(self) -> Option<u64> {
        match self {
            Self::Existing { key } => Some(key),
            Self::New => None,
        }
    }
}

/// Where the focus goes when OK is refused: the first invalid control, in the
/// order the form shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    Field(Field),
    /// An item of the environment list (an empty or repeated name).
    EnvItem(usize),
}

/// Why OK was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditorProblems {
    pub(crate) fields: BTreeMap<Field, ProfileProblemKind>,
    pub(crate) env_item: Option<usize>,
    pub(crate) first: Option<Focus>,
}

impl EditorProblems {
    pub(crate) fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.env_item.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileEditor {
    target: Target,
    form: Form,
    /// The `id` still follows the name (a new profile whose `id` nobody edited).
    id_follows_name: bool,
}

impl ProfileEditor {
    pub(super) fn from_parts(target: Target, form: Form, id_follows_name: bool) -> Self {
        Self {
            target,
            form,
            id_follows_name,
        }
    }

    pub(super) fn target(&self) -> Target {
        self.target
    }

    pub(super) fn id_follows_name(&self) -> bool {
        self.id_follows_name
    }

    pub(crate) fn form(&self) -> &Form {
        &self.form
    }

    pub(crate) fn kind(&self) -> ProfileKind {
        self.form.kind
    }

    /// The editor creates a profile, instead of editing one of the draft.
    pub(crate) fn is_new(&self) -> bool {
        self.target == Target::New
    }

    /// Writes a text field. `draft` is only read, for the ids a name-derived
    /// `id` must not repeat.
    pub(crate) fn set_text(&mut self, draft: &ProfilesDraft, field: Field, value: &str) {
        let taken = draft.other_ids(self.target);
        self.form
            .set_text(field, value, &mut self.id_follows_name, &taken);
    }

    pub(crate) fn set_args(&mut self, args: Vec<String>) {
        self.form.set_args(args);
    }

    pub(crate) fn set_env(&mut self, env: Vec<(String, String)>) {
        self.form.set_env(env);
    }

    pub(crate) fn set_auth_preset(&mut self, preset: usize) {
        self.form.set_auth_preset(preset);
    }

    pub(crate) fn list_rows(&self, kind: ListKind) -> Vec<(String, String)> {
        self.form.list_rows(kind)
    }

    pub(crate) fn set_list_text(
        &mut self,
        kind: ListKind,
        item: usize,
        second: bool,
        text: &str,
    ) -> bool {
        self.form.set_list_text(kind, item, second, text)
    }

    pub(crate) fn list_add(&mut self, kind: ListKind) -> Option<usize> {
        self.form.list_add(kind)
    }

    pub(crate) fn list_remove(&mut self, kind: ListKind, item: usize) -> bool {
        self.form.list_remove(kind, item)
    }

    pub(crate) fn env_item_invalid(&self, item: usize) -> bool {
        self.form.env_item_invalid(item)
    }

    /// What "Esquecer" wipes from the vault for the copy as it stands. The
    /// forgetting is immediate and does not wait for OK (ADR-0080 §6).
    pub(crate) fn forget_target(&self, kind: SecretKind) -> Option<Forget> {
        self.form.forget_target(kind)
    }
}
