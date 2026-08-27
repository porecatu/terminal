# Roadmap

Fases de entrega do v1. Cada fase tem **critério de saída explícito** — enquanto ele não for atendido, a fase não terminou e a próxima não começa.

A ordem não é arbitrária: cada fase entrega algo utilizável e é pré-requisito real da seguinte. Em particular, F1 entrega um emulador de terminal funcional; tudo depois disso é o diferencial do produto ([PRD-000](prd/prd-000-visao-de-produto.md)).

O alvo visual está em [`docs/design/`](design/README.md). O mockup mostra o produto **completo**, não o v1 — a [tabela de fases](design/especificacao-visual.md) diz o que pertence a cada etapa. Nenhuma fase abaixo implementa elemento `[v2]`.

| Fase | Status |
|---|---|
| F0 — Esqueleto | **implementada** |
| F1 — Terminal único | **implementada**, com verificação interativa pendente (ver a fase) |
| F2 — Abas | **implementada**, com verificação interativa pendente (ver a fase) |
| F3 — Grupos | **implementada exceto RF-2.21** (`group.next`/`group.prev`); fase **não fechada** |
| F4 a F6 | não iniciadas |

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

## F1 — Terminal único — implementada

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

### O que ficou pendente da F1

O código está escrito e o CI está verde nas três plataformas — 51 testes automatizados, `clippy -D warnings` limpo. O que **não** foi confirmado é o critério de saída na parte interativa:

- Toda a verificação manual aconteceu no **Windows**. Linux e macOS têm build e teste verdes no CI, nada além disso.
- Teclado real, arraste de mouse, seleção, copiar/colar e mouse dentro do `htop`/`fzf` **não foram exercitados**: a proteção de foco do Windows bloqueia `SetForegroundWindow`/`AppActivate` de processo em segundo plano, e input sintético não é caminho viável. Precisa de uma sessão desktop de verdade.
- `vim`, `htop`, `fzf`, `less` e `man` não foram abertos de ponta a ponta; acentuação ABNT2 e resize com TUI aberta idem.
- **Clipboard no Wayland** segue sem verificação (sem ambiente Linux na fase). O plano B `copypasta` do [ADR-0013](adr/0013-mouse-selecao-e-clipboard.md) continua de pé.
- Confirmado no Windows: terminal spawna, CPU ~0% ocioso, título por OSC chega à janela, fechar a janela mata o processo filho sem órfão, cores ANSI do prompt batem com a paleta.

Duas simplificações conscientes, documentadas no código:

- `PushClip`/`PopClip` existem na API de `porecatu-render` mas ainda não recortam nada — sem consumidor até o overflow da barra de abas, na F2. *(A F2 descobriu que não bastava implementar: clip por índice não sobrevive ao achatamento das primitivas em baldes. Resolvido junto com as camadas no [ADR-0018](adr/0018-composicao-de-frame.md).)*
- `line_height` é aplicado sobre `size` em vez das métricas naturais da face (ascent+descent+lineGap), que exigiria ler `hhea`/`OS/2`. Ajustar quando importar na prática.

---

## F2 — Abas — implementada, com verificação interativa pendente

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

### O que ficou pendente da F2

O código está escrito e o CI está verde nas três plataformas, com testes automatizados cobrindo `porecatu-core` (invariantes de `Workspace`/`Group`), `porecatu-term` (OSC 7, ciclo de vida) e `porecatu-ui` (layout, overflow, indicadores, arraste, os quatro widgets de chrome — tudo o que é função pura, sem GPU nem janela). O que **não** foi confirmado, mesma limitação da F1:

- Toda a verificação aconteceu por build/teste automatizado e por um smoke test não-interativo do `cargo run` (a janela sobe e roda alguns segundos sem `panic`) — a proteção de foco do Windows continua bloqueando input sintético, então nenhum gesto de mouse ou teclado de verdade foi exercitado nesta fase.
- **Arraste de reordenação (RF-1.15), menu de contexto (clique direito), overflow por roda do mouse e o tooltip com atraso de 600ms** — a lógica está coberta por teste unitário isolado (`tab_bar`, `context_menu`, `tooltip`), mas o gesto ponta-a-ponta nunca rodou numa sessão desktop real.
- **Segunda janela** (`Ctrl+Shift+N`/`Ctrl+Shift+Q`, ADR-0015): a cascata de geometria e a superfície `wgpu` compartilhada nunca foram exercitadas com duas janelas de verdade, muito menos em dois monitores com DPI diferente.
- **Diálogo de confirmação de RF-1.6**: a condição (tela alternativa ou reporte de mouse ligado) depende de abrir `vim`/`htop`/`fzf` de verdade dentro do Porecatu, que segue bloqueado pela mesma proteção de foco.

Quatro simplificações conscientes, documentadas no código (`chrome.rs`, `overlay.rs`, `lib.rs`) e registradas na seção 4.4 da [especificação visual](design/especificacao-visual.md):

- Nenhuma sombra nos quatro widgets de chrome — `porecatu-render` não tem primitiva de sombra; e o corpo de aviso/diálogo não quebra linha (`TextRun` é sempre uma linha), truncando com reticências em vez do "três linhas" da espec.
- A cascata da janela nova prende aos limites físicos do monitor, não à área útil (descontada a barra de tarefas/dock) que o parágrafo acima descreve — `winit::monitor::MonitorHandle` não expõe área útil em nenhuma plataforma.
- **A barra não tem hover visual.** O item "hover" da lista acima ficou entregue pela metade: o hover **existe** como hit-test, e é o que dispara o tooltip do RF-1.10, mas nenhum elemento muda de aparência sob o cursor. A espec. pede `filter: brightness(1.18)` na aba e `1.25` na pílula, e `porecatu-render` não tem primitiva de filtro — o realce e a sombra do fantasma de arraste (espec. §2.19) caíram pelo mesmo motivo. É resolvível em CPU dentro de `porecatu-ui`, multiplicando canais e clampando, e não precisa de primitiva nova; ficou para quando houver o primeiro hover que **dependa** dele.
- O auto-scroll durante o arraste acontece por evento de `CursorMoved` dentro da zona de 30 px, não pelo intervalo de `.15s` da espec., que exigiria o temporizador de UI que só a etapa 6 introduziu.

---

## F3 — Grupos — implementada exceto RF-2.21

Implementa [PRD-002](prd/prd-002-grupos-de-abas.md). É o diferencial do produto.

Entregue em seis etapas, uma por PR, na divisão sugerida abaixo: modelo em `core`;
seleção múltipla e gestos da barra; pílula, wrapper tingido e sublinhado por cor;
colapso ponta a ponta; editor de grupo, menu de contexto e popover de destino;
arraste entre grupos, arraste de grupo e a animação de reflui. Depois delas vieram
quatro PRs de correção — bug de ordem em `group_tabs` mais o wiring de
`group.create`, que atravessou a fase sem gesto de UI, e três causas distintas do
mesmo sintoma ("o colapso só anima no primeiro grupo"), a última delas a lentidão
da barra em overflow.

**A fase não está fechada:** o RF-2.21 (`group.next`/`group.prev`) não foi
implementado — ver [o que ficou pendente](#o-que-ficou-pendente-da-f3).

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

O RF-2.17 (*"ativar uma aba de grupo colapsado expande o grupo"*) fica **parcialmente verificável** nesta fase: as duas fontes que o requisito cita são busca (F6) e restauração de sessão (F5), e na F3 não há caminho que ative uma aba oculta. A regra entra no modelo e no teste unitário; o cenário de ponta a ponta espera a F5.

### O que ficou pendente da F3

O código dos itens acima está escrito, o CI está verde nas três plataformas e o
workspace tem **241 testes** (contra 145 ao fim da F2): `porecatu-ui` 108,
`porecatu-core` 54, `porecatu-term` 51, `porecatu-render` 20, `porecatu-pty` 8.
O que falta:

- **RF-2.21 (`group.next`/`group.prev`) não existe** — e é por isso que a fase não
  fecha. O MRU está no modelo desde a etapa 1 (`Group::last_active`, atualizado por
  `Workspace::activate_tab`, [ADR-0020](adr/0020-grupos-explicitos.md) §6), mas não
  há operação em `Workspace` que ande de grupo em grupo nem gesto que a acione. É o
  próximo PR desta fase, não trabalho da F4.
- **O nível de grupo da cadeia do [ADR-0008](adr/0008-teclas-e-roteamento-de-input.md)
  ficou pela metade.** Só `group.create` tem tecla (`Ctrl+Shift+G`, default fixo no
  código). `group.dissolve` (o `Ctrl+Shift+U` do ADR) e `group.toggle_collapse` só
  existem por menu, editor ou clique na pílula; as demais `group.*` são de menu por
  natureza. Fecha com o parser de `[keybindings]`, na F4.
- **Verificação interativa**, mesma limitação da F1 e da F2: build/teste automatizado
  e um smoke test não-interativo do `cargo run`. Nenhum cenário de aceite de PRD-002
  foi exercitado com gesto de verdade — seleção múltipla por `Ctrl`/`Shift`+clique,
  arraste de aba entre grupos, arraste da pílula, duplo clique na pílula, editor de
  grupo por teclado, e a animação de reflui vista com olho humano. Dez grupos numa
  janela e a métrica do PRD-002 idem.
- **Entrada de cor por hexadecimal** (RF-2.10) segue diferida pelo próprio
  [ADR-0023](adr/0023-editor-de-grupo.md): o editor tem os seis swatches e nada mais.
- **Roda do mouse no popover de destino.** A primeira lista rolável do chrome rola
  por realce de teclado e por clique, não por roda — o ADR-0023 pediu lista rolável,
  não disse por qual gesto.
- **`show_new_tab_button = false` não desliga o botão "+" por grupo.** A zona fixa da
  direita respeita a chave (largura zero, sem botão global); o botão ao final de cada
  wrapper é desenhado sempre. Fica para a F4, quando a chave existir de verdade em
  `porecatu-config`.
- **`animations = false` (ADR-0022) não existe.** As durações são constantes
  (`COLLAPSE_REFLOW_DURATION` 150 ms, `GROUP_CREATE_REFLOW_DURATION` 180 ms) com a
  chave TOML citada no comentário, como o resto da aparência até a F4.
- **Sem hover e sem sombra** — as duas dívidas de primitiva da F2 continuam, e a F3
  acrescentou superfícies que a especificação também quer com realce
  (`brightness(1.25)` na pílula). Nada no chrome desenha hover ainda.

Quatro divergências entre desenho e binário, todas registradas na seção 4.4 da
[especificação visual](design/especificacao-visual.md) e com a prosa das seções de
anatomia já atualizada para o comportamento real:

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

- `porecatu-config`: structs `serde`, defaults completos, resolução de caminho, `PORECATU_CONFIG`, `--config`
- Hot reload com `notify`, parse fora da main thread, debounce
- Erro com linha e chave; chave desconhecida como aviso; config inválida preserva a anterior
- Parser de `[keybindings]` contra o [catálogo fechado de ações](reference/acoes.md): ação desconhecida é erro com sugestão do nome mais próximo, binding duplicado cita as duas linhas ([ADR-0008](adr/0008-teclas-e-roteamento-de-input.md))
- Toda a superfície de [porecatu.example.toml](config/porecatu.example.toml) ligada de fato ao desenho, com os valores default vindos da [tabela de tokens](design/especificacao-visual.md)
- Fallback de fonte, temas nomeados com override, zoom por atalho
- Recálculo de grade e resize de todos os PTYs ao mudar métricas de fonte
- **Cobranças que a F2 e a F3 deixaram** (seção 4.4 da [especificação visual](design/especificacao-visual.md)): hover por brilho resolvido em CPU, sombra de popover ou decisão registrada de não tê-la, corpo de aviso em três linhas, `show_new_tab_button` desligando também o botão por grupo, `animations = false` aplicando o reflui instantâneo ([ADR-0022](adr/0022-animacao-de-interface.md)) e `tint_strength` voltando a governar a cápsula do grupo

**Critério de saída:** zero valores de aparência hardcoded (verificação por revisão dirigida); todos os cenários de aceite de PRD-004 e PRD-005 passam; **o binário com a config padrão bate com o mockup** — divergência visível é bug, não configuração ([ADR-0009](adr/0009-referencia-visual-e-reconciliacao.md)); auditoria de rastreabilidade — nenhuma chave do exemplo sem requisito, nenhum requisito sem chave.

---

## F5 — Sessão

Implementa [PRD-003](prd/prd-003-persistencia-de-sessao.md).

- `porecatu-session`: schema versionado, escrita atômica, debounce, save síncrono no exit
- Restauração preguiçosa, geometria de janela, recuperação de monitor ausente
- Captura de OSC 7; fallbacks `/proc` no Linux e `libproc` no macOS
- Detecção de ausência de OSC 7 e convite à integração de shell, com snippets por shell
- Recuperação: arquivo ausente, corrompido, schema antigo, schema mais novo

**Critério de saída:** todos os cenários de aceite de PRD-003 passam; restauração de 20 abas em menos de 1 s; teste de crash durante a gravação preservando a sessão anterior; limitação do Windows sem OSC 7 verificada e documentada como comportamento esperado, não como bug.

---

## F6 — Polimento

O que separa "funciona" de "usável o dia inteiro".

- Busca no scrollback, com as ações `search.*` do [catálogo](reference/acoes.md)
- Hyperlinks OSC 8, com clique ([ADR-0012](adr/0012-identificacao-do-terminal.md))
- **Acessibilidade via `accesskit`** — dívida assumida em [ADR-0001](adr/0001-stack-de-gui.md), não esquecimento. Precisa cobrir também os três widgets do [ADR-0014](adr/0014-superficie-de-aviso-e-dialogo.md): diálogo modal, menu e notificação são justamente os papéis que leitor de tela trata de forma especial
- Menu de contexto do terminal, com `selection.select_all`
- Notificação de desktop opcional na campainha
- Ícone e empacotamento por plataforma
- Documentação de usuário e página de release

**Critério de saída:** leitor de tela consegue navegar a barra de abas; métricas de [PRD-000](prd/prd-000-visao-de-produto.md) medidas e atingidas; artefatos de release para as três plataformas.

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
| Barra de título customizada | [ADR-0009](adr/0009-referencia-visual-e-reconciliacao.md), [PRD-004](prd/prd-004-aparencia-do-chrome.md) | sim, `[v2]` |
| Multiplexação remota | [PRD-000](prd/prd-000-visao-de-produto.md) | não |
| Plugins e config programável | [ADR-0003](adr/0003-formato-de-configuracao.md) | não |
| Persistir scrollback | [PRD-003](prd/prd-003-persistencia-de-sessao.md) | não |
| Mover aba entre janelas | [PRD-001](prd/prd-001-abas.md) | não |
| Temas como arquivo importável | [PRD-004](prd/prd-004-aparencia-do-chrome.md), [PRD-005](prd/prd-005-aparencia-do-terminal.md) | não |
| Tema claro/escuro seguindo o sistema | [PRD-004](prd/prd-004-aparencia-do-chrome.md) | não |
| Agrupamento automático por projeto | [PRD-002](prd/prd-002-grupos-de-abas.md) | não |
| Protocolos de imagem (sixel) | [ADR-0002](adr/0002-motor-vte.md) | não |
