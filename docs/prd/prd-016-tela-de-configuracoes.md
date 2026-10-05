# PRD-016 — Tela de configurações

**Status:** Aprovado
**Data:** 2026-10-02
**Requisito de origem:** pedido direto do dono do produto — *"Hoje, este app possui um arquivo de configuração (porecatu.toml) que fica armazenado em alguns possíveis diretórios, mas toda alteração da configuração do app precisa ser feita manualmente, editando esse arquivo. Quero que essa nova tela atenda a esse requisito. Essa tela deve ter uma guia lateral (esquerda) com os grupos de configurações e o painel (central, tomando o restante da janela) com as opções propriamente ditas."*
**Relacionados:** [ADR-0058](../adr/0058-escrita-do-arquivo-de-configuracao.md), [ADR-0059](../adr/0059-janela-de-configuracoes.md), [ADR-0060](../adr/0060-anatomia-da-tela-de-configuracoes.md), [ADR-0003](../adr/0003-formato-de-configuracao.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0015](../adr/0015-multiplas-janelas.md), [ADR-0029](../adr/0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0030](../adr/0030-escopo-do-hot-reload.md), [ADR-0031](../adr/0031-temas-nomeados.md), [ADR-0043](../adr/0043-arvore-de-acessibilidade.md), [ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md), [PRD-004](prd-004-aparencia-do-chrome.md), [PRD-010](prd-010-interacao-e-superficie-de-app.md), [PRD-011](prd-011-polimento.md)

> Aprovado em 2026-10-02, por decisão do dono do produto, **fora da ordem de fases** — pelo caminho da barra de status ([PRD-009](prd-009-barra-de-status.md)) e dos painéis divididos ([PRD-006](prd-006-paineis-divididos.md)): é a promoção de um elemento que o canvas desenhou e a tabela de fases marcou `[v2]` — o "painel de configurações GUI" da §2.12 da [especificação visual](../design/especificacao-visual.md) —, mas sem rascunho a promover: nenhum PRD o descrevia, só o [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md) §6, que fixou a regra que este documento herda inteira — **a tela escreve no TOML, e o arquivo continua sendo a única fonte de verdade**. Como o arquivo é regravado sem perder nada do que o usuário escreveu está no [ADR-0058](../adr/0058-escrita-do-arquivo-de-configuracao.md); a janela que hospeda a tela, no [ADR-0059](../adr/0059-janela-de-configuracoes.md); o que aparece nela, no [ADR-0060](../adr/0060-anatomia-da-tela-de-configuracoes.md).

## Problema

Toda mudança de comportamento do Porecatu passa por um arquivo de ~1700 linhas. A engrenagem da barra de abas o abre no editor do sistema (RF-11.27), e a recarga a quente aplica o que for gravado ([ADR-0030](../adr/0030-escopo-do-hot-reload.md)) — então editar **funciona**, e é por isso que o arquivo é bom para quem versiona dotfiles. Mas tem três custos, e todos recaem sobre o uso comum, não sobre o avançado:

- **Descobrir o que existe.** Para saber que dá para desligar a confirmação ao fechar janela, ou que existe `copy_on_select`, é preciso ler o arquivo de exemplo de ponta a ponta, entre centenas de chaves de cor e espaçamento do chrome que o usuário comum nunca vai tocar.
- **Acertar a forma.** Um nome de chave errado vira aviso de chave desconhecida; um tipo errado (`"sim"` em vez de `true`) derruba a gravação inteira para os defaults anteriores. O app avisa — mas o usuário só fica sabendo depois de gravar, e na língua do TOML.
- **Atalhos.** Remapear uma tecla exige saber o nome da ação no catálogo, a gramática de tecla (`ctrl+shift+comma`), a tabela da plataforma certa e a convenção de `"none"` para desvincular. Ninguém faz isso sem abrir três documentos.

O que falta é uma **segunda porta** para o mesmo arquivo: uma tela que mostre as opções que importam no dia a dia, com nome legível, descrição e o controle certo para cada uma, e que grave no `porecatu.toml` o que o usuário escolheu — sem tirar dele a primeira porta.

## O que já existe e não muda

- **O arquivo é a configuração** ([ADR-0003](../adr/0003-formato-de-configuracao.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md) §6). A tela não guarda preferência em lugar nenhum além dele, não tem cache próprio e não sabe nada que o arquivo não diga. Fechar a tela e editar o arquivo à mão continua sendo um caminho completo.
- **A recarga a quente aplica.** A tela grava; quem aplica é o mesmo watcher de sempre, com as mesmas classes A, B e C do [ADR-0030](../adr/0030-escopo-do-hot-reload.md). Não existe caminho paralelo de aplicação.
- **O arquivo de exemplo, `config.reload`, `--config` e `PORECATU_CONFIG`** continuam como estão. A tela edita o arquivo que o app resolveu no arranque — o mesmo que a engrenagem abria.
- **Temas são do arquivo** ([ADR-0031](../adr/0031-temas-nomeados.md)). A tela escolhe qual tema vale; criar ou editar um tema continua sendo `[[themes]]` à mão. O ciclo por atalho (`theme.cycle`) continua de sessão e continua sem escrever no arquivo.
- **Nenhum diálogo nativo do sistema** ([ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md)) — nem seletor de arquivo, nem de pasta, nem de cor.

## Usuário-alvo

O do [PRD-000](prd-000-visao-de-produto.md), no momento em que ele quer mudar **uma** coisa — o idioma, a fonte, o tema, um atalho que colide com o programa que ele usa — e não quer aprender o formato do arquivo para isso.

**Não é para** substituir o arquivo para quem já o mantém versionado: para esse, a tela é uma conveniência que **nunca** reescreve o que ele escreveu fora das chaves que mexeu (RF-16.17).

## Em uma tela

Uma janela própria, aberta pela engrenagem da barra de abas:

```
┌──────────────────────────────────────────────────────────────────────┐
│ Configurações                                              ─  □  ✕   │
├────────────────┬─────────────────────────────────────────────────────┤
│  Geral         │  Terminal                                           │
│  Shell         │                                                     │
│ ▌Terminal   •  │  Família da fonte              [Iosevka Fixed     ] │
│  Aparência     │  Tamanho da fonte                      [ 14.0 ] •   │
│  Sessão        │  Forma do cursor          [ bloco | barra | sub ]   │
│  Projeto       │  Cursor pisca                              [ ○  ]   │
│  Git           │  Linhas de histórico   vale em aba nova  [ 10000 ]  │
│  Painéis       │  Copiar ao selecionar                      [ ○  ]   │
│  Atalhos       │  …                                                  │
│                ├─────────────────────────────────────────────────────┤
│                │  Abrir arquivo no editor     [Descartar]  [Salvar]  │
└────────────────┴─────────────────────────────────────────────────────┘
```

À esquerda, a guia com os grupos; à direita, ocupando o resto da janela, as opções do grupo escolhido. O ponto (`•`) marca o que foi alterado e ainda não gravado. **Salvar** grava no `porecatu.toml`; **Descartar** volta ao que o arquivo diz.

## Requisitos funcionais

### Abrir e fechar

**RF-16.1** — A tela de configurações vive numa **janela própria do sistema**, separada das janelas de terminal. Abre por dois caminhos: o clique no **botão de configurações** da zona fixa da barra de abas (a engrenagem, que deixa de abrir o arquivo — emenda ao RF-11.27) e a ação `settings.open` do catálogo, com default `Ctrl+Shift+O` no Windows e no Linux e `Cmd+,` no macOS ([ADR-0059](../adr/0059-janela-de-configuracoes.md) §6).

**RF-16.2** — Existe **no máximo uma** janela de configurações por processo. Abrir com ela já aberta a traz para a frente e lhe dá o foco, sem perder o grupo escolhido nem as alterações pendentes.

**RF-16.3** — A janela abre centrada sobre a janela de terminal de onde o gesto partiu, no monitor dela, com tamanho padrão e tamanho mínimo próprios ([ADR-0060](../adr/0060-anatomia-da-tela-de-configuracoes.md)). Redimensionável, maximizável; **não** entra no `session.json` nem é restaurada no arranque.

**RF-16.4** — Fecha pelo botão de fechar da janela, pelo gesto do sistema e por `Esc` (quando nenhum campo, lista ou captura de tecla está consumindo o `Esc`). Fechar com alterações pendentes abre o **diálogo de confirmação** ([ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md)) com três saídas — salvar e fechar, descartar e fechar, cancelar —, foco inicial no cancelar.

**RF-16.5** — Fechar a **última janela de terminal** encerra o app e fecha a de configurações junto. Se ela tiver alterações pendentes, o encerramento espera: a janela de configurações vem para a frente com o diálogo do RF-16.4, e cancelar nele cancela o encerramento. A janela de configurações sozinha não mantém o processo vivo.

**RF-16.6** — A janela segue o **tema** e o **idioma** correntes ao vivo, como as de terminal: a recarga que muda um muda o outro no próximo frame dela também.

### Guia lateral e painel

**RF-16.7** — À esquerda, uma **guia lateral** fixa lista os grupos, nesta ordem: **Geral, Shell, Terminal, Aparência, Sessão, Projeto, Git, Painéis, Atalhos**. Um grupo é escolhido por clique ou pelas setas com o foco na guia; o escolhido fica realçado. Grupo com alteração pendente leva o marcador de pendente ao lado do nome.

**RF-16.8** — À direita, ocupando o restante da janela, o **painel** mostra o título do grupo e, abaixo, as opções dele, uma por linha: nome, descrição curta e o controle. O painel **rola** quando as opções não cabem; a guia lateral e o rodapé (RF-16.14) não rolam.

**RF-16.9** — A janela abre no grupo **Geral** na primeira vez de cada execução do app; reabri-la na mesma execução volta ao último grupo escolhido, com a rolagem no topo.

**RF-16.10** — Teclado: `Tab`/`Shift+Tab` percorrem guia, opções do painel e botões do rodapé, nessa ordem, com anel de foco visível; setas movem dentro da guia e entre opções de escolha; `Espaço` alterna; `Enter` confirma campo; `Ctrl+S` (`Cmd+S` no macOS) salva. Nada disso é ação do catálogo — vale só dentro da janela de configurações, como as teclas do diálogo e do editor de grupo.

### O que a tela mostra

**RF-16.11** — A tela mostra um **catálogo curado** de opções, não o arquivo inteiro. Ele é este, por grupo (chave do arquivo entre parênteses; o rótulo de interface sai do catálogo de textos, [ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md)):

| Grupo | Opções |
|---|---|
| **Geral** | idioma (`general.language`, escolha entre os arquivos de idioma encontrados); diretório inicial (`general.startup_directory`, "pasta pessoal" ou caminho); confirmar ao fechar aba com processo (`general.confirm_close_with_process`); confirmar ao fechar janela (`general.confirm_close_window`) |
| **Shell** | programa (`shell.program`, vazio = detectar, mostrando o que seria detectado); argumentos (`shell.args`, lista); variáveis de ambiente (`shell.env`, lista de nome e valor) |
| **Terminal** | família e tamanho da fonte (`terminal.font.family`, `terminal.font.size`); altura de linha e espaçamento (`terminal.font.line_height`, `letter_spacing`); negrito em cor brilhante (`terminal.font.bold_is_bright`); forma, piscar, cor do grupo e vazado sem foco do cursor (`terminal.cursor.shape`, `blink`, `follows_group_color`, `unfocused_hollow`); linhas de histórico e passo da roda (`terminal.scrollback.lines`, `scroll_multiplier`); rolar com saída, voltar ao final ao digitar, roda vira setas na tela alternativa (`scroll_on_output`, `scroll_on_input`, `alternate_scroll`); copiar ao selecionar e separadores de palavra (`terminal.selection.copy_on_select`, `word_separators`); OSC 52 escrita, leitura e teto (`terminal.clipboard.*`); hyperlinks (`terminal.hyperlinks.enabled`); opacidade do fundo (`terminal.background_opacity`) |
| **Aparência** | tema (`terminal.theme`, RF-16.24); animações (`appearance.window.animations`); opacidade da janela (`appearance.window.opacity`); decorações do sistema (`appearance.window.decorations`); posição da barra de abas (`appearance.window.tab_bar_position`); botão de fechar da aba, número da aba, indicadores de atividade e de campainha, botão de nova aba, ocultar barra com uma aba (`appearance.tabs.show_close_button`, `show_index`, `show_activity_indicator`, `show_bell_indicator`, `show_new_tab_button`, `hide_when_single_tab`); barra de status (`appearance.status_bar.enabled`) |
| **Sessão** | gravar e restaurar a sessão (`session.enabled`); iniciar abas só no primeiro foco (`session.lazy_restore`); restaurar tamanho e posição das janelas (`session.restore_window_geometry`); oferecer integração de shell (`session.suggest_shell_integration`) |
| **Projeto** | arquivo `.porecatu` ligado (`project_file.enabled`); diretórios autorizados (`project_file.trusted_paths`, lista de caminhos, RF-16.27) |
| **Git** | consultar o remoto e intervalo (`git.remote_poll_interval_secs`: alternância que grava `0` desligada, mais o número em segundos) |
| **Painéis** | mínimo de colunas e de linhas (`panes.min_columns`, `panes.min_rows`) |
| **Atalhos** | RF-16.28 a RF-16.31 |

Mudar este catálogo — acrescentar ou tirar uma opção — é mudança deste requisito, não decisão de implementação.

> **Emenda ([PRD-017](prd-017-imagem-de-fundo-do-terminal.md) RF-17.20, [ADR-0061](../adr/0061-imagem-de-fundo-do-terminal.md) §10).** O grupo **Terminal** ganha, junto da opacidade do fundo, três opções: imagem de fundo (`terminal.background_image.path`, campo de texto, com a nota de "arquivo não encontrado" do RF-17.21 abaixo dele, que não impede o Salvar), modo da imagem (`terminal.background_image.mode`, escolha entre `stretch`, `tile` e `center`) e opacidade da imagem (`terminal.background_image.opacity`, campo numérico de `0.0` a `1.0`). Caminho continua sendo texto (RF-16.12).

**RF-16.12** — Cada opção usa o controle que o tipo dela pede ([ADR-0060](../adr/0060-anatomia-da-tela-de-configuracoes.md) §3): **alternância** para booleano; **escolha** entre valores nomeados para enum (`shape`, `tab_bar_position`, `show_close_button`) e para idioma; **campo numérico** com faixa para número; **campo de texto** para texto livre; **lista editável** para lista de textos; **lista de nome e valor** para `shell.env`. Nenhum seletor nativo de arquivo, pasta ou fonte: caminho e família de fonte são texto.

**RF-16.13** — Opção cuja mudança **não vale na hora** (classe C do [ADR-0030](../adr/0030-escopo-do-hot-reload.md)) diz o escopo real ao lado do nome, antes de o usuário mudá-la: "vale em aba nova", "vale na próxima janela", "após reiniciar". O aviso que a recarga já mostra depois de gravar continua.

### Editar, salvar, descartar

**RF-16.14** — Um **rodapé fixo** no painel tem três botões: **Abrir arquivo no editor** (o comportamento antigo da engrenagem, RF-11.27, inclusive criar o arquivo a partir do exemplo se ele não existir), **Descartar** e **Salvar**. Descartar e Salvar ficam indisponíveis — esmaecidos, nunca ausentes — enquanto não há alteração pendente.

**RF-16.15** — Alterar uma opção **não grava**: a alteração fica pendente na janela, marcada na linha e no grupo (RF-16.7). **Salvar** grava todas as pendentes de uma vez, num único gravar atômico; **Descartar** as descarta e mostra de novo o que o arquivo diz. Voltar uma opção à mão ao valor do arquivo deixa de ser pendência.

**RF-16.16** — Cada opção tem **Restaurar padrão**, visível sob o cursor e no foco da linha. Restaurar é uma pendência como outra: ao salvar, a chave **sai do arquivo** em vez de ser regravada com o valor padrão — o arquivo volta a dizer "padrão" e acompanha um padrão que mude numa versão futura.

**RF-16.17** — Salvar altera **só** as chaves com pendência. Comentários, linhas em branco, ordem das chaves e das tabelas, chaves desconhecidas, tabelas fora do catálogo (`[[themes]]`, os tokens de `[appearance.*]`, `[keybindings]` de outras plataformas) e o final de linha (CRLF ou LF) do arquivo ficam **byte a byte** como estavam. Chave que ainda não existe no arquivo entra na tabela dela, criando a tabela no fim se preciso ([ADR-0058](../adr/0058-escrita-do-arquivo-de-configuracao.md) §2).

**RF-16.18** — Valor que a tela recusa — fora da faixa da opção, número malformado, nome de variável de ambiente vazio — é marcado **na própria linha**, com a razão abaixo dela, e **Salvar** fica indisponível enquanto houver algum. A faixa da tela é regra de **edição**, não de leitura: valor fora dela que já está no arquivo, escrito à mão, aparece como está, marcado, e só é regravado se o usuário o alterar.

**RF-16.19** — A gravação nunca produz arquivo que o app recusaria: o texto resultante é relido como configuração **antes** de ir ao disco, e se falhar nada é gravado e um aviso diz por quê ([ADR-0058](../adr/0058-escrita-do-arquivo-de-configuracao.md) §4). Falha de disco (permissão, disco cheio) vira aviso com a causa, e as pendências continuam na janela.

**RF-16.20** — Depois de salvar, a recarga a quente aplica as mudanças a todas as janelas de terminal, como se o arquivo tivesse sido gravado num editor. A tela não aplica nada por conta própria.

### O arquivo

**RF-16.21** — **Arquivo inexistente:** a tela abre normalmente mostrando os padrões. O primeiro Salvar cria o arquivo **a partir do arquivo de exemplo embutido** — o mesmo que a engrenagem criava (RF-11.27) — e aplica as pendências sobre ele, para que o usuário que um dia abrir o arquivo encontre a documentação de cada chave.

**RF-16.22** — **Arquivo inválido** (sintaxe quebrada ou tipo errado): a tela abre **somente leitura**, com uma faixa no topo do painel com o erro (linha e coluna, quando houver) e o botão **Abrir arquivo no editor**. Os controles mostram os valores em vigor — os que a recarga manteve — e não podem ser alterados até o arquivo ser corrigido; corrigido fora, a tela volta ao normal sozinha.

**RF-16.23** — **Arquivo alterado fora da tela.** Se o arquivo muda no disco com a tela aberta (editor externo, `git pull` nos dotfiles):
- **sem pendências**, a tela passa a mostrar o arquivo novo, em silêncio;
- **com pendências**, uma faixa no topo do painel diz que o arquivo mudou fora daqui e oferece **Recarregar** (descarta as pendências e mostra o arquivo novo) e **Manter minhas alterações** (some a faixa; o próximo Salvar aplica as pendências **sobre o arquivo novo**, chave a chave, sem desfazer o que mudou fora). Salvar com a faixa visível equivale a Manter.

A gravação da própria tela nunca dispara essa faixa.

### Tema

**RF-16.24** — A opção de tema é uma **lista**: "sem tema" (as cores de `[terminal.colors]`), os temas embutidos e os de `[[themes]]` do arquivo, na ordem do ciclo ([ADR-0031](../adr/0031-temas-nomeados.md) §3). Cada linha mostra o nome e uma **amostra** com as cores do tema (fundo, texto e as oito ANSI normais). Escolher é uma pendência que grava `terminal.theme`.

**RF-16.25** — Um tema escolhido por `theme.cycle` na sessão em curso **não** é tratado como pendência nem é gravado: a lista mostra o tema do arquivo como escolhido, e uma linha abaixo dela diz qual tema a sessão está usando, se for outro. Salvar um tema pela tela vale a partir da recarga, como gravar `theme` à mão vale hoje (o tema de sessão é descartado só se deixar de existir, [ADR-0031](../adr/0031-temas-nomeados.md) §4).

### Opções com lista

**RF-16.26** — Lista editável (`shell.args`, `project_file.trusted_paths`) e lista de nome e valor (`shell.env`) acrescentam item por um botão no fim da lista, removem pelo botão do item e reordenam por arraste ou por `Alt+Up`/`Alt+Down` com o item focado (`shell.args` é a única em que a ordem importa; as outras aceitam o gesto e ele não muda o efeito). Nome repetido em `shell.env` é recusado na linha (RF-16.18).

**RF-16.27** — Os diretórios autorizados do `.porecatu` levam, acima da lista, o aviso que o arquivo de exemplo já escreve em prosa: um caminho cobre a árvore inteira abaixo dele, e listar a pasta onde se clonam repositórios — ou a pasta pessoal — faz todo projeto clonado ali rodar o próprio `.porecatu`. A mesma razão de a lista ser vazia por padrão ([ADR-0051](../adr/0051-arquivo-de-projeto-porecatu.md)), dita onde o usuário a edita.

### Atalhos

**RF-16.28** — O grupo **Atalhos** lista **todas as ações vinculáveis do catálogo** ([docs/reference/acoes.md](../reference/acoes.md)), agrupadas pelo domínio (`tab.*`, `group.*`, `window.*`, …), com o nome legível da ação e os atalhos **efetivos na plataforma em uso** — o resultado da resolução em três níveis do [ADR-0029](../adr/0029-enum-de-acao-e-gramatica-de-tecla.md), não só o que está no arquivo. Ação sem atalho mostra "sem atalho". Um campo de filtro no topo do grupo filtra a lista pelo nome da ação ou pela tecla.

**RF-16.29** — **Captura.** Clicar no atalho de uma ação (ou `Enter` com ele focado) entra em **captura**: a próxima combinação pressionada vira o atalho novo, mostrada no formato do chip de tecla. `Esc` cancela a captura sem mudar nada; `Backspace` remove o atalho. Tecla que não pode ser atalho (modificador sozinho, tecla morta, IME em composição) é ignorada e a captura continua. Uma combinação que o [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md) proíbe nos defaults (`Ctrl+<letra>` sozinho no Windows e no Linux) é **aceita**, com a advertência de que o terminal deixa de recebê-la: o arquivo já permite, e a tela não é mais restritiva que o arquivo.

**RF-16.30** — **Conflito.** Se a combinação capturada já é atalho de outra ação, a linha mostra o conflito e oferece **Substituir** (a outra ação perde o atalho) ou **Cancelar**. Nenhum atalho fica com duas ações.

**RF-16.31** — Cada ação tem **Restaurar padrão**, que remove do arquivo o que a tela tiver escrito para ela e volta ao atalho embutido. Alterações de atalho são pendências como as outras (RF-16.15), gravadas na tabela de atalhos **da plataforma em uso** — `[keybindings.windows]`, `[keybindings.linux]` ou `[keybindings.macos]` — para nunca mudar o que vale nas outras plataformas de quem compartilha o arquivo entre máquinas ([ADR-0058](../adr/0058-escrita-do-arquivo-de-configuracao.md) §5).

### Acessibilidade e idioma

**RF-16.32** — A janela de configurações tem **árvore de acessibilidade própria** ([ADR-0043](../adr/0043-arvore-de-acessibilidade.md)): guia como lista, cada grupo como item selecionável, cada opção com o papel do controle (alternância, campo, lista), nome, descrição e valor, e o estado de pendente e de inválido expostos. É projeção do mesmo layout que a desenha, como a das janelas de terminal.

**RF-16.33** — Toda frase da tela — nome de grupo, nome e descrição de opção, nome de ação, botões, faixas, mensagens de validação — sai do **catálogo de textos** ([ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md)), em todo arquivo de `locales/`. Valores do arquivo (caminhos, nomes de tema, de família de fonte, de variável) aparecem como estão, sem tradução.

## Cenários

```gherkin
Cenário: a engrenagem abre a tela
  Dado uma janela de terminal aberta
  Quando o usuário clica na engrenagem da barra de abas
  Então uma janela de configurações abre centrada sobre ela, no grupo Geral
  E nenhum editor de texto é aberto

Cenário: abrir de novo traz a mesma janela
  Dado a janela de configurações aberta no grupo Terminal, com uma alteração pendente
  Quando o usuário tecla Ctrl+Shift+O numa janela de terminal
  Então a janela de configurações vem para a frente, ainda no grupo Terminal
  E a alteração continua pendente

Cenário: alterar e salvar
  Dado a fonte em 14
  Quando o usuário muda o tamanho da fonte para 16 e clica em Salvar
  Então o porecatu.toml passa a ter size = 16.0 em [terminal.font]
  E a grade de todas as janelas de terminal é redesenhada com a fonte nova
  E nenhum comentário do arquivo mudou

Cenário: descartar
  Dado três alterações pendentes em dois grupos
  Quando o usuário clica em Descartar
  Então a tela volta a mostrar os valores do arquivo
  E o arquivo não foi gravado

Cenário: restaurar padrão tira a chave do arquivo
  Dado o arquivo com copy_on_select = true
  Quando o usuário restaura o padrão dessa opção e salva
  Então a linha copy_on_select sai do arquivo
  E a opção aparece desligada

Cenário: opção que só vale em aba nova
  Quando o usuário abre o grupo Shell
  Então as opções dizem "vale em aba nova" antes de qualquer alteração

Cenário: valor inválido bloqueia o salvar
  Quando o usuário digita 0 no tamanho da fonte
  Então a linha mostra que o valor está fora da faixa
  E Salvar fica indisponível

Cenário: arquivo inexistente
  Dado nenhum porecatu.toml no caminho resolvido
  Quando o usuário liga "copiar ao selecionar" e salva
  Então o arquivo é criado a partir do exemplo embutido, com copy_on_select = true

Cenário: arquivo inválido
  Dado o porecatu.toml com erro de sintaxe na linha 12
  Quando o usuário abre a tela de configurações
  Então a tela abre somente leitura, com a faixa do erro na linha 12
  E o botão Abrir arquivo no editor

Cenário: mudança externa com pendências
  Dado uma alteração pendente no tamanho da fonte
  Quando o arquivo é gravado num editor externo, mudando o tema
  Então a tela mostra a faixa de arquivo alterado fora daqui
  E Manter minhas alterações seguido de Salvar grava o tamanho da fonte e mantém o tema novo

Cenário: fechar com pendências
  Dado uma alteração pendente
  Quando o usuário fecha a janela de configurações
  Então o diálogo oferece salvar e fechar, descartar e fechar, ou cancelar

Cenário: remapear um atalho com conflito
  Dado tab.rename em Ctrl+Shift+R
  Quando o usuário captura Ctrl+Shift+R para search.open
  Então a linha mostra que Ctrl+Shift+R já é de "Renomear aba"
  E Substituir deixa tab.rename sem atalho e search.open em Ctrl+Shift+R
  E ao salvar as duas mudanças vão para a tabela de atalhos da plataforma em uso

Cenário: escolher um tema
  Quando o usuário escolhe "nord" na lista de temas e salva
  Então terminal.theme = "nord" é gravado
  E as janelas de terminal mudam de cor na recarga
```

## Fora de escopo

Cada item é decisão, não esquecimento.

- **Todas as chaves do arquivo.** As ~200 cores, dimensões, raios e espaçamentos de `[appearance.*]` ficam só no arquivo: são tokens de design ([ADR-0028](../adr/0028-o-binario-como-referencia-visual.md)), e mexer neles sem a especificação visual ao lado produz interface que ninguém aprovou. "Abrir arquivo no editor" é a porta para eles.
- **Criar, editar ou apagar tema; editar cores.** Escolher sim (RF-16.24); o resto pede seletor de cor, que o projeto não tem, e é o que `[[themes]]` já faz.
- **Seletor de fonte com a lista das instaladas.** A família é texto (RF-16.12); fonte ausente continua virando o aviso do RF-5.8 na recarga. Ideia plausível, registrada.
- **Aplicar ao vivo, antes de salvar** (pré-visualização). A recarga aplica depois de gravar; uma pré-visualização exigiria aplicar `Config` que não está no arquivo, que é a segunda fonte de verdade que o [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md) §6 proíbe.
- **Desfazer depois de salvar.** O arquivo é do usuário e pode estar versionado; Descartar cobre o antes de salvar.
- **Busca entre as opções.** O catálogo é curado e cabe em nove grupos; o filtro existe só em Atalhos, que é a única lista longa.
- **Perfis de aba** ([PRD-007](prd-007-perfis-de-aba.md)) e **paleta de comandos** ([PRD-008](prd-008-paleta-de-comandos.md)) — a seção "Perfis instalados" do drawer do canvas continua `[v2]` junto com eles.
- **Tela de configurações dentro da janela de terminal** (sobreposição ou drawer, como o canvas desenha) e **como aba**. A janela própria foi escolha do dono do produto; ver as alternativas do [ADR-0059](../adr/0059-janela-de-configuracoes.md).
- **Configuração por janela.** O `Config` é do processo ([ADR-0030](../adr/0030-escopo-do-hot-reload.md)); a tela edita o arquivo, que vale para todas.
- **Editar `[keybindings]` comum ou de outra plataforma.** A tela mostra e grava só a da plataforma em uso (RF-16.31).

### O que este documento **não** contradiz

O [ADR-0031](../adr/0031-temas-nomeados.md) §4 escreveu que *"o arquivo é do usuário, o app não o edita"*, com a ressalva de que o painel de configurações seria *"a única superfície que um dia escreverá nele, e por ação explícita"*. É esta: nada aqui grava sem o clique em Salvar, e o ciclo de tema e o zoom seguem sem escrever. O [ADR-0005](../adr/0005-persistencia-de-sessao.md) separou estado da máquina (`session.json`) de configuração do usuário (`porecatu.toml`); a janela de configurações fica fora da sessão justamente para não atravessar essa linha.

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Bytes alterados no arquivo fora das chaves com pendência, ao salvar | **zero** |
| Fontes de verdade da configuração | **uma** — o `porecatu.toml` |
| Caminhos de aplicação de config | **um** — a recarga a quente existente |
| Gravações que produzem arquivo recusado pelo app | **zero** |
| Gestos para mudar uma opção do catálogo | **três** (abrir a tela, mudar, salvar), contra abrir o arquivo, achar a chave, acertar a forma e gravar |
| Atalhos com duas ações depois de salvar pela tela | **zero** |
| Dependências novas no workspace | **uma** direta, `toml_edit`, que já está no grafo ([ADR-0058](../adr/0058-escrita-do-arquivo-de-configuracao.md) §1) |

A primeira é a que mantém a promessa ao usuário que versiona o arquivo: se a tela reformatar uma linha que ele não tocou, o `git diff` dele vira ruído, e a segunda porta passa a custar a primeira.
