# ADR-0081 — Disparo no release para botões e itens

**Status:** Aceito — aval visual do estado pressionado pendente
**Data:** 2026-10-29
**Relacionados:** [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), [ADR-0059](0059-janela-de-configuracoes.md), [ADR-0066](0066-estado-pressionado-dos-botoes-de-janela.md), [ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md), [ADR-0080](0080-editor-de-perfil-em-janela-propria.md), [PRD-010](../prd/prd-010-interacao-e-superficie-de-app.md)

## Contexto

Os botões de janela, o "+", o fechar da aba e os botões da zona fixa da barra já disparam no **release** sobre o mesmo botão que o press armou ([ADR-0066](0066-estado-pressionado-dos-botoes-de-janela.md), [ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md)). Todo o resto disparava no **press**: a tela de configurações, o editor de perfil e todos os overlays da janela principal (diálogos, menus de contexto, popovers, editor de grupo, avisos, barra de busca, indicador de commits). O comportamento era inconsistente, e o press agia antes de o usuário poder desistir arrastando o cursor para fora.

## Decisão

### 1. Regra geral

Botão e item **armam no press e agem no release, se o cursor está sobre o mesmo alvo**. Soltar fora do alvo, ou sobre outro, cancela. Perder o foco da janela desarma. A decisão é uma função só, `input::release_on_armed`, que a tela de configurações, o editor de perfil e a janela principal usam.

### 2. O que continua no press

- **Clique fora** de um popover, menu, lista aberta ou diálogo (fechar é instantâneo, como nos menus do Windows).
- **Foco e arraste de campo de texto** (campos da tela, do editor de perfil, da busca, do nome do grupo e da sessão, do segredo).
- **Gestos de terminal**: seleção, mouse reporting, Ctrl+clique em hyperlink, conexão de painel SSH dormente.
- **Arrastes**: aba, grupo, divisor de painéis, borda de resize, drag region, chevrons de overflow da trilha.

### 3. Overlays da janela principal

`WindowState.armed_overlay` guarda um `OverlayTarget` (o alvo já resolvido, na ordem de precedência da cascata). No release, se o cursor ainda está sobre o mesmo alvo, o press é **refeito** (`dispatch_mouse_input_impl(.., replay = true)`): o código que sempre tratou o clique é o mesmo, só muda quando roda. Todo press que um overlay modal consome marca `overlay_press_consumed`, e o release correspondente nunca chega ao terminal (antes caía em `input::handle_mouse_button`, podendo disparar `copy_on_select`).

### 4. Tela de configurações e editor de perfil

`SettingsWindow` e `ProfileEditorWindow` ganham `armed`. `left_pressed` só arma o que não é texto; `left_released` refaz a ação (`activate_hit`, `click_control`, `press_button`). Duplo clique em linha de perfil conta no release.

### 5. Estado pressionado

Um alvo armado, com o cursor ainda sobre ele, pinta **um passo de brilho acima do hover** (`settings::paint::pressed_fill`, o mesmo `tab_hover_brightness`), sem cor nem chave nova. Cobre, por ora, os botões do rodapé da tela e do editor, os botões da faixa de conflito e os itens da guia. Os demais alvos (controles de linha, itens de menu e popover, botões de diálogo) só mudam o **disparo**; o estado pressionado deles é dívida.

## Consequências

- Não há teste que simule `dispatch_mouse_input` (precisa de janela winit): a lógica testável é `release_on_armed` e a pintura; o resto é verificação ao vivo.
- O release de um press de overlay não repassa mais nada ao terminal.
- Soltar o botão fora do alvo é um gesto de desistência em todo o app, não só nos botões da barra.
