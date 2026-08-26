# Design

Alvo visual do Porecatu. Desenhado em Claude Design e importado para cá para que a implementação tenha um modelo concreto, não uma descrição textual de barra de abas.

## Origem

| | |
|---|---|
| Projeto | **Emulador de terminais multiplataforma** |
| `projectId` | `b0bc7589-f967-40cb-98ab-caef4070a95a` |
| Arquivo | `Terminal Multiplataforma.dc.html` |
| Canvas | https://claude.ai/design/p/b0bc7589-f967-40cb-98ab-caef4070a95a?file=Terminal+Multiplataforma.dc.html |

O canvas é editado em claude.ai; esta pasta é uma cópia de leitura.

## Autoridade

O design é **normativo para a aparência** do chrome — cores, dimensões, raios, espaçamentos, estados.

Os PRDs continuam **normativos para o comportamento** — o que acontece ao clicar, o que persiste, o que é configurável.

Onde os dois divergirem, [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md) registra o conflito e quem venceu. A seção 4.4 da especificação visual lista todas as divergências já resolvidas.

**Regra prática:** nenhum valor de aparência é inventado. Sai da tabela de tokens da especificação visual ou do [porecatu.example.toml](../config/porecatu.example.toml) — que já traz esses mesmos valores como default.

## Aviso de fase

O mockup mostra o produto completo, **não o v1**. Painéis divididos, perfis de aba, paleta de comandos, painel de configurações GUI, barra de status e barra de título customizada são todos `[v2]`.

Antes de implementar qualquer coisa daqui, consulte a **tabela de fases** (seção 3 da especificação visual). Ela classifica todo elemento do desenho como `[v1]` ou `[v2]` e aponta o PRD que o governa.

## Arquivos

| Arquivo | O que é |
|---|---|
| [`especificacao-visual.md`](especificacao-visual.md) | **Comece por aqui.** Tokens, anatomia por componente, tabela de fases e rastreabilidade design ↔ requisito |
| [`mockup-estatico.html`](mockup-estatico.html) | Render estático do estado padrão. Abre com duplo clique, sem dependências. É o que se deixa aberto ao lado do editor |
| [`Terminal Multiplataforma.dc.html`](Terminal%20Multiplataforma.dc.html) | Cópia verbatim do canvas. Fonte de verdade e registro do que foi aprovado |

O canvas original acompanha um `support.js` — o runtime do Claude Design. Não foi copiado: não é código do projeto, e a cópia local não precisa executar. Para interagir com o mockup, abra o link do canvas; para uma visão estática, abra o `mockup-estatico.html`.

## Atualizar quando o canvas mudar

1. Reler o arquivo com a ferramenta `DesignSync` (`get_file`), usando o `projectId` acima.
2. Regravar `Terminal Multiplataforma.dc.html` com o conteúdo novo.
3. Revisar a tabela de tokens e a anatomia da especificação visual contra o que mudou.
4. Propagar as mudanças de valor para o `porecatu.example.toml`.
5. Classificar qualquer elemento novo na tabela de fases — nada fica sem etiqueta.
6. Se a mudança conflitar com um PRD ou ADR, escrever um ADR novo. Não editar decisão aceita.
