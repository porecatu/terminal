# PRD-006 — Painéis divididos

**Status:** Rascunho — fase v2
**Data:** 2026-08-26
**Relacionados:** [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md), [PRD-001](prd-001-abas.md)

> Rascunho. Existe para dar endereço a um elemento desenhado no canvas mas **fora do escopo do v1** ([PRD-000](prd-000-visao-de-produto.md)). Não implementar antes de o documento ser promovido a Aprovado.

## Problema

Alguns pares de terminais só fazem sentido lado a lado: um servidor rodando e o log dele; um build e os testes; um `ssh` e o `journalctl` da mesma máquina. Alternar abas entre os dois destrói o motivo de olhar para os dois — que é ver a correlação no momento em que ela acontece.

Abas resolvem "muitos contextos". Painéis resolvem "um contexto, duas superfícies".

## O que o design mostra

Ver [especificação visual](../design/especificacao-visual.md), seção 2.7.

- Aba ativa com dois painéis lado a lado, separados por divisor de 1px sobre `#23272f`
- Cabeçalho por painel: ponto de foco na cor do grupo, título mono truncado, botões de dividir (`◫`) e fechar (`✕`)
- Painel focado recebe `border-top: 2px` na cor do grupo; sem foco, transparente
- O anel de foco só aparece quando há mais de um painel
- Barra de status exibe a contagem ("2 painéis")
- Atalho ilustrado no mockup: `Alt+Shift+D` — sujeito ao [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md) quando promovido

## Requisitos esboçados

- **RF-6.1** — Dividir a aba ativa em dois painéis, horizontal ou verticalmente.
- **RF-6.2** — Cada painel tem seu próprio PTY, grid, scrollback e diretório.
- **RF-6.3** — Foco entre painéis por clique e por teclado; o painel focado recebe todo o input do terminal.
- **RF-6.4** — Redimensionar painéis arrastando o divisor; proporções persistem na sessão ([PRD-003](prd-003-persistencia-de-sessao.md)).
- **RF-6.5** — Fechar um painel; fechar o último painel fecha a aba.
- **RF-6.6** — Limite de aninhamento e de contagem, a definir.
- **RF-6.7** — O resize da janela redistribui os painéis mantendo as proporções e redimensiona cada PTY.

## Impacto arquitetural

Não é um recurso de superfície. `Tab` deixa de conter um terminal e passa a conter uma **árvore de painéis**. O [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md) já previu isso e mitigou: `Tab` é uma struct própria, não um alias de terminal, então a mudança fica contida.

Consequências a avaliar quando o rascunho for promovido:

- O modelo de threading ([ADR-0007](../adr/0007-modelo-de-threading.md)) passa a ter N threads de leitura por aba, não uma.
- A persistência de sessão precisa gravar a árvore e as proporções.
- O `Wakeup` passa a endereçar painel, não aba.
- Um ADR próprio é obrigatório antes da implementação.

## Questões em aberto

- Divisão livre em árvore, ou layouts predefinidos?
- Painel pode ser movido entre abas?
- Zoom temporário de um painel para tela cheia?
- Como o `cwd` de uma aba com vários painéis é definido para a restauração de sessão?

## Fora de escopo

Layouts salvos e nomeados; sincronizar input entre painéis; painéis flutuantes.
