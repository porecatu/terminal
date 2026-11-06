# ADR-0088 — Ícone da aba fixada: seção reservada `[icon]` no `.porecatu`, `icon.png`, ícone do app; resolução e decodificação fora da main thread

**Status:** Aceito
**Data:** 2026-11-06
**Relacionados:** [ADR-0007](0007-modelo-de-threading.md), [ADR-0051](0051-arquivo-de-projeto-porecatu.md), [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md), [ADR-0061](0061-imagem-de-fundo-do-terminal.md), [ADR-0079](0079-botao-de-menu-do-app.md), [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md), [ADR-0087](0087-zona-de-abas-fixadas-na-barra.md), [PRD-012](../prd/prd-012-comando-de-projeto-por-diretorio.md), [PRD-017](../prd/prd-017-imagem-de-fundo-do-terminal.md), [PRD-021](../prd/prd-021-abas-fixadas.md)
**Supersedes (parcial):** ADR-0051 §2 e §3 (o formato ganha um nome de seção reservado, que não é shell) e §8 (o item "metadados por seção" ganha uma exceção de arquivo, não de seção)

## Contexto

O [PRD-021](../prd/prd-021-abas-fixadas.md) dá a cada aba fixada um ícone, escolhido no diretório gravado ao fixar (RF-21.17): o arquivo declarado na seção `[icon]` do `.porecatu`; senão um `icon.png` no diretório; senão o ícone do app. O [ADR-0087](0087-zona-de-abas-fixadas-na-barra.md) decidiu onde ele é desenhado — centrado na caixa, com lado `icon_em_size`, por `Primitive::Image`, como o ícone do botão do app. Falta o resto, e cada parte tem uma resposta errada tentadora:

1. **O formato de `[icon]`.** O `.porecatu` é feito de seções cruas nomeadas **pelo shell** ([ADR-0051](0051-arquivo-de-projeto-porecatu.md) §2 e §3), e o §8 do mesmo ADR recusou "metadados por seção". Uma seção que não é shell entra num formato que não tem lugar para ela, e o nome dela pode colidir com o de um shell.
2. **O caminho.** Um caminho escrito num arquivo que veio com o projeto é lido pelo app sem o usuário olhar. No Windows, um caminho UNC (`\\servidor\pasta\x.png`) faz o sistema abrir uma conexão SMB com aquele servidor **e oferecer as credenciais da sessão**; abrir o arquivo já é o vazamento.
3. **A confiança.** O ADR-0051 §4 lê o `.porecatu` só sob `trusted_paths`, e o PRD diz que o `[icon]` é lido com ou sem ele (RF-21.19).
4. **O custo.** O arranque passa a resolver e decodificar N ícones, com N arbitrário, de imagens de tamanho arbitrário — e a main thread não faz I/O ([ADR-0007](0007-modelo-de-threading.md)).
5. **A memória.** Uma imagem de 4000 px decodificada para um quadrado de 20 px é 64 MB de RGBA, e textura em GPU integrada é RAM do processo ([CLAUDE.md](../../CLAUDE.md), armadilha do `MemoryHints`).

## Decisão

**`[icon]` é o único nome de seção reservado do `.porecatu`: o corpo é um caminho, que não pode ser de rede nem de dispositivo. A cadeia `[icon]` → `icon.png` → ícone do app é resolvida e decodificada numa thread, no arranque e ao fixar, com a imagem reduzida a um teto pequeno antes de virar textura. Uma textura por arquivo, compartilhada no processo, e o ícone do app no lugar até o resultado chegar.**

### 1. `[icon]`: seção reservada, corpo é um caminho

`[icon]` usa a sintaxe de cabeçalho que o formato já tem — a mesma regex do ADR-0051 §2, insensível a maiúsculas —, e por isso **nenhum `.porecatu` existente muda de leitura**. O que muda é o significado do nome:

- **`icon` é reservado.** Não é shell, e `command_for` **nunca** o devolve: um shell cujo programa se chame `icon` cai em `[default]`, como um shell sem seção. A lista de nomes reservados é uma constante em `porecatu-config::project_file` (`RESERVED_SECTIONS = ["icon"]`) e tem um elemento; nome reservado novo exige ADR.
- **`[default]` não substitui `[icon]`.** `[default]` é "o script para qualquer shell", e um ícone não é script.
- **O corpo é a primeira linha não vazia, aparada nas pontas.** As demais linhas são ignoradas — o formato não tem comentário dentro de seção (ADR-0051 §2), e linha a mais num corpo de caminho é erro de quem escreveu, não ambiguidade a resolver. Corpo vazio é o mesmo que `[icon]` ausente.
- **Duas seções `[icon]`:** a primeira vence, a regra que o parser já aplica a todo nome repetido.

**Por que não reabre o ADR-0051 §8.** O que o §8 recusou foram metadados **por seção** — `title`, `working_dir`, `enabled` dentro de cada `[pwsh]`, `[bash]` —, cada um uma chave a manter num formato só nosso, sem pedido. `[icon]` é um dado **do arquivo**, não de uma seção, e tem requisito aprovado. A recusa do §8 continua valendo para o que ela nomeia.

**Por que não `icon.png` sozinho.** Seria zero formato novo. Mas o projeto que já tem um logo — em `assets/`, num nome que outra ferramenta exige — teria de duplicá-lo na raiz com outro nome. `[icon]` existe para apontar o arquivo que o projeto já tem, e `icon.png` existe para quem não quer escrever nada.

### 2. O caminho: local, de arquivo regular, sem rede

Resolução, nesta ordem:

1. `~` inicial expandido por `expand_tilde`, a função que `trusted_paths` já usa.
2. Caminho relativo é juntado ao **diretório do `.porecatu`** (o diretório gravado da fixada); absoluto é aceito como está.
3. **Recusa textual**, antes de qualquer chamada ao sistema de arquivos: caminho que comece com `\\` ou `//` — UNC, `\\?\`, `\\.\` (dispositivo) — é recusado. A checagem é sobre o texto porque **tocar** um caminho UNC no Windows já dispara a conexão e a autenticação: `canonicalize`, `metadata` e `File::open` vazariam a credencial antes de qualquer verificação posterior.
4. O alvo tem de ser **arquivo regular** (`metadata().is_file()`): diretório, *pipe*, `/dev/zero` e afins são recusados.

**Não há restrição ao diretório do projeto.** Um `[icon]` pode apontar para `~/icones/api.png`. Restringir à árvore do projeto fecharia o caminho UNC também, mas o passo 3 já o fecha, e ler uma imagem local que o usuário pode ler não executa nada — o pior que um `[icon]` hostil consegue é mostrar uma imagem que não devia. Unidade de rede mapeada (`Z:`) não é distinguível de uma local pelo texto e é aceita: foi o usuário quem a mapeou.

Qualquer recusa, arquivo ausente ou ilegível faz a cadeia seguir para `icon.png` (RF-21.20).

### 3. Confiança: quem lê é a fixada, e a fixada já é a autorização

O ícone só existe para abas **fixadas**, e o [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md) §6 decidiu que fixar autoriza o diretório gravado. Então a leitura do `.porecatu` para o ícone usa o mesmo `Trust::Pinned` da execução do script, e a pergunta "o `[icon]` é lido fora de `trusted_paths`?" (RF-21.19) não precisa de uma regra própria: ele nunca é lido para um diretório que não seja de uma fixada.

`[project_file] enabled = false` desliga a leitura do `.porecatu` antes de qualquer acesso ao disco, como sempre (RF-12.7). Nesse caso a cadeia começa em `icon.png`, que não é parte do `.porecatu` (RF-21.19).

A função é separada de `resolve` — `porecatu_config::project_file::resolve_icon(dir, config) -> Option<PathBuf>` —, porque o ícone e o script têm desfechos diferentes: o script tem nota de "não autorizado", seção ausente e arquivo ilegível, cada um com uma mensagem; o ícone tem um caminho ou nada.

### 4. Resolução e decodificação numa thread, com resultado chaveado

Nada da cadeia roda na main thread: nem ler o `.porecatu`, nem testar se `icon.png` existe, nem decodificar. A main thread manda o pedido e recebe **dado chaveado**, nunca comando — a regra que o [ADR-0052](0052-sincronizacao-com-o-remoto-do-git.md) fixou para todo resultado de thread:

```rust
struct PinnedIconRequest {
    key: PinnedKey,             // ADR-0086 §2
    dir: Option<PathBuf>,       // None: fixada SSH, ou diretório que não existe mais
    project_file_enabled: bool,
}

enum PinnedIconSource { Declared(PathBuf), IconPng(PathBuf), App }

struct PinnedIconResult {
    key: PinnedKey,
    source: PinnedIconSource,
    image: Option<DecodedImage>,   // None com `App`
}
```

A thread percorre a cadeia **inteira**: tenta o `[icon]` (§1, §2); se não deu imagem, `icon.png` no diretório, nome exato; se também não, devolve `App`. Um passo que falha — caminho recusado, arquivo ausente, formato não suportado, imagem corrompida — só passa ao seguinte, **sem aviso** (RF-21.20): o ícone do app no lugar já é o sinal visível, e um aviso por fixada a cada arranque seria ruído pelo mesmo motivo que o ADR-0051 §5 deu para não repetir nota.

**Decodificação:** `background_image::decode`, a mesma função da imagem de fundo e do ícone do app — formato pelo conteúdo, só PNG e JPEG ([PRD-017](../prd/prd-017-imagem-de-fundo-do-terminal.md)), limites padrão do crate `image`, mips gerados. Um pânico do decodificador é pego como em `spawn_load` e vira falha daquele passo.

**Teto de tamanho:** `PINNED_ICON_MAX_DIM = 128` px no lado maior, passado como `max_dim` ao `decode`, que já reduz mantendo a proporção. É **valor de implementação, não de aparência**: o ícone é desenhado a `icon_em_size` (20 px lógicos), e 128 cobre uma escala de 400% (80 px físicos) com um nível de mip acima. Custa uns 87 KB de textura por ícone, contra os 64 MB que uma imagem de 4000 px custaria sem teto. O botão do app continua subindo o PNG embutido inteiro, como o ADR-0079 decidiu — é um arquivo nosso, de tamanho conhecido.

**Quando:** no arranque, **um pedido em lote** para todas as fixadas da primeira janela, numa thread só, depois de `open_pinned_tabs` ([ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md) §5) e nunca antes do primeiro frame; e um pedido ao **fixar**, para aquela aba. Em nenhum outro momento (RF-21.21): o ícone não segue o `cd`, não vigia o arquivo, não muda no hot reload. Fixada SSH e fixada cujo diretório não existe mais nem viram pedido: são `App` direto.

**Até chegar:** a caixa desenha o ícone do app (RF-21.21). Resultado que chega para uma `PinnedKey` que não existe mais — a aba foi desafixada ou fechada no meio — é descartado. Resultado que chega antes de haver GPU fica guardado decodificado e sobe quando o `GpuContext` nasce, como a imagem de fundo já faz.

### 5. Texturas: uma por arquivo, do processo

As texturas vivem no `GpuContext` do processo, como a do ícone do app e a da imagem de fundo: um `ImageId` serve a toda janela. O cache é por **caminho resolvido** — duas fixadas no mesmo projeto, ou dois projetos que apontam o mesmo `[icon]`, dividem uma textura —, com contagem de uso pelas `PinnedKey` que a usam. Desafixar ou fechar a última fixada que usava uma textura a solta (`remove_image`). `App` não ocupa entrada no cache: é o `app_icon_image` que o botão do app já tem.

Por que cache por caminho e não por fixada: o caso comum de várias fixadas sem ícone próprio cai todo em `App`, que já não custa nada; o segundo mais comum é o mesmo projeto fixado duas vezes (o servidor e o *watcher*), que é exatamente o par que o cache por caminho junta.

### 6. Desenho

O ADR-0087 §2 já decide caixa, estados e alfa. Duas regras do ícone em si:

- **Proporção preservada**, encaixado (*contain*) no quadrado de `icon_em_size` e centrado nele. Imagem retangular sobra em transparência dos dois lados, e o fundo da caixa aparece.
- **Sem máscara arredondada nem recorte**: `mask_radius = 0`, como o ícone do botão do app. Um raio para o ícone seria valor novo, e o ícone de um projeto pode já ter a forma dele.

### 7. Onde o código mora

| Crate | O que muda |
|---|---|
| `porecatu-config` | `RESERVED_SECTIONS`; `command_for` pula reservados; `ProjectScript::icon_path()`; `resolve_icon`, com o §2 e o §3 |
| `porecatu-ui` | módulo `pinned_icon.rs`: pedido, thread, cadeia, resultado chaveado, cache de texturas; `PINNED_ICON_MAX_DIM`; reuso de `background_image::decode` |
| `porecatu-render` | **nada** — `Primitive::Image` com alfa já existe |
| `porecatu-core`, `porecatu-session`, `porecatu-term`, `porecatu-pty`, `porecatu-ssh` | **nada** |

Nenhuma dependência nova: `image` (PNG e JPEG) já está em `porecatu-ui`.

Documentação no mesmo PR da implementação: a [referência do arquivo de projeto](../reference/arquivo-de-projeto.md) ganha a seção `[icon]` — o nome reservado, o corpo, os caminhos aceitos e recusados, a ordem da cadeia e o "não muda até o próximo arranque" —, e o [guia do usuário](../guia-do-usuario.md) a menciona junto das abas fixadas.

## Alternativas consideradas

### Só `icon.png`, sem `[icon]`

Nenhum formato novo, nenhuma seção reservada. Recusada pelo pedido do dono, que nomeou os dois, e pela razão do §1: o projeto que já tem o logo em outro lugar teria de duplicá-lo.

### Uma chave num formato próprio, fora das seções (`icon = assets/logo.png` no preâmbulo)

Não reservaria nome de seção nenhum. Recusada porque o preâmbulo é, por decisão do ADR-0051 §2, **comentário** — ignorado inteiro. Dar significado a uma linha dele quebraria a única garantia que o preâmbulo tem, e um `.porecatu` existente cujo comentário diga "icon = ..." mudaria de comportamento.

### `[icon]` lido também pelo `[default]` ou por apelidos (`[logo]`, `[image]`)

Recusada pelo mesmo motivo do ADR-0051 §3 para os apelidos de shell: conhecimento que o app teria de manter e que ninguém pediu.

### Restringir o caminho à árvore do diretório do projeto

Fecharia de uma vez UNC, dispositivo e qualquer leitura fora do projeto. Recusada porque o ícone pode viver numa pasta de ícones do usuário (`~/icones/`), e o passo 3 do §2 já fecha o que importa — a rede — sem impedir o uso legítimo.

### Avisar quando o ícone declarado falha

"Ignorar em silêncio é indistinguível de um recurso quebrado" ([ADR-0030](0030-escopo-do-hot-reload.md)). Recusada porque aqui a falha não é silenciosa: o ícone do app aparece no lugar, na barra, onde o usuário está olhando. Um aviso por arranque, por fixada, seria a nota repetida que o ADR-0051 §5 já recusou.

### Decodificar na main thread, no arranque

Mais simples, e uma imagem pequena decodifica em poucos milissegundos. Recusada porque o tamanho da imagem é do projeto, não nosso, e N fixadas multiplicam o custo antes do primeiro prompt — a armadilha "carga pesada no arranque" do CLAUDE.md.

### Sem teto de tamanho, deixando os mips e o render reduzirem

O mesmo caminho da imagem de fundo. Recusada pela memória: a imagem de fundo é uma, do tamanho da janela, escolhida pelo usuário; os ícones são N, do tamanho de um quadrado de 20 px, escolhidos pelos projetos.

### Ícone que acompanha o `cwd` atual

Recusada pelo PRD (RF-21.17, "Fora de escopo"): o diretório é o gravado ao fixar.

## Consequências

### Positivas

- Nenhum `.porecatu` existente muda de leitura: a sintaxe é a de sempre, e só um shell chamado `icon` — que não existe no fluxo — perde a seção própria.
- A main thread não toca o disco por causa de ícone, e o arranque não espera ícone nenhum.
- Uma textura por arquivo, com teto de 128 px: o custo de memória de dez fixadas é menor que o de um único ícone sem teto.
- Nenhum valor visual novo, nenhuma dependência nova, `porecatu-render` intocado.
- A leitura do `[icon]` não abre um segundo caminho de confiança: é a mesma autorização da fixada.

### Negativas

- **O formato do `.porecatu` ganha o primeiro nome reservado**, e com ele a regra de que um nome de seção pode não ser shell. A [referência](../reference/arquivo-de-projeto.md) precisa dizer isso.
- **Um `[icon]` que falha não diz por quê.** Quem errou o caminho vê o ícone do app e precisa conferir o arquivo; não há nota.
- **Trocar o ícone exige reiniciar o app** (ou desafixar e fixar de novo).
- **Ler um caminho declarado pelo projeto** é uma superfície nova, ainda que só de leitura, e a regra do §2 passo 3 é crítica: errar a checagem textual reabre o vazamento de credencial.
- O teto de 128 px deixa um ícone muito detalhado mais suave em escalas acima de 400%.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| `[icon]` com caminho UNC vazar credencial NTLM ao abrir | Média | **Alto** | Recusa **textual** de `\\` e `//` antes de qualquer chamada ao sistema de arquivos (§2); teste com `\\servidor\x.png`, `//servidor/x.png`, `\\?\UNC\...` e `\\.\pipe\...`, conferindo que nenhuma função de `std::fs` é chamada |
| Caminho apontar para dispositivo ou arquivo gigante e travar a thread | Baixa | Baixo | `is_file()` recusa dispositivo; limites padrão do `image`; a thread é de vida curta e a main não espera por ela |
| Decodificador entrar em pânico com imagem malformada | Baixa | Baixo | `catch_unwind`, como em `spawn_load`; vira falha do passo |
| Resultado de thread aplicado a uma fixada já desafixada | Média | Baixo | Resultado chaveado por `PinnedKey`; chave ausente descarta |
| Textura vazada depois de desafixar | Média | Médio | Contagem de uso no cache; teste de que desafixar a última fixada de um caminho solta o `ImageId` |
| Shell chamado `icon` perder a seção | Baixa | Baixo | Cai em `[default]`; registrado na referência |
| Arranque mais lento com muitos ícones | Baixa | Baixo | Lote numa thread só, depois do primeiro frame; o primeiro prompt não espera |
