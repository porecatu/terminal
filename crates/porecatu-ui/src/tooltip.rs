// SPDX-License-Identifier: GPL-3.0-or-later

//! Hover e tooltip (ADR-0019, RF-1.10): só aparece pra alvo com texto
//! truncado, depois de 600ms de hover parado. F2 só tem alvo de aba
//! (rótulo); grupo (RF-2.12) é F3.
//!
//! `Instant::now()` não aparece aqui, pelo mesmo motivo de `warning.rs`:
//! quem chama passa `now`, o que torna o atraso testável sem dormir.

use std::time::{Duration, Instant};

use porecatu_core::TabId;
use porecatu_render::Rect;

/// Espec. §2.20: "após 600ms de hover parado".
pub const HOVER_DELAY: Duration = Duration::from_millis(600);

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Hover {
    #[default]
    None,
    Pending {
        tab: TabId,
        anchor: Rect,
        text: String,
        since: Instant,
    },
    Shown {
        tab: TabId,
        anchor: Rect,
        text: String,
    },
}

impl Hover {
    /// Atualiza a partir do que está sob o cursor agora -- `None` quando
    /// não há alvo elegível (fora de qualquer aba truncada, ou aba não
    /// truncada: ADR-0019 "aba cujo título cabe inteiro não tem tooltip").
    /// Mudar de alvo reinicia o atraso; o mesmo alvo atualiza a geometria
    /// (a trilha pode ter rolado) sem reiniciar.
    pub fn update(&mut self, target: Option<(TabId, Rect, String)>, now: Instant) {
        let Some((tab, anchor, text)) = target else {
            *self = Hover::None;
            return;
        };
        match self {
            Hover::None => {
                *self = Hover::Pending {
                    tab,
                    anchor,
                    text,
                    since: now,
                };
            }
            Hover::Pending {
                tab: cur,
                anchor: a,
                text: t,
                ..
            } => {
                if *cur == tab {
                    *a = anchor;
                    *t = text;
                } else {
                    *self = Hover::Pending {
                        tab,
                        anchor,
                        text,
                        since: now,
                    };
                }
            }
            Hover::Shown {
                tab: cur,
                anchor: a,
                text: t,
            } => {
                if *cur == tab {
                    *a = anchor;
                    *t = text;
                } else {
                    *self = Hover::Pending {
                        tab,
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
            tab,
            anchor,
            text,
            since,
        } = self
            && now.duration_since(*since) >= HOVER_DELAY
        {
            *self = Hover::Shown {
                tab: *tab,
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

    #[test]
    fn shows_after_delay_elapses() {
        let mut hover = Hover::default();
        hover.update(Some((TabId::new(0), rect(), "titulo".into())), t(0));
        hover.tick(t(599));
        assert_eq!(hover.visible(), None);
        hover.tick(t(600));
        assert_eq!(hover.visible(), Some((rect(), "titulo")));
    }

    #[test]
    fn switching_target_restarts_the_delay() {
        let mut hover = Hover::default();
        hover.update(Some((TabId::new(0), rect(), "a".into())), t(0));
        hover.update(Some((TabId::new(1), rect(), "b".into())), t(500));
        hover.tick(t(600)); // só 100ms desde o segundo alvo
        assert_eq!(hover.visible(), None);
        hover.tick(t(1100));
        assert_eq!(hover.visible(), Some((rect(), "b")));
    }

    #[test]
    fn losing_the_target_dismisses_immediately() {
        let mut hover = Hover::default();
        hover.update(Some((TabId::new(0), rect(), "a".into())), t(0));
        hover.tick(t(700));
        assert!(hover.visible().is_some());
        hover.update(None, t(701));
        assert_eq!(hover.visible(), None);
    }

    #[test]
    fn dismiss_clears_regardless_of_state() {
        let mut hover = Hover::default();
        hover.update(Some((TabId::new(0), rect(), "a".into())), t(0));
        hover.tick(t(700));
        assert!(hover.visible().is_some());
        hover.dismiss();
        assert_eq!(hover, Hover::None);
        assert_eq!(hover.next_deadline(), None);
    }
}
