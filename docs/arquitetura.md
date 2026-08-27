# Arquitetura

Documento técnico central. Os ADRs justificam *por que* cada peça foi escolhida; este documento descreve *como* elas se encaixam.

As seções 2 e 4 estão implementadas (F0 e F1 do [roadmap](roadmap.md)); a seção 5 está implementada na forma da F1 e é revista pelo [ADR-0018](adr/0018-composicao-de-frame.md) na F2; as seções 3 (parte de chrome), 6 e a metade de `core`/`config`/`session` da seção 1 ainda são projeto. Onde o código divergiu do escrito, há um bloco **Na implementação** dizendo o quê e por quê.

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

O grafo de dependências permitido está tabelado em [CLAUDE.md](../CLAUDE.md). Duas regras merecem destaque:

**`porecatu-render` não conhece o domínio.** Ele expõe um punhado de primitivas — retângulo, retângulo arredondado, run de texto, clip rect — e nada mais. Não sabe o que é uma aba. Isso mantém o renderer testável e substituível, e força a aparência configurável a viver onde ela pertence: em `config` (o que o usuário pediu) + `ui` (como isso vira geometria).

O [ADR-0018](adr/0018-composicao-de-frame.md) acrescenta duas coisas a esse crate sem furar a regra: **camadas** (uma sequência ordenada de listas de primitivas, para que popover possa cobrir texto) e um **medidor de texto sem GPU**, que mede string, face e tamanho e continua não sabendo o que é uma aba. É o medidor que torna o layout da barra a função pura da seção 7.

**`porecatu-core` não depende de nada.** É o modelo de domínio puro: `Workspace`, `Group`, `Tab`, IDs, tipos geométricos. Serializável, testável sem GPU e sem PTY. É por isso que `porecatu-session` consegue ser um crate trivial: ele serializa `core` e mais nada.

> **Na implementação.** Na F1 o crate tem só `TabId`, para que o `Wakeup { window, tab }` do [ADR-0015](adr/0015-multiplas-janelas.md) nascesse com o formato certo. `Workspace`, `Group` e `Tab` entram na F2, com `serde` derivado já ali — o round-trip que o [ADR-0006](adr/0006-modelo-de-abas-e-grupos.md) lista como invariante é testável na fase que o exige, mesmo com `porecatu-session` ainda vazio. `Tab` carrega o estado `Exited` do [ADR-0017](adr/0017-ciclo-de-vida-da-aba.md).

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
> **Encerramento de uma aba (F2).** O [ADR-0017](adr/0017-ciclo-de-vida-da-aba.md) decide o que o RF-1.2 chamava de "aguardar EOF": o ConPTY não emite EOF, então a espera é pela confirmação de morte da thread de observação, **fora da main thread**. A aba sai da barra imediatamente. Sem isso, fechar uma janela com 50 abas custaria 50 × `SHUTDOWN_TIMEOUT` na main thread, contra a métrica de 50 abas do PRD-001.
>
> **Encerramento (Windows).** `Terminal::shutdown` não dá `join` em nenhuma das três threads e **não fecha o pseudo-console**. `ClosePseudoConsole` bloqueia até o pipe de leitura clonado ser liberado, e a thread de leitura está parada num `read()` síncrono nele — as duas esperam uma pela outra e o app trava. O que se faz é matar o processo e `mem::forget` no handle: o SO reclama tudo quando o Porecatu sai. A confirmação de "processo morto" vem de um canal dedicado que a thread de observação sinaliza, com timeout de segurança. Dar `join` na thread de leitura violaria a regra de que a main thread nunca bloqueia, justamente no fechamento da janela.

### Render damage-driven

Este é o ponto onde emuladores ingênuos queimam bateria. `cargo build` cospe centenas de linhas em milissegundos; se cada `Wakeup` disparar um frame, o app renderiza centenas de frames que ninguém vê.

A regra:

1. `Wakeup { window, tab }` marca a aba como suja e nada mais. O par é necessário porque `TabId` é gerado por workspace, e workspace é por janela — só o `TabId` não identifica a aba ([ADR-0015](adr/0015-multiplas-janelas.md)).
2. Se a aba suja não é a visível, para por aí — só o grid é atualizado, sem render.
3. Se é a visível, agenda um `request_redraw()` **no máximo uma vez por intervalo de frame** (limitado pela taxa de atualização do monitor).
4. Terminal ocioso = zero frames. Não há loop de render contínuo.

O mesmo vale para o chrome: mudança de hover, foco ou config marca a barra de abas como suja.

> **Na implementação.** O ponto 3 não precisou de bookkeeping próprio de sujeira na F1: `request_redraw` do `winit` já coalesce chamadas repetidas antes do próximo `RedrawRequested` num só evento (no Win32 é uma flag booleana, não fila). N wakeups de saída rápida viram um frame. O ponto 2 já existe com a forma final — `Wakeup { window, tab }` é comparado com a aba visível antes de qualquer redraw, mesmo havendo hoje uma janela e uma aba só.

### Propriedade dos dados

| Dado | Dono | Compartilhamento |
|---|---|---|
| `Term` (grid, scrollback) | thread de leitura + render | `Arc<Mutex<Term>>` |
| `Workspace` (abas, grupos) | main thread | exclusivo, sem lock |
| `Config` | main thread | `Arc<Config>`, trocado inteiro no reload |
| Handle de escrita do PTY | `mpsc` | clonável |

`Workspace` só é tocado pela main thread, então não precisa de lock. `Config` é imutável e trocado por inteiro no hot reload — nenhum lock, só uma troca de `Arc`.

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

É a fronteira mais crítica do projeto: separa a F1 da F2, contém o
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
| `Exit` | EOF do PTY | `ui` (RF-1.3) |

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
> - As faces são carregadas por `include_bytes!` num `fontdb::Database` que **nunca chama `load_system_fonts`**. É o que garante a precedência do [ADR-0016](adr/0016-fontes-embutidas.md) sem lógica de desempate: não existe cópia do sistema para competir. Emoji, CJK e Nerd Font ficam de fora até haver fallback configurável (F4).
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

---

## 7. Estratégia de teste

| Crate | Como testar |
|---|---|
| `porecatu-core` | testes unitários puros: mover aba entre grupos, agrupar, desagrupar, invariantes de ordenação |
| `porecatu-config` | round-trip TOML, defaults completos, rejeição de config inválida com erro legível |
| `porecatu-session` | round-trip de serialização, migração de `schema_version`, recuperação de arquivo corrompido |
| `porecatu-term` | golden files: sequência de escape na entrada, dump de grid esperado na saída |
| `porecatu-pty` | teste de integração por plataforma: spawn de `echo`, resize, encerramento |
| `porecatu-render` | sem teste automatizado no v1 — verificação manual e screenshot |
| `porecatu-ui` | hit-testing e layout são funções puras sobre geometria, testáveis sem janela |

O layout da barra de abas é deliberadamente uma função pura `(Workspace, Config, largura) -> Vec<TabRect>`. Isso permite testar overflow, colapso de grupo e truncamento de título sem abrir uma janela.

Isso só é cumprível porque o medidor de texto do [ADR-0018](adr/0018-composicao-de-frame.md) se constrói **sem `Device` nem `Queue`**: a função recebe o medidor emprestado, e o teste constrói um sem tocar em `wgpu`. Na F1 não era — o `FontSystem` vivia dentro do pipeline de texto, que exige GPU, e não havia como medir largura de string proporcional.

> **Na implementação (F2, etapa 3 — layout puro e hit-testing).** A assinatura real, em `porecatu-ui/tab_bar.rs`, é `(&Workspace, &TabBarStyle, &mut TextMeasurer) -> TabBarLayout` — `Config` ainda não existe (`TabBarStyle` é constantes com a chave TOML de origem no comentário, como `palette.rs` já fazia na F1) e o resultado é agrupado por `GroupWrapperRect` (um por grupo não vazio, com seus `TabRect`), não uma lista achatada — a estrutura em árvore do ADR-0006 (`Workspace -> Group -> Tab`) se preserva no layout. O parâmetro `largura` ainda não existe: esta etapa produz a geometria natural da trilha (rótulo truncado só no teto fixo de 180px, ADR-0018/espec. §2.5), sem clamping ao espaço disponível; encolhimento abaixo disso e rolagem entram na Etapa 5, que é quando `largura` passa a fazer parte da assinatura. `hit_test(&TabBarLayout, ponto) -> Option<TabBarHit>` é a segunda função pura que a seção promete — resolve corpo da aba, botão de fechar (com a folga de 2px da espec. §2.2) e botão de nova aba, com a fronteira entre abas vizinhas partindo o `gap` ao meio.

> **Na implementação (F2, etapa 4 — ciclo de vida, OSC 7, título e rename).** `App` deixa de modelar uma `Terminal` só e passa a ter `Workspace` + `HashMap<TabId, TabRuntime>` (`Terminal` + snapshot por aba) — a fronteira entre domínio puro e I/O que a seção 4 já descrevia, agora com mais de um lado ocupado. `rename.rs` é o modo de captura do ADR-0008 passo 1 (RF-1.8/RF-1.9): `RenameState` puro, sem `winit`, testável isolado — o buffer não tem posição de cursor no meio da string (sempre no fim), simplificação que `chrome.rs` já assume ao desenhar o caret. `chrome.rs` (tradução de `TabBarLayout` + estado efêmero para `Primitive`) segue a mesma linha de `porecatu-render`: sem teste automatizado, verificação manual — é pintura, não geometria. `porecatu-term` ganha `osc7.rs` (captura de OSC 7, golden-style como o resto do crate) e dois testes de integração novos: `Terminal::close` devolve antes de `SHUTDOWN_TIMEOUT` mesmo com processo vivo (a versão não-bloqueante de `shutdown`, ADR-0017 item 4), e `inject_note` aparece no snapshot como se fosse saída do programa (RF-1.3).
>
> **Na implementação (F1).** 51 testes no workspace. `porecatu-term` tem 43 deles: golden-style alimentando o parser com sequência VT crua (sem PTY real — o [ADR-0004](adr/0004-pty-cross-platform.md) avisa que o ConPTY reemite bytes de um jeito não portável), mais unitários puros de codificação de tecla e de reporte de mouse, que não dependem de `winit` nem do motor rodando, só de `TermModes`. `porecatu-pty` tem 8, incluindo integração de spawn/kill. Um teste de regressão cobre o deadlock de fechamento da seção 2: fecha um terminal com processo de longa duração numa thread separada com timeout, e falha se `shutdown` travar.
