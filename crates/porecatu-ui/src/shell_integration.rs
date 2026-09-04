// SPDX-License-Identifier: GPL-3.0-or-later

//! Texto do convite de integração de shell (RF-3.1, ADR-0039). O snippet
//! por shell é **embutido a partir de** `docs/reference/integracao-de-shell.md`
//! -- fonte única (ADR-0039 §5): corrigir o snippet lá corrige o que o
//! usuário vê na nota, sem duplicar o texto aqui.

use porecatu_term::SHELL_INTEGRATION_DISMISS_MARKER;

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

/// Texto completo da nota (RF-3.1, ADR-0039 §1/§3/§4): consequência
/// (proeminente no Windows -- §3), snippet ou explicação, instrução de
/// cópia (a seleção normal do terminal já basta) e a dispensa definitiva
/// (§4, digitada no terminal).
pub fn invite_text(shell_name: &str) -> String {
    let consequence = if cfg!(windows) {
        "Sem ela, o diretório desta aba não será restaurado quando você reabrir o Porecatu."
    } else {
        "Sem ela, a restauração de diretório desta aba fica menos exata (usa um caminho mais caro para descobrir onde você está)."
    };

    let body = match snippet_for(shell_name) {
        Snippet::Named(heading) => extract_snippet(heading)
            .map_or_else(generic_explanation, |snippet| {
                format!("Cole isto na config do seu shell:\n\n{snippet}")
            }),
        Snippet::Cmd => {
            "O cmd.exe não tem forma confiável de emitir esse aviso -- use PowerShell se \
             restaurar o diretório importar para você."
                .to_string()
        }
        Snippet::Unknown => generic_explanation(),
    };

    let dismiss_marker = SHELL_INTEGRATION_DISMISS_MARKER;
    let text = format!(
        "Este terminal ainda não tem integração de shell (OSC 7). {consequence}\n\n{body}\n\nSelecione o trecho acima com o mouse para copiar, do jeito de sempre.\n\nPara não ver este aviso de novo, digite \"{dismiss_marker}\" e pressione Enter (o shell provavelmente vai dizer que o comando não existe -- sem problema, é só o sinal de dispensa)."
    );
    // `Terminal::inject_note` não normaliza quebra de linha -- o snippet
    // embutido do arquivo de referência traz `\n` cru (markdown comum), e
    // este módulo monta o resto do texto com `\n` pela mesma razão de
    // simplicidade. Uma passada só, no fim, garante `\r\n` em toda parte
    // (idempotente: colapsa `\r\n` já existente antes de expandir).
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn generic_explanation() -> String {
    "Shell não reconhecido para sugerir um trecho pronto. OSC 7 é a sequência que o shell \
     emite a cada mudança de diretório -- veja docs/reference/integracao-de-shell.md no \
     repositório do Porecatu para configurar o seu."
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_snippet_is_extracted_from_the_reference_file() {
        let text = invite_text("bash");
        assert!(text.contains("__porecatu_osc7"));
        assert!(text.contains("PROMPT_COMMAND"));
    }

    #[test]
    fn zsh_snippet_is_extracted() {
        let text = invite_text("zsh");
        assert!(text.contains("add-zsh-hook"));
    }

    #[test]
    fn fish_snippet_is_extracted() {
        let text = invite_text("fish");
        assert!(text.contains("--on-variable PWD"));
    }

    #[test]
    fn powershell_snippet_is_extracted_for_pwsh_and_windows_powershell() {
        assert!(invite_text("pwsh").contains("function prompt"));
        assert!(invite_text("powershell").contains("function prompt"));
    }

    #[test]
    fn cmd_has_no_snippet_and_points_to_powershell() {
        let text = invite_text("cmd");
        assert!(!text.contains("```"));
        assert!(text.contains("PowerShell"));
    }

    #[test]
    fn unknown_shell_falls_back_to_the_generic_note() {
        let text = invite_text("nu");
        assert!(text.contains("docs/reference/integracao-de-shell.md"));
    }

    #[test]
    fn dismiss_instruction_carries_the_real_marker() {
        assert!(invite_text("bash").contains(porecatu_term::SHELL_INTEGRATION_DISMISS_MARKER));
    }

    #[test]
    fn no_bare_newline_ever_reaches_inject_note() {
        // `Terminal::inject_note` não normaliza `\n` cru -- confirma que a
        // nota inteira só usa `\r\n` como quebra de linha.
        let text = invite_text("bash");
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                assert_eq!(
                    text.as_bytes()[index - 1],
                    b'\r',
                    "quebra de linha crua em {index}"
                );
            }
        }
    }
}
