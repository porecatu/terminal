# ADR-0099 — Ajustes dos gráficos e da lista do gerenciador de tarefas: ponto além da borda, folga no topo da escala e respiro vertical da lista

**Status:** Aceito — aval visual do dono dado em 2026-11-11
**Data:** 2026-11-11
**Relacionados:** [ADR-0094](0094-polilinha-e-area-no-render.md), [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md), [ADR-0096](0096-anatomia-do-gerenciador-de-tarefas.md), [ADR-0098](0098-monitoramento-em-segundo-plano-do-gerenciador.md), [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md)
**Supersedes:** ADR-0095 §1 e §6 (**parcial**: o histórico guarda um ponto além do período; as escalas automáticas ganham folga), ADR-0096 §2 e §4 (**parcial**: o topo da escala e o respiro vertical da lista)

## Contexto

O dono do produto revisou o gerenciador em tela e pediu três ajustes:

1. Os gráficos não desenham até o fim da grade: o lado esquerdo às vezes fica com um vazio, e o vazio muda com a largura da janela.
2. O gráfico de memória se autodimensiona, o que está certo, mas quando o valor chega perto do topo a linha e a área cobrem o rótulo do topo da escala (canto superior esquerdo).
3. A lista de processos não tem respiro vertical entre as duas réguas (a de baixo do cabeçalho e a de cima do rodapé) e as linhas.

**Causa do 1.** `TaskManagerHistory::trim` descartava todo ponto mais velho que `history_seconds` antes do último ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §1). Com amostras a cada ~1 s, que nunca caem exatamente em `history_seconds`, o ponto mais antigo guardado ficava a, por exemplo, 59,2 s do último, e a linha começava `x = direita − 59,2 / 60 × largura`, uma amostra depois da borda. Como a falta é uma fração da largura, o vazio cresce com a janela. `chart::series` já mantinha **um** ponto além da borda esquerda para a linha entrar por ela ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §6), mas o histórico já o tinha jogado fora.

## Decisão

### 1. O histórico guarda um ponto além do período

`trim` passa a descartar o ponto da frente só enquanto o **segundo** também for mais velho que `history_seconds` antes do último. Sobra, no máximo, **um** ponto além do período: o mais novo dos velhos. `chart::series` o usa para a linha entrar pela borda esquerda e o recorte do interior da moldura corta o excesso; `chart::visible_max` já ignora o que passa do período, então a escala não muda por causa dele. Diminuir `history_seconds` continua cortando o histórico na hora (RF-24.27). Nada é gravado (RF-24.12).

### 2. Folga de 1,2× no topo das escalas automáticas

`chart::scale_top` de memória e disco passa a arredondar `maior valor visível × HEADROOM`, com `HEADROOM = 1,2`, para o próximo `1, 2 ou 5 × 10ⁿ` ([ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md) §6). O pico nunca passa de 1/1,2 (cerca de 83%) da altura, então fica abaixo do rótulo do canto superior esquerdo. O rótulo continua dizendo o topo **real** da escala. A CPU, fixa em 0–100%, não muda. Valor adimensional: não ganha chave em `[appearance.task_manager]`; vive como constante em `chart.rs`, citando este ADR.

Efeito colateral aceito: um pico exatamente num degrau (1 GB) sobe para o degrau seguinte (2 GB) em vez de encostar no topo. Disco leva a mesma folga porque usa a mesma escala e teria o mesmo defeito.

### 3. Respiro vertical da lista

O corpo da lista (entre a régua do cabeçalho e a régua do rodapé) reserva `row_padding_x` (**8**) em cima e embaixo; as linhas vivem no retângulo `ListLayout::rows`, o corpo menos esse respiro. O respiro nunca passa de um terço do corpo para cada lado, para uma lista curta continuar mostrando linhas. `visible_rows` conta as linhas **inteiras** que cabem em `rows`, e `row_rect`/`row_at` partem de `rows`; o recorte do pintor continua sendo o corpo, para o fundo arredondado de seleção e hover nunca ser cortado. **Nenhum valor novo**: o 8 é o recuo horizontal das mesmas linhas.

Com a janela de referência (600 de altura), o corpo tem 506, as linhas 490 e cabem **17** linhas inteiras (eram 18).

## Alternativas consideradas

### Esticar o ponto mais antigo até a borda
Fabricaria um valor que não foi medido. Guardar o ponto real que já existia, além da borda, desenha a inclinação certa.

### Faixa fixa em pixels no topo, em vez de folga na escala
Deixaria o rótulo do topo dizendo um valor que não é o topo desenhado. A folga na escala mantém o rótulo verdadeiro e reaproveita `nice_top`.

### Folga só na memória
O disco usa a mesma escala e sofre do mesmo defeito. Se o dono preferir só a memória, a mudança é restringir o `match` de `scale_top`.

### Chave `[appearance.task_manager] list_padding_y`
Valor novo a documentar nos três lados (`VALORES`, exemplo, especificação) sem necessidade: o recuo horizontal já é o que o dono vê como "o respiro" das linhas.

## Consequências

- Os gráficos preenchem a largura inteira em regime, em qualquer tamanho de janela. Nos primeiros `history_seconds` de dados o vazio à esquerda continua, e é correto: não há dado mais antigo.
- O histórico guarda um ponto a mais por janela. Custo desprezível.
- A lista mostra uma linha a menos na altura de referência.
- Aval visual do dono dado em 2026-11-11, no Windows.
