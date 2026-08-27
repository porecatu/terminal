# ADR-0023 — Editor de grupo, o quinto widget de chrome

**Status:** Aceito
**Data:** 2026-08-27
**Relacionados:** [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0019](0019-tooltip.md), [ADR-0020](0020-grupos-explicitos.md), [PRD-002](../prd/prd-002-grupos-de-abas.md), [PRD-010](../prd/prd-010-interacao-e-superficie-de-app.md)

## Contexto

O [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) decidiu **três** widgets
próprios: aviso, diálogo de confirmação e menu de contexto. O
[ADR-0019](0019-tooltip.md) acrescentou o quarto, porque o RF-1.10 exigia
tooltip e ninguém tinha notado. A F3 traz o quinto, pela mesma via.

O **editor de grupo** está desenhado (especificação visual §2.10), classificado
`[v1]` na tabela de fases, e é o que o **RF-2.22** exige: *"Menu de contexto do
grupo oferece: renomear, mudar cor, colapsar/expandir, nova aba no grupo, fechar
todas as abas do grupo (com confirmação), desagrupar."* O RF-10.21 amarra os
dois: *"O menu de contexto do grupo e o editor de grupo oferecem exatamente a
mesma lista de ações, lida de uma definição única."*

O ADR-0014 menciona o editor uma vez, e para **descartá-lo** como substituto do
menu de contexto: *"O editor já existe no design e cobre quatro dos seis itens do
RF-2.22. Descartada como substituto porque menu de contexto precisa abrir no
cursor, sobre qualquer aba, com itens que variam por alvo — não é a mesma coisa
que um painel ancorado num grupo."* Correto, e ainda vale — mas decidir o que o
editor **não** é não decide o que ele é.

Quatro pontos abertos:

**1. Camada.** A tabela do [ADR-0018](0018-composicao-de-frame.md) tem cinco
camadas fixas, e o próprio ADR declara a lista fechada: *"camada nova exige
requisito novo, como ação nova exige entrada no catálogo"*. A camada 4 é
descrita como *"menu de contexto e tooltip"*; o editor não está em nenhuma.

**2. Modo de captura.** O [ADR-0008](0008-teclas-e-roteamento-de-input.md) prevê
captura para *"renomear aba/grupo, busca"* — um campo de texto. O editor tem
campo de texto **mais** seis swatches de cor **mais** quatro ações, e a cadeia de
roteamento não diz o que `Tab` e as setas fazem lá dentro.

**3. Duas superfícies de rename com semântica incompatível.** O RF-2.9 pede
*"edição inline, `Enter` confirma, `Esc` cancela"*, e o RF-2.4 diz que o grupo
nasce *"com nome vazio, em modo de edição inline"*. Mas a §2.10 especifica o
oposto para o campo do editor: *"**edição ao vivo: o nome muda na barra enquanto
se digita**"*. Com edição ao vivo, "`Esc` cancela" só funciona se alguém guardar
o valor anterior — e nada diz que guarda. Também não está decidido se a edição
inline na pílula e o campo do editor são a mesma implementação, nem o que
`group.rename` abre.

**4. Seletor de destino de `tab.move_to_group`.** O RF-2.20 pede mover *"por
menu de contexto, sem usar o mouse para arrastar"*, e a ação é marcada `Arg` no
[catálogo](../reference/acoes.md) — precisa de um alvo que a tecla não tem. Mas a
§2.16 decide que o menu **não rola** e tem largura máxima 320; com dez grupos e
nomes longos, uma lista plana estoura. Submenu não existe em documento nenhum.
Mesma lacuna em `group.set_color` a partir do menu de grupo, onde não há os seis
swatches do editor.

## Decisão

**O editor de grupo é o quinto widget de chrome: um popover na camada 4, com
captura de teclado própria, e é a única superfície de escolha de cor e de destino
no v1. A edição de nome é uma implementação só, com edição ao vivo e valor
anterior guardado para o `Esc`.**

### 1. Camada e natureza

O editor entra na **camada 4 (popover)**, ao lado do menu de contexto e do
tooltip. A tabela do ADR-0018 passa a ler *"menu de contexto, tooltip e editor de
grupo"* — é acréscimo de conteúdo a uma camada existente, não camada nova, e
portanto não fura a regra que o ADR-0018 declarou.

É **popover, não modal**:

- Fecha em clique fora, `Esc` ou perda de foco da janela — as três condições que
  o RF-10.19 já impõe ao menu de contexto.
- **Não** tem overlay escurecendo o fundo. Overlay é da camada 5 e é a marca do
  diálogo de confirmação; usá-lo aqui daria a um painel de edição o peso visual
  de uma ação destrutiva.
- **Nunca coexiste com o menu de contexto.** Abrir um fecha o outro. Os dois
  ocupam a mesma camada e oferecem a mesma lista de ações (RF-10.21); dois
  popovers com os mesmos itens abertos ao mesmo tempo é ambiguidade sem ganho.

O diálogo de confirmação do RF-2.23 (*"fechar grupo, com a contagem"*) abre
**sobre** o editor, na camada 5, e o editor permanece aberto atrás dele — se o
usuário cancelar, volta para onde estava.

### 2. Captura de teclado

O editor é um **modo de captura** do ADR-0008, no mesmo passo que o rename de
aba, e consome tudo exceto as teclas de navegação dele próprio:

| Tecla | Efeito |
|---|---|
| Texto, `Backspace` | Editam o nome (o campo tem foco ao abrir, §2.10) |
| `Tab` / `Shift+Tab` | Avançam e retrocedem entre as três regiões: campo, faixa de swatches, lista de ações |
| Setas | Dentro da faixa de swatches, movem a seleção de cor; dentro da lista de ações, movem o realce |
| `Enter` | No campo, confirma e fecha. Na faixa ou na lista, aciona o que está realçado |
| `Esc` | Cancela: restaura o nome anterior e fecha |

**Hover e foco por teclado são o mesmo realce e mutuamente exclusivos**, como a
§2.16 já decide para o menu de contexto: mover o mouse move o realce e limpa o
foco por teclado, e vice-versa. Um realce por vez.

O estado do editor é puro e testável sem `winit`, no padrão que a F2 estabeleceu
em `rename.rs`, `dialog.rs`, `context_menu.rs` e `tooltip.rs`.

### 3. Uma implementação de edição de nome, com valor anterior

`group.rename` **abre o editor**, não um campo inline solto. O campo inline
sobre a pílula que o RF-2.4 pede no nascimento do grupo é o **mesmo componente**
do campo do editor, renderizado no lugar da pílula em vez de dentro do popover.

Semântica única, que reconcilia RF-2.9 e §2.10:

- **Edição ao vivo:** cada tecla atualiza o nome do grupo, e a barra reflete
  imediatamente. É o que a §2.10 especifica e é o comportamento mais legível —
  o usuário vê o nome crescer na pílula enquanto digita.
- **O valor anterior é guardado** ao entrar em edição. `Esc` restaura e fecha;
  `Enter` confirma e fecha. É o que o RF-2.9 pede, e é a única forma de as duas
  frases coexistirem.
- **Nome vazio é válido** (RF-2.9: *"o grupo aparece apenas como um marcador
  colorido"*). Sair com o campo vazio não cancela nem reverte.
- Ao **criar** o grupo (RF-2.4), o valor anterior é a string vazia: `Esc` deixa
  o grupo criado e sem nome, e **não** desfaz a criação. Desagrupar é uma ação
  própria (`group.dissolve`), não o efeito colateral de uma tecla de cancelar.

O buffer não tem posição de cursor no meio da string — sempre no fim —, a mesma
simplificação que o rename de aba da F2 assumiu e que a pintura do caret já
supõe.

### 4. O editor é o seletor de cor e de destino

**`group.set_color` e `tab.move_to_group`, quando invocadas por menu, abrem uma
superfície de escolha em vez de executar direto.** Nenhum submenu é introduzido
no v1:

- `group.set_color` a partir do menu de grupo **abre o editor**, com o foco na
  faixa de swatches. Os seis swatches da §2.10 são a única superfície de cor do
  v1, e a entrada por hexadecimal do RF-2.10 fica **diferida** — registrada como
  nota de fase no PRD-002, não como lacuna esquecida.
- `tab.move_to_group` abre um **popover de destino**: a mesma anatomia do menu
  de contexto (§2.16), com uma linha por grupo — swatch, nome truncado, contagem
  — mais "Novo grupo" no fim. Ele **rola**, e é a exceção explícita à regra
  *"o menu não rola"* da §2.16: aquela regra vale para listas de ação, cujo
  tamanho é conhecido em tempo de escrita, e esta lista é do tamanho do número
  de grupos do usuário.

O item "Mover para grupo" no menu de aba deixa de ser esmaecido (como está na
F2) e passa a abrir esse popover.

## Alternativas consideradas

### Editor como modal na camada 5, com overlay

Daria foco absoluto à edição e reaproveitaria o overlay do diálogo. Descartada
porque o editor edita **enquanto se vê o resultado**: a §2.10 exige que o nome
mude na barra ao digitar, e um overlay escurecendo a barra tornaria isso ilegível.
Overlay também é o sinal visual do diálogo destrutivo, e editar nome e cor não é
destrutivo.

### Editor e menu de contexto coexistindo

Permitiria abrir o menu sobre uma aba com o editor de outro grupo aberto.
Descartada porque os dois oferecem a mesma lista de ações por RF-10.21: com os
dois abertos, `Enter` tem dois alvos plausíveis e o usuário não sabe qual
responde. Fechar um ao abrir o outro custa uma linha e remove a ambiguidade.

### Submenu no menu de contexto para grupo de destino e para cor

O que a maioria dos apps faz, e não exigiria popover novo. Descartada porque
submenu não existe em nenhum documento do projeto: precisaria de anatomia
própria (atraso de abertura, direção de flip, navegação por seta para dentro e
para fora), o que é mais desenho novo do que reaproveitar a anatomia do menu que
já existe com rolagem.

### Campo inline na pílula e campo do editor como implementações separadas

Cada um otimizado para o seu lugar. Descartada porque são o mesmo problema —
editar uma string curta com `Enter`/`Esc` — e duas implementações divergem: uma
ganha edição ao vivo, a outra não, e o usuário encontra semânticas diferentes
para a mesma tarefa em dois lugares. É exatamente o risco que o RF-10.21 tenta
evitar do lado das ações.

### `Esc` no nascimento do grupo desfazer a criação

Leitura literal de "`Esc` cancela": o grupo não deveria existir. Descartada
porque o RF-2.4 descreve **duas** coisas num gesto — criar o grupo e nomeá-lo —,
e `Esc` cancelaria a segunda. Desfazer a primeira exigiria devolver as abas aos
runs implícitos de origem, o que é `group.dissolve` com outro nome; e o usuário
que quer desfazer tem essa ação no menu.

### Entrada de cor por hexadecimal no editor, como o RF-2.10 pede

Cumpriria o requisito por inteiro. Descartada para o v1 porque exigiria um campo
de texto a mais, validação de hex com erro visível, e uma decisão de desenho que
o §2.10 não tem. A paleta de seis cores é configurável (RF-4.18): quem quer outra
cor a coloca na config, o que atende o caso real sem interface nova.

## Consequências

### Positivas

- O RF-2.22 e o RF-10.21 ficam implementáveis, e a lista única de ações fica com
  um dono claro.
- Nenhuma camada nova em `porecatu-render`: a regra do ADR-0018 continua
  fechada, com um conteúdo a mais na camada 4.
- Uma implementação de edição de nome atende RF-2.4, RF-2.9 e a §2.10 sem que
  nenhuma das três precise de exceção.
- `tab.move_to_group` sai de esmaecido sem introduzir submenu, reaproveitando a
  anatomia do menu de contexto.
- O estado do editor é puro e testável sem janela, como os quatro widgets da F2.

### Negativas

- É o quinto widget de chrome, e o mais complexo: três regiões navegáveis contra
  uma lista linear dos outros quatro.
- `group.set_color` e `tab.move_to_group` deixam de executar quando invocadas e
  passam a abrir superfície — comportamento que o catálogo de ações precisa
  registrar, sob pena de alguém as vincular a tecla esperando efeito direto.
- O popover de destino rola, e é a primeira lista rolável do chrome: precisa de
  recorte, que existe desde o ADR-0018, mas também de indicação de que há mais
  itens.
- A entrada por hexadecimal do RF-2.10 fica cumprida só pela config, o que
  precisa de nota de reconciliação no PRD-002.
- Falta valor de desenho para duas coisas que este ADR cria: a posição real do
  popover (a §2.10 fixa `top: 76px`, que pressupõe a barra de título `[v2]`) e o
  campo inline sobre a pílula. As duas vão para a especificação visual junto com
  a F3.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| `Esc` perder o nome digitado por o valor anterior não ter sido guardado | Média | Médio | O valor anterior é capturado na construção do estado de edição, não na primeira tecla; teste de que `Esc` restaura exatamente o nome de entrada |
| Editor e menu abertos ao mesmo tempo por um caminho esquecido | Média | Médio | Abrir qualquer um dos dois fecha o outro num ponto só do código; teste do estado de `WindowState` com as duas aberturas em sequência |
| `top: 76px` desenhar o editor sobre a barra ou fora da janela | Alta | Médio | Posição derivada da altura real da barra, com a regra de flip do §2.16; valor novo registrado na especificação antes de implementar |
| Popover de destino com dez grupos não caber e não indicar rolagem | Média | Baixo | Reaproveita o indicador de conteúdo oculto da §2.18, que já existe para a trilha |
| Alguém vincular `group.set_color` a tecla esperando efeito direto | Média | Baixo | A ação já é marcada `Arg` e não é vinculável; a nota do catálogo passa a dizer que ela abre o editor |
| Usuário não achar como usar cor fora da paleta | Média | Baixo | Nota de reconciliação no RF-2.10 e comentário na chave `palette` do exemplo de config |
