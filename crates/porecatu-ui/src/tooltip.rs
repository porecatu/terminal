// SPDX-License-Identifier: GPL-3.0-or-later

//! Hover e tooltip (ADR-0019, RF-1.10): só aparece pra alvo com texto
//! truncado, depois de 600ms de hover parado. F2 só tem alvo de aba
//! (rótulo); grupo (RF-2.12) é F3. A aba fixada (ADR-0087 §3) é o caso-limite:
//! o rótulo dela está truncado a zero, então **sempre** tem tooltip
//! ([`pinned_target`]). Desde o ADR-0055 §2, também a linha
//! do popover de sessões (`HoverKey::SessionRow`) -- `Hover` não conhece
//! ali `TabId` como o único tipo de alvo possível, só um `HoverKey` que
//! distingue os dois por igualdade, o que é tudo que a máquina de estado
//! abaixo precisa.
//!
//! `Instant::now()` não aparece aqui, pelo mesmo motivo de `warning.rs`:
//! quem chama passa `now`, o que torna o atraso testável sem dormir.

use std::time::{Duration, Instant};

use porecatu_core::{TabId, Workspace};
use porecatu_render::Rect;

use crate::tab_bar::{self, TabBarStyle};

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
    /// Descrição truncada de uma linha de opção da janela de configurações
    /// (ADR-0060 §2); o índice é o do bloco no painel.
    SettingsRow(usize),
    /// O botão de restaurar padrão de uma linha da janela de configurações
    /// (ADR-0060 §2: "Restaurar padrão", espec. §2.20); o índice é o do bloco
    /// no painel. Ícone sem texto: o tooltip diz o que ele faz.
    SettingsRestore(usize),
}

/// The tooltip target of a pinned box under `point` (screen coordinates), or
/// `None` over a gap, a margin or anything else (ADR-0087 §3). A pinned box
/// has no label -- its label is truncated to zero -- so it **always** has a
/// tooltip, with the title of the tab (the one the bar's own labels are made
/// from, RF-1.7), anchored by the box: [`paint_tooltip`] puts it below the
/// box, aligned to its left edge. Same key as a track tab: a tab is in one
/// place at a time, and the state machine only compares keys.
///
/// [`paint_tooltip`]: crate::overlay::paint_tooltip
pub fn pinned_target(
    workspace: &Workspace,
    style: &TabBarStyle,
    is_macos: bool,
    point: (f32, f32),
) -> Option<(HoverKey, Rect, String)> {
    let i = tab_bar::point_in_pinned(style, is_macos, workspace.pinned().len(), point)?;
    let id = *workspace.pinned().get(i)?;
    let title = workspace.tab(id)?.title().to_owned();
    Some((
        HoverKey::Tab(id),
        tab_bar::pinned_rect(style, is_macos, i),
        title,
    ))
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

    /// ADR-0087 §3: a pinned box always has a tooltip -- the tab title, even
    /// a short one that no track label would ever need -- anchored by the box
    /// itself, and only over the box: a gap or a margin has none.
    #[test]
    fn a_pinned_box_always_has_a_tooltip_with_the_title() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("a", None);
        let b = ws.append_tab("b", None);
        ws.append_tab("track", None);
        ws.pin_tab(a, 0);
        ws.pin_tab(b, 1);
        let style = TabBarStyle::DEFAULT;

        let second = tab_bar::pinned_rect(&style, false, 1);
        let over = (second.x + 3.0, second.y + 3.0);
        assert_eq!(
            pinned_target(&ws, &style, false, over),
            Some((HoverKey::Tab(b), second, "b".to_owned()))
        );
        let first = tab_bar::pinned_rect(&style, false, 0);
        let gap = (first.x + first.width + 1.0, first.y + 3.0);
        assert!(gap.0 < second.x);
        assert_eq!(pinned_target(&ws, &style, false, gap), None);
        // Past the zone, the track answers: no pinned target there.
        let past = tab_bar::track_shift(&style, false, 2) + 10.0;
        assert_eq!(pinned_target(&ws, &style, false, (past, 10.0)), None);

        // The state machine shows it after the usual 600 ms, and drops it
        // when the cursor leaves.
        let mut hover = Hover::default();
        hover.update(pinned_target(&ws, &style, false, over), t(0));
        hover.tick(t(599));
        assert_eq!(hover.visible(), None);
        hover.tick(t(600));
        assert_eq!(hover.visible(), Some((second, "b")));
        hover.update(pinned_target(&ws, &style, false, gap), t(700));
        assert_eq!(hover, Hover::None);
    }
}
