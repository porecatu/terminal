# ADR-0109 — O celular muda a aba ativa do desktop: ações com a ativação do gesto local e `activate-tab`

**Status:** Superseded by ADR-0111 (2026-11-20) · antes: aceito em 2026-11-17 — implementado em 2026-11-17 (`App::remote_activate_tab` e as demais em `porecatu-ui/src/app_remote.rs`, decisões puras em `remote/actions.rs`; verificação ao vivo é dívida, no [roadmap](../roadmap.md))
**Data:** 2026-11-17
**Supersedes (parcial):** ADR-0107 §2 (o celular passa a mudar a aba ativa; a janela do sistema continua intocada) e a frase do ADR-0107 §2 sobre o gerenciador de tarefas · PRD-025 RF-25.27 e a última cláusula do RF-25.28 (emendados)
**Relacionados:** [PRD-002](../prd/prd-002-grupos-de-abas.md), [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md), [PRD-025](../prd/prd-025-acesso-remoto.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0090](0090-transferencia-de-abas-entre-janelas.md), [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md), [ADR-0103](0103-acesso-remoto-tres-partes-e-protocolo-externo.md), [ADR-0105](0105-espelhamento-da-grade-e-input-remoto.md), [ADR-0107](0107-acoes-remotas-sobre-o-workspace.md)

## Contexto

O [ADR-0107](0107-acoes-remotas-sobre-o-workspace.md) §2 decidiu que nenhuma ação remota muda o foco nem a aba ativa do desktop. Para isso, a implementação deu "não ativar" a caminhos que o gesto local sempre ativa: `new-tab` criava a aba sem ativar, expandir grupo restaurava a aba ativa que o core troca, e mover para outra janela chegava ao destino sem ativar nada. A especificação 0.3 do protocolo tinha a mesma regra (§4 e §7 de [03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md)), e não havia ação para trocar a aba ativa.

O motivo era proteger quem está **no desktop ao mesmo tempo**: se o celular troca a aba ativa enquanto alguém digita no teclado do desktop, as teclas seguintes vão para outra aba. O dono do produto decidiu, durante a implementação, que esse risco não justifica a regra. O uso típico é uma pessoa só, longe da mesa. Quem volta para o desktop quer encontrar ativa a aba em que estava trabalhando pelo celular, e o desvio "sem ativar" fazia cada ação remota diferir do gesto local que ela diz reproduzir.

## Decisão

> **Substituído pelo [ADR-0111](0111-o-celular-nao-muda-a-aba-ativa-do-desktop.md) (2026-11-20).** O dono desfez esta decisão por inteiro: nenhuma ação remota muda a aba ativa do desktop, `activate-tab` sai do protocolo (especificação 0.5) e o ADR-0107 §2 volta a valer. A janela do sistema continua intocada (§3 abaixo, que já era regra). A aba que o celular vê passa a ser só indicada na barra ([ADR-0112](0112-indicador-de-aba-vista-pelo-celular.md)).

**A ação remota segue o gesto local por inteiro, inclusive a ativação de aba que ele faz. O protocolo ganha `activate-tab`. A janela do sistema operacional continua intocada: o celular nunca levanta, desminimiza nem dá foco a janela.**

### 1. Ações com a ativação do gesto local

| Ação | O que acontece com a aba ativa | Equivalente local |
|---|---|---|
| `new-tab` | A aba criada fica ativa na janela dela, e a barra rola até ela | `tab.new`, "Nova aba", menu de perfis |
| `close-tab`, `close-pane` | Se a aba ativa sai, a vizinha é ativada pela escada de foco (como já era no ADR-0107 §2) | `tab.close`, `pane.close` |
| `collapse-group{collapsed: false}` | Ativa o que o desktop ativa ao expandir | clique na pílula do grupo |
| `collapse-group{collapsed: true}` | Se o grupo contém a aba ativa, ela muda pela escada (RF-2.14, RF-2.17) | clique na pílula do grupo |
| `move-tab-to-window`, `move-group-to-window` | A aba ou o grupo movido fica ativo na janela de destino; na origem, a escada de foco | arraste, "Mover para janela…" |
| `start-pane` | A aba do painel fica ativa na janela dela e sobe (no desktop, a aba não iniciada sobe quando é ativada); SSH dormente conecta | ativar a aba, clique ou `Enter` no painel |
| `move-tab-to-group`, `new-group`, `rename-*`, `set-group-color`, `ungroup` | O mesmo que o gesto local faz | menu da aba, editor de grupo |

O caminho continua sendo o do ADR-0107 §1: a mesma função do gesto local, com alvo explícito. Onde a implementação deu a uma função o parâmetro "não ativar", o caminho remoto passa a pedir a ativação. O parâmetro pode ficar, porque a restauração de sessão também cria abas sem ativar. O que continua de fora é o que só faz sentido com alguém olhando o desktop: `new-group` não abre o editor de grupo, e nenhuma ação abre diálogo (ADR-0107 §3).

### 2. `activate-tab`

Ação nova no protocolo: `[13, { "tab": tab-id }]`. Ela torna a aba ativa **na janela dela**, pela mesma função do clique na aba:

- Se a aba está num grupo colapsado, o grupo expande (RF-2.17).
- Se a aba está `NotStarted`, ela sobe ([ADR-0037](0037-aba-nao-iniciada.md)).
- Uma aba SSH dormente continua dormente: ela conecta no clique ou `Enter` no painel, ou pelo `start-pane`.
- Aba desconhecida responde `unknown-tab`, e aba já ativa responde sucesso sem efeito.
- `busy` vale como em toda ação (ADR-0107 §4).

### 3. A janela do sistema, nunca

Nenhuma ação remota chama `focus_window`, desminimiza nem traz janela para a frente, nem no `activate-tab`, nem no destino de `move-*-to-window`. Só a aba ativa *dentro* da janela muda. O painel focado dentro da aba também não é mudado pelo celular, porque não há ação para isso. A proteção de foco do Windows recusaria o roubo de foco de todo modo, e roubar o teclado de outro programa é um dano maior que trocar a aba dentro do Porecatu.

### 4. Gerenciador de tarefas

Ativar ou criar aba por ação remota, com o gerenciador de tarefas aberto naquela janela, volta ao terminal, como qualquer ativação ou criação local ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md)). A ação remota passa a rodar dentro da mesma checagem única (`task_manager_guard_begin`/`end`) da janela afetada, e não por uma chamada espalhada. Isso revê a frase do ADR-0107 §2 de que o gerenciador "não fecha por causa de ação remota".

### 5. O que não muda

A confirmação continua indo ao celular, sem diálogo no desktop. `busy`, `last-tab` e as perguntas do backend no desktop seguem o ADR-0107 §3 a §6. O celular continua sem mudar o tamanho do PTY (ADR-0105).

### 6. Protocolo

A mudança começa no [porecatu-protocol](https://github.com/porecatu/protocol), como manda o [ADR-0103](0103-acesso-remoto-tres-partes-e-protocolo-externo.md) §5. Lá saem o ADR-0010 do protocolo e a especificação 0.4, com a versão de fio ainda `[0, 1]`, porque não congelou. A 0.4 traz:

- a regra nova da §7 e a ação `activate-tab`;
- três correções de texto achadas na implementação deste lado: o `close-pane` no último painel de aba fixada, a posição do `new-group` e os códigos de casos que a 0.3 não definia.

O `porecatu-remote` passa a apontar para o commit da 0.4. O Relay não muda, porque não lê ações: elas viajam cifradas. O app Remote precisa implementar `activate-tab` no repositório dele.

> **Nota de implementação (2026-11-17).** O `porecatu-remote` aponta para `0909e7ca88e3c180198e8ce7246445c4b0d9fbd1`, o merge da especificação 0.4 (commits `f3715bb` e `d8618ef`) na `main` do protocolo, o mesmo commit do Relay ([ADR-0103](0103-acesso-remoto-tres-partes-e-protocolo-externo.md), nota do §6). O app Remote já manda `activate-tab`. O app Remote ainda não implementa `activate-tab` nem a 0.4. A §4 se cumpre por `task_manager_guard_end_remote`, em `app_task_manager.rs`: o fim da checagem única do ADR-0095 para o que não é evento de janela. Não houve clique de aba, então só a contagem e a aba ativa decidem (`should_leave`), e a ação remota que ativa ou cria aba fecha o gerenciador da janela afetada. Os pontos de comportamento que a 0.3 deixava errados ou abertos estão na nota do [ADR-0107](0107-acoes-remotas-sobre-o-workspace.md) §1.

## Alternativas consideradas

### Manter o ADR-0107 §2

Protege quem digita no desktop enquanto o celular age. Mas exige um desvio "não ativar" em cada ação e deixa o celular sem jeito de deixar pronta a aba em que se trabalhou. Recusada pelo dono.

### Só `activate-tab`, com as ações sem ativar

O celular trocaria a aba ativa só quando pedisse explicitamente. As ações continuariam diferentes do gesto local que dizem reproduzir. Recusada.

### Focar também a janela do sistema

É o gesto local por inteiro, mas tira o teclado de outro programa, e o Windows o recusa de forma inconsistente. Recusada.

## Consequências

### Positivas

- Cada ação remota faz exatamente o que o gesto local faz, sem exceção de ativação.
- O celular deixa pronta no desktop a aba em que se trabalhou.

### Negativas

- Quem usa o desktop ao mesmo tempo que o celular pode ver a aba ativa mudar debaixo do teclado. As teclas seguintes vão para a aba nova.
- Uma mudança de fio: o app Remote precisa acompanhar (`activate-tab`).

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Digitar na aba errada no desktop porque o celular trocou a aba ativa | Baixa (uso típico de uma pessoa só) | Médio | A janela do sistema nunca muda (§3). `busy` protege quem está no meio de um gesto. O botão de aparelhos conectados ([ADR-0108](0108-superficies-de-ui-do-acesso-remoto.md)) mostra que há um celular conectado |
| A ativação remota fechar o gerenciador de tarefas de quem o está usando | Baixa | Baixo | É a regra local do ADR-0095. O histórico do gerenciador sobrevive a fechá-lo |
