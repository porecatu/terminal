// SPDX-License-Identifier: GPL-3.0-or-later

//! Texto do convite de integração de shell (RF-3.1, ADR-0039). O snippet
//! por shell é **embutido a partir de** `docs/reference/integracao-de-shell.md`
//! -- fonte única (ADR-0039 §5): corrigir o snippet lá corrige o que o
//! usuário vê na nota, sem duplicar o texto aqui.

use porecatu_locale::Catalog;

use crate::messages::msg;

const REFERENCE_MD: &str = include_str!("../../../docs/reference/integracao-de-shell.md");

/// Extrai o primeiro bloco de código depois do heading `## {heading}` --
/// os cinco heading de shell do arquivo (bash, zsh, fish, PowerShell) têm
/// o bloco imediatamente depois do título, antes de qualquer prosa.
fn extract_snippet(heading: &str) -> Option<&'static str> {
    let marker = format!("## {heading}");
    let after_heading = &REFERENCE_MD[REFERENCE_MD.find(&marker)?..];
    let fence_start = after_heading.find("```")?;
    let after_fence = &after_heading[fence_start + 3..];
    let lang_line_end = after_fence.find('\n')?;
    let body = &after_fence[lang_line_end + 1..];
    let fence_end = body.find("```")?;
    Some(body[..fence_end].trim_end_matches('\n'))
}

/// Nome do heading de `docs/reference/integracao-de-shell.md` para o
/// `shell_name` detectado (`Tab::shell_name`, ADR-0039 §5: "o shell é o
/// detectado no spawn da aba"). `None` = shell não reconhecido, cai na
/// nota genérica.
enum Snippet {
    Named(&'static str),
    Cmd,
    Unknown,
}

fn snippet_for(shell_name: &str) -> Snippet {
    match shell_name.to_ascii_lowercase().as_str() {
        "bash" => Snippet::Named("bash"),
        "zsh" => Snippet::Named("zsh"),
        "fish" => Snippet::Named("fish"),
        "pwsh" | "powershell" => Snippet::Named("PowerShell"),
        "cmd" => Snippet::Cmd,
        _ => Snippet::Unknown,
    }
}

/// O convite de integração de shell (RF-3.1, ADR-0039): o texto do diálogo e,
/// quando há um trecho pronto para o shell, o trecho -- o que o botão de copiar
/// põe na área de transferência.
pub struct Invite {
    pub text: String,
    pub snippet: Option<String>,
}

/// Texto completo do convite (RF-3.1, ADR-0039 §1/§3): consequência
/// (proeminente no Windows -- §3) e snippet ou explicação. As frases são do
/// catálogo; o snippet vem do arquivo de referência, sem tradução (ADR-0056
/// §3). Copiar e dispensar são botões do diálogo, não instruções no texto.
pub fn invite(catalog: &Catalog, shell_name: &str) -> Invite {
    let consequence = if cfg!(windows) {
        msg::note::shell_invite::consequence_windows(catalog)
    } else {
        msg::note::shell_invite::consequence_other(catalog)
    };

    let (body, snippet) = match snippet_for(shell_name) {
        Snippet::Named(heading) => match extract_snippet(heading) {
            Some(snippet) => (
                msg::note::shell_invite::body_snippet(catalog, snippet),
                Some(snippet.to_owned()),
            ),
            None => (msg::note::shell_invite::body_generic(catalog), None),
        },
        Snippet::Cmd => (msg::note::shell_invite::body_cmd(catalog), None),
        Snippet::Unknown => (msg::note::shell_invite::body_generic(catalog), None),
    };

    Invite {
        text: msg::note::shell_invite::text(catalog, consequence, body),
        snippet,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(shell: &str) -> String {
        invite(&crate::messages::test_support::pt_br(), shell).text
    }

    #[test]
    fn bash_snippet_is_extracted_from_the_reference_file() {
        let text = text("bash");
        assert!(text.contains("__porecatu_osc7"));
        assert!(text.contains("PROMPT_COMMAND"));
    }

    #[test]
    fn zsh_snippet_is_extracted() {
        assert!(text("zsh").contains("add-zsh-hook"));
    }

    #[test]
    fn fish_snippet_is_extracted() {
        assert!(text("fish").contains("--on-variable PWD"));
    }

    #[test]
    fn powershell_snippet_is_extracted_for_pwsh_and_windows_powershell() {
        assert!(text("pwsh").contains("function prompt"));
        assert!(text("powershell").contains("function prompt"));
    }

    #[test]
    fn cmd_has_no_snippet_and_points_to_powershell() {
        let invite = invite(&crate::messages::test_support::pt_br(), "cmd");
        assert!(invite.snippet.is_none());
        assert!(!invite.text.contains("```"));
        assert!(invite.text.contains("PowerShell"));
    }

    #[test]
    fn unknown_shell_falls_back_to_the_generic_text_without_a_snippet() {
        let invite = invite(&crate::messages::test_support::pt_br(), "nu");
        assert!(invite.snippet.is_none());
        assert!(
            invite
                .text
                .contains("docs/reference/integracao-de-shell.md")
        );
    }

    #[test]
    fn a_known_shell_hands_the_snippet_to_the_copy_button() {
        let invite = invite(&crate::messages::test_support::pt_br(), "bash");
        let snippet = invite.snippet.expect("bash has a snippet");
        assert!(snippet.contains("PROMPT_COMMAND"));
        assert!(invite.text.contains(&snippet));
    }

    #[test]
    fn the_text_no_longer_asks_to_type_the_dismissal_marker() {
        assert!(!text("bash").contains(porecatu_term::SHELL_INTEGRATION_DISMISS_MARKER));
    }
}
