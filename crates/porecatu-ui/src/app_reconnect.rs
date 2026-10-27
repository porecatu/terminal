// SPDX-License-Identifier: GPL-3.0-or-later

//! A reconexão automática no `App` (ADR-0076 §3 a §5): o que a máquina pura de
//! `reconnect.rs` não faz -- achar os painéis de uma conexão, fazer o
//! `reattach`, escrever as notas e juntar o próximo acordar ao `WaitUntil`.
//!
//! O ciclo é guardado **por conexão** (a que caiu), num mapa do `App`: o mesmo
//! desenho do `git_remotes`. Nenhuma thread, nenhum `sleep`: quem acorda é o
//! `ControlFlow::WaitUntil` de `schedule_next_wake`.

use porecatu_term::{Connection, ConnectionId};

use super::*;
use crate::reconnect::{ATTEMPTS, Cycle, Departure, Reconnect, starts_a_cycle, will_retry};

/// Um painel de uma janela: a chave de um ciclo e de um `reattach`.
pub(crate) type PaneKey = (WindowId, TabId, PaneId);

/// O que `lib.rs` junta de uma volta do laço de eventos de um painel SSH e
/// entrega a `App::handle_ssh_events`.
pub(crate) enum SshEvent {
    /// O backend do painel terminou.
    Ended(EndReason),
    /// O backend abriu o shell.
    Connected,
}

/// O mapa de ciclos do `App`.
pub(crate) type Cycles = HashMap<ConnectionId, Cycle<PaneKey>>;

/// A razão de fim de um painel SSH é de um ciclo de reconexão? Se for,
/// `lib.rs` não segue o caminho de sempre (nota, painel `Exited`): quem decide
/// o destino do painel é o ciclo. Função livre, e não método, para o laço de
/// eventos poder chamá-la com a janela emprestada.
pub(crate) fn end_belongs_to_a_cycle(
    runtime: Option<&PaneRuntime>,
    cycles: &Cycles,
    key: PaneKey,
    reason: &EndReason,
) -> bool {
    let is_ssh = runtime.is_some_and(|runtime| runtime.ssh_spawn.is_some());
    is_ssh && (starts_a_cycle(reason) || cycles.values().any(|cycle| cycle.contains(key)))
}

impl App {
    fn runtime(&self, key: PaneKey) -> Option<&PaneRuntime> {
        self.windows.get(&key.0)?.panes.get(&(key.1, key.2))
    }

    fn runtime_mut(&mut self, key: PaneKey) -> Option<&mut PaneRuntime> {
        self.windows.get_mut(&key.0)?.panes.get_mut(&(key.1, key.2))
    }

    /// A conexão (a que caiu) cujo ciclo tem `key`.
    fn cycle_of(&self, key: PaneKey) -> Option<ConnectionId> {
        self.reconnects
            .iter()
            .find(|(_, cycle)| cycle.contains(key))
            .map(|(id, _)| *id)
    }

    fn write_note(&self, key: PaneKey, text: &str) {
        if let Some(runtime) = self.runtime(key) {
            runtime.terminal.inject_note(text, palette::NOTE_ACCENT_RGB);
        }
    }

    /// A nota do que o ciclo espera agora, para um painel.
    fn write_state_note(&self, key: PaneKey, state: Reconnect, now: Instant) {
        let Some(host) = self.runtime(key).and_then(|rt| rt.ssh_host.clone()) else {
            return;
        };
        match state {
            Reconnect::Waiting { attempt, due } => {
                // Uma frase, sem animação: a contagem não se reescreve.
                let seconds = due.saturating_duration_since(now).as_secs_f64().ceil() as u64;
                self.write_note(
                    key,
                    &messages::ssh_reconnect_waiting_note(
                        &self.catalog,
                        attempt,
                        ATTEMPTS,
                        seconds,
                    ),
                );
            }
            Reconnect::Connecting { attempt } => {
                self.write_note(
                    key,
                    &messages::ssh_reconnect_connecting_note(
                        &self.catalog,
                        attempt,
                        ATTEMPTS,
                        &host,
                    ),
                );
            }
            Reconnect::GaveUp | Reconnect::Connected => {}
        }
    }

    /// Entrega os eventos SSH de um painel (ADR-0076 §1, §3).
    pub(super) fn handle_ssh_events(&mut self, key: PaneKey, events: Vec<SshEvent>, now: Instant) {
        for event in events {
            match event {
                SshEvent::Ended(reason) => self.on_ssh_ended(key, reason, now),
                SshEvent::Connected => self.on_ssh_connected(key, now),
            }
        }
        // Uma tentativa pode ter ficado para agora (`ssh.reconnect`, ou o
        // próximo painel assumindo).
        self.poll_reconnects(now);
        if let Some(state) = self.windows.get(&key.0) {
            state.window.request_redraw();
        }
    }

    fn on_ssh_ended(&mut self, key: PaneKey, reason: EndReason, now: Instant) {
        let host = self
            .runtime(key)
            .and_then(|runtime| runtime.ssh_host.clone())
            .unwrap_or_default();
        let end_note = messages::ssh_end_note(&self.catalog, &host, &reason);
        let in_cycle = self.cycle_of(key);

        match (&reason, in_cycle) {
            // A conexão caiu sozinha: entra no ciclo da conexão dela, ou o
            // abre (os demais painéis dela chegam pelos próprios eventos).
            (EndReason::Lost { .. }, None) => {
                let Some(connection) = self
                    .runtime(key)
                    .and_then(|runtime| runtime.terminal.connection_id())
                else {
                    return;
                };
                if let Some(note) = &end_note {
                    self.write_note(key, note);
                }
                let cycle = self
                    .reconnects
                    .entry(connection)
                    .or_insert_with(|| Cycle::new(now, key));
                cycle.add(key);
                let state = cycle.state;
                if let Some(runtime) = self.runtime_mut(key) {
                    runtime.reconnecting = true;
                }
                self.write_state_note(key, state, now);
            }
            // A conexão da própria tentativa caiu logo depois de abrir, ou o
            // servidor a encerrou: conta como falha da tentativa.
            (EndReason::Lost { .. } | EndReason::RemoteEnded { .. }, Some(id))
                if self.is_attempting(id, key) =>
            {
                if let Some(note) = &end_note {
                    self.write_note(key, note);
                }
                if let Some(cycle) = self.reconnects.get_mut(&id) {
                    cycle.state.on_attempt_lost(now);
                }
                self.after_attempt(id, key, &reason, now);
            }
            (EndReason::Failed { cause }, Some(id)) if self.is_attempting(id, key) => {
                if let Some(note) = &end_note {
                    self.write_note(key, note);
                }
                if let Some(cycle) = self.reconnects.get_mut(&id) {
                    cycle.state.on_attempt_failed(cause, now);
                }
                self.after_attempt(id, key, &reason, now);
            }
            // Evento atrasado de quem já não tenta.
            _ => {}
        }
    }

    /// `key` é o painel que tenta, e a tentativa está em andamento.
    fn is_attempting(&self, id: ConnectionId, key: PaneKey) -> bool {
        self.reconnects.get(&id).is_some_and(|cycle| {
            cycle.owner() == Some(key) && matches!(cycle.state, Reconnect::Connecting { .. })
        })
    }

    /// Depois do resultado de uma tentativa: avisa a espera da próxima, ou
    /// desiste, para **todos** os painéis da conexão.
    fn after_attempt(
        &mut self,
        id: ConnectionId,
        owner: PaneKey,
        reason: &EndReason,
        now: Instant,
    ) {
        let Some(cycle) = self.reconnects.get(&id) else {
            return;
        };
        let state = cycle.state;
        match state {
            Reconnect::GaveUp => self.give_up(id, owner, reason),
            Reconnect::Waiting { .. } => {
                for member in cycle.members().to_vec() {
                    self.write_state_note(member, state, now);
                }
            }
            Reconnect::Connecting { .. } | Reconnect::Connected => {}
        }
    }

    /// `GaveUp` vale para todos os painéis da conexão de uma vez (ADR-0076 §3):
    /// cada um fica `Exited` com a nota da última falha.
    fn give_up(&mut self, id: ConnectionId, owner: PaneKey, reason: &EndReason) {
        let Some(cycle) = self.reconnects.remove(&id) else {
            return;
        };
        // Esgotou as tentativas (e não parou numa causa que não se repete)?
        let exhausted = match reason {
            EndReason::Failed { cause } => will_retry(cause),
            EndReason::Lost { .. } | EndReason::RemoteEnded { .. } => true,
            EndReason::Exited { .. } => false,
        };
        for member in cycle.members().to_vec() {
            let host = self
                .runtime(member)
                .and_then(|runtime| runtime.ssh_host.clone())
                .unwrap_or_default();
            // O que tentou já escreveu a causa; os outros a ganham agora.
            if member != owner
                && let Some(note) = messages::ssh_end_note(&self.catalog, &host, reason)
            {
                self.write_note(member, &note);
            }
            if exhausted {
                self.write_note(
                    member,
                    &messages::ssh_reconnect_gave_up_note(&self.catalog, ATTEMPTS),
                );
            }
            if let Some(runtime) = self.runtime_mut(member) {
                runtime.reconnecting = false;
            }
            if let Some(state) = self.windows.get_mut(&member.0) {
                if let Some(pane) = state
                    .workspace
                    .tab_mut(member.1)
                    .and_then(|tab| tab.panes_mut().pane_mut(member.2))
                {
                    pane.mark_exited(1);
                }
                state.mark_session_dirty();
            }
        }
    }

    /// O shell da tentativa abriu: o ciclo acaba e os outros painéis da
    /// conexão entram nela, sem autenticar de novo (ADR-0076 §3).
    fn on_ssh_connected(&mut self, key: PaneKey, now: Instant) {
        // Todo painel SSH que abre manda `Connected`; só o que tentava importa.
        let Some(id) = self
            .reconnects
            .iter()
            .find(|(id, _)| self.is_attempting(**id, key))
            .map(|(id, _)| *id)
        else {
            return;
        };
        let Some(mut cycle) = self.reconnects.remove(&id) else {
            return;
        };
        cycle.state.on_connected();
        let Some(connection) = self
            .runtime(key)
            .and_then(|runtime| runtime.terminal.connection_id())
        else {
            return;
        };
        if let Some(runtime) = self.runtime_mut(key) {
            runtime.reconnecting = false;
        }
        // RF-20.29: a nota de que é outro shell, uma vez por painel, na
        // primeira volta.
        let resumed_note = messages::ssh_reconnect_not_resumed_note(&self.catalog);
        self.write_note(key, &resumed_note);
        let _ = cycle.take_resumed_note();
        for follower in cycle.followers().collect::<Vec<_>>() {
            self.reattach_ssh(follower, Connection::Share(connection), now);
            self.write_note(follower, &resumed_note);
        }
    }

    /// `reattach` de um painel SSH com `connection`, no tamanho que a grade tem
    /// agora. O que foi digitado antes de o shell novo abrir é descartado.
    fn reattach_ssh(&mut self, key: PaneKey, connection: Connection, now: Instant) -> bool {
        let catalog = self.catalog.clone();
        let Some(state) = self.windows.get_mut(&key.0) else {
            return false;
        };
        let Some(runtime) = state.panes.get_mut(&(key.1, key.2)) else {
            return false;
        };
        let Some(template) = runtime.ssh_spawn.clone() else {
            return false;
        };
        let (rows, cols) = match (runtime.snapshot.rows, runtime.snapshot.cols) {
            (0, _) | (_, 0) => (24, 80),
            (rows, cols) => (rows as u16, cols as u16),
        };
        let mut spawn = template;
        spawn.rows = rows;
        spawn.cols = cols;
        spawn.connection = connection;
        spawn.discard_input_until_open = true;
        match runtime.terminal.reattach(BackendSpec::Ssh(spawn)) {
            Ok(()) => {
                runtime.dormant = false;
                runtime.remote_cwd = None;
                runtime.reconnecting = false;
                true
            }
            Err(err) => {
                runtime.reconnecting = false;
                state.warnings.push(
                    Severity::Error,
                    msg::notice::spawn_failed::title(&catalog),
                    messages::terminal_spawn_error(&catalog, &err),
                    now,
                );
                if let Some(pane) = state
                    .workspace
                    .tab_mut(key.1)
                    .and_then(|tab| tab.panes_mut().pane_mut(key.2))
                {
                    pane.mark_exited(1);
                }
                false
            }
        }
    }

    /// Começa as tentativas cujo prazo venceu e limpa os painéis que fecharam.
    /// Chamado a cada volta do laço de eventos (`schedule_next_wake`).
    pub(super) fn poll_reconnects(&mut self, now: Instant) {
        self.prune_reconnects(now);
        let due: Vec<(ConnectionId, u8)> = self
            .reconnects
            .iter_mut()
            .filter_map(|(id, cycle)| cycle.state.poll(now).map(|attempt| (*id, attempt)))
            .collect();
        for (id, attempt) in due {
            self.start_attempt(id, attempt, now);
        }
    }

    /// O próximo acordar de todos os ciclos, para o `WaitUntil`.
    pub(super) fn reconnect_deadline(&self) -> Option<Instant> {
        self.reconnects
            .values()
            .filter_map(|cycle| cycle.state.next_wakeup())
            .min()
    }

    /// A tentativa `attempt`: o painel mais antigo faz `reattach` com uma
    /// conexão nova.
    fn start_attempt(&mut self, id: ConnectionId, attempt: u8, now: Instant) {
        let Some(owner) = self.reconnects.get(&id).and_then(Cycle::owner) else {
            self.reconnects.remove(&id);
            return;
        };
        // O painel pode estar `Exited` (`ssh.reconnect` depois de desistir): o
        // modelo o põe de volta em `Running`, porque ganhou backend.
        if let Some(pane) = self
            .windows
            .get_mut(&owner.0)
            .and_then(|state| state.workspace.tab_mut(owner.1))
            .and_then(|tab| tab.panes_mut().pane_mut(owner.2))
        {
            pane.resume();
        }
        if let Some(runtime) = self.runtime_mut(owner) {
            runtime.reconnecting = true;
        }
        self.write_state_note(owner, Reconnect::Connecting { attempt }, now);
        if !self.reattach_ssh(owner, Connection::New, now) {
            // Não saiu do chão: é uma falha da tentativa (`Runtime` não se
            // repete).
            let reason = EndReason::Failed {
                cause: porecatu_term::FailCause::Runtime,
            };
            if let Some(cycle) = self.reconnects.get_mut(&id) {
                cycle
                    .state
                    .on_attempt_failed(&porecatu_term::FailCause::Runtime, now);
            }
            self.after_attempt(id, owner, &reason, now);
        }
    }

    /// Esquece o que fechou: um painel que sumiu sai do ciclo, e o próximo
    /// assume a tentativa (ADR-0076 §3, item 3); a conexão sem painel nenhum
    /// perde a entrada do mapa.
    fn prune_reconnects(&mut self, now: Instant) {
        let gone: Vec<(ConnectionId, PaneKey)> = self
            .reconnects
            .iter()
            .flat_map(|(id, cycle)| {
                cycle
                    .members()
                    .iter()
                    .filter(|key| self.runtime(**key).is_none())
                    .map(|key| (*id, *key))
                    .collect::<Vec<_>>()
            })
            .collect();
        for (id, key) in gone {
            let Some(cycle) = self.reconnects.get_mut(&id) else {
                continue;
            };
            match cycle.remove(key) {
                Departure::Last => {
                    self.reconnects.remove(&id);
                }
                Departure::NewOwner(_) => {
                    // A tentativa em andamento morreu com o painel: o novo a
                    // refaz já.
                    if let Reconnect::Connecting { attempt } = cycle.state {
                        cycle.state = Reconnect::Waiting { attempt, due: now };
                    }
                }
                Departure::Follower | Departure::Unknown => {}
            }
        }
    }

    /// `ssh.reconnect` e o item "Reconectar" do menu (ADR-0076 §5): no painel
    /// SSH que espera a próxima tentativa, antecipa-a; no que está `Exited` ou
    /// desistiu, recomeça a contagem; em qualquer outro, não faz nada.
    pub(super) fn ssh_reconnect_pane(&mut self, key: PaneKey, now: Instant) {
        if self
            .runtime(key)
            .is_none_or(|runtime| runtime.ssh_spawn.is_none() || runtime.dormant)
        {
            return;
        }
        if let Some(id) = self.cycle_of(key) {
            if let Some(cycle) = self.reconnects.get_mut(&id) {
                cycle.state.reconnect_now(now);
            }
        } else {
            let exited = self
                .windows
                .get(&key.0)
                .and_then(|state| state.workspace.tab(key.1))
                .and_then(|tab| tab.panes().pane(key.2))
                .is_some_and(|pane| pane.is_exited());
            if !exited {
                return;
            }
            // Um ciclo novo, com a contagem do zero, que vence agora.
            let mut cycle = Cycle::new(now, key);
            cycle.state.reconnect_now(now);
            let Some(connection) = self
                .runtime(key)
                .and_then(|runtime| runtime.terminal.connection_id())
            else {
                return;
            };
            self.reconnects.insert(connection, cycle);
            if let Some(runtime) = self.runtime_mut(key) {
                runtime.reconnecting = true;
            }
        }
        self.poll_reconnects(now);
        if let Some(state) = self.windows.get(&key.0) {
            state.window.request_redraw();
        }
    }
}
