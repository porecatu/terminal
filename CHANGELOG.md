# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).
Versionamento seguirá [SemVer](https://semver.org/lang/pt-BR/) a partir do
primeiro release.

> **Nenhuma versão foi publicada ainda.** As fases F0, F1 e F2 do
> [roadmap](docs/roadmap.md) estão implementadas — `cargo run` abre uma janela
> com abas de terminal funcionais, e `Ctrl+Shift+N` abre uma segunda janela —,
> mas artefatos de release só aparecem na F6.

## [Não publicado]

### Adicionado

#### F2 — Abas

O app deixa de modelar um terminal só. Entregue em seis etapas, uma por PR.
Não há grupos nomeados, configuração nem sessão: o grupo implícito do
[ADR-0006](docs/adr/0006-modelo-de-abas-e-grupos.md) é o único que existe, e
os valores de aparência seguem como constantes citando a chave TOML de origem.

- **Decisões que a fase exigia**, escritas antes de ela abrir:
  [ADR-0017](docs/adr/0017-ciclo-de-vida-da-aba.md) (ciclo de vida e identidade
  da aba — OSC 7 antecipado, precedência de título sem o nível de processo em
  primeiro plano, encerramento sem EOF e sem bloquear a main thread, estado
  `Exited`), [ADR-0018](docs/adr/0018-composicao-de-frame.md) (composição de
  frame — camadas, recorte, medição de texto sem GPU) e
  [ADR-0019](docs/adr/0019-tooltip.md) (tooltip, o quarto widget de chrome, que
  o RF-1.10 exige e o ADR-0014 não previa)
- `porecatu-core`: `Workspace -> Vec<Group> -> Vec<TabId>` com IDs opacos e
  estáveis por contador monotônico, `Tab` carregando ciclo de vida, título com
  precedência, `cwd` e indicadores. Operações puras com testes de invariante, e
  `serde` derivado desde já — o round-trip `Workspace -> JSON -> Workspace` que
  o ADR-0006 lista como invariante é testável mesmo com `porecatu-session` vazio
- `porecatu-render`: o frame passa a ter **cinco camadas** (grade, chrome,
  aviso, popover, modal). `resolve_layer` — pura, testada sem GPU — percorre o
  stream mantendo a pilha de clip e agrupa geometria contígua de mesmo clip em
  batches, cada um com seu `set_scissor_rect`. Isso substitui o achatamento em
  três baldes da F1, que impedia tanto recorte quanto popover sobre texto.
  `TextMeasurer` mede string, face e tamanho **sem `Device` nem `Queue`**, dono
  do único `FontSystem` do processo. `Renderer` se parte em `GpuContext` (um por
  processo) e `WindowSurface` (uma por janela), o que abre caminho para a
  segunda janela
- Barra de abas: `layout(&Workspace, &TabBarStyle, &mut TextMeasurer)` e
  `hit_test(&TabBarLayout, ponto)`, as duas funções puras que a
  [arquitetura](docs/arquitetura.md) promete — testáveis sem janela e sem GPU.
  A fronteira entre abas vizinhas parte o `gap` ao meio; o botão de fechar tem
  prioridade sobre o corpo da aba onde os dois se sobrepõem
- Ciclo de vida: criar herdando o `cwd`, fechar com confirmação quando há
  programa de tela cheia (RF-1.6), navegar por sequência e por índice, renomear
  inline com modo de captura, e nota no grid quando o shell sai com código
  diferente de zero (RF-1.3). `porecatu-term` ganha captura de OSC 7 e
  `Terminal::close` não-bloqueante
- Overflow, arraste e indicadores: `fit_width` encolhe o teto do rótulo por
  busca binária até o piso; `overflow_state` resolve deslocamento de rolagem e
  contagem de abas ocultas de cada lado; o arraste **não toca o `Workspace`
  real** durante o gesto — clona, aplica no clone e só efetiva ao soltar, então
  `Esc` ou soltar fora não precisam desfazer nada
- Quatro widgets de chrome, com estado puro e testado sem `winit`
  (`warning.rs`, `dialog.rs`, `context_menu.rs`, `tooltip.rs`) e pintura em
  `overlay.rs`. Os temporizadores recebem `Instant` de fora, nunca chamam
  `Instant::now()`, o que torna atraso e expiração testáveis sem dormir
- Múltiplas janelas ([ADR-0015](docs/adr/0015-multiplas-janelas.md)):
  `WindowState` por `WindowId`, com `GpuContext`, `cell_metrics` e
  `startup_directory` seguindo por processo. Janela nova nasce em cascata de
  30 px a partir da que a criou
- Temporizadores de UI por `ControlFlow::WaitUntil` — sem thread própria e sem
  loop de render: o event loop dorme até a hora exata e volta a dormir depois
- 145 testes no workspace, contra os 51 da F1

#### F1 — Terminal único

Emulador de terminal funcional, entregue em seis etapas. Não há abas, grupos,
configuração nem sessão: tudo isso vem das fases seguintes.

- `porecatu-pty`: spawn, leitura, escrita, resize e encerramento sobre
  `portable-pty` ([ADR-0004](docs/adr/0004-pty-cross-platform.md)), com
  resolução de shell default por plataforma e o ambiente do
  [ADR-0012](docs/adr/0012-identificacao-do-terminal.md) (`TERM=xterm-256color`,
  `COLORTERM`, `TERM_PROGRAM`) injetado no spawn
- `porecatu-term`: `alacritty_terminal` encapsulado
  ([ADR-0002](docs/adr/0002-motor-vte.md)) — nenhum tipo do motor atravessa a
  API pública. Snapshot de grade com buffer reusado, cor **não resolvida**,
  arena de clusters para grafema composto e `wide_spacer` para largura dupla,
  conforme a [seção 4 da arquitetura](docs/arquitetura.md)
- Três threads por terminal (leitura, escrita, observação do processo) e loop
  de render **damage-driven** ([ADR-0007](docs/adr/0007-modelo-de-threading.md)):
  terminal ocioso não gera frame, CPU em ~0%. `Wakeup` já carrega
  `(WindowId, TabId)` ([ADR-0015](docs/adr/0015-multiplas-janelas.md))
- `porecatu-render`: pipeline de quads instanciados com cantos arredondados
  via SDF e pipeline de texto via `glyphon`, com atlas de glyphs em cache
  entre frames. Primitivas são o único contrato público — o crate não conhece
  aba nem grupo
- Cinco faces do IBM Plex embutidas no binário sem subsetting
  ([ADR-0016](docs/adr/0016-fontes-embutidas.md)), com a OFL e a atribuição em
  `assets/fonts/`. A grade do terminal deriva da métrica de fonte medida
- Teclado: codificação xterm de setas, navegação e F1–F12 com modificador,
  DECCKM, `Ctrl`+letra, `Alt` prefixando ESC, bracketed paste e IME
  (`Ime::Commit` vai direto ao terminal, sem consultar keybind — tecla morta
  do ABNT2 e composição de CJK dependem disso)
  ([ADR-0008](docs/adr/0008-teclas-e-roteamento-de-input.md))
- Mouse reportado ao programa nos modos 1000/1002/1003 com encoding SGR 1006 e
  fallback X10, e a regra de conflito do
  [ADR-0013](docs/adr/0013-mouse-selecao-e-clipboard.md): `Shift` força
  seleção local sempre, que é o que permite copiar de dentro do `htop`
  (PRD-010 RF-10.1 a RF-10.3)
- Seleção nos quatro modos do motor, com recorte de espaço à direita e
  remontagem de `WRAPLINE` (RF-10.4 a RF-10.9); clipboard via `arboard`
  encapsulado num único lugar; OSC 52 com escrita permitida e leitura negada
  por default (RF-10.10 e RF-10.11); `Ctrl+Shift+C`/`V`
- Rolagem de scrollback por teclado e por roda, com tela alternativa virando
  setas (RF-10.12 a RF-10.14) e resize propagado ao motor e ao PTY
- 51 testes: golden-style de sequência VT crua, unitários puros de codificação
  de tecla e de reporte de mouse, integração de PTY e regressão do deadlock de
  fechamento no Windows

#### F0 — Esqueleto

- Workspace Cargo com os oito crates, com os `Cargo.toml` refletindo o grafo
  de dependências de [CLAUDE.md](CLAUDE.md)
- Janela `winit` + surface `wgpu`, com `alacritty_terminal = "=0.26.0"` e
  `wgpu = "=30.0.1"` travados por igualdade exata
- Toolchain pinada em `rust-toolchain.toml` (1.98.0, edition 2024), lints em
  `[workspace.lints]` e `unsafe_code = "deny"`
  ([ADR-0011](docs/adr/0011-toolchain-rust.md))
- Matriz Rust do CI acordada nas três plataformas e job canário semanal ativo

#### Documentação

- Documentação normativa completa do v1: [ADR-0001 a 0023](docs/adr/),
  [PRD-000 a 005](docs/prd/) e PRD-010 aprovados, PRD-006 a 009 em rascunho
- Decisões que faltavam para a F1 e a F2 começarem sem pendência:
  toolchain Rust pinada ([ADR-0011](docs/adr/0011-toolchain-rust.md)),
  `TERM` e capacidades anunciadas ([ADR-0012](docs/adr/0012-identificacao-do-terminal.md)),
  reporte de mouse, seleção e política de clipboard
  ([ADR-0013](docs/adr/0013-mouse-selecao-e-clipboard.md)),
  superfície de aviso, diálogo e menu de contexto
  ([ADR-0014](docs/adr/0014-superficie-de-aviso-e-dialogo.md)) e
  múltiplas janelas em escopo mínimo
  ([ADR-0015](docs/adr/0015-multiplas-janelas.md))
- [Catálogo fechado de ações](docs/reference/acoes.md), com a origem de cada
  uma — a enumeração que o ADR-0008 exigia e não existia
- [Arquitetura](docs/arquitetura.md): camadas, modelo de threading e render
  damage-driven
- [Design](docs/design/README.md) importado do canvas, com tabela de tokens,
  anatomia por componente e classificação de fases `[v1]`/`[v2]`
- [Configuração de referência](docs/config/porecatu.example.toml) comentada,
  com os valores default vindos do design
- Licenciamento sob GPL-3.0-or-later ([ADR-0010](docs/adr/0010-licenciamento.md))
- CI: verificação de documentação ativa; matriz Rust das três plataformas e
  workflow de release escritos e dormindo até existir `Cargo.toml`
- `scripts/verify-docs.py`: links, TOML, cores sem origem na especificação
  visual e cobertura da tabela de fases

- [PRD-010](docs/prd/prd-010-interacao-e-superficie-de-app.md): consolida como
  requisito o comportamento visível ao usuário que os ADR-0013 a 0016 haviam
  decidido — mouse, seleção, clipboard, rolagem, avisos, diálogos, menus de
  contexto e janelas. Não decide nada novo: dá procedência de PRD ao que só
  tinha procedência de ADR, e fecha a métrica do PRD-004 de zero chaves de
  configuração sem requisito
- [ADR-0016](docs/adr/0016-fontes-embutidas.md): as cinco faces do design
  embutidas no binário, sem o que o critério *"o binário com a config padrão
  bate com o mockup"* seria inalcançável em máquina limpa
- [Fronteira de `porecatu-term`](docs/arquitetura.md) especificada: forma do
  snapshot de grade, quem lê a config do terminal e por onde o OSC 52
  atravessa para o clipboard
- Job canário do [ADR-0011](docs/adr/0011-toolchain-rust.md) no `ci.yml`,
  dormindo junto com a matriz

### Alterado

- Actions do GitHub pinadas por SHA de commit em vez de tag major flutuante,
  com a versão legível no comentário. Tag pode ser reapontada e uma release
  menor muda comportamento sem PR do Dependabot — mesma disciplina que o
  [ADR-0011](docs/adr/0011-toolchain-rust.md) aplica à toolchain Rust

### Corrigido

- Deadlock ao fechar a janela no Windows: `ClosePseudoConsole` esperava o pipe
  de leitura clonado ser liberado enquanto a thread de leitura estava parada
  num `read()` síncrono nele, e o app só morria por kill externo. O terminal
  passa a só matar o processo e deixar o SO reclamar as handles no fim
- Cor de fundo saindo lavada: a surface escolhia um formato `*Srgb` por
  default e a GPU reaplicava a curva sobre valores que já vinham em espaço
  sRGB. Corrigido com `remove_srgb_suffix()` no formato da surface
- Etimologia de "Porecatu" no README: do tupi, **"salto bonito"**
