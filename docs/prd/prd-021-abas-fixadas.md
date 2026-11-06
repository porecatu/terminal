# PRD-021 — Abas fixadas

**Status:** Aprovado
**Data:** 2026-11-06
**Requisito de origem:** pedido direto do dono do produto — e o item que o [PRD-001](prd-001-abas.md) deixou escrito em "Fora de escopo": *"Fixar aba ("pin")"*, com o reflexo em [acoes.md](../reference/acoes.md), que lista `tab.pin` entre as ações que não existem
**Relacionados:** [ADR-0005](../adr/0005-persistencia-de-sessao.md), [ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0015](../adr/0015-multiplas-janelas.md), [ADR-0032](../adr/0032-interface-do-v1-fechada.md), [ADR-0036](../adr/0036-formato-do-arquivo-de-sessao.md), [ADR-0037](../adr/0037-aba-nao-iniciada.md), [ADR-0051](../adr/0051-arquivo-de-projeto-porecatu.md), [ADR-0053](../adr/0053-paineis-divididos.md), [ADR-0054](../adr/0054-sessoes-nomeadas.md), [ADR-0079](../adr/0079-botao-de-menu-do-app.md), [ADR-0083](../adr/0083-gap-entre-os-botoes-da-zona-fixa.md), [PRD-001](prd-001-abas.md), [PRD-002](prd-002-grupos-de-abas.md), [PRD-003](prd-003-persistencia-de-sessao.md), [PRD-006](prd-006-paineis-divididos.md), [PRD-012](prd-012-comando-de-projeto-por-diretorio.md), [PRD-014](prd-014-sessoes-nomeadas.md), [PRD-020](prd-020-conexoes-ssh.md)

> Aprovado em 2026-11-06, por decisão do dono do produto, **fora da ordem de fases**, como os outros recursos pedidos pelo dono depois da F6. As decisões de comportamento abaixo foram tomadas com o dono antes da escrita; o como fica em três ADRs — modelo, arquivo e ciclo de vida ([ADR-0086](../adr/0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md)), a zona na barra de abas ([ADR-0087](../adr/0087-zona-de-abas-fixadas-na-barra.md)) e o ícone ([ADR-0088](../adr/0088-icone-da-aba-fixada.md)). Este PRD muda a anatomia da barra, que a [interface do v1 fechada](../adr/0032-interface-do-v1-fechada.md) só permite com ADR: é o [ADR-0087](../adr/0087-zona-de-abas-fixadas-na-barra.md) que abre essa porta, com o aval visual do dono pendente até a aparência existir no binário.

## Problema

O usuário tem programas que quer **sempre** abertos quando o Porecatu está aberto — um servidor de desenvolvimento, um `htop`, um cliente de banco, uma conexão SSH com o servidor de sempre. Hoje eles vivem em abas comuns, e isso falha de três jeitos:

- **Eles dependem da sessão.** A aba volta no arranque só porque a sessão automática a gravou ([PRD-003](prd-003-persistencia-de-sessao.md)). Fechada sem querer, some na próxima gravação. Com `[session] enabled = false`, nunca volta. Abrir o app com um caminho (`porecatu <caminho>`, RF-3.12) não a traz.
- **Eles disputam lugar com o trabalho.** Ficam no meio da trilha, entre grupos de projeto, rolam para fora da vista quando a trilha enche, e entram nos grupos por acidente num arraste.
- **Eles não sobem sozinhos sem configurar confiança.** O `.porecatu` ([PRD-012](prd-012-comando-de-projeto-por-diretorio.md)) é o jeito de uma aba restaurada subir o programa, mas exige o diretório em `[project_file] trusted_paths`, editado à mão — e com `lazy_restore` ligado, só roda quando o usuário visita a aba.

O que falta é um lugar para **"os apps que abrem junto com o Porecatu"**: fora dos grupos, sempre à vista, e independente do que a sessão lembra.

## Usuário-alvo

O mesmo do [PRD-000](prd-000-visao-de-produto.md), no caso em que ele tem um punhado de programas de longa duração que acompanham **todo** o dia de trabalho, qualquer que seja o projeto.

**Não é para** organizar trabalho de projeto: para isso há grupos ([PRD-002](prd-002-grupos-de-abas.md)) e sessões nomeadas ([PRD-014](prd-014-sessoes-nomeadas.md)). Quem não fixa nenhuma aba não vê nada novo, nem um pixel.

## Em uma tela

As abas fixadas ficam numa zona logo à direita do botão do app, quadradas, sem rótulo, cada uma com um ícone:

```
 [◆] [▣][▣][▣]  ( api ▾  [aba]  [aba] ) [aba]  [+]  …          [▣] [⚙]   ─  □  ✕
  │   └─────── abas fixadas (ícone, sem rótulo)
  └ botão do app
```

O ícone de cada fixada sai do diretório dela: o declarado no `.porecatu`, senão um `icon.png` no diretório, senão o próprio ícone do app.

## Requisitos funcionais

### Forma e lugar

**RF-21.1** — Uma aba fixada é desenhada como uma **caixa quadrada**, do tamanho do botão do app ([ADR-0079](../adr/0079-botao-de-menu-do-app.md)), **sem rótulo** e com um **ícone** centrado (RF-21.17). Nenhuma dimensão nova é inventada: a anatomia — tamanho, espaço entre as caixas, estados ativo, inativo, hover e pressionado — é do ADR-0087.

**RF-21.2** — As fixadas moram numa **zona própria**, logo à direita do botão do app (ou, com `show_app_menu_button = false`, no lugar dele), **antes** da trilha de grupos e abas. A zona **não rola** com a trilha. Com muitas fixadas, a zona cresce e a trilha rolável encolhe; a zona em si nunca rola.

**RF-21.3** — Uma aba fixada **não pertence a grupo nenhum**, nem ao grupo implícito ([ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md)). Ela não entra em seleção múltipla (RF-2.1), não é movida para grupo pelo menu (RF-2.20) e não é alcançada por "fechar todas as abas do grupo" (RF-2.23).

**RF-21.4** — No resto, a fixada é uma aba como as outras: clicar a ativa e mostra o terminal dela; tem painéis ([PRD-006](prd-006-paineis-divididos.md)), busca, seleção, perfil ([PRD-007](prd-007-perfis-de-aba.md)). Na navegação sequencial e no acesso por índice (RF-1.11, RF-1.12), as fixadas vêm **primeiro**, na ordem da zona.

**RF-21.5** — A aba ativa continua inequívoca (RF-1.14) quando é uma fixada: a caixa ativa se distingue das inativas — não só por matiz.

**RF-21.6** — Sem rótulo, a fixada se identifica por três meios:
- **tooltip** com o título da aba no hover (o título segue a precedência do RF-1.7);
- o **ponto** de atividade e de campainha (RF-1.20, RF-1.21), sobreposto à caixa, com as mesmas cores e a mesma regra de "campainha vence atividade";
- um **nó de acessibilidade** com o título da aba.

### Fixar e desafixar

**RF-21.7** — O usuário fixa uma aba por três caminhos: o item **"Fixar aba"** do menu de contexto da aba; a ação **`tab.toggle_pin`**, sem atalho default (precedente de `group.new_tab`); e **arrastar** a aba da trilha para dentro da zona de fixadas.

**RF-21.8** — Fixar **não reinicia nada**: o processo continua vivo, e o `.porecatu` não roda nesse momento. A aba sai do grupo em que estava — e o grupo que fica vazio é removido, como manda o RF-2.7 — e vai para o **fim** da zona, ou para a posição de queda quando veio por arraste.

**RF-21.9** — Fixar grava um **retrato** da aba naquele momento: a árvore de painéis com as proporções, o diretório, o programa e o perfil de cada painel, qual painel estava focado, e o título customizado, quando houver — o mesmo conjunto que a sessão grava para uma aba (RF-6.22, RF-7.16). O que muda depois — um `cd`, um painel novo, um painel fechado — vale **só nesta execução** e não altera o retrato. Para atualizá-lo, o usuário desafixa e fixa de novo.

**RF-21.10** — O usuário desafixa pelo item **"Desafixar aba"** do menu de contexto da fixada, pela mesma ação `tab.toggle_pin`, ou **arrastando** a fixada para fora da zona. Desafixar **não fecha** a aba: o processo continua vivo, a aba vira aba solta no **início** da trilha — ou entra no grupo onde foi solta, por arraste (RF-1.16) — e sai da lista de fixadas.

**RF-21.11** — Dentro da zona, o usuário reordena as fixadas por arraste e pela reordenação por teclado (RF-1.17). Mover por teclado **não atravessa** a fronteira da zona: a primeira fixada não vai para a trilha, nem a primeira aba da trilha vai para a zona. Atravessar é fixar ou desafixar, e só acontece por gesto explícito (RF-21.7, RF-21.10).

**RF-21.12** — Item do menu de contexto que não se aplica a uma fixada — mover para grupo, criar grupo com a seleção, renomear inline — aparece **esmaecido**, nunca ausente, como todo item indisponível ([especificação visual](../design/especificacao-visual.md) §2.16).

### Abertura

**RF-21.13** — As abas fixadas abrem **uma vez por execução do app, na primeira janela do arranque**, antes das abas da sessão, na ordem da lista. Isso vale:
- com a sessão automática restaurada (RF-3.7), ligada ou desligada (`[session] enabled`, RF-3.6);
- com o app aberto por um caminho (`porecatu <caminho>`, RF-3.12);
- com a sessão inválida ou de schema mais novo (RF-3.14, RF-3.16).

Janelas abertas depois — `window.new` ou uma sessão nomeada ([PRD-014](prd-014-sessoes-nomeadas.md)) — **não** abrem as fixadas. As fixadas são apps: abri-las duas vezes subiria o mesmo servidor duas vezes, com o segundo morrendo por porta em uso.

**RF-21.14** — A aba que fica **ativa** no arranque continua sendo a que a sessão diz (ou a aba nova do RF-3.12). Abrir as fixadas não rouba o foco.

**RF-21.15** — Toda fixada **sobe o processo no arranque**, mesmo com `lazy_restore = true` (RF-3.8): são apps, e app que só abre quando visitado não é app aberto junto com o Porecatu. A exceção é a fixada **SSH**, que volta não iniciada e conecta no clique ou no `Enter`, como todo painel SSH restaurado (RF-20.33) — conectar sozinho no arranque pediria segredo e decisão de chave de host sem o usuário estar olhando.

**RF-21.16** — Na abertura, o `.porecatu` do diretório da fixada **roda sempre**, mesmo com o diretório **fora** de `[project_file] trusted_paths`: fixar a aba é o gesto de confiança. Todo o resto do [PRD-012](prd-012-comando-de-projeto-por-diretorio.md) continua valendo:
- `[project_file] enabled = false` desliga o script também na fixada (RF-12.7);
- uma vez por aba, no painel focado (RF-12.9, RF-6.24);
- só quando o shell está pronto, visível, no histórico e interrompível (RF-12.8, RF-12.10);
- diretório que não existe mais não roda nada (RF-12.14);
- fixada SSH não roda `.porecatu`: o diretório dela é remoto.

### Ícone

**RF-21.17** — O ícone da fixada é escolhido nesta ordem, no **diretório gravado ao fixar** (o do painel focado, RF-21.9):
1. o arquivo apontado pela seção **`[icon]`** do `.porecatu` daquele diretório;
2. senão, um arquivo **`icon.png`** no diretório;
3. senão, o **ícone do app**, o mesmo do botão do app.

Fixada SSH, e fixada cujo diretório não existe mais, usam o ícone do app.

**RF-21.18** — A seção `[icon]` do `.porecatu` tem como corpo o **caminho** da imagem: a primeira linha não vazia. Caminho relativo é resolvido a partir do diretório do `.porecatu`; `~` é expandido. `[icon]` é um nome **reservado**: não é um shell, e `[default]` não a substitui. O formato exato é do [ADR-0088](../adr/0088-icone-da-aba-fixada.md), e a [referência do arquivo de projeto](../reference/arquivo-de-projeto.md) ganha a seção.

**RF-21.19** — `[icon]` é lido **com ou sem** o diretório em `trusted_paths` (ler uma imagem não executa nada), mas **não** com `[project_file] enabled = false`, que desliga toda leitura do `.porecatu` (RF-12.7). Nesse caso, o `icon.png` continua valendo: ele não é parte do `.porecatu`.

**RF-21.20** — Os formatos aceitos são os que a imagem de fundo já decodifica ([PRD-017](prd-017-imagem-de-fundo-do-terminal.md)). Arquivo ausente, ilegível ou em formato não suportado passa ao próximo item da ordem do RF-21.17, **sem aviso**: o ícone do app no lugar já é o sinal visível de que o ícone declarado não foi usado.

**RF-21.21** — O ícone é resolvido **no arranque e ao fixar**, e em nenhum outro momento. Trocar o `icon.png` com o app aberto não muda a aba até a próxima execução. A decodificação **não bloqueia** a main thread: até o ícone ficar pronto, a caixa mostra o ícone do app.

### Fechar e encerrar

**RF-21.22** — **Fechar uma aba fixada a desafixa.** Por atalho, menu ou clique do meio (RF-1.2), fechar encerra o processo, remove a aba e a tira da lista. A fixada não tem botão de fechar desenhado.

**RF-21.23** — Fechar uma fixada **sempre pede confirmação**, com ou sem processo ativo, e a confirmação não é configurável — como a de fechar todas as abas de um grupo (RF-2.23). O diálogo diz que a aba sai das fixadas; com processo em primeiro plano (RF-1.6), diz isso também, **no mesmo diálogo**. O foco inicial é no cancelar, como em todo diálogo do app.

**RF-21.24** — Quando o processo de uma fixada termina sozinho (`exit`, `Ctrl+D`, conexão encerrada), a aba **não fecha** — ao contrário do RF-1.3 para abas comuns, mesmo com código de saída zero. Ela fica no estado encerrado, com a caixa distinguível de uma fixada viva, até o usuário fechá-la (o que a desafixa, RF-21.22) ou até a próxima execução, que a abre de novo. Um `exit` digitado não pode custar a lista de fixadas.

**RF-21.25** — **Fechar a janela** que tem as fixadas **não** as desafixa: elas encerram junto com a janela (com a confirmação do RF-10.23) e voltam no próximo arranque. Fechar a última aba **comum** de uma janela que ainda tem fixadas **não** fecha a janela (RF-1.4): as fixadas contam como abas, e o foco vai para a última fixada.

**RF-21.26** — Fixar uma aba numa janela que não é a do arranque é permitido: a aba vai para a zona **daquela** janela, e a partir do próximo arranque abre na primeira, com as demais.

### Arquivo

**RF-21.27** — A lista de fixadas vive num **arquivo próprio**, no diretório de estado do app, ao lado do `session.json` ([ADR-0005](../adr/0005-persistencia-de-sessao.md)) — não na config, porque carrega `cwd` absoluto da máquina. Ela é **independente** da sessão: `[session] enabled` não a alcança, e nem a sessão automática nem as sessões nomeadas gravam as fixadas (senão elas abririam duas vezes, uma pela lista e outra pela sessão).

**RF-21.28** — O arquivo é gravado a cada fixar, desafixar e reordenar, de forma atômica (RF-3.5). Gravação que falha vira **aviso** do app com a razão.

**RF-21.29** — Arquivo ausente é uma lista vazia, não erro. Arquivo inválido é preservado com `.corrupt` e o app abre sem fixadas, avisando (RF-3.14). Arquivo de schema mais novo **nunca é sobrescrito** (RF-3.16): o app abre sem fixadas, avisa, e fixar nesta execução também avisa em vez de gravar.

**RF-21.30** — Diretório gravado que não existe mais abre a fixada no diretório inicial com a nota de sempre (RF-3.10) e **não** a remove da lista: o diretório pode ser de um disco que só não está montado agora.

## Cenários

```gherkin
Cenário: fixar uma aba pelo menu de contexto
  Dado uma aba no grupo "api" rodando "npm run dev" em ~/Projetos/api
  Quando o usuário escolhe "Fixar aba" no menu de contexto dela
  Então a aba aparece como caixa quadrada na zona à direita do botão do app
  E sai do grupo "api"
  E o processo continua rodando, sem ser reiniciado

Cenário: a fixada abre no próximo arranque, independente da sessão
  Dado uma aba fixada em ~/Projetos/api
  E [session] enabled = false
  Quando o usuário fecha e reabre o app
  Então a primeira janela abre com a fixada na zona
  E o .porecatu de ~/Projetos/api roda nela

Cenário: o .porecatu roda sem trusted_paths
  Dado uma aba fixada em ~/Projetos/api, com um .porecatu
  E ~/Projetos/api fora de [project_file] trusted_paths
  Quando o app é reaberto
  Então o comando do .porecatu é escrito no prompt da fixada
  E nenhuma nota de "não autorizado" aparece

Cenário: lazy_restore não segura a fixada
  Dado [session] lazy_restore = true
  E uma aba fixada em ~/Projetos/api
  Quando o app é reaberto
  Então a fixada sobe o shell e roda o .porecatu sem ser visitada
  E a aba ativa da sessão continua sendo a ativa

Cenário: janela nova não duplica as fixadas
  Dado o app aberto, com uma fixada rodando um servidor na porta 3000
  Quando o usuário abre uma janela nova
  Então a janela nova não tem abas fixadas

Cenário: ícone declarado no .porecatu
  Dado ~/Projetos/api com um .porecatu cuja seção [icon] contém "assets/logo.png"
  E ~/Projetos/api/assets/logo.png existe
  Quando o usuário fixa uma aba em ~/Projetos/api
  Então a caixa da fixada mostra logo.png

Cenário: icon.png como segunda opção
  Dado ~/Projetos/web sem seção [icon] no .porecatu e com um icon.png
  Quando o usuário fixa uma aba em ~/Projetos/web
  Então a caixa mostra o icon.png

Cenário: sem ícone nenhum
  Dado ~/Projetos/scripts sem .porecatu e sem icon.png
  Quando o usuário fixa uma aba ali
  Então a caixa mostra o ícone do app

Cenário: o cd depois de fixar não muda nada
  Dado uma aba fixada em ~/Projetos/api
  Quando o usuário dá cd ~/Projetos/web nela
  E reabre o app
  Então a fixada abre em ~/Projetos/api, com o ícone de ~/Projetos/api

Cenário: fechar desafixa, com confirmação
  Dado uma aba fixada
  Quando o usuário tecla o atalho de fechar aba nela
  Então um diálogo diz que a aba sai das fixadas, com o foco em cancelar
  E confirmar encerra o processo e remove a aba da zona e da lista

Cenário: o processo sai sozinho
  Dado uma aba fixada rodando um shell
  Quando o usuário digita exit
  Então a caixa continua na zona, no estado encerrado
  E a aba continua na lista de fixadas

Cenário: desafixar não fecha
  Dado uma aba fixada rodando "htop"
  Quando o usuário escolhe "Desafixar aba"
  Então a aba vira aba solta no início da trilha, com o rótulo de volta
  E "htop" continua rodando
  E a aba não abre mais no próximo arranque

Cenário: arrastar para dentro e para fora da zona
  Dado uma aba solta na trilha
  Quando o usuário a arrasta para a zona de fixadas e solta
  Então ela é fixada na posição de queda
  E arrastá-la de volta para dentro do grupo "infra" a desafixa e a põe no grupo

Cenário: diretório gravado que sumiu
  Dado uma aba fixada em D:/Externo/app, num disco desconectado
  Quando o app é reaberto
  Então a fixada abre no diretório inicial, com a nota de diretório inexistente
  E continua na lista de fixadas

Cenário: fixada SSH
  Dado uma aba SSH fixada
  Quando o app é reaberto
  Então a fixada aparece não iniciada, com o ícone do app
  E conecta quando o usuário clica nela
```

## Fora de escopo

Cada item é decisão, não esquecimento.

- **Chave de config para ligar e desligar o recurso.** Quem não quer um app abrindo no arranque o desafixa. Sem fixadas, não há zona, nem leitura de ícone, nem nada.
- **Fixadas em todas as janelas.** Uma aba vive num workspace só ([ADR-0015](../adr/0015-multiplas-janelas.md)); mostrar a mesma fixada em várias janelas pediria aba compartilhada entre workspaces, e abri-la de novo em cada uma duplicaria o processo (RF-21.13).
- **Reabrir as fixadas numa janela nova** quando a janela delas fecha com outras ainda abertas. Elas voltam no próximo arranque (RF-21.25).
- **Rótulo na fixada**, e portanto renomear inline (RF-1.8). O título customizado que a aba tinha ao ser fixada é gravado e aparece no tooltip; para mudá-lo, desafixar, renomear e fixar de novo.
- **Atualizar o retrato sozinho** enquanto a fixada muda (RF-21.9). É a mesma regra das sessões nomeadas (RF-14.15): retrato, não vínculo.
- **Ícone que acompanha o `cd`.** O ícone é do diretório gravado (RF-21.17); seguir o `cwd` dependeria de OSC 7, que no Windows não existe sem integração de shell ([ADR-0038](../adr/0038-fallbacks-de-cwd.md)).
- **Vigiar o arquivo do ícone** e trocá-lo com o app aberto (RF-21.21).
- **Ícone por URL, SVG, ou glyph da face de ícones.** Arquivo de imagem local, nos formatos já decodificados.
- **Cor ou fundo próprio por fixada.** Os perfis já recusaram cor por perfil; a fixada não reabre a questão.
- **Fixada sem processo** — um atalho que só abre quando clicado. A fixada é um app aberto junto com o Porecatu; o caso "abrir quando eu pedir" é um perfil ([PRD-007](prd-007-perfis-de-aba.md)).
- **Conectar a fixada SSH sozinha no arranque** (RF-21.15).
- **Limite de quantidade.** A zona cresce (RF-21.2); dezenas de fixadas são um uso que a barra não precisa otimizar.

### O que este documento revisa

- **[PRD-001](prd-001-abas.md), "Fora de escopo".** "Fixar aba" sai da lista, com emenda datada no próprio PRD-001. O RF-1.3 (aba fecha quando o shell sai) ganha a exceção do RF-21.24, e o RF-1.4 a do RF-21.25.
- **[PRD-012](prd-012-comando-de-projeto-por-diretorio.md).** Três exceções, todas só para abas fixadas: o gatilho deixa de ser só "aba restaurada de sessão" (RF-12.1, RF-12.12) e passa a incluir "fixada aberta no arranque"; o `trusted_paths` não é exigido (RF-12.5); e `lazy_restore` não adia o script (RF-12.11), porque a fixada não é preguiçosa. O formato ganha o nome reservado `[icon]` (RF-12.2, RF-12.3). A revisão correspondente no [ADR-0051](../adr/0051-arquivo-de-projeto-porecatu.md) entra como blockquote, pelo [ADR-0086](../adr/0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md) e pelo [ADR-0088](../adr/0088-icone-da-aba-fixada.md).
- **[PRD-003](prd-003-persistencia-de-sessao.md).** A sessão deixa de gravar uma parte das abas da janela — as fixadas (RF-21.27). O RF-3.8 ganha a exceção do RF-21.15.
- **[acoes.md](../reference/acoes.md).** `tab.pin` sai de "ações que não existem" e `tab.toggle_pin` entra no catálogo.

### O que este documento **não** contradiz

A proibição do [PRD-003](prd-003-persistencia-de-sessao.md) de reexecutar processos continua inteira. O que roda numa fixada é o `.porecatu`, escrito pelo usuário ou pelo projeto, entregue ao shell como se digitado — a mesma distinção que a emenda do PRD-003 registrou para o PRD-012. O que muda é **quem autoriza**: para a fixada, o gesto de fixar no lugar da linha em `trusted_paths`.

O custo dessa troca, registrado aqui e detalhado no ADR-0086: a autorização vale para o **diretório**, não para o **conteúdo**. Um `.porecatu` que muda depois de fixado — um `git pull` que traz uma versão nova — roda no próximo arranque sem que o usuário o tenha visto. É o mesmo custo que o `trusted_paths` já tem, aplicado a um diretório por vez em vez de a uma árvore.

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Gestos para ter um app aberto em todo arranque | **um** (fixar), e zero a cada arranque seguinte |
| Fixadas abertas em dobro numa mesma execução | **zero** |
| Fixadas perdidas por `exit`, fechar janela ou sessão desligada | **zero** — só fechar a aba ou desafixar as tira da lista |
| Pixels novos na barra para quem não fixa nada | **zero** |
| Frames bloqueados pela decodificação de ícone | **zero** |
| Leituras de ícone fora do arranque e do gesto de fixar | **zero** |
| Valores visuais inventados | **zero** — anatomia sai dos tokens e do botão do app |
| Dependências novas no workspace | **zero** |

A terceira é a que mais importa: a lista de fixadas é o que o usuário montou de propósito, e ela só pode mudar por um gesto que diz, com todas as letras, que muda.
