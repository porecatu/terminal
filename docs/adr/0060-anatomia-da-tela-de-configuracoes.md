# ADR-0060 — Anatomia da tela de configurações: guia lateral, painel de opções e os controles que faltavam

**Status:** Proposto — aguarda o aval visual do dono do produto, como o [ADR-0032](0032-interface-do-v1-fechada.md) exige para mudança das seções 1/2 da especificação visual. O aval é pedido sobre a primeira build com a janela desenhada (etapa 4 do [roadmap](../roadmap.md)), e os valores novos do §5 podem mudar nele; o que for alterado é registrado aqui antes de o status passar a Aceito.
**Data:** 2026-10-02
**Supersedes:** a §2.12 da [especificação visual](../design/especificacao-visual.md) (drawer `[v2]` de 400px) — reescrita no PR que muda o binário, como o [ADR-0028](0028-o-binario-como-referencia-visual.md) manda
**Relacionados:** [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0022](0022-animacao-de-interface.md), [ADR-0023](0023-editor-de-grupo.md), [ADR-0024](0024-face-de-icones.md), [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md), [ADR-0041](0041-busca-no-scrollback.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), [ADR-0059](0059-janela-de-configuracoes.md), [PRD-004](../prd/prd-004-aparencia-do-chrome.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md)

## Contexto

O [ADR-0059](0059-janela-de-configuracoes.md) decidiu **onde** a tela vive — uma janela própria — e o [PRD-016](../prd/prd-016-tela-de-configuracoes.md) **o que** ela mostra: guia lateral à esquerda, painel de opções ocupando o resto, rodapé com Salvar e Descartar. Este ADR decide **como ela é desenhada**. É o quarto a passar pela porta do [ADR-0032](0032-interface-do-v1-fechada.md), depois do [ADR-0037](0037-aba-nao-iniciada.md), do [ADR-0041](0041-busca-no-scrollback.md) e do [ADR-0055](0055-botao-e-popover-de-sessoes.md).

O canvas já tinha desenhado um painel de configurações, e a especificação visual guarda os valores dele (§2.12, §1): fundo de drawer `#171b21`, linha de configuração `#1c2028` com borda `#262b34`, título 15px/500, rótulo de seção 10px uppercase, alternância de 34×19 com trilho `#3f8f80`/`#2a3038`, chip de atalho mono 10.5px. O desenho era um drawer estreito de três seções; a tela pedida é uma janela inteira com nove grupos e controles que o canvas não tem — campo numérico, escolha entre opções, lista editável, captura de atalho.

A regra que não cai, desde o [ADR-0028](0028-o-binario-como-referencia-visual.md): **nenhuma cor, dimensão, raio ou espaçamento inventado.** O que este ADR faz é, para cada elemento, apontar o token de onde ele sai; e, onde não há token — as dimensões da janela e de dois controles —, nomear o valor novo, a seção de config que o carrega e a razão, para o aval.

## Decisão

**Três faixas — cabeçalho, guia lateral, painel com rodapé — sobre os tokens que o canvas já deu ao painel de configurações, com os controles montados a partir de widgets que o app já desenha: campo do editor de grupo, botão do diálogo, item e lista do menu de contexto, alternância e chip do canvas. Nove chaves numa seção `[appearance.settings]`, sete delas com valor novo, todos de dimensão. Nenhuma cor nova. Um ícone novo. Sem animação.**

### 1. As três faixas

```
┌─────────────────────────────────────────────────────────────────┐
│ ⚙ Configurações                                     ─   □   ✕   │  cabeçalho
├──────────────┬──────────────────────────────────────────────────┤
│ Geral        │ Terminal                                         │
│ Shell        │ FONTE                                            │
│▐Terminal  •  │ ┌──────────────────────────────────────────────┐ │
│ Aparência    │ │ Tamanho da fonte                  [ 16.0 ] ↺ │ │  linha
│ …            │ │ Altura da célula em px lógicos               │ │
│              │ └──────────────────────────────────────────────┘ │
│              ├──────────────────────────────────────────────────┤
│              │ [Abrir arquivo no editor]   [Descartar] [Salvar] │  rodapé
└──────────────┴──────────────────────────────────────────────────┘
```

- **Cabeçalho.** Fora do macOS, faixa com a altura da barra de abas (`chrome::bar_height`, 52) e fundo **Barras** `#1b1f26` — a mesma faixa das janelas de terminal, para que os três botões de janela, a drag region e o resize por borda do [ADR-0027](0027-controles-de-janela-e-resize-proprios.md) sejam **os mesmos**, com o mesmo `hit_test`. À esquerda, no `trilha_padding` (6) mais o `padding_left` da aba (10): o ícone `SETTINGS` na em de ícone do chrome × `0.8` (o multiplicador do botão de configurações, §1.1) e o título 15px/500 `#e6eaef` (o token "título do painel de configurações" da §1.1), `gap: 8`. No macOS, a decoração nativa leva o título, e a faixa não existe.
- **Guia lateral.** Largura `sidebar_width` (§5), fundo **Barras** `#1b1f26`, separada do painel por **1px `#2a2f38`** — a "borda esquerda do drawer" da §1.3, agora do lado que a guia toca. `padding: 6` (o do menu de contexto, §2.16).
- **Painel.** Fundo **Drawer** `#171b21` (§1.2, "painel de configurações"), `padding: 18` (o do drawer, §1.7). Rola na vertical; a guia e o rodapé não.
- **Rodapé.** Fixo na base do painel, mesma cor do painel, separado das opções por **1px `#23272f`** (o "separador de barra" da §1.3 — o binário não o pinta em nenhuma barra hoje, e aqui ele volta a ter função: dizer onde a rolagem acaba). `padding: 12px 18px` — o 18 do painel nas laterais e o 12 do `padding: 0 12` do botão do diálogo na vertical.

**Sem barra de rolagem.** O painel rola pela roda do mouse e acompanha o foco do teclado, como o popover de sessões (§2.22); a rolagem disponível se lê pela linha cortada na borda inferior, rente ao separador do rodapé. Uma barra de rolagem seria a primeira do app e um widget novo; se o aval pedir, ela entra por outro ADR.

### 2. Guia lateral e linhas do painel

**Item da guia** = item de menu da §2.16: `padding: 7px 8px`, raio 5, texto 12.5px `#d7dce3`, hover `#242a33`, `gap: 2` entre itens (o do menu). **Grupo escolhido**: fundo **Aba ativa** `#282e37` e texto **Máximo** `#eaeef3` — o par que a barra de abas já usa para "este é o atual". Foco de teclado: borda `1px #5ed3bc`, o anel do diálogo (§2.15). **Marcador de pendente**: o ponto 6×6 dos indicadores da aba (§2.17), em **Acento** `#5ed3bc`, à direita do nome — o mesmo ponto, outra cor da escada semântica, porque "alterado, não gravado" não é atividade nem campainha.

**Título do grupo** no topo do painel: 15px/500 `#e6eaef`. **Rótulo de seção** dentro do grupo (FONTE, CURSOR, HISTÓRICO…): 10px uppercase `#5c646f`, `letter-spacing: .8px`, o rótulo de seção do drawer (§2.12). `gap: 24` entre seções (o do drawer).

**Linha de opção** = a **linha de perfil** do drawer (§1.2, §2.12): fundo `#1c2028`, borda `1px #262b34`, raio 6, `padding: 9px 11px`, `row_gap` (§5) entre linhas. Dentro dela, à esquerda, nome 12.5px `#d7dce3` sobre descrição 11px `#5c646f` (os dois do toggle do drawer, §2.12); à direita, o controle (§3), centrado na vertical.

- **Escopo de classe C** (RF-16.13): logo depois do nome, 11px **Terciário** `#828a96` — o tom que a barra de status usa para o diretório obsoleto (§2.8), pela mesma razão: precisa ser lido, não apagado (`#5c646f` cai abaixo de AA em 11px).
- **Pendente**: o ponto 6×6 Acento, entre o controle e o botão de restaurar.
- **Restaurar padrão**: botão de ícone **`rotate-ccw`** (Lucide, §4) com a anatomia do botão de fechar da aba — 17×17 de desenho, 25×17 de alvo, ícone `#727a86`, hover fundo `#39404b` e ícone `#e4e8ee` (§1.7, §2.14) —, visível só com a linha sob o cursor ou focada, e só quando a opção difere do padrão; tooltip "Restaurar padrão" (§2.20).
- **Inválida** (RF-16.18): borda do controle em **Erro** `#ef8a8a` e, abaixo da descrição, a razão em 11px `#ef8a8a`.
- **Somente leitura** (RF-16.22): controles e textos da linha em `#5c646f`, a regra "indisponível fica esmaecido, nunca ausente" da §2.16.

**Faixa** de arquivo alterado fora ou inválido (RF-16.22, RF-16.23): o **aviso do app** (§2.14) embutido no topo do painel, largura cheia em vez de 320 e sem sombra (não flutua, como a barra de busca — [ADR-0041](0041-busca-no-scrollback.md)): fundo `#1a1e25`, borda `1px #2e343e`, raio 8, `padding: 11px 12px`, barra de severidade de 2px (aviso `#e0b060`, erro `#ef8a8a`), título 12.5px/500 `#dfe4ea`, corpo 11px `#a8b0bb`, e os botões da faixa (Recarregar, Manter minhas alterações, Abrir arquivo no editor) à direita com a anatomia do botão do diálogo (§3).

### 3. Os controles

| Controle | Anatomia | De onde vem |
|---|---|---|
| **Alternância** (booleano) | trilho 34×19, raio 10, `padding: 2`, ligado `#3f8f80`, desligado `#2a3038`; botão 15×15 circular `#f0f3f6`, deslocado 15px quando ligado. **Sem a transição `.15s`** | toggle do drawer (§2.12, §1.5, §1.7); a transição não entra pela lista fechada de consumidores do relógio ([ADR-0022](0022-animacao-de-interface.md)) |
| **Campo de texto** | fundo `#0f1216`, borda `1px #333a45`, foco `#5ed3bc`, raio 5, 13px `#e4e8ee`, `padding: 7px 9px`, altura 30, largura `text_field_width` (§5); placeholder `#5c646f`. Cursor e seleção do [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md) | campo do editor de grupo (§2.10) — o mesmo `TextFieldState` |
| **Campo numérico** | o campo de texto, largura `number_field_width` (§5), texto alinhado à direita; `Up`/`Down` somam e subtraem o passo da opção | idem |
| **Escolha com até três opções** (`shape`, `tab_bar_position`, `show_close_button`, alternância + número de `git`) | **segmentado**: os botões do diálogo (§2.15) colados, altura 30, `padding: 0 12`, borda `1px #262b34`, texto 12.5px `#d7dce3`; raio 5 só nas pontas; o escolhido com fundo `#282e37`, borda `#39404b` e texto `#eaeef3` (a aba ativa) | botão do diálogo + aba ativa |
| **Escolha com mais opções** (idioma) | botão com a anatomia do campo (sem cursor) e o caret `CHEVRON_DOWN` à direita no `gap: 7` da pílula; abre a lista no **menu de contexto** (§2.16) ancorado sob ele, item escolhido com o realce `#242a33` | campo + menu de contexto |
| **Lista editável** (`shell.args`, `trusted_paths`) | uma linha interna por item: campo de texto em largura cheia e, à direita, o `X` do botão de fechar da aba; abaixo, o item "Adicionar" do menu de contexto com o ícone `PLUS` (como "Salvar esta janela…" da §2.22). `gap: 6` entre itens (`trilha_gap`) | campo + botão de fechar da aba + item de menu |
| **Lista de nome e valor** (`shell.env`) | a lista editável com dois campos por linha, nome (`text_field_width / 2`) e valor (o resto), `gap: 6` | idem |
| **Lista de temas** | uma linha de opção por tema, sem botão de restaurar por linha; à direita do nome, a **amostra**: dez quadrados `theme_swatch_size` (§5), raio 3 (swatch do grupo, §1.7), `gap: 2` — fundo, texto e as oito ANSI normais do tema. O escolhido ganha a borda `1px #5ed3bc`; o da sessão (RF-16.25), quando diferente, uma linha 11px `#828a96` abaixo da lista | linha de perfil + swatch do grupo |
| **Atalho** | o chip de atalho do drawer (§2.12): mono 10.5px sobre `#1e232b`, borda `1px #2a2f38`, raio 4, `padding: 3px 7px`, texto `Chord::label`; vários atalhos lado a lado com `gap: 6`; "sem atalho" em `#5c646f`. **Em captura**: borda `#5ed3bc` e o texto "pressione as teclas…" em `#5c646f`. **Conflito**: abaixo do nome da ação, 11px **Aviso** `#e0b060` com a ação em conflito, e os botões Substituir/Cancelar do segmentado | chip do drawer + anel de foco + semântica de aviso |
| **Filtro** (Atalhos) | o campo de busca da barra de busca (§2.21), em largura cheia no topo do grupo | barra de busca |

Toda linha e todo controle focável recebe o **anel de foco** `1px #5ed3bc` do diálogo (§2.15) quando o foco é do teclado.

### 4. Botões, diálogo e ícone

- **Botões do rodapé**: anatomia do diálogo (§2.15) — altura 30, `padding: 0 12`, raio 5, `gap: 8`. **Abrir arquivo no editor** à esquerda e **Descartar** à direita no estilo cancelar (borda `1px #262b34`, texto `#d7dce3`, hover `#262b34`). **Salvar** é o **primeiro botão primário do app**: fundo `#3f8f80` com texto `#f0f3f6` — o par "ligado" da alternância (§1.5), a única cor de ação afirmativa que a paleta já tem — e hover por brilho (`chrome::brighten`, 1.18, o da aba). Indisponível (RF-16.14): texto `#5c646f`, sem fundo, como item de menu indisponível.
- **Diálogo de pendências** (RF-16.4): o diálogo da §2.15, largura 380, agora com **três** botões — Cancelar (foco inicial), Descartar e fechar (destrutivo, `#e08585` com hover `#2e2224`) e Salvar e fechar (primário, acima). Na camada **modal** da janela de configurações.
- **Ícone novo**: Lucide **`rotate-ccw`** em `porecatu_render::icon` (constante, lista `ALL`, `ink_width_em`/`ink_height_em` pinados contra a rasterização, [ADR-0024](0024-face-de-icones.md)). A face Lucide embutida não é recortada; não há subset a refazer. Os outros ícones da tela já existem (`SETTINGS`, `CHEVRON_DOWN`, `PLUS`, `X`, e os de janela).
- **Camadas** ([ADR-0018](0018-composicao-de-frame.md)), dentro da janela de configurações: faixas, guia, painel e controles em `Chrome`; a lista de escolha e o tooltip em `Popover`; o diálogo em `Modal`. Nenhuma camada nova.
- **Sem animação**: nem o `slidein .16s` do drawer, nem a transição da alternância, nem `pop`. A lista do [ADR-0022](0022-animacao-de-interface.md) continua fechada em dois consumidores.

### 5. Valores novos: `[appearance.settings]`

O canvas não tinha janela de configurações, então as dimensões dela não têm token. São nove chaves, sete com valor novo e todas de dimensão, numa seção nova do arquivo de exemplo — com o comentário de origem em cada um, como `[appearance.session_picker]` — e na §1.7 da especificação, **na mesma leva** em que entram no código:

| Chave | Valor proposto | Razão |
|---|---|---|
| `window_width` | 900 | guia de 200 + painel com linha de 640 + os dois `padding: 18` do painel + folga para o botão de restaurar |
| `window_height` | 640 | cabeçalho de 52 + rodapé de 54 + ~8 linhas de opção visíveis sem rolar, o grupo Terminal inteiro em duas telas |
| `min_width` | 640 | abaixo disso a linha não comporta nome, descrição e o campo de texto lado a lado |
| `min_height` | 420 | cabeçalho, rodapé e pelo menos três linhas |
| `sidebar_width` | 200 | a largura mínima do menu de contexto (§2.16), que é o que a guia é: uma lista de itens de menu |
| `text_field_width` | 240 | cabe um caminho de projeto comum e `Iosevka Fixed` em 13px sem truncar |
| `number_field_width` | 88 | cabe `102400` (o maior default do catálogo, `osc52_max_bytes`, seis dígitos, com folga para um sétimo) mais o `padding: 7px 9px` |
| `row_gap` | 8 | o `gap: 8` dos botões do diálogo e dos avisos empilhados (§2.14, §2.15) — não é valor novo, mas ganha chave porque é o espaçamento que define a densidade da tela inteira |
| `theme_swatch_size` | 12 | dez quadrados mais `gap: 2` em 138px, menos que um campo de texto |

(`sidebar_width` e `row_gap` repetem tokens existentes e por isso não contam entre os sete valores novos.) Classe de recarga: **A** para as de dentro da tela, aplicadas no próximo frame da janela aberta; as quatro de tamanho da janela valem na próxima abertura, como `opacity` na próxima janela de terminal — classe **C**, "vale na próxima abertura da tela".

## Alternativas consideradas

### O drawer do canvas, tal como desenhado

Recusado com a superfície (o [ADR-0059](0059-janela-de-configuracoes.md) decide a janela própria): 400px de largura não comportam guia lateral e painel lado a lado, que é o que o dono do produto pediu.

### Guia lateral com ícones por grupo

Cada grupo com um ícone Lucide à esquerda do nome. Nove ícones novos para uma lista de nove palavras, e nenhum deles com significado inequívoco ("Painéis" e "Aparência" competiriam pelo mesmo desenho). O nome basta, e a guia fica mais estreita.

### Salvar com o tom destrutivo invertido ou com o acento `#5ed3bc` cheio

O acento é o token de foco (anel, campo, chip em captura); pintar o botão com ele faria o botão parecer sempre focado. O trilho da alternância ligada já é a cor de "afirmativo" do canvas e está calibrado contra o texto `#f0f3f6`.

### Lista suspensa também para as escolhas de duas ou três opções

Um clique a mais para ver as opções que cabem na linha. O segmentado mostra o estado e as alternativas de uma vez, que é o que uma tela de ajustes quer.

### Barra de rolagem visível

Ver §1: seria o primeiro widget do tipo no app, e a rolagem já se lê pela linha cortada.

## Consequências

### Positivas

- Nenhuma cor nova: tudo sai das §1.2 a §1.5 e da anatomia de widgets que o binário já desenha.
- Os valores do drawer do canvas (§2.12) — os únicos que o projeto guardava sem uso — ganham consumidor.
- Campo, botão, menu e chip são os mesmos componentes do resto do app; a tela não abre uma segunda linguagem visual.

### Negativas

- Sete valores novos de dimensão (nove chaves), numa seção nova do arquivo de exemplo e na tabela `VALORES` do `scripts/verify-docs.py` (os que forem estruturais).
- O primeiro botão primário do app — uma decisão de aparência que vai valer para o próximo que aparecer.
- O cabeçalho repete os 52px da barra de abas numa janela que não tem abas — custo de reaproveitar o `hit_test` dos botões de janela em vez de escrever um segundo.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| O aval visual muda valores do §5 | Alta | Baixo | Status Proposto até a primeira build; mudanças registradas aqui e na §4.4 antes de Aceito |
| Rótulos e descrições traduzidos (de_DE) estouram a largura da linha | Média | Médio | Descrição trunca com reticências e mostra inteira no tooltip, a regra de rótulo de menu do [ADR-0056](0056-catalogo-de-textos-da-interface.md) §12 |
| Contraste da descrição 11px `#5c646f` sobre `#1c2028` abaixo de AA | Média | Médio | Medir na build; se reprovar, subir para **Terciário** `#828a96`, como a barra de status e o aviso já fizeram (§2.8, §2.14) — correção registrada, não valor novo |
