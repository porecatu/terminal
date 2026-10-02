# ADR-0059 — Janela de configurações: a primeira janela que não é um workspace de terminal

**Status:** Aceito
**Data:** 2026-10-02
**Supersedes:** [ADR-0009](0009-referencia-visual-e-reconciliacao.md) §6 (**parcial**: só a forma — o painel deixa de ser o drawer `[v2]` do canvas e vira janela própria, fora da ordem de fases; a regra de escrever no TOML sem segunda fonte de verdade continua inteira) · [ADR-0015](0015-multiplas-janelas.md), "Escopo" (**parcial**: "janela sem abas" sai da coluna "Fora do v1" para uma janela só, a de configurações; toda janela de **terminal** continua sendo um `Workspace`)
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0031](0031-temas-nomeados.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md), [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md), [PRD-011](../prd/prd-011-polimento.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md)

## Contexto

O [PRD-016](../prd/prd-016-tela-de-configuracoes.md) pede a tela de configurações numa **janela própria do sistema**, uma por processo — escolha do dono do produto entre três superfícies (ver as alternativas). O canvas desenhou outra coisa: um drawer de 400px sobreposto à direita da janela de terminal (§2.12 da [especificação visual](../design/especificacao-visual.md)), e o [ADR-0009](0009-referencia-visual-e-reconciliacao.md) §6 o adiou para o `[v2]`.

Hoje **toda janela é um workspace de terminal**. `WindowState` (`porecatu-ui/src/lib.rs`) carrega `workspace`, `panes`, `rename`, `search`, `git`, os popovers da barra e o adaptador de acessibilidade, e `App.windows: HashMap<WindowId, WindowState>` é o único mapa de janelas; todo handler de `WindowEvent` assume um `Workspace` do outro lado. O [ADR-0015](0015-multiplas-janelas.md) pôs "janela sem abas" explicitamente fora do v1.

Seis perguntas:

1. **Como a janela nova convive com `App.windows`** sem espalhar um `match` de tipo de janela por todo handler.
2. **Ciclo de vida**: quantas, quando fecham, o que acontece com pendências quando o app encerra.
3. **Teclado**: a janela não tem terminal, e o [ADR-0043](0043-arvore-de-acessibilidade.md) §6 recusou travessia por `Tab` no chrome justamente porque `Tab` é do shell.
4. **Onde vive o estado** da tela e o que é testável sem janela.
5. **Acessibilidade e idioma.**
6. **Que ação abre a tela** e com que tecla.

## Decisão

**A tela é uma janela do SO separada, no máximo uma por processo, guardada em `App` num campo próprio — `settings: Option<SettingsWindow>` — fora do mapa de janelas de terminal. O estado dela é um módulo puro em `porecatu-ui`; ela lê o `Arc<Config>` do processo como as outras e grava pelo [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md). A ação `settings.open` entra no catálogo, e a engrenagem passa a chamá-la.**

### 1. Um campo próprio em `App`, não um tipo de janela

`App` ganha `settings: Option<SettingsWindow>`, ao lado de `windows`. `SettingsWindow` tem a própria `WindowSurface` (surface `wgpu`, escala, métrica — o `GpuContext` e o atlas de glyphs são os do processo, como para toda janela, [ADR-0015](0015-multiplas-janelas.md) "Event loop"), o adaptador `accesskit_winit` dela e o estado da tela (§4).

O roteamento é um teste no topo de `window_event`: se o `WindowId` é o da janela de configurações, o evento vai para `SettingsWindow::handle`; senão, segue o caminho de hoje, intocado. **Nenhum handler de janela de terminal muda.**

Recusado: um `enum WindowKind { Terminal(WindowState), Settings(..) }` dentro de `windows`. Cada um dos acessos a `self.windows` — iteração para recarga, para wakeup de PTY, para gravar sessão, para contar janelas no fechamento — passaria a precisar ignorar o caso `Settings`, e esquecer um deles é bug silencioso (a sessão gravando uma janela sem workspace, o wakeup procurando aba numa janela que não tem). Um campo à parte faz o compilador dizer onde a janela nova entra, em vez de esconder isso em `match`.

### 2. Ciclo de vida

- **No máximo uma.** `settings.open` com ela aberta faz `focus_window` e a traz para a frente (RF-16.2); com ela fechada, cria. O estado da tela morre com a janela, exceto o **último grupo escolhido**, que fica em `App` até o fim da execução (RF-16.9).
- **Criação**: pelo mesmo `create_window_with_attributes` das janelas de terminal (com a árvore de acessibilidade montada antes de a janela ficar visível, como lá), centrada sobre a janela de origem, no monitor dela. Tamanho padrão e mínimo saem da `[appearance.settings]` ([ADR-0060](0060-anatomia-da-tela-de-configuracoes.md) §5).
- **Decoração**: a mesma regra das janelas de terminal ([ADR-0027](0027-controles-de-janela-e-resize-proprios.md)). Fora do macOS, sem decoração nativa — a faixa de cabeçalho da tela carrega o título, a drag region e os três botões de janela, com o mesmo `hit_test` de resize por borda; no macOS, decoração nativa. `appearance.window.decorations` vale para as duas.
- **Fora da sessão.** Não entra em `SessionFileV1`, não conta em "fechar janela com mais de uma aba" e não é restaurada.
- **Não segura o processo.** Fechar a última janela de **terminal** encerra o app, como o RF-1.4 manda. Antes de encerrar, `App` pergunta à janela de configurações se há pendências; se houver, o encerramento é **adiado**: a janela vem para a frente com o diálogo de três saídas (RF-16.4, RF-16.5), e só a resposta dele conclui ou cancela o encerramento. `app.quit` passa pelo mesmo ponto.
- **Fechar com pendências** usa o diálogo de confirmação do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), na camada modal **da janela de configurações** — o primeiro diálogo de três botões do app; a anatomia está no [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md) §4.

### 3. Teclado: a janela de configurações é um modo de captura

A janela não tem terminal, e por isso o argumento do [ADR-0043](0043-arvore-de-acessibilidade.md) §6 não se aplica a ela: **`Tab`/`Shift+Tab` percorrem guia, opções e rodapé**, com anel de foco visível, como dentro do editor de grupo (RF-16.10). O mapa de teclas do processo (`keymap`) **não** é consultado dentro dela — nenhuma ação de aba, grupo ou painel faz sentido ali, e `Ctrl+Shift+W` fechando uma aba de outra janela seria efeito fora do alvo. As teclas próprias (`Esc`, setas, `Espaço`, `Enter`, `Ctrl+S`/`Cmd+S`) são fixas, como as do diálogo, e não são ações do catálogo.

**Captura de atalho** (RF-16.29) consome a próxima combinação inteira, inclusive o que seria tecla da própria tela (`Esc` cancela a captura; `Backspace` remove o atalho). A combinação é lida pelo **mesmo** caminho que o `keymap` usa para casar uma tecla com um `Chord` (`porecatu-ui/src/keymap.rs`), e mostrada pelo mesmo `Chord::label` que os chips de tecla dos menus — um formato de tecla no app, não dois. Modificador sozinho, tecla morta e composição de IME são ignorados (o [ADR-0008](0008-teclas-e-roteamento-de-input.md), "IME e teclas mortas", vale aqui como no terminal).

A ressalva do [ADR-0030](0030-escopo-do-hot-reload.md) ("o mapa novo vale imediatamente, exceto para um modo de captura em curso") se estende à tela: uma recarga que muda `[keybindings]` com a tela aberta atualiza a lista de atalhos **exceto** a linha em captura, que termina com o que tinha.

### 4. Estado puro, em módulos testáveis

Um diretório `porecatu-ui/src/settings/`, com o padrão de `group_editor.rs` e `session_picker.rs` — estado que recebe eventos e devolve efeitos, sem `winit` nem `wgpu`:

- **`catalog.rs`** — o catálogo curado do RF-16.11 como **tabela declarativa**: para cada opção, o `KeyPath`, o grupo, os identificadores de rótulo e descrição no registro de mensagens, o tipo de controle, a faixa de edição (mínimo, máximo, passo) e o escopo de recarga (lido da mesma classificação que `reload::diff` usa, para que "vale em aba nova" na tela e o aviso depois da gravação nunca divirjam). Teste: toda entrada aponta para uma chave que existe em `Config::default()`, e todo identificador existe no catálogo de textos.
- **`draft.rs`** — o rascunho: valores lidos do `Config` em vigor, pendências como `Vec<Edit>` do [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md) (uma por chave, a última vence; voltar ao valor do arquivo apaga a pendência), validação por faixa, e os estados de arquivo inexistente, inválido e alterado fora (RF-16.21 a RF-16.23). A faixa é **regra de edição** (RF-16.18): valor fora dela vindo do arquivo é exibido e marcado, nunca corrigido sozinho.
- **`shortcuts.rs`** — a lista do grupo Atalhos: as ações vinculáveis de `porecatu-core::Action`, os atalhos efetivos pela resolução do `keymap` para a plataforma em uso, o filtro, a captura e a detecção de conflito (RF-16.28 a RF-16.31), produzindo as `Edit`s da tabela da plataforma ([ADR-0058](0058-escrita-do-arquivo-de-configuracao.md) §5).
- **`layout.rs`** — a função pura de layout da janela (guia, painel, linhas, rodapé, faixa), consumida pela pintura, pelo hit-test **e** pela árvore de acessibilidade (§5).

Componentes de controle que já existem são **reusados**, não copiados: o campo de texto é o `TextFieldState` (`text_field.rs`, com a seleção do [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md)) — quinto consumidor dele; a alternância sai de `search_bar.rs` (`push_toggle`, hoje privada) para um módulo compartilhado; a lista com rolagem segue o modelo de `session_picker.rs` (`scroll_top`, `scroll_by`, janela de linhas visíveis).

**Recarga com a tela aberta.** O evento `Reload` que já chega a `App` pelo `EventLoopProxy` também é entregue a `SettingsWindow`: a tela relê o texto do arquivo e decide pela regra do [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md) §3 — texto igual à base é a própria gravação (nada a fazer além de trocar os valores mostrados); diferente e sem pendências, troca a base em silêncio; diferente e com pendências, mostra a faixa de conflito. `ConfigReload::Invalid` põe a tela em somente leitura (RF-16.22), e o próximo `Loaded` a tira.

### 5. Acessibilidade e idioma

- **Árvore própria.** `access.rs` ganha um segundo construtor, `build_settings_tree`, projeção da mesma função de layout (§4) — o princípio do [ADR-0043](0043-arvore-de-acessibilidade.md) §2 ("árvore é projeção do layout") vale igual. Bloco de `NodeId` próprio, sem sobreposição com o da janela de terminal (cada janela tem sua árvore, mas um bloco separado evita confusão em teste). Papéis: lista para a guia, `Tab`/`TabPanel` para grupo e painel, `CheckBox`/`Switch` para alternância, `TextInput`/`SpinButton` para campos, `ComboBox` para escolha; estados de pendente e inválido expostos como descrição.
- **Idioma.** A janela lê o `Arc<Catalog>` do processo, como `WindowState.catalog`; a troca ao vivo do [ADR-0056](0056-catalogo-de-textos-da-interface.md) põe o catálogo novo também nela e a redesenha. Rótulo e descrição de opção, nome de ação e toda mensagem são frases do registro (`settings.*`); os nomes de ação, hoje só identificadores, ganham frase própria (`action.*`) porque é a primeira superfície que os mostra ao usuário.

### 6. A ação `settings.open` e a engrenagem

- **`Action::SettingsOpen`**, `settings.open`, no catálogo fechado ([docs/reference/acoes.md](../reference/acoes.md)), origem RF-16.1, vinculável, sem argumento.
- **A engrenagem chama a ação.** O clique deixa de devolver `NewTabRequest::OpenConfigFile` e passa a disparar `settings.open` — a mesma regra que a barra já segue para o "+" e `tab.new`. `open_config_file` e `ensure_config_file_exists` **continuam**, agora chamados pelo botão "Abrir arquivo no editor" da tela (RF-16.14). Emenda ao RF-11.27 do [PRD-011](../prd/prd-011-polimento.md).
- **Defaults:**

| Plataforma | `settings.open` | `config.reload` |
|---|---|---|
| Windows / Linux | `Ctrl+Shift+O` (novo) | `Ctrl+Shift+,` (sem mudança) |
| macOS | `Cmd+,` (**era** `config.reload`) | `Cmd+Shift+,` (novo) |

No macOS, `Cmd+,` é a convenção do sistema para "Ajustes…" em todo aplicativo, e deixá-la em "recarregar config" foi coerente enquanto não existia tela de ajustes; agora seria a convenção apontando para o lugar errado. `config.reload` vai para `Cmd+Shift+,`, simétrico ao `Ctrl+Shift+,` das outras plataformas. É a mudança de um default estabelecido, e por isso nomeada aqui; quem tiver `"cmd+comma" = "config.reload"` escrito no próprio arquivo continua com ele.

No Windows e no Linux, `Ctrl+,` sozinho ficaria fora da regra do [ADR-0008](0008-teclas-e-roteamento-de-input.md) (nada de `Ctrl+<tecla>` sem `Shift` — o espaço é do terminal) e `Ctrl+Shift+,` já é `config.reload`. `Ctrl+Shift+O` está livre e lê como "opções".

## Alternativas consideradas

### Sobreposição na camada modal da janela de terminal

O drawer do canvas, alargado para ocupar a área abaixo da barra de abas. Seria o caminho mais barato — camada `Modal` do [ADR-0018](0018-composicao-de-frame.md), sem janela nova, sem segundo roteamento de eventos —, e esconde o terminal enquanto aberta: o usuário que muda a fonte não vê a grade mudar, e a tela fica presa a uma janela entre várias. Recusada pelo dono do produto.

### Aba especial, sem PTY

Uma aba "Configurações" na trilha. Exigiria que `Tab` deixasse de ter sempre ao menos um painel com terminal — a refatoração dos painéis divididos ([ADR-0053](0053-paineis-divididos.md)) acabou de fixar o contrário —, e entraria em sessão, grupos, arraste e painéis, cada um com a pergunta "e se a aba for a de configurações?". Recusada.

### `enum WindowKind` em `App.windows`

Recusada no §1: transforma "a janela nova é diferente" em `match` espalhado, e o caso esquecido não falha a compilação.

### Mapa de teclas do processo ativo dentro da janela de configurações

Deixaria `Ctrl+Shift+T` abrir uma aba "em algum lugar" e `Ctrl+Shift+Q` fechar a janela errada. A tela tem poucas teclas, e todas são dela.

### `Ctrl+,` como default no Windows e no Linux

É o que o canvas mostra e o que VS Code e navegadores usam. Recusado pela regra do [ADR-0008](0008-teclas-e-roteamento-de-input.md), que vale sem exceção para defaults: o espaço de `Ctrl+<tecla>` sem `Shift` pertence ao programa que roda no terminal, e o usuário que o quer o vincula na própria tela.

## Consequências

### Positivas

- Nenhum handler de janela de terminal muda; a janela nova entra por um ponto de roteamento.
- A tela é testável sem janela: catálogo, rascunho, atalhos e layout são funções puras.
- Um formato de tecla no app: captura e chips usam o `Chord` do `keymap`.
- O macOS ganha a convenção de `Cmd+,`.

### Negativas

- `App` passa a ter dois tipos de janela, e tudo que é "para cada janela" precisa decidir se inclui a de configurações (recarga e troca de idioma sim; sessão, wakeup de PTY e contagem de abas não). O campo separado torna a decisão explícita, mas ela existe.
- O encerramento do app ganha um passo assíncrono (esperar a resposta do diálogo de pendências), onde antes era síncrono.
- Mudança de um default estabelecido no macOS (`Cmd+,`).
- ~150 frases novas no registro de mensagens, nos cinco arquivos de `locales/`.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Algum laço sobre janelas esquece a de configurações (recarga, tema, idioma) | Média | Médio (tela com tema ou idioma velho) | Recarga e troca de catálogo passam por uma função `App::for_each_surface` que inclui as duas; teste de que a recarga alcança a tela |
| Encerramento preso esperando um diálogo que não abriu | Baixa | Alto | O adiamento só acontece se a tela existe **e** tem pendências; o diálogo é aberto no mesmo passo, e fechar a janela de configurações pelo SO conta como "cancelar" |
| Captura de tecla diferente do que o `keymap` casa depois | Média | Médio | Captura e casamento usam a mesma conversão de evento para `Chord`; teste de ida e volta `capturar → gravar → resolver → casar` |
| Janela abre fora da tela em configuração multi-monitor | Baixa | Baixo | Centrada sobre a janela de origem e recortada ao monitor dela, como a cascata de `window.new` |
