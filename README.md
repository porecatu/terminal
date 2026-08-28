# Porecatu

[![docs](https://github.com/porecatu/terminal/actions/workflows/docs.yml/badge.svg)](https://github.com/porecatu/terminal/actions/workflows/docs.yml)
[![ci](https://github.com/porecatu/terminal/actions/workflows/ci.yml/badge.svg)](https://github.com/porecatu/terminal/actions/workflows/ci.yml)
[![licença: GPL-3.0-or-later](https://img.shields.io/badge/licen%C3%A7a-GPL--3.0--or--later-blue)](LICENSE)

Emulador de terminal cross-platform escrito em Rust, com foco em **organização de múltiplos terminais**: abas, grupos de abas nomeados e restauração de sessão.

> Status: **em implementação**. As fases F0 (esqueleto), F1 (terminal único), F2 (abas) e F3 (grupos) do [roadmap](docs/roadmap.md) estão implementadas — a F3 **exceto o RF-2.21** (`group.next`/`group.prev`), que a mantém aberta. `cargo run` abre uma janela com abas e grupos de terminal funcionais: motor VT, PTY, render por GPU, teclado, mouse, seleção, clipboard, ciclo de vida de aba, overflow da barra, seleção múltipla, grupos nomeados e coloridos com colapso, editor de grupo, arraste entre grupos, animação de reflui, menu de contexto, tooltip, aviso, diálogo de confirmação e uma segunda janela. Ainda não há arquivo de configuração nem sessão persistente.

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
| F0 | Workspace Cargo, janela `winit` + surface `wgpu`, CI nas três plataformas | implementada |
| F1 | Terminal único: PTY, motor VT, threading, render de texto, teclado, mouse, seleção, clipboard | implementada |
| F2 | Abas: modelo de workspace, barra de abas, ciclo de vida, overflow, arraste, widgets de chrome, segunda janela | implementada |
| F3 | Grupos: modelo explícito, seleção múltipla, pílula e cápsula de cor, colapso, editor de grupo, arraste entre grupos, animação | implementada exceto RF-2.21 |
| F4–F6 | Configuração, sessão, polimento | não iniciadas |

O que já roda hoje:

- PTY cross-platform (`portable-pty`, ConPTY no Windows) com spawn, leitura, escrita, resize e encerramento
- `alacritty_terminal` encapsulado, com snapshot de grade de tipos próprios e cor não resolvida
- Três threads por terminal (leitura, escrita, observação do processo) e render **damage-driven**: terminal ocioso não gera frame
- Pipelines `wgpu` de quads (com cantos arredondados via SDF) e de texto (`glyphon`, atlas em cache), com as cinco faces da Iosevka e a de ícones embutidas no binário
- Teclado com codificação xterm, `Ctrl`/`Alt`, DECCKM, bracketed paste e IME (tecla morta do ABNT2)
- Mouse reportado ao programa (modos 1000/1002/1003, encoding SGR 1006), seleção nos quatro modos com `Shift` forçando seleção local, cópia/cola via `arboard` e OSC 52 com leitura negada por default
- Rolagem de scrollback por teclado e por roda, com tela alternativa tratada
- Abas com ciclo de vida completo: criar herdando o `cwd` (OSC 7), fechar com confirmação quando há programa de tela cheia, navegar por sequência e por índice, renomear inline, e estado `Exited` para aba cujo shell saiu
- Título com precedência — customizado, depois OSC 0/2, depois nome do shell — sincronizado com o título da janela
- Barra de abas com layout e hit-testing como funções puras, testáveis sem GPU e sem janela; reordenação por arraste e por teclado; overflow por rolagem da trilha, com indicador de abas fora da vista e o botão de nova aba numa zona fixa que não rola; indicadores de atividade e de campainha
- Grupos de abas: nomeados, coloridos por uma paleta de seis, colapsáveis (as abas saem da barra e da navegação sequencial, os processos seguem vivos), com contador e indicador agregado na pílula, cápsula de cor por trás das abas e sublinhado por grupo
- Seleção múltipla de abas (`Ctrl`/`Cmd`+clique alterna, `Shift`+clique estende), `Ctrl+Shift+G` para agrupar, arraste de aba entre grupos e arraste da pílula para mover o grupo inteiro
- Animação de reflui da trilha ao formar grupo e ao colapsar/expandir, dirigida pelo event loop — sem thread de timer e sem loop de render contínuo
- Cinco widgets de chrome próprios, desenhados por cima do terminal: aviso do app, diálogo de confirmação, menu de contexto (de aba e de grupo), tooltip e editor de grupo. Nenhum diálogo nativo do sistema
- Múltiplas janelas: cada uma com seu conjunto de abas e sua surface, nascendo em cascata a partir da que a criou

Ainda **não** existe: `porecatu-config` e `porecatu-session` são stubs (só o cabeçalho SPDX). Arquivo de configuração e restauração de sessão chegam nas fases seguintes — enquanto `porecatu-config` não existe, os valores de aparência entram como constantes citando no comentário a chave TOML de origem.

O CI passa nas três plataformas (**241 testes**, `clippy -D warnings` limpo), mas o **critério de saída das três fases não foi fechado na parte interativa**: ele exige gesto de verdade — `vim`/`htop`/`fzf` usáveis, mouse dentro do `htop`, copiar e colar no Wayland, acentuação ABNT2, arraste de aba e de grupo, seleção múltipla, editor de grupo, duas janelas em monitores de DPI diferente — e a proteção de foco do Windows bloqueia input sintético, então a verificação aconteceu só em parte, e só no Windows. A F3 tem ainda uma pendência de código: o RF-2.21 (`group.next`/`group.prev`) não foi implementado. Lista do que falta em [docs/roadmap.md](docs/roadmap.md).

## Design

O alvo visual está desenhado e importado em [`docs/design/`](docs/design/README.md).

- [**Mockup estático**](docs/design/mockup-estatico.html) — abre com duplo clique, sem dependências. É o que se deixa aberto ao lado do editor
- [**Especificação visual**](docs/design/especificacao-visual.md) — tokens, anatomia por componente, tabela de fases, rastreabilidade design ↔ requisito
- [Canvas original](https://claude.ai/design/p/b0bc7589-f967-40cb-98ab-caef4070a95a?file=Terminal+Multiplataforma.dc.html) — interativo, em claude.ai

Os valores default do [`porecatu.example.toml`](docs/config/porecatu.example.toml) vêm dessa especificação: o binário com a configuração padrão deve bater com o mockup.

> O mockup mostra o produto **completo**, não o v1. Painéis divididos, perfis, paleta de comandos, painel de configurações, barra de status e barra de título customizada são `[v2]`. Consulte a tabela de fases antes de implementar. Ver [ADR-0009](docs/adr/0009-referencia-visual-e-reconciliacao.md).

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
| Fontes | Iosevka (OFL-1.1) + Lucide (ISC), embutidas | [ADR-0025](docs/adr/0025-iosevka-no-lugar-da-ibm-plex.md) |
| Referência visual | design canvas importado | [ADR-0009](docs/adr/0009-referencia-visual-e-reconciliacao.md) |
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
├── assets/fonts/           # Iosevka + Lucide embutidas no binário
└── docs/
```

Versões travadas por igualdade exata onde a API quebra a cada release: `alacritty_terminal = "=0.26.0"` e `wgpu = "=30.0.1"` ([ADR-0002](docs/adr/0002-motor-vte.md), [ADR-0001](docs/adr/0001-stack-de-gui.md)). Toolchain em `rust-toolchain.toml`, com job canário semanal contra a stable do dia ([ADR-0011](docs/adr/0011-toolchain-rust.md)).

## Documentação

- [CLAUDE.md](CLAUDE.md) — guia operacional para agentes e contribuidores
- [docs/arquitetura.md](docs/arquitetura.md) — camadas, threading, fluxo de dados
- [docs/design/](docs/design/README.md) — alvo visual: mockup, tokens, anatomia, fases
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

O binário embute cinco faces de texto da **[Iosevka](https://typeof.net/Iosevka/)** (Iosevka Fixed 400/500 no terminal, Iosevka Aile 400/500/600 no chrome), Copyright © 2015-2026 Renzhi Li, sob a [SIL Open Font License 1.1](assets/fonts/LICENSE-OFL-iosevka.txt). São recortadas por [`scripts/subset-fonts.py`](scripts/subset-fonts.py) — 48 MB viram 2.1 MB —, o que a OFL da Iosevka permite por não ter cláusula de Reserved Font Name. Decisão e medições em [ADR-0025](docs/adr/0025-iosevka-no-lugar-da-ibm-plex.md).

Embute também uma face de ícones, **[Lucide](https://lucide.dev)**, Copyright © Lucide Contributors, sob a [licença ISC](assets/fonts/LICENSE-ISC-lucide.txt) — é ela que desenha o botão de fechar, o caret do grupo e os chevrons de overflow, que nenhuma das faces de texto cobre. Decisão em [ADR-0024](docs/adr/0024-face-de-icones.md).

## Contribuindo

Convenções, processo de ADR e verificação local em [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
git clone https://github.com/porecatu/terminal.git
cd terminal

cargo run                    # abre a janela com um terminal
cargo test --workspace       # 241 testes
python scripts/verify-docs.py
```
