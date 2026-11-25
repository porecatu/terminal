# ADR-0117 — Recorte da Iosevka sem os caracteres largos e os símbolos espremidos

**Status:** Proposto — implementado localmente, sem commit; aval visual do dono pendente
**Data:** 2026-11-25
**Relacionados:** [ADR-0016](0016-fontes-embutidas.md), [ADR-0025](0025-iosevka-no-lugar-da-ibm-plex.md), [ADR-0116](0116-glyph-de-fallback-encaixado-pela-tinta.md)
**Revisa:** [ADR-0025](0025-iosevka-no-lugar-da-ibm-plex.md) "Subsetting, agora permitido" (**parcial**: dentro das faixas mantidas, dois grupos de codepoints saem do recorte). O blockquote de revisão entra no ADR-0025 quando este for aceito.

## Contexto

O [ADR-0116](0116-glyph-de-fallback-encaixado-pela-tinta.md) corrigiu o tamanho dos símbolos que vêm do fallback do sistema. Com a config default, porém, `⚠` e `⏵` não são fallback: a Iosevka Fixed embutida os cobre, e o dono pediu a correção também nela.

A variante Fixed desenha em meia em tudo o que a Iosevka normal desenha numa em inteira. Medido na 34.8.1, nas faixas do recorte:

- **853 codepoints** são largos na Iosevka normal. A Fixed os redesenha ou os reduz, quase sempre a ~65% da altura original. Estão no grupo `●`, `▶`, `■`, `⏺`, `✓`, `✽`, `…` e as setas. É o desenho da variante, e é o que faz a grade das TUIs sair inteira da face embutida (ADR-0025).
- **24 codepoints** têm largura dupla na grade (East Asian Width `W`), mas a Fixed os desenha numa célula só: `⌚ ⌛ 〈 〉 ⏩ ⏪ ⏫ ⏬ ⏰ ⏳ ◽ ◾ ⚡ ⚪ ⚫ ✅ ❌ ❎ ➰ ➿ ⬛ ⬜ ⭐ ⭕`. O `alacritty_terminal` reserva duas células, e o glyph de meia em sai encostado à esquerda delas, com uma célula vazia ao lado. Isso não é desenho, é erro: a face não serve para esses caracteres.
- **`⚠`** (U+26A0) sai como um triângulo de 0.44 em de largura, que lê como um `A`. **`⏴ ⏵ ⏶ ⏷`** (U+23F4 a U+23F7, triângulos médios) saem com 0.22 × 0.44 em, 60% da altura de versal. O `⚠` e o `⏵⏵` aparecem em toda linha de status do Claude Code.

## Decisão

**O recorte de `scripts/subset-fonts.py` tira, de dentro das faixas mantidas, todo caractere de largura dupla e os símbolos de `EXCLUIDOS`.** Esses caracteres passam a vir do fallback do sistema, que o ADR-0116 encaixa na célula pela tinta.

1. **Largura dupla, calculada.** O script tira todo codepoint com `unicodedata.east_asian_width` igual a `W` ou `F`, a mesma propriedade com que o `unicode-width` marca a célula como larga. A regra é a propriedade, não uma lista: um codepoint que entre numa faixa nova sai sozinho.
2. **Espremidos, listados.** `EXCLUIDOS` lista `26A0` e `23F4-23F7`, cada um com o motivo. Entra nela só um símbolo que a Fixed desenha **visivelmente menor que o texto** e que aparece em saída de programa comum. A redução sistemática a ~65% não basta, porque é o desenho da variante para centenas de marcadores de TUI.
3. **A lista explícita vai em arquivo** (`--unicodes-file`), porque passa do limite de linha de comando do Windows.
4. **Regerado dos originais.** Os TTF foram regerados a partir dos pacotes originais da 34.8.1 (`PkgTTF-IosevkaFixed-34.8.1`). Antes da mudança, o script reproduziu os arquivos versionados **byte a byte**, e depois dela tirou exatamente 29 codepoints: o cmap foi de 3132 para 3103.

## Alternativas consideradas

### Tirar todo símbolo que a Fixed reduz (os 853)

É o critério mais "limpo" no papel. Mas mandaria ao fallback `●`, `▶`, `✓`, `…`, as setas e o `⏺`/`✽` do Claude Code, que hoje saem da face na grade, monocromáticos e na cor SGR do programa. No Windows, boa parte deles viraria emoji colorido da Segoe UI Emoji, sem a cor que o programa pediu, e sem garantia nenhuma no Linux. É o oposto do que o ADR-0025 quis da Iosevka.

### Tirar só os 24 de largura dupla

Corrige o erro objetivo, mas não o que o dono relatou: `⚠` e `⏵` continuariam estreitos com a config default.

### Embutir a Iosevka normal (não Fixed) para esses símbolos

Daria o desenho largo da própria família, mas a Iosevka normal traz ligaduras e é outra face para embutir, recortar e manter. E o glyph largo numa célula de largura um ainda dependeria do transbordo do ADR-0116, como o fallback.

### Deixar como está

O `⚠` e o `⏵⏵` continuariam pequenos na config default, e os 24 largos continuariam errados.

## Consequências

### Positivas

- Na config default, `⚠`, `⏵⏵` e os 24 largos saem do tamanho do texto, pelo mesmo caminho da família do dono.
- `✅`, `⭐`, `⚡` e os demais largos ocupam as duas células que a grade reserva.
- As faces ficam ~3 KB menores cada (29 glyphs a menos: 422 776 → 419 508 bytes a Regular).

### Negativas

- `⚠`, `✅`, `⭐`, `⚡` e os demais que a Segoe UI Emoji cobre saem como **emoji colorido**, que ignora a cor SGR do programa. Um `⚠` pintado de amarelo pelo programa sai laranja do emoji.
- Esses 29 codepoints deixam de ser garantidos pelo binário: numa máquina sem fonte de símbolo ou de emoji, saem como retângulo vazio, como o ADR-0016 já aceita para emoji.
- A lista `EXCLUIDOS` é julgamento, não propriedade Unicode. Cada entrada nova precisa do motivo ao lado.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Linux sem fonte que cubra `⏵` ou `⚠` | Média | Retângulo vazio no lugar do símbolo | A cadeia `[terminal.font] fallback` do usuário (RF-5.2); registrado como dívida no roadmap |
| Python com outra versão do Unicode muda a lista de largura dupla | Baixa | Recorte diferente ao regerar | O script imprime a versão do Unicode usada; o teste `the_embedded_face_leaves_wide_and_shrunken_symbols_to_the_fallback` trava os casos conhecidos |
| Subir a versão da Iosevka muda o desenho dos excluídos | Baixa | Exclusão sem motivo | O motivo de cada entrada cita a medida; conferir ao subir, como a lista de faixas |
