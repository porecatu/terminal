# ADR-0018 — Composição de frame: camadas, recorte e medição de texto

**Status:** Aceito
**Data:** 2026-08-27
**Relacionados:** ADR-0001, ADR-0007, ADR-0009, ADR-0014, ADR-0015, ADR-0016, ADR-0019, PRD-001, PRD-004

> **Nota de escopo.** Este ADR **refina a seção 5 da [arquitetura](../arquitetura.md)** e a decisão de stack do [ADR-0001](0001-stack-de-gui.md) sem mudar nenhuma das duas: `winit` + `wgpu` + `glyphon` continuam, `porecatu-render` continua não conhecendo domínio, e as primitivas continuam sendo quad, retângulo arredondado, run de texto e clip. O que muda é a **forma da entrada** do renderer e o que ele exporta. Por isso não há `Supersedes`.

## Contexto

A F1 entregou um renderer que desenha exatamente uma coisa: a grade do terminal. A F2 pede quatro coisas novas — barra de abas com trilha rolável, menu de contexto, diálogo modal e aviso empilhado — e a API atual não consegue desenhar nenhuma delas corretamente. Não é questão de esforço: são quatro limitações estruturais, e três só ficam visíveis olhando o código.

### 1. Não existe "por cima"

`Renderer::render` recebe `&[Primitive]` e **achata a lista em três baldes** — todos os `Quad`, depois todos os `RoundedQuad`, depois todo o texto — desenhando nessa ordem fixa e ignorando a ordem em que as primitivas chegaram. A justificativa está escrita no próprio código e era correta para a F1: *"para a grade do terminal (única consumidora nesta fase) isso já é correto — texto de uma célula nunca precisa ficar atrás do fundo de outra — e evita alternar pipeline em UM render pass, que o `glyphon` não foi desenhado para fazer"*.

Com um popover, deixa de ser correto: o fundo do menu é um `RoundedQuad` e o texto do terminal é texto, então **o menu fica atrás do texto**. Os três widgets do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) são inviáveis com a API atual, e o RF-1.6 e o RF-2.23 são cenários de aceite de F2 e F3.

### 2. `PushClip`/`PopClip` não recortam, e não podem

As duas variantes existem em `Primitive` e são explicitamente no-op — limitação conhecida, registrada na arquitetura e no roadmap como *"sem consumidor até o overflow da barra de abas, na F2"*. O consumidor chegou (RF-1.18).

Mas o problema não é só implementar: **clip por índice na lista não sobrevive ao achatamento** em baldes. Um `PushClip` que valia para as próximas cinco primitivas perde o sentido quando as cinco são redistribuídas em três passes. Resolver o recorte exige resolver a ordenação primeiro.

### 3. Não há como medir texto proporcional

A seção 7 da arquitetura promete que o layout da barra é uma função pura, e diz por quê: *"O layout da barra de abas é deliberadamente uma função pura `(Workspace, Config, largura) -> Vec<TabRect>`. Isso permite testar overflow, colapso de grupo e truncamento de título sem abrir uma janela."*

Não há como cumprir isso. O único medidor público é `measure_mono_cell`, que shapa a string `"M"` na face mono e serve à grade. O `FontSystem` é privado dentro de `TextPipeline`, que só se constrói com `&wgpu::Device` e `&wgpu::Queue`. Truncar título em 180 px, calcular a largura de um rótulo em IBM Plex Sans, dimensionar item de menu — nada disso tem API, e o que existe não é alcançável sem GPU.

Pior, o caminho de desenho estima a largura do buffer como `text.len() * size_px * 2.0 + 1.0`: contagem de **bytes**, não de caracteres. Com acentuação, que é o ambiente-alvo, a conta erra.

### 4. Um `Device` por janela

O [ADR-0015](0015-multiplas-janelas.md) decidiu: *"Cada janela tem sua surface `wgpu` e seu swapchain; o atlas de glyphs é compartilhado, porque as métricas de fonte são as mesmas e duplicar atlas por janela desperdiça VRAM sem motivo."*

`Renderer::new` cria `Instance`, `Adapter` e `Device` a cada chamada, e possui surface, pipelines e atlas juntos. Duas janelas seriam dois devices e dois atlas — o oposto do decidido. O mesmo ADR também deixou uma condicional aberta na tabela de riscos: *"Atlas de glyphs compartilhado entre janelas com DPI distinto — se virar problema, atlas por escala, não por janela"*. Duas janelas em monitores de DPI diferente é critério de saída da F2, então a condicional precisa virar decisão antes, não depois.

## Decisão

**O frame passa a ser uma sequência ordenada de camadas; recorte e medição de texto ganham API própria; a GPU é do processo e a surface é da janela.**

### Camadas

`porecatu-render` recebe por frame uma sequência de camadas, cada uma com sua própria lista de primitivas. Dentro de uma camada vale a ordem de hoje — quads, arredondados, texto —, que é correta porque nenhum elemento `[v1]` precisa de texto sob um quad da mesma camada. **Entre** camadas vale a ordem da sequência: a camada N inteira desenha sobre a camada N−1 inteira.

As camadas são **nomeadas e em número fixo**, não profundidade arbitrária:

| Ordem | Camada | Conteúdo |
|---|---|---|
| 1 | grade | fundo de célula, glyphs, cursor, seleção |
| 2 | chrome | barra de abas, trilha, pílulas, botões |
| 3 | aviso | pilha de avisos do app (seção 2.14 da especificação) |
| 4 | popover | menu de contexto e tooltip ([ADR-0019](0019-tooltip.md)) |
| 5 | modal | overlay e diálogo de confirmação |

A ordem resolve o z-order que a seção 2.14 da especificação visual deixava em aberto: modal cobre popover, que cobre aviso, que cobre chrome. Camada vazia não custa nada — nenhum passe é emitido.

No `glyphon`, isso significa um `TextRenderer` por camada, mantidos num pool e reusados entre frames. O `TextAtlas`, o `Cache` e o `Viewport` continuam **únicos**: é o atlas compartilhado do ADR-0015, e é o que impede que cinco camadas custem cinco atlas.

### Recorte

`PushClip`/`PopClip` passam a recortar, **dentro da camada**. A pilha de clip é de retângulos que se intersectam: aninhar recorta pela interseção, e `PopClip` sem `PushClip` correspondente é erro de programação, não comportamento definido.

- Quads: `set_scissor_rect` no passe, quebrando o batch quando o clip muda.
- Texto: `TextBounds` por `TextArea` — o `glyphon` já suporta, e hoje recebe `TextBounds::default()` em toda área.

É o suficiente para o RF-1.18: a trilha da barra é uma camada com um clip só, e as abas que saem dela desaparecem sem lógica de visibilidade no layout.

### Medição de texto sem GPU

`porecatu-render` exporta um **`TextMeasurer`**, dono do `FontSystem` e do `fontdb` com as cinco faces embutidas do [ADR-0016](0016-fontes-embutidas.md), construível **sem `Device` nem `Queue`**. Ele responde:

- largura de avanço de uma string numa face e num tamanho;
- a largura/altura de célula da grade, absorvendo o `measure_mono_cell` de hoje;
- truncamento a uma largura máxima, devolvendo o texto com reticências e se houve corte — que é o que o RF-1.10 pede e o que decide se a aba mostra tooltip.

O `FontSystem` passa a ser **um por processo**, de propriedade do `TextMeasurer`; o pipeline de texto o recebe emprestado no `prepare`. Duas consequências que valem a decisão:

- O layout da barra de abas vira de fato a função pura da seção 7 da arquitetura: `porecatu-ui` guarda um `TextMeasurer`, e um teste constrói outro sem abrir janela nem tocar em `wgpu`.
- A estimativa de largura por contagem de bytes sai do caminho de desenho: o tamanho do buffer vem da medição real.

O `TextMeasurer` continua sem conhecer domínio — mede string, face e tamanho, e não sabe o que é uma aba.

### GPU do processo, surface da janela

`Renderer` se divide em dois:

- **`GpuContext`** — `Instance`, `Adapter`, `Device`, `Queue`, `TextAtlas`, `Cache`, pipelines. Um por processo.
- **`WindowSurface`** — surface, `SurfaceConfiguration`, `Viewport`, os `TextRenderer` das camadas e a escala. Um por janela.

O adapter é escolhido compatível com a surface da primeira janela e reusado nas demais; surface criada depois é validada contra ele. Falha de compatibilidade — cenário de máquina com duas GPUs — é aviso do app pela superfície do ADR-0014, não `panic`, na mesma linha do fallback de GPU que o [ADR-0001](0001-stack-de-gui.md) já previu.

### Escala de DPI: um atlas, chave com a escala

O texto é shapado em **pixels físicos**: o tamanho lógico da config é multiplicado pela escala da janela antes de ir ao `FontSystem`, e as posições vão ao `Viewport` em pixels físicos. Isso resolve as duas coisas de uma vez — o glyph rasterizado é nítido em qualquer DPI, e a escala entra naturalmente na chave do cache do atlas, porque um `M` de 12,5 px lógicos a 1.0 e a 1.5 são entradas de tamanho diferente.

Fica decidido, fechando a condicional do ADR-0015: **um atlas só, sem partição por janela nem por escala.** O `Viewport` é por janela, porque a resolução é.

### Raio escalar continua bastando

A seção 5 da arquitetura fala em `radii` (plural) e o código tem `radius` escalar. Fica o escalar: todo elemento `[v1]` — aba, pílula, wrapper, botão, aviso, diálogo, menu, tooltip — usa raio uniforme. Raio por canto entra se e quando a barra de título customizada `[v2]` pedir, e é aditivo.

## Alternativas consideradas

### Manter um passe só e ordenar por profundidade

Dar um `z` a cada primitiva e ordenar. Descartada porque não resolve o texto: o `glyphon` prepara e desenha todo o texto de uma vez, com um `TextRenderer` por chamada — não há como intercalar glyphs entre quads dentro do mesmo passe. Um `z` daria falsa impressão de ordenação e o texto continuaria por cima de tudo.

### Um `TextAtlas` por camada

Simplificaria o pool. Descartada por desperdiçar VRAM exatamente como o ADR-0015 rejeitou fazer por janela: as faces e os tamanhos são os mesmos entre camadas, e o rótulo de aba e o item de menu usam a mesma face no mesmo tamanho.

### Profundidade arbitrária de camadas

Uma API genérica de `Layer` empilhável, como um compositor de verdade. Descartada porque o v1 tem cinco superfícies conhecidas e enumeradas nos ADRs 0014 e 0019 — profundidade arbitrária é generalidade sem requisito, e cada camada custa um `TextRenderer` e um passe. Camada nova exige requisito novo, como ação nova exige entrada no catálogo.

### Clip por índice na lista achatada

Preservar a lista única e fazer o clip valer por faixa de índices. Descartada por ser justamente o que o achatamento em baldes quebra: para funcionar, o renderer teria de deixar de reordenar — o que é a decisão de camadas, por um caminho mais confuso.

### `TextMeasurer` em `porecatu-ui`, com `fontdb` próprio

Manteria `porecatu-render` intacto. Descartada porque duplicaria o carregamento das cinco faces embutidas (megabytes no binário e na memória) e criaria duas fontes de verdade para métrica de fonte — o cenário exato que o [ADR-0016](0016-fontes-embutidas.md) quer evitar, já que largura de célula e largura de aba passariam a ser calculadas por instâncias diferentes.

### Crate novo só para shaping, abaixo de `render` e de `ui`

Resolveria a medição sem inflar `render`. Descartada por não pagar: o `FontSystem` é o mesmo objeto que o `glyphon` precisa no `prepare`, e separá-lo em outro crate significaria `render` dependendo desse crate e reexportando quase tudo. Um crate a mais no grafo em troca de nenhuma fronteira nova.

### Escalar a área de texto pelo campo `scale` do `glyphon`

Shapar em pixels lógicos e deixar o `glyphon` escalar. Descartada porque escala o glyph **já rasterizado**: em monitor 150 % o texto sai interpolado. Um emulador de terminal cuja fonte fica borrada em tela HiDPI falha no requisito mais básico de legibilidade.

## Consequências

### Positivas

- Os três widgets do ADR-0014 e o tooltip do ADR-0019 passam a ser desenháveis; sem isso a F2 não fecha.
- O overflow do RF-1.18 sai de graça de um clip só, sem lógica de visibilidade no layout.
- A promessa da seção 7 da arquitetura — barra de abas testável sem janela — passa a ser cumprível, e o teste de layout não precisa de GPU no CI.
- O atlas compartilhado do ADR-0015 deixa de ser intenção e passa a ser estrutura, e a condicional de atlas por escala fecha antes de a F2 abrir a segunda janela.
- Texto nítido em qualquer DPI, com uma única regra: shapar em pixels físicos.
- A estimativa de largura por contagem de bytes desaparece — bug latente com acentuação, que é o teclado-alvo.

### Negativas

- `porecatu-render` cresce: cinco `TextRenderer`, pilha de clip, quebra de batch por scissor. Saiu da F1 com menos de 700 linhas.
- Quebra de batch por mudança de clip custa draw calls. Irrelevante no volume do chrome, mas é custo real.
- `GpuContext`/`WindowSurface` é refatoração da API que a F1 acabou de estabilizar, feita antes de existir a segunda janela que a justifica.
- `TextMeasurer` emprestado ao pipeline introduz um empréstimo mutável que atravessa `ui` → `render` no caminho quente; ordem de chamada passa a importar.
- Camadas fixas significam que superfície nova (busca da F6, paleta de comandos `[v2]`) precisa escolher uma camada existente ou justificar uma nova.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Adapter da primeira janela não servir a uma janela em outra GPU | Baixa | Médio | Validar a surface contra o adapter; aviso do app pela superfície do ADR-0014, nunca `panic` |
| Pool de `TextRenderer` crescer sem controle | Baixa | Baixo | Número de camadas é fixo e enumerado nesta decisão |
| Scissor em pixels lógicos vs. físicos trocados | Média | Alto | Um só ponto de conversão, na fronteira `WindowSurface`; teste com escala 1.0 e 1.5 |
| Empréstimo do `TextMeasurer` virar `RefCell` por conveniência | Média | Baixo | Assinatura explícita `&mut` no `prepare`; revisão dirigida |
| Texto borrado em HiDPI por alguém usar `TextArea::scale` | Média | Alto | Decisão registrada aqui; a conversão para físico acontece antes do `FontSystem`, não depois |
| Refatoração de `Renderer` regredir a grade da F1 | Média | Alto | A grade passa a ser a camada 1 sem mudança de conteúdo; comparação visual antes e depois no mesmo shell |
| Camada nova entrar sem requisito | Média | Baixo | Tabela de camadas neste ADR é fechada, como o catálogo de ações |
