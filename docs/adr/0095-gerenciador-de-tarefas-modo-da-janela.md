# ADR-0095 — Gerenciador de tarefas como modo da janela: estado em `WindowState`, ação `task_manager.toggle`, captura de entrada, lista derivada da amostra e chave `[task_manager] history_seconds`

**Status:** Aceito
**Data:** 2026-11-10
**Relacionados:** [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0019](0019-tooltip.md), [ADR-0020](0020-grupos-explicitos.md), [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0053](0053-paineis-divididos.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md), [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md), [ADR-0078](0078-largura-e-quebra-de-texto-do-dialogo.md), [ADR-0079](0079-botao-de-menu-do-app.md), [ADR-0090](0090-transferencia-de-abas-entre-janelas.md), [ADR-0093](0093-medicao-de-processos-do-gerenciador.md), [ADR-0094](0094-polilinha-e-area-no-render.md), [ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md), [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md)
**Supersedes:** ADR-0079 decisão 5 (**parcial**: o botão do app deixa de ser "sem ação" e passa a alternar o gerenciador; geometria, posição, ocultação e a regra do fundo não mudam)

## Contexto

O [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md) decidiu que o gerenciador é um **modo da janela**. O botão do app alterna a área de conteúdo entre a árvore de painéis da aba ativa e o gerenciador, a barra de abas fica, o modo não é gravado, o teclado não chega ao terminal, e qualquer gesto que ative ou crie uma aba volta ao terminal. A medição é do [ADR-0093](0093-medicao-de-processos-do-gerenciador.md), a primitiva de linha do [ADR-0094](0094-polilinha-e-area-no-render.md) e a aparência do [ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md). Este ADR decide o resto: o estado, a entrada, a lista, as ações e a configuração.

O que já existe e serve de molde:

- `WindowState` guarda o estado de UI **da janela** (busca, menus, popovers, diálogo, `hover`, `warnings`) e `close_all_popovers`/`modal_overlay_open`.
- `App::dispatch_keyboard_input` é uma **cadeia de captura**: diálogo, menus e popovers, rename, `Esc` de aviso e de seleção, busca, `handle_tab_action_key` e, por fim, o painel focado.
- As ações são `porecatu_core::Action`, com o catálogo de strings em `action.rs`; `handle_tab_action_key` despacha por `match action`.
- O botão do app tem hit-test (`ZoneButtonHit::AppMenu`), estado pressionado (`pressed_zone_button`) e um braço vazio no release ("ADR-0079: … no action yet").
- `App::redraw` calcula o `box_rect` da aba ativa (`paint::terminal_box_rect`), faz o `panes::layout` e desenha cada painel na `Layer::Grid`, e depois a busca sobre o quadro.
- O `ConfirmDialog`, com `DialogAction` e `run_dialog_action`, e o menu de contexto, com `MenuItem { action, enabled }` e o item desabilitado esmaecido, são os widgets do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md).

## Decisão

**O modo é um `Option<TaskManagerView>` em `WindowState`. O histórico dos gráficos é um `TaskManagerHistory` separado, que vive com a janela. A cadeia de teclado ganha um elo antes da busca, que consome a navegação da lista e engole tudo o que iria ao painel. A volta ao terminal é uma função só, `leave_task_manager`, chamada por todo gesto que ative ou crie aba. A lista é uma função pura do workspace e da última amostra. As ações usam o menu de contexto e o diálogo que já existem. A chave `[task_manager] history_seconds` é de classe A, com o piso e o teto aplicados na UI, como o intervalo do Git.**

### 1. O estado

Em `WindowState`:

```rust
/// ADR-0095 §1: `Some` = a área de conteúdo mostra o gerenciador.
task_manager: Option<TaskManagerView>,
/// ADR-0095 §1: sobrevive a fechar o gerenciador (RF-24.12); some com a janela.
task_manager_history: TaskManagerHistory,
```

- `TaskManagerView` guarda a última amostra **desta janela** (os `ProcessStat` por `(TabId, PaneId)`, do [ADR-0093](0093-medicao-de-processos-do-gerenciador.md) §6), a linha selecionada (`RowKey`, §5), a rolagem da lista, a linha em hover e o menu de contexto da lista, quando aberto.
- `TaskManagerHistory` é um `VecDeque<HistoryPoint { at: Instant, cpu: Option<f32>, memory: u64, disk_per_sec: f64 }>`. Ao acrescentar, descarta os pontos com `at` mais velho que `history_seconds` antes do último. Não grava nada em disco nem na sessão (RF-24.6, RF-24.12).

O módulo novo `crates/porecatu-ui/src/task_manager.rs` reúne os tipos, as funções puras (lista, escalas, pontos do gráfico) e a pintura. O `lib.rs` só liga as pontas, como fazem `app_named_append.rs` e `app_transfer.rs`.

### 2. Abrir e fechar

- **Abrir** (`App::enter_task_manager(window)`): cria a `TaskManagerView`, fecha os popovers e o menu do terminal (`close_all_popovers`), fecha a busca da janela (o quadro em que ela flutuava não está à vista), e cria o amostrador se ainda não houver ([ADR-0093](0093-medicao-de-processos-do-gerenciador.md) §4). A seleção começa na linha da aba ativa.
- **Fechar** (`App::leave_task_manager(window)`): `task_manager = None`, mantém o histórico e derruba o amostrador se nenhuma outra janela tiver o gerenciador aberto.

  > **Revisto pelo [ADR-0098](0098-monitoramento-em-segundo-plano-do-gerenciador.md)** (2026-11-10): com `[task_manager] background_monitoring = true`, fechar não derruba o amostrador, e o histórico da janela continua recebendo pontos com o gerenciador fechado.
- **Alternar** (`App::toggle_task_manager(window)`) chama uma das duas.

Quem chama, e qual das duas:

| Gesto | Chama |
|---|---|
| Release do botão do app armado (o braço `ZoneButtonHit::AppMenu` do release) | `toggle` |
| Ação `task_manager.toggle` | `toggle` |
| `Esc` no elo do gerenciador (§3) | `leave` |
| "Ir para a aba" (§7) | `leave` e depois ativa |
| Clique numa aba da barra, **inclusive a ativa**; clique numa fixada | `leave` |
| Qualquer criação ou ativação de aba nesta janela por ação, menu ou popover (`tab.new`, `tab.next`/`prev`, índice, `group.next`/`prev`, `group.new_tab`, nova aba pelo `+` e pelo menu de perfis, restaurar sessão na janela atual, uma aba chegando por transferência e ativada) | `leave` |
| Fechar a janela | o estado some com o `WindowState`; o amostrador cai se era o último |

A última linha da tabela não é uma lista de chamadas espalhadas. Ela é **uma checagem só**, no fim de cada despacho de entrada da janela: se o `TabId` ativo mudou ou se a contagem de abas cresceu desde o começo do despacho, `leave` é chamada. O clique na aba ativa é o único caso explícito, porque não muda nada que a checagem veja. Assim, uma ação nova que ative abas no futuro volta ao terminal sem que ninguém precise lembrar.

Fechar abas com o gerenciador aberto **não** fecha o gerenciador (RF-24.6): a checagem olha só para ativação e crescimento. Fechar a aba ativa troca o `TabId` ativo, por isso o fechamento é **excluído** da checagem: o despacho de `tab.close`, o "x" e o menu marcam o despacho como fechamento, e a checagem não dispara nele.

### 3. Teclado

Em `dispatch_keyboard_input`, um elo novo, **`handle_task_manager_key`**, entra depois da cadeia de diálogo, menus, popovers, rename e avisos, e **antes** da busca. Com `task_manager` em `None`, o elo devolve `Unhandled` e nada muda. Com o gerenciador aberto:

1. Se o menu de contexto da lista está aberto, ele consome a tecla, como os outros menus.
2. Navegação: `↑`/`↓` movem a seleção, `Home`/`End` vão às pontas, `PageUp`/`PageDown` andam uma tela de linhas, `Enter` faz "Ir para a aba", e a tecla de menu ou `Shift+F10` abre o menu da linha selecionada. `Esc` chama `leave`. Todas são consumidas.
3. Qualquer outra tecla segue para `handle_tab_action_key`. Se ela casar com uma ação, o `match` despacha, com um filtro antes: as ações **que miram o painel** não fazem nada com o gerenciador aberto (RF-24.5). São as famílias `search.*`, `pane.*`, `scrollback.*`, `clipboard.*`, `selection.*`, `font.*` e `ssh.reconnect`. A lista mora numa função `Action::targets_pane(&self) -> bool` no core, com um teste que percorre o `CATALOG` inteiro, para uma ação nova ter de ser classificada.
4. O que não casou com ação **é engolido**: nunca chega a `input::handle_keyboard_input` nem ao painel SSH dormente (que conectaria por `Enter`).

O `Esc` da cadeia que fecha um aviso continua antes do elo: com um aviso na tela, o primeiro `Esc` fecha o aviso, e o segundo volta ao terminal. É a mesma precedência que o `Esc` já tem com a busca.

### 4. Mouse, pintura e transferência

- **Mouse na área de conteúdo** vai para o hit-test do gerenciador (gráficos, cabeçalho, linhas), nunca para `input::handle_mouse_*` do painel. A roda rola a lista, o clique seleciona, o duplo clique faz "Ir para a aba" e o clique direito abre o menu. O menu de contexto do terminal não abre. O release continua chegando a `input::handle_mouse_button` para zerar o estado de botão (CLAUDE.md), sem efeito no painel.
- **Pintura.** Em `App::redraw`, com o gerenciador aberto, o laço de painéis (`panes::layout`, `snapshot_into`, `build_primitives_with_image`) e a busca **não rodam**. No lugar deles, `task_manager::paint(…)` desenha no mesmo `box_rect` e na mesma `Layer::Grid` ([ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md)). O `box_rect` é o mesmo, então a grade dos painéis **não muda de tamanho** e nenhum PTY é redimensionado ao alternar.
- **Redesenho.** O gerenciador não anima: ele redesenha quando chega uma amostra, quando há entrada e no hover. O `Wakeup::TabDirty` de um painel continua marcando atividade e campainha na barra, como sempre.
- **Transferência** ([ADR-0090](0090-transferencia-de-abas-entre-janelas.md)): `App::transfer` remove da `TaskManagerView` das duas janelas as entradas da última amostra chaveadas pelos `TabId` que saíram. Se a linha selecionada era de uma aba que saiu, a seleção vai para a primeira linha. É a regra do ADR-0090: "estado novo chaveado por `TabId` fora do runtime entra em `transfer`, senão vira órfão". O histórico é da janela e não muda (RF-24.25).

### 5. A lista é uma função pura

```rust
pub enum RowKey {
    Tab(TabId),
    Note(TabId, PaneId),
    Process { tab: TabId, pane: PaneId, pid: u32, start_time: u64 },
}

pub fn build_rows(workspace: &Workspace, panes: &PaneFacts, sample: &WindowSample) -> Vec<Row>
```

- **Ordem**: `workspace.visual_order()`, ou seja, as fixadas e depois a trilha, inclusive as abas de grupos colapsados (RF-24.13). Uma linha `Tab` por aba, com título (precedência do RF-1.7, pelo mesmo `tab_title` da barra), cor do grupo (`None` no implícito) e a soma dos processos dela.
- **Painéis**: na ordem de `tab.panes().panes()` (a ordem da árvore, que é a da tela, de cima para baixo e da esquerda para a direita, RF-24.15). Para cada painel:
  - com `ProcessStat`s na amostra, a árvore deles: a raiz (o PID igual a `root_pid`) no nível 1, e cada filho sob o pai pelo campo `parent`. Os irmãos são ordenados por `start_time` e depois por PID. Um membro cujo `parent` não está na árvore (a cadeia quebrada que só a lista do Job alcança) entra como filho da raiz;
  - sem amostra, uma linha `Note` com o motivo, decidido por `PaneFacts`: aba `NotStarted` sem runtime, painel SSH dormente ou conectado (`ssh_host.is_some()`), painel encerrado, ou "aguardando a primeira medição" na primeira abertura, antes da primeira resposta (RF-24.17).
- **Seleção estável**: depois de cada amostra, a seleção é procurada pela `RowKey`. Se não existir mais, vai para a linha `Tab` da mesma aba; se a aba também sumiu, vai para a primeira linha (RF-24.18). A rolagem é mantida pelo índice da primeira linha visível, ajustada só se a seleção sair da vista.

`build_rows` é pura e testada sem janela: ordem com fixadas e grupo colapsado, árvore com órfão pendurado na raiz, nota por tipo de painel, seleção que sobrevive à morte do processo e cai para a aba.

### 6. Os gráficos a partir do histórico

Também puras, em `task_manager.rs`:

- **Eixo do tempo**: o ponto mais recente fica na borda direita. Cada ponto fica a `(último.at − ponto.at) / history_seconds × largura` da direita. O tempo de referência é o **instante da última amostra**, não o relógio do quadro: o gráfico só anda quando chega dado, sem redesenho contínuo (§4).
- **Lacuna**: dois pontos consecutivos com `at` a mais de 1,5 s um do outro quebram a série. Cada trecho contínuo vira uma `Polyline` e uma `Area` próprias ([ADR-0094](0094-polilinha-e-area-no-render.md) §1), e um trecho de um ponto só não desenha linha. É a lacuna do RF-24.12.
- **Escalas**: a CPU é fixa em 0–1. Memória e disco usam o maior valor do período visível, arredondado para cima ao próximo `1, 2 ou 5 × 10ⁿ` na unidade de exibição. Com tudo em zero, o topo é o menor degrau da unidade (1 KB, 1 KB/s), para a linha ficar no chão e não no meio.
- **Grade vertical**: a cada 10 s, ancorada ao instante da amostra, para correr junto com a linha, como no Windows ([ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md)).
- **Formatação**: unidades em base 1024 (B, KB, MB, GB) com uma casa decimal a partir de KB, e porcentagem inteira. Os símbolos e o separador decimal vêm do catálogo de textos, porque `1,4 GB` em `pt_BR` é `1.4 GB` em `en_US`.

### 7. As ações da lista

- **Ir para a aba** (duplo clique, `Enter` ou o item do menu): `leave_task_manager`, depois ativa a aba pelo mesmo caminho de um clique na barra (a escada de foco expande o grupo colapsado, [ADR-0020](0020-grupos-explicitos.md)), foca o painel da linha (`set_focused_pane`) e rola a trilha até a aba (`ensure_active_tab_visible`). Numa linha `Tab`, só ativa a aba.
- **Menu de contexto da linha**: o widget de menu de contexto do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), com um enum de itens próprio, `TaskMenuItem { GoToTab, EndProcess }`, e o mesmo `enabled` e a mesma pintura esmaecida. "Encerrar processo" fica desabilitado numa linha `Tab`, numa `Note` e na raiz do painel (RF-24.22). Ancorado no ponto do clique, ou, pelo teclado, na linha selecionada.
- **Encerrar processo** abre o `ConfirmDialog` com um `DialogAction::EndProcess { tab, pane, pid, start_time }`. O título e o corpo vêm do catálogo (nome, PID e "e N processos filhos", com N contado na última amostra). O botão confirmatório é destrutivo, com o foco inicial no cancelar, na largura e quebra do [ADR-0078](0078-largura-e-quebra-de-texto-do-dialogo.md). `retarget_tab`, que casa todas as variantes, ganha o braço novo e re-chaveia a aba numa transferência. Confirmado, `run_dialog_action` monta o `KillRequest` com o `ProcessGroup` do painel e o envia ([ADR-0093](0093-medicao-de-processos-do-gerenciador.md) §7). Painel que não existe mais: nada acontece.
- **Tooltip** com o nome inteiro, pelo `Hover` do [ADR-0019](0019-tooltip.md), com uma variante de `HoverKey` para a linha. Só aparece quando o nome foi cortado.

### 8. A ação `task_manager.toggle`

`Action::TaskManagerToggle` no core, com a string `task_manager.toggle` no `CATALOG` e a gramática do [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), **sem tecla default** (precedente de `group.new_tab`). Despachada em `handle_tab_action_key`, porque mira a janela, não o painel (`targets_pane() == false`). Em [acoes.md](../reference/acoes.md), uma seção `task_manager.*` nova, antes das reservadas `[v2]`, entra **no PR de implementação**: o catálogo é fechado, e uma ação documentada antes de existir no binário seria um erro de config prometido.

### 9. A chave `[task_manager] history_seconds`

Em `porecatu-config`, `crates/porecatu-config/src/task_manager.rs`:

```rust
/// Período dos gráficos do gerenciador de tarefas, em segundos (RF-24.26).
/// A faixa de 30 a 600 é aplicada na UI (`effective_history`), que avisa
/// uma vez quando ajusta, como o intervalo do Git.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct TaskManager {
    pub history_seconds: u32,   // default 60
}
```

registrada em `Config`, com `default_matches_example_toml`.

- **Piso e teto na UI**, como `git::effective_poll_interval`: `task_manager::effective_history(raw) -> EffectiveHistory { seconds, adjusted }`, com uma flag `task_manager_history_warned` no `App` e um aviso `Severity::Info` (`msg::notice::task_manager_history_adjusted`) dado uma vez por valor carregado (RF-24.26).
- **Classe A** ([ADR-0030](0030-escopo-do-hot-reload.md)): a chave é lida a cada amostra e a cada pintura pelo `self.config` corrente, e o `reload::diff` não precisa de código. Aumentar o período abre espaço à esquerda, e diminuir corta o histórico na próxima amostra (RF-24.27).
- **Arquivo de exemplo**: a seção nova entra **no PR de implementação**, porque `tests/example_toml.rs` reprova chave desconhecida antes de o código existir, como em `[git]` e `[panes]`:

  ```toml
  # --- Gerenciador de tarefas -------------------------------------------------
  [task_manager]
  # Quantos segundos os gráficos do gerenciador de tarefas mostram. De 30 a 600.
  # Vale na hora.
  history_seconds = 60
  ```

- **Tela de configurações** ([PRD-016](../prd/prd-016-tela-de-configuracoes.md) RF-16.11, emendado no mesmo PR deste ADR): um `Group::TaskManager` novo, **depois de Painéis e antes de Atalhos**, com uma seção e um `option!`, controle `integer(30.0, 600.0, 1.0)`, caminho `"task_manager.history_seconds"`, lido por `|c| EditValue::Integer(i64::from(c.task_manager.history_seconds))`. A anatomia é a do [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md) §3 (campo numérico de `number_field_width`). O teste `the_catalog_has_exactly_the_options_of_rf_16_11` passa de 58 para 59 opções, com um grupo a mais na contagem por grupo, e `probe_scope` devolve `Live` sem código próprio.

> **Revisto pelo [ADR-0098](0098-monitoramento-em-segundo-plano-do-gerenciador.md)** (2026-11-10): a seção `[task_manager]` ganha a chave booleana `background_monitoring` (default `false`, classe A), e o grupo da tela de configurações, uma segunda opção, com alternância, antes do período (59 → 60 opções). A alternância também fica no rodapé da lista do gerenciador e grava a chave pelo `porecatu-config::edit`.

### 10. Acessibilidade

Em `access::build_tree` ([ADR-0043](0043-arvore-de-acessibilidade.md)):

- o nó do botão do app passa de `access.app_menu_button` ("Menu do aplicativo") a `access.task_manager_button` ("Gerenciador de tarefas"), com `Role::Button` e o estado de alternância (`Toggled::True`/`False`), mais a ação `Click`. O identificador velho sai do catálogo e de todo arquivo de `locales/`;
- com o gerenciador aberto, os nós dos painéis saem da árvore e entra um nó de grupo "Gerenciador de tarefas" com dois filhos:
  - os três gráficos, cada um um nó `Role::Meter` com o nome e o valor atual em texto ("CPU, 12%"). O desenho da linha não é acessível, o número é;
  - a lista, `Role::TreeGrid`: uma linha `Role::Row` por linha visível ou não (a árvore inteira, não só a vista), com nível, expandido sempre verdadeiro, selecionado, e o texto "nome, PID, CPU, memória, disco". A linha de aba traz o título e a soma. As ações `Focus` e `ShowContextMenu` levam aos mesmos caminhos do teclado.

### 11. Textos

Todo texto novo nasce no registro de mensagens de `porecatu-ui` (`messages.rs`), em módulos `task_manager::*`, `access::*`, `notice::*` e `settings::*`, com valor em **todo** arquivo de `locales/` e o teste de completude que já existe ([ADR-0056](0056-catalogo-de-textos-da-interface.md)). Isso cobre os títulos dos gráficos, os cabeçalhos das colunas, as notas do RF-24.17, os itens do menu, o diálogo, as unidades e o separador decimal, o aviso de ajuste, e o rótulo e a descrição da tela de configurações. A terminologia é **"abas"**.

### 12. Onde o código mora

| Crate | O que muda |
|---|---|
| `porecatu-core` | `Action::TaskManagerToggle`, `task_manager.toggle` no `CATALOG`, `Action::targets_pane` com o teste que percorre o catálogo |
| `porecatu-config` | `task_manager.rs` (`TaskManager`), campo em `Config`, default e teste |
| `porecatu-ui` | `task_manager.rs` (tipos, `build_rows`, escalas, pontos, pintura); `WindowState::{task_manager, task_manager_history}`; `enter`/`leave`/`toggle`; o elo de teclado; o hit-test de mouse; o desvio em `redraw`; a checagem de "ativou ou criou aba"; `TaskMenuItem`; `DialogAction::EndProcess`; `HoverKey` nova; `effective_history` e o aviso; o `Group::TaskManager` e o `option!`; o nó de acessibilidade; as mensagens |
| `locales/` | todos os textos, em todos os idiomas |
| `porecatu-render` | o do [ADR-0094](0094-polilinha-e-area-no-render.md) |
| `porecatu-pty`, `porecatu-term` | o do [ADR-0093](0093-medicao-de-processos-do-gerenciador.md) |
| `porecatu-session`, `porecatu-ssh`, `porecatu-locale` | nada |

## Alternativas consideradas

### Aba especial na trilha

Recusada pelo dono do produto no PRD-024. Uma aba sem processo pediria regra própria para a sessão, os grupos, o arraste, a fixação, a transferência entre janelas e o fechamento, ou seja, uma exceção em cada ADR da barra.

### Uma janela própria, como a de configurações

Recusada pelo dono no PRD-024 ("no lugar do painel do terminal, como se fosse um terminal").

### Um tipo de painel na árvore do [ADR-0053](0053-paineis-divididos.md)

O gerenciador como folha da árvore de painéis, dividindo espaço com terminais. Recusada: o pedido é "no lugar", não "ao lado", e uma folha sem `Terminal` quebraria a premissa de que todo painel tem processo, `cwd` e estado de ciclo de vida, de que a sessão, a barra de status e o `.porecatu` dependem.

### Listas de chamadas de `leave` em cada ação que ativa aba

Recusada no §2: a lista envelhece. A checagem de "o ativo mudou ou abriu aba" no fim do despacho cobre as ações de hoje e as que vierem.

### Deixar as ações de painel agirem sobre o painel focado, escondido

Por exemplo, `clipboard.paste` colando no terminal que não está à vista. Recusada pelo RF-24.5: um efeito invisível no terminal é o pior dos dois mundos. Voltar ao terminal e executar também foi considerado e recusado: o usuário teria apertado um atalho de painel com o gerenciador à frente, e trocar de tela por isso surpreende mais do que ignorar.

### Piso e teto da chave em `porecatu-config`

Como o `clamped_blur` do acrílico. Recusada pela simetria com `[git]`, que é a chave de comportamento mais parecida (um tempo, com piso, avisado uma vez). O acrílico é aparência, e a validação dele fica junto do valor que o render usa.

## Consequências

### Positivas

- Nenhum ADR da barra, da sessão ou dos painéis ganha exceção: o modo não é aba, não é painel e não é gravado.
- A volta ao terminal é uma regra (§2), não uma lista. Uma ação nova que crie ou ative aba já sai certa.
- A lista, as escalas, a lacuna e a seleção estável são funções puras, testáveis sem janela, como pede a convenção do projeto para o que anda à frente da UI.
- As ações reaproveitam o menu de contexto, o diálogo e o tooltip. O único widget novo é o próprio gerenciador.

### Negativas

- **O botão do app perde o papel de "menu do app"** que o ADR-0079 reservava. Um menu do app no futuro precisará de outro lugar ou de um gesto diferente (clique direito, por exemplo).
- Mais um elo na cadeia de teclado, e uma classificação nova (`targets_pane`) que toda ação nova precisa respeitar. O teste sobre o `CATALOG` obriga a isso.
- `App::redraw` ganha um desvio grande. O corpo do gerenciador fica em `task_manager.rs` para o `lib.rs` só escolher.
- O identificador `access.app_menu_button` some, o que é uma mudança nos arquivos de `locales/` de terceiros, se houver.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Uma tecla vazar para o PTY com o gerenciador aberto | Média | **Alto** (comando digitado às cegas) | O elo engole todo `Unhandled` (§3 item 4); teste de despacho de que nenhuma tecla imprimível nem `Enter` chega ao painel focado, inclusive no SSH dormente |
| Fechar a aba ativa disparar a volta ao terminal | Média | Baixo | O fechamento é excluído da checagem (§2), com teste |
| Chave de `TabId` órfã na `TaskManagerView` depois de uma transferência | Média | Baixo | Limpeza em `App::transfer` (§4); teste de que nenhuma chave da view aponta para aba fora do workspace |
| `Action::targets_pane` desatualizada para uma ação nova | Média | Médio | O teste percorre o `CATALOG` e exige classificação explícita de cada string |
| Lista longa (centenas de processos) custar no quadro | Baixa | Médio | Só as linhas visíveis são medidas e pintadas; o texto é medido uma vez por string ([ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md) §5), nunca por quadro sem cache |
