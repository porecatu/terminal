# ADR-0068 — Efeito acrílico no fundo do terminal: desfoque da camada da janela em passe próprio, amostrado por quadro

**Status:** Aceito — aval visual pendente
**Data:** 2026-10-22
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md), [ADR-0061](0061-imagem-de-fundo-do-terminal.md), [ADR-0062](0062-imagem-de-fundo-da-janela.md), [ADR-0063](0063-imagem-da-janela-sem-furo-ate-o-desktop.md), [ADR-0067](0067-temas-em-arquivos-proprios.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md), [PRD-017](../prd/prd-017-imagem-de-fundo-do-terminal.md), [PRD-018](../prd/prd-018-imagem-de-fundo-da-janela.md), [PRD-019](../prd/prd-019-efeito-acrilico-no-fundo-do-terminal.md)

## Contexto

O [PRD-019](../prd/prd-019-efeito-acrilico-no-fundo-do-terminal.md) pede que a imagem de fundo da janela apareça **desfocada** através do quadro do terminal, com raio configurável, e nítida fora dele. Quatro coisas do binário de hoje estão no caminho:

1. **`porecatu-render` não tem passo de desfoque, e isso foi escrito como definitivo.** As sombras são três `RoundedQuad` empilhados e o hover é `chrome::brighten` em CPU (CLAUDE.md, "Descobertas na F2"; §1.2 e §4.4 da [especificação visual](../design/especificacao-visual.md)); o PRD-017 e o PRD-018 puseram "desfoque" no fora de escopo pela mesma razão. Não há como turvar o que passa por trás de um quadro sem amostrar o que já foi desenhado, e nenhuma primitiva faz isso.
2. **O que está atrás do quadro é conhecido e pouco.** Com a imagem da janela configurada, o fundo do quadro não fura até o desktop ([ADR-0063](0063-imagem-da-janela-sem-furo-ate-o-desktop.md) §1): embaixo dele há o `clear` em `bar_background` e a imagem da janela, a primeira primitiva de `Layer::Grid` ([ADR-0062](0062-imagem-de-fundo-da-janela.md) §2). A sombra do próprio quadro cai fora dele.
3. **A opacidade da janela traz o desktop de volta.** Com `[appearance.window] opacity < 1`, a `WindowSurface` desenha a cena num `Offscreen` e a compõe com esse alfa no blit (`blit.rs`). O desktop aparece então atrás de **tudo**, inclusive de um acrílico desenhado pelo app, e aparece nítido: o app não tem os pixels do desktop para desfocar.
4. **O render é damage-driven** ([ADR-0007](0007-modelo-de-threading.md)). Um desfoque refeito a cada frame custaria o mesmo em todo frame de saída do PTY, que é exatamente o caminho que precisa ser barato.

E é mudança da §2.7 da especificação: o [ADR-0032](0032-interface-do-v1-fechada.md) exige ADR, e este é ele.

## Decisão

### 1. Um passe de desfoque em `porecatu-render`, só para isto

O "sem passo de filtro, definitivo" é revogado **para o acrílico e só para ele**. `porecatu-render` ganha um passe de desfoque gaussiano; as sombras continuam em camadas e o hover continua em CPU — reaproveitar o passe para eles seria mudança de aparência sem pedido, contra o ADR-0032. Sem dependência nova: WGSL próprio sobre o `wgpu` já pinado.

### 2. A fonte do desfoque é uma lista de primitivas, e o render não sabe de onde ela veio

A `WindowSurface` ganha uma **fonte de desfoque**: cor de fundo, lista de primitivas, raio em pixels lógicos e uma chave de cache, tudo dado por `porecatu-ui`. Ela desenha essa lista sobre a cor de fundo numa textura offscreen do tamanho da surface, desfoca e guarda o resultado. `porecatu-ui` passa a imagem da janela já posta pelo `placement` (o mesmo `Primitive::Image` da cabeça de `Layer::Grid`) e `bar_background`. O render não aprende o que é "janela" nem "acrílico", como não aprendeu o que é "imagem de fundo" ([ADR-0062](0062-imagem-de-fundo-da-janela.md) §1).

A fonte é **só** a camada da janela: nem abas, nem barra de status, nem sombras, nem outros quadros entram nela. É o que torna o resultado estável — ele não muda quando uma aba muda de título — e é o que o PRD pede (RF-19.2).

### 3. Algoritmo: gaussiana separável sobre cópia reduzida

- Dois passes, horizontal e vertical, com `sigma = raio_físico / 3`, raio físico = `blur × scale_factor` (RF-19.9).
- Sobre uma cópia **reduzida** da fonte, por fator 2 ou 4, o menor que mantém o passe em até 32 amostras. O resultado é ampliado por amostragem bilinear na hora de usar. Com raio grande, a redução não se vê; com raio pequeno, o fator é 2.
- Bordas da janela por *clamp*: a imagem não escurece rumo à borda.
- Texturas `Rgba8Unorm`, **nunca** `Srgb`, e conteúdo premultiplicado — a mesma disciplina do [ADR-0061](0061-imagem-de-fundo-do-terminal.md), pelo mesmo motivo (a dupla conversão de sempre).

### 4. Refeito só quando a entrada muda

O resultado fica guardado na `WindowSurface` e só é refeito quando a **chave** muda: tamanho físico, escala, o `ImageId` e o estado da imagem da janela, `mode` e `opacity` dela, `blur` e `bar_background`. Saída do terminal, cursor piscando, hover e abas não entram na chave. Desligado o acrílico, a textura é descartada. Assim o frame comum custa **uma amostragem** por quadro, e o terminal ocioso continua sem frame (RF-19.16). O preço fica no redimensionamento: cada tamanho novo refaz o desfoque uma vez.

### 5. Primitiva `BlurredBackdrop`, amostrada por coordenada de tela

`Primitive::BlurredBackdrop { mask, mask_radius }` desenha, dentro da máscara, a textura desfocada da própria janela, amostrada pela **coordenada de tela** do fragmento. A máscara usa a SDF compartilhada do `quad.wgsl`, como o `image.wgsl` já faz. Por amostrar em coordenada de tela, dois quadros vizinhos mostram pedaços contínuos da mesma imagem desfocada (RF-19.4), sem conta de `uv` por quadro. Sem fonte de desfoque configurada na surface, a primitiva não desenha nada.

### 6. Ordem dentro do quadro

Em `Layer::Grid`, por quadro: sombra → **`BlurredBackdrop`** com o retângulo e o raio do quadro (`terminal_frame_corner_radius`) → fundo do quadro (`RoundedQuad` com `background_opacity`, em blend normal, como o ADR-0063 já deixa) → imagem do terminal ([ADR-0061](0061-imagem-de-fundo-do-terminal.md)) → fundo e texto das células. O fundo do quadro **é** o tingimento: nenhum valor novo. A imagem do terminal não entra no desfoque e fica nítida (RF-19.2). Célula pintada faz blend sobre o acrílico (RF-19.6).

O `BlurredBackdrop` é opaco onde a imagem da janela é opaca; onde ela é translúcida, carrega o `bar_background` desfocado junto, que é o que estaria ali sem o efeito.

### 7. Uma regra de disponibilidade, três consumidores

Uma função pura em `porecatu-config`, `acrylic_state(&Config) -> AcrylicState`, com `Active`, `Disabled` e `Blocked(AcrylicBlock)`, este com `WindowOpacity` e `NoWindowImage` (nesta ordem de precedência, RF-19.19). É chamada pela pintura (emite ou não a fonte e o `BlurredBackdrop`), pela recarga (aviso, §11) e pela tela de configurações (§10), sobre a config pendente. Uma regra, sem cópia.

- A opacidade é comparada **exatamente** com `1.0`, que é o padrão e o valor que a tela grava; não há tolerância a esconder.
- O critério é a configuração, não o estado de carga (RF-19.11), pela razão do [ADR-0063](0063-imagem-da-janela-sem-furo-ate-o-desktop.md) §1: uma imagem que falha não pode mudar o desenho de uma janela para outra.
- Com as duas condições, `wants_transparent` já dá janela opaca (ADR-0063 §2). O acrílico não entra nessa conta (RF-19.13).

### 8. Config

`[terminal.acrylic]`, com `enabled` (`false`) e `blur` (`24`, limitado a `1..=64` com aviso). Nenhuma cor: a seção não entra em tema ([ADR-0067](0067-temas-em-arquivos-proprios.md)). O bloco comentado entra no `porecatu.example.toml` junto do campo, porque `tests/example_toml.rs` reprova chave sem campo.

### 9. Recarga a quente: classe A

`enabled` e `blur` são classe A do [ADR-0030](0030-escopo-do-hot-reload.md), e também a passagem de `Blocked` para `Active` e de volta, por mudança de `[appearance.window] opacity` ou da imagem. A transparência da surface continua classe C: uma janela que nasceu transparente com opacidade `< 1` e passa a `1.0` a quente fica com surface transparente, compondo com alfa 1, e o acrílico vale nela igual. Revisão por blockquote no ADR-0030.

### 10. Tela de configurações: linha indisponível por dependência

Duas opções no grupo **Aparência**, depois das três da imagem da janela: alternância e campo numérico (`1` a `64`, passo `1`), com os controles do [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md) §3.

Elas trazem um estado de linha que a tela não tinha: **indisponível por dependência**. É a anatomia da linha somente leitura (ADR-0060 §2: controles e textos em `#5c646f`, sem edição, sem Restaurar padrão), mais uma nota abaixo da descrição em 11px **Terciário** `#828a96` — o tom que a linha já usa para o escopo de classe C, pela mesma razão: precisa ser lido. Nenhum valor novo. O estado é recalculado a cada edição pendente, por `acrylic_state` sobre a config com as pendências aplicadas (RF-19.20). O valor no arquivo não é removido (RF-19.21). Revisão por blockquote no ADR-0060 §2.

### 11. Avisos

`Blocked` com `enabled = true` gera um aviso por carga ou recarga que **cria** o bloqueio (de `Active`/`Disabled` para `Blocked`, ou de um motivo para outro), com frases por motivo em `locales/` (`notice.acrylic.window_opacity`, `notice.acrylic.no_window_image`), nunca por concatenação ([ADR-0056](0056-catalogo-de-textos-da-interface.md)). Um aviso por processo, não por janela, como os da imagem.

### 12. Aparência

Nenhuma cor, dimensão, raio ou espaçamento novo. Os únicos valores novos são os padrões das duas chaves. Com `enabled = false`, que é o padrão, ou com o acrílico bloqueado, o binário é pixel por pixel o de hoje. As §1.2, §2.7 e §4.4 da especificação são reescritas no PR que muda o binário ([ADR-0028](0028-o-binario-como-referencia-visual.md)); aqui só a tabela de fases ganha a classificação.

## Alternativas consideradas

### Acrílico do sistema operacional

`DwmSetWindowAttribute` com o fundo de sistema no Windows 11, `NSVisualEffectView` no macOS: o "acrílico de verdade", sobre o desktop. Recusada: o raio não é configurável no Windows, o que é metade do pedido; o efeito é da janela inteira, não do quadro; o X11 não tem equivalente; e desfocaria o desktop, não a imagem da janela.

### Desfocar dentro de cada quadro, a cada frame

Sem cache, sem textura guardada. Recusada: o custo cairia em todo frame de saída do PTY, multiplicado pelo número de painéis, contra o [ADR-0007](0007-modelo-de-threading.md). E daria costura entre painéis vizinhos, cada um desfocado com as próprias bordas.

### Desfocar a imagem em CPU, na thread de carga

Sem passe na GPU: a thread que decodifica guardaria também a versão desfocada. Recusada: no `stretch` a imagem depende do tamanho da janela, então cada redimensionamento pediria uma carga nova; mudar `blur` ao vivo também; e a mesma imagem em janelas de tamanhos diferentes pediria uma versão por janela.

### Dual Kawase no lugar da gaussiana

Mais barato por amostra. Recusada: o raio sai em degraus de iteração, e o pedido é um raio em pixels que o usuário escolhe. Com a cópia reduzida e o cache do §4, a gaussiana cabe no orçamento.

### Desfocar também a imagem do terminal

Recusada pelo dono do produto: ela fica nítida por cima. Também custaria um desfoque por painel, porque a imagem do terminal é posta por quadro.

### Permitir com a janela translúcida

Recusada: é o conflito do pedido. O desktop apareceria nítido atrás do que o app desfoca, e o app não tem os pixels dele.

### Permitir sem imagem da janela

Recusada: a fonte seria uma cor lisa, e o efeito, nenhum. Uma opção ligada sem efeito visível parece bug.

## Consequências

### Positivas

- O efeito sem dependência nova, sem camada nova e sem domínio no render.
- Frame comum custa uma amostragem por quadro; terminal ocioso continua sem frame.
- Uma regra de disponibilidade, a mesma no desenho, no aviso e na tela.
- Com o acrílico desligado ou bloqueado, nada muda.

### Negativas

- **A primeira exceção ao "sem passo de filtro".** O argumento de que não há como turvar o que passa por trás deixa de ser verdade em geral, e a tentação de reaproveitar o passe (sombras, widgets) vai aparecer; ela precisa de ADR próprio.
- **Uma textura a mais por janela**, do tamanho da surface reduzida, enquanto o acrílico está ativo.
- **O redimensionamento fica mais caro**: um desfoque por tamanho novo.
- **Duas condições para o usuário entender.** A tela de configurações as diz na nota; o arquivo, no comentário e no aviso.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Costura entre dois painéis vizinhos | Baixa | Médio | Amostragem por coordenada de tela (§5), uma textura por janela; verificação de pixel atravessando o vão |
| Borda da janela escurecida ou clara pelo desfoque | Média | Baixo | *Clamp* nas bordas (§3); verificação de pixel no canto do quadro rente à margem |
| Redimensionamento travando em 4K com raio 64 | Média | Médio | Cópia reduzida com teto de 32 amostras por passe; medição em 1080p e 4K registrada no roadmap |
| Desfoque com escala errada em DPI misto | Média | Médio | Raio físico = `blur × scale_factor`, `scale_factor` na chave de cache; teste do raio físico em 1.0/1.25/1.5/2.0 |
| Dupla conversão sRGB lavando a imagem desfocada | Baixa | Médio | `Rgba8Unorm` e premultiplicação, como o ADR-0061; teste de GPU comparando cor lisa antes e depois do passe |
| Flash quando a imagem termina de carregar | Média | Baixo | A chave muda uma vez, o desfoque é refeito no frame que já ia ser desenhado pela chegada da imagem |

## Registro do aval visual

> **Pendente.** Pedido na etapa de pintura do [roadmap](../roadmap.md), sobre a build. Itens a avaliar: o acrílico com o raio padrão e nos extremos (1 e 64); o tingimento pelo fundo do quadro em `background_opacity` 0.5 e 0.8; a continuidade entre painéis; a imagem do terminal nítida por cima; as linhas indisponíveis da tela de configurações.
