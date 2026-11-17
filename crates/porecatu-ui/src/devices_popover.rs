// SPDX-License-Identifier: GPL-3.0-or-later

//! The connected-devices button and its popover (PRD-025 RF-25.12, RF-25.13,
//! ADR-0108 section 4): pure state, testable without `winit` -- the pattern
//! of `session_picker.rs`. Layout, painting and hit testing live in
//! `overlay.rs`; the wiring (the button, the commands to the host, the mutual
//! exclusion with the other popovers) is `lib.rs`.
//!
//! `WindowState` does not know the remote access: `App` hands every window
//! the list of the open sessions (`WindowState::set_connected_devices`, from
//! `RemoteState::clients`), and this module only reads it. The popover is a
//! capture mode of the window (a remote action on it answers `busy`) and it
//! closes by itself when the last device goes.

use std::time::{Duration, Instant};

use porecatu_locale::Catalog;
use porecatu_remote::{ClientId, DevicePlatform};
use winit::keyboard::{Key, NamedKey};

use crate::messages::msg;

/// A phone with an open session, as a window shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceEntry {
    pub(crate) client: ClientId,
    pub(crate) name: String,
    pub(crate) platform: DevicePlatform,
    pub(crate) connected_at: Instant,
}

/// What a click or a key can act on in the popover: the "Disconnect" of one
/// device (by its session, not its row, so a list that changes under the
/// cursor never disconnects the wrong one), the two items below the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DevicesTarget {
    Disconnect(ClientId),
    DisconnectAll,
    OpenSettings,
}

/// The effect a key asked for. Nothing here touches the host: `App` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DevicesOutcome {
    None,
    Close,
    /// `Command::Disconnect`: the session ends, the device stays paired
    /// (RF-25.13).
    Disconnect(ClientId),
    /// `Command::DisconnectAll`.
    DisconnectAll,
    /// The settings screen on "Acesso remoto".
    OpenSettings,
}

impl From<DevicesTarget> for DevicesOutcome {
    fn from(target: DevicesTarget) -> Self {
        match target {
            DevicesTarget::Disconnect(client) => Self::Disconnect(client),
            DevicesTarget::DisconnectAll => Self::DisconnectAll,
            DevicesTarget::OpenSettings => Self::OpenSettings,
        }
    }
}

/// The targets in reading order: one "Disconnect" per device, then
/// "Disconnect all" and "Open settings".
pub(crate) fn targets(devices: &[DeviceEntry]) -> Vec<DevicesTarget> {
    devices
        .iter()
        .map(|device| DevicesTarget::Disconnect(device.client))
        .chain([DevicesTarget::DisconnectAll, DevicesTarget::OpenSettings])
        .collect()
}

/// The popover. It only keeps what the keyboard and the hover share: which
/// target is highlighted. The list itself is the window's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DevicesPopover {
    highlighted: Option<DevicesTarget>,
}

impl DevicesPopover {
    /// Opens with nothing highlighted: a click opens it, and the first arrow
    /// key or hover picks the target.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The highlighted target, when it still exists: a device that
    /// disconnected under the highlight leaves nothing highlighted.
    pub(crate) fn highlighted(&self, devices: &[DeviceEntry]) -> Option<DevicesTarget> {
        self.highlighted
            .filter(|target| targets(devices).contains(target))
    }

    /// Hover: the same highlight as the keyboard, mutually exclusive
    /// (spec 2.16). A target that is not in the list is ignored.
    pub(crate) fn set_highlight(&mut self, target: DevicesTarget, devices: &[DeviceEntry]) {
        if targets(devices).contains(&target) {
            self.highlighted = Some(target);
        }
    }

    /// `Up`/`Down` (`-1`/`1`): a ring over the targets; with nothing
    /// highlighted, `Down` goes to the first and `Up` to the last.
    pub(crate) fn move_highlight(&mut self, delta: isize, devices: &[DeviceEntry]) {
        let all = targets(devices);
        let len = all.len() as isize;
        let next = match self
            .highlighted(devices)
            .and_then(|t| all.iter().position(|x| *x == t))
        {
            Some(current) => (current as isize + delta).rem_euclid(len),
            None if delta < 0 => len - 1,
            None => 0,
        };
        self.highlighted = all.get(next as usize).copied();
    }

    /// A key pressed with the popover open (it captures the keyboard):
    /// `Esc` closes, the arrows move the highlight, `Enter` acts on it.
    pub(crate) fn handle_key(&mut self, key: &Key, devices: &[DeviceEntry]) -> DevicesOutcome {
        match key {
            Key::Named(NamedKey::Escape) => DevicesOutcome::Close,
            Key::Named(NamedKey::ArrowUp) => {
                self.move_highlight(-1, devices);
                DevicesOutcome::None
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.move_highlight(1, devices);
                DevicesOutcome::None
            }
            Key::Named(NamedKey::Enter) => self
                .highlighted(devices)
                .map_or(DevicesOutcome::None, DevicesOutcome::from),
            _ => DevicesOutcome::None,
        }
    }
}

/// How long a device has been connected, in the unit the row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Since {
    /// Less than a minute.
    Moments,
    Minutes(u64),
    /// Hours, however many (a day is "24 hours").
    Hours(u64),
}

/// The elapsed time as the popover says it: minutes up to the first hour,
/// hours from there.
pub(crate) fn since(elapsed: Duration) -> Since {
    let secs = elapsed.as_secs();
    if secs < 60 {
        Since::Moments
    } else if secs < 3600 {
        Since::Minutes(secs / 60)
    } else {
        Since::Hours(secs / 3600)
    }
}

/// When the text of some row changes next: the first minute (or hour, from
/// the first hour on) boundary after `now` of any device. The popover shows
/// "connected X ago", so while it is open the window wakes up then (a
/// `WaitUntil`, no thread, ADR-0007) and repaints. `None` with no device.
pub(crate) fn next_age_change(devices: &[DeviceEntry], now: Instant) -> Option<Instant> {
    devices
        .iter()
        .map(|device| {
            let elapsed = now.saturating_duration_since(device.connected_at);
            let step = if elapsed.as_secs() < 3600 { 60 } else { 3600 };
            let boundaries = elapsed.as_secs() / step + 1;
            device.connected_at + Duration::from_secs(boundaries * step)
        })
        .min()
}

/// "Connected X ago" of the catalog.
pub(crate) fn since_text(catalog: &Catalog, since: Since) -> String {
    match since {
        Since::Moments => msg::devices::since_now(catalog),
        Since::Minutes(n) => msg::devices::since_minutes(catalog, n as usize),
        Since::Hours(n) => msg::devices::since_hours(catalog, n as usize),
    }
}

/// The platform name of the catalog: the one the list of paired devices of
/// the settings screen shows, so the two never disagree.
pub(crate) use crate::settings::platform_text;

/// The second line of a row: "platform · connected X ago".
pub(crate) fn detail_text(catalog: &Catalog, device: &DeviceEntry, now: Instant) -> String {
    msg::devices::row_detail(
        catalog,
        platform_text(catalog, device.platform),
        since_text(
            catalog,
            since(now.saturating_duration_since(device.connected_at)),
        ),
    )
}

/// The text of the button's tooltip and of its accessibility node: "N
/// device(s) connected: name, name" (ADR-0108 section 4).
pub(crate) fn summary_text(catalog: &Catalog, devices: &[DeviceEntry]) -> String {
    let names = devices
        .iter()
        .map(|device| device.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    msg::devices::summary(catalog, devices.len(), names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::test_support;

    fn device(id: u64, name: &str, at: Instant) -> DeviceEntry {
        DeviceEntry {
            client: ClientId(id),
            name: name.to_owned(),
            platform: DevicePlatform::Android,
            connected_at: at,
        }
    }

    fn two() -> Vec<DeviceEntry> {
        let now = Instant::now();
        vec![device(1, "Pixel", now), device(2, "iPad", now)]
    }

    #[test]
    fn the_targets_are_one_disconnect_per_device_then_the_two_items() {
        assert_eq!(
            targets(&two()),
            [
                DevicesTarget::Disconnect(ClientId(1)),
                DevicesTarget::Disconnect(ClientId(2)),
                DevicesTarget::DisconnectAll,
                DevicesTarget::OpenSettings,
            ]
        );
    }

    #[test]
    fn it_opens_with_nothing_highlighted_and_the_arrows_pick_the_ends() {
        let devices = two();
        let mut popover = DevicesPopover::new();
        assert_eq!(popover.highlighted(&devices), None);
        popover.move_highlight(1, &devices);
        assert_eq!(
            popover.highlighted(&devices),
            Some(DevicesTarget::Disconnect(ClientId(1)))
        );

        let mut popover = DevicesPopover::new();
        popover.move_highlight(-1, &devices);
        assert_eq!(
            popover.highlighted(&devices),
            Some(DevicesTarget::OpenSettings)
        );
    }

    #[test]
    fn the_highlight_wraps_around_the_targets() {
        let devices = two();
        let mut popover = DevicesPopover::new();
        popover.move_highlight(-1, &devices);
        popover.move_highlight(1, &devices);
        assert_eq!(
            popover.highlighted(&devices),
            Some(DevicesTarget::Disconnect(ClientId(1))),
            "after the last, the first"
        );
        popover.move_highlight(-1, &devices);
        assert_eq!(
            popover.highlighted(&devices),
            Some(DevicesTarget::OpenSettings),
            "before the first, the last"
        );
    }

    #[test]
    fn enter_acts_on_the_highlight_and_does_nothing_without_one() {
        let devices = two();
        let mut popover = DevicesPopover::new();
        assert_eq!(
            popover.handle_key(&Key::Named(NamedKey::Enter), &devices),
            DevicesOutcome::None
        );
        popover.handle_key(&Key::Named(NamedKey::ArrowDown), &devices);
        popover.handle_key(&Key::Named(NamedKey::ArrowDown), &devices);
        assert_eq!(
            popover.handle_key(&Key::Named(NamedKey::Enter), &devices),
            DevicesOutcome::Disconnect(ClientId(2))
        );
        popover.set_highlight(DevicesTarget::DisconnectAll, &devices);
        assert_eq!(
            popover.handle_key(&Key::Named(NamedKey::Enter), &devices),
            DevicesOutcome::DisconnectAll
        );
        popover.set_highlight(DevicesTarget::OpenSettings, &devices);
        assert_eq!(
            popover.handle_key(&Key::Named(NamedKey::Enter), &devices),
            DevicesOutcome::OpenSettings
        );
    }

    #[test]
    fn escape_closes_and_other_keys_are_taken_without_effect() {
        let devices = two();
        let mut popover = DevicesPopover::new();
        assert_eq!(
            popover.handle_key(&Key::Named(NamedKey::Escape), &devices),
            DevicesOutcome::Close
        );
        assert_eq!(
            popover.handle_key(&Key::Character("a".into()), &devices),
            DevicesOutcome::None
        );
    }

    #[test]
    fn a_highlight_on_a_device_that_left_is_forgotten() {
        let devices = two();
        let mut popover = DevicesPopover::new();
        popover.set_highlight(DevicesTarget::Disconnect(ClientId(2)), &devices);
        let only_first = vec![devices[0].clone()];
        assert_eq!(popover.highlighted(&only_first), None);
        assert_eq!(
            popover.handle_key(&Key::Named(NamedKey::Enter), &only_first),
            DevicesOutcome::None,
            "Enter never disconnects a device that is not there"
        );
        // A target that is not in the list is not taken either.
        popover.set_highlight(DevicesTarget::Disconnect(ClientId(9)), &devices);
        assert_eq!(
            popover.highlighted(&devices),
            Some(DevicesTarget::Disconnect(ClientId(2))),
            "the old highlight stays: the new one was ignored"
        );
    }

    #[test]
    fn the_elapsed_time_is_minutes_up_to_the_first_hour_then_hours() {
        assert_eq!(since(Duration::from_secs(0)), Since::Moments);
        assert_eq!(since(Duration::from_secs(59)), Since::Moments);
        assert_eq!(since(Duration::from_secs(60)), Since::Minutes(1));
        assert_eq!(since(Duration::from_secs(3599)), Since::Minutes(59));
        assert_eq!(since(Duration::from_secs(3600)), Since::Hours(1));
        assert_eq!(since(Duration::from_secs(30 * 3600)), Since::Hours(30));
    }

    #[test]
    fn the_next_age_change_is_the_next_minute_then_the_next_hour_boundary() {
        let t0 = Instant::now();
        let devices = vec![device(1, "Pixel", t0)];
        let at = |secs: u64| next_age_change(&devices, t0 + Duration::from_secs(secs));
        assert_eq!(at(0), Some(t0 + Duration::from_secs(60)));
        assert_eq!(at(59), Some(t0 + Duration::from_secs(60)));
        assert_eq!(
            at(60),
            Some(t0 + Duration::from_secs(120)),
            "on the boundary the change already happened"
        );
        assert_eq!(at(3599), Some(t0 + Duration::from_secs(3600)));
        assert_eq!(at(3600), Some(t0 + Duration::from_secs(7200)));
        assert_eq!(next_age_change(&[], t0), None);

        // The earliest of the devices wins.
        let later = vec![
            device(1, "Pixel", t0),
            device(2, "iPad", t0 + Duration::from_secs(30)),
        ];
        assert_eq!(
            next_age_change(&later, t0 + Duration::from_secs(40)),
            Some(t0 + Duration::from_secs(60))
        );
    }

    #[test]
    fn the_row_detail_says_the_platform_and_how_long_ago() {
        let catalog = test_support::pt_br();
        let t0 = Instant::now();
        let entry = device(1, "Pixel", t0);
        assert_eq!(
            detail_text(&catalog, &entry, t0 + Duration::from_secs(5 * 60)),
            "Android · conectado há 5 minutos"
        );
        assert_eq!(
            detail_text(&catalog, &entry, t0 + Duration::from_secs(60)),
            "Android · conectado há 1 minuto"
        );
        assert_eq!(
            detail_text(&catalog, &entry, t0 + Duration::from_secs(2 * 3600)),
            "Android · conectado há 2 horas"
        );
        assert_eq!(
            detail_text(&catalog, &entry, t0),
            "Android · conectado agora"
        );
    }

    #[test]
    fn the_summary_counts_and_names_the_devices() {
        let catalog = test_support::pt_br();
        let now = Instant::now();
        assert_eq!(
            summary_text(&catalog, &[device(1, "Pixel", now)]),
            "1 aparelho conectado: Pixel"
        );
        assert_eq!(
            summary_text(
                &catalog,
                &[
                    device(1, "Pixel", now),
                    device(2, "iPad", now),
                    device(3, "Galaxy", now)
                ]
            ),
            "3 aparelhos conectados: Pixel, iPad, Galaxy"
        );
        let en = test_support::en_us();
        assert_eq!(
            summary_text(&en, &[device(1, "Pixel", now)]),
            "1 device connected: Pixel"
        );
    }
}
