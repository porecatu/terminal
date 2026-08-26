# Porecatu

[![docs](https://github.com/porecatu/terminal/actions/workflows/docs.yml/badge.svg)](https://github.com/porecatu/terminal/actions/workflows/docs.yml)
[![ci](https://github.com/porecatu/terminal/actions/workflows/ci.yml/badge.svg)](https://github.com/porecatu/terminal/actions/workflows/ci.yml)
[![licença: GPL-3.0-or-later](https://img.shields.io/badge/licen%C3%A7a-GPL--3.0--or--later-blue)](LICENSE)

Emulador de terminal cross-platform escrito em Rust, com foco em **organização de múltiplos terminais**: abas, grupos de abas nomeados e restauração de sessão.

> Status: **em implementação**. As fases F0 (esqueleto) e F1 (terminal único) do [roadmap](docs/roadmap.md) estão implementadas: `cargo run` abre uma janela com um terminal funcional — motor VT, PTY, render por GPU, teclado, mouse, seleção e clipboard. Ainda não há abas, grupos, configuração nem sessão persistente; a próxima fase é a **F2 (abas)**.

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
| F2 | Abas | próxima |
| F3–F6 | Grupos, configuração, sessão, polimento | não iniciadas |

O que já roda hoje:

- PTY cross-platform (`portable-pty`, ConPTY no Windows) com spawn, leitura, escrita, resize e encerramento
- `alacritty_terminal` encapsulado, com snapshot de grade de tipos próprios e cor não resolvida
- Três threads por terminal (leitura, escrita, observação do processo) e render **damage-driven**: terminal ocioso não gera frame
- Pipelines `wgpu` de quads (com cantos arredondados via SDF) e de texto (`glyphon`, atlas em cache), com as cinco faces do IBM Plex embutidas no binário
- Teclado com codificação xterm, `Ctrl`/`Alt`, DECCKM, bracketed paste e IME (tecla morta do ABNT2)
- Mouse reportado ao programa (modos 1000/1002/1003, encoding SGR 1006), seleção nos quatro modos com `Shift` forçando seleção local, cópia/cola via `arboard` e OSC 52 com leitura negada por default
- Rolagem de scrollback por teclado e por roda, com tela alternativa tratada

Ainda **não** existe: `porecatu-config` e `porecatu-session` são stubs, e `porecatu-core` tem só o `TabId`. Múltiplas abas, grupos, arquivo de configuração e restauração de sessão chegam nas fases seguintes.

O código da F1 está completo e o CI passa nas três plataformas (51 testes, `clippy -D warnings` limpo), mas o **critério de saída da fase ainda não foi fechado**: ele é interativo — `vim`/`htop`/`fzf` usáveis, mouse dentro do `htop`, copiar e colar no Wayland, acentuação ABNT2 — e a verificação manual aconteceu só em parte, e só no Windows. Lista do que falta em [docs/roadmap.md](docs/roadmap.md).

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
| Fontes | IBM Plex embutida (OFL-1.1) | [ADR-0016](docs/adr/0016-fontes-embutidas.md) |
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
├── assets/fonts/           # IBM Plex embutida no binário (OFL-1.1)
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

O binário embute cinco faces de **IBM Plex** (IBM Plex Mono 400/500, IBM Plex Sans 400/500/600), Copyright © 2017 IBM Corp. com Reserved Font Name "Plex", sob a [SIL Open Font License 1.1](assets/fonts/LICENSE-OFL.txt). Sem subsetting — decisão e motivo em [ADR-0016](docs/adr/0016-fontes-embutidas.md).

## Contribuindo

Convenções, processo de ADR e verificação local em [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
git clone https://github.com/porecatu/terminal.git
cd terminal

cargo run                    # abre a janela com um terminal
cargo test --workspace       # 51 testes
python scripts/verify-docs.py
```
