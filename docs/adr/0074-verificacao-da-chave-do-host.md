# ADR-0074 — Verificação da chave do host: `~/.ssh/known_hosts` do OpenSSH, leitura pelo `ssh-key`, casamento próprio, só acréscimo

**Status:** Aceito
**Data:** 2026-10-27
**Relacionados:** [ADR-0071](0071-backend-de-terminal.md), [ADR-0072](0072-cliente-ssh-embutido.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md), [ADR-0076](0076-reconexao-automatica.md), [PRD-020](../prd/prd-020-conexoes-ssh.md)

## Contexto

O dono do produto decidiu que a chave do host é conferida contra o **`~/.ssh/known_hosts` compartilhado com o OpenSSH** — lido e gravado ([PRD-020](../prd/prd-020-conexoes-ssh.md), "Decisões do dono"). O PRD fixou o comportamento: entradas com nome em hash valem (RF-20.15); host desconhecido abre um diálogo com a impressão digital SHA-256 e o foco no cancelar (RF-20.16); chave diferente bloqueia sem botão de continuar (RF-20.17); `@revoked` bloqueia e `@cert-authority` não basta para confiar (RF-20.18).

Compartilhar o arquivo tem um preço que define o resto: **o arquivo é do OpenSSH**. O usuário o edita, o `ssh` o edita, o `ssh-keygen -R` o edita, às vezes ao mesmo tempo que nós. Qualquer coisa que o Porecatu faça nele tem de ser algo que o OpenSSH também faria.

O que as bibliotecas oferecem (levantamento de 2026-10-27):

- **`russh::keys::known_hosts`** confere e aprende, casa entradas em hash e `[host]:porta`, mas **não trata `@cert-authority` nem `@revoked`**, não casa curinga nem negação, e — pela leitura do fonte, sem teste — uma linha com marcador provavelmente faz a leitura inteira falhar. Grava sempre sem hash.
- **`ssh-key`** (já na árvore do `russh`, que o re-exporta) **lê** o formato completo — marcadores, padrões, entradas em hash — e escreve entradas, mas não tem função de casamento.

## Decisão

**O Porecatu lê o `known_hosts` com o leitor do `ssh-key` (pela re-exportação do `russh`), casa o host com código próprio em `porecatu-ssh` — curinga, negação, `[host]:porta` e hash HMAC-SHA1 —, nunca reescreve nem remove linha, e acrescenta no fim do arquivo do usuário só quando o usuário aceita um host desconhecido.**

### 1. Quais arquivos

| Arquivo | Lido | Gravado |
|---|---|---|
| `~/.ssh/known_hosts` | sim | **sim**, só acréscimo |
| `~/.ssh/known_hosts2` | sim | não |
| Global: `/etc/ssh/ssh_known_hosts` e `/etc/ssh/ssh_known_hosts2` (Linux, macOS), `%ProgramData%\ssh\ssh_known_hosts` e `ssh_known_hosts2` (Windows) | sim | não |

É a lista default do OpenSSH (`UserKnownHostsFile` e `GlobalKnownHostsFile`). `~` é `dirs::home_dir()`, como no resto do projeto.

- Arquivo **ausente** é lista vazia.
- Arquivo que **existe e não pode ser lido** — permissão, codificação que não é texto (um `known_hosts` regravado em UTF-16 por um redirecionamento do PowerShell 5, por exemplo) — **encerra a conexão** com `Failed { cause: KnownHostsUnreadable }`, sem pergunta e sem gravação. Tratá-lo como vazio transformaria um host conhecido, ou um host com a chave alterada, em "desconhecido", e o usuário aceitaria uma chave por cima de uma verificação que não aconteceu; e o acréscimo iria para um arquivo que não foi lido. A nota diz qual arquivo e por quê. Vale para os três arquivos.

O arquivo é lido **a cada conexão**, sem cache: ele muda por fora (`ssh-keygen -R`), e uma conexão a cada tanto não justifica um *watcher*.

### 2. Casamento

Para o destino `(host, porta)` do perfil:

- **Nome procurado**: `host` em minúsculas se `porta = 22`; `[host]:porta` caso contrário — o formato do OpenSSH.
- **Linha com padrões**: lista separada por vírgula; `*` e `?` como curinga; `!padrão` nega e, se casar, **exclui a linha**, mesmo que outro padrão dela case — a semântica do OpenSSH.
- **Linha em hash** (`|1|sal|hash`): `HMAC-SHA1(chave = sal, mensagem = nome procurado)`, comparado em tempo constante. `hmac` e `sha1` devem vir da pilha RustCrypto que o `russh` já traz — **a confirmar com `cargo tree` no PR da dependência** —, declarados em `porecatu-ssh` na versão da árvore.
- **Só o nome do perfil é procurado**, não o IP resolvido — o default do OpenSSH desde a 8.5 (`CheckHostIP no`).

O resultado é um enum, sem prosa:

```rust
pub struct HostKeyCheck {
    pub status: HostKeyStatus,
    pub skipped_lines: Vec<(PathBuf, usize)>,     // linhas que o leitor não entendeu
}
pub enum HostKeyStatus {
    Known,                                        // alguma linha casa com a mesma chave
    Unknown { other_types: Vec<KeyAlgorithm> },   // nenhuma linha casa com este tipo
    Changed { expected: Vec<KnownEntry> },        // linhas do mesmo tipo, nenhuma com esta chave
    Revoked { entry: KnownEntry },                // @revoked casa com a chave apresentada
}
pub struct KnownEntry { pub file: PathBuf, pub line: usize, pub fingerprint: Fingerprint }
```

Ordem de decisão:

1. **`Revoked`** vence tudo.
2. **`Known`** se **alguma** linha do host casa com a chave apresentada — inclusive quando outras linhas do mesmo tipo têm chave diferente. É o caso de quem rotacionou a chave e manteve a entrada antiga, e o OpenSSH aceita.
3. **`Changed`** só se há linhas do mesmo tipo para o host e **nenhuma** casa.
4. **`Unknown`** no resto.

Linha `@cert-authority` **não entra no casamento** (RF-20.18): o host coberto só por ela é `Unknown`. Linha que o `ssh-key` não consegue ler é **pulada**, não derruba o arquivo — o OpenSSH faz o mesmo, e é isso que o `russh` não faz —, mas **é contada**: com `skipped_lines` não vazio e resultado `Unknown`, o diálogo de host desconhecido mostra a linha de aviso "N linhas do `known_hosts` não puderam ser lidas" ([ADR-0075](0075-superficies-de-ui-do-ssh.md) §3). Uma linha `@revoked` ilegível não pode desaparecer em silêncio.

### 3. Tipo de chave

Antes de conectar, o algoritmo de chave do host preferido na negociação é **o que já está registrado** para aquele host, quando houver — o que o OpenSSH faz para evitar o falso "chave mudou" quando o servidor oferece vários tipos. Pela configuração de preferência do `russh`.

Se mesmo assim o servidor apresenta um tipo sem entrada, mas o host tem entrada de **outro** tipo, o resultado é `Unknown { other_types }`: o diálogo de host desconhecido abre com uma linha a mais dizendo que o host já tem chave registrada de outro tipo. Não é `Changed` — não há chave do mesmo tipo para divergir —, mas também não é um host nunca visto, e o usuário precisa saber.

### 4. Impressão digital

`SHA256:` seguido do SHA-256 da chave pública em base64 **sem preenchimento** — o formato de `ssh-keygen -lf` e do OpenSSH desde a 6.8 —, mais o nome do algoritmo (`ssh-ed25519`, `ecdsa-sha2-nistp256`, `rsa-sha2-512`). É dado, não frase: vai ao diálogo e à nota como veio, sem passar pelo catálogo de idiomas. Sem *randomart*.

### 5. Gravar: só acréscimo, no formato que o arquivo já usa

Aceitar um host desconhecido (RF-20.16) acrescenta **uma linha** ao fim de `~/.ssh/known_hosts`:

- **Em hash se o arquivo já tem alguma linha em hash**, sem hash caso contrário. É a forma de respeitar o `HashKnownHosts` do usuário sem ler a configuração do OpenSSH: quem tem o arquivo em hash ligou isso de propósito.
- Garante a quebra de linha antes, se o arquivo não termina em uma.
- **Recusa gravar** se o nome do host ou a chave serializada contêm `\r`, `\n` ou espaço onde o formato não admite — defesa em profundidade sobre a validação de `host` na config ([ADR-0072](0072-cliente-ssh-embutido.md) §6): uma linha injetada no `known_hosts` do usuário seria uma entrada de confiança para o OpenSSH também.
- Abre em modo de acréscimo e escreve a linha **numa única chamada** — o mesmo que o `ssh` faz, e o que convive com um `ssh` gravando ao mesmo tempo. **Sem** gravação atômica por renomeação: renomear por cima de um arquivo que outro programa pode estar acrescentando perderia a linha dele.
- Diretório `~/.ssh` ausente é criado; no Linux e no macOS, com permissão `0700`, e o arquivo novo com `0600`, como o OpenSSH exige dos dois.

**Nunca** remove, reescreve ou reordena linha. Chave alterada (RF-20.17) **não** oferece "substituir": a nota diz o arquivo e a linha da entrada divergente, e o usuário resolve com o editor ou com `ssh-keygen -R` — que é o mesmo caminho que o OpenSSH manda seguir, pela mesma razão: essa é a hora em que alguém pode estar no meio da conexão.

### 6. Onde acontece

A verificação roda **dentro do callback de chave do servidor** do `russh`, na tarefa SSH, antes de qualquer autenticação — e a leitura do arquivo em `spawn_blocking`. `Unknown` vira uma pergunta pelo canal do [ADR-0071](0071-backend-de-terminal.md) §5 e espera a resposta; `Changed` e `Revoked` encerram com `Failed { cause: HostKeyChanged | HostKeyRevoked }` e os dados da nota; arquivo ilegível encerra com `Failed { cause: KnownHostsUnreadable }` (§1). A reconexão automática ([ADR-0076](0076-reconexao-automatica.md)) passa pelo mesmo caminho a cada tentativa, e por isso uma chave que muda entre a queda e a volta bloqueia (RF-20.30).

## Alternativas consideradas

### Usar `russh::keys::known_hosts` como está

Zero código nosso. Recusada: sem `@revoked` (um requisito de bloqueio), sem curinga, e com o risco de um único marcador tornar o arquivo inteiro ilegível — que, num arquivo compartilhado com o OpenSSH, é o tipo de falha que transforma host conhecido em "desconhecido" e treina o usuário a aceitar.

### Arquivo próprio do Porecatu

Isolado do OpenSSH, sem risco de pisar no arquivo dele. Recusada pelo dono: o usuário teria de reconfirmar todo host que já confia, e um host confirmado num lado não estaria no outro.

### Ler os dois, gravar só no próprio

Meio-termo. Recusada pelo mesmo motivo, mais um: chave alterada no OpenSSH e não no nosso daria respostas diferentes para o mesmo servidor.

### Oferecer "substituir a chave" no bloqueio

Mais conveniente. Recusada pelo RF-20.17: o botão que resolve o caso legítimo é o mesmo que entrega a sessão no ataque, e o usuário não tem como distinguir um do outro de dentro do diálogo.

### Gravar sempre sem hash

O que o `russh` faz. Recusada: num arquivo que o usuário mantém em hash, uma linha em texto claro expõe a lista de hosts que o hash existia para esconder.

## Consequências

### Positivas

- Host confiado no OpenSSH é confiado no Porecatu, e vice-versa — a métrica de "zero reconfirmações" do PRD-020.
- O comportamento em linha estranha, chave alterada e entrada revogada é o do OpenSSH, que é o que o usuário conhece.
- O arquivo do usuário só ganha linhas, no formato que ele já usa.

### Negativas

- Casamento de padrões e de hash é código nosso, num lugar sensível.
- Ler o arquivo a cada conexão custa alguns milissegundos fora da main thread por conexão.
- Certificados de host continuam não suportados; quem depende só de `@cert-authority` vê o diálogo de host desconhecido.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Casamento próprio aceitar chave que o OpenSSH recusaria | Baixa | **Alto** | Testes com um `known_hosts` de prova gerado pelo próprio `ssh-keygen` (hash, `[host]:porta`, curinga, negação, `@revoked`, `@cert-authority`), comparando com a resposta do `ssh-keygen -F` |
| Comparação de hash vazar por tempo | Baixa | Baixo | Comparação em tempo constante (`subtle`, que já vem com a pilha) |
| Corromper o arquivo do usuário | Baixa | **Alto** | §5: só acréscimo numa chamada, quebra de linha garantida, nunca renomeação; teste de acréscimo sobre arquivo sem quebra final |
| Permissão errada no `~/.ssh` criado fazer o OpenSSH recusar o diretório | Baixa | Médio | `0700`/`0600` no Unix, com teste |
| Linha que o `ssh-key` não lê esconder uma entrada `@revoked` | Baixa | Médio | Linha ilegível é pulada como no OpenSSH, mas vai em `skipped_lines`, e o diálogo de host desconhecido avisa quantas foram (§2) |
| Host rotacionado com entrada antiga mantida ser bloqueado como "chave mudou" | Média | Médio | §2: `Known` antes de `Changed`; teste com duas linhas do mesmo tipo, uma casando |
| `known_hosts` ilegível virar "vazio" e o usuário aceitar uma chave sem verificação | Baixa | **Alto** | §1: arquivo existente e ilegível encerra a conexão, sem pergunta e sem gravação |
| Linha injetada no `known_hosts` por um `host` malformado | Baixa | **Alto** | Validação de `host` na config ([ADR-0072](0072-cliente-ssh-embutido.md) §6) e recusa de `\r`/`\n` na gravação (§5) |
