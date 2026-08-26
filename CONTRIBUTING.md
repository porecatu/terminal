# Contribuindo com o Porecatu

Obrigado pelo interesse. Este documento reúne as convenções que já estão
decididas, para que ninguém precise garimpá-las nos ADRs.

> **Estado atual: F0 e F1 implementadas.** `cargo run` abre uma janela com um
> terminal funcional. Não há abas, grupos, arquivo de configuração nem sessão
> persistente ainda — a próxima fase é a F2. Ver
> [docs/roadmap.md](docs/roadmap.md).

## Antes de abrir uma issue ou um PR

Boa parte do produto já está decidida por escrito. Vale conferir:

| Onde | O quê |
|---|---|
| [docs/prd/](docs/prd/) | Requisitos. PRD-000 a 005 e 010 aprovados; 006 a 009 em rascunho, fase v2 |
| [PRD-010](docs/prd/prd-010-interacao-e-superficie-de-app.md) | Transversal aos cinco recursos: mouse, seleção, clipboard, avisos, diálogos, menus e janelas |
| [docs/adr/](docs/adr/) | Decisões arquiteturais e o porquê de cada uma |
| [docs/design/](docs/design/README.md) | Alvo visual, tokens e tabela de fases |
| [docs/roadmap.md](docs/roadmap.md) | O que é escopo de agora |
| [docs/reference/acoes.md](docs/reference/acoes.md) | Ações vinculáveis a teclas. Conjunto **fechado**: ação nova exige RF ou ADR antes |
| [PRD-000](docs/prd/prd-000-visao-de-produto.md) | Não-objetivos do v1 — cada um é decisão, não esquecimento |

Se sua ideia contraria uma decisão aceita, diga isso explicitamente. Não é
impedimento: é o começo de um ADR novo.

## Idioma

Regra que costuma surpreender, então vem primeiro:

- **Código, identificadores, comentários de código e nomes de arquivo de código: inglês.**
- **Documentação, mensagens de commit, issues e PRs: português do Brasil.**

O projeto usa **"abas"**, não "guias" — inclusive em futuras strings de
interface ([ADR-0009](docs/adr/0009-referencia-visual-e-reconciliacao.md)).

## Commits

[Conventional Commits](https://www.conventionalcommits.org), em pt-BR:

```
feat: agrupa abas selecionadas com nome e cor
fix: preserva última linha de saída ao encerrar shell no Windows
docs: registra reconciliação com o design canvas
refactor: isola alacritty_terminal atrás de porecatu-term
test: cobre round-trip de sessão com schema antigo
chore: atualiza dependências de desenvolvimento
ci: adiciona dependências de sistema do winit no Linux
```

Prefixos: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`.

## Processo de ADR

O projeto tem uma regra estrita: **decisão aceita não se edita.**

Para mudar uma decisão:

1. Escreva um ADR novo com o próximo número, com `Supersedes: ADR-NNNN`
2. Marque o antigo como `Status: Superseded by ADR-MMMM`
3. Atualize o índice em [docs/adr/0000-template.md](docs/adr/0000-template.md)
4. Se a decisão aparece na tabela de stack do README, atualize lá também

Corrigir erro factual ou melhorar a clareza do texto de um ADR aceito é
permitido. Mudar a **decisão**, não.

O template está no próprio [0000-template.md](docs/adr/0000-template.md).
Alternativa listada sem motivo de rejeição não conta como alternativa
considerada.

## Aparência: nada de valor inventado

Antes de mexer em qualquer coisa de chrome, leia
[docs/design/especificacao-visual.md](docs/design/especificacao-visual.md).

Nenhuma cor, dimensão, raio ou espaçamento é inventado — sai da tabela de
tokens ou do [porecatu.example.toml](docs/config/porecatu.example.toml), que
traz os mesmos valores como default. O binário com a configuração padrão deve
bater com [o mockup](docs/design/mockup-estatico.html); divergência visível é
bug de implementação.

O mockup mostra o produto **completo**, não o v1. Consulte a tabela de fases
antes de construir algo que aparece no desenho.

## Verificação local

Código:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --workspace
cargo test --workspace
```

Warning de clippy é erro. O CI roda os quatro em Windows, Linux e macOS.

Documentação:

```bash
python scripts/verify-docs.py
```

Checa links relativos, o TOML de exemplo, cores sem origem na especificação
visual e a cobertura da tabela de fases. É o mesmo que o CI roda.

**Verificação interativa não é opcional.** Boa parte do comportamento do
terminal — teclado, mouse, seleção, clipboard, resize com TUI aberta — não é
coberta por teste automatizado e não é testável com input sintético (a
proteção de foco do Windows bloqueia isso). Se sua mudança toca esse caminho,
abra o app de verdade, exercite `vim`/`htop`/`fzf` e diga no PR o que
verificou e o que não conseguiu verificar.

## Ambiente

- Rust na versão pinada em [rust-toolchain.toml](rust-toolchain.toml) — o
  `rustup` instala sozinho ao entrar no diretório
- No Linux, as bibliotecas de sistema listadas em
  [.github/workflows/ci.yml](.github/workflows/ci.yml) (X11/Wayland, GL,
  fontconfig)
- Python 3.11 ou superior (o script de docs usa `tomllib`)
- Git com `core.autocrlf` deixado em paz — o [.gitattributes](.gitattributes)
  já normaliza para LF

## Licença

O Porecatu é distribuído sob **GPL-3.0-or-later** — ver [LICENSE](LICENSE) e
[ADR-0010](docs/adr/0010-licenciamento.md).

Ao enviar uma contribuição, você concorda em licenciá-la sob os mesmos termos.
Arquivos de código levam o cabeçalho:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
```
