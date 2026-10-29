// SPDX-License-Identifier: GPL-3.0-or-later

//! As teclas do diálogo de segredo (ADR-0075 §2). O estado é dado puro em
//! [`ConfirmDialog::secret`] (foco e caixa "Lembrar") mais o campo mascarado, que
//! é o único pedaço que **não** pode morar no `ConfirmDialog` (`Clone`,
//! `PartialEq`): por isso as funções daqui recebem os dois. Testável sem `winit`
//! nem GPU.

use porecatu_term::Modifiers;
use winit::keyboard::{Key, NamedKey};

use crate::dialog::{ConfirmDialog, DialogButton, SecretFocus};
use crate::masked_field::{MaskedFieldState, MaskedKey, apply_masked_key};

/// O que uma tecla pede a quem tem o diálogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKey {
    None,
    /// Conectar: confirma com o texto do campo.
    Submit,
    Cancel,
    /// Colar: quem chama busca a área de transferência.
    Paste,
}

/// Traduz uma tecla. `Enter` no campo confirma (e nunca com o campo vazio),
/// `Esc` cancela. Não filtra `event.state`; quem chama já garantiu `Pressed`.
pub fn apply_secret_key(
    field: &mut MaskedFieldState,
    dialog: &mut ConfirmDialog,
    logical_key: &Key,
    text: Option<&str>,
    modifiers: Modifiers,
) -> SecretKey {
    let Some(focus) = dialog.secret.as_ref().map(|facts| facts.focus) else {
        return SecretKey::None;
    };
    match logical_key {
        Key::Named(NamedKey::Escape) => SecretKey::Cancel,
        Key::Named(NamedKey::Tab) => {
            dialog.step_secret_focus(if modifiers.shift { -1 } else { 1 });
            SecretKey::None
        }
        Key::Named(NamedKey::Enter) => {
            if focus == SecretFocus::Buttons && dialog.focused() == DialogButton::Cancel {
                SecretKey::Cancel
            } else if field.is_empty() {
                // Nada digitado: não há o que conectar.
                SecretKey::None
            } else {
                SecretKey::Submit
            }
        }
        _ => match focus {
            SecretFocus::Field => match apply_masked_key(field, logical_key, text, modifiers) {
                MaskedKey::Paste => SecretKey::Paste,
                MaskedKey::Handled | MaskedKey::Ignored => SecretKey::None,
            },
            SecretFocus::Remember => {
                let space = matches!(logical_key, Key::Named(NamedKey::Space))
                    || matches!(logical_key, Key::Character(c) if c.as_str() == " ");
                if space {
                    dialog.toggle_remember();
                }
                SecretKey::None
            }
            SecretFocus::Buttons => {
                if matches!(
                    logical_key,
                    Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight)
                ) {
                    dialog.toggle_focus();
                }
                SecretKey::None
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use porecatu_core::{PaneId, TabId};
    use porecatu_term::{PromptId, SecretLabel};

    use super::*;
    use crate::dialog::DialogAction;
    use crate::messages::test_support;

    fn mods() -> Modifiers {
        Modifiers {
            shift: false,
            ctrl: false,
            alt: false,
            super_: false,
        }
    }

    fn action() -> DialogAction {
        DialogAction::Secret(TabId::new(1), PaneId::new(2), PromptId::new(3))
    }

    fn password_dialog(vault: bool, retry: bool) -> ConfirmDialog {
        ConfirmDialog::secret(
            &test_support::pt_br(),
            &SecretLabel::Password {
                user: "deploy".into(),
                host: "10.4.1.20".into(),
            },
            vault,
            retry,
            action(),
        )
    }

    fn facts(dialog: &ConfirmDialog) -> &crate::dialog::SecretFacts {
        dialog.secret.as_ref().unwrap()
    }

    fn press(
        field: &mut MaskedFieldState,
        dialog: &mut ConfirmDialog,
        key: Key,
        text: Option<&str>,
    ) -> SecretKey {
        apply_secret_key(field, dialog, &key, text, mods())
    }

    fn type_text(field: &mut MaskedFieldState, dialog: &mut ConfirmDialog, text: &str) {
        for c in text.chars() {
            let s = c.to_string();
            press(field, dialog, Key::Character(s.as_str().into()), Some(&s));
        }
    }

    #[test]
    fn the_initial_focus_is_the_field_and_the_box_starts_unticked() {
        let dialog = password_dialog(true, false);
        assert_eq!(facts(&dialog).focus, SecretFocus::Field);
        assert!(!facts(&dialog).remember);
    }

    #[test]
    fn enter_in_the_field_connects_and_escape_cancels() {
        let mut field = MaskedFieldState::new();
        let mut dialog = password_dialog(true, false);
        type_text(&mut field, &mut dialog, "s3cret");
        assert_eq!(
            press(&mut field, &mut dialog, Key::Named(NamedKey::Enter), None),
            SecretKey::Submit
        );
        assert_eq!(
            press(&mut field, &mut dialog, Key::Named(NamedKey::Escape), None),
            SecretKey::Cancel
        );
    }

    #[test]
    fn enter_with_nothing_typed_does_not_connect() {
        let mut field = MaskedFieldState::new();
        let mut dialog = password_dialog(true, false);
        assert_eq!(
            press(&mut field, &mut dialog, Key::Named(NamedKey::Enter), None),
            SecretKey::None
        );
    }

    #[test]
    fn typing_goes_into_the_field_and_a_space_is_part_of_the_password() {
        let mut field = MaskedFieldState::new();
        let mut dialog = password_dialog(true, false);
        type_text(&mut field, &mut dialog, "a");
        press(
            &mut field,
            &mut dialog,
            Key::Named(NamedKey::Space),
            Some(" "),
        );
        type_text(&mut field, &mut dialog, "b");
        assert_eq!(field.len(), 3);
        assert_eq!(field.take().expose(), "a b");
    }

    #[test]
    fn tab_walks_field_box_cancel_connect_and_around() {
        let mut field = MaskedFieldState::new();
        let mut dialog = password_dialog(true, false);
        let mut seen = Vec::new();
        for _ in 0..5 {
            seen.push((facts(&dialog).focus, dialog.focused()));
            press(&mut field, &mut dialog, Key::Named(NamedKey::Tab), None);
        }
        assert_eq!(
            seen,
            [
                (SecretFocus::Field, DialogButton::Cancel),
                (SecretFocus::Remember, DialogButton::Cancel),
                (SecretFocus::Buttons, DialogButton::Cancel),
                (SecretFocus::Buttons, DialogButton::Confirm),
                (SecretFocus::Field, DialogButton::Confirm),
            ]
        );
    }

    #[test]
    fn without_a_vault_tab_skips_the_box_and_space_does_not_tick_it() {
        let mut field = MaskedFieldState::new();
        let mut dialog = password_dialog(false, false);
        press(&mut field, &mut dialog, Key::Named(NamedKey::Tab), None);
        assert_eq!(facts(&dialog).focus, SecretFocus::Buttons);

        dialog.focus_secret(SecretFocus::Remember);
        press(
            &mut field,
            &mut dialog,
            Key::Named(NamedKey::Space),
            Some(" "),
        );
        assert!(
            !facts(&dialog).remember,
            "the box is disabled without a vault"
        );
    }

    #[test]
    fn space_on_the_box_ticks_and_unticks_it() {
        let mut field = MaskedFieldState::new();
        let mut dialog = password_dialog(true, false);
        press(&mut field, &mut dialog, Key::Named(NamedKey::Tab), None);
        assert_eq!(facts(&dialog).focus, SecretFocus::Remember);
        press(
            &mut field,
            &mut dialog,
            Key::Named(NamedKey::Space),
            Some(" "),
        );
        assert!(facts(&dialog).remember);
        press(
            &mut field,
            &mut dialog,
            Key::Named(NamedKey::Space),
            Some(" "),
        );
        assert!(!facts(&dialog).remember);
        assert!(field.is_empty(), "the space did not go to the field");
    }

    #[test]
    fn enter_on_cancel_cancels_and_on_connect_connects() {
        let mut field = MaskedFieldState::new();
        let mut dialog = password_dialog(false, false);
        type_text(&mut field, &mut dialog, "x");
        press(&mut field, &mut dialog, Key::Named(NamedKey::Tab), None);
        assert_eq!(
            press(&mut field, &mut dialog, Key::Named(NamedKey::Enter), None),
            SecretKey::Cancel
        );
        press(
            &mut field,
            &mut dialog,
            Key::Named(NamedKey::ArrowRight),
            None,
        );
        assert_eq!(dialog.focused(), DialogButton::Confirm);
        assert_eq!(
            press(&mut field, &mut dialog, Key::Named(NamedKey::Enter), None),
            SecretKey::Submit
        );
    }

    #[test]
    fn ctrl_v_asks_for_the_clipboard_and_ctrl_c_does_nothing() {
        let mut field = MaskedFieldState::new();
        let mut dialog = password_dialog(true, false);
        type_text(&mut field, &mut dialog, "kept");
        let primary = Modifiers {
            ctrl: !cfg!(target_os = "macos"),
            super_: cfg!(target_os = "macos"),
            ..mods()
        };
        assert_eq!(
            apply_secret_key(
                &mut field,
                &mut dialog,
                &Key::Character("v".into()),
                None,
                primary
            ),
            SecretKey::Paste
        );
        field.select_all();
        assert_eq!(
            apply_secret_key(
                &mut field,
                &mut dialog,
                &Key::Character("c".into()),
                None,
                primary
            ),
            SecretKey::None
        );
        assert_eq!(field.take().expose(), "kept");
    }

    #[test]
    fn the_password_dialog_names_the_destination_and_a_retry_swaps_the_body_for_the_error() {
        let fresh = password_dialog(true, false);
        assert_eq!(fresh.title, "Senha para deploy@10.4.1.20");
        assert_eq!(fresh.body, "deploy@10.4.1.20");
        assert!(!fresh.body_is_error);
        assert_eq!(fresh.confirm_label, "Conectar");
        assert_eq!(fresh.cancel_label, "Cancelar");

        let retry = password_dialog(true, true);
        assert_eq!(retry.title, "Senha para deploy@10.4.1.20");
        assert_eq!(retry.body, "O servidor recusou a senha.");
        assert!(retry.body_is_error);
    }

    #[test]
    fn the_passphrase_dialog_shows_the_key_path_and_a_retry_swaps_it_for_the_error() {
        let key = SecretLabel::KeyPassphrase {
            path: "/home/me/.ssh/id_ed25519".into(),
        };
        let dialog = ConfirmDialog::secret(&test_support::pt_br(), &key, true, false, action());
        assert_eq!(dialog.title, "Passphrase da chave");
        assert_eq!(dialog.body, "/home/me/.ssh/id_ed25519");

        let retry = ConfirmDialog::secret(&test_support::pt_br(), &key, true, true, action());
        assert_eq!(retry.body, "A passphrase não abre a chave.");
    }

    #[test]
    fn the_unavailable_note_and_the_label_come_from_the_catalog() {
        let dialog = password_dialog(false, false);
        let facts = dialog.secret.unwrap();
        assert!(!facts.vault_available);
        assert_eq!(facts.remember_label, "Lembrar no cofre do sistema");
        assert_eq!(
            facts.unavailable_note,
            "O cofre do sistema não está disponível."
        );
    }
}
