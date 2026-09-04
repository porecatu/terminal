// SPDX-License-Identifier: GPL-3.0-or-later

//! Captura de OSC 7 (ADR-0017 item 1). O `osc_dispatch` embutido no crate
//! `vte` (que `alacritty_terminal::vte` reexporta) descarta OSC 7 antes de
//! chamar qualquer método do `Handler` do motor -- não existe gancho para
//! interceptar essa sequência de dentro do `Term`. Este módulo roda um
//! segundo parser `vte`, independente e sem efeito colateral no motor,
//! sobre os mesmos bytes que `TermEngine::advance` já processa, só para
//! observar essa sequência específica.

use std::path::PathBuf;

use alacritty_terminal::vte::{Parser, Perform};

#[derive(Default)]
struct Sink {
    cwd: Option<PathBuf>,
}

impl Perform for Sink {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.first() != Some(&b"7".as_slice()) {
            return;
        }
        if let Some(uri) = params.get(1) {
            self.cwd = parse_file_uri(uri);
        }
    }
}

/// Observa bytes crus do PTY e devolve o `cwd` mais recente capturado por
/// OSC 7 (`file://[host]/caminho`), se algum tiver chegado.
pub struct Osc7Watcher {
    parser: Parser,
    sink: Sink,
}

impl Osc7Watcher {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
            sink: Sink::default(),
        }
    }

    /// Processa `bytes` e devolve o `cwd` capturado neste lote, se houver.
    pub fn advance(&mut self, bytes: &[u8]) -> Option<PathBuf> {
        self.parser.advance(&mut self.sink, bytes);
        self.sink.cwd.take()
    }
}

impl Default for Osc7Watcher {
    fn default() -> Self {
        Self::new()
    }
}

/// `file://host/caminho` ou `file:///caminho` (sem host) -> caminho local,
/// percent-decodificado. `host` é descartado -- OSC 7 do shell local sempre
/// aponta pro próprio host, e nada aqui valida isso.
///
/// `pub` e reexportado (`crate::lib`) porque o OSC 8 do ADR-0042 precisa da
/// mesma conversão para revelar um `file://` -- inclusive a correção da
/// letra de unidade do Windows abaixo, que só existe aqui uma vez.
pub fn parse_file_uri(bytes: &[u8]) -> Option<PathBuf> {
    let s = std::str::from_utf8(bytes).ok()?;
    let rest = s.strip_prefix("file://")?;
    let path = rest.find('/').map(|idx| &rest[idx..])?;
    let path = strip_windows_drive_leading_slash(path);
    let decoded = percent_decode(path);
    if decoded.is_empty() {
        None
    } else {
        Some(PathBuf::from(decoded))
    }
}

/// Bug latente da F2 (achado ao escrever os snippets do ADR-0039):
/// `file:///C:/Users/ana` chega aqui como `/C:/Users/ana` -- a barra que
/// sobrou do corte do host, antes da letra de unidade, que não é caminho
/// válido no Windows (`std::path::Path` não reconhece `\C:\...` como
/// absoluto). RFC 8089 trata `/<letra>:/...` como o caso especial do
/// caminho local do Windows; corta a barra inicial só quando o padrão
/// bate -- `/home/user` (Unix) não bate e sai intocado.
fn strip_windows_drive_leading_slash(path: &str) -> &str {
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        &path[1..]
    } else {
        path
    }
}

/// Decodificador `%XX` mínimo -- sem depender de um crate novo só para
/// isto (`percent-encoding` exigiria ADR de dependência).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn osc7(cwd: &str) -> Vec<u8> {
        let mut bytes = b"\x1b]7;".to_vec();
        bytes.extend_from_slice(cwd.as_bytes());
        bytes.push(0x07);
        bytes
    }

    #[test]
    fn captures_path_without_host() {
        let mut w = Osc7Watcher::new();
        let cwd = w.advance(&osc7("file:///home/user/projeto"));
        assert_eq!(cwd, Some(PathBuf::from("/home/user/projeto")));
    }

    #[test]
    fn captures_path_with_host_and_discards_it() {
        let mut w = Osc7Watcher::new();
        let cwd = w.advance(&osc7("file://myhost/home/user/projeto"));
        assert_eq!(cwd, Some(PathBuf::from("/home/user/projeto")));
    }

    #[test]
    fn percent_decodes_path() {
        let mut w = Osc7Watcher::new();
        let cwd = w.advance(&osc7("file:///home/user/um%20diret%C3%B3rio"));
        assert_eq!(cwd, Some(PathBuf::from("/home/user/um diretório")));
    }

    #[test]
    fn ignores_other_osc_sequences() {
        let mut w = Osc7Watcher::new();
        let cwd = w.advance(b"\x1b]0;titulo\x07");
        assert_eq!(cwd, None);
    }

    #[test]
    fn take_clears_after_read() {
        let mut w = Osc7Watcher::new();
        w.advance(&osc7("file:///a"));
        assert_eq!(w.advance(b""), None);
    }

    #[test]
    fn malformed_uri_without_slash_is_ignored() {
        let mut w = Osc7Watcher::new();
        let cwd = w.advance(&osc7("file://semslashnenhum"));
        assert_eq!(cwd, None);
    }

    /// Bug latente da F2, corrigido na F5 etapa 2: sem a barra a mais
    /// antes da letra de unidade, `/C:/Users/ana` não é caminho válido
    /// no Windows.
    #[test]
    fn windows_drive_letter_uri_drops_the_extra_leading_slash() {
        let mut w = Osc7Watcher::new();
        let cwd = w.advance(&osc7("file:///C:/Users/ana"));
        assert_eq!(cwd, Some(PathBuf::from("C:/Users/ana")));
    }

    /// Letra de unidade minúscula (`cmd.exe`/PowerShell podem emitir
    /// qualquer caixa) também bate no padrão.
    #[test]
    fn windows_drive_letter_uri_matches_lowercase_letter_too() {
        let mut w = Osc7Watcher::new();
        let cwd = w.advance(&osc7("file:///d:/projetos/porecatu"));
        assert_eq!(cwd, Some(PathBuf::from("d:/projetos/porecatu")));
    }
}
