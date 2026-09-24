# PRD-014 — Sessões nomeadas

**Status:** Aprovado
**Data:** 2026-09-24
**Requisito de origem:** pedido direto do dono do produto, sobre a persistência de sessão já em uso — e a ideia que o [PRD-003](prd-003-persistencia-de-sessao.md) deixou escrita em "Fora de escopo": *"Múltiplas sessões nomeadas, salvas e alternáveis (ideia para v2)"*
**Relacionados:** [ADR-0054](../adr/0054-sessoes-nomeadas.md), [ADR-0055](../adr/0055-botao-e-popover-de-sessoes.md), [ADR-0005](../adr/0005-persistencia-de-sessao.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0015](../adr/0015-multiplas-janelas.md), [ADR-0036](../adr/0036-formato-do-arquivo-de-sessao.md), [ADR-0037](../adr/0037-aba-nao-iniciada.md), [ADR-0051](../adr/0051-arquivo-de-projeto-porecatu.md), [ADR-0053](../adr/0053-paineis-divididos.md), [PRD-003](prd-003-persistencia-de-sessao.md), [PRD-006](prd-006-paineis-divididos.md), [PRD-012](prd-012-comando-de-projeto-por-diretorio.md)

> Aprovado em 2026-09-24, por decisão do dono do produto, **fora da ordem de fases** — como o arquivo de projeto ([PRD-012](prd-012-comando-de-projeto-por-diretorio.md)) e a sincronização com o remoto ([PRD-013](prd-013-sincronizacao-com-o-remoto-do-git.md)). É a promoção de uma ideia que o PRD-003 marcou como v2, não um rascunho promovido: nenhum documento a descrevia além daquela linha. Como gravar, onde, com que formato e como a janela nova nasce está no [ADR-0054](../adr/0054-sessoes-nomeadas.md); o botão e o popover que a apresentam, no [ADR-0055](../adr/0055-botao-e-popover-de-sessoes.md).

## Problema

A sessão automática do [PRD-003](prd-003-persistencia-de-sessao.md) responde a uma pergunta: **"onde eu parei?"**. Ela guarda o último estado e o devolve no arranque. Não responde à pergunta vizinha, que é a de quem alterna entre contextos de trabalho: **"como eu monto o ambiente do projeto X?"**.

Hoje o usuário tem três caminhos, e nenhum é bom:

- **Remontar à mão.** Abrir as abas, entrar em cada diretório, dividir os painéis, agrupar, renomear. É exatamente o trabalho que a sessão automática existe para poupar — só que ela poupa uma vez, e o layout do projeto X se perde assim que o usuário passa uma tarde no projeto Y.
- **Manter uma janela aberta por projeto, para sempre.** Funciona até o número de projetos passar de três; depois vira a barra de tarefas cheia de janelas cujo conteúdo ninguém lembra.
- **Editar o `session.json` à mão**, copiando-o de lado e de volta. Funciona, e é justamente o tipo de operação que o [ADR-0005](../adr/0005-persistencia-de-sessao.md) considerou fora do contrato: o arquivo é estado da máquina, não do usuário, e o app o sobrescreve a cada dois segundos.

O `.porecatu` ([PRD-012](prd-012-comando-de-projeto-por-diretorio.md)) já fechou metade do laço: uma aba restaurada num diretório autorizado sobe o servidor, o watcher, o que o projeto precisar. O que falta é a outra metade — **uma forma de pedir "a disposição do projeto X" quando se quer, e não só no arranque**.

## O que já existe e não muda

O pedido tinha três partes. A primeira — *"o usuário escolhe na config se a sessão é gravada e restaurada automaticamente, default ligado"* — **já está implementada**: é o RF-3.6 do [PRD-003](prd-003-persistencia-de-sessao.md), a chave `[session] enabled = true` ([ADR-0036](../adr/0036-formato-do-arquivo-de-sessao.md) §6). Com `false`, nada é gravado e o app sempre abre com uma aba limpa. Este documento **não a toca** e a cita só para fixar a relação entre as duas coisas (RF-14.14).

## Usuário-alvo

O mesmo do [PRD-000](prd-000-visao-de-produto.md), no caso em que o valor dele é maior: quem trabalha em **vários projetos, cada um com uma disposição própria** de abas, grupos e painéis — e que alterna entre eles ao longo da semana, não ao longo do dia.

**Não é para** quem trabalha sempre no mesmo lugar: para esse, a sessão automática já é tudo. Quem não usar o recurso não vê nada além de um botão na zona fixa da barra.

## Em uma tela

Na zona fixa à direita da barra de abas, um botão novo ao lado da engrenagem:

```
  … trilha de abas …                      [▣] [⚙]   ─  □  ✕
                                           └── sessões salvas
```

Clicado, ele abre um popover ancorado abaixo dele:

```
                           ┌──────────────────────────────┐
                           │ + Salvar esta janela…  Ctrl+Shift+S
                           │ ──────────────────────────── │
                           │   api + front          ✕     │
                           │   infra                ✕     │
                           │   estudo rust          ✕     │
                           └──────────────────────────────┘
```

Clicar numa linha abre **uma janela nova** com a disposição salva. O `✕` da linha exclui, com confirmação. "Salvar esta janela…" vira um campo de nome no lugar do item.

## Requisitos funcionais

### Salvar

**RF-14.1** — O usuário salva **a janela em que está** como uma sessão nomeada. Só ela: as demais janelas abertas não entram. O gesto existe por dois caminhos — o item "Salvar esta janela…" do popover (RF-14.7) e a ação `session.save_named`, com default `Ctrl+Shift+S` (`Cmd+Shift+S` no macOS).

**RF-14.2** — Salvar pede um nome num campo de texto de uma linha, com foco automático, que ocupa o lugar do item "Salvar esta janela…" dentro do popover. A ação por tecla abre o popover já nesse estado. `Enter` confirma; `Esc` cancela e não grava nada.

**RF-14.3** — O nome é texto livre, com espaços e acentos. Espaço nas pontas é aparado; nome vazio depois disso **não confirma** — o campo continua aberto, sem aviso. O nome tem um teto de comprimento em caracteres ([ADR-0054](../adr/0054-sessoes-nomeadas.md) §3) — o campo para de aceitar texto nele, sem aviso — e caractere que não vale em nome de arquivo **não é recusado**: o nome não é o nome do arquivo.

**RF-14.4** — O que é gravado é o **mesmo conjunto** que a sessão automática grava para uma janela ([ADR-0036](../adr/0036-formato-do-arquivo-de-sessao.md) §3, revisto pelo [ADR-0053](../adr/0053-paineis-divididos.md) §11): grupos com nome, cor e colapso; abas com título customizado, `cwd` e programa; a árvore de painéis de cada aba; a aba ativa; o tamanho da janela. O que ela **não** grava continua fora pelas mesmas razões — processos, scrollback, histórico.

**RF-14.5** — Nome que já existe (comparado sem distinguir maiúsculas de minúsculas, depois de aparado) abre o **diálogo de confirmação** ([ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md)) antes de sobrescrever, com o foco inicial no cancelar, como todo diálogo do app. Cancelar devolve ao campo com o nome digitado.

**RF-14.6** — A gravação é atômica, como a da sessão automática (RF-3.5): um crash no meio preserva a versão anterior. Gravação que falha (disco cheio, permissão) vira **aviso** do app com a razão — nunca silêncio.

### Listar e restaurar

**RF-14.7** — Um botão novo na zona fixa da barra de abas, **à esquerda do botão de configurações**, abre um popover com o item "Salvar esta janela…" no topo e, abaixo de um divisor, a lista das sessões nomeadas, **a mais recentemente salva primeiro**. A anatomia é do [ADR-0055](../adr/0055-botao-e-popover-de-sessoes.md).

**RF-14.8** — A lista rola quando passa de um teto de linhas visíveis, como o popover de grupo de destino (RF-2.20). Nome longo trunca com reticências e mostra o nome inteiro no tooltip (RF-1.10).

**RF-14.9** — Sem nenhuma sessão salva, o popover mostra só o item de salvar e, abaixo dele, uma linha esmaecida dizendo que não há sessões salvas. Não há estado em que o botão abra um popover vazio.

**RF-14.10** — Clicar numa sessão (ou realçá-la pelas setas e dar `Enter`) **abre uma janela nova** com aquela disposição e fecha o popover. A janela de onde o gesto partiu **não muda** — nem abas, nem foco de aba, nem geometria.

**RF-14.11** — A janela restaurada se comporta **exatamente como uma janela restaurada no arranque**:
- cada aba (cada painel) sobe no `cwd` salvo; `cwd` que não existe mais cai no diretório inicial com uma nota na aba (RF-3.10);
- a árvore de painéis volta com as proporções salvas (RF-6.22, RF-6.23);
- `[session] lazy_restore` vale igual (RF-3.8): com `true`, só a aba ativa sobe e as outras no primeiro foco, com o rótulo esmaecido do RF-3.9;
- o `.porecatu` do `cwd` de cada aba roda como em qualquer aba **restaurada** (RF-12.x): só em diretório declarado em `[project_file] trusted_paths`, uma vez por aba, no painel focado (RF-6.24); fora da lista, a nota de "não autorizado".

**RF-14.12** — A janela nova abre com o **tamanho** salvo, na posição em cascata a partir da janela de onde o gesto partiu (o mesmo deslocamento de `window.new`), no monitor dela. Não herda a posição nem o monitor gravados: abrir uma sessão não pode derrubar uma janela em cima da outra, nem num monitor que o usuário não está olhando.

**RF-14.13** — Tema e zoom gravados junto com a janela **não** são aplicados na restauração nomeada. São estado do processo, e trocar o tema de todas as janelas abertas por ter aberto uma sessão seria efeito colateral fora do alvo do gesto.

**RF-14.14** — Sessões nomeadas funcionam **com `[session] enabled` ligado ou desligado**. As duas coisas são independentes:
- com `enabled = true`, a janela restaurada passa a fazer parte da sessão automática como qualquer outra janela aberta, e é gravada no `session.json` a partir daí;
- com `enabled = false`, a sessão automática continua sem gravar nada, e as nomeadas seguem gravadas e restauradas quando o usuário pede.

**RF-14.15** — Uma sessão nomeada é **retrato, não vínculo**. Depois de restaurada, o que o usuário muda na janela nova não altera a sessão salva; para atualizá-la, ele salva de novo com o mesmo nome (RF-14.5).

### Excluir

**RF-14.16** — Cada linha da lista tem um botão de excluir, visível sob o cursor e na linha realçada pelo teclado (`Delete` exclui a realçada). Excluir abre o diálogo de confirmação com o nome da sessão no corpo; confirmado, o arquivo é removido e a linha some da lista, com o popover continuando aberto.

### Arquivos

**RF-14.17** — Arquivo de sessão nomeada ilegível ou inválido **não derruba a lista**: a linha aparece esmaecida, não restaura, e o arquivo é preservado para exame com a mesma política do RF-3.14 (`.corrupt`, nunca sobrescrito). Arquivo com `schema_version` mais nova que a suportada aparece esmaecido e **nunca é sobrescrito** (RF-3.16), nem mesmo por um salvar com o mesmo nome — esse caso vira aviso.

**RF-14.18** — O app só lê o diretório das sessões nomeadas **ao abrir o popover** e ao salvar ou excluir. Nada é lido no arranque, e nada é vigiado em segundo plano: o tempo até o primeiro prompt (métrica do [PRD-000](prd-000-visao-de-produto.md)) não muda.

## Cenários

```gherkin
Cenário: salvar a janela com um nome
  Dado uma janela com dois grupos e cinco abas, uma delas dividida em dois painéis
  Quando o usuário tecla Ctrl+Shift+S, digita "api + front" e confirma
  Então uma sessão "api + front" aparece no topo da lista do popover
  E as outras janelas abertas não entram nela

Cenário: restaurar abre uma janela nova
  Dado a sessão "api + front" salva
  Quando o usuário clica nela no popover
  Então uma janela nova abre com os dois grupos, as cinco abas e os dois painéis
  E a janela de onde ele clicou continua exatamente como estava

Cenário: o .porecatu roda como na restauração do arranque
  Dado a sessão "api + front" com uma aba em ~/Projetos/api
  E ~/Projetos/api com um .porecatu e declarado em trusted_paths
  Quando o usuário restaura a sessão
  Então o comando do .porecatu é escrito no prompt daquela aba

Cenário: diretório não autorizado continua não autorizado
  Dado a sessão com uma aba num diretório com .porecatu fora de trusted_paths
  Quando o usuário restaura a sessão
  Então nada é executado
  E a aba mostra a nota de arquivo de projeto não autorizado

Cenário: funciona com a sessão automática desligada
  Dado [session] enabled = false
  Quando o usuário salva e depois restaura uma sessão nomeada
  Então a janela nova abre com a disposição salva
  E o session.json continua sem ser gravado

Cenário: nome repetido pede confirmação
  Dado a sessão "infra" salva
  Quando o usuário salva a janela atual como "Infra"
  Então um diálogo pergunta se sobrescreve a sessão existente
  E cancelar volta ao campo com "Infra" digitado

Cenário: excluir pede confirmação
  Dado a sessão "estudo rust" salva
  Quando o usuário clica no ✕ da linha e confirma
  Então a linha some da lista
  E o popover continua aberto

Cenário: a sessão é retrato, não vínculo
  Dado a sessão "infra" restaurada numa janela nova
  Quando o usuário fecha duas abas dessa janela
  E reabre o popover e restaura "infra" de novo
  Então a segunda janela nova tem as abas originais

Cenário: diretório que sumiu
  Dado a sessão com uma aba num diretório que foi apagado
  Quando o usuário restaura a sessão
  Então a aba abre no diretório inicial
  E mostra uma nota dizendo que o diretório salvo não existe mais

Cenário: lista vazia
  Dado nenhuma sessão salva
  Quando o usuário clica no botão de sessões
  Então o popover mostra "Salvar esta janela…" e a linha "nenhuma sessão salva"
```

## Fora de escopo

Cada item é decisão, não esquecimento.

- **Salvar todas as janelas de uma vez.** O gesto é sobre "esta disposição", e a restauração é "uma janela nova"; um retrato de N janelas pediria restaurar N janelas, com N geometrias, e a pergunta "em cima de quais?" que o RF-14.12 existe para não ter.
- **Renomear uma sessão salva.** Salvar de novo com outro nome e excluir a antiga cobre o caso com os gestos que já existem.
- **Restaurar dentro da janela atual**, substituindo ou acrescentando abas. Substituir descartaria processos vivos; acrescentar misturaria dois contextos numa barra, que é o que o recurso existe para separar.
- **Abrir uma sessão nomeada pela linha de comando** (`porecatu --session <nome>`). Ideia plausível, registrada; a superfície de CLI do [ADR-0040](../adr/0040-superficie-de-linha-de-comando.md) é pequena de propósito, e entrar nela pede requisito próprio.
- **Sessão nomeada como sessão de arranque**, no lugar da automática. Idem.
- **Atualizar a sessão salva sozinho** enquanto a janela restaurada muda (RF-14.15).
- **Ordenar, filtrar, buscar na lista.** A lista é do tamanho do número de projetos do usuário, não de abas.
- **Exportar, importar, sincronizar entre máquinas.** Os arquivos estão num diretório conhecido ([ADR-0054](../adr/0054-sessoes-nomeadas.md) §2), e copiá-los à mão é o suficiente — o `cwd` gravado é da máquina que os gravou.
- **Ação de catálogo para restaurar uma sessão específica.** Exigiria argumento (qual sessão?), e ação com argumento não é vinculável a tecla ([docs/reference/acoes.md](../reference/acoes.md), Convenções).

### O que este documento **não** contradiz

O [ADR-0005](../adr/0005-persistencia-de-sessao.md) decidiu que a gravação é automática, e o catálogo de ações registrou `session.save` / `session.restore` entre as ações que **não existem**, com a razão *"ação manual sugeriria que não é"*. As duas coisas continuam verdadeiras para a sessão automática: ela segue sem gesto manual, e o `session.json` segue sendo estado da máquina. O que entra aqui é um objeto diferente — um retrato **nomeado pelo usuário**, num arquivo à parte, que a sessão automática nunca lê nem escreve. O [ADR-0054](../adr/0054-sessoes-nomeadas.md) revisa a linha do catálogo em vez de apagá-la em silêncio.

A proibição do [PRD-003](prd-003-persistencia-de-sessao.md) de reexecutar processos também continua inteira: o que roda numa sessão nomeada restaurada é o `.porecatu`, declarado pelo usuário e autorizado por diretório — a mesma distinção que a emenda do PRD-003 já registrou para o [PRD-012](prd-012-comando-de-projeto-por-diretorio.md).

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Gestos para remontar a disposição salva de um projeto | **dois** (abrir o popover, clicar no nome) |
| Janelas já abertas alteradas por restaurar uma sessão nomeada | **zero** |
| Diferenças de comportamento entre aba restaurada no arranque e aba restaurada por nome | **zero** — `cwd`, painéis, `lazy_restore` e `.porecatu` pelo mesmo caminho de código |
| Leituras do diretório de sessões nomeadas no arranque | **zero** |
| Sobrescritas sem confirmação | **zero** |
| Dependências novas no workspace | **zero** |

A terceira é a que mantém o recurso honesto: se a restauração nomeada ganhar um caminho próprio, cedo ou tarde ela diverge da automática, e o usuário descobre qual das duas tem o bug na hora em que mais precisa dela.
