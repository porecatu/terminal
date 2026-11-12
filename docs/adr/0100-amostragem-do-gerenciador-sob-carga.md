# ADR-0100 — Amostragem do gerenciador de tarefas sob carga: CPU por tempo acumulado, varredura em duas passadas, relógio ancorado e lacuna tolerante

**Status:** Aceito — verificado só por teste automatizado (verificação ao vivo sob carga no Windows é dívida, no roadmap)
**Data:** 2026-11-12
**Relacionados:** [ADR-0093](0093-medicao-de-processos-do-gerenciador.md), [ADR-0095](0095-gerenciador-de-tarefas-modo-da-janela.md), [ADR-0098](0098-monitoramento-em-segundo-plano-do-gerenciador.md), [PRD-024](../prd/prd-024-gerenciador-de-tarefas.md)
**Supersedes:** ADR-0093 §4 e §5 (**parcial**: o prazo seguinte é ancorado, o amostrador perdido exige a thread encerrada, a varredura é em duas passadas e a CPU sai do tempo acumulado), ADR-0095 §6 (**parcial**: o limiar da lacuna passa de 1,5 s para 3,5 s)

## Contexto

O dono relatou que, com a máquina sob carga alta, os gráficos do gerenciador abrem lacunas e mostram pouca CPU, e que um processo a ~100% iniciado por um shell do Porecatu quase não aparecia na lista. Lendo o código e o `sysinfo` 0.40.1 (Windows), as causas são duas, independentes, e ambas pioram com a carga.

**Lacunas.** O limiar de lacuna era 1,5 s para um período de 1 s: 0,5 s de folga. Sob carga ele estoura por quatro caminhos somados:

1. a varredura fazia `OpenProcess`, `GetProcessTimes`, `GetSystemTimes`, `GetProcessMemoryInfo` e `GetProcessIoCounters` em **todos** os processos da máquina, numa thread de prioridade normal que disputa CPU com o próprio processo que satura a máquina; uma varredura acima de 1 s pula o tick seguinte (o pedido não acumula, ADR-0093 §4) e o ponto seguinte chega a 2 s ou mais;
2. o prazo seguinte era `now + 1 s`, com `now` o instante em que o event loop processou o tick: a latência do loop se acumulava;
3. `taken_at` era medido depois da varredura, então a duração dela virava jitter entre pontos;
4. um conjunto vazio de processos (abas só com SSH ou saídas) dava CPU `None`: lacuna só no gráfico de CPU, com memória e disco seguindo.

**CPU subestimada.**

1. O `cpu_usage()` do `sysinfo` no Windows parte de uma linha de base zero para cada processo novo: a primeira leitura é 0 e a **segunda** é o CPU de toda a vida do processo dividido pelo tempo de sistema desde o boot, ou seja, perto de zero. Só a terceira é correta. O ADR-0093 §5 só tratava a primeira (`None`).
2. O `System` é recriado quando o amostrador é dado por perdido (`ANSWER_LOST_AFTER`, 10 s) e quando o último gerenciador fecha sem `background_monitoring`. Recriar reinicia a linha de base de **todos** os processos. Sob carga, uma varredura acima de 10 s derrubava o amostrador, e o novo repetia o ciclo (CPU `None` e duas amostras de ~0) enquanto a carga durasse.

## Decisão

### 1. A CPU sai do tempo acumulado, calculada pelo amostrador

`ProcessStat::cpu` deixa de vir de `Process::cpu_usage()`. A thread guarda, por `(pid, start_time)`, a última leitura de `Process::accumulated_cpu_time()` (ms de usuário mais sistema, API pública, atualizada em todo refresh com `with_cpu()`) e o instante dela. A CPU é `Δms / 1000 / Δtempo-real / CPUs lógicas`, limitada a `0.0..=1.0`. A chave com `start_time` impede que um PID reaproveitado herde a leitura de outro processo.

- Um processo que o amostrador nunca viu tem CPU `None` (RF-24.9), no lugar do valor errado da segunda leitura do `sysinfo`. A segunda leitura já é correta.
- O disco segue a mesma regra: a primeira leitura de um processo é o total da vida dele, não uma taxa, e conta como zero. O tempo da taxa é o `Δtempo-real` por processo, não um intervalo global.
- Um contador que diminuiu conta como zero.
- O estado guarda só os processos da última amostra: um painel que deixa de ser medido volta a `None` quando voltar.

### 2. Varredura em duas passadas

1. `refresh_processes_specifics(All, true, ProcessRefreshKind::nothing())`: só a lista (pai e início), tudo de que o critério de pertença do ADR-0093 §2 precisa.
2. `members` por painel (o mesmo critério, sem mudança) e `refresh_processes_specifics(Some(membros), true, cpu + memória + disco)`: as chamadas caras só nos processos medidos.

`taken_at` é o ponto médio da varredura: as leituras se espalham por ela, e o eixo do tempo e as taxas usam esse único instante.

### 3. Relógio ancorado

O prazo seguinte é `due + 1 s`, o prazo que acabou de vencer, e não `now + 1 s`: um tick atrasado não empurra os seguintes. Um tick atrasado por **mais de um período** (a máquina parou) recomeça a grade a partir de `now`, sem rajada de recuperação. Função pura `next_deadline(due, now)`.

### 4. O amostrador lento é esperado

`SampleTicket::is_lost(now, thread_finished)`: perdido só se pendente há 10 s **e** a thread terminou (`Sampler::is_finished`), ou pendente há 60 s com a thread viva (presa numa chamada ao SO). Uma varredura apenas lenta é esperada e **não** reinicia a linha de base. O pedido continua sem acumular fila (ADR-0093 §4).

### 5. A lacuna é "o gerenciador estava fechado", não "uma amostra atrasou"

`GAP_SECONDS` passa de 1,5 para **3,5** (RF-24.12, ADR-0095 §6): tolera duas amostras perdidas. Fechar o gerenciador sem `background_monitoring` continua abrindo uma lacuna de vários segundos. Efeito aceito: sair e voltar em menos de 3,5 s liga os pontos, em vez de mostrar o vão.

Uma janela medida **sem nenhum processo local** (todas as abas são notas) grava o ponto com CPU `0.0`, não `None`: é "zero, não lacuna", como o `apply_to_window` já documentava e o teste não cumpria. Processos presentes, mas ainda sem leitura de CPU (a primeira amostra de um amostrador novo), continuam `None`: essa é uma lacuna real.

## Alternativas consideradas

### Só aumentar o limiar
Esconderia as lacunas sem tocar na CPU baixa nem no ciclo de recriação; o gráfico ficaria contínuo e errado.

### Medir o `porecatu.exe` e o `OpenConsole.exe`
Muda o critério de pertença do ADR-0093 §2 e o que "uma aba consome" quer dizer. Fora do escopo; ver Riscos.

### Subir a prioridade da thread do amostrador
Exigiria um crate wrapper (`unsafe_code = "deny"`). Só se a verificação ao vivo mostrar que as correções acima não bastam.

### Priming: duas atualizações curtas antes de reportar
Atrasaria a primeira leitura de todo processo e mantém a dependência do comportamento interno do `sysinfo`. O tempo acumulado é um contrato público e estável.

## Consequências

### Positivas
- A varredura toca centenas de processos a menos por amostra; o custo passa a depender das abas, não da máquina.
- A CPU de um processo novo aparece correta na segunda amostra, e não na terceira.
- Carga alta não derruba nem reinicia o amostrador.

### Negativas
- Um processo que passa a ser membro depois (aba nova, janela que passa a ser medida) começa em `None` e só tem CPU na amostra seguinte, como qualquer processo visto pela primeira vez. Antes, a varredura total já o aquecia.
- A lacuna de um gerenciador fechado e reaberto em menos de 3,5 s some.

### Riscos e mitigação

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| A causa observada pelo dono ser o `OpenConsole.exe` ou o próprio `porecatu.exe`, fora da árvore do shell | Baixa | Médio | Não é medido por decisão do ADR-0093 §2; se a verificação ao vivo mostrar isso, é ADR novo |
| `accumulated_cpu_time` com granularidade de tique do relógio (≈15,6 ms) | Alta | Baixo | O erro é no máximo ~1,5% de um núcleo por amostra de 1 s |
| Verificação sob carga não feita ainda | — | Médio | Dívida no roadmap: laços em 2–3 abas, 2 min sem lacuna, comparar com o Gerenciador de Tarefas do Windows |
