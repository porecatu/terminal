# ADR-0098 — Monitoramento em segundo plano do gerenciador de tarefas: chave `[task_manager] background_monitoring` e alternância no rodapé da lista

**Status:** Aceito — aval visual do dono **pendente** (§5)
**Data:** 2026-11-10
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md), [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md), [ADR-0090](0090-transferencia-de-abas-entre-janelas.md), [ADR-0093](0093-medicao-de-processos-do-gerenciador.md), [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md), [ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md), [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md)
**Supersedes:** ADR-0093 §4, §6 e a alternativa "Medir sempre, desde o arranque" (**parcial**: o amostrador também existe com a chave ligada, e a resposta alimenta o histórico de toda janela); ADR-0095 §2 e §9 (**parcial**: fechar o gerenciador não derruba o amostrador com a chave ligada; a seção `[task_manager]` ganha uma segunda chave e a tela de configurações, uma segunda opção); ADR-0096 §4 (**parcial**: a coluna da lista ganha um rodapé fixo com a alternância)

## Contexto

O [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md) decidiu que a medição só acontece com o gerenciador aberto (RF-24.24). Fechar o gerenciador da última janela derruba o amostrador ([ADR-0093](0093-medicao-de-processos-do-gerenciador.md) §4), e os segundos sem medição viram lacuna no gráfico (RF-24.12). Quem alterna entre o terminal e o gerenciador para acompanhar um processo perde justamente o trecho em que estava trabalhando: o gráfico só conta o que aconteceu enquanto o usuário olhava para ele.

O dono do produto pediu uma **alternância no canto inferior direito do gerenciador, logo abaixo da lista de processos**, entre medir só com o gerenciador aberto (o comportamento de hoje) e medir o tempo todo, para alternar entre as abas e o gerenciador sem perder o histórico. Ele decidiu, antes da escrita:

- a escolha é uma **chave do `porecatu.toml`**, gravada pela alternância, e aparece também na tela de configurações;
- ela vale para o **app inteiro**, não por janela;
- ligada, a medição começa **no arranque**, e o gerenciador já abre com o gráfico cheio.

O que já existe e serve de molde:

- `sampler_needed` (`crates/porecatu-ui/src/app_task_manager.rs`) decide se o amostrador deve existir, hoje só pela pergunta "alguma janela tem o gerenciador aberto?". `stop_task_manager_sampler_if_unused` e `task_manager_tick` a consultam.
- `sample_roots` monta o pedido com os painéis das janelas com o gerenciador aberto, e `apply_process_sample` aplica a resposta só a essas janelas.
- `porecatu-config::edit` ([ADR-0058](0058-escrita-do-arquivo-de-configuracao.md)) grava uma lista de edições por chave sobre o documento do usuário, de forma atômica, com o conflito decidido por conteúdo. Até aqui só a tela de configurações e o editor de perfil a usam.
- A alternância já desenhada da especificação visual (trilho 34×19, §1.5 e §2.12), pintada por `crates/porecatu-ui/src/toggle.rs::push_toggle`, é usada pela tela de configurações, pela barra de busca (expressão regular) e pelo diálogo de segredo do SSH.

## Decisão

**Uma chave booleana `[task_manager] background_monitoring`, default `false`, de classe A, mantém o amostrador vivo desde o arranque e alimenta o histórico de toda janela, aberta ou não. A coluna da lista ganha um rodapé fixo com uma alternância à direita, que grava a chave pelo `porecatu-config::edit` e desenha sempre o valor do `Config` corrente. A tela de configurações ganha a mesma opção. Nenhum valor visual é novo.**

### 1. A chave

Em `crates/porecatu-config/src/task_manager.rs`, ao lado de `history_seconds`:

```rust
/// Mede os processos com o gerenciador fechado, para os gráficos guardarem o
/// histórico (ADR-0098). `false` mantém o custo zero do RF-24.24.
pub background_monitoring: bool,   // default false
```

- **Default `false`**: quem nunca liga a opção continua com zero varreduras com o gerenciador fechado, a métrica do PRD-024.
- **Classe A** ([ADR-0030](0030-escopo-do-hot-reload.md)): o `App` lê `self.config.task_manager.background_monitoring` a cada tick do amostrador, como já lê `history_seconds`. O `reload::diff` não ganha código. Ligar com o app aberto cria o amostrador no próximo tick; desligar o derruba no próximo tick, se nenhuma janela tiver o gerenciador aberto.
- **Arquivo de exemplo**, na seção `[task_manager]` que já existe, entra **no PR de implementação** (o `tests/example_toml.rs` reprova chave sem código, como no [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §9):

  ```toml
  # Continua medindo os processos com o gerenciador fechado, para os gráficos
  # guardarem o histórico enquanto você usa as abas. Custa uma varredura de
  # processos por segundo enquanto o app está aberto. Vale na hora.
  background_monitoring = false
  ```

### 2. O ciclo do amostrador

Revê o [ADR-0093](0093-medicao-de-processos-do-gerenciador.md) §4. A pergunta de `sampler_needed` passa a ser:

```text
background_monitoring || alguma janela tem o gerenciador aberto
```

- **Arranque**, com a chave ligada: o amostrador é criado quando a primeira janela existe, e `task_manager_next_sample` é agendado para `now`, como na abertura do primeiro gerenciador hoje. O primeiro ponto de CPU chega no segundo tick (RF-24.9), agora contado do arranque.
- **`leave_task_manager`** e **fechar a janela** continuam chamando `stop_task_manager_sampler_if_unused`, que, com a chave ligada, não derruba nada.
- **Desligar a chave** com todos os gerenciadores fechados: o tick seguinte vê `sampler_needed == false` e derruba o amostrador pelo caminho de sempre (o `Sender` cai e a thread termina). O histórico já registrado fica, e o tempo dali em diante é lacuna, pela regra do RF-24.12.
- **O ritmo, o pedido que não acumula e a recuperação de resposta perdida** não mudam. O relógio continua sendo o `WaitUntil` do `App` ([ADR-0007](0007-modelo-de-threading.md)); nenhuma thread dorme para marcar o ritmo.

### 3. O pedido e a aplicação da resposta

Revê o [ADR-0093](0093-medicao-de-processos-do-gerenciador.md) §5 e §6, sem mudar nenhum tipo de `porecatu-pty`:

- **Pedido**: com a chave ligada, `sample_roots` inclui os painéis com processo local de **todas** as janelas. Desligada, só os das janelas com o gerenciador aberto, como hoje. A regra das chaves (`(WindowId, TabId, PaneId)`) e dos painéis que não entram (não iniciado, SSH, encerrado) é a mesma.
- **Resposta**: `apply_process_sample` acrescenta o ponto ao `task_manager_history` de **toda janela que aparece na resposta e ainda existe**, aberta ou não. A `TaskManagerView` (a última amostra, a lista, a seleção) continua só nas janelas com o gerenciador aberto: a janela fechada guarda só o histórico dos gráficos, que é o que o pedido quer preservar.
- **Abrir o gerenciador** com a chave ligada: a primeira resposta depois da abertura preenche a lista, como hoje. Até ela chegar (no máximo um segundo), a lista mostra a nota "aguardando a primeira medição", e os gráficos já mostram o histórico.
- **Transferência** ([ADR-0090](0090-transferencia-de-abas-entre-janelas.md)): nada muda. O próximo pedido já sai com as chaves novas, e o histórico das duas janelas não é recalculado (RF-24.25).
- **Lacuna**: continua sendo só a falta de medição. Com a chave ligada desde o arranque, o gráfico não tem lacuna nenhuma por abrir e fechar o gerenciador.

O custo é o que o ADR-0093 já mediu para o gerenciador aberto: uma varredura por segundo, numa thread, servindo todas as janelas. A memória a mais é um `TaskManagerHistory` por janela, limitado por `history_seconds` (no máximo 600 pontos de quatro números).

### 4. A alternância grava a chave

É o primeiro caminho de gravação da config fora da tela de configurações e do editor de perfil. Ele usa o mesmo módulo, sem regra nova de escrita:

1. lê o arquivo de config atual num `ConfigDocument` (o exemplo embutido, se o arquivo não existe, como no RF-16.21);
2. aplica `Edit::Set(KeyPath::parse("task_manager.background_monitoring"), EditValue::Bool(!atual))`, em que `atual` é o valor do `Config` corrente;
3. grava com `ConfigDocument::save` (temporário, `rename` sobre o alvo real do link, revalidação pelo `parse` da carga, [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md) §3 e §4);
4. `EditError::Changed` (o arquivo mudou entre a leitura e a gravação): `rebase` com o texto novo e reaplica **uma vez**. A edição por chave dá isso de graça. Um segundo `Changed` vira erro;
5. qualquer erro (arquivo só de leitura, config inválida em disco, segundo `Changed`) vira um aviso `Severity::Warning` com texto do catálogo (`msg::notice::task_manager_background_save_failed`), e nada mais muda.

**Quem aplica é o hot reload**, como na tela de configurações: a gravação dispara o watcher, e a recarga traz o valor novo ao `Config`. A alternância **desenha sempre `self.config.task_manager.background_monitoring`**, nunca um estado próprio. Por isso uma gravação que falha deixa a alternância como estava, e uma edição à mão no arquivo aparece nela sem código.

A gravação é feita na main thread, como a da tela de configurações: é um arquivo pequeno, e o gesto é raro. Se a tela de configurações estiver aberta com uma pendência na mesma chave, a próxima gravação dela encontra o arquivo mudado e segue o conflito do RF-16.23 de sempre.

### 5. O rodapé da lista

Revê o [ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md) §4. **Nenhum valor novo**: tudo vem do `[appearance.task_manager]` que já existe e da alternância da especificação visual.

- **Lugar**: o fim da coluna direita, na largura dela, fixo. A lista passa a rolar entre o cabeçalho e o rodapé, recortada pelo mesmo `PushClip`, que encolhe uma linha. A coluna dos gráficos não muda.
- **Altura**: `row_height` **28**.
- **Separador**: um `Quad` de 1 px na cor `chart_border`, na largura da coluna, na borda de cima do rodapé. É o espelho do separador sob o cabeçalho.
- **Conteúdo, alinhado à direita**, com `row_padding_x` **8** da borda direita da coluna: a alternância de `push_toggle` (trilho 34×19, raio 10, `padding` 2; ligado e desligado nas cores da alternância do tema; botão 15×15; **sem transição**) centrada na vertical; à esquerda dela, a `row_padding_x` **8**, o rótulo "Monitorar em segundo plano" (do catálogo) na fonte do sistema, 12.5, em texto Secundário, centrado pelo render.
- **Hit-test**: o retângulo do rótulo mais o trilho alterna a chave no release, como as linhas de alternância da tela de configurações. Sem hover próprio, como elas.
- **Janela estreita**: o rótulo termina em reticências antes de a alternância ser cortada; a alternância nunca é cortada, como as colunas numéricas.
- **Geometria**: a do rodapé entra em `task_manager::paint::layout`, a única geometria do gerenciador, e o hit-test a lê de lá.

A especificação visual ganha a linha do rodapé na seção do gerenciador **no PR que muda o binário** ([ADR-0028](0028-o-binario-como-referencia-visual.md)). O aval visual do dono fica **pendente** até ele ver em tela.

### 6. Entrada e acessibilidade

- **Mouse**: o rodapé está dentro da área de conteúdo do gerenciador, então o clique nele nunca chega ao painel ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §4). A roda sobre o rodapé rola a lista, como sobre o cabeçalho.
- **Teclado**: o elo `task_manager_key_route` **não muda**. As setas, `Home`, `End` e as páginas continuam só na lista, e nenhuma tecla nova é capturada. O caminho por teclado é a opção da tela de configurações (§7). Não entra ação nova no catálogo: o dono não pediu atalho, e uma ação `task_manager.*` a mais seria mais uma linha no catálogo fechado sem uso.
- **Acessibilidade** ([ADR-0043](0043-arvore-de-acessibilidade.md)): um nó `Role::Switch`, filho do grupo "Gerenciador de tarefas" e irmão dos gráficos e da lista, com o nome do rótulo, `Toggled::True`/`False` pelo valor do `Config` e a ação `Click`, roteada para o mesmo caminho do §4. O leitor de tela alterna a opção sem o mouse.

### 7. A tela de configurações

O grupo **Gerenciador de tarefas** ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §9) ganha uma segunda opção, **antes** do período: um `option!` com controle de alternância, caminho `"task_manager.background_monitoring"`, lido por `|c| EditValue::Bool(c.task_manager.background_monitoring)`, rótulo e descrição do catálogo. A anatomia é a linha de opção com alternância do [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md) §3. O teste `the_catalog_has_exactly_the_options_of_rf_16_11` passa de 59 para 60 opções, e `probe_scope` devolve `Live` sem código próprio. Isso emenda o RF-16.11 do [PRD-016](../prd/prd-016-tela-de-configuracoes.md) no mesmo PR deste ADR.

### 8. Textos

Todo texto novo nasce no registro de mensagens de `porecatu-ui`, com valor em **todo** arquivo de `locales/` ([ADR-0056](0056-catalogo-de-textos-da-interface.md)): o rótulo do rodapé (que é também o nome do nó de acessibilidade), o rótulo e a descrição da tela de configurações, e o aviso de falha de gravação. Em `pt_BR`, o rótulo é "Monitorar em segundo plano", e a descrição diz que os gráficos guardam o histórico com o gerenciador fechado, ao custo de uma varredura de processos por segundo.

### 9. Onde o código mora

| Crate | O que muda |
|---|---|
| `porecatu-config` | `TaskManager::background_monitoring`, default e o teste `default_matches_example_toml` |
| `porecatu-ui` | `sampler_needed`, `sample_roots` e `apply_process_sample` com a chave (§2, §3); o rodapé em `task_manager::paint::layout` e na pintura; o hit-test e a gravação (§4); o nó `Switch`; o `option!` da tela de configurações; o aviso; as mensagens |
| `locales/` | os textos, em todos os idiomas |
| `porecatu-pty`, `porecatu-term`, `porecatu-render`, `porecatu-core`, `porecatu-session`, `porecatu-ssh`, `porecatu-locale` | nada |

O PR de implementação traz estes testes:

- `sampler_needed` com a chave ligada e nenhum gerenciador aberto devolve `true`; desligada, o comportamento de hoje (puro);
- `sample_roots` com a chave ligada inclui janelas com o gerenciador fechado (puro);
- `apply_process_sample` acrescenta ponto ao histórico de uma janela fechada e não cria `TaskManagerView` nela (puro);
- a geometria do rodapé: a lista encolhe uma linha, a alternância fica a `row_padding_x` da borda direita e nunca é cortada em janela estreita (puro, em `layout`);
- a edição gerada pela alternância é `Set` da chave com o valor invertido, e um `Changed` reaplica uma vez (puro, sobre `ConfigDocument`);
- a contagem de opções da tela de configurações (60).

## Alternativas consideradas

### Estado só da execução, nunca gravado

Mais simples: nenhum caminho de gravação novo. Recusada pelo dono: quem quer o histórico contínuo quer sempre, e religar a cada arranque seria um gesto repetido. Também deixaria a tela de configurações sem a opção.

### Uma escolha por janela

Cada janela decidiria se continua medindo. Recusada pelo dono, e também pelo custo de regra: a thread já é uma só e serve todas as janelas, então medir só algumas não economiza a varredura, só o histórico. Seria mais estado na `WindowState` e uma pergunta a mais na transferência de abas (a aba que muda de janela leva a escolha ou não?).

### Começar a medir só na primeira abertura do gerenciador

O app não mediria nada até o usuário abrir o gerenciador uma vez na execução. Recusada pelo dono: o primeiro gráfico abriria vazio, que é o que o pedido quer evitar. Quem liga a opção já aceitou o custo da varredura contínua.

### Ação nova `task_manager.background_toggle`

Daria um atalho à opção. Recusada: o dono não pediu atalho, o catálogo de ações é fechado, e a tela de configurações já é o caminho de teclado. Se vier o pedido, a ação entra por ADR, sem mudar a chave.

### Aplicar o valor na memória antes do hot reload

A alternância mudaria sem esperar a recarga. Recusada: seriam duas fontes de verdade para a mesma chave, e uma gravação que falhasse deixaria o app medindo com um valor que o arquivo não tem. A tela de configurações já depende da recarga para aplicar o que grava, e a latência dela é a mesma aqui.

### A alternância dentro da coluna dos gráficos

Perto do que ela controla (o histórico). Recusada pelo pedido do dono: canto inferior direito, abaixo da lista. A coluna dos gráficos também teria de encolher os três blocos, que hoje têm altura igual.

## Consequências

### Positivas

- O usuário alterna entre as abas e o gerenciador sem perder o histórico, e o gerenciador abre com o gráfico cheio.
- O custo zero continua sendo o padrão: com a chave desligada, nada muda em relação ao PRD-024.
- Nenhum tipo de `porecatu-pty` muda, e nenhuma thread nova existe: a mesma thread de amostragem, com uma condição a mais para viver.
- Nenhum valor visual novo: o rodapé reaproveita as medidas da lista e a alternância da especificação.

### Negativas

- **Uma varredura por segundo durante toda a execução** para quem liga a opção, inclusive com o gerenciador fechado em todas as janelas.
- O ADR-0093 §4 deixa de valer como estava: "o amostrador existe só com gerenciador aberto" vira "com gerenciador aberto ou com a chave ligada".
- A gravação da config deixa de ser exclusiva da tela de configurações e do editor de perfil. O caminho é o mesmo módulo, mas mais um chamador.
- A lista perde uma linha de altura útil.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| A latência do hot reload deixar a alternância "atrasada" depois do clique | Média | Baixo | Aval em tela do dono; a recarga é a mesma da tela de configurações, e a alternância nunca mostra um valor que o arquivo não tem (§4) |
| Gravar a config por um clique apagar comentário ou reordenar o arquivo | Baixa | Médio | Edição por chave do ADR-0058, que troca só o valor e herda a decoração; o teste de byte a byte fora da chave já existe |
| A varredura contínua pesar numa máquina com milhares de processos | Baixa | Médio | Opção desligada por padrão; a descrição na tela e no arquivo de exemplo diz o custo; o pedido não acumula (ADR-0093 §4) |
| O amostrador não cair ao desligar a chave | Baixa | Baixo | O tick reavalia `sampler_needed` (§2), com teste |
| Uma janela fechada acumular `TaskManagerView` órfã | Baixa | Baixo | A resposta só cria histórico nas janelas fechadas (§3), com teste |
