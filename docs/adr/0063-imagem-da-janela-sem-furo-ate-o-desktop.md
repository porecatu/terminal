# ADR-0063 — Imagem da janela sem furo até o desktop

**Status:** Aceito
**Data:** 2026-10-06
**Relacionados:** [ADR-0018](0018-composicao-de-frame.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0061](0061-imagem-de-fundo-do-terminal.md), [PRD-018](../prd/prd-018-imagem-de-fundo-da-janela.md)
**Supersedes:** [ADR-0062](0062-imagem-de-fundo-da-janela.md) (parcial — o §4, "Dentro do quadro, em janela transparente: furo, imagem recortada, fundo", a alternativa recusada "Trocar o `REPLACE` do fundo do quadro por blend normal quando há imagem" e a consequência negativa "Célula pintada em janela transparente mostra o desktop")

## Contexto

O §4 do ADR-0062 decidiu que, numa surface transparente, o fundo translúcido do quadro de terminal continua furando até o desktop. A imagem da janela é reposta dentro do furo com o alfa **dela**, e o pixel final tem alfa `b + i·(1 − b)`. Com a imagem a `1.0`, o desktop some. Com a imagem translúcida, ele volta na medida do que falta.

O dono do produto relatou que, às vezes, uma janela mostra a imagem atrás do terminal translúcido e outra mostra o desktop. Às vezes o app também fechava. O defeito foi reproduzido ao vivo com a imagem dele (`term_bg.jpg`), imagem a `0.2` e terminal a `0.5`:

- **App iniciado opaco e mudado a quente.** Na janela original e na sessão nomeada restaurada, a imagem aparece fraca sobre o fundo escuro, sem desktop.
- **App iniciado já com o terminal translúcido.** Nas duas janelas, o quadro do terminal mostra 40% de desktop.

A causa não está numa janela ou noutra, e sim no par "regra do §4 + transparência decidida na criação". A transparência da surface sai de `wants_transparent` quando a janela nasce: janela `< 1` **ou terminal `< 1`**, classe C. Ela não muda depois. Numa surface opaca não há furo, e a mesma config desenha a imagem sobre o fundo da janela. Numa transparente, há furo, e a mesma config desenha a imagem sobre o desktop. O resultado depende da config que valia quando cada janela nasceu e do caminho de GPU que o processo pegou. O app iniciado opaco usa o caminho sem DirectComposition, e as janelas novas dele saem opacas. Para quem usa, isso é "às vezes".

O pânico não pôde ser reproduzido nem investigado: o binário é `windows_subsystem = "windows"`, sem console, e não havia hook que guardasse a mensagem.

## Decisão

### 1. Com imagem da janela, o terminal translúcido revela a imagem, nunca o desktop

Com `[appearance.window.background_image] path` não vazio, o fundo translúcido do quadro **não fura**. `backdrop_punch` passa a ser `surface transparente && sem imagem da janela` (`lib.rs`), e o fundo do quadro e o de cada célula com cor própria se misturam em blend normal sobre o que está embaixo. Embaixo está a imagem da janela, que continua a primeira primitiva de `Layer::Grid` (ADR-0062 §2), sobre o `clear` em `bar_background`. É o mesmo pixel que a surface opaca já desenhava. Toda janela desenha igual, tenha nascido opaca ou transparente.

O desktop só aparece pela **opacidade da janela** (`[appearance.window] opacity < 1`), pelo blit que já compõe a cena inteira com esse alfa. Ele age igual sobre a barra, a margem, o vão e o quadro.

O critério é a **configuração** (caminho não vazio), não o estado de carga. Uma imagem que ainda carrega, que falhou ou que está com `opacity = 0` decide igual a uma pronta. Se não fosse assim, uma imagem que falhasse voltaria a separar janela opaca de transparente, e o arranque mostraria um flash de desktop enquanto a carga espera o primeiro prompt (ADR-0061).

O ramo de três primitivas do §4 (furo, imagem recortada pelo quadro, fundo) sai de `paint.rs`, e com ele o parâmetro da imagem da janela de `build_primitives_with_image`. Nada novo no render.

### 2. A janela nova não pede surface transparente por causa do terminal, se há imagem da janela

`wants_transparent` = `[appearance.window] opacity < 1` **ou** (`[terminal] background_opacity < 1` **e** sem imagem da janela). Com imagem e janela opaca, nada precisa compor com o desktop, e a janela nasce opaca. O §1 continua necessário para as janelas que nasceram transparentes antes de a imagem entrar na config (classe C, "vale na próxima janela", inalterada).

### 3. Pânico registrado em arquivo

`crash_log::install`, no começo de `porecatu_ui::run`, acrescenta a mensagem, o local e o backtrace a `crash.log`, no diretório do arquivo de sessão (o mesmo que `PORECATU_SESSION` desloca), e chama o hook padrão em seguida. O texto é inglês fixo, de desenvolvedor, como a saída de `PORECATU_TRACE`, e não é interface (ADR-0056). Sem dependência nova. Um erro de validação do `wgpu` sem tratador vira pânico e cai aqui também.

## Alternativas consideradas

### Manter o furo e só tornar a transparência igual entre as janelas

Criar toda janela com a mesma transparência, por exemplo sempre pelo caminho DirectComposition no Windows. Recusada pelo dono do produto: ele não quer o desktop atrás do terminal quando há imagem. Além disso, a regra do ADR-0062 §4 continuaria dando um quadro diferente da barra, onde a imagem está sobre o `clear` opaco.

### Decidir pelo estado de carga (`Ready`) em vez do caminho

Recusada pelos dois motivos do §1: imagem que falha separando janelas, e flash de desktop no arranque.

## Consequências

### Positivas

- A mesma config desenha a mesma coisa em toda janela, com qualquer histórico de recarga e em qualquer caminho de GPU.
- Uma regra só para "ver o desktop": a opacidade da janela.
- Célula com fundo próprio deixa de ser o único lugar da janela que mostra o desktop em vez da imagem (a consequência negativa do ADR-0062 cai).
- O próximo pânico deixa registro.

### Negativas

- Quem quer o desktop atrás do terminal **e** a imagem na janela precisa usar `[appearance.window] opacity`, que deixa a janela inteira translúcida, não só o terminal.
- Célula com fundo próprio passa a deixar a imagem transparecer na medida de `background_opacity` (blend), em vez de cobri-la com `REPLACE`. É o que a surface opaca sempre fez.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Janela já aberta e transparente, imagem removida a quente: volta a furar | Certa | Baixo | É a regra sem imagem, a de antes do PRD-018; a próxima janela segue `wants_transparent` |
| `crash.log` crescer sem limite | Baixa | Baixo | Só recebe pânico, que encerra o processo; uma entrada por crash |
