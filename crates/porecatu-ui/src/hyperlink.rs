// SPDX-License-Identifier: GPL-3.0-or-later

//! Política de esquema do ADR-0042 §4/§5 e abertura via `opener` (§6/§7):
//! `porecatu-term` só reporta span e URI (fronteira do §7, mesma que já
//! vale para cor não resolvida) -- quem decide o que um esquema significa
//! é este módulo, dentro de `porecatu-ui`.
//!
//! [`classify_scheme`] é pura de propósito: normaliza (minúsculas,
//! %-decode, sem espaço nas pontas) e compara contra a allowlist fechada
//! **antes** de qualquer I/O, para poder testar a normalização sem
//! spawnar processo nenhum -- é o teste que a mitigação do risco "esquema
//! perigoso passar por normalização" do ADR pede.
//!
//! [`open_hyperlink`] é o único ponto que chama `opener`: o URI vai como
//! **argumento** (`open`/`reveal` recebem `&str`/`&Path`, nunca uma string
//! montada por `format!`/concatenação) -- nunca há shell no meio, então
//! não há injeção possível por metacaractere no conteúdo do URI.

use std::path::PathBuf;

use porecatu_term::{GridSnapshot, HyperlinkSpan};

/// Todos os spans do snapshot que compartilham o id do span sob `(row,
/// col)` -- é o conjunto que a affordance sublinha junto (RF-11.11: "todos
/// os trechos do mesmo id na vista"). Vazio se a célula não tem link.
pub fn spans_sharing_id_at(snapshot: &GridSnapshot, row: usize, col: usize) -> Vec<HyperlinkSpan> {
    let Some(&target) = snapshot
        .hyperlink_spans
        .iter()
        .find(|s| s.row == row && col >= s.start_col && col <= s.end_col)
    else {
        return Vec::new();
    };
    let id = &snapshot.hyperlinks[target.id_start as usize..target.id_end as usize];
    snapshot
        .hyperlink_spans
        .iter()
        .filter(|s| &snapshot.hyperlinks[s.id_start as usize..s.id_end as usize] == id)
        .copied()
        .collect()
}

/// Uri do hyperlink em `(row, col)`, se houver -- clique com modificador
/// (RF-11.12) e itens "abrir link"/"copiar link" do menu (RF-11.14) usam
/// o mesmo lookup.
pub fn uri_at(snapshot: &GridSnapshot, row: usize, col: usize) -> Option<&str> {
    snapshot
        .hyperlink_spans
        .iter()
        .find(|s| s.row == row && col >= s.start_col && col <= s.end_col)
        .map(|s| &snapshot.hyperlinks[s.uri_start as usize..s.uri_end as usize])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkPolicy {
    /// `http`, `https`, `mailto` -- handler padrão do sistema.
    Open,
    /// `file` -- revelado no gerenciador de arquivos, nunca handler por
    /// extensão (ADR-0042 §4: é o ponto inteiro da decisão).
    Reveal,
    /// Esquema fora da lista -- nunca abre.
    Refuse,
}

/// Resultado de agir sobre um hyperlink (RF-11.12/RF-11.13): o que
/// aconteceu, para `lib.rs` decidir o que informar ao usuário.
#[derive(Debug, PartialEq, Eq)]
pub enum LinkOutcome {
    /// Aberto no handler padrão do sistema.
    Opened,
    /// Revelado no gerenciador de arquivos.
    Revealed,
    /// `file://` sem caminho reconhecível, ou o gerenciador de arquivos
    /// não respondeu (ADR-0042, risco "revelar não suportado em algum
    /// ambiente Linux") -- cai na mesma vala do esquema recusado: copia e
    /// informa, nunca no handler por extensão como fallback.
    RevealFailed,
    /// `opener::open` falhou (sem handler registrado, por exemplo).
    OpenFailed,
    /// Esquema fora da allowlist -- URI copiado, esquema normalizado
    /// devolvido para a mensagem ao usuário.
    Refused { normalized_scheme: String },
}

/// Extrai e normaliza o esquema (minúsculas, `%`-decode, sem espaço nas
/// pontas) antes de comparar contra a allowlist fechada -- ADR-0042 §4/§5.
/// Sem `:`, ou esquema que não decodifica como UTF-8 válido: recusado, o
/// default de qualquer coisa não reconhecida.
fn classify_scheme(uri: &str) -> LinkPolicy {
    match normalized_scheme(uri) {
        Some(scheme) => match scheme.as_str() {
            "http" | "https" | "mailto" => LinkPolicy::Open,
            "file" => LinkPolicy::Reveal,
            _ => LinkPolicy::Refuse,
        },
        None => LinkPolicy::Refuse,
    }
}

fn normalized_scheme(uri: &str) -> Option<String> {
    let colon = uri.find(':')?;
    let decoded = percent_decode(uri[..colon].trim())?;
    Some(decoded.to_ascii_lowercase())
}

/// Decodificador `%XX` mínimo, mesmo padrão de `porecatu_term::osc7` (sem
/// puxar um crate novo só para isto) -- percent-encoding no esquema é
/// exatamente o truque que a mitigação do risco do ADR cobre.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Age sobre `uri` conforme a política do esquema (RF-11.12/RF-11.13).
/// Chamado só sob o modificador de abertura (ADR-0042 §2/§3) -- clique sem
/// ele nunca chega aqui.
pub fn open_hyperlink(uri: &str) -> LinkOutcome {
    match classify_scheme(uri) {
        LinkPolicy::Open => match opener::open(uri) {
            Ok(()) => LinkOutcome::Opened,
            Err(_) => LinkOutcome::OpenFailed,
        },
        LinkPolicy::Reveal => match file_uri_to_path(uri) {
            Some(path) => match opener::reveal(&path) {
                Ok(()) => LinkOutcome::Revealed,
                Err(_) => LinkOutcome::RevealFailed,
            },
            None => LinkOutcome::RevealFailed,
        },
        LinkPolicy::Refuse => LinkOutcome::Refused {
            normalized_scheme: normalized_scheme(uri).unwrap_or_default(),
        },
    }
}

/// `file://...` -> caminho local. Reusa `porecatu_term::parse_file_uri`
/// (a mesma conversão do OSC 7, letra de unidade do Windows inclusa) em
/// vez de duplicá-la -- é a exata classe de bug que o comentário dela
/// documenta.
fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    porecatu_term::parse_file_uri(uri.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_https_mailto_open() {
        assert_eq!(classify_scheme("http://example.com"), LinkPolicy::Open);
        assert_eq!(classify_scheme("https://example.com"), LinkPolicy::Open);
        assert_eq!(classify_scheme("mailto:a@b.com"), LinkPolicy::Open);
    }

    #[test]
    fn file_is_reveal_not_open() {
        assert_eq!(
            classify_scheme("file:///C:/Windows/System32/calc.exe"),
            LinkPolicy::Reveal
        );
    }

    #[test]
    fn unknown_scheme_is_refused() {
        assert_eq!(classify_scheme("javascript:alert(1)"), LinkPolicy::Refuse);
        assert_eq!(classify_scheme("ms-msdt:whatever"), LinkPolicy::Refuse);
    }

    #[test]
    fn sem_dois_pontos_e_recusado() {
        assert_eq!(classify_scheme("nao-e-uri"), LinkPolicy::Refuse);
    }

    /// Mitigação do risco do ADR: maiúsculas não escapam da allowlist.
    #[test]
    fn normaliza_maiusculas_antes_de_comparar() {
        assert_eq!(classify_scheme("HTTPS://example.com"), LinkPolicy::Open);
        assert_eq!(classify_scheme("FiLe:///tmp/a"), LinkPolicy::Reveal);
    }

    /// Mitigação do risco do ADR: `%`-encoding no esquema não escapa da
    /// allowlist -- `%68%74%74%70` decodifica para `http`.
    #[test]
    fn normaliza_percent_encoding_antes_de_comparar() {
        assert_eq!(
            classify_scheme("%68%74%74%70://example.com"),
            LinkPolicy::Open
        );
    }

    /// Mitigação do risco do ADR: espaço nas pontas do esquema não escapa
    /// da allowlist.
    #[test]
    fn normaliza_espaco_antes_de_comparar() {
        assert_eq!(classify_scheme(" http://example.com"), LinkPolicy::Open);
    }

    /// O teste que o ADR pede: um URI com metacaractere de shell prova
    /// que nada é interpretado -- `classify_scheme` só olha o prefixo até
    /// o primeiro `:`, e o restante do URI atravessa intocado até
    /// `opener::open`, que recebe `uri` como argumento (nunca uma string
    /// montada por `format!`/concatenação que um shell reinterpretaria).
    #[test]
    fn metacaractere_de_shell_no_uri_nao_afeta_a_classificacao() {
        let uri = "http://example.com/$(rm -rf ~); echo pwned";
        assert_eq!(classify_scheme(uri), LinkPolicy::Open);
    }

    #[test]
    fn file_uri_sem_caminho_reconhecivel_falha_a_revelar() {
        assert!(file_uri_to_path("file://").is_none());
    }

    #[test]
    fn file_uri_com_letra_de_unidade_do_windows_vira_caminho_valido() {
        let path = file_uri_to_path("file:///C:/Users/ana/relatorio.pdf").unwrap();
        assert_eq!(path, PathBuf::from("C:/Users/ana/relatorio.pdf"));
    }

    fn snapshot_with_split_link() -> GridSnapshot {
        let mut snap = GridSnapshot::default();
        let uri = "http://example.com";
        let id = "abc";
        snap.hyperlinks.push_str(id);
        snap.hyperlinks.push_str(uri);
        snap.hyperlink_spans.push(HyperlinkSpan {
            row: 0,
            start_col: 0,
            end_col: 2,
            id_start: 0,
            id_end: id.len() as u32,
            uri_start: id.len() as u32,
            uri_end: (id.len() + uri.len()) as u32,
        });
        snap.hyperlink_spans.push(HyperlinkSpan {
            row: 1,
            start_col: 0,
            end_col: 2,
            id_start: 0,
            id_end: id.len() as u32,
            uri_start: id.len() as u32,
            uri_end: (id.len() + uri.len()) as u32,
        });
        snap
    }

    #[test]
    fn spans_sharing_id_at_encontra_os_dois_trechos_partidos() {
        let snap = snapshot_with_split_link();
        let spans = spans_sharing_id_at(&snap, 0, 1);
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn spans_sharing_id_at_vazio_fora_de_qualquer_link() {
        let snap = snapshot_with_split_link();
        assert!(spans_sharing_id_at(&snap, 0, 5).is_empty());
    }

    #[test]
    fn uri_at_le_a_uri_do_span_sob_a_celula() {
        let snap = snapshot_with_split_link();
        assert_eq!(uri_at(&snap, 1, 0), Some("http://example.com"));
        assert_eq!(uri_at(&snap, 5, 5), None);
    }
}
