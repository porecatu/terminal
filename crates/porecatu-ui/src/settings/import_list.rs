// SPDX-License-Identifier: GPL-3.0-or-later

//! The "Importar do ~/.ssh/config" list (ADR-0075 §7): what the reader found
//! and which candidates the user checked. Pure state; the file is read by the
//! window and the sentences come from the catalog in `content`.

use porecatu_config::{
    SshConfigImport, SshConfigImportError, SshConfigImportErrorKind, SshImportCandidate,
    SshImportStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImportList {
    /// There is no `~/.ssh/config`.
    Missing,
    /// The file has no concrete `Host`.
    Empty,
    /// The reader refused the file: nothing can be imported.
    Refused {
        line: Option<usize>,
        kind: SshConfigImportErrorKind,
    },
    Candidates {
        items: Vec<SshImportCandidate>,
        checked: Vec<bool>,
    },
}

impl ImportList {
    pub(crate) fn from_result(result: Result<SshConfigImport, SshConfigImportError>) -> Self {
        match result {
            Err(SshConfigImportError { line, kind }) => Self::Refused { line, kind },
            Ok(import) if !import.file_found => Self::Missing,
            Ok(import) if import.candidates.is_empty() => Self::Empty,
            Ok(import) => Self::Candidates {
                checked: vec![false; import.candidates.len()],
                items: import.candidates,
            },
        }
    }

    /// Checks or unchecks candidate `index`. Only an importable one can be
    /// checked: the others are disabled.
    pub(crate) fn toggle(&mut self, index: usize) {
        if let Self::Candidates { items, checked } = self
            && items
                .get(index)
                .is_some_and(|item| item.status == SshImportStatus::Importable)
            && let Some(slot) = checked.get_mut(index)
        {
            *slot = !*slot;
        }
    }

    pub(crate) fn is_checked(&self, index: usize) -> bool {
        match self {
            Self::Candidates { checked, .. } => checked.get(index).copied().unwrap_or(false),
            _ => false,
        }
    }

    /// The checked candidates, in file order.
    pub(crate) fn chosen(&self) -> Vec<&SshImportCandidate> {
        match self {
            Self::Candidates { items, checked } => items
                .iter()
                .zip(checked)
                .filter_map(|(item, on)| on.then_some(item))
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(alias: &str, status: SshImportStatus) -> SshImportCandidate {
        SshImportCandidate {
            alias: alias.to_owned(),
            host: format!("{alias}.example"),
            user: "u".to_owned(),
            port: 22,
            identity_file: String::new(),
            status,
        }
    }

    fn found(candidates: Vec<SshImportCandidate>) -> ImportList {
        ImportList::from_result(Ok(SshConfigImport {
            file_found: true,
            candidates,
        }))
    }

    #[test]
    fn the_four_outcomes_become_four_states() {
        assert_eq!(
            ImportList::from_result(Ok(SshConfigImport {
                file_found: false,
                candidates: Vec::new()
            })),
            ImportList::Missing
        );
        assert_eq!(found(Vec::new()), ImportList::Empty);
        assert!(matches!(
            ImportList::from_result(Err(SshConfigImportError {
                line: Some(3),
                kind: SshConfigImportErrorKind::Match
            })),
            ImportList::Refused {
                line: Some(3),
                kind: SshConfigImportErrorKind::Match
            }
        ));
        assert!(matches!(
            found(vec![candidate("a", SshImportStatus::Importable)]),
            ImportList::Candidates { .. }
        ));
    }

    #[test]
    fn only_an_importable_candidate_can_be_checked() {
        let mut list = found(vec![
            candidate("a", SshImportStatus::Importable),
            candidate("b", SshImportStatus::AlreadyImported),
            candidate("c", SshImportStatus::Importable),
        ]);
        list.toggle(0);
        list.toggle(1);
        list.toggle(2);
        list.toggle(2);
        assert!(list.is_checked(0));
        assert!(!list.is_checked(1));
        assert!(!list.is_checked(2));
        let aliases: Vec<_> = list.chosen().iter().map(|c| c.alias.as_str()).collect();
        assert_eq!(aliases, ["a"]);
    }

    #[test]
    fn nothing_starts_checked_and_a_refused_file_chooses_nothing() {
        let list = found(vec![candidate("a", SshImportStatus::Importable)]);
        assert!(list.chosen().is_empty());
        let mut refused = ImportList::Refused {
            line: None,
            kind: SshConfigImportErrorKind::InvalidValue,
        };
        refused.toggle(0);
        assert!(refused.chosen().is_empty());
    }
}
