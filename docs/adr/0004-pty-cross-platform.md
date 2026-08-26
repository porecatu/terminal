# ADR-0004 — PTY cross-platform via portable-pty

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** ADR-0002, ADR-0005, ADR-0007

## Contexto

O v1 tem as três plataformas no escopo: Windows, Linux e macOS. Cada uma abre pseudo-terminal de forma diferente:

- **Unix (Linux, macOS):** `openpty`/`forkpty`, par master/slave, `ioctl(TIOCSWINSZ)` para resize, `SIGWINCH` para notificar o processo filho.
- **Windows:** ConPTY, a API introduzida no Windows 10 1809 (`CreatePseudoConsole`, `ResizePseudoConsole`). Não é um PTY Unix com nomes diferentes — o modelo é outro.

Essas diferenças **vazam para o comportamento observável**, não ficam contidas na camada de abstração. Precisam ser documentadas onde a implementação vai encontrá-las, senão viram bug misterioso na fase F1.

## Decisão

Usar o crate **`portable-pty`** (do projeto WezTerm) como camada de abstração, encapsulado em `porecatu-pty`.

`porecatu-pty` expõe uma superfície mínima:

- `spawn(shell, args, env, cwd, size) -> PtyHandle`
- `PtyHandle::reader() -> impl Read + Send` (consumido pela thread de leitura, ver [ADR-0007](0007-modelo-de-threading.md))
- `PtyHandle::writer() -> impl Write + Send`
- `PtyHandle::resize(rows, cols)`
- `PtyHandle::child_status()` — para saber que o shell morreu
- `PtyHandle::kill()`

Nada mais. Nenhum outro crate importa `portable-pty`.

### Comportamento de shell padrão

| Plataforma | Ordem de resolução |
|---|---|
| Linux | `config.shell.program` → `$SHELL` → `/bin/sh` |
| macOS | `config.shell.program` → `$SHELL` → `/bin/zsh` |
| Windows | `config.shell.program` → PowerShell 7 (`pwsh.exe`) se presente → `powershell.exe` → `cmd.exe` |

## Diferenças de plataforma que vazam

Esta seção é a razão principal deste ADR existir.

### ConPTY re-renderiza a tela

ConPTY mantém seu próprio buffer de console e **reemite conteúdo** para manter o terminal sincronizado, incluindo sequências de posicionamento de cursor e reescrita de linhas que um PTY Unix nunca enviaria. Consequência prática: o fluxo de bytes no Windows é mais volumoso e menos previsível. Testes de conformidade baseados em golden files de saída de PTY **não são portáveis entre plataformas** — os golden files de `porecatu-term` devem alimentar o parser diretamente, sem passar por PTY real.

### Não existe `SIGWINCH` no Windows

No Unix, o resize do PTY entrega `SIGWINCH` ao processo filho, que relê o tamanho e se redesenha. No Windows, `ResizePseudoConsole` altera o buffer do console e o ConPTY lida com a notificação por conta própria. O efeito visível difere: aplicações TUI podem reagir em momentos distintos, e o conteúdo pode ser reflowado pelo ConPTY em vez de pela aplicação.

### Semântica de resize e reflow

Reduzir a largura no Unix normalmente trunca ou deixa a aplicação decidir; o ConPTY tem seu próprio comportamento de reflow do buffer. Não tentar unificar isso — resize é operação por plataforma e o teste é manual, com `vim` e `htop` abertos.

### Encerramento do processo filho

Unix: o filho vira zumbi até `waitpid`; ler do master retorna EOF. Windows: o handle do processo sinaliza; o pipe fecha. `portable-pty` normaliza o suficiente, mas o **momento** do EOF em relação à disponibilidade dos últimos bytes de saída difere. Ler até EOF antes de considerar a aba morta, sempre — senão a última linha de saída se perde.

### Codificação e locale

Unix: UTF-8 assumido, `LANG` no ambiente. Windows: o code page do console pode não ser UTF-8. Forçar UTF-8 no ConPTY no spawn; não confiar no default do sistema.

### Detecção de diretório atual

Não há API barata no Windows para ler o cwd de um processo filho, ao contrário de `/proc/<pid>/cwd` no Linux. Isso afeta diretamente a restauração de sessão e é tratado em [ADR-0005](0005-persistencia-de-sessao.md).

## Alternativas consideradas

### `nix` + `windows-sys` direto, abstração própria

Escrever a camada de PTY na mão, sem crate intermediário. Dá controle total e remove uma dependência.

Descartada porque as armadilhas do ConPTY (ordem de criação de handle, herança de handle, pipe de I/O, cleanup) são exatamente o tipo de conhecimento que o `portable-pty` já encapsula depois de anos de uso no WezTerm. Reimplementar é assumir os mesmos bugs de novo, um por um.

### `pty-process` / `pty` (crates só-Unix)

APIs mais agradáveis, mas Unix-only. Windows está no escopo do v1 e é a plataforma primária de desenvolvimento. Descartadas por não cobrirem o requisito.

### `alacritty_terminal::tty`

O Alacritty tem seu próprio módulo de TTY cross-platform, e já usamos o crate ([ADR-0002](0002-motor-vte.md)) — seria uma dependência a menos.

Descartada porque é módulo interno pensado para o event loop do próprio Alacritty, com acoplamento a decisões dele; e porque manter PTY e motor VT como dependências independentes preserva a capacidade de trocar um sem o outro, que é o ponto do isolamento em `porecatu-pty` e `porecatu-term`.

## Consequências

### Positivas

- Uma API de PTY para as três plataformas; `porecatu-term` não sabe em que sistema roda.
- ConPTY tratado por código maduro em vez de nosso primeiro contato com a API.
- Fronteira estreita (seis funções) mantém a substituição viável.

### Negativas

- Dependência transitiva vinda do ecossistema WezTerm.
- A abstração não elimina as diferenças de comportamento listadas acima — elas continuam exigindo teste manual por plataforma.
- Reflow no resize será perceptivelmente diferente entre Windows e Unix, e isso é aceito, não corrigido.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Bug específico de ConPTY aparecer só no Windows | Alta | Médio | Windows é a plataforma primária de desenvolvimento; CI nas três desde F0 |
| Comportamento de resize divergente confundir usuários | Média | Baixo | Documentar como limitação conhecida; teste manual com `vim` e `htop` no critério de saída de F1 |
| Perda dos últimos bytes de saída ao fechar aba | Média | Médio | Regra explícita: ler até EOF antes de marcar a aba como encerrada |
| Code page do Windows corromper acentuação | Média | Médio | Forçar UTF-8 no spawn, com teste dedicado |
