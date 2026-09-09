// SPDX-License-Identifier: GPL-3.0-or-later

//! Branch do repositório Git do diretório da aba ativa (ADR-0049), para o
//! segmento correspondente da barra de status.
//!
//! **Só a branch.** Estado da árvore -- sujo/limpo, ahead/behind, o que
//! está em stage -- fica de fora, e é fronteira, não pendência: tudo isso
//! precisa percorrer a árvore, custa ordens de grandeza mais, e nada
//! barato diz quando refazer. Ver ADR-0049 §7.
//!
//! Sem `git2`, sem processo `git`, sem thread e sem temporizador: é a
//! leitura de um arquivo de ~30 bytes, revalidada por `mtime`. O que roda
//! por frame é um `stat`; a descoberta do repositório (subir diretórios
//! atrás de `.git`) só refaz quando o `cwd` muda.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// O que `.git/HEAD` diz. As duas formas que aparecem em uso normal --
/// qualquer outra coisa é `None` em [`parse_head`], não um palpite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// `ref: refs/heads/<nome>`.
    Branch(String),
    /// Um SHA solto: `HEAD` destacado. Guarda já encurtado.
    Detached(String),
}

impl Head {
    /// O que a barra exibe -- o nome, ou o SHA curto.
    pub fn label(&self) -> &str {
        match self {
            Self::Branch(name) | Self::Detached(name) => name,
        }
    }
}

/// Quantos hex de um SHA destacado a barra mostra. É o corte que o
/// próprio `git` usa por padrão ao abreviar.
const SHORT_SHA_LEN: usize = 7;

/// Interpreta o conteúdo de `.git/HEAD`. Pura: é o que torna as duas
/// formas testáveis sem repositório nenhum no disco.
///
/// Devolve `None` para qualquer coisa que não bata exatamente com uma
/// delas -- inclusive conteúdo truncado, que é o que se leria ao pegar o
/// arquivo no meio de uma escrita. Esconder o segmento por um frame é
/// melhor que mostrar lixo.
pub fn parse_head(content: &str) -> Option<Head> {
    let line = content.lines().next()?.trim();
    if let Some(reference) = line.strip_prefix("ref:") {
        // O nome da branch é o que vem depois do último `/`: um
        // `refs/heads/feat/x` é a branch `feat/x`, não `x`, então o corte
        // é do prefixo conhecido, não do último separador.
        let reference = reference.trim();
        let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        if name.is_empty() {
            return None;
        }
        return Some(Head::Branch(name.to_owned()));
    }
    // SHA solto: 40 hex (SHA-1) ou 64 (SHA-256, que o git já aceita).
    let is_sha =
        (line.len() == 40 || line.len() == 64) && line.chars().all(|c| c.is_ascii_hexdigit());
    if is_sha {
        return Some(Head::Detached(line[..SHORT_SHA_LEN].to_owned()));
    }
    None
}

/// Resolve o conteúdo de um `.git` que é **arquivo**, não diretório --
/// a forma que worktree e submódulo usam: `gitdir: <caminho>`.
///
/// Ignorar isto mostraria "sem repositório" dentro de um worktree, que é
/// pior que não mostrar nada (ADR-0049 §2).
fn parse_gitdir_file(content: &str) -> Option<PathBuf> {
    let line = content.lines().next()?.trim();
    let path = line.strip_prefix("gitdir:")?.trim();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

/// Sobe de `start` até a raiz procurando um `.git`, e devolve o caminho
/// do **`HEAD`** dentro dele.
///
/// É o passo caro -- um `stat` por nível --, e por isso [`GitInfo`] só o
/// refaz quando o `cwd` muda.
fn find_head_file(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let dot_git = dir.join(".git");
        let metadata = std::fs::metadata(&dot_git).ok();
        match metadata {
            Some(m) if m.is_dir() => return Some(dot_git.join("HEAD")),
            Some(m) if m.is_file() => {
                // Worktree ou submódulo. O caminho pode ser relativo ao
                // diretório que contém o `.git`.
                let content = std::fs::read_to_string(&dot_git).ok()?;
                let gitdir = parse_gitdir_file(&content)?;
                let gitdir = if gitdir.is_absolute() {
                    gitdir
                } else {
                    dir.join(gitdir)
                };
                return Some(gitdir.join("HEAD"));
            }
            _ => {}
        }
    }
    None
}

/// Branch da aba ativa, com o cache que a torna barata de consultar por
/// frame (ADR-0049 §1). Vive em `WindowState`: é sempre a do `cwd` que a
/// barra está exibindo.
#[derive(Debug, Default)]
pub struct GitInfo {
    /// Para qual `cwd` a descoberta abaixo foi feita. Mudou o `cwd`,
    /// redescobre; é a única coisa que dispara o passo caro.
    resolved_for: Option<PathBuf>,
    /// Caminho do `HEAD`, ou `None` se não há repositório acima do `cwd`
    /// -- **e o `None` também é cacheado**: fora de um repositório, a
    /// subida de diretórios não se repete a cada frame.
    head_path: Option<PathBuf>,
    /// `mtime` da última leitura. É o que decide se relê.
    read_at: Option<SystemTime>,
    head: Option<Head>,
}

impl GitInfo {
    /// Revalida contra `cwd` e devolve a branch, se houver. Chamada no
    /// caminho que monta o conteúdo da barra -- ou seja, só quando um
    /// frame vai ser desenhado.
    ///
    /// O trabalho no caso comum (mesmo `cwd`, `HEAD` intocado) é **um
    /// `stat`**. Sem `cwd`, nem isso.
    pub fn branch(&mut self, cwd: Option<&Path>) -> Option<&str> {
        let cwd = cwd?;

        if self.resolved_for.as_deref() != Some(cwd) {
            self.resolved_for = Some(cwd.to_path_buf());
            self.head_path = find_head_file(cwd);
            self.read_at = None;
            self.head = None;
        }

        let head_path = self.head_path.as_ref()?;
        let mtime = std::fs::metadata(head_path).and_then(|m| m.modified()).ok();
        if mtime != self.read_at || self.head.is_none() {
            self.read_at = mtime;
            self.head = std::fs::read_to_string(head_path)
                .ok()
                .as_deref()
                .and_then(parse_head);
        }
        self.head.as_ref().map(Head::label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_branch() {
        assert_eq!(
            parse_head("ref: refs/heads/main\n"),
            Some(Head::Branch("main".to_owned()))
        );
    }

    #[test]
    fn branch_name_keeps_its_own_slashes() {
        // `feat/x` é o nome da branch inteiro. Cortar no último `/`
        // mostraria só `x`, que é outra branch possível no mesmo repo.
        assert_eq!(
            parse_head("ref: refs/heads/feat/porecatu-ui\n"),
            Some(Head::Branch("feat/porecatu-ui".to_owned()))
        );
    }

    #[test]
    fn detached_head_is_the_short_sha() {
        let sha = "80722d9e1f4a6b3c5d8e0f2a4b6c8d0e2f4a6b8c";
        assert_eq!(
            parse_head(sha),
            Some(Head::Detached("80722d9".to_owned())),
            "os mesmos 7 hex que o git usa ao abreviar"
        );
    }

    #[test]
    fn sha256_detached_head_also_works() {
        let sha = "a".repeat(64);
        assert_eq!(parse_head(&sha), Some(Head::Detached("aaaaaaa".to_owned())));
    }

    #[test]
    fn garbage_and_partial_writes_yield_nothing() {
        // Ler o arquivo no meio de uma escrita do git: melhor esconder o
        // segmento por um frame que desenhar lixo (ADR-0049 §2).
        for content in ["", "\n", "ref:", "ref: ", "não é um head", "80722d9"] {
            assert_eq!(parse_head(content), None, "conteúdo {content:?}");
        }
    }

    #[test]
    fn a_sha_of_the_wrong_length_is_not_a_sha() {
        assert_eq!(parse_head(&"a".repeat(39)), None);
        assert_eq!(parse_head(&"a".repeat(41)), None);
        assert_eq!(parse_head(&"z".repeat(40)), None, "não é hex");
    }

    #[test]
    fn gitdir_file_of_a_worktree() {
        assert_eq!(
            parse_gitdir_file("gitdir: /repo/.git/worktrees/wt\n"),
            Some(PathBuf::from("/repo/.git/worktrees/wt"))
        );
        assert_eq!(
            parse_gitdir_file("gitdir: ../.git/modules/sub"),
            Some(PathBuf::from("../.git/modules/sub"))
        );
    }

    #[test]
    fn a_dot_git_file_without_gitdir_is_not_a_repository() {
        for content in ["", "qualquer coisa", "gitdir:", "gitdir:   "] {
            assert_eq!(parse_gitdir_file(content), None, "conteúdo {content:?}");
        }
    }

    #[test]
    fn no_cwd_means_no_work_and_no_branch() {
        let mut info = GitInfo::default();
        assert_eq!(info.branch(None), None);
        assert!(
            info.resolved_for.is_none(),
            "sem `cwd` não há nem o que descobrir"
        );
    }

    #[test]
    fn this_very_repository_reports_its_branch() {
        // O teste roda dentro do repositório do Porecatu, então a
        // descoberta tem de achá-lo subindo de `crates/porecatu-ui`.
        let mut info = GitInfo::default();
        let here = std::env::current_dir().expect("cwd do teste");
        let branch = info.branch(Some(&here));
        assert!(
            branch.is_some(),
            "não achou o repositório subindo de {}",
            here.display()
        );
        assert!(!branch.unwrap().is_empty());
    }

    #[test]
    fn a_directory_outside_any_repository_caches_the_absence() {
        // O `None` também é cacheado: fora de um repositório, a subida de
        // diretórios não pode se repetir a cada frame.
        let mut info = GitInfo::default();
        let root = if cfg!(windows) {
            PathBuf::from("C:\\")
        } else {
            PathBuf::from("/")
        };
        // A raiz do sistema não é um repositório em nenhuma máquina de
        // desenvolvimento sã; se for, o teste não tem o que afirmar.
        if info.branch(Some(&root)).is_none() {
            assert_eq!(info.resolved_for.as_deref(), Some(root.as_path()));
            assert!(info.head_path.is_none(), "a ausência ficou registrada");
        }
    }
}
