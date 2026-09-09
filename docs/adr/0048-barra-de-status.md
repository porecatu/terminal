# ADR-0048 — Barra de status: faixa no rodapé que encolhe a grade

**Status:** Aceito · §8 Superseded by [ADR-0049](0049-branch-git-na-barra-de-status.md) (**parcial**: só a linha que punha `git` entre o que nunca entra)
**Data:** 2026-09-09
**Relacionados:** ADR-0005, ADR-0007, ADR-0009, ADR-0014, ADR-0018, ADR-0019, ADR-0022, ADR-0027, ADR-0030, ADR-0032, ADR-0034, ADR-0037, ADR-0039, ADR-0041, ADR-0043, PRD-004, PRD-009

## Contexto

A barra de status é o elemento mais bem documentado do projeto que **não tem uma linha de código**. Está desenhada no canvas desde o começo, tem anatomia na especificação visual (§2.8), tem PRD próprio com oito requisitos esboçados ([PRD-009](../prd/prd-009-barra-de-status.md)) e tem os quatro valores de cor já na tabela de tokens. O que nunca teve foi decisão: o [ADR-0009](0009-referencia-visual-e-reconciliacao.md) a classificou `[v2]` e abriu o PRD-009 em rascunho *"para dar endereço a um elemento desenhado no canvas mas fora do escopo do v1"*, e ali parou.

O dono do produto decidiu trazê-la para o produto agora, antes do resto do v2. A razão é o **RF-9.4**, e ela é mais forte do que "mostrar informação". Nas palavras do próprio PRD-009:

> O app precisa dele para a restauração de sessão e o obtém via OSC 7. Hoje esse dado fica invisível. Exibi-lo tem um efeito colateral valioso: **o usuário passa a ver se o OSC 7 está funcionando**. Sem barra de status, a ausência de integração de shell só se manifesta muito depois, quando a sessão restaura no diretório errado — o pior momento possível para descobrir.

O [ADR-0039](0039-convite-a-integracao-de-shell.md) atacou o mesmo problema pelo outro lado: um convite no grid, uma vez, com o snippet a copiar. Ele avisa **quando o app detecta** que não há OSC 7; a barra de status mostra o estado **o tempo todo**, e é o que fecha o laço depois que o convite foi dispensado.

Cinco decisões estavam abertas, e nenhuma tinha resposta escrita:

1. **Empurrar ou sobrepor.** O [ADR-0041](0041-busca-no-scrollback.md) enfrentou a mesma pergunta para a busca e respondeu "sobrepor", com uma razão explícita: empurrar manda `resize` ao PTY, e um programa em execução veria a tela encolher. Aplicar a mesma resposta aqui, sem pensar, seria errado — as duas superfícies não têm a mesma natureza.
2. **A camada.** O [ADR-0018](0018-composicao-de-frame.md) fixou cinco camadas e deixou escrito que *"superfície nova precisa escolher uma camada existente ou justificar uma nova"*.
3. **A marca do RF-9.4.** O requisito pede *"indicação visível de que o diretório não vem de OSC 7"* e não diz qual. O canvas não desenha nada disso — o mockup mostra um caminho e pronto. E o [ADR-0032](0032-interface-do-v1-fechada.md) fechou a interface: mudança das seções 1/2 da especificação exige ADR.
4. **O conflito com a borda de resize.** O [ADR-0027](0027-controles-de-janela-e-resize-proprios.md) tirou a decoração nativa fora do macOS e pôs 6px de zona de resize em toda borda da janela. A borda inferior é exatamente onde a barra de status vai.
5. **O que a barra mostra.** O mockup desenha seis segmentos, e um deles — contagem de painéis — depende do [PRD-006](../prd/prd-006-paineis-divididos.md), que é `[v2]` e não existe.

## Decisão

**A barra de status é uma faixa fixa no rodapé da janela, na camada `Chrome`, que encolhe a grade — ligada por padrão, com cinco segmentos e o `cwd` esmaecido quando não vem de OSC 7.**

### 1. Encolhe a grade — e por que aqui é o contrário do ADR-0041

A barra ocupa 26px permanentes na base da janela. A grade perde as linhas correspondentes, e o PTY recebe `resize`.

O ADR-0041 recusou exatamente isso para a busca, e a razão continua correta lá: *"abrir a busca mandaria uma mudança de tamanho a um programa em execução"*. A diferença é a natureza das duas superfícies, e é ela que inverte a resposta:

| | Busca | Barra de status |
|---|---|---|
| Vida | transitória, abre e fecha durante o trabalho | permanente, é mobília |
| Frequência do `resize` | a cada gesto | uma vez no arranque, mais uma por hot reload de `enabled`/`height` |
| O que cobriria, se sobrepusesse | o topo da vista (saída mais antiga) | **o rodapé — a linha do prompt ativo** |

A terceira linha é decisiva. O próprio ADR-0041 rejeitou a barra de busca no rodapé com esta frase: *"o rodapé é onde o prompt ativo está: a barra taparia justamente a linha que o usuário acabou de digitar"*. Uma barra de status sobreposta cometeria esse erro **permanentemente**, não durante um gesto. O truque de `reserved_rows` que a busca usa para rolar em volta da própria barra não tem análogo aqui: não há para onde rolar o prompt.

Então a barra ocupa espaço de verdade, e a grade é honesta sobre o tamanho que tem.

### 2. Camada `Chrome`, nenhuma camada nova

Mesma resposta e mesmo raciocínio do ADR-0041 §2: `Chrome` está acima da grade e abaixo de aviso, popover e modal, que é a ordem correta — um menu de contexto ou um aviso precisam desenhar por cima da barra. `Layer::ORDER` continua com cinco elementos.

### 3. Cinco segmentos, não seis

| Zona | Segmento | Fonte do dado |
|---|---|---|
| Esquerda | nome do shell, na cor de acento `#5ed3bc` | `Tab::shell_name()` |
| Esquerda | diretório atual | `Tab::cwd()`, com o `cwd` de spawn como fallback |
| Esquerda | nome do grupo da aba ativa | `Group::name()`; ausente em grupo implícito |
| Direita | `UTF-8` | literal |
| Direita | sistema e versão | `std::env::consts::OS` e `CARGO_PKG_VERSION` |

**A contagem de painéis sai.** Ela é o único segmento do mockup que depende de um recurso inexistente (PRD-006, `[v2]`); hoje exibiria "1 painel" para sempre, que é ruído com aparência de informação. Volta quando os painéis existirem, e o PRD-009 já registra a dependência.

O nome do shell é o **único item colorido**, como o design sempre pediu — é o que distingue a aba de relance.

Nenhum desses dados custa syscall por frame. Em particular, `ProcessGroup::process_count()` e `Terminal::cwd_fallback()` estão **proibidos** neste caminho: os dois fazem `refresh_processes(All)` do `sysinfo`, e o RF-9.8 é regra dura (§8).

### 4. A marca do RF-9.4: o `cwd` um degrau abaixo na escada de texto

Quando o diretório exibido **não** veio de um OSC 7 — ou seja, é o `cwd` de spawn, potencialmente obsoleto —, ele é desenhado em `#828a96` ("Terciário", §1.4) em vez da cor de base da barra, `#a8b0bb` ("Secundário").

A primeira versão desta decisão usava **alfa `.45`** sobre a cor de base, reusando o valor que o [ADR-0037](0037-aba-nao-iniciada.md) deu ao rótulo da aba `NotStarted`. Foi descartada ao medir: a 10.5px, o caminho caía para **2.61:1** de contraste contra o fundo, bem abaixo do mínimo WCAG AA de 4.5:1 para texto pequeno. O RF-9.4 pede o caminho **marcado**, não apagado — e como o caso comum no Windows é justamente não haver OSC 7, o alfa tornaria ilegível o segmento que a barra existe para mostrar.

O ADR-0037 continua valendo onde vale: lá o alvo é um rótulo de 13px sobre o fundo de uma aba, e a aba `NotStarted` é o caso raro. Aqui é 10.5px, e o esmaecido é o caso comum. O que se reusa não é o número, é a ideia — *este dado não é o real, é o que sabemos* —, e a escada de ênfase de texto da §1.4 já é o vocabulário do projeto para isso. Os dois tons são tokens existentes, e os dois passam AA: 7.55:1 e 4.74:1.

O tooltip com o caminho completo (a outra metade do RF-9.3) fica de fora — ver §7.

### 5. Geometria: o quadro do terminal cede, e a borda de resize ganha

**O quadro do terminal encosta na barra, sem `margin`.** A primeira versão desta decisão mantinha os 6px de `terminal_frame_margin` entre os dois, por simetria com os outros três lados. Em tela ficou folgado demais — relato do dono do produto — e a razão é que a comparação estava errada: os outros três lados do quadro dão para a **borda da janela**, e a faixa de status não é borda de janela, é outra barra de chrome. A comparação certa é com o **topo**, onde o quadro sempre encostou na barra de abas sem gap, porque um vão ali lê como uma linha a mais entre as duas.

Então o `margin` da base vale contra a borda da janela quando a barra está desligada, e vale zero quando ela está ligada. O quadro tem cantos arredondados embaixo e a faixa é reta: o fundo da janela aparece nos dois cantos, do mesmo jeito que já aparece nos de cima contra a barra de abas.

**A zona de resize ganha os 6px que disputa com a barra.** O ADR-0027 pôs `South`/`SouthEast`/`SouthWest` na borda inferior, exatamente sobre a barra. A barra cede, e a razão é aritmética: no escopo aprovado ela **não tem nenhum alvo clicável** (o RF-9.7 ficou de fora, §7), então ceder 6px não custa função nenhuma — enquanto tirar o resize do rodapé quebraria um gesto que existe hoje e não tem substituto.

O que a barra faz com o mouse é só uma coisa: **impedir que o clique chegue à grade**. Clicar na faixa não posiciona cursor, não inicia seleção, e arrastar uma seleção para dentro dela não continua selecionando.

### 6. Ligada por padrão

`enabled = true`. A decisão estava explicitamente em aberto no RF-9.1 (*"desligada por padrão é uma decisão em aberto, dado que ela custa 26px permanentes"*), e o argumento que a resolve é o RF-9.4: uma barra desligada por padrão **não avisa ninguém** de que o `cwd` é o de spawn. Quem já sabe que o OSC 7 pode faltar não precisa do aviso; quem não sabe nunca vai ligar a barra para descobrir. O valor do recurso está inteiro em ele aparecer sem ser pedido.

Desligar é uma linha de TOML, e desligada a barra não desenha **nem ocupa altura** — a grade volta exatamente ao tamanho de hoje.

### 7. Escopo: seis dos oito requisitos

Entram: **RF-9.1** (ligável), **RF-9.2** (shell, diretório, grupo), **RF-9.3 parcial** (abreviação com `~` e truncamento), **RF-9.4** (a marca), **RF-9.6** (cores, altura e fonte na config) e **RF-9.8** (damage-driven).

Ficam de fora, registrados no PRD-009 como diferidos:

- **RF-9.5** — conteúdo de cada zona configurável a partir de um conjunto fechado de campos. Pede uma gramática de config nova, com catálogo fechado e validação, no mesmo peso do catálogo de ações. Os cinco segmentos fixos entregam o RF-9.2 inteiro sem ela.
- **RF-9.7** — clicar no diretório copia o caminho. Barato em si, mas arrasta hit-test com semântica de clique e um feedback de "copiado" que não existe fora da seleção do terminal. E é o requisito que, se entrasse, tornaria a cessão dos 6px da §5 uma perda real em vez de nenhuma.
- **A metade de tooltip do RF-9.3** — `tooltip::Hover` está amarrado a `TabId`, e `update_hover` só roda quando o cursor está na barra de abas. Generalizar o alvo é refator próprio, não efeito colateral desta entrega. O [ADR-0019](0019-tooltip.md) previu este consumidor; ele continua previsto.

### 8. RF-9.8: nada que mude sozinho

Todos os cinco segmentos mudam **por evento** — troca de aba, OSC 7, renomear grupo, hot reload —, e todos esses eventos já sujam o frame por outros motivos. A barra não introduz temporizador nem redraw periódico, e a propriedade de "terminal ocioso custa zero frames" ([ADR-0007](0007-modelo-de-threading.md)) fica intacta.

Isto exclui, definitivamente: relógio, uso de CPU e memória, estado de repositório `git`, e a contagem de processos do [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md). Os três primeiros mudam sozinhos; o quarto mudaria de graça, mas lê-lo custa uma varredura de `sysinfo`.

> **Revisto pelo [ADR-0049](0049-branch-git-na-barra-de-status.md).** A palavra `git` acima agrupava duas coisas de custo muito diferente: saber **qual é a branch** é ler 30 bytes revalidados por `mtime`; saber se a **árvore está suja** é um `git status` completo. A primeira entrou; a segunda continua fora, e pela razão que este parágrafo dá. O resto da lista não muda.

### 9. O que **não** muda: o ADR-0014

O [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) rejeitou a barra de status como superfície de avisos, e a razão que ele deu foi que ela era `[v2]`: *"amarrar oito requisitos do v1 a um elemento de v2 inverteria a ordem das fases"*. Essa razão expira agora.

**A rejeição continua valendo, por outro motivo, e este ADR não a reabre.** A pilha de avisos tem até três itens simultâneos, severidade em barra colorida, título e corpo, botão de fechar e expiração temporizada (§2.14). Uma faixa de 26px com cinco segmentos fixos não comporta nada disso, e transformá-la em canal de avisos destruiria a única coisa que ela faz bem — estar sempre lá, dizendo sempre a mesma coisa, sem pedir atenção.

Decisão aceita não se edita; o ADR-0014 fica como está, e esta seção é o registro de que a conclusão dele sobrevive à mudança de premissa.

### 10. Anatomia — seção 2.8 da especificação visual

**Nenhum valor novo entra**: tudo sai da tabela de tokens, que estava pronta para este elemento desde o ADR-0009. Altura 26 (§1.7), `padding: 0 12`, mono 10.5px (§1.1, que já diz "barra de status") em `#a8b0bb` (§1.4), `gap: 16`, e o acento `#5ed3bc` no shell (§1.5, que já diz "nome do shell na status").

Quatro coisas que o desenho pedia e o binário **não** faz, as quatro por decisão do dono do produto depois de ver a barra em tela — o binário é a referência normativa ([ADR-0028](0028-o-binario-como-referencia-visual.md)), e a §2.8 registra todas:

- **Sem fundo próprio.** O `clear` da janela já pinta `#1b1f26` ali, e é a mesma cor nos três temas claros embutidos — então o quad era redundante. Pior que redundante: sendo desenhado na camada `Chrome`, ele cobria a **sombra do quadro do terminal**, que sai na camada `Grid` e desce 7,5px (`SHADOW_LAYERS`, o `spread + offset_y` da camada mais externa) sobre o topo da faixa. Em tela a sombra aparecia decepada numa linha reta, e foi o que o dono do produto relatou assim que o quadro passou a encostar na faixa (§5). Sem o quad, a ordem das camadas trabalha a favor: a sombra cai sobre o fundo da janela, e o texto, que começa a 7,75px do topo, fica logo abaixo dela.
- **Sem a borda superior `#23272f`.** Com o quadro encostado na faixa, a sombra dele mais a diferença de fundo já separam os dois; a borda seria uma terceira separação. O token "Separador de barra" continua na §1.3 — ele tem outros consumidores —, e a menção a "topo da status" na descrição dele fica como registro do que o desenho pedia.
- **Sem a versão do app** ao lado do sistema. Ela não muda entre execuções, e o que não muda não é o que se consulta de relance numa barra.
- **Texto em `#a8b0bb`, não `#6b737e`.** Ver §4: o tom do desenho reprova WCAG AA a 10.5px.

Consequência da primeira: `[appearance.status_bar]` **não tem chave de fundo**. Chave que não desenha nada é pior que chave ausente — o usuário mexe e não acontece nada.

**Sem sombra.** A barra é encostada e opaca, não flutua — mesma razão da barra de busca. A lista de superfícies com sombra do ADR-0032 §2 é exaustiva e não muda.

**Sem animação.** A lista de consumidores do relógio do [ADR-0022](0022-animacao-de-interface.md) segue **fechada em dois**.

### 11. Acessibilidade

A barra entra na árvore do [ADR-0043](0043-arvore-de-acessibilidade.md) como projeção das funções puras de layout, igual ao resto do chrome — construída a partir do mesmo `StatusBarLayout` que o pintor consome, nunca em paralelo. Árvore que mente é pior que ausente.

## Alternativas consideradas

### Sobrepor a grade, como a barra de busca

Consistente com o precedente mais recente, e sem `resize` nenhum ao PTY. Rejeitada porque a barra é permanente e mora no rodapé: taparia a linha do prompt ativo para sempre. O ADR-0041 usou exatamente esse argumento para tirar a **sua** barra do rodapé; aplicá-lo aqui leva à conclusão oposta, porque esta barra não pode sair de lá. E o truque de `reserved_rows` da busca não tem análogo — não há para onde rolar o prompt.

### Barra por aba, em vez de por janela

O PRD-009 deixou a pergunta em aberto. Rejeitada: os cinco segmentos descrevem a **aba ativa**, e uma barra por aba significaria N barras invisíveis exceto uma — o mesmo desenho, com estado a mais. Uma barra por janela, lendo a aba ativa, entrega o mesmo comportamento observável.

### Contagem de painéis exibindo "1 painel"

Fidelidade ao mockup. Rejeitada: um número que nunca muda não é informação, e ocupa um `gap` de 16px mais o texto na zona direita. Volta com o PRD-006.

### Marca do RF-9.4 na cor de aviso `#e0b060`

Mais explícito que o alfa, e o token existe. Rejeitada por colisão semântica: `#e0b060` é a cor de **saída WARN do terminal** (§1.5), e a barra fica encostada no terminal. Um caminho amarelo no rodapé leria como "o programa avisou algo", não como "este caminho pode estar velho".

### Marca do RF-9.4 como ícone de alerta antes do caminho

O mais legível dos três, e o mais convencional. Rejeitada pelo custo mecânico registrado: exigiria um codepoint novo na face Lucide recortada, e **glyph que a face embutida não tem não desenha e não avisa** — armadilha que já custou uma fase. Ícone novo pede recorte novo da fonte, para comunicar o que o alfa já comunica com o vocabulário que o app tem.

### Desligada por padrão

Preserva 26px de terminal para quem não pediu a barra. Rejeitada porque anula o RF-9.4, que é a razão de existir do recurso: quem não sabe que o OSC 7 pode faltar nunca vai ligar uma barra para descobrir.

### Camada nova, abaixo de `Chrome`

Daria à barra uma faixa própria. Rejeitada pela mesma razão do ADR-0041: `Chrome` já entrega a ordem correta, e `Layer::ORDER` é uma constante de cinco elementos que todo consumidor percorre.

### A barra vence a borda de resize nos 6px

Seria o comportamento esperado se a barra tivesse alvos. Rejeitada porque ela não tem nenhum no escopo aprovado, e o resize inferior é um gesto sem substituto desde o ADR-0027. Se o RF-9.7 entrar depois, a decisão volta à mesa — e a saída provável é a da busca com o botão de fechar: o alvo específico ganha, o resto da faixa continua sendo resize.

## Consequências

### Positivas

- O RF-9.4 fecha o laço que o ADR-0039 abriu: o convite avisa uma vez, a barra mostra sempre. Uma limitação silenciosa vira uma limitação visível.
- **Zero valor de aparência novo e zero ícone novo.** Todos os tons da §2.8 saem da tabela, e três dela já nomeavam a barra de status explicitamente desde o ADR-0009; a marca do RF-9.4 é um degrau da escada de texto da §1.4, não um valor inventado.
- Todo texto da barra passa **WCAG AA** contra o fundo (4.5:1 para texto pequeno), com teste que reprova a regressão. O tom que o desenho pedia não passava.
- A grade é honesta sobre o tamanho que tem: nada de conteúdo escondido sob uma faixa opaca.
- A barra não toca a trilha de grupos e abas, que é o que o ADR-0032 §2 protege.
- Um elemento sai da tabela de "desenhado, sem requisito no v1" (§4.3) — a primeira redução dessa lista desde que ela foi escrita.

### Negativas

- **26px permanentes de terminal**, e um `resize` de PTY no arranque que não existia. É o preço do §1, e a mitigação é o `enabled = false`.
- A geometria do quadro do terminal ganha um terceiro termo (`bar_height`, `status_bar_height`, `margin`). A fórmula continua em **uma** função (`paint::terminal_box_rect`), que é a defesa contra a cicatriz do `chrome::bar_height`.
- Dois dos oito requisitos do PRD-009 nascem diferidos, e o RF-9.3 nasce pela metade. Registrados como tal, não descobertos depois.
- A especificação visual ganha a terceira mudança de seções 1/2 depois do ADR-0032 — e a primeira que **remove** um `[v2]` da tabela de fases em vez de acrescentar anatomia.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Truncar o `cwd` remedindo texto por frame — a armadilha nº1 do projeto (F3, `fit_width`) | **Alta** | Alto | `TextMeasurer::truncate` já é um shaping só desde a F3, e o resultado é cacheado por `(cwd, largura)`: recomputa em evento de OSC 7 e em resize, nunca no caminho de render |
| Fórmula de geometria duplicada, como o `chrome::bar_height` custou uma vez | Média | Médio | Uma função `status_bar::height(style)`, e o teste que amarra o quad pintado ao que ela promete — o mesmo padrão de `painted_background_and_clip_span_the_whole_bar` |
| `enabled = false` deixar faixa residual ou grade errada | Média | Médio | `height()` devolve `0.0`, e um teste exige que `terminal_box_rect` devolva **exatamente** o retângulo de hoje nessa condição |
| Clique na faixa vazar para a grade, ou seleção continuar dentro dela | Média | Baixo | `in_status_bar` irmão de `in_bar`, com auditoria de cada `!in_bar` — hoje todos significam "a grade" |
| Hot reload de `height` não refluir a grade | Baixa | Médio | `enabled`, `height` e `font_size` entram na cadeia de **classe B** do [ADR-0030](0030-escopo-do-hot-reload.md), com teste ao lado de `tab_height_change_is_class_b` |
| Segmento novo, depois, trazer um dado caro (contagem de processos, `git`) | Baixa | Alto | O §8 é a lista fechada do que não entra, e a razão é o RF-9.8 — não gosto pessoal |
