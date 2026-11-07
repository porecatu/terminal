# ADR-0089 — Ranhura de soltura da zona de fixadas com o botão do app oculto

**Status:** Aceito
**Data:** 2026-11-07
**Relacionados:** [ADR-0079](0079-botao-de-menu-do-app.md), [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md), [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md), [ADR-0087](0087-zona-de-abas-fixadas-na-barra.md), [PRD-021](../prd/prd-021-abas-fixadas.md)
**Supersedes (parcial):** ADR-0087 §5 (a frase "uma fileira sem nenhuma caixa não é zona")

## Contexto

O [ADR-0087](0087-zona-de-abas-fixadas-na-barra.md) §5 decide o alvo do arraste pelo centro do fantasma: à esquerda de `track_shift` é a zona de fixadas, inclusive sobre o botão do app. O mesmo ADR dizia que uma fileira sem nenhuma caixa — `show_app_menu_button = false` e nada fixado — **não é zona**, e um teste fixava isso.

O efeito não foi previsto: sem o botão e sem fixadas, `track_shift` é só `left_inset` (0 fora do macOS), e o centro do fantasma nunca fica à esquerda dele. O botão do app era a única "porta" para a primeira fixada pelo arraste. Quem o oculta para recuperar espaço perde o gesto (o item "Fixar aba" do menu de contexto continua funcionando).

## Decisão

**Uma fileira vazia tem uma ranhura virtual de soltura, só durante o arraste: a caixa que o botão ocuparia.** `tab_bar::drop_zone_edge` devolve a borda direita da zona de soltura: `track_shift` quando a fileira tem alguma caixa, e, quando não tem, a borda que `track_shift` teria com uma caixa (`left_row_box(0)` mais `trilha_padding`). `tab_drag_target` compara o centro do fantasma com ela.

- **Em repouso nada muda.** A trilha continua em `track_shift = left_inset`, e o espaço recuperado pelo botão oculto continua recuperado. A conversão tela→conteúdo do ramo da trilha continua usando o `track_shift` real.
- **Sem valor novo.** A ranhura tem as medidas da caixa do botão (ADR-0079/ADR-0087 §1); nenhuma cor, dimensão ou raio entra. O realce e o vão são os do ADR-0087 §5, sobre `pinned_rect(0)`.
- **Efeito aceito.** Com o botão oculto, soltar com o centro do fantasma nos primeiros `trilha_padding + S + trilha_padding` px (52 no default) **fixa** em vez de abrir um *run* antes do primeiro grupo — a mesma regra que já vale com o botão visível. Como o fantasma da trilha tem 209 px, o centro só chega lá com o cursor encostado na borda esquerda.
- O defeito cosmético já registrado (preview do arraste desloca a trilha 6 px quando a zona vai de 0 para 1 fixada, com o botão oculto) segue como está: o alvo e o drop não são afetados.

## Alternativas consideradas

- **Reservar sempre uma caixa vazia à esquerda:** desfaz o motivo de ocultar o botão e muda a aparência em repouso, o que exige aval do dono.
- **Deixar só o menu de contexto:** o gesto de arrastar passa a depender de uma configuração de aparência, sem razão que o usuário veja.

## Consequências

- `a_row_without_boxes_is_not_a_drop_zone` vira `a_row_without_boxes_still_has_a_virtual_drop_slot`; um teste novo fixa que `drop_zone_edge == track_shift` sempre que a fileira tem caixa.
- Aval visual do dono pendente, como o resto da zona: a pintura é a de sempre, mas o realce aparece agora numa posição em que antes não havia zona.
