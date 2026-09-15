# PRD-006 — Painéis divididos

**Status:** Aprovado
**Data:** 2026-08-26 (rascunho) · 2026-09-15 (aprovado)
**Requisito de origem:** pedido direto do dono do produto, sobre o v1 já em uso — *"quando estou com uma aba aberta, posso dividi-la horizontal ou verticalmente, ficando com 2 terminais na mesma aba… deve ser possível repetir esse processo mais vezes"*
**Relacionados:** [ADR-0053](../adr/0053-paineis-divididos.md), [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md), [ADR-0007](../adr/0007-modelo-de-threading.md), [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md), [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md), [ADR-0037](../adr/0037-aba-nao-iniciada.md), [ADR-0048](../adr/0048-barra-de-status.md), [PRD-001](prd-001-abas.md), [PRD-003](prd-003-persistencia-de-sessao.md), [PRD-009](prd-009-barra-de-status.md)

> Aprovado em 2026-09-15, por decisão do dono do produto, **fora da ordem de fases** — como a barra de status ([PRD-009](prd-009-barra-de-status.md)), o arquivo de projeto ([PRD-012](prd-012-comando-de-projeto-por-diretorio.md)) e a sincronização com o Git ([PRD-013](prd-013-sincronizacao-com-o-remoto-do-git.md)). Diferente dos dois últimos, este é **rascunho promovido**, o caminho que o PRD-009 percorreu: o documento existia desde 2026-08-26 para dar endereço a um elemento desenhado e fora do v1. O texto do rascunho não sobreviveu — o que era esboço virou requisito, e as quatro perguntas que ele deixava em aberto viraram decisões no [ADR-0053](../adr/0053-paineis-divididos.md).

## Problema

Alguns pares de terminais só fazem sentido **ao mesmo tempo**: um servidor rodando e o log dele; um build e os testes; um `ssh` e o `journalctl` da mesma máquina; um editor e o comando que ele deveria fazer passar.

Abas resolvem "muitos contextos". Elas não resolvem este, porque a troca de aba **destrói exatamente o que se quer olhar**: a correlação no momento em que ela acontece. Quem alterna entre duas abas para comparar duas saídas está usando a memória de curto prazo como se fosse tela, e é a memória que falha primeiro.

Hoje, quem precisa disso no Porecatu tem três saídas, e as três empurram o trabalho para fora do produto:

- **Duas janelas lado a lado**, arrumadas à mão a cada vez. Funciona, e custa o gerenciamento de janelas do sistema operacional toda vez que a geometria muda. A sessão restaura as duas janelas ([PRD-003](prd-003-persistencia-de-sessao.md)), mas não restaura o fato de que elas precisam ficar uma ao lado da outra.
- **`tmux` dentro de uma aba.** Resolve de verdade, e é a alternativa honesta — mas põe um segundo multiplexador embaixo do nosso, com uma segunda tecla líder, um segundo conceito de "aba" e um scrollback que não é o do Porecatu. Quem faz isso está dizendo que o produto não faz o que ele precisa.
- **Não fazer.** Alternar e torcer.

Abas resolvem "muitos contextos". **Painéis resolvem "um contexto, duas superfícies"** — e o [PRD-000](prd-000-visao-de-produto.md) define o produto pela gestão de muitos terminais, o que torna esta a lacuna mais visível que sobrou depois do v1.

## Usuário-alvo

O mesmo do [PRD-000](prd-000-visao-de-produto.md): quem mantém muitos terminais abertos ao mesmo tempo. O valor aqui cresce com **quantos processos longos o trabalho exige rodando em paralelo** — um servidor de desenvolvimento, um observador de testes, um túnel —, não com o número de projetos.

**Não é para** quem usa um terminal de cada vez, e não é substituto de abas nem de grupos: dividir uma aba em seis painéis de 40 colunas é a maneira de ter seis terminais ilegíveis. O RF-6.4 existe para que o produto recuse essa divisão em vez de entregá-la.

## Em uma tela

Hoje, uma aba é um terminal, dentro de um quadro arredondado com 6px de margem da janela:

```
┌──────────────────────────────────────────┐
│  ▏ api ▕  ▏ web ▕  +                     │  barra de abas
├──────────────────────────────────────────┤
│ ╭──────────────────────────────────────╮ │
│ │ $ npm run dev                        │ │
│ │ listening on :3000                   │ │  um quadro,
│ │ ▊                                    │ │  uma grade
│ ╰──────────────────────────────────────╯ │
│  pwsh   ~/Projetos/api   ⑂ main   API    │  barra de status
└──────────────────────────────────────────┘
```

`Ctrl+Shift+H` **empilha** — o painel novo nasce **abaixo** do focado:

```
│ ╭──────────────────────────────────────╮ │
│ │ $ npm run dev                        │ │
│ │ listening on :3000                   │ │
│ ╰──────────────────────────────────────╯ │
│                                          │ ← o divisor é este vão
│ ╭──────────────────────────────────────╮ │
│ │ $ npm test -- --watch                │ │
│ │ 42 passing  ▊                        │ │
│ ╰──────────────────────────────────────╯ │
│  pwsh  ~/Projetos/api  ⑂ main  2 painéis │
```

`Ctrl+Shift+D` põe **lado a lado** — o painel novo nasce **à direita**:

```
│ ╭─────────────────╮ ╭──────────────────╮ │
│ │ $ npm run dev   │ │ $ tail -f log    │ │
│ │ listening :3000 │ │ GET /health 200  │ │
│ │ ▊               │ │ GET /users  200  │ │
│ ╰─────────────────╯ ╰──────────────────╯ │
│                     ↑                    │
│            arrastar aqui redimensiona    │
```

Os dois gestos se repetem sobre qualquer painel, quantas vezes couber. **O divisor não é um traço: é o mesmo vão que já existe entre a janela e o terminal.** Cada painel é um quadro inteiro — mesmo raio, mesma sombra, mesmo recuo interno, mesma fonte, mesmas cores.

E, uma vez, na config do usuário:

```toml
[panes]
min_columns = 20
min_rows = 5
```

## Requisitos funcionais

### O split

**RF-6.1** — A aba pode ser dividida em dois painéis, e o alvo da divisão é sempre o **painel focado**, nunca a aba inteira. Duas orientações, e a diferença entre elas é literal:

| Ação | Atalho | O divisor fica | O painel novo nasce |
|---|---|---|---|
| `pane.split_horizontal` | `Ctrl+Shift+H` | deitado | **abaixo** do focado |
| `pane.split_vertical` | `Ctrl+Shift+D` | em pé | **à direita** do focado |

A ambiguidade de "horizontal" entre emuladores é conhecida e está resolvida aqui, na tabela, e não no nome da ação: quem lê o catálogo lê esta linha junto.

**RF-6.2** — O gesto **se repete sobre qualquer painel**, sem limite de aninhamento. Dividir um painel que já é metade de um split produz três painéis; dividir de novo produz quatro, e assim por diante. Quem limita é o RF-6.4, por tamanho útil, não por contagem: um limite em número seria arbitrário numa tela de 4K e generoso demais num laptop.

**RF-6.3** — O painel novo sobe o shell configurado no diretório do painel de origem — mesma herança de `cwd` que `group.new_tab` já faz a partir da última aba do grupo. Ele nasce **focado**.

**RF-6.4** — O split é **recusado** quando qualquer um dos dois painéis resultantes ficaria abaixo do mínimo útil, configurável em `[panes] min_columns` e `min_rows`. O app informa por que recusou e não divide nada. Entregar um painel de oito colunas é entregar um terminal que não serve para nada, com a aparência de ter funcionado.

### O painel

**RF-6.5** — Cada painel tem **PTY, grade, scrollback, diretório, título e ciclo de vida próprios**. Dois painéis da mesma aba são dois terminais completos; nada é compartilhado entre eles além do retângulo que dividem.

**RF-6.6** — Há **exatamente um painel focado por aba**, e ele recebe todo o input de teclado: teclas, IME e teclas mortas, colagem, rolagem do scrollback e as ações de `clipboard.*`, `selection.*` e `search.*`. Nenhuma tecla é entregue a dois painéis, e não existe aba com N painéis e nenhum focado.

**RF-6.7** — Clicar dentro de um painel o foca. O clique que foca **também é entregue ao terminal daquele painel** — posicionar o cursor, iniciar uma seleção ou clicar dentro de um programa que pede o mouse funciona no primeiro clique, sem exigir um clique de foco antes.

**RF-6.8** — `Alt+←`, `Alt+→`, `Alt+↑` e `Alt+↓` movem o foco para o painel vizinho **naquela direção geométrica**. Sem vizinho naquela direção, nada acontece — o foco não dá a volta, porque num layout em cruz dar a volta leva a um painel que não está do lado nenhum.

**RF-6.9** — O painel focado se distingue **pelo cursor, e só por ele**: o focado desenha o cursor como sempre desenhou, os demais desenham o mesmo cursor **vazado**. Nenhuma borda, nenhum ponto, nenhum cabeçalho e nenhum esmaecimento — o quadro de um painel sem foco é pixel por pixel o quadro de um painel com foco, exceto ali.

**RF-6.10** — `pane.close` fecha o painel focado, e o espaço dele volta para os vizinhos. Fechar o **último** painel fecha a aba. A confirmação do RF-1.6 do [PRD-001](prd-001-abas.md) continua valendo, agora sobre o painel: fechar um painel com processo ativo pergunta antes.

**RF-6.11** — O shell que termina sozinho (`exit`, `Ctrl+D`) fecha **aquele painel**, não a aba — mesma regra que o [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md) já dá à aba, um nível abaixo. Saída com código diferente de zero mantém o painel aberto com a nota de saída, como hoje.

### O divisor

**RF-6.12** — O divisor é **espaço vazio**, com a mesma medida da margem entre a janela e o terminal. Não há linha, traço, alça nem sombra própria: o que separa dois painéis é o fundo da janela aparecendo entre dois quadros, exatamente como ele já aparece entre a janela e o terminal único de hoje.

**RF-6.13** — Arrastar o vão entre dois painéis os redimensiona, e a grade de cada um **reencaixa durante o gesto**, não ao soltar. O cursor muda de forma sobre o divisor, antes do arraste, para que ele seja descobrível sem documentação.

**RF-6.14** — O arraste é clampado pelo mesmo mínimo do RF-6.4: o divisor para de andar quando um dos vizinhos chega ao limite. Não há estado intermediário a desfazer — o arraste aplica ao vivo, e soltar não confirma nada que já não estivesse valendo.

**RF-6.15** — Redimensionar a janela, mudar o zoom de fonte ou ligar e desligar a barra de status **preserva as proporções** e reencaixa o PTY de todos os painéis. Nenhum painel some, nenhum fica com a grade velha.

**RF-6.16** — A borda de redimensionamento da janela continua vencendo em toda a borda, **menos** dentro do retângulo de um divisor. É a mesma disputa que os botões de janela ([ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md)) e o indicador de commits ([PRD-013](prd-013-sincronizacao-com-o-remoto-do-git.md)) já resolvem, e ela se resolve do mesmo jeito.

### O que a aba passa a mostrar

**RF-6.17** — O título da aba é o do **painel focado**, com a mesma precedência do RF-1.7 do [PRD-001](prd-001-abas.md) aplicada dentro dele. Título definido pelo usuário para a aba continua vencendo tudo, inclusive a troca de painel focado — é da aba, não do painel.

**RF-6.18** — Os indicadores de atividade e de campainha da aba **agregam**: qualquer painel que produza saída em segundo plano, ou que toque a campainha, acende o indicador da aba. Ativar a aba os limpa, como hoje. Uma aba que esconde a atividade de metade dos seus terminais seria pior que não ter indicador.

**RF-6.19** — A barra de status descreve o **painel focado** — shell, diretório, grupo e Git. A regra do [PRD-009](prd-009-barra-de-status.md) não muda de natureza: a barra sempre descreveu o terminal em foco, e agora o terminal em foco é um painel.

**RF-6.20** — Com dois ou mais painéis, a barra de status mostra a **contagem**. Com um painel só, o segmento não existe — nem "1 painel", nem versão apagada dele. É a mesma regra de ausência do indicador de Git e do ícone de repositório, e é a condição sob a qual o [ADR-0048](../adr/0048-barra-de-status.md) §5 tinha prometido que esse segmento voltaria.

**RF-6.21** — Busca, seleção, colagem e menu de contexto do terminal agem sobre **um painel**: o focado, quando vêm do teclado; o que está sob o cursor, quando vêm do mouse. A barra de busca se posiciona sobre o quadro daquele painel, e não atravessa a aba inteira.

### Sessão

**RF-6.22** — A sessão grava a **árvore de painéis, as proporções, o diretório e o programa de cada painel e qual deles estava focado**. Reabrir o app devolve o layout como estava, com cada painel no seu diretório.

**RF-6.23** — A restauração preguiçosa ([PRD-003](prd-003-persistencia-de-sessao.md) RF-3.8) continua sendo **por aba**: focar uma aba restaurada sobe **todos** os painéis dela, de uma vez. Meia aba com prompt e meia aba em branco seria um estado que ninguém pediu, e as proporções precisam dos dois lados para valer.

**RF-6.24** — O comando de projeto do [PRD-012](prd-012-comando-de-projeto-por-diretorio.md) roda **uma vez por aba restaurada, no painel focado**, e não uma vez por painel. Painéis de um split quase sempre dividem o mesmo diretório, e disparar o mesmo `.porecatu` em dois deles é subir o mesmo servidor duas vezes na mesma porta.

### Fronteiras

**RF-6.25** — Painel **não se move** entre abas nem entre janelas. É a mesma fronteira que o [PRD-001](prd-001-abas.md) já mantém para abas entre janelas, pela mesma razão: o modelo permite, a UI é que fica para depois.

**RF-6.26** — Não há **zoom** nem maximização temporária de painel nesta entrega.

**RF-6.27** — Não há **cabeçalho de painel**: nem título, nem ponto de foco, nem botão de dividir, nem botão de fechar. O desenho do canvas previa um, e ele foi recusado — ver o [ADR-0053](../adr/0053-paineis-divididos.md) §4 e as alternativas.

## Critérios de aceite

```gherkin
Cenário: o caso que motiva o recurso
  Dado uma aba com um servidor rodando
  Quando o usuário pressiona Ctrl+Shift+H
  Então a aba passa a ter dois painéis, um acima do outro
  E o painel de baixo é o novo, está focado e abriu no mesmo diretório
  E o servidor continua rodando no painel de cima, sem ter sido reiniciado

Cenário: as duas orientações são o que a tabela diz
  Dado uma aba com um painel
  Quando o usuário pressiona Ctrl+Shift+D
  Então o painel novo aparece à direita do anterior
  E os dois ficam lado a lado, com a mesma altura

Cenário: repetir o gesto
  Dado uma aba com dois painéis lado a lado e o da direita focado
  Quando o usuário pressiona Ctrl+Shift+H
  Então só o painel da direita se divide em dois
  E a aba passa a ter três painéis
  E o painel da esquerda não mudou de tamanho

Cenário: o divisor é só o vão
  Dado uma aba com dois painéis
  Quando o usuário olha o espaço entre eles
  Então não há linha, traço nem alça desenhada ali
  E o vão tem a mesma medida da margem entre a janela e o terminal
  E cada painel tem o mesmo raio, a mesma sombra e o mesmo recuo interno de antes

Cenário: arrastar o vão redimensiona
  Dado uma aba com dois painéis lado a lado
  Quando o usuário arrasta o vão entre eles para a direita
  Então o painel da esquerda cresce e o da direita encolhe
  E o conteúdo dos dois reencaixa durante o arraste, não só ao soltar

Cenário: o arraste para no mínimo
  Dado dois painéis lado a lado e min_columns = 20
  Quando o usuário arrasta o vão até o fim da janela
  Então o divisor para quando o painel que encolhe chega a 20 colunas
  E nenhum painel desaparece

Cenário: o split que não cabe é recusado
  Dado um painel com 30 colunas e min_columns = 20
  Quando o usuário pressiona Ctrl+Shift+D
  Então nenhum painel novo é criado
  E o app informa por que recusou

Cenário: o foco se vê pelo cursor
  Dado uma aba com dois painéis e o de cima focado
  Quando o usuário olha os dois
  Então o cursor do painel de cima está cheio
  E o cursor do painel de baixo está vazado
  E nada mais difere entre os dois quadros

Cenário: o teclado vai para um painel só
  Dado uma aba com dois painéis e o de baixo focado
  Quando o usuário digita
  Então o texto aparece no painel de baixo
  E nada é escrito no painel de cima

Cenário: clicar foca e já age
  Dado uma aba com dois painéis e o de cima focado
  Quando o usuário clica no meio de uma palavra no painel de baixo e arrasta
  Então o painel de baixo passa a ser o focado
  E a seleção começou no primeiro clique, sem exigir um clique antes

Cenário: navegar pelo teclado
  Dado quatro painéis em cruz, com o superior esquerdo focado
  Quando o usuário pressiona Alt+Direita
  Então o foco vai para o painel superior direito
  Quando o usuário pressiona Alt+Direita de novo
  Então nada acontece

Cenário: fechar um painel devolve o espaço
  Dado uma aba com dois painéis
  Quando o usuário fecha o painel focado
  Então o painel restante ocupa a aba inteira
  E a aba continua aberta

Cenário: fechar o último painel fecha a aba
  Dado uma aba com um painel só
  Quando o usuário digita exit no shell dele
  Então a aba fecha, como sempre fechou

Cenário: exit num painel não leva a aba junto
  Dado uma aba com três painéis
  Quando o usuário digita exit no painel focado
  Então só aquele painel fecha
  E os outros dois continuam rodando

Cenário: a aba fala pelo painel focado
  Dado uma aba com dois painéis em diretórios diferentes
  Quando o usuário troca o foco entre eles
  Então o título da aba e a barra de status passam a descrever o painel focado
  E a contagem "2 painéis" aparece na barra nos dois casos

Cenário: um painel só não mostra contagem
  Dado uma aba com um painel
  Quando o usuário olha a barra de status
  Então não há segmento de contagem de painéis
  E não aparece "1 painel"

Cenário: atividade de painel escondido acende a aba
  Dado uma aba em segundo plano com dois painéis
  Quando um deles produz saída
  Então o indicador de atividade da aba acende
  E ativar a aba o apaga

Cenário: a sessão devolve o layout
  Dado uma aba dividida em três painéis, com proporções ajustadas à mão
  Quando o usuário fecha e reabre o app
  Então a aba volta com os três painéis, nas mesmas proporções
  E cada painel volta no diretório em que estava

Cenário: restauração preguiçosa sobe a aba inteira
  Dado uma sessão restaurada com uma aba dividida que não é a ativa
  Quando o usuário foca essa aba pela primeira vez
  Então todos os painéis dela sobem
  E nenhum deles fica sem prompt

Cenário: o comando de projeto não roda duas vezes
  Dado uma aba restaurada com dois painéis no mesmo diretório autorizado
  E um .porecatu que sobe o servidor de desenvolvimento
  Quando a aba é restaurada
  Então o comando roda uma vez, no painel focado
  E o outro painel não executa nada

Cenário: a borda continua sendo a borda
  Dado uma aba dividida, com um divisor perto da borda direita da janela
  Quando o usuário arrasta a borda direita da janela fora do divisor
  Então a janela é redimensionada, como antes

Cenário: a janela muda de tamanho e as proporções ficam
  Dado dois painéis lado a lado, um com o dobro da largura do outro
  Quando o usuário maximiza a janela
  Então a proporção entre eles continua sendo de dois para um
  E os dois PTYs foram redimensionados
```

## Fora de escopo

Cada item é decisão, não esquecimento. Os motivos completos estão no [ADR-0053](../adr/0053-paineis-divididos.md) e nas alternativas dele.

- **Cabeçalho de painel** — título, ponto de foco, botão de dividir e botão de fechar, como o canvas desenha. Recusado pelo dono do produto (RF-6.27): cada cabeçalho custaria linhas da grade em todo painel, para repetir o que a barra de status já diz do focado, e o que ele resolveria — saber qual painel tem o foco — o cursor resolve sem tirar uma linha de ninguém.
- **Divisor desenhado**, como linha de 1px. O vão já separa; uma linha entre dois quadros que já têm sombra é a terceira separação no mesmo lugar, que é o argumento com que a §2.8 da [especificação visual](../design/especificacao-visual.md) já tirou a borda do topo da barra de status.
- **Zoom de painel** (RF-6.26). Conveniência clássica, e nada nesta entrega a impede depois.
- **Mover painel entre abas ou janelas** (RF-6.25).
- **Layouts salvos e nomeados**, e abrir aba já dividida por perfil. Depende do [PRD-007](prd-007-perfis-de-aba.md), que é rascunho.
- **Sincronizar input entre painéis** (digitar nos dois ao mesmo tempo). Recurso de administração de frota; o produto é de desenvolvimento.
- **Painéis flutuantes ou sobrepostos.** A divisão é do retângulo, sem sobreposição, pela mesma razão que os grupos são contíguos ([ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md)): o que não é desenhável de forma legível não ajuda.
- **Fonte ou tema por painel.** O zoom de fonte continua sendo do processo inteiro, com a dívida do `zoom_scope` registrada desde a F4.

### O que este documento **não** contradiz

O [ADR-0032](../adr/0032-interface-do-v1-fechada.md) fechou a interface do v1 e disse que **a trilha de grupos e abas só é tocada quando um recurso novo exigir**, e que qualquer mudança das seções 1 e 2 da especificação visual exige ADR novo. Este documento **não toca a trilha** — nenhuma aba, pílula, cápsula ou botão da barra muda —, e a mudança que ele faz na §2.7 passa pelo ADR que aquele documento exige, que é o [ADR-0053](../adr/0053-paineis-divididos.md). A regra continua de pé, inclusive para o próximo.

O [ADR-0007](../adr/0007-modelo-de-threading.md) decidiu **uma thread de leitura por terminal**, e isso continua literalmente verdadeiro: o que muda é que uma aba passa a ter mais de um terminal. O regime damage-driven não muda em nada — um painel ocioso não produz quadro, e um painel que escreve suja o painel, não a aba.

O [ADR-0043](../adr/0043-arvore-de-acessibilidade.md) deixou a grade do terminal **declaradamente fora** da árvore de acessibilidade. Os painéis entram na árvore como estrutura — existem, têm posição e um deles tem o foco —, e o conteúdo da grade continua fora, pela mesma razão de antes.

O [ADR-0003](../adr/0003-formato-de-configuracao.md) e o [PRD-004](prd-004-aparencia-do-chrome.md) exigem que nenhuma cor ou dimensão viva no código. Este documento **não inventa nenhum valor de aparência**: o vão entre painéis é o `terminal_frame_margin` que já existe, e as duas chaves novas — `min_columns` e `min_rows` — são de comportamento, não de desenho.

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Ações para ver dois terminais do mesmo projeto ao mesmo tempo | **uma** (o atalho) |
| Janelas do sistema operacional arrumadas à mão para conseguir isso | **zero** |
| Multiplexadores concorrentes rodando dentro do app | **zero** |
| Valores de aparência novos introduzidos pelo recurso | **zero** |
| Dependências novas no workspace | **zero** |
| Painéis entregues abaixo do mínimo útil | **zero** — o split é recusado antes |
| Ações do usuário para reconstruir o layout dividido após reabrir | **zero** |

A primeira é a razão de este documento existir. A **quarta** é a que o mantém honesto: um recurso que ocupa metade da área de conteúdo do app sem introduzir um único token novo é a prova de que ele nasce dentro da linguagem visual que o [ADR-0028](../adr/0028-o-binario-como-referencia-visual.md) fixou, e não ao lado dela.
