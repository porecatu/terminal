// SPDX-License-Identifier: GPL-3.0-or-later

//! Remote access host side (PRD-025, ADR-0103, ADR-0104): a `tokio` runtime
//! owned by this crate, behind a synchronous API. It connects to a
//! self-hosted relay, speaks the Porecatu remote protocol, and turns what the
//! phone sends into [`RemoteEvent`]s for the main thread.
//!
//! This is a leaf crate: it depends on no crate of the project and knows
//! neither `Workspace`, `Config`, grid snapshots of the terminal engine nor
//! `winit`. It talks protocol types (re-exported as [`protocol`]) and
//! `porecatu-ui` is the only crate that depends on it.
//!
//! Rules that hold for every module:
//!
//! - No public function is `async`, and no `tokio` type crosses the API.
//! - No error carries free text: failures are typed enums, the user-facing
//!   prose is born in `porecatu-ui`.
//! - Nothing that comes from a terminal or from a device (screen content,
//!   keys, titles, directories, tokens, keys) is ever logged.

mod api;
mod devices;
mod grid_flow;
mod host;
mod name;
mod relay;
mod session;
#[cfg(test)]
mod test_support;
mod time;
mod vault;

/// The protocol crate, re-exported so `porecatu-ui` never declares it.
pub use porecatu_protocol as protocol;

pub use api::*;
pub use host::RemoteHost;
pub use name::machine_name;
pub use vault::{VaultError, operator_token_present, store_operator_token, vault_available};
