# ADR-0041 — Busca no scrollback: barra sobreposta no topo do quadro

**Status:** Aceito
**Data:** 2026-09-04
**Relacionados:** ADR-0008, ADR-0013, ADR-0018, ADR-0021, ADR-0022, ADR-0023, ADR-0032, ADR-0035, PRD-002, PRD-011

## Contexto

A busca no scrollback é o primeiro item da F6 e o único recurso do v1 que **precisa de um widget novo** — o sexto de chrome, depois de aviso, diálogo, menu de contexto, tooltip e editor de grupo. Quatro decisões estavam abertas, e três delas foram deixadas explicitamente em aberto por ADRs anteriores:

1. **A camada.** O [ADR-0018](0018-composicao-de-frame.md) fixou cinco camadas e escreveu a pergunta: *"superfície nova (busca da F6, paleta de comandos `[v2]`) precisa escolher uma camada existente ou justificar uma nova"*.
2. **A aparência.** O [ADR-0032](0032-interface-do-v1-fechada.md) fechou a interface do v1 e estabeleceu que *"a trilha de grupos e abas só é tocada quando um recurso novo exigir"* e que qualquer mudança das seções 1/2 da especificação visual **exige ADR novo**. A seção 4.2 (requisitos sem desenho) está vazia desde a abertura da F5, e o único "campo de busca" desenhado no canvas é o da paleta de comandos, marcada `[v2]`. Ou seja: não há desenho a seguir, e não há como implementar sem esta decisão.
3. **O modo de captura de teclado.** A cadeia do [ADR-0008](0008-teclas-e-roteamento-de-input.md) tem o modo de captura como passo 1, e todos os modos que existem hoje — diálogo, menu, rename, editor de grupo — **consomem a tecla por inteiro**. A busca é a primeira superfície de longa duração do app, e esse contrato, aplicado tal e qual, tornaria `search.next` inalcançável por tecla.
4. **Como o realce chega ao pintor.** `porecatu_term::snapshot::Cell` não tem conceito de ocorrência, e o snapshot sai do crate com **cor não resolvida** de propósito — quem aplica paleta é `porecatu-ui`.

Some-se um ponto que não é decisão de desenho e sim de comportamento observável: uma barra que **empurre** a grade muda o número de linhas da aba, e mudar o número de linhas significa `resize` de PTY. Um programa em execução veria a tela encolher ao abrir a busca e crescer ao fechá-la — `vim` redesenharia, `htop` recalcularia, e o histórico de saída se reflui. Abrir uma busca não pode ter esse efeito.

O motor já entrega a metade difícil: o `alacritty_terminal` traz `term::search::RegexSearch`, `regex_search_left`/`regex_search_right` e `RegexIter`. Nada disso pode atravessar a fronteira — a armadilha registrada é que *"o `alacritty_terminal` não segue SemVer estável; mantenha o uso isolado dentro de `porecatu-term`"*.

## Decisão

**A busca é uma barra sobreposta ao topo do quadro do terminal, na camada `Chrome`, com captura de teclado parcial e realce resolvido em `porecatu-ui`.**

### 1. Sobreposição, nunca reflui

A barra desenha **por cima** das primeiras linhas da grade. A grade não muda de tamanho, o PTY não recebe `resize` e nenhum programa em execução percebe que a busca existe.

Duas consequências que a implementação tem de honrar:

- O conteúdo sob a barra fica **oculto** enquanto a busca está aberta. É o preço, e é o topo da vista — a região mais antiga do que está em tela, não a linha do prompt.
- Rolar até a ocorrência ativa **reserva a altura da barra**: a ocorrência nunca para numa linha coberta. O alvo de rolagem é a primeira linha visível *abaixo* da barra.

### 2. Camada `Chrome`, não `Popover`

A busca coexiste com tooltip e menu de contexto, que são `Popover`, e tem de ficar **por baixo** deles — um menu de contexto aberto sobre a busca precisa desenhar em cima. E não é `Grid`, porque desenha sobre o texto do terminal. `Chrome` é a única camada que satisfaz as duas, e **nenhuma camada nova entra**: a pergunta que o ADR-0018 deixou escrita fica respondida sem alterar `Layer::ORDER`.

### 3. Captura parcial — a busca é a primeira superfície não modal

Enquanto a busca está aberta e o campo tem foco, o nível de captura consome:

- todo texto imprimível;
- `Enter` (`search.next`), `Shift+Enter` (`search.prev`), `Esc` (fecha);
- as teclas de edição do [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md): setas, `Home`/`End`, `Shift`+seta, `Ctrl+A`/`Cmd+A`.

**Todo o resto cai para o passo 2 da cadeia** — o keybind de aplicação. Trocar de aba, abrir aba nova, rolar o scrollback e fechar a janela continuam funcionando com a busca aberta.

Isto é um **refinamento** do ADR-0008, não uma contradição: o passo 1 continua vindo primeiro e continua consumindo por inteiro **o que reivindica**. O que muda é que a busca reivindica um conjunto nomeado em vez de tudo. A razão é decisiva e não estética: `search.next`/`search.prev` são ações do catálogo, vinculáveis a tecla (`docs/reference/acoes.md`), e uma captura total as tornaria inalcançáveis exatamente quando fazem sentido.

Os quatro modos de captura existentes **não mudam**: diálogo, menu, rename e editor de grupo são modais por natureza — o usuário está no meio de uma decisão ou de uma edição curta, e sair dela é um `Esc`. A busca fica aberta enquanto o usuário trabalha.

### 4. Ocorrências como lista de ranges, não como flag de célula

`CellFlags` tem 7 bits livres de 16, e usar um deles seria mais barato de pintar. Rejeitado: o snapshot é reconstruído a cada frame a partir do motor, e marcar célula significaria reescrever o snapshot inteiro a cada tecla digitada na busca — o custo cresce com a área da grade, no caminho mais quente que existe.

`porecatu-term` expõe a busca num módulo próprio (`search.rs`) que devolve **ocorrências como ranges de posição na grade**, com o `RegexSearch` e todo tipo do `alacritty_terminal` presos dentro do crate. `porecatu-ui` recebe a lista, corta pela vista e resolve a cor — a mesma divisão de trabalho que já vale para a paleta.

### 5. Cores: nenhum valor novo

| Elemento | Valor | De onde vem |
|---|---|---|
| Ocorrência não ativa | fundo `#2e6b62`, texto `#eef2f4` | `[terminal.colors] selection_background`/`selection_foreground` (§1.5, RF-5.14) |
| Ocorrência ativa | fundo `#5ed3bc`, texto `#12151a` | acento (§1.5) e o tom escuro que a pílula de grupo já usa sobre cor cheia (§1.4) |

Como a ocorrência não ativa usa a **mesma** cor da seleção de texto, abrir a busca **limpa a seleção**. Sem isso, seleção e ocorrência seriam indistinguíveis. O motor já invalida seleção sozinho em várias situações e a regra registrada é não reimplementar isso de fora — aqui é uma limpeza explícita no gesto de abrir, não uma regra de invalidação nova.

### 6. Anatomia — seção 2.21 da especificação visual

Tudo sai dos tokens que existem. **Nenhuma cor, dimensão, raio ou espaçamento novo, e nenhum ícone novo.**

- Barra dentro do quadro arredondado do terminal (§2.7), encostada no topo, ocupando a largura interna do quadro. Altura **30** (`input_height` do editor de grupo). Fundo `#1a1e25` e borda inferior `1px #2e343e` — os do aviso e do popover (§1.2). Os cantos superiores acompanham o raio do quadro; os inferiores são retos.
- **Campo de texto**: o mesmo componente do item 1 da §2.10 e da §2.10.1 — fundo `#0f1216`, borda `1px #333a45` com foco `#5ed3bc`, raio 5, texto 13px `#e4e8ee`, `padding: 7px 9px`, cursor e seleção do ADR-0035. Ocupa a largura restante.
- **Contador** `n/total`, 11px `#6b737e` (tênue, §1.4), à direita do campo. Sem ocorrência, o mesmo lugar traz "nenhum resultado" no mesmo tamanho e cor. Padrão de regex inválido traz "padrão inválido" em `#ef8a8a` (erro, §1.5).
- **Alternador de expressão regular**: o toggle da §1.5 — trilho 34×19 raio 10, ligado `#3f8f80` e desligado `#2a3038`, botão 15×15 circular `#f0f3f6`. É a primeira vez que esse token desenha no v1; ele já estava na tabela.
- **Três botões de ícone** à direita, na ordem: `CHEVRON_LEFT`, `CHEVRON_RIGHT`, `X` — os três já registrados em `porecatu_render::icon`, com a em de `chrome::ICON_EM_SIZE`, o tom de base `#e4e8ee` e o mesmo `icon_button_padding_x` do resto da barra. **Não há lupa**: seria um ícone novo, e o campo com foco automático já diz o que é.
- Sombra: **nenhuma**. A barra é encostada e opaca, não flutua — as sombras do ADR-0032 §2 são dos cinco widgets que flutuam sobre o terminal, e a lista é exaustiva.
- Hover por brilho nos três botões, `1.18`, como os demais botões de ícone da barra de abas.

### 7. Tela alternativa

Com `alt_screen` ativo não existe scrollback a percorrer e a tela pertence ao programa. A busca abre, opera **só sobre a tela visível** e o contador é acompanhado da razão. Não há caso em que a busca devolva zero silenciosamente por um limite que o usuário não pode ver.

### 8. Escopo por aba, estado efêmero

O estado da busca — termo, modo, ocorrência ativa — vive em `WindowState`, por aba, e **não é persistido na sessão**. É a mesma classificação da seleção múltipla no [ADR-0021](0021-selecao-multipla-e-gestos-da-barra.md): estado efêmero de janela. O [ADR-0036](0036-formato-do-arquivo-de-sessao.md) não ganha campo.

Ir até uma ocorrência numa aba dentro de grupo colapsado **expande o grupo** pelo caminho que já existe: `Workspace::activate_tab` carrega a regra do RF-2.17 desde o PR de fechamento da F3, e a restauração de sessão já provou o mecanismo na F5. A busca é a segunda fonte que o requisito citava, e fecha o RF-2.17 por completo.

### 9. Sem animação

A barra aparece e desaparece na hora. A lista de consumidores do relógio do [ADR-0022](0022-animacao-de-interface.md) é **fechada em dois** (colapso e formação de grupo), e abrir busca é um gesto que o usuário quer imediato — 150 ms entre `Ctrl+Shift+F` e poder digitar é atrito, não polimento.

### 10. Defaults de tecla

O arquivo de exemplo dizia que *"os defaults são decididos quando a fase começar"*. São estes:

| Ação | Windows, Linux | macOS |
|---|---|---|
| `search.open` | `Ctrl+Shift+F` | `Cmd+F` |
| `search.next` | `F3` | `F3` |
| `search.prev` | `Shift+F3` | `Shift+F3` |
| `selection.select_all` | `Ctrl+Shift+A` | `Cmd+A` |

`F3`/`Shift+F3` valem nas **três** plataformas, e no macOS isso é desvio consciente da convenção local: lá "próxima ocorrência" seria `Cmd+G`/`Cmd+Shift+G`, mas as duas já são `group.create` e `group.dissolve` pela tabela do [ADR-0008](0008-teclas-e-roteamento-de-input.md). Mover um default estabelecido de grupo — o diferencial do produto — para acomodar um recurso novo é o negócio errado; e, dentro da busca, `Enter` e `Shift+Enter` fazem o mesmo sem binding nenhum, pela captura da §3.

Os quatro entram no arquivo de exemplo e nos defaults embutidos **na etapa 2, junto com o despacho**, e não antes. Ligar a tecla enquanto a ação ainda devolve "não tratada" faria o app **engolir** `F3` e `Ctrl+Shift+A` sem repassá-los ao terminal — uma regressão em qualquer TUI que use essas teclas. Até lá ficam comentados no arquivo, com o valor já escrito.

## Alternativas consideradas

### Barra no rodapé, como `vim`, `less` e `fzf`

Familiar para quem vive em TUI, e foi a primeira candidata. Rejeitada porque o rodapé é onde o **prompt ativo** está: a barra taparia justamente a linha que o usuário acabou de digitar, e num terminal a última linha é a mais quente da tela. No topo, o que fica coberto é a saída mais antiga em vista.

### Overlay flutuante no canto superior direito da grade

Tapa o mínimo da grade e reusaria a geometria de popover que já existe. Rejeitada por colisão de âncora: é exatamente onde o aviso do app mora ([ADR-0014](0014-superficie-de-aviso-e-dialogo.md), canal 1), com **até três empilhados**. Uma config inválida durante uma busca deixaria as duas superfícies disputando o mesmo canto, e nenhuma das duas pode ceder — o aviso porque é o único canal de erro do app, a busca porque está sob o dedo do usuário.

### Empurrar a grade em vez de sobrepor

Nada ficaria oculto, e é o que um layout de caixa faria naturalmente. Rejeitada pelo `resize` de PTY: abrir a busca mandaria uma mudança de tamanho a um programa em execução. `vim` redesenha, `htop` recalcula, e a saída já emitida se reflui — um efeito colateral grande e visível para um gesto que deveria ser somente de leitura.

### Camada nova, entre `Chrome` e `Warning`

Daria à busca uma faixa própria sem disputar ordem com nada. Rejeitada porque não resolve problema nenhum: `Chrome` já a põe acima da grade e abaixo de popover e modal, que é a ordem correta. Camada é uma constante enumerada de cinco elementos (`Layer::ORDER`) que todo consumidor percorre; somar uma sexta por conveniência é o tipo de crescimento que o ADR-0018 fechou de propósito.

### Um bit de `CellFlags` para marcar ocorrência

Mais barato de pintar: o pintor já lê `flags` por célula. Rejeitado pelo custo do outro lado — o snapshot é reconstruído por frame, e marcar célula obrigaria a reescrevê-lo a cada tecla digitada na busca, com custo proporcional à área da grade. A lista de ranges é O(ocorrências), e ocorrências são poucas.

### Captura total de teclado, como os outros quatro modos

Consistente com o contrato existente, e `Esc` fecha. Rejeitada porque tornaria `search.next` e `search.prev` inalcançáveis por tecla — as duas ações que o catálogo define para esta fase —, e porque a busca é a primeira superfície do app que fica aberta enquanto o usuário continua trabalhando. Consistência que impede o recurso de funcionar não é consistência.

### Ícone de lupa no campo

É a convenção universal de campo de busca. Rejeitado por uma razão mecânica: exigiria um codepoint novo na face Lucide recortada, e a regra registrada é que **ícone do chrome sai de `porecatu_render::icon`, nunca de um glyph escrito à mão** — glyph que a face embutida não tem *não desenha e não avisa*, armadilha que já custou uma fase. Um ícone novo pede recorte novo da fonte; o campo com foco automático resolve a mesma comunicação por zero.

## Consequências

### Positivas

- A pergunta que o ADR-0018 deixou escrita fica respondida **sem camada nova**.
- O widget entra com **zero valor de aparência novo e zero ícone novo** — reusa o campo de texto do editor de grupo (inclusive a seleção do ADR-0035), o toggle que já estava na §1.5 e três ícones já recortados.
- Nenhum programa em execução percebe a busca: sem `resize`, sem reflui.
- O RF-2.17 fecha por completo, pela segunda fonte que ele sempre citou.
- A busca não toca a trilha de grupos e abas, que é o que o ADR-0032 §2 protege.

### Negativas

- O topo da vista fica coberto enquanto a busca está aberta. É a troca deliberada contra o `resize` de PTY, e a mitigação é o alvo de rolagem reservar a altura da barra.
- A cadeia de captura do ADR-0008 passa a ter **dois formatos** — total nos quatro modais, parcial na busca. É complexidade real, e a razão de estar escrita aqui em vez de descoberta em código.
- Abrir a busca limpa a seleção de texto. Quem selecionou algo e depois abre a busca perde a seleção; o alternativo era inventar uma segunda cor de realce, o que o ADR-0032 proíbe sem aval.
- A especificação visual ganha a **§2.21**, primeira seção de anatomia nova desde a F3.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Buscar em scrollback grande travar a UI | Média | Alto | A busca roda na main thread, então a métrica é o teto: se um scrollback cheio (`scrollback.lines`) não fechar dentro de um frame, a busca vira incremental por lotes. Medido na etapa 1, antes de existir barra |
| Captura parcial deixar passar tecla que devia ser texto | Média | Médio | A lista de teclas reivindicadas é fechada e testada como função pura, no padrão de `should_confirm_tab_close` |
| Ocorrência ativa parar numa linha coberta pela barra | Média | Baixo | O alvo de rolagem desconta a altura da barra; teste de função pura sobre a geometria, sem GPU |
| Regex do usuário catastrófico (backtracking) | Baixa | Médio | O `regex-automata` que o motor usa não faz backtracking exponencial por construção; padrão que não compila é erro exibido, não pane |
| Reuso do campo de texto divergir do editor de grupo | Baixa | Baixo | É literalmente o mesmo código (`text_field.rs`, ADR-0035); divergir exigiria copiar, e copiar fórmula de geometria já é armadilha registrada |
