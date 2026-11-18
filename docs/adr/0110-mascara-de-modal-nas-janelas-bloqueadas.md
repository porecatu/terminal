# ADR-0110 — Máscara de modal nas janelas bloqueadas

**Status:** Aceito — implementado em 2026-11-18 (`porecatu-ui/src/modal_mask.rs`); verificado só por teste automatizado, aval visual do dono pendente
**Data:** 2026-11-18
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0022](0022-animacao-de-interface.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0059](0059-janela-de-configuracoes.md), [ADR-0064](0064-janela-de-configuracoes-modal.md), [ADR-0080](0080-editor-de-perfil-em-janela-propria.md)
**Supersedes:** ADR-0022, "Consumidores no v1" (**parcial**: a lista fechada ganha o fade da máscara)

## Contexto

Duas janelas do app são modais sobre outras: a tela de configurações sobre as janelas de terminal ([ADR-0064](0064-janela-de-configuracoes-modal.md)) e o editor de perfil sobre a tela de configurações ([ADR-0080](0080-editor-de-perfil-em-janela-propria.md)). No Windows a janela de trás fica desabilitada pelo SO, mas nada nela diz isso: ela continua com a mesma aparência, e só um clique (que faz a modal piscar) mostra que ela não responde. O dono do produto pediu uma máscara escura e semitransparente sobre a janela bloqueada, para deixar claro que a modal precisa ser fechada antes.

Os diálogos dentro da janela ([ADR-0014](0014-superficie-de-aviso-e-dialogo.md)) já têm essa máscara: o diálogo de confirmação pinta a janela inteira com `[dialog] overlay` (`#06070973` no tema escuro, `#4c4f6966` no claro) antes do modal (espec. §2.15).

## Decisão

**Uma janela bloqueada por uma janela modal do app é coberta pela cor `[dialog] overlay`, a mesma máscara dos diálogos, que entra e sai em fade.**

### 1. Onde

- **Janelas de terminal**, enquanto a tela de configurações está aberta — **só no Windows**, onde o `EnableWindow` do SO as bloqueia (ADR-0064 §2). Nas outras plataformas elas continuam recebendo input, e a máscara diria que não recebem; `modal_mask::terminal_windows_blocked` decide.
- **A tela de configurações**, enquanto o editor de perfil está aberto, em todas as plataformas: lá a modalidade é do SO no Windows e lógica nas outras (ADR-0080 §2), e em ambos os casos a tela não responde.
- Os **diálogos dentro da janela** ficam como estão: já têm a máscara.

### 2. Cor: a do diálogo, sem chave nova

A máscara usa `pal.dialog_overlay`, a cor `[dialog] overlay` dos temas. Nenhuma cor nova: todo modal do app escurece igual, e um tema personalizado leva a máscara junto. Decisão do dono, entre reusar a cor, trocar a dos diálogos para preto a 40% e criar uma chave própria.

### 3. Camada e ordem

Um `Quad` do tamanho da janela, na camada `Modal` do [ADR-0018](0018-composicao-de-frame.md), **por cima de tudo** o que a janela pinta — inclusive de um diálogo próprio dela aberto antes, que também está bloqueado — e **abaixo da borda de 1px** da janela, que continua a última primitiva. Nenhuma camada nova.

### 4. Fade

A máscara entra e sai variando a opacidade da cor de 0 a 1, linear, em `[appearance.window] animation_collapse_ms` (150 ms), a transição curta do app; com `animations = false`, troca de uma vez. Nenhuma duração nova. Um fade invertido no meio continua de onde a máscara está, sem salto. A primeira sincronização de uma janela não anima: uma janela de terminal criada com a tela aberta já nasce com a máscara.

`ModalMask` é estado puro, com o `Instant` recebido de fora, como o `AnimationClock`: a janela de terminal e a tela o sincronizam no redraw (`advance`) com o alvo do momento, e enquanto o fade corre o redraw encadeia o próximo quadro — e, na janela de terminal, `next_deadline` entra no `WaitUntil`. Sem fade, nenhum quadro a mais ([ADR-0007](0007-modelo-de-threading.md)). Abrir e fechar a tela pedem um quadro a toda janela de terminal; abrir e fechar o editor, um à tela.

Isso acrescenta um consumidor à lista fechada do [ADR-0022](0022-animacao-de-interface.md), que tem começo e fim como os outros.

## Alternativas consideradas

### Cor própria, preto a 40%

Foi a primeira proposta, feita antes de se notar que os diálogos já tinham máscara. Recusada pelo dono depois disso: duas cores para a mesma ideia divergiriam no primeiro tema personalizado.

### Máscara nas janelas de terminal também fora do Windows

Fora do Windows a tela de configurações não desabilita as janelas de terminal (ADR-0064). Escurecê-las sem bloqueá-las seria dizer uma coisa e fazer outra. Tornar a modalidade lógica nelas, como o editor faz com a tela, é outra decisão, sem pedido.

### Sem fade

Mais simples, e é o que o diálogo faz com o overlay dele (espec. §2.15, "sem animação no overlay"). Recusada pelo dono, que escolheu o fade curto.

## Consequências

### Positivas

- A janela bloqueada se lê como bloqueada sem precisar de um clique.
- Nenhuma cor, duração ou camada nova; o fade usa o mesmo mecanismo das outras animações.

### Negativas

- Um consumidor a mais na lista de animações do ADR-0022.
- Uma janela de terminal com diálogo próprio aberto, com a tela aberta, fica com as duas máscaras somadas.

### Riscos

- Verificado só por teste automatizado. A verificação ao vivo no Windows (tela aberta sobre várias janelas, editor sobre a tela, fade com e sem `animations`) e o aval visual do dono ficam pendentes; macOS e Linux, como o resto, são dívida.
