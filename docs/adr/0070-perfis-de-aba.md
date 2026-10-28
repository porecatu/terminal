# ADR-0070 — Perfis de aba: lista em `[profiles]`, `[shell]` como perfil padrão implícito, identificador no painel, menu no clique direito do `+`

**Status:** Aceito — aval visual pendente
**Data:** 2026-10-27
**Relacionados:** [ADR-0004](0004-pty-cross-platform.md), [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0036](0036-formato-do-arquivo-de-sessao.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0051](0051-arquivo-de-projeto-porecatu.md), [ADR-0053](0053-paineis-divididos.md), [ADR-0054](0054-sessoes-nomeadas.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0067](0067-temas-em-arquivos-proprios.md), [PRD-007](../prd/prd-007-perfis-de-aba.md), PRD-001, PRD-003, PRD-004, PRD-006
**Supersedes:** ADR-0036 §3 (**parcial**: um campo opcional `profile` em `TabV1` e em `PaneV1`, sem subir a versão; a restauração passa a lançar o perfil gravado em vez de sempre o `[shell]`) · ADR-0014, "Anatomia — menu de contexto" (**parcial**: um menu ancorado num botão, não no cursor)

> **Aceito com o aval visual pendente**, como o [ADR-0068](0068-efeito-acrilico-no-fundo-do-terminal.md): o comportamento está decidido; a aparência do badge na aba e do menu de perfis (§6 e §7) passa pelo aval do dono do produto no PR que a implementa, e o que ele mudar entra como revisão desta seção.

## Contexto

O [PRD-007](../prd/prd-007-perfis-de-aba.md) pede que o usuário escolha **o que** uma aba nova roda — PowerShell, WSL, `cmd`, um `zsh` de login e, depois, uma conexão SSH do cliente embutido — e que a barra mostre essa escolha por um badge. O dono decidiu cinco coisas que delimitam a forma: o clique esquerdo no `+` não muda; o menu abre pelo clique direito; o atalho é por posição, com nove ações fechadas; não há cor por perfil; a detecção roda sob demanda.

O que existe hoje e condiciona a decisão:

- **O shell é montado em dois lugares e escolhido em muitos.** `spawn_tab_runtime` e `spawn_split_pane_runtime` (`crates/porecatu-ui/src/lib.rs`) recebem um `&Shell` e montam o `SpawnConfig` com ele — `program` vazio cai em `porecatu_pty::resolve_default_shell`, que procura `pwsh.exe`, depois `powershell.exe`, depois `cmd.exe` no Windows, e usa `$SHELL` no Unix. Quem escolhe **qual** `Shell` passar são os chamadores: cinco chamadas diretas das duas funções, alimentadas por catorze leituras de `config.shell` em `lib.rs` (contagem de 2026-10-27).
- **O shell já é do painel.** Desde o [ADR-0053](0053-paineis-divididos.md), `shell_name` é propriedade do `Pane`, e a aba a deriva do focado.
- **A sessão grava o nome do shell e o ignora.** `TabV1.spawn_program` e o campo de mesmo nome em `PaneV1` (a folha da árvore, `PaneNodeV1::Leaf`) guardam `Pane::shell_name()`, mas a restauração sempre lança o `[shell]` corrente — um comentário em `spawn_tab_runtime` registra que a restauração ignora o campo e que os dois podem divergir. O nome, sozinho, nem bastaria para relançar: não há argumentos nem ambiente gravados.
- **`show_profile_badge` já existe** em `[appearance.tabs]` como reserva de nome (`crates/porecatu-config/src/appearance/tabs.rs`), e `badge_tint_strength = 0.14` já existe em `[appearance.groups]`, com consumidor no realce de arraste.
- **O clique secundário no `+` não faz nada hoje.** `handle_bar_click` (`crates/porecatu-ui/src/lib.rs`) abre o menu da aba sobre uma aba e o do grupo sobre a pílula, e ignora `GroupNewTab`/`UngroupedNewTab`. O gesto está livre — e "secundário" já inclui `Ctrl`+clique no macOS (`is_secondary_bar_click`, [ADR-0021](0021-selecao-multipla-e-gestos-da-barra.md) §3).
- **O catálogo de ações registra `profile.*` como ausente** ("perfis de aba são v2").
- **Cor só no tema** desde o [ADR-0067](0067-temas-em-arquivos-proprios.md).

Cinco perguntas ficam em aberto, e cada uma tem uma resposta errada tentadora:

1. **Onde o `[shell]` fica** quando há perfis — a tentação é deprecá-lo em favor de um perfil obrigatório, e quebrar todo arquivo de config existente.
2. **O que a sessão grava** — a tentação é gravar o comando inteiro, e restaurar um programa que o usuário já tirou da config.
3. **Como um perfil detectado volta numa restauração** — a tentação é rodar a detecção no arranque, que é exatamente o custo que o PRD proíbe.
4. **Que widget o menu é** — a tentação é um oitavo widget de chrome para o que é um menu de contexto com uma coluna a mais.
5. **Onde o tipo `ssh` entra** — a tentação é desenhá-lo aqui, antes das decisões de conexão que ainda não existem.

## Decisão

**Perfis são uma lista `[[profiles.list]]` no `porecatu.toml`, com `[shell]` como perfil padrão implícito quando `[profiles] default` não aponta outro. O perfil é do painel e é gravado na sessão pelo identificador; perfis detectados têm identificador determinístico, que se relança sem detectar. O menu é o menu de contexto do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) com uma coluna de badge, aberto pelo clique direito no `+` ou por `profile.open_menu`; o atalho é `profile.new_tab_1` a `_9`. O tipo `ssh` é só reservado.**

### 1. Forma na config

```toml
[profiles]
# Identificador do perfil usado pelo "+", tab.new e painéis novos.
# Vazio = o próprio [shell].
default = ""
# Procura shells instalados ao abrir o menu de perfis pela primeira vez.
detect = true

[[profiles.list]]
id = "wsl"
name = "Ubuntu 24.04"
badge = "WSL"
program = "wsl.exe"
args = ["-d", "Ubuntu"]
# cwd = "~/projetos"        # opcional; vazio = herda, como tab.new
# [profiles.list.env]       # opcional, mescla sobre [shell.env]
```

- **`[profiles]` é uma tabela e `list` é a lista** — TOML não permite `[profiles]` e `[[profiles]]` ao mesmo tempo, e as duas chaves de controle precisam de casa.
- **`id`** casa `^[a-z0-9][a-z0-9_-]{0,31}$`. É o que a sessão grava (§4) e não é exibido. Prefixo `auto:` é reservado aos detectados (§5) e recusado na config.
- **`badge`**: de 1 a 4 escalares Unicode, exibido em maiúsculas como escrito. O teto é o que cabe no badge de 9px sem morder o rótulo além do que o índice (`show_index`, removido depois) morderia.
- **`type`**: `"local"` (default, omitido no exemplo) ou `"ssh"`. Perfil `"ssh"` é **recusado com erro de config** até o PRD de conexões SSH ser aprovado e implementado — reservar o valor agora impede que um perfil `local` com `program = "ssh"` seja confundido com ele depois, e recusar impede que alguém escreva uma config que vai mudar de significado.
- **`name`** é obrigatório e não pode ser vazio depois de aparado; é texto livre, exibido no menu.
- **`env`** é mesclado **sobre** `[shell.env]`, que continua valendo para todo perfil `local`: é o lugar onde o usuário já põe `EDITOR` e afins. **Não é lugar de segredo**, pela mesma razão que `[shell.env]` não é: o `porecatu.toml` é um arquivo de texto que o usuário versiona. O app não grava `env` em lugar nenhum além do próprio arquivo — nem na sessão (§4) —, e o comentário da chave no arquivo de exemplo diz isso.
- **Sem `color` e sem `icon`**, pelas razões do PRD-007 ("O que este documento faz com o desenho").

> **Revisto pelo [ADR-0072](0072-cliente-ssh-embutido.md) §6.** `type = "ssh"` deixa de ser recusado quando o cliente embutido estiver implementado, com os campos `host`, `port`, `user`, `auth` e `identity_file`, e sem `program`, `args`, `env` e `cwd`. O badge default de um perfil `ssh` é `SSH`. Até a implementação, a recusa decidida acima continua.

Validação (RF-7.5) em `porecatu-config`: identificador inválido, repetido ou com `auto:`, `name` vazio, `program` vazio, `badge` vazio ou longo demais, `type` desconhecido ou `"ssh"` — erro de config **naquele item**, que é descartado; os demais valem. `default` apontando para identificador inexistente é erro e volta a `""`.

### 2. `[shell]` é o perfil padrão implícito

Com `default = ""`, o perfil padrão é **o próprio `[shell]`**, com o identificador interno `""` e sem badge. Nada muda para quem não declarou perfil: o mesmo `SpawnConfig`, o mesmo `resolve_default_shell`, o mesmo `[shell.env]`.

Com `default = "wsl"`, o perfil `wsl` passa a ser o do `+`, de `tab.new`, `group.new_tab`, `window.new` e das abas da primeira janela de um arranque sem sessão. O `[shell]` continua sendo o que roda numa aba restaurada sem `profile` (§4), e continua no menu (§6).

**Dividir um painel nunca consulta o padrão**: o painel novo herda o perfil do painel de origem (RF-7.15), e um painel do `[shell]` gera outro painel do `[shell]`, qualquer que seja o `default` naquele momento. Dividir é "mais um destes", não "mais uma aba".

**O `[shell]` não é deprecado.** Ele já é, na prática, um perfil — o único. Torná-lo implícito custa uma linha de `if` e não quebra nenhum arquivo.

### 3. Resolver um perfil é uma função pura de `porecatu-config`

```rust
pub enum ProfileRef<'a> { Default, Id(&'a str) }

pub struct ResolvedProfile {
    pub id: String,          // "" para o [shell]
    pub badge: Option<String>,
    pub program: Option<String>,   // None = resolve_default_shell
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

pub fn resolve_profile(config: &Config, r: ProfileRef) -> Result<ResolvedProfile, MissingProfile>;
```

`spawn_tab_runtime` e `spawn_split_pane_runtime` passam a receber um `ResolvedProfile` em vez de um `&Shell`; o `SpawnConfig` sai dele. Os caminhos que hoje leem `config.shell` para chegar a elas passam a resolver o perfil certo para o caso deles — o padrão, o do painel de origem, o gravado na sessão, o escolhido no menu —, e nenhum deles continua lendo `config.shell` para spawnar. A mudança é mais larga que as duas funções, e a regra que a mantém honesta é essa última: quem quer saber o que lançar pergunta a `resolve_profile`.

Identificadores `auto:` (§5) são resolvidos pela mesma função, sem rodar a detecção completa, e **só se passarem a validação do §5** — o identificador vem de um arquivo de sessão, e arquivo de sessão não é fonte de confiança para escolher executável.

### 4. Sessão: `profile` opcional no painel, sem subir a versão

`TabV1` (aba de um painel só) e `PaneV1` (a folha da árvore, `PaneNodeV1::Leaf`) ganham:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
profile: Option<String>,
```

Com `panes` presente, vale o `profile` de cada `PaneV1`, e `TabV1.profile` **não é gravado** — o mesmo par que `spawn_program` já forma entre aba e painel. Painel do `[shell]` grava o campo **ausente**, nunca `""`: o identificador interno do §2 não sai do processo.

`CURRENT_SCHEMA_VERSION` continua 1, pela regra do [ADR-0036](0036-formato-do-arquivo-de-sessao.md) §1 que o [ADR-0053](0053-paineis-divididos.md) §11 e o [ADR-0054](0054-sessoes-nomeadas.md) §4 já usaram: campo ausente tem significado definido.

Na restauração:

| `profile` gravado | O que sobe |
|---|---|
| ausente (sessão antiga, ou painel do `[shell]`) | o `[shell]` corrente — **o comportamento de hoje, intacto**, mesmo que `default` tenha passado a apontar outro perfil: o painel rodava o `[shell]`, e é o `[shell]` que volta |
| identificador que existe | aquele perfil |
| `auto:…` que passa a validação do §5 | o perfil derivado do identificador, sem rodar a detecção completa |
| identificador que não existe mais, ou `auto:…` recusado | o perfil padrão, com **nota no grid** (`note.profile_missing`, com o identificador), como o `cwd` que sumiu (RF-3.10) |

Gravar o identificador, e não o comando, é o que faz "mudei o `args` do perfil" valer na próxima restauração — e o que impede a sessão de escolher um executável que a config não declara nem a máquina oferece. `spawn_program` continua gravado e continua sendo só nome de exibição.

O `.porecatu` ([ADR-0051](0051-arquivo-de-projeto-porecatu.md)) recebe o nome do shell **do perfil lançado** — o mesmo cuidado que o comentário de `spawn_tab_runtime` já toma com o `[shell]`: nunca o nome gravado.

A aba `NotStarted` ([ADR-0037](0037-aba-nao-iniciada.md)) guarda o `profile` do painel e o resolve **no primeiro foco**, não na restauração — o perfil pode ter sido editado entre as duas coisas, e quem vale é a config do momento do spawn.

### 5. Detecção: sob demanda, em thread, com identificador determinístico

A primeira abertura do menu num processo, com `detect = true`, dispara **uma** thread. O resultado volta ao event loop como **dado**, pelo `EventLoopProxy` que os wakeups já usam — thread que devolve dado, nunca comando, a regra que o [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md) fixou para o poll do Git —, fica guardado em `App` até o fim do processo e pede redesenho do menu se ainda estiver aberto.

| Plataforma | Fonte | Identificador | Lança |
|---|---|---|---|
| Windows | `pwsh.exe`, `powershell.exe`, `cmd.exe` no `PATH` (`porecatu_pty::search_path`) | `auto:shell:pwsh.exe` | o executável, sem argumentos |
| Windows | `wsl.exe --list --quiet` | `auto:wsl:Ubuntu-24.04` | `wsl.exe -d Ubuntu-24.04` |
| Linux, macOS | `/etc/shells`, só caminhos que existem no disco | `auto:shell:/bin/zsh` | o caminho, sem argumentos |

- **`wsl.exe --list --quiet` escreve UTF-16LE**, não UTF-8, com quebra `\r\n`. É decodificado como tal; linha vazia é descartada.
- **`wsl.exe` é lançado com `CREATE_NO_WINDOW`**, como o `git` em `crates/porecatu-ui/src/git.rs`, senão pisca um console a cada detecção.
- **Teto de 3 s** para a thread inteira. `wsl.exe` com o serviço parado pode demorar a responder; passado o teto, o processo é morto e a lista de WSL fica vazia. Falha e teto **não viram aviso** (RF-7.18): o menu é útil sem eles.
- Detectado com o mesmo `program` e `args` de um perfil declarado não aparece (RF-7.9).
- Badge do detectado: `PS` para `pwsh`/`powershell`, `CMD`, `WSL`, e para os de `/etc/shells` as três primeiras letras do nome do executável em maiúsculas (`ZSH`, `BAS`, `FIS`). Nome exibido: o nome da distribuição para WSL, o nome do executável para os demais. Esses textos são dado, não frase de interface, e não passam pelo catálogo do [ADR-0056](0056-catalogo-de-textos-da-interface.md).

O identificador `auto:` é o que fecha o RF-7.19, e é também uma porta: ele vem de um arquivo de sessão, que pode ter sido copiado de outra máquina ou editado à mão. Por isso **a restauração valida o identificador contra o que a detecção aceitaria**, sem a parte lenta dela:

- **`auto:shell:<x>` no Windows** só vale se `<x>` é um dos três nomes fixos (`pwsh.exe`, `powershell.exe`, `cmd.exe`) — nunca um caminho —, e é resolvido pelo `PATH`, como na detecção.
- **`auto:shell:<x>` no Linux e no macOS** só vale se `<x>` é um caminho listado no `/etc/shells` **no momento da restauração** — a leitura de um arquivo pequeno, não a detecção.
- **`auto:wsl:<distro>`** só vale se `<distro>` casa `^[A-Za-z0-9._-]{1,64}$`; o programa é sempre `wsl.exe`, e o nome vai como argumento separado depois de `-d`, nunca interpolado numa linha de comando. Não se confere se a distribuição existe — isso pediria o `wsl.exe --list`, que é exatamente a parte lenta. Se não existe, quem diz é o próprio `wsl.exe`, no grid, e o processo sai com código ≠ 0 e a aba fica aberta (RF-1.3).

Identificador que não passa cai no perfil padrão, com a nota do §4. O que a sessão consegue pedir, no pior caso, é um dos shells que a máquina já oferece ou uma distribuição WSL pelo nome — nunca um executável arbitrário.

### 6. Menu: o menu de contexto, com uma coluna de badge

O menu de perfis **não é um widget novo**. É o menu de contexto do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) — mesma camada, mesma navegação por setas, mesmo `Esc`, mesmo flip nos dois eixos, mesmos tokens de item, hover, divisor e sombra. A volta é literal: o ADR-0014 tirou os tokens do menu de contexto **do menu de perfis da §2.9**, que ainda não existia. Há duas diferenças:

- **ancorado no `+`**, abaixo dele (acima com `tab_bar_position = "bottom"`), em vez de no cursor; pela ação, no `+` do grupo da aba ativa, ou abaixo da aba ativa quando o botão está desligado;
- **cada linha leva o badge à esquerda do rótulo** e, quando há atalho, a tecla à direita, pelo `Chord::label` que o popover de sessões já usa.

O badge do menu tem a mesma anatomia do badge da aba (§7), na cor do grupo de destino. O rótulo "Perfis" em caixa alta da §2.9 **não entra**: o menu de contexto não tem cabeçalho, e um menu de perfis aberto no `+` não precisa dizer o que é.

**Linhas, nesta ordem** (RF-7.9):

1. o **perfil padrão**;
2. os perfis de `[[profiles.list]]`, na ordem do arquivo, **sem repetir** o padrão;
3. o **`[shell]`**, quando ele não é o padrão — é o que mantém o shell de sempre alcançável depois de `default` apontar outro perfil;
4. divisor e os **detectados** que não repetem uma linha acima pelo mesmo `program` e `args`;
5. divisor e **"Novo grupo de abas"**, que executa `group.create` — o item que a §2.9 desenha no fim do menu, e que só existe neste menu.

A linha do `[shell]` mostra como nome o nome do shell resolvido (`pwsh`, `zsh`), o mesmo que a aba mostra hoje, e **não tem badge** — coerente com a aba, que não mostra badge para ele; a coluna do badge fica vazia e o nome alinha com os demais.

Os detectados ainda não prontos não têm linha de "carregando": a seção aparece quando chega. O realce é preservado pelo **perfil** realçado, não pela posição, para que uma seção que chega não mova o que o usuário está prestes a escolher. A regra do ADR-0014 de que item indisponível fica esmaecido, nunca ausente, é sobre **ações** que dependem de contexto; as linhas deste menu são **dados** — a lista de perfis da máquina —, como as linhas do popover de sessões, e uma lista de dados tem o tamanho dos dados.

### 7. Badge na aba

Anatomia da §2.5 item 1, que já está na especificação: mono 9px/500, raio 3, `padding: 2px 4px`, `letter-spacing: .4px`, texto na cor do grupo, fundo na cor do grupo com o alfa de `[appearance.groups] badge_tint_strength` (0.14). **Nenhum valor novo.**

- Entra **à esquerda do rótulo**, com o `gap: 8` da aba, e **consome largura do rótulo**, como o prefixo de `show_index` (§2.5, opção removida depois). Com o ponto de atividade ou de campainha (§2.17, que já reservou "a posição que o badge ocupará"), a ordem é **badge, ponto, rótulo**, com o mesmo `gap` entre os três: o badge é o dado estável da aba e fica na ponta; o ponto vai e vem, e fica colado no rótulo que ele qualifica. A aba continua com 229px: a largura fixa é o que impede a barra de refluir quando o título muda, e o badge não pode ser exceção.
- Aparece **só em painel cujo perfil não é o padrão** (RF-7.13), e a aba deriva do painel focado.
- `[appearance.tabs] show_profile_badge` deixa de ser reserva de nome e passa a ter consumidor. Classe A.
- Na árvore de acessibilidade ([ADR-0043](0043-arvore-de-acessibilidade.md)), o nome do perfil entra na descrição do nó da aba, não no nome — o nome continua sendo o título.

### 8. Ações

| Ação | O que faz | Default |
|---|---|---|
| `profile.open_menu` | Abre o menu de perfis (§6) com a primeira linha realçada | **nenhum** — precedente de `session.open_list` e `pane.close` |
| `profile.new_tab_1` … `profile.new_tab_9` | Cria aba com o N-ésimo perfil de `[[profiles.list]]`, no grupo da aba ativa | **nenhum** |

As nove entradas são explícitas, como `tab.goto_1` … `tab.goto_9`, e `profile.new_tab_10` é erro de config ([ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md)). **Nenhuma nasce com tecla**: a família óbvia, `Alt+<número>` (`Cmd+<número>` no macOS), já é `tab.goto_N`, e `Ctrl+Shift+<número>` muda de tecla física conforme o layout de teclado (`Shift+1` é `!` no ABNT2 e no US, e outra coisa no AZERTY). A escolha é do usuário. O índice é sobre a **lista declarada**, nunca sobre os detectados: um atalho não pode mudar de alvo porque o WSL ganhou uma distribuição.

O clique direito no `+` e a escolha de uma linha do menu entram na tabela de "superfícies de mouse e de modal, que não são ações" do catálogo, como o clique numa sessão nomeada.

### 9. Recarga

`[profiles]` inteiro entra **na mesma linha da classe C que `[shell] program`/`args`/`env`** no [ADR-0030](0030-escopo-do-hot-reload.md) — a linha que aquele ADR já anota como *"não é 'exige reinício' — é escopo"*: nada é reiniciado, a mudança vale no próximo spawn, e o aviso diz "vale em aba nova" porque aba aberta tem processo já lançado. É o comportamento que o RF-7.6 pede. Os detectados não são refeitos por recarga — são da máquina, não da config.

### 10. Onde o código mora

| Crate | O que muda |
|---|---|
| `porecatu-config` | `[profiles]`, validação, `resolve_profile`, `show_profile_badge` com consumidor |
| `porecatu-core` | `Pane` ganha `profile: Option<String>`; `Action::ProfileOpenMenu`, `Action::ProfileNewTab(u8)` |
| `porecatu-session` | `profile` em `TabV1` e em `PaneV1`; conversão nos dois sentidos |
| `porecatu-ui` | spawn por `ResolvedProfile`; menu de perfis sobre o menu de contexto; badge; detecção; nota de perfil ausente; mensagens no catálogo |
| `porecatu-pty` | **nada** — `search_path` já é público |
| `porecatu-term`, `porecatu-render` | **nada** |

## Alternativas consideradas

### Deprecar `[shell]` e exigir um perfil

Um conceito só, sem caso especial. Recusada: quebra todo `porecatu.toml` existente para entregar o mesmo comportamento, e o caso especial custa uma linha.

### Gravar o comando inteiro na sessão

A aba volta com exatamente o que rodava, mesmo que a config tenha mudado. Recusada porque é o oposto do que o usuário espera de uma config: editar o perfil e não ver a mudança na próxima abertura. E grava na sessão o ambiente do perfil, que é onde alguém um dia vai pôr um token.

### Detectar no arranque, em thread

O menu abriria sempre completo. Recusada pelo dono: a armadilha de carga no arranque está registrada (CLAUDE.md, "Render e texto"), e `wsl.exe` é justamente o tipo de processo que acorda um serviço.

### Atalho declarado dentro do perfil (`key = "ctrl+shift+1"`)

Mais direto de escrever. Recusada pelo dono: seria uma segunda fonte de tecla fora de `[keybindings]`, sem a detecção de conflito do [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), e a tela de configurações teria dois lugares para capturar atalho.

### Clique no `+` sempre abre o menu, como no canvas

Fiel ao desenho. Recusada pelo dono: um gesto a mais em toda aba nova, para quem usa um perfil só — que é o usuário que o PRD-000 descreve.

### Widget próprio de menu de perfis

Rótulo de cabeçalho, largura fixa de 268px, sombra da §2.9. Recusada: o menu de contexto já tem navegação, camada, flip e acessibilidade, e um oitavo widget seria a mesma coisa com tokens divergentes. A largura do menu de contexto acompanha o rótulo mais longo entre 200 e 320px, truncando além disso (§2.16), que é o comportamento certo para nomes de perfil.

### Cor por perfil

O canvas tem. Recusada: cor só no tema ([ADR-0067](0067-temas-em-arquivos-proprios.md)), e uma cor por item de uma lista do usuário não tem como morar no tema.

### Desenhar o tipo `ssh` agora

Um perfil com `host`, `user`, `port`. Recusada: o que um perfil SSH guarda depende de decisões de cofre, de chave de host e de cadastro que são do PRD de conexões SSH. Aqui só se reserva o valor, recusado até lá.

## Consequências

### Positivas

- **Zero mudança para quem não declara perfil**, por construção: `[shell]` é o padrão, sessão antiga não tem `profile`, badge só aparece fora do padrão.
- A restauração passa a lançar o que a aba rodava — o `spawn_program` que a sessão grava desde a F5 e nunca usou finalmente tem um par que se relança.
- O menu herda navegação, acessibilidade e flip do menu de contexto.
- O tipo `ssh` tem endereço reservado, e o PRD de conexões SSH entra como um tipo a mais, não como um recurso paralelo.

### Negativas

- **Um campo opcional novo** em dois tipos do schema de sessão, e o ADR-0036 §3 parcialmente supersedido.
- Identificador `auto:` gravado na sessão relança sem saber se o programa ainda existe; quem diz que não existe é o próprio processo, no grid.
- O badge come até ~40px do rótulo, que trunca mais cedo nas abas de perfil não padrão.
- O canvas e o binário divergem em três pontos (clique, cabeçalho, cor) — registrados na §4.4 da especificação quando o binário mudar.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Um dos caminhos de spawn continuar lendo `config.shell` e ignorar o perfil | Média | **Alto** | §3: as duas funções de spawn recebem `ResolvedProfile` e deixam de aceitar `&Shell`, então o compilador acha cada chamador; teste por caminho (aba nova, divisão, restauração, `NotStarted`) conferindo o `SpawnConfig` |
| `wsl.exe` travar a thread de detecção | Média | Baixo | Teto de 3 s e processo morto; a detecção é de um processo só e nunca bloqueia o menu |
| Arquivo de sessão copiado ou editado lançar um executável arbitrário por `auto:` | Baixa | **Alto** | §5: `auto:` validado na restauração contra os três nomes fixos do Windows, o `/etc/shells` corrente e um padrão de nome de distribuição; recusado cai no padrão com nota; teste com `auto:shell:C:\x\evil.exe` e `auto:wsl:a b;c` |
| Saída do `wsl.exe` lida como UTF-8 e virar lixo | **Alta** se esquecido | Médio | §5, e teste com a saída UTF-16LE real capturada como fixture |
| Console piscando a cada detecção no Windows | Alta se esquecido | Baixo | `CREATE_NO_WINDOW`, como o `git.rs` |
| Perfil `ssh` escrito antes do cliente existir | Baixa | Médio | Recusado com erro de config até o PRD de conexões SSH |
| Realce do menu pular quando os detectados chegam | Média | Baixo | Realce guardado pelo perfil, não pelo índice (§6) |
| Badge ou menu entrarem no binário sem aval | Baixa | Médio | Aval visual no PR que implementa (etapa 5 do roadmap), registrado aqui como revisão |
