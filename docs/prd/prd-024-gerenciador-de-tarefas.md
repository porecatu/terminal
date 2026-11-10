# PRD-024 — Gerenciador de tarefas

**Status:** Aprovado
**Data:** 2026-11-10
**Requisito de origem:** pedido direto do dono do produto: um gerenciador de tarefas dentro do Porecatu, em que os processos que rodam nas abas aparecem num painel com gráficos de consumo, aberto pelo botão do app, que hoje não tem ação ([ADR-0079](../adr/0079-botao-de-menu-do-app.md)).
**Relacionados:** [ADR-0007](../adr/0007-modelo-de-threading.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0015](../adr/0015-multiplas-janelas.md), [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md), [ADR-0030](../adr/0030-escopo-do-hot-reload.md), [ADR-0032](../adr/0032-interface-do-v1-fechada.md), [ADR-0033](../adr/0033-job-object-encerramento-de-processo.md), [ADR-0034](../adr/0034-deteccao-de-processo-ativo-para-confirmacao.md), [ADR-0037](../adr/0037-aba-nao-iniciada.md), [ADR-0048](../adr/0048-barra-de-status.md), [ADR-0052](../adr/0052-sincronizacao-com-o-remoto-do-git.md), [ADR-0053](../adr/0053-paineis-divididos.md), [ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md), [ADR-0068](../adr/0068-efeito-acrilico-no-fundo-do-terminal.md), [ADR-0078](../adr/0078-largura-e-quebra-de-texto-do-dialogo.md), [ADR-0079](../adr/0079-botao-de-menu-do-app.md), [PRD-001](prd-001-abas.md), [PRD-002](prd-002-grupos-de-abas.md), [PRD-006](prd-006-paineis-divididos.md), [PRD-009](prd-009-barra-de-status.md), [PRD-016](prd-016-tela-de-configuracoes.md), [PRD-020](prd-020-conexoes-ssh.md), [PRD-021](prd-021-abas-fixadas.md), [PRD-022](prd-022-abas-e-grupos-entre-janelas.md)

> Aprovado em 2026-11-10, por decisão do dono do produto, **fora da ordem de fases**, como os outros recursos pedidos depois da F6. O dono decidiu, antes da escrita:
>
> - o gerenciador é um **modo da janela**, aberto pelo botão do app, que ocupa o lugar do terminal. Não é uma aba nem uma janela própria;
> - a área se divide **verticalmente ao meio**: à esquerda, três gráficos empilhados (**CPU, memória e disco**, nesta ordem); à direita, a **lista de processos**;
> - cada janela mostra **só os processos dela**, e os gráficos somam esses processos;
> - os gráficos são **de linha**, desenhados **da direita para a esquerda**, como no Gerenciador de Tarefas do Windows, com o período **configurável** (60 s por padrão);
> - a lista é **agrupada por aba**, com os processos em árvore;
> - a lista permite **ir para a aba** de um processo e **encerrar** um processo, mas não o shell raiz do painel;
> - a medição só acontece **com o gerenciador visível**;
> - **rede e GPU ficam fora**, inclusive como total.
>
> O como fica em quatro ADRs:
>
> - a medição, que revê o "nunca continuamente" do [ADR-0033](../adr/0033-job-object-encerramento-de-processo.md) §3: [ADR-0093](../adr/0093-medicao-de-processos-do-gerenciador.md);
> - a primitiva de linha e área do render: [ADR-0094](../adr/0094-polilinha-e-area-no-render.md);
> - o modo da janela, a entrada, a lista, as ações e a chave: [ADR-0095](../adr/0095-gerenciador-de-tarefas-modo-da-janela.md);
> - a anatomia e o botão ligado, com o aval visual do dono pendente: [ADR-0096](../adr/0096-anatomia-do-gerenciador-de-tarefas.md).
>
> **Emendado em 2026-11-10**, a pedido do dono, pelo [ADR-0098](../adr/0098-monitoramento-em-segundo-plano-do-gerenciador.md): uma **alternância no canto inferior direito**, abaixo da lista de processos, escolhe entre medir só com o gerenciador visível (o padrão, como acima) e **medir o tempo todo**, para alternar entre as abas e o gerenciador sem perder o histórico. A escolha é uma chave do `porecatu.toml`, vale para o app inteiro e, ligada, a medição começa no arranque. Os requisitos novos são os RF-24.31 a RF-24.37; os que mudam têm a emenda no próprio texto.

## Problema

Um terminal com muitas abas é, na prática, um lançador de processos de longa duração: servidores de desenvolvimento, watchers, compiladores, bancos locais, `npm start` em três projetos ao mesmo tempo. Quando a máquina fica lenta ou o ventilador dispara, a pergunta é sempre a mesma: **qual das minhas abas está fazendo isso?**

Hoje, para responder, o usuário sai do Porecatu e abre o gerenciador de tarefas do sistema. Lá encontra uma lista plana de dezenas de `node.exe`, `java.exe` e `pwsh.exe` sem nenhuma pista de qual aba abriu cada um. O Porecatu já sabe a resposta, porque rastreia a árvore de processos de cada painel para encerrá-la ao fechar a aba ([ADR-0033](../adr/0033-job-object-encerramento-de-processo.md)). Só não mostra.

## O que já existe e não muda

- **A árvore de processos de cada painel** já é conhecida: a união da lista do Job Object com a varredura a partir do PID raiz ([ADR-0033](../adr/0033-job-object-encerramento-de-processo.md) §3). O gerenciador usa **o mesmo critério**, sem um segundo jeito de decidir de quem é cada processo.
- **Fechar aba, fechar painel e as confirmações** (RF-1.2, RF-1.6, RF-6.10) não mudam. A lista não fecha abas nem painéis.
- **A barra de abas, a barra de status, os grupos, as fixadas e o arraste** ([PRD-001](prd-001-abas.md), [PRD-002](prd-002-grupos-de-abas.md), [PRD-009](prd-009-barra-de-status.md), [PRD-021](prd-021-abas-fixadas.md), [PRD-022](prd-022-abas-e-grupos-entre-janelas.md)) continuam funcionando com o gerenciador aberto.
- **A sessão** não muda: o gerenciador não é gravado nem restaurado.
- **A geometria do botão do app** ([ADR-0079](../adr/0079-botao-de-menu-do-app.md)) não muda. Ele só ganha uma ação.

## Usuário-alvo

O do [PRD-000](prd-000-visao-de-produto.md), no caso de quem mantém vários processos de longa duração abertos em abas e precisa saber, sem sair do terminal, qual deles está pesando. Quem nunca clica no botão do app não vê nada novo, e o app não mede nada para ele.

## Em uma tela

O botão do app alterna a janela entre o terminal e o gerenciador. A barra de abas continua no lugar:

```
 [◆] ( api ▾ [server] [tests] ) [front]  [+]                       [▣] [⚙]  ─ □ ✕
 ─────────────────────────────────────────────────────────────────────────────────
  CPU                      12%  │  Nome                PID    CPU   Memória  Disco
  ╭───────────────────────────╮ │  ● api · server                 9%   612 MB  0 B/s
  │              ╱╲      ╱╲__ │ │    pwsh.exe          8120   0%    74 MB   0 B/s
  │ ___╱╲___╱╲__╱  ╲____╱    │ │    └ node.exe        9312   9%   538 MB   0 B/s
  ╰───────────────────────────╯ │  ● api · tests                  3%   201 MB  1 MB/s
  Memória               1,4 GB  │    pwsh.exe          7764   0%    71 MB   0 B/s
  ╭───────────────────────────╮ │    └ cargo.exe       5520   3%   130 MB  1 MB/s
  │ ‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾ │ │    front                        0%    80 MB   0 B/s
  ╰───────────────────────────╯ │    pwsh.exe          6608   0%    80 MB   0 B/s
  Disco                 1 MB/s  │    ssh-prod
  ╭───────────────────────────╮ │      processos remotos não são listados
  │                       ╱╲_ │ │
  │ _____________________╱    │ │
  ╰───────────────────────────╯ │  ─────────────────────────────────────────────
                     agora ◀──  │               Monitorar em segundo plano  (○━)
```

A linha mais nova entra pela borda direita de cada gráfico e desliza para a esquerda. No rodapé da lista, a alternância do monitoramento em segundo plano (RF-24.31).

## Requisitos funcionais

### Abrir e fechar

**RF-24.1** — Clicar no **botão do app** alterna a janela entre o **terminal** e o **gerenciador de tarefas**. A ação nova **`task_manager.toggle`** faz o mesmo e **não tem atalho default** (precedente de `group.new_tab`). Com o botão oculto (`[appearance.tabs] show_app_menu_button = false`), a ação é o único caminho.

**RF-24.2** — O gerenciador ocupa a **área de conteúdo inteira** da janela, no lugar da árvore de painéis da aba ativa ([PRD-006](prd-006-paineis-divididos.md)). A barra de abas, os controles da janela e a barra de status continuam visíveis. A barra de status segue mostrando o painel focado da aba ativa, como antes.

**RF-24.3** — Com o gerenciador aberto, o botão do app mostra que o modo está ligado, e não só por matiz (RF-1.14). A aparência desse estado é decisão do ADR visual, com o aval do dono. O nó de acessibilidade do botão passa de "Menu do aplicativo" a "Gerenciador de tarefas", com estado de alternância.

**RF-24.4** — A janela volta ao terminal:
- pelo botão do app ou pela ação `task_manager.toggle`;
- por `Esc`, quando nenhum menu ou diálogo está aberto;
- por **qualquer gesto que ative ou crie uma aba**: clicar numa aba, inclusive na que já é ativa; o `+`; `tab.new`, `tab.next`, `tab.prev` e o acesso por índice (RF-1.11, RF-1.12); restaurar uma sessão na janela atual ([PRD-023](prd-023-restaurar-sessao-nomeada-na-janela-atual.md));
- por "Ir para a aba" (RF-24.17).

**RF-24.5** — Com o gerenciador aberto, **o teclado não chega ao terminal**: nada do que se digita é escrito em painel nenhum. As ações que miram o painel (busca, dividir, colar, copiar, rolagem do scrollback, zoom de fonte) não fazem nada. As ações de aba, grupo, janela, sessão e configurações continuam valendo, com a regra do RF-24.4.

**RF-24.6** — O modo é **da janela** e **não é gravado**. Cada janela tem o seu, e abrir o gerenciador numa não muda as outras. O arranque, uma janela nova e uma janela de sessão nomeada abrem no terminal. Fechar abas pela barra (pelo "x" ou pelo menu), com o gerenciador aberto, mantém o gerenciador aberto.

### Gráficos

**RF-24.7** — A metade **esquerda** do gerenciador tem três **gráficos de linha** empilhados, de altura igual, nesta ordem: **CPU**, **Memória**, **Disco**. Cada gráfico tem um ponto por amostra e cobre os últimos `history_seconds` (RF-24.22). Ele é desenhado **da direita para a esquerda**, como no Gerenciador de Tarefas do Windows: a amostra mais nova entra na **borda direita** e, a cada segundo, a linha inteira desliza para a esquerda, até a amostra mais antiga sair pela borda esquerda. Cada gráfico mostra o **título**, o **valor atual** e o **topo da escala**. Preencher ou não a área sob a linha é decisão visual do ADR, com o aval do dono.

**RF-24.8** — Os três gráficos mostram a **soma dos processos desta janela** (RF-24.14), nunca o total da máquina.

**RF-24.9** — **CPU**: a porcentagem da máquina inteira, em que **100%** é todos os núcleos ocupados. A escala vai de 0 a 100%, fixa. Como a CPU é a diferença entre duas medições, a primeira amostra só aparece no segundo tick depois de o gerenciador abrir.

> **Emendado pelo [ADR-0098](../adr/0098-monitoramento-em-segundo-plano-do-gerenciador.md) (2026-11-10).** Com o monitoramento em segundo plano ligado (RF-24.31), o segundo tick é contado do arranque, não da abertura do gerenciador.

**RF-24.10** — **Memória**: a soma da memória residente dos processos (o *working set* no Windows), em bytes, com unidade legível (MB, GB). A escala é **automática**: o topo é o maior valor visível no período, arredondado para cima a um número redondo.

**RF-24.11** — **Disco**: a soma de bytes **lidos e escritos** por segundo, com unidade legível (B/s, KB/s, MB/s) e escala automática, como a da memória. Não é o espaço ocupado em disco.

**RF-24.12** — O eixo do tempo é o **relógio**. Os segundos em que a janela não mediu nada (o gerenciador estava fechado, RF-24.21) ficam como **lacuna**: a linha se interrompe e **nunca liga as duas pontas** por cima do buraco. Assim, sair e voltar em poucos segundos mantém o histórico, com o vão visível; depois de um período maior que `history_seconds`, o gráfico reabre vazio. O histórico é da janela, vive enquanto ela existe e **nunca é gravado**.

> **Emendado pelo [ADR-0098](../adr/0098-monitoramento-em-segundo-plano-do-gerenciador.md) (2026-11-10).** A lacuna é a falta de medição, não o gerenciador fechado. Com o monitoramento em segundo plano ligado (RF-24.31), a janela continua medindo com o gerenciador fechado, e sair e voltar **não deixa vão**.

### Lista de processos

**RF-24.13** — A metade **direita** do gerenciador é a **lista de processos desta janela**, rolável. Ela abrange **todas as abas da janela** na ordem visual (as fixadas primeiro, depois a trilha), inclusive as abas de grupos colapsados.

**RF-24.14** — Um processo pertence a um **painel** quando é o processo raiz dele ou descende do PID raiz, pelo mesmo critério que o app usa para encerrar a árvore ao fechar a aba ([ADR-0033](../adr/0033-job-object-encerramento-de-processo.md) §3). O que pertence a um painel pertence à aba do painel e, portanto, a esta janela. Nenhum processo de outra janela, nem de fora do Porecatu, aparece.

**RF-24.15** — A lista é **agrupada por aba**:
- uma **linha de aba**, com o título da aba (precedência do RF-1.7), a cor do grupo quando ela está num grupo e a **soma** de CPU, memória e disco dos processos dela;
- abaixo, os **processos em árvore**: o shell raiz e, recuados, os filhos, os netos e assim por diante, cada nível ordenado pelo início do processo;
- numa aba com mais de um painel ([PRD-006](prd-006-paineis-divididos.md)), cada painel contribui com a sua raiz, na ordem dos painéis na tela (da esquerda para a direita, de cima para baixo).

**RF-24.16** — Cada processo mostra **Nome**, **PID**, **CPU** (%, como no RF-24.9), **Memória** e **Disco** (/s). Um nome longo termina em reticências, e o nome inteiro aparece no tooltip da linha.

**RF-24.17** — Uma aba sem processo local aparece com uma **nota** no lugar dos processos, com o texto no catálogo de todos os idiomas:
- **não iniciada** ([ADR-0037](../adr/0037-aba-nao-iniciada.md), `lazy_restore`): a aba inicia no primeiro foco;
- **SSH** ([PRD-020](prd-020-conexoes-ssh.md)): os processos remotos não são listados. O cliente SSH é embutido, então não há processo local;
- **encerrada**: o processo do painel terminou ([ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md)).

Numa aba com painéis de tipos diferentes, a nota vale por painel.

**RF-24.18** — A lista é **atualizada a cada amostra**. Linhas entram e saem conforme os processos nascem e morrem, mas a **linha selecionada** e a **rolagem** se mantêm. Uma linha selecionada cujo processo morreu passa a seleção para a linha de aba dele.

**RF-24.19** — A lista é navegável pelo **teclado**: as setas movem a seleção, `Home` e `End` vão às pontas, `Enter` faz "Ir para a aba" (RF-24.20), e a tecla de menu ou `Shift+F10` abre o menu de contexto da linha. Cada linha tem um nó de acessibilidade com o nome, o PID e os valores; a linha de aba tem o título e a soma.

### Ações

**RF-24.20** — **Ir para a aba**: duplo clique, ou `Enter`, num processo ou numa linha de aba volta ao terminal (RF-24.4), ativa a aba e **foca o painel** daquele processo. Uma aba dentro de um grupo colapsado se comporta como em qualquer outro gesto que a ativa: o grupo expande. O item também aparece no menu de contexto da linha.

**RF-24.21** — **Encerrar processo**: o item do menu de contexto de uma linha de processo abre um **diálogo de confirmação** ([ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), com a largura e a quebra do [ADR-0078](../adr/0078-largura-e-quebra-de-texto-do-dialogo.md)) com o nome do processo, o PID e quantos descendentes vão junto. O foco inicial fica no cancelar, como em todo diálogo do app. Confirmado, o app encerra o processo **e a árvore dele**. A confirmação **sempre** aparece e não é configurável, e a ação não tem atalho.

**RF-24.22** — O **processo raiz** de um painel (o shell) **não pode** ser encerrado pela lista. O item "Encerrar processo" aparece **esmaecido**, nunca ausente, como todo item indisponível ([especificação visual](../design/especificacao-visual.md) §2.16), e o mesmo vale nas linhas de aba. Para encerrar o shell existem fechar aba e fechar painel, com as confirmações que já têm.

**RF-24.23** — Encerrar um filho não muda nada no painel além disso: o shell continua vivo, e o que o programa encerrado escreveu fica no terminal. Um processo que já terminou entre a abertura do diálogo e a confirmação não gera erro nem aviso: nada acontece, e a linha some na próxima amostra.

### Medição

**RF-24.24** — A medição acontece **uma vez por segundo**, numa thread, **nunca na main thread** ([ADR-0007](../adr/0007-modelo-de-threading.md)), e **só enquanto ao menos uma janela está com o gerenciador aberto**. Com todos os gerenciadores fechados, o app não mede nada, e o custo é zero. O resultado chega à janela como **dado chaveado por janela**, como os resultados do Git ([ADR-0052](../adr/0052-sincronizacao-com-o-remoto-do-git.md)), nunca como comando.

> **Emendado pelo [ADR-0098](../adr/0098-monitoramento-em-segundo-plano-do-gerenciador.md) (2026-11-10).** "Só enquanto ao menos uma janela está com o gerenciador aberto" vale **com o monitoramento em segundo plano desligado**, o padrão. Ligado (RF-24.31 a RF-24.34), a medição acontece durante toda a execução, para todas as janelas. O ritmo, a thread e o dado chaveado não mudam.

**RF-24.25** — Uma aba que muda de janela ([PRD-022](prd-022-abas-e-grupos-entre-janelas.md)) leva os processos dela para a lista e para os gráficos da janela de destino **a partir da próxima amostra**. O histórico já registrado das duas janelas não é recalculado.

### Configuração

**RF-24.26** — Uma chave **`[task_manager] history_seconds`**, com default **60**, define o período dos gráficos. A faixa é de **30 a 600**. Um valor fora dela é **ajustado ao limite mais próximo**, e o app informa uma vez que o ajustou, como faz com o intervalo do Git ([PRD-013](prd-013-sincronizacao-com-o-remoto-do-git.md)). O resto da configuração continua valendo.

**RF-24.27** — A chave **vale na hora** ([ADR-0030](../adr/0030-escopo-do-hot-reload.md)), sem reiniciar. Aumentar o período abre espaço à esquerda, e diminuir descarta as amostras mais antigas.

**RF-24.28** — Na tela de configurações ([PRD-016](prd-016-tela-de-configuracoes.md)), um grupo novo, **Gerenciador de tarefas**, mostra o período como **campo numérico**, com rótulo e descrição no catálogo de textos. Isso emenda o catálogo do RF-16.11 na mesma leva.

**RF-24.29** — Todo texto novo (títulos dos gráficos, cabeçalhos das colunas, notas, itens de menu, diálogo, nós de acessibilidade, o grupo da tela de configurações) vive no catálogo de textos, em **todos** os arquivos de `locales/` ([ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md)), com a terminologia **"abas"**.

### Plataforma

**RF-24.30** — No **Windows**, a árvore de cada painel é a do [ADR-0033](../adr/0033-job-object-encerramento-de-processo.md), com Job Object e varredura. No **Linux** e no **macOS**, onde não há Job, ela é só a varredura a partir do PID raiz, e um processo cujo pai intermediário já morreu pode não aparecer, com o mesmo ponto cego que o ADR-0033 já registra. A verificação nessas duas plataformas é dívida, como em todo recurso fora de fase.

### Monitoramento em segundo plano

Acrescentado pela emenda de 2026-11-10 ([ADR-0098](../adr/0098-monitoramento-em-segundo-plano-do-gerenciador.md)).

**RF-24.31** — O gerenciador tem, no **canto inferior direito**, logo **abaixo da lista de processos**, uma **alternância** com o rótulo "Monitorar em segundo plano". **Desligada** (o padrão), a medição segue o RF-24.24: só com o gerenciador aberto. **Ligada**, o app mede **o tempo todo**, com o gerenciador aberto ou fechado, e os gráficos guardam o histórico enquanto o usuário usa as abas. A aparência é decisão do ADR, sem valor novo, com o aval visual do dono pendente.

**RF-24.32** — A escolha é a chave **`[task_manager] background_monitoring`**, booleana, com default **`false`**. Clicar na alternância **grava a chave** no `porecatu.toml`, pela mesma escrita por chave da tela de configurações ([ADR-0058](../adr/0058-escrita-do-arquivo-de-configuracao.md)), sem mexer no resto do arquivo. A alternância mostra sempre o valor em vigor: uma edição à mão no arquivo aparece nela, e uma gravação que falha a deixa como estava.

**RF-24.33** — A chave vale para o **app inteiro**: o estado da alternância é o mesmo em todas as janelas, e, ligada, **toda janela** acumula o próprio histórico (RF-24.8, RF-24.12), aberta ou não.

**RF-24.34** — Com a chave ligada, a medição começa **no arranque**, e o gerenciador já abre com o gráfico do período inteiro. Ela **vale na hora** ([ADR-0030](../adr/0030-escopo-do-hot-reload.md)): ligar com o app aberto começa a medir no segundo seguinte; desligar com todos os gerenciadores fechados para a medição no segundo seguinte, e o histórico registrado fica, com a lacuna do RF-24.12 dali em diante.

**RF-24.35** — Uma gravação que falha (arquivo só de leitura, config inválida em disco) mostra um **aviso** com o texto do catálogo, e nada mais muda.

**RF-24.36** — Na tela de configurações ([PRD-016](prd-016-tela-de-configuracoes.md)), o grupo **Gerenciador de tarefas** (RF-24.28) ganha a mesma opção, como **alternância**, antes do período, com rótulo e descrição no catálogo. A descrição diz o custo: uma varredura de processos por segundo enquanto o app está aberto. É também o caminho pelo teclado: a alternância do gerenciador não tem atalho, e as teclas do gerenciador continuam só na lista (RF-24.19).

**RF-24.37** — A alternância tem um **nó de acessibilidade** de interruptor, com o rótulo, o estado ligado ou desligado e a ação de clicar, para o leitor de tela alterná-la sem o mouse.

## Cenários

```gherkin
Cenário: abrir e fechar pelo botão
  Dado uma janela com duas abas
  Quando o usuário clica no botão do app
  Então a área do terminal dá lugar ao gerenciador de tarefas
  E a barra de abas continua visível
  Quando o usuário clica de novo no botão do app
  Então o terminal da aba ativa volta, como estava

Cenário: o gráfico anda da direita para a esquerda
  Dado o gerenciador aberto há dez segundos
  Quando passa mais um segundo
  Então a amostra nova aparece na borda direita de cada gráfico
  E as anteriores deslizam uma posição para a esquerda

Cenário: voltar ao terminal por uma aba
  Dado o gerenciador aberto
  Quando o usuário clica na aba "front"
  Então a aba "front" fica ativa
  E a janela mostra o terminal de "front"

Cenário: o teclado não chega ao terminal
  Dado o gerenciador aberto
  Quando o usuário digita "ls" e Enter
  Então nada é escrito em nenhum painel

Cenário: cada janela vê só os seus processos
  Dado a janela A com uma aba rodando "npm start"
  E a janela B com uma aba rodando "cargo build"
  Quando o usuário abre o gerenciador na janela B
  Então a lista mostra "cargo" e não mostra "node"
  E os gráficos somam só os processos da janela B

Cenário: aba SSH
  Dado uma aba SSH conectada
  Quando o usuário abre o gerenciador
  Então a linha da aba mostra a nota de que os processos remotos não são listados

Cenário: aba não iniciada
  Dado [session] lazy_restore = true e uma aba restaurada nunca visitada
  Quando o usuário abre o gerenciador
  Então a linha da aba mostra a nota de aba não iniciada
  E abrir o gerenciador não inicia a aba

Cenário: ir para um processo numa aba de grupo colapsado
  Dado o grupo "api" colapsado, com a aba "server" de dois painéis
  E o node.exe rodando no painel da direita
  Quando o usuário dá duplo clique no node.exe na lista
  Então o grupo "api" expande
  E a aba "server" fica ativa com o painel da direita focado
  E a janela mostra o terminal

Cenário: encerrar um filho
  Dado o node.exe rodando sob o pwsh.exe de uma aba
  Quando o usuário escolhe "Encerrar processo" no node.exe
  Então um diálogo pede confirmação com o nome, o PID e a contagem de descendentes
  E o foco está no cancelar
  Quando o usuário confirma
  Então o node.exe e os descendentes dele terminam
  E o pwsh.exe continua vivo, com o prompt de volta

Cenário: o shell raiz não é encerrável pela lista
  Quando o usuário abre o menu de contexto do pwsh.exe raiz de um painel
  Então "Encerrar processo" aparece esmaecido

Cenário: lacuna depois de sair e voltar
  Dado history_seconds = 60 e o gerenciador aberto há 30 segundos
  Quando o usuário volta ao terminal por 10 segundos e reabre o gerenciador
  Então o gráfico mostra as amostras anteriores à esquerda
  E um vão de 10 segundos sem linha entre elas e as novas

Cenário: o período muda sem reiniciar
  Dado o gerenciador aberto com history_seconds = 60
  Quando o usuário muda history_seconds para 120 na tela de configurações e salva
  Então os gráficos passam a cobrir 120 segundos
  E nenhum aviso de "reinicie o app" aparece

Cenário: valor fora da faixa
  Dado history_seconds = 5
  Quando o app carrega a config
  Então o período usado é 30 segundos
  E o app informa uma vez que ajustou o valor

Cenário: botão oculto
  Dado show_app_menu_button = false
  E task_manager.toggle ligada a um atalho
  Quando o usuário aperta o atalho
  Então o gerenciador abre

Cenário: aba que muda de janela leva os processos
  Dado o gerenciador aberto nas janelas A e B
  Quando o usuário arrasta a aba "server" da janela A para a janela B
  Então, na próxima amostra, os processos de "server" saem da lista de A
  E entram na lista de B

Cenário: gerenciador fechado não mede
  Dado background_monitoring = false
  E nenhuma janela com o gerenciador aberto
  Então o app não faz nenhuma varredura de processos por causa do gerenciador

Cenário: monitoramento em segundo plano mantém o histórico
  Dado o gerenciador aberto com a alternância "Monitorar em segundo plano" desligada
  Quando o usuário liga a alternância
  Então o porecatu.toml passa a ter background_monitoring = true
  Quando o usuário volta ao terminal por 10 segundos e reabre o gerenciador
  Então o gráfico mostra os 10 segundos sem nenhum vão

Cenário: arranque com o monitoramento ligado
  Dado background_monitoring = true e history_seconds = 60
  Quando o app abre e o usuário só abre o gerenciador 90 segundos depois
  Então os gráficos já cobrem os 60 segundos inteiros

Cenário: desligar com o gerenciador fechado para de medir
  Dado background_monitoring = true e nenhuma janela com o gerenciador aberto
  Quando o usuário desliga a opção na tela de configurações e salva
  Então, a partir do segundo seguinte, o app não faz nenhuma varredura de processos

Cenário: a alternância vale para todas as janelas
  Dado as janelas A e B
  Quando o usuário liga a alternância no gerenciador da janela A
  Então o gerenciador da janela B mostra a alternância ligada
  E o histórico da janela B passa a ser registrado com o gerenciador dela fechado

Cenário: gravação que falha
  Dado o porecatu.toml só de leitura
  Quando o usuário clica na alternância
  Então um aviso informa que a configuração não foi gravada
  E a alternância continua desligada
```

## Fora de escopo

Cada item é decisão, não esquecimento.

- **Rede e GPU**, por processo e também no total. Rede por processo, no Windows, só sai por ETW, com privilégio e muito código; GPU tem uma API diferente em cada sistema e fabricante. O dono dispensou as duas.
- **Totais do sistema** (a CPU ou a memória da máquina inteira). O gerenciador responde "qual das **minhas** abas está pesando", e os gráficos somam a janela (RF-24.8).
- **Processos fora desta janela**, de outras janelas do Porecatu ou de fora do app.
- **Processos remotos de uma aba SSH.** Listá-los exigiria rodar comandos no servidor, o que é outro recurso.
- **Ordenar por coluna, filtrar e buscar** na lista. A ordem é a das abas e a da árvore (RF-24.15).
- **Prioridade, afinidade, suspender e retomar** um processo.
- **Gráfico por processo ou por aba.** Os gráficos são da janela, e a lista mostra os números por linha.
- **Histórico gravado ou exportado.** O histórico vive com a janela (RF-24.12).
- **O gerenciador como aba ou janela própria.** O dono escolheu um modo da janela (RF-24.1, RF-24.6).
- **O "menu do app"** que o [ADR-0079](../adr/0079-botao-de-menu-do-app.md) previa para o botão. O botão passa a ser do gerenciador, e um menu do app, se vier, precisa de requisito próprio.
- **Medir em segundo plano**, com o gerenciador fechado, para os gráficos já abrirem cheios. Custaria uma varredura de processos por segundo o tempo todo (RF-24.24).

  > **Emendado pelo [ADR-0098](../adr/0098-monitoramento-em-segundo-plano-do-gerenciador.md) (2026-11-10).** Saiu do fora de escopo: medir em segundo plano é **opção**, desligada por padrão (RF-24.31 a RF-24.37). Continuam fora a escolha por janela e um atalho para a alternância.
- **Encerrar o shell raiz pela lista** (RF-24.22).
- **Intervalo de amostragem configurável.** O período é configurável, e a amostra é de um segundo.

### O que este documento **não** contradiz

- O [ADR-0033](../adr/0033-job-object-encerramento-de-processo.md) §3 diz que a varredura roda "nunca continuamente". Isso valia para a limpeza e a contagem que ele decidia. A medição a 1 Hz, só com o gerenciador aberto, é outro uso, e o ADR novo que a decide registra a revisão naquele parágrafo, com um blockquote. O critério de pertença (Job mais varredura) é o mesmo.
- O [ADR-0079](../adr/0079-botao-de-menu-do-app.md) decidiu o botão **sem ação**, "ponto de entrada do menu do app quando houver um". Este documento lhe dá a ação de alternar o gerenciador. A geometria e a aparência em repouso não mudam.
- A [interface do v1 está fechada](../adr/0032-interface-do-v1-fechada.md): a anatomia do gerenciador e o estado ligado do botão entram por ADR, com o aval visual do dono. Nenhuma cor nem dimensão é inventada. As cores dos gráficos entram no `Config`, em `theme::mergeable_fields` e nas duas `thm_*.toml`, no mesmo PR.
- O `porecatu-render` não tem primitiva de linha (só `Quad`, `RoundedQuad`, `Text`, `Image` e o recorte). O gráfico de linha pede uma **primitiva genérica nova**, uma polilinha com espessura e, se o ADR visual decidir, a área preenchida sob ela. Ela não conhece o domínio, então respeita a fronteira do render, e não é passe de filtro, então não esbarra no [ADR-0068](../adr/0068-efeito-acrilico-no-fundo-do-terminal.md). Entra por ADR próprio.
- O [PRD-000](prd-000-visao-de-produto.md) não lista um gerenciador de processos entre os não-objetivos.

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Gestos para ver o consumo das abas da janela | **um** (o botão do app) |
| Varreduras de processo com todos os gerenciadores fechados e `background_monitoring = false` (o padrão) | **zero** |
| Vãos no gráfico por sair e voltar ao gerenciador com `background_monitoring = true` | **zero** |
| Varreduras de processo na main thread | **zero** |
| Processos de outra janela, ou de fora do Porecatu, na lista | **zero** |
| Critérios diferentes para "processo desta aba" entre o gerenciador e o fechamento de aba | **zero**, com a mesma árvore do ADR-0033 |
| Diálogos para encerrar um processo | **um**, sempre |
| Dependências novas no workspace | **zero** (o `sysinfo` já existe e só deixa de ser exclusivo do Windows) |
