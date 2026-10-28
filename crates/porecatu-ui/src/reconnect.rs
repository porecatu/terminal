// SPDX-License-Identifier: GPL-3.0-or-later

//! Reconexão automática de uma conexão SSH que caiu (ADR-0076, RF-20.27 a
//! RF-20.31).
//!
//! Estado puro: o relógio entra como `Instant` recebido de fora, e quem dorme é
//! o `ControlFlow::WaitUntil` do `App`, nunca uma thread. Aqui só se decide
//! **quando** tentar de novo e **se** vale tentar; quem faz o `reattach` e
//! escreve as notas é `lib.rs`.

use std::time::{Duration, Instant};

use porecatu_term::{EndReason, FailCause};

/// Quantas tentativas há por ciclo (decisão do dono, RF-20.27).
pub(crate) const ATTEMPTS: u8 = 5;

/// A espera **antes** das tentativas 1 a 5: exponencial de base 2, sem *jitter*
/// (ADR-0076 §2) -- 31 s no total, fora o tempo das próprias tentativas.
pub(crate) const ATTEMPT_DELAYS: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
];

/// Em que ponto está o ciclo de uma conexão.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reconnect {
    /// Esperando a tentativa `attempt` (de 1 a 5), que vence em `due`.
    Waiting { attempt: u8, due: Instant },
    /// A tentativa `attempt` está em andamento: um `reattach` esperando o
    /// resultado.
    Connecting { attempt: u8 },
    /// Esgotou as tentativas, ou parou por uma causa que não adianta repetir.
    GaveUp,
    /// A tentativa deu certo. Fora do ciclo: a próxima queda recomeça em 1 de
    /// 5.
    Connected,
}

/// O que uma falha de tentativa pede (ADR-0076 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Vale tentar de novo: é o servidor reiniciando ou a rede voltando.
    Retry,
    /// Tentar de novo daria a mesma resposta, ou o usuário disse não.
    Stop,
}

/// A tabela do ADR-0076 §2. `match` **exaustivo**, sem `_`: uma causa nova não
/// compila sem uma decisão aqui.
fn verdict(cause: &FailCause) -> Verdict {
    match cause {
        FailCause::Refused | FailCause::Timeout | FailCause::ChannelRefused => Verdict::Retry,
        // RF-20.30: cancelar interrompe as tentativas.
        FailCause::AuthCancelled | FailCause::HostKeyRejectedByUser => Verdict::Stop,
        // RF-20.30: chave do host que mudou bloqueia, sem mais tentativas.
        FailCause::HostKeyChanged { .. }
        | FailCause::HostKeyRevoked { .. }
        | FailCause::KnownHostsUnreadable { .. } => Verdict::Stop,
        FailCause::AuthRejected | FailCause::AuthUnsupported | FailCause::KeyUnreadable => {
            Verdict::Stop
        }
        FailCause::Runtime => Verdict::Stop,
    }
}

/// Esta causa é de uma tentativa que se repetiria? (Para a nota de "desisti
/// depois de N tentativas": uma causa que para na hora não tem N.)
pub(crate) fn will_retry(cause: &FailCause) -> bool {
    verdict(cause) == Verdict::Retry
}

/// Esta razão de fim **inicia** um ciclo? Só uma conexão que caiu sozinha
/// (ADR-0076 §1): `Failed` fora de um ciclo é uma aba que nunca conectou, e
/// não há sessão a recuperar; `RemoteEnded` é o servidor encerrando de
/// propósito (RF-20.26).
pub(crate) fn starts_a_cycle(reason: &EndReason) -> bool {
    match reason {
        EndReason::Lost { .. } => true,
        EndReason::Exited { .. } | EndReason::RemoteEnded { .. } | EndReason::Failed { .. } => {
            false
        }
    }
}

impl Reconnect {
    /// A conexão acabou de cair: a tentativa 1 vence em 1 s.
    pub(crate) fn on_lost(now: Instant) -> Self {
        Self::Waiting {
            attempt: 1,
            due: now + ATTEMPT_DELAYS[0],
        }
    }

    /// Quando acordar, se há algo esperando. Só `Waiting` tem hora.
    pub(crate) fn next_wakeup(&self) -> Option<Instant> {
        match self {
            Self::Waiting { due, .. } => Some(*due),
            Self::Connecting { .. } | Self::GaveUp | Self::Connected => None,
        }
    }

    /// `Some(n)` quando é hora de tentar: o ciclo passa a `Connecting`.
    pub(crate) fn poll(&mut self, now: Instant) -> Option<u8> {
        match *self {
            Self::Waiting { attempt, due } if now >= due => {
                *self = Self::Connecting { attempt };
                Some(attempt)
            }
            Self::Waiting { .. } | Self::Connecting { .. } | Self::GaveUp | Self::Connected => None,
        }
    }

    /// A tentativa em andamento falhou antes de abrir, por `cause`.
    pub(crate) fn on_attempt_failed(&mut self, cause: &FailCause, now: Instant) {
        match verdict(cause) {
            Verdict::Retry => self.retry_or_give_up(now),
            Verdict::Stop => {
                if matches!(self, Self::Connecting { .. }) {
                    *self = Self::GaveUp;
                }
            }
        }
    }

    /// A conexão da tentativa caiu logo depois de abrir, ainda dentro do ciclo:
    /// vale como `Refused` (ADR-0076 §2, "`Lost` durante a tentativa").
    pub(crate) fn on_attempt_lost(&mut self, now: Instant) {
        self.retry_or_give_up(now);
    }

    fn retry_or_give_up(&mut self, now: Instant) {
        if let Self::Connecting { attempt } = *self {
            *self = if attempt >= ATTEMPTS {
                Self::GaveUp
            } else {
                Self::Waiting {
                    attempt: attempt + 1,
                    due: now + ATTEMPT_DELAYS[usize::from(attempt)],
                }
            };
        }
    }

    /// A tentativa abriu o shell: sai do ciclo.
    pub(crate) fn on_connected(&mut self) {
        *self = Self::Connected;
    }

    /// `ssh.reconnect` (ADR-0076 §5): em `Waiting`, antecipa a tentativa; em
    /// `GaveUp`, recomeça a contagem agora. Em `Connecting`, não há o que
    /// fazer.
    pub(crate) fn reconnect_now(&mut self, now: Instant) {
        match *self {
            Self::Waiting { attempt, .. } => *self = Self::Waiting { attempt, due: now },
            Self::GaveUp => {
                *self = Self::Waiting {
                    attempt: 1,
                    due: now,
                }
            }
            Self::Connecting { .. } | Self::Connected => {}
        }
    }

    /// Ainda há ciclo: esperando ou tentando.
    #[cfg(test)]
    pub(crate) fn is_active(&self) -> bool {
        matches!(self, Self::Waiting { .. } | Self::Connecting { .. })
    }

    /// A tentativa (1 a 5) a que a nota se refere, quando há uma.
    #[cfg(test)]
    pub(crate) fn attempt(&self) -> Option<u8> {
        match self {
            Self::Waiting { attempt, .. } | Self::Connecting { attempt } => Some(*attempt),
            Self::GaveUp | Self::Connected => None,
        }
    }
}

/// O que mudou quando um painel saiu do ciclo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Departure<K> {
    /// Não era o que tenta; nada muda.
    Follower,
    /// Era o que tentava (ou esperava para tentar): `K` assume.
    NewOwner(K),
    /// Era o último: o ciclo acaba, e a entrada do mapa sai com ele.
    Last,
    /// Não estava no ciclo.
    Unknown,
}

/// O ciclo de **uma conexão**: a máquina de estados e os painéis que a
/// dividiam (ADR-0076 §3). O mais antigo -- o primeiro da ordem de `K` --
/// tenta; os demais seguem quando a conexão nova abre.
#[derive(Debug, Clone)]
pub(crate) struct Cycle<K> {
    pub(crate) state: Reconnect,
    members: Vec<K>,
    /// A nota "o processo remoto não foi retomado" (RF-20.29) já saiu neste
    /// ciclo? É uma vez só, na primeira volta.
    resumed_note_written: bool,
}

impl<K: Ord + Copy> Cycle<K> {
    pub(crate) fn new(now: Instant, first: K) -> Self {
        Self {
            state: Reconnect::on_lost(now),
            members: vec![first],
            resumed_note_written: false,
        }
    }

    /// Mais um painel da conexão recebeu a queda. Já estar no ciclo não repete.
    pub(crate) fn add(&mut self, member: K) {
        if let Err(at) = self.members.binary_search(&member) {
            self.members.insert(at, member);
        }
    }

    /// O painel que tenta: o mais antigo.
    pub(crate) fn owner(&self) -> Option<K> {
        self.members.first().copied()
    }

    /// Os que esperam a conexão nova abrir para entrar nela.
    pub(crate) fn followers(&self) -> impl Iterator<Item = K> + '_ {
        self.members.iter().skip(1).copied()
    }

    pub(crate) fn members(&self) -> &[K] {
        &self.members
    }

    pub(crate) fn contains(&self, member: K) -> bool {
        self.members.binary_search(&member).is_ok()
    }

    /// Um painel fechou (ou saiu do ciclo). O próximo assume o lugar do que
    /// tentava.
    pub(crate) fn remove(&mut self, member: K) -> Departure<K> {
        let Ok(at) = self.members.binary_search(&member) else {
            return Departure::Unknown;
        };
        self.members.remove(at);
        match (at, self.members.first().copied()) {
            (_, None) => Departure::Last,
            (0, Some(next)) => Departure::NewOwner(next),
            _ => Departure::Follower,
        }
    }

    /// A nota de "processo não retomado" vale uma vez por ciclo: a primeira
    /// chamada diz `true`.
    pub(crate) fn take_resumed_note(&mut self) -> bool {
        !std::mem::replace(&mut self.resumed_note_written, true)
    }
}

#[cfg(test)]
mod tests {
    use porecatu_term::{HostKeyEntry, HostKeyPresented, KnownHostsCause, LostCause};

    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    /// Um relógio construído: `t0` mais o que o teste quiser, sem dormir.
    fn t0() -> Instant {
        Instant::now()
    }

    fn every_cause() -> Vec<FailCause> {
        let presented = HostKeyPresented {
            algorithm: "ssh-ed25519".to_owned(),
            fingerprint: "SHA256:x".to_owned(),
        };
        let entry = HostKeyEntry {
            file: "known_hosts".into(),
            line: 1,
            fingerprint: "SHA256:y".to_owned(),
        };
        vec![
            FailCause::Refused,
            FailCause::Timeout,
            FailCause::AuthRejected,
            FailCause::AuthCancelled,
            FailCause::AuthUnsupported,
            FailCause::KeyUnreadable,
            FailCause::HostKeyRejectedByUser,
            FailCause::HostKeyChanged {
                presented: presented.clone(),
                expected: vec![entry.clone()],
            },
            FailCause::HostKeyRevoked { presented, entry },
            FailCause::KnownHostsUnreadable {
                file: "known_hosts".into(),
                cause: KnownHostsCause::NotText,
            },
            FailCause::ChannelRefused,
            FailCause::Runtime,
        ]
    }

    #[test]
    fn the_delays_are_one_two_four_eight_sixteen_and_add_up_to_thirty_one() {
        assert_eq!(
            ATTEMPT_DELAYS,
            [secs(1), secs(2), secs(4), secs(8), secs(16)]
        );
        assert_eq!(ATTEMPT_DELAYS.iter().sum::<Duration>(), secs(31));
    }

    #[test]
    fn a_drop_waits_one_second_for_the_first_attempt() {
        let now = t0();
        let state = Reconnect::on_lost(now);
        assert_eq!(
            state,
            Reconnect::Waiting {
                attempt: 1,
                due: now + secs(1)
            }
        );
        assert_eq!(state.next_wakeup(), Some(now + secs(1)));
    }

    #[test]
    fn poll_is_silent_until_the_due_time_and_then_starts_the_attempt_once() {
        let now = t0();
        let mut state = Reconnect::on_lost(now);

        assert_eq!(state.poll(now + Duration::from_millis(999)), None);
        assert_eq!(state.poll(now + secs(1)), Some(1));
        assert_eq!(state, Reconnect::Connecting { attempt: 1 });
        // Nothing starts twice, and a connecting cycle has no wake-up.
        assert_eq!(state.poll(now + secs(60)), None);
        assert_eq!(state.next_wakeup(), None);
    }

    #[test]
    fn five_failed_attempts_wait_one_two_four_eight_sixteen_and_then_give_up() {
        let mut now = t0();
        let mut state = Reconnect::on_lost(now);
        let mut waited = Vec::new();

        for attempt in 1..=ATTEMPTS {
            let due = state.next_wakeup().expect("waiting");
            waited.push(due - now);
            now = due;
            assert_eq!(state.poll(now), Some(attempt));
            state.on_attempt_failed(&FailCause::Refused, now);
        }

        assert_eq!(waited, [secs(1), secs(2), secs(4), secs(8), secs(16)]);
        assert_eq!(state, Reconnect::GaveUp);
        assert_eq!(state.next_wakeup(), None);
        assert_eq!(state.poll(now + secs(3600)), None);
        assert!(!state.is_active());
    }

    #[test]
    fn the_attempt_number_is_one_to_five() {
        let mut now = t0();
        let mut state = Reconnect::on_lost(now);
        let mut seen = Vec::new();
        while let Some(due) = state.next_wakeup() {
            now = due;
            state.poll(now);
            seen.push(state.attempt().unwrap());
            state.on_attempt_failed(&FailCause::Timeout, now);
        }
        assert_eq!(seen, [1, 2, 3, 4, 5]);
        assert_eq!(state.attempt(), None);
    }

    #[test]
    fn every_cause_has_a_decision_and_the_ones_that_stop_stop_at_once() {
        let retry = [
            FailCause::Refused,
            FailCause::Timeout,
            FailCause::ChannelRefused,
        ];
        for cause in every_cause() {
            let now = t0();
            let mut state = Reconnect::Connecting { attempt: 2 };
            state.on_attempt_failed(&cause, now);
            if retry.contains(&cause) {
                assert_eq!(
                    state,
                    Reconnect::Waiting {
                        attempt: 3,
                        due: now + secs(4)
                    },
                    "{cause:?} retries"
                );
            } else {
                assert_eq!(state, Reconnect::GaveUp, "{cause:?} stops");
            }
        }
    }

    #[test]
    fn a_cancelled_password_and_a_changed_host_key_stop_even_on_the_first_attempt() {
        for cause in [
            FailCause::AuthCancelled,
            FailCause::HostKeyRejectedByUser,
            FailCause::AuthRejected,
        ] {
            let mut state = Reconnect::Connecting { attempt: 1 };
            state.on_attempt_failed(&cause, t0());
            assert_eq!(state, Reconnect::GaveUp, "{cause:?}");
        }
    }

    #[test]
    fn a_connection_lost_during_an_attempt_counts_like_a_refusal() {
        let now = t0();
        let mut state = Reconnect::Connecting { attempt: 4 };
        state.on_attempt_lost(now);
        assert_eq!(
            state,
            Reconnect::Waiting {
                attempt: 5,
                due: now + secs(16)
            }
        );
        state.poll(now + secs(16));
        state.on_attempt_lost(now + secs(16));
        assert_eq!(state, Reconnect::GaveUp);
    }

    #[test]
    fn a_failure_outside_an_attempt_changes_nothing() {
        let now = t0();
        let mut waiting = Reconnect::on_lost(now);
        waiting.on_attempt_failed(&FailCause::AuthCancelled, now);
        assert_eq!(waiting, Reconnect::on_lost(now));
        waiting.on_attempt_lost(now);
        assert_eq!(waiting, Reconnect::on_lost(now));
    }

    #[test]
    fn connecting_zeroes_the_cycle_and_the_next_drop_starts_at_one_of_five() {
        let now = t0();
        let mut state = Reconnect::Connecting { attempt: 4 };
        state.on_connected();
        assert_eq!(state, Reconnect::Connected);
        assert!(!state.is_active());
        assert_eq!(state.next_wakeup(), None);

        let later = now + secs(500);
        assert_eq!(
            Reconnect::on_lost(later),
            Reconnect::Waiting {
                attempt: 1,
                due: later + secs(1)
            }
        );
    }

    #[test]
    fn reconnect_now_brings_a_waiting_attempt_forward_and_restarts_a_gave_up_cycle() {
        let now = t0();
        let mut waiting = Reconnect::Waiting {
            attempt: 3,
            due: now + secs(4),
        };
        waiting.reconnect_now(now + secs(1));
        assert_eq!(
            waiting,
            Reconnect::Waiting {
                attempt: 3,
                due: now + secs(1)
            },
            "the same attempt, now"
        );
        assert_eq!(waiting.poll(now + secs(1)), Some(3));

        let mut gave_up = Reconnect::GaveUp;
        gave_up.reconnect_now(now);
        assert_eq!(
            gave_up,
            Reconnect::Waiting {
                attempt: 1,
                due: now
            },
            "the count starts over"
        );

        let mut connecting = Reconnect::Connecting { attempt: 2 };
        connecting.reconnect_now(now);
        assert_eq!(connecting, Reconnect::Connecting { attempt: 2 });
    }

    #[test]
    fn only_a_connection_that_fell_by_itself_starts_a_cycle() {
        assert!(starts_a_cycle(&EndReason::Lost {
            cause: LostCause::KeepaliveTimeout
        }));
        assert!(starts_a_cycle(&EndReason::Lost {
            cause: LostCause::ServerDisconnect
        }));
        // A tab that never connected has no session to recover.
        assert!(!starts_a_cycle(&EndReason::Failed {
            cause: FailCause::Refused
        }));
        // The server ended the session on purpose (RF-20.26).
        assert!(!starts_a_cycle(&EndReason::RemoteEnded {
            code: Some(0),
            signal: None
        }));
        assert!(!starts_a_cycle(&EndReason::Exited {
            success: true,
            code: 0
        }));
    }

    // ---- o ciclo de uma conexão ----

    #[test]
    fn the_oldest_pane_tries_and_the_others_follow() {
        let mut cycle = Cycle::new(t0(), 7u32);
        cycle.add(3);
        cycle.add(9);
        cycle.add(3);

        assert_eq!(cycle.members(), [3, 7, 9], "kept in order, no repeats");
        assert_eq!(cycle.owner(), Some(3));
        assert_eq!(cycle.followers().collect::<Vec<_>>(), [7, 9]);
        assert!(cycle.contains(9));
        assert!(!cycle.contains(4));
    }

    #[test]
    fn closing_the_pane_that_tries_hands_the_attempt_to_the_next_one() {
        let mut cycle = Cycle::new(t0(), 3u32);
        cycle.add(7);
        cycle.add(9);

        assert_eq!(cycle.remove(3), Departure::NewOwner(7));
        assert_eq!(cycle.owner(), Some(7));
        assert_eq!(cycle.remove(9), Departure::Follower);
        assert_eq!(cycle.remove(7), Departure::Last);
        assert_eq!(cycle.owner(), None);
        assert_eq!(cycle.remove(7), Departure::Unknown);
    }

    #[test]
    fn closing_a_follower_changes_nobody_else() {
        let mut cycle = Cycle::new(t0(), 3u32);
        cycle.add(7);
        assert_eq!(cycle.remove(7), Departure::Follower);
        assert_eq!(cycle.owner(), Some(3));
    }

    #[test]
    fn giving_up_is_one_state_for_every_pane_of_the_connection() {
        let mut cycle = Cycle::new(t0(), 1u32);
        cycle.add(2);
        cycle.add(3);
        cycle.state = Reconnect::Connecting { attempt: 5 };

        cycle.state.on_attempt_failed(&FailCause::Refused, t0());

        // One state, read by all three.
        assert_eq!(cycle.state, Reconnect::GaveUp);
        assert_eq!(cycle.members().len(), 3);
    }

    #[test]
    fn the_not_resumed_note_is_written_once_per_cycle() {
        let mut cycle = Cycle::new(t0(), 1u32);
        assert!(cycle.take_resumed_note());
        assert!(!cycle.take_resumed_note());
    }
}
