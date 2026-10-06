# ADR-0062 — Imagem de fundo da janela: cabeça da camada da grade, recorte por quadro e estado em dois slots

**Status:** Aceito
**Data:** 2026-10-06
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0015](0015-multiplas-janelas.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0048](0048-barra-de-status.md), [ADR-0053](0053-paineis-divididos.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0059](0059-janela-de-configuracoes.md), [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md), [ADR-0061](0061-imagem-de-fundo-do-terminal.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md), [PRD-017](../prd/prd-017-imagem-de-fundo-do-terminal.md), [PRD-018](../prd/prd-018-imagem-de-fundo-da-janela.md)

## Contexto

O [PRD-018](../prd/prd-018-imagem-de-fundo-da-janela.md) pede uma imagem atrás da janela inteira — barra de abas, barra de status, margem e vão entre painéis —, abaixo dos quadros de terminal e visível através deles quando o terminal é translúcido. O [ADR-0061](0061-imagem-de-fundo-do-terminal.md) já resolveu decodificação, primitiva, espaço de cor, carga e erros para a imagem do terminal, e quase tudo dele serve aqui sem mudança. Quatro coisas ele não resolve, porque a imagem do terminal mora **dentro** de um quadro e esta mora **fora** de todos:

1. **A janela não tem fundo pintado.** O "quadro do app" é a cor de `clear` do passe de render (`pal.bar_background`, passada a `WindowSurface::render`). A margem, o vão e a área da barra de status não recebem primitiva nenhuma — o que se vê ali é o `clear`. A barra de abas pinta por cima um `Quad` opaco da mesma cor, em `Layer::Chrome`. Não existe camada abaixo de `Grid` ([ADR-0018](0018-composicao-de-frame.md)).
2. **A imagem precisa ficar abaixo de `Grid`**, porque os quadros de terminal (sombra, fundo, imagem do terminal, células) estão lá, e acima do `clear`.
3. **Em janela transparente, o fundo do quadro apaga o que está embaixo.** `paint::backdrop_fill` emite `Primitive::Backdrop`, com `BlendState::REPLACE`, quando a surface é transparente — é o que faz o desktop aparecer através do terminal. Uma imagem pintada embaixo seria apagada exatamente na área em que o PRD pede que ela apareça (RF-18.11).
4. **O estado de carga do ADR-0061 é de um slot só.** `BackgroundImageStore` guarda a imagem atual e a anterior de **uma** chave; `Wakeup::BackgroundImageLoaded` não diz de qual imagem é o resultado; o aviso de erro fala de "terminal".

E é mudança das §2.2, §2.7 e §2.8 da [especificação visual](../design/especificacao-visual.md): o [ADR-0032](0032-interface-do-v1-fechada.md) exige ADR, e este é ele.

## Decisão

### 1. Nada novo no render nem no `Cargo.lock`

A imagem da janela usa o que o ADR-0061 criou: o crate `image` (PNG e JPEG), o registro de imagens do `GpuContext`, `Primitive::Image`, `image.wgsl` com a SDF compartilhada, textura `Rgba8Unorm`, saída premultiplicada, mips gerados na thread de carga. Nenhuma dependência, primitiva, shader ou pipeline novo. `porecatu-render` não aprende o que é "janela": recebe `Image`s como recebe hoje.

### 2. Na cabeça de `Layer::Grid`, não numa camada nova

A imagem da janela é a **primeira primitiva da lista de `Layer::Grid`**, antes da sombra e do fundo do primeiro quadro de terminal. Como `resolve_layer` respeita a ordem da lista para geometria, isso a põe acima do `clear` e abaixo de tudo o que a grade e as camadas seguintes desenham (RF-18.9), sem tocar em `Layer`.

- `rect` e `mask`: o retângulo da janela inteira em coordenadas lógicas (`0, 0, logical_width, logical_height`). `mask_radius = 0`: a janela não tem canto arredondado pintado por nós, e o ramo de caixa por eixo do `box_coverage` corta reto.
- A borda de 1px da janela (`chrome::window_border`, em `Layer::Modal`) continua por cima (RF-18.8).
- `alpha = opacity` da imagem, só. A multiplicação por `[appearance.window] opacity` (RF-18.15) **já acontece** no blit de opacidade da `WindowSurface`, que compõe a cena inteira com esse alfa: fazer a conta de novo no primitivo multiplicaria duas vezes.

### 3. A barra de abas deixa de pintar o fundo com a imagem exibida

O `Quad` opaco de `bar_background` que `chrome::paint` empurra sobre a barra inteira cobriria a imagem. Com a imagem da janela **exibida** (estado `Ready`, `opacity > 0`), ele não é emitido. Sem imagem, ele continua, e o binário é pixel por pixel o de hoje (RF-18.1).

Tirá-lo não muda a cor de nada fora da imagem: o `clear` é a mesma cor. Muda uma coisa, registrada para o aval visual: o `Quad` cobria a parte da sombra do quadro do terminal que vaza para cima (a barra é desenhada depois da grade), e sem ele essa sombra passa a aparecer sobre a imagem, na faixa de baixo da barra. É o que já acontece na barra de status, que não pinta fundo justamente para não cortar essa sombra ([ADR-0048](0048-barra-de-status.md)).

As abas, cápsulas e pílulas não mudam: continuam com os alfas que têm (`.85`, `.92`), e por isso a imagem transparece por elas na medida desses alfas (RF-18.10).

### 4. Dentro do quadro, em janela transparente: furo, imagem recortada, fundo

O PRD pede que a imagem da janela faça, dentro do quadro translúcido, o papel do desktop (RF-18.11). Nos três casos de surface:

- **Terminal opaco** (`background_opacity = 1`). O fundo do quadro é um `RoundedQuad` opaco e cobre a imagem. Nada muda.
- **Surface opaca com terminal translúcido** (a plataforma não deu transparência; `backdrop_punch = false`). O fundo do quadro já é um `RoundedQuad` translúcido em blend normal sobre o que estiver embaixo — que agora é a imagem da janela. Ela transparece sem caso especial.
- **Surface transparente com terminal translúcido** (`backdrop_punch = true`). O `Backdrop` com `REPLACE` apagaria a imagem. Com a imagem da janela exibida, `build_primitives_with_image` troca o fundo do quadro por **três** primitivas, nesta ordem:
  1. `Primitive::Backdrop` com cor **transparente** (`0,0,0,0`), na forma do quadro: o furo, que zera o destino como hoje;
  2. a imagem da janela **recortada pelo quadro**: o mesmo `rect`/`uv`/`repeat` do §2 (a imagem continua posta pela janela, não pelo quadro — é uma imagem só, RF-18.6), com `mask` = o quadro e `mask_radius` = `terminal_frame_corner_radius`, e o mesmo `alpha`;
  3. o fundo do quadro como `RoundedQuad` em **blend normal**, com o alfa `background_opacity` que já tem.

  O pixel final é `fundo·b + imagem·i·(1 − b)`, com alfa `b + i·(1 − b)`: com a imagem a `1.0`, o desktop some de trás do quadro, que é o pedido; com a imagem translúcida, o desktop volta na medida do que falta. Sem imagem da janela, o caminho é o de hoje, byte a byte. A imagem do terminal (ADR-0061 §4) continua logo depois do fundo, acima dele.

- **Célula com fundo próprio** continua `Backdrop` com `REPLACE` e apaga **as duas** imagens embaixo dela (RF-18.12). Fazer a imagem da janela reaparecer embaixo de cada célula pintada exigiria um furo e uma imagem por run de fundo de célula — uma troca de pipeline por run, por frame, pelo único benefício de um detalhe que só existe com janela transparente e programa que pinta fundo. É a mesma regra do RF-17.10 estendida: célula pintada cobre imagem.

A sombra do quadro, empurrada antes do fundo, cai sobre a imagem da janela na margem e no vão como hoje cai sobre o `clear`.

### 5. Geometria: o `placement` que existe, com a janela como quadro

`background_image::placement(mode, frame, image_px, scale)` já aceita qualquer retângulo. A imagem da janela usa `frame` = a janela inteira em lógico. A conta é refeita a cada frame que já ia ser desenhado — resize, maximizar, tela cheia e `ScaleFactorChanged` não recarregam nada (RF-18.7). Os testes de escala do ADR-0061 cobrem a função; entra um teste para o recorte do §4, provando que a imagem recortada por dois quadros vizinhos tem o mesmo `rect`/`uv` (é a mesma imagem, não duas).

### 6. Estado: dois slots, uma textura por arquivo

O estado do ADR-0061 §7 vira **dois slots**, `Terminal` e `Window` (um `enum BackgroundImageSlot`), cada um com chave (caminho, `mtime`, tamanho), estado (`Loading`/`Ready`/`Failed`) e imagem anterior desenhada até a nova chegar, exatamente como hoje.

- **Mesma chave nos dois slots, uma textura** (RF-18.21): o slot que pede uma chave já carregada ou em carga pelo outro não abre thread nova; aponta para o mesmo `ImageId`. A textura é removida do registro quando **nenhum** slot a usa mais.
- `Wakeup::BackgroundImageLoaded` leva a **chave** do resultado, e é pela chave que a chegada é casada com os slots que a esperam — não por slot. Um resultado cuja chave nenhum slot quer mais é descartado (a corrida de duas recargas some por construção, como antes). A variante continua em `Box`, com o teste de tamanho do `Wakeup`.
- A carga do arranque é adiada ao primeiro byte do PTY (`deferred_background_load`), agora para as chaves dos dois slots — a lição da verificação do PRD-017 vale igual para a imagem da janela, que pode ser tão grande quanto.
- `resolve_background_image_path` não muda: é pura e não sabe de qual chave o texto veio.

### 7. Erros e avisos por slot

`BackgroundImageError` não muda. O aviso ganha o slot: o título e as frases dizem "imagem de fundo **da janela**" ou "**do terminal**" (RF-18.17), por chaves próprias em `locales/` (`notice.window_background_image.*`, ao lado de `notice.background_image.*`), nunca por concatenação ([ADR-0056](0056-catalogo-de-textos-da-interface.md)). "Uma vez por arquivo e por problema" passa a ser por slot: o mesmo arquivo ausente nas duas chaves dá dois avisos, um de cada, porque são duas configurações erradas.

### 8. Config

`[appearance.window.background_image]`, com `path`, `mode` e `opacity` e os padrões `""`, `"stretch"` e `1.0`. O tipo é o mesmo `BackgroundImage` de `porecatu-config::terminal::background_image`, reutilizado — não copiado — como campo de `appearance::window::Window`. O bloco comentado entra no `porecatu.example.toml` **junto do campo**, porque o `tests/example_toml.rs` reprova chave sem campo.

O token `[appearance.window] background` (`#15181d`), que o arquivo declara e o binário nunca leu, **continua sem uso**: o fundo atrás da imagem é o `bar_background`, que é o que o binário desenha ([ADR-0028](0028-o-binario-como-referencia-visual.md)).

### 9. Recarga a quente: classe A

As três chaves são classe A do [ADR-0030](0030-escopo-do-hot-reload.md), com o mesmo comportamento das do terminal: `mode` e `opacity` no frame seguinte, `path` pela carga do §6. Revisão por blockquote no ADR-0030.

### 10. Tela de configurações

Três opções no grupo **Aparência**, logo depois de `appearance.window.opacity`, com os controles do [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md) §3 que a imagem do terminal já usa: campo de texto, escolha em três botões colados, campo numérico de `0.0` a `1.0`. A nota de arquivo não encontrado é a mesma função (`background_image::missing_file`). A janela de configurações **não** recebe a imagem (RF-18.13): o fundo dela é a constante `PANEL_BACKGROUND`, pintada pelo próprio `SettingsWindow`, que não lê este estado.

### 11. Aparência

Nenhuma cor, dimensão, raio ou espaçamento novo. Os únicos valores novos são os padrões das três chaves. Com `path` vazio, o binário é pixel por pixel o de hoje. As §2.2, §2.7 e §2.8 da especificação são reescritas no PR que muda o binário ([ADR-0028](0028-o-binario-como-referencia-visual.md)), com a entrada na §4.4; aqui só a tabela de fases ganha a classificação.

## Alternativas consideradas

### Uma sexta camada abaixo de `Grid`

Explícita, e com o nome certo. Recusada: uma camada inteira para uma primitiva, mais um índice em `Layer::ORDER` que todo `match` de camada passa a ter, quando a ordem da lista dentro de `Grid` já resolve. Se aparecer uma segunda coisa "atrás de tudo", ela vira camada.

### Imagem em cada pedaço do chrome (barra de abas, barra de status, margem) separadamente

O que "imagem de fundo na barra de abas" sugeria no PRD-004. Recusada: a imagem sairia recomeçando em cada faixa, e o vão entre painéis ficaria de fora ou exigiria uma terceira lógica. Uma imagem na janela inteira, com o resto por cima, é uma conta só.

### Imagem só fora dos quadros

Recortada para nunca aparecer atrás do terminal. Recusada pelo dono do produto: ele quer a imagem atrás de tudo, aparecendo através do terminal translúcido. Também exigiria recortar a imagem pelo **negativo** dos quadros, o que nenhuma primitiva faz.

### Trocar o `REPLACE` do fundo do quadro por blend normal quando há imagem

Uma linha. Recusada: blend normal sobre o `clear` opaco tornaria o quadro opaco em janela transparente — o desktop sumiria de trás do terminal mesmo com a imagem a `opacity < 1`, e mesmo onde um PNG é transparente.

### Furo e imagem também embaixo de cada célula com fundo próprio

A imagem da janela continuaria aparecendo através de células pintadas em janela transparente. Recusada pelo custo (§4): uma troca de pipeline por run de fundo de célula, por frame, para um caso estreito, contra a regra do RF-17.10 que o usuário já tem.

### Multiplicar `opacity × [appearance.window] opacity` no primitivo

Recusada: o blit de opacidade já multiplica a cena inteira. Seria `opacity × window_opacity²`.

### Dar uso ao token `[appearance.window] background` como fundo atrás da imagem

Mudaria a cor da moldura de todo usuário que tem a chave no arquivo — e o arquivo de exemplo a tem —, contra "o binário é a referência". Fora deste ADR.

### Um slot por imagem com textura própria sempre, mesmo arquivo duas vezes

Mais simples de escrever, e recusada pela métrica do PRD: o mesmo JPEG grande nas duas chaves custaria duas decodificações e duas texturas.

## Consequências

### Positivas

- Zero dependência, zero primitiva, zero shader novo: o ADR-0061 pagou a infraestrutura, e esta é a primeira reutilização dela.
- Com `path` vazio, nada muda — nem o `Quad` da barra, nem o fundo do quadro em janela transparente.
- A transparência da janela continua significando o que significa, sem conta nova: o blit já a aplica.
- O mesmo arquivo nas duas chaves custa uma textura.

### Negativas

- **O fundo do quadro em janela transparente vira três primitivas** quando há imagem de janela, uma delas trocando de pipeline: uma troca a mais por painel por frame, além da do ADR-0061. Só em frame que já ia ser desenhado.
- **A sombra do quadro passa a aparecer na faixa de baixo da barra de abas** com a imagem exibida (§3). Item do aval visual.
- **Célula pintada em janela transparente mostra o desktop, não a imagem da janela** (§4). Coerente com a regra de célula pintada, mas é o único lugar da janela em que a imagem não faz o papel do desktop.
- **O estado de carga fica mais complexo**: dois slots, chave compartilhada, liberação por contagem de uso. É o preço da textura única por arquivo.
- **Duas imagens, dois avisos** para o mesmo arquivo ausente nas duas chaves.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Imagem de janela forte tornar as abas e a barra de status ilegíveis | Média | Médio | `opacity` própria; abas e pílulas mantêm o fundo translúcido; documentado no guia |
| Furo do §4 vazar a imagem fora do raio do quadro | Baixa | Médio | Mesma SDF, mesmo raio, mesmo retângulo para o furo, a imagem recortada e o fundo; verificação de pixel no canto |
| A imagem recortada pelo quadro desalinhar da imagem fora dele (costura na borda) | Média | Médio | O mesmo `placement` da janela para as duas; teste de `rect`/`uv` iguais; verificação de pixel atravessando a borda |
| Textura compartilhada removida enquanto o outro slot ainda a usa | Média | Alto | Liberação só quando nenhum slot referencia a `ImageId`; teste do store com as duas chaves iguais e troca de uma delas |
| Duplicar a multiplicação pela opacidade da janela | Baixa | Baixo | `alpha` do primitivo é só `opacity` (§2); medição de pixel com `[appearance.window] opacity < 1` |

## Registro do aval visual

> **Pendente — pedido em 2026-10-06.** Pedido na etapa de pintura do [roadmap](../roadmap.md), sobre a build. Itens a avaliar: a imagem atrás da barra de abas sem o fundo dela; a sombra do quadro na faixa de baixo da barra (§3); a imagem através do quadro translúcido; as duas imagens juntas.

### Defeito conhecido, aberto: desktop nos cantos do quadro (pré-existente)

Achado na medição de pixel da etapa de composição, e confirmado pelo dono do produto a olho: em janela transparente, o canto arredondado do quadro do terminal mostra a cor de trás da janela. A causa é anterior a este ADR. O `Primitive::Backdrop` usa `BlendState::REPLACE` e escreve o retângulo inteiro do quadro, inclusive os pixels de cobertura zero fora do raio; o destino ali vira transparente. Com a imagem da janela exibida, o furo do §4 herda o mesmo defeito, mas a imagem recortada **não** vaza para fora do raio. O controle sem imagem da janela, com o mesmo `background_opacity = 0.6`, mostra o mesmo pixel (medido em (6,52) numa janela de 800 px de largura).

**Dívida de verificação e de correção, decisão do dono do produto de 2026-10-06: anotar e corrigir depois.** A correção, a verificar, é descartar o fragmento de cobertura zero no pipeline `REPLACE` do `quad.wgsl`, o que mexe no app inteiro e pede aval próprio.
