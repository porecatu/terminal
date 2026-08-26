# PRD-009 — Barra de status

**Status:** Rascunho — fase v2
**Data:** 2026-08-26
**Relacionados:** [ADR-0005](../adr/0005-persistencia-de-sessao.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md), [PRD-004](prd-004-aparencia-do-chrome.md)

> Rascunho. Existe para dar endereço a um elemento desenhado no canvas mas **fora do escopo do v1** ([PRD-000](prd-000-visao-de-produto.md)). Não implementar antes de o documento ser promovido a Aprovado.

## Problema

Informação que o app **já tem** e não mostra em lugar nenhum: qual shell está rodando na aba ativa, em que diretório ela está, a que grupo pertence, qual a codificação.

O diretório é o caso mais forte. O app precisa dele para a restauração de sessão ([ADR-0005](../adr/0005-persistencia-de-sessao.md)) e o obtém via OSC 7. Hoje esse dado fica invisível. Exibi-lo tem um efeito colateral valioso: **o usuário passa a ver se o OSC 7 está funcionando**. Sem barra de status, a ausência de integração de shell só se manifesta muito depois, quando a sessão restaura no diretório errado — o pior momento possível para descobrir.

Uma barra de status transforma uma limitação silenciosa em uma limitação visível.

## O que o design mostra

Ver [especificação visual](../design/especificacao-visual.md), seção 2.8.

Altura 26, fundo `#1b1f26`, borda superior `#23272f`, mono 10.5px `#6b737e`, `gap: 16`.

| Zona | Conteúdo no mockup |
|---|---|
| Esquerda | nome do shell em `#5ed3bc`, diretório atual, grupo da aba |
| Direita | codificação (`UTF-8`), contagem de painéis, sistema e versão |

O nome do shell é o único item colorido — é o que distingue a aba de relance.

## Requisitos esboçados

- **RF-9.1** — Barra opcional, ligável e desligável na config; **desligada por padrão** é uma decisão em aberto, dado que ela custa 26px permanentes de altura.
- **RF-9.2** — Exibe da aba ativa: shell ou perfil, diretório atual, grupo.
- **RF-9.3** — Diretório abreviado quando longo, com `~` para o home; caminho completo em tooltip.
- **RF-9.4** — **Indicação visível de que o diretório não vem de OSC 7** — o dado é o de spawn, não o atual. É o requisito que dá razão de existir a esta barra ([ADR-0005](../adr/0005-persistencia-de-sessao.md)).
- **RF-9.5** — Conteúdo de cada zona configurável a partir de um conjunto fechado de campos.
- **RF-9.6** — Cores, altura e fonte configuráveis, como o resto do chrome ([PRD-004](prd-004-aparencia-do-chrome.md)).
- **RF-9.7** — Clicar no campo de diretório copia o caminho.
- **RF-9.8** — A barra atualiza sob o mesmo regime damage-driven do resto da UI ([ADR-0007](../adr/0007-modelo-de-threading.md)); não introduz timer nem redraw periódico.

RF-9.8 não é detalhe: uma barra de status com relógio ou uso de CPU quebraria a propriedade de "terminal ocioso custa zero frames", que é um princípio do produto. Qualquer campo que mude sozinho precisa ser avaliado contra isso.

## Questões em aberto

- Campos de sistema (versão, plataforma) valem o espaço, ou são ruído?
- Contagem de painéis só faz sentido com [PRD-006](prd-006-paineis-divididos.md) implementado.
- Barra por janela ou por aba?
- Exibe indicadores de atividade e campainha ([PRD-001](prd-001-abas.md) RF-1.20, RF-1.21), ou eles ficam só na aba?

## Fora de escopo

Campos definidos por script; integração com `git` (branch, estado do repositório); relógio; medidores de CPU e memória — todos violam RF-9.8 ou dependem de lógica programável, descartada no v1 pelo [ADR-0003](../adr/0003-formato-de-configuracao.md).
