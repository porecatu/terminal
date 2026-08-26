# ADR-0007 — Modelo de threading e render damage-driven

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** ADR-0001, ADR-0002, ADR-0004

## Contexto

Um emulador com N abas tem N fontes de bytes chegando de forma assíncrona e imprevisível, e uma superfície de desenho compartilhada. Três forças em conflito:

1. **`winit` e `wgpu` exigem a main thread.** macOS obriga interação com janela na thread principal; Windows tem restrições equivalentes. Não é preferência de design.
2. **Ler de PTY bloqueia.** Não há como saber quando os bytes chegam sem bloquear ou fazer polling.
3. **Terminal ocioso deve custar zero.** Um emulador que renderiza continuamente drena bateria de laptop sem entregar nada.

O terceiro ponto merece número concreto: `cargo build --workspace` num projeto médio emite alguns milhares de linhas em poucos segundos. Se cada chegada de bytes disparar um frame, o app renderiza milhares de frames dos quais o usuário vê, no máximo, 60 por segundo. O trabalho restante é puro desperdício de CPU e GPU.

## Decisão

### Distribuição de trabalho

| Thread | Responsabilidade |
|---|---|
| **Main** | Event loop `winit`, mutação do `Workspace`, layout de UI, submissão de frame `wgpu` |
| **Leitura de PTY** (uma por aba) | `read()` bloqueante, alimentar o parser VT, aplicar no `Term`, sinalizar a UI |
| **Config watcher** (uma) | `notify` + parse do TOML fora da main thread |
| **Sessão** (ocasional) | Escrita atômica do `session.json` com debounce |

A main thread **nunca** faz I/O bloqueante.

### Thread de leitura

```rust
loop {
    let n = match reader.read(&mut buf) {
        Ok(0) => break,              // EOF: shell encerrou
        Ok(n) => n,
        Err(e) if e.kind() == Interrupted => continue,
        Err(e) => break,
    };
    {
        let mut term = term.lock();
        parser.advance(&mut *term, &buf[..n]);
    }                                 // lock liberado aqui, sempre
    proxy.send_event(Wakeup(tab_id));
}
```

Duas regras:

- O `Mutex<Term>` é segurado **só durante o `advance`**. Nunca durante render, nunca durante I/O.
- Ler até EOF antes de considerar a aba encerrada — senão a última linha de saída se perde ([ADR-0004](0004-pty-cross-platform.md)).

### Escrita

Input de teclado vira bytes em `porecatu-ui` e vai por `mpsc::Sender` para o handle de escrita. Não passa pela thread de leitura. O `Write` do PTY é `Send` e independente do `Read`.

### Render damage-driven

O núcleo da decisão:

1. `Wakeup(tab_id)` **não renderiza nada**. Marca a aba como suja.
2. Aba suja que não é a visível: para aí. O `Term` já foi atualizado pela thread de leitura; quando a aba for focada, o conteúdo está lá. Zero custo de GPU para abas em segundo plano.
3. Aba suja que é a visível: agenda `request_redraw()`, **no máximo uma vez por intervalo de frame**. Wakeups adicionais dentro do mesmo intervalo são absorvidos pela flag de sujeira.
4. Mudanças de chrome (hover, foco, drag, reload de config) marcam a barra como suja pelo mesmo mecanismo.
5. **Sem sujeira, sem frame.** Não existe loop de render contínuo. Terminal parado consome zero GPU.

O intervalo de frame vem da taxa de atualização do monitor. O cursor piscando é um timer que marca sujeira, não um loop.

### Snapshot antes do desenho

No `RedrawRequested`:

1. Trava o `Term`.
2. Copia as células visíveis para um buffer de snapshot (reusado entre frames, sem alocação no caminho quente).
3. **Solta o lock.**
4. Só então faz layout e submete à GPU.

Se o render segurasse o lock, a thread de leitura ficaria esperando a GPU — e a latência de saída do terminal passaria a depender do driver gráfico.

### Propriedade dos dados

| Dado | Dono | Compartilhamento |
|---|---|---|
| `Term` | thread de leitura + render | `Arc<Mutex<Term>>` |
| `Workspace` | main thread | exclusivo, **sem lock** |
| `Config` | main thread | `Arc<Config>`, trocado inteiro no reload |
| Writer do PTY | canal | `mpsc::Sender` clonável |

`Workspace` sem lock não é otimização: é a consequência de só a main thread poder mutá-lo. Se aparecer necessidade de mutar `Workspace` de outra thread, a resposta é enviar um evento pelo `EventLoopProxy`, não adicionar um `Mutex`.

`Config` é imutável; hot reload troca o `Arc` inteiro. Nenhum leitor observa estado meio-atualizado.

## Alternativas consideradas

### Runtime async (tokio) com tasks por PTY

Idiomático em Rust moderno, e evita N threads do sistema. Descartada porque não há ganho real: a contagem de abas é dezenas, não milhares — o custo de N threads bloqueadas é irrelevante. Em troca, arrastaríamos um runtime inteiro e a ponte entre o event loop do `winit` (que não é async) e o executor async seria um ponto permanente de atrito. A leitura de PTY é justamente o caso em que uma thread bloqueada é a solução simples e correta.

### Uma thread única multiplexando todos os PTYs (`epoll`/`kqueue`/IOCP)

Escala melhor e é o que emuladores clássicos fazem. Descartada por complexidade de plataforma: IOCP no Windows é um modelo diferente de `epoll`, e o ConPTY não se encaixa limpo em multiplexação por readiness. Uma thread por aba é chata e funciona nas três plataformas com o mesmo código. Revisitar só se o uso mostrar centenas de abas simultâneas.

### Render em loop contínuo (estilo game loop)

Simples de escrever, latência previsível. Descartada pelo custo de energia: um emulador de terminal fica ocioso a maior parte do tempo, e renderizar 60 FPS de tela parada é indefensável num laptop.

### Renderizar direto no `Wakeup`, sem coalescing

Descartada pelo motivo do contexto: milhares de frames invisíveis durante saída rápida. É o bug de performance mais comum em emuladores caseiros.

### `RwLock` no `Term` em vez de `Mutex`

Permitiria render e leitura concorrentes. Descartada porque só há um leitor (o render) e um escritor (a thread de leitura); `RwLock` adiciona custo sem concorrência para explorar.

## Consequências

### Positivas

- Terminal ocioso custa zero CPU e zero GPU.
- Saída rápida não degrada a UI: os bytes entram no `Term` na velocidade do PTY, o desenho acompanha na velocidade do monitor.
- Abas em segundo plano são baratas — só memória de grid.
- Modelo simples de raciocinar: um lock, um dono por dado, uma direção de mensagem.

### Negativas

- Uma thread do SO por aba. Com 100 abas, 100 threads — funciona, mas não é elegante.
- Coalescing introduz latência de até um intervalo de frame entre o byte e o pixel. Imperceptível na prática, mas real.
- A regra "só a main thread muta `Workspace`" precisa ser respeitada por disciplina; o compilador não a impõe sozinho.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Contenção do `Mutex<Term>` sob saída muito rápida | Média | Médio | Lock só no `advance`; snapshot solta o lock antes da GPU; medir com `yes` rodando |
| Alguém segurar o lock durante o render | Média | Alto | Snapshot em função separada que devolve o buffer; revisão de código atenta a isso |
| Fila do `EventLoopProxy` encher com `Wakeup` | Média | Médio | `Wakeup` é idempotente (só marca flag); considerar flag atômica compartilhada se a fila virar gargalo |
| 100+ abas esgotarem threads | Baixa | Médio | Aceito no v1; migração para multiplexação é local à `porecatu-pty` |
| Cursor piscando forçar frames desnecessários | Média | Baixo | Timer só marca sujeira da região do cursor; desligável na config |
