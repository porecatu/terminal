# ADR-0066 — Estado pressionado dos botões de janela

**Status:** Aceito
**Data:** 2026-10-22
**Relacionados:** [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0031](0031-temas-nomeados.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0059](0059-janela-de-configuracoes.md), [PRD-004](../prd/prd-004-aparencia-do-chrome.md)

## Contexto

Os botões de minimizar, maximizar/restaurar e fechar do [ADR-0027](0027-controles-de-janela-e-resize-proprios.md) têm dois estados visuais: repouso e hover. Desde a correção que passou o disparo para o release, o press só **arma** o botão (`pressed_window_button`) e quem dispara é o release sobre o mesmo botão (`tab_bar::window_button_release`) — mas nada muda na tela entre um e outro. Segurando o clique, o usuário não sabe se o botão registrou o press.

O dono do produto pediu o estado "pressionado" (o `:active` do CSS) nos três botões, nas duas janelas que os desenham: a de terminal e a de configurações ([ADR-0059](0059-janela-de-configuracoes.md) §2, que reusa `chrome::paint_window_buttons`).

É mudança das seções 1/2 da especificação visual, e o [ADR-0032](0032-interface-do-v1-fechada.md) exige ADR para isso.

## Decisão

### 1. Duas cores novas em `[appearance.window_controls]`

| Chave | Default | Uso |
|---|---|---|
| `pressed_background` | `#39404b` | minimizar e maximizar/restaurar pressionados |
| `close_pressed_background` | `#a33533` | fechar pressionado |

`#39404b` não é cor nova: é o hover do ✕ da aba (`[appearance.tabs] close_button_hover_background`) e do botão de restaurar da tela de configurações, um degrau acima do hover `#252a33`. `#a33533` é o vermelho do hover (`#c4413f`) escurecido, a convenção do Windows 11 para o fechar pressionado. Os três temas claros sobrescrevem as duas em `[themes.window_controls]`, um degrau além do hover de cada um.

### 2. O ícone não ganha cor própria

O ícone do fechar pressionado é `close_hover_foreground`, o mesmo do hover. Os outros dois ficam na cor de repouso, como no hover.

### 3. Pressionado só com o cursor sobre o botão armado

O fundo de pressionado aparece quando o botão está armado **e** o cursor está sobre ele — a mesma condição em que o release dispara. Arrastado para fora com o botão apertado, o botão volta ao repouso; de volta para dentro, volta a pressionado. É o comportamento dos botões nativos do Windows, e faz o visual dizer a verdade sobre o que o release vai fazer.

### 4. Nenhuma primitiva e nenhum estado novo

O estado já existia (`pressed_window_button`, em `WindowState` e em `SettingsWindow`), e o press, o release e a perda de foco já pedem redraw. A mudança é um parâmetro a mais em `chrome::paint_window_buttons`.

## Alternativas consideradas

### Derivar do hover por fator, em CPU

Sem chave nova: pressionado seria o hover multiplicado por um fator fixo (`chrome::brighten`). O fator seria um número novo que só a especificação conheceria, e os temas não teriam como ajustar o resultado. Descartada pelo dono do produto.

### Manter pressionado enquanto armado, onde quer que esteja o cursor

Mostraria pressionado sobre um botão que não vai disparar se soltar ali. Descartada pelo dono do produto.

## Consequências

### Positivas

- Feedback do press nos três botões, nas duas janelas, pelo mesmo código.
- Ajustável por arquivo e por tema, como o hover.

### Negativas

- Duas chaves a mais em `[appearance.window_controls]` e em `[themes.window_controls]`.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Tema do usuário que muda o hover e esquece o pressionado fica com um pressionado do tema escuro | Média | Baixo | As duas chaves estão no arquivo de exemplo, ao lado das de hover, em todo tema |
