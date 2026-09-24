# ADR-0054 — Sessões nomeadas: um arquivo por sessão ao lado do `session.json`, mesmo schema, restauração pelo caminho do arranque

**Status:** Aceito
**Data:** 2026-09-24
**Relacionados:** [ADR-0005](0005-persistencia-de-sessao.md), [ADR-0008](0008-teclas-e-roteamento-de-input.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0015](0015-multiplas-janelas.md), [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0031](0031-temas-nomeados.md), [ADR-0036](0036-formato-do-arquivo-de-sessao.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0040](0040-superficie-de-linha-de-comando.md), [ADR-0051](0051-arquivo-de-projeto-porecatu.md), [ADR-0053](0053-paineis-divididos.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), PRD-003, PRD-006, PRD-012, [PRD-014](../prd/prd-014-sessoes-nomeadas.md)
**Supersedes:** ADR-0005, "Quando gravar" (**parcial**: só a premissa implícita de que toda gravação é automática; a sessão automática continua sem gesto manual) · ADR-0036 §1 e §6 (**parcial**: um campo opcional novo em `SessionFileV1` e um segundo destino de arquivo; o schema, a versão e a resolução do `session.json` não mudam)

## Contexto

O [PRD-014](../prd/prd-014-sessoes-nomeadas.md) pede que o usuário salve **a janela em que está** com um nome, e depois a reabra numa **janela nova**, com o mesmo comportamento de uma janela restaurada no arranque — `cwd`, árvore de painéis, `lazy_restore`, `.porecatu`. A terceira parte do pedido original, desligar a sessão automática, já é o RF-3.6 e não se mexe.

O terreno está quase todo pronto, e é por isso que este ADR é sobre **onde encaixar**, não sobre o que construir:

- `porecatu-session` já tem o schema de uma janela inteira em `WindowV1` ([ADR-0036](0036-formato-do-arquivo-de-sessao.md) §1), com a árvore de painéis desde o [ADR-0053](0053-paineis-divididos.md) §11, conversão nos dois sentidos (`convert.rs`), gravação atômica e quarentena de arquivo inválido — e `load_from`/`save_to` já recebem o caminho explícito.
- `porecatu-ui` já cria uma janela a partir de um `WindowV1` em `open_window_from_session` (`crates/porecatu-ui/src/lib.rs`), e é esse caminho que decide `SpawnOrigin::Restored`, sobe a aba ativa (ou todas, com `lazy_restore = false`), os painéis irmãos, e é por ele que o `.porecatu` roda.
- `open_window` já sabe posicionar uma janela nova em cascata a partir de outra (`NEW_WINDOW_CASCADE_PX`, grampeada no monitor da origem).

Quatro coisas **não** estão decididas por nada disso, e cada uma tem uma resposta errada tentadora:

1. **Onde os arquivos moram** — a tentação é a config (é "do usuário"); o [ADR-0005](0005-persistencia-de-sessao.md) já explicou por que estado de máquina não vai para dotfiles.
2. **Em que formato** — a tentação é um formato próprio, "mais simples"; é um segundo schema a migrar para sempre.
3. **Como a janela nasce** — a tentação é chamar o caminho do arranque inteiro, que aplica tema e zoom do arquivo ao processo todo.
4. **Como isso convive com a decisão de que gravar é automático** — o catálogo registra `session.save`/`session.restore` como ações que **não existem**, e a razão escrita ali é boa.

## Decisão

**Cada sessão nomeada é um arquivo `SessionFileV1` com exatamente uma janela, num diretório `sessions/` ao lado do `session.json`. A restauração passa pelo mesmo `open_window_from_session` do arranque, com a posição trocada pela cascata e sem tocar tema nem zoom. A sessão automática não lê nem escreve esse diretório, e `[session] enabled` não o alcança.**

### 1. A sessão nomeada é outro objeto, não um gesto manual sobre a automática

O [ADR-0005](0005-persistencia-de-sessao.md) decidiu que a sessão é gravada **sozinha** — debounce, encerramento — e o catálogo de ações traduziu isso em *"`session.save` / `session.restore` não existem: ação manual sugeriria que não é"*. As duas coisas continuam verdadeiras para o `session.json`. Nada neste ADR dá ao usuário um jeito de forçar, adiar ou restaurar a sessão automática.

O que entra é um objeto diferente, com três diferenças que o separam por construção:

| | Sessão automática | Sessão nomeada |
|---|---|---|
| Quem grava | o app, sozinho | o usuário, por gesto |
| O que grava | todas as janelas, como conjunto (RF-3.17) | **uma** janela |
| Quando é lida | no arranque, uma vez | quando o usuário pede, numa janela **nova** |

Por isso a linha do catálogo é **revista**, não apagada: `session.save` e `session.restore` continuam não existindo, e o que passa a existir tem nome que não se confunde com eles (§7). A premissa implícita do ADR-0005 — *toda* gravação de sessão é automática — é a única coisa supersedida.

### 2. Diretório: `sessions/` ao lado do `session.json`

```
<diretório de estado>/porecatu/
  session.json
  sessions/
    api-front.json
    infra.json
    estudo-rust.json
```

O diretório de estado é o mesmo que o [ADR-0005](0005-persistencia-de-sessao.md) já escolheu por plataforma, pelo mesmo motivo: o arquivo carrega `cwd` absoluto da máquina, geometria em pixels físicos, identidade de monitor. Levar isso para `~/.config/porecatu/`, que o usuário versiona, é levar estado de uma máquina para outra que não o entende.

O diretório é **derivado do caminho resolvido do `session.json`** — `session_dir(resolve_session_path()) / "sessions"` —, e não resolvido de novo por conta própria. A consequência que se quer: `PORECATU_SESSION` ([ADR-0036](0036-formato-do-arquivo-de-sessao.md) §6) desloca os dois juntos, e a costura de teste continua sendo uma só. Não há chave TOML de caminho, pela razão que o arquivo de exemplo já dá para a sessão automática.

O diretório nasce na primeira gravação, nunca no arranque. Ausente, a lista é vazia — não é erro, como o `session.json` ausente não é (RF-3.13).

### 3. Nome exibido dentro do arquivo, nome de arquivo derivado

O nome que o usuário digita é **texto livre**: espaço, acento, `/`, `:`, `?` — tudo o que um nome de grupo aceita. Nome de arquivo não aceita metade disso no Windows, e o que aceita muda de plataforma para plataforma. Fazer o campo recusar caractere seria vazar uma regra de sistema de arquivos para a interface.

Então os dois se separam:

- **O nome exibido vive dentro do arquivo**, num campo `name` de `SessionFileV1` (§4). É ele que a lista mostra e que a comparação de "nome repetido" usa.
- **O nome do arquivo é derivado**: minúsculas, diacríticos removidos, tudo que não for `[a-z0-9]` vira `-`, hífens colapsados e aparados, truncado em 48 bytes; vazio depois disso vira `sessao`. Colisão de *slug* entre nomes **diferentes** (`"API/front"` e `"api front"` dão os dois `api-front`) ganha sufixo `-2`, `-3`, no primeiro livre — a mesma regra do `.corrupt.N` do [ADR-0036](0036-formato-do-arquivo-de-sessao.md) §5.
- **"Nome repetido"** (RF-14.5) é comparado pelo nome exibido, aparado e sem distinguir caixa (`to_lowercase` do Unicode, não só ASCII). Sobrescrever grava **no arquivo que já tem aquele nome**, qualquer que seja o *slug* dele, e não num arquivo novo — senão salvar "Infra" sobre "infra" deixaria dois arquivos com o mesmo nome aos olhos do usuário.
- **Teto do nome: 64 caracteres** (escalares Unicode), aplicado pelo campo. É comportamento, não aparência; o que aparece na tela é truncado pela largura do popover ([ADR-0055](0055-botao-e-popover-de-sessoes.md)), como o nome de grupo é truncado pela largura da pílula.

A função de *slug* é pura e mora em `porecatu-session`, com teste por plataforma do conjunto de nomes reservados do Windows (`con`, `nul`, `com1`…), que ganham o mesmo sufixo `-2`.

### 4. Formato: o mesmo `SessionFileV1`, com um campo opcional e uma janela só

Um arquivo de sessão nomeada é um `SessionFileV1` **com exatamente um elemento em `windows`** e um campo novo:

```rust
struct SessionFileV1 {
    schema_version: u32,                 // continua 1
    windows: Vec<WindowV1>,
    shell_integration_dismissed: bool,
    #[serde(default)]
    name: Option<String>,                // novo: só sessões nomeadas
    #[serde(default)]
    saved_at: Option<u64>,               // novo: segundos desde a época, para ordenar a lista
}
```

**`CURRENT_SCHEMA_VERSION` continua 1.** É a mesma propriedade que o ADR-0036 §1 comprou e que o ADR-0053 §11 já pagou uma vez: campo ausente tem significado definido (`None`), e o `session.json` automático continua gravando os dois como ausentes. Migração, quarentena, recusa de schema mais novo — tudo vem do mesmo código, sem um segundo schema a manter.

O preço, registrado: um arquivo nomeado gravado por esta versão e lido por uma anterior **é aceito** como sessão de uma janela e o nome é ignorado — o que não acontece, porque nenhuma versão anterior sabe onde procurar esse diretório.

`saved_at` e não o `mtime` do arquivo: `mtime` muda por cópia, por sincronização de pasta, por antivírus. A ordem da lista (RF-14.7) é a do gesto do usuário, e o gesto é o que o arquivo registra.

`shell_integration_dismissed` é gravado com o valor do processo e **ignorado** na leitura: a dispensa do convite é do processo ([ADR-0039](0039-convite-a-integracao-de-shell.md)), não da disposição.

### 5. API em `porecatu-session`

Quatro funções novas, todas recebendo o diretório explícito — a mesma forma de `load_from`/`save_to`, que é o que as torna testáveis sem `std::env` — e uma versão sem argumento que resolve o diretório pelo §2:

```rust
pub struct NamedSessionEntry {
    pub name: String,
    pub file: PathBuf,
    pub saved_at: Option<u64>,
    pub status: EntryStatus,     // Ok | Unreadable | NewerSchema { found }
}

pub fn list_named_in(dir: &Path) -> Vec<NamedSessionEntry>;
pub fn load_named(file: &Path) -> LoadOutcome;
pub fn save_named_in(dir: &Path, name: &str, window: WindowV1) -> Result<PathBuf, SaveError>;
pub fn delete_named(file: &Path) -> io::Result<()>;
```

- `list_named_in` lê cada `*.json` do diretório e devolve **uma entrada por arquivo, inclusive os ruins** (RF-14.17): o arquivo inválido vira `Unreadable` com o nome do arquivo como rótulo, e só é movido para `.corrupt` quando o usuário tenta restaurá-lo — listar nunca renomeia nada. Ordenada por `saved_at` decrescente, sem `saved_at` no fim, empate pelo nome.
- `save_named_in` resolve o arquivo pelo §3, recusa sobrescrever arquivo de schema mais novo (`SaveError::NewerSchema`, RF-3.16) e grava pelo mesmo `save_to` atômico.
- **A pergunta "já existe?" não é da API**: a UI a faz sobre a lista que já tem na mão, antes de chamar `save_named_in`, porque é a UI que precisa abrir o diálogo (RF-14.5). A API sobrescreve quando mandada.

`porecatu-session` continua sem GUI e sem PTY; a regra de dependência da [arquitetura](../arquitetura.md) não ganha aresta.

### 6. Restaurar é `open_window_from_session`, com duas diferenças explícitas

A restauração nomeada chama o **mesmo** `open_window_from_session` que o arranque chama por janela. É isso que faz a terceira métrica do PRD-014 — *zero* diferença de comportamento entre as duas — ser verdade por construção e não por disciplina: `SpawnOrigin::Restored`, `lazy_restore`, `TabState::NotStarted` ([ADR-0037](0037-aba-nao-iniciada.md)), os painéis irmãos por `spawn_split_pane_runtime` e o `.porecatu` uma vez por aba no painel focado ([ADR-0051](0051-arquivo-de-projeto-porecatu.md), revisto pelo ADR-0053 §12) vêm todos de graça, e qualquer correção futura num vale para o outro.

As duas diferenças entram como **parâmetro**, nunca como caminho paralelo:

1. **Posição.** O arranque posiciona pela geometria e monitor gravados (RF-3.11). A nomeada usa o **tamanho** gravado e a **posição em cascata** da janela de onde o gesto partiu, grampeada no monitor dela — a mesma conta de `open_window`, extraída para uma função que as duas chamam. Posição gravada abriria a janela exatamente em cima de outra, ou num monitor que o usuário não está olhando (RF-14.12). O parâmetro é um `enum WindowPlacement { Saved, CascadeFrom(WindowId) }`, e o arranque passa `Saved`.
2. **Tema e zoom não são aplicados.** Eles são aplicados no arranque por `apply_restored_session_state`, que exige rodar **antes** de qualquer janela existir, porque o zoom decide o `cell_metrics` global do processo. A restauração nomeada simplesmente não a chama (RF-14.13). O [ADR-0031](0031-temas-nomeados.md) tornou tema e zoom estado de sessão, e continuam sendo — da sessão automática.

A leitura do arquivo e a construção da janela acontecem **na main thread**, no tratamento do clique. Um `WindowV1` é alguns kilobytes de JSON; ler e converter custa menos que o frame que a janela nova vai desenhar. O que é caro — os shells subindo — já é assíncrono por `porecatu-term`.

### 7. Ações novas no catálogo

| Ação | O que faz | Default |
|---|---|---|
| `session.save_named` | Abre o popover de sessões com o campo de nome em foco, para salvar a janela ativa (RF-14.1, RF-14.2) | `Ctrl+Shift+S` · `Cmd+Shift+S` no macOS |
| `session.open_list` | Abre o popover de sessões com a primeira linha realçada (RF-14.7) | **nenhum** — precedente de `group.new_tab` e `pane.close` |

`Ctrl+Shift+S` está livre nos defaults embutidos das três plataformas (conferido em `crates/porecatu-config/src/keybindings.rs`), e é a convenção de "salvar como" que o usuário já traz de outros programas.

**Restaurar uma sessão específica e excluí-la não são ações**: exigiriam argumento (qual sessão?), e ação com argumento não é vinculável a tecla ([docs/reference/acoes.md](../reference/acoes.md), Convenções). Entram na tabela de "superfícies de mouse e de modal, que não são ações" do catálogo, como o clique no indicador de commits do [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md).

Os nomes não colidem com os que o catálogo diz que não existem — `session.save`/`session.restore` continuam na lista de ausentes, agora com a razão completada por uma nota que aponta para este ADR.

### 8. `[session] enabled` não alcança as nomeadas, e o modo posicional também não

`enabled = false` desliga a gravação e a leitura do `session.json` — e **só** dele. As sessões nomeadas seguem gravadas e restauradas por gesto (RF-14.14): quem desliga a sessão automática quer abrir o app limpo, não perder o jeito de montar o projeto X quando pedir.

O mesmo vale para o modo posicional do [ADR-0040](0040-superficie-de-linha-de-comando.md) (`porecatu <caminho>`), que é, por decisão daquele ADR, *"o mesmo caminho de código de `[session] enabled = false`"*: lá também as nomeadas funcionam, porque não são a sessão que ele promete não ler nem sobrescrever.

Com `enabled = true`, a janela aberta por uma sessão nomeada **é uma janela como outra qualquer** a partir do momento em que existe: entra no `session.json` na próxima gravação, e reaparece no próximo arranque pela sessão automática. Nenhum vínculo com o arquivo nomeado sobrevive (RF-14.15) — `WindowState` não guarda de qual sessão nasceu, e é isso que impede o recurso de virar "sincronizar janela com arquivo" por acidente.

**Sem chave nova em `[session]`** e sem classe de recarga nova: nada do recurso é configurável além do atalho, que é `[keybindings]` e já tem classe A ([ADR-0030](0030-escopo-do-hot-reload.md)).

### 9. Onde o código mora

| Crate | O que muda |
|---|---|
| `porecatu-session` | `name` e `saved_at` em `SessionFileV1`; módulo `named.rs` com o §3 e o §5 |
| `porecatu-core` | `Action::SessionSaveNamed`, `Action::SessionOpenList` |
| `porecatu-config` | o default `ctrl+shift+s`/`cmd+shift+s` |
| `porecatu-ui` | `WindowPlacement` em `open_window_from_session`; o popover e o botão do [ADR-0055](0055-botao-e-popover-de-sessoes.md) |
| `porecatu-term`, `porecatu-pty`, `porecatu-render` | **nada** (o ícone novo do ADR-0055 é uma constante em `porecatu_render::icon`, não código de render) |

## Alternativas consideradas

### Um arquivo só, `sessions.json`, com um mapa de nome para janela

Uma leitura, uma escrita, e a lista sai de graça. Recusada porque transforma cada gravação numa reescrita de **todas** as sessões — um arquivo corrompido leva todas junto, e a quarentena do RF-3.14 passaria a guardar o trabalho inteiro do usuário num `.corrupt`. Um arquivo por sessão falha uma de cada vez, e é o que o RF-14.17 pede.

### Formato próprio, mais enxuto

Um `NamedSessionV1 { name, window: WindowV1 }` sem os campos do envelope. Mais limpo de ler, e é exatamente um segundo schema com versão própria, migração própria e quarentena própria. Recusada pela terceira métrica do PRD-014: o jeito de não divergir é não ter o que divergir.

### Guardar no diretório da config

É "do usuário", e o usuário poderia versionar os arquivos. Recusada pelo mesmo motivo do [ADR-0005](0005-persistencia-de-sessao.md): o arquivo carrega `cwd` absoluto, pixels físicos e identidade de monitor — estado da máquina. Quem quiser copiá-lo para outra máquina sabe onde está, e o `cwd` ou existe lá ou cai no fallback do RF-3.10.

### O nome do arquivo é o nome da sessão

Sem *slug*, sem campo `name`, a lista é um `read_dir`. Recusada porque obriga o campo a recusar caractere — e a regra do que recusar é diferente no Windows, no macOS e no Linux, então o mesmo nome valeria numa máquina e não noutra.

### Restaurar pelo caminho do arranque inteiro, tema e zoom inclusos

Um caminho só, literalmente. Recusada porque `apply_restored_session_state` só é correta antes de qualquer janela existir, e aplicá-la depois trocaria tema e métrica de célula de **todas** as janelas abertas por um gesto que o usuário fez para abrir **uma**. O que se compartilha é a construção da janela; o que é do processo fica com o processo.

### Restaurar na posição gravada

Coerente com o RF-3.11. Recusada porque o arranque restaura num desktop vazio e a nomeada restaura num desktop ocupado: a posição gravada é, com frequência, exatamente a de uma janela que já está aberta — muitas vezes a própria de onde a sessão foi salva.

### Uma ação `session.restore_named` com argumento, para menus futuros

Deixaria o gesto de restaurar nomeado no catálogo. Recusada: ação `Arg` só existe no catálogo quando alguma superfície a invoca por nome (`tab.move_to_group`, `group.set_color`), e o popover não precisa — ele chama a restauração diretamente, como o clique no indicador de commits chama a integração.

### Sessões nomeadas dependentes de `[session] enabled`

Uma chave governando tudo que é "sessão". Recusada pelo dono do produto, e com razão: quem desliga a sessão automática está dizendo "não quero que o app lembre sozinho", e não "não quero poder pedir".

## Consequências

### Positivas

- **Zero schema novo, zero dependência nova, zero configuração nova.** O arquivo nomeado é o arquivo de sempre com uma janela, e o que valida, migra e põe em quarentena o `session.json` faz o mesmo com ele.
- A restauração nomeada e a do arranque são **o mesmo código**, e o `.porecatu`, os painéis, o `lazy_restore` e o fallback de `cwd` não podem divergir entre elas sem alguém escrever um `if` deliberado.
- Um arquivo por sessão falha sozinho: um arquivo ruim é uma linha esmaecida na lista, não a lista inteira.
- A decisão de que a sessão automática não tem gesto manual (ADR-0005) sai intacta — e mais clara, porque agora há um contraste escrito entre as duas.

### Negativas

- **Dois objetos com a palavra "sessão"** passam a existir para o usuário, com regras diferentes: um que o app grava sozinho e que `enabled` desliga, outro que ele grava por gesto e que `enabled` não alcança. O guia do usuário precisa dizer isso numa frase, e o risco de confusão é real.
- `SessionFileV1` ganha dois campos que o `session.json` automático nunca preenche. O tipo passa a descrever dois usos, e a invariante "nomeada tem exatamente uma janela" é da função que grava, não do tipo.
- **Duas seções de dois ADRs aceitos** ficam parcialmente supersedidas, mais a linha do catálogo de ações revista.
- Arquivo nomeado carrega `cwd` absoluto. Copiado para outra máquina, restaura com tudo no diretório inicial e uma nota por aba — correto, mas não útil.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Alguém dar à restauração nomeada um caminho próprio "porque é mais simples" e ela divergir da do arranque | Média | **Alto** | §6: diferenças como parâmetro de `open_window_from_session`, nunca função paralela; teste que restaura o mesmo `WindowV1` pelos dois caminhos e compara o `Workspace` resultante |
| Aplicar tema e zoom do arquivo por reaproveitar o caminho do arranque inteiro | Média | Médio | §6 item 2, e a restauração nomeada não chama `apply_restored_session_state`; o comentário da função já diz que ela só vale antes da primeira janela |
| Dois nomes diferentes com o mesmo *slug* sobrescreverem um ao outro | Média | **Alto** | Sufixo `-N` na colisão, e sobrescrita resolvida pelo **nome exibido**, não pelo *slug* (§3); teste com `"API/front"` e `"api front"` |
| Nome reservado do Windows (`con`, `nul`) virar arquivo impossível de criar | Baixa | Médio | Lista de reservados na função de *slug*, com teste |
| Listar mover arquivo para `.corrupt` só por abrir o popover | Baixa | Médio | §5: listar nunca renomeia; a quarentena acontece na tentativa de restaurar, como no arranque |
| Usuário confundir "desliguei a sessão" com "desliguei as sessões nomeadas" | Média | Baixo | RF-14.14 e §8; o comentário de `[session] enabled` no arquivo de exemplo passa a dizer que as nomeadas não são afetadas |
| Diretório de sessões com centenas de arquivos deixar a abertura do popover lenta | Baixa | Baixo | Leitura só ao abrir o popover (RF-14.18); um `WindowV1` é pequeno. Se virar problema, ler só o cabeçalho (`name`, `saved_at`) é otimização local de `list_named_in` |
