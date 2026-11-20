# ADR-0113 — Restaurar sessão nomeada pelo celular: a restauração do popover, sem foco, sem ativar e sem fechar

**Status:** Aceito em 2026-11-20 — **implementação pendente** (só a documentação existe; etapas no [roadmap](../roadmap.md))
**Data:** 2026-11-20
**Relacionados:** [PRD-014](../prd/prd-014-sessoes-nomeadas.md), [PRD-023](../prd/prd-023-restaurar-sessao-nomeada-na-janela-atual.md), [PRD-025](../prd/prd-025-acesso-remoto.md) (RF-25.28, RF-25.31), [ADR-0054](0054-sessoes-nomeadas.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), [ADR-0091](0091-arraste-entre-janelas.md), [ADR-0092](0092-restauracao-nomeada-na-janela-atual.md), [ADR-0102](0102-fechar-o-terminal-parado-ao-restaurar-na-janela-atual.md), [ADR-0104](0104-crate-porecatu-remote.md), [ADR-0105](0105-espelhamento-da-grade-e-input-remoto.md), [ADR-0107](0107-acoes-remotas-sobre-o-workspace.md), [ADR-0111](0111-o-celular-nao-muda-a-aba-ativa-do-desktop.md); no protocolo, [ADR-0012](https://github.com/porecatu/protocol/blob/main/docs/adr/0012-sessoes-nomeadas-pelo-cliente.md) e as [§4.5 e §7 de 03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md)
**Supersedes:** PRD-025 RF-25.28 (**parcial**: "sessões nomeadas" e "abrir janela nova" saem da lista do que fica fora do alcance do celular, só para restaurar) · ADR-0092 §4 e ADR-0102 (**parcial**: a ativação da aba da sessão e o fechamento do terminal parado não valem quando quem restaura é o celular)

## Contexto

O dono do produto pediu, em 2026-11-20, que o celular possa **escolher uma sessão nomeada e restaurá-la** ([PRD-014](../prd/prd-014-sessoes-nomeadas.md)). O PRD-025 deixava sessões nomeadas fora do alcance do celular (RF-25.28). A especificação **0.6** do protocolo cria o que falta no fio ([ADR-0012 do protocolo](https://github.com/porecatu/protocol/blob/main/docs/adr/0012-sessoes-nomeadas-pelo-cliente.md)):

- `list-sessions` (pedido) e `sessions` (resposta), com o `mode`, que é o valor de `[session] restore_named_in_current_window`, e a lista na ordem do popover;
- um id novo, `session-id`;
- a ação `restore-session { session, ? window }`;
- os erros `unknown-session` e `session-unreadable`.

Este ADR decide como o host executa isso.

O dono fixou quatro coisas antes da escrita:

1. **O destino segue a chave do desktop.** Com `false`, abre uma janela nova. Com `true`, acrescenta a uma janela, que o celular escolhe.
2. **A janela nova não ganha foco.**
3. **No acréscimo, nada é ativado e nada é fechado**, ao contrário do gesto local (ADR-0092 §4, ADR-0102).
4. **Só listar e restaurar.** Salvar e excluir continuam só no desktop.

O caminho local que existe hoje esbarra em três regras do acesso remoto:

- **`restore_named_session(event_loop, origin, file)`** (`lib.rs`) recebe a janela de onde o gesto partiu. Usa essa janela para a cascata da janela nova, para o destino do acréscimo e para o aviso de falha. O celular não parte de janela nenhuma.
- **A janela nova ganha o foco do sistema.** `base_window_attributes` não diz nada sobre ativação, `create_window_with_attributes` chama `set_visible(true)` e `open_window_from_session` chama `focus_order.touch`, que põe a janela no topo de `windows_by_recent_focus`. O ADR-0107 §2 e o ADR-0111 proíbem as duas coisas por causa do celular.
- **`append_named_window`** (`app_named_append.rs`) ativa a aba que era ativa na sessão (RF-23.9), rola a trilha até ela (`reveal_active_tab`) e fecha o terminal parado solitário (ADR-0102, que só fecha **depois** dessa ativação). As duas primeiras coisas o ADR-0111 proíbe; a terceira o dono recusou.

## Decisão

**`restore-session` chama a mesma restauração do popover, com o destino pela chave e a janela de origem trocada por parâmetros explícitos: uma janela nova que nasce sem foco e vai para o fim da ordem de foco, ou um acréscimo que não ativa, não rola e não fecha. A lista é lida sob pedido, na main thread, como o popover a lê.**

### 1. Listar

- **Ator.** `porecatu-remote` recebe `list-sessions` e emite `RemoteEvent::ListSessions { client, id }`. O ator não lê o disco: `porecatu-remote` não depende de `porecatu-session` (ADR-0104), e a lista é dado do `App`.
- **Main thread.** O `App` responde no tratamento do evento, chamando `porecatu_session::named::list_named()`, a mesma função que `WindowState::open_session_picker` chama ao abrir o popover. Ler um diretório de alguns arquivos JSON custa o mesmo que abrir o popover, e o RF-14.18 (ler só sob pedido, nada no arranque, nada vigiado) continua valendo sem exceção.
- **Resposta.** `sessions { id, mode, list }`, mandada só ao canal que pediu (`Command::Send`):
  - `mode` é `config.session.restore_named_in_current_window` no momento da resposta;
  - `list` segue a ordem de `list_named_in` (`saved_at` decrescente);
  - cada `NamedSessionEntry` vira `{ session, name, readable }`, com `readable = matches!(status, EntryStatus::Ok)`;
  - o `name` de uma entrada `Unreadable` é o rótulo que o popover mostra (o nome do arquivo).
- **Erro de leitura do diretório** (inexistente, sem permissão) é lista vazia, como no popover.

### 2. O id de sessão

- `ProtocolIds` (`remote/ids.rs`) ganha uma quinta tabela, `sessions`, chaveada pelo **caminho do arquivo** (`PathBuf`), com contador próprio que começa em 1 e nunca reaproveita, como as outras.
- A tabela de hoje exige `K: Copy`. A restrição passa a `K: Clone + Eq + Hash`, sem mudar o comportamento das quatro tabelas que existem.
- Um id é atribuído ao montar a resposta (`get_or_assign`), então o mesmo arquivo recebe o mesmo id durante a vida do processo.
- A tabela **não** é podada por `retain_seen`, que é da árvore. Um arquivo excluído no desktop deixa o id órfão, e `restore-session` com ele cai em `unknown-session` (§4). O custo é uma entrada por arquivo que o processo já listou.
- O caminho nunca vai ao fio, e o id nunca vira caminho por conta do celular. Só o que o host listou antes pode ser restaurado.

### 3. Restaurar: um só caminho, com dois parâmetros

`restore_named_session` se divide em duas partes, que o popover e o celular chamam:

1. **Ler**: `load_named(file)` → `Result<WindowV1, RestoreFailure>`, com a quarentena do `.corrupt` como hoje. O gesto local transforma a falha no aviso da janela de origem, como hoje. O remoto a transforma em `session-unreadable`, e **nenhum aviso aparece no desktop**: o resultado vai ao celular, como a confirmação vai (ADR-0107 §3).
2. **Aplicar**: o `WindowV1` vai para um dos dois destinos, pela chave lida **no momento da ação**, como no clique (ADR-0092 §2).

**Janela nova.** `open_window_from_session` ganha o parâmetro `activation: WindowActivation`. `Focus` é o de hoje, usado pelo arranque e pelo popover. `Background` é o do celular:

- `base_window_attributes` recebe `with_active(false)`. No Windows, o winit 0.30.13 guarda isso como a ausência do `MARKER_ACTIVATE`, e o primeiro `set_visible(true)` de `create_window_with_attributes` vira `ShowWindow(SW_SHOWNOACTIVATE)`: a janela aparece sem tomar o foco e sem passar à frente da janela ativa. No macOS, `with_active(false)` também é suportado.
- No lugar de `focus_order.touch`, um `FocusOrder::push_back` novo põe a janela no **fim** da ordem, se ainda não estiver lá. A janela fica em `windows_by_recent_focus` (senão sairia do `workspace` do celular, que só lista o que está nessa ordem) e por último, onde fica uma janela que ninguém focou. Quando o usuário clicar nela, `WindowEvent::Focused(true)` faz o `touch` de sempre.
- **Cascata**: `WindowPlacement::CascadeFrom(w)`, com `w` = o primeiro de `windows_by_recent_focus()`, a janela que o usuário usou por último. É a mesma conta do popover, com a origem trocada pela janela mais recente. Sem nenhuma janela de terminal, o que não acontece com o celular conectado, cai na posição padrão do sistema, como já cai hoje quando a origem não existe.
- Dentro da janela nova, a aba ativa é a da sessão (`workspace_from_window`, como hoje): ela não existia e não tinha aba ativa para preservar.

**Acréscimo.** `append_named_window(origin, window_v1)` ganha `rules: AppendRules { activate_session_tab: bool, close_lone_idle: bool }`:

- o popover passa os dois `true` e o comportamento de hoje não muda;
- o celular passa os dois `false`. A aba ativa da janela não muda (sem `activate_tab`), a trilha não rola (sem `reveal_active_tab`, que rolaria até uma aba que não mudou) e `lone_idle_tab` nem é consultada;
- o resto é o mesmo nos dois: `append_session_workspace`, `start_restored_tabs(tabs_to_start(..))` com só a aba da sessão marcada como a que subiria primeiro com `lazy_restore`, `mark_session_dirty`, `sync_window_title` e `refresh_access_tree`;
- a janela alvo é `ids.window_of(window)`.

**O que não muda nos dois destinos:** tema, zoom e fixadas da sessão não são aplicados (RF-14.13, RF-21.13). Cada aba sobe como restaurada (`SpawnOrigin::Restored`, `lazy_restore`, `.porecatu` só em diretório autorizado). Nenhum diálogo é aberto no desktop.

### 4. Ordem das checagens

Numa função pura em `remote/actions.rs`, `plan_restore_session`, no molde de `plan_new_tab`:

1. `unknown-session`: o id não está na tabela, ou o arquivo não existe mais.
2. `unknown-window`: `window` veio e não resolve.
3. `busy`: no acréscimo, `remote_window_busy(alvo)`, com todos os modos de captura, inclusive o popover de sessões aberto nessa janela (`session_picker`). Na janela nova, `cross_drag.is_some()`, porque um arraste entre janelas em curso é o único estado do processo inteiro que uma janela a mais atrapalha.
4. `invalid-argument`: chave em `true` e `window` ausente. Com a chave em `false`, `window` é ignorado.
5. `session-unreadable`: a leitura (§3, passo 1) falha. É a última porque é a única que toca o disco, e por isso também a única que pode pôr um arquivo em quarentena.

O sucesso devolve `action-result` sem `created` e marca a árvore suja (`mark_tree_dirty`), como as outras ações.

### 5. Onde o código mora

| Crate / módulo | O que muda |
|---|---|
| `porecatu-protocol` | o `rev` sobe para o commit que implementar a 0.6, junto com o Relay e o Remote ([ADR-0103](0103-acesso-remoto-tres-partes-e-protocolo-externo.md) §5) |
| `porecatu-remote` (`api.rs`, `session.rs`) | `RemoteEvent::ListSessions`; `list-sessions` vira evento. `restore-session` já chega pelo `RemoteEvent::Action` de hoje |
| `porecatu-ui/src/remote/ids.rs` | tabela `sessions`, `session(path)`, `session_of(id)`; `Table<K: Clone + Eq + Hash>` |
| `porecatu-ui/src/remote/actions.rs` | `plan_restore_session` e o mapeamento da falha de leitura, puros e testados |
| `porecatu-ui/src/app_remote.rs` | `ListSessions` em `apply_remote_request`; `remote_restore_session` |
| `porecatu-ui/src/lib.rs` | `restore_named_session` em duas partes; `WindowActivation` em `open_window_from_session` e `base_window_attributes` |
| `porecatu-ui/src/app_named_append.rs` | `AppendRules` |
| `porecatu-ui/src/cross_window.rs` | `FocusOrder::push_back` |

Nenhuma frase nova de interface: nada aparece no desktop.

## Alternativas consideradas

### Um caminho de restauração próprio para o celular

Uma `remote_restore_session` que montasse a janela por conta própria. Recusada pela mesma razão do ADR-0054 §6 e do ADR-0092: duas restaurações divergem na primeira correção feita numa só. As diferenças são parâmetros (`WindowActivation`, `AppendRules`), não um segundo caminho.

### Criar a janela visível e devolver o foco à janela anterior

Mostrar a janela como hoje e chamar `focus_window` na que estava ativa. Recusada: a janela nova passa à frente por um quadro, e quem digita no desktop pode perder teclas nesse intervalo. `SW_SHOWNOACTIVATE` não ativa em momento nenhum.

### Pôr a janela nova no topo da ordem de foco, como o popover

O celular veria a janela nova primeiro no `workspace`. Recusada: a ordem de foco é o que o desktop usa para "a janela de foco mais recente" (cascata, arraste, a ordem que o celular mostra), e uma janela que ninguém focou no topo mentiria para todas essas contas.

### Mostrar o aviso de falha no desktop, como o popover

O usuário do desktop veria que um arquivo foi para quarentena. Recusada: quem pediu está no celular, e o ADR-0107 §3 já decidiu que o desktop não recebe diálogo nem aviso por causa do celular. A quarentena acontece igual, e o arquivo `.corrupt` fica para exame.

### Ler a lista numa thread curta, com resposta por `Wakeup`

Tiraria o `read_dir` da main thread. Recusada por ora: o popover já lê na main thread, e o custo medido no PRD-014 é desprezível. Se a leitura aparecer na medição de custo da main thread do acesso remoto (dívida do ADR-0105), as duas leituras saem juntas para a mesma thread.

## Consequências

### Positivas

- O celular restaura com o resultado do popover, pelo mesmo código, e as regras do acesso remoto continuam sem exceção: nada ativa, nada ganha foco, nada fecha e nada aparece no desktop.
- `WindowActivation::Background` e `FocusOrder::push_back` ficam disponíveis para "destacar numa janela nova" pelo celular, se um dia entrar.

### Negativas

- O mesmo gesto dá resultados diferentes conforme quem o faz. Pelo popover, a aba da sessão fica ativa e o terminal parado fecha; pelo celular, não. O guia do usuário explica a diferença.
- A tabela de ids de sessão só cresce durante a vida do processo.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| A janela nova roubar o foco no X11 e no Wayland, onde `with_active` não é suportado e o gerenciador de janelas decide | Alta nesses sistemas | Médio | Registrado como dívida de verificação no roadmap. O host não tenta contornar (nada de devolver o foco depois, ver as alternativas) |
| `SW_SHOWNOACTIVATE` mudar de comportamento numa subida do `winit` | Baixa | Médio | Conferir o `apply_diff` de `window_state.rs` do `winit` a cada subida, junto com a captura implícita do ADR-0091 |
| Restaurar enquanto o usuário do desktop usa o popover de sessões na janela alvo | Baixa | Baixo | `session_picker` é modo de captura: a ação volta `busy` |
| Quarentena disparada por um pedido do celular, sem ninguém no desktop ver o aviso | Baixa | Baixo | O celular recebe `session-unreadable` e mostra a falha. O popover do desktop mostra a linha esmaecida na próxima abertura |
