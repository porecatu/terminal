# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).
Versionamento seguirá [SemVer](https://semver.org/lang/pt-BR/) a partir do
primeiro release.

> **Nenhuma versão foi publicada ainda.** As fases F0 a F5 do
> [roadmap](docs/roadmap.md) estão **fechadas**; a F6 (polimento) está
> **aberta** — [PRD-011](docs/prd/prd-011-polimento.md) e ADR-0041 a 0044
> escritos, implementação não iniciada. `cargo run` abre uma janela com abas e
> grupos de terminal funcionais, sem decoração nativa fora do macOS,
> `Ctrl+Shift+N` abre uma segunda janela, o arquivo de config (`porecatu.toml`)
> governa aparência, teclas e temas com recarga a quente, e a sessão volta como
> estava ao reabrir. Artefatos de release saem na F6, na versão `1.0.0`
> ([ADR-0044](docs/adr/0044-empacotamento-e-release.md)).

## [Não publicado]

### Adicionado

#### Abertura da F6 — polimento

- **[PRD-011](docs/prd/prd-011-polimento.md)**, o requisito que a fase não tinha:
  a F6 era a única fase do v1 sem PRD, e as ações `search.*` do catálogo tinham
  origem "roadmap" — contra a métrica do PRD-010 (*"ações do catálogo sem origem
  em RF ou ADR: zero"*). RF-11.1 a RF-11.30, e ao fechar a lacuna o PRD trouxe
  para dentro da fase **sete requisitos aprovados de fases anteriores que nunca
  foram entregues** e não estavam em lista nenhuma (RF-11.24 a RF-11.30)
- **[ADR-0041](docs/adr/0041-busca-no-scrollback.md)** — busca no scrollback:
  barra **sobreposta** ao topo do quadro do terminal (empurrar mandaria `resize`
  ao PTY), camada `Chrome` sem camada nova (responde a pergunta que o ADR-0018
  deixou escrita), captura de teclado **parcial** — a primeira superfície não
  modal do app —, ocorrências como lista de ranges em vez de bit em `CellFlags`,
  e a anatomia da §2.21 sem um valor ou ícone novo
- **[ADR-0042](docs/adr/0042-hyperlinks-osc-8.md)** — hyperlinks OSC 8: o URI
  viaja como span ao lado do snapshot, então `Cell` não muda um byte e continua
  `Copy`; `Ctrl`+clique (`Cmd` no macOS, mesma razão do ADR-0021); quatro
  esquemas aceitos, com **`file` revelado no gerenciador de arquivos e nunca
  entregue ao handler por extensão** — o URI vem da saída de um programa
- **[ADR-0043](docs/adr/0043-arvore-de-acessibilidade.md)** — `accesskit` sobre o
  chrome, com a árvore como **projeção das funções puras de layout**, construída
  só com leitor de tela ativo e nunca dentro do caminho de render. Paga a dívida
  registrada no ADR-0001 antes da F1, e corrige a conta: são **cinco** papéis de
  widget, não três. A grade do terminal fica declaradamente fora do v1
- **[ADR-0044](docs/adr/0044-empacotamento-e-release.md)** — instalador nativo
  por plataforma, sem assinatura de código (o número da primeira release, § 3,
  foi revisto pelo [ADR-0045](docs/adr/0045-primeira-versao-0-7-0.md): **`0.7.0`**,
  não `1.0.0` — dívida de verificação ainda em aberto no fechamento da F6). Traz
  `x86_64-apple-darwin` para a matriz, `--locked` para o `ci.yml` (última
  pendência aberta da F0) e a **atribuição das fontes embutidas para dentro do
  artefato** — hoje o `release.yml` copia só `LICENSE` e `README.md`, e publicar
  assim descumpriria a OFL e a ISC

#### Fechamento da F5 — persistência de sessão

Implementa [PRD-003](docs/prd/prd-003-persistencia-de-sessao.md) (RF-3.1 a
RF-3.17), em seis etapas, atrás dos cinco ADRs que abriram a fase
([ADR-0036](docs/adr/0036-formato-do-arquivo-de-sessao.md) a
[ADR-0040](docs/adr/0040-superficie-de-linha-de-comando.md)).

- **`porecatu-session` nasce** (etapa 1): `path.rs` com `PORECATU_SESSION` →
  diretório de **estado** da plataforma (deliberadamente diferente do da
  config), `schema/v1.rs` com o DTO versionado do ADR-0036, conversão nos dois
  sentidos, escrita `tmp` → `fsync` → `rename` e a tabela de recuperação
  inteira. Sem consumidor ainda, como a etapa 1 da F4. O teste que justifica ter
  escolhido DTO em vez de serde do domínio reprova quando um campo novo de
  `porecatu-core` não foi classificado como gravado ou descartado
- **Gravação fiada na UI** (etapa 2): debounce por `ControlFlow::WaitUntil`
  (`SessionScheduler`), por **processo** e não por janela (RF-3.17: um arquivo
  para todas), com a janela marcada suja por dois pontos únicos em
  `WindowState`. A gravação síncrona no exit preenche o no-op documentado que a
  F2 plantou. Inclui a correção do `file:///C:/...` em `parse_file_uri`
  (`strip_windows_drive_leading_slash`) — é aqui que o `cwd` de OSC 7 passa a ir
  para o disco, e caminho inválido gravado é pior que não gravado
- **`TabState::NotStarted`** (etapa 3, [ADR-0037](docs/adr/0037-aba-nao-iniciada.md)):
  o terceiro estado que a restauração preguiçosa exige, com o shell subindo no
  primeiro foco por um ponto único (`App::ensure_active_tab_started`). Rótulo
  esmaecido do RF-3.9 com alfa `.45`, nenhum valor novo. Dois bugs reais achados
  no cruzamento com o ADR-0034: fechar aba `NotStarted` não fazia nada, e
  `request_close_window` confirmaria por contagem numa janela sem nada a perder
- **Restauração no start** (etapa 4): `App::resumed` carrega a sessão e abre uma
  janela por `WindowV1`; aba ativa nasce `Running`, as outras `NotStarted` com
  `lazy_restore`. Geometria e monitor por nome → posição → primário
  (RF-3.11), `cwd` inexistente com nota no grid (RF-3.10), avisos de recuperação
  na barra do ADR-0014. **RF-2.17 fecha ponta a ponta** de graça: ativar a aba
  certa no fim da reconstrução expande o grupo que a contém
- **Linha de comando e fallback de `cwd`** (etapa 5,
  [ADR-0040](docs/adr/0040-superficie-de-linha-de-comando.md) e
  [ADR-0038](docs/adr/0038-fallbacks-de-cwd.md)): `src/cli.rs` com parsing à mão
  sobre `OsString` — `--config`, `--help`, `--version` e caminho posicional, que
  cria sessão nova sem ler nem sobrescrever a gravada (RF-3.12). Fecha a dívida
  do `--config` da etapa 1 da F4. E `ProcessGroup::cwd()` por `sysinfo` no Linux
  e no macOS, sem dependência nova; Windows segue sem fallback, por erro de
  compilação e não por decisão em runtime
- **Convite à integração de shell, tema e zoom** (etapa 6,
  [ADR-0039](docs/adr/0039-convite-a-integracao-de-shell.md)): a nota do RF-3.1
  entra por `inject_note`, com dois gatilhos de detecção que convergem num ponto
  único — fallback de `cwd` fora do Windows, temporal de 3s no Windows. Snippets
  embutidos de [integracao-de-shell.md](docs/reference/integracao-de-shell.md)
  por `include_str!`. A **dispensa definitiva é digitada no terminal** e
  reconhecida por um segundo parser sobre o eco do PTY, mesmo padrão do
  `Osc7Watcher`. Tema e zoom de sessão passam a restaurar, não só gravar

### Dívida da F5

- **Verificação interativa**, mesma limitação de F1 a F4 (ver a nota no topo do
  [roadmap](docs/roadmap.md)). Sete dos oito cenários de aceite do PRD-003 rodam
  sem gesto de teclado; os dois que pedem foco real são "o shell de uma aba
  inicia quando ela é focada" — verificado por **clique sintético**, que
  atravessa a proteção de foco do Windows — e dispensar o convite do RF-3.1, que
  é digitação e continua bloqueado
- **Métrica de 20 abas em menos de 1 s não foi medida.** A verificação da etapa 4
  foi interrompida a pedido do usuário. A etapa 6 da F6 instrumenta e mede
- **Aparência da nota do convite** (posição, cor `#5ed3bc`, snippet renderizado)
  não confirmada por captura de tela, e a dispensa digitada não exercitada ao
  vivo — as duas cobertas só por teste automatizado
- **Fallback de `cwd` no macOS** verificado por compilação e CI, nunca em máquina
  real; no Linux o teste de integração passou de verdade no CI
- **Dívida Unix do ADR-0033** segue: sem `setsid`/`killpg`,
  `ProcessGroup::process_count`/`kill_tree` continuam degradados fora do Windows

#### Fechamento da F4 — configuração

- **`porecatu-config`** (etapa 1): structs `serde`, defaults completos batendo
  com [`porecatu.example.toml`](docs/config/porecatu.example.toml), resolução
  de caminho (`--config` → `PORECATU_CONFIG` → padrão da plataforma), erro
  localizado (linha/coluna), chave desconhecida como aviso
- **`porecatu-ui` lê `Config`** (etapa 2): chrome e barra de abas saem de
  `const`, com a config padrão reproduzindo exatamente o binário de antes
- **Terminal lê `Config`** (etapa 3): fonte, cores, cursor, scrollback,
  seleção, clipboard, `[general] startup_directory`, `[shell]`
- **Hot reload** (etapa 4, [ADR-0030](docs/adr/0030-escopo-do-hot-reload.md)):
  `notify` assistindo o diretório do arquivo, debounce de ~200ms, três classes
  de chave (aplica a quente / aplica com recálculo de grade e resize de PTY /
  exige reinício e avisa)
- **`enum Action` + parser de `[keybindings]`** (etapa 5,
  [ADR-0029](docs/adr/0029-enum-de-acao-e-gramatica-de-tecla.md)): 46 ações do
  [catálogo fechado](docs/reference/acoes.md), gramática de tecla com
  canonicalização, resolução em três níveis (embutido → comum → plataforma) —
  os defaults de macOS respondem pela primeira vez
- **Temas nomeados, zoom por atalho, `animations = false`, roda do mouse no
  popover de destino, e as duas mudanças visuais aprovadas**
  ([ADR-0031](docs/adr/0031-temas-nomeados.md),
  [ADR-0032](docs/adr/0032-interface-do-v1-fechada.md), etapa 6): hover por
  brilho (`1.18` na aba, `1.25` na pílula) e sombra em camadas nos cinco
  widgets de chrome e no fantasma de arraste, ambos resolvidos em CPU sem
  primitiva nova em `porecatu-render`
- **Merge de tema cobre toda a superfície do ADR-0031 §1** (fora de fase,
  depois do fechamento da etapa 6): `[appearance.groups]` (`palette` inclusa,
  substituída inteira pelo caso especial do ADR-0031 §2) e as cores dos cinco
  widgets de chrome entram no merge junto das 16 ANSI e dos dez campos
  nomeados de terminal/abas

### Dívida da F4

- `zoom_scope = "active"` não tem efeito -- o zoom por atalho é sempre do
  processo inteiro, nunca só da aba ativa
- Entrada de cor por hexadecimal no editor de grupo (RF-2.10) não foi
  implementada

#### Fechamento da F3 — navegação de grupo

- **`group.next` / `group.prev` (RF-2.21)**, o item que mantinha a fase aberta.
  `Workspace::step_group` anda de grupo em grupo na ordem visual, circulando, e
  ativa **a última aba visitada** do destino — o `Group::last_active` que estava
  gravado desde a primeira etapa da fase e não tinha nenhum consumidor fora dos
  testes. Grupo colapsado é pulado (navegar não expande nada) e grupo vazio
  também; sem `last_active`, cai na primeira aba do grupo
  ([ADR-0020](docs/adr/0020-grupos-explicitos.md) §6). Run implícito conta como
  destino, senão "voltar para as abas soltas" não teria gesto. Teclas
  `Ctrl+Shift+PageDown` / `Ctrl+Shift+PageUp`, exigindo `Ctrl` **e** `Shift`
  para não colidir com a rolagem de scrollback
- **Atalhos do nível de grupo** que faltavam na cadeia do
  [ADR-0008](docs/adr/0008-teclas-e-roteamento-de-input.md): `Ctrl+Shift+U`
  (`group.dissolve`), `Ctrl+Shift+E` (`group.rename`) e `Ctrl+Shift+K`
  (`group.toggle_collapse`) — antes só existiam por menu, editor ou clique na
  pílula. Despacham pelo mesmo `run_group_action` do menu, então tecla e menu
  não divergem. O alvo é o grupo da aba ativa, resolvido por
  `group_menu::keyboard_target`, que devolve `None` sobre um run implícito: o
  que o menu mostra esmaecido (RF-10.20), a tecla trata como no-op
- **RF-2.17: ativar aba de grupo colapsado expande o grupo.** Em
  `Workspace::activate_tab` — o roadmap afirmava que a regra estava no modelo, e
  não estava. Nenhum caminho da F3 ativa aba oculta (as duas fontes que o
  requisito cita são busca, F6, e restauração de sessão, F5), então isto entra
  como invariante para que o primeiro desses caminhos não tenha de redescobri-la

#### Depois da F3

Ajustes de interface e de infraestrutura pedidos com o app em tela, fora do
recorte de fase. A interface resultante é o alvo desde o
[ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md).

- **Janela sem decoração nativa fora do macOS**
  ([ADR-0027](docs/adr/0027-controles-de-janela-e-resize-proprios.md)): drag
  region na área vazia da barra de abas com duplo clique maximizando, três
  botões de janela de 46px colados na borda direita (Lucide `minus`/`square`/
  `copy`/`x`, com hover e o destrutivo do fechar) e resize de 6px em toda borda,
  desligado com a janela maximizada. Fechar continua passando pelo diálogo de
  confirmação. No macOS a decoração nativa fica, e a trilha reserva 78px à
  esquerda para o semáforo
- **Ícone do app**: PNG embutido decodificado em runtime para toda janela
  (`app_icon.rs`, crate `png`) e o `.ico` embutido como recurso PE no Windows
  por um `build.rs` com `winres`. Era item da F6; entregou-se antes
- **Efeito de vidro** na cápsula e na pílula de grupo: alfa de `.85` e `.92` na
  cor cheia, mais um rim translúcido de 1px em branco a `.16` (`GLASS_BORDER`).
  Sem primitiva de blur, não há como turvar o que passa por trás — só deixar
  passar menos dele, e ainda assim lê como painel translúcido em vez de chapado.
  Custo zero de render: troca de cor e alfa nos quads que já eram desenhados
- **Sombra em camadas** (`chrome::push_shadow`): três `RoundedQuad` pretos
  empilhados, spread crescente e alfa decrescente, na cápsula de grupo, na aba
  solta e no quadro do terminal. É a aproximação possível sem passo de blur; os
  cinco widgets de chrome e o fantasma de arraste seguem sem sombra
- **Borda de 1px** na cápsula de grupo e na aba solta; aba dentro de um grupo
  fica só com a borda dela, porque a cápsula carrega a sombra por ela

#### F3 — Grupos

O diferencial do produto. Entregue em seis etapas, uma por PR, mais quatro PRs
de correção e o PR de fechamento (acima). Ainda não há configuração nem sessão:
os valores de aparência seguem como constantes citando a chave TOML de origem.
O RF-2.21 (`group.next`/`group.prev`) atravessou as seis etapas **sem
implementação** — o MRU por grupo ficou gravado sem consumidor — e foi o
primeiro item do PR de fechamento.

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
  a pílula e mais nada — mas a cápsula de cor do grupo continua desenhada
  colapsada, abraçando a pílula. Os botões de ícone ganham respiro horizontal
  (`icon_button_padding_x`), ficando mais largos que altos. Ao fim da trilha,
  um "+" que cria aba **fora de todo
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

- `png` de 0.17 para 0.18, com a adaptação de `app_icon.rs`: `Decoder::new`
  passou a exigir `BufRead + Seek` e `output_buffer_size` devolve `Option`
- `[appearance.tabs] font_size` de 12.5 para 13 — rótulo da aba e nome da
  pílula do grupo, que segue o mesmo **tamanho** mas diverge no **peso** (500,
  para o rótulo do grupo ler como bold; pedido do usuário). Itens de
  menu continuam em 12.5, token separado (`overlay.rs::MENU_ITEM_TEXT_SIZE`)
- Actions do GitHub pinadas por SHA de commit em vez de tag major flutuante,
  com a versão legível no comentário. Tag pode ser reapontada e uma release
  menor muda comportamento sem PR do Dependabot — mesma disciplina que o
  [ADR-0011](docs/adr/0011-toolchain-rust.md) aplica à toolchain Rust

### Corrigido

#### Documentação, na abertura da F6

Dívida **já paga** que continuava registrada como pendente, mais dois erros de
contagem. Achado ao auditar a documentação contra o código:

- `adr/0032` dizia "os quatro seguem previstos" sobre defaults de macOS,
  `animations = false`, roda do mouse no popover e cor hexadecimal — **três dos
  quatro** foram entregues na F4. Mesma correção na `Dívida da F3` do roadmap e
  em três caixas de estado do catálogo de ações
- `app.quit` estava listada como "sem gesto": está implementada e ligada a
  `Cmd+Q` no macOS desde a etapa 5 da F4; não ter default em Windows/Linux é
  deliberado. O que **realmente** não responde é `scrollback.to_top`/`to_bottom`
  — elas têm default embutido e entram no mapa resolvido, mas o `match` de
  despacho as devolve como não tratadas (agora RF-11.30)
- `parse_file_uri` estava descrito em tempo futuro ("corrigir na etapa 2")
  depois de corrigido
- `roadmap` dizia que o `accesskit` cobre "os três widgets"; são cinco
- `porecatu.example.toml` marcava RF-1.20/RF-1.21 como "sem desenho aprovado
  ainda", desenhados desde a §2.17 — e o `adr/0032` §2 é explícito de que
  documento assim *"está errado por definição"*
- A tabela de ADRs não marcava como **parcial** o supersede do ADR-0025
- `CLAUDE.md` dizia "mais de vinte entradas" na §4.4 da especificação visual;
  são 47

- Colapsar um grupo poderia ter passado a ser no-op com a regra do RF-2.17: a
  escada de foco do RF-1.5 move o foco para fora ao colapsar, e se ela
  devolvesse uma aba do **próprio** grupo, `activate_tab` o expandiria de volta.
  Ela não devolve — pula grupo colapsado e começa depois do índice dele —, e há
  teste de regressão fixando isso
- **Clique preso em qualquer app que peça mouse tracking** (`btop4win`, o
  Claude Code CLI): `dispatch_mouse_input` retornava cedo em todo release, então
  `input::handle_mouse_button` nunca era chamado com `pressed = false` — o
  programa recebia o `M` do press (SGR) e nunca o `m` do release, em modo nenhum.
  Regressão introduzida na etapa 6 da F2. O estado de botão apertado passa a ser
  zerado sempre no release e em `Focused(false)`, para alt-tab com o botão
  físico apertado não deixá-lo preso
- **Aba nova abria no diretório do binário**: `startup_directory` usava
  `std::env::current_dir()`, que é de onde o Porecatu foi lançado, não o
  diretório do usuário. Passa a ser `dirs::home_dir()` quando não há `cwd`
  conhecido por OSC 7 ([ADR-0017](docs/adr/0017-ciclo-de-vida-da-aba.md))
- **Vão maior, e sem cor, entre um grupo e as abas soltas ao lado do que entre
  dois grupos**: o `wrapper_padding` entrava também no run implícito, que não
  tem cápsula para absorvê-lo. Agora só entra onde há cápsula
- **Indicador de abas fora da vista** virou círculo de 18×18 com só o chevron,
  no lugar da cápsula de 34×18 com chevron e contagem — a cápsula lia como
  comprida demais para o que informa (pedido do usuário)
- Cursor do terminal invisível: `frame::GeometryBatch` guardava `Quad` e
  `RoundedQuad` em dois `Vec` separados, e a montagem de instâncias sempre
  desenhava todo `rounded` depois de todo `quads` do batch — não importa a
  ordem de chegada. O quadro arredondado do terminal (`RoundedQuad`, mesmo
  clip do cursor) acabava sempre por cima dele. `geometry` agora é um só
  `Vec<GeometryPrimitive>` ordenado, e a mesma armadilha valeria para
  qualquer fundo de célula colorido (seleção, `ls --color`), não só o
  cursor
- Cursor do terminal alto e baixo demais: desenhava com a altura de
  `metrics.height` (a altura de **linha**, com os 1.75 de
  `LINE_HEIGHT_MULTIPLIER` de folga) a partir do topo dela, sobrando bem
  abaixo do glyph — visível assim que parou de ficar escondido atrás do
  quadro do terminal (item acima). Altura passa a ser
  `font_size_px * 1.2`, a mesma proporção do `.caret-blk` do mockup
  (15/12.5) e do line-height que `text.rs` usa pra montar a caixa do
  glyph (`Metrics::new(size_px, size_px * 1.2)`), colado no topo da linha
  como o glyph também está
- Janela abrindo um console junto no Windows: faltava
  `#![windows_subsystem = "windows"]` em `src/main.rs`, então o binário
  ficava no subsistema `console` default e o Windows abria um terminal ao
  lado da janela. Sem efeito nos outros alvos — a diretiva é ignorada fora
  de `windows`
- Render do terminal e troca de aba ficaram lentos: `fits_the_grid` (o teste que
  decide se um caractere pode viajar num `TextRun` compartilhado) comparava o
  avanço **natural** do caractere contra a largura de célula já **arredondada ao
  pixel físico** por `snap_cell_metrics_to_pixel_grid`. As duas diferem por até
  meio pixel, dez vezes a tolerância, então toda célula reprovava: cada
  caractere virava um `TextRun` próprio com um shaping sem cache cada, e a grade
  inteira era re-shapada por frame — ~4000 operações de `cosmic-text` numa grade
  80x24, contra ~24. O teste agora é em **em**, contra o avanço do `'M'` da
  própria face mono, e não depende mais de tamanho de fonte nem de escala de
  janela. Os testes de `paint.rs` não pegaram porque montavam a célula sem o
  arredondamento do runtime; há um teste novo que exercita as cinco escalas
- `TextMeasurer::truncate` media cada prefixo candidato — um `Buffer` novo e um
  `shape_until_scroll` por caractere, mais um `String::clone` — e roda por aba a
  cada layout da barra, isto é por frame e a cada movimento do mouse. Passou a
  ser **um shaping**, com o corte saindo do avanço acumulado dos glyphs. O
  caminho ficou latente até a largura de aba virar fixa, quando todo título mais
  longo que o teto passou a cair nele
- `fitted_size` chamava `measure_width` sem cache para cada caractere de
  fallback, por frame — uma tela de braille do `btop` são centenas de shapings.
  Caractere único agora resolve pelo cache de `advance_em`; só cluster mede
- `terminal.font.size` de 13.0 para 14.0. A Iosevka Fixed avança 0.5 em em todo
  glyph, então `size / 2 * scale` precisa cair em pixel inteiro para a célula
  não ser arredondada; a 13 o avanço era 6.5 numa célula de 7.0. A largura de
  célula não muda (a contagem de colunas é a mesma), o glyph é que passa a
  preencher a célula
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
