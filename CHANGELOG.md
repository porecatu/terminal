# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).
Versionamento seguirá [SemVer](https://semver.org/lang/pt-BR/) a partir do
primeiro release.

> **Nenhuma versão foi publicada ainda.** As fases F0, F1, F2 e F3 do
> [roadmap](docs/roadmap.md) estão implementadas — a F3 exceto o RF-2.21
> (`group.next`/`group.prev`), que a mantém aberta. `cargo run` abre uma janela
> com abas e grupos de terminal funcionais, e `Ctrl+Shift+N` abre uma segunda
> janela; artefatos de release só aparecem na F6.

## [Não publicado]

### Adicionado

#### F3 — Grupos

O diferencial do produto. Entregue em seis etapas, uma por PR, mais quatro PRs
de correção. Ainda não há configuração nem sessão: os valores de aparência
seguem como constantes citando a chave TOML de origem. **O RF-2.21
(`group.next`/`group.prev`) não foi implementado** — o MRU por grupo está
gravado, falta a operação que anda de grupo em grupo.

- **Decisões que a fase exigia**, escritas antes de ela abrir:
  [ADR-0020](docs/adr/0020-grupos-explicitos.md) (grupos explícitos — grupo
  implícito deixa de ser único, colapso ganha ordem navegável própria, escada de
  foco, regra de repetição da paleta de seis cores),
  [ADR-0021](docs/adr/0021-selecao-multipla-e-gestos-da-barra.md) (seleção
  múltipla e gestos da barra — estado efêmero de janela, `Cmd` no macOS,
  fronteira do arraste entre grupos),
  [ADR-0022](docs/adr/0022-animacao-de-interface.md) (animação sob render
  damage-driven — relógio por janela, dois consumidores, lista fechada) e
  [ADR-0023](docs/adr/0023-editor-de-grupo.md) (editor de grupo, o quinto widget
  de chrome)
- `porecatu-core`: `GroupColor`/`GroupKind`/`GroupMeta`, **N runs implícitos**
  (um por trecho contíguo de abas sem grupo) mantidos por `normalize_groups`
  depois de toda operação estrutural, `navigable_order()` ao lado de
  `visual_order()`, MRU por grupo, escada de foco de quatro níveis numa função
  só. Operações novas: `group_tabs`, `ungroup`, `rename_group`,
  `set_group_color`, `collapse_group`, `next_auto_color`, `move_tab_to_group`,
  `move_tab_to_group_at`, `move_tab_to_new_run`, `move_group`
- Seleção múltipla (`selection.rs`), fora do core: `Ctrl`/`Cmd`+clique alterna,
  `Shift`+clique estende sobre a ordem navegável, clique sem modificador limpa e
  ativa, `Esc` limpa. Fechar ou colapsar reposiciona a âncora; no macOS
  `Ctrl`+clique na barra abre o menu em vez de tocar a seleção
- Pílula de grupo na geometria (swatch, nome truncável, contador, caret),
  cápsula de cor por trás das abas do grupo e sublinhado da aba resolvido pela
  cor do grupo — `ungrouped_color` para as abas soltas
- Colapso ponta a ponta: a trilha para de gerar geometria para as abas do grupo
  colapsado, `next_tab`/`prev_tab` e `Alt+1..9` passam a andar sobre a ordem
  navegável (colapsar renumera, deliberadamente), e a pílula ganha o indicador
  agregado — campainha vence atividade, aba `Exited` não contribui
- Editor de grupo, menu de contexto de grupo e popover de destino do
  `tab.move_to_group`, os três lendo a **mesma** lista de seis ações
  (`group_menu.rs`, RF-10.21). Nome editado ao vivo sem escrever no `Workspace`
  até o `Enter`; `group.close_all` sempre confirma, com a contagem no corpo do
  diálogo. Os cinco popovers nunca coexistem
- Arraste de aba **entre** grupos, com realce de fronteira, e arraste da pílula
  movendo o grupo inteiro — nunca para dentro de outro grupo, porque grupos não
  aninham. O gesto continua sem tocar o `Workspace` real até a soltura
- Animação de reflui (`animation.rs`): relógio por janela dirigido pelo
  `ControlFlow::WaitUntil`, ativo só enquanto há movimento pendente. Dois
  gatilhos — formar grupo (`.18s`) e colapsar/expandir (`.15s`). Interpola
  posição do wrapper, **largura da cápsula** e **opacidade** das abas que entram
  ou saem da trilha; o `Workspace` nunca é interpolado
- `Ctrl+Shift+G` agrupa a seleção corrente, ou a aba ativa quando não há
  seleção, com cor automática e o editor aberto no nome
- Botão "+" ao final de cada grupo (`group.new_tab`), governado por
  `show_new_tab_button`, com a cor do ícone decidida pelo que está atrás dele:
  escuro sobre a cápsula de cor de um grupo, claro sobre a barra num run de
  abas soltas, e sumindo quando o grupo está colapsado — o wrapper colapsado é
  a pílula e mais nada. Ao fim da trilha, um "+" que cria aba **fora de todo
  grupo**, presente só quando o último grupo é explícito (senão duplicaria o do
  run solto): é ele que cobre o caso de toda aba estar em grupo. O botão de
  nova aba **global** chegou a existir numa zona fixa
  à direita e foi removido — era um segundo botão para a mesma ação a um palmo
  do primeiro. A zona ficou, com o botão de configurações, inerte até a F4
- Botão de nova aba global cria a aba **fora de qualquer grupo explícito**, no
  fim da barra (`Workspace::append_ungrouped_tab`) — antes ele seguia o grupo
  da aba ativa, que é o comportamento de `tab.new` (RF-1.1) e deixava o botão
  sem jeito de criar aba desagrupada. O atalho não mudou
- Aba com **largura fixa** (`TabBarStyle::tab_width`), derivada dos mesmos
  tokens da §2.5 da especificação: título, indicador e renomeação deixam de
  refluir a trilha
- **Face de ícones embutida** ([ADR-0024](docs/adr/0024-face-de-icones.md)):
  Lucide sob licença ISC, em `FontFace::Icon`, com os codepoints em uso
  nomeados em `porecatu_render::icon`. Corrige os ícones do chrome, que não
  desenhavam — `✕`, `▶` e `▼` não existem na IBM Plex Sans e o `fontdb` do
  projeto não carrega fonte do sistema, então não havia fallback nenhum.
  A em dos ícones é 20 px: o Lucide desenha ~0.6 dela, e nos tamanhos que a
  especificação cita o traço fica sub-pixel e esmaece contra o fundo
- Acabamento da barra pedido pelo usuário: o sublinhado de grupo sai da base
  da aba (redundante desde que a cápsula virou cor cheia), a borda da aba vai
  a 2px (1px não se lia contra a cápsula), o nome do grupo passa a usar a
  mesma fonte do rótulo da aba (12.5px/400, contra os 12px/500 da espec.), os
  ícones passam a usar em repouso o `#e4e8ee` que a espec. reservava ao hover
  (o traço fino do Lucide sumia num cinza médio) — exceto o "+" de dentro do
  grupo, que vai para `#12151a` por ser o único sobre a cápsula de cor —, a
  aba sobe para 34px de altura e a
  trilha ganha 6px de respiro das bordas da barra (`trilha_padding`) — os
  mesmos 6px que a espec. §2.5 já dava à barra. Aba solta passa a ocupar a
  caixa inteira do wrapper, alinhando topo e base com o bloco de um grupo;
  dentro de um grupo ela continua cedendo `wrapper_padding` ao bloco. Junto,
  um bug que só o respiro revelou: `chrome::paint` recalculava a altura da
  barra por conta própria, e o recorte da trilha, curto, cortava as abas
  antes do respiro de baixo aparecer
- **Cadeia de fallback de fonte ligada** — o `fontdb` passa a carregar as
  faces do sistema **depois** das embutidas, que é o que o ADR-0016 sempre
  exigiu e a implementação nunca teve. Sem ela, todo codepoint fora das
  faces embutidas simplesmente não desenhava: era o caso do braille dos
  gráficos do `btop` (a IBM Plex Mono não cobre um só dos 256), dos
  geométricos e dos dingbats. A ordem preserva a precedência do design
  para "IBM Plex Mono"/"IBM Plex Sans"
- A grade quebra o `TextRun` em qualquer caractere cujo avanço não seja o da
  célula e o desenha ancorado no `x` dela, encolhido para caber: glyph de
  fallback avança o que a fonte dele mandar (1.26 célula num braille) e num
  run compartilhado empurrava o resto da linha. Linha de ASCII continua
  sendo um run só; o avanço por caractere é cacheado
- **Iosevka no lugar da IBM Plex** ([ADR-0025](docs/adr/0025-iosevka-no-lugar-da-ibm-plex.md)):
  Iosevka Fixed 400/500 no terminal, Iosevka Aile 400/500/600 no chrome. A
  Plex Mono não cobre braille (0/256 — os gráficos do `btop`), formas
  geométricas (1/96), dingbats nem powerline; a Iosevka cobre os quatro
  inteiros, no avanço da célula. Escolhida a variante **Fixed** por não ter
  ligaduras: substituição de dois glyphs por um sairia da grade, e a
  verificação de avanço do `paint.rs` é por caractere, não pegaria.
  Google Sans Code foi medida e descartada — cobre menos que a Plex Mono
- `scripts/subset-fonts.py` recorta as faces para os blocos que o projeto
  desenha: 48 MB viram 2.1 MB. Permitido porque a OFL da Iosevka não tem
  cláusula de Reserved Font Name, ao contrário da IBM Plex
- 261 testes no workspace, contra os 145 da F2

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
  ([ADR-0016](docs/adr/0016-fontes-embutidas.md), hoje superado pelo
  [ADR-0024](docs/adr/0024-face-de-icones.md)), com a OFL e a atribuição em
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

- `Workspace::group_tabs` invertia a ordem visual ao agrupar a partir de um
  grupo explícito quando a aba extraída não era a primeira dele. Bagunçava
  `self.groups` em silêncio, e era a causa real do relato de que a animação de
  colapso "só funcionava no primeiro grupo": o grupo errado recebia a geometria
  antiga
- A animação de colapso não animava o **próprio** grupo — as abas dele sumiam na
  hora (agora esmaecem) e a cápsula saltava para a largura final (agora
  interpola largura, não só posição). Só os vizinhos deslizavam
- Barra lenta com overflow: `fit_width` fazia até 24 recálculos completos da
  trilha por frame, cada um remedindo o texto de toda aba sem cache. O
  encolhimento de rótulo e de nome de pílula foi **descartado** — os dois ficam
  no teto e a trilha rola como um componente só; as chaves `min_width` e
  `label_min_width` saíram do arquivo de exemplo
- `group.create` (RF-2.4/RF-2.5) tinha modelo e testes, mas nenhum gesto de UI
  em seis etapas. Ganhou `Ctrl+Shift+G`, nome default "Novo grupo" e cor
  automática
- Deadlock ao fechar a janela no Windows: `ClosePseudoConsole` esperava o pipe
  de leitura clonado ser liberado enquanto a thread de leitura estava parada
  num `read()` síncrono nele, e o app só morria por kill externo. O terminal
  passa a só matar o processo e deixar o SO reclamar as handles no fim
- Cor de fundo saindo lavada: a surface escolhia um formato `*Srgb` por
  default e a GPU reaplicava a curva sobre valores que já vinham em espaço
  sRGB. Corrigido com `remove_srgb_suffix()` no formato da surface
- Etimologia de "Porecatu" no README: do tupi, **"salto bonito"**
