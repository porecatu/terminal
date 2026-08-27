# ADR-0021 — Seleção múltipla e gestos da barra de abas

**Status:** Aceito
**Data:** 2026-08-27
**Relacionados:** [ADR-0006](0006-modelo-de-abas-e-grupos.md), [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0013](0013-mouse-selecao-e-clipboard.md), [ADR-0020](0020-grupos-explicitos.md), [PRD-001](../prd/prd-001-abas.md), [PRD-002](../prd/prd-002-grupos-de-abas.md), [PRD-010](../prd/prd-010-interacao-e-superficie-de-app.md)

## Contexto

A F3 começa por um gesto: **selecionar várias abas** é o passo 1 de
`group.create` (RF-2.4), e sem ele o recurso que distingue o produto não tem
entrada. Três requisitos o definem:

- **RF-2.1** — *"`Ctrl`/`Cmd` + clique alterna uma aba na seleção; `Shift` +
  clique seleciona o intervalo entre a última selecionada e a clicada."*
- **RF-2.2** — *"Abas selecionadas têm indicação visual distinta da aba ativa.
  (Selecionada e ativa são estados diferentes: uma aba pode estar selecionada
  sem estar ativa.)"*
- **RF-2.3** — *"Clique numa aba sem modificador limpa a seleção e ativa aquela
  aba."*

Nenhum ADR modela isso. `porecatu-core` não tem campo de seleção; `porecatu-ui`
tem seleção de **texto** e de item de menu, e nada mais. O
[catálogo de ações](../reference/acoes.md) trata o assunto por ausência
deliberada — *"`group.select_all_tabs`: RF-2.1 define seleção múltipla por
mouse; nenhum RF pede equivalente de teclado"* — e a tabela de "superfícies de
mouse que não são ações" enumera seis gestos da F2 sem nenhum da F3.

Quatro perguntas ficaram abertas, e as quatro têm consequência em código:

**1. Onde a seleção vive.** Se em `Workspace`, ela é serializável e a sessão
teria de decidir se a grava. Se em `WindowState`, é efêmera como `rename`,
`hover` e `drag` — mas então `group.create`, que é operação de domínio, precisa
recebê-la de fora.

**2. O que a invalida.** Fechar uma aba selecionada, colapsar o grupo dela,
criar o grupo, perder o foco da janela. E qual é a âncora do `Shift`+clique
quando a última aba selecionada não existe mais.

**3. `Ctrl` no macOS.** O RF-2.1 escreve *"`Ctrl`/`Cmd`"* como se fossem
equivalentes por plataforma, mas no macOS **`Ctrl`+clique é o clique
secundário** — é o gesto que abre o menu de contexto (RF-10.19). O
[ADR-0013](0013-mouse-selecao-e-clipboard.md) só resolveu a disputa de mouse na
área de conteúdo, e é explícito: *"A barra de abas não participa disso... Só a
área de conteúdo do terminal está em disputa."* A barra ficou sem regra.

**4. A fronteira do arraste entre grupos.** O RF-1.16 diz *"arrastar uma aba
para dentro dos **limites visuais** de um grupo a move para aquele grupo"*. Os
wrappers são separados por `gap: 6`, e a regra de partição existe só **dentro**
do grupo: a especificação visual §2.2 decide que *"a fronteira entre abas
vizinhas parte o `gap` ao meio — nenhum pixel da barra fica sem dono"*, e a F2
implementou isso sem estendê-lo ao espaço entre wrappers. Falta dizer a quem
pertencem aqueles 6 px durante o gesto, e o que significa soltar sobre uma
pílula ou sobre um grupo colapsado.

## Decisão

**A seleção é estado efêmero de janela, com âncora explícita; na barra de abas o
modificador de seleção é `Ctrl` em Windows e Linux e `Cmd` no macOS; e o espaço
entre wrappers pertence ao grupo da esquerda durante o arraste.**

### 1. A seleção vive em `WindowState`, não em `Workspace`

```
Selection {
    tabs: BTreeSet<TabId>,   // vazio = nada selecionado
    anchor: Option<TabId>,   // origem do intervalo de Shift+clique
}
```

Fica ao lado de `rename`, `drag`, `hover` e `scroll_offset`, e **não é
persistida** — a lista do [ADR-0005](0005-persistencia-de-sessao.md) não a
inclui, e seleção sobrevivente a um restart é surpresa: o usuário reabre o app e
uma ação destrutiva de grupo já tem alvo escolhido sem que ele tenha escolhido.

`group.create` é operação de domínio e continua em `porecatu-core`: recebe a
lista de `TabId` como argumento (`group_tabs(ids, ...)`, como a tabela do
ADR-0006 já prevê). O crate não sabe o que é "seleção", só recebe abas.

**Seleção vazia não é caso especial.** Com nada selecionado, `group.create` opera
sobre a aba **ativa** — é o caminho que dispensa o mouse e o que torna o atalho
útil sem gesto anterior.

### 2. Invalidação e âncora

- **Fechar aba selecionada** — sai da seleção. Se era a âncora, a âncora passa a
  ser a aba selecionada mais próxima na ordem visual; sem nenhuma, `None`.
- **Colapsar o grupo de uma aba selecionada** — a aba **sai** da seleção. Aba
  invisível na seleção é alvo que o usuário não vê, o mesmo raciocínio que o
  [ADR-0020](0020-grupos-explicitos.md) usou para tirar aba colapsada do
  `tab.goto_N`.
- **Criar o grupo** — limpa a seleção. O grupo recém-criado é o novo alvo, e o
  RF-2.4 já manda entrar em edição de nome.
- **Clique sem modificador** — limpa (RF-2.3, literal).
- **`Esc`** — limpa. É a mesma tecla que dispensa aviso e cancela rename; a
  seleção entra na cadeia de captura do
  [ADR-0008](0008-teclas-e-roteamento-de-input.md) **depois** do rename e do
  diálogo, e antes da tabela de keybindings.
- **Perda de foco da janela** — **preserva**. Trocar de janela para consultar
  algo e voltar não deve custar a seleção; é o comportamento de todo gerenciador
  de arquivos.
- **`Shift`+clique sem âncora** — seleciona só a aba clicada e a torna âncora.
- **Intervalo de `Shift`+clique** — sobre a **ordem visual**, atravessando
  fronteira de grupo, e **exclui** abas de grupo colapsado (que não estão na
  ordem navegável). Atravessar grupos é o que permite formar um grupo a partir
  de abas que hoje estão espalhadas, que é o caso de uso do RF-2.5.

### 3. Modificador de seleção por plataforma

| Plataforma | Alterna uma aba | Seleciona intervalo | Clique secundário |
|---|---|---|---|
| Windows, Linux | `Ctrl`+clique | `Shift`+clique | botão direito |
| macOS | `Cmd`+clique | `Shift`+clique | botão direito **e** `Ctrl`+clique |

No macOS, **`Ctrl`+clique na barra abre o menu de contexto** e não toca a
seleção. É a convenção da plataforma, e o RF-10.19 já exige que o menu abra
ancorado no cursor; um gesto que faz duas coisas diferentes em dois lugares do
app é pior que um requisito escrito de forma genérica.

Isso não conflita com o ADR-0013: ele governa a disputa entre seleção de texto e
reporte de mouse na **área de conteúdo**, e diz explicitamente que a barra não
participa. Esta tabela é a regra que faltava para a barra.

### 4. Fronteira do arraste

- **O `gap` entre wrappers pertence ao grupo da esquerda.** Soltar ali entra no
  fim daquele grupo. A alternativa — partir os 6 px ao meio, como §2.2 faz entre
  abas — foi descartada porque 3 px é menor que o limiar de 4 px do próprio
  gesto: seria uma zona que o usuário não consegue mirar.
- **Soltar sobre a pílula** entra no **início** do grupo. É a posição que a
  pílula ocupa visualmente, e é o único jeito de inserir antes da primeira aba
  sem mirar num vão de 6 px.
- **Soltar sobre um grupo colapsado** move a aba para dentro dele e o grupo
  **continua colapsado** — a aba desaparece da trilha. O ADR-0020 já garante que
  ela sai da seleção e da ordem navegável; o feedback de que algo aconteceu é o
  contador da pílula, que incrementa. Recusar o gesto seria pior: a pílula é um
  alvo grande e óbvio, e recusa silenciosa parece travamento.
- **Soltar fora de qualquer wrapper** move para o run implícito daquela posição,
  criando-o se não existir (RF-1.16, segunda frase).
- **Arrastar o rótulo do grupo** (RF-2.19) move o grupo inteiro. O alvo é a
  fronteira entre grupos, com a mesma regra de partição do `gap` acima, e o
  grupo nunca cai dentro de outro grupo — grupos não aninham (ADR-0006).

Consequência em código: `tab_bar::drag_target_index` hoje só resolve posição
dentro do grupo da aba arrastada; passa a devolver `(GroupId, usize)`.

## Alternativas consideradas

### Seleção em `Workspace`, persistida com a sessão

Colocaria a seleção junto das abas e dos grupos, e `group_tabs` não precisaria de
argumento. Descartada por dois motivos: o PRD-003 não lista seleção entre o que a
sessão grava, e restaurar o app com abas já selecionadas dá alvo pronto a
`group.close_all`, a ação que o RF-2.23 chama de *"a mais destrutiva da
interface"*. Estado que arma uma ação destrutiva não deve atravessar restart.

### Seleção em `Workspace`, mas marcada como não serializável

Resolveria a persistência com `#[serde(skip)]` e manteria a seleção perto do
domínio. Descartada porque `porecatu-core` é o crate de **operações puras sobre
estrutura**, e seleção é estado de interação: quem seleciona é o mouse. Enfiar
interação no core abre a porta para hover e arraste virem depois, e o round-trip
de sessão passaria a comparar um campo que não é gravado.

### Manter `Ctrl`+clique no macOS, como o RF-2.1 escreve

Uniformidade entre plataformas, e o requisito ficaria cumprido ao pé da letra.
Descartada porque `Ctrl`+clique é o clique secundário do macOS: o usuário que
tenta abrir o menu de contexto na barra alteraria a seleção, e o que ele espera —
o menu — não apareceria. Cumprir a letra do requisito quebrando a convenção da
plataforma é o pior dos dois mundos.

### Exigir `Cmd` em todas as plataformas

Um só modificador no código, sem tabela por plataforma. Descartada porque não
existe `Cmd` em teclado de PC, e `Super`/`Win` é tecla do gerenciador de janelas
em Linux — vincular seleção a ela produz conflito com o ambiente.

### Partir o `gap` entre wrappers ao meio, como entre abas

Consistência com a regra de §2.2 e nenhum pixel sem dono. Descartada porque cada
metade tem 3 px, abaixo do limiar de 4 px do próprio arraste: uma zona que o
usuário não consegue acertar de propósito não é uma zona, é uma loteria.

### Recusar o drop sobre grupo colapsado

Evitaria que a aba desaparecesse da vista logo depois de o usuário arrastá-la.
Descartada porque a pílula é o alvo mais fácil de acertar da barra, e recusa sem
efeito visível é indistinguível de um travamento. O contador que incrementa é
feedback suficiente, e a operação é reversível pelo próprio arraste.

## Consequências

### Positivas

- `porecatu-core` continua sem estado de interação: seleção entra como argumento
  nas operações que precisam dela.
- A seleção é testável como estado puro, no padrão de `rename.rs`, `dialog.rs` e
  `tooltip.rs` — sem `winit`, sem GPU.
- Todo pixel da barra tem dono durante o arraste, incluindo os vãos entre
  grupos, e nenhuma zona de alvo fica abaixo do limiar do gesto.
- O menu de contexto no macOS funciona pela convenção da plataforma sem exceção
  no roteamento de input.

### Negativas

- O RF-2.1 fica cumprido de forma diferente do que escreve, e precisa de nota de
  reconciliação apontando para cá.
- `Selection` é o quinto estado efêmero de `WindowState`, e a lista de "o que
  invalida" cresce com cada operação nova de grupo — é uma matriz que só teste
  cobre.
- Preservar a seleção na perda de foco significa que ela pode ficar viva por
  muito tempo; um `group.close_all` disparado depois age sobre algo que o
  usuário selecionou minutos antes. O RF-2.23 exige confirmação com contagem
  sempre, o que é a mitigação já decidida.
- `drag_target_index` passa a devolver grupo e posição, o que muda a assinatura
  de uma função que a F2 acabou de estabilizar e testar.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Aba fechada continuar na seleção e virar `TabId` órfão | Alta | Alto | A remoção acontece no mesmo ponto que `close_tab` é chamada; teste de que nenhum `TabId` da seleção falta no `Workspace`, verificado depois de cada operação |
| Âncora apontar para aba que não existe | Alta | Médio | Âncora é revalidada junto com a seleção; teste específico de fechar a âncora e depois usar `Shift`+clique |
| Seleção esquecida armar ação destrutiva | Média | Alto | `group.close_all` confirma sempre com contagem (RF-2.23); criar grupo limpa a seleção |
| Usuário de macOS não descobrir `Cmd`+clique | Média | Baixo | Menu de contexto da aba oferece as mesmas ações, e o `Shift`+clique funciona igual nas três plataformas |
| Aba somindo ao ser solta em grupo colapsado ser lida como perda | Média | Médio | Contador da pílula incrementa no mesmo frame; o gesto é reversível expandindo o grupo |
| `drag_target_index` com grupo errado mover aba para o grupo vizinho | Média | Alto | A regra do `gap` é uma função só, testada nas duas fronteiras de cada wrapper e sobre a pílula |
