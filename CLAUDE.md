# CLAUDE.md

Guia operacional do projeto. Leia antes de mexer em qualquer coisa. O porquê de cada armadilha, com o custo de cada uma, está em [docs/descobertas.md](docs/descobertas.md).

## O que é

**Porecatu** — emulador de terminal cross-platform em Rust. Diferencial: gestão de muitos terminais (abas, grupos nomeados, sessão persistente), não conformidade VT.

**Estado:** F0 a F6 fechadas (o v1 inteiro); versão atual **0.9.5** (a primeira publicada foi `0.7.0`, não `1.0.0` — [ADR-0045](docs/adr/0045-primeira-versao-0-7-0.md)). Detalhe por fase e etapa em [docs/roadmap.md](docs/roadmap.md). A verificação interativa é **dívida assumida**, não critério pendente: a proteção de foco do Windows atrapalha input sintético e não há Linux/macOS no fluxo — fases e recursos fecham com teste automatizado, smoke test e verificação ao vivo só no Windows.

Recursos entregues **fora da ordem de fases**, todos implementados (detalhe e dívida de verificação de cada um no roadmap):

- **Barra de status** — [PRD-009](docs/prd/prd-009-barra-de-status.md), [ADR-0048](docs/adr/0048-barra-de-status.md); branch Git por `.git/HEAD` + `mtime` ([ADR-0049](docs/adr/0049-branch-git-na-barra-de-status.md)).
- **Sincronização com o remoto do Git** — [PRD-013](docs/prd/prd-013-sincronizacao-com-o-remoto-do-git.md), [ADR-0052](docs/adr/0052-sincronizacao-com-o-remoto-do-git.md). Estado por repositório num mapa do processo (`App.git_remotes`), nunca por janela; resultado de thread é **dado chaveado**, nunca comando.
- **Arquivo de projeto `.porecatu`** — [PRD-012](docs/prd/prd-012-comando-de-projeto-por-diretorio.md), [ADR-0051](docs/adr/0051-arquivo-de-projeto-porecatu.md), [referência](docs/reference/arquivo-de-projeto.md). Só em aba restaurada, só em `[project_file] trusted_paths` (vazio por default), uma vez por aba no painel focado.
- **Painéis divididos** — [PRD-006](docs/prd/prd-006-paineis-divididos.md), [ADR-0053](docs/adr/0053-paineis-divididos.md). Árvore binária no core; `process_title`/`cwd`/`shell_name`/`state`/`activity`/`bell` são **do painel**, a aba deriva.
- **Sessões nomeadas** — [PRD-014](docs/prd/prd-014-sessoes-nomeadas.md), [ADR-0054](docs/adr/0054-sessoes-nomeadas.md), [ADR-0055](docs/adr/0055-botao-e-popover-de-sessoes.md).
- **Idioma da interface** — [PRD-015](docs/prd/prd-015-idioma-da-interface.md), [ADR-0056](docs/adr/0056-catalogo-de-textos-da-interface.md), [ADR-0057](docs/adr/0057-idiomas-nos-artefatos.md). Crate `porecatu-locale`; arquivos `locales/` só em disco, nunca embutidos; default `en_US`.
- **Tela de configurações** — [PRD-016](docs/prd/prd-016-tela-de-configuracoes.md), [ADR-0058](docs/adr/0058-escrita-do-arquivo-de-configuracao.md), [ADR-0059](docs/adr/0059-janela-de-configuracoes.md), [ADR-0060](docs/adr/0060-anatomia-da-tela-de-configuracoes.md). `porecatu.toml` segue a única fonte de verdade; escrita por `toml_edit` em `porecatu-config::edit`, por edição de chave; quem aplica é o hot reload.
- **Imagem de fundo do terminal e da janela** — [PRD-017](docs/prd/prd-017-imagem-de-fundo-do-terminal.md)/[ADR-0061](docs/adr/0061-imagem-de-fundo-do-terminal.md), [PRD-018](docs/prd/prd-018-imagem-de-fundo-da-janela.md)/[ADR-0062](docs/adr/0062-imagem-de-fundo-da-janela.md), [ADR-0063](docs/adr/0063-imagem-da-janela-sem-furo-ate-o-desktop.md). Aval visual do dono **pendente**.
- **Temas em arquivos próprios** — [ADR-0067](docs/adr/0067-temas-em-arquivos-proprios.md). `thm_<nome>.toml` ao lado do `porecatu.toml` é o único lugar de cor.
- **Efeito acrílico** — [PRD-019](docs/prd/prd-019-efeito-acrilico-no-fundo-do-terminal.md), [ADR-0068](docs/adr/0068-efeito-acrilico-no-fundo-do-terminal.md). Único passe de filtro do render; aval visual **pendente**.

Antes de mexer, leia [docs/arquitetura.md](docs/arquitetura.md) e os ADRs da área — o código segue o que está escrito lá, incluindo os desvios anotados. Grupos, seleção, animação e popover: ADR-0020 a 0023. Sessão: ADR-0036 a 0040.

### Regras que vieram dos recursos e continuam valendo

- **Cor nova em `Config` entra em três lugares no mesmo PR**: o campo (com `#[serde(skip_deserializing)]`), a linha em `theme::mergeable_fields` e as duas `docs/config/thm_*.toml`. O teste de completude reprova se faltar.
- **Valor de aparência sem consumidor em `Config`** entra como constante citando a chave TOML de origem no comentário.
- **Texto de interface só nasce em `porecatu-ui`**; `core`, `config` e `session` devolvem erro tipado, nunca prosa.
- **`unsafe_code = "deny"` sem exceção** — operação de SO passa por crate wrapper seguro (`win32job`, `portable-pty`, ...).
- **Estado com temporizador recebe `Instant` de fora**; temporizador de UI é sempre `ControlFlow::WaitUntil`, nunca thread nem loop de render.
- **Painel em segundo plano**: evento de PTY vem com `pane_id` de origem — use as variantes `set_pane_*`/`mark_pane_*`, nunca as que miram o painel focado.

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
   │        ├── porecatu-locale
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
| `porecatu-locale` | — | todo crate do projeto ([ADR-0056](docs/adr/0056-catalogo-de-textos-da-interface.md)) |

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

Cinco checagens, e a quinta é a que mais importa depois do [ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md): os **valores estruturais de aparência têm de bater entre código, `porecatu.example.toml` e especificação visual** (`VALORES`, em `scripts/verify-docs.py`). Mudou uma dessas constantes? Atualize os três lados no mesmo PR, senão o CI reprova. Constante renomeada ou movida de crate também reprova — rode o script sempre que uma constante muda de lugar, não só de valor.

CI roda os quatro comandos de código nas três plataformas, mais a verificação de documentação. Warning de clippy é erro.

## Convenções

- **Código, identificadores, comentários de código e nomes de arquivo de código: inglês.**
- **Documentação (README, ADR, PRD, docs/) e mensagens de commit: português do Brasil.**
- Commits em [Conventional Commits](https://www.conventionalcommits.org): `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`.
- Arquivos de doc em `kebab-case.md`.
- Actions do GitHub são pinadas por **SHA de commit**, com a versão no comentário ao lado (`@3d3c42e… # v7.0.1`). Tag major é ponteiro móvel; o Dependabot atualiza SHA e comentário juntos. Mesma disciplina do [ADR-0011](docs/adr/0011-toolchain-rust.md).
- Terminologia: **"abas"**, nunca "guias" — inclusive em strings de UI ([ADR-0009](docs/adr/0009-referencia-visual-e-reconciliacao.md)), que moram em `locales/pt_BR.toml`, com teste.
- **Frase de interface nunca é escrita no código** ([ADR-0056](docs/adr/0056-catalogo-de-textos-da-interface.md)): identificador no registro de mensagens de `porecatu-ui` e valor em todo arquivo de `locales/`. Crate abaixo de `ui` devolve erro tipado, não prosa.
- Quando o core anda à frente da UI, pelo menos um teste por operação fixa a **ordem resultante**, não só invariantes.

## Licença

**GPL-3.0-or-later** ([ADR-0010](docs/adr/0010-licenciamento.md)). Todo arquivo de código-fonte novo começa com:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
```

Sem exceção — inclusive scripts.

`LICENSE` é cópia verbatim da FSF e **nunca é editado**; o workflow `docs` verifica o hash. Antes de adotar qualquer crate novo, conferir se a licença é compatível com GPLv3 — Apache-2.0 e MIT são; nem toda é.

## Aparência: o binário é o alvo, e nada de valor inventado

Antes de implementar qualquer coisa de chrome, leia [docs/design/especificacao-visual.md](docs/design/especificacao-visual.md).

- **O binário é normativo para a aparência** ([ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md)); a especificação o **descreve** e é atualizada no mesmo PR que o muda. O [mockup](docs/design/mockup-estatico.html) é histórico: divergência com ele não é bug. PRDs continuam normativos para comportamento; requisito que descrevia aparência mudada é **emendado**.
- **Nenhuma mudança de aparência sem aval do dono do produto.** A seção 4.4 da especificação é histórico das decisões, não lista de tarefas.
- **A interface do v1 está fechada** ([ADR-0032](docs/adr/0032-interface-do-v1-fechada.md)): mudar seções 1/2 da especificação exige ADR novo.
- **Nenhuma cor, dimensão, raio ou espaçamento é inventado.** Sai da tabela de tokens (seção 1), do [porecatu.example.toml](docs/config/porecatu.example.toml) (dimensões) ou do [thm_default.toml](docs/config/thm_default.toml) (cores). Valor novo em código entra nesses lugares na mesma leva.
- **Fonte:** Iosevka Fixed 400/500, terminal **e** chrome, mesma chave `[terminal.font]` ([ADR-0026](docs/adr/0026-chrome-unificado-em-iosevka-fixed.md), [ADR-0065](docs/adr/0065-fonte-unica-para-terminal-e-chrome.md)), **embutida e recortada** por `scripts/subset-fonts.py` (rodar à mão ao subir a versão; conferir a lista de faixas). Fixed porque ligadura quebra a grade.
- **Ícones:** face Lucide (`FontFace::Icon`, [ADR-0024](docs/adr/0024-face-de-icones.md)), codepoints em `porecatu_render::icon` — nunca glyph Unicode escrito à mão. `size_px` de ícone é a **em**, não o desenho (~0.6 da em): layout usa `Icon::ink_width`, centragem usa `Icon::centered_origin`, em do chrome é `chrome::ICON_EM_SIZE`.
- **Sombra e hover** ficam fora do render: `chrome::push_shadow` (três `RoundedQuad` empilhados) e `chrome::brighten` (CPU). O único passe de filtro do render é o do acrílico ([ADR-0068](docs/adr/0068-efeito-acrilico-no-fundo-do-terminal.md)); reaproveitá-lo exige ADR.
- **Altura de barra vem de `chrome::bar_height(style)`**, sempre; nunca recalcular geometria localmente.
- **O mockup mostra o produto completo, não o v1.** Perfis de aba, paleta de comandos, cabeçalho de painel e a faixa de identidade da barra de título são `[v2]` ou recusados. Consulte a tabela de fases (seção 3) antes de construir algo que aparece no desenho ([ADR-0009](docs/adr/0009-referencia-visual-e-reconciliacao.md)).

## Processo de decisão arquitetural

Decisão aceita **não se edita**. Para mudar:

1. Escreva um ADR novo com o próximo número, referenciando o antigo em `Supersedes: ADR-NNNN`.
2. Marque o antigo como `Status: Superseded by ADR-MMMM`.
3. Atualize a tabela em [docs/adr/0000-template.md](docs/adr/0000-template.md) e a tabela de stack do README.

Correção de erro factual ou clareza no texto de um ADR aceito é permitida. Mudança de *decisão*, não. Revisão parcial de seção aceita entra como **blockquote de revisão** na própria seção, apontando o ADR novo.

## Armadilhas conhecidas

Versão curta. Texto completo, com sintoma e custo, no [Apêndice A de docs/descobertas.md](docs/descobertas.md).

**Ambiente de desenvolvimento (Windows)**

- **Criar/editar arquivo: sempre `Write`/`Edit`, nunca heredoc no `Bash`** — aspas, `$` e `\` são mutilados, às vezes em silêncio. Patch em vários pontos: script Python gravado com `Write`, rodado com `python caminho.py`. Python real: `%LOCALAPPDATA%\Programs\Python\Python312\python.exe` (o do `PATH` é atalho da Store).
- **`cargo build --release` falha com "Acesso negado"** se o `porecatu.exe` de `target/release` está aberto: use `--target-dir target/<temporário>`. Esse build precisa de `PORECATU_LOCALES=<repo>/locales`, senão abre mostrando identificadores (`tab_menu.close`).
- **Nunca matar `porecatu.exe` sem subir a árvore de processo a partir do próprio PID** (`Get-CimInstance Win32_Process`, `ParentProcessId`): a sessão do agente costuma rodar dentro de um. Só matar instância que o próprio agente lançou, com PID anotado no spawn.
- **Verificação ao vivo com input sintético:** foco (`SetForegroundWindow`), ação e captura **na mesma chamada de ferramenta**, conferindo `GetForegroundWindow`. Mouse sintético atravessa a proteção de foco; teclado às vezes. Capturar só a região da janela confirmada por PID, nunca a tela inteira; não abrir janela de teste grande sobre o trabalho do usuário. Script de captura precisa ser DPI-aware (`SetProcessDPIAware`, ou per-monitor v2 em monitor com escala ≠ 100%).
- **`cmd | grep -q` sob `pipefail` (GitHub Actions)** dá falso negativo por SIGPIPE: capture a saída numa variável primeiro.

**Dependências**

- **`alacritty_terminal`, `wgpu`, `toml_edit`**: pinar versão exata; subir é tarefa própria. `alacritty_terminal` isolado em `porecatu-term`; `toml_edit` isolado em `porecatu-config`.
- **`accesskit_winit`** é acoplado a uma versão de `winit`: subir os dois juntos e conferir `cargo tree -p porecatu-ui -i winit` (uma ocorrência só).
- **Clipboard no Wayland** é o ponto frágil do `arboard`; encapsulado em `porecatu-ui/clipboard.rs`, `copypasta` é o plano B, verificação pendente.

**Plataforma, PTY e event loop**

- **`winit`**: event loop na main thread; I/O de PTY nela trava a UI. Damage-driven: wakeups do PTY coalescidos por frame ([ADR-0007](docs/adr/0007-modelo-de-threading.md)).
- **`Wakeup` precisa de `(WindowId, TabId)`** — IDs são por workspace, e há um workspace por janela ([ADR-0015](docs/adr/0015-multiplas-janelas.md)).
- **ConPTY**: não fechar o pseudo-console no encerramento (deadlock) — `porecatu-term` mata o processo e faz `mem::forget` no `PtyHandle`; o `ProcessGroup` viaja separado dele. `TermEvent::Exit` vem de `try_wait`, não de EOF. Respostas DSR/DA/CPR vão direto ao canal de escrita do PTY.
- **Árvore de processo da aba** = união da lista do Job Object com varredura `sysinfo` a partir do PID raiz; nenhuma das duas sozinha basta ([ADR-0033](docs/adr/0033-job-object-encerramento-de-processo.md)).
- **`cwd` no Windows** só por OSC 7 ([ADR-0005](docs/adr/0005-persistencia-de-sessao.md)); `std::env::current_dir()` não é o diretório do usuário — default é `dirs::home_dir()`.
- **`Window::set_fullscreen` em janela maximizada (Windows)** não funciona e quebra a taskbar: `set_maximized(false)` antes, restaurar a maximização ao sair por conta própria.
- **Estado `NotStarted` não tem `TabRuntime`**: decidir `is_not_started()` **antes** de qualquer `self.tabs.get(&id)?`.

**Mouse e teclado**

- **Release do mouse sempre chega a `input::handle_mouse_button`**; estado de botão zerado no release e em `Focused(false)`.
- **`Shift` força seleção local** mesmo com o programa pedindo mouse ([ADR-0013](docs/adr/0013-mouse-selecao-e-clipboard.md)).
- **Coordenadas lógicas vs físicas**: `cell_at` divide por métrica lógica; todo ponto passa por `input::logical_point_in_content`. `App.cell_metrics` é recalculado também em `ScaleFactorChanged`.
- **`TERM=xterm-256color`**, nunca terminfo próprio ([ADR-0012](docs/adr/0012-identificacao-do-terminal.md)).

**Render e texto**

- **Nenhum diálogo nativo do sistema** — sete widgets próprios (aviso, diálogo, menu de contexto, tooltip, editor de grupo, barra de busca, popover de sessões) ([ADR-0014](docs/adr/0014-superficie-de-aviso-e-dialogo.md)).
- **Sem gestão de cor, hex cru**: surface com `remove_srgb_suffix()`, `glyphon` em `ColorMode::Web`, textura `Rgba8Unorm` nunca `Srgb` — senão dupla conversão (cores lavadas, texto esmaecido invisível).
- **Shader devolve premultiplicado → pipeline `PREMULTIPLIED_ALPHA_BLENDING`**, nunca `ALPHA_BLENDING`.
- **Quad reto (`radius <= 0`) usa `box_coverage` por eixo**; bordas de quad arredondadas ao pixel físico pelos dois cantos; borda de célula sempre de `row_top`/`col_left` (empate de 1 ULP gera costura). Blocos e box-drawing reto são `Quad`s de `porecatu-ui/src/box_glyphs.rs`, nunca glyph da fonte.
- **Borda de retângulo arredondado (`quad.wgsl`, `radius > 0`) nunca é pintada** — defeito aberto, corrigir depende de aval.
- **Medir texto no caminho quente sem cache é a armadilha de performance do projeto**: `TextMeasurer` não tem cache; um shaping por string, nunca por prefixo nem por célula. Teste de grade é em **em**, contra o avanço do `'M'`; célula em teste sai de `measure_mono_cell`, nunca de número fixo.
- **Fórmula de geometria nunca copiada** — copiada, diverge quando alguém mexe numa só.
- **Primeiro present de surface recém-criada pode não chegar à tela** (janela preta): `settle_frames` redesenha mais um quadro.
- **Carga pesada no arranque atrasa o primeiro prompt** mesmo em thread: imagem de fundo só começa no primeiro byte do PTY. Medir arranque sempre em A/B intercalado, trocando a ordem entre lotes.

Descobertas por fase (F1 a F6, pós-F6 e cada recurso fora de fase) vivem em [docs/descobertas.md](docs/descobertas.md), com os mesmos títulos que tinham aqui: citações em código e ADRs do tipo "CLAUDE.md, Descobertas na F2" se resolvem lá.

## Índice

- [docs/guia-do-usuario.md](docs/guia-do-usuario.md) — instalação, config, atalhos, integração de shell, o `.porecatu`, painéis, a convenção do `Shift`
- [docs/arquitetura.md](docs/arquitetura.md) — camadas, threading, fluxo de dados
- [docs/design/](docs/design/README.md) — registro visual: tokens, anatomia, tabela de fases, histórico. Descreve o binário ([ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md))
- [docs/adr/](docs/adr/) — decisões arquiteturais
- [docs/prd/](docs/prd/) — requisitos de produto: 000–006 e 009–019 aprovados; 007 e 008 rascunho (v2). PRD-010 é transversal (mouse, seleção, clipboard, avisos, diálogos, menus, janelas)
- [docs/roadmap.md](docs/roadmap.md) — fases e etapas, com a dívida de verificação de cada uma
- [docs/descobertas.md](docs/descobertas.md) — descobertas e armadilhas, texto completo
- [docs/reference/acoes.md](docs/reference/acoes.md) — catálogo **fechado** de ações; ação fora dele é erro de config
- [docs/reference/arquivo-de-projeto.md](docs/reference/arquivo-de-projeto.md) — formato do `.porecatu`
- [docs/reference/integracao-de-shell.md](docs/reference/integracao-de-shell.md) — snippets de OSC 7 por shell ([ADR-0039](docs/adr/0039-convite-a-integracao-de-shell.md))
- [docs/config/porecatu.example.toml](docs/config/porecatu.example.toml) — configuração de referência
- [docs/config/thm_default.toml](docs/config/thm_default.toml) e [thm_default_light.toml](docs/config/thm_default_light.toml) — os dois temas embutidos, fonte das cores ([ADR-0067](docs/adr/0067-temas-em-arquivos-proprios.md))
