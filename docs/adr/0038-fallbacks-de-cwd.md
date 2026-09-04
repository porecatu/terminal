# ADR-0038 — Diretório de trabalho sem OSC 7: fallbacks por `sysinfo`

**Status:** Aceito
**Data:** 2026-09-04
**Relacionados:** [ADR-0004](0004-pty-cross-platform.md), [ADR-0005](0005-persistencia-de-sessao.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0033](0033-job-object-encerramento-de-processo.md), [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md), [ADR-0036](0036-formato-do-arquivo-de-sessao.md), [ADR-0039](0039-convite-a-integracao-de-shell.md), PRD-003

## Contexto

A fonte primária do `cwd` é OSC 7, capturada desde a F2 (`porecatu-term/src/osc7.rs`, [ADR-0017](0017-ciclo-de-vida-da-aba.md) §1). Ela funciona igual nas três plataformas e não custa nada — mas **exige integração de shell**, e nem todo shell a emite por padrão.

O ADR-0005 previu fallbacks para os casos em que ela não chega, e os nomeou por mecanismo: `/proc/<pid>/cwd` no Linux, `libproc` com `PROC_PIDVNODEPATHINFO` no macOS, e **nada** no Windows — a leitura do PEB por `NtQueryInformationProcess` + `ReadProcessMemory` foi explicitamente rejeitada ali (API não documentada, sensível a 32 vs 64 bits, quebra entre versões e dispara heurística de antivírus), e injeção de código ficou fora de cogitação.

Nenhum dos dois fallbacks foi implementado. Ao abrir a F5, duas coisas mudaram desde que o ADR-0005 foi escrito:

1. **`sysinfo` entrou no workspace** (`crates/porecatu-pty/Cargo.toml`, MIT, versão 0.39), trazido pelo [ADR-0033](0033-job-object-encerramento-de-processo.md) para a varredura de descendentes por PID que o Job Object sozinho não cobre. Ele expõe `Process::cwd()`, e os mecanismos que ele usa por trás **são exatamente os dois** que o ADR-0005 nomeou.
2. **`ProcessGroup` já guarda o `root_pid`** de cada aba (`porecatu-pty/src/job.rs`), e já faz consulta pontual por PID — a máquina de fazer a pergunta existe e está exercitada.

A decisão que falta é qual caminho tomar, e onde ele mora, dado que `porecatu-core` não conhece processo e `porecatu-pty` não conhece domínio.

## Decisão

**O fallback de `cwd` é `sysinfo::Process::cwd()`, consultado a partir do `root_pid` do `ProcessGroup`, no Linux e no macOS. No Windows não há fallback.**

### 1. Onde mora

`ProcessGroup::cwd() -> Option<PathBuf>`, em `porecatu-pty/src/job.rs`, ao lado de `process_count` e `kill_tree`. É o único tipo do projeto que já tem o `root_pid` de uma aba e já usa `sysinfo`; pôr a consulta em qualquer outro lugar exigiria vazar o PID por mais uma fronteira. `porecatu-term` re-exporta, como já re-exporta `SpawnConfig`/`PtySize`/`PtyError` — é o caminho permitido, e nenhum tipo de `sysinfo` atravessa.

A consulta é **pontual**: `refresh` de um PID só, como `process_count` já faz, nunca da lista inteira de processos da máquina.

### 2. Quando é consultado

**No momento da gravação da sessão**, e só quando a aba nunca recebeu um `TermEvent::Cwd`. Nunca por frame, nunca por byte do PTY, nunca em laço.

Isso é o inverso do que o ADR-0005 descartou como fonte primária ("polling do cwd por plataforma"): não há polling. A gravação já é debounced em ~2 s (RF-3.3) e já acontece só em mudança estrutural (RF-3.2); a consulta pega carona nela. Com OSC 7 presente — o caso bom — a consulta não acontece nunca, porque o `cwd` da aba já está preenchido.

A precedência, então, é: `Tab::cwd` vindo de OSC 7 → `ProcessGroup::cwd()` → o `cwd` de spawn da aba. Aba em estado `NotStarted` ([ADR-0037](0037-aba-nao-iniciada.md)) não tem `ProcessGroup` e grava o `cwd` que a sessão anterior já tinha para ela.

### 3. Windows: a rejeição do ADR-0005 continua de pé

`sysinfo` compila `Process::cwd()` nas três plataformas, mas **não o chamamos no Windows**. A rejeição do ADR-0005 é do **mecanismo**, não de quem o escreve: ler o PEB de outro processo continua sendo API não documentada e continua disparando heurística de antivírus, e nada disso melhora por estar dentro de uma dependência.

Consequência aceita, idêntica à que o ADR-0005 registrou e o PRD-003 documenta: **no Windows, sem OSC 7, o diretório restaurado é o de spawn da aba, não aquele onde o usuário estava.** É comportamento esperado, não bug — e é por isso que o convite à integração de shell é mais proeminente lá ([ADR-0039](0039-convite-a-integracao-de-shell.md)).

### 4. O `cwd` de quem

Continua sendo o do **shell diretamente spawnado**, sem varrer a árvore de descendentes — a regra do ADR-0005 não muda. `ProcessGroup` sabe varrer a árvore (é o que `process_count` faz), e deliberadamente não a varre aqui: com `vim` ou um `ssh` aberto, o descendente "mais interessante" é ambíguo, e escolher errado restaura um diretório que o usuário nunca viu. OSC 7 resolve isso naturalmente, porque quem a emite é o shell interativo em primeiro plano.

### 5. Detecção de ausência de OSC 7

O mesmo sinal que decide consultar o fallback é o que alimenta o convite do [ADR-0039](0039-convite-a-integracao-de-shell.md): uma aba que nunca recebeu `TermEvent::Cwd`. O critério temporal e a superfície do convite são decididos lá; aqui fica só o fato de que o sinal é um por aba, e é o mesmo.

## Alternativas consideradas

### `/proc` lido à mão no Linux e o crate `libproc` no macOS

Era o que o ADR-0005 nomeou, e foi o plano até a escrita deste ADR. Descartada por três motivos concretos: acrescenta uma dependência nova (`libproc` 0.14.11, MIT — compatível, mas nova) exatamente na plataforma que **não temos como verificar** no fluxo do projeto; produz dois caminhos de código com `#[cfg]` onde um resolve; e o mecanismo por trás de `sysinfo` já é esse mesmo, com a diferença de estar em uso e sob CI desde o ADR-0033. Trocar dois caminhos novos por uma chamada num crate que já está na árvore é menos superfície, não mais.

### PEB no Windows, via `sysinfo` em vez de à mão

O crate esconderia o `unsafe`, como `win32job` fez pelo ADR-0033. Descartada: o problema do PEB nunca foi o `unsafe` — foi a API não documentada, a sensibilidade a arquitetura e o antivírus. Nada disso muda de dono junto com o código.

### Varrer a árvore de descendentes e usar o `cwd` do processo em primeiro plano

Resolveria `vim`, `ssh` e shell aninhado. Descartada pelo ADR-0005 e mantida descartada: em Unix daria para achar o grupo em primeiro plano por `tcgetpgrp`, mas o ConPTY não tem equivalente ([ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md) já registrou isso), então o resultado seria uma regra boa em duas plataformas e uma heurística na terceira.

### Consultar o fallback continuamente, mantendo `Tab::cwd` sempre fresco

Faria o `cwd` do menu de contexto e do `tab.new` herdado ficar correto mesmo sem OSC 7, não só o gravado. Descartada: é o polling que o ADR-0005 rejeitou como fonte primária, gasta I/O proporcional ao número de abas, e não ajuda no Windows — que é onde o problema dói —, porque lá não há fallback nenhum a consultar.

## Consequências

### Positivas

- **Nenhuma dependência nova.** O que o ADR-0005 previa como dois mecanismos e um crate a mais vira uma chamada num crate já vetado, já em uso e já sob CI nas três plataformas.
- Um caminho de código para Linux e macOS, em vez de dois com `#[cfg]`.
- Custo zero no caso bom: com OSC 7, a consulta nunca acontece.
- A pergunta mora onde o PID já mora, sem vazar `sysinfo` nem PID por fronteira nenhuma.

### Negativas

- O fallback do macOS entra **verificado só por compilação e por CI**, sem nunca ter rodado numa máquina real — não há ambiente macOS no fluxo do projeto. Dívida assumida, na mesma linha da dívida de verificação interativa registrada no topo do roadmap.
- `porecatu-pty` ganha uma responsabilidade que não é PTY nem encerramento. É onde o PID está, e o custo é essa impureza.
- A qualidade do recurso no Windows continua dependendo inteiramente de integração de shell.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| `sysinfo` com `default-features = false` não popular `cwd` | Média | Médio | Refresh explícito pedindo `cwd`, com teste que spawna um shell, faz `cd` e compara — reprova se vier vazio |
| Alguém chamar `ProcessGroup::cwd()` no Windows por engano | Média | Alto | A função não existe fora de Linux/macOS (`#[cfg]`), então é erro de compilação, não decisão em runtime |
| Consulta de `sysinfo` custar caro com muitas abas na gravação | Baixa | Médio | Refresh de um PID por aba, só para abas sem OSC 7, só na gravação já debounced |
| Custo de `sysinfo` no macOS diferir do medido no Linux | Média | Baixo | Sem ambiente para medir; a mitigação real é a frequência (uma vez a cada ~2 s, no pior caso) |
| Usuário no Windows achar o recurso quebrado | Alta | Alto | Convite proeminente do [ADR-0039](0039-convite-a-integracao-de-shell.md); limitação documentada no PRD-003 e no critério de saída da F5 |
