# ADR-0017 — Ciclo de vida e identidade da aba

**Status:** Superseded by [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md) (parcial — só a seção 3, sobre o critério de confirmação do RF-1.6) e por [ADR-0037](0037-aba-nao-iniciada.md) (parcial — só a seção 6, que enumerava `Running` e `Exited` como os dois estados possíveis da aba)
**Data:** 2026-08-27
**Relacionados:** ADR-0004, ADR-0005, ADR-0007, ADR-0008, ADR-0012, ADR-0014, ADR-0015, PRD-001, PRD-003, PRD-010

## Contexto

A F2 é a primeira fase em que uma aba **nasce, muda de nome e morre** por vontade do usuário. Sete requisitos aprovados descrevem esse ciclo, e cinco deles nomeiam um mecanismo que não existe — ou que outro ADR aceito já descartou por escrito.

O que torna isso decisão de produto, e não detalhe de implementação, é a F1: ela descobriu duas coisas que a documentação não previa, e as duas caem exatamente aqui.

### O que cada requisito pede e o que falta

| Requisito | Pede | O que falta |
|---|---|---|
| RF-1.1 | aba nova abre no `cwd` da aba ativa | `cwd` não é conhecido: [arquitetura.md](../arquitetura.md) seção 4.3 registra que **OSC 7 não é capturado** e deferiu para a F5 |
| RF-1.2 | fechar aguarda **EOF** do shell, para não perder a última saída | no Windows o pipe do ConPTY **não emite EOF** quando o processo hospedado sai; `Exit` vem do `try_wait`. E nenhum documento define timeout ou escape |
| RF-1.3 | aba com saída ≠ 0 permanece aberta exibindo o código | [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) põe essa nota na *"primeira linha no grid"*, que é onde ela não pode estar; e `porecatu-term` não expõe escrita no grid. Falta também o estado visual da aba morta |
| RF-1.6 | confirmar ao fechar aba com **processo em primeiro plano diferente do shell** | nada decide como detectar isso |
| RF-1.7 | precedência de título: customizado → OSC 0/2 → **processo em primeiro plano** → shell | mesma dependência inexistente |
| RF-1.4 / `app.quit` | fechar a última janela grava a sessão de forma síncrona | `porecatu-session` tem uma linha; sessão é F5 |
| PRD-001 métricas | 50 abas sem degradação perceptível | `Terminal::shutdown` é serial, com `SHUTDOWN_TIMEOUT` de 2 s por aba |

### A contradição de três documentos

RF-1.6 e o terceiro nível do RF-1.7 exigem saber qual processo está em primeiro plano na aba. Dois ADRs aceitos já fecharam a porta pela qual isso passaria:

- [ADR-0005](0005-persistencia-de-sessao.md): *"Decisão v1: **usar sempre o shell diretamente spawnado**, sem varrer a árvore de processos. Varrer descendentes é caro, ambíguo e específico de plataforma."*
- [ADR-0008](0008-teclas-e-roteamento-de-input.md), ao descartar bindings adaptativos: *"Descartada por fragilidade — exigiria varrer a árvore de processos, teria comportamento diferente por plataforma."*

Três documentos aprovados em desacordo, e o RF-1.6 é **cenário de aceite da F2**. Não há como declarar a fase concluída sem resolver isso.

### Por que agora

Todos os sete pontos atravessam a fronteira de `porecatu-term`, e três deles mudam a forma de tipos que a F1 já construiu (`TermEvent`, `Terminal::shutdown`, o estado da aba em `porecatu-ui`). Descobrir na etapa de overflow que a aba precisa de um estado `Exited` é refazer o modelo de domínio depois de a barra de abas já desenhar sobre ele.

## Decisão

**O ciclo de vida da aba se apoia só em fatos que o app já observa: o que o terminal emite e o que o SO confirma. Nada de inspeção de árvore de processos.**

### 1. `cwd` vem de OSC 7, capturado a partir da F2

`porecatu-term` ganha uma captura de OSC 7 própria, emitindo `TermEvent::Cwd`. Isso **antecipa** o que a seção 4.3 da arquitetura havia deferido para a F5; a F5 passa a consumir um evento que já existe, em vez de introduzi-lo.

*Correção de implementação (F2): o `osc_dispatch` do `vte` descarta OSC 7 antes de chamar qualquer método de `Handler` — não existe gancho de `Handler` para interceptar essa sequência específica sem forkar o crate. A captura roda como um segundo parser `vte::Perform`, independente e sem efeito colateral no motor, sobre os mesmos bytes — não um `Handler` que envolve o `Term`. O resultado observável é o mesmo desta decisão: `Term` segue intocado, `TermEvent::Cwd` sai capturado. Ver docs/arquitetura.md seção 4.3.*

`porecatu-ui` guarda o último `cwd` conhecido por aba. `tab.new` e `window.new` herdam dele. Sem OSC 7 — shell sem integração, típico do Windows —, o fallback é `startup_directory`, que o [ADR-0005](0005-persistencia-de-sessao.md) já documenta como **comportamento esperado, não bug**.

A precedência é: `cwd` da aba ativa → `startup_directory` → home. Nenhum nível consulta o SO.

> **Correção factual, 2026-09-02.** Os dois últimos níveis colapsaram na implementação: `startup_directory` é o **diretório home do usuário** (`dirs::home_dir()`). Antes era `std::env::current_dir()`, o diretório de onde o Porecatu foi lançado — o que fazia toda aba nova abrir na pasta do binário quando não havia `cwd` conhecido por OSC 7. Quando `porecatu-config` existir (F4), a chave passa a poder apontar outro lugar, e a precedência volta a ter três níveis distintos.
>
> No mesmo movimento: **`group.new_tab` herda o `cwd` da última aba do grupo de destino**, não o da aba ativa. Criar aba num grupo pelo "+" dele, estando com outra aba em foco, herdaria um diretório sem relação com o grupo em que a aba nasce.

### 2. RF-1.7 perde o nível do processo em primeiro plano

A precedência de título passa a ser, em definitivo:

```
título customizado (RF-1.8)  →  OSC 0 / OSC 2  →  nome do shell
```

O nível intermediário sai. Mantê-lo exigiria a varredura que ADR-0005 e ADR-0008 descartaram, e um requisito que só é atendível violando duas decisões aceitas não é requisito — é dívida mal registrada.

Na prática o usuário perde pouco: programas de tela cheia (`vim`, `htop`, `less`, `ssh`) emitem OSC 0/2 com o próprio nome, e o nível 2 já os cobre. O que descobre é o comando não interativo de longa duração, que não emite título nenhum — e para esse caso o indicador de atividade do RF-1.20 é a resposta certa, não o título.

### 3. RF-1.6 usa o modo do terminal, não o processo

A confirmação ao fechar dispara quando **a aba está em tela alternativa ou tem reporte de mouse ligado** — ou seja, quando um programa de tela cheia tomou o terminal. É a mesma informação que `TermModes` já carrega (`alt_screen`, `mouse_reporting`), consultada pelo acessor barato que a seção 4.3 da arquitetura já prevê para o caminho de input. Custo zero, idêntica nas três plataformas, sem tocar no SO.

A chave `confirm_close_with_process` continua governando, com o default `true` do arquivo de exemplo, e a origem passa a ser este ADR.

O que isso **cobre**: `vim`, `nvim`, `htop`, `less`, `man`, `fzf`, `tmux`, e `ssh` para um host onde qualquer um desses está aberto. É a classe de perda de trabalho que o requisito nomeia.

O que isso **não cobre**, e é registrado aqui para não ser lido como bug: comando não interativo de longa duração (`cargo build`, `rsync`) e `ssh` parado num prompt remoto. Nos dois casos a aba fecha sem perguntar. O RF-1.3 é a rede de segurança parcial — processo que morre com código ≠ 0 mantém a aba aberta —, e o `confirm_close_window` do [ADR-0015](0015-multiplas-janelas.md) protege o caso agregado.

### 4. Encerramento sem EOF, com confirmação e sem bloqueio

A regra do RF-1.2 — não perder a última saída — vale; o mecanismo muda, porque o ConPTY não oferece o que o requisito nomeia.

Fechar uma aba:

1. Drena o que já chegou do PTY e aplica no grid.
2. Sinaliza o processo e **arma a espera pela confirmação da thread de observação** — o canal `killed` que a F1 já construiu, não EOF.
3. A aba sai da barra imediatamente. A confirmação é aguardada **fora da main thread**.

A main thread nunca bloqueia por causa de uma aba fechando. Consequência direta: fechar uma janela com 50 abas custa uma volta de sinalização, não 50 × timeout. O `SHUTDOWN_TIMEOUT` deixa de ser latência do caminho comum e volta a ser o que o nome diz — uma rede de segurança.

O deadlock do `ClosePseudoConsole` documentado na seção 2 da arquitetura fica intacto: o pseudo-console continua não sendo fechado, e o `PtyHandle` continua sendo vazado de propósito.

### 5. Nota no grid: a posição segue o fato

O canal 2 do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) continua sendo "escrita no grid daquela aba". O que este ADR corrige é a posição, que ali ficou fixa em "primeira linha":

| Fato | Onde | Por quê |
|---|---|---|
| RF-3.10 — diretório gravado inexistente | primeira linha | acontece na **abertura** da aba, antes de qualquer saída |
| RF-1.3 — código de saída do processo | após a última linha de saída | acontece no **fim**; na primeira linha estaria fora da vista ou já rolada |

Quem escreve é `porecatu-term`, por um método de injeção que passa os bytes pelo próprio parser — o grid é do motor, e escrever nele por fora furaria a fronteira da seção 4. A marcação visual é a do ADR-0014: `#5ed3bc`, nunca imitando prompt.

### 6. Estado `Exited` da aba

O RF-1.3 mantém a aba aberta depois de o processo morrer. Isso é um estado, e ele precisa existir no modelo:

- Sem PTY. Nada é escrito, nenhuma thread sobrevive.
- Aceita rolagem, seleção e cópia — é para isso que ela ficou aberta.
- **Não aceita input.** Tecla digitada numa aba `Exited` não vai a lugar nenhum.
- Título congela no que era.
- Não participa dos indicadores de atividade e campainha (RF-1.20, RF-1.21): não há mais saída possível.
- Conta para a navegação e para o índice do RF-1.12 como qualquer aba.
- **Não é restaurada** pela sessão na F5 — restaurar uma aba morta restauraria um erro passado.

### 7. `app.quit` e RF-1.4 na F2

Na F2 as duas encerram sem gravar sessão, porque não há sessão. O gancho é explícito e único: o ponto em que a última janela fecha chama a gravação síncrona, e na F2 esse ponto é um no-op documentado. A F5 preenche o no-op e não precisa procurar onde.

O catálogo de ações registra isso na caixa "Estado na F2", no mesmo padrão das caixas de estado da F1 em [`acoes.md`](../reference/acoes.md).

## Alternativas consideradas

### Detecção de processo em primeiro plano por plataforma

Em Unix é barato e exato: `tcgetpgrp` no descritor do master do PTY devolve o grupo de processos em primeiro plano, sem varrer árvore nenhuma, e é o que emuladores maduros fazem. O ConPTY não tem equivalente — o pseudo-console não expõe o grupo em primeiro plano, e o caminho restante seria justamente a varredura de descendentes.

Descartada porque produziria RF-1.6 e RF-1.7 com **comportamento diferente por plataforma** num projeto que decidiu ser consistente nas três ([ADR-0014](0014-superficie-de-aviso-e-dialogo.md), motivo 3 da rejeição de diálogo nativo), e porque o nível 3 do RF-1.7 passaria a existir em Linux e macOS e não em Windows — um título de aba que muda de regra conforme o sistema. Reabrir isso exige ADR novo; a decisão aqui é a do modo do terminal, uniforme.

### Varrer a árvore de processos nas três plataformas

Resolveria RF-1.6 e RF-1.7 exatamente como escritos. Descartada porque supersederia duas decisões aceitas ([ADR-0005](0005-persistencia-de-sessao.md) e [ADR-0008](0008-teclas-e-roteamento-de-input.md)) para ganhar um nível de precedência de título e uma heurística de confirmação, com custo por plataforma e comportamento ambíguo em shell aninhado — que é exatamente o motivo pelo qual as duas a descartaram.

### Deixar RF-1.6 fora da F2

Menos trabalho, e o diálogo do ADR-0014 continuaria exercitado pelo `confirm_close_window`. Descartada porque o RF-1.6 é o requisito que o ADR-0014 cita como razão de o diálogo ser da F2, e porque fechar `vim` por engano é a perda de trabalho mais comum num emulador com abas — sair da fase sem nenhuma proteção seria pior que uma proteção parcial e documentada.

### Confirmar sempre que a aba tiver processo filho vivo

Uniforme, trivial, e sem inspeção nenhuma. Descartada porque a aba **sempre** tem processo filho vivo — é o shell. Confirmação em todo fechamento treina o usuário a apertar Enter sem ler, que é o oposto do que o RF-10.18 quer.

### Adiar OSC 7 para a F5 e abrir toda aba em `startup_directory`

Mantém a F2 fora de `porecatu-term`. Descartada porque o RF-1.1 é cenário de aceite da F2 e o `window.new` do [ADR-0015](0015-multiplas-janelas.md) repete a herança pela mesma razão: *"herdar o diretório é o comportamento que economiza mais digitação no dia a dia"*. Abrir toda aba no mesmo lugar é a limitação que o usuário percebe na primeira hora. O custo real é um parser auxiliar sobre os mesmos bytes, não um motor novo.

### Nota do RF-1.3 na primeira linha, como o ADR-0014 escreveu

Seria zero trabalho. Descartada porque não funciona: o código de saída chega depois de a saída do programa já ter rolado a primeira linha para fora da viewport, e o usuário fecharia a aba sem nunca ver o motivo. A informação existe para ser lida no momento em que ela aparece.

### `join` nas threads no fechamento da aba

O caminho intuitivo. Descartada pelo mesmo motivo que a seção 2 da arquitetura já registra para o fechamento da janela: a thread de leitura está parada num `read()` síncrono que não retorna, e dar `join` nela bloquearia a main thread — violando a regra central do [ADR-0007](0007-modelo-de-threading.md) justamente na operação mais frequente da F2.

## Consequências

### Positivas

- Sete requisitos deixam de depender de mecanismo inexistente; a F2 pode fechar seu critério de saída.
- A contradição entre PRD-001, ADR-0005 e ADR-0008 é resolvida sem superseder nenhum dos dois ADRs.
- `TermEvent::Cwd` nasce na F2 e a F5 recebe a sessão com o dado já disponível — a ordem inversa custaria mexer no engine duas vezes.
- Fechar aba deixa de ser operação bloqueante, o que é pré-requisito real da métrica de 50 abas.
- A confirmação do RF-1.6 usa dado que o app já tem, com custo zero e comportamento idêntico nas três plataformas.
- O estado `Exited` dá casa a um comportamento que o RF-1.3 exigia e que nenhum documento modelava.

### Negativas

- RF-1.6 cobre menos que o texto original prometia: comando não interativo de longa duração e `ssh` em prompt remoto fecham sem perguntar.
- RF-1.7 perde um nível de precedência. Aba rodando comando que não emite título mostra o nome do shell.
- `porecatu-term` ganha um parser auxiliar de OSC 7, que é código a mais na fronteira mais crítica do projeto e mais uma coisa a reescrever se o motor VT for trocado. *(Correção de implementação, F2: é um segundo `vte::Perform` sobre os mesmos bytes, não um `Handler` envolvendo o `Term` — ver a nota da decisão.)*
- O estado `Exited` é um caso especial em toda operação de aba — input, indicadores, sessão —, e cada um precisa de teste.
- Encerramento assíncrono significa que a aba desaparece da barra antes de o processo estar comprovadamente morto. Se a confirmação nunca vier, o app carrega um processo órfão até sair.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Usuário fechar aba com `cargo build` rodando e não ser avisado | Alta | Baixo | RF-1.3 mantém a aba aberta quando o processo morre com código ≠ 0; indicador de atividade do RF-1.20 sinaliza que há saída nova |
| `alt_screen` dar falso positivo e confirmar sem necessidade | Média | Baixo | `confirm_close_with_process = false` desliga; o falso positivo é o lado seguro do erro |
| Shell sem OSC 7 fazer toda aba abrir em `startup_directory` no Windows | Alta | Médio | Convite à integração de shell do RF-3.1, já previsto no ADR-0014 e no ADR-0005 |
| Captura própria engolir OSC que o `Term` deveria tratar | Média | Alto | Risco eliminado pela implementação: o parser auxiliar **não** fica no caminho do motor, ele só observa os mesmos bytes, então nada pode ser engolido. Teste golden com OSC 0, 2, 4, 8 e 52 atravessando cobre isso |
| Confirmação de morte não chegar e vazar processo | Baixa | Médio | `SHUTDOWN_TIMEOUT` volta a ser rede de segurança; o SO reclama as handles quando o Porecatu sai, como a seção 2 da arquitetura já registra |
| Aba `Exited` aceitar input por esquecimento em algum caminho | Média | Baixo | O estado é verificado num ponto só, na entrada do roteamento de input; teste unitário |
| Alguém reintroduzir o nível 3 do RF-1.7 lendo o PRD-001 ao pé da letra | Média | Médio | Nota de reconciliação no próprio RF-1.7 apontando para este ADR |
