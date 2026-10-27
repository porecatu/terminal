# ADR-0073 — Credenciais SSH no cofre do sistema: `keyring` dentro de `porecatu-ssh`, entrada por destino, gravada só depois de aceita

**Status:** Aceito
**Data:** 2026-10-27
**Relacionados:** [ADR-0010](0010-licenciamento.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0071](0071-backend-de-terminal.md), [ADR-0072](0072-cliente-ssh-embutido.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md), [PRD-020](../prd/prd-020-conexoes-ssh.md)

## Contexto

O dono do produto decidiu que senhas de SSH ficam **no cofre do sistema operacional**, nunca em arquivo do app ([PRD-020](../prd/prd-020-conexoes-ssh.md), "Decisões do dono"). O PRD fixou o ciclo de vida do segredo:

- o perfil não tem campo de segredo (RF-20.2);
- o usuário escolhe lembrar no diálogo, desmarcado por padrão (RF-20.12);
- só se grava **depois** de o servidor aceitar; segredo salvo e recusado é apagado (RF-20.13);
- sem cofre, a opção some e o segredo é pedido sempre (RF-20.14);
- excluir o perfil oferece apagar o que foi salvo (RF-20.4);
- segredo nunca aparece em arquivo, nota, aviso, título, árvore de acessibilidade ou log (RF-20.34).

Os cofres são três, um por plataforma — Gerenciador de Credenciais do Windows, Keychain do macOS, Secret Service (via D-Bus) no Linux —, todos com API nativa que exige `unsafe` ou D-Bus. O projeto proíbe `unsafe` no próprio código e passa operação de SO por crate wrapper seguro (CLAUDE.md, "Regras que vieram dos recursos").

Levantamento de 2026-10-27: o crate `keyring` 4.2.0 (MIT OR Apache-2.0) cobre os três com a feature `v1` — Keychain, Credential Manager e Secret Service por `zbus` em Rust puro. Desde a 4.0 a API mora em `keyring-core`, e o autor recomenda que aplicações que querem escolher o armazenamento dependam de `keyring-core` mais crates de armazenamento específicos; a feature `v1` do `keyring` é exatamente a escolha "o armazenamento nativo de cada plataforma", que é a nossa.

## Decisão

**O cofre é o `keyring` 4.x com a feature `v1`, usado só dentro de um módulo `vault` de `porecatu-ssh`. Cada segredo é uma entrada indexada pelo destino — `usuário@host:porta` para senha, caminho canônico do arquivo para passphrase —, não pelo perfil. Quem lê, grava e apaga durante a conexão é a tarefa SSH; a UI só pergunta se o cofre existe e pede para esquecer.**

### 1. Dependência

```toml
# crates/porecatu-ssh/Cargo.toml
# ADR-0073. Pinar exato; subir é tarefa própria.
keyring = { version = "=4.2.0", default-features = false, features = ["v1"] }
```

Versão exata pelo mesmo motivo do `russh`: o crate mudou de forma entre a 3.x e a 4.x, e a próxima pode mudar de novo. A verificação de licença do [ADR-0010](0010-licenciamento.md) roda sobre a árvore que ele traz (`zbus` e afins no Linux), no PR que o adiciona.

### 2. Entradas

| Segredo | Serviço | Conta |
|---|---|---|
| Senha | `porecatu-ssh` | `password:<usuário>@<host>:<porta>` |
| Passphrase de chave | `porecatu-ssh` | `passphrase:<caminho canônico do arquivo da chave>` |

- **Por destino, não por perfil.** Renomear o identificador de um perfil, reimportá-lo do `~/.ssh/config` ou ter dois perfis para o mesmo servidor não perde nem duplica a senha. A senha é de uma conta num servidor; o perfil é só um jeito de chegar lá.
- `host` como escrito no perfil, em minúsculas; porta sempre explícita, mesmo 22. `caminho canônico` resolve `~`, `..` e links.
- O nome do serviço é fixo e aparece na interface do cofre do sistema; ele diz ao usuário de quem é a entrada.

### 3. Ciclo de vida, todo dentro da conexão

A tarefa SSH ([ADR-0072](0072-cliente-ssh-embutido.md) §4) é a única que lê e grava durante uma conexão:

1. **Precisa de senha ou passphrase** → lê a entrada. Achou → tenta, sem perguntar nada.
2. **O segredo salvo foi recusado como credencial** → **apaga a entrada** e segue para o passo 3, que é o que o RF-20.13 pede: um segredo salvo que não funciona mais não pode ficar sendo tentado a cada conexão. "Recusado como credencial" é estrito:
   - **senha**: o servidor respondeu `SSH_MSG_USERAUTH_FAILURE` à tentativa de senha (ou ao keyboard-interactive de um campo, RF-20.10) **e continua oferecendo** `password`/`keyboard-interactive` na lista de métodos da resposta — ou seja, o método segue disponível e foi esta senha que não passou;
   - **passphrase**: a chave não decifra com ela, o que é local e inequívoco.
   
   Qualquer outra falha — conexão derrubada no meio da autenticação (`fail2ban`, `MaxAuthTries`, limite de taxa), método retirado da lista pelo servidor, tempo esgotado — **mantém a entrada** e encerra com a causa que houver. Apagar ali seria perder uma senha válida por um motivo que não é a senha.
3. **Não havia entrada, ou ela foi apagada** → pergunta pelo canal do [ADR-0071](0071-backend-de-terminal.md) §5, com a informação de se o cofre está disponível (para o diálogo habilitar ou não a caixa "Lembrar").
4. **Usuário respondeu com "Lembrar"** → o segredo é guardado **em memória** até o servidor responder. Aceito → grava no cofre. Recusado → descarta e volta ao passo 3.

Falha ao **gravar** (cofre trancado, usuário negou a permissão do sistema) não derruba a conexão, que já foi aceita: vira **aviso do app** ([ADR-0014](0014-superficie-de-aviso-e-dialogo.md), canal 1) dizendo que o segredo não foi salvo — um consumidor novo daquele canal, registrado no ADR-0075. Falha ao **ler** é tratada como entrada ausente: o usuário é perguntado, que é o que aconteceria sem cofre.

O acesso ao cofre é bloqueante (chamada de SO, D-Bus) e roda em `tokio::task::spawn_blocking`, nunca numa thread de trabalho do runtime e nunca na main thread.

> **Como ficou (prompt 13).** O ciclo mora em `cycle.rs`, sem `russh`: quem fala com o servidor é um `Attempter` (senha, keyboard-interactive de um campo, passphrase), o que o deixa testável com um cofre em memória (`vault::Vault`, trait interno). Três precisões que a implementação trouxe:
>
> - **Quatro resultados, não dois.** Além de aceito e recusado, há **`Declined`** (o segredo está certo e o método não levou à autenticação: a chave decifrou e o servidor não a quer; o método saiu da lista) e **`Failed`** (a troca não terminou). Nenhum dos dois apaga a entrada, grava ou reabre o diálogo; `Declined` deixa o próximo método da lista seguir.
> - **Queda no meio da autenticação.** O `russh` não devolve erro quando a conexão cai durante uma tentativa: devolve uma falha com a **lista de métodos vazia**. É essa a assinatura que a implementação lê como queda (`Failed`, causa `Refused`, entrada **mantida**); uma lista com outros métodos, mas sem o de senha, é método retirado (`Declined`). Um servidor de verdade não fecha a lista vazia.
> - **Passphrase só é gravada depois do aceite da chave.** "Recusada" é local (a chave não decifra), mas gravar espera o servidor aceitar a chave decifrada, como na senha: uma passphrase certa para uma chave que o servidor não conhece não é guardada.
>
> **`vault_available()`** é guardado por processo e **nunca bloqueia quem chama**: na primeira chamada devolve `false` e dispara o cálculo numa thread de fundo; a conexão calcula o seu dentro da tarefa SSH, onde pode esperar. A tela de configurações (prompt 15) lê o valor guardado, como o §Riscos já previa. **Servidor com campo oculto único** (keyboard-interactive, RF-20.10) é examinado **antes** de perguntar a senha: a troca começa, e um segundo fator ou um campo visível falha com `AuthUnsupported` sem diálogo. Um segundo campo depois da senha também falha assim, **sem** apagar a entrada. **Cópia fora do alcance do `Secret`:** o `russh` pede um `String` para `authenticate_password` e o clipboard devolve um `String` ao colar; o primeiro é do `russh`, o segundo é zerado ao entrar no campo.

### 4. A UI só pergunta e esquece

`porecatu-ssh` expõe, re-exportado por `porecatu-term` como os demais tipos ([ADR-0071](0071-backend-de-terminal.md) §2):

```rust
/// Destino normalizado: host em minúsculas, porta sempre explícita.
/// `user` e `host` já validados pela config (ADR-0072 §6) — sem `@`, `:`,
/// espaço nem controle —, o que torna `password:<user>@<host>:<port>`
/// inequívoco.
pub struct Destination { pub user: String, pub host: String, pub port: u16 }

pub fn vault_available() -> bool;                  // resultado guardado por processo
pub fn forget_password(dest: &Destination) -> Result<(), VaultError>;
pub fn forget_passphrase(key_file: &Path) -> Result<(), VaultError>;
```

- `vault_available` é usada pela tela de configurações para mostrar ou não o botão de esquecer, e é a mesma resposta que a tarefa SSH manda junto da pergunta. É calculada uma vez, na primeira chamada, por uma operação de leitura que não exige entrada existente. No Linux sem sessão D-Bus ou sem serviço de segredos, é `false` — sem fallback para outro armazenamento (RF-20.14).
- Excluir um perfil SSH (RF-20.4) chama `forget_*` **só se nenhum outro perfil** aponta para o mesmo destino ou o mesmo arquivo de chave — senão o diálogo nem oferece a opção. Quem decide isso é `porecatu-ui`, que tem a lista de perfis.
- Nenhuma função devolve o segredo. A UI nunca lê o cofre.

### 5. O segredo na memória

```rust
pub struct Secret(zeroize::Zeroizing<String>);
// sem Clone, sem Display; Debug imprime "Secret(..)"
```

- O diálogo entrega o texto direto num `Secret`; o campo mascarado zera o próprio buffer ao fechar ([ADR-0075](0075-superficies-de-ui-do-ssh.md)).
- `Secret` atravessa `porecatu-term` **por movimento**, sem ser guardado ([ADR-0071](0071-backend-de-terminal.md) §5), e é descartado — e zerado — pela tarefa SSH depois da autenticação ou de gravado no cofre.
- `zeroize` (MIT OR Apache-2.0) deve vir da árvore do `russh`, pela pilha RustCrypto — a confirmar com `cargo tree` no PR que o adiciona —; é declarado explicitamente em `porecatu-ssh`, na versão que a árvore já usa.
- **Nenhum `Error` de `porecatu-ssh` carrega o segredo**, e as variantes de erro de autenticação são enums sem campo de texto livre vindo do usuário.

Zerar memória é defesa em profundidade, não garantia: `String` intermediária do sistema de texto, cópia no clipboard se o usuário colou de lá, *swap*. O que se garante é que o app não **guarda** o segredo além do necessário.

### 6. O que o sistema operacional pode mostrar

O Keychain do macOS e alguns serviços de segredo do Linux podem abrir um **diálogo do sistema** pedindo permissão ou a senha do cofre. Isso não é um diálogo nativo **nosso** — o [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) proíbe o app de usar diálogo do sistema para falar com o usuário, e aqui é o sistema falando com o usuário sobre o próprio cofre, sem o app poder impedir. Registrado para não ser lido como violação.

## Alternativas consideradas

### Arquivo cifrado do app, com senha-mestra

Funciona igual nas três plataformas. Recusada: é um cofre caseiro — derivação de chave, formato, migração, e mais uma senha para o usuário. O cofre do sistema é desbloqueado pelo login, tem auditoria do SO e é onde o usuário procura quando quer apagar.

### Senha no `porecatu.toml`

Recusada pelo dono e pelo RF-20.2. O arquivo é texto e é versionado.

### Indexar pelo identificador do perfil

Mais simples de achar. Recusada: renomear o identificador ou reimportar perde a senha em silêncio, e dois perfis para o mesmo servidor pediriam a mesma senha duas vezes.

### `keyring-core` com armazenamentos escolhidos à mão

O que o autor recomenda para quem quer controle. Recusada **por ora**: a feature `v1` escolhe exatamente os armazenamentos nativos que queremos, e escolher à mão seria repetir a decisão dele em três `#[cfg]`. Revisitável se o Linux precisar de `keyutils` (que funciona sem D-Bus, mas não sobrevive a reinício).

### Fallback em memória, sem cofre

Lembrar só até fechar o app. Recusada: cria um terceiro comportamento que o usuário não vê e não controla — a caixa "Lembrar" diria uma coisa e faria outra.

### Gravar no cofre antes de saber se o servidor aceita

Um passo a menos. Recusada pelo RF-20.13: senha errada salva vira uma falha que se repete sozinha a cada conexão.

## Consequências

### Positivas

- Nenhum segredo em arquivo do app, por construção: o perfil não tem o campo, e só `porecatu-ssh` toca o cofre.
- O usuário encontra e apaga as entradas na ferramenta do próprio sistema, sob o nome `porecatu-ssh`.
- Uma senha por conta de servidor, não por perfil.

### Negativas

- Mais uma dependência com código de SO (`keyring`, e `zbus` no Linux).
- Linux sem serviço de segredos — servidor gráfico mínimo, WSLg, sessões remotas — fica sem "Lembrar".
- O macOS pode mostrar um diálogo do sistema na primeira gravação, fora do controle e da aparência do app.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Segredo vazar por `Debug`, erro ou log | Baixa | **Alto** | `Secret` sem `Display`/`Clone`, `Debug` opaco; erros sem campo de texto livre; teste que formata cada variante de erro de `porecatu-ssh` e procura um segredo de prova |
| Senha errada ficar salva | Baixa | Médio | §3 passo 4: grava só depois de aceita; teste com servidor de prova que recusa a primeira |
| Senha válida ser apagada por bloqueio do servidor (`fail2ban`, `MaxAuthTries`) | Média | Médio | §3 passo 2: só apaga em recusa de credencial com o método ainda oferecido; teste com servidor de prova que derruba a conexão no meio da autenticação |
| `vault_available` travar a UI no Linux com D-Bus lento | Média | Médio | Calculada fora da main thread na primeira conexão; a tela de configurações lê o valor guardado e, enquanto não há valor, mostra o botão desabilitado |
| Apagar a senha de um perfil que outro perfil ainda usa | Média | Baixo | §4: só se nenhum outro perfil aponta para o mesmo destino |
| API do `keyring` mudar de novo | Média | Baixo | Versão exata; uso confinado ao módulo `vault` |
