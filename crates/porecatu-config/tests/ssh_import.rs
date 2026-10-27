// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::path::Path;

use porecatu_config::{
    SshConfigImport, SshConfigImportErrorKind, SshImportInvalid, SshImportStatus,
    SshImportUnsupported, SshProfile, read_ssh_config,
};

const LOCAL: &str = "leo";

fn write(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    path
}

fn read(text: &str, existing: &[SshProfile]) -> SshConfigImport {
    let home = tempfile::tempdir().unwrap();
    let path = write(home.path(), ".ssh/config", text);
    read_ssh_config(&path, home.path(), LOCAL, existing).unwrap()
}

fn status<'a>(import: &'a SshConfigImport, alias: &str) -> &'a SshImportStatus {
    &import
        .candidates
        .iter()
        .find(|c| c.alias == alias)
        .unwrap_or_else(|| panic!("no candidate {alias}"))
        .status
}

#[test]
fn a_concrete_host_comes_with_every_field_resolved() {
    let home = tempfile::tempdir().unwrap();
    let path = write(
        home.path(),
        ".ssh/config",
        "Host web\n  HostName 10.0.0.5\n  User deploy\n  Port 2222\n  IdentityFile ~/.ssh/id_web\n",
    );
    let import = read_ssh_config(&path, home.path(), LOCAL, &[]).unwrap();
    assert!(import.file_found);
    assert_eq!(import.candidates.len(), 1);
    let c = &import.candidates[0];
    assert_eq!(
        (c.alias.as_str(), c.host.as_str(), c.user.as_str(), c.port),
        ("web", "10.0.0.5", "deploy", 2222)
    );
    assert!(c.identity_file.ends_with("id_web"), "{}", c.identity_file);
    assert!(Path::new(&c.identity_file).starts_with(home.path()));
    assert_eq!(c.status, SshImportStatus::Importable);
}

#[test]
fn a_block_without_hostname_user_or_port_gets_the_defaults() {
    let import = read("Host bare\n", &[]);
    let c = &import.candidates[0];
    assert_eq!(
        (
            c.host.as_str(),
            c.user.as_str(),
            c.port,
            c.identity_file.as_str()
        ),
        ("bare", LOCAL, 22, "")
    );
}

#[test]
fn wildcard_hosts_stay_out_of_the_list_but_their_defaults_apply() {
    let import = read(
        "Host web\n  HostName w.example\nHost *.interno\n  User x\nHost *\n  User everyone\nHost a? !b\n",
        &[],
    );
    let aliases: Vec<_> = import.candidates.iter().map(|c| c.alias.as_str()).collect();
    assert_eq!(aliases, ["web"]);
    assert_eq!(import.candidates[0].user, "everyone");
}

#[test]
fn a_host_line_with_several_names_gives_one_candidate_each() {
    let import = read("Host one two *.x\n  HostName shared.example\n", &[]);
    let aliases: Vec<_> = import.candidates.iter().map(|c| c.alias.as_str()).collect();
    assert_eq!(aliases, ["one", "two"]);
    assert!(import.candidates.iter().all(|c| c.host == "shared.example"));
}

#[test]
fn an_include_is_resolved_against_the_ssh_folder() {
    let home = tempfile::tempdir().unwrap();
    write(
        home.path(),
        ".ssh/config.d/extra",
        "Host included\n  HostName inc.example\n  User inc\n",
    );
    let path = write(
        home.path(),
        ".ssh/config",
        // At the top, as people write it: an `Include` under a `Host` is
        // scoped to that block, as in OpenSSH.
        "Include config.d/*\nHost first\n  HostName f.example\n",
    );
    let import = read_ssh_config(&path, home.path(), LOCAL, &[]).unwrap();
    let aliases: Vec<_> = import.candidates.iter().map(|c| c.alias.as_str()).collect();
    assert_eq!(aliases, ["included", "first"]);
    assert_eq!(import.candidates[0].host, "inc.example");
    assert_eq!(import.candidates[0].user, "inc");
}

#[test]
fn every_unsupported_directive_disables_the_block_with_its_reason() {
    let cases = [
        ("ProxyJump bastion", SshImportUnsupported::ProxyJump),
        (
            "ProxyCommand ssh -W %h:%p x",
            SshImportUnsupported::ProxyCommand,
        ),
        (
            "LocalForward 8080 localhost:80",
            SshImportUnsupported::LocalForward,
        ),
        (
            "RemoteForward 9000 localhost:9000",
            SshImportUnsupported::RemoteForward,
        ),
        ("DynamicForward 1080", SshImportUnsupported::DynamicForward),
        (
            "CertificateFile ~/.ssh/id-cert.pub",
            SshImportUnsupported::CertificateFile,
        ),
    ];
    for (line, reason) in cases {
        let import = read(&format!("Host h\n  HostName h.example\n  {line}\n"), &[]);
        assert_eq!(
            status(&import, "h"),
            &SshImportStatus::Unsupported(reason),
            "{line}"
        );
    }
}

#[test]
fn an_unsupported_default_block_disables_every_host_it_reaches() {
    let import = read(
        "Host a\n  HostName a.example\nHost *\n  ProxyJump hop\n",
        &[],
    );
    assert_eq!(
        status(&import, "a"),
        &SshImportStatus::Unsupported(SshImportUnsupported::ProxyJump)
    );
}

#[test]
fn a_destination_that_is_already_a_profile_is_marked_imported() {
    let existing = [SshProfile {
        host: "WEB.example".to_owned(),
        port: 2222,
        user: "deploy".to_owned(),
        auth: Vec::new(),
        identity_file: String::new(),
    }];
    let import = read(
        "Host same\n  HostName web.example\n  User deploy\n  Port 2222\nHost otherport\n  HostName web.example\n  User deploy\n  Port 22\nHost otheruser\n  HostName web.example\n  User root\n  Port 2222\n",
        &existing,
    );
    assert_eq!(status(&import, "same"), &SshImportStatus::AlreadyImported);
    assert_eq!(status(&import, "otherport"), &SshImportStatus::Importable);
    assert_eq!(status(&import, "otheruser"), &SshImportStatus::Importable);
}

#[test]
fn a_match_block_refuses_the_whole_file_with_its_line() {
    let home = tempfile::tempdir().unwrap();
    let path = write(
        home.path(),
        ".ssh/config",
        "Host a\n  HostName a.example\nMatch host b\n  HostName b.example\n",
    );
    let err = read_ssh_config(&path, home.path(), LOCAL, &[]).unwrap_err();
    assert_eq!(err.kind, SshConfigImportErrorKind::Match);
    assert_eq!(err.line, Some(3));
}

#[test]
fn an_unknown_directive_refuses_the_file_and_names_it() {
    let home = tempfile::tempdir().unwrap();
    let path = write(home.path(), ".ssh/config", "Host a\n  Frobnicate yes\n");
    let err = read_ssh_config(&path, home.path(), LOCAL, &[]).unwrap_err();
    assert_eq!(
        err.kind,
        SshConfigImportErrorKind::UnknownDirective {
            name: "Frobnicate".to_owned()
        }
    );
    assert_eq!(err.line, Some(2));
}

#[test]
fn a_bad_value_refuses_the_file() {
    let home = tempfile::tempdir().unwrap();
    let path = write(home.path(), ".ssh/config", "Host a\n  Port banana\n");
    let err = read_ssh_config(&path, home.path(), LOCAL, &[]).unwrap_err();
    assert_eq!(err.kind, SshConfigImportErrorKind::InvalidValue);
}

#[test]
fn an_include_loop_refuses_the_file() {
    let home = tempfile::tempdir().unwrap();
    write(home.path(), ".ssh/loop", "Include config\n");
    let path = write(home.path(), ".ssh/config", "Include loop\n");
    let err = read_ssh_config(&path, home.path(), LOCAL, &[]).unwrap_err();
    assert_eq!(err.kind, SshConfigImportErrorKind::Include);
}

#[test]
fn a_host_or_user_the_profile_would_refuse_is_invalid() {
    let import = read(
        "Host badhost\n  HostName bad@host\nHost baduser\n  HostName ok.example\n  User a:b\nHost nouser\n  HostName ok.example\n",
        &[],
    );
    assert_eq!(
        status(&import, "badhost"),
        &SshImportStatus::Invalid(SshImportInvalid::Host)
    );
    assert_eq!(
        status(&import, "baduser"),
        &SshImportStatus::Invalid(SshImportInvalid::User)
    );
    assert_eq!(status(&import, "nouser"), &SshImportStatus::Importable);

    let home = tempfile::tempdir().unwrap();
    let path = write(home.path(), ".ssh/config", "Host nouser\n");
    let import = read_ssh_config(&path, home.path(), "", &[]).unwrap();
    assert_eq!(
        status(&import, "nouser"),
        &SshImportStatus::Invalid(SshImportInvalid::User)
    );
}

#[test]
fn a_missing_file_is_an_empty_list_that_says_so() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(".ssh").join("config");
    let import = read_ssh_config(&path, home.path(), LOCAL, &[]).unwrap();
    assert!(!import.file_found);
    assert!(import.candidates.is_empty());
}
