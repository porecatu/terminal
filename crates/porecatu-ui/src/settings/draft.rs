// SPDX-License-Identifier: GPL-3.0-or-later

//! O rascunho da tela de configurações (RF-16.15, ADR-0059 §4): o que o
//! usuário mudou e ainda não gravou. Estado puro -- sem `winit`, sem `wgpu`,
//! sem disco --, no molde de `group_editor.rs` e `session_picker.rs`: recebe o
//! que a tela faz e responde o que ela precisa mostrar. A tela **só produz
//! `Edit`s**; gravá-los é do `ConfigDocument::save` (ADR-0058).
//!
//! Regras (RF-16.15 a RF-16.18):
//!
//! - uma pendência por opção, a última vence;
//! - voltar uma opção ao valor do arquivo apaga a pendência;
//! - "restaurar padrão" é uma pendência que vira `Edit::Remove` -- a chave sai
//!   do arquivo em vez de ser regravada com o padrão --, e só existe quando o
//!   valor em vista difere do de `Config::default()`;
//! - valor recusado (fora da faixa, número malformado, nome de variável vazio
//!   ou repetido) **fica** no rascunho, marcado, e bloqueia o Salvar; nada
//!   vira `Edit` enquanto estiver assim;
//! - a faixa é regra de **edição**, não de leitura: um valor do arquivo fora
//!   dela é exibido como está e marcado ([`Draft::file_issue`]), nunca
//!   corrigido sozinho, e só é regravado se o usuário o alterar.
//!
//! Erro é tipado ([`ValueError`]), nunca prosa: a frase é composta na tela, a
//! partir do catálogo de textos (ADR-0056 §2).

// Sem consumidor até as tarefas que desenham a tela (06 em diante).
#![allow(dead_code)]

use std::collections::{BTreeMap, HashSet};

use porecatu_config::{Config, Edit, EditValue};

use super::catalog::{Control, GIT_POLL_MAX, GIT_POLL_MIN, Group, OPTIONS, OptionDef, number_of};

/// Por que um valor foi recusado.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ValueError {
    /// O texto de um campo numérico não é número.
    NotANumber,
    /// Campo inteiro recebeu um número com parte decimal.
    NotAnInteger,
    /// Número fora da faixa de edição da opção.
    OutOfRange { min: f64, max: f64 },
    /// Valor que não é um dos nomeados do enum.
    NotAChoice,
    /// O valor não é do tipo que o controle da opção produz -- erro de quem
    /// chama, não do usuário.
    WrongType,
    /// Nome de variável de ambiente vazio (RF-16.18).
    EmptyName,
    /// Nome de variável de ambiente repetido (RF-16.26).
    DuplicateName(String),
}

/// A pendência de uma opção.
#[derive(Debug, Clone, PartialEq)]
enum Pending {
    /// Um valor válido, diferente do arquivo.
    Value(EditValue),
    /// "Restaurar padrão": a chave sai do arquivo.
    Reset,
    /// Um valor recusado. `raw` é o texto digitado, para o campo continuar
    /// mostrando o que o usuário escreveu (vazio onde não há texto, como numa
    /// lista).
    Invalid { raw: String, error: ValueError },
}

/// O rascunho: o `Config` em vigor (o que o arquivo diz) mais as pendências.
pub(crate) struct Draft {
    file: Config,
    /// Por identificador de opção. `Vec` e não mapa: são poucas, e a ordem de
    /// `edits` sai da tabela, não da inserção.
    pending: Vec<(&'static str, Pending)>,
}

impl Draft {
    /// Um rascunho sem pendências, mostrando os valores de `file`.
    pub(crate) fn new(file: &Config) -> Self {
        Self {
            file: file.clone(),
            pending: Vec::new(),
        }
    }

    // ---- leitura

    /// O `Config` que o arquivo diz -- a base do rascunho: lista de temas,
    /// valores das opções sem pendência.
    pub(crate) fn file_config(&self) -> &Config {
        &self.file
    }

    /// O valor que o arquivo diz para `option`.
    pub(crate) fn file_value(&self, option: &OptionDef) -> EditValue {
        option.read(&self.file)
    }

    /// O valor em vista: o pendente, o padrão se a pendência é restaurar, ou o
    /// do arquivo. Pendência recusada não vira valor -- o campo mostra o texto
    /// dela ([`Self::raw`]) e este devolve o do arquivo.
    pub(crate) fn value(&self, option: &OptionDef) -> EditValue {
        match self.entry(option) {
            Some(Pending::Value(value)) => value.clone(),
            Some(Pending::Reset) => option.default_value(),
            Some(Pending::Invalid { .. }) | None => self.file_value(option),
        }
    }

    /// O texto digitado de uma pendência recusada.
    pub(crate) fn raw(&self, option: &OptionDef) -> Option<&str> {
        match self.entry(option) {
            Some(Pending::Invalid { raw, .. }) => Some(raw),
            _ => None,
        }
    }

    /// A razão da recusa, se a pendência de `option` foi recusada.
    pub(crate) fn invalid(&self, option: &OptionDef) -> Option<&ValueError> {
        match self.entry(option) {
            Some(Pending::Invalid { error, .. }) => Some(error),
            _ => None,
        }
    }

    /// RF-16.18: o valor que **o arquivo** traz está fora da faixa de edição.
    /// É só uma marca -- não é pendência, não bloqueia o Salvar e nada o
    /// corrige.
    pub(crate) fn file_issue(&self, option: &OptionDef) -> Option<ValueError> {
        validate(option, &self.file_value(option)).err()
    }

    /// "Restaurar padrão" está disponível: o valor em vista difere do padrão.
    pub(crate) fn can_reset(&self, option: &OptionDef) -> bool {
        self.value(option) != option.default_value()
    }

    // ---- consultas do rascunho

    /// Há alguma pendência (válida ou recusada)?
    pub(crate) fn is_dirty(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Quantas pendências há em `group` -- o marcador ao lado do nome na guia.
    pub(crate) fn pending_in_group(&self, group: Group) -> usize {
        self.pending
            .iter()
            .filter(|(id, _)| OPTIONS.iter().any(|o| o.id == *id && o.group == group))
            .count()
    }

    /// Os grupos com ao menos uma pendência, na ordem da guia.
    pub(crate) fn pending_groups(&self) -> Vec<Group> {
        Group::ALL
            .into_iter()
            .filter(|group| self.pending_in_group(*group) > 0)
            .collect()
    }

    /// Quantas pendências há ao todo.
    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// `option` tem pendência.
    pub(crate) fn is_pending(&self, option: &OptionDef) -> bool {
        self.entry(option).is_some()
    }

    /// Alguma pendência foi recusada: o Salvar fica indisponível (RF-16.18).
    pub(crate) fn has_invalid(&self) -> bool {
        self.pending
            .iter()
            .any(|(_, pending)| matches!(pending, Pending::Invalid { .. }))
    }

    /// As edições a gravar: uma por pendência válida, na ordem do catálogo.
    /// Pendência recusada nunca vira edição.
    pub(crate) fn edits(&self) -> Vec<Edit> {
        OPTIONS
            .iter()
            .filter_map(|option| match self.entry(option)? {
                Pending::Value(value) => Some(Edit::Set(option.key_path(), value.clone())),
                Pending::Reset => Some(Edit::Remove(option.key_path())),
                Pending::Invalid { .. } => None,
            })
            .collect()
    }

    // ---- mudanças

    /// Põe `value` em `option`. O valor é validado e normalizado (um número
    /// inteiro num campo decimal vira decimal); recusado, fica no rascunho
    /// como pendência recusada e o erro volta. Igual ao do arquivo, apaga a
    /// pendência.
    pub(crate) fn set(&mut self, option: &OptionDef, value: EditValue) -> Result<(), ValueError> {
        match validate(option, &value) {
            Ok(value) => {
                self.record(option, value);
                Ok(())
            }
            Err(error) => {
                self.put(
                    option,
                    Pending::Invalid {
                        raw: String::new(),
                        error: error.clone(),
                    },
                );
                Err(error)
            }
        }
    }

    /// Como [`Self::set`], para o texto de um campo (numérico ou de texto):
    /// lê o texto pelo controle da opção. Recusado, o texto fica guardado.
    pub(crate) fn set_raw(&mut self, option: &OptionDef, raw: &str) -> Result<(), ValueError> {
        match parse_raw(option, raw).and_then(|value| validate(option, &value)) {
            Ok(value) => {
                self.record(option, value);
                Ok(())
            }
            Err(error) => {
                self.put(
                    option,
                    Pending::Invalid {
                        raw: raw.to_owned(),
                        error: error.clone(),
                    },
                );
                Err(error)
            }
        }
    }

    /// Lista de nome e valor (`shell.env`), como as linhas que a tela mostra:
    /// aqui é que nome vazio e nome repetido são vistos, porque o mapa do
    /// `EditValue` já não os pode ter.
    pub(crate) fn set_rows(
        &mut self,
        option: &OptionDef,
        rows: &[(String, String)],
    ) -> Result<(), ValueError> {
        let mut seen = HashSet::new();
        let mut error = None;
        for (name, _) in rows {
            if name.is_empty() {
                error = Some(ValueError::EmptyName);
                break;
            }
            if !seen.insert(name.as_str()) {
                error = Some(ValueError::DuplicateName(name.clone()));
                break;
            }
        }
        match error {
            None => {
                let map: BTreeMap<String, String> = rows.iter().cloned().collect();
                self.set(option, EditValue::StringMap(map))
            }
            Some(error) => {
                self.put(
                    option,
                    Pending::Invalid {
                        raw: String::new(),
                        error: error.clone(),
                    },
                );
                Err(error)
            }
        }
    }

    /// RF-16.16: "restaurar padrão". Sem efeito quando o valor em vista já é o
    /// padrão. Se o arquivo já diz o padrão, restaurar é só desfazer a
    /// pendência -- não há o que remover.
    pub(crate) fn reset(&mut self, option: &OptionDef) {
        if !self.can_reset(option) {
            return;
        }
        if self.file_value(option) == option.default_value() {
            self.clear(option);
        } else {
            self.put(option, Pending::Reset);
        }
    }

    /// Desfaz a pendência de `option`.
    pub(crate) fn clear(&mut self, option: &OptionDef) {
        self.pending.retain(|(id, _)| *id != option.id);
    }

    /// Descartar (RF-16.15): nenhuma pendência, de volta ao que o arquivo diz.
    pub(crate) fn discard(&mut self) {
        self.pending.clear();
    }

    /// O que o arquivo diz mudou -- uma recarga que não veio do Salvar da
    /// própria tela. A base troca; uma pendência que agora coincide com o
    /// arquivo deixa de ser pendência (RF-16.15), e as outras ficam.
    // TODO(tarefa 11): arquivo alterado fora com pendências é a faixa do
    // RF-16.23 (Recarregar / Manter minhas alterações); aqui a base só troca.
    pub(crate) fn rebase(&mut self, file: &Config) {
        self.file = file.clone();
        let file = &self.file;
        self.pending.retain(|(id, pending)| {
            let Some(option) = OPTIONS.iter().find(|option| option.id == *id) else {
                return true;
            };
            match pending {
                Pending::Value(value) => *value != option.read(file),
                Pending::Reset => option.read(file) != option.default_value(),
                Pending::Invalid { .. } => true,
            }
        });
    }

    /// Salvar deu certo: o arquivo agora diz `saved` (o texto gravado,
    /// relido), e nenhuma pendência sobra. A tela passa a mostrar isso na
    /// hora, sem esperar a recarga a quente que o watcher vai disparar.
    pub(crate) fn commit(&mut self, saved: &Config) {
        self.file = saved.clone();
        self.pending.clear();
    }

    // ---- internos

    fn entry(&self, option: &OptionDef) -> Option<&Pending> {
        self.pending
            .iter()
            .find(|(id, _)| *id == option.id)
            .map(|(_, pending)| pending)
    }

    /// Grava um valor **válido**: igual ao do arquivo é o mesmo que nada.
    fn record(&mut self, option: &OptionDef, value: EditValue) {
        if value == self.file_value(option) {
            self.clear(option);
        } else {
            self.put(option, Pending::Value(value));
        }
    }

    fn put(&mut self, option: &OptionDef, pending: Pending) {
        match self.pending.iter_mut().find(|(id, _)| *id == option.id) {
            Some((_, slot)) => *slot = pending,
            None => self.pending.push((option.id, pending)),
        }
    }
}

/// Lê o texto de um campo conforme o controle da opção.
fn parse_raw(option: &OptionDef, raw: &str) -> Result<EditValue, ValueError> {
    match option.control {
        Control::Text | Control::Language | Control::Theme => Ok(EditValue::String(raw.to_owned())),
        Control::Number { float: true, .. } => raw
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(EditValue::Float)
            .ok_or(ValueError::NotANumber),
        Control::Number { float: false, .. } => parse_integer(raw),
        Control::GitPoll => parse_integer(raw),
        _ => Err(ValueError::WrongType),
    }
}

fn parse_integer(raw: &str) -> Result<EditValue, ValueError> {
    let raw = raw.trim();
    match raw.parse::<i64>() {
        Ok(number) => Ok(EditValue::Integer(number)),
        // "3.5" é número, mas não inteiro; "abc" nem é número.
        Err(_) if raw.parse::<f64>().is_ok_and(f64::is_finite) => Err(ValueError::NotAnInteger),
        Err(_) => Err(ValueError::NotANumber),
    }
}

/// Valida `value` contra o controle de `option` e devolve a forma normalizada
/// (inteiro num campo decimal vira decimal). Pura: é a única regra de
/// edição, usada tanto para o que o usuário faz quanto para marcar o que o
/// arquivo traz.
pub(crate) fn validate(option: &OptionDef, value: &EditValue) -> Result<EditValue, ValueError> {
    match (option.control, value) {
        (Control::Toggle, EditValue::Bool(_)) => Ok(value.clone()),
        (Control::Text | Control::Language | Control::Theme, EditValue::String(_)) => {
            Ok(value.clone())
        }
        (
            Control::Number {
                min, max, float, ..
            },
            value,
        ) => {
            let number = number_of(value).ok_or(ValueError::WrongType)?;
            if !number.is_finite() {
                return Err(ValueError::NotANumber);
            }
            if !float && number.fract() != 0.0 {
                return Err(ValueError::NotAnInteger);
            }
            if !(min..=max).contains(&number) {
                return Err(ValueError::OutOfRange { min, max });
            }
            Ok(if float {
                EditValue::Float(number)
            } else {
                EditValue::Integer(number as i64)
            })
        }
        (Control::Choice(choices), EditValue::String(choice)) => {
            if choices.contains(&choice.as_str()) {
                Ok(value.clone())
            } else {
                Err(ValueError::NotAChoice)
            }
        }
        (Control::StringList, EditValue::StringList(_)) => Ok(value.clone()),
        (Control::StringMap, EditValue::StringMap(map)) => {
            if map.keys().any(String::is_empty) {
                Err(ValueError::EmptyName)
            } else {
                Ok(value.clone())
            }
        }
        (Control::GitPoll, EditValue::Integer(seconds)) => {
            if *seconds == 0 || (GIT_POLL_MIN..=GIT_POLL_MAX).contains(seconds) {
                Ok(value.clone())
            } else {
                Err(ValueError::OutOfRange {
                    min: GIT_POLL_MIN as f64,
                    max: GIT_POLL_MAX as f64,
                })
            }
        }
        _ => Err(ValueError::WrongType),
    }
}

#[cfg(test)]
mod tests {
    use super::super::catalog::option;
    use super::*;

    fn opt(id: &str) -> &'static OptionDef {
        option(id).unwrap()
    }

    fn draft() -> Draft {
        Draft::new(&Config::default())
    }

    fn draft_from(text: &str) -> Draft {
        Draft::new(&porecatu_config::parse(text).unwrap().0)
    }

    #[test]
    fn a_fresh_draft_has_nothing_pending() {
        let draft = draft();
        assert!(!draft.is_dirty());
        assert!(!draft.has_invalid());
        assert!(draft.edits().is_empty());
        assert_eq!(draft.value(opt("font_size")), EditValue::Float(14.0));
    }

    #[test]
    fn changing_a_value_creates_a_pending_edit() {
        let mut draft = draft();
        draft.set(opt("font_size"), EditValue::Float(16.0)).unwrap();
        assert!(draft.is_dirty());
        assert!(draft.is_pending(opt("font_size")));
        assert!(!draft.is_pending(opt("line_height")));
        assert_eq!(draft.value(opt("font_size")), EditValue::Float(16.0));
        assert_eq!(
            draft.edits(),
            vec![Edit::Set(
                opt("font_size").key_path(),
                EditValue::Float(16.0)
            )]
        );
    }

    #[test]
    fn the_last_value_wins() {
        let mut draft = draft();
        draft.set(opt("font_size"), EditValue::Float(16.0)).unwrap();
        draft.set(opt("font_size"), EditValue::Float(18.0)).unwrap();
        assert_eq!(draft.edits().len(), 1);
        assert_eq!(draft.value(opt("font_size")), EditValue::Float(18.0));
    }

    #[test]
    fn going_back_to_the_file_value_removes_the_pending_edit() {
        let mut draft = draft_from("[terminal.font]\nsize = 12.0\n");
        draft.set(opt("font_size"), EditValue::Float(20.0)).unwrap();
        assert!(draft.is_dirty());
        draft.set(opt("font_size"), EditValue::Float(12.0)).unwrap();
        assert!(!draft.is_dirty());
        assert!(draft.edits().is_empty());
    }

    #[test]
    fn setting_the_file_value_from_the_start_is_not_a_change() {
        let mut draft = draft();
        draft.set(opt("font_size"), EditValue::Float(14.0)).unwrap();
        assert!(!draft.is_dirty());
    }

    #[test]
    fn edits_follow_the_catalog_order_not_the_insertion_order() {
        let mut draft = draft();
        draft.set(opt("min_rows"), EditValue::Integer(8)).unwrap();
        draft.set(opt("font_size"), EditValue::Float(16.0)).unwrap();
        draft
            .set(opt("language"), EditValue::String("pt_BR".into()))
            .unwrap();
        let paths: Vec<String> = draft
            .edits()
            .iter()
            .map(|edit| match edit {
                Edit::Set(path, _) | Edit::Remove(path) => path.to_string(),
            })
            .collect();
        assert_eq!(
            paths,
            ["general.language", "terminal.font.size", "panes.min_rows"]
        );
    }

    #[test]
    fn restore_default_becomes_a_remove() {
        let mut draft = draft_from("[terminal.font]\nsize = 20.0\n");
        let size = opt("font_size");
        assert!(draft.can_reset(size));
        draft.reset(size);
        assert!(draft.is_pending(size));
        assert_eq!(draft.value(size), EditValue::Float(14.0));
        assert_eq!(draft.edits(), vec![Edit::Remove(size.key_path())]);
    }

    #[test]
    fn restore_default_is_unavailable_at_the_default() {
        let mut draft = draft();
        let size = opt("font_size");
        assert!(!draft.can_reset(size));
        draft.reset(size);
        assert!(!draft.is_dirty());
    }

    #[test]
    fn restore_default_on_a_pending_change_undoes_it_when_the_file_says_default() {
        let mut draft = draft();
        let size = opt("font_size");
        draft.set(size, EditValue::Float(20.0)).unwrap();
        assert!(draft.can_reset(size));
        draft.reset(size);
        // O arquivo já diz o padrão: não há chave a remover.
        assert!(!draft.is_dirty());
    }

    #[test]
    fn restore_default_after_a_change_in_the_file_value_stays_a_remove() {
        let mut draft = draft_from("[terminal.font]\nsize = 20.0\n");
        let size = opt("font_size");
        draft.set(size, EditValue::Float(30.0)).unwrap();
        draft.reset(size);
        assert_eq!(draft.edits(), vec![Edit::Remove(size.key_path())]);
    }

    #[test]
    fn a_value_out_of_range_is_recorded_as_invalid_and_blocks_saving() {
        let mut draft = draft();
        let size = opt("font_size");
        assert_eq!(
            draft.set_raw(size, "500"),
            Err(ValueError::OutOfRange {
                min: 6.0,
                max: 72.0
            })
        );
        assert!(draft.has_invalid());
        assert!(draft.is_dirty());
        assert_eq!(draft.raw(size), Some("500"));
        assert_eq!(
            draft.invalid(size),
            Some(&ValueError::OutOfRange {
                min: 6.0,
                max: 72.0
            })
        );
        // O que o campo mostra como valor continua o do arquivo, e nada vira
        // edição.
        assert_eq!(draft.value(size), EditValue::Float(14.0));
        assert!(draft.edits().is_empty());
    }

    #[test]
    fn fixing_an_invalid_value_clears_the_mark() {
        let mut draft = draft();
        let size = opt("font_size");
        draft.set_raw(size, "500").unwrap_err();
        draft.set_raw(size, "16").unwrap();
        assert!(!draft.has_invalid());
        assert_eq!(draft.value(size), EditValue::Float(16.0));
        draft.set_raw(size, "abc").unwrap_err();
        assert_eq!(draft.invalid(size), Some(&ValueError::NotANumber));
        draft.set_raw(size, "14").unwrap();
        assert!(!draft.is_dirty(), "voltou ao valor do arquivo");
    }

    #[test]
    fn number_fields_read_text_by_their_own_shape() {
        let mut draft = draft();
        let lines = opt("scrollback_lines");
        assert_eq!(draft.set_raw(lines, "2.5"), Err(ValueError::NotAnInteger));
        assert_eq!(draft.set_raw(lines, "mil"), Err(ValueError::NotANumber));
        assert_eq!(draft.set_raw(lines, "NaN"), Err(ValueError::NotANumber));
        assert_eq!(
            draft.set_raw(lines, "2000000"),
            Err(ValueError::OutOfRange {
                min: 0.0,
                max: 1_000_000.0
            })
        );
        draft.set_raw(lines, " 50000 ").unwrap();
        assert_eq!(draft.value(lines), EditValue::Integer(50_000));
        // Decimal aceita inteiro e grava decimal.
        let size = opt("font_size");
        draft.set(size, EditValue::Integer(16)).unwrap();
        assert_eq!(draft.value(size), EditValue::Float(16.0));
    }

    #[test]
    fn range_edges_are_inclusive() {
        let mut draft = draft();
        let opacity = opt("window_opacity");
        draft.set(opacity, EditValue::Float(0.0)).unwrap();
        draft.set(opacity, EditValue::Float(1.0)).unwrap();
        assert!(draft.set(opacity, EditValue::Float(1.0001)).is_err());
        assert!(draft.set(opacity, EditValue::Float(-0.01)).is_err());
    }

    #[test]
    fn a_file_value_out_of_range_is_shown_and_marked_never_fixed() {
        let mut draft = draft_from("[terminal.font]\nsize = 500.0\n");
        let size = opt("font_size");
        assert_eq!(draft.value(size), EditValue::Float(500.0));
        assert_eq!(
            draft.file_issue(size),
            Some(ValueError::OutOfRange {
                min: 6.0,
                max: 72.0
            })
        );
        // Marcar não é pendência, e não bloqueia o Salvar.
        assert!(!draft.is_dirty());
        assert!(!draft.has_invalid());
        assert!(draft.edits().is_empty());
        // Só é regravado se o usuário o altera.
        draft.set(size, EditValue::Float(40.0)).unwrap();
        assert_eq!(draft.edits().len(), 1);
        // Em faixa, nenhuma marca.
        assert_eq!(draft_from("").file_issue(size), None);
    }

    #[test]
    fn choices_accept_only_the_named_values() {
        let mut draft = draft();
        let shape = opt("cursor_shape");
        draft.set(shape, EditValue::String("beam".into())).unwrap();
        assert_eq!(
            draft.set(shape, EditValue::String("triangle".into())),
            Err(ValueError::NotAChoice)
        );
        assert!(draft.has_invalid());
    }

    #[test]
    fn a_value_of_the_wrong_type_is_refused() {
        let mut draft = draft();
        assert_eq!(
            draft.set(opt("cursor_blink"), EditValue::Integer(1)),
            Err(ValueError::WrongType)
        );
    }

    #[test]
    fn env_rows_refuse_an_empty_or_repeated_name() {
        let mut draft = draft();
        let env = opt("shell_env");
        let row = |name: &str, value: &str| (name.to_owned(), value.to_owned());
        assert_eq!(
            draft.set_rows(env, &[row("A", "1"), row("", "2")]),
            Err(ValueError::EmptyName)
        );
        assert!(draft.has_invalid());
        assert_eq!(
            draft.set_rows(env, &[row("A", "1"), row("A", "2")]),
            Err(ValueError::DuplicateName("A".into()))
        );
        draft
            .set_rows(env, &[row("A", "1"), row("B", "2")])
            .unwrap();
        assert!(!draft.has_invalid());
        assert_eq!(draft.edits().len(), 1);
        // Voltar a nenhuma variável é voltar ao arquivo.
        draft.set_rows(env, &[]).unwrap();
        assert!(!draft.is_dirty());
    }

    #[test]
    fn lists_are_compared_as_a_whole() {
        let mut draft = draft_from("[shell]\nargs = [\"-l\"]\n");
        let args = opt("shell_args");
        draft
            .set(args, EditValue::StringList(vec!["-l".into(), "-i".into()]))
            .unwrap();
        assert!(draft.is_pending(args));
        draft
            .set(args, EditValue::StringList(vec!["-l".into()]))
            .unwrap();
        assert!(!draft.is_pending(args));
    }

    #[test]
    fn git_poll_is_off_or_between_the_floor_and_a_day() {
        let mut draft = draft();
        let poll = opt("git_remote_poll");
        draft.set(poll, EditValue::Integer(0)).unwrap();
        draft.set(poll, EditValue::Integer(30)).unwrap();
        draft.set(poll, EditValue::Integer(86_400)).unwrap();
        for bad in [1, 29, 86_401, -5] {
            assert!(
                matches!(
                    draft.set(poll, EditValue::Integer(bad)),
                    Err(ValueError::OutOfRange { .. })
                ),
                "{bad}"
            );
        }
        assert_eq!(draft.set_raw(poll, "1.5"), Err(ValueError::NotAnInteger));
    }

    #[test]
    fn pending_is_counted_per_group() {
        let mut draft = draft();
        draft.set(opt("font_size"), EditValue::Float(16.0)).unwrap();
        draft
            .set(opt("cursor_blink"), EditValue::Bool(true))
            .unwrap();
        draft.set(opt("min_rows"), EditValue::Integer(8)).unwrap();
        assert_eq!(draft.pending_in_group(Group::Terminal), 2);
        assert_eq!(draft.pending_in_group(Group::Panes), 1);
        assert_eq!(draft.pending_in_group(Group::General), 0);
        assert_eq!(draft.pending_in_group(Group::Shortcuts), 0);
    }

    #[test]
    fn discard_drops_every_pending_edit() {
        let mut draft = draft();
        draft.set(opt("font_size"), EditValue::Float(16.0)).unwrap();
        draft.set_raw(opt("line_height"), "abc").unwrap_err();
        draft.discard();
        assert!(!draft.is_dirty());
        assert!(!draft.has_invalid());
        assert!(draft.edits().is_empty());
    }

    #[test]
    fn every_valid_edit_round_trips_through_save_and_parse() {
        // As edições do rascunho aplicadas ao arquivo de exemplo dão um
        // arquivo que o `parse` aceita, sem chave desconhecida (RF-16.19).
        let example = include_str!("../../../../docs/config/porecatu.example.toml");
        let document = porecatu_config::ConfigDocument::parse(example).unwrap();
        let mut draft = draft();
        for option in OPTIONS {
            draft
                .set(option, super::super::catalog::probe_value(option))
                .unwrap();
        }
        assert_eq!(draft.edits().len(), OPTIONS.len());
        let (text, unknown) = document.apply_checked(&draft.edits()).unwrap();
        assert!(unknown.is_empty(), "{unknown:?}");
        let (config, _) = porecatu_config::parse(&text).unwrap();
        for option in OPTIONS {
            assert_eq!(option.read(&config), draft.value(option), "{}", option.id);
        }
    }

    #[test]
    fn pending_groups_follow_the_catalog_group_of_each_pending_option() {
        let mut draft = draft();
        assert!(draft.pending_groups().is_empty());
        draft.set(opt("font_size"), EditValue::Float(16.0)).unwrap();
        draft
            .set(opt("language"), EditValue::String("pt_BR".to_owned()))
            .unwrap();
        assert_eq!(draft.pending_groups(), [Group::General, Group::Terminal]);
        assert_eq!(draft.pending_count(), 2);
    }

    #[test]
    fn committing_a_save_makes_the_saved_config_the_new_base_and_clears_everything() {
        let mut draft = draft();
        draft.set(opt("font_size"), EditValue::Float(16.0)).unwrap();
        draft.set_raw(opt("line_height"), "abc").unwrap_err();
        let mut saved = Config::default();
        saved.terminal.font.size = 16.0;
        draft.commit(&saved);
        assert!(!draft.is_dirty());
        assert!(!draft.has_invalid());
        assert_eq!(draft.value(opt("font_size")), EditValue::Float(16.0));
        assert_eq!(draft.file_value(opt("font_size")), EditValue::Float(16.0));
        // O novo arquivo já diz 16: restaurar agora remove a chave.
        draft.reset(opt("font_size"));
        assert_eq!(draft.edits().len(), 1);
    }

    #[test]
    fn a_reload_drops_the_pendings_the_file_now_agrees_with_and_keeps_the_rest() {
        let mut draft = draft();
        draft.set(opt("font_size"), EditValue::Float(16.0)).unwrap();
        draft
            .set(opt("cursor_blink"), EditValue::Bool(true))
            .unwrap();
        // Um editor externo gravou o 16: essa pendência acabou, a outra segue.
        let mut reloaded = Config::default();
        reloaded.terminal.font.size = 16.0;
        draft.rebase(&reloaded);
        assert!(!draft.is_pending(opt("font_size")));
        assert!(draft.is_pending(opt("cursor_blink")));
        assert_eq!(draft.file_value(opt("font_size")), EditValue::Float(16.0));
    }

    #[test]
    fn a_pending_reset_is_dropped_when_the_file_already_says_the_default() {
        let file = porecatu_config::parse(
            "[terminal.font]
size = 18.0
",
        )
        .unwrap()
        .0;
        let mut draft = Draft::new(&file);
        draft.reset(opt("font_size"));
        assert_eq!(draft.edits().len(), 1);
        // O arquivo passa a dizer o padrão (outra janela restaurou): nada a remover.
        draft.rebase(&Config::default());
        assert!(!draft.is_dirty());
    }
}
