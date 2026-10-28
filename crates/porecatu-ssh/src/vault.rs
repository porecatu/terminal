// SPDX-License-Identifier: GPL-3.0-or-later

//! O cofre do sistema (ADR-0073): Gerenciador de Credenciais do Windows,
//! Keychain do macOS, Secret Service no Linux, por `keyring`. **Só este módulo
//! toca o `keyring`.**
//!
//! Cada segredo é uma entrada indexada pelo destino (senha) ou pelo arquivo da
//! chave (passphrase), nunca pelo perfil. Nenhuma função devolve o segredo à
//! UI: ela só pergunta se o cofre existe e pede para esquecer.
//!
//! Todo acesso é bloqueante (chamada de SO, D-Bus). Quem o usa dentro de uma
//! conexão passa por `tokio::task::spawn_blocking`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::secret::Secret;

/// Nome do serviço nas entradas. Aparece na interface do cofre do sistema e diz
/// ao usuário de quem é a entrada.
const SERVICE: &str = "porecatu-ssh";

/// Conta lida só para saber se o cofre responde: nenhuma entrada com este nome
/// existe nem é criada.
const PROBE_ACCOUNT: &str = "probe:vault-available";

/// Destino normalizado: host em minúsculas, porta sempre explícita. `user` e
/// `host` já vêm validados pela config (ADR-0072 §6) -- sem `@`, `:`, espaço
/// nem controle --, o que torna `password:<user>@<host>:<port>` inequívoco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub user: String,
    pub host: String,
    pub port: u16,
}

impl Destination {
    /// O host é guardado em minúsculas.
    pub fn new(user: impl Into<String>, host: impl AsRef<str>, port: u16) -> Self {
        Self {
            user: user.into(),
            host: host.as_ref().to_lowercase(),
            port,
        }
    }

    /// A conta da senha deste destino (ADR-0073 §2).
    pub(crate) fn password_account(&self) -> String {
        format!(
            "password:{}@{}:{}",
            self.user,
            self.host.to_lowercase(),
            self.port
        )
    }
}

/// A conta da passphrase de um arquivo de chave: o caminho canônico, que
/// resolve `~`, `..` e links (ADR-0073 §2).
pub(crate) fn passphrase_account(key_file: &Path) -> String {
    format!("passphrase:{}", canonical(key_file).to_string_lossy())
}

fn canonical(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(canonical) => strip_verbatim(canonical),
        Err(_) => path.to_owned(),
    }
}

/// No Windows `canonicalize` devolve `\\?\C:\...`; a mesma chave tem de dar a
/// mesma conta de quem escreveu o caminho sem o prefixo.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}

/// Por que o cofre não fez o que se pediu. Sem texto livre: a UI decide a
/// frase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultError {
    /// Não há cofre neste sistema (Linux sem Secret Service).
    Unavailable,
    /// O cofre existe e recusou: trancado, ou o usuário negou a permissão.
    Denied,
    /// Qualquer outra falha do sistema.
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

/// O que a conexão precisa do cofre. Trait só para os testes trocarem o cofre
/// de verdade por um em memória: ninguém fora deste crate o implementa.
pub(crate) trait Vault: Send + Sync + 'static {
    /// O cofre responde. Bloqueante na primeira vez.
    fn available(&self) -> bool;
    /// `None` quando não há entrada.
    fn read(&self, account: &str) -> Result<Option<Secret>, VaultError>;
    fn write(&self, account: &str, secret: &Secret) -> Result<(), VaultError>;
    /// Apagar o que não existe não é erro.
    fn delete(&self, account: &str) -> Result<(), VaultError>;
}

/// O cofre do sistema operacional.
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
    fn available(&self) -> bool {
        *AVAILABLE.get_or_init(probe)
    }

    fn read(&self, account: &str) -> Result<Option<Secret>, VaultError> {
        match entry(account)?.get_password() {
            // O `String` do cofre passa direto para o `Secret`, que o zera.
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(translate(error)),
        }
    }

    fn write(&self, account: &str, secret: &Secret) -> Result<(), VaultError> {
        entry(account)?
            .set_password(secret.expose())
            .map_err(translate)
    }

    fn delete(&self, account: &str) -> Result<(), VaultError> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(translate(error)),
        }
    }
}

/// O cofre do sistema responde? O resultado é guardado por processo e
/// **calculado fora da thread que chama**: na primeira chamada devolve `false`
/// e dispara o cálculo numa thread de fundo, para uma tela de configurações
/// nunca esperar o D-Bus (ADR-0073 §4, risco "`vault_available` travar a UI").
/// A resposta que a conexão manda junto da pergunta de segredo é a mesma, mas
/// calculada dentro da tarefa SSH, que pode esperar.
pub fn vault_available() -> bool {
    if let Some(known) = AVAILABLE.get() {
        return *known;
    }
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        // Se a thread não subir, o valor fica desconhecido: `false`, o lado
        // seguro (a caixa "Lembrar" fica desabilitada).
        let _ = std::thread::Builder::new()
            .name("porecatu-vault-probe".into())
            .spawn(|| {
                let _ = AVAILABLE.get_or_init(probe);
            });
    });
    false
}

/// Apaga a senha guardada para o destino (RF-20.4). Nenhuma outra entrada é
/// tocada.
pub fn forget_password(dest: &Destination) -> Result<(), VaultError> {
    SystemVault.delete(&dest.password_account())
}

/// Apaga a passphrase guardada para o arquivo de chave.
pub fn forget_passphrase(key_file: &Path) -> Result<(), VaultError> {
    SystemVault.delete(&passphrase_account(key_file))
}

#[cfg(test)]
pub(crate) mod fake {
    //! O cofre dos testes: em memória, com falhas escolhidas pelo teste.

    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    pub(crate) struct MemoryVault {
        pub(crate) unavailable: bool,
        /// A gravação falha com este erro, e nada é guardado.
        pub(crate) write_error: Mutex<Option<VaultError>>,
        entries: Mutex<HashMap<String, String>>,
        /// Contas apagadas, em ordem.
        pub(crate) deleted: Mutex<Vec<String>>,
        /// Contas gravadas, em ordem.
        pub(crate) written: Mutex<Vec<String>>,
    }

    impl MemoryVault {
        pub(crate) fn with(account: &str, secret: &str) -> Self {
            let vault = Self::default();
            vault
                .entries
                .lock()
                .unwrap()
                .insert(account.to_owned(), secret.to_owned());
            vault
        }

        pub(crate) fn unavailable() -> Self {
            Self {
                unavailable: true,
                ..Self::default()
            }
        }

        pub(crate) fn stored(&self, account: &str) -> Option<String> {
            self.entries.lock().unwrap().get(account).cloned()
        }

        pub(crate) fn is_empty(&self) -> bool {
            self.entries.lock().unwrap().is_empty()
        }
    }

    impl Vault for MemoryVault {
        fn available(&self) -> bool {
            !self.unavailable
        }

        fn read(&self, account: &str) -> Result<Option<Secret>, VaultError> {
            if self.unavailable {
                return Err(VaultError::Unavailable);
            }
            Ok(self
                .entries
                .lock()
                .unwrap()
                .get(account)
                .map(|value| Secret::new(value.clone())))
        }

        fn write(&self, account: &str, secret: &Secret) -> Result<(), VaultError> {
            if self.unavailable {
                return Err(VaultError::Unavailable);
            }
            if let Some(error) = *self.write_error.lock().unwrap() {
                return Err(error);
            }
            self.written.lock().unwrap().push(account.to_owned());
            self.entries
                .lock()
                .unwrap()
                .insert(account.to_owned(), secret.expose().to_owned());
            Ok(())
        }

        fn delete(&self, account: &str) -> Result<(), VaultError> {
            if self.unavailable {
                return Err(VaultError::Unavailable);
            }
            self.deleted.lock().unwrap().push(account.to_owned());
            self.entries.lock().unwrap().remove(account);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_password_account_has_a_lowercase_host_and_an_explicit_port() {
        let dest = Destination::new("deploy", "Prod-Web.Example.COM", 22);
        assert_eq!(
            dest.password_account(),
            "password:deploy@prod-web.example.com:22"
        );
    }

    #[test]
    fn the_user_keeps_its_case_and_the_port_is_always_written() {
        assert_eq!(
            Destination::new("Admin", "10.0.0.1", 2222).password_account(),
            "password:Admin@10.0.0.1:2222"
        );
    }

    #[test]
    fn the_passphrase_account_is_the_canonical_path_of_the_key_file() {
        let dir = std::env::temp_dir().join(format!("porecatu-vault-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        let key = dir.join("id_ed25519");
        std::fs::write(&key, b"x").unwrap();

        let plain = passphrase_account(&key);
        let roundabout = passphrase_account(&dir.join("nested").join("..").join("id_ed25519"));
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(plain, roundabout);
        assert!(plain.starts_with("passphrase:"));
        assert!(!plain.contains("\\\\?\\"), "no verbatim prefix: {plain}");
        assert!(!plain.contains(".."));
    }

    #[test]
    fn a_key_that_does_not_exist_keeps_the_path_as_written() {
        let path = Path::new("no-such-dir/id_rsa");
        assert_eq!(passphrase_account(path), "passphrase:no-such-dir/id_rsa");
    }

    #[test]
    fn verbatim_prefix_is_dropped_but_a_unc_share_is_kept() {
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\C:\Users\a\id")),
            PathBuf::from(r"C:\Users\a\id")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\UNC\server\share\id")),
            PathBuf::from(r"\\?\UNC\server\share\id")
        );
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
