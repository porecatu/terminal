# ADR-0094 — Polilinha e área no render: segmentos instanciados com SDF de segmento, área por faixas verticais

**Status:** Aceito
**Data:** 2026-11-10
**Relacionados:** [ADR-0001](0001-stack-de-gui.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0061](0061-imagem-de-fundo-do-terminal.md), [ADR-0068](0068-efeito-acrilico-no-fundo-do-terminal.md), [ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md), [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md)

## Contexto

O [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md) RF-24.7 pede **gráficos de linha**, e o dono decidiu que a área sob a linha é preenchida com a mesma cor, translúcida ([ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md)).

`porecatu-render` não desenha nada que não seja um retângulo alinhado aos eixos. As primitivas são `Quad`, `RoundedQuad`, `Backdrop`, `Image`, `BlurredBackdrop`, `Text` e o par `PushClip`/`PopClip` (`primitives.rs`). Toda geometria é um **quad unitário instanciado** (`TriangleStrip` de 4 vértices, uma instância de 64 B por quad), com cobertura por SDF no fragment shader (`sdf.wgsl` + `quad.wgsl`), saída premultiplicada e pipeline `PREMULTIPLIED_ALPHA_BLENDING`. Não há MSAA, malha de triângulos nem traço. O sublinhado, o cursor e o box-drawing são `Quad`s retos. `frame.rs::resolve_layer` agrupa a geometria contígua de mesmo recorte em lotes, preservando a ordem do fluxo, e o recorte é um scissor.

Um gráfico de linha com 60 a 600 pontos não sai de `Quad`s: escadinha de retângulos é serrilhado, que é exatamente o que o SDF existe para evitar. É preciso uma primitiva nova. A regra do CLAUDE.md é que o render **não conhece o domínio**: a primitiva tem de ser geometria genérica, e não "gráfico".

## Decisão

**Duas primitivas genéricas novas, `Polyline` e `Area`, desenhadas por um pipeline novo de instâncias (`segment-pipeline`, `segment.wgsl` sobre o mesmo `sdf.wgsl`), uma instância por segmento. A polilinha usa a distância ao segmento (cápsula) e a área usa faixas verticais com antisserrilhado só na borda de cima. As duas entram no fluxo de geometria como qualquer outra, respeitando a ordem e o recorte.**

### 1. As primitivas

Em `primitives.rs`:

```rust
/// Linha poligonal aberta: segmentos entre pontos consecutivos, com
/// espessura e pontas e junções redondas. Pixels lógicos.
pub struct Polyline {
    pub points: Vec<Point>,
    pub width: f32,
    pub color: Color,
}

/// Região entre uma linha poligonal monotônica em x e uma reta horizontal
/// (`baseline_y`): para cada x entre o primeiro e o último ponto, pinta de
/// `y(x)` até a base. Pixels lógicos.
pub struct Area {
    pub points: Vec<Point>,
    pub baseline_y: f32,
    pub color: Color,
}

pub enum Primitive {
    …
    Polyline(Polyline),
    Area(Area),
}
```

`Point { x, y }` é o par lógico que o `Rect` já usa nos cantos. Se não existir como tipo, nasce em `primitives.rs`. Os contratos são estes:

- **`Polyline` com menos de 2 pontos** não desenha nada. Uma lacuna no dado não é assunto do render: quem chama emite **uma `Polyline` por trecho contínuo** (o gerenciador, [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §6).
- **`Area` exige `x` crescente** nos pontos. Um par fora de ordem é descartado com `debug_assert!`, e em release o segmento é pulado. A área de um gráfico de tempo é sempre monotônica, e o contrato estreito é o que permite o desenho do §3.
- **Cor premultiplicada na saída**, como toda geometria.

### 2. A polilinha: uma instância por segmento, SDF de segmento

Instância (`#[repr(C)]`, pixels **físicos**, 48 B):

```rust
struct SegmentInstance {
    p0: [f32; 2],
    p1: [f32; 2],
    half_width: f32,   // linha: meia espessura; área: baseline_y (§3)
    kind: u32,         // 0 = linha, 1 = área
    _pad: [f32; 2],
    color: [f32; 4],
}
```

Para uma linha, o vertex shader monta um **retângulo orientado** em volta do segmento: o eixo vai de `p0` a `p1` e é estendido por `half_width + 1 px físico` nas duas pontas e nos dois lados. O fragment shader calcula a distância do pixel ao segmento (a SDF de cápsula, que entra em `sdf.wgsl` ao lado de `sdf_rounded_box`) e a cobertura, com a mesma rampa de antisserrilhado das bordas arredondadas (`edge_coverage` com `fwidth`).

As junções e as pontas saem **redondas de graça**: duas cápsulas consecutivas se sobrepõem num disco no ponto comum. A consequência é que, **com cor translúcida**, a junção recebe cobertura duas vezes e fica mais escura. O contrato da `Polyline` é **cor opaca**. A linha do gráfico é opaca ([ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md)), e um `debug_assert!(color.a == 1.0)` fixa isso. Juntar segmentos num traço só (malha com junção calculada) resolveria o caso translúcido, mas não há consumidor para ele.

A espessura é em pixels lógicos e vira física pelo fator de escala, como o resto. Ao contrário do `Quad`, os pontos **não** são arredondados ao pixel físico: uma linha inclinada não tem pixel a encaixar, e o antisserrilhado cuida da borda.

### 3. A área: faixas verticais, antisserrilhado só em cima

Para cada par de pontos consecutivos `(x0, y0)`, `(x1, y1)`, uma instância `kind = 1` desenha o retângulo **alinhado aos eixos** `[x0, x1] × [min(y0, y1) − 1 px, baseline_y]`. O fragment shader interpola `y_linha(x)` no centro do pixel e cobre `clamp(y − y_linha(x) + 0.5, 0, 1)`: antisserrilhado vertical na borda de cima, cheio abaixo dela.

Os lados verticais **não** têm antisserrilhado. Duas faixas vizinhas compartilham o mesmo `x` exato (o `x1` de uma é o `x0` da outra, vindo do mesmo `f32`), e a regra de rasterização da GPU (*top-left rule*) entrega cada pixel a exatamente uma das duas, sem buraco nem sobreposição. É o que evita a **costura** que o CLAUDE.md registra para bordas de célula ("empate de 1 ULP gera costura"): aqui não há empate a arredondar, porque o ponto é o mesmo número.

Como a área é translúcida (a opacidade de preenchimento do [ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md)) e cada pixel pertence a uma faixa só, a cor fica uniforme. Uma área por triangulação em leque teria o mesmo problema de sobreposição da junção translúcida do §2.

### 4. Lote, ordem e recorte

- `frame.rs::resolve_layer` trata `Polyline` e `Area` como geometria, no mesmo lote de mesmo recorte das outras primitivas, sem quebrar a ordem do fluxo.
- `quad.rs::prepare_layer` ganha um `Draw::Segments(range)`, ao lado de `Draw::Quads`, `Draw::Image` e `Draw::Blurred`. Uma corrida contígua de `Polyline`/`Area` vira **um** `Draw::Segments`, com as instâncias num buffer próprio (`SegmentLayerBuffer`). O buffer cresce como o de quads (recriado com o tamanho pedido quando não cabe, e senão reescrito por `write_buffer`), com capacidade inicial de 256 segmentos, que cobre três gráficos de 60 pontos sem crescer.
- `render_layer` troca o pipeline e o slot de vértice 1 como já faz entre quad e imagem. O recorte é o mesmo scissor por draw.
- O pipeline novo usa `PREMULTIPLIED_ALPHA_BLENDING`, nunca `ALPHA_BLENDING` (CLAUDE.md), sem MSAA, com o mesmo formato de surface.

### 5. O que não muda

- **O passe de desfoque do acrílico** ([ADR-0068](0068-efeito-acrilico-no-fundo-do-terminal.md)) continua sendo o único passe de filtro. A primitiva nova não é filtro, é geometria. `BlurState::build` renderiza as primitivas de uma `BlurSource` pelo mesmo `resolve_layer`/`prepare_layer`, então uma `Polyline` posta numa `BlurSource` seria desenhada lá também. Isso é aceito e não é usado: só o fundo da janela vai para a `BlurSource`, e o gerenciador desenha na camada da grade ([ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md)). O comentário de `BlurSource` passa a citar as primitivas novas entre as que "desenham lá".
- **O render continua sem conhecer o domínio.** Ele recebe pontos, espessura, base e cor. Escala, tempo, lacuna e grade são do `porecatu-ui`.
- **`MemoryHints::MemoryUsage`**, o formato `Rgba8Unorm` sem `Srgb` e o `ColorMode::Web` do texto não mudam.

### 6. Testes que o PR de implementação traz

- **CPU** (`frame.rs`, `quad.rs`): uma `Polyline` de `n` pontos vira `n − 1` instâncias; uma `Polyline` com menos de dois pontos não vira nada; uma `Area` com `x` fora de ordem pula o par (teste em release com `cfg(not(debug_assertions))`, ou função pura de geração separada do `debug_assert!`); a ordem entre `Quad`, `Polyline` e `Text` é preservada nos `draws`; o recorte se aplica.
- **GPU sem janela** (o harness de `blur.rs`/`image.rs`, que pula com `eprintln!` sem adaptador): uma linha horizontal de espessura 2 pinta as linhas de pixel esperadas, com borda antisserrilhada; uma área com 60 faixas sob uma linha reta tem **cor idêntica em todas as colunas** abaixo da borda (o teste da costura); uma linha diagonal não tem buraco entre segmentos.

## Alternativas consideradas

### Escada de `Quad`s

Um quad por coluna de pixel, com altura até o valor. Zero código no render. Recusada: serrilha na linha, e a linha é o que o dono pediu. Uma coluna por pixel também custaria centenas de instâncias por gráfico numa janela larga.

### Malha de triângulos com franja de antisserrilhado (CPU)

O traçado clássico: o `porecatu-ui` (ou o render) triangula a linha com junções em mitra ou redondas, mais uma franja de 1 px com alfa zero na borda externa. Resolveria a junção translúcida. Recusada: é o primeiro caminho de **malha** do render (vertex buffer não instanciado, pipeline de topologia diferente), e o antisserrilhado por franja depende da escala e de casos de junção aguda. A SDF por instância reaproveita o modelo que o render inteiro já usa.

### MSAA no pipeline novo

Daria antisserrilhado sem SDF. Recusada: o render não tem MSAA em nenhum lugar, e ligar só num pipeline exigiria um alvo multiamostrado e resolve à parte, por causa de um gráfico.

### Uma primitiva `LineChart` com escala e grade

Recusada pela fronteira do render: escala, grade e lacuna são domínio do gerenciador. O render recebe geometria.

### Área por triangulação em leque

Menos instâncias. Recusada no §3: sobreposição de triângulos translúcidos escurece as junções, e o antisserrilhado da borda de cima fica a cargo de cada triângulo.

## Consequências

### Positivas

- O render passa a desenhar linhas inclinadas e áreas, sem conhecer gráficos. O box-drawing diagonal, hoje entregue à fonte (`box_glyphs.rs`), poderia usá-las um dia, mas isso não é decidido aqui.
- **Um pipeline e um shader novos, no mesmo modelo** (instância e SDF) dos que existem. Nada de malha, MSAA ou passe novo.
- A área sai sem costura por construção (§3), e o teste de GPU vigia isso.

### Negativas

- **Polilinha translúcida escurece nas junções.** O contrato é cor opaca, vigiado por `debug_assert!`. Um consumidor futuro que precise de linha translúcida terá de revisitar o §2.
- Mais um buffer por camada e mais uma troca de pipeline por corrida de segmentos.
- `Area` só aceita `x` crescente. Uma área genérica (polígono qualquer) pediria outra primitiva.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Costura entre faixas da área em alguma GPU | Baixa | Médio | Faixas com `x` exatamente compartilhado e sem antisserrilhado lateral (§3); teste de GPU de cor uniforme por coluna |
| A extensão de 1 px físico do retângulo orientado cortar o antisserrilhado em escala alta | Baixa | Baixo | A extensão é `half_width + 1 px físico` em pixels físicos, depois da escala; teste com fator 2 |
| Polilinha translúcida usada por engano | Baixa | Baixo | `debug_assert!` de alfa 1 (§2) |
| Lotes partidos demais (troca de pipeline entre cada linha e cada quad da grade) | Média | Baixo | O gerenciador emite a grade inteira, depois as áreas, depois as linhas ([ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md) §3), o que dá uma troca por gráfico |
