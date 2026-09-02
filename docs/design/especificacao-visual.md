# Especificação visual

Registro dos valores de aparência do chrome. **O binário é a referência normativa** desde o [ADR-0028](../adr/0028-o-binario-como-referencia-visual.md): este documento descreve o que a build com a configuração padrão desenha, e é atualizado quando ela muda — no mesmo PR que a muda. Os PRDs continuam normativos para **comportamento** (ADR-0009 §1, na metade que segue em vigor).

**Origem dos valores.** Eles nasceram do canvas ([`Terminal Multiplataforma.dc.html`](Terminal%20Multiplataforma.dc.html), cópia verbatim) e a maior parte continua intacta. Onde o binário divergiu — quase sempre por pedido direto do usuário depois de ver a barra em tela —, **vale o valor do binário**, e a mudança está registrada na [seção 4.4](#44-histórico-de-decisões-visuais). Nenhum valor é inventado na implementação: o que entra em código entra aqui e no [`porecatu.example.toml`](../config/porecatu.example.toml) na mesma leva.

> **Nenhuma mudança de aparência sem aval do dono do produto** (ADR-0028 §4). A seção 4.4 é histórico, não lista de tarefas: nada nela autoriza mexer na interface.

> **Aviso de fase.** O mockup contém elementos que **não são do v1**, e para o que é `[v1]` ele é referência **histórica** — divergência entre ele e o binário não é bug. Antes de implementar qualquer coisa daqui, consulte a [tabela de fases](#3-tabela-de-fases). Painéis divididos, perfis, paleta de comandos, painel de configurações, barra de status e a faixa de identidade da barra de título são todos `[v2]`.

---

## 1. Tokens

### 1.1 Tipografia

| Uso | Família | Pesos | Onde |
|---|---|---|---|
| Interface | `Iosevka Fixed` | 400, 500 | títulos de aba, rótulos de grupo, menus, configurações |
| Monoespaçada | `Iosevka Fixed` | 400, 500 | conteúdo do terminal, badges, chips de tecla, contadores, barra de status |
| Ícones | `Lucide` | — | fechar, nova aba, caret de grupo, chevron de overflow |

Interface e conteúdo do terminal usam a **mesma família** desde o [ADR-0026](../adr/0026-chrome-unificado-em-iosevka-fixed.md): o [ADR-0025](../adr/0025-iosevka-no-lugar-da-ibm-plex.md) tinha trocado IBM Plex Sans/Mono por Iosevka Aile/Fixed, mas Aile e Fixed são desenhadas diferente dentro da mesma superfamília (Aile é a variante proporcional/humanista) — lado a lado na barra, a diferença de desenho lia como duas fontes, não como uma identidade só. O peso 600 (`SemiBold`) saiu de uso: a Fixed recortada só embute 400/500. O **mockup ainda mostra IBM Plex** e não será regerado (ADR-0028) — registro na seção 4.4.

Fallback de UI: `system-ui, sans-serif`. Fallback mono: `monospace`.

> **Estas três faces são embutidas no binário** ([ADR-0016](../adr/0016-fontes-embutidas.md)): nenhuma delas vem instalada por default em Windows, macOS ou Linux, e sem embuti-las a aparência mudaria de máquina para máquina — métrica de fonte diferente muda largura de célula, de aba e onde o título trunca. Itálico e negrito fora desses pesos seguem sintetizados (RF-5.4). A cadeia de fallback (Nerd Font, emoji, CJK) **não** é embutida e continua vindo do sistema.

| Tamanho | Uso |
|---|---|
| 19px / 500 | título da tela de nova aba |
| 15px / 500 | título do painel de configurações |
| 14.5px | campo de busca da paleta de comandos |
| 13px | **rótulo da aba** (peso 400) e **nome da pílula do grupo** (peso 500, `Medium` — lê como bold; ver seção 4.4), nome de perfil, item de resultado, campo de nome de grupo |
| 12.5px | itens de menu |
| 12px | nome do app, campo de rename |
| 11px | subtítulo da barra de título, título do painel, descrição de toggle |
| 10.5px | barra de status, comando do perfil |
| 10px | contador do grupo, rótulo de seção (uppercase, `letter-spacing: .7px`) |
| 9.5px | chips de tecla |
| 9px | badge de perfil (`letter-spacing: .4px`) |
| 8px | glyph do logotipo |
| **20px de em** | **todo ícone do chrome** (`chrome::ICON_EM_SIZE`) — é a em, não o desenho: o Lucide preenche ~0.6 dela, então o ✕ sai em 11.8px e o caret do grupo em 11.8px. Configurações usa `0.8 ×` da em e os botões de janela `0.7 ×`, para o desenho bater com o do "+" |

Terminal: **14px** (espec. original pedia 12.5px; foi a 13 por pedido do usuário na F3 e a 14 depois, para o avanço cair em pixel inteiro -- a Iosevka Fixed avança 0.5 em em todo glyph, então `size / 2 * scale` precisa ser inteiro para a célula não ser arredondada), `line-height: 1.75`.

### 1.2 Superfícies

| Token | Valor | Onde |
|---|---|---|
| Fundo do desktop | `#08090b` | fora da janela (só no mockup) |
| Janela | `#15181d` | corpo da janela |
| Barras | `#1b1f26` | barra de título, barra de abas, barra de status |
| Terminal | `#0f1216` | área de conteúdo e fundo do painel |
| Aba ativa | `#282e37` | |
| Aba inativa | `#191d23` | |
| Pílula de grupo | `#1f242c` | **fora de uso** — a pílula é pintada com a cor cheia do grupo (§2.4) |
| Contador do grupo | `#12151a` | também o botão de busca, o fundo do indicador de overflow (§2.18) e o traço do "+", do nome e do caret que caem sobre a cápsula de cor cheia (§2.4, §2.6) |
| Popover | `#1a1e25` | menu de perfis, editor de grupo, paleta de comandos |
| Drawer | `#171b21` | painel de configurações |
| Campo de rename | `#0e1116` | |
| Cartão de perfil | `#161a20` (hover `#1b2028`) | tela de nova aba |
| Linha de perfil | `#1c2028` | configurações |
| Hover de menu | `#242a33` | |
| Hover de aba/botão | `#262b34` · `#252a33` · `#39404b` · `#1e232b` · `#262c35` | por componente, ver anatomia |
| Hover destrutivo | `#2e2224` | "Fechar grupo" |
| Chips de tecla | `#1d222a` · `#1e232b` · `#232830` | por componente |
| Vidro (rim) | `#ffffff` a `.16` | borda translúcida de 1px da cápsula e da pílula do grupo (§2.3, §2.4) |
| Sombra em camadas | preto a `.16` / `.10` / `.06` | três `RoundedQuad` empilhados (spread 1/2.5/4.5, offset Y 1/2/3) sob a cápsula de grupo, a aba solta e o quadro do terminal (§2.3, §2.5, §2.7). É a aproximação de sombra do projeto — não há passo de blur |

### 1.3 Bordas

| Token | Valor | Onde |
|---|---|---|
| Janela / divisor de popover | `#2a2f38` | também borda esquerda do drawer |
| Separador de barra | `#23272f` | base da barra de título e de abas, topo da status, gap entre painéis |
| Borda de controle | `#262b34` | botões da barra, botão de nova aba, borda de card e de linha de perfil |
| Borda de popover | `#2e343e` | |
| Borda da aba ativa | `#39404b` | também hover do botão de busca. **2px** em todo estado (§2.5) |
| Borda da aba inativa | `#22262e` | **2px**, como a ativa |
| Borda da pílula | `#2b313b` | **fora de uso** — a borda da pílula é o rim de vidro (§1.2, §2.4) |
| Borda de input | `#333a45`, foco `#5ed3bc` | |
| Separador de painel | `#1c2027` | base do cabeçalho do painel |
| Card de perfil hover | `#3b434f` | também ponto de painel sem foco |

### 1.4 Texto

| Token | Valor | Uso |
|---|---|---|
| Máximo | `#eaeef3` · `#eef2f4` | rótulo da aba ativa, anel do swatch selecionado |
| Alto | `#e6eaef` · `#e4e8ee` | títulos de painel, input |
| Padrão | `#dfe4ea` · `#d7dce3` | itens de menu |
| Corpo | `#c7ccd6` | **saída padrão do terminal** |
| Rótulo de grupo | `#c3cad3` | |
| Secundário | `#a8b0bb` · `#9aa2ae` · `#98a0ab` | nome do app, rótulo da aba inativa |
| Terciário | `#828a96` · `#8b929e` · `#7b838f` | título do painel, botões da barra de título, contador |
| Tênue | `#6b737e` · `#6f7783` | dicas, saída esmaecida do terminal |
| Mínimo | `#5c646f` | rótulos de seção, chips de tecla |
| Botão de fechar da aba | `#727a86` (hover `#e4e8ee`) | |

### 1.5 Acento e semântica

| Token | Valor | Uso |
|---|---|---|
| Acento | `#5ed3bc` | logotipo, foco de input, nome do shell na status, prompt da paleta |
| Sucesso | `#86c56a` | saída OK do terminal |
| Aviso | `#e0b060` | saída WARN |
| Erro | `#ef8a8a` | saída ERROR; também a primeira cor de grupo |
| Destrutivo forte | `#c4413f` | hover do botão de fechar a janela, com o ícone em `#ffffff` (§2.2.1) |
| Destrutivo brando | `#e08585` | item "Fechar grupo" |
| Toggle ligado | `#3f8f80` (trilho), `#f0f3f6` (botão) | desligado: `#2a3038` |
| Seleção de texto | `#2e6b62` (fundo), `#eef2f4` (texto) | RF-5.14. Nasceu do `::selection` do canvas e é o valor que o binário desenha — logo, o valor deliberado (ADR-0028) |

### 1.6 Paleta de grupos

Seis cores, nesta ordem. É a lista oferecida no editor de grupo e a sequência de atribuição automática ([PRD-002](../prd/prd-002-grupos-de-abas.md) RF-2.4).

| # | Cor | Nome sugerido |
|---|---|---|
| 1 | `#ef8a8a` | vermelho |
| 2 | `#e0b060` | amarelo |
| 3 | `#5ed3bc` | ciano |
| 4 | `#6fa8f5` | azul |
| 5 | `#a68cf0` | roxo |
| 6 | `#86c56a` | verde |

Cor de aba sem grupo: `#7b838f`.

### 1.7 Geometria

| Raio | Onde |
|---|---|
| 10px | modal da paleta de comandos |
| 8px | janela, popover, cápsula de grupo, card de perfil |
| 6px | **aba**, pílula de grupo, botões da barra, swatch, linha de resultado, **quadro do terminal** (§2.7) |
| 5px | item de menu, input de grupo, badge de perfil grande |
| 4px | botão de fechar da aba, botões do painel, campo de rename |
| 3px | badge de perfil, chips de tecla, logotipo, swatch do grupo |
| 9px | indicador de overflow (círculo de 18px, raio = metade — §2.18); era o contador do grupo, hoje fora de uso na pílula |
| 50% | ponto de status do painel |

| Altura | Valor |
|---|---|
| Barra de título `[v2]` | 36px |
| **Barra de abas** | **52px** — `chrome::bar_height` = aba 34 + `wrapper_padding` 3 nos dois lados + `trilha_padding` 6 nos dois lados. Sai de uma função só: recalcular a altura localmente já custou um respiro inferior que não aparecia, porque o recorte da trilha vinha de uma cópia velha da conta |
| **Aba dentro de grupo, pílula de grupo** | **34px** (`tab_height`) |
| **Aba solta** | **40px** — `tab_height + wrapper_padding * 2`: sem bloco de grupo a que ceder o `wrapper_padding`, ela ocupa a caixa inteira do wrapper e alinha topo e base com a agrupada (§2.5) |
| Barra de status `[v2]` | 26px |
| Botão da barra de título `[v2]` | 44px de largura |
| **Botão de janela** (minimizar / maximizar / fechar) | **46px de largura, altura cheia da barra** — ADR-0027, §2.2.1 |
| **Zona de resize da janela** | **6px** em toda borda (ADR-0027) |
| Botão de fechar da aba | 17×17 de desenho, **25×17** de alvo com o respiro de `icon_button_padding_x` |
| Botão da zona fixa à direita (configurações) | 30×30 de desenho, **38×30** de alvo |
| **Indicador de overflow** | **18×18**, círculo (§2.18) |
| Botões do painel `[v2]` | 22×20 |
| Swatch de cor | 28×28 |
| Toggle `[v2]` | 34×19, botão 15×15, deslocamento 15px |
| Cursor do terminal | `1.2 ×` o tamanho da fonte de altura, uma célula de largura (7×16.8 nos 14px de hoje; o 7×15 do mockup é a mesma proporção sobre 12.5px) |

| Largura | Valor |
|---|---|
| **Aba** | **fixa e igual para toda aba**: `padding_left` 10 + rótulo 180 + `internal_gap` 8 + botão de fechar 25 + `padding_right` 6 = **229px**, saturada em `max_width` 260. O teto de 180px do rótulo é também o piso — título, indicador e renomeação não refluem a trilha (§2.5) |
| Nome da pílula do grupo | teto de **140px** (`pill_name_max_width`), truncado com reticências |
| Trilha | largura da barra **menos** a zona fixa à direita (`right_zone_width` = `trilha_gap` 6 nos dois lados + o botão de 38px). No macOS reserva-se ainda `MACOS_TRAFFIC_LIGHT_INSET` (78px) à esquerda, para a trilha não desenhar sob o semáforo nativo |

| Espaçamento | Valor |
|---|---|
| **Trilha da barra de abas** | `trilha_padding` **6px nos quatro lados** — o mesmo respiro que a §2.5 sempre descreveu ("aba h30 + padding 6px da barra") e que a implementação não tinha |
| Entre abas do mesmo grupo | `gap: 4` |
| **Entre grupos** | `gap: 6` (`trilha_gap`) — também o gap antes do botão de nova aba e o respiro das zonas fixas |
| Cápsula de grupo | `padding: 3` (`wrapper_padding`). **Não** se aplica a run implícito: sem cápsula não há o que absorva o respiro, e aplicá-lo abria entre grupo e aba solta um vão maior — e sem cor — do que entre dois grupos |
| Aba | `padding: 0 6px 0 10px`, `gap: 8` |
| Pílula | `padding: 0 9px 0 10px`, `gap: 7` (o `label_padding_left` subiu de 8 para 10) |
| **Dentro de todo botão de ícone** | `icon_button_padding_x` **4px de cada lado** — fechar da aba, os dois "+", caret da pílula e o botão da zona fixa ficam mais largos que altos; a altura não muda, senão o botão de fechar deixaria de caber na aba |
| **Quadro do terminal** | 6px de margem da borda da janela nos três lados que não encostam na barra, e 6px de padding entre a borda do quadro e a grade (§2.7) |
| Cabeçalho do painel | `padding: 7px 12px` |
| Conteúdo do painel | `padding: 12px 14px` |
| Item de menu | `padding: 7px 8px` |
| Drawer | `padding: 18px`, `gap: 24` entre seções |

Sombras. O binário desenha sombra **em camadas** (§1.2): três `RoundedQuad` pretos empilhados, crescendo de spread (1 / 2.5 / 4.5) e caindo de alfa (`.16` / `.10` / `.06`), com offset Y de 1 / 2 / 3. É a aproximação possível sem passo de blur, e está em três lugares — cápsula de grupo, aba solta e quadro do terminal (`chrome::push_shadow`).

Os valores CSS do mockup — janela `0 32px 80px rgba(0,0,0,.6)`, popover `0 18px 44px rgba(0,0,0,.55)`, modal `0 28px 70px rgba(0,0,0,.6)` — descrevem manchas grandes, que aliaseariam nessa técnica. Nos cinco widgets de chrome e no fantasma de arraste, a sombra em camadas é **uma das duas mudanças visuais aprovadas** do v1, agendada para a F4 ([ADR-0032](../adr/0032-interface-do-v1-fechada.md) §2, lista exaustiva).

Overlays: diálogo de confirmação `rgba(6,7,9,.45)`; paleta de comandos `[v2]` `rgba(6,7,9,.55)`.

### 1.8 Tingimentos

Derivados da cor do grupo, por composição alfa. Cápsula e pílula são pintadas com a **cor cheia** do grupo, num alfa alto que deixa passar um traço do que está atrás — é o efeito de vidro, junto com o rim translúcido da §1.2:

| Alvo | Alfa | Efeito |
|---|---|---|
| **Cápsula do grupo** | `.85` da cor cheia | também com o grupo **colapsado**, abraçando a pílula sozinha — é a cápsula que diz de que cor o grupo é. Por trás dela passa `BAR_BACKGROUND` |
| **Pílula do grupo** | `.92` da cor cheia | fica por cima da cápsula, do mesmo tom: duas camadas translúcidas empilhadas, "vidro sobre vidro" |
| **Fundo da aba** | `.85` do próprio tom (ativa `#282e37`, inativa `#191d23`) | deixa passar 15% da cápsula atrás dela como indício da cor do grupo |
| Realce de fronteira do arraste | `.14` da cor do grupo, borda na cor a `.45` | §2.19. O `.16` que a prosa daquela seção citava arredondava o `badge_tint_strength` do arquivo de exemplo, que vale `.14` — o TOML é a fonte numérica |
| Fundo do badge de perfil `[v2]` | `.14` | texto do badge usa a cor cheia |
| Fundo do chip de tipo na paleta `[v2]` | `.14` | |

O tingimento de `.07` que esta seção pedia para o wrapper não se via atrás do fundo das abas; a cápsula de cor cheia o substituiu, por pedido do usuário. `tint_strength` continua no arquivo de exemplo e governa o valor a partir da F4 — com default `1.0`, que é o que o binário desenha.

### 1.9 Paleta ANSI derivada

O design define seis cores semânticas de saída de terminal (seções 1.4 e 1.5), não uma paleta ANSI completa. As 16 cores são **derivadas** delas — esta seção é a origem declarada dos valores em `[terminal.colors.normal]` e `[terminal.colors.bright]` do [`porecatu.example.toml`](../config/porecatu.example.toml), para que nenhuma cor da configuração fique sem procedência.

Regra: as cores normais reaproveitam as semânticas do design; as brilhantes são as mesmas clareadas, preservando o matiz.

| Slot | Normal | Origem | Brilhante |
|---|---|---|---|
| black | `#3b434f` | superfície de contorno tênue | `#6f7783` (esmaecido do design) |
| red | `#ef8a8a` | erro | `#f5a3a3` |
| green | `#86c56a` | sucesso | `#9bd482` |
| yellow | `#e0b060` | aviso | `#ecc37c` |
| blue | `#6fa8f5` | 4ª cor de grupo | `#8dbcf8` |
| magenta | `#a68cf0` | 5ª cor de grupo | `#bda6f5` |
| cyan | `#5ed3bc` | acento | `#7fdfcc` |
| white | `#c7ccd6` | texto de corpo | `#eaeef3` (texto máximo) |

Só as oito brilhantes de cor (`red` a `cyan`) são valores novos; `black`, `white` e todas as normais já existiam no design. Cores de 256 e true color emitidas pelos programas não passam por esta paleta.

### 1.10 Movimento

| Animação | Definição | Onde |
|---|---|---|
| `blink` | `1.1s step-end infinite` | cursor do terminal |
| `pop` | `.13s ease-out`, de `opacity 0` + `translateY(-4px)` | popovers; `.14s` no modal da paleta |
| `slidein` | `.16s ease-out`, de `translateX(24px)` + `opacity 0` | drawer de configurações |
| Transições | `.15s` | rotação do caret, cor do trilho e posição do botão do toggle |
| `reflow` | `.18s` linear ao formar grupo; **`.15s`** no colapso e na expansão | reordenação das abas ao formar grupo (RF-2.5) e ao arrastar grupo; colapso e expansão de grupo. São duas durações, uma por consumidor (`GROUP_CREATE_REFLOW_DURATION`, `COLLAPSE_REFLOW_DURATION`) |
| Hover por brilho | `filter: brightness(1.25)` na pílula, `1.18` na aba | evita definir uma cor de hover por grupo. É a **segunda das duas mudanças visuais aprovadas** do v1, agendada para a F4 ([ADR-0032](../adr/0032-interface-do-v1-fechada.md) §2); hoje o único hover que troca fundo e ícone é o dos botões de janela (ADR-0027) |

O hover por `brightness` é uma decisão relevante: com seis cores de grupo, definir hover por cor exigiria doze tokens. O filtro resolve com um.

**`reflow` é a única animação de movimento do v1**, e é interpolação **linear**,
sem curva: 150–180 ms de deslocamento não distinguem easing, e o projeto
não tem primitiva de curva. Ela interpola **posição e largura** da cápsula, mais a
**opacidade** das abas que entram ou saem da trilha — deslocar só a posição faz os
vizinhos parecerem suaves e o grupo que o usuário está tocando saltar. Ela existe porque o RF-2.5 a exige como cenário de
aceite — abas selecionadas em três pontos distantes da barra aparecendo juntas
noutro lugar, sem nada na tela explicando o quê, é a surpresa que o
[ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md) registrou como risco. É
animação **explicativa**, não decorativa, e é por isso que ela entra onde o
indicador que pisca e o easing de rolagem foram recusados (§2.17, §2.18).

Qualquer interação durante o `reflow` aplica o estado final na hora e descarta o
movimento: animação não enfileira input. `animations = false` aplicará todo
`reflow` instantaneamente — as abas ficam contíguas, só não se vê o caminho —, e
essa chave passa a existir na F4; hoje as duas durações são constantes. Ver
[ADR-0022](../adr/0022-animacao-de-interface.md).

---

## 2. Anatomia

### 2.1 Barra de título `[v2]`

Altura 36, fundo `#1b1f26`, borda inferior `#23272f`, `padding-left: 12px`.

**Esquerda** (`gap: 9`): logotipo 14×14, raio 3, borda `1.5px #5ed3bc`, com `>` mono 8px `#5ed3bc` centralizado. Nome "Porecatu" 12px/500 `#9aa2ae` (`letter-spacing: .2px`). Travessão `—` 11px `#5c646f`. Rótulo da aba ativa 11px `#5c646f`.

**Direita**: três botões de 44px de largura, altura cheia, `#8b929e`. Minimizar 11px, maximizar 10px, fechar 11px. Hover `#252a33`; o de fechar vira `#c4413f` com texto `#ffffff`.

**Ainda `[v2]` só nesta faixa própria** (logo, nome do app, travessão, título da aba ativa) — é identidade de app, não controle de janela. O [ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md) tirou os três botões e o resize dela e os deu à barra de abas existente, sem empilhar uma segunda barra: ver §2.2.1, que já é `[v1]` fora do macOS. As cores de hover acima (`#252a33`, `#c4413f`, `#ffffff`) são as que §2.2.1 reaproveita.

### 2.2 Barra de abas `[v1]`

Altura **52px** (§1.7), fundo `#1b1f26`, e o conteúdo recuado por `trilha_padding` **6px nos quatro lados**. A borda inferior `#23272f` que o mockup desenha **não é pintada**: ela virava uma linha contra o quadro do terminal (§2.7), que o usuário pediu para tirar — a cor segue registrada na §1.3 para o caso de `tab_bar_position = "bottom"`, que muda a aresta.

Zonas, da esquerda para a direita:

1. **Trilha rolável** `[v1]` — ocupa toda a largura que sobra, `gap: 6`, recortada nas duas pontas. Contém os wrappers de grupo, e só eles. Rola como **um componente só**: nada dentro dela encolhe (§2.18).
2. **Zona fixa à direita** `[v1]` — não rola, e a largura disponível para a trilha é a da barra **menos** esta zona. Carrega o **botão de configurações**: 30×30 de desenho (38×30 de alvo), raio 6, borda `#262b34`, engrenagem Lucide, com o mesmo `gap: 6` da trilha como respiro nas duas pontas. Ele **desenha e consome o clique, mas não faz nada** — `porecatu-config` é F4. Consumir o clique é deliberado: sem isso o gesto atravessaria até o que estivesse embaixo. A zona nasceu na F3 para o botão de nova aba global, que saiu (§2.6); ela fica, reservada para o que a barra ganhar à direita daqui em diante.
3. **Botão de busca** `[v2]` — altura 30, `padding: 0 10`, raio 6, fundo `#12151a`, borda `#262b34` (hover `#39404b`). Texto "Buscar" 11px `#6b737e` + chip `Ctrl+Shift+P` mono 9.5px `#7b838f` sobre `#1d222a`, raio 3, `padding: 2px 5px`.
4. **Zona de botões de janela** `[v1]` — minimizar, maximizar/restaurar, fechar (ADR-0027; anatomia em §2.2.1). Ausente no macOS: lá o semáforo nativo faz esse papel, e a trilha reserva espaço à **esquerda** (§2.2.1) em vez de perder espaço à direita.

**Por que a zona fixa existe.** Com a trilha rolando como um componente só, um botão ao final dela sai de vista com muitas abas — botão que o usuário não alcança. A largura da trilha é a da barra menos esta zona, e é essa largura que o cálculo de overflow e o auto-scroll do arraste usam.

Quatro comportamentos que o canvas não mostra e que a configuração alcança:

- **`tab_bar_position = "bottom"`** (RF-4.1): a barra vai para a base da janela e a borda `#23272f` muda para a **aresta superior**. Nada mais muda — mesmos raios, mesmo padding, mesma trilha. A pilha de avisos continua ancorada no alto da área de conteúdo.
- **`hide_when_single_tab`** (RF-4.2): a barra aparece e desaparece **sem transição**, e a grade é redimensionada no mesmo frame. Animar a altura da barra animaria um resize de PTY, e resize por quadro é uma tempestade de `SIGWINCH` no programa que está rodando.
- **`show_index`** (RF-4.11): prefixo antes do rótulo, em mono 10px `#7b838f` — os tokens do contador que a pílula tinha —, com o `gap: 8` da aba. Consome largura do rótulo como o ponto de indicador da seção 2.17.
- **Janela sem foco:** a barra **não muda**. É omissão deliberada, não pendência: o `unfocused_hollow` do cursor (seção 2.7) já diz qual janela tem o foco, e no lugar onde o usuário está olhando. Esmaecer a barra inteira de cada janela inativa é ruído maior que a informação que carrega.

**Folga de acerto:** o botão de fechar de 17×17 e o `gap: 4` entre abas são alvos pequenos. O hit-testing dá 2 px de folga em volta do botão de fechar, o `icon_button_padding_x` da §1.7 alarga o alvo em 4 px de cada lado, e a fronteira entre abas vizinhas parte o `gap` ao meio — nenhum pixel da barra fica sem dono.

### 2.2.1 Controles de janela e resize `[v1]`

[ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md): fora do macOS a janela perde a decoração nativa (`decorations = false`), e a própria barra de abas assume o que a decoração fazia — sem token de design próprio, a espec. nunca cobriu janela sem decoração nativa antes deste ADR.

**Drag region** — a área vazia da barra (fora de aba, pílula, "+", botão de configurações e botões de janela) arrasta a janela. Duplo clique nela maximiza/restaura.

**Botões de janela** — três, **46px** de largura cada, altura cheia da barra (52px), colados na borda direita: minimizar, maximizar/restaurar, fechar. Ícones Lucide 14px (`ICON_EM_SIZE * 0.7`): `minus`, `square`, `copy` (sem ícone dedicado de "restore" no Lucide; dois quadrados sobrepostos, aproximação comum de outras suítes) para minimizar/maximizar/restaurar, `x` para fechar — mesmo ícone da aba. Cor de repouso `#8b929e`, hover `#252a33`; fechar em hover vira `#c4413f` com ícone `#ffffff` — os três tons são os que §2.1 já descrevia para a faixa `[v2]`, sem cor nova.

**Resize por borda** — 6px em toda borda da janela (não só a barra), desligado com a janela maximizada. Sem token de design.

**macOS**: nenhum botão nosso — decoração nativa (semáforo) continua. A trilha reserva **78px** à esquerda (`MACOS_TRAFFIC_LIGHT_INSET`, não medido contra `NSWindow` real) para não desenhar sob ele.

### 2.3 Wrapper de grupo `[v1]`

Envolve a pílula e as abas do grupo. `gap: 4`, raio 8, e `padding: 3`
(`wrapper_padding`) **só quando há cápsula** — ver a última nota desta seção.

**A cápsula é pintada com a cor cheia do grupo, a `.85` de alfa** (§1.8), com um rim
de vidro de 1px em branco a `.16` (§1.2) e a sombra em camadas da §1.7 por baixo. Ela
é desenhada **expandida e colapsada**: com o grupo fechado ela abraça a pílula
sozinha, porque é a cápsula que diz de que cor o grupo é, e sumir com ela tiraria a
única marca de cor justo quando o nome do grupo é tudo o que resta na barra.

**Abas sem grupo usam um wrapper sem pílula e sem cápsula** — é a representação
visual do grupo implícito do
[ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md), e nunca ganha cápsula. Duas
consequências: a aba solta é **mais alta** que a agrupada, porque não tem bloco a que
ceder o `wrapper_padding` (§2.5), e ela leva sombra e borda por conta própria, já que
não há cápsula que as carregue por ela.

**O `wrapper_padding` não entra em run implícito.** Sem cápsula não há o que absorva o
respiro: aplicá-lo abria, entre um grupo e as abas soltas ao lado, um vão maior — e
sem cor — do que o vão entre dois grupos.

O `tint_strength = 0.07` do arquivo de exemplo (RF-4.19) era o valor original desta
seção, descartado na F3 por pedido do usuário: com o fundo da aba opaco por cima, 7%
da cor do grupo ficava invisível — o indicador de grupo mais visível da barra não
podia ser o menos legível dela. Em vez do tingimento vieram a cor cheia e o alfa `.85`
no fundo da aba (§2.5), que deixa passar um indício da cápsula por baixo de cada uma.
A chave continua no arquivo e passa a governar o valor na F4, com default `1.0`.
Registro na seção 4.4.

### 2.4 Pílula de grupo `[v1]`

Altura **34** (a mesma da aba), `padding: 0 9px 0 10px`, raio 6, `gap: 7`.
**Fundo: a cor cheia do grupo a `.92`** (§1.8), com o mesmo rim de vidro de 1px da
cápsula (§1.2). Ela fica por cima da cápsula, do mesmo tom — duas camadas
translúcidas empilhadas. Hover `brightness(1.25)`: aprovado para a F4 (§1.10).

Da esquerda para a direita:

1. **Nome** 13px/**500** (`Medium`, para ler como bold contra o 400 da aba) em
   `#12151a`, sem quebra, `max-width: 140px`, truncado com reticências (RF-2.12).
   Nome completo em tooltip (seção 2.20), pelo mesmo caminho da aba.
2. **Caret** — ícone Lucide `chevron-right` colapsado, `chevron-down` expandido, na
   em de 20px da §1.1, em `#12151a`. Não há rotação: `porecatu-render` não tem
   transformação afim, e a troca de ícone é o equivalente estático.

**Nome e caret em `#12151a`** porque caem sobre a cor cheia do grupo, onde o claro
perde contraste — é o mesmo escuro do "+" de dentro do grupo (§2.6), pela mesma
razão. O **swatch de 8×8** que a pílula tinha à esquerda saiu junto com a borda
neutra `#2b313b`: com a pílula inteira pintada na cor do grupo, o quadrado ficou
redundante e a borda virou contorno cinza sem função. Tudo pedido do usuário; ver a
seção 4.4.

Interação: clique alterna colapso; duplo clique abre o editor.

**Teto do nome, e nenhum piso.** O teto de 140 px é o da aba (180) menos os 41 px de
cromo que a §2.18 contabiliza — uma pílula não deve poder ficar mais larga que a aba
que ela rotula. **Não há piso:** o nome nunca encolhe abaixo do teto, porque nada
encolhe — a trilha rola (§2.18). O piso de 60 px que esta seção especificava, e a
`label_min_width` que o carregava no arquivo de exemplo, saíram na F3; a métrica de
dez grupos do [PRD-002](../prd/prd-002-grupos-de-abas.md) continua valendo por
rolagem, não por encolhimento.

**Indicador agregado de grupo colapsado (RF-2.16).** Ponto de 6×6, raio 50%,
antes do nome, com o mesmo `gap: 7` dos demais elementos — as cores são
as da seção 2.17 (`#86c56a` atividade, `#ef8a8a` campainha), e vale a mesma regra
de **um ponto só, campainha vence**. Só aparece com o grupo **colapsado**: com o
grupo expandido, cada aba mostra o seu próprio ponto e um agregado seria
redundante. Como a §2.17, **não pisca**, e o ponto consome largura do nome, não
da pílula.

**A pílula não tem contador.** O contador mono 10px sobre `#12151a` que o mockup
desenha entre o nome e o caret **saiu**, por pedido do usuário. `show_tab_count_when_collapsed`
(RF-4.17) fica no arquivo de exemplo e volta a governar um contador se ele voltar; os
tokens `count_background`/`count_foreground` continuam em uso, mas noutros lugares —
o escuro virou a cor de nome, caret e "+" sobre a cápsula, e o fundo do indicador de
overflow (§2.18). Registro na seção 4.4.

**Grupo colapsado** muda três coisas e nada mais: o caret troca de ícone, o "+" do
grupo desaparece (§2.6) e o indicador agregado pode aparecer. A cápsula **continua
pintada** (§2.3) e a pílula mantém altura, paddings e teto. As abas somem da trilha,
e o que anima nesse movimento é a largura da cápsula mais a opacidade delas
(§1.10).

### 2.5 Aba `[v1]`

Altura **34** dentro de um grupo e **40** solta (§1.7), `padding: 0 6px 0 10px`,
raio 6, `gap: 8`, **borda 2px** em todo estado, **largura fixa de 229px**. Hover
`brightness(1.18)`: aprovado para a F4 (§1.10).

**A aba solta é mais alta.** Dentro de um grupo ela cede o `wrapper_padding` ao bloco
do grupo; solta não há bloco a que ceder, então ocupa a caixa inteira do wrapper.
As duas alinham topo e base na barra, em vez de a solta parecer encolhida no meio de
um vão vazio. A aba solta leva também a sombra em camadas (§1.7) e a borda de
controle `#262b34` — o que a cápsula carrega pelas agrupadas.

**A largura é fixa e igual para toda aba** (§1.7): `padding_left` + o teto de 180px do
rótulo + `gap` + botão de fechar + `padding_right`. O teto do rótulo virou também o
piso, porque largura por conteúdo refluía a trilha inteira a cada título novo —
trocar de aba, renomear ou abrir um programa que muda o título mexia na posição de
todas as outras. `max_width` continua no arquivo de exemplo, agora como saturação.

| Estado | Fundo | Borda | Texto |
|---|---|---|---|
| Ativa | `#282e37` a `.85` | `#39404b`, 2px | `#eaeef3` |
| Inativa | `#191d23` a `.85` | `#22262e`, 2px | `#98a0ab` |
| Selecionada (RF-2.2) | o do estado de base | `#5ed3bc`, 2px **por dentro** | o do estado de base |

**O fundo da aba tem alfa `.85`.** Os dois valores da tabela são as cores do design;
o que a F3 acrescentou foi a transparência, para que a cápsula do grupo (§2.3)
apareça por baixo da aba em vez de ficar coberta. Sobre a barra (`#1b1f26`), fora de
qualquer grupo, o resultado é indistinguível do opaco — o que muda é só o que se vê
dentro de um grupo.

**Selecionada é um modificador, não um quarto estado.** Uma aba pode estar
selecionada e ativa ao mesmo tempo (RF-2.2 é explícito: *"uma aba pode estar
selecionada sem estar ativa"*), então o fundo e o texto continuam vindo de
"Ativa" ou "Inativa" e só a borda muda. O valor é o `selected_border` do arquivo
de exemplo, que é o acento `#5ed3bc` da seção 1.5 — o mesmo do campo de rename e
do anel de foco do diálogo, aqui no seu terceiro papel de "isto está sob a ação
do usuário".

A borda é desenhada **para dentro** e não soma largura: a aba **não muda de tamanho
ao ser selecionada**, pela mesma razão que não muda ao entrar em rename. Ela tem 2px
em todo estado — 1px do `#22262e` da inativa não se lê contra a cápsula de cor cheia,
e a espessura vem do `indicator_thickness`/`selected_border_width` que o arquivo de
exemplo já usava. Como a espessura é a mesma em todo estado, o que distingue a aba
selecionada é só a **cor** do token.

**Não há sublinhado de grupo, e não há como ligá-lo.** O `box-shadow: inset 0 -2px 0
<cor do grupo>` na base da aba existia para dizer a que grupo ela pertence quando a
pílula sai da vista por rolagem; desde que a cápsula passou a ser pintada com a cor
cheia (§2.3), a cor do grupo já está atrás da aba inteira, e o traço virou ruído. O
indicador de grupo é a pílula mais a cápsula, e a chave `indicator_style` **deixou de
existir** — junto com os estilos `left-bar` e `outline`, que nunca tiveram anatomia
([ADR-0032](../adr/0032-interface-do-v1-fechada.md)). Registro na seção 4.4.

Conteúdo:

1. **Badge de perfil** `[v2]` — mono 9px/500, raio 3, `padding: 2px 4px`, `letter-spacing: .4px`. Texto na cor do grupo, fundo na cor do grupo com alfa `.14`.
2. **Rótulo** 13px/400 (12.5px originalmente; pedido do usuário, ver seção 4.4),
   `max-width: 180px` — que é também o piso —, truncado com reticências.
3. **Botão de fechar** 17×17 de desenho e 25×17 de alvo (§1.7), raio 4, ícone Lucide
   `x` na em de 20px, em **`#e4e8ee`** já em repouso: contra a barra `#1b1f26` o traço
   fino do Lucide desaparece num cinza médio, então o tom que a espec. reservava para
   o hover virou o tom de base. Hover: fundo `#39404b`.

**Campo de rename** `[v1]` — substitui o rótulo no lugar. Largura 120, fundo `#0e1116`, borda `1px #5ed3bc`, raio 4, texto `#e4e8ee` 12px, `padding: 2px 5px`, `outline: none`, foco automático. Confirma em `Enter` e no blur; cancela em `Esc`.

A largura é `min(120, largura disponível do rótulo)` — a cláusula sobrevive à mudança da §2.18, que deixou o rótulo sempre no teto de 180 px, porque um título curto ainda dá uma aba mais estreita que o campo. **A aba não muda de largura ao entrar em rename**: mudar reflui a barra enquanto o usuário digita. Título mais longo que o campo rola dentro dele, com o caret sempre visível. OSC 0/2 que chegue com o campo aberto é registrado mas **não redesenha o campo**: o texto sob o cursor do usuário não muda sozinho.

**Aba no estado `Exited`** ([ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md)): fundo e borda de aba inativa, e o rótulo esmaecido para `#727a86` — o tom do botão de fechar, que é o token de "inerte" da barra. Nenhum indicador (seção 2.17) e nenhum estado novo: o **motivo** de a aba ter ficado aberta é o código de saída escrito no grid, que sobrevive à rolagem e continua lá quando o usuário voltar; um quarto estado de aba exigiria cor nova para dizer o que a nota já diz.

**Cor de grupo das abas soltas:** para as abas do grupo implícito do
[ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md), a "cor do grupo" é o `#7b838f` da
seção 1.6 — o `ungrouped_color` do arquivo de exemplo. Sem sublinhado e sem cápsula,
quem ainda o usa é o realce de fronteira do arraste sobre um run implícito (§2.19) e o
fantasma do arraste de grupo.

### 2.6 Botão de nova aba `[v1]`

**17×17 de desenho e 25×17 de alvo** (§1.7 — o mesmo tamanho do botão de fechar da
aba, e não os 30×30 que o mockup dava ao botão global), raio 6, ícone Lucide `plus` na
em de 20px, borda `1px #262b34`. Hover: fundo `#262b34`.

**Um por grupo, e só.** O botão fica logo depois da última aba do wrapper e executa
`group.new_tab` naquele grupo (RF-2.8), inclusive num run implícito, que é o que dá
um "+" às abas soltas. `show_new_tab_button = false` desliga o botão e devolve a
largura dele ao wrapper.

**Grupo colapsado não tem botão.** O wrapper colapsado é a pílula e mais nada: as
abas dele sumiram da barra (§2.4), e um "+" ao lado da pílula criaria aba num grupo
cujas abas não estão à vista. O wrapper encolhe para caber só a pílula, que é o que
faz o colapso parecer colapso.

**Mais um, ao fim da trilha, para a aba solta.** Fica **fora** de qualquer wrapper,
sobre o fundo da barra, e cria uma aba fora de todo grupo. Só aparece quando o último
grupo da barra é **explícito**: se a barra já termina num run de abas soltas, o "+"
daquele run cria exatamente isso, no mesmo lugar. É ele que cobre o caso em que toda
aba está em grupo — sem ele, um workspace com um único grupo colapsado não tem gesto
nenhum que crie uma aba solta.

**A cor do ícone depende do que está atrás dele**, e é o único lugar do chrome que
decide cor assim: `#12151a` sobre a cápsula de cor cheia de um grupo explícito,
`#e4e8ee` sobre a barra escura de um run de abas soltas, que não tem cápsula.

Houve também um botão **global**, numa zona fixa à direita da barra (§2.2), entre a
F3 e o fim dela. Foi removido: com um "+" por grupo, e todo run de abas soltas sendo
um grupo implícito, ele era um segundo botão para a mesma ação a um palmo do
primeiro. A zona ficou, e hoje carrega o botão de configurações. Ver a seção 4.4.

### 2.7 Área de terminal `[v1]`

Fundo `#0f1216`, desenhado dentro de um **quadro arredondado** — raio 6 (o mesmo das
abas), 6px de margem da borda da janela nos três lados que não encostam na barra de
abas, e a sombra em camadas da §1.7 por baixo. Em cima ele começa colado em
`bar_height`, sem gap: um vão ali é uma linha visível entre a trilha e o terminal. A
grade fica recuada mais 6px por dentro do quadro, nos quatro lados. Tudo pedido do
usuário, com os valores tirados de `trilha_padding` e `wrapper_padding` (§1.7) — nada
de número novo. Registro na seção 4.4.

Com painéis divididos `[v2]`, os painéis ficam lado a lado com `gap: 1px` sobre `#23272f` — o gap é o divisor.

**Painel:** `border-top: 2px` na cor do grupo quando focado, `transparent` quando não. O anel só aparece com mais de um painel.

**Cabeçalho do painel** `[v2]` — `padding: 7px 12px`, borda inferior `#1c2027`. Ponto 6×6 circular na cor do grupo quando focado, `#3b434f` quando não. Título mono 11px `#828a96`, truncado. À direita, dividir (`◫` 11px) e fechar (`✕` 10px), 22×20, raio 4, `#6b737e`, hover fundo `#1e232b` e ícone `#cfd5dd`.

**Conteúdo** — `padding: 12px 14px`, mono 12.5px, `line-height: 1.75`, `white-space: pre-wrap`.

**Prompt e cursor** `[v1]` — primeira parte do prompt na cor do grupo, segunda em `#6b737e`. Cursor de uma célula de largura e `1.2 ×` o tamanho da fonte de altura (§1.7), cor do grupo, `animation: blink 1.1s step-end infinite`. Ele é ancorado no **topo** da linha, não centrado na altura de linha: a caixa do glyph começa ali, e a folga do `line-height` de 1.75 fica embaixo.

Cores de saída: padrão `#c7ccd6`, esmaecido `#6f7783`, sucesso `#86c56a`, aviso `#e0b060`, erro `#ef8a8a`, destaque `#5ed3bc`.

### 2.8 Barra de status `[v2]`

Altura 26, `padding: 0 12`, fundo `#1b1f26`, borda superior `#23272f`, mono 10.5px `#6b737e`, `gap: 16`.

Esquerda: nome do shell em `#5ed3bc`, diretório atual, grupo da aba. Direita: codificação, contagem de painéis, sistema e versão.

### 2.9 Menu de perfis `[v2]`

Popover `top: 76px`, largura 268, fundo `#1a1e25`, borda `#2e343e`, raio 8, `padding: 6`, sombra `0 18px 44px rgba(0,0,0,.55)`, animação `pop .13s`.

Rótulo "Perfis" 10px uppercase `#5c646f`, `letter-spacing: .7px`. Itens: `padding: 7px 8px`, raio 5, `gap: 10`, hover `#242a33` — badge, nome 12.5px `#d7dce3`, tecla mono 9.5px `#5c646f`. Divisor `1px #2a2f38`, `margin: 5px 4px`. Ao final, "Novo grupo de abas".

### 2.10 Editor de grupo `[v1]`

Popover largura 286, `padding: 14`, `gap: 13`. Mesmo fundo, borda e raio do menu de perfis. Posicionado horizontalmente sobre o grupo que está sendo editado.

> **A borda de 1px é o que separa o popover do fundo hoje**, nos cinco widgets de chrome. A sombra em camadas da §1.7 é uma das duas mudanças visuais aprovadas e entra na F4 ([ADR-0032](../adr/0032-interface-do-v1-fechada.md) §2). O `pop .13s` **não entra**: a única animação do v1 é o `reflow` (§1.10, ADR-0022, com a lista de consumidores fechada em dois).

**Posição vertical: 8 px abaixo da borda inferior da barra de abas**, não o
`top: 76px` que o canvas usa. Aquele valor pressupõe a barra de título `[v2]` de
36 px somada à barra de abas; no v1 o default é `decorations = true`, a barra de
abas começa em `y = 0` e o valor fixo desenharia o popover flutuando sobre a
grade com uma folga arbitrária. Os 8 px são o mesmo respiro da pilha de avisos
(§2.14). Com `tab_bar_position = "bottom"`, o editor abre **acima** da barra, com
a mesma folga.

**Flip nos dois eixos**, como o menu de contexto (§2.16): a borda direita nunca
passa da janela, e se não couber abaixo da barra o popover vira para cima. Camada
**popover** do [ADR-0018](../adr/0018-composicao-de-frame.md), compartilhada com
o menu de contexto e o tooltip — e o editor **nunca** coexiste com o menu de
contexto ([ADR-0023](../adr/0023-editor-de-grupo.md)).

1. **Rótulo do grupo** — seção 10px uppercase `#5c646f` + input largura total, fundo `#0f1216`, borda `#333a45` (foco `#5ed3bc`), raio 5, 13px `#e4e8ee`, `padding: 7px 9px`, foco automático. Edição ao vivo: o nome muda na barra enquanto se digita.
2. **Cor** — seis swatches 28×28, raio 6, `gap: 8`, borda `2px`. O selecionado ganha anel `#eef2f4`; os demais, `transparent`.
3. **Divisor** `1px #2a2f38`.
4. **Ações** — "Colapsar/Expandir grupo" com chip de tecla à direita; "Desagrupar abas"; "Fechar grupo (N abas)" em `#e08585`, hover `#2e2224`.

O rótulo do botão alterna entre "Colapsar grupo" e "Expandir grupo" conforme o estado.

**Fecha em clique fora, `Esc` ou perda de foco da janela** — e não tem overlay: overlay é da camada modal e é a marca do diálogo destrutivo (§2.15). "Fechar grupo (N abas)" abre o diálogo de confirmação **por cima** do editor, que permanece atrás; cancelar volta para ele.

**Navegação por teclado.** `Tab` e `Shift+Tab` percorrem as três regiões — campo, faixa de swatches, lista de ações. Dentro da faixa e da lista, as setas movem o realce; `Enter` aciona o realçado. No campo, `Enter` confirma e fecha. `Esc` restaura o nome anterior e fecha, mesmo com a edição ao vivo tendo mudado a barra. Hover e foco por teclado são o **mesmo** realce `#242a33` e são mutuamente exclusivos, como em §2.16.

**Os seis swatches são a única superfície de cor do v1.** A entrada por valor hexadecimal do RF-2.10 fica diferida: quem quer outra cor a coloca na `palette` da config (RF-4.18). Ver [ADR-0023](../adr/0023-editor-de-grupo.md).

### 2.10.1 Campo de nome inline na pílula `[v1]`

Sem representação no canvas. É o que o RF-2.4 exige no nascimento do grupo — *"nome vazio, em modo de edição inline"* — e é o **mesmo componente** do input do item 1 acima, renderizado no lugar da pílula em vez de dentro do popover.

Largura 140 — o teto do nome da §2.4 —, ou o que couber se a barra estiver em overflow, no mesmo `min()` que o campo de rename da aba usa. Altura 30 e raio 6, os da pílula, para que **a pílula não mude de tamanho ao entrar em edição**: o swatch e o contador continuam onde estão, e só o nome vira campo. Fundo `#0f1216`, borda `1px #5ed3bc`, texto 12px `#e4e8ee`, `padding: 2px 6px`, foco automático.

Semântica idêntica à do editor: edição ao vivo, `Enter` confirma, `Esc` restaura o valor anterior. Nome vazio é válido e deixa o grupo como marcador colorido (RF-2.9). Ao criar o grupo, o valor anterior é a string vazia — `Esc` deixa o grupo criado e sem nome, e **não** desfaz a criação.

### 2.11 Paleta de comandos `[v2]`

Overlay `rgba(6,7,9,.55)`, `padding-top: 96px`. Modal 600 de largura, `max-height: 440`, raio 10, sombra `0 28px 70px rgba(0,0,0,.6)`, animação `pop .14s`.

**Cabeçalho** `padding: 13px 15px`, borda inferior `#262b34`, `gap: 10`: prompt `>` mono 13px `#5ed3bc`; input transparente 14.5px `#e6eaef` com placeholder "Ir para aba, grupo ou comando…"; chip `Esc` mono 9.5px sobre `#232830`.

**Resultados** `padding: 6`, `gap: 1`. Cada linha `padding: 9px 10px`, raio 6, `gap: 11`, hover `#262c35`; o primeiro resultado de uma busca ativa recebe fundo `#242a33`. Conteúdo: chip de tipo (mono 9px, largura fixa 38, centralizado, tingido `.14`), rótulo 13px `#dfe4ea`, dica 11.5px `#5c646f` truncada, tecla mono 9.5px `#5c646f`.

Tipos e cores: `aba` `#5ed3bc`, `grupo` `#a68cf0`, `ação` `#e0b060`.

Vazio: "Nada encontrado", `padding: 26`, centralizado, 13px `#5c646f`.

### 2.12 Painel de configurações `[v2]`

Overlay `rgba(6,7,9,.45)` alinhado à direita. Drawer largura 400, altura cheia, fundo `#171b21`, borda esquerda `#2a2f38`, animação `slidein .16s`.

Cabeçalho `padding: 16px 18px`, borda inferior `#23272f`: título 15px/500 `#e6eaef` e botão de fechar 24×24, raio 5, hover `#242a33`.

Corpo `padding: 18`, `gap: 24`. Três seções, cada uma com rótulo 10px uppercase `#5c646f` (`letter-spacing: .8px`):

- **Abas e grupos** — toggles: trilho 34×19, raio 10, `padding: 2`, ligado `#3f8f80` e desligado `#2a3038`; botão 15×15 circular `#f0f3f6`, `translateX(15px)` quando ligado, transição `.15s`. Ao lado, rótulo 12.5px `#d7dce3` e descrição 11px `#5c646f`.
- **Perfis instalados** — linhas `padding: 9px 11px`, raio 6, fundo `#1c2028`, borda `#262b34`: badge, nome, comando mono 10.5px truncado, sistema à direita.
- **Atalhos** — pares rótulo/tecla, chip mono 10.5px sobre `#1e232b` com borda `#2a2f38`, raio 4, `padding: 3px 7px`.

### 2.13 Tela de nova aba `[v2]`

Ocupa a área de terminal quando não há aba. Centralizada, `gap: 34`, `padding: 48`. Título "Abrir um novo terminal" 19px/500 `#dfe4ea` e subtítulo 13px `#6b737e`. Grade de três colunas de 232px, `gap: 10`: cards `padding: 14`, raio 8, fundo `#161a20`, borda `#252a33`; hover borda `#3b434f` e fundo `#1b2028`. Cada card: badge grande (mono 11px/500, raio 5, `padding: 6px 7px`), nome 13px `#dfe4ea`, comando mono 10.5px `#6b737e`.

### 2.14 Aviso do app `[v1]`

Definido em [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), que não tem representação no canvas: os valores abaixo reaproveitam tokens de popover, sem cor nova.

Empilhado no canto superior direito da área de conteúdo, sob a barra de abas. Largura 320, `padding: 11px 12px`, `gap: 8` entre avisos, no máximo **três** visíveis — o quarto substitui o mais antigo.

Fundo `#1a1e25`, borda `1px #2e343e`, raio 8 (sem sombra e sem `pop`, ver §2.10).

**O corpo é de uma linha**, truncado com reticências. Esta seção especificava até três linhas; `TextRun` é sempre uma linha, e quebrar por palavra é trabalho de `porecatu-ui` sobre o `TextMeasurer`. Fica como está: o truncamento em uma linha é o comportamento aprovado (ADR-0028 §4). Registro na seção 4.4.

Da esquerda para a direita: barra de severidade de 2px em altura cheia — erro `#ef8a8a`, aviso `#e0b060`, informação `#5ed3bc` —, depois título 12.5px/500 `#dfe4ea` e corpo 11px `#6b737e`. Botão de fechar 17×17, raio 4, `✕` 10px `#727a86`, hover fundo `#39404b` e ícone `#e4e8ee` — os mesmos do botão de fechar da aba.

Erro de config cita caminho, linha e chave em mono 10.5px `#6b737e`, para que a coordenada seja legível. O convite de integração de shell (RF-3.1) é o único com ação embutida: snippet copiável em mono 10.5px sobre `#12151a`, raio 3, mais um "não mostrar mais".

Erro e aviso persistem até dispensa; informação sai em 6 s. `Esc` dispensa o do topo.

A pilha fica a **10 px** da borda direita da área de conteúdo e **8 px** abaixo da barra de abas. Saída é o `pop` invertido, em `.13s`; quando um aviso sai, os de baixo sobem com a transição `.15s` da seção 1.10. **O temporizador da informação pausa no hover** — perder a mensagem enquanto se lê é o pior momento possível. Corpo longo trunca em três linhas com reticências: aviso não é documento.

Camada **aviso** do [ADR-0018](../adr/0018-composicao-de-frame.md): cobre o chrome, é coberto por menu, tooltip e diálogo.

**O que não vem para cá:** fato de uma aba só é escrito no grid dela — diretório inexistente (RF-3.10), código de saída (RF-1.3) —, marcado em `#5ed3bc` e nunca imitando prompt. A **posição** dentro do grid segue o momento do fato, e está decidida no [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md): primeira linha para o RF-3.10, que acontece na abertura da aba; **após a última linha de saída** para o RF-1.3, que acontece no fim — na primeira linha o código de saída já teria rolado para fora da vista.

### 2.15 Diálogo de confirmação `[v1]`

Overlay `rgba(6,7,9,.45)` sobre a janela. Modal largura 380, `padding: 16`, raio 10, fundo `#1a1e25`, borda `1px #2e343e` (sem sombra e sem `pop`, ver §2.10). O corpo é de uma linha, como no aviso (§2.14).

Título 13px/500 `#e6eaef`, corpo 12.5px `#d7dce3`, `gap: 14`. Dois botões à direita, `gap: 8`, altura 30, `padding: 0 12`, raio 5: **cancelar** com borda `1px #262b34` e texto `#d7dce3`; **confirmar destrutivo** em `#e08585` com hover de fundo `#2e2224`.

O foco inicial é o cancelar. `Enter` aciona o botão focado, `Esc` cancela.

**Anel de foco:** o botão focado leva borda `1px #5ed3bc` — o mesmo acento do campo de rename, que é o token que o projeto usa para dizer "as teclas vão para cá". Sem isso o RF-10.18 é inverificável: "o foco inicial é o cancelar" não é observável se o foco não tem aparência. **Hover do cancelar:** fundo `#262b34`, o mesmo dos botões de borda `#262b34` da barra.

O corpo é curto por construção nos três diálogos do v1, e **não rola**. Diálogo que precisasse de corpo longo seria aviso, não diálogo. Sem animação no overlay; só o modal tem `pop .14s`.

Camada **modal** do [ADR-0018](../adr/0018-composicao-de-frame.md), a mais alta: cobre tudo e suprime tooltip.

Usado por RF-1.6 (aba com programa de tela cheia, conforme o [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md)), RF-2.23 (fechar grupo, com a contagem no corpo) e RF-10.23, o fechamento de janela com mais de uma aba ([ADR-0015](../adr/0015-multiplas-janelas.md)).

### 2.16 Menu de contexto `[v1]`

Mesmos tokens do menu de perfis (2.9), que é `[v2]` — o menu de contexto é `[v1]` e reaproveita a definição.

Popover ancorado no cursor, largura mínima 200, fundo `#1a1e25`, borda `#2e343e`, raio 8, `padding: 6` (sem sombra e sem `pop`, ver §2.10). Vira nos dois eixos para caber no monitor da janela.

Itens: `padding: 7px 8px`, raio 5, `gap: 10`, texto 12.5px `#d7dce3`, hover `#242a33`. Chip de tecla à direita, mono 9.5px `#5c646f`. Divisor `1px #2a2f38` com `margin: 5px 4px`. Item destrutivo `#e08585`, hover `#2e2224`. **Item indisponível fica esmaecido em `#5c646f`, nunca ausente.**

Navegável por setas, `Enter` aciona, `Esc` fecha; clique fora ou perda de foco também fecham.

**Hover e foco por teclado são o mesmo estado visual `#242a33`, e são mutuamente exclusivos:** mover o mouse move o realce, as setas movem o realce e limpam o hover. Um realce por vez — dois destaques simultâneos deixam ambíguo o que o `Enter` vai acionar.

Largura máxima **320**, a do aviso; rótulo mais longo trunca com reticências. O menu **não rola**: as listas do v1 têm meia dúzia de itens, e quando não cabe na vertical o flip resolve. Deslocamento de 6 px do cursor até o canto do popover, e a animação `pop` nasce do canto ancorado — quando o menu vira para cima, o `translateY` inverte de sinal.

Camada **popover** do [ADR-0018](../adr/0018-composicao-de-frame.md), compartilhada com o tooltip da seção 2.20.

Três menus — aba (RF-1.1, RF-1.2, RF-2.20), grupo (RF-2.22) e terminal (F6). O menu do grupo e o editor de grupo (2.10) leem a **mesma** lista de ações, catalogada em [`docs/reference/acoes.md`](../reference/acoes.md).

### 2.17 Indicadores da aba `[v1]`

Sem representação no canvas — estava na seção 4.2. Os valores abaixo saem dos tokens da seção 1; as duas cores já existem no arquivo de exemplo (`activity_indicator`, `bell_indicator`) e vêm da semântica da seção 1.5.

Ponto circular **6×6, raio 50%** — o token do ponto de status do painel —, à esquerda do rótulo, com o `gap: 8` da aba. Fica na posição que o badge de perfil `[v2]` ocupará; quando o badge existir, os dois convivem com o mesmo `gap`.

| Estado | Cor |
|---|---|
| Atividade (RF-1.20) | `#86c56a` |
| Campainha (RF-1.21) | `#ef8a8a` |

**Um ponto só.** Atividade e campainha juntas mostram o de campainha: dois pontos consomem 14 px de uma aba que no piso tem 49 px de rótulo, e campainha é o fato mais raro e mais urgente dos dois.

**Não pisca.** Indicador animado em trinta abas de fundo é um frame por intervalo de piscada, contra a regra do [ADR-0007](../adr/0007-modelo-de-threading.md) de que terminal ocioso não gera frame. Presença é o sinal.

O ponto consome largura do rótulo — 6 px mais o `gap: 8` —, e o truncamento é recalculado com isso: a aba **não** muda de largura por causa do indicador. Ambos somem ao visitar a aba (RF-1.22) e são desligáveis na config. Aba no estado `Exited` do [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md) não exibe nenhum dos dois — não há mais saída possível.

### 2.18 Overflow da trilha `[v1]`

Sem representação no canvas — estava na seção 4.2. O canvas resolve a trilha com `overflow-x: auto`, que é a rolagem do navegador, não um desenho.

**Nada cede: a trilha rola.** Ao faltar espaço, nenhum elemento da aba encolhe —
rótulo no teto de 180 px de `max-width`, `padding: 0 6px 0 10px`, `gap: 8`, botão de
fechar 17×17 e ponto de 6 px, todos fixos —, e a trilha inteira rola como um
componente só. Os 41 px de cromo da aba seguem sendo a conta que dá o teto de 140 px
do nome da pílula (§2.4); o que eles não dão mais é piso de encolhimento.

Esta seção especificava uma **ordem de cedência** — encolhe o rótulo da aba até os
90 px de `min_width`, depois o nome da pílula até os 60 px de `label_min_width`, e só
então rola. Foi implementada assim na F2 e na F3, e **descartada na F3** por custo:
encolher exige busca binária sobre o layout inteiro (até 24 recálculos por frame,
cada um remedindo o texto de toda aba com `cosmic-text`, sem cache), e o custo cresce
justamente no caso que a motiva — muitas abas, barra em overflow. A barra parecia
travada ao trocar de aba. As duas chaves saíram do arquivo de exemplo. Registro na
seção 4.4.

O botão de fechar **não desaparece por falta de espaço**. `show_close_button` é escolha do usuário (RF-4.11), não degradação automática: aba cujo conteúdo muda conforme a largura é a mesma armadilha que o RF-10.20 evita no menu.

**A rolagem é um recorte só**, na camada de chrome do [ADR-0018](../adr/0018-composicao-de-frame.md); as abas fora da vista desaparecem pelo clip, sem lógica de visibilidade no layout. O que rola é a trilha (§2.2) — a zona fixa da direita fica parada.

- Gesto: roda do mouse sobre a barra rola a trilha na horizontal, com ou sem `Shift`. Passo de 90 px por notch — era a largura de uma aba no piso, e sobreviveu como valor de passo ao fim do encolhimento. Não reaproveita `scroll_multiplier`, que conta linhas do grid.
- Sem inércia e sem easing, pela mesma razão do indicador que não pisca: rolagem contínua é um frame por quadro de animação; rolagem discreta é um frame por evento.
- **Sem barra de rolagem desenhada.** Não há token de scrollbar, e 6 px numa barra de 52 px sairiam do rótulo. A affordance é o indicador abaixo.
- Trazer a aba ativa para a vista (RF-1.18) é **rolagem mínima**: alinha à borda esquerda da trilha se ela está à esquerda, à direita se está à direita. Nunca centraliza — centralizar move mais que o necessário e desorienta.

**Indicador de abas fora da vista (RF-1.19).** Nas duas pontas da trilha, por dentro
(4 px de recuo), desenhado sobre o clip e centrado na altura da barra. **Círculo de
18×18** — raio 9, a metade da largura —, fundo `#12151a` (o `count_background` da §1.2)
e só o chevron Lucide `chevron-left` / `chevron-right` em `#e4e8ee`, na em de 20 px.

**Sem contagem.** Esta seção especificava uma cápsula de 34×18 com o chevron mais o
número de abas ocultas, nos tokens do contador da pílula (§2.4) — pedido do usuário
para trocar por um círculo só com o chevron, porque a cápsula lia como "comprida"
demais para o que informa. Registro na seção 4.4.

Cada ponta só aparece se houver aba oculta naquele lado. Clique rola uma aba (90 px).

### 2.19 Arraste de aba `[v1]`

Sem representação no canvas — estava na seção 4.2. Cobre RF-1.15 e, desde a F3, o RF-1.16 (arraste entre grupos) e o RF-2.19 (arraste do grupo inteiro).

- **Limiar de 4 px** de movimento com o botão apertado — o `gap` entre abas. Abaixo disso o gesto é clique, e ativa a aba (RF-1.13). Sem limiar, todo clique com micro-tremor vira arraste.
- **Aba fantasma:** a aba arrastada continua desenhada, seguindo o cursor no eixo X e presa ao Y da barra. Não sai da barra: arrastar aba entre janelas não é gesto do v1 (RF-10.24). Hoje o fantasma sai no mesmo tom da aba parada; o `filter: brightness(1.18)` e a sombra que esta seção pede para separá-la da trilha estão nas duas mudanças aprovadas para a F4 (§1.7, §1.10).
- **O buraco é o marcador.** A posição de origem fica vazia, mostrando o fundo da barra `#1b1f26`, e as vizinhas deslizam com a transição `.15s` da seção 1.10. Não há caret de inserção separado: ele diria a mesma coisa que o buraco.
- **Auto-scroll:** cursor a menos de 30 px de uma ponta da trilha rola naquela direção, uma aba a cada `.15s`.
- `Esc` cancela e a aba volta à origem; soltar fora da trilha cancela também.
- Cursor `Grabbing` durante o arraste. É o único elemento do gesto que vem do sistema, e não há alternativa desenhável — o ponteiro não é superfície nossa.

**Realce da fronteira do grupo (RF-1.16).** O wrapper que receberia a aba é tingido na
cor do grupo com alfa **`.14`** — o `badge_tint_strength` do arquivo de exemplo; a
prosa desta seção citava `.16`, que arredondava o valor, e o TOML é a fonte numérica —
e ganha borda `1px` na cor do grupo com alfa `.45`. Nada de cor nova: é a própria cor
do grupo, em duas intensidades que o arquivo já contém. O realce entra **por cima** da
cápsula de cor cheia (§2.3), que é o fundo de repouso do wrapper.

O **run implícito** também recebe realce, e é justamente o caso que não pode
faltar: sem ele, "arrastar para fora de todos os grupos" (RF-1.16, segunda frase)
não teria feedback nenhum. Como o run implícito não tem cápsula (§2.3), o
realce dele usa o `ungrouped_color` `#7b838f` nas mesmas duas intensidades.

Um wrapper realçado por vez. O destino é o que a regra de fronteira do
[ADR-0021](../adr/0021-selecao-multipla-e-gestos-da-barra.md) resolve: o `gap`
entre wrappers pertence ao grupo da esquerda, e soltar sobre a pílula entra no
início do grupo. Soltar sobre grupo **colapsado** realça a pílula, não um wrapper
— não há trilha para realçar. O contador que incrementava ao soltar saiu da pílula (§2.4); o que confirma a chegada é a aba aparecer ao expandir.

### 2.19.1 Arraste do rótulo do grupo `[v1]`

Sem representação no canvas. Cobre RF-2.19: arrastar a pílula move o grupo
inteiro, com suas abas.

- **Mesmo limiar de 4 px** do arraste de aba, e mesmo cursor `Grabbing`.
- **O fantasma é a pílula sozinha**, não o wrapper com as N abas. Um wrapper de
  oito abas é mais largo que a janela e o fantasma cobriria a barra que o usuário
  precisa ver para escolher o destino. A pílula carrega a cor cheia do grupo e o nome
  (§2.4) — identidade suficiente para saber o que se está movendo.
- **O buraco é o marcador**, como no arraste de aba: o wrapper de origem colapsa
  para largura zero e os wrappers vizinhos deslizam, abrindo o vão onde o grupo
  vai cair. O destino é sempre uma **fronteira entre grupos**, nunca o interior de
  outro grupo — grupos não aninham ([ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md)).
- **Nenhum wrapper é realçado.** Realce significa "a aba entra aqui", e um grupo
  nunca entra em outro; o vão que se abre é o feedback.
- Auto-scroll, `Esc` e soltar fora da trilha: idênticos ao arraste de aba.

### 2.20 Tooltip `[v1]`

Definido em [ADR-0019](../adr/0019-tooltip.md), que também não tem representação no canvas: os valores reaproveitam tokens de popover, sem cor nova.

Aparece **só quando o texto do alvo foi truncado**, após 600 ms de hover parado. Uma linha, largura máxima 320 — a do aviso —, texto além disso truncado com reticências.

Fundo `#1a1e25`, borda `1px #2e343e`, raio **6** (não o 8 de popover: num retângulo de uma linha o 8 pesa, e o 6 é a classe dos elementos de altura de aba da barra). Texto 11px `#d7dce3`, `padding: 7px 8px` — o espaçamento do item de menu. A borda de 1px é o que separa o tooltip do fundo hoje; a sombra em camadas (§1.7) entra na F4 junto com a dos outros quatro widgets, e a animação `pop .13s` não entra (§2.10).

Ancorado no **alvo**, não no cursor: abaixo dele, alinhado à borda esquerda, a 6 px. Vira nos dois eixos para caber no monitor da janela. Some ao sair do alvo, clicar, digitar, começar arraste, a janela perder foco ou o alvo deixar de existir.

Não recebe foco de teclado, **não participa do hit-testing** — o ponteiro atravessa — e não carrega ação. Diálogo aberto suprime tooltip.

---

## 3. Tabela de fases

Todo elemento do design, classificado. **Nada aqui fica sem etiqueta.**

| Elemento | Fase | Governado por |
|---|---|---|
| Barra de abas (trilha, rolagem, overflow) | `[v1]` | [PRD-001](../prd/prd-001-abas.md) |
| Aba: estados, borda, rótulo, truncamento | `[v1]` | PRD-001, [PRD-004](../prd/prd-004-aparencia-do-chrome.md) |
| Botão de fechar da aba | `[v1]` | PRD-001 RF-1.2, PRD-004 RF-4.11 |
| Campo de rename inline | `[v1]` | PRD-001 RF-1.8 |
| Botão de nova aba | `[v1]` | PRD-001 RF-1.1, PRD-004 RF-4.11 |
| Cápsula de grupo (cor cheia, vidro, sombra) | `[v1]` | [PRD-002](../prd/prd-002-grupos-de-abas.md), PRD-004 RF-4.19 |
| Pílula: nome e caret, na cor cheia do grupo | `[v1]` | PRD-002 RF-2.9 a RF-2.13, RF-2.17 |
| Indicador de grupo: pílula e cápsula | `[v1]` | PRD-004 RF-4.14 — forma única, sem chave de estilo ([ADR-0032](../adr/0032-interface-do-v1-fechada.md)) |
| Grupo colapsado (só pílula) | `[v1]` | PRD-002 RF-2.13 |
| Abas sem grupo (wrapper sem pílula) | `[v1]` | [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md) |
| Editor de grupo: nome, swatches, ações | `[v1]` | PRD-002 RF-2.9 a RF-2.11, RF-2.22 |
| Área de terminal (quadro arredondado), prompt, cursor | `[v1]` | [PRD-005](../prd/prd-005-aparencia-do-terminal.md) |
| Paleta de cores de grupo | `[v1]` | PRD-004 RF-4.18 |
| Tema (fontes, superfícies, cores) | `[v1]` | PRD-004, PRD-005 |
| Aviso do app (empilhado, com severidade) | `[v1]` | [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md) — sem representação no canvas |
| Nota na aba (escrita no grid) | `[v1]` | ADR-0014, [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md); PRD-003 RF-3.10, PRD-001 RF-1.3 |
| Diálogo de confirmação | `[v1]` | ADR-0014; PRD-001 RF-1.6, PRD-002 RF-2.23 |
| Menu de contexto de aba e de grupo | `[v1]` | ADR-0014; PRD-001 RF-1.1, PRD-002 RF-2.22 |
| Indicadores de atividade e campainha na aba | `[v1]` | PRD-001 RF-1.20 a RF-1.22, PRD-004 RF-4.8 — sem representação no canvas |
| Overflow da trilha e indicador de abas fora da vista | `[v1]` | PRD-001 RF-1.18, RF-1.19 — sem representação no canvas |
| Arraste de aba: fantasma, buraco, auto-scroll | `[v1]` | PRD-001 RF-1.15 — sem representação no canvas |
| Tooltip de texto truncado | `[v1]` | [ADR-0019](../adr/0019-tooltip.md); PRD-001 RF-1.10, PRD-002 RF-2.12 — sem representação no canvas |
| Aba selecionada (seleção múltipla) | `[v1]` | PRD-002 RF-2.2, PRD-004 RF-4.7; [ADR-0021](../adr/0021-selecao-multipla-e-gestos-da-barra.md) — sem representação no canvas |
| Campo de nome inline na pílula | `[v1]` | PRD-002 RF-2.4, RF-2.9; [ADR-0023](../adr/0023-editor-de-grupo.md) — sem representação no canvas |
| Indicador agregado de grupo colapsado | `[v1]` | PRD-002 RF-2.16 — sem representação no canvas |
| Realce da fronteira do grupo no arraste | `[v1]` | PRD-001 RF-1.16, PRD-002 RF-2.18 — sem representação no canvas |
| Arraste do rótulo do grupo | `[v1]` | PRD-002 RF-2.19 — sem representação no canvas |
| Popover de grupo de destino | `[v1]` | PRD-002 RF-2.20; ADR-0023 — sem representação no canvas |
| Reordenação animada ao formar grupo | `[v1]` | PRD-002 RF-2.5; [ADR-0022](../adr/0022-animacao-de-interface.md) — sem representação no canvas |
| Controles de janela e resize sem decoração nativa | `[v1]` | [ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md) — sem PRD, sem representação no canvas |
| Botão de configurações da zona fixa (inerte até a F4) | `[v1]` | PRD-004 — ver §2.2 |
| Ícone da janela e do executável | fora de fase | estava listado em F6 no roadmap; entregue antes, ver o roadmap |
| **Painéis divididos** | `[v2]` | [PRD-006](../prd/prd-006-paineis-divididos.md) *(rascunho)* |
| **Cabeçalho e botões do painel** | `[v2]` | PRD-006 *(rascunho)* |
| **Perfis de aba e menu de perfis** | `[v2]` | [PRD-007](../prd/prd-007-perfis-de-aba.md) *(rascunho)* |
| **Badge de perfil na aba** | `[v2]` | PRD-007 *(rascunho)*, PRD-004 RF-4.23 |
| **Tela de nova aba** | `[v2]` | PRD-007 *(rascunho)* |
| **Paleta de comandos e botão de busca** | `[v2]` | [PRD-008](../prd/prd-008-paleta-de-comandos.md) *(rascunho)* |
| **Barra de status** | `[v2]` | [PRD-009](../prd/prd-009-barra-de-status.md) *(rascunho)* |
| **Painel de configurações GUI** | `[v2]` | [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md) — sem PRD |
| **Faixa de identidade da barra de título** (logo, nome do app, título da aba ativa) | `[v2]` | ADR-0009 (parcial; controles de janela e resize já são `[v1]`, ver [ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md) e §2.2.1) — sem PRD |

---

## 4. Rastreabilidade

### 4.1 Requisitos do v1 que o design cobre

| Requisito | Onde aparece |
|---|---|
| PRD-001 RF-1.7, RF-1.10 (título, truncamento) | rótulo da aba, 180px de teto **e** de piso (largura de aba fixa) |
| PRD-001 RF-1.8 (rename inline) | campo com borda de acento sobre a aba |
| PRD-001 RF-1.14 (aba ativa inequívoca) | fundo, borda **e** cor de texto mudam juntos — não só matiz |
| PRD-002 RF-2.9 (nome do grupo) | pílula + input do editor |
| PRD-002 RF-2.10, RF-2.11 (cor e indicador) | swatches do editor, cápsula de cor cheia, pílula na cor do grupo |
| PRD-002 RF-2.13 (colapso) | troca de ícone do caret, abas ocultas, cápsula abraçando a pílula |
| PRD-002 RF-2.22, RF-2.23 (ações e confirmação) | editor: colapsar, desagrupar, "Fechar grupo (N abas)" |
| PRD-004 RF-4.18 (paleta) | seis swatches |
| PRD-004 RF-4.19 (tingimento) | cápsula na cor cheia a `.85` (`tint_strength` default `1.0`) |
| PRD-005 (cores do terminal) | seis cores semânticas de saída |
| ADR-0006 (grupo implícito) | wrapper sem pílula das abas soltas |

### 4.2 Requisitos do v1 **sem** representação no design

Precisam de decisão de desenho na implementação. Listados para não passarem batido:

| Requisito | O que falta |
|---|---|
| PRD-003 RF-3.9 | aba restaurada ainda sem shell iniciado (F5) |

Enquanto não houver desenho aprovado para esses, valem os tokens da seção 1 e o julgamento de quem implementa — nunca cores ou dimensões novas fora da tabela.

> **Quarta rodada, com o [ADR-0028](../adr/0028-o-binario-como-referencia-visual.md).** O RF-5.14 (cores de seleção de texto no terminal) saiu desta lista: o valor da seção 1.5 nasceu do `::selection` do canvas, e é o que o binário desenha — logo, é o valor deliberado, e não há desenho faltando. O que falta é a chave de configuração, que é trabalho normal da F4.

> **Quinta rodada, com o [ADR-0032](../adr/0032-interface-do-v1-fechada.md).** Saiu o RF-4.14, e com ele o último item desta lista que falava do chrome **atual**: `left-bar` e `outline` deixaram de ser escopo do v1, em vez de ganharem anatomia. Sobra um item, e ele é de **recurso** que ainda não existe — a aba restaurada da F5 —, não de desenho pendente na interface de hoje. É a diferença que o ADR-0032 §2 fixa: falta de comportamento se registra; falta de aparência não existe.

**Resolvidos depois da primeira versão desta lista.** O RF-4.21 (como o erro de configuração é exibido) estava aqui e saiu: o [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md) definiu a superfície de aviso, e a anatomia está na seção 2.14. O mesmo ADR cobriu sete requisitos que não constavam nem desta lista nem da 4.1 — RF-1.6, RF-2.23, RF-3.1, RF-3.10, RF-3.14, RF-3.16 e RF-5.8 —, além do menu de contexto exigido por RF-1.1, RF-1.2, RF-2.20 e RF-2.22. Nenhum deles introduziu cor nova: todos saem dos tokens de popover da seção 1.

**Terceira rodada, ao abrir a F3.** A lista ficou com três itens. Saíram os quatro que eram da F3: o realce de fronteira de grupo do RF-1.16 (seção 2.19), a aba selecionada do RF-2.2 (seção 2.5, onde é modificador de borda e não um quarto estado), a animação de reordenação do RF-2.5 (token `reflow` na seção 1.10, sob a decisão do [ADR-0022](../adr/0022-animacao-de-interface.md)) e o indicador agregado do RF-2.16 (seção 2.4). Entraram três requisitos que **não constavam de nenhuma das duas listas** — mesma descoberta que o RF-1.10 e o tooltip foram na rodada anterior: o RF-2.19 pede arrastar o grupo inteiro e a seção 2.19 só cobria arraste de aba (agora 2.19.1); o RF-2.4 pede edição de nome inline no nascimento do grupo e os valores do campo eram só do editor (agora 2.10.1); e o RF-2.12 pede truncamento do nome do grupo, que não tinha teto nem piso (agora na seção 2.4). Também aqui, **nenhuma cor nova**: a borda da aba selecionada é o acento que o campo de rename já usa, o realce de fronteira é a própria cor do grupo em duas intensidades que o arquivo de exemplo já contém, e o indicador agregado reusa as duas cores da seção 2.17.

**Segunda rodada, ao abrir a F2.** Saíram desta lista os RF-1.20 e RF-1.21 (seção 2.17), o RF-1.19 (seção 2.18) e o RF-1.15 (seção 2.19); do RF-1.16 sobrou só o realce de fronteira de grupo, que é da F3. Entrou um requisito que **não constava de nenhuma das duas listas**: o RF-1.10 pede tooltip para o título truncado, e o ADR-0014 havia decidido três widgets de chrome quando o v1 precisa de quatro — o [ADR-0019](../adr/0019-tooltip.md) fecha isso, e a anatomia está na seção 2.20. Também aqui, nenhuma cor nova.

O comportamento da seleção de texto — gesto, semântica de palavra, recorte de espaço, remontagem de linha quebrada — está no [ADR-0013](../adr/0013-mouse-selecao-e-clipboard.md); o que a nota do RF-5.14 acima registra é só a origem da **cor**.

### 4.3 Elementos do design **sem** requisito no v1

Todos `[v2]`, todos endereçados na tabela de fases: painéis divididos, perfis e badge, tela de nova aba, paleta de comandos, barra de status, painel de configurações, faixa de identidade da barra de título.

### 4.4 Histórico de decisões visuais

Cada linha é uma decisão **já tomada e já em vigor**: o que o desenho original pedia, o que o binário faz, e por quê. Desde o [ADR-0028](../adr/0028-o-binario-como-referencia-visual.md) esta seção é **histórico, não lista de tarefas** — nada aqui autoriza mudar a interface, e as seções 1 e 2 acima já descrevem o estado real. Ela existe para que ninguém "corrija" de volta um valor que foi decidido de propósito.

A coluna **Onde** diz em que fase (ou por qual ADR) a decisão foi tomada.

| O que o desenho pedia | O que vale, e por quê | Onde |
|---|---|---|
| Design combina pílula **e** sublinhado; PRD-004 modelava enum exclusivo | `indicator_style` virou lista combinável, default `["pill", "underline"]` — e **a chave depois saiu por inteiro**: o indicador é a pílula mais a cápsula, forma única (linha do ADR-0032, abaixo) | ADR-0009, superado pelo [ADR-0032](../adr/0032-interface-do-v1-fechada.md) |
| Design usa `Ctrl+T`, `Ctrl+G`, `Ctrl+,`, `Ctrl+1..6` | ADR-0008 vence: nada de `Ctrl+<letra>` sozinho. Chips de tecla do mockup são ilustrativos | ADR-0009 |
| `Ctrl+Shift+P` é paleta no design, `theme.cycle` no ADR-0008 | Paleta fica com `Ctrl+Shift+P`; `theme.cycle` migra para `Ctrl+Shift+Y` | ADR-0009 |
| Design tem configurações por GUI; ADR-0003 decidiu só TOML | Painel é `[v2]` e, quando existir, **escreve no TOML** — o arquivo continua sendo a única fonte de verdade | ADR-0009 |
| Design tem barra de título própria; default era `decorations = true` | A faixa de identidade (logo, nome do app, título da aba) segue `[v2]`. Os controles de janela e o resize, não: saem da barra de abas existente, sem decoração nativa fora do macOS — pedido do usuário, fora de fase | ADR-0009, [ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md) |
| Design diz "guias"; docs dizem "abas" | Projeto padroniza **"abas"**; rótulos do mockup ajustados | ADR-0009 |
| Paleta do design tem 6 cores; exemplo tinha 8 | Seis cores do design viram a paleta padrão | ADR-0009 |
| Fontes e cores do design vs. catppuccin/JetBrains | Design vira o default; catppuccin sobra como tema nomeado | ADR-0009 |
| `filter: brightness()` no hover da aba e da pílula | `porecatu-render` não tem primitiva de filtro, e a solução é **em CPU** dentro de `porecatu-ui`: multiplicar os canais da cor e clampar dá o mesmo resultado, sem primitiva nova. Hoje o hover existe como hit-test e alimenta o tooltip, sem mudar aparência — exceto nos botões de janela do ADR-0027. **Aprovado para a F4**, e é a outra das duas do ADR-0032 §2 | F2, aprovado para F4 |
| Sombra de popover (`0 18px 44px rgba(0,0,0,.55)`) nos widgets de chrome, no fantasma de arraste e na janela | **Há sombra, em camadas, e em três lugares.** `porecatu-render` não tem primitiva de sombra nem passo de blur, então `chrome::push_shadow` empilha três `RoundedQuad` pretos (spread 1/2.5/4.5, alfa `.16`/`.10`/`.06`, offset Y 1/2/3) — pedido do usuário, aplicado à **cápsula de grupo**, à **aba solta** e ao **quadro do terminal** (§1.7). Nos **cinco widgets de chrome e no fantasma de arraste** a mesma técnica entra na F4 — uma das **duas** mudanças visuais que o [ADR-0032](../adr/0032-interface-do-v1-fechada.md) §2 deixa em aberto no v1, e a lista é exaustiva. A mancha grande do CSS não é reproduzível assim: aliaseia em anéis visíveis | F3 e depois, aprovado para F4 |
| Corpo do aviso e do diálogo em **três linhas** com reticências | **Uma linha, truncada.** `TextRun` é sempre uma linha, e quebrar por palavra é trabalho de `porecatu-ui` sobre o `TextMeasurer` do [ADR-0018](../adr/0018-composicao-de-frame.md). **Decidido não fazer** (ADR-0028 §4): o truncamento em uma linha é o comportamento aprovado, e mudar a altura desses dois widgets mudaria a interface sem ninguém ter pedido | F2, fechado no ADR-0028 |
| Auto-scroll do arraste a uma aba a cada `.15s` | Rola **por evento de `CursorMoved`** dentro da zona de 30 px, não por intervalo. O relógio do [ADR-0022](../adr/0022-animacao-de-interface.md) tornaria o intervalo possível, mas o gesto atual funciona e mudá-lo mudaria a sensação do arraste: **decidido não fazer** (ADR-0028 §4) | F2, fechado no ADR-0028 |
| Caret da pílula especifica `rotate(0deg)` / `rotate(90deg)` | Não há transformação afim em `porecatu-render`, e o caret é um glifo. A rotação vira **troca de ícone** — `chevron-right` / `chevron-down` do Lucide, no lugar dos `▶`/`▼` da prosa (ver a linha da face de ícones abaixo); o que anima nos `.15s` é o resto do colapso | ADR-0022, [ADR-0024](../adr/0024-face-de-icones.md) |
| Editor de grupo em `top: 76px` | Aquele valor pressupõe a barra de título `[v2]`. No v1, 8 px abaixo da barra de abas, com flip nos dois eixos — ver seção 2.10 | [ADR-0023](../adr/0023-editor-de-grupo.md) |
| Menu de contexto **não rola** (seção 2.16) | Vale para listas de ação, cujo tamanho é conhecido em tempo de escrita. O popover de grupo de destino do RF-2.20 **rola**, porque a lista é do tamanho do número de grupos do usuário | ADR-0023 |
| RF-2.10 permite cor por valor hexadecimal direto | O editor tem seis swatches e nada mais. A entrada por hex fica **diferida**: quem quer outra cor a coloca na `palette` da config (RF-4.18) | ADR-0023 |
| Wrapper de grupo tingido com alfa `.07`, e transparente quando colapsado (§2.3, RF-4.19) | Atrás do fundo **opaco** das abas, 7% da cor não se vê. O wrapper passa a ser **cápsula de cor cheia** e o fundo da aba ganha alfa `.85` (§2.3, §2.5). E ela é desenhada **também colapsada**, abraçando a pílula sozinha: é a cápsula que diz de que cor o grupo é, e sumir com ela tirava a única marca de cor justo quando o nome do grupo é tudo o que resta na barra. Pedido direto do usuário; `tint_strength` continua no arquivo e volta a governar o valor na F4 | F3 |
| Um botão de nova aba só, ao final da trilha rolável (§2.6) | **Um por grupo** (`group.new_tab`, inclusive no run implícito), escondido quando o grupo está colapsado, **mais um ao fim da trilha** que cria aba fora de todo grupo — este só quando o último grupo é explícito, senão duplicaria o "+" do run solto. Houve um botão **global** numa zona fixa à direita (§2.2), removido por ser um segundo botão para a mesma ação a um palmo do primeiro. `show_new_tab_button` governa os dois que restaram. Pedido do usuário | F3 |
| A zona fixa à direita da barra é do botão de nova aba (§2.2) | É do **botão de configurações**, que herdou o bloco quando o botão de nova aba global saiu — e que está **inerte**: desenha, consome o clique para ele não atravessar até a aba de baixo, e não faz nada (`config` é F4). O bloco fica reservado para o que a barra ganhar à direita daqui em diante. Pedido do usuário | F3 |
| Um tom só para o ícone do "+" (§2.6) | A cor depende do que está **atrás** dele: `#12151a` sobre a cápsula de cor cheia de um grupo explícito, `#e4e8ee` sobre a barra escura de um run de abas soltas. Com um tom só, o "+" ficava preto no fundo preto sempre que a barra não tinha grupo nenhum | F3 |
| Ordem de cedência do overflow: rótulo, depois nome da pílula, só então rolagem (§2.18, §2.4) | **Nada cede.** Encolher exige busca binária sobre o layout (até 24 recálculos por frame, cada um remedindo texto sem cache) e era a lentidão da barra em overflow. Rótulo e nome ficam no teto e a trilha rola; `min_width` e `label_min_width` saem do arquivo de exemplo | F3 |
| Interpolação do `reflow` descrita como deslocamento horizontal (§1.10, ADR-0022) | Deslocar só a posição faz os **vizinhos** parecerem suaves e o grupo tocado saltar. A cápsula interpola **posição e largura**, e as abas que entram ou saem da trilha ao expandir/colapsar interpolam **opacidade** — leitura literal do §2.4 ("o que anima de fato é o resto do colapso: as abas desaparecendo da trilha"). Nenhuma animação nova: mesmo relógio, mesmos dois consumidores | F3 |
| Ícones do chrome em cinza médio em repouso, `#e4e8ee` só no hover (§2.5, §2.6, §2.4) | **`#e4e8ee` em repouso.** O traço do Lucide é fino (`2/24` da em) e, num cinza médio contra a barra `#1b1f26`, some. O tom de hover vira o tom de base — nenhuma cor nova, é o token da própria espec. promovido de estado. Vale para fechar da aba, fechar do aviso, chevron de overflow e o "+" global. **Exceção:** o caret da pílula e o "+" do grupo caem sobre a cor cheia do grupo, não sobre a barra escura — `#e4e8ee` ali perde contraste, e os dois usam `#12151a` (`count_background`, linha abaixo) | F3 |
| Botões de ícone quadrados (§1.7: fechar 17×17; §2.6: "+" 30×30) | Ganham **respiro horizontal** (`icon_button_padding_x`, 4px de cada lado): ficam mais largos que altos, com o desenho ainda centrado no quadrado original. Vale para fechar da aba, os dois "+", o caret da pílula e o botão da zona fixa. A altura não muda — senão o botão de fechar deixaria de caber na aba. Pedido do usuário, sem origem na espec. | F3 |
| Nome do grupo em 12px/500 (§2.4, `label_font_size`) contra o rótulo da aba em 12.5px/400 (§1.1, §2.5) | Tamanho igual ao da aba (**13px** nos dois, mesma razão de sempre: meio pixel de diferença lê como fonte trocada, não como hierarquia). O **peso** diverge: aba em 400, nome do grupo em **500 (`Medium`), pedido do usuário para o rótulo do grupo ler como bold** — reverte a igualação de peso que a F3 tinha feito. `label_font_size` continua no arquivo e volta a governar o valor na F4 | F3, revisto |
| Famílias de texto do mockup e da tabela de tokens (IBM Plex Sans/Mono) | **Iosevka Fixed**, uma família só, no binário ([ADR-0026](../adr/0026-chrome-unificado-em-iosevka-fixed.md), que supersede o ADR-0025 nisso). A F3 tinha trocado IBM Plex Sans/Mono por Iosevka Aile/Fixed — duas variantes desenhadas diferente dentro da mesma superfamília —, e lado a lado na barra a diferença de desenho lia como duas fontes, pedido do usuário para unificar. A Iosevka é bem mais estreita que a Plex, então a célula do terminal encolhe e cabe mais coluna na mesma largura — o mockup não foi regerado. Dimensões da barra em px não mudam; o que muda é a métrica de texto dentro delas | [ADR-0026](../adr/0026-chrome-unificado-em-iosevka-fixed.md) |
| Swatch de cor 8×8 na pílula, fundo `#1f242c` neutro em volta, borda `1px #2b313b` (§2.4, `swatch_size`/`label_background`/`label_border`) | **Sem swatch, sem borda.** A pílula inteira (fundo, não só um quadrado) é pintada com a cor cheia do grupo — a mesma cápsula do wrapper por trás dela, então o quadrado virou redundante e a borda neutra virou um contorno cinza sem função sobre a cor cheia. Nome e caret passam de `#c3cad3`/`#e4e8ee` para **`#12151a`** (`count_background`), o mesmo escuro do "+" do grupo: sobre a cor cheia o claro perde contraste. `label_background`/`label_border`/`label_foreground`/`caret_foreground`/`swatch_size`/`swatch_corner_radius` saem do arquivo de exemplo. `label_padding_left` sobe de 8 para **10**. Tudo pedido do usuário | F3, revisto |
| Ícones do chrome escritos como glyphs Unicode: `✕ 10px` (§2.5, §2.15), `▶ 8px` (§2.4), chevrons de overflow (§2.18) | **Nenhum deles desenhava.** U+2715, U+25B6 e U+25BC não estão na IBM Plex Sans e o `fontdb` do projeto não carrega fonte do sistema ([ADR-0016](../adr/0016-fontes-embutidas.md)) — sem fallback, sem desenho. Entra uma **face de ícones** (Lucide, ISC): o binário desenha o ícone equivalente do catálogo, não aquele codepoint. Tamanhos e caixas da especificação valem sem mudança | [ADR-0024](../adr/0024-face-de-icones.md) |
| Tamanhos de ícone da §2.5/§2.6/§2.4/§2.18 (`✕ 10px`, `+ 15px`, `▶ 8px`, `chevron 10px`) | Lidos como tamanho de **desenho**, e o binário desenha **maior** que eles: a em dos ícones é 20 px (`chrome::ICON_EM_SIZE`), o que dá ✕ de 11.8 px. Nos tamanhos da prosa o traço do Lucide (`2/24` da em) fica em 0.83 px, e o antialiasing o mistura com o fundo — o ícone some, sem a cor ter mudado. Pedido do usuário depois de ver os tamanhos originais em tela | [ADR-0024](../adr/0024-face-de-icones.md) |
| Sublinhado de 2px na cor do grupo na base de cada aba (§2.5, `indicator_style = ["pill", "underline"]`, RF-4.19) | **Removido.** Ele existia para dizer a que grupo a aba pertence quando a pílula sai da vista; desde que a cápsula passou a ser pintada com a cor cheia (linha acima), a cor do grupo já está atrás da aba inteira e o traço virou ruído na base dela. Pedido do usuário. Não volta por configuração: a chave que o governaria saiu (linha abaixo) | F3, fechado no ADR-0032 |
| Quatro estilos de indicador de grupo numa lista combinável — `pill`, `underline`, `left-bar`, `outline` (§2.3, §2.5, RF-4.14) | **Um só: a pílula mais a cápsula.** `underline` foi desenhado e removido; `left-bar` e `outline` nunca tiveram anatomia e saíram de escopo — nasceram como alternativas ao wrapper tingido a `.07` e, contra a cápsula de cor cheia, seriam versões mais discretas de uma marca que o produto quer forte. `indicator_style` e `indicator_thickness` saem do arquivo de exemplo; a espessura de 2px que a segunda carregava é a da borda da aba, que já tem chave própria. Os dois estilos eram **desenháveis** hoje (um `Quad` fino e um `RoundedQuad` sem preenchimento) — a recusa é de produto, não de capacidade | [ADR-0032](../adr/0032-interface-do-v1-fechada.md) |
| Borda da aba de 1px (§2.5, §1.7) | **2px**, em todo estado — 1px do `#22262e` da aba inativa não se lê contra a cápsula de cor cheia. Espessura reaproveitada de `indicator_thickness`/`selected_border_width`, não inventada; cada estado mantém a cor dele. A seleção deixa de se distinguir por espessura e passa a se distinguir só pelo verde-água do token. Pedido do usuário | F3 |
| Aba de 30px de altura, trilha colada nas bordas da barra (§2.2, §2.5) | Aba de **34px** e trilha com **6px** de respiro nos quatro lados (`trilha_padding`) — os mesmos 6px que a §2.5 já dá à barra ("aba h30 + padding 6px da barra") e que a implementação nunca teve. A barra vai de 36 para **52px**, acima dos 42 que a §2.5 e o `height` do arquivo de exemplo declaram, porque a aba cresceu junto. Pedidos do usuário, calibrados em tela; os dois números ficam num lugar só | F3 |
| Aba tem a mesma altura dentro e fora de grupo (§2.3, §2.5: o wrapper apenas envolve abas de altura `tab_height`) | **Aba solta é mais alta.** Dentro de um grupo ela cede `wrapper_padding` acima e abaixo para o bloco do grupo; solta não há bloco a que ceder, então ela ocupa a caixa inteira do wrapper (`tab_height + wrapper_padding * 2`). Efeito: agrupada e solta alinham topo e base na barra, em vez de a solta parecer encolhida no meio de um vão vazio. Pedido do usuário | F3 |
| Largura da aba acompanha o conteúdo até `max_width` (§2.5, §1.7) | **Largura fixa**, igual para toda aba: `padding_left + label_max_width + internal_gap + close_button_size + padding_right`, saturada em `max_width` — os mesmos tokens da §2.5, com o teto de 180 px do rótulo virando também o piso. Título, indicador e renomeação deixam de refluir a trilha. Pedido do usuário; `max_width` continua no arquivo, agora como saturação, e o rótulo continua truncando com reticências | F3 |
| Botão de nova aba global cria a aba "onde o usuário está" (§2.6 não distingue os dois botões) | O **global** cria aba **fora de qualquer grupo explícito**, no fim da barra; o de dentro do grupo continua sendo `group.new_tab`. Seguir o grupo da aba ativa (`tab.new`, RF-1.1, [ADR-0020](../adr/0020-grupos-explicitos.md) §1) deixava o botão global sem jeito de criar aba desagrupada. O atalho `tab.new` **não** muda: continua no grupo da aba ativa | F3 |
| Cápsula e pílula chapadas, na cor cheia opaca | **Efeito de vidro.** A cápsula é pintada a `.85` da cor cheia e a pílula a `.92`, as duas com um rim translúcido de 1px em branco a `.16` (§1.2, §1.8) — pedido do usuário, "aparência chapada demais". Sem primitiva de blur em `porecatu-render`, não há como turvar o que passa por trás, só deixar passar menos dele; ainda assim lê como painel translúcido. Custo zero de render: troca de cor e alfa nos quads que já eram desenhados. A borda de 1px da pílula, que a F3 tinha removido por ser um contorno cinza sem função, **volta** aqui com propósito diferente — é o rim que lê como vidro | pós-F3 |
| Wrapper de grupo sem borda e sem sombra; aba com fundo e borda e nada mais (§2.3, §2.5) | **Cápsula e aba solta ganham sombra em camadas e borda de 1px**; a aba dentro de um grupo fica só com a borda, porque a cápsula carrega a sombra por ela. Pedido do usuário. É o mesmo `push_shadow` da linha da sombra, acima | pós-F3 |
| Área de terminal como retângulo pleno, colado nas bordas da janela (§2.7) | **Quadro arredondado**: raio 6, 6px de margem da borda da janela nos três lados que não encostam na barra, 6px de padding interno até a grade, e a sombra em camadas por baixo. Em cima ele encosta em `bar_height` sem gap — um vão ali desenha uma linha entre a trilha e o terminal, que o usuário pediu para eliminar. Nada de número novo: a margem é o `trilha_padding` e o padding é o `wrapper_padding` dobrado (§1.7). Pedido do usuário | pós-F3 |
| Indicador de overflow: chevron **mais a contagem** de abas ocultas, em cápsula de 34×18 (§2.18, §2.4) | **Círculo de 18×18, só o chevron.** A cápsula lia como "comprida" demais para o que informa, e a contagem saiu com ela — o pedido do usuário foi por um círculo. O fundo continua o `count_background` `#12151a` e o raio é a metade da largura (§1.7) | pós-F3 |
| `wrapper_padding` em todo wrapper, inclusive no run implícito (§2.3) | **Só onde há cápsula.** Sem cápsula não há o que absorva o respiro, e aplicá-lo abria entre um grupo e as abas soltas ao lado um vão maior — e sem cor — do que entre dois grupos. O eixo vertical já tinha essa exceção (a linha "aba solta é mais alta", acima); esta fecha o horizontal | pós-F3 |
| Contador de abas na pílula, mono 10px sobre `#12151a`, raio 9 (§2.4, item 3) | **Removido**, por pedido do usuário. `show_tab_count_when_collapsed` (RF-4.17) fica no arquivo de exemplo, e os tokens `count_*` continuam em uso noutros lugares: o escuro virou a cor de nome, caret e "+" sobre a cápsula, e o fundo do indicador de overflow (§2.18). O indicador **agregado** de grupo colapsado (RF-2.16) não foi afetado — ele é o ponto de 6×6, não o contador | pós-F3 |
| Botão de nova aba de 30×30 (§2.6, §1.7) | **17×17 de desenho, 25×17 de alvo** — o mesmo tamanho do botão de fechar da aba, que é o vizinho dele dentro da mesma trilha; os 30×30 eram do botão global, que saiu. O botão da zona fixa à direita herdou aqueles 30×30 (§2.2) | pós-F3 |
| Barra de abas com borda inferior `#23272f` (§2.2) | **Não é pintada.** O separador de 1px na base virava a linha contra o quadro do terminal (§2.7) que o usuário pediu para tirar. A cor fica registrada na §1.3: com `tab_bar_position = "bottom"` (RF-4.1) o separador muda de aresta e volta a fazer sentido | pós-F3 |

**Nada nesta seção é pendência.** Ela era, até o [ADR-0028](../adr/0028-o-binario-como-referencia-visual.md), uma lista de dívida a cobrar no critério de saída da F4 — *"o binário com a config padrão bate com o mockup"*. Esse critério inverteu: **a configuração padrão reproduz o binário**, e as seções 1 e 2 já foram reescritas para descrever o que ele desenha.

Das quatro linhas que eram dívida de primitiva, duas foram **aprovadas para a F4** (hover por brilho e sombra nos cinco widgets e no fantasma de arraste) e duas foram **fechadas como decisão de não fazer** (corpo de aviso em três linhas, auto-scroll por intervalo). Fora dessas duas aprovadas, mexer em qualquer coisa descrita nas seções 1 e 2 exige aval do dono do produto.
