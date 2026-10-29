# ADR-0075 — Superfícies de UI do SSH: diálogo com campo mascarado e linhas de dado, pergunta que espera o foco da aba, grupo "Perfis" na tela de configurações com lista e formulário

**Status:** Aceito — aval visual pendente
**Data:** 2026-10-27
**Relacionados:** [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0021](0021-selecao-multipla-e-gestos-da-barra.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0048](0048-barra-de-status.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md), [ADR-0059](0059-janela-de-configuracoes.md), [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md), [ADR-0070](0070-perfis-de-aba.md), [ADR-0071](0071-backend-de-terminal.md), [ADR-0072](0072-cliente-ssh-embutido.md), [ADR-0073](0073-credenciais-no-cofre-do-sistema.md), [ADR-0074](0074-verificacao-da-chave-do-host.md), [ADR-0076](0076-reconexao-automatica.md), [PRD-007](../prd/prd-007-perfis-de-aba.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md), [PRD-020](../prd/prd-020-conexoes-ssh.md)
**Supersedes:** ADR-0014, "Anatomia — diálogo" (**parcial**: o diálogo ganha campo, caixa de marcar e linhas de dado em mono, para as perguntas do SSH; o corpo continua de uma linha) · ADR-0014, canal 1 (**parcial**: um consumidor novo, a falha ao gravar no cofre) · ADR-0060 (**parcial**: um grupo novo, "Perfis", com um padrão de lista e formulário que nenhum grupo tinha) · ADR-0037 §2 e §6 (**parcial**: painel SSH restaurado nasce `NotStarted` sempre e conecta por gesto, não por foco)

> **Aceito com o aval visual pendente**, como o [ADR-0068](0068-efeito-acrilico-no-fundo-do-terminal.md) e o [ADR-0070](0070-perfis-de-aba.md). O comportamento está decidido; a aparência dos três diálogos (§2 a §4) e do grupo de perfis (§6) passa pelo aval do dono do produto no PR que a implementa. **Nenhuma cor, dimensão ou raio novo** está previsto: cada elemento abaixo cita o token de onde sai.

> **Aval parcial dado (2026-10-27, prompt 16).** O dono aprovou o **diálogo de excluir perfil** (§6), na largura **760** que passou a valer para todo diálogo. O resto — os diálogos de host desconhecido e de segredo (§2 e §3), o grupo "Perfis" e a lista de importar (§6 e §7) — continua com o aval **pendente**.

## Contexto

O [PRD-020](../prd/prd-020-conexoes-ssh.md) pede quatro superfícies que o app não tem:

1. **Pedir um segredo** — senha ou passphrase — num campo mascarado, com a caixa "Lembrar no cofre do sistema" (RF-20.12, RF-20.14), que não copia e não se expõe à acessibilidade como texto (RF-20.35).
2. **Confirmar um host desconhecido**, mostrando o tipo da chave e a impressão digital, com o foco no cancelar (RF-20.16).
3. **Cadastrar perfis** — locais e SSH — na tela de configurações (RF-20.3), com exclusão que oferece esquecer o segredo (RF-20.4).
4. **Importar do `~/.ssh/config`** escolhendo quais blocos viram perfis (RF-20.5 a RF-20.7).

E mais duas que já existem e precisam de uma regra: o estado da conexão na aba, e a barra de status de uma aba SSH (RF-20.21).

O que existe e condiciona:

- O **diálogo de confirmação** ([ADR-0014](0014-superficie-de-aviso-e-dialogo.md), §2.15 da especificação) tem título, corpo **de uma linha** e botões. Não tem campo.
- O **campo de texto** do [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md) (`TextFieldState`) tem cursor, seleção e índices seguros em UTF-8 — e nada de máscara.
- A **tela de configurações** ([ADR-0060](0060-anatomia-da-tela-de-configuracoes.md)) tem nove grupos e controles de **uma chave por linha** (`OptionDef` → um `KeyPath`). Uma lista de perfis é uma lista de tabelas, e cada perfil tem um conjunto de campos que muda com o tipo.
- `porecatu-config::edit` ([ADR-0058](0058-escrita-do-arquivo-de-configuracao.md)) já navega por índice dentro de `ArrayOfTables`, mas não insere nem remove elemento.

## Decisão

**As perguntas do SSH usam o diálogo do ADR-0014 com três acréscimos — campo (mascarável), caixa de marcar e linhas de dado em mono —, abertos só quando a aba que pergunta está em foco. O cadastro é um grupo novo "Perfis" na tela de configurações, com uma lista à esquerda do painel de opções e o formulário do perfil escolhido, montado com os controles que já existem. Nenhum widget novo de chrome.**

### 1. Campo mascarado

Um tipo **à parte**, `MaskedFieldState`, e não uma flag em `TextFieldState`: este deriva `Debug` e `Clone` e guarda o texto num `String` comum, e qualquer *dump* de estado ou cópia acidental levaria a senha junto. O tipo novo reaproveita a lógica de cursor e seleção do [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md) por composição, sobre um buffer próprio:

- buffer `Zeroizing<String>` com **capacidade reservada de antemão** (1 KiB) e teto de comprimento nela, para que inserir nunca realoque e deixe uma cópia antiga não zerada na memória; zerado no `Drop`;
- **sem `Clone`**, e `Debug` opaco (`MaskedFieldState(..)`);
- cada escalar é desenhado como `•` (U+2022, da própria Iosevka Fixed — texto, não ícone), com o mesmo avanço de célula; cursor e seleção andam sobre os pontos;
- **copiar e recortar não fazem nada**; **colar funciona** — colar do gerenciador de senhas é o uso normal;
- o texto sai do campo **por movimento** para o `Secret` do [ADR-0073](0073-credenciais-no-cofre-do-sistema.md) §5, e o campo fica vazio;
- na árvore de acessibilidade ([ADR-0043](0043-arvore-de-acessibilidade.md)) o nó é um campo de **senha** (`Role::PasswordInput` no `accesskit`), sem valor exposto (RF-20.35).

Não há botão de "mostrar senha": seria um ícone novo e um terceiro estado do campo, sem requisito pedindo.

### 2. Diálogo de segredo

Sobre a anatomia da §2.15 — overlay, largura 380, título, botões —, na ordem:

> **Revisto pelo dono do produto (prompt 16, verificação ao vivo).** A largura de **todo** diálogo passou de 380 para **760** (`appearance.dialog.width`): o corpo do diálogo de excluir perfil saía cortado com reticências. Onde este ADR diz 380, leia 760; o resto da anatomia não muda.
>
> **Nova revisão (largura 500).** O dono reduziu depois o default de `appearance.dialog.width` de 760 para **500**; a largura é piso e o diálogo cresce quando o conteúdo exige. Onde este ADR diz 760, leia 500.
>
> **Nova revisão (largura 360).** O default caiu depois para **360**; onde este ADR diz 760 ou 500, leia 360.
>
> **Revisão de texto longo ([ADR-0078](0078-largura-e-quebra-de-texto-do-dialogo.md)).** A largura 360 é piso e o diálogo cresce até 720; o texto que não cabe **quebra**, nunca é cortado. Onde este ADR diz "corpo de uma linha", "truncado no meio" (caminho da chave) ou "cortada ao conteúdo" (linhas de aviso), leia: quebra em linhas, o caminho de chave por caractere.

| Elemento | Origem |
|---|---|
| Título: "Senha para `deploy@10.4.1.20`" / "Passphrase da chave" | título da §2.15 |
| Corpo (uma linha): o destino ou o caminho da chave, truncado no meio | corpo da §2.15 |
| Campo mascarado, largura cheia, foco automático | campo de texto da tela de configurações (§2.12) |
| Caixa "Lembrar no cofre do sistema", desmarcada | o alternador (`Toggle`) da tela de configurações; **desabilitado**, com a nota esmaecida da §2.12, quando `vault_available()` é falso (RF-20.14) |
| Botões **Cancelar** e **Conectar** | botões da §2.15; Conectar é o primário do rodapé da tela de configurações, não o destrutivo |

`Enter` no campo confirma; `Esc` cancela. Aqui o **foco inicial é o campo**, não o cancelar: o diálogo não é destrutivo, e o que o usuário veio fazer é digitar.

**Todo texto do diálogo é do app.** Quando a senha é pedida por keyboard-interactive (RF-20.10), o texto do campo que o servidor manda ("Password:", ou qualquer outra coisa) **não é exibido**: o título e o corpo são os mesmos do pedido de senha comum. O servidor não escreve no diálogo, e é isso que faz o diálogo valer mais que uma nota.

Segredo recusado reabre o mesmo diálogo com uma linha de erro no lugar do corpo, na cor de erro da barra de severidade do aviso (§2.14) — a tentativa anterior não é mencionada além disso, e o campo volta vazio.

> **Como ficou (prompt 13).** O diálogo de segredo é um `ConfirmDialog` com `secret: Option<Box<SecretFacts>>`, dado puro: rótulo e nota da caixa, se há cofre, a caixa ligada ou não e **onde está o foco** (campo, caixa ou botões). O **texto digitado** é o único pedaço que não cabe ali (`ConfirmDialog` é `Clone`) e mora num `MaskedFieldState` em `WindowState::secret_field`, que cai e é zerado com o diálogo. O foco anda campo → caixa (só com cofre) → Cancelar → Conectar; `Enter` no campo ou na caixa conecta, e **nunca com o campo vazio**; Espaço na caixa alterna, e no campo é parte da senha; o anel de foco dos botões só aparece com o foco neles. O corpo é o destino (`usuário@host`) ou o caminho da chave, cortado **no meio**; recusado, vira a linha de erro na cor da §2.14. A pergunta de segredo e a de host usam a mesma fila de pendentes (campainha, abre ao chegar ao painel). O campo é desenhado em Iosevka Fixed (pontos com o mesmo avanço de célula), e o clique nele põe o cursor antes do ponto sob o mouse; arrastar para selecionar não foi feito (`Shift+setas` e `Ctrl+A` valem).

### 3. Diálogo de host desconhecido

| Elemento | Origem |
|---|---|
| Título: "Host desconhecido: `10.4.1.20`" | título da §2.15 |
| Corpo (uma linha): "A autenticidade deste host não pode ser confirmada." | corpo da §2.15 |
| **Linhas de dado**: tipo da chave; impressão digital `SHA256:…` | **novo arranjo, não token novo**: mono no tamanho do corpo, cor "Secundário" `#a8b0bb` (§1.4), uma linha cada, sem truncar — a impressão digital SHA-256 tem 50 caracteres e cabe nos 380px menos o `padding` |
| Linha extra, quando o host tem chave de outro tipo registrada ([ADR-0074](0074-verificacao-da-chave-do-host.md) §3), e outra quando linhas do `known_hosts` foram puladas por ilegíveis (ADR-0074 §2) | corpo da §2.15, na cor de aviso da §2.14 |
| Botões **Cancelar** (foco inicial) e **Confiar e conectar** | botões da §2.15 |

O foco no cancelar é o do ADR-0014, pelo mesmo motivo: um `Enter` distraído aqui confia num host.

O texto das linhas de dado não passa pelo catálogo de idiomas — é dado, como a impressão digital no `ssh`. Os rótulos ("Tipo", "Impressão digital") passam.

### 4. Quando o diálogo aparece: com a aba em foco

A pergunta chega por `TermEvent::Prompt` ([ADR-0071](0071-backend-de-terminal.md) §5) a qualquer momento — inclusive com a aba em segundo plano, numa reconexão. O diálogo **não rouba o foco**:

- aba **ativa, painel focado, janela em foco** → o diálogo abre;
- caso contrário → a pergunta fica **pendente no painel**, que acende o **indicador de campainha** (§2.17, RF-1.21) — o sinal que já significa "esta aba quer sua atenção" —, e o diálogo abre quando o usuário chega ao painel;
- duas perguntas pendentes em painéis diferentes abrem uma de cada vez, conforme o usuário visita.

Fechar a aba ou o painel com pergunta pendente responde cancelar (ADR-0071 §5), sem diálogo de confirmação a mais além do RF-20.25.

### 5. Estado da conexão: notas no grid, nenhum estado visual novo de aba

Conectando, autenticando, reconectando, falhou — tudo vira **nota no grid** (canal 2 do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md)), escrita abaixo do conteúdo, como a nota de processo encerrado. A aba **não** ganha cor, ícone nem animação por estado de conexão: um quarto estado de aba pediria cor nova (§2.5, aba `Exited`), e a nota já diz o que acontece onde o usuário está olhando.

- **Restauração (RF-20.33).** Todo painel SSH restaurado — no arranque ou de uma sessão nomeada — nasce `NotStarted` ([ADR-0037](0037-aba-nao-iniciada.md)), **mesmo com `lazy_restore = false` e mesmo sendo o painel ativo**, com o rótulo esmaecido e a nota "`Enter` para conectar a `prod-web-01`". Ele conecta no primeiro **gesto sobre ele** — clique dentro do painel ou `Enter` com ele focado —, e **não** por ganhar foco: `Ctrl+Tab`, `tab.goto_N` e a escolha da aba ativa na restauração só o mostram. É uma superfície de teclado e mouse, não ação de catálogo. Painéis locais seguem o ADR-0037 sem mudança.
- **Limitação registrada: nota não é autenticada.** A nota do app é escrita no mesmo grid que os bytes do servidor, com a cor de destaque das notas; um servidor hostil pode imprimir texto na mesma cor imitando uma nota. Por isso **toda decisão de segurança passa por diálogo**, que o servidor não alcança — confiar num host, digitar um segredo —, e nenhuma nota pede ao usuário uma ação que mude a confiança; a nota de chave alterada **informa** o arquivo e a linha, e é o OpenSSH, não o Porecatu, quem diz como resolver.
- O único consumidor novo do **canal 1** (aviso do app) é a falha ao gravar no cofre ([ADR-0073](0073-credenciais-no-cofre-do-sistema.md) §3): a conexão já está aberta e a nota no grid se perderia na saída do shell.

### 6. Grupo "Perfis" na tela de configurações

Um décimo grupo na guia lateral, **depois de "Shell"**, cujo assunto ele estende. O painel de opções do grupo tem duas partes:

```
┌ guia ──────┐┌ painel de opções ───────────────────────────────────────────┐
│ Geral      ││  Perfil padrão         [ Shell do sistema       ▾ ]          │
│ Shell      ││  Detectar shells       [■]                                   │
│ Perfis   ◀ ││  ─────────────────────────────────────────────────────────── │
│ Terminal   ││  [+ Perfil local] [+ Perfil SSH] [Importar do ~/.ssh/config] │
│ …          ││   WSL  Ubuntu 24.04                                     ✕    │
│            ││ ▸ SSH  prod-web-01                                      ✕    │
│            ││   SSH  homolog                                          ✕    │
│            ││  ─────────────────────────────────────────────────────────── │
│            ││  Nome            [ prod-web-01            ]                  │
│            ││  Badge           [ SSH ]                                     │
│            ││  Host            [ 10.4.1.20              ]                  │
│            ││  Porta           [    22 ]                                   │
│            ││  Usuário         [ deploy                 ]                  │
│            ││  Autenticação    [ Agente, chave, senha  ▾ ]                 │
│            ││  Arquivo da chave[ ~/.ssh/id_ed25519      ]                  │
│            ││  Senha salva     [ Esquecer ]                                │
└────────────┘└─────────────────────────────────────────────────────────────┘
```

> **Revisto pelo [ADR-0080](0080-editor-de-perfil-em-janela-propria.md) — decidido, a implementar.** O formulário sai do painel: abre numa **janela própria**, filha e modal da tela de configurações, com o rótulo **acima** de cada campo. A linha da lista ganha um **lápis** à esquerda do ✕; lápis, duplo clique e `Enter` abrem o editor, e o clique simples só escolhe. "+ Perfil local" e "+ Perfil SSH" abrem o editor com um perfil novo. O OK do editor leva o perfil às alterações pendentes (sem gravar) e **não aceita** perfil inválido; a regra de que perfil inválido não impede salvar os outros fica só para o que vem inválido do arquivo. A importação continua no painel, abaixo da lista. O resto desta seção continua.

- **No topo**, as duas chaves de `[profiles]` (`default`, `detect`) como linhas comuns — um `Choice` cujas opções são o `[shell]` e os identificadores declarados, e um `Toggle`.
- **A lista**: uma linha por perfil de `[[profiles.list]]`, na ordem do arquivo — badge (a anatomia do [ADR-0070](0070-perfis-de-aba.md) §7, na cor neutra do grupo implícito, `#7b838f`, porque aqui não há grupo), nome, e o `✕` de excluir com a anatomia da linha do popover de sessões. Clique escolhe; setas navegam. **Sem arrastar para reordenar** nesta leva: a ordem é a do arquivo e decide `profile.new_tab_N`, e mudá-la de lugar sem querer muda atalhos.
- **O formulário** do perfil escolhido, abaixo da lista, com os controles que a tela já tem: `Text` (identificador, nome, badge, host, usuário, programa, arquivo da chave, diretório), `Number` (porta), `Choice` (tipo — só na criação —, autenticação), `StringList` (argumentos), `StringMap` (ambiente). O tipo decide os campos: perfil local não mostra host; perfil SSH não mostra programa.
- **"Senha salva — Esquecer"** e **"Passphrase salva — Esquecer"** só aparecem em perfil SSH com cofre disponível, e chamam `forget_*` ([ADR-0073](0073-credenciais-no-cofre-do-sistema.md) §4). O app não sabe se há senha salva sem ler o cofre, e não lê: o botão sempre aparece, e esquecer o que não existe não é erro.
- **Excluir** abre o diálogo de confirmação com, quando o perfil é SSH e nenhum outro perfil aponta para o mesmo destino, a caixa "Apagar também a senha e a passphrase salvas", **marcada** (RF-20.4).
- Validação do [ADR-0070](0070-perfis-de-aba.md) §1 e do [ADR-0072](0072-cliente-ssh-embutido.md) §6 aparece **no campo**, com a nota de erro de linha que a tela já usa; perfil inválido não impede salvar os outros.

Tudo isso entra nas **alterações pendentes** da tela ([PRD-016](../prd/prd-016-tela-de-configuracoes.md)) e é gravado pelo Salvar, como qualquer opção. Para isso, `porecatu-config::edit` ganha três operações sobre `ArrayOfTables` — acrescentar elemento, editar chave de um elemento **identificado pelo `id`** (não pelo índice, que muda quando outro perfil é removido), remover elemento —, preservando comentários e formatação como as edições de chave já preservam.

### 7. Importar do `~/.ssh/config`

O botão abre, no lugar do formulário, uma lista de marcar:

- leitura pelo crate `ssh2-config` 0.9.0 (MIT), em `porecatu-config` — que já é quem lê arquivo de configuração e não tem GUI nem PTY. Ele resolve `Include` e `~`; **`Match` não é suportado** por ele. A regra é uma só: o que estiver sob `Match` **nunca aparece na lista**. Se o leitor tolera o bloco, ele é ignorado; se o leitor recusa o arquivo inteiro por causa dele, vale o caso de arquivo recusado abaixo. Qual dos dois o `ssh2-config` faz é conferido com um arquivo de prova no PR que o adiciona;
- uma linha por bloco `Host` concreto (RF-20.5): nome, `usuário@host:porta` resolvidos; já importado aparece marcado e desabilitado (RF-20.7); bloco com diretiva não suportada aparece desabilitado com o motivo (RF-20.6);
- **Importar** acrescenta um perfil SSH por linha marcada às alterações pendentes, com `id` derivado do nome do bloco pela regra do [ADR-0070](0070-perfis-de-aba.md) §1 (minúsculas, fora de `[a-z0-9_-]` vira `-`, sufixo `-2` em colisão) e `user` local quando o bloco não tem `User` ([ADR-0072](0072-cliente-ssh-embutido.md) §6);
- arquivo ausente: a lista diz que não há `~/.ssh/config`. Arquivo que o leitor recusa: `porecatu-config` devolve um **erro tipado** (`SshConfigImportError`, com a linha quando o leitor a informa), a frase sai do catálogo em `porecatu-ui` ([ADR-0056](0056-catalogo-de-textos-da-interface.md)) — a mensagem em inglês do crate nunca chega à tela —, e nada é importado.

> **Correção de fato (prompt 16).** A prova com `ssh2-config` 0.9.0 mostrou que, no modo padrão (e com `ALLOW_UNSUPPORTED_FIELDS`), o leitor **recusa o arquivo inteiro** em `Match` com `unknown field: Match` e sem número de linha — é o caso de arquivo recusado acima. Com `ALLOW_UNKNOWN_FIELDS` ele **não ignora** o bloco: engole a linha `Match` e **funde as diretivas de baixo no bloco `Host` de cima** (um `HostName` sob `Match` trocou o do `Host` anterior), o que quebraria a regra de que o que está sob `Match` nunca aparece. Por isso `porecatu-config` lê **sem** `ALLOW_UNKNOWN_FIELDS`: `Match`, e também qualquer diretiva que o leitor não conhece, recusa o arquivo. A linha do erro vem de uma busca da diretiva no texto do arquivo principal (nada em arquivo incluído). `ProxyCommand`, `LocalForward` e `DynamicForward` chegam em `unsupported_fields` (minúsculas); `ProxyJump`, `RemoteForward` e `CertificateFile` são campos próprios. Um `Include` escrito **dentro** de um bloco `Host` vale só para aquele bloco, como no OpenSSH.

A licença do `ssh2-config` é conferida com a árvore dele no PR que o adiciona ([ADR-0010](0010-licenciamento.md)).

### 8. Barra de status e menu da aba

- O segmento de shell mostra `usuário@host` numa aba SSH; o de diretório, o caminho remoto do OSC 7 quando o host do OSC 7 bate com o do perfil, e nada caso contrário; os de Git não aparecem (RF-20.21). Nenhum segmento novo, nenhum token novo.
> **Como ficou (prompt 12).** O OSC 7 chega com o host (`TermEvent::Cwd { path, host }`; até aqui o host era descartado). Num painel SSH o caminho **não** entra em `Pane::cwd`: fica em `PaneRuntime::remote_cwd`, e só se o host da URI for **igual, sem distinguir caixa,** ao `host` do perfil — o que impede, por construção, que o `cwd` herdado por `tab.new`, o fallback de `cwd` e a sessão vejam um caminho do servidor (RF-20.22, RF-20.32). A comparação é estrita: um shell que diz `file:///dir` (sem host) ou o nome da máquina quando o perfil usa o IP **não** mostra diretório — a barra fica só com `usuário@host`. O rótulo da aba sem título é o **nome do perfil** (RF-20.20), não o host.
- O menu de contexto da aba ganha **"Reconectar"** (`ssh.reconnect`, [ADR-0076](0076-reconexao-automatica.md)) numa aba SSH; em aba que não é SSH o item **não aparece** — exceção deliberada à regra do item esmaecido do ADR-0014, porque o item não está indisponível nessa aba, ele não tem sentido nela.

## Alternativas consideradas

### Um widget novo de "prompt de credencial"

Mais livre para desenhar. Recusada: o diálogo já tem overlay, foco, `Enter`/`Esc`, camada e acessibilidade; o que falta são três linhas de conteúdo, todas com token existente.

### Pedir o segredo dentro do grid, como o `ssh` do sistema

Zero UI nova: a nota pede a senha e o teclado vai para a conexão. Recusada: não há como ter a caixa "Lembrar" nem garantir que o texto não ecoe, e o usuário não tem como distinguir um pedido do app de um prompt do servidor imitando um — o que é exatamente o golpe que um diálogo do app evita.

### Diálogo que rouba o foco

Abrir na hora, mesmo com a aba em segundo plano. Recusada: uma reconexão de madrugada abriria um diálogo de senha sobre o que o usuário estiver digitando, e o próximo `Enter` dele iria para o lugar errado.

### Perfis editados num diálogo à parte, fora da tela de configurações

Evitaria o padrão de lista e formulário. Recusada pelo PRD-020 (RF-20.3) e pela regra do [PRD-016](../prd/prd-016-tela-de-configuracoes.md) de que a tela edita o arquivo inteiro num lugar só.

> **Nota ([ADR-0080](0080-editor-de-perfil-em-janela-propria.md)).** O editor de perfil em janela própria **não** reabre esta alternativa: ele só abre a partir da tela de configurações, morre com ela, e o que produz é pendência da tela, gravada pelo mesmo Salvar.

### Estado da conexão como cor ou ícone na aba

Visível sem abrir a aba. Recusada: cor e ícone novos, e a nota no grid já cobre o caso em que o usuário está olhando; para a aba de fundo, o indicador de campainha cobre o que exige ação.

## Consequências

### Positivas

- Zero widget novo; três acréscimos ao diálogo, todos de tokens existentes.
- Pergunta nenhuma interrompe o que o usuário está fazendo em outra aba.
- O cadastro de perfis é o arquivo, editado pela mesma tela e pelo mesmo Salvar de tudo o mais.

### Negativas

- O diálogo deixa de ser "título, uma linha e botões"; a §2.15 passa a descrever variantes.
- O grupo "Perfis" é o primeiro com lista e formulário, um padrão que os outros nove grupos não têm.
- `porecatu-config::edit` ganha operações estruturais sobre arrays, mais delicadas que editar uma chave.
- Mais um crate (`ssh2-config`) em `porecatu-config`.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Segredo copiado do campo mascarado, exposto à acessibilidade, impresso por `Debug` ou deixado em memória realocada | Baixa | **Alto** | §1: tipo próprio sem `Clone` e com `Debug` opaco, capacidade reservada, zerado no `Drop`; testes de que copiar/recortar não alcançam o clipboard, de que o nó não tem valor e de que `Debug` não contém o texto |
| Servidor hostil imitar uma nota do app | Média | Médio | §5: decisões de segurança só por diálogo; nota nunca pede ação que mude confiança |
| Pergunta pendente esquecida numa aba de fundo, e a conexão expirar | Média | Baixo | Campainha acesa; o servidor encerra pelo próprio tempo e a nota diz isso; `ssh.reconnect` recomeça |
| Edição por `id` atingir o perfil errado depois de exclusão | Baixa | Médio | §6: endereçamento por `id`; teste que remove o primeiro perfil e edita o terceiro |
| `ssh2-config` recusar um `~/.ssh/config` com `Match` | Média | Baixo | §7: erro mostrado, nada importado; importar é opcional |
| Aparência dos diálogos entrar sem aval | Baixa | Médio | Status com aval pendente; aval no PR que implementa |
