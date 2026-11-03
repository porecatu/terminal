# ADR-0083 — Gap entre os botões da zona fixa

**Status:** Aceito — aval visual do dono do produto pendente (pedido do dono; a aparência nova ainda não foi vista no binário)
**Data:** 2026-11-03
**Relacionados:** [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), [ADR-0082](0082-botoes-da-barra-com-o-tamanho-do-botao-de-menu.md)

## Contexto

Com os botões de sessões e configurações em 40×40 ([ADR-0082](0082-botoes-da-barra-com-o-tamanho-do-botao-de-menu.md)) e os de janela em 46px, com o ícone centrado em cada um, o vão visual entre os ícones ficou grande. Eram dois respiros diferentes:

- entre sessões e configurações: `trilha_gap` (6px, o que separa grupos);
- entre configurações e os botões de janela: `trilha_gap` **mais** `[appearance.window_controls] gap` (6 + 6 = 12px).

O dono do produto pediu para diminuir os dois, e escolheu **3px**. O respiro antes do botão de sessões (que o separa da trilha) não entra no pedido.

É mudança da seção 2 da [especificação visual](../design/especificacao-visual.md), e o [ADR-0032](0032-interface-do-v1-fechada.md) exige ADR para isso.

## Decisão

- **`[appearance.window_controls] gap` passa de 6 para 3** (default do código, `porecatu.example.toml`, `TabBarStyle::DEFAULT`) e governa os **dois** respiros: entre sessões e configurações e entre configurações e os botões de janela. Nenhuma chave nova, nenhum valor novo no projeto.
- O `trilha_gap` deixa de entrar entre configurações e os botões de janela: era o que fazia aquele vão ser o dobro do outro.
- **Antes do botão de sessões** continua o `trilha_gap` (6px), e **no macOS**, sem botões de janela, o `trilha_gap` continua como margem da borda direita, depois da engrenagem.
- `right_zone_width` = 6 + 40 + 3 + 40 + 3 = **92px** antes dos botões de janela (era 98); no macOS, 6 + 40 + 3 + 40 + 6 = **95px**. A trilha ganha 12px fora do macOS.

## Consequências

- Os botões de sessões, configurações e de janela ficam mais juntos, com 3px entre as caixas de hover.
- Um `porecatu.toml` que sobrescreva `[appearance.window_controls] gap` mantém o valor próprio, e ele passa a valer também entre sessões e configurações.
- Aval visual do dono pendente.

## Revisões em outros ADRs

[ADR-0082](0082-botoes-da-barra-com-o-tamanho-do-botao-de-menu.md) (98px) e [ADR-0055](0055-botao-e-popover-de-sessoes.md) (respiros de `trilha_gap` na zona) trazem um blockquote de revisão apontando para este.
