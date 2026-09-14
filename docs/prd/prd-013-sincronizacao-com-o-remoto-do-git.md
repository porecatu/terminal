# PRD-013 — Sincronização com o remoto do Git

**Status:** Aprovado
**Data:** 2026-09-14
**Requisito de origem:** pedido direto do dono do produto, sobre a barra de status já em uso — o mesmo caminho do [ADR-0049](../adr/0049-branch-git-na-barra-de-status.md), que entregou a branch e parou **exatamente** aqui: *"ahead/behind... entrar exigiria thread e um ADR que enfrente o RF-9.8 de verdade"* (§7)
**Relacionados:** [ADR-0052](../adr/0052-sincronizacao-com-o-remoto-do-git.md), [ADR-0007](../adr/0007-modelo-de-threading.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md), [ADR-0048](../adr/0048-barra-de-status.md), [ADR-0049](../adr/0049-branch-git-na-barra-de-status.md), [PRD-009](prd-009-barra-de-status.md)

> Aprovado em 2026-09-14, por decisão do dono do produto, **fora da ordem de fases** — como a barra de status ([PRD-009](prd-009-barra-de-status.md)) e o arquivo de projeto ([PRD-012](prd-012-comando-de-projeto-por-diretorio.md)). Não é rascunho promovido: é requisito novo, escrito depois de o v1 estar em uso. As decisões que ele deixa em aberto — como consultar o remoto, onde o estado vive, como a thread convive com um app damage-driven, e como um alvo clicável cabe numa faixa que cedeu a borda ao resize — estão no [ADR-0052](../adr/0052-sincronizacao-com-o-remoto-do-git.md).

## Problema

A barra de status responde "em que branch eu estou?". Não responde a pergunta seguinte, que é a que custa tempo: **"o que mudou lá enquanto eu trabalhava aqui?"**

Hoje o usuário descobre isso de três maneiras, e as três são ruins:

- **Rodando `git fetch` à mão**, de tempos em tempos, em cada repositório aberto. É digitar um comando para *não* aprender nada na maioria das vezes — o resultado esperado é "nada mudou".
- **Não descobrindo.** Trabalha uma hora sobre uma base desatualizada e leva o conflito na hora do merge, que é o pior momento possível para saber.
- **Por um prompt de shell que faz isso sozinho.** Funciona, e é justamente o que o [ADR-0049](../adr/0049-branch-git-na-barra-de-status.md) existe para evitar: um `git status` dentro do prompt custa centenas de milissegundos em árvore grande, e o custo é pago **a cada Enter**, inclusive nas milhares de vezes em que nada mudou.

O terceiro é o mais instrutivo. A informação é barata de descobrir e cara de perguntar **no lugar errado**. Perguntar a cada prompt é errado; perguntar a cada frame é pior. Perguntar a cada poucos minutos, fora do caminho de desenho, é a forma certa da mesma pergunta.

E há o outro lado, que é o que transforma isto de indicador em recurso: **saber que há commits novos e ter de trocar de contexto para buscá-los já é metade do atrito.** Quem vê "3 commits atrás" vai digitar `git pull` — no terminal, parando o que estava fazendo. O clique existe para fechar esse laço no mesmo lugar em que a informação apareceu.

## Usuário-alvo

O mesmo do [PRD-000](prd-000-visao-de-produto.md): quem trabalha em mais de um repositório ao mesmo tempo. O valor cresce com **quantas pessoas empurram para os repositórios abertos**, não com o número de abas — um projeto solo raramente tem commits novos; um repositório de time tem vários por dia.

**Não é para** quem trabalha offline, em repositório sem remoto, ou em máquina onde falar com a rede sozinho é indesejado. Para esses, `remote_poll_interval_secs = 0` desliga tudo, e desligado o recurso não custa nada — nem uma consulta, nem um prazo agendado, nem um pixel.

## Em uma tela

Hoje, a zona esquerda da barra:

```
  pwsh   ~/Projetos/api   ⑂ main   API
```

Com o recurso, quando o remoto tem commits que o local não tem:

```
  pwsh   ~/Projetos/api   ⑂ main   ↓ 3 commits atrás   API
                                   └── clicável: git pull --ff-only
```

Quando a branch também tem commits locais ainda não enviados, o `pull --ff-only` não
teria como funcionar, e o indicador diz isso em vez de oferecer um botão quebrado:

```
  pwsh   ~/Projetos/api   ⑂ main   ↓ 2 atrás, 1 à frente
                                   └── informação; não é clicável
```

Sem commits novos, o segmento **não existe** — nem apagado, nem com zero. Como o ícone
de repositório do [ADR-0049](../adr/0049-branch-git-na-barra-de-status.md) §5, a
ausência já é a resposta.

E, uma vez, na config do usuário:

```toml
[git]
remote_poll_interval_secs = 300
```

## Requisitos funcionais

### A consulta

**RF-13.1** — Quando o diretório da aba ativa pertence a um repositório Git, o app consulta periodicamente o remoto daquela branch para saber quantos commits ela está atrás. O intervalo é configurável **em segundos**, em `[git] remote_poll_interval_secs`.

**RF-13.2** — **`0` desliga o recurso inteiro**: nenhuma consulta, nenhum indicador, nenhum processo lançado e **nenhum prazo agendado** no laço de eventos. Com `0`, o app se comporta exatamente como antes deste documento existir. O default é `300` — ver as métricas e a primeira consequência negativa do [ADR-0052](../adr/0052-sincronizacao-com-o-remoto-do-git.md).

**RF-13.3** — Valor maior que zero e menor que **30** é elevado a 30, e o app **informa uma vez** que o elevou. Saturar em silêncio esconderia do usuário que o número que ele escreveu não é o que vale; recusar a config inteira por causa disso derrubaria o app por um exagero (o [ADR-0003](../adr/0003-formato-de-configuracao.md) não permite que config ruim derrube nada).

**RF-13.4** — A consulta **não altera a árvore de trabalho, o índice, nem o `HEAD` local**. Ela busca o que o remoto tem; integrar é o RF-13.12, e só acontece por clique.

**RF-13.5** — A primeira consulta acontece **um intervalo depois** do arranque, nunca durante. O tempo até o primeiro prompt utilizável é métrica do [PRD-000](prd-000-visao-de-produto.md), e nada deste documento pode disputá-la.

**RF-13.6** — O que é vigiado é o repositório da aba **ativa de cada janela**, deduplicado por repositório: duas abas — ou duas janelas — no mesmo projeto produzem **uma** consulta, não duas. Trocar de aba não dispara consulta nova; mostra o que já se sabe daquele repositório, ou nada.

### O indicador

**RF-13.7** — Havendo commits no remoto que o local não tem, a barra exibe, ao lado do segmento de branch, um indicador clicável dizendo **quantos**, por extenso, com singular e plural corretos (`1 commit atrás`, `3 commits atrás`). A cor dele é configurável como a dos outros segmentos da barra ([PRD-004](prd-004-aparencia-do-chrome.md): nenhuma cor vive no código).

**RF-13.8** — Sem commits novos, o indicador **não aparece** — nem apagado, nem zerado. Mesma regra do ícone de repositório ([ADR-0049](../adr/0049-branch-git-na-barra-de-status.md) §5): a ausência é a resposta, e uma barra que mostra "0 atrás" o tempo todo é ruído com aparência de informação.

**RF-13.9** — Quando a branch está **ao mesmo tempo** atrás do remoto e à frente dele, o indicador mostra os dois números e **deixa de ser clicável**. O `pull --ff-only` do RF-13.12 falharia por definição nesse estado, e o app não oferece um botão que ele já sabe que não funciona.

**RF-13.10** — A contagem pertence a uma branch. Trocar de branch a invalida: o indicador some até haver consulta nova para a branch nova. Número certo da branch errada é pior que número nenhum.

**RF-13.11** — Enquanto uma consulta ou um `pull` está em andamento para aquele repositório, o indicador não aceita um segundo clique. Nada pisca e nada se move: o app é damage-driven, e um indicador animado contrariaria o RF-9.8 por um motivo muito pior que o deste documento.

### A integração

**RF-13.12** — Clicar no indicador integra os commits do remoto **por fast-forward apenas**, em segundo plano. É a única forma de o app tocar a árvore de trabalho, e ela nunca acontece sozinha.

**RF-13.13** — A interface não trava enquanto isso acontece. Nem a consulta do RF-13.1 nem a integração do RF-13.12 podem bloquear o laço de eventos, desenhar quadro nenhum ou atrasar uma tecla.

**RF-13.14** — O resultado é **sempre informado** — sucesso e falha, com a mensagem do `git` quando ela existir. Árvore suja, credencial recusada, sem rede, branch divergida: todos terminam com o usuário sabendo o que aconteceu. Clicar e não acontecer nada é o pior resultado possível, e é o resultado natural de um recurso que roda em segundo plano.

**RF-13.15** — Nada disto é escrito no terminal da aba. O canal é o aviso do app — canal 1 do [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md) —, porque o fato é sobre um repositório do sistema e não sobre o histórico daquele terminal, e porque escrever no grid mexeria na tela do programa que estiver rodando ali.

### Fronteiras

**RF-13.16** — **Estado da árvore continua fora**, e a fronteira é a mesma do [ADR-0049](../adr/0049-branch-git-na-barra-de-status.md) §7, com a razão reescrita em base nova: sujo/limpo, arquivos em stage e `git status` ficam de fora porque **percorrem a árvore de trabalho**, não porque exigiriam uma thread. A thread agora existe; a razão não dependia dela.

**RF-13.17** — O app **nunca** empurra commits, nunca cria merge, nunca faz rebase e nunca resolve conflito. Tudo isso é decisão do usuário, tomada no terminal, onde ele vê o que está fazendo.

**RF-13.18** — O app **não escolhe remoto**. Ele consulta o remoto que a própria branch já declara seguir; uma branch que segue um `fork` não faz o app buscar o `origin`. Repositório cuja branch não segue remoto nenhum não é consultado e não mostra indicador.

**RF-13.19** — Sem o `git` disponível no sistema, o recurso informa **uma vez por execução** e não tenta de novo. O aviso só acontece para quem tem o recurso ligado — quem o desligou nunca é incomodado por causa dele.

## Critérios de aceite

```gherkin
Cenário: o caso que motiva o recurso
  Dado um repositório aberto numa aba, com remote_poll_interval_secs = 300
  E uma colega empurrou três commits para a branch que essa aba segue
  Quando o intervalo de consulta se cumpre
  Então a barra de status passa a mostrar "3 commits atrás" ao lado da branch
  E nada foi alterado na árvore de trabalho do usuário

Cenário: o clique integra
  Dado o indicador mostrando "3 commits atrás"
  Quando o usuário clica nele
  Então os commits são integrados por fast-forward, em segundo plano
  E a interface continua respondendo enquanto isso acontece
  E o indicador desaparece quando a branch fica em dia

Cenário: desligado é desligado
  Dado remote_poll_interval_secs = 0
  Quando o app roda por uma hora com um repositório aberto
  Então nenhum processo git foi lançado
  E nenhum prazo foi agendado por causa deste recurso
  E o indicador nunca apareceu

Cenário: nada muda sozinho
  Dado o indicador mostrando "3 commits atrás"
  Quando o usuário não clica nele
  Então a árvore de trabalho continua exatamente como estava
  E nenhum merge, rebase ou push aconteceu

Cenário: branch divergida não oferece o que não funciona
  Dado uma branch dois commits atrás do remoto e um commit à frente
  Quando a consulta termina
  Então o indicador mostra os dois números
  E clicar nele não faz nada

Cenário: o clique que falha não falha em silêncio
  Dado o indicador clicável e alterações não commitadas na árvore
  Quando o usuário clica
  Então a integração é recusada sem alterar nada
  E o app informa o motivo, com a mensagem do git

Cenário: trocar de branch invalida a contagem
  Dado o indicador mostrando "3 commits atrás" na branch main
  Quando o usuário faz checkout de outra branch no terminal
  Então o indicador desaparece
  E só volta quando houver consulta para a branch nova

Cenário: branch sem remoto
  Dado uma branch local que nunca foi empurrada
  Quando o intervalo se cumpre
  Então nenhuma consulta de rede acontece para ela
  E nenhum indicador aparece

Cenário: dois lugares, um repositório
  Dado duas janelas com a aba ativa no mesmo repositório
  Quando o intervalo se cumpre
  Então uma única consulta acontece
  E as duas barras mostram o mesmo resultado

Cenário: o valor pequeno demais é corrigido, não obedecido nem recusado
  Dado remote_poll_interval_secs = 1
  Quando o app carrega a config
  Então o intervalo usado é 30 segundos
  E o app informa uma vez que elevou o valor
  E o resto da configuração continua valendo

Cenário: sem git instalado
  Dado um sistema sem git disponível e o recurso ligado
  Quando a primeira consulta seria feita
  Então o app informa uma vez que não encontrou o git
  E não tenta de novo nesta execução

Cenário: o arranque não compete com o primeiro prompt
  Dado o recurso ligado com intervalo de 300 segundos
  Quando o usuário abre o app
  Então nenhuma consulta acontece durante o arranque
  E a primeira acontece um intervalo depois

Cenário: em dia não mostra nada
  Dado uma branch exatamente igual ao remoto
  Quando a consulta termina
  Então nenhum indicador aparece
  E não aparece "0 atrás" nem versão apagada dele

Cenário: o resultado não suja o terminal
  Dado um programa de tela cheia rodando na aba ativa
  Quando o usuário clica no indicador e a integração termina
  Então nada é escrito no grid daquela aba
  E o resultado aparece no aviso do app

Cenário: a borda continua sendo a borda
  Dado o indicador visível na barra de status
  Quando o usuário arrasta a borda inferior da janela fora do indicador
  Então a janela é redimensionada, como antes
```

## Fora de escopo

Cada item é decisão, não esquecimento. Os motivos completos estão no [ADR-0052](../adr/0052-sincronizacao-com-o-remoto-do-git.md) §2 e nas alternativas.

- **Estado da árvore de trabalho** — sujo/limpo, contagem de arquivos modificados, o que está em stage. Percorrem a árvore, e é isso que os mantém fora (RF-13.16). O que mudou desde o [ADR-0049](../adr/0049-branch-git-na-barra-de-status.md) foi a existência de uma thread, e a thread nunca foi a razão.
- **Empurrar commits, merge, rebase, resolver conflito** (RF-13.17). O recurso é "trazer o que já está pronto lá"; tudo o mais é decisão que se toma vendo o que se faz.
- **Escolher remoto, ou consultar todos** (RF-13.18). A branch já declara o que segue.
- **Um indicador por aba, ou por repositório aberto.** A barra descreve a **aba ativa** — decisão do [PRD-009](prd-009-barra-de-status.md), e não se reabre aqui.
- **Ação no catálogo para integrar por teclado.** O catálogo é fechado ([docs/reference/acoes.md](../reference/acoes.md)) e a razão de não entrar não é burocrática: um atalho que roda `git pull` no repositório que a aba ativa por acaso tem é a mesma classe de decisão que o [ADR-0051](../adr/0051-arquivo-de-projeto-porecatu.md) §4 registrou para `trusted_paths` — tomada uma vez, esquecida depois. O clique é deliberado e mira o que descreve.
- **Notificação de desktop quando aparecem commits novos.** Já está registrada como fora do v1 no [roadmap](../roadmap.md) para a campainha, pelo mesmo motivo.
- **Lista dos commits novos, com autor e mensagem.** É um painel, não um segmento de 26px de altura.
- **Autorizar o recurso por diretório**, no modelo do `.porecatu`. Ver o §4 do [ADR-0052](../adr/0052-sincronizacao-com-o-remoto-do-git.md): a consulta não executa código declarado por terceiros, que é o que aquela allowlist existe para conter. O que ela faz é falar com a rede, e para isso a chave global é o controle proporcional.

### O que este documento **não** contradiz

O [PRD-009](prd-009-barra-de-status.md) diz, no RF-9.8, que a barra *"não introduz timer nem redraw periódico"*, e o [ADR-0048](../adr/0048-barra-de-status.md) §8 lista o que isso exclui. **Este documento emenda o RF-9.8 e mantém o resto da lista inteira** — relógio, uso de CPU e memória e contagem de processos da aba continuam fora, pelas razões originais.

A emenda não é um relaxamento geral. O critério que substitui "nada muda sozinho" está no [ADR-0052](../adr/0052-sincronizacao-com-o-remoto-do-git.md) §1 e é mais estreito do que parece: **o que muda sozinho precisa ser desligável numa linha de config, custar exatamente zero quando desligado, e não percorrer a árvore de trabalho.** Um relógio continua reprovando nos três; `git status`, no terceiro.

O princípio 4 do [PRD-000](prd-000-visao-de-produto.md) — *"terminal ocioso custa zero. Nenhum frame renderizado sem mudança"* — continua literalmente verdadeiro: a consulta só suja a barra quando a contagem **muda**, e consulta que confirma "nada novo" não desenha quadro nenhum. O que deixa de ser literal é a frase das consequências do [ADR-0007](../adr/0007-modelo-de-threading.md), *"terminal ocioso custa zero CPU"*, e o ADR-0052 registra isso em vez de contorná-lo.

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Ações do usuário para saber que há commits novos no repositório aberto | **zero** |
| Ações para integrá-los, quando é fast-forward | **uma** (o clique) |
| Alterações na árvore de trabalho sem clique do usuário | **zero** |
| Cliques que falham sem o usuário saber por quê | **zero** |
| Custo do recurso com `remote_poll_interval_secs = 0` | **zero** — nenhum processo, nenhum prazo, nenhum pixel |
| Dependências novas no workspace | **zero** |

A primeira é a razão de este documento existir. A **quinta** é a que o mantém honesto: é ela que transforma a emenda ao RF-9.8 em algo verificável por teste, e não numa promessa de prosa.
