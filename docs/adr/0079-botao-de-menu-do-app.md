# ADR-0079 — Botão de menu do app: zona fixa à esquerda, com a caixa de uma aba solta e o ícone do app

**Status:** Aceito
**Data:** 2026-10-29
**Relacionados:** [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), [ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md)

## Contexto

O dono do produto pediu um botão quadrado no chrome, **antes** da barra de grupos e abas, "exatamente no mesmo formato de uma aba solta", com o ícone do app. Ele será o botão de menu do app; por ora não tem ação, mas já tem hover e clique. A interface do v1 está fechada ([ADR-0032](0032-interface-do-v1-fechada.md)): mexer na anatomia da barra exige ADR, e este é o ADR. O pedido dá o aval para a mudança, e o dono deu depois o aval visual do botão e do tint roxo.

## Decisão

1. **Zona fixa à esquerda.** O botão não rola com a trilha: mora fora do recorte, como a zona da direita ([ADR-0055](0055-botao-e-popover-de-sessoes.md)). A trilha começa depois dele.
2. **Geometria sem valor novo.** Lado `tab_height + 2 · wrapper_padding` (40 no default) — a caixa que [`layout`] dá à aba solta —, topo em `trilha_padding`, `x = left_inset + trilha_padding`. No macOS o inset dos semáforos vem **antes** do botão e a trilha inteira acompanha. A trilha começa em `track_shift = left_inset + trilha_padding + lado`, e o conteúdo dela mantém `trilha_padding` de respiro antes do primeiro grupo.
3. **Aparência de aba solta.** Fundo e borda de `tab_inactive_*`, raio `tab_corner_radius`, sombra de aba solta quando `group_shadow_enabled`, hover pelo brilho `tab_hover_brightness`. **Pressed** usa as cores de aba **ativa** (`tab_active_*`, com o mesmo brilho de hover): a aba não tem estado pressed, e a ativa é a única "marcada" que ela tem. Pressed só vale com o cursor ainda sobre o botão armado, como nos outros botões da zona fixa.
4. **Ícone.** O PNG do app (`assets/icon/porecatu.png`, o mesmo do ícone da janela) desenhado por `Primitive::Image`, centrado, com lado `icon_em_size`. A textura sobe uma vez por `GpuContext`, com mips (`background_image::decode`), porque o PNG de 512 px a poucas dezenas de px faria alias. Falhou a decodificação: o botão aparece sem o ícone.
5. **Sem ação.** Press arma (`pressed_zone_button`), release sobre o mesmo botão só pede redesenho. `ZoneButtonHit::AppMenu` entra no mesmo caminho dos botões de sessões e configurações, testado em coordenadas de tela **antes** da trilha e do fallback da drag region — clicar nele não arrasta a janela. Clicar à esquerda da trilha (margens do botão, inset do macOS) vira drag region e nunca acerta uma aba rolada para baixo do recorte.
6. **Coordenadas.** `tela = conteúdo + track_shift − scroll_offset`. `trilha_width` passou a descontar as duas zonas, e `overflow_state` segue em coordenadas de conteúdo; as pílulas de overflow recebem `track_left` e `track_width`. `fit_width` deixou de depender da plataforma: o inset do macOS saiu do conteúdo e foi para a zona fixa.
7. **Acessibilidade.** Nó `Button` "Menu do aplicativo" (`access.app_menu_button`, nos cinco idiomas), o primeiro da barra na ordem de leitura. Sem tooltip: nenhum botão da barra tem.

> **Revisão (pedido do dono, 2026-10-29) — ocultar e tint roxo.** Não muda as decisões 1–7 com o botão visível; acrescenta duas coisas.
>
> **a) Ocultar.** `[appearance.tabs] show_app_menu_button` (bool, default `true`, recarga A) e o toggle correspondente no grupo Aparência da tela de configurações ([PRD-016](../prd/prd-016-tela-de-configuracoes.md) RF-16.11). Com `false`: nada é desenhado, o retângulo não é atingido (`point_in_app_menu_button` devolve `false`), o nó de acessibilidade some e `track_shift` cai para `left_inset` — 0 fora do macOS, 78 no macOS, onde o semáforo continua reservado. Nenhuma outra conversão tela↔conteúdo muda: todas passam por `track_shift`. A trilha ganha de volta os 46 px.
>
> **b) Tint roxo fixo.** O fundo da caixa é a cor de aba (inativa; ativa no pressed) misturada com `#A78BFA` (`APP_MENU_TINT`, em `chrome.rs`) na proporção `APP_MENU_TINT_MIX = 0.30`, **antes** do brilho de hover; borda e ícone ficam como estavam. É uma cor **fixa por decisão do dono**: sem chave no TOML e fora dos `thm_*.toml`, portanto não entra na regra de "cor nova em `Config`". A proporção `0.30` é valor de implementação, aprovado em tela pelo dono.

> **Revisão (pedido do dono, 2026-10-29) — aparência da aba ativa; o tint sai.** Substitui a decisão 3 e a alínea b) acima. O botão passa a ter **exatamente** a aparência de uma aba solta **ativa**: fundo `tab_active_background`, borda `tab_active_border` com `active_border_width`, hover pelo brilho `tab_hover_brightness` sobre o fundo ativo. O pressed é o da aba ativa apertada: a aba não pinta um estado próprio, mas o press arma `Drag::TabPressed`, que suprime o hover da barra (`bar_hover`), e ela volta ao fundo ativo **sem** o brilho — mais escuro que no hover. O botão faz o mesmo (só com o cursor sobre o botão armado). `APP_MENU_TINT`, `APP_MENU_TINT_MIX` e `mix` foram removidos de `chrome.rs`. Dimensões, sombra de aba solta, ícone, zona fixa e demais decisões não mudam. Aval visual do dono **pendente**.

> **Revisão (pedido do dono, 2026-10-29) — margem entre o botão e o recorte da trilha.** Altera a decisão 2. Com a trilha cheia, a aba rolada era cortada rente à borda direita do botão: o respiro de `trilha_padding` entre os dois morava **dentro** do conteúdo (`layout` nascia em `x = trilha_padding`) e rolava junto. Agora o respiro mora em `track_shift`: `left_inset + 2 · trilha_padding + lado` (46 → 52 px com o botão, a mesma margem de 6 px que ele tem no topo e à esquerda), e o recorte abre aí. Com o botão **visível**, o conteúdo nasce em `x = 0`, então em repouso a primeira aba fica onde sempre ficou (a 6 px do botão); rolada, some a 6 px dele. Com o botão **oculto** nada muda: `track_shift = left_inset` e o conteúdo mantém o respiro de `trilha_padding` das bordas, como antes. A trilha rolável fica 6 px mais estreita. Efeito colateral aceito: a sombra do primeiro grupo, que ocupava esse respiro, é cortada pelo recorte no lado esquerdo.

## Consequências

- A trilha rolável fica 46 px mais estreita (40 do botão + 6 de respiro); a rolagem, as pílulas de overflow, o arraste de aba e a âncora do menu de perfil usam a nova origem.
- O botão é o ponto de entrada do menu do app quando houver um; o ADR dele define o conteúdo e a ação.
- Cada conversão tela↔conteúdo em `porecatu-ui` ganhou `track_shift`; esquecer um ponto desloca clique ou hover em 46 px — por isso os testes de `tab_bar` e `chrome` fixam a origem.
- `ImageId` do ícone é do processo, não da janela: uma textura serve todas.

## Riscos

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Algum ponto tela↔conteúdo esquecido desloca clique/hover | Média | Médio | Testes de geometria; busca por `scroll_offset` |
| Ícone sem alias depende dos mips | Baixa | Baixo | Reuso de `background_image::decode`, que já os gera |
