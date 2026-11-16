# ADR-0105 — Espelhamento da grade e input remoto: snapshot do motor no ritmo do relógio da UI, diff fora da main thread, teclas pelo codificador local

**Status:** Aceito em 2026-11-16 — não implementado
**Data:** 2026-11-15
**Relacionados:** [PRD-025](../prd/prd-025-acesso-remoto.md), [ADR-0002](0002-motor-vte.md), [ADR-0007](0007-modelo-de-threading.md), [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0013](0013-mouse-selecao-e-clipboard.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0053](0053-paineis-divididos.md), [ADR-0067](0067-temas-em-arquivos-proprios.md), [ADR-0090](0090-transferencia-de-abas-entre-janelas.md), [ADR-0104](0104-crate-porecatu-remote.md)

## Contexto

O dono decidiu que o celular recebe **a grade do desktop** — células já resolvidas pelo motor — e não roda emulador próprio, e que **o desktop dita o tamanho** do terminal. O protocolo já fixa a forma do fio ([03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md) §4 a §6): árvore inteira do workspace a cada mudança, `grid-snapshot` e `grid-diff` por painel assinado com confirmação (`grid-ack`) e janela de 2, scrollback por página sob demanda, input semântico.

Falta decidir **como o host produz isso** sem ferir o que o projeto já garante:

- a main thread não faz I/O nem trabalho caro ([ADR-0007](0007-modelo-de-threading.md));
- painel ocioso custa zero e painel invisível não gera quadro (princípio 4 do PRD-000);
- `porecatu-term` não conhece `Config` nem GUI, e `porecatu-remote` não conhece `porecatu-term` ([ADR-0104](0104-crate-porecatu-remote.md));
- há um codificador de teclas só, puro, em `porecatu-term/src/keys.rs` (`encode_key`, `encode_text`, `wrap_paste`), e duplicá-lo é o tipo de divergência que o projeto evita ("fórmula de geometria nunca copiada", aplicada a bytes).

## Decisão

**A main thread tira um snapshot da tela viva de cada painel assinado e sujo, no máximo 30 vezes por segundo, pelo relógio da UI, converte para linhas do protocolo e entrega a `porecatu-remote`, que guarda só o mais recente, calcula o diff e aplica o controle de fluxo. Input remoto vira `TermKey`/texto e passa pelo mesmo codificador do teclado local.**

### 1. Ids estáveis do protocolo

`porecatu-ui` mantém, em `App`, um mapa **do processo** entre os ids do protocolo (`window-id`, `group-id`, `tab-id`, `pane-id`, inteiros de contadores monotônicos) e os ids internos (`WindowId`, `GroupId`/`TabId` por workspace, `PaneId` por aba).

- O id do protocolo nasce quando o objeto aparece pela primeira vez numa árvore enviada, e **não muda** quando a aba ou o grupo mudam de janela: `App::transfer` ([ADR-0090](0090-transferencia-de-abas-entre-janelas.md)), que já re-chaveia tudo que é chaveado por `TabId` fora do runtime, re-chaveia também esse mapa. Estado novo chaveado por `TabId` que não entra em `transfer` vira órfão — é a regra que já está no CLAUDE.md.
- Os ids nunca são reaproveitados. Fechar o acesso remoto não zera os contadores; reiniciar o app, sim. Por isso o host gera, ao iniciar o processo, o `host_instance` de 16 bytes aleatórios que vai no `welcome`: é o que diz ao celular que os ids guardados de uma sessão anterior não valem mais ([03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md) §2 e §3).

### 2. Árvore do workspace

- Qualquer mudança que alteraria o que a árvore do protocolo descreve (aba criada, fechada, movida, renomeada, título por OSC, estado do painel, atividade, campainha, grupo, foco do desktop) **marca a árvore suja**, por processo.
- Com algum cliente conectado e a árvore suja, a main thread monta a árvore **inteira** no próximo prazo do relógio (teto de 4 por segundo, `ControlFlow::WaitUntil`, sem thread de timer) e a entrega a `porecatu-remote`, que a manda a todos os clientes.
- Sem cliente conectado, nada é montado: marcar sujo é escrever um `bool`.

### 3. A grade: quem faz o quê

```
thread de leitura do PTY
  └─ Wakeup::TabDirty { window, tab, pane }       (já existe)
       └─ main thread: painel assinado? marca "sujo para o remoto"
            └─ prazo do espelhamento (≤ 30/s, WaitUntil)
                 └─ Terminal::snapshot_live_into(&mut buf)   (lock curto, como no render)
                      └─ ui converte buf -> RemoteFrame (linhas do protocolo, estilos, cursor)
                           └─ RemoteHost::send(Command::Frame { pane, frame })   (try_send)
                                └─ porecatu-remote: guarda o último por painel,
                                   diff contra o último ENVIADO, janela de 2 sem ack,
                                   CBOR, Noise, relay
```

- **Snapshot da tela viva, não da vista.** `Terminal::snapshot_into` devolve o que o desktop está mostrando, incluindo o deslocamento de scrollback em que o usuário do desktop parou e a seleção. O celular precisa do **fundo vivo**, independente da rolagem do desktop. `porecatu-term` ganha `snapshot_live_into` (a tela no fundo do scrollback, sem seleção, sem ocorrências de busca) e `history_into(offset, count, out)` para as páginas de scrollback (§5). As duas são acréscimos à API de `Terminal`, sem mudar `snapshot_into`.
- **A conversão é de `porecatu-ui`.** Ela conhece `GridSnapshot` (de `porecatu-term`) e os tipos do protocolo (re-exportados por `porecatu-remote`), e é o único lugar que pode conhecer os dois. É uma função pura, testada com snapshots montados à mão, que aplica as regras do fio ([03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md) §5.2): spans de largura uniforme, grafema de vários code points sozinho num **span de grafema** (`text` como array de um elemento), cores **não resolvidas** (o `TermColor` já é não resolvido — vira `null`/indexada/direta sem passar pela paleta).
- **O diff e o controle de fluxo são de `porecatu-remote`.** Ele guarda, por assinatura, a última grade **enviada** e o último quadro **recebido** da UI. Com a janela livre, manda snapshot ou diff; com a janela cheia, só substitui o último quadro recebido. Quando chega um `grid-ack`, o diff sai de "último enviado" para "último recebido". A main thread nunca espera por isso e nunca acumula fila.
- **Custo zero quando ocioso.** Painel sem assinatura não é lido. Painel assinado sem `TabDirty` não é lido. O prazo do espelhamento só entra no `WaitUntil` enquanto há painel sujo para o remoto.
- **Sem quadro no desktop.** Sujeira para o remoto **não** chama `request_redraw`. Um painel numa aba invisível no desktop e assinado pelo celular é lido pelo espelhamento e não gera frame de GPU.

### 4. Tamanho

O desktop dita o tamanho. Nenhuma mensagem do cliente chega a `Terminal::resize`. Resize no desktop (janela, divisão, zoom de fonte) muda `cols`/`rows` do snapshot, e `porecatu-remote` manda `grid-snapshot` em vez de diff. Entrar ou sair da tela alternativa também.

### 5. Scrollback

`scrollback-request` chega como `RemoteEvent`; a main thread chama `Terminal::history_into` para até 500 linhas, converte e devolve como `scrollback-page`.

`grid-snapshot`, `grid-diff` e `scrollback-page` levam, além das linhas, o tamanho do histórico (`history`) e o contador de linhas já descartadas do topo dele (`dropped`, monotônico por painel, zerado só quando o histórico é limpo, caso em que vai um `grid-snapshot`), que dão a cada linha de histórico um índice absoluto estável ([03 — Mensagens](https://github.com/porecatu/protocol/blob/main/docs/especificacao/03-mensagens.md) §5.3 e §5.5). Os dois números saem do motor junto com a tela e a página; como `porecatu-term` os obtém fica para a implementação. É o único caminho em que a main thread lê o motor por pedido do cliente; o teto de 500 linhas mantém o lock no tamanho de um snapshot de tela grande. **Todo pedido recebe resposta**: os que chegam mais rápido que um por prazo do relógio esperam numa fila e são atendidos um por prazo, na ordem de chegada (decidido pelo dono em 2026-11-16; a especificação não prevê pedido sem resposta). O app de referência mantém no máximo dois pedidos em voo, o que segura o tamanho da fila.

### 6. Tema

`theme` sai de `Config` já resolvida: `fg`, `bg`, cursor, seleção e as 16 cores do tema ativo ([ADR-0067](0067-temas-em-arquivos-proprios.md)), mais a paleta de cores de grupo. Recarga que muda o tema manda `theme` de novo. Cor alterada por OSC 4/10/11 numa sessão de terminal não é espelhada no v1 (o celular pinta com o tema).

### 7. Input

- `key` vira `TermKey` (as teclas nomeadas do protocolo são exatamente as variantes de `porecatu_term::TermKey`, F1–F12 como `Function(n)`; os bits de `mods` Ctrl, Alt, Shift e Meta viram `ctrl`, `alt`, `shift` e `super_` de `Modifiers`) ou, com caractere, `encode_ctrl_char`/`encode_text` com `Modifiers`; `text-input` vira `encode_text`; `paste` vira `wrap_paste` com os `TermModes` atuais do painel. Os bytes vão por `Terminal::write`, como no teclado local.
- **Nenhum atalho do Porecatu** é consultado: o input remoto entra no passo 3 da cadeia do [ADR-0008](0008-teclas-e-roteamento-de-input.md) direto, sem passar pela tabela de ações nem pelos modos de captura da janela. Um diálogo aberto no desktop não engole tecla do celular, e vice-versa.
- `scroll_on_input` vale: digitar no celular leva o desktop ao fundo daquele painel, como digitar no desktop.
- Input para painel não assinado ou fora de `running` é recusado com o código do protocolo (`error` com `not-subscribed` ou `pane-not-running` e o `pane`); o gerenciador de tarefas aberto no desktop **não** bloqueia input remoto (a regra dele é sobre o teclado da janela).
- Mouse remoto não existe no v1.

### 8. Painéis especiais

- **Não iniciado** ([ADR-0037](0037-aba-nao-iniciada.md)): não tem `TabRuntime`. Assinar devolve `grid-snapshot` vazio com o tamanho da última grade conhecida (ou 80×24), decidido por `is_not_started()` **antes** de qualquer acesso ao runtime — a armadilha já anotada no CLAUDE.md.
- **SSH reconectando**: o motor sobrevive à reconexão (`Terminal::reattach`), e a assinatura continua; o estado do painel aparece na árvore.

## Alternativas consideradas

### Repassar os bytes do PTY e rodar um emulador no celular

Recusada pelo dono. Exigiria reproduzir o estado inicial (o motor do desktop não grava bytes, só a grade), manteria dois motores que divergem em casos de borda, e daria ao celular o poder de "ver" um tamanho diferente do PTY.

### Diff na main thread

Mais simples de escrever, mas o diff depende de confirmações que chegam pela rede, e manter o "último enviado" por cliente na main thread traz estado de rede para `App`. Em `porecatu-remote` ele é testável sem GUI.

### Thread de espelhamento com acesso direto ao `Term`

Uma thread de `porecatu-ui` lendo o `Arc<Mutex<Term>>` no próprio ritmo tiraria até o snapshot da main thread. Mas `Terminal` não expõe o motor, e expor só para isso quebraria a fronteira da seção 4 da arquitetura. O snapshot na main thread custa o mesmo que o de um frame, e só acontece para painel assinado e sujo.

### Celular assumindo o tamanho enquanto interage

Recusada pelo dono: bagunça o que está na tela do desktop.

## Consequências

### Positivas

- Um motor, um codificador de teclas, uma regra de tamanho: o que o celular vê é o que existe.
- Rede lenta não acumula nada: o celular recebe o estado mais recente, não a história.
- Painel ocioso e acesso sem assinatura não custam leitura de motor.

### Negativas

- Duas funções novas em `porecatu-term` (`snapshot_live_into`, `history_into`), testadas com o motor real.
- Um mapa de ids do protocolo a mais em `App`, que `transfer` tem de manter.
- Grade larga no celular depende de zoom e rolagem horizontal.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Esquecer de re-chavear o mapa de ids numa transferência | Média | Médio | Teste de `transfer` que confere o id do protocolo antes e depois; o celular só perde a aba de vista, nunca recebe dado de outra |
| Snapshot de 30/s em vários painéis pesar na main thread | Baixa | Médio | Teto de assinaturas (4 por cliente); só painel sujo; medir com `cargo build` rolando em 4 painéis assinados antes de fechar a etapa |
| Conversão quebrar a regra de grafemas do fio | Média | Médio | Vetores de teste do `porecatu-protocol` rodando contra a saída da função pura |
