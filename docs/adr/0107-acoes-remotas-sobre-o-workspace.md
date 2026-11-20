# ADR-0107 — Ações remotas sobre o workspace: o caminho do menu de contexto, alvo explícito, sem foco e sem diálogo no desktop

**Status:** Aceito em 2026-11-16 — implementado em 2026-11-17; §2 revisto pelo [ADR-0109](0109-o-celular-muda-a-aba-ativa-do-desktop.md) (2026-11-17); notas de implementação nas seções 1, 4 e 7 (verificação ao vivo é dívida, no [roadmap](../roadmap.md))
**Data:** 2026-11-15
**Relacionados:** [PRD-001](../prd/prd-001-abas.md), [PRD-002](../prd/prd-002-grupos-de-abas.md), [PRD-022](../prd/prd-022-abas-e-grupos-entre-janelas.md), [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md), [PRD-025](../prd/prd-025-acesso-remoto.md), [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md), [ADR-0090](0090-transferencia-de-abas-entre-janelas.md), [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md), [ADR-0105](0105-espelhamento-da-grade-e-input-remoto.md)

## Contexto

O dono quer que o celular crie e feche abas e organize grupos, inclusive entre janelas. O protocolo lista as ações ([03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md) §7). O host precisa executá-las sem abrir um segundo jeito de mexer no workspace: o projeto já pagou caro por caminhos paralelos (é por isso que `App::transfer` é **o único** caminho entre janelas, e que o gerenciador de tarefas volta ao terminal por **uma checagem só**).

Duas diferenças separam a ação remota do gesto local:

1. **O alvo é explícito.** O catálogo de ações ([ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md)) age sobre a aba ativa; o celular age sobre a aba que escolheu, sem torná-la ativa.
2. **Ninguém está olhando o desktop.** Diálogo de confirmação aberto no desktop por pedido do celular ficaria lá, esperando alguém que não está.

## Decisão

**Cada ação remota chama a mesma função de `App` que o menu de contexto chama para um alvo explícito, nunca a ação do catálogo sobre a aba ativa; nenhuma muda o foco nem a aba ativa do desktop; confirmações vão ao celular que pediu.**

> **Revisto pelo [ADR-0109](0109-o-celular-muda-a-aba-ativa-do-desktop.md) (2026-11-17, decisão do dono).** A ação remota passa a fazer a ativação de aba que o gesto local faz, e o protocolo ganha `activate-tab`. O que continua proibido é mexer na **janela do sistema** (levantar, desminimizar, dar foco) e no painel focado. Confirmações continuam indo ao celular.

### 1. Mapa

| Ação do protocolo | Caminho no host | Equivalente local |
|---|---|---|
| `new-tab` | Criação de aba do `tab.new`/`group.new_tab`, com perfil e posição, **sem ativar** (§2) | `tab.new`, item "Nova aba" do menu, menu de perfis |
| `close-tab` | Fechamento de aba do item "Fechar" do menu da aba, com a detecção do [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md) | `tab.close` |
| `close-pane` | Fechamento de painel do `pane.close`, com alvo explícito | `pane.close` |
| `rename-tab` | Renomear da aba, sem rename inline | `tab.rename` |
| `move-tab-to-group` | Operações de grupo do core (`move_tab_to_group`, `move_tab_to_new_run`) | `tab.move_to_group`, arraste |
| `move-tab-to-window`, `move-group-to-window` | **`App::transfer`** ([ADR-0090](0090-transferencia-de-abas-entre-janelas.md)) | "Mover para janela…", arraste |
| `new-group`, `rename-group`, `set-group-color`, `collapse-group`, `ungroup` | Operações de grupo do editor de grupo e do menu de grupo | `group.create`, `group.rename`, `group.set_color`, `group.toggle_collapse`, `group.dissolve` |
| `start-pane` | Início de painel não iniciado / conexão de SSH dormente pelo mesmo gatilho do clique ([ADR-0037](0037-aba-nao-iniciada.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md)) | Clique ou `Enter` no painel |

As regras de cada ação no fio — `new-tab` sem `group` nasce onde o `tab.new` criaria, com `group` no fim dele; `new-group` exige abas da mesma janela e nenhuma fixada (senão `invalid-argument`); `close-pane` do último painel fecha a aba — estão na [§7 da especificação](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md), e o caminho local tem de produzi-las. Todo resultado, sucesso ou erro, volta em `action-result` com o `id` do pedido.

Quando a implementação achar uma dessas operações escrita só "sobre a aba ativa", a correção é **extrair** a função com alvo explícito e fazer o caminho local chamá-la também — nunca uma cópia para o remoto.

> **Revisto pelo [ADR-0113](0113-restaurar-sessao-nomeada-pelo-celular.md) (2026-11-20; implementado no mesmo dia, verificado só por teste automatizado).** O mapa ganha uma linha: `restore-session` (especificação 0.6 do protocolo) chama a mesma restauração do popover de sessões (`restore_named_session`, dividida em ler e aplicar), com o destino pela chave `[session] restore_named_in_current_window`. A janela de origem do gesto local é substituída por parâmetros explícitos: a janela nova nasce com `WindowActivation::Background`, e o acréscimo usa `AppendRules` sem ativar e sem fechar. O equivalente local é o clique numa linha do popover de sessões. O pedido `list-sessions` não é ação: o `App` responde com a mesma `list_named()` do popover.

> **Nota de implementação (2026-11-17).** As decisões puras (o que a ação faz, sem janela) moram em `porecatu-ui/src/remote/actions.rs` e as funções com `&mut App` em `porecatu-ui/src/app_remote.rs` (`remote_new_tab`, `remote_close_tab`, `remote_close_pane`, `remote_rename_tab`, `remote_start_pane`, `remote_move_tab_to_group`, `remote_new_group`, `remote_activate_tab` e as demais, uma por ação). Casos que a especificação do protocolo deixava abertos, decididos pelo dono em 2026-11-17: **`close-pane` no último painel de uma aba fixada é o `close-tab` da fixada** — confirma no celular, fecha e desafixa, como no desktop (o texto "painel reiniciado" da §7 da especificação 0.3 estava errado; a 0.4 o corrige); **`rename-tab` em aba fixada é aceito**, e o título vale só para a execução (não vai para `pinned.json`); um **grupo de outra janela** dá `unknown-group` em `new-tab` e em `move-tab-to-group`; **colapsar o grupo da aba ativa pelo celular é permitido** (a escada de foco do desktop age, como no gesto local). `close-pane` e `close-tab` da última aba da última janela dão `last-tab`, como na §5.

### 2. Foco

> **Revisto pelo [ADR-0109](0109-o-celular-muda-a-aba-ativa-do-desktop.md) (2026-11-17).** Os três itens abaixo deixam de valer como escritos. `new-tab` ativa a aba criada, como o `tab.new`. Expandir grupo e mover para outra janela ativam o que o gesto local ativa. `activate-tab` troca a aba ativa da janela. Nenhuma ação dá foco, levanta ou desminimiza janela do sistema. Com o gerenciador de tarefas aberto, a ativação ou criação remota volta ao terminal pela mesma checagem única do ADR-0095.

> **Restabelecido pelo [ADR-0111](0111-o-celular-nao-muda-a-aba-ativa-do-desktop.md) (2026-11-20).** O ADR-0109 foi substituído: os três itens abaixo voltam a valer como escritos. A única adição é que o celular informa a aba que vê (`view-tab`, especificação 0.5 do protocolo), e o desktop só a indica na barra ([ADR-0112](0112-indicador-de-aba-vista-pelo-celular.md)), sem ativá-la.

> **Revisto pelo [ADR-0113](0113-restaurar-sessao-nomeada-pelo-celular.md) (2026-11-20; implementado no mesmo dia, verificado só por teste automatizado).** `restore-session` é a primeira ação remota que pode **criar uma janela do sistema**. Ela nasce sem foco (`with_active(false)`, `SW_SHOWNOACTIVATE` no Windows) e vai para o **fim** da ordem de foco (`FocusOrder::push_back`), nunca para o topo. No acréscimo a uma janela existente, a aba ativa não muda e a trilha não rola. Os três itens abaixo continuam valendo sem exceção.

- Nenhuma ação remota chama ativação de aba, foco de painel nem foco de janela. `new-tab` cria a aba **sem ativar**: o caminho de criação ganha esse parâmetro, e o padrão local (ativar) não muda.
- A escada de foco continua valendo quando a ação remota **remove** a aba ativa do desktop (fechar, mover para outra janela): o desktop ativa a vizinha pela regra de sempre — não é o celular mudando o foco, é a aba ativa deixando de existir.
- O gerenciador de tarefas aberto numa janela **não fecha** por causa de ação remota. A checagem única do [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) (`task_manager_guard_begin`/`end`) roda em volta dos **eventos de janela**; o evento remoto chega como `Wakeup` e não passa por ela. Aba ativa removida por ação remota com o gerenciador aberto segue a escada de foco por baixo, e o gerenciador continua aberto.

### 3. Confirmação

- O host decide se a ação precisa de confirmação **pela mesma regra do gesto local** (processo em primeiro plano, conexão SSH aberta, `confirm_close_with_process`). Precisando, manda `confirm-request` **ao canal que pediu**, com o motivo (`running-process` ou `ssh-connection`) e os nomes dos processos, e **não abre diálogo no desktop**.
- O pedido pendente é estado do `App`, chaveado por `(canal, request-id)`, com prazo de 60 s no `WaitUntil`. Sem resposta no prazo, a ação termina com `action-result { error: confirm-timeout }`; recusada, com `cancelled`. A aba alvo fechar ou mudar de janela enquanto espera não muda nada: o alvo é o id do protocolo, que é estável ([ADR-0105](0105-espelhamento-da-grade-e-input-remoto.md) §1); se ele deixou de existir, a confirmação vira `unknown-tab` (`unknown-pane` no `close-pane`).
- O canal cair com confirmação pendente cancela a ação.

### 4. Quando o desktop está ocupado

Ação sobre uma janela que está num modo de captura — diálogo modal aberto, rename inline, editor de grupo, arraste em curso, popover "Mover para janela…" — volta `busy`. O usuário do desktop no meio de um gesto não vê o chão mudar debaixo dele; o celular tenta de novo.

> **Nota de implementação (2026-11-17).** O predicado é `App::remote_window_busy`, e os modos de captura que contam são: diálogo, menus de contexto, editor de grupo, popovers (**inclusive o de aparelhos conectados**), rename inline, arraste de aba e arraste entre janelas. A **barra de busca não conta**: ela captura teclado, mas não é um gesto que mexa na estrutura do workspace.

### 5. A última aba

Fechar, pelo celular, a última aba da última janela encerraria o Porecatu e, com ele, o próprio acesso. O host recusa com `last-tab`, venha o pedido por `close-tab` ou por `close-pane` no último painel dela ([03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md) §7 e §9; o app trata o código no RF-4.11 do [PRD-004 do Porecatu Remote](https://github.com/porecatu/remote/blob/main/docs/prd/prd-004-gestao-de-abas.md)). Mover a última aba de uma janela para outra continua permitido (a origem vazia fecha sem diálogo, como no arraste).

### 6. Perguntas do backend

Senha, passphrase e chave de servidor desconhecida, que um `start-pane` de SSH pode provocar, abrem **no desktop**, pelas superfícies do [ADR-0075](0075-superficies-de-ui-do-ssh.md). Enquanto a pergunta espera resposta, o painel aparece ao celular no estado `awaiting-desktop` ([03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md) §4.1); depois, `connecting` e `running` como de costume. Segredo não trafega para outro aparelho no v1.

### 7. Ações novas no catálogo

Entram em `docs/reference/acoes.md` **no PR de implementação**, junto com o `Action` correspondente (o catálogo é a enumeração do que existe em `porecatu-core`):

| Ação | O que faz | Atalho |
|---|---|---|
| `remote.toggle` | Liga ou desliga o acesso remoto, gravando `[remote] enabled` pelo `porecatu-config::edit` | — |
| `remote.pair` | Abre a tela de configurações no grupo "Acesso remoto" com o QR já gerado | — |
| `remote.disconnect_all` | Desconecta todos os aparelhos, sem revogar | — |

Nenhuma mira painel: `Action::targets_pane` as classifica como `false`, e o teste sobre o `CATALOG` inteiro do gerenciador de tarefas reprova se faltar a classificação.

> **Nota de implementação (2026-11-17).** As três têm efeito. `remote.toggle` grava `[remote] enabled` por `porecatu-config::edit` (o hot reload aplica; a gravação usa `app_key_edit.rs`, o gravador de chave única extraído de `save_background_monitoring`). `remote.pair` abre a tela de configurações no grupo "Acesso remoto" e, **com o relay conectado**, já pede o QR; sem conexão, a tela abre com o botão esmaecido e a razão. `remote.disconnect_all` desconecta todos os aparelhos sem revogar. As três ficam despachadas com o gerenciador de tarefas aberto.

## Alternativas consideradas

### Despachar as ações do catálogo depois de ativar o alvo

Ativar a aba, despachar `tab.close`, voltar o foco. Reaproveita o despacho, mas muda o foco do desktop por um instante (e para sempre, se a aba ativa sumir), dispara a checagem do gerenciador de tarefas e anima a barra. Recusada.

### Confirmação no desktop

Mantém um lugar só para confirmar, mas o diálogo ficaria esperando alguém que não está, e a resposta não chegaria ao celular. Recusada.

### Fechar a última aba encerrando o app

Fiel ao gesto local, mas o celular derrubaria o próprio acesso, sem como reabrir (abrir janela pelo celular está fora do v1).

## Consequências

### Positivas

- Nenhuma regra de aba, grupo ou janela escrita duas vezes.
- O usuário do desktop não perde o foco nem vê diálogo aparecer por causa do celular.

### Negativas

- Algumas operações ganham parâmetro de alvo ou de "não ativar", o que mexe em código que hoje só conhece a aba ativa.
- `busy` empurra para o celular a decisão de tentar de novo.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Uma ação remota ganhar caminho próprio por conveniência | Média | Alto | Tabela do §1 como contrato; revisão de PR confere que o caminho local chama a mesma função |
| Ação remota com o desktop no meio de um gesto corromper estado | Baixa | Alto | `busy` em todo modo de captura (§4); teste por modo |
| Pedido de confirmação órfão | Baixa | Baixo | Prazo de 60 s; cancelado quando o canal cai |
