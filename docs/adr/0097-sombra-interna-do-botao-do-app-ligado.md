# ADR-0097 — Sombra interna do botão do app ligado: mais opaca, nas bordas de cima e esquerda

**Status:** Aceito
**Data:** 2026-11-10
**Relacionados:** [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0079](0079-botao-de-menu-do-app.md), [ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md), [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md)
**Supersedes:** ADR-0096 §6 item 2 (**parcial**: a sombra interna ganha alfas próprios, o dobro dos de `SHADOW_LAYERS`, e passa a ter também a borda esquerda; o fundo pressionado fixo, o ícone por cima e o comportamento de hover e clique não mudam)

## Contexto

O [ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md) §6 desenhou o botão do app ligado com uma sombra interna feita das **mesmas** camadas da sombra externa (`chrome::SHADOW_LAYERS`, alfas `.16` / `.10` / `.06`), só na faixa de cima, com "zero valor novo". A sombra interna nas quatro bordas foi recusada ali, e a lateral junto com ela. O risco "sombra imperceptível" ficou para o aval em tela do dono.

No aval visual, o dono pediu duas mudanças:

- a sombra **mais opaca**;
- a sombra **contornando o lado esquerdo e o de cima** do botão, e **acompanhando a curvatura do canto superior esquerdo**. Só com a borda de cima, ela parecia vir diretamente de cima.

O ADR-0096 dizia que um ajuste "pede ADR novo, porque mexe na sombra externa também". Este ADR existe por isso, mas a decisão abaixo não mexe na sombra externa.

## Decisão

**A sombra interna tem uma tabela própria, `chrome::INSET_SHADOW_LAYERS: [(profundidade, alfa); 3] = [(1, .32), (2, .20), (3, .12)]`. Cada camada é pintada ao longo da borda de cima, do arco do canto superior esquerdo e da borda esquerda.**

- **Profundidades**: 1, 2 e 3 px, os `offset_y` de `SHADOW_LAYERS`. A espessura do degradê é a mesma da primeira versão, e a luz das duas sombras do chrome continua uma só.
- **Alfas**: o **dobro** dos de `SHADOW_LAYERS`. Na fileira mais externa, as três camadas compõem cerca de `.52` de preto, contra cerca de `.29` antes. O dobro é o menor passo que lê como "mais opaca" sem virar uma borda preta, e o teste `the_inset_shadow_keeps_the_depths_and_doubles_the_alphas_of_the_outer_one` amarra as duas tabelas, de modo que mudar a sombra externa obriga a olhar a interna.
- **Bordas e arco**: cada camada é uma faixa preta de `depth` ao longo da borda de cima, do **arco do canto superior esquerdo** e da borda esquerda. `chrome::push_inset_shadow` a monta com três peças recortadas, **sem sobreposição**:
  1. o quadrado do canto (`radius` × `radius`, no topo à esquerda): o `RoundedQuad` do botão com preenchimento transparente e **borda** preta de `depth`, que o `quad.wgsl` pinta como o anel por dentro do contorno. Assim a faixa acompanha a curvatura;
  2. a faixa de cima, à direita do quadrado (`depth` de altura): o `RoundedQuad` do botão preenchido de preto, que também segue o topo do canto superior direito;
  3. a faixa esquerda, abaixo do quadrado (`depth` de largura), do mesmo jeito.

  Na fronteira do quadrado, o anel é exatamente uma faixa reta de `depth`, então as peças se encontram sem costura nem pixel dobrado. As camadas se sobrepõem da borda para dentro, então os pixels mais externos recebem as três: um degradê de 3 px para cima e para a esquerda, como num botão iluminado de cima e da esquerda.

  A primeira versão deste ajuste usava só as duas faixas retas, e o arco ficava sem sombra. As faixas recortam o quad arredondado: perto do canto, a forma começa depois da curva, e a região logo dentro do arco não caía em nenhuma faixa. O dono apontou isso em tela.

  O anel usa a borda de retângulo arredondado do `quad.wgsl`. O [ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md) §2 e o CLAUDE.md a tratavam como "defeito aberto, nunca pintada", mas ela já é pintada desde o commit `8e93906`, que já corrigia o shader. O CLAUDE.md foi corrigido junto com este ADR.
- **`SHADOW_LAYERS` não muda.** A sombra externa das abas, dos grupos, do quadro e dos widgets continua igual.
- **Onde os valores ficam**: na constante, com comentário, e na especificação visual (§1.2, linha "Sombra interna"; §1.7; §2.2 item 0; §4.4). Não há chave TOML, pela mesma regra de `SHADOW_LAYERS`: é técnica de desenho, não valor de tema.

## Alternativas consideradas

### Mudar `SHADOW_LAYERS`

Recusada: escureceria também a sombra externa de todo o chrome, e o dono pediu só a do botão.

### Multiplicar os alfas de `SHADOW_LAYERS` na chamada

Sem constante nova. Recusada: o fator vira um número solto no código, e a especificação ficaria sem um valor para citar.

### As quatro bordas

Recusada de novo, pelo motivo do ADR-0096: uma sombra simétrica lê como borda escura, não como botão apertado. O pedido do dono foi o de cima e o da esquerda.

## Consequências

- O botão ligado lê como apertado e iluminado de cima e da esquerda, com contraste maior contra o fundo pressionado e a sombra contornando o arco do canto.
- São três valores novos (os alfas), documentados na especificação e amarrados por teste à sombra externa.
- São 27 primitivas em vez de 9 (três camadas de três peças recortadas), num único botão. O custo é irrelevante.
- A moldura dos gráficos do gerenciador continua sem raio. O motivo que o ADR-0096 deu para isso (o defeito da borda) não vale mais, e voltar ao raio 6 é decisão visual separada, do dono.
- A verificação ao vivo no Windows continua dívida. O dono deu o aval ao desenho a partir do binário anterior, e esta versão ainda precisa ser vista em tela.
