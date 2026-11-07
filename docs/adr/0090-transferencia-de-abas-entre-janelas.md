# ADR-0090 — Transferência de abas e grupos entre janelas: extrair e inserir no core, endereço do wakeup numa célula do painel, `TabId` novo no destino

**Status:** Aceito
**Data:** 2026-11-07
**Relacionados:** [ADR-0006](0006-modelo-de-abas-e-grupos.md), [ADR-0007](0007-modelo-de-threading.md), [ADR-0015](0015-multiplas-janelas.md), [ADR-0020](0020-grupos-explicitos.md), [ADR-0021](0021-selecao-multipla-e-gestos-da-barra.md), [ADR-0023](0023-editor-de-grupo.md), [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0036](0036-formato-do-arquivo-de-sessao.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0051](0051-arquivo-de-projeto-porecatu.md), [ADR-0053](0053-paineis-divididos.md), [ADR-0071](0071-backend-de-terminal.md), [ADR-0076](0076-reconexao-automatica.md), [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md), [ADR-0091](0091-arraste-entre-janelas.md), [PRD-022](../prd/prd-022-abas-e-grupos-entre-janelas.md)
**Supersedes (parcial):** ADR-0015 "Escopo" (as linhas "Arrastar aba entre janelas" e "Mover grupo entre janelas" saem da coluna "Fora do v1") e ADR-0015 "Refino do ADR-0007" (o `WindowId` do wakeup deixa de ser fixo no spawn) · ADR-0086 §2 (ordem da lista quando uma fixada muda de janela)

## Contexto

O [PRD-022](../prd/prd-022-abas-e-grupos-entre-janelas.md) pede que abas, seleções, grupos e fixadas mudem de janela **sem reiniciar nada** (RF-22.2). O [ADR-0006](0006-modelo-de-abas-e-grupos.md) dizia que isso era só *"`move_tab` com destino em outro `Workspace`"*. No modelo de dados, era mesmo. O código que cresceu em volta do modelo, porém, prendeu a aba à janela em quatro lugares, e cada um tem uma resposta errada tentadora:

1. **A identidade colide.** `TabId` e `GroupId` são contadores monotônicos **por workspace** (`Workspace::next_tab_id`, ADR-0006, mantido pelo [ADR-0015](0015-multiplas-janelas.md)), e a primeira aba de toda janela é `TabId(0)`. A aba que chega com o id que tinha na origem pode colidir com uma aba do destino. A tentação é tornar os ids globais ao processo, e o ADR-0015 já recusou isso uma vez.
2. **O wakeup está gravado no spawn.** `WindowState::spawn_tab_runtime` monta o closure que a thread de leitura chama a cada saída, e o closure captura `Wakeup::TabDirty { window, tab, pane }` por valor. `Terminal` guarda esse closure (`on_wakeup`) e não tem como trocá-lo. Movida a aba, a saída dela continuaria acordando a janela de origem, e a janela onde ela agora está não redesenharia. É o mesmo bug silencioso de render que o ADR-0015 corrigiu antes da F1.
3. **O estado de runtime está espalhado e chaveado por `TabId`.** Os runtimes ficam em `WindowState.panes: HashMap<(TabId, PaneId), PaneRuntime>`. Na janela há também `pending_prompts`, `notices`, a busca, as animações e a árvore de acessibilidade. Fora dela, há chaves com `WindowId` dentro: `App.reconnects` (`PaneKey = (WindowId, TabId, PaneId)`, [ADR-0076](0076-reconexao-automatica.md)), `PinnedRegistry.live` ([ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md) §2), `pending_shell_integration_note` e `pending_resumed_metric`. Uma transferência que esquece uma dessas chaves deixa um órfão, como uma reconexão que procura o painel na janela errada ou uma fixada que o registro acha estar aberta onde não está.
4. **Falta a operação no core.** `porecatu-core` sabe mover dentro de um workspace (`move_tab_to_group`, `move_tab_to_new_run`, `move_group`, `pin_tab`, `unpin_tab`). Para fora, só sabe **fechar** (`close_tab`), e `Group::into_tabs` é `pub(crate)`. A peça mais próxima de "chegar" é `insert_tab_with_panes`, da restauração, que já cria a aba com um `TabId` novo.

Este ADR decide o modelo e a transferência, que servem igualmente ao arraste e ao popover. O gesto de arraste é do [ADR-0091](0091-arraste-entre-janelas.md).

## Decisão

**A aba sai de um workspace e entra em outro por um par de operações puras de `porecatu-core`, extrair e inserir. A aba extraída não leva `TabId` e ganha um novo no destino. Os runtimes vão junto, re-chaveados por uma única função de `App`. O endereço que a thread de leitura usa para acordar a UI passa a viver numa célula do painel, reescrita pela main thread na transferência. Os contadores por workspace, o payload do `Wakeup` e `porecatu-term` não mudam.**

### 1. Core: extrair e inserir

```rust
/// Uma aba fora de qualquer workspace. Não tem `TabId`: quem a recebe dá um.
pub struct DetachedTab {
    tab: Tab,               // título customizado, árvore de painéis, estado
    // o `TabId` original fica só no mapa devolvido por quem extrai
}

/// Um grupo explícito fora de qualquer workspace.
pub struct DetachedGroup {
    meta: GroupMeta,        // nome, cor, colapso
    tabs: Vec<DetachedTab>, // na ordem do grupo
    last_active: Option<usize>, // índice em `tabs`, não `TabId`
}

impl Workspace {
    pub fn take_tabs(&mut self, ids: &[TabId]) -> Vec<(TabId, DetachedTab)>;
    pub fn take_group(&mut self, id: GroupId) -> Option<(DetachedGroup, Vec<TabId>)>;
    pub fn take_pinned(&mut self, id: TabId) -> Option<DetachedTab>;

    pub fn insert_detached_tabs(&mut self, tabs: Vec<DetachedTab>, at: InsertAt) -> Vec<TabId>;
    pub fn insert_detached_group(&mut self, group: DetachedGroup, pos: usize) -> (GroupId, Vec<TabId>);

    /// O mesmo movimento dentro de um workspace só (arraste da seleção,
    /// ADR-0091 §6): extrai e reinsere **preservando** os `TabId`.
    pub fn move_tabs(&mut self, ids: &[TabId], at: InsertAt) -> bool;
}

pub enum InsertAt {
    IntoGroup { group: GroupId, pos: usize },
    NewRun { group_index: usize },
    Pinned { pos: usize },
    TrackEnd,   // o popover (RF-22.23)
}
```

`InsertAt` tem a mesma forma do `DragDrop` que a barra já resolve (`tab_bar::DragDrop`, [ADR-0021](0021-selecao-multipla-e-gestos-da-barra.md) §4, [ADR-0087](0087-zona-de-abas-fixadas-na-barra.md) §5), mais `TrackEnd`. Fica no core porque é argumento de domínio, não de layout, e a UI traduz um no outro.

As regras de saída e chegada são as que o core já tem. Nenhuma é reescrita:

- **Extrair** remove como `close_tab` remove: o grupo que esvazia sai (RF-2.7), os *runs* implícitos vizinhos se fundem (`normalize_groups`) e a escada de foco do [ADR-0020](0020-grupos-explicitos.md) §3, revista pelo ADR-0086 §1, escolhe a nova ativa da origem. A diferença para `close_tab` é que extrair **devolve** a `Tab` em vez de descartá-la, e a UI não encerra nenhum processo.
- **Extrair várias** (`take_tabs`, seleção do RF-22.17) devolve as abas na **ordem visual** da origem, qualquer que seja a ordem de `ids`. É o que torna o bloco contíguo e ordenado no destino.
- **Inserir** atribui `TabId` (e `GroupId`, no grupo) pelos contadores do **destino**. Um grupo que chega mantém nome, cor e colapso, e não se funde com outro de mesmo nome (RF-22.5). Inserir não ativa nada: quem ativa é a UI (§3), porque a regra de foco do RF-22.10 é de interação.
- **Fixada** sai e entra pelas mesmas funções. `take_tabs` recusa uma fixada (`take_pinned` existe para ela), e `InsertAt::Pinned` a põe na zona, como `pin_tab` faz. Uma fixada que chega na trilha é desafixada, e o registro é atualizado pela UI (§5).

Todas com teste que fixa a **ordem resultante** na origem e no destino (regra do [CLAUDE.md](../../CLAUDE.md) desde a F3): extrair do meio de um grupo, extrair a última aba de um grupo, extrair uma seleção espalhada por três grupos, inserir num grupo colapsado, inserir um grupo entre dois grupos. Junto, o teste de invariante do ADR-0086 §1 (toda `TabId` em exatamente um lugar) roda nos **dois** workspaces depois de cada operação.

**A `Tab` não muda.** Os `PaneId` são por aba (o contador `next_pane_id` mora na `PaneTree`, [ADR-0053](0053-paineis-divididos.md)), então viajam intactos dentro da `PaneTree`. Só o `TabId` é trocado.

### 2. Endereço do wakeup numa célula do painel

O closure de wakeup deixa de capturar o endereço por valor. Ele captura uma célula compartilhada com o `PaneRuntime`:

```rust
#[derive(Clone, Copy)]
struct PaneAddress { window: WindowId, tab: TabId, pane: PaneId }

struct PaneRuntime {
    address: Arc<Mutex<PaneAddress>>,   // novo
    terminal: Terminal,
    // ...
}

// em spawn_tab_runtime
let address = Arc::new(Mutex::new(PaneAddress { window, tab, pane }));
let cell = Arc::clone(&address);
Terminal::spawn(spec, params, move || {
    let a = *cell.lock().unwrap_or_else(PoisonError::into_inner); // cópia de 3 inteiros
    let _ = proxy.send_event(Wakeup::TabDirty { window: a.window, tab: a.tab, pane: a.pane });
});
```

A main thread reescreve a célula na transferência. **`porecatu-term` não muda**: o closure sempre foi de `porecatu-ui`, e `Terminal` continua recebendo um `Fn() + Send + Sync` opaco. O payload do `Wakeup` também não muda. O que muda é de onde ele tira os valores.

**Por que isso é correto, e não só provável.** Entre a cópia que a thread de leitura faz da célula e a chegada do evento na main thread, a transferência pode acontecer. O evento chega então com o endereço **antigo**, e há só três casos:

- **A janela de origem fechou** (a aba era a última, RF-22.19): `user_event` não acha a janela e descarta o evento, como já faz hoje.
- **A janela de origem continua aberta:** o `TabId` antigo **não existe mais nela** e **nunca vai existir de novo**, porque o contador é monotônico e não reaproveita id. `panes.get` não acha nada e o evento é descartado.
- **Nos dois casos, o destino foi marcado sujo na chegada (§3).** O primeiro quadro no destino desenha o estado do motor, que já contém aquela saída. Um wakeup descartado não perde quadro, porque a marca de sujeira não é o dado: o dado está no `Term`, e o snapshot é tirado do `Term` no quadro.

O caso residual é o `WindowId` reaproveitado pelo sistema: no Windows, o `WindowId` é o `HWND`, e um `HWND` de janela fechada pode ser reusado por uma janela nova do mesmo processo. Um evento antigo poderia então marcar suja uma aba de mesmo `TabId` na janela nova. O efeito é **um quadro a mais**, nunca um a menos, e a regra do ADR-0007 que importa ("janela sem mudança não gera frame") é violada uma vez, em uma corrida de microssegundos depois de fechar uma janela. Aceito.

**O lock na thread de leitura** é um `Mutex` sem disputa na prática: a main thread o toma uma vez por transferência, e os wakeups já são coalescidos ([ADR-0007](0007-modelo-de-threading.md)). Copiar três inteiros sob um `Mutex` livre custa menos que o `send_event` ao lado. Nenhuma dependência nova (`ArcSwap` não entra).

### 3. A transferência na UI: uma função só

```rust
impl App {
    fn transfer(&mut self, event_loop: &ActiveEventLoop, from: WindowId,
                payload: TransferPayload, to: TransferDest) -> Option<TransferResult>;
}

enum TransferPayload { Tabs(Vec<TabId>), Group(GroupId), Pinned(TabId) }
enum TransferDest   { Window(WindowId, InsertAt), NewWindow(NewWindowPlacement) }
```

É o **único** caminho que move uma aba entre janelas. O arraste (ADR-0091) e o popover (§6) chamam esta função, e nenhum dos dois re-chaveia nada por conta própria. Ela faz, nesta ordem, tudo na main thread e sem quadro no meio:

1. **Extrai** do workspace de origem (§1) e guarda o mapa `TabId antigo → DetachedTab`.
2. **Cria a janela**, quando o destino é `NewWindow`, pelo caminho de `open_window_with` **sem** `open_tab` e sem abrir fixadas (RF-22.14). O `WindowId` existe antes de qualquer aba chegar, como já acontece em `window.new`.
3. **Insere** no workspace de destino e obtém o mapa `TabId antigo → TabId novo`.
4. **Move os runtimes**, re-chaveando: cada `PaneRuntime` sai de `origem.panes[(antigo, pane)]` e entra em `destino.panes[(novo, pane)]`, e a célula `address` dele é reescrita **antes** de qualquer outro passo que possa gerar quadro.
5. **Re-chaveia o estado de aba fora do runtime:**

   | Estado | Onde | O que acontece |
   |---|---|---|
   | Perguntas pendentes (segredo, chave de host) | `pending_prompts` | vão para o destino, com a chave nova (RF-22.3) |
   | Avisos pendentes do painel | `notices` | idem |
   | Ciclo de reconexão | `App.reconnects` (`PaneKey`) | a chave do painel é trocada **dentro** do ciclo, e o ciclo continua de onde estava |
   | Fixada viva | `PinnedRegistry.live` | `(janela, TabId)` novos (§5) |
   | Nota de integração de shell, métrica de retomada | `App.pending_*` | a chave é trocada se aponta para a aba |
   | Comando pendente do `.porecatu` | dentro do `PaneRuntime` | viaja com ele, e nunca é recriado (RF-12.9) |

6. **Descarta** o estado de gesto da origem que aponta para as abas que saíram: rename, busca aberta, menu de contexto, popover, hover, tooltip, animações de deslize e o `drag` já concluído. Diálogo de confirmação não pode estar aberto, porque ele é modal e impede o gesto.
7. **Ativa e foca** no destino (RF-22.10): a aba que o usuário segurou, ou a única que chegou, ou a `last_active` do grupo, passa a ser a ativa do destino. `focus_window()` traz a janela à frente. A seleção é limpa nas duas janelas.
8. **Redimensiona** cada painel que chegou ao quadro do destino pelo caminho de resize de sempre: a grade muda e o backend avisa o programa, ou o canal SSH no caso remoto. Aba `NotStarted` não tem runtime e só muda de lugar. Ela é decidida **antes** de qualquer `panes.get` (armadilha do CLAUDE.md).
9. **Marca sujos** os dois lados: redesenho nas duas janelas, `session_dirty` nas duas (RF-22.25) e a árvore de acessibilidade das duas ([ADR-0043](0043-arvore-de-acessibilidade.md)), cuja raiz de abas mudou.
10. **Fecha a origem vazia** (RF-22.19) pelo caminho de fechar janela **sem** o diálogo do RF-10.23. Como nenhum processo é encerrado, a gravação síncrona do RF-1.4 não se aplica, porque não é a última janela. Uma origem que ainda tem fixada não está vazia (ADR-0086 §1).

A função devolve as `TabId` novas, para quem chamou ajustar o que for dele.

**O que a função recusa**, devolvendo `None` sem efeito: origem igual ao destino (quem move na mesma janela é a operação local), payload vazio, grupo implícito (não tem pílula nem identidade estável, `group.rs`), e destino que é a janela de configurações ou o editor de perfil, que não estão em `App.windows` ([ADR-0059](0059-janela-de-configuracoes.md)).

### 4. Sessão

O formato do `session.json` não muda ([ADR-0036](0036-formato-do-arquivo-de-sessao.md)). A aba é gravada na janela onde está, com o `TabId` que tem lá, e a sessão nunca soube do anterior. As duas janelas ficam sujas e a gravação segue o *debounce* de sempre (RF-3.3). Uma origem que fecha no passo 10 sai da sessão como sai qualquer janela fechada com outras abertas.

### 5. Fixadas

A lista de fixadas é do processo, e a entrada é identificada por `PinnedKey`, não pelo `TabId` (ADR-0086 §2). Por isso a transferência de uma fixada só atualiza `live[key] = (destino, TabId novo)`. O retrato não muda (RF-22.26).

A **ordem** da lista é que pode mudar. Uma fixada que chega na zona de outra janela é tratada, para a lista, como uma fixada nova inserida naquela zona: a regra do ADR-0086 §2 (*"logo depois da vizinha à esquerda na zona; sem vizinha à esquerda, logo antes da primeira entrada daquela janela; sem nenhuma, no fim"*) decide a posição dela, e a entrada sai da posição antiga. Se a posição resultante é a mesma, nada é gravado. Se muda, o `pinned.json` é gravado como em todo reordenar (RF-21.28). Uma fixada que chega na trilha do destino é desafixada como no RF-21.10: sai da lista, e o arquivo é gravado.

### 6. Caminho sem mouse: popover "Mover para janela…"

O popover **não é widget novo**. É o popover de destino de `tab.move_to_group` (`move_to_group.rs`, [ADR-0023](0023-editor-de-grupo.md) §4), com a mesma anatomia (uma linha por destino, rolável, linha final fixa) e outro tipo de destino:

```rust
enum WindowTarget { Window(WindowId), NewWindow }
```

As linhas são as outras janelas de terminal, na ordem de uso mais recente (a mesma ordem de foco que o ADR-0091 §2 introduz), cada uma com o título da aba ativa dela e a contagem de abas, e por fim "Nova janela" (RF-22.21, RF-22.22). Os textos novos entram em todo arquivo de `locales/` ([ADR-0056](0056-catalogo-de-textos-da-interface.md)): o item de menu, o rótulo "Nova janela" e o formato da contagem.

Duas ações entram no enum do [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), sem tecla default:

- `tab.move_to_window` (`Action::TabMoveToWindow`): popover para a seleção, ou para a aba ativa quando não há seleção;
- `group.move_to_window` (`Action::GroupMoveToWindow`): popover para o grupo da aba ativa, inerte fora de grupo explícito.

O item "Mover para janela…" entra no menu de aba **depois** de "Mover para grupo", porque o binário é a ordem de referência e os itens existentes não mudam de lugar. No menu de grupo e no editor, ele entra pela definição única que os dois leem (RF-10.21). O destino do popover é `InsertAt::TrackEnd`, ou o fim da zona para uma fixada (RF-22.23).

## Alternativas consideradas

### `TabId` global ao processo

Com ids únicos no processo, a aba chegaria com o mesmo id e nada precisaria ser re-chaveado. Recusada pela mesma razão que o [ADR-0015](0015-multiplas-janelas.md) já registrou: muda a decisão de identidade do [ADR-0006](0006-modelo-de-abas-e-grupos.md) em vez de refiná-la. E não elimina o problema principal, que é o `WindowId` dentro do closure, nem as chaves `(WindowId, …)` de `reconnects` e do registro de fixadas. Trocaria um re-chaveamento local por uma mudança de identidade em todos os crates.

### Preservar o `TabId` no destino, renumerando em caso de colisão

Evitaria trocar o id na maioria dos casos. Recusada porque transforma uma regra simples ("o destino sempre dá id novo") num caso raro ("só quando colide"), e caso raro é o que teste esquece. Também quebraria a monotonicidade do contador do destino, que é a garantia do §2.

### Setter no `Terminal` (`set_on_wakeup`)

Trocar o closure inteiro em vez de ler uma célula. Recusada porque muda a API de `porecatu-term` para resolver um problema que é de endereçamento da UI, e porque o closure é copiado para duas threads (leitura e vigia de saída) e para o `reattach` da reconexão ([ADR-0071](0071-backend-de-terminal.md)). Trocá-lo com segurança pediria a mesma célula, só que dentro de `term`.

### Endereço global com mapa no `App` (`RouteId → (WindowId, TabId, PaneId)`)

O closure mandaria um id de rota opaco, e `App` traduziria. Também é correta. Recusada porque muda o payload do `Wakeup` e põe uma tradução por evento no caminho quente da main thread, além de um mapa a mais para manter em sincronia com `panes` em todo spawn e fechamento. A célula é local ao painel e nasce e morre com ele.

### Recriar o terminal no destino

Encerrar na origem e abrir de novo no destino, no mesmo `cwd`. Recusada pelo RF-22.2: é exatamente o fluxo manual que o recurso existe para eliminar.

### Re-chavear em cada chamador (arraste e popover)

Cada caminho moveria o que precisa. Recusada porque são dois caminhos com dez passos cada, e a lista do §3 vai crescer com o próximo estado por aba. Um passo esquecido num dos caminhos é um órfão que só aparece num cenário específico, o mesmo risco que o ADR-0054 §6 eliminou para as sessões nomeadas com uma função só.

## Consequências

### Positivas

- O processo, o scrollback e a conexão SSH atravessam a janela intactos, sem nenhuma mudança em `porecatu-term`, `porecatu-pty`, `porecatu-ssh` ou no formato da sessão.
- O core ganha extrair e inserir como operações puras e testáveis sem janela, e elas reaproveitam as regras de saída (RF-2.7, escada de foco) e de chegada (contadores do destino) que já existem.
- Um único ponto conhece a lista de estado por aba, e o teste de órfãos tem um lugar para olhar.

### Negativas

- O `PaneRuntime` ganha um campo e cada wakeup ganha um lock. É barato, mas está no caminho quente.
- `transfer` é longa e toca quase todo o estado de `App`. Cada estado novo chaveado por aba precisa entrar nela, e só o teste de órfãos lembra disso.
- O `TabId` muda ao mudar de janela. Qualquer coisa fora de `App` que guarde um `TabId` (nenhuma hoje) ficaria apontando para o nada.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Estado por aba esquecido na transferência (órfão) | Média | Alto | Teste que move uma aba com reconexão em curso, pergunta pendente, aviso, fixada e nota de integração, e verifica que **nenhuma** chave com o `TabId` antigo sobra em `App` nem na origem |
| Wakeup perdido deixar a aba sem redesenhar no destino | Baixa | Médio | Destino marcado sujo no passo 9, depois de reescrever a célula no passo 4; teste do estado puro que verifica a ordem |
| `HWND` reaproveitado gerar um quadro espúrio | Baixa | Baixo | Aceito (§2): um quadro a mais, nunca a menos |
| Origem fechar com fixada viva | Baixa | Alto | A origem só fecha com o workspace vazio, e fixada conta como aba (ADR-0086 §1); teste |
| Reconexão continuar procurando o painel na janela antiga | Média | Alto | A `PaneKey` é trocada dentro do ciclo, no mesmo passo; teste com o ciclo em `WaitUntil` |
| Árvore de acessibilidade com nós de abas que saíram | Média | Médio | As duas janelas reconstroem a árvore no passo 9 |
