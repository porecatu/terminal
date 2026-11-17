// SPDX-License-Identifier: GPL-3.0-or-later

//! `[appearance.remote]` -- PRD-025, ADR-0108 §2. Pairing QR code: the
//! target size (reload class A) and the two colors, which live in the themes
//! (`[remote]` of `thm_*.toml`, ADR-0067).
//!
//! The type is named `RemoteAppearance` because the crate root also exports
//! `Remote` (the `[remote]` behaviour section).

use serde::Deserialize;

use crate::color::Color;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct RemoteAppearance {
    /// Target side of the pairing QR code, in logical pixels, quiet zone
    /// included. The module side is the largest whole number of physical
    /// pixels that fits. New in ADR-0108 §2; owner's visual approval pending.
    pub qr_size: i32,
    /// Dark modules of the QR code (theme key `[remote] qr_dark`). Always
    /// dark on light in both themes: common readers do not read it inverted.
    #[serde(skip_deserializing)]
    pub qr_dark: Color,
    /// Light modules and quiet zone of the QR code (theme key
    /// `[remote] qr_light`).
    #[serde(skip_deserializing)]
    pub qr_light: Color,
}

impl Default for RemoteAppearance {
    fn default() -> Self {
        Self {
            qr_size: 208,
            qr_dark: Color::hex("#000000"),
            qr_light: Color::hex("#ffffff"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let d = RemoteAppearance::default();
        assert_eq!(d.qr_size, 208);
        assert_eq!(d.qr_dark, Color::hex("#000000"));
        assert_eq!(d.qr_light, Color::hex("#ffffff"));
    }
}
