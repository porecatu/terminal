# O que muda

<!-- Uma ou duas frases. O "porquê" importa mais que o "o quê". -->

Resolve #

## Checklist

Marque o que se aplica; risque o que não. Cada item vem de uma regra já
registrada em [CLAUDE.md](../blob/main/CLAUDE.md) ou nos ADRs.

### Sempre

- [ ] `python scripts/verify-docs.py` passa
- [ ] Commits em [Conventional Commits](https://www.conventionalcommits.org), em pt-BR
- [ ] Documentação em pt-BR; código, identificadores e comentários de código em inglês

### Se mexeu em decisão arquitetural

- [ ] Não editei ADR aceito para **mudar decisão** — escrevi ADR novo com `Supersedes: ADR-NNNN` e marquei o antigo
- [ ] Índice em `docs/adr/0000-template.md` atualizado

### Se mexeu em aparência ou no chrome

- [ ] Nenhum valor de aparência inventado — cores, dimensões e raios vêm da [tabela de tokens](../blob/main/docs/design/especificacao-visual.md) ou do `porecatu.example.toml`
- [ ] Conferi o resultado contra [`docs/design/mockup-estatico.html`](../blob/main/docs/design/mockup-estatico.html)
- [ ] Não implementei elemento marcado `[v2]` na tabela de fases

### Se mexeu na configuração

- [ ] Toda chave nova tem default e aparece em algum PRD
- [ ] Config ausente ou inválida continua não derrubando o app (ADR-0003)

### Se mexeu em código Rust

- [ ] Cabeçalho `// SPDX-License-Identifier: GPL-3.0-or-later` em cada arquivo novo
- [ ] `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --workspace` passam
- [ ] Respeitei o grafo de dependências entre crates de CLAUDE.md
- [ ] Nenhum I/O bloqueante na main thread (ADR-0007)

## Notas para quem revisa

<!-- Trade-offs, o que deixei de fora de propósito, o que merece atenção. -->
