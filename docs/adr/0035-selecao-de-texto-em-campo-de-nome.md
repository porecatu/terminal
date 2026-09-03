# ADR-0035 — Cursor navegável e seleção de texto no campo de nome

**Status:** Aceito
**Data:** 2026-09-03
**Relacionados:** [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0023](0023-editor-de-grupo.md), [PRD-001](../prd/prd-001-abas.md), [PRD-002](../prd/prd-002-grupos-de-abas.md)
**Supersedes:** [ADR-0023](0023-editor-de-grupo.md) (parcial — só a frase da seção 3 sobre o buffer do campo de nome)

## Contexto

Os dois únicos campos de texto do chrome — o nome do grupo, no editor
([ADR-0023](0023-editor-de-grupo.md)), e o rename inline da aba (F2,
`rename.rs`) — nasceram com a mesma simplificação deliberada: **sem
posição de cursor no meio da string, sempre no fim**. O ADR-0023 registra
isso por escrito, na seção 3: *"O buffer não tem posição de cursor no meio
da string — sempre no fim —, a mesma simplificação que o rename de aba da
F2 assumiu e que a pintura do caret já supõe."*

Usuário relatou: no campo de nome do editor de grupo, não dá para
selecionar o texto (nem com o mouse, nem com `Ctrl+A`) para substituí-lo —
só apagando manualmente com `Backspace`, caractere por caractere. É a
simplificação do ADR-0023 se manifestando como bug de usabilidade, não uma
lacuna nova.

O rename de aba tem exatamente o mesmo modelo, pela mesma simplificação —
corrigir só o editor de grupo deixaria o app com dois campos de texto de
comportamento diferente para a mesma tarefa (editar uma string curta), o
que o próprio ADR-0023 (seção "Alternativas consideradas") já apontou como
risco ao decidir que campo inline da pílula e campo do editor são a
**mesma implementação**: duas implementações divergem, e o usuário
encontra semânticas diferentes para a mesma tarefa em dois lugares.

## Decisão

**Os dois campos de nome do chrome ganham cursor navegável e seleção de
texto, através de um modelo de dados único (`TextFieldState`,
`crates/porecatu-ui/src/text_field.rs`) compartilhado por `GroupEditor` e
`RenameState`.**

### 1. Modelo: buffer, cursor, âncora

`TextFieldState` guarda `buffer: String`, `cursor: usize` (índice de byte,
sempre em fronteira de char UTF-8) e `anchor: Option<usize>`. Há seleção
visível exatamente quando `anchor` é `Some` e difere de `cursor` —
`selection_range()` devolve o par `(início, fim)` normalizado, ou `None`.

Durante um arraste do mouse, `anchor` continua armado mesmo passando
momentaneamente por `anchor == cursor` (o usuário pode arrastar de volta);
só a navegação por teclado **sem** `Shift` limpa `anchor` de propósito,
para colapsar a seleção — mesmo comportamento que qualquer editor de texto
tem quando se aperta uma seta com texto selecionado.

### 2. Teclado

`Ctrl+A` (`Cmd+A` no macOS, mesmo idioma do modificador de seleção múltipla
de aba do [ADR-0021](0021-selecao-multipla-e-gestos-da-barra.md) §3) seleciona
tudo. `Shift+Seta` estende a seleção; seta sem `Shift` move o cursor um
caractere, colapsando uma seleção ativa na borda do lado do movimento em
vez de mover mais um caractere. `Home`/`End` (com/sem `Shift`) vão para o
início/fim. `Delete` apaga à frente. `Backspace`, texto digitado e colar
substituem a seleção quando há uma — o comportamento universal de "digitar
por cima do que está selecionado".

Uma função livre, `apply_text_field_key`, concentra essa tradução
tecla→edição; é compartilhada pelos dois modos de captura (`handle_group_
editor_key`, `handle_rename_key` em `lib.rs`), que continuam tratando
`Enter`/`Esc`/`Tab` — exclusivos de cada widget (confirmar, cancelar,
trocar de região) — fora dela.

### 3. Mouse

Clique posiciona o cursor no caractere sob o ponto — resolvido por um
método novo em `porecatu-render`, `TextMeasurer::index_at_offset`, o
inverso de `TextMeasurer::truncate` (que já resolve "onde cortar" a partir
de um orçamento de largura; este resolve "que índice está sob este x").
Mesma técnica de **um shaping só**, sem cache — roda só em clique/arraste
dentro do campo, não no caminho quente de frame.

Clique + arraste seleciona: enquanto o botão esquerdo permanece
pressionado a partir de um clique dentro do próprio campo, o movimento do
cursor estende a seleção. Diferente do arraste de aba/grupo
([ADR-0021](0021-selecao-multipla-e-gestos-da-barra.md)), não há clone
nem confirmação ao soltar — a seleção de texto já é o estado real durante
o gesto, porque não há nada destrutivo a descartar se o usuário soltar em
qualquer lugar.

### 4. Destaque visual

A seleção é pintada como um retângulo opaco atrás do texto, na cor
`[terminal.colors] selection_background` já existente
(`ResolvedTermPalette::selection_background`) — a mesma cor que a seleção
de texto do terminal usa, sem inventar token novo. O caret (barra sólida
de 1px) só desenha **quando não há seleção ativa**; com seleção, o
destaque já indica onde a próxima edição vai atuar.

## Alternativas consideradas

### Corrigir só o campo do editor de grupo, deixar o rename como está

Seria o escopo mínimo do relato original. Descartada porque os dois campos
são, desde o ADR-0023, a mesma tarefa ("editar uma string curta com
`Enter`/`Esc`") com a mesma simplificação — corrigir um só deixaria o
usuário com um campo que seleciona e outro que não, na mesma barra.

### Cor de seleção própria para o chrome, em vez de reaproveitar a do terminal

Daria controle de tema independente entre terminal e chrome. Descartada
para esta entrega por contrariar "prefira reaproveitar antes de inventar"
quando já existe uma cor com semântica idêntica — se o dono do produto,
vendo o resultado, achar o tom errado para algum tema customizado, é uma
reconsideração de acabamento visual, não um pré-requisito de arquitetura.

### Seleção como overlay translúcido, em vez de opaco

Mais parecido com seleção de texto de sistemas de desktop convencionais.
Descartada porque introduziria um alfa novo sem token existente no
projeto (a seleção do terminal já é opaca, `paint.rs::is_selected`), e o
contraste do texto claro do chrome sobre `selection_background` já lê bem
sem precisar de transparência.

### Duplo clique seleciona a palavra

Comportamento comum de editor de texto. Fora de escopo desta entrega: não
foi pedido, e "palavra" exigiria decidir separadores — os campos de nome
não têm por que herdar `[terminal.selection] word_separators`, que é
config do terminal, não do chrome. Fica registrado como possível extensão
futura, não como lacuna esquecida.

## Consequências

### Positivas

- `Ctrl+A`/clique/arraste funcionam nos dois únicos campos de texto do
  chrome, com o mesmo modelo — nenhuma divergência de comportamento entre
  editor de grupo e rename de aba.
- Nenhuma primitiva nova em `porecatu-render`: seleção usa `Quad` já
  existente, cor já existente, e o único método novo
  (`index_at_offset`) segue o mesmo padrão de shaping único que
  `truncate` já estabeleceu.
- `TextFieldState` é puro e testável sem `winit`/GPU, mesmo padrão dos
  outros widgets de chrome desde a F2.

### Negativas

- Duas fórmulas de geometria que já existiam (deslocamento de scroll do
  texto dentro do campo, retângulo do campo de rename) precisaram ser
  extraídas para funções compartilhadas entre pintura e hit-test
  (`tab_bar::scrolled_text_x`, `chrome::rename_field_rect`) — mais duas
  funções pequenas no módulo de geometria pura, pela mesma razão que o
  CLAUDE.md já registra como armadilha ("fórmula copiada em dois lugares
  só diverge quando alguém mexe nela").
- `chrome::paint`/`overlay::paint_group_editor` ganharam mais um parâmetro
  (`term_pal`) para alcançar a cor de seleção — assinatura um pouco mais
  longa nas duas funções de pintura mais antigas do chrome.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Tom de `selection_background` não combinar com algum tema customizado de chrome | Baixa | Baixo | Reconsiderar como chave própria depois, se o dono do produto pedir ao ver o resultado num tema real |
| Hit-test de clique divergir da pintura por causa da correção de scroll do texto | Média | Médio | `scrolled_text_x` é a única fórmula, usada pelos dois lados — divergência exigiria editar só um dos dois call sites, o que já não é possível |
| `index_at_offset` cortar no meio de um caractere multibyte | Baixa | Médio | `clamp_to_boundary` em `TextFieldState` corrige qualquer índice para a fronteira de char mais próxima antes de usá-lo; testado com acento (`á`, 2 bytes) |
