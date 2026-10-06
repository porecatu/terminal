# PRD-018 — Imagem de fundo da janela

**Status:** Aprovado
**Data:** 2026-10-06
**Requisito de origem:** pedido direto do dono do produto, estendendo a imagem de fundo do terminal ([PRD-017](prd-017-imagem-de-fundo-do-terminal.md)) ao quadro do app
**Relacionados:** [ADR-0062](../adr/0062-imagem-de-fundo-da-janela.md), [ADR-0061](../adr/0061-imagem-de-fundo-do-terminal.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0018](../adr/0018-composicao-de-frame.md), [ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md), [ADR-0030](../adr/0030-escopo-do-hot-reload.md), [ADR-0032](../adr/0032-interface-do-v1-fechada.md), [ADR-0048](../adr/0048-barra-de-status.md), [ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md), [ADR-0059](../adr/0059-janela-de-configuracoes.md), [PRD-004](prd-004-aparencia-do-chrome.md), [PRD-016](prd-016-tela-de-configuracoes.md), [PRD-017](prd-017-imagem-de-fundo-do-terminal.md)

> Aprovado em 2026-10-06, por decisão do dono do produto, **fora da ordem de fases**, pelo mesmo caminho do [PRD-017](prd-017-imagem-de-fundo-do-terminal.md): requisito novo, escrito com o v1 em uso, sem elemento do canvas. Revoga dois "fora de escopo" escritos antes — "imagem de fundo na barra de abas" do [PRD-004](prd-004-aparencia-do-chrome.md) e "imagem no chrome" do PRD-017 —, os dois emendados por blockquote, não apagados. Muda pixel na barra de abas, na barra de status, na margem e no vão entre painéis (§2.2, §2.7 e §2.8 da [especificação visual](../design/especificacao-visual.md)), então passa pelo ADR que o [ADR-0032](../adr/0032-interface-do-v1-fechada.md) exige: o [ADR-0062](../adr/0062-imagem-de-fundo-da-janela.md), que decide também a camada, a composição com a transparência e o estado de carga compartilhado com a imagem do terminal.

## Problema

O PRD-017 pôs uma imagem **dentro** de cada quadro de terminal e deixou de propósito todo o resto do app numa cor só: a barra de abas, a barra de status, a margem em volta do quadro e o vão entre painéis são o `bar_background` (`#1b1f26`), sem exceção. Para quem quer o app com a cara dele, isso é metade do pedido. A moldura é o que se vê em volta do texto o dia inteiro, e é a parte da janela que mais se parece com "o app" e menos com "o terminal".

Três coisas fazem desta uma extensão e não uma cópia do PRD-017:

- **A moldura é uma superfície só.** A barra de abas, a margem, o vão e a barra de status formam, juntos, o "quadro do app" (é o comentário que o código já tem sobre a cor da margem). Uma imagem que recomeçasse em cada pedaço sairia picada; ela é **uma** imagem, posta na janela inteira.
- **Ela fica atrás dos terminais, não ao lado.** Com o terminal translúcido (`background_opacity < 1`), o que aparece através do quadro hoje é o desktop. Com uma imagem de janela, passa a ser ela — a imagem faz o papel de papel de parede do próprio app.
- **As duas imagens convivem.** A do terminal continua dentro do quadro, por cima do fundo dele; a da janela fica embaixo de tudo. Uma não desliga a outra.

## Usuário-alvo

O mesmo do [PRD-017](prd-017-imagem-de-fundo-do-terminal.md): quem vive no terminal o dia inteiro e decide que o app vai ter a cara dele. Uma configuração de uma vez só, igual em toda janela.

## Em uma tela

```toml
[appearance.window.background_image]
# Vazio = sem imagem. Relativo ao diretório deste arquivo; `~` é a pasta pessoal.
path = "imagens/praia.jpg"
# "stretch", "tile" ou "center", como em [terminal.background_image].
mode = "stretch"
# Opacidade da imagem. Multiplica por [appearance.window] opacity.
opacity = 1.0
```

O que o usuário vê: a imagem esticada na janela inteira, aparecendo atrás das abas, entre elas, na barra de status e na margem em volta do quadro do terminal. Os quadros de terminal continuam por cima, com o fundo deles; se o terminal for translúcido, a imagem aparece através dele.

## Requisitos funcionais

### A imagem

**RF-18.1** — A chave `[appearance.window.background_image] path` escolhe a imagem. Vazia ou ausente, não há imagem, e a janela é desenhada exatamente como hoje. **O padrão é vazio.**

**RF-18.2** — Formatos, reconhecimento pelo conteúdo e canal alfa: os do RF-17.2. Onde um PNG é transparente, aparece o fundo da janela (`bar_background`), não o desktop.

**RF-18.3** — Resolução do caminho: a do RF-17.3, relativa ao diretório do arquivo de configuração em uso.

**RF-18.4** — Uma imagem por configuração, igual em toda janela de terminal. Não há imagem por janela nem por grupo.

**RF-18.5** — A imagem da janela é **independente** da do terminal ([PRD-017](prd-017-imagem-de-fundo-do-terminal.md)): cada uma tem as próprias três chaves, e qualquer combinação vale — só a da janela, só a do terminal, as duas, nenhuma. As duas podem apontar para o mesmo arquivo.

### Modos

**RF-18.6** — A chave `mode` aceita os mesmos três valores do RF-17.5 (`stretch`, `tile`, `center`), com o mesmo significado e o mesmo padrão, `stretch`. A referência é a **área de conteúdo da janela inteira**, não um quadro: `stretch` estica até as quatro bordas da janela, `tile` começa no canto superior esquerdo da janela, `center` centraliza na janela.

**RF-18.7** — Tamanho natural é o do RF-17.6: um pixel da imagem por pixel físico. Redimensionar a janela, maximizá-la, entrar em tela cheia ou mudar de monitor **refaz a posição** da imagem sem recarregá-la; no `stretch` ela acompanha o tamanho novo.

### Onde e em que ordem

**RF-18.8** — A imagem cobre a **área de conteúdo inteira** da janela: a barra de abas (inclusive atrás dos controles de janela do [ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md) e, no macOS, atrás dos botões nativos sobre a barra), a barra de status, a margem entre a borda da janela e o quadro do terminal e o vão entre painéis. Ela não cobre a borda de 1px da janela, que continua por cima.

**RF-18.9** — A imagem fica **acima** do fundo da janela e **abaixo** de tudo o mais: quadros de terminal (com sombra, fundo, imagem do terminal e grade), abas, cápsulas e pílulas de grupo, ícones, textos e indicadores da barra de abas e da barra de status. Avisos, menus, tooltip, popovers, editor de grupo, diálogos e barra de busca continuam por cima de tudo e não mostram a imagem por baixo do próprio fundo.

**RF-18.10** — Com a imagem exibida, a barra de abas **deixa de pintar o fundo próprio** sobre ela. As abas, as cápsulas e as pílulas continuam pintadas como hoje, com a translucidez que já têm, e por isso deixam a imagem transparecer na medida dessa translucidez.

**RF-18.11** — **Através do terminal translúcido.** Com `[terminal] background_opacity < 1`, a imagem da janela aparece através do quadro de terminal, no lugar do que hoje aparece ali (o desktop). Ela entra **abaixo** do fundo do quadro, e a imagem do terminal, se houver, fica acima desse fundo (RF-17.9). Com `background_opacity = 1`, o quadro é opaco e a imagem da janela não aparece dentro dele.

**RF-18.12** — Célula com cor de fundo própria **cobre as duas imagens**, como o RF-17.10 já decide para a do terminal: onde um programa pinta o fundo, aparece a cor dele, com a transparência de `background_opacity`, como sem imagem nenhuma.

**RF-18.13** — A imagem **não aparece na janela de configurações** ([ADR-0059](../adr/0059-janela-de-configuracoes.md)), que segue com o fundo dela.

### Opacidade e transparência

**RF-18.14** — A chave `opacity`, de `0.0` a `1.0`, é a opacidade da imagem. O padrão é `1.0`. `0.0` não desenha, mas carrega (RF-17.11).

**RF-18.15** — A imagem **obedece à transparência da janela**: a opacidade efetiva é `opacity × [appearance.window] opacity`. Uma janela transparente nunca fica opaca por ter imagem. `[terminal] background_opacity` **não** entra nessa conta fora dos quadros; dentro deles, vale o RF-18.11.

**RF-18.16** — A imagem **não liga** a transparência da janela por conta própria: o que decide se a janela nasce transparente continua sendo `background_opacity` e `[appearance.window] opacity` (RF-17.13). Com `opacity < 1` numa janela opaca, a imagem se mistura ao fundo da janela, não ao desktop.

### Erros

**RF-18.17** — Arquivo inexistente, ilegível, de formato não aceito, corrompido ou grande demais: o comportamento do RF-17.14 — a janela é desenhada sem imagem e um aviso diz qual arquivo e por quê, uma vez por arquivo e por problema. O aviso diz que a imagem é **a da janela**, para não ser confundido com o da imagem do terminal.

**RF-18.18** — Imagem maior que a maior textura aceita pela placa de vídeo: reduzida sem aviso, como no RF-17.15.

### Recarga e desempenho

**RF-18.19** — Mudar `path`, `mode` ou `opacity` vale **na hora**, em todas as janelas, sem reiniciar e sem redimensionar terminal nenhum (classe A do [ADR-0030](../adr/0030-escopo-do-hot-reload.md)). O arquivo de imagem não é vigiado (RF-17.16).

**RF-18.20** — Leitura e decodificação fora da thread da interface, e o arranque não espera por ela (RF-17.17): a janela abre com o fundo de hoje e a imagem aparece quando fica pronta, depois do primeiro prompt, como a do terminal.

**RF-18.21** — Uma decodificação e uma textura por processo, qualquer que seja o número de janelas (RF-17.18). Se a imagem da janela e a do terminal são **o mesmo arquivo**, é uma só, para as duas.

**RF-18.22** — Com a imagem configurada e nada mudando, nenhum frame é desenhado ([ADR-0007](../adr/0007-modelo-de-threading.md)).

### Tela de configurações

**RF-18.23** — A tela de configurações ([PRD-016](prd-016-tela-de-configuracoes.md)) ganha três opções no grupo **Aparência**, junto da opacidade da janela: **imagem de fundo da janela** (`appearance.window.background_image.path`, campo de texto), **modo da imagem da janela** (`mode`, escolha entre os três) e **opacidade da imagem da janela** (`opacity`, campo numérico de `0.0` a `1.0`). Cada uma tem Restaurar padrão. O caminho é texto, sem seletor de arquivo do sistema.

**RF-18.24** — O campo do caminho mostra, abaixo dele e sem impedir o Salvar, quando o arquivo **não existe** no caminho resolvido — a mesma nota do RF-17.21.

**RF-18.25** — Toda frase nova (rótulos, nota, avisos) vem do catálogo de textos, em todo arquivo de `locales/` ([ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md)).

## Cenários

```gherkin
Cenário: o caso que motiva o recurso
  Dado um porecatu.toml com [appearance.window.background_image] path = "praia.jpg"
  E um JPEG "praia.jpg" no mesmo diretório do porecatu.toml
  E o terminal opaco (background_opacity = 1.0)
  Quando o usuário abre o app
  Então a imagem aparece esticada na janela inteira
  E é vista atrás das abas, entre elas, na barra de status e na margem em volta do quadro
  E o quadro do terminal continua opaco por cima dela

Cenário: através do terminal translúcido
  Dado uma imagem de janela configurada com opacity = 1.0
  E background_opacity = 0.7
  Quando o terminal é desenhado
  Então a imagem da janela aparece através do quadro do terminal
  E o desktop não aparece através do quadro

Cenário: as duas imagens juntas
  Dado uma imagem de janela e uma imagem de terminal diferentes
  E background_opacity = 0.7
  Quando o terminal é desenhado
  Então a imagem do terminal aparece por cima do fundo do quadro
  E a imagem da janela aparece por baixo dele
  E fora dos quadros só a imagem da janela aparece

Cenário: uma imagem só pela janela, não por painel
  Dado uma imagem de janela em mode = "stretch"
  E uma aba dividida em dois painéis lado a lado, com background_opacity = 0.7
  Quando o terminal é desenhado
  Então a imagem é uma só, esticada na janela inteira
  E o vão entre os painéis mostra o pedaço da imagem que fica atrás dele

Cenário: os três modos
  Dado uma imagem PNG de 200×200 pixels e uma janela bem maior que ela
  Quando mode = "tile"
  Então a imagem se repete a partir do canto superior esquerdo da janela, cada cópia com 200×200 pixels físicos
  Quando mode = "center"
  Então uma cópia de 200×200 pixels físicos fica no centro da janela, com o fundo da janela em volta

Cenário: redimensionar
  Dado uma imagem de janela em mode = "stretch"
  Quando o usuário maximiza a janela
  Então a imagem passa a cobrir a janela maximizada inteira
  E não é recarregada do disco

Cenário: janela transparente
  Dado [appearance.window] opacity = 0.8
  E opacity = 1.0 na imagem da janela
  Quando a janela é desenhada
  Então a imagem aparece a 80% de opacidade
  E o que está atrás da janela continua aparecendo

Cenário: programa que pinta o próprio fundo
  Dado uma imagem de janela, background_opacity = 0.7 e a janela transparente
  Quando o usuário roda um programa que pinta o fundo das células com uma cor própria
  Então essas células mostram a cor delas, sem a imagem da janela por trás
  E as células sem cor própria continuam mostrando a imagem da janela através do quadro

Cenário: a janela de configurações não muda
  Dado uma imagem de janela configurada
  Quando o usuário abre a tela de configurações
  Então a janela de configurações não mostra imagem

Cenário: arquivo que não existe
  Dado path = "nao-existe.png" em [appearance.window.background_image]
  Quando o app abre
  Então a janela é desenhada sem imagem
  E um aviso diz que a imagem de fundo da janela "nao-existe.png" não foi encontrada, com o caminho resolvido
  E o aviso aparece uma vez, não uma por janela

Cenário: mesmo arquivo nas duas
  Dado o mesmo path em [appearance.window.background_image] e em [terminal.background_image]
  Quando o app abre
  Então o arquivo é decodificado uma vez
  E as duas imagens são desenhadas com a mesma textura

Cenário: troca ao vivo
  Dado o app aberto com uma imagem de janela em mode = "stretch"
  Quando o usuário salva mode = "center" no porecatu.toml
  Então em menos de 500 ms toda janela passa a mostrar a imagem centralizada
  E nenhum terminal é redimensionado

Cenário: pela tela de configurações
  Dado a tela de configurações aberta no grupo Aparência
  Quando o usuário escreve um caminho que não existe no campo da imagem de fundo da janela
  Então a linha diz que o arquivo não foi encontrado
  E Salvar continua disponível
```

## Fora de escopo

- **Imagem por janela ou por grupo.** Uma imagem global, como a do terminal.
- **Imagem na janela de configurações**, nos avisos, menus, tooltip, popovers, editor de grupo e diálogos.
- **Imagem por baixo da decoração nativa do macOS** além do que já é nosso pixel: a barra transparente com o conteúdo por baixo ([ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md)) já deixa a área inteira para o app; a moldura do sistema não é desenhada por nós.
- **Um quarto modo** (`fill`/`fit`), desfoque, escurecimento ou tingimento, GIF/WebP/BMP/SVG, imagem animada, imagem em tema, troca periódica, URL e vigiar o arquivo — pelas mesmas razões do [PRD-017](prd-017-imagem-de-fundo-do-terminal.md).
- **Dar uso ao token `[appearance.window] background`** (`#15181d`), que existe no arquivo e nunca foi lido. Ele não vira o fundo atrás da imagem; o fundo continua sendo o `bar_background`.
- **Uma imagem que respeite a sombra do quadro do terminal** de outro jeito que não pela ordem de pintura: a sombra é pintada por cima da imagem, como é pintada por cima do fundo.

## Métricas de sucesso

- **Configurar uma imagem de janela: uma chave.** `path` sozinho já desenha a imagem na janela inteira.
- **Tempo até o primeiro prompt com imagem de janela: igual ao sem imagem** (`PORECATU_TRACE`, PRD-011).
- **Frames com o terminal ocioso e imagem de janela configurada: zero.**
- **Memória de imagem: uma cópia por arquivo distinto por processo** — no máximo duas, uma por imagem configurada, e uma só quando as duas são o mesmo arquivo.
- **Configuração errada que derruba o app ou deixa a janela sem desenhar: zero.**
