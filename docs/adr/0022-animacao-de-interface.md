# ADR-0022 — Animação de interface sob render damage-driven

**Status:** Aceito
**Data:** 2026-08-27
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0019](0019-tooltip.md), [PRD-002](../prd/prd-002-grupos-de-abas.md)

## Contexto

O [ADR-0007](0007-modelo-de-threading.md) decidiu o modelo de render em cinco
regras, e a quinta é categórica: *"**Sem sujeira, sem frame.** Não existe loop
de render contínuo. Terminal parado consome zero GPU."* A quarta define a única
fonte de sujeira de chrome — *"mudanças de chrome (hover, foco, drag, reload de
config) marcam a barra como suja pelo mesmo mecanismo"* —, e o cursor piscando é
tratado como exceção nomeada: *"um timer que marca sujeira, não um loop"*.

A F3 traz um requisito que essa regra não cobre. O **RF-2.5** é explícito:

> Ao criar um grupo com abas não adjacentes, as abas são **movidas para ficarem
> contíguas**, na ordem em que aparecem na barra. O movimento é animado, para
> que a reordenação não surpreenda.

Não é enfeite: é cenário de aceite (*"E o movimento é animado"*), é risco
mitigado no [ADR-0006](0006-modelo-de-abas-e-grupos.md) (*"reordenação ao
agrupar surpreender o usuário → animar o movimento das abas ao formar o
grupo"*), e a consequência negativa registrada no mesmo ADR-0006 diz que a
reordenação *"precisa ser visível na UI (animação de movimento), senão
surpreende"*.

**A F2 recusou animação três vezes**, sempre pelo mesmo argumento, e sempre
registrando o motivo na especificação visual:

- Indicador de atividade **não pisca** (§2.17): *"indicador animado em trinta
  abas de fundo é um frame por intervalo de piscada, contra a regra do ADR-0007
  de que terminal ocioso não gera frame. Presença é o sinal."*
- Rolagem da trilha **sem inércia e sem easing** (§2.18): *"rolagem contínua é
  um frame por quadro de animação; rolagem discreta é um frame por evento."*
- Auto-scroll no arraste por evento de cursor, não pelo intervalo de `.15s` que
  a §2.19 descreve — a etapa 5 registrou que isso *"exigiria um temporizador de
  UI que esta etapa não introduziu"*.

A etapa 6 **introduziu** esse temporizador, por outro caminho: o atraso do
tooltip ([ADR-0019](0019-tooltip.md), 600 ms) e a expiração da informação
([ADR-0014](0014-superficie-de-aviso-e-dialogo.md), 6 s) rodam por
`ControlFlow::WaitUntil`, sem thread própria — o event loop dorme até a hora
exata e volta a `ControlFlow::Wait` quando nada está pendente. O mecanismo
existe; falta decidir se ele pode dirigir movimento e sob que limites.

Sem esta decisão, a F3 tem duas saídas ruins: ignorar o RF-2.5 (e falhar um
cenário de aceite) ou improvisar um loop de render, que é exatamente o que o
ADR-0007 proíbe.

## Decisão

**Existe um relógio de animação por janela, dirigido pelo `WaitUntil` que já
existe, ativo só enquanto há animação pendente. A regra "sem sujeira, sem frame"
do ADR-0007 continua válida: animação em curso *é* sujeira, e quando ela termina
o event loop volta a dormir.**

Este ADR **não** supersede o ADR-0007. Ele nomeia uma quinta fonte de sujeira,
ao lado das quatro que a regra 4 já lista, e a submete a limites explícitos.

### O relógio

```
AnimationClock {
    active: Vec<Animation>,   // vazio = event loop volta a Wait
}

Animation {
    kind: AnimationKind,
    started_at: Instant,
    duration: Duration,
}
```

- Vive em `WindowState`, ao lado de `warnings`, `hover` e `drag`.
- **Recebe `Instant` de fora e nunca chama `Instant::now()`** — a mesma
  disciplina que a F2 impôs a `WarningStack`, `ConfirmDialog` e `Hover`, e que é
  o que torna esses estados testáveis sem dormir de verdade.
- Contribui para `next_wake()` como qualquer outro temporizador. Enquanto há
  animação ativa, o deadline é o próximo intervalo de frame; quando a última
  termina, ele desaparece da conta e o `schedule_next_wake` volta a `Wait`.
- Interpolação é **linear**. Não há curva de easing: `porecatu-render` não tem
  primitiva de curva, a duração é curta, e curva sobre 180 ms é diferença que
  não se percebe numa barra de abas.

### Consumidores no v1 — dois, e a lista é fechada

| O que anima | Requisito | Duração |
|---|---|---|
| Reordenação das abas ao formar grupo | RF-2.5 | `.18s` |
| Rotação do caret da pílula ao colapsar/expandir | espec. §2.4 | `.15s` |

**Animação nova exige requisito novo**, na mesma disciplina que o
[ADR-0018](0018-composicao-de-frame.md) aplica a camadas (*"camada nova exige
requisito novo, como ação nova exige entrada no catálogo"*) e que o
[catálogo de ações](../reference/acoes.md) aplica a ações.

As três recusas da F2 **permanecem recusadas**, e agora por decisão registrada,
não por falta de mecanismo:

- Indicador que pisca: seria animação **contínua e sem fim**, em N abas ao mesmo
  tempo. O relógio só existe para movimento com começo e fim.
- Easing de rolagem: o gesto já é discreto por decisão de §2.18; easing tornaria
  contínuo um frame que hoje é um por evento.
- Auto-scroll por intervalo durante o arraste: pode passar a usar o relógio,
  porque tem fim (a borda deixa de ser tocada), mas fica **fora do v1** — o
  comportamento atual por `CursorMoved` funciona, e trocá-lo é refinamento sem
  requisito.

### Rotação do caret sem primitiva de rotação

`porecatu-render` não tem transformação afim, e o caret é um glifo. A rotação de
`0°` para `90°` da §2.4 é implementada como **troca de glifo** — `▶` para `▼` —
no meio da animação. Durante os `.15s` o que anima, de fato, é o resto do
colapso: as abas do grupo desaparecendo da trilha.

Isso é divergência de desenho e vai para a seção 4.4 da especificação visual,
não para uma primitiva nova em `porecatu-render`.

### Desligável

Chave de configuração `[appearance] animations = true`, com `false` aplicando
todas as animações instantaneamente — o estado final no primeiro frame, sem
nenhum intermediário. Três motivos, na ordem em que importam:

1. **Acessibilidade.** Movimento na tela é gatilho documentado; o v1 já assumiu
   dívida de acessibilidade no ADR-0001 e não deve acrescentar outra.
2. **Máquina remota.** Sob RDP ou VNC, animação é banda desperdiçada.
3. **Coerência com o princípio 2 do PRD-004** — nada de aparência fica fora do
   alcance da config.

Com `animations = false` o RF-2.5 continua **funcionalmente** cumprido: as abas
ficam contíguas. O que se perde é a legibilidade do movimento, que é escolha
explícita do usuário.

### Interação durante a animação

**A animação nunca bloqueia input.** Qualquer interação — clique, tecla, novo
arraste — aplica o estado final **imediatamente** e descarta a animação em
curso. Duas razões: o estado do `Workspace` já é o final desde o primeiro frame
(a animação interpola apenas a **posição de desenho**, nunca o modelo), e input
enfileirado atrás de uma animação é a classe de bug que faz interface parecer
travada.

Consequência de projeto que decorre disso: `tab_bar::layout` continua puro e
alheio a animação. Quem interpola é a **pintura** — `chrome.rs` recebe o layout
final e o progresso do relógio, e desloca retângulos. Nada em
`porecatu-core` nem em `tab_bar.rs` sabe que animação existe.

## Alternativas consideradas

### Ignorar o RF-2.5 e reordenar instantaneamente

O caminho que a F2 tomou três vezes, e o mais barato. Descartada porque aqui o
requisito é explícito, é cenário de aceite, e a razão dele está registrada em
dois documentos: sem o movimento, abas que o usuário selecionou em três pontos
distantes da barra aparecem juntas noutro lugar, e nada na tela explica o que
aconteceu. As três recusas da F2 eram sobre animação **decorativa**; esta é
explicativa.

### Loop de render contínuo enquanto a janela está em foco

Como a maioria dos apps de GUI faz, e resolveria animação sem mecanismo novo.
Descartada porque revoga a regra 5 do ADR-0007 e a métrica de CPU em ~0% com o
terminal ocioso, que é critério de saída da F1 e uma das razões de o projeto ter
escolhido `wgpu` com render damage-driven em vez de um toolkit pronto. Um
emulador de terminal que consome GPU parado é o oposto do que este projeto
prometeu.

### Thread própria de animação, marcando sujeira por `EventLoopProxy`

Espelharia a thread de leitura do PTY, e o mecanismo de `Wakeup` já existe.
Descartada porque `WaitUntil` resolve o mesmo problema sem thread, sem canal e
sem sincronização — a F2 já provou isso com o tooltip e o aviso. Thread para
dormir é thread a mais para encerrar corretamente no shutdown, e o
[ADR-0017](0017-ciclo-de-vida-da-aba.md) mostra o custo de encerrar threads
neste projeto.

### Animar interpolando o `Workspace` em vez da posição de desenho

Reaproveitaria o padrão do arraste da F2, que clona o `Workspace` e aplica
`move_tab` no clone. Descartada porque o arraste tem um destino **indefinido até
o usuário soltar**, e a animação tem destino conhecido desde o primeiro frame:
interpolar o modelo criaria estados intermediários inválidos (abas de um grupo
não contíguas, violando a invariante do ADR-0006) e faria a invariante falhar no
meio da animação.

### Curva de easing, como a especificação sugere para `pop` e `slidein`

Movimento mais natural, e a espec. já usa `ease-out` nos popovers. Descartada
para o v1 porque exigiria uma tabela de curvas em `porecatu-ui` e porque a
diferença perceptível sobre 180 ms de deslocamento horizontal é mínima. Os
popovers da F2 já não implementam suas curvas — aparecem sem animação —, então
adotar easing aqui criaria inconsistência com o que já existe.

## Consequências

### Positivas

- O RF-2.5 passa a ser implementável, e com ele o cenário de aceite
  *"agrupar abas não adjacentes"*.
- A regra do ADR-0007 continua íntegra: o event loop dorme quando nada anima, e
  a métrica de CPU ocioso não muda.
- Reaproveita inteiramente o `WaitUntil` da F2 — nenhuma thread, nenhum canal,
  nenhum mecanismo novo de sujeira.
- `porecatu-core` e `tab_bar.rs` seguem alheios a animação, e o layout continua
  função pura testável sem GPU.
- A lista fechada de dois consumidores impede que "animar" vire resposta padrão
  para qualquer refinamento visual.

### Negativas

- `WindowState` ganha o sexto estado com temporizador, e `next_wake()` passa a
  ter uma fonte que dispara a cada frame enquanto ativa — é o único caso em que
  o app acorda em cadência de vídeo.
- A pintura deixa de ser tradução direta do layout: `chrome.rs` passa a receber
  progresso e deslocar retângulos, o que é lógica nova numa camada que
  deliberadamente não tem teste automatizado.
- Duração e interpolação viram valores de aparência a mais para a F4 ligar à
  config.
- Rotação de caret entregue como troca de glifo é divergência visível de quem
  compara com o mockup.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Animação nunca terminar e manter o loop acordado | Média | Alto | Duração é fixa e o relógio compara com `Instant` recebido de fora; animação expirada é removida no `tick`, e o teste cobre que o clock fica vazio depois da duração |
| CPU ociosa subir por animação esquecida ativa | Média | Alto | `next_wake()` só considera o clock se `active` não está vazio; teste de que `schedule_next_wake` volta a `Wait` depois do fim |
| Input enfileirado atrás da animação fazer a UI parecer travada | Média | Alto | Qualquer input aplica o estado final e descarta a animação; o modelo já está no estado final desde o primeiro frame |
| Invariante de contiguidade falhar no meio da animação | Baixa | Alto | O `Workspace` nunca é interpolado — só a posição de desenho. Nenhum estado intermediário existe no modelo |
| "Animar" virar resposta para qualquer refinamento visual | Alta | Médio | Lista de consumidores fechada neste ADR; animação nova exige requisito novo, como camada nova no ADR-0018 |
| Troca de glifo do caret parecer defeito | Baixa | Baixo | Registrado na seção 4.4 da especificação como divergência conhecida |
