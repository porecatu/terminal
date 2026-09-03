// SPDX-License-Identifier: GPL-3.0-or-later

//! `Action` (ADR-0029): o catálogo fechado de `docs/reference/acoes.md`,
//! como enum. Vive aqui, não em `porecatu-config` nem em `porecatu-ui`,
//! porque é o único crate que os dois veem (regra de dependência de
//! CLAUDE.md) e porque ação é vocabulário de domínio -- metade das ações
//! (`group.next`, `tab.move_left`, ...) são executadas pelo próprio
//! `Workspace`.
//!
//! `FromStr`/`Display` casam **exatamente** o nome do catálogo. As duas
//! ações com argumento (`group.set_color`, `tab.move_to_group`) carregam
//! o argumento na variante, mas `FromStr` as rejeita: elas não são
//! vinculáveis a tecla (o catálogo já as marca `Arg`), e quem as invoca é
//! sempre um widget com o alvo já resolvido -- não o parser de
//! `[keybindings]`.

use std::fmt;
use std::str::FromStr;

use crate::group::GroupColor;
use crate::id::GroupId;

/// Argumento de `tab.move_to_group` -- espelha `porecatu_ui::MoveTarget`,
/// que não pode ser reusado aqui (a flecha de dependência vai de `ui`
/// para `core`, nunca o contrário).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDestination {
    Group(GroupId),
    /// Espec.: "mais 'Novo grupo' no fim" -- sempre a última linha.
    NewGroup,
}

/// Uma linha do catálogo fechado (`docs/reference/acoes.md`). `"none"`
/// não é variante: é a ausência de uma (ADR-0029) -- o mapa resolvido é
/// `HashMap<Chord, Action>`, e a config `"none"` remove a entrada em vez
/// de inserir uma variante inerte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    TabNew,
    TabClose,
    TabNext,
    TabPrev,
    /// `tab.goto_1`..`tab.goto_9`. As nove são explícitas no catálogo, não
    /// um padrão com curinga: `FromStr` valida `1..=9` e rejeita
    /// `tab.goto_10`/`tab.goto_0` como ação desconhecida, não como uma
    /// variante silenciosamente inerte.
    TabGoto(u8),
    TabRename,
    TabMoveLeft,
    TabMoveRight,
    /// `Arg` -- não vinculável, `FromStr` rejeita.
    TabMoveToGroup(MoveDestination),

    GroupCreate,
    GroupDissolve,
    GroupRename,
    /// `Arg` -- não vinculável, `FromStr` rejeita.
    GroupSetColor(GroupColor),
    GroupToggleCollapse,
    GroupNext,
    GroupPrev,
    GroupNewTab,
    GroupCloseAll,

    WindowNew,
    WindowClose,

    ScrollbackLineUp,
    ScrollbackLineDown,
    ScrollbackPageUp,
    ScrollbackPageDown,
    ScrollbackToTop,
    ScrollbackToBottom,

    ClipboardCopy,
    ClipboardPaste,
    SelectionSelectAll,

    FontIncrease,
    FontDecrease,
    FontReset,
    ThemeCycle,
    ConfigReload,
    SearchOpen,
    SearchNext,
    SearchPrev,
    AppQuit,
}

/// As 46 linhas do catálogo fechado, na grafia exata que `FromStr`/
/// `Display` usam. Único array-fonte: o teste bidirecional
/// (`tests::every_catalog_name_round_trips`) e a sugestão de erro
/// (`closest_name`) partem dele, então as duas checagens não podem
/// divergir da lista real.
///
/// `tab.goto_N` entra como as nove linhas do catálogo, não um padrão --
/// é o que faz `closest_name` sugerir `tab.goto_9` para um `tab.goto_9 `
/// digitado com espaço, por exemplo, em vez de nunca considerar a forma
/// completa.
pub const CATALOG: &[&str] = &[
    "tab.new",
    "tab.close",
    "tab.next",
    "tab.prev",
    "tab.goto_1",
    "tab.goto_2",
    "tab.goto_3",
    "tab.goto_4",
    "tab.goto_5",
    "tab.goto_6",
    "tab.goto_7",
    "tab.goto_8",
    "tab.goto_9",
    "tab.rename",
    "tab.move_left",
    "tab.move_right",
    "tab.move_to_group",
    "group.create",
    "group.dissolve",
    "group.rename",
    "group.set_color",
    "group.toggle_collapse",
    "group.next",
    "group.prev",
    "group.new_tab",
    "group.close_all",
    "window.new",
    "window.close",
    "scrollback.line_up",
    "scrollback.line_down",
    "scrollback.page_up",
    "scrollback.page_down",
    "scrollback.to_top",
    "scrollback.to_bottom",
    "clipboard.copy",
    "clipboard.paste",
    "selection.select_all",
    "font.increase",
    "font.decrease",
    "font.reset",
    "theme.cycle",
    "config.reload",
    "search.open",
    "search.next",
    "search.prev",
    "app.quit",
];

/// Erro de parse de uma ação (ADR-0029 §4): sempre traz a sugestão do
/// nome mais próximo do catálogo, exceto para as duas `Arg` -- rejeitadas
/// por razão própria, não por grafia errada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionParseError {
    pub input: String,
    /// `None` só para `group.set_color`/`tab.move_to_group`: a rejeição é
    /// "isto não é vinculável", não "isto não existe".
    pub suggestion: Option<&'static str>,
}

impl fmt::Display for ActionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.suggestion {
            Some(s) => write!(
                f,
                "ação desconhecida: \"{}\" -- você quis dizer \"{s}\"?",
                self.input
            ),
            None => write!(
                f,
                "\"{}\" tem argumento e não é vinculável a tecla",
                self.input
            ),
        }
    }
}

impl std::error::Error for ActionParseError {}

impl FromStr for Action {
    type Err = ActionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "group.set_color" || s == "tab.move_to_group" {
            return Err(ActionParseError {
                input: s.to_owned(),
                suggestion: None,
            });
        }
        if let Some(n) = s.strip_prefix("tab.goto_")
            && let Ok(digit) = n.parse::<u8>()
            && (1..=9).contains(&digit)
        {
            return Ok(Action::TabGoto(digit));
        }
        let action = match s {
            "tab.new" => Action::TabNew,
            "tab.close" => Action::TabClose,
            "tab.next" => Action::TabNext,
            "tab.prev" => Action::TabPrev,
            "tab.rename" => Action::TabRename,
            "tab.move_left" => Action::TabMoveLeft,
            "tab.move_right" => Action::TabMoveRight,
            "group.create" => Action::GroupCreate,
            "group.dissolve" => Action::GroupDissolve,
            "group.rename" => Action::GroupRename,
            "group.toggle_collapse" => Action::GroupToggleCollapse,
            "group.next" => Action::GroupNext,
            "group.prev" => Action::GroupPrev,
            "group.new_tab" => Action::GroupNewTab,
            "group.close_all" => Action::GroupCloseAll,
            "window.new" => Action::WindowNew,
            "window.close" => Action::WindowClose,
            "scrollback.line_up" => Action::ScrollbackLineUp,
            "scrollback.line_down" => Action::ScrollbackLineDown,
            "scrollback.page_up" => Action::ScrollbackPageUp,
            "scrollback.page_down" => Action::ScrollbackPageDown,
            "scrollback.to_top" => Action::ScrollbackToTop,
            "scrollback.to_bottom" => Action::ScrollbackToBottom,
            "clipboard.copy" => Action::ClipboardCopy,
            "clipboard.paste" => Action::ClipboardPaste,
            "selection.select_all" => Action::SelectionSelectAll,
            "font.increase" => Action::FontIncrease,
            "font.decrease" => Action::FontDecrease,
            "font.reset" => Action::FontReset,
            "theme.cycle" => Action::ThemeCycle,
            "config.reload" => Action::ConfigReload,
            "search.open" => Action::SearchOpen,
            "search.next" => Action::SearchNext,
            "search.prev" => Action::SearchPrev,
            "app.quit" => Action::AppQuit,
            _ => {
                return Err(ActionParseError {
                    input: s.to_owned(),
                    suggestion: Some(closest_name(s)),
                });
            }
        };
        Ok(action)
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Action::TabNew => "tab.new",
            Action::TabClose => "tab.close",
            Action::TabNext => "tab.next",
            Action::TabPrev => "tab.prev",
            Action::TabGoto(n) => return write!(f, "tab.goto_{n}"),
            Action::TabRename => "tab.rename",
            Action::TabMoveLeft => "tab.move_left",
            Action::TabMoveRight => "tab.move_right",
            Action::TabMoveToGroup(_) => "tab.move_to_group",
            Action::GroupCreate => "group.create",
            Action::GroupDissolve => "group.dissolve",
            Action::GroupRename => "group.rename",
            Action::GroupSetColor(_) => "group.set_color",
            Action::GroupToggleCollapse => "group.toggle_collapse",
            Action::GroupNext => "group.next",
            Action::GroupPrev => "group.prev",
            Action::GroupNewTab => "group.new_tab",
            Action::GroupCloseAll => "group.close_all",
            Action::WindowNew => "window.new",
            Action::WindowClose => "window.close",
            Action::ScrollbackLineUp => "scrollback.line_up",
            Action::ScrollbackLineDown => "scrollback.line_down",
            Action::ScrollbackPageUp => "scrollback.page_up",
            Action::ScrollbackPageDown => "scrollback.page_down",
            Action::ScrollbackToTop => "scrollback.to_top",
            Action::ScrollbackToBottom => "scrollback.to_bottom",
            Action::ClipboardCopy => "clipboard.copy",
            Action::ClipboardPaste => "clipboard.paste",
            Action::SelectionSelectAll => "selection.select_all",
            Action::FontIncrease => "font.increase",
            Action::FontDecrease => "font.decrease",
            Action::FontReset => "font.reset",
            Action::ThemeCycle => "theme.cycle",
            Action::ConfigReload => "config.reload",
            Action::SearchOpen => "search.open",
            Action::SearchNext => "search.next",
            Action::SearchPrev => "search.prev",
            Action::AppQuit => "app.quit",
        };
        f.write_str(name)
    }
}

/// Distância de Levenshtein, sem crate externo -- a tabela é pequena (46
/// nomes, todos curtos) e roda só no caminho de erro, nunca por frame.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, &ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let temp = row[j + 1];
            row[j + 1] = if ca == cb {
                prev
            } else {
                1 + prev.min(row[j]).min(row[j + 1])
            };
            prev = temp;
        }
    }
    row[b.len()]
}

/// Nome do catálogo mais próximo de `input` (ADR-0029 §4: "sugestão do
/// nome mais próximo"). O catálogo nunca é vazio, então sempre há um
/// candidato.
fn closest_name(input: &str) -> &'static str {
    CATALOG
        .iter()
        .min_by_key(|name| levenshtein(input, name))
        .expect("CATALOG não é vazio")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O bidirecional do ADR-0029: toda linha do catálogo (`CATALOG`, a
    /// mesma lista que alimenta `closest_name`) é uma variante que faz
    /// `from_str` -> `to_string` dar volta exata -- exceto as duas `Arg`,
    /// que `from_str` rejeita de propósito (não são vinculáveis) e por
    /// isso têm o próprio teste, `arg_actions_are_rejected_without_suggestion`.
    #[test]
    fn every_catalog_name_round_trips() {
        for &name in CATALOG {
            if name == "group.set_color" || name == "tab.move_to_group" {
                continue;
            }
            let action: Action = name.parse().unwrap_or_else(|e| {
                panic!("catálogo tem \"{name}\" mas Action::from_str falhou: {e}")
            });
            assert_eq!(action.to_string(), name, "Display não bate com o catálogo");
        }
    }

    /// A outra metade do bidirecional: toda variante monta um `Display`
    /// que está em `CATALOG` -- exceto as duas `Arg`, que o catálogo marca
    /// `Arg` e por isso ficam fora da lista vinculável.
    #[test]
    fn every_non_arg_variant_is_in_catalog() {
        let variants = [
            Action::TabNew,
            Action::TabClose,
            Action::TabNext,
            Action::TabPrev,
            Action::TabGoto(1),
            Action::TabGoto(9),
            Action::TabRename,
            Action::TabMoveLeft,
            Action::TabMoveRight,
            Action::GroupCreate,
            Action::GroupDissolve,
            Action::GroupRename,
            Action::GroupToggleCollapse,
            Action::GroupNext,
            Action::GroupPrev,
            Action::GroupNewTab,
            Action::GroupCloseAll,
            Action::WindowNew,
            Action::WindowClose,
            Action::ScrollbackLineUp,
            Action::ScrollbackLineDown,
            Action::ScrollbackPageUp,
            Action::ScrollbackPageDown,
            Action::ScrollbackToTop,
            Action::ScrollbackToBottom,
            Action::ClipboardCopy,
            Action::ClipboardPaste,
            Action::SelectionSelectAll,
            Action::FontIncrease,
            Action::FontDecrease,
            Action::FontReset,
            Action::ThemeCycle,
            Action::ConfigReload,
            Action::SearchOpen,
            Action::SearchNext,
            Action::SearchPrev,
            Action::AppQuit,
        ];
        for action in variants {
            let name = action.to_string();
            assert!(
                CATALOG.contains(&name.as_str()),
                "{name} não está em CATALOG"
            );
        }
    }

    #[test]
    fn tab_goto_out_of_range_is_unknown() {
        assert!("tab.goto_0".parse::<Action>().is_err());
        assert!("tab.goto_10".parse::<Action>().is_err());
        assert!("tab.goto_99".parse::<Action>().is_err());
    }

    #[test]
    fn arg_actions_are_rejected_without_suggestion() {
        let err = "group.set_color".parse::<Action>().unwrap_err();
        assert_eq!(err.suggestion, None);
        let err = "tab.move_to_group".parse::<Action>().unwrap_err();
        assert_eq!(err.suggestion, None);
    }

    #[test]
    fn unknown_action_suggests_closest() {
        let err = "tab.clsoe".parse::<Action>().unwrap_err();
        assert_eq!(err.suggestion, Some("tab.close"));
    }

    #[test]
    fn none_is_not_a_variant() {
        assert!("none".parse::<Action>().is_err());
    }

    #[test]
    fn display_move_to_group_and_set_color_still_name_correctly() {
        assert_eq!(
            Action::GroupSetColor(GroupColor::Red).to_string(),
            "group.set_color"
        );
        assert_eq!(
            Action::TabMoveToGroup(MoveDestination::NewGroup).to_string(),
            "tab.move_to_group"
        );
    }
}
