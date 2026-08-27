# ADR-0019 — Tooltip, o quarto widget de chrome

**Status:** Aceito
**Data:** 2026-08-27
**Relacionados:** ADR-0001, ADR-0009, ADR-0014, ADR-0018, PRD-001, PRD-002, PRD-004, PRD-009, PRD-010

## Contexto

O [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) fez o levantamento certo e chegou a um número errado. Ele decidiu **três** widgets próprios — aviso, diálogo e menu de contexto — abrindo com a citação do [ADR-0001](0001-stack-de-gui.md) que lista o custo de não usar toolkit: *"todo o chrome é código nosso: hit-testing, foco, ordem de tab, drag & drop, menu de contexto, **tooltip**. Nada vem de graça."*

O tooltip está na lista do ADR-0001 e ficou fora da decisão do ADR-0014. E ele é requisito, em três fases:

| Requisito | Pede | Fase |
|---|---|---|
| PRD-001 RF-1.10 | *"O título completo aparece em tooltip no hover"* | **F2** |
| PRD-002 RF-2.12 | *"Nome de grupo longo é truncado; o nome completo aparece em tooltip"* | F3 |
| PRD-009 RF-9.3 | *"Diretório abreviado quando longo, com `~` para o home; caminho completo em tooltip"* | `[v2]` |

Hoje não existe nada: nenhum token, nenhuma anatomia na especificação visual, nenhuma chave no arquivo de exemplo, nenhum atraso de hover, nenhuma fase atribuída. A seção 4.2 da [especificação visual](../design/especificacao-visual.md), que existe justamente para listar os requisitos do v1 sem representação no design, **também não o menciona** — é a mesma lacuna entre duas listas que o ADR-0014 se propôs a fechar, com um widget sobrando.

O RF-1.10 é da F2, e ele é a metade que dá sentido ao truncamento: truncar o título sem oferecer como ler o resto é esconder informação, não economizar espaço.

## Decisão

**Tooltip é o quarto widget próprio, com os tokens que já existem, e aparece só quando há texto escondido.**

### Quando aparece

Só quando o texto do alvo foi **efetivamente truncado**. O `TextMeasurer` do [ADR-0018](0018-composicao-de-frame.md) já devolve se houve corte ao ajustar o rótulo à largura disponível; esse booleano é a condição. Aba cujo título cabe inteiro não tem tooltip — nada a revelar, e um popover que repete o que já está na tela é ruído.

Atraso de 600 ms de hover parado sobre o alvo. É comportamento, não token de design: a seção 1.10 não tem entrada de atraso, e o valor entra como chave de configuração com origem neste ADR.

### Anatomia

Nenhuma cor nem dimensão nova. Tudo sai da seção 1 da especificação visual:

- Fundo `#1a1e25`, borda `1px #2e343e`, sombra de popover `0 18px 44px rgba(0,0,0,.55)`, animação `pop .13s` — a mesma família do menu e do aviso.
- Raio **6px**, não 8. O 8 é o raio de popover, dimensionado para menu e janela; num retângulo de uma linha de texto ele pesa. O 6 é a classe de raio dos elementos de 30 px de altura da barra, que é a escala do tooltip.
- Texto 11px `#d7dce3`, `padding: 7px 8px` — o espaçamento do item de menu.
- Largura máxima 320, a mesma do aviso. Texto além disso trunca com reticências: um tooltip que ocupa meia tela não resolve o problema que o tooltip existe para resolver.
- Uma linha só. Sem quebra, sem título, sem ícone, sem chip de tecla.

### Posicionamento

Ancorado no **alvo**, não no cursor — é o que diferencia tooltip de menu de contexto. Abaixo do alvo, alinhado à sua borda esquerda, a 6 px de distância. Vira nos dois eixos para caber no monitor da janela, como o menu do ADR-0014: acima quando não cabe abaixo, alinhado à direita quando estoura a borda direita.

### Dispensa

Some no que quer que interrompa o hover: sair do alvo, clicar, digitar qualquer tecla, começar um arraste, a janela perder foco, o alvo deixar de existir. Não tem botão de fechar e não persiste.

### O que ele não é

- **Não recebe foco de teclado** e não entra na ordem de tab. Não há navegação por setas dentro dele.
- **Não participa do hit-testing.** O ponteiro atravessa: o tooltip nunca rouba um clique do que está embaixo, e nunca dispara o próprio hover.
- **Não é nativo.** Vale a mesma proibição do RF-10.17 e do ADR-0014: diálogo, menu e notificação do sistema estão fora, e o tooltip do SO — que existe no Win32 e no Cocoa — está fora pelos mesmos três motivos, com destaque para o segundo, o de não ser alcançável pela config do usuário.
- **Não carrega ação.** Texto e nada mais. Conteúdo com ação é aviso (ADR-0014) ou menu.

### Camada e acessibilidade

Desenha na camada **popover** do [ADR-0018](0018-composicao-de-frame.md), junto com o menu de contexto: cobre chrome e aviso, é coberto por modal. Diálogo aberto suprime tooltip.

A dívida de `accesskit` cresce de três papéis para quatro, e é da F6, como os outros. Registrado aqui para que a F6 não descubra o crescimento na hora.

## Alternativas consideradas

### Tooltip nativo da plataforma

Vem de graça, com atraso e acessibilidade da plataforma inclusos, e é o argumento mais forte a favor. Descartada pelos motivos que o ADR-0014 já enumerou para diálogo e menu, e o segundo decide: não é configurável pelo usuário. Um tooltip com a fonte e a cor do sistema pendurado numa barra de abas inteiramente customizada é exatamente a costura visível que o princípio 2 do [PRD-004](../prd/prd-004-aparencia-do-chrome.md) proíbe. Somem-se aparência e atraso diferentes nas três plataformas.

### Não ter tooltip: remover a segunda frase do RF-1.10

Reduz escopo da F2 num widget. Descartada porque o truncamento e o tooltip são uma coisa só. O `max-width: 180px` do rótulo é agressivo — dois títulos de aba de projeto real colidem nele — e sem tooltip o usuário fica com abas visualmente idênticas e nenhum jeito de distinguir, que é o problema que abas nomeadas existem para resolver. O RF-2.12 e o RF-9.3 pediriam o mesmo widget depois, com o mesmo trabalho e uma fase de atraso.

### Mostrar o título completo alargando a aba no hover

Sem widget novo: a aba cresce e o vizinho cede. Descartada porque reflui a barra a cada movimento de mouse — as abas dançam sob o cursor, o alvo de clique muda enquanto o usuário mira, e com overflow ativo o crescimento empurraria a aba ativa para fora da vista.

### Reaproveitar o menu de contexto como tooltip

Já terá posicionamento com flip, popover e camada. Descartada porque as semânticas divergem em tudo que importa: menu ancora no cursor, tooltip no alvo; menu captura foco e teclado, tooltip é atravessável; menu persiste até uma escolha, tooltip morre no primeiro movimento. Compartilhar o código de flip é reuso; compartilhar o widget seria um menu com comportamento condicional em toda função.

### Tooltip para tudo que tem hover, não só para texto truncado

Botão de fechar, botão de nova aba, indicadores — o padrão de aplicação de desktop. Descartada porque nenhum requisito pede, e o catálogo de ações estabeleceu a regra que vale aqui também: *"nenhuma ação existe sem origem"*. Rótulo em botão de ícone é discussão de acessibilidade e pertence à F6, com `accesskit`, não a um popover que aparece por hover e que leitor de tela não lê.

## Consequências

### Positivas

- RF-1.10 fica implementável na F2, e RF-2.12 e RF-9.3 já têm widget quando chegarem.
- Nenhuma cor nem dimensão nova: o tooltip sai inteiro dos tokens da seção 1, e a especificação visual continua sendo a fonte única do ADR-0009.
- Aparecer só quando há truncamento aproveita o `TextMeasurer` do ADR-0018 e evita a praga do tooltip que repete o texto visível.
- A lacuna entre as duas listas da seção 4.2 fecha: os quatro widgets de chrome do v1 passam a estar todos decididos.

### Negativas

- Um quarto widget na mão, com atraso, posicionamento com flip e dispensa por seis gatilhos diferentes.
- Mais um papel para o `accesskit` cobrir na F6 — e tooltip é o menos padronizado dos quatro entre leitores de tela.
- O atraso de 600 ms introduz o segundo temporizador de UI do projeto, depois do aviso: mais uma fonte de sujeira para o render damage-driven do [ADR-0007](0007-modelo-de-threading.md).
- Estado de hover passa a ter duração, não só posição: a barra precisa saber há quanto tempo o cursor está parado onde está.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Temporizador do tooltip forçar frames desnecessários | Média | Baixo | Mesma regra do cursor no ADR-0007 e do aviso no ADR-0014: o timer marca sujeira, não roda loop |
| Tooltip aparecer durante arraste de aba | Média | Baixo | Arraste é um dos gatilhos de dispensa, e o hover parado é pré-condição |
| Tooltip abrir fora da tela em monitor secundário | Média | Baixo | Flip nos dois eixos contra o monitor da janela, o mesmo cálculo do menu do ADR-0014 |
| Reuso do widget para rótulo de botão de ícone por conveniência | Média | Baixo | Decisão registrada: só texto truncado; rótulo de ícone é `accesskit` na F6 |
| Tooltip capturar clique por herdar o hit-testing do menu | Média | Médio | Atravessável por decisão; verificado em teste de hit-testing, que é função pura |
| Atraso de 600 ms parecer lento ou rápido demais | Média | Baixo | Chave de configuração com origem neste ADR |
