# ADR-0012 — Identificação do terminal: `TERM` e capacidades anunciadas

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** ADR-0002, ADR-0004, ADR-0005, ADR-0013, PRD-003, PRD-005

## Contexto

O [`porecatu.example.toml`](../config/porecatu.example.toml) diz, no comentário de `[shell.env]`, que *"TERM é definido pelo app"* — e nunca diz qual valor. É uma lacuna pequena no texto e grande na consequência: `TERM` é como todo programa do ecossistema Unix descobre o que o terminal sabe fazer.

O valor decide, sem intermediários:

- se o `vim` usa 256 cores ou 8;
- que bytes o `readline` espera das setas e das teclas de função;
- se o `tmux` acredita que pode usar cores, itálico, sublinhado colorido;
- se o `ncurses` acha que o terminal existe (`TERM` desconhecido faz o `htop` recusar-se a abrir).

Escolher errado não produz um bug localizado: produz uma cauda longa de comportamento estranho em programas diferentes, exatamente o tipo de problema que o [ADR-0002](0002-motor-vte.md) decidiu não perseguir.

Há uma segunda lacuna do mesmo tipo. Vários documentos aprovados dependem de sequências de escape específicas — OSC 7 para o `cwd` ([ADR-0005](0005-persistencia-de-sessao.md)), OSC 0/2 para o título (RF-1.7), DECSCUSR para a forma do cursor (RF-5.25), bracketed paste ([ADR-0008](0008-teclas-e-roteamento-de-input.md)) — mas nenhum documento reúne **o conjunto completo do que é honrado e do que não é**. Sem essa lista, a F1 descobre o escopo por tentativa, e cada sequência não tratada aparece como caractere de lixo na tela.

Terceira força, específica de produto: o RF-3.1 exige que o app **detecte a ausência de OSC 7** e ofereça o snippet de integração do shell. Para o usuário escrever esse hook condicionalmente — emitir OSC 7 só quando estiver no Porecatu — o app precisa se identificar de alguma forma que um script de shell consiga testar. `TERM` não serve para isso, porque será um valor genérico.

## Decisão

**Anunciar `TERM=xterm-256color`.** Identificar o app por variáveis próprias, e honrar um conjunto fechado e declarado de sequências.

### Ambiente injetado no spawn

Definido por `porecatu-pty` no spawn de todo shell ([ADR-0004](0004-pty-cross-platform.md)), antes das variáveis de `[shell.env]` — que podem sobrescrevê-las, porque a config do usuário é soberana:

| Variável | Valor | Para quê |
|---|---|---|
| `TERM` | `xterm-256color` | capacidades, via terminfo |
| `COLORTERM` | `truecolor` | convenção de fato para anunciar 24 bits, que o terminfo do xterm não descreve |
| `TERM_PROGRAM` | `porecatu` | identificação do app |
| `TERM_PROGRAM_VERSION` | versão do binário | identificação de versão |

`TERM_PROGRAM` / `TERM_PROGRAM_VERSION` é a convenção que Terminal.app, iTerm2 e VS Code já usam, e por isso é o que scripts de integração de shell existentes testam. É o gancho do RF-3.1: o snippet oferecido ao usuário pode ser condicionado a `$TERM_PROGRAM = "porecatu"` sem depender de `TERM`.

### Por que `xterm-256color` e não um terminfo próprio

Um terminfo `porecatu` seria mais honesto sobre capacidades reais. Custa caro justamente onde mais dói:

**Sob SSH, o host remoto não tem a entrada.** `TERM` viaja com a conexão; o servidor consulta o terminfo *dele*. Um valor que só existe na máquina local produz `unknown terminal type` no outro lado — e o usuário não tem como instalar terminfo em todo host que acessa. É o atrito conhecido do Alacritty, relatado desde sempre, e não é um problema que se resolva com documentação.

Anunciar `xterm-256color` é a escolha do Windows Terminal e do VS Code, pelo mesmo motivo. O custo é anunciar capacidades que talvez não tenhamos com fidelidade total — aceitável, porque o Porecatu **não compete em conformidade VT** ([ADR-0002](0002-motor-vte.md)) e usa o motor do Alacritty, que cobre bem o que o terminfo do xterm descreve.

### Capacidades honradas no v1

Lista fechada. Sequência fora dela é consumida e descartada pelo parser, nunca desenhada como lixo na tela.

> **Emenda, 2026-09-04.** A linha do OSC 8 dizia "não — F6". O [ADR-0042](0042-hyperlinks-osc-8.md) a virou na abertura da F6: a sequência passa a ser reconhecida, o URI viaja como span ao lado do snapshot, e **apenas quatro esquemas são aceitos** — `http`, `https`, `mailto` e `file`, este último revelado no gerenciador de arquivos e nunca entregue ao handler por extensão. A lista de sequências continua fechada; o que mudou foi o valor de uma linha dela.

| Sequência | v1 | Origem da exigência |
|---|---|---|
| Bracketed paste (modo 2004) | sim | [ADR-0008](0008-teclas-e-roteamento-de-input.md) — obrigatório, não opcional |
| OSC 0 / OSC 2 — título | sim | PRD-001 RF-1.7 |
| OSC 7 — diretório atual | sim | [ADR-0005](0005-persistencia-de-sessao.md), PRD-003 |
| DECSCUSR — forma do cursor | sim | PRD-005 RF-5.25 |
| Modos de mouse 1000 / 1002 / 1003, encoding SGR 1006 | sim | [ADR-0013](0013-mouse-selecao-e-clipboard.md) |
| OSC 52 — clipboard | **só escrita** | [ADR-0013](0013-mouse-selecao-e-clipboard.md) |
| OSC 4 / 10 / 11 — paleta, frente, fundo | sim, consulta e set com escopo de sessão | PRD-005, precedente do RF-5.25 |
| 256 cores e true color | sim | PRD-005 RF-5.17 |
| Modo de cursor de aplicação (DECCKM), teclado numérico | sim | ADR-0008 |
| Tela alternativa (1049) | sim | pré-requisito de `vim` e `htop`, critério de saída da F1 |
| OSC 8 — hyperlinks | **sim, com esquemas fechados** — F6 | [ADR-0042](0042-hyperlinks-osc-8.md), [PRD-011](../prd/prd-011-polimento.md) RF-11.10 a RF-11.13 |
| Sixel, kitty graphics | não | ADR-0002 |

### OSC 4 / 10 / 11: consulta e set

Programas consultam frente e fundo para decidir se o tema é claro ou escuro — o `nvim` faz isso, e sem resposta ele escolhe errado. Decisão em duas partes:

- **Consulta** é respondida a partir da config resolvida (tema aplicado, overrides aplicados).
- **Set** vale só para a sessão daquela aba e é revertido no `RIS`. A config permanece a fonte de verdade; o programa em execução tem precedência **enquanto roda**.

Isso não é regra nova: é exatamente o que o RF-5.25 já estabelece para o DECSCUSR, aplicado à paleta.

### Windows

`TERM` continua sendo definido no Windows, ainda que o `cmd.exe` e o PowerShell nativo o ignorem: processos hospedados no ConPTY — WSL, git-bash, MSYS2, qualquer coisa vinda de um ambiente Unix — consomem a variável normalmente. Definir só no Unix criaria um bug que aparece exclusivamente dentro do WSL.

A regra de forçar UTF-8 no spawn do ConPTY permanece como está no [ADR-0004](0004-pty-cross-platform.md).

## Alternativas consideradas

### Terminfo próprio, entrada `porecatu`

Descreve com precisão o que o terminal faz, permite anunciar capacidade nossa que o xterm não tem, e é o caminho "correto" na tradição Unix.

Descartada pelo custo sob SSH, detalhado acima: a entrada precisa existir na máquina remota, e não existe. A dor é permanente, recai sobre o usuário e não tem contorno razoável. Revisitável no v2, quando houver capacidade nossa que o terminfo do xterm não descreva — com ADR próprio e, provavelmente, mantendo `xterm-256color` como default e o terminfo próprio como opção.

### `xterm-direct` (true color pelo terminfo)

Descreve true color de forma padronizada, sem depender da convenção `COLORTERM`.

Descartada por disponibilidade: é uma entrada relativamente recente, ausente em bases de terminfo mais antigas — inclusive em servidores, que é onde `TERM` mais importa. `xterm-256color` mais `COLORTERM=truecolor` é o par que funciona em toda parte, mesmo sendo menos elegante.

### `TERM=xterm` (sem 256 cores)

Máxima compatibilidade histórica.

Descartada de imediato: o PRD-005 tem paleta configurável de 16 cores e true color como requisito. Anunciar 8 cores contradiz o produto.

### Deixar `TERM` configurável pelo usuário como chave de primeira classe

Daria escape a quem tem necessidade específica.

Descartada como chave dedicada porque `[shell.env]` já resolve — quem precisa, sobrescreve lá, e a ordem de precedência decidida acima garante que funcione. Uma chave própria sugeriria que mexer nisso é rotina, quando é caso de exceção.

## Consequências

### Positivas

- `htop`, `vim`, `tmux` e `fzf` funcionam sob SSH em host que nunca ouviu falar do Porecatu — o critério de saída da F1 deixa de depender de instalar nada em máquina remota.
- Escopo de parser da F1 fica declarado: a tabela diz o que implementar e o que descartar.
- `TERM_PROGRAM` dá ao RF-3.1 um gancho estável para o snippet de integração de shell.
- Resposta a OSC 10/11 faz o `nvim` acertar o tema de primeira, sem configuração do usuário.

### Negativas

- Anunciamos ser xterm sem ser xterm. Onde a divergência aparecer, ela aparece como comportamento estranho de um programa específico, e o ônus de investigar é nosso.
- `COLORTERM` é convenção, não padrão: programa que só olhe terminfo verá 256 cores, não true color.
- Set de paleta por OSC 4 cria um estado de aba que a config não descreve — mais um lugar onde o que está na tela divergiu do arquivo.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Programa usar capacidade do terminfo do xterm que não implementamos | Média | Médio | Tabela de capacidades declarada; sequência desconhecida é descartada, nunca desenhada; `vim`/`htop`/`fzf` no critério de saída da F1 |
| Divergência entre o que `TERM` promete e o motor entrega | Média | Baixo | Motor é o do Alacritty, que também anuncia base xterm; ADR-0002 já aceita herdar as decisões dele |
| Set de paleta por programa confundir o usuário | Baixa | Baixo | Escopo de sessão, revertido no `RIS`; mesmo modelo já aceito no RF-5.25 |
| `TERM_PROGRAM` não ser testado por script de shell existente | Baixa | Baixo | Convenção usada por iTerm2 e VS Code; os snippets do RF-3.1 são escritos por nós de qualquer forma |
| Usuário sobrescrever `TERM` em `[shell.env]` e quebrar o próprio terminal | Baixa | Baixo | Precedência documentada no arquivo de exemplo; é escape deliberado |
