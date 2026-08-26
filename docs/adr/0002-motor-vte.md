# ADR-0002 — Motor VT: crate alacritty_terminal

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** ADR-0001, ADR-0004

## Contexto

Um emulador de terminal precisa interpretar o fluxo de bytes do PTY: sequências CSI, OSC, DCS, modos DEC privados, tabstops, regiões de scroll, character sets, largura de caractere Unicode, scrollback, seleção. É um corpo de conhecimento acumulado em décadas, e o custo real não está em implementar o caso comum — está na cauda longa de compatibilidade que faz `vim`, `htop`, `tmux` e `fzf` funcionarem sem artefatos.

O diferencial do Porecatu (PRD-000) **não é** conformidade VT. É gestão de abas, grupos e sessão. Cada semana gasta perseguindo bug de modo DEC é uma semana não gasta no que o produto tem de próprio.

Restrição vinda de [ADR-0001](0001-stack-de-gui.md): a renderização é nossa, então o motor precisa expor o grid de células de forma que possamos varrer por conta própria, sem impor um caminho de desenho.

## Decisão

Usar o crate **`alacritty_terminal`** como motor VT e estrutura de grid.

Ele fornece `Term` (grid + scrollback + seleção + cursor) e o parser `vte`, é battle-tested pelo uso no Alacritty, e expõe as células para iteração — encaixa exatamente na fronteira que precisamos.

Duas regras de contenção, não negociáveis:

1. **Versão pinada com igualdade exata** (`alacritty_terminal = "=0.x.y"`). O crate não segue SemVer estável entre releases; um `cargo update` descuidado quebra a build.
2. **Todo o uso fica dentro de `porecatu-term`.** Nenhum outro crate importa `alacritty_terminal`. `porecatu-term` expõe um tipo próprio de snapshot de grid (células visíveis, cursor, dimensões) para o resto do app. Se o motor precisar ser trocado, o dano fica contido em um crate.

## Alternativas consideradas

### Crate `vte` cru + grid próprio

`vte` é só o parser (a máquina de estados de escape sequences); o grid, scrollback, tabstops, regiões de scroll e modos DEC seriam nossos. Foi considerada seriamente porque dá controle total e remove a dependência instável.

Descartada pelo custo de oportunidade: são meses de trabalho para chegar ao ponto em que o `alacritty_terminal` já está, e o resultado seria pior por anos — os bugs de conformidade são justamente aqueles que só aparecem com uso real e variado. Não é onde o projeto quer gastar seu orçamento de complexidade.

### Crate `wezterm-term`

Motor do WezTerm, mais rico em features: sixel, imagens, hyperlinks OSC 8, tratamento cuidadoso de largura Unicode. Tecnicamente o motor mais completo dos três.

Descartada porque é pensado para uso interno do WezTerm, não como biblioteca de terceiros: a documentação como lib é escassa, arrasta `termwiz` e um conjunto maior de dependências transitivas, e a superfície de API é grande demais para a fronteira estreita que queremos manter. As features extras (sixel) não são requisito do v1.

### Escrever tudo do zero, sem parser de terceiros

Nem considerada além da menção. Reimplementar a máquina de estados de escape sequences é trabalho puro de replicação sem nenhum retorno de produto.

## Consequências

### Positivas

- Conformidade VT razoável desde o primeiro dia utilizável (F1 do [roadmap](../roadmap.md)).
- Scrollback, seleção e busca já modelados pelo crate.
- O orçamento de esforço vai para abas, grupos e sessão — o diferencial real.
- A fronteira em `porecatu-term` deixa a troca de motor viável, se um dia for necessária.

### Negativas

- Dependência de um crate cuja API muda sem aviso de SemVer. Atualizar exige ler o changelog e ajustar código, não só bumpar o número.
- Herdamos as decisões de modelagem do Alacritty (formato de célula, modelo de scrollback). Onde nossa visão divergir, ou adaptamos ou vivemos com a diferença.
- Features fora do escopo do Alacritty (sixel, imagens inline) ficam indisponíveis enquanto este motor for usado.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Breaking change em release nova | Alta | Baixo | Versão pinada; atualização é tarefa deliberada com changelog na mão |
| Crate ser descontinuado ou congelado | Baixa | Alto | Isolamento em `porecatu-term` permite trocar para `wezterm-term` ou fork sem tocar em `ui`/`render` |
| API interna do `Term` não expor algo que precisamos | Média | Médio | Verificar as necessidades de F1 (grid, cursor, scrollback, seleção) já na fase F1, antes de construir F2 em cima |
| Custo de conversão grid -> snapshot virar gargalo | Baixa | Médio | Snapshot só das células visíveis, reuso de buffer entre frames; medir antes de otimizar |
