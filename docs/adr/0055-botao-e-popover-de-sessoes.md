# ADR-0055 — Botão e popover de sessões: segundo botão da zona fixa, sétimo widget de chrome

**Status:** Aceito — aval visual do dono do produto em 2026-09-24 (ícone `bookmark`, posição à esquerda da engrenagem, perda de 44px da trilha), como o [ADR-0032](0032-interface-do-v1-fechada.md) exige para mudança das seções 1/2 da especificação visual
**Data:** 2026-09-24
**Relacionados:** [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0019](0019-tooltip.md), [ADR-0022](0022-animacao-de-interface.md), [ADR-0023](0023-editor-de-grupo.md), [ADR-0024](0024-face-de-icones.md), [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md), [ADR-0041](0041-busca-no-scrollback.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0054](0054-sessoes-nomeadas.md), PRD-004, PRD-010, [PRD-014](../prd/prd-014-sessoes-nomeadas.md)

## Contexto

O [PRD-014](../prd/prd-014-sessoes-nomeadas.md) pede um botão **ao lado da engrenagem de configurações** que liste as sessões nomeadas, restaure a escolhida numa janela nova, salve a janela atual com um nome e exclua uma sessão. O [ADR-0054](0054-sessoes-nomeadas.md) decidiu tudo o que acontece por trás; este decide **o que aparece**.

É mudança das seções 1 e 2 da [especificação visual](../design/especificacao-visual.md), e o [ADR-0032](0032-interface-do-v1-fechada.md) fechou a interface do v1: mudar pixel exige ADR — o terceiro a passar por essa porta, depois do [ADR-0037](0037-aba-nao-iniciada.md) (rótulo esmaecido) e do [ADR-0041](0041-busca-no-scrollback.md) (barra de busca). E exige o aval do dono do produto, que é a razão do status.

Três perguntas de desenho, cada uma com precedente no projeto:

1. **Onde o botão fica.** A zona fixa à direita existe desde a F3 exatamente para isto: a §2.2 da especificação a descreve como *"reservada para o que a barra ganhar à direita daqui em diante"*, e a razão de ela existir — botão ao final da trilha rolável sai de vista — vale igual para o botão novo.
2. **Que superfície mostra a lista.** O projeto tem seis widgets de chrome ([ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0019](0019-tooltip.md), [ADR-0023](0023-editor-de-grupo.md), [ADR-0041](0041-busca-no-scrollback.md)), e nenhum deles faz as três coisas: o menu de contexto **não rola** (§2.16); o popover de grupo de destino rola mas é lista de destinos fixos; o editor de grupo tem campo de texto mas não lista.
3. **Que valores ele usa.** A regra que não cai, desde o [ADR-0028](0028-o-binario-como-referencia-visual.md): nenhuma cor, dimensão, raio ou espaçamento inventado.

## Decisão

**Um segundo botão de ícone na zona fixa, à esquerda da engrenagem, com a mesma anatomia dela; ele abre um popover novo — o sétimo widget de chrome — que é o popover de grupo de destino com um campo de texto do editor de grupo no topo. Nenhum valor novo; um ícone novo.**

### 1. O botão

- **Posição:** zona fixa à direita (§2.2 item 2), **imediatamente à esquerda do botão de configurações**, separado dele pelo `trilha_gap` (6px) que já é o respiro da zona. A ordem, da esquerda para a direita: trilha · **sessões** · configurações · botões de janela.
- **Anatomia:** idêntica à do botão de configurações — 30×30 de desenho, **38×30** de alvo com o `icon_button_padding_x`, raio 6, borda `#262b34`, ícone na em de ícone do chrome (`chrome::ICON_EM_SIZE`) no tom de base `#e4e8ee`, hover por brilho como os demais botões da barra. Sem estado "ativo" enquanto o popover está aberto: a engrenagem não tem, e o popover aberto já é a informação.
- **Ícone:** Lucide **`bookmark`**, acrescentado como constante em `porecatu_render::icon` e na lista `ALL`, com `ink_width_em`/`ink_height_em` pinados contra a rasterização como todo ícone do projeto ([ADR-0024](0024-face-de-icones.md); a armadilha da em registrada no CLAUDE.md). A face embutida já contém o codepoint — não há recorte a refazer. Candidatos considerados, para o aval: `bookmark` (salvar e voltar), `history` (confunde com histórico de comandos), `folder-open` (confunde com abrir diretório), `layout-grid` (confunde com painéis).
- **`right_zone_width`** passa a somar o botão novo e mais um `trilha_gap`: 6 + 38 + 6 + 38 + 6 = **94px** fora do macOS antes dos botões de janela (hoje 50). A trilha perde 44px; é o custo do recurso e está registrado nas consequências.
- **Hit test:** `sessions_button_rect` / `point_in_sessions_button` em `tab_bar.rs`, ao lado de `settings_button_rect`, e testado **antes** da trilha, depois dos botões de janela — a mesma ordem da engrenagem. A drag region do [ADR-0027](0027-controles-de-janela-e-resize-proprios.md) exclui o botão novo como exclui a engrenagem.
- **Acessibilidade:** nó próprio no `access.rs` ([ADR-0043](0043-arvore-de-acessibilidade.md)), papel botão, nome "Sessões salvas", projeção da mesma função de geometria.

### 2. O popover: o sétimo widget

**Onde abre.** Ancorado sob o botão, com a **borda direita alinhada à borda direita do botão**, **8px abaixo da borda inferior da barra** — a regra do editor de grupo (§2.10). Flip nos dois eixos como o menu de contexto; com `tab_bar_position = "bottom"`, abre acima da barra. Camada **popover** do [ADR-0018](0018-composicao-de-frame.md), a mesma do menu de contexto, do editor e do tooltip; nenhuma camada nova. Nunca coexiste com o menu de contexto nem com o editor de grupo: abrir um fecha o outro, a regra que o [ADR-0023](0023-editor-de-grupo.md) já tem.

**Superfície.** Os tokens do menu de contexto (§2.16): fundo `#1a1e25`, borda `1px #2e343e`, raio 8, `padding: 6`, sombra em camadas (§1.7). **Largura fixa 320** — o teto do menu de contexto e do aviso, não um valor novo: nomes de sessão são o conteúdo, e a largura mínima de 200 do menu truncaria quase todos.

**Conteúdo, de cima para baixo:**

1. **Item "Salvar esta janela…"** — item de menu da §2.16 (`padding: 7px 8px`, raio 5, texto 12.5px `#d7dce3`, hover `#242a33`), com o ícone `PLUS` à esquerda no `gap: 10` e o chip de tecla do atalho à direita (mono 9.5px `#5c646f`), lido do mapa resolvido — some se o usuário desvincular a ação.
   - **Em modo de edição**, o item vira o **campo de texto** do editor de grupo (§2.10 item 1): fundo `#0f1216`, borda `1px #333a45` com foco `#5ed3bc`, raio 5, 13px `#e4e8ee`, `padding: 7px 9px`, altura 30, foco automático, cursor e seleção do [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md). É o **mesmo componente** (`TextFieldState`) — quarto consumidor dele, depois do editor, do rename de aba e do rename de pílula. Placeholder "nome da sessão" em `#5c646f`.
2. **Divisor** `1px #2a2f38`, `margin: 5px 4px` (§2.16).
3. **Lista de sessões** — linhas com os valores do popover de grupo de destino (`row_height` 28, `row_padding_x` 8), rótulo 12.5px `#d7dce3` truncado com reticências e tooltip com o nome inteiro ([ADR-0019](0019-tooltip.md)). À direita de cada linha, o **botão de excluir**: o `X` do botão de fechar da aba, 17×17 de desenho e 25×17 de alvo (§1.7), visível **só** na linha sob o cursor ou realçada pelo teclado — com um `X` em cada linha o tempo todo, a lista viraria uma coluna de botões destrutivos. Hover da linha `#242a33`; hover do `X` com o tom destrutivo `#e08585` do item destrutivo da §2.16.
   - **Linha de arquivo ilegível ou de schema mais novo** (RF-14.17): rótulo esmaecido em `#5c646f`, a regra do *"item indisponível fica esmaecido, nunca ausente"* da §2.16. Não restaura; o `X` continua disponível — excluir um arquivo ruim é uma das coisas que o usuário quer fazer com ele.
   - **Lista vazia** (RF-14.9): uma linha só, "nenhuma sessão salva", 12.5px `#5c646f`, sem hover e sem alvo.
   - **Rolagem:** teto de linhas visíveis com o valor 6 do popover de grupo de destino; acima disso a lista rola pelo realce de teclado e pela roda do mouse, como o popover de destino desde a F4 etapa 6. O item de salvar e o divisor **não rolam**: ficam fixos no topo.

**Uma seção nova de config, sem valor novo.** `[appearance.session_picker]` com `width = 320`, `row_height = 28`, `row_padding_x = 8` e `max_visible_rows = 6`, cada um comentado com o token de onde vem — a mesma forma de `[appearance.move_to_group]`, que também repete os números do menu de contexto. Cores e tipografia vêm de `[appearance.context_menu]`, como lá. Classe de recarga A.

### 3. Teclado e ciclo de vida

- **Abre** pelo clique no botão, por `session.open_list` (primeira linha realçada) e por `session.save_named` (campo em edição). Clicar no botão com o popover aberto fecha.
- **Navegação:** `Up`/`Down` movem o realce entre o item de salvar e as linhas; `Enter` aciona o realçado (restaurar, ou entrar em edição); `Delete` exclui a linha realçada. Hover e realce de teclado são o **mesmo** estado visual e mutuamente exclusivos, como na §2.16.
- **No campo:** `Enter` confirma, `Esc` sai da edição e **volta ao popover** com o item de salvar realçado — não fecha o popover inteiro, porque o `Esc` da edição é "desisti deste nome", não "desisti de tudo". Um segundo `Esc` fecha. A captura de teclado é **total** enquanto o popover está aberto, como no editor de grupo: não é a barra de busca, que precisa deixar o terminal responder.
- **Fecha** em clique fora, `Esc`, perda de foco da janela, depois de restaurar (a janela nova ganha o foco) e depois de salvar com sucesso.
- **Confirmações** (sobrescrever, excluir) abrem o diálogo da §2.15 **por cima** do popover, que permanece atrás; cancelar volta a ele, com o estado de antes — a regra que o editor de grupo já usa para "Fechar grupo".
- **Sem animação.** A lista de consumidores do relógio do [ADR-0022](0022-animacao-de-interface.md) é fechada em dois; o `pop .13s` não entra, como não entrou em nenhum outro popover.

### 4. O que a especificação visual ganha

Aceito o ADR, nada abaixo entra na especificação **antes do código**: desde o [ADR-0028](0028-o-binario-como-referencia-visual.md) ela descreve o binário e é atualizada no mesmo PR que o muda — aqui, a etapa que desenha o botão e o popover. A seção `[appearance.session_picker]` do arquivo de exemplo entra na mesma leva, pela razão que o roadmap já registrou para `[git]` e `[panes]`: `tests/example_toml.rs` exige zero chaves desconhecidas.

- **§2.22 Popover de sessões `[v1]`** — a anatomia acima, no molde da §2.21.
- **§2.2 item 2** — a zona fixa passa a carregar dois botões. O texto que ainda chamava a engrenagem de "inerte" já foi corrigido junto com este ADR — correção de fato, não de decisão.
- **§1.7** — a linha "Botão da zona fixa à direita" vira plural, e a linha de largura da trilha passa a descrever o `right_zone_width` com os dois botões.
- **§3** — linhas novas para o botão e para o popover, governadas pelo PRD-014 e por este ADR.
- **§4.4** — uma entrada: a zona fixa, reservada desde a F3, ganha o segundo ocupante.
- **Lista de widgets de chrome** no CLAUDE.md ("Nenhum diálogo nativo do sistema"): sete, não seis.

## Alternativas consideradas

### Reaproveitar o menu de contexto

Zero widget novo. Recusada porque o menu de contexto não rola por decisão registrada (§2.16, *"as listas do v1 têm meia dúzia de itens"*), e a lista de sessões é do tamanho do número de projetos do usuário — o mesmo argumento que fez o popover de grupo de destino rolar. Fazer o menu rolar reabriria uma decisão de outro widget para servir este.

### Salvar num diálogo modal com campo de texto

Separaria salvar de listar. Recusada porque o diálogo da §2.15 é a marca do gesto **destrutivo** — overlay, foco no cancelar — e salvar não é destrutivo; o destrutivo (sobrescrever) já abre o diálogo por cima. Um campo no próprio popover mantém o gesto num lugar só, e é o que o editor de grupo já faz.

### Botão à direita da engrenagem

Colaria o botão novo nos botões de janela. Recusada porque a engrenagem é a âncora visual da zona desde a F3, e o que é mais usado no dia a dia — abrir uma sessão — fica mais perto da trilha, onde o olho já está.

### Ícone só com texto ("Sessões")

Legível sem aprender ícone. Recusada porque a zona fixa tem largura contada — texto de 8 caracteres come o dobro do botão de ícone — e a engrenagem ao lado é só ícone; um botão de texto e um de ícone lado a lado não leem como a mesma classe de coisa (o argumento que o [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md) usou ao contrário, para dar ícone ao indicador de commits).

### Excluir por menu de contexto na linha

Clique direito na linha, "Excluir". Evitaria o `X` sempre à mão. Recusada porque menu de contexto dentro de popover é menu sobre popover, que o ADR-0023 proíbe coexistir; e o `X` é o gesto que a barra já ensina no botão de fechar da aba.

## Consequências

### Positivas

- **Nenhum valor novo e nenhuma cor nova.** Todo número do popover já estava na tabela de tokens ou no arquivo de exemplo; o que entra é a combinação.
- O **campo de texto** e a **lista rolável** já existiam separados; juntá-los não pede primitiva nova em `porecatu-render`.
- A zona fixa cumpre a razão de existir que a F3 lhe deu, sem trilha rolável escondendo o botão.

### Negativas

- **Um ícone novo na face**, com a medição de tinta a pinar por teste — o caminho que o [ADR-0048](0048-barra-de-status.md) §10 e o ADR-0052 já percorreram.
- **A trilha perde 44px** fora do macOS (e no macOS). Com a largura fixa de aba (229px), é um quinto de aba a menos antes do overflow.
- **Sétimo widget de chrome.** Cada widget é um estado de captura de teclado a mais, e a regra de coexistência (popover × menu × editor × diálogo por cima) fica mais longa.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| `right_zone_width` recalculado em um lugar e não em outro (overflow, arraste na borda, hit test) | Média | Médio | Continua saindo de uma função só, `tab_bar::right_zone_width`; teste de geometria que amarra os dois retângulos da zona e a largura da trilha — a lição do `bar_height` da F3 |
| Ícone com `size_px` errado desenhar pequeno e fino demais | Média | Baixo | `chrome::ICON_EM_SIZE` e `Icon::centered_origin`, como a engrenagem; teste de largura de tinta não-zero |
| `Esc` no campo fechar o popover inteiro por reaproveitar o tratamento do editor | Média | Baixo | §3: `Esc` em edição volta ao popover; teste de estado puro no módulo do popover |
| Popover e editor de grupo abertos ao mesmo tempo | Baixa | Médio | Mesma regra de exclusão do ADR-0023, estendida ao widget novo |
| Mudança visual além do que o aval cobriu | Baixa | Alto | O aval cobre ícone, posição e a perda de 44px; qualquer desvio da anatomia da §2 deste ADR volta ao dono do produto antes de entrar |
