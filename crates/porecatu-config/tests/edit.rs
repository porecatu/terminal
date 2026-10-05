// SPDX-License-Identifier: GPL-3.0-or-later

//! `porecatu_config::edit` (ADR-0058 §2): byte-exact round trips of the
//! example file, one edit touching exactly its own lines, and the rules for
//! `Set`, `Remove`, `StringMap` and line endings. Only the public API.

use std::collections::BTreeMap;

use porecatu_config::{ConfigDocument, Edit, EditError, EditValue, KeyPath};

const EXAMPLE: &str = include_str!("../../../docs/config/porecatu.example.toml");

fn path(dotted: &str) -> KeyPath {
    KeyPath::parse(dotted).unwrap()
}

fn set(dotted: &str, value: EditValue) -> Edit {
    Edit::Set(path(dotted), value)
}

fn remove(dotted: &str) -> Edit {
    Edit::Remove(path(dotted))
}

fn apply(text: &str, edits: &[Edit]) -> String {
    ConfigDocument::parse(text).unwrap().apply(edits).unwrap()
}

fn map(entries: &[(&str, &str)]) -> EditValue {
    EditValue::StringMap(
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<BTreeMap<_, _>>(),
    )
}

/// Lines that differ, as `(removed, added)`: the two texts with their common
/// first and last lines trimmed.
fn diff(old: &str, new: &str) -> (Vec<String>, Vec<String>) {
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let own = |lines: &[&str]| lines.iter().map(|line| (*line).to_owned()).collect();
    (
        own(&old[head..old.len() - tail]),
        own(&new[head..new.len() - tail]),
    )
}

fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn assert_all_crlf(text: &str) {
    for (index, _) in text.match_indices('\n') {
        assert_eq!(
            text.as_bytes()[index.wrapping_sub(1)],
            b'\r',
            "bare LF at byte {index}"
        );
    }
}

// ---------------------------------------------------------------------------
// KeyPath
// ---------------------------------------------------------------------------

#[test]
fn key_path_parses_and_displays_plain_segments() {
    let parsed = path("terminal.font.size");
    assert_eq!(parsed.segments(), ["terminal", "font", "size"]);
    assert_eq!(parsed.to_string(), "terminal.font.size");
}

#[test]
fn key_path_supports_a_quoted_segment() {
    let parsed = path(r#"keybindings.windows."ctrl+shift+o""#);
    assert_eq!(
        parsed.segments(),
        ["keybindings", "windows", "ctrl+shift+o"]
    );
    assert_eq!(parsed.to_string(), r#"keybindings.windows."ctrl+shift+o""#);
    assert_eq!(path(&parsed.to_string()), parsed);
}

#[test]
fn key_path_quoted_segment_may_contain_a_dot() {
    let parsed = path(r#"a."b.c""#);
    assert_eq!(parsed.segments(), ["a", "b.c"]);
    assert_eq!(parsed.to_string(), r#"a."b.c""#);
}

#[test]
fn key_path_from_segments_quotes_what_needs_it() {
    let built = KeyPath::new(["keybindings", "linux", "ctrl+shift+o"]).unwrap();
    assert_eq!(built.to_string(), r#"keybindings.linux."ctrl+shift+o""#);
    assert_eq!(built.child("x").segments().len(), 4);
}

#[test]
fn key_path_rejects_empty_and_malformed() {
    assert_eq!(
        KeyPath::new(Vec::<String>::new()),
        Err(EditError::EmptyKeyPath)
    );
    assert!(matches!(
        KeyPath::parse(""),
        Err(EditError::InvalidKeyPath { .. })
    ));
    assert!(matches!(
        KeyPath::parse("a..b"),
        Err(EditError::InvalidKeyPath { .. })
    ));
    assert!(matches!(
        "a.\"b".parse::<KeyPath>(),
        Err(EditError::InvalidKeyPath { .. })
    ));
}

// ---------------------------------------------------------------------------
// Round trip
// ---------------------------------------------------------------------------

#[test]
fn example_file_round_trips_byte_for_byte() {
    assert_eq!(apply(EXAMPLE, &[]), EXAMPLE);
}

#[test]
fn example_file_round_trips_with_crlf() {
    let text = crlf(EXAMPLE);
    assert_ne!(text, EXAMPLE);
    assert_eq!(apply(&text, &[]), text);
}

const AWKWARD: &str = "\
# header comment

title = \"x\"   # trailing
dotted.key = 1
dotted.other = \"two\"   # kept
\"quoted key\" = true
'literal' = 'C:\\path'
inline = { a = 1, b = \"two\", c = [1, 2] }   # inline

multi = \"\"\"
first
second\"\"\"
list = [
  \"a\",   # first
  \"b\",
]

[table]
\tindented = 1
spaced   =   2    # aligned

[table.sub]
k = 1


# orphan comment

[[array]]
n = 1

[[array]]
n = 2
";

#[test]
fn awkward_forms_round_trip_byte_for_byte() {
    assert_eq!(apply(AWKWARD, &[]), AWKWARD);
    let text = crlf(AWKWARD);
    assert_eq!(apply(&text, &[]), text);
}

#[test]
fn text_without_a_final_newline_round_trips() {
    assert_eq!(apply("a = 1\n[t]\nb = 2", &[]), "a = 1\n[t]\nb = 2");
    assert_eq!(apply("a = 1", &[]), "a = 1");
    assert_eq!(apply("", &[]), "");
}

#[test]
fn apply_does_not_change_the_document() {
    let document = ConfigDocument::parse("a = 1\n").unwrap();
    let edits = [
        set("a", EditValue::Integer(2)),
        set("b.c", EditValue::Bool(true)),
    ];
    let first = document.apply(&edits).unwrap();
    assert_eq!(document.base(), "a = 1\n");
    assert_eq!(document.apply(&edits).unwrap(), first);
    assert_eq!(document.apply(&[]).unwrap(), "a = 1\n");
}

#[test]
fn syntax_error_is_typed_and_located() {
    let err = ConfigDocument::parse("ok = 1\nbad = \n").unwrap_err();
    match err {
        EditError::Syntax { line, column, .. } => {
            assert_eq!(line, Some(2));
            assert!(column.is_some());
        }
        other => panic!("unexpected {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// One edit on the example file
// ---------------------------------------------------------------------------

#[test]
fn example_one_float_edit_changes_exactly_its_line() {
    let out = apply(
        EXAMPLE,
        &[set("terminal.font.size", EditValue::Float(16.0))],
    );
    assert_eq!(
        diff(EXAMPLE, &out),
        (
            vec!["size = 14.0".to_owned()],
            vec!["size = 16.0".to_owned()]
        )
    );
}

#[test]
fn example_set_keeps_the_end_of_line_comment_and_its_spacing() {
    let out = apply(
        EXAMPLE,
        &[set(
            "terminal.selection.copy_on_select",
            EditValue::Bool(true),
        )],
    );
    let (removed, added) = diff(EXAMPLE, &out);
    assert_eq!(removed.len(), 1);
    assert_eq!(added, vec![removed[0].replace("false", "true")]);
    assert!(added[0].ends_with("# RF-10.8"));
}

#[test]
fn example_edits_reparse_as_config() {
    let out = apply(
        EXAMPLE,
        &[
            set("terminal.font.size", EditValue::Float(16.0)),
            set("terminal.selection.copy_on_select", EditValue::Bool(true)),
            remove("terminal.scrollback.lines"),
            set("shell.args", EditValue::StringList(vec!["-l".to_owned()])),
            set("shell.env", map(&[("EDITOR", "vim")])),
        ],
    );
    let (config, _) = porecatu_config::parse(&out).unwrap();
    assert_eq!(config.terminal.font.size, 16.0);
    assert!(config.terminal.selection.copy_on_select);
    assert_eq!(config.shell.args, ["-l"]);
    assert_eq!(config.shell.env["EDITOR"], "vim");
}

#[test]
fn example_remove_drops_only_the_key_line() {
    let out = apply(EXAMPLE, &[remove("terminal.selection.copy_on_select")]);
    let (removed, added) = diff(EXAMPLE, &out);
    assert_eq!(removed.len(), 1);
    assert!(removed[0].starts_with("copy_on_select = false"));
    assert!(added.is_empty());
    // The documentation comment that sat above the key is still there.
    assert!(out.contains("# Selecionar já copia para o clipboard."));
}

#[test]
fn example_remove_of_the_last_key_of_the_file_keeps_its_comments() {
    let out = apply(EXAMPLE, &[remove("panes.min_rows")]);
    let (removed, added) = diff(EXAMPLE, &out);
    assert_eq!(removed, vec!["min_rows = 5".to_owned()]);
    assert!(added.is_empty());
}

#[test]
fn example_string_map_adds_one_line_to_shell_env() {
    let out = apply(EXAMPLE, &[set("shell.env", map(&[("EDITOR", "vim")]))]);
    assert_eq!(
        diff(EXAMPLE, &out),
        (Vec::new(), vec!["EDITOR = \"vim\"".to_owned()])
    );
}

// ---------------------------------------------------------------------------
// Set
// ---------------------------------------------------------------------------

#[test]
fn set_replaces_the_value_and_inherits_its_decor() {
    let base = "[t]\nsize = 14.0   # RF-5.3\nother = 1\n";
    assert_eq!(
        apply(base, &[set("t.size", EditValue::Float(16.0))]),
        "[t]\nsize = 16.0   # RF-5.3\nother = 1\n"
    );
}

#[test]
fn set_replaces_inside_an_inline_table() {
    let base = "t = { a = 1, b = 2 }   # note\n";
    assert_eq!(
        apply(base, &[set("t.b", EditValue::Integer(5))]),
        "t = { a = 1, b = 5 }   # note\n"
    );
}

#[test]
fn set_replaces_a_dotted_key() {
    let base = "x.y = 1   # c\nx.z = 2\n";
    assert_eq!(
        apply(base, &[set("x.y", EditValue::Integer(9))]),
        "x.y = 9   # c\nx.z = 2\n"
    );
}

#[test]
fn set_through_a_quoted_segment() {
    let base = "[keybindings.windows]\n\"ctrl+shift+o\" = \"tab.new\"   # mine\n";
    assert_eq!(
        apply(
            base,
            &[Edit::Set(
                path(r#"keybindings.windows."ctrl+shift+o""#),
                EditValue::String("settings.open".to_owned()),
            )]
        ),
        "[keybindings.windows]\n\"ctrl+shift+o\" = \"settings.open\"   # mine\n"
    );
}

#[test]
fn set_missing_key_goes_to_the_end_of_its_table() {
    let base = "[a]\nx = 1\n\n[b]\ny = 2\n";
    assert_eq!(
        apply(base, &[set("a.z", EditValue::Bool(true))]),
        "[a]\nx = 1\nz = true\n\n[b]\ny = 2\n"
    );
}

#[test]
fn set_missing_key_follows_the_indentation_of_its_siblings() {
    assert_eq!(
        apply("[a]\n  x = 1\n", &[set("a.z", EditValue::Integer(2))]),
        "[a]\n  x = 1\n  z = 2\n"
    );
}

#[test]
fn set_missing_key_in_an_inline_table_and_a_dotted_table() {
    assert_eq!(
        apply("t = { a = 1 }\n", &[set("t.b", EditValue::Integer(2))]),
        "t = { a = 1, b = 2 }\n"
    );
    assert_eq!(
        apply("x.y = 1\n", &[set("x.z", EditValue::Integer(2))]),
        "x.y = 1\nx.z = 2\n"
    );
}

#[test]
fn set_missing_root_key_joins_the_root_table() {
    assert_eq!(
        apply("a = 1\n[t]\nx = 1\n", &[set("b", EditValue::Bool(true))]),
        "a = 1\nb = true\n[t]\nx = 1\n"
    );
}

#[test]
fn set_missing_table_is_created_at_the_end_after_a_blank_line() {
    assert_eq!(
        apply("[a]\nx = 1\n", &[set("b.y", EditValue::Integer(2))]),
        "[a]\nx = 1\n\n[b]\ny = 2\n"
    );
}

#[test]
fn set_missing_table_is_a_pure_append_when_the_file_ends_with_a_blank_line() {
    let base = "[a]\nx = 1\n\n";
    let out = apply(base, &[set("b.y", EditValue::Integer(2))]);
    assert_eq!(out, "[a]\nx = 1\n\n[b]\ny = 2\n");
    assert!(out.starts_with(base));
}

#[test]
fn set_missing_table_goes_after_a_footer_comment() {
    assert_eq!(
        apply(
            "[a]\nx = 1\n\n# footer\n",
            &[set("b.y", EditValue::Integer(2))]
        ),
        "[a]\nx = 1\n\n# footer\n\n[b]\ny = 2\n"
    );
}

#[test]
fn set_missing_table_in_an_empty_document_has_no_leading_blank_line() {
    assert_eq!(
        apply("", &[set("a.b", EditValue::Bool(true))]),
        "[a]\nb = true\n"
    );
    assert_eq!(
        apply("", &[set("top", EditValue::Bool(true))]),
        "top = true\n"
    );
}

#[test]
fn set_missing_nested_tables_print_only_the_innermost_header() {
    assert_eq!(
        apply("[a]\nx = 1\n", &[set("c.d.e", EditValue::Bool(true))]),
        "[a]\nx = 1\n\n[c.d]\ne = true\n"
    );
}

#[test]
fn set_missing_sub_table_of_an_existing_table_still_goes_to_the_end() {
    assert_eq!(
        apply(
            "[terminal]\nk = 1\n\n[other]\no = 1\n",
            &[set("terminal.font.size", EditValue::Float(14.0))]
        ),
        "[terminal]\nk = 1\n\n[other]\no = 1\n\n[terminal.font]\nsize = 14.0\n"
    );
}

#[test]
fn set_two_new_tables_keeps_their_order_and_never_reorders() {
    assert_eq!(
        apply(
            "[a]\nx = 1\n",
            &[
                set("b.y", EditValue::Integer(2)),
                set("c.z", EditValue::Integer(3)),
                set("b.w", EditValue::Integer(4)),
            ]
        ),
        "[a]\nx = 1\n\n[b]\ny = 2\nw = 4\n\n[c]\nz = 3\n"
    );
}

#[test]
fn set_missing_key_under_an_inline_table_creates_inline_tables() {
    assert_eq!(
        apply("t = { }\n", &[set("t.u.v", EditValue::Integer(1))]),
        "t = { u = { v = 1 } }\n"
    );
}

#[test]
fn set_scalar_forms() {
    let out = apply(
        "",
        &[
            set("f", EditValue::Float(16.0)),
            set("g", EditValue::Float(0.5)),
            set("i", EditValue::Integer(-3)),
            set("b", EditValue::Bool(false)),
            set("s", EditValue::String("a \"q\" \\ b".to_owned())),
            set(
                "l",
                EditValue::StringList(vec!["x".to_owned(), "y z".to_owned()]),
            ),
            set("e", EditValue::StringList(Vec::new())),
        ],
    );
    assert!(out.contains("f = 16.0\n"), "{out}");
    assert!(out.contains("g = 0.5\n"), "{out}");
    assert!(out.contains("i = -3\n"), "{out}");
    assert!(out.contains("b = false\n"), "{out}");
    assert!(out.contains("l = [\"x\", \"y z\"]\n"), "{out}");
    assert!(out.contains("e = []\n"), "{out}");
    let table: toml::Table = out.parse().unwrap();
    assert_eq!(table["s"].as_str(), Some("a \"q\" \\ b"));
    assert_eq!(table["f"].as_float(), Some(16.0));
}

#[test]
fn set_list_replaces_an_existing_list_keeping_the_comment() {
    assert_eq!(
        apply(
            "args = []   # none\n",
            &[set("args", EditValue::StringList(vec!["-l".to_owned()]))]
        ),
        "args = [\"-l\"]   # none\n"
    );
}

// ---------------------------------------------------------------------------
// Remove
// ---------------------------------------------------------------------------

#[test]
fn remove_moves_the_comment_block_to_the_next_key() {
    assert_eq!(
        apply(
            "a = 1\n# doc b\nb = 2   # gone\n# doc c\nc = 3\n",
            &[remove("b")]
        ),
        "a = 1\n# doc b\n# doc c\nc = 3\n"
    );
}

#[test]
fn remove_first_key_moves_the_comment_block_to_the_next_key() {
    assert_eq!(
        apply("[t]\n# doc a\na = 1\nb = 2\n", &[remove("t.a")]),
        "[t]\n# doc a\nb = 2\n"
    );
}

#[test]
fn remove_last_key_of_the_last_table_keeps_the_comment_at_the_end() {
    assert_eq!(
        apply("[t]\nx = 1\n# doc y\ny = 2\n", &[remove("t.y")]),
        "[t]\nx = 1\n# doc y\n"
    );
}

#[test]
fn remove_last_key_of_a_table_followed_by_another_table() {
    let base = "# top\n[a]\n# doc x\nx = 1\n# doc y\ny = 2\n\n# about b\n[b]\nz = 3\n";
    assert_eq!(
        apply(base, &[remove("a.y")]),
        "# top\n[a]\n# doc x\nx = 1\n# doc y\n\n# about b\n[b]\nz = 3\n"
    );
}

#[test]
fn remove_last_key_skips_an_implicit_table_with_no_header() {
    let base = "[a]\n# doc x\nx = 1\n\n[b.c]\nz = 3\n";
    assert_eq!(
        apply(base, &[remove("a.x")]),
        "[a]\n# doc x\n\n[b.c]\nz = 3\n"
    );
}

#[test]
fn remove_the_only_key_of_a_table_leaves_the_header() {
    assert_eq!(
        apply("[a]\n# doc x\nx = 1\n\n[b]\ny = 1\n", &[remove("a.x")]),
        "[a]\n# doc x\n\n[b]\ny = 1\n"
    );
}

#[test]
fn remove_last_dotted_key_hands_the_comment_to_the_next_line() {
    assert_eq!(
        apply(
            "# doc y\nx.y = 1\n# doc z\nx.z = 2\nw = 3\n",
            &[remove("x.z")]
        ),
        "# doc y\nx.y = 1\n# doc z\nw = 3\n"
    );
}

#[test]
fn remove_from_an_inline_table() {
    let out = apply("t = { a = 1, b = 2 }   # note\n", &[remove("t.b")]);
    let table: toml::Table = out.parse().unwrap();
    assert_eq!(table["t"].as_table().unwrap().len(), 1);
    assert!(out.ends_with("# note\n"), "{out}");
}

#[test]
fn remove_of_an_absent_key_changes_nothing() {
    let base = "[a]\nx = 1\n";
    assert_eq!(apply(base, &[remove("a.nope")]), base);
    assert_eq!(apply(base, &[remove("nope.deeper")]), base);
}

#[test]
fn remove_then_set_the_same_key_writes_it_fresh() {
    assert_eq!(
        apply(
            "[a]\n# doc x\nx = 1   # old\ny = 2\n",
            &[remove("a.x"), set("a.x", EditValue::Integer(5))]
        ),
        "[a]\n# doc x\ny = 2\nx = 5\n"
    );
}

// ---------------------------------------------------------------------------
// StringMap
// ---------------------------------------------------------------------------

#[test]
fn string_map_keeps_the_entries_that_stay_and_changes_the_rest() {
    let base = "[shell.env]\nA = \"1\"   # keep\n# doc B\nB = \"2\"\nE = \"5\"   # stays\n";
    let out = apply(
        base,
        &[set("shell.env", map(&[("A", "9"), ("E", "5"), ("D", "4")]))],
    );
    assert_eq!(
        out,
        "[shell.env]\nA = \"9\"   # keep\n# doc B\nE = \"5\"   # stays\nD = \"4\"\n"
    );
}

#[test]
fn string_map_creates_the_table_at_the_end() {
    assert_eq!(
        apply(
            "[shell]\nprogram = \"\"\n",
            &[set("shell.env", map(&[("A", "1"), ("B", "2")]))]
        ),
        "[shell]\nprogram = \"\"\n\n[shell.env]\nA = \"1\"\nB = \"2\"\n"
    );
}

#[test]
fn string_map_empty_on_an_absent_table_writes_nothing() {
    let base = "[shell]\nprogram = \"\"\n";
    assert_eq!(apply(base, &[set("shell.env", map(&[]))]), base);
}

#[test]
fn string_map_empty_on_an_existing_table_empties_it_and_keeps_the_header() {
    assert_eq!(
        apply(
            "[shell.env]\nA = \"1\"\nB = \"2\"\n\n[x]\ny = 1\n",
            &[set("shell.env", map(&[]))]
        ),
        "[shell.env]\n\n[x]\ny = 1\n"
    );
}

#[test]
fn string_map_over_an_inline_table() {
    let out = apply(
        "[shell]\nenv = { A = \"1\", B = \"2\" }\n",
        &[set("shell.env", map(&[("A", "3")]))],
    );
    let table: toml::Table = out.parse().unwrap();
    let env = table["shell"]["env"].as_table().unwrap();
    assert_eq!(env.len(), 1);
    assert_eq!(env["A"].as_str(), Some("3"));
}

// ---------------------------------------------------------------------------
// Line endings
// ---------------------------------------------------------------------------

#[test]
fn crlf_base_stays_crlf_after_edits() {
    let base = crlf(EXAMPLE);
    let out = apply(
        &base,
        &[
            set("terminal.font.size", EditValue::Float(16.0)),
            remove("terminal.selection.copy_on_select"),
            set("shell.env", map(&[("EDITOR", "vim")])),
            set("brand.new", EditValue::Bool(true)),
        ],
    );
    assert_all_crlf(&out);
    let lf = apply(
        EXAMPLE,
        &[
            set("terminal.font.size", EditValue::Float(16.0)),
            remove("terminal.selection.copy_on_select"),
            set("shell.env", map(&[("EDITOR", "vim")])),
            set("brand.new", EditValue::Bool(true)),
        ],
    );
    assert_eq!(out, crlf(&lf));
}

#[test]
fn lf_base_stays_lf_after_edits() {
    let out = apply(EXAMPLE, &[set("brand.new", EditValue::Bool(true))]);
    assert!(!out.contains('\r'));
}

#[test]
fn a_base_without_a_final_newline_does_not_gain_one() {
    assert_eq!(apply("a = 1", &[set("a", EditValue::Integer(2))]), "a = 2");
    assert_eq!(
        apply("a = 1", &[set("b", EditValue::Bool(true))]),
        "a = 1\nb = true"
    );
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[test]
fn set_under_a_value_is_not_a_table() {
    let document = ConfigDocument::parse("[a]\nb = 1\n").unwrap();
    assert_eq!(
        document.apply(&[set("a.b.c", EditValue::Bool(true))]),
        Err(EditError::NotATable { path: path("a.b") })
    );
}

#[test]
fn set_under_an_array_of_tables_is_not_a_table() {
    let document = ConfigDocument::parse("[[themes]]\nname = \"x\"\n").unwrap();
    assert_eq!(
        document.apply(&[set("themes.name", EditValue::String("y".to_owned()))]),
        Err(EditError::NotATable {
            path: path("themes")
        })
    );
}

#[test]
fn a_string_map_over_a_plain_value_is_not_a_table() {
    let document = ConfigDocument::parse("[shell]\nenv = 1\n").unwrap();
    assert_eq!(
        document.apply(&[set("shell.env", map(&[("A", "1")]))]),
        Err(EditError::NotATable {
            path: path("shell.env")
        })
    );
}

#[test]
fn a_scalar_over_a_table_is_not_a_value() {
    let document = ConfigDocument::parse("[a.b]\nc = 1\n").unwrap();
    assert_eq!(
        document.apply(&[set("a.b", EditValue::Bool(true))]),
        Err(EditError::NotAValue { path: path("a.b") })
    );
}

#[test]
fn remove_of_a_table_is_not_a_value() {
    let document = ConfigDocument::parse("[a.b]\nc = 1\n").unwrap();
    assert_eq!(
        document.apply(&[remove("a.b")]),
        Err(EditError::NotAValue { path: path("a.b") })
    );
}

#[test]
fn one_failing_edit_fails_the_whole_list() {
    let document = ConfigDocument::parse("a = 1\n[t]\nx = 1\n").unwrap();
    let result = document.apply(&[
        set("a", EditValue::Integer(2)),
        set("t.x.y", EditValue::Integer(3)),
    ]);
    assert!(result.is_err());
    assert_eq!(document.apply(&[]).unwrap(), "a = 1\n[t]\nx = 1\n");
}
