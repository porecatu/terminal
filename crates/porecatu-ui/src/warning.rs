// SPDX-License-Identifier: GPL-3.0-or-later

//! Pilha de avisos do app (ADR-0014 canal 1, PRD-010 RF-10.15/RF-10.16):
//! fatos que valem para o app inteiro, não para uma aba só -- essa distinção
//! é o que separa isto da nota escrita no grid (`Terminal::inject_note`,
//! canal 2, já em uso desde a Etapa 4 para RF-1.3).
//!
//! Modelo puro, sem `winit` nem `wgpu`: o instante (`Instant`) sempre chega
//! de fora, nunca lido daqui (`Instant::now()` não aparece neste arquivo) --
//! é o que torna o temporizador de dispensa da informação testável sem
//! dormir de verdade. Pintura fica em `overlay.rs`.

use std::time::{Duration, Instant};

/// Espec. §2.14: "no máximo três avisos", "o quarto substitui o mais
/// antigo".
pub const MAX_WARNINGS: usize = 3;
/// Espec. §2.14: "informação sai em 6s".
const INFO_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Warning {
    pub severity: Severity,
    pub title: String,
    pub body: String,
    /// Instante em que desaparece sozinha -- só para `Info` (espec: "erro e
    /// aviso persistem até dispensa"). `None` também enquanto pausado por
    /// hover; `remaining` guarda quanto faltava nesse caso.
    dismiss_at: Option<Instant>,
    remaining: Option<Duration>,
}

impl Warning {
    fn new(severity: Severity, title: String, body: String, now: Instant) -> Self {
        let dismiss_at = (severity == Severity::Info).then(|| now + INFO_TIMEOUT);
        Self {
            severity,
            title,
            body,
            dismiss_at,
            remaining: None,
        }
    }

    fn is_expired(&self, now: Instant) -> bool {
        self.dismiss_at.is_some_and(|at| now >= at)
    }
}

/// Empilha do mais antigo (índice 0) ao mais recente -- "o topo" que `Esc`
/// dispensa (espec §2.14, RF-10.16) é o item mais recente, o de cima da
/// pilha visual.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WarningStack {
    items: Vec<Warning>,
    /// Espec §2.14: "o temporizador da informação pausa no hover" -- vale
    /// pra pilha inteira, não por item: hover em qualquer aviso da pilha
    /// pausa todos os que têm temporizador.
    hovered: bool,
}

impl WarningStack {
    pub fn push(
        &mut self,
        severity: Severity,
        title: impl Into<String>,
        body: impl Into<String>,
        now: Instant,
    ) {
        if self.items.len() >= MAX_WARNINGS {
            self.items.remove(0);
        }
        let mut warning = Warning::new(severity, title.into(), body.into(), now);
        // Aviso novo nasce já pausado se a pilha está sob o cursor -- sem
        // isso ele começaria a contar antes de o usuário nem ter visto.
        if self.hovered
            && let Some(at) = warning.dismiss_at.take()
        {
            warning.remaining = Some(at.saturating_duration_since(now));
        }
        self.items.push(warning);
    }

    pub fn dismiss(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }

    /// RF-10.16: `Esc` dispensa o do topo (o mais recente).
    pub fn dismiss_top(&mut self) {
        self.items.pop();
    }

    pub fn set_hovered(&mut self, hovered: bool, now: Instant) {
        if hovered == self.hovered {
            return;
        }
        self.hovered = hovered;
        if hovered {
            for item in &mut self.items {
                if let Some(at) = item.dismiss_at.take() {
                    item.remaining = Some(at.saturating_duration_since(now));
                }
            }
        } else {
            for item in &mut self.items {
                if let Some(remaining) = item.remaining.take() {
                    item.dismiss_at = Some(now + remaining);
                }
            }
        }
    }

    /// Remove avisos de informação cujo prazo já passou. Erro e aviso nunca
    /// expiram sozinhos.
    pub fn tick(&mut self, now: Instant) {
        self.items.retain(|item| !item.is_expired(now));
    }

    /// Próximo instante em que algo muda por conta própria -- para
    /// `App::about_to_wait` agendar `ControlFlow::WaitUntil`. `None` quando
    /// não há nenhuma informação pendente (nada pausado conta: não tem
    /// prazo até sair do hover).
    pub fn next_deadline(&self) -> Option<Instant> {
        self.items.iter().filter_map(|item| item.dismiss_at).min()
    }

    pub fn items(&self) -> &[Warning] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: u64) -> Instant {
        // `Instant` não tem construtor determinístico público -- ancora
        // tudo em `Instant::now()` uma vez por teste e soma dali. Os testes
        // nunca chamam `Instant::now()` de novo depois disso.
        Instant::now() + Duration::from_secs(secs)
    }

    #[test]
    fn error_and_warning_never_expire() {
        let mut stack = WarningStack::default();
        stack.push(Severity::Error, "erro", "corpo", t(0));
        stack.push(Severity::Warning, "aviso", "corpo", t(0));
        stack.tick(t(1000));
        assert_eq!(stack.items().len(), 2);
    }

    #[test]
    fn info_expires_after_timeout() {
        let mut stack = WarningStack::default();
        stack.push(Severity::Info, "info", "corpo", t(0));
        stack.tick(t(5));
        assert_eq!(stack.items().len(), 1);
        stack.tick(t(7));
        assert!(stack.is_empty());
    }

    #[test]
    fn fourth_warning_replaces_oldest() {
        let mut stack = WarningStack::default();
        stack.push(Severity::Error, "a", "", t(0));
        stack.push(Severity::Error, "b", "", t(0));
        stack.push(Severity::Error, "c", "", t(0));
        stack.push(Severity::Error, "d", "", t(0));
        assert_eq!(stack.items().len(), MAX_WARNINGS);
        assert_eq!(stack.items()[0].title, "b");
        assert_eq!(stack.items()[2].title, "d");
    }

    #[test]
    fn dismiss_top_removes_most_recent() {
        let mut stack = WarningStack::default();
        stack.push(Severity::Error, "a", "", t(0));
        stack.push(Severity::Error, "b", "", t(0));
        stack.dismiss_top();
        assert_eq!(stack.items().len(), 1);
        assert_eq!(stack.items()[0].title, "a");
    }

    #[test]
    fn hover_pauses_and_resuming_keeps_remaining_time() {
        let mut stack = WarningStack::default();
        stack.push(Severity::Info, "info", "corpo", t(0));
        stack.set_hovered(true, t(5)); // faltavam 1s (de 6) quando pausou
        stack.tick(t(100)); // muito tempo pausado, não deveria expirar
        assert_eq!(stack.items().len(), 1);
        stack.set_hovered(false, t(100)); // retoma com 1s restante
        stack.tick(t(100)); // ainda não passou o 1s restante
        assert_eq!(stack.items().len(), 1);
        stack.tick(t(102));
        assert!(stack.is_empty());
    }

    #[test]
    fn next_deadline_ignores_persistent_and_paused() {
        let base = Instant::now();
        let mut stack = WarningStack::default();
        stack.push(Severity::Error, "a", "", base);
        assert_eq!(stack.next_deadline(), None);
        stack.push(Severity::Info, "info", "", base);
        assert_eq!(stack.next_deadline(), Some(base + Duration::from_secs(6)));
        stack.set_hovered(true, base + Duration::from_secs(1));
        assert_eq!(stack.next_deadline(), None);
    }
}
