# ADR-0076 — Reconexão automática: keepalive de 15 s, cinco tentativas em 1-2-4-8-16 s decididas por estado puro em `porecatu-ui`, uma reconexão por conexão

**Status:** Aceito
**Data:** 2026-10-27
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0022](0022-animacao-de-interface.md), [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0071](0071-backend-de-terminal.md), [ADR-0072](0072-cliente-ssh-embutido.md), [ADR-0073](0073-credenciais-no-cofre-do-sistema.md), [ADR-0074](0074-verificacao-da-chave-do-host.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md), [PRD-020](../prd/prd-020-conexoes-ssh.md)

## Contexto

O [PRD-020](../prd/prd-020-conexoes-ssh.md) pede reconexão automática quando a conexão cai sem o servidor encerrar a sessão (RF-20.27), até **5 tentativas com intervalo crescente** — decisão do dono —, com o scrollback preservado e uma nota por tentativa (RF-20.28), sem retomar o processo remoto (RF-20.29), parando em cancelamento de senha e em chave de host alterada (RF-20.30), e com reconexão manual depois (RF-20.31).

As peças já estão decididas:

- o fim de um backend chega como `TermEvent::Exit(EndReason)`, e `Lost` é distinto de `RemoteEnded` e de `Failed` ([ADR-0071](0071-backend-de-terminal.md) §3);
- `Terminal::reattach` liga um backend novo ao mesmo motor, com reset suave dos modos ([ADR-0071](0071-backend-de-terminal.md) §4);
- dividir um painel SSH abre canal na mesma conexão ([ADR-0072](0072-cliente-ssh-embutido.md) §5).

O que falta: **como se sabe que caiu**, **quem conta e espera**, e **o que acontece com os painéis que dividem uma conexão**. E uma regra do projeto que restringe a resposta: estado com temporizador recebe `Instant` de fora, e temporizador de UI é `ControlFlow::WaitUntil`, nunca thread nem loop de render (CLAUDE.md).

## Decisão

**A queda é detectada pelo keepalive do `russh` (15 s, 3 sem resposta) ou por erro de transporte. A política de tentativas é uma máquina de estados pura em `porecatu-ui`, por conexão, com os intervalos 1, 2, 4, 8 e 16 s, acordada por `WaitUntil`. Uma tentativa é um `reattach` do primeiro painel da conexão; os demais painéis dela abrem canal na conexão nova quando ela autentica.**

### 1. Detectar a queda

No `client::Config` do `russh`:

| Parâmetro | Valor | Efeito |
|---|---|---|
| `keepalive_interval` | 15 s | um `keepalive@openssh.com` a cada 15 s sem tráfego |
| `keepalive_max` | 3 | três sem resposta → conexão dada como morta |
| `inactivity_timeout` | nenhum | sessão ociosa por horas é uso normal |

Com isso, uma rede que some é percebida em **45 a 60 s**; um erro de transporte (conexão recusada, reset, rota perdida ao acordar a máquina) é percebido **na hora**. Os valores são os que o OpenSSH recomenda para `ServerAliveInterval`/`ServerAliveCountMax` em rede instável, e **não são configuráveis** nesta leva — nenhum requisito pede, e uma chave a mais é um valor a mais para o usuário errar.

A classificação, em `porecatu-ssh`:

| Acontecimento | `EndReason` |
|---|---|
| `exit-status` ou `exit-signal` no canal, depois o fechamento | `RemoteEnded` — **não reconecta** (RF-20.26) |
| Canal fechado pelo servidor sem `exit-*` | `RemoteEnded { code: None, signal: None }` — não reconecta: o servidor fechou de propósito |
| Keepalive esgotado, erro de transporte, `SSH_MSG_DISCONNECT` do servidor | `Lost` — reconecta |
| Falha antes de o canal abrir | `Failed` — **fora** de um ciclo, não inicia ciclo: a aba nunca conectou, e não há sessão a recuperar. **Dentro** de um ciclo, é o resultado de uma tentativa, e quem decide se há outra é a tabela do §2 |

`SSH_MSG_DISCONNECT` é `Lost` e não `RemoteEnded` porque é o que um servidor manda ao reiniciar — exatamente o caso que o usuário quer ver voltar sozinho.

### 2. A política é estado puro em `porecatu-ui`

```rust
// crates/porecatu-ui/src/reconnect.rs
pub const ATTEMPT_DELAYS: [Duration; 5] = [
    Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(4),
    Duration::from_secs(8), Duration::from_secs(16),
];

pub enum Reconnect {
    Waiting { attempt: u8, due: Instant },   // attempt em 1..=5
    Connecting { attempt: u8 },
    GaveUp,
}

impl Reconnect {
    pub fn on_lost(now: Instant) -> Self;                       // Waiting { 1, now + 1 s }
    pub fn next_wakeup(&self) -> Option<Instant>;               // o due de Waiting
    pub fn poll(&mut self, now: Instant) -> Option<u8>;         // Some(n) quando é hora de tentar
    pub fn on_attempt_failed(&mut self, cause: &FailCause, now: Instant);
    pub fn on_connected(&mut self);                             // sai do ciclo
}
```

- **Os intervalos**: 1, 2, 4, 8, 16 s antes das tentativas 1 a 5 — 31 s de espera no total, somados ao tempo das próprias tentativas. Exponencial de base 2, sem *jitter*: o *jitter* existe para muitos clientes não martelarem um servidor ao mesmo tempo, e aqui é um usuário, com poucas conexões.
- **`now` vem de fora**, como em todo estado com temporizador do projeto, e os testes andam o relógio sem dormir.
- O `App` junta o `next_wakeup` de todos os ciclos ativos no `ControlFlow::WaitUntil` que já calcula para o cursor e as animações ([ADR-0022](0022-animacao-de-interface.md)). Nenhuma thread, nenhum frame extra enquanto espera: a contagem regressiva **não** é animada; a nota diz "em 2 s" e não se reescreve a cada segundo.

`on_attempt_failed` decide pela variante de `FailCause` ([ADR-0071](0071-backend-de-terminal.md) §3), e a tabela cobre todas:

| Causa | Efeito |
|---|---|
| `Refused`, `Timeout`, e `Lost` durante a tentativa | próxima tentativa, ou `GaveUp` depois da quinta — é o servidor reiniciando ou a rede voltando |
| `ChannelRefused` | próxima tentativa: servidor que acabou de subir pode recusar a sessão por instantes |
| `AuthCancelled`, `HostKeyRejectedByUser` | `GaveUp` imediato (RF-20.30) |
| `HostKeyChanged`, `HostKeyRevoked`, `KnownHostsUnreadable` ([ADR-0074](0074-verificacao-da-chave-do-host.md)) | `GaveUp` imediato, com a nota de bloqueio (RF-20.30) |
| `AuthRejected`, `AuthUnsupported`, `KeyUnreadable` | `GaveUp` imediato — tentar de novo daria a mesma resposta |
| `Runtime` | `GaveUp` imediato |

Uma tentativa bem-sucedida **zera** o ciclo: a próxima queda recomeça em 1 de 5.

### 3. Por conexão, não por painel

O ciclo é guardado por **`ConnectionId`** ([ADR-0072](0072-cliente-ssh-embutido.md) §5), num mapa do `App` — o mesmo desenho do `git_remotes` do [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md): estado do processo chaveado pelo que ele descreve, nunca pela janela.

Quando uma conexão cai, **todos** os painéis dela recebem `Lost`. Então:

1. o **primeiro** painel da conexão (o mais antigo) é o que tenta: `Terminal::reattach` com `SshSpawn` de conexão nova;
2. os **demais** ficam com a nota de espera e, quando a conexão nova **autentica**, fazem `reattach` com canal nessa conexão — sem autenticar de novo, sem diálogo;
3. se o primeiro painel é fechado durante o ciclo, o próximo assume;
4. `GaveUp` vale para todos os painéis da conexão de uma vez.

Assim um servidor com três painéis abertos pede a senha **uma vez** na reconexão, e não três.

### 4. O que acontece na aba

- Cada tentativa escreve a nota "conexão perdida; reconectando (tentativa N de 5) em X s" **abaixo do conteúdo** (RF-20.28) — o motor é o mesmo, então o scrollback está lá por construção ([ADR-0071](0071-backend-de-terminal.md) §4).
- **Teclas digitadas durante o ciclo são descartadas**, não enfileiradas: o canal de escrita do backend morto não tem destino, e uma fila mandaria para o shell novo o que o usuário digitou para o velho — com o risco real de um comando, ou uma senha, cair num prompt que ele não viu.
- A primeira reconexão bem-sucedida de cada ciclo escreve a nota de que o processo remoto anterior não foi retomado (RF-20.29).
- `GaveUp` deixa o painel `Exited`, com a nota da última falha; a aba segue as regras do [ADR-0017](0017-ciclo-de-vida-da-aba.md) para `Exited` com código ≠ 0 — fica aberta.
- Durante o ciclo, a conexão conta como **aberta** para o RF-20.25 (`Liveness::OpenConnection`, [ADR-0071](0071-backend-de-terminal.md) §6): fechar a aba pergunta.

### 5. Reconectar à mão: `ssh.reconnect`

| Ação | O que faz | Default |
|---|---|---|
| `ssh.reconnect` | No painel focado, se ele é SSH e está `Exited`, `GaveUp` ou esperando a próxima tentativa: reconecta agora, recomeçando a contagem (RF-20.31). Em qualquer outro painel, não faz nada | **nenhum** |

Também é o item "Reconectar" do menu da aba ([ADR-0075](0075-superficies-de-ui-do-ssh.md) §8). Num painel em `Waiting`, antecipa a tentativa em vez de esperar o intervalo.

Ação sem argumento, vinculável ([ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md)); entra no catálogo numa seção `ssh.*`.

### 6. Suspensão da máquina

Nada específico. Ao acordar, o keepalive ou o transporte acusa a queda em segundos, e o ciclo começa. Um ciclo que já estava em andamento ao suspender depende do `Instant`, que **pode ou não** contar o tempo suspenso conforme a plataforma (documentação do `std`). Os dois resultados são aceitáveis e nenhum é tratado: ou a espera continua de onde parou, ou a tentativa pendente vence logo ao acordar. Como cada `due` é de **uma** tentativa, nunca há uma rajada de tentativas acumuladas.

> **Como ficou (prompt 14).** Três precisões da implementação:
>
> - **A conexão compartilhada mora em `porecatu-ssh`**: um registro de conexões por `ConnectionId` (referência fraca; quem a mantém de pé são os canais, e o último a sair a fecha). `Connection::Share(id)` num `SshSpawn` abre um canal **sem autenticar**; conexão morta (fechada, ou com a queda já anotada) devolve `OpenError::ConnectionGone`, e `porecatu_term::connection_is_alive(id)` deixa quem divide escolher entre dividir e abrir uma própria.
> - **O sinal de que a tentativa deu certo** é `TermEvent::Connected`, mandado **uma vez** pela thread de observação do `Terminal` quando o backend remoto abre o shell (`Backend::is_established`). Sem ele a UI só saberia pelo primeiro byte da saída, que um shell silencioso pode nunca mandar. `Terminal::connection_id()` diz a conexão do painel e muda a cada `reattach`.
> - **Teclas durante o ciclo.** Com o backend velho já encerrado, escrever nele falha e não enfileira; para a tentativa em andamento, o `SshSpawn` ganhou `discard_input_until_open`, que joga fora o que foi digitado **antes** de o shell novo abrir. O estado do painel em ciclo é `PaneRuntime::reconnecting`, que faz `liveness()` valer `OpenConnection`. `Pane::resume()` é a única volta de `Exited` (só `ssh.reconnect` a usa).
>
> A máquina pura ganhou `Connected` (fora do ciclo), `reconnect_now` e o `Cycle<K>` por conexão (o mais antigo tenta, os demais seguem, e quem sai passa a vez); a tabela do §2 é um `match` **exaustivo** sobre `FailCause`. Um painel que fecha durante o ciclo sai dele na volta seguinte do laço de eventos (`prune_reconnects`), e a tentativa em andamento é refeita já pelo próximo.

## Alternativas consideradas

### Política dentro de `porecatu-ssh`

A tarefa SSH reconectaria sozinha, sem a UI saber. Recusada: a reconexão pede diálogo (senha não salva), mexe no motor (`reattach`) e escreve notas no idioma da interface — três coisas que são de `porecatu-ui`. E um temporizador numa tarefa `tokio` é exatamente o "temporizador fora do `WaitUntil`" que o projeto evita.

### Intervalo fixo

Cinco tentativas a cada 5 s. Recusada pelo dono, que pediu intervalo crescente: a primeira tentativa rápida pega a queda de um segundo; as últimas, mais espaçadas, dão tempo a um servidor que está reiniciando.

### Tentar para sempre

Útil para quem deixa um `journalctl -f` aberto o dia todo. Recusada pelo dono (5 tentativas). `ssh.reconnect` cobre a volta depois.

### Reconexão por painel

Mais simples. Recusada: três painéis da mesma conexão abririam três conexões e três diálogos de senha — e o RF-20.23 existe justamente para que dividir não multiplique autenticação.

### Enfileirar o que o usuário digita durante a queda

O comando digitado não se perde. Recusada pelo risco descrito no §4.

### Keepalive configurável

Recusada nesta leva: nenhum requisito pede, e os valores são os de uso comum. Revisitável se aparecer servidor que derruba conexão ociosa em menos de 15 s.

## Consequências

### Positivas

- Queda curta de rede volta sozinha, no mesmo scrollback, com a senha pedida no máximo uma vez por conexão.
- Nenhuma thread nem frame a mais durante a espera.
- A política é testável sem rede nem relógio real.

### Negativas

- Até 60 s para perceber uma rede que sumiu sem erro de transporte.
- O que o usuário digitou durante a queda se perde, e ele não é avisado tecla a tecla — só pela nota de que a conexão caiu.
- O mapa de ciclos por conexão é mais um estado de processo para manter coerente com o fechamento de painéis.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| `exit` legítimo ser classificado como queda e reabrir a sessão | Baixa | Médio | §1: `exit-status`/`exit-signal` e fechamento de canal pelo servidor nunca são `Lost`; teste com servidor de prova que fecha das três formas |
| Painéis de uma conexão ficarem órfãos se o primeiro fecha no meio | Média | Médio | §3 item 3; teste que fecha o painel que tenta durante `Connecting` |
| Ciclo continuar depois de a aba fechar | Baixa | Baixo | Fechar o último painel de uma conexão remove a entrada do mapa; teste |
| Rajada de diálogos de senha em várias conexões que caem juntas (Wi-Fi) | Média | Baixo | Uma pergunta por conexão, e só no painel em foco ([ADR-0075](0075-superficies-de-ui-do-ssh.md) §4); as outras esperam a visita |
| Keepalive de 15 s ser barrado por servidor que limita mensagens | Baixa | Baixo | É o mesmo `keepalive@openssh.com` do OpenSSH; servidor que o recusa responde com falha, que conta como resposta |
