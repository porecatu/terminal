# PRD-007 — Perfis de aba

**Status:** Aprovado
**Data:** 2026-08-26 (rascunho) · 2026-10-27 (aprovado)
**Requisito de origem:** pedido direto do dono do produto, de **abrir sessões SSH** pelo app — que passa por perfis porque um destino SSH é, para a barra de abas, mais um tipo de terminal
**Relacionados:** [ADR-0070](../adr/0070-perfis-de-aba.md), [ADR-0004](../adr/0004-pty-cross-platform.md), [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0032](../adr/0032-interface-do-v1-fechada.md), [ADR-0036](../adr/0036-formato-do-arquivo-de-sessao.md), [ADR-0053](../adr/0053-paineis-divididos.md), [ADR-0067](../adr/0067-temas-em-arquivos-proprios.md), [PRD-001](prd-001-abas.md), [PRD-002](prd-002-grupos-de-abas.md), [PRD-003](prd-003-persistencia-de-sessao.md), [PRD-004](prd-004-aparencia-do-chrome.md), [PRD-006](prd-006-paineis-divididos.md), [PRD-012](prd-012-comando-de-projeto-por-diretorio.md)

> Aprovado em 2026-10-27, por decisão do dono do produto, **fora da ordem de fases** — como os painéis divididos ([PRD-006](prd-006-paineis-divididos.md)), que também eram rascunho `[v2]` promovido. O gatilho foi o pedido de **sessões SSH**: o dono decidiu que o SSH entra como **um tipo de perfil**, e por isso este documento é promovido inteiro, não só a parte que o SSH usa.
>
> **O que mudou em relação ao rascunho.** O rascunho deixava "gerenciar conexões SSH (host, chave, porta) dentro do app" fora de escopo e inclinava a questão das credenciais para o `ssh_config`. O dono decidiu o contrário: o cliente SSH é **embutido**, com cadastro de hosts no app. Este documento define só o que é **comum a todo perfil** e reserva o tipo `ssh`; conexão, autenticação, chave de host e reconexão são do PRD de conexões SSH, que vem a seguir. As quatro questões abertas do rascunho estão respondidas em "Decisões do dono", e a tela de nova aba, desenhada no canvas, ficou fora (RF-7.20).

## Problema

No v1, uma aba nova abre o shell de `[shell]` no diretório da aba ativa. Isso cobre o caso comum e nada além dele.

Quem trabalha em Windows raramente quer só PowerShell: quer PowerShell, WSL, `cmd` para uma ferramenta legada, e um `ssh` para o servidor de produção. Hoje, cada um desses exige abrir uma aba e digitar o comando à mão, toda vez — ou trocar o `[shell] program` e reabrir, o que muda o shell de **todas** as abas novas.

Perfis transformam isso em uma escolha. E, por tabela, resolvem um segundo problema: **identificar visualmente o tipo de terminal na barra**, que é justamente o que o badge do design faz.

## Usuário-alvo

O mesmo do [PRD-000](prd-000-visao-de-produto.md), no caso em que ele usa **mais de um tipo de shell** — Windows com WSL é o caso dominante, e quem acessa servidores por SSH é o caso que motivou a promoção.

**Não é para** quem usa um shell só: para esse, nada muda. Sem perfil declarado, o `+`, os atalhos e a barra se comportam exatamente como hoje, e nenhum badge aparece (RF-7.13).

## O que o design mostra

Ver [especificação visual](../design/especificacao-visual.md), seções 2.5, 2.9 e 2.13.

Seis perfis de exemplo:

| Badge | Nome | Comando | Cor | Sistema |
|---|---|---|---|---|
| `PS` | Windows PowerShell | `pwsh.exe -NoLogo` | `#6fa8f5` | Windows |
| `CMD` | Prompt de Comando | `cmd.exe /k` | `#8b929e` | Windows |
| `WSL` | Ubuntu 24.04 | `wsl.exe -d Ubuntu` | `#e0b060` | Linux |
| `SSH` | prod-web-01 | `ssh deploy@10.4.1.20` | `#ef8a8a` | Remoto |
| `ZSH` | macOS zsh | `/bin/zsh -l` | `#a68cf0` | macOS |
| `PY` | Python 3.12 | `python -i` | `#5ed3bc` | Multi |

Três superfícies:

- **Badge na aba** — mono 9px, na cor do grupo (não do perfil), fundo tingido `.14`
- **Menu de perfis** — popover ao clicar no `+`, com nome, badge e tecla; ao final, "Novo grupo de abas"
- **Tela de nova aba** — grade de cards quando não há aba aberta

Note a decisão de cor: no design, o badge usa a cor **do grupo**, não a do perfil. A cor do perfil aparece só no menu e nas configurações. Mantém a barra legível por grupo, que é o eixo principal de organização.

**O que este documento faz com o desenho** — o canvas é norte, não contrato ([ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md)):

- O **badge** entra como desenhado, na cor do grupo.
- O **menu** entra, mas aberto pelo **clique direito** no `+` (RF-7.8): o clique esquerdo continua criando a aba sem perguntar.
- A **cor do perfil não entra.** Desde o [ADR-0067](../adr/0067-temas-em-arquivos-proprios.md) cor só existe no tema, e uma cor por perfil no `porecatu.toml` seria a primeira exceção. Como o badge já usa a cor do grupo, a do perfil só apareceria no menu — e ali ela não carrega informação que o badge e o nome não carreguem.
- A **tela de nova aba não entra** (RF-7.20).

## Decisões do dono

As quatro questões abertas do rascunho, respondidas em 2026-10-27:

| Questão | Decisão |
|---|---|
| Perfil determina também tema de cores da aba? | **Não.** Tema é estado do processo — o mesmo argumento do RF-14.13 —, e tema por aba exigiria paleta por quadro no render. |
| Perfil de `ssh` guarda credenciais? | **Sim, no cofre do sistema operacional, nunca em arquivo do app** — decidido no PRD de conexões SSH. Aqui só se fixa que nenhum campo de perfil carrega segredo (RF-7.3). |
| Perfil pode fixar um grupo de destino? | **Não.** A aba nasce onde nasceria hoje (RF-7.10). |
| Detecção automática roda no start ou sob demanda? | **Sob demanda**, na primeira abertura do menu, em thread, com o resultado guardado até o fim do processo (RF-7.17). |

## Requisitos funcionais

### Definir

**RF-7.1** — Perfis são declarados no arquivo de config, numa lista. Cada perfil tem: um **identificador** estável, um **nome** exibido, um **badge** curto, e o que lançar — **programa, argumentos, ambiente** e, opcionalmente, um **diretório inicial**. A forma exata das chaves é do [ADR-0070](../adr/0070-perfis-de-aba.md).

**RF-7.2** — O identificador é o que a sessão grava e o que liga o perfil a um atalho por posição; o nome é o que o usuário lê. Renomear um perfil (mudar o nome) não perde as abas restauradas dele; mudar o identificador perde (RF-7.16).

**RF-7.3** — Nenhum campo de perfil é desenhado para segredo. Senha, passphrase e chave privada de um perfil SSH vivem onde o PRD de conexões SSH decidir — nunca no `porecatu.toml`, no `session.json` ou num arquivo de sessão nomeada. O ambiente de um perfil é texto do `porecatu.toml`, como o `[shell.env]` de hoje, e não é lugar de segredo; a sessão não o grava.

**RF-7.4** — Um perfil tem um **tipo**: `local` (o default — um programa lançado num pseudo-terminal, como toda aba de hoje) ou `ssh` (uma conexão do cliente embutido, definida no [PRD-020](prd-020-conexoes-ssh.md)). Este documento descreve o comportamento comum aos dois; onde o tipo `ssh` diverge, quem decide é aquele PRD.

**RF-7.5** — Perfil inválido — identificador repetido, identificador vazio, programa vazio num perfil `local`, badge vazio — é **erro de config** com a linha, pelo caminho de erro que a config já tem; os perfis válidos continuam valendo. A regra do RF-4.22 vale igual: chave desconhecida dentro de um perfil é aviso, não erro.

**RF-7.6** — Mudanças na lista de perfis valem **sem reiniciar**, na próxima vez que um perfil é escolhido. Abas já abertas não mudam: o perfil é lido no spawn.

### Perfil padrão

**RF-7.7** — Existe sempre um perfil padrão, que é o que o `+`, `tab.new`, `group.new_tab` e `window.new` usam (dividir um painel não usa o padrão, RF-7.15):
- sem nada declarado, o perfil padrão é o próprio `[shell]` — exatamente o comportamento de hoje, incluindo a detecção do shell por plataforma quando `program` está vazio;
- a config pode apontar um dos perfis declarados como padrão; apontar um identificador que não existe é erro de config, e o padrão volta a ser o `[shell]`.

### Escolher

**RF-7.8** — **Clique direito** num botão `+` abre o **menu de perfis** ancorado naquele botão. O **clique esquerdo** continua fazendo o que faz hoje — `group.new_tab` naquele grupo, com o perfil padrão. O caso comum não ganha nenhum gesto.

**RF-7.9** — O menu lista, nesta ordem: o perfil padrão; os perfis declarados, na ordem do arquivo, sem repetir o padrão; o shell de `[shell]`, quando ele não é o padrão; um divisor e os perfis **detectados** (RF-7.17) que não repetem uma linha acima; um divisor e o item **"Novo grupo de abas"**, que executa `group.create`. Cada linha mostra o badge, o nome e, quando o perfil tem atalho vinculado (RF-7.11), a tecla. A linha do `[shell]` mostra o nome do shell, como a aba mostra hoje, e não tem badge (RF-7.13).

**RF-7.10** — Escolher um perfil no menu cria a aba **no grupo do `+` clicado**, como o clique esquerdo; pela ação `profile.open_menu` (RF-7.12), no grupo da aba ativa. O `cwd` é herdado como hoje (`tab.new`), a não ser que o perfil declare diretório inicial próprio, que então vence. O perfil não escolhe grupo.

**RF-7.11** — Nove ações fechadas, `profile.new_tab_1` a `profile.new_tab_9`, criam uma aba com o N-ésimo perfil **declarado**, no grupo da aba ativa. Nascem **sem default**: `Alt+<número>` (`Cmd+<número>` no macOS) já é `tab.goto_N`, e as demais combinações dependem do layout de teclado — a escolha é do usuário, em `[keybindings]` ([ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md)). Um índice sem perfil declarado não faz nada, sem aviso — como `tab.goto_N` sem a N-ésima aba.

**RF-7.12** — A ação `profile.open_menu` abre o mesmo menu pelo teclado, ancorado no `+` do grupo da aba ativa — ou, com `show_new_tab_button = false`, abaixo da aba ativa —, com a primeira linha realçada. Setas navegam, `Enter` escolhe, `Esc` fecha. **Sem default de tecla**, precedente de `session.open_list` e `pane.close`.

### Mostrar

**RF-7.13** — A aba mostra o **badge** do perfil antes do rótulo, na cor do grupo com fundo tingido (RF-4.24), **só quando o perfil dela não é o padrão** — e nunca para o `[shell]`. Uma barra só de abas do perfil padrão fica exatamente como é hoje, e quem nunca declarar um perfil nunca vê um badge.

**RF-7.14** — `show_profile_badge = false` esconde o badge em todas as abas. A largura da aba não muda com ou sem badge: o badge ocupa espaço do rótulo, que trunca antes ([ADR-0070](../adr/0070-perfis-de-aba.md)).

### Painéis e sessão

**RF-7.15** — O perfil é **do painel**, como o shell já é ([ADR-0053](../adr/0053-paineis-divididos.md)). Dividir um painel abre o painel novo **com o perfil do painel de origem** — inclusive quando ele é o `[shell]`, qualquer que seja o padrão naquele momento. A aba deriva o badge do painel focado.

**RF-7.16** — A sessão grava o identificador do perfil de cada painel, e a restauração **lança o perfil gravado** — hoje a restauração grava o nome do shell mas sempre lança o `[shell]` atual. Identificador que não existe mais na config cai no perfil padrão, com uma **nota no grid** dizendo qual perfil sumiu, como o `cwd` que sumiu (RF-3.10).

### Detectar

**RF-7.17** — A primeira abertura do menu num processo dispara, **em thread**, a detecção dos shells disponíveis na máquina:
- Windows: PowerShell 7, Windows PowerShell, `cmd`, e cada distribuição WSL instalada;
- Linux e macOS: os shells listados em `/etc/shells` que existem no disco.

O menu abre na hora com os perfis declarados; os detectados entram quando a thread termina, sem fechar nem mover o realce. O resultado vale até o fim do processo. **Nada é detectado no arranque**: o tempo até o primeiro prompt (métrica do [PRD-000](prd-000-visao-de-produto.md)) não muda.

**RF-7.18** — A detecção pode ser desligada na config. Detecção que falha ou passa de um teto de tempo **não vira aviso**: o menu fica só com os declarados, e o padrão continua lá.

**RF-7.19** — Um perfil detectado pode ser gravado na sessão e restaurado **sem rodar a detecção de novo**: o identificador dele descreve de forma determinística o que lançar ([ADR-0070](../adr/0070-perfis-de-aba.md) §5). Antes de lançar, a restauração confere que o identificador só pede um shell que a máquina oferece ou uma distribuição WSL pelo nome — nunca um executável arbitrário vindo do arquivo de sessão; o que não passa cai no padrão com a nota do RF-7.16.

### O que não entra

**RF-7.20** — A **tela de nova aba** (§2.13 da especificação visual) **não entra**. Ela só existe numa janela sem abas, e no Porecatu fechar a última aba fecha a janela (RF-1.4). Ter a tela pediria um estado novo de janela, que nenhum requisito pede.

## Cenários

```gherkin
Cenário: o clique comum não muda
  Dado perfis "WSL" e "CMD" declarados, e nenhum perfil padrão escolhido
  Quando o usuário clica com o botão esquerdo no + de um grupo
  Então uma aba nova abre com o shell de [shell], naquele grupo
  E nenhum menu aparece

Cenário: abrir uma aba WSL pelo menu
  Dado o perfil "WSL" declarado
  Quando o usuário clica com o botão direito no + do grupo "api"
  E escolhe "WSL"
  Então uma aba nova abre no grupo "api" rodando o programa do perfil "WSL"
  E a aba mostra o badge do perfil na cor do grupo "api"

Cenário: atalho por posição
  Dado "profile.new_tab_2" vinculado a uma tecla, e dois perfis declarados
  Quando o usuário tecla o atalho
  Então uma aba abre com o segundo perfil, no grupo da aba ativa

Cenário: dividir herda o perfil
  Dado uma aba com o perfil "WSL"
  Quando o usuário divide o painel
  Então o painel novo roda o programa do perfil "WSL"

Cenário: restaurar lança o perfil gravado
  Dado uma sessão gravada com uma aba no perfil "WSL"
  Quando o app abre
  Então a aba restaurada roda o perfil "WSL", não o [shell]

Cenário: perfil removido da config
  Dado uma sessão gravada com uma aba no perfil "py"
  E "py" removido da config
  Quando o app abre
  Então a aba roda o perfil padrão
  E mostra uma nota dizendo que o perfil "py" não existe mais

Cenário: detecção sob demanda
  Dado o Windows com a distribuição "Ubuntu" do WSL instalada e nenhum perfil declarado
  Quando o usuário abre o menu de perfis pela primeira vez
  Então o menu abre na hora com o perfil padrão
  E "Ubuntu" aparece entre os detectados quando a detecção termina

Cenário: sem perfis, nada muda na barra
  Dado nenhum perfil declarado
  Então nenhuma aba mostra badge
```

## Fora de escopo

Cada item é decisão, não esquecimento.

- **Tela de nova aba** (RF-7.20).
- **Cor por perfil.** Cor só no tema ([ADR-0067](../adr/0067-temas-em-arquivos-proprios.md)); o badge usa a do grupo.
- **Tema por perfil, grupo por perfil** — ver "Decisões do dono".
- **Ícone por perfil.** O badge já identifica, e a face de ícones é fechada no que o chrome usa ([ADR-0024](../adr/0024-face-de-icones.md)).
- **Trocar o perfil de uma aba aberta.** Abre-se outra aba com o perfil certo; trocar teria de matar o processo vivo.
- **Perfis de container com ciclo de vida** e **sincronizar perfis entre máquinas.**
- **Editar perfis na tela de configurações nesta leva.** A tela edita uma chave por linha, e uma lista de tabelas pede um controle novo; o controle é decidido junto com o cadastro de hosts SSH, que precisa dele de qualquer jeito. Até lá, perfis são editados no arquivo.

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Gestos a mais para abrir uma aba do perfil padrão | **zero** |
| Gestos para abrir uma aba de outro perfil | **dois** (clique direito, clique no perfil) ou **um** com atalho |
| Diferença visível na barra para quem não declarou perfil | **zero** |
| Tempo de arranque gasto em detecção | **zero** |
| Abas restauradas com um shell diferente do que tinham | **zero**, salvo perfil removido, que avisa por nota |
| Segredos em arquivo do app | **zero** |
