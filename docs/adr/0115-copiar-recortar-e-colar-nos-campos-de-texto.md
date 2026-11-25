# ADR-0115 — Copiar, recortar e colar nos campos de texto

**Status:** Aceito — a implementar
**Data:** 2026-11-25
**Relacionados:** [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0013](0013-mouse-selecao-e-clipboard.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md), [ADR-0041](0041-busca-no-scrollback.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0059](0059-janela-de-configuracoes.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md), [ADR-0080](0080-editor-de-perfil-em-janela-propria.md), [ADR-0108](0108-superficies-de-ui-do-acesso-remoto.md), [PRD-010](../prd/prd-010-interacao-e-superficie-de-app.md), [PRD-011](../prd/prd-011-polimento.md), [PRD-014](../prd/prd-014-sessoes-nomeadas.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md), [PRD-020](../prd/prd-020-conexoes-ssh.md)
**Supersedes:** [ADR-0041](0041-busca-no-scrollback.md) §3 (**parcial**: só a frase "todo o resto cai para o passo 2 da cadeia" — as teclas de clipboard do campo passam a ser da busca) · [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md) §2 (**parcial**: o "colar" que a seção cita ganha teclas, e copiar e recortar entram) · [ADR-0059](0059-janela-de-configuracoes.md) §3 e [ADR-0080](0080-editor-de-perfil-em-janela-propria.md) §5 (**parcial**: as teclas próprias da janela ganham as de clipboard do campo, e o campo ganha o menu) · [ADR-0075](0075-superficies-de-ui-do-ssh.md) §1 (**parcial**: o campo mascarado ganha o menu do campo, com Copiar e Recortar esmaecidos; a regra de clipboard não muda) · [ADR-0013](0013-mouse-selecao-e-clipboard.md) §Clipboard (**parcial**: o ponto único do `arboard` passa a atender também os campos)

## Contexto

Usuário relatou: nos campos de texto do app — o nome no editor de grupo, os campos de texto da tela de configurações — `Ctrl+C`, `Ctrl+X` e `Ctrl+V` não funcionam. A leitura do código confirma o relato e o estende a **todo** campo que usa `TextFieldState` ([ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md)): rename de aba, editor de grupo, nome no popover de sessões ([ADR-0055](0055-botao-e-popover-de-sessoes.md)), barra de busca ([ADR-0041](0041-busca-no-scrollback.md)), campos da tela de configurações ([ADR-0059](0059-janela-de-configuracoes.md)) e do editor de perfil ([ADR-0080](0080-editor-de-perfil-em-janela-propria.md)).

A causa não é um atalho engolindo a tecla. É que nunca foi implementado:

- `apply_text_field_key` (`porecatu-ui/src/text_field.rs`), o núcleo de edição único que todo campo chama, conhece `Ctrl+A`, setas, `Home`/`End` e `Delete`. Qualquer outra combinação cai no ramo "digitar", que insere o texto do evento depois de filtrar caracteres de controle. No Windows e no Linux o `winit` entrega `Ctrl+C`/`X`/`V` como `"\x03"`/`"\x18"`/`"\x16"` no texto do evento; o filtro os descarta, e a tecla some em silêncio.
- `TextFieldState` não tem como devolver o texto selecionado, recortar nem inserir uma string inteira.
- O ADR-0035 §2 já dizia que "colar substitui a seleção", sem dizer por qual tecla. É lacuna de implementação, não de decisão — a mesma natureza do relato que originou o ADR-0035.

Na **barra de busca** o efeito é pior que "não faz nada". A busca tem captura **parcial** (ADR-0041 §3): o que ela não reivindica cai para o keybind de aplicação e, sem keybind, para o terminal. `Ctrl+C` com a busca aberta chega ao PTY como `0x03` e **interrompe o processo em primeiro plano**; `Ctrl+V` manda `^V`. E `Ctrl+Shift+V`, que é `clipboard.paste`, cola **no terminal**, não no campo em que o usuário está digitando.

O único campo que já trata clipboard é o mascarado ([ADR-0075](0075-superficies-de-ui-do-ssh.md) §1): copiar e recortar não fazem nada, de propósito, e colar funciona — `apply_masked_key` devolve `MaskedKey::Paste` e quem chama lê o clipboard. Esse desenho é o que este ADR estende aos outros campos.

## Decisão

**Todo campo de texto do app copia, recorta e cola pelas convenções de teclado do sistema e por um menu de clique direito. O núcleo do campo continua puro: ele diz o que fazer, e quem chama fala com o clipboard.**

### 1. As teclas

| Gesto | Windows e Linux | macOS |
|---|---|---|
| Copiar | `Ctrl+C`, `Ctrl+Shift+C`, `Ctrl+Insert` | `Cmd+C` |
| Recortar | `Ctrl+X`, `Shift+Delete` | `Cmd+X` |
| Colar | `Ctrl+V`, `Ctrl+Shift+V`, `Shift+Insert` | `Cmd+V` |

`Ctrl+Shift+C`/`V` entram porque são os atalhos de copiar e colar do terminal (`clipboard.copy`/`clipboard.paste`, [ADR-0008](0008-teclas-e-roteamento-de-input.md)): quem cola no terminal com eles cola no campo com eles, sem pensar. `Ctrl+Insert`/`Shift+Insert`/`Shift+Delete` são a convenção antiga do Windows e do X11, que o campo mascarado já aceita.

São **teclas próprias do modo de captura** (passo 1 da cadeia do ADR-0008), como `Ctrl+A` e as setas do ADR-0035: não são ações do catálogo, não entram em `[keybindings]` e não são configuráveis. Por isso não ferem a regra do ADR-0008 de que "nenhum default usa `Ctrl+<letra>` sozinho": a regra é sobre o mapa de teclas, e estas só existem enquanto um campo tem o foco. Fora do campo, `Ctrl+C` continua sendo do terminal.

No macOS, `Cmd`+letra **nunca** digita a letra no campo, reconhecida ou não — a guarda que `masked_field.rs` já tem e `apply_text_field_key` não tem.

### 2. O que cada gesto faz

- **Copiar** escreve a seleção no clipboard. Sem seleção, não faz nada — não copia o campo inteiro.
- **Recortar** copia a seleção e a apaga. Sem seleção, não faz nada.
- **Colar** insere o texto do clipboard no cursor, substituindo a seleção quando há uma, e deixa o cursor no fim do que foi colado. Clipboard vazio, sem texto ou ilegível: não faz nada, sem aviso.
- **A tecla é consumida mesmo quando não faz nada.** Com um campo focado, nenhuma dessas combinações chega ao terminal ou ao mapa de teclas.

### 3. Colar numa linha só

Todo campo é de uma linha. O texto colado é normalizado antes de entrar:

- cada quebra de linha (`CRLF`, `LF` ou `CR`) vira **um espaço**;
- cada `Tab` vira **um espaço**;
- os demais caracteres de controle são descartados — o mesmo filtro que o texto digitado já passa;
- num campo com teto de comprimento (o nome de sessão, [PRD-014](../prd/prd-014-sessoes-nomeadas.md) RF-14.3), o excedente é cortado em fronteira de caractere, sem aviso — a mesma regra de "o campo para de aceitar texto" do RF-14.3.

O colar não filtra pelo tipo do campo: o campo numérico da tela de configurações recebe o texto como recebe o digitado, e a validação dele é a de sempre, no confirmar.

### 4. O núcleo não fala com o clipboard

`TextFieldState` continua puro e testável sem `winit` nem `arboard`. Ganha o que lhe falta — o texto selecionado, recortar a seleção, inserir uma string já normalizada —, e `apply_text_field_key` passa a devolver um resultado que distingue **copiar**, **recortar** e **colar** de "tratado" e "não tratado", no padrão do `MaskedKey::Paste` que já existe.

Quem chama faz a leitura e a escrita por `porecatu-ui/src/clipboard.rs`, que continua sendo **o único ponto do `arboard`** na UI ([ADR-0013](0013-mouse-selecao-e-clipboard.md) §Clipboard). A tradução "resultado → clipboard → campo" é **uma função só**, chamada por todos os modos de captura que têm campo — não uma cópia em cada um, pela armadilha da fórmula copiada.

### 5. A busca reivindica as teclas de clipboard

O conjunto que a busca consome (ADR-0041 §3) ganha as teclas da §1, **inclusive `Ctrl+Shift+C`/`V`**. Com a busca focada:

- `Ctrl+C` copia a seleção do campo, ou não faz nada — **nunca** vai ao processo;
- `Ctrl+Shift+V` cola no campo, não no terminal;
- para interromper o processo, o usuário fecha a busca com `Esc` e aperta `Ctrl+C`.

O resto do teclado continua caindo para o passo 2: trocar de aba, abrir aba e rolar o scrollback seguem funcionando com a busca aberta. É a mesma regra do ADR-0041 — a busca consome por inteiro o que reivindica —, com um conjunto maior.

### 6. O campo mascarado não muda

Copiar e recortar continuam sem efeito no `MaskedFieldState` ([ADR-0075](0075-superficies-de-ui-do-ssh.md) §1, [PRD-020](../prd/prd-020-conexoes-ssh.md) RF-20.35), e colar continua funcionando, agora pelas mesmas teclas da §1. Vale para o diálogo de segredo do SSH e para o token do relay ([ADR-0108](0108-superficies-de-ui-do-acesso-remoto.md)). O colar no campo mascarado segue a normalização da §3.

### 7. Menu de contexto do campo

Clique direito num campo abre um menu com a anatomia do menu de contexto da especificação visual (§2.16) — mesmo widget, mesmas chaves de `[appearance.context_menu]`, **nenhum valor novo** e nenhum widget novo para o [ADR-0014](0014-superficie-de-aviso-e-dialogo.md):

- itens, na ordem: **Recortar**, **Copiar**, **Colar**, divisor, **Selecionar tudo**;
- **sem chip de tecla**, como o menu do terminal, que também tem copiar e colar;
- Recortar e Copiar ficam **esmaecidos, nunca ausentes**, sem seleção ([PRD-010](../prd/prd-010-interacao-e-superficie-de-app.md) RF-10.20), e sempre no campo mascarado;
- Colar fica sempre habilitado: o menu **não lê o clipboard ao abrir** só para decidir um estado de pintura — colar com o clipboard vazio não faz nada, pela §2;
- o clique direito **dentro** da seleção a mantém; **fora** dela, colapsa a seleção e põe o cursor no ponto, como o clique esquerdo do ADR-0035 §3;
- o menu fecha com `Esc`, clique fora ou perda de foco, como os outros menus; o `Esc` que fecha o menu **não** cancela o campo nem fecha a superfície onde ele está;
- acionar um item faz exatamente o que a tecla correspondente faz, pela função única da §4.

Vale em todo campo: rename de aba, editor de grupo, popover de sessões, barra de busca, diálogo de segredo, campos da tela de configurações e do editor de perfil, e o token do relay. Na tela de configurações e no editor de perfil, o menu é desenhado na própria janela, como a lista de escolha (`settings/choice_list.rs`) já desenha a anatomia do menu numa janela que não é de terminal.

Os textos dos itens vêm do catálogo ([ADR-0056](0056-catalogo-de-textos-da-interface.md)), numa seção própria do menu do campo, em todo arquivo de `locales/`. "Recortar" é texto novo; os outros três repetem palavras de menus existentes, com identificador próprio, para que cada menu possa mudar sem arrastar o outro.

### 8. Como entra nos documentos

ADRs, PRDs, o catálogo de ações e o roadmap registram a decisão **agora**, marcados como "a implementar". A especificação visual e o guia do usuário descrevem o que o binário faz ([ADR-0028](0028-o-binario-como-referencia-visual.md)), e por isso mudam **no PR do código**: a §2.16 ganha o menu do campo, a tabela de fases ganha a linha dele, o histórico §4.4 registra a decisão com o aval visual do dono pendente, e o guia ganha uma seção sobre editar texto nos campos. A mudança nas seções 1/2 da especificação é a que o [ADR-0032](0032-interface-do-v1-fechada.md) exige ADR para fazer, e é este.

## Alternativas consideradas

### Na busca, `Ctrl+C` sem seleção ir ao processo

É o "copiar ou interromper" de alguns terminais. Recusada por decisão do dono do produto: o mesmo gesto teria dois efeitos conforme um estado que o usuário mal vê (há seleção no campo?), e o efeito errado é interromper um processo. Com o campo consumindo sempre, o pior caso é não acontecer nada, e `Esc` é uma tecla.

### Só `Ctrl+C`/`X`/`V`

Mais curto. Recusada: `Ctrl+Shift+V` com a busca aberta continuaria colando no terminal, e quem tem os atalhos do terminal na mão erraria nos campos.

### Teclas do campo configuráveis em `[keybindings]`

Daria controle ao usuário. Recusada: o mapa de teclas não é consultado nos modos de captura (ADR-0008, ADR-0059 §3), e nenhuma outra tecla de edição de campo é configurável. Abrir essa porta para três teclas criaria uma exceção sem pedido.

### Copiar o campo inteiro quando não há seleção

Comum em alguns editores para a linha. Recusada: nenhum campo de texto de sistema faz isso, e copiar sem seleção sobrescreveria o clipboard sem o usuário ter escolhido o quê.

### Descartar as quebras de linha, ou colar só a primeira linha

Recusadas por decisão do dono do produto. Descartar cola palavras umas nas outras; cortar na primeira linha perde texto em silêncio. Espaço preserva a separação e não perde nada.

### O menu ler o clipboard para esmaecer o Colar

Mais informativo. Recusada: seria I/O de clipboard só para pintar um item, a cada abertura do menu, e no Wayland o clipboard é o ponto frágil do `arboard`. Colar com o clipboard vazio já não faz nada.

### Chip de tecla nos itens do menu

A §2.16 da especificação prevê o chip. Recusada para este menu pela coerência com o menu do terminal, que tem os mesmos verbos e não mostra chip; e o chip mostraria uma tecla entre três equivalentes.

## Consequências

### Positivas

- Copiar, recortar e colar funcionam em todos os campos com o mesmo comportamento, porque todos passam pelo mesmo núcleo.
- A busca deixa de interromper o processo quando o usuário tenta copiar do campo, e deixa de colar no terminal.
- `TextFieldState` continua puro e testável; o clipboard continua num ponto só.
- O campo mascarado não muda de regra, só ganha o menu.

### Negativas

- Com a busca aberta, `Ctrl+C` deixa de chegar ao processo. Quem usava isso para interromper sem fechar a busca precisa fechá-la antes. É o custo aceito na decisão 5.
- O menu do campo é mais um consumidor do widget de menu de contexto, e o primeiro nas duas janelas filhas que não é uma lista de escolha.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| No macOS, o `winit` entregar a letra no texto do evento com `Cmd` e o campo digitá-la | Média | Médio | Guarda explícita: `Cmd`+letra nunca chega ao ramo "digitar" (§1); teste de unidade com o evento montado à mão. Verificação ao vivo no macOS é dívida — não há Mac no fluxo |
| Clipboard no Wayland falhar | Média | Baixo | Já é armadilha conhecida do `arboard`, encapsulada em `clipboard.rs`; falha de leitura é "não faz nada" (§2) |
| Algum modo de captura esquecer de chamar a função única da §4 e o campo ficar sem clipboard | Média | Médio | Uma função só; teste por modo de captura que monta `Ctrl+V` e confere o texto inserido, com o clipboard injetado |
| Texto enorme colado num campo sem teto | Baixa | Baixo | O campo é de uma linha e o render já recorta; o teto, onde existe, vale (§3) |
| Verificação só no Windows | Alta | Médio | Teste automatizado do núcleo e dos chamadores; Linux, macOS e leitor de tela ficam como dívida no roadmap |
