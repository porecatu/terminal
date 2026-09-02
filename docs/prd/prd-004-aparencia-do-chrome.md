# PRD-004 — Aparência do chrome (abas e grupos)

**Status:** Aprovado
**Data:** 2026-08-26
**Requisito de origem:** 4 — as cores, desenho e dimensões das abas e grupos de abas devem ser customizáveis em um arquivo de configuração
**Relacionados:** [ADR-0003](../adr/0003-formato-de-configuracao.md), [ADR-0001](../adr/0001-stack-de-gui.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md), [PRD-001](prd-001-abas.md), [PRD-002](prd-002-grupos-de-abas.md)

> **Valores default reproduzem o binário.** Este PRD define *qual* superfície é configurável. Os *valores* padrão de cada chave estão registrados na [especificação visual](../design/especificacao-visual.md), seção 1, e no [`porecatu.example.toml`](../config/porecatu.example.toml). Nenhuma cor ou dimensão é inventada na implementação. Desde o [ADR-0028](../adr/0028-o-binario-como-referencia-visual.md) a referência visual é o **binário**, não o mockup: os defaults têm de reproduzir o que ele desenha, e **nenhuma mudança de aparência acontece sem aval do dono do produto**.

> **Emendas de 2026-09-02** (ADR-0028 §3, que manda emendar requisito em contradição com o binário em vez de deixá-lo pendente): RF-4.1, RF-4.3, RF-4.5, RF-4.6, RF-4.14 e RF-4.19 foram ajustados ao que a interface faz, e RF-4.25 a RF-4.27 entraram para cobrir o que existe em código sem requisito. As mudanças estão marcadas em cada requisito, com o registro correspondente na seção 4.4 da especificação visual.

## Problema

O requisito é específico e vai além de "aplicar um tema": **cores, desenho e dimensões**. Isso significa que o usuário controla a geometria da barra de abas, não só sua paleta.

A razão de fundo é que a barra de abas é permanente. Ela ocupa espaço vertical em todas as sessões, o dia inteiro. Usuários têm preferências fortes e legítimas sobre densidade (barra compacta versus confortável), sobre quanto contraste separa a aba ativa das inativas, e sobre o quão marcado é o indicador de grupo. Um emulador que impõe uma resposta única a essas perguntas irrita metade dos usuários o tempo todo.

Foi essa exigência que decidiu a stack de renderização: nenhum toolkit pronto expõe essa superfície de controle ([ADR-0001](../adr/0001-stack-de-gui.md)).

## Usuário-alvo

Todo usuário, com defaults que funcionam sem configuração alguma. A profundidade existe para quem quiser.

## Princípios

1. **Defaults completos.** Sem arquivo de config, o app abre com uma barra sensata. Nenhuma chave é obrigatória.
2. **Nenhum valor de aparência hardcoded.** Toda cor, dimensão e raio é uma chave de config com default declarado.
3. **Recarga a quente.** Editar o arquivo redesenha a barra imediatamente, sem reiniciar.
4. **Config inválida não quebra a tela.** Mantém a config anterior e mostra o erro ([ADR-0003](../adr/0003-formato-de-configuracao.md)).
5. **O default não depende de cor sozinha.** A aba ativa é distinguível também por forma e peso, para quem não distingue matizes.

## Requisitos funcionais

### Janela e barra

**RF-4.1** — Configuráveis: espaçamento interno da janela (horizontal e vertical), opacidade da janela, presença das decorações do sistema, e posição da barra de abas (topo ou base).

*Emendado.* O default de `decorations` é **`false` fora do macOS** e `true` no macOS ([ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md)): sem decoração nativa, a própria barra de abas assume drag region, botões de janela e resize por borda. No macOS a decoração nativa fica, com o semáforo, e a trilha reserva espaço à esquerda dele.

**RF-4.2** — Configurável: se a barra aparece quando existe apenas uma aba. *(Quem usa uma aba por vez não deveria pagar espaço vertical por isso.)*

### Dimensões das abas

**RF-4.3** — Configuráveis: altura da barra, largura mínima e máxima de aba, espaçamento interno horizontal da aba, espaço entre abas.

*Emendado.* A **altura da barra é derivada**, não livre: `tab_height + wrapper_padding * 2 + trilha_padding * 2` (`chrome::bar_height`). A chave `height` fica como o valor esperado dessa conta — serve de verificação, não de entrada; duas fórmulas de altura em dois lugares já divergiram uma vez. Entra na lista `trilha_padding`, o respiro da trilha contra as bordas da barra.

**RF-4.4** — Configurável: raio dos cantos da aba. Raio zero produz abas retangulares.

**RF-4.5** — A largura mínima é o piso do encolhimento antes de a barra passar a rolar ([PRD-001](prd-001-abas.md), RF-1.18).

*Emendado.* **A largura da aba é fixa**, e com os defaults o piso e o teto descrevem a mesma largura: o teto de 180 px do rótulo virou também o piso, e **nada encolhe** — em overflow, a trilha rola inteira (especificação visual §2.18). Duas razões, as duas verificadas em tela: largura por conteúdo refluía a trilha a cada título novo (trocar de aba, renomear, ou um programa que muda o título mexia na posição de todas as outras abas), e o encolhimento por busca binária custava até 24 relayouts completos por frame, cada um remedindo o texto de toda aba sem cache — a barra parecia travada justamente no caso que o encolhimento existia para tratar. As chaves continuam; aceitar `min_width < max_width` de novo exige cache de medição antes.

### Cores das abas

**RF-4.6** — Configuráveis por estado — ativa, inativa, hover — as cores de fundo e de texto.

*Emendado.* O **hover não tem cores próprias**: é um multiplicador de brilho sobre o estado atual (`hover_brightness`, `label_hover_brightness`). Com seis cores de grupo, cor de hover por estado exigiria doze tokens para dizer o que um multiplicador diz. O realce em si **ainda não é desenhado** — o hover existe como hit-test e alimenta o tooltip —, e entra na F4 com aval do dono do produto (ADR-0028 §4). O fundo da aba ganhou um alfa (`background_alpha`, `0.85`) para deixar passar a cor da cápsula do grupo por baixo.

**RF-4.7** — Configuráveis: cor de fundo da barra (atrás das abas), cor e espessura da borda da aba ativa, e cor da borda de aba selecionada ([PRD-002](prd-002-grupos-de-abas.md), RF-2.2).

**RF-4.8** — Configuráveis: cores dos indicadores de atividade e de campainha ([PRD-001](prd-001-abas.md), RF-1.20 e RF-1.21), e se cada um aparece.

**RF-4.9** — Cores aceitam hexadecimal (`#rrggbb`, `#rrggbbaa`) e o valor `"transparent"`.

### Conteúdo da aba

**RF-4.10** — Configuráveis: família e tamanho da fonte do título da aba, independentes da fonte do terminal ([PRD-005](prd-005-aparencia-do-terminal.md)).

**RF-4.11** — Configuráveis, cada um ligável e desligável: botão de fechar, número de índice da aba, indicador de atividade, indicador de campainha, botão de nova aba na barra.

**RF-4.12** — Configurável: se o botão de fechar aparece sempre ou apenas no hover.

### Grupos

**RF-4.13** — Configuráveis para o rótulo do grupo: espaçamento interno horizontal, tamanho da fonte, raio dos cantos.

**RF-4.14** — Configurável: o **estilo do indicador de grupo**, como **lista combinável** — não escolha exclusiva. Quatro formas:

| Estilo | Desenho |
|---|---|
| `pill` | Pílula colorida em volta do rótulo, mais a **cápsula** do grupo atrás das abas |
| `underline` | Linha colorida na base de cada aba do grupo |
| `left-bar` | Barra vertical colorida antes da primeira aba do grupo |
| `outline` | Contorno colorido envolvendo rótulo e abas |

Default: **`["pill"]`**. *Emendado* — era `["pill", "underline"]`, como no design. O sublinhado existia para dizer a que grupo uma aba pertence quando a pílula sai da vista por rolagem; desde que a cápsula do grupo passou a ser pintada com a cor cheia (RF-4.19), a cor do grupo está atrás da aba inteira e o traço na base virou ruído. A lista continua **combinável**, e `underline` continua válido para quem quiser ligá-lo. `left-bar` e `outline` seguem sem anatomia desenhada (especificação visual §4.2). Ver [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md) e [ADR-0028](../adr/0028-o-binario-como-referencia-visual.md).

**RF-4.15** — Configurável: espessura do indicador de grupo.

**RF-4.16** — Configurável: espaço horizontal entre grupos, distinto do espaço entre abas do mesmo grupo. *(É o que faz a fronteira do grupo ser percebida sem depender só da cor.)*

**RF-4.17** — Configurável: se um grupo colapsado mostra a contagem de abas no rótulo.

**RF-4.18** — Configurável: a **paleta de grupos** — uma lista de cores nomeadas, atribuídas automaticamente a grupos novos ([PRD-002](prd-002-grupos-de-abas.md), RF-2.4) e oferecidas na troca manual de cor.

**RF-4.19** — Configurável: quanto a cor do grupo tinge o fundo do wrapper do grupo, de nenhum tingimento a tingimento pleno.

*Emendado* em dois pontos. O default é **`1.0` — cor cheia**, não o `0.07` do design: atrás do fundo das abas, 7% da cor não se via, e o indicador de grupo mais visível da barra não podia ser o menos legível dela. E a cápsula é pintada **também com o grupo colapsado**, em vez de ficar transparente: é ela que diz de que cor o grupo é, e sumir com ela tirava a única marca de cor justo quando o nome do grupo é tudo o que resta na barra. Os dois foram pedido direto do usuário. Sobre a cor cheia há ainda o efeito de vidro — alfa de `.85` na cápsula, `.92` na pílula e um rim translúcido de 1 px nas duas (`capsule_alpha`, `label_alpha`, `glass_border`), mais a sombra em camadas de `shadow`.

### Controles de janela, quadro do terminal e sombra

Três coisas que existem em código sem requisito de origem — o ADR-0027 e os ajustes pós-F3 vieram depois deste PRD. Entram aqui para fechar a auditoria bidirecional do critério de saída da F4 (nenhuma chave sem requisito, nenhum requisito sem chave).

**RF-4.25** — Configuráveis, quando `decorations = false`: largura dos botões de janela, cores de hover deles (inclusive o hover destrutivo do botão de fechar), espessura da zona de resize por borda, e o espaço reservado à esquerda da trilha para o semáforo nativo do macOS. Ver [ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md).

**RF-4.26** — Configuráveis para o **quadro do terminal** — o retângulo arredondado em volta da grade: margem contra a borda da janela, padding interno até a grade, raio dos cantos e presença de sombra. O quadro encosta na barra de abas sem folga; um vão ali desenha uma linha entre a trilha e o terminal.

**RF-4.27** — Configurável: presença da **sombra** da cápsula de grupo, da aba solta e do quadro do terminal. É liga-desliga, não seis números: `porecatu-render` não tem primitiva de sombra nem passo de blur, e o desenho é uma pilha fixa de três quads arredondados cujos valores estão na especificação visual §1.7.

### Badge de perfil `[v2]`

**RF-4.23** — Configurável: exibir ou não o **badge de perfil** na aba — o marcador curto que identifica o tipo de terminal (`SSH`, `WSL`, `PS`). Requer [PRD-007](prd-007-perfis-de-aba.md), em rascunho.

**RF-4.24** — O badge herda a cor **do grupo**, não a do perfil, com o fundo tingido dessa mesma cor. Mantém a barra legível pelo eixo principal de organização; a cor do perfil aparece apenas no menu de perfis e nas configurações.

### Recarga e erro

**RF-4.20** — Salvar o arquivo de config aplica as mudanças de aparência em menos de 500 ms, sem reiniciar e sem perder o conteúdo das abas.

**RF-4.21** — Config inválida mantém a aparência anterior e mostra o erro com linha e chave. A barra nunca fica em estado quebrado ou invisível.

**RF-4.22** — Chave desconhecida gera aviso, não erro. O app continua funcionando.

## Critérios de aceite

```gherkin
Cenário: app funciona sem arquivo de config
  Dado que nenhum arquivo de configuração existe
  Quando o usuário abre o app
  Então a barra de abas é desenhada com valores padrão
  E nenhum erro é exibido

Cenário: recarga a quente de dimensão
  Dado o app aberto com altura de aba 32
  Quando o usuário altera a altura para 44 e salva
  Então a barra é redesenhada com a nova altura em menos de 500 ms
  E o conteúdo das abas é preservado

Cenário: config inválida preserva a aparência
  Dado o app aberto com aparência customizada
  Quando o usuário salva uma cor inválida como "#gggggg"
  Então a aparência anterior é mantida
  E um erro citando a linha e a chave é exibido

Cenário: indicador de grupo no default
  Dado a configuração padrão
  Quando o usuário observa um grupo expandido
  Então o rótulo aparece como pílula na cor cheia do grupo
  E a cápsula do grupo é pintada nessa mesma cor atrás das abas
  E nenhuma linha colorida é desenhada na base das abas

Cenário: indicador combinado, ligado pelo usuário
  Dado um grupo "api" de cor azul
  Quando o usuário define o estilo do indicador como ["pill", "underline"]
  Então o rótulo aparece como pílula
  E cada aba do grupo recebe linha azul na base

Cenário: estilo de indicador reduzido a uma forma
  Dado um grupo "api" de cor azul
  Quando o usuário define o estilo do indicador como ["underline"]
  Então as abas do grupo recebem linha azul embaixo
  E nenhuma pílula é desenhada

Cenário: aba ativa distinguível sem cor
  Dado a configuração padrão em escala de cinza
  Quando o usuário observa a barra
  Então a aba ativa é identificável por contraste e borda, não só por matiz

Cenário: barra oculta com uma aba
  Dado a barra configurada para ocultar quando há uma só aba
  Quando existe apenas uma aba aberta
  Então a barra não é desenhada
  E ela reaparece ao abrir a segunda aba

Cenário: chave desconhecida não quebra
  Dado uma config contendo uma chave inexistente
  Quando o app carrega a configuração
  Então um aviso é exibido
  E o restante da configuração é aplicado normalmente
```

## Fora de escopo

- **Faixa de identidade da barra de título** (logo, nome do app, título da aba ativa em faixa própria) — desenhada no canvas, fase `[v2]`. Decorações nativas continuam só no macOS; Windows/Linux já têm controles de janela e resize próprios, resolvidos na barra de abas existente, não numa faixa nova ([ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md), que supersede parcialmente [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md) nisso)
- **Painel de configurações por GUI** — desenhado no canvas, fase `[v2]`. Quando existir, escreverá no TOML, sem introduzir segunda fonte de verdade ([ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md))
- Temas visuais de chrome distribuídos como arquivo separado e importável (v2)
- Aparência definida por script ou lógica ([ADR-0003](../adr/0003-formato-de-configuracao.md))
- Imagem de fundo na barra de abas
- Animações configuráveis além de ligar e desligar
- Ícone por aba ou por grupo
- Herdar o tema claro/escuro do sistema automaticamente (v2)

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Valores de aparência hardcoded no código | **zero** |
| Tempo entre salvar a config e ver o resultado | < 500 ms |
| Config inválida que derruba o app | zero ocorrências |
| Chaves de aparência sem default declarado | zero |
| Chaves no arquivo de exemplo sem requisito correspondente | zero |
