# ADR-0013 — Mouse, seleção e clipboard

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** ADR-0002, ADR-0008, ADR-0012, PRD-001, PRD-005

## Contexto

O critério de saída da F1 no [roadmap](../roadmap.md) exige `vim`, `htop` e `fzf` **usáveis** nas três plataformas. Os três esperam receber eventos de mouse: o `htop` mata processo por clique, o `fzf` seleciona resultado por clique, o `vim` posiciona cursor. Nenhum documento aprovado do projeto menciona reporte de mouse ao programa em execução. A F1 não fecha sem essa decisão.

O mesmo bloco de requisitos tem um segundo furo. "Seleção com mouse e cópia" é **um bullet** do roadmap: nenhum RF, nenhum cenário de aceite, nenhuma decisão sobre o que duplo clique faz. Enquanto isso, o PRD-005 RF-5.14 configura a *cor* da seleção — o projeto especifica como a seleção é pintada antes de especificar o que ela é.

As duas lacunas são a mesma lacuna, porque **o mouse é disputado**. Quando um programa pede eventos de mouse, ele os quer todos; arrastar para selecionar texto deixa de funcionar, porque o arraste virou input do programa. Todo emulador precisa de uma regra para esse conflito, e a regra precisa ser previsível — o usuário não pode ficar sem saber se o arraste vai selecionar ou clicar dentro do `htop`.

Terceiro item, de natureza diferente: **OSC 52**, a sequência com que um programa lê ou escreve o clipboard do sistema. Ela existe por um caso de uso legítimo e frequente — copiar de dentro de um `tmux` ou `nvim` rodando por SSH, onde o programa não tem acesso ao clipboard da máquina local. E tem um lado ruim óbvio: se a leitura for honrada, qualquer processo, inclusive num host remoto comprometido, lê o que o usuário copiou. Senha de gerenciador de senhas inclusa. Isso não é lacuna de conveniência, é decisão de segurança, e omissão não é resposta.

Por fim, `[terminal.scrollback]` configura `lines` e `scroll_multiplier` mas nada diz sobre **como se rola**: não há ação de teclado para o scrollback, nem decisão sobre o que a roda do mouse faz quando `vim` está aberto e não existe scrollback para rolar.

## Decisão

Três regras, uma por disputa:

1. **Mouse:** o programa tem prioridade quando pede; `Shift` devolve o controle ao usuário.
2. **Seleção:** usar o modelo do motor VT, não reimplementar.
3. **Clipboard:** OSC 52 escreve, não lê.

### Reporte de mouse

Modos honrados, lidos do `TermMode` do `alacritty_terminal` — **sem manter estado paralelo**, porque duas cópias da mesma verdade divergem:

| Modo | O que é |
|---|---|
| 1000 | reporte de clique (pressiona e solta) |
| 1002 | reporte de clique mais arraste com botão pressionado |
| 1003 | reporte de qualquer movimento, com ou sem botão |
| 1006 | encoding SGR — **preferido**, é o único sem limite de 223 colunas |
| X10 (legado) | encoding de fallback, quando o programa não negocia 1006 |
| 1005 | ignorado (encoding UTF-8, obsoleto e ambíguo) |

**A regra de conflito, que evita a classe inteira de bug:**

```
clique/arraste na área do terminal
  |
  +- Shift pressionado?      -> seleção local, SEMPRE
  +- programa pediu o mouse? -> evento codificado e escrito no PTY
  +- nenhum dos dois         -> seleção local
```

`Shift` sobrepõe o programa, sem exceção. É a convenção do xterm, e é o que torna possível copiar uma linha de dentro do `htop` — sem ela, o usuário precisaria sair do programa para copiar sua saída. Simetria deliberada com o [ADR-0008](0008-teclas-e-roteamento-de-input.md): lá, um binding que casa nunca cai para o terminal; aqui, `Shift` nunca cai para o programa.

A barra de abas não participa disso. Ela nunca repassa mouse ao terminal — o clique do meio numa aba fecha a aba (RF-1.2) e não há ambiguidade, porque a área é outra. Só a área de conteúdo do terminal está em disputa.

### Seleção

Usar o tipo `Selection` do motor ([ADR-0002](0002-motor-vte.md)), que já modela os quatro modos, em vez de escrever geometria de seleção própria:

| Gesto | Modo | Resultado |
|---|---|---|
| Arraste | `Simple` | caractere a caractere |
| Duplo clique | `Semantic` | palavra, com separadores configuráveis |
| Triplo clique | `Lines` | linha lógica inteira |
| `Alt` + arraste | `Block` | retangular |

Regras de cópia, ambas com motivo prático:

- **Espaço em branco à direita é removido.** O grid é preenchido com células vazias até a borda; copiar sem cortar traz dezenas de espaços invisíveis atrás de cada linha.
- **Linha marcada com `WRAPLINE` é remontada sem quebra.** Uma linha que só foi quebrada porque a janela é estreita não é duas linhas. Copiar um caminho longo e colar não pode produzir dois comandos.

Ciclo de vida: a seleção é limpa por input de teclado e por escrita do programa que toque a região selecionada; **rolagem pura preserva** a seleção, porque rolar para conferir o que se está selecionando é uso normal.

`copy_on_select` existe como chave, default `false`. Ligado por default surpreenderia quem seleciona só para ler, sobrescrevendo o clipboard sem pedido.

**Limitação conhecida do v1:** a seleção PRIMARY do X11 e do Wayland — colar com o botão do meio — não é implementada. O gesto do botão do meio na barra de abas já está tomado pelo fechamento de aba (RF-1.2), a integração exige tratamento próprio por plataforma, e o ganho não cabe no orçamento da F1. Fica registrado como limitação, não como bug, com chave reservada no arquivo de exemplo.

### Clipboard

Crate **`arboard`**, encapsulado no ponto onde a UI trata `clipboard.copy` e `clipboard.paste` (ações já definidas no ADR-0008).

**OSC 52 — escrita permitida, leitura negada:**

| Direção | Default | Chave |
|---|---|---|
| Escrita (programa → clipboard) | permitida | `osc52_write = true` |
| Leitura (clipboard → programa) | **negada** | `osc52_read = false` |

A escrita atende o caso de uso real: copiar de dentro do `tmux` ou do `nvim` sobre SSH. A leitura é negada porque um processo remoto lendo o clipboard local é vetor de exfiltração — o usuário acabou de copiar uma senha e não tem como saber que ela foi lida. A chave existe e pode ser ligada por quem entender o risco; o arquivo de exemplo diz qual é o risco, em vez de apresentar a chave como preferência neutra.

A escrita tem **teto de tamanho no payload**. Sem limite, uma sequência absurda vinda de saída não confiável escreve megabytes no clipboard do usuário.

### Rolagem

Ações novas, que entram no catálogo fechado do ADR-0008: `scrollback.line_up`, `scrollback.line_down`, `scrollback.page_up`, `scrollback.page_down`, `scrollback.to_top`, `scrollback.to_bottom`. Defaults `shift+pageup` e `shift+pagedown` — `Shift` de novo como o modificador que fala com o emulador, não com o programa.

Comportamento:

- `scroll_on_output = false` — saída em segundo plano não arranca o usuário de onde ele estava lendo.
- `scroll_on_input = true` — digitar volta ao final. É onde o prompt está.
- **Tela alternativa** (`1049`): não existe scrollback nela, e as ações de rolagem não fazem nada. Com `alternate_scroll = true` (default), a roda do mouse é traduzida em setas para cima e para baixo — é o que faz `less`, `man` e `git log` rolarem com a roda, e sem isso a roda parece quebrada nos programas onde o usuário mais quer usá-la.

## Alternativas consideradas

### Não repassar mouse ao programa

Seleção sempre local, sem modificador, comportamento uniforme e simples de explicar.

Descartada por contrariar o critério de saída da F1: `htop` e `fzf` ficariam parcialmente inúteis, e o usuário não teria como saber por quê — o programa desenha uma interface que responde a clique e o clique não chega. Uniformidade que quebra os programas de referência não é simplicidade, é defeito.

### Modificador diferente de `Shift` para forçar seleção local

`Ctrl` ou `Alt` estariam livres em alguns contextos.

Descartada por convenção estabelecida: `Shift` é o que xterm, Alacritty, Kitty, WezTerm e gnome-terminal usam. Memória muscular de décadas, e `Alt` já foi tomado pela seleção retangular.

### Escrever a lógica de seleção por conta própria

Controle total sobre semântica de palavra, comportamento de wrap e seleção retangular.

Descartada pelo mesmo raciocínio do ADR-0002: o motor já modela os quatro modos, incluindo os casos chatos — largura dupla de CJK, células vazias no fim da linha, seleção que atravessa o limite do scrollback. Reimplementar é assumir esses bugs de novo, um por um, para chegar onde já se está.

### `copypasta` em vez de `arboard`

É o crate do Alacritty, com Wayland tratado via `smithay-clipboard`, e portanto testado no cenário exato.

Não descartada, apenas não escolhida: entra como plano B na tabela de riscos. `arboard` foi preferido por manutenção mais ativa e API menor, mas o caminho Wayland é justamente o ponto a confirmar na F1 — se ele exigir handle do display do `winit` de um jeito que atrapalhe, a troca é local ao ponto de encapsulamento.

### OSC 52 totalmente desligado por default

Postura mais conservadora: nenhuma sequência mexe no clipboard sem o usuário ligar.

Descartada porque quebra a expectativa de quem usa `tmux` ou `nvim` sobre SSH — que é público-alvo direto do produto — em troca de proteção contra um risco que a escrita não tem. Escrever no clipboard do usuário é chato; **ler** o clipboard do usuário é que é perigoso, e é a leitura que fica negada.

### OSC 52 com leitura permitida

Compatibilidade máxima; alguns fluxos de `tmux` usam leitura.

Descartada por segurança, e é a única alternativa deste ADR descartada por esse motivo. O ganho é marginal e o dano potencial é vazamento de credencial para um host que o usuário não controla.

## Consequências

### Positivas

- `htop`, `vim` e `fzf` respondem ao mouse; o critério de saída da F1 fica alcançável.
- Uma regra única e memorizável para o conflito: `Shift` fala com o emulador.
- Seleção herda os casos difíceis já resolvidos pelo motor (CJK, wrap, borda do scrollback).
- Copiar de `tmux`/`nvim` remoto funciona sem expor o clipboard à leitura.
- Roda do mouse funciona em `less` e `man`, onde o usuário mais espera que funcione.

### Negativas

- Usuário que não conheça a convenção do `Shift` vai achar que a seleção quebrou dentro do `htop`. Precisa estar na documentação de usuário da F6.
- Sem seleção PRIMARY, quem usa Linux perde o colar de botão do meio — hábito arraigado.
- Duas fontes de verdade para "o que o mouse faz agora": o modo do programa e a tecla modificadora. Inerente ao problema, não à decisão.
- `copy_on_select = false` desagrada quem vem de emulador onde o default é o contrário.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Caminho Wayland do `arboard` exigir handle do `winit` ou não funcionar | Média | Médio | Uso encapsulado num ponto; `copypasta` como plano B; verificar na F1, não na F6 |
| Encoding X10 quebrar acima de 223 colunas | Alta em terminal largo | Baixo | SGR 1006 é o preferido; X10 só quando o programa não negocia — e nesse caso o limite é do programa |
| OSC 52 usado para escrever lixo no clipboard | Média | Baixo | Teto de tamanho no payload; chave para desligar |
| Usuário ligar `osc52_read` sem entender o risco | Baixa | Alto | Comentário no arquivo de exemplo explica o vetor, não só a sintaxe |
| Seleção limpa cedo demais por saída de programa em segundo plano | Média | Baixo | Só limpa quando a escrita toca a região selecionada; rolagem preserva |
| `alternate_scroll` atrapalhar programa que já trate a roda | Baixa | Baixo | Desligável na config; default segue o comportamento do xterm |
