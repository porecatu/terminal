// SPDX-License-Identifier: GPL-3.0-or-later

//! Key-by-key edits of the user's `porecatu.toml` (ADR-0058 §2).
//!
//! The settings screen never serializes a `Config`: it produces a list of
//! [`Edit`]s (`Set`/`Remove` over a [`KeyPath`]) and [`ConfigDocument::apply`]
//! replays them over the text the user wrote, with `toml_edit`, so comments,
//! blank lines, key order, unknown keys and the line ending survive byte for
//! byte outside the keys that were touched.
//!
//! This module only knows text and its own types: no disk (task 02), no
//! `parse` revalidation (task 02), and no `toml_edit` type in its public API.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use toml_edit::{Decor, DocumentMut, InlineTable, Item, Key, RawString, Table, TableLike, Value};

use crate::error::line_column_at;

/// Dotted path of a key, e.g. `terminal.font.size` or
/// `keybindings.windows."ctrl+shift+o"`. Never empty; a segment is a whole
/// key, so it may contain dots or any other character.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyPath(Vec<String>);

impl KeyPath {
    /// Builds a path from already-split segments.
    pub fn new<I, S>(segments: I) -> Result<Self, EditError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let segments: Vec<String> = segments.into_iter().map(Into::into).collect();
        if segments.is_empty() {
            return Err(EditError::EmptyKeyPath);
        }
        Ok(Self(segments))
    }

    /// Parses the dotted form; a segment with special characters is quoted
    /// (`a."b.c"`), with the same grammar as a TOML key.
    pub fn parse(dotted: &str) -> Result<Self, EditError> {
        let keys = Key::parse(dotted).map_err(|_| EditError::InvalidKeyPath {
            input: dotted.to_owned(),
        })?;
        Self::new(keys.iter().map(|key| key.get().to_owned())).map_err(|_| {
            EditError::InvalidKeyPath {
                input: dotted.to_owned(),
            }
        })
    }

    pub fn segments(&self) -> &[String] {
        &self.0
    }

    /// This path with one more segment at the end.
    pub fn child(&self, segment: impl Into<String>) -> Self {
        let mut segments = self.0.clone();
        segments.push(segment.into());
        Self(segments)
    }

    fn prefix(&self, len: usize) -> Self {
        Self(self.0[..len].to_vec())
    }
}

impl FromStr for KeyPath {
    type Err = EditError;

    fn from_str(dotted: &str) -> Result<Self, Self::Err> {
        Self::parse(dotted)
    }
}

impl fmt::Display for KeyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, segment) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(".")?;
            }
            f.write_str(&Key::new(segment.as_str()).display_repr())?;
        }
        Ok(())
    }
}

/// A value the screen can write. Typed, never interface text (ADR-0056).
#[derive(Debug, Clone, PartialEq)]
pub enum EditValue {
    Bool(bool),
    Integer(i64),
    /// Always written with a decimal part (`16.0`, never `16`): the `Config`
    /// field is `f64` and the example file spells it that way.
    Float(f64),
    String(String),
    StringList(Vec<String>),
    /// Applied as per-key edits inside the table (`[shell.env]`).
    StringMap(BTreeMap<String, String>),
}

/// One pending change.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Set(KeyPath, EditValue),
    /// Drops the key's line. Removing an absent key is a no-op.
    Remove(KeyPath),
}

/// Why an edit could not be built or applied. Typed, no interface text: the
/// sentence is composed in `porecatu-ui` (ADR-0056).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// A [`KeyPath`] with no segment.
    EmptyKeyPath,
    /// Text that is not a dotted key path.
    InvalidKeyPath { input: String },
    /// The base text is not valid TOML. `detail` is the parser's own message.
    Syntax {
        line: Option<usize>,
        column: Option<usize>,
        detail: String,
    },
    /// A segment of `path` exists but is not a table (or is an array of
    /// tables), so nothing can live under it; also a `StringMap` aimed at a
    /// plain value.
    NotATable { path: KeyPath },
    /// `path` names a table where a value was expected (`Set` of a scalar,
    /// `Remove`).
    NotAValue { path: KeyPath },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKeyPath => f.write_str("empty key path"),
            Self::InvalidKeyPath { input } => write!(f, "invalid key path: {input:?}"),
            Self::Syntax {
                line: Some(line),
                column: Some(column),
                detail,
            } => write!(f, "line {line}, column {column}: {detail}"),
            Self::Syntax { detail, .. } => f.write_str(detail),
            Self::NotATable { path } => write!(f, "`{path}` is not a table"),
            Self::NotAValue { path } => write!(f, "`{path}` is a table, not a value"),
        }
    }
}

impl std::error::Error for EditError {}

/// The text the user wrote plus its syntax tree. Immutable: [`apply`] returns
/// the new text and leaves this document as it was.
///
/// [`apply`]: ConfigDocument::apply
#[derive(Debug, Clone)]
pub struct ConfigDocument {
    base: String,
    doc: DocumentMut,
    crlf: bool,
}

impl ConfigDocument {
    pub fn parse(text: &str) -> Result<Self, EditError> {
        let doc: DocumentMut = text.parse().map_err(|err: toml_edit::TomlError| {
            let position = err.span().map(|span| line_column_at(text, span.start));
            EditError::Syntax {
                line: position.map(|(line, _)| line),
                column: position.map(|(_, column)| column),
                detail: err.message().to_owned(),
            }
        })?;
        // The first line break decides, as ADR-0058 §2 says.
        let crlf = text
            .find('\n')
            .is_some_and(|index| index > 0 && text.as_bytes()[index - 1] == b'\r');
        Ok(Self {
            base: text.to_owned(),
            doc,
            crlf,
        })
    }

    /// The text this document was parsed from.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Applies `edits` in order over the base and returns the new text. Fails
    /// as a whole: on error nothing is returned, and `self` never changes.
    pub fn apply(&self, edits: &[Edit]) -> Result<String, EditError> {
        let mut doc = self.doc.clone();
        for edit in edits {
            match edit {
                Edit::Set(path, value) => set(&mut doc, path, value)?,
                Edit::Remove(path) => remove(&mut doc, path)?,
            }
        }
        Ok(self.render(&doc))
    }

    fn render(&self, doc: &DocumentMut) -> String {
        let mut text = doc.to_string();
        // `toml_edit` ends every line it writes itself with a bare `\n`, so a
        // CRLF base is brought back to CRLF in one pass. An LF base is left
        // exactly as the tree prints it.
        if self.crlf {
            text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        }
        // The tree always terminates the last line; a base that did not do
        // so keeps not doing so.
        if !self.base.is_empty() && !self.base.ends_with('\n') {
            if text.ends_with("\r\n") {
                text.truncate(text.len() - 2);
            } else if text.ends_with('\n') {
                text.truncate(text.len() - 1);
            }
        }
        text
    }
}

// ---------------------------------------------------------------------------
// Set
// ---------------------------------------------------------------------------

fn set(doc: &mut DocumentMut, path: &KeyPath, value: &EditValue) -> Result<(), EditError> {
    let new = match value {
        EditValue::Bool(value) => Value::from(*value),
        EditValue::Integer(value) => Value::from(*value),
        EditValue::Float(value) => Value::from(*value),
        EditValue::String(value) => Value::from(value.as_str()),
        EditValue::StringList(values) => Value::Array(values.iter().map(String::as_str).collect()),
        EditValue::StringMap(map) => return set_map(doc, path, map),
    };
    set_value(doc, path, new)
}

/// Table-like shape of a container, which decides how a missing table under
/// it is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Root or `[header]` table.
    Header,
    /// Table reached by a dotted key (`a.b = 1`).
    Dotted,
    /// `{ inline = "table" }`.
    Inline,
}

fn kind_of(item: &Item) -> Kind {
    match item {
        Item::Table(table) if table.is_dotted() => Kind::Dotted,
        Item::Table(_) => Kind::Header,
        _ => Kind::Inline,
    }
}

struct Probe {
    /// How many leading parent segments already exist as tables.
    depth: usize,
    /// Kind of the deepest existing one (the root is `Header`).
    kind: Kind,
}

fn probe(root: &Table, path: &KeyPath, parents: &[String]) -> Result<Probe, EditError> {
    let mut current: &dyn TableLike = root;
    let mut kind = Kind::Header;
    for (index, segment) in parents.iter().enumerate() {
        let Some(item) = current.get(segment) else {
            return Ok(Probe { depth: index, kind });
        };
        let Some(next) = item.as_table_like() else {
            return Err(EditError::NotATable {
                path: path.prefix(index + 1),
            });
        };
        kind = kind_of(item);
        current = next;
    }
    Ok(Probe {
        depth: parents.len(),
        kind,
    })
}

fn walk_mut<'a>(root: &'a mut Table, segments: &[String]) -> Option<&'a mut dyn TableLike> {
    let mut current: &mut dyn TableLike = root;
    for segment in segments {
        current = current.get_mut(segment)?.as_table_like_mut()?;
    }
    Some(current)
}

fn set_value(doc: &mut DocumentMut, path: &KeyPath, mut new: Value) -> Result<(), EditError> {
    let Some((leaf, parents)) = path.segments().split_last() else {
        return Err(EditError::EmptyKeyPath);
    };
    let found = probe(doc.as_table(), path, parents)?;

    if found.depth == parents.len() {
        let parent = walk_mut(doc.as_table_mut(), parents)
            .ok_or_else(|| EditError::NotATable { path: path.clone() })?;
        return match parent.get_mut(leaf) {
            Some(Item::Value(old)) => {
                // The old value's decor is the space after `=` and the
                // end-of-line comment: the new value inherits both.
                *new.decor_mut() = old.decor().clone();
                *old = new;
                Ok(())
            }
            Some(Item::None) | None => {
                insert_leaf(parent, found.kind, leaf, new);
                Ok(())
            }
            Some(_) => Err(EditError::NotAValue { path: path.clone() }),
        };
    }

    // Some table on the way is missing: create the chain in one piece under
    // the deepest one that exists.
    let anchor = &parents[..found.depth];
    let missing = &parents[found.depth..];
    let header = (found.kind == Kind::Header).then(|| NewHeader {
        prefix: new_table_prefix(doc),
        position: next_position(doc.as_table()),
    });
    let mut child_name = leaf.as_str();
    let mut child = Item::Value(new);
    for (index, name) in missing.iter().enumerate().rev() {
        let innermost = index == missing.len() - 1;
        child = wrap(
            found.kind,
            child_name,
            child,
            header.as_ref().filter(|_| innermost),
        );
        child_name = name;
    }
    let parent = walk_mut(doc.as_table_mut(), anchor)
        .ok_or_else(|| EditError::NotATable { path: path.clone() })?;
    match (found.kind, child) {
        (Kind::Inline, Item::Value(value)) => insert_inline(parent, child_name, value),
        (_, child) => {
            parent.insert(child_name, child);
        }
    }
    Ok(())
}

struct NewHeader {
    prefix: String,
    position: isize,
}

/// A table holding `name = child`, written the way the container writes its
/// tables. `header` is `Some` for the innermost table of a `[header]` chain,
/// the only one that prints a header line; the outer ones stay implicit.
fn wrap(kind: Kind, name: &str, child: Item, header: Option<&NewHeader>) -> Item {
    match kind {
        Kind::Header => {
            let mut table = Table::new();
            match header {
                Some(header) => {
                    table.set_position(Some(header.position));
                    table.decor_mut().set_prefix(header.prefix.clone());
                }
                None => table.set_implicit(true),
            }
            table.insert(name, child);
            Item::Table(table)
        }
        Kind::Dotted => {
            let mut table = Table::new();
            table.set_dotted(true);
            table.insert(name, child);
            Item::Table(table)
        }
        Kind::Inline => {
            let mut table = InlineTable::new();
            table.insert(name, as_value(child));
            Item::Value(Value::InlineTable(table))
        }
    }
}

fn as_value(item: Item) -> Value {
    match item {
        Item::Value(value) => value,
        // `wrap` only ever nests values and inline tables here.
        _ => Value::InlineTable(InlineTable::new()),
    }
}

/// Appends `leaf = value` at the end of the table, indented like its
/// siblings.
fn insert_leaf(parent: &mut dyn TableLike, kind: Kind, leaf: &str, value: Value) {
    if kind == Kind::Inline {
        return insert_inline(parent, leaf, value);
    }
    let indent = (kind == Kind::Header)
        .then(|| sibling_indent(parent))
        .flatten();
    parent.insert(leaf, Item::Value(value));
    if let Some(indent) = indent
        && let Some(mut key) = parent.key_mut(leaf)
    {
        key.leaf_decor_mut().set_prefix(indent);
    }
}

/// Appends `name = value` to an inline table. The space before the closing
/// brace belongs to the last value, so it moves to the new one; otherwise the
/// old last value would print `{ a = 1 , b = 2 }`.
fn insert_inline(parent: &mut dyn TableLike, name: &str, mut value: Value) {
    let closing = parent
        .iter_mut()
        .filter_map(|(_, item)| item.as_value_mut())
        .last()
        .and_then(|last| {
            let suffix = last.decor().suffix().cloned();
            last.decor_mut().set_suffix("");
            suffix
        });
    *value.decor_mut() = Decor::new(" ", closing.unwrap_or_default());
    parent.insert(name, Item::Value(value));
}

/// Indentation of the last plain `key = value` line of the table, if any.
fn sibling_indent(table: &dyn TableLike) -> Option<String> {
    let last = table
        .iter()
        .filter(|(_, item)| matches!(item, Item::Value(_)))
        .last()?;
    let prefix = table.key(last.0)?.leaf_decor().prefix()?.as_str()?;
    let indent = &prefix[prefix.rfind('\n').map_or(0, |index| index + 1)..];
    (!indent.is_empty()).then(|| indent.to_owned())
}

/// Decor prefix for a `[table]` created at the end of the document: a blank
/// line before it. Whatever the document ended with (a footer comment, blank
/// lines) is moved in front of it, so it keeps hugging the table it belonged
/// to and the new text is a pure append.
fn new_table_prefix(doc: &mut DocumentMut) -> String {
    let trailing = doc.trailing().as_str().unwrap_or("").to_owned();
    if doc.as_table().is_empty() && trailing.trim().is_empty() {
        return String::new();
    }
    doc.set_trailing("");
    let mut prefix = trailing;
    if !prefix.is_empty() && !prefix.ends_with('\n') {
        prefix.push('\n');
    }
    let flat = prefix.replace('\r', "");
    if !(flat == "\n" || flat.ends_with("\n\n")) {
        prefix.push('\n');
    }
    prefix
}

fn next_position(root: &Table) -> isize {
    textual_tables(root)
        .iter()
        .map(|table| table.position)
        .max()
        .map_or(1, |max| max + 1)
}

// ---------------------------------------------------------------------------
// StringMap
// ---------------------------------------------------------------------------

fn set_map(
    doc: &mut DocumentMut,
    path: &KeyPath,
    map: &BTreeMap<String, String>,
) -> Result<(), EditError> {
    let existing: Vec<String> = match get_item(doc.as_table(), path)? {
        None => Vec::new(),
        Some(item) => match item.as_table_like() {
            Some(table) => table.iter().map(|(key, _)| key.to_owned()).collect(),
            None => return Err(EditError::NotATable { path: path.clone() }),
        },
    };
    // A missing table is created by the first entry's `Set`.
    for (key, value) in map {
        set(
            doc,
            &path.child(key.as_str()),
            &EditValue::String(value.clone()),
        )?;
    }
    for key in existing {
        if !map.contains_key(&key) {
            remove(doc, &path.child(key))?;
        }
    }
    Ok(())
}

fn get_item<'a>(root: &'a Table, path: &KeyPath) -> Result<Option<&'a Item>, EditError> {
    let mut current: &dyn TableLike = root;
    let segments = path.segments();
    for (index, segment) in segments.iter().enumerate() {
        let Some(item) = current.get(segment) else {
            return Ok(None);
        };
        if index + 1 == segments.len() {
            return Ok(Some(item));
        }
        current = item.as_table_like().ok_or_else(|| EditError::NotATable {
            path: path.prefix(index + 1),
        })?;
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Remove
// ---------------------------------------------------------------------------

/// Where the comment block that sat above a removed key goes.
enum Deposit {
    /// Above this other key (full path), the next line of the table.
    Key(Vec<String>),
    /// Above the header of this table: the removed key was the last of its
    /// table, so the next thing in the text is the next header.
    Header(Vec<(String, usize)>),
    /// After the last table of the document.
    Trailer,
    /// Inside the closing of this inline table.
    InlineEnd(Vec<String>),
}

fn remove(doc: &mut DocumentMut, path: &KeyPath) -> Result<(), EditError> {
    let Some((leaf, parents)) = path.segments().split_last() else {
        return Err(EditError::EmptyKeyPath);
    };
    let found = probe(doc.as_table(), path, parents)?;
    if found.depth < parents.len() {
        return Ok(());
    }
    let parent = walk_mut(doc.as_table_mut(), parents)
        .ok_or_else(|| EditError::NotATable { path: path.clone() })?;
    match parent.get(leaf) {
        Some(Item::Value(_)) => {}
        Some(Item::None) | None => return Ok(()),
        Some(_) => return Err(EditError::NotAValue { path: path.clone() }),
    }

    // The comment block: every whole line of the key's prefix. What follows
    // the last line break is indentation, which goes with the line.
    let head = parent
        .key(leaf)
        .and_then(|key| key.leaf_decor().prefix())
        .and_then(RawString::as_str)
        .and_then(|prefix| prefix.rfind('\n').map(|index| prefix[..=index].to_owned()))
        .unwrap_or_default();

    // Where it goes has to be found while the key is still there.
    let deposit = (!head.is_empty()).then(|| find_deposit(doc.as_table(), parents, leaf));
    if let Some(parent) = walk_mut(doc.as_table_mut(), parents) {
        parent.remove(leaf);
    }
    if let Some(deposit) = deposit {
        place(doc, deposit, &head);
    }
    Ok(())
}

fn find_deposit(root: &Table, parents: &[String], leaf: &str) -> Deposit {
    // Containers from the root down to the one that held the key.
    let mut chain: Vec<(&dyn TableLike, Kind)> = vec![(root, Kind::Header)];
    for segment in parents {
        let Some(item) = chain
            .last()
            .and_then(|(container, _)| container.get(segment))
        else {
            break;
        };
        let Some(table) = item.as_table_like() else {
            break;
        };
        chain.push((table, kind_of(item)));
    }

    for depth in (0..chain.len()).rev() {
        let (container, kind) = chain[depth];
        let after = if depth == parents.len() {
            leaf
        } else {
            parents[depth].as_str()
        };
        if let Some(rest) = next_line_after(container, after) {
            let mut names = parents[..depth].to_vec();
            names.extend(rest);
            return Deposit::Key(names);
        }
        // A dotted table has no end of its own: the line after it is the
        // parent's. Anything else is where the lines stop.
        match kind {
            Kind::Dotted => continue,
            Kind::Inline => return Deposit::InlineEnd(parents[..depth].to_vec()),
            Kind::Header => return header_end(root, &parents[..depth]),
        }
    }
    Deposit::Trailer
}

/// Names (from `container`) of the first line after `after`: a plain value
/// or the first line of a dotted table. Sub-tables with their own header are
/// printed later and are not lines of this table.
fn next_line_after(container: &dyn TableLike, after: &str) -> Option<Vec<String>> {
    container
        .iter()
        .skip_while(|(key, _)| *key != after)
        .skip(1)
        .find_map(|(key, item)| line_in(key, item))
}

fn line_in(key: &str, item: &Item) -> Option<Vec<String>> {
    let nested = |table: &dyn TableLike| {
        table
            .iter()
            .find_map(|(inner, item)| line_in(inner, item))
            .map(|mut names| {
                names.insert(0, key.to_owned());
                names
            })
    };
    match item {
        Item::Value(Value::InlineTable(table)) if table.is_dotted() => nested(table),
        Item::Value(_) => Some(vec![key.to_owned()]),
        Item::Table(table) if table.is_dotted() => nested(table),
        _ => None,
    }
}

/// The next printed header after the table at `names`, else the document's
/// tail.
fn header_end(root: &Table, names: &[String]) -> Deposit {
    let tables = textual_tables(root);
    let Some(index) = tables.iter().position(|table| {
        !table.array
            && table.path.len() == names.len()
            && table
                .path
                .iter()
                .zip(names)
                .all(|((key, _), name)| key == name)
    }) else {
        return Deposit::Trailer;
    };
    tables[index + 1..]
        .iter()
        .find(|table| table.visible)
        .map_or(Deposit::Trailer, |table| {
            Deposit::Header(table.path.clone())
        })
}

fn place(doc: &mut DocumentMut, deposit: Deposit, head: &str) {
    match deposit {
        Deposit::Key(names) => {
            let Some((last, parents)) = names.split_last() else {
                return;
            };
            if let Some(container) = walk_mut(doc.as_table_mut(), parents)
                && let Some(mut key) = container.key_mut(last)
            {
                let old = key
                    .leaf_decor()
                    .prefix()
                    .and_then(RawString::as_str)
                    .unwrap_or_default()
                    .to_owned();
                key.leaf_decor_mut().set_prefix(format!("{head}{old}"));
            }
        }
        Deposit::Header(path) => {
            if let Some(table) = table_mut(doc.as_table_mut(), &path) {
                // No prefix recorded means the printer's default: a blank
                // line.
                let old = table
                    .decor()
                    .prefix()
                    .map_or("\n", |prefix| prefix.as_str().unwrap_or_default())
                    .to_owned();
                table.decor_mut().set_prefix(format!("{head}{old}"));
            }
        }
        Deposit::Trailer => {
            let old = doc.trailing().as_str().unwrap_or_default().to_owned();
            doc.set_trailing(format!("{head}{old}"));
        }
        Deposit::InlineEnd(names) => {
            let Some((last, parents)) = names.split_last() else {
                return;
            };
            let Some(container) = walk_mut(doc.as_table_mut(), parents) else {
                return;
            };
            if let Some(Item::Value(Value::InlineTable(table))) = container.get_mut(last) {
                let old = table.trailing().as_str().unwrap_or_default().to_owned();
                table.set_trailing(format!("{head}{old}"));
            }
        }
    }
}

fn table_mut<'a>(root: &'a mut Table, path: &[(String, usize)]) -> Option<&'a mut Table> {
    let mut current = root;
    for (key, index) in path {
        current = match current.get_mut(key)? {
            Item::Table(table) => table,
            Item::ArrayOfTables(array) => array.get_mut(*index)?,
            _ => return None,
        };
    }
    Some(current)
}

// ---------------------------------------------------------------------------
// Tables in the order they are printed
// ---------------------------------------------------------------------------

struct TableRef {
    /// Keys from the root, each with the element index for `[[arrays]]`.
    path: Vec<(String, usize)>,
    array: bool,
    position: isize,
    /// Prints a header line (an implicit table with no values does not).
    visible: bool,
}

/// Every table in the order `toml_edit` prints them: the root first, then by
/// `position`, with a table that has none printed where the one before it
/// was. Mirrors the printer, because "the next header" is defined by it.
fn textual_tables(root: &Table) -> Vec<TableRef> {
    fn visit(
        table: &Table,
        path: &mut Vec<(String, usize)>,
        array: bool,
        last: &mut isize,
        out: &mut Vec<TableRef>,
    ) {
        if !table.is_dotted() {
            if let Some(position) = table.position() {
                *last = position;
            }
            out.push(TableRef {
                path: path.clone(),
                array,
                position: *last,
                visible: array || !(table.is_implicit() && table.get_values().is_empty()),
            });
        }
        for (key, item) in table.iter() {
            match item {
                Item::Table(child) => {
                    path.push((key.to_owned(), 0));
                    visit(child, path, false, last, out);
                    path.pop();
                }
                Item::ArrayOfTables(children) => {
                    for (index, child) in children.iter().enumerate() {
                        path.push((key.to_owned(), index));
                        visit(child, path, true, last, out);
                        path.pop();
                    }
                }
                _ => {}
            }
        }
    }

    let mut out = Vec::new();
    visit(root, &mut Vec::new(), false, &mut 0, &mut out);
    out.sort_by_key(|table| (!table.path.is_empty(), table.position));
    out
}
