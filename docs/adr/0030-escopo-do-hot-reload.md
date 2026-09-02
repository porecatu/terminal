# ADR-0030 — Escopo do hot reload

**Status:** Aceito
**Data:** 2026-09-02
**Relacionados:** ADR-0003, ADR-0007, ADR-0014, ADR-0015, ADR-0018, ADR-0022, ADR-0028, ADR-0029, PRD-004, PRD-005

## Contexto

O [ADR-0003](0003-formato-de-configuracao.md) decidiu recarga a quente via `notify`, com parse fora da main thread e debounce; a [arquitetura](../arquitetura.md) desenhou o fluxo (evento de arquivo → parse em thread → troca do `Arc<Config>` → recálculo de métricas → redraw). O RF-4.20 e o RF-5.28 prometem que **salvar o arquivo aplica as mudanças de aparência em menos de 500 ms, sem reiniciar e sem perder o conteúdo das abas**.

O que ninguém decidiu é o que acontece com as chaves que **não** são aparência, ou que são aparência mas mudam algo que já existe e não se recria: `decorations` (a janela já está criada, com ou sem decoração nativa), `tab_bar_position`, `opacity` de janela, `[shell] program`, `[session] enabled`, `[keybindings]` trocado no meio de um modo de captura. Sem essa lista, a F4 decide chave por chave durante a implementação, e "aplica em menos de 500 ms" vira uma promessa que ora vale ora não — o pior dos dois mundos, porque o usuário não tem como saber qual é o caso.

Há também uma restrição que o hot reload não pode violar: **terminal ocioso não gera frame** ([ADR-0007](0007-modelo-de-threading.md)). Uma recarga precisa acordar o loop uma vez e voltar a dormir, como o tooltip e o relógio de animação já fazem.

## Decisão

Três classes, e cada chave do [`porecatu.example.toml`](../config/porecatu.example.toml) pertence a exatamente uma. A classe fica **escrita no arquivo de exemplo**, junto da chave, e é o que o usuário lê antes de esperar um efeito que não vem.

### Classe A — aplica a quente, sem tocar no PTY

Cor, fonte de chrome, dimensão, raio, espaçamento, indicador, geometria de widget, `animations`, `hover_brightness`, paleta de grupos, tema. Troca o `Arc<Config>`, recalcula o layout da barra e pede um redraw. É o caso do RF-4.20, e é a maioria do arquivo.

O layout da barra é função pura de `(Workspace, Config, largura)` desde a F2 — é isso que torna a classe A trivial: nada de estado a migrar.

### Classe B — aplica a quente, com recálculo de grade e resize de PTY

`terminal.font.*` (família, tamanho, `line_height`, `letter_spacing`), `[appearance.tabs] height`/`tab_height`/`trilha_padding` (mudam a altura da barra, logo a área do terminal) e `[appearance.terminal_frame]` (mudam a área útil dentro do quadro). Depois da troca do `Arc`, recalcula a métrica de célula, deriva colunas e linhas e **redimensiona todos os PTYs da janela** — o mesmo caminho de um resize de janela, que já existe.

O resize é **um por recarga**, coalescido com o debounce de ~200 ms do ADR-0003. Redimensionar PTY por tecla digitada no editor de config é uma tempestade de `SIGWINCH` no programa que está rodando, e há programa que redesenha a tela inteira a cada um.

### Classe C — exige reinício da janela ou do app, e **avisa**

| Chave | Por que não a quente |
|---|---|
| `decorations` | A janela já foi criada com ou sem decoração nativa; `winit` não recria o frame do SO sem recriar a janela ([ADR-0027](0027-controles-de-janela-e-resize-proprios.md)) |
| `tab_bar_position` | Move a barra de aresta, e com ela a origem de todo hit-testing e o recorte da trilha. Aplicável em teoria; fora do v1 por custo de verificação, não por impossibilidade |
| `[appearance.window] opacity` | Atributo de superfície decidido na criação da janela |
| `[shell] program`, `args`, `env` | Aba já aberta tem processo já lançado. A mudança vale para **aba nova**, e isso não é "exige reinício" — é escopo: nada muda nas abas existentes, e é isso que o aviso diz |
| `[session] enabled`, caminho | Gravação de sessão é F5; trocar o destino com sessão em memória pediria migração de arquivo |

Chave da classe C alterada gera **aviso do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md)** — severidade informação, com o nome da chave e o que fazer ("vale na próxima janela", "vale em aba nova", "reinicie o app"). O resto da recarga **é aplicado normalmente**: uma chave de classe C não invalida a gravação inteira.

Aviso é a decisão que importa aqui. A alternativa — ignorar em silêncio — produz o relato "mudei e não aconteceu nada", que é indistinguível de bug e não tem como o usuário diagnosticar.

### `[keybindings]` e modo de captura

Classe A, com a ressalva do [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md) §4: o mapa novo passa a valer imediatamente, exceto para um modo de captura em curso (rename inline, editor de grupo, diálogo), que tem teclado próprio e termina com o mapa que tinha ao abrir. Erro numa linha descarta **aquela linha**, não o mapa.

### Recarga não acorda o loop mais de uma vez

O evento de `notify` chega por `EventLoopProxy`, como o `Wakeup` de PTY (ADR-0007): o parse acontece na thread do watcher, e o que chega à main thread é o `Config` já pronto ou o erro já formatado. Uma recarga = um evento = um frame. O loop volta a `Wait` depois, e a mesma disciplina de `ControlFlow::WaitUntil` do tooltip e do relógio de animação vale aqui.

**Duas janelas compartilham um `Config`.** O `Arc` é do processo, não da janela ([ADR-0015](0015-multiplas-janelas.md), `GpuContext` do processo com surface por janela): uma recarga redesenha as duas, e o recálculo de grade da classe B roda por janela, porque a métrica é a mesma mas as dimensões não.

## Alternativas consideradas

### Tudo a quente, recriando a janela quando preciso

Cumpriria o RF-4.20 sem exceção. Rejeitada: recriar a janela perde posição, tamanho, foco e o `wgpu::Surface`, e faz um `decorations = false` salvo por engano piscar a janela inteira. O usuário está editando o arquivo com o app aberto — estado intermediário é normal (ADR-0003, regra 2), e recriação não é operação para acontecer a cada gravação.

### Ignorar em silêncio o que não aplica

Menos código, nenhum aviso a escrever. Rejeitada pelo motivo do §C: silêncio é indistinguível de bug.

### Exigir reinício para tudo (sem hot reload)

Rejeitada pelo ADR-0003, e o motivo continua: ajustar aparência é iterativo, e reiniciar perde as abas.

## Consequências

### Positivas

- Cada chave tem resposta escrita para "o que acontece quando eu salvo isto", e o arquivo de exemplo a carrega junto da chave.
- A classe B isola o único caminho caro (resize de PTY) e o coalesce com o debounce que já existe.
- A promessa do RF-4.20 fica verdadeira no escopo em que é verdadeira, em vez de valer "quase sempre".

### Negativas

- Três classes é mais complexidade de implementação que "troca o `Arc` e redesenha": alguém tem de manter a classificação junto do struct de config, e uma chave nova entra sem classe se ninguém lembrar.
- A classe C é uma lista que só cresce com o produto (`tab_bar_position` pode migrar para A quando houver ambiente para verificar).
- Comparar config antiga e nova para saber **quais** chaves mudaram exige `PartialEq` em toda a árvore de config — barato, mas obrigatório.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Chave nova entrar sem classe declarada | Alta | Baixo | Teste que percorre as folhas do `porecatu.example.toml` e reprova chave sem classe anotada |
| Classe B disparar resize em rajada durante a edição | Média | Médio | Debounce do ADR-0003, um resize por recarga, coalescido |
| Aviso de classe C virar ruído para quem edita muito | Média | Baixo | Severidade informação, que expira sozinha em 6 s (RF-10.16) |
