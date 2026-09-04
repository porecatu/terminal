// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolução de caminho (ADR-0036 §6): `PORECATU_SESSION` -> caminho de
//! plataforma via `dirs`. Sem nível de flag -- não existe `--session`.
//! Deliberadamente diferente do diretório de config (ADR-0005): config é
//! arquivo do usuário, sessão é estado da máquina.

use std::path::{Path, PathBuf};

const ENV_VAR: &str = "PORECATU_SESSION";
const FILE_NAME: &str = "session.json";

/// Resolve o caminho do arquivo de sessão pela precedência do ADR-0036 §6.
///
/// Devolve `None` só se `PORECATU_SESSION` não estiver definida e o
/// diretório de estado de plataforma também falhar em resolver.
pub fn resolve_session_path() -> Option<PathBuf> {
    let env_session = std::env::var(ENV_VAR).ok();
    resolve(env_session.as_deref(), platform_default_path)
}

/// Núcleo puro da precedência, sem tocar `std::env` -- é o que torna a
/// ordem testável sem mutar estado de processo.
fn resolve(
    env_session: Option<&str>,
    platform_default: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(path) = env_session
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }
    platform_default()
}

#[cfg(target_os = "linux")]
fn platform_default_path() -> Option<PathBuf> {
    dirs::state_dir().map(|dir| dir.join("porecatu").join(FILE_NAME))
}

#[cfg(target_os = "macos")]
fn platform_default_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("porecatu").join(FILE_NAME))
}

#[cfg(target_os = "windows")]
fn platform_default_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("porecatu").join(FILE_NAME))
}

/// Devolve o diretório que contém o arquivo de sessão, para quem grava
/// `session.json.tmp` no mesmo diretório antes do `rename` atômico.
pub fn session_dir(session_path: &Path) -> Option<&Path> {
    session_path.parent()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform_stub() -> Option<PathBuf> {
        Some(PathBuf::from("/platform/session.json"))
    }

    #[test]
    fn env_wins_over_platform() {
        assert_eq!(
            resolve(Some("/env/session.json"), platform_stub),
            Some(PathBuf::from("/env/session.json"))
        );
    }

    #[test]
    fn empty_env_falls_back_to_platform() {
        assert_eq!(resolve(Some(""), platform_stub), platform_stub());
    }

    #[test]
    fn platform_default_is_last_resort() {
        assert_eq!(resolve(None, platform_stub), platform_stub());
    }
}
