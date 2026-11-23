# ADR-0114 — Nova aba solta pelo celular: `new-tab` com `group: null`

**Status:** Aceito em 2026-11-23 — implementado em 2026-11-23 (verificado só por teste automatizado; ninguém viu rodando, dívida no [roadmap](../roadmap.md))
**Data:** 2026-11-23
**Relacionados:** [PRD-025](../prd/prd-025-acesso-remoto.md), [ADR-0107](0107-acoes-remotas-sobre-o-workspace.md), [ADR-0111](0111-o-celular-nao-muda-a-aba-ativa-do-desktop.md), [ADR-0013 do `porecatu-protocol`](https://github.com/porecatu/protocol/blob/main/docs/adr/0013-nova-aba-solta.md), [ADR-0020](0020-grupos-explicitos.md)

## Contexto

O `new-tab` remoto sem `group` cria a aba onde o `tab.new` criaria: no grupo da aba ativa (`NewTabTarget::ActiveGroup`). O celular tem a linha "Nova aba" no fim de cada janela, que o usuário lê como "aba solta". Com a aba ativa no último grupo, a aba nascia nesse grupo. O protocolo não tinha como pedir a aba solta que o "+" do fim da trilha cria (`NewTabTarget::Ungrouped`).

## Decisão

**O host aceita `new-tab` com `group: null` (especificação 0.7) e cria a aba pelo caminho do "+" do fim da trilha.**

1. `group` ausente continua `NewTabTarget::ActiveGroup`; um id, `NewTabTarget::Group`; `null`, `NewTabTarget::Ungrouped` (`Workspace::append_ungrouped_tab_with`): sem grupo, no fim da trilha, sem olhar a aba ativa.
2. `null` não valida grupo, então não gera `unknown-group`. A ordem das checagens continua: janela, grupo (só para id), perfil, depois `busy`.
3. A aba continua nascendo **sem ativar** ([ADR-0111](0111-o-celular-nao-muda-a-aba-ativa-do-desktop.md)).
4. O `rev` do `porecatu-protocol` sobe para o commit da 0.7, junto com o Relay e o app Remote. O pino exato `bytes` do `porecatu-remote` sobe para `=1.12.2`, o que o protocolo passou a exigir.

Complementa o [ADR-0107](0107-acoes-remotas-sobre-o-workspace.md) (que define o `new-tab` sem `group`); não substitui nada.

## Consequências

- Um app 0.6 não manda `null`, e um host 0.6 recusaria o `null` do app 0.7: os três sobem juntos, como nas versões anteriores (o fio `[0, 1]` não as distingue).
- Cobertura: `plan_new_tab` (`remote/actions.rs`) com os três estados e um teste do posicionamento com a aba ativa no último grupo.
