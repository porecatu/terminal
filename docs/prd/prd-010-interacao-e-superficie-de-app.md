# PRD-010 — Interação com o terminal e superfície do app

**Status:** Aprovado
**Data:** 2026-08-26
**Requisito de origem:** nenhum dos cinco originais — consolidação de comportamentos que os ADR-0013 a 0016 decidiram e que nenhum PRD abrigava
**Relacionados:** [ADR-0013](../adr/0013-mouse-selecao-e-clipboard.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0015](../adr/0015-multiplas-janelas.md), [ADR-0016](../adr/0016-fontes-embutidas.md), [PRD-001](prd-001-abas.md), [PRD-004](prd-004-aparencia-do-chrome.md), [PRD-005](prd-005-aparencia-do-terminal.md)

## Problema

Cinco requisitos de produto geraram cinco PRDs, e os ADRs correspondentes decidiram *como* atendê-los. Ao fechar as lacunas que travavam a F1 e a F2, os ADR-0013 a 0016 decidiram também um conjunto de comportamentos **visíveis ao usuário** que nenhum dos cinco requisitos originais previa: o que o duplo clique seleciona, quem ganha o mouse quando um programa o solicita, onde um erro de configuração aparece, como uma segunda janela nasce.

Isso funciona, mas cria um problema de procedência que cresce com o projeto. O checklist de PR exige que *"toda chave nova tenha default e apareça em algum PRD"*, e a métrica do [PRD-004](prd-004-aparencia-do-chrome.md) exige **zero** chaves no arquivo de exemplo sem requisito correspondente. Chaves como `copy_on_select`, `osc52_read` e `confirm_close_window` rastreavam a ADRs — o que é defensável, mas deixa "onde está o requisito de X" com duas respostas possíveis. Com contribuidor externo, duas respostas possíveis é uma resposta faltando.

Este PRD **não decide nada de novo**. Ele reformula como requisito o que os ADRs já decidiram, para que a decisão técnica fique no ADR e o comportamento esperado fique onde os outros comportamentos estão. Onde este documento e um ADR discordarem, é erro de transcrição deste documento — o ADR vence.

## Usuário-alvo

Todo usuário. Nada aqui é recurso opcional: é a camada de interação que os cinco recursos do [PRD-000](prd-000-visao-de-produto.md) assumem existir.

## Relação com o escopo do v1

Não é um sexto recurso. É transversal aos cinco, e por isso não entra na tabela de escopo do [PRD-000](prd-000-visao-de-produto.md). A numeração vem depois dos rascunhos de v2 (PRD-006 a 009) apenas porque é sequencial, não temática.

---

## Requisitos funcionais

### Mouse

**RF-10.1** — Programa que solicita eventos de mouse os recebe: modos 1000 (clique), 1002 (arraste) e 1003 (movimento), com encoding SGR 1006 preferido e X10 como fallback.

**RF-10.2** — `Shift` pressionado força a **seleção local**, sempre, mesmo com o programa solicitando o mouse. Não é configurável. *(É o que permite copiar uma linha de dentro do `htop` sem sair dele.)*

**RF-10.3** — A barra de abas nunca repassa evento de mouse ao terminal. O clique do meio numa aba a fecha ([PRD-001](prd-001-abas.md) RF-1.2) e não disputa com nada, porque a área é outra.

### Seleção

**RF-10.4** — Gestos de seleção na área do terminal: arraste seleciona caractere a caractere; duplo clique seleciona palavra; triplo clique seleciona a linha lógica; `Alt` + arraste seleciona retângulo.

**RF-10.5** — Configurável: os caracteres que delimitam palavra no duplo clique.

**RF-10.6** — Ao copiar, o espaço em branco à direita é removido, e a linha que foi apenas quebrada pela largura da janela é remontada **sem** quebra de linha. *(Colar um caminho longo não pode produzir dois comandos.)*

**RF-10.7** — A seleção é limpa por input de teclado e por escrita do programa que toque a região selecionada. Rolagem pura **preserva** a seleção.

**RF-10.8** — Configurável, desligado por default: selecionar já copia para o clipboard.

**RF-10.9** — A seleção PRIMARY do X11 e do Wayland — colar com o botão do meio — **não** existe no v1. É limitação declarada, não defeito.

### Clipboard

**RF-10.10** — O programa em execução pode **escrever** no clipboard do sistema por OSC 52. Configurável, ligado por default, com teto de tamanho no payload. *(Atende copiar de dentro de um `tmux` ou `nvim` rodando por SSH.)*

**RF-10.11** — O programa em execução **não pode ler** o clipboard do sistema. Configurável, **desligado** por default, e o arquivo de exemplo declara o risco: processo remoto lendo o clipboard local expõe o que o usuário acabou de copiar, sem que ele tenha como saber.

### Rolagem

**RF-10.12** — O usuário rola o scrollback por teclado: linha, tela, início e fim.

**RF-10.13** — Configuráveis: se saída nova rola até o fim (desligado por default, para não arrancar o usuário de onde ele lia) e se digitar rola até o fim (ligado por default, porque é onde está o prompt).

**RF-10.14** — Na tela alternativa não existe scrollback: as ações de rolagem não fazem nada e, com `alternate_scroll` ligado por default, a roda do mouse é traduzida em setas. *(É o que faz `less` e `man` rolarem com a roda.)*

### Avisos

**RF-10.15** — Mensagens do app usam **dois canais distintos**:

| Canal | O que recebe |
|---|---|
| Aviso do app, empilhado sobre a área de conteúdo | erro de config com linha e chave (RF-4.21), chave desconhecida (RF-4.22), fonte ausente (RF-5.8), sessão corrompida (RF-3.14), schema mais novo (RF-3.16), convite de integração de shell (RF-3.1), fallback de GPU |
| Nota na aba, primeira linha do grid | diretório gravado inexistente (RF-3.10), código de saída de processo que falhou (RF-1.3) |

O critério é a quem o fato pertence: informação sobre o app não pode ser apagada por um `clear` na aba ativa, e informação sobre um terminal precisa sobreviver à rolagem e continuar lá quando o usuário voltar.

**RF-10.16** — No máximo três avisos convivem na tela. Erro e aviso persistem até dispensa; informação desaparece sozinha. `Esc` dispensa o do topo.

**RF-10.17** — Nenhuma mensagem do app usa diálogo ou notificação **nativa do sistema**. *(Diálogo nativo bloqueia o loop de eventos e é a única superfície que a config do usuário não alcançaria, contrariando o princípio 2 do [PRD-004](prd-004-aparencia-do-chrome.md).)*

### Confirmação

**RF-10.18** — Todo diálogo de confirmação abre com o foco no **cancelar**. `Enter` aciona o botão focado, `Esc` cancela. *(Diálogo destrutivo que confirma no `Enter` transforma distração em perda de trabalho.)*

Os diálogos existentes são os de RF-1.6 (aba com programa de tela cheia, conforme o [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md)), RF-2.23 (fechar grupo, com contagem) e RF-10.23 (fechar janela com mais de uma aba). *(A primeira versão deste parágrafo citava RF-10.20 no lugar de RF-10.23; RF-10.20 é o item esmaecido do menu.)*

### Menu de contexto

**RF-10.19** — Existem menus de contexto de aba (RF-1.1, RF-1.2, RF-2.20), de grupo (RF-2.22) e de terminal. Abrem ancorados no cursor, viram nos dois eixos para caber na tela, são navegáveis por teclado e fecham em clique fora ou perda de foco.

**RF-10.20** — Item indisponível aparece **esmaecido, nunca ausente**. *(Menu que muda de tamanho a cada abertura obriga a reaprender a posição dos itens.)*

**RF-10.21** — O menu de contexto do grupo e o editor de grupo oferecem exatamente a mesma lista de ações, lida de uma definição única.

### Janelas

**RF-10.22** — O usuário abre uma janela nova por atalho. Ela nasce com uma aba, herdando o diretório da aba ativa — mesma razão do RF-1.1.

**RF-10.23** — Cada janela tem seu próprio conjunto de abas e grupos, independente das outras. Fechar uma janela com mais de uma aba pede confirmação; o comportamento é configurável.

**RF-10.24** — Arrastar aba entre janelas **não** existe no v1, como já registram [PRD-000](prd-000-visao-de-produto.md) e PRD-001. Fechar a última janela encerra o app gravando a sessão de forma síncrona (RF-1.4).

### Aparência padrão

**RF-10.25** — A aparência padrão do app **não depende de fonte instalada no sistema**: as faces que o design especifica acompanham o app. É o que torna verificável a afirmação de que o binário com a config padrão confere com o mockup ([ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md)).

Glyphs fora dessas faces — emoji, ícones de Nerd Font, CJK — continuam vindo do sistema, pela cadeia de fallback do RF-5.2.

---

## Critérios de aceite

```gherkin
Cenário: Shift vence o programa que pediu o mouse
  Dado o "htop" em execução, com reporte de mouse ativo
  Quando o usuário arrasta com Shift pressionado sobre a saída
  Então o texto é selecionado localmente
  E nenhum evento de mouse é enviado ao programa

Cenário: clique sem Shift vai para o programa
  Dado o "htop" em execução, com reporte de mouse ativo
  Quando o usuário clica numa linha da lista
  Então o evento é entregue ao programa
  E nenhuma seleção local é criada

Cenário: linha quebrada pela largura é copiada como uma linha
  Dado um caminho longo que a janela quebrou em duas linhas visuais
  Quando o usuário seleciona a linha por triplo clique e copia
  Então o clipboard contém uma única linha, sem quebra

Cenário: rolagem preserva a seleção
  Dado um texto selecionado no scrollback
  Quando o usuário rola a visualização
  Então a seleção continua ativa

Cenário: programa escreve no clipboard, não lê
  Dado um programa que emite OSC 52 de escrita
  Quando a sequência é recebida
  Então o conteúdo vai para o clipboard do sistema
  Mas uma requisição de leitura por OSC 52 é negada

Cenário: roda do mouse na tela alternativa
  Dado o "less" em execução
  Quando o usuário gira a roda do mouse
  Então o conteúdo rola, porque a roda foi traduzida em setas

Cenário: erro de config é aviso do app, não da aba
  Dado o app aberto com duas abas
  Quando o usuário salva uma configuração inválida
  Então um aviso do app exibe arquivo, linha e chave
  E nenhuma aba tem seu conteúdo alterado
  E um "clear" na aba ativa não apaga o aviso

Cenário: diretório ausente é nota na aba
  Dado uma sessão gravada com uma aba em um diretório que não existe mais
  Quando a sessão é restaurada
  Então a primeira linha daquela aba informa o ocorrido
  E a informação continua visível depois de rolar e voltar

Cenário: confirmação abre no cancelar
  Dado uma aba com "vim" em primeiro plano
  Quando o usuário fecha a aba e o diálogo aparece
  E pressiona Enter sem mover o foco
  Então a ação é cancelada e a aba permanece aberta

Cenário: item indisponível é esmaecido
  Dado uma aba fora de qualquer grupo
  Quando o usuário abre o menu de contexto da aba
  Então o item de desagrupar aparece esmaecido
  E não desaparece do menu

Cenário: janela nova herda o diretório
  Dado que a aba ativa está em /home/user/projeto
  Quando o usuário abre uma janela nova
  Então ela abre com uma aba em /home/user/projeto
  E com seu próprio conjunto de abas e grupos

Cenário: aparência padrão sem fonte instalada
  Dado um sistema sem nenhuma fonte do design instalada
  Quando o usuário abre o app com a configuração padrão
  Então a barra de abas e o terminal usam as faces do design
  E nenhum aviso de fonte ausente é exibido
```

## Fora de escopo

- Seleção PRIMARY do X11/Wayland e colar com botão do meio (RF-10.9)
- Leitura de clipboard pelo programa, ligada por default (RF-10.11)
- Arrastar aba entre janelas (RF-10.24)
- Notificação de desktop do sistema operacional na campainha — está na F6 do [roadmap](../roadmap.md), não aqui
- Menu de contexto do terminal com busca e hyperlink, que dependem da F6
- Protocolo de mouse por pixel (modo 1016)

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Chaves do arquivo de exemplo sem requisito correspondente | **zero** |
| Ações do catálogo sem origem em RF ou ADR | zero |
| Diálogos ou notificações nativas do sistema no binário | zero |
| Perda de trabalho por confirmação acionada sem intenção | zero |
| Máquinas em que a config padrão divergiu do mockup por falta de fonte | zero |
