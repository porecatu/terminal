# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).
Versionamento seguirá [SemVer](https://semver.org/lang/pt-BR/) a partir do
primeiro release.

> **Nenhuma versão foi publicada ainda.** O Porecatu está em fase de
> documentação: não há código nem binário. O [roadmap](docs/roadmap.md) coloca
> artefatos de release na F6.

## [Não publicado]

### Adicionado

- Documentação normativa completa do v1: [ADR-0001 a 0015](docs/adr/),
  [PRD-000 a 005](docs/prd/) aprovados e PRD-006 a 009 em rascunho
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

- Actions do GitHub pinadas por SHA de commit em vez de tag major flutuante,
  com a versão legível no comentário. Tag pode ser reapontada e uma release
  menor muda comportamento sem PR do Dependabot — mesma disciplina que o
  [ADR-0011](docs/adr/0011-toolchain-rust.md) aplica à toolchain Rust

### Corrigido

- Etimologia de "Porecatu" no README: do tupi, **"salto bonito"**
