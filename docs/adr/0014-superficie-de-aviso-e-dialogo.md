# ADR-0014 — Superfície de aviso, diálogo e menu de contexto

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** ADR-0001, ADR-0003, ADR-0009, PRD-001, PRD-002, PRD-003, PRD-004, PRD-005

## Contexto

Oito requisitos aprovados dependem de o app **dizer algo ao usuário** ou **pedir confirmação**, e nenhum documento diz onde isso aparece nem que forma tem:

| Requisito | Precisa de |
|---|---|
| PRD-001 RF-1.6 | confirmação ao fechar aba com processo em primeiro plano |
| PRD-002 RF-2.23 | confirmação ao fechar grupo, exibindo a contagem — não desligável |
| PRD-003 RF-3.1 | convite à integração de shell, com snippet, uma vez, dispensável em definitivo |
| PRD-003 RF-3.10 | informar que o diretório gravado não existe mais |
| PRD-003 RF-3.14 | informar que o arquivo de sessão estava corrompido |
| PRD-003 RF-3.16 | informar que o schema é mais novo que o suportado |
| PRD-004 RF-4.21 | erro de config citando linha e chave |
| PRD-004 RF-4.22 | aviso de chave desconhecida |
| PRD-005 RF-5.8 | avisar qual família de fonte não foi encontrada |

Some-se a esses o aviso de fallback de GPU, previsto na tabela de riscos do [ADR-0001](0001-stack-de-gui.md) — *"detectar e avisar no primeiro start"*.

A [especificação visual](../design/especificacao-visual.md) lista apenas o RF-4.21 na seção 4.2 (requisitos do v1 sem desenho). Os outros oito estão numa lacuna entre duas listas: não têm desenho e também não constam como faltando. Pior, **RF-1.6 e RF-2.23 são cenários de aceite** das fases F2 e F3 — não há como declarar a F2 concluída sem um diálogo de confirmação existindo.

Há um segundo grupo, do mesmo tipo, igualmente sem casa: quatro requisitos pedem **menu de contexto** (RF-1.1 nova aba, RF-1.2 fechar aba, RF-2.20 mover aba para grupo, RF-2.22 menu do grupo com seis itens). O design mostra um popover de "editor de grupo" e um "menu de perfis" `[v2]`, mas nenhum menu de contexto — e a seção 4.2 também não o lista como ausente.

O [ADR-0001](0001-stack-de-gui.md) já avisou qual é o custo disso: *"todo o chrome é código nosso: hit-testing, foco, ordem de tab, drag & drop, menu de contexto, tooltip. Nada vem de graça."* Descobrir na F2 que faltam três widgets é descobrir tarde.

## Decisão

**Três widgets próprios, desenhados com os tokens que já existem. Nenhum diálogo nativo do sistema.**

Diálogo nativo — `MessageBox` no Windows, `NSAlert` no macOS, GTK no Linux — foi rejeitado por três motivos, e o segundo é decisivo:

1. Bloqueia o event loop da plataforma, e com ele todo o render `wgpu` da janela ([ADR-0007](0007-modelo-de-threading.md)).
2. **Não é configurável pelo usuário**, contrariando o princípio 2 do PRD-004 — *"nenhum valor de aparência hardcoded"*. Um diálogo do sistema no meio de uma barra de abas inteiramente customizada é a única parte da interface que a config não alcança.
3. Comportamento e aparência diferentes nas três plataformas, para um app que decidiu ser consistente nelas.

### Dois canais, deliberadamente distintos

A escolha que organiza o resto: **um fato que vale para o app não aparece no mesmo lugar que um fato que vale para uma aba só.**

**Canal 1 — aviso do app.** Overlay no canto superior direito da área de conteúdo, sob a barra de abas. Recebe: erro de config com linha e chave (RF-4.21), chave desconhecida (RF-4.22), fonte ausente (RF-5.8), sessão corrompida (RF-3.14), schema mais novo (RF-3.16), convite de integração de shell (RF-3.1) e fallback de GPU (ADR-0001).

**Canal 2 — nota na aba.** Escrita como primeira linha no grid daquela aba, marcada para não se confundir com saída de programa. Recebe: diretório gravado inexistente (RF-3.10) e código de saída de processo que falhou (RF-1.3).

O RF-3.10 já pede exatamente isso — *"a aba abre no diretório home e informa isso na primeira linha"* — e o RF-1.3 cabe no mesmo mecanismo sem inventar nada. O critério é simples: informação que pertence ao histórico de um terminal fica dentro dele, e sobrevive à rolagem; informação sobre o app fica no overlay, e é dispensável.

### Anatomia — aviso do app

Tokens da [especificação visual](../design/especificacao-visual.md), seções 1.2, 1.3, 1.5 e 1.10:

- Fundo `#1a1e25`, borda `#2e343e`, raio 8, sombra `0 18px 44px rgba(0,0,0,.55)`, animação `pop .13s` — os mesmos do popover.
- Barra de severidade de 2px à esquerda: erro `#ef8a8a`, aviso `#e0b060`, informação `#5ed3bc`.
- Título 12.5px `#dfe4ea`; corpo 11px `#6b737e`; botão de fechar 17×17 raio 4, `#727a86` com hover `#39404b`.
- **Máximo de três empilhados**, `gap: 8`; o quarto substitui o mais antigo.
- Erro e aviso persistem até dispensa; informação sai sozinha em 6 s. `Esc` dispensa o do topo.
- Erro de config traz o caminho do arquivo, a linha e a chave, em mono 10.5px — o RF-4.21 exige linha e chave, e mostrar isso em fonte proporcional dificulta a leitura.
- O convite do RF-3.1 é o único com ação embutida: um snippet copiável e um "não mostrar mais", que é o *"dispensável em definitivo"* do requisito.

### Anatomia — diálogo de confirmação

Atende RF-1.6, RF-2.23 e a chave `confirm_close_window`.

- Overlay `rgba(6,7,9,.45)` sobre a janela; modal raio 10, fundo `#1a1e25`, borda `#2e343e`, sombra `0 28px 70px rgba(0,0,0,.6)`, largura 380, `padding: 16`.
- Título 13px `#e6eaef`, corpo 12.5px `#d7dce3`.
- Dois botões, raio 5: cancelar com borda `#262b34`; confirmar destrutivo em `#e08585` com hover `#2e2224`.
- **Foco inicial no cancelar.** `Enter` confirma o botão focado, `Esc` cancela. Um diálogo destrutivo que confirma com `Enter` por default transforma um `Enter` distraído em perda de trabalho.
- RF-2.23 exibe a contagem de abas e não é desligável, como o requisito determina.

### Anatomia — menu de contexto

Reaproveita integralmente os tokens do menu de perfis (seção 2.9 da especificação), que já estão definidos: fundo `#1a1e25`, borda `#2e343e`, raio 8, `padding: 6`, sombra de popover, animação `pop .13s`; item `padding: 7px 8px`, raio 5, `gap: 10`, texto 12.5px `#d7dce3`, hover `#242a33`; divisor `1px #2a2f38` com `margin: 5px 4px`; item destrutivo `#e08585` com hover `#2e2224`; chip de tecla mono 9.5px `#5c646f`.

Comportamento: ancorado no cursor, virado horizontal e verticalmente para caber na tela, navegável por setas com `Enter` e `Esc`, fechado por clique fora ou perda de foco da janela. Item indisponível fica esmaecido em `#5c646f`, nunca ausente — menu cuja lista muda de tamanho a cada abertura obriga a reaprender a posição dos itens.

Três menus:

| Menu | Origem | Fase |
|---|---|---|
| Aba | RF-1.1, RF-1.2, RF-2.20 | F2 |
| Grupo | RF-2.22 | F3 |
| Terminal (copiar, colar, selecionar tudo) | roadmap F6 | F6 |

**O menu do grupo e o editor de grupo compartilham uma única lista de ações.** O RF-2.22 enumera seis itens e a seção 2.10 do design mostra o editor com quatro deles; manter duas listas garante que divirjam na primeira mudança. Duplo clique na pílula abre o editor, botão direito abre o menu, e os dois leem a mesma definição.

### Acessibilidade

Estes três widgets são o que o `accesskit` precisa expor na F6 — diálogo modal, menu e notificação são justamente os papéis que leitor de tela trata de forma especial. Registrado aqui para que a F6 não descubra que a dívida do ADR-0001 cresceu.

## Alternativas consideradas

### Diálogos e menus nativos do sistema

Vêm de graça, com acessibilidade e convenções da plataforma incluídas — o que é um argumento real, não desprezível.

Descartada pelos três motivos do início da decisão. O que pesa mais não é o bloqueio do event loop, é a inconfigurabilidade: o PRD-004 vendeu controle total da aparência, e um `MessageBox` seria a única superfície fora desse controle. A acessibilidade perdida está endereçada — `accesskit` na F6 — e é dívida já assumida pelo ADR-0001, não nova.

### Aviso apenas na aba, sem overlay

Um canal só, tudo escrito no grid do terminal. Simples, e resolve o RF-3.10 de graça.

Descartada porque erro de config não pertence a nenhuma aba — pertence ao app. Escrever no grid da aba ativa espalharia a mesma mensagem por abas diferentes conforme o foco no momento do erro, e ainda seria apagado por um `clear`. Informação sobre o app não pode ser destruída por um comando do shell.

### Overlay apenas, sem nota na aba

Também um canal só, e mais consistente à primeira vista.

Descartada porque o RF-3.10 pede explicitamente a primeira linha da aba, e com razão: "este terminal não abriu onde devia" é um fato daquele terminal, precisa sobreviver à rolagem e continuar visível quando o usuário voltar à aba dez minutos depois. Um toast que desapareceu não informa nada.

### Barra de status permanente para avisos

Concentraria mensagens num lugar fixo.

Descartada porque a barra de status é `[v2]` ([ADR-0009](0009-referencia-visual-e-reconciliacao.md), PRD-009 em rascunho). Amarrar oito requisitos do v1 a um elemento de v2 inverteria a ordem das fases.

### Menu de contexto reaproveitando o popover do editor de grupo

O editor já existe no design e cobre quatro dos seis itens do RF-2.22.

Descartada como substituto porque menu de contexto precisa abrir no cursor, sobre qualquer aba, com itens que variam por alvo — não é a mesma coisa que um painel ancorado num grupo. Mas a observação foi aproveitada: os dois compartilham a lista de ações, que era o risco real de divergência.

## Consequências

### Positivas

- Oito requisitos aprovados deixam de depender de invenção na hora da implementação.
- RF-1.6 e RF-2.23, que são cenários de aceite de F2 e F3, ficam implementáveis.
- Nenhuma cor nova: os três widgets saem de tokens já declarados, e a spec visual continua sendo a fonte única.
- Menu de contexto e editor de grupo não podem divergir, por construção.
- Regra clara para futuras mensagens: é fato do app ou da aba?

### Negativas

- Três widgets a escrever na mão, incluindo navegação por teclado e posicionamento com flip — trabalho que o ADR-0001 previu e que agora tem tamanho.
- Aviso empilhável introduz estado de UI com temporizador, que é mais uma fonte de sujeira para o render damage-driven ([ADR-0007](0007-modelo-de-threading.md)): o timer marca sujeira, não roda loop.
- Dois canais significam decidir, para cada mensagem nova, em qual deles ela entra. Custo pequeno, mas recorrente.
- A dívida de acessibilidade cresce: três papéis a mais para o `accesskit` cobrir na F6.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Aviso de erro de config aparecer repetidamente durante edição do arquivo | Alta | Médio | Substituir o aviso existente da mesma origem em vez de empilhar; o hot reload dispara a cada gravação |
| Diálogo modal em janela sem foco travar o usuário | Média | Médio | Modal é por janela, não por app; `Esc` sempre cancela |
| Menu de contexto abrir fora da tela em monitor secundário | Média | Baixo | Flip nos dois eixos, calculado contra o monitor da janela |
| Timer do aviso forçar frames desnecessários | Média | Baixo | Mesma regra do cursor piscando no ADR-0007: timer marca sujeira |
| Divergência entre menu do grupo e editor de grupo | Média | Baixo | Lista de ações única, compartilhada pelos dois |
| Nota na aba ser confundida com saída de programa | Média | Baixo | Marcação visual distinta usando `#5ed3bc`; nunca imita prompt |
