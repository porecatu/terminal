# ADR-0026 — Chrome unificado em Iosevka Fixed

**Status:** Aceito
**Data:** 2026-08-28
**Supersedes:** ADR-0025 (parcial: só a escolha de família para o chrome)
**Relacionados:** ADR-0016, ADR-0018, ADR-0025

## Contexto

O [ADR-0025](0025-iosevka-no-lugar-da-ibm-plex.md) trocou IBM Plex Sans/Mono por Iosevka Aile/Fixed: Fixed para o conteúdo do terminal, Aile para o chrome (título de aba, rótulo de grupo, menu). A troca resolveu o problema que motivou o ADR — cobertura de braille, powerline, geométricos e cirílico —, mas manteve a divisão de família entre terminal e chrome que já existia com a IBM Plex.

Usuário relatou a letra "i" do terminal e a do rótulo de aba como visivelmente diferentes, lado a lado na barra. Correto: Aile e Fixed são variantes desenhadas diferente dentro da mesma superfamília Iosevka — Aile é a variante quasi-proporcional/humanista (pensada para prosa de UI), Fixed é a monoespaçada sem ligadura (pensada para grade de terminal). Mesmo nome de família em `family = "Iosevka Aile"` vs. `"Iosevka Fixed"`, desenho de glyph distinto. O ADR-0025 já tinha considerado e descartado "trocar só a mono, mantendo a IBM Plex Sans no chrome" por coerência de família; o caso agora é o inverso — duas variantes da mesma família ainda leem como duas fontes quando postas lado a lado, então a coerência que faltava era de **desenho**, não de nome de família.

## Decisão

**Chrome usa Iosevka Fixed, a mesma face do terminal.** Iosevka Aile sai do binário.

| Uso | Antes (ADR-0025) | Agora |
|---|---|---|
| Conteúdo do terminal | Iosevka Fixed 400/500 | Iosevka Fixed 400/500 (sem mudança) |
| Chrome (aba, rótulo, menu) | Iosevka Aile 400/500/600 | **Iosevka Fixed 400/500** |

`FontFace::Sans` (`porecatu-render/src/primitives.rs`) continua existindo como nome do **papel** que a face desempenha no chrome (distinto de `FontFace::Mono`, que carrega o parâmetro `bold` da grade e não passa pela mesma tabela de pesos) — não implica família proporcional separada. Internamente, `SANS_FAMILY` em `text_measurer.rs` passa a apontar para a mesma constante que `MONO_FAMILY` (`"Iosevka Fixed"`).

### Peso 600 (`SansWeight::SemiBold`)

Nenhum widget do chrome pedia peso 600 em código antes desta decisão — a tabela de tokens o listava disponível para uso futuro (configurações, F4), mas a face Aile embutida era a única fonte dele. Sem um arquivo `IosevkaFixed-SemiBold.ttf`, a Fixed recortada só embute 400/500. `SansWeight::SemiBold` continua no enum (é decisão de peso, não de família, e não custa nada manter); pedir `Weight::SEMIBOLD` sem um arquivo 600 registrado faz o `fontdb` casar pelo peso mais próximo disponível (Medium, 500) — comportamento padrão do `fontdb`, não um caso especial deste projeto. Se um widget futuro precisar de um 600 de verdade, `scripts/subset-fonts.py` precisa de `IosevkaFixed-SemiBold.ttf` (original) na lista de faces antes de rodar.

### Assets

`assets/fonts/IosevkaAile-{Regular,Medium,SemiBold}.ttf` são removidos. `scripts/subset-fonts.py` recorta só `IosevkaFixed-{Regular,Medium}.ttf` (mais `Lucide.ttf`, sexta face que este ADR não toca). O binário passa de cinco faces embutidas (~2.1 MB) para três (Iosevka Fixed Regular/Medium + Lucide).

## Alternativas consideradas

### Manter Aile no chrome e só ajustar métrica para aproximar o desenho

Ajustar tamanho ou peso não muda o desenho do glyph — "i" de Aile e "i" de Fixed vêm de contornos diferentes no arquivo, não de escala. Descartada: não resolve o que foi relatado.

### Usar Iosevka Fixed no terminal e a variante **Term** (com ligaduras) no chrome, evitando o mono "quadrado" em prosa de UI

Descartada por trazer de volta o risco que o ADR-0025 evitou ao escolher Fixed sobre Term: ligadura funde N glyphs em 1 durante o shaping. O chrome não tem a verificação de avanço por caractere que a grade tem (`paint::fits_the_grid`), então uma ligadura ali não quebraria layout de grade — mas seria uma segunda variante da Iosevka no binário só para o chrome, o mesmo problema de coerência que este ADR resolve, com um risco novo no lugar do antigo.

### Sintetizar um peso 600 a partir do Medium (negrito de software)

`cosmic-text`/`fontdb` não expõem negrito sintético controlável neste projeto (RF-5.4 já limita síntese a itálico/negrito fora dos pesos embutidos, no runtime do glyphon, não como escolha nossa). Descartada por não ser um mecanismo disponível para acionar deliberadamente; o comportamento de fallback do `fontdb` (peso mais próximo) já cobre o caso sem código novo.

## Consequências

### Positivas

- Identidade visual única entre terminal e chrome — o motivo direto do pedido.
- Menos peso no binário: de cinco faces Iosevka para três (Aile Regular/Medium/SemiBold saem).
- Um `SANS_FAMILY`/`MONO_FAMILY` a menos para manter sincronizados em `text_measurer.rs`.

### Negativas

- Texto de prosa no chrome (nome de grupo, item de menu) roda em face monoespaçada — mais larga por caractere que uma proporcional equivalente, o que já era coberto por `pill_name_max_width`/`label_max_width` (teto fixo, não dependente do texto).
- Peso 600 fica sem arquivo próprio; um widget futuro que precise dele de verdade exige rodar `subset-fonts.py` de novo com o TTF original da Iosevka Fixed SemiBold.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Um widget novo pedir `SansWeight::SemiBold` esperando um 600 de verdade e receber 500 silenciosamente | Baixa | Baixo | Documentado em `attrs_for` (`text_measurer.rs`) e neste ADR; o fallback do `fontdb` não gera erro, então só um teste de peso pegaria a divergência |
| Alguém reintroduzir uma família separada para o chrome achando que resolve legibilidade de prosa | Baixa | Médio | Este ADR registra o motivo da unificação; reverter exige um ADR novo, não edição |
