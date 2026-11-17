# ADR-0103 — Acesso remoto: três partes, o desktop como host e o protocolo num repositório próprio

**Status:** Aceito em 2026-11-16 — implementado em 2026-11-17 ([PRD-025](../prd/prd-025-acesso-remoto.md); verificação ao vivo é dívida, no [roadmap](../roadmap.md))
**Data:** 2026-11-15
**Relacionados:** [PRD-000](../prd/prd-000-visao-de-produto.md), [PRD-025](../prd/prd-025-acesso-remoto.md), [ADR-0002](0002-motor-vte.md), [ADR-0007](0007-modelo-de-threading.md), [ADR-0071](0071-backend-de-terminal.md), [ADR-0104](0104-crate-porecatu-remote.md), [ADR-0105](0105-espelhamento-da-grade-e-input-remoto.md), [ADR-0106](0106-pareamento-identidade-e-autorizacao.md)

## Contexto

O [PRD-025](../prd/prd-025-acesso-remoto.md) pede que um app mobile alcance a sessão viva do Porecatu. O [PRD-000](../prd/prd-000-visao-de-produto.md) lista "multiplexação remota (anexar a sessão em servidor, estilo `tmux -CC`)" como não-objetivo, reafirmado na emenda de 2026-10-27. É preciso dizer o que muda nesse não-objetivo e o que continua valendo, antes de desenhar qualquer peça.

Restrições que vêm de decisões do dono:

- o desktop não pode virar servidor exposto: atrás de NAT, em rede corporativa, num notebook que muda de rede. Alguém de fora tem de ser o ponto de encontro;
- esse ponto de encontro é **auto-hospedado** e **cego**: não lê o que passa por ele;
- o app é **Expo (React Native)**, em TypeScript. O desktop e o relay são Rust. O protocolo terá, portanto, duas implementações em linguagens diferentes.

## Decisão

**O acesso remoto tem três partes — o Porecatu desktop como host, o Porecatu Relay como ponto de encontro cego e o Porecatu Remote como cliente — e o protocolo entre elas é especificado num quarto repositório, `porecatu-protocol`, que é a fonte normativa para os três.**

### 1. O host é o desktop, e só ele tem estado

O processo do Porecatu é o **host**: dono do `Workspace`, dos PTYs, do motor VT e do tema. O cliente nunca guarda estado de workspace e não interpreta sequências VT ([ADR-0105](0105-espelhamento-da-grade-e-input-remoto.md)). O relay não guarda nada.

Isso é o que separa o recurso do não-objetivo do PRD-000: não existe sessão que viva fora do desktop. Fechar o Porecatu encerra tudo, como hoje. O que muda é que a sessão viva **pode ser operada de outro aparelho** enquanto existe. A emenda no PRD-000 registra a distinção.

### 2. O host sai, nunca escuta

O host abre uma conexão de saída para o relay (`wss://`) e mantém essa conexão. Não há porta aberta no desktop, não há UPnP, não há descoberta na rede local no v1. O celular também sai para o relay. É o que funciona atrás de qualquer NAT ou firewall que deixe passar HTTPS.

### 3. O relay é cego

Host e cliente estabelecem uma sessão **Noise** de ponta a ponta dentro do canal que o relay encaminha ([ADR-0106](0106-pareamento-identidade-e-autorizacao.md)). O relay vê endereços IP, horários, o `host_id` e a chave pública de roteamento do host, quantos canais cada host tem abertos e o tamanho e o momento de cada quadro — a lista fechada de [01 — Transporte e relay §7](https://github.com/porecatu/protocol/blob/main/docs/especificacao/01-transporte-e-relay.md), que o [modelo de ameaça](https://github.com/porecatu/protocol/blob/main/docs/especificacao/modelo-de-ameaca.md) do protocolo usa — e nada de conteúdo.

### 4. Quatro repositórios

| Repositório | Conteúdo | Licença |
|---|---|---|
| [porecatu/terminal](https://github.com/porecatu/terminal) (este) | O host: crate `porecatu-remote` e a integração em `porecatu-ui` | GPL-3.0-or-later |
| [porecatu/protocol](https://github.com/porecatu/protocol) | Especificação normativa, vetores de teste, crate Rust `porecatu-protocol` e pacote TS `@porecatu/protocol` | GPL-3.0-or-later |
| [porecatu/relay](https://github.com/porecatu/relay) | O relay (Rust, `tokio` + `axum`) | GPL-3.0-or-later |
| [porecatu/remote](https://github.com/porecatu/remote) | O app (Expo, TypeScript) | GPL-3.0-or-later |

A especificação mora fora deste repositório porque três projetos a seguem e dois deles não são Rust: o lugar neutro evita que a forma do fio seja decidida pelo que é conveniente para o desktop. **Mudança no fio começa no `porecatu-protocol`** (ADR lá, versão subida, vetor novo), e este repositório acompanha num PR próprio.

### 5. O crate de protocolo é dependência externa pinada

O Porecatu depende do crate `porecatu-protocol` por **versão exata**, como já faz com `alacritty_terminal`, `wgpu`, `toml_edit` e `russh`. O crate ainda **não foi publicado** no crates.io (a publicação é a fase P3 do [roadmap do protocolo](https://github.com/porecatu/protocol/blob/main/docs/roadmap.md), junto com o congelamento do fio): até lá, a dependência é por `git` com `rev` de um commit e a versão exata ao lado (`version = "=0.1.0"`), a mesma forma que o [`Cargo.toml` do Porecatu Relay](https://github.com/porecatu/relay/blob/main/Cargo.toml) usa, e passa a só versão exata quando o crate sair. Só `porecatu-remote` o declara ([ADR-0104](0104-crate-porecatu-remote.md)); `porecatu-ui` vê os tipos que precisa pelo re-export de `porecatu-remote`, o mesmo padrão de `porecatu-term` com `porecatu-pty`.

### 6. Versão do protocolo

A versão `[major, minor]` é negociada no início da sessão (`hello`/`welcome`; o `major` vai também no prólogo Noise e no `v` do QR); enquanto `major = 0`, host e cliente exigem `minor` igual ([04 — Versionamento](https://github.com/porecatu/protocol/blob/main/docs/especificacao/04-versionamento.md)). A versão de fio hoje é `[0, 1]`, ainda não congelada, e não se confunde com a versão do texto da especificação (0.3) nem com a do crate (0.1.0). O Porecatu declara, no `CHANGELOG`, a versão de protocolo de cada release a partir da que trouxer o recurso.

> **Nota de implementação (2026-11-17).** O host foi implementado contra a especificação **0.4** do protocolo, não a 0.3: a 0.4 acrescenta a ação `activate-tab` e corrige pontos do texto da 0.3 que a implementação expôs ([ADR-0109](0109-o-celular-muda-a-aba-ativa-do-desktop.md), ADR-0010 do `porecatu-protocol`). O fio continua `[0, 1]` e o crate continua `0.1.0`; o que mudou é o `rev` — `0909e7ca88e3c180198e8ce7246445c4b0d9fbd1`, o commit de merge da 0.4 na `main` do `porecatu-protocol` (PR #2 de lá, mesclado em 2026-11-17 com o CI verde nas três plataformas), o mesmo que o Relay usa. O crate de protocolo continua não publicado. Ver o [roadmap](../roadmap.md).

## Alternativas consideradas

### Sessão no servidor (estilo `tmux -CC` ou servidor de multiplexação)

Os PTYs rodariam num servidor e o desktop seria mais um cliente. Resolve "acessar com o desktop desligado", mas inverte o produto: o Porecatu deixaria de ser um emulador com sessão local e passaria a ser cliente de um multiplexador, que é exatamente a "camada extra" que o princípio 2 do PRD-000 recusa. Continua não-objetivo.

### Desktop como servidor direto (porta aberta, ou túnel de terceiros)

Sem relay, o celular falaria direto com o desktop. Exige porta aberta e IP alcançável, ou um serviço de túnel de terceiro (que não seria cego nem auto-hospedado). Conexão direta na rede local fica como evolução possível, sobre o mesmo protocolo.

### Especificação dentro deste repositório

Tinha a vantagem de mudar o fio e o host num PR só. Foi a recomendação inicial, recusada pelo dono em favor de um repositório próprio: o protocolo tem duas implementações de referência e vetores próprios, e o ritmo dele não é o do desktop.

### Relay que entende o protocolo (só TLS até ele)

Mais simples de implementar e permitiria recursos no servidor, mas quem opera o relay leria tudo que é digitado. Recusado pelo dono.

## Consequências

### Positivas

- O desktop continua sendo o único dono do estado: nenhuma divergência entre "o que o celular acha" e "o que existe".
- Nenhuma porta aberta no desktop; funciona de qualquer rede que alcance o relay.
- O relay pode ser operado por alguém em quem o usuário não confia para conteúdo.
- O protocolo tem dono e versão próprios, e as duas implementações se conferem pelos mesmos vetores.

### Negativas

- Quatro repositórios para manter em sincronia; mudança de fio custa PRs em pelo menos três deles.
- O Porecatu passa a ter um recurso de rede de entrada (ainda que por conexão de saída), com tudo o que isso pede de cuidado ([ADR-0106](0106-pareamento-identidade-e-autorizacao.md)).
- O usuário precisa de um servidor próprio.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Fio do protocolo mudar sem o host acompanhar | Média | Alto | Versão negociada; crate pinado exato; vetores rodando no CI do `porecatu-protocol` |
| Recurso ser lido como volta do não-objetivo "multiplexação remota" | Média | Baixo | Emenda no PRD-000 com a distinção do §1 |
| Relay comprometido | Baixa | Médio | Cegueira por construção; só metadados ficam expostos |
