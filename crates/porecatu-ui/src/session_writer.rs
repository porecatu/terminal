// SPDX-License-Identifier: GPL-3.0-or-later

//! Gravação de sessão (F5 etapa 2, RF-3.2/RF-3.3/RF-3.4/RF-3.6, metade de
//! gravação do RF-3.17). Debounce pelo `ControlFlow::WaitUntil` que já
//! move o tooltip/aviso/animação (`AnimationClock`/`Hover`/
//! `WarningStack`) -- `Instant` injetado, sem thread, nunca chama
//! `Instant::now()` aqui, mesmo molde de `reload::Debounce`.
//!
//! `enabled = false` (RF-3.6) não é decidido aqui: quem chama
//! `mark_dirty`/grava de fato consulta a config antes -- este módulo só
//! sabe debounçar e converter geometria/monitor de `winit`.

use std::time::{Duration, Instant};

use porecatu_core::Workspace;
use porecatu_session::{GeometryV1, MonitorIdV1, WindowV1};
use winit::dpi::PhysicalPosition;
use winit::window::Window;

/// Estado puro do debounce de gravação (RF-3.3): `mark_dirty` adia o
/// disparo, `ready` dispara uma vez só e limpa -- a mesma rajada de
/// mudanças não produz duas escritas.
#[derive(Debug, Default)]
pub struct SessionScheduler {
    deadline: Option<Instant>,
}

impl SessionScheduler {
    pub fn mark_dirty(&mut self, now: Instant, debounce: Duration) {
        self.deadline = Some(now + debounce);
    }

    /// `true` uma vez só, quando o período de silêncio termina; limpa o
    /// estado, então a mesma rajada não dispara duas vezes.
    pub fn ready(&mut self, now: Instant) -> bool {
        match self.deadline {
            Some(deadline) if now >= deadline => {
                self.deadline = None;
                true
            }
            _ => false,
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Descarta o agendamento sem gravar -- usado depois da gravação
    /// síncrona do exit, pra não deixar um debounce pendente apontando
    /// pro passado.
    pub fn clear(&mut self) {
        self.deadline = None;
    }
}

/// Geometria da janela (ADR-0036 §1/§4), em pixels físicos -- a unidade
/// que `winit` já devolve, sem conversão. `outer_position` pode falhar
/// (raro, plataforma sem suporte); cai em `(0, 0)` em vez de descartar a
/// janela inteira da gravação.
pub fn window_geometry(window: &Window) -> GeometryV1 {
    let position = window
        .outer_position()
        .unwrap_or(PhysicalPosition::new(0, 0));
    let size = window.inner_size();
    GeometryV1 {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized: window.is_maximized(),
    }
}

/// Identidade do monitor (ADR-0036 §4): nome quando a plataforma o dá,
/// mais a posição da origem no espaço virtual como desempate. `None`
/// quando `winit` não sabe dizer em qual monitor a janela está.
pub fn window_monitor(window: &Window) -> Option<MonitorIdV1> {
    let monitor = window.current_monitor()?;
    let position = monitor.position();
    Some(MonitorIdV1 {
        name: monitor.name(),
        x: position.x,
        y: position.y,
    })
}

/// Monta o `WindowV1` de uma janela a partir das peças já extraídas --
/// puro, testável sem um `winit::window::Window` real. `App::
/// build_session_file` é quem drena `winit` (`window_geometry`/
/// `window_monitor`) antes de chamar isto, uma janela por vez.
pub fn window_v1(
    workspace: &Workspace,
    geometry: GeometryV1,
    monitor: Option<MonitorIdV1>,
    theme: Option<String>,
    zoom_steps: i32,
) -> WindowV1 {
    let (groups, tabs, active_tab) = porecatu_session::convert::window_from_workspace(workspace);
    WindowV1 {
        geometry,
        monitor,
        groups,
        tabs,
        active_tab,
        theme,
        zoom_steps,
    }
}

/// RF-5.9 (ADR-0036 §3): converte o tamanho de fonte absoluto de zoom da
/// sessão (`App::font_zoom_px`, `None` = sem override) em passos --
/// `FONT_ZOOM_STEP_PX` é `1.0`, então a conversão é exata, sem
/// arredondamento perceptível mesmo depois de várias sessões.
pub fn zoom_px_to_steps(font_zoom_px: Option<f32>, base_size_px: f32, step_px: f32) -> i32 {
    font_zoom_px.map_or(0, |px| ((px - base_size_px) / step_px).round() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use porecatu_core::GroupColor;

    fn geometry(x: i32) -> GeometryV1 {
        GeometryV1 {
            x,
            y: 0,
            width: 800,
            height: 600,
            maximized: false,
        }
    }

    /// RF-3.17 (metade de gravação): duas janelas produzem duas entradas
    /// em `windows`, na ordem em que foram montadas -- `build_session_file`
    /// (em `lib.rs`) monta uma por janela, na ordem de `self.windows`;
    /// aqui a montagem em si (`window_v1`) é o que se testa sem depender
    /// de `winit::window::Window`.
    #[test]
    fn two_windows_produce_two_entries_in_order() {
        let mut first = Workspace::new();
        first.append_tab("zsh", None);

        let mut second = Workspace::new();
        let a = second.append_tab("bash", None);
        second.group_tabs(&[a], "api", GroupColor::Blue).unwrap();

        let windows = vec![
            window_v1(&first, geometry(0), None, None, 0),
            window_v1(&second, geometry(830), None, Some("dracula".to_string()), 2),
        ];

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].geometry.x, 0);
        assert!(windows[0].groups.iter().all(|g| g.color.is_none()));
        assert_eq!(windows[1].geometry.x, 830);
        assert_eq!(windows[1].theme.as_deref(), Some("dracula"));
        assert_eq!(windows[1].zoom_steps, 2);
        assert!(
            windows[1]
                .groups
                .iter()
                .any(|g| g.name.as_deref() == Some("api"))
        );
    }

    #[test]
    fn zoom_px_to_steps_is_zero_without_override() {
        assert_eq!(zoom_px_to_steps(None, 14.0, 1.0), 0);
    }

    #[test]
    fn zoom_px_to_steps_counts_increments_and_decrements() {
        assert_eq!(zoom_px_to_steps(Some(17.0), 14.0, 1.0), 3);
        assert_eq!(zoom_px_to_steps(Some(11.0), 14.0, 1.0), -3);
        assert_eq!(zoom_px_to_steps(Some(14.0), 14.0, 1.0), 0);
    }

    #[test]
    fn debounce_collapses_a_burst_into_one_fire() {
        let mut s = SessionScheduler::default();
        let now = Instant::now();
        let debounce = Duration::from_millis(2000);
        s.mark_dirty(now, debounce);
        s.mark_dirty(now + Duration::from_millis(500), debounce);
        s.mark_dirty(now + Duration::from_millis(900), debounce);

        // Sem o adiamento dos dois eventos seguintes, já teria disparado.
        assert!(!s.ready(now + Duration::from_millis(2000)));
        assert!(s.ready(now + Duration::from_millis(2900)));
        assert!(
            !s.ready(now + Duration::from_millis(2900)),
            "dispara uma vez só, depois limpa"
        );
    }

    #[test]
    fn debounce_without_events_never_fires() {
        let mut s = SessionScheduler::default();
        assert!(!s.ready(Instant::now() + Duration::from_secs(9999)));
        assert_eq!(s.next_deadline(), None);
    }

    #[test]
    fn debounce_fires_again_for_a_second_burst() {
        let mut s = SessionScheduler::default();
        let now = Instant::now();
        let debounce = Duration::from_millis(100);
        s.mark_dirty(now, debounce);
        assert!(s.ready(now + Duration::from_millis(150)));

        s.mark_dirty(now + Duration::from_millis(200), debounce);
        assert!(!s.ready(now + Duration::from_millis(250)));
        assert!(s.ready(now + Duration::from_millis(320)));
    }

    #[test]
    fn clear_discards_pending_deadline() {
        let mut s = SessionScheduler::default();
        let now = Instant::now();
        s.mark_dirty(now, Duration::from_millis(100));
        s.clear();
        assert_eq!(s.next_deadline(), None);
        assert!(!s.ready(now + Duration::from_secs(1)));
    }
}
