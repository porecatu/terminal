# PRD-008 — Paleta de comandos

**Status:** Rascunho — fase v2
**Data:** 2026-08-26
**Relacionados:** [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md), [PRD-002](prd-002-grupos-de-abas.md)

> Rascunho. Existe para dar endereço a um elemento desenhado no canvas mas **fora do escopo do v1** ([PRD-000](prd-000-visao-de-produto.md)). Não implementar antes de o documento ser promovido a Aprovado.

## Problema

A paleta ataca dois problemas que crescem juntos e que o próprio produto cria.

**Encontrar uma aba.** O v1 resolve navegação com sequencial e índice 1..9 ([PRD-001](prd-001-abas.md)). Com trinta abas em seis grupos, três deles colapsados, isso não basta: a aba que se procura pode nem estar visível na barra.

**Encontrar uma ação.** Cada recurso novo — agrupar, colapsar, recolorir, desagrupar, dividir, trocar tema — é um atalho a mais para memorizar. E [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md) restringe o espaço de teclas de propósito, porque `Ctrl+<letra>` pertence ao terminal. Uma busca por nome torna esses atalhos opcionais em vez de obrigatórios.

## O que o design mostra

Ver [especificação visual](../design/especificacao-visual.md), seção 2.11.

- Modal centralizado de 600px, `max-height: 440`, sobre overlay `rgba(6,7,9,.55)`
- Cabeçalho com prompt `>` na cor de acento, campo de busca 14.5px, chip `Esc`
- Placeholder: "Ir para aba, grupo ou comando…"
- Resultados unificados, cada um com chip de tipo, rótulo, dica e tecla
- Três tipos, com cores próprias: `aba` `#5ed3bc`, `grupo` `#a68cf0`, `ação` `#e0b060`
- Primeiro resultado de uma busca ativa é realçado com fundo `#242a33`
- Estado vazio: "Nada encontrado"
- Aberta por `Ctrl+Shift+P` e pelo botão "Buscar" na barra de abas

No mockup, a dica de cada resultado dá contexto útil — uma aba mostra a que grupo pertence, um grupo mostra quantas abas tem.

## Requisitos esboçados

- **RF-8.1** — Abrir por atalho e pelo botão da barra; fechar com `Esc` ou clique fora.
- **RF-8.2** — Busca unificada sobre abas, grupos e ações, sem separar por modo.
- **RF-8.3** — Abas encontradas por título, diretório e nome do grupo; **incluindo abas de grupos colapsados** — ativar uma delas expande o grupo ([PRD-002](prd-002-grupos-de-abas.md) RF-2.17).
- **RF-8.4** — Correspondência aproximada (*fuzzy*), com destaque dos trechos que casaram.
- **RF-8.5** — Navegação por setas, execução por `Enter`, execução direta do primeiro resultado.
- **RF-8.6** — Ações exibem seu atalho, quando houver — a paleta ensina os atalhos em vez de substituí-los.
- **RF-8.7** — Ordenação por uso recente quando a busca está vazia.
- **RF-8.8** — O conjunto de ações é o mesmo do [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md); uma ação nova aparece na paleta sem trabalho extra.

## Impacto sobre decisões existentes

`Ctrl+Shift+P` pertencia a `theme.cycle` no ADR-0008. Já reconciliado em [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md): a paleta fica com `Ctrl+Shift+P` e `theme.cycle` migra para `Ctrl+Shift+Y`.

## Questões em aberto

- A paleta executa comandos de shell, ou só ações do app? (Provável só ações — comando de shell é o que o terminal já faz.)
- Entra a busca no scrollback, prevista para F6 do [roadmap](../roadmap.md), ou são superfícies separadas?
- Histórico de comandos da paleta persiste na sessão?
- Ações destrutivas — fechar grupo — pedem confirmação a partir da paleta?

## Fora de escopo

Extensibilidade por plugin; execução de comandos de shell; busca em conteúdo de terminal (é a busca no scrollback, recurso distinto).
