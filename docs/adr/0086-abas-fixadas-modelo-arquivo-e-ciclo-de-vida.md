# ADR-0086 — Abas fixadas: lista própria no `Workspace`, arquivo `pinned.json` ao lado da sessão, abertura uma vez por execução e fixar como gesto de confiança

**Status:** Aceito
**Data:** 2026-11-06
**Relacionados:** [ADR-0005](0005-persistencia-de-sessao.md), [ADR-0006](0006-modelo-de-abas-e-grupos.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0015](0015-multiplas-janelas.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0020](0020-grupos-explicitos.md), [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0036](0036-formato-do-arquivo-de-sessao.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0042](0042-hyperlinks-osc-8.md), [ADR-0051](0051-arquivo-de-projeto-porecatu.md), [ADR-0053](0053-paineis-divididos.md), [ADR-0054](0054-sessoes-nomeadas.md), [ADR-0071](0071-backend-de-terminal.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md), [ADR-0080](0080-editor-de-perfil-em-janela-propria.md), [PRD-021](../prd/prd-021-abas-fixadas.md)
**Supersedes (parcial):** ADR-0020 §2 e §3 (a ordem visual ganha as fixadas na frente; a escada de foco ganha a zona) · ADR-0036 §3 (as fixadas ficam fora do que a sessão grava; `WindowV1` ganha um campo opcional) · ADR-0051 §1 e §4 (a fixada aberta no arranque dispara o `.porecatu` sem `trusted_paths`) · ADR-0017 §6, revisto pelo ADR-0053 §10 (o último painel de uma fixada não fecha a aba com código zero)

## Contexto

O [PRD-021](../prd/prd-021-abas-fixadas.md) pede abas que ficam fora dos grupos, numa zona própria ao lado do botão do app, e que abrem a cada execução do app **independentemente da sessão**, como apps que sobem junto com o Porecatu. O PRD decide o comportamento. Seis coisas ficam para este ADR, e cada uma tem uma resposta errada tentadora:

1. **Onde a fixada mora no modelo.** A tentação é um grupo especial, "Fixadas". Mas o `Workspace` é `Vec<Group>` de `Vec<TabId>` ([ADR-0006](0006-modelo-de-abas-e-grupos.md), revisto pelo [ADR-0020](0020-grupos-explicitos.md)), e grupo tem nome, cor, colapso, contiguidade, fusão de *runs* implícitos e remoção quando esvazia — todas regras que a fixada não tem, e cada uma ganharia um `if`.
2. **Como uma aba viva se liga à entrada da lista.** A lista é do processo e sobrevive à aba; a aba é da janela e tem um `TabId` que não sobrevive à execução. Ligar as duas pelo índice quebra no primeiro reordenar.
3. **Onde a lista é gravada e em que formato.** A tentação é o `session.json` — já tem as abas. Mas o PRD exige que as fixadas abram com a sessão desligada, inválida, de schema mais novo ou ignorada pelo modo posicional (RF-21.13).
4. **Quando e onde elas abrem.** O arranque tem quatro caminhos (sessão restaurada, sessão desligada, modo posicional, sessão inválida), e a fixada precisa entrar nos quatro, uma vez, sem entrar em `window.new` nem nas sessões nomeadas.
5. **Como o `.porecatu` confia na fixada.** O [ADR-0051](0051-arquivo-de-projeto-porecatu.md) §4 decidiu que só `trusted_paths` autoriza, e o §8 dele recusou a autorização dada por gesto porque ela transformaria um arquivo de estado em fronteira de segurança. O dono decidiu que fixar é o gesto de confiança (RF-21.16), e este ADR tem de dizer o que isso custa e como fica de pé.
6. **O que fechar, sair e desafixar fazem com a lista.** Fechar desafixa (RF-21.22), mas o processo sair sozinho não (RF-21.24), e fechar a janela também não (RF-21.25). As três coisas passam hoje pelo mesmo caminho de remoção de aba.

## Decisão

**A fixada é uma `TabId` numa lista própria do `Workspace`, fora de todo grupo. A lista de fixadas do processo vive em `pinned.json`, ao lado do `session.json`, escrita só por gesto, como retratos de aba no mesmo `TabV1` da sessão. As fixadas abrem uma vez por execução, na primeira janela do arranque, com origem de spawn própria que dispara o `.porecatu` sem exigir `trusted_paths`. A sessão nunca as grava.**

### 1. Modelo: `Workspace::pinned`, fora de `Vec<Group>`

```rust
pub struct Workspace {
    pinned: Vec<TabId>,      // novo: a zona, na ordem da zona
    groups: Vec<Group>,
    tabs: Vec<Tab>,          // dados de todas as abas, fixadas inclusive
    active_tab: Option<TabId>,
    // ...
}
```

**Invariante:** toda `TabId` de `tabs` está em **exatamente um** lugar — em `pinned` ou em um `Group`. O teste de invariante do `Workspace` passa a contar os dois.

Os dados da aba (`Tab`, com a árvore de painéis) não mudam e não ganham campo: "fixada" é **onde a aba está**, não um atributo dela. É o que mantém `Tab` igual para todo o resto do código — busca, painéis, perfil, indicadores, estado.

As ordens, revendo o [ADR-0020](0020-grupos-explicitos.md) §2:

| Ordem | Definição | Quem usa |
|---|---|---|
| `track_order()` | grupos na ordem do `Vec`, abas na ordem do grupo — **a definição antiga de `visual_order()`** | a trilha, a sessão |
| `visual_order()` | `pinned` seguido de `track_order()` | a barra inteira |
| `navigable_order()` | `pinned` seguido das abas de grupo não colapsado | `tab.next`, `tab.prev`, `tab.goto_N` (RF-21.4) |

A zona nunca colapsa, então `navigable_order()` continua sendo subsequência de `visual_order()` por filtro, nunca construída em paralelo, como o ADR-0020 exige.

Operações novas, todas em `porecatu-core` e todas com teste que fixa a **ordem resultante** (regra do [CLAUDE.md](../../CLAUDE.md) desde a F3):

- `pin_tab(id, pos)` — tira a aba do grupo (grupo vazio é removido e *runs* implícitos vizinhos se fundem, pelo `normalize_groups` de sempre, RF-2.7) e a insere em `pinned[pos]`. Não ativa nem desativa nada.
- `unpin_tab(id, target)` — tira de `pinned` e põe na trilha. `target` é `TrackStart` (o menu e a ação, RF-21.10: um *run* implícito novo na frente de todos os grupos) ou o destino de arraste que o [ADR-0006](0006-modelo-de-abas-e-grupos.md) já conhece (grupo e posição, ou *run* novo num índice).
- `move_pinned(id, pos)` — reordena dentro da zona. A reordenação por teclado (RF-1.17) usa esta operação quando a aba ativa é fixada e **para na borda** (RF-21.11).
- `insert_pinned_tab_with_panes(tree, pos) -> TabId` — o par do `insert_tab_with_panes` da restauração, usado na abertura (§5).
- `is_pinned(id)`, `pinned()`.

As operações de grupo **recusam** uma fixada (`false`, como recusam um id que não existe): `move_tab_to_group`, `group_tabs` (que a filtra da seleção), `move_tab` (que é "dentro do grupo"). Atravessar a fronteira da zona é sempre `pin_tab` ou `unpin_tab`, chamada pela UI a partir de um gesto que diz isso (RF-21.11).

**Foco depois de fechar**, revendo o ADR-0020 §3: fechar uma fixada ativa vai para a fixada seguinte; não havendo, para a anterior; não havendo, para a aba que a escada da trilha daria a partir do início. Fechar uma aba da trilha segue a escada de sempre, e, se ela não achar nada, cai na **última** fixada (RF-21.25) — é isso que impede `close_tab` de devolver `None` e a UI de fechar uma janela que ainda tem fixadas (RF-1.4).

### 2. Retrato e identidade: o registro de fixadas do processo

A lista de fixadas é **do processo**, não da janela — o mesmo lugar que o estado por repositório do [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md) ocupa em `App`:

```rust
struct PinnedRegistry {
    entries: Vec<PinnedEntry>,                       // a ordem do arquivo
    live: HashMap<PinnedKey, (WindowId, TabId)>,     // quem está aberta agora, e onde
    writable: bool,                                  // false com schema mais novo (§3)
}

struct PinnedEntry {
    key: PinnedKey,      // interna, do processo; nunca gravada
    saved: TabV1,        // o retrato (RF-21.9)
}
```

**`PinnedKey` é a identidade, não o índice** — a mesma lição do editor de perfil do [ADR-0080](0080-editor-de-perfil-em-janela-propria.md), que trabalha numa cópia identificada por uma chave interna do item. Reordenar, desafixar outra aba ou fixar uma nova não muda a chave de ninguém. A chave nasce na carga do arquivo e no gesto de fixar; não vai para o disco, porque o arquivo só é escrito por este processo e a posição basta para relê-lo.

**O retrato** é o `TabV1` que a sessão gravaria para a aba naquele instante — árvore de painéis com proporções, `cwd`, programa e perfil de cada painel, painel focado, título customizado (RF-21.9). A função que monta um `TabV1` a partir de uma `Tab` sai de dentro de `window_from_workspace` e passa a ser pública em `porecatu-session::convert`, chamada pelos dois. **Dois caminhos que gravam a mesma aba de duas formas** era exatamente o risco que o ADR-0054 §6 eliminou para as sessões nomeadas, e aqui ele é eliminado do mesmo jeito: não existe um segundo.

O que muda na aba viva depois de fixada — `cd`, painel novo — **não** toca o retrato. É o "retrato, não vínculo" do RF-14.15, e é também o que permite ao `pinned.json` ser escrito só por gesto (§3).

**Ordem com várias janelas.** Uma fixada pode estar viva em qualquer janela (RF-21.26), e a lista é uma só. A zona de uma janela mostra as entradas vivas naquela janela **na ordem da lista**. Os gestos traduzem assim:

- reordenar na zona permuta as entradas daquela janela **entre as posições que elas já ocupam** na lista — as entradas das outras janelas não se movem;
- fixar insere a entrada **logo depois** da entrada da vizinha à esquerda na zona; sem vizinha à esquerda, **logo antes** da primeira entrada daquela janela; sem nenhuma entrada naquela janela, no **fim** da lista.

No arranque seguinte todas abrem na primeira janela, na ordem da lista, e a regra acima garante que essa ordem é a que o usuário via em cada zona.

### 3. Arquivo: `pinned.json`, escrito só por gesto

```
<diretório de estado>/porecatu/
  session.json
  pinned.json          ← novo
  sessions/
```

O caminho é **derivado do `session.json` resolvido** — `session_dir(resolve_session_path()) / "pinned.json"` —, exatamente como o diretório das sessões nomeadas ([ADR-0054](0054-sessoes-nomeadas.md) §2). `PORECATU_SESSION` desloca os três juntos, e a costura de teste continua sendo uma. Fora da config pela razão do [ADR-0005](0005-persistencia-de-sessao.md): `cwd` absoluto é estado da máquina (RF-21.27).

```rust
pub const PINNED_SCHEMA_VERSION: u32 = 1;

pub struct PinnedFileV1 {
    pub schema_version: u32,
    #[serde(default)]
    pub tabs: Vec<TabV1>,     // o mesmo TabV1 da sessão, com `panes` sempre presente
}
```

**Envelope próprio, conteúdo compartilhado.** O arquivo tem versão própria porque é outro arquivo — reaproveitar `SessionFileV1` traria geometria, monitor, tema e zoom que não significam nada aqui. Mas cada aba é um `TabV1`: quando o schema de aba mudar (como mudou no ADR-0053 §11 e no ADR-0070 §4), a sessão e as fixadas mudam juntas, pela mesma conversão. `TabV1::id` é gravado como a posição e ignorado na leitura, como já é na sessão.

**API em `porecatu-session`**, módulo `pinned.rs`, na forma de `load_from`/`save_to`:

```rust
pub enum PinnedLoadOutcome {
    Missing,                         // lista vazia, não é erro (RF-21.29)
    Loaded(PinnedFileV1),
    Quarantined { reason: String },  // inválido: renomeado para .corrupt.N, lista vazia
    NewerSchema { found: u32 },      // preservado, lista vazia, registro somente leitura
}

pub fn pinned_path() -> Option<PathBuf>;
pub fn load_pinned_from(path: &Path) -> PinnedLoadOutcome;
pub fn save_pinned_to(path: &Path, file: &PinnedFileV1) -> io::Result<()>;
```

Escrita atômica e quarentena são as do `session.json` (RF-3.5, RF-3.14, [ADR-0036](0036-formato-do-arquivo-de-sessao.md) §5), extraídas para funções internas do crate que os dois arquivos chamam. Os três desfechos que não são `Loaded` viram aviso do app, menos `Missing`.

**Quando ler:** uma vez, no arranque, na main thread, antes da primeira janela. São poucos kilobytes, como o `session.json` que já é lido no mesmo ponto.

**Quando escrever:** **na hora**, a cada gesto que muda a lista — fixar, desafixar, fechar uma fixada, reordenar a zona —, síncrono, sem o *debounce* da sessão (RF-3.3). O *debounce* existe porque a sessão muda a cada `cd` e a cada aba; a lista de fixadas muda quando o usuário diz, e perder o último gesto num crash dentro da janela de dois segundos é perder justamente o que ele acabou de pedir. Falha de escrita é aviso com a razão (RF-21.28); o gesto continua valendo nesta execução, e o aviso diz que não foi gravado.

**Nunca escrever por derivação.** Encerrar o app, fechar a janela, o processo sair — nada disso reescreve o arquivo, porque nada disso muda a lista (RF-21.24, RF-21.25). O arquivo em disco é sempre o último gesto, não o último estado.

**Schema mais novo** (RF-21.29): o registro abre com `writable = false` e lista vazia. Fixar nesse estado é **recusado** com aviso — a aba não é fixada. Fixar em memória sem gravar mostraria na barra uma fixada que não volta no próximo arranque, e esse é o tipo de mentira que o RF-3.16 existe para evitar.

`[session] enabled` não alcança o arquivo (RF-21.27), pela razão do ADR-0054 §8: desligar a sessão automática é não querer que o app lembre sozinho, e a fixada é lembrança pedida.

### 4. A sessão não grava fixadas

`window_from_workspace` passa a percorrer `track_order()` em vez de `visual_order()`. Uma fixada nunca entra em `WindowV1::tabs` nem em grupo nenhum — nem no `session.json`, nem numa sessão nomeada, que usa a mesma função ([ADR-0054](0054-sessoes-nomeadas.md) §6). Sem isso, toda fixada abriria duas vezes no arranque: pela lista e pela sessão.

**A aba ativa.** Se a ativa da janela é uma fixada, `WindowV1::active_tab` é gravado `None`, e um campo novo diz qual fixada era:

```rust
pub struct WindowV1 {
    // ...
    /// ADR-0086 §4: posição em `pinned.json` da fixada ativa, quando a
    /// ativa era uma fixada. Só a janela que recebe as fixadas no
    /// arranque o honra.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_pinned: Option<u32>,
}
```

**`CURRENT_SCHEMA_VERSION` continua 1**, pela propriedade que o ADR-0036 §1 comprou e que o ADR-0053 §11 e o ADR-0054 §4 já usaram: campo ausente tem significado (`None`), e um arquivo gravado sem fixada ativa sai byte a byte igual ao de antes. Na restauração, a janela que recebe as fixadas (§5) ativa a fixada daquela posição, se ela existir; as outras janelas, e uma posição que não existe mais, caem na primeira aba navegável da trilha. A sessão nomeada grava sempre `None`: ela nunca abre fixadas.

Consequência aceita: com `lazy_restore = true` e uma fixada ativa no encerramento, nenhuma aba da trilha da primeira janela sobe no arranque — a ativa era a fixada, e a fixada sobe sozinha (§5).

### 5. Abertura: uma vez por execução, na primeira janela, com origem própria

`App` ganha `pinned_opened: bool`. No arranque, **depois** que a primeira janela existe — pelo caminho que for: `open_window_from_session` da primeira `WindowV1`, a janela nova da sessão desligada ou ausente, a do modo posicional ([ADR-0040](0040-superficie-de-linha-de-comando.md)), a do arquivo inválido —, `open_pinned_tabs(window)` roda uma vez e liga a flag. `window.new` e a restauração nomeada nunca a chamam (RF-21.13). Fechar a primeira janela não reabre as fixadas em lugar nenhum: a flag continua ligada até o processo acabar.

**Antes do primeiro quadro.** `open_pinned_tabs` roda antes do primeiro redesenho da janela, de modo que a zona e o `track_shift` com a contagem de fixadas ([ADR-0087](0087-zona-de-abas-fixadas-na-barra.md) §1) já nascem no primeiro quadro — inserir depois faria a trilha pular 46 px por fixada logo depois de aparecer. Só o ícone chega depois ([ADR-0088](0088-icone-da-aba-fixada.md) §4), com o ícone do app no lugar até lá.

Para cada entrada, na ordem da lista: `insert_pinned_tab_with_panes` com a árvore do retrato, e o spawn de cada painel com **`SpawnOrigin::Pinned`**, uma terceira variante ao lado de `New` e `Restored`:

| | `New` | `Restored` | `Pinned` |
|---|---|---|---|
| `cwd` gravado, com o fallback do RF-3.10 | — | sim | sim |
| sobe no arranque | — | só com `lazy_restore = false` ou ativa | **sempre** (RF-21.15) |
| painel SSH volta dormente ([ADR-0071](0071-backend-de-terminal.md), RF-20.33) | — | sim | sim |
| candidata a `.porecatu` | nunca | com `trusted_paths` | **sem** `trusted_paths` (§6) |

A inserção **não ativa** nada: a ativa continua a que a sessão (ou o §4) disse (RF-21.14). O registro liga cada `PinnedKey` a `(WindowId, TabId)` em `live`.

Diretório que não existe mais segue o RF-3.10 pelo caminho de spawn de sempre, e a entrada **fica** na lista (RF-21.30): o §3 não escreve por derivação.

O custo de subir N shells a mais no arranque é real, e é o que o PRD pediu. Os spawns já são assíncronos em `porecatu-term`; o que se mede é o tempo até o primeiro prompt **da aba ativa**, em A/B intercalado (armadilha do [CLAUDE.md](../../CLAUDE.md), "Carga pesada no arranque").

### 6. `.porecatu`: fixar é o gesto de confiança

`project_file_candidate_dir` aceita `Restored` **e** `Pinned`. `porecatu_config::project_file::resolve` ganha um parâmetro:

```rust
pub enum Trust {
    Allowlist,   // ADR-0051 §4: o diretório tem de estar sob trusted_paths
    Pinned,      // ADR-0086 §6: a fixada já é a autorização
}
```

Com `Trust::Pinned`, `resolve` pula `is_trusted` e segue para a leitura. **A ordem dos passos não muda**: `enabled = false` continua sendo o primeiro, antes de qualquer leitura de disco (RF-12.7, RF-21.16). Todo o resto do ADR-0051 §6 vale inteiro: painel focado, uma vez por aba, primeiro byte mais silêncio, teto de 5 s, guarda de tela alternativa. Fixar uma aba viva **não** dispara nada (RF-21.8): não há spawn, e o gatilho é o spawn.

**Por que isto não reabre o que o ADR-0051 recusou.** A alternativa recusada lá — autorização por gesto, gravada em `session.json` — caiu porque faria de um arquivo **de conveniência** uma fronteira de segurança: o app apaga e recria o `session.json` sem cerimônia, e a perda dele não custa nada. O `pinned.json` nasce do outro lado dessa linha, e as regras do §3 são as que um arquivo de confiança precisa:

- é escrito **só por gesto** explícito do usuário, nunca por derivação de estado;
- arquivo inválido vai para quarentena e a lista abre **vazia** — falha fechada, nunca uma lista parcial;
- schema mais novo nunca é sobrescrito, e o registro abre somente leitura;
- a autorização é de **um diretório exato** por fixada, mais estreita que qualquer entrada de `trusted_paths`, que cobre a árvore inteira.

O que fica, e é dito em voz alta: quem escreve no diretório de estado do usuário consegue que um `.porecatu` rode no arranque seguinte. Isso já era verdade para quem escreve no diretório de config (`trusted_paths`), com privilégio igual. E a autorização é do **diretório**, não do **conteúdo** — um `.porecatu` que muda por `git pull` depois de fixado roda sem ser visto. É o mesmo custo do `trusted_paths`, aplicado a um diretório por vez; o [PRD-021](../prd/prd-021-abas-fixadas.md) o registra, e a referência do arquivo de projeto passa a dizê-lo.

### 7. Fechar, sair, fechar a janela, desafixar

**Fechar uma fixada** — `tab.close`, clique do meio, item do menu — abre sempre o diálogo `DialogAction::ClosePinnedTab(TabId)`, **fora** do alcance de `[general] confirm_close_with_process` (RF-21.23). Quando a aba também confirmaria pelo RF-1.6 — processo em primeiro plano, conexão SSH aberta ([ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md), revisto pelo ADR-0071) —, a frase de processo ativo entra **no mesmo diálogo**, como uma linha a mais: nunca dois diálogos em sequência. Confirmado, o encerramento é o de sempre ([ADR-0017](0017-ciclo-de-vida-da-aba.md) §4, ConPTY sem fechar o pseudo-console), seguido de `Workspace::close_tab`, da remoção da entrada e da escrita do §3.

**O processo sair sozinho** (RF-21.24): `TermEvent::Exit` com código zero fecha o painel, como o ADR-0053 §10 decidiu, **exceto o último painel de uma fixada**, que fica `Exited` e mantém a aba. Painéis que não são o último fecham como sempre. A aba fixada com o painel `Exited` não muda a lista; fechá-la cai no parágrafo anterior. A decisão é tomada em `porecatu-ui`, no tratamento de `Exit`, perguntando `Workspace::is_pinned` — não há estado novo.

**Fechar a janela** (RF-21.25): as fixadas encerram com as outras abas, as entradas de `live` daquela janela são apagadas, e a lista **não** muda. A confirmação do RF-10.23 conta as fixadas como abas.

**Desafixar** — item do menu, `tab.toggle_pin`, arraste para fora: `unpin_tab`, remoção da entrada, escrita. O processo segue vivo.

**A ação** `tab.toggle_pin` entra no enum de ações ([ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md)) como `Action::TabTogglePin`, sobre a aba ativa, **sem atalho default** (precedente de `group.new_tab`). No catálogo, `tab.pin` sai de "ações que não existem", e a linha aponta para este ADR.

### 8. Onde o código mora

| Crate | O que muda |
|---|---|
| `porecatu-core` | `Workspace::pinned`, as três ordens, `pin_tab`/`unpin_tab`/`move_pinned`/`insert_pinned_tab_with_panes`, a escada de foco, `Action::TabTogglePin` |
| `porecatu-session` | `pinned.rs` (`PinnedFileV1`, carga, gravação), `WindowV1::active_pinned`, `tab_v1_from_tab` público em `convert.rs`, `window_from_workspace` sobre `track_order()`, escrita atômica e quarentena extraídas |
| `porecatu-config` | `project_file::Trust` e o parâmetro em `resolve` |
| `porecatu-ui` | `PinnedRegistry` (módulo novo), `SpawnOrigin::Pinned`, `open_pinned_tabs`, `DialogAction::ClosePinnedTab`, a exceção do `Exit`, os itens de menu; a zona é do ADR-0087, o ícone do ADR-0088 |
| `porecatu-locale` / `locales/` | os textos do menu, do diálogo, dos avisos de arquivo e de acessibilidade, em todos os idiomas |
| `porecatu-term`, `porecatu-pty`, `porecatu-ssh`, `porecatu-render` | **nada** |

Nenhuma dependência nova. A regra de dependência da [arquitetura](../arquitetura.md) não ganha aresta: `porecatu-session` já depende de `porecatu-core`, e `porecatu-ui` já vê os dois.

## Alternativas consideradas

### Um grupo especial, `GroupKind::Pinned`, no começo de `Vec<Group>`

Reaproveitaria a barra, o arraste e a ordem visual sem mudança. Recusada porque o grupo carrega regras que a fixada não tem, e todas virariam exceção: não tem nome nem cor, não colapsa, não some quando esvazia (RF-2.7), não se funde com *run* implícito vizinho, não é alvo de "mover para grupo" nem de "fechar todas as abas do grupo" (RF-21.3). E a sessão, que grava grupos, precisaria saber pular um. Uma lista ao lado custa três ordens explícitas; um grupo especial custa um `if` em cada operação de grupo, para sempre.

### `pinned: bool` na `Tab`, com a aba continuando no grupo

O modelo dos navegadores, em que a aba fixada vai para o começo da faixa. Recusada pelo próprio PRD: a fixada **sai** do grupo (RF-21.3). Um booleano numa aba que continua dentro de um `Group` criaria o estado "fixada no grupo X", que nenhum requisito define e que toda operação de grupo teria de tratar.

### Gravar a lista dentro do `session.json`

Um arquivo, uma escrita, e a fixada viria com a janela. Recusada porque o PRD a quer **independente** da sessão (RF-21.13, RF-21.27): com `[session] enabled = false` o `session.json` não é lido; no modo posicional, também não; inválido, vai para quarentena e o app abre limpo — e as fixadas iriam junto em todos os três. Além disso, o `session.json` é gravado com *debounce* e reescrito a partir do estado vivo, que é exatamente o que o §3 recusa para a lista.

### Gravar a lista em `porecatu.toml`

Versionável, levável entre máquinas, editável à mão. Recusada pelo dono, pela razão do ADR-0005: a entrada carrega `cwd` absoluto e perfil de uma máquina, e levar isso a outra que não tem aqueles diretórios é levar uma lista de fixadas que abrem todas no home.

### Derivar a lista do estado vivo, gravando o que estiver na zona no encerramento

Sem retrato, sem escrita por gesto — o arquivo seria uma fotografia do fim da execução. Recusada por três motivos: um crash perde tudo desde a última gravação; a janela das fixadas fechada antes do encerramento as apagaria da lista (contra o RF-21.25); e o `cd` dentro de uma fixada mudaria o diretório de abertura, contra o RF-21.9.

### Identificar a entrada pelo índice, ou por um UUID gravado no arquivo

O índice quebra no primeiro reordenar e no primeiro desafixar de outra aba — o defeito que o ADR-0080 já evitou no editor de perfil. O UUID gravado resolveria, mas o arquivo só é escrito por este processo, e a chave interna faz o mesmo sem um campo a manter no disco.

### Escrever com o *debounce* da sessão

Coerente com o RF-3.3. Recusada porque o *debounce* resolve um problema que a lista não tem — mudança frequente — e cria um que ela não pode ter: o crash nos dois segundos depois de fixar perde a fixada.

### Fixar sem dispensar `trusted_paths`

O caminho mais conservador, e o recomendado antes da decisão. Recusada pelo dono do produto: a fixada é um app que se quer aberto, e exigir a edição do TOML para cada uma é o atrito que o recurso existe para tirar. O §6 registra o custo.

### Abrir as fixadas em toda janela nova

Recusada pelo PRD (RF-21.13): duplica o processo, e o caso de uso são servidores com porta.

## Consequências

### Positivas

- Nenhuma regra de grupo ganha exceção: a fixada não está em grupo nenhum, e as operações de grupo a recusam por construção.
- O retrato da fixada e a aba da sessão são o mesmo `TabV1`, montado pela mesma função — não há dois caminhos para divergir.
- A lista sobrevive a tudo que não é gesto de mudá-la: sessão desligada, sessão corrompida, modo posicional, `exit`, fechar a janela, crash.
- `porecatu-term`, `porecatu-pty`, `porecatu-ssh` e `porecatu-render` não mudam; nenhuma dependência nova.
- A dispensa de `trusted_paths` é um valor de enum passado a `resolve`, não um caminho paralelo de leitura do `.porecatu`.

### Negativas

- **`visual_order()` muda de definição**, depois de o ADR-0020 ter escrito que ela não mudaria. Todo chamador precisa ser revisto para escolher entre `visual_order()` e `track_order()` — a sessão é o caso conhecido, e o desenho da trilha é outro.
- **Um terceiro arquivo de estado**, com versão própria, quarentena e aviso próprios.
- **O `pinned.json` passa a carregar autorização de execução.** É estado da máquina com peso de config, e o diretório de estado passa a merecer o mesmo cuidado que o de config.
- **`WindowV1` ganha um campo** que só uma janela por arquivo usa, e só quando a ativa era fixada.
- **N shells a mais no arranque**, e com `lazy_restore` ligado eles sobem mesmo assim — é o pedido, mas é custo de arranque que antes não existia.
- **Duas instâncias do app** escrevem o mesmo `pinned.json`, e a última vence — o mesmo comportamento do `session.json`, com a diferença de que aqui cada escrita é um gesto que pode ser perdido.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Algum chamador de `visual_order()` gravar ou desenhar fixadas onde não deve (sessão com fixadas = abertura dupla) | Média | **Alto** | `track_order()` com nome próprio; teste de `window_from_workspace` com uma fixada que exige `tabs` sem ela; teste de arranque que conta as abas da primeira janela |
| `close_tab` devolver `None` com fixadas vivas e a UI fechar a janela | Baixa | Alto | Escada de foco que cai na última fixada (§1), com teste |
| Gesto de mudar a fronteira da zona por um caminho que não seja `pin_tab`/`unpin_tab` (fixada "no grupo") | Média | Médio | Invariante de lugar único no teste do `Workspace`; operações de grupo recusam fixada |
| `.porecatu` alterado depois de fixado rodar sem o usuário ver | Média | Alto | Registrado no PRD e na referência; o script é escrito no terminal, visível e interrompível (RF-12.8); `enabled = false` desliga tudo |
| Escrita no diretório de estado por terceiro injetar uma fixada que roda um `.porecatu` | Baixa | Alto | Mesmo privilégio que escrever em `trusted_paths`; arquivo inválido falha fechado (lista vazia) |
| Crash entre o gesto e a escrita | Baixa | Médio | Escrita síncrona no gesto, sem *debounce* (§3) |
| Arranque mais lento com muitas fixadas | Média | Médio | Spawns assíncronos já existentes; medição A/B intercalada do primeiro prompt da aba ativa |
| Último painel de fixada com `exit` 0 fechar a aba e desafixá-la por engano | Baixa | Alto | A exceção do §7 num ponto só (tratamento de `Exit`), com teste |
| Duas instâncias sobrescreverem a lista uma da outra | Baixa | Médio | Mesmo comportamento já aceito para o `session.json`; registrado |
