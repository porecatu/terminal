# ADR-0107 — Ações remotas sobre o workspace: o caminho do menu de contexto, alvo explícito, sem foco e sem diálogo no desktop

**Status:** Aceito em 2026-11-16 — não implementado
**Data:** 2026-11-15
**Relacionados:** [PRD-001](../prd/prd-001-abas.md), [PRD-002](../prd/prd-002-grupos-de-abas.md), [PRD-022](../prd/prd-022-abas-e-grupos-entre-janelas.md), [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md), [PRD-025](../prd/prd-025-acesso-remoto.md), [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md), [ADR-0090](0090-transferencia-de-abas-entre-janelas.md), [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md), [ADR-0105](0105-espelhamento-da-grade-e-input-remoto.md)

## Contexto

O dono quer que o celular crie e feche abas e organize grupos, inclusive entre janelas. O protocolo lista as ações ([03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md) §7). O host precisa executá-las sem abrir um segundo jeito de mexer no workspace: o projeto já pagou caro por caminhos paralelos (é por isso que `App::transfer` é **o único** caminho entre janelas, e que o gerenciador de tarefas volta ao terminal por **uma checagem só**).

Duas diferenças separam a ação remota do gesto local:

1. **O alvo é explícito.** O catálogo de ações ([ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md)) age sobre a aba ativa; o celular age sobre a aba que escolheu, sem torná-la ativa.
2. **Ninguém está olhando o desktop.** Diálogo de confirmação aberto no desktop por pedido do celular ficaria lá, esperando alguém que não está.

## Decisão

**Cada ação remota chama a mesma função de `App` que o menu de contexto chama para um alvo explícito, nunca a ação do catálogo sobre a aba ativa; nenhuma muda o foco nem a aba ativa do desktop; confirmações vão ao celular que pediu.**

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

### 2. Foco

- Nenhuma ação remota chama ativação de aba, foco de painel nem foco de janela. `new-tab` cria a aba **sem ativar**: o caminho de criação ganha esse parâmetro, e o padrão local (ativar) não muda.
- A escada de foco continua valendo quando a ação remota **remove** a aba ativa do desktop (fechar, mover para outra janela): o desktop ativa a vizinha pela regra de sempre — não é o celular mudando o foco, é a aba ativa deixando de existir.
- O gerenciador de tarefas aberto numa janela **não fecha** por causa de ação remota. A checagem única do [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) (`task_manager_guard_begin`/`end`) roda em volta dos **eventos de janela**; o evento remoto chega como `Wakeup` e não passa por ela. Aba ativa removida por ação remota com o gerenciador aberto segue a escada de foco por baixo, e o gerenciador continua aberto.

### 3. Confirmação

- O host decide se a ação precisa de confirmação **pela mesma regra do gesto local** (processo em primeiro plano, conexão SSH aberta, `confirm_close_with_process`). Precisando, manda `confirm-request` **ao canal que pediu**, com o motivo (`running-process` ou `ssh-connection`) e os nomes dos processos, e **não abre diálogo no desktop**.
- O pedido pendente é estado do `App`, chaveado por `(canal, request-id)`, com prazo de 60 s no `WaitUntil`. Sem resposta no prazo, a ação termina com `action-result { error: confirm-timeout }`; recusada, com `cancelled`. A aba alvo fechar ou mudar de janela enquanto espera não muda nada: o alvo é o id do protocolo, que é estável ([ADR-0105](0105-espelhamento-da-grade-e-input-remoto.md) §1); se ele deixou de existir, a confirmação vira `unknown-tab` (`unknown-pane` no `close-pane`).
- O canal cair com confirmação pendente cancela a ação.

### 4. Quando o desktop está ocupado

Ação sobre uma janela que está num modo de captura — diálogo modal aberto, rename inline, editor de grupo, arraste em curso, popover "Mover para janela…" — volta `busy`. O usuário do desktop no meio de um gesto não vê o chão mudar debaixo dele; o celular tenta de novo.

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
