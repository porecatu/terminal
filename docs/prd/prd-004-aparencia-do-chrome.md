# PRD-004 — Aparência do chrome (abas e grupos)

**Status:** Aprovado
**Data:** 2026-08-26
**Requisito de origem:** 4 — as cores, desenho e dimensões das abas e grupos de abas devem ser customizáveis em um arquivo de configuração
**Relacionados:** [ADR-0003](../adr/0003-formato-de-configuracao.md), [ADR-0001](../adr/0001-stack-de-gui.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md), [PRD-001](prd-001-abas.md), [PRD-002](prd-002-grupos-de-abas.md)

> **Valores default vêm do design.** Este PRD define *qual* superfície é configurável. Os *valores* padrão de cada chave saem da [especificação visual](../design/especificacao-visual.md), seção 1, e já estão no [`porecatu.example.toml`](../config/porecatu.example.toml). Nenhuma cor ou dimensão é inventada na implementação. Ver [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md).

## Problema

O requisito é específico e vai além de "aplicar um tema": **cores, desenho e dimensões**. Isso significa que o usuário controla a geometria da barra de abas, não só sua paleta.

A razão de fundo é que a barra de abas é permanente. Ela ocupa espaço vertical em todas as sessões, o dia inteiro. Usuários têm preferências fortes e legítimas sobre densidade (barra compacta versus confortável), sobre quanto contraste separa a aba ativa das inativas, e sobre o quão marcado é o indicador de grupo. Um emulador que impõe uma resposta única a essas perguntas irrita metade dos usuários o tempo todo.

Foi essa exigência que decidiu a stack de renderização: nenhum toolkit pronto expõe essa superfície de controle ([ADR-0001](../adr/0001-stack-de-gui.md)).

## Usuário-alvo

Todo usuário, com defaults que funcionam sem configuração alguma. A profundidade existe para quem quiser.

## Princípios

1. **Defaults completos.** Sem arquivo de config, o app abre com uma barra sensata. Nenhuma chave é obrigatória.
2. **Nenhum valor de aparência hardcoded.** Toda cor, dimensão e raio é uma chave de config com default declarado.
3. **Recarga a quente.** Editar o arquivo redesenha a barra imediatamente, sem reiniciar.
4. **Config inválida não quebra a tela.** Mantém a config anterior e mostra o erro ([ADR-0003](../adr/0003-formato-de-configuracao.md)).
5. **O default não depende de cor sozinha.** A aba ativa é distinguível também por forma e peso, para quem não distingue matizes.

## Requisitos funcionais

### Janela e barra

**RF-4.1** — Configuráveis: espaçamento interno da janela (horizontal e vertical), opacidade da janela, presença das decorações do sistema, e posição da barra de abas (topo ou base).

**RF-4.2** — Configurável: se a barra aparece quando existe apenas uma aba. *(Quem usa uma aba por vez não deveria pagar espaço vertical por isso.)*

### Dimensões das abas

**RF-4.3** — Configuráveis: altura da barra, largura mínima e máxima de aba, espaçamento interno horizontal da aba, espaço entre abas.

**RF-4.4** — Configurável: raio dos cantos da aba. Raio zero produz abas retangulares.

**RF-4.5** — A largura mínima é o piso do encolhimento antes de a barra passar a rolar ([PRD-001](prd-001-abas.md), RF-1.18).

### Cores das abas

**RF-4.6** — Configuráveis por estado — ativa, inativa, hover — as cores de fundo e de texto.

**RF-4.7** — Configuráveis: cor de fundo da barra (atrás das abas), cor e espessura da borda da aba ativa, e cor da borda de aba selecionada ([PRD-002](prd-002-grupos-de-abas.md), RF-2.2).

**RF-4.8** — Configuráveis: cores dos indicadores de atividade e de campainha ([PRD-001](prd-001-abas.md), RF-1.20 e RF-1.21), e se cada um aparece.

**RF-4.9** — Cores aceitam hexadecimal (`#rrggbb`, `#rrggbbaa`) e o valor `"transparent"`.

### Conteúdo da aba

**RF-4.10** — Configuráveis: família e tamanho da fonte do título da aba, independentes da fonte do terminal ([PRD-005](prd-005-aparencia-do-terminal.md)).

**RF-4.11** — Configuráveis, cada um ligável e desligável: botão de fechar, número de índice da aba, indicador de atividade, indicador de campainha, botão de nova aba na barra.

**RF-4.12** — Configurável: se o botão de fechar aparece sempre ou apenas no hover.

### Grupos

**RF-4.13** — Configuráveis para o rótulo do grupo: espaçamento interno horizontal, tamanho da fonte, raio dos cantos.

**RF-4.14** — Configurável: o **estilo do indicador de grupo**, como **lista combinável** — não escolha exclusiva. Quatro formas:

| Estilo | Desenho |
|---|---|
| `pill` | Pílula colorida em volta do rótulo; wrapper do grupo com fundo tingido |
| `underline` | Linha colorida na base de cada aba do grupo |
| `left-bar` | Barra vertical colorida antes da primeira aba do grupo |
| `outline` | Contorno colorido envolvendo rótulo e abas |

Default: `["pill", "underline"]`, como no design. Os dois se complementam — a pílula identifica o grupo na barra, o sublinhado diz a que grupo uma aba pertence quando a pílula saiu da vista por rolagem. Ver [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md).

**RF-4.15** — Configurável: espessura do indicador de grupo.

**RF-4.16** — Configurável: espaço horizontal entre grupos, distinto do espaço entre abas do mesmo grupo. *(É o que faz a fronteira do grupo ser percebida sem depender só da cor.)*

**RF-4.17** — Configurável: se um grupo colapsado mostra a contagem de abas no rótulo.

**RF-4.18** — Configurável: a **paleta de grupos** — uma lista de cores nomeadas, atribuídas automaticamente a grupos novos ([PRD-002](prd-002-grupos-de-abas.md), RF-2.4) e oferecidas na troca manual de cor.

**RF-4.19** — Configurável: quanto a cor do grupo tinge o fundo do wrapper do grupo, de nenhum tingimento a tingimento pleno. O tingimento se aplica apenas ao grupo expandido; colapsado fica transparente.

### Badge de perfil `[v2]`

**RF-4.23** — Configurável: exibir ou não o **badge de perfil** na aba — o marcador curto que identifica o tipo de terminal (`SSH`, `WSL`, `PS`). Requer [PRD-007](prd-007-perfis-de-aba.md), em rascunho.

**RF-4.24** — O badge herda a cor **do grupo**, não a do perfil, com o fundo tingido dessa mesma cor. Mantém a barra legível pelo eixo principal de organização; a cor do perfil aparece apenas no menu de perfis e nas configurações.

### Recarga e erro

**RF-4.20** — Salvar o arquivo de config aplica as mudanças de aparência em menos de 500 ms, sem reiniciar e sem perder o conteúdo das abas.

**RF-4.21** — Config inválida mantém a aparência anterior e mostra o erro com linha e chave. A barra nunca fica em estado quebrado ou invisível.

**RF-4.22** — Chave desconhecida gera aviso, não erro. O app continua funcionando.

## Critérios de aceite

```gherkin
Cenário: app funciona sem arquivo de config
  Dado que nenhum arquivo de configuração existe
  Quando o usuário abre o app
  Então a barra de abas é desenhada com valores padrão
  E nenhum erro é exibido

Cenário: recarga a quente de dimensão
  Dado o app aberto com altura de aba 32
  Quando o usuário altera a altura para 44 e salva
  Então a barra é redesenhada com a nova altura em menos de 500 ms
  E o conteúdo das abas é preservado

Cenário: config inválida preserva a aparência
  Dado o app aberto com aparência customizada
  Quando o usuário salva uma cor inválida como "#gggggg"
  Então a aparência anterior é mantida
  E um erro citando a linha e a chave é exibido

Cenário: indicador de grupo combinado, default
  Dado a configuração padrão
  Quando o usuário observa um grupo expandido
  Então o rótulo aparece como pílula colorida
  E cada aba do grupo recebe linha colorida na base

Cenário: estilo de indicador reduzido a uma forma
  Dado um grupo "api" de cor azul
  Quando o usuário define o estilo do indicador como ["underline"]
  Então as abas do grupo recebem linha azul embaixo
  E nenhuma pílula é desenhada

Cenário: aba ativa distinguível sem cor
  Dado a configuração padrão em escala de cinza
  Quando o usuário observa a barra
  Então a aba ativa é identificável por contraste e borda, não só por matiz

Cenário: barra oculta com uma aba
  Dado a barra configurada para ocultar quando há uma só aba
  Quando existe apenas uma aba aberta
  Então a barra não é desenhada
  E ela reaparece ao abrir a segunda aba

Cenário: chave desconhecida não quebra
  Dado uma config contendo uma chave inexistente
  Quando o app carrega a configuração
  Então um aviso é exibido
  E o restante da configuração é aplicado normalmente
```

## Fora de escopo

- **Barra de título customizada** — desenhada no canvas, fase `[v2]`. O default do v1 usa as decorações do sistema ([ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md))
- **Painel de configurações por GUI** — desenhado no canvas, fase `[v2]`. Quando existir, escreverá no TOML, sem introduzir segunda fonte de verdade ([ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md))
- Temas visuais de chrome distribuídos como arquivo separado e importável (v2)
- Aparência definida por script ou lógica ([ADR-0003](../adr/0003-formato-de-configuracao.md))
- Imagem de fundo na barra de abas
- Animações configuráveis além de ligar e desligar
- Ícone por aba ou por grupo
- Herdar o tema claro/escuro do sistema automaticamente (v2)

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Valores de aparência hardcoded no código | **zero** |
| Tempo entre salvar a config e ver o resultado | < 500 ms |
| Config inválida que derruba o app | zero ocorrências |
| Chaves de aparência sem default declarado | zero |
| Chaves no arquivo de exemplo sem requisito correspondente | zero |
