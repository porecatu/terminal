# ADR-0015 — Múltiplas janelas no v1, em escopo mínimo

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** ADR-0005, ADR-0006, ADR-0007, ADR-0008, ADR-0014, PRD-001, PRD-003

## Contexto

Três documentos aprovados pressupõem que o Porecatu tem mais de uma janela:

- [ADR-0006](0006-modelo-de-abas-e-grupos.md): *"`Workspace` é por janela. Cada janela tem seus grupos e abas."*
- [ADR-0005](0005-persistencia-de-sessao.md): o arquivo de sessão grava, **por janela**, geometria, monitor, grupos e aba ativa.
- PRD-003 RF-3.17: *"múltiplas janelas são gravadas e restauradas como um conjunto."* PRD-001 RF-1.4 fala em fechar a janela e em *"fechar a última janela"*.

E nada cria a segunda janela. Não existe ação, não existe keybinding, não existe RF, não existe cenário de aceite. O RF-3.17 restaura um conjunto de janelas que o usuário não tem como produzir.

> **Nota de reconciliação (2026-08-27).** As duas afirmações desta seção sobre o estado da documentação já foram fechadas, pela própria decisão deste ADR: o [PRD-010](../prd/prd-010-interacao-e-superficie-de-app.md) formalizou os RF-10.22 a RF-10.24, e o [`porecatu.example.toml`](../config/porecatu.example.toml) já traz `confirm_close_window` etiquetada com `RF-10.23`. O texto abaixo é preservado como o registro do problema que motivou a decisão, não como descrição do estado atual.

O sintoma menor dessa lacuna já está visível no [`porecatu.example.toml`](../config/porecatu.example.toml): a chave `confirm_close_window` é a única do arquivo sem etiqueta de origem, porque não há requisito a que ela se ligue. Isso contradiz a métrica do PRD-004 — *"chaves no arquivo de exemplo sem requisito correspondente: zero"*.

O sintoma maior é técnico, e é o que torna este ADR necessário em vez de opcional.

### O `Wakeup` é ambíguo com mais de uma janela

O [ADR-0006](0006-modelo-de-abas-e-grupos.md) define a identidade das abas assim: *"`TabId` e `GroupId` são inteiros opacos e estáveis, gerados por contador monotônico **por workspace**"*. Como `Workspace` é por janela, dois workspaces geram a mesma sequência: a primeira aba de cada janela é `TabId(1)`.

O [ADR-0007](0007-modelo-de-threading.md) define o evento que a thread de leitura envia à UI como `Wakeup(tab_id)`, e a regra de damage: *"marca a aba como suja; se não é a visível, para aí"*.

Com duas janelas abertas, `Wakeup(TabId(1))` não diz qual aba ficou suja. O resultado não é um crash — é pior: a janela errada redesenha, e a aba que realmente recebeu saída não. Um bug de render que só aparece com duas janelas e some quando se fecha uma delas.

Isso precisa ser resolvido **antes** da F1, porque o tipo do evento atravessa a fronteira entre `porecatu-pty`, `porecatu-term` e o binário. Descobrir na F5, quando a restauração de sessão finalmente abrir duas janelas, significa mexer no caminho quente já construído.

## Decisão

**Múltiplas janelas entram no v1, em escopo mínimo.**

### Escopo

| No v1 | Fora do v1 |
|---|---|
| Ação `window.new` e `window.close` | Arrastar aba entre janelas |
| Um `Workspace` independente por janela | Mover grupo entre janelas |
| Barra de abas, surface `wgpu` e foco próprios por janela | Janela sem abas |
| Gravação e restauração do conjunto (RF-3.17) | Menu de janelas |

Arrastar aba entre janelas continua **fora** — é não-objetivo explícito do PRD-000 e do PRD-001, e nada aqui o reabre. O [ADR-0006](0006-modelo-de-abas-e-grupos.md) já observa que o modelo não impede (*"é `move_tab` com destino em outro `Workspace`"*); continua sendo verdade e continua sendo v2.

### Ações e defaults

Entram no catálogo fechado do [ADR-0008](0008-teclas-e-roteamento-de-input.md):

| Ação | Windows / Linux | macOS |
|---|---|---|
| `window.new` | `Ctrl+Shift+N` | `Cmd+N` |
| `window.close` | `Ctrl+Shift+Q` | `Cmd+Shift+W` |

`Ctrl+Shift+N` respeita a regra do ADR-0008 — nada de `Ctrl+<letra>` sozinho — e é a convenção de "nova janela" em navegador. No macOS, `Cmd+N` para nova janela e `Cmd+T` para nova aba é a convenção da plataforma.

`Ctrl+Shift+Q` para fechar janela em vez de `Ctrl+Shift+W`, que já é `tab.close`. Fechar janela com mais de uma aba passa pelo diálogo de confirmação do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), governado pela chave `confirm_close_window` — que assim ganha a origem que lhe faltava.

### Janela nova herda o diretório

A janela nova abre com uma aba no `cwd` da aba ativa no momento da criação, pela mesma razão registrada no RF-1.1 para abas: *"herdar o diretório é o comportamento que economiza mais digitação no dia a dia."* Sem aba ativa — primeiro start sem sessão —, vale `startup_directory`.

### Refino do ADR-0007: o payload do `Wakeup`

**Esta subseção refina um ADR aceito sem mudar a decisão dele**, e por isso não há `Supersedes`.

O evento passa a carregar o par:

```rust
Wakeup { window: WindowId, tab: TabId }
```

O que **não** muda:

- Os contadores de `TabId` e `GroupId` seguem sendo por workspace (ADR-0006 intacto). Tornar os IDs globais ao processo seria mudar a decisão de identidade, não refiná-la — e não é necessário, porque o par já é único.
- A decisão de threading segue intacta (ADR-0007): uma thread de leitura por terminal, `Mutex<Term>` segurado só no `advance`, damage-driven, coalescing por intervalo de frame. Só o payload do evento ganha um campo.
- `Workspace` continua sendo mutado exclusivamente pela main thread, sem lock — agora são N `Workspace`, todos com o mesmo dono.

`WindowId` é o identificador da janela do `winit`, propagado à thread de leitura no spawn do terminal.

Consequência para a marcação de sujeira: ela passa a ser por janela. Saída numa aba não visível da janela B não redesenha a janela A, o que preserva a propriedade central do ADR-0007 — janela sem mudança não gera frame.

### Sessão

Nada muda no [ADR-0005](0005-persistencia-de-sessao.md): o formato já é uma lista de janelas. O RF-3.17 passa a ter quem produza o dado que ele restaura, e **nenhum PRD precisa ser superseded** — que é justamente o motivo de trazer multi-janela para o v1 em vez de removê-la.

Fechar a última janela encerra o app com gravação síncrona da sessão, como o RF-1.4 determina.

### Event loop

Um único event loop na main thread atende todas as janelas — é o modelo do `winit` e não há alternativa em macOS ([ADR-0007](0007-modelo-de-threading.md)). Cada janela tem sua surface `wgpu` e seu swapchain; o atlas de glyphs é compartilhado, porque as métricas de fonte são as mesmas e duplicar atlas por janela desperdiça VRAM sem motivo.

## Alternativas consideradas

### Janela única no v1

Mais barato de implementar: uma surface, um `Workspace`, nenhuma ambiguidade de `Wakeup`.

Descartada pelo custo documental, que é maior que o de implementação. Exigiria superseder o RF-3.17, revisar o RF-1.4 (que fala em "a última janela"), revisar o ADR-0005 (formato com lista de janelas) e o ADR-0006 (`Workspace` por janela) — quatro documentos aprovados alterados para remover uma capacidade que o modelo já suporta. E deixaria o produto atrás do concorrente mais simples: emulador com abas que não abre uma segunda janela é limitação que o usuário percebe no primeiro dia.

### Multi-janela completa, com arrastar aba entre janelas

Fecharia o assunto de uma vez, e o modelo de dados permite.

Descartada por contrariar não-objetivo explícito de dois PRDs aprovados (PRD-000 e PRD-001, ambos registrando *"o modelo permite; a UI fica para depois"*). O custo real não é o `move_tab` — é a UI de arraste atravessando fronteira de janela, com drop em janela não focada e feedback visual entre superfícies distintas. Escopo de v2, com ADR próprio se necessário.

### `TabId` global ao processo em vez do par `(WindowId, TabId)`

Resolveria a ambiguidade do `Wakeup` sem tocar no payload.

Descartada porque mudaria a decisão do ADR-0006 — *"contador monotônico por workspace"* — em vez de refiná-la, e o processo do projeto não permite editar decisão aceita. Também não traz ganho: o par já é único, e IDs por workspace mantêm o round-trip de sessão por janela simples de ler e de testar.

### Deixar o `Wakeup` ambíguo e resolver por busca

A main thread poderia varrer os workspaces procurando o `TabId`.

Descartada por ser incorreta, não só ineficiente: com IDs colidentes, a busca acha a primeira janela que tenha aquele `TabId` — que pode ser a errada. Um bug silencioso de render em troca de um campo no evento.

## Consequências

### Positivas

- RF-3.17 deixa de ser requisito órfão; ADR-0005 e ADR-0006 passam a descrever algo alcançável.
- `confirm_close_window` ganha origem, fechando a última chave órfã do arquivo de exemplo.
- O `Wakeup` ambíguo é corrigido antes de existir código, não depois da F5.
- Usuário ganha a separação por janela que o PRD-000 cita como recurso dos concorrentes (*"a única separação disponível hoje é abrir outra janela"*) — agora somada aos grupos, não em lugar deles.

### Negativas

- Mais estado por janela: surface, swapchain, sujeira, foco. A F2 cresce.
- Multi-monitor com escalas de DPI diferentes passa a ser cenário real de teste, não hipótese.
- Um `Workspace` por janela significa que o mesmo grupo nomeado pode existir em duas janelas sem relação nenhuma. Aceito: grupos são por janela por decisão do ADR-0006.
- Duas janelas competem por config e tema globais; o hot reload precisa marcar todas as janelas como sujas.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Alguém implementar `Wakeup(tab_id)` seguindo o ADR-0007 ao pé da letra | Alta | Alto | Este ADR é referenciado no ADR-0007 pela tabela do índice e citado no roadmap da F1 |
| Escala de DPI diferente por monitor quebrar métricas de fonte | Média | Médio | Métricas recalculadas por janela; teste manual com dois monitores no critério de saída da F2 |
| Atlas de glyphs compartilhado entre janelas com DPI distinto | Média | Médio | Chave do atlas inclui a escala; se virar problema, atlas por escala, não por janela |
| Fechar a última janela perder a sessão | Baixa | Alto | Gravação síncrona no encerramento, já exigida pelo RF-1.4 e pelo RF-3.4 |
| Escopo de multi-janela crescer para arraste entre janelas durante a F2 | Média | Médio | Tabela de escopo neste ADR; arraste entre janelas exige ADR novo |
