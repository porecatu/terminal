# ADR-0085 — Atividade no ponto da aba do grupo colapsado e margem direita do "x"

**Status:** Aceito — aval visual do dono do produto dado em 2026-11-04 (só no Windows)
**Data:** 2026-11-04
**Relacionados:** [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md), [PRD-002](../prd/prd-002-grupos-de-abas.md) (RF-2.16)

## Contexto

Dois ajustes visuais pedidos pelo dono do produto sobre a barra de abas do [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md), vistos no binário e aprovados. Mexem na especificação visual (§1.7, §2.4, §2.5), coberta pelo [ADR-0032](0032-interface-do-v1-fechada.md).

- Com o grupo colapsado e atividade em alguma aba, aparecia um ponto **agregado** à esquerda do nome (RF-2.16), além dos pontos de aba à direita dele: dois sinais para a mesma coisa.
- A margem direita do quadrado de hover do "x" era fixa (`padding_right` 6 + 0,5 de centragem = 6,5px), então não casava com a margem de cima e de baixo, que é de 8px na aba solta (40px) e de 5px na aba de grupo (34px).

## Decisão

### 1. O ponto da aba leva a cor do indicador; o agregado sai

Com o grupo colapsado, cada ponto de aba (`indicator_dot_size`, 6px, já introduzido pelo ADR-0084 §2) é pintado com a cor do indicador **da própria aba**: `activity_indicator` com atividade, `bell_indicator` com campainha. Sem indicador, continua na cor do nome (`group_new_tab_icon`). A regra é a da aba expandida (§2.17) — campainha vence atividade, aba `Exited` não mostra nada — e mora num só lugar, `tab_bar::tab_indicator`, que serve às duas.

O ponto agregado à esquerda do nome sai, com a largura que ele tirava do nome da pílula (`GroupPillRect::aggregate_indicator`/`aggregate_indicator_origin` saem; entra `dot_indicators`, um por ponto). A pílula colapsada tem a mesma largura com e sem atividade, e o RF-2.16 passa a ser lido assim: o grupo colapsado com atividade em alguma aba mostra o ponto dessa aba na cor de atividade.

### 2. A margem direita do "x" é a vertical

O quadrado de hover do "x" fica a `(altura da aba − close_hover_size) / 2` da borda direita da aba, a mesma distância do topo e da base: **8px** na aba solta (era 6,5) e **5px** na aba de grupo (era 6,5). O retângulo do botão (25×24) e a área de clique, com a folga de 2px, acompanham. A largura fixa da aba (209px) não muda, e na aba solta a folga entre o fim do rótulo e o botão passa de 8 para 6,5px.

## Alternativas consideradas

- **Manter o agregado e colorir também o ponto da aba**: rejeitada pelo dono — era a redundância que ele quis tirar.
- **Mudar `padding_right` para casar a margem**: rejeitada. Vale para as duas alturas de aba e só uma das duas margens verticais bate com ele; e entra na conta da largura fixa e do teto do rótulo.

## Consequências

- Um sinal só de atividade no grupo colapsado, no lugar onde a aba estava representada.
- A pílula colapsada deixa de variar de largura com atividade, o que também elimina um reflow da barra.
- Com vários pontos coloridos, o grupo diz **quais** abas pedem atenção, não só que alguma pede.

## Revisões em outros ADRs

O [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md), §2, traz um blockquote de revisão apontando para este (o ponto agregado saiu).
