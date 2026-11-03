# ADR-0082 — Botões da barra com o tamanho do botão de menu do app

**Status:** Aceito — aval visual do dono do produto pendente (pedido do dono; a aparência nova ainda não foi vista no binário)
**Data:** 2026-11-03
**Relacionados:** [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), [ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md), [ADR-0079](0079-botao-de-menu-do-app.md)

## Contexto

O botão de menu do app ([ADR-0079](0079-botao-de-menu-do-app.md)) é a caixa inteira de uma aba solta: 40×40 (`tab_height` 34 + `wrapper_padding` 3 dos dois lados, `tab_bar::app_menu_button_size`). Os botões de sessões e configurações eram 38×30 de alvo, e o "+" de aba avulsa (fora de grupos) era 30×30, os três derivados de `right_zone_button_size` (30). Na barra, o botão do app ficava visivelmente maior que os vizinhos.

O dono do produto pediu que sessões (`bookmark`), configurações (cog) e o "+" avulso tenham a mesma largura e altura do botão do app. **Só o tamanho**: o aspecto (transparente em repouso, hover e pressionado de `paint_zone_icon_button`) continua o mesmo.

É mudança da seção 2 da [especificação visual](../design/especificacao-visual.md), e o [ADR-0032](0032-interface-do-v1-fechada.md) exige ADR para isso.

## Decisão

### 1. Um tamanho só, vindo do botão do app

Sessões, configurações e o "+" avulso têm lado `tab_bar::app_menu_button_size` (40). Nenhum número novo: se o botão do app mudar, os quatro mudam juntos. `settings_button_rect` e o `layout` (para o "+" avulso) usam essa função; `sessions_button_rect` continua derivado de `settings_button_rect`.

"+ avulso" são **dois** botões que o `layout` produz: o `ungrouped_new_tab_button`, ao fim da trilha quando o último grupo é explícito, e o `new_tab_button` do **run solto** (grupo implícito, sem cápsula), que é o único "+" da barra quando só há abas soltas. Os dois têm os 40×40. O que decide é o grupo ser explícito ou não, não o botão ser um ou outro.

### 2. Alinhamento

O topo dos quatro é o mesmo (`trilha_padding`, 6px): a barra tem 52px, então o botão de 40px centrado nela começa em 6. O "+" avulso ocupa a caixa do wrapper, no topo da trilha, como uma aba solta.

### 3. O que não muda

- **O "+" dentro de um grupo explícito** continua 30×30 (`right_zone_button_size`): a aba do grupo tem 34px de altura, e 40 não caberia dentro da cápsula. Fora do pedido.
- **O ícone** mantém a em de antes (`chrome::ICON_EM_SIZE`); só a caixa cresce. O quadrado revelado no hover e no pressionado é de lado igual à altura do botão, então passa de 30×30 para 40×40, centrado no ícone.
- Cores, raio (6), ausência de borda, disparo no release ([ADR-0081](0081-disparo-no-release-para-botoes-e-itens.md)).

### 4. Zona fixa à direita

`right_zone_width` = 6 + 40 + 6 + 40 + 6 = **98px** fora do macOS antes dos botões de janela (eram 94). A trilha perde 4px.

> **Revisão ([ADR-0083](0083-gap-entre-os-botoes-da-zona-fixa.md)):** os respiros entre sessões, configurações e os botões de janela passaram a 3px, e a zona a **92px** (95px no macOS). Os 98px acima são o que valia aqui.

## Consequências

- Os botões ficam com o mesmo peso visual do botão do app, e o hover deles é o mesmo quadrado 40×40.
- O alvo de clique deixa de ser mais largo que o desenho: o respiro lateral de `icon_button_padding_x` não entra mais na largura desses botões (a caixa já é quadrada). O respiro continua valendo para o fechar da aba e o caret da pílula.
- A trilha perde 4px à direita.
- Aval visual do dono pendente.

## Revisões em outros ADRs

[ADR-0055](0055-botao-e-popover-de-sessoes.md) (anatomia 38×30, 94px) e [ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md) (fundo 30×30, alvo 38×30) trazem um blockquote de revisão apontando para este.
