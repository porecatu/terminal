# ADR-0053 — Painéis divididos: árvore binária no core, divisor que é ausência de pixel, foco pelo cursor

**Status:** Aceito
**Data:** 2026-09-15
**Relacionados:** ADR-0003, ADR-0006, ADR-0007, ADR-0008, ADR-0013, ADR-0014, ADR-0015, ADR-0017, ADR-0018, ADR-0021, ADR-0022, ADR-0027, ADR-0028, ADR-0030, ADR-0032, ADR-0033, ADR-0036, ADR-0037, ADR-0038, ADR-0039, ADR-0041, ADR-0043, ADR-0048, ADR-0051, ADR-0052, PRD-000, PRD-001, PRD-003, PRD-006, PRD-009, PRD-011, PRD-012
**Supersedes:** ADR-0006, Alternativas (**parcial**: só o item "Splits/panes dentro da aba"; as quatro restrições do modelo de grupos continuam inteiras) · ADR-0017 (**parcial**: só o nível em que o ciclo de vida acontece; a escada de decisão não muda) · ADR-0036 (**parcial**: só o conteúdo de `TabV1`, sem subir `schema_version`) · ADR-0037 (**parcial**: só a granularidade de `NotStarted`; o gatilho continua sendo o foco da aba) · ADR-0039 (**parcial**: só o endereço da nota) · ADR-0041 (**parcial**: só o retângulo sobre o qual a barra de busca se posiciona) · ADR-0043 (**parcial**: acrescenta nós de painel; a grade continua fora) · ADR-0048 §5 (**parcial**: só a contagem de painéis, que aquele parágrafo já previa que voltaria) · ADR-0051 §6 (**parcial**: só qual terminal recebe o comando numa aba dividida)

## Contexto

O [ADR-0006](0006-modelo-de-abas-e-grupos.md) registrou, entre as alternativas recusadas, uma frase que era metade decisão e metade profecia:

> Fora do escopo do v1 por decisão de produto (PRD-000). Registrado aqui porque afeta o modelo: se entrar, `Tab` passa a conter uma árvore de panes em vez de um terminal. O modelo atual não impede — `Tab` já é uma struct, não um alias de terminal — mas a mudança seria real e mereceria ADR próprio.

Este é esse ADR, e a mitigação funcionou: `Tab` é struct própria desde o primeiro dia, e o [PRD-006](../prd/prd-006-paineis-divididos.md) foi promovido a Aprovado sem que nenhuma decisão anterior precisasse ser revogada. O que existe é um conjunto de **revisões parciais**, todas do mesmo tipo: coisas que eram "por aba" passam a ser "por painel", e a aba passa a ser a agregação delas.

O que o PRD deixou em aberto, e este documento fecha:

1. **Onde a árvore vive**, e se ela é árvore mesmo.
2. **O que deixa de ser da aba** — porque `Tab` carrega hoje seis campos que descrevem um shell, e com N shells eles não têm dono.
3. **Como o divisor existe sem existir**, dado que o dono do produto pediu explicitamente que ele seja só o vão entre dois quadros, e que cada painel respeite raio, sombra, recuo, fonte e cores do terminal inteiro de hoje.
4. **Como se vê o foco**, sem cabeçalho de painel.
5. **Como um arraste contínuo cabe** num app damage-driven que já tem duas outras famílias de arraste, uma delas delegada ao sistema operacional.

O pedido do dono do produto foi literal em três pontos, e os três são restrições de desenho, não preferências: dividir com `Ctrl+Shift+H` e `Ctrl+Shift+D`, arrastar o meio entre os painéis para redimensionar, e *"os divisores entre os terminais devem ser somente um espaço entre os dois terminais, como são, por exemplo, as próprias margens entre a janela e o terminal"*.

Vale dizer o que **não** entrou em disputa. `Ctrl+Shift+V` foi pedido primeiro e recusado na verificação: ele já é `clipboard.paste` nas três plataformas ([ADR-0008](0008-teclas-e-roteamento-de-input.md)), e é a convenção universal de colar em terminal. `Ctrl+Alt+H`/`Ctrl+Alt+V` foram considerados em seguida e descartados pelo dono do produto por dois custos: no Windows e no Linux, `Ctrl+Alt` é como o sistema sintetiza o **AltGr**, e o [ADR-0008](0008-teclas-e-roteamento-de-input.md) é enfático que teclado ABNT2 e IME não podem ser capturados por engano; e `Alt+Ctrl+H` é `backward-kill-word` no readline por padrão. `Ctrl+Shift+H` e `Ctrl+Shift+D` ficam **dentro** do padrão que o ADR-0008 fixou para Windows e Linux, e não movem nenhum default existente.

## Decisão

**A aba passa a conter uma árvore binária de painéis, e cada painel é um terminal completo — com PTY, grade, scrollback, diretório e ciclo de vida próprios. O que separa dois painéis não é desenhado: é o mesmo vão que já existe entre a janela e o terminal, e cada painel repete o quadro arredondado inteiro que a §2.7 da especificação visual já descreve, sem um único valor novo. O foco se vê pelo cursor — cheio no painel focado, vazado nos demais — e por mais nada. A árvore mora em `porecatu-core`, o layout é função pura sobre ela, e o `Wakeup` passa a endereçar painel, não aba.**

### 1. A árvore mora no core, e é binária

`PaneId` nasce em `crates/porecatu-core/src/id.rs`, no mesmo molde de `TabId` e `GroupId`: inteiro opaco, estável, gerado por contador monotônico, serializado com a sessão. O nó é folha ou split:

```
PaneNode = Leaf(PaneId)
         | Split { axis, ratio, first, second }
```

Três razões para binária em vez de n-ária, e a terceira é a que decide:

- **O gesto é binário.** Cada split divide um painel em dois; não existe gesto no produto que produza três irmãos de uma vez.
- **A proporção é um número só.** Com N irmãos, arrastar um divisor exige decidir quem cede e quanto, e a resposta muda conforme o divisor arrastado. Com dois, `ratio` é a posição do divisor e não há ambiguidade.
- **O arraste tem alvo óbvio.** Todo divisor é exatamente um nó `Split`; arrastar é escrever `ratio` naquele nó. Numa lista de N filhos, o mesmo gesto é uma redistribuição, que é onde a implementação cara mora.

A árvore fica no **core**, e não em `porecatu-ui`, por uma razão estrutural: `porecatu-session` depende só de `porecatu-core` ([CLAUDE.md](../../CLAUDE.md), tabela de dependências), e ela precisa serializar o layout. Árvore na UI obrigaria a sessão a conhecer a UI, o que o grafo proíbe.

Como o resto do domínio, as operações são puras e testáveis sem janela: `split`, `close`, `focus`, `focus_in_direction`, `set_ratio`, `leaves_in_order`. Os invariantes vão para teste, no molde dos de `Workspace`: toda aba tem pelo menos um painel; há exatamente um focado, e ele é folha da própria árvore; `ratio` está sempre no intervalo aberto; fechar o penúltimo painel colapsa o nó `Split` e devolve a folha ao lugar dele.

### 2. Seis campos deixam de ser da aba

Hoje `Tab` (`crates/porecatu-core/src/tab.rs:33-54`) carrega `process_title`, `cwd`, `shell_name`, `state`, `activity` e `bell`. Os seis descrevem **um shell**. Com N shells eles não têm dono, e a saída não é escolher um: é mudar de nível.

Os seis migram para `Pane`. `Tab` fica com identidade, `custom_title`, a árvore e o painel focado — e **deriva** o que a barra de abas precisa:

| O que a aba mostra | De onde passa a vir |
|---|---|
| Título (RF-1.7) | Do painel **focado**, com a mesma precedência de antes aplicada dentro dele. `custom_title` continua sendo da aba e continua vencendo tudo |
| Atividade e campainha (RF-1.20, RF-1.21) | **Agregação**: qualquer painel acende o indicador da aba |
| Diretório, para a barra de status e para a sessão | Do painel **focado** |
| Estado | Deixa de existir como valor único — ver §10 |

A agregação de atividade não é detalhe de implementação: é o que impede uma aba de esconder a saída de metade dos terminais dela. Já o título vir do focado, e não de uma concatenação, é a escolha oposta e pela mesma razão — o rótulo da aba tem 180px de teto ([§2.5](../design/especificacao-visual.md)), e dois títulos truncados ali não informam nenhum.

### 3. O divisor é ausência de pixel

A régua já está escrita no projeto, no topo de `crates/porecatu-ui/src/status_bar.rs`: **quem é mobília encolhe a grade; quem é transitório sobrepõe.** O divisor é mobília. Ele sai do retângulo **antes** do cálculo de linhas e colunas, e é por isso que ele não pode ser "desenhado por cima" de nada.

O vão entre dois painéis é o **`terminal_frame_margin`** — os mesmos 6px que já separam a janela do terminal. Cada painel então recebe, de dentro para fora, exatamente o que a §2.7 já descreve para o terminal único: fundo `#0f1216`, quadro arredondado de raio 6, a sombra em camadas da §1.7 (`chrome::push_shadow`) por baixo, e a grade recuada mais 6px por dentro do quadro nos quatro lados.

**Nenhum valor novo, nenhuma cor nova, nenhuma primitiva nova.** É a forma mais forte do argumento que a própria §2.7 usa ("os valores tirados de `trilha_padding` e `wrapper_padding` — nada de número novo"), e é o que faz um recurso que ocupa metade da área de conteúdo do app não introduzir um único token.

Uma consequência que vale antecipar: com dois painéis lado a lado, o vão entre eles tem 6px, e a margem de cada um contra a borda da janela tem outros 6px. O espaço entre um painel e a janela **não** é somado ao vão — o vão é o espaço entre os dois quadros, e é ele que o arraste move.

### 4. O foco é o cursor, e só

Sem cabeçalho de painel, alguma coisa precisa dizer para onde o teclado vai. A decisão do dono do produto, entre três opções apresentadas, foi a mais discreta das três: **o painel focado desenha o cursor como sempre desenhou; os demais desenham o mesmo cursor vazado.**

Tecnicamente isso não custa primitiva nova: o cursor vazado é um `RoundedQuad` de raio 0, preenchimento transparente e borda de 1px na cor do cursor — e ele desenha um retângulo reto de verdade desde a correção de `sdf_box` no `quad.wgsl`, que passou a usar distância de Chebyshev quando o raio é zero. É a convenção nativa de emulador de terminal: cursor cheio é foco, cursor vazado é "esta janela existe e não é a que recebe teclas".

**O custo está aceito e registrado**: um painel rodando `vim` em modo normal, ou qualquer TUI que esconda o cursor, não tem marca de foco nenhuma. As duas alternativas que cobriam esse caso — borda na cor do grupo e esmaecer os painéis sem foco — foram recusadas, e estão nas alternativas com o motivo. A mitigação real é que o foco quase sempre acabou de ser posto pelo usuário, por clique ou por `Alt+seta`, e o estado que ele não vê é o que ele mesmo escolheu há um segundo.

### 5. O layout é função pura, e o retângulo externo continua tendo uma fonte só

A subdivisão é uma função pura:

```
(árvore, retângulo do quadro, style) -> Vec<(PaneId, Rect)>
```

`paint::terminal_box_rect` e `terminal_content_rect` (`crates/porecatu-ui/src/paint.rs:93` e `:117`) **continuam sendo a fonte única** do retângulo externo — quem já sabe descontar a barra de abas, a barra de status e as margens continua sabendo. A função de painéis **subdivide** o que elas devolvem, recursivamente, tirando o vão a cada nó `Split`. Duas propriedades seguem daí, e as duas são testáveis sem abrir janela: a soma dos painéis mais os vãos é o retângulo original, e o mesmo ponto lógico cai sempre no mesmo painel.

É o mesmo movimento que fez o layout da barra de abas ser a função pura da §7 da [arquitetura](../arquitetura.md), e pela mesma razão: aninhamento, proporção, mínimo e hit-test passam a ser testáveis em unidade.

`grid_size` deixa de ser da janela e passa a ser de um retângulo. Hoje ela é `WindowState::grid_size(cell_metrics, style)` e devolve um par para todas as abas; `resize_to` aplica esse par a **todos** os runtimes num laço só (`lib.rs:3639`). Depois disto, cada painel calcula o seu a partir do retângulo dele, e o laço passa a redimensionar cada terminal com o par que é dele.

### 6. Um runtime por painel, e o `Wakeup` endereça painel

`WindowState.tabs: HashMap<TabId, TabRuntime>` (`lib.rs:663`) vira um mapa de `PaneId` para `PaneRuntime`. Todo o conteúdo do runtime de hoje é por terminal, não por aba — `Terminal`, `snapshot`, `spawn_cwd`, `received_osc7`, o pendente de comando de projeto —, então a troca é de chave, não de conteúdo.

`Wakeup::TabDirty { window, tab }` (`lib.rs:248`) passa a carregar também o painel. **É a mesma razão pela qual ele já carrega `WindowId`** ([ADR-0015](0015-multiplas-janelas.md)): os IDs são por workspace, e o evento sozinho não diz o que sujou. Sem o painel, o sintoma seria o painel errado redesenhando — a versão exata, um nível abaixo, do bug que o ADR-0015 registrou como "a janela errada redesenhando".

### 7. O arraste do divisor entra na máquina de estados que já existe

`enum Drag` (`lib.rs:198-232`) ganha os estados de divisor e nada mais muda de forma: press arma o estado com o nó alvo, o limiar de 4px (`DRAG_THRESHOLD_PX`) promove a arraste, `set_cursor` muda a forma, cada `CursorMoved` pede um redraw, o release volta a `Idle`.

**Duas diferenças em relação ao arraste de aba, e as duas são simplificações.** Não há clone do `Workspace`: o arraste de aba clona e aplica no clone porque reordenar de verdade durante o gesto obrigaria a implementar undo; aqui o que muda é um `ratio`, o gesto é contínuo por natureza, e não existe "soltar fora" que precise descartar alguma coisa. E não há `AnimationClock`: a cadência é a dos eventos de mouse, como já é no arraste de aba e na seleção de texto. O [ADR-0022](0022-animacao-de-interface.md) continua com os **dois** consumidores fechados que ele tem.

O divisor também não é um retângulo de zero pixel para o mouse: a faixa sensível é o vão de 6px, que é a mesma medida da zona de resize da janela (`resize_border`, ADR-0027) — o que é uma coincidência útil, não um valor compartilhado.

### 8. O PTY reencaixa durante o arraste

A grade de cada painel é recalculada e `Terminal::resize` é chamado **a cada quadro do arraste**, não só ao soltar. O comentário que já está em `lib.rs:3620-3625` diz o que sustenta isso: `Terminal::resize` é barato de chamar em rajada, e perder um em trânsito não é grave — o motor é redimensionado de forma síncrona e o PTY recebe o par pela thread de observação, que sempre aplica o último.

A alternativa está nas alternativas recusadas. O que ela custaria é visível: durante todo o gesto, o conteúdo ficaria com a grade velha dentro de um quadro de tamanho novo — texto cortado de um lado e faixa vazia do outro —, e o usuário estaria arrastando sem ver o resultado do que arrasta, que é justamente o que um arraste existe para dar.

### 9. Mínimo de painel: comportamento, não aparência

O limite do RF-6.4 e do RF-6.14 é uma seção própria de config, no molde de `[git]`:

```toml
[panes]
min_columns = 20
min_rows = 5
```

Ele **gateia o split** (dividir um painel que não comporta dois é recusado, com aviso do canal 1 do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md)) e **clampa o arraste** (o divisor para quando um dos vizinhos chega ao limite).

Duas decisões dentro dela. **Não é `[appearance]`**, porque não descreve dimensão desenhada nenhuma: é um limite sobre uma ação. Por consequência, não entra na tabela `VALORES` de `scripts/verify-docs.py`, que existe para impedir divergência silenciosa entre código, arquivo de exemplo e especificação visual de valores **estruturais de aparência**. E são **dois** números, não um: 20 linhas e 20 colunas não são a mesma quantidade de terminal, e um valor único obrigaria a escolher qual dos dois eixos fica errado.

Classe de recarga **A** ([ADR-0030](0030-escopo-do-hot-reload.md)): aplica a quente sem tocar no PTY. Mudar o mínimo não redimensiona nada que já existe — governa o próximo split e o próximo arraste.

### 10. O ciclo de vida desce um nível

`TabState` (`NotStarted`, `Running`, `Exited`) passa a ser estado de **painel**. A escada de decisão do [ADR-0017](0017-ciclo-de-vida-da-aba.md) não muda de forma; muda de sujeito:

- `TermEvent::Exit` com código zero fecha **o painel** (`lib.rs:5864-5890`), e só fecha a aba quando ele era o último. Com código diferente de zero, o painel fica aberto com a nota de saída, como a aba ficava.
- A confirmação de fechamento ([PRD-001](../prd/prd-001-abas.md) RF-1.6, [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md)) passa a ser sobre o painel. Fechar a aba pergunta se **qualquer** painel dela tem processo ativo; fechar a janela, se qualquer painel de qualquer aba tem.
- `window_close_needs_confirmation` e `should_confirm_tab_close` (`lib.rs:770-787`) recebem contagem de painéis ocupados. **Atenção ao que a F5 já ensinou**: `request_close_window` conta `state.tabs.len()` justamente porque o mapa de runtimes exclui `NotStarted` de graça. Com o mapa passando a ser de painéis, o mesmo `len()` passa a contar painéis — o que continua sendo a pergunta certa ("quantos terminais vivos há nesta janela"), mas **por acidente**, e por isso vai explícito em teste.

Cada painel tem seu próprio `ProcessGroup` ([ADR-0033](0033-job-object-encerramento-de-processo.md)) e seu próprio Job Object: fechar um painel mata a árvore de processos dele, e não toca a dos vizinhos.

### 11. A restauração preguiçosa continua sendo por aba

O `NotStarted` do [ADR-0037](0037-aba-nao-iniciada.md) passa a ser por painel, mas o **gatilho não muda de nível**: focar uma aba restaurada sobe **todos** os painéis dela, de uma vez.

A alternativa — subir só o painel focado e deixar os outros esperando — foi recusada por duas razões. A primeira é de produto: meia aba com prompt e meia aba em branco é um estado que ninguém pediu e que o rótulo esmaecido do RF-3.9 não sabe descrever, porque ele é da aba. A segunda é geométrica: as proporções precisam dos dois lados para valer, e um painel `NotStarted` que ainda não tem grade não tem como dizer quantas colunas o vizinho pode ter.

O que a sessão grava é a árvore, os `ratio`, o `cwd` e o programa de cada painel, e qual deles estava focado — e isso cabe em `TabV1` **sem subir `schema_version`**, o que é exatamente a propriedade que o [ADR-0036](0036-formato-do-arquivo-de-sessao.md) escolheu ao pôr `#[serde(default)]` no container: campo opcional novo, ausência significando "um painel só". O mecanismo de migração encadeada continua onde está, ainda com a lista vazia. Uma sessão gravada por esta versão e lida por uma anterior devolve uma aba com um painel — perda de layout, não corrupção.

### 12. O `.porecatu` roda uma vez por aba, no painel focado

É a decisão mais consequente do recurso sobre um documento já aceito. O [ADR-0051](0051-arquivo-de-projeto-porecatu.md) dispara o comando do projeto por **aba restaurada**; a leitura ingênua com painéis seria "por terminal restaurado", e ela está errada por um motivo concreto: **painéis de um split quase sempre compartilham o diretório**, porque o painel novo herda o `cwd` do painel de origem (RF-6.3). Rodar o `.porecatu` em dois deles é subir o mesmo servidor duas vezes na mesma porta, e o segundo morre com um erro que parece bug do app.

Então: uma vez por aba restaurada, no painel focado. O resto do ADR-0051 sobrevive inteiro — a allowlist vazia por padrão, a checagem de confiança, a escrita no PTY como se digitada, a espera pelo primeiro byte mais o silêncio, o teto de 5s e a guarda de tela alternativa. `SpawnOrigin` continua sendo parâmetro explícito, nunca inferido, e agora ele é de painel.

Pela mesma razão de "uma vez", o convite de integração de shell do [ADR-0039](0039-convite-a-integracao-de-shell.md) passa a ser endereçado a `(janela, painel)` em vez de `(janela, aba)`, e continua acontecendo **uma vez por execução do app** — a mudança é só de destino.

### 13. Acessibilidade: os painéis entram, a grade continua fora

A árvore do [ADR-0043](0043-arvore-de-acessibilidade.md) ganha os painéis como filhos do nó da aba: cada um com posição, rótulo derivado do título e o estado de foco. É estrutura, e estrutura é exatamente o que aquele ADR decidiu projetar.

**O conteúdo da grade continua declaradamente fora**, e a fronteira não se mexe. O que muda é o namespacing de `NodeId` (`crates/porecatu-ui/src/access.rs`), que ganha uma faixa própria para painéis — a mesma disciplina de `TAB_STRIDE` e `GROUP_ID_BASE` que já está lá. Como hoje, a árvore é projeção das funções puras de layout, e não uma segunda descrição do desenho: com a §5, a função que posiciona os painéis é a mesma que a árvore consulta.

### 14. O que muda na barra de status, e o que não muda

Um `SegmentRole` novo, com a contagem de painéis, **só quando há dois ou mais**. Com um painel, o segmento não existe — nem apagado, nem "1 painel". É literalmente a condição que o [ADR-0048](0048-barra-de-status.md) §5 escreveu ao cortá-lo (*"hoje exibiria '1 painel' para sempre, que é ruído com aparência de informação. Volta quando os painéis existirem"*), e é a mesma regra de ausência do indicador de commits ([ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md)) e do ícone de repositório.

Os demais segmentos — shell, diretório, grupo, Git — passam a descrever o **painel focado**. Não é mudança de regra: a barra sempre descreveu o terminal em foco, e o terminal em foco passou a ser um painel. O RF-9.8 não é tocado por este documento: a contagem muda por gesto do usuário, e gesto do usuário já desenhava quadro.

A busca ([ADR-0041](0041-busca-no-scrollback.md)) continua sendo **uma por janela**, amarrada agora a um painel, e se posiciona sobre o quadro **daquele painel** em vez de atravessar a aba. `reserved_rows` e `scroll_delta_to_reveal` já são funções puras que recebem as linhas de fora — elas funcionam sem mudar uma linha, desde que recebam as do painel. A captura parcial de teclado e as ocorrências como lista de ranges sobrevivem inteiras.

### 15. Onde o código mora

| Camada | O que entra |
|---|---|
| `porecatu-core` | `PaneId` (`id.rs`); `Pane` e a árvore (`pane.rs`, novo); os seis campos que saem de `Tab` (`tab.rs`); as sete ações `pane.*` em `Action` e no `CATALOG` (`action.rs`) |
| `porecatu-config` | `[panes]` com `min_columns` e `min_rows`; os seis defaults de `[keybindings]` |
| `porecatu-session` | `PaneV1` e o campo opcional em `TabV1` (`schema/v1.rs`); a conversão em `convert.rs` |
| `porecatu-ui` | o layout puro (`panes.rs`, novo); `PaneRuntime` e o mapa por `PaneId`; o `Wakeup` com painel; N quadros e o cursor vazado em `paint.rs`; os estados de divisor em `Drag` e o hit-test; a contagem em `status_bar.rs`; os nós em `access.rs` |
| `porecatu-term` | **nada.** O crate já trata um terminal por vez e nunca soube o que é uma aba |
| `porecatu-render` | **nada.** Nenhuma primitiva nova, nenhuma camada nova — os quadros vão em `Grid`, como o quadro único já vai |

Que `porecatu-term` e `porecatu-render` não mudem é o teste retroativo das fronteiras do [ADR-0018](0018-composicao-de-frame.md) e da §4 da [arquitetura](../arquitetura.md): o recurso que mais mexe na área de conteúdo do app em todo o projeto não atravessa nenhuma das duas.

## Alternativas consideradas

### Divisor desenhado, como linha de 1px sobre `#23272f`

É o que o canvas desenha e o que a §2.7 descrevia como `[v2]`. Recusada pelo dono do produto, e a razão é a mesma que já tirou a borda do topo da barra de status (§2.8) e a borda inferior da barra de abas (§4.4): entre dois quadros que já têm sombra e fundo próprio, uma linha é a **terceira** separação no mesmo lugar. O vão já separa, e ele separa com a medida que o olho do usuário já aprendeu na margem da janela.

### Cabeçalho por painel, com título, ponto de foco e botões

Também do canvas: `padding: 7px 12px`, ponto de 6×6 na cor do grupo, título mono truncado, botões de dividir e fechar. Recusada pelo dono do produto. O custo decisivo não é visual: um cabeçalho de ~26px **por painel** sai da grade, e numa aba com quatro painéis são quatro cabeçalhos comendo linhas de terminal para repetir o que a barra de status já diz do focado. O que ele resolveria — saber qual painel tem o foco — o cursor resolve sem tirar linha de ninguém. O desenho continua registrado na §4.3 da especificação visual, como elemento do canvas deliberadamente não construído.

### Foco marcado por borda na cor do grupo

Era a recomendação levada ao dono do produto, e é a mais legível com muitos painéis: 1px na cor cheia do grupo no quadro focado. Recusada por ele em favor do cursor. Ela reintroduziria no quadro do terminal a distinção de cor que o produto já faz na trilha, e o pedido era que um painel fosse **indistinguível** de um terminal inteiro.

### Esmaecer os painéis sem foco

Resolve o caso que o cursor não resolve (TUI que esconde o cursor) e é barato — `chrome::brighten` já existe em CPU. Recusada porque o caso de uso do recurso é **ler os dois ao mesmo tempo**: esmaecer o log que se quer acompanhar enquanto se digita no outro painel piora exatamente a situação que justifica o produto.

### Árvore n-ária, com lista de filhos por eixo

Mais próxima de como um layout de janelas costuma ser modelado, e evita a cascata de nós `Split` quando se divide três vezes no mesmo eixo. Recusada pela §1: o arraste de um divisor vira redistribuição entre N irmãos, com regra de quem cede, e o gesto do produto nunca produz mais de dois de uma vez. A cascata é custo de representação, não de comportamento.

### Arena plana de painéis, com retângulos absolutos

Cada painel guardaria o próprio retângulo, sem árvore. Recusada porque o resize de janela deixa de ter resposta: sem a estrutura que diz quem é irmão de quem, "preservar as proporções" (RF-6.15) não é definível, e o layout precisaria ser reconstruído por heurística de adjacência a cada mudança de tamanho.

### Redimensionar o PTY só ao soltar o divisor

Menos chamadas de `resize`, e evita que uma TUI se redesenhe a cada quadro do arraste. Recusada pela §8: durante todo o gesto o conteúdo ficaria com a grade velha dentro do quadro novo, e o usuário estaria arrastando às cegas. O custo que ela evita já está resolvido — `Terminal::resize` é barato em rajada, e a thread de observação sempre aplica o último par.

### `Ctrl+Shift+V` para dividir, movendo `clipboard.paste`

Foi o pedido original. Recusada na verificação: `Ctrl+Shift+V` é a convenção universal de colar em terminal no Windows e no Linux, está nos defaults embutidos, no arquivo de exemplo, na tabela do ADR-0008 e no guia do usuário. Mover um default estabelecido para acomodar recurso novo é o mesmo negócio errado que o ADR-0008 já recusou no macOS ao escolher `f3` em vez de `cmd+g` para a busca.

### `Ctrl+Alt+H` e `Ctrl+Alt+V`

Livres, simétricos e mnemônicos. Recusada pelo dono do produto depois de ver os dois custos: `Ctrl+Alt` é o AltGr sintetizado do Windows, e a tabela de keybindings é consultada **antes** do terminal ([ADR-0008](0008-teclas-e-roteamento-de-input.md)), então num layout em que AltGr+letra produz caractere o app engoliria a digitação; e `Alt+Ctrl+H` é `backward-kill-word` no readline. Nenhum dos dois é fatal — `"none"` devolve a tecla —, mas os dois são atrito por padrão, e `Ctrl+Shift+H`/`Ctrl+Shift+D` não têm nenhum.

### Atalho único que alterna a orientação

Um `pane.split` só, decidindo o eixo pela forma do painel (divide no lado mais longo). É o que alguns gerenciadores de janela fazem. Recusada porque torna o resultado do gesto imprevisível a partir do gesto: a mesma tecla produziria layouts diferentes conforme o tamanho da janela, e o usuário não teria como pedir o que quer.

### `pane.close` com atalho default

Foi oferecido `Ctrl+Shift+X`, e também reaproveitar `Ctrl+Shift+W` para fechar o painel. Recusadas pelo dono do produto: a ação entra no catálogo **vinculável e sem default**, como `group.new_tab` e `group.close_all`. `exit` no shell já fecha o painel (RF-6.11), e `Ctrl+Shift+W` continua significando "fecha esta aba inteira", que é o que ele sempre significou.

### Zoom temporário de painel nesta entrega

Conveniência clássica de multiplexador. Deixada de fora pelo dono do produto ao escolher o escopo. Nada nesta decisão a impede: um painel em zoom é a mesma árvore com um retângulo diferente.

## Consequências

### Positivas

- O produto deixa de ter uma resposta pior que a do `tmux` para "dois terminais, um contexto", que era a lacuna mais visível do v1.
- **Zero valor de aparência novo, zero cor nova, zero primitiva nova, zero dependência nova.** O recurso nasce inteiro dentro da linguagem visual que o [ADR-0028](0028-o-binario-como-referencia-visual.md) fixou.
- `porecatu-term` e `porecatu-render` não mudam. A fronteira que o [ADR-0018](0018-composicao-de-frame.md) desenhou paga aqui pela primeira vez em escala.
- O layout puro de painéis torna testável sem janela o que seria a parte mais frágil do recurso: aninhamento, proporção, mínimo e hit-test.
- A profecia do [ADR-0006](0006-modelo-de-abas-e-grupos.md) se confirma: `Tab` ser struct própria desde o primeiro dia fez a mudança ser localizada, e nenhuma decisão anterior precisou ser revogada.

### Negativas

- **A mais desconfortável:** seis campos saem de `Tab`, e tudo que os lê muda junto — título, indicadores, barra de status, sessão, `.porecatu`, convite de shell. É a maior refatoração de `porecatu-core` desde a F3, e ela não tem versão pequena.
- Uma aba passa a ter N terminais, e portanto **3N threads** ([ADR-0007](0007-modelo-de-threading.md)). A regra não muda; a conta sim, e quem abrir dezesseis painéis paga por dezesseis.
- O painel sem foco rodando TUI que esconde o cursor **não tem marca de foco**. É consequência direta da §4, aceita pelo dono do produto.
- O catálogo fechado ganha sete ações de uma vez — o maior acréscimo desde que ele foi fechado na F4.
- A sessão gravada por esta versão, lida por uma anterior, perde o layout dividido (devolve uma aba por painel raiz). É perda silenciosa, não corrupção, e é o preço de não subir `schema_version`.
- `zoom_scope` e a métrica de célula por processo continuam como estavam, e agora com um motivo a mais para incomodar: painel também não tem zoom próprio.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| A refatoração de `Tab` quebrar comportamento que nenhum teste cobre (título, indicadores) | **Alta** | Médio | Etapa 2 é só modelo, com os invariantes e a derivação de título/atividade em teste antes de qualquer pixel |
| `Wakeup` sem painel passar despercebido e redesenhar o painel errado | Média | Médio | Mesmo precedente do [ADR-0015](0015-multiplas-janelas.md); o campo entra no evento, não numa busca por aba |
| Arraste do divisor brigar com a borda de resize da janela | Média | Médio | A precedência muda **nos dois caminhos ao mesmo tempo** (cursor e clique), molde de `over_window_button`; teste de geometria contra `titlebar::resize_direction_at` |
| Duplo clique contar entre painéis diferentes (`ClickTracker` é por janela) | Média | Baixo | O rastreador passa a guardar o painel do clique anterior; teste com dois cliques rápidos em painéis distintos |
| `.porecatu` rodar N vezes numa aba dividida | Baixa | **Alto** | §12: uma vez por aba, no painel focado; teste com duas folhas no mesmo diretório autorizado |
| Split produzir painel inútil em janela pequena | Média | Baixo | RF-6.4 recusa antes de dividir, com aviso |
| Confirmação de fechamento contar painéis onde contava abas, sem ninguém notar | Média | Médio | A dívida já aconteceu uma vez na F5 (`request_close_window` contando `tabs.len()`); vai explícito em teste |
| Reencaixe de PTY a cada quadro do arraste pesar em TUI grande | Baixa | Baixo | Alternativa (resize só no release) registrada e reversível sem mudar o modelo |
| A ausência de marca de foco em TUI gerar relato de bug | Média | Baixo | Decisão registrada aqui e no RF-6.9; reversível por aval, e as duas alternativas já estão levantadas |
