// SPDX-License-Identifier: GPL-3.0-or-later

//! As decisões puras de fechar com pendências (RF-16.4, RF-16.5, ADR-0059
//! §2): quando o encerramento do app espera pela janela de configurações, e o
//! que a resposta ao diálogo de três saídas faz com o que esperava. Sem
//! janela e sem `winit` -- quem as executa é `App`, que tem o `event_loop` e
//! as janelas.

use super::window::DialogAnswer;

/// O encerramento do app espera a janela de configurações: a janela de
/// terminal que ia fechar é a última (`terminal_windows == 1`) e a tela tem
/// alterações não gravadas. Com mais de uma janela de terminal, ou sem
/// pendências, o fechamento segue direto.
pub(crate) fn defers_quit(terminal_windows: usize, settings_has_pending: bool) -> bool {
    terminal_windows == 1 && settings_has_pending
}

/// O que acontece depois da resposta ao diálogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Conclusion {
    /// Nada: a janela de configurações e a de terminal seguem abertas. O
    /// encerramento que esperava, se havia, está cancelado.
    Stay,
    /// Fecha só a janela de configurações.
    CloseSettings,
    /// Conclui o encerramento que esperava: a janela de terminal fecha, e com
    /// ela o app (com a gravação de sessão de sempre).
    Quit,
}

/// A conclusão da resposta `answer`. `quitting` diz se o diálogo veio de um
/// encerramento adiado. `save` roda **só** em "Salvar e fechar" e diz se a
/// gravação deu certo: falha deixa tudo como está, com as pendências.
pub(crate) fn conclude(
    answer: DialogAnswer,
    quitting: bool,
    save: impl FnOnce() -> bool,
) -> Conclusion {
    let finished = match answer {
        DialogAnswer::Cancel => return Conclusion::Stay,
        DialogAnswer::Discard => true,
        DialogAnswer::Save => save(),
    };
    match (finished, quitting) {
        (false, _) => Conclusion::Stay,
        (true, true) => Conclusion::Quit,
        (true, false) => Conclusion::CloseSettings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_last_terminal_window_with_pending_changes_defers_the_quit() {
        assert!(defers_quit(1, true));
        assert!(!defers_quit(1, false), "sem pendências encerra direto");
        assert!(!defers_quit(2, true), "outra janela segura o processo");
        assert!(!defers_quit(0, true));
    }

    #[test]
    fn cancel_never_closes_anything_and_never_saves() {
        for quitting in [false, true] {
            let conclusion = conclude(DialogAnswer::Cancel, quitting, || {
                panic!("cancelar não grava")
            });
            assert_eq!(conclusion, Conclusion::Stay);
        }
    }

    #[test]
    fn discard_finishes_what_was_waiting_without_saving() {
        assert_eq!(
            conclude(DialogAnswer::Discard, false, || panic!(
                "descartar não grava"
            )),
            Conclusion::CloseSettings
        );
        assert_eq!(
            conclude(DialogAnswer::Discard, true, || panic!(
                "descartar não grava"
            )),
            Conclusion::Quit
        );
    }

    #[test]
    fn save_finishes_only_when_the_write_worked() {
        assert_eq!(
            conclude(DialogAnswer::Save, false, || true),
            Conclusion::CloseSettings
        );
        assert_eq!(
            conclude(DialogAnswer::Save, true, || true),
            Conclusion::Quit
        );
        // Falha: as pendências ficam, a tela fica, o encerramento se cancela.
        assert_eq!(
            conclude(DialogAnswer::Save, false, || false),
            Conclusion::Stay
        );
        assert_eq!(
            conclude(DialogAnswer::Save, true, || false),
            Conclusion::Stay
        );
    }
}
