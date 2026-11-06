// SPDX-License-Identifier: GPL-3.0-or-later

//! Duas coisas diferentes moram aqui: a seção `[project_file]` da config do
//! app (RF-12.5, RF-12.7), e o parser do arquivo `.porecatu` do PROJETO, que
//! não é TOML (ADR-0051 §7, docs/arquitetura.md §6.1) -- é o formato de
//! seções cruas que o ADR-0051 §2 decide. `[icon]` é o único nome de seção
//! reservado (ADR-0088 §1): o corpo é um caminho de imagem, não um script.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const PROJECT_FILE_NAME: &str = ".porecatu";

/// Section names that are not shells (ADR-0088 §1). `command_for` never
/// returns one of them, and `[default]` never stands in for them. A new
/// reserved name needs a new ADR.
pub const RESERVED_SECTIONS: &[&str] = &["icon"];

/// Name of the fallback icon file looked up in a pinned tab's directory
/// (ADR-0088 §4). The UI owns the fallback chain; this is only the name.
pub const ICON_PNG_FILE_NAME: &str = "icon.png";

/// Who authorizes reading a directory's `.porecatu` (ADR-0086 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// ADR-0051 §4: the directory has to be under `[project_file]
    /// trusted_paths`.
    Allowlist,
    /// ADR-0086 §6: the tab is pinned, and pinning is already the
    /// authorization of that exact directory.
    Pinned,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ProjectFile {
    /// RF-12.7. Desligado, nada é lido do disco.
    pub enabled: bool,
    /// RF-12.5. Vazia por default: numa instalação nova nada roda.
    pub trusted_paths: Vec<String>,
}

impl Default for ProjectFile {
    fn default() -> Self {
        Self {
            enabled: true,
            trusted_paths: Vec::new(),
        }
    }
}

/// Uma seção do `.porecatu`, na ordem em que apareceu no arquivo. Nome já
/// em minúsculas; corpo literal, sem as linhas em branco finais.
struct Section {
    name: String,
    body: String,
}

/// Resultado de `parse` -- todas as seções encontradas, na ordem do
/// arquivo. Qualquer texto é um script válido, possivelmente sem a seção
/// que interessa (ADR-0051 §2).
pub struct ProjectScript {
    sections: Vec<Section>,
}

/// Cabeçalho de seção: linha, já sem espaço nas pontas, que começa com
/// `[`, termina com `]` e cujo miolo é não-vazio e só `A-Za-z0-9_.+-`.
fn section_header(trimmed: &str) -> Option<&str> {
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.is_empty()
        || !inner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '+' | '-'))
    {
        return None;
    }
    Some(inner)
}

/// Corta as linhas em branco do fim de uma seção (preservando as do meio)
/// e registra a seção -- a primeira com um dado nome vence (regra que o
/// ADR-0051 não decide; ver docs/reference/arquivo-de-projeto.md).
fn push_section(sections: &mut Vec<Section>, name: String, mut lines: Vec<&str>) {
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    if sections.iter().any(|section| section.name == name) {
        return;
    }
    sections.push(Section {
        name,
        body: lines.join("\n"),
    });
}

/// Parseia o texto de um `.porecatu`. Sem estado de erro: qualquer texto é
/// um arquivo válido. `\r\n` é normalizado para `\n` antes de tudo, para o
/// arquivo funcionar vindo de qualquer plataforma.
pub fn parse(text: &str) -> ProjectScript {
    let normalized = text.replace("\r\n", "\n");
    let mut sections = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;

    for line in normalized.split('\n') {
        if let Some(name) = section_header(line.trim()) {
            if let Some((name, lines)) = current.take() {
                push_section(&mut sections, name, lines);
            }
            current = Some((name.to_lowercase(), Vec::new()));
        } else if let Some((_, lines)) = current.as_mut() {
            lines.push(line);
        }
        // Linha antes da primeira seção: preâmbulo, descartada.
    }
    if let Some((name, lines)) = current.take() {
        push_section(&mut sections, name, lines);
    }

    ProjectScript { sections }
}

impl ProjectScript {
    /// Seção do shell dado, por casamento exato e insensível a maiúsculas;
    /// sem ela, `[default]`; sem nenhuma das duas, `None` -- que **não** é
    /// erro (ADR-0051 §3). Seções reservadas (`RESERVED_SECTIONS`, ADR-0088
    /// §1) nunca são devolvidas: um shell chamado `icon` cai em `[default]`.
    pub fn command_for(&self, shell_name: &str) -> Option<&str> {
        let shell_name = shell_name.to_lowercase();
        let reserved = |name: &str| RESERVED_SECTIONS.contains(&name);
        self.sections
            .iter()
            .find(|section| section.name == shell_name && !reserved(&section.name))
            .or_else(|| {
                self.sections
                    .iter()
                    .find(|section| section.name == "default")
            })
            .map(|section| section.body.as_str())
    }

    /// Path declared in `[icon]` (ADR-0088 §1): the first non-blank line of
    /// the body, trimmed. An empty body is the same as no section.
    pub fn icon_path(&self) -> Option<&str> {
        self.sections
            .iter()
            .find(|section| section.name == "icon")?
            .body
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

/// Expande um `~` inicial via `dirs::home_dir`; qualquer outro caminho
/// volta inalterado.
pub fn expand_tilde(entry: &str) -> PathBuf {
    match entry.strip_prefix('~') {
        Some(rest) => {
            let rest = rest
                .strip_prefix('/')
                .or_else(|| rest.strip_prefix('\\'))
                .unwrap_or(rest);
            match dirs::home_dir() {
                Some(home) => home.join(rest),
                None => PathBuf::from(entry),
            }
        }
        None => PathBuf::from(entry),
    }
}

/// Confiança por allowlist (ADR-0051 §4). `dir` está confiável se estiver
/// sob algum caminho de `trusted_paths` -- lista vazia recusa tudo.
pub fn is_trusted(dir: &Path, trusted_paths: &[String]) -> bool {
    if trusted_paths.is_empty() {
        return false;
    }

    let Ok(dir) = fs::canonicalize(dir) else {
        return false;
    };

    trusted_paths.iter().any(|entry| {
        // Entrada da lista que não canonicaliza é ignorada em silêncio:
        // dotfiles viajam entre máquinas, e um caminho que só existe numa
        // delas não pode derrubar a config na outra.
        let Ok(root) = fs::canonicalize(expand_tilde(entry)) else {
            return false;
        };

        // Comparação por COMPONENTE de caminho, nunca por prefixo de
        // string -- "C:/Projetos-do-vizinho" não pode casar com
        // "C:/Projetos". No Windows, `canonicalize` devolve o caminho
        // verbatim (`\\?\C:\...`) com a caixa como está no disco nos dois
        // lados, o que já resolve prefixo e diferença de maiúsculas de
        // graça -- não "conserte" isto adicionando um `to_lowercase`.
        let mut dir_components = dir.components();
        root.components().all(|c| dir_components.next() == Some(c))
    })
}

/// O que a resolução do `.porecatu` de um diretório produziu. A etapa 3
/// consome isto para decidir o que escrever no PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectFileOutcome {
    Disabled,
    NotFound,
    Untrusted { path: PathBuf },
    Unreadable { path: PathBuf, reason: String },
    NoSection { path: PathBuf },
    Ready { path: PathBuf, command: String },
}

/// Resolução completa de um `.porecatu` de diretório (ADR-0051 §2 a §4).
/// A ordem dos passos abaixo é a que dá as garantias do PRD-012 -- não
/// reordene. `trust` diz quem autoriza a leitura (ADR-0086 §6): com
/// `Trust::Pinned` o passo da allowlist é pulado, e `enabled = false`
/// continua sendo o primeiro passo nos dois casos.
pub fn resolve(
    dir: &Path,
    shell_name: &str,
    config: &ProjectFile,
    trust: Trust,
) -> ProjectFileOutcome {
    if !config.enabled {
        return ProjectFileOutcome::Disabled;
    }

    let path = dir.join(PROJECT_FILE_NAME);
    if !path.is_file() {
        return ProjectFileOutcome::NotFound;
    }

    if trust == Trust::Allowlist && !is_trusted(dir, &config.trusted_paths) {
        // Não lê o conteúdo do arquivo neste caminho: a nota da etapa 3 só
        // precisa do caminho, e não ler é uma propriedade que vale a pena
        // preservar.
        return ProjectFileOutcome::Untrusted { path };
    }

    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            return ProjectFileOutcome::Unreadable {
                path,
                reason: err.to_string(),
            };
        }
    };

    match parse(&text).command_for(shell_name) {
        Some(command) => ProjectFileOutcome::Ready {
            path,
            command: command.to_owned(),
        },
        None => ProjectFileOutcome::NoSection { path },
    }
}

/// The disk access of `resolve_icon`, behind a trait so a test can prove
/// which paths are (not) touched.
trait IconFs {
    /// Content of the `.porecatu`; `None` if it is not a file or unreadable.
    fn read_project_file(&self, path: &Path) -> Option<String>;
    /// Whether `path` is a regular file (follows symlinks).
    fn is_regular_file(&self, path: &Path) -> bool;
}

struct RealIconFs;

impl IconFs for RealIconFs {
    fn read_project_file(&self, path: &Path) -> Option<String> {
        if !path.is_file() {
            return None;
        }
        fs::read_to_string(path).ok()
    }

    fn is_regular_file(&self, path: &Path) -> bool {
        fs::metadata(path).is_ok_and(|meta| meta.is_file())
    }
}

fn is_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Two separators at the start, in any combination: UNC, `\\?\`, `\\.\`,
/// `//server/x`, `\/server`, `/\server` (ADR-0088 §2 step 3). Textual on
/// purpose: on Windows merely touching a UNC path opens an SMB connection
/// and offers the session credentials.
fn starts_with_two_separators(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(is_separator) && chars.next().is_some_and(is_separator)
}

/// `\\?\C:\...` / `//?/C:/...`: the verbatim form of a local drive path.
fn is_verbatim_local_drive(text: &str) -> bool {
    let mut chars = text.chars();
    let (Some(a), Some(b), Some(c), Some(d)) =
        (chars.next(), chars.next(), chars.next(), chars.next())
    else {
        return false;
    };
    let (Some(drive), Some(colon), Some(sep)) = (chars.next(), chars.next(), chars.next()) else {
        return false;
    };
    is_separator(a)
        && is_separator(b)
        && c == '?'
        && is_separator(d)
        && drive.is_ascii_alphabetic()
        && colon == ':'
        && is_separator(sep)
}

/// Whether `text` names a network or device location that must not be
/// touched: it starts with two separators (UNC, `\\.\`, `//server/x`, ...),
/// except the verbatim form of a local drive (`\\?\C:\...`), which is how
/// Windows spells a canonical local path. Textual, so it runs before any
/// filesystem call (ADR-0088 §2 step 3). The UI uses it on the pinned tab's
/// directory before probing for `icon.png`, and `resolve_icon` on the final
/// icon path.
pub fn is_unc_or_device_path(text: &str) -> bool {
    starts_with_two_separators(text) && !is_verbatim_local_drive(text)
}

fn resolve_icon_with(fs: &impl IconFs, dir: &Path, config: &ProjectFile) -> Option<PathBuf> {
    if !config.enabled {
        return None;
    }

    let text = fs.read_project_file(&dir.join(PROJECT_FILE_NAME))?;
    let script = parse(&text);
    let declared = script.icon_path()?;

    // Textual refusal, before any filesystem call on the icon path.
    if starts_with_two_separators(declared) {
        return None;
    }

    let expanded = expand_tilde(declared);
    let target = if expanded.is_relative() {
        dir.join(expanded)
    } else {
        expanded
    };

    // The final path is checked too (a `~` or `dir` could produce one). The
    // verbatim form of a local drive is let through only here: it can come
    // from `dir`, never from the declared text, which was refused above.
    let final_text = target.to_string_lossy();
    if is_unc_or_device_path(&final_text) {
        return None;
    }

    fs.is_regular_file(&target).then_some(target)
}

/// Path declared in `[icon]` of `dir`'s `.porecatu`, validated, or `None`
/// (ADR-0088 §2 and §3). Only the declared path: falling back to `icon.png`
/// and to the app icon is up to the UI, since a declared image that does not
/// decode falls back too. No `trusted_paths`: the reader is a pinned tab,
/// and pinning is the authorization (ADR-0088 §3). `enabled = false` returns
/// `None` before touching the disk.
pub fn resolve_icon(dir: &Path, config: &ProjectFile) -> Option<PathBuf> {
    resolve_icon_with(&RealIconFs, dir, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parser ---

    #[test]
    fn preamble_before_first_section_is_discarded() {
        let script = parse("isto é comentário do arquivo\n[pwsh]\nnpm run dev\n");
        assert_eq!(script.command_for("pwsh"), Some("npm run dev"));
    }

    #[test]
    fn single_line_section() {
        let script = parse("[pwsh]\nnpm run dev\n");
        assert_eq!(script.command_for("pwsh"), Some("npm run dev"));
    }

    #[test]
    fn multi_line_section() {
        let script = parse("[bash]\nnvm use\nnpm run dev\n");
        assert_eq!(script.command_for("bash"), Some("nvm use\nnpm run dev"));
    }

    #[test]
    fn blank_line_in_the_middle_is_preserved_and_at_the_end_is_trimmed() {
        let script = parse("[bash]\nfirst\n\nsecond\n\n\n");
        assert_eq!(script.command_for("bash"), Some("first\n\nsecond"));
    }

    #[test]
    fn hash_inside_a_section_is_literal_script_body() {
        let script = parse("[cmd]\nrem o rem e o comentario do cmd; o # nao e\n#not-a-comment\n");
        assert_eq!(
            script.command_for("cmd"),
            Some("rem o rem e o comentario do cmd; o # nao e\n#not-a-comment")
        );
    }

    #[test]
    fn bracket_line_inside_a_body_becomes_a_new_section() {
        // Limitação documentada (ADR-0051 §2, referência do formato): sem
        // escape, uma linha de script que seja literalmente "[algo]" não é
        // expressável. O teste FIXA o comportamento, não o conserta.
        let script = parse("[bash]\necho antes\n[algo]\necho depois\n");
        assert_eq!(script.command_for("bash"), Some("echo antes"));
        assert_eq!(script.command_for("algo"), Some("echo depois"));
    }

    #[test]
    fn uppercase_section_header_matches_lowercase_shell_name() {
        let script = parse("[PWSH]\nnpm run dev\n");
        assert_eq!(script.command_for("pwsh"), Some("npm run dev"));
    }

    #[test]
    fn falls_back_to_default_as_last_resort() {
        let script = parse("[bash]\necho bash\n[default]\necho default\n");
        assert_eq!(script.command_for("cmd"), Some("echo default"));
    }

    #[test]
    fn no_matching_section_and_no_default_is_none() {
        let script = parse("[bash]\necho bash\n");
        assert_eq!(script.command_for("cmd"), None);
    }

    #[test]
    fn crlf_is_normalized() {
        let script = parse("[pwsh]\r\nnpm run dev\r\n");
        assert_eq!(script.command_for("pwsh"), Some("npm run dev"));
    }

    #[test]
    fn duplicate_section_first_one_wins() {
        let script = parse("[pwsh]\nfirst\n[pwsh]\nsecond\n");
        assert_eq!(script.command_for("pwsh"), Some("first"));
    }

    // --- confiança ---

    #[test]
    fn empty_list_denies_everything() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!is_trusted(dir.path(), &[]));
    }

    #[test]
    fn exact_path_is_accepted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let trusted = vec![dir.path().to_string_lossy().into_owned()];
        assert!(is_trusted(dir.path(), &trusted));
    }

    #[test]
    fn subdirectory_is_accepted() {
        let root = tempfile::tempdir().expect("tempdir");
        let sub = root.path().join("sub");
        std::fs::create_dir(&sub).expect("create subdir");
        let trusted = vec![root.path().to_string_lossy().into_owned()];
        assert!(is_trusted(&sub, &trusted));
    }

    #[test]
    fn dotdot_escaping_the_tree_is_denied() {
        let root = tempfile::tempdir().expect("tempdir");
        let child = root.path().join("child");
        let sibling = root.path().join("sibling");
        std::fs::create_dir(&child).expect("create child");
        std::fs::create_dir(&sibling).expect("create sibling");

        let escaping = child.join("..").join("sibling");
        let trusted = vec![child.to_string_lossy().into_owned()];

        assert!(!is_trusted(&escaping, &trusted));
    }

    #[test]
    fn prefix_looking_sibling_directory_is_not_matched_by_string_prefix() {
        let root = tempfile::tempdir().expect("tempdir");
        let trusted_dir = root.path().join("projetos");
        let sibling_dir = root.path().join("projetos-do-vizinho");
        std::fs::create_dir(&trusted_dir).expect("create trusted dir");
        std::fs::create_dir(&sibling_dir).expect("create sibling dir");

        let trusted = vec![trusted_dir.to_string_lossy().into_owned()];

        assert!(!is_trusted(&sibling_dir, &trusted));
        assert!(is_trusted(&trusted_dir, &trusted));
    }

    #[test]
    fn nonexistent_entry_in_the_list_is_skipped_silently() {
        let root = tempfile::tempdir().expect("tempdir");
        let missing = root.path().join("does-not-exist");
        let trusted = vec![
            missing.to_string_lossy().into_owned(),
            root.path().to_string_lossy().into_owned(),
        ];
        assert!(is_trusted(root.path(), &trusted));
    }

    #[test]
    fn tilde_is_expanded() {
        let home = dirs::home_dir().expect("home dir must exist in test env");
        let nested = tempfile::tempdir_in(&home).expect("cannot create temp dir under home");
        let trusted = vec!["~".to_owned()];
        assert!(is_trusted(nested.path(), &trusted));
    }

    // --- resolução ---

    #[test]
    fn disabled_returns_disabled_without_touching_disk() {
        let config = ProjectFile {
            enabled: false,
            trusted_paths: Vec::new(),
        };
        let missing_dir = std::env::temp_dir().join("porecatu-config-test-does-not-exist");
        let outcome = resolve(&missing_dir, "pwsh", &config, Trust::Allowlist);
        assert_eq!(outcome, ProjectFileOutcome::Disabled);
    }

    #[test]
    fn untrusted_directory_wins_over_unreadable_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(PROJECT_FILE_NAME), [0xff, 0xfe, 0x00])
            .expect("write invalid utf-8 file");
        let config = ProjectFile {
            enabled: true,
            trusted_paths: Vec::new(),
        };
        let outcome = resolve(dir.path(), "pwsh", &config, Trust::Allowlist);
        assert!(matches!(outcome, ProjectFileOutcome::Untrusted { .. }));
    }

    #[test]
    fn no_section_when_shell_is_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(PROJECT_FILE_NAME), "[bash]\necho hi\n")
            .expect("write project file");
        let config = ProjectFile {
            enabled: true,
            trusted_paths: vec![dir.path().to_string_lossy().into_owned()],
        };
        let outcome = resolve(dir.path(), "cmd", &config, Trust::Allowlist);
        assert!(matches!(outcome, ProjectFileOutcome::NoSection { .. }));
    }

    fn project_dir_with(text: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(PROJECT_FILE_NAME), text).expect("write project file");
        dir
    }

    #[test]
    fn pinned_trust_reads_outside_trusted_paths_and_allowlist_does_not() {
        let dir = project_dir_with("[pwsh]\nnpm run dev\n");
        let config = ProjectFile::default();
        assert!(matches!(
            resolve(dir.path(), "pwsh", &config, Trust::Allowlist),
            ProjectFileOutcome::Untrusted { .. }
        ));
        match resolve(dir.path(), "pwsh", &config, Trust::Pinned) {
            ProjectFileOutcome::Ready { command, .. } => assert_eq!(command, "npm run dev"),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn disabled_wins_with_both_trusts_without_touching_disk() {
        let config = ProjectFile {
            enabled: false,
            trusted_paths: Vec::new(),
        };
        let missing_dir = std::env::temp_dir().join("porecatu-config-test-does-not-exist");
        for trust in [Trust::Allowlist, Trust::Pinned] {
            assert_eq!(
                resolve(&missing_dir, "pwsh", &config, trust),
                ProjectFileOutcome::Disabled
            );
        }
    }

    // --- seção reservada [icon] ---

    #[test]
    fn a_shell_called_icon_falls_back_to_default() {
        let script = parse("[icon]\nassets/logo.png\n[default]\necho default\n");
        assert_eq!(script.command_for("icon"), Some("echo default"));
        assert_eq!(script.command_for("ICON"), Some("echo default"));
    }

    #[test]
    fn a_shell_called_icon_with_no_default_has_no_command() {
        let script = parse("[icon]\nassets/logo.png\n");
        assert_eq!(script.command_for("icon"), None);
    }

    #[test]
    fn default_does_not_satisfy_icon_path() {
        let script = parse("[default]\nassets/logo.png\n");
        assert_eq!(script.icon_path(), None);
    }

    #[test]
    fn icon_path_is_the_first_non_blank_line_trimmed() {
        let script = parse("[icon]\n\n   assets/logo.png  \nignored\n");
        assert_eq!(script.icon_path(), Some("assets/logo.png"));
    }

    #[test]
    fn empty_icon_body_is_none() {
        assert_eq!(parse("[icon]\n\n  \n[pwsh]\necho\n").icon_path(), None);
        assert_eq!(parse("[icon]").icon_path(), None);
    }

    #[test]
    fn the_first_icon_section_wins() {
        let script = parse("[icon]\nfirst.png\n[icon]\nsecond.png\n");
        assert_eq!(script.icon_path(), Some("first.png"));
    }

    #[test]
    fn uppercase_icon_header_works() {
        assert_eq!(parse("[ICON]\nlogo.png\n").icon_path(), Some("logo.png"));
    }

    // --- resolve_icon ---

    use std::cell::RefCell;

    /// Fake disk: serves one `.porecatu` text, says every non-UNC path is a
    /// regular file, and records every path asked about.
    struct FakeFs {
        project_text: Option<String>,
        regular: Vec<PathBuf>,
        read_calls: RefCell<Vec<PathBuf>>,
        file_calls: RefCell<Vec<PathBuf>>,
    }

    impl FakeFs {
        fn new(project_text: Option<&str>, regular: Vec<PathBuf>) -> Self {
            Self {
                project_text: project_text.map(str::to_owned),
                regular,
                read_calls: RefCell::default(),
                file_calls: RefCell::default(),
            }
        }
    }

    impl IconFs for FakeFs {
        fn read_project_file(&self, path: &Path) -> Option<String> {
            self.read_calls.borrow_mut().push(path.to_path_buf());
            self.project_text.clone()
        }

        fn is_regular_file(&self, path: &Path) -> bool {
            self.file_calls.borrow_mut().push(path.to_path_buf());
            self.regular.iter().any(|p| p == path)
        }
    }

    #[test]
    fn relative_icon_is_joined_to_the_project_dir() {
        let dir = project_dir_with("[icon]\nassets/logo.png\n");
        std::fs::create_dir(dir.path().join("assets")).expect("mkdir");
        std::fs::write(dir.path().join("assets").join("logo.png"), b"x").expect("write");
        assert_eq!(
            resolve_icon(dir.path(), &ProjectFile::default()),
            Some(dir.path().join("assets/logo.png"))
        );
    }

    #[test]
    fn absolute_icon_is_accepted_as_is() {
        let elsewhere = tempfile::tempdir().expect("tempdir");
        let image = elsewhere.path().join("logo.png");
        std::fs::write(&image, b"x").expect("write");
        let dir = project_dir_with(&format!("[icon]\n{}\n", image.display()));
        assert_eq!(
            resolve_icon(dir.path(), &ProjectFile::default()),
            Some(image)
        );
    }

    #[test]
    fn tilde_icon_is_expanded_under_home() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let nested = tempfile::tempdir_in(&home).expect("cannot create temp dir under home");
        std::fs::write(nested.path().join("logo.png"), b"x").expect("write");
        let name = nested.path().file_name().expect("name").to_string_lossy();
        let dir = project_dir_with(&format!("[icon]\n~/{name}/logo.png\n"));
        let got = resolve_icon(dir.path(), &ProjectFile::default()).expect("resolved");
        assert_eq!(got, home.join(name.as_ref()).join("logo.png"));
    }

    #[test]
    fn missing_icon_file_is_none() {
        let dir = project_dir_with("[icon]\nnope.png\n");
        assert_eq!(resolve_icon(dir.path(), &ProjectFile::default()), None);
    }

    #[test]
    fn directory_as_icon_is_none() {
        let dir = project_dir_with("[icon]\nassets\n");
        std::fs::create_dir(dir.path().join("assets")).expect("mkdir");
        assert_eq!(resolve_icon(dir.path(), &ProjectFile::default()), None);
    }

    #[test]
    fn no_project_file_or_no_icon_section_is_none() {
        let empty = tempfile::tempdir().expect("tempdir");
        assert_eq!(resolve_icon(empty.path(), &ProjectFile::default()), None);
        let dir = project_dir_with("[pwsh]\necho\n");
        assert_eq!(resolve_icon(dir.path(), &ProjectFile::default()), None);
    }

    #[test]
    fn disabled_project_file_touches_nothing() {
        let fs = FakeFs::new(Some("[icon]\nlogo.png\n"), Vec::new());
        let config = ProjectFile {
            enabled: false,
            trusted_paths: Vec::new(),
        };
        assert_eq!(resolve_icon_with(&fs, Path::new("/p"), &config), None);
        assert!(fs.read_calls.borrow().is_empty());
        assert!(fs.file_calls.borrow().is_empty());
    }

    #[test]
    fn network_and_device_paths_never_reach_the_filesystem() {
        let refused = [
            r"\\servidor\x.png",
            "//servidor/x.png",
            r"\\?\UNC\servidor\x.png",
            r"\\.\pipe\x",
            r"\\?\C:\x.png",
            r"\/servidor/x.png",
            r"/\servidor\x.png",
        ];
        for declared in refused {
            let text = format!("[icon]\n{declared}\n");
            // Even if the fake would answer "yes" to it, it is never asked.
            let fs = FakeFs::new(Some(&text), vec![PathBuf::from(declared)]);
            let got = resolve_icon_with(&fs, Path::new("/p"), &ProjectFile::default());
            assert_eq!(got, None, "{declared}");
            assert!(
                fs.file_calls.borrow().is_empty(),
                "{declared} reached is_regular_file"
            );
            // Only the `.porecatu` itself was read.
            assert_eq!(
                *fs.read_calls.borrow(),
                vec![Path::new("/p").join(PROJECT_FILE_NAME)]
            );
        }
    }

    #[test]
    fn a_project_dir_that_is_a_network_path_is_refused_without_touching_the_icon() {
        let fs = FakeFs::new(Some("[icon]\nlogo.png\n"), Vec::new());
        let dir = Path::new(r"\\servidor\share");
        assert_eq!(resolve_icon_with(&fs, dir, &ProjectFile::default()), None);
        assert!(fs.file_calls.borrow().is_empty());
    }

    #[test]
    fn a_relative_icon_under_a_verbatim_local_dir_is_allowed() {
        let dir = PathBuf::from(r"\\?\C:\projetos\api");
        let expected = dir.join("logo.png");
        let fs = FakeFs::new(Some("[icon]\nlogo.png\n"), vec![expected.clone()]);
        assert_eq!(
            resolve_icon_with(&fs, &dir, &ProjectFile::default()),
            Some(expected)
        );
    }

    #[test]
    fn the_verbatim_local_drive_check_is_exact() {
        assert!(is_verbatim_local_drive(r"\\?\C:\x"));
        assert!(!is_verbatim_local_drive(r"\\?\UNC\srv\x"));
        assert!(!is_verbatim_local_drive(r"\\srv\x"));
        assert!(!is_verbatim_local_drive(r"\\.\C:\x"));
        assert!(!is_verbatim_local_drive("C:"));
    }

    #[test]
    fn unc_and_device_paths_are_refused_by_text_but_not_local_ones() {
        for refused in [
            r"\\srv\share\x",
            "//srv/share/x",
            r"\/srv",
            r"/\srv",
            r"\\?\UNC\srv\x",
            r"\\.\pipe\x",
            r"\\.\C:\x",
        ] {
            assert!(is_unc_or_device_path(refused), "{refused}");
        }
        for allowed in [
            r"C:\proj",
            "/home/user/proj",
            r"\\?\C:\proj",
            "//?/C:/proj",
            "relative/dir",
            "",
            r"\single",
        ] {
            assert!(!is_unc_or_device_path(allowed), "{allowed}");
        }
    }
}
