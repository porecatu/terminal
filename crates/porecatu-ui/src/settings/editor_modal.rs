// SPDX-License-Identifier: GPL-3.0-or-later

//! As decisões puras da modalidade do editor de perfil (ADR-0080 §2, §6): o
//! que a tela de configurações faz com o input enquanto o editor existe, e o
//! que acontece com o editor quando a tela fecha. Sem janela e sem `winit` --
//! quem as executa é `SettingsWindow`, que tem as duas janelas.

/// Um evento de entrada que chega à tela de configurações.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Input {
    Key,
    MouseMove,
    MousePress,
    MouseRelease,
    Wheel,
}

/// O que a tela faz com um evento de entrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Segue o caminho de sempre.
    Pass,
    /// A modalidade é lógica (macOS, Wayland, X11: o SO não desabilita a tela):
    /// o evento é descartado e nada muda no rascunho. Um clique, além disso,
    /// traz o editor para a frente.
    Discard { bring_editor_to_front: bool },
}

/// Com o editor aberto, a tela descarta todo input de teclado e de mouse. No
/// Windows o SO já a desabilita e nada chega; esta é a defesa das outras
/// plataformas (e do que escapa).
pub(crate) fn gate(editor_open: bool, input: Input) -> Gate {
    if !editor_open {
        return Gate::Pass;
    }
    Gate::Discard {
        bring_editor_to_front: input == Input::MousePress,
    }
}

/// Por que a tela de configurações vai fechar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseReason {
    /// O botão de fechar, `Esc` ou o gesto do sistema (onde a tela não está
    /// desabilitada).
    Request,
    /// O app vai encerrar (a última janela de terminal fechou, ou `app.quit`).
    Quit,
}

/// Fechar a tela com o editor aberto fecha o editor **como Cancelar** primeiro
/// (§6); só então segue o fluxo de pendências do ADR-0059 §2, com o que já
/// estava no rascunho. A cópia do editor nunca é pendência.
pub(crate) fn closes_editor_first(editor_open: bool, _reason: CloseReason) -> bool {
    editor_open
}

#[cfg(test)]
mod tests {
    use super::*;

    const INPUTS: [Input; 5] = [
        Input::Key,
        Input::MouseMove,
        Input::MousePress,
        Input::MouseRelease,
        Input::Wheel,
    ];

    #[test]
    fn with_the_editor_open_the_screen_discards_every_keyboard_and_mouse_input() {
        for input in INPUTS {
            assert!(
                matches!(gate(true, input), Gate::Discard { .. }),
                "{input:?} must not reach the screen"
            );
        }
    }

    #[test]
    fn only_a_click_brings_the_editor_to_the_front() {
        for input in INPUTS {
            assert_eq!(
                gate(true, input),
                Gate::Discard {
                    bring_editor_to_front: input == Input::MousePress
                },
                "{input:?}"
            );
        }
    }

    #[test]
    fn without_the_editor_nothing_is_discarded() {
        for input in INPUTS {
            assert_eq!(gate(false, input), Gate::Pass, "{input:?}");
        }
    }

    #[test]
    fn closing_the_screen_cancels_the_open_editor_first_whatever_the_reason() {
        for reason in [CloseReason::Request, CloseReason::Quit] {
            assert!(closes_editor_first(true, reason), "{reason:?}");
            assert!(!closes_editor_first(false, reason), "{reason:?}");
        }
    }
}
