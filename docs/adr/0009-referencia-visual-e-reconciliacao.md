# ADR-0009 — Referência visual e reconciliação com o design canvas

**Status:** Superseded by [ADR-0027](0027-controles-de-janela-e-resize-proprios.md) (parcial: só a linha "Barra de título customizada" da tabela de escopo faseado na seção 2 da decisão, e a divergência equivalente da especificação visual §4.4 — as demais sete linhas e as outras três seções desta decisão continuam valendo)
**Data:** 2026-08-26
**Relacionados:** ADR-0003, ADR-0006, ADR-0008, PRD-000, PRD-004, PRD-005

## Contexto

A documentação de ADR-0001 a ADR-0008 e PRD-000 a PRD-005 fixou comportamento, arquitetura e superfície de configuração. Não fixou **aparência**. Um implementador — pessoa ou agente — que lesse o PRD-002 saberia que grupos têm nome, cor e colapso, mas desenharia uma barra de abas arbitrária.

O design foi então produzido em Claude Design (projeto `b0bc7589-f967-40cb-98ab-caef4070a95a`) e importado para [`docs/design/`](../design/README.md). Ele é um mockup interativo completo, e ao ser comparado com a documentação existente revelou **oito divergências**, três delas de escopo e não apenas de estilo.

Este ADR resolve todas.

## Decisão

### 1. Autoridade dividida

O design é **normativo para a aparência** do chrome: cores, dimensões, raios, espaçamentos e estados visuais. Os PRDs continuam **normativos para o comportamento**: o que acontece ao interagir, o que persiste, o que é configurável.

Nenhum valor de aparência é inventado durante a implementação. Sai da tabela de tokens da [especificação visual](../design/especificacao-visual.md) ou do [`porecatu.example.toml`](../config/porecatu.example.toml), que passa a trazer esses mesmos valores como default.

### 2. Escopo faseado, roadmap intacto

O design mostra o produto completo, não o v1. Seis elementos dele estão fora do escopo do v1 — três porque o [PRD-000](../prd/prd-000-visao-de-produto.md) já os havia declarado não-objetivos, três porque nunca tiveram PRD:

| Elemento | Situação anterior |
|---|---|
| Painéis divididos | não-objetivo declarado (PRD-000, [ADR-0006](0006-modelo-de-abas-e-grupos.md)) |
| Perfis de aba e badge | não-objetivo declarado (PRD-000) |
| Barra de título customizada | contraria o default `decorations = true` |
| Paleta de comandos | inexistente |
| Barra de status | inexistente |
| Painel de configurações GUI | contraria [ADR-0003](0003-formato-de-configuracao.md) |

**Decisão:** todo elemento do design recebe etiqueta `[v1]` ou `[v2]` na tabela de fases da especificação visual. Os `[v2]` entram na documentação como **desenho aprovado**, não como escopo: o roadmap F0..F6 e os não-objetivos do PRD-000 permanecem exatamente como estavam.

Quatro deles ganham PRD em rascunho, para ter endereço e não serem reinventados do zero depois: [PRD-006](../prd/prd-006-paineis-divididos.md) painéis, [PRD-007](../prd/prd-007-perfis-de-aba.md) perfis, [PRD-008](../prd/prd-008-paleta-de-comandos.md) paleta de comandos, [PRD-009](../prd/prd-009-barra-de-status.md) barra de status. Painel de configurações e barra de título ficam sem PRD, tratados aqui.

A razão de registrar o desenho sem abrir o escopo: um alvo visual de longo prazo evita que decisões do v1 fechem portas para o v2 sem necessidade. Saber que existirá uma barra de status muda onde se reserva espaço vertical; saber que existirão painéis muda se `Tab` é um alias de terminal ou uma struct própria — e ela já é uma struct própria, por causa do ADR-0006.

### 3. Keybindings: ADR-0008 vence

O design exibe `Ctrl+T` (nova aba), `Ctrl+G` (colapsar grupo), `Ctrl+,` (configurações) e `Ctrl+1..6` (perfis). O [ADR-0008](0008-teclas-e-roteamento-de-input.md) proibiu `Ctrl+<letra>` sozinho nos defaults de Windows e Linux, porque esse espaço pertence ao terminal: `Ctrl+T` é transpose-chars do readline, `Ctrl+G` é abort do emacs.

**Decisão:** ADR-0008 prevalece. Os chips de tecla do mockup são ilustrativos e estão anotados como tal na especificação visual. A regra "nunca `Ctrl+<letra>` sozinho" permanece.

Duas consequências de detalhe, dentro da decisão vigente e não contra ela:

- `Ctrl+Shift+P` passa a ser a **paleta de comandos** `[v2]`, como no design.
- `theme.cycle` migra de `Ctrl+Shift+P` para **`Ctrl+Shift+Y`**.

Isso corrige um default no ADR-0008 sem mudar sua decisão, então ele permanece `Aceito`.

### 4. Tema do design como default do projeto

O `porecatu.example.toml` trazia JetBrains Mono e uma paleta catppuccin de oito cores; o design usa IBM Plex Sans/Mono, superfícies próprias e seis cores de grupo.

**Decisão:** os tokens do design viram os valores **default**. Catppuccin passa a ser um `[[themes]]` nomeado opcional.

A razão é operacional: com o design como default, o binário recém-compilado bate com o mockup, e qualquer diferença visível é um bug de implementação — não uma questão de configuração. Se o mockup e o default divergissem, ninguém saberia qual dos dois está certo.

### 5. Indicador de grupo combinável

O design usa **pílula e sublinhado ao mesmo tempo**: a pílula identifica o grupo na barra, o sublinhado colorido na base de cada aba diz a que grupo ela pertence mesmo quando a pílula está fora da vista por rolagem. O [PRD-004](../prd/prd-004-aparencia-do-chrome.md) RF-4.14 havia modelado `indicator_style` como enum exclusivo.

**Decisão:** `indicator_style` aceita **lista combinável**. Default `["pill", "underline"]`. Os quatro estilos (`pill`, `underline`, `left-bar`, `outline`) continuam válidos, isolados ou combinados.

### 6. Painel de configurações GUI versus ADR-0003

O ADR-0003 decidiu que a configuração vive em um arquivo TOML. O design tem um drawer de configurações com toggles.

**Decisão:** o painel é `[v2]` e, quando existir, **escreve no arquivo TOML**. Não mantém estado paralelo, não guarda preferências em outro lugar, não introduz uma segunda fonte de verdade. O arquivo continua sendo a configuração; o painel é apenas um editor dela.

Isso preserva o ADR-0003 intacto e mantém a propriedade que importa: o usuário pode versionar seus dotfiles e ver toda a configuração em um lugar. O custo é que o painel precisa preservar comentários e formatação ao regravar o arquivo — problema conhecido, resolvível com um parser que preserva a árvore sintática.

### 7. Barra de título customizada

**Decisão:** `[v2]`. O default do v1 permanece `decorations = true`, com as decorações do sistema. Quando a barra customizada existir, será opcional e configurável, nunca imposta — decorações nativas trazem comportamento de janela que é caro e arriscado reimplementar em três plataformas.

### 8. Terminologia

O design diz "guias"; a documentação diz "abas".

**Decisão:** o projeto padroniza **"abas"**, em documentação e em strings de interface. Os rótulos do mockup ficam anotados como divergência conhecida. A documentação não muda.

## Alternativas consideradas

### Expandir o v1 para tudo que está no design

Traria coerência imediata entre desenho e escopo. Descartada porque multiplicaria o escopo do v1: painéis mudam o modelo de `Tab` (ADR-0006), perfis exigem um subsistema próprio de spawn, o painel de configurações exige reescrever a gravação de TOML, e a barra de título exige lidar com arrasto, snap e maximização em três plataformas. O v1 deixaria de entregar em prazo razoável, e o valor central — organização de terminais — chegaria depois, não antes.

### Manter o design apenas como link no README

Custo zero. Descartada porque referência que não está no repositório não é lida. Um link não impede que alguém invente uma cor de aba, e não sobrevive a mudanças no canvas sem que ninguém perceba.

### Copiar o design sem reconciliar as divergências

Seria mais rápido. Descartada porque deixaria a documentação em contradição consigo mesma: o PRD-000 dizendo que painéis estão fora do v1 e o mockup mostrando painéis, sem nada dizendo qual dos dois vale. Ambiguidade em documentação normativa é pior que ausência dela.

### Adotar os keybindings do design

Traria fidelidade total ao mockup. Descartada pelo motivo original do ADR-0008: `Ctrl+T` e `Ctrl+G` capturados pela aplicação quebram readline e emacs dentro do terminal, e o usuário não tem como descobrir por quê. Fidelidade visual não justifica quebrar o uso normal do shell.

## Consequências

### Positivas

- Existe um alvo visual concreto e versionado; a implementação para de inventar aparência.
- O `porecatu.example.toml` vira o contrato executável entre desenho e código — divergência visível é bug, não configuração.
- Elementos `[v2]` estão registrados com desenho aprovado, sem inflar o escopo do v1.
- As oito divergências estão resolvidas por escrito, em vez de virarem discussão durante a implementação.
- A tabela de fases dá ao implementador uma resposta direta para "isto é para agora?".

### Negativas

- A pasta `docs/design/` precisa ser mantida em sincronia com o canvas manualmente. Canvas alterado sem regravar a cópia produz documentação errada.
- O mockup mostra mais do que o v1 entrega, e isso pode gerar expectativa equivocada em quem olhe só a imagem. Mitigado pelas marcações `v2` no mockup estático e pelo aviso de fase em três lugares.
- Nove requisitos do v1 não têm representação visual (seção 4.2 da especificação visual). Continuam sendo decisão de desenho na implementação.
- O painel de configurações do `[v2]` herda um problema real: regravar TOML preservando comentários.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Canvas evoluir e a cópia local ficar defasada | Alta | Médio | Procedimento de atualização no [README do design](../design/README.md); `projectId` registrado para releitura |
| Implementador construir elemento `[v2]` na fase errada | Média | Alto | Tabela de fases; aviso em CLAUDE.md; marcação `v2` visível no mockup |
| Divergência silenciosa entre tokens e o TOML | Média | Médio | Verificação de cruzamento de tokens na rotina de checagem da documentação |
| Requisitos sem desenho gerarem soluções inconsistentes | Média | Médio | Seção 4.2 lista todos explicitamente; tokens da seção 1 são obrigatórios mesmo sem desenho |
| Expectativa de que painéis e perfis estão no v1 | Média | Baixo | Não-objetivos do PRD-000 preservados e apontando para os rascunhos |
