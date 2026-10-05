// SPDX-License-Identifier: GPL-3.0-or-later

//! Fase do piscar do cursor (`[terminal.cursor] blink`/`blink_interval_ms`,
//! RF-5.22). Estado puro: recebe `Instant` de fora e nunca chama
//! `Instant::now()` (regra do projeto, ADR-0022) -- por isso é testável sem
//! dormir. O prazo entra em `WindowState::next_wake`, então o event loop
//! dorme de verdade (`ControlFlow::WaitUntil`) quando nada pisca: terminal
//! ocioso com cursor fixo continua custando zero frame (ADR-0007).

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct CursorBlink {
    on: bool,
    since: Instant,
    /// `Some` enquanto há um cursor piscando de verdade (painel focado, janela
    /// com foco, estilo vigente piscante); `None` desliga o relógio.
    interval: Option<Duration>,
}

impl CursorBlink {
    pub fn new(now: Instant) -> Self {
        Self {
            on: true,
            since: now,
            interval: None,
        }
    }

    /// Fase atual: `true` = cursor desenhado.
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Liga ou desliga o relógio. Ao ligar (transição `None` -> `Some`) a
    /// fase recomeça acesa; mudar só o intervalo não reinicia o ciclo.
    pub fn set_interval(&mut self, interval: Option<Duration>, now: Instant) {
        match (self.interval, interval) {
            (_, None) => {
                self.on = true;
                self.interval = None;
            }
            (None, Some(_)) => {
                self.on = true;
                self.since = now;
                self.interval = interval;
            }
            (Some(_), Some(_)) => self.interval = interval,
        }
    }

    /// Tecla, foco ganho: o cursor reaparece aceso e o ciclo recomeça.
    /// Devolve `true` se a fase mudou (precisa de redraw).
    pub fn reset(&mut self, now: Instant) -> bool {
        let changed = !self.on;
        self.on = true;
        self.since = now;
        changed
    }

    /// Avança a fase se o prazo venceu. Devolve `true` se mudou.
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.interval {
            Some(interval) if now >= self.since + interval => {
                self.on = !self.on;
                self.since = now;
                true
            }
            _ => false,
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.interval.map(|interval| self.since + interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HALF_SECOND: Duration = Duration::from_millis(500);

    #[test]
    fn idle_without_interval_never_schedules_or_toggles() {
        let t0 = Instant::now();
        let mut blink = CursorBlink::new(t0);
        assert_eq!(blink.next_deadline(), None);
        assert!(!blink.tick(t0 + Duration::from_secs(10)));
        assert!(blink.is_on());
    }

    #[test]
    fn toggles_each_interval_and_schedules_the_next_deadline() {
        let t0 = Instant::now();
        let mut blink = CursorBlink::new(t0);
        blink.set_interval(Some(HALF_SECOND), t0);
        assert_eq!(blink.next_deadline(), Some(t0 + HALF_SECOND));

        assert!(!blink.tick(t0 + Duration::from_millis(499)));
        assert!(blink.is_on());

        let t1 = t0 + HALF_SECOND;
        assert!(blink.tick(t1));
        assert!(!blink.is_on());
        assert_eq!(blink.next_deadline(), Some(t1 + HALF_SECOND));

        assert!(blink.tick(t1 + HALF_SECOND));
        assert!(blink.is_on());
    }

    #[test]
    fn disabling_the_clock_leaves_the_cursor_drawn() {
        let t0 = Instant::now();
        let mut blink = CursorBlink::new(t0);
        blink.set_interval(Some(HALF_SECOND), t0);
        blink.tick(t0 + HALF_SECOND);
        assert!(!blink.is_on());

        blink.set_interval(None, t0 + HALF_SECOND);
        assert!(blink.is_on());
        assert_eq!(blink.next_deadline(), None);
    }

    #[test]
    fn reset_lights_the_cursor_and_restarts_the_cycle() {
        let t0 = Instant::now();
        let mut blink = CursorBlink::new(t0);
        blink.set_interval(Some(HALF_SECOND), t0);
        blink.tick(t0 + HALF_SECOND);
        assert!(!blink.is_on());

        let t1 = t0 + Duration::from_millis(700);
        assert!(blink.reset(t1), "estava apagado: precisa de redraw");
        assert!(blink.is_on());
        assert_eq!(blink.next_deadline(), Some(t1 + HALF_SECOND));
        assert!(!blink.reset(t1), "já aceso: nada mudou");
    }

    #[test]
    fn changing_only_the_interval_keeps_the_cycle() {
        let t0 = Instant::now();
        let mut blink = CursorBlink::new(t0);
        blink.set_interval(Some(HALF_SECOND), t0);
        blink.set_interval(
            Some(Duration::from_millis(250)),
            t0 + Duration::from_millis(100),
        );
        assert_eq!(blink.next_deadline(), Some(t0 + Duration::from_millis(250)));
    }
}
