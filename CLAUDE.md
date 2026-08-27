# CLAUDE.md

Guia operacional do projeto. Leia antes de mexer em qualquer coisa.

## O que é

**Porecatu** — emulador de terminal cross-platform em Rust. Diferencial: gestão de muitos terminais (abas, grupos nomeados, sessão persistente), não conformidade VT.

**Estado atual: F0, F1 e F2 implementadas; a próxima fase é a F3 (grupos).** `cargo run` abre uma janela com abas de terminal funcionais — PTY, motor VT, render por GPU, teclado, mouse, seleção, clipboard, ciclo de vida de aba, overflow da barra, arraste, menu de contexto, tooltip, aviso e diálogo de confirmação — e `Ctrl+Shift+N` abre uma segunda janela. `porecatu-config` e `porecatu-session` ainda são stubs (só o cabeçalho SPDX); ganham corpo nas F4/F5. Enquanto `config` não existe, valor de aparência entra como constante citando no comentário a chave TOML de origem. Antes de mexer, leia [docs/arquitetura.md](docs/arquitetura.md) e os ADRs — o que está em código segue o que está escrito lá, incluindo os desvios anotados.

**As decisões que faltavam para a F3 já estão fechadas.** Quatro ADRs, escritos depois da F2 justamente porque foi ela que expôs as lacunas — o mesmo movimento que os ADR-0017 a 0019 fizeram entre a F1 e a F2:

- [**ADR-0020**](docs/adr/0020-grupos-explicitos.md) — modelo de grupos explícitos: o grupo implícito deixa de ser único, colapso deixa de ser "só desenho" e passa a ter ordem navegável própria, o terceiro nível do RF-1.5 ganha direção, e a paleta de seis cores ganha regra para os dez grupos que a métrica do PRD-002 exige.
- [**ADR-0021**](docs/adr/0021-selecao-multipla-e-gestos-da-barra.md) — seleção múltipla e gestos da barra: onde a seleção vive, o que a invalida, `Ctrl` versus `Cmd` no macOS (onde `Ctrl`+clique é o clique secundário) e a fronteira do arraste entre grupos.
- [**ADR-0022**](docs/adr/0022-animacao-de-interface.md) — animação sob render damage-driven: o RF-2.5 exige movimento animado e o [ADR-0007](docs/adr/0007-modelo-de-threading.md) decidiu que terminal ocioso não gera frame. Sem esta decisão, a F2 recusou animação três vezes.
- [**ADR-0023**](docs/adr/0023-editor-de-grupo.md) — o editor de grupo, quinto widget de chrome, que o RF-2.22 exige e que o ADR-0014 descartou como substituto de menu sem decidir como ele próprio funciona.

Não comece a F3 sem lê-los: o ADR-0020 muda a forma de `Group` e de `Workspace`, que a F2 acabou de estabilizar.

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

## Aparência: nada de valor inventado

Antes de implementar qualquer coisa de chrome — barra de abas, pílula de grupo, popover, terminal — leia [docs/design/especificacao-visual.md](docs/design/especificacao-visual.md).

Regra: **nenhuma cor, dimensão, raio ou espaçamento é inventado.** Sai da tabela de tokens (seção 1) ou do [porecatu.example.toml](docs/config/porecatu.example.toml), que já traz esses mesmos valores como default. O binário com a config padrão deve bater com [docs/design/mockup-estatico.html](docs/design/mockup-estatico.html); divergência visível é bug de implementação, não questão de configuração.

Isso só se sustenta porque as cinco faces do design (IBM Plex Mono 400/500, Sans 400/500/600) são **embutidas no binário** ([ADR-0016](docs/adr/0016-fontes-embutidas.md)) — métrica de fonte diferente muda largura de célula e de aba. Não fazer subsetting: viola a cláusula de Reserved Font Name da OFL e obrigaria a renomear a família.

Três requisitos do v1 ainda não têm desenho aprovado: os estilos `left-bar` e `outline` do indicador de grupo (F4), aba restaurada sem shell iniciado (F5) e cor de seleção de texto no terminal (F4). Estão listados na seção 4.2 da especificação. Para esses, vale o julgamento de quem implementa — mas ainda usando os tokens existentes, nunca cores novas.

O que a F2 precisava e não tinha desenho foi decidido e escrito: **seções 2.17 a 2.20** (indicadores da aba, overflow da trilha, arraste, tooltip), mais os detalhes completados nas seções 2.2, 2.5, 2.14, 2.15 e 2.16.

O da F3 também: **seções 2.10.1** (campo de nome inline na pílula) e **2.19.1** (arraste do rótulo do grupo), mais o realce de fronteira na 2.19, a aba selecionada na 2.5, o teto/piso do nome e o indicador agregado na 2.4, a posição real do editor na 2.10 e o token `reflow` na 1.10. As quatro divergências que a F2 abriu entre desenho e binário — sem sombra, sem `brightness`, corpo de aviso em uma linha, auto-scroll por evento — estão registradas na **seção 4.4**, que é onde a F4 vai cobrá-las.

> **O mockup mostra o produto completo, não o v1.** Painéis divididos, perfis de aba, paleta de comandos, painel de configurações GUI, barra de status e barra de título customizada são `[v2]`. A tabela de fases (seção 3) classifica todo elemento. Consulte-a antes de construir qualquer coisa que apareça no desenho. Ver [ADR-0009](docs/adr/0009-referencia-visual-e-reconciliacao.md).

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
- **`Shift` sobrepõe o programa no mouse.** Quando um programa pede eventos de mouse, o arraste vira input dele e a seleção de texto para de funcionar. `Shift` força a seleção local, sempre — sem isso não se copia de dentro do `htop`. Ver [ADR-0013](docs/adr/0013-mouse-selecao-e-clipboard.md).
- **`TERM=xterm-256color`, não terminfo próprio.** Sob SSH, o host remoto consulta o terminfo dele; um valor que só existe na máquina local produz `unknown terminal type` do outro lado. Ver [ADR-0012](docs/adr/0012-identificacao-do-terminal.md).
- **Clipboard no Wayland** é o ponto frágil do `arboard`. Já está encapsulado num só lugar (`porecatu-ui/clipboard.rs`); `copypasta` é o plano B, e a verificação **continua pendente** — não houve ambiente Linux/Wayland na F1 nem na F2.
- **Nenhum diálogo nativo do sistema.** `MessageBox` e `NSAlert` bloqueiam o event loop e são a única superfície que a config do usuário não alcança. Aviso, diálogo, menu de contexto, tooltip e editor de grupo são widgets nossos — cinco, não três: o tooltip entrou pelo [ADR-0019](docs/adr/0019-tooltip.md) e o editor pelo [ADR-0023](docs/adr/0023-editor-de-grupo.md). Ver [ADR-0014](docs/adr/0014-superficie-de-aviso-e-dialogo.md).

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
- **`porecatu-render` não tem primitiva de sombra nem de filtro de brilho.** A especificação visual pede `filter: brightness()` no hover e sombra de popover nos quatro widgets; nenhum dos dois existe. Hover por brilho é resolvível em CPU dentro de `porecatu-ui` (multiplicar canais e clampar); sombra, não. Registrado na seção 4.4 da especificação.

## Índice

- [docs/arquitetura.md](docs/arquitetura.md) — camadas, threading, fluxo de dados
- [docs/design/](docs/design/README.md) — alvo visual: tokens, anatomia, tabela de fases
- [docs/adr/](docs/adr/) — decisões arquiteturais
- [docs/prd/](docs/prd/) — requisitos de produto (000–005 e 010 aprovados; 006–009 rascunho, fase v2). O [PRD-010](docs/prd/prd-010-interacao-e-superficie-de-app.md) é transversal: mouse, seleção, clipboard, avisos, diálogos, menus e janelas
- [docs/roadmap.md](docs/roadmap.md) — fases de entrega
- [docs/reference/acoes.md](docs/reference/acoes.md) — catálogo **fechado** de ações; ação fora dele é erro de config
- [docs/config/porecatu.example.toml](docs/config/porecatu.example.toml) — configuração de referência
