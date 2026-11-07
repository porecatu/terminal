# PRD-022 — Abas e grupos entre janelas

**Status:** Aprovado
**Data:** 2026-11-07
**Requisito de origem:** pedido direto do dono do produto, e o item que três PRDs deixaram em "Fora de escopo": PRD-000 (*"Mover abas entre janelas por drag. O modelo permite; a UI fica para depois."*), [PRD-001](prd-001-abas.md) (*"Mover abas entre janelas por drag"*) e [PRD-002](prd-002-grupos-de-abas.md) (*"Mover grupo entre janelas"*). O [PRD-010](prd-010-interacao-e-superficie-de-app.md) o repetia no RF-10.24, e o [ADR-0015](../adr/0015-multiplas-janelas.md) o deixou na coluna "Fora do v1", exigindo ADR novo.
**Relacionados:** [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md), [ADR-0007](../adr/0007-modelo-de-threading.md), [ADR-0015](../adr/0015-multiplas-janelas.md), [ADR-0020](../adr/0020-grupos-explicitos.md), [ADR-0021](../adr/0021-selecao-multipla-e-gestos-da-barra.md), [ADR-0023](../adr/0023-editor-de-grupo.md), [ADR-0036](../adr/0036-formato-do-arquivo-de-sessao.md), [ADR-0037](../adr/0037-aba-nao-iniciada.md), [ADR-0053](../adr/0053-paineis-divididos.md), [ADR-0076](../adr/0076-reconexao-automatica.md), [ADR-0086](../adr/0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md), [ADR-0087](../adr/0087-zona-de-abas-fixadas-na-barra.md), [ADR-0090](../adr/0090-transferencia-de-abas-entre-janelas.md), [ADR-0091](../adr/0091-arraste-entre-janelas.md), [PRD-001](prd-001-abas.md), [PRD-002](prd-002-grupos-de-abas.md), [PRD-003](prd-003-persistencia-de-sessao.md), [PRD-006](prd-006-paineis-divididos.md), [PRD-010](prd-010-interacao-e-superficie-de-app.md), [PRD-020](prd-020-conexoes-ssh.md), [PRD-021](prd-021-abas-fixadas.md)

> Aprovado em 2026-11-07, por decisão do dono do produto, **fora da ordem de fases**, como os outros recursos pedidos depois da F6. O dono tomou as decisões de comportamento abaixo antes da escrita: soltar fora de toda janela cria uma janela nova; o fantasma salta de barra em barra, sem janela flutuante; a aba fixada, a seleção múltipla e um caminho sem mouse entram junto; e a janela que fica vazia fecha sem perguntar. Dois ADRs dizem o como: a transferência entre janelas ([ADR-0090](../adr/0090-transferencia-de-abas-entre-janelas.md)) e o gesto de arraste ([ADR-0091](../adr/0091-arraste-entre-janelas.md)). A barra não ganha anatomia nova e nenhum valor visual é inventado, mas o fantasma passa a aparecer em outra janela, e o aval visual do dono fica pendente até isso existir no binário.

## Problema

Com o [ADR-0015](../adr/0015-multiplas-janelas.md), o Porecatu passou a ter várias janelas, cada uma com seus grupos e abas. A ideia é que o usuário separe o trabalho por janela, por exemplo uma por monitor ou uma por projeto. O que essa separação não permite é **corrigir**:

- **Uma aba aberta na janela errada fica nela para sempre.** Para levá-la à outra janela, o usuário abre uma aba nova lá, refaz o `cd`, sobe o programa de novo e fecha a original. O servidor que estava rodando é reiniciado, o scrollback se perde e a conexão SSH autentica de novo.
- **Um grupo não pode ganhar janela própria.** O caso típico é um grupo que cresceu e mereceria um monitor só dele. O usuário teria de recriar o grupo inteiro na janela nova, aba por aba.
- **Juntar duas janelas é refazer tudo.** Duas janelas abertas por engano, ou uma sessão nomeada restaurada ao lado do trabalho do dia ([PRD-014](prd-014-sessoes-nomeadas.md)), só se juntam fechando uma delas e reabrindo as abas na outra.

Nos navegadores, o gesto que resolve isso é arrastar a aba para outra janela, ou para fora de todas, e os usuários já o conhecem. No Porecatu, até agora, soltar a aba fora da barra cancelava o arraste.

## Usuário-alvo

O mesmo do [PRD-000](prd-000-visao-de-produto.md), no caso em que trabalha com **mais de uma janela**: dois monitores, uma janela por projeto, ou uma sessão nomeada aberta ao lado da do dia. Quem usa uma janela só não vê nada novo, exceto o arraste da seleção múltipla (RF-22.17) e o "Nova janela" do popover (RF-22.22).

## Em uma tela

```
 janela A                                        janela B
┌──────────────────────────────────────────┐    ┌─────────────────────────────────────┐
│ [◆] ( api ▾ [server] [·logs·] ) [+]      │    │ [◆] ( web ▾ [vite] [▒▒▒] [test] )   │
│                         └─ buraco        │    │                  └─ vão: "logs" cai aqui
│                                          │    │              ┌────────┐             │
│                                          │    │              │  logs  │ ← fantasma  │
│                                          │    │              └────────┘             │
└──────────────────────────────────────────┘    └─────────────────────────────────────┘

 soltar sobre a barra de B → "logs" passa para o grupo "web", com o processo vivo
 soltar no desktop         → "logs" ganha uma janela própria, no ponto da soltura
 soltar no corpo de A ou B → nada muda
```

## Requisitos funcionais

### O que viaja

**RF-22.1** — Podem ir de uma janela a outra: uma **aba** (solta ou de grupo), uma **seleção de abas** (RF-2.1), um **grupo explícito inteiro** e uma **aba fixada** ([PRD-021](prd-021-abas-fixadas.md)). Painéis não viajam sozinhos; vão sempre com a aba deles (RF-22.27).

**RF-22.2** — Mover **nunca reinicia nada**. O processo de cada painel continua vivo, e nenhum byte de saída se perde no caminho. Uma aba SSH continua no mesmo canal da mesma conexão, sem pedir autenticação de novo ([PRD-020](prd-020-conexoes-ssh.md)).

**RF-22.3** — Vai junto tudo o que é **da aba**:
- a árvore de painéis com as proporções e o painel focado;
- o scrollback e o estado do motor de cada painel;
- o título customizado e o perfil de cada painel;
- o estado da aba: viva, encerrada, não iniciada ([ADR-0037](../adr/0037-aba-nao-iniciada.md)), SSH dormente ou reconectando ([ADR-0076](../adr/0076-reconexao-automatica.md));
- os indicadores de atividade e de campainha;
- a pergunta pendente do painel, quando houver (segredo SSH, chave de host desconhecida);
- a marca de que o `.porecatu` já rodou. Mover não o roda de novo (RF-12.9).

**RF-22.4** — Fica para trás o que é **da janela** e do gesto em curso: barra de busca aberta, rename em curso, menu, popover, hover e tooltip. Zoom e tema são do processo, então nada muda neles. Se o quadro do destino tiver outro tamanho, a grade é redimensionada como num redimensionamento de janela comum, e o programa recebe o novo tamanho. Isso inclui o caso de um monitor com outra escala de DPI.

**RF-22.5** — Um grupo leva nome, cor, estado de colapso e a ordem das abas. A janela de destino pode já ter um grupo com o mesmo nome. Os dois **não se fundem**: grupos são por janela ([ADR-0015](../adr/0015-multiplas-janelas.md)), e fundir por nome seria adivinhar.

**RF-22.6** — Uma aba fixada chega fixada na zona do destino, quando cai na zona, ou desafixada na trilha, quando cai na trilha. A trilha segue as regras de desafixar por arraste (RF-21.10). Ela continua na lista de fixadas e abre na primeira janela no próximo arranque, como toda fixada (RF-21.13, RF-21.26).

### Arrastar para outra janela

**RF-22.7** — Com uma aba ou um grupo em arraste, **a barra de abas de outra janela do Porecatu é um destino**. Valem as mesmas regras de destino da barra de origem: entre abas, dentro de um grupo, fora de todos, sobre a pílula, sobre um grupo colapsado e na zona de fixadas (RF-1.16, RF-2.18, RF-2.19, RF-21.7, RF-21.10). O grupo só cai em fronteira entre grupos, nunca dentro de outro (RF-2.19), e não cai na zona de fixadas.

**RF-22.8** — Enquanto o cursor está sobre a barra de outra janela, **o fantasma aparece nela**: segue o cursor, abre o vão onde vai cair, realça o grupo de destino e rola a trilha nas pontas, como na barra de origem. A barra de origem continua mostrando o buraco de onde a aba saiu.

**RF-22.9** — Fora de qualquer barra, **o fantasma fica preso à barra de origem**, seguindo o cursor no eixo X como hoje. Não existe janela flutuante. O cursor de arraste (`Grabbing`) continua em todo lugar onde o sistema deixa o app escolher o cursor.

**RF-22.10** — Ao soltar sobre a barra de outra janela, a aba passa para ela. **A janela de destino vem para a frente**, recebe o foco, e a aba arrastada fica ativa nela. Num grupo, a aba ativa do destino passa a ser a que estava ativa no grupo. A seleção é limpa nas duas janelas.

**RF-22.11** — Entre janelas que se sobrepõem, o destino é a que o usuário **usou por último**. Janelas minimizadas não são destino.

### Destacar numa janela nova

**RF-22.12** — Soltar **fora de toda janela do Porecatu** (sobre o desktop, a barra de tarefas ou outro app) **cria uma janela nova** no ponto da soltura, com o que foi arrastado dentro:
- uma aba ou uma seleção vira aba solta, fora de todo grupo, como no RF-1.16 para "fora de todos os grupos";
- um grupo vem inteiro, como grupo;
- uma fixada vem fixada, na zona da janela nova.

**RF-22.13** — Soltar **dentro de uma janela do Porecatu, mas fora da barra** (sobre o terminal, a barra de status ou a tela de configurações) **cancela**, como hoje. A aba volta à origem.

**RF-22.14** — A janela nova:
- tem o tamanho da janela de origem quando ela não está maximizada;
- abre posicionada para que **o ponto por onde o usuário segurava a aba fique sob o cursor**, dentro da área útil do monitor onde está o cursor;
- abre **sem** aba nova de shell e **sem** abrir as fixadas da lista (RF-21.13): ela nasce só com o que foi arrastado;
- recebe o foco.

**RF-22.15** — Destacar a **única** aba de uma janela é permitido. O efeito é o mesmo de qualquer destacar, e a origem vazia fecha (RF-22.19).

### Cancelar

**RF-22.16** — `Esc` cancela o arraste em qualquer ponto, com o fantasma em qualquer janela. A janela de origem perder o foco (`Alt+Tab` com o botão apertado, por exemplo) também cancela. A aba volta à origem e nada muda.

### Seleção múltipla

**RF-22.17** — **Arrastar uma aba selecionada leva todas as abas selecionadas**, dentro da mesma janela e entre janelas:
- elas chegam contíguas no destino, na ordem visual em que estavam, e saem dos grupos de origem (grupo esvaziado é removido, RF-2.7);
- o fantasma é a aba que o usuário segurou. As outras selecionadas viram buraco na origem, e o vão no destino abre para todas;
- soltas na zona de fixadas, todas são fixadas, na ordem, na posição do vão (RF-21.7). Fixadas não entram em seleção (RF-21.3), então nunca vão junto;
- arrastar uma aba **não** selecionada leva só ela, e a seleção é limpa, como hoje (RF-2.3).

**RF-22.18** — Pressionar uma aba selecionada **não limpa a seleção no press**, porque o usuário pode estar começando a arrastá-la. Soltar sem ter arrastado limpa a seleção e ativa a aba, como o RF-2.3 manda. Isso muda só o **quando**: o resultado de um clique sem arraste é o mesmo de hoje.

### A janela que fica vazia

**RF-22.19** — Quando a última aba de uma janela sai dela, por arraste ou pelo popover, **a janela de origem fecha sem pedir confirmação**. Nada foi encerrado, e o RF-10.23 confirma o fim de processos, não a mudança de lugar. Abas fixadas contam como abas (RF-21.25): uma janela que ainda tem fixada não fecha.

### Sem mouse

**RF-22.20** — O menu de contexto da aba ganha o item **"Mover para janela…"**. O menu do grupo e o editor de grupo também o ganham, como o mesmo item da lista única que os dois compartilham (RF-10.21).

**RF-22.21** — O item abre um **popover de destino**, com a mesma anatomia e o mesmo comportamento do popover de "Mover para grupo" (RF-2.20, [ADR-0023](../adr/0023-editor-de-grupo.md) §4). Cada linha é uma das **outras** janelas de terminal, identificada pelo título da aba ativa dela e pela contagem de abas. A ordem é a de uso mais recente.

**RF-22.22** — A última linha do popover é sempre **"Nova janela"**. Com uma janela só aberta, ela é a única linha. Escolhê-la equivale a destacar (RF-22.12, RF-22.14), com a janela nova na posição em cascata de `window.new` (RF-10.22).

**RF-22.23** — Escolher uma janela no popover põe o que foi movido **no fim da trilha** dela: a aba como aba solta, o grupo como grupo. Uma fixada vai para o **fim da zona** e continua fixada. A janela de destino vem para a frente, como no RF-22.10.

**RF-22.24** — Duas ações entram no catálogo, **sem atalho default**, como `group.new_tab`:
- **`tab.move_to_window`** abre o popover para a aba ativa, ou para a seleção quando houver uma;
- **`group.move_to_window`** abre o popover para o grupo da aba ativa. Quando a aba ativa não está em grupo explícito, não faz nada, e o item do menu aparece esmaecido (RF-10.20).

### Persistência

**RF-22.25** — A sessão automática registra o novo arranjo como qualquer outra mudança estrutural, com a gravação adiada de sempre (RF-3.2, RF-3.3). O formato do arquivo de sessão não muda: a aba é gravada na janela onde está.

**RF-22.26** — Mover uma fixada entre janelas **não muda** o retrato dela no `pinned.json` (RF-21.9). O arquivo só é reescrito quando o movimento muda a **ordem** da lista, pela regra de ordem por janela do [ADR-0086](../adr/0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md) §2, porque reordenar é gesto (RF-21.28).

### Fronteiras

**RF-22.27** — Painel continua **não se movendo** entre abas nem entre janelas (RF-6.25). Este PRD move abas inteiras, não pedaços delas.

**RF-22.28** — O arraste atravessa só **janelas do mesmo processo**. Soltar sobre a janela de **outra instância** do Porecatu é soltar fora de toda janela deste processo, e destaca (RF-22.12).

**RF-22.29** — No **Wayland**, o arraste **não atravessa janelas**: o protocolo não informa ao app a posição das janelas nem a do cursor fora delas. Lá, soltar fora da barra de origem cancela como antes, e o popover (RF-22.20) é o caminho para mover entre janelas e para destacar. A razão técnica está no [ADR-0091](../adr/0091-arraste-entre-janelas.md).

## Cenários

```gherkin
Cenário: arrastar uma aba para outra janela
  Dado a janela A com a aba "logs" no grupo "api", rodando "tail -f app.log"
  E a janela B com o grupo "web"
  Quando o usuário arrasta "logs" até a barra de B, dentro do grupo "web", e solta
  Então "logs" aparece no grupo "web" da janela B, ativa
  E a janela B recebe o foco
  E o "tail -f" continua rodando, com o scrollback anterior
  E "logs" não está mais na janela A

Cenário: o fantasma salta de barra
  Dado duas janelas lado a lado
  Quando o usuário arrasta uma aba da janela A e leva o cursor até a barra da janela B
  Então o fantasma aparece na barra de B, com o vão aberto onde a aba cairia
  E a barra de A continua mostrando o buraco de onde a aba saiu
  Quando o cursor volta ao desktop
  Então o fantasma volta para a barra de A, seguindo o cursor no eixo X

Cenário: destacar uma aba numa janela nova
  Dado uma janela com cinco abas
  Quando o usuário arrasta uma aba para o desktop e solta
  Então uma janela nova abre com o ponto agarrado da aba sob o cursor
  E a aba é a única aba da janela nova, solta, com o processo vivo
  E a janela original fica com quatro abas

Cenário: soltar sobre o terminal cancela
  Dado uma aba em arraste
  Quando o usuário a solta sobre a área do terminal de qualquer janela do Porecatu
  Então nada muda e a aba volta à origem

Cenário: mover um grupo inteiro
  Dado a janela A com o grupo "infra" de três abas, colapsado
  Quando o usuário arrasta o rótulo de "infra" até a barra da janela B e solta entre dois grupos
  Então "infra" aparece na janela B, colapsado, com nome, cor e as três abas
  E nenhuma das três abas reinicia

Cenário: a janela que fica vazia fecha
  Dado a janela A com uma única aba
  Quando o usuário arrasta essa aba para a barra da janela B
  Então a aba passa para B
  E a janela A fecha, sem diálogo de confirmação

Cenário: arrastar uma seleção
  Dado as abas "a", "b" e "c" selecionadas, em grupos diferentes
  Quando o usuário arrasta "b" para a barra de outra janela
  Então "a", "b" e "c" chegam juntas, nessa ordem, no ponto de queda
  E "b" fica ativa no destino

Cenário: clicar numa aba selecionada continua limpando a seleção
  Dado as abas "a" e "b" selecionadas
  Quando o usuário clica em "b" sem arrastar
  Então a seleção é limpa e "b" fica ativa

Cenário: Esc cancela no meio do caminho
  Dado uma aba em arraste, com o fantasma na barra de outra janela
  Quando o usuário tecla Esc
  Então o fantasma some da outra janela e a aba volta à origem

Cenário: aba SSH muda de janela sem reautenticar
  Dado uma aba SSH conectada na janela A
  Quando o usuário a move para a janela B
  Então a sessão remota continua na mesma conexão
  E nenhum diálogo de segredo aparece

Cenário: mover pelo menu
  Dado duas janelas abertas
  Quando o usuário escolhe "Mover para janela…" no menu de uma aba
  Então um popover lista a outra janela, pelo título da aba ativa dela e a contagem de abas, e "Nova janela"
  Quando ele escolhe a outra janela
  Então a aba vai para o fim da trilha dela, e aquela janela vem para a frente

Cenário: fixada muda de janela continuando fixada
  Dado uma aba fixada na janela A
  Quando o usuário arrasta a caixa dela até a zona de fixadas da janela B
  Então ela aparece fixada em B
  E no próximo arranque abre na primeira janela, como toda fixada

Cenário: a grade se ajusta ao quadro do destino
  Dado a janela A larga e a janela B estreita, com "htop" rodando numa aba de A
  Quando o usuário move essa aba para B
  Então "htop" recebe o novo tamanho de grade e se redesenha na largura de B

Cenário: Wayland
  Dado o app rodando no Wayland, com duas janelas
  Quando o usuário arrasta uma aba para fora da barra e solta
  Então o arraste é cancelado
  E "Mover para janela…" no menu da aba continua levando-a à outra janela
```

## Fora de escopo

Cada item é decisão, não esquecimento.

- **Janela flutuante seguindo o cursor** durante o arraste, como no Chrome. O dono escolheu o fantasma que salta de barra (RF-22.8, RF-22.9). A janela flutuante exigiria uma janela sem borda e sempre no topo, posicionada a cada movimento do mouse, e isso o Wayland nem permite.
- **Mover a janela inteira ao puxar a única aba**, também como no Chrome. Destacar a única aba produz o mesmo arranjo (RF-22.15), e o arraste da área vazia da barra já move a janela ([ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md)).
- **Mover painel** entre abas ou janelas (RF-6.25, RF-22.27).
- **Arrastar entre processos** (outra instância do Porecatu) e **para outro emulador**. O terminal vivo não sai do processo (RF-22.28).
- **Fundir grupos de mesmo nome** no destino (RF-22.5).
- **Nome de janela**, e menu de janelas além do popover. O popover identifica a janela pela aba ativa (RF-22.21); a [ADR-0015](../adr/0015-multiplas-janelas.md) deixou "menu de janelas" fora, e só este popover entra.
- **Atalho default** para as ações novas (RF-22.24).
- **Arraste entre janelas no Wayland** (RF-22.29).
- **Desfazer** o movimento. O mesmo gesto de volta o desfaz, com o processo vivo.

### O que este documento revisa

- **[PRD-000](prd-000-visao-de-produto.md), não-objetivos.** "Mover abas entre janelas por drag" deixa de ser não-objetivo, com emenda datada.
- **[PRD-001](prd-001-abas.md), "Fora de escopo".** O mesmo item sai da lista, com emenda datada.
- **[PRD-002](prd-002-grupos-de-abas.md), "Fora de escopo".** "Mover grupo entre janelas" sai da lista. O RF-2.3 ganha a precisão do RF-22.18: o efeito é o mesmo, e só o momento muda para uma aba já selecionada.
- **[PRD-010](prd-010-interacao-e-superficie-de-app.md).** O RF-10.24 deixa de valer na primeira frase. A segunda (fechar a última janela encerra o app gravando a sessão) continua valendo. O item correspondente sai de "Fora de escopo". O RF-10.23 ganha a exceção do RF-22.19: a janela que fica vazia por mudança de lugar não confirma.
- **[PRD-006](prd-006-paineis-divididos.md), RF-6.25.** A regra continua, mas a razão citada (*"a mesma fronteira que o PRD-001 mantém para abas entre janelas"*) deixa de existir. Emenda datada.
- **[acoes.md](../reference/acoes.md).** A linha de `tab.move_to_window` em "ações que não existem" ganha a nota de revisão agora. As duas ações do RF-22.24 entram nas seções `tab.*` e `group.*` **junto com o binário que as implementa**.

### O que este documento **não** contradiz

O [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md) já previa o recurso: *"o modelo não impede: é `move_tab` com destino em outro `Workspace`"*. Os contadores de identidade por workspace continuam como estão: a aba ganha um `TabId` novo ao chegar, como o [ADR-0090](../adr/0090-transferencia-de-abas-entre-janelas.md) detalha. A proibição de reexecutar processos do [PRD-003](prd-003-persistencia-de-sessao.md) também continua inteira: mover não executa nada.

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Processos reiniciados por mover uma aba ou um grupo | **zero** |
| Bytes de saída perdidos durante a mudança de janela | **zero** |
| Reautenticações SSH por mover | **zero** |
| Gestos para separar uma aba numa janela própria | **um** (arrastar para fora) |
| Diálogos de confirmação por mover | **zero** |
| Valores visuais inventados | **zero**: fantasma, vão, realce e popover são os existentes |
| Dependências novas no workspace | **zero** |

A primeira é a que define o recurso. Mover que reinicia seria só o fluxo antigo de abrir uma aba nova e fechar a velha, com animação.
