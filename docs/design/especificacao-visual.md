# Especificação visual

Tradução do design canvas em valores implementáveis. É a referência normativa para a **aparência** do chrome; os PRDs continuam normativos para **comportamento**. Onde os dois divergirem, [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md) diz quem vence.

**Fonte:** [`Terminal Multiplataforma.dc.html`](Terminal%20Multiplataforma.dc.html) — cópia verbatim do canvas. Todos os valores abaixo foram extraídos dele, não inventados.

> **Aviso de fase.** O mockup contém elementos que **não são do v1**. Antes de implementar qualquer coisa daqui, consulte a [tabela de fases](#3-tabela-de-fases). Painéis divididos, perfis, paleta de comandos, painel de configurações, barra de status e título customizado são todos `[v2]`.

---

## 1. Tokens

### 1.1 Tipografia

| Uso | Família | Pesos | Onde |
|---|---|---|---|
| Interface | `IBM Plex Sans` | 400, 500, 600 | títulos de aba, rótulos de grupo, menus, configurações |
| Monoespaçada | `IBM Plex Mono` | 400, 500 | conteúdo do terminal, badges, chips de tecla, contadores, barra de status |

Fallback de UI: `system-ui, sans-serif`. Fallback mono: `monospace`.

| Tamanho | Uso |
|---|---|
| 19px / 500 | título da tela de nova aba |
| 15px / 500 | título do painel de configurações |
| 14.5px | campo de busca da paleta de comandos |
| 13px | nome de perfil, item de resultado, campo de nome de grupo |
| 12.5px | **rótulo da aba**, itens de menu |
| 12px | nome do app, rótulo do grupo, campo de rename |
| 11px | subtítulo da barra de título, título do painel, descrição de toggle |
| 10.5px | barra de status, comando do perfil |
| 10px | contador do grupo, rótulo de seção (uppercase, `letter-spacing: .7px`) |
| 9.5px | chips de tecla |
| 9px | badge de perfil (`letter-spacing: .4px`) |
| 8px | caret do grupo, glyph do logotipo |

Terminal: **12.5px**, `line-height: 1.75`.

### 1.2 Superfícies

| Token | Valor | Onde |
|---|---|---|
| Fundo do desktop | `#08090b` | fora da janela (só no mockup) |
| Janela | `#15181d` | corpo da janela |
| Barras | `#1b1f26` | barra de título, barra de abas, barra de status |
| Terminal | `#0f1216` | área de conteúdo e fundo do painel |
| Aba ativa | `#282e37` | |
| Aba inativa | `#191d23` | |
| Pílula de grupo | `#1f242c` | |
| Contador do grupo | `#12151a` | também o botão de busca |
| Popover | `#1a1e25` | menu de perfis, editor de grupo, paleta de comandos |
| Drawer | `#171b21` | painel de configurações |
| Campo de rename | `#0e1116` | |
| Cartão de perfil | `#161a20` (hover `#1b2028`) | tela de nova aba |
| Linha de perfil | `#1c2028` | configurações |
| Hover de menu | `#242a33` | |
| Hover de aba/botão | `#262b34` · `#252a33` · `#39404b` · `#1e232b` · `#262c35` | por componente, ver anatomia |
| Hover destrutivo | `#2e2224` | "Fechar grupo" |
| Chips de tecla | `#1d222a` · `#1e232b` · `#232830` | por componente |

### 1.3 Bordas

| Token | Valor | Onde |
|---|---|---|
| Janela / divisor de popover | `#2a2f38` | também borda esquerda do drawer |
| Separador de barra | `#23272f` | base da barra de título e de abas, topo da status, gap entre painéis |
| Borda de controle | `#262b34` | botões da barra, borda de card e de linha de perfil |
| Borda de popover | `#2e343e` | |
| Borda da aba ativa | `#39404b` | também hover do botão de busca |
| Borda da aba inativa | `#22262e` | |
| Borda da pílula | `#2b313b` | |
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
| Destrutivo forte | `#c4413f` | hover do botão de fechar a janela |
| Destrutivo brando | `#e08585` | item "Fechar grupo" |
| Toggle ligado | `#3f8f80` (trilho), `#f0f3f6` (botão) | desligado: `#2a3038` |
| Seleção de texto | `#2e6b62` (fundo), `#eef2f4` (texto) | vem do `::selection` do canvas |

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
| 8px | janela, popover, wrapper de grupo, card de perfil |
| 6px | **aba**, pílula de grupo, botões da barra, swatch, linha de resultado |
| 5px | item de menu, input de grupo, badge de perfil grande |
| 4px | botão de fechar da aba, botões do painel, campo de rename |
| 3px | badge de perfil, chips de tecla, logotipo, swatch do grupo |
| 9px | contador do grupo (pílula) |
| 50% | ponto de status do painel |

| Altura | Valor |
|---|---|
| Barra de título | 36px |
| **Aba, pílula de grupo, botões da barra** | 30px |
| Barra de status | 26px |
| Botão da barra de título | 44px de largura |
| Botão de fechar da aba | 17×17 |
| Botões do painel | 22×20 |
| Swatch de cor | 28×28 |
| Toggle | 34×19, botão 15×15, deslocamento 15px |
| Cursor do terminal | 7×15 |

| Espaçamento | Valor |
|---|---|
| Barra de abas | `padding: 6px 10px`, `gap: 8` entre as três zonas |
| Entre abas do mesmo grupo | `gap: 4` |
| **Entre grupos** | `gap: 6` |
| Wrapper de grupo | `padding: 3` |
| Aba | `padding: 0 6px 0 10px`, `gap: 8` |
| Pílula | `padding: 0 9px 0 8px`, `gap: 7` |
| Cabeçalho do painel | `padding: 7px 12px` |
| Conteúdo do painel | `padding: 12px 14px` |
| Item de menu | `padding: 7px 8px` |
| Drawer | `padding: 18px`, `gap: 24` entre seções |

Sombras: janela `0 32px 80px rgba(0,0,0,.6)`; popover `0 18px 44px rgba(0,0,0,.55)`; modal `0 28px 70px rgba(0,0,0,.6)`.

Overlays: paleta `rgba(6,7,9,.55)`; configurações `rgba(6,7,9,.45)`.

### 1.8 Tingimentos

Derivados da cor do grupo, por composição alfa:

| Alvo | Alfa | Efeito |
|---|---|---|
| Fundo do wrapper do grupo | `.07` | só quando o grupo está expandido; colapsado fica transparente |
| Fundo do badge de perfil | `.14` | texto do badge usa a cor cheia |
| Fundo do chip de tipo na paleta | `.14` | |

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
| Hover por brilho | `filter: brightness(1.25)` na pílula, `1.18` na aba | evita definir uma cor de hover por grupo |

O hover por `brightness` é uma decisão relevante: com seis cores de grupo, definir hover por cor exigiria doze tokens. O filtro resolve com um.

---

## 2. Anatomia

### 2.1 Barra de título `[v2]`

Altura 36, fundo `#1b1f26`, borda inferior `#23272f`, `padding-left: 12px`.

**Esquerda** (`gap: 9`): logotipo 14×14, raio 3, borda `1.5px #5ed3bc`, com `>` mono 8px `#5ed3bc` centralizado. Nome "Porecatu" 12px/500 `#9aa2ae` (`letter-spacing: .2px`). Travessão `—` 11px `#5c646f`. Rótulo da aba ativa 11px `#5c646f`.

**Direita**: três botões de 44px de largura, altura cheia, `#8b929e`. Minimizar 11px, maximizar 10px, fechar 11px. Hover `#252a33`; o de fechar vira `#c4413f` com texto `#ffffff`.

### 2.2 Barra de abas `[v1]`

Fundo `#1b1f26`, borda inferior `#23272f`, `padding: 6px 10px`, `gap: 8`. Três zonas:

1. **Trilha rolável** — `flex: 1`, `min-width: 0`, `overflow-x: auto`, `gap: 6`. Contém os wrappers de grupo e, ao final, o botão de nova aba.
2. **Botão de busca** `[v2]` — altura 30, `padding: 0 10`, raio 6, fundo `#12151a`, borda `#262b34` (hover `#39404b`). Texto "Buscar" 11px `#6b737e` + chip `Ctrl+Shift+P` mono 9.5px `#7b838f` sobre `#1d222a`, raio 3, `padding: 2px 5px`.
3. **Botão de configurações** `[v2]` — 30×30, raio 6, borda `#262b34`, engrenagem 13px `#9aa2ae`. Hover: fundo `#262b34`, ícone `#e4e8ee`.

### 2.3 Wrapper de grupo `[v1]`

Envolve a pílula e as abas do grupo. `display: flex`, `gap: 4`, `padding: 3`, raio 8.

Fundo: cor do grupo com alfa `.07` quando expandido; `transparent` quando colapsado. **Abas sem grupo usam um wrapper sem pílula e com fundo transparente** — é a representação visual do grupo implícito do [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md).

### 2.4 Pílula de grupo `[v1]`

Altura 30, `padding: 0 9px 0 8px`, raio 6, `gap: 7`, fundo `#1f242c`, borda `1px #2b313b`. Hover `brightness(1.25)`.

Da esquerda para a direita:

1. **Swatch** 8×8, raio 2, na cor do grupo.
2. **Nome** 12px/500 `#c3cad3`, sem quebra.
3. **Contador** mono 10px `#7b838f` sobre `#12151a`, raio 9, `padding: 1px 6px`.
4. **Caret** `▶` 8px `#6b737e`, `rotate(0deg)` colapsado e `rotate(90deg)` expandido, transição `.15s`.

Interação: clique alterna colapso; duplo clique abre o editor.

### 2.5 Aba `[v1]`

Altura 30, `padding: 0 6px 0 10px`, raio 6, `gap: 8`, borda 1px. Hover `brightness(1.18)`.

| Estado | Fundo | Borda | Texto |
|---|---|---|---|
| Ativa | `#282e37` | `#39404b` | `#eaeef3` |
| Inativa | `#191d23` | `#22262e` | `#98a0ab` |

**Sublinhado de grupo**: `box-shadow: inset 0 -2px 0 <cor do grupo>`, ou `transparent` quando desligado. Aparece **junto** com a pílula — os dois indicadores coexistem, não são alternativos. É a origem do `indicator_style` combinável do [PRD-004](../prd/prd-004-aparencia-do-chrome.md) RF-4.14.

Conteúdo:

1. **Badge de perfil** `[v2]` — mono 9px/500, raio 3, `padding: 2px 4px`, `letter-spacing: .4px`. Texto na cor do grupo, fundo na cor do grupo com alfa `.14`.
2. **Rótulo** 12.5px, `max-width: 180px`, truncado com reticências.
3. **Botão de fechar** 17×17, raio 4, `✕` 10px `#727a86`. Hover: fundo `#39404b`, ícone `#e4e8ee`.

**Campo de rename** `[v1]` — substitui o rótulo no lugar. Largura 120, fundo `#0e1116`, borda `1px #5ed3bc`, raio 4, texto `#e4e8ee` 12px, `padding: 2px 5px`, `outline: none`, foco automático. Confirma em `Enter` e no blur; cancela em `Esc`.

### 2.6 Botão de nova aba `[v1]`

30×30, raio 6, `+` 15px `#9aa2ae`, borda `1px #262b34`. Hover: fundo `#262b34`, ícone `#e4e8ee`. Fica ao final da trilha rolável, acompanhando o scroll.

### 2.7 Área de terminal `[v1]`

Fundo `#0f1216`. Com painéis divididos `[v2]`, os painéis ficam lado a lado com `gap: 1px` sobre `#23272f` — o gap é o divisor.

**Painel:** `border-top: 2px` na cor do grupo quando focado, `transparent` quando não. O anel só aparece com mais de um painel.

**Cabeçalho do painel** `[v2]` — `padding: 7px 12px`, borda inferior `#1c2027`. Ponto 6×6 circular na cor do grupo quando focado, `#3b434f` quando não. Título mono 11px `#828a96`, truncado. À direita, dividir (`◫` 11px) e fechar (`✕` 10px), 22×20, raio 4, `#6b737e`, hover fundo `#1e232b` e ícone `#cfd5dd`.

**Conteúdo** — `padding: 12px 14px`, mono 12.5px, `line-height: 1.75`, `white-space: pre-wrap`.

**Prompt e cursor** `[v1]` — primeira parte do prompt na cor do grupo, segunda em `#6b737e`. Cursor 7×15, `margin-left: 6`, cor do grupo, `animation: blink 1.1s step-end infinite`.

Cores de saída: padrão `#c7ccd6`, esmaecido `#6f7783`, sucesso `#86c56a`, aviso `#e0b060`, erro `#ef8a8a`, destaque `#5ed3bc`.

### 2.8 Barra de status `[v2]`

Altura 26, `padding: 0 12`, fundo `#1b1f26`, borda superior `#23272f`, mono 10.5px `#6b737e`, `gap: 16`.

Esquerda: nome do shell em `#5ed3bc`, diretório atual, grupo da aba. Direita: codificação, contagem de painéis, sistema e versão.

### 2.9 Menu de perfis `[v2]`

Popover `top: 76px`, largura 268, fundo `#1a1e25`, borda `#2e343e`, raio 8, `padding: 6`, sombra `0 18px 44px rgba(0,0,0,.55)`, animação `pop .13s`.

Rótulo "Perfis" 10px uppercase `#5c646f`, `letter-spacing: .7px`. Itens: `padding: 7px 8px`, raio 5, `gap: 10`, hover `#242a33` — badge, nome 12.5px `#d7dce3`, tecla mono 9.5px `#5c646f`. Divisor `1px #2a2f38`, `margin: 5px 4px`. Ao final, "Novo grupo de abas".

### 2.10 Editor de grupo `[v1]`

Popover `top: 76px`, largura 286, `padding: 14`, `gap: 13`. Mesmo fundo, borda, raio, sombra e animação do menu de perfis. Posicionado horizontalmente sobre o grupo que está sendo editado.

1. **Rótulo do grupo** — seção 10px uppercase `#5c646f` + input largura total, fundo `#0f1216`, borda `#333a45` (foco `#5ed3bc`), raio 5, 13px `#e4e8ee`, `padding: 7px 9px`, foco automático. Edição ao vivo: o nome muda na barra enquanto se digita.
2. **Cor** — seis swatches 28×28, raio 6, `gap: 8`, borda `2px`. O selecionado ganha anel `#eef2f4`; os demais, `transparent`.
3. **Divisor** `1px #2a2f38`.
4. **Ações** — "Colapsar/Expandir grupo" com chip de tecla à direita; "Desagrupar abas"; "Fechar grupo (N abas)" em `#e08585`, hover `#2e2224`.

O rótulo do botão alterna entre "Colapsar grupo" e "Expandir grupo" conforme o estado.

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

Fundo `#1a1e25`, borda `1px #2e343e`, raio 8, sombra `0 18px 44px rgba(0,0,0,.55)`, animação `pop .13s`.

Da esquerda para a direita: barra de severidade de 2px em altura cheia — erro `#ef8a8a`, aviso `#e0b060`, informação `#5ed3bc` —, depois título 12.5px/500 `#dfe4ea` e corpo 11px `#6b737e`. Botão de fechar 17×17, raio 4, `✕` 10px `#727a86`, hover fundo `#39404b` e ícone `#e4e8ee` — os mesmos do botão de fechar da aba.

Erro de config cita caminho, linha e chave em mono 10.5px `#6b737e`, para que a coordenada seja legível. O convite de integração de shell (RF-3.1) é o único com ação embutida: snippet copiável em mono 10.5px sobre `#12151a`, raio 3, mais um "não mostrar mais".

Erro e aviso persistem até dispensa; informação sai em 6 s. `Esc` dispensa o do topo.

**O que não vem para cá:** fato de uma aba só é escrito como primeira linha no grid dela — diretório inexistente (RF-3.10), código de saída (RF-1.3) —, marcado em `#5ed3bc` e nunca imitando prompt.

### 2.15 Diálogo de confirmação `[v1]`

Overlay `rgba(6,7,9,.45)` sobre a janela. Modal largura 380, `padding: 16`, raio 10, fundo `#1a1e25`, borda `1px #2e343e`, sombra `0 28px 70px rgba(0,0,0,.6)`, animação `pop .14s`.

Título 13px/500 `#e6eaef`, corpo 12.5px `#d7dce3`, `gap: 14`. Dois botões à direita, `gap: 8`, altura 30, `padding: 0 12`, raio 5: **cancelar** com borda `1px #262b34` e texto `#d7dce3`; **confirmar destrutivo** em `#e08585` com hover de fundo `#2e2224`.

O foco inicial é o cancelar. `Enter` aciona o botão focado, `Esc` cancela.

Usado por RF-1.6 (processo em primeiro plano), RF-2.23 (fechar grupo, com a contagem no corpo) e pelo fechamento de janela com mais de uma aba ([ADR-0015](../adr/0015-multiplas-janelas.md)).

### 2.16 Menu de contexto `[v1]`

Mesmos tokens do menu de perfis (2.9), que é `[v2]` — o menu de contexto é `[v1]` e reaproveita a definição.

Popover ancorado no cursor, largura mínima 200, fundo `#1a1e25`, borda `#2e343e`, raio 8, `padding: 6`, sombra `0 18px 44px rgba(0,0,0,.55)`, animação `pop .13s`. Vira nos dois eixos para caber no monitor da janela.

Itens: `padding: 7px 8px`, raio 5, `gap: 10`, texto 12.5px `#d7dce3`, hover `#242a33`. Chip de tecla à direita, mono 9.5px `#5c646f`. Divisor `1px #2a2f38` com `margin: 5px 4px`. Item destrutivo `#e08585`, hover `#2e2224`. **Item indisponível fica esmaecido em `#5c646f`, nunca ausente.**

Navegável por setas, `Enter` aciona, `Esc` fecha; clique fora ou perda de foco também fecham.

Três menus — aba (RF-1.1, RF-1.2, RF-2.20), grupo (RF-2.22) e terminal (F6). O menu do grupo e o editor de grupo (2.10) leem a **mesma** lista de ações, catalogada em [`docs/reference/acoes.md`](../reference/acoes.md).

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
| Wrapper de grupo e tingimento | `[v1]` | [PRD-002](../prd/prd-002-grupos-de-abas.md), PRD-004 RF-4.19 |
| Pílula: swatch, nome, contador, caret | `[v1]` | PRD-002 RF-2.9 a RF-2.13, RF-2.17 |
| Sublinhado de grupo na aba | `[v1]` | PRD-004 RF-4.14 |
| Grupo colapsado (só pílula) | `[v1]` | PRD-002 RF-2.13 |
| Abas sem grupo (wrapper sem pílula) | `[v1]` | [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md) |
| Editor de grupo: nome, swatches, ações | `[v1]` | PRD-002 RF-2.9 a RF-2.11, RF-2.22 |
| Área de terminal, prompt, cursor | `[v1]` | [PRD-005](../prd/prd-005-aparencia-do-terminal.md) |
| Paleta de cores de grupo | `[v1]` | PRD-004 RF-4.18 |
| Tema (fontes, superfícies, cores) | `[v1]` | PRD-004, PRD-005 |
| Aviso do app (empilhado, com severidade) | `[v1]` | [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md) — sem representação no canvas |
| Nota na aba (primeira linha do grid) | `[v1]` | ADR-0014, PRD-003 RF-3.10, PRD-001 RF-1.3 |
| Diálogo de confirmação | `[v1]` | ADR-0014; PRD-001 RF-1.6, PRD-002 RF-2.23 |
| Menu de contexto de aba e de grupo | `[v1]` | ADR-0014; PRD-001 RF-1.1, PRD-002 RF-2.22 |
| **Painéis divididos** | `[v2]` | [PRD-006](../prd/prd-006-paineis-divididos.md) *(rascunho)* |
| **Cabeçalho e botões do painel** | `[v2]` | PRD-006 *(rascunho)* |
| **Perfis de aba e menu de perfis** | `[v2]` | [PRD-007](../prd/prd-007-perfis-de-aba.md) *(rascunho)* |
| **Badge de perfil na aba** | `[v2]` | PRD-007 *(rascunho)*, PRD-004 RF-4.23 |
| **Tela de nova aba** | `[v2]` | PRD-007 *(rascunho)* |
| **Paleta de comandos e botão de busca** | `[v2]` | [PRD-008](../prd/prd-008-paleta-de-comandos.md) *(rascunho)* |
| **Barra de status** | `[v2]` | [PRD-009](../prd/prd-009-barra-de-status.md) *(rascunho)* |
| **Painel de configurações GUI** | `[v2]` | [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md) — sem PRD |
| **Barra de título customizada** | `[v2]` | ADR-0009 — sem PRD |

---

## 4. Rastreabilidade

### 4.1 Requisitos do v1 que o design cobre

| Requisito | Onde aparece |
|---|---|
| PRD-001 RF-1.7, RF-1.10 (título, truncamento) | rótulo da aba, `max-width: 180px` |
| PRD-001 RF-1.8 (rename inline) | campo com borda de acento sobre a aba |
| PRD-001 RF-1.14 (aba ativa inequívoca) | fundo, borda **e** cor de texto mudam juntos — não só matiz |
| PRD-002 RF-2.9 (nome do grupo) | pílula + input do editor |
| PRD-002 RF-2.10, RF-2.11 (cor e indicador) | swatch, tingimento do wrapper, sublinhado da aba |
| PRD-002 RF-2.13 (colapso) | caret rotacionado, abas ocultas, contador visível |
| PRD-002 RF-2.22, RF-2.23 (ações e confirmação) | editor: colapsar, desagrupar, "Fechar grupo (N abas)" |
| PRD-004 RF-4.18 (paleta) | seis swatches |
| PRD-004 RF-4.19 (tingimento) | alfa `.07` no wrapper |
| PRD-005 (cores do terminal) | seis cores semânticas de saída |
| ADR-0006 (grupo implícito) | wrapper sem pílula das abas soltas |

### 4.2 Requisitos do v1 **sem** representação no design

Precisam de decisão de desenho na implementação. Listados para não passarem batido:

| Requisito | O que falta |
|---|---|
| PRD-001 RF-1.20, RF-1.21 | indicadores de atividade e de campainha na aba |
| PRD-001 RF-1.19 | indicador de abas fora da vista, com contagem |
| PRD-001 RF-1.15, RF-1.16 | estado de arraste: aba fantasma, deslocamento das vizinhas, realce da fronteira do grupo |
| PRD-002 RF-2.2 | aba selecionada (seleção múltipla) — distinta da aba ativa |
| PRD-002 RF-2.5 | animação da reordenação ao formar grupo |
| PRD-002 RF-2.16 | indicador agregado de atividade em grupo colapsado |
| PRD-003 RF-3.9 | aba restaurada ainda sem shell iniciado |
| PRD-005 RF-5.14 | cores de seleção de texto no terminal — o valor da seção 1.5 vem do `::selection` do canvas, não de decisão deliberada |

Enquanto não houver desenho aprovado para esses, valem os tokens da seção 1 e o julgamento de quem implementa — nunca cores ou dimensões novas fora da tabela.

**Resolvidos depois da primeira versão desta lista.** O RF-4.21 (como o erro de configuração é exibido) estava aqui e saiu: o [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md) definiu a superfície de aviso, e a anatomia está na seção 2.14. O mesmo ADR cobriu sete requisitos que não constavam nem desta lista nem da 4.1 — RF-1.6, RF-2.23, RF-3.1, RF-3.10, RF-3.14, RF-3.16 e RF-5.8 —, além do menu de contexto exigido por RF-1.1, RF-1.2, RF-2.20 e RF-2.22. Nenhum deles introduziu cor nova: todos saem dos tokens de popover da seção 1.

O comportamento da seleção de texto — gesto, semântica de palavra, recorte de espaço, remontagem de linha quebrada — está no [ADR-0013](../adr/0013-mouse-selecao-e-clipboard.md); o que a linha do RF-5.14 acima registra é só a origem incidental da **cor**.

### 4.3 Elementos do design **sem** requisito no v1

Todos `[v2]`, todos endereçados na tabela de fases: painéis divididos, perfis e badge, tela de nova aba, paleta de comandos, barra de status, painel de configurações, barra de título customizada.

### 4.4 Divergências resolvidas

| Divergência | Resolução | Onde |
|---|---|---|
| Design combina pílula **e** sublinhado; PRD-004 modelava enum exclusivo | `indicator_style` vira lista combinável, default `["pill", "underline"]` | ADR-0009 |
| Design usa `Ctrl+T`, `Ctrl+G`, `Ctrl+,`, `Ctrl+1..6` | ADR-0008 vence: nada de `Ctrl+<letra>` sozinho. Chips de tecla do mockup são ilustrativos | ADR-0009 |
| `Ctrl+Shift+P` é paleta no design, `theme.cycle` no ADR-0008 | Paleta fica com `Ctrl+Shift+P`; `theme.cycle` migra para `Ctrl+Shift+Y` | ADR-0009 |
| Design tem configurações por GUI; ADR-0003 decidiu só TOML | Painel é `[v2]` e, quando existir, **escreve no TOML** — o arquivo continua sendo a única fonte de verdade | ADR-0009 |
| Design tem barra de título própria; default é `decorations = true` | Barra customizada é `[v2]`; o default do v1 permanece nas decorações do sistema | ADR-0009 |
| Design diz "guias"; docs dizem "abas" | Projeto padroniza **"abas"**; rótulos do mockup ajustados | ADR-0009 |
| Paleta do design tem 6 cores; exemplo tinha 8 | Seis cores do design viram a paleta padrão | ADR-0009 |
| Fontes e cores do design vs. catppuccin/JetBrains | Design vira o default; catppuccin sobra como tema nomeado | ADR-0009 |
