# ADR-0020 — Grupos explícitos: multiplicidade do implícito, colapso e foco

**Status:** Aceito
**Data:** 2026-08-27
**Relacionados:** [ADR-0006](0006-modelo-de-abas-e-grupos.md), [ADR-0005](0005-persistencia-de-sessao.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [PRD-001](../prd/prd-001-abas.md), [PRD-002](../prd/prd-002-grupos-de-abas.md)

## Contexto

O [ADR-0006](0006-modelo-de-abas-e-grupos.md) decidiu o modelo de abas e grupos
antes de existir uma linha de código. A F2 implementou a metade dele que a fase
exercitava — `Workspace`, `Tab`, `Group` com um grupo implícito único — e deixou
`group_tabs`, `ungroup`, `rename_group`, `set_group_color` e `collapse_group`
fora de propósito, porque nada na fase as chamava.

Ao preparar a F3, cinco pontos do ADR-0006 mostraram-se **ambíguos ou
insuficientes**, não errados. Nenhum deles impedia a F2; todos impedem a F3.

**1. O grupo implícito é declarado no singular.** O ADR-0006 diz: *"Abas 'sem
grupo' pertencem a um grupo implícito. Ele não tem nome nem cor, não é desenhado
como pílula, e não pode ser renomeado, colapsado ou removido. Existe para que o
código tenha um caminho único — o resto da aplicação nunca lida com
`Option<GroupId>`."* O código materializa isso literalmente:
`Workspace::new` cria `GroupId::new(0)` e `new_tab` insere sempre em
`self.groups[0]`.

Mas o cenário de aceite do PRD-002 é *"Dado o grupo 'api' com três abas **na
posição central da barra**"*, e a restrição 3 do próprio ADR-0006 é *"grupos são
contíguos na barra; a ordem visual é a ordem do modelo"*. Com **um** nó
implícito, abas soltas à esquerda e à direita de um grupo explícito caem no mesmo
nó, e um `Vec<Group>` não consegue representar isso sem que a ordem visual deixe
de ser a ordem do `Vec`. É uma contradição interna do ADR-0006, não uma lacuna
de implementação.

**2. Colapso é classificado como assunto de desenho.** A tabela de operações do
ADR-0006 traz *"`collapse_group(id, bool)` | Só afeta o desenho, não a
estrutura"*. O RF-2.15 diz o contrário: *"Abas de grupo colapsado **não
participam** da navegação sequencial nem do acesso por índice"*, e o
[catálogo de ações](../reference/acoes.md) já assume isso ao definir `tab.goto_N`
(*"abas de grupo colapsado não contam"*). Isso não é desenho: é a definição de
`Workspace::visual_order()`, que hoje é simultaneamente a ordem de desenho e a
ordem de navegação, e é a base de `next_tab`, `prev_tab` e
`tab_at_visual_index`. Há uma consequência que nenhum documento enfrentou:
**colapsar um grupo renumera os atalhos de índice.**

**3. O terceiro nível do RF-1.5 não tem direção.** *"Ao fechar uma aba, o foco
vai para a aba seguinte do mesmo grupo; não havendo, para a anterior do mesmo
grupo; não havendo, para a aba mais próxima do grupo adjacente."* Qual grupo
adjacente, o da direita ou o da esquerda? E se ele estiver colapsado? A F2
deixou o nível 3 inerte, com o motivo escrito em `workspace.rs`: com um grupo
só, não havia segundo grupo alcançável. O RF-2.14 repete a ambiguidade em outras
palavras — *"a aba visível mais próxima fora dele"*.

**4. RF-2.7 e RF-2.8 se contradizem.** RF-2.7: *"Um grupo cuja última aba é
fechada ou movida para fora é removido automaticamente."* RF-2.8: *"O usuário
cria um grupo vazio e nele abre uma aba nova diretamente."* Se grupo vazio não
sobrevive, o segundo requisito descreve algo que o primeiro apaga.

**5. A paleta não cobre a métrica.** RF-2.4 manda atribuir *"a próxima cor ainda
não usada na janela"*, a paleta tem **seis** cores, e a métrica de sucesso do
PRD-002 é **dez grupos por janela**. Do sétimo grupo em diante não existe "cor
ainda não usada", e nada diz o que fazer.

Além dos cinco, dois requisitos pedem estado que o modelo não tem: RF-2.21
(`group.next`/`group.prev` ativando *"a última aba visitada daquele grupo"*) e o
overflow da barra com grupos colapsados, que o ADR-0006 lista como mitigação do
risco de barra cheia sem dizer como as duas coisas se compõem.

## Decisão

**Grupos explícitos e implícitos são o mesmo tipo com um discriminante, o
`Vec<Group>` admite vários nós implícitos, e colapso passa a ser estrutura: há
duas ordens, a visual e a navegável.**

### 1. Multiplicidade do grupo implícito

`Group` ganha um discriminante `GroupKind::{ Implicit, Explicit(GroupMeta) }`,
onde `GroupMeta` carrega nome, cor e estado de colapso. Um `Workspace` tem
**zero ou mais** grupos implícitos — um por *run* contíguo de abas sem grupo —
e cada um tem `GroupId` próprio.

A promessa central do ADR-0006 se mantém: **o resto da aplicação nunca lida com
`Option<GroupId>`.** Toda aba pertence a exatamente um grupo, e o caminho de
código continua único. O que muda é a cardinalidade, não a existência.

Regras de manutenção dos runs, todas dentro de `porecatu-core` e testadas:

- **Divisão.** Criar um grupo explícito a partir de abas de um run implícito
  parte o run em até dois: o pedaço antes e o pedaço depois. Pedaço vazio não é
  criado.
- **Fusão.** Dois runs implícitos adjacentes se fundem imediatamente no de
  menor índice visual. Dissolver um grupo explícito entre dois runs implícitos
  produz um run só, o que é exatamente o que o RF-2.6 pede (*"as abas voltam ao
  grupo implícito, mantendo a ordem relativa e a posição onde o grupo estava"*).
- **Identidade.** `GroupId` de grupo implícito **não é identidade estável**: é
  válido dentro da sessão em execução e não atravessa gravação. A sessão
  ([ADR-0005](0005-persistencia-de-sessao.md)) grava, por grupo, apenas o que a
  lista dela já prevê — `id`, nome, cor, colapso — e para grupo implícito grava
  a **ausência** de metadados; a restauração reconstrói os runs a partir da
  ordem, gerando `GroupId` novos. `GroupId` de grupo **explícito** segue
  estável e serializado, como o ADR-0006 exige.

`Workspace::new_tab` deixa de escrever em `groups[0]`: passa a receber o grupo
de destino, como a tabela do ADR-0006 já previa (`new_tab(group, pos)`), com o
grupo da aba ativa como default do chamador.

### 2. Duas ordens: visual e navegável

- `visual_order()` — todas as abas, na ordem do modelo. É o que a barra desenha
  e o que a sessão grava. **Não muda de definição.**
- `navigable_order()` — `visual_order()` menos as abas de grupo colapsado. É o
  que `tab.next`, `tab.prev` e `tab.goto_N` usam.

**`tab.goto_N` renumera quando um grupo colapsa, e isso é deliberado.** O
RF-1.12 diz que o índice é *"sobre a ordem visual da janela toda, não por
grupo"*, e o RF-2.15 tira as abas colapsadas do acesso por índice: a única
leitura consistente das duas frases é que `Alt+3` vai sempre para a terceira aba
**alcançável**. A alternativa — índice estável sobre abas invisíveis — daria a
`Alt+3` um alvo que o usuário não vê, o que é pior que renumerar.

Isso corrige a linha do ADR-0006 que classifica `collapse_group` como *"só afeta
o desenho, não a estrutura"*: colapso afeta a ordem navegável, e por isso vive
em `porecatu-core`, não em `porecatu-ui`.

### 3. Foco depois de fechar e depois de colapsar

O RF-1.5 ganha a mesma preferência em todos os níveis — **seguinte, depois
anterior** — aplicada em escopo crescente:

1. Aba seguinte no mesmo grupo.
2. Aba anterior no mesmo grupo.
3. Primeira aba do grupo **seguinte** que esteja alcançável, percorrendo à
   direita.
4. Última aba do grupo **anterior** que esteja alcançável, percorrendo à
   esquerda.
5. Nenhuma: o workspace ficou sem aba ativa (e, na prática, sem abas).

"Alcançável" significa expandido. **Grupo colapsado é pulado, nunca expandido
automaticamente.** Expandir sozinho desfaria uma escolha explícita do usuário
para um evento — fechar uma aba — que não tem relação com o grupo colapsado.

O RF-2.14 (colapsar o grupo que contém a aba ativa) usa a mesma escada a partir
do grupo colapsado, começando no nível 3. O RF-2.17 (*"ativar uma aba de grupo
colapsado expande o grupo"*) continua valendo: ele descreve o caminho inverso —
alguém pede aquela aba nominalmente —, e aí expandir é o que o usuário quis.

### 4. Grupo vazio existe, mas não sobrevive a uma interação

`group.new_tab` sobre um grupo recém-criado é **uma operação**: cria o grupo e a
aba no mesmo passo, e o estado intermediário nunca é observável pela UI. O
RF-2.7 continua literal — grupo que perde a última aba é removido no mesmo
`close_tab`. Assim `tab_bar::layout` pode seguir descartando grupo vazio do
layout, como já faz, sem que isso esconda um estado válido.

### 5. Paleta esgotada: repete, na mesma ordem

Passado o sexto grupo, a atribuição automática **recomeça a paleta**, escolhendo
a cor **menos usada** na janela e, em empate, a de menor índice na paleta. Cor
definida à mão — nomeada ou hexadecimal (RF-2.10) — **conta** como usada para
esse cálculo, porque o objetivo do requisito é que dois grupos vizinhos não
fiquem iguais, e a origem da cor é irrelevante para isso.

O RF-2.4 pede *"a próxima cor ainda não usada"*; com dez grupos e seis cores,
isso é impossível de cumprir ao pé da letra. A regra acima é a leitura que
preserva a intenção — distinguir — quando a letra não pode ser satisfeita.

### 6. Última aba visitada por grupo

`Group` (variante explícita e implícita) guarda um `last_active: Option<TabId>`,
atualizado por `activate_tab`. Se a aba registrada foi fechada, o campo cai para
`None` e `group.next`/`group.prev` ativam a **primeira** aba do grupo.

**Não é persistido.** A lista do ADR-0005 não o inclui, e a sessão já grava a
aba ativa por janela — que é o único ponto de retomada que o usuário percebe
depois de reabrir o app.

Grupo colapsado é **pulado** por `group.next`/`group.prev`, pela mesma razão do
item 3.

### 7. Colapso e overflow

A ordem de cedência da [especificação visual](../design/especificacao-visual.md)
§2.18 ganha um degrau **antes** da rolagem da trilha, e a pílula entra nela:

1. Rótulo da aba encolhe até o piso (comportamento atual).
2. Nome do grupo na pílula encolhe até o piso dele.
3. A trilha rola.

`overflow_state` passa a contar como oculta apenas aba **alcançável** que esteja
fora da vista: aba de grupo colapsado não é "fora da vista", é ausente da
trilha, e contá-la faria o indicador prometer abas que a rolagem nunca traria.

Dez grupos colapsados numa janela ocupam dez pílulas no piso, o que é o caso que
a métrica do PRD-002 mede.

## Alternativas consideradas

### Manter um grupo implícito único e deixar a ordem visual fora do `Vec<Group>`

Uma lista separada de posições resolveria o cenário "grupo no meio de abas
soltas" sem tocar na cardinalidade. Descartada porque a restrição 3 do ADR-0006
— *"a ordem visual é a ordem do modelo"* — é o que faz o layout da barra ser uma
função pura sobre o `Workspace`, e é o que torna o round-trip de sessão
verificável por igualdade estrutural. Duas fontes de verdade para ordem é a
classe de bug mais caro que este projeto pode comprar: reordenar por arraste
passaria a ter de manter as duas em sincronia.

### `Option<GroupId>` na aba, sem grupo implícito nenhum

O modelo mais direto, e o que a maioria dos emuladores usa. Descartada
explicitamente pelo ADR-0006 (*"o resto da aplicação nunca lida com
`Option<GroupId>`"*) e a F2 confirmou o benefício na prática: `layout`,
`hit_test`, `close_tab` e `move_tab` não têm um único ramo condicional de
"tem grupo?". Reverter isso agora trocaria uma cardinalidade nova por dezenas de
`if let Some(group)`.

### Índice de `tab.goto_N` estável, ignorando o colapso

`Alt+3` sempre na terceira aba da ordem visual, colapsada ou não, expandindo o
grupo se necessário. Plausível: nada renumera, e o RF-2.17 já prevê expandir ao
ativar. Descartada porque transforma um atalho de navegação em um comando que
muda o layout, e porque contraria o RF-2.15, que é explícito sobre acesso por
índice. Um atalho cujo alvo o usuário não vê é um atalho que ele não usa.

### Expandir o grupo adjacente ao fechar a última aba de um grupo

Evitaria o caso de "fechei uma aba e o foco pulou dois grupos". Descartada
porque colapso é uma escolha do usuário sobre o que ele quer ver, e fechar uma
aba em outro lugar da barra não é motivo para desfazê-la. Pular é previsível;
expandir é uma mudança de layout que o usuário não pediu.

### Bloquear a criação do sétimo grupo, ou deixá-lo sem cor

As duas leituras literais do RF-2.4. Descartadas porque a métrica de sucesso do
mesmo PRD é dez grupos: um requisito não pode inviabilizar a métrica do
documento que o contém. Grupo sem cor, além disso, quebra o RF-2.11 (a cor é
aplicada ao rótulo e ao indicador) e o RF-4.19 (tingimento do wrapper).

### Persistir o MRU por grupo

Restauraria a sessão num estado mais próximo do que o usuário deixou.
Descartada porque a lista do ADR-0005 é deliberadamente curta, o benefício é
invisível na prática — depois de reabrir o app, o usuário navega a partir da aba
ativa restaurada, não a partir de um histórico que não vê — e cada campo novo no
schema é uma migração a manter.

## Consequências

### Positivas

- O cenário de aceite *"desagrupar preserva ordem e posição"* passa a ser
  representável, e o *"grupo na posição central da barra"* também.
- A contradição interna do ADR-0006 (implícito singular versus contiguidade)
  desaparece sem que nenhuma das duas restrições seja abandonada.
- Colapso vira estrutura testável em `porecatu-core`, sem GPU e sem janela, no
  mesmo padrão do resto do crate.
- `visual_order()` continua sendo o que a barra desenha e o que a sessão grava:
  a ordem nova é derivada, não paralela.
- A ordem de cedência do overflow ganha um degrau antes da rolagem, o que é o
  que sustenta a métrica de dez grupos.

### Negativas

- `Group` deixa de ser um struct de dois campos: ganha discriminante e
  metadados, e as invariantes de fusão/divisão de runs implícitos são código
  novo que não existia.
- A manutenção dos runs é uma classe de bug nova: run implícito vazio esquecido
  no `Vec`, ou dois runs adjacentes que não se fundiram, quebram a contiguidade
  sem quebrar nenhum teste que já existe.
- `GroupId` passa a ter duas semânticas de estabilidade — estável para
  explícito, de sessão para implícito —, o que precisa estar dito no lugar onde
  alguém vai procurar (o doc do tipo).
- Renumerar `Alt+1..9` ao colapsar é comportamento que surpreende na primeira
  vez, mesmo sendo a leitura consistente dos requisitos.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Run implícito vazio sobrevivendo no `Vec<Group>` | Alta | Médio | Invariante testada: nenhum grupo implícito vazio existe depois de qualquer operação. Teste sobre a sequência completa de operações, não só sobre cada uma isolada |
| Dois runs implícitos adjacentes não se fundirem | Alta | Médio | Invariante testada: não existem dois grupos implícitos adjacentes na ordem visual |
| `GroupId` implícito vazar para a sessão como identidade | Média | Alto | O tipo gravado pela sessão não tem campo de `id` para grupo implícito; teste de round-trip com grupo implícito no meio, comparando por estrutura e não por `id` |
| Duas ordens divergirem (aba na navegável que não está na visual) | Média | Alto | `navigable_order()` é derivada de `visual_order()` por filtro, nunca construída em paralelo; teste de que é sempre subsequência |
| Renumeração de `Alt+1..9` ser lida como bug | Alta | Baixo | Documentado aqui e no catálogo de ações; é a leitura consistente de RF-1.12 com RF-2.15 |
| Foco parar em grupo colapsado por um caminho esquecido | Média | Médio | A escada do item 3 é uma função só, usada por `close_tab` e por `collapse_group`; teste com grupo colapsado em cada uma das duas direções |
| Cor repetida ficar adjacente com dez grupos | Média | Baixo | Empate resolvido pelo menor índice da paleta, o que espalha as repetições; o usuário pode trocar à mão (RF-2.10) |
