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

use std::collections::HashMap;
use std::time::{Duration, Instant};

use porecatu_core::{TabId, Workspace};
use porecatu_locale::Catalog;
use porecatu_render::Rect;

use crate::devices_popover::{self, DeviceEntry};
use crate::messages::msg;
use crate::tab_bar::{self, TabBarStyle};
use crate::task_manager::RowKey;

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
    /// The name of a row of the task manager list, cut by the column
    /// (PRD-024 RF-24.16, ADR-0095 §7). By the row's identity, not its index:
    /// a new sample does not make the tooltip jump to another row.
    TaskRow(RowKey),
    /// The connected-devices button of the bar's fixed zone (PRD-025
    /// RF-25.12, ADR-0108 section 4): "N device(s) connected: name, name".
    /// One per window, so it needs no identity.
    DevicesButton,
    /// The box of the viewed-on-a-phone indicator of a track tab (ADR-0112
    /// section 4). Not `Tab`: moving from the box to the rest of the tab is a
    /// change of target, with the delay restarted, like any other.
    TabViewers(TabId),
}

/// The "Viewed on: name, name" of a tab a phone is viewing (ADR-0112 section
/// 4), in the order the phones arrived. `None` for a tab nobody is viewing.
fn viewers_text(
    catalog: &Catalog,
    viewers: &HashMap<TabId, Vec<String>>,
    id: TabId,
) -> Option<String> {
    let names = viewer_names(viewers, id)?;
    Some(msg::devices::viewed_on(
        catalog,
        devices_popover::join_names(names.iter().map(String::as_str)),
    ))
}

/// The names of the phones viewing `id`, `None` when it is nobody (no entry,
/// or an empty list).
fn viewer_names(viewers: &HashMap<TabId, Vec<String>>, id: TabId) -> Option<&[String]> {
    viewers
        .get(&id)
        .map(Vec::as_slice)
        .filter(|names| !names.is_empty())
}

/// The tooltip target of the viewed-on-a-phone indicator of a track tab under
/// `content_point` (content coordinates, the track's), or `None` away from the
/// box -- there the tab's own tooltip decides. The box is the one
/// [`tab_bar::TabRect::viewer_box`] gives; the anchor is it in screen
/// coordinates (`screen_dx` = the track's shift minus the scroll). The tooltip
/// is not part of the hit test: the click is still the tab's.
pub fn viewers_target(
    layout: &tab_bar::TabBarLayout,
    style: &TabBarStyle,
    catalog: &Catalog,
    viewers: &HashMap<TabId, Vec<String>>,
    content_point: (f32, f32),
    screen_dx: f32,
) -> Option<(HoverKey, Rect, String)> {
    let (id, rect) = layout
        .groups
        .iter()
        .flat_map(|group| &group.tabs)
        .find_map(|tab| {
            let rect = tab.viewer_box(style)?;
            tab_bar::rect_contains(rect, content_point).then_some((tab.id, rect))
        })?;
    let text = viewers_text(catalog, viewers, id)?;
    let anchor = Rect {
        x: rect.x + screen_dx,
        ..rect
    };
    Some((HoverKey::TabViewers(id), anchor, text))
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
    catalog: &Catalog,
    viewers: &HashMap<TabId, Vec<String>>,
) -> Option<(HoverKey, Rect, String)> {
    let i = tab_bar::point_in_pinned(style, is_macos, workspace.pinned().len(), point)?;
    let id = *workspace.pinned().get(i)?;
    let title = workspace.tab(id)?.title().to_owned();
    // ADR-0112 section 4: the dot is too small to be a target, so the phrase
    // goes after the title, in the same line.
    let title = match viewer_names(viewers, id) {
        Some(names) => msg::devices::viewed_on_pinned(
            catalog,
            title,
            devices_popover::join_names(names.iter().map(String::as_str)),
        ),
        None => title,
    };
    Some((
        HoverKey::Tab(id),
        tab_bar::pinned_rect(style, is_macos, i),
        title,
    ))
}

/// The tooltip target of the connected-devices button under `point` (screen
/// coordinates), or `None` when there is no such button (no device connected)
/// or the point is elsewhere. The button is a plain icon, so it **always** has
/// a tooltip: the count and the names (ADR-0108 section 4), the same text its
/// accessibility node carries.
pub fn devices_target(
    devices: &[DeviceEntry],
    catalog: &Catalog,
    style: &TabBarStyle,
    bar_width: f32,
    bar_height: f32,
    is_macos: bool,
    point: (f32, f32),
) -> Option<(HoverKey, Rect, String)> {
    if tab_bar::point_in_zone_button(
        style,
        bar_width,
        bar_height,
        is_macos,
        !devices.is_empty(),
        point,
    ) != Some(tab_bar::ZoneButtonHit::Devices)
    {
        return None;
    }
    Some((
        HoverKey::DevicesButton,
        tab_bar::devices_button_rect(style, bar_width, bar_height, is_macos),
        devices_popover::summary_text(catalog, devices),
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

    fn devices(names: &[&str]) -> Vec<DeviceEntry> {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| DeviceEntry {
                client: porecatu_remote::ClientId(i as u64),
                name: (*name).to_owned(),
                platform: porecatu_remote::DevicePlatform::Android,
                connected_at: Instant::now(),
            })
            .collect()
    }

    /// The tooltip of the connected-devices button (ADR-0108 section 4): the
    /// count and the names, plural by the catalog, anchored by the button.
    #[test]
    fn the_devices_button_has_a_tooltip_with_the_count_and_the_names() {
        let style = TabBarStyle::DEFAULT;
        let bar_height = crate::chrome::bar_height(&style);
        let catalog = crate::messages::test_support::pt_br();
        let button = tab_bar::devices_button_rect(&style, 800.0, bar_height, false);
        let inside = (
            button.x + button.width / 2.0,
            button.y + button.height / 2.0,
        );

        let one = devices(&["Pixel"]);
        let (key, anchor, text) =
            devices_target(&one, &catalog, &style, 800.0, bar_height, false, inside).unwrap();
        assert_eq!(key, HoverKey::DevicesButton);
        assert_eq!(anchor, button);
        assert_eq!(text, "1 aparelho conectado: Pixel");

        let three = devices(&["Pixel", "iPad", "Galaxy"]);
        let (_, _, text) =
            devices_target(&three, &catalog, &style, 800.0, bar_height, false, inside).unwrap();
        assert_eq!(text, "3 aparelhos conectados: Pixel, iPad, Galaxy");
    }

    /// Without a device the button is not there, so neither is its tooltip --
    /// and the point where it would be is the track.
    #[test]
    fn there_is_no_devices_tooltip_without_the_button_or_away_from_it() {
        let style = TabBarStyle::DEFAULT;
        let bar_height = crate::chrome::bar_height(&style);
        let catalog = crate::messages::test_support::pt_br();
        let button = tab_bar::devices_button_rect(&style, 800.0, bar_height, false);
        let inside = (
            button.x + button.width / 2.0,
            button.y + button.height / 2.0,
        );
        assert!(devices_target(&[], &catalog, &style, 800.0, bar_height, false, inside).is_none());
        let one = devices(&["Pixel"]);
        let sessions = tab_bar::sessions_button_rect(&style, 800.0, bar_height, false);
        let over_sessions = (sessions.x + 1.0, sessions.y + 1.0);
        assert!(
            devices_target(
                &one,
                &catalog,
                &style,
                800.0,
                bar_height,
                false,
                over_sessions
            )
            .is_none()
        );
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

    /// A task manager row is a different target from a tab with the same
    /// text, and the same row keeps its delay across samples.
    #[test]
    fn a_task_row_is_its_own_target_by_identity() {
        let row = RowKey::Tab(TabId::new(0));
        let mut hover = Hover::default();
        hover.update(Some((tab(0), rect(), "igual".into())), t(0));
        hover.update(
            Some((HoverKey::TaskRow(row), rect(), "igual".into())),
            t(100),
        );
        hover.tick(t(699));
        assert_eq!(hover.visible(), None, "a new target restarts the delay");
        hover.update(
            Some((HoverKey::TaskRow(row), rect(), "igual".into())),
            t(650),
        );
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
            pinned_target(
                &ws,
                &style,
                false,
                over,
                &crate::messages::test_support::pt_br(),
                &HashMap::new()
            ),
            Some((HoverKey::Tab(b), second, "b".to_owned()))
        );
        let first = tab_bar::pinned_rect(&style, false, 0);
        let gap = (first.x + first.width + 1.0, first.y + 3.0);
        assert!(gap.0 < second.x);
        assert_eq!(
            pinned_target(
                &ws,
                &style,
                false,
                gap,
                &crate::messages::test_support::pt_br(),
                &HashMap::new()
            ),
            None
        );
        // Past the zone, the track answers: no pinned target there.
        let past = tab_bar::track_shift(&style, false, 2) + 10.0;
        assert_eq!(
            pinned_target(
                &ws,
                &style,
                false,
                (past, 10.0),
                &crate::messages::test_support::pt_br(),
                &HashMap::new()
            ),
            None
        );

        // The state machine shows it after the usual 600 ms, and drops it
        // when the cursor leaves.
        let mut hover = Hover::default();
        hover.update(
            pinned_target(
                &ws,
                &style,
                false,
                over,
                &crate::messages::test_support::pt_br(),
                &HashMap::new(),
            ),
            t(0),
        );
        hover.tick(t(599));
        assert_eq!(hover.visible(), None);
        hover.tick(t(600));
        assert_eq!(hover.visible(), Some((second, "b")));
        hover.update(
            pinned_target(
                &ws,
                &style,
                false,
                gap,
                &crate::messages::test_support::pt_br(),
                &HashMap::new(),
            ),
            t(700),
        );
        assert_eq!(hover, Hover::None);
    }

    /// ADR-0112 section 4: over the box of the indicator the tooltip is the box
    /// with "Vista em: nome, nome" (names in the order they arrived, even when
    /// the label fits); anywhere else on the tab -- and on a tab nobody views --
    /// it is not this one's, and the box and the tab are different targets.
    #[test]
    fn the_viewer_box_of_a_track_tab_has_the_viewed_on_tooltip() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("a", None);
        let b = ws.append_tab("b", None);
        let style = TabBarStyle::DEFAULT;
        let catalog = crate::messages::test_support::pt_br();
        let viewers = HashMap::from([(a, vec!["Pixel".to_owned(), "iPad".to_owned()])]);
        let mut m = porecatu_render::TextMeasurer::new();
        let layout =
            tab_bar::fit_width_viewed(&ws, &style, &mut m, &tab_bar::viewed_tabs(&viewers));
        let tabs = &layout.groups[0].tabs;
        let viewer_box = tabs[0].viewer_box(&style).unwrap();
        let center = (
            viewer_box.x + viewer_box.width / 2.0,
            viewer_box.y + viewer_box.height / 2.0,
        );
        let scroll_shift = -37.0;
        let (key, anchor, text) =
            viewers_target(&layout, &style, &catalog, &viewers, center, scroll_shift).unwrap();
        assert_eq!(key, HoverKey::TabViewers(a));
        assert_ne!(key, HoverKey::Tab(a));
        assert_eq!(
            anchor,
            Rect {
                x: viewer_box.x + scroll_shift,
                ..viewer_box
            }
        );
        assert_eq!(text, "Vista em: Pixel, iPad");

        // The tab's body, and the other tab's whole area, are not the box.
        let body = (tabs[0].rect.x + 2.0, tabs[0].rect.y + 2.0);
        assert!(viewers_target(&layout, &style, &catalog, &viewers, body, 0.0).is_none());
        let other = (tabs[1].rect.x + 2.0, tabs[1].rect.y + 2.0);
        assert!(viewers_target(&layout, &style, &catalog, &viewers, other, 0.0).is_none());
        let _ = b;

        // The state machine: moving from the tab to the box restarts the delay.
        let mut hover = Hover::default();
        hover.update(Some((HoverKey::Tab(a), rect(), "a".into())), t(0));
        hover.update(Some((key, anchor, text.clone())), t(500));
        hover.tick(t(600));
        assert_eq!(hover.visible(), None);
        hover.tick(t(1100));
        assert_eq!(hover.visible(), Some((anchor, text.as_str())));
    }

    /// ADR-0112 section 4: the title tooltip a pinned box always has gets the
    /// phrase after the title, in the same line, joined by the " · " of the
    /// catalog's composed phrases; without phones it is the title alone.
    #[test]
    fn the_tooltip_of_a_viewed_pinned_box_has_the_phrase_after_the_title() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("a", None);
        let b = ws.append_tab("b", None);
        ws.pin_tab(a, 0);
        ws.pin_tab(b, 1);
        let style = TabBarStyle::DEFAULT;
        let catalog = crate::messages::test_support::pt_br();
        let second = tab_bar::pinned_rect(&style, false, 1);
        let over = (second.x + 3.0, second.y + 3.0);

        let viewers = HashMap::from([(b, vec!["Pixel 8".to_owned()])]);
        let (key, anchor, text) =
            pinned_target(&ws, &style, false, over, &catalog, &viewers).unwrap();
        assert_eq!((key, anchor), (HoverKey::Tab(b), second));
        assert_eq!(text, "b · Vista em: Pixel 8");
        assert!(!text.contains('\n'), "one line");

        let two = HashMap::from([(b, vec!["Pixel 8".to_owned(), "iPad".to_owned()])]);
        let (_, _, text) = pinned_target(&ws, &style, false, over, &catalog, &two).unwrap();
        assert_eq!(text, "b · Vista em: Pixel 8, iPad");

        // Another pinned tab's phones do not leak into this one's.
        let elsewhere = HashMap::from([(a, vec!["Pixel 8".to_owned()])]);
        let (_, _, text) = pinned_target(&ws, &style, false, over, &catalog, &elsewhere).unwrap();
        assert_eq!(text, "b");
        let (_, _, text) =
            pinned_target(&ws, &style, false, over, &catalog, &HashMap::new()).unwrap();
        assert_eq!(text, "b");
    }
}
