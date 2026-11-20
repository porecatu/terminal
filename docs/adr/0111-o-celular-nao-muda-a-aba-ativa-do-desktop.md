# ADR-0111 — O celular não muda a aba ativa do desktop

**Status:** Aceito em 2026-11-20 — implementado em 2026-11-20 (verificado só por teste automatizado; verificação ao vivo é dívida, no [roadmap](../roadmap.md))
**Data:** 2026-11-20
**Supersedes:** ADR-0109 (inteiro) · restabelece o [ADR-0107](0107-acoes-remotas-sobre-o-workspace.md) §2 · PRD-025 RF-25.27 e RF-25.28 voltam ao texto original
**Relacionados:** [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md), [PRD-025](../prd/prd-025-acesso-remoto.md), [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md), [ADR-0103](0103-acesso-remoto-tres-partes-e-protocolo-externo.md), [ADR-0107](0107-acoes-remotas-sobre-o-workspace.md), [ADR-0108](0108-superficies-de-ui-do-acesso-remoto.md), [ADR-0109](0109-o-celular-muda-a-aba-ativa-do-desktop.md), [ADR-0112](0112-indicador-de-aba-vista-pelo-celular.md); no protocolo, [ADR-0011](https://github.com/porecatu/protocol/blob/main/docs/adr/0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md) e a [especificação 0.5](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md)

## Contexto

O [ADR-0109](0109-o-celular-muda-a-aba-ativa-do-desktop.md) deixou o celular mudar a aba ativa do desktop. As ações remotas passaram a fazer a ativação do gesto local (`new-tab`, expandir grupo, `move-*-to-window`, `start-pane`), e entrou a ação `activate-tab`. O app Remote manda essa ação sozinho cada vez que o usuário abre uma aba no terminal do celular (`activateOnce()`, em `src/controllers/terminalSession.ts` do Remote).

O dono do produto viu o resultado e decidiu desfazer: **abrir uma aba no celular para olhar não pode trocar a aba de quem está no desktop**. A troca acontece num gesto de leitura, debaixo do teclado de outra pessoa, e era exatamente o risco que o ADR-0107 §2 evitava. O pedido vai além do gesto de abrir: nenhuma ação remota volta a ativar aba ("reverter tudo").

O que o desktop ganha no lugar é **saber** qual aba o celular está vendo, e mostrar isso na barra. Isso é o [ADR-0112](0112-indicador-de-aba-vista-pelo-celular.md), que depende da mensagem `view-tab` da especificação 0.5 do protocolo.

## Decisão

**Nenhuma ação remota muda a aba ativa do desktop. O ADR-0107 §2 volta a valer como escrito, e `activate-tab` sai do host junto com a especificação 0.5 do protocolo.**

### 1. Ações sem ativação

| Ação | O que acontece com a aba ativa | Diferença para o gesto local |
|---|---|---|
| `new-tab` | A aba nasce **sem ativar**; a ativa continua a mesma | O `tab.new` ativa |
| `collapse-group{collapsed: false}` | O grupo expande, e a aba ativa continua a mesma (o caminho restaura a ativa que o core trocaria) | O clique na pílula ativa o que o core escolhe |
| `move-tab-to-window`, `move-group-to-window` | Chega ao destino **sem ativar**. Na origem, se a aba ativa saiu, vale a escada de foco | O arraste e o "Mover para janela…" ativam no destino |
| `start-pane` | O painel sobe (ou o SSH dormente conecta) **sem ativar** a aba | No desktop, a aba não iniciada sobe ao ser ativada |
| `close-tab`, `close-pane`, `collapse-group{collapsed: true}` | Se a aba ativa sai ou some, a vizinha é ativada pela escada de foco | Nenhuma: não é o celular escolhendo, é a ativa deixando de estar ali |
| `move-tab-to-group`, `new-group`, `rename-*`, `set-group-color`, `ungroup` | Nada muda | Nenhuma |

O caminho continua sendo o do ADR-0107 §1, a mesma função do gesto local com alvo explícito. Nos pontos em que o ADR-0109 fez o caminho remoto pedir a ativação, ele volta a pedir "não ativar". Cada caso, no código atual:

- `new-tab`: o parâmetro `activate` de `WindowState::open_tab` ficou no código (a restauração de sessão o usa), e a volta é chamá-lo com `false`.
- `move-*-to-window`: o `App::transfer` só tem as chegadas `TransferArrival::Take` (gesto local) e `ActivateOnly` (a do ADR-0109). Entra uma terceira chegada, que insere no destino **sem ativar** e mantém a ativa do destino, e é ela que o caminho remoto usa. A origem segue a escada de foco como sempre.
- `start-pane`: em vez de `activate_tab_by_click` mais `ensure_active_tab_started`, a aba não ativa sobe por `WindowState::start_tab`, a função que já inicia uma aba pelo id.
- `collapse-group{collapsed: false}`: guarda a aba ativa antes e a restaura se a expansão a trocou.

### 2. `activate-tab` sai

A ação deixa de existir no fio (o código 13 de `action-body` fica reservado na especificação 0.5). O host recusa o código 13 como faz com qualquer valor fora do domínio, porque o crate do protocolo nem o decodifica mais. Saem:

- `App::remote_activate_tab` e a rota `ActionBody::ActivateTab` em `apply_remote_action` (`porecatu-ui/src/app_remote.rs`);
- `plan_activate_tab` e `ActivateStep` (`porecatu-ui/src/remote/actions.rs`), com os testes deles.

### 3. Gerenciador de tarefas

Volta o terceiro item do ADR-0107 §2: o gerenciador de tarefas aberto numa janela **não fecha** por causa de ação remota. A ação remota sai da checagem única do [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md). Saem o embrulho `task_manager_guard_begin` em `apply_remote_request` e o `task_manager_guard_end_remote` (`app_task_manager.rs`). Uma aba ativa removida por ação remota com o gerenciador aberto segue a escada de foco por baixo, e o gerenciador continua aberto.

### 4. O que não muda

- A janela do sistema continua intocada: nenhuma ação remota dá foco, levanta ou desminimiza janela (ADR-0109 §3, que já era regra antes dele).
- O painel focado não é mudado pelo celular.
- A confirmação vai ao celular, sem diálogo no desktop.
- `busy`, `last-tab` e as perguntas do backend seguem o ADR-0107 §3 a §6.
- O celular não muda o tamanho do PTY (ADR-0105).

### 5. Protocolo

A mudança começou no `porecatu-protocol`, como manda o [ADR-0103](0103-acesso-remoto-tres-partes-e-protocolo-externo.md) §5: ADR-0011 e especificação 0.5, com o fio ainda `[0, 1]`. O `porecatu-remote` passa a apontar para o commit da 0.5 quando o crate a implementar, e o Relay sobe o mesmo `rev`. O fio `[0, 1]` não distingue a 0.4 da 0.5: o handshake passa, e a sessão cai com erro de protocolo na primeira mensagem que o outro lado não conhece (`view-tab` num host 0.4, `activate-tab` num host 0.5). Por isso os três sobem juntos.

> **Nota de implementação (2026-11-20).**
> - `new-tab`: `open_tab(.., false)`; o processo sobe do mesmo jeito (`spawn_tab_runtime`), e o título da janela e a rolagem da barra não seguem a aba nova.
> - `move-*-to-window`: a chegada nova é `TransferArrival::Passive` (`app_transfer.rs`), que insere no destino sem ativar nada e sem tocar a janela do SO; `ActivateOnly` ficou sem uso e saiu. A decisão de qual aba ativar no destino é a função pura `arrival_target`, que devolve `None` para `Passive`.
> - `start-pane`: `WindowState::start_tab` na aba do painel, sem `activate_tab_by_click`.
> - `collapse-group{collapsed: false}`: em vez de expandir pelo `Workspace::collapse_group` e restaurar a ativa depois (o que ainda apagava os indicadores da primeira aba do grupo e mudava o `last_active` dele), o core ganhou `Workspace::expand_group_in_place`, que só tira o colapso. `WindowState::set_group_collapsed` recebe `visit: bool`; o caminho remoto passa `false` (expande no lugar, anima o reflow e não rola a barra) e o gesto local passa `true` (sem mudança).
> - O embrulho da checagem do gerenciador de tarefas saiu de `apply_remote_request`, e `task_manager_guard_end_remote` foi removida.
> - `remote_activate_tab`, `plan_activate_tab` e `ActivateStep` saíram junto com a subida do `porecatu-protocol` para `4ce7e4f`, o commit da especificação 0.5 na `main` do protocolo.
> - Não há teste de `WindowState` sem GPU e janela: os testes fixam as decisões puras e o modelo (`remote/actions.rs`, `app_transfer.rs`, `porecatu-core`). A ausência de rolagem, a animação da expansão e o gerenciador de tarefas aberto não foram vistos rodando.

## Alternativas consideradas

### Só o app parar de mandar `activate-tab` ao abrir a aba

Seria a menor mudança: o protocolo e o host ficariam como estão. Recusada pelo dono. O problema é o celular mudar o desktop, e criar, expandir ou mover pelo celular também ativava abas que ninguém no desktop pediu.

### Manter `activate-tab` para um botão explícito no app

Daria ao celular uma forma de "deixar pronta" a aba ao voltar para a mesa. Recusada: nenhuma UI a usaria agora, e o indicador do ADR-0112 resolve o "onde eu estava" sem mudar o desktop.

## Consequências

### Positivas

- Abrir, criar ou mover uma aba pelo celular nunca troca a aba sob o teclado de quem está no desktop.
- O gerenciador de tarefas de quem o está usando não fecha por causa do celular.

### Negativas

- As ações remotas voltam a divergir do gesto local na ativação, e cada uma carrega o "não ativar".
- Quem volta para a mesa não encontra ativa a aba em que trabalhou pelo celular. O indicador do ADR-0112 mostra qual é, mas é preciso clicar.
- Uma aba criada pelo celular com um gerenciador de tarefas aberto fica fora da vista até alguém voltar ao terminal.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Algum caminho remoto continuar ativando aba (o "não ativar" esquecido numa das ações) | Média | Médio | Um teste por ação em `remote/actions.rs` e `app_remote.rs` fixa o `active` antes e depois, como o ADR-0107 pedia antes do 0109 |
| App 0.4 conversar com host 0.5, ou o contrário (o fio `[0, 1]` não os distingue) | Média | Médio | A sessão cai com erro de protocolo, sem efeito no workspace; host, Relay e Remote sobem o `rev` juntos |
