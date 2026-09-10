# ADR-0051 — Arquivo `.porecatu`: seções cruas, confiança por allowlist e escrita no PTY

**Status:** Aceito
**Data:** 2026-09-10
**Relacionados:** ADR-0003, ADR-0005, ADR-0013, ADR-0014, ADR-0030, ADR-0031, ADR-0036, ADR-0037, ADR-0038, ADR-0039, ADR-0042, PRD-003, PRD-012

## Contexto

O [PRD-012](../prd/prd-012-comando-de-projeto-por-diretorio.md) pede que uma aba restaurada execute o script que o projeto declarou no arquivo `.porecatu` do seu diretório. O requisito diz **o quê** e para de propósito antes de cinco coisas, e nenhuma delas é detalhe de implementação:

1. **Qual formato.** O projeto inteiro é TOML ([ADR-0003](0003-formato-de-configuracao.md)), e a resposta óbvia seria TOML de novo. Mas o conteúdo aqui é um script multi-linha, e TOML transforma "cole o que você digitaria" em "cole o que você digitaria, depois escape as aspas e escolha entre três formas de string".
2. **Como o shell é casado com a seção.** O domínio carrega `Tab::shell_name`, que é o `file_stem` minúsculo do programa de spawn — `pwsh`, `bash`, `cmd`. Falta dizer se `pwsh` cai numa seção `[powershell]`, se `bash` cai em `[sh]`, e o que acontece quando não há seção nenhuma.
3. **Onde vive a confiança.** Esta é a que pesa. O `.porecatu` **vem junto com o projeto**, e projeto clonado é arquivo escrito por outra pessoa: `git clone` de um repositório qualquer, abrir uma aba ali, fechar o app e reabrir bastaria para executar o que estiver no arquivo. É a mesma escada que o [ADR-0042](0042-hyperlinks-osc-8.md) §3 recusou construir para o OSC 8, com uma diferença que corta para os dois lados — lá o conteúdo vinha da saída de um programa e nunca era pedido; aqui o usuário quer, deliberadamente, que o arquivo rode.
4. **Quando exatamente escrever no PTY.** O app nunca escreveu um byte no PTY que não tenha vindo de tecla, clipboard, mouse ou resposta ao próprio programa hospedado — este é o primeiro. Escrever cedo demais entrega o script ao vazio: a medição da etapa 6 da F6 mostrou **~480 ms** entre `main` e o primeiro byte do PTY com `pwsh`, e ~358 ms com `cmd.exe`. Um atraso fixo acerta numa máquina e erra na seguinte.
5. **O que acontece com um arquivo encontrado e não autorizado.** Ignorar em silêncio é indistinguível de um recurso quebrado — a frase é do [ADR-0030](0030-escopo-do-hot-reload.md), e vale aqui inteira.

Sem estas cinco, cada uma seria resolvida na implementação, em cinco lugares diferentes, por quem estivesse escrevendo a linha. A terceira seria resolvida errado.

## Decisão

**O `.porecatu` é lido no diretório de trabalho de uma aba restaurada, tem seções cruas nomeadas pelo shell, e o script da seção correspondente é escrito no PTY como se digitado — só em diretório declarado na config do usuário, e no máximo uma vez por aba.**

### 1. Gatilho: só restauração, só o diretório exato

O arquivo é procurado quando uma aba **restaurada de sessão** sobe o shell, e em nenhum outro momento. Aba criada por `tab.new`, `group.new_tab` ou `window.new` não dispara, mesmo herdando o `cwd` de uma aba que dispararia; `cd` para um diretório com `.porecatu` também não.

Duas razões, nesta ordem. A primeira é de superfície: restauração é um evento raro e deliberado — o usuário reabriu o app —, enquanto abrir uma aba é gesto de rotina, e ninguém quer que abrir um terminal para dar um `git status` suba o servidor de desenvolvimento junto. A segunda é de capacidade: disparar no `cd` exigiria saber que o diretório mudou, o que depende de OSC 7, o que no Windows não existe sem integração de shell ([ADR-0038](0038-fallbacks-de-cwd.md) §3). O recurso funcionaria em duas plataformas e não na terceira, que é o oposto do princípio 6 do [PRD-000](../prd/prd-000-visao-de-produto.md).

A busca acontece **só no diretório exato**, sem subir a árvore. Um `.porecatu` na raiz do projeto não alcança uma aba restaurada em `src/`: quem quer o script nas duas põe o arquivo nas duas, e o custo disso é menor que o de uma aba disparar um script que o usuário não vê de onde veio.

E, se o diretório gravado não existe mais e a aba caiu no home pelo RF-3.10, **nada é procurado**. O home do usuário é o último lugar onde um `.porecatu` de outro projeto deveria rodar por acidente.

### 2. Formato: seções cruas, corpo literal

Uma linha que case `^\[[A-Za-z0-9_.+-]+\]\s*$` abre uma seção. Todo o resto é corpo, literal, linha por linha, até a próxima seção ou o fim do arquivo. Linhas antes da primeira seção são preâmbulo e são ignoradas — é ali que vive o comentário sobre o arquivo.

Três consequências que decidem a escolha:

- **Não há sintaxe de comentário própria.** Dentro de uma seção, `#` é do script, porque é o shell daquela seção quem vai recebê-lo — e em `cmd.exe` `#` não comenta coisa nenhuma. Um comentário nosso que se parecesse com comentário do shell seria a pior ambiguidade possível num arquivo cujo corpo é código.
- **Não há escape, e portanto não há como escrever uma linha de script que seja literalmente `[algo]`.** É limitação, está documentada como tal na [referência](../reference/arquivo-de-projeto.md), e é o preço de o corpo ser exatamente o que se cola. Nenhum shell do fluxo tem comando cuja linha inteira tenha essa forma.
- **Linhas em branco no fim de uma seção são cortadas; no meio, preservadas.** Uma linha em branco no meio de um script é uma linha em branco enviada ao shell, que é o que o usuário digitaria.

O parser cabe em trinta linhas e não tem estado de erro: qualquer arquivo é um arquivo válido, possivelmente sem a seção que interessa. O único caminho de falha é de I/O — arquivo ilegível ou fora de UTF-8 —, e ele informa (RF-12.4).

### 3. Seleção da seção: nome exato do shell, depois `[default]`

A chave é `Tab::shell_name`, que o domínio já carrega e que a sessão já grava: o `file_stem` minúsculo do programa de spawn, sem caminho e sem extensão — `"pwsh"`, `"bash"`, `"cmd"`, nunca `pwsh.exe` nem `/bin/zsh`. Casamento **exato**, insensível a maiúsculas. Sem seção correspondente, `[default]`. Sem nenhuma das duas, nada acontece e nada é informado: o arquivo declara os shells que suporta, e não suportar o seu não é erro dele nem do app.

**Não há tabela de apelidos.** `pwsh` não cai em `[powershell]`, e `bash` não cai em `[sh]`. Uma tabela de compatibilidade entre shells é conhecimento que o app teria de manter, que envelhece, e que estaria errado com frequência incômoda: `&&` e `??` do PowerShell 7 não existem no 5.1, e `nvm use` num `sh` puro não é a mesma coisa que num `bash` com o `nvm` carregado pelo `.bashrc`. Quem quer os dois escreve as duas seções — que é uma linha a mais, contra uma classe inteira de "rodou o script errado e nem sei por quê".

`[default]` existe justamente para quem prefere não enumerar: é o apelido de todos, declarado pelo autor do arquivo em vez de inferido pelo app.

### 4. Confiança: allowlist declarada na config, vazia por default

O script só roda se o diretório estiver sob um caminho declarado em `[project_file] trusted_paths`, na configuração do usuário. **A lista é vazia por default**, o que significa que numa instalação recém-feita nenhum `.porecatu` roda — negação por default, e a única forma de sair dela é o usuário escrever um caminho no próprio arquivo de config.

Isso parece contradizer o [ADR-0042](0042-hyperlinks-osc-8.md) §5 — *"config não é fronteira de segurança… uma lista configurável é escalada de privilégio embalada como conveniência"* — e não contradiz, porque as duas listas apontam para lados opostos. Lá, tornar a lista de esquemas configurável **alargaria** o que era executável a partir de um default que já funcionava, e o ganho ia para quem copia um `porecatu.toml` de um gist sem ler a linha nova. Aqui a lista é a **única** coisa que liga o recurso: sem ela nada roda, e cada entrada é uma declaração explícita sobre uma parte do disco da própria pessoa. O precedente que se aplica é o de `[terminal.clipboard] osc52_read` ([ADR-0013](0013-mouse-selecao-e-clipboard.md)): política configurável, default seguro, e o custo escrito em prosa no arquivo do usuário, onde ele o lê no momento de mudar.

Regras da comparação, todas obrigatórias:

- **`~` é expandido** por `dirs::home_dir`, crate que já está no workspace.
- **Os dois lados são canonicalizados antes de comparar** — o `cwd` da aba e cada caminho da lista. Sem isso, `C:/Projetos/../Windows/System32` passa por uma lista que só contém `C:/Projetos`.
- **A comparação é por componente de caminho, nunca por prefixo de string.** `C:/Projetos-do-vizinho` não pode casar com `C:/Projetos` só porque um é prefixo textual do outro.
- **Canonicalizar resolve symlink, e esse é o comportamento certo.** Um link dentro de um diretório confiável apontando para fora dele não é confiável: o que vale é onde o diretório está de fato.
- **Caminho da lista que não existe é ignorado em silêncio**, não é erro de config. Dotfiles são compartilhados entre máquinas, e um `~/work` que só existe na máquina do escritório não pode derrubar a config na de casa.

Se `[project_file] enabled = false`, nada disso acontece: o recurso é desligado antes de qualquer leitura de disco, incluindo a do §5.

### 5. Arquivo encontrado e não autorizado: nota no grid, uma vez por execução

Um `.porecatu` num diretório fora da lista **não roda e não passa em silêncio**. O app escreve uma nota no grid daquela aba — canal 2 do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), via `Terminal::inject_note`, o mesmo mecanismo e a mesma marcação do convite do [ADR-0039](0039-convite-a-integracao-de-shell.md) — dizendo o caminho encontrado e a linha exata a acrescentar na config.

**Uma vez por execução do app**, não por aba: restaurar dez abas de dez projetos não autorizados produz uma nota, na primeira que encontrar um arquivo. Dez notas idênticas seriam ruído, e a lição está escrita no ADR-0039, que recusou o convite por aba pela mesma razão.

Ao contrário do convite do ADR-0039, esta nota **não tem dispensa definitiva** e não grava nada em `session.json`. Ela não é um convite a configurar algo genérico: é a resposta a um arquivo concreto que o usuário acabou de encontrar no caminho, e ela só aparece porque esse arquivo existe. Quem não quer vê-la nunca mais apaga o arquivo, autoriza o diretório, ou desliga o recurso — três saídas, todas nas mãos dele, nenhuma exigindo um quarto lugar de estado.

O texto da nota é curto e **não é embutido de um arquivo de documentação**. O [ADR-0039](0039-convite-a-integracao-de-shell.md) §5 embute os snippets de `docs/reference/integracao-de-shell.md` porque são cinco blocos de código longos que precisam ser corrigidos num lugar só; aqui são três linhas com um caminho interpolado. Um `include_str!` traria acoplamento sem ter o problema que ele resolve.

### 6. Execução: escrita no PTY, depois do primeiro byte e de um silêncio

O script vai por `Terminal::write`, o caminho que já existe, uma linha por vez, cada uma seguida de `\r` — que é o que `input::handle_keyboard_input` manda no Enter. O shell ecoa, e é isso que dá ao usuário as três coisas que o RF-12.8 exige de graça: o comando **visível** na tela, **no histórico** do shell, e **interrompível** com `Ctrl+C` como qualquer outro.

**Quando escrever** é a parte difícil. O critério é: a aba já produziu o **primeiro byte do PTY** e ficou em **silêncio por um intervalo** (~300 ms, dimensionado na etapa), com um teto de segurança (~5 s) a partir do qual o app escreve mesmo assim ou desiste.

Um atraso fixo contado do spawn não serve, e a medição da F6 diz por quê: ~480 ms até o primeiro byte com o `pwsh` desta máquina, ~358 ms com `cmd.exe` — a diferença entre um shell e outro na mesma máquina já é maior que qualquer margem confortável, e entre máquinas é pior. O primeiro byte é o evento que prova que o shell existe; o silêncio depois dele é o que separa "o prompt terminou de desenhar" de "o ConPTY ainda está reemitindo a tela", que é comportamento conhecido e registrado ([ADR-0004](0004-pty-cross-platform.md)). No Windows isso vem depois do handshake de DSR que o motor já responde sozinho.

Como todo prazo do projeto, este não é thread nem laço de render: o `Instant` vem de fora, entra no `next_deadline()` da janela e o event loop dorme até a hora exata por `ControlFlow::WaitUntil`. Quem chama `Instant::now()` é `lib.rs`, nunca o módulo de estado.

Mais três regras:

- **Uma vez por aba, por execução do app.** A marca vive no `TabRuntime`, que nasce e morre com o PTY.
- **Não escrever com a tela alternativa ativa** — mesma guarda do ADR-0039. Se algo já tomou a tela, o prompt não está lá para receber o script.
- **Nada do que o script faz entra na sessão.** O arquivo de sessão continua gravando estrutura e diretórios, e a lista de campos do [ADR-0036](0036-formato-do-arquivo-de-sessao.md) não muda um byte.

### 7. Onde o código mora

`crates/porecatu-config/src/project_file.rs`: o parser (puro, sobre uma string), a escolha da seção, a leitura do arquivo e a checagem de confiança contra `trusted_paths`. O crate já é "arquivo do usuário, parseado", já depende de `dirs` e já é visto por `porecatu-ui`; a regra de dependência da [arquitetura](../arquitetura.md) continua satisfeita sem uma aresta nova.

O gatilho e a escrita ficam em `porecatu-ui`, junto do resto do ciclo de vida da aba. **`porecatu-term` e `porecatu-session` não mudam** — o primeiro já expõe `write` e `inject_note`, e o segundo não tem nada a gravar.

Um crate `porecatu-project` foi considerado e não paga: é um parser de trinta linhas e uma comparação de caminho, contra um `Cargo.toml`, uma linha na árvore de dependências e uma fronteira a explicar em toda revisão futura.

### 8. O que fica de fora, e por quê

- **Subir a árvore de diretórios** (§1): uma aba em `src/` disparando o script da raiz é surpresa, e multiplica a superfície de execução por quantos diretórios o projeto tiver.
- **Aba nova e `cd`** (§1): gesto de rotina não pode disparar execução, e o `cd` não é observável no Windows sem integração de shell.
- **Lógica dentro do arquivo** — variável, condicional, interpolação. O corpo é literal, e quem precisa de lógica a escreve no script, que é do shell. Isso **não reabre** o [ADR-0003](0003-formato-de-configuracao.md): aquilo é a config do app, isto é um arquivo do projeto com um comando dentro, sem API nossa e sem runtime embutido.
- **Metadados por seção** (`title`, `working_dir`, `enabled`): cada um é uma chave a manter para sempre num formato que só nós versionamos, e nenhum foi pedido.
- **Ação no catálogo para reexecutar à mão.** O catálogo é fechado ([acoes.md](../reference/acoes.md)) e ação sem RF não entra. Se aparecer o pedido, ele vem com requisito antes.
- **Autorizar pela interface.** O app não escreve na config do usuário — decisão consistente desde o [ADR-0031](0031-temas-nomeados.md) e reafirmada pelo [ADR-0039](0039-convite-a-integracao-de-shell.md) §4. Ver a primeira consequência negativa.

## Alternativas consideradas

### TOML, como todo o resto do projeto

Era a resposta por default: o parser está no workspace, o erro sai com linha e coluna de graça, e o usuário-alvo já edita TOML. Rejeitada porque o conteúdo é um script, e TOML transforma "cole o que você digitaria" em uma escolha entre `"..."`, `'''...'''` e `"""..."""`, com regras de escape diferentes em cada uma e uma armadilha garantida no dia em que o script tiver uma aspa ou uma barra invertida — que num script de Windows é toda linha com caminho. O ganho do TOML é validação estruturada, e aqui não há estrutura a validar: há um nome de seção e um bloco de texto opaco.

O [ADR-0003](0003-formato-de-configuracao.md) continua de pé e não é tocado. Ele decide o formato da **config do app**; o `.porecatu` é arquivo do projeto.

### Confiança por diretório, autorizada dentro do terminal

O modelo do `direnv` e do "trusted workspace" do VS Code, adaptado ao que este projeto já sabe fazer: na primeira vez que um `.porecatu` aparece num diretório, a nota do §5 mostra o script e a instrução de autorizar; o usuário digita a autorização no próprio terminal, e ela é reconhecida pelo mesmo parser de eco que o [ADR-0039](0039-convite-a-integracao-de-shell.md) já usa para a dispensa; a autorização e o hash do arquivo vão para `session.json`, e o arquivo mudando pergunta de novo.

Foi a alternativa recomendada, e foi a recusada — decisão do dono do produto. Registrado com o que ela teria trazido, porque a diferença é real: ela dispensaria editar o TOML à mão, autorizaria um projeto por vez em vez de uma árvore inteira, e reagiria a uma mudança do arquivo depois de autorizado, que a allowlist por caminho não faz. E com o que ela teria custado: transformaria o `session.json` — hoje um arquivo de conveniência, que o app apaga e recria sem cerimônia e cuja perda não custa nada — em fronteira de segurança, com tudo o que isso implica sobre corrupção, migração de schema e `PORECATU_SESSION` apontando para outro lugar.

### Executar sem perguntar, com uma chave global para desligar

A conveniência máxima, e a leitura mais natural do pedido original. Rejeitada: `git clone` de um repositório qualquer, uma aba ali, fechar e reabrir o app executa o que estiver no arquivo. Um default que transforma clonar em executar não é aceitável, e uma chave para desligar não ajuda quem não sabia que precisava desligá-la.

### Deixar o comando digitado no prompt, sem Enter

Elimina o risco por construção: o usuário vê exatamente o que vai rodar e confirma com uma tecla. Rejeitada porque cobra uma ação por aba, e a métrica que motiva o recurso inteiro é "zero ações do usuário" — com quinze abas, quinze confirmações é o mesmo trabalho de antes, só com o texto já digitado. E interage mal com a restauração preguiçosa: a linha ficaria pendurada no prompt de uma aba que o usuário só visita meia hora depois, sem lembrar de onde ela veio.

### Passar o script como argumento de inicialização do shell (`-c`, `/k`, `-Command`)

Roda antes de qualquer prompt, sem depender de detectar prontidão, e não polui o histórico. Rejeitada por três motivos que se somam: muda o `SpawnConfig` de cada aba restaurada, e portanto colide com o que o usuário configurou em `[shell] args`; a sintaxe difere por shell, o que reintroduz pela porta dos fundos a tabela de conhecimento que o §3 recusou; e o comando deixa de ser visível e de estar no histórico, que são as três garantias do RF-12.8.

### Tabela de apelidos entre shells (`pwsh` casa `[powershell]`, `bash` casa `[sh]`)

Pouparia linhas repetidas em arquivos que suportam vários shells. Rejeitada: é conhecimento de compatibilidade que o app passaria a manter e que estaria errado às vezes, e "às vezes" aqui significa executar o script errado sem o usuário entender por quê. `[default]` cobre o caso de quem não quer enumerar, e cobre declarado pelo autor do arquivo em vez de inferido por nós.

### Crate novo `porecatu-project`

Fronteira limpa para um conceito novo. Rejeitada: um parser de trinta linhas e uma comparação de caminho não pagam um crate, e `porecatu-config` já é exatamente "arquivo do usuário, parseado, sem GUI e sem PTY".

## Consequências

### Positivas

- A métrica do [PRD-000](../prd/prd-000-visao-de-produto.md) passa a valer para o trabalho retomado, não só para a estrutura: reabrir o app devolve quinze abas **com o que elas estavam fazendo**.
- Nenhum widget novo, nenhum pixel novo, nenhuma cor nova. O [ADR-0032](0032-interface-do-v1-fechada.md) continua fechado e a especificação visual não é tocada.
- Nenhuma dependência nova; nenhum `unsafe`; a tabela de stack do README não muda.
- O script fica visível, no histórico e interrompível de graça, porque a escrita no PTY é o mesmo caminho da tecla.
- O default é seguro sem precisar de uma chave: lista vazia é o mesmo que recurso desligado, e ninguém precisa saber que ele existe para estar protegido dele.
- `porecatu-term` e `porecatu-session` não mudam; a mudança é um módulo em `porecatu-config` e um gatilho em `porecatu-ui`.

### Negativas

- **Autorizar um diretório exige editar o TOML à mão.** Não há caminho de UI, porque o app não escreve na config ([ADR-0031](0031-temas-nomeados.md)). O usuário lê a nota do §5, abre o arquivo de config, acrescenta a linha e reinicia. É atrito real, aceito para não abrir a primeira exceção à regra de não escrever na config do usuário.
- **Um `trusted_paths` largo é uma decisão que se toma uma vez e se esquece.** Quem escrever o diretório onde clona repositórios — ou pior, o home — transforma todo repositório novo ali dentro em execução automática ao restaurar, meses depois de ter escrito a linha e sem nenhum lembrete. É a maior fragilidade desta decisão, e a mitigação é só documental.
- **O app passa a manter um formato de arquivo próprio**, versionado apenas pela documentação, sem número de versão dentro do arquivo e sem caminho de migração. Foi escolhido pequeno o bastante para que isso não doa.
- **Navegar rápido por abas restauradas dispara scripts.** O [ADR-0037](0037-aba-nao-iniciada.md) §2 decidiu que passar por uma aba é focá-la, e focar sobe o shell; agora sobe o shell **e** roda o script. Passar por dez abas com `Ctrl+Tab` sobe dez shells e dispara dez scripts. É o comportamento decidido, e é mais caro do que era.
- **O critério de prontidão é uma aproximação.** Um shell com prompt lento, ou um `.bashrc` que escreve saída por dois segundos, pode receber o script no meio de outra coisa. O teto de tempo limita o dano, não o elimina.
- **A escrita no PTY assume um prompt do outro lado.** Hoje isso é sempre verdade numa aba restaurada, porque a sessão não restaura processos. Se algum dia restaurar, esta decisão precisa ser revisitada.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| `trusted_paths` largo transformar todo repositório clonado em execução automática | Média | Alto | Default vazio; o exemplo do `porecatu.example.toml` usa um diretório de projetos e **nunca** `~`, com o custo escrito em prosa na própria seção; o guia do usuário explica o que a linha significa antes de mostrá-la |
| Script escrito antes de o shell estar pronto e se perder em silêncio | Média | Médio | Critério de dois sinais (primeiro byte + silêncio), `Instant` vindo de fora para ser testável sem dormir, teto de tempo, e guarda de tela alternativa |
| Comparação de caminho deixar `..` ou prefixo textual escapar da lista | Baixa | Alto | Canonicalizar os dois lados antes de comparar e comparar por componente, nunca por `starts_with` de string; teste com `..`, com symlink e com o par `C:/Projetos` × `C:/Projetos-do-vizinho` |
| Linha de script que é literalmente `[algo]` ser lida como cabeçalho de seção | Baixa | Baixo | Documentado como limitação do formato na referência; nenhum shell do fluxo tem comando com essa forma de linha inteira |
| Usuário esperar que o `.porecatu` rode em aba nova, e concluir que está quebrado | Média | Baixo | A referência e o guia dizem "só na restauração" na primeira frase; a nota do §5 aparece só onde o arquivo foi de fato encontrado |
| Nota do §5 virar ruído em quem tem muitos projetos não autorizados | Baixa | Baixo | Uma vez por execução do app, não por aba; `enabled = false` silencia tudo |
