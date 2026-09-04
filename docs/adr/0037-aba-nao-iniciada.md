# ADR-0037 — Aba não iniciada: o terceiro estado do ciclo de vida

**Status:** Aceito
**Data:** 2026-09-04
**Relacionados:** [ADR-0005](0005-persistencia-de-sessao.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0019](0019-tooltip.md), [ADR-0020](0020-grupos-explicitos.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md), [ADR-0036](0036-formato-do-arquivo-de-sessao.md), PRD-001, PRD-003
**Supersedes:** [ADR-0017](0017-ciclo-de-vida-da-aba.md) (parcial — a seção 6, que enumerava os estados da aba como se `Running` e `Exited` fossem os dois possíveis)

## Contexto

O RF-3.8 pede restauração **preguiçosa**: ao restaurar uma sessão, só a aba ativa de cada janela tem o shell iniciado; as demais aparecem na barra com título e grupo, e sobem ao serem focadas pela primeira vez. É o que permite restaurar cinquenta abas rápido em vez de disparar cinquenta processos de uma vez, e é a mitigação que o próprio ADR-0005 registrou na tabela de riscos.

Isso é um estado de aba, e ele não existe. `TabState` tem `Running` e `Exited`, e o ADR-0017 §6 os tratou como a enumeração completa. Uma aba restaurada sem shell não é nenhum dos dois: não tem PTY como a `Exited`, mas vai ter um, e o título dela não é o congelado de um processo morto — é o que a sessão gravou.

O estado novo cruza com cinco decisões já aceitas, e cada cruzamento tem uma resposta errada plausível:

- **[ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md)** decide confirmar o fechamento quando há processo ativo. Uma aba sem PTY não tem processo — mas a contagem que o ADR-0034 usa não tem como distinguir "zero processos porque nada subiu" de "zero processos porque a consulta falhou".
- **RF-1.20 e RF-1.21** (atividade e campainha) pressupõem saída possível. O ADR-0017 §6 já excluiu a aba `Exited` desses indicadores pela mesma razão.
- **[ADR-0019](0019-tooltip.md)** mostra o título truncado; o título de uma aba não iniciada existe e é o gravado.
- **[ADR-0020](0020-grupos-explicitos.md)** define a escada de foco e a navegação; se "focar" é o que inicia o shell, é preciso dizer se passar por uma aba conta como focar.
- **RF-3.9** exige que a aba não iniciada seja visualmente distinguível "discretamente, sem poluir a barra" — o **único** requisito do v1 que ainda constava da lista de pendências de desenho da especificação visual (§4.2). E o [ADR-0032](0032-interface-do-v1-fechada.md) fechou a interface: mexer nas seções 1 ou 2 da especificação exige ADR.

Sem esta decisão, cada um desses cruzamentos seria resolvido na implementação, em cinco lugares diferentes, por quem estivesse escrevendo a linha.

## Decisão

### 1. `TabState::NotStarted`

`porecatu-core` ganha um terceiro estado. A enumeração completa passa a ser `NotStarted`, `Running` e `Exited`, e as transições possíveis são duas: `NotStarted -> Running` no primeiro foco, e `Running -> Exited` quando o processo morre. **Não há volta**: aba `Exited` não renasce, e aba `Running` não regride.

`NotStarted` só é criada pela restauração de sessão. Aba nova por `tab.new`, `group.new_tab` ou `window.new` nasce `Running` — o usuário pediu um terminal e vai ver um terminal.

### 2. O shell inicia no foco, e passar por uma aba é focar

O shell sobe quando a aba se torna a **aba ativa** da janela, por qualquer caminho: clique, `Ctrl+Tab`, `Ctrl+PageDown`, índice do RF-1.12, `step_group` do RF-2.21, expansão de grupo colapsado que a torne ativa, ou a restauração que a escolheu como ativa no start.

Não há "passar por cima sem iniciar". Navegar por `Ctrl+Tab` mostra o conteúdo da aba na tela; uma aba ativa mostrando uma área vazia porque o gesto foi rápido demais seria um estado que o usuário não pediu e não entende. Quem passa por dez abas com `Ctrl+Tab` sobe dez shells, e isso é o comportamento correto — o custo do lazy restore é no start, não na navegação.

### 3. Sem processo é sem processo, e o fechamento não confirma

Aba `NotStarted` fecha **sem diálogo**. Não há processo a perder, não há trabalho a interromper, e confirmar seria pedir permissão para descartar nada.

Isso não é uma exceção dentro da detecção do [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md): é uma decisão **antes** dela. A aba `NotStarted` não tem `ProcessGroup`, então a pergunta não chega a ser feita — o que evita justamente o caso ruim, que seria uma consulta devolvendo zero e a UI não sabendo se zero é "nada subiu" ou "a consulta falhou". Vale igual para o fechamento de janela e para `app.quit`: aba `NotStarted` não conta na decisão de confirmar.

### 4. Indicadores, tooltip e título

- **Atividade e campainha (RF-1.20, RF-1.21): não se aplicam.** Sem PTY não há saída, pelo mesmo raciocínio que o ADR-0017 §6 usou para a `Exited`. Vale também para o indicador agregado do grupo colapsado (RF-2.16): aba `NotStarted` não contribui.
- **Título:** o customizado gravado quando houver; senão o `shell_name`. Ela não tem `process_title` — o [ADR-0036](0036-formato-do-arquivo-de-sessao.md) não grava esse campo, e não haveria processo para produzi-lo.
- **Tooltip:** funciona normalmente. O título existe e trunca como qualquer outro.
- **Menu de contexto, rename, arraste, seleção múltipla, agrupamento:** tudo normal. A aba é uma aba; ela só não tem processo ainda.
- **Contagem e índice:** conta como qualquer aba, na navegação e no RF-1.12.

### 5. Aparência: rótulo esmaecido (RF-3.9)

**O rótulo da aba não iniciada é desenhado com alfa `.45`**, sobre a cor de texto do estado de base (ativa ou inativa, §2.5 da especificação visual). Nada mais muda: mesmo fundo, mesma borda, mesma largura, mesma cápsula de grupo atrás.

Três razões, nesta ordem:

1. **Não é elemento novo.** O ADR-0032 fechou a interface do v1; um indicador novo na aba abriria um sexto significado no mesmo espaço onde já vivem atividade, campainha e seleção. Alfa no texto que já está lá não ocupa espaço nenhum.
2. **Não é valor novo.** `.45` já está na tabela de tokens da seção 1, usado na borda do indicador agregado. Nenhuma cor, dimensão ou alfa inventado — a regra que não cai.
3. **Ele se apaga sozinho.** No instante em que o shell sobe, o alfa volta a 1 porque o estado mudou. Não há nada a limpar, e nenhum caminho em que o indicador sobreviva ao fato que ele indicava.

"Discretamente, sem poluir a barra" é literal: com dez abas restauradas, a barra tem dez rótulos mais claros, não dez marcas novas.

`selected_border` continua por cima — uma aba `NotStarted` pode estar selecionada, e a borda é modificador de estado, não estado (§2.5).

### 6. `lazy_restore = false`

Com a chave desligada, a restauração inicia o shell de **todas** as abas no start e nenhuma nasce `NotStarted`. O estado continua existindo no modelo; simplesmente ninguém o produz. É o que mantém a chave como uma decisão de start, sem um segundo caminho de código para manter.

## Alternativas consideradas

### Reusar `Exited` com um sinalizador de "ainda não subiu"

Zero mudança de enumeração. Descartada: `Exited` não aceita input e tem título congelado por decisão do ADR-0017 §6; `NotStarted` aceita tudo assim que sobe. Um `bool` ao lado de um estado que significa o oposto é a forma clássica de um bug de ordem de checagem.

### Iniciar o shell ao passar por cima, mas só depois de N milissegundos parado

Evitaria subir dez shells num `Ctrl+Tab` rápido. Descartada: introduz um temporizador e um estado intermediário ("ativa, sem shell, esperando") só para otimizar um gesto que ninguém repete, e o usuário veria a área do terminal vazia durante a espera. O projeto já tem temporizador de UI demais para acrescentar um sem requisito.

### Indicador próprio na aba para o RF-3.9

Um ponto vazado, na família da §2.17. Foi a alternativa levada ao dono do produto junto com o rótulo esmaecido, e a recusada: acrescenta um quarto significado ao mesmo slot visual que já carrega atividade, campainha e o agregado de grupo colapsado, e o RF-3.9 pede o oposto de proeminência.

### Aba não iniciada sem fundo, só com a borda

Também levada ao dono do produto e recusada. É muito visível com muitas abas restauradas — que é exatamente o caso comum do recurso —, e vira o efeito contrário de "sem poluir a barra".

## Consequências

### Positivas

- O RF-3.8 fica implementável sem gambiarra de estado, e o RF-3.9 sai da lista de pendências de desenho da §4.2 — que passa a ficar **vazia**.
- A restauração de 20 abas em menos de 1 s (métrica do PRD-003 e critério de saída da F5) passa a depender de um shell por janela, não de vinte.
- Fechar uma sessão restaurada inteira não dispara diálogo nenhum, porque não há processo nenhum.
- Nenhum pixel novo: a mudança é um alfa sobre um texto que já era desenhado.

### Negativas

- `TabState` deixa de ser binário, e todo `match` sobre ele no código existente precisa considerar o terceiro braço. É trabalho mecânico, e o compilador o encontra — desde que ninguém tenha escrito um `_ =>`.
- Navegar rápido por muitas abas restauradas sobe muitos shells de uma vez, o que é o comportamento decidido mas não é o mais barato possível.
- O alfa do rótulo é sutil por construção; num monitor mal calibrado a distinção pode passar despercebida. Aceito: o requisito pede discrição, e o custo de errar é o usuário clicar numa aba e ela subir, que é o que ele queria de qualquer forma.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Um `match` com braço curinga esconder o estado novo e tratá-lo como `Running` | Média | Alto | Auditar os `match` sobre `TabState` na etapa; nenhum braço curinga novo sobre esse tipo |
| Aba `NotStarted` receber tecla e o input sumir | Média | Médio | O foco **inicia** o shell antes de qualquer roteamento de input; não existe janela de tempo em que a aba ativa esteja `NotStarted` |
| Consulta de processo ser feita para aba sem `ProcessGroup` e devolver zero ambíguo | Baixa | Médio | A pergunta não é feita: o estado é checado antes (§3) |
| Alfa `.45` ficar indistinguível do texto da aba inativa (`#98a0ab`) | Média | Baixo | O contraste é entre a aba e as vizinhas no mesmo estado, não contra o fundo; verificável por medição de pixel, como o hover por brilho da F4 |
