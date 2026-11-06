# ADR-0087 — Zona de abas fixadas: caixas de aba solta à direita do botão do app, fora do recorte da trilha

**Status:** Aceito
**Data:** 2026-11-06
**Relacionados:** [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0019](0019-tooltip.md), [ADR-0021](0021-selecao-multipla-e-gestos-da-barra.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0079](0079-botao-de-menu-do-app.md), [ADR-0081](0081-disparo-no-release-para-botoes-e-itens.md), [ADR-0083](0083-gap-entre-os-botoes-da-zona-fixa.md), [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md), [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md), [PRD-021](../prd/prd-021-abas-fixadas.md)
**Supersedes (parcial):** ADR-0079 decisões 2 e 6 (`track_shift` passa a somar a zona de fixadas) · ADR-0021 §4 (o alvo do arraste ganha a zona) · ADR-0019 (o gatilho do tooltip ganha a aba fixada, que não tem rótulo)

## Contexto

O [PRD-021](../prd/prd-021-abas-fixadas.md) põe as abas fixadas numa zona própria, **logo à direita do botão do app**, quadradas, sem rótulo, com ícone (RF-21.1, RF-21.2). O [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md) já decidiu o modelo: a fixada é uma `TabId` em `Workspace::pinned`, fora de todo grupo. Falta a barra.

A [interface do v1 está fechada](0032-interface-do-v1-fechada.md): mudar a anatomia da barra exige ADR, e este é o ADR. E o projeto não inventa valor visual ([CLAUDE.md](../../CLAUDE.md), "Aparência"): tamanho, espaço, cor, raio e sombra têm de sair dos tokens que já existem.

O terreno é quase todo do [ADR-0079](0079-botao-de-menu-do-app.md). O botão do app já é uma **caixa de aba solta** (40×40, `tab_height + 2 · wrapper_padding`) numa zona fixa à esquerda, fora do recorte da trilha, com toda conversão tela↔conteúdo passando por `tab_bar::track_shift`. Uma fixada é, visualmente, essa mesma caixa — com o estado de uma aba em vez do de um botão. Seis coisas não estão decididas por isso:

1. **Geometria da zona** e como ela empurra a trilha, com e sem o botão do app.
2. **Estados da caixa**: ativa, inativa, hover, pressionada, não iniciada, encerrada — o que a [especificação](../design/especificacao-visual.md) §2.5 define para a aba com rótulo, e que precisa de tradução para uma caixa só com ícone.
3. **Identificação sem rótulo** (RF-21.6): tooltip, ponto de atividade e acessibilidade.
4. **Hit-testing**: a ordem dos testes na barra, e o que é drag region.
5. **Arraste** para dentro, para fora e dentro da zona (RF-21.7, RF-21.10, RF-21.11).
6. **Menu de contexto** e os gestos de aba que, numa fixada, não têm alvo (RF-21.12).

## Decisão

**Cada fixada é uma caixa de aba solta, quadrada, do lado do botão do app, numa zona fixa que cresce a trilha para a direita; os estados são os da aba, com o ícone no lugar do rótulo; a zona entra no `track_shift` e no hit-testing antes da trilha, e o arraste a reconhece como um destino a mais.**

### 1. Geometria: a zona esquerda vira uma fileira de caixas

A zona fixa da esquerda passa a ser uma **fileira de `k` caixas** de lado `S = app_menu_button_size(style)` (40 no default), onde `k` é o botão do app (1, ou 0 com `show_app_menu_button = false`) mais o número de fixadas **daquela janela**:

```
left_inset │ tp │ [app] │ gap │ [fix 1] │ gap │ [fix 2] │ tp │ trilha…
```

- **Margens:** `trilha_padding` (6) antes da primeira caixa e depois da última, e de topo — as do botão do app hoje.
- **Entre caixas:** `trilha_gap` (6), o que separa os wrappers da trilha. Com os dois em 6 no default, a fileira tem um ritmo só: botão, fixadas e o primeiro grupo da trilha ficam todos a 6 px um do outro.
- **Nenhum valor novo.** Lado, margens e vão são tokens que já existem; `porecatu.example.toml` não ganha chave e `VALORES` de `scripts/verify-docs.py` não ganha linha.

A conta, que **substitui** a de `track_shift` do ADR-0079 (revisão da decisão 2):

```rust
/// k = botão do app visível (0/1) + fixadas da janela.
pub fn track_shift(style: &TabBarStyle, is_macos: bool, pinned: usize) -> f32 {
    let k = usize::from(style.show_app_menu_button) + pinned;
    if k == 0 {
        return left_inset(style, is_macos);
    }
    left_inset(style, is_macos)
        + 2.0 * style.trilha_padding
        + k as f32 * app_menu_button_size(style)
        + (k - 1) as f32 * style.trilha_gap
}

/// Fixada de índice `i` (0 = a primeira da zona), em coordenadas de tela.
pub fn pinned_rect(style: &TabBarStyle, is_macos: bool, i: usize) -> Rect;
```

Com `k = 1` (só o botão) a conta dá os mesmos 52 px de hoje, e com `k = 0` o mesmo `left_inset` — os testes atuais de `track_shift` continuam valendo sem mudar. Cada fixada custa **46 px** de trilha. A regra da origem do conteúdo do ADR-0079 se generaliza: com `k > 0` o conteúdo da trilha nasce em `x = 0` e o respiro mora em `track_shift`; com `k = 0`, o conteúdo mantém o `trilha_padding` dele.

`track_shift` passa a receber a contagem, e é isso que mantém a regra do ADR-0079 de pé: **toda** conversão tela↔conteúdo já passa por ela, então a contagem chega a todos os pontos por um parâmetro, não por uma segunda fórmula. `trilha_width` e `point_in_track` passam a contagem adiante. Esquecer de passá-la desloca clique e hover em 46 px por fixada — o mesmo risco que o ADR-0079 registrou, com a mesma mitigação.

**A zona não rola e não tem teto** (RF-21.2). Com muitas fixadas a trilha encolhe, e `trilha_width` já satura em zero. Numa janela de 1280 px, por volta de vinte fixadas deixam a trilha mais estreita que uma aba; a trilha continua alcançável pelo teclado. O PRD registra que esse uso não é otimizado.

No macOS, a fileira vem depois do inset dos semáforos, como o botão do app.

### 2. Estados: os da aba solta, com o ícone no lugar do rótulo

A caixa é desenhada como a aba solta da especificação §2.5 — raio `tab_corner_radius`, borda de aba solta, sombra de aba solta quando `group_shadow_enabled` (`chrome::push_shadow`) —, e o ícone ([ADR-0088](0088-icone-da-aba-fixada.md)) vai centrado, com lado `icon_em_size` (20), por `Primitive::Image`, como o ícone do botão do app.

| Estado | Fundo e borda | Ícone |
|---|---|---|
| Inativa | `tab_inactive_*`, `inactive_border_width` | alfa 1 |
| Ativa | `tab_active_*`, `active_border_width` | alfa 1 |
| Hover | o do estado, com `brighten(tab_hover_brightness)` | — |
| Pressionada | o do estado **sem** o brilho: o press arma `Drag::TabPressed`, que suprime o hover, como em toda aba e no botão do app | — |
| Não iniciada (fixada SSH dormente, [ADR-0037](0037-aba-nao-iniciada.md)) | o do estado de base | alfa **`.45`** |
| Encerrada (`Exited`, RF-21.24) | `tab_inactive_*` **mesmo ativa** | alfa **`.45`** |

As duas linhas de baixo traduzem a §2.5 para uma caixa sem texto. **Não iniciada** é modificador e só esmaece o que seria o rótulo — aqui o ícone —, com o alfa `.45` que a §2.5 já usa. **Encerrada** mantém o fundo de inativa da §2.5 e troca o esmaecimento do rótulo (`#727a86`, uma cor de texto que não se aplica a uma imagem) pelo mesmo alfa `.45`. Sem cor nova, sem estado visual novo. Uma fixada dormente inativa e uma encerrada inativa ficam iguais na barra; a diferença está no grid (a nota de conexão, o código de saída) e no tooltip — é a mesma lógica da §2.5, de não pintar o que a nota já diz.

**Seleção não se aplica:** a fixada não entra em seleção múltipla (RF-21.3), e `Ctrl`/`Shift`+clique numa fixada é um clique simples.

### 3. Identificação: tooltip, ponto e acessibilidade

**Tooltip** (RF-21.6). O [ADR-0019](0019-tooltip.md) mostra o tooltip "só quando o texto do alvo foi truncado". A fixada é o caso-limite dessa regra: o rótulo dela está truncado **a zero**. Então ela **sempre** tem tooltip, com o título da aba pela precedência do RF-1.7, e todo o resto do ADR-0019 vale sem mudança — 600 ms de hover parado, ancorado abaixo da caixa e alinhado à borda esquerda dela, some no clique, no arraste e na perda de foco. A única revisão é a do gatilho.

**Ponto de atividade e campainha** (RF-1.20, RF-1.21). O ponto de 6×6 (`indicator_dot_size`) com as cores e a regra de "campainha vence" da §2.17 é desenhado **sobre a caixa**, com o canto superior direito a `padding_right` (6) das bordas de cima e da direita — a margem que a aba já dá ao botão de fechar. Não muda o tamanho da caixa, como na aba não muda a largura. Some ao visitar, é desligado pelas mesmas chaves, e não aparece em caixa encerrada.

**Acessibilidade.** Cada fixada é um nó `Role::Tab` no mesmo `TabList`, **antes** das abas da trilha (a ordem de `visual_order()`, [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md) §1) e depois do botão do app. O nome é o título, seguido dos estados de sempre e de uma peça nova, `access.state_pinned` ("fixada"), encaixada por `access.tab_state` como as outras.

### 4. Hit-testing e drag region

A ordem dos testes na barra passa a ser:

1. botões de janela;
2. botões das zonas fixas (`point_in_zone_button`: menu do app, sessões, configurações);
3. **fixadas** — `point_in_pinned(style, is_macos, pinned, point) -> Option<usize>`, em coordenadas de tela, **sem** `scroll_offset`;
4. a trilha, só dentro do recorte (`point_in_track`, [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md) §7);
5. drag region.

Uma fixada atingida devolve a mesma `TabId` que um acerto na trilha, e todo gesto de aba segue o caminho que já existe: o press arma `Drag::TabPressed`, o release ativa (RF-1.13), o botão do meio fecha (pelo diálogo do ADR-0086 §7), o direito abre o menu. A fixada não tem botão de fechar desenhado (RF-21.22), então não há alvo de fechar a testar.

**Drag region:** as margens da fileira e os vãos de `trilha_gap` entre as caixas arrastam a janela, como as margens do botão do app já arrastam ([especificação](../design/especificacao-visual.md) §2.2.1). A folga de "nenhum pixel sem dono" da §2.2 é da trilha, onde abas são vizinhas coladas; aqui as caixas são separadas como botões.

### 5. Arraste

O arraste continua sendo o da §2.19 — limiar de 4 px, fantasma preso ao Y da barra seguindo o X, "o buraco é o marcador", `Esc` cancela, soltar fora da barra cancela — com três extensões.

**O alvo ganha a zona.** `DragDrop` ganha uma variante, revendo o ADR-0021 §4:

```rust
pub enum DragDrop {
    IntoGroup { group: GroupId, pos: usize },
    NewRun { group_index: usize },
    Pinned { pos: usize },   // novo: soltar na zona fixa
}
```

Com o centro do fantasma **à esquerda de `track_shift`** — sobre a fileira, inclusive sobre o botão do app —, o alvo é `Pinned`, e `pos` sai da comparação contra o centro de cada caixa de fixada, como `resolve_within_group` faz com as abas (sobre o botão do app, `pos = 0`). À direita, o `drag_target` de sempre, em coordenadas de conteúdo. Ao soltar, `lib.rs` traduz o par (origem, alvo):

| Origem | Alvo | Operação ([ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md) §1) |
|---|---|---|
| trilha | `Pinned { pos }` | `pin_tab(id, pos)` — fixa |
| zona | `Pinned { pos }` | `move_pinned(id, pos)` — reordena |
| zona | `IntoGroup` / `NewRun` | `unpin_tab(id, destino)` — desafixa para ali |
| trilha | `IntoGroup` / `NewRun` | o de sempre |

**Feedback na zona.** Quando o alvo é `Pinned`, a fileira abre o vão onde a aba vai cair — a vizinha desliza com a transição `.15s` da §1.10, o buraco é o marcador — e a zona recebe o **realce do *run* implícito** da §2.19: `ungrouped_color` com `drag_highlight_tint_strength` (`.14`) e borda a `.45`, sobre o retângulo das caixas de fixada. Realce, ali, significa "a aba entra aqui", e é o que a zona é. Nada de cor nova.

**O fantasma mantém a forma de origem.** Arrastada da trilha, a aba é o fantasma de 209 px de sempre, mesmo sobre a zona; arrastada da zona, é a caixa quadrada, mesmo sobre a trilha. Trocar a forma no meio do gesto, quando o centro cruza a fronteira, faria o fantasma pular sob o cursor. O fantasma é desenhado **fora do recorte da trilha** (na camada de chrome do [ADR-0018](0018-composicao-de-frame.md), por cima das zonas), para não sumir ao cruzar a borda.

**Auto-scroll** só com o centro do fantasma **dentro** da trilha, a menos de 30 px de uma ponta. Levar uma aba da trilha à zona passa pela ponta esquerda e pode rolar a trilha alguns passos no caminho; é o comportamento da §2.19 sem exceção, aceito em vez de uma regra de direção.

Arraste de **grupo** (§2.19.1) não alcança a zona: um grupo não se fixa, e o alvo dele continua sendo uma fronteira entre grupos.

### 6. Menu de contexto e gestos sem alvo

O menu da aba ([especificação](../design/especificacao-visual.md) §2.16) ganha um item, com o rótulo pelo estado da aba clicada:

| Aba | Itens |
|---|---|
| da trilha | Nova aba · **Fixar aba** · Mover para grupo · Fechar aba (· Reconectar, se SSH) |
| fixada | Nova aba · **Desafixar aba** · Mover para grupo *(esmaecido)* · Fechar aba (· Reconectar, se SSH) |

"Mover para grupo" fica **esmaecido**, nunca ausente (RF-21.12, §2.16). "Fechar aba" mantém o rótulo: o diálogo do [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md) §7 é quem diz que fechar desafixa — um rótulo "Fechar e desafixar" duplicaria a frase em dois lugares do mesmo gesto. Os itens disparam no release ([ADR-0081](0081-disparo-no-release-para-botoes-e-itens.md)). Identificadores novos: `tab_menu.pin` e `tab_menu.unpin`, em todos os arquivos de `locales/`.

Três gestos de aba não têm alvo numa fixada:

- **Renomear** (RF-1.8): duplo clique na caixa só ativa; `tab.rename` com a fixada ativa não faz nada. Não há rótulo onde editar.
- **Nova aba** com a fixada ativa (`tab.new`, item "Nova aba"): a fixada não tem grupo onde pôr a aba, então ela nasce num ***run* implícito no fim da trilha**, herdando o `cwd` do painel focado da fixada (RF-1.1), e a trilha rola até ela (RF-1.18).
- **Trazer à vista** (RF-1.18): a fixada ativa já está sempre à vista; a trilha não rola.

### 7. A especificação visual

O binário é normativo ([ADR-0028](0028-o-binario-como-referencia-visual.md)), e a [especificação](../design/especificacao-visual.md) é atualizada **no mesmo PR que implementa** a zona, com: a zona 0.5 na §2.2 (entre o botão do app e a trilha) e a nota de drag region da §2.2.1; uma subseção **"Aba fixada"** com a tabela do §2 deste ADR; o ponto sobreposto na §2.17; a variante de alvo, o realce e o fantasma na §2.19; o gatilho do tooltip na §2.20; o item novo na §2.16; a linha na tabela de fases (§3) e o registro na §4.4. Até o aval visual do dono, o registro diz "aval pendente".

## Alternativas consideradas

### Fixadas no começo da trilha, rolando com ela

O modelo dos navegadores. Evitaria mexer em `track_shift` e no hit-testing. Recusada pelo PRD (RF-21.2): a zona não rola, porque o problema que motiva o recurso é justamente o app que some da vista quando a trilha enche.

### Caixa menor que a aba solta (os 30 px do `right_zone_button_size`)

Caberiam mais fixadas. Recusada porque o pedido é uma **aba** quadrada, e o [ADR-0082](0082-botoes-da-barra-com-o-tamanho-do-botao-de-menu.md) acabou de levar os botões da barra aos 40 px da aba solta para que a barra tenha uma altura de caixa só.

### `window_controls_gap` (3 px) entre as caixas, como os botões da zona direita

O valor que o [ADR-0083](0083-gap-entre-os-botoes-da-zona-fixa.md) escolheu para botões. Recusada porque os botões da direita não têm borda nem sombra em repouso, e o vão de 3 px ali separa caixas de hover. As fixadas têm borda e sombra de aba sempre visíveis, e 3 px entre duas sombras em camadas as fundiriam. O `trilha_gap` é o vão entre caixas com sombra que a barra já usa.

### Ponto de atividade como anel em volta do ícone, ou como borda colorida da caixa

Mais visível que um ponto de 6 px. Recusada porque é um indicador novo, com geometria nova, para dizer o que o ponto da §2.17 já diz em toda outra aba — e a borda colorida colidiria com a da seleção e a da aba ativa.

### Tooltip só com o título truncado, como hoje

Coerente com o ADR-0019 ao pé da letra. Recusada porque deixa a fixada sem nome nenhum na tela: o rótulo não existe, e a regra do ADR-0019 existe para mostrar o texto que não coube — o de uma fixada não coube inteiro.

### Fantasma que muda de forma ao cruzar a fronteira

Mostraria, durante o gesto, como a aba vai ficar. Recusada porque o salto de 209 px para 40 px sob o cursor faz o alvo pular — o centro do fantasma muda junto, e o ponto de queda oscila na fronteira.

### Arrastar uma aba para a zona fixa sem realce

Menos desenho. Recusada pela mesma razão que fez o *run* implícito ganhar realce na §2.19: sem ele, o destino mais importante do gesto não teria feedback nenhum além do vão.

## Consequências

### Positivas

- **Nenhum valor visual novo.** Caixa, margens, vão, cores, raio, sombra, alfa de esmaecimento, ponto e realce saem de tokens que já existem.
- A fixada reaproveita o caminho inteiro de gestos de aba — press, release, botão do meio, menu, arraste — porque o hit-test devolve uma `TabId` como qualquer aba.
- `track_shift` continua sendo o ponto único de conversão tela↔conteúdo; a contagem de fixadas entra por parâmetro, sem segunda fórmula.
- Com zero fixadas a barra é pixel por pixel a de hoje (RF-21, métrica "zero pixels novos").

### Negativas

- **`track_shift` muda de assinatura**, e todo chamador passa a contagem. É o tipo de mudança que o ADR-0079 descreveu como "esquecer um ponto desloca clique ou hover".
- **Cada fixada tira 46 px da trilha**, e a zona não tem teto.
- **Uma fixada ativa com o ícone do app fica igual ao botão do app**: os dois usam a caixa de aba solta ativa e o mesmo PNG. Só a posição os separa (o botão é sempre o primeiro). Ver os riscos.
- Fixada dormente e fixada encerrada, ambas inativas, são iguais na barra.
- O fantasma passa a ser desenhado fora do recorte da trilha.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Algum ponto tela↔conteúdo sem a contagem de fixadas desloca clique/hover | Média | Médio | Parâmetro obrigatório em `track_shift` (o compilador acha os chamadores); testes de geometria com 0, 1 e 3 fixadas, com e sem botão do app; busca por `scroll_offset` |
| Fixada ativa sem ícone próprio confundida com o botão do app | Média | Baixo | Posição fixa do botão; tooltip na fixada (o botão não tem); **decisão pendente do aval visual do dono** — se não bastar, a revisão é no botão do app (ADR-0079), não um valor novo na fixada |
| Clique numa fixada cair na trilha rolada por baixo dela | Baixa | Médio | Fixadas testadas antes de `point_in_track`, e a trilha só atende dentro do recorte (ADR-0084 §7) |
| Auto-scroll rolar a trilha no caminho até a zona | Média | Baixo | Aceito; o alvo é decidido no release, e o vão na zona mostra onde a aba cai |
| Muitas fixadas zerarem a trilha | Baixa | Médio | Trilha alcançável por teclado; registrado no PRD como uso não otimizado |
| Ponto sobreposto encostar no ícone | Média | Baixo | Ícone de 20 px centrado numa caixa de 40 deixa o canto livre; aval visual decide |
