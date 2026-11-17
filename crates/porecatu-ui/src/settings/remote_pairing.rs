// SPDX-License-Identifier: GPL-3.0-or-later

//! O pareamento e os aparelhos pareados do grupo "Acesso remoto" (PRD-025
//! RF-25.5 a RF-25.11, ADR-0106 §2, §3 e §6, ADR-0108 §1 a §3).
//!
//! Estado puro -- sem `winit`, sem `wgpu`, sem rede: o que o `App` sabe do host
//! (a lista de aparelhos, o QR, o pedido de pareamento) chega aqui como dado, e
//! o que a tela quer do host sai como [`RemoteRequest`], que `App` atende. Os
//! relógios recebem o `Instant` de fora.
//!
//! O que mora aqui:
//!
//! - o **QR** que o host mandou ([`PairingQr`]): a matriz, o fim da validade e a
//!   contagem regressiva, com o prazo em que o texto da contagem muda;
//! - o **pedido de pareamento** que espera a resposta ([`PairingPrompt`]), com o
//!   prazo de 120 s;
//! - a **lista de aparelhos** ([`DeviceRow`]) e as frases de cada linha;
//! - as **decisões puras** (botão habilitado, "conectado agora", data curta).
//!
//! A URI do QR carrega o token de pareamento: ela só vive em [`PairingQr`], que
//! não implementa `Debug` com o conteúdo, e nunca vai para log.

use std::fmt;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, TimeZone};
use porecatu_locale::Catalog;
use porecatu_remote::{DeviceInfo, DevicePlatform, PairingAnswer, PairingId};

use crate::dialog::DialogAction;
use crate::messages::msg;
use crate::remote::PendingPairing;
use crate::remote::RemoteStatus;
use crate::remote::qr::{QrMatrix, qr_matrix};

/// O pedido de pareamento sem resposta em 120 s é recusado (ADR-0106 §3,
/// RF-25.7).
pub(crate) const PAIRING_PROMPT_TIMEOUT: Duration = Duration::from_secs(120);

/// O que a tela de configurações pede ao host. `App` atende na volta do event
/// loop (`App::service_remote_requests`): a janela não tem o `RemoteHost`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteRequest {
    /// "Mostrar QR code": um token novo, que invalida o anterior.
    ShowPairing,
    /// Descarta o token vivo: o QR some da tela.
    CancelPairing,
    /// Revoga o aparelho de chave `key`.
    Revoke([u8; 32]),
    /// Recria a identidade do desktop.
    RecreateIdentity,
    /// A resposta ao pedido de pareamento `id`.
    AnswerPairing {
        id: PairingId,
        answer: PairingAnswer,
    },
}

// ---- decisões puras

/// O que a resposta de um diálogo de revogar ou de recriar a identidade pede ao
/// host: o pedido, **só** quando se confirma. Cancelar e `Esc` não pedem nada.
/// Os outros diálogos (excluir perfil, pareamento) não passam por aqui.
pub(crate) fn dialog_request(action: &DialogAction, confirm: bool) -> Option<RemoteRequest> {
    if !confirm {
        return None;
    }
    match action {
        DialogAction::RevokeDevice(key) => Some(RemoteRequest::Revoke(*key)),
        DialogAction::RecreateIdentity => Some(RemoteRequest::RecreateIdentity),
        _ => None,
    }
}

/// "Mostrar QR code" só vale com o relay conectado: o host só gera token
/// registrado (ADR-0108 §1).
pub(crate) fn pairing_available(status: RemoteStatus) -> bool {
    status == RemoteStatus::Connected
}

/// Há um host vivo: a lista de aparelhos é a dele e revogar ou recriar são
/// comandos a ele (ADR-0106 §6: o comando vai por `try_send`; não há runtime
/// para mexer no cofre e na lista com o acesso desligado, RNF-25.1).
pub(crate) fn host_present(status: RemoteStatus) -> bool {
    status != RemoteStatus::Off
}

/// O aparelho de chave `key` tem sessão aberta agora?
pub(crate) fn is_connected_now(key: &[u8; 32], client_keys: &[[u8; 32]]) -> bool {
    client_keys.contains(key)
}

/// A lista como a tela a mostra: a ordem do host, e o "conectado agora" pela
/// chave comparada com a das sessões abertas.
pub(crate) fn device_rows(devices: &[DeviceInfo], client_keys: &[[u8; 32]]) -> Vec<DeviceRow> {
    devices
        .iter()
        .map(|device| DeviceRow {
            key: device.key,
            name: device.name.clone(),
            platform: device.platform,
            paired_at: device.paired_at.clone(),
            last_seen: device.last_seen.clone(),
            connected: is_connected_now(&device.key, client_keys),
        })
        .collect()
}

/// Segundos que faltam, arredondados para cima (um QR que vale mais 0,2 s ainda
/// diz "0:01"); zero a partir do fim da validade.
pub(crate) fn countdown_secs(expires_at: Instant, now: Instant) -> u64 {
    let remaining = expires_at.saturating_duration_since(now);
    remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
}

/// "m:ss": minutos sem zero à esquerda, segundos com dois dígitos.
pub(crate) fn countdown_text(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Quando o texto da contagem, que mostra `shown` segundos, passa a mostrar um a
/// menos: o instante em que faltam `shown - 1` segundos. Sem contagem (zero),
/// nenhum prazo. É o prazo de até 1 s que o `WaitUntil` recebe enquanto o QR
/// está à vista.
pub(crate) fn next_countdown_change(expires_at: Instant, shown: u64) -> Option<Instant> {
    let shown = shown.checked_sub(1)?;
    expires_at.checked_sub(Duration::from_secs(shown))
}

/// A data curta no fuso `tz`: `AAAA-MM-DD`, sem ambiguidade entre idiomas.
/// `None` se o texto não é uma data RFC 3339.
pub(crate) fn short_date<Tz>(rfc3339: &str, tz: &Tz) -> Option<String>
where
    Tz: TimeZone,
    Tz::Offset: fmt::Display,
{
    let parsed = DateTime::parse_from_rfc3339(rfc3339).ok()?;
    Some(parsed.with_timezone(tz).format("%Y-%m-%d").to_string())
}

/// O nome da plataforma, do catálogo.
pub(crate) fn platform_text(catalog: &Catalog, platform: DevicePlatform) -> String {
    use msg::settings::remote as r;
    match platform {
        DevicePlatform::Android => r::platform_android(catalog),
        DevicePlatform::Ios => r::platform_ios(catalog),
        DevicePlatform::Other => r::platform_other(catalog),
    }
}

/// A linha de detalhe de um aparelho -- "plataforma · pareado em DATA · último
/// acesso DATA" --, com as datas no fuso `tz`. Um `last_seen` vazio ou ausente
/// é "nunca"; um texto que não é data aparece como veio.
pub(crate) fn device_detail_in<Tz>(catalog: &Catalog, row: &DeviceRow, tz: &Tz) -> String
where
    Tz: TimeZone,
    Tz::Offset: fmt::Display,
{
    use msg::settings::remote as r;
    let date = |text: &str| short_date(text, tz).unwrap_or_else(|| text.to_owned());
    let seen = match row.last_seen.as_deref() {
        None | Some("") => r::device_never(catalog),
        Some(text) => date(text),
    };
    r::device_detail(
        catalog,
        platform_text(catalog, row.platform),
        date(&row.paired_at),
        seen,
    )
}

/// [`device_detail_in`] na hora local do sistema.
pub(crate) fn device_detail(catalog: &Catalog, row: &DeviceRow) -> String {
    device_detail_in(catalog, row, &Local)
}

// ---- os dados

/// Um aparelho pareado como a tela o lista.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceRow {
    pub key: [u8; 32],
    pub name: String,
    pub platform: DevicePlatform,
    /// RFC 3339, UTC.
    pub paired_at: String,
    pub last_seen: Option<String>,
    /// Tem sessão aberta agora.
    pub connected: bool,
}

/// O QR que o host mandou: a URI (que carrega o token), a matriz que dela sai e a
/// validade. A URI nunca é impressa: `Debug` é opaco.
pub(crate) struct PairingQr {
    uri: String,
    matrix: QrMatrix,
    expires_at: Instant,
    /// Os segundos que o texto da contagem mostra agora.
    shown: u64,
}

impl fmt::Debug for PairingQr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingQr").finish_non_exhaustive()
    }
}

impl PairingQr {
    /// `None` se a URI não cabe num QR, ou se a validade já passou.
    pub(crate) fn new(uri: String, expires_at: Instant, now: Instant) -> Option<Self> {
        let shown = countdown_secs(expires_at, now);
        if shown == 0 {
            return None;
        }
        let matrix = qr_matrix(&uri)?;
        Some(Self {
            uri,
            matrix,
            expires_at,
            shown,
        })
    }

    /// O link de pareamento, para a área de transferência.
    pub(crate) fn uri(&self) -> &str {
        &self.uri
    }

    pub(crate) fn matrix(&self) -> &QrMatrix {
        &self.matrix
    }

    pub(crate) fn shown_secs(&self) -> u64 {
        self.shown
    }
}

/// O pedido de pareamento que espera a resposta do usuário.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PairingPrompt {
    pub id: PairingId,
    pub device_name: String,
    pub platform: DevicePlatform,
    /// "ABCD EFGH IJKL MNOP", como o host mandou.
    pub fingerprint: String,
    deadline: Instant,
}

impl PairingPrompt {
    pub(crate) fn new(pending: PendingPairing, now: Instant) -> Self {
        Self {
            id: pending.id,
            device_name: pending.device_name,
            platform: pending.platform,
            fingerprint: pending.fingerprint,
            deadline: now + PAIRING_PROMPT_TIMEOUT,
        }
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    /// O prazo de 120 s venceu.
    pub(crate) fn expired(&self, now: Instant) -> bool {
        now >= self.deadline
    }
}

/// O que um `tick` mudou.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Tick {
    /// O que a tela mostra mudou (a contagem, ou o QR que expirou).
    pub redraw: bool,
    /// O pedido de pareamento venceu: o diálogo dele fecha.
    pub prompt_expired: bool,
}

/// O estado do pareamento e da lista de aparelhos na tela de configurações.
#[derive(Debug, Default)]
pub(crate) struct PairingPanel {
    devices: Vec<DeviceRow>,
    qr: Option<PairingQr>,
    prompt: Option<PairingPrompt>,
    requests: Vec<RemoteRequest>,
}

impl PairingPanel {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    // ---- aparelhos

    pub(crate) fn devices(&self) -> &[DeviceRow] {
        &self.devices
    }

    /// A lista mudou? Só então a tela refaz o conteúdo.
    pub(crate) fn set_devices(&mut self, rows: Vec<DeviceRow>) -> bool {
        if self.devices == rows {
            return false;
        }
        self.devices = rows;
        true
    }

    // ---- o QR

    pub(crate) fn qr(&self) -> Option<&PairingQr> {
        self.qr.as_ref()
    }

    /// O host mandou o QR. Substitui o que havia (o host invalidou o anterior).
    /// `false` se a URI não gera um QR ou já venceu: nada aparece.
    pub(crate) fn show_qr(&mut self, uri: String, expires_at: Instant, now: Instant) -> bool {
        self.qr = PairingQr::new(uri, expires_at, now);
        self.qr.is_some()
    }

    /// O QR sai da tela. Com `cancel`, e se havia um, o token do host é
    /// descartado (RF-25.6: mostrar de novo, sair do grupo ou fechar a tela
    /// invalidam o anterior). `true` se havia QR.
    pub(crate) fn drop_qr(&mut self, cancel: bool) -> bool {
        let had = self.qr.take().is_some();
        if had && cancel {
            self.requests.push(RemoteRequest::CancelPairing);
        }
        had
    }

    /// "Mostrar QR code": o pedido, se o estado deixa.
    pub(crate) fn request_show(&mut self, status: RemoteStatus) -> bool {
        if !pairing_available(status) {
            return false;
        }
        self.requests.push(RemoteRequest::ShowPairing);
        true
    }

    /// Saiu do grupo, ou a tela vai fechar: o QR some e o token é descartado.
    pub(crate) fn leave_group(&mut self) -> bool {
        self.drop_qr(true)
    }

    // ---- o pedido de pareamento

    pub(crate) fn prompt(&self) -> Option<&PairingPrompt> {
        self.prompt.as_ref()
    }

    /// Um aparelho apresentou o token: o pedido espera a resposta (120 s). O
    /// token foi consumido, então o QR sai (sem `CancelPairing`: o host já o
    /// descartou). Não há fila (ADR-0106 §3): se já havia um pedido, ele é
    /// recusado.
    pub(crate) fn prompt_arrived(&mut self, pending: PendingPairing, now: Instant) {
        self.qr = None;
        if let Some(old) = self.prompt.take() {
            self.requests.push(RemoteRequest::AnswerPairing {
                id: old.id,
                answer: PairingAnswer::Decline,
            });
        }
        self.prompt = Some(PairingPrompt::new(pending, now));
    }

    /// O canal do pedido morreu antes da resposta: o pedido some, sem resposta
    /// a dar. `true` se era o pedido em espera.
    pub(crate) fn prompt_closed(&mut self, id: PairingId) -> bool {
        if self.prompt.as_ref().is_some_and(|prompt| prompt.id == id) {
            self.prompt = None;
            return true;
        }
        false
    }

    /// A resposta do usuário: Parear (`accept`) ou Recusar. O pedido sai.
    pub(crate) fn answer_prompt(&mut self, accept: bool) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        self.requests.push(RemoteRequest::AnswerPairing {
            id: prompt.id,
            answer: if accept {
                PairingAnswer::Accept
            } else {
                PairingAnswer::Decline
            },
        });
    }

    // ---- o relógio

    /// O prazo mais próximo: a próxima virada da contagem (no máximo 1 s) e o fim
    /// dos 120 s do pedido. Sem QR e sem pedido, nenhum -- a tela não acorda o
    /// event loop à toa.
    pub(crate) fn next_wake(&self) -> Option<Instant> {
        let countdown = self
            .qr
            .as_ref()
            .and_then(|qr| next_countdown_change(qr.expires_at, qr.shown));
        let prompt = self.prompt.as_ref().map(PairingPrompt::deadline);
        [countdown, prompt].into_iter().flatten().min()
    }

    /// O relógio andou até `now`: a contagem se atualiza, o QR vencido sai, e o
    /// pedido sem resposta em 120 s é recusado por prazo (`Timeout`).
    pub(crate) fn tick(&mut self, now: Instant) -> Tick {
        let mut tick = Tick::default();
        if let Some(qr) = &mut self.qr {
            let secs = countdown_secs(qr.expires_at, now);
            if secs == 0 {
                // Vencido: o host também o esquece; não há o que cancelar.
                self.qr = None;
                tick.redraw = true;
            } else if secs != qr.shown {
                qr.shown = secs;
                tick.redraw = true;
            }
        }
        if self
            .prompt
            .as_ref()
            .is_some_and(|prompt| prompt.expired(now))
        {
            if let Some(prompt) = self.prompt.take() {
                self.requests.push(RemoteRequest::AnswerPairing {
                    id: prompt.id,
                    answer: PairingAnswer::Timeout,
                });
            }
            tick.prompt_expired = true;
            tick.redraw = true;
        }
        tick
    }

    // ---- fechar a tela

    /// A tela vai fechar: o pedido em espera é recusado e o QR, cancelado.
    pub(crate) fn close(&mut self) {
        self.answer_prompt(false);
        self.drop_qr(true);
    }

    // ---- o que a tela pede ao host

    /// Um pedido que a tela fez (revogar, recriar).
    pub(crate) fn request(&mut self, request: RemoteRequest) {
        self.requests.push(request);
    }

    /// Os pedidos feitos até agora, na ordem; a fila esvazia.
    pub(crate) fn take_requests(&mut self) -> Vec<RemoteRequest> {
        std::mem::take(&mut self.requests)
    }
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;
    use crate::messages::test_support;

    const URI: &str = "porecatu://pair?v=0&relay=wss%3A%2F%2Frelay.example.com&host=abcdefghijklmnopqrstuvwxyz234567&key=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&token=AAAAAAAAAAAAAAAAAAAAAA&name=Desk";

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn pending(id: u64) -> PendingPairing {
        PendingPairing {
            id: PairingId(id),
            device_name: "Pixel".to_owned(),
            platform: DevicePlatform::Android,
            fingerprint: "ABCD EFGH IJKL MNOP".to_owned(),
        }
    }

    fn row(connected: bool, last_seen: Option<&str>) -> DeviceRow {
        DeviceRow {
            key: [1; 32],
            name: "Pixel".to_owned(),
            platform: DevicePlatform::Android,
            paired_at: "2026-11-15T14:03:00Z".to_owned(),
            last_seen: last_seen.map(str::to_owned),
            connected,
        }
    }

    // ---- the dialogs

    #[test]
    fn only_a_confirmed_revoke_or_recreate_asks_the_host() {
        let revoke = DialogAction::RevokeDevice([5; 32]);
        assert_eq!(
            dialog_request(&revoke, true),
            Some(RemoteRequest::Revoke([5; 32]))
        );
        assert_eq!(dialog_request(&revoke, false), None);
        assert_eq!(
            dialog_request(&DialogAction::RecreateIdentity, true),
            Some(RemoteRequest::RecreateIdentity)
        );
        assert_eq!(dialog_request(&DialogAction::RecreateIdentity, false), None);
        // The other dialogs are not this one's business.
        assert_eq!(
            dialog_request(&DialogAction::PairDevice(PairingId(1)), true),
            None
        );
        assert_eq!(dialog_request(&DialogAction::DeleteProfile(0), true), None);
    }

    // ---- the button and the host

    #[test]
    fn the_qr_button_is_enabled_only_while_connected() {
        assert!(pairing_available(RemoteStatus::Connected));
        for status in [
            RemoteStatus::Off,
            RemoteStatus::Connecting,
            RemoteStatus::Failed(porecatu_remote::RelayFailure::Unreachable),
        ] {
            assert!(!pairing_available(status), "{status:?}");
        }
    }

    #[test]
    fn a_host_is_present_in_every_state_but_off() {
        assert!(!host_present(RemoteStatus::Off));
        assert!(host_present(RemoteStatus::Connecting));
        assert!(host_present(RemoteStatus::Connected));
        assert!(host_present(RemoteStatus::Failed(
            porecatu_remote::RelayFailure::TokenMissing
        )));
    }

    #[test]
    fn showing_the_qr_asks_the_host_only_when_connected() {
        let mut panel = PairingPanel::new();
        assert!(!panel.request_show(RemoteStatus::Connecting));
        assert!(panel.take_requests().is_empty());
        assert!(panel.request_show(RemoteStatus::Connected));
        assert_eq!(panel.take_requests(), vec![RemoteRequest::ShowPairing]);
        assert!(panel.take_requests().is_empty());
    }

    // ---- the countdown

    #[test]
    fn the_countdown_reads_minutes_and_two_digit_seconds() {
        assert_eq!(countdown_text(300), "5:00");
        assert_eq!(countdown_text(272), "4:32");
        assert_eq!(countdown_text(61), "1:01");
        assert_eq!(countdown_text(9), "0:09");
        assert_eq!(countdown_text(0), "0:00");
    }

    #[test]
    fn the_countdown_rounds_up_and_ends_with_the_validity() {
        let t0 = Instant::now();
        let end = t0 + secs(300);
        assert_eq!(countdown_secs(end, t0), 300);
        assert_eq!(countdown_secs(end, t0 + Duration::from_millis(100)), 300);
        assert_eq!(countdown_secs(end, t0 + secs(299)), 1);
        assert_eq!(countdown_secs(end, t0 + Duration::from_millis(299_500)), 1);
        assert_eq!(countdown_secs(end, end), 0);
        assert_eq!(countdown_secs(end, end + secs(5)), 0);
    }

    #[test]
    fn the_next_change_of_the_text_is_at_most_a_second_away() {
        let t0 = Instant::now();
        let end = t0 + secs(300);
        // Showing 300 s: it becomes 299 s when 299 s remain.
        assert_eq!(next_countdown_change(end, 300), Some(t0 + secs(1)));
        let now = t0 + Duration::from_millis(400);
        let at = next_countdown_change(end, countdown_secs(end, now)).unwrap();
        assert!(at - now <= secs(1));
        // The last second ends with the validity itself.
        assert_eq!(next_countdown_change(end, 1), Some(end));
        assert_eq!(next_countdown_change(end, 0), None);
    }

    #[test]
    fn an_expired_or_unreadable_uri_shows_no_qr() {
        let t0 = Instant::now();
        let mut panel = PairingPanel::new();
        assert!(!panel.show_qr(URI.to_owned(), t0, t0));
        assert!(panel.qr().is_none());
        assert!(!panel.show_qr("x".repeat(5000), t0 + secs(300), t0));
        assert!(panel.qr().is_none());
        assert!(panel.show_qr(URI.to_owned(), t0 + secs(300), t0));
        assert_eq!(panel.qr().unwrap().shown_secs(), 300);
        assert_eq!(panel.qr().unwrap().uri(), URI);
    }

    #[test]
    fn a_ticking_qr_updates_its_countdown_and_leaves_when_it_expires() {
        let t0 = Instant::now();
        let mut panel = PairingPanel::new();
        panel.show_qr(URI.to_owned(), t0 + secs(300), t0);
        assert_eq!(panel.next_wake(), Some(t0 + secs(1)));

        let tick = panel.tick(t0 + Duration::from_millis(500));
        assert!(!tick.redraw);
        let tick = panel.tick(t0 + secs(2));
        assert!(tick.redraw);
        assert_eq!(panel.qr().unwrap().shown_secs(), 298);

        let tick = panel.tick(t0 + secs(300));
        assert!(tick.redraw);
        assert!(panel.qr().is_none());
        // It expired by itself: nothing to cancel on the host.
        assert!(panel.take_requests().is_empty());
        assert_eq!(panel.next_wake(), None);
    }

    #[test]
    fn without_a_qr_or_a_prompt_there_is_no_deadline() {
        assert_eq!(PairingPanel::new().next_wake(), None);
    }

    // ---- invalidating the qr

    #[test]
    fn leaving_the_group_or_closing_the_screen_cancels_the_token() {
        let t0 = Instant::now();
        let mut panel = PairingPanel::new();
        panel.show_qr(URI.to_owned(), t0 + secs(300), t0);
        assert!(panel.leave_group());
        assert!(panel.qr().is_none());
        assert_eq!(panel.take_requests(), vec![RemoteRequest::CancelPairing]);

        // Nothing to cancel without a QR.
        assert!(!panel.leave_group());
        assert!(panel.take_requests().is_empty());

        panel.show_qr(URI.to_owned(), t0 + secs(300), t0);
        panel.close();
        assert!(panel.qr().is_none());
        assert_eq!(panel.take_requests(), vec![RemoteRequest::CancelPairing]);
    }

    #[test]
    fn showing_again_replaces_the_qr() {
        let t0 = Instant::now();
        let mut panel = PairingPanel::new();
        panel.show_qr(URI.to_owned(), t0 + secs(300), t0);
        let other = URI.replace("Desk", "Other");
        panel.show_qr(other.clone(), t0 + secs(290), t0 + secs(10));
        assert_eq!(panel.qr().unwrap().uri(), other);
    }

    #[test]
    fn the_uri_never_shows_in_debug() {
        let t0 = Instant::now();
        let mut panel = PairingPanel::new();
        panel.show_qr(URI.to_owned(), t0 + secs(300), t0);
        let text = format!("{panel:?}");
        assert!(!text.contains("token"), "{text}");
        assert!(!text.contains("porecatu://"), "{text}");
    }

    // ---- the pairing prompt

    #[test]
    fn a_prompt_is_answered_by_accept_or_decline() {
        let t0 = Instant::now();
        let mut accepted = PairingPanel::new();
        accepted.prompt_arrived(pending(7), t0);
        assert_eq!(accepted.prompt().unwrap().id, PairingId(7));
        accepted.answer_prompt(true);
        assert!(accepted.prompt().is_none());
        assert_eq!(
            accepted.take_requests(),
            vec![RemoteRequest::AnswerPairing {
                id: PairingId(7),
                answer: PairingAnswer::Accept
            }]
        );

        let mut declined = PairingPanel::new();
        declined.prompt_arrived(pending(8), t0);
        declined.answer_prompt(false);
        assert_eq!(
            declined.take_requests(),
            vec![RemoteRequest::AnswerPairing {
                id: PairingId(8),
                answer: PairingAnswer::Decline
            }]
        );
    }

    #[test]
    fn a_prompt_without_an_answer_times_out_after_120_seconds() {
        let t0 = Instant::now();
        let mut panel = PairingPanel::new();
        panel.prompt_arrived(pending(3), t0);
        assert_eq!(panel.next_wake(), Some(t0 + secs(120)));

        let tick = panel.tick(t0 + secs(119));
        assert_eq!(tick, Tick::default());
        assert!(panel.prompt().is_some());

        let tick = panel.tick(t0 + secs(120));
        assert!(tick.prompt_expired);
        assert!(panel.prompt().is_none());
        assert_eq!(
            panel.take_requests(),
            vec![RemoteRequest::AnswerPairing {
                id: PairingId(3),
                answer: PairingAnswer::Timeout
            }]
        );
        assert_eq!(panel.next_wake(), None);
    }

    #[test]
    fn a_closed_channel_drops_the_prompt_without_an_answer() {
        let t0 = Instant::now();
        let mut panel = PairingPanel::new();
        panel.prompt_arrived(pending(5), t0);
        // Another request closing does not touch this one.
        assert!(!panel.prompt_closed(PairingId(4)));
        assert!(panel.prompt().is_some());
        assert!(panel.prompt_closed(PairingId(5)));
        assert!(panel.prompt().is_none());
        assert!(panel.take_requests().is_empty());
        // The deadline died with it.
        assert!(!panel.tick(t0 + secs(500)).prompt_expired);
    }

    #[test]
    fn the_token_is_consumed_when_the_request_arrives_so_the_qr_leaves() {
        let t0 = Instant::now();
        let mut panel = PairingPanel::new();
        panel.show_qr(URI.to_owned(), t0 + secs(300), t0);
        panel.prompt_arrived(pending(1), t0 + secs(20));
        assert!(panel.qr().is_none());
        // The host already dropped the token: nothing to cancel.
        assert!(panel.take_requests().is_empty());
    }

    #[test]
    fn a_second_request_declines_the_first_there_is_no_queue() {
        let t0 = Instant::now();
        let mut panel = PairingPanel::new();
        panel.prompt_arrived(pending(1), t0);
        panel.prompt_arrived(pending(2), t0);
        assert_eq!(panel.prompt().unwrap().id, PairingId(2));
        assert_eq!(
            panel.take_requests(),
            vec![RemoteRequest::AnswerPairing {
                id: PairingId(1),
                answer: PairingAnswer::Decline
            }]
        );
    }

    #[test]
    fn closing_the_screen_declines_a_pending_request() {
        let t0 = Instant::now();
        let mut panel = PairingPanel::new();
        panel.prompt_arrived(pending(9), t0);
        panel.close();
        assert_eq!(
            panel.take_requests(),
            vec![RemoteRequest::AnswerPairing {
                id: PairingId(9),
                answer: PairingAnswer::Decline
            }]
        );
    }

    // ---- the devices

    #[test]
    fn a_device_is_connected_now_when_a_session_has_its_key() {
        let a = [1_u8; 32];
        let b = [2_u8; 32];
        assert!(is_connected_now(&a, &[b, a]));
        assert!(!is_connected_now(&a, &[b]));
        assert!(!is_connected_now(&a, &[]));
    }

    #[test]
    fn the_rows_keep_the_hosts_order_and_mark_the_connected_ones() {
        let info = |key: u8, name: &str| DeviceInfo {
            key: [key; 32],
            name: name.to_owned(),
            platform: DevicePlatform::Ios,
            paired_at: "2026-11-15T14:03:00Z".to_owned(),
            last_seen: None,
        };
        let rows = device_rows(&[info(1, "a"), info(2, "b")], &[[2; 32]]);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].name.as_str(), rows[0].connected), ("a", false));
        assert_eq!((rows[1].name.as_str(), rows[1].connected), ("b", true));
    }

    #[test]
    fn setting_the_same_list_changes_nothing() {
        let mut panel = PairingPanel::new();
        assert!(panel.set_devices(vec![row(false, None)]));
        assert!(!panel.set_devices(vec![row(false, None)]));
        assert!(panel.set_devices(vec![row(true, None)]));
        assert!(panel.set_devices(Vec::new()));
    }

    #[test]
    fn the_short_date_follows_the_time_zone_and_refuses_what_is_not_a_date() {
        let utc = FixedOffset::east_opt(0).unwrap();
        let west = FixedOffset::west_opt(3 * 3600).unwrap();
        assert_eq!(
            short_date("2026-11-15T14:03:00Z", &utc).as_deref(),
            Some("2026-11-15")
        );
        // 01:00 UTC is still the day before three hours to the west.
        assert_eq!(
            short_date("2026-11-15T01:00:00Z", &west).as_deref(),
            Some("2026-11-14")
        );
        assert_eq!(short_date("yesterday", &utc), None);
        assert_eq!(short_date("", &utc), None);
    }

    #[test]
    fn the_detail_line_has_platform_paired_and_last_access() {
        let utc = FixedOffset::east_opt(0).unwrap();
        let pt = test_support::pt_br();
        assert_eq!(
            device_detail_in(&pt, &row(false, Some("2026-11-16T09:30:00Z")), &utc),
            "Android · pareado em 2026-11-15 · último acesso 2026-11-16"
        );
        let en = test_support::en_us();
        assert_eq!(
            device_detail_in(&en, &row(false, Some("2026-11-16T09:30:00Z")), &utc),
            "Android · paired on 2026-11-15 · last access 2026-11-16"
        );
    }

    #[test]
    fn a_device_never_seen_says_never_in_every_language() {
        let utc = FixedOffset::east_opt(0).unwrap();
        let pt = test_support::pt_br();
        for last_seen in [None, Some("")] {
            assert_eq!(
                device_detail_in(&pt, &row(false, last_seen), &utc),
                "Android · pareado em 2026-11-15 · último acesso nunca"
            );
        }
        for catalog in test_support::all_catalogs() {
            let text = device_detail_in(&catalog, &row(false, None), &utc);
            assert!(!text.is_empty());
            assert!(!text.contains('{'), "{text}");
        }
    }

    #[test]
    fn an_unparsable_date_is_shown_as_it_came() {
        let utc = FixedOffset::east_opt(0).unwrap();
        let mut odd = row(false, Some("a long time ago"));
        odd.paired_at = "sometime".to_owned();
        let text = device_detail_in(&test_support::en_us(), &odd, &utc);
        assert!(text.contains("sometime") && text.contains("a long time ago"));
    }

    #[test]
    fn the_platforms_have_a_name_in_every_language() {
        for catalog in test_support::all_catalogs() {
            for platform in [
                DevicePlatform::Android,
                DevicePlatform::Ios,
                DevicePlatform::Other,
            ] {
                assert!(!platform_text(&catalog, platform).is_empty());
            }
        }
    }
}
