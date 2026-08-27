// SPDX-License-Identifier: GPL-3.0-or-later

//! Relógio de animação por janela (ADR-0022): dirigido pelo `ControlFlow::
//! WaitUntil` que já existe pro tooltip/aviso, sem thread própria, ativo
//! só enquanto há animação pendente -- "sem sujeira, sem frame" (ADR-0007)
//! continua valendo, animação em curso *é* a sujeira. Recebe `Instant` de
//! fora e nunca chama `Instant::now()` -- mesma disciplina de
//! `WarningStack`/`ConfirmDialog`/`Hover`, o que torna isto testável sem
//! dormir de verdade.
//!
//! Lista fechada de dois consumidores (ADR-0022): reordenação ao formar
//! grupo (RF-2.5, `.18s`) e a reflui da trilha ao colapsar/expandir
//! (espec. §2.4, `.15s` -- "o que anima de fato é o resto do colapso: as
//! abas desaparecendo da trilha", não o glifo do caret, que troca sem
//! transição por falta de primitiva de rotação em `porecatu-render`). Os
//! dois usam o mesmo mecanismo -- capturar a posição X de cada wrapper
//! antes da operação que muda o modelo, interpolar linearmente até a
//! posição nova durante a duração. `Workspace` nunca é interpolado
//! (ADR-0022, "alternativas consideradas"): o modelo já está no estado
//! final desde o primeiro frame, só a posição de **desenho** anima --
//! `tab_bar::layout` continua alheio a isto, quem interpola é `chrome.rs`.
//!
//! Os dois têm gatilho de UI: colapso/expansão (clique na pílula, menu,
//! editor) e `group.create` (`Ctrl+Shift+G`, `WindowState::
//! action_group_create`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use porecatu_core::GroupId;

use crate::tab_bar::TabBarLayout;

/// Cadência de redraw enquanto uma animação está ativa (~60fps) -- não é
/// valor de design, é a mesma classe de constante de interação que
/// `input::MULTI_CLICK_THRESHOLD`.
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Debug, Clone)]
struct Reflow {
    started_at: Instant,
    duration: Duration,
    /// Posição X (coordenadas de conteúdo, pré-rolagem) do wrapper de cada
    /// grupo, capturada em `old_layout` antes da operação que disparou a
    /// animação. Só os grupos que existiam nesse layout entram aqui --
    /// um grupo recém-criado pela própria operação não tem "posição
    /// antiga" e não anima (nasce direto na posição final).
    wrapper_x: HashMap<GroupId, f32>,
}

impl Reflow {
    fn progress(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started_at).as_secs_f32();
        let duration = self.duration.as_secs_f32();
        if duration <= 0.0 {
            1.0
        } else {
            (elapsed / duration).clamp(0.0, 1.0)
        }
    }

    fn is_finished(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started_at) >= self.duration
    }
}

#[derive(Debug, Clone, Default)]
pub struct AnimationClock {
    active: Vec<Reflow>,
}

impl AnimationClock {
    /// Captura `old_layout` (calculado **antes** da operação que muda o
    /// modelo -- `Workspace::collapse_group`/`group_tabs`) e começa a
    /// animar a diferença até a posição que o layout **novo** (pós-
    /// operação) calcular. `duration`: `.18s` pro RF-2.5, `.15s` pro
    /// colapso/expansão (ADR-0022, lista fechada de dois consumidores).
    pub fn start_reflow(&mut self, old_layout: &TabBarLayout, duration: Duration, now: Instant) {
        let wrapper_x = old_layout.groups.iter().map(|g| (g.id, g.rect.x)).collect();
        self.active.push(Reflow {
            started_at: now,
            duration,
            wrapper_x,
        });
    }

    /// Remove animações concluídas -- chamado no `tick` da janela, junto
    /// com `warnings`/`hover`.
    pub fn tick(&mut self, now: Instant) {
        self.active.retain(|r| !r.is_finished(now));
    }

    /// Qualquer input descarta a animação em curso e aplica o estado
    /// final na hora (ADR-0022: "a animação nunca bloqueia input").
    pub fn clear(&mut self) {
        self.active.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    /// `(x_antigo, progresso)` do wrapper de `group`, se alguma animação
    /// ativa o carrega -- `None` fora de animação, caso em que quem pinta
    /// usa a posição normal do layout corrente. Quando mais de uma
    /// animação ativa carrega o mesmo grupo (raro -- colapsar de novo
    /// antes da anterior terminar), vale a mais recente.
    pub fn wrapper_progress(&self, id: GroupId, now: Instant) -> Option<(f32, f32)> {
        self.active
            .iter()
            .rev()
            .find_map(|r| r.wrapper_x.get(&id).map(|&x| (x, r.progress(now))))
    }

    /// Próximo instante em que a janela deve acordar pra continuar a
    /// animação -- um intervalo de quadro à frente de `now`, enquanto
    /// houver alguma ativa; `None` quando não há nenhuma, e o event loop
    /// volta a `ControlFlow::Wait` (ADR-0007).
    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        if self.active.is_empty() {
            None
        } else {
            Some(now + FRAME_INTERVAL)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tab_bar::{self, TabBarStyle};
    use porecatu_core::{GroupColor, Workspace};
    use porecatu_render::TextMeasurer;

    fn layout_with_two_groups() -> (TabBarLayout, GroupId, GroupId) {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let g1 = ws.group_tabs(&[a], "g1", GroupColor::Red).unwrap();
        let b = ws.new_tab(None, "bash", None, 0);
        let g2 = ws.group_tabs(&[b], "g2", GroupColor::Blue).unwrap();
        let mut m = TextMeasurer::new();
        let layout = tab_bar::layout(&ws, &TabBarStyle::DEFAULT, &mut m);
        (layout, g1, g2)
    }

    // Cenário de aceite (bug real, F3 etapa 6): colapsar um grupo do meio
    // tinha que animar o grupo depois dele -- `Workspace::group_tabs`
    // invertia a ordem de `groups` quando o grupo novo saía de um grupo
    // explícito (não implícito) já existente, o que fazia
    // `wrapper_progress` devolver a posição antiga do grupo ERRADO pra
    // cada um. Consertado em `group_tabs` (ver comentário lá); este teste
    // fecha a lacuna que deixou passar.
    #[test]
    fn collapsing_middle_group_animates_the_group_after_it() {
        let mut ws = Workspace::new();
        let a = ws.append_tab("zsh", None);
        let g1 = ws.group_tabs(&[a], "g1", GroupColor::Red).unwrap();
        let b = ws.new_tab(Some(g1), "bash", None, 1);
        let g2 = ws.group_tabs(&[b], "g2", GroupColor::Blue).unwrap();
        let c = ws.new_tab(Some(g2), "fish", None, 1);
        let g3 = ws.group_tabs(&[c], "g3", GroupColor::Green).unwrap();
        assert_eq!(
            ws.groups().iter().map(|g| g.id()).collect::<Vec<_>>(),
            [g1, g2, g3],
            "ordem dos grupos precisa ficar na ordem de criação"
        );

        let mut m = TextMeasurer::new();
        let old_layout = tab_bar::layout(&ws, &TabBarStyle::DEFAULT, &mut m);

        ws.collapse_group(g2, true);
        let new_layout = tab_bar::layout(&ws, &TabBarStyle::DEFAULT, &mut m);

        let mut clock = AnimationClock::default();
        let now = Instant::now();
        clock.start_reflow(&old_layout, Duration::from_millis(150), now);

        let (g1_old_x, _) = clock.wrapper_progress(g1, now).unwrap();
        let (g2_old_x, _) = clock.wrapper_progress(g2, now).unwrap();
        let (g3_old_x, _) = clock.wrapper_progress(g3, now).unwrap();
        assert_eq!(g1_old_x, old_layout.groups[0].rect.x);
        assert_eq!(g2_old_x, old_layout.groups[1].rect.x);
        assert_eq!(g3_old_x, old_layout.groups[2].rect.x);

        // g1 não se move (nada muda antes dele); g3 se move (g2 encolheu).
        assert_eq!(old_layout.groups[0].rect.x, new_layout.groups[0].rect.x);
        assert_ne!(old_layout.groups[2].rect.x, new_layout.groups[2].rect.x);
    }

    #[test]
    fn starts_empty() {
        let clock = AnimationClock::default();
        assert!(clock.is_empty());
        assert_eq!(clock.next_deadline(Instant::now()), None);
    }

    #[test]
    fn start_reflow_captures_wrapper_positions() {
        let (layout, g1, g2) = layout_with_two_groups();
        let g1_x = layout.groups[0].rect.x;
        let mut clock = AnimationClock::default();
        let now = Instant::now();
        clock.start_reflow(&layout, Duration::from_millis(150), now);
        assert!(!clock.is_empty());
        let (old_x, progress) = clock.wrapper_progress(g1, now).unwrap();
        assert_eq!(old_x, g1_x);
        assert_eq!(progress, 0.0);
        assert!(clock.wrapper_progress(GroupId::new(999), now).is_none());
        let _ = g2;
    }

    #[test]
    fn progress_advances_linearly_and_clamps_at_one() {
        let (layout, g1, _) = layout_with_two_groups();
        let mut clock = AnimationClock::default();
        let now = Instant::now();
        clock.start_reflow(&layout, Duration::from_millis(100), now);

        let (_, halfway) = clock
            .wrapper_progress(g1, now + Duration::from_millis(50))
            .unwrap();
        assert!((halfway - 0.5).abs() < 0.01);

        let (_, past_end) = clock
            .wrapper_progress(g1, now + Duration::from_millis(500))
            .unwrap();
        assert_eq!(past_end, 1.0);
    }

    #[test]
    fn tick_removes_finished_animations() {
        let (layout, _, _) = layout_with_two_groups();
        let mut clock = AnimationClock::default();
        let now = Instant::now();
        clock.start_reflow(&layout, Duration::from_millis(100), now);
        clock.tick(now + Duration::from_millis(50));
        assert!(!clock.is_empty());
        clock.tick(now + Duration::from_millis(150));
        assert!(clock.is_empty());
    }

    #[test]
    fn clear_discards_all_active_animations() {
        let (layout, _, _) = layout_with_two_groups();
        let mut clock = AnimationClock::default();
        let now = Instant::now();
        clock.start_reflow(&layout, Duration::from_millis(180), now);
        assert!(!clock.is_empty());
        clock.clear();
        assert!(clock.is_empty());
    }

    #[test]
    fn next_deadline_is_one_frame_ahead_while_active_and_none_when_empty() {
        let (layout, _, _) = layout_with_two_groups();
        let mut clock = AnimationClock::default();
        let now = Instant::now();
        assert_eq!(clock.next_deadline(now), None);
        clock.start_reflow(&layout, Duration::from_millis(150), now);
        assert_eq!(clock.next_deadline(now), Some(now + FRAME_INTERVAL));
        clock.tick(now + Duration::from_millis(200));
        assert_eq!(clock.next_deadline(now + Duration::from_millis(200)), None);
    }
}
