// SPDX-License-Identifier: GPL-3.0-or-later

//! Sessões nomeadas (ADR-0054, PRD-014): um `SessionFileV1` por sessão,
//! com exatamente uma janela, em `sessions/` ao lado do `session.json`
//! ([`named_sessions_dir`], derivado de `path::resolve_session_path` --
//! nunca resolvido à parte, para `PORECATU_SESSION` deslocar os dois
//! juntos). Reusa `load_from`/`save_to`/`quarantine` do módulo raiz: é o
//! mesmo schema, a mesma quarentena de arquivo inválido, a mesma recusa
//! de `schema_version` mais nova -- sem um segundo schema a manter.
//!
//! A pergunta "já existe, confirma sobrescrita?" (RF-14.5) não é deste
//! módulo -- é da UI, sobre a lista que ela já tem na mão. [`save_named_in`]
//! sobrescreve quando mandada.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io};

use crate::schema::{self, Dispatch};
use crate::{CURRENT_SCHEMA_VERSION, LoadOutcome, SessionFileV1, WindowV1, path};

/// Teto do nome de sessão em caracteres, escalares Unicode (ADR-0054 §3).
/// Este crate não recusa nome maior -- é o campo de texto da UI que vai
/// aplicar o teto; a constante mora aqui para as duas pontas concordarem.
pub const MAX_NAME_CHARS: usize = 64;

const MAX_SLUG_BYTES: usize = 48;

const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Estado de uma entrada da lista (ADR-0054 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryStatus {
    Ok,
    Unreadable,
    NewerSchema { found: u32 },
}

/// Uma linha da lista de sessões nomeadas -- uma por arquivo, inclusive as
/// ruins (RF-14.17): listar nunca renomeia nem move nada no disco.
#[derive(Debug, Clone, PartialEq)]
pub struct NamedSessionEntry {
    pub name: String,
    pub file: PathBuf,
    pub saved_at: Option<u64>,
    pub status: EntryStatus,
}

/// Erro de [`save_named_in`]/[`save_named`] (ADR-0054 §5).
#[derive(Debug)]
pub enum SaveError {
    Io(io::Error),
    /// RF-3.16/RF-14.17: o arquivo do mesmo nome já tem `schema_version`
    /// mais nova que a suportada -- nunca sobrescrito, nem por um salvar
    /// com o mesmo nome.
    NewerSchema {
        found: u32,
    },
    /// Nome vazio depois de aparado (RF-14.3): o campo continua aberto,
    /// sem gravar nada.
    EmptyName,
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Io(err) => write!(f, "erro de E/S ao gravar sessão nomeada: {err}"),
            SaveError::NewerSchema { found } => write!(
                f,
                "arquivo existente tem schema_version {found}, mais nova que {CURRENT_SCHEMA_VERSION}; não sobrescrito"
            ),
            SaveError::EmptyName => write!(f, "nome de sessão vazio depois de aparado"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Transforma um nome livre em nome de arquivo (ADR-0054 §3): minúsculas,
/// diacríticos latinos comuns decompostos à mão (sem crate novo -- ver
/// [`decompose_latin`]), tudo fora de `[a-z0-9]` vira `-`, hífens
/// colapsados e aparados, truncado em [`MAX_SLUG_BYTES`] bytes -- só ASCII
/// sobrevive até aqui, então o corte nunca quebra um caractere ao meio.
/// Vazio depois disso vira `"sessao"`. Nome reservado do Windows (`con`,
/// `nul`, `com1`...) recebe sufixo `-2`.
pub fn slug(name: &str) -> String {
    let mapped: String = name
        .to_lowercase()
        .chars()
        .map(|ch| {
            let base = decompose_latin(ch);
            if base.is_ascii_alphanumeric() {
                base
            } else {
                '-'
            }
        })
        .collect();

    let collapsed = collapse_hyphens(&mapped);
    let mut result = if collapsed.len() > MAX_SLUG_BYTES {
        collapsed[..MAX_SLUG_BYTES]
            .trim_end_matches('-')
            .to_string()
    } else {
        collapsed
    };

    if result.is_empty() {
        result = "sessao".to_string();
    }
    if WINDOWS_RESERVED_NAMES.contains(&result.as_str()) {
        result.push_str("-2");
    }
    result
}

/// Decompõe diacríticos latinos comuns à mão -- só os casos que o ADR-0054
/// §3 lista. Caractere fora dessa tabela sai como veio, e vira `-` em
/// [`slug`] se não for `[a-z0-9]`.
fn decompose_latin(ch: char) -> char {
    match ch {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

/// Colapsa hífens consecutivos em um só e apara as pontas -- separar por
/// `-` e descartar os pedaços vazios faz as duas coisas de uma vez.
fn collapse_hyphens(input: &str) -> String {
    input
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Chave de "nome repetido" (RF-14.5): aparado e sem distinguir caixa
/// (`to_lowercase` Unicode, não só ASCII) -- decide se salvar sobrescreve
/// um arquivo existente ou cria um novo.
pub fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Diretório das sessões nomeadas (ADR-0054 §2): `sessions/`
/// **derivado** do caminho resolvido do `session.json`, nunca resolvido à
/// parte -- é isso que faz `PORECATU_SESSION` deslocar os dois juntos.
pub fn named_sessions_dir() -> Option<PathBuf> {
    let session_path = path::resolve_session_path()?;
    let dir = path::session_dir(&session_path)?;
    Some(dir.join("sessions"))
}

/// Resultado de espiar um arquivo sem tocar o disco -- usado só por
/// [`list_named_in`], que nunca pode renomear nem mover nada (RF-14.17).
enum Peek {
    Valid(SessionFileV1),
    Unreadable,
    NewerSchema(u32),
}

fn peek(file: &Path) -> Peek {
    let bytes = match fs::read(file) {
        Ok(bytes) => bytes,
        Err(_) => return Peek::Unreadable,
    };
    let raw: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return Peek::Unreadable,
    };
    match schema::dispatch(&raw, &[]) {
        Ok(Dispatch::Current(session)) if session.windows.len() == 1 => Peek::Valid(session),
        // Zero ou mais de uma janela: não é uma sessão nomeada válida
        // (ADR-0054 §4 exige exatamente uma).
        Ok(Dispatch::Current(_)) => Peek::Unreadable,
        Ok(Dispatch::Newer(found)) => Peek::NewerSchema(found),
        Err(_) => Peek::Unreadable,
    }
}

/// Lê cada `*.json` de `dir` (ignora `.tmp` e `.corrupt*`, que não têm
/// essa extensão) e devolve uma entrada por arquivo, inclusive as ruins
/// (RF-14.17) -- **nunca muda o disco**. Diretório ausente é lista vazia,
/// não erro, a mesma regra do `session.json` ausente. Ordenada por
/// `saved_at` decrescente, sem `saved_at` no fim, empate pelo nome
/// normalizado (ADR-0054 §5).
pub fn list_named_in(dir: &Path) -> Vec<NamedSessionEntry> {
    let read_dir = match fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for dir_entry in read_dir.flatten() {
        let file = dir_entry.path();
        if file.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        let entry = match peek(&file) {
            Peek::Valid(session) => NamedSessionEntry {
                name: session.name.unwrap_or(stem),
                file,
                saved_at: session.saved_at,
                status: EntryStatus::Ok,
            },
            Peek::Unreadable => NamedSessionEntry {
                name: stem,
                file,
                saved_at: None,
                status: EntryStatus::Unreadable,
            },
            Peek::NewerSchema(found) => NamedSessionEntry {
                name: stem,
                file,
                saved_at: None,
                status: EntryStatus::NewerSchema { found },
            },
        };
        entries.push(entry);
    }

    entries.sort_by(|a, b| match (a.saved_at, b.saved_at) {
        (Some(a_at), Some(b_at)) => b_at
            .cmp(&a_at)
            .then_with(|| normalize_name(&a.name).cmp(&normalize_name(&b.name))),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => normalize_name(&a.name).cmp(&normalize_name(&b.name)),
    });
    entries
}

/// [`list_named_in`] resolvendo o diretório por [`named_sessions_dir`].
/// Sem diretório resolvível, lista vazia.
pub fn list_named() -> Vec<NamedSessionEntry> {
    named_sessions_dir()
        .map(|dir| list_named_in(&dir))
        .unwrap_or_default()
}

/// Carrega um arquivo de sessão nomeada para restaurar (ADR-0054 §5).
/// Reusa [`crate::load_from`] -- mesma quarentena de `.corrupt`, mesma
/// recusa de schema mais novo -- e valida além disso que há **exatamente
/// uma** janela; zero ou mais de uma é tratado como inválido, e o arquivo
/// vai para quarentena como um JSON corrompido iria.
pub fn load_named(file: &Path) -> LoadOutcome {
    let outcome = crate::load_from(file);
    match &outcome.session {
        Some(session) if session.windows.len() == 1 => outcome,
        Some(_) => crate::quarantine(file),
        None => outcome,
    }
}

/// Grava `window` como sessão nomeada `name` em `dir` (ADR-0054 §3/§5).
/// Nome aparado; vazio recusa sem tocar disco. Nome que já existe (por
/// [`normalize_name`]) grava **no arquivo dela**, qualquer que seja o
/// *slug*; senão, [`slug`] com sufixo `-2`, `-3`... no primeiro nome de
/// arquivo livre. Recusa sobrescrever arquivo de `schema_version` mais
/// nova. Cria o diretório se preciso (`save_to` já cria o pai).
pub fn save_named_in(dir: &Path, name: &str, window: WindowV1) -> Result<PathBuf, SaveError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(SaveError::EmptyName);
    }

    let target_key = normalize_name(trimmed);
    let existing = list_named_in(dir);
    let same_name = existing
        .iter()
        .find(|entry| normalize_name(&entry.name) == target_key);

    let target_path = match same_name {
        Some(entry) => {
            if let EntryStatus::NewerSchema { found } = entry.status {
                return Err(SaveError::NewerSchema { found });
            }
            entry.file.clone()
        }
        None => first_free_path(dir, &slug(trimmed)),
    };

    let session = SessionFileV1 {
        schema_version: CURRENT_SCHEMA_VERSION,
        windows: vec![window],
        // ADR-0054 §4: gravado com o valor do processo, ignorado na
        // leitura -- a dispensa do convite de integração de shell é do
        // processo, nunca da disposição salva.
        shell_integration_dismissed: false,
        name: Some(trimmed.to_string()),
        saved_at: Some(unix_now_secs()),
    };

    crate::save_to(&target_path, &session).map_err(SaveError::Io)?;
    Ok(target_path)
}

/// [`save_named_in`] resolvendo o diretório por [`named_sessions_dir`].
pub fn save_named(name: &str, window: WindowV1) -> Result<PathBuf, SaveError> {
    let dir = named_sessions_dir().ok_or_else(|| {
        SaveError::Io(io::Error::other(
            "sem diretório de sessões nomeadas resolvível",
        ))
    })?;
    save_named_in(&dir, name, window)
}

/// Primeiro caminho livre para `base_slug` em `dir`: `<slug>.json`, senão
/// `<slug>-2.json`, `<slug>-3.json`... -- a mesma regra do `.corrupt.N` do
/// ADR-0036 §5, aplicada a colisão de *slug* entre nomes diferentes.
fn first_free_path(dir: &Path, base_slug: &str) -> PathBuf {
    let candidate = dir.join(format!("{base_slug}.json"));
    if !candidate.exists() {
        return candidate;
    }
    let mut suffix = 2;
    loop {
        let candidate = dir.join(format!("{base_slug}-{suffix}.json"));
        if !candidate.exists() {
            return candidate;
        }
        suffix += 1;
    }
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Remove um arquivo de sessão nomeada (RF-14.16).
pub fn delete_named(file: &Path) -> io::Result<()> {
    fs::remove_file(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::v1::{GeometryV1, GroupV1, TabV1};

    fn window() -> WindowV1 {
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
                panes: None,
            }],
            active_tab: Some(0),
            theme: None,
            zoom_steps: 0,
        }
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("cria diretório temporário de teste")
    }

    #[test]
    fn round_trip_preserves_window_name_and_saved_at() {
        let dir = tempdir();
        let path = save_named_in(dir.path(), "api + front", window()).unwrap();

        let outcome = load_named(&path);
        let session = outcome.session.expect("sessão nomeada carrega");
        assert_eq!(session.name.as_deref(), Some("api + front"));
        assert!(session.saved_at.is_some());
        assert_eq!(session.windows, vec![window()]);
    }

    #[test]
    fn slug_removes_accents_slashes_and_spaces() {
        assert_eq!(slug("API/front"), "api-front");
        assert_eq!(slug("api front"), "api-front");
        assert_eq!(
            slug("Estudo Rust: à moda antiga"),
            "estudo-rust-a-moda-antiga"
        );
    }

    #[test]
    fn slug_of_only_symbols_falls_back_to_sessao() {
        assert_eq!(slug("!!!???"), "sessao");
        assert_eq!(slug(""), "sessao");
        assert_eq!(slug("   "), "sessao");
    }

    #[test]
    fn slug_truncates_at_48_bytes_without_breaking_mid_character() {
        let long = "a".repeat(100);
        let truncated = slug(&long);
        assert_eq!(truncated.len(), 48);
        assert_eq!(truncated, "a".repeat(48));
    }

    #[test]
    fn slug_appends_suffix_for_windows_reserved_names() {
        assert_eq!(slug("CON"), "con-2");
        assert_eq!(slug("nul"), "nul-2");
        assert_eq!(slug("Com1"), "com1-2");
        assert_eq!(slug("lpt9"), "lpt9-2");
        // Não reservado: nada de sufixo.
        assert_eq!(slug("console"), "console");
    }

    #[test]
    fn slug_collision_between_different_names_creates_distinct_files() {
        let dir = tempdir();
        let a = save_named_in(dir.path(), "API/front", window()).unwrap();
        let b = save_named_in(dir.path(), "api front", window()).unwrap();
        assert_ne!(a, b);
        assert!(a.exists());
        assert!(b.exists());
        assert_eq!(list_named_in(dir.path()).len(), 2);
    }

    #[test]
    fn saving_the_same_normalized_name_overwrites_the_same_file() {
        let dir = tempdir();
        let first = save_named_in(dir.path(), "infra", window()).unwrap();
        let second = save_named_in(dir.path(), "Infra", window()).unwrap();
        assert_eq!(first, second);
        assert_eq!(list_named_in(dir.path()).len(), 1);

        let session = load_named(&second).session.unwrap();
        assert_eq!(session.name.as_deref(), Some("Infra"));
    }

    #[test]
    fn list_orders_by_saved_at_descending() {
        let dir = tempdir();
        let newer = SessionFileV1 {
            schema_version: CURRENT_SCHEMA_VERSION,
            windows: vec![window()],
            shell_integration_dismissed: false,
            name: Some("newer".to_string()),
            saved_at: Some(200),
        };
        crate::save_to(&dir.path().join("newer.json"), &newer).unwrap();

        let mut older = newer.clone();
        older.name = Some("older".to_string());
        older.saved_at = Some(100);
        crate::save_to(&dir.path().join("older.json"), &older).unwrap();

        let entries = list_named_in(dir.path());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "newer");
        assert_eq!(entries[1].name, "older");
    }

    #[test]
    fn entries_without_saved_at_sort_last() {
        let dir = tempdir();
        let with_time = SessionFileV1 {
            schema_version: CURRENT_SCHEMA_VERSION,
            windows: vec![window()],
            shell_integration_dismissed: false,
            name: Some("com-data".to_string()),
            saved_at: Some(50),
        };
        crate::save_to(&dir.path().join("com-data.json"), &with_time).unwrap();

        let mut without_time = with_time.clone();
        without_time.name = Some("sem-data".to_string());
        without_time.saved_at = None;
        crate::save_to(&dir.path().join("sem-data.json"), &without_time).unwrap();

        let entries = list_named_in(dir.path());
        assert_eq!(entries[0].name, "com-data");
        assert_eq!(entries[1].name, "sem-data");
    }

    #[test]
    fn corrupt_file_appears_as_unreadable_and_stays_on_disk_after_listing() {
        let dir = tempdir();
        let path = dir.path().join("quebrado.json");
        fs::write(&path, b"nao eh json valido").unwrap();

        let entries = list_named_in(dir.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, EntryStatus::Unreadable);
        assert_eq!(entries[0].name, "quebrado");
        assert!(path.exists(), "listar nunca move nem renomeia o arquivo");
        assert_eq!(fs::read(&path).unwrap(), b"nao eh json valido");
    }

    #[test]
    fn newer_schema_appears_in_list_and_save_refuses_to_overwrite() {
        let dir = tempdir();
        let path = dir.path().join("futuro.json");
        let original = br#"{"schema_version":99,"windows":[{}],"name":"futuro"}"#;
        fs::write(&path, original).unwrap();

        let entries = list_named_in(dir.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, EntryStatus::NewerSchema { found: 99 });

        let result = save_named_in(dir.path(), "futuro", window());
        assert!(matches!(result, Err(SaveError::NewerSchema { found: 99 })));
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn delete_named_removes_the_file() {
        let dir = tempdir();
        let path = save_named_in(dir.path(), "descartavel", window()).unwrap();
        assert!(path.exists());
        delete_named(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn list_named_in_is_empty_for_a_missing_directory() {
        let dir = tempdir();
        let missing = dir.path().join("does-not-exist");
        assert_eq!(list_named_in(&missing), Vec::new());
    }

    #[test]
    fn save_named_in_rejects_a_blank_name() {
        let dir = tempdir();
        let result = save_named_in(dir.path(), "   ", window());
        assert!(matches!(result, Err(SaveError::EmptyName)));
        assert!(list_named_in(dir.path()).is_empty());
    }

    #[test]
    fn save_named_in_creates_the_directory_when_missing() {
        let dir = tempdir();
        let nested = dir.path().join("sessions");
        assert!(!nested.exists());
        let path = save_named_in(&nested, "nova", window()).unwrap();
        assert!(path.exists());
    }
}
