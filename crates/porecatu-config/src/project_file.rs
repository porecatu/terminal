// SPDX-License-Identifier: GPL-3.0-or-later

//! Duas coisas diferentes moram aqui: a seção `[project_file]` da config do
//! app (RF-12.5, RF-12.7), e o parser do arquivo `.porecatu` do PROJETO, que
//! não é TOML (ADR-0051 §7, docs/arquitetura.md §6.1) -- é o formato de
//! seções cruas que o ADR-0051 §2 decide.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const PROJECT_FILE_NAME: &str = ".porecatu";

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
    /// erro (ADR-0051 §3).
    pub fn command_for(&self, shell_name: &str) -> Option<&str> {
        let shell_name = shell_name.to_lowercase();
        self.sections
            .iter()
            .find(|section| section.name == shell_name)
            .or_else(|| {
                self.sections
                    .iter()
                    .find(|section| section.name == "default")
            })
            .map(|section| section.body.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

/// Expande um `~` inicial via `dirs::home_dir`; qualquer outro caminho
/// volta inalterado.
fn expand_tilde(entry: &str) -> PathBuf {
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
/// reordene.
pub fn resolve(dir: &Path, shell_name: &str, config: &ProjectFile) -> ProjectFileOutcome {
    if !config.enabled {
        return ProjectFileOutcome::Disabled;
    }

    let path = dir.join(PROJECT_FILE_NAME);
    if !path.is_file() {
        return ProjectFileOutcome::NotFound;
    }

    if !is_trusted(dir, &config.trusted_paths) {
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
        let outcome = resolve(&missing_dir, "pwsh", &config);
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
        let outcome = resolve(dir.path(), "pwsh", &config);
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
        let outcome = resolve(dir.path(), "cmd", &config);
        assert!(matches!(outcome, ProjectFileOutcome::NoSection { .. }));
    }
}
