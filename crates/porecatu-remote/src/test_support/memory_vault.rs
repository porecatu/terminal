// SPDX-License-Identifier: GPL-3.0-or-later

//! An in-memory [`Vault`] for the tests of the host.

use std::collections::HashMap;
use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::vault::{Vault, VaultError, store_operator_token_in};

/// The operator token the tests of the host store.
pub(crate) const TOKEN: &str = "0123456789abcdef0123456789abcdef";

/// A vault that lives in memory; `unavailable` makes every call fail.
#[derive(Default)]
pub(crate) struct MemoryVault {
    unavailable: bool,
    entries: Mutex<HashMap<String, String>>,
}

impl MemoryVault {
    /// A vault holding the operator token [`TOKEN`].
    pub(crate) fn with_token() -> MemoryVault {
        let vault = MemoryVault::default();
        store_operator_token_in(&vault, TOKEN).expect("store the token");
        vault
    }

    /// A vault that fails on every call.
    pub(crate) fn failing() -> MemoryVault {
        MemoryVault {
            unavailable: true,
            ..MemoryVault::default()
        }
    }
}

impl Vault for MemoryVault {
    fn read(&self, account: &str) -> Result<Option<Zeroizing<String>>, VaultError> {
        if self.unavailable {
            return Err(VaultError::Unavailable);
        }
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        Ok(entries.get(account).cloned().map(Zeroizing::new))
    }

    fn write(&self, account: &str, secret: &str) -> Result<(), VaultError> {
        if self.unavailable {
            return Err(VaultError::Unavailable);
        }
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.insert(account.to_owned(), secret.to_owned());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), VaultError> {
        if self.unavailable {
            return Err(VaultError::Unavailable);
        }
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.remove(account);
        Ok(())
    }
}
