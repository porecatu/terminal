# Arquitetura

Documento técnico central. Os ADRs justificam *por que* cada peça foi escolhida; este documento descreve *como* elas se encaixam.

As seções 2, 3, 4, 5, 6 e 7 estão implementadas (F0 a F4 do [roadmap](roadmap.md)) — a seção 5 na forma final do [ADR-0018](adr/0018-composicao-de-frame.md), com camadas e recorte de verdade; a seção 6 com `porecatu-config` e o hot reload das três classes do [ADR-0030](adr/0030-escopo-do-hot-reload.md); e a seção 7 com as funções puras da barra de abas. Da seção 1, só `session` segue projeto — restauração de sessão é a F5. Onde o código divergiu do escrito, há um bloco **Na implementação** dizendo o quê e por quê.

---

## 1. Camadas

```
                    +----------------------------+
                    |   porecatu (bin)           |
                    |   event loop winit         |
                    +-------------+--------------+
                                  |
            +---------------------+---------------------+
            |                     |                     |
   +--------v--------+  +---------v--------+  +---------v--------+
   |  porecatu-ui    |  | porecatu-session |  | porecatu-config  |
   |  layout, hit    |  | save / restore   |  | TOML, defaults   |
   |  test, input    |  |                  |  | hot reload       |
   +---+---------+---+  +---------+--------+  +---------+--------+
       |         |                |                     |
+------v------+  |      +---------v---------------------v--------+
| porecatu-   |  |      |            porecatu-core               |
| render      |  |      |  Workspace, Group, Tab, IDs, geometria |
| wgpu, quads |  |      +----------------------------------------+
| glyphon     |  |
+-------------+  |
                 |
        +--------v--------+      +------------------+
        | porecatu-term   |----->| porecatu-pty     |
        | alacritty_term  |      | portable-pty     |
        | snapshot grid   |      | spawn, resize    |
        +-----------------+      +------------------+
```

Fora dos crates, três coisas moram na raiz e valem registro, porque nenhuma delas cabia no diagrama: `src/main.rs` carrega `#![windows_subsystem = "windows"]` (sem isso o binário fica no subsistema `console` e o Windows abre um terminal ao lado da janela); `build.rs` embute o `.ico` de `assets/icon/` como recurso PE no Windows, via `winres`, sob `cfg(windows)`; e `porecatu-ui/src/app_icon.rs` decodifica um PNG embutido em runtime (crate `png`) para dar ícone a toda janela criada — os dois caminhos existem porque o ícone da barra de tarefas e o ícone da janela não vêm do mesmo lugar no Windows.

O grafo de dependências permitido está tabelado em [CLAUDE.md](../CLAUDE.md). Duas regras merecem destaque:

**`porecatu-render` não conhece o domínio.** Ele expõe um punhado de primitivas — retângulo, retângulo arredondado, run de texto, clip rect — e nada mais. Não sabe o que é uma aba. Isso mantém o renderer testável e substituível, e força a aparência configurável a viver onde ela pertence: em `config` (o que o usuário pediu) + `ui` (como isso vira geometria).

O [ADR-0018](adr/0018-composicao-de-frame.md) acrescenta duas coisas a esse crate sem furar a regra: **camadas** (uma sequência ordenada de listas de primitivas, para que popover possa cobrir texto) e um **medidor de texto sem GPU**, que mede string, face e tamanho e continua não sabendo o que é uma aba. É o medidor que torna o layout da barra a função pura da seção 7.

**`porecatu-core` não depende de nada.** É o modelo de domínio puro: `Workspace`, `Group`, `Tab`, IDs, tipos geométricos. Serializável, testável sem GPU e sem PTY. É por isso que `porecatu-session` consegue ser um crate trivial: ele serializa `core` e mais nada.

> **Na implementação.** Na F1 o crate tinha só `TabId`, para que o `Wakeup { window, tab }` do [ADR-0015](adr/0015-multiplas-janelas.md) nascesse com o formato certo. `Workspace`, `Group` e `Tab` entraram na F2, com `serde` derivado já ali — o round-trip que o [ADR-0006](adr/0006-modelo-de-abas-e-grupos.md) lista como invariante é testável mesmo com `porecatu-session` vazio, e há teste para ele. `Tab` carrega o estado `Exited` do [ADR-0017](adr/0017-ciclo-de-vida-da-aba.md), o título com precedência, o `cwd` de OSC 7 e os indicadores de atividade e campainha.
>
> Na F2 existe **um grupo só**, o implícito, e as operações de grupo da tabela do ADR-0006 (`group_tabs`, `ungroup`, `rename_group`, `set_group_color`, `collapse_group`) ficaram de fora de propósito: nada nesta fase as chamava. O [ADR-0020](adr/0020-grupos-explicitos.md) revisa duas coisas que a F3 precisa e que o ADR-0006 não resolvia — o grupo implícito deixa de ser único, e colapso deixa de ser "só desenho".
>
> **Na F3 o modelo do ADR-0020 está em pé:** grupos explícitos com nome, cor e colapso, N runs implícitos mantidos por `normalize_groups`, `navigable_order()` ao lado de `visual_order()`, MRU por grupo e a escada de foco de quatro níveis. As operações da tabela existem todas, mais quatro de movimentação entre grupos e de grupo (`move_tab_to_group`, `move_tab_to_group_at`, `move_tab_to_new_run`, `move_group`). Detalhes na seção 7. O que falta do PRD-002 no core é o RF-2.21: o MRU está gravado, mas não há operação que ande de grupo em grupo.

---

## 2. Modelo de threading

Ver [ADR-0007](adr/0007-modelo-de-threading.md) para a decisão e as alternativas descartadas.

### Main thread

Roda o event loop do `winit` e faz toda a submissão de frame do `wgpu`. Isso não é preferência: macOS exige interação com janela na main thread, e Windows tem restrições equivalentes.

A main thread **nunca** faz I/O bloqueante. Nem leitura de PTY, nem leitura de arquivo de config, nem carregamento de fonte de disco em resposta a input.

### Uma thread de leitura por terminal

Cada terminal aberto tem uma thread dedicada:

```rust
loop {
    let n = pty_reader.read(&mut buf)?;      // bloqueia
    let mut term = term.lock();              // Mutex<Term>
    parser.advance(&mut *term, &buf[..n]);   // aplica no grid
    drop(term);
    proxy.send_event(Wakeup { window, tab }); // acorda a UI  [ADR-0015]
}
```

Bloquear aqui é correto e barato — a thread existe justamente para isso. O `Mutex` é segurado só durante o `advance`, nunca durante o render.

### Escrita

Input do teclado vira bytes no `porecatu-ui` e é enviado por `mpsc::Sender` para o handle de escrita do PTY. A escrita não passa pela thread de leitura.

> **Na implementação.** São **três** threads por terminal, não duas: leitura, escrita e observação do processo (`try_wait` em intervalo curto). A de observação é a dona do `PtyHandle`, e por isso é ela quem aplica o resize do lado do PTY — o lado do motor é síncrono. Quem spawna as três é `porecatu-term::Terminal`; `porecatu-ui` nunca vê `PtyHandle` nem thread nenhuma, só `spawn`, `write`, `snapshot_into`, `try_recv_event`, `scroll`, `modes`, `resize` e as operações de seleção. A notificação de sujeira sai por um closure genérico (`on_wakeup`), para o crate não precisar conhecer `winit`; quem fecha esse closure sobre `EventLoopProxy` e `Wakeup` é a `ui`.
>
> **Encerramento de uma aba.** O [ADR-0017](adr/0017-ciclo-de-vida-da-aba.md) decidiu o que o RF-1.2 chamava de "aguardar EOF": o ConPTY não emite EOF, então a espera é pela confirmação de morte da thread de observação, **fora da main thread**. A aba sai da barra imediatamente. Sem isso, fechar uma janela com 50 abas custaria 50 × `SHUTDOWN_TIMEOUT` na main thread, contra a métrica de 50 abas do PRD-001.
>
> **Implementado na F2:** `Terminal::close` sinaliza o processo e devolve na hora; `Terminal::shutdown` virou `close().wait()`, para quem precisa da confirmação. Fechar a janela sinaliza todas as abas primeiro e espera depois, não uma a uma. Há teste de integração cobrindo que `close` devolve antes do `SHUTDOWN_TIMEOUT` mesmo com processo vivo.
>
> **Encerramento (Windows).** `Terminal::shutdown` não dá `join` em nenhuma das três threads e **não fecha o pseudo-console**. `ClosePseudoConsole` bloqueia até o pipe de leitura clonado ser liberado, e a thread de leitura está parada num `read()` síncrono nele — as duas esperam uma pela outra e o app trava. O que se faz é matar o processo e `mem::forget` no handle: o SO reclama tudo quando o Porecatu sai. A confirmação de "processo morto" vem de um canal dedicado que a thread de observação sinaliza, com timeout de segurança. Dar `join` na thread de leitura violaria a regra de que a main thread nunca bloqueia, justamente no fechamento da janela.
>
> **Na implementação ([ADR-0033](adr/0033-job-object-encerramento-de-processo.md)).** O parágrafo acima descreve o processo **raiz** — o shell — e continua verdadeiro: o pseudo-console nunca fecha. Mas `TerminateProcess` no shell não mata o que ele tenha spawnado (ex. um servidor de longa duração em primeiro plano), que sobrevivia à aba fechada. Desde a F4, o shell é atribuído a um Job Object (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, `crates/porecatu-pty/src/job.rs`) no spawn; fechar o handle desse Job — não o do pseudo-console, um objeto totalmente separado — mata a árvore inteira de uma vez, sem varrer processo nenhum. O handle do Job é a peça que decide se a árvore morre (fechamento pedido pelo usuário) ou sobrevive (saída natural do shell, preservando processo deliberadamente destacado) — ver a tabela de RAII do ADR-0033 seção 2.

### Render damage-driven

Este é o ponto onde emuladores ingênuos queimam bateria. `cargo build` cospe centenas de linhas em milissegundos; se cada `Wakeup` disparar um frame, o app renderiza centenas de frames que ninguém vê.

A regra:

1. `Wakeup { window, tab }` marca a aba como suja e nada mais. O par é necessário porque `TabId` é gerado por workspace, e workspace é por janela — só o `TabId` não identifica a aba ([ADR-0015](adr/0015-multiplas-janelas.md)).
2. Se a aba suja não é a visível, para por aí — só o grid é atualizado, sem render.
3. Se é a visível, agenda um `request_redraw()` **no máximo uma vez por intervalo de frame** (limitado pela taxa de atualização do monitor).
4. Terminal ocioso = zero frames. Não há loop de render contínuo.

O mesmo vale para o chrome: mudança de hover, foco ou config marca a barra de abas como suja.

> **Na implementação (F3, etapa 6 — o relógio de animação).** A regra 4 ganhou a única exceção que o [ADR-0022](adr/0022-animacao-de-interface.md) autoriza: **animação em curso *é* a sujeira.** `AnimationClock` (`porecatu-ui/animation.rs`) vive em `WindowState` ao lado de `warnings`/`hover`/`drag`, recebe `Instant` de fora e nunca chama `Instant::now()`, e contribui para o `next_deadline()` da janela com o próximo intervalo de frame (16 ms) enquanto há reflui pendente. Quando a última termina, o deadline desaparece da conta e `schedule_next_wake` volta a `Wait` — terminal ocioso continua em zero frames. Não há thread de timer, como não havia para tooltip e aviso: é o mesmo `ControlFlow::WaitUntil`.
>
> **Na implementação.** O ponto 3 não precisou de bookkeeping próprio de sujeira na F1: `request_redraw` do `winit` já coalesce chamadas repetidas antes do próximo `RedrawRequested` num só evento (no Win32 é uma flag booleana, não fila). N wakeups de saída rápida viram um frame. O ponto 2 já existe com a forma final — `Wakeup { window, tab }` é comparado com a aba visível antes de qualquer redraw, mesmo havendo hoje uma janela e uma aba só.

### Propriedade dos dados

| Dado | Dono | Compartilhamento |
|---|---|---|
| `Term` (grid, scrollback) | thread de leitura + render | `Arc<Mutex<Term>>` |
| `Workspace` (abas, grupos) | main thread | exclusivo, sem lock — um por janela |
| `Config` | main thread | `Arc<Config>`, trocado inteiro no reload |
| Handle de escrita do PTY | `mpsc` | clonável |

`Workspace` só é tocado pela main thread, então não precisa de lock. `Config` é imutável e trocado por inteiro no hot reload — nenhum lock, só uma troca de `Arc`.

> **Na implementação (F2, etapa 6).** Há **um `Workspace` por janela**, dentro de `WindowState`, e as janelas vivem num `HashMap<WindowId, WindowState>` em `App` ([ADR-0015](adr/0015-multiplas-janelas.md)). Junto com o workspace, migrou para `WindowState` tudo o que varia por janela: abas, rename em curso, arraste, deslocamento de rolagem da barra, hover, avisos, diálogo e menu. O que não varia — `GpuContext`, `cell_metrics` (em pixels lógicos, DPI-independente) e `startup_directory` — continua em `App`, um por processo.

---

## 3. Fluxo de dados

### Do teclado até o PTY

```
winit WindowEvent::KeyboardInput
  |
  +- porecatu-ui: resolve keybind (app -> grupo -> terminal)   [ADR-0008]
       |
       +- é ação da app?  -> muta Workspace, marca chrome sujo
       +- não é           -> codifica em bytes (modo de cursor,
                             modificadores, bracketed paste)
                             -> mpsc -> PTY write
```

> **Na implementação (F2, etapa 4; parser de `[keybindings]` na F4, etapa 5).** O passo 1 da cadeia do [ADR-0008](adr/0008-teclas-e-roteamento-de-input.md) existe: `porecatu-ui/input.rs` resolve **modo de captura** antes de qualquer keybind — rename de aba, diálogo aberto e menu de contexto consomem a tecla e nada desce ao terminal —, depois a tabela de ações da app, depois o terminal. Desde a F4 etapa 5, `handle_tab_action_key` consulta um `HashMap<Chord, Action>` resolvido em três níveis (embutido da plataforma → `[keybindings]` comum → `[keybindings.<plataforma>]`) em vez do `match` fixo que existia antes — `Chord`/a resolução vivem em `porecatu-ui` (`keymap.rs`), `Action` em `porecatu-core` ([ADR-0029](adr/0029-enum-de-acao-e-gramatica-de-tecla.md)). `scrollback.*`/`clipboard.*` continuam num caminho hardcoded em `input.rs`, de propósito: não entraram no escopo da etapa 5. `Ime::Commit` vai direto ao terminal sem consultar keybind — tecla morta do ABNT2 e composição de CJK dependem disso.

### Do PTY até o pixel

```
PTY read (thread dedicada)
  |
  +- parser VT -> alacritty_terminal::Term  (Mutex)
       |
       +- EventLoopProxy::send_event(Wakeup)
            |
            +- main thread: marca sujo, agenda redraw (coalescido)
                 |
                 +- RedrawRequested:
                      +- trava o Term, tira snapshot das células visíveis
                      +- porecatu-ui: Workspace + Config -> primitivas
                      +- porecatu-render: primitivas -> wgpu render pass
```

O snapshot existe para que o `Mutex` seja liberado antes do trabalho de GPU. Renderizar segurando o lock faria a thread de leitura esperar a GPU.

---

## 4. Fronteira de `porecatu-term`

É a fronteira mais crítica do projeto: separa o motor VT do resto, contém o
`alacritty_terminal` ([ADR-0002](adr/0002-motor-vte.md)) e é atravessada a cada frame.
Três regras a governam, e as três derivam da mesma restrição — **`porecatu-term` não
conhece `Config` nem GUI**.

### 4.1 O snapshot de grade

Tipo próprio de `porecatu-term`. Nenhum tipo do `alacritty_terminal` aparece na
assinatura; trocar o motor não vaza para `ui`.

```
GridSnapshot
  cols, rows            dimensões da viewport
  cells                 rows*cols, row-major, SÓ a área visível
  clusters              arena de texto do frame (String reusada)
  cursor                posição, forma, visível
  scroll_offset         linhas acima do fundo do scrollback
  selection             span resolvido, para pintura
  modes                 alt screen, mouse, bracketed paste

Cell
  text                  char único, ou fatia em `clusters`
  fg, bg                TermColor — NÃO resolvido em RGBA
  flags                 bold, italic, underline, inverse, wide, wide_spacer, wrapline
```

Quatro decisões que a forma acima carrega:

**As cores não são resolvidas aqui.** `Cell.fg` e `Cell.bg` são um `TermColor`
— `Default`, `Indexed(u8)` ou `Rgb`. Quem traduz para RGBA é `porecatu-ui`, que tem
`Config` e portanto a paleta, o tema e os overrides. É o que permite ao `term` ignorar
a existência de config e ao renderer receber só cor concreta (seção 5).

**O buffer é reusado entre frames.** Nem `cells` nem `clusters` alocam no caminho
quente ([ADR-0007](adr/0007-modelo-de-threading.md)). O snapshot é preenchido sob o
lock e o lock cai antes do trabalho de GPU.

**Grafema composto não aloca por célula.** Célula com um só `char` guarda o `char`;
emoji com ZWJ ou base mais combinantes guarda uma fatia na arena `clusters`, que é a
mesma `String` reusada a cada frame.

**Caractere de largura dupla ocupa duas células.** A primeira leva o texto e a flag
`wide`; a segunda vem vazia com `wide_spacer`. Sem isso, a coluna do CJK desalinha e
o hit-testing do mouse erra.

### 4.2 Quem lê a config que o terminal precisa

`porecatu-term` **não importa `porecatu-config`**. Valores como `scrollback.lines`,
`selection.word_separators` e `terminal.clipboard.*` chegam num struct simples de
parâmetros, do próprio `porecatu-term`, montado por `porecatu-ui` a partir de
`Config`. Hot reload é o mesmo caminho: `ui` remonta os parâmetros e reaplica.

A tabela de dependências do [CLAUDE.md](../CLAUDE.md) fica valendo como está — `term`
depende só de `pty`. Sem essa regra, a primeira chave de config nova arrasta o crate
inteiro para dentro do grafo da GUI.

### 4.3 O que sai do terminal além de pixels

Sequências de escape que **não** são desenho viram eventos, consumidos por
`porecatu-ui`. O `term` nunca age sobre elas:

| Evento | Origem | Quem decide o que fazer |
|---|---|---|
| `Title` | OSC 0 / OSC 2 | `ui`, respeitando a precedência do RF-1.7 — customizado → OSC → shell, conforme o [ADR-0017](adr/0017-ciclo-de-vida-da-aba.md) |
| `Cwd` | OSC 7 | `ui` → `tab.new`/`window.new` (F2, ADR-0017) e → `session` ([ADR-0005](adr/0005-persistencia-de-sessao.md), F5) |
| `ClipboardWrite` | OSC 52 | `ui`, sujeito a `osc52_write` e ao teto de tamanho |
| `ClipboardRead` | OSC 52 | `ui`, **negado por default** ([ADR-0013](adr/0013-mouse-selecao-e-clipboard.md)) |
| `ColorSet` / `ColorQuery` | OSC 4 / 10 / 11 | `ui`, com escopo de sessão ([ADR-0012](adr/0012-identificacao-do-terminal.md)) |
| `Bell` | BEL | `ui` (RF-1.21) |
| `Exit` | `try_wait` da thread de observação, **não** EOF do PTY (ver nota) | `ui` (RF-1.3) |

O clipboard é o caso que mais tenta o atalho errado: o OSC 52 chega **do PTY**, dentro
do `term`, mas o `arboard` vive do lado da GUI. O caminho é `term` → evento → `ui` →
`arboard`, sempre. Chamar o clipboard de dentro do `term` furaria o grafo e enterraria
a política de segurança do ADR-0013 no lugar errado.

Os flags de modo (mouse, bracketed paste, tela alternativa) são exceção por serem
consultados no caminho de **input**, não de render: `porecatu-term` expõe um acessor
barato para eles, além de estampá-los no snapshot.

> **Na implementação.** Três desvios da tabela acima:
>
> - **`Cwd` não é capturado na F1.** `alacritty_terminal` não trata OSC 7 (não é xterm-padrão) e capturá-lo exige um `Handler` próprio interceptando essa sequência antes de delegar o resto ao `Term`. OSC não reconhecido é descartado pelo parser, então nada vira lixo na tela nesse meio-tempo, que é o que o [ADR-0012](adr/0012-identificacao-do-terminal.md) exige.
>
>   **Revisto na F2.** A avaliação de que o único consumidor era `porecatu-session` estava errada: o RF-1.1 e o `window.new` do [ADR-0015](adr/0015-multiplas-janelas.md) herdam o `cwd` da aba ativa, e os dois são da F2. O [ADR-0017](adr/0017-ciclo-de-vida-da-aba.md) antecipa a captura para lá, e a F5 recebe um evento que já existe. Sem OSC 7, o fallback é `startup_directory`, comportamento esperado no Windows pelo [ADR-0005](adr/0005-persistencia-de-sessao.md).
>
>   **Mecanismo real, diferente do previsto no ADR-0017.** Não é um `Handler` que intercepta OSC 7 e delega o resto ao `Term`: o `osc_dispatch` do `vte` (que `alacritty_terminal::vte` reexporta) descarta OSC 7 antes de chamar qualquer método de `Handler` — não existe gancho aí para interceptar. A captura roda como um segundo `vte::Parser`/`Perform`, independente e sem efeito colateral no motor, sobre os mesmos bytes que `TermEngine::advance` já processa (`porecatu-term/src/osc7.rs`). O resultado observável é idêntico ao que o ADR decidiu — `Term` intocado, `TermEvent::Cwd` capturado —, só o mecanismo mudou.
> - **`ColorSet` não existe**, só `ColorQuery`. OSC 4/10/11 de consulta viram evento com um responder que formata a resposta; a variante de escrita entra quando houver tema para escrever em cima (F4).
> - **As respostas automáticas do motor (DSR, DA, CPR) não passam por evento.** Vão direto ao canal de escrita do PTY, de dentro do `TermEngine`. Roteá-las como `TermEvent` obrigaria todo consumidor a filtrar e repassar, e esquecer um write pendente é o programa ficar parado esperando resposta que nunca chega.
>
> E `Exit` não vem de EOF do PTY: vem do `try_wait` da thread de observação, injetado no mesmo canal. No Windows o pipe do ConPTY não emite EOF só porque o processo hospedado saiu — só quando o pseudo-console é fechado, o que (ver seção 2) não acontece.
>
> O teto de tamanho do payload de escrita OSC 52 é aplicado no `term`, já que o motor não tem essa noção; a negação de leitura, não — `alacritty_terminal` já não emite o evento quando só a escrita está habilitada, então o default do ADR-0013 cai direto no mapeamento.

---

## 5. Fronteira de render

> **Depois da F3.** Duas coisas entraram na pintura da grade e valem nota aqui, porque são invariantes de layout e não escolha de desenho. (1) A grade é desenhada dentro de um **quadro arredondado** (`paint::terminal_box_rect`), colado na barra de abas em cima e recuado nos outros três lados; a área útil do terminal é a de dentro do quadro menos o padding interno, e é dela que saem colunas e linhas. (2) A célula e a origem de cada `TextRun` são **arredondadas ao pixel físico** (`snap_cell_metrics_to_pixel_grid`), o que mata a costura de 1px entre glyphs — mas esse valor arredondado **não** serve para decidir se um caractere pode viajar num run compartilhado: essa decisão é em **em**, contra o avanço natural do `'M'` da face mono. Comparar um contra o outro erra por até meio pixel, reprova toda célula e re-shapa a grade inteira por frame.

`porecatu-render` recebe uma **sequência de camadas** por frame ([ADR-0018](adr/0018-composicao-de-frame.md)), cada uma com sua lista de primitivas:

- `Quad { rect, color }`
- `RoundedQuad { rect, radius, color, border }`
- `TextRun { origin, text, font, size, color }`
- `PushClip(rect)` / `PopClip`

Duas passadas de pipeline **por camada**: uma de geometria (quads instanciados, cantos arredondados via SDF no fragment shader) e uma de texto (`glyphon`, com atlas de glyphs em cache entre frames). Dentro da camada vale a ordem geometria-antes-de-texto; **entre** camadas vale a ordem da sequência, e é isso que permite a um popover cobrir o texto do terminal. As camadas são cinco e enumeradas — grade, chrome, aviso, popover, modal —, não profundidade arbitrária. O `TextAtlas` é único; o que se multiplica por camada é o `TextRenderer`.

`PushClip`/`PopClip` recortam dentro da camada: `set_scissor_rect` para os quads, `TextBounds` por área para o texto, com a pilha de clip intersectando no aninhamento.

A grade do terminal é um caso particular: fundo de célula vira quads em batch, glyphs viram um `TextRun` por run de mesmo estilo — não um por caractere.

**Nenhuma cor, raio ou dimensão é hardcoded no renderer.** Tudo vem de `Config` via `ui`. É isso que torna o requisito de customização (PRD-004, PRD-005) uma questão de configuração e não de recompilação.

> **Na implementação (F2, etapa 2 — camadas, recorte e medidor).** As camadas e o recorte do ADR-0018 estão implementados: `porecatu-render` recebe um `Frame` com cinco `Vec<Primitive>` (uma por [`Layer`](adr/0018-composicao-de-frame.md)) em vez de uma lista só. Dentro de cada camada, uma função pura (`resolve_layer`, testada sem GPU) percorre o stream mantendo a pilha de clip e agrupa quads/arredondados contíguos de mesmo clip em batches — cada batch vira um `draw` com seu próprio `set_scissor_rect`, o que substitui o achatamento em três baldes da F1. Texto não precisa de batch: cada `TextRun` carrega seu próprio `TextBounds`, granular por natureza no `glyphon`.
>
> `Renderer` virou dois tipos, como o ADR pede: [`GpuContext`](adr/0018-composicao-de-frame.md) (`Instance`/`Adapter`/`Device`/`Queue`/`TextAtlas`/`Cache`/`SwashCache`/pipeline de quads/[`TextMeasurer`](adr/0018-composicao-de-frame.md), um por processo) e `WindowSurface` (surface/`SurfaceConfiguration`/`Viewport`/os cinco `TextRenderer`/escala, um por janela). `GpuContext::new` cria o primeiro par junto (o `Adapter` precisa da surface da primeira janela para ser escolhido); `create_window_surface` cria as seguintes reusando `Adapter`/`Device`/atlas, e devolve `Result` — nunca `panic` — se a surface da janela nova não for compatível.
>
> `TextMeasurer` mede string, face e tamanho sem `Device` nem `Queue` (`measure_width`, `measure_mono_cell`, `truncate` para o RF-1.10), dono do único `FontSystem` do processo; o pipeline de texto o recebe emprestado no `prepare`, nunca com uma cópia própria. A conversão de pixels lógicos (contrato de `Rect`/`TextRun`, o que `porecatu-ui` monta) para físicos acontece só dentro de `WindowSurface` — um ponto só, como o ADR exige — e isso fechou uma lacuna que a F1 tinha sem registrar: a métrica de fonte nunca era multiplicada pela escala da janela, então texto e grade saíam do tamanho físico errado em monitor HiDPI.
>
> Quatro notas que seguem valendo:
>
> - O `RoundedQuad` tem `radius` escalar, não `radii` por canto. Todo elemento `[v1]` usa raio uniforme; per-canto é aditivo e entra se a barra de título customizada `[v2]` pedir (ADR-0018).
> - As faces embutidas são carregadas por `include_bytes!` num `fontdb::Database` **antes** das do sistema (`load_system_fonts`). A ordem é o que entrega as duas metades do [ADR-0016](adr/0016-fontes-embutidas.md) ao mesmo tempo: o `fontdb` resolve empate de família pela ordem de registro, então as faces do design vencem para "Iosevka Fixed" ([ADR-0025](adr/0025-iosevka-no-lugar-da-ibm-plex.md)) — terminal e chrome, uma família só desde o [ADR-0026](adr/0026-chrome-unificado-em-iosevka-fixed.md) —, e a cadeia de fallback do RF-5.2 — que o ADR manda vir do sistema — existe de fato para o que nenhuma face embutida cobre. Emoji e CJK saem do sistema; braille, powerline, box drawing e formas geométricas a Iosevka cobre, então não dependem dele.
> - A surface precisa de `remove_srgb_suffix()` no formato depois do `get_default_config`. O default é um formato `*Srgb`, e a GPU reaplicaria a curva sobre cores que já vêm em espaço sRGB (saem de hex do design): dupla conversão, fundo quase-preto virando cinza-azulado.
> - A estimativa de largura de buffer por contagem de bytes (`text.len()`) saiu do caminho de desenho e da medição — cortava errado com acentuação. `measure_width`/`prepare_layer` deixam o `glyphon` shapar sem limite de largura (`set_size(None, None)`), já que cada `TextRun` é um trecho de uma linha só, não algo que deva quebrar.
>
> Enquanto `porecatu-config` não existe, quem resolve `TermColor` em cor concreta é `porecatu-ui/palette.rs`, com a paleta ANSI do [porecatu.example.toml](config/porecatu.example.toml) e a fórmula xterm padrão para o range 256 (cubo 6×6×6 + rampa de cinza). `paint.rs` agrupa células contíguas de mesmo estilo num `TextRun` só.

---

## 6. Configuração e hot reload

Ver [ADR-0003](adr/0003-formato-de-configuracao.md).

```
notify (watcher) -> arquivo mudou
  |
  +- parse em thread separada
       +- erro -> mantém Config anterior, notifica o usuário na UI
       +- ok   -> envia novo Arc<Config> para a main thread
                  -> troca + recalcula métricas de fonte + redraw total
```

Config inválida **nunca** derruba o app nem limpa a tela. O usuário está editando o arquivo enquanto o app roda; estados intermediários inválidos são normais.

> **Escopo, decidido no [ADR-0030](adr/0030-escopo-do-hot-reload.md).** "Recalcula métricas + redraw" acima é só uma das três classes de chave. **A**: cor, fonte de chrome, dimensão, geometria de widget, `animations`, tema — troca o `Arc`, recalcula o layout da barra (que é função pura desde a F2) e redesenha. **B**: métrica de fonte do terminal e as alturas que mudam a área útil — recalcula a célula, deriva colunas e linhas e **redimensiona todos os PTYs**, um resize por recarga, coalescido pelo debounce. **C**: `decorations`, `tab_bar_position`, `opacity` de janela, `[shell]`, `[session]` — não aplica a quente e **avisa** qual é o escopo real, porque ignorar em silêncio produz o relato "mudei e não aconteceu nada", que é indistinguível de bug.
>
> A classe fica escrita ao lado da chave no arquivo de exemplo. O evento de `notify` chega por `EventLoopProxy`, como o `Wakeup` de PTY: uma recarga é um evento e um frame, e o loop volta a dormir ([ADR-0007](adr/0007-modelo-de-threading.md)). O `Arc<Config>` é **do processo**, não da janela — uma recarga redesenha todas as janelas, e o recálculo da classe B roda por janela, porque a métrica é a mesma e as dimensões não.
>
> Duas decisões vizinhas: o enum `Action` que o parser de `[keybindings]` produz nasce em `porecatu-core`, porque `config` não pode depender de `ui` ([ADR-0029](adr/0029-enum-de-acao-e-gramatica-de-tecla.md)); e um tema nomeado só declara **cor**, nunca fonte ou dimensão, o que mantém `theme.cycle` na classe A ([ADR-0031](adr/0031-temas-nomeados.md)).

---

## 7. Estratégia de teste

| Crate | Como testar |
|---|---|
| `porecatu-core` | testes unitários puros: mover aba entre grupos, agrupar, desagrupar, invariantes de ordenação |
| `porecatu-config` | round-trip TOML, defaults completos, rejeição de config inválida com erro legível |
| `porecatu-session` | round-trip de serialização, migração de `schema_version`, recuperação de arquivo corrompido |
| `porecatu-term` | golden files: sequência de escape na entrada, dump de grid esperado na saída |
| `porecatu-pty` | teste de integração por plataforma: spawn de `echo`, resize, encerramento |
| `porecatu-render` | geometria e resolução de camada são puras e testadas sem GPU; pipeline e pintura, verificação manual e screenshot |
| `porecatu-ui` | hit-testing e layout são funções puras sobre geometria, testáveis sem janela |

O layout da barra de abas é deliberadamente uma função pura `(Workspace, Config, largura) -> Vec<TabRect>`. Isso permite testar overflow, colapso de grupo e truncamento de título sem abrir uma janela.

Isso só é cumprível porque o medidor de texto do [ADR-0018](adr/0018-composicao-de-frame.md) se constrói **sem `Device` nem `Queue`**: a função recebe o medidor emprestado, e o teste constrói um sem tocar em `wgpu`. Na F1 não era — o `FontSystem` vivia dentro do pipeline de texto, que exige GPU, e não havia como medir largura de string proporcional.

> **Na implementação (F2, etapa 3 — layout puro e hit-testing).** A assinatura real, em `porecatu-ui/tab_bar.rs`, é `(&Workspace, &TabBarStyle, &mut TextMeasurer) -> TabBarLayout` — `Config` ainda não existe (`TabBarStyle` é constantes com a chave TOML de origem no comentário, como `palette.rs` já fazia na F1) e o resultado é agrupado por `GroupWrapperRect` (um por grupo não vazio, com seus `TabRect`), não uma lista achatada — a estrutura em árvore do ADR-0006 (`Workspace -> Group -> Tab`) se preserva no layout. O parâmetro `largura` ainda não existe: esta etapa produz a geometria natural da trilha (rótulo truncado só no teto fixo de 180px, ADR-0018/espec. §2.5), sem clamping ao espaço disponível; encolhimento abaixo disso e rolagem entram na Etapa 5, que é quando `largura` passa a fazer parte da assinatura. `hit_test(&TabBarLayout, ponto) -> Option<TabBarHit>` é a segunda função pura que a seção promete — resolve corpo da aba, botão de fechar (com a folga de 2px da espec. §2.2) e botão de nova aba, com a fronteira entre abas vizinhas partindo o `gap` ao meio.

> **Na implementação (F2, etapa 4 — ciclo de vida, OSC 7, título e rename).** `App` deixa de modelar uma `Terminal` só e passa a ter `Workspace` + `HashMap<TabId, TabRuntime>` (`Terminal` + snapshot por aba) — a fronteira entre domínio puro e I/O que a seção 4 já descrevia, agora com mais de um lado ocupado. `rename.rs` é o modo de captura do ADR-0008 passo 1 (RF-1.8/RF-1.9): `RenameState` puro, sem `winit`, testável isolado — o buffer não tem posição de cursor no meio da string (sempre no fim), simplificação que `chrome.rs` já assume ao desenhar o caret. `chrome.rs` (tradução de `TabBarLayout` + estado efêmero para `Primitive`) segue a mesma linha de `porecatu-render`: sem teste automatizado, verificação manual — é pintura, não geometria. `porecatu-term` ganha `osc7.rs` (captura de OSC 7, golden-style como o resto do crate) e dois testes de integração novos: `Terminal::close` devolve antes de `SHUTDOWN_TIMEOUT` mesmo com processo vivo (a versão não-bloqueante de `shutdown`, ADR-0017 item 4), e `inject_note` aparece no snapshot como se fosse saída do programa (RF-1.3).
>
> **Na implementação (F2, etapa 5 — overflow, arraste e indicadores).** `largura` entra na assinatura prometida na nota da etapa 3: `tab_bar::fit_width(&Workspace, &TabBarStyle, largura_disponível, &mut TextMeasurer) -> TabBarLayout` encolhe o teto do rótulo por busca binária (o `content_width` de `layout` é não decrescente no teto, então converge) até o piso de `TabBarStyle::min_width` (espec. §2.18); `overflow_state` resolve o deslocamento de rolagem (saturado) e a contagem de abas ocultas de cada lado a partir desse layout — as duas continuam funções puras, testáveis sem GPU. O indicador de atividade/campainha (espec. §2.17) entra em `layout` mesmo, como campo `TabRect::indicator`: consome parte do teto do rótulo em vez de somar cromo, e o teste `indicator_does_not_widen_a_truncated_tab` verifica isso com tolerância (o truncamento decide por caractere medido, não há garantia de igualdade exata ao pixel entre dois teto diferentes). O arraste de reordenação (espec. §2.19) não mexe no `Workspace` real durante o gesto: `lib.rs` clona o `Workspace` a cada redraw, aplica `move_tab` no clone na posição que `tab_bar::drag_target_index` calcula a partir da posição do fantasma, e só aplica no `Workspace` de verdade ao soltar dentro da barra (`App::finish_drag`) — soltar fora ou `Esc` descarta o clone sem desfazer nada, porque nada foi tocado. Duas simplificações documentadas: o realce `brightness(1.18)` e a sombra do fantasma (espec. §2.19) não têm primitiva equivalente em `porecatu-render` (nenhum hover em lugar nenhum do chrome usa filtro ainda) — o fantasma reaproveita as cores normais da aba; e o auto-scroll nas bordas durante o arraste acontece por evento de `CursorMoved` dentro da zona de 30px, não pelo intervalo real de `.15s` da espec., que exigiria um temporizador de UI (`ControlFlow::WaitUntil`) que esta etapa não introduziu. *(A F3 descartou o encolhimento por busca binária e trocou `drag_target_index` por `drag_target` — ver os blocos da F3 acima.)*
>
> **Na implementação (F2, etapa 6 — os quatro widgets de chrome e a segunda janela).** `App` deixa de ser uma janela só: o que era campo direto de `App` (workspace, abas, rename, scroll, arraste, ...) migra para `WindowState`, guardado num `HashMap<WindowId, WindowState>`; o que não varia por janela (`GpuContext` do processo, `startup_directory`, `cell_metrics` -- DPI-independente em pixels lógicos, só `WindowSurface` converte pra físico) continua em `App`. Quatro módulos de estado novos, puros e testados sem `winit`/`wgpu`, no mesmo padrão de `rename.rs` (Etapa 4): `warning.rs` (`WarningStack`, ADR-0014 canal 1), `dialog.rs` (`ConfirmDialog`, RF-10.18), `context_menu.rs` (`ContextMenu`, RF-10.19/RF-10.20) e `tooltip.rs` (`Hover`, ADR-0019) -- os três primeiros recebem `Instant` de fora (nunca chamam `Instant::now()`), o que torna atraso e expiração testáveis sem dormir de verdade. A pintura dos quatro fica em `overlay.rs`, mesma divisão de responsabilidade que `chrome.rs` já tinha com `tab_bar.rs`: geometria calculada na hora de pintar, sem camada pura testável (mesma nota de `chrome.rs`).
>
> O temporizador que o ADR-0014 e o ADR-0019 previram ("marca sujeira, não roda loop") é `ControlFlow::WaitUntil`: `App::schedule_next_wake` pega o menor `next_deadline()` entre todas as janelas e agenda; `new_events` com `StartCause::ResumeTimeReached` dispara `tick_all`, que expira avisos e promove tooltip pendente, redesenhando só as janelas que mudaram. Nenhuma thread de timer -- é o próprio event loop do `winit` que dorme até a hora certa.
>
> RF-1.6 (confirmar fechar aba com processo em primeiro plano), adiado desde a Etapa 4, fecha aqui: a condição do ADR-0017 (tela alternativa ou reporte de mouse ligado) já estava disponível via `Terminal::modes()`, só faltava o diálogo pra perguntar.
>
> **Na implementação (F3, etapas 1 e 2 — modelo de grupos e seleção).** `porecatu-core` recebe `GroupColor`/`GroupKind`/`GroupMeta` e o grupo implícito deixa de ser único: é **um por run contíguo** de abas sem grupo, cada um com `GroupId` de sessão ([ADR-0020](adr/0020-grupos-explicitos.md)). `normalize_groups` restabelece as invariantes — nenhum run vazio, nenhum par de runs implícitos adjacentes — depois de **toda** operação estrutural, em vez de cada operação cuidar do seu caso: é o que torna testável sequência de operações, não só operação isolada. `navigable_order()` entra ao lado de `visual_order()` (aba de grupo colapsado sai da primeira, nunca da segunda), a escada de foco de quatro níveis do RF-1.5 fica numa função só usada por `close_tab` e por `collapse_group`, e o MRU por grupo é o campo `Group::last_active`, escrito por `activate_tab`. As operações novas: `group_tabs`, `ungroup`, `rename_group`, `set_group_color`, `collapse_group`, `next_auto_color`, e depois `move_tab_to_group`, `move_tab_to_group_at`, `move_tab_to_new_run`, `move_group`.
>
> A seleção múltipla **não entra no core**: `Selection` (`porecatu-ui/selection.rs`) é estado efêmero de `WindowState` — `BTreeSet<TabId>` mais âncora explícita —, testado puro, e `group_tabs` recebe a lista de `TabId` já resolvida ([ADR-0021](adr/0021-selecao-multipla-e-gestos-da-barra.md) §1). O modificador é `Ctrl` em Windows/Linux e `Cmd` no macOS, onde `Ctrl`+clique na barra abre o menu de contexto em vez de tocar a seleção.
>
> **Na implementação (F3, etapas 3 a 5 — pílula, colapso e os três widgets).** A geometria da pílula (§2.4) entra em `tab_bar::layout` como `GroupPillRect`, antes das abas no wrapper; grupo colapsado simplesmente **não gera `TabRect`**, e é daí que sai "suas abas somem da barra" sem lógica de visibilidade em quem pinta. `next_tab`/`prev_tab` passam a andar sobre `navigable_order()` — a lacuna que a etapa 1 deixou, com a função pronta e sem chamador.
>
> Três módulos de estado puro, no mesmo padrão de `rename.rs`/`dialog.rs`: `group_menu.rs` (a **definição única** das seis `GroupAction` que o RF-10.21 exige — menu de contexto e editor leem a mesma lista, com rótulo dinâmico onde a espec. pede), `group_editor.rs` (três regiões navegáveis por `Tab`/`Shift+Tab`, edição de nome ao vivo sem escrever no `Workspace` até `Enter` — o truque do rename da F2 — e cor/ação só no `Enter` ou clique) e `move_to_group.rs` (a primeira lista **rolável** do chrome, sem estado de scroll próprio: `overlay.rs` deriva o deslocamento do item realçado). Os cinco popovers nunca coexistem: `WindowState::close_all_popovers`, um ponto só.
>
> **Na implementação (F3, etapa 6 e correções — arraste e animação).** `tab_bar::drag_target` substitui `drag_target_index`: resolve o alvo em **qualquer** wrapper, com a regra de fronteira do ADR-0021 §4 (o gap entre wrappers pertence ao grupo da esquerda; sobre a pílula entra no início do grupo, o que também cobre grupo colapsado, que não tem trilha para mirar; fora de tudo cria run implícito novo). O gesto continua sem tocar o `Workspace` real — clone por redraw, como a F2 estabeleceu. Arrastar a **pílula** move o grupo inteiro (`Workspace::move_group`), nunca para dentro de outro grupo, e o fantasma é só a pílula.
>
> A animação é `AnimationClock` (ver a seção 2): captura a geometria de `old_layout` antes da operação e interpola até o layout novo. Duas correções depois da etapa 6 mudaram o que se interpola: além da posição X de cada wrapper, a **cápsula interpola largura** (senão o grupo tocado salta para o tamanho final enquanto só os vizinhos deslizam) e as abas que somem ou aparecem do layout interpolam **opacidade** (`old_tabs`/`had_tab`). `Workspace` nunca é interpolado; quem interpola é `chrome.rs`, e `tab_bar::layout` segue alheio a isso.
>
> Uma terceira correção veio de um bug de longa data em `Workspace::group_tabs`, e é o tipo de coisa que só um gesto de UI expõe: agrupar a partir de um grupo explícito empurrava o grupo novo antes do que sobrava, mesmo quando a aba extraída vinha depois — a ordem de `self.groups` saía errada em silêncio, e o sintoma observável era a animação de colapso "só funcionar no primeiro grupo", porque o grupo errado recebia a geometria antiga.
>
> **Na implementação (F3 — fim da ordem de cedência do overflow).** `fit_width` deixou de encolher qualquer coisa e virou sinônimo de `layout`: a busca binária sobre o teto do rótulo (nota da etapa 5 da F2, abaixo) fazia até 24 recálculos completos da trilha **por frame**, cada um remedindo o texto de toda aba com `cosmic-text` sem cache — custo que cresce com o número de abas, justamente no caso de overflow que a motivava. Rótulo e nome de pílula ficam no teto e a trilha rola como um componente só (`trilha_width`/`right_zone_width` separam a trilha rolável da zona fixa à direita, que estreou com o botão de nova aba global e hoje carrega o de configurações). `available_width` continua na assinatura, sem uso dentro da função, para não mexer em todos os chamadores. Divergência registrada na seção 4.4 da [especificação visual](design/especificacao-visual.md), com a prosa da §2.18 já atualizada.
>
> **Na implementação.** 404 testes no workspace ao fim da F4, contra 292 ao fim da F3: `porecatu-ui` 185, `porecatu-core` 76, `porecatu-term` 53, `porecatu-config` 53, `porecatu-render` 29, `porecatu-pty` 8. `porecatu-config` nasceu na F4 (etapa 1) e cresceu junto com cada etapa que ela desbloqueou (`Action`, `Chord`/`keymap`, merge de tema). O crescimento de `porecatu-ui` é o mesmo padrão das fases anteriores: cada estado novo (hot reload, resolução de keybindings, zoom/tema de sessão) nasceu como módulo puro, testável sem `winit` nem `wgpu`.
>
> **Na implementação.** 145 testes no workspace ao fim da F2, contra 51 ao fim da F1: `porecatu-term` 51, `porecatu-ui` 43, `porecatu-core` 23, `porecatu-render` 20, `porecatu-pty` 10 — os três últimos crates não tinham teste nenhum na F1. `porecatu-ui` deixou de ser um crate sem teste porque a F2 extraiu o estado puro dos widgets e o layout da barra para módulos que não dependem de `winit` nem de `wgpu`. Da F1: golden-style alimentando o parser com sequência VT crua (sem PTY real — o [ADR-0004](adr/0004-pty-cross-platform.md) avisa que o ConPTY reemite bytes de um jeito não portável), mais unitários puros de codificação de tecla e de reporte de mouse, que não dependem de `winit` nem do motor rodando, só de `TermModes`. `porecatu-pty` tem 8, incluindo integração de spawn/kill. Um teste de regressão cobre o deadlock de fechamento da seção 2: fecha um terminal com processo de longa duração numa thread separada com timeout, e falha se `shutdown` travar.
