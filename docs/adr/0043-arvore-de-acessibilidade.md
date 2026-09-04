# ADR-0043 — Árvore de acessibilidade: `accesskit` sobre o chrome, grade fora do v1

**Status:** Aceito
**Data:** 2026-09-04
**Relacionados:** ADR-0001, ADR-0007, ADR-0008, ADR-0011, ADR-0014, ADR-0018, ADR-0019, ADR-0023, ADR-0035, PRD-011

## Contexto

O [ADR-0001](0001-stack-de-gui.md) comprou render próprio sobre GPU com uma consequência declarada na mesma frase: *"acessibilidade é trabalho explícito. Leitores de tela não enxergam pixels de GPU. Mitigação: integrar `accesskit` (que já conversa com `winit`) numa fase posterior — registrado no roadmap como dívida consciente, não esquecimento."* A fase posterior é esta.

A dívida **cresceu duas vezes** desde então, e as duas foram registradas na hora justamente para que a F6 não descobrisse o crescimento de surpresa:

- o [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) somou três papéis — *"diálogo modal, menu e notificação são justamente os papéis que leitor de tela trata de forma especial"*;
- o [ADR-0019](0019-tooltip.md) somou o quarto, com a ressalva de que *"tooltip é o menos padronizado dos quatro entre leitores de tela"*;
- e o [ADR-0023](0023-editor-de-grupo.md) trouxe o quinto sem que ninguém atualizasse a conta. **São cinco widgets, não três** — o texto da F6 no roadmap ainda diz três, e é erro factual a corrigir.

Três coisas estavam abertas.

**A primeira é de acoplamento de versão.** `accesskit_winit` é adaptador de uma versão específica de `winit`, e o `winit` deste projeto é pinado. A F0 já perdeu tempo com exatamente esse tipo de cadeia (`glyphon → wgpu → raw-window-handle → winit`) e deixou a lição escrita: fixe o crate acoplado primeiro e aceite o que ele exige, em vez de escolher os dois de forma independente.

**A segunda é o modelo de render.** O [ADR-0007](0007-modelo-de-threading.md) decidiu que terminal ocioso não gera frame, e o [ADR-0022](0022-animacao-de-interface.md) precisou de um ADR só para abrir exceção a isso. Uma árvore de acessibilidade reconstruída por frame — ou, pior, que peça frame para se atualizar — desfaz a premissa que sustenta os ~0% de CPU ociosa.

**A terceira é escopo, e é a pergunta de produto.** Num emulador de terminal, o conteúdo **é** a grade. Expor só o chrome é honesto ou é fingir?

## Decisão

**`accesskit_winit` expõe o chrome, a árvore é uma projeção das funções puras de layout que já existem, e a grade do terminal fica declaradamente fora do v1.**

### 1. `accesskit_winit`, com a versão ditada pelo `winit` já pinado

O adaptador é `accesskit_winit`, na versão que casa com o `winit 0.30.13` do projeto — a disciplina da F0, aplicada de novo. **Verificado antes de escrever este ADR**, não presumido: `accesskit_winit 0.34.0` resolve contra o mesmo `winit 0.30.13`, sem duplicar `winit` na árvore de dependências.

Licenças conferidas, todas compatíveis com GPLv3: `accesskit` e `accesskit_windows` são `MIT OR Apache-2.0`, `accesskit_winit` é `Apache-2.0`.

O adaptador cobre as três plataformas — UI Automation no Windows, `NSAccessibility` no macOS, AT-SPI no Linux. No Linux o suporte vem por D-Bus e traz `zbus`/`zvariant` para a árvore; é a maior soma de dependências transitivas de uma feature do v1, e está registrada como consequência, não descoberta.

### 2. A árvore é projeção do layout, não uma segunda fonte de verdade

`porecatu-ui` monta os nós **a partir das mesmas funções puras que produzem o desenho** — `tab_bar::layout` e as funções de geometria dos widgets, que já devolvem retângulo, rótulo e estado de cada elemento e já são testadas sem GPU e sem janela.

Isto é a decisão estrutural, e é o que impede o modo de falha clássico: uma árvore construída à parte diverge do que está na tela no primeiro PR que mexer na barra, e ninguém percebe, porque nenhum teste de layout olha para ela. Sendo projeção, mudar a barra muda a árvore por construção.

`porecatu-render` **não participa**. Ele não conhece o domínio — recebe quad, retângulo arredondado e run de texto — e por isso não tem como nomear uma aba. A regra de dependência não abre exceção aqui.

### 3. Construção sob demanda, e sem pedir frame

O adaptador só constrói a árvore quando há **cliente de acessibilidade ativo**. Sem leitor de tela, o custo é zero e nada muda no modelo do ADR-0007.

Com cliente ativo, a atualização acontece **na volta do event loop que mudou o estado** — no mesmo ponto único onde `schedule_next_wake` já drena o que sujou —, nunca dentro do caminho de render e nunca como razão para redesenhar. Árvore e frame são consumidores independentes do mesmo estado. Terminal ocioso continua sem gerar frame, com ou sem leitor de tela.

### 4. O que a árvore expõe

**Barra de abas.** Cada aba é um nó com título, posição na ordem visual, o grupo a que pertence, e o estado que muda o que ela significa: ativa, com atividade (RF-1.20), com campainha (RF-1.21), não iniciada ([ADR-0037](0037-aba-nao-iniciada.md)). Pílula de grupo, botão de fechar da aba, botões de nova aba, indicador de overflow, botão de configurações e os botões de janela ([ADR-0027](0027-controles-de-janela-e-resize-proprios.md)) são nós com nome e ação.

**Os cinco widgets**, cada um com o papel que o leitor de tela trata de forma especial:

| Widget | Papel | Comportamento exigido |
|---|---|---|
| Diálogo (§2.15) | modal | anuncia-se ao abrir e **prende o foco** enquanto está aberto — o que a captura de teclado do ADR-0008 já garante em código |
| Aviso (§2.14) | notificação | anunciado ao aparecer, sem roubar o foco; o texto inclui a severidade |
| Menu de contexto (§2.16) | menu | itens com estado habilitado/esmaecido, e o item realçado é o foco |
| Editor de grupo (§2.10) | grupo com campo de texto | o campo expõe valor e seleção ([ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md)) |
| Tooltip (§2.20) | dica | associada ao elemento que a disparou, **não** como nó navegável |

O tooltip é o caso que o ADR-0019 avisou ser o menos padronizado. A regra aqui é a que menos depende de comportamento de leitor: ele é **descrição do elemento de origem**, não um nó por si. O ADR-0019 já havia decidido que rótulo de botão de ícone é assunto de `accesskit` e não de tooltip — o que essa modelagem entrega de graça.

**Barra de busca** (§2.21, [ADR-0041](0041-busca-no-scrollback.md)): campo de texto com valor, mais o contador como descrição. Nasce acessível, em vez de virar dívida na semana seguinte.

### 5. A grade do terminal fica fora do v1 — declarado, com o porquê

`porecatu-render` desenha a grade e `accesskit` não a vê. **No v1 continua assim**, e isto é limitação registrada, não esquecimento. As três perguntas que precisariam de resposta antes de uma linha de código:

- **Granularidade.** Linha, tela, scrollback inteiro? Um `cargo build` gera centenas de linhas; expô-las como um bloco de texto único e refazê-lo a cada byte é inútil para navegação.
- **Quando anunciar.** Saída de terminal chega em rajada. Anunciar tudo inunda o leitor e torna o app inutilizável; anunciar nada faz a exposição não servir para acompanhar o que está acontecendo. A resposta certa é alguma forma de região dinâmica com política de coalescência — e coalescência é exatamente o que o ADR-0007 já faz para frame, por outra razão e com outro critério.
- **Cursor e posição.** O cursor do terminal precisa mapear para posição de texto na árvore, senão editar uma linha de comando com leitor de tela não funciona.

Nenhuma dessas é difícil isoladamente; juntas são **trabalho de tamanho próprio**, com decisão de produto no meio. Fica como dívida nomeada, no mesmo formato da dívida de verificação interativa: escrita, com o motivo, e não como tarefa que impede a fase.

O que o v1 entrega é o que a exposição do chrome resolve de fato: descobrir quantas abas existem, onde se está, o que cada grupo agrupa, e não perder um diálogo modal que apareceu. Sem isso, um usuário de leitor de tela não consegue **nem começar**.

### 6. Sem travessia por `Tab`

Não entra navegação de chrome por `Tab`/`Shift+Tab`. Leitor de tela navega a árvore pelos mecanismos dele — cursor virtual, navegação por objeto —, e é para isso que a árvore existe.

A razão de não somar a travessia é concreta: `Tab` num terminal pertence ao shell, onde é completação. Capturar `Tab` no nível do app para andar pelo chrome quebraria autocompletar em toda aba, e o ADR-0008 é explícito sobre o que o app pode reivindicar. Dentro do editor de grupo, `Tab` já percorre as três regiões — mas ali existe um modo de captura modal, que é justamente o que a barra de abas não tem.

## Alternativas consideradas

### Escrever a integração de acessibilidade à mão, por plataforma

Controle total e nenhuma dependência nova. Rejeitada por três motivos que se somam: exigiria UI Automation, `NSAccessibility` e AT-SPI escritos separadamente, os três com FFI — e portanto `unsafe`, que o workspace nega e que nunca foi excepcionado; e o ADR-0001 já nomeou `accesskit` como a mitigação, justamente porque ele existe para resolver este problema em app de render próprio.

### Construir a árvore como estrutura própria, ao lado do layout

Daria liberdade para modelar a árvore sem amarrar a geometria. Rejeitada porque cria uma segunda fonte de verdade sobre o mesmo estado, e a lição registrada deste projeto é direta: *"fórmula de geometria copiada em dois lugares só diverge quando alguém mexe nela"* — a altura da barra já custou um bug por isso. Árvore que diverge do desenho é pior que árvore ausente: ela mente.

### Manter a árvore sempre construída, com leitor de tela ou sem

Simplifica o código: um caminho só, sem estado de ativação. Rejeitada pelo ADR-0007 — trabalho por evento para ninguém é exatamente o que o modelo damage-driven existe para não fazer, e a métrica de CPU ociosa em ~0% é do PRD-000.

### Expor a grade agora, como bloco de texto

É o que faria o app "acessível" no sentido de marcar a caixa. Rejeitada porque um bloco de texto de milhares de linhas, refeito a cada rajada do PTY, é pior que nada: o leitor fala sem parar e o usuário não consegue navegar. Exposição de conteúdo de terminal precisa das três decisões da §5, e tomá-las no apagar das luzes de uma fase de polimento é como se erra.

### Fechar o escopo em "só a barra de abas", como o critério de saída dizia literalmente

Era o texto do critério da F6 e seria menos trabalho. Rejeitada porque deixaria de fora exatamente os cinco papéis que os ADR-0014, 0019 e 0023 registraram como o motivo de a dívida ter crescido — inclusive o **diálogo modal**, cuja invisibilidade significa que o app pede uma confirmação que o usuário não percebe e o teclado para de responder sem explicação. O critério de saída é emendado, não obedecido ao pé da letra.

### Somar travessia de chrome por `Tab`

Padrão de aplicação de desktop, e ajudaria também quem navega só por teclado. Rejeitada porque `Tab` é do shell: capturá-lo no app quebra completação em toda aba. Se a navegação de chrome por teclado for pedida algum dia, ela precisa de um gesto próprio e de um ADR — não de tomar a tecla mais usada de um terminal.

## Consequências

### Positivas

- A dívida mais antiga do projeto — registrada no ADR-0001, antes da F1 — sai do papel.
- A árvore não pode divergir do desenho, porque é projeção das mesmas funções puras.
- Custo zero sem leitor de tela ativo: o modelo do ADR-0007 e a métrica de CPU ociosa ficam intactos.
- O tooltip como descrição, e não como nó, entrega a decisão que o ADR-0019 já havia antecipado sobre rótulo de botão de ícone.
- A barra de busca do ADR-0041 nasce acessível.
- Nenhum `unsafe`: a regra do workspace segue sem exceção, pelo mesmo caminho de `portable-pty`, `arboard` e `win32job`.

### Negativas

- **A grade fica inacessível no v1.** Para o usuário-alvo de leitor de tela, o app fica navegável mas não legível — e é a limitação mais séria que o v1 assume.
- No Linux, `zbus`/`zvariant` entram na árvore de dependências. É a maior soma transitiva de uma feature do v1.
- O critério de saída da F6 é emendado: de "leitor de tela navega a barra de abas" para a barra **e** os cinco widgets.
- Verificação real depende de leitor de tela em três plataformas, e este fluxo tem uma.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Verificação com leitor de tela só acontecer no Windows | **Alta** | Médio | Registrada como dívida da fase, no formato da dívida de verificação interativa. NVDA no Windows é gratuito e roda sem input sintético — a única metade verificável, e ela é verificável de verdade |
| Subir `winit` no futuro exigir subir `accesskit_winit` no mesmo PR | Alta | Baixo | É a mesma disciplina de `wgpu`: crate acoplado sobe como tarefa própria, nunca como efeito colateral. Registrado nas armadilhas |
| Árvore atualizada dentro do caminho de render por descuido | Média | Médio | O ponto de atualização é único e nomeado, no mesmo lugar de `schedule_next_wake`; teste de que nenhum frame é solicitado por mudança de árvore |
| Tooltip modelado de forma que algum leitor ignore | Média | Baixo | É descrição do elemento de origem, a modelagem que menos depende de comportamento de leitor — e o ADR-0019 já avisou do risco |
| `accesskit_unix` pesar no tempo de build do CI | Média | Baixo | Cache de build já existe no CI; se pesar, a feature entra atrás de `cfg` por plataforma |
