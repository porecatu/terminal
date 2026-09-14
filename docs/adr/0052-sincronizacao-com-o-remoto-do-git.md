# ADR-0052 — Sincronização com o remoto do Git: poll em thread, contagem por `rev-list`, integração só por clique

**Status:** Aceito
**Data:** 2026-09-14
**Relacionados:** ADR-0003, ADR-0007, ADR-0014, ADR-0015, ADR-0018, ADR-0027, ADR-0030, ADR-0032, ADR-0033, ADR-0038, ADR-0042, ADR-0043, ADR-0048, ADR-0049, ADR-0051, PRD-000, PRD-009, PRD-013
**Supersedes:** ADR-0049 §7 (**parcial**: só ahead/behind; sujo/limpo e stage continuam fora) · ADR-0048 §5 (**parcial**: só a razão pela qual a barra cede a borda) · ADR-0048 §8 (**parcial**: só a regra "nada que mude sozinho", substituída por um critério mais estreito) · ADR-0048 §3 (**parcial**: só a linha "o nome do shell é o único item colorido") · ADR-0007, Consequências (**parcial**: só o "zero CPU" com a consulta ligada; a decisão de render damage-driven não muda)

## Contexto

O [ADR-0049](0049-branch-git-na-barra-de-status.md) entregou a branch na barra de status e parou numa linha que era quase um convite:

> Não é "ainda não", é a fronteira desta decisão. Sujo/limpo, contagem de ahead/behind, arquivos em stage: todos precisam percorrer a árvore, todos custam ordens de grandeza mais, e nenhum tem um `mtime` único que diga "mudou". Entrar exigiria thread e um ADR que enfrente o RF-9.8 de verdade — não a extensão deste.

Este é esse ADR. E a primeira coisa a fazer é notar que aquele parágrafo **agrupou três coisas sob uma razão que só vale para duas delas** — o mesmo erro, na mesma forma, que o próprio ADR-0049 apontou no ADR-0048 §8 quando a palavra "git" agrupava branch e estado da árvore:

| | O que custa descobrir | Toca a árvore de trabalho? |
|---|---|---|
| **Árvore suja / o que está em stage** | um `git status` completo, proporcional ao tamanho da árvore | **sim** — é o que ele faz |
| **Quantos commits atrás do remoto** | uma consulta de rede mais um `rev-list` sobre o grafo de commits | **não** — nem lê a árvore |

A contagem de ahead/behind **não percorre a árvore**. Ela é cara por outro motivo — fala com a rede —, e o custo de rede não é o que o RF-9.8 tinha em vista. O RF-9.8 defende a propriedade de que *terminal ocioso não desenha quadro*; uma consulta de rede a cada cinco minutos não desenha quadro nenhum quando o resultado é "nada novo".

Isso não é uma brecha. É uma distinção que muda o que está em jogo, e é honesto dizer o que continua em jogo mesmo com ela: **o app passa a fazer trabalho periódico com a máquina ociosa, e a fazer isso ligado por padrão.** O RF-9.8 é emendado, não contornado, e a §1 é onde isso é dito com todas as letras em vez de escondido atrás da tabela acima.

O dono do produto pediu quatro coisas, e a quarta é a que mais pesa no desenho:

1. Consultar o remoto periodicamente, com o intervalo em segundos na config e `0` desligando.
2. Mostrar na barra, ao lado da branch, quantos commits faltam.
3. **Não** atualizar automaticamente.
4. **Clicar** no indicador integra, em segundo plano.

A quarta atravessa uma decisão que o [ADR-0048](0048-barra-de-status.md) §5 tomou com uma justificativa aritmética explícita: a barra cedeu os 6px da borda inferior à zona de resize da janela *"porque no escopo aprovado ela não tem nenhum alvo clicável"*. A premissa deixa de valer. A boa notícia é que aquele mesmo ADR já tinha escrito a saída, na alternativa que recusou.

## Decisão

**O app consulta o remoto da branch da aba ativa num intervalo configurável, mostra a contagem como um segmento clicável da barra de status, e integra por `fast-forward` apenas — nunca sozinho. A consulta roda numa thread de vida curta, fora do caminho de frame, e o intervalo `0` devolve o app exatamente ao que ele era antes.**

### 1. O RF-9.8 é emendado, e este é o texto que substitui a regra

O [RF-9.8](../prd/prd-009-barra-de-status.md) diz que a barra *"não introduz timer nem redraw periódico"*, e o [ADR-0048](0048-barra-de-status.md) §8 o traduziu em **"nada que mude sozinho"**. Essa regra vai longe demais e de menos ao mesmo tempo: proíbe uma consulta de rede de cinco em cinco minutos e não teria como proibir um `git status` por frame, que é muito pior e não "muda sozinho".

O critério que a substitui tem três partes, e uma coisa só entra se passar nas três:

1. **Desligável numa linha de config**, e
2. **custa exatamente zero quando desligado** — nenhum processo, nenhuma thread, nenhum prazo agendado —, e
3. **não percorre a árvore de trabalho**.

O que a regra antiga proibia e a nova continua proibindo, item por item: **relógio** (reprova em 2 — um relógio desligado não tem sentido, e ligado desenha quadro a cada segundo sem que nada tenha mudado), **uso de CPU e memória** (mesma coisa), **contagem de processos da aba** (reprova em 3: a leitura é um `refresh_processes(All)` do `sysinfo`, que é a varredura que o ADR-0048 §3 proíbe nominalmente neste caminho), **árvore suja e stage** (reprova em 3). A lista do ADR-0048 §8 sobrevive inteira; só a formulação da regra muda.

O que **sobrevive literalmente**, e é importante que sobreviva, é o princípio 4 do [PRD-000](../prd/prd-000-visao-de-produto.md): *"nenhum frame renderizado sem mudança"*. A consulta marca sujeira **só quando a contagem muda**. Numa jornada típica ela roda dezenas de vezes e desenha quadro uma ou duas — o caso comum é "nada novo", e o caso comum não desenha nada. É o mesmo mecanismo do cursor piscando, que o [ADR-0007](0007-modelo-de-threading.md) já descreve como *"um timer que marca sujeira, não um loop"*.

O que **não** sobrevive literalmente é uma frase das consequências positivas do ADR-0007: *"terminal ocioso custa zero CPU e zero GPU"*. Com o poll ligado, o zero de CPU passa a ser zero-com-um-pico-a-cada-intervalo. O GPU continua zero. Registrar isto aqui é o ponto: a alternativa seria deixar a frase de pé num documento aceito enquanto o binário a desmente, que é exatamente o estado que o [ADR-0028](0028-o-binario-como-referencia-visual.md) existe para não permitir.

**O default é `300` segundos — ligado.** Foi decisão do dono do produto, e o argumento é o mesmo que ligou a barra de status por padrão (ADR-0048 §6): recurso que exige ser descoberto para ser ligado não é descoberto. O custo dessa escolha é real, está na primeira consequência negativa e na primeira linha da tabela de riscos, e a §4 explica por que ele não é da mesma natureza do custo que a allowlist do `.porecatu` contém.

### 2. O que fica de fora, e a razão reescrita

O [ADR-0049](0049-branch-git-na-barra-de-status.md) §7 dizia que o estado da árvore precisaria de "thread e um ADR". A thread agora existe, e é por isso que a razão precisa ser dita de novo, em base nova — senão a §7 inteira cai por arrasto na primeira vez que alguém disser "mas já temos uma thread".

**Sujo/limpo, contagem de arquivos modificados e o que está em stage continuam fora porque percorrem a árvore de trabalho.** Não porque precisariam de thread; a thread nunca foi a razão, foi uma consequência da razão. Um `git status` numa árvore grande custa centenas de milissegundos **por consulta**, cresce com o projeto, e não tem sinal barato que diga quando refazer — os três problemas continuam exatamente como o ADR-0049 os descreveu.

A contagem de commits não tem nenhum dos três: o `rev-list` percorre um intervalo do grafo de commits, não a árvore; o custo não cresce com o número de arquivos; e o sinal que diz quando refazer é o próprio intervalo, que o usuário escolhe.

### 3. Estado por repositório, num mapa do processo

O `GitInfo` do ADR-0049 vive em `WindowState` — uma instância por janela, sempre a do `cwd` que aquela barra exibe. **O estado remoto não pode morar lá**, e a razão não é estética:

- **`git fetch` escreve no repositório** (`FETCH_HEAD`, `refs/remotes/*`). Duas janelas com a aba ativa no mesmo projeto fariam dois `fetch` concorrentes no mesmo `.git`, com chance real de um falhar por lock. É a única das alternativas que está errada por mais do que gosto.
- **A chave é o repositório, não o `cwd`.** Um `cd src/` muda o `cwd` e não muda nada do que interessa; chavear por `cwd` refaria a consulta a cada navegação dentro do mesmo projeto. A descoberta do repositório já existe e já é cacheada (ADR-0049 §1).

Então o estado é um mapa no app inteiro, chaveado pelo repositório, com a contagem, a branch a que ela pertence, o instante da última consulta e o estado atual (parado, em voo, sem upstream, falhou).

**A consequência mais importante do mapa é que o problema de corrida desaparece por construção**, e vale escrever por quê. O resultado que chega da thread **não é um comando para mostrar alguma coisa** — é dado chaveado. Quem o recebe faz uma coisa só: guarda no mapa. Quem decide o que a barra mostra é o mesmo caminho de sempre, que a cada quadro calcula a chave do `cwd` da aba ativa e **consulta** o mapa. Se a chave não bate, não há segmento; e como o resultado ficou guardado, voltar para aquela aba mostra a contagem na hora, sem consulta nova.

Duas guardas a mais, ambas de graça:

- A entrada carrega **a branch a que a contagem pertence**, e branch diferente esconde o indicador (RF-13.10). Um `git checkout` é detectado pelo `mtime` de `.git/HEAD` que o ADR-0049 já vigia — nada novo precisa observar nada.
- Fechar janela não faz nada com o mapa: nenhuma limpeza, nenhum cancelamento. O resultado em voo chega, encontra um mapa que ninguém mais consulta, e morre ali.

O mapa cresce com os repositórios visitados na sessão. Um teto foi considerado e recusado: é código para um problema que não aparece em sessão nenhuma real, e a entrada é de dezenas de bytes.

### 4. Sem allowlist, e por que este caso é diferente do `.porecatu`

O [ADR-0051](0051-arquivo-de-projeto-porecatu.md) §4 exige que um diretório esteja declarado em `trusted_paths` antes de o app executar qualquer coisa do `.porecatu`, com a lista **vazia por default**. Aqui não há allowlist, e o recurso vem ligado. A diferença não é de rigor, é de natureza do que acontece:

| | `.porecatu` | Consulta ao remoto |
|---|---|---|
| O que roda | **comandos que o projeto declarou** — código de quem escreveu o arquivo | `git fetch`, sempre o mesmo, escolhido por nós |
| Quem controla o conteúdo | quem fez commit no repositório, possivelmente outra pessoa | ninguém: não há conteúdo a controlar |
| O que pode acontecer de pior | execução arbitrária na máquina do usuário | tráfego de rede e escrita em `refs/remotes` |

A allowlist existe para conter **execução de código de terceiros**. Não é o que acontece aqui, e aplicá-la assim mesmo trocaria um controle proporcional (uma chave que liga e desliga) por um desproporcional (autorizar repositório por repositório para uma operação somente-leitura), com a mesma dívida que o ADR-0051 registrou como consequência negativa: não há caminho de UI para autorizar, porque o app não escreve na config do usuário.

O que sobra de risco real está registrado e não é escondido: **o `fetch` escreve no `.git`** de repositórios que o usuário apenas abriu, e **fala com a rede sem ele pedir naquele momento**. Quem não quer isso escreve uma linha, e desligado o recurso custa zero.

A **integração**, essa, nunca é automática (RF-13.12), e é aí que mora a assimetria que o dono do produto pediu: consultar é barato de desfazer (nada a desfazer), integrar não é.

### 5. O relógio é da main thread; a thread faz uma execução e morre

O agendamento é `ControlFlow::WaitUntil`, encadeado no mesmo lugar em que os prazos de aviso, tooltip, animação, gravação de sessão, convite de integração de shell e comando de projeto já se encadeiam. É a terceira vez que o projeto diz isto e continua valendo: **temporizador de UI é sempre `WaitUntil`**, e estado com prazo recebe o instante de fora, nunca o consulta por conta própria.

Isso não é detalhe de implementação, é o que torna a emenda da §1 **verificável**: com `remote_poll_interval_secs = 0`, a função de prazo devolve "nenhum", nada entra na conta, o laço volta a dormir sem prazo — e isso vira um teste, não uma promessa. O item 2 do critério da §1 deixa de ser prosa.

A execução acontece numa thread **de vida curta, uma por consulta, detached, sem `join`** — a mesma disciplina do watcher de config e das três threads por terminal. Ela não vê o estado do app: recebe o que precisa, executa, manda o resultado pelo mesmo canal por onde a sujeira do PTY e o reload de config já chegam, e termina.

Uma thread de vida longa com canal de comando foi considerada e recusada nas alternativas: duplicaria o agendamento que o `WaitUntil` já faz, e pediria um caminho de parada que **nenhuma thread do projeto tem**.

**Sair do app com uma consulta em voo.** A thread é detached e não segura nada; ela morre quando o processo morre. O processo `git` filho, porém, **não** morre junto no Windows — não há herança de morte sem Job Object. Uma consulta disparada pouco antes do fechamento sobrevive alguns segundos como órfão, até o próprio `git` terminar ou o watchdog da §6 matá-lo — e o watchdog morre com o app, então o teto real é o `git` terminar sozinho.

Pôr o filho num Job Object, como o [ADR-0033](0033-job-object-encerramento-de-processo.md) faz com os shells, resolveria — e foi recusado: `porecatu-ui` não pode depender de `porecatu-pty` (é a regra de dependência da [arquitetura](../arquitetura.md)), e contorná-la significaria arrastar a infraestrutura de encerramento de árvore de processo para um comando somente-leitura que termina sozinho. **Aceito e registrado**, na tabela de riscos, em vez de resolvido de um jeito que custa mais que o problema.

### 6. O `git` do sistema, sem dependência nova — e ele não pode travar nem piscar

A consulta é o executável `git`, lançado como processo. É o **primeiro lançamento de processo do projeto fora do PTY** que não passa por um crate wrapper, e por isso vale dizer o que muda e o que não muda:

- **Não é o caso do `opener`.** Aquele crate entrou para encapsular chamada `unsafe` de plataforma, a mesma razão do `arboard`, do `win32job` e do `png`. Lançar um processo é API segura da biblioteca padrão; não há `unsafe` para esconder, logo não há crate a adotar. **`unsafe_code = "deny"` continua sem exceção.**
- **Não reabre a rejeição do ADR-0049.** Aquele ADR recusou `git rev-parse` com uma razão precisa: *"um processo por consulta... no Windows um `CreateProcess` de dezenas de milissegundos — **dentro do caminho de frame**, o pior lugar possível"*. A objeção era o lugar. Aqui o processo roda numa thread, uma vez a cada centenas de segundos, e nunca no caminho que desenha.
- **`git2`/libgit2 continua recusado, com razão mais forte.** Além do tamanho e da licença que o ADR-0049 já pesou, ele não faz `pull --ff-only` sem que reimplementemos a política de fast-forward por cima — trocaríamos um comando por um algoritmo nosso, na operação que mexe na árvore do usuário.

Três coisas precisam ser verdade, e nenhuma delas é automática:

**O processo não pode abrir janela.** O binário é GUI; um processo de console lançado por ele **ganha um console novo**, que pisca na tela a cada intervalo. A supressão é uma flag de criação de processo — e, detalhe que merece estar escrito porque é fácil de concluir errado depois: **o método que a aplica é seguro**, não `unsafe`. Verificado ao escrever esta decisão, com um binário mínimo sob `#![deny(unsafe_code)]` que a chama e compila; na extensão de `Command` para Windows, o único item `unsafe` é o de atributos crus, que não usamos. A regra do workspace segue intacta e **sem exceção** — o que importa registrar é justamente isto, porque a suposição contrária levaria alguém a abrir a primeira exceção do projeto por nada.

**O processo não pode ficar esperando o usuário.** São quatro canais de interatividade, e desligar três deixa o quarto travar a consulta para sempre:

| Canal | O que acontece se ficar ligado |
|---|---|
| Prompt no próprio terminal | o `git` fica esperando uma senha que ninguém vai digitar |
| `askpass` do `git` | idem, por outro caminho |
| `askpass` do `ssh`, e passphrase de chave | idem, num terceiro |
| **Gerenciador de credenciais** | o pior: abre **janela gráfica própria**, que não é console e que a flag acima não segura |

Os quatro são desligados explicitamente, e o vetor que faz isso é montado por uma função pura, testada — é o teste de segurança desta entrega, e ele também fixa que **nada vindo do usuário é concatenado em linha de comando**: caminho é argumento, nunca texto interpolado. É a mesma propriedade que o [ADR-0042](0042-hyperlinks-osc-8.md) exige para abrir um URI.

**A consulta precisa de teto.** A espera pelo fim do processo é feita por checagem em intervalo curto, no padrão que a thread de observação de processo do terminal já usa; estourado o teto, o filho é morto e o resultado é "falhou". Falha entra em recuo progressivo, para que um repositório atrás de uma VPN fora do ar não vire uma consulta perdida a cada intervalo.

**Sem `git` no sistema**, o recurso se desliga para o resto da execução e informa uma vez (RF-13.19). O silêncio foi recusado pelo mesmo argumento do [ADR-0030](0030-escopo-do-hot-reload.md): silêncio é indistinguível de recurso quebrado. Um aviso por execução, no canal que expira sozinho, e só para quem ligou o recurso.

### 7. A contagem sai de um comando só, e ele traz os dois números

A contagem é a diferença simétrica entre a branch e o que ela segue: **quantos commits o remoto tem que o local não tem** (atrás) e **quantos o local tem que o remoto não tem** (à frente). É o mesmo par que o `git status` mostra no cabeçalho, e vem de uma invocação só.

O número de "à frente" **não é bônus**: com ele diferente de zero, o `pull --ff-only` do clique **falha por definição**. Sem contá-lo, o app ofereceria um botão que já sabe que não funciona — e é por isso que o RF-13.9 existe e que o estado divergente é exibido em vez de escondido.

A forma é `rev-list --count --left-right` sobre a diferença simétrica (`...`, três pontos) entre o que a branch segue e ela própria. **A ordem dos dois números é o detalhe que se erra**, e por isso fica escrita: em `A...B`, a contagem da **esquerda** é a dos commits exclusivos de `A`. Com `<upstream>...HEAD`, a esquerda é portanto o **atrás** e a direita o **à frente** — verificado neste repositório, não deduzido. Quem consome isso é uma função pura sobre a string, e é ela que fixa a ordem em teste.

Os casos de borda, todos decididos aqui para não sobrarem para a implementação:

| Caso | O que acontece |
|---|---|
| **Branch sem upstream** (inclusive branch nova, nunca empurrada) | nem consulta de rede acontece. Não é erro, não é aviso: é o RF-13.18. E não se reconsulta a cada intervalo — só quando a branch muda, o que o `mtime` de `.git/HEAD` já entrega |
| **`HEAD` destacado** | nem consulta nem indicador. Exige distinguir branch de SHA curto, coisa que o ADR-0049 guarda mas não expõe: quem chama recebe um rótulo e não sabe qual dos dois é. A API cresce um degrau |
| **Repositório shallow** | não é consultado: a contagem seria limitada pela fronteira do clone e o `fetch` poderia aprofundá-lo, que é caro e não foi pedido. Detectar é um `stat`, a mesma classe de custo que a §1 do ADR-0049 já abençoou |
| **Vários remotos** | **nunca escolhemos remoto** (RF-13.18). A consulta usa o que a branch declara seguir; uma branch que segue um `fork` não faz o app baixar o `origin` |
| **Worktree e submódulo** | funcionam: a descoberta do repositório já trata a forma `gitdir:` desde o ADR-0049 §2, e submódulos não são percorridos |
| **Merge ou rebase em curso** | a contagem é o retrato da última consulta, e o rótulo não promete "agora". Clicar durante um merge falha e informa, como qualquer outra falha |

### 7.1. O que o app diz, em que canal e com que severidade

O canal é o **aviso do app** — canal 1 do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) —, e não a nota no grid. O critério daquele ADR é *"informação que pertence ao histórico de um terminal fica dentro dele; informação sobre o app fica no overlay"*, e à primeira vista um fato sobre o repositório de uma aba parece ser do terminal. Não é: a nota no grid é **saída de um programa que rodou ali**, e nada disto rodou na aba — o `git` rodou fora dela, a pedido de um clique no chrome. Escrever no grid mexeria na tela do programa que o usuário tem aberto, para dizer algo que ele não pediu àquele terminal.

O ADR-0014 lista nominalmente quem usa o canal 1, e esta decisão acrescenta três, com a severidade pinada aqui para não sobrar indefinida:

| Quando | Severidade | Por quê |
|---|---|---|
| Integração deu certo | **informação** (some sozinha) | o resultado já está na barra: o indicador sumiu. O aviso só confirma, e confirmar não merece ocupar a tela até alguém dispensar |
| Integração falhou | **erro** (persiste até dispensa) | traz a mensagem do `git`, que é o que diz o que fazer a seguir — e que o usuário pode não estar olhando no instante em que ela chega |
| `git` ausente, ou intervalo elevado ao piso | **informação**, uma vez por execução | é sobre a configuração, não sobre uma ação que acabou de falhar |

### 8. Anatomia: o segundo item colorido da barra

Entra na zona esquerda, **depois da branch e antes do grupo**: a contagem é propriedade da branch, como a branch é propriedade do diretório.

- **Ícone** de seta para baixo, da face Lucide — que **não é subsetada** (2059 ícones), então nenhum recorte precisa ser refeito, como o ADR-0049 §4 já registrou ao trazer `git-branch`. A largura do desenho é **medida da rasterização**, nunca estimada: é a armadilha que a face já mordeu duas vezes.
- **Em do ícone**: a mesma do segmento de branch, e pela mesma razão. Não é valor novo.
- **Cor**: o **Acento `#5ed3bc`**, que já é token (§1.5) e já tem outros consumidores. **Zero cor nova** — mas **não zero chave nova**: o [PRD-004](../prd/prd-004-aparencia-do-chrome.md) exige que *"toda cor, dimensão e raio é uma chave de config com default declarado"*, e a métrica dele de valores de aparência no código é zero. Então `[appearance.status_bar]` ganha uma chave para esta cor, ao lado de `shell` e `stale_cwd`, com o mesmo valor do token. Ela é de **aparência** e mora lá; `[git]` fica só com o comportamento. São as duas únicas chaves que este recurso acrescenta.
- **Sublinhado sob o cursor**, mais o cursor de mão — a mesma affordance que o hyperlink OSC 8 recebe, e pelo mesmo motivo: o que é clicável precisa dizer que é, e o sublinhado é o vocabulário que o app já usa para isso.
- **Nunca truncado.** Em janela estreita ele cai inteiro, como a branch já faz; um "3 commi" não é informação.

**Isto revisa o ADR-0048 §3**, que dizia que o nome do shell é *"o único item colorido"*, e é uma mudança de aparência — portanto, decisão do dono do produto, que a tomou depois de ver as três opções e as recusas registradas. O argumento a favor: um alvo clicável que não se distingue do texto ao redor não é descoberto, e a barra inteira estava declarada não-clicável até agora. O argumento contra, que perdeu: dois pontos de cor disputam o olhar, e a cor era o que distinguia a aba de relance.

A mitigação está na natureza do segmento: ele **quase nunca está lá**. Sem commits novos não há indicador (RF-13.8), então o caso comum da barra continua tendo um item colorido só. O segundo acento aparece justamente quando há algo a notar — que é o que a cor deveria significar.

### 9. O clique e a borda de resize

O [ADR-0048](0048-barra-de-status.md) §5 cedeu a borda inferior à zona de resize com uma conta explícita: *"no escopo aprovado ela não tem nenhum alvo clicável, então ceder 6px não custa função nenhuma — enquanto tirar o resize do rodapé quebraria um gesto que existe hoje e não tem substituto"*. A premissa muda; a conta, não. A metade sobre o resize continua inteira, e é ela que decide.

**O retângulo do indicador vence a faixa inteira; todo o resto dela continua sendo resize.** É a saída que o próprio ADR-0048 escreveu na alternativa que recusou — *"o alvo específico ganha, o resto da faixa continua sendo resize"* —, e a precedência não é inventada: é exatamente a que os botões de janela já têm contra o canto superior direito desde o [ADR-0027](0027-controles-de-janela-e-resize-proprios.md).

O que se perde é uma tira da borda inferior com a largura do indicador. Os cantos não são tocados: o indicador nasce depois do padding da barra e de três segmentos, então ele nunca alcança nem o canto esquerdo nem o direito, que são onde o redimensionamento diagonal mora.

A alternativa — dar ao indicador só a parte de cima da faixa e deixar a tira inferior para o resize — foi recusada: um alvo cujo quarto inferior arrasta a janela em vez de clicar é pior que não ter alvo. E ela seria pior ainda por não ser um número fixo: a espessura da zona de resize é **configurável** (RF-4.25 do [PRD-004](../prd/prd-004-aparencia-do-chrome.md)), então "os 6px" é o default, não a regra — uma divisão horizontal da faixa mudaria de proporção com a config do usuário, e com um valor grande o bastante não sobraria alvo. A regra é por **retângulo**, e não depende daquele número.

**As duas precedências mudam juntas.** O caminho que decide a forma do cursor e o caminho que decide o clique resolvem a disputa com o resize **separadamente**, e mudar só um produz um cursor que diz "redimensionar" sobre um alvo que faz `pull`. É a armadilha desta parte, e por isso está escrita aqui e não só no roadmap.

**Não há ação nova no catálogo.** O clique é superfície de mouse, e entra na tabela de comportamentos que têm requisito e não recebem nome de ação. O contra-argumento honesto — uma ação vinculável daria caminho de teclado — foi recusado pela razão do ADR-0051 §4: um atalho que roda `git pull` no repositório que a aba ativa por acaso tem é decisão que se toma uma vez e se esquece. O clique mira o que descreve.

**Acessibilidade.** O segmento entra na árvore como os outros, pela mesma projeção do layout que o [ADR-0043](0043-arvore-de-acessibilidade.md) exige — árvore construída à parte divergiria do desenho. Ele é anunciado como **botão**, com a contagem na descrição, porque o que a cor e o sublinhado dizem não chega a quem não vê a tela — mesmo motivo do RF-9.4 lá. É o **primeiro nó de chrome com ação que não é da barra de abas**, e o ADR-0043 §4 precisa ganhá-lo na enumeração.

E aqui há um custo que a recusa da ação no catálogo cobra, e que fica registrado em vez de escondido: **o alvo é só de mouse.** O ADR-0043 §6 decidiu que não há travessia de chrome por `Tab`, de propósito — leitor de tela navega pelos mecanismos dele —, então anunciar o nó como botão resolve *saber que ele existe*, e não necessariamente *acioná-lo* sem mouse. O ADR-0043 previu isso ao dizer que nó com ação existe para o leitor de tela invocar; enquanto essa invocação não for roteada, quem não usa mouse **não integra pelo indicador** — integra digitando `git pull`, que é o caminho que sempre existiu e que este recurso não tira de ninguém. Está na tabela de riscos, e é a razão mais forte a favor de reabrir a ação de catálogo se ela for pedida um dia.

### 10. Config: seção própria, `[git]`, classe de recarga A

**Não vai em `[appearance.status_bar]`.** Aquela seção é o contrato **visual** da faixa — cores, altura, fonte —, e isto é comportamento com efeito de rede e de processo. O precedente é `[project_file]`: recurso com PRD e ADR próprios, implicação a explicar, seção de topo própria.

`[general]` também foi recusada: as chaves dela são sobre confirmação de fechamento e diretório inicial, e usá-la como destino de tudo que não tem casa é como ela deixa de significar alguma coisa.

Uma chave só:

```toml
[git]
remote_poll_interval_secs = 300
```

O sufixo de unidade segue o que a config já faz. A unidade é **segundos**, não milissegundos como as outras chaves de tempo, e a divergência é deliberada: as outras são prazos de interface, onde milissegundos são a unidade natural; um intervalo de rede em milissegundos seria um número de seis dígitos que ninguém lê.

**Piso de 30 segundos, com aviso** (RF-13.3). Um `1` ali dispararia uma consulta por segundo em cada repositório aberto. Saturar em silêncio esconderia do usuário que o número dele não vale; recusar a config derrubaria mais do que o erro merece — e o [ADR-0003](0003-formato-de-configuracao.md) não permite que config ruim derrube nada. O valor do piso mora numa função pura, com o `0` e a saturação, testável sem tocar em nada.

**Classe de recarga A**, e a consequência prática é que não há trabalho nenhum a fazer para o hot reload funcionar: o prazo é recalculado a cada volta do laço de eventos, lendo a config atual. Pôr `0` faz o prazo sumir na volta seguinte; tirar o `0` faz a consulta começar na volta seguinte. O que entra é um **teste** fixando essa classe, ao lado do que já fixa a mesma coisa para as cores da barra.

Um detalhe que precisa de regra, porque é onde a recarga a quente mente: com uma consulta **em voo**, pôr `0` não a cancela, e o resultado chega depois. A regra é que a exibição do segmento é condicionada ao intervalo estar ligado — então `0` esconde o indicador **imediatamente**, mesmo com dado fresco no mapa.

### 11. Onde o código mora

`porecatu-config` ganha a seção. O módulo que já lê a branch em `porecatu-ui` ganha a consulta, a contagem, a classificação de falha e as decisões — todas funções puras, testáveis sem repositório no disco: o parsing da saída do `rev-list`, o intervalo efetivo, se está na hora de consultar, o que fazer com um resultado que chega, o rótulo com singular e plural, o vetor de não-interação, e se o clique pode ou não integrar.

O segmento e o teste de acerto do clique ficam junto do resto do layout da barra, que já é função pura. O mapa, o prazo e a variante nova do canal de eventos ficam no ciclo de vida do app.

**`porecatu-term`, `porecatu-pty`, `porecatu-session` e `porecatu-core` não mudam** — nenhum deles tem o que dizer sobre isto, e a regra de dependência da arquitetura continua satisfeita sem aresta nova. Um crate `porecatu-git` foi considerado e não paga: é um punhado de funções puras e um lançamento de processo, contra um `Cargo.toml`, uma linha na árvore de dependências e uma fronteira a explicar em toda revisão futura — o mesmo julgamento do ADR-0051 §7.

## Alternativas consideradas

### Consultar com `ls-remote`, sem baixar nada

Pergunta ao remoto o identificador da branch e compara com o local. **Não escreve nada** no repositório do usuário, não baixa objeto nenhum, e teria evitado a consequência negativa mais desconfortável desta decisão.

Recusada porque não responde a pergunta que foi feita. Ela diz *se* há diferença, não **quantos** commits — para contar, os commits precisam estar no repositório local. O indicador viraria "há atualizações", sem número, e o clique teria de baixar tudo na hora, tornando a única operação interativa a mais lenta. O dono do produto escolheu a contagem exata sabendo o custo.

### `git2` / libgit2

Faria consulta, contagem e integração sem lançar processo. Recusada pelas razões que o ADR-0049 já pesou (tamanho, build de C, licença a analisar) mais uma que só aparece aqui: **ela não faz `pull --ff-only`**. Teríamos de reimplementar a política de fast-forward por cima da biblioteca — escrever nosso próprio algoritmo de merge justamente na operação que mexe na árvore do usuário, em vez de usar o comando que ele mesmo usaria.

### Uma thread de vida longa, com canal de comando

Uma thread só, dormindo e acordando por conta própria. Recusada por duplicar o agendamento que o `WaitUntil` já faz de graça e por pedir um caminho de parada que **nenhuma thread do projeto tem** — o watcher de config, que é o análogo mais próximo, é detached e roda para sempre. Uma thread por consulta não tem ciclo de vida para acertar.

### Estado por janela, junto do `GitInfo` que já existe

O caminho de menor mudança: o estado remoto ao lado do estado da branch, em `WindowState`. Recusada pela §3 — duas janelas no mesmo repositório fariam duas escritas concorrentes no mesmo `.git`. É a alternativa que não é questão de gosto.

### Vigiar só o repositório da janela **focada**

Economizaria consultas com várias janelas abertas. Recusada porque **cada janela tem uma barra visível na tela**: a janela não focada mostraria uma contagem velha sem nada dizendo que é velha, que é o defeito que o RF-9.4 existe para não repetir. Consultar a aba ativa de cada janela custa uma consulta por repositório distinto — e repositórios iguais já são deduplicados.

### Allowlist de diretórios, no modelo do `.porecatu`

Simetria aparente com o [ADR-0051](0051-arquivo-de-projeto-porecatu.md), e teria evitado o `fetch` automático em repositório de terceiros. Recusada pela §4: aquela lista contém **execução de código declarado por outra pessoa**, e não é o que acontece aqui. Aplicá-la assim mesmo traria a dívida que o ADR-0051 registrou — sem caminho de UI para autorizar, porque o app não escreve na config — em troca de conter um risco de natureza diferente.

### Ação `git.pull` no catálogo, vinculável a tecla

Daria caminho de teclado e uniformizaria com a acessibilidade. Recusada na §9: um atalho que integra commits no repositório que a aba ativa por acaso tem, sem confirmação, é decisão que se toma uma vez e se esquece. O clique é deliberado e mira o que descreve.

### Integrar automaticamente quando for fast-forward

Tecnicamente seguro — fast-forward não sobrescreve trabalho — e tornaria o indicador desnecessário. Recusada porque foi explicitamente excluída pelo dono do produto, e a razão é boa: mudar os arquivos sob um editor aberto, ou sob um servidor de desenvolvimento em modo watch, é surpresa mesmo quando a operação é segura. Quem decide quando o código muda é quem está trabalhando nele.

### Indicador sem ícone, só o texto sublinhado

Evitaria acrescentar um codepoint à face de ícones, e o ADR-0048 §10 registra a armadilha de ícone novo. Recusada pelo dono do produto em favor de ícone mais texto: o segmento de branch ao lado já abre com ícone, e um item de texto solto no meio da barra não lê como a mesma classe de coisa.

### Barra de status como canal do resultado do `pull`

Tentador: o clique aconteceu ali, a resposta poderia aparecer ali. Recusada pela mesma razão que o ADR-0048 §9 usou para não fazer da barra um canal de avisos — uma faixa de 26px com segmentos fixos não comporta título, corpo e a mensagem de erro do `git`, e transformá-la nisso destruiria a única coisa que ela faz bem: estar sempre lá, dizendo sempre a mesma coisa.

## Consequências

### Positivas

- A pergunta "o que mudou lá enquanto eu trabalhava aqui?" é respondida sem digitar nada e **sem `git status` no prompt**, que é como a maioria dos shells a responde e é o que custa caro em repositório grande — a mesma vitória que o ADR-0049 já tinha obtido para a branch.
- **Nenhuma dependência nova, nenhum `unsafe`, nenhuma cor nova.** Terceira entrega seguida sem crate novo; o token de acento já existia e já tinha outros consumidores.
- O RF-9.8 sai desta decisão **mais preciso, não mais frouxo**. A regra antiga ("nada que mude sozinho") não teria como barrar um `git status` por frame; a nova barra, porque o terceiro critério fala de percorrer a árvore.
- **`porecatu-term`, `porecatu-pty`, `porecatu-core` e `porecatu-session` não mudam**, e a regra de dependência da arquitetura continua satisfeita sem aresta nova.
- A propriedade de que *terminal ocioso não desenha quadro* fica intacta: consulta que confirma "nada novo" não suja nada.

### Negativas

- **O app passa a falar com a rede sozinho, por padrão, e a escrever no `.git` de todo repositório que o usuário abrir.** É a consequência mais desconfortável desta decisão e não tem mitigação além da chave que a desliga. Vem de uma escolha deliberada do dono do produto, pela razão da §1, e o custo está aqui e na primeira linha da tabela de riscos em vez de escondido.
- *"Terminal ocioso custa zero CPU"* (ADR-0007) deixa de ser literal com o recurso ligado. O GPU continua zero; o CPU passa a ter um pico por intervalo.
- **O primeiro lançamento de processo do projeto fora do PTY sem crate wrapper**, com tudo o que vem junto: um vetor de variáveis de não-interação a manter, um watchdog, e um processo filho que sobrevive ao fechamento do app no Windows.
- A barra ganha **um segundo item colorido e o primeiro alvo clicável**, revisando duas linhas do ADR-0048. A faixa deixa de ser puramente informativa, que era parte do que a tornava simples.
- **Quatro seções de dois ADRs aceitos passam a ser parcialmente supersedidas.** A fronteira do que entra na barra fica mais fina a cada vez — era uma palavra ("git"), virou duas coisas, e agora são três com critério próprio. É por isso que a §2 existe.
- A contagem é sempre um retrato do último instante consultado. Entre dois intervalos, ela está desatualizada e não há nada dizendo isso — diferente do `cwd`, que tem a marca do RF-9.4. Recusou-se dar marca própria a ela pela mesma razão do ADR-0049 §6: duas marcas para a mesma classe de ressalva viram ruído.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| O default ligado fazer `fetch` em repositório de terceiros que o usuário só abriu para ler | **Alta** | Médio | `fetch` não toca a árvore de trabalho nem o `HEAD`; o que ele escreve é `refs/remotes` e `FETCH_HEAD`. Uma linha desliga, e desligado custa zero. Documentado no arquivo de exemplo com a mesma franqueza do bloco de `trusted_paths` |
| O RF-9.8 deixar de valer e o app passar a acordar sozinho | **Alta** (é o que acontece) | Alto | Critério novo com três partes na §1; teste provando que com `0` nenhum prazo entra na conta e o laço dorme sem prazo |
| Gerenciador de credenciais abrir **janela gráfica** e travar a consulta para sempre | Média | **Alto** | Vetor de não-interação cobrindo os quatro canais, não três; mais o teto de tempo. O vetor é função pura e é o teste de segurança da entrega |
| Janela de console piscando a cada intervalo no Windows | Alta sem a flag | Médio | Flag de criação de processo que suprime o console — e ela é **API segura**, então `unsafe_code = "deny"` segue sem exceção |
| Processo `git` órfão sobrevivendo ao fechamento do app (Windows) | Média | Baixo | Sem Job Object, porque `porecatu-ui` não pode depender de `porecatu-pty`; o `fetch` termina sozinho em segundos. Registrado, não resolvido |
| Duas janelas no mesmo repositório disputando lock do `.git` | Média | Médio | Chave por repositório deduplica; consulta em voo impede a segunda. Resíduo: dois *worktrees* do mesmo repositório têm chaves distintas |
| A contagem mentir depois de um `checkout` ou de um commit local | Alta | Baixo | A entrada carrega a branch a que pertence, e branch diferente esconde o indicador; o `mtime` de `.git/HEAD` do ADR-0049 detecta o checkout sem nada novo |
| Clique acidental alterar a árvore de trabalho | Média | **Alto** | `--ff-only` recusa em vez de sobrescrever ou criar merge; sem binding de tecla; nunca automático; e com commits locais à frente o indicador nem é clicável |
| Cursor e clique discordarem sobre quem vence a borda de resize | **Alta** (é o erro fácil) | Médio | As duas precedências mudam na mesma etapa, com teste que as compara — está escrito na §9 e no critério de saída do roadmap |
| O alvo clicável comer o gesto de redimensionar | Média | Baixo | Só o retângulo do indicador vence; os cantos nunca são alcançados, porque ele nasce depois de três segmentos |
| Repositório atrás de VPN fora do ar consumindo uma consulta perdida por intervalo | Média | Baixo | Teto de tempo e recuo progressivo depois de falha; falha não repinta a barra nem gera aviso repetido |
| Alguém estender para `git status` porque "agora já temos uma thread" | **Média** | **Alto** | §2 diz que a razão nunca foi a thread. O critério é percorrer a árvore, e ele não depende de quanta infraestrutura já existe |
| `[git]` virar a seção onde toda integração futura de controle de versão se acumula | Baixa | Médio | Uma chave só, e chave nova ali exige requisito novo — a mesma regra que governa o catálogo de ações |
