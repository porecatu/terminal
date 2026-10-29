# PRD-020 — Conexões SSH

**Status:** Aprovado
**Data:** 2026-10-27
**Requisito de origem:** pedido direto do dono do produto — *"o estabelecimento de sessões SSH"* —, com as decisões de forma tomadas por ele na mesma conversa (ver "Decisões do dono")
**Relacionados:** [PRD-007](prd-007-perfis-de-aba.md), [ADR-0070](../adr/0070-perfis-de-aba.md), [ADR-0071](../adr/0071-backend-de-terminal.md), [ADR-0072](../adr/0072-cliente-ssh-embutido.md), [ADR-0073](../adr/0073-credenciais-no-cofre-do-sistema.md), [ADR-0074](../adr/0074-verificacao-da-chave-do-host.md), [ADR-0075](../adr/0075-superficies-de-ui-do-ssh.md), [ADR-0076](../adr/0076-reconexao-automatica.md), [ADR-0004](../adr/0004-pty-cross-platform.md), [ADR-0007](../adr/0007-modelo-de-threading.md), [ADR-0010](../adr/0010-licenciamento.md), [ADR-0012](../adr/0012-identificacao-do-terminal.md), [ADR-0013](../adr/0013-mouse-selecao-e-clipboard.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md), [ADR-0034](../adr/0034-deteccao-de-processo-ativo-para-confirmacao.md), [ADR-0037](../adr/0037-aba-nao-iniciada.md), [ADR-0051](../adr/0051-arquivo-de-projeto-porecatu.md), [ADR-0053](../adr/0053-paineis-divididos.md), [ADR-0058](../adr/0058-escrita-do-arquivo-de-configuracao.md), [ADR-0060](../adr/0060-anatomia-da-tela-de-configuracoes.md), [PRD-001](prd-001-abas.md), [PRD-003](prd-003-persistencia-de-sessao.md), [PRD-009](prd-009-barra-de-status.md), [PRD-016](prd-016-tela-de-configuracoes.md)

> Aprovado em 2026-10-27, por decisão do dono do produto, **fora da ordem de fases**, junto com a promoção do [PRD-007](prd-007-perfis-de-aba.md). Uma conexão SSH é um **perfil de aba do tipo `ssh`** (RF-7.4): tudo o que o PRD-007 diz de perfis — menu, badge, atalho, perfil do painel, gravação na sessão — vale aqui, e este documento só diz o que é próprio da conexão.

## Problema

Hoje o usuário acessa um servidor digitando `ssh deploy@10.4.1.20` num shell local. Funciona, e tem quatro custos que o Porecatu, sendo um gerenciador de muitos terminais, consegue tirar:

- **O destino não existe para o app.** A aba se chama `pwsh` até o servidor emitir um título; o badge, a barra de status e a sessão salva não sabem que ali dentro há uma máquina remota. Restaurar a sessão devolve um shell local no diretório local — o [ADR-0005](../adr/0005-persistencia-de-sessao.md) registrou isso como "aceito".
- **A queda de rede mata a aba.** O `ssh` sai com código 255, o shell local volta, e o usuário reconecta à mão, procurando o comando no histórico.
- **Fechar a aba não pergunta.** O [ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md) registrou: `ssh` parado num prompt remoto fecha sem confirmação, porque para o app é só um processo filho do shell.
- **Cada destino é um comando a lembrar.** Host, porta, usuário, chave — o `~/.ssh/config` resolve para quem o mantém, e só para quem o mantém.

## Usuário-alvo

O do [PRD-000](prd-000-visao-de-produto.md) que **acessa servidores com frequência**: desenvolvedor que entra em máquinas de homologação e produção, operador de infraestrutura, quem administra uma VPS. Quem usa SSH uma vez por mês continua digitando `ssh` no shell local, e nada aqui o impede.

## Decisões do dono

Tomadas em 2026-10-27, antes da escrita deste documento. Cada uma tem alternativa razoável; estão aqui para não serem reabertas sem ele.

| Pergunta | Decisão |
|---|---|
| Como conectar | **Cliente SSH embutido** no app, não o `ssh` do sistema lançado num PTY |
| Onde vivem os hosts | **Cadastro no app** — perfis do tipo `ssh`, editáveis na tela de configurações |
| Onde vivem as senhas | **No cofre do sistema operacional** (Gerenciador de Credenciais do Windows, Keychain do macOS, Secret Service no Linux), nunca em arquivo do app |
| Autenticação na primeira entrega | **Senha, chave privada (com passphrase) e agente SSH**. Keyboard-interactive geral e segundo fator ficam fora (RF-20.11) |
| Chave do host | **`~/.ssh/known_hosts` compartilhado com o OpenSSH** — lido e gravado |
| Extras na primeira entrega | **Importar do `~/.ssh/config`** e **reconexão automática**. ProxyJump e encaminhamento de porta ficam fora |
| Restauração de sessão | A aba SSH volta **não iniciada** e só conecta por gesto do usuário |
| Fechar aba com conexão aberta | **Pede confirmação** |
| Reconexão | **Até 5 tentativas, com intervalo crescente** |

## Em uma tela

```
  ┌──────────────────────────────────────────────────────────────────────┐
  │ [ api ▾ │ SSH prod-web-01  ✕ │ PS pwsh  ✕ │ + ]                [▣][⚙]│
  ├──────────────────────────────────────────────────────────────────────┤
  │ deploy@prod-web-01:/srv/porecatu$ journalctl -fu porecatu            │
  │ ...                                                                  │
  │ ── conexão perdida; reconectando (tentativa 2 de 5) em 2 s ──        │
  │                                                                      │
  ├──────────────────────────────────────────────────────────────────────┤
  │ deploy@prod-web-01 · /srv/porecatu                     UTF-8 Windows │
  └──────────────────────────────────────────────────────────────────────┘
```

## Requisitos funcionais

### Cadastro

**RF-20.1** — Um destino SSH é um **perfil do tipo `ssh`** (RF-7.4), com os campos comuns de todo perfil (identificador, nome, badge — `SSH` quando omitido) e os próprios: **host**, **porta** (22 quando omitida), **usuário**, **método de autenticação** e, para chave, o **caminho do arquivo da chave**. A forma exata é do ADR-0072.

**RF-20.2** — O perfil SSH **não tem campo de segredo**. Senha e passphrase vivem só no cofre do sistema (RF-20.13); o arquivo da chave privada é referenciado pelo caminho, nunca copiado.

**RF-20.3** — Perfis SSH são criados, editados e excluídos **na tela de configurações**, num grupo novo de perfis que lista todos os perfis — locais e SSH — e edita cada um num formulário. A tela escreve no `porecatu.toml`, como toda opção dela ([ADR-0058](../adr/0058-escrita-do-arquivo-de-configuracao.md)); o arquivo continua sendo a única fonte de verdade, e editar à mão continua valendo. A anatomia é do ADR-0075.

> **Emenda, 2026-10-29 ([ADR-0080](../adr/0080-editor-de-perfil-em-janela-propria.md)).** O formulário abre numa **janela própria**, à frente da tela de configurações e modal sobre ela, com o rótulo acima de cada campo. Abre pelo lápis da linha (à esquerda do ✕), por duplo clique na linha ou por `Enter` na linha escolhida; "+ Perfil local" e "+ Perfil SSH" o abrem com um perfil novo. **OK** leva o perfil às alterações pendentes da tela e **não grava** o arquivo — gravar continua sendo o Salvar da tela; OK não fecha enquanto houver campo inválido. **Cancelar** (ou fechar a janela, ou `Esc`) não muda nada e, num perfil novo, não cria nada. A importação do `~/.ssh/config` continua no painel do grupo.

**RF-20.4** — Excluir um perfil SSH pergunta, no mesmo diálogo de confirmação, se a senha e a passphrase salvas para ele no cofre também devem ser apagadas. A resposta padrão é **apagar**. A pergunta **não aparece** quando outro perfil aponta para o mesmo destino ou o mesmo arquivo de chave: o segredo é da conta no servidor, não do perfil, e apagá-lo quebraria o outro perfil ([ADR-0073](../adr/0073-credenciais-no-cofre-do-sistema.md) §4).

### Importar do `~/.ssh/config`

**RF-20.5** — A tela de configurações oferece **importar do `~/.ssh/config`**: lê o arquivo, lista os blocos `Host` que nomeiam um destino concreto (sem `*`, `?` nem `!`), com `HostName`, `User`, `Port` e `IdentityFile` resolvidos, e o usuário marca quais viram perfis. Os escolhidos são gravados como perfis SSH comuns.

**RF-20.6** — A importação é **uma cópia, não um vínculo**: mudar o `~/.ssh/config` depois não muda os perfis importados. Bloco que usa algo que o cliente embutido não suporta (`ProxyJump`, `ProxyCommand`, `LocalForward`, `RemoteForward`, `DynamicForward`, `CertificateFile`) aparece na lista **desabilitado**, com o motivo — nunca importado pela metade.

**RF-20.7** — Bloco cujo destino já é um perfil SSH (mesmo host, porta e usuário) aparece marcado como já importado. Importar não sobrescreve perfil existente.

### Conectar e autenticar

**RF-20.8** — Abrir uma aba com um perfil SSH — por qualquer caminho do PRD-007 — conecta ao host em segundo plano. Enquanto conecta, o grid mostra uma nota de "conectando a `host`"; a interface **nunca** espera a rede.

**RF-20.9** — Os métodos de autenticação são tentados nesta ordem, parando no primeiro aceito:
1. **agente SSH** — o agente do OpenSSH (pipe nomeado no Windows, `SSH_AUTH_SOCK` no Linux e no macOS); ausente, o passo é pulado sem aviso;
2. **chave privada**, quando o perfil aponta uma; chave cifrada pede a passphrase (RF-20.12);
3. **senha**, pedida ao usuário (RF-20.12), quando o servidor a aceita.

O perfil pode restringir a lista a um método só.

**RF-20.10** — Servidor que oferece senha só por **keyboard-interactive** com **um único campo oculto** é atendido como senha — é como a maioria dos servidores com PAM pede senha, e recusá-lo deixaria a autenticação por senha inútil na prática.

**RF-20.11** — Keyboard-interactive com mais de um campo, campo visível, ou segundo fator (OTP, push) **não é suportado**: a autenticação falha com uma nota no grid dizendo que o servidor pede um método que o Porecatu não atende, e sugerindo o `ssh` do sistema.

**RF-20.12** — Pedir senha ou passphrase abre um **diálogo próprio** ([ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md)) com um campo **mascarado**, o destino (`usuário@host`) ou o arquivo da chave no corpo, e uma caixa **"Lembrar no cofre do sistema"**, desmarcada por padrão. Cancelar encerra a tentativa de conexão; a aba fica com a nota de autenticação cancelada.

**RF-20.13** — Segredo marcado para lembrar é gravado no cofre do sistema **só depois de a autenticação ter dado certo** — senha errada nunca fica salva. Na próxima conexão o segredo salvo é tentado primeiro; recusado pelo servidor, é **apagado** do cofre e o diálogo volta.

**RF-20.14** — Sem cofre disponível (Linux sem Secret Service), a caixa "Lembrar" aparece desabilitada com o motivo, e o segredo é pedido a cada conexão. Não há segundo lugar para guardá-lo.

### Chave do host

**RF-20.15** — A chave do host é conferida contra o **`~/.ssh/known_hosts`** do usuário, o mesmo do OpenSSH, incluindo entradas com nome de host em hash.

**RF-20.16** — Host **desconhecido** abre um diálogo com o host, o tipo da chave e a **impressão digital SHA-256**, no formato do OpenSSH (`SHA256:…`), com o foco inicial no cancelar. Aceitar acrescenta a entrada ao `known_hosts` e segue a conexão; cancelar encerra a tentativa.

**RF-20.17** — Chave **diferente** da registrada **bloqueia** a conexão, sem botão de continuar: o grid mostra uma nota com a impressão digital nova, a antiga e a linha do `known_hosts` que diverge. Resolver é tarefa do usuário, fora do app — é o mesmo comportamento do OpenSSH, e pela mesma razão.

**RF-20.18** — Entrada marcada `@revoked` bloqueia como chave diferente. Host coberto só por `@cert-authority` é tratado como **desconhecido** (certificados de host ficam fora, RF-20.37).

### A aba conectada

**RF-20.19** — O terminal remoto recebe `TERM=xterm-256color` ([ADR-0012](../adr/0012-identificacao-do-terminal.md)) e o tamanho da grade, e cada resize da aba vira um pedido de mudança de janela no canal SSH.

**RF-20.20** — O badge é o do perfil (RF-7.13). O título segue a mesma precedência de toda aba ([ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md)); sem título customizado e sem OSC 0/2, o rótulo é o **nome do perfil**, não o nome de um shell.

**RF-20.21** — A barra de status mostra `usuário@host` no lugar do nome do shell, e o diretório **remoto** quando o shell do servidor emite OSC 7 com aquele host. Os segmentos de Git (branch e commits atrás, [PRD-009](prd-009-barra-de-status.md), [PRD-013](prd-013-sincronizacao-com-o-remoto-do-git.md)) **não aparecem** numa aba SSH: o repositório que o app enxerga é local.

**RF-20.22** — O `cwd` por OSC 7 de uma aba SSH é **remoto** e nunca é usado como diretório local: `tab.new` a partir de uma aba SSH abre o perfil padrão no `startup_directory`, e o `.porecatu` ([PRD-012](prd-012-comando-de-projeto-por-diretorio.md)) nunca roda numa aba SSH.

**RF-20.23** — **Dividir** um painel SSH abre um **segundo canal na mesma conexão**, com um shell novo no servidor: não reautentica, não pede senha de novo, e não abre outra conexão TCP. Se a conexão daquele painel não está viva, o painel novo abre uma conexão própria, como uma aba nova do perfil.

**RF-20.24** — OSC 52 (escrever no clipboard local) vale numa aba SSH como em qualquer aba ([ADR-0013](../adr/0013-mouse-selecao-e-clipboard.md)): é o caso de uso que motivou a chave. **Não é fronteira de confiança nova**: o ADR-0013 decidiu a escrita **para** o caso remoto (`tmux`/`nvim` sobre SSH), pesou o risco de um programa escrever no clipboard e deixou a **leitura** negada, que é o lado perigoso; uma aba `pwsh` rodando `ssh` já expõe exatamente o mesmo. Quem não quer servidor nenhum escrevendo no clipboard desliga a chave, que vale para todas as abas.

### Fechar

**RF-20.25** — Fechar uma aba ou um painel com conexão SSH **aberta** — conectada, autenticando ou reconectando — **pede confirmação**, mesmo sem programa em tela cheia, atrás da mesma chave `general.confirm_close_with_process` do RF-1.6. Aba SSH já encerrada ou nunca iniciada fecha sem perguntar.

**RF-20.26** — Quando o **servidor** encerra a sessão (o usuário digitou `exit`, o shell remoto saiu), a aba se comporta como uma aba local cujo processo saiu ([ADR-0017](../adr/0017-ciclo-de-vida-da-aba.md)): código 0 fecha, outro código mantém a aba com a nota. Encerramento pelo servidor **não** dispara reconexão.

### Reconexão

**RF-20.27** — Conexão que cai **sem** o servidor ter encerrado a sessão — rede, suspensão da máquina, servidor reiniciado, keepalive sem resposta — dispara **reconexão automática**: até **5 tentativas**, com intervalo crescente entre elas. O intervalo exato é do ADR-0076.

**RF-20.28** — Cada tentativa escreve uma nota no grid (`conexão perdida; reconectando, tentativa N de 5`), **abaixo do conteúdo que já estava lá**: o scrollback da sessão anterior fica inteiro, e o shell novo começa embaixo dele.

**RF-20.29** — O processo remoto da sessão perdida **não é retomado** — é outro shell, num login novo. A nota diz isso uma vez, na primeira reconexão bem-sucedida. Retomar sessão remota é multiplexação, que é não-objetivo ([PRD-000](prd-000-visao-de-produto.md)).

**RF-20.30** — Reconexão que pede um segredo **não salvo** abre o diálogo de novo (RF-20.12); cancelar interrompe as tentativas. Chave do host que mudou entre a queda e a reconexão bloqueia como no RF-20.17, sem mais tentativas.

**RF-20.31** — Esgotadas as tentativas, a aba fica encerrada com a nota da última falha. A ação `ssh.reconnect` — também no menu de contexto da aba — reconecta à mão uma aba SSH encerrada ou que esgotou as tentativas, recomeçando a contagem.

### Sessão e restauração

**RF-20.32** — A sessão grava o perfil do painel SSH (RF-7.16) e **nada da conexão**: nem segredo, nem chave de sessão, nem o diretório remoto.

**RF-20.33** — Na restauração — no arranque e de uma sessão nomeada —, todo painel SSH volta **não iniciado** ([ADR-0037](../adr/0037-aba-nao-iniciada.md)), **mesmo com `lazy_restore = false`** e **mesmo sendo o painel ativo**. Ele mostra uma nota dizendo a qual destino pertence e conecta no primeiro gesto do usuário sobre ele: clique no painel ou `Enter`. Abrir o app nunca dispara conexão, pedido de senha ou diálogo de chave de host sozinho.

### Segurança e privacidade

**RF-20.34** — Nenhum segredo — senha, passphrase, conteúdo de chave privada — aparece em arquivo do app (`porecatu.toml`, `session.json`, sessões nomeadas), em nota, aviso, título, árvore de acessibilidade ou saída de diagnóstico.

**RF-20.35** — O campo mascarado não copia para o clipboard, não aceita colar por OSC 52 e não é exposto como texto na árvore de acessibilidade — só como campo de senha.

**RF-20.36** — Encaminhamento de agente, de porta e de X11 ficam **desligados** e não são oferecidos.

## Fora de escopo

Cada item é decisão, não esquecimento.

- **RF-20.37 — Certificados de host e de usuário** (`@cert-authority`, `CertificateFile`).
- **Segundo fator e keyboard-interactive geral** (RF-20.11).
- **ProxyJump, ProxyCommand, bastion.**
- **Encaminhamento de porta, de agente e de X11** (RF-20.36).
- **SFTP, transferência de arquivo, arrastar arquivo para a aba.**
- **GSSAPI/Kerberos.**
- **Retomar a sessão remota após queda** (RF-20.29) — `tmux`/`screen` no servidor continuam sendo o caminho, e funcionam dentro da aba.
- **Usar o `~/.ssh/config` ao vivo**, como fonte de verdade em paralelo ao cadastro (RF-20.6).
- **Mosh, telnet, serial.**
- **Sincronizar hosts entre máquinas.**

## Cenários

```gherkin
Cenário: conectar com o agente
  Dado o perfil SSH "prod" para deploy@10.4.1.20, sem método restrito
  E o agente do OpenSSH rodando com a chave aceita pelo servidor
  Quando o usuário abre uma aba do perfil "prod"
  Então o grid mostra "conectando a 10.4.1.20"
  E o prompt remoto aparece sem nenhum diálogo

Cenário: host desconhecido
  Dado um host que não está no ~/.ssh/known_hosts
  Quando o usuário abre uma aba do perfil daquele host
  Então um diálogo mostra o tipo da chave e a impressão digital SHA256
  E o foco inicial está no cancelar
  E aceitar acrescenta a linha ao ~/.ssh/known_hosts

Cenário: chave do host mudou
  Dado um host com outra chave no ~/.ssh/known_hosts
  Quando o usuário abre uma aba do perfil daquele host
  Então a conexão é bloqueada sem botão de continuar
  E o grid mostra a impressão digital nova, a antiga e a linha divergente

Cenário: senha lembrada só depois de dar certo
  Dado um perfil com autenticação por senha e nada salvo no cofre
  Quando o usuário digita a senha errada com "Lembrar" marcado
  Então nada é gravado no cofre
  E o diálogo volta
  Quando digita a senha certa com "Lembrar" marcado
  Então a senha é gravada no cofre
  E a próxima conexão não pede senha

Cenário: servidor reiniciado
  Dado uma aba SSH conectada, com saída no scrollback
  Quando o sshd do servidor é reiniciado
  Então a aba mostra a nota de reconexão com a tentativa e o total de 5
  E quando o sshd volta um shell novo abre abaixo do scrollback anterior
  E uma nota diz que o processo remoto anterior não foi retomado

Cenário: rede que some sem erro
  Dado uma aba SSH conectada
  Quando a rede deixa de entregar pacotes, sem fechar a conexão
  Então em até 60 segundos a aba mostra a nota de conexão perdida e de reconexão

Cenário: exit no servidor não reconecta
  Dado uma aba SSH conectada
  Quando o usuário digita exit no shell remoto
  Então a aba fecha
  E nenhuma reconexão é tentada

Cenário: fechar pede confirmação
  Dado uma aba SSH conectada, parada no prompt remoto
  Quando o usuário fecha a aba
  Então o diálogo de confirmação aparece

Cenário: restauração não conecta sozinha
  Dado uma sessão gravada com uma aba SSH ativa e lazy_restore = false
  Quando o app abre
  Então nenhuma conexão é feita e nenhum diálogo aparece
  E a aba mostra a qual destino pertence
  Quando o usuário tecla Enter nela
  Então a conexão começa

Cenário: dividir reaproveita a conexão
  Dado uma aba SSH conectada por senha não salva
  Quando o usuário divide o painel
  Então o painel novo abre um shell no mesmo servidor
  E nenhuma senha é pedida

Cenário: importar do ssh_config
  Dado um ~/.ssh/config com "Host prod" e "Host *.interno" e "Host bastion" com ProxyJump
  Quando o usuário abre a importação na tela de configurações
  Então "prod" aparece selecionável
  E "*.interno" não aparece
  E "bastion" aparece desabilitado, com o motivo
```

## Onde cada coisa é decidida

| Tema | Documento |
|---|---|
| Perfis, menu, badge, gravação na sessão | [PRD-007](prd-007-perfis-de-aba.md), [ADR-0070](../adr/0070-perfis-de-aba.md) |
| O terminal com outro backend além do PTY local | [ADR-0071](../adr/0071-backend-de-terminal.md) |
| Biblioteca SSH, crate novo, runtime assíncrono isolado, forma do perfil `ssh`, agente | [ADR-0072](../adr/0072-cliente-ssh-embutido.md) |
| Cofre do sistema e o ciclo de vida do segredo | [ADR-0073](../adr/0073-credenciais-no-cofre-do-sistema.md) |
| `known_hosts`: leitura, gravação, impressão digital | [ADR-0074](../adr/0074-verificacao-da-chave-do-host.md) |
| Campo mascarado, diálogos de segredo e de chave do host, grupo de perfis na tela de configurações, importação | [ADR-0075](../adr/0075-superficies-de-ui-do-ssh.md) |
| Keepalive, detecção de queda, tentativas e intervalo | [ADR-0076](../adr/0076-reconexao-automatica.md) |

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Segredos em arquivo, nota, aviso, título ou log do app | **zero** |
| Conexões, diálogos ou pedidos de senha disparados pela abertura do app | **zero** |
| Gestos para abrir um servidor cadastrado | **dois** (clique direito no `+`, clique no perfil), **um** com atalho |
| Gestos para voltar a um servidor depois de uma queda curta de rede | **zero** |
| Hosts confiados no OpenSSH que o Porecatu pede para confirmar de novo | **zero** |
| Conexão aceita com chave de host diferente da registrada | **zero** |
| Quadros perdidos pela interface durante conexão, autenticação ou reconexão | **zero** — nada de rede na main thread |
