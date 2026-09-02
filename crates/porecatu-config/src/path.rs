// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolução de caminho (ADR-0003): `--config` -> `PORECATU_CONFIG` ->
//! caminho de plataforma via `dirs`. Arquivo ausente não é erro -- é
//! `load` (em `lib.rs`) quem decide o que fazer com um caminho que não
//! existe.

use std::path::{Path, PathBuf};

const ENV_VAR: &str = "PORECATU_CONFIG";
const FILE_NAME: &str = "porecatu.toml";

/// Resolve o caminho do arquivo de config pela precedência do ADR-0003.
/// `cli_config`, quando presente, é sempre o vencedor -- normalmente vem da
/// flag `--config <caminho>` do binário.
///
/// Devolve `None` só se nenhuma das três fontes resolver um caminho, o que
/// na prática exige que `dirs::config_dir` também falhe -- caso em que não
/// há diretório de config de plataforma para usar.
pub fn resolve_config_path(cli_config: Option<&Path>) -> Option<PathBuf> {
    let env_config = std::env::var(ENV_VAR).ok();
    resolve(cli_config, env_config.as_deref(), platform_default_path)
}

/// Núcleo puro da precedência, sem tocar `std::env` -- é o que torna a
/// ordem das três fontes testável sem mutar estado de processo.
fn resolve(
    cli_config: Option<&Path>,
    env_config: Option<&str>,
    platform_default: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(path) = cli_config {
        return Some(path.to_path_buf());
    }
    if let Some(path) = env_config
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }
    platform_default()
}

#[cfg(not(target_os = "macos"))]
fn platform_default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("porecatu").join(FILE_NAME))
}

/// No macOS o caminho é deliberadamente `~/.config`, não
/// `~/Library/Application Support` (`dirs::config_dir`) -- ver ADR-0003.
#[cfg(target_os = "macos")]
fn platform_default_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config").join("porecatu").join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform_stub() -> Option<PathBuf> {
        Some(PathBuf::from("/platform/porecatu.toml"))
    }

    #[test]
    fn cli_wins_over_env_and_platform() {
        let cli = Path::new("/cli/porecatu.toml");
        assert_eq!(
            resolve(Some(cli), Some("/env/porecatu.toml"), platform_stub),
            Some(cli.to_path_buf())
        );
    }

    #[test]
    fn env_wins_over_platform_when_no_cli() {
        assert_eq!(
            resolve(None, Some("/env/porecatu.toml"), platform_stub),
            Some(PathBuf::from("/env/porecatu.toml"))
        );
    }

    #[test]
    fn empty_env_falls_back_to_platform() {
        assert_eq!(resolve(None, Some(""), platform_stub), platform_stub());
    }

    #[test]
    fn platform_default_is_last_resort() {
        assert_eq!(resolve(None, None, platform_stub), platform_stub());
    }
}
