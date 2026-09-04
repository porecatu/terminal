# Roadmap

Fases de entrega do v1. Cada fase tem **critério de saída explícito** — enquanto ele não for atendido, a fase não terminou e a próxima não começa.

A ordem não é arbitrária: cada fase entrega algo utilizável e é pré-requisito real da seguinte. Em particular, F1 entrega um emulador de terminal funcional; tudo depois disso é o diferencial do produto ([PRD-000](prd/prd-000-visao-de-produto.md)).

O alvo visual está em [`docs/design/`](design/README.md). O mockup mostra o produto **completo**, não o v1 — a [tabela de fases](design/especificacao-visual.md) diz o que pertence a cada etapa. Nenhuma fase abaixo implementa elemento `[v2]`.

| Fase | Status |
|---|---|
| F0 — Esqueleto | **fechada** |
| F1 — Terminal único | **fechada** |
| F2 — Abas | **fechada** |
| F3 — Grupos | **fechada** |
| F4 — Configuração | **fechada** (dívida na etapa 6, ver seção dela) |
| F5 — Sessão | **fechada** (dívida de verificação, ver seção dela) |
| F6 — Polimento | **aberta** (etapa 1 de 6 fechada; PRD-011 e ADR-0041 a 0044 escritos) |

> **A verificação interativa é dívida assumida, não critério pendente.** Os
> critérios de saída da F1, da F2 e da F3 exigiam gesto de verdade — `vim`,
> `htop` e `fzf` usáveis, mouse dentro deles, `Shift`+arraste, ABNT2, arraste de
> aba e de grupo, seleção múltipla, editor de grupo, duas janelas em monitores de
> DPI diferente. Nada disso foi exercitado com input sintético, e não vai ser: a
> proteção de foco do Windows bloqueia `SetForegroundWindow`/`AppActivate` de
> processo em segundo plano, e não há ambiente Linux/macOS no fluxo. As três
> fases foram **fechadas com cobertura automatizada mais smoke test do
> `cargo run`**, por decisão registrada. O que cada uma não confirmou continua
> escrito na seção dela, agora como dívida — não como tarefa que impede a
> próxima fase. Risco aceito: regressão de gesto não aparece em CI.

---

## F0 — Esqueleto — implementada

Colocar a infraestrutura de pé antes de escrever qualquer lógica.

- Workspace Cargo com os oito crates de [arquitetura.md](arquitetura.md)
- Grafo de dependências entre crates conforme a tabela de [CLAUDE.md](../CLAUDE.md), com os `Cargo.toml` já refletindo as restrições
- Janela `winit` + surface `wgpu` limpando a tela, respondendo a resize e a mudança de DPI
- Versões de `wgpu` e `alacritty_terminal` pinadas com igualdade exata ([ADR-0001](adr/0001-stack-de-gui.md), [ADR-0002](adr/0002-motor-vte.md))
- `rust-toolchain.toml` com a stable do dia, MSRV igual, edition 2024 e lints em `[workspace.lints]` ([ADR-0011](adr/0011-toolchain-rust.md))
- Cabeçalho `// SPDX-License-Identifier: GPL-3.0-or-later` em todo arquivo ([ADR-0010](adr/0010-licenciamento.md))

**O CI acordou junto com o `Cargo.toml`.** O job `detect` encontra o workspace e a matriz das três plataformas roda `fmt`, `clippy -D warnings`, `build` e `test` de verdade, como o contrato de [CLAUDE.md](../CLAUDE.md) prevê. Não foi preciso editar o workflow para isso.

As três pendências desta fase foram fechadas:

- `verificar_fase_documentacao()` saiu de `scripts/verify-docs.py` no commit que criou o `Cargo.toml`
- As dependências de sistema do Linux listadas no workflow bastam para `winit` e `wgpu` — a matriz passa nas três plataformas
- `dtolnay/rust-toolchain` recebe `toolchain: "1.98.0"`, o mesmo valor do `channel` em `rust-toolchain.toml` (sem isso o rustup baixaria uma segunda toolchain por job), e o job canário semanal está ativo ([ADR-0011](adr/0011-toolchain-rust.md))

Uma ficou em aberto, sem impedir a fase: os comandos do CI ainda não usam `--locked`. O `Cargo.lock` é versionado, mas nada impede o CI de resolver versões novas.

> **A ordem de escolha das versões não é livre.** Nenhum ADR fixa números — eles são escolhidos aqui, olhando o crates.io do dia — mas quatro crates da stack são acoplados:
>
> ```
> glyphon → wgpu → raw-window-handle → winit
> ```
>
> Escolher os quatro de forma independente produz erro de tipo em `raw-window-handle` que não se parece com o que é, e é o jeito mais fácil de perder um dia na F0. A ordem que funciona: **fixar o `glyphon` primeiro**, aceitar o `wgpu` que ele exige, e só então escolher o `winit` cuja `raw-window-handle` casa com a do `wgpu`. `alacritty_terminal` e `portable-pty` são independentes dessa cadeia.
>
> O registro das escolhas é o `Cargo.lock`, que é versionado de propósito ([.gitignore](../.gitignore)). Considerar `--locked` nos comandos do CI para que a build seja reproduzível de fato.
>
> **Como ficou:** `glyphon = "0.12.0"` primeiro, `wgpu = "=30.0.1"` (o que ele exige) e `winit = "0.30.13"`, cuja `raw-window-handle` casa com a do `wgpu`. `alacritty_terminal = "=0.26.0"` e `portable-pty = "0.9.0"`, independentes dessa cadeia. Toolchain 1.98.0, edition 2024.

**Critério de saída:** atendido. CI verde nas três plataformas com a matriz Rust efetivamente rodando; janela abre, redimensiona e fecha.

---

## F1 — Terminal único — fechada

O maior salto de risco técnico. Ao fim desta fase o projeto é um emulador de terminal.

Entregue em seis etapas, uma por PR: PTY, motor VT e snapshot, threading e loop de render, fontes e render de texto, teclado/scroll/resize, mouse/seleção/clipboard.

Todos os itens abaixo estão implementados:

- `porecatu-pty`: spawn, read, write, resize, encerramento ([ADR-0004](adr/0004-pty-cross-platform.md))
- `porecatu-term`: `alacritty_terminal` encapsulado, snapshot de grid
- Thread de leitura por terminal, `Wakeup` via `EventLoopProxy`, render damage-driven ([ADR-0007](adr/0007-modelo-de-threading.md))
- `porecatu-render`: pipeline de quads e de texto com `glyphon`, atlas em cache
- Fontes do design embutidas em `assets/fonts/`, com o texto da OFL e a atribuição (PRD-010 RF-10.25, [ADR-0016](adr/0016-fontes-embutidas.md)) — sem subsetting
- Snapshot de grade conforme a [seção 4 da arquitetura](arquitetura.md): tipos próprios, buffer reusado, cor não resolvida, `wide_spacer` para largura dupla
- Roteamento de input, codificação de teclas, bracketed paste ([ADR-0008](adr/0008-teclas-e-roteamento-de-input.md))
- Ambiente do shell: `TERM=xterm-256color`, `COLORTERM`, `TERM_PROGRAM` ([ADR-0012](adr/0012-identificacao-do-terminal.md))
- Reporte de mouse ao programa (modos 1000/1002/1003, encoding SGR 1006), com `Shift` forçando seleção local ([PRD-010](prd/prd-010-interacao-e-superficie-de-app.md) RF-10.1 a RF-10.3, [ADR-0013](adr/0013-mouse-selecao-e-clipboard.md))
- Seleção com mouse pelos quatro modos do motor, cópia com recorte de espaço e remontagem de `WRAPLINE` (PRD-010 RF-10.4 a RF-10.9)
- Clipboard via `arboard`; OSC 52 com escrita permitida e leitura negada (PRD-010 RF-10.10 e RF-10.11)
- Rolagem do scrollback por teclado e por roda, com tela alternativa tratada (PRD-010 RF-10.12 a RF-10.14)
- Forçar UTF-8 no spawn do ConPTY
- `Wakeup` carregando `(WindowId, TabId)` desde já — o tipo atravessa a fronteira de três crates, e corrigir depois é mexer no caminho quente ([ADR-0015](adr/0015-multiplas-janelas.md))

**Critério de saída:** `vim`, `htop` e `fzf` usáveis sem artefatos nas três plataformas; **mouse funciona dentro do `htop` e do `fzf`, e `Shift`+arraste seleciona texto mesmo com o programa pedindo o mouse**; roda do mouse rola `less` e `man`; copiar e colar funciona nas três plataformas, incluindo Wayland; resize funciona com TUI aberta; acentuação por tecla morta em teclado ABNT2 funciona; CPU em ~0% com o terminal ocioso; verificação de que a última linha de saída não se perde ao encerrar o shell.

### Dívida da F1

O código está escrito e o CI está verde nas três plataformas — 51 testes automatizados, `clippy -D warnings` limpo. O que **não** foi confirmado é o critério de saída na parte interativa, e a fase fechou assim (ver a nota no topo):

- Toda a verificação manual aconteceu no **Windows**. Linux e macOS têm build e teste verdes no CI, nada além disso.
- Teclado real, arraste de mouse, seleção, copiar/colar e mouse dentro do `htop`/`fzf` **não foram exercitados**: a proteção de foco do Windows bloqueia `SetForegroundWindow`/`AppActivate` de processo em segundo plano, e input sintético não é caminho viável. Precisa de uma sessão desktop de verdade.
- `vim`, `htop`, `fzf`, `less` e `man` não foram abertos de ponta a ponta; acentuação ABNT2 e resize com TUI aberta idem.
- **Clipboard no Wayland** segue sem verificação (sem ambiente Linux na fase). O plano B `copypasta` do [ADR-0013](adr/0013-mouse-selecao-e-clipboard.md) continua de pé.
- Confirmado no Windows: terminal spawna, CPU ~0% ocioso, título por OSC chega à janela, fechar a janela mata o processo filho sem órfão, cores ANSI do prompt batem com a paleta. *(Nota pós-F4: "sem órfão" era verdade só para o processo filho direto — o shell. Um neto que o shell tivesse spawnado (ex. um servidor de longa duração) sobrevivia; corrigido pelo Job Object do [ADR-0033](adr/0033-job-object-encerramento-de-processo.md).)*

Duas simplificações conscientes, documentadas no código:

- `PushClip`/`PopClip` existem na API de `porecatu-render` mas ainda não recortam nada — sem consumidor até o overflow da barra de abas, na F2. *(A F2 descobriu que não bastava implementar: clip por índice não sobrevive ao achatamento das primitivas em baldes. Resolvido junto com as camadas no [ADR-0018](adr/0018-composicao-de-frame.md).)*
- `line_height` é aplicado sobre `size` em vez das métricas naturais da face (ascent+descent+lineGap), que exigiria ler `hhea`/`OS/2`. Ajustar quando importar na prática.

---

## F2 — Abas — fechada

Implementa [PRD-001](prd/prd-001-abas.md).

Antes de a fase abrir, três ADRs fecharam as decisões que faltavam — não é preâmbulo opcional, é o que torna os itens abaixo executáveis:

- [**ADR-0017**](adr/0017-ciclo-de-vida-da-aba.md) — ciclo de vida e identidade da aba: OSC 7 antecipado para esta fase, precedência de título sem o nível de processo, condição da confirmação do RF-1.6, encerramento sem EOF e sem bloquear a main thread, posição da nota do RF-1.3, estado `Exited`.
- [**ADR-0018**](adr/0018-composicao-de-frame.md) — composição de frame: camadas (a API da F1 desenha todo o texto sobre todos os quads, então nenhum popover ficaria por cima), recorte de verdade para o overflow, medidor de texto sem GPU, `GpuContext` do processo com surface por janela, atlas único com a escala na chave.
- [**ADR-0019**](adr/0019-tooltip.md) — tooltip, o quarto widget de chrome, que o RF-1.10 exige e que o ADR-0014 não previu.

Itens:

- `porecatu-core`: `Workspace`, `Tab`, `Group` com o grupo implícito, IDs estáveis, operações puras com testes de invariante ([ADR-0006](adr/0006-modelo-de-abas-e-grupos.md)). Deriva `serde` já aqui, para que o round-trip `Workspace -> JSON -> Workspace` que o ADR lista como invariante seja testável na fase que o pede
- `porecatu-render`: camadas, recorte, `TextMeasurer` sem GPU, separação `GpuContext`/`WindowSurface` (ADR-0018)
- Barra de abas: layout como função pura `(Workspace, Config, largura) -> Vec<TabRect>`, hit-testing, hover, foco
- Ciclo de vida, título com precedência (customizado → OSC 0/2 → shell), renomeação inline, captura de OSC 7 e herança de `cwd` (ADR-0017)
- Navegação sequencial e por índice; reordenação por arraste e por teclado
- Overflow: encolhimento do rótulo até o piso, depois rolagem da trilha, com indicador de abas fora da vista
- Indicadores de atividade e campainha
- Menu de contexto da aba, tooltip, diálogo de confirmação e superfície de aviso do app (PRD-010 RF-10.15 a RF-10.21, [ADR-0014](adr/0014-superficie-de-aviso-e-dialogo.md), ADR-0019)
- Enum de ação e modo de captura no roteamento de input — passo 1 da cadeia do [ADR-0008](adr/0008-teclas-e-roteamento-de-input.md), que a F1 não tinha; defaults fixos no código, sem parser até a F4
- Janelas múltiplas em escopo mínimo: `window.new`, `window.close`, um `Workspace` e uma surface por janela (PRD-010 RF-10.22 a RF-10.24, [ADR-0015](adr/0015-multiplas-janelas.md))

Um detalhe que nenhum documento cobria e que a fase decide: **a geometria da janela nova.** Ela nasce com o tamanho da janela que a criou, deslocada 30 px para a direita e para baixo, presa à área útil do monitor dessa janela — cascata, não sobreposição exata, que esconderia a janela nova atrás da antiga. Sem janela de origem (primeiro start sem sessão), vale o default da plataforma. A partir da F5 a sessão restaura a geometria gravada e isso só se aplica a janela criada pelo usuário.

Divisão sugerida, no padrão das seis etapas da F1, uma por PR: (1) `core` e as operações puras; (2) camadas, recorte e medidor em `render`; (3) barra de abas com layout puro e hit-testing; (4) ciclo de vida, OSC 7, título e rename; (5) overflow, arraste e indicadores; (6) os quatro widgets de chrome e a segunda janela.

**Aparência:** os valores que o canvas não desenhava estão agora nas seções 2.17 a 2.20 da [especificação visual](design/especificacao-visual.md) — indicadores, overflow, arraste e tooltip —, mais os detalhes completados nas seções 2.2, 2.5, 2.14, 2.15 e 2.16. Nenhuma cor nova. Enquanto `porecatu-config` não existe, os valores entram como constantes com a chave TOML citada no comentário, como a F1 fez em `palette.rs`; sem essa disciplina a revisão dirigida da F4 não tem por onde começar.

**Critério de saída:** todos os cenários de aceite de PRD-001 passam, **exceto "arrastar para dentro de um grupo" (RF-1.16), que passa para a F3** — na F2 só existe o grupo implícito, que não tem fronteira visual para dentro da qual arrastar, e `tab.move_to_group` já é catalogada como F3; **barra de abas confere com [`mockup-estatico.html`](design/mockup-estatico.html)** nos elementos `[v1]` — dimensões, raios, cores por estado, rename inline; layout da barra testado sem abrir janela **e sem GPU**; 50 abas abertas sem degradação perceptível, e fechar uma janela com 50 abas não bloqueia a main thread; menu, tooltip, aviso e diálogo desenham **por cima** do texto do terminal; IME e teclas mortas continuam funcionando com a barra presente; duas janelas abertas em monitores de DPI diferente desenham com métricas corretas e texto nítido, e saída numa delas não redesenha a outra.

`app.quit` e o fechamento da última janela **não gravam sessão** nesta fase; o ponto de chamada existe como no-op documentado e a F5 o preenche (ADR-0017).

### Dívida da F2

O código está escrito e o CI está verde nas três plataformas, com testes automatizados cobrindo `porecatu-core` (invariantes de `Workspace`/`Group`), `porecatu-term` (OSC 7, ciclo de vida) e `porecatu-ui` (layout, overflow, indicadores, arraste, os quatro widgets de chrome — tudo o que é função pura, sem GPU nem janela). O que **não** foi confirmado, mesma limitação da F1:

- Toda a verificação aconteceu por build/teste automatizado e por um smoke test não-interativo do `cargo run` (a janela sobe e roda alguns segundos sem `panic`) — a proteção de foco do Windows continua bloqueando input sintético, então nenhum gesto de mouse ou teclado de verdade foi exercitado nesta fase.
- **Arraste de reordenação (RF-1.15), menu de contexto (clique direito), overflow por roda do mouse e o tooltip com atraso de 600ms** — a lógica está coberta por teste unitário isolado (`tab_bar`, `context_menu`, `tooltip`), mas o gesto ponta-a-ponta nunca rodou numa sessão desktop real.
- **Segunda janela** (`Ctrl+Shift+N`/`Ctrl+Shift+Q`, ADR-0015): a cascata de geometria e a superfície `wgpu` compartilhada nunca foram exercitadas com duas janelas de verdade, muito menos em dois monitores com DPI diferente.
- **Diálogo de confirmação de RF-1.6**: a condição (tela alternativa ou reporte de mouse ligado) depende de abrir `vim`/`htop`/`fzf` de verdade dentro do Porecatu, que segue bloqueado pela mesma proteção de foco.

Quatro simplificações conscientes, documentadas no código (`chrome.rs`, `overlay.rs`, `lib.rs`) e registradas na seção 4.4 da [especificação visual](design/especificacao-visual.md). **Nenhuma delas é pendência**: as quatro foram decididas no [ADR-0028](adr/0028-o-binario-como-referencia-visual.md) §4 — duas aprovadas para a F4, duas fechadas como decisão de não fazer — e o [ADR-0032](adr/0032-interface-do-v1-fechada.md) §2 fechou a lista das que ainda mudam de pixel no v1 exatamente nessas duas.

- Nenhuma sombra nos quatro widgets de chrome — `porecatu-render` não tem primitiva de sombra. *Depois da fase apareceu uma sombra **em camadas** (três `RoundedQuad` empilhados, `chrome::push_shadow`), aplicada à cápsula de grupo, à aba solta e ao quadro do terminal; os cinco widgets e o fantasma de arraste seguem sem ela, e é para eles que a F4 leva a mesma técnica.* O corpo de aviso/diálogo não quebra linha (`TextRun` é sempre uma linha), truncando com reticências em vez do "três linhas" da espec. — **decidido não fazer**, o truncamento em uma linha é o comportamento aprovado.
- A cascata da janela nova prende aos limites físicos do monitor, não à área útil (descontada a barra de tarefas/dock) que o parágrafo acima descreve — `winit::monitor::MonitorHandle` não expõe área útil em nenhuma plataforma.
- **A barra não tem hover visual.** O item "hover" da lista acima ficou entregue pela metade: o hover **existe** como hit-test, e é o que dispara o tooltip do RF-1.10, mas nenhum elemento muda de aparência sob o cursor — exceto os botões de janela do [ADR-0027](adr/0027-controles-de-janela-e-resize-proprios.md), que trocam fundo e ícone. A espec. pede `filter: brightness(1.18)` na aba e `1.25` na pílula, e `porecatu-render` não tem primitiva de filtro; é resolvível em CPU dentro de `porecatu-ui`, multiplicando canais e clampando. **Aprovado para a F4**, junto com o realce e a sombra do fantasma de arraste (espec. §2.19).
- O auto-scroll durante o arraste acontece por evento de `CursorMoved` dentro da zona de 30 px, não pelo intervalo de `.15s` da espec. **Decidido não fazer:** o gesto atual funciona, e mudá-lo mudaria a sensação do arraste sem ninguém ter pedido.

---

## F3 — Grupos — fechada

Implementa [PRD-002](prd/prd-002-grupos-de-abas.md). É o diferencial do produto.

Entregue em seis etapas, uma por PR, na divisão sugerida abaixo: modelo em `core`;
seleção múltipla e gestos da barra; pílula, wrapper tingido e sublinhado por cor;
colapso ponta a ponta; editor de grupo, menu de contexto e popover de destino;
arraste entre grupos, arraste de grupo e a animação de reflui. Depois delas vieram
quatro PRs de correção — bug de ordem em `group_tabs` mais o wiring de
`group.create`, que atravessou a fase sem gesto de UI, e três causas distintas do
mesmo sintoma ("o colapso só anima no primeiro grupo"), a última delas a lentidão
da barra em overflow.

**A fase fechou** com um PR final de três itens: o RF-2.21
(`group.next`/`group.prev`), a regra do RF-2.17 e os atalhos que faltavam no
nível de grupo — ver [o PR de fechamento](#o-pr-de-fechamento-da-f3).

Antes de a fase abrir, quatro ADRs fecharam as decisões que faltavam — mesmo movimento que os ADR-0017 a 0019 fizeram entre a F1 e a F2, e pela mesma razão: foi a fase anterior que expôs as lacunas. **Não é preâmbulo opcional**; dois deles mudam código que a F2 acabou de estabilizar.

- [**ADR-0020**](adr/0020-grupos-explicitos.md) — modelo de grupos explícitos. Resolve cinco pontos em que o [ADR-0006](adr/0006-modelo-de-abas-e-grupos.md) era ambíguo ou insuficiente, sendo o primeiro uma contradição interna dele: o grupo implícito era declarado **único**, mas o cenário de aceite *"grupo na posição central da barra"* exige abas soltas dos dois lados, o que um nó implícito só não representa sem quebrar a contiguidade. Passa a haver **N runs implícitos**, cada um com `GroupId` de sessão. Também: colapso deixa de ser "só desenho" e ganha uma **ordem navegável** separada da visual (o que renumera `Alt+1..9`, deliberadamente); o terceiro nível do RF-1.5 ganha direção e regra para grupo colapsado; RF-2.7 e RF-2.8 param de se contradizer; e a paleta de seis cores ganha regra para os dez grupos que a métrica exige.
- [**ADR-0021**](adr/0021-selecao-multipla-e-gestos-da-barra.md) — seleção múltipla e gestos da barra. A seleção é estado **efêmero de janela**, não persistido, com âncora explícita para o `Shift`+clique. Fecha o conflito de modificador que o RF-2.1 não viu: no macOS `Ctrl`+clique é o clique secundário e abre o menu, então o modificador de seleção lá é `Cmd`. Decide também a quem pertencem os 6 px entre wrappers durante o arraste, e o que é soltar sobre uma pílula ou sobre um grupo colapsado.
- [**ADR-0022**](adr/0022-animacao-de-interface.md) — animação sob render damage-driven. O RF-2.5 exige movimento animado como cenário de aceite, e o [ADR-0007](adr/0007-modelo-de-threading.md) decidiu que terminal ocioso não gera frame — a F2 recusou animação três vezes por causa disso. Passa a existir um relógio por janela, dirigido pelo `ControlFlow::WaitUntil` que a F2 já usa para tooltip e aviso, ativo só enquanto há movimento pendente. Lista de consumidores **fechada** em dois, e desligável na config.
- [**ADR-0023**](adr/0023-editor-de-grupo.md) — o editor de grupo, quinto widget de chrome. O ADR-0014 o mencionava só para descartá-lo como substituto de menu, sem decidir o que ele é. Ganha camada, captura de teclado própria, e vira a **única** superfície de escolha de cor e de destino do v1 — sem submenu, que não existe em documento nenhum.

Itens:

- `porecatu-core`: `Group` com discriminante implícito/explícito, nome, cor e colapso; manutenção dos runs implícitos (divisão ao agrupar, fusão ao dissolver); `group_tabs`, `ungroup`, `rename_group`, `set_group_color`, `collapse_group`; `navigable_order()` ao lado de `visual_order()`; MRU por grupo; a escada de foco do RF-1.5 numa função só, usada por `close_tab` e por `collapse_group`. `new_tab` deixa de escrever em `groups[0]`
- Seleção múltipla em `WindowState`, com invalidação e âncora, e os gestos da barra por plataforma (ADR-0021)
- Criar, dissolver, renomear, recolorir; atribuição automática de cor com a regra de repetição do ADR-0020
- Colapso ponta a ponta: navegação, foco, acesso por índice, overflow e indicador agregado na pílula
- Pílula, wrapper tingido na cor do grupo e sublinhado da aba por grupo — o layout já é multi-grupo desde a F2 (`GroupWrapperRect`), o que falta é a pílula na geometria e a cedência dela no overflow
- Arraste de aba entre grupos e arraste do grupo inteiro, com o realce de fronteira — inclui o cenário de aceite *"arrastar para dentro de um grupo"* (PRD-001 RF-1.16), que veio da F2 porque exige grupo explícito
- Menu de contexto do grupo e editor de grupo, lendo a **mesma** lista de ações (RF-10.21), incluindo fechar todas com confirmação e contagem
- Popover de grupo de destino para `tab.move_to_group`, que sai de esmaecido no menu de aba
- Animação da reordenação ao formar grupo, pelo relógio do ADR-0022

**Aparência:** o que a F3 precisava e não tinha desenho está escrito. Saíram da seção 4.2 da [especificação visual](design/especificacao-visual.md) os quatro itens que eram desta fase — realce de fronteira (§2.19), aba selecionada (§2.5, como modificador de borda), animação de reordenação (token `reflow`, §1.10) e indicador agregado (§2.4) — e entraram três que não constavam de nenhuma lista: arraste do rótulo do grupo (§2.19.1), campo de nome inline na pílula (§2.10.1) e truncamento do nome do grupo (§2.4). A lista de pendências de desenho fica com **três** itens, todos de F4/F5 — nenhum bloqueia esta fase. Nenhuma cor nova.

**Divisão sugerida** — foi a divisão executada —, no padrão das seis etapas da F1 e da F2, uma por PR: (1) modelo em `core` — grupos explícitos, runs implícitos, colapso, duas ordens, MRU, escada de foco; (2) seleção múltipla e gestos da barra; (3) pílula, wrapper tingido e sublinhado por cor de grupo, no layout e na pintura; (4) colapso ponta a ponta, incluindo overflow, navegação e indicador agregado; (5) editor de grupo, menu de contexto de grupo e popover de destino; (6) arraste entre grupos, arraste de grupo e a animação do RF-2.5.

**Critério de saída:** todos os cenários de aceite de PRD-002 passam; **pílula, tingimento do wrapper, sublinhado da aba e editor de grupo conferem com o mockup**; 10 grupos numa janela sem quebra de layout, com a ordem de cedência do §2.18 respeitada; processos seguem vivos em grupo colapsado; as invariantes de run implícito — nenhum vazio, nenhum par adjacente — verificadas em teste depois de sequências de operações, não só de operações isoladas; `navigable_order()` sempre subsequência de `visual_order()`; o event loop volta a dormir depois de toda animação.

> **Como o critério ficou.** Atendido em teste automatizado: invariantes de run
> implícito depois de sequências de operações, `navigable_order()` como
> subsequência de `visual_order()`, e o relógio de animação que sai do
> `next_deadline()` quando a última reflui termina. **Não atendido:** os cenários
> de aceite não foram exercitados com gesto real (verificação interativa), o
> RF-2.21 não existe, e duas cláusulas mudaram de alvo junto com o desenho — a
> "ordem de cedência do §2.18" foi descartada (nada cede, a trilha rola) e o
> "tingimento do wrapper" virou cápsula de cor cheia. As duas estão registradas
> na seção 4.4 da especificação visual, e é a F4 que cobra o casamento com o
> mockup.

O RF-2.17 (*"ativar uma aba de grupo colapsado expande o grupo"*) fica **parcialmente verificável** nesta fase: as duas fontes que o requisito cita são busca (F6) e restauração de sessão (F5), e na F3 não há caminho que ative uma aba oculta.

> **Correção, e como ficou.** Esta seção afirmava que "a regra entra no modelo
> e no teste unitário" quando ela **não havia entrado**: `activate_tab` não
> expandia grupo colapsado e não havia teste do cenário. O PR de fechamento
> corrigiu isso — a regra está em `Workspace::activate_tab`, com dois testes,
> um deles de regressão para o laço que ela poderia criar com
> `collapse_group` (a escada de foco do RF-1.5 nunca devolve aba do grupo que
> está colapsando, senão o colapso viraria no-op). Continua sem cenário de
> ponta a ponta: nenhum caminho da F3 ativa aba oculta, e as duas fontes que o
> requisito cita são F5 e F6.
>
> **Fechado na etapa 4 da F5.** Restaurar uma sessão cuja aba ativa está
> dentro de um grupo gravado como colapsado expande o grupo -- o primeiro
> caminho real do app que ativa uma aba oculta. Sem mecanismo novo: a aba
> ativa é sempre criada `Running` e `Workspace::activate_tab` (já testado
> aqui, na F3) roda de novo no fim da reconstrução, depois do
> `collapse_group` que o próprio arquivo pediu -- ativá-la de novo é o que
> expande. Teste de ponta a ponta em
> `porecatu-session::convert::tests::restoring_the_active_tab_inside_a_collapsed_group_expands_it`,
> confirmado ao vivo na verificação manual daquela etapa.

### O PR de fechamento da F3

O código dos itens acima está escrito, o CI está verde nas três plataformas e o
workspace tem **292 testes** (241 ao fim das seis etapas, mais os PRs de
correção e o de fechamento; contra 145 ao fim da F2): `porecatu-ui` 137,
`porecatu-core` 69, `porecatu-term` 51, `porecatu-render` 27, `porecatu-pty` 8.

Um PR fechou a fase, com três coisas que são a mesma conversa — o nível de grupo
do [ADR-0008](adr/0008-teclas-e-roteamento-de-input.md) e a navegação que ele
aciona:

1. **RF-2.21 — `group.next`/`group.prev`.** `Workspace::step_group` anda de grupo
   em grupo na ordem visual, circulando, e ativa **a última aba visitada** do
   destino (`Group::last_active`, gravado desde a etapa 1 e até aqui sem nenhum
   consumidor fora dos testes). Três regras do
   [ADR-0020](adr/0020-grupos-explicitos.md) §6: grupo colapsado é pulado —
   navegar não expande nada —, grupo vazio também, e `last_active` ausente cai na
   **primeira** aba do grupo. Sem grupo de origem navegável, entra pela ponta. Run
   implícito conta como destino: sem isso, "voltar para as abas soltas" não teria
   gesto. Teclas `Ctrl+Shift+PageDown`/`PageUp`, exigindo `Ctrl` **e** `Shift` para
   não colidir com o `Shift+PageUp`/`PageDown` do scrollback.
2. **Os atalhos que faltavam no nível de grupo.** `group.dissolve`
   (`Ctrl+Shift+U`), `group.rename` (`Ctrl+Shift+E`) e `group.toggle_collapse`
   (`Ctrl+Shift+K`) existiam só por menu, editor ou clique na pílula. Entraram como
   defaults fixos no código, sem esperar o parser da F4 — a lição da própria F3 é
   que **ação sem gesto no fim da etapa que a nomeia é dívida, não escopo
   adiado**, e foi um gesto que revelou o bug de ordem do `group_tabs`. As três
   despacham pelo mesmo `run_group_action` do menu, então tecla e menu não podem
   divergir. O alvo por tecla é o grupo da aba ativa, resolvido por
   `group_menu::keyboard_target` — função pura, testada sem GPU, que devolve `None`
   sobre run implícito: o que o menu mostra esmaecido (RF-10.20), a tecla trata
   como no-op.
3. **RF-2.17 — ativar aba de grupo colapsado expande o grupo.** Em
   `Workspace::activate_tab`, com o teste de regressão do laço com
   `collapse_group`; ver a correção acima.

**Como ficou:** +15 testes (12 em `porecatu-core`, 3 em `porecatu-ui`), `clippy
-D warnings` limpo, `cargo run` sobe e roda sem `panic`. Os gestos em si entram
na dívida de verificação interativa das três fases (nota no topo).

### Dívida da F3

- **Verificação interativa**, mesma limitação da F1 e da F2 — a fase fechou assim
  (nota no topo). Nenhum cenário de aceite de PRD-002 foi exercitado com gesto de
  verdade: seleção múltipla por `Ctrl`/`Shift`+clique, arraste de aba entre grupos,
  arraste da pílula, duplo clique na pílula, editor de grupo por teclado, e a
  animação de reflui vista com olho humano. Dez grupos numa janela e a métrica do
  PRD-002 idem.
- **Entrada de cor por hexadecimal** (RF-2.10) segue diferida pelo próprio
  [ADR-0023](adr/0023-editor-de-grupo.md): o editor tem os seis swatches e nada mais.
  **Entra na etapa 4 da F6** como RF-11.29.
- **`scrollback.to_top`/`to_bottom` não respondem.** Correção do que esta linha
  dizia: elas **têm** default embutido (`Shift+Home`/`Shift+End`) desde o parser da
  F4 e entram no mapa resolvido — o que falta não é tecla, é despacho: o `match` de
  `run_action` as devolve como não tratadas. A operação existe em `porecatu-term`
  desde a F1. **Entra na etapa 2 da F6** como RF-11.30.
- `app.quit` **saiu** desta lista: está implementada (`ActionOutcome::CloseWindowRequested`)
  e ligada a `Cmd+Q` no macOS desde a etapa 5 da F4. Não ter default em Windows/Linux
  é deliberado — é convenção de plataforma, como o [ADR-0008](adr/0008-teclas-e-roteamento-de-input.md) previu.

> **Três itens saíram desta lista, entregues na etapa 6 da F4** — estavam escritos
> aqui como pendentes muito depois de terem sido feitos, e a correção veio na
> auditoria que abriu a F6:
>
> - **Defaults de macOS**, que o parser do [ADR-0029](adr/0029-enum-de-acao-e-gramatica-de-tecla.md)
>   entregou na etapa 5 da F4 — *"os defaults de macOS respondem pela primeira vez"*;
> - **`animations = false`** ([ADR-0022](adr/0022-animacao-de-interface.md)), na etapa 6;
> - **roda do mouse no popover de destino**, na etapa 6.

> `show_new_tab_button = false` **saiu** desta lista: a chave desliga hoje tanto o
> "+" de cada grupo quanto o de aba solta, com teste dedicado em `tab_bar.rs`. E
> "sem hover e sem sombra" foi reescrito: a sombra existe em três lugares e o hover
> está aprovado para a F4 — ver a dívida da F2 acima.

Quatro decisões visuais da fase, registradas na seção 4.4 da
[especificação visual](design/especificacao-visual.md), com a prosa das seções de
anatomia atualizada para o comportamento real. Desde o
[ADR-0028](adr/0028-o-binario-como-referencia-visual.md) elas **não são
divergências a cobrar**: são o alvo, e a F4 não as desfaz. Depois da fase vieram
outras nove, no mesmo registro — vidro, sombra, quadro do terminal, indicador de
overflow em círculo, contador fora da pílula, entre elas.

- **A cápsula do grupo é pintada com a cor cheia**, não com o tingimento de `.07` do
  §2.3, e o fundo da aba passou a ter alfa `.85` para deixar passar um indício dela.
  Pedido direto do usuário: a `.07` ficava invisível atrás do fundo opaco das abas.
- **Botão "+" ao final de cada grupo** (`group.new_tab`), que a especificação não
  previa — o §2.6 só tinha o botão global.
- **A barra virou trilha rolável + zona fixa à direita**: o botão de nova aba global
  não rola mais junto com a trilha.
- **Não há mais ordem de cedência no overflow.** `fit_width` deixou de encolher
  rótulo de aba e nome de pílula por busca binária — até 24 relayouts completos por
  frame, cada um remedindo o texto de toda aba com `cosmic-text` sem cache, e a causa
  real da barra parecer travada ao trocar de aba com overflow. Rótulo e nome ficam
  sempre no teto e a trilha rola como componente só; as chaves `min_width` e
  `label_min_width` saíram do arquivo de exemplo, órfãs.

---

## F4 — Configuração

Implementa [PRD-004](prd/prd-004-aparencia-do-chrome.md) e [PRD-005](prd/prd-005-aparencia-do-terminal.md).

Antes de a fase abrir, **três ADRs fecharam as decisões que faltavam** — mesmo movimento dos ADR-0017 a 0019 entre a F1 e a F2 e dos ADR-0020 a 0023 entre a F2 e a F3, e pela mesma razão: sem eles, três respostas seriam inventadas durante a implementação.

- [**ADR-0029**](adr/0029-enum-de-acao-e-gramatica-de-tecla.md) — o enum `Action` nasce em `porecatu-core` (é o único crate que `config` e `ui` ambos veem, e `config` não pode depender de `ui`), com um teste bidirecional contra o [catálogo](reference/acoes.md). Mais a gramática que o [ADR-0008](adr/0008-teclas-e-roteamento-de-input.md) não deu: grafia das teclas, canonicalização dos modificadores — sem ela "binding duplicado é erro" é indetectável —, `cmd` como nome do modificador de macOS, e `[keybindings.<plataforma>]` para um dotfile só expressar defaults por SO.
- [**ADR-0030**](adr/0030-escopo-do-hot-reload.md) — três classes de chave: aplica a quente; aplica a quente com recálculo de grade e resize dos PTYs; exige reinício e **avisa**. A classe fica escrita ao lado da chave no arquivo de exemplo. Ignorar em silêncio o que não aplica produz o relato "mudei e não aconteceu nada", indistinguível de bug.
- [**ADR-0031**](adr/0031-temas-nomeados.md) — tema é conjunto de **cores** (de terminal e de chrome), nunca de fonte ou dimensão, o que mantém `theme.cycle` em uma classe barata; merge por folha com a `palette` substituída inteira; ciclo na ordem do arquivo, com o estado "sem tema" participando; e ciclar **não escreve** no arquivo.

Itens:

- `porecatu-config`: structs `serde`, defaults completos, resolução de caminho, `PORECATU_CONFIG` (`--config` tem a precedência pronta em `resolve_config_path` desde a etapa 1; a dívida de nada no binário chamá-la com um valor foi paga na etapa 5 da F5, [ADR-0040](adr/0040-superficie-de-linha-de-comando.md))
- Hot reload com `notify`, parse fora da main thread, debounce, nas três classes do ADR-0030
- Erro com linha e chave; chave desconhecida como aviso; config inválida preserva a anterior — com a exceção fina do ADR-0029 §4, em que erro numa linha de `[keybindings]` descarta **aquela linha** e mantém o default embutido, em vez de reverter a gravação inteira
- Parser de `[keybindings]` contra o [catálogo fechado de ações](reference/acoes.md): ação desconhecida é erro com sugestão do nome mais próximo, binding duplicado cita as duas linhas (ADR-0008, ADR-0029)
- **Os defaults de macOS**, que não existem em código: `handle_tab_action_key` só traz Windows/Linux, e a tabela do ADR-0008 define `Cmd+…` para toda ação de F2 e F3
- Toda a superfície de [porecatu.example.toml](config/porecatu.example.toml) ligada de fato ao que o binário desenha
- Fallback de fonte, temas nomeados com override (ADR-0031), zoom por atalho
- Recálculo de grade e resize de todos os PTYs ao mudar métricas de fonte (classe B do ADR-0030)
- **As duas mudanças visuais aprovadas** — e a lista é **fechada nessas duas** ([ADR-0032](adr/0032-interface-do-v1-fechada.md)): hover por brilho resolvido em CPU (aba, pílula e fantasma de arraste) e a sombra em camadas nos **cinco widgets de chrome** e no fantasma. A técnica da sombra já existe em `chrome::push_shadow`, aplicada hoje à cápsula, à aba solta e ao quadro do terminal. Fora dessas duas, **nada nesta fase mexe em pixel**
- `animations = false` aplicando o reflui instantâneo ([ADR-0022](adr/0022-animacao-de-interface.md)), com as **duas** durações do arquivo de exemplo governando as duas constantes de hoje
- Roda do mouse no popover de destino, e as chaves que a F3 deixou como constante

**O que a F4 não faz:** mexer na interface. O critério de saída inverteu com o ADR-0028 — a configuração padrão reproduz o binário, não o contrário —, e o [ADR-0032](adr/0032-interface-do-v1-fechada.md) fechou a lista do que ainda muda de pixel no v1: as duas do item acima, e nada mais. A **trilha de grupos e abas só é tocada quando um recurso novo exigir**, não por acabamento. Quatro itens que já foram dívida estão fechados como **decisão de não fazer**: corpo de aviso em três linhas, auto-scroll do arraste por intervalo, e os estilos `underline`/`left-bar`/`outline` do indicador de grupo, que saíram de escopo junto com a chave `indicator_style`.

**Divisão sugerida**, no padrão das seis etapas da F1, da F2 e da F3, uma por PR:

1. **`porecatu-config` nasce — fechada.** Structs `serde` com defaults completos,
   resolução de caminho com a precedência `--config` → `PORECATU_CONFIG` → caminho
   de plataforma via `dirs` (`resolve_config_path`, testada com as três fontes),
   erro com linha e chave, chave desconhecida como aviso
   ([ADR-0003](adr/0003-formato-de-configuracao.md)). **Sem consumidor ainda:** a
   etapa entrega `Config` carregado e testado, não aparência mudada — e é isso que a
   torna testável sem GPU e sem janela. **Dívida, paga na etapa 5 da F5**
   ([ADR-0040](adr/0040-superficie-de-linha-de-comando.md)): até lá, nada em
   `main.rs`/`App::new` lia `argv` e chamava `load`/`resolve_config_path` com um
   valor -- a função aceitava `--config`, mas a flag não existia de fato até algo
   passar `Some(path)` pra ela; `App::new` chamava `load(None)` fixo, e só
   `PORECATU_CONFIG`/o caminho padrão respondiam.
2. **`porecatu-ui` lê `Config` — fechada.** As ~30 constantes que já citavam a chave
   TOML de origem, mais a geometria da barra (`TabBarStyle`) e dos cinco widgets,
   saíram de `const` e passaram a vir de um `Arc<Config>`. Critério de saída
   verificado: com a config padrão, o binário continua desenhando exatamente o que
   desenhava antes.
3. **Terminal — fechada.** Fonte (família, tamanho, `line_height`, fallback), cores,
   cursor, scrollback, seleção e clipboard saíram do `TermParams` fixo que `ui`
   montava. Inclui o recálculo de grade e o resize de todos os PTYs quando a métrica
   de fonte muda — a classe B do [ADR-0030](adr/0030-escopo-do-hot-reload.md),
   acionado na carga inicial.
4. **Hot reload — fechada.** `notify` assistindo o diretório do arquivo (não só ele,
   por causa de write-then-rename), parse fora da main thread, debounce de ~200ms, e
   as três classes do ADR-0030 aplicadas comparando config antiga e nova: A troca o
   `Arc` e redesenha, B soma recálculo de grade e resize de todos os PTYs da janela,
   C avisa o escopo real (severidade informação) e aplica o resto da gravação mesmo
   assim. Erro de config mostra linha/coluna/mensagem e mantém a config anterior
   (ADR-0003 regra 2); chave desconhecida vira aviso (RF-4.22). `config.reload`
   existe e é chamável, sem tecla ainda (etapa 5).
5. **`enum Action` em `porecatu-core` + parser de `[keybindings]` — fechada.**
   ([ADR-0029](adr/0029-enum-de-acao-e-gramatica-de-tecla.md)), com o teste
   bidirecional contra o [catálogo](reference/acoes.md) e os **defaults de macOS**,
   que antes existiam só como dado embutido em `porecatu-config` e agora respondem
   de fato: `Chord`/a resolução dos três níveis (embutido da plataforma -> comum ->
   plataforma) vivem em `porecatu-ui`, que é quem conhece `winit`.
   `handle_tab_action_key` consulta o mapa resolvido em vez do `match` fixo que
   existia até aqui. `scrollback.*`/`clipboard.*` continuam num caminho hardcoded
   em `input.rs`, de propósito (armadilha registrada no código): mover os dois pro
   mapa não era escopo desta etapa.
6. **Temas nomeados, zoom, visual — fechada, com dívida registrada.**
   [ADR-0031](adr/0031-temas-nomeados.md), `animations = false`, a roda do mouse no
   popover de destino, e as **duas mudanças visuais aprovadas**
   ([ADR-0032](adr/0032-interface-do-v1-fechada.md)): hover por brilho (`chrome::
   brighten`, CPU, `1.18`/`1.25`, calculado por frame a partir de `tab_bar::hit_test`)
   e sombra em camadas (`chrome::push_shadow`) nos **cinco widgets de chrome** e no
   fantasma de arraste, sempre ligada nele. Dois itens ficaram de fora, registrados
   como dívida, não como decisão de não fazer (a dívida do merge de tema cobrir só
   os campos que `Theme` já tinha desde a etapa 1 foi paga depois, fora de fase —
   `apply_theme` mescla hoje toda a superfície do ADR-0031 §1, `[appearance.groups]`
   e os cinco widgets de chrome inclusos, com `palette` substituída inteira pelo
   caso especial do ADR-0031 §2):
   - **Zoom de sessão é sempre do processo inteiro.** `zoom_scope = "active"`
     (RF-5.10) não tem efeito -- zoom por aba pediria métrica de célula por
     `TabRuntime`, não só por processo (`App::cell_metrics`), mudança maior que caberia
     nesta etapa com segurança.
   - **Entrada de cor por hexadecimal no editor de grupo (RF-2.10) não foi
     implementada.** Os tokens `input_*` de `[appearance.group_editor]` já existem
     (reaproveitáveis, o campo de nome já os usa) para quando alguém fizer o campo.

A ordem não é arbitrária: 1 e 2 destravam todo o resto (sem `Config` carregado e sem a
UI lendo dele, nada é verificável de ponta a ponta); 4 depende de 2 e 3; 5 e 6 são
independentes entre si e podem trocar de posição. As etapas 1 a 3 já entregam o que a
fase promete — o arquivo passa a governar a aparência — antes de a fase acabar.

**Critério de saída:** **todo valor de aparência com procedência declarada** — vira chave o que se vê, fica fixo o que é mecânica de interação (limiar de clique e de arraste, cascata de janela, intervalo de frame), e a lista do que fica fixo está no cabeçalho do arquivo de exemplo; verificação por revisão dirigida. Todos os cenários de aceite de PRD-004 e PRD-005 passam, **exceto os três itens de dívida da etapa 6 acima** (zoom por aba, cor hex no editor, tema cobrindo grupos/widgets). **A config padrão reproduz o binário** — se divergir, o errado é o default, não a interface (ADR-0028). Auditoria de rastreabilidade nas duas direções: nenhuma chave do exemplo sem requisito, nenhum requisito sem chave — com a metade das ações automatizada pelo teste bidirecional do ADR-0029.

---

## F5 — Sessão

Implementa [PRD-003](prd/prd-003-persistencia-de-sessao.md), os 17 requisitos RF-3.1 a RF-3.17.

Antes de a fase abrir, **cinco ADRs fecharam as decisões que faltavam** — mesmo movimento dos ADR-0017 a 0019 entre a F1 e a F2, dos ADR-0020 a 0023 entre a F2 e a F3 e dos ADR-0029 a 0031 entre a F3 e a F4. Aqui a razão é um pouco diferente das outras três: não foi a fase anterior que expôs as lacunas, foi o **ADR-0005** — aceito desde antes da F1 — decidir o formato em prosa e parar antes de nome de chave, mecanismo de migração e envelope de janela. Mais duas contradições vivas entre documento e código, que ficaram latentes só porque ninguém escrevia o arquivo ainda.

- [**ADR-0036**](adr/0036-formato-do-arquivo-de-sessao.md) — o schema vira **tipos próprios em `porecatu-session`**, versionados por módulo, com conversão explícita de e para `porecatu-core`. Supersede parcialmente o ADR-0005: aquele prometia "serializa `porecatu-core` e mais nada", e o derive do domínio grava hoje cinco campos que a lista dele não inclui — `Group::last_active` (cujo próprio comentário diz "não persistido"), `activity`, `bell`, `process_title` e `state`, este último contradizendo o ADR-0017 §6, que decidiu que aba `Exited` **não é restaurada**. Fecha também o envelope multi-janela do RF-3.17 (que `Workspace` não pode carregar, por ser mono-janela desde o ADR-0015), a identidade de monitor, a colisão de `.corrupt` e `PORECATU_SESSION` como costura de teste.
- [**ADR-0037**](adr/0037-aba-nao-iniciada.md) — `TabState::NotStarted`, o terceiro estado. A restauração preguiçosa do RF-3.8 pede um estado que não existe, e ele cruza com cinco decisões aceitas: confirmação de fechamento (ADR-0034), indicadores de atividade e campainha, tooltip, escada de foco e navegação. Decide também o **RF-3.9**, que era o último item da lista de pendências de desenho da especificação visual: rótulo com alfa `.45`, sem elemento novo e sem valor novo.
- [**ADR-0038**](adr/0038-fallbacks-de-cwd.md) — o fallback de `cwd` sem OSC 7 é `sysinfo`, **que já está no workspace** desde o ADR-0033, consultado a partir do `root_pid` do `ProcessGroup` e só no momento da gravação. Substitui o par `/proc` à mão + crate `libproc` que o ADR-0005 nomeava: são os mesmos dois mecanismos, sem dependência nova e com um caminho de código em vez de dois. No Windows a rejeição do PEB continua de pé.
- [**ADR-0039**](adr/0039-convite-a-integracao-de-shell.md) — o convite do RF-3.1 é **nota escrita no grid**, pelo `inject_note` que já existe: o snippet fica copiável pela seleção normal do terminal, nenhum widget novo entra e o ADR-0014 continua com cinco. Fecha o critério de detecção, a proeminência maior no Windows e onde mora a dispensa definitiva — em `session.json`, porque o app não escreve na config do usuário.
- [**ADR-0040**](adr/0040-superficie-de-linha-de-comando.md) — o binário passa a ler `argv`, com uma superfície pequena de propósito e parsing à mão. É o RF-3.12 (caminho posicional cria sessão nova, sem restaurar e sem sobrescrever) e é a mesma engrenagem que paga a **dívida da etapa 1 da F4**: `resolve_config_path` aceita `--config` desde lá e nada nunca a chamou com um valor.

Itens:

- `porecatu-session`: schema versionado com tipos por versão, escrita atômica, debounce, save síncrono no exit
- Restauração preguiçosa, geometria de janela, recuperação de monitor ausente
- Fallback de `cwd` por `sysinfo` no Linux e no macOS, sobre a captura de OSC 7 que existe desde a F2
- Detecção de ausência de OSC 7 e convite à integração de shell, com snippets por shell
- Recuperação: arquivo ausente, corrompido, schema antigo, schema mais novo
- `argv` no binário: `--config`, caminho posicional, `--help`, `--version`
- **Bug latente da F2, achado ao escrever os snippets:** `parse_file_uri` (`porecatu-term/src/osc7.rs`) devolvia o caminho a partir da primeira `/` depois de `file://`, então `file:///C:/Users/ana` virava `/C:/Users/ana` — que não é caminho válido no Windows. Passou batido porque nenhum shell do fluxo emite OSC 7, e o efeito já existia no `cwd` herdado por `tab.new`/`window.new`, não só na sessão. **Corrigido na etapa 2** (`strip_windows_drive_leading_slash`), com teste do URI de letra de unidade
- **Escopo extra aprovado**: tema e zoom de sessão persistidos (o [ADR-0031](adr/0031-temas-nomeados.md) já dizia que persistir o tema é F5, e a lista do ADR-0005 não o incluía), e o RF-2.17 fechado ponta a ponta

**Divisão sugerida**, no padrão das seis etapas da F1, da F2, da F3 e da F4, uma por PR:

1. **`porecatu-session` nasce — fechada.** Dependências (`serde`, `serde_json`, `dirs`, `porecatu-core`), `path.rs` com `PORECATU_SESSION` → caminho de plataforma via `dirs` (o diretório de **estado**, deliberadamente diferente do da config), `schema/v1.rs` com o DTO do [ADR-0036](adr/0036-formato-do-arquivo-de-sessao.md), conversão nos dois sentidos, escrita `tmp` → `fsync` → `rename`, e a tabela de recuperação inteira. **Sem consumidor ainda**, como a etapa 1 da F4: o crate carrega e grava, e é testável sem GPU e sem janela. Entrega RF-3.5, RF-3.13, RF-3.14, RF-3.15, RF-3.16. O teste que justifica ter escolhido o DTO entra aqui: reprova quando um campo novo de `porecatu-core` não foi classificado como gravado ou explicitamente descartado, por introspecção das chaves do JSON que o próprio derive de `Tab`/`Group`/`Workspace` produz — sem acessar campo privado. **Porta de entrada mínima em `porecatu-core`**: `Tab::shell_name()`, acessor novo, necessário porque `TabV1::spawn_program` precisava do shell spawnado e nada expunha isso antes. **Mecanismo de migração exercitado com uma versão fictícia** (v0 → v1 só de teste), já que ainda não existe uma v2 real. Nenhum desvio do ADR-0036: a ordem de reconstrução (`groups`/`tabs`/`active_tab` → `Workspace` via API pública, sem IDs do arquivo sobrevivendo) tem teste próprio de ordem visual, não só de invariante.
2. **Gravação fiada na UI — fechada.** Debounce por `ControlFlow::WaitUntil` (`SessionScheduler`, `porecatu-ui/src/session_writer.rs`), entrando no `next_deadline()`/`schedule_next_wake` da janela e recebendo `Instant` de fora — a regra que vale desde o `AnimationClock` da F3; por **processo**, não por janela (RF-3.17: um arquivo para todas). A lista de mudanças estruturais do RF-3.2 marca suja a janela por dois pontos únicos em `WindowState` — `touch_workspace` (22 chamadores de método mutante de `Workspace`) e `mark_session_dirty` (5 chamadores fora de `Workspace`: título customizado via `tab_mut`, `cwd`/`Exited` de `TermEvent`, mover/redimensionar a janela) —, drenados uma vez por volta do event loop em `schedule_next_wake`. A gravação síncrona preenche o **no-op documentado** que a F2 plantou (`close_window_unconditionally`, [ADR-0017](adr/0017-ciclo-de-vida-da-aba.md) §7); geometria e monitor são capturados por janela, frescos de `winit` no momento da escrita (nunca cacheados); `enabled = false` (`App::session_persistence_enabled`) é o único ponto que decide se algo é gravado, debounce ou exit. Entrega RF-3.2, RF-3.3, RF-3.4, RF-3.6 e a metade de gravação do RF-3.17. Inclui a correção do `file:///C:/...` no `parse_file_uri` — é aqui que o `cwd` de OSC 7 passa a ir para o disco, e um caminho inválido gravado é pior que não gravado. Verificação manual real (não só automatizada): `cargo run` com `PORECATU_SESSION` apontando pra um arquivo de scratch grava, sozinho, depois do debounce — a janela nasce com uma aba (RF-1.1), que já marca suja; o `session.json` resultante tem geometria, monitor (`\\.\DISPLAY2`), grupo implícito e `cwd` corretos.
3. **O terceiro estado da aba — fechada.** `TabState::NotStarted` em `porecatu-core` (`is_not_started()`, `start()` — sem volta, no-op fora de `NotStarted`, `accepts_input()` passa a exigir `Running`, simétrico com `Exited`). Auditoria dos `match` sobre `TabState`: **zero** encontrados em todo o workspace — os consumidores existentes usam `matches!`/comparação, nenhum braço curinga pré-existente escondia o estado novo. Shell no primeiro foco é um ponto único, `App::ensure_active_tab_started` (extraído de `open_tab`, que ganhou irmã `spawn_tab_runtime` reusável), chamado uma vez no fim de `window_event` — mesmo padrão de `schedule_next_wake` —, cobrindo clique, `Ctrl+Tab`/`Ctrl+PageDown`, índice, passo de grupo e expansão sem wiring por caminho. Dois bugs reais achados no cruzamento com o [ADR-0034](adr/0034-deteccao-de-processo-ativo-para-confirmacao.md): `action_close_tab`/`close_tab_via_button` tinham `self.tabs.get(&id)?`, que devolveria `None` e não fecharia nada numa aba sem `TabRuntime` — corrigido com a checagem `is_not_started()` **antes** da detecção, como o §3 do ADR pede; e `request_close_window` contava `tab_count` do `Workspace` (confirmaria "mais de uma aba" numa janela só com `NotStarted`) — passou a contar `state.tabs.len()` (o mapa de `TabRuntime`, que só abas iniciadas ocupam). Rótulo esmaecido (RF-3.9): `chrome::label_color`, alfa `NOT_STARTED_LABEL_ALPHA` igual a `DRAG_HIGHLIGHT_BORDER_ALPHA` (`.45`, nenhum valor novo), nos três pontos que já desenhavam o rótulo (normal, fade-out do colapso, fantasma de arraste); indicadores e indicador agregado não precisaram de código novo, porque `activity`/`bell` são estruturalmente `false` sem PTY, mesma razão que já valia para `Exited`. `verify-docs.py` não precisou de entrada nova: `chrome.rs` não está nos arquivos que a tabela `VALORES` rastreia. Testes: `porecatu-core` 76→83, `porecatu-ui` 214→220 — funções puras (o estado do core, o alfa, o gate de confirmação de fechamento); `WindowState`/`App` seguem sem unit test, mesma fronteira de GPU/janela das fases anteriores. Verificação manual real: `cargo run` com um hook de debug temporário (removido antes do commit) forçando uma aba a `NotStarted` via clique do meio, e clique sintético — que atravessa a proteção de foco do Windows — confirmando ao vivo os três comportamentos (rótulo esmaecido só no texto, foco sobe o shell e apaga o esmaecimento, fechar não confirma). Teclado sintético continua bloqueado, confirmado de novo neste caminho: um `Ctrl+T` de teste foi para a janela em foco de verdade (Firefox do usuário) em vez da janela de debug, por isso o resto da verificação usou só mouse. `lazy_restore = false` não tem teste próprio: nenhum caminho de produção produz `NotStarted` ainda (a etapa 4 liga o produtor real), então a regra vale por vacuidade.
4. **Restauração no start — fechada.** `App::resumed` carrega `porecatu_session::load()` (guardado em `pending_session`, consumido só aqui: sem `ActiveEventLoop`/janela antes disso para restaurar ou avisar) e, com ao menos uma `WindowV1`, chama `App::open_window_from_session` por janela em vez do caminho de sempre (`open_window`, janela em branco) — os dois compartilham a criação de janela/surface/GPU/métrica de célula por um núcleo comum extraído, `App::create_window_with_attributes`. `porecatu-core` ganhou o construtor que a etapa 3 deixou para esta ("reservado à restauração"): `Tab::new_not_started`/`Workspace::new_tab_not_started`, que **não ativa** a aba criada (`new_tab` continua ativando, para não regredir o comportamento de `tab.new`). `porecatu_session::convert::workspace_from_window` ganhou o parâmetro `lazy_restore`: aba cujo `id` do arquivo bate com `active_tab` nasce por `new_tab` (`Running`), toda outra nasce por `new_tab_not_started` (`NotStarted`) quando `lazy_restore = true` — com `false`, todas nascem `Running`, sem segundo caminho de código (só a escolha do construtor por aba muda). RF-2.17 ponta a ponta saiu de graça do mecanismo que já existia: como a aba real ativa é sempre criada por `new_tab` (que a ativa transitoriamente) e o `ws.activate_tab(active)` final da função roda **depois** do `ws.collapse_group(..., true)` de dentro do laço, ativar a aba certa no fim expande o grupo que a contém — testado (`restoring_the_active_tab_inside_a_collapsed_group_expands_it`) e confirmado ao vivo (ver verificação manual). Geometria/monitor (RF-3.11, ADR-0036 §4): `session_writer::resolve_restored_geometry`/`match_restored_monitor` (nome → posição → primário com tamanho preservado dentro dos limites — mesmo clamp que a cascata de `window.new` já usava, aqui contra o primário), puras e testadas sem `winit::monitor::MonitorHandle` via `MonitorInfo`; `restore_window_geometry = false` pula a chamada inteira, sem tocar o resto da restauração. `cwd` inexistente (RF-3.10, ADR-0017 §5): a checagem (`Path::is_dir`) e a nota (`Terminal::inject_note`, `#5ed3bc`) entram em `WindowState::spawn_tab_runtime` — o único ponto onde qualquer aba sobe de verdade, seja `tab.new`, o primeiro foco de uma `NotStarted` (etapa 3) ou a restauração —, nunca adiantada para toda aba `NotStarted` de uma sessão (seria N `exists()` no caminho do start para abas que talvez nunca sejam focadas); a posição "primeira linha" sai de graça de **quando** a nota é injetada (antes de qualquer byte do PTY chegar à grade, ainda em branco), não de um parâmetro novo em `inject_note`. Avisos de recuperação (RF-3.14/RF-3.16, `Notice::Corrupt`/`Notice::NewerSchema`) chegam à barra do ADR-0014 na primeira janela criada em `resumed`, severidade `Error` (mesmo tratamento de "config inválida"). Entrega RF-3.7, RF-3.10, RF-3.11 e a metade de restauração do RF-3.17. Testes: `porecatu-core` 83→85, `porecatu-session` 19→22, `porecatu-ui` 220→230. Verificação manual real, antes de ser interrompida (ver dívida): `PORECATU_SESSION` apontando para um `session.json` escrito à mão com duas janelas — uma com grupo explícito colapsado contendo a aba ativa e `cwd` inexistente na outra aba do grupo, outra com geometria de um monitor que não existe mais (nome e posição sem casamento nenhum). `cargo run` restaurou as duas: o título da janela 1 (`backend`) bateu com o `custom_title` da aba ativa restaurada; a posição/tamanho da janela 1 (100,100,900,650) bateram exatamente com o gravado, monitor casado por nome; a gravação de sessão que o próprio debounce disparou durante o teste confirmou, sem intervenção nenhuma, que o grupo salvou como **`collapsed: false`** (RF-2.17 expandiu de verdade) e que a janela 2 nasceu na geometria calculada pelo fallback (monitor primário, tamanho preservado) — exatamente os números que `resolve_restored_geometry` prevê. **Dívida**: a confirmação visual do rótulo esmaecido, da nota de `cwd` ausente e da barra de aviso de arquivo corrompido, mais a **métrica de 20 abas em menos de 1 s** (que a etapa 6 da F6 instrumenta e mede), não foram completadas — a verificação foi interrompida a pedido do usuário (um `screenshot` de tela cheia capturou, sem querer, outra janela dele com dados sensíveis, e abrir uma segunda janela de 1920×1080 de teste na tela dele o incomodou). Ficam registradas como dívida explícita desta etapa, não como "passou".
5. **Linha de comando e fallback de `cwd` — fechada.** Dois assuntos independentes, dois commits, um PR. **Parte A** ([ADR-0040](adr/0040-superficie-de-linha-de-comando.md)): `src/cli.rs`, no binário (não em `porecatu-ui` -- `argv` é do processo), com `parse` puro sobre `impl IntoIterator<Item = OsString>` (nunca `args()`/`String`, pelo mesmo motivo de `resolve_config_path`) devolvendo `Cli::{Help, Version, Run { config, directory }}`. `porecatu_ui::run` muda de assinatura uma vez, para `run(cli_config: Option<PathBuf>, cli_directory: Option<PathBuf>)`, repassados a um `App::new` que também ganhou os dois parâmetros -- `load`/`resolve_config_path` chamam com `cli_config.as_deref()` no lugar do `None` fixo, o que fecha a dívida da etapa 1 da F4 registrada acima. RF-3.12 (caminho posicional): `App` ganhou `positional_directory: Option<PathBuf>`; `Some` faz `pending_session` nem carregar (a sessão gravada não é lida) e `App::session_persistence_enabled` (o único ponto que decide gravar, desde a etapa 2) devolver `false` mesmo com `[session] enabled = true` -- os dois chamadores (debounce e exit) não mudaram uma linha. A janela do arranque nesse modo é criada por um novo núcleo compartilhado, `App::open_window_with` (extraído do fim de `open_window`), com o `cwd` passado direto, vencendo `startup_directory` por construção -- não por prioridade numa cadeia de `or_else`. Validação do caminho posicional (existe, é diretório) mora em `main.rs` (`cli::validate_directory`, impura de propósito -- toca disco depois que o parse, que é puro, já decidiu que há algo pra validar): erro é `stderr` + saída ≠ 0, nunca o fallback silencioso pro home que o RF-3.10 usa para um `cwd` **gravado** que sumiu. **Parte B** ([ADR-0038](adr/0038-fallbacks-de-cwd.md)): `ProcessGroup::cwd()` em `porecatu-pty/src/job.rs`, `#[cfg(any(target_os = "linux", target_os = "macos"))]` -- ausente no Windows por erro de compilação, não decisão em runtime. `sysinfo` (mesma versão/features `default-features = false, features = ["system"]` do Windows) entra como dependência também para esses dois alvos; a consulta é pontual (`ProcessesToUpdate::Some(&[pid])` com `ProcessRefreshKind::nothing().with_cwd(...)`), nunca a lista inteira. `for_child` fora do Windows deixou de devolver `None` sempre -- passou a guardar o `root_pid` de verdade (só isso; `process_count`/`kill_tree` continuam degradados, dívida assumida à parte). Precedência completa (`Tab::cwd`/OSC 7 → `ProcessGroup::cwd()` → `cwd` de spawn), montada em `App::build_session_file`: `TabRuntime` ganhou `spawn_cwd` (o `cwd` já resolvido pelo RF-3.10 que efetivamente foi ao `SpawnConfig`, já que `porecatu_core::Tab` só guarda o que OSC 7 reporta); `Terminal::cwd_fallback()` (novo, mesmo padrão de `has_extra_processes`) delega a `ProcessGroup::cwd` nos dois alvos e devolve `None` sempre no Windows, sem expor `#[cfg]` pra `porecatu-ui`; `session_writer::window_v1` ganhou o parâmetro `cwd_fallback: impl Fn(TabId) -> Option<PathBuf>`, consultado só para a aba cujo `cwd` saiu `None` da conversão -- com OSC 7 presente, o closure nem é chamado. Entrega RF-3.12 e a metade Linux/macOS do fallback de `cwd`. Testes: `porecatu` (binário) 0→11, `porecatu-ui` 230→233. **Como se provou que o modo posicional não sobrescreve**: `session.json` de rascunho com conteúdo e `mtime` conhecidos, `porecatu.exe <diretório>` rodado de verdade (`PORECATU_SESSION` apontando pro rascunho) por PID exato, esperado por 4s (mais que o debounce padrão de 2s) e encerrado por `Stop-Process -Id` exato -- `mtime`/hash/conteúdo do arquivo, idênticos antes e depois. `--config` vencendo `PORECATU_CONFIG`: dois arquivos de config com `startup_directory` diferentes, `--config` apontando pro primeiro e a variável de ambiente pro segundo -- o `session.json` que o próprio debounce escreveu, no modo normal (não posicional), gravou o `cwd` do primeiro. `--help`/`--version` imprimem e saem com código 0 sem abrir janela; argumento desconhecido, `--config` sem valor e caminho posicional inexistente saem com código 2, também sem janela -- os três confirmados rodando o binário de verdade. Fallback de `cwd` em si (Parte B) não foi exercitado **localmente** (sem ambiente Linux/macOS neste fluxo), mas o teste de integração (`crates/porecatu-pty/tests/cwd.rs`, `#[cfg]`-gated pra rodar só lá) **passou de verdade no CI das três plataformas** (PR #59) -- spawna um shell, faz `cd`, e confirma que `ProcessGroup::cwd()` lê o valor certo do SO, em Linux e macOS reais, não só compilação. Sem desvio de ADR; nenhuma dependência nova (Parte B reusa `sysinfo`, já vetado desde o ADR-0033).
6. **Convite à integração de shell, tema e zoom — fechada.** Duas partes independentes, mais o fechamento da fase. **Parte A** ([ADR-0039](adr/0039-convite-a-integracao-de-shell.md), RF-3.1): a nota entra por `Terminal::inject_note`, na primeira aba que provar não ter OSC 7 — fora do Windows, o gatilho é o mesmo do [ADR-0038](adr/0038-fallbacks-de-cwd.md) §5 (`ProcessGroup::cwd()` diferente do `cwd` de spawn, consultado só na gravação da sessão, nunca por frame); no Windows, temporal (`TabRuntime::spawned_at + WINDOWS_SHELL_INTEGRATION_INVITE_INTERVAL`, **3 segundos**, escolhido porque um `precmd`/`PROMPT_COMMAND` configurado emite OSC 7 no primeiro prompt, bem menos de um segundo depois do spawn — 3s cobre shell lento pra subir sem atrasar o convite pra quem não tem OSC 7 nenhum). Os dois sinais convergem no mesmo ponto único, `App::claim_shell_integration_invite` — "uma vez por execução" é literal: `shell_integration_invite_claimed` fecha a porta pros dois gatilhos assim que qualquer um dispara. Tela alternativa (`Terminal::modes().alt_screen`) adia a escrita para o próximo `Wakeup::TabDirty` daquela aba (`pending_shell_integration_note`), nunca escreve calado. Os snippets são **embutidos de `docs/reference/integracao-de-shell.md`** via `include_str!` (`porecatu-ui/src/shell_integration.rs`): extrai o primeiro bloco de código depois do heading do shell detectado (`Tab::shell_name`) — bash, zsh, fish, PowerShell; `cmd` cai numa explicação fixa (o arquivo de referência já diz que não há forma confiável ali) e shell desconhecido numa nota genérica, nunca no snippet de outro shell. **A dispensa definitiva é digitada no terminal**, não é gesto de teclado do app: o texto da nota pede que o usuário digite um marcador (`dispensar-convite-porecatu`) e dê Enter — um segundo parser `vte`, independente do motor (mesmo padrão do `Osc7Watcher` da F2, `porecatu-term/src/dismiss.rs`), observa o eco do próprio PTY por essa substring e emite `TermEvent::ShellIntegrationDismiss`; o shell provavelmente responde "comando não encontrado", efeito colateral aceito e avisado na própria nota. Grava em `shell_integration_dismissed` (`session.json`, campo que já existia desde o ADR-0036). **Parte B** (ADR-0031 §4, ADR-0036 §3): tema e zoom de sessão, até aqui só gravados (a etapa 5 já preenchia `WindowV1::theme`/`zoom_steps`, porque o DTO exigia o parâmetro), passam a também **restaurar**: `App::apply_restored_session_state`, chamado em `resumed` antes de qualquer janela abrir (`font_zoom_px` decide a métrica de célula que `create_window_with_attributes` usa pra todas), toma a primeira janela do envelope como o estado do processo inteiro — mesma simplificação de `session_zoom_steps`, já que zoom e tema continuam sendo do processo, não da janela (dívida da F4 etapa 6, não paga aqui). Tema restaurado que sumiu do arquivo leva o mesmo tratamento que o hot reload já dava (zera, avisa, nunca aplica calado) — extraído para `session_writer::restore_theme`, puro. Conversão de passos para pixels é `session_writer::steps_to_zoom_px`, inverso exato de `zoom_px_to_steps`, com o mesmo clamp que `App::apply_zoom` já usa ao vivo. **Parte C**: a caixa "Estado na F2, e como a F5 o fecha" do [catálogo de ações](reference/acoes.md) saiu — deixou de ser verdade na etapa 2 da F5, ficou só desatualizada até agora. Sem desvio de ADR, sem dependência nova, sem `unsafe`. Testes: `porecatu-term` 28→33 (o marcador de dispensa, inclusive dividido entre dois lotes de leitura), `porecatu-ui` 233→261 (elegibilidade do convite, os dois gatilhos, adiamento por tela alternativa, extração de snippet por shell, conversão de zoom/tema de restauração — tudo como função livre e pura, o mesmo padrão de `should_confirm_tab_close`), `porecatu-session` 22→23 (a dispensa sobrevive a um round-trip do arquivo). **Desvio**: `windows_invite_timeout_due` (só chamada dentro do `#[cfg(windows)]` de `check_shell_integration_invite_timeout`) reprovou o clippy do CI em Linux e macOS por `dead_code` — não local, Windows, onde a função tem chamador — a mesma armadilha de `#[cfg]`-por-plataforma já registrada na correção de processo zumbi. Corrigida com `#[cfg(windows)]` na própria função (e nos três testes que a chamam), não com `#[allow]`.

**Verificação manual real, parcial**: instância de teste (`PORECATU_CONFIG`/`PORECATU_SESSION` de rascunho, `[shell] program = "cmd.exe"`, PID exato) ficou viva mais de 5s sem travar nem sair — o gatilho temporal do Windows disparou nesse intervalo sem gerar pane — e o `session.json` que ela escreveu tem a forma esperada (`shell_integration_dismissed: false`, `spawn_program: "cmd"`, `theme`/`zoom_steps` presentes). **Dívida**: a aparência da nota em si (cor, posição, texto do snippet renderizado) e a dispensa digitada não foram confirmadas ao vivo — a primeira exigiria ler a grade por captura de tela (fora de cogitação desde a etapa 4) e a segunda é gesto de teclado, bloqueado pela proteção de foco do Windows (mesma dívida da F1 em diante). Ambas cobertas só por teste automatizado.

A ordem não é arbitrária: 1 destrava tudo; 2 e 4 são as duas metades do recurso e 4 depende de 2; 3 é pré-requisito de 4 (sem `NotStarted` não há restauração preguiçosa); 5 e 6 são independentes entre si e podem trocar de posição.

**Critério de saída:** todos os cenários de aceite de PRD-003 passam; restauração de 20 abas em menos de 1 s; teste de crash durante a gravação preservando a sessão anterior; limitação do Windows sem OSC 7 verificada e documentada como comportamento esperado, não como bug.

**O que a F5 não faz:** mexer na interface além do rótulo esmaecido do RF-3.9, que é a exceção decidida pelo [ADR-0037](adr/0037-aba-nao-iniciada.md) §5 — o [ADR-0032](adr/0032-interface-do-v1-fechada.md) continua de pé para todo o resto. Também não persiste scrollback (fora do v1) nem restaura processos (fora do requisito, por definição).

**O que já não precisa nascer.** A configuração `[session]` está completa desde a F4 (cinco chaves, defaults alinhados ao arquivo de exemplo, classe de recarga C fiada no hot reload) e **sem nenhum consumidor**; o `serde` do domínio e o teste de round-trip estão em `porecatu-core` desde a F2; a captura de OSC 7 e o `TermEvent::Cwd` estão em `porecatu-term` desde a F2, antecipados pelo ADR-0017; o `inject_note` que o RF-3.10 e o RF-3.1 usam já é chamado em produção; e o `Wakeup` com `(WindowId, TabId)` foi resolvido antes da F1 justamente pensando na fase em que duas janelas seriam restauradas ([ADR-0015](adr/0015-multiplas-janelas.md)).

**Dívida herdada de verificação.** Sete dos oito cenários de aceite do PRD-003 rodam sem gesto de teclado — fechar e reabrir, arquivo corrompido, schema mais novo, crash na gravação, diretório removido, argumento de linha de comando. Os dois pontos que puxam a dívida das fases anteriores são "o shell de uma aba inicia quando ela é focada" e dispensar o convite do RF-3.1: os dois pedem foco real. **Mouse sintético atravessa a proteção de foco do Windows** (descoberta da F4), então focar uma aba não iniciada com clique **é** verificável sem pedir gesto ao usuário; o resto continua sendo dívida assumida. O fallback do macOS entra verificado só por compilação e CI, e está registrado como tal no ADR-0038.

**Dívida acrescentada pela etapa 6.** A aparência real da nota do convite (posição depois da última linha, cor `#5ed3bc`, snippet legível) não foi confirmada por captura de tela — decisão da etapa 4, mantida aqui. A dispensa digitada (`dispensar-convite-porecatu` + Enter) é gesto de teclado, mesma proteção de foco do Windows que já bloqueia o resto; a detecção do marcador em si tem cobertura automatizada completa (`porecatu-term`, incluindo o marcador dividido entre dois lotes de leitura do PTY), só o gesto de digitar de verdade numa janela real não foi exercitado. Nenhuma dívida nova de arquitetura: os dois gatilhos (fallback e temporal) convergem num ponto só, testado como função pura nos dois lados.

---

## F6 — Polimento

Implementa [PRD-011](prd/prd-011-polimento.md), RF-11.1 a RF-11.30. É o que separa "funciona" de "usável o dia inteiro", e o que separa "compila aqui" de "instalável por outra pessoa".

Antes de a fase abrir, quatro ADRs fecharam as decisões que faltavam — mesmo movimento dos ADR-0017 a 0019 entre a F1 e a F2, dos ADR-0020 a 0023 entre a F2 e a F3, e dos ADR-0036 a 0040 na abertura da F5. **Não é preâmbulo opcional:** três deles respondem perguntas que ADRs anteriores deixaram escritas, e um decide a fronteira de segurança de um recurso novo.

Diferente das outras aberturas, aqui faltava também o **requisito**. A F6 era a única fase do v1 sem PRD: a seção não citava um único RF, e as ações `search.*` do catálogo tinham origem *"roadmap F6"* — contra a métrica do [PRD-010](prd/prd-010-interacao-e-superficie-de-app.md) (*"ações do catálogo sem origem em RF ou ADR: zero"*) e contra a regra do próprio catálogo (*"o caminho é PRD ou ADR primeiro"*). O PRD-011 fecha essa lacuna, e ao fechá-la trouxe para dentro da fase **sete requisitos aprovados de fases anteriores que nunca foram entregues** e não estavam registrados em lista nenhuma (RF-11.24 a RF-11.30).

- [**ADR-0041**](adr/0041-busca-no-scrollback.md) — a busca é uma **barra sobreposta ao topo do quadro do terminal**, na camada `Chrome`. Responde a pergunta que o [ADR-0018](adr/0018-composicao-de-frame.md) deixou escrita (*"superfície nova precisa escolher uma camada existente ou justificar uma nova"*) **sem camada nova**, e carrega a anatomia do sexto widget de chrome, porque o [ADR-0032](adr/0032-interface-do-v1-fechada.md) exige ADR para qualquer mudança das seções 1/2. Decide também a **sobreposição em vez de reflui** — empurrar a grade mandaria `resize` de PTY a um programa em execução —, a captura de teclado **parcial** (a primeira superfície não modal do app; captura total tornaria `search.next` inalcançável por tecla) e as ocorrências como lista de ranges, não como bit em `CellFlags`.
- [**ADR-0042**](adr/0042-hyperlinks-osc-8.md) — hyperlinks OSC 8. O URI viaja como **lista de spans ao lado do snapshot**, então `Cell` sai da fase byte a byte igual e continua `Copy`. O gesto é `Ctrl`+clique (`Cmd` no macOS, pela mesma razão do [ADR-0021](adr/0021-selecao-multipla-e-gestos-da-barra.md)), a affordance só aparece sob o modificador, e a decisão de segurança: quatro esquemas aceitos, com **`file` revelado no gerenciador de arquivos e nunca entregue ao handler por extensão** — o URI vem da saída de um programa, e no Windows um `.exe` executaria.
- [**ADR-0043**](adr/0043-arvore-de-acessibilidade.md) — `accesskit` sobre o chrome, com a árvore como **projeção das funções puras de layout que já existem** (árvore construída à parte divergiria do desenho no primeiro PR, e árvore que mente é pior que árvore ausente). Construção só com leitor de tela ativo, e nunca dentro do caminho de render — o [ADR-0007](adr/0007-modelo-de-threading.md) segue intacto. Paga a dívida mais antiga do projeto, registrada no [ADR-0001](adr/0001-stack-de-gui.md) antes da F1, e corrige a conta: são **cinco** papéis de widget, não três.
- [**ADR-0044**](adr/0044-empacotamento-e-release.md) — instalador nativo por plataforma, primeira release **`1.0.0`**, sem assinatura de código. Traz também três correções que são a mesma conversa: `x86_64-apple-darwin` entra na matriz, `--locked` passa a valer no `ci.yml` (a última pendência aberta da F0) e a **atribuição das fontes embutidas passa a acompanhar o artefato** — hoje o `release.yml` copia só `LICENSE` e `README.md`, e publicar assim seria descumprir a OFL e a ISC.

Itens:

- Busca no scrollback, com as ações `search.*` do [catálogo](reference/acoes.md): incremental, literal e regex, circulando nas duas pontas, com realce da ocorrência ativa distinto das demais
- Hyperlinks OSC 8, com clique sob modificador e esquemas fechados — vira a linha `| OSC 8 — hyperlinks | não — F6 |` da lista fechada do [ADR-0012](adr/0012-identificacao-do-terminal.md)
- **Acessibilidade via `accesskit`** — dívida assumida em [ADR-0001](adr/0001-stack-de-gui.md), não esquecimento. Cobre a barra de abas e os **cinco** widgets do [ADR-0014](adr/0014-superficie-de-aviso-e-dialogo.md) e seguintes: diálogo modal, aviso, menu, tooltip e editor de grupo — os papéis que leitor de tela trata de forma especial
- Menu de contexto do terminal, com `selection.select_all` — o terceiro menu, que a §2.16 da [especificação visual](design/especificacao-visual.md) já previa
- Os sete requisitos aprovados e não entregues: avisos de config no arranque (RF-4.21, RF-4.22, RF-5.18), aviso de fonte ausente (RF-5.8), fallback de GPU detectado e avisado em vez de `panic`, botão de configurações deixando de ser inerte, `zoom_scope = "active"`, cor hexadecimal do RF-2.10, e gesto para `scrollback.to_top`/`to_bottom`
- Instrumentação das cinco métricas do [PRD-000](prd/prd-000-visao-de-produto.md) — incluindo a de 20 abas em menos de 1 s, dívida da etapa 4 da F5
- Empacotamento por plataforma. **O ícone já existe**, entregue fora de fase: PNG embutido decodificado em runtime para toda janela (`app_icon.rs`) e o `.ico` embutido como recurso PE no Windows por um `build.rs` com `winres`
- Documentação de usuário e página de release

**Aparência:** a fase acrescenta **uma** seção de anatomia à [especificação visual](design/especificacao-visual.md) — a **§2.21**, a barra de busca —, e é a segunda mudança das seções 1/2 depois do [ADR-0032](adr/0032-interface-do-v1-fechada.md), passando pelo ADR que ele exige (a primeira foi o rótulo esmaecido do ADR-0037, na abertura da F5). **Nenhuma cor, dimensão, raio, espaçamento ou ícone novo:** a barra reusa o campo de texto do editor de grupo (inclusive a seleção do [ADR-0035](adr/0035-selecao-de-texto-em-campo-de-nome.md)), o toggle que já estava na §1.5 sem consumidor no v1, os ícones `CHEVRON_LEFT`/`CHEVRON_RIGHT`/`X` já recortados, e — para o realce das ocorrências — a cor de seleção de texto e o acento. A affordance do hyperlink é a flag `UNDERLINE` que o pintor já desenha. **A trilha de grupos e abas não é tocada**, que é o que o ADR-0032 §2 protege.

**Divisão sugerida**, no padrão das seis etapas da F1, da F2, da F3, da F4 e da F5, uma por PR:

1. **Busca no motor e no modelo — fechada.** `porecatu-term` ganhou `search.rs`: `SearchMode` (literal, default, ou regex — parâmetro, não duas funções), `GridPos`/`Occurrence` (ranges de posição na grade, ADR-0041 §4 — nenhum tipo do `alacritty_terminal` atravessa a fronteira) e `InvalidPattern` (regex que não compila é erro devolvido, `RegexSearch::new` mapeado, nunca `panic`). Busca literal escapa metacaracteres à mão antes de compilar como regex (`escape_literal`, sem depender de crate novo — o `alacritty_terminal` não expõe o escape do `regex-automata` que usa por baixo). Escopo é `term.topmost_line()`..`term.bottommost_line()` via `RegexIter` (tela visível mais scrollback inteiro, RF-11.3); com `TermMode::ALT_SCREEN` ativo isso já é só a tela visível de graça (a tela alternativa não tem histórico), e `SearchOutcome`/`SearchJob::scope_reduced` sinaliza isso explicitamente (RF-11.8) em vez de devolver zero calado. `GridSnapshot` ganhou `occurrences: Vec<OccurrenceSpan>` (viewport-relative, mesma forma de `SelectionSpan`) e `TermEngine::snapshot_into` só limpa esse buffer por frame, sem realocar (ADR-0007) — quem preenche cortando pela vista e resolvendo a cor é `porecatu-ui`, na etapa 2 (ADR-0041 §4: "a mesma divisão de trabalho que já vale para a paleta"). Testes: `porecatu-term` 33→34 unitários (`escape_literal`), mais 13 de integração novos (`tests/search.rs`, 8; `tests/search_batching.rs`, 3; `tests/search_scrollback_cost.rs`, 2) — tudo função pura, sem GPU e sem janela, mesmo padrão da etapa 1 da F4 e da F5.

   **Medição do item 5, e o desvio que ela forçou.** Uma busca de uma vez só (`RegexIter` do início ao fim da grade, sem lote) num scrollback cheio no *default* de `scrollback.lines` (10.000) já estourou o orçamento de um frame a 60fps antes de chegar ao teto configurável: **22ms (literal) / 18.6ms (regex) em release, 125ms em debug** — contra ~16ms de orçamento —, e em dez vezes esse valor (100.000 linhas, teto plausível já que `porecatu-config` não impõe um máximo) chegou a **203ms/207ms em release, ~1s/2.1s em debug**. Reportado ao usuário antes de seguir, como o ADR-0041 pede ("busca em scrollback grande travar a UI" — risco do §"Riscos e mitigação", que já nomeava a saída: "a busca vira incremental por lotes, e isso muda o desenho da API"). Por decisão do usuário, a API saiu incremental: `TermEngine::start_search`/`step_search` devolvem um `SearchJob` que `step` varre em lotes de `DEFAULT_SEARCH_LINES_PER_STEP` (100) **linhas**, não ocorrências — decisivo, porque um lote por ocorrência não protegeria uma query sem nenhuma (o caso em que a grade inteira seria varrida mesmo assim para concluir "zero"; testado em `sem_ocorrencia_nenhuma_o_lote_ainda_limita_o_passo`). Resume exatamente de onde parou usando a mesma aritmética de ponto que `RegexIter::skip` (`Term::expand_wide` + `Point::add`), sem reimplementar a busca em si — só o corte em lotes. Com o lote de 100 linhas, o pior passo medido caiu para poucos milissegundos tanto em 10.000 quanto em 100.000 linhas, em release e em debug (`tests/search_scrollback_cost.rs`). Isto é uma elaboração da leitura literal do ADR-0041 §4 ("devolve ocorrências como ranges de posição na grade") — que se lia como um retorno único — não uma contradição da decisão: o próprio ADR já previa e nomeava esta saída, contingente à medição que esta etapa fez. Nenhum outro ponto do ADR-0041 foi desviado.
2. **Barra de busca na UI.** O widget, a captura de teclado parcial, a camada `Chrome`, o realce no pintor com a reserva de altura no alvo de rolagem, as ações `search.*` com defaults por plataforma — `Ctrl+Shift+F`/`Cmd+F`, `F3`, `Shift+F3` e `Ctrl+Shift+A`/`Cmd+A`, hoje **comentados** no arquivo de exemplo porque ligar a tecla antes do despacho faria o app engolir `F3` sem repassar ao terminal ([ADR-0041](adr/0041-busca-no-scrollback.md) §10) —, e a §2.21 na especificação visual. O RF-2.17 fecha por completo aqui, pela segunda fonte que ele sempre citou. Entram também, por serem a mesma conversa de `[keybindings]`, as duas ações que a F3 deixou sem despacho: `scrollback.to_top` e `scrollback.to_bottom`.
3. **Hyperlinks OSC 8 e menu de contexto do terminal.** Os spans e a arena de URI no snapshot, o hit-test do link, a affordance sob modificador, a política de esquemas com `file` revelado, e a chave `[terminal.hyperlinks]`, que nasce aqui em `porecatu-config` (está comentada no arquivo de exemplo, porque seção que a struct não conhece reprova a auditoria de config da F4); e o terceiro menu, com `selection.select_all`, mais os itens de link quando o clique cai sobre um.
4. **Os requisitos aprovados e não entregues.** Avisos de config no arranque pelo caminho que a etapa 4 da F5 abriu (aviso na primeira janela criada em `resumed`), RF-5.8, detecção e aviso de fallback de GPU sem `panic`, botão de configurações abrindo o arquivo de config, `zoom_scope = "active"` (que pede métrica de célula por `TabRuntime`, não só por processo) e a cor hexadecimal do RF-2.10.
5. **Acessibilidade.** `accesskit_winit` na versão que casa com o `winit` já pinado, a árvore projetada do layout, a barra de abas e os cinco widgets — mais a barra de busca da etapa 2, que nasce acessível em vez de virar dívida.
6. **Métricas, empacotamento e release.** Instrumentação das cinco métricas do PRD-000, os instaladores das três plataformas, `x86_64-apple-darwin` na matriz, `--locked` no `ci.yml`, a atribuição das fontes dentro do artefato, `version` a `1.0.0`, documentação de usuário, `CHANGELOG`, página de release e o fechamento da fase.

A ordem não é arbitrária: 1 destrava 2, e as duas são as duas metades da busca; 3 é independente de 1 e 2 e pode andar em paralelo; 4 não depende de nada e é a etapa que pode ser cortada se o prazo apertar, porque cada item dela é isolado; 5 depende de 2, porque a barra de busca é um dos nós da árvore; 6 é última por definição.

**Critério de saída:** todos os cenários de aceite de PRD-011 passam; **leitor de tela navega a barra de abas e os cinco widgets** — emendado de "a barra de abas", pelo [ADR-0043](adr/0043-arvore-de-acessibilidade.md), que fechou o escopo em cinco papéis em vez de um; as cinco métricas de [PRD-000](prd/prd-000-visao-de-produto.md) **medidas por instrumentação** e atingidas; busca num scrollback cheio sem travar a UI; nenhum esquema de URI fora dos quatro é executável por um gesto; instaladores nativos publicados para as três plataformas, com a atribuição das fontes dentro.

**O que a F6 não faz:** expor a **grade do terminal** ao leitor de tela — limitação declarada do v1, com as três decisões que faltam nomeadas no [ADR-0043](adr/0043-arvore-de-acessibilidade.md) §5; **notificação de desktop na campainha**, que sai do v1 (era item desta fase, e é superfície nativa do sistema — o [ADR-0014](adr/0014-superficie-de-aviso-e-dialogo.md) decidiu que nada da interface escapa da config do usuário, e o indicador na aba do RF-1.21 é a superfície que o produto controla); **assinatura de código**; detecção de URL em texto plano, sem OSC 8; e nada de aparência fora da §2.21 que o ADR-0041 aprova.

**O que já não precisa nascer.** O `alacritty_terminal` já traz `term::search::RegexSearch` e `RegexIter`, e **já parseia OSC 8** até `Cell::hyperlink()` pelo `Handler` — ao contrário do OSC 7, que o `vte` descarta e que precisou de um segundo parser na F2. O campo de texto com cursor e seleção existe desde o ADR-0035 e é compartilhado com o editor de grupo; o widget de menu já prevê o terceiro menu na §2.16; as cinco camadas do [ADR-0018](adr/0018-composicao-de-frame.md) bastam; o `release.yml` já compila e publica nas três plataformas; o ícone veio fora de fase; e `accesskit_winit` resolve contra o mesmo `winit 0.30.13` do projeto, sem duplicar `winit` na árvore — verificado na abertura, não presumido.

**Dívida herdada de verificação.** A nota do topo continua valendo, e esta fase acrescenta um ponto novo: a acessibilidade só é verificável com leitor de tela real, e este fluxo tem um sistema. O NVDA no Windows é gratuito e **não depende de input sintético**, então a metade Windows é verificável de verdade; macOS e Linux entram por compilação e CI, como o fallback de `cwd` do [ADR-0038](adr/0038-fallbacks-de-cwd.md) entrou. Instalação real dos três instaladores tem a mesma limitação. Mouse sintético atravessa a proteção de foco do Windows (descoberta da F4), então clicar num hyperlink, abrir o menu do terminal e usar os botões da barra de busca **são** verificáveis sem pedir gesto ao usuário; digitar na busca, não.

---

## Fora do v1

Registrado para não ser reinventado como ideia nova. Cada item está justificado nos PRDs correspondentes.

A coluna **Desenhado** marca o que já tem alvo visual aprovado no canvas. Estar desenhado não traz nada para o v1 — só significa que, quando entrar, a aparência já está decidida ([ADR-0009](adr/0009-referencia-visual-e-reconciliacao.md)).

| Item | Onde está justificado | Desenhado |
|---|---|---|
| Splits / panes na aba | [PRD-000](prd/prd-000-visao-de-produto.md), [ADR-0006](adr/0006-modelo-de-abas-e-grupos.md), [PRD-006](prd/prd-006-paineis-divididos.md) *(rascunho)* | sim, `[v2]` |
| Perfis de aba (WSL, SSH, container) | [PRD-000](prd/prd-000-visao-de-produto.md), [PRD-007](prd/prd-007-perfis-de-aba.md) *(rascunho)* | sim, `[v2]` |
| Paleta de comandos | [PRD-008](prd/prd-008-paleta-de-comandos.md) *(rascunho)* | sim, `[v2]` |
| Barra de status | [PRD-009](prd/prd-009-barra-de-status.md) *(rascunho)* | sim, `[v2]` |
| Configuração por GUI | [ADR-0003](adr/0003-formato-de-configuracao.md), [ADR-0009](adr/0009-referencia-visual-e-reconciliacao.md) | sim, `[v2]` |
| Faixa de identidade da barra de título (logo, nome do app, título da aba ativa) | [ADR-0009](adr/0009-referencia-visual-e-reconciliacao.md) (parcial, ver [ADR-0027](adr/0027-controles-de-janela-e-resize-proprios.md)), [PRD-004](prd/prd-004-aparencia-do-chrome.md) | sim, `[v2]` |
| Multiplexação remota | [PRD-000](prd/prd-000-visao-de-produto.md) | não |
| Plugins e config programável | [ADR-0003](adr/0003-formato-de-configuracao.md) | não |
| Persistir scrollback | [PRD-003](prd/prd-003-persistencia-de-sessao.md) | não |
| Mover aba entre janelas | [PRD-001](prd/prd-001-abas.md) | não |
| Temas como arquivo importável | [PRD-004](prd/prd-004-aparencia-do-chrome.md), [PRD-005](prd/prd-005-aparencia-do-terminal.md) | não |
| Tema claro/escuro seguindo o sistema | [PRD-004](prd/prd-004-aparencia-do-chrome.md) | não |
| Agrupamento automático por projeto | [PRD-002](prd/prd-002-grupos-de-abas.md) | não |
| Protocolos de imagem (sixel) | [ADR-0002](adr/0002-motor-vte.md) | não |
| Notificação de desktop na campainha | [PRD-011](prd/prd-011-polimento.md), [ADR-0014](adr/0014-superficie-de-aviso-e-dialogo.md) — era item da F6 e saiu na abertura dela | não |
