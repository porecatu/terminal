# PRD-005 — Aparência do terminal (cores e fontes)

**Status:** Aprovado
**Data:** 2026-08-26
**Requisito de origem:** 5 — as cores e fontes dos terminais devem ser customizáveis
**Relacionados:** [ADR-0003](../adr/0003-formato-de-configuracao.md), [ADR-0002](../adr/0002-motor-vte.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md), [PRD-004](prd-004-aparencia-do-chrome.md)

> **Valores default vêm do design.** A fonte padrão é Iosevka Fixed 12.5 com `line-height` 1.75 ([ADR-0025](../adr/0025-iosevka-no-lugar-da-ibm-plex.md) a trocou pela IBM Plex Mono, que não cobre braille, powerline nem formas geométricas); o fundo é `#0f1216` e a cor de texto `#c7ccd6`. As 16 cores ANSI derivam das seis cores semânticas de saída que o design define. Tudo está na [especificação visual](../design/especificacao-visual.md), seções 1.1, 1.4, 1.5 e 1.9, e já no [`porecatu.example.toml`](../config/porecatu.example.toml).

## Problema

Cores e fonte de terminal são a preferência mais pessoal do desenvolvedor, e a que mais afeta conforto ao longo de oito horas de uso. É também o requisito de entrada: um emulador que não deixa escolher a paleta e a fonte não é adotado, por melhor que seja no resto.

Há duas exigências técnicas menos óbvias por trás disso:

1. **Fallback de fonte.** Nenhuma fonte monoespaçada cobre todo o Unicode. Prompt de shell moderno mistura glyphs de Nerd Font, emoji e, dependendo do usuário, CJK. Sem cadeia de fallback, aparecem retângulos vazios.
2. **Paleta completa.** As 16 cores ANSI não bastam: cursor, texto sob o cursor, seleção e fundo da seleção são cores próprias, e programas TUI dependem delas para serem legíveis.

## Usuário-alvo

Todo usuário. Defaults com boa legibilidade e contraste adequado, sem exigir nenhuma configuração.

## Requisitos funcionais

### Fonte

**RF-5.1** — Configurável: a família da fonte principal do terminal, independente da fonte usada nos títulos de aba ([PRD-004](prd-004-aparencia-do-chrome.md)).

**RF-5.2** — Configurável: uma **cadeia ordenada de fontes de fallback**. Um glyph ausente na fonte principal é procurado nas seguintes, em ordem. Não achando em nenhuma, o app usa a fonte padrão do sistema antes de desenhar um retângulo vazio.

**RF-5.3** — Configurável: o tamanho da fonte em pontos, com valores fracionários.

**RF-5.4** — Configuráveis: as variantes de negrito e itálico, cada uma podendo apontar para uma família diferente da principal ou ser sintetizada.

**RF-5.5** — Configurável: se texto em negrito usa a versão brilhante da cor ANSI. *(Comportamento herdado de terminais antigos que alguns esperam e outros detestam.)*

**RF-5.6** — Configuráveis: altura de linha e espaçamento entre caracteres, ambos como multiplicadores das métricas naturais da fonte.

**RF-5.7** — Configurável: se ligaduras de fonte são aplicadas.

**RF-5.8** — Fonte inexistente no sistema não impede o app de abrir: cai para uma monoespaçada disponível e avisa qual família não foi encontrada.

**RF-5.9** — O usuário aumenta e diminui o tamanho da fonte por atalho, em tempo real. O ajuste vale para a sessão e não altera o arquivo de config.

**RF-5.10** — Configurável: se o ajuste por atalho vale só para a aba ativa ou para todas.

### Cores

**RF-5.11** — Configuráveis: as **16 cores ANSI** — oito normais e oito brilhantes.

**RF-5.12** — Configuráveis: cor de frente padrão, cor de fundo padrão.

**RF-5.13** — Configuráveis: cor do cursor e cor do texto sob o cursor.

**RF-5.14** — Configuráveis: cor de fundo e cor de texto da seleção. Os defaults (`#2e6b62` e `#eef2f4`) nasceram do `::selection` do canvas e são os que o binário desenha — logo, os valores deliberados ([ADR-0028](../adr/0028-o-binario-como-referencia-visual.md)). O requisito saiu da lista de "sem desenho aprovado" da especificação visual §4.2: não falta desenho, faltava só a chave.

**RF-5.15** — Configurável: opacidade do fundo do terminal, independente da opacidade da janela.

**RF-5.16** — Cores aceitam hexadecimal (`#rrggbb`, `#rrggbbaa`) e o valor `"transparent"`.

**RF-5.17** — Cores definidas pelo programa em execução — 256 cores e true color — funcionam sempre, sem configuração, e não são afetadas pela paleta.

### Temas nomeados

**RF-5.18** — O usuário define **temas nomeados** no arquivo de config, cada um com paleta completa, e seleciona o tema ativo por nome.

**RF-5.19** — Chaves de cor declaradas fora do tema têm precedência sobre as do tema selecionado. *(Permite adotar um tema e ajustar uma cor específica sem copiar a paleta inteira.)*

**RF-5.20** — Trocar o tema no arquivo aplica a mudança a quente em todas as abas abertas.

**RF-5.21** — O usuário troca de tema por atalho, ciclando entre os temas definidos, sem editar o arquivo. Atalho padrão `Ctrl+Shift+Y` — movido de `Ctrl+Shift+P`, que passou a abrir a paleta de comandos ([ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md)).

### Cursor

**RF-5.22** — Configurável: a forma do cursor — bloco, barra vertical ou sublinhado.

**RF-5.23** — Configuráveis: se o cursor pisca e o intervalo do piscar.

**RF-5.24** — Configurável: se o cursor de uma janela sem foco é desenhado vazado.

**RF-5.25** — Programa que muda a forma do cursor por sequência de escape (DECSCUSR) tem precedência sobre a config enquanto estiver em execução.

### Scrollback e rolagem

**RF-5.26** — Configurável: o número de linhas de scrollback por aba.

**RF-5.27** — Configurável: o multiplicador de linhas por passo da roda do mouse.

### Recarga

**RF-5.28** — Salvar o arquivo aplica mudanças de cor e fonte a todas as abas em menos de 500 ms, sem reiniciar e sem perder o scrollback.

**RF-5.29** — Mudança no tamanho da fonte ou na altura de linha recalcula a grade e redimensiona todos os PTYs, notificando os programas em execução.

## Critérios de aceite

```gherkin
Cenário: fallback de fonte cobre glyph ausente
  Dado a fonte principal sem glyphs de emoji
  E uma fonte de emoji na cadeia de fallback
  Quando o prompt exibe um emoji
  Então o glyph é desenhado pela fonte de fallback
  E nenhum retângulo vazio aparece

Cenário: fonte inexistente não impede a abertura
  Dado uma família de fonte inexistente na config
  Quando o usuário abre o app
  Então o app abre com uma monoespaçada do sistema
  E avisa qual família não foi encontrada

Cenário: paleta ANSI aplicada
  Dado uma paleta customizada na config
  Quando um programa emite texto em vermelho ANSI
  Então o texto usa a cor vermelha definida pelo usuário

Cenário: true color não é afetado pela paleta
  Dado qualquer paleta configurada
  Quando um programa emite uma cor RGB de 24 bits
  Então a cor exata emitida é exibida

Cenário: override sobre tema
  Dado o tema "catppuccin" selecionado
  E a cor de fundo declarada como "#000000" fora do tema
  Quando o app carrega a configuração
  Então todas as cores vêm do tema
  E o fundo é preto

Cenário: recarga a quente preserva o scrollback
  Dado uma aba com saída acumulada
  Quando o usuário muda a paleta e salva
  Então as cores mudam em todas as abas
  E o scrollback é preservado

Cenário: mudança de fonte redimensiona a grade
  Dado uma aba com "htop" em execução
  Quando o usuário aumenta o tamanho da fonte
  Então a grade é recalculada
  E o PTY é redimensionado
  E o "htop" se redesenha no novo tamanho

Cenário: programa controla a forma do cursor
  Dado a config definindo cursor em bloco
  Quando um programa emite DECSCUSR pedindo barra vertical
  Então o cursor é desenhado como barra
  E volta a bloco quando o programa encerra

Cenário: zoom por atalho não altera o arquivo
  Dado o tamanho de fonte 12 na config
  Quando o usuário aumenta a fonte por atalho
  Então a fonte aumenta na sessão
  E o arquivo de config permanece com 12
```

## Fora de escopo

- Temas distribuídos como arquivos separados e importáveis (v2 — no v1 os temas ficam no próprio arquivo de config)
- Detecção automática de tema claro/escuro do sistema (v2)
- Imagem de fundo no terminal
- Efeitos visuais de cursor (rastro, animação)
- Ajuste fino de renderização de glyph (hinting, subpixel) além do default da plataforma
- Protocolos de imagem inline, sixel ([ADR-0002](../adr/0002-motor-vte.md))

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Glyphs desenhados como retângulo vazio, com fallback configurado | zero |
| Tempo entre salvar a config e ver o resultado | < 500 ms |
| Scrollback perdido em recarga de config | zero linhas |
| Cores ANSI e de UI sem chave de config correspondente | zero |
| Fonte inexistente que impede a abertura do app | zero ocorrências |
