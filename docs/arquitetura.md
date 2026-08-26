# Arquitetura

Documento técnico central. Os ADRs justificam *por que* cada peça foi escolhida; este documento descreve *como* elas se encaixam.

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

**`porecatu-core` não depende de nada.** É o modelo de domínio puro: `Workspace`, `Group`, `Tab`, IDs, tipos geométricos. Serializável, testável sem GPU e sem PTY. É por isso que `porecatu-session` consegue ser um crate trivial: ele serializa `core` e mais nada.

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
    proxy.send_event(Wakeup(tab_id));        // acorda a UI
}
```

Bloquear aqui é correto e barato — a thread existe justamente para isso. O `Mutex` é segurado só durante o `advance`, nunca durante o render.

### Escrita

Input do teclado vira bytes no `porecatu-ui` e é enviado por `mpsc::Sender` para o handle de escrita do PTY. A escrita não passa pela thread de leitura.

### Render damage-driven

Este é o ponto onde emuladores ingênuos queimam bateria. `cargo build` cospe centenas de linhas em milissegundos; se cada `Wakeup` disparar um frame, o app renderiza centenas de frames que ninguém vê.

A regra:

1. `Wakeup(tab_id)` marca a aba como suja e nada mais.
2. Se a aba suja não é a visível, para por aí — só o grid é atualizado, sem render.
3. Se é a visível, agenda um `request_redraw()` **no máximo uma vez por intervalo de frame** (limitado pela taxa de atualização do monitor).
4. Terminal ocioso = zero frames. Não há loop de render contínuo.

O mesmo vale para o chrome: mudança de hover, foco ou config marca a barra de abas como suja.

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

## 4. Fronteira de render

`porecatu-render` recebe uma lista de primitivas por frame:

- `Quad { rect, color }`
- `RoundedQuad { rect, radii, color, border }`
- `TextRun { origin, text, font, size, color }`
- `PushClip(rect)` / `PopClip`

Duas passadas de pipeline por frame: uma de geometria (quads instanciados, cantos arredondados via SDF no fragment shader) e uma de texto (`glyphon`, com atlas de glyphs em cache entre frames).

A grade do terminal é um caso particular: fundo de célula vira quads em batch, glyphs viram um `TextRun` por run de mesmo estilo — não um por caractere.

**Nenhuma cor, raio ou dimensão é hardcoded no renderer.** Tudo vem de `Config` via `ui`. É isso que torna o requisito de customização (PRD-004, PRD-005) uma questão de configuração e não de recompilação.

---

## 5. Configuração e hot reload

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

## 6. Estratégia de teste

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
