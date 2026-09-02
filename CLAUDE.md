# CLAUDE.md

Guia operacional do projeto. Leia antes de mexer em qualquer coisa.

## O que é

**Porecatu** — emulador de terminal cross-platform em Rust. Diferencial: gestão de muitos terminais (abas, grupos nomeados, sessão persistente), não conformidade VT.

**Estado atual: F0, F1, F2 e F3 fechadas.** A verificação interativa das três fases é **dívida assumida**, não critério pendente: a proteção de foco do Windows bloqueia input sintético e não há ambiente Linux/macOS no fluxo, então elas fecharam com cobertura automatizada mais smoke test (ver a nota no topo do roadmap). `cargo run` abre uma janela com abas de terminal funcionais e **grupos**: PTY, motor VT, render por GPU, teclado, mouse, seleção, clipboard, ciclo de vida de aba, overflow da barra, arraste, seleção múltipla, pílula e cápsula de cor, colapso, editor de grupo, menu de contexto de grupo, popover de destino, arraste entre grupos e do grupo inteiro, animação de reflui, tooltip, aviso e diálogo de confirmação — e `Ctrl+Shift+N` abre uma segunda janela. Fora do macOS, a janela também perdeu a decoração nativa: drag region e botões de janela na própria barra de abas, resize por toda borda ([ADR-0027](docs/adr/0027-controles-de-janela-e-resize-proprios.md), pedido do usuário, fora de fase). `porecatu-config` e `porecatu-session` ainda são stubs (só o cabeçalho SPDX); ganham corpo nas F4/F5, e as três decisões que destravam a F4 já estão escritas ([ADR-0029](docs/adr/0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0030](docs/adr/0030-escopo-do-hot-reload.md), [ADR-0031](docs/adr/0031-temas-nomeados.md)). Enquanto `config` não existe, valor de aparência entra como constante citando no comentário a chave TOML de origem. O binário tem ícone próprio, entregue fora de fase: `app_icon.rs` decodifica um PNG embutido em runtime (crate `png`) e um `build.rs` com `winres` embute o `.ico` como recurso PE no Windows; `dirs` entrou junto, para o diretório home ser o `startup_directory`. Antes de mexer, leia [docs/arquitetura.md](docs/arquitetura.md) e os ADRs — o que está em código segue o que está escrito lá, incluindo os desvios anotados.

**A F3 fechou com o PR de navegação de grupo:** `Workspace::step_group` (RF-2.21) andando de grupo em grupo pela última aba visitada de cada um, pulando colapsado e vazio; `Ctrl+Shift+PageDown`/`PageUp` para ele e `Ctrl+Shift+U`/`E`/`K` para `group.dissolve`/`rename`/`toggle_collapse`, com o alvo por tecla em `group_menu::keyboard_target` (`None` sobre run implícito, que é o que o menu mostra esmaecido); e a regra do RF-2.17 em `activate_tab`, que o roadmap afirmava estar no modelo e **não estava**. A próxima fase é a **F4**, com [ADR-0029](docs/adr/0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0030](docs/adr/0030-escopo-do-hot-reload.md) e [ADR-0031](docs/adr/0031-temas-nomeados.md) já escritos.

**Os quatro ADRs que destravaram a F3 estão implementados.** Escritos depois da F2 justamente porque foi ela que expôs as lacunas — o mesmo movimento que os ADR-0017 a 0019 fizeram entre a F1 e a F2:

- [**ADR-0020**](docs/adr/0020-grupos-explicitos.md) — modelo de grupos explícitos: o grupo implícito deixa de ser único, colapso deixa de ser "só desenho" e passa a ter ordem navegável própria, o terceiro nível do RF-1.5 ganha direção, e a paleta de seis cores ganha regra para os dez grupos que a métrica do PRD-002 exige.
- [**ADR-0021**](docs/adr/0021-selecao-multipla-e-gestos-da-barra.md) — seleção múltipla e gestos da barra: onde a seleção vive, o que a invalida, `Ctrl` versus `Cmd` no macOS (onde `Ctrl`+clique é o clique secundário) e a fronteira do arraste entre grupos.
- [**ADR-0022**](docs/adr/0022-animacao-de-interface.md) — animação sob render damage-driven: o RF-2.5 exige movimento animado e o [ADR-0007](docs/adr/0007-modelo-de-threading.md) decidiu que terminal ocioso não gera frame. Sem esta decisão, a F2 recusou animação três vezes.
- [**ADR-0023**](docs/adr/0023-editor-de-grupo.md) — o editor de grupo, quinto widget de chrome, que o RF-2.22 exige e que o ADR-0014 descartou como substituto de menu sem decidir como ele próprio funciona.

Leia-os antes de mexer em grupo, seleção, animação ou popover: são eles que explicam a forma de `Group`/`Workspace`, por que a seleção não vive no core, e por que existe um relógio de animação num app de render damage-driven. Duas coisas ficaram fora do que eles previam, e estão registradas na seção 4.4 da especificação visual: o `reflow` interpola largura de cápsula e opacidade de aba, não só posição (ADR-0022), e a entrada de cor por hexadecimal segue diferida (ADR-0023).

## Stack travada

`winit` (janela/eventos) · `wgpu` (GPU) · `glyphon`/`cosmic-text` (texto) · `alacritty_terminal` (motor VT) · `portable-pty` (PTY) · `arboard` (clipboard) · TOML+`serde` (config).

Toolchain: stable **pinada** em `rust-toolchain.toml`, edition 2024, lints em `[workspace.lints]` ([ADR-0011](docs/adr/0011-toolchain-rust.md)).

Cada uma dessas escolhas tem um ADR em [docs/adr/](docs/adr/). Não troque nenhuma sem escrever um ADR novo.

## Crates e regra de dependência

```
porecatu (bin)
   ├── porecatu-ui ──── porecatu-render
   │        │
   │        ├── porecatu-term ──── porecatu-pty
   │        ├── porecatu-config
   │        └── porecatu-session ── porecatu-core
   └── porecatu-core
```

Dependências apontam **só para baixo**. Em particular:

| Crate | Pode depender de | Nunca depende de |
|---|---|---|
| `porecatu-core` | nada do projeto | tudo o mais |
| `porecatu-config` | `core` | GUI, PTY |
| `porecatu-pty` | — | GUI, `core` |
| `porecatu-term` | `pty` | GUI, `config` |
| `porecatu-render` | — | `core`, `config`, `term` |
| `porecatu-ui` | `core`, `config`, `term`, `render` | — |
| `porecatu-session` | `core` | GUI, PTY |

`porecatu-render` **não conhece o domínio**: recebe primitivas de desenho (quad, retângulo arredondado, run de texto) e nada sobre abas ou grupos. Quem traduz config+estado em primitivas é `porecatu-ui`.

`porecatu-term` **não conhece `Config`**: chaves como `scrollback.lines`, `selection.word_separators` e `terminal.clipboard.*` chegam num struct de parâmetros do próprio crate (`TermParams`), montado por `porecatu-ui`. E o snapshot de grade sai com cor **não resolvida** (`Default`/`Indexed`/`Rgb`) — quem aplica paleta e tema é `ui`. Detalhes na [seção 4 da arquitetura](docs/arquitetura.md).

Duas consequências que já apareceram em código:

- O binário `src/main.rs` só chama `porecatu_ui::run()`. **O event loop do `winit` vive em `porecatu-ui`**, não no bin — a árvore acima é grafo de dependência, não de responsabilidade.
- `porecatu-ui` precisa de `SpawnConfig`/`PtySize`/`PtyError` para chamar `Terminal::spawn`, mas não pode depender de `porecatu-pty`. `porecatu-term` **re-exporta** esses tipos; é o único caminho permitido, e nenhum tipo do `portable-pty` ou do `alacritty_terminal` atravessa a fronteira.

## Comandos

Código:

```bash
cargo build --workspace
cargo test  --workspace
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
cargo run                      # abre a janela
```

Documentação:

```bash
python scripts/verify-docs.py
```

Cinco checagens, e a quinta é a que mais importa depois do [ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md): os **valores estruturais de aparência têm de bater entre código, `porecatu.example.toml` e especificação visual** (`VALORES`, em `scripts/verify-docs.py`) — altura de barra e de aba, paddings, alfas de vidro, em de ícone, botão de janela, raio do quadro do terminal, mais a altura de barra e a largura de aba **derivadas**. Mudou uma dessas constantes? Atualize os três lados no mesmo PR, senão o CI reprova. Constante renomeada também reprova, em vez de passar como "não encontrada".

CI roda os quatro comandos de código nas três plataformas, mais a verificação de documentação. Warning de clippy é erro. A matriz Rust do `.github/workflows/ci.yml` acordou junto com o `Cargo.toml` da F0 (o job `detect` já vê o workspace) e o job canário semanal contra a stable do dia está ativo.

## Convenções

- **Código, identificadores, comentários de código e nomes de arquivo de código: inglês.**
- **Documentação (README, ADR, PRD, docs/) e mensagens de commit: português do Brasil.**
- Commits em [Conventional Commits](https://www.conventionalcommits.org): `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`.
- Arquivos de doc em `kebab-case.md`.
- Actions do GitHub são pinadas por **SHA de commit**, com a versão no comentário ao lado (`@3d3c42e… # v7.0.1`). Tag major é ponteiro móvel; o Dependabot atualiza SHA e comentário juntos. Mesma disciplina do [ADR-0011](docs/adr/0011-toolchain-rust.md).
- Terminologia: **"abas"**, nunca "guias" — inclusive em strings de UI ([ADR-0009](docs/adr/0009-referencia-visual-e-reconciliacao.md)).

## Licença

**GPL-3.0-or-later** ([ADR-0010](docs/adr/0010-licenciamento.md)). Todo arquivo de código-fonte novo começa com:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
```

Sem exceção — inclusive scripts. Registrar a convenção antes de existir o primeiro `.rs` é o que evita varrer o repositório depois.

`LICENSE` é cópia verbatim da FSF e **nunca é editado**; o workflow `docs` verifica o hash. Antes de adotar qualquer crate novo, conferir se a licença é compatível com GPLv3 — Apache-2.0 e MIT são; nem toda é.

## Aparência: o binário é o alvo, e nada de valor inventado

Antes de implementar qualquer coisa de chrome — barra de abas, pílula de grupo, popover, terminal — leia [docs/design/especificacao-visual.md](docs/design/especificacao-visual.md).

**A interface como está é o alvo.** O [ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md) inverteu a autoridade que o ADR-0009 tinha estabelecido: o **binário** é normativo para a aparência, a especificação visual o **descreve** — e é atualizada no mesmo PR que o muda —, e o [mockup](docs/design/mockup-estatico.html) é referência **histórica**. Divergência entre mockup e binário não é bug e não se conserta. Os PRDs continuam normativos para **comportamento**; requisito de PRD que descrevia aparência mudada é **emendado**, não deixado em contradição.

**Nenhuma mudança de aparência sem aval do dono do produto.** Vale inclusive para o que a seção 4.4 registra: ela é histórico, não lista de tarefas. Duas das quatro dívidas antigas foram aprovadas para a F4 (hover por brilho, sombra nos cinco widgets e no fantasma de arraste) e duas foram fechadas como decisão de **não fazer** (corpo de aviso em três linhas, auto-scroll do arraste por intervalo).

Regra que **não** cai: **nenhuma cor, dimensão, raio ou espaçamento é inventado.** Sai da tabela de tokens (seção 1) ou do [porecatu.example.toml](docs/config/porecatu.example.toml), que traz esses mesmos valores como default. O que muda é a fonte de verdade — valor novo que entra em código entra nesses dois lugares na mesma leva, senão quem ler depois não sabe de onde ele veio.

Isso só se sustenta porque a face do design (**Iosevka Fixed** 400/500, terminal **e** chrome — mesma família nos dois desde o [ADR-0026](docs/adr/0026-chrome-unificado-em-iosevka-fixed.md), que supersede a divisão do [ADR-0025](docs/adr/0025-iosevka-no-lugar-da-ibm-plex.md) entre Fixed no terminal e Aile no chrome) é **embutida no binário** — métrica de fonte diferente muda largura de célula e de aba. É **recortada** por `scripts/subset-fonts.py`; a OFL da Iosevka permite, por não ter Reserved Font Name. Rodar o script à mão ao subir a versão da fonte, e conferir a lista de faixas antes de assumir que um bloco novo desenha.

Uma **sexta** face, só de ícones, entrou pelo [ADR-0024](docs/adr/0024-face-de-icones.md) e continua: Lucide (ISC), em `FontFace::Icon`, com os codepoints em uso nomeados em `porecatu_render::icon`. Ícone do chrome sai de lá, nunca de um glyph Unicode escrito na mão — a Plex não tem `✕`, `▶` nem `▼`, e o chrome não usa a cadeia de fallback do sistema de propósito (ícone vindo do sistema mudaria de desenho por máquina). E `size_px` de ícone é a **em**, não o tamanho do desenho: o Lucide preenche ~0.6 dela, então a em vai a 20px pra desenhar o que a espec. chama de 10px — quem precisa da largura do desenho (layout) usa `Icon::ink_width`, e quem precisa centrar usa `Icon::centered_origin`.

**Dois** requisitos do v1 ainda não têm desenho aprovado: os estilos `left-bar` e `outline` do indicador de grupo (F4, e nem o default os usa mais) e aba restaurada sem shell iniciado (F5). Estão na seção 4.2 da especificação. Para esses, vale o julgamento de quem implementa — mas ainda usando os tokens existentes, nunca cores novas. A cor de seleção de texto no terminal saiu da lista com o ADR-0028: o valor que o binário desenha é o valor deliberado.

O que a F2 precisava e não tinha desenho foi decidido e escrito: **seções 2.17 a 2.20** (indicadores da aba, overflow da trilha, arraste, tooltip), mais os detalhes completados nas seções 2.2, 2.5, 2.14, 2.15 e 2.16.

O da F3 também: **seções 2.10.1** (campo de nome inline na pílula) e **2.19.1** (arraste do rótulo do grupo), mais o realce de fronteira na 2.19, a aba selecionada na 2.5, o teto do nome e o indicador agregado na 2.4, a posição real do editor na 2.10 e o token `reflow` na 1.10.

**A seção 4.4 é o histórico dessas decisões, não uma lista de dívida** — desde o ADR-0028 ela registra o que vale e por quê, e a F4 não a cobra. São mais de vinte entradas, e a maioria veio de pedido direto do usuário depois de ver a barra em tela.

Da F3, em ordem de impacto: a **cápsula de cor cheia** no lugar do tingimento de `.07` (§2.3), com o fundo da aba em alfa `.85` (§2.5) para deixar passar um indício dela; o **fim da ordem de cedência** do overflow — nada encolhe, a trilha rola (§2.18, §2.4), por custo de medição de texto; a **largura fixa da aba** (§2.5), para título novo não refluir a trilha inteira; o **botão de nova aba por grupo** (§2.6), com o global tendo aparecido e saído no caminho — a zona fixa à direita ficou com o **botão de configurações**, inerte até a F4; o **sublinhado de grupo removido** (§2.5), redundante desde a cápsula; **borda de aba em 2px**; **aba de 34px numa trilha com 6px de respiro** (barra a 52px); **aba solta mais alta que a agrupada**, porque não tem bloco a que ceder o `wrapper_padding`; o **tom de hover dos ícones promovido a tom de base** (`#e4e8ee`, o traço fino do Lucide some num cinza médio), com o "+" do grupo e o nome/caret da pílula no escuro (`#12151a`) por caírem sobre a cor cheia; o **respiro horizontal dos botões de ícone** (`icon_button_padding_x`); a cápsula desenhada **também com o grupo colapsado**; o "+" do grupo somindo no colapso e um "+" de aba solta fechando a trilha; e a **face de ícones** do ADR-0024, a única que não é escolha de desenho e sim ícone que não desenhava.

Depois da fase, também a pedido do usuário: o **efeito de vidro** na cápsula (`.85`) e na pílula (`.92`), com rim translúcido de 1px em branco a `.16`; a **sombra em camadas** na cápsula, na aba solta e no **quadro arredondado do terminal**, que também é novo (§2.7); o **indicador de overflow em círculo** de 18×18, só chevron, sem contagem (§2.18); o **contador removido da pílula** (§2.4); o `wrapper_padding` deixando de entrar em run implícito; e a **borda inferior da barra** deixando de ser pintada, porque virava uma linha contra o quadro do terminal.

**Revisto depois de fechada a fase, a pedido do usuário:** o nome do grupo voltou a divergir da aba, agora em peso (500/`Medium`, para ler como bold, contra o 400 da aba — o tamanho continua igual); o swatch de cor da pílula saiu, e a pílula inteira passou a ser pintada com a cor cheia do grupo; e nome/caret da pílula seguiram o "+" do grupo para o mesmo escuro (`#12151a`) por caírem sobre essa mesma cor cheia — o "+" deixou de ser o único ícone nessa condição. Tudo registrado na seção 4.4.

> **O mockup mostra o produto completo, não o v1.** Painéis divididos, perfis de aba, paleta de comandos, painel de configurações GUI e barra de status são `[v2]`. Da barra de título do mockup (logo, nome do app, título da aba ativa em faixa própria), só essa faixa de identidade continua `[v2]` — os controles de janela e o resize sem decoração nativa já são `[v1]`, fora do macOS ([ADR-0027](docs/adr/0027-controles-de-janela-e-resize-proprios.md)). A tabela de fases (seção 3) classifica todo elemento. Consulte-a antes de construir qualquer coisa que apareça no desenho. Ver [ADR-0009](docs/adr/0009-referencia-visual-e-reconciliacao.md).

## Processo de decisão arquitetural

Decisão aceita **não se edita**. Para mudar:

1. Escreva um ADR novo com o próximo número, referenciando o antigo em `Supersedes: ADR-NNNN`.
2. Marque o antigo como `Status: Superseded by ADR-MMMM`.
3. Atualize a tabela em [docs/adr/0000-template.md](docs/adr/0000-template.md) e a tabela de stack do README.

Correção de erro factual ou clareza no texto de um ADR aceito é permitida. Mudança de *decisão*, não.

## Armadilhas conhecidas

Anotadas aqui porque custam horas quando descobertas na marra:

- **`winit`**: o event loop precisa rodar na main thread (obrigatório em macOS e Windows). Toda submissão de frame `wgpu` acontece lá. I/O de PTY na main thread trava a UI — nunca faça.
- **`alacritty_terminal`**: não segue SemVer estável entre releases. **Pine a versão exata** (`=0.x.y`) e mantenha o uso isolado dentro de `porecatu-term`, para que uma troca de motor não vaze para o resto.
- **`wgpu`**: quebra API a cada release. Pine a versão. Subir `wgpu` é uma tarefa própria, não um efeito colateral de outra.
- **Render damage-driven**: 60 linhas de saída de `cargo build` não podem virar 60 frames. Wakeups do PTY são coalescidos por frame. Ver [ADR-0007](docs/adr/0007-modelo-de-threading.md).
- **`cwd` no Windows**: não há API barata para ler o diretório atual de um processo filho. A restauração de sessão depende de OSC 7 emitido pelo shell. Ver [ADR-0005](docs/adr/0005-persistencia-de-sessao.md).
- **ConPTY**: re-renderiza a tela e injeta sequências próprias; o comportamento não é idêntico a um PTY Unix. Ver [ADR-0004](docs/adr/0004-pty-cross-platform.md).
- **`Wakeup` precisa de `(WindowId, TabId)`**, não só do `TabId`. Os IDs são por workspace e o workspace é por janela ([ADR-0006](docs/adr/0006-modelo-de-abas-e-grupos.md)): com duas janelas abertas, dois `TabId(1)` existem, e o evento sozinho não diz qual aba sujou. O sintoma é a janela errada redesenhando. Ver [ADR-0015](docs/adr/0015-multiplas-janelas.md).
- **Retorno antecipado no `!pressed` engole o release do mouse.** `dispatch_mouse_input` tratava só o press e voltava cedo em todo release, então `input::handle_mouse_button` nunca era chamado com `pressed = false`: o programa recebia o `M` do press (SGR) e nunca o `m` do release, em modo de mouse tracking nenhum. O sintoma é clique preso em qualquer app que peça mouse — `btop4win`, o próprio Claude Code CLI. E o estado de botão apertado precisa ser zerado **sempre** no release e em `Focused(false)`, senão um alt-tab com o botão físico apertado o deixa preso.
- **`std::env::current_dir()` não é "o diretório do usuário".** É o diretório de onde o binário foi lançado, e usá-lo como `startup_directory` fazia toda aba nova abrir na pasta do executável quando não havia `cwd` por OSC 7. Hoje é `dirs::home_dir()`; `group.new_tab` herda o `cwd` da **última aba do grupo de destino**, não o da aba ativa.
- **`Shift` sobrepõe o programa no mouse.** Quando um programa pede eventos de mouse, o arraste vira input dele e a seleção de texto para de funcionar. `Shift` força a seleção local, sempre — sem isso não se copia de dentro do `htop`. Ver [ADR-0013](docs/adr/0013-mouse-selecao-e-clipboard.md).
- **`TERM=xterm-256color`, não terminfo próprio.** Sob SSH, o host remoto consulta o terminfo dele; um valor que só existe na máquina local produz `unknown terminal type` do outro lado. Ver [ADR-0012](docs/adr/0012-identificacao-do-terminal.md).
- **Clipboard no Wayland** é o ponto frágil do `arboard`. Já está encapsulado num só lugar (`porecatu-ui/clipboard.rs`); `copypasta` é o plano B, e a verificação **continua pendente** — não houve ambiente Linux/Wayland na F1 nem na F2.
- **Nenhum diálogo nativo do sistema.** `MessageBox` e `NSAlert` bloqueiam o event loop e são a única superfície que a config do usuário não alcança. Aviso, diálogo, menu de contexto, tooltip e editor de grupo são widgets nossos — cinco, não três: o tooltip entrou pelo [ADR-0019](docs/adr/0019-tooltip.md) e o editor pelo [ADR-0023](docs/adr/0023-editor-de-grupo.md). Ver [ADR-0014](docs/adr/0014-superficie-de-aviso-e-dialogo.md).
- **Blend mode do pipeline de quad tem que casar com o que o shader devolve.** `quad.wgsl` sempre devolveu cor **premultiplicada** (`color.rgb * alpha, alpha`), mas o pipeline (`quad.rs`) usava `wgpu::BlendState::ALPHA_BLENDING` (straight) — o par errado para saída premultiplicada. O alpha era aplicado em dobro exatamente na faixa de antialiasing do SDF (`fwidth(dist)`), escurecendo um anel fino no contorno de **todo** canto arredondado do chrome, sempre, desde sempre. Ficou invisível enquanto qualquer coisa (borda, cor diferente atrás) já marcava aquele contorno; virou um "borda estranha nos rounded corners" visível assim que a pílula do grupo passou a ser pintada com a mesma cor cheia da cápsula atrás dela, sem borda — o primeiro caso do app com duas formas arredondadas idênticas sobrepostas. Fix: `wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING`.

### Descobertas na F1 — custaram horas, não repetir

- **Não fechar o pseudo-console no encerramento (Windows).** `ClosePseudoConsole` bloqueia até a cópia clonada do pipe de leitura ser liberada, e a thread de leitura está parada num `read()` síncrono nela: deadlock, app só morre por kill externo. `porecatu-term` mata o processo (`TerminateProcess`) e usa `mem::forget` no `PtyHandle` — o SO reclama as handles quando o processo do Porecatu sai. `Terminal::shutdown` **não dá `join` em nenhuma das três threads**; a confirmação de "processo morto" vem de um canal dedicado com timeout de segurança.
- **Formato de surface `*Srgb` lava as cores.** `get_default_config` escolhe um formato sRGB e a GPU reaplica a curva sobre valores que já vêm em espaço sRGB (as cores saem de hex do design): fundo quase-preto vira cinza-azulado. Fix: `config.format.remove_srgb_suffix()` depois do `get_default_config`.
- **Respostas automáticas do motor (DSR/DA/CPR) vão direto ao canal de escrita do PTY**, não por `TermEvent`. Roteá-las como evento obriga todo consumidor a filtrar e repassar, e esquecer um write pendente é o programa travar esperando resposta que nunca chega.
- **`TermEvent::Exit` vem do `try_wait` do PTY, não de EOF.** No Windows o pipe do ConPTY não emite EOF só porque o processo hospedado saiu.
- **Teste interativo é bloqueado pela proteção de foco do Windows.** `SetForegroundWindow`/`AppActivate` de processo em segundo plano não funcionam; teclado e arraste sintéticos não são caminho viável. Verificação de teclado, mouse e seleção precisa de sessão desktop real.
- **O motor já invalida a seleção sozinho** quando o programa escreve sobre a região selecionada e ao entrar/sair de tela alternativa, e `scroll_display` a preserva. Não reimplementar isso do lado de fora.
- **O `Renderer` da F1 achatava as primitivas em três baldes** — todos os quads, todos os arredondados, todo o texto — e desenhava nessa ordem, ignorando a ordem da lista. Correto para a grade, fatal para chrome: o fundo de um popover caía **atrás** do texto do terminal, e `PushClip` por índice não sobrevivia ao achatamento. Resolvido na F2 pelas camadas do [ADR-0018](docs/adr/0018-composicao-de-frame.md): hoje é `resolve_layer` em `porecatu-render/frame.rs`, e `Renderer` não existe mais.
- **Medir largura de string proporcional exigiu tirar o `FontSystem` da GPU.** Na F1 o único medidor era `measure_mono_cell` e o `FontSystem` vivia trancado dentro do pipeline de texto, que exige `wgpu::Device` — então não havia como truncar título de aba nem dimensionar item de menu fora de um frame. Hoje é `TextMeasurer` (`porecatu-render/text_measurer.rs`), construível sem `Device` nem `Queue`, e é o que torna o layout da barra a função pura da seção 7 da arquitetura.

### Descobertas na F2 — custaram horas, não repetir

- **O `vte` descarta OSC 7 antes de qualquer `Handler`.** O `osc_dispatch` filtra a sequência sem chamar método nenhum, então não existe gancho de `Handler` para interceptá-la sem forkar o crate — o mecanismo que o ADR-0017 previa não existe. A captura roda como um **segundo parser `vte::Perform`**, independente e sem efeito colateral no motor, sobre os mesmos bytes (`porecatu-term/osc7.rs`). O resultado observável é o mesmo; o caminho, não.
- **A escala da janela nunca era aplicada à métrica de fonte na F1.** O bug ficou latente porque toda a verificação aconteceu em escala 1.0. Hoje a conversão de pixels lógicos para físicos acontece num ponto só, dentro de `WindowSurface`, como o ADR-0018 exige — e é o que faz duas janelas em monitores de DPI diferente desenharem com texto nítido.
- **Arraste não deve mutar o estado real.** O gesto clona o `Workspace` a cada redraw, aplica `move_tab` no clone e só efetiva ao soltar dentro da barra. Soltar fora ou `Esc` descartam o clone: não há o que desfazer, porque nada foi tocado. Reordenar de verdade durante o gesto obrigaria a implementar undo do arraste.
- **`ControlFlow::WaitUntil` é o temporizador de UI do projeto.** Atraso do tooltip (600 ms) e expiração da informação (6 s) não usam thread nem loop de render: o event loop dorme até a hora exata e volta a `Wait` depois. Estado com temporizador recebe `Instant` de fora e nunca chama `Instant::now()` — é o que os torna testáveis sem dormir de verdade.
- **`porecatu-render` não tem primitiva de sombra nem de filtro de brilho — e a sombra que existe é empilhada à mão.** `chrome::push_shadow` aproxima uma sombra com três `RoundedQuad` pretos empilhados (spread crescente, alfa decrescente), e ela está na cápsula de grupo, na aba solta e no quadro do terminal. Os **cinco widgets de chrome e o fantasma de arraste seguem sem sombra**, e a mancha grande do CSS (`0 18px 44px`) não sai por essa técnica — aliaseia em anéis visíveis. Hover por brilho é resolvível em CPU dentro de `porecatu-ui` (multiplicar canais e clampar) e nada no chrome o desenha ainda, exceto os botões de janela do ADR-0027. Os dois estão aprovados para a F4 (ADR-0028 §4); até lá, a seção 4.4 da especificação registra o estado.

### Descobertas na F3 — custaram horas, não repetir

- **Medir texto sem cache no caminho quente é a armadilha de performance deste projeto.** O `fit_width` da F2 fazia busca binária sobre o teto do rótulo: até **24 relayouts completos da trilha por frame**, cada um remedindo o texto de toda aba com `cosmic-text` (`TextMeasurer` não tem cache). Com a barra em overflow — o caso que a busca existia para tratar — o app parecia travado ao trocar de aba. O encolhimento foi descartado; se voltar na F4, volta com cache de medição, não sem.
- **Animar a posição dos vizinhos não parece animação.** Três PRs para o mesmo relato ("o colapso só anima no primeiro grupo"): faltava esmaecer as abas do próprio grupo, que sumiam na hora, e faltava interpolar a **largura** da cápsula, que saltava para o tamanho final. O elemento que o usuário está tocando é o que precisa animar primeiro; se ele salta, o deslize dos vizinhos não salva nada. Corolário: a cápsula continua desenhada até o progresso chegar em 1, mesmo já colapsada no modelo — senão ela sumiria na hora e as abas esmaeceriam por cima do nada.
- **Core à frente da UI esconde bug de ordem.** `Workspace::group_tabs` empurrava o grupo novo antes do que sobrava mesmo quando a aba extraída vinha depois: quatro etapas de testes de invariante verdes, porque invariante de contiguidade não é ordem visual. Quando o core anda à frente da UI, pelo menos um teste por operação precisa fixar a **ordem resultante**, não só as invariantes.
- **Ação sem gesto no fim da etapa que a nomeia é dívida, não escopo adiado.** `group.create` — o RF-2.4/RF-2.5, o coração da fase — atravessou as seis etapas sem wiring de UI, cada etapa registrando "testável via `Workspace` direto". Só um PR de correção deu a ela `Ctrl+Shift+G`, e foi esse gesto que expôs o bug de ordem acima.
- **Trilha rolável e botão global não convivem.** Com a trilha rolando como um componente só, um botão ao final dela sai de vista com muitas abas — botão inalcançável. Daí a zona fixa à direita (`right_zone_width`/`trilha_width`): a largura disponível para a trilha é a da barra **menos** essa zona, e é ela que `overflow_state` e o cálculo de arraste na borda devem usar. O botão de nova aba global que a estreou já saiu; a zona continua, com o botão de configurações.
- **Cor fixa em ícone que muda de fundo é bug esperando.** O "+" do grupo foi para `#12151a` porque fica sobre a cápsula de cor cheia — e virou um botão preto no fundo preto assim que a barra ficou sem grupo nenhum, porque um run de abas soltas não pinta cápsula. Ele é o único ícone do chrome que decide cor pelo que está atrás (`group.pill.is_some()`, a mesma condição que decide pintar a cápsula).
- **Glyph que a fonte embutida não tem não desenha — e não avisa.** Os ícones do chrome eram `U+2715`, `U+25B6` e `U+25BC` pedidos à IBM Plex Sans, que não os tem no `cmap`; o `fontdb` do projeto não chamava `load_system_fonts`, então não havia fallback: o `TextRun` sai vazio, sem erro, sem log, sem retângulo de tofu. Passou a F1, a F2 e a F3 assim, e não só no chrome — o **braille** dos gráficos do `btop` sumia pelo mesmo motivo (a IBM Plex Mono não tem um só dos 256; box drawing e blocos, esses, ela cobre inteiros). Hoje ícone vem da face Lucide ([ADR-0024](docs/adr/0024-face-de-icones.md)), com teste que reprova largura zero, e o terminal tem a cadeia de fallback do sistema que o ADR-0016 sempre exigiu.
- **Embutida antes, sistema depois — a ordem é a decisão inteira.** O `fontdb` resolve empate de família pela ordem de registro. Registrar as faces do design primeiro e `load_system_fonts()` depois entrega as duas metades do ADR-0016 de uma vez: uma cópia do sistema da Iosevka nunca ganha, e existe fallback para o que nenhuma face embutida cobre. Inverter a ordem quebra a paridade com o mockup em silêncio, só na máquina de quem tem a fonte instalada.
- **Glyph de fallback não avança uma célula — e a grade não é grade se ele viajar num run compartilhado.** Braille avança 1.26 célula, triângulo geométrico 2.29, powerline 1.67; num `TextRun` de trecho contíguo isso empurra todo o resto da linha. `paint.rs` quebra o trecho em qualquer caractere cujo avanço não seja o que a grade reservou (`TextMeasurer::advance_em`, com cache — medir por célula por frame seria a armadilha de performance de sempre), e desenha esse caractere sozinho, ancorado no `x` da célula e encolhido para caber nela. Linha de ASCII continua sendo um run só.
- **`size_px` de ícone é a em, não o desenho — e errar isso parece dois bugs diferentes.** O Lucide avança 1 em e desenha ~0.6 dela, com traço de `2/24` da em. Pedir `size_px = 10` porque a espec. diz "✕ 10px" desenha um ✕ de 5.9px (relato: "muito pequenos") com traço de 0.83px, que o antialiasing dilui a meia cobertura e mistura com o fundo (relato: "quase invisíveis", e a cor estava certa o tempo todo). A em do chrome é `chrome::ICON_EM_SIZE`; largura de desenho pra layout é `Icon::ink_width`.
- **Ícone não se centra como texto.** O desenho é centrado na em, mas a em não é centrada na linha: ascent = em e descent = 0 põem a baseline a `1.1 * size_px` do topo do `TextRun`, então o centro fica a `0.6 * size_px`, não a `0.5`. Use `Icon::centered_origin`. Tanto essa constante quanto o tamanho do desenho de cada ícone são pinados por teste contra a **rasterização** — a tabela `glyf` do arquivo me levou a uma leitura errada antes disso.
- **Largura de aba variável reflui a barra inteira a cada título novo.** Cada aba tinha a largura do próprio texto: trocar de aba, renomear ou abrir um programa que muda o título mexia na posição de todas as outras. A largura hoje é fixa (`TabBarStyle::tab_width`), derivada dos mesmos tokens da §2.5 — o teto de 180px do rótulo virou também o piso.
- **Fórmula de geometria copiada em dois lugares só diverge quando alguém mexe nela.** `chrome::paint` recalculava a altura da barra localmente, e as duas cópias concordaram até `trilha_padding` entrar na conta de `bar_height` e não na dela. O sintoma não foi "a barra está curta": foi o **respiro de baixo não aparecer**, porque o recorte da trilha, curto pela mesma cópia, cortava as abas antes dele. Altura de barra vem de `chrome::bar_height(style)`, sempre — hoje há teste que amarra o fundo e o clip pintados ao que ela promete.
- **Ligadura de fonte quebra a grade, e a verificação de avanço não pega.** A troca para a Iosevka trouxe o risco que a IBM Plex não tinha: `calt`/`dlig` substituem N glyphs por 1 durante o shaping, então `!=` viraria um glyph com um avanço só e a linha sairia do lugar. O `fits_the_grid` do `paint.rs` mede **por caractere** e passaria batido. Daí a variante **Fixed** (sem ligaduras) e o recorte mantendo só `ccmp,locl,mark,mkmk`: garantido por construção, não por confiança na variante.
- **Teste que fixa a largura da célula em número quebra na troca de fonte.** Quatro testes de `paint.rs` tinham `width: 8.4` — a célula da IBM Plex Mono a 14px. A Iosevka é mais estreita e todos reprovaram de uma vez. Célula em teste sai de `measure_mono_cell`, como no runtime.
- **Snapping de pixel e teste de avanço não medem a mesma coisa — misturar os dois explodiu a grade.** `snap_cell_metrics_to_pixel_grid` arredonda a célula ao pixel **físico** (mata a costura de 1px entre glyphs); `fits_the_grid` compara o avanço **natural** do caractere para decidir se ele viaja num run compartilhado. Comparar um contra o outro erra por até meio pixel — dez vezes a tolerância — e **toda célula reprova**: cada caractere vira um `TextRun` próprio, com um `measure_width` sem cache cada, e a grade inteira é re-shapada por frame (~4000 shapings numa 80x24, contra ~24). Os testes não pegaram porque o helper `cell()` de `paint.rs` não arredondava. O teste de grade é **em em**, contra o avanço do próprio `'M'` da face mono: invariante a tamanho de fonte e a escala de janela. Comparar em pixels obrigaria a escolher entre duas larguras que não são a mesma.
- **Medir prefixo caractere a caractere é a mesma armadilha de sempre, noutro lugar.** `TextMeasurer::truncate` media cada prefixo candidato — um `Buffer` novo e um `shape_until_scroll` **por caractere**, mais um `String::clone` — e roda por aba a cada layout da barra, que acontece por frame e a cada `CursorMoved`. Ficou latente até a largura de aba virar fixa: aí todo título mais longo que o teto passou a cair no laço. Hoje é **um shaping** e o corte sai do avanço acumulado dos glyphs (`LayoutGlyph::start`/`w`); cortar por glyph só divergiria de remedir o prefixo com ligadura ou kerning, e o recorte das faces não deixa nenhum dos dois passar.
- **A Iosevka Fixed avança 0.5 em em todo glyph (`upem` 1000, avanço 500).** Então o avanço lógico é `size / 2`, e o que precisa cair em pixel inteiro é `size / 2 * scale`: em 100%, `size` par; em 125%, múltiplo de 8; em 150%, múltiplo de 4. Daí `FONT_SIZE_PX = 14.0` — a 13 o avanço era 6.5 numa célula que arredondava para 7.0. É conforto numa escala, não correção: quem garante a grade em qualquer tamanho e qualquer escala é o teste em em acima.
- **`vte`, `wgpu` e agora o relógio: temporizador de UI é sempre `ControlFlow::WaitUntil`.** `AnimationClock` não abriu exceção — contribui para o `next_deadline()` da janela com um intervalo de frame enquanto há reflui pendente e desaparece da conta quando acaba. Estado com temporizador recebe `Instant` de fora; quem chama `Instant::now()` é `lib.rs`, nunca o módulo de estado.

## Índice

- [docs/arquitetura.md](docs/arquitetura.md) — camadas, threading, fluxo de dados
- [docs/design/](docs/design/README.md) — registro visual: tokens, anatomia, tabela de fases, histórico de decisões. Descreve o binário ([ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md)), não um alvo a perseguir
- [docs/adr/](docs/adr/) — decisões arquiteturais
- [docs/prd/](docs/prd/) — requisitos de produto (000–005 e 010 aprovados; 006–009 rascunho, fase v2). O [PRD-010](docs/prd/prd-010-interacao-e-superficie-de-app.md) é transversal: mouse, seleção, clipboard, avisos, diálogos, menus e janelas
- [docs/roadmap.md](docs/roadmap.md) — fases de entrega
- [docs/reference/acoes.md](docs/reference/acoes.md) — catálogo **fechado** de ações; ação fora dele é erro de config
- [docs/config/porecatu.example.toml](docs/config/porecatu.example.toml) — configuração de referência
