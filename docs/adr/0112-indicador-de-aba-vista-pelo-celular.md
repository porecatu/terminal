# ADR-0112 — Indicador de aba vista pelo celular: o ícone de aparelho ao lado do "x"

**Status:** Aceito em 2026-11-20 — implementado em 2026-11-20 (verificado só por teste automatizado; ninguém viu rodando, dívida no [roadmap](../roadmap.md)); aval visual do dono **pendente** sobre o binário ([ADR-0028](0028-o-binario-como-referencia-visual.md))
**Data:** 2026-11-20
**Relacionados:** [PRD-021](../prd/prd-021-abas-fixadas.md), [PRD-025](../prd/prd-025-acesso-remoto.md) (RF-25.30), [ADR-0019](0019-tooltip.md), [ADR-0024](0024-face-de-icones.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md), [ADR-0085](0085-ponto-por-aba-no-grupo-colapsado-e-margem-do-x.md), [ADR-0087](0087-zona-de-abas-fixadas-na-barra.md), [ADR-0090](0090-transferencia-de-abas-entre-janelas.md), [ADR-0104](0104-crate-porecatu-remote.md), [ADR-0108](0108-superficies-de-ui-do-acesso-remoto.md), [ADR-0111](0111-o-celular-nao-muda-a-aba-ativa-do-desktop.md); no protocolo, [ADR-0011](https://github.com/porecatu/protocol/blob/main/docs/adr/0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md) e a [§4.4 de 03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md)

## Contexto

Com o [ADR-0111](0111-o-celular-nao-muda-a-aba-ativa-do-desktop.md), abrir uma aba no celular deixa de trocar a aba ativa do desktop. Quem está diante do computador perde o sinal que tinha, por pior que fosse, de que o celular estava naquela aba. O botão de aparelhos conectados ([ADR-0108](0108-superficies-de-ui-do-acesso-remoto.md)) diz que **há** alguém conectado, mas não **onde** ele está olhando. Sem isso, uma aba cujo terminal recebe teclas do celular parece mudar sozinha.

A especificação 0.5 do protocolo criou a mensagem `view-tab`: o app diz ao host a aba que exibe, sem pedir nada. Este ADR decide o que o host faz com essa informação. Como a interface do v1 está fechada ([ADR-0032](0032-interface-do-v1-fechada.md)), uma marca nova na aba precisa de ADR.

## Decisão

**A aba que algum aparelho conectado está vendo mostra o ícone `smartphone` em Acento, à esquerda do botão de fechar. Na aba fixada, a marca é um ponto 6×6 em Acento no canto inferior direito. Nada no desktop muda além disso.**

### 1. Estado

- **No crate `porecatu-remote`.** `session.rs` recebe `Message::ViewTab { tab }` e emite `RemoteEvent::ClientViewing { client, tab: Option<u64> }` (em `api.rs`, ao lado de `ClientConnected`/`ClientDisconnected`), com o id **do protocolo**. A sessão não guarda nada além do último valor, que morre com ela.
  - Aba desconhecida: a sessão valida o `tab` contra a árvore que o crate guarda (o último `Command::SetWorkspace`, como `core.has_pane` faz com o `subscribe`), responde `error { unknown-tab }` por `send_error` e emite `tab: None`.
  - A aba sai da árvore num `SetWorkspace`: a sessão limpa o estado dela e emite `tab: None`. A UI não precisa de caso próprio para aba fechada.
- **No `RemoteState`** (`porecatu-ui/src/remote/mod.rs`), que é do processo, não da janela, entra `viewing: BTreeMap<ClientId, u64>`.
  - `absorb` insere ou remove a entrada a cada `ClientViewing`.
  - Remove em `ClientDisconnected`.
  - Remove quando a aba deixa de existir no mapa de ids.
- **Projeção por janela**, no mesmo passo de `App::sync_connected_devices` (`app_remote.rs`), que roda a cada volta do event loop e só aloca quando algo muda.
  - Cada `WindowState` recebe `remote_viewers: HashMap<TabId, Vec<DeviceName>>`, resolvendo o id do protocolo por `ids` para `(WindowId, TabId)`.
  - Uma aba que muda de janela (`App::transfer`) leva o indicador junto, porque o id do protocolo é estável na transferência e a projeção é recalculada.
- **Pintura.** `chrome::paint` recebe a projeção como já recebe `connected_devices`.

### 2. Aba da trilha

- **Glifo.** `icon::SMARTPHONE` (`porecatu_render::icon`, o do botão de aparelhos), na em do ícone do chrome (`icon_em_size`, a mesma do "x"), na cor **Acento** `#5ed3bc` (`pal.dialog_focus_ring`, a do botão de aparelhos). É o único elemento da aba nessa cor, além da borda de seleção.
- **Caixa.** Do tamanho do retângulo do botão de fechar (17×17 de desenho, §2.5 item 3), com o glifo centrado por `Icon::centered_origin`.
- **Posição: deriva sempre do `close_button` da aba, nunca de conta própria.** A geometria entra em `tab_bar::layout`, junto da do "x" (a armadilha "fórmula de geometria nunca copiada"):
  - `show_close_button = "always"` e `"hover"`: imediatamente à **esquerda** do "x", separada dele pelo `gap: 8` da aba.
  - `"hover"`: o lugar do "x" fica **reservado** mesmo com o "x" invisível, então o ícone não se desloca quando o hover faz o "x" aparecer. A aba já tem largura fixa com o "x" incluído (§2.5); só é preciso que o ícone use o retângulo do "x", que o layout calcula em todo modo.
  - `"never"`: o ícone **ocupa o retângulo do "x"**, encostado na borda direita pela margem do [ADR-0085](0085-ponto-por-aba-no-grupo-colapsado-e-margem-do-x.md). Não tira largura do rótulo.
- **Largura do rótulo.** Em `always` e `hover`, o ícone tira do rótulo a largura da caixa mais o `gap: 8`, só na aba vista. O título trunca antes e a aba **não muda de largura**, como no indicador de atividade (§2.17).
- **Não é botão.** Não tem hover, pressionado nem alvo de clique próprio: clique, duplo clique, arraste e menu de contexto sobre ele são os da aba.
- **Não pisca nem anima.** Aparece e some no quadro em que o estado muda, sem fade.
- **Quando não aparece:**
  - a aba que esmaece ao fechar e o fantasma do arraste não desenham o ícone, como não desenham o "x";
  - a aba em **grupo colapsado** não está à vista, e o indicador não vai para a pílula: o celular não expande grupo (ADR-0111);
  - a barra **não rola** até a aba vista.

### 3. Aba fixada

A fixada não tem "x" nem rótulo (§2.5.1). A marca é um **ponto 6×6, raio 50%, em Acento**, desenhado sobre a caixa no **canto inferior direito**, a `padding_right` (6px) das bordas de baixo e da direita. É o espelho exato do ponto de atividade e campainha, que fica no canto superior direito, e os dois convivem. Não muda o tamanho da caixa. Aparece em todo estado da fixada, inclusive encerrada e não iniciada: o celular pode estar olhando o código de saída ou o convite para iniciar.

### 4. Tooltip e acessibilidade

- **Tooltip** ([ADR-0019](0019-tooltip.md), §2.20). Na trilha, o alvo do tooltip é a caixa do ícone, e o texto é "Vista em: nome, nome", com os nomes dos aparelhos na ordem de conexão. O tooltip não participa do hit-testing, então o clique continua sendo da aba. Na fixada, o ponto é pequeno demais para ser alvo: o tooltip que a fixada já tem (o título) ganha a mesma frase depois do título. Sempre uma linha, truncada na largura máxima de 320.
- **Acessibilidade** ([ADR-0043](0043-arvore-de-acessibilidade.md)). O nó `Tab` ganha o estado "vista no celular" (identificador de catálogo `access.state_remote_viewing`), sem nó próprio para o ícone.
- **Textos** só no catálogo ([ADR-0056](0056-catalogo-de-textos-da-interface.md)), em todo arquivo de `locales/`, com "aba", nunca "guia".

### 5. Valores

**Nenhum valor novo.** O glifo, a em, o Acento, o retângulo do "x", o `gap: 8`, o ponto 6×6 e a margem de 6px saem de tokens que já existem, e o `verify-docs.py` não ganha linha em `VALORES`. Não existe chave para esconder o indicador, pela mesma razão do botão de aparelhos (ADR-0108): quem está no desktop precisa saber onde alguém está olhando.

> **Nota de implementação (2026-11-20).**
> - **Crate:** `RemoteEvent::ClientViewing { client, tab }`; a sessão guarda `Active.viewing`, valida por `Core::has_tab` (fixadas, soltas e de grupos de toda janela da árvore guardada) e limpa a aba que sai da árvore em `end_gone_view`, chamada por `set_workspace` ao lado de `end_gone_subscriptions`.
> - **UI:** `RemoteState::viewing`; `absorb` devolve `Effect::ViewingChanged` só quando muda e **descarta** `ClientViewing` de cliente que não está em `clients` (não deixa entrada órfã). A projeção é `remote::viewers_by_window` (nomes em ordem de `connected_at`), e `viewers_current` confere sem alocar se a janela já tem o mapa certo; `App::sync_remote_viewers` roda dentro de `sync_connected_devices` e chama `WindowState::set_remote_viewers`.
> - **Layout:** `tab_bar::viewer_indicator_rect(close_button, style)` é a fórmula única; o `gap` é `internal_gap`. `layout` e `fit_width` continuam com a assinatura de sempre e delegam a `layout_viewed`/`fit_width_viewed`, que recebem o conjunto de abas vistas; o quadro, as prévias do arraste e o tooltip usam as variantes `_viewed`, e o hit-test continua em `fit_width` (a geometria é a mesma; só o rótulo muda).
> - **Fixada:** `pinned_dot_rect(rect, style, PinnedDotCorner::{TopRight, BottomRight})` serve aos dois pontos; o fantasma do arraste da fixada também leva o ponto, como leva o de atividade.
> - **Tooltip:** `HoverKey::TabViewers(TabId)` separa a caixa do ícone da aba (o atraso reinicia ao passar de uma para a outra). Frases `devices.viewed_on` e `devices.viewed_on_pinned` ("título · Vista em: nomes"), estado `access.state_remote_viewing`, nos cinco idiomas.
> - A animação de colapso (`old_tabs`) usa o layout sem o conjunto de abas vistas: só afeta o rótulo de uma aba que esmaece ao fechar, que não desenha o ícone.

## Alternativas consideradas

### Deduzir a aba vista das assinaturas de painel

Não exigiria mudar o protocolo. Recusada no ADR-0011 do protocolo: "assinado" não é "visto", e a assinatura cai por `host-limit` ou some ao trocar de painel enquanto o usuário continua na tela.

### Marcar a aba com a borda de Acento

A borda de Acento já é "selecionada" (§2.5), e uma aba pode estar selecionada e vista ao mesmo tempo. Recusada.

### Contador quando vários aparelhos veem a mesma aba

Como o badge do botão de aparelhos. Recusada pelo dono: o tooltip diz quantos e quais, e um badge dentro de uma aba de 209px disputa espaço com o rótulo.

### Pôr o indicador na pílula do grupo colapsado

Manteria o sinal à vista. Recusada para agora: a pílula colapsada já tem um ponto por aba ([ADR-0085](0085-ponto-por-aba-no-grupo-colapsado-e-margem-do-x.md)), e uma segunda marca nela exige outra decisão de anatomia. Fica como limite aceito.

## Consequências

### Positivas

- Quem está no desktop vê onde o celular está, sem que nada mude debaixo do teclado.
- Nenhum valor novo nem widget novo: o ícone, a cor e o ponto já existem.
- O modo `hover` do "x" não faz o ícone pular.

### Negativas

- O rótulo da aba vista trunca alguns caracteres antes (nos modos `always` e `hover`).
- Uma aba vista dentro de grupo colapsado não tem sinal na barra.
- Mais um estado por janela que precisa acompanhar `transfer`, fechamento de aba e desconexão.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Indicador órfão (aba fechada, aparelho caído, sessão sem `view-tab null`) | Média | Baixo | O estado morre com a sessão e com a aba; testes em `remote/mod.rs` para `ClientDisconnected`, aba fechada e `transfer` |
| A geometria do ícone divergir da do "x" num dos três modos | Média | Baixo | Uma só fórmula em `tab_bar::layout`, com testes por modo em `tab_bar.rs`, ao lado dos do `show_close_button` |
| O Acento do ícone ser confundido com a borda de seleção | Baixa | Baixo | Formas diferentes (glifo e contorno). Aval visual do dono no binário |
