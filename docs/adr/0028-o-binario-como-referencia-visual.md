# ADR-0028 — O binário como referência visual

**Status:** Aceito
**Data:** 2026-09-02
**Supersedes:** ADR-0009 (parcial: a seção "1. Autoridade dividida" e a razão operacional da seção "4. Tema do design como default do projeto" — *"com o design como default, o binário recém-compilado bate com o mockup, e qualquer diferença visível é um bug de implementação"*. As seções 2, 3, 5, 6, 7 e 8 daquela decisão continuam valendo, assim como o tema do design permanecendo o default)
**Relacionados:** ADR-0003, ADR-0009, ADR-0022, ADR-0024, ADR-0026, ADR-0027, PRD-004, PRD-005

## Contexto

O [ADR-0009](0009-referencia-visual-e-reconciliacao.md) resolveu um problema real: não havia alvo visual, e quem implementasse desenharia uma barra de abas arbitrária. A resposta foi eleger o design canvas **normativo para a aparência** e o mockup estático como contrato executável — divergência visível entre binário e mockup seria bug de implementação.

Três fases depois, a relação se inverteu na prática. Ao ver a barra em tela, o usuário pediu, um a um, ajustes que o desenho não previa: cápsula de cor cheia no lugar do tingimento de `.07`, botão de nova aba por grupo, fim da ordem de cedência do overflow, ícones em `#e4e8ee` em repouso, borda de aba em 2px, aba de 34px numa trilha com 6px de respiro, largura de aba fixa, nome do grupo em peso 500, efeito de vidro na cápsula e na pílula, sombra em cápsula/aba solta/quadro do terminal, quadro arredondado em volta da grade, indicador de overflow em círculo sem contagem. São mais de vinte entradas na seção 4.4 da [especificação visual](../design/especificacao-visual.md), e a maioria delas veio de pedido direto do usuário depois de olhar o binário rodando — não de limitação técnica.

Enquanto o mockup era normativo, cada um desses pedidos entrava na documentação como **divergência a cobrar**, e o critério de saída da F4 prometia fechá-las: *"o binário com a config padrão bate com o mockup"*. Isso agenda, para a F4, o desfazimento de decisões que o dono do produto tomou deliberadamente.

O usuário fixou o mandato oposto: *"a interface do jeito que está é exatamente o que eu quero que seja. O modelo era só um modelo, um ponto de partida. Nenhuma mudança nela deve ser feita sem meu aval."*

## Decisão

### 1. O binário é normativo para a aparência

O que o binário desenha com a configuração padrão **é** o alvo visual. A [especificação visual](../design/especificacao-visual.md) passa a **descrever** esse estado, e é atualizada quando ele muda — deixa de ser a promessa que o código persegue e passa a ser o registro do que o código faz.

Consequência para o critério de saída da F4: *"o binário bate com o mockup"* vira **"a configuração padrão reproduz o binário atual"**. A direção da conferência inverte. Onde os dois não coincidirem, o errado é o documento.

### 2. Mockup e canvas são referência histórica

`mockup-estatico.html` e o projeto de canvas continuam versionados, como registro do ponto de partida e como desenho aprovado dos elementos `[v2]` que ainda não existem em código. **Não são normativos para nada `[v1]` já implementado.** Divergência entre eles e o binário não é bug, não entra em lista de pendência e não se "conserta" — nem no mockup, nem no código.

Isso encerra o risco que o próprio ADR-0009 registrou ("canvas evoluir e a cópia local ficar defasada"): a cópia local não precisa mais acompanhar nada.

### 3. Os PRDs continuam normativos para comportamento

Nada aqui toca a outra metade da autoridade dividida do ADR-0009 §1. O que acontece ao interagir, o que persiste, o que é configurável — isso continua vindo dos PRDs, e o binário divergir de um requisito de **comportamento** continua sendo bug.

A fronteira nem sempre é óbvia, e a regra prática é: se o requisito diz *o que a interface faz*, o PRD vence; se diz *como ela se parece*, o binário vence. Onde um requisito de PRD descrevia aparência que o binário mudou, o requisito é **emendado** para o estado real — não deixado em contradição. É o que esta rodada de documentação faz com RF-4.3, RF-4.5, RF-4.6, RF-4.14 e RF-4.19.

### 4. Nenhuma mudança de aparência sem aval explícito do usuário

Vale inclusive para o que a documentação antiga classificava como dívida a pagar. Um item de "dívida de primitiva" listado na seção 4.4 não autoriza ninguém a implementá-lo: cada um precisa de aval, um por um, porque implementá-lo **muda a interface aprovada**.

Sob essa regra, as quatro dívidas herdadas da F2 foram decididas:

| Item | Decisão |
|---|---|
| Hover por brilho na aba e na pílula (`brightness` em CPU) | **entra** na F4 |
| Sombra nos cinco widgets de chrome e no fantasma de arraste | **entra** na F4 (a técnica de camadas já existe em `chrome::push_shadow`) |
| Corpo de aviso e diálogo em três linhas | **não** se faz — o truncamento em uma linha é o comportamento aprovado |
| Auto-scroll do arraste a cada `.15s` | **não** se faz — rolar por evento de `CursorMoved` é o comportamento aprovado |

### 5. "Nenhum valor inventado" continua valendo — muda de fonte

A regra de CLAUDE.md não cai. Todo valor de aparência continua precisando de procedência declarada; a diferença é que a procedência agora é a especificação visual **atualizada a partir do código**, mais o [`porecatu.example.toml`](../config/porecatu.example.toml), que carrega os mesmos números como default. Um valor novo que apareça em código sem entrar nesses dois lugares continua sendo erro — porque quem lê a documentação depois não tem como saber de onde ele veio.

O que deixa de ser erro é o valor **divergir do mockup**.

## Alternativas consideradas

### Manter o mockup normativo e tratar os pedidos do usuário como exceções

É o que a documentação fazia até aqui: cada pedido virava uma linha de divergência, e a F4 prometia reconciliá-las. Rejeitada porque a promessa é falsa — reconciliar significaria reverter decisões do dono do produto. E porque a lista já passou de vinte entradas: uma exceção que cresce a cada fase deixou de ser exceção.

### Regerar o mockup a partir do binário

Manteria os dois artefatos coincidindo e o mockup normativo. Rejeitada por custo e por fragilidade: reescrever o HTML/CSS à mão a cada ajuste de tela reintroduz exatamente a defasagem que o risco do ADR-0009 previa, e o próximo pedido do usuário a recria. Um segundo artefato que precisa ser mantido em sincronia sem verificação automática é dívida, não garantia.

### Abandonar a especificação visual e ler os valores do código

Tentador: `palette.rs` e `TabBarStyle::DEFAULT` já são a fonte numérica, e os comentários deles já registram procedência e o pedido que originou cada valor. Rejeitada porque a especificação faz duas coisas que o código não faz: dá **anatomia** (o que compõe cada elemento e em que ordem) e serve de alvo para `porecatu-config` na F4 — sem ela, "toda chave do exemplo tem requisito" não é auditável. Também porque a auditoria de cor de `scripts/verify-docs.py` cruza o TOML contra a especificação, não contra o código.

## Consequências

### Positivas

- A documentação para de contradizer o binário em três pontos onde estava factualmente errada (sombra declarada inexistente, RF-2.17 declarado no modelo, `show_new_tab_button` declarado pendente).
- A F4 deixa de carregar um item impossível: nada nela tenta "consertar" a interface aprovada. O escopo encolhe para o que ela sempre foi — ligar a configuração ao que já está desenhado.
- O usuário ganha um veto explícito e documentado sobre aparência, em vez de descobrir mudanças depois de implementadas.
- `docs/design/` deixa de precisar de sincronia manual com o canvas.

### Negativas

- A especificação visual perde a propriedade de ser um alvo **independente** do código: um erro de implementação que ninguém note vira, por definição, o alvo. A mitigação é a revisão dirigida da F4 e o olho do usuário em tela — não há mais um terceiro artefato para desempatar.
- Reescrever as seções 1 e 2 da especificação a partir do código é trabalho manual, e a partir de agora todo ajuste visual carrega a obrigação de atualizá-las na mesma leva.
- Elementos `[v2]` continuam sem alvo verificável a não ser o mockup — a inversão vale só para o que já existe em `[v1]`.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Especificação voltar a defasar do código, agora sem ninguém para cobrar | Alta | Alto | Toda mudança visual passa a exigir a atualização da seção correspondente no mesmo PR; a regra fica em CLAUDE.md, junto com a de aval do usuário |
| Alguém ler a seção 4.4 como lista de pendências e "pagar a dívida" | Média | Alto | A seção vira "Histórico de decisões visuais", e o parágrafo de encerramento diz explicitamente que nada ali autoriza mudança |
| Valor novo entrar em código sem procedência, agora que o mockup não desempata | Média | Médio | A regra do §5 acima; `verify-docs.py` continua reprovando cor do TOML sem origem na especificação |
| Requisito de PRD ficar em contradição silenciosa com o binário | Média | Médio | O §3 exige emenda do requisito, e a auditoria bidirecional do critério de saída da F4 (chave sem requisito / requisito sem chave) é o que a detecta |
