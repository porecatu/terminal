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

use std::path::PathBuf;
use std::time::{Duration, Instant};

use porecatu_core::{TabId, Workspace};
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
/// puro, testável sem um `winit::window::Window` real (`cwd_fallback` é
/// injetado pelo mesmo motivo: `App::build_session_file` é quem tem
/// acesso a `TabRuntime`/`Terminal`, este módulo não). `App::
/// build_session_file` é quem drena `winit` (`window_geometry`/
/// `window_monitor`) antes de chamar isto, uma janela por vez.
///
/// RF-3.10/ADR-0038 §2: o fallback só é consultado para a aba cujo `cwd`
/// saiu `None` de [`porecatu_session::convert::window_from_workspace`] --
/// ou seja, que nunca recebeu OSC 7. Com OSC 7 presente, a aba já tem
/// `cwd` aqui e o closure nem é chamado para ela.
pub fn window_v1(
    workspace: &Workspace,
    geometry: GeometryV1,
    monitor: Option<MonitorIdV1>,
    theme: Option<String>,
    zoom_steps: i32,
    cwd_fallback: impl Fn(TabId) -> Option<PathBuf>,
) -> WindowV1 {
    let (groups, mut tabs, active_tab) =
        porecatu_session::convert::window_from_workspace(workspace);
    for tab in &mut tabs {
        if tab.cwd.is_none() {
            tab.cwd = cwd_fallback(TabId::new(tab.id));
        }
    }
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

/// Inverso de [`zoom_px_to_steps`] (RF-5.9/RF-3.1, metade de restauração):
/// `zoom_steps` gravado -> `font_zoom_px` de sessão. Zero passos é "sem
/// override" (`None`), do mesmo jeito que zero é o resultado de nenhum
/// `font.increase`/`decrease` ter rodado. `min_px`/`max_px` protegem
/// contra um arquivo editado à mão com passos absurdos -- o mesmo clamp
/// que `App::apply_zoom` já aplica ao vivo.
pub fn steps_to_zoom_px(
    steps: i32,
    base_size_px: f32,
    step_px: f32,
    min_px: f32,
    max_px: f32,
) -> Option<f32> {
    if steps == 0 {
        return None;
    }
    Some((base_size_px + steps as f32 * step_px).clamp(min_px, max_px))
}

/// Tema de sessão restaurado (ADR-0031 §4, ADR-0036 §3): o arquivo é
/// checado contra os temas **hoje** declarados na config -- um tema que
/// existia quando a sessão foi gravada e sumiu do arquivo desde então
/// recebe o mesmo tratamento que o hot reload já dá a um tema que some
/// (zera e avisa), nunca aplicado calado. Nome vazio é "sem tema", nunca
/// "vanished" (é o estado inicial do ciclo, ADR-0031 §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoredTheme {
    Applied(Option<String>),
    Vanished(String),
}

pub fn restore_theme(saved: Option<&str>, declared: &[&str]) -> RestoredTheme {
    match saved {
        Some(name) if !name.is_empty() && !declared.contains(&name) => {
            RestoredTheme::Vanished(name.to_string())
        }
        Some(name) => RestoredTheme::Applied(Some(name.to_string())),
        None => RestoredTheme::Applied(None),
    }
}

/// Um monitor, na forma mínima que o casamento do ADR-0036 §4 precisa --
/// testável sem `winit::monitor::MonitorHandle`, que não é construível em
/// teste. [`monitor_info`] converte o `MonitorHandle` real na hora de
/// restaurar.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    pub name: Option<String>,
    pub position: (i32, i32),
    pub size: (u32, u32),
}

/// Converte o `MonitorHandle` do `winit` para a forma testável acima --
/// mesmos três campos que [`window_monitor`] já extrai da janela.
pub fn monitor_info(monitor: &winit::monitor::MonitorHandle) -> MonitorInfo {
    let position = monitor.position();
    let size = monitor.size();
    MonitorInfo {
        name: monitor.name(),
        position: (position.x, position.y),
        size: (size.width, size.height),
    }
}

/// Geometria final de uma janela restaurada, em pixels físicos -- pronta
/// para `WindowAttributes::with_position`/`with_inner_size`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedGeometry {
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub maximized: bool,
}

/// Casamento de monitor (ADR-0036 §4): nome vence quando presente e bate
/// com algum candidato; sem nome, ou sem nome batendo, cai na posição --
/// o desempate para quando o mesmo modelo aparece duas vezes. `None`
/// (sem monitor gravado, ou nenhum candidato bate) é "sem casamento": quem
/// chama cai no primário.
fn match_restored_monitor<'a>(
    target: Option<&MonitorIdV1>,
    candidates: &'a [MonitorInfo],
) -> Option<&'a MonitorInfo> {
    let target = target?;
    if let Some(name) = target.name.as_deref()
        && let Some(found) = candidates.iter().find(|m| m.name.as_deref() == Some(name))
    {
        return Some(found);
    }
    candidates
        .iter()
        .find(|m| m.position == (target.x, target.y))
}

/// RF-3.11 (ADR-0036 §4): geometria gravada quando o monitor casa; sem
/// casamento, monitor primário com o tamanho **preservado dentro dos
/// limites da tela** -- mesmo clamp que `App::open_window` já usa para a
/// cascata de janela nova (lá contra o monitor de origem, aqui contra o
/// primário). `restore_window_geometry = false` não passa por aqui: quem
/// chama nem invoca esta função nesse caso, usando o default da
/// plataforma para a janela inteira.
pub fn resolve_restored_geometry(
    geometry: &GeometryV1,
    monitor: Option<&MonitorIdV1>,
    monitors: &[MonitorInfo],
    primary: Option<&MonitorInfo>,
) -> ResolvedGeometry {
    let recorded = ResolvedGeometry {
        position: (geometry.x, geometry.y),
        size: (geometry.width, geometry.height),
        maximized: geometry.maximized,
    };
    if match_restored_monitor(monitor, monitors).is_some() {
        return recorded;
    }
    let Some(primary) = primary else {
        // Nenhum monitor conhecido (raríssimo) -- geometria como veio, sem
        // clamp possível.
        return recorded;
    };
    let size = (
        geometry.width.min(primary.size.0),
        geometry.height.min(primary.size.1),
    );
    let max_x = primary.position.0 + primary.size.0 as i32 - size.0 as i32;
    let max_y = primary.position.1 + primary.size.1 as i32 - size.1 as i32;
    let position = (
        geometry
            .x
            .clamp(primary.position.0, max_x.max(primary.position.0)),
        geometry
            .y
            .clamp(primary.position.1, max_y.max(primary.position.1)),
    );
    ResolvedGeometry {
        position,
        size,
        maximized: geometry.maximized,
    }
}

/// RF-3.10 (ADR-0017 §5): decide se o `cwd` da aba precisa cair no home --
/// puro, não toca disco. Quem chama já resolveu `exists` (`Path::is_dir`)
/// e é quem escreve a nota de verdade (`Terminal::inject_note`, canal 2 do
/// ADR-0014) quando este devolve `Some`. Chamado só no momento em que a
/// aba sobe de fato (`WindowState::spawn_tab_runtime`) -- nunca adiantado
/// para toda aba `NotStarted` de uma restauração, que seria N chamadas de
/// `exists()` no caminho do start para abas que talvez nunca sejam
/// focadas.
pub fn resolve_tab_cwd(
    cwd: Option<std::path::PathBuf>,
    exists: bool,
    startup_directory: &Option<std::path::PathBuf>,
) -> (Option<std::path::PathBuf>, Option<String>) {
    match cwd {
        Some(path) if !exists => (
            startup_directory.clone(),
            Some(format!(
                "diretório \"{}\" não existe mais, aba aberta no home",
                path.display()
            )),
        ),
        other => (other, None),
    }
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
            window_v1(&first, geometry(0), None, None, 0, |_| None),
            window_v1(
                &second,
                geometry(830),
                None,
                Some("dracula".to_string()),
                2,
                |_| None,
            ),
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

    /// ADR-0038 §2: `cwd` de OSC 7 vence -- o fallback nem precisa
    /// devolver o valor certo, só não pode ser chamado (aqui ele erra de
    /// propósito para provar que não entrou em jogo).
    #[test]
    fn osc7_cwd_wins_over_the_fallback() {
        let mut ws = Workspace::new();
        ws.append_tab("zsh", Some(PathBuf::from("/from/osc7")));
        let window = window_v1(&ws, geometry(0), None, None, 0, |_| {
            Some(PathBuf::from("/should/not/be/used"))
        });
        assert_eq!(window.tabs[0].cwd, Some(PathBuf::from("/from/osc7")));
    }

    /// Sem OSC 7 (`cwd` gravado como `None`), o fallback por `TabId`
    /// preenche o valor -- é o degrau `ProcessGroup::cwd`/`cwd` de spawn
    /// da precedência, resolvido por `App::build_session_file` antes de
    /// chegar aqui.
    #[test]
    fn missing_cwd_falls_back_by_tab_id() {
        let mut ws = Workspace::new();
        let id = ws.append_tab("zsh", None);
        let window = window_v1(&ws, geometry(0), None, None, 0, move |tab_id| {
            (tab_id == id).then(|| PathBuf::from("/fallback"))
        });
        assert_eq!(window.tabs[0].cwd, Some(PathBuf::from("/fallback")));
    }

    /// Nenhuma das duas fontes tem valor -- fica `None`, não inventa nada.
    #[test]
    fn missing_cwd_without_fallback_stays_none() {
        let mut ws = Workspace::new();
        ws.append_tab("zsh", None);
        let window = window_v1(&ws, geometry(0), None, None, 0, |_| None);
        assert_eq!(window.tabs[0].cwd, None);
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
    fn steps_to_zoom_px_is_none_without_steps() {
        assert_eq!(steps_to_zoom_px(0, 14.0, 1.0, 6.0, 96.0), None);
    }

    #[test]
    fn steps_to_zoom_px_inverts_zoom_px_to_steps() {
        assert_eq!(steps_to_zoom_px(3, 14.0, 1.0, 6.0, 96.0), Some(17.0));
        assert_eq!(steps_to_zoom_px(-3, 14.0, 1.0, 6.0, 96.0), Some(11.0));
    }

    #[test]
    fn steps_to_zoom_px_clamps_absurd_saved_steps() {
        assert_eq!(steps_to_zoom_px(-9999, 14.0, 1.0, 6.0, 96.0), Some(6.0));
        assert_eq!(steps_to_zoom_px(9999, 14.0, 1.0, 6.0, 96.0), Some(96.0));
    }

    #[test]
    fn restore_theme_applies_a_known_name() {
        assert_eq!(
            restore_theme(Some("dracula"), &["dracula", "nord"]),
            RestoredTheme::Applied(Some("dracula".to_string()))
        );
    }

    #[test]
    fn restore_theme_applies_no_theme() {
        assert_eq!(
            restore_theme(None, &["dracula"]),
            RestoredTheme::Applied(None)
        );
    }

    /// ADR-0031 §3: `""` é o estado "sem tema" do ciclo, distinto de
    /// `None` ("nunca ciclou, usa o do arquivo") -- nunca cai no ramo de
    /// "sumiu", porque não há nome nenhum para procurar na lista.
    #[test]
    fn restore_theme_keeps_the_explicit_no_theme_state_distinct_from_none() {
        assert_eq!(
            restore_theme(Some(""), &["dracula"]),
            RestoredTheme::Applied(Some(String::new()))
        );
    }

    #[test]
    fn restore_theme_flags_a_name_that_is_no_longer_declared() {
        assert_eq!(
            restore_theme(Some("sumiu"), &["dracula"]),
            RestoredTheme::Vanished("sumiu".to_string())
        );
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

    fn monitor(name: Option<&str>, x: i32, y: i32, width: u32, height: u32) -> MonitorInfo {
        MonitorInfo {
            name: name.map(str::to_string),
            position: (x, y),
            size: (width, height),
        }
    }

    fn monitor_id(name: Option<&str>, x: i32, y: i32) -> MonitorIdV1 {
        MonitorIdV1 {
            name: name.map(str::to_string),
            x,
            y,
        }
    }

    /// Monitor casado (por nome): geometria gravada volta intocada, sem
    /// clamp nenhum.
    #[test]
    fn matched_monitor_by_name_keeps_recorded_geometry() {
        let monitors = [
            monitor(Some("DISPLAY1"), 0, 0, 1920, 1080),
            monitor(Some("DISPLAY2"), 1920, 0, 2560, 1440),
        ];
        let resolved = resolve_restored_geometry(
            &geometry(2000),
            Some(&monitor_id(Some("DISPLAY2"), 1920, 0)),
            &monitors,
            Some(&monitors[0]),
        );
        assert_eq!(resolved.position, (2000, 0));
        assert_eq!(resolved.size, (800, 600));
    }

    /// Sem nome batendo, cai na posição -- o desempate para o mesmo
    /// modelo aparecendo duas vezes.
    #[test]
    fn falls_back_to_position_when_name_does_not_match() {
        let monitors = [monitor(Some("outro-nome"), 1920, 0, 2560, 1440)];
        let resolved = resolve_restored_geometry(
            &geometry(2000),
            Some(&monitor_id(Some("DISPLAY2"), 1920, 0)),
            &monitors,
            Some(&monitors[0]),
        );
        assert_eq!(resolved.position, (2000, 0));
    }

    /// RF-3.11: sem casamento nenhum, cai no primário com o tamanho
    /// preservado dentro dos limites da tela -- geometria gravada era de
    /// um monitor de 2560px de largura, primário agora só tem 1920.
    #[test]
    fn no_match_falls_back_to_primary_clamped_within_bounds() {
        let primary = monitor(Some("DISPLAY1"), 0, 0, 1920, 1080);
        let recorded = GeometryV1 {
            x: 2200,
            y: 100,
            width: 2400,
            height: 1300,
            maximized: false,
        };
        let resolved = resolve_restored_geometry(
            &recorded,
            Some(&monitor_id(Some("sumiu"), 1920, 0)),
            &[],
            Some(&primary),
        );
        assert_eq!(resolved.size, (1920, 1080));
        assert_eq!(resolved.position, (0, 0));
    }

    /// Sem monitor gravado nenhum (arquivo antigo, ou plataforma que não
    /// deu monitor no momento da gravação): mesmo caminho de "sem
    /// casamento" -- primário, tamanho preservado dentro dos limites.
    #[test]
    fn no_recorded_monitor_falls_back_to_primary_too() {
        let primary = monitor(None, 0, 0, 1920, 1080);
        let resolved = resolve_restored_geometry(&geometry(500), None, &[], Some(&primary));
        assert_eq!(resolved.size, (800, 600));
        assert_eq!(resolved.position, (500, 0));
    }

    #[test]
    fn maximized_flag_survives_either_path() {
        let mut g = geometry(0);
        g.maximized = true;
        let resolved = resolve_restored_geometry(&g, None, &[], None);
        assert!(resolved.maximized);
    }

    #[test]
    fn existing_cwd_is_kept_without_a_note() {
        let home = Some(std::path::PathBuf::from("/home/user"));
        let (cwd, note) = resolve_tab_cwd(Some(std::path::PathBuf::from("/srv/api")), true, &home);
        assert_eq!(cwd, Some(std::path::PathBuf::from("/srv/api")));
        assert_eq!(note, None);
    }

    #[test]
    fn missing_cwd_falls_back_to_home_with_a_note() {
        let home = Some(std::path::PathBuf::from("/home/user"));
        let (cwd, note) =
            resolve_tab_cwd(Some(std::path::PathBuf::from("/srv/sumiu")), false, &home);
        assert_eq!(cwd, home);
        assert!(note.is_some_and(|n| n.contains("/srv/sumiu")));
    }

    #[test]
    fn absent_cwd_stays_absent_without_a_note() {
        let home = Some(std::path::PathBuf::from("/home/user"));
        let (cwd, note) = resolve_tab_cwd(None, true, &home);
        assert_eq!(cwd, None);
        assert_eq!(note, None);
    }
}
