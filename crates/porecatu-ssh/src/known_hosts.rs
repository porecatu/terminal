// SPDX-License-Identifier: GPL-3.0-or-later

//! `~/.ssh/known_hosts` do OpenSSH (ADR-0074): quais arquivos, como ler, o
//! casamento, a impressão digital e a gravação.
//!
//! **O arquivo é do OpenSSH.** Aqui só se lê, e só se acrescenta uma linha
//! quando o usuário aceita um host desconhecido -- nunca se reescreve, remove
//! ou renomeia nada.
//!
//! O leitor de linha é o do `ssh-key` (pela re-exportação do `russh`); o
//! casamento de nome é código nosso, porque nenhuma biblioteca da árvore trata
//! curinga, negação, `@revoked` e `@cert-authority` do jeito do OpenSSH.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use hmac::{Hmac, KeyInit, Mac};
use russh::keys::ssh_key::encoding::base64::{Base64, Encoding};
use russh::keys::ssh_key::known_hosts::{Entry, HostPatterns, Marker};
use russh::keys::{Algorithm, HashAlg, PublicKey};
use sha1::Sha1;
use subtle::ConstantTimeEq;

#[cfg(test)]
mod tests;

/// Porta em que o nome procurado é só o host, sem colchetes.
const DEFAULT_PORT: u16 = 22;

/// Tamanho do sal de uma entrada em hash, como o OpenSSH grava (o tamanho do
/// SHA-1).
const SALT_LEN: usize = 20;

// ---------------------------------------------------------------------------
// Arquivos
// ---------------------------------------------------------------------------

/// Quais arquivos entram na verificação (ADR-0074 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownHostsFiles {
    /// O `known_hosts` do usuário: lido e **o único gravado**. `None` quando
    /// o diretório do usuário não é conhecido.
    pub user: Option<PathBuf>,
    /// Lidos, nunca gravados: `known_hosts2` do usuário e os dois globais.
    pub read_only: Vec<PathBuf>,
}

impl KnownHostsFiles {
    /// A lista default do OpenSSH (`UserKnownHostsFile` e
    /// `GlobalKnownHostsFile`). `~` é `dirs::home_dir()`.
    pub fn standard() -> Self {
        let dot_ssh = dirs::home_dir().map(|home| home.join(".ssh"));
        let mut read_only = Vec::new();
        if let Some(dot_ssh) = &dot_ssh {
            read_only.push(dot_ssh.join("known_hosts2"));
        }
        read_only.extend(global_files());
        Self {
            user: dot_ssh.map(|dir| dir.join("known_hosts")),
            read_only,
        }
    }

    /// Ordem de leitura: o do usuário primeiro, depois os outros.
    fn read_order(&self) -> impl Iterator<Item = &PathBuf> {
        self.user.iter().chain(self.read_only.iter())
    }
}

#[cfg(windows)]
fn global_files() -> Vec<PathBuf> {
    match std::env::var_os("ProgramData") {
        Some(base) => {
            let dir = PathBuf::from(base).join("ssh");
            vec![dir.join("ssh_known_hosts"), dir.join("ssh_known_hosts2")]
        }
        None => Vec::new(),
    }
}

#[cfg(not(windows))]
fn global_files() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/etc/ssh/ssh_known_hosts"),
        PathBuf::from("/etc/ssh/ssh_known_hosts2"),
    ]
}

/// Por que um arquivo que existe não pôde ser lido. Enum, sem prosa: a frase
/// é de `porecatu-ui`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCause {
    PermissionDenied,
    /// Não é texto UTF-8: um arquivo regravado em UTF-16 pelo PowerShell 5,
    /// por exemplo.
    NotText,
    Other,
}

/// Um arquivo que existe e não pôde ser lido (ADR-0074 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    pub file: PathBuf,
    pub cause: ReadCause,
}

/// O texto de um arquivo lido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileText {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
}

/// Lê todos os arquivos, a cada conexão e sem cache. **Ausente vale como
/// vazio; existente e ilegível encerra**: tratá-lo como vazio transformaria
/// host conhecido, ou com chave alterada, em "desconhecido", e o usuário
/// aceitaria uma chave por cima de uma verificação que não aconteceu.
pub(crate) fn read_files(files: &KnownHostsFiles) -> Result<Vec<FileText>, Unreadable> {
    let mut texts = Vec::new();
    for path in files.read_order() {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(Unreadable {
                    file: path.clone(),
                    cause: if err.kind() == io::ErrorKind::PermissionDenied {
                        ReadCause::PermissionDenied
                    } else {
                        ReadCause::Other
                    },
                });
            }
        };
        // UTF-16 de texto ASCII é UTF-8 válido, cheio de NUL: o NUL não existe
        // num `known_hosts` de verdade.
        let text = match String::from_utf8(bytes) {
            Ok(text) if !text.contains('\0') => text,
            _ => {
                return Err(Unreadable {
                    file: path.clone(),
                    cause: ReadCause::NotText,
                });
            }
        };
        texts.push(FileText {
            path: path.clone(),
            text,
        });
    }
    Ok(texts)
}

// ---------------------------------------------------------------------------
// Resultado da verificação
// ---------------------------------------------------------------------------

/// Uma entrada do arquivo, para a nota e para o diálogo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownEntry {
    pub file: PathBuf,
    /// Número da linha no arquivo, a partir de 1.
    pub line: usize,
    /// `SHA256:` e o hash em base64 sem preenchimento.
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    /// Alguma linha do host casa com a chave apresentada.
    Known,
    /// Nenhuma linha casa com este tipo. `other_types` são os tipos que o host
    /// já tem registrados (§3), em ordem de aparição, sem repetição.
    Unknown { other_types: Vec<String> },
    /// Há linhas do mesmo tipo e nenhuma casa.
    Changed { expected: Vec<KnownEntry> },
    /// `@revoked` casa com a chave apresentada.
    Revoked { entry: KnownEntry },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyCheck {
    pub status: HostKeyStatus,
    /// Linhas que o leitor não entendeu. Uma `@revoked` ilegível não pode
    /// desaparecer em silêncio.
    pub skipped_lines: Vec<(PathBuf, usize)>,
}

// ---------------------------------------------------------------------------
// Casamento
// ---------------------------------------------------------------------------

/// O nome procurado: `host` em minúsculas se a porta é 22, `[host]:porta`
/// caso contrário -- o formato do OpenSSH. Só o nome do perfil, nunca o IP
/// resolvido (`CheckHostIP no`).
pub(crate) fn lookup_name(host: &str, port: u16) -> String {
    let host = host.to_lowercase();
    if port == DEFAULT_PORT {
        host
    } else {
        format!("[{host}]:{port}")
    }
}

/// `*` e `?` como curinga, sem distinção de caixa nos dois lados -- o que o
/// `ssh-keygen -F` faz.
fn glob_match(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let (mut p, mut n) = (0, 0);
    // Onde o último `*` parou, para voltar quando o resto não casa.
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((star_p, star_n)) = star {
            p = star_p + 1;
            n = star_n + 1;
            star = Some((star_p, star_n + 1));
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

/// `HMAC-SHA1(chave = sal, mensagem = nome)`.
fn hash_name(salt: &[u8], name: &str) -> Option<[u8; 20]> {
    let mut mac = Hmac::<Sha1>::new_from_slice(salt).ok()?;
    mac.update(name.as_bytes());
    let digest = mac.finalize().into_bytes();
    let mut out = [0u8; 20];
    out.copy_from_slice(&digest);
    Some(out)
}

/// Se a linha vale para o nome procurado.
///
/// Lista com `!padrão`: se o nome casa um padrão negado, **a linha inteira é
/// excluída**, mesmo que outro padrão dela case -- a semântica do OpenSSH.
/// Entrada em hash: comparação em tempo constante.
fn host_matches(patterns: &HostPatterns, lookup: &str) -> bool {
    match patterns {
        HostPatterns::HashedName { salt, hash } => {
            hash_name(salt, lookup).is_some_and(|computed| bool::from(computed.ct_eq(hash)))
        }
        HostPatterns::Patterns(list) => {
            let mut matched = false;
            for pattern in list {
                if let Some(negated) = pattern.strip_prefix('!') {
                    if glob_match(negated, lookup) {
                        return false;
                    }
                } else if glob_match(pattern, lookup) {
                    matched = true;
                }
            }
            matched
        }
    }
}

/// Impressão digital no formato de `ssh-keygen -lf`.
pub(crate) fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

/// Nome do algoritmo da chave que o servidor apresentou. Para RSA, o nome
/// negociado (`rsa-sha2-256`, `rsa-sha2-512`, ou `ssh-rsa`).
pub(crate) fn algorithm_name(key: &PublicKey, hash_alg: Option<HashAlg>) -> String {
    match (key.algorithm(), hash_alg) {
        (Algorithm::Rsa { .. }, Some(HashAlg::Sha256)) => "rsa-sha2-256".to_owned(),
        (Algorithm::Rsa { .. }, Some(HashAlg::Sha512)) => "rsa-sha2-512".to_owned(),
        (algorithm, _) => algorithm.as_str().to_owned(),
    }
}

/// Uma linha lida, com o lugar de onde veio.
struct Line {
    file: PathBuf,
    number: usize,
    entry: Entry,
}

/// Lê as linhas de um texto como o `ssh-key` lê (comentário até o fim da linha,
/// linha vazia ignorada), mas **com o número da linha**, e devolvendo as que
/// ele não entende em vez de derrubar o arquivo -- o OpenSSH também as pula.
fn parse_lines(file: &FileText, skipped: &mut Vec<(PathBuf, usize)>) -> Vec<Line> {
    let mut lines = Vec::new();
    for (index, raw) in file.text.lines().enumerate() {
        let mut line = raw;
        if let Some((before, _)) = line.split_once('#') {
            line = before;
        }
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        match line.parse::<Entry>() {
            Ok(entry) => lines.push(Line {
                file: file.path.clone(),
                number: index + 1,
                entry,
            }),
            Err(_) => skipped.push((file.path.clone(), index + 1)),
        }
    }
    lines
}

fn entry_of(line: &Line) -> KnownEntry {
    KnownEntry {
        file: line.file.clone(),
        line: line.number,
        fingerprint: fingerprint(line.entry.public_key()),
    }
}

/// A decisão do ADR-0074 §2, **nesta ordem**: `Revoked` vence tudo; `Known` se
/// alguma linha do host casa com a chave (mesmo havendo outras do mesmo tipo
/// com chave diferente: é a rotação); `Changed` só se há linhas do mesmo tipo e
/// nenhuma casa; `Unknown` no resto. `@cert-authority` não entra.
pub(crate) fn check(
    files: &[FileText],
    host: &str,
    port: u16,
    presented: &PublicKey,
) -> HostKeyCheck {
    let lookup = lookup_name(host, port);
    let mut skipped = Vec::new();
    let mut lines = Vec::new();
    for file in files {
        lines.extend(parse_lines(file, &mut skipped));
    }
    let for_host: Vec<&Line> = lines
        .iter()
        .filter(|line| host_matches(line.entry.host_patterns(), &lookup))
        .collect();

    let same_key = |line: &Line| line.entry.public_key().key_data() == presented.key_data();

    // 1. Revogada.
    if let Some(revoked) = for_host
        .iter()
        .find(|line| matches!(line.entry.marker(), Some(Marker::Revoked)) && same_key(line))
    {
        return HostKeyCheck {
            status: HostKeyStatus::Revoked {
                entry: entry_of(revoked),
            },
            skipped_lines: skipped,
        };
    }

    // O que sobra: linhas comuns (sem marcador) do host.
    let plain: Vec<&&Line> = for_host
        .iter()
        .filter(|line| line.entry.marker().is_none())
        .collect();

    // 2. Conhecida.
    if plain.iter().any(|line| same_key(line)) {
        return HostKeyCheck {
            status: HostKeyStatus::Known,
            skipped_lines: skipped,
        };
    }

    // 3. Alterada: linhas do mesmo tipo, nenhuma casando.
    let same_type: Vec<KnownEntry> = plain
        .iter()
        .filter(|line| line.entry.public_key().algorithm() == presented.algorithm())
        .map(|line| entry_of(line))
        .collect();
    if !same_type.is_empty() {
        return HostKeyCheck {
            status: HostKeyStatus::Changed {
                expected: same_type,
            },
            skipped_lines: skipped,
        };
    }

    // 4. Desconhecida, com os tipos que o host já tem.
    let mut other_types: Vec<String> = Vec::new();
    for line in &plain {
        let name = line.entry.public_key().algorithm().as_str().to_owned();
        if !other_types.contains(&name) {
            other_types.push(name);
        }
    }
    HostKeyCheck {
        status: HostKeyStatus::Unknown { other_types },
        skipped_lines: skipped,
    }
}

/// Os tipos de chave que o host já tem registrados (§3), para a negociação
/// oferecer primeiro o que já está no arquivo.
pub(crate) fn registered_algorithms(files: &[FileText], host: &str, port: u16) -> Vec<Algorithm> {
    let lookup = lookup_name(host, port);
    let mut skipped = Vec::new();
    let mut found: Vec<Algorithm> = Vec::new();
    for file in files {
        for line in parse_lines(file, &mut skipped) {
            if line.entry.marker().is_none() && host_matches(line.entry.host_patterns(), &lookup) {
                let algorithm = line.entry.public_key().algorithm();
                if !found.contains(&algorithm) {
                    found.push(algorithm);
                }
            }
        }
    }
    found
}

// ---------------------------------------------------------------------------
// Gravação
// ---------------------------------------------------------------------------

/// Por que a linha não foi gravada.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "the I/O error is for the tests and for debugging; production only needs to know nothing was written"
)]
pub(crate) enum WriteError {
    /// O nome do host ou a chave serializada trariam quebra de linha, espaço
    /// ou um caractere que mudaria o sentido da linha: **recusa**.
    Unsafe,
    Io(io::Error),
}

impl From<io::Error> for WriteError {
    fn from(err: io::Error) -> Self {
        WriteError::Io(err)
    }
}

/// Caracteres que o nome do host não pode ter numa linha de `known_hosts`
/// (ADR-0072 §6): espaço, controle, e os que viram padrão, lista, hash,
/// negação, curinga ou comentário.
fn host_is_writable(host: &str) -> bool {
    !host.is_empty()
        && !host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || ",|[]@#*?!".contains(c))
}

/// `algoritmo base64`, sem comentário, ou `None` se a chave serializada traria
/// algo além disso.
fn key_field(key: &PublicKey) -> Option<String> {
    let openssh = key.to_openssh().ok()?;
    if openssh.contains(['\r', '\n']) {
        return None;
    }
    let mut parts = openssh.split(' ');
    let algorithm = parts.next()?;
    let blob = parts.next()?;
    // Sem comentário: a chave que o servidor apresenta não tem, e um espaço a
    // mais abriria campo novo.
    if parts.next().is_some() || algorithm.is_empty() || blob.is_empty() {
        return None;
    }
    Some(format!("{algorithm} {blob}"))
}

fn random_salt() -> Option<[u8; SALT_LEN]> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut salt = [0u8; SALT_LEN];
    SystemRandom::new().fill(&mut salt).ok()?;
    Some(salt)
}

/// A linha a acrescentar, sem a quebra final. `hashed` põe o nome em hash
/// (`|1|sal|hash`).
fn entry_line(host: &str, port: u16, key: &PublicKey, hashed: bool) -> Result<String, WriteError> {
    if !host_is_writable(host) {
        return Err(WriteError::Unsafe);
    }
    let key = key_field(key).ok_or(WriteError::Unsafe)?;
    let name = lookup_name(host, port);
    let field = if hashed {
        let salt = random_salt().ok_or_else(|| io::Error::other("no random source"))?;
        let hash = hash_name(&salt, &name).ok_or(WriteError::Unsafe)?;
        format!(
            "|1|{}|{}",
            Base64::encode_string(&salt),
            Base64::encode_string(&hash)
        )
    } else {
        name
    };
    Ok(format!("{field} {key}"))
}

/// Se o arquivo já tem alguma linha em hash: quem o tem em hash ligou o
/// `HashKnownHosts` de propósito (ADR-0074 §5).
fn has_hashed_line(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("|1|")
            || line.split_once(' ').is_some_and(|(marker, rest)| {
                marker.starts_with('@') && rest.trim_start().starts_with("|1|")
            })
    })
}

/// Acrescenta **uma linha** ao `known_hosts` do usuário (ADR-0074 §5):
///
/// - em hash se o arquivo já tem alguma linha em hash;
/// - com a quebra de linha garantida antes, se o arquivo não termina em uma;
/// - numa única chamada de escrita, em modo de acréscimo -- o que o `ssh` faz,
///   e o que convive com um `ssh` gravando ao mesmo tempo. **Sem** gravação
///   atômica por renomeação: renomear por cima de um arquivo que outro
///   programa pode estar acrescentando perderia a linha dele;
/// - criando `~/.ssh` (`0700` no Unix) e o arquivo (`0600`) se faltarem.
///
/// Nunca remove, reescreve nem reordena.
pub(crate) fn append_entry(
    path: &Path,
    host: &str,
    port: u16,
    key: &PublicKey,
) -> Result<(), WriteError> {
    // Lê antes, só para decidir o formato e a quebra; um arquivo que não é
    // texto não chega aqui (a leitura da verificação já o recusou).
    let existing = match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| WriteError::Io(io::Error::from(io::ErrorKind::InvalidData)))?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err.into()),
    };
    let line = entry_line(host, port, key, has_hashed_line(&existing))?;

    if let Some(dir) = path.parent()
        && !dir.exists()
    {
        create_private_dir(dir)?;
    }

    let mut buffer = String::with_capacity(line.len() + 2);
    if !existing.is_empty() && !existing.ends_with('\n') {
        buffer.push('\n');
    }
    buffer.push_str(&line);
    buffer.push('\n');

    let mut file = open_append(path)?;
    file.write_all(buffer.as_bytes())?;
    Ok(())
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

#[cfg(unix)]
fn open_append(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_append(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new().append(true).create(true).open(path)
}
