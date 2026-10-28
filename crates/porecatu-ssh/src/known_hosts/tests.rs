// SPDX-License-Identifier: GPL-3.0-or-later

//! Casamento, decisão, leitura e gravação do `known_hosts` (ADR-0074).
//!
//! **Fixtures geradas pelo próprio OpenSSH** (`fixtures/known_hosts/`): as
//! chaves são descartáveis feitas por `ssh-keygen`, `known_hosts.hashed` foi
//! produzido por `ssh-keygen -H`, e `expected.*.txt` são as respostas de
//! `ssh-keygen -F` sobre cada arquivo (OpenSSH 9.7p1). Estão gravadas para o
//! teste não depender do `ssh-keygen` no CI.

use std::path::PathBuf;

use russh::keys::PublicKey;

use super::*;

const PLAIN: &str = include_str!("../../fixtures/known_hosts/known_hosts.plain");
const HASHED: &str = include_str!("../../fixtures/known_hosts/known_hosts.hashed");
const EXPECTED_PLAIN: &str = include_str!("../../fixtures/known_hosts/expected.plain.txt");
const EXPECTED_HASHED: &str = include_str!("../../fixtures/known_hosts/expected.hashed.txt");
const KEYS: &str = include_str!("../../fixtures/known_hosts/keys.txt");

fn key(name: &str) -> PublicKey {
    let line = KEYS
        .lines()
        .find(|line| line.split(' ').next() == Some(name))
        .unwrap_or_else(|| panic!("no key {name}"));
    let (_, openssh) = line.split_once(' ').unwrap();
    PublicKey::from_openssh(openssh).unwrap()
}

fn file(path: &str, text: &str) -> FileText {
    FileText {
        path: PathBuf::from(path),
        text: text.to_owned(),
    }
}

fn status(text: &str, host: &str, port: u16, presented: &PublicKey) -> HostKeyStatus {
    check(&[file("known_hosts", text)], host, port, presented).status
}

/// As linhas que casam com o nome, como `ssh-keygen -F` as lista:
/// `linha:marcador`, com `-` para nenhum.
fn matching_lines(text: &str, lookup: &str) -> String {
    let mut skipped = Vec::new();
    let lines = parse_lines(&file("known_hosts", text), &mut skipped);
    let found: Vec<String> = lines
        .iter()
        .filter(|line| host_matches(line.entry.host_patterns(), lookup))
        .map(|line| {
            let flag = match line.entry.marker() {
                Some(Marker::Revoked) => "REVOKED",
                Some(Marker::CertAuthority) => "CA",
                None => "-",
            };
            format!("{}:{flag}", line.number)
        })
        .collect();
    if found.is_empty() {
        "-".to_owned()
    } else {
        found.join(",")
    }
}

// ---------------------------------------------------------------------------
// Casamento contra o OpenSSH
// ---------------------------------------------------------------------------

fn assert_agrees_with_openssh(text: &str, expected: &str) {
    for row in expected.lines() {
        let (query, answer) = row.split_once('|').unwrap();
        // `ssh-keygen -F` recebe o nome como o OpenSSH o procura.
        assert_eq!(
            matching_lines(text, &query.to_lowercase()),
            answer,
            "query {query:?}"
        );
    }
}

#[test]
fn matching_agrees_with_ssh_keygen_on_the_plain_file() {
    assert_agrees_with_openssh(PLAIN, EXPECTED_PLAIN);
}

#[test]
fn matching_agrees_with_ssh_keygen_on_the_hashed_file() {
    // Sanity: the hashed fixture really is hashed, so the HMAC path runs.
    assert!(HASHED.lines().filter(|l| l.starts_with("|1|")).count() >= 5);
    assert_agrees_with_openssh(HASHED, EXPECTED_HASHED);
}

#[test]
fn lookup_name_is_the_host_for_port_22_and_bracketed_otherwise() {
    assert_eq!(lookup_name("Example.ORG", 22), "example.org");
    assert_eq!(lookup_name("Example.ORG", 2222), "[example.org]:2222");
    assert_eq!(lookup_name("::1", 22), "::1");
}

#[test]
fn glob_matches_star_and_question_mark_case_insensitively() {
    assert!(glob_match("*.lan.test", "x.lan.test"));
    assert!(glob_match("*.lan.test", "a.b.lan.test"));
    assert!(!glob_match("*.lan.test", "lan.test"));
    assert!(glob_match("10.0.0.?", "10.0.0.5"));
    assert!(!glob_match("10.0.0.?", "10.0.0.55"));
    assert!(glob_match("*", "anything"));
    assert!(glob_match("a*b*c", "aXXbYYc"));
    assert!(!glob_match("a*b*c", "aXXbYY"));
    assert!(glob_match("Mixed.CASE", "mixed.case"));
    assert!(glob_match("", ""));
    assert!(!glob_match("", "x"));
}

// ---------------------------------------------------------------------------
// Decisão
// ---------------------------------------------------------------------------

#[test]
fn a_registered_key_is_known_in_clear_text_and_hashed() {
    for text in [PLAIN, HASHED] {
        assert_eq!(
            status(text, "example.org", 22, &key("a")),
            HostKeyStatus::Known
        );
        assert_eq!(
            status(text, "example.org", 2222, &key("b")),
            HostKeyStatus::Known
        );
    }
}

#[test]
fn a_host_name_matches_without_regard_to_case() {
    for text in [PLAIN, HASHED] {
        assert_eq!(
            status(text, "MIXED.Case.Example.org", 22, &key("a")),
            HostKeyStatus::Known
        );
    }
}

#[test]
fn the_port_selects_the_entry() {
    // Port 2222 has key B only; key A (port 22's) is a changed key there.
    match status(PLAIN, "example.org", 2222, &key("a")) {
        HostKeyStatus::Changed { expected } => {
            assert_eq!(expected.len(), 1);
            assert_eq!(expected[0].line, 3);
        }
        other => panic!("expected Changed, got {other:?}"),
    }
    // And a port nobody registered is unknown, not changed.
    assert_eq!(
        status(PLAIN, "example.org", 2223, &key("a")),
        HostKeyStatus::Unknown {
            other_types: Vec::new()
        }
    );
}

#[test]
fn wildcard_lines_match_and_a_negated_pattern_excludes_the_whole_line() {
    assert_eq!(
        status(PLAIN, "x.lan.test", 22, &key("c")),
        HostKeyStatus::Known
    );
    assert_eq!(
        status(PLAIN, "10.0.0.5", 22, &key("d")),
        HostKeyStatus::Known
    );
    assert_eq!(
        status(PLAIN, "a.neg.test", 22, &key("d")),
        HostKeyStatus::Known
    );
    // `!secret.neg.test` excludes the line although `*.neg.test` also matches.
    assert_eq!(
        status(PLAIN, "secret.neg.test", 22, &key("d")),
        HostKeyStatus::Unknown {
            other_types: Vec::new()
        }
    );
}

#[test]
fn a_revoked_key_is_revoked_and_beats_everything() {
    match status(PLAIN, "bad.example.org", 22, &key("e")) {
        HostKeyStatus::Revoked { entry } => {
            assert_eq!(entry.line, 10);
            assert_eq!(entry.file, PathBuf::from("known_hosts"));
            assert_eq!(
                entry.fingerprint,
                key("e").fingerprint(HashAlg::Sha256).to_string()
            );
        }
        other => panic!("expected Revoked, got {other:?}"),
    }

    // Revoked wins even if a plain line for the same host trusts the same key.
    let text = format!(
        "bad.example.org {k}\n@revoked bad.example.org {k}\n",
        k = key("a").to_openssh().unwrap()
    );
    assert!(matches!(
        status(&text, "bad.example.org", 22, &key("a")),
        HostKeyStatus::Revoked { .. }
    ));
}

#[test]
fn a_revoked_line_for_another_key_does_not_block_this_one() {
    // `bad.example.org` only has a @revoked line, for key E. Key A is not it.
    assert_eq!(
        status(PLAIN, "bad.example.org", 22, &key("a")),
        HostKeyStatus::Unknown {
            other_types: Vec::new()
        }
    );
}

#[test]
fn a_cert_authority_line_alone_does_not_make_a_host_known() {
    // The CA line trusts key C for `ca.example.org`; it is not a host key.
    assert_eq!(
        status(PLAIN, "ca.example.org", 22, &key("c")),
        HostKeyStatus::Unknown {
            other_types: Vec::new()
        }
    );
}

#[test]
fn a_garbage_line_is_skipped_and_reported_not_fatal() {
    let result = check(&[file("known_hosts", PLAIN)], "example.org", 22, &key("a"));

    assert_eq!(result.status, HostKeyStatus::Known);
    assert_eq!(
        result.skipped_lines,
        vec![(PathBuf::from("known_hosts"), 13)]
    );
    // Comments and blank lines are not "skipped lines".
    let clean = check(&[file("known_hosts", HASHED)], "example.org", 22, &key("a"));
    assert!(clean.skipped_lines.is_empty());
}

#[test]
fn a_garbage_line_does_not_hide_the_entries_around_it() {
    // Lines 12 and 14 are fine and the garbage is in between.
    assert_eq!(
        status(PLAIN, "a.neg.test", 22, &key("d")),
        HostKeyStatus::Known
    );
    assert_eq!(
        status(PLAIN, "mixed.case.example.org", 22, &key("a")),
        HostKeyStatus::Known
    );
}

#[test]
fn another_registered_type_is_unknown_with_the_other_types() {
    // `ec.example.org` has an ECDSA key; the server offers ed25519.
    assert_eq!(
        status(PLAIN, "ec.example.org", 22, &key("a")),
        HostKeyStatus::Unknown {
            other_types: vec!["ecdsa-sha2-nistp256".to_owned()]
        }
    );
    // The registered type itself is known.
    assert_eq!(
        status(PLAIN, "ec.example.org", 22, &key("ec")),
        HostKeyStatus::Known
    );
}

#[test]
fn two_lines_of_the_same_type_with_one_matching_is_known_not_changed() {
    // `rot.example.org` kept the old key (B) after rotating to A: OpenSSH
    // accepts either.
    assert_eq!(
        status(PLAIN, "rot.example.org", 22, &key("a")),
        HostKeyStatus::Known
    );
    assert_eq!(
        status(PLAIN, "rot.example.org", 22, &key("b")),
        HostKeyStatus::Known
    );
    // A third key matches neither: changed, with both entries.
    match status(PLAIN, "rot.example.org", 22, &key("c")) {
        HostKeyStatus::Changed { expected } => {
            let lines: Vec<usize> = expected.iter().map(|entry| entry.line).collect();
            assert_eq!(lines, vec![7, 8]);
        }
        other => panic!("expected Changed, got {other:?}"),
    }
}

#[test]
fn a_host_nobody_registered_is_unknown() {
    for text in [PLAIN, HASHED, ""] {
        assert_eq!(
            status(text, "unknown.example.org", 22, &key("a")),
            HostKeyStatus::Unknown {
                other_types: Vec::new()
            }
        );
    }
}

#[test]
fn lines_from_several_files_are_all_considered_and_say_where_they_are() {
    let user = file("user/known_hosts", "other.example.org ssh-ed25519 AAAA\n");
    let global = file(
        "global/ssh_known_hosts",
        &format!("example.org {}\n", key("b").to_openssh().unwrap()),
    );

    match check(&[user, global], "example.org", 22, &key("a")).status {
        HostKeyStatus::Changed { expected } => {
            assert_eq!(expected[0].file, PathBuf::from("global/ssh_known_hosts"));
            assert_eq!(expected[0].line, 1);
        }
        other => panic!("expected Changed, got {other:?}"),
    }
}

#[test]
fn the_registered_algorithms_come_in_order_without_repeats() {
    let text = format!(
        "h {ec}\nh {ed}\nh {ed2}\n@revoked h {ed}\n",
        ec = {
            let (_, ec) = KEYS
                .lines()
                .find(|l| l.starts_with("ec "))
                .unwrap()
                .split_once(' ')
                .unwrap();
            ec.to_owned()
        },
        ed = key("a").to_openssh().unwrap(),
        ed2 = key("b").to_openssh().unwrap(),
    );

    let algorithms = registered_algorithms(&[file("known_hosts", &text)], "h", 22);

    assert_eq!(algorithms.len(), 2);
    assert_eq!(algorithms[0].as_str(), "ecdsa-sha2-nistp256");
    assert_eq!(algorithms[1].as_str(), "ssh-ed25519");
}

// ---------------------------------------------------------------------------
// Impressão digital
// ---------------------------------------------------------------------------

#[test]
fn the_fingerprint_is_sha256_in_unpadded_base64() {
    let fingerprint = fingerprint(&key("a"));

    let body = fingerprint.strip_prefix("SHA256:").expect("SHA256: prefix");
    assert_eq!(body.len(), 43, "{fingerprint}");
    assert!(!body.contains('='));
}

#[test]
fn the_algorithm_name_follows_the_negotiated_rsa_hash() {
    assert_eq!(algorithm_name(&key("a"), None), "ssh-ed25519");
    assert_eq!(algorithm_name(&key("ec"), None), "ecdsa-sha2-nistp256");
}

// ---------------------------------------------------------------------------
// Leitura
// ---------------------------------------------------------------------------

/// Um diretório temporário só deste teste.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("porecatu-ssh-kh-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn files_of(user: PathBuf, read_only: Vec<PathBuf>) -> KnownHostsFiles {
    KnownHostsFiles {
        user: Some(user),
        read_only,
    }
}

#[test]
fn a_missing_file_is_empty() {
    let dir = TempDir::new("missing");

    let texts = read_files(&files_of(
        dir.path("known_hosts"),
        vec![dir.path("known_hosts2")],
    ));

    assert_eq!(texts.unwrap(), Vec::new());
}

#[test]
fn files_are_read_in_order_user_first() {
    let dir = TempDir::new("order");
    fs::write(dir.path("known_hosts"), "one\n").unwrap();
    fs::write(dir.path("known_hosts2"), "two\n").unwrap();

    let texts = read_files(&files_of(
        dir.path("known_hosts"),
        vec![dir.path("known_hosts2")],
    ))
    .unwrap();

    assert_eq!(texts[0].text, "one\n");
    assert_eq!(texts[1].text, "two\n");
}

#[test]
fn a_utf16_file_is_unreadable_and_names_the_file() {
    let dir = TempDir::new("utf16");
    let mut utf16 = vec![0xFF, 0xFE];
    for unit in "example.org ssh-ed25519 AAAA\r\n".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(dir.path("known_hosts"), &utf16).unwrap();

    let err = read_files(&files_of(dir.path("known_hosts"), Vec::new())).unwrap_err();

    assert_eq!(err.file, dir.path("known_hosts"));
    assert_eq!(err.cause, ReadCause::NotText);
}

#[test]
fn utf16_without_a_bom_is_unreadable_too() {
    // ASCII text in UTF-16 is *valid UTF-8* full of NULs: it must not slip
    // through as an empty-looking file.
    let dir = TempDir::new("utf16-nobom");
    let mut utf16 = Vec::new();
    for unit in "example.org ssh-ed25519 AAAA\n".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(dir.path("known_hosts"), &utf16).unwrap();

    let err = read_files(&files_of(dir.path("known_hosts"), Vec::new())).unwrap_err();

    assert_eq!(err.cause, ReadCause::NotText);
}

#[test]
fn an_unreadable_read_only_file_stops_the_reading_too() {
    let dir = TempDir::new("global-bad");
    fs::write(dir.path("known_hosts"), "").unwrap();
    fs::write(dir.path("ssh_known_hosts"), [0xFF, 0xFE, 0x00]).unwrap();

    let err = read_files(&files_of(
        dir.path("known_hosts"),
        vec![dir.path("ssh_known_hosts")],
    ))
    .unwrap_err();

    assert_eq!(err.file, dir.path("ssh_known_hosts"));
}

#[test]
fn a_path_that_exists_but_cannot_be_read_as_a_file_is_unreadable() {
    // A directory where the file should be: it exists, and reading it fails.
    let dir = TempDir::new("is-dir");
    fs::create_dir_all(dir.path("known_hosts")).unwrap();

    let err = read_files(&files_of(dir.path("known_hosts"), Vec::new())).unwrap_err();

    assert_eq!(err.file, dir.path("known_hosts"));
    assert_ne!(err.cause, ReadCause::NotText);
}

// ---------------------------------------------------------------------------
// Gravação
// ---------------------------------------------------------------------------

fn written(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn appending_to_a_file_without_a_final_newline_adds_one_first() {
    let dir = TempDir::new("no-newline");
    let path = dir.path("known_hosts");
    let existing = format!("other.example.org {}", key("b").to_openssh().unwrap());
    fs::write(&path, &existing).unwrap();

    append_entry(&path, "example.org", 22, &key("a")).unwrap();

    let text = written(&path);
    assert!(text.starts_with(&existing), "existing bytes are untouched");
    let tail = &text[existing.len()..];
    assert!(tail.starts_with('\n'), "{tail:?}");
    assert!(text.ends_with('\n'));
    assert_eq!(text.lines().count(), 2);
    assert_eq!(
        status(&text, "example.org", 22, &key("a")),
        HostKeyStatus::Known
    );
}

#[test]
fn a_clear_text_file_gets_a_clear_text_line() {
    let dir = TempDir::new("clear");
    let path = dir.path("known_hosts");
    fs::write(
        &path,
        format!("other.example.org {}\n", key("b").to_openssh().unwrap()),
    )
    .unwrap();

    append_entry(&path, "Example.ORG", 22, &key("a")).unwrap();

    let last = written(&path).lines().last().unwrap().to_owned();
    assert_eq!(
        last,
        format!("example.org {}", key("a").to_openssh().unwrap())
    );
}

#[test]
fn a_hashed_file_gets_a_hashed_line_that_matches_back() {
    let dir = TempDir::new("hashed");
    let path = dir.path("known_hosts");
    fs::write(&path, HASHED).unwrap();

    append_entry(&path, "new.example.org", 2222, &key("c")).unwrap();

    let text = written(&path);
    let last = text.lines().last().unwrap();
    assert!(last.starts_with("|1|"), "{last}");
    assert!(!last.contains("new.example.org"), "the name must not leak");
    // The old content is a prefix, byte for byte.
    assert!(text.starts_with(HASHED));
    // And the hash we wrote is one we (and so OpenSSH's format) read back.
    assert_eq!(
        status(&text, "new.example.org", 2222, &key("c")),
        HostKeyStatus::Known
    );
    assert_eq!(
        status(&text, "new.example.org", 2223, &key("c")),
        HostKeyStatus::Unknown {
            other_types: Vec::new()
        }
    );
}

#[test]
fn a_non_default_port_is_written_in_brackets() {
    let dir = TempDir::new("port");
    let path = dir.path("known_hosts");

    append_entry(&path, "example.org", 2222, &key("a")).unwrap();

    assert!(written(&path).starts_with("[example.org]:2222 ssh-ed25519 "));
}

#[test]
fn a_missing_directory_is_created() {
    let dir = TempDir::new("mkdir");
    let path = dir.path("dot_ssh").join("known_hosts");

    append_entry(&path, "example.org", 22, &key("a")).unwrap();

    assert_eq!(written(&path).lines().count(), 1);
}

#[cfg(unix)]
#[test]
fn the_created_directory_and_file_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("perms");
    let path = dir.path("dot_ssh").join("known_hosts");

    append_entry(&path, "example.org", 22, &key("a")).unwrap();

    let dir_mode = fs::metadata(path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700);
    assert_eq!(file_mode, 0o600);
}

#[test]
fn a_host_that_could_inject_a_line_is_refused_and_nothing_is_written() {
    let dir = TempDir::new("inject");
    let path = dir.path("known_hosts");

    for host in [
        "evil.example.org\n@revoked other",
        "evil.example.org\r",
        "has space.example.org",
        "tab\there",
        "a,b.example.org",
        "*.example.org",
        "!example.org",
        "#comment",
        "[example.org]",
        "|1|salt|hash",
        "user@example.org",
        "",
    ] {
        let result = append_entry(&path, host, 22, &key("a"));
        assert!(matches!(result, Err(WriteError::Unsafe)), "{host:?}");
    }
    assert!(!path.exists(), "nothing was written");
}

#[test]
fn a_key_with_a_comment_is_refused() {
    // The key the server presents has no comment; one would open a new field.
    let dir = TempDir::new("comment");
    let path = dir.path("known_hosts");
    let with_comment =
        PublicKey::from_openssh(&format!("{} some comment", key("a").to_openssh().unwrap()))
            .unwrap();

    let result = append_entry(&path, "example.org", 22, &with_comment);

    assert!(matches!(result, Err(WriteError::Unsafe)));
    assert!(!path.exists());
}

#[test]
fn appending_never_rewrites_what_was_there() {
    let dir = TempDir::new("append-only");
    let path = dir.path("known_hosts");
    fs::write(&path, PLAIN).unwrap();

    append_entry(&path, "brand.new.example.org", 22, &key("e")).unwrap();
    append_entry(&path, "another.example.org", 22, &key("d")).unwrap();

    let text = written(&path);
    assert!(text.starts_with(PLAIN));
    assert_eq!(text.lines().count(), PLAIN.lines().count() + 2);
}
