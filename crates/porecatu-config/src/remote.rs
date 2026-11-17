// SPDX-License-Identifier: GPL-3.0-or-later

//! `[remote]` -- PRD-025, ADR-0104 §7. Remote access through the Porecatu
//! Remote app. Both keys are reload class A: the host is created, destroyed
//! or reconnected at the next event from the current `Config`, so there is no
//! extra hot reload work here.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(default)]
pub struct Remote {
    /// Turns remote access on (PRD-025 RF-25.1). Off by default: with it off
    /// the remote crate has no runtime and no thread (RNF-25.1).
    pub enabled: bool,
    /// Address of the user's own Porecatu Relay. The `wss://` rule (and the
    /// `ws://` exception for loopback) is checked by the UI and by the remote
    /// crate, not here: this section only fixes the type. The relay token
    /// lives in the OS vault, never in this file (ADR-0104 §7).
    pub relay_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_example_toml() {
        let d = Remote::default();
        assert!(!d.enabled);
        assert_eq!(d.relay_url, "");
    }
}
