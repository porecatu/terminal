// SPDX-License-Identifier: GPL-3.0-or-later

//! `porecatu_config::edit` over an array of tables (ADR-0075 §6): append an
//! element, set or drop a key of an element **found by its `id`**, remove an
//! element. Comments and formatting outside the touched lines survive.

use std::collections::BTreeMap;

use porecatu_config::{ConfigDocument, Edit, EditError, EditValue, KeyPath};

const LIST: &str = "profiles.list";

const THREE: &str = r#"# my config
[shell]
program = "pwsh" # the default

[profiles]
detect = false # keep it off

# the first one
[[profiles.list]]
id = "one"
name = "One"
badge = "ONE"
program = "cmd.exe"

# the second one, an SSH server
[[profiles.list]]
id = "two"
name = "Two"
type = "ssh"
host = "10.0.0.2" # production
user = "deploy"

[[profiles.list]]
id = "three"
name = "Three"
badge = "THR"
program = "bash"
"#;

fn list() -> KeyPath {
    KeyPath::parse(LIST).unwrap()
}

fn text(value: &str) -> EditValue {
    EditValue::String(value.to_owned())
}

fn apply(base: &str, edits: &[Edit]) -> String {
    ConfigDocument::parse(base).unwrap().apply(edits).unwrap()
}

fn set_in(id: &str, key: &str, value: EditValue) -> Edit {
    Edit::SetInTable {
        array: list(),
        id: id.to_owned(),
        key: key.to_owned(),
        value,
    }
}

fn remove_table(id: &str) -> Edit {
    Edit::RemoveTable {
        array: list(),
        id: id.to_owned(),
    }
}

fn loads(text: &str) -> porecatu_config::Config {
    porecatu_config::parse(text)
        .expect("the edited text loads")
        .0
}

#[test]
fn appending_keeps_every_comment_and_adds_the_element_at_the_end() {
    let out = apply(
        THREE,
        &[Edit::AppendTable {
            array: list(),
            entries: vec![
                ("id".to_owned(), text("four")),
                ("name".to_owned(), text("Four")),
                ("badge".to_owned(), text("FOUR")),
                ("program".to_owned(), text("zsh")),
            ],
        }],
    );

    assert!(
        out.starts_with(THREE),
        "everything before is untouched:\n{out}"
    );
    assert!(out.contains(
        "[[profiles.list]]\nid = \"four\"\nname = \"Four\"\nbadge = \"FOUR\"\nprogram = \"zsh\""
    ));
    let ids: Vec<&str> = out
        .lines()
        .filter_map(|line| line.strip_prefix("id = "))
        .collect();
    assert_eq!(ids, ["\"one\"", "\"two\"", "\"three\"", "\"four\""]);
    assert_eq!(loads(&out).profiles.list.len(), 4);
}

#[test]
fn appending_to_a_file_with_no_profiles_creates_the_array() {
    let out = apply(
        "[shell]\nprogram = \"pwsh\"\n",
        &[Edit::AppendTable {
            array: list(),
            entries: vec![
                ("id".to_owned(), text("a")),
                ("name".to_owned(), text("A")),
                ("badge".to_owned(), text("A")),
                ("program".to_owned(), text("cmd.exe")),
            ],
        }],
    );

    assert!(out.starts_with("[shell]\nprogram = \"pwsh\"\n"));
    assert!(out.contains("[[profiles.list]]"));
    assert_eq!(loads(&out).profiles.list.len(), 1);
}

#[test]
fn a_string_list_and_a_map_are_written_as_values_of_the_element() {
    let mut env = BTreeMap::new();
    env.insert("EDITOR".to_owned(), "vim".to_owned());
    let out = apply(
        THREE,
        &[
            set_in(
                "one",
                "args",
                EditValue::StringList(vec!["-NoLogo".to_owned(), "-X".to_owned()]),
            ),
            set_in("one", "env", EditValue::StringMap(env)),
        ],
    );

    let profile = &loads(&out).profiles.list[0];
    assert_eq!(profile.args, ["-NoLogo", "-X"]);
    assert_eq!(profile.env.get("EDITOR").map(String::as_str), Some("vim"));
}

#[test]
fn setting_a_key_finds_the_element_by_id_and_keeps_the_line_comment() {
    let out = apply(THREE, &[set_in("two", "host", text("10.0.0.99"))]);

    assert!(out.contains("host = \"10.0.0.99\" # production"), "{out}");
    // Only that one line changed.
    let changed = THREE
        .lines()
        .zip(out.lines())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(changed, 1);
}

#[test]
fn setting_an_absent_key_adds_it_to_that_element_only() {
    let out = apply(THREE, &[set_in("three", "badge", text("B"))]);

    let profiles = loads(&out).profiles.list;
    assert_eq!(profiles[2].badge, "B");
    assert_ne!(profiles[0].badge, "B");
    assert_ne!(profiles[1].badge, "B");
}

#[test]
fn removing_the_first_then_editing_the_third_hits_the_right_profile() {
    // The index of "three" is 2 before and 1 after: by `id`, it does not matter.
    let out = apply(
        THREE,
        &[
            remove_table("one"),
            set_in("three", "program", text("fish")),
        ],
    );

    let profiles = loads(&out).profiles.list;
    let ids: Vec<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["two", "three"]);
    assert_eq!(profiles[1].program, "fish");
    assert!(out.contains("# the second one, an SSH server"));
    assert!(out.contains("# my config"));
    assert!(out.contains("detect = false # keep it off"));
}

#[test]
fn removing_a_key_of_an_element_drops_only_its_line() {
    let out = apply(
        THREE,
        &[Edit::RemoveInTable {
            array: list(),
            id: "two".to_owned(),
            key: "user".to_owned(),
        }],
    );

    assert!(!out.contains("user = \"deploy\""));
    assert!(out.contains("host = \"10.0.0.2\" # production"));
    // An absent key is not an error.
    let again = apply(
        &out,
        &[Edit::RemoveInTable {
            array: list(),
            id: "two".to_owned(),
            key: "user".to_owned(),
        }],
    );
    assert_eq!(again, out);
}

#[test]
fn removing_every_element_leaves_the_rest_of_the_file() {
    let out = apply(
        THREE,
        &[
            remove_table("one"),
            remove_table("two"),
            remove_table("three"),
        ],
    );

    assert!(out.contains("detect = false # keep it off"));
    assert!(loads(&out).profiles.list.is_empty());
}

#[test]
fn an_id_that_does_not_exist_is_a_typed_error_and_nothing_changes() {
    let document = ConfigDocument::parse(THREE).unwrap();
    let wrong = |edit: Edit| document.apply(&[edit]);

    let expected = EditError::NoSuchElement {
        array: list(),
        id: "ghost".to_owned(),
    };
    assert_eq!(
        wrong(set_in("ghost", "name", text("x"))),
        Err(expected.clone())
    );
    assert_eq!(wrong(remove_table("ghost")), Err(expected.clone()));
    assert_eq!(
        wrong(Edit::RemoveInTable {
            array: list(),
            id: "ghost".to_owned(),
            key: "name".to_owned(),
        }),
        Err(expected)
    );
    assert_eq!(document.apply(&[]).unwrap(), THREE);
}

#[test]
fn editing_a_file_with_no_array_is_a_typed_error() {
    let document = ConfigDocument::parse("[shell]\nprogram = \"x\"\n").unwrap();
    assert_eq!(
        document.apply(&[remove_table("one")]),
        Err(EditError::NoSuchElement {
            array: list(),
            id: "one".to_owned(),
        })
    );
}

#[test]
fn an_edit_that_aims_at_a_plain_value_is_an_error() {
    let document = ConfigDocument::parse("[profiles]\nlist = 3\n").unwrap();
    assert!(matches!(
        document.apply(&[remove_table("one")]),
        Err(EditError::NoSuchElement { .. })
    ));
}

#[test]
fn a_crlf_file_stays_crlf() {
    let base = THREE.replace('\n', "\r\n");
    let out = ConfigDocument::parse(&base)
        .unwrap()
        .apply(&[
            remove_table("one"),
            Edit::AppendTable {
                array: list(),
                entries: vec![("id".to_owned(), text("four"))],
            },
        ])
        .unwrap();

    assert!(!out.replace("\r\n", "").contains('\n'), "no bare LF");
}

#[test]
fn the_whole_list_fails_as_one() {
    let document = ConfigDocument::parse(THREE).unwrap();
    let result = document.apply(&[remove_table("one"), set_in("ghost", "name", text("x"))]);
    assert!(result.is_err());
    assert_eq!(document.apply(&[]).unwrap(), THREE);
}
