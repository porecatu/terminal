// SPDX-License-Identifier: GPL-3.0-or-later

//! As frases-base da janela de configurações que as tarefas seguintes vão
//! usar -- botões, escopos de recarga, faixas, diálogo de pendências,
//! validação, tema e atalho (ADR-0059 §5). Entram no registro de mensagens e
//! nos cinco `locales/` antes de quem as usa, e este módulo as exercita: nada
//! aqui desenha, mas [`all`] chama todos os acessores, com argumentos de
//! amostra, e o teste lê o resultado nos cinco idiomas.

// Sem consumidor até as tarefas que desenham botões, faixas e diálogo.
#![allow(dead_code)]

use porecatu_locale::Catalog;

use crate::messages::msg::settings as s;

/// Toda frase-base, como `(identificador, texto)`. O texto é o que o
/// catálogo devolve; se faltasse, seria o próprio identificador.
pub(crate) fn all(catalog: &Catalog) -> Vec<(&'static str, String)> {
    vec![
        ("settings.button.open_file", s::button::open_file(catalog)),
        ("settings.button.discard", s::button::discard(catalog)),
        ("settings.button.save", s::button::save(catalog)),
        (
            "settings.button.restore_default",
            s::button::restore_default(catalog),
        ),
        ("settings.button.add_item", s::button::add_item(catalog)),
        (
            "settings.button.remove_item",
            s::button::remove_item(catalog),
        ),
        ("settings.scope.new_tab", s::scope::new_tab(catalog)),
        ("settings.scope.next_window", s::scope::next_window(catalog)),
        ("settings.scope.restart", s::scope::restart(catalog)),
        ("settings.scope.next_open", s::scope::next_open(catalog)),
        (
            "settings.banner.file_changed",
            s::banner::file_changed(catalog),
        ),
        ("settings.banner.reload", s::banner::reload(catalog)),
        ("settings.banner.keep_mine", s::banner::keep_mine(catalog)),
        (
            "settings.banner.invalid_file",
            s::banner::invalid_file(catalog),
        ),
        (
            "settings.dialog.close_title",
            s::dialog::close_title(catalog),
        ),
        (
            "settings.dialog.close_body",
            s::dialog::close_body(catalog, 1),
        ),
        (
            "settings.dialog.save_and_close",
            s::dialog::save_and_close(catalog),
        ),
        (
            "settings.dialog.discard_and_close",
            s::dialog::discard_and_close(catalog),
        ),
        ("settings.dialog.cancel", s::dialog::cancel(catalog)),
        (
            "settings.validation.out_of_range",
            s::validation::out_of_range(catalog, 6, 72),
        ),
        (
            "settings.validation.not_a_number",
            s::validation::not_a_number(catalog),
        ),
        (
            "settings.validation.not_an_integer",
            s::validation::not_an_integer(catalog),
        ),
        (
            "settings.validation.not_a_choice",
            s::validation::not_a_choice(catalog),
        ),
        (
            "settings.validation.empty_name",
            s::validation::empty_name(catalog),
        ),
        (
            "settings.validation.duplicate_name",
            s::validation::duplicate_name(catalog, "EDITOR"),
        ),
        (
            "settings.option.trusted_paths_warning",
            s::option::trusted_paths_warning(catalog),
        ),
        ("settings.theme.none", s::theme::none(catalog)),
        (
            "settings.theme.session_using",
            s::theme::session_using(catalog, "Solar"),
        ),
        ("settings.shortcut.none", s::shortcut::none(catalog)),
        (
            "settings.shortcut.press_keys",
            s::shortcut::press_keys(catalog),
        ),
        (
            "settings.shortcut.conflict",
            s::shortcut::conflict(catalog, "Sample"),
        ),
        ("settings.shortcut.replace", s::shortcut::replace(catalog)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::test_support;

    const LOCALES: [&str; 5] = ["pt_BR", "en_US", "es_ES", "fr_FR", "de_DE"];

    #[test]
    fn every_base_phrase_exists_in_every_language() {
        for locale in LOCALES {
            let catalog = test_support::catalog(locale);
            for (id, text) in all(&catalog) {
                assert!(!text.is_empty(), "{locale} {id}");
                assert_ne!(text, id, "{locale}: frase ausente {id}");
            }
        }
    }

    #[test]
    fn markers_are_filled_with_the_arguments() {
        for locale in LOCALES {
            let catalog = test_support::catalog(locale);
            let phrases = all(&catalog);
            let text = |id: &str| phrases.iter().find(|(i, _)| *i == id).unwrap().1.clone();
            let range = text("settings.validation.out_of_range");
            assert!(
                range.contains('6') && range.contains("72"),
                "{locale}: {range}"
            );
            assert!(text("settings.validation.duplicate_name").contains("EDITOR"));
            assert!(text("settings.theme.session_using").contains("Solar"));
            assert!(text("settings.shortcut.conflict").contains("Sample"));
            assert!(
                phrases
                    .iter()
                    .all(|(_, text)| !text.contains('{') && !text.contains('}')),
                "{locale}: marcador sem preencher"
            );
        }
    }

    #[test]
    fn the_pending_changes_phrase_picks_singular_and_plural() {
        let pt = test_support::pt_br();
        assert_eq!(s::dialog::close_body(&pt, 1), "Há 1 alteração não salva.");
        assert_eq!(s::dialog::close_body(&pt, 3), "Há 3 alterações não salvas.");
    }
}
