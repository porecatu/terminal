# ADR-0091 — Arraste entre janelas: ponto global pela captura implícita, alvo pela ordem de foco, fantasma que salta de barra, destacar numa janela nova e arraste da seleção

**Status:** Aceito
**Data:** 2026-11-07
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0013](0013-mouse-selecao-e-clipboard.md), [ADR-0015](0015-multiplas-janelas.md), [ADR-0021](0021-selecao-multipla-e-gestos-da-barra.md), [ADR-0022](0022-animacao-de-interface.md), [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0059](0059-janela-de-configuracoes.md), [ADR-0064](0064-janela-de-configuracoes-modal.md), [ADR-0081](0081-disparo-no-release-para-botoes-e-itens.md), [ADR-0087](0087-zona-de-abas-fixadas-na-barra.md), [ADR-0089](0089-ranhura-de-soltura-com-o-botao-do-app-oculto.md), [ADR-0090](0090-transferencia-de-abas-entre-janelas.md), [PRD-022](../prd/prd-022-abas-e-grupos-entre-janelas.md)
**Supersedes (parcial):** ADR-0021 §1 e §4 (o press numa aba selecionada não limpa a seleção; o arraste leva a seleção; a barra de outra janela é alvo) · ADR-0087 §5 (a zona de fixadas de outra janela é alvo)

## Contexto

O [ADR-0090](0090-transferencia-de-abas-entre-janelas.md) decide **como** uma aba muda de janela. Este decide o **gesto**: como o app sabe, com o botão do mouse apertado, que o cursor está sobre a barra de outra janela, o que desenhar enquanto isso, e o que soltar em cada lugar faz. O [PRD-022](../prd/prd-022-abas-e-grupos-entre-janelas.md) fixou o comportamento com o dono. O ADR-0015 tinha registrado o motivo de deixar isto para depois: *"o custo real não é o `move_tab` — é a UI de arraste atravessando fronteira de janela, com drop em janela não focada e feedback visual entre superfícies distintas"*. É esse custo que este ADR paga.

O arraste de hoje é inteiro de uma janela:

- **O estado** é o enum `Drag` (`Idle`, `TabPressed`, `TabDragging`, `GroupPressed`, `GroupDragging` e os do divisor de painel), um por `WindowState`.
- **O fantasma** tem o Y fixo na fileira da barra (`chrome::DragGhost`, `y: tab.rect.y`) e segue o cursor só no X.
- **O alvo** é resolvido a cada redesenho contra o layout da própria barra (`tab_bar::tab_drag_target`, `drag_target`, `group_drag_target_index`), e o preview é um `Workspace` clonado com a operação aplicada.
- **A soltura** (`WindowState::finish_drag`) aceita o alvo se `in_bar(cursor_y)`, que só testa `y < altura da barra`. Não há limite inferior nem teste de X, de modo que soltar **acima** da janela conta como "na barra".
- **Nada** trata `CursorLeft`, posição global do cursor ou captura de cursor. `Focused(false)` zera o botão apertado, mas não o `drag`.
- **O arraste leva uma aba só.** `Ctrl`/`Shift`+clique só mexem na seleção, e o clique simples a limpa **no press**.

Seis perguntas ficam abertas, e cada uma tem uma resposta que funciona numa plataforma e falha em outra:

1. **Como saber onde está o cursor fora da janela de origem.** O `winit` não expõe posição global do cursor nem ordem Z das janelas.
2. **Qual janela é o alvo** quando duas se sobrepõem.
3. **Quem desenha o quê**, quando o fantasma está numa janela e o buraco em outra.
4. **O que "fora de toda janela" significa**, para o destacar.
5. **Onde a janela destacada nasce.**
6. **Como o arraste da seleção convive com o RF-2.3**, que limpa a seleção no clique.

## Decisão

**Durante o arraste, a janela de origem converte cada posição do cursor num ponto global de tela, somando a posição da área cliente dela. O cursor continua chegando com o botão apertado pela captura implícita que as três plataformas de desktop dão. O ponto é testado contra as barras das outras janelas de terminal, na ordem de foco mais recente. O estado do gesto passa a ser do `App`. A janela de destino desenha fantasma, vão e realce. Soltar fora de toda janela destaca numa janela nova, com o ponto agarrado sob o cursor. Pressionar uma aba selecionada arma o arraste da seleção sem limpá-la. No Wayland o gesto não atravessa janelas.**

### 1. Ponto global

```
global = inner_position(origem) + cursor_position(origem)      // físico
```

`cursor_position` é a coordenada física que `WindowEvent::CursorMoved` já entrega e que `WindowState` já guarda. `inner_position()` é a posição física da área cliente na tela, que o app já lê para a cascata de `window.new` e a geometria da sessão. A soma é a coordenada física do cursor na tela, sem nenhuma chamada nova ao sistema e sem `unsafe` ([CLAUDE.md](../../CLAUDE.md): `unsafe_code = "deny"`).

**O que faz o `CursorMoved` continuar chegando fora da janela** é a captura implícita do botão apertado:

- **Windows:** o `winit` chama `SetCapture` no press de botão e `ReleaseCapture` no release. Enquanto o botão está apertado, `WM_MOUSEMOVE` vai para a janela de origem, com coordenadas fora da área cliente, inclusive negativas.
- **X11:** o press de botão abre um *grab* implícito do ponteiro, e os eventos de movimento seguem para a janela de origem até o release.
- **macOS:** `mouseDragged:` continua sendo entregue à view de origem fora dos limites dela, e o `winit` o repassa como `CursorMoved` enquanto há botão apertado.

Esta é a premissa mais frágil do ADR, porque depende de comportamento do `winit` que a documentação dele não promete. Por isso ela é a **primeira** coisa verificada na implementação (Riscos) e fica isolada numa função, `drag_global_point(&WindowState) -> Option<PhysicalPosition>`, que devolve `None` onde não há como saber. `None` faz o gesto se comportar como hoje: só a barra de origem é alvo.

**Para cada janela candidata**, o ponto global é convertido para a coordenada lógica **dela**: subtrai-se o `inner_position` dela e divide-se pelo `scale_factor` dela. Com dois monitores de escala diferente ([ADR-0015](0015-multiplas-janelas.md), "Riscos"), o que vale para testar o ponto contra a barra de B é a escala de B, nunca a de A.

### 2. Alvo: a janela de foco mais recente sob o ponto

O `winit` não diz qual janela está por cima. Para os casos que importam, a ordem de **foco mais recente** é uma boa aproximação: a janela que o usuário usou por último é a que está por cima, a menos que ele a tenha movido para baixo de outra sem focar a outra, o que é raro. `App` passa a manter essa ordem:

```rust
struct App {
    focus_order: Vec<WindowId>,   // novo: mais recente primeiro; `Focused(true)` move para a frente
    // ...
}
```

A mesma ordem lista as janelas no popover "Mover para janela…" ([ADR-0090](0090-transferencia-de-abas-entre-janelas.md) §6).

**A busca do alvo**, a cada `CursorMoved` com arraste ativo:

1. Percorre `focus_order` e pula a origem, as minimizadas (`is_minimized() == Some(true)`) e, durante este passo, as janelas fora de `App.windows` (configurações e editor de perfil).
2. A **primeira** janela cuja área cliente contém o ponto é a janela sob o cursor. Se o ponto está na **faixa da barra** dela, entre `0` e `chrome::bar_height(style)` na coordenada lógica dela e dentro da largura, ela é a **janela-alvo**, e o alvo dentro dela é resolvido pelas funções de sempre (`tab_drag_target`, `group_drag_target_index`) contra o layout **dela**.
3. Se nenhuma outra janela contém o ponto, vale a barra da origem, como hoje.

**O que não é visto:** a janela de **outro app** por cima de uma janela do Porecatu. Soltar sobre ela, num ponto que cai dentro da área de uma janela do Porecatu por baixo, é tratado como soltar na janela do Porecatu. Isso é uma barra sob outro app virando destino, ou, fora da barra, um cancelamento. É o caso residual de não haver ordem Z, e o efeito nunca é destrutivo: o processo não morre, e o mesmo gesto o desfaz. Aceito e registrado.

### 3. O estado do gesto passa a ser do `App`

A janela de origem continua dona do `Drag`, porque é ela que recebe os eventos de mouse. O que muda é que o `Drag` passa a descrever **o que** está sendo arrastado, e não só qual aba:

```rust
enum DragPayload {
    Tabs { grabbed: TabId, all: Vec<TabId> },   // `all` na ordem visual; `[grabbed]` sem seleção
    Group(GroupId),
    Pinned(TabId),
}
```

`App` ganha o que atravessa janelas:

```rust
struct CrossDrag {
    origin: WindowId,
    target: Option<(WindowId, DragDrop)>,   // `None`: o alvo é a barra da origem, ou nenhum
    ghost: GhostContent,                    // o que desenhar em outra janela (abaixo)
}
```

`GhostContent` é o que a janela de destino precisa para desenhar um fantasma de uma aba que **não está** no workspace dela: rótulo, cor do grupo (aba de grupo), forma de origem (aba de trilha, caixa de fixada, pílula de grupo), largura e o `grab_offset`. É montado uma vez, quando o limiar de 4 px promove o press a arraste, com os mesmos dados que o fantasma local já usa (`chrome::DragGhost`, `GroupDragGhost`).

**Quem desenha o quê:**

| Onde o cursor está | Barra de origem | Barra de destino |
|---|---|---|
| Barra da origem | fantasma, vão e realce, como hoje | nada |
| Barra de outra janela | **buraco** onde a aba estava, sem fantasma, sem vão | **fantasma, vão e realce**, pelo mesmo preview de `Workspace` clonado, agora o do destino com as abas inseridas |
| Fora de toda barra | fantasma preso à barra, seguindo o X, como hoje (RF-22.9) | nada |

A janela de destino recebe `incoming: Option<IncomingDrag>` em `WindowState` (o alvo e o `GhostContent`), definido e limpo pelo `App` a cada mudança de alvo. As duas janelas envolvidas são marcadas sujas a cada mudança. Uma janela que deixa de ser alvo é marcada suja uma última vez, para apagar o fantasma. Não há janela flutuante nem desenho fora das superfícies que já existem (decisão do dono, RF-22.9).

**Auto-scroll no destino.** O mesmo da origem: com o ponto a menos de 30 px de uma ponta da trilha **do destino**, a trilha do destino rola um passo por `CursorMoved`, como hoje (especificação §2.19, "decidido não fazer" o intervalo).

**O fantasma de origem mantém a forma** quando salta de barra, pela mesma razão da §5 do [ADR-0087](0087-zona-de-abas-fixadas-na-barra.md): uma caixa de fixada é desenhada como caixa sobre a trilha do destino, e uma aba como aba sobre a zona do destino.

### 4. Soltura: quatro casos, decididos pelo ponto global

`finish_drag` deixa de perguntar `in_bar(y)`. Ele pergunta **onde está o ponto**, nesta ordem:

| Onde | Efeito |
|---|---|
| 1. Na barra da **origem** (X e Y dentro dela) | soltura local, a de hoje (`drop_op`, `apply_zone_drop`), agora também para a seleção (§6) |
| 2. Na barra de **outra janela** (§2) | `App::transfer(origem, payload, Window(destino, alvo))` ([ADR-0090](0090-transferencia-de-abas-entre-janelas.md) §3) |
| 3. Dentro da área cliente de **qualquer janela do Porecatu** (origem, outra de terminal, configurações, editor de perfil), fora da barra | **cancela** (RF-22.13) |
| 4. **Fora de todas** | `App::transfer(origem, payload, NewWindow(colocação))`: destacar (RF-22.12) |

O caso 1 corrige o defeito de hoje: soltar **acima** da janela de origem, que o `in_bar` sem limite inferior aceitava como na barra, passa a ser o caso 4.

O caso 3 inclui a janela de configurações por segurança, embora o arraste não comece com ela aberta no Windows, onde as janelas de terminal ficam desabilitadas enquanto ela está aberta ([ADR-0064](0064-janela-de-configuracoes-modal.md)). As áreas dela e do editor de perfil vêm de `inner_position`/`inner_size` deles, sem entrar em `App.windows`.

**Sem ponto global** (`drag_global_point` devolve `None`), só o caso 1 existe, e todo o resto cancela: é o comportamento de hoje.

### 5. Destacar: onde a janela nasce

A janela nova ([ADR-0090](0090-transferencia-de-abas-entre-janelas.md) §3, passo 2):

- **Tamanho:** o `inner_size` da origem quando a origem está normal, ou o tamanho restaurado quando está maximizada ou em tela cheia (RF-22.14). A janela nova nasce sempre normal.
- **Posição:** de modo que o **ponto agarrado fique sob o cursor**. A aba ou caixa que chega é a primeira da trilha, ou a primeira da zona, ou o grupo na primeira posição, e a posição dela na barra da janela nova é conhecida pelo layout antes de a janela existir (`tab_bar::layout` sobre o workspace de destino, que é puro). Assim:

  ```
  posição_cliente = global − (x_do_elemento_na_barra_nova + grab_offset, y_do_press_na_barra) × escala
  ```

  Escala é a do monitor sob o cursor (`ActiveEventLoop::available_monitors`, o que contém o ponto), e a soma é **presa à área do monitor**. O `winit` não dá a área útil sem a barra de tarefas, então a regra é que **a faixa da barra da janela nova fique inteira dentro do monitor**. É o critério que importa, porque é por ela que o usuário pega a janela de novo.
- **Sem foco roubado de ninguém:** o app já é o processo em primeiro plano, porque o usuário estava arrastando nele, e trazer a janela nova à frente não esbarra na proteção de foco do Windows.

Pelo popover, "Nova janela" usa a colocação em cascata de `window.new` (RF-22.22), não esta.

### 6. Arraste da seleção

Revendo o [ADR-0021](0021-selecao-multipla-e-gestos-da-barra.md) §1 e §4:

- **Press sem modificador numa aba selecionada**, com mais de uma selecionada, **arma** o arraste da seleção (`Drag::TabPressed` com o payload `Tabs { grabbed, all }`) e **não limpa** a seleção nem ativa a aba.
- **Release sem passar do limiar de 4 px** faz o que o RF-2.3 manda: limpa a seleção e ativa a aba. É o mesmo princípio de "o press arma, o release age" do [ADR-0081](0081-disparo-no-release-para-botoes-e-itens.md), aplicado aqui porque o press passou a ter dois destinos possíveis.
- **Press numa aba não selecionada** continua como hoje: limpa no press e arma o arraste só daquela aba.
- **Passado o limiar**, o arraste leva `all`. O fantasma é a aba `grabbed`. As outras selecionadas viram **buraco** na origem: o preview as retira, e as vizinhas deslizam pela animação de sempre. No destino, local ou em outra janela, o vão abre para o bloco inteiro, porque o preview insere `all` no alvo.
- **A operação local** da seleção é `Workspace::move_tabs(ids, at)` ([ADR-0090](0090-transferencia-de-abas-entre-janelas.md) §1), uma só, em vez de N chamadas de `move_tab_to_group`. Uma única operação devolve uma única ordem, e o teste da ordem resultante cobre o bloco. Dentro do mesmo workspace os `TabId` são preservados: não há colisão possível, e nenhum runtime precisa ser re-chaveado.
- **Fixadas** não entram em seleção (RF-21.3), e o arraste da seleção nunca as leva. Uma seleção solta na **zona** fixa todas, na ordem, na posição do vão. É a mesma regra de "arrastar para a zona fixa" do RF-21.7, aplicada ao bloco.
- **Depois da soltura** a seleção é limpa, e `grabbed` fica ativa onde caiu (RF-22.10).

### 7. Cancelar

O arraste, local ou entre janelas, é cancelado por:

- **`Esc`**, como hoje. O teclado continua indo para a origem, que mantém o foco durante o gesto;
- **`Focused(false)` da origem**, que passa a zerar o `drag`, além do botão apertado. Com o botão preso numa captura que o sistema tirou (`Alt+Tab`, uma notificação que rouba o foco), o release pode nunca chegar, e um arraste vivo sem botão apertado é estado preso;
- **release** nos casos 3 e 4 sem efeito válido: payload vazio, ou `transfer` recusando por `None`.

Cancelar limpa o `incoming` de qualquer janela que o tinha, e a marca suja.

### 8. Wayland

O protocolo do Wayland **não informa** ao cliente a posição das janelas dele na tela (`inner_position` devolve erro) nem a posição do cursor fora da superfície que tem o *grab*. O único jeito de arrastar entre superfícies é o protocolo de *drag-and-drop* do compositor (`wl_data_device`), que o `winit` não expõe para iniciar um arraste. Escrevê-lo por fora exigiria um cliente Wayland próprio ao lado do `winit`, uma dependência e uma fronteira nova, para uma plataforma que o projeto ainda não verifica ao vivo.

Por isso, no Wayland (`ActiveEventLoopExtWayland::is_wayland()`), `drag_global_point` devolve `None`, e o §4 cai no comportamento de hoje (RF-22.29). O popover "Mover para janela…" é o caminho completo lá, incluindo "Nova janela". O arraste da seleção dentro da janela (§6) funciona normalmente, porque não depende de ponto global.

## Alternativas consideradas

### Janela flutuante seguindo o cursor

Uma janela sem borda, transparente e sempre no topo, com o fantasma dentro, posicionada a cada movimento. É o que os navegadores fazem. Recusada pelo dono (RF-22.9). Teria também três custos que o fantasma que salta não tem: uma janela criada e destruída por gesto, com surface `wgpu` própria e o primeiro present que pode não chegar à tela (armadilha do CLAUDE.md); um `set_outer_position` por evento de mouse, que atrasa visivelmente em X11; e nenhum suporte no Wayland, onde o cliente não posiciona janelas.

### Ler a posição global do cursor do sistema (`GetCursorPos`, `NSEvent.mouseLocation`, `XQueryPointer`)

Não dependeria da captura implícita. Recusada porque são três chamadas de plataforma sem wrapper seguro no grafo de dependências de hoje. `unsafe_code = "deny"` vale sem exceção, e o projeto só aceita operação de SO por crate wrapper seguro. A soma de `inner_position` com a coordenada do evento dá o mesmo número nas três plataformas onde a captura implícita existe, que são as mesmas onde a chamada existiria. Se a verificação mostrar uma plataforma sem captura implícita, este ADR é revisto, e não contornado.

### `set_cursor_grab` explícito durante o arraste

Garantiria os eventos fora da janela. Recusada porque `CursorGrabMode::Confined` **prende** o cursor dentro da janela, o oposto do que se quer, e `Locked` o esconde. Nenhum dos modos do `winit` é "continue me mandando eventos lá fora".

### Ordem Z real, consultada ao sistema

Acertaria o caso de outro app por cima. Recusada pela mesma razão das chamadas de plataforma e porque o caso residual não é destrutivo. A ordem de foco é estado do próprio app, testável sem janela.

### Alvo em qualquer lugar da outra janela, não só na barra

Soltar no corpo de B mandaria a aba para o fim da trilha de B. É mais tolerante com a mira. Recusada pelo dono (RF-22.13): soltar dentro de uma janela do Porecatu fora da barra cancela. O corpo do terminal é onde um arraste acidental mais termina, e uma aba que some da janela de origem por um soltar sem querer é pior que um gesto que precisa mirar a barra.

### Manter a seleção limpa no press e arrastar só uma aba

Sem mudança no RF-2.3, e o arraste da seleção não existiria. Recusada pelo dono, que quis a seleção atravessando janelas. E, sem mudar o momento da limpeza, não há como arrastar o que o próprio press desfaz.

### No Wayland, destacar quando o cursor sai da superfície de origem

A superfície de origem sabe que o cursor saiu dela, pela coordenada fora dos limites. Recusada porque ela não sabe **onde** ele está: soltar sobre outra janela do Porecatu criaria uma janela nova em vez de mover para a que o usuário estava vendo. Um gesto que às vezes faz o contrário do que mostra é pior que um gesto que não existe, e o popover cobre o caso inteiro.

## Consequências

### Positivas

- O gesto atravessa janelas sem chamada nova ao sistema, sem `unsafe`, sem dependência e sem janela nova durante o arraste.
- O alvo dentro de uma barra continua sendo resolvido pelas mesmas funções puras, já testadas, agora chamadas com o layout de outra janela. As regras do ADR-0021 §4 e do ADR-0087 §5 valem sem uma linha nova.
- O defeito do `in_bar` sem limite inferior é corrigido como parte do caso 4.
- O arraste da seleção também serve dentro de uma janela só, e passa por uma única operação de core com ordem testada.

### Negativas

- O `Drag` sai do escopo de uma janela: `App` e a janela de destino passam a ter estado do gesto, e o fantasma passa a ser desenhado por uma janela que não tem a aba.
- O comportamento depende de captura implícita, não documentada como contrato do `winit`. Uma subida de `winit` pode quebrá-lo em silêncio, e isso só um teste ao vivo pega.
- A ordem de foco erra quando o usuário reordena janelas sem focá-las, e não enxerga janela de outro app por cima.
- No Wayland o recurso é só o popover. É uma diferença de plataforma que o usuário percebe.
- O RF-2.3 passa a acontecer no release para uma aba já selecionada, e no press para as outras. É a única assimetria do gesto, e é invisível num clique comum.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| `CursorMoved` não chegar fora da janela em alguma plataforma | Média | Alto | Primeira etapa da implementação: verificar ao vivo no Windows e registrar macOS e X11 como dívida; `drag_global_point` devolve `None` onde falhar, e o gesto degrada para o de hoje, nunca para um estado preso |
| Subida do `winit` mudar a captura implícita | Baixa | Alto | Item na lista de conferência da subida do `winit`, junto com o do `accesskit_winit` (CLAUDE.md, "Dependências") |
| Ponto convertido com a escala errada em DPI misto | Média | Médio | Conversão por janela, numa função pura testada com duas escalas |
| Fantasma ficar desenhado numa janela que deixou de ser alvo | Média | Baixo | `incoming` só é escrito por `App`, que marca suja a janela que perde o alvo; teste do estado puro de troca de alvo |
| Release perdido deixar o arraste vivo | Baixa | Médio | `Focused(false)` cancela (§7) |
| Janela destacada nascer com a barra fora da tela | Baixa | Médio | A posição é presa para a faixa da barra caber no monitor sob o cursor; teste puro da colocação |
| Clique simples numa aba selecionada parecer lento por agir no release | Baixa | Baixo | O release vem em milissegundos, e o mesmo padrão já vale para os botões ([ADR-0081](0081-disparo-no-release-para-botoes-e-itens.md)) |
