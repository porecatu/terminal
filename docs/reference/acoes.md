# Catálogo de ações

Referência normativa do **conjunto fechado de ações** que o [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md) exige:

> O conjunto de ações é fechado e enumerado (`tab.new`, `tab.close`, `group.create`, ...). Ação desconhecida é erro de config com sugestão do nome mais próximo.

Esta é a enumeração. É o insumo direto do parser de `[keybindings]` na F4 e do roteamento de input na F2.

**Regra:** nenhuma ação existe sem origem. Toda linha abaixo rastreia a um RF aprovado ou a um ADR aceito. Ação nova exige requisito ou decisão nova primeiro — não o contrário.

---

## Convenções

- Nome no formato `dominio.verbo`, em `snake_case`, sempre em inglês (é identificador, não texto de UI).
- A coluna **Fase** é a do [roadmap](../roadmap.md) em que a ação passa a existir.
- **Arg** marca ação que exige um argumento e por isso **não é vinculável a tecla**: ela é invocada por menu ou por arraste, onde o alvo é implícito. Vincular `group.set_color` a uma tecla não faria sentido — qual cor?
- A ação especial `none` remove um binding e devolve a tecla ao terminal ([ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md)). É a válvula de escape para conflito com `emacs` e afins.
- Ação desconhecida na config é **erro** com sugestão do nome mais próximo; binding duplicado é erro citando as duas linhas. Chave desconhecida é aviso (RF-4.22), mas ação desconhecida não — um binding que não faz nada é pior que um erro.

---

## `tab.*` — abas

| Ação | O que faz | Origem | Fase | Arg |
|---|---|---|---|---|
| `tab.new` | Cria aba no grupo da aba ativa, herdando o `cwd` | [PRD-001](../prd/prd-001-abas.md) RF-1.1 | F2 | |
| `tab.close` | Fecha a aba ativa; confirma se houver processo em primeiro plano. Fechar a **última** aba fecha a janela | RF-1.2, RF-1.6, RF-1.4 | F2 | |
| `tab.next` | Próxima aba na ordem visual, circulando | RF-1.11 | F2 | |
| `tab.prev` | Aba anterior na ordem visual, circulando | RF-1.11 | F2 | |
| `tab.goto_1` … `tab.goto_9` | Ativa a N-ésima aba visível da janela | RF-1.12 | F2 | |
| `tab.rename` | Abre a edição inline do título na aba ativa | RF-1.8 | F2 | |
| `tab.move_left` | Move a aba ativa uma posição à esquerda | RF-1.17 | F2 | |
| `tab.move_right` | Move a aba ativa uma posição à direita | RF-1.17 | F2 | |
| `tab.move_to_group` | Move a aba ativa para um grupo | [PRD-002](../prd/prd-002-grupos-de-abas.md) RF-2.20 | F3 | sim |

As nove entradas de `tab.goto_N` são explícitas, não um padrão com curinga: o conjunto é fechado, e `tab.goto_12` precisa ser erro de config, não uma ação silenciosamente inerte. O índice é sobre a ordem visual da janela inteira, não por grupo (RF-1.12), e abas de grupo colapsado não contam (RF-2.15).

> **Estado na F2.** Não há parser de `[keybindings]` até a F4, então nenhuma
> ação de aba é *vinculável* ainda: os defaults de plataforma do
> [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md) e do
> [ADR-0015](../adr/0015-multiplas-janelas.md) entram fixos no código, como
> `Ctrl+Shift+C`/`V` na F1. O que a F2 traz de novo é o **enum de ação** e o
> **modo de captura** (passo 1 da cadeia do ADR-0008), que o rename inline
> exige e que a F1 não tinha.
>
> `tab.close` confirma conforme o [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md)
> e o [ADR-0034](../adr/0034-deteccao-de-processo-ativo-para-confirmacao.md),
> dois sinais em OU: tela alternativa/reporte de mouse ligado, **ou**, no
> Windows, mais de um processo vivo na árvore do shell ([ADR-0033](../adr/0033-job-object-encerramento-de-processo.md))
> — ex. um servidor de longa duração em primeiro plano. Os dois ficam atrás
> da chave `general.confirm_close_with_process`. `tab.new` herda o `cwd`
> capturado por OSC 7, que passa a
> existir na F2; sem OSC 7, cai em `startup_directory`, que hoje é o **home do
> usuário** (correção factual no ADR-0017).
>
> **Fechar a última aba fecha a janela**, e fechar a última janela encerra o app
> (RF-1.4) — decidido depois da F2, pela mesma razão que uma janela sem aba não
> tem estado que valha manter na tela. O caminho é o mesmo do
> `window.close`: sobe como pedido de fechamento e passa pelo diálogo de
> confirmação quando há mais de uma aba. **Desde o ADR-0034**, confirma
> também com uma aba só se ela tiver processo ativo (mesmo critério do
> `tab.close`) — fechar a janela mata a árvore de processo igual a fechar a
> aba, então o aviso precisa valer nos dois caminhos.

> **Estado ao fim da F3.** As nove `group.*` e a `tab.move_to_group` existem.
> `group.next`/`group.prev` (RF-2.21) foram as últimas, no PR que fechou a fase:
> `Workspace::step_group` anda na ordem visual, circulando, e ativa a última aba
> visitada do destino (`Group::last_active`, gravado desde a primeira etapa e até
> ali sem consumidor). Pula grupo **colapsado** — navegar não expande nada — e
> grupo vazio; sem `last_active`, cai na primeira aba do grupo
> ([ADR-0020](../adr/0020-grupos-explicitos.md) §6). Run implícito conta como
> destino: sem isso, "voltar para as abas soltas" não teria gesto.
>
> **Teclas ligadas** (defaults de Windows/Linux fixos no código, sem parser de
> `[keybindings]` até a F4): `group.create` `Ctrl+Shift+G`, `group.dissolve`
> `Ctrl+Shift+U`, `group.rename` `Ctrl+Shift+E`, `group.toggle_collapse`
> `Ctrl+Shift+K`, `group.next`/`group.prev` `Ctrl+Shift+PageDown`/`PageUp`. As
> duas últimas exigem `Ctrl` **e** `Shift`, porque `Shift+PageUp`/`PageDown`
> sozinhos são a rolagem de scrollback. `group.new_tab` e `group.close_all`
> seguem sem default, de propósito (ver abaixo), e `group.set_color` não é
> vinculável.
>
> Os **defaults de macOS** da tabela do ADR-0008 (`Cmd+…`, incluindo
> `Cmd+Alt+Right`/`Left` para navegar entre grupos) não existem em código: nada
> disso responde no Mac até o parser da F4.
>
> `group.new_tab` ganhou um segundo caminho que nenhum documento previa: um
> botão "+" ao final de cada grupo na barra, inclusive de um run implícito
> (espec. visual §2.6, seção 4.4). É gesto de mouse, não ação nova.
>
> `tab.goto_N` passa a numerar sobre a ordem **navegável**, não a visual: aba de
> grupo colapsado sai da numeração, e colapsar um grupo renumera `Alt+1..9`. É a
> leitura consistente de RF-1.12 com RF-2.15, decidida no
> [ADR-0020](../adr/0020-grupos-explicitos.md).
>
> `group.set_color` e `tab.move_to_group` **abrem uma superfície de escolha** em
> vez de executar direto — editor e popover de destino, respectivamente
> ([ADR-0023](../adr/0023-editor-de-grupo.md)). Continuam `Arg` e continuam não
> vinculáveis; o item "Mover para grupo" do menu de aba deixa de ser esmaecido.

---

## `group.*` — grupos

| Ação | O que faz | Origem | Fase | Arg |
|---|---|---|---|---|
| `group.create` | Cria grupo com as abas selecionadas, cor automática, nome em edição | RF-2.4, RF-2.5 | F3 | |
| `group.dissolve` | Desagrupa: abas voltam ao grupo implícito na mesma posição | RF-2.6 | F3 | |
| `group.rename` | Abre o editor de grupo com o foco no nome | RF-2.9 | F3 | |
| `group.set_color` | Abre o editor de grupo com o foco na faixa de cores | RF-2.10 | F3 | sim |
| `group.toggle_collapse` | Colapsa ou expande o grupo | RF-2.13, RF-2.14 | F3 | |
| `group.next` | Próximo grupo, ativando sua última aba visitada | RF-2.21 | F3 | |
| `group.prev` | Grupo anterior, ativando sua última aba visitada | RF-2.21 | F3 | |
| `group.new_tab` | Cria aba dentro do grupo, herdando o `cwd` da **última aba daquele grupo** | RF-2.8, RF-2.22 | F3 | |
| `group.close_all` | Fecha todas as abas do grupo; confirmação com contagem, **sempre** | RF-2.22, RF-2.23 | F3 | |

`group.close_all` é a única ação cuja confirmação não é configurável — o RF-2.23 chama isso de *"a ação mais destrutiva da interface"*.

O menu de contexto do grupo e o editor de grupo invocam **esta mesma lista**, por decisão do [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md): duas listas divergiriam na primeira mudança.

### Qual grupo cada uma dessas ações afeta

Sete das nove operam sobre **um** grupo, e o alvo depende de como foram
invocadas — o que precisa estar escrito aqui, e não ser decidido na
implementação, porque o conjunto é fechado:

- **Por tecla:** o grupo da **aba ativa**. Sempre existe, porque toda aba
  pertence a exatamente um grupo, inclusive um run implícito
  ([ADR-0020](../adr/0020-grupos-explicitos.md)).
- **Por menu de contexto de grupo ou pelo editor:** o grupo **clicado**, que
  pode não conter a aba ativa.

As ações **não** ganham argumento por isso: o alvo é o contexto de invocação, do
mesmo jeito que `tab.close` fecha a aba ativa por tecla e a aba clicada por
menu. Só `group.set_color` e `tab.move_to_group` seguem marcadas `Arg`, porque
precisam de um valor — cor e grupo de destino — que nem a tecla nem o alvo
implícito fornecem.

Duas ações são vinculáveis e **não têm default de tecla**, de propósito:

- `group.close_all` — atalho para a ação mais destrutiva da interface é risco
  desproporcional ao ganho. Quem quiser, vincula.
- `group.new_tab` — conveniência de menu de grupo; `tab.new` já cria aba no
  grupo da aba ativa, que é o caso comum.

Sobre um grupo implícito, `group.rename`, `group.set_color`, `group.dissolve` e
`group.toggle_collapse` ficam **indisponíveis** — esmaecidas no menu, nunca
ausentes (RF-10.20). Grupo implícito não tem nome, cor nem colapso
(ADR-0006, ADR-0020).

> **Na implementação (F3).** O menu de grupo só abre a partir da pílula, e
> pílula só existe para grupo explícito: nenhum item nasce esmaecido ali, porque
> o menu nunca abre sobre um run implícito. **Por tecla, o caminho existe** desde
> o PR de fechamento da fase, e é `group_menu::keyboard_target` quem aplica a
> regra: ele resolve o grupo da aba ativa e devolve `None` sobre um run
> implícito, então as quatro ações acima são no-op ali. Função pura, testada sem
> GPU — o "esmaecido" do menu e o "no-op" da tecla são a mesma regra, não duas.

---

## `window.*` — janelas

| Ação | O que faz | Origem | Fase | Arg |
|---|---|---|---|---|
| `window.new` | Abre janela nova com uma aba, herdando o `cwd` da aba ativa | [ADR-0015](../adr/0015-multiplas-janelas.md) | F2 | |
| `window.close` | Fecha a janela; confirma se houver mais de uma aba, ou uma só com processo ativo | ADR-0015, ADR-0034, RF-1.4 | F2 | |

---

## `scrollback.*` — rolagem

| Ação | O que faz | Origem | Fase | Arg |
|---|---|---|---|---|
| `scrollback.line_up` | Rola uma linha para trás | [ADR-0013](../adr/0013-mouse-selecao-e-clipboard.md) | F1 | |
| `scrollback.line_down` | Rola uma linha para frente | ADR-0013 | F1 | |
| `scrollback.page_up` | Rola uma tela para trás | ADR-0013 | F1 | |
| `scrollback.page_down` | Rola uma tela para frente | ADR-0013 | F1 | |
| `scrollback.to_top` | Vai ao início do scrollback | ADR-0013 | F1 | |
| `scrollback.to_bottom` | Volta ao final, onde está o prompt | ADR-0013 | F1 | |

Nenhuma delas faz nada na tela alternativa, onde não existe scrollback ([ADR-0013](../adr/0013-mouse-selecao-e-clipboard.md)).

> **Estado na F1.** Não há parser de `[keybindings]` até a F4, então nenhuma
> dessas ações é *vinculável* ainda — o que existe é a mecânica e alguns
> defaults fixos no código. `page_up`/`page_down` respondem a
> `Shift+PageUp`/`PageDown`; `line_up`/`line_down` acontecem pela roda do
> mouse; `to_top`/`to_bottom` têm a operação implementada em `porecatu-term`,
> sem tecla que a dispare.

---

## `clipboard.*` e `selection.*`

| Ação | O que faz | Origem | Fase | Arg |
|---|---|---|---|---|
| `clipboard.copy` | Copia a seleção, com espaço à direita cortado e `WRAPLINE` remontado | [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md), ADR-0013 | F1 | |
| `clipboard.paste` | Cola, envolvido em bracketed paste quando o modo está ativo | ADR-0008 | F1 | |
| `selection.select_all` | Seleciona a tela visível e o scrollback | [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md) — menu do terminal | F6 | |

> **Estado na F1.** `clipboard.copy` e `clipboard.paste` estão ligados a
> `Ctrl+Shift+C`/`V` fixos no código — os únicos defaults de app que não
> dependem de `porecatu-config`. O caminho completo já existe: seleção pelo
> motor, `arboard` de um lado só e bracketed paste na cola.

---

## `font.*`, `theme.*`, `config.*`, `search.*`, `app.*`

| Ação | O que faz | Origem | Fase | Arg |
|---|---|---|---|---|
| `font.increase` | Aumenta a fonte na sessão, sem tocar no arquivo | [PRD-005](../prd/prd-005-aparencia-do-terminal.md) RF-5.9 | F4 | |
| `font.decrease` | Diminui a fonte na sessão | RF-5.9 | F4 | |
| `font.reset` | Volta ao tamanho da config | RF-5.9 | F4 | |
| `theme.cycle` | Cicla entre os temas nomeados definidos na config | RF-5.21 | F4 | |
| `config.reload` | Relê o arquivo de config imediatamente | [ADR-0003](../adr/0003-formato-de-configuracao.md) | F4 | |
| `search.open` | Abre a busca no scrollback | [roadmap](../roadmap.md) F6 | F6 | |
| `search.next` | Próxima ocorrência | roadmap F6 | F6 | |
| `search.prev` | Ocorrência anterior | roadmap F6 | F6 | |
| `app.quit` | Encerra o app, gravando a sessão de forma síncrona | RF-1.4, RF-3.4 | F2 | |
| `none` | Remove o binding; a tecla vai para o terminal | ADR-0008 | F4 | |

`app.quit` existe por convenção de plataforma: `Cmd+Q` no macOS é esperado e o [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md) já define defaults por plataforma. O efeito é o mesmo do RF-1.4 ao fechar a última janela, incluindo a gravação síncrona do RF-3.4.

> **Estado na F2.** `app.quit` e `window.close` da última janela **não gravam
> sessão** — `porecatu-session` só existe na F5. O
> [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md) decide que o ponto de
> chamada da gravação síncrona existe desde a F2 como no-op documentado, para
> que a F5 preencha em vez de procurar onde.

`config.reload` não substitui o hot reload automático (RF-4.20), que continua acontecendo a cada gravação do arquivo. Existe para o caso em que o watcher não disparou — editor que grava por `rename`, arquivo em rede.

---

## `[v2]` — reservadas, não implementar

| Ação | O que faz | Origem |
|---|---|---|
| `command.palette` | Abre a paleta de comandos | [PRD-008](../prd/prd-008-paleta-de-comandos.md) *(rascunho)*, [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md) |

O nome está reservado porque o binding default já está no arquivo de exemplo: `Ctrl+Shift+P`, que foi o motivo de `theme.cycle` migrar para `Ctrl+Shift+Y` ([ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md)).

---

## Ações que **não** existem

Ausências deliberadas, registradas para que ninguém as adicione achando que foram esquecidas. Cada uma é não-objetivo de um documento aprovado ou simplesmente não tem requisito:

| Ação ausente | Por quê |
|---|---|
| `tab.duplicate` | Fora de escopo do PRD-001 (*"duplicar aba com o estado do processo"*) |
| `tab.pin` | Fora de escopo do PRD-001 (*"fixar aba"*) |
| `tab.move_to_window` | Arrastar ou mover aba entre janelas é v2 ([ADR-0015](../adr/0015-multiplas-janelas.md), PRD-000) |
| `pane.split` / `pane.close` | Painéis divididos são v2 ([PRD-006](../prd/prd-006-paineis-divididos.md), rascunho) |
| `profile.*` | Perfis de aba são v2 ([PRD-007](../prd/prd-007-perfis-de-aba.md), rascunho) |
| `terminal.clear` | Nenhum RF pede; o shell já tem `clear` |
| `terminal.reset` | Idem; `reset` existe no shell |
| `session.save` / `session.restore` | A gravação é automática por decisão do [ADR-0005](../adr/0005-persistencia-de-sessao.md); ação manual sugeriria que não é |
| `group.select_all_tabs` | RF-2.1 define seleção múltipla por mouse; nenhum RF pede equivalente de teclado |

Precisar de uma delas é sinal de requisito faltando, não de catálogo incompleto. O caminho é PRD ou ADR primeiro.

### Superfícies de mouse e de modal, que não são ações

Treze comportamentos das F2 e F3 têm requisito aprovado e **não recebem nome de ação** — os seis primeiros são da F2, os sete últimos da F3. Registrados aqui porque a ausência confunde: eles não estão faltando no catálogo, estão fora dele por definição. O critério é o da seção Convenções — ação é o que o parser de `[keybindings]` resolve, e nenhum destes é vinculável a tecla.

| Comportamento | Requisito | Por que não é ação |
|---|---|---|
| Ativar aba por clique | RF-1.13 | o alvo é o pixel clicado; sem alvo, a tecla equivalente já existe (`tab.next`, `tab.goto_N`) |
| Abrir menu de contexto | RF-10.19 | ancora no cursor e o conteúdo varia com o alvo; o que o menu invoca **são** ações do catálogo |
| Rolar a trilha de abas | RF-1.18, RF-1.19 | gesto de roda sobre a barra; `scrollback.*` é do grid e conta linhas |
| Dispensar aviso | RF-10.16 | `Esc` sobre o aviso do topo é modo de captura, não binding — passo 1 da cadeia do [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md) |
| Confirmar ou cancelar diálogo | RF-10.18 | idem: `Enter` e `Esc` dentro de modal, consumidos antes da tabela de keybindings |
| Reordenar aba por arraste | RF-1.15 | posição de queda é contínua; `tab.move_left`/`tab.move_right` são o equivalente de teclado, uma posição por vez |
| Selecionar múltiplas abas | RF-2.1 | `Ctrl`/`Cmd`+clique alterna, `Shift`+clique estende a partir da âncora; o alvo é o pixel clicado, e nenhum RF pede equivalente de teclado (ver `group.select_all_tabs` acima) |
| Limpar a seleção por clique simples | RF-2.3 | é a ausência de modificador sobre "ativar aba por clique", não um comando próprio |
| Colapsar por clique no rótulo do grupo | RF-2.13 | o alvo é a pílula clicada; `group.toggle_collapse` é o equivalente de teclado |
| Abrir o editor por duplo clique no rótulo | RF-2.22 | idem; `group.rename` e `group.set_color` abrem o mesmo editor por outro caminho |
| Arrastar aba para dentro ou fora de um grupo | RF-1.16, RF-2.18 | o grupo de destino vem dos limites visuais sob o cursor; `tab.move_to_group` é o equivalente sem mouse, e por isso é `Arg` |
| Arrastar o rótulo do grupo | RF-2.19 | move o grupo inteiro para uma fronteira contínua; nenhum RF pede equivalente de teclado |
| Botão "+" ao final de um grupo | RF-2.8 | o alvo é o wrapper clicado; `group.new_tab` **é** a ação que ele invoca, e continua sem default de tecla |

Vincular qualquer um deles a tecla exigiria um argumento que a tecla não tem — que é a mesma razão pela qual `group.set_color` é marcada `Arg` e não é vinculável.
