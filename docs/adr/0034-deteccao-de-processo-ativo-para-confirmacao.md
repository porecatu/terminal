# ADR-0034 — Detecção de processo ativo para confirmação de fechamento

**Status:** Aceito
**Data:** 2026-09-03
**Relacionados:** [ADR-0005](0005-persistencia-de-sessao.md), [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0033](0033-job-object-encerramento-de-processo.md)
**Supersedes:** [ADR-0017](0017-ciclo-de-vida-da-aba.md) (parcial — só a seção 3, "RF-1.6 usa o modo do terminal, não o processo", e a linha correspondente da tabela de consequências negativas)

## Contexto

O usuário relatou dois sintomas do mesmo fechamento de aba com processo
ativo: o processo sobrevive à aba fechada ([ADR-0033](0033-job-object-encerramento-de-processo.md)
resolve isso) e **nenhum aviso aparece antes de fechar**. O segundo é este
ADR.

O ADR-0017 já havia decidido o critério de confirmação do RF-1.6: *"A
confirmação ao fechar dispara quando a aba está em tela alternativa ou tem
reporte de mouse ligado"* — um proxy barato para "programa de tela cheia
tomou o terminal" (`vim`, `htop`, `less`, `tmux`, `ssh` para um host com
algum desses aberto). O próprio ADR-0017 já registrava, como limitação
conhecida e aceita, exatamente o caso do relato: *"O que isso não cobre...
comando não interativo de longa duração... a aba fecha sem perguntar."*
`node server.js`/`mvn spring-boot:run` imprimindo log em primeiro plano,
sem tela alternativa nem mouse reporting, é esse caso.

O [ADR-0033](0033-job-object-encerramento-de-processo.md), ao resolver o
problema de matar a árvore, criou de graça a peça que faltava para cobrir
esse buraco: `ProcessGroup::process_count()`, contagem de processos vivos
na árvore a partir do shell raiz.

## Decisão

**A confirmação de `tab.close`/botão de fechar — e, desde a seção 6,
também `window.close` — passa a considerar dois sinais em OU: o modo do
terminal (ADR-0017, mantido) e, no Windows, mais de um processo vivo na
árvore do shell (novo). Os dois juntos ficam atrás da config
`general.confirm_close_with_process`, que existia desde o ADR-0017 mas
nunca teve consumidor.**

### 1. Por que isto supersede o ADR-0017 e não é ADR novo avulso

O ADR-0017 é a decisão aceita especificamente sobre o critério do RF-1.6.
Mudar esse critério é mudar aquela decisão — a regra do processo de decisão
arquitetural (CLAUDE.md) não permite editar uma decisão aceita, só
superseder com um ADR novo. O resto do ADR-0017 (OSC 7, precedência de
título, estado `Exited`, encerramento sem EOF) não muda em nada.

### 2. O sinal novo: contagem de processos vivos na árvore

`Terminal::has_extra_processes(&self) -> bool` (`crates/porecatu-term/src/terminal.rs`):
lê a cópia do `ProcessGroup` que fica do lado da UI (a mesma peça do
[ADR-0033](0033-job-object-encerramento-de-processo.md) seção 2 — leitura
direta, sem canal, sem espera), e considera "ativo" quando
`process_count()` devolve mais de 1 (o shell sozinho já é 1). Sem
`ProcessGroup` (fora do Windows, ou sem `process_id()` no spawn) resolve
para `false` — lado seguro do erro: não confirma por um sinal que não se
conseguiu ler, e o modo do terminal continua cobrindo o que sempre cobriu.

**A contagem em si não vem só do Job.** `process_count` é a união de duas
fontes — lista de membros do Job **e** varredura de processo vivo por
`sysinfo` a partir do PID raiz — porque nenhuma das duas sozinha basta: o
achado da investigação (ver [ADR-0033](0033-job-object-encerramento-de-processo.md)
seção "A primeira tentativa não bastou") é que **PowerShell 7 não propaga
a associação ao Job para os filhos**, apesar de ser o shell default do
Porecatu quando instalado. Contar só pelo Job teria deixado este ADR tão
incompleto quanto o ADR-0033 original — o diálogo simplesmente não
apareceria para o caso mais comum (`npm start` sob pwsh).

Em `crates/porecatu-ui/src/lib.rs`, `action_close_tab`/`close_tab_via_button`
passam a chamar uma função livre e pura,
`should_confirm_tab_close(confirm_close_with_process, modes, has_extra_processes)`,
testável nos quatro quadrantes sem `Terminal` nenhum:

```
confirm_close_with_process
    && (modes.alt_screen || modes.mouse_reporting != None || has_extra_processes)
```

### 3. Por que isto não é a "varredura" que o ADR-0005/0008/0017 rejeitaram

É a objeção óbvia, e precisa de resposta explícita, inclusive para a
metade da contagem que **é** uma varredura (a de `sysinfo`, seção 2). A
distinção não é "varre ou não varre" — é **o que decide e quando**:

- O que o ADR-0005/0008/0017 rejeitaram foi varrer a árvore de processos
  para **decidir qual processo é o de primeiro plano**, continuamente,
  como parte do roteamento de input/precedência de título — uma decisão
  ambígua (qual processo "conta"?), cara se repetida a cada tecla, e
  dependente de heurística por plataforma.
- O que a contagem daqui faz é **contar quantos processos existem** numa
  raiz já conhecida (o shell desta aba), disparada só pelo clique de
  fechar — um evento discreto, não um caminho quente. Não há ambiguidade
  sobre "qual é o alvo": é a árvore inteira do shell desta aba, ponto.
- A parte via Job é, além disso, O(1) do ponto de vista de quem chama,
  sobre um objeto de kernel que o próprio Porecatu já possui e é dono —
  não abre handle para processo nenhum do sistema. A parte via `sysinfo`
  é, sim, uma enumeração de todos os processos do SO — mas de custo
  aceitável porque roda só no clique de fechar, nunca em loop, e é
  exatamente o mesmo raciocínio que o [ADR-0033](0033-job-object-encerramento-de-processo.md)
  já usa para justificar a mesma varredura no momento de matar.

### 4. `GetConsoleProcessList`: avaliada e descartada

Alternativa óbvia (é a API que pergunta "quais processos estão anexados a
um console"). Descartada por incompatibilidade estrutural, não por não
funcionar: ela devolve os processos anexados ao **console do processo
chamador**, e perguntar sobre o console de outro processo (o shell de uma
aba específica) exigiria `AttachConsole(shell_pid)` — uma operação **por
processo inteiro**, não por thread. O Porecatu gerencia N abas
simultâneas, cada uma com seu próprio pseudo-console; perguntar sobre a
aba A exigiria desanexar de onde quer que o processo estivesse (inclusive
de uma consulta em andamento para a aba B) e anexar ao console de A —
inerentemente serial e mutuamente exclusivo entre abas, incompatível com
"múltiplos terminais abertos ao mesmo tempo" (a proposta central do
produto). Mesmo serializando atrás de um mutex global, o app inteiro
ficaria bloqueado por consulta, numa operação que já precisa ser
não-bloqueante (ADR-0017 item 4).

### 5. A config que finalmente ganha efeito

`confirm_close_with_process` (`crates/porecatu-config/src/general.rs`,
default `true`) existe desde o ADR-0017, mas nenhum código a lia — a
condição de confirmação era incondicional. Este ADR é o que a liga pela
primeira vez: com `false`, nem o sinal do modo do terminal nem o da
contagem de processo disparam confirmação — é a chave de escape para quem
prefere fechar sempre sem perguntar.

### 6. O mesmo critério vale para fechar a janela, não só a aba

Achado ao usar o app depois desta correção: fechar uma **janela com uma
única aba** que tem processo ativo não avisava nada, mesmo já valendo o
critério do `tab.close`. A razão é estrutural, não um esquecimento tardio:
`request_close_window` (RF-10.23, [ADR-0015](0015-multiplas-janelas.md))
só confirmava quando havia **mais de uma aba** — o critério de processo
ativo nunca tinha sido considerado nesse caminho, porque não existia até
este ADR. Mas fechar a janela mata a árvore de processo exatamente como
fechar a aba (`close_window_unconditionally` chama `Terminal::close` para
cada aba da janela) — não tem por que o aviso valer num caminho e não no
outro.

`request_close_window` passa a confirmar quando `tab_count > 1` **ou**
quando `should_confirm_tab_close` (a mesma função da seção 2) é verdadeira
para **qualquer** aba da janela — reaproveita a função inteira, sem
duplicar o critério. A mensagem do diálogo distingue os dois motivos
("mais de uma aba aberta" vs. "um programa em primeiro plano") para não
confundir o usuário com um aviso genérico demais.

## Alternativas consideradas

### `GetConsoleProcessList`

Descartada — seção 4 acima.

### Abrir o processo do shell e checar se ele tem um descendente direto vivo, sem recursão profunda

Evitaria depender do Job Object para este fim específico. Descartada
porque ainda exigiria uma enumeração (`CreateToolhelp32Snapshot` filtrando
por `ParentProcessId == shell_pid`, só não recursiva) — mais código, e o
Job já dá a resposta pronta sem esse passo, como efeito colateral do
ADR-0033. Só voltaria a ser relevante se o ADR-0033 não pudesse ser aceito.

### Manter só o critério do ADR-0017

Descartada: é exatamente o bug relatado.

### Confirmar sempre que houver qualquer processo além do shell, sem configurável

Uniforme, sem nova config. Descartada pelo mesmo motivo que o ADR-0017 já
rejeitou "confirmar sempre que a aba tiver processo filho vivo" — treina o
usuário a apertar Enter sem ler. A config é o portão.

## Consequências

### Positivas

- Cobre o caso relatado (`node`/`mvn`/qualquer servidor de longa duração
  em primeiro plano sem tela alternativa) sem regressão no que o
  ADR-0017 já cobria.
- `confirm_close_with_process` finalmente faz alguma coisa — lacuna que
  existia desde a F2.
- Lógica de decisão pura e testável sem `Terminal`/GPU/janela.

### Negativas

- Fora do Windows, o sinal novo nunca dispara (`ProcessGroup` sempre
  `None`) — a cobertura adicional é Windows-only até a dívida Unix do
  [ADR-0033](0033-job-object-encerramento-de-processo.md) ser paga.
- Ainda não cobre: comando que já terminou mas cujo processo de saída
  não foi coletado (janela de milissegundos, irrelevante), e `ssh`
  parado num prompt remoto (o processo remoto não existe do lado local)
  — mesmas lacunas que o ADR-0017 já documentava para o que ele cobria.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Prompt customizado (`starship`, `oh-my-posh`) spawna subprocesso curto (`git status`) bem no instante do clique | Baixa (janela de milissegundos) | Baixo | Mesmo raciocínio do ADR-0017 para falso positivo de `alt_screen`: lado seguro do erro, sem debounce nesta entrega |
| `ProcessGroup` é `None` (sem `process_id()` no spawn, ou fora do Windows) | Baixa | Médio — aba perde a cobertura nova | Aceito e documentado; é a mesma degradação que o ADR-0033 já promete |
| Descendente com cadeia de ancestrais parcialmente morta **e** que nunca entrou no Job (o ponto cego residual do ADR-0033 depois de unir as duas técnicas) não é contado | Baixa (exige as duas condições juntas) | Baixo — subestima a contagem, nunca superestima | Registrado como dívida residual no ADR-0033; lado seguro do erro (não confirmar por sinal não lido) |
| `confirm_close_with_process = false` desligar também o sinal do modo do terminal, que hoje era incondicional | Certeza (é a mudança proposta) | Baixo/neutro | Comportamento correto: a chave sempre foi documentada como guarda-chuva do RF-1.6 inteiro, só nunca tinha sido ligada |
