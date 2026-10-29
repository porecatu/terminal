# ADR-0079 — Botão de menu do app: zona fixa à esquerda, com a caixa de uma aba solta e o ícone do app

**Status:** Aceito — aval visual pendente
**Data:** 2026-10-29
**Relacionados:** [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), [ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md)

## Contexto

O dono do produto pediu um botão quadrado no chrome, **antes** da barra de grupos e abas, "exatamente no mesmo formato de uma aba solta", com o ícone do app. Ele será o botão de menu do app; por ora não tem ação, mas já tem hover e clique. A interface do v1 está fechada ([ADR-0032](0032-interface-do-v1-fechada.md)): mexer na anatomia da barra exige ADR, e este é o ADR. O pedido dá o aval para a mudança; o aval visual do resultado, como nas outras, fica pendente.

## Decisão

1. **Zona fixa à esquerda.** O botão não rola com a trilha: mora fora do recorte, como a zona da direita ([ADR-0055](0055-botao-e-popover-de-sessoes.md)). A trilha começa depois dele.
2. **Geometria sem valor novo.** Lado `tab_height + 2 · wrapper_padding` (40 no default) — a caixa que [`layout`] dá à aba solta —, topo em `trilha_padding`, `x = left_inset + trilha_padding`. No macOS o inset dos semáforos vem **antes** do botão e a trilha inteira acompanha. A trilha começa em `track_shift = left_inset + trilha_padding + lado`, e o conteúdo dela mantém `trilha_padding` de respiro antes do primeiro grupo.
3. **Aparência de aba solta.** Fundo e borda de `tab_inactive_*`, raio `tab_corner_radius`, sombra de aba solta quando `group_shadow_enabled`, hover pelo brilho `tab_hover_brightness`. **Pressed** usa as cores de aba **ativa** (`tab_active_*`, com o mesmo brilho de hover): a aba não tem estado pressed, e a ativa é a única "marcada" que ela tem. Pressed só vale com o cursor ainda sobre o botão armado, como nos outros botões da zona fixa.
4. **Ícone.** O PNG do app (`assets/icon/porecatu.png`, o mesmo do ícone da janela) desenhado por `Primitive::Image`, centrado, com lado `icon_em_size`. A textura sobe uma vez por `GpuContext`, com mips (`background_image::decode`), porque o PNG de 512 px a poucas dezenas de px faria alias. Falhou a decodificação: o botão aparece sem o ícone.
5. **Sem ação.** Press arma (`pressed_zone_button`), release sobre o mesmo botão só pede redesenho. `ZoneButtonHit::AppMenu` entra no mesmo caminho dos botões de sessões e configurações, testado em coordenadas de tela **antes** da trilha e do fallback da drag region — clicar nele não arrasta a janela. Clicar à esquerda da trilha (margens do botão, inset do macOS) vira drag region e nunca acerta uma aba rolada para baixo do recorte.
6. **Coordenadas.** `tela = conteúdo + track_shift − scroll_offset`. `trilha_width` passou a descontar as duas zonas, e `overflow_state` segue em coordenadas de conteúdo; as pílulas de overflow recebem `track_left` e `track_width`. `fit_width` deixou de depender da plataforma: o inset do macOS saiu do conteúdo e foi para a zona fixa.
7. **Acessibilidade.** Nó `Button` "Menu do aplicativo" (`access.app_menu_button`, nos cinco idiomas), o primeiro da barra na ordem de leitura. Sem tooltip: nenhum botão da barra tem.

## Consequências

- A trilha rolável fica 46 px mais estreita (40 do botão + 6 de respiro); a rolagem, as pílulas de overflow, o arraste de aba e a âncora do menu de perfil usam a nova origem.
- O botão é o ponto de entrada do menu do app quando houver um; o ADR dele define o conteúdo e a ação.
- Cada conversão tela↔conteúdo em `porecatu-ui` ganhou `track_shift`; esquecer um ponto desloca clique ou hover em 46 px — por isso os testes de `tab_bar` e `chrome` fixam a origem.
- `ImageId` do ícone é do processo, não da janela: uma textura serve todas.

## Riscos

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Algum ponto tela↔conteúdo esquecido desloca clique/hover | Média | Médio | Testes de geometria; busca por `scroll_offset` |
| Aval visual do dono ainda não dado (ícone, pressed com cor de aba ativa) | Alta | Baixo | Registrado no roadmap como pendente |
| Ícone sem alias depende dos mips | Baixa | Baixo | Reuso de `background_image::decode`, que já os gera |
