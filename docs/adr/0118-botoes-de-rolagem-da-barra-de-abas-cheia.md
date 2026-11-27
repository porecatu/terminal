# ADR-0118 — Botões de rolagem da barra de abas cheia

**Status:** Proposto — implementado; verificado só por teste automatizado (verificação ao vivo no Windows, macOS e Linux é dívida); aval visual do dono pendente
**Data:** 2026-11-27
**Relacionados:** [ADR-0018](0018-composicao-de-frame.md), [ADR-0024](0024-face-de-icones.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md), [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md) (§1, **revisto**)
**Supersedes:** ADR-0084 §1 (parcial)

## Contexto

O [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md) §1 tirou os botões de abas fora da vista: com a barra cheia, a trilha só rolava pela roda do mouse e o recorte na ponta era o único sinal de que havia mais abas. O dono pediu, em 2026-11-27, que a barra cheia **volte a ter botões de rolagem**, de outro desenho: dois botões **retangulares e estreitos**, sem cantos arredondados e sem borda, de cima a baixo da barra, com hover e pressionado, e chevrons do mesmo tamanho dos demais ícones do chrome (o "+", por exemplo), com um respiro horizontal pequeno entre o chevron e as bordas do botão.

O ADR-0084 revisa-se só neste ponto: o resto dele (rótulo do grupo, fonte do sistema, "x" da aba) continua valendo. O ADR-0032 (interface fechada) exige ADR para mexer na especificação visual; este é ele.

## Decisão

### 1. Quando existem

Só com a trilha **estourada**: `content_width` maior que a largura da trilha (`tab_bar::trilha_width`). Se tudo cabe, a barra é pixel por pixel a de antes. A decisão usa a largura **inteira** da trilha, nunca a que sobra depois dos botões, senão a barra oscilaria entre "cabe" e "estoura" a cada quadro.

### 2. Anatomia

- Dois botões, um em cada ponta da janela da trilha (`track_shift` e `track_shift + trilha_width`), **parados** como as zonas fixas, fora do recorte e fora do scroll.
- **Altura: a barra inteira** (`chrome::bar_height`, do topo à base), **sem raio e sem borda**. Retangulares: bem mais estreitos que altos.
- Chevron Lucide (`chevron-left` / `chevron-right`, [ADR-0024](0024-face-de-icones.md)) no tamanho dos demais ícones do chrome (`icon_em_size`, o mesmo do "+"), cor `chrome_icon`, centrado.
- **Largura:** a tinta do chevron mais `SCROLL_BUTTON_PADDING_X` (6px) de cada lado, arredondada para cima: `tab_bar::scroll_button_width`. Constante, sem chave no TOML (como `COLLAPSED_DOTS_GAP`).
- **Hover e pressionado:** o fundo `window_controls.hover_background` / `pressed_background` sobre o retângulo inteiro ([ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md)), pelo mesmo `zone_button_background`. Dispara no **release** sobre o mesmo botão do press, como os da zona fixa, e o pressionado só aparece com o cursor sobre o botão armado.
- **Apagado** quando a trilha já está na ponta daquele lado: o chevron sai na cor de item desabilitado (`menu_item_disabled_text`, token existente) e o botão não reage. Os dois botões aparecem sempre juntos.
- Um clique rola **um passo da roda do mouse** (`[appearance.tabs.overflow] scroll_step`, 90px). Sem inércia e sem easing, como a roda (§2.18).

### 3. A trilha não fica por baixo dos botões

O recorte da trilha passa a ser o que sobra **entre** os botões. A conversão tela-conteúdo não muda (`tela = conteúdo + track_shift − scroll_offset`): o que muda é a faixa do deslocamento, que com a trilha estourada vai de `−inset` (a primeira aba logo depois do botão da esquerda) a `content_width − largura + inset` (a última logo antes do da direita), com `inset` a largura de um botão. Sem estourar, `inset` é 0 e o deslocamento é sempre 0.

Na passagem de "cabe" para "estoura" o deslocamento recua `inset` uma vez, para o começo da trilha não nascer por baixo do botão da esquerda (`overflow_state` recebe se já estourava; `WindowState::update_overflow` é o único que o guarda). Trazer a aba ativa à vista (RF-1.18) usa a janela entre os botões.

### 4. Hit-testing

O botão é dono do pixel, mesmo apagado e no clique direito: por baixo dele há uma aba rolada para fora de vista, que não pode responder no lugar dele (a mesma armadilha do ADR-0084 §7). Hover da aba e tooltip da trilha também o respeitam. Os botões são variantes de `ZoneButtonHit` (`ScrollLeft`, `ScrollRight`) e reaproveitam o estado de armado e de hover da zona fixa; `point_in_zone_button` não os devolve (não sabe se a trilha estoura) — quem os resolve é `point_in_scroll_button`, via `WindowState::zone_button_at`.

## O que não muda

A roda do mouse, o auto-scroll do arraste e a rolagem mínima para a aba ativa seguem como estavam. Nenhuma chave de config nova, nenhuma cor nova, nenhuma frase nova.

## Consequências

- O recorte na ponta deixa de ser o único sinal de abas fora da vista, e quem não tem roda de mouse ganha um jeito de rolar.
- A trilha estourada perde `2 × largura do botão` (≈ 19px × 2) de área visível.
- **Dívida:** os botões **não têm nó na árvore de acessibilidade** ([ADR-0043](0043-arvore-de-acessibilidade.md)); os nós `tabs_hidden_*` que existiam foram tirados pelo ADR-0084 e não voltam aqui. Sem tooltip. Segurar o botão **não repete** a rolagem. Nada disso foi visto rodando.
