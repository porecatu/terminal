// SPDX-License-Identifier: GPL-3.0-or-later

//! Registro de pânico em arquivo.
//!
//! O binário é `windows_subsystem = "windows"`: lançado pelo atalho, ele não
//! tem console, e a mensagem que o hook padrão escreve em `stderr` some junto
//! com o processo -- o app só "fecha sozinho". O hook daqui acrescenta a
//! mensagem, o local e o backtrace a `crash.log`, no diretório do arquivo de
//! sessão (o mesmo que `PORECATU_SESSION` desloca), e depois chama o hook
//! padrão, que segue escrevendo em `stderr` como sempre.
//!
//! Erro de validação do `wgpu` sem tratador próprio também vira pânico, e por
//! isso também cai aqui. Texto em inglês fixo, de desenvolvedor: não é
//! interface (ADR-0056), como a saída de `PORECATU_TRACE`.

use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::panic::{self, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Nome do arquivo, ao lado do `session.json`.
const FILE_NAME: &str = "crash.log";

/// Instala o hook. Sem diretório de sessão resolvível, fica o hook padrão.
pub(crate) fn install() {
    let Some(path) = log_path() else {
        return;
    };
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let location = info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()));
        let backtrace = Backtrace::force_capture().to_string();
        let text = entry(
            seconds,
            &payload_message(info),
            location.as_deref(),
            &backtrace,
        );
        // Falhar ao gravar o registro não pode virar um segundo pânico dentro
        // do hook: o padrão ainda escreve em `stderr`.
        let _ = append(&path, &text);
        default_hook(info);
    }));
}

fn log_path() -> Option<PathBuf> {
    let session = porecatu_session::path::resolve_session_path()?;
    Some(session.parent()?.join(FILE_NAME))
}

fn append(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(text.as_bytes())
}

/// A mensagem do pânico: `&str` (`panic!("literal")`) ou `String`
/// (`panic!("{x}")`, `.expect(...)`); outro tipo não tem texto.
fn payload_message(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "<non-string panic payload>".to_owned()
    }
}

/// Uma entrada do registro, pura para ser testável sem pânico de verdade.
/// `unix_seconds` em vez de data formatada: sem dependência de calendário, e
/// a ordem das entradas já diz o que veio antes.
fn entry(unix_seconds: u64, message: &str, location: Option<&str>, backtrace: &str) -> String {
    format!(
        "=== porecatu {version} panic at unix {unix_seconds} ===\n\
         message: {message}\n\
         location: {location}\n\
         backtrace:\n{backtrace}\n\n",
        version = env!("CARGO_PKG_VERSION"),
        location = location.unwrap_or("<unknown>"),
        backtrace = backtrace.trim_end(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_carries_the_message_the_location_and_the_backtrace() {
        let text = entry(
            1_791_227_876,
            "wgpu error: Validation Error",
            Some("crates/porecatu-render/src/quad.rs:42"),
            "   0: frame\n",
        );
        assert!(text.contains("panic at unix 1791227876"));
        assert!(text.contains("message: wgpu error: Validation Error\n"));
        assert!(text.contains("location: crates/porecatu-render/src/quad.rs:42\n"));
        assert!(text.contains("backtrace:\n   0: frame\n"));
        assert!(
            text.ends_with("\n\n"),
            "entradas separadas por linha em branco"
        );
    }

    #[test]
    fn an_entry_without_a_location_says_so() {
        assert!(entry(0, "x", None, "").contains("location: <unknown>\n"));
    }

    #[test]
    fn appending_twice_keeps_both_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(FILE_NAME);
        append(&path, &entry(1, "first", None, "")).unwrap();
        append(&path, &entry(2, "second", None, "")).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        let first = written.find("message: first").unwrap();
        let second = written.find("message: second").unwrap();
        assert!(first < second);
    }
}
