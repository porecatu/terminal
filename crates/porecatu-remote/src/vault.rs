// SPDX-License-Identifier: GPL-3.0-or-later

//! The host identity and the operator token in the OS vault (ADR-0104 §5,
//! ADR-0106 §1). **Only this module touches `keyring`.**
//!
//! Three entries under the service [`SERVICE`]:
//!
//! - `identity:static-x25519`: the host's Noise static key;
//! - `identity:routing-ed25519`: the key whose public half addresses the host
//!   at the relay (its hash is the `host_id`);
//! - `relay:operator-token`: the registration token of the relay.
//!
//! Binary secrets are stored as unpadded base64url. The two identity keys
//! share one life cycle (ADR-0106 §1): when only one of them can be read, both
//! are generated again, never one without the other. Nothing here is logged
//! and no type with a secret prints it.
//!
//! These functions are synchronous and blocking: the UI calls the public ones
//! on a short thread.

use std::fmt;
use std::sync::OnceLock;

use porecatu_protocol::StaticKeypair;
use porecatu_protocol::encoding::{decode_base64url, encode_base64url};
use porecatu_protocol::host_id::{HostId, host_id};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;

/// Service name of the entries. It shows in the OS vault interface and tells
/// the user whose entry it is; distinct from the SSH client's.
const SERVICE: &str = "porecatu-remote";

/// Account read only to learn whether the vault answers: no entry with this
/// name exists or is created.
const PROBE_ACCOUNT: &str = "probe:vault-available";

const STATIC_ACCOUNT: &str = "identity:static-x25519";
const ROUTING_ACCOUNT: &str = "identity:routing-ed25519";
const TOKEN_ACCOUNT: &str = "relay:operator-token";

/// Why a vault operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultError {
    /// There is no usable vault on this system.
    Unavailable,
    /// The vault refused access.
    Denied,
    /// Any other failure.
    Failed,
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            VaultError::Unavailable => "vault unavailable",
            VaultError::Denied => "vault access denied",
            VaultError::Failed => "vault operation failed",
        })
    }
}

impl std::error::Error for VaultError {}

/// What the host needs from the vault. A trait only so tests can swap the real
/// vault for an in-memory one.
pub(crate) trait Vault: Send + Sync + 'static {
    /// `None` when there is no entry.
    fn read(&self, account: &str) -> Result<Option<Zeroizing<String>>, VaultError>;
    fn write(&self, account: &str, secret: &str) -> Result<(), VaultError>;
    /// Deleting what does not exist is not an error.
    fn delete(&self, account: &str) -> Result<(), VaultError>;
}

/// The operating system's vault.
pub(crate) struct SystemVault;

static AVAILABLE: OnceLock<bool> = OnceLock::new();

fn probe() -> bool {
    if keyring::Entry::store_status().is_err() {
        return false;
    }
    match keyring::Entry::new(SERVICE, PROBE_ACCOUNT).and_then(|entry| entry.get_password()) {
        Ok(_) | Err(keyring::Error::NoEntry) => true,
        Err(_) => false,
    }
}

fn entry(account: &str) -> Result<keyring::Entry, VaultError> {
    keyring::Entry::new(SERVICE, account).map_err(translate)
}

fn translate(error: keyring::Error) -> VaultError {
    match error {
        keyring::Error::NoDefaultStore => VaultError::Unavailable,
        keyring::Error::NoStorageAccess(_) => VaultError::Denied,
        _ => VaultError::Failed,
    }
}

impl Vault for SystemVault {
    fn read(&self, account: &str) -> Result<Option<Zeroizing<String>>, VaultError> {
        match entry(account)?.get_password() {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(translate(error)),
        }
    }

    fn write(&self, account: &str, secret: &str) -> Result<(), VaultError> {
        entry(account)?.set_password(secret).map_err(translate)
    }

    fn delete(&self, account: &str) -> Result<(), VaultError> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(translate(error)),
        }
    }
}

/// Whether the OS vault is usable. The answer is cached per process and
/// **computed off the calling thread**: the first call returns `false` and
/// starts the probe on a background thread, so a settings screen never waits
/// for D-Bus. `false` is the safe side (remote access stays off).
pub fn vault_available() -> bool {
    if let Some(known) = AVAILABLE.get() {
        return *known;
    }
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        // If the thread does not start the value stays unknown: `false`.
        let _ = std::thread::Builder::new()
            .name("porecatu-remote-vault-probe".into())
            .spawn(|| {
                let _ = AVAILABLE.get_or_init(probe);
            });
    });
    false
}

/// Stores the operator token; an empty string deletes it.
pub fn store_operator_token(token: &str) -> Result<(), VaultError> {
    store_operator_token_in(&SystemVault, token)
}

/// Whether an operator token is stored. The token itself never leaves this
/// module through the public API.
pub fn operator_token_present() -> Result<bool, VaultError> {
    Ok(operator_token(&SystemVault)?.is_some())
}

pub(crate) fn store_operator_token_in(vault: &dyn Vault, token: &str) -> Result<(), VaultError> {
    if token.is_empty() {
        vault.delete(TOKEN_ACCOUNT)
    } else {
        vault.write(TOKEN_ACCOUNT, token)
    }
}

/// The stored operator token, if any.
pub(crate) fn operator_token(vault: &dyn Vault) -> Result<Option<Zeroizing<String>>, VaultError> {
    Ok(vault.read(TOKEN_ACCOUNT)?.filter(|token| !token.is_empty()))
}

/// The private halves of the host identity. Never printed, zeroed on drop.
pub(crate) struct HostIdentity {
    static_private: Zeroizing<[u8; 32]>,
    routing_private: Zeroizing<[u8; 32]>,
}

impl fmt::Debug for HostIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HostIdentity { .. }")
    }
}

impl HostIdentity {
    /// The Noise static key, rebuilt from the stored private bytes.
    pub(crate) fn static_keypair(&self) -> StaticKeypair {
        StaticKeypair::from_private(*self.static_private)
    }

    /// The key that signs the relay's challenge.
    pub(crate) fn signing_key(&self) -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&self.routing_private)
    }

    /// Public half of the routing key.
    pub(crate) fn routing_public(&self) -> [u8; 32] {
        self.signing_key().verifying_key().to_bytes()
    }

    /// The address of this host at the relay.
    pub(crate) fn host_id(&self) -> HostId {
        host_id(&self.routing_public())
    }

    /// Public half of the Noise static key.
    pub(crate) fn static_public(&self) -> [u8; 32] {
        self.static_keypair().public()
    }
}

/// Whether [`load_or_create_identity`] read the identity or made a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentityOrigin {
    Loaded,
    Created,
}

fn random_key() -> Result<Zeroizing<[u8; 32]>, VaultError> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    SystemRandom::new()
        .fill(&mut *bytes)
        .map_err(|_| VaultError::Failed)?;
    Ok(bytes)
}

/// Reads one identity key. `Ok(None)` when absent **or unreadable** (not
/// base64url, wrong length); a vault failure is an error.
fn read_key(vault: &dyn Vault, account: &str) -> Result<Option<Zeroizing<[u8; 32]>>, VaultError> {
    let Some(text) = vault.read(account)? else {
        return Ok(None);
    };
    let Ok(decoded) = decode_base64url(&text).map(Zeroizing::new) else {
        return Ok(None);
    };
    let Ok(bytes) = <[u8; 32]>::try_from(decoded.as_slice()) else {
        return Ok(None);
    };
    Ok(Some(Zeroizing::new(bytes)))
}

fn write_key(vault: &dyn Vault, account: &str, key: &[u8; 32]) -> Result<(), VaultError> {
    let text = Zeroizing::new(encode_base64url(key));
    vault.write(account, &text)
}

/// The stored identity, or a new one when it cannot be read whole: if only
/// one key exists or either is unreadable, **both** are generated again
/// (ADR-0106 §1). A vault failure is an error and changes nothing.
pub(crate) fn load_or_create_identity(
    vault: &dyn Vault,
) -> Result<(HostIdentity, IdentityOrigin), VaultError> {
    let static_private = read_key(vault, STATIC_ACCOUNT)?;
    let routing_private = read_key(vault, ROUTING_ACCOUNT)?;
    if let (Some(static_private), Some(routing_private)) = (static_private, routing_private) {
        return Ok((
            HostIdentity {
                static_private,
                routing_private,
            },
            IdentityOrigin::Loaded,
        ));
    }
    Ok((recreate_identity(vault)?, IdentityOrigin::Created))
}

/// Generates and stores both keys, replacing whatever was there.
pub(crate) fn recreate_identity(vault: &dyn Vault) -> Result<HostIdentity, VaultError> {
    let static_private = random_key()?;
    let routing_private = random_key()?;
    write_key(vault, STATIC_ACCOUNT, &static_private)?;
    if let Err(error) = write_key(vault, ROUTING_ACCOUNT, &routing_private) {
        // Never leave one new key beside an old one.
        let _ = vault.delete(STATIC_ACCOUNT);
        return Err(error);
    }
    Ok(HostIdentity {
        static_private,
        routing_private,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    /// In-memory vault for tests.
    #[derive(Default)]
    struct MemoryVault {
        unavailable: bool,
        entries: Mutex<HashMap<String, String>>,
    }

    impl MemoryVault {
        fn stored(&self, account: &str) -> Option<String> {
            self.entries.lock().unwrap().get(account).cloned()
        }

        fn put(&self, account: &str, value: &str) {
            self.entries
                .lock()
                .unwrap()
                .insert(account.to_owned(), value.to_owned());
        }
    }

    impl Vault for MemoryVault {
        fn read(&self, account: &str) -> Result<Option<Zeroizing<String>>, VaultError> {
            if self.unavailable {
                return Err(VaultError::Unavailable);
            }
            Ok(self.stored(account).map(Zeroizing::new))
        }

        fn write(&self, account: &str, secret: &str) -> Result<(), VaultError> {
            if self.unavailable {
                return Err(VaultError::Unavailable);
            }
            self.put(account, secret);
            Ok(())
        }

        fn delete(&self, account: &str) -> Result<(), VaultError> {
            if self.unavailable {
                return Err(VaultError::Unavailable);
            }
            self.entries.lock().unwrap().remove(account);
            Ok(())
        }
    }

    #[test]
    fn a_created_identity_loads_back_the_same() {
        let vault = MemoryVault::default();
        let (created, origin) = load_or_create_identity(&vault).unwrap();
        assert_eq!(origin, IdentityOrigin::Created);

        let (loaded, origin) = load_or_create_identity(&vault).unwrap();
        assert_eq!(origin, IdentityOrigin::Loaded);
        assert_eq!(*created.static_private, *loaded.static_private);
        assert_eq!(created.routing_public(), loaded.routing_public());
        assert_eq!(created.static_public(), loaded.static_public());
        assert_eq!(created.host_id(), loaded.host_id());
        assert_eq!(created.host_id().as_str().len(), 32);
    }

    #[test]
    fn the_stored_keys_are_unpadded_base64url_of_32_bytes() {
        let vault = MemoryVault::default();
        load_or_create_identity(&vault).unwrap();
        for account in [STATIC_ACCOUNT, ROUTING_ACCOUNT] {
            let text = vault.stored(account).unwrap();
            assert_eq!(decode_base64url(&text).unwrap().len(), 32);
            assert!(!text.contains('='));
        }
    }

    #[test]
    fn a_lone_key_makes_both_new() {
        for kept in [STATIC_ACCOUNT, ROUTING_ACCOUNT] {
            let vault = MemoryVault::default();
            let (first, _) = load_or_create_identity(&vault).unwrap();
            let other = if kept == STATIC_ACCOUNT {
                ROUTING_ACCOUNT
            } else {
                STATIC_ACCOUNT
            };
            let kept_before = vault.stored(kept).unwrap();
            vault.entries.lock().unwrap().remove(other);

            let (second, origin) = load_or_create_identity(&vault).unwrap();
            assert_eq!(origin, IdentityOrigin::Created);
            assert_ne!(vault.stored(kept).unwrap(), kept_before, "kept {kept}");
            assert_ne!(first.host_id(), second.host_id());
            assert_ne!(first.static_public(), second.static_public());
            assert!(vault.stored(STATIC_ACCOUNT).is_some());
            assert!(vault.stored(ROUTING_ACCOUNT).is_some());
        }
    }

    #[test]
    fn an_unreadable_key_makes_both_new() {
        for bad in ["not base64!", "AAAA", ""] {
            let vault = MemoryVault::default();
            load_or_create_identity(&vault).unwrap();
            let routing_before = vault.stored(ROUTING_ACCOUNT).unwrap();
            vault.put(STATIC_ACCOUNT, bad);

            let (_, origin) = load_or_create_identity(&vault).unwrap();
            assert_eq!(origin, IdentityOrigin::Created, "bad {bad:?}");
            assert_ne!(vault.stored(ROUTING_ACCOUNT).unwrap(), routing_before);
        }
    }

    #[test]
    fn recreating_changes_the_host_id_and_the_static_key() {
        let vault = MemoryVault::default();
        let (old, _) = load_or_create_identity(&vault).unwrap();
        let new = recreate_identity(&vault).unwrap();
        assert_ne!(old.host_id(), new.host_id());
        assert_ne!(old.static_public(), new.static_public());

        let (loaded, origin) = load_or_create_identity(&vault).unwrap();
        assert_eq!(origin, IdentityOrigin::Loaded);
        assert_eq!(loaded.host_id(), new.host_id());
    }

    #[test]
    fn a_vault_failure_is_an_error_and_creates_nothing() {
        let vault = MemoryVault {
            unavailable: true,
            ..MemoryVault::default()
        };
        assert_eq!(
            load_or_create_identity(&vault).unwrap_err(),
            VaultError::Unavailable
        );
        assert!(vault.entries.lock().unwrap().is_empty());
    }

    #[test]
    fn the_signing_key_matches_the_public_key_and_the_host_id() {
        let vault = MemoryVault::default();
        let (identity, _) = load_or_create_identity(&vault).unwrap();
        assert_eq!(
            identity.signing_key().verifying_key().to_bytes(),
            identity.routing_public()
        );
        assert_eq!(identity.host_id(), host_id(&identity.routing_public()));
    }

    #[test]
    fn the_operator_token_is_stored_read_and_deleted() {
        let vault = MemoryVault::default();
        assert!(operator_token(&vault).unwrap().is_none());

        store_operator_token_in(&vault, "secret-token").unwrap();
        assert_eq!(
            operator_token(&vault)
                .unwrap()
                .as_deref()
                .map(String::as_str),
            Some("secret-token")
        );

        store_operator_token_in(&vault, "").unwrap();
        assert!(operator_token(&vault).unwrap().is_none());
        assert!(vault.stored(TOKEN_ACCOUNT).is_none());
    }

    #[test]
    fn no_debug_shows_secret_bytes() {
        let vault = MemoryVault::default();
        let (identity, _) = load_or_create_identity(&vault).unwrap();
        let shown = format!("{identity:?}");
        assert_eq!(shown, "HostIdentity { .. }");
        for account in [STATIC_ACCOUNT, ROUTING_ACCOUNT] {
            assert!(!shown.contains(&vault.stored(account).unwrap()));
        }

        store_operator_token_in(&vault, "tok-123").unwrap();
        let token = operator_token(&vault).unwrap().unwrap();
        // `Zeroizing<String>` prints its content: the guard is that nothing in
        // this module formats it, and that the public API never returns it.
        assert!(!format!("{:?}", operator_token_present_in(&vault)).contains("tok-123"));
        drop(token);
    }

    fn operator_token_present_in(vault: &dyn Vault) -> Result<bool, VaultError> {
        Ok(operator_token(vault)?.is_some())
    }

    #[test]
    fn vault_errors_have_no_free_text() {
        for error in [
            VaultError::Unavailable,
            VaultError::Denied,
            VaultError::Failed,
        ] {
            assert!(!error.to_string().is_empty());
        }
    }
}
