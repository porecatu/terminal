# ADR-0092 — Restauração nomeada na janela atual: chave `[session] restore_named_in_current_window`, acréscimo pelo `insert_detached_*` do core, mesmo arranque de runtimes

**Status:** Aceito
**Data:** 2026-11-07
**Relacionados:** [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0036](0036-formato-do-arquivo-de-sessao.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0051](0051-arquivo-de-projeto-porecatu.md), [ADR-0053](0053-paineis-divididos.md), [ADR-0054](0054-sessoes-nomeadas.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md), [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md), [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md), [ADR-0090](0090-transferencia-de-abas-entre-janelas.md), PRD-014, PRD-016, PRD-021, PRD-022, [PRD-023](../prd/prd-023-restaurar-sessao-nomeada-na-janela-atual.md)
**Supersedes:** ADR-0054 §6 (**parcial**: o destino da restauração deixa de ser sempre uma janela nova e passa a ser decidido por uma chave; a construção do `Workspace` e o arranque dos runtimes continuam compartilhados) · ADR-0054 §8 (**parcial**: só a frase "sem chave nova em `[session]`"; entra uma, de classe A)

## Contexto

O [PRD-023](../prd/prd-023-restaurar-sessao-nomeada-na-janela-atual.md) pede uma opção, desligada por padrão, para que restaurar uma sessão nomeada **acrescente** os grupos e as abas dela à janela de onde o gesto partiu, em vez de abrir uma janela nova. Nada é substituído.

O [ADR-0054](0054-sessoes-nomeadas.md) §6 decidiu que a restauração nomeada passa pelo **mesmo** `open_window_from_session` do arranque. As diferenças entram como parâmetro (`WindowPlacement::CascadeFrom`), nunca como caminho paralelo. É isso que sustenta a métrica do PRD-014 de *zero* diferença de comportamento entre aba restaurada no arranque e aba restaurada por nome. O destino novo tem de preservar essa propriedade.

O terreno já está quase todo pronto:

- `porecatu_session::convert::workspace_from_window` monta um `Workspace` completo a partir de um `WindowV1`: grupos, abas, árvore de painéis, estado `NotStarted` com `lazy_restore` e SSH dormente (RF-20.33).
- O [ADR-0090](0090-transferencia-de-abas-entre-janelas.md) §1 deu ao core o único caminho para **inserir num workspace abas e grupos vindos de fora**, em `crates/porecatu-core/src/workspace/transfer.rs`. `take_tabs` e `take_group` extraem; `insert_detached_tabs` e `insert_detached_group` inserem com `TabId` e `GroupId` **novos** do workspace de destino; `InsertAt::TrackEnd` é o fim da trilha. O ADR-0090 fixou também que um grupo inserido **nunca se funde** com um grupo de mesmo nome.
- O laço que sobe os runtimes de uma janela restaurada mora dentro de `open_window_from_session` (`crates/porecatu-ui/src/lib.rs`). Ele chama `spawn_tab_runtime` com `SpawnOrigin::Restored` para cada aba a iniciar e depois `spawn_restored_sibling_panes` para os painéis irmãos.

Três perguntas não estão respondidas por nada disso:

1. **Onde a opção mora e quando vale.** `[session]` inteira é classe C ([ADR-0030](0030-escopo-do-hot-reload.md)), e `reload::diff` marca como "reinicie o app" qualquer mudança na seção.
2. **Como juntar a sessão à janela** sem um segundo caminho de inserção no core.
3. **Como subir os runtimes** sem copiar o laço do arranque.

## Decisão

**Uma chave booleana `[session] restore_named_in_current_window`, default `false`, de classe A. Ligada, `restore_named_session` monta o `Workspace` da sessão pelo mesmo `workspace_from_window`, extrai dele grupos e abas pela ordem da trilha e os insere no fim da trilha da janela de origem por `insert_detached_group`/`insert_detached_tabs`. Os runtimes sobem por uma função extraída de `open_window_from_session`, que os dois destinos chamam. O core não ganha código.**

### 1. A chave

Em `porecatu_config::Session` (`crates/porecatu-config/src/session.rs`):

```rust
/// `true`: restaurar uma sessão nomeada acrescenta os grupos e as abas
/// dela à janela de onde o gesto partiu, no fim da trilha; `false`
/// (default): abre uma janela nova (RF-14.10). RF-23.1. Classe A:
/// lida no clique, fora do `diff` de `[session]` (ADR-0092 §2).
pub restore_named_in_current_window: bool,
```

Com `false` no `Default`, e uma linha no teste `default_matches_example_toml`.

No [arquivo de exemplo](../config/porecatu.example.toml), a linha entra **no PR de implementação**, e não neste: `tests/example_toml.rs` reprova chave desconhecida. Ela fica depois de `restore_window_geometry`, com o comentário que avisa a exceção à frase "Reinicie o app depois de mudar" do cabeçalho da seção:

```toml
# true = escolher uma sessão salva no botão de sessões acrescenta os grupos
# e as abas dela a esta janela, em vez de abrir uma janela nova. Nada do
# que já está aberto é fechado. Vale na hora, sem reiniciar.
restore_named_in_current_window = false
```

O nome diz o que a chave escolhe, que é o **destino** da restauração. "In place" foi evitado de propósito, porque sugere substituir.

### 2. Classe A, numa seção de classe C

A chave é lida **no clique**, em `restore_named_session`, pelo `self.config` corrente. A recarga já troca o `Arc<Config>` inteiro mesmo quando há chaves diferidas (`apply_reload`), então o valor novo está lá na próxima restauração. Ninguém guarda cópia do valor.

O que impede a classe A hoje é só o aviso: `reload::diff` compara `old.session != new.session` e empurra um `Deferred { key: "[session]", scope: Restart }`. A comparação passa a ignorar o campo novo: compara as duas `Session` com `restore_named_in_current_window` igualado, ou compara campo a campo. Com isso:

- mudar **só** a chave nova não gera aviso;
- mudar a chave nova **junto** com outra de `[session]` gera o aviso de sempre, que é verdade para a outra;
- na tela de configurações, `probe_scope` (`crates/porecatu-ui/src/settings/catalog.rs`) devolve `ReloadScope::Live` para a opção sem código próprio, porque deriva o escopo do `diff`. O teste de escopos ganha uma linha: `scope("restore_named_in_current_window") == ReloadScope::Live`.

É a primeira chave classe A dentro de `[session]`. O preço é uma exceção a registrar, e o ADR-0030 a ganha num blockquote, como já ganhou as de `[git]`, `[panes]` e da imagem de fundo.

### 3. A tela de configurações

Um `option!` no grupo `Session`, seção `Restore`, controle `TOGGLE`, caminho `"session.restore_named_in_current_window"`, lido por `|c| EditValue::Bool(c.session.restore_named_in_current_window)`. Ele entra **depois** de `restore_window_geometry` e antes de `suggest_shell_integration`, perto das duas chaves de restauração. Rótulo e descrição entram no registro de mensagens de `porecatu-ui` (`messages.rs`) e em **todo** arquivo de `locales/`, com o teste de completude que já existe. Em `pt_BR`, o rótulo é "Restaurar sessões salvas na janela atual" e a descrição diz que a sessão escolhida é acrescentada à janela, sem fechar o que está aberto.

Gravar passa por `porecatu-config::edit`, por edição de chave ([ADR-0058](0058-escrita-do-arquivo-de-configuracao.md)), como toda alternância. Nada de anatomia nova: a alternância é a da §3 do [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md).

### 4. Restaurar na janela atual

`restore_named_session(event_loop, origin, file)` continua sendo o único ponto de entrada, chamado pelo popover. A leitura e o tratamento de falha (aviso na janela de origem, canal 1) **não mudam**. Muda o que acontece com um `WindowV1` válido:

```text
if self.config.session.restore_named_in_current_window
    → self.append_named_window(origin, window_v1)
else
    → self.open_window_from_session(event_loop, window_v1, CascadeFrom(origin), false)   // como hoje
```

`append_named_window` faz, nesta ordem:

1. **Monta** um `Workspace` temporário com o **mesmo** `workspace_from_window(groups, tabs, active_tab, lazy_restore, restores_as_ssh)` que `open_window_from_session` chama, com os mesmos argumentos. `NotStarted`, SSH dormente e árvore de painéis vêm daí, sem cópia.
2. **Percorre** a trilha do temporário em ordem visual (`track_order` e os grupos na ordem da lista). Cada grupo explícito sai por `take_group` e entra no workspace da janela por `insert_detached_group` no fim da lista de grupos. Cada corrida de abas soltas sai por `take_tabs` e entra por `insert_detached_tabs(…, InsertAt::TrackEnd)`. As duas funções devolvem os ids **novos**, na ordem de entrada. O `last_active` de cada grupo é traduzido pelo próprio `insert_detached_group`.
3. **Traduz** o id da aba ativa da sessão, pela posição dela na sequência extraída, para o id novo na janela.
4. **Sobe os runtimes** pela função do §5, com os ids novos: todas as abas acrescentadas com `lazy_restore = false`, só a ativa traduzida com `true`.
5. **Ativa** a aba traduzida, chama `ensure_active_tab_visible` no próximo frame (a trilha rola até ela, como num clique), `sync_window_title`, e pede redraw. O popover já fechou antes da chamada (ADR-0055 §3).

O workspace temporário é descartado no fim, vazio. Ele nunca tem janela, runtime nem endereço de wakeup: é só o adaptador entre o `WindowV1` e o caminho de inserção que o core já tem.

**Por que extrair e inserir, e não converter direto para dentro da janela.** `workspace_from_window` constrói um `Workspace` do zero, com os contadores de id dele. Convertê-lo "para dentro" de um workspace existente pediria uma segunda versão da conversão, que recebesse o workspace de destino, ou um merge novo no core. As duas coisas são o segundo caminho que o ADR-0054 §6 e o ADR-0090 §1 existem para não ter. `take_*`/`insert_detached_*` já resolvem ids novos, grupos homônimos, corridas implícitas adjacentes e `last_active`, e já têm teste de ordem resultante.

### 5. O arranque de runtimes é uma função só

O laço de `open_window_from_session` que hoje vai de `let to_start` até o fim do `for` (spawn da aba com `SpawnOrigin::Restored`, `profile_spawn::restored_profile` com o perfil salvo do painel focado, `spawn_restored_sibling_panes`) sai para um método de `WindowState`. A forma provável é `start_restored_tabs(&mut self, ids: &[TabId], …)`, com os mesmos parâmetros que o laço já usa (`cell_metrics`, `proxy`, `style`, `term_params`, `config`, `startup_directory`, `now`).

- `open_window_from_session` passa `track_order()` (eager) ou a aba ativa (lazy), como hoje.
- `append_named_window` passa os ids novos acrescentados (eager) ou só a ativa traduzida (lazy).

A decisão eager × lazy fica **fora** da função, nos dois chamadores, porque o conjunto de partida é diferente: a janela inteira num caso, só o que foi acrescentado no outro. O que é da aba restaurada, como `cwd` com fallback (RF-3.10), `.porecatu` uma vez no painel focado, perfil salvo e painéis irmãos, fica **dentro**, e não pode divergir sem alguém escrever um `if`.

### 6. O que não é tocado

- **Geometria da janela.** `append_named_window` não lê `window_v1.geometry` nem `monitor`. A janela fica com o tamanho, a posição, a maximização e a tela cheia que tem (RF-23.11).
- **Tema e zoom.** `apply_restored_session_state` não é chamada, como já não é na restauração nomeada (ADR-0054 §6 item 2).
- **Fixadas.** `open_pinned_tabs_into` não é chamada, e `window_v1.active_pinned` é ignorado (RF-21.13, [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md) §5). `insert_detached_*` nunca usa `InsertAt::Pinned` aqui.
- **Garantia de terminal mínimo.** `window_needs_a_minimum_tab` não entra: a janela de origem já tem abas. Uma sessão sem aba a restaurar não acrescenta nada (RF-23.13).
- **O que já estava na janela.** Nenhuma chamada de `take_*` é feita sobre o workspace da janela. As abas e os grupos existentes só ganham vizinhos à direita.

### 7. Sessão automática e retrato

Nada muda no `session_writer`. As abas acrescentadas são abas da janela, e a próxima gravação as inclui (RF-23.14). `WindowState` continua sem guardar de qual sessão nomeada algo nasceu (ADR-0054 §8), e é isso que mantém o retrato sem vínculo. Restaurar duas vezes acrescenta duas vezes (RF-23.15), porque não há o que comparar.

### 8. Onde o código mora

| Crate | O que muda |
|---|---|
| `porecatu-config` | campo `restore_named_in_current_window` em `Session`, default `false`, teste do default |
| `porecatu-ui` | `reload::diff` ignora o campo (§2); `option!` no catálogo da tela, rótulo e descrição em `messages.rs` (§3); `append_named_window` e o desvio em `restore_named_session` (§4); `start_restored_tabs` extraída de `open_window_from_session` (§5) |
| `locales/` | rótulo e descrição em todos os idiomas |
| `porecatu-core` | **nada**: `take_group`, `take_tabs`, `insert_detached_group`, `insert_detached_tabs` e `InsertAt::TrackEnd` já existem (ADR-0090) |
| `porecatu-session`, `porecatu-term`, `porecatu-pty`, `porecatu-render`, `porecatu-ssh` | **nada** |

Testes que o PR de implementação traz:

- **ordem resultante** da trilha depois de acrescentar uma sessão com grupo, aba solta e grupo, a uma janela com grupo e aba solta, seguindo a convenção do CLAUDE.md de que o core andando à frente da UI fixa a ordem e não só invariantes;
- grupo homônimo não fundido;
- aba ativa traduzida;
- `lazy_restore` iniciando só a ativa;
- nenhum id da janela original alterado;
- escopo `Live` na tela;
- `diff` sem aviso para a chave sozinha e com aviso quando outra chave de `[session]` muda junto.

## Alternativas consideradas

### Substituir as abas da janela

Recusada pelo dono do produto no pedido. Encerraria processos vivos por um gesto de "abrir", pediria diálogo de confirmação com a detecção de processo ativo ([ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md)) e uma regra para as fixadas. É a razão do PRD-014, que continua valendo.

### Perguntar a cada restauração

Um diálogo "nova janela ou esta?" em todo clique. Recusada: o pedido foi uma opção fixa, e um diálogo em todo gesto de abrir é o custo que a opção existe para evitar.

### Fundir com grupo de mesmo nome

Restaurar "api" numa janela que já tem "api" juntaria as abas num grupo só. Recusada pela mesma razão do RF-22.5 e do ADR-0090: fundir por nome é adivinhar, e um grupo de mesmo nome não é necessariamente o mesmo contexto. Juntar é um arraste.

### Um merge próprio no core (`Workspace::append_window`)

Uma função que receba grupos e abas e os anexe. Recusada: seria o segundo caminho de inserção de abas vindas de fora, ao lado do `insert_detached_*` do ADR-0090, e os dois divergiriam em ids, corridas implícitas adjacentes e `last_active`. O workspace temporário custa uma alocação por gesto do usuário e não pede código novo no core.

### Converter o `WindowV1` direto para dentro do workspace da janela

Uma segunda versão de `workspace_from_window` que recebesse o destino. Recusada porque duplicaria a conversão: `NotStarted`, SSH dormente e árvore de painéis passariam a ter dois lugares, contra a terceira métrica do PRD-014.

### Inserir depois da aba ativa, ou dentro do grupo ativo

Mais perto de onde o olho está. Recusada: dentro do grupo ativo misturaria abas da sessão num grupo que não é dela, e depois da aba ativa quebraria grupos da janela ao meio quando a ativa está num grupo. O fim da trilha nunca toca o que existe, e a trilha rola até a aba ativada (§4 passo 5).

### Chave de classe C, como o resto de `[session]`

Sem exceção no `diff`. Recusada porque a chave não tem estado a migrar: é lida no clique. Pedir reinício para uma escolha que vale no próximo clique seria mentir no aviso, e o ADR-0030 classifica pela natureza da chave, não pela seção.

### Aplicar o tamanho salvo à janela atual

Recusada (RF-23.11): a janela atual pode estar maximizada, em tela cheia ou ajustada a um monitor, e redimensioná-la por ter acrescentado abas seria efeito fora do alvo do gesto.

## Consequências

### Positivas

- **Zero código no core, zero schema novo, zero dependência nova.** O recurso é uma chave, um desvio em `restore_named_session` e uma extração de função.
- A extração do §5 torna o arranque de runtimes restaurados **uma função só** para os três usos (arranque, janela nova nomeada, janela atual). Isso fortalece a métrica do PRD-014 em vez de só preservá-la.
- Quem não liga a opção não vê diferença: o default é o comportamento do PRD-014, pelo mesmo código.

### Negativas

- **Primeira chave classe A em `[session]`.** O cabeçalho do arquivo de exemplo diz "Reinicie o app depois de mudar" para a seção inteira, e a linha da chave precisa desmentir isso para si mesma. O `diff` ganha um caso especial.
- **A barra pode encher rápido.** Restaurar duas sessões grandes numa janela só leva ao overflow com rolagem. É o uso pedido, e a roda do mouse rola a trilha ([ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md)).
- **Grupos com o mesmo nome** passam a ser um estado comum com a opção ligada (restaurar duas vezes, ou uma sessão salva da própria janela), e não só depois de mover grupos entre janelas.
- **Duas seções de um ADR aceito** (ADR-0054 §6 e §8) e **um item de "Fora de escopo" do PRD-014** ficam parcialmente revistos.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| A restauração na janela atual ganhar um arranque de runtime próprio e divergir da do arranque | Média | **Alto** | §5: uma função só, chamada pelos dois destinos; teste que restaura o mesmo `WindowV1` numa janela nova e na atual e compara as abas acrescentadas (estado, `cwd`, árvore de painéis) com as da janela nova |
| Id traduzido errado deixar runtime chaveado por um `TabId` que não existe, ou ativar a aba errada | Média | Médio | Os ids saem de `insert_detached_*`, nunca recalculados; teste da aba ativa traduzida e teste de que todo `TabId` com runtime existe no workspace |
| Acrescentar alterar a ordem ou o estado do que já estava na janela | Baixa | **Alto** | Nenhum `take_*` sobre o workspace da janela (§6); teste de ordem resultante com o prefixo original intacto |
| O aviso de "reinicie" aparecer ao mudar só a chave nova, ou sumir quando outra chave de `[session]` muda junto | Média | Baixo | §2; dois testes de `reload::diff` e o teste de escopo da tela |
| Fixada ser acrescentada por `active_pinned` ou por `InsertAt::Pinned` | Baixa | Médio | §6; `append_named_window` não lê `active_pinned`; teste com janela que tem fixadas |
| Usuário ligar a opção e esperar substituição | Baixa | Baixo | Rótulo e descrição dizem "acrescenta" e "sem fechar o que está aberto"; o guia do usuário registra numa frase |
