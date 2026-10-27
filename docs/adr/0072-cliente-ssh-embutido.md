# ADR-0072 — Cliente SSH embutido: `russh` com `ring` num crate novo `porecatu-ssh`, runtime `tokio` isolado dentro dele, perfil `ssh` em `[[profiles.list]]`

**Status:** Aceito
**Data:** 2026-10-27
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0010](0010-licenciamento.md), [ADR-0011](0011-toolchain-rust.md), [ADR-0012](0012-identificacao-do-terminal.md), [ADR-0070](0070-perfis-de-aba.md), [ADR-0071](0071-backend-de-terminal.md), [ADR-0073](0073-credenciais-no-cofre-do-sistema.md), [ADR-0074](0074-verificacao-da-chave-do-host.md), [ADR-0076](0076-reconexao-automatica.md), [PRD-007](../prd/prd-007-perfis-de-aba.md), [PRD-020](../prd/prd-020-conexoes-ssh.md)
**Supersedes:** ADR-0007, "Alternativas consideradas → Runtime async (tokio)" (**parcial**: um runtime `tokio` passa a existir, **só** dentro de `porecatu-ssh`; a main thread, o PTY local e o render continuam sem async) · ADR-0070 §1 (**parcial**: `type = "ssh"` deixa de ser recusado quando este ADR estiver implementado)

## Contexto

O dono do produto decidiu por um **cliente SSH embutido** ([PRD-020](../prd/prd-020-conexoes-ssh.md), "Decisões do dono"), com autenticação por agente, chave e senha, chave de host no `~/.ssh/known_hosts`, divisão de painel sobre a mesma conexão e reconexão automática. O [ADR-0071](0071-backend-de-terminal.md) já abriu o lugar: um `SshBackend` em `porecatu-term`, sobre um crate novo no mesmo andar de `porecatu-pty`.

Falta escolher a biblioteca, e a escolha arrasta três restrições do projeto:

- **Licença** compatível com GPL-3.0-or-later, conferida antes de adotar ([ADR-0010](0010-licenciamento.md)), incluindo a pilha de criptografia.
- **`unsafe_code = "deny"`** vale para o nosso código; `unsafe` dentro de crate de terceiro é aceito, como em `portable-pty` e `wgpu`.
- **O [ADR-0007](0007-modelo-de-threading.md) recusou runtime async**: uma thread de SO por PTY, bloqueante, sem `tokio`. A recusa tinha um motivo concreto — um runtime inteiro para três tarefas bloqueantes por aba —, e esse motivo continua certo para o PTY.

Levantamento feito em 2026-10-27, por crates.io e leitura do fonte:

| Opção | Licença | Forma | O que pesa |
|---|---|---|---|
| **`russh` 0.65.0** (2026-10-25) | Apache-2.0 | Rust, async, exige `tokio` | Ativo; agente nativo (socket Unix, pipe nomeado do OpenSSH no Windows, Pageant); chave cifrada com passphrase, PKCS#8 e `.ppk`; keyboard-interactive; keepalive; vários canais por conexão. Fixa `ssh-key =0.7.0-rc.11` e `rsa =0.10.0-rc.18`, pré-lançamentos |
| `ssh2` 0.9.6 | MIT OR Apache-2.0, libssh2 BSD-3 | C via FFI, bloqueante | OpenSSL fora do Windows; toda a lógica de protocolo em C |
| `thrussh` 0.49.0 | Apache-2.0 | Rust, async | Depende de libsodium em C e de RustCrypto antigo; o `russh` é o fork ativo |
| `wezterm-ssh` 0.4.0 | MIT | sobre `ssh2`/libssh | Última publicação de 2022; libssh é LGPL-2.1 |

Avisos de segurança do `russh` nos últimos dois anos, todos corrigidos: alocação ilimitada em quadro de agente (RUSTSEC-2026-0154, corrigido em 0.60.3), DoS por descompressão e falha de estado de autenticação do servidor (0.61.0), alocação enorme em keyboard-interactive do cliente (0.61.0), pânico por X25519 de tamanho errado vindo de servidor malicioso (0.62.4). **0.62.4 é o piso**; 0.65.0 cobre todos.

O `russh` exige escolher um backend de criptografia, `aws-lc-rs` (o default) ou `ring`:

| Backend | Licença | Build no Windows |
|---|---|---|
| `ring` 0.17.14 | Apache-2.0 AND ISC | compilador C (MSVC, que o projeto já exige) |
| `aws-lc-rs` 1.18.2 / `aws-lc-sys` 0.46.0 | ISC, Apache-2.0, MIT, BSD-3-Clause | compilador C **e NASM** em x86-64, a menos de binário pré-compilado de NASM |

As duas licenças são compatíveis com a GPLv3. A maior parte dos algoritmos do `russh` (AES-CTR, Ed25519, ECDSA, ML-KEM, SHA-2, HMAC) vem de RustCrypto em qualquer dos dois; o backend cobre uma parte.

## Decisão

**O cliente SSH é o `russh` `=0.65.0` com `default-features = false` e as features `ring`, `rsa` e `flate2`, dentro de um crate novo `porecatu-ssh`, que é o único do workspace a depender de `russh` e de `tokio`. O runtime `tokio` é criado sob demanda, na primeira conexão, e vive dentro de `porecatu-ssh`; a fronteira com `porecatu-term` é síncrona. Um destino SSH é um perfil `type = "ssh"` em `[[profiles.list]]`.**

### 1. Por que `russh`

É a única opção em Rust puro no protocolo, ativa, com licença permissiva e com **agente nativo nas três plataformas** — o pipe nomeado do OpenSSH do Windows é o caso que mais pesa, porque é o agente que o próprio Windows instala. Vários canais por conexão é o que faz o RF-20.23 (dividir sem reautenticar) ser barato. `ssh2` traria libssh2 e OpenSSL em C, num projeto que até aqui só tem C dentro de crates de GPU e de fonte; `thrussh` traria libsodium.

O custo aceito é o `tokio` (§3) e os pré-lançamentos fixados pelo `russh` (§2).

### 2. Dependência: versão exata, `ring`, sem DSA

```toml
# crates/porecatu-ssh/Cargo.toml
[dependencies]
# ADR-0072. Pinar exato; subir é tarefa própria, conferindo RUSTSEC (piso 0.62.4).
russh = { version = "=0.65.0", default-features = false, features = ["ring", "rsa", "flate2"] }
tokio = { version = "…", default-features = false, features = ["rt-multi-thread", "net", "sync", "time", "io-util"] }
```

- **Versão exata**, como `alacritty_terminal`, `wgpu` e `toml_edit` (CLAUDE.md, "Dependências"). O `russh` fixa `ssh-key` e `rsa` em pré-lançamento com `=`, e subir um sem o outro é exatamente o tipo de resolução que quebra em silêncio.
- **`ring`, não `aws-lc-rs`**: mesma compatibilidade de licença, um requisito de build a menos (NASM) nas máquinas de quem contribui e no CI de Windows.
- **`rsa` ligado**: chave RSA de usuário ainda é a mais comum em servidor legado. O `rsa` em pré-lançamento tem histórico de canal lateral de tempo (RUSTSEC-2023-0071, *Marvin*); o estado dele na série 0.10 fica **a conferir no PR que adiciona a dependência**, registrado como risco abaixo.
- **`dsa` desligado**: DSA está desabilitado no OpenSSH desde 2015.
- **`tokio` não entra em `[workspace.dependencies]`**: declarado só em `porecatu-ssh`, para que nenhum outro crate o importe por conveniência. A versão é a que o `russh` 0.65.0 aceita, fixada no mesmo PR.

A verificação de licença do [ADR-0010](0010-licenciamento.md) roda sobre a **árvore inteira** que `porecatu-ssh` traz (`cargo tree -p porecatu-ssh -e normal`), não só sobre os dois nomes acima, e o resultado entra no PR.

### 3. Runtime: `tokio` isolado, criado sob demanda

```rust
// crates/porecatu-ssh/src/runtime.rs
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("porecatu-ssh")
            .enable_io()
            .enable_time()
            .build()
            .expect("runtime do porecatu-ssh")
    })
}
```

- **Sob demanda.** Nada é criado no arranque: quem nunca abre uma aba SSH não tem runtime, nem threads, nem um byte a mais de memória residente além do código. O tempo até o primeiro prompt (métrica do [PRD-000](../prd/prd-000-visao-de-produto.md)) não muda.
- **Duas threads de trabalho para o processo inteiro**, não por conexão. Uma conexão SSH ociosa custa uma tarefa, não uma thread — o oposto do PTY, e o motivo de a recusa do ADR-0007 não se aplicar aqui.
- **Nunca na main thread.** Nenhuma função pública de `porecatu-ssh` é `async`, nenhuma chama `block_on` de fora do runtime, e nenhum tipo de `tokio` aparece na API.
- O `expect` acima é a falha de criar threads de SO no arranque da primeira conexão; vira `FailCause::Runtime` em vez de pânico na implementação.

### 4. Fronteira síncrona com `porecatu-term`

O `SshBackend` do [ADR-0071](0071-backend-de-terminal.md) §1 é escrito sobre uma API **bloqueante** que `porecatu-ssh` oferece:

```rust
pub struct SshSpawn { /* §6 — tipos do projeto, nenhum do russh */ }

pub struct SshChannel { … }
impl SshChannel {
    pub fn open(spawn: SshSpawn, prompts: PromptSink) -> SshChannel;   // devolve na hora
    pub fn reader(&self) -> impl Read + Send;     // std::sync::mpsc de Vec<u8> por baixo
    pub fn writer(&self) -> impl Write + Send;    // tokio::sync::mpsc::UnboundedSender por baixo
    pub fn resize(&self, rows: u16, cols: u16);
    pub fn try_end(&self) -> Option<End>;
    pub fn connection(&self) -> Option<ConnectionId>;
    pub fn close(self);
}
```

- **Leitura**: a tarefa async empurra os dados do canal num `std::sync::mpsc`, e o `Read` bloqueia a thread de leitura de `porecatu-term` como o PTY bloqueia. A thread de leitura não sabe que existe um runtime.
- **Escrita**: `UnboundedSender::send` é síncrono e não bloqueia; a tarefa async drena e escreve no canal. As respostas DSR/DA/CPR do motor seguem pelo mesmo caminho, como no PTY.
- **Perguntas** (senha, passphrase, chave de host): a tarefa manda a pergunta pelo `PromptSink` — um `Sender` de `porecatu-term` — e espera a resposta num `tokio::sync::oneshot`, **sem ocupar a thread de trabalho** enquanto o usuário digita. `Terminal::answer` completa o `oneshot` do lado síncrono.

### 5. Conexões e canais

`porecatu-ssh` guarda um registro de conexões vivas, por `ConnectionId`. `SshSpawn` diz se o canal pede **conexão nova** ou **canal numa conexão existente**:

- aba nova de um perfil SSH → conexão nova, sempre — duas abas do mesmo perfil não compartilham conexão, para que uma queda ou um `exit` não leve as duas;
- dividir um painel SSH → canal novo na conexão do painel de origem (RF-20.23), sem reautenticar; se ela não está viva, conexão nova;
- a conexão fecha quando o último canal dela fecha.

Cada canal pede um pseudo-terminal remoto com `TERM=xterm-256color` ([ADR-0012](0012-identificacao-do-terminal.md)) e o tamanho da grade, tenta `COLORTERM=truecolor` por `setenv` — servidor que recusa não é erro —, e abre o shell de login do usuário. Nenhum comando remoto é configurável nesta leva.

### 6. O perfil `ssh`

```toml
[[profiles.list]]
id = "prod"
name = "prod-web-01"
type = "ssh"
# badge = "SSH"                      # default para type = "ssh"
host = "10.4.1.20"
port = 22                            # opcional
user = "deploy"
# auth = ["agent", "key", "password"]  # opcional; esta é a ordem default (RF-20.9)
# identity_file = "~/.ssh/id_ed25519"  # obrigatório se "key" estiver em auth e for o único método
```

- `user` é **obrigatório**. O OpenSSH cai no usuário local; aqui o perfil é cadastro, e um cadastro que funciona numa máquina e não na outra por depender do nome de login local é pior que um campo a mais. A importação do `~/.ssh/config` (ADR-0075) preenche com o usuário local quando o bloco não tem `User`.
- `program`, `args`, `env` e `cwd` **não valem** em perfil `ssh`. São chaves conhecidas de perfil com valor inválido para o tipo, e por isso **erro** naquele perfil, que é descartado — o caso "perfil inválido" do RF-7.5, não o de "chave desconhecida".
- **`host` e `user` são validados**, porque viram linha do `known_hosts` do OpenSSH ([ADR-0074](0074-verificacao-da-chave-do-host.md) §5) e conta no cofre ([ADR-0073](0073-credenciais-no-cofre-do-sistema.md) §2). `host` não aceita espaço, caractere de controle, `,`, `|`, `[`, `]`, `@`, `#`, `*`, `?`, `!` — um IPv6 literal é escrito sem colchetes (`::1`), e os colchetes do formato `[host]:porta` são postos por quem grava. `user` não aceita espaço, caractere de controle, `@` nem `:`. Inválido é erro naquele perfil.
- `auth` aceita só `"agent"`, `"key"`, `"password"`; **a lista é a regra**: a ordem dada é a ordem tentada, e método fora dela não é tentado — com `identity_file` presente e `auth = ["agent", "password"]`, a chave não é usada. Sem `auth`, a ordem default (RF-20.9) usa a chave só quando `identity_file` existe. Keyboard-interactive com um campo oculto é tratado como `"password"` (RF-20.10), sem valor próprio, e o texto que o servidor manda para o campo não é exibido ([ADR-0075](0075-superficies-de-ui-do-ssh.md) §2).
- **Nenhum campo de segredo.** Chave cujo nome contenha `password`, `passphrase` ou `secret` é chave desconhecida e, como toda chave desconhecida, **aviso** (RF-7.5) — mas com frase própria, que diz que segredo vai no cofre do sistema e que o valor foi ignorado. O valor nunca é lido para nada.
- `porecatu-ui` traduz o perfil validado num `SshSpawn` (§4), como traduz `[scrollback]` em `TermParams`. `porecatu-term` e `porecatu-ssh` não conhecem `Config`.

### 7. Agente

| Plataforma | Ordem |
|---|---|
| Windows | pipe nomeado do OpenSSH (`\\.\pipe\openssh-ssh-agent`), depois Pageant |
| Linux, macOS | `SSH_AUTH_SOCK` |

Agente ausente pula o passo sem aviso (RF-20.9). **Encaminhamento de agente nunca é pedido** (RF-20.36).

### 8. Tempos

- Conexão TCP mais troca de chaves: teto de **15 s**, depois `Failed { cause: Timeout }`.
- **Não há teto de autenticação do lado do cliente.** Tempo que o usuário leva num diálogo não conta: a conexão espera o `oneshot`. O servidor tem o próprio `LoginGraceTime`, e se ele desistir primeiro, o que chega é `Failed { cause: Refused }` ou `AuthRejected`, conforme o servidor encerrar.
- Keepalive e detecção de queda são do ADR-0076.

### 9. Onde o código mora

| Crate | O que muda |
|---|---|
| `porecatu-ssh` (novo) | `russh`, `tokio`, runtime, registro de conexões, `SshChannel`, agente; e o cofre (ADR-0073) e o `known_hosts` (ADR-0074) |
| `porecatu-term` | `SshBackend`; re-exporta `SshSpawn` e os tipos de pergunta |
| `porecatu-config` | `type = "ssh"` e os campos do §6, com validação |
| `porecatu-ui` | perfil → `SshSpawn`; diálogos (ADR-0075); reconexão (ADR-0076) |
| `porecatu-pty`, `porecatu-core`, `porecatu-session`, `porecatu-render` | **nada** além do que o ADR-0070 já pede |

O CLAUDE.md ganha `porecatu-ssh` no grafo e na tabela de dependências (pode depender de nada do projeto; nunca de GUI, `config`, `core`), e `russh` na lista "Stack travada".

## Alternativas consideradas

### `ssh` do sistema num PTY

Recusada pelo dono do produto, por produto: sem acesso ao protocolo não há reconexão com o scrollback, divisão sobre a mesma conexão nem confirmação por conexão aberta (ver [ADR-0071](0071-backend-de-terminal.md)).

### `ssh2` (libssh2)

API bloqueante, que casaria com o ADR-0007 sem runtime nenhum. Recusada: traz o protocolo inteiro em C, com OpenSSL em Linux e macOS, e o agente do Windows por pipe nomeado não é coberto pela libssh2 da mesma forma — o caso que mais pesa para o público do produto.

### `aws-lc-rs` como backend de criptografia

O default do `russh`. Recusada só por build: NASM obrigatório em x86-64 no Windows, sem ganho funcional para um cliente.

### Um runtime `tokio` por conexão, ou `current_thread` numa thread dedicada por aba

Isolaria cada conexão. Recusada: reproduz o custo de thread por terminal que o ADR-0007 queria evitar no PTY, sem o motivo que o justificava lá (I/O bloqueante de SO).

### Runtime criado no arranque

Simplifica o código. Recusada pela métrica de arranque e porque a maioria dos usuários nunca abre uma aba SSH.

### Destino SSH como seção própria (`[[ssh.hosts]]`), fora de perfis

Separaria cadastro de host de perfil. Recusada pelo dono: o SSH é um tipo de perfil (PRD-007), e dois cadastros paralelos dariam dois menus, dois badges e dois jeitos de gravar na sessão.

## Consequências

### Positivas

- Agente, chave cifrada e senha nas três plataformas sem uma linha de C nossa nem de terceiro no protocolo.
- Conexão ociosa custa uma tarefa, não uma thread; quem não usa SSH não paga runtime.
- A fronteira com `porecatu-term` é síncrona: o `SshBackend` é escrito como o `PtyBackend`.

### Negativas

- **`tokio` entra no workspace**, e com ele a tentação de usá-lo em outro lugar. A regra do §2 (não entra em `[workspace.dependencies]`) é a barreira, e é só convenção.
- A árvore de dependências cresce bastante (RustCrypto, `ring`, `tokio`), com efeito no tempo de build limpo e no tamanho do binário.
- `ssh-key` e `rsa` em pré-lançamento, fixados pelo `russh`: subir o `russh` pode mudar a API de chave sem aviso de semver.
- Um ADR aceito (0007) parcialmente supersedido numa decisão que ele tinha recusado de forma explícita.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Vulnerabilidade nova no `russh` ou na pilha cripto | Média | **Alto** | Versão exata; subir é tarefa própria, conferindo RUSTSEC; piso 0.62.4 registrado aqui |
| `rsa` em pré-lançamento com canal lateral de tempo (*Marvin*) | Média | Médio | Conferir o estado do RUSTSEC-2023-0071 na série 0.10 no PR da dependência; se aberto, registrar no guia que chave Ed25519 é a recomendada, e avaliar `rsa` desligado |
| Licença incompatível entrando por dependência transitiva | Baixa | **Alto** | §2: `cargo tree` completo conferido no PR, com a lista no texto do PR |
| Código de outro crate passar a usar `tokio` | Média | Médio | `tokio` só no `Cargo.toml` de `porecatu-ssh`; revisão de PR confere `cargo tree -i tokio` com um único caminho |
| Bloquear a thread de trabalho do runtime esperando o usuário | Média | **Alto** | §4: perguntas por `oneshot`, nunca por `recv` síncrono dentro de tarefa |
| Conexão que não responde travar a aba em "conectando" | Média | Baixo | §8: teto de 15 s até a troca de chaves |
| Build do `ring` falhar em alvo do CI | Baixa | Médio | `ring` só pede compilador C, que o CI das três plataformas já tem |
