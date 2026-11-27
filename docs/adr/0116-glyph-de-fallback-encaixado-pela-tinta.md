# ADR-0116 — Glyph de fallback encaixado pela tinta, com transbordo para a célula vazia

**Status:** Proposto — implementado localmente, sem commit; aval visual do dono pendente
**Data:** 2026-11-25
**Relacionados:** [ADR-0016](0016-fontes-embutidas.md), [ADR-0025](0025-iosevka-no-lugar-da-ibm-plex.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [PRD-005](../prd/prd-005-aparencia-do-terminal.md)

## Contexto

Relato do dono em 2026-11-25: alguns símbolos, como `⚠` e o `⏵⏵` do Claude Code, saem no terminal muito menores do que deveriam.

A config do dono usa `[terminal.font] family = "Google Sans Code"`, a 13 px. Essa família não tem nenhum dos dois, e os dois vêm da cadeia de fallback do sistema (RF-5.2). Desde a F2, `paint.rs` desenha esse glyph num `TextRun` próprio, ancorado na célula e encolhido até o **avanço** caber nela ([descobertas da F2](../descobertas.md)). O avanço é a medida errada para isso. Medido no Windows 11, em fração da em:

| Glyph | Fonte que o desenha | Avanço | Tinta (largura × altura) | Escala de antes |
|---|---|---|---|---|
| `⚠` U+26A0 | Segoe UI Emoji | 1.373 | 1.056 × 0.936 | 44% |
| `⏵` U+23F5 | Segoe UI Symbol | 1.000 | 0.312 × 0.625 | 60% |
| célula da Google Sans Code | — | 0.600 | — | — |

O `⏵` cabe na célula sem encolher nada: a tinta dele tem metade da largura dela. O avanço de 1 em é quase todo margem, e encolher por ele deixava o triângulo com cerca de 2 px de largura. O `⚠` não cabe numa célula de jeito nenhum. Os programas, porém, o escrevem seguido de espaço (`⚠ Warning`), e encolhido para uma célula ele fica com menos da metade da altura do texto.

Com a Iosevka Fixed embutida (a config default), `⚠` e `⏵` **não** são fallback: a Iosevka os cobre e os desenha estreitos, em meia em, por desenho da variante Fixed. Isso não muda aqui (ver "Alternativas").

## Decisão

O glyph de fallback se encaixa na célula pela **tinta**, e um glyph de uma célula pode **transbordar para a célula seguinte quando ela está vazia**. A grade não muda.

1. **Medida.** `TextMeasurer::ink_em` (caractere) e `cluster_ink_em` (grafema composto) devolvem a caixa da tinta (`InkBox`), em fração de `size_px`, relativa à origem do `TextRun`. A medição rasteriza a 128 px pela mesma conta de posição do `glyphon` e entra em cache: uma rasterização por caractere distinto. O cache de grafemas tem teto de 1024 entradas.
2. **Espaço.** O espaço disponível são as células do caractere: uma, ou duas se for `WIDE`. Se a tinta não cabe nele, além da folga da medição (`INK_SLACK_EM`, dois pixels da rasterização a 128 px), o caractere é de uma célula e a célula seguinte é um espaço que não pertence a um caractere largo, o espaço passa a ter duas células (`next_cell_is_blank`). É a regra do kitty e a `allow_square_glyphs_to_overflow_width = "WhenFollowedBySpace"` do WezTerm, o default dele.
3. **Escala.** O glyph só encolhe, e só o necessário para a tinta caber na largura disponível e na altura da linha. Tinta que passa delas só pela folga da medição não encolhe.
4. **Posição** (`place_fallback`, função pura). Na horizontal, o glyph fica onde a fonte o põe, com a pena no `x` da célula, e, se a tinta sai do espaço disponível, é **empurrado para dentro pelo mínimo**; só a tinta mais larga que o espaço é centrada nele. A correção é contínua, proporcional ao excesso (ver "Centrar a tinta quando ela sai da caixa", em Alternativas). Na vertical, o glyph encolhe em torno do centro da própria tinta no tamanho cheio e, se ainda passar da linha, é empurrado para dentro dela.
5. **O que não muda.** Nenhuma coluna muda: largura de caractere, cursor, seleção, hit-testing e o tamanho do PTY continuam os do motor. A célula vazia segue vazia para o programa; só o desenho passa por cima dela. Caracteres que a face mono cobre na largura da grade continuam no run compartilhado, sem medição de tinta. Glyph sem tinta mensurável (`ink_em` devolve `None`) segue o caminho antigo, pelo avanço.

## Alternativas consideradas

### Dar duas colunas na grade aos símbolos tipo emoji

Resolveria o espaço na raiz, mas o programa do outro lado conta uma coluna para `⚠` (`wcwidth`), e a grade desalinharia do que ele escreve: TUIs quebradas, cursor fora do lugar. Uma tentativa nessa linha (o ADR-0046, "largura de ícones tipo-emoji") foi escrita e revertida antes de chegar ao histórico.

### Desenhar sempre no tamanho natural, por cima do vizinho (como o Alacritty)

Não encolhe nada, mas `⚠Texto` sem espaço pinta por cima da letra seguinte. O transbordo só para a célula vazia dá o tamanho cheio no caso comum (`⚠ `) e preserva a legibilidade no resto.

### Só encaixar pela tinta, sem transbordo

Já resolve o `⏵`. O `⚠` seguido de espaço, porém, ainda sairia a 57% (tinta de 1.056 em numa célula de 0.6), pequeno ao lado do texto.

### Centrar sempre a tinta na célula

Mais simples, mas move glyph que ocupa só parte da célula de propósito (sextantes e meios-blocos de fontes de símbolo, que precisam encostar na borda certa). Centrar só quando a posição natural sai da caixa preserva esses glyphs.

### Centrar a tinta quando ela sai da caixa

Era a regra da primeira versão deste ADR, e o `btop` a derrubou (relato do dono em 2026-11-27, com `Google Sans Code`, que não tem braille). O braille vem da Segoe UI Symbol, cuja coluna direita de pontos termina em 0.599 em, numa célula de 0.600. A medição rasteriza em pixel inteiro e conta a borda de antialias, então todo padrão com a coluna direita saía medido um fio além da célula e era recentrado, ~0.08 em (~1.1 px a 14 px) para a esquerda. Os padrões só com a coluna esquerda ficavam no lugar. O gráfico rola uma coluna por atualização, os padrões de cada célula mudam o tempo todo, e os pontos dançavam para os lados. O defeito é o **degrau**: um erro de medição de 0.005 em virava um salto de 0.08 em, e glyphs de uma mesma família caíam de lados diferentes dele. Empurrar para dentro pelo excesso, com a folga nos limiares, deixa o deslocamento proporcional ao erro, subpixel nesse caso.

### Tirar da Iosevka embutida os símbolos que ela espreme

Com a config default, `⚠` e `⏵` vêm da Iosevka Fixed, estreitos por desenho da variante. Tirá-los do recorte (`scripts/subset-fonts.py`) os mandaria ao fallback, e a regra acima os desenharia grandes. Mas isso muda a face embutida, que é decisão de identidade visual ([ADR-0025](0025-iosevka-no-lugar-da-ibm-plex.md)), e não resolve a fonte configurada pelo dono. Fica fora deste ADR: é o [ADR-0117](0117-recorte-da-iosevka-sem-largos-e-espremidos.md).

## Consequências

### Positivas

- `⏵⏵` sai no tamanho da fonte, e `⚠ ` ocupa as duas células, na altura do texto.
- Emoji de duas células (`😀`, `🚀`) sai maior, porque o encaixe também é pela tinta.
- A decisão é toda de desenho: nada no motor, no PTY ou na sessão.

### Negativas

- O glyph transbordado encosta no texto seguinte: `⚠ Warning` fica visualmente perto de `⚠Warning`. Esse é o comportamento do kitty e do WezTerm.
- Com o cursor na célula vazia, o cursor em bloco cobre metade do glyph.
- O `⚠` continua vindo colorido da Segoe UI Emoji, porque é a ordem da cadeia de fallback da plataforma. Essa ordem não muda aqui.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| A tinta medida diverge da posição em que o `glyphon` desenha | Baixa | Glyph deslocado alguns décimos de pixel | Mesma conta de posição do `glyphon` (`x = pena + placement.left`, `y = baseline + deslocamento − placement.top`), medida a 128 px; teste contra o contorno do `H` da Iosevka |
| Custo no caminho quente | Baixa | Frame mais lento em tela cheia de fallback (braille numa fonte sem braille) | Cache por caractere: depois da primeira medição, uma consulta de `HashMap` por célula de fallback, como `advance_em` |
| Glyphs feitos para se encaixar lado a lado (braille, sextantes) com tintas diferentes por caractere | Média | Desenho que pula quando o conteúdo muda | Posição contínua na tinta e folga de medição nos limiares (`INK_SLACK_EM`); teste `braille_patterns_keep_their_columns_put` com as tintas medidas da Segoe UI Symbol |
| Máquina sem fonte de emoji ou de símbolo | Média (CI Linux) | Nada pintado | `ink_em` devolve `None` e o caminho antigo vale; os testes que dependem da fonte se calam sem ela |
