# CLAUDE.md

Guia operacional do projeto. Leia antes de mexer em qualquer coisa.

## O que é

**Porecatu** — emulador de terminal cross-platform em Rust. Diferencial: gestão de muitos terminais (abas, grupos nomeados, sessão persistente), não conformidade VT.

**Estado atual: fase de design. Não existe código.** O repositório contém apenas documentação. Antes de escrever a primeira linha de Rust, leia [docs/arquitetura.md](docs/arquitetura.md) e os ADRs.

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
| `porecatu-term` | `pty` | GUI |
| `porecatu-render` | — | `core`, `config`, `term` |
| `porecatu-ui` | `core`, `config`, `term`, `render` | — |
| `porecatu-session` | `core` | GUI, PTY |

`porecatu-render` **não conhece o domínio**: recebe primitivas de desenho (quad, retângulo arredondado, run de texto) e nada sobre abas ou grupos. Quem traduz config+estado em primitivas é `porecatu-ui`.

## Comandos

Documentação — roda hoje, e é o que o CI executa nesta fase:

```bash
python scripts/verify-docs.py
```

Código, a partir da F0:

```bash
cargo build --workspace
cargo test  --workspace
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
```

CI roda os quatro nas três plataformas. Warning de clippy é erro.

O workflow de Rust (`.github/workflows/ci.yml`) já existe e está **dormindo**: um job `detect` pula a matriz enquanto não houver `Cargo.toml` na raiz. Criar o workspace na F0 liga tudo sozinho — não é preciso editar o workflow.

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

Nove requisitos do v1 não têm desenho aprovado (indicadores de atividade e campainha, estado de arraste, aba selecionada, entre outros). Estão listados na seção 4.2 da especificação. Para esses, vale o julgamento de quem implementa — mas ainda usando os tokens existentes, nunca cores novas.

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
- **Clipboard no Wayland** é o ponto frágil do `arboard`. Encapsular num só lugar; `copypasta` é o plano B, e a verificação é tarefa da F1, não da F6.
- **Nenhum diálogo nativo do sistema.** `MessageBox` e `NSAlert` bloqueiam o event loop e são a única superfície que a config do usuário não alcança. Aviso, diálogo e menu de contexto são widgets nossos. Ver [ADR-0014](docs/adr/0014-superficie-de-aviso-e-dialogo.md).

## Índice

- [docs/arquitetura.md](docs/arquitetura.md) — camadas, threading, fluxo de dados
- [docs/design/](docs/design/README.md) — alvo visual: tokens, anatomia, tabela de fases
- [docs/adr/](docs/adr/) — decisões arquiteturais
- [docs/prd/](docs/prd/) — requisitos de produto (000–005 aprovados; 006–009 rascunho, fase v2)
- [docs/roadmap.md](docs/roadmap.md) — fases de entrega
- [docs/reference/acoes.md](docs/reference/acoes.md) — catálogo **fechado** de ações; ação fora dele é erro de config
- [docs/config/porecatu.example.toml](docs/config/porecatu.example.toml) — configuração de referência
