# PRD-023 — Restaurar sessão nomeada na janela atual

**Status:** Aprovado
**Data:** 2026-11-07
**Requisito de origem:** pedido direto do dono do produto, sobre as sessões nomeadas já em uso ([PRD-014](prd-014-sessoes-nomeadas.md)). É a revisão de um item que o PRD-014 deixou em "Fora de escopo": *"Restaurar dentro da janela atual, substituindo ou acrescentando abas"*.
**Relacionados:** [ADR-0092](../adr/0092-restauracao-nomeada-na-janela-atual.md), [ADR-0030](../adr/0030-escopo-do-hot-reload.md), [ADR-0037](../adr/0037-aba-nao-iniciada.md), [ADR-0051](../adr/0051-arquivo-de-projeto-porecatu.md), [ADR-0053](../adr/0053-paineis-divididos.md), [ADR-0054](../adr/0054-sessoes-nomeadas.md), [ADR-0055](../adr/0055-botao-e-popover-de-sessoes.md), [ADR-0060](../adr/0060-anatomia-da-tela-de-configuracoes.md), [ADR-0086](../adr/0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md), [ADR-0090](../adr/0090-transferencia-de-abas-entre-janelas.md), [PRD-003](prd-003-persistencia-de-sessao.md), [PRD-014](prd-014-sessoes-nomeadas.md), [PRD-016](prd-016-tela-de-configuracoes.md), [PRD-020](prd-020-conexoes-ssh.md), [PRD-021](prd-021-abas-fixadas.md), [PRD-022](prd-022-abas-e-grupos-entre-janelas.md)

> Aprovado em 2026-11-07, por decisão do dono do produto, **fora da ordem de fases**, como os outros recursos pedidos depois da F6. O dono decidiu três coisas antes da escrita: o comportamento é uma **opção** nas configurações, um toggle `true`/`false`; o padrão é **desligado**, e com ele nada muda em relação ao PRD-014; ligada, a sessão é restaurada **acrescentando** os grupos e as abas dela à janela atual, **sem substituir** nada do que já está lá. O como está no [ADR-0092](../adr/0092-restauracao-nomeada-na-janela-atual.md).

## Problema

O PRD-014 decidiu que restaurar uma sessão nomeada **abre uma janela nova** (RF-14.10). A razão era boa para quem usa uma janela por projeto: o projeto X numa janela, o projeto Y noutra, e restaurar um nunca mexe no outro.

Nem todo mundo trabalha assim. Há quem mantenha **uma janela só**, maximizada ou em tela cheia, e separe os projetos por **grupo** dentro dela. É o uso que os grupos nomeados existem para servir ([PRD-002](prd-002-grupos-de-abas.md)). Para esse usuário, cada restauração cria uma janela que ele não queria, e o gesto passa a ser de quatro passos: restaurar, arrastar os grupos para a janela de sempre ([PRD-022](prd-022-abas-e-grupos-entre-janelas.md)), fechar a janela nova, voltar à de sempre. O recurso que existia para poupar trabalho de montagem passa a criar trabalho de arrumação.

O PRD-014 recusou restaurar na janela atual por duas razões: *"substituir descartaria processos vivos; acrescentar misturaria dois contextos numa barra"*. A primeira continua valendo, e este documento não substitui nada. A segunda é verdadeira para quem usa uma janela por projeto. Para quem usa um grupo por projeto, misturar contextos numa barra é justamente o jeito de trabalhar. Por isso o comportamento é uma **opção**, e o padrão continua sendo o do PRD-014.

## O que já existe e não muda

- **Salvar, listar e excluir** sessões nomeadas (RF-14.1 a RF-14.9, RF-14.16 a RF-14.18) não mudam. A opção só governa **para onde vai** o que se restaura.
- **O que uma aba restaurada faz** (RF-14.11) não muda: `cwd` com fallback, painéis com proporções, `lazy_restore`, `.porecatu` só em diretório autorizado. Continua sendo o caminho de código do arranque.
- **Tema e zoom** gravados continuam não aplicados (RF-14.13), e as **abas fixadas** continuam fora da restauração nomeada (RF-21.13).
- **Retrato, não vínculo** (RF-14.15) continua valendo.
- A **sessão automática** (`[session] enabled`, RF-3.6) não é afetada, como as nomeadas em geral não são (RF-14.14).

## Usuário-alvo

O do [PRD-014](prd-014-sessoes-nomeadas.md), no caso de quem trabalha com **uma janela e um grupo por projeto**, e não com uma janela por projeto. Quem não ligar a opção não vê diferença nenhuma.

## Em uma tela

Na tela de configurações, grupo **Sessão**, uma alternância nova:

```
  Sessão
  ─────────────────────────────────────────────────────────────
  Gravar e restaurar a sessão                              [●  ]
  Iniciar abas só no primeiro foco                         [  ○]
  Restaurar tamanho e posição das janelas                  [●  ]
  Restaurar sessões salvas na janela atual                 [  ○]   ← nova, desligada
  Oferecer integração de shell                             [●  ]
```

Desligada, clicar em "api + front" no popover de sessões abre uma janela nova, como hoje. Ligada, a mesma janela onde o popover estava ganha os grupos e as abas da sessão no fim da trilha:

```
antes    [ infra · 2 ] [ shell ]
depois   [ infra · 2 ] [ shell ] [ api · 3 ] [ front · 2 ]
                                    └─ grupos de "api + front", acrescentados
```

## Requisitos funcionais

### A opção

**RF-23.1**: Uma chave booleana `[session] restore_named_in_current_window`, com default **`false`**, decide o destino da restauração de uma sessão nomeada. O nome da chave e a linha do arquivo de exemplo estão no [ADR-0092](../adr/0092-restauracao-nomeada-na-janela-atual.md) §1.

**RF-23.2**: A opção aparece na tela de configurações ([PRD-016](prd-016-tela-de-configuracoes.md)), no grupo **Sessão**, como **alternância**, com rótulo e descrição no catálogo de textos de todos os idiomas ([ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md)). Gravar pela tela escreve a chave no `porecatu.toml` pelo caminho de sempre ([ADR-0058](../adr/0058-escrita-do-arquivo-de-configuracao.md)).

**RF-23.3**: A opção **vale na hora**. Mudá-la pela tela de configurações ou editando o arquivo afeta a **próxima** restauração, sem reiniciar o app e sem o aviso de "reinicie" que as outras chaves de `[session]` mostram. A tela de configurações não a marca como "exige reinício".

**RF-23.4**: Com a opção **desligada**, tudo continua como no PRD-014: restaurar abre uma janela nova (RF-14.10), com o tamanho salvo e em cascata a partir da janela de origem (RF-14.12).

### Restaurar na janela atual

**RF-23.5**: Com a opção **ligada**, todo gesto que restaura uma sessão no popover de sessões, seja o clique na linha ou o `Enter` na linha realçada, **acrescenta** a disposição da sessão à **janela onde o popover estava aberto**. Nenhuma janela nova é aberta.

**RF-23.6**: O que se acrescenta entra **no fim da trilha**, depois de tudo o que a janela já tinha, na ordem em que a sessão foi salva:
- cada **grupo** da sessão entra como um grupo novo, com nome, cor e estado de colapso salvos;
- cada **aba solta** da sessão (fora de grupo) entra como aba solta, na mesma posição relativa.

**RF-23.7**: Nada do que já estava na janela muda: nenhuma aba é fechada, movida, renomeada ou reiniciada, nenhum grupo muda de nome, cor ou colapso, e nenhum processo é encerrado. Por isso não há diálogo de confirmação.

**RF-23.8**: Um grupo da sessão com o **mesmo nome** de um grupo que já está na janela **não se funde** com ele. Os dois coexistem, como coexistem quando um grupo chega de outra janela (RF-22.5). Juntar dois grupos é gesto do usuário, não efeito colateral de restaurar.

**RF-23.9**: Depois de acrescentar, a **aba que era ativa na sessão** passa a ser a aba ativa da janela, e a trilha rola até ela, como quando o usuário clica numa aba. O popover fecha. A janela continua com o foco do sistema.

**RF-23.10**: Cada aba acrescentada se comporta **exatamente como uma aba restaurada** (RF-14.11), pelo mesmo caminho de código:
- cada painel sobe no `cwd` salvo, e um `cwd` que não existe mais cai no diretório inicial com a nota do RF-3.10;
- a árvore de painéis volta com as proporções salvas (RF-6.22, RF-6.23);
- com `[session] lazy_restore = true`, só a aba que fica ativa (RF-23.9) sobe agora, e as outras sobem no primeiro foco, com o rótulo esmaecido do RF-3.9;
- o `.porecatu` roda como em qualquer aba restaurada: só em diretório de `[project_file] trusted_paths`, uma vez por aba, no painel focado, e com a nota de "não autorizado" fora da lista;
- todo painel SSH volta não iniciado e conecta no primeiro gesto (RF-20.33).

**RF-23.11**: A janela **não muda** de tamanho, posição, maximização nem tela cheia. O tamanho gravado na sessão é ignorado: quem restaura na janela atual escolheu a janela que já tem.

**RF-23.12**: As **abas fixadas** da janela não mudam, e a restauração não acrescenta nenhuma (RF-21.13). Tema e zoom gravados não são aplicados (RF-14.13).

**RF-23.13**: Sessão que falha ao ler (arquivo ilegível, corrompido, de schema mais novo) se comporta como hoje: aviso na janela de origem e nada acrescentado (RF-14.17). Sessão lida sem nenhuma aba a restaurar não acrescenta nada e fecha o popover. A janela já tem abas, então a garantia de "terminal mínimo" da restauração numa janela nova não se aplica.

### Depois

**RF-23.14**: As abas acrescentadas passam a ser **abas da janela como quaisquer outras**. Com `[session] enabled = true`, entram no `session.json` na próxima gravação e voltam no próximo arranque pela sessão automática. Nenhum vínculo com o arquivo nomeado sobrevive (RF-14.15): mudar, fechar ou mover essas abas não altera a sessão salva.

**RF-23.15**: Restaurar a **mesma** sessão duas vezes na mesma janela acrescenta a disposição **duas vezes**. A restauração é um gesto, não uma sincronização; o app não procura abas "iguais" já abertas para pular.

## Cenários

```gherkin
Cenário: com a opção desligada, nada muda
  Dado [session] restore_named_in_current_window = false
  E a sessão "api + front" salva
  Quando o usuário clica nela no popover de sessões
  Então uma janela nova abre com a disposição salva
  E a janela de onde ele clicou continua exatamente como estava

Cenário: com a opção ligada, a sessão entra na janela atual
  Dado [session] restore_named_in_current_window = true
  E uma janela com o grupo "infra" e uma aba solta
  E a sessão "api + front" com os grupos "api" e "front"
  Quando o usuário clica em "api + front" no popover
  Então nenhuma janela nova abre
  E a trilha fica "infra", a aba solta, "api", "front", nessa ordem
  E a aba que era ativa em "api + front" é a aba ativa da janela
  E as abas de "infra" e a aba solta continuam com os mesmos processos

Cenário: grupo com nome repetido não se funde
  Dado a opção ligada
  E uma janela com o grupo "api"
  Quando o usuário restaura uma sessão que também tem um grupo "api"
  Então a trilha tem dois grupos "api", o original e o acrescentado

Cenário: lazy_restore vale para o que é acrescentado
  Dado a opção ligada e [session] lazy_restore = true
  Quando o usuário restaura uma sessão de quatro abas na janela atual
  Então só a aba que fica ativa inicia o shell
  E as outras três aparecem com o rótulo esmaecido até o primeiro foco

Cenário: o .porecatu roda como na restauração do arranque
  Dado a opção ligada
  E a sessão com uma aba em ~/Projetos/api, que tem um .porecatu e está em trusted_paths
  Quando o usuário restaura a sessão na janela atual
  Então o comando do .porecatu é escrito no prompt daquela aba, uma vez

Cenário: diretório não autorizado continua não autorizado
  Dado a opção ligada
  E a sessão com uma aba num diretório com .porecatu fora de trusted_paths
  Quando o usuário restaura a sessão na janela atual
  Então nada é executado
  E a aba mostra a nota de arquivo de projeto não autorizado

Cenário: a janela não muda de tamanho
  Dado a opção ligada e uma janela maximizada
  E a sessão salva numa janela de 900×600
  Quando o usuário restaura a sessão
  Então a janela continua maximizada

Cenário: fixadas ficam como estão
  Dado a opção ligada e uma janela com duas abas fixadas
  Quando o usuário restaura uma sessão
  Então as duas fixadas continuam na zona de fixadas, na mesma ordem
  E nenhuma fixada nova aparece

Cenário: a opção vale sem reiniciar
  Dado a opção desligada
  Quando o usuário a liga na tela de configurações e salva
  E restaura uma sessão logo em seguida
  Então a sessão é acrescentada à janela atual
  E nenhum aviso de "reinicie o app" aparece

Cenário: arquivo ilegível
  Dado a opção ligada e uma sessão cujo arquivo está corrompido
  Quando o usuário tenta restaurá-la
  Então a janela mostra o aviso de sessão ilegível
  E nenhuma aba é acrescentada

Cenário: restaurar duas vezes acrescenta duas vezes
  Dado a opção ligada
  Quando o usuário restaura "infra" duas vezes seguidas na mesma janela
  Então a trilha ganha os grupos de "infra" duas vezes
```

## Fora de escopo

Cada item é decisão, não esquecimento.

- **Substituir as abas da janela pelas da sessão.** O dono do produto escolheu acrescentar. Substituir encerraria processos vivos por um gesto que, na lista, parece de abrir, e pediria diálogo de confirmação e regra para as fixadas. A razão que o PRD-014 deu para recusar continua inteira para este caso.
- **Inverter a opção por gesto**, por exemplo com `Shift+clique` restaurando no destino contrário ao configurado. É ideia plausível e fica registrada. Este documento entrega a opção que foi pedida, e um modificador é superfície nova de mouse que pede requisito próprio.
- **Perguntar a cada restauração** ("nova janela ou esta?"). Seria um diálogo a mais em todo gesto, e o pedido foi uma opção fixa.
- **Escolher onde a sessão entra** (dentro de um grupo existente, depois da aba ativa). O fim da trilha é previsível e não mistura nada com os grupos que já existem. Depois, o usuário move o que quiser com os gestos da barra.
- **Pular abas que já estão abertas** no mesmo `cwd`. Abas iguais não são a mesma aba: duas abas no mesmo diretório são um uso comum, e o app não tem como saber qual o usuário quer de novo (RF-23.15).
- **Ação de catálogo para alternar a opção.** Configuração de comportamento não é ação ([docs/reference/acoes.md](../reference/acoes.md)), e o toggle já está a dois cliques, na tela de configurações.
- **Aplicar o tamanho salvo à janela atual.** RF-23.11.

### O que este documento **não** contradiz

O PRD-014 listou em "Fora de escopo" restaurar dentro da janela atual, *substituindo ou acrescentando abas*. Este documento revê **só a metade "acrescentando"**, e só com a opção ligada. Substituir continua fora, e o padrão continua sendo a janela nova. A emenda está registrada no próprio PRD-014.

A métrica do PRD-014 *"diferenças de comportamento entre aba restaurada no arranque e aba restaurada por nome: zero"* continua valendo e se estende ao destino novo. Uma aba acrescentada à janela atual sobe pelo mesmo caminho de código de uma aba restaurada numa janela nova ([ADR-0092](../adr/0092-restauracao-nomeada-na-janela-atual.md) §4).

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Gestos para trazer a disposição de um projeto para a janela de sempre, com a opção ligada | **dois** (abrir o popover, clicar no nome), contra quatro com a opção desligada |
| Abas, grupos e processos já abertos alterados por restaurar na janela atual | **zero** |
| Diferenças de comportamento entre aba acrescentada à janela atual e aba restaurada numa janela nova | **zero**, com `cwd`, painéis, `lazy_restore`, `.porecatu` e SSH pelo mesmo caminho de código |
| Mudança de comportamento para quem não liga a opção | **zero** |
| Reinícios necessários para a opção valer | **zero** |
| Dependências novas no workspace | **zero** |
