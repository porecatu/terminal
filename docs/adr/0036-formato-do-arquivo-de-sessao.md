# ADR-0036 — Formato do arquivo de sessão: DTO versionado em `porecatu-session`

**Status:** Aceito
**Data:** 2026-09-04
**Relacionados:** [ADR-0005](0005-persistencia-de-sessao.md), [ADR-0006](0006-modelo-de-abas-e-grupos.md), [ADR-0015](0015-multiplas-janelas.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0031](0031-temas-nomeados.md), PRD-003
**Supersedes:** [ADR-0005](0005-persistencia-de-sessao.md) (parcial — a seção "O que é gravado", a linha "`porecatu-session` fica trivial: serializa `porecatu-core`" das consequências positivas, e a linha "JSON inválido / truncado" da tabela de recuperação)

## Contexto

O ADR-0005 decidiu o formato da sessão em prosa: JSON com `schema_version`, no diretório de estado da plataforma, escrito por tmp+fsync+rename. Isso basta para escolher a tecnologia e não basta para escrever o código. Ao abrir a F5, seis lacunas apareceram de uma vez.

**Não há nome de chave, nem valor inicial de `schema_version`, nem mecanismo de migração.** "Migra em memória, grava na versão nova" descreve o efeito, não a forma. Sem tipos por versão, migrar de 1 para 2 significa desserializar num tipo que precisa aceitar as duas formas ao mesmo tempo — e é exatamente aí que migração de schema vira um `Option<T>` atrás do outro.

**O envelope multi-janela não existe em tipo nenhum.** O RF-3.17 grava e restaura N janelas como conjunto, mas `Workspace` é mono-janela por decisão do [ADR-0015](0015-multiplas-janelas.md) — um workspace por janela. Geometria, estado de maximizada e monitor não têm representação em lugar algum do domínio, e não podem ter: `porecatu-core` não conhece `winit`.

**A identidade do monitor entre execuções não foi decidida.** O RF-3.11 diz o que fazer quando ele sumiu; não diz como reconhecê-lo quando ele está lá.

**E há uma contradição viva entre o ADR-0005 e o código.** `porecatu-core` deriva `Serialize`/`Deserialize` em todo o domínio desde a F2 — decisão deliberada, para que o round-trip que o ADR-0006 lista como invariante fosse testável com `porecatu-session` ainda vazio. Só que não há um `#[serde(skip)]` no crate inteiro. Hoje o derive grava:

| Campo | Onde | O que o documento diz |
|---|---|---|
| `Group::last_active` | `porecatu-core/src/group.rs` | o comentário no próprio campo afirma "não persistido -- a lista do ADR-0005 não o inclui" |
| `Tab::activity`, `Tab::bell` | `porecatu-core/src/tab.rs` | indicadores de saída; nada a restaurar |
| `Tab::process_title` | idem | título vindo do processo que já morreu |
| `Tab::state` | idem | [ADR-0017](0017-ciclo-de-vida-da-aba.md) §6: aba `Exited` **não é restaurada** — restaurar uma aba morta restauraria um erro passado |

Nenhum deles está na lista do ADR-0005, e o último a contradiz diretamente. A contradição é latente só porque ninguém escreve o arquivo ainda.

O [ADR-0031](0031-temas-nomeados.md) acrescenta um sétimo item: persistir o tema de sessão é F5, e ele também não está na lista do ADR-0005.

## Decisão

**`porecatu-session` define o schema do arquivo em tipos próprios, versionados por módulo, e converte de e para `porecatu-core` explicitamente.** O domínio não é serializado direto para o disco.

### 1. Tipos por versão

```
crates/porecatu-session/src/
  schema/
    mod.rs      -- despacho por schema_version e migração encadeada
    v1.rs       -- SessionFileV1 e tudo que ela contém
  convert.rs    -- v1 <-> porecatu-core, nos dois sentidos
  path.rs       -- resolução do caminho
  lib.rs        -- load / save, a tabela de recuperação
```

A forma da v1:

```rust
struct SessionFileV1 {
    schema_version: u32,                 // 1
    windows: Vec<WindowV1>,
    shell_integration_dismissed: bool,   // ADR-0039
}

struct WindowV1 {
    geometry: GeometryV1,
    monitor: Option<MonitorIdV1>,
    groups: Vec<GroupV1>,
    tabs: Vec<TabV1>,
    active_tab: Option<u32>,
    theme: Option<String>,               // ADR-0031
    zoom_steps: i32,
}

struct GroupV1 { id: u32, name: Option<String>, color: Option<String>, collapsed: bool, tabs: Vec<u32> }
struct TabV1   { id: u32, custom_title: Option<String>, cwd: Option<PathBuf>, spawn_program: Option<String> }
struct GeometryV1  { x: i32, y: i32, width: u32, height: u32, maximized: bool }
struct MonitorIdV1 { name: Option<String>, x: i32, y: i32 }
```

`schema_version` nasce em **1**. Ela sobe quando um campo muda de significado ou some — não quando um campo opcional é acrescentado, que `#[serde(default)]` já absorve. Subir a versão obriga a criar `v2.rs` e a função `v1 -> v2`; migrar de 1 para 3 é a composição das duas, nunca um caminho direto.

### 2. Por que DTO, e não `#[serde(skip)]` no domínio

A alternativa barata era marcar os voláteis com `skip` em `porecatu-core` e filtrar abas `Exited` na gravação. Ela mantém o "trivial" que o ADR-0005 prometeu e reusa o teste de round-trip que já existe. Foi descartada por um motivo específico: **acopla o formato de disco à forma do domínio**. Campo novo em `Tab` vira campo novo no arquivo sem ninguém decidir, e a decisão de persistir passa a ser o default — o inverso do que se quer num formato que precisa sobreviver a versões do app. E migração com tipos por versão fica impossível de escrever, porque só existe **um** tipo, o de hoje.

O custo do DTO é código de conversão mais um teste. É custo aceito, e vem com um ganho: o teste que reprova quando um campo novo do domínio não foi considerado só é escrevível porque a conversão é explícita.

### 3. O que é gravado — a lista fecha aqui

Substitui a seção "O que é gravado" do ADR-0005.

**Gravado:** `schema_version`; por janela, geometria (posição, tamanho, maximizada) e monitor; ordem dos grupos e, por grupo, `id`, nome, cor e colapso; ordem das abas dentro de cada grupo e, por aba, `id`, título customizado, `cwd` e programa de spawn quando diferente do shell padrão; aba ativa por janela; tema de sessão e passos de zoom por janela; a dispensa definitiva do convite de integração de shell ([ADR-0039](0039-convite-a-integracao-de-shell.md)).

**Não gravado, explicitamente:** `Group::last_active`, `Tab::activity`, `Tab::bell`, `Tab::process_title`, `Tab::state`. Aba em estado `Exited` **é descartada na gravação**, não na leitura — o arquivo não guarda o que não deve voltar. Processos, scrollback e histórico de comandos continuam fora, como o ADR-0005 decidiu.

Tema e zoom são **por janela**, não por processo. Restaurar duas janelas com temas diferentes sai de graça nesta forma, e é a forma que não precisa mudar se o `zoom_scope` ganhar escopo menor depois.

### 4. Identidade de monitor

`MonitorIdV1` grava o nome do monitor quando a plataforma o dá, mais a posição da origem dele no espaço virtual. Na restauração, casa por nome; sem nome, ou sem casamento, casa por posição; sem nenhum dos dois, cai no monitor primário com o tamanho preservado dentro dos limites da tela (RF-3.11). Nome é estável e legível quando existe; a posição é o desempate que funciona quando o mesmo modelo aparece duas vezes.

### 5. `.corrupt` que já existe

O ADR-0005 manda renomear para `session.json.corrupt`. Se já houver um, o novo vira `session.json.corrupt.1`, `.2`, e assim por diante, sempre no primeiro livre. **Nunca sobrescrever**: o arquivo preservado existe para ser examinado, e o segundo acidente não pode apagar a evidência do primeiro.

### 6. `PORECATU_SESSION`

O caminho resolve por `PORECATU_SESSION` → caminho de plataforma via `dirs`, na mesma forma que `porecatu-config/src/path.rs` já usa para a config (sem o nível de `--config`: não há flag para a sessão). A variável é a costura que torna gravação e leitura testáveis sem tocar o diretório de estado real da máquina de quem roda o teste.

Não há chave TOML de caminho de sessão, pelo motivo que o próprio arquivo de exemplo registra: trocar o destino com sessão em memória pediria migração de arquivo. `[session] enabled = false` continua sendo o desligamento inteiro, tema e zoom inclusos.

## Alternativas consideradas

### `#[serde(skip)]` no domínio, com filtro de `Exited` na gravação

Descrita e descartada na seção 2. Era a leitura literal do ADR-0005, e o motivo de não seguir é o acoplamento do formato de disco à forma do domínio, mais a impossibilidade de tipos por versão.

### Um tipo só, com todos os campos opcionais, migrando por `Option`

É o que acontece por acidente quando não se decide isto. Descartada: cada versão nova acrescenta uma camada de `Option` que nunca some, e ler o tipo deixa de dizer qual é a forma atual do arquivo.

### Gravar o `Workspace` inteiro por janela, como valor opaco dentro do envelope

Resolve o envelope multi-janela sem escrever `GroupV1`/`TabV1`. Descartada pelo mesmo motivo da primeira: o conteúdo do valor opaco continua sendo a forma do domínio, e é o conteúdo que precisa de versão.

### Índice de monitor em vez de nome e posição

Mais simples de gravar. Descartada: o índice muda quando o usuário desliga um monitor ou troca a ordem no sistema, e o sintoma seria a janela reaparecer na tela errada sem que nada tenha sumido — exatamente o caso que o RF-3.11 não cobre, porque para ele o monitor ainda existe.

## Consequências

### Positivas

- Campo novo em `porecatu-core` não vaza para o disco por acidente. Persistir passa a ser decisão, não default.
- Migração de `schema_version` tem tipos por versão e uma função de migração por salto, encadeáveis.
- A contradição entre a lista do ADR-0005 e o derive do domínio deixa de existir: a lista está num tipo, não numa prosa.
- `porecatu-core` não muda. O derive de `serde` continua lá, servindo ao teste de round-trip do ADR-0006.
- O envelope multi-janela do RF-3.17 fica onde pode ficar — em `porecatu-session`, que pode conhecer geometria sem que `porecatu-core` conheça `winit`.

### Negativas

- **`porecatu-session` deixa de ser trivial.** O ADR-0005 prometia um crate que serializa `core` e mais nada; ele passa a ter conversão nos dois sentidos e um módulo por versão de schema. É o custo direto desta decisão.
- Campo novo que **deve** ser persistido agora exige mexer em dois lugares. Mitigado pelo teste de cobertura de campo, não eliminado.
- Duas representações do mesmo dado convivem no processo (domínio e DTO), e um bug de conversão é invisível para os testes do domínio.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Campo novo do domínio ficar de fora do DTO sem ninguém notar | Alta | Médio | Teste de cobertura de campo em `convert.rs`, que reprova quando um campo de `Tab`/`Group`/`Workspace` não foi classificado como gravado ou explicitamente descartado |
| Bug de conversão passar pelos testes de `porecatu-core` | Média | Alto | Round-trip pelo **DTO**, não pelo derive do domínio: `Workspace -> WindowV1 -> JSON -> WindowV1 -> Workspace` |
| Migração encadeada não exercitada até existir uma v2 | Alta | Baixo | Teste com uma versão fictícia de migração desde a v1, para que o mecanismo nasça exercitado |
| `PORECATU_SESSION` apontando para caminho inválido em produção | Baixa | Baixo | Mesmo tratamento de arquivo ausente: sessão nova, sem erro |
