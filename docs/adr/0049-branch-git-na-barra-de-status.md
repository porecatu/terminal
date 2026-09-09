# ADR-0049 — Branch do Git na barra de status: `.git/HEAD` por `mtime`, sem estado da árvore

**Status:** Aceito
**Data:** 2026-09-09
**Relacionados:** ADR-0003, ADR-0007, ADR-0024, ADR-0034, ADR-0038, ADR-0039, ADR-0048, PRD-009
**Supersedes:** ADR-0048 §8 (**parcial**: só a linha que põe `git` na lista do que nunca entra)

## Contexto

O [ADR-0048](0048-barra-de-status.md) §8 listou "estado de repositório `git`" entre o que **não** entra na barra, e o [PRD-009](../prd/prd-009-barra-de-status.md) diz o mesmo em "Fora de escopo". A razão dada foi o **RF-9.8**: nada na barra pode mudar sozinho, porque isso quebraria a propriedade de "terminal ocioso custa zero frames" ([ADR-0007](0007-modelo-de-threading.md)).

A razão está certa e continua valendo — mas ela agrupou duas coisas de custo muito diferente sob a mesma palavra:

| | O que custa ler | Como se sabe que mudou |
|---|---|---|
| **Qual é a branch** | um arquivo de ~30 bytes (`.git/HEAD`) | o `mtime` desse arquivo |
| **Se a árvore está suja** | um `git status` completo | nada barato — só vigiando a árvore inteira |

O segundo é o que o RF-9.8 proíbe de fato: centenas de milissegundos numa árvore grande, thread própria, e nenhum evento barato que diga quando refazer. O primeiro é da ordem de um `stat`.

O dono do produto pediu os dois primeiros itens e **excluiu o estado explicitamente**. Esta decisão registra a fronteira, para que ela não seja atravessada por engano depois.

## Decisão

**A barra mostra um ícone de repositório e o nome da branch, lidos de `.git/HEAD` e revalidados por `mtime`. O estado da árvore fica fora, e continua fora.**

### 1. Sem thread, sem watcher, sem temporizador

A leitura acontece **no caminho que já monta o conteúdo da barra**, que só roda quando um frame vai ser desenhado. Terminal ocioso não desenha frame, logo não lê nada: a propriedade do ADR-0007 fica intacta, literalmente e não por aproximação.

Duas camadas de cache evitam que "por frame" signifique "trabalho por frame":

- **Descobrir o repositório** — subir de diretório em diretório procurando `.git` — só refaz quando o `cwd` da aba ativa muda. É o passo caro (vários `stat`), e é o que muda menos.
- **Reler `HEAD`** só quando o `mtime` dele mudou. O `stat` que verifica isso é da ordem de microssegundos; comparado ao que já acontece num frame, não aparece.

Um `notify::Watcher` em `.git` foi considerado e recusado — ver as alternativas. O ponto decisivo: um `stat` num arquivo é quatro ordens de grandeza mais barato que o `refresh_processes(All)` do `sysinfo` que o ADR-0048 §8 proíbe, e não é dessa classe de custo que o RF-9.8 fala.

### 2. O que se lê, e o que se faz com cada forma

`.git` pode ser um diretório (repositório comum) ou um **arquivo** com `gitdir: <caminho>` — é assim que worktree e submódulo aparecem. As duas formas são tratadas; ignorar a segunda mostraria "sem repositório" dentro de um worktree, que é pior que não mostrar nada.

O conteúdo de `HEAD` tem duas formas, e as duas aparecem no uso normal:

| `HEAD` contém | O que a barra mostra |
|---|---|
| `ref: refs/heads/<nome>` | `<nome>` |
| um SHA de 40 hex (detached) | os 7 primeiros, o mesmo corte que o `git` usa |

Nada mais é interpretado. Em particular, **não se resolve o ref**: saber para onde `refs/heads/main` aponta exigiria ler `packed-refs` ou outro arquivo, e o nome da branch já está na primeira linha.

### 3. Sem dependência nova

Ler um arquivo de 30 bytes e cortar um prefixo não justifica `git2`/libgit2 — uma dependência grande, com build de C, para usar um centésimo do que ela faz. O parsing é uma função pura de ~15 linhas, testável sem tocar disco.

Isso é o mesmo julgamento do [ADR-0038](0038-fallbacks-de-cwd.md), que preferiu o `sysinfo` já presente ao par `/proc` + `libproc` que o ADR-0005 nomeava: a dependência que não entra não tem versão para subir nem licença para conferir.

### 4. Anatomia: ícone `git-branch` mais o nome, um segmento só

Entra na zona esquerda, **depois do diretório e antes do grupo** — o repositório é uma propriedade do diretório, e ler os dois juntos é o que faz sentido.

- **Ícone**: `git-branch` do Lucide, `U+E0E2`, nomeado em `porecatu_render::icon` como os outros nove. A face embutida **não é subsetada** (2059 ícones), então nenhum recorte precisa ser refeito — a armadilha do "glyph que a fonte não tem não desenha e não avisa" não se aplica aqui, e o teste que varre `icon::ALL` cobre o resto.
- **Em do ícone**: `icon_em_size * 0.8`, o mesmo multiplicador que o botão de configurações já usa (§1.1). Não é escolha nova: a fonte da barra é 10.5px contra os 13px do rótulo de aba, e `10.5 / 13 ≈ 0.8`.
- **Folga entre ícone e nome**: nenhuma explícita. Toda glyph da face de ícones avança **1 em** e o desenho preenche ~0.6 dela, então sobram ~3px depois do desenho antes de o texto começar. Um `gap` aqui seria um valor inventado para resolver um problema que a métrica da fonte já resolve.
- **Cor**: a de base da barra, nos dois. O acento continua sendo só do shell — é ele que distingue a aba de relance (§2.8), e um segundo item colorido apagaria essa distinção.

### 5. O ícone é a resposta ao "estou num repositório?"

O requisito pedido são duas coisas, e o ícone é a primeira: **ele só aparece quando há repositório**. Fora de um, o segmento inteiro some — não há ícone apagado nem texto de "sem repositório", que ocupariam espaço para dizer o que a ausência já diz.

### 6. Herda o problema do OSC 7, e a marca dele

A branch é a do `cwd` que a barra conhece. Sem OSC 7, esse `cwd` é o de spawn — então a branch pode ser a de outro repositório, ou não haver ícone dentro de um. É o mesmo defeito do RF-9.4, e a mitigação é a que já existe: o diretório ao lado está no tom apagado, dizendo que o dado é o de origem.

**O segmento do Git não recebe marca própria.** Duas marcas para uma causa só seriam ruído: quem entende a do diretório entende que o que vem depois dele herda a mesma ressalva.

### 7. O estado da árvore continua fora

Não é "ainda não", é a fronteira desta decisão. Sujo/limpo, contagem de ahead/behind, arquivos em stage: todos precisam percorrer a árvore, todos custam ordens de grandeza mais, e nenhum tem um `mtime` único que diga "mudou". Entrar exigiria thread e um ADR que enfrente o RF-9.8 de verdade — não a extensão deste.

## Alternativas consideradas

### `notify::Watcher` em `.git/HEAD`

Era o caminho previsto quando o assunto foi levantado, e o crate já está no workspace pelo hot reload. Rejeitado por custo de ciclo de vida desproporcional ao ganho: o repositório muda quando o `cwd` da aba ativa muda, e quando o usuário troca de aba — então o watcher teria de ser derrubado e recriado nesses eventos, cada um com sua thread (`reload::watch` cria uma que roda para sempre, sem caminho de parada). O `mtime` entrega a mesma informação com um `stat` e zero threads.

### Reler `HEAD` a cada wakeup do PTY

Simples, e o evento existe. Rejeitado porque o wakeup do PTY é o evento mais quente do app — durante um `cargo build` são dezenas por segundo, e cada um viraria uma leitura de arquivo. O frame é o lugar certo justamente porque ele **já** é coalescido (ADR-0007).

### Reler só quando o `cwd` muda

Zero custo por frame, e cobre trocar de projeto. Rejeitado porque não cobre `git checkout`, que é precisamente a operação que muda a branch: o número na barra ficaria errado no caso de uso principal.

### `git2` (libgit2)

Resolveria branch, estado e tudo o mais, sem parsing à mão. Rejeitado pelo tamanho: uma dependência com build de C e superfície enorme, adotada para ler 30 bytes. E a licença (GPL2 com exceção de linking) exigiria a análise que o CLAUDE.md pede a cada crate novo, para um ganho que uma função pura de 15 linhas entrega.

### Rodar `git rev-parse --abbrev-ref HEAD`

É o que um prompt de shell faz. Rejeitado: um processo por consulta, dependência do `git` estar no `PATH`, e no Windows um `CreateProcess` de dezenas de milissegundos — dentro do caminho de frame, o pior lugar possível.

## Consequências

### Positivas

- A pergunta "em que branch eu estou?" é respondida sem `git status` no prompt, que é o que a maioria dos shells faz e o que custa caro em repositório grande.
- **Nenhuma dependência nova, nenhuma thread nova, nenhum temporizador.** O RF-9.8 continua verdadeiro no sentido literal: terminal ocioso não desenha frame, e sem frame nada é lido.
- **Nenhum valor de aparência novo**: o ícone sai da face que já está embutida, e a em dele é um multiplicador que o chrome já usa.
- Worktree e submódulo funcionam, porque a forma `gitdir:` é tratada.

### Negativas

- O ADR-0048 §8 e a lista de "Fora de escopo" do PRD-009 ficam **parcialmente supersedidos**, e a fronteira agora é mais fina: `git` deixou de ser uma palavra na lista e virou duas coisas, uma dentro e uma fora. É por isso que a §7 existe.
- Um `stat` por frame que não existia. Medido contra o que um frame já faz, não aparece — mas é trabalho novo no caminho quente, e é honesto registrá-lo.
- A branch pode estar errada sem OSC 7 (§6), sem marca própria dizendo isso.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Subida de diretório atrás de `.git` virar caro num caminho fundo | Baixa | Médio | Só refaz quando o `cwd` muda, e o resultado (inclusive "não há repositório") é cacheado; um caminho de 20 níveis é um `stat` por nível, uma vez |
| `mtime` de granularidade grosseira perder uma troca de branch no mesmo segundo | Baixa | Baixo | Dois `checkout` dentro da mesma granularidade de `mtime` é caso de teste automatizado, não de uso; o próximo evento que mexer no `HEAD` corrige |
| Ler `HEAD` no meio de uma escrita do `git` e ver conteúdo parcial | Baixa | Baixo | O `git` escreve `HEAD` por rename atômico; e o parser devolve `None` para conteúdo que não bate com nenhuma das duas formas, o que esconde o segmento por um frame em vez de mostrar lixo |
| Alguém estender para estado da árvore por parecer "só mais um campo" | Média | Alto | §7 é explícita, e a razão é de custo, não de gosto |
