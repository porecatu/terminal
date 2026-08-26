# Changelog

Formato baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).
Versionamento seguirá [SemVer](https://semver.org/lang/pt-BR/) a partir do
primeiro release.

> **Nenhuma versão foi publicada ainda.** O Porecatu está em fase de
> documentação: não há código nem binário. O [roadmap](docs/roadmap.md) coloca
> artefatos de release na F6.

## [Não publicado]

### Adicionado

- Documentação normativa completa do v1: [ADR-0001 a 0010](docs/adr/),
  [PRD-000 a 005](docs/prd/) aprovados e PRD-006 a 009 em rascunho
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

### Corrigido

- Etimologia de "Porecatu" no README: do tupi, **"salto bonito"**
