# ADR-0101 — Indicador de atualização do `pull` na barra de status: estado `Integrating`, arco girando e a exceção ao RF-9.8

**Status:** Aceito — verificado só por teste automatizado; aval visual do dono **pendente**
**Data:** 2026-11-12
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0022](0022-animacao-de-interface.md), [ADR-0048](0048-barra-de-status.md), [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md), [ADR-0094](0094-polilinha-e-area-no-render.md), [PRD-009](../prd/prd-009-barra-de-status.md), [PRD-013](../prd/prd-013-sincronizacao-com-o-remoto-do-git.md)
**Supersedes:** ADR-0052 §3 (**parcial**: a integração deixa de reaproveitar `InFlight` e ganha o estado `Integrating`) · ADR-0052 §8 (**parcial**: o segmento ganha um estado "atualizando") · PRD-013 RF-13.11 (**parcial**: a metade "nada pisca e nada se move")

## Contexto

Clicar no indicador de commits atrás do remoto dispara `git pull --ff-only` em segundo plano (RF-13.12). Enquanto o processo roda — até 20 s, o timeout do `pull` —, nada na tela mudava: o indicador continuava com o mesmo texto e a mesma cor, só deixava de aceitar clique. Quem clicava não sabia se o clique tinha pegado.

O RF-13.11 dizia "nada pisca e nada se move", apoiado no RF-9.8 (a barra não introduz timer nem redraw periódico). A regra existe para o terminal **ocioso** custar zero quadros. Um `pull` em andamento não é ocioso: foi pedido pelo usuário, dura segundos e termina sozinho.

## Decisão

**Um estado próprio `QueryState::Integrating`; enquanto ele durar, o segmento de commits vira "Atualizando repositório" com um arco girando no lugar da seta, sem alvo de clique. O quadro seguinte é agendado por `ControlFlow::WaitUntil` só enquanto algum `pull` roda.**

### 1. Estado

`git::QueryState` ganha `Integrating`. `InFlight` fica só para a consulta periódica. A regra de bloqueio é a mesma (nem segundo clique nem consulta concorrente no mesmo `.git`: `is_time_to_query` e `next_query_deadline` tratam as duas variantes igual), mas só `Integrating` mostra o indicador. `attempt_git_integration` grava `Integrating`; `apply_git_integration_result` volta a `Idle`, como antes.

### 2. Segmento

`StatusBarContent` ganha `updating: Option<String>`. Com `Some`, `layout_status_bar` coloca no mesmo lugar do indicador de commits um `SegmentRole::Updating`, na mesma largura de `em + texto`, e o indicador de commits não é desenhado. `Updating` não é alvo de `hit_test`, não tem sublinhado de hover e usa a cor `status_bar_ahead_behind` (nenhuma cor nova, os temas não mudam). O texto é a mensagem `status_bar::updating_repository` do catálogo (ADR-0056), nos cinco idiomas.

### 3. O arco

O render não tem rotação de glifo (`TextRun` não carrega transformação). O `loader-circle` do Lucide é um arco de três quartos de círculo; é redesenhado como `Primitive::Polyline` (pontas redondas, ADR-0094), de raio `0,3 em` e traço `em / 12`, centrado onde a seta estaria. `status_bar::spinner_arc_points` gera os pontos para uma fase `0..1` (uma volta); `spinner_phase(elapsed)` a calcula com período de 900 ms. As duas são puras: o tempo chega de fora (`App::git_spinner_epoch`, reescrito quando o primeiro `pull` começa, para o arco partir sempre da mesma posição).

### 4. Relógio

`App::git_spinner_deadline` devolve `now + 33 ms` **somente** se há um `pull` em andamento, a barra de status existe e `appearance.window.animations` está ligada; entra na cadeia de `schedule_next_wake` ao lado de `clock_deadline` e `tick_all` repinta as janelas. Sem thread, sem loop de render. O limite natural é o timeout de 20 s do `pull`. Com `animations = false` o arco aparece parado: o texto continua dizendo o que acontece.

### 5. Acessibilidade

`SegmentRole::Updating` é `Role::Label` (não `Button`) com o texto no valor: o leitor de tela não o anuncia como ação.

## Por que é uma exceção limitada, e não uma revisão do RF-9.8

O RF-9.8 protege o terminal ocioso. Este redraw periódico existe só durante uma operação que o usuário iniciou, com fim garantido (timeout), e some junto com ela. O custo em repouso, com nenhum `pull` rodando, continua zero: `git_spinner_deadline` devolve `None`. Qualquer outro campo da barra que mude sozinho continua precisando de avaliação contra o RF-9.8.

## Alternativas descartadas

- **Rotação real do glifo** (`TextRun` com ângulo, atlas e shader): é mudança no `porecatu-render` para um indicador de 12 px, e o arco de `Polyline` é visualmente igual.
- **Texto sem ícone girando** ("Atualizando repositório…" estático): resolve a dúvida do clique, mas não diz que o processo está vivo.
- **Reaproveitar `InFlight` com um sinalizador**: faria a consulta periódica também desenhar o indicador, ou exigiria um segundo campo para distinguir. Um estado próprio deixa o `match` exaustivo apontar todo lugar que precisa decidir.

## Consequências

- Três lugares que casam `QueryState` ganharam a variante: `is_time_to_query`, `next_query_deadline` e o teste de "não agenda nada".
- `paint_status_bar` ganhou o parâmetro `spin_phase`.
- Verificação ao vivo no Windows e aval visual do dono ficam pendentes.
