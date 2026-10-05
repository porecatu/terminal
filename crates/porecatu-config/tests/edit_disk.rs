// SPDX-License-Identifier: GPL-3.0-or-later

//! `ConfigDocument::save` (ADR-0058 §3, §4): revalidation before the disk,
//! atomic write on the real target, conflict by content, template for a file
//! that does not exist yet.

use std::fs;

use porecatu_config::{ConfigDocument, Edit, EditError, EditValue, KeyPath};

const EXAMPLE: &str = include_str!("../../../docs/config/porecatu.example.toml");

fn set(dotted: &str, value: EditValue) -> Edit {
    Edit::Set(KeyPath::parse(dotted).unwrap(), value)
}

fn size(value: f64) -> Edit {
    set("terminal.font.size", EditValue::Float(value))
}

const BASE: &str = "[terminal.font]\nsize = 14.0   # keep\n\n[general]\n";

#[test]
fn save_writes_exactly_what_apply_returns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("porecatu.toml");
    fs::write(&path, BASE).unwrap();

    let mut document = ConfigDocument::parse(BASE).unwrap();
    let expected = document.apply(&[size(16.0)]).unwrap();
    document.save(&path, &[size(16.0)]).unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
    assert_eq!(document.base(), expected);
    assert!(expected.contains("size = 16.0   # keep"));
    assert!(!dir.path().join("porecatu.toml.tmp").exists());
}

#[test]
fn invalid_result_is_refused_and_the_file_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("porecatu.toml");
    fs::write(&path, BASE).unwrap();

    let mut document = ConfigDocument::parse(BASE).unwrap();
    let err = document
        .save(
            &path,
            &[set(
                "terminal.font.size",
                EditValue::String("big".to_owned()),
            )],
        )
        .unwrap_err();

    match err {
        EditError::Invalid(error) => assert!(error.line.is_some()),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), BASE);
    assert_eq!(document.base(), BASE);
}

#[test]
fn unknown_keys_do_not_block() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("porecatu.toml");
    let base = "mystery = 1\n[terminal.font]\nsize = 14.0\n";
    fs::write(&path, base).unwrap();

    let mut document = ConfigDocument::parse(base).unwrap();
    let outcome = document.save(&path, &[size(15.0)]).unwrap();
    assert_eq!(outcome.unknown_keys, ["mystery"]);
    assert!(fs::read_to_string(&path).unwrap().contains("mystery = 1"));
}

#[test]
fn file_changed_after_reading_is_a_conflict_and_merge_keeps_both() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("porecatu.toml");
    fs::write(&path, BASE).unwrap();
    let mut document = ConfigDocument::parse(BASE).unwrap();

    let external = "[terminal.font]\nsize = 14.0   # keep\n\n[general]\nlanguage = \"pt_BR\"\n";
    fs::write(&path, external).unwrap();

    let err = document.save(&path, &[size(16.0)]).unwrap_err();
    let EditError::Changed { current } = err else {
        panic!("expected Changed");
    };
    assert_eq!(current, external);
    assert_eq!(fs::read_to_string(&path).unwrap(), external);

    document.rebase(&current).unwrap();
    document.save(&path, &[size(16.0)]).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("size = 16.0   # keep"));
    assert!(saved.contains("language = \"pt_BR\""));
}

#[test]
fn saving_twice_never_conflicts_with_itself() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("porecatu.toml");
    fs::write(&path, BASE).unwrap();
    let mut document = ConfigDocument::parse(BASE).unwrap();

    document.save(&path, &[size(16.0)]).unwrap();
    document.save(&path, &[size(18.0)]).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains("size = 18.0"));
}

#[test]
fn deleted_file_is_a_conflict_with_empty_current() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("porecatu.toml");
    let mut document = ConfigDocument::parse(BASE).unwrap();
    assert_eq!(
        document.save(&path, &[size(16.0)]),
        Err(EditError::Changed {
            current: String::new()
        })
    );
    assert!(!path.exists());
}

#[test]
fn missing_file_with_a_template_is_created_from_it() {
    let dir = tempfile::tempdir().unwrap();
    // The directory does not exist either.
    let path = dir.path().join("nested").join("porecatu.toml");

    let mut document = ConfigDocument::from_template(EXAMPLE).unwrap();
    document
        .save(
            &path,
            &[set(
                "terminal.selection.copy_on_select",
                EditValue::Bool(true),
            )],
        )
        .unwrap();

    let saved = fs::read_to_string(&path).unwrap();
    let (config, _) = porecatu_config::parse(&saved).unwrap();
    assert!(config.terminal.selection.copy_on_select);
    assert!(saved.contains("# Selecionar já copia para o clipboard."));
    // Saved once: it is a plain document now.
    document.save(&path, &[size(15.0)]).unwrap();
}

#[test]
fn a_file_that_appears_under_a_template_is_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("porecatu.toml");
    let mut document = ConfigDocument::from_template(EXAMPLE).unwrap();
    fs::write(&path, "[general]\n").unwrap();

    assert_eq!(
        document.save(&path, &[size(16.0)]),
        Err(EditError::Changed {
            current: "[general]\n".to_owned()
        })
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "[general]\n");
}

#[test]
fn crlf_file_is_written_back_as_crlf() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("porecatu.toml");
    let base = BASE.replace('\n', "\r\n");
    fs::write(&path, &base).unwrap();

    let mut document = ConfigDocument::parse(&base).unwrap();
    document.save(&path, &[size(16.0)]).unwrap();
    let saved = fs::read(&path).unwrap();
    assert!(saved.windows(2).filter(|pair| pair == b"\r\n").count() >= 4);
    assert!(
        !String::from_utf8(saved)
            .unwrap()
            .replace("\r\n", "")
            .contains('\n')
    );
}

#[test]
fn save_follows_a_symlink_and_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("dotfiles");
    let config = dir.path().join("config");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&config).unwrap();
    let real = repo.join("porecatu.toml");
    let link = config.join("porecatu.toml");
    fs::write(&real, BASE).unwrap();

    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&real, &link);
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&real, &link);
    if let Err(err) = linked {
        eprintln!("skipped: cannot create a symlink here ({err})");
        return;
    }

    let mut document = ConfigDocument::parse(BASE).unwrap();
    document.save(&link, &[size(16.0)]).unwrap();

    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link was replaced by a regular file"
    );
    assert!(fs::read_to_string(&real).unwrap().contains("size = 16.0"));
    assert!(!repo.join("porecatu.toml.tmp").exists());
    assert!(!config.join("porecatu.toml.tmp").exists());
}
