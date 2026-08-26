# PRD-002 — Grupos de abas

**Status:** Aprovado
**Data:** 2026-08-26
**Requisito de origem:** 2 — agrupamento de tabs, para selecionar e agrupar terminais abertos, incluindo atribuir um nome ao grupo
**Relacionados:** [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md), [PRD-001](prd-001-abas.md), [PRD-004](prd-004-aparencia-do-chrome.md)

## Problema

Abas resolvem "muitas janelas". Não resolvem "muitas abas".

Com quinze terminais abertos, a barra vira uma fileira de rótulos parecidos. O usuário sabe que quatro deles são do projeto A e três são do projeto B, mas nada na tela diz isso. A única separação disponível hoje é abrir outra janela — que devolve o problema original.

Grupos dão ao usuário a capacidade de dizer, na própria barra, **quais abas formam um contexto de trabalho**. Este é o recurso que distingue o Porecatu ([PRD-000](prd-000-visao-de-produto.md)).

## Usuário-alvo

Desenvolvedor com mais de um contexto simultâneo: dois repositórios, ou frontend/backend/infra, ou vários clientes. Quem trabalha em um projeto por vez não precisa deste recurso — e o app funciona normalmente sem nenhum grupo criado.

## Modelo

Definido em [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md), resumido aqui porque as restrições são visíveis ao usuário:

- Uma aba pertence a **exatamente um** grupo.
- Grupos **não aninham**.
- Grupos são **contíguos** na barra.
- Abas "sem grupo" existem e são normais — pertencem a um grupo implícito que não é desenhado.

## Requisitos funcionais

### Seleção

**RF-2.1** — O usuário seleciona múltiplas abas: `Ctrl`/`Cmd` + clique alterna uma aba na seleção; `Shift` + clique seleciona o intervalo entre a última selecionada e a clicada.

**RF-2.2** — Abas selecionadas têm indicação visual distinta da aba ativa. *(Selecionada e ativa são estados diferentes: uma aba pode estar selecionada sem estar ativa.)*

**RF-2.3** — Clique numa aba sem modificador limpa a seleção e ativa aquela aba.

### Criação e dissolução

**RF-2.4** — Com uma ou mais abas selecionadas, o usuário cria um grupo por atalho ou menu de contexto. O grupo nasce com nome vazio, em modo de edição inline, e uma cor atribuída automaticamente da paleta de grupos — a próxima cor ainda não usada na janela.

**RF-2.5** — Ao criar um grupo com abas não adjacentes, as abas são **movidas para ficarem contíguas**, na ordem em que aparecem na barra. O movimento é animado, para que a reordenação não surpreenda.

**RF-2.6** — O usuário desagrupa um grupo. As abas voltam ao grupo implícito, mantendo a ordem relativa e a posição onde o grupo estava.

**RF-2.7** — Um grupo cuja última aba é fechada ou movida para fora é removido automaticamente.

**RF-2.8** — O usuário cria um grupo vazio e nele abre uma aba nova diretamente.

### Nome e cor

**RF-2.9** — Todo grupo tem um nome editável, exibido à esquerda de suas abas. Edição inline, `Enter` confirma, `Esc` cancela. Nome vazio é permitido: o grupo aparece apenas como um marcador colorido.

**RF-2.10** — Todo grupo tem uma cor, escolhida pelo usuário entre as cores nomeadas da paleta de grupos definida na config, ou por valor hexadecimal direto.

**RF-2.11** — A cor do grupo é aplicada ao rótulo e a um indicador que abrange visualmente as abas do grupo. A forma exata desse indicador é configurável ([PRD-004](prd-004-aparencia-do-chrome.md)).

**RF-2.12** — Nome de grupo longo é truncado; o nome completo aparece em tooltip.

### Colapso

**RF-2.13** — O usuário colapsa e expande um grupo por clique no rótulo ou por atalho. Colapsado, o grupo mostra apenas o rótulo e a contagem de abas; suas abas somem da barra.

**RF-2.14** — Colapsar um grupo que contém a aba ativa move o foco para a aba visível mais próxima fora dele.

**RF-2.15** — Abas de grupo colapsado **não participam** da navegação sequencial nem do acesso por índice ([PRD-001](prd-001-abas.md)). Continuam vivas: seus processos rodam e sua saída é acumulada.

**RF-2.16** — Grupo colapsado com atividade em alguma aba exibe indicador agregado no rótulo.

**RF-2.17** — Ativar uma aba de grupo colapsado (por busca ou restauração de sessão) expande o grupo.

### Movimentação

**RF-2.18** — O usuário arrasta uma aba para dentro ou para fora de um grupo; a fronteira do grupo é evidenciada durante o arraste ([PRD-001](prd-001-abas.md), RF-1.16).

**RF-2.19** — O usuário arrasta o rótulo de um grupo para reordenar o grupo inteiro na barra. As abas do grupo acompanham.

**RF-2.20** — O usuário move a aba ativa para um grupo por menu de contexto, sem usar o mouse para arrastar.

**RF-2.21** — Navegação por teclado entre grupos: próximo e anterior grupo, ativando a última aba visitada daquele grupo.

### Ações em lote

**RF-2.22** — Menu de contexto do grupo oferece: renomear, mudar cor, colapsar/expandir, nova aba no grupo, fechar todas as abas do grupo (com confirmação), desagrupar.

**RF-2.23** — Fechar todas as abas de um grupo pede confirmação exibindo a contagem, sempre — não é configurável. *(É a ação mais destrutiva da interface.)*

## Critérios de aceite

```gherkin
Cenário: agrupar abas não adjacentes
  Dado abas nas posições 1, 3 e 5 selecionadas
  Quando o usuário cria um grupo
  Então as três ficam contíguas na barra, na ordem 1, 3, 5
  E o movimento é animado
  E o campo de nome do grupo entra em edição

Cenário: cor automática não repete
  Dado uma janela com um grupo de cor azul
  Quando o usuário cria um segundo grupo
  Então o novo grupo recebe uma cor diferente de azul

Cenário: colapso remove da navegação
  Dado um grupo expandido com três abas e um grupo colapsado com duas
  Quando o usuário navega sequencialmente por toda a barra
  Então apenas as três abas do grupo expandido são visitadas

Cenário: colapso desloca o foco
  Dado que a aba ativa pertence ao grupo "api"
  Quando o usuário colapsa o grupo "api"
  Então a aba ativa passa a ser a visível mais próxima fora do grupo

Cenário: processos seguem vivos em grupo colapsado
  Dado um grupo colapsado com um build em execução
  Quando o build termina
  Então o rótulo do grupo exibe indicador de atividade
  E a saída completa está disponível ao expandir

Cenário: grupo vazio é removido
  Dado um grupo com uma única aba
  Quando o usuário fecha essa aba
  Então o grupo deixa de existir

Cenário: desagrupar preserva ordem e posição
  Dado o grupo "api" com três abas na posição central da barra
  Quando o usuário desagrupa
  Então as três abas permanecem naquela posição, na mesma ordem
  E deixam de ter indicador de grupo

Cenário: reordenar grupo inteiro
  Dado dois grupos na barra
  Quando o usuário arrasta o rótulo do segundo para antes do primeiro
  Então o grupo inteiro muda de posição, com suas abas

Cenário: fechar grupo pede confirmação
  Dado um grupo com quatro abas
  Quando o usuário escolhe fechar todas as abas do grupo
  Então o app pede confirmação exibindo a contagem de quatro
```

## Fora de escopo

- Grupos aninhados ([ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md))
- Uma aba em mais de um grupo
- Grupos não-contíguos
- Mover grupo entre janelas
- Agrupamento automático por diretório ou projeto (ideia para v2; o v1 é manual e explícito)
- Ícone customizado por grupo além da cor

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Ações para agrupar abas já selecionadas | 1 (um atalho) |
| Ações para nomear o grupo após criar | 0 extras — a edição já abre |
| Identificar visualmente o grupo de uma aba | imediato, sem hover |
| Grupos por janela sem degradação de layout | 10 |
