# ADR-0080 — Editor de perfil em janela própria: formulário em coluna, modal sobre a tela de configurações, OK que só leva às pendências

**Status:** Aceito — aval visual pendente
**Data:** 2026-10-29
**Relacionados:** [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0024](0024-face-de-icones.md), [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md), [ADR-0059](0059-janela-de-configuracoes.md), [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md), [ADR-0064](0064-janela-de-configuracoes-modal.md), [ADR-0070](0070-perfis-de-aba.md), [ADR-0073](0073-credenciais-no-cofre-do-sistema.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md), [ADR-0078](0078-largura-e-quebra-de-texto-do-dialogo.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md), [PRD-020](../prd/prd-020-conexoes-ssh.md)
**Supersedes:** ADR-0075 §6 (**parcial**: o formulário do perfil sai do painel do grupo "Perfis" para uma janela própria; a linha da lista ganha o botão de editar; "+ Perfil local" e "+ Perfil SSH" abrem o editor; perfil inválido não entra na lista pela tela) · ADR-0059 §1 e §2 (**parcial**: a tela de configurações deixa de ser a única janela que não é de terminal — ela pode abrir uma janela filha, o editor de perfil) · ADR-0064 §2 (**parcial**: a modalidade encadeia — com o editor aberto, a própria tela de configurações fica desabilitada)

> **Aceito com o aval visual pendente**, como o [ADR-0075](0075-superficies-de-ui-do-ssh.md). O comportamento está decidido pelo dono do produto; a aparência do editor e do botão de editar passa pelo aval no PR que a implementa. Os únicos valores novos são as quatro dimensões da janela (§10), decididas pelo dono; todo o resto cita o token de onde sai.

## Contexto

O grupo "Perfis" da tela de configurações ([ADR-0075](0075-superficies-de-ui-do-ssh.md) §6) mostra a lista dos perfis e, **abaixo dela, o formulário do perfil escolhido**, montado com a anatomia de "linha de opção" da §2.12 da especificação: rótulo à esquerda, controle à direita (`push_profile_form`, `porecatu-ui/src/settings/content.rs`). Na prática:

- o formulário muda de tamanho com o tipo — um perfil SSH tem oito linhas mais as duas de "Esquecer"; um local tem cinco campos mais as listas de argumentos e de ambiente — e empurra para baixo o que vem depois;
- a lista e o formulário disputam a mesma rolagem, e escolher outro perfil troca o formulário por baixo do cursor;
- a linha de opção foi feita para uma chave curta e um controle estreito (`text_field_width` = 240); host, caminho de chave e diretório ficam apertados nela.

O dono do produto pediu quatro coisas:

1. os campos do perfil num **modal à frente da tela de configurações** — e, perguntado sobre a forma, escolheu uma **janela do sistema própria, como a tela de configurações**, não um diálogo dentro dela;
2. os campos **em linhas, com o rótulo acima** do controle, como num formulário;
3. **OK nesse editor não grava o arquivo** — gravar continua sendo o Salvar da tela de configurações;
4. na linha do perfil, **à esquerda do ✕**, um botão de **editar**.

A alternativa recusada no ADR-0075 — "perfis editados num diálogo à parte, **fora** da tela de configurações" — **não volta** com isto. Ela foi recusada porque tiraria o cadastro do lugar onde se edita o arquivo inteiro e do Salvar único do [PRD-016](../prd/prd-016-tela-de-configuracoes.md). O editor desta decisão é **parte da tela**: só abre a partir dela, só existe enquanto ela existe, e o que ele produz é pendência da tela, gravada pelo mesmo Salvar (RF-20.3).

## Decisão

**O formulário do perfil passa a uma janela do SO própria — o editor de perfil —, filha e modal da tela de configurações, com os campos em coluna e o rótulo acima de cada um. O editor trabalha sobre uma cópia do perfil; OK leva a cópia às alterações pendentes da tela e não escreve nada no disco; Cancelar a descarta. Na lista, cada linha ganha um lápis à esquerda do ✕.**

### 1. Uma janela filha da tela, num campo da tela

`SettingsWindow` ganha `profile_editor: Option<ProfileEditorWindow>` — um campo dela, **não** uma entrada em `App.windows` nem um segundo campo em `App`, pelo mesmo motivo do [ADR-0059](0059-janela-de-configuracoes.md) §1: o compilador aponta onde a janela nova entra, em vez de um `match` espalhado. E é campo **da tela** porque o editor não tem sentido sem ela: fechar a tela leva o editor junto (§6).

- `ProfileEditorWindow` tem `WindowSurface` própria (o `GpuContext` e o atlas de glyphs são os do processo, como para toda janela) e o adaptador `accesskit_winit` dele, com a árvore montada **antes** de a janela ficar visível, como nas outras.
- **Roteamento:** o teste no topo de `window_event` olha primeiro o `WindowId` do editor, depois o da tela, depois o caminho das janelas de terminal, intocado.
- **No máximo um** editor. Pedir para editar com um aberto o traz para a frente (`focus_window`), sem abrir outro.
- **Criação** pelo mesmo `create_window_with_attributes`, **centrada sobre a tela de configurações** e recortada ao monitor dela, como a tela se centra sobre a janela de origem (ADR-0059 §2).
- Fora da sessão e fora de qualquer contagem de janelas, como a tela.

### 2. Modal sobre a tela

- **Windows:** o editor é criado com `with_owner_window` apontando para a tela de configurações — sem botão na barra de tarefas, sempre à frente dela. Enquanto ele existe, a tela fica `set_enable(false)`; as janelas de terminal já estão desabilitadas pela própria tela ([ADR-0064](0064-janela-de-configuracoes-modal.md) §2). Ao fechar, a tela é **reabilitada antes** de o editor ser destruído — com a dona desabilitada, o Windows passaria o foco a outro programa (a mesma lição do ADR-0064 §2).
- **X11:** `_NET_WM_WINDOW_TYPE_DIALOG`, como a tela (revisão do ADR-0064 §4); sem dona nem modal do SO.
- **macOS, Wayland e X11:** onde o SO não desabilita a tela, a modalidade é **lógica**: `SettingsWindow::handle` descarta todo input de teclado e mouse enquanto o editor existe, e um clique nela traz o editor para a frente. Redesenho, recarga e troca de idioma continuam chegando à tela.

### 3. Cabeçalho: só fechar

A faixa de cabeçalho da tela de configurações ([ADR-0060](0060-anatomia-da-tela-de-configuracoes.md)), com a mesma regra de decoração ([ADR-0027](0027-controles-de-janela-e-resize-proprios.md)): fora do macOS, sem decoração nativa, com título, drag region e resize pela borda até o mínimo (§10); no macOS, decoração nativa.

- **Só o botão de fechar.** Minimizar e maximizar não têm sentido num editor modal possuído: minimizar só ele deixaria a tela desabilitada e sem nada à vista. No macOS, `WindowButtons::CLOSE`.
- **Fechar = Cancelar** (§6).
- **Título:** "Editar perfil — `<nome>`" para um perfil existente, "Novo perfil local" e "Novo perfil SSH" para os criados pelos botões. O tipo do perfil (fixo na criação, ADR-0075 §6) aparece só aí, não como campo.

### 4. Formulário em coluna, rótulo acima

Uma linha por campo, **o rótulo acima do controle**:

- **Rótulo:** o nome da linha de opção da §2.12 (12.5px), na cor dele.
- **Controle:** o campo de texto ocupa a **largura útil da janela** — a largura menos o `padding` do painel de opções, sem valor novo. A **porta** mantém `number_field_width`, alinhada à direita, como hoje.
- **Erro:** a nota de erro de linha que a tela já usa, **abaixo do campo**.
- **Espaçamento:** `row_gap` (§5 do ADR-0060) entre uma linha e a seguinte; quando o conteúdo passa da altura, o painel rola com o modelo de rolagem da tela.
- **Ordem dos campos:** a de hoje — `LOCAL_FIELDS` e `SSH_FIELDS` em `porecatu-ui/src/settings/profiles_draft.rs`, a autenticação entre o usuário e o arquivo da chave; no SSH com cofre, **"Senha salva — Esquecer"** e **"Passphrase salva — Esquecer"**; no local, as listas de argumentos e de ambiente.
- **Controles:** os que a tela já tem — `TextFieldState` ([ADR-0035](0035-selecao-de-texto-em-campo-de-nome.md)), a escolha, as listas de um e de dois campos. **Nenhum widget novo**; o que muda é a disposição.

### 5. Rodapé e teclado

- **Rodapé:** **Cancelar** e **OK**, à direita, na anatomia do rodapé da tela; OK é o botão primário dela.
- `Tab`/`Shift+Tab` percorrem os campos e depois os botões, com o anel de foco da tela; o foco inicial é o primeiro campo.
- `Enter` num campo de uma linha = OK. Nas listas, `Enter` segue o que a lista faz hoje.
- `Esc` = Cancelar, quando nenhum campo ou lista está consumindo o `Esc`.
- `Ctrl+S`/`Cmd+S` **não faz nada** no editor: salvar é da tela, e a tela só se alcança depois de fechar o editor.

### 6. OK leva às pendências; Cancelar descarta

O editor trabalha sobre uma **cópia** do `Form` do perfil (ou sobre um `Form` novo, na criação). O `ProfilesDraft` da tela não muda enquanto o editor está aberto.

- **OK** valida a cópia **contra o rascunho com ela no lugar** — a unicidade do `id` é conferida contra os outros perfis do rascunho, inclusive os pendentes —, pela mesma validação de hoje ([ADR-0070](0070-perfis-de-aba.md) §1, [ADR-0072](0072-cliente-ssh-embutido.md) §6).
  - **Sem problema:** a cópia substitui o perfil no `ProfilesDraft` (ou entra no fim da lista, se é novo, com o `id` gerado do nome como hoje) e o editor fecha. O perfil fica **pendente** na lista, com o ponto de pendente, e entra na contagem do rodapé da tela. **Nada é escrito no disco.**
  - **Com problema:** o editor **não fecha**; cada campo inválido mostra a nota de erro e o foco vai para o primeiro deles. Um perfil inválido **nunca entra na lista pela tela**.
- **Cancelar**, `Esc` e o botão de fechar descartam a cópia. Num perfil novo, **nada é criado**.

> **Nota de implementação.** "Identificada pelo `id` de origem" vale para o perfil que veio do arquivo. Um perfil criado na tela (ou importado) e já confirmado com OK **não tem `id` de origem** até o Salvar, e o `rebase` o troca depois; para o editor poder reabri-lo, cada item do rascunho tem uma **chave interna** monotônica, nunca reusada, que o editor guarda no lugar do índice. O `id` de origem segue servindo só às edições do Salvar.

- **Gravar** continua sendo o Salvar da tela ([PRD-016](../prd/prd-016-tela-de-configuracoes.md)), pelas operações de `ArrayOfTables` do [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md) — sem mudança. Descartar na tela desfaz também o que o editor levou às pendências.
- **Perfil inválido vindo do arquivo** (editado à mão) continua na lista, marcado, e não impede salvar os outros (ADR-0075 §6). Abrir o editor nele mostra os erros, e o OK só fecha com eles corrigidos.
- **"Esquecer"** age **na hora** no cofre ([ADR-0073](0073-credenciais-no-cofre-do-sistema.md) §4), como hoje: Cancelar não desfaz. Esquecer não muda o arquivo, então não é pendência, e um segredo esquecido não tem como voltar.
- **Recarga com o editor aberto:** o editor mantém a cópia. Se o perfil sumiu do arquivo, o OK o devolve ao rascunho como novo; o conflito entre o arquivo e as pendências continua sendo da faixa da tela (ADR-0058 §3).
- **A tela fecha com o editor aberto** (encerramento do app, `app.quit`, fechar pelo SO onde a tela não está desabilitada): o editor fecha **como Cancelar** primeiro, e o fluxo de pendências do ADR-0059 §2 segue com o que já estava no rascunho.

### 7. A lista no painel

O painel do grupo "Perfis" fica com o topo (`profiles.default`, `profiles.detect`), a linha dos três botões e a lista. **A seção "Perfil escolhido" sai.**

- **Linha:** badge, nome, a sub-linha (`usuário@host:porta` ou o programa), o ponto de pendente, o **lápis** e o **✕**, nessa ordem — o lápis imediatamente à esquerda do ✕.
- **Lápis:** ícone `pencil` da face Lucide ([ADR-0024](0024-face-de-icones.md)), constante nova `porecatu_render::icon::PENCIL`. A fonte embutida não é recortada; o ícone só ganha o nome. **Mesma anatomia do ✕** da linha: a mesma em, a mesma cor, o mesmo hover, o mesmo espaçamento — layout por `Icon::ink_width`, centragem por `Icon::centered_origin`. Nenhum valor novo.
- **Gestos:** clique simples numa linha só a **escolhe** (destaque e alvo do teclado); **lápis**, **duplo clique** na linha (fora do lápis e do ✕) e **`Enter`** com a linha escolhida abrem o editor. Setas continuam navegando; o ✕ continua abrindo o diálogo de excluir (ADR-0075 §6, sem mudança).
- **"+ Perfil local" e "+ Perfil SSH"** abrem o editor com um perfil novo daquele tipo; nada entra na lista até o OK.
- **"Importar do ~/.ssh/config"** continua abrindo a lista de marcar **no painel**, abaixo da lista de perfis — no espaço que era do formulário —, sem nenhuma mudança (ADR-0075 §7). Os perfis importados entram como pendentes, sem passar pelo editor.

### 8. Acessibilidade

- `access.rs` ganha `build_profile_editor_tree`, projeção do layout do editor (o princípio do [ADR-0043](0043-arvore-de-acessibilidade.md) §2), com bloco de `NodeId` próprio, como a tela tem o dela.
- Cada campo é ligado ao rótulo acima dele (`labelled_by`); o erro, quando há, é a descrição do campo. Papéis os mesmos da tela (`TextInput`, `SpinButton`, `ComboBox`, lista).
- Na lista, o lápis é um nó `Button` com o nome "Editar `<nome>`", ao lado do ✕; a linha continua `ListBoxOption`.

### 9. Idioma

Frases novas no registro de mensagens de `porecatu-ui` e em **todo** arquivo de `locales/` ([ADR-0056](0056-catalogo-de-textos-da-interface.md)): o título de editar (com o nome), "Novo perfil local", "Novo perfil SSH", o nome do lápis ("Editar") e "OK". "Cancelar" e os rótulos dos campos já existem e são reusados.

### 10. Tamanho da janela

Quatro chaves novas em `[appearance.settings]`, decididas pelo dono:

| Chave | Default |
|---|---|
| `profile_window_width` | 560 |
| `profile_window_height` | 600 |
| `profile_min_width` | 420 |
| `profile_min_height` | 360 |

560 × 600 cabe o formulário SSH inteiro (oito campos com rótulo acima, mais as duas linhas de "Esquecer") sem rolar; o local rola quando as listas crescem. No PR de implementação elas entram **juntas** em `Config`, no [porecatu.example.toml](../config/porecatu.example.toml), na especificação visual e em `VALORES` (`scripts/verify-docs.py`).

> **Revisão (pedido do dono do produto, depois de ver o editor).** O default de `profile_window_height` caiu de **600 para 398**; a largura (560) e os mínimos (420 × 360) ficam. Onde este ADR diz 600, leia 398 -- e a frase acima de que o formulário SSH cabe sem rolar deixa de valer: com 398 o corpo rola. O mesmo pedido pôs nome, identificador e badge numa linha só, host e porta noutra, e os botões "Esquecer" no rodapé (§4, §5), o que encurta o formulário.

## Alternativas consideradas

### Diálogo na camada modal da própria tela de configurações

Era a primeira leitura do pedido: o diálogo do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), sobre a tela, com um formulário dentro. Mais barato — nada de janela nova, de segundo roteamento nem de modal encadeado. Recusado pelo dono do produto, que quer uma janela própria, como a da tela. E o diálogo é feito para título, poucas linhas e botões, com teto de 720 ([ADR-0078](0078-largura-e-quebra-de-texto-do-dialogo.md)); um formulário com listas e rolagem o esticaria para algo que ele não é.

### Manter o formulário no painel, só com o rótulo acima

Resolve o aperto dos campos, não os outros dois problemas: o formulário continua empurrando o painel e trocando por baixo do cursor. E o dono pediu o modal.

### OK gravando o arquivo

Daria ao editor um "salvar" próprio. Recusado pelo pedido e pelo [PRD-016](../prd/prd-016-tela-de-configuracoes.md): a tela tem **um** Salvar, e um segundo, que grava um pedaço do arquivo enquanto o resto fica pendente, faria do Descartar uma mentira.

### OK aceitando perfil inválido, marcado na lista

O comportamento de hoje. Recusado pelo dono: com o formulário fora da vista, um perfil inválido na lista é um problema que o usuário só vê de novo se abrir o editor. Bloquear o OK mostra o erro onde ele está sendo cometido.

### Botão "Editar" em texto

Mais explícito. Recusado: mais largo que o ✕ e de outra anatomia, desequilibra a linha; o lápis tem o mesmo peso do ✕, e o nome acessível diz o que ele é.

### O editor como janela de primeira classe em `App`

Um `profile_editor` ao lado de `settings` em `App`. Recusado no §1: o editor depende da tela e morre com ela; como campo dela, a dependência está no tipo.

## Consequências

### Positivas

- O painel do grupo "Perfis" fica estável: topo, botões e lista, sem um formulário que muda de tamanho.
- Campos com a largura da janela, rótulo acima: host, caminho de chave e diretório deixam de ficar apertados.
- Um perfil inválido não entra na lista pela tela, e o erro aparece onde está sendo cometido.
- Nenhum widget novo, nenhuma cor nova: os controles da tela, numa disposição em coluna, e um ícone novo de uma face que já está embutida.
- O Salvar continua único; o arquivo continua editado num lugar só.

### Negativas

- Uma segunda janela que não é de terminal, com surface, árvore de acessibilidade e roteamento próprios, e uma modalidade em dois níveis (terminais desabilitadas pela tela, tela desabilitada pelo editor).
- Fora do Windows a modalidade é lógica, não do SO: a tela continua clicável para o sistema e é o código que descarta o input.
- Editar um perfil custa um gesto a mais (abrir o editor) do que escolher a linha.
- Quatro chaves novas de aparência.
- O layout do formulário em coluna é uma segunda função de layout ao lado da do painel, e as duas precisam reusar os mesmos controles para não divergirem.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Ao fechar o editor, o Windows passar o foco a outro programa | Média | Médio | §2: reabilitar a tela **antes** de destruir o editor, como o ADR-0064 §2 faz com as janelas de terminal |
| Evento do editor cair no roteamento da tela ou de terminal | Baixa | Médio | §1: o `WindowId` do editor é testado primeiro; teste de roteamento com as três janelas |
| Input chegar à tela por baixo do editor (macOS, Wayland, X11) | Média | Médio | §2: descarte explícito em `SettingsWindow::handle` com o editor aberto; teste puro de que clique e tecla não mudam o rascunho |
| Tela fechada com o editor aberto deixar o editor órfão | Baixa | Alto | §1 e §6: o editor é campo da tela e cai com ela, como Cancelar; teste de encerramento com o editor aberto |
| OK aplicar a cópia ao perfil errado depois de uma exclusão ou recarga | Baixa | Médio | §6: a cópia é identificada pelo `id` de origem (como as edições do ADR-0075 §6), nunca pelo índice; teste que exclui um perfil anterior com o editor aberto |
| Validação do editor divergir da do rascunho (`id` repetido aceito) | Baixa | Médio | §6: a validação é a do `ProfilesDraft`, chamada com a cópia no lugar, não uma segunda |
| Aparência entrar sem aval | Baixa | Médio | Status com aval pendente; aval no PR que implementa |
