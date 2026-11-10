# ADR-0093 — Medição de processos do gerenciador de tarefas: amostrador em `porecatu-pty`, pedido pelo relógio da UI, uma thread só enquanto houver gerenciador aberto

**Status:** Aceito
**Data:** 2026-11-10
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0015](0015-multiplas-janelas.md), [ADR-0033](0033-job-object-encerramento-de-processo.md), [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md), [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md), [ADR-0053](0053-paineis-divididos.md), [ADR-0071](0071-backend-de-terminal.md), [ADR-0090](0090-transferencia-de-abas-entre-janelas.md), [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md), [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md)
**Supersedes:** ADR-0033 §3 (**parcial**: a frase "disparada só no fechamento e na consulta de contagem (nunca continuamente)" passa a ter uma terceira origem, a amostragem a 1 Hz com o gerenciador aberto; e `descendants_of` deixa de ser exclusiva do Windows. O critério de pertença — a união da lista do Job com a varredura — não muda, e o comportamento de `process_count`/`kill_tree` fora do Windows também não)

## Contexto

O [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md) pede, para cada janela, a lista dos processos das abas dela, com CPU, memória e disco por processo, e três gráficos com a soma. A medição é feita uma vez por segundo, numa thread, só enquanto algum gerenciador está aberto (RF-24.24). O PRD exige ainda que "processo desta aba" use **o mesmo critério** do fechamento de aba (RF-24.14).

O que já existe:

- `ProcessGroup` (`crates/porecatu-pty/src/job.rs`) conhece a raiz de cada painel (`root_pid`, privado) e, no Windows, o Job Object. `live_pids` faz a união do ADR-0033 §3 (raiz, lista do Job e `descendants_of`), mas é privada e só existe no Windows.
- O `sysinfo 0.40` (feature `system`) já é dependência de `porecatu-pty` nas três plataformas. Nenhum outro crate o usa.
- O `porecatu-term` re-exporta `ProcessGroup` e o guarda dentro do backend PTY (`PtyBackend.group`, `PtyInspect.group`, em `backend.rs`). Ele não chega à UI: `Terminal` não expõe PID nenhum.
- O resultado de thread volta à UI como **dado** por `EventLoopProxy::send_event(Wakeup::…)`, no padrão do [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md) (`Wakeup::GitQueryResult`). O `Wakeup` tem um orçamento de tamanho, vigiado por teste (`the_image_result_does_not_widen_wakeup`).
- Pelo CLAUDE.md, o **temporizador de UI é sempre `ControlFlow::WaitUntil`**, nunca thread. `App::schedule_next_wake` agrega os prazos.

Quatro perguntas ficam em aberto:

1. **Onde a medição mora** na árvore de crates.
2. **Quem marca o ritmo** e **quem faz o trabalho**.
3. **O que volta** para a UI, e como volta.
4. **Como encerrar** um processo escolhido na lista sem o risco de matar o processo errado.

## Decisão

**A medição é um módulo novo de `porecatu-pty` (`stats.rs`), re-exportado por `porecatu-term`, como já são `SpawnConfig` e `ProcessGroup`. O `App` marca o ritmo pelo `WaitUntil` e manda um pedido por canal a uma thread de amostragem, que vive só enquanto houver gerenciador aberto e guarda o `sysinfo::System` entre as amostras. A resposta volta como `Wakeup::ProcessSample(Box<…>)`, com dados chaveados por janela. Encerrar um processo também é um pedido a essa thread, com a identidade (PID e instante de início) conferida antes de matar.**

### 1. Onde mora: `porecatu-pty`, não um crate novo

O critério de pertença está em `job.rs`, junto com o Job e o `sysinfo`. Medir em outro crate exigiria expor o Job (um `win32job::Job`) para fora de `porecatu-pty` ou duplicar a união, e as duas coisas quebram a regra do RF-24.14 (um critério só). Então o módulo novo `crates/porecatu-pty/src/stats.rs` fica ao lado de `job.rs` e usa as funções dele.

`porecatu-term` re-exporta os tipos públicos do módulo, como faz com `SpawnConfig`/`PtySize`/`PtyError`. A regra do CLAUDE.md continua valendo: nenhum tipo do `sysinfo` atravessa a fronteira, porque os tipos públicos são do próprio `porecatu-pty`. A árvore de dependências não muda e nenhum crate novo entra.

### 2. O critério de pertença vira uma função só, nas três plataformas

Em `job.rs`:

- `descendants_of` perde o `#[cfg(windows)]`. Ela só usa `sysinfo`, que já compila nas três plataformas.
- `live_pids` vira `ProcessGroup::members(&self, system: &sysinfo::System) -> HashSet<u32>`, sem o `refreshed_system()` dentro: o **chamador** fornece o `System` já atualizado. No Windows, a função une a raiz, a lista do Job e a varredura. Fora do Windows, une a raiz e a varredura.
- `process_count` e `kill_tree` no Windows passam a chamar `members` com o `refreshed_system()` de sempre. **Fora do Windows os dois não mudam** (`1` e no-op): mudar o que o fechamento de aba mata no Linux e no macOS é a dívida Unix do ADR-0033 ("Riscos"), não este recurso.
- `ProcessGroup::root_pid(&self) -> u32` passa a ser público.

Assim, "processo desta aba" é decidido num lugar só, para o fechamento, para a contagem e para o gerenciador.

### 3. Do painel ao PID: `Terminal::process_group`

O trait `Inspect` (`crates/porecatu-term/src/backend.rs`) ganha `fn process_group(&self) -> Option<ProcessGroup>`. `PtyInspect` devolve um clone do seu (`ProcessGroup` é `Clone`, e o Job por dentro é `Arc`). O backend SSH devolve `None`: o cliente é embutido e não há processo local ([ADR-0071](0071-backend-de-terminal.md), RF-24.17). `Terminal::process_group()` repassa.

Um painel sem `PaneRuntime` (aba `NotStarted`, [ADR-0037](0037-aba-nao-iniciada.md)), um painel SSH dormente e um painel encerrado não entram no pedido. A UI os mostra com a nota do RF-24.17, pela regra do [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §5.

### 4. O relógio é da UI; o trabalho é da thread

O `App` ganha:

```rust
/// ADR-0093 §4: só `Some` enquanto ao menos uma janela tem o gerenciador aberto.
task_manager_sampler: Option<SamplerHandle>,
/// Próximo pedido de amostra; entra no `min()` de `schedule_next_wake`.
task_manager_next_sample: Option<Instant>,
```

- **Abrir o primeiro gerenciador** cria o `SamplerHandle`: um `mpsc::Sender<SamplerRequest>` e a `JoinHandle` de uma thread nomeada `porecatu-sampler`. Ele também agenda `next_sample = now`.
- **No prazo** (tratado em `tick_all`, como os outros prazos), o `App` monta um `SampleRequest` e o envia. O prazo seguinte é `now + 1 s`. **Se a resposta anterior ainda não chegou**, este tick não envia nada e só reagenda: nunca se forma fila, e uma máquina lenta mede com menos frequência sem acumular atraso.
- **Fechar o último gerenciador** (ou fechar a última janela que tinha um aberto) derruba o `SamplerHandle`. O `Sender` some, o `recv` da thread falha, e a thread termina e solta o `System`. O `JoinHandle` não é juntado na main thread: a thread termina sozinha. Daí em diante não há medição nenhuma (RF-24.24, métrica "zero varreduras").

A thread é **uma por processo**, não uma por janela: uma varredura do sistema serve a todas as janelas abertas. Ela é longa porque a CPU de um processo é a **diferença** entre duas atualizações do mesmo `System`, e por isso o `System` tem de sobreviver entre as amostras. Uma thread por pedido, como no Git, perderia essa diferença e mostraria zero sempre.

Isso respeita o ADR-0007: o temporizador é o `WaitUntil` (nenhuma thread dorme para marcar o ritmo), e a I/O pesada sai da main thread.

### 5. O pedido e a resposta

```rust
pub enum SamplerRequest {
    Sample(SampleRequest),
    Kill(KillRequest),        // §7
}

pub struct SampleRequest {
    pub seq: u64,
    /// Um item por painel com processo local, de todas as janelas com o
    /// gerenciador aberto. `K` é a chave opaca do chamador.
    pub roots: Vec<(K, ProcessGroup)>,
}

pub struct ProcessStat {
    pub pid: u32,
    pub parent: Option<u32>,
    pub start_time: u64,      // segundos desde a época, do `sysinfo`
    pub name: String,
    pub cpu: Option<f32>,     // 0.0..=1.0 da máquina inteira; `None` na 1ª amostra
    pub memory: u64,          // bytes residentes (working set no Windows)
    pub disk_per_sec: f64,    // bytes lidos + escritos por segundo
}

pub struct SampleResult {
    pub seq: u64,
    pub taken_at: Instant,
    /// Para cada chave do pedido, os membros vivos da árvore daquele painel.
    pub panes: Vec<(K, Vec<ProcessStat>)>,
}
```

`K` é genérico no `porecatu-pty` e, na UI, é `(WindowId, TabId, PaneId)`. O crate não conhece janela nem aba.

Em cada pedido, a thread:

1. atualiza o `System` **uma vez**: `refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage())`. O pai e o instante de início vêm sempre com o processo;
2. para cada raiz, calcula `members(&system)`, o mesmo do §2, e monta os `ProcessStat`;
3. normaliza a CPU: o `cpu_usage()` do `sysinfo` é em % de **um** núcleo, então é dividido por 100 e pelo número de CPUs lógicas (`System::cpus().len()`, carregado na criação; se vier zero, cai em `std::thread::available_parallelism`). Na primeira amostra de um `System` novo, a CPU é `None` (RF-24.9);
4. calcula o disco: o `disk_usage()` dá os bytes **desde a atualização anterior**, divididos pelo tempo real entre as duas atualizações (`Instant` da thread), não por 1 s fixo;
5. envia `Wakeup::ProcessSample(Box<SampleResult>)`. O `Box` mantém o `Wakeup` dentro do orçamento, e o teste de tamanho que já existe passa a cobrir a variante nova.

Um processo membro de **dois** painéis (impossível pela árvore, mas possível pela lista do Job se um PID for reaproveitado) aparece só no primeiro painel em que é encontrado. O membro que veio da lista do Job e cujo pai já morreu (a cadeia quebrada que o ADR-0033 §3 descreve) tem `parent` fora da árvore. A UI o pendura na raiz do painel ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §5).

### 6. A resposta na UI: dado chaveado, aplicado por janela

`App::user_event` encaminha `Wakeup::ProcessSample` para `App::apply_process_sample`, que:

- descarta a resposta se `seq` não for o do último pedido enviado;
- separa os `panes` por `WindowId`, e cada janela que **ainda existe e ainda tem o gerenciador aberto** recebe a parte dela no `TaskManagerHistory`/`TaskManagerView` ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §4). Uma chave de janela que fechou é ignorada;
- **numa janela, uma chave cuja aba não existe mais ou mudou de janela é ignorada.** Nenhum `TabId` velho é procurado nem corrigido: o próximo pedido já sai com as chaves novas (RF-24.25). Por isso a transferência do [ADR-0090](0090-transferencia-de-abas-entre-janelas.md) não precisa re-chavear nada da medição. O estado que o gerenciador guarda por `TabId` é só o último retrato da lista, e `App::transfer` o limpa nas duas janelas ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §4), para não deixar órfãos;
- soma os totais da janela (CPU, memória, disco) e acrescenta um ponto ao histórico com o `taken_at` da amostra.

### 7. Encerrar um processo

```rust
pub struct KillRequest {
    pub group: ProcessGroup,
    pub pid: u32,
    pub start_time: u64,
}
```

A thread de amostragem trata o pedido assim, sem resposta (RF-24.23):

1. atualiza o `System` **só** para a lista de processos (sem CPU nem disco);
2. **confere a identidade**: o PID ainda existe, o `start_time` é o mesmo da linha que o usuário escolheu, e o PID está em `group.members(&system)`. Se qualquer um falhar, nada acontece. É isso que impede matar um processo **de outro programa** que herdou o PID entre a amostra e a confirmação;
3. **recusa a raiz**: `pid == group.root_pid()` não é encerrado, mesmo que a UI peça. É uma defesa em profundidade do RF-24.22, além do item esmaecido;
4. mata os **descendentes primeiro**, das folhas para cima (`descendants_of(pid)`, em ordem reversa), e então o próprio processo, com `Process::kill()` do `sysinfo` (`TerminateProcess` no Windows, `SIGKILL` fora dele). Matar de baixo para cima evita que um filho órfão seja re-parentado no meio da operação e escape da varredura.

O pedido de encerrar sai do diálogo de confirmação, que só existe com o gerenciador aberto, então a thread sempre está viva. Se, por corrida, o `SamplerHandle` já tiver caído quando o usuário confirma, o `App` sobe uma thread de uso único com um `System` novo para cumprir o mesmo pedido. A main thread nunca enumera processos.

### 8. Onde o código mora

| Crate | O que muda |
|---|---|
| `porecatu-pty` | `stats.rs` (novo): `Sampler` (o laço da thread), `SamplerRequest`, `SampleRequest`, `SampleResult`, `ProcessStat`, `KillRequest`. Em `job.rs`: `descendants_of` nas três plataformas, `members(&System)`, `root_pid()` público |
| `porecatu-term` | re-exporta os tipos de `stats.rs`; `Inspect::process_group`, `Terminal::process_group` |
| `porecatu-ui` | `SamplerHandle`, o prazo em `schedule_next_wake`/`tick_all`, `Wakeup::ProcessSample`, `apply_process_sample`, e o encerramento a partir do diálogo |
| `porecatu-core`, `porecatu-config`, `porecatu-session`, `porecatu-render`, `porecatu-ssh`, `porecatu-locale` | nada por causa deste ADR |

O PR de implementação traz estes testes:

- `members` com `System` falso não é possível (o `sysinfo` não tem fake), então o teste de integração no Windows sobe um shell com um filho e um neto, como os do ADR-0033, e confere que `members` devolve os três e que a amostra traz os três com `memory > 0`;
- a normalização da CPU e a divisão do disco pelo tempo real ficam em funções puras, com testes;
- `KillRequest` com `start_time` diferente não mata, `KillRequest` com o PID raiz não mata, e o filho é morto com o neto (integração no Windows);
- `apply_process_sample` ignora `seq` antigo, janela fechada e aba que mudou de janela (puro, na UI);
- o teste de tamanho do `Wakeup` cobre `ProcessSample`.

## Alternativas consideradas

### Crate novo (`porecatu-monitor`)

Seria mais limpo no nome. Recusada: o crate novo precisaria do Job, que mora em `porecatu-pty`. Ou o Job seria exposto (vazando `win32job`), ou o crate novo dependeria de `porecatu-pty` só para chamar `members`, o que é um crate para uma função. O critério de pertença é o que importa, e ele já mora em `porecatu-pty`.

### Medir na main thread

Uma enumeração de processos custa alguns milissegundos no Windows, e às vezes mais. Recusada pelo ADR-0007 e pela métrica do PRD-024: a main thread nunca varre processos.

### Thread que dorme 1 s em laço

Mais simples que o pedido por canal. Recusada pelo CLAUDE.md ("temporizador de UI é sempre `ControlFlow::WaitUntil`, nunca thread"). Ela também mediria às cegas: sem o pedido, a thread não sabe quais raízes existem agora. Teria de ler o estado da UI por um `Mutex` compartilhado, e esse estado é da main thread.

### Uma thread por pedido, como no Git

Recusada no §4: a CPU é diferença entre duas atualizações do mesmo `System`, e por isso o `System` tem de viver entre as amostras.

### Medir sempre, desde o arranque

Os gráficos abririam cheios. Recusada pelo dono do produto no PRD-024: custaria uma varredura por segundo durante toda a execução, para quem talvez nunca abra o gerenciador.

### Matar só pelo PID, sem conferir o início

Mais simples. Recusada no §7: entre a amostra e o clique em "Encerrar" há um diálogo, então segundos, às vezes minutos. Um PID reaproveitado nesse intervalo faria o app matar um processo que não é dele, e o `start_time` custa um campo.

### `ProcessGroup::kill_tree` para o processo escolhido

Recusada: `kill_tree` consome o grupo e mata a partir da **raiz**, fechando o Job. É o fechamento da aba, não o encerramento de um filho.

## Consequências

### Positivas

- **Um critério só** para "processo desta aba": o fechamento, a contagem e o gerenciador passam por `members`.
- **Custo zero com o gerenciador fechado**: nenhuma thread e nenhum `System` existem.
- **Uma varredura serve a todas as janelas**: abrir o gerenciador em três janelas custa o mesmo que em uma.
- Nenhuma dependência nova: o `sysinfo` já estava lá, e as features novas (`with_cpu`, `with_memory`, `with_disk_usage`) são da mesma feature `system`.
- `descendants_of` nas três plataformas adianta parte da dívida Unix do ADR-0033, sem mudar o fechamento de aba.

### Negativas

- **Uma varredura por segundo** com o gerenciador aberto. É o preço do recurso, limitado pelo pedido não acumular (§4).
- `porecatu-pty` deixa de ser "só PTY": ganha um módulo de medição. O nome do crate fica um pouco mais estreito que o conteúdo, e o comentário do módulo registra o porquê.
- O ADR-0033 §3 fica parcialmente revisto: a varredura deixa de ser "nunca contínua".
- Fora do Windows, sem Job, um processo com a cadeia quebrada não aparece. É o mesmo ponto cego que o ADR-0033 já registra (RF-24.30).

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| A varredura a 1 Hz pesar numa máquina com milhares de processos | Baixa | Médio | Ela roda numa thread; o pedido não acumula (§4); o PR de implementação mede o tempo de uma amostra no Windows e o registra em `descobertas.md` |
| Matar um processo alheio por PID reaproveitado | Baixa | **Alto** | `start_time` e pertença conferidos na hora (§7) |
| Encerrar o shell raiz por um caminho que pule o item esmaecido | Baixa | Médio | Recusa também no `porecatu-pty` (§7, passo 3), com teste |
| Resposta atrasada aplicada a uma aba que mudou de janela | Média | Baixo | `seq` e checagem da aba na janela (§6); o próximo pedido corrige |
| A thread não terminar ao fechar o último gerenciador | Baixa | Baixo | O fim é o `recv` falhar quando o `Sender` cai; teste de que o `SamplerHandle` derrubado termina a thread (com `join` no teste) |
| `Wakeup` crescer além do orçamento | Baixa | Baixo | `Box` na variante; o teste de tamanho que já existe cobre |
