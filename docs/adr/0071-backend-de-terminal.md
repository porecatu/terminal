# ADR-0071 — Backend de terminal: o PTY local passa a ser uma de duas implementações de um trait interno de `porecatu-term`, e o motor sobrevive à troca de backend

**Status:** Aceito
**Data:** 2026-10-27
**Relacionados:** [ADR-0002](0002-motor-vte.md), [ADR-0004](0004-pty-cross-platform.md), [ADR-0007](0007-modelo-de-threading.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0033](0033-job-object-encerramento-de-processo.md), [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md), [ADR-0038](0038-fallbacks-de-cwd.md), [ADR-0070](0070-perfis-de-aba.md), [ADR-0072](0072-cliente-ssh-embutido.md), [ADR-0073](0073-credenciais-no-cofre-do-sistema.md), [ADR-0076](0076-reconexao-automatica.md), [PRD-020](../prd/prd-020-conexoes-ssh.md)
**Supersedes:** ADR-0004, "Decisão" (**parcial**: o PTY local deixa de ser o único jeito de alimentar um terminal; continua o único jeito de lançar um programa local) · ADR-0034, "Decisão" (**parcial**: um terceiro sinal de confirmação, a conexão aberta, para o backend SSH)

## Contexto

O [PRD-020](../prd/prd-020-conexoes-ssh.md) pede que uma aba seja alimentada por um **canal SSH** do cliente embutido, e não por um processo local num pseudo-terminal. Para o motor VT isso não muda nada: ele consome bytes e devolve bytes. Para tudo em volta dele, muda.

Hoje `porecatu-term` é amarrado ao PTY por construção (`crates/porecatu-term/src/terminal.rs`):

- `Terminal::spawn` recebe um `porecatu_pty::SpawnConfig`, chama `porecatu_pty::spawn` e guarda um `ProcessGroup`.
- Três threads: leitura (bloqueante, aplica bytes ao motor e chama `on_wakeup`), escrita (drena um `mpsc<Vec<u8>>` — inclusive as respostas DSR/DA/CPR do motor) e observação (`watch_loop`, dona do `PtyHandle`: aplica o resize mais recente, chama `try_wait` a cada 100 ms e emite `TermEvent::Exit { success, code }`).
- O encerramento tem duas formas — pedido pelo usuário (`kill` + `kill_tree` + confirmação) e saída natural (`mem::forget` do `ProcessGroup`) — e as duas terminam em `leak_pty`, porque fechar o ConPTY com a leitura parada nele é deadlock.
- `has_extra_processes` e `cwd_fallback` consultam o `ProcessGroup`.

Quatro coisas do PRD-020 não cabem nessa forma:

1. **Outro produtor de bytes.** Um canal SSH não tem `PtyHandle`, nem `ProcessGroup`, nem processo local para `try_wait`.
2. **Outra forma de terminar.** A sessão remota termina com *exit-status* (código), com *exit-signal* (nome de sinal, sem código) ou **cai** — e cair é diferente de sair, porque cair dispara reconexão (RF-20.27) e sair não (RF-20.26).
3. **Reconexão com o mesmo scrollback** (RF-20.28). Hoje o motor nasce e morre com o PTY dentro do mesmo `Terminal`; reconectar com um `Terminal` novo perderia a grade.
4. **Um diálogo no meio do spawn.** Autenticar pode exigir senha e chave de host confirmada (RF-20.12, RF-20.16) — o backend precisa perguntar à UI e esperar a resposta, sem que `porecatu-term` conheça a UI.

E uma restrição que não muda: **`porecatu-ui` não vê tipo de terceiro**. Nenhum tipo do `portable-pty` atravessa a fronteira hoje, e nenhum tipo da biblioteca SSH vai atravessar.

## Decisão

**`porecatu-term` define um trait interno de backend, com duas implementações — `PtyBackend` sobre `porecatu-pty` e `SshBackend` sobre um crate novo `porecatu-ssh` (ADR-0072) —, e `Terminal` passa a ser motor + backend trocável. O backend escolhido entra por um enum público; a saída vira um enum de razão; o motor sobrevive a `Terminal::reattach`; perguntas do backend à UI viajam como evento, com resposta por método.**

### 1. O trait

```rust
// crates/porecatu-term/src/backend.rs — privado ao crate
pub(crate) trait Backend: Send {
    fn reader(&mut self) -> io::Result<Box<dyn Read + Send>>;
    fn writer(&mut self) -> io::Result<Box<dyn Write + Send>>;
    fn resize(&mut self, rows: u16, cols: u16) -> io::Result<()>;
    /// Não bloqueia. `None` = ainda vivo.
    fn try_wait(&mut self) -> Option<EndReason>;
    /// Encerramento pedido pelo usuário.
    fn kill(&mut self);
    /// Último ato da thread de observação, com o motivo de ter terminado.
    fn finish(self: Box<Self>, how: Finish);
    /// Sinais de consulta da UI (ADR-0034, ADR-0038).
    fn liveness(&self) -> Liveness;
    fn cwd_fallback(&self) -> Option<PathBuf>;
}

pub(crate) enum Finish { Killed, Ended }
```

O trait é **`pub(crate)`**: quem está fora de `porecatu-term` nunca o implementa nem o vê. O conjunto de backends é fechado, como o de ações, e um terceiro backend pede ADR.

`watch_loop` passa a receber `Box<dyn Backend>` no lugar de `PtyHandle` + `Option<ProcessGroup>`. O corpo é o mesmo — drenar resize, `try_wait`, emitir o fim, `finish` — e o que era específico do PTY desce para `PtyBackend`:

| Hoje em `watch_loop` | Depois |
|---|---|
| `pty.kill()` + `group.kill_tree()` | `PtyBackend::kill` |
| `leak_pty(pty)` | `PtyBackend::finish`, nos dois caminhos — o deadlock do ConPTY é do PTY, não do terminal |
| `mem::forget(process_group)` na saída natural | `PtyBackend::finish(Finish::Ended)` |
| `ProcessGroup` guardado em `Terminal` para consulta | dentro de `PtyBackend`, lido por `liveness`/`cwd_fallback` |

`SshBackend::finish` fecha o canal e solta a conexão normalmente — ali não há deadlock a contornar, e vazar uma conexão TCP seria vazar um recurso de verdade.

### 2. Escolha do backend: um enum público

```rust
pub enum BackendSpec {
    Local(SpawnConfig),        // re-exportado de porecatu-pty, como hoje
    Ssh(SshSpawn),             // re-exportado de porecatu-ssh (ADR-0072)
}

impl Terminal {
    pub fn spawn(spec: BackendSpec, params: TermParams,
                 on_wakeup: impl Fn() + Send + Sync + 'static)
        -> Result<Self, TerminalSpawnError>;
}
```

A re-exportação é o caminho que o CLAUDE.md já prescreve para `SpawnConfig`/`PtySize`/`PtyError`: `porecatu-ui` não depende de `porecatu-pty` nem de `porecatu-ssh`. `SshSpawn` é um struct **do projeto**, sem tipo da biblioteca SSH dentro.

> **Acrescentado na implementação do [PRD-020](../prd/prd-020-conexoes-ssh.md) RF-20.33 (prompt 12).** `BackendSpec` ganhou uma terceira variante, `Dormant { rows, cols }`: **só o motor**, nenhum backend (leitor vazio, escritor que descarta, nunca termina, `Liveness::Idle`). É o que o painel SSH restaurado usa para existir, mostrar a nota "`Enter` para conectar a `nome`" e esperar o primeiro gesto do usuário; o gesto chama `Terminal::reattach` com o `Ssh(..)` de verdade, e a nota que estava na grade fica (§4). Sem a variante, o painel não teria grade onde escrever a nota, e a decisão "nada conecta por acaso" ficaria sem lugar para viver.

`Terminal::spawn` de um `Ssh` **não espera a conexão**: devolve na hora, com o motor criado e o backend em estado "conectando". Conectar, autenticar e abrir o canal acontecem fora da main thread (ADR-0072); os bytes só começam a chegar depois. Enquanto isso o leitor do backend simplesmente não tem o que ler.

### 3. Fim: razão em vez de código

```rust
pub enum EndReason {
    /// Processo local saiu (PTY). Igual ao `Exit` de hoje.
    Exited { success: bool, code: u32 },
    /// O servidor encerrou a sessão: exit-status, ou exit-signal sem código.
    RemoteEnded { code: Option<u32>, signal: Option<String> },
    /// A conexão caiu sem o servidor encerrar a sessão (RF-20.27).
    Lost { cause: LostCause },
    /// Não chegou a abrir: recusa, autenticação, chave de host (RF-20.11, RF-20.17).
    Failed { cause: FailCause },
}

TermEvent::Exit(EndReason)   // substitui TermEvent::Exit { success, code }
```

`LostCause` e `FailCause` são **enums tipados**, sem prosa: a frase da nota é de `porecatu-ui`, pelo catálogo ([ADR-0056](0056-catalogo-de-textos-da-interface.md)). A lista fecha aqui, e variante nova pede revisão desta seção:

```rust
pub enum LostCause { KeepaliveTimeout, Transport, ServerDisconnect }

pub enum FailCause {
    Refused,              // TCP recusado, host inalcançável, DNS
    Timeout,              // sem troca de chaves no teto (ADR-0072 §8)
    AuthRejected,         // nenhum método aceito, sem nada a perguntar
    AuthCancelled,        // o usuário cancelou um diálogo de segredo
    AuthUnsupported,      // keyboard-interactive além de um campo oculto (RF-20.11)
    KeyUnreadable,        // arquivo da chave ausente ou ilegível
    HostKeyRejectedByUser,// o usuário cancelou o diálogo de host desconhecido
    HostKeyChanged { .. },
    HostKeyRevoked { .. },
    KnownHostsUnreadable { .. },
    ChannelRefused,       // o servidor recusou pty ou shell
    Runtime,              // não foi possível criar o runtime (ADR-0072 §3)
}
```

> **Na implementação (prompt 10).** As três de chave de host e a de `known_hosts` ilegível carregam os dados da nota, e o `{ .. }` do bloco acima é: `HostKeyChanged { presented, expected }` (a chave apresentada e as entradas divergentes, com arquivo e linha), `HostKeyRevoked { presented, entry }` e `KnownHostsUnreadable { file, cause }` (`cause` é um enum: permissão, não é texto, outra). `presented` é o algoritmo e a impressão digital; nada é frase.

Quais delas reabrem uma tentativa de reconexão é decidido por variante no [ADR-0076](0076-reconexao-automatica.md) §2. O que decide o ciclo de vida da aba ([ADR-0017](0017-ciclo-de-vida-da-aba.md)):

| Razão | Aba |
|---|---|
| `Exited { success: true }`, `RemoteEnded { code: Some(0) }` | fecha |
| `Exited { success: false }`, `RemoteEnded` com código ≠ 0 ou com sinal | fica `Exited`, com nota |
| `Lost` | entra no ciclo de reconexão (ADR-0076) |
| `Failed` | fica `Exited`, com a nota da causa |

### 4. O motor sobrevive: `Terminal::reattach`

```rust
impl Terminal {
    /// Encerra o backend atual (se ainda vivo) e liga outro ao mesmo motor.
    pub fn reattach(&mut self, spec: BackendSpec) -> Result<(), TerminalSpawnError>;
}
```

O motor (`Arc<Mutex<TermEngine>>`), a grade, o scrollback, a seleção e a busca ficam; as três threads são trocadas por três novas. As antigas terminam pelo caminho de sempre — sinal de desligamento, `finish(Killed)`.

Duas regras impedem que a troca misture bytes:

- **O canal de escrita é recriado**, e o motor recebe o `Sender` novo antes de as threads novas lerem o primeiro byte. Resposta DSR pendente da sessão antiga vai para o canal antigo e morre com ele — correto, porque quem perguntou já não existe.
- **O motor não é reiniciado.** Modos que a sessão antiga ligou (tela alternativa, reporte de mouse, *bracketed paste*) são **desligados** por `reattach`, que manda ao motor a mesma sequência de reset suave que um `reset` do shell mandaria. Sem isso, um `vim` aberto na hora da queda deixaria a aba nova presa na tela alternativa.

`reattach` não é exclusivo do SSH, mas só ele o usa hoje. Não existe "reiniciar o processo local" no produto, e este ADR não o cria.

### 5. Perguntas do backend à UI

```rust
TermEvent::Prompt(Prompt)          // novo
impl Terminal {
    pub fn answer(&self, id: PromptId, answer: Answer);
}
```

`Prompt` e `Answer` são tipos **de `porecatu-term`**, genéricos o bastante para o que o PRD-020 pede — segredo (com rótulo tipado: senha de `usuário@host`, passphrase de arquivo) e confirmação de chave de host (tipo e impressão digital) — e nada mais. **Quem espera a resposta é o próprio backend, no contexto de execução dele** — no `SshBackend`, a tarefa async, por um `oneshot` que não ocupa thread de trabalho ([ADR-0072](0072-cliente-ssh-embutido.md) §4) —, nunca a thread de leitura do motor e nunca a main thread. A UI recebe o evento no wakeup, abre o diálogo e chama `answer`. Fechar a aba com pergunta aberta responde `Answer::Cancel` implicitamente pelo `Drop`.

> **Na implementação (prompt 10).** `porecatu-ssh` não pode nomear `PromptSink`, que é de `porecatu-term` — e `porecatu-term` depende de `porecatu-ssh`. O tipo da pergunta que o backend SSH faz mora, portanto, **em `porecatu-ssh`** (`Asker`, `Question`, `Reply`), e `porecatu-term` o adapta ao `PromptSink` (`PromptAsker`). A resposta chega por *callback* (`PromptSink::ask_with`), e não por um `Receiver`, para a tarefa async esperá-la num `oneshot` sem ocupar uma thread. `PromptKind::HostKey` leva `skipped_lines` no lugar de `known_hosts_read`: um arquivo ilegível nunca chega a virar pergunta.

**Roteamento por painel.** Há um `Terminal` por painel, e `porecatu-ui` drena os eventos de cada um a partir do `Wakeup { window, tab, pane }` que o acordou. `TermEvent::Exit` e `TermEvent::Prompt` chegam, portanto, **com o painel de origem conhecido**, e a UI os aplica com as variantes `set_pane_*`/`mark_pane_*` daquele painel — nunca com as que miram o focado, a regra do CLAUDE.md para painel em segundo plano. É isso que permite à pergunta de um painel de fundo esperar nele, com a campainha acesa ([ADR-0075](0075-superficies-de-ui-do-ssh.md) §4).

O segredo atravessa a fronteira dentro de um tipo que zera a memória ao ser descartado e não implementa `Debug` com conteúdo (ADR-0073); `porecatu-term` não o guarda depois de repassar.

### 6. Sinais de consulta

```rust
pub enum Liveness {
    /// PTY: só o shell vivo. SSH: não se aplica.
    Idle,
    /// PTY: mais de um processo na árvore (ADR-0034).
    ExtraProcesses,
    /// SSH: o canal **deste painel** está aberto ou abrindo, ou o painel
    /// está num ciclo de reconexão (RF-20.25).
    OpenConnection,
}
```

`OpenConnection` é **por canal**, não por conexão: com dois painéis na mesma conexão ([ADR-0072](0072-cliente-ssh-embutido.md) §5), o painel cujo shell remoto já saiu é um painel encerrado e fecha sem perguntar (RF-20.25), mesmo com a conexão viva por causa do outro.

`Terminal::has_extra_processes` vira `Terminal::liveness`. A regra de confirmação do [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md) fica: confirma se `modes()` indica programa em tela cheia **ou** `liveness() != Idle`, atrás de `confirm_close_with_process`. Para o PTY o resultado é idêntico ao de hoje; para o SSH, uma aba parada no prompt remoto passa a confirmar — o caso que o [ADR-0017](0017-ciclo-de-vida-da-aba.md) registrou como não coberto.

`cwd_fallback` do `SshBackend` devolve sempre `None`: não há processo local para consultar, e o `cwd` de OSC 7 de uma aba SSH é remoto (RF-20.22), decisão de `porecatu-ui`.

### 7. Grafo de dependências

```
porecatu-term ──── porecatu-pty
      └─────────── porecatu-ssh     (novo, ADR-0072)
```

`porecatu-ssh` fica no mesmo andar de `porecatu-pty`: não depende de nenhum crate do projeto, e nunca de GUI, `config` ou `core`. A tabela de dependências do CLAUDE.md ganha a linha.

### 8. Ordem de implementação

A troca é feita **primeiro só com o PTY**, num PR sem comportamento observável novo: trait, `PtyBackend`, `BackendSpec::Local`, `EndReason::Exited`, `Liveness`, com os testes de `porecatu-term` e `porecatu-pty` passando com uma única mudança de asserção: a forma de `TermEvent::Exit` (hoje casada como `TermEvent::Exit { success, .. }` em `crates/porecatu-term/tests/terminal.rs`), que passa a `TermEvent::Exit(EndReason::Exited { success, .. })`. O `SshBackend` entra depois, sobre uma fronteira já verificada.

## Alternativas consideradas

### `porecatu-ui` escolhe entre dois tipos de terminal

`Terminal` para PTY, `SshTerminal` para SSH, um enum em `porecatu-ui`. Recusada: o motor, a grade, a seleção e a busca seriam duplicados ou extraídos para um terceiro tipo, e cada método público de `Terminal` (são cerca de vinte) ganharia um `match` do outro lado da fronteira.

### O SSH como um PTY: lançar o `ssh` do sistema

Zero trait novo — é o que o rascunho do PRD-007 imaginava. Recusada pelo dono do produto, por decisão de produto e não de arquitetura (PRD-020, "Decisões do dono"): sem acesso ao protocolo não há reconexão com o mesmo scrollback, nem divisão sobre a mesma conexão, nem confirmação de fechamento por conexão aberta.

### Trait público, para backends de terceiros

Abriria espaço para serial, telnet, container. Recusada: nenhum requisito pede, e um trait público é uma fronteira que não se muda sem quebrar quem implementou. `pub(crate)` pode virar `pub` depois; o contrário não.

### Reconectar criando um `Terminal` novo e copiando a grade

Sem `reattach`. Recusada: copiar a grade do `alacritty_terminal` é copiar estado interno que o motor não expõe, e a seleção, a busca e a posição de rolagem se perderiam no caminho.

### Perguntas por callback em vez de evento

O backend chama um `Fn(Prompt) -> Answer` fornecido pela UI. Recusada: o callback rodaria na thread do backend e teria de bloquear esperando a main thread — o mesmo acoplamento que `on_wakeup` evita sendo só "marque sujo e volte".

## Consequências

### Positivas

- O deadlock do ConPTY e o `leak_pty` ficam confinados ao `PtyBackend`, onde pertencem.
- A aba SSH confirma ao fechar pelo mesmo caminho da aba com servidor local rodando.
- `porecatu-ui` continua sem ver tipo de terceiro, e sem saber como um backend é construído.
- O primeiro PR é uma refatoração verificável sem nenhum comportamento novo.

### Negativas

- `TermEvent::Exit` muda de forma: todo `match` sobre ele em `porecatu-ui` muda junto.
- Um despacho dinâmico por chamada de `resize`/`try_wait` na thread de observação — irrelevante a 10 Hz.
- `reattach` é um caminho novo de troca de thread com o motor vivo, e o tipo de código em que corrida aparece.
- Duas seções de ADRs aceitos parcialmente supersedidas.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| A refatoração mudar o encerramento do PTY e reabrir o deadlock do ConPTY | Média | **Alto** | §8: PR só de PTY, testes de encerramento de `porecatu-term` sem mudança; o smoke test de fechar janela com aba viva, que achou o bug original, repetido ao vivo |
| Bytes da sessão antiga chegarem depois de `reattach` | Média | Médio | Canal de escrita recriado; a thread de leitura antiga só aplica ao motor enquanto o identificador de geração dela é o corrente — teste com um backend falso que escreve depois de ser trocado |
| Modo de tela alternativa sobreviver à queda | **Alta** sem o reset | Médio | §4: reset suave em `reattach`, com teste que liga a tela alternativa, reconecta e confere o snapshot |
| Segredo ficar numa estrutura de `porecatu-term` | Baixa | **Alto** | §5: repassado e descartado; tipo sem `Debug` com conteúdo (ADR-0073) |
| Pergunta aberta deixar o backend esperando para sempre | Média | Baixo | `Drop` do `Terminal` responde `Cancel`; e o servidor encerra a autenticação pelo próprio `LoginGraceTime`, o que chega como `Failed` ([ADR-0072](0072-cliente-ssh-embutido.md) §8). A espera é uma tarefa async, não uma thread |
