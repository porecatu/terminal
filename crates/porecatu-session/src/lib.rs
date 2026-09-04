// SPDX-License-Identifier: GPL-3.0-or-later

//! `porecatu-session` (ADR-0036, PRD-003): carrega e grava o arquivo de
//! sessão. **Sem consumidor nesta etapa** -- ninguém em `porecatu-ui`
//! chama isto ainda; a F5 liga isto à UI em etapas seguintes.

pub mod convert;
pub mod path;
pub mod schema;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub use schema::CURRENT_SCHEMA_VERSION;
pub use schema::v1::{GeometryV1, GroupV1, MonitorIdV1, SessionFileV1, TabV1, WindowV1};

/// Um aviso a mostrar ao usuário sobre a leitura da sessão (ADR-0036 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// Arquivo estava corrompido/truncado; foi renomeado e preservado no
    /// caminho dado, nunca sobrescrevendo um `.corrupt` anterior.
    Corrupt(PathBuf),
    /// `schema_version` é mais nova que a que este binário suporta; o
    /// arquivo original foi preservado, nada foi sobrescrito.
    NewerSchema { found: u32, supported: u32 },
}

/// O que [`load`] devolve: a sessão a restaurar (se houver) e o que
/// avisar. A UI precisa dos dois -- sessão `None` com aviso não é o mesmo
/// que sessão `None` sem aviso (arquivo ausente, normal).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadOutcome {
    pub session: Option<SessionFileV1>,
    pub notices: Vec<Notice>,
}

/// Carrega o arquivo de sessão do caminho resolvido por
/// [`path::resolve_session_path`]. Arquivo ausente não é erro -- devolve
/// `LoadOutcome` vazio, sem aviso (ADR-0036 §5).
pub fn load() -> LoadOutcome {
    match path::resolve_session_path() {
        Some(session_path) => load_from(&session_path),
        None => LoadOutcome::default(),
    }
}

/// Núcleo testável de [`load`], recebendo o caminho explícito -- é o que
/// permite testar sem tocar `PORECATU_SESSION`/`std::env`.
pub fn load_from(session_path: &Path) -> LoadOutcome {
    let bytes = match fs::read(session_path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return LoadOutcome::default(),
        // Erro de I/O que não é "ausente" (permissão, etc.): tratado como
        // sessão ausente, sem aviso -- não há arquivo bom para preservar
        // nem conteúdo pra examinar.
        Err(_) => return LoadOutcome::default(),
    };

    let raw: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return quarantine(session_path),
    };

    match schema::dispatch(&raw, &[]) {
        Ok(schema::Dispatch::Current(session)) => LoadOutcome {
            session: Some(session),
            notices: Vec::new(),
        },
        Ok(schema::Dispatch::Newer(found)) => LoadOutcome {
            session: None,
            notices: vec![Notice::NewerSchema {
                found,
                supported: CURRENT_SCHEMA_VERSION,
            }],
        },
        Err(_) => quarantine(session_path),
    }
}

/// Renomeia o arquivo corrompido/inválido para `.corrupt`, `.corrupt.1`,
/// `.corrupt.2`... no primeiro nome livre. **Nunca sobrescreve**: o
/// arquivo preservado existe para ser examinado, e um segundo acidente não
/// pode apagar a evidência do primeiro (ADR-0036 §5).
fn quarantine(session_path: &Path) -> LoadOutcome {
    match rename_to_first_free_corrupt(session_path) {
        Ok(corrupt_path) => LoadOutcome {
            session: None,
            notices: vec![Notice::Corrupt(corrupt_path)],
        },
        // Não deu para renomear (ex.: já sumiu do disco entre o read e
        // aqui): sessão ausente é o fallback mais seguro.
        Err(_) => LoadOutcome::default(),
    }
}

fn rename_to_first_free_corrupt(session_path: &Path) -> io::Result<PathBuf> {
    let mut suffix = None;
    let candidate = loop {
        let candidate = corrupt_path(session_path, suffix);
        if !candidate.exists() {
            break candidate;
        }
        suffix = Some(suffix.map_or(1, |n: u32| n + 1));
    };
    fs::rename(session_path, &candidate)?;
    Ok(candidate)
}

fn corrupt_path(session_path: &Path, suffix: Option<u32>) -> PathBuf {
    let mut name = session_path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".corrupt");
    if let Some(n) = suffix {
        name.push(format!(".{n}"));
    }
    session_path.with_file_name(name)
}

/// Grava a sessão no caminho resolvido por [`path::resolve_session_path`].
pub fn save(session: &SessionFileV1) -> io::Result<()> {
    let session_path = path::resolve_session_path()
        .ok_or_else(|| io::Error::other("sem caminho de sessão resolvível"))?;
    save_to(&session_path, session)
}

/// Núcleo testável de [`save`]: escreve `<nome>.tmp` no mesmo diretório,
/// `fsync`, `rename` atômico sobre o caminho final. Nunca trunca o
/// arquivo bom antes de ter o novo completo (ADR-0005/ADR-0036 §5).
pub fn save_to(session_path: &Path, session: &SessionFileV1) -> io::Result<()> {
    if let Some(dir) = session_path.parent() {
        fs::create_dir_all(dir)?;
    }

    let mut tmp_name = session_path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    tmp_name.push(".tmp");
    let tmp_path = session_path.with_file_name(tmp_name);

    let bytes = serde_json::to_vec_pretty(session)?;
    let mut file = fs::File::create(&tmp_path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);

    fs::rename(&tmp_path, session_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(active_tab: Option<u32>) -> WindowV1 {
        WindowV1 {
            geometry: GeometryV1 {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
                maximized: false,
            },
            monitor: None,
            groups: vec![GroupV1 {
                id: 0,
                name: None,
                color: None,
                collapsed: false,
                tabs: vec![0],
            }],
            tabs: vec![TabV1 {
                id: 0,
                custom_title: None,
                cwd: None,
                spawn_program: Some("zsh".to_string()),
            }],
            active_tab,
            theme: None,
            zoom_steps: 0,
        }
    }

    fn sample_session() -> SessionFileV1 {
        SessionFileV1 {
            schema_version: CURRENT_SCHEMA_VERSION,
            windows: vec![window(Some(0))],
            shell_integration_dismissed: false,
        }
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let dir = tempdir();
        let path = dir.join("session.json");
        let outcome = load_from(&path);
        assert_eq!(outcome, LoadOutcome::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempdir();
        let path = dir.join("session.json");
        let session = sample_session();
        save_to(&path, &session).unwrap();

        let outcome = load_from(&path);
        assert_eq!(outcome.session, Some(session));
        assert!(outcome.notices.is_empty());
    }

    #[test]
    fn corrupt_json_is_quarantined_without_overwrite() {
        let dir = tempdir();
        let path = dir.join("session.json");
        fs::write(&path, b"{ nao eh json valido").unwrap();

        let outcome = load_from(&path);
        assert_eq!(outcome.session, None);
        let corrupt_path = dir.join("session.json.corrupt");
        assert_eq!(outcome.notices, vec![Notice::Corrupt(corrupt_path.clone())]);
        assert!(corrupt_path.exists());
        assert!(!path.exists());
    }

    #[test]
    fn second_corruption_does_not_overwrite_the_first() {
        let dir = tempdir();
        let path = dir.join("session.json");

        fs::write(&path, b"primeiro corrompido").unwrap();
        load_from(&path);
        assert!(dir.join("session.json.corrupt").exists());

        fs::write(&path, b"segundo corrompido").unwrap();
        load_from(&path);
        assert!(dir.join("session.json.corrupt.1").exists());

        assert_eq!(
            fs::read(dir.join("session.json.corrupt")).unwrap(),
            b"primeiro corrompido"
        );
        assert_eq!(
            fs::read(dir.join("session.json.corrupt.1")).unwrap(),
            b"segundo corrompido"
        );
    }

    #[test]
    fn newer_schema_version_preserves_file_and_warns() {
        let dir = tempdir();
        let path = dir.join("session.json");
        fs::write(&path, br#"{"schema_version":99,"windows":[]}"#).unwrap();

        let outcome = load_from(&path);
        assert_eq!(outcome.session, None);
        assert_eq!(
            outcome.notices,
            vec![Notice::NewerSchema {
                found: 99,
                supported: CURRENT_SCHEMA_VERSION,
            }]
        );
        assert!(
            path.exists(),
            "arquivo de versão mais nova precisa ser preservado"
        );
        assert_eq!(
            fs::read(&path).unwrap(),
            br#"{"schema_version":99,"windows":[]}"#
        );
    }

    /// Escrita interrompida: o `.tmp` existe, o `rename` não aconteceu.
    /// `session.json` anterior continua íntegro e legível.
    #[test]
    fn interrupted_write_leaves_previous_session_intact() {
        let dir = tempdir();
        let path = dir.join("session.json");
        let good = sample_session();
        save_to(&path, &good).unwrap();

        let mut tmp_name = path.file_name().unwrap().to_os_string();
        tmp_name.push(".tmp");
        fs::write(dir.join(tmp_name), b"escrita pela metade").unwrap();

        let outcome = load_from(&path);
        assert_eq!(outcome.session, Some(good));
        assert!(outcome.notices.is_empty());
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "porecatu-session-test-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn unique_suffix() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }
}
