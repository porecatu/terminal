# PRD-001 — Abas

**Status:** Aprovado
**Data:** 2026-08-26
**Requisito de origem:** 1 — interface em tabs, para gestão de múltiplos terminais na mesma janela
**Relacionados:** [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md), [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md), [PRD-002](prd-002-grupos-de-abas.md)

## Problema

Múltiplos terminais hoje significam múltiplas janelas — alt-tab entre elas, nenhuma noção de conjunto, e o gerenciador de janelas do SO como única ferramenta de organização. Abas resolvem o básico: um lugar, uma barra, navegação por teclado.

Abas são a fundação sobre a qual [PRD-002](prd-002-grupos-de-abas.md) (grupos) e [PRD-003](prd-003-persistencia-de-sessao.md) (sessão) são construídos. Sem elas, os outros dois não existem.

## Usuário-alvo

Todo usuário do Porecatu. Este é o recurso de base.

## Requisitos funcionais

### Ciclo de vida

**RF-1.1** — O usuário cria uma aba nova por atalho, por botão `+` na barra, ou por menu de contexto. A aba nova abre no diretório da aba ativa no momento da criação, não no home. *(Herdar o diretório é o comportamento que economiza mais digitação no dia a dia.)*

**RF-1.2** — O usuário fecha uma aba por atalho, por botão de fechar na aba, ou por clique do botão do meio. O shell recebe sinal de encerramento e o app aguarda EOF antes de remover a aba, para não perder a última saída ([ADR-0004](../adr/0004-pty-cross-platform.md)).

**RF-1.3** — Quando o shell de uma aba encerra por conta própria (`exit`, `Ctrl+D`), a aba fecha automaticamente. Se o processo encerrar com código diferente de zero, a aba permanece aberta exibindo o código de saída, até que o usuário a feche. *(Fechar uma aba que falhou esconde a mensagem de erro justamente quando ela importa.)*

**RF-1.4** — Fechar a última aba de uma janela fecha a janela. Fechar a última janela encerra o app, gravando a sessão de forma síncrona antes de sair.

**RF-1.5** — Ao fechar uma aba, o foco vai para a aba seguinte do mesmo grupo; não havendo, para a anterior do mesmo grupo; não havendo, para a aba mais próxima do grupo adjacente.

**RF-1.6** — Fechar uma aba com processo em primeiro plano diferente do shell (por exemplo `vim` ou `ssh`) pede confirmação. O comportamento é configurável e pode ser desligado.

### Identidade e título

**RF-1.7** — Cada aba exibe um título. A precedência é: título customizado definido pelo usuário → título vindo de OSC 0 / OSC 2 emitido pelo programa → nome do processo em primeiro plano → nome do shell.

**RF-1.8** — O usuário renomeia uma aba por atalho ou duplo clique no título. A renomeação é edição inline na própria aba, com `Enter` para confirmar e `Esc` para cancelar. Título customizado **congela** o título: OSC 0/2 posteriores não o sobrescrevem.

**RF-1.9** — Limpar o título customizado (renomear para vazio) devolve a aba ao comportamento automático.

**RF-1.10** — Título longo é truncado com reticências no fim, respeitando a largura mínima de aba da config ([PRD-004](prd-004-aparencia-do-chrome.md)). O título completo aparece em tooltip no hover.

### Navegação

**RF-1.11** — Navegação sequencial: próxima e anterior, circulando nas pontas. A ordem de navegação é a ordem visual, atravessando fronteiras de grupo.

**RF-1.12** — Acesso direto por índice: ir para a 1ª até a 9ª aba. O índice é sobre a ordem visual da janela toda, não por grupo.

**RF-1.13** — Clique numa aba a ativa. Grupos colapsados não participam da navegação sequencial ([PRD-002](prd-002-grupos-de-abas.md)).

**RF-1.14** — A aba ativa é visualmente inequívoca — não só por matiz de cor, para não depender de percepção cromática. Toda a superfície de estilo vem da config ([PRD-004](prd-004-aparencia-do-chrome.md)).

### Reordenação

**RF-1.15** — O usuário arrasta uma aba para reordená-la. Durante o arraste, as demais abas se deslocam mostrando onde ela vai cair.

**RF-1.16** — Arrastar uma aba para dentro dos limites visuais de um grupo a move para aquele grupo, na posição de queda. Arrastar para fora de todos os grupos a move para o grupo implícito ([ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md)).

**RF-1.17** — Reordenação também por teclado: mover a aba ativa uma posição para a esquerda ou direita.

### Overflow da barra

**RF-1.18** — Quando as abas não cabem, elas encolhem até a largura mínima configurada. Abaixo disso, a barra ganha rolagem horizontal, e a aba ativa é sempre trazida para a área visível.

**RF-1.19** — Um indicador mostra que há abas fora da vista, com contagem.

### Indicadores de estado

**RF-1.20** — Uma aba em segundo plano cuja saída mudou desde a última visita exibe um indicador de atividade. *(É o que permite deixar um build rodando em outra aba e perceber que terminou.)*

**RF-1.21** — Uma aba em segundo plano que emitiu campainha (BEL) exibe um indicador distinto do de atividade.

**RF-1.22** — Ambos os indicadores somem ao visitar a aba, e ambos são desligáveis na config.

## Critérios de aceite

```gherkin
Cenário: aba nova herda o diretório
  Dado que a aba ativa está em /home/user/projeto
  Quando o usuário cria uma aba nova
  Então a aba nova abre em /home/user/projeto

Cenário: título automático segue o programa
  Dado uma aba sem título customizado
  Quando o programa emite OSC 2 com "vim: main.rs"
  Então o título da aba passa a ser "vim: main.rs"

Cenário: título customizado congela
  Dado uma aba renomeada pelo usuário para "backend"
  Quando o programa emite OSC 2 com "vim: main.rs"
  Então o título da aba continua "backend"

Cenário: saída com erro mantém a aba aberta
  Dado uma aba com um comando em execução
  Quando o processo encerra com código 1
  Então a aba permanece aberta exibindo o código de saída

Cenário: foco após fechar
  Dado três abas no mesmo grupo, com a do meio ativa
  Quando o usuário fecha a aba ativa
  Então a terceira aba passa a ser a ativa

Cenário: arrastar para dentro de um grupo
  Dado uma aba fora de qualquer grupo
  Quando o usuário a arrasta para dentro dos limites do grupo "api"
  Então a aba passa a pertencer ao grupo "api" na posição de queda

Cenário: overflow rola até a aba ativa
  Dado abas suficientes para estourar a largura da barra
  Quando o usuário ativa por teclado uma aba fora da vista
  Então a barra rola até torná-la visível

Cenário: indicador de atividade
  Dado uma aba em segundo plano
  Quando um comando nela produz saída
  Então a aba exibe indicador de atividade
  E o indicador some quando o usuário a visita

Cenário: confirmação ao fechar com processo ativo
  Dado uma aba com "vim" em primeiro plano
  Quando o usuário fecha a aba
  Então o app pede confirmação antes de encerrar
```

## Fora de escopo

- Mover abas entre janelas por drag (o modelo permite; a UI fica para depois)
- Splits / panes dentro da aba ([PRD-000](prd-000-visao-de-produto.md))
- Duplicar aba com o estado do processo
- Fixar aba ("pin")
- Miniatura de preview no hover
- Perfis de aba (aba que abre WSL, SSH, container)

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Tempo de criação de aba até prompt | < 150 ms |
| Custo de aba em segundo plano ociosa | zero CPU, só memória de grid |
| Latência de troca de aba | dentro de um intervalo de frame |
| Abas simultâneas sem degradação perceptível | 50 |
