# ADR-0033 — Encerramento robusto de árvore de processo (Windows)

**Status:** Aceito
**Data:** 2026-09-03
**Relacionados:** [ADR-0004](0004-pty-cross-platform.md), [ADR-0007](0007-modelo-de-threading.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md)

## Contexto

Usuário relatou: fechar uma aba com um processo de longa duração ativo (ex.
`node server.js`, `mvn spring-boot:run`) deixa esse processo vivo depois de
a aba fechar. Investigado e confirmado: `PtyHandle::kill()`
(`crates/porecatu-pty/src/spawn.rs`) delega a `portable_pty::Child::kill()`,
que no Windows chama `TerminateProcess` só no processo raiz — o shell.
Qualquer processo que o shell tenha spawnado (o servidor de longa duração,
rodando em primeiro plano) não é tocado, e sobrevive ao fechamento da aba.

O ciclo de vida da aba (ADR-0017) já havia enfrentado uma pergunta
parecida — "que processo está em primeiro plano" — e rejeitado varrer a
árvore de processos, citando o [ADR-0005](0005-persistencia-de-sessao.md) e
o [ADR-0008](0008-teclas-e-roteamento-de-input.md): *"Varrer descendentes é
caro, ambíguo e específico de plataforma."* Essa rejeição continua correta
para o problema que ela resolvia — decidir, no momento do fechamento, qual
processo é "o de primeiro plano" para fins de precedência de título ou
confirmação. Este ADR é sobre um problema diferente: **garantir que nenhum
descendente sobreviva ao fechamento pedido pelo usuário**, o que não exige
saber quem são os descendentes, só poder matá-los todos de uma vez.

### A primeira tentativa (só Job Object) não bastou

A decisão original deste ADR era só a seção 1 abaixo: um Job Object com
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, sem varredura nenhuma — o kernel já
mantém a lista de membros do Job, então fechar o handle mataria tudo de
graça. Testado com `cmd.exe`: funciona exatamente assim.

**Verificação com um `npm start` real, feita pelo usuário depois da
primeira entrega, encontrou o mecanismo incompleto.** Reproduzido: um
shell interativo rodando `npm start` (Node 20, projeto React) num diretório
com `package.json` real, atribuído ao Job no spawn. Resultado — contagem de
membros do Job nunca passou de 1 (só o shell), do início ao fim da
execução do servidor. Subindo a cadeia de processos via
`Get-CimInstance Win32_Process` a partir do `node.exe` do servidor: `node
server.js` → `cmd.exe /d /s /c node server.js` → `node npm-cli.js start` →
`cmd.exe /c npm.cmd start` → **`pwsh.exe`** (o shell raiz, corretamente no
Job) — toda a cadeia de `ParentProcessId` íntegra, nenhum reparenting,
nenhum processo órfão. Só que **nenhum dos quatro descendentes aparecia na
lista de membros do Job**, apesar da árvore de processos do SO estar
inteiramente correta.

Testado em três shells para isolar a variável: `cmd.exe` e Windows
PowerShell 5.1 propagam a associação ao Job para os filhos normalmente (o
comportamento documentado da Microsoft); **PowerShell 7 (`pwsh`) não
propaga** — nenhum filho, neto ou bisneto de um processo spawnado por pwsh
jamais aparece na lista do Job, mesmo continuando descendente legítimo na
árvore do SO. `pwsh` é justamente o shell que `resolve_default_shell`
prefere quando instalado (`crates/porecatu-pty/src/shell.rs`) — ou seja, o
Job sozinho falhava silenciosamente no caso mais comum, não num caso de
borda. A causa exata a nível de syscall não foi fechada (exigiria
rastreamento de API tipo Process Monitor); o fato empírico, reproduzido
duas vezes com métodos diferentes, é que o Job sozinho sub-conta e
sub-mata sob esse shell.

## Decisão

**Duas técnicas, combinadas — nenhuma substitui a outra:**

1. Um **Job Object** com `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: cobre o
   caminho rápido e cobre um caso que a segunda técnica sozinha não
   alcança (processo que sobrevive ao próprio pai já morto, ex. `start
   /b`) — ver seção 2.
2. Uma **varredura pontual da árvore de processos do SO** (`sysinfo`), a
   partir do PID do processo raiz, disparada só no fechamento e na
   consulta de contagem (nunca continuamente) — cobre o que o Job sozinho
   não vê (pwsh) — ver seção 3.

### 1. Job Object — onde entra em código

Módulo novo `crates/porecatu-pty/src/job.rs`, `#[cfg(windows)]` no corpo
funcional (stub fora do Windows). Tipo público `ProcessGroup` — nome
deliberadamente não-Windows, para não vazar o conceito de Job Object para
`porecatu-term`/`porecatu-ui`:

- `ProcessGroup::for_child(child: &dyn portable_pty::Child) -> Option<Self>`
  — falha só se `child.process_id()` não devolver nada (sem PID, nem Job
  nem varredura têm o que rastrear). Com PID em mãos, tenta criar o Job
  (`ExtendedLimitInfo::new().limit_kill_on_job_close()`, crate
  [`win32job`](https://crates.io/crates/win32job), MIT OR Apache-2.0,
  compatível com a GPLv3 do [ADR-0010](0010-licenciamento.md)) e atribuir
  via `child.as_raw_handle()` + `Job::assign_process`; qualquer falha
  nesses dois passos loga um aviso (`eprintln!`, mesmo padrão que
  `porecatu-ui` já usa — não há `log`/`tracing` no workspace) e degrada
  para `job: None` **sem impedir a criação do `ProcessGroup`** — a
  varredura da seção 3 não depende do Job para funcionar.
- `porecatu_pty::spawn` passa a devolver `(PtyHandle, Option<ProcessGroup>)`
  em vez de só `PtyHandle`.

`win32job` foi escolhido, em vez de chamar `windows-sys`/`winapi`
diretamente, porque encapsula o `unsafe` **dentro dele** — o workspace tem
`unsafe_code = "deny"` (`Cargo.toml` raiz) e, até esta entrega, nenhuma
linha de `unsafe` existia em código do projeto.

### 2. O RAII não pode morar dentro de `PtyHandle`

`PtyHandle` é **vazado de propósito** na saída natural do PTY (`leak_pty`,
`crates/porecatu-term/src/terminal.rs`) para não repetir o deadlock do
`ClosePseudoConsole` (ver a armadilha já registrada no CLAUDE.md). Se o
Job estivesse guardado dentro do `PtyHandle`, o mesmo `mem::forget` que
evita o deadlock também esqueceria o Job — e o `KILL_ON_JOB_CLOSE` nunca
dispararia. Seria o mesmo bug, só reimplementado com uma peça a mais.

Por isso `ProcessGroup` viaja **separado** do `PtyHandle`, como um struct
`Clone` (o Job por dentro é um `Arc`, `root_pid` é `Copy`) em dois
lugares: uma cópia fica em `Terminal` (thread da UI, nunca se move, usada
só para consulta — ver [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md));
a outra se move para dentro de `watch_loop`, ao lado do `PtyHandle`, e é
essa cópia que decide o destino da árvore, por caminho de saída:

| Caminho de saída de `watch_loop` | O que acontece com a cópia de `watch_loop` | Resultado |
|---|---|---|
| **Fechamento pedido pelo usuário** (`Terminal::close()` → `_shutdown` desconectado) | `ProcessGroup::kill_tree(self)` | Fecha o handle do Job (mata quem propagou — inclusive quem sobreviveu ao próprio pai intermediário já morto) **e** varre os descendentes vivos de `root_pid` por `sysinfo`, matando quem a varredura encontrar (cobre pwsh) |
| **Saída natural do shell** (`try_wait` detecta, ex. usuário digitou `exit`) | `std::mem::forget(process_group)` | Nenhuma das duas técnicas roda. A referência ao Job nunca decrementa (a cópia da UI pode dropar depois sem efeito) e a varredura não é chamada — um processo que o shell tenha deliberadamente destacado (`start /b algo & exit`) sobrevive, como hoje |

`pty.kill()` (o `TerminateProcess` de sempre, no processo raiz) continua
sendo chamado no primeiro caminho, como rede de segurança para o caso
`ProcessGroup` ser `None`.

### 3. A varredura complementar (`sysinfo`)

`ProcessGroup::process_count()`/`kill_tree()` fazem a **união** de duas
fontes de PID — nenhuma sozinha basta:

- **Lista do Job** (`query_process_id_list`): inclui todo processo que
  alguma vez propagou a associação, **mesmo que o pai intermediário já
  tenha morrido** — membresia de Job é permanente, não depende do pai
  continuar vivo. É o que cobre `start /b algo` sobrevivendo sozinho
  depois do `cmd.exe` que o lançou sair. Não vê nada que nunca entrou no
  Job (pwsh).
- **Varredura de processo vivo** (`sysinfo::System`, função `descendants_of`
  em `job.rs`): acha qualquer descendente cuja cadeia de ancestrais até
  `root_pid` esteja **inteira viva agora**. É o que cobre pwsh — mas não
  alcança um descendente cujo pai direto já morreu (o link quebra: um
  processo morto não aparece em `system.processes()` para ligar o neto ao
  avô).

Juntas cobrem os dois casos reais observados. A varredura roda **uma vez
por chamada** (`refreshed_system()` monta um snapshot novo a cada
`process_count()`/`kill_tree()`), disparada só pelo fechamento pedido pelo
usuário ou pela consulta de contagem do [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md)
— nunca continuamente, e nunca para decidir "qual processo é o de
primeiro plano" (isso continua sendo `TermModes`, ADR-0017 seção 3). Alvo
(`root_pid`) e gatilho já são conhecidos sem ambiguidade: **isto não é a
varredura que o ADR-0005/ADR-0008/ADR-0017 rejeitaram** para fins de
*detecção* de processo em primeiro plano — é limpeza e contagem
determinísticas, disparadas por evento, sobre uma raiz já conhecida.

Dependência nova: `sysinfo` (crate, licença MIT, feature mínima `system`,
sem os padrões de rede/disco/usuário que o crate também oferece),
`[target."cfg(windows)".dependencies]` — mesmo lugar do `win32job`.

## Alternativas consideradas

### Só o Job Object, sem varredura complementar

A decisão original deste ADR. Descartada depois da verificação com `npm
start`: sozinho, o Job deixa passar qualquer processo spawnado por um
shell que não propaga a associação (pwsh) — o shell mais comum quando
instalado. Manter só o Job corrigiria o problema pela metade.

### Varredura no lugar do Job (sem Job nenhum)

Já que a varredura por `sysinfo` funciona sozinha para o caso comum (cadeia
inteira viva), por que manter o Job? Descartada: a varredura tem seu
próprio ponto cego (cadeia com um elo morto, ex. `start /b` cujo lançador
já saiu) que só o Job cobre, por a associação ser permanente e
independente do pai continuar vivo. Um dos dois sozinho erra por um lado
ou pelo outro; juntos, cobrem os dois casos reproduzidos nesta
investigação.

### Persistir por PID em vez de handle (para o Job)

Guardar o PID do shell e reabrir com `OpenProcess` só na hora de matar.
Rejeitada: `as_raw_handle()` já dá o handle de graça no momento do spawn,
sem esse custo extra, e sem o risco (por menor que seja) de reuso de PID
entre o spawn e o kill.

### `CREATE_SUSPENDED` + atribuir ao Job antes do primeiro `ResumeThread`

Fecharia de vez a janela de corrida descrita nos riscos abaixo (processo
que o shell spawna antes de o Porecatu conseguir atribuí-lo ao Job).
Rejeitada por enquanto: `portable-pty` não expõe controle sobre as flags de
criação do processo no caminho ConPTY — fechar isso exigiria fork do
crate, o que o CLAUDE.md já trata como problema à parte para o
`alacritty_terminal` ("mantenha o uso isolado"), mesma lógica aplicada
aqui.

## Consequências

### Positivas

- Fecha o bug relatado, para os dois shells reproduzidos (`cmd.exe`
  diretamente e `pwsh` através de um `npm start` real) — validado
  empiricamente por testes de integração que sobem shell interativo real,
  respondem ao handshake do ConPTY, rodam um comando de longa duração em
  primeiro plano e confirmam a morte de tudo após `kill_tree`.
- A mesma peça (`ProcessGroup`) alimenta o [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md)
  sem duplicar mecanismo.
- A varredura por `sysinfo` é a mesma técnica nas três plataformas — se a
  dívida Unix (seção de riscos) for paga um dia, a maior parte do
  mecanismo de contagem/kill já está pronta, faltando só o Job (que é
  puramente aditivo no Windows).

### Negativas

- Duas dependências novas de Win32/sistema no workspace (`win32job` e
  `sysinfo`, ambas `[target."cfg(windows)".dependencies]`), onde antes só
  chegava via `portable-pty`/`winres`, incidentais.
- `porecatu_pty::spawn` muda de assinatura (`PtyHandle` → `(PtyHandle,
  Option<ProcessGroup>)`) — afeta todo chamador, incluindo os testes de
  integração existentes.
- `process_count`/`kill_tree` custam uma enumeração de processos do SO
  inteiro por chamada (via `sysinfo`) — aceitável porque só rodam por
  clique de fechar aba ou por essa mesma consulta, nunca em loop, mas é
  trabalho a mais que uma leitura de Job puro.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Corrida entre `spawn_command` e `assign_process`: um neto nascido nesse intervalo escapa do Job | Baixíssima (shells não spawnam nada no primeiro instante de vida) | Baixo | A varredura por `sysinfo` ainda pega esse neto, contanto que a cadeia esteja viva no momento do fechamento — mitigação parcial já embutida, não dívida pura |
| `AssignProcessToJobObject` falhar em produção por motivo não previsto | Baixa | Baixo — a varredura por `sysinfo` continua funcionando mesmo com `job: None` | Degradação documentada e testada (`for_child_degrades_gracefully_on_invalid_handle`) |
| Processo naturalmente encerrado matar netos "detached" de propósito | Baixa, mas seria regressão real se acontecesse | Médio | `mem::forget` no caminho de saída natural evita as duas técnicas por igual — decisão consciente |
| Um descendente sobreviver por ter uma cadeia com elo morto **e** nunca ter entrado no Job (o ponto cego que sobra depois de unir as duas técnicas) | Baixa (exige as duas falhas simultâneas: shell que não propaga E processo cujo pai intermediário já morreu) | Médio | Não coberto nesta entrega — registrar como dívida residual, não bloqueante para o caso relatado (`npm start`/servidor em primeiro plano, cadeia inteira viva) |
| Dívida Unix: sem `setsid`/`killpg`, o mesmo bug persiste em Linux/macOS | Certa (não implementado) | Médio | `ProcessGroup::for_child` fora do Windows sempre devolve `None` — mesmo padrão de dívida assumida que outras verificações interativas do projeto já têm, sem ambiente disponível para verificar. A varredura por `sysinfo` já é multiplataforma; só falta o `for_child`/kill específico de Unix |
