# ADR-0005 — Persistência de sessão em JSON versionado

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** PRD-003, ADR-0004, ADR-0006

## Contexto

Requisito 3 do produto: ao fechar e reabrir o emulador, as abas e grupos devem voltar abertos **nos mesmos diretórios** onde estavam. Processos não são restaurados — só o contexto de trabalho.

Isso impõe três problemas distintos:

1. **O quê e onde gravar** — formato, local, momento da escrita, durabilidade.
2. **Como descobrir o diretório atual de cada aba** — o problema difícil, e o que efetivamente limita a qualidade do recurso.
3. **O que fazer quando o arquivo estiver corrompido ou de uma versão antiga.**

O arquivo de sessão não é editado por humanos. É estado da aplicação, e as prioridades são durabilidade e evolução de schema, não legibilidade.

## Decisão

### Formato e local

**JSON** com campo `schema_version` no topo, gravado no diretório de estado da plataforma (crate `dirs`):

| Plataforma | Caminho |
|---|---|
| Linux | `$XDG_STATE_HOME/porecatu/session.json` (default `~/.local/state/porecatu/`) |
| macOS | `~/Library/Application Support/porecatu/session.json` |
| Windows | `%LOCALAPPDATA%\porecatu\session.json` |

Isso difere deliberadamente do local da **config** ([ADR-0003](0003-formato-de-configuracao.md), que usa `~/.config` inclusive no macOS): config é arquivo do usuário e segue a expectativa de quem usa terminal; sessão é estado da máquina e segue a convenção da plataforma. Estado de máquina não deve ir para dotfiles versionados.

JSON e não TOML porque não há humano lendo, `serde_json` é rápido, e representação de estruturas aninhadas com arrays é mais direta.

### O que é gravado

- `schema_version`
- Por janela: geometria (posição, tamanho, estado de maximizada) e monitor
- Ordem dos grupos, e por grupo: `id`, nome, cor, se está colapsado
- Ordem das abas dentro de cada grupo, e por aba: `id`, título customizado (se houver), `cwd`, e o programa de spawn se diferente do shell padrão
- Aba ativa por janela

### O que **não** é gravado

- Processos em execução (por definição do requisito)
- Conteúdo do scrollback (fora do escopo do v1 — ver PRD-003)
- Histórico de comandos (é do shell, não nosso)

### Durabilidade da escrita

1. Escrever em `session.json.tmp` no mesmo diretório.
2. `fsync` no arquivo.
3. `rename` atômico sobre `session.json`.

Nunca truncar o arquivo bom antes de ter o novo completo. Um crash no meio da gravação deixa a sessão anterior intacta.

### Quando gravar

- **Debounce de ~2 s** após qualquer mudança estrutural (abrir/fechar aba, agrupar, renomear, reordenar, mudar de diretório).
- **No encerramento**, de forma síncrona, antes de sair.
- **Nunca** a cada frame ou a cada byte do PTY.

O debounce importa: fechar 10 abas em sequência deve gerar uma escrita, não dez.

### Recuperação

| Situação | Comportamento |
|---|---|
| Arquivo ausente | Sessão nova com uma aba no diretório home. Normal, não é erro |
| JSON inválido / truncado | Renomeia para `session.json.corrupt`, inicia sessão nova, avisa o usuário |
| `schema_version` mais antiga | Migra em memória, grava na versão nova |
| `schema_version` mais nova que a suportada | Não sobrescreve. Inicia sessão nova, preserva o arquivo, avisa. Evita que uma versão antiga do app destrua o estado de uma nova |
| `cwd` gravado não existe mais | Abre a aba no home, mantém a estrutura de grupos |

---

## Detecção de `cwd` — o ponto crítico

Este é o único ponto onde o requisito 3 é limitado pela plataforma, e a limitação é real.

### Fonte primária: OSC 7

A sequência **OSC 7** (`ESC ] 7 ; file://host/path ST`) é emitida pelo shell a cada mudança de diretório. É a única fonte confiável, funciona igual nas três plataformas, e é como todos os emuladores modernos resolvem isso.

`porecatu-term` intercepta OSC 7 e atualiza o `cwd` da aba. Sem heurística, sem polling.

O custo: **exige integração de shell**. O usuário precisa ter um hook no `PROMPT_COMMAND` (bash), `precmd` (zsh), `starship`/`fish` (já emitem por padrão) ou no prompt do PowerShell.

Decisão de produto: o app **detecta a ausência de OSC 7** e oferece o snippet de integração adequado ao shell detectado, uma vez, de forma não intrusiva.

### Fallbacks por plataforma

| Plataforma | Fallback | Qualidade |
|---|---|---|
| Linux | ler `/proc/<pid>/cwd` (symlink) | Bom — barato e confiável |
| macOS | `libproc::proc_pidinfo` com `PROC_PIDVNODEPATHINFO` | Bom — requer o crate `libproc` |
| **Windows** | **nenhum barato** | **Ruim** |

### A limitação no Windows

Não existe API Win32 simples para ler o diretório atual de outro processo. As opções são todas ruins:

- Ler o PEB do processo com `NtQueryInformationProcess` + `ReadProcessMemory`: funciona, mas é API não documentada, sensível à arquitetura (32 vs 64 bits), quebra entre versões do Windows e dispara heurística de antivírus. **Rejeitada.**
- Injetar código no processo filho: fora de cogitação.

**Consequência aceita:** no Windows, sem OSC 7, o `cwd` restaurado é o diretório de spawn da aba, não o diretório onde o usuário estava. Isso é uma limitação conhecida e documentada no PRD-003, não um bug a ser perseguido.

Mitigação: no Windows, o convite à integração de shell é mais proeminente, porque lá ela não é uma melhoria — é a única forma de o recurso funcionar direito.

### Complicação adicional: o `cwd` de quem?

O PID do filho direto é o shell. Se o usuário está com `vim` aberto, ou dentro de um `ssh`, ou num shell aninhado, o cwd interessante pode ser de um descendente. Decisão v1: **usar sempre o shell diretamente spawnado**, sem varrer a árvore de processos. Varrer descendentes é caro, ambíguo e específico de plataforma. OSC 7 resolve isso naturalmente, porque quem emite é o shell interativo em primeiro plano.

---

## Alternativas consideradas

### TOML para a sessão

Consistente com a config. Descartada: ninguém edita o arquivo de sessão à mão, e TOML é pior que JSON para arrays de estruturas aninhadas, que é exatamente a forma do dado.

### SQLite

Escrita atômica e migração de schema resolvidas pela ferramenta, e abriria caminho para persistir scrollback depois. Descartada como exagero para um arquivo de algumas dezenas de KB escrito uma vez a cada poucos segundos, além de arrastar uma dependência nativa nas três plataformas. Revisitar se a persistência de scrollback entrar no escopo.

### Polling do cwd por plataforma, sem OSC 7

Ler `/proc` periodicamente para cada aba. Descartada como fonte primária: não funciona no Windows (que é o caso que mais precisa), gasta I/O proporcional ao número de abas, e é sempre uma aproximação. Fica só como fallback.

### Restaurar também os processos

Explicitamente fora do requisito. E não é possível em geral: reexecutar o comando que estava rodando não é seguro nem desejável (imagine restaurar um `rm -rf` interrompido).

## Consequências

### Positivas

- Requisito 3 atendido com durabilidade real: crash não perde a sessão anterior.
- `schema_version` permite evoluir o formato sem quebrar instalações existentes.
- `porecatu-session` fica trivial: serializa `porecatu-core`, que é puro e testável.

### Negativas

- **A qualidade do recurso depende de integração de shell**, especialmente no Windows. Um usuário que ignore o convite terá restauração parcial (estrutura sim, diretórios não).
- Restauração de diretório em aba que estava dentro de `ssh` ou container restaura o caminho local, que pode não fazer sentido. Aceito.
- Arquivo de sessão é mais um lugar onde estado pode divergir da realidade.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Usuário Windows sem OSC 7 achar o recurso quebrado | Alta | Alto | Detecção de ausência + convite proeminente com snippet pronto; limitação documentada no PRD-003 |
| Corrupção do arquivo por crash durante a escrita | Baixa | Alto | tmp + fsync + rename atômico |
| Versão antiga do app destruir sessão de versão nova | Baixa | Alto | Recusa explícita de sobrescrever `schema_version` mais nova |
| Debounce perder a última mudança antes de um crash | Média | Baixo | Perda máxima de 2 s de mudanças estruturais; aceito |
| Sessão com 50 abas ficar lenta para restaurar | Média | Médio | Spawn de shells preguiçoso: só a aba visível de cada janela sobe no start; as demais sobem ao serem focadas |
