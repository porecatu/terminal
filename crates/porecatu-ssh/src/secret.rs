// SPDX-License-Identifier: GPL-3.0-or-later

//! O segredo digitado pelo usuário (senha, passphrase), na forma do ADR-0073 §5.

use std::fmt;

use zeroize::Zeroizing;

/// Texto secreto. Zerado quando cai.
///
/// Sem `Clone` e sem `Display`, e com `Debug` opaco: um `{:?}` de qualquer
/// estrutura que o contenha nunca leva o valor junto. Atravessa os crates **por
/// movimento** e é descartado pela tarefa SSH depois da autenticação.
///
/// ```compile_fail
/// let secret = porecatu_ssh::Secret::new("hunter2".to_owned());
/// let _copy = secret.clone();
/// ```
///
/// ```compile_fail
/// let secret = porecatu_ssh::Secret::new("hunter2".to_owned());
/// let _text = format!("{secret}");
/// ```
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    /// Para um buffer que já é zerado (o campo mascarado): o texto muda de
    /// dono sem ser copiado.
    pub fn from_zeroizing(value: Zeroizing<String>) -> Self {
        Self(value)
    }

    /// O texto, para quem vai usá-lo (a autenticação). Não copiar.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_carries_the_value() {
        let secret = Secret::new("hunter2-proof".to_owned());
        assert_eq!(format!("{secret:?}"), "Secret(..)");
        assert!(!format!("{secret:#?}").contains("hunter2"));
    }

    #[test]
    fn a_structure_holding_a_secret_does_not_leak_it_through_debug() {
        #[derive(Debug)]
        struct Holder {
            #[allow(dead_code, reason = "read through Debug only")]
            secret: Secret,
        }
        let holder = Holder {
            secret: Secret::new("hunter2-proof".to_owned()),
        };
        assert!(!format!("{holder:?}").contains("hunter2"));
    }

    #[test]
    fn a_zeroizing_buffer_moves_in_without_copying() {
        let buffer = Zeroizing::new("moved".to_owned());
        let address = buffer.as_ptr();
        let secret = Secret::from_zeroizing(buffer);
        assert_eq!(secret.expose().as_ptr(), address);
    }
}
