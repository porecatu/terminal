# ADR-0096 — Anatomia do gerenciador de tarefas e do botão do app ligado

**Status:** Aceito — aval visual do dono pendente
**Data:** 2026-11-10
**Relacionados:** [ADR-0024](0024-face-de-icones.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0053](0053-paineis-divididos.md), [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md), [ADR-0067](0067-temas-em-arquivos-proprios.md), [ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md), [ADR-0079](0079-botao-de-menu-do-app.md), [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md), [ADR-0087](0087-zona-de-abas-fixadas-na-barra.md), [ADR-0094](0094-polilinha-e-area-no-render.md), [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md), [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md)
**Supersedes:** ADR-0079 revisão de 2026-11-06 (**parcial**: acrescenta um estado "ligado" ao botão, com sombra interna; repouso, hover e pressionado com o modo desligado não mudam)

## Contexto

O [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md) desenha o gerenciador em uma frase: a área de conteúdo dividida verticalmente ao meio, com três gráficos empilhados à esquerda (CPU, memória e disco) e a lista de processos à direita. Ele também exige que o botão do app mostre que o modo está ligado, e não só por matiz (RF-24.3). A [interface do v1 está fechada](0032-interface-do-v1-fechada.md): anatomia nova entra por ADR, e nenhuma cor, dimensão, raio ou espaçamento é inventado (CLAUDE.md).

O dono do produto decidiu, antes da escrita:

- **uma cor por gráfico**, tirada da paleta de grupos, como no Gerenciador de Tarefas do Windows: CPU azul, memória roxo, disco verde;
- **área sob a linha preenchida**, na cor da linha, translúcida;
- **moldura e grade** nos gráficos;
- o botão do app ligado **parece pressionado, com uma sombra interna** (*inset*).

Nenhum gráfico, grade ou sombra interna existe hoje no chrome. A tabela de tokens (especificação visual §1) não tem cor de série nem de grade. O que existe e serve de origem:

- a paleta de grupos (`[groups] palette`): azul `#6fa8f5`, roxo `#a68cf0` e verde `#86c56a`;
- a borda de controle `#262b34` (`[groups] border`, `[dialog] button_border`) e o separador de painéis `#1c2027`, da especificação;
- o texto em degraus (Padrão `#d7dce3`, Secundário `#a8b0bb`, Mínimo `#5c646f`), o hover de item `#242a33` e o fundo de aba ativa `#282e37`;
- as medidas de lista do popover de sessões (`row_height` 28, `row_padding_x` 8) e o `padding` 18 do painel da tela de configurações ([ADR-0060](0060-anatomia-da-tela-de-configuracoes.md));
- a sombra em camadas (`chrome::SHADOW_LAYERS`, três `RoundedQuad` empilhados por `chrome::push_shadow`) e a regra do fundo dos botões da zona fixa (`chrome::zone_button_background`, [ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md)).

## Decisão

**O gerenciador ocupa o quadro do terminal, com o fundo do terminal. Ele tem duas colunas iguais, com três gráficos de altura igual à esquerda e a lista à direita. A moldura é retangular sem raio (por causa do defeito aberto da borda arredondada), com grade de quartos e de 10 s, área a 0.15 e linha de 1.5 px nas cores da paleta de grupos. A lista tem linhas de 28 px, números em Iosevka e nomes na fonte do sistema. As cores novas vão para uma seção `[task_manager]` dos temas, e as dimensões novas para `[appearance.task_manager]`. O botão ligado tem o fundo pressionado fixo e uma sombra interna feita com as camadas de `SHADOW_LAYERS`, recortadas na faixa de cima.**

### 1. O quadro

- **Retângulo**: o mesmo `paint::terminal_box_rect` em que os painéis são desenhados. O gerenciador não muda o tamanho do quadro, a margem nem a barra de status ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §4).
- **Fundo**: o do quadro de um painel, pelo mesmo caminho de pintura (cor `[terminal] background` com `terminal.background_opacity`, imagem de fundo do terminal e acrílico quando ligados), mas sem grade de células. "Como se fosse um terminal" (PRD-024).
- **Camada**: `Layer::Grid`, a dos painéis. Menus, diálogo e tooltip da lista ficam nas camadas de sempre, por cima.
- **Recuo interno**: `padding` **18** de cada lado.
- **Colunas**: o interior é dividido em duas colunas de **largura igual**, separadas por `column_gap` **18**. Sem divisor desenhado: o espaço vazio separa, como o divisor dos painéis, que é ausência de pixel ([ADR-0053](0053-paineis-divididos.md)).

### 2. Os gráficos (coluna esquerda)

Há três blocos de **altura igual**, de cima para baixo: CPU, Memória e Disco, separados por `chart_gap` **8**. Cada bloco tem:

- **Cabeçalho**, com altura `row_height` **28**. À esquerda, o título ("CPU", "Memória", "Disco", do catálogo) na fonte do sistema, 12.5, peso Medium, em texto Padrão. À direita, o valor atual em Iosevka, 12.5, em texto Secundário. Os dois ficam centrados na vertical pelo render (CLAUDE.md: não compensar na geometria).
- **Moldura**: o resto da altura do bloco. É um retângulo com fundo transparente (o fundo do quadro aparece) e borda de 1 px na cor `chart_border`, **sem raio**. O defeito aberto do `quad.wgsl` não pinta a borda de retângulo arredondado (CLAUDE.md, "Render e texto"), e uma moldura de raio 6 sairia sem borda. Quando o defeito for corrigido, o raio passa a ser decidido com o aval do dono. Até lá, sem raio.
- **Topo da escala**: um rótulo dentro da moldura, no canto superior esquerdo, com `row_padding_x` **8** de recuo, em Iosevka 10 e texto Mínimo ("100%", "2 GB", "5 MB/s"). O degrau da escala é do [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §6.

### 3. Dentro da moldura: grade, área e linha

Tudo dentro de um `PushClip` com o interior da moldura (o retângulo menos a borda de 1 px), nesta ordem, que dá uma troca de pipeline por gráfico ([ADR-0094](0094-polilinha-e-area-no-render.md), "Riscos"):

1. **Grade horizontal**: três `Quad`s de 1 px na cor `grid`, a 25%, 50% e 75% da altura.
2. **Grade vertical**: `Quad`s de 1 px na cor `grid`, um a cada 10 s, contados a partir do instante da última amostra para a esquerda ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §6). A grade corre junto com a linha, como no Windows. Com `history_seconds = 60`, são cinco linhas.
3. **Área**: uma `Area` por trecho contínuo, da linha até a base da moldura, na cor do gráfico com o alfa multiplicado por `fill_opacity` **0.15**.
4. **Linha**: uma `Polyline` por trecho contínuo, de espessura `line_width` **1.5**, na cor do gráfico, **opaca** (o contrato do [ADR-0094](0094-polilinha-e-area-no-render.md) §2).

A posição de cada ponto no eixo x é a do [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §6, e no eixo y é `base − valor / topo × altura interna`. Um valor acima do topo (só na fração de segundo antes de a escala subir) é preso ao topo.

### 4. A lista (coluna direita)

- **Cabeçalho**: uma linha de `row_height` com os rótulos das colunas (Nome, PID, CPU, Memória, Disco, do catálogo) no estilo de rótulo de seção da especificação: 10, maiúsculas, texto Mínimo. Abaixo, um `Quad` de 1 px na cor `chart_border`, na largura da coluna. O cabeçalho não rola.
- **Colunas**: PID, CPU, Memória e Disco ficam **à direita**, alinhadas à direita. Cada uma tem a largura do seu maior texto possível ("8888888", "100%", "8888,8 MB", "8888,8 MB/s", com o separador do idioma), medido **uma vez** por tamanho de fonte (§5), mais `row_padding_x` entre colunas. O Nome ocupa o resto, à esquerda. Uma janela estreita demais encolhe o Nome até zero, e as colunas numéricas nunca são cortadas.
- **Linhas**: altura `row_height` **28**, conteúdo recuado `row_padding_x` **8** das bordas.
  - **Linha de aba**: um ponto de 6×6 na cor do grupo (o tamanho do ponto de atividade das fixadas, [ADR-0087](0087-zona-de-abas-fixadas-na-barra.md) §3), ou nenhum ponto no grupo implícito. Depois, o título na fonte do sistema, 12.5, Medium, em texto Padrão, e as somas nas colunas, em Iosevka 12.5, em texto Secundário.
  - **Linha de processo**: o nome na fonte do sistema, 12.5, Regular, em texto Padrão, recuado `tree_indent` **16** por nível a partir do início do Nome (a raiz no nível 1 já recua um degrau em relação ao título da aba). Os números em Iosevka 12.5, em texto Secundário.
  - **Linha de nota**: o texto da nota na fonte do sistema, 11, em texto Mínimo, no recuo do nível 1.
  - **Seleção**: fundo `#282e37` (o fundo de aba ativa, `[tabs] active_background`), raio 5 (o do item de menu). **Hover**: fundo `#242a33` (`[context_menu] item_hover_background`), mesmo raio. Seleção e hover juntos: a seleção vence. São `RoundedQuad`s sem borda, então o defeito de borda não se aplica.
- **Números em Iosevka** porque mudam a cada segundo: com dígitos de largura fixa, a coluna não "treme". Os nomes e títulos ficam na fonte do sistema, que é a do chrome ([ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md)).
- **Rolagem**: a lista rola sob o cabeçalho, recortada por `PushClip`, sem barra de rolagem desenhada, como as listas dos popovers.
- Os tamanhos 12.5, 11 e 10 são os da tabela de tipografia da especificação e escalam com `[terminal.font] size`, como o resto do chrome.

### 5. Texto: medir sem pagar por quadro

O CLAUDE.md registra que medir texto no caminho quente sem cache é a armadilha de performance do projeto. Por isso:

- as larguras das colunas numéricas são medidas **uma vez** por tamanho de fonte e idioma, a partir dos modelos do §4, e guardadas na `TaskManagerView`;
- os nomes de processo são medidos uma vez por string e guardados num cache `HashMap<String, f32>` na view, limpo quando a fonte ou a escala mudam. Um processo novo custa uma medição, e os mesmos `node.exe` não custam mais nada;
- o corte com reticências usa os avanços de glifo de **uma** medição da string, nunca uma medição por prefixo;
- só as linhas visíveis são medidas e pintadas.

### 6. O botão do app ligado

Com o gerenciador aberto **nesta janela**, o botão do app é desenhado assim, no lugar das regras de repouso e hover do [ADR-0079](0079-botao-de-menu-do-app.md):

1. **Fundo**: o quadrado de raio `tab_corner_radius` com `pressed_background`, que hoje só aparece no clique ([ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md)). Ele fica fixo enquanto o modo durar. A regra continua em `chrome::zone_button_background`, que ganha a entrada "ligado".
2. **Sombra interna**: uma função nova, `chrome::push_inset_shadow(out, rect, radius)`, irmã de `push_shadow`, com os **mesmos** `SHADOW_LAYERS` e **zero valor novo**. Para cada camada `(spread, offset_y, alpha)`, ela empilha um `PushClip` com a faixa de cima do botão (`rect.y` até `rect.y + offset_y`), um `RoundedQuad` do tamanho e do raio do botão, preto com o alfa da camada, e um `PopClip`. As faixas se sobrepõem a partir do topo, então o primeiro pixel recebe as três camadas, o segundo duas e o terceiro uma: um degradê de 3 px que escurece para a borda de cima e acompanha os cantos arredondados do botão, porque o recorte é reto e a forma é a do botão. É o "afundado" de um botão iluminado de cima, a mesma luz da sombra externa das abas (`offset_y` positivo). `spread` não é usado: a sombra interna só tem a borda de cima.
3. **Ícone**: o ícone do app, como sempre, **depois** da sombra, sem deslocamento.
4. **Hover e clique com o modo ligado** não mudam o desenho: o botão já está "apertado". O clique desliga o modo ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §2) e o botão volta ao repouso.

Fora do modo, o botão segue a revisão de 2026-11-06 do ADR-0079 sem mudança: em repouso só o ícone, e no hover e no clique, o fundo.

Isso atende ao RF-24.3 sem depender de matiz: o ligado tem um fundo que o repouso não tem e uma sombra que nenhum outro estado tem. O nó de acessibilidade diz "alternado" ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §10).

### 7. Cores novas: seção `[task_manager]` dos temas

| Chave | `thm_default.toml` | `thm_default_light.toml` | Origem |
|---|---|---|---|
| `cpu` | `#6fa8f5` | `#1e66f5` | azul: paleta de grupos no escuro; `[terminal.normal] blue` no claro |
| `memory` | `#a68cf0` | `#ea76cb` | roxo: paleta de grupos; `[terminal.normal] magenta` no claro |
| `disk` | `#86c56a` | `#40a02b` | verde: paleta de grupos; `[terminal.normal] green` no claro |
| `grid` | `#1c2027` | `#dce0e8` | separador de painéis no escuro; `[tabs] bar_border` no claro |
| `chart_border` | `#262b34` | `#ccd0da` | borda de controle; `[groups] border` no claro |

No tema claro, as séries usam o azul, o magenta e o verde da paleta ANSI do próprio tema claro, e não a paleta de grupos. A paleta de grupos é a mesma nos dois temas, e o azul `#6fa8f5` sobre o fundo `#eff1f5` tem contraste baixo para uma linha de 1.5 px. É a mesma cor de função (azul, roxo, verde), escolhida pelo tema, como o resto do `thm_default_light.toml` faz.

Pela regra do CLAUDE.md, cada cor entra **no mesmo PR** em três lugares: o campo em `Config` com `#[serde(skip_deserializing)]`, a linha em `theme::mergeable_fields` e as duas `thm_*.toml`. O teste `built_in_themes_declare_every_color` reprova se faltar. Na especificação visual, as cinco entram na tabela de tokens do §1, para o `verify-docs.py` as rastrear.

### 8. Dimensões novas: `[appearance.task_manager]`

| Chave | Valor | Origem do número |
|---|---|---|
| `padding` | 18 | o `padding` do painel da tela de configurações |
| `column_gap` | 18 | o mesmo 18: as colunas respiram o que o quadro respira |
| `chart_gap` | 8 | o `row_gap` da tela de configurações |
| `row_height` | 28 | o `row_height` das listas de popover (§2.22, §2.23) |
| `row_padding_x` | 8 | o `row_padding_x` das mesmas listas |
| `tree_indent` | 16 | **novo**: um degrau de árvore ≈ a largura de dois dígitos de 12.5 |
| `line_width` | 1.5 | **novo**: a linha do gráfico |
| `fill_opacity` | 0.15 | **novo**: o alfa da área sob a linha |

Os cinco primeiros repetem números que já existem em outros componentes, mas ganham chave própria, porque são do gerenciador e podem divergir sem arrastar o popover nem a tela de configurações. Os três últimos são novos, propostos aqui e **pendentes do aval em tela do dono**, como a proporção do tint do ADR-0079 foi. Todas são classe A. No PR de implementação, cada uma entra no [arquivo de exemplo](../config/porecatu.example.toml), na especificação visual e em `VALORES` do `scripts/verify-docs.py`, nos três lados juntos.

### 9. Especificação visual

A seção nova da especificação ("Gerenciador de tarefas", com a etiqueta de fase dos recursos fora de fase) e a linha do botão ligado na seção do botão do app entram **no PR que muda o binário**, como manda o [ADR-0028](0028-o-binario-como-referencia-visual.md): a especificação descreve o binário. Este ADR é a fonte até lá.

## Alternativas consideradas

### Uma cor só (o acento `#5ed3bc`) nos três gráficos

Era a alternativa mais econômica (uma chave de cor). O dono escolheu uma cor por gráfico.

### Só a moldura, sem grade

Menos desenho. O dono escolheu moldura com grade, como no Windows.

### Moldura de raio 6

Combinaria com as linhas de opção da tela de configurações. Recusada **por ora**: a borda de retângulo arredondado não é pintada (defeito aberto). Desenhar a moldura com dois retângulos aninhados (o de fora na cor da borda e o de dentro no fundo) também foi recusado, porque o fundo do quadro pode ser translúcido ou ter imagem, e o retângulo de dentro taparia a imagem. Volta à mesa quando o defeito for corrigido.

### Botão ligado com a caixa de aba ativa

Voltaria ao desenho anterior à revisão de 2026-11-06 do ADR-0079, e uma aba fixada ativa voltaria a ficar igual ao botão, o ponto que aquela revisão fechou. O dono pediu o efeito de pressionado.

### Sombra interna por um passe de filtro (desfoque real)

Sairia mais suave. Recusada: o único passe de filtro do render é o do acrílico ([ADR-0068](0068-efeito-acrilico-no-fundo-do-terminal.md)), e reaproveitá-lo exige ADR (CLAUDE.md). Sombra fica fora do render, em camadas, como a externa.

### Sombra interna nas quatro bordas, com `spread`

Um "afundado" simétrico. Recusada: a luz da sombra externa vem de cima (`offset_y` positivo), e uma interna simétrica pareceria uma borda escura, não um botão apertado. As bordas laterais também pediriam recorte vertical por camada, dobrando as primitivas.

### Números na fonte do sistema

Mais coerente com o resto da linha. Recusada: a Segoe UI tem dígitos proporcionais, e os valores mudam a cada segundo. A coluna tremeria, e a Iosevka já é a fonte de números do chrome (badges, chips).

## Consequências

### Positivas

- **Zero valor sem origem.** Cinco cores e cinco dimensões vêm de tokens que já existem, e as três dimensões novas estão nomeadas, com o aval pendente.
- A sombra interna reaproveita `SHADOW_LAYERS`, de modo que a luz das duas sombras do chrome é uma só.
- O tema claro ganha séries legíveis sem uma paleta nova.

### Negativas

- **Moldura sem raio**, num chrome que arredonda quase tudo, até o defeito da borda ser corrigido.
- Duas famílias de fonte na mesma linha da lista (sistema no nome, Iosevka nos números).
- O botão ligado tem um efeito (sombra interna) que nenhum outro controle tem.
- Mais cinco cores nos temas, e todo tema de usuário herda as do `default` até declará-las.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| A sombra interna de 3 px ficar imperceptível ou pesada demais num botão de 40 px | Média | Baixo | Aval em tela do dono; os números são os de `SHADOW_LAYERS`, e um ajuste pede ADR novo, porque mexe na sombra externa também |
| `fill_opacity` 0.15 sumir sobre imagem de fundo ou acrílico | Média | Baixo | Aval em tela com imagem e acrílico ligados; a chave existe para o usuário ajustar |
| Grade `#1c2027` invisível com `background_opacity` baixa | Média | Baixo | Aval em tela; a cor é do tema e pode ser trocada sem código |
| Coluna de nome sumir em janela estreita | Baixa | Baixo | Regra explícita (§4): os números nunca são cortados; o nome inteiro está no tooltip |
