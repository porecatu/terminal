// SPDX-License-Identifier: GPL-3.0-or-later

//! O grupo "Acesso remoto" da tela de configurações (PRD-025 RF-25.2 a RF-25.4,
//! ADR-0108 §1, ADR-0106 §7): o que não é uma opção do `porecatu.toml`.
//!
//! - o **token do relay**, que mora no cofre do sistema e nunca no arquivo: o
//!   campo mascarado, a pendência (digitado, ou "apagar o guardado"), a
//!   validação do ADR-0106 §7 e o que o cofre disse sobre o token guardado;
//! - o **estado do relay**: o texto (do tooltip) e o tom do indicador de cada
//!   [`RemoteStatus`].
//!
//! Estado puro -- sem `winit`, sem `wgpu`, sem cofre: quem conversa com o cofre
//! (que bloqueia) é uma thread curta de `App`, e o que ela acha chega aqui como
//! dado (`set_presence`, `written`). O texto digitado vive num
//! [`MaskedFieldState`] (zerado ao cair, sem `Clone`, `Debug` opaco) e sai por
//! **movimento** para um [`Secret`] no Salvar; nada dele é log, nem arquivo, nem
//! parte de um [`TokenView`].

use porecatu_locale::Catalog;
use porecatu_remote::protocol::pairing_uri::is_secure_relay_url;
use porecatu_remote::{RelayFailure, VaultError};
use porecatu_term::Secret;
use winit::keyboard::Key;

use super::content::StatusTone;
use super::draft::ValueError;
use super::remote_pairing::DeviceRow;
use crate::masked_field::{MaskedFieldState, MaskedKey, apply_masked_key};
use crate::messages::msg;
use crate::remote::RemoteStatus;
use crate::remote::qr::QrMatrix;

/// Menor token que o relay aceita (ADR-0106 §7).
pub(crate) const TOKEN_MIN: usize = 32;
/// Maior token que o relay aceita (ADR-0106 §7).
pub(crate) const TOKEN_MAX: usize = 128;

/// A regra do endereço do relay (ADR-0108 §1): `wss://`, ou `ws://` só para
/// `localhost`, `127.0.0.1` e `[::1]`. Vazio vale -- é "não configurado", e quem
/// liga o acesso sem endereço vê a falha no estado. A conta é a do crate de
/// protocolo (`is_secure_relay_url` normaliza antes de decidir), a mesma que o
/// host aplica ao conectar.
pub(crate) fn check_relay_url(url: &str) -> Result<(), ValueError> {
    if url.is_empty() || is_secure_relay_url(url) {
        Ok(())
    } else {
        Err(ValueError::InvalidRelayUrl)
    }
}

/// Por que um token digitado foi recusado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenError {
    TooShort,
    TooLong,
    /// Fora de ASCII imprimível (`0x20..=0x7e`), ou com espaço numa ponta (um
    /// cabeçalho HTTP o aparaa).
    BadCharacters,
}

/// A regra do token do operador, a mesma que o relay aplica ao ler o próprio
/// arquivo: 32 a 128 caracteres ASCII imprimíveis, sem espaço nas pontas.
pub(crate) fn check_token(token: &str) -> Result<(), TokenError> {
    let length = token.chars().count();
    if length < TOKEN_MIN {
        return Err(TokenError::TooShort);
    }
    if length > TOKEN_MAX {
        return Err(TokenError::TooLong);
    }
    if !token.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
        || token.starts_with(' ')
        || token.ends_with(' ')
    {
        return Err(TokenError::BadCharacters);
    }
    Ok(())
}

/// A frase de um [`TokenError`], composta pelo catálogo de textos.
pub(crate) fn token_error_text(catalog: &Catalog, error: TokenError) -> String {
    use msg::settings::remote as r;
    match error {
        TokenError::TooShort => r::token_too_short(catalog, TOKEN_MIN),
        TokenError::TooLong => r::token_too_long(catalog, TOKEN_MAX),
        TokenError::BadCharacters => r::token_bad_characters(catalog),
    }
}

/// O que o cofre disse sobre o token **guardado**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stored {
    /// A consulta ainda não voltou (ou falhou ao gravar e será refeita).
    Unknown,
    Present,
    Absent,
    /// Sem cofre utilizável: o campo fica desabilitado, e o acesso remoto não
    /// liga (ADR-0106 §1).
    Unavailable,
}

/// O que o Salvar faz com o cofre. O segredo atravessa **por movimento**.
pub(crate) enum TokenWrite {
    /// Guarda o token digitado.
    Store(Secret),
    /// Apaga o guardado (a string vazia de `store_operator_token`).
    Delete,
}

/// A frase que a linha do token mostra como descrição.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenNote {
    Stored,
    Absent,
    Unknown,
    Unavailable,
    WillStore,
    WillDelete,
}

impl TokenNote {
    pub(crate) fn text(self, catalog: &Catalog) -> String {
        use msg::settings::remote as r;
        match self {
            TokenNote::Stored => r::token_stored(catalog),
            TokenNote::Absent => r::token_absent(catalog),
            TokenNote::Unknown => r::token_unknown(catalog),
            TokenNote::Unavailable => r::token_unavailable(catalog),
            TokenNote::WillStore => r::token_will_store(catalog),
            TokenNote::WillDelete => r::token_will_delete(catalog),
        }
    }
}

/// O que a tela lê do token para montar as linhas: **nunca o texto**, só quantos
/// escalares há (os pontos) e o estado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TokenView {
    pub note: TokenNote,
    /// Quantos escalares o campo tem: um ponto cada.
    pub filled: usize,
    pub error: Option<TokenError>,
    /// "Apagar o guardado" está pendente.
    pub deleting: bool,
    /// Há token guardado (ou a exclusão dele pendente): a linha de apagar
    /// aparece.
    pub can_delete: bool,
    /// Sem cofre: o campo é esmaecido e não aceita nada.
    pub unavailable: bool,
    /// Há alteração do token pendente (o ponto de pendente da linha).
    pub pending: bool,
}

/// O QR à vista: a matriz e os segundos que a contagem mostra (nunca a URI, que
/// carrega o token de pareamento).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QrView<'a> {
    pub matrix: &'a QrMatrix,
    pub secs: u64,
}

/// Tudo o que o conteúdo do grupo lê além das opções.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RemoteAccessView<'a> {
    pub status: RemoteStatus,
    pub token: TokenView,
    /// O QR de pareamento, se está à vista (ADR-0108 §1, §2).
    pub qr: Option<QrView<'a>>,
    /// Os aparelhos pareados, na ordem do host.
    pub devices: &'a [DeviceRow],
}

/// O estado do token do relay na tela de configurações.
#[derive(Debug)]
pub(crate) struct RemoteToken {
    field: MaskedFieldState,
    /// O campo recebe teclas.
    focused: bool,
    /// "Apagar o guardado" está pendente.
    delete: bool,
    stored: Stored,
    /// A tela quer saber o que o cofre guarda, e `App` ainda não perguntou.
    probe_wanted: bool,
    /// Uma pergunta ao cofre está em voo.
    probing: bool,
}

impl RemoteToken {
    pub(crate) fn new() -> Self {
        Self {
            field: MaskedFieldState::new(),
            focused: false,
            delete: false,
            stored: Stored::Unknown,
            probe_wanted: false,
            probing: false,
        }
    }

    // ---- o que o cofre guarda

    /// O grupo veio à vista: o cofre é consultado (uma vez por vez em que o
    /// grupo é aberto; nunca com uma consulta já em voo).
    pub(crate) fn want_probe(&mut self) {
        if !self.probing {
            self.probe_wanted = true;
        }
    }

    /// `App` atende a consulta pedida: devolve se há uma a fazer e a marca em
    /// voo.
    pub(crate) fn take_probe_request(&mut self) -> bool {
        if self.probe_wanted && !self.probing {
            self.probe_wanted = false;
            self.probing = true;
            true
        } else {
            false
        }
    }

    /// A resposta de `operator_token_present`. Um erro do cofre (sem cofre,
    /// acesso negado, falha) deixa o campo desabilitado: o acesso remoto não
    /// liga sem ele.
    pub(crate) fn set_presence(&mut self, result: Result<bool, VaultError>) {
        self.probing = false;
        self.stored = match result {
            Ok(true) => Stored::Present,
            Ok(false) => Stored::Absent,
            Err(_) => Stored::Unavailable,
        };
        if self.stored != Stored::Present {
            self.delete = false;
        }
    }

    /// A gravação no cofre terminou. Certa, o guardado é o que se escreveu (ou
    /// nada, se foi apagado); errada, não se sabe -- o cofre é consultado de
    /// novo.
    pub(crate) fn written(&mut self, deleted: bool, ok: bool) {
        if ok {
            self.stored = if deleted {
                Stored::Absent
            } else {
                Stored::Present
            };
        } else {
            self.stored = Stored::Unknown;
            self.probe_wanted = true;
        }
    }

    #[cfg(test)]
    pub(crate) fn stored(&self) -> Stored {
        self.stored
    }

    pub(crate) fn unavailable(&self) -> bool {
        self.stored == Stored::Unavailable
    }

    // ---- a pendência

    /// Há token digitado ou exclusão pedida: entra no "há mudanças".
    pub(crate) fn is_dirty(&self) -> bool {
        !self.field.is_empty() || self.delete
    }

    /// Quantas pendências o token soma ao diálogo de fechar (zero ou uma).
    pub(crate) fn pending_count(&self) -> usize {
        usize::from(self.is_dirty())
    }

    /// O token digitado é recusado: o Salvar fica bloqueado como nas outras
    /// validações.
    pub(crate) fn error(&self) -> Option<TokenError> {
        if self.field.is_empty() {
            None
        } else {
            self.field.inspect(check_token).err()
        }
    }

    pub(crate) fn has_invalid(&self) -> bool {
        self.error().is_some()
    }

    /// A frase da descrição da linha, pela ordem: o que o Salvar vai fazer, e só
    /// então o que o cofre guarda.
    pub(crate) fn note(&self) -> TokenNote {
        if self.delete {
            TokenNote::WillDelete
        } else if !self.field.is_empty() {
            TokenNote::WillStore
        } else {
            match self.stored {
                Stored::Unknown => TokenNote::Unknown,
                Stored::Present => TokenNote::Stored,
                Stored::Absent => TokenNote::Absent,
                Stored::Unavailable => TokenNote::Unavailable,
            }
        }
    }

    pub(crate) fn view(&self) -> TokenView {
        TokenView {
            note: self.note(),
            filled: self.field.len(),
            error: self.error(),
            deleting: self.delete,
            can_delete: self.stored == Stored::Present || self.delete,
            unavailable: self.unavailable(),
            pending: self.is_dirty(),
        }
    }

    /// O botão "Apagar" da linha: alterna a exclusão pendente. Só existe com
    /// token guardado; pedir apagar descarta o que estava digitado.
    pub(crate) fn toggle_delete(&mut self) -> bool {
        if self.unavailable() || (self.stored != Stored::Present && !self.delete) {
            return false;
        }
        self.delete = !self.delete;
        if self.delete {
            drop(self.field.take());
        }
        true
    }

    /// Descartar: nada digitado, nada a apagar, sem foco. O texto é zerado.
    pub(crate) fn discard(&mut self) {
        drop(self.field.take());
        self.delete = false;
        self.focused = false;
    }

    /// O Salvar chegou: o que vai ao cofre, **movido** para fora do campo, e o
    /// estado volta ao repouso. `None` sem pendência (ou com o token recusado:
    /// o Salvar já estava bloqueado).
    pub(crate) fn take_write(&mut self) -> Option<TokenWrite> {
        self.focused = false;
        if self.delete {
            self.delete = false;
            return Some(TokenWrite::Delete);
        }
        if self.field.is_empty() || self.has_invalid() {
            return None;
        }
        Some(TokenWrite::Store(self.field.take()))
    }

    // ---- o campo

    pub(crate) fn is_focused(&self) -> bool {
        self.focused
    }

    pub(crate) fn focus(&mut self) {
        if !self.unavailable() {
            self.focused = true;
        }
    }

    pub(crate) fn blur(&mut self) {
        self.focused = false;
    }

    /// O cursor e a seleção do campo, em pontos, para a pintura.
    pub(crate) fn display(&self) -> &crate::text_field::TextFieldState {
        self.field.display()
    }

    /// Uma tecla de edição. Digitar cancela uma exclusão pendente: o que se
    /// digita é o token novo.
    pub(crate) fn key(
        &mut self,
        logical_key: &Key,
        text: Option<&str>,
        modifiers: porecatu_term::Modifiers,
    ) -> MaskedKey {
        if self.unavailable() {
            return MaskedKey::Ignored;
        }
        let outcome = apply_masked_key(&mut self.field, logical_key, text, modifiers);
        if outcome == MaskedKey::Handled && !self.field.is_empty() {
            self.delete = false;
        }
        outcome
    }

    /// Cola o que veio da área de transferência.
    pub(crate) fn paste(&mut self, text: &str) {
        if self.unavailable() {
            return;
        }
        self.field.paste(text);
        if !self.field.is_empty() {
            self.delete = false;
        }
    }
}

// ---- a linha de estado

/// Qual das frases de estado vale (RF-25.4, ADR-0108 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusText {
    Off,
    Connecting,
    Connected,
    Unreachable,
    TokenMissing,
    TokenRejected,
    RelayFull,
    TooManyConnections,
    /// O erro de protocolo da lista do ADR-0108: o registro recusado
    /// (`4001`/`4004`/`4005`, envelope inválido), e também `Replaced` e
    /// `InvalidUrl`, que o ADR não lista à parte.
    Protocol,
    VaultUnavailable,
}

/// A decisão pura: de que estado sai que frase.
pub(crate) fn status_text_key(status: RemoteStatus) -> StatusText {
    match status {
        RemoteStatus::Off => StatusText::Off,
        RemoteStatus::Connecting => StatusText::Connecting,
        RemoteStatus::Connected => StatusText::Connected,
        RemoteStatus::Failed(failure) => match failure {
            RelayFailure::Unreachable => StatusText::Unreachable,
            RelayFailure::TokenMissing => StatusText::TokenMissing,
            RelayFailure::TokenRejected => StatusText::TokenRejected,
            RelayFailure::RelayFull => StatusText::RelayFull,
            RelayFailure::TooManyConnections => StatusText::TooManyConnections,
            RelayFailure::VaultUnavailable => StatusText::VaultUnavailable,
            RelayFailure::Protocol | RelayFailure::Replaced | RelayFailure::InvalidUrl => {
                StatusText::Protocol
            }
        },
    }
}

/// A linha de estado, composta pelo catálogo de textos.
pub(crate) fn status_text(catalog: &Catalog, status: RemoteStatus) -> String {
    use msg::settings::remote as r;
    match status_text_key(status) {
        StatusText::Off => r::status_off(catalog),
        StatusText::Connecting => r::status_connecting(catalog),
        StatusText::Connected => r::status_connected(catalog),
        StatusText::Unreachable => r::status_unreachable(catalog),
        StatusText::TokenMissing => r::status_token_missing(catalog),
        StatusText::TokenRejected => r::status_token_rejected(catalog),
        StatusText::RelayFull => r::status_relay_full(catalog),
        StatusText::TooManyConnections => r::status_too_many_connections(catalog),
        StatusText::Protocol => r::status_protocol(catalog),
        StatusText::VaultUnavailable => r::status_vault_unavailable(catalog),
    }
}

/// O tom do indicador ao lado do endereço do relay: Acento conectado, Aviso
/// conectando, Erro numa falha, e o Terciário desligado.
pub(crate) fn status_tone(status: RemoteStatus) -> StatusTone {
    match status {
        RemoteStatus::Off => StatusTone::Off,
        RemoteStatus::Connecting => StatusTone::Connecting,
        RemoteStatus::Connected => StatusTone::Connected,
        RemoteStatus::Failed(_) => StatusTone::Failed,
    }
}

/// Depois de gravar o token, o host precisa recomeçar (ele lê o token só ao
/// iniciar) quando está parado numa falha que o token resolve -- ausente ou
/// recusado -- **ou ainda conectando**: ligar o acesso e gravar o token no mesmo
/// Salvar põe o host de pé antes de o cofre ter o token, e a falha dele chega
/// depois da gravação. Um host conectado não recomeça (a sessão em curso não
/// depende do token novo), nem um que falhou por outro motivo.
pub(crate) fn token_change_restarts_host(status: RemoteStatus) -> bool {
    matches!(
        status,
        RemoteStatus::Connecting
            | RemoteStatus::Failed(RelayFailure::TokenMissing | RelayFailure::TokenRejected)
    )
}

#[cfg(test)]
mod tests {
    use porecatu_term::Modifiers;
    use winit::keyboard::NamedKey;

    use super::*;
    use crate::messages::test_support;

    const GOOD: &str = "0123456789abcdef0123456789abcdef";

    fn no_mods() -> Modifiers {
        Modifiers {
            shift: false,
            ctrl: false,
            alt: false,
            super_: false,
        }
    }

    fn typed(token: &str) -> RemoteToken {
        let mut state = RemoteToken::new();
        state.paste(token);
        state
    }

    // ---- the relay address

    #[test]
    fn the_relay_address_is_wss_or_a_local_ws_or_empty() {
        for ok in [
            "",
            "wss://relay.example.com",
            "wss://relay.example.com:8443/prefix",
            "ws://localhost:8080",
            "ws://127.0.0.1",
            "ws://[::1]:9000",
        ] {
            assert_eq!(check_relay_url(ok), Ok(()), "{ok}");
        }
        for bad in [
            "ws://relay.example.com",
            "http://relay.example.com",
            "relay.example.com",
            "wss://",
            "wss://user@relay.example.com",
            " wss://relay.example.com",
            "https://relay.example.com",
        ] {
            assert_eq!(
                check_relay_url(bad),
                Err(ValueError::InvalidRelayUrl),
                "{bad}"
            );
        }
    }

    // ---- the token rule

    #[test]
    fn a_token_has_32_to_128_printable_ascii_characters() {
        assert_eq!(
            check_token(&"a".repeat(TOKEN_MIN - 1)),
            Err(TokenError::TooShort)
        );
        assert_eq!(check_token(&"a".repeat(TOKEN_MIN)), Ok(()));
        assert_eq!(check_token(&"a".repeat(TOKEN_MAX)), Ok(()));
        assert_eq!(
            check_token(&"a".repeat(TOKEN_MAX + 1)),
            Err(TokenError::TooLong)
        );
        // 0x20 and 0x7e are in; the controls and anything past ASCII are out.
        assert_eq!(check_token(&format!("a b{}~", "c".repeat(30))), Ok(()));
        assert_eq!(
            check_token(&format!("{}\u{7f}", "a".repeat(40))),
            Err(TokenError::BadCharacters)
        );
        assert_eq!(
            check_token(&format!("{}é", "a".repeat(40))),
            Err(TokenError::BadCharacters)
        );
        // A header would trim a space at either end.
        assert_eq!(
            check_token(&format!(" {}", "a".repeat(40))),
            Err(TokenError::BadCharacters)
        );
        assert_eq!(
            check_token(&format!("{} ", "a".repeat(40))),
            Err(TokenError::BadCharacters)
        );
    }

    #[test]
    fn the_length_counts_characters_not_bytes() {
        // 31 two-byte characters are 62 bytes and still too short -- and not
        // ASCII either; the length is judged first, like the relay does.
        assert_eq!(check_token(&"é".repeat(31)), Err(TokenError::TooShort));
        assert_eq!(check_token(&"é".repeat(32)), Err(TokenError::BadCharacters));
    }

    #[test]
    fn the_token_error_texts_come_from_the_catalog() {
        let catalog = test_support::pt_br();
        assert_eq!(
            token_error_text(&catalog, TokenError::TooShort),
            "O token tem de ter ao menos 32 caracteres."
        );
        assert_eq!(
            token_error_text(&catalog, TokenError::TooLong),
            "O token tem de ter no máximo 128 caracteres."
        );
        assert!(!token_error_text(&catalog, TokenError::BadCharacters).is_empty());
    }

    // ---- the pending token

    #[test]
    fn a_typed_token_is_pending_and_a_wrong_one_blocks_the_save() {
        let mut state = RemoteToken::new();
        assert!(!state.is_dirty());
        assert!(!state.has_invalid());
        state.paste("short");
        assert!(state.is_dirty());
        assert_eq!(state.error(), Some(TokenError::TooShort));
        assert!(state.has_invalid());
        assert_eq!(state.pending_count(), 1);
        // Too short never goes to the vault.
        assert!(state.take_write().is_none());
    }

    #[test]
    fn a_good_token_leaves_by_movement_and_the_field_is_empty_after() {
        let mut state = typed(GOOD);
        assert_eq!(state.error(), None);
        let Some(TokenWrite::Store(secret)) = state.take_write() else {
            panic!("a token to store");
        };
        assert_eq!(secret.expose(), GOOD);
        assert!(!state.is_dirty());
        assert_eq!(state.view().filled, 0);
    }

    #[test]
    fn the_view_and_the_debug_never_carry_the_text() {
        let state = typed(GOOD);
        let view = state.view();
        assert_eq!(view.filled, GOOD.chars().count());
        assert!(!format!("{view:?}").contains("0123456789abcdef"));
        assert!(!format!("{state:?}").contains("0123456789abcdef"));
    }

    #[test]
    fn discard_empties_the_field_and_the_delete() {
        let mut state = typed(GOOD);
        state.discard();
        assert!(!state.is_dirty());
        state.set_presence(Ok(true));
        assert!(state.toggle_delete());
        state.discard();
        assert!(!state.is_dirty());
    }

    // ---- what the vault says

    #[test]
    fn the_probe_is_asked_once_and_answered_into_the_stored_state() {
        let mut state = RemoteToken::new();
        assert!(!state.take_probe_request());
        state.want_probe();
        assert!(state.take_probe_request());
        // In flight: asking again does not duplicate it.
        state.want_probe();
        assert!(!state.take_probe_request());
        assert_eq!(state.note(), TokenNote::Unknown);
        state.set_presence(Ok(true));
        assert_eq!(state.stored(), Stored::Present);
        assert_eq!(state.note(), TokenNote::Stored);
        state.want_probe();
        assert!(state.take_probe_request());
        state.set_presence(Ok(false));
        assert_eq!(state.note(), TokenNote::Absent);
    }

    #[test]
    fn a_vault_that_fails_disables_the_field() {
        for error in [
            VaultError::Unavailable,
            VaultError::Denied,
            VaultError::Failed,
        ] {
            let mut state = RemoteToken::new();
            state.set_presence(Err(error));
            assert!(state.unavailable());
            assert_eq!(state.note(), TokenNote::Unavailable);
            // Nothing is typed, focused or deleted in a field that is off.
            state.focus();
            assert!(!state.is_focused());
            state.paste(GOOD);
            assert!(!state.is_dirty());
            assert!(!state.toggle_delete());
            assert_eq!(
                state.key(&Key::Character("a".into()), Some("a"), no_mods()),
                MaskedKey::Ignored
            );
        }
    }

    #[test]
    fn delete_only_exists_with_a_stored_token_and_is_a_toggle() {
        let mut state = RemoteToken::new();
        state.set_presence(Ok(false));
        assert!(!state.view().can_delete);
        assert!(!state.toggle_delete());
        state.set_presence(Ok(true));
        assert!(state.view().can_delete);
        assert!(state.toggle_delete());
        assert_eq!(state.note(), TokenNote::WillDelete);
        assert!(state.is_dirty());
        assert!(state.view().deleting);
        // Again: back to keeping it.
        assert!(state.toggle_delete());
        assert!(!state.is_dirty());
    }

    #[test]
    fn deleting_wins_over_the_typed_text_and_typing_cancels_the_delete() {
        let mut state = typed(GOOD);
        state.set_presence(Ok(true));
        assert_eq!(state.note(), TokenNote::WillStore);
        // Asking to delete drops what was typed.
        assert!(state.toggle_delete());
        assert_eq!(state.view().filled, 0);
        assert!(matches!(state.take_write(), Some(TokenWrite::Delete)));
        assert!(!state.is_dirty());
        // Typing after asking to delete is the new token.
        state.set_presence(Ok(true));
        assert!(state.toggle_delete());
        state.paste(GOOD);
        assert!(!state.view().deleting);
        assert!(matches!(state.take_write(), Some(TokenWrite::Store(_))));
    }

    #[test]
    fn a_finished_write_updates_what_is_stored_and_a_failed_one_asks_again() {
        let mut state = RemoteToken::new();
        state.written(false, true);
        assert_eq!(state.stored(), Stored::Present);
        state.written(true, true);
        assert_eq!(state.stored(), Stored::Absent);
        state.written(false, false);
        assert_eq!(state.stored(), Stored::Unknown);
        assert!(state.take_probe_request());
    }

    // ---- the keys

    #[test]
    fn the_keys_edit_the_field_and_copy_does_nothing() {
        let mut state = RemoteToken::new();
        for c in "abc".chars() {
            let text = c.to_string();
            assert_eq!(
                state.key(&Key::Character(text.clone().into()), Some(&text), no_mods()),
                MaskedKey::Handled
            );
        }
        assert_eq!(state.view().filled, 3);
        assert_eq!(
            state.key(&Key::Named(NamedKey::Backspace), None, no_mods()),
            MaskedKey::Handled
        );
        assert_eq!(state.view().filled, 2);
        assert_eq!(
            state.key(&Key::Named(NamedKey::Enter), None, no_mods()),
            MaskedKey::Ignored
        );
        assert_eq!(
            state.key(&Key::Named(NamedKey::Escape), None, no_mods()),
            MaskedKey::Ignored
        );
    }

    // ---- the status line

    #[test]
    fn every_status_has_its_own_sentence() {
        let cases = [
            (RemoteStatus::Off, "Desligado"),
            (RemoteStatus::Connecting, "Conectando"),
            (RemoteStatus::Connected, "Conectado"),
            (
                RemoteStatus::Failed(RelayFailure::Unreachable),
                "Falha: relay inacessível",
            ),
            (
                RemoteStatus::Failed(RelayFailure::TokenMissing),
                "Falha: token ausente",
            ),
            (
                RemoteStatus::Failed(RelayFailure::TokenRejected),
                "Falha: token recusado",
            ),
            (
                RemoteStatus::Failed(RelayFailure::RelayFull),
                "Falha: relay cheio",
            ),
            (
                RemoteStatus::Failed(RelayFailure::TooManyConnections),
                "Falha: excesso de conexões",
            ),
            (
                RemoteStatus::Failed(RelayFailure::Protocol),
                "Falha: erro de protocolo",
            ),
            (
                RemoteStatus::Failed(RelayFailure::VaultUnavailable),
                "Falha: cofre indisponível",
            ),
        ];
        let catalog = test_support::pt_br();
        for (status, expected) in cases {
            assert_eq!(status_text(&catalog, status), expected, "{status:?}");
        }
    }

    #[test]
    fn replaced_and_an_invalid_url_read_as_a_protocol_error() {
        assert_eq!(
            status_text_key(RemoteStatus::Failed(RelayFailure::Replaced)),
            StatusText::Protocol
        );
        assert_eq!(
            status_text_key(RemoteStatus::Failed(RelayFailure::InvalidUrl)),
            StatusText::Protocol
        );
    }

    #[test]
    fn each_state_has_its_own_indicator_tone_and_every_failure_is_one() {
        assert_eq!(status_tone(RemoteStatus::Off), StatusTone::Off);
        assert_eq!(
            status_tone(RemoteStatus::Connecting),
            StatusTone::Connecting
        );
        assert_eq!(status_tone(RemoteStatus::Connected), StatusTone::Connected);
        for failure in [
            RelayFailure::Unreachable,
            RelayFailure::TokenMissing,
            RelayFailure::TokenRejected,
            RelayFailure::RelayFull,
            RelayFailure::TooManyConnections,
            RelayFailure::Protocol,
            RelayFailure::Replaced,
            RelayFailure::InvalidUrl,
            RelayFailure::VaultUnavailable,
        ] {
            assert_eq!(
                status_tone(RemoteStatus::Failed(failure)),
                StatusTone::Failed,
                "{failure:?}"
            );
        }
    }

    #[test]
    fn a_new_token_restarts_the_host_only_for_a_token_failure_or_while_connecting() {
        assert!(token_change_restarts_host(RemoteStatus::Connecting));
        assert!(token_change_restarts_host(RemoteStatus::Failed(
            RelayFailure::TokenMissing
        )));
        assert!(token_change_restarts_host(RemoteStatus::Failed(
            RelayFailure::TokenRejected
        )));
        for status in [
            RemoteStatus::Off,
            RemoteStatus::Connected,
            RemoteStatus::Failed(RelayFailure::Unreachable),
            RemoteStatus::Failed(RelayFailure::RelayFull),
        ] {
            assert!(!token_change_restarts_host(status), "{status:?}");
        }
    }
}
