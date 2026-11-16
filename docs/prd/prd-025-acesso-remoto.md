# PRD-025 — Acesso remoto

**Status:** Rascunho — decisões de produto tomadas pelo dono em 2026-11-15; texto aguardando revisão
**Data:** 2026-11-15
**Requisito de origem:** pedido direto do dono do produto: acessar uma sessão viva do Porecatu a partir de um app mobile (Android e iOS), pelo meio de um servidor intermediário, para acompanhar e operar as abas longe do desktop.
**Relacionados:** [PRD-000](prd-000-visao-de-produto.md), [PRD-001](prd-001-abas.md), [PRD-002](prd-002-grupos-de-abas.md), [PRD-006](prd-006-paineis-divididos.md), [PRD-007](prd-007-perfis-de-aba.md), [PRD-009](prd-009-barra-de-status.md), [PRD-010](prd-010-interacao-e-superficie-de-app.md), [PRD-016](prd-016-tela-de-configuracoes.md), [PRD-020](prd-020-conexoes-ssh.md), [PRD-022](prd-022-abas-e-grupos-entre-janelas.md), [ADR-0007](../adr/0007-modelo-de-threading.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0032](../adr/0032-interface-do-v1-fechada.md), [ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md), [ADR-0072](../adr/0072-cliente-ssh-embutido.md), [ADR-0073](../adr/0073-credenciais-no-cofre-do-sistema.md), [ADR-0090](../adr/0090-transferencia-de-abas-entre-janelas.md), [ADR-0103](../adr/0103-acesso-remoto-tres-partes-e-protocolo-externo.md) a [ADR-0108](../adr/0108-superficies-de-ui-do-acesso-remoto.md)

> O dono decidiu, antes da escrita:
>
> - o acesso tem **três partes**: o Porecatu desktop (o **host**), um servidor intermediário **auto-hospedado**, o [Porecatu Relay](https://github.com/porecatu/relay), e um app mobile, o [Porecatu Remote](https://github.com/porecatu/remote), feito em **Expo (React Native)**. O protocolo entre eles é especificado num quarto repositório, o [porecatu-protocol](https://github.com/porecatu/protocol);
> - o relay é **cego**: tudo que é de terminal viaja **cifrado de ponta a ponta** entre o desktop e o celular, e o relay só roteia;
> - **não há relay público nem contas**: quem usa sobe o próprio relay; identidade é chave de aparelho, e o vínculo nasce de um **pareamento por QR code** mostrado na tela de configurações;
> - o celular recebe **a grade do desktop**, já resolvida pelo motor do Porecatu, e não roda emulador próprio;
> - **o desktop dita o tamanho** do terminal; o celular se adapta;
> - na primeira versão, o celular **vê e digita** nas abas, **cria e fecha** abas e **organiza** (grupos, mover entre grupos e janelas). **Notificação push fica fora**;
> - um celular pareado acessa **tudo**; o acesso remoto vem **desligado**, o desktop **mostra** quando há celular conectado e permite **desconectar e revogar** na hora;
> - um celular pode estar pareado com **vários desktops**;
> - **sem Porecatu aberto, não há acesso**: o celular mostra o desktop como offline. O ciclo de vida do app não muda;
> - os três projetos novos são **GPL-3.0-or-later**, como este.
>
> O como fica em seis ADRs:
>
> - a arquitetura de três partes e o protocolo externo: [ADR-0103](../adr/0103-acesso-remoto-tres-partes-e-protocolo-externo.md);
> - o crate `porecatu-remote`, com `tokio` e `keyring` próprios: [ADR-0104](../adr/0104-crate-porecatu-remote.md);
> - o espelhamento da grade e o input remoto: [ADR-0105](../adr/0105-espelhamento-da-grade-e-input-remoto.md);
> - o pareamento, a identidade e a autorização: [ADR-0106](../adr/0106-pareamento-identidade-e-autorizacao.md);
> - as ações remotas sobre o workspace: [ADR-0107](../adr/0107-acoes-remotas-sobre-o-workspace.md);
> - as superfícies de UI no desktop, com o aval visual pendente: [ADR-0108](../adr/0108-superficies-de-ui-do-acesso-remoto.md).

## Problema

O Porecatu é bom em manter quinze terminais organizados enquanto o desenvolvedor está na frente dele. Longe do desktop, a organização some: o build longo, o servidor que pode ter caído, o deploy que pede confirmação, a migração rodando — para saber de qualquer um deles, é preciso voltar à mesa, ou ter montado antes um `tmux` num servidor, ou expor o desktop por SSH à internet.

As três saídas têm o mesmo defeito: exigem preparar o terminal **antes** de saber que vai precisar dele de longe, e nenhuma enxerga a estrutura que o Porecatu já tem — grupos, nomes, painéis. O que o usuário quer é pegar o celular e ver **as mesmas abas**, nos mesmos grupos, com o mesmo conteúdo, e digitar nelas.

## O que já existe e não muda

- **Abas, grupos, fixadas, painéis e janelas** ([PRD-001](prd-001-abas.md), [PRD-002](prd-002-grupos-de-abas.md), [PRD-006](prd-006-paineis-divididos.md), [PRD-021](prd-021-abas-fixadas.md), [PRD-022](prd-022-abas-e-grupos-entre-janelas.md)). O celular é **mais um jeito de operar** o mesmo modelo, não um modelo paralelo.
- **As regras de cada gesto**: onde uma aba nova nasce, quando fechar pede confirmação, como uma aba muda de janela. O celular pede; o desktop executa pelo mesmo caminho.
- **A sessão gravada** ([PRD-003](prd-003-persistencia-de-sessao.md)): o acesso remoto não grava nada nela e não muda a restauração.
- **O ciclo de vida do app**: fechar a última janela continua encerrando o processo.
- **Terminal ocioso custa zero** ([PRD-000](prd-000-visao-de-produto.md), princípio 4): com o acesso desligado, nada roda; ligado e sem celular assinando um painel, nenhum terminal é lido a mais.
- **A aparência existente**: o ADR-0032 continua valendo; as superfícies novas entram pelo [ADR-0108](../adr/0108-superficies-de-ui-do-acesso-remoto.md).

## Usuário-alvo

O mesmo do [PRD-000](prd-000-visao-de-produto.md), mais uma condição: tem (ou está disposto a ter) um servidor acessível pela internet onde subir o relay — uma VPS, um homelab, um servidor da empresa. Quem não tem servidor não usa o recurso no v1; não há relay público.

## Requisitos funcionais

### Ligar e configurar

- **RF-25.1** — O acesso remoto é **desligado por padrão** (`[remote] enabled = false`). Desligado, o Porecatu não abre conexão de rede nenhuma por causa dele e não cria thread nem runtime.
- **RF-25.2** — Ligar exige o **endereço do relay** (`[remote] relay_url`, `wss://`) e o **token do operador** do relay. O endereço fica no `porecatu.toml`; o token, **no cofre do sistema** ([ADR-0073](../adr/0073-credenciais-no-cofre-do-sistema.md)), nunca no arquivo.
- **RF-25.3** — Ligar, desligar e trocar o relay **valem na hora**, sem reiniciar o app. Desligar derruba todas as conexões remotas imediatamente (RF-25.16).
- **RF-25.4** — O estado da conexão com o relay é visível na tela de configurações: desligado, conectando, conectado, falha (com o motivo: relay inacessível, token recusado, versão incompatível). Queda de rede é refeita sozinha, com espera crescente, sem aviso a cada tentativa.

### Parear

- **RF-25.5** — O usuário pareia um celular **a partir da tela de configurações**, com o acesso ligado e conectado ao relay: um gesto mostra um **QR code** com o endereço do relay, a identidade do desktop e um código de uso único.
- **RF-25.6** — O código do QR **vale 5 minutos e uma única vez**. Mostrar o QR de novo, ou sair da tela, invalida o anterior.
- **RF-25.7** — Quando um celular lê o QR, o desktop **pede confirmação** num diálogo com o **nome do aparelho** e a **impressão digital** da chave dele, a mesma que o celular mostra. Sem confirmação em 120 s, o pedido é recusado. Código inválido, vencido ou já usado é recusado sem diálogo ([02 — Handshake e pareamento](https://github.com/porecatu/protocol/blob/main/docs/especificacao/02-handshake-e-pareamento.md) §3.2).
- **RF-25.8** — Confirmado, o aparelho entra na **lista de aparelhos pareados** e a conexão já fica aberta, sem novo passo no celular.

### Aparelhos pareados

- **RF-25.9** — A tela de configurações lista os aparelhos pareados: nome, plataforma, data do pareamento, último acesso e se está **conectado agora**.
- **RF-25.10** — O usuário **revoga** um aparelho pela lista. A revogação derruba na hora a conexão viva daquele aparelho, e ele não conecta mais até parear de novo.
- **RF-25.11** — O usuário pode **recriar a identidade** do desktop: todos os pareamentos deixam de valer de uma vez (o caminho para "perdi o celular e não sei qual era").

### Indicador e controle no desktop

- **RF-25.12** — Enquanto houver **ao menos um celular conectado**, toda janela do Porecatu mostra um **indicador** disso, com a contagem. O indicador não pode ser desligado enquanto houver conexão: quem está na frente do desktop precisa saber que alguém está olhando.
- **RF-25.13** — O indicador leva a uma lista dos aparelhos conectados, com **desconectar** (derruba a conexão; o aparelho continua pareado) e o caminho para a revogação.
- **RF-25.14** — Cada **nova conexão** de aparelho gera um aviso de informação no desktop ([ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md)) com o nome do aparelho.
- **RF-25.15** — Há ações para ligar e desligar o acesso e para desconectar todos os aparelhos, **sem atalho padrão** (catálogo de ações, [ADR-0107](../adr/0107-acoes-remotas-sobre-o-workspace.md)).
- **RF-25.16** — Desligar o acesso remoto encerra todas as conexões, com aviso ao celular; a lista de pareados fica.

### O que o celular vê

- **RF-25.17** — O celular vê **todas as janelas** do processo, com as abas fixadas, os grupos (nome, cor, colapsado) e as abas na ordem da barra, cada aba com título, perfil, `usuário@host` nas SSH, indicadores de atividade e campainha, e a árvore de painéis com o estado de cada um.
- **RF-25.18** — O celular acompanha a **grade ao vivo** do painel que abre, com as cores do **tema ativo do desktop**, o cursor, e o **scrollback** sob demanda. Rolar no celular **não** rola o desktop.
- **RF-25.19** — O tamanho do terminal é **sempre o do desktop**. Abrir uma aba no celular nunca redimensiona o PTY nem muda o que o desktop mostra.
- **RF-25.20** — Mudanças no desktop (aba nova, título, grupo, aba movida) aparecem no celular em menos de 1 s em rede comum.

### O que o celular faz

- **RF-25.21** — O celular **digita** no painel aberto: texto, teclas especiais (Enter, Tab, Backspace, Esc, setas, Home/End, PgUp/PgDn, Insert, Delete, F1–F12) com modificadores (Ctrl, Alt, Shift, Meta), e **colar** ([03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md) §6). O desktop codifica as teclas com o **mesmo codificador do teclado local**, respeitando o modo do programa (cursor de aplicação, bracketed paste).
- **RF-25.22** — Tecla vinda do celular **nunca dispara atalho do Porecatu**: vai sempre ao terminal.
- **RF-25.23** — O celular **cria aba** numa janela escolhida, no grupo escolhido ou onde o `tab.new` a criaria, com o perfil escolhido ou o padrão.
- **RF-25.24** — O celular **fecha aba** e **fecha painel**. Quando o fechamento no desktop pediria confirmação (processo em primeiro plano, conexão SSH aberta), a confirmação é feita **no celular que pediu**, não no desktop. Fechar assim a última aba da última janela — por "fechar aba" ou pelo último painel dela — é recusado, porque encerraria o Porecatu e o próprio acesso ([ADR-0107](../adr/0107-acoes-remotas-sobre-o-workspace.md) §5).
- **RF-25.25** — O celular **renomeia** aba, **move** aba entre grupos e entre janelas, **cria grupo** com abas escolhidas, **renomeia**, **muda a cor**, **colapsa/expande** e **desfaz** grupo, e **move grupo** entre janelas. Mover entre janelas leva o processo vivo, pelo mesmo caminho do arraste ([ADR-0090](../adr/0090-transferencia-de-abas-entre-janelas.md)).
- **RF-25.26** — O celular **inicia** um painel não iniciado e **conecta** uma aba SSH dormente. Perguntas da conexão (senha, passphrase, chave de servidor desconhecida) aparecem **só no desktop** no v1; enquanto esperam resposta, o celular vê o painel aguardando o desktop (estado `awaiting-desktop`, [ADR-0107](../adr/0107-acoes-remotas-sobre-o-workspace.md) §6).
- **RF-25.27** — Nenhuma ação remota **muda o foco nem a aba ativa** do desktop. Aba criada pelo celular nasce sem roubar o foco de quem está no desktop.

### Limites

- **RF-25.28** — Ficam **fora do alcance do celular** no v1: fixar e desafixar, sessões nomeadas, configurações, o gerenciador de tarefas, abrir janela nova, destacar aba numa janela nova, e qualquer coisa que mude o foco do desktop.
- **RF-25.29** — Com o Porecatu fechado, o celular mostra o desktop como **offline**. Não há modo em segundo plano.

## Requisitos não funcionais

- **RNF-25.1** — Com `[remote] enabled = false`, custo zero: nenhuma thread, nenhum socket, nenhum runtime `tokio` criado por este recurso.
- **RNF-25.2** — Ligado e sem nenhum painel assinado, o custo é o de manter uma conexão WebSocket aberta: nenhum quadro desenhado a mais, nenhum terminal lido a mais.
- **RNF-25.3** — A main thread **nunca** faz I/O de rede nem criptografia ([ADR-0007](../adr/0007-modelo-de-threading.md)). Ela tira snapshots dos painéis assinados, no máximo 30 vezes por segundo por painel, e só dos que sujaram.
- **RNF-25.4** — Latência de tecla no celular até o eco na tela do celular: dominada pela rede; o desktop acrescenta no máximo um intervalo de espelhamento (33 ms).
- **RNF-25.5** — Nenhum conteúdo de terminal, tecla, título ou diretório vai para log, nem no desktop.
- **RNF-25.6** — Texto de interface novo segue o catálogo de idiomas ([ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md)); o protocolo não carrega prosa.

## Fora de escopo

| Item | Por quê |
|---|---|
| Relay público operado pelo projeto | Custo, abuso e obrigação de privacidade; decisão do dono |
| Notificações push | Exigiria credenciais da Apple e do Google no relay de cada usuário; fica para depois do v1 |
| Modo em segundo plano (bandeja) | Muda o ciclo de vida do app; PRD próprio se vier |
| Conexão direta na rede local, sem relay | Segundo transporte; depois do v1 |
| Cliente web | O protocolo permite; não é o pedido |
| Permissão por aparelho (só leitura) | Decisão do dono: pareado acessa tudo |
| Responder perguntas de SSH no celular | Segredo trafegando para outro aparelho; o v1 mantém no desktop |
| Sessão vivendo no servidor (estilo `tmux -CC`) | Continua não-objetivo ([PRD-000](prd-000-visao-de-produto.md)): o host é sempre o desktop |

## Riscos de produto

| Risco | Mitigação |
|---|---|
| Recurso de rede num app que até aqui não escutava nada amplia a superfície de ataque | Desligado por padrão; relay cego; pareamento presencial; indicador obrigatório; revogação imediata ([ADR-0106](../adr/0106-pareamento-identidade-e-autorizacao.md)) |
| Exigir servidor próprio limita a adoção | Aceito: o público-alvo é técnico. O relay é um binário único com imagem Docker |
| Grade larga do desktop ilegível no celular | Zoom e rolagem horizontal no app; o desktop não cede o tamanho por decisão do dono |
| Três repositórios novos e um protocolo para manter em sincronia | Especificação normativa com vetores de teste num repositório próprio; versão conferida no início da sessão (`hello`/`welcome`), com `minor` igual exigido enquanto o `major` é 0 ([04 — Versionamento](https://github.com/porecatu/protocol/blob/main/docs/especificacao/04-versionamento.md)) |
| GPL na App Store | Risco aceito pelo dono; registrado no [Porecatu Remote](https://github.com/porecatu/remote) |
