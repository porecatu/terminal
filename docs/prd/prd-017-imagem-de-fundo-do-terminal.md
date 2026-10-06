# PRD-017 — Imagem de fundo do terminal

**Status:** Aprovado
**Data:** 2026-10-05
**Requisito de origem:** pedido direto do dono do produto, estendendo a opacidade do fundo do terminal ([PRD-005](prd-005-aparencia-do-terminal.md) RF-5.15)
**Relacionados:** [ADR-0061](../adr/0061-imagem-de-fundo-do-terminal.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0018](../adr/0018-composicao-de-frame.md), [ADR-0030](../adr/0030-escopo-do-hot-reload.md), [ADR-0032](../adr/0032-interface-do-v1-fechada.md), [ADR-0053](../adr/0053-paineis-divididos.md), [ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md), [PRD-005](prd-005-aparencia-do-terminal.md), [PRD-006](prd-006-paineis-divididos.md), [PRD-016](prd-016-tela-de-configuracoes.md)

> Aprovado em 2026-10-05, por decisão do dono do produto, **fora da ordem de fases**, como o arquivo de projeto ([PRD-012](prd-012-comando-de-projeto-por-diretorio.md)) e as sessões nomeadas ([PRD-014](prd-014-sessoes-nomeadas.md)). Não é elemento do canvas nem rascunho promovido: é requisito novo, escrito com o v1 em uso. Muda pixel na área de terminal (§2.7 da [especificação visual](../design/especificacao-visual.md)), então passa pelo ADR que o [ADR-0032](../adr/0032-interface-do-v1-fechada.md) exige: o [ADR-0061](../adr/0061-imagem-de-fundo-do-terminal.md), que decide também a decodificação, a primitiva de render e a composição com a transparência.

## Problema

O fundo do terminal hoje é uma cor só: `[terminal.colors] background`, ou a do tema, com a opacidade de `[terminal] background_opacity` por cima. É o que o [PRD-005](prd-005-aparencia-do-terminal.md) pediu, e é suficiente para quem quer um terminal sóbrio.

Não é suficiente para quem quer um terminal **seu**. Pôr uma imagem atrás do texto é a personalização mais pedida de terminal depois de tema e fonte, e os terminais com que o Porecatu é comparado (Windows Terminal, iTerm2, Kitty, WezTerm) têm todos. Quem vem de um deles e não acha a opção concluiu que o Porecatu não tem, não que ela está noutro lugar.

Três coisas tornam o recurso menos trivial do que "desenhar um PNG atrás da grade":

- **O app tem muitos terminais na tela.** Abas, grupos e, desde o [PRD-006](prd-006-paineis-divididos.md), painéis — um quadro arredondado por painel. A imagem tem de valer em todos, sem o usuário configurar cada um.
- **A opacidade já existe, e as duas precisam conversar.** Quem já usa `background_opacity < 1` para ver o desktop através do terminal não pode perder isso ao pôr uma imagem. E quem quer uma imagem discreta precisa esmaecê-la **sem** mexer na transparência da janela.
- **Texto continua sendo o produto.** Imagem que atrapalha a leitura é desligada no dia seguinte. A opacidade da imagem é o controle que mantém o texto legível sobre ela.

## Usuário-alvo

O mesmo do [PRD-000](prd-000-visao-de-produto.md) — quem vive no terminal o dia inteiro, com muitas abas — no momento em que decide que aquilo vai ter a cara dele. Uma configuração de uma vez só, não algo que se troca por aba.

## Em uma tela

```toml
[terminal.background_image]
# Vazio = sem imagem. Relativo ao diretório deste arquivo; `~` é a pasta pessoal.
path = "imagens/montanha.jpg"
# "stretch" (estica até o quadro, distorcendo), "tile" (lado a lado, no
# tamanho natural) ou "center" (centralizada, no tamanho natural).
mode = "stretch"
# Opacidade da imagem, independente da do terminal. Multiplica por
# [terminal] background_opacity.
opacity = 0.35
```

O que o usuário vê: cada quadro de terminal — de toda aba, de todo grupo, de cada painel de uma aba dividida — com a imagem atrás do texto, a 35% de opacidade, esticada até as bordas arredondadas do quadro.

## Requisitos funcionais

### A imagem

**RF-17.1** — A chave `[terminal.background_image] path` escolhe a imagem. Vazia ou ausente, não há imagem, e o terminal é desenhado exatamente como hoje. **O padrão é vazio.**

**RF-17.2** — Formatos aceitos: **PNG** e **JPEG**, reconhecidos pelo conteúdo do arquivo e não pela extensão — um `.jpg` que é PNG por dentro funciona. Qualquer outro formato é recusado com aviso (RF-17.11). O canal alfa de um PNG é respeitado: onde a imagem é transparente, aparece o fundo do terminal.

**RF-17.3** — Caminho relativo é resolvido contra o **diretório do arquivo de configuração em uso** (o do `--config`, de `PORECATU_CONFIG` ou o padrão da plataforma), não contra o diretório de onde o app foi lançado. Caminho absoluto é usado como está. `~` no início, seguido de `/` ou `\`, é a pasta pessoal.

**RF-17.4** — Uma imagem por configuração, igual em todo terminal. Não há imagem por aba, por grupo nem por painel (ver Fora de escopo).

### Modos

**RF-17.5** — A chave `mode` escolhe como a imagem ocupa o quadro. O padrão é `stretch`.

- **`stretch`** — a imagem é esticada até preencher o quadro inteiro, nos dois eixos, **sem manter a proporção**. Um quadro largo achata a imagem.
- **`tile`** — a imagem é repetida lado a lado no **tamanho natural**, a partir do canto superior esquerdo do quadro, até cobri-lo. Os ladrilhos cortados na borda direita e na de baixo ficam cortados.
- **`center`** — a imagem fica no tamanho natural, centralizada no quadro. Menor que o quadro, sobra fundo do terminal em volta. Maior, é cortada nas bordas, mantendo o centro.

**RF-17.6** — **Tamanho natural** é um pixel da imagem para um pixel físico da tela. A imagem não cresce com a escala da janela nem com o zoom de fonte. Numa janela que muda de monitor, ela é redesenhada na escala nova sem ser recarregada.

### Onde e em que ordem

**RF-17.7** — A imagem é desenhada em **todo quadro de terminal**: aba solta, aba de grupo, e cada painel de uma aba dividida, cada painel com a **imagem inteira** posta no próprio quadro pelo modo escolhido. Os painéis não dividem uma imagem só entre eles, e o vão entre dois painéis não mostra imagem.

**RF-17.8** — A imagem ocupa o **quadro inteiro** do terminal, inclusive o padding entre a borda do quadro e a grade, e é recortada pelo raio do quadro: os cantos arredondados continuam arredondados. Ela nunca sai do quadro. Não aparece na barra de abas, na barra de status, no vão em volta do quadro, nem na janela de configurações.

> **Emenda ([PRD-018](prd-018-imagem-de-fundo-da-janela.md)).** Continua valendo para **esta** imagem, a do terminal. A barra de abas, a barra de status, a margem e o vão podem ter agora uma imagem **da janela**, outra chave, desenhada abaixo dos quadros e, com o terminal translúcido, visível através deles — abaixo do fundo do quadro e, portanto, abaixo desta imagem ([ADR-0062](../adr/0062-imagem-de-fundo-da-janela.md) §4).

**RF-17.9** — A imagem fica **acima** do fundo do quadro e **abaixo** de tudo o que a grade desenha: fundo de célula, texto, sublinhado, seleção, realce de busca, affordance de hyperlink e cursor. A barra de busca, os avisos, os menus e os diálogos continuam por cima de tudo.

**RF-17.10** — Célula sem cor de fundo própria deixa a imagem aparecer. Célula com cor de fundo própria — ANSI, 256 cores ou true color, posta pelo programa — **cobre** a imagem, como cobre o fundo hoje. Programa de tela cheia que pinta o próprio fundo (um `htop`, um editor com tema) esconde a imagem onde pinta, e isso é o esperado.

### Opacidade e transparência

**RF-17.11** — A chave `opacity`, de `0.0` a `1.0`, é a opacidade da imagem, **independente** de `[terminal] background_opacity` e de `[appearance.window] opacity`. O padrão é `1.0`. `0.0` não desenha a imagem, mas a carrega (é um valor, não um desligamento).

**RF-17.12** — A imagem **obedece à transparência do terminal**: a opacidade efetiva dela é `opacity × background_opacity`. Com o terminal a `0.8` e a imagem a `0.5`, a imagem é desenhada a `0.4`. Um terminal transparente nunca fica opaco por ter imagem: com a janela transparente, o que está atrás da janela continua aparecendo através do quadro, imagem incluída.

**RF-17.13** — Com a janela opaca e `background_opacity < 1`, a imagem se mistura ao fundo do quadro como o fundo já se mistura à barra hoje. A imagem não liga a transparência da janela por conta própria: o que decide se a janela nasce transparente continua sendo `background_opacity` e `[appearance.window] opacity` ([ADR-0030](../adr/0030-escopo-do-hot-reload.md)).

### Erros

**RF-17.14** — Arquivo inexistente, ilegível, de formato não aceito, corrompido ou grande demais para ser carregado **não derruba nada**: o terminal é desenhado sem imagem, e um **aviso** da barra de avisos ([ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md)) diz qual arquivo e por quê. O aviso aparece no arranque e na recarga a quente, como os de configuração inválida (RF-4.21), e **uma vez** por arquivo e por problema, não a cada frame nem a cada janela.

**RF-17.15** — Imagem maior que o maior tamanho de textura que a placa de vídeo aceita é **reduzida**, mantendo a proporção, até caber, sem aviso: o usuário pediu a imagem, e ela aparece. O limite de memória que protege contra arquivo malformado (o arquivo pequeno que diz ter 100 000 × 100 000 pixels) é outro, e esse dá aviso (RF-17.14).

### Recarga e desempenho

**RF-17.16** — Mudar `path`, `mode` ou `opacity` vale **na hora**, em todas as janelas, sem reiniciar e sem redimensionar terminal nenhum — classe A do [ADR-0030](../adr/0030-escopo-do-hot-reload.md). Trocar o conteúdo do arquivo de imagem no disco, mantendo o caminho, vale na próxima recarga da configuração. O app **não vigia** o arquivo de imagem.

**RF-17.17** — A imagem é lida e decodificada **fora** da thread da interface. O arranque não espera por ela: a janela abre e o terminal funciona sem imagem até ela ficar pronta, e então ela aparece.

**RF-17.18** — Uma imagem é decodificada e enviada à placa de vídeo **uma vez por processo**, e todas as janelas e painéis desenham a mesma cópia. Dez painéis não custam dez imagens em memória.

**RF-17.19** — Com a imagem configurada e nada mudando, o app continua sem desenhar frame ([ADR-0007](../adr/0007-modelo-de-threading.md)): a imagem é parada e não acorda o loop.

### Tela de configurações

**RF-17.20** — A tela de configurações ([PRD-016](prd-016-tela-de-configuracoes.md)) ganha três opções no grupo **Terminal**, junto da opacidade do fundo: **imagem de fundo** (`terminal.background_image.path`, campo de texto), **modo da imagem** (`mode`, escolha entre os três) e **opacidade da imagem** (`opacity`, campo numérico de `0.0` a `1.0`). Cada uma tem Restaurar padrão (RF-16.16). O caminho é texto, como todo caminho da tela (RF-16.12): não há seletor de arquivo do sistema.

**RF-17.21** — O campo do caminho mostra abaixo dele, sem impedir o Salvar, quando o arquivo **não existe** no caminho resolvido. Outros problemas (formato, arquivo corrompido) só aparecem no aviso do RF-17.14, depois da recarga, porque descobri-los exige decodificar a imagem.

**RF-17.22** — Toda frase nova (rótulos, nota do RF-17.21, avisos do RF-17.14) vem do catálogo de textos, em todo arquivo de `locales/` ([ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md)).

## Cenários

```gherkin
Cenário: o caso que motiva o recurso
  Dado um porecatu.toml com [terminal.background_image] path = "fundo.jpg"
  E um JPEG "fundo.jpg" no mesmo diretório do porecatu.toml
  Quando o usuário abre o app
  Então o quadro do terminal mostra a imagem esticada até as bordas arredondadas
  E o texto do prompt aparece por cima dela

Cenário: todo terminal tem a imagem
  Dado uma imagem configurada
  E uma janela com uma aba solta, um grupo com duas abas e uma aba dividida em três painéis
  Quando o usuário passa por todas as abas
  Então todo quadro de terminal mostra a imagem
  E cada um dos três painéis mostra a imagem inteira no próprio quadro
  E o vão entre os painéis não mostra imagem

Cenário: os três modos
  Dado uma imagem PNG de 200×200 pixels e um quadro de terminal bem maior que ela
  Quando mode = "stretch"
  Então a imagem cobre o quadro inteiro, distorcida
  Quando mode = "tile"
  Então a imagem se repete a partir do canto superior esquerdo, cada cópia com 200×200 pixels físicos
  Quando mode = "center"
  Então uma cópia de 200×200 pixels físicos aparece no centro, com o fundo do terminal em volta

Cenário: opacidades que se multiplicam
  Dado background_opacity = 0.8 e a janela transparente
  E opacity = 0.5 na imagem
  Quando o terminal é desenhado
  Então a imagem aparece a 40% de opacidade
  E o que está atrás da janela aparece através do quadro

Cenário: a imagem não torna opaco um terminal transparente
  Dado background_opacity = 0.5 e a janela transparente
  E opacity = 1.0 na imagem
  Quando o terminal é desenhado
  Então o que está atrás da janela continua aparecendo através do quadro

Cenário: programa que pinta o próprio fundo
  Dado uma imagem configurada
  Quando o usuário roda um programa que pinta o fundo das células com uma cor própria
  Então essas células cobrem a imagem
  E as células sem cor própria continuam mostrando a imagem

Cenário: arquivo que não existe
  Dado path = "nao-existe.png"
  Quando o app abre
  Então o terminal é desenhado sem imagem
  E um aviso diz que "nao-existe.png" não foi encontrado, com o caminho resolvido
  E o aviso aparece uma vez, não uma por janela

Cenário: formato não aceito
  Dado path apontando para um arquivo GIF
  Quando a configuração é recarregada
  Então o terminal segue sem imagem
  E um aviso diz que o formato não é aceito, e que os aceitos são PNG e JPEG

Cenário: troca ao vivo
  Dado o app aberto com uma imagem em mode = "stretch"
  Quando o usuário salva mode = "center" no porecatu.toml
  Então em menos de 500 ms todo quadro passa a mostrar a imagem centralizada
  E nenhum terminal é redimensionado

Cenário: arranque não espera
  Dado um JPEG de 8000×6000 pixels configurado
  Quando o app abre
  Então a janela e o prompt aparecem no mesmo tempo de sem imagem
  E a imagem aparece quando terminar de carregar

Cenário: pela tela de configurações
  Dado a tela de configurações aberta no grupo Terminal
  Quando o usuário escreve um caminho que não existe no campo da imagem de fundo
  Então a linha diz que o arquivo não foi encontrado
  E Salvar continua disponível
```

## Fora de escopo

- **Imagem por aba, por grupo ou por painel.** Uma imagem global atende o pedido ("ao fundo de cada terminal"). Imagem por grupo mexeria no editor de grupo e no arquivo de sessão, e é um recurso próprio se for pedido.
- **Uma imagem pela aba inteira**, com cada painel mostrando o pedaço que fica atrás dele. Decidido contra: cada painel é um terminal inteiro ([ADR-0053](../adr/0053-paineis-divididos.md)), e a imagem inteira em cada um é o que se espera de "cada terminal".
- **Um quarto modo que preenche mantendo a proporção** (`fill`/*cover*) ou que cabe mantendo a proporção (`fit`/*contain*). São três modos, decisão do dono do produto; acrescentar um é aditivo.
- **GIF, WebP, BMP, SVG e imagem animada.**
- **Desfoque, escurecimento ou tingimento da imagem** além da opacidade. Não há primitiva de filtro em `porecatu-render`, e é definitivo ([CLAUDE.md](../../CLAUDE.md), "Descobertas na F2").
- **Seletor de arquivo do sistema.** Nenhum diálogo nativo ([ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md)); caminho é texto (RF-16.12).
- **Vigiar o arquivo de imagem** e recarregá-lo sozinho quando ele muda.
- **Imagem no chrome**: barra de abas, barra de status, widgets, janela de configurações.

  > **Emenda (2026-10-06).** A barra de abas e a barra de status saíram daqui pelo [PRD-018](prd-018-imagem-de-fundo-da-janela.md), com uma imagem da janela inteira e chave própria. Widgets e janela de configurações continuam fora.
- **Imagem de fundo dentro de um tema** (`[[themes]]`). Tema é paleta de cores ([ADR-0031](../adr/0031-temas-nomeados.md)).
- **Troca periódica de imagem** (slideshow) e imagem baixada de URL.

## Métricas de sucesso

- **Configurar uma imagem: uma chave.** `path` sozinho, com os padrões, já desenha a imagem em todo terminal.
- **Tempo até o primeiro prompt com imagem configurada: igual ao sem imagem**, medido pela instrumentação do PRD-011 (`PORECATU_TRACE`). A imagem não entra no caminho do arranque.
- **Frames com o terminal ocioso e uma imagem configurada: zero**, como sem imagem.
- **Memória de imagem: uma cópia por processo**, qualquer que seja o número de janelas e de painéis.
- **Configuração errada que derruba o app ou deixa o terminal sem desenhar: zero.**
