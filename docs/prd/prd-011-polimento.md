# PRD-011 — Polimento: busca, hyperlinks, acessibilidade e release

**Status:** Aprovado
**Data:** 2026-09-04
**Requisito de origem:** derivado da [visão de produto](prd-000-visao-de-produto.md) — o v1 tem de ser usável o dia inteiro, e distribuível
**Relacionados:** [ADR-0041](../adr/0041-busca-no-scrollback.md), [ADR-0042](../adr/0042-hyperlinks-osc-8.md), [ADR-0043](../adr/0043-arvore-de-acessibilidade.md), [ADR-0044](../adr/0044-empacotamento-e-release.md), [ADR-0001](../adr/0001-stack-de-gui.md), [ADR-0012](../adr/0012-identificacao-do-terminal.md), [ADR-0013](../adr/0013-mouse-selecao-e-clipboard.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [PRD-010](prd-010-interacao-e-superficie-de-app.md)

## Problema

Da F1 à F5 o Porecatu ganhou terminal, abas, grupos, configuração e sessão. O que falta não é recurso novo de organização — é o que separa **"funciona"** de **"usável o dia inteiro"**, mais o que separa **"compila aqui"** de **"instalável por outra pessoa"**.

Três ausências aparecem no uso diário de qualquer emulador:

- **Não há como achar nada.** Um `cargo build` que falhou trinta linhas atrás só se encontra rolando com o olho. O scrollback já guarda o conteúdo — falta a superfície.
- **Link na saída de um programa é texto morto.** O motor VT já reconhece a sequência OSC 8 e guarda o URI; o app o joga no chão.
- **Leitor de tela não vê nada.** A decisão de renderizar todo o chrome por GPU ([ADR-0001](../adr/0001-stack-de-gui.md)) tem esse custo declarado desde o começo: *"leitores de tela não enxergam pixels de GPU"*. É dívida assumida com fase própria, e esta é a fase.

E uma quarta, que não é de uso e sim de existência: **não há release.** O `release.yml` compila nas três plataformas e anexa o binário cru desde a F0, mas nenhuma versão foi publicada, e um executável solto não instala nada — o ícone que o `build.rs` embute não tem onde aparecer.

Este PRD também recolhe **requisitos aprovados de fases anteriores que não foram entregues**, achados ao auditar o código no fim da F5. Não são escopo novo; são dívida que estava fora de qualquer lista.

## Usuário-alvo

Os mesmos do [PRD-000](prd-000-visao-de-produto.md), em dois momentos diferentes:

- **quem já usa** — busca, hyperlink e menu do terminal são gestos de todo dia;
- **quem vai instalar** — o release e a documentação de usuário são o primeiro contato, e hoje não existem.

A acessibilidade tem um alvo próprio e menor, e é a razão de o requisito existir mesmo sem demanda registrada: sem árvore de acessibilidade, um usuário de leitor de tela não consegue nem descobrir quantas abas existem.

## Requisitos funcionais

### Busca no scrollback

**RF-11.1** — `search.open` abre a busca na aba ativa com o campo de texto já focado. `Esc` ou o botão de fechar encerram a busca, devolvem o foco ao terminal e limpam o realce. A busca é **por aba**: abrir busca numa aba não afeta as outras.

**RF-11.2** — A busca é **incremental**: cada caractere digitado recalcula o resultado, sem exigir `Enter`.

**RF-11.3** — O escopo é a tela visível **mais o scrollback inteiro** daquela aba — não só o que está em tela no momento.

**RF-11.4** — O padrão é **literal por default**, e o usuário liga expressão regular por um alternador na própria barra. Padrão de regex inválido não apaga o último resultado válido: a barra sinaliza o erro e mantém o realce anterior.

**RF-11.5** — `search.next` e `search.prev` andam entre as ocorrências **circulando nas duas pontas**, rolando a vista até a ocorrência ativa. `Enter` equivale a `search.next` e `Shift+Enter` a `search.prev`.

**RF-11.6** — A barra mostra a posição da ocorrência ativa e o total (`3/17`). Nenhuma ocorrência é um estado próprio, visualmente distinto de campo vazio.

**RF-11.7** — Todas as ocorrências dentro da vista são realçadas, e a ocorrência ativa tem realce **distinto** das demais.

**RF-11.8** — Com um programa em tela alternativa, o app não finge que a busca funcionou: não há scrollback a percorrer e a tela pertence ao programa, então a busca opera apenas sobre a tela visível e informa esse limite, em vez de devolver zero resultado sem explicação.

**RF-11.9** — Encontrar uma ocorrência numa aba que está dentro de um grupo colapsado **expande o grupo**. É a segunda das duas fontes que o RF-2.17 cita; a primeira (restauração de sessão) fechou na F5.

### Hyperlinks OSC 8

**RF-11.10** — O app reconhece a sequência OSC 8 e associa o URI às células que ela marca. Sem intenção declarada do usuário, essas células **não mudam de aparência** — a saída de um programa não se enfeita sozinha.

**RF-11.11** — Com o modificador de abertura pressionado, o link sob o cursor ganha sublinhado e o cursor do mouse muda de forma. É a única affordance, e ela é sob demanda.

**RF-11.12** — Com esse mesmo modificador, o clique segue o link. `http`, `https` e `mailto` abrem no handler padrão do sistema. `file` é **revelado no gerenciador de arquivos** — localizado e selecionado —, nunca entregue ao handler por extensão: o caminho veio da saída de um programa, e no Windows um `.exe` ou `.bat` executaria.

**RF-11.13** — Esquema fora dessa lista **não abre**: o URI é copiado para o clipboard e o app informa o que fez e por quê. O conteúdo vem da saída de um programa, e um clique não pode virar execução arbitrária. A lista de esquemas **não é configurável**; o recurso inteiro é desligável.

### Menu de contexto do terminal

**RF-11.14** — O clique secundário sobre a área do terminal abre um menu de contexto com copiar, colar, selecionar tudo e abrir a busca. Sobre um hyperlink, o menu traz também abrir e copiar o link.

**RF-11.15** — Item sem alvo aparece **esmaecido, não ausente** — a mesma regra do RF-10.20, que já vale para os menus de aba e de grupo. Copiar sem seleção é o caso óbvio.

**RF-11.16** — `selection.select_all` seleciona a tela visível **e** o scrollback, e é acionável tanto pelo menu quanto por tecla.

### Acessibilidade

**RF-11.17** — Um leitor de tela navega a barra de abas: cada aba anuncia título, posição, o grupo a que pertence e o estado relevante — ativa, com atividade, com campainha, não iniciada.

**RF-11.18** — Os **cinco** widgets de chrome do app expõem o papel que o leitor de tela espera de cada um: aviso, diálogo modal, menu de contexto, tooltip e editor de grupo. O diálogo modal se anuncia ao abrir e prende o foco enquanto está aberto.

**RF-11.19** — A **grade do terminal não é exposta** no v1. É limitação declarada, não esquecimento: expor conteúdo de terminal exige decidir granularidade de anúncio e como não inundar o leitor a cada byte do PTY, e isso é trabalho de tamanho próprio ([ADR-0043](../adr/0043-arvore-de-acessibilidade.md)).

### Release e empacotamento

**RF-11.20** — Cada plataforma suportada tem um **instalador nativo**, não apenas um binário solto: o app aparece no menu de aplicativos do sistema, com o ícone que já é embutido.

**RF-11.21** — Todo artefato publicado é **verificável** por checksum e carrega dentro de si a licença do projeto e a atribuição das fontes embutidas.

**RF-11.22** — Existe documentação de usuário cobrindo instalação, arquivo de configuração, atalhos, integração de shell e a convenção do `Shift` para selecionar texto dentro de um programa que pede o mouse — este último item é exigência explícita do [ADR-0013](../adr/0013-mouse-selecao-e-clipboard.md).

**RF-11.23** — A release publicada traz notas de versão legíveis por quem não acompanhou o desenvolvimento.

### Requisitos aprovados de fases anteriores, não entregues

Não são escopo novo. Cada um tem requisito aprovado desde a fase indicada, e nenhum estava registrado como dívida antes da auditoria que abriu esta fase.

**RF-11.24** — Os avisos de configuração que hoje só aparecem durante o hot reload passam a aparecer também **no arranque**: config inválida com linha e chave (RF-4.21), chave desconhecida (RF-4.22) e nome de tema inexistente (RF-5.18). Hoje eles vão para a saída de erro do processo, que ninguém lê.

**RF-11.25** — Fonte configurada e não encontrada no sistema **avisa qual família faltou** (RF-5.8). O fallback já funciona; o aviso nunca existiu.

**RF-11.26** — Ausência de aceleração de GPU é detectada e avisada no primeiro start, e o app abre com o backend disponível em vez de encerrar. É a mitigação de risco que o [ADR-0001](../adr/0001-stack-de-gui.md) registrou e que o [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md) já lista como consumidor do canal de aviso.

**RF-11.27** — O botão de configurações da barra de abas deixa de ser inerte: ele abre o arquivo de configuração do usuário no editor padrão do sistema.

**RF-11.28** — `zoom_scope = "active"` passa a ter efeito: com esse valor, o zoom de sessão vale só para a aba ativa (RF-5.10). Hoje o zoom é sempre do processo inteiro.

**RF-11.29** — O editor de grupo aceita cor por **hexadecimal**, além dos seis swatches da paleta (RF-2.10).

**RF-11.30** — `scrollback.to_top` e `scrollback.to_bottom` passam a responder. As duas têm default embutido (`Shift+Home`/`Shift+End`), entram no mapa resolvido e a operação existe em `porecatu-term` desde a F1 — o que falta é o despacho, que hoje as devolve como não tratadas.

## Critérios de aceite

```gherkin
Cenário: encontrar um erro rolado para fora da vista
  Dado uma aba com 200 linhas de saída no scrollback
  E a palavra "error" aparecendo em três linhas distintas
  Quando o usuário abre a busca e digita "error"
  Então a barra mostra 1/3
  E a vista rola até a primeira ocorrência
  E as três ocorrências ficam realçadas, com a ativa distinta

Cenário: circular entre as ocorrências
  Dado uma busca ativa mostrando 3/3
  Quando o usuário aciona a próxima ocorrência
  Então a barra mostra 1/3
  E a vista rola de volta para a primeira

Cenário: regex inválida não apaga o resultado bom
  Dado uma busca em modo de expressão regular mostrando 2/5
  Quando o usuário digita um padrão inválido
  Então a barra sinaliza o erro do padrão
  E as cinco ocorrências anteriores continuam realçadas

Cenário: fechar a busca limpa a tela
  Dado uma busca ativa com ocorrências realçadas
  Quando o usuário pressiona Esc
  Então o realce desaparece
  E o teclado volta a escrever no terminal

Cenário: buscar dentro de um grupo colapsado
  Dado um grupo colapsado cuja terceira aba contém a palavra buscada
  Quando o usuário busca essa palavra e vai até a ocorrência
  Então o grupo expande
  E a aba que contém a ocorrência fica ativa

Cenário: seguir um hyperlink
  Dado um programa que emitiu OSC 8 com uma URL https
  Quando o usuário passa o cursor sobre o texto sem modificador
  Então nada muda de aparência
  Quando o usuário pressiona o modificador de abertura
  Então o link ganha sublinhado
  E o clique abre a URL no navegador padrão

Cenário: esquema de URI não permitido
  Dado uma célula marcada com OSC 8 apontando para um esquema fora da lista
  Quando o usuário aciona o link
  Então nada é executado
  E o URI é copiado para o clipboard
  E o app informa que o esquema não é aberto

Cenário: link file não executa
  Dado uma célula marcada com OSC 8 apontando para um executável por file://
  Quando o usuário aciona o link
  Então o gerenciador de arquivos abre com o item selecionado
  E o executável não é executado

Cenário: menu de contexto sem seleção
  Dado o terminal sem nenhum texto selecionado
  Quando o usuário clica com o botão secundário na área do terminal
  Então o menu abre com copiar esmaecido
  E colar e selecionar tudo acionáveis

Cenário: leitor de tela na barra de abas
  Dado um leitor de tela ativo e três abas, uma delas num grupo chamado "api"
  Quando o usuário navega a barra pelo leitor
  Então cada aba é anunciada com título, posição e grupo
  E a aba ativa é anunciada como ativa

Cenário: diálogo modal anunciado
  Dado um leitor de tela ativo
  Quando o app pede confirmação para fechar uma aba com processo em primeiro plano
  Então o diálogo é anunciado ao abrir
  E o foco não sai dele até haver resposta

Cenário: instalar a partir do release
  Dado um usuário sem toolchain Rust
  Quando ele baixa o instalador da plataforma dele e o executa
  Então o app aparece no menu de aplicativos com o ícone próprio
  E abre sem exigir mais nenhum passo

Cenário: config inválida no arranque
  Dado um porecatu.toml com erro de sintaxe
  Quando o usuário abre o app
  Então o app abre com os defaults
  E exibe um aviso na janela citando o arquivo, a linha e a chave

Cenário: máquina sem aceleração de GPU
  Dado um ambiente sem GPU acelerada
  Quando o usuário abre o app
  Então o app abre pelo backend disponível
  E avisa uma vez que está sem aceleração
```

## Fora de escopo

- **Notificação de desktop na campainha.** Estava listada na F6 do [roadmap](../roadmap.md) e sai do v1: é superfície nativa do sistema, e o [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md) decidiu que nada da interface do app escapa da configuração do usuário. O indicador de campainha na aba (RF-1.21) é a superfície que o produto controla, e ela já existe.
- **Grade do terminal acessível** (RF-11.19) — declarada, com o porquê, no [ADR-0043](../adr/0043-arvore-de-acessibilidade.md).
- **Abrir arquivo por `file://`** — revelar é o comportamento, e é decisão de segurança ([ADR-0042](../adr/0042-hyperlinks-osc-8.md) §4), não limitação.
- **Assinatura de código** — certificado Authenticode e notarização Apple custam dinheiro e segredos no CI; o v1 sai sem, e a documentação avisa o usuário do que esperar ([ADR-0044](../adr/0044-empacotamento-e-release.md)).
- **Buscar em todas as abas ao mesmo tempo.** A busca é por aba (RF-11.1). Busca global é conversa de paleta de comandos, que é `[v2]`.
- **Substituir texto encontrado.** Um emulador não edita a saída de um programa.
- **Paleta de comandos** ([PRD-008](prd-008-paleta-de-comandos.md), rascunho, `[v2]`) — é superfície distinta da busca no scrollback, e o próprio PRD-008 já diz isso.
- **Persistir a busca na sessão.** Termo buscado é estado efêmero de janela, como a seleção múltipla do [ADR-0021](../adr/0021-selecao-multipla-e-gestos-da-barra.md).
- **Detecção de URL em texto plano** (sem OSC 8). Exigiria varrer a grade por regex a cada frame, e o custo de medir sem cache no caminho quente é a armadilha registrada deste projeto.

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Ações do catálogo sem origem em RF ou ADR | **zero** — este PRD é o que fecha |
| As cinco métricas do [PRD-000](prd-000-visao-de-produto.md), medidas por instrumentação | todas atingidas |
| Requisitos do v1 aprovados e não entregues | zero |
| Plataformas com instalador nativo publicado | 3 |
| Papéis de chrome invisíveis a leitor de tela | zero |
| Esquemas de URI executáveis por um clique | zero fora da lista de quatro |

A primeira e a terceira são a razão de este documento existir: a F6 era a única fase do v1 sem PRD, e foi essa ausência que deixou sete requisitos aprovados fora de qualquer lista de trabalho.
