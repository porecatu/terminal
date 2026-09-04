// SPDX-License-Identifier: GPL-3.0-or-later

//! Busca no scrollback (ADR-0041), encapsulando `term::search::RegexSearch`
//! do `alacritty_terminal`. Nenhum tipo do motor atravessa esta fronteira --
//! só os tipos próprios abaixo saem de `porecatu-term`.
//!
//! A busca é **incremental por lotes** (`SearchJob::step`), não um único
//! passo que varre a grade inteira: o item 5 da Etapa 1 mediu que uma busca
//! de uma vez só, num scrollback cheio (default de 10.000 linhas), já passa
//! do orçamento de um frame a 60fps -- 22ms/18.6ms em release, 125ms em
//! debug, inclusive quando a busca não acha ocorrência nenhuma e teria que
//! varrer a grade inteira mesmo assim (ver `tests/search_scrollback_cost.rs`
//! e o relato da Etapa 1). Por isso o lote é limitado por **linhas
//! varridas**, não por ocorrências achadas -- uma query sem ocorrência
//! nenhuma é exatamente o caso em que um lote por match não protegeria
//! nada.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Boundary, Column, Direction, Point};
use alacritty_terminal::term::search::{RegexIter, RegexSearch};
use alacritty_terminal::term::{Term, TermMode};

/// Modo de busca (RF-11.4). Literal é o default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Literal,
    Regex,
}

/// Padrão de regex que não compila (RF-11.4) -- erro devolvido, nunca
/// `panic` e nunca resultado vazio silencioso: quem chama precisa
/// distinguir "nenhuma ocorrência" de "padrão inválido".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidPattern(String);

impl InvalidPattern {
    /// Mensagem para exibição (RF-11.4: "a barra sinaliza o erro").
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InvalidPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for InvalidPattern {}

/// Posição absoluta na grade (ADR-0041 §4: "ocorrências como lista de
/// ranges de posição na grade"), independente da posição de rolagem
/// (`GridSnapshot::scroll_offset`). Linha negativa é histórico acima do
/// topo da tela ativa -- mesma convenção do motor, mas tipo próprio: nada
/// do `alacritty_terminal` atravessa esta fronteira.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GridPos {
    pub line: i32,
    pub column: usize,
}

/// Uma ocorrência de busca: range inclusivo de posições na grade. Não é bit
/// de `CellFlags` -- ver a rejeição dessa alternativa no ADR-0041 §4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    pub start: GridPos,
    pub end: GridPos,
}

/// Progresso de uma chamada a [`SearchJob::step`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchStep {
    /// Ainda há grade por varrer -- chamar `step` de novo no próximo frame.
    InProgress,
    /// A grade inteira (ou só a tela visível, na tela alternativa) foi
    /// varrida; [`SearchJob::occurrences`] tem o resultado final.
    Done,
}

/// Quantas linhas da grade um lote de [`SearchJob::step`] varre antes de
/// devolver o controle. Medido em `tests/search_scrollback_cost.rs` (item 5
/// da Etapa 1): o pior lote fica em poucos milissegundos, folgado dentro do
/// orçamento de frame, tanto no default de `scrollback.lines` (10.000)
/// quanto em dez vezes esse valor -- em debug (`cargo test` sem
/// `--release`) e a fortiori em release, o build que roda de verdade.
/// `porecatu-ui` (Etapa 2) pode escolher outro valor, ex. para se adaptar
/// ao tempo restante do frame.
pub const DEFAULT_SEARCH_LINES_PER_STEP: usize = 100;

/// Escapa metacaracteres de regex para busca literal (RF-11.4). Sem
/// depender de crate novo: `alacritty_terminal` não expõe a função de
/// escape do `regex-automata` que usa por baixo, e "nenhuma dependência
/// nova" é regra desta etapa.
fn escape_literal(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    for c in pattern.chars() {
        if matches!(
            c,
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn to_grid_pos(point: Point) -> GridPos {
    GridPos {
        line: point.line.0,
        column: point.column.0,
    }
}

/// Busca continuável sobre a grade (ADR-0041 §"Riscos e mitigação": "a
/// busca vira incremental por lotes"). Varre no máximo `lines_per_step`
/// linhas por chamada de [`SearchJob::step`] em vez da grade inteira de uma
/// vez -- é o que mantém cada chamada dentro do orçamento de frame, mesmo
/// com scrollback cheio e um padrão sem ocorrência nenhuma. `pub(crate)`:
/// só `crate::engine::TermEngine` chama `new`/`step` (precisam de `&Term`);
/// o resto da API (`occurrences`, `scope_reduced`, `is_done`) é público --
/// `porecatu-ui` guarda o job por aba e consulta o progresso.
#[derive(Debug)]
pub struct SearchJob {
    regex: RegexSearch,
    /// Próximo ponto a varrer. `None` quando a busca termina.
    cursor: Option<Point>,
    /// Fim absoluto da busca inteira (não do lote atual).
    end: Point,
    lines_per_step: usize,
    occurrences: Vec<Occurrence>,
    scope_reduced: bool,
}

impl SearchJob {
    /// Compila o padrão (literal ou regex, `mode`) e prepara a busca sobre
    /// `term`, sem varrer nada ainda -- a primeira chamada de `step` faz o
    /// primeiro lote. Erro devolvido se o padrão de regex não compila
    /// (RF-11.4), nunca `panic`. Escopo: tela visível mais scrollback
    /// inteiro (RF-11.3), reduzido à tela visível na tela alternativa
    /// (RF-11.8, `scope_reduced`).
    pub(crate) fn new<T>(
        term: &Term<T>,
        pattern: &str,
        mode: SearchMode,
        lines_per_step: usize,
    ) -> Result<Self, InvalidPattern> {
        let scope_reduced = term.mode().contains(TermMode::ALT_SCREEN);

        if pattern.is_empty() {
            // `RegexSearch::new("")` compila (casa em toda posição), mas
            // não há o que varrer -- padrão vazio é outcome vazio, não erro
            // nem trabalho.
            return Ok(Self {
                regex: RegexSearch::new(".").expect("padrão fixo sempre compila"),
                cursor: None,
                end: Point::new(term.bottommost_line(), term.last_column()),
                lines_per_step,
                occurrences: Vec::new(),
                scope_reduced,
            });
        }

        let escaped;
        let regex_pattern = match mode {
            SearchMode::Literal => {
                escaped = escape_literal(pattern);
                escaped.as_str()
            }
            SearchMode::Regex => pattern,
        };

        let regex =
            RegexSearch::new(regex_pattern).map_err(|err| InvalidPattern(err.to_string()))?;

        Ok(Self {
            regex,
            cursor: Some(Point::new(term.topmost_line(), Column(0))),
            end: Point::new(term.bottommost_line(), term.last_column()),
            lines_per_step,
            occurrences: Vec::new(),
            scope_reduced,
        })
    }

    /// Varre até `lines_per_step` linhas a partir de onde a busca parou.
    /// Devolve [`SearchStep::InProgress`] se ainda há grade por varrer --
    /// chamar de novo no próximo frame -- ou [`SearchStep::Done`] quando
    /// termina. Sem efeito (devolve `Done` direto) se já tiver terminado.
    pub(crate) fn step<T>(&mut self, term: &Term<T>) -> SearchStep {
        let Some(cursor) = self.cursor else {
            return SearchStep::Done;
        };

        let batch_end_line = cursor.line + self.lines_per_step;
        let (batch_end, is_last_batch) = if batch_end_line >= self.end.line {
            (self.end, true)
        } else {
            (Point::new(batch_end_line, term.last_column()), false)
        };

        let mut last_match_end = None;
        for regex_match in
            RegexIter::new(cursor, batch_end, Direction::Right, term, &mut self.regex)
        {
            last_match_end = Some(*regex_match.end());
            self.occurrences.push(Occurrence {
                start: to_grid_pos(*regex_match.start()),
                end: to_grid_pos(*regex_match.end()),
            });
        }

        if is_last_batch {
            self.cursor = None;
            return SearchStep::Done;
        }

        // Retoma depois do fim do lote, ou depois do fim da última
        // ocorrência achada, se ela avançou além do lote (ex. padrão que
        // casa através de uma quebra de linha) -- mesma lógica de
        // `RegexIter::skip`, para não reencontrar a mesma ocorrência no
        // próximo lote.
        let resume_from = match last_match_end {
            Some(end) if end > batch_end => end,
            _ => batch_end,
        };
        let resume_from = term.expand_wide(resume_from, Direction::Right);
        self.cursor = Some(resume_from.add(term, Boundary::None, 1));

        SearchStep::InProgress
    }

    /// Ocorrências achadas até agora -- cresce a cada [`SearchJob::step`].
    pub fn occurrences(&self) -> &[Occurrence] {
        &self.occurrences
    }

    /// `true` com a tela alternativa ativa: não há scrollback a percorrer e
    /// a tela pertence ao programa, então a busca operou só sobre a tela
    /// visível (RF-11.8, ADR-0041 §7) -- quem chama precisa saber que o
    /// escopo foi reduzido, para dizê-lo ao usuário em vez de devolver zero
    /// ocorrências sem explicação.
    pub fn scope_reduced(&self) -> bool {
        self.scope_reduced
    }

    /// `true` quando não há mais lote para varrer.
    pub fn is_done(&self) -> bool {
        self.cursor.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_literal_escapa_metacaracteres() {
        assert_eq!(escape_literal("a.b*c"), r"a\.b\*c");
        assert_eq!(escape_literal("(x)"), r"\(x\)");
        assert_eq!(escape_literal("plain"), "plain");
    }
}
