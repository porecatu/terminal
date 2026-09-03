# Porecatu

[![docs](https://github.com/porecatu/terminal/actions/workflows/docs.yml/badge.svg)](https://github.com/porecatu/terminal/actions/workflows/docs.yml)
[![ci](https://github.com/porecatu/terminal/actions/workflows/ci.yml/badge.svg)](https://github.com/porecatu/terminal/actions/workflows/ci.yml)
[![licença: GPL-3.0-or-later](https://img.shields.io/badge/licen%C3%A7a-GPL--3.0--or--later-blue)](LICENSE)

Emulador de terminal cross-platform escrito em Rust, com foco em **organização de múltiplos terminais**: abas, grupos de abas nomeados e restauração de sessão.

> Status: **em implementação**. As fases F0 (esqueleto), F1 (terminal único), F2 (abas), F3 (grupos) e F4 (configuração) do [roadmap](docs/roadmap.md) estão **fechadas**; a próxima é a F5 (sessão). `cargo run` abre uma janela com abas e grupos de terminal funcionais: motor VT, PTY, render por GPU, teclado, mouse, seleção, clipboard, ciclo de vida de aba, overflow da barra, seleção múltipla, grupos nomeados e coloridos com colapso, navegação entre grupos pela última aba visitada de cada um, editor de grupo, arraste entre grupos, animação de reflui, menu de contexto, tooltip, aviso, diálogo de confirmação e uma segunda janela — tudo governado por um arquivo de configuração (`porecatu.toml`) com recarga a quente. Restauração de sessão ainda não existe.

---

## Por que

Emuladores modernos (Alacritty, WezTerm, Windows Terminal, Kitty) resolvem bem *renderização* e *conformidade VT*. O que continua ruim é a **gestão de muitos terminais abertos ao mesmo tempo**: abas viram uma fileira indistinguível de "bash", grupos não existem, e fechar a janela perde todo o contexto de trabalho.

Porecatu ataca esse problema:

- abas com **grupos nomeados e coloridos**, colapsáveis;
- **sessão persistente**: reabrir o app restaura abas e grupos nos mesmos diretórios;
- **aparência totalmente configurável** — tanto o *chrome* (barra de abas, grupos) quanto o conteúdo do terminal (cores, fonte, ligaduras).

## Recursos-alvo (v1)

| # | Recurso | PRD | Referência visual |
|---|---------|-----|---|
| 1 | Abas para múltiplos terminais na mesma janela | [PRD-001](docs/prd/prd-001-abas.md) | [anatomia 2.2, 2.5](docs/design/especificacao-visual.md) |
| 2 | Agrupamento de abas com nome e cor | [PRD-002](docs/prd/prd-002-grupos-de-abas.md) | [anatomia 2.3, 2.4, 2.10](docs/design/especificacao-visual.md) |
| 3 | Persistência e restauração de sessão | [PRD-003](docs/prd/prd-003-persistencia-de-sessao.md) | — |
| 4 | Aparência configurável do chrome (abas/grupos) | [PRD-004](docs/prd/prd-004-aparencia-do-chrome.md) | [tokens, seção 1](docs/design/especificacao-visual.md) |
| 5 | Cores e fontes do terminal configuráveis | [PRD-005](docs/prd/prd-005-aparencia-do-terminal.md) | [anatomia 2.7](docs/design/especificacao-visual.md) |

Visão de produto completa: [PRD-000](docs/prd/prd-000-visao-de-produto.md).

## Estado atual

| Fase | O que entrega | Status |
|---|---|---|
| F0 | Workspace Cargo, janela `winit` + surface `wgpu`, CI nas três plataformas | **fechada** |
| F1 | Terminal único: PTY, motor VT, threading, render de texto, teclado, mouse, seleção, clipboard | **fechada** |
| F2 | Abas: modelo de workspace, barra de abas, ciclo de vida, overflow, arraste, widgets de chrome, segunda janela | **fechada** |
| F3 | Grupos: modelo explícito, seleção múltipla, pílula e cápsula de cor, colapso, editor de grupo, arraste entre grupos, animação, navegação entre grupos | **fechada** |
| F4 | Configuração: `porecatu-config`, hot reload, `[keybindings]`, temas, zoom, hover e sombra aprovados | **fechada** (dívida na etapa 6, ver abaixo) |
| F5–F6 | Sessão, polimento | não iniciadas |

As quatro primeiras fecharam com **dívida de verificação interativa** — ver o parágrafo do CI abaixo.

O que já roda hoje:

- PTY cross-platform (`portable-pty`, ConPTY no Windows) com spawn, leitura, escrita, resize e encerramento
- `alacritty_terminal` encapsulado, com snapshot de grade de tipos próprios e cor não resolvida
- Três threads por terminal (leitura, escrita, observação do processo) e render **damage-driven**: terminal ocioso não gera frame
- Pipelines `wgpu` de quads (com cantos arredondados via SDF) e de texto (`glyphon`, atlas em cache), com a Iosevka Fixed (terminal e chrome) e a face de ícones embutidas no binário
- Teclado com codificação xterm, `Ctrl`/`Alt`, DECCKM, bracketed paste e IME (tecla morta do ABNT2)
- Mouse reportado ao programa (modos 1000/1002/1003, encoding SGR 1006), seleção nos quatro modos com `Shift` forçando seleção local, cópia/cola via `arboard` e OSC 52 com leitura negada por default
- Rolagem de scrollback por teclado e por roda, com tela alternativa tratada
- Abas com ciclo de vida completo: criar herdando o `cwd` (OSC 7), fechar com confirmação quando há programa de tela cheia, navegar por sequência e por índice, renomear inline, e estado `Exited` para aba cujo shell saiu
- Título com precedência — customizado, depois OSC 0/2, depois nome do shell — sincronizado com o título da janela
- Barra de abas com layout e hit-testing como funções puras, testáveis sem GPU e sem janela; reordenação por arraste e por teclado; overflow por rolagem da trilha, com indicador de abas fora da vista e uma zona fixa à direita que não rola (hoje o botão de configurações, inerte — painel de configurações é `[v2]`); indicadores de atividade e de campainha
- Grupos de abas: nomeados, coloridos por uma paleta de seis, colapsáveis (as abas saem da barra e da navegação sequencial, os processos seguem vivos), com indicador agregado na pílula e a cápsula de cor cheia por trás das abas — que continua desenhada com o grupo colapsado, porque é ela que diz de que cor o grupo é. Um "+" por grupo cria aba dentro dele
- Seleção múltipla de abas (`Ctrl`/`Cmd`+clique alterna, `Shift`+clique estende), arraste de aba entre grupos e arraste da pílula para mover o grupo inteiro
- Navegação **entre grupos** (`Ctrl+Shift+PageDown`/`PageUp`), caindo na última aba visitada de cada um e pulando grupo colapsado; mais `Ctrl+Shift+G` para agrupar, `Ctrl+Shift+U` para desagrupar, `Ctrl+Shift+E` para renomear e `Ctrl+Shift+K` para colapsar. Todos rebindáveis via `[keybindings]`, incluindo os defaults de macOS (F4 etapa 5)
- Animação de reflui da trilha ao formar grupo e ao colapsar/expandir, dirigida pelo event loop — sem thread de timer e sem loop de render contínuo
- Cinco widgets de chrome próprios, desenhados por cima do terminal: aviso do app, diálogo de confirmação, menu de contexto (de aba e de grupo), tooltip e editor de grupo. Nenhum diálogo nativo do sistema
- Múltiplas janelas: cada uma com seu conjunto de abas e sua surface, nascendo em cascata a partir da que a criou

A **F4 (configuração) fechou** em seis etapas: `porecatu-config` (structs `serde`, defaults completos, resolução de caminho por `PORECATU_CONFIG` ou o padrão da plataforma — a flag `--config` já tem a precedência implementada em `resolve_config_path`, mas nada no binário lê `argv` ainda para chamá-la com um valor), toda a barra e o terminal lendo `Config` em vez de constante, hot reload (`notify`, três classes de chave — aplica a quente, aplica com recálculo de grade, exige reinício), `enum Action` + parser de `[keybindings]` em três níveis (comum → plataforma, com os defaults de macOS finalmente respondendo), temas nomeados, zoom de fonte por atalho, `animations = false`, e as duas mudanças visuais aprovadas ([ADR-0032](docs/adr/0032-interface-do-v1-fechada.md)): hover por brilho e sombra em camadas nos cinco widgets de chrome. Dívida registrada em [docs/roadmap.md](docs/roadmap.md): merge de tema ainda não cobre cores de grupo/widgets, zoom por atalho é sempre do processo (não por aba), a entrada de cor por hexadecimal do editor de grupo não foi implementada, e a flag `--config` não é lida de `argv` (só `PORECATU_CONFIG` e o caminho padrão da plataforma funcionam hoje). `porecatu-session` continua stub (só o cabeçalho SPDX) — restauração de sessão é a F5.

O CI passa nas três plataformas (**404 testes**, `clippy -D warnings` limpo). A **verificação interativa é dívida assumida**: o critério de saída das fases exigia gesto de verdade — `vim`/`htop`/`fzf` usáveis, mouse dentro do `htop`, copiar e colar no Wayland, acentuação ABNT2, arraste de aba e de grupo, seleção múltipla, editor de grupo, duas janelas em monitores de DPI diferente, teclado de verdade nos atalhos —, e a proteção de foco do Windows bloqueia input sintético de teclado (mouse sintético funciona e foi usado para confirmar hover/sombra por captura de tela). As fases fecharam com cobertura automatizada mais smoke test, e o que não foi confirmado está escrito por fase em [docs/roadmap.md](docs/roadmap.md). A próxima fase é a F5 (sessão).

## Design

O registro visual está em [`docs/design/`](docs/design/README.md) — e o **alvo é o binário**, não o desenho (ver abaixo).

- [**Mockup estático**](docs/design/mockup-estatico.html) — o ponto de partida do desenho, abre com duplo clique e sem dependências. Referência **histórica**: onde ele e o binário divergem, o binário é o alvo ([ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md))
- [**Especificação visual**](docs/design/especificacao-visual.md) — tokens, anatomia por componente, tabela de fases, rastreabilidade design ↔ requisito. Descreve o que o binário desenha hoje, e é atualizada quando ele muda
- [Canvas original](https://claude.ai/design/p/b0bc7589-f967-40cb-98ab-caef4070a95a?file=Terminal+Multiplataforma.dc.html) — interativo, em claude.ai

**A interface como está é o alvo.** O que o binário desenha com a configuração padrão é normativo para a aparência; a especificação registra esses valores e o [`porecatu.example.toml`](docs/config/porecatu.example.toml) os carrega como default. Nenhuma mudança de aparência é feita sem aval do dono do produto — inclusive as que a documentação já chamou de "dívida a pagar" ([ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md), que supersede em parte o [ADR-0009](docs/adr/0009-referencia-visual-e-reconciliacao.md)). Isso não afrouxa a regra de procedência: valor de aparência sem origem declarada na especificação continua sendo erro.

> O mockup mostra o produto **completo**, não o v1. Painéis divididos, perfis, paleta de comandos, painel de configurações e barra de status são `[v2]`. A faixa de identidade da barra de título (logo, nome do app, título da aba ativa) também segue `[v2]`; os controles de janela e o resize sem decoração nativa já são `[v1]` fora do macOS ([ADR-0027](docs/adr/0027-controles-de-janela-e-resize-proprios.md)). Consulte a tabela de fases antes de implementar. Ver [ADR-0009](docs/adr/0009-referencia-visual-e-reconciliacao.md) e [ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md).

## Stack

| Camada | Escolha | ADR |
|--------|---------|-----|
| Janela + eventos | `winit` | [ADR-0001](docs/adr/0001-stack-de-gui.md) |
| Render GPU | `wgpu` | [ADR-0001](docs/adr/0001-stack-de-gui.md) |
| Shaping/atlas de texto | `glyphon` + `cosmic-text` | [ADR-0001](docs/adr/0001-stack-de-gui.md) |
| Motor VT / grid | `alacritty_terminal` | [ADR-0002](docs/adr/0002-motor-vte.md) |
| Configuração | TOML (`serde` + `toml`) | [ADR-0003](docs/adr/0003-formato-de-configuracao.md) |
| PTY | `portable-pty` (ConPTY no Windows) | [ADR-0004](docs/adr/0004-pty-cross-platform.md) |
| Persistência de sessão | JSON versionado em state dir | [ADR-0005](docs/adr/0005-persistencia-de-sessao.md) |
| Clipboard | `arboard` | [ADR-0013](docs/adr/0013-mouse-selecao-e-clipboard.md) |
| Fontes | Iosevka Fixed (OFL-1.1, terminal e chrome) + Lucide (ISC), embutidas | [ADR-0026](docs/adr/0026-chrome-unificado-em-iosevka-fixed.md) |
| Ícone do app | `png` (decodifica em runtime) + `winres` num `build.rs` (recurso PE no Windows) | — |
| Caminhos do usuário | `dirs` (home como diretório inicial de aba; caminho de config) | [ADR-0003](docs/adr/0003-formato-de-configuracao.md) |
| Referência visual | o binário; design canvas como histórico | [ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md) |
| Toolchain | stable pinada, edition 2024 | [ADR-0011](docs/adr/0011-toolchain-rust.md) |
| Licença | GPL-3.0-or-later | [ADR-0010](docs/adr/0010-licenciamento.md) |

Plataformas-alvo do v1: **Windows 10+, Linux (X11/Wayland), macOS 12+**.

## Arquitetura

Workspace Cargo multi-crate. Detalhes em [docs/arquitetura.md](docs/arquitetura.md).

```
porecatu/
├── src/main.rs             # binário: chama porecatu_ui::run()
├── crates/
│   ├── porecatu-core/      # modelo de domínio: Workspace, Group, Tab, IDs
│   ├── porecatu-config/    # parse TOML, defaults, hot reload
│   ├── porecatu-pty/       # abstração de PTY sobre portable-pty
│   ├── porecatu-term/      # wrapper de alacritty_terminal, snapshot de grid
│   ├── porecatu-render/    # wgpu: pipelines de quad, texto, arredondamento
│   ├── porecatu-ui/        # event loop winit, layout, hit-testing, roteamento de input
│   └── porecatu-session/   # serialização/restauração de sessão
├── build.rs                # embute o .ico como recurso PE no Windows (winres)
├── assets/
│   ├── fonts/              # Iosevka + Lucide embutidas no binário
│   └── icon/               # porecatu.png (embutido) e porecatu.ico
└── docs/
```

Versões travadas por igualdade exata onde a API quebra a cada release: `alacritty_terminal = "=0.26.0"` e `wgpu = "=30.0.1"` ([ADR-0002](docs/adr/0002-motor-vte.md), [ADR-0001](docs/adr/0001-stack-de-gui.md)). Toolchain em `rust-toolchain.toml`, com job canário semanal contra a stable do dia ([ADR-0011](docs/adr/0011-toolchain-rust.md)).

## Documentação

- [CLAUDE.md](CLAUDE.md) — guia operacional para agentes e contribuidores
- [docs/arquitetura.md](docs/arquitetura.md) — camadas, threading, fluxo de dados
- [docs/design/](docs/design/README.md) — registro visual: tokens, anatomia, fases, histórico de decisões (o mockup é histórico, ver [ADR-0028](docs/adr/0028-o-binario-como-referencia-visual.md))
- [docs/adr/](docs/adr/) — Architecture Decision Records
- [docs/prd/](docs/prd/) — Product Requirement Documents
- [docs/roadmap.md](docs/roadmap.md) — fases de entrega
- [docs/reference/acoes.md](docs/reference/acoes.md) — catálogo fechado de ações vinculáveis a teclas
- [docs/config/porecatu.example.toml](docs/config/porecatu.example.toml) — configuração de referência comentada

## Nome

Porecatu é uma cidade do norte do Paraná. O nome vem do tupi e significa **"salto bonito"**.

## Licença

**GPL-3.0-or-later.** Texto integral em [LICENSE](LICENSE); a decisão e suas alternativas em [ADR-0010](docs/adr/0010-licenciamento.md).

Copyright © 2026 Leonardo Otaviano Pedrozo.

A escolha da versão 3 não é estética: `winit` e `alacritty_terminal` são Apache-2.0, e a FSF declara Apache-2.0 **incompatível com a GPLv2**. A v3 é compatível com toda a stack travada nos ADRs.

### Fontes embutidas

O binário embute duas faces de texto da **[Iosevka](https://typeof.net/Iosevka/)** (Iosevka Fixed 400/500, terminal **e** chrome — mesma família nos dois, [ADR-0026](docs/adr/0026-chrome-unificado-em-iosevka-fixed.md)), Copyright © 2015-2026 Renzhi Li, sob a [SIL Open Font License 1.1](assets/fonts/LICENSE-OFL-iosevka.txt). São recortadas por [`scripts/subset-fonts.py`](scripts/subset-fonts.py), o que a OFL da Iosevka permite por não ter cláusula de Reserved Font Name. Decisão e medições em [ADR-0025](docs/adr/0025-iosevka-no-lugar-da-ibm-plex.md), unificação de família no [ADR-0026](docs/adr/0026-chrome-unificado-em-iosevka-fixed.md).

Embute também uma face de ícones, **[Lucide](https://lucide.dev)**, Copyright © Lucide Contributors, sob a [licença ISC](assets/fonts/LICENSE-ISC-lucide.txt) — é ela que desenha o botão de fechar, o caret do grupo e os chevrons de overflow, que nenhuma das faces de texto cobre. Decisão em [ADR-0024](docs/adr/0024-face-de-icones.md).

## Contribuindo

Convenções, processo de ADR e verificação local em [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
git clone https://github.com/porecatu/terminal.git
cd terminal

cargo run                    # abre a janela com um terminal
cargo test --workspace       # 404 testes
python scripts/verify-docs.py
```
