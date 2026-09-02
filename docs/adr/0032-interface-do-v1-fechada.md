# ADR-0032 — A interface do v1 está fechada

**Status:** Aceito
**Data:** 2026-09-02
**Supersedes:** ADR-0009 (parcial: a seção "5. Indicador de grupo combinável")
**Relacionados:** ADR-0006, ADR-0009, ADR-0020, ADR-0022, ADR-0023, ADR-0028, PRD-002, PRD-004

## Contexto

O [ADR-0028](0028-o-binario-como-referencia-visual.md) inverteu a autoridade visual: o binário é normativo para a aparência, a especificação o descreve, e nenhuma mudança de aparência acontece sem aval do dono do produto. Ele resolveu *quem manda*. Duas perguntas ficaram abertas, e as duas apareceram na primeira vez que a F4 foi olhada de perto.

**A primeira é o indicador de grupo.** O ADR-0009 §5 decidiu que `indicator_style` é uma **lista combinável** de quatro formas — `pill`, `underline`, `left-bar`, `outline` —, e o RF-4.14 do [PRD-004](../prd/prd-004-aparencia-do-chrome.md) carrega essa tabela. Só duas delas chegaram a existir: `pill` é o que o binário desenha, e `underline` foi desenhado na F3 e **removido** por pedido do usuário — o traço na base da aba virou ruído depois que a cápsula passou a ser pintada com a cor cheia do grupo. `left-bar` e `outline` **nunca tiveram anatomia**: a seção 4.2 da [especificação visual](../design/especificacao-visual.md) os lista há três fases como "requisito do v1 sem representação no design", com a F4 como prazo.

**A segunda é o que ainda pode mudar de pixel no v1.** O ADR-0028 §4 aprovou duas das quatro dívidas de primitiva herdadas da F2 — hover por brilho e sombra nos widgets de chrome — e fechou as outras duas como decisão de não fazer. O que ele não disse é se aquela lista é **fechada**: qualquer acabamento novo poderia entrar por ser "pequeno", e a especificação continha sete frases do tipo *"ainda não desenhado"*, que leem como lacuna a preencher.

O usuário fechou as duas: *"a interface do app está ótima do jeito que está hoje. Principalmente a trilha de grupos e abas. Só vamos mexer nesse componente se for realmente necessário para incluir novos recursos. Quaisquer documentos que sugiram que falta algo ou algo precisa ser removido/alterado, quero que sejam corrigidos ou eliminados."* E, questionado especificamente sobre as duas dívidas aprovadas, confirmou que **ficam**.

## Decisão

### 1. O indicador de grupo é a pílula e a cápsula, e nada mais

`underline`, `left-bar` e `outline` **saem do escopo do v1**. O indicador é a pílula — nome e caret na cor cheia do grupo — mais a cápsula atrás das abas (§2.3, §2.4 da especificação visual).

Consequências na superfície de configuração:

- **`indicator_style` sai** do [`porecatu.example.toml`](../config/porecatu.example.toml). Com um valor só, a chave não escolhe nada; e mantê-la aceitando lista vazia criaria um estado que ninguém desenhou — grupo sem pílula é grupo sem lugar para o nome.
- **`indicator_thickness` sai** de `[appearance.groups]`. O valor `2` sobrevive, mas descrevendo o que ele governa de fato hoje: a espessura da borda da aba, que já tem chave própria (`active_border_width`/`inactive_border_width` em `[appearance.tabs.colors]`).
- RF-4.14 e RF-4.15 são **emendados** para descrever isso, na disciplina do ADR-0028 §3 (requisito que descrevia aparência mudada é emendado, não deixado em contradição).

Para quem quiser reabrir a decisão algum dia, o que o levantamento técnico apurou: `left-bar` seria um `Quad` fino antes da primeira aba e `outline` um `RoundedQuad` sem preenchimento sobre o retângulo do wrapper — **os dois desenháveis hoje, sem primitiva nova**. Com uma ressalva: a borda do `RoundedQuad` é um anel isotrópico derivado da SDF (`quad.wgsl`), então `outline` só existe como contorno **fechado nos quatro lados**; um contorno interrompido sob a pílula exigiria empilhar quads por lado. **A recusa é de produto, não de capacidade.**

A decisão também apaga dois casos especiais que qualquer estilo per-aba teria de resolver: com grupo colapsado não existe `TabRect` nenhum para pendurar indicador (o layout faz `continue`), e run implícito nunca ganha cápsula porque a condição de pintura é `pill.is_some()`. Um `underline` ligável obrigaria a decidir o que ele faz nos dois; não ter estilo per-aba resolve por construção.

### 2. A lista de mudanças visuais do v1 é fechada em duas

São elas, e nenhuma outra:

| Mudança | Onde | Fase |
|---|---|---|
| **Hover por brilho** — `brightness(1.18)` na aba, `1.25` na pílula, resolvido em CPU | §1.10, §2.4, §2.5 | F4, etapa 6 |
| **Sombra em camadas** nos cinco widgets de chrome e no fantasma de arraste | §1.7, §2.10, §2.19, §2.20 | F4, etapa 6 |

Este ADR **não supersede** o ADR-0028 §4: aquela tabela continua verdadeira, com as duas aprovadas e as duas fechadas como decisão de não fazer (corpo de aviso em três linhas, auto-scroll do arraste por intervalo). O que muda é que a lista passa a ser **exaustiva**.

Duas regras seguem daí:

- **A trilha de grupos e abas só é tocada quando um recurso novo exigir.** Não por acabamento, não por consistência com o mockup, não por "ficaria melhor". O hover é o único item aprovado que a toca, e ele já tem aval.
- **Falta de comportamento se registra; falta de aparência não existe.** Um documento que diga "falta desenhar X" está errado por definição — o binário é a referência (ADR-0028 §1) —, e a correção é reescrever o documento, não implementar X. As duas linhas da tabela acima são o único lugar onde "vai mudar" é uma afirmação verdadeira sobre pixels.

O que **continua** legitimamente registrado como pendência, porque é comportamento e não desenho: os defaults de macOS (nenhum atalho de app responde no Mac), `animations = false` ([ADR-0022](0022-animacao-de-interface.md)), a roda do mouse no popover de grupo de destino ([ADR-0023](0023-editor-de-grupo.md) pediu lista rolável sem dizer por qual gesto) e a entrada de cor por hexadecimal do RF-2.10, que acrescenta um campo ao editor de grupo. Os quatro seguem previstos.

## Alternativas consideradas

### Manter `indicator_style` com `pill` como único valor válido

Preservaria a forma da chave para o dia em que um estilo novo entrasse. Rejeitada porque uma lista de um elemento é uma escolha falsa, e porque o valor interessante dela seria a lista **vazia** — que desliga a pílula e deixa o grupo sem onde mostrar o nome. Configuração cuja única opção real é quebrar a interface não é configuração.

### Desenhar `left-bar` e `outline` na F4, como a §4.2 previa

Cumpriria o RF-4.14 ao pé da letra, e os dois são baratos de desenhar. Rejeitada pelo dono do produto: os dois nasceram como alternativas ao wrapper tingido a `.07`, e depois que a cápsula virou cor cheia eles são versões *mais discretas* de uma marca que o produto quer forte. Implementá-los seria oferecer ao usuário três maneiras de ter uma barra pior.

### Fechar também hover e sombra, congelando a interface por completo

Foi oferecido e recusado duas vezes. O hover resolve uma lacuna de *affordance* — hoje nada na barra responde ao cursor, exceto os botões de janela — e a sombra separa cinco superfícies que flutuam sobre o terminal com uma borda de 1px como única defesa. Nenhuma das duas muda o desenho parado; as duas mudam o que acontece quando o usuário interage.

### Deixar a lista aberta, decidindo caso a caso

É o estado anterior a este ADR. Rejeitada porque foi assim que a especificação acumulou sete frases de "ainda não desenhado" — cada uma inofensiva sozinha, todas juntas lendo como uma interface inacabada que a F4 deveria terminar.

## Consequências

### Positivas

- A F4 fica sendo **só configuração**: fora dos dois itens aprovados da etapa 6, nada nela mexe em pixel. O escopo encolhe e o critério de saída fica verificável.
- A superfície de configuração perde duas chaves que prometiam o que não existe.
- Um implementador que abra a documentação amanhã não encontra nenhum lugar sugerindo que a interface está incompleta — o que era o pedido literal.
- Some a última linha da §4.2 que falava do chrome atual; a lista fica só com a aba restaurada da F5, que é recurso e não desenho.

### Negativas

- RF-4.14 perde três dos quatro estilos, e o PRD-004 fica com um requisito de "estilo configurável" que não configura estilo nenhum. É emenda de requisito aprovado — o preço da disciplina do ADR-0028 §3.
- Fechar a lista em duas significa que uma melhoria visual futura precisa de um ADR para entrar, não de um commit. É deliberado, e é atrito por design.
- Quem gostava do `underline` — dizer a que grupo uma aba pertence quando a pílula sai da vista por rolagem — perde a única resposta que existia para esse caso. Mitigação parcial: a cápsula de cor cheia está atrás da aba inteira, então a cor continua visível mesmo sem a pílula em tela.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Acabamento visual entrar aos poucos, sem aval, por parecer pequeno | Média | Médio | A lista da §2 é exaustiva; qualquer item fora dela exige ADR novo, e a regra está em CLAUDE.md |
| Alguém reintroduzir `indicator_style` ao implementar a F4, seguindo o PRD antigo | Média | Baixo | RF-4.14 e RF-4.15 emendados na mesma leva; a chave sai do arquivo de exemplo, que é o insumo real do parser |
| A ausência de `underline` incomodar na prática com muitas abas por grupo | Baixa | Baixo | Reversível: o estilo existiu, a técnica é um `Quad` por aba, e voltaria por aval — como saiu |
