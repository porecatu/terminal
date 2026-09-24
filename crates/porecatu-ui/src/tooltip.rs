// SPDX-License-Identifier: GPL-3.0-or-later

//! Hover e tooltip (ADR-0019, RF-1.10): só aparece pra alvo com texto
//! truncado, depois de 600ms de hover parado. F2 só tem alvo de aba
//! (rótulo); grupo (RF-2.12) é F3. Desde o ADR-0055 §2, também a linha
//! do popover de sessões (`HoverKey::SessionRow`) -- `Hover` não conhece
//! ali `TabId` como o único tipo de alvo possível, só um `HoverKey` que
//! distingue os dois por igualdade, o que é tudo que a máquina de estado
//! abaixo precisa.
//!
//! `Instant::now()` não aparece aqui, pelo mesmo motivo de `warning.rs`:
//! quem chama passa `now`, o que torna o atraso testável sem dormir.

use std::time::{Duration, Instant};

use porecatu_core::TabId;
use porecatu_render::Rect;

/// Espec. §2.20: "após 600ms de hover parado".
pub const HOVER_DELAY: Duration = Duration::from_millis(600);

/// O que está sob hover -- aba (rótulo truncado) ou linha do popover de
/// sessões (nome truncado, ADR-0055 §2). Um `enum` em vez de genérico
/// porque só há dois consumidores, e um genérico não pagaria pela
/// simplicidade perdida no resto do módulo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverKey {
    Tab(TabId),
    SessionRow(usize),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Hover {
    #[default]
    None,
    Pending {
        key: HoverKey,
        anchor: Rect,
        text: String,
        since: Instant,
    },
    Shown {
        key: HoverKey,
        anchor: Rect,
        text: String,
    },
}

impl Hover {
    /// Atualiza a partir do que está sob o cursor agora -- `None` quando
    /// não há alvo elegível (fora de qualquer aba truncada, ou aba não
    /// truncada: ADR-0019 "aba cujo título cabe inteiro não tem tooltip").
    /// Mudar de alvo reinicia o atraso; o mesmo alvo atualiza a geometria
    /// (a trilha pode ter rolado, ou a lista de sessões) sem reiniciar.
    pub fn update(&mut self, target: Option<(HoverKey, Rect, String)>, now: Instant) {
        let Some((key, anchor, text)) = target else {
            *self = Hover::None;
            return;
        };
        match self {
            Hover::None => {
                *self = Hover::Pending {
                    key,
                    anchor,
                    text,
                    since: now,
                };
            }
            Hover::Pending {
                key: cur,
                anchor: a,
                text: t,
                ..
            } => {
                if *cur == key {
                    *a = anchor;
                    *t = text;
                } else {
                    *self = Hover::Pending {
                        key,
                        anchor,
                        text,
                        since: now,
                    };
                }
            }
            Hover::Shown {
                key: cur,
                anchor: a,
                text: t,
            } => {
                if *cur == key {
                    *a = anchor;
                    *t = text;
                } else {
                    *self = Hover::Pending {
                        key,
                        anchor,
                        text,
                        since: now,
                    };
                }
            }
        }
    }

    /// Promove `Pending` a `Shown` quando o atraso passou.
    pub fn tick(&mut self, now: Instant) {
        if let Hover::Pending {
            key,
            anchor,
            text,
            since,
        } = self
            && now.duration_since(*since) >= HOVER_DELAY
        {
            *self = Hover::Shown {
                key: *key,
                anchor: *anchor,
                text: std::mem::take(text),
            };
        }
    }

    /// Dispensa por qualquer um dos gatilhos do ADR-0019: clicar, digitar,
    /// começar arraste, a janela perder foco, o alvo deixar de existir.
    pub fn dismiss(&mut self) {
        *self = Hover::None;
    }

    pub fn visible(&self) -> Option<(Rect, &str)> {
        match self {
            Hover::Shown { anchor, text, .. } => Some((*anchor, text.as_str())),
            _ => None,
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        match self {
            Hover::Pending { since, .. } => Some(*since + HOVER_DELAY),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(millis: u64) -> Instant {
        Instant::now() + Duration::from_millis(millis)
    }

    fn rect() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        }
    }

    fn tab(n: u32) -> HoverKey {
        HoverKey::Tab(TabId::new(n))
    }

    #[test]
    fn shows_after_delay_elapses() {
        let mut hover = Hover::default();
        hover.update(Some((tab(0), rect(), "titulo".into())), t(0));
        hover.tick(t(599));
        assert_eq!(hover.visible(), None);
        hover.tick(t(600));
        assert_eq!(hover.visible(), Some((rect(), "titulo")));
    }

    #[test]
    fn switching_target_restarts_the_delay() {
        let mut hover = Hover::default();
        hover.update(Some((tab(0), rect(), "a".into())), t(0));
        hover.update(Some((tab(1), rect(), "b".into())), t(500));
        hover.tick(t(600)); // só 100ms desde o segundo alvo
        assert_eq!(hover.visible(), None);
        hover.tick(t(1100));
        assert_eq!(hover.visible(), Some((rect(), "b")));
    }

    #[test]
    fn losing_the_target_dismisses_immediately() {
        let mut hover = Hover::default();
        hover.update(Some((tab(0), rect(), "a".into())), t(0));
        hover.tick(t(700));
        assert!(hover.visible().is_some());
        hover.update(None, t(701));
        assert_eq!(hover.visible(), None);
    }

    #[test]
    fn dismiss_clears_regardless_of_state() {
        let mut hover = Hover::default();
        hover.update(Some((tab(0), rect(), "a".into())), t(0));
        hover.tick(t(700));
        assert!(hover.visible().is_some());
        hover.dismiss();
        assert_eq!(hover, Hover::None);
        assert_eq!(hover.next_deadline(), None);
    }

    /// Diferente de `TabId`, não é o único tipo de chave possível -- uma
    /// linha do popover de sessões (ADR-0055 §2) muda de alvo mesmo com
    /// `text`/`rect` iguais a uma aba, porque a chave é o que decide
    /// "é o mesmo alvo", não o conteúdo mostrado.
    #[test]
    fn a_session_row_and_a_tab_are_never_the_same_target() {
        let mut hover = Hover::default();
        hover.update(Some((tab(0), rect(), "igual".into())), t(0));
        hover.update(
            Some((HoverKey::SessionRow(0), rect(), "igual".into())),
            t(100),
        );
        hover.tick(t(699)); // 599ms desde a troca -- ainda não mostrou
        assert_eq!(hover.visible(), None);
        hover.tick(t(700));
        assert_eq!(hover.visible(), Some((rect(), "igual")));
    }
}
