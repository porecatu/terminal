# PRD-009 — Barra de status

**Status:** Aprovado
**Data:** 2026-08-26 (rascunho) · 2026-09-09 (aprovado)
**Relacionados:** [ADR-0005](../adr/0005-persistencia-de-sessao.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md), [ADR-0039](../adr/0039-convite-a-integracao-de-shell.md), [ADR-0048](../adr/0048-barra-de-status.md), [ADR-0049](../adr/0049-branch-git-na-barra-de-status.md), [PRD-004](prd-004-aparencia-do-chrome.md)

> Aprovado em 2026-09-09, por decisão do dono do produto, **fora da ordem de fases**: era o único elemento `[v2]` cujo valor não dependia de nenhum recurso de v2 para existir. A anatomia e as cinco decisões que o rascunho deixava em aberto estão no [ADR-0048](../adr/0048-barra-de-status.md).

## Problema

Informação que o app **já tem** e não mostra em lugar nenhum: qual shell está rodando na aba ativa, em que diretório ela está, a que grupo pertence, qual a codificação.

O diretório é o caso mais forte. O app precisa dele para a restauração de sessão ([ADR-0005](../adr/0005-persistencia-de-sessao.md)) e o obtém via OSC 7. Hoje esse dado fica invisível. Exibi-lo tem um efeito colateral valioso: **o usuário passa a ver se o OSC 7 está funcionando**. Sem barra de status, a ausência de integração de shell só se manifesta muito depois, quando a sessão restaura no diretório errado — o pior momento possível para descobrir.

Uma barra de status transforma uma limitação silenciosa em uma limitação visível.

O [ADR-0039](../adr/0039-convite-a-integracao-de-shell.md) ataca o mesmo problema pelo outro lado: um convite no grid, uma vez, com o snippet a copiar. Os dois se completam — o convite avisa quando o app **detecta** a ausência, a barra mostra o estado **o tempo todo**, inclusive depois de o convite ter sido dispensado.

## O que o design mostra

Ver [especificação visual](../design/especificacao-visual.md), seção 2.8.

Altura 26, mono 10.5px, `gap: 16`. **O que o binário desenha divergiu do desenho em quatro pontos**, todos por decisão do dono do produto depois de ver a barra em tela e todos registrados na §2.8 e na §4.4 da especificação: sem fundo próprio, sem a borda superior, sem a versão do app, e o texto em `#a8b0bb` em vez do `#6b737e` do desenho, que reprovava WCAG AA a 10.5px.

| Zona | Conteúdo |
|---|---|
| Esquerda | nome do shell em `#5ed3bc`, diretório atual, branch do repositório, grupo da aba |
| Direita | codificação (`UTF-8`), sistema |

O nome do shell é o único item colorido — é o que distingue a aba de relance.

O mockup desenha a **contagem de painéis**, que **não entra**: depende do [PRD-006](prd-006-paineis-divididos.md) (`[v2]`, inexistente) e hoje exibiria "1 painel" para sempre. Volta com os painéis. Ver [ADR-0048](../adr/0048-barra-de-status.md) §3. O segmento de **branch** não estava no desenho e entrou depois, pelo [ADR-0049](../adr/0049-branch-git-na-barra-de-status.md).

## Requisitos

- **RF-9.1** — Barra opcional, ligável e desligável na config. **Ligada por padrão** — a decisão que o rascunho deixava em aberto, resolvida no [ADR-0048](../adr/0048-barra-de-status.md) §6: desligada por padrão, ela anula o RF-9.4, porque quem não sabe que o OSC 7 pode faltar nunca vai ligá-la para descobrir. Desligada, a barra não desenha **nem ocupa altura**.
- **RF-9.2** — Exibe da aba ativa: shell ou perfil, diretório atual, grupo.
- **RF-9.3** — Diretório abreviado quando longo, com `~` para o home. *(A segunda metade — caminho completo em tooltip — está **diferida**: `tooltip::Hover` está amarrado a `TabId` e generalizá-lo é refator próprio. O [ADR-0019](../adr/0019-tooltip.md) previu este consumidor, e ele continua previsto.)*
- **RF-9.4** — **Indicação visível de que o diretório não vem de OSC 7** — o dado é o de spawn, não o atual. É o requisito que dá razão de existir a esta barra ([ADR-0005](../adr/0005-persistencia-de-sessao.md)). A indicação é o **diretório em `#828a96`**, um degrau abaixo da cor de base na escada de texto da §1.4 — a mesma semântica que o [ADR-0037](../adr/0037-aba-nao-iniciada.md) deu ao rótulo da aba não iniciada (*este dado não é o real, é o que sabemos*), sem o alfa dele, que a 10.5px deixava o caminho ilegível ([ADR-0048](../adr/0048-barra-de-status.md) §4).
- **RF-9.5** — *(**Diferido.** Conteúdo de cada zona configurável a partir de um conjunto fechado de campos. Pede uma gramática de config nova, com catálogo fechado e validação, no peso do catálogo de ações; os cinco segmentos fixos entregam o RF-9.2 inteiro sem ela. [ADR-0048](../adr/0048-barra-de-status.md) §7.)*
- **RF-9.6** — Cores, altura e fonte configuráveis, como o resto do chrome ([PRD-004](prd-004-aparencia-do-chrome.md)).
- **RF-9.7** — *(**Diferido.** Clicar no campo de diretório copia o caminho. Arrasta hit-test com semântica de clique e um feedback de "copiado" que não existe fora da seleção do terminal — e é o requisito que tornaria a cessão dos 6px da borda de resize uma perda real. [ADR-0048](../adr/0048-barra-de-status.md) §5 e §7.)*
- **RF-9.8** — A barra atualiza sob o mesmo regime damage-driven do resto da UI ([ADR-0007](../adr/0007-modelo-de-threading.md)); não introduz timer nem redraw periódico.
- **RF-9.9** — Quando o diretório da aba pertence a um repositório Git, a barra exibe um **ícone** indicando isso e o **nome da branch** atual. Fora de um repositório, o segmento inteiro desaparece — o ícone é a resposta ao "estou num repositório?", e não há versão apagada dele. Estado da árvore fica de fora, ver "Fora de escopo". ([ADR-0049](../adr/0049-branch-git-na-barra-de-status.md), pedido do dono do produto depois de a barra estar em uso.)

RF-9.8 não é detalhe: uma barra de status com relógio ou uso de CPU quebraria a propriedade de "terminal ocioso custa zero frames", que é um princípio do produto. Qualquer campo que mude sozinho precisa ser avaliado contra isso.

## Questões que o rascunho deixou em aberto

Todas resolvidas pelo [ADR-0048](../adr/0048-barra-de-status.md).

| Pergunta | Resposta |
|---|---|
| Campos de sistema (versão, plataforma) valem o espaço, ou são ruído? | **A plataforma vale; a versão não.** São constantes e não custam frame, mas a versão não muda entre execuções, e o que não muda não é o que se consulta de relance — saiu depois de aparecer em tela ([ADR-0048](../adr/0048-barra-de-status.md) §10). |
| Contagem de painéis só faz sentido com [PRD-006](prd-006-paineis-divididos.md) implementado. | **Sai do escopo** até os painéis existirem (§3). |
| Barra por janela ou por aba? | **Por janela**, lendo a aba ativa. Uma barra por aba seria N barras invisíveis exceto uma — o mesmo desenho, com estado a mais. |
| Exibe indicadores de atividade e campainha ([PRD-001](prd-001-abas.md) RF-1.20, RF-1.21), ou eles ficam só na aba? | **Só na aba.** Os indicadores existem para dizer o que acontece em abas que **não** estão à vista; a barra descreve a aba ativa, onde não há o que sinalizar. |

## Fora de escopo

Campos definidos por script; relógio; medidores de CPU e memória — todos violam RF-9.8 ou dependem de lógica programável, descartada no v1 pelo [ADR-0003](../adr/0003-formato-de-configuracao.md).

**Integração com `git`** estava nesta lista inteira e foi **partida em duas** pelo [ADR-0049](../adr/0049-branch-git-na-barra-de-status.md), que mediu o custo de cada metade:

- **Branch — entrou** (RF-9.9 acima). Ler `.git/HEAD` é um arquivo de ~30 bytes, revalidado por `mtime`; não precisa de thread, temporizador nem dependência nova, e portanto não toca o RF-9.8.
- **Estado da árvore — continua fora.** Sujo/limpo, ahead/behind, o que está em stage: todos precisam percorrer a árvore, e nenhum tem um sinal barato que diga quando refazer. É fronteira, não pendência.

Também fora, pelo mesmo RF-9.8: a **contagem de processos da aba** ([ADR-0034](../adr/0034-deteccao-de-processo-ativo-para-confirmacao.md)). Ela mudaria por evento, mas lê-la custa uma varredura de `sysinfo` — cara demais para o caminho de render.
