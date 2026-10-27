# ADR-0069 — Hover e pressionado dos botões da zona fixa (sessões e configurações)

**Status:** Aceito (aval visual do dono dado em 2026-10-27)
**Data:** 2026-10-27
**Relacionados:** [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), [ADR-0066](0066-estado-pressionado-dos-botoes-de-janela.md), [ADR-0067](0067-temas-em-arquivos-proprios.md), [PRD-004](../prd/prd-004-aparencia-do-chrome.md), [PRD-014](../prd/prd-014-sessoes-nomeadas.md)

## Contexto

Os botões de janela têm três estados visuais: repouso, hover e pressionado ([ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0066](0066-estado-pressionado-dos-botoes-de-janela.md)). Os dois botões da zona fixa à direita da barra — sessões (`bookmark`) e configurações (`settings`) — tinham menos: o de sessões clareava o ícone no hover (`tab_hover_brightness`), o de configurações não tinha estado nenhum, e os dois disparavam no **press**, sem nada que mostrasse que o clique registrou.

O dono do produto pediu para os dois ganharem hover e clique como os botões de janela, com uma diferença: não ocupam a altura da barra. São botões quadrados de cantos arredondados, sem borda, que se "revelam" como botão só no hover e no clique.

É mudança das seções 1/2 da especificação visual, e o [ADR-0032](0032-interface-do-v1-fechada.md) exige ADR para isso.

## Decisão

### 1. Quadrado arredondado revelado por fundo

Em repouso só o ícone aparece, como hoje. No hover e no pressionado pinta-se, atrás do ícone, um quadrado de lado igual à altura do botão (`right_zone_button_size`, 30), centrado no retângulo do botão, com o raio `tab_corner_radius` (6) e **sem borda**. O alvo de clique continua o retângulo do botão (38×30, com o respiro lateral de `icon_button_padding_x`), então o fundo é uns 4px mais estreito que o alvo de cada lado.

### 2. Cores dos botões de janela, sem chave nova

Hover: `[appearance.window_controls] hover_background`. Pressionado: `pressed_background`. São as dos botões de minimizar e maximizar, e os temas já as sobrescrevem. Nenhuma cor, chave ou dimensão nova; o arquivo de exemplo e os temas não mudam.

### 3. O ícone não muda de cor

O brilho do ícone no hover do botão de sessões sai. Só o fundo muda, como nos botões de janela.

### 4. Disparo no release, pressionado só sobre o botão armado

Como o [ADR-0066](0066-estado-pressionado-dos-botoes-de-janela.md) §3 para os botões de janela: o press só **arma** (`WindowState.pressed_zone_button`), e quem dispara é o release sobre o **mesmo** botão (`tab_bar::zone_button_release`). O fundo de pressionado só aparece com o cursor ainda sobre o botão armado; arrastado para fora, volta ao repouso e o release não dispara. Perder o foco da janela desarma. O que cada botão faz não muda: configurações dispara `settings.open`, sessões abre o popover em navegação. Com o popover aberto, `dispatch_session_picker_click` ainda intercepta o press antes de qualquer botão ser armado, então clicar no botão de sessões com ele aberto continua fechando-o ([ADR-0055](0055-botao-e-popover-de-sessoes.md) §3).

### 5. Estado e primitivas

`tab_bar::ZoneButtonHit` (`Sessions`, `Settings`) identifica o botão, com `point_in_zone_button` sobre os `*_button_rect` existentes. `NewTabRequest::OpenSettings` e `OpenSessionPicker` saem: o clique na barra não devolve mais pedido de abertura, o release é que abre. Nenhuma primitiva nova no render: o fundo é um `RoundedQuad`.

## Alternativas consideradas

### Manter o disparo no press

O fundo de pressionado apareceria por um instante, praticamente invisível, porque o popover ou a janela de configurações abre junto. Descartada pelo dono do produto.

### Chaves de cor próprias

Daria ajuste independente dos botões de janela, ao custo de duas chaves a mais em `[appearance.window_controls]` e em todo tema. Descartada pelo dono do produto: os botões têm a mesma família visual.

### Manter o brilho do ícone junto com o fundo

Descartada pelo dono do produto.

## Consequências

### Positivas

- Feedback de hover e de press nos dois botões, pelo mesmo helper (`chrome::paint_zone_icon_button`), sem divergir entre si.
- O disparo segue a mesma regra dos botões de janela: soltar fora cancela.

### Negativas

- Mudança de comportamento: o botão não abre mais no press, só no release.
- O fundo (30×30) é mais estreito que o alvo de clique (38×30).

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Aparência do fundo revelado não agradar | Baixa | Baixo | O dono verificou e aprovou o visual; as cores vêm de `window_controls` e o raio é `corner_radius`, então um ajuste futuro é de tema ou de uma constante |
