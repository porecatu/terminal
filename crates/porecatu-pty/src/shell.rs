// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolução do shell default por plataforma
//! (ADR-0004, tabela "Comportamento de shell padrão").
//!
//! `porecatu-config` ainda não existe nesta fase (F1): o passo
//! `config.shell.program` da tabela é responsabilidade de quem chama
//! [`crate::spawn`] — basta preencher `SpawnConfig.program` e esta resolução
//! nunca entra em jogo. O que resta aqui é o resto da cadeia: `$SHELL` e o
//! fallback fixo por plataforma.

/// Resolve o shell default a partir de `$SHELL` e, no Windows, de uma busca
/// por `pwsh.exe` / `powershell.exe` no `PATH`.
///
/// Função pura: recebe o valor de `$SHELL` e uma busca de `PATH` como
/// parâmetros em vez de ler o ambiente diretamente, para ser testável sem
/// depender do processo atual. [`search_path`] é a implementação real.
pub fn resolve_default_shell(
    shell_env: Option<&str>,
    path_lookup: impl Fn(&str) -> bool,
) -> String {
    if let Some(shell) = shell_env
        && !shell.is_empty()
    {
        return shell.to_string();
    }
    platform_fallback(path_lookup)
}

#[cfg(target_os = "windows")]
fn platform_fallback(path_lookup: impl Fn(&str) -> bool) -> String {
    if path_lookup("pwsh.exe") {
        "pwsh.exe".to_string()
    } else if path_lookup("powershell.exe") {
        "powershell.exe".to_string()
    } else {
        "cmd.exe".to_string()
    }
}

#[cfg(target_os = "macos")]
fn platform_fallback(_path_lookup: impl Fn(&str) -> bool) -> String {
    "/bin/zsh".to_string()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_fallback(_path_lookup: impl Fn(&str) -> bool) -> String {
    "/bin/sh".to_string()
}

/// Busca `name` nos diretórios do `PATH` real do processo atual.
/// Implementação de produção do `path_lookup` de [`resolve_default_shell`].
pub fn search_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(name).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_env_tem_precedencia_sobre_fallback() {
        assert_eq!(
            resolve_default_shell(Some("/usr/bin/fish"), |_| true),
            "/usr/bin/fish"
        );
    }

    #[test]
    fn shell_env_vazio_cai_no_fallback() {
        let resolved = resolve_default_shell(Some(""), |_| false);
        assert_ne!(resolved, "");
    }

    #[test]
    fn shell_env_ausente_cai_no_fallback() {
        let resolved = resolve_default_shell(None, |_| false);
        assert_ne!(resolved, "");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_prefere_pwsh_quando_presente() {
        assert_eq!(
            resolve_default_shell(None, |name| name == "pwsh.exe"),
            "pwsh.exe"
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_cai_para_powershell_sem_pwsh() {
        assert_eq!(
            resolve_default_shell(None, |name| name == "powershell.exe"),
            "powershell.exe"
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn windows_cai_para_cmd_sem_nada() {
        assert_eq!(resolve_default_shell(None, |_| false), "cmd.exe");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_usa_zsh() {
        assert_eq!(resolve_default_shell(None, |_| false), "/bin/zsh");
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn linux_usa_sh() {
        assert_eq!(resolve_default_shell(None, |_| false), "/bin/sh");
    }
}
