# ADR-0104 — Crate `porecatu-remote`: folha com `tokio` e `keyring` próprios, fronteira síncrona com `porecatu-ui`

**Status:** Proposto
**Data:** 2026-11-15
**Relacionados:** [PRD-025](../prd/prd-025-acesso-remoto.md), [ADR-0007](0007-modelo-de-threading.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md), [ADR-0072](0072-cliente-ssh-embutido.md), [ADR-0073](0073-credenciais-no-cofre-do-sistema.md), [ADR-0103](0103-acesso-remoto-tres-partes-e-protocolo-externo.md), [ADR-0105](0105-espelhamento-da-grade-e-input-remoto.md), [ADR-0106](0106-pareamento-identidade-e-autorizacao.md)
**Supersedes:** ADR-0072, a regra "único crate com `tokio`" (**parcial**: `tokio` passa a ser permitido em crate de rede folha, um runtime por crate); ADR-0073, o isolamento de `keyring` em `porecatu-ssh` (**parcial**: `porecatu-remote` também o usa, com módulo `vault` próprio); ADR-0007, "Runtime async" (**parcial**: a mesma exceção do ADR-0072, estendida)

## Contexto

O host mantém uma conexão WebSocket com o relay, multiplexa nela vários canais de clientes, roda um handshake Noise por canal, cifra e decifra cada registro, aplica controle de fluxo por painel assinado e se reconecta quando a rede cai. É trabalho de rede assíncrono, com temporizadores e muitas filas, e não pode tocar a main thread ([ADR-0007](0007-modelo-de-threading.md)).

Três regras do projeto encostam nisso:

- **`tokio` só em `porecatu-ssh`** ([ADR-0072](0072-cliente-ssh-embutido.md), CLAUDE.md), com `cargo tree -i tokio` mostrando um único caminho;
- **`keyring` exato e isolado em `porecatu-ssh`** ([ADR-0073](0073-credenciais-no-cofre-do-sistema.md)), no módulo `vault`;
- **dependências só para baixo**, com crates de I/O (`porecatu-pty`, `porecatu-ssh`) sem dependência de nenhum crate do projeto.

A pergunta foi feita ao dono, com três caminhos: um crate folha com runtime próprio; um runtime compartilhado com o SSH; threads síncronas sem `tokio`. Ele escolheu o primeiro.

## Decisão

**Um crate novo, `porecatu-remote`, no andar de `porecatu-ssh`, sem dependência de nenhum crate do projeto, dono de um runtime `tokio` próprio criado quando o acesso remoto liga e destruído quando desliga. A fronteira com `porecatu-ui` é síncrona: comandos por `mpsc`, eventos por um closure que `porecatu-ui` fecha sobre o `EventLoopProxy`.**

### 1. Lugar no grafo

```
porecatu (bin)
   ├── porecatu-ui ──── porecatu-render
   │        ├── porecatu-term ──── porecatu-pty
   │        │        └──────────── porecatu-ssh
   │        ├── porecatu-remote                  (novo, folha)
   │        ├── porecatu-config
   │        ├── porecatu-locale
   │        └── porecatu-session ── porecatu-core
   └── porecatu-core
```

| Crate | Pode depender de | Nunca depende de |
|---|---|---|
| `porecatu-remote` | nenhum crate do projeto; externos: `porecatu-protocol` (exato), `tokio`, `tokio-tungstenite` com `rustls`, `keyring` (exato), `ed25519-dalek` | todo crate do projeto; GUI; `core`, `config`, `term` |

`porecatu-remote` não conhece `Workspace`, `Config`, `GridSnapshot` nem `winit`. Ele fala **tipos do protocolo**, re-exportados para `porecatu-ui` (`pub use porecatu_protocol::...`), e é `porecatu-ui` quem traduz entre o domínio e o fio — o mesmo papel que já tem entre `Config` e `TermParams`.

### 2. Runtime

- O runtime é **`tokio` multi-thread com 2 workers**, criado na primeira vez que `[remote] enabled` fica `true` (no arranque ou por recarga) e **destruído** quando fica `false`. Com o recurso desligado, não existe ([RNF-25.1](../prd/prd-025-acesso-remoto.md)).
- `porecatu-ssh` mantém o runtime dele. São dois runtimes no processo quando os dois recursos estão em uso: o custo é algumas threads ociosas, e a recompensa é que nenhum dos dois crates conhece o outro.
- A regra do CLAUDE.md passa a ser: **`tokio` só em crate de rede folha (`porecatu-ssh`, `porecatu-remote`), cada um com runtime próprio, nunca em `[workspace.dependencies]`**. `cargo tree -i tokio` mostra **dois** caminhos esperados.

### 3. Fronteira com `porecatu-ui`

```rust
// Esboço da API; nomes finais na implementação.
pub struct RemoteHost { /* runtime, Sender<Command>, ... */ }

impl RemoteHost {
    pub fn start(params: RemoteParams, on_event: impl Fn(RemoteEvent) + Send + 'static) -> Self;
    pub fn send(&self, command: Command);   // não bloqueia
    pub fn stop(self);                      // derruba conexões, não espera a rede
}
```

- **`RemoteParams`** é struct do próprio crate (URL do relay, diretório de dados, nome do host), montada por `porecatu-ui` a partir de `Config` — a regra da seção 4.2 da arquitetura aplicada a mais um crate.
- **`Command`** (main thread → rede): mensagens a mandar a um cliente, quadros de grade novos ([ADR-0105](0105-espelhamento-da-grade-e-input-remoto.md)), respostas de ação, gerar QR, revogar, desconectar.
- **`RemoteEvent`** (rede → main thread): estado da conexão com o relay, cliente conectado/desconectado, pedido de pareamento esperando confirmação, mensagem de aplicação recebida (input, ação, assinatura). Chega como uma variante nova de `Wakeup` (`porecatu-ui/src/lib.rs`) e é **dado chaveado por canal**, nunca um comando de UI — a regra do resultado de thread do [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md).
- A main thread **não espera** a rede em nenhum momento: `send` é `try_send` numa fila limitada, e fila cheia descarta o comando de grade mais antigo daquele painel (o estado mais novo substitui o velho), nunca bloqueia.

### 4. Estado do processo, não da janela

O estado remoto — conexão com o relay, canais, assinaturas, aparelhos conectados — é **um por processo**, em `App`, como `App.git_remotes` e o registro de fixadas. Um cliente vê todas as janelas; uma janela fechando não derruba cliente.

### 5. Segredos

`porecatu-remote` tem módulo `vault` próprio, com `keyring` na mesma versão exata do `porecatu-ssh`, guardando:

- a chave estática X25519 e a chave de roteamento Ed25519 do host ([ADR-0106](0106-pareamento-identidade-e-autorizacao.md));
- o token do operador do relay.

Os nomes de entrada no cofre têm prefixo próprio (`porecatu-remote/…`), distinto do de `porecatu-ssh`, e nenhum dos dois crates lê entrada do outro. A regra do CLAUDE.md passa a ser: **`keyring` exato, só nos módulos `vault` de `porecatu-ssh` e de `porecatu-remote`**.

### 6. Arquivo do crate

`remote_devices.json`, ao lado do `session.json`, guarda os aparelhos pareados ([ADR-0106](0106-pareamento-identidade-e-autorizacao.md) §5). Quem lê e escreve é `porecatu-remote`, na thread dele, a pedido da main thread; o diretório chega em `RemoteParams`.

### 7. Configuração

```toml
[remote]
# Acesso remoto pelo Porecatu Remote (PRD-025). Desligado por padrão.
enabled = false
# Endereço do seu Porecatu Relay. O token fica no cofre do sistema.
relay_url = ""
```

As duas chaves são **classe A** ([ADR-0030](0030-escopo-do-hot-reload.md)): `enabled` cria ou destrói o `RemoteHost` no próximo evento; `relay_url` trocado derruba a conexão e abre outra. Nenhuma toca PTY nem grade. A seção entra no `porecatu.example.toml` **no PR de implementação**, com o código (o `tests/example_toml.rs` reprova chave sem campo em `Config`, como no [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §9).

## Alternativas consideradas

### Runtime compartilhado com `porecatu-ssh`

Um runtime do processo num crate comum, usado pelos dois. Economiza threads, mas obriga os dois crates a conhecer um terceiro e acopla o ciclo de vida: o SSH nasce na primeira conexão, o remoto nasce com a chave ligada, e um runtime comum teria de nascer com o primeiro e morrer com o último. Mexe no [ADR-0072](0072-cliente-ssh-embutido.md) sem ganho de comportamento.

### Sem `tokio`: threads e `tungstenite` síncrono

Mantém a regra intacta: uma thread por conexão, bloqueante, como as de PTY. Funciona para a conexão com o relay, mas o host tem N canais, cada um com handshake, temporizador de confirmação, controle de fluxo e fila própria, em cima de um socket só. Reescrever um escalonador à mão para isso é pior que aceitar um runtime isolado.

### Integrar no `porecatu-ui`

Rede e cripto dentro do crate da GUI. Violaria a regra de que I/O mora em crate folha e traria `tokio` para o crate que tem o event loop.

## Consequências

### Positivas

- `porecatu-ui` continua sem `tokio`, sem cripto e sem rede.
- Desligado, o recurso não existe em tempo de execução.
- `porecatu-remote` é testável sem GUI: um relay falso em memória e um cliente de teste falam o protocolo com ele.

### Negativas

- Dois runtimes `tokio` e duas versões de `keyring` para manter alinhadas (a mesma versão exata, conferida em `cargo tree`).
- Mais uma variante em `Wakeup`, com o cuidado de tamanho que o comentário do enum já pede.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Duas versões de `tokio` no grafo | Média | Médio | `tokio` exato nos dois crates, subido junto; `cargo tree -d` no CI |
| Fila de comandos crescendo com rede lenta | Média | Médio | Fila limitada; grade é "último estado vence" por painel ([ADR-0105](0105-espelhamento-da-grade-e-input-remoto.md)) |
| `keyring` em dois crates divergir de versão | Baixa | Baixo | Mesma regra de pino exato; Dependabot sobe os dois juntos |
