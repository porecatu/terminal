// SPDX-License-Identifier: GPL-3.0-or-later

//! Nomes legíveis das ações vinculáveis (ADR-0059 §5): o grupo Atalhos
//! (RF-16.28) é a primeira superfície que mostra uma ação a quem não a
//! conhece pelo identificador (`tab.new`), então cada uma ganha uma frase
//! (`action.*`) e cada domínio (`tab.*`, `group.*`, ...) um título
//! (`action_domain.*`).
//!
//! O `match` de [`label`] é exaustivo, sem braço curinga: ação nova em
//! `porecatu_core::Action` não compila até ganhar frase aqui, que é a forma de
//! o catálogo de textos nunca ficar atrás do catálogo de ações.

// Sem consumidor até a tarefa que desenha o grupo Atalhos (10).
#![allow(dead_code)]

use porecatu_core::{Action, CATALOG};
use porecatu_locale::Catalog;

use crate::messages::msg;

/// As ações que podem ser ligadas a uma tecla, na ordem do catálogo fechado
/// (`docs/reference/acoes.md`). É o que o `FromStr` aceita: as duas ações com
/// argumento (`group.set_color`, `tab.move_to_group`) são rejeitadas por ele,
/// então ficam de fora sem lista à parte para esquecer de manter.
pub(crate) fn bindable_actions() -> Vec<Action> {
    CATALOG
        .iter()
        .filter_map(|name| name.parse().ok())
        .collect()
}

/// A frase da ação, ou `None` para as duas que levam argumento e não são
/// vinculáveis.
pub(crate) fn label(catalog: &Catalog, action: Action) -> Option<String> {
    use msg::action as a;
    Some(match action {
        Action::TabNew => a::tab_new(catalog),
        Action::TabClose => a::tab_close(catalog),
        Action::TabNext => a::tab_next(catalog),
        Action::TabPrev => a::tab_prev(catalog),
        Action::TabGoto(n) => a::tab_goto(catalog, n),
        Action::TabRename => a::tab_rename(catalog),
        Action::TabMoveLeft => a::tab_move_left(catalog),
        Action::TabMoveRight => a::tab_move_right(catalog),
        Action::TabMoveToGroup(_) => return None,
        Action::GroupCreate => a::group_create(catalog),
        Action::GroupDissolve => a::group_dissolve(catalog),
        Action::GroupRename => a::group_rename(catalog),
        Action::GroupSetColor(_) => return None,
        Action::GroupToggleCollapse => a::group_toggle_collapse(catalog),
        Action::GroupNext => a::group_next(catalog),
        Action::GroupPrev => a::group_prev(catalog),
        Action::GroupNewTab => a::group_new_tab(catalog),
        Action::GroupCloseAll => a::group_close_all(catalog),
        Action::WindowNew => a::window_new(catalog),
        Action::WindowClose => a::window_close(catalog),
        Action::WindowToggleFullscreen => a::window_toggle_fullscreen(catalog),
        Action::ScrollbackLineUp => a::scrollback_line_up(catalog),
        Action::ScrollbackLineDown => a::scrollback_line_down(catalog),
        Action::ScrollbackPageUp => a::scrollback_page_up(catalog),
        Action::ScrollbackPageDown => a::scrollback_page_down(catalog),
        Action::ScrollbackToTop => a::scrollback_to_top(catalog),
        Action::ScrollbackToBottom => a::scrollback_to_bottom(catalog),
        Action::ClipboardCopy => a::clipboard_copy(catalog),
        Action::ClipboardPaste => a::clipboard_paste(catalog),
        Action::SelectionSelectAll => a::selection_select_all(catalog),
        Action::FontIncrease => a::font_increase(catalog),
        Action::FontDecrease => a::font_decrease(catalog),
        Action::FontReset => a::font_reset(catalog),
        Action::ThemeCycle => a::theme_cycle(catalog),
        Action::ConfigReload => a::config_reload(catalog),
        Action::SearchOpen => a::search_open(catalog),
        Action::SearchNext => a::search_next(catalog),
        Action::SearchPrev => a::search_prev(catalog),
        Action::AppQuit => a::app_quit(catalog),
        Action::PaneSplitHorizontal => a::pane_split_horizontal(catalog),
        Action::PaneSplitVertical => a::pane_split_vertical(catalog),
        Action::PaneClose => a::pane_close(catalog),
        Action::PaneFocusLeft => a::pane_focus_left(catalog),
        Action::PaneFocusRight => a::pane_focus_right(catalog),
        Action::PaneFocusUp => a::pane_focus_up(catalog),
        Action::PaneFocusDown => a::pane_focus_down(catalog),
        Action::SessionSaveNamed => a::session_save_named(catalog),
        Action::SessionOpenList => a::session_open_list(catalog),
        Action::SettingsOpen => a::settings_open(catalog),
    })
}

/// O domínio de uma ação: o prefixo do nome dela (`tab.new` → `tab`), que é
/// também a seção de `docs/reference/acoes.md` em que ela está.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Domain {
    Tab,
    Group,
    Window,
    Session,
    Pane,
    Scrollback,
    Clipboard,
    Selection,
    Font,
    Theme,
    Config,
    Settings,
    Search,
    App,
}

impl Domain {
    pub(crate) const ALL: [Domain; 14] = [
        Domain::Tab,
        Domain::Group,
        Domain::Window,
        Domain::Session,
        Domain::Pane,
        Domain::Scrollback,
        Domain::Clipboard,
        Domain::Selection,
        Domain::Font,
        Domain::Theme,
        Domain::Config,
        Domain::Settings,
        Domain::Search,
        Domain::App,
    ];

    /// O domínio de `action`, pelo prefixo do nome. Toda ação do catálogo tem
    /// um (testado); o prefixo desconhecido seria ação nova sem domínio.
    pub(crate) fn of(action: Action) -> Option<Domain> {
        let name = action.to_string();
        let prefix = name.split('.').next()?;
        Some(match prefix {
            "tab" => Domain::Tab,
            "group" => Domain::Group,
            "window" => Domain::Window,
            "session" => Domain::Session,
            "pane" => Domain::Pane,
            "scrollback" => Domain::Scrollback,
            "clipboard" => Domain::Clipboard,
            "selection" => Domain::Selection,
            "font" => Domain::Font,
            "theme" => Domain::Theme,
            "config" => Domain::Config,
            "settings" => Domain::Settings,
            "search" => Domain::Search,
            "app" => Domain::App,
            _ => return None,
        })
    }

    pub(crate) fn title(self, catalog: &Catalog) -> String {
        use msg::action_domain as d;
        match self {
            Domain::Tab => d::tab(catalog),
            Domain::Group => d::group(catalog),
            Domain::Window => d::window(catalog),
            Domain::Session => d::session(catalog),
            Domain::Pane => d::pane(catalog),
            Domain::Scrollback => d::scrollback(catalog),
            Domain::Clipboard => d::clipboard(catalog),
            Domain::Selection => d::selection(catalog),
            Domain::Font => d::font(catalog),
            Domain::Theme => d::theme(catalog),
            Domain::Config => d::config(catalog),
            Domain::Settings => d::settings(catalog),
            Domain::Search => d::search(catalog),
            Domain::App => d::app(catalog),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::messages::test_support;

    #[test]
    fn bindable_actions_are_the_catalog_minus_the_two_with_an_argument() {
        let actions = bindable_actions();
        assert_eq!(actions.len(), CATALOG.len() - 2);
        assert!(!actions.iter().any(|a| a.to_string() == "group.set_color"));
        assert!(!actions.iter().any(|a| a.to_string() == "tab.move_to_group"));
    }

    /// Toda ação vinculável tem frase, e a frase é a do arquivo -- não o
    /// identificador que o acessor devolve quando falta.
    #[test]
    fn every_bindable_action_has_a_phrase_in_every_language() {
        for locale in ["pt_BR", "en_US", "es_ES", "fr_FR", "de_DE"] {
            let catalog = test_support::catalog(locale);
            for action in bindable_actions() {
                let phrase =
                    label(&catalog, action).unwrap_or_else(|| panic!("{action}: sem frase"));
                assert!(!phrase.is_empty(), "{locale} {action}");
                assert!(
                    !phrase.starts_with("action."),
                    "{locale} {action}: frase ausente ({phrase})"
                );
            }
        }
    }

    #[test]
    fn the_phrases_are_distinct_per_action() {
        let catalog = test_support::pt_br();
        let mut seen = HashSet::new();
        for action in bindable_actions() {
            let phrase = label(&catalog, action).unwrap();
            assert!(
                seen.insert(phrase.clone()),
                "{action}: frase repetida {phrase}"
            );
        }
    }

    #[test]
    fn the_actions_with_an_argument_have_no_phrase() {
        let catalog = test_support::pt_br();
        for name in ["group.set_color", "tab.move_to_group"] {
            assert!(name.parse::<Action>().is_err());
        }
        assert_eq!(
            label(
                &catalog,
                Action::TabMoveToGroup(porecatu_core::MoveDestination::NewGroup)
            ),
            None
        );
        assert_eq!(
            label(
                &catalog,
                Action::GroupSetColor(porecatu_core::GroupColor::Red)
            ),
            None
        );
    }

    #[test]
    fn tab_goto_names_its_number() {
        let catalog = test_support::pt_br();
        assert_eq!(
            label(&catalog, Action::TabGoto(3)).unwrap(),
            "Ir para a aba 3"
        );
        assert_eq!(
            label(&test_support::en_us(), Action::TabGoto(9)).unwrap(),
            "Go to tab 9"
        );
    }

    #[test]
    fn the_reference_examples_read_as_the_task_asks() {
        let catalog = test_support::pt_br();
        assert_eq!(label(&catalog, Action::TabNew).unwrap(), "Nova aba");
        assert_eq!(Domain::Tab.title(&catalog), "Abas");
    }

    #[test]
    fn every_bindable_action_belongs_to_a_domain_with_a_title() {
        for locale in ["pt_BR", "en_US", "es_ES", "fr_FR", "de_DE"] {
            let catalog = test_support::catalog(locale);
            for action in bindable_actions() {
                let domain = Domain::of(action).unwrap_or_else(|| panic!("{action}: sem domínio"));
                let title = domain.title(&catalog);
                assert!(!title.starts_with("action_domain."), "{locale} {domain:?}");
            }
        }
    }

    #[test]
    fn every_domain_is_used_by_some_action() {
        let used: HashSet<Domain> = bindable_actions()
            .into_iter()
            .filter_map(Domain::of)
            .collect();
        assert_eq!(used.len(), Domain::ALL.len());
    }
}
