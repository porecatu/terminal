# ADR-0061 — Imagem de fundo do terminal: `image` para decodificar, primitiva de imagem no render, alfa multiplicado

**Status:** Aceito
**Data:** 2026-10-05
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0010](0010-licenciamento.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0015](0015-multiplas-janelas.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md), [ADR-0053](0053-paineis-divididos.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md), [PRD-005](../prd/prd-005-aparencia-do-terminal.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md), [PRD-017](../prd/prd-017-imagem-de-fundo-do-terminal.md)

## Contexto

O [PRD-017](../prd/prd-017-imagem-de-fundo-do-terminal.md) pede uma imagem PNG ou JPEG atrás de todo terminal, em três modos (`stretch`, `tile`, `center`), com opacidade própria que se multiplica pela do terminal. O requisito diz **o quê**. Seis coisas ele não decide, e nenhuma é detalhe de implementação:

1. **Quem decodifica.** O projeto não decodifica imagem nenhuma além do ícone do app, que é um PNG embutido, conhecido e pequeno, lido pelo crate `png` em `porecatu-ui` (`app_icon.rs`). JPEG não tem decodificador no workspace. O `Cargo.lock` já tem o crate `image 0.25.10`, mas só como dependência transitiva do `arboard`, e sem a feature de JPEG.
2. **Como uma textura chega à tela.** `porecatu-render` não tem primitiva de imagem. Os únicos `wgpu::Texture` do projeto são o atlas de glyphs do `glyphon` e a textura offscreen do blit de opacidade de janela (`blit.rs`, `WindowSurface`). As primitivas são quad, retângulo arredondado, `Backdrop`, run de texto e clip ([ADR-0018](0018-composicao-de-frame.md)), e nenhuma amostra textura.
3. **Onde a imagem entra na ordem de pintura**, e quem a recorta pelo raio do quadro. `PushClip` é `set_scissor_rect`, que só recorta retângulo. O canto arredondado do quadro é SDF no shader de quads.
4. **Como ela compõe com a transparência que já existe.** `[terminal] background_opacity` é pintado por `paint::backdrop_fill`: numa janela transparente, o fundo do quadro é um `Primitive::Backdrop` que **substitui** o destino (`BlendState::REPLACE`) para o desktop aparecer. Numa janela opaca, é um `RoundedQuad` translúcido sobre a cor da barra. A imagem precisa caber nos dois caminhos sem desfazer nenhum.
5. **Quando ela é lida.** Um JPEG de câmera tem 24 megapixels, e decodificá-lo na main thread trava a interface pelo tempo da decodificação. Isso é exatamente o que a regra do [ADR-0007](0007-modelo-de-threading.md) proíbe para o PTY, e vale igual aqui.
6. **Em que espaço de cor.** O projeto não faz gestão de cor: hex do design usado cru, surface sem sufixo `Srgb`, `glyphon` em `ColorMode::Web`. Uma textura criada com o formato `Srgb` "correto" repetiria a dupla conversão que já custou duas investigações (CLAUDE.md, F1 e "texto esmaecido invisível").

E é mudança das §1/2 da [especificação visual](../design/especificacao-visual.md), §2.7: o [ADR-0032](0032-interface-do-v1-fechada.md) exige ADR para isso, e este é ele.

## Decisão

### 1. `image`, só com PNG e JPEG

`porecatu-ui` passa a depender **diretamente** do crate `image`, com `default-features = false` e `features = ["png", "jpeg"]`, na versão pinada que o `Cargo.lock` já resolve (`=0.25.10`), subida como tarefa própria (mesma disciplina de `wgpu` e `toml_edit`).

- **Licença:** `image` é MIT OR Apache-2.0. O decodificador de JPEG que a feature traz, `zune-jpeg`, é MIT OR Apache-2.0 OR Zlib. O de PNG, `png`, já está no projeto. Todos compatíveis com GPLv3 ([ADR-0010](0010-licenciamento.md)).
- **Formato pelo conteúdo** (RF-17.2): `ImageReader::with_guessed_format`, nunca a extensão.
- **Limites contra arquivo malformado** (RF-17.15): os `image::Limits` padrão do crate, sem número próprio. Estourar o limite é o erro `TooLarge` do §8.
- O `png` direto de `porecatu-ui` continua, para o ícone. Unificá-lo no `image` é limpeza possível, não parte desta decisão.

Nenhum tipo do `image` atravessa para `porecatu-render`. O que atravessa são bytes RGBA8 e dimensões (§2).

### 2. Uma primitiva de imagem sem domínio em `porecatu-render`

`porecatu-render` ganha dois conceitos, os dois sem saber o que é terminal:

- **Registro de imagens no `GpuContext`** (do processo, [ADR-0015](0015-multiplas-janelas.md)): `create_image(width, height, levels: &[&[u8]]) -> ImageId` e `remove_image(ImageId)`. Uma `ImageId` é um identificador opaco; o registro é dono do `wgpu::Texture`, da view e do bind group. Toda janela desenha com a mesma textura (RF-17.18).
- **`Primitive::Image`**:

  ```rust
  Image {
      rect: Rect,         // onde a textura é desenhada (lógico)
      uv: Rect,           // que parte da textura cobre `rect`; pode passar de 0..1
      repeat: bool,       // Repeat ou ClampToEdge no sampler
      mask: Rect,         // forma que recorta o desenho...
      mask_radius: f32,   // ...com este raio (SDF), como o RoundedQuad
      alpha: f32,         // multiplica a cobertura inteira
      image: ImageId,
  }
  ```

  `rect` e `mask` são separados de propósito. No `center` com a imagem maior que o quadro num eixo e menor no outro, o retângulo desenhado sai do quadro por um lado e não chega aos cantos pelo outro, e o recorte arredondado tem de ser o do **quadro**, não o da imagem.

Quem traduz `mode` em `rect`/`uv`/`repeat` é `porecatu-ui` (§6), numa função pura. A regra da tabela de crates continua de pé: `porecatu-render` recebe primitivas, não conceitos.

### 3. Pipeline próprio, premultiplicado, sem curva de cor

Um shader novo, `image.wgsl`, com pipeline próprio em `porecatu-render`:

- **Formato da textura `Rgba8Unorm`, nunca `Rgba8UnormSrgb`.** Os bytes do arquivo vão crus para a GPU e saem crus na surface, como as cores do design. É a mesma decisão do `remove_srgb_suffix()` da surface e do `ColorMode::Web` do `glyphon`, um nível acima. A filtragem linear acontece então sobre valores com curva, o que mistura tons um pouco mais escuro que o "correto"; é aceito pela mesma razão da surface.
- **Saída premultiplicada** (`rgb * a, a`, com `a = texel.a * alpha * cobertura_da_máscara`) e `BlendState::PREMULTIPLIED_ALPHA_BLENDING`. É o par que o `quad.rs` levou anos para acertar (CLAUDE.md, "Blend mode do pipeline de quad"), e já nasce certo aqui. O alfa do PNG é tratado como reto no arquivo e premultiplicado no shader.
- **Máscara por SDF** de retângulo arredondado, com a **mesma** fórmula e a mesma cobertura do `quad.wgsl`. Ela é extraída para uma função WGSL compartilhada, não copiada, porque fórmula de geometria copiada diverge (CLAUDE.md, F3). Com `mask_radius = 0` vale o ramo de caixa por eixo do `box_coverage`, pela mesma razão do canto de bloco.
- **Sampler linear com mipmaps.** Dois samplers, um `ClampToEdge` e um `Repeat`, escolhidos por `repeat`. A cadeia de mips é gerada **na thread de carga** (§7), por redução sucessiva na CPU. Sem ela, um JPEG de 6000 px esticado num painel de 600 px cintila e serrilha, e nenhum dos três modos é imune: o `stretch` reduz sempre que o quadro é menor que a imagem.

### 4. Na camada da grade, entre o fundo do quadro e o fundo das células

A imagem entra em `paint::build_primitives`, uma vez por painel, **logo depois** do `backdrop_fill` do fundo do quadro e **antes** de `paint_row_backgrounds`, em `Layer::Grid`. Fica acima do fundo e abaixo de célula, texto, seleção, realce de busca, affordance de hyperlink e cursor (RF-17.9). A barra de busca e os widgets estão em camadas acima e não mudam.

Em `resolve_layer` a imagem é **geometria**, na ordem da lista, como o `Backdrop`. Ela quebra o batch de quads em volta, porque troca de pipeline e de bind group, e por isso custa um `draw` a mais por painel por frame. Ela não vai para um balde à parte. A regra de geometria antes de texto dentro da camada continua, e é o que deixa o texto por cima.

O retângulo é o **quadro inteiro** do painel (`terminal_box_rect` numa aba sem divisão, o retângulo de `panes::layout` numa aba dividida), padding incluído (RF-17.8). A máscara é esse mesmo retângulo com `terminal_frame_corner_radius`. O vão entre painéis não é de painel nenhum, e não recebe nada.

> **Revisto pelo [ADR-0062](0062-imagem-de-fundo-da-janela.md) §2 e §4.** A imagem do terminal continua neste lugar, e o vão continua sem ela. Mas `Layer::Grid` passa a abrir com a imagem **da janela**, abaixo de todo quadro, e, em janela transparente com essa imagem exibida, o fundo do quadro vira furo transparente, imagem da janela recortada pelo quadro e fundo em blend normal — esta imagem entra logo depois, como antes.

### 5. Alfa multiplicado, e a composição com o `Backdrop`

`alpha = image.opacity × terminal.background_opacity` (RF-17.12), calculado em `porecatu-ui` junto da paleta resolvida (`ResolvedTermPalette`), não no shader.

- **Janela transparente.** O `Backdrop` do quadro escreve o fundo com alfa `b = background_opacity`, substituindo o destino. A imagem compõe por cima em premultiplicado, com alfa `i = opacity × b`. O pixel final tem alfa `b + i·(1 − b)`, que é menor que 1 sempre que `b < 1`, qualquer que seja `opacity`. Com `opacity = 1` e `b = 0.5`, fica `0.75`: o desktop continua aparecendo. **O terminal transparente nunca fica opaco por causa da imagem**, que é o que o "obedecer à transparência" do pedido quer dizer.
- **Célula com fundo próprio** numa janela transparente também é `Backdrop`, e também substitui o destino. Ela apaga a imagem embaixo dela e escreve o próprio fundo com o mesmo alfa `b`. É o RF-17.10 por construção, sem caso especial.
- **Janela opaca.** O fundo do quadro já é um `RoundedQuad` translúcido sobre a cor da barra, e a imagem compõe por cima com o mesmo `i`. Nada muda no caminho de hoje.
- **A imagem não decide a transparência da janela.** `wants_transparent` continua olhando só `background_opacity` e `[appearance.window] opacity` (RF-17.13). Imagem com alfa num PNG, numa janela opaca, mostra o fundo do quadro atrás, não o desktop.

### 6. Geometria dos modos, uma função pura

`porecatu-ui` ganha `background_image::placement(mode, frame: Rect, image_px: (u32, u32), scale: f32) -> Placement { rect, uv, repeat }`, sem GPU e testada à parte. Tamanho natural, em lógico, é `image_px / scale`: um pixel da imagem para um pixel físico (RF-17.6).

| Modo | `rect` | `uv` | `repeat` |
|---|---|---|---|
| `stretch` | `frame` | `(0,0)–(1,1)` | não |
| `tile` | `frame` | `(0,0)–(frame.w / natural.w, frame.h / natural.h)` | sim |
| `center` | `natural`, centrado em `frame` e intersectado com ele | a fração da imagem que cai dentro de `frame` | não |

A origem do `center` e do `tile` é arredondada ao pixel físico, para que cada texel caia num pixel e a imagem saia nítida em tamanho natural. As bordas de `rect` saem do mesmo arredondamento dos quads (lição da costura de blocos, CLAUDE.md). O zoom de fonte não entra na conta. Mudança de `scale_factor` só refaz a conta, não a carga.

### 7. Carga fora da main thread, como dado chaveado

O estado vive em `App`, **do processo**, nunca da janela (o molde de `App.git_remotes`, [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md)):

- **Chave:** caminho resolvido, `mtime` e tamanho do arquivo.
- **Estado:** `Loading`, `Ready { id, size }` ou `Failed(BackgroundImageError)`.
- A cada aplicação de config (arranque e cada recarga), a main thread resolve o caminho e faz um `metadata` (barato, a mesma ordem de custo do `stat` do `.git/HEAD` do [ADR-0049](0049-branch-git-na-barra-de-status.md)). Chave igual à atual: nada acontece, e é o caso de mudar só `mode` ou `opacity`. Chave nova: abre uma **thread de vida curta e detached** (molde de `reload::watch` e das consultas do Git), que lê o arquivo, decodifica, reduz até `max_texture_dimension_2d` do `Device` se preciso (RF-17.15; é 8192 com o `DeviceDescriptor::default()` que o projeto pede), gera os mips e devolve `Wakeup::BackgroundImageLoaded` pela `EventLoopProxy`.
- Na chegada, se a chave do resultado não é mais a atual, o resultado é descartado. É isso que faz a corrida de "duas recargas seguidas" sumir por construção. Se é a atual, a main thread cria a textura, solta a anterior e pede redraw a todas as janelas.
- **Até a nova ficar pronta, a anterior continua sendo desenhada**, sem piscar sem imagem no meio de uma troca. Falha remove a anterior e desenha sem imagem.
- O arranque não espera (RF-17.17). A primeira janela abre com o estado em `Loading`, e a imagem aparece no frame em que chega. Nenhum frame é pedido enquanto nada chega (RF-17.19, [ADR-0007](0007-modelo-de-threading.md)).
- `Wakeup` leva o resultado em `Box` se ele passar dos 80 bytes que a maior variante já reserva, com a mesma medição feita para `GitQueryResult`.

O arquivo de imagem **não é vigiado** (RF-17.16). O `metadata` na recarga de config é o que pega uma imagem trocada no disco.

> **Revisto pelo [ADR-0062](0062-imagem-de-fundo-da-janela.md) §6.** O estado passa a ter dois slots, `Terminal` e `Window`, cada um com chave, estado e imagem anterior como descrito aqui. O mesmo arquivo nos dois é uma carga e uma textura, liberada quando nenhum slot a usa; `Wakeup::BackgroundImageLoaded` leva a chave, e a chegada é casada pelos slots que a esperam. A carga do arranque adiada ao primeiro byte do PTY vale para os dois.

### 8. Caminho e erros tipados

- **Resolução do caminho** em `porecatu-config`, função pura `resolve_background_image_path(config_path: Option<&Path>, raw: &str) -> Option<PathBuf>`. Vazio dá `None`. `~/` ou `~\` no início vira `dirs::home_dir()`. Absoluto fica como está. Relativo junta com `config_path.parent()` (RF-17.3), o mesmo diretório de que já derivam `locales/` e `sessions/`. É a **primeira** chave do config com caminho relativo ao arquivo. `startup_directory` e `trusted_paths` não mudam de regra, e a referência do arquivo de exemplo diz isso em prosa.
- **Erros** como enum em `porecatu-ui`, `BackgroundImageError { NotFound, Unreadable(io::ErrorKind), UnsupportedFormat, Malformed, TooLarge }`, com o caminho resolvido ao lado. A frase sai do registro de mensagens e de `locales/` ([ADR-0056](0056-catalogo-de-textos-da-interface.md)), nunca do `Display` do `image`.
- **Aviso** da barra de avisos ([ADR-0014](0014-superficie-de-aviso-e-dialogo.md)), severidade aviso, emitido **na transição** para `Failed` de uma chave. Por isso aparece uma vez por arquivo e por problema, não por janela nem por recarga que repete a mesma chave (RF-17.14). No arranque vai pela mesma porta dos avisos de config na primeira janela.

### 9. Recarga a quente: classe A

`[terminal.background_image] path`, `mode` e `opacity` são **classe A** do [ADR-0030](0030-escopo-do-hot-reload.md): sem PTY, sem recálculo de grade, sem recriar janela. `path` aciona a carga do §7 e o frame seguinte usa o que houver. Revisão registrada por blockquote no ADR-0030.

### 10. Tela de configurações

Três opções no grupo **Terminal**, subgrupo do fundo, junto de `background_opacity` (`settings/catalog.rs`), com os controles que o [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md) §3 já tem: **campo de texto** (caminho), **escolha** (modo) e **campo numérico** de `0.0` a `1.0`, no passo e na precisão de `background_opacity` (opacidade). A nota de "arquivo não encontrado" do RF-17.21 vem de um `Path::exists` sobre o caminho resolvido a cada edição do campo. Ocupa o lugar e o estilo do aviso de `trusted_paths` (RF-16.27), não o da razão de recusa: não impede o Salvar. A emenda vai no catálogo do RF-16.11 do [PRD-016](../prd/prd-016-tela-de-configuracoes.md).

### 11. Aparência

Nenhuma cor, dimensão, raio ou espaçamento novo. O recorte usa `terminal_frame_corner_radius`, o retângulo é o quadro que já existe, e os únicos valores novos são os padrões das três chaves (`path = ""`, `mode = "stretch"`, `opacity = 1.0`), que entram no `porecatu.example.toml` **junto do código**. O `tests/example_toml.rs` reprova chave do arquivo de exemplo sem campo em `Config`. Com `path` vazio, o binário é **pixel por pixel** o de hoje. A §2.7 da especificação é reescrita no PR que muda o binário ([ADR-0028](0028-o-binario-como-referencia-visual.md)), com a entrada na §4.4. Aqui só a tabela de fases ganha a classificação.

## Alternativas consideradas

### Uma imagem só pela aba, com cada painel mostrando o pedaço dele

Lê como "uma janela com papel de parede", e é o que alguns multiplexadores fazem. Recusada pelo dono do produto: cada painel é um terminal inteiro ([ADR-0053](0053-paineis-divididos.md)), e o pedido é a imagem "ao fundo de cada terminal". Ela também faria o vão entre painéis ou mostrar imagem (contra o "o divisor é ausência de pixel") ou cortá-la em pedaços desalinhados.

### Alfas independentes

`opacity` pintando a imagem por cima, sem multiplicar por `background_opacity`. Mais simples de explicar, e recusada: imagem a `1.0` sobre um terminal a `0.5` taparia o desktop que o usuário configurou para ver, e a transparência passaria a depender de o arquivo de imagem existir. Multiplicar mantém as duas chaves com o significado que cada uma já tinha.

### `png` mais um decodificador de JPEG à mão (`zune-jpeg` ou `jpeg-decoder` direto)

Duas dependências diretas e dois caminhos de código para a mesma operação, mais o reconhecimento de formato escrito por nós. O `image` já está no lock, já faz as duas coisas e o reconhecimento, e com `default-features = false` não traz decodificador além dos dois pedidos.

### `image` com as features padrão

Traz uma dúzia de formatos (GIF, WebP, TIFF, AVIF, BMP, ICO…), mais código para compilar e auditar e mais superfície de arquivo malformado. Isso por formatos que o PRD deixa fora de escopo. Acrescentar um depois é uma linha de `features`.

### Decodificar na main thread

Uma linha a menos de código. Recusada pelo [ADR-0007](0007-modelo-de-threading.md) e pela métrica do PRD-017: um JPEG grande tomaria centenas de milissegundos do arranque e de cada recarga.

### Primitiva `Image` só com `rect`, recorte por `PushClip`

`PushClip` é `set_scissor_rect`: recorta retângulo. Os cantos arredondados do quadro sumiriam sob a imagem no `stretch` e no `tile`, que são os modos que chegam aos cantos. Daí a máscara por SDF dentro da própria primitiva.

### Textura `Rgba8UnormSrgb`

"Correta" em gestão de cor, e faria o shader decodificar a curva que a surface sem sufixo `Srgb` nunca recodifica: a imagem sairia escura, a terceira encarnação da dupla conversão do projeto.

### Seletor de arquivo do sistema na tela de configurações

Recusado pelo [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) (nenhum diálogo nativo) e pelo RF-16.12 (caminho é texto), e traria um crate com toolkit no Linux.

### Um quarto modo `fill` (preencher mantendo a proporção)

Decisão do dono do produto: três modos, `stretch` distorcendo. O `placement` do §6 é onde ele entraria, como uma linha a mais na tabela.

## Consequências

### Positivas

- O recurso vale em toda aba, grupo e painel sem configuração por terminal, porque nasce dentro do único lugar que pinta quadro (`build_primitives`, uma vez por painel).
- Com `path` vazio, que é o padrão, não muda um pixel nem um byte de memória de GPU.
- A transparência existente continua significando o que significa. A imagem é uma camada a mais entre duas que já existiam, sem caso especial para célula com fundo próprio.
- `porecatu-render` ganha uma primitiva genérica de imagem sem aprender domínio. Ela serve a qualquer imagem futura do chrome.
- Uma decodificação e uma textura por processo, qualquer que seja o número de janelas e de painéis.
- Nenhum `unsafe`. `image` e `zune-jpeg` são Rust seguro na API usada.

### Negativas

- **Primeira dependência de decodificação de imagem**, e a superfície de ataque que vem com ela: o arquivo é do usuário, mas pode ter vindo de qualquer lugar. Mitigada pelos `Limits`, pelos dois formatos só e pela thread separada (um pânico de decodificador derruba a thread, não o app).
- **Primeira chave com caminho relativo ao arquivo de config**, quando `startup_directory` e `trusted_paths` não são. Duas regras de caminho no mesmo arquivo. A prosa do exemplo precisa dizer qual vale onde.
- **Um `draw` a mais por painel por frame**, com troca de pipeline e de bind group. Só em frame que já ia ser desenhado: não acorda o loop.
- **Mips na CPU** custam tempo na thread de carga e um terço a mais de memória de GPU. É o preço de não serrilhar.
- **A imagem trocada no disco só aparece na próxima recarga de config.** Quem edita a imagem e espera vê-la mudar sozinha vai achar que não funcionou.
- **Filtragem sobre valores com curva** (§3) escurece levemente as transições de uma imagem reduzida, frente a um app com gestão de cor. Coerente com o resto do app, que também não tem.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Imagem forte atrás do texto torna o terminal ilegível | Média | Médio | `opacity` própria, documentada no exemplo e no guia como o controle de legibilidade; padrão `path` vazio |
| Arquivo malformado esgotar memória ou travar a decodificação | Baixa | Alto | `image::Limits` padrão; decodificação em thread detached; falha vira `Failed` com aviso |
| Duas recargas seguidas desenharem a imagem da primeira | Média | Baixo | Resultado chaveado por caminho, `mtime` e tamanho, descartado se a chave mudou (§7) |
| Blend errado escurecer a borda da imagem ou somar alfa em dobro | Média | Médio | Premultiplicado desde o primeiro commit, com o mesmo par do `quad.rs`; verificação de pixel numa janela transparente e numa opaca |
| Cantos arredondados vazarem a imagem ou serrilharem | Baixa | Médio | Mesma função SDF e mesma cobertura do `quad.wgsl`, compartilhada, não copiada |
| Costura ou meio texel borrado no `tile` e no `center` | Média | Baixo | Origem arredondada ao pixel físico; teste de `placement` em `scale` 1.0, 1.25, 1.5 e 2.0, como o de `cell_at` |
| Imagem maior que o limite do `Device` falhar na criação da textura | Baixa | Médio | Redução na thread de carga até `max_texture_dimension_2d`, lido do `Device` em uso |

## Registro do aval visual

> **Pendente — dívida de verificação (2026-10-05).** A pintura por painel (etapa 4) foi implementada e medida ao vivo: o alfa efetivo bate com `opacity × background_opacity` em janela opaca e transparente, e célula com fundo próprio cobre a imagem. O dono do produto **ainda não viu a aparência** e fará a verificação visual depois, em vez de dar o aval na hora. Até lá, os três modos, o recorte pelo raio do quadro e a opacidade estão **medidos, não aprovados**. Ajuste de aparência pedido na verificação entra aqui e na §4.4 da especificação visual.
