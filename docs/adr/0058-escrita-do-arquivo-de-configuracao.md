# ADR-0058 — Escrita do arquivo de configuração: `toml_edit`, edição por chave e gravação atômica

**Status:** Aceito
**Data:** 2026-10-02
**Relacionados:** [ADR-0003](0003-formato-de-configuracao.md), [ADR-0009](0009-referencia-visual-e-reconciliacao.md), [ADR-0010](0010-licenciamento.md), [ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0031](0031-temas-nomeados.md), [ADR-0036](0036-formato-do-arquivo-de-sessao.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0059](0059-janela-de-configuracoes.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md)

## Contexto

O [PRD-016](../prd/prd-016-tela-de-configuracoes.md) pede uma tela que grave no `porecatu.toml`. Até aqui **nada no projeto escreve esse arquivo**: `porecatu-config` só lê (`load`, `parse`, `lib.rs`), `Config` e todas as suas seções derivam só `Deserialize`, e o único `fs::write` sobre ele é o da engrenagem criando o arquivo a partir do exemplo embutido (`ensure_config_file_exists`, `porecatu-ui`). O [ADR-0009](0009-referencia-visual-e-reconciliacao.md) §6 já tinha nomeado o problema quando adiou o painel para o `[v2]`: *"o painel precisa preservar comentários e formatação ao regravar o arquivo — problema conhecido, resolvível com um parser que preserva a árvore sintática"*.

Cinco perguntas que este ADR fecha:

1. **Com que biblioteca.** O `toml` 1.1 do workspace desserializa para `serde` e descarta comentário, espaço e ordem — gravar a partir dele reescreveria o arquivo inteiro.
2. **Qual é a unidade de edição.** Serializar um `Config` modificado, ou aplicar mudanças chave a chave sobre o texto do usuário.
3. **Como gravar** sem deixar um arquivo pela metade, e sem quebrar o link simbólico de quem aponta o arquivo para um repositório de dotfiles.
4. **Como garantir** que a tela nunca grave algo que o app recusaria.
5. **Onde gravar os atalhos**, que têm três tabelas com precedência ([ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md)).

## Decisão

**`toml_edit` entra como dependência direta de `porecatu-config`; a tela produz uma lista de edições por caminho de chave, aplicadas sobre o documento do usuário, revalidadas pelo mesmo `parse` da carga e gravadas atomicamente no alvo real do arquivo. `Config` não ganha `Serialize`.**

### 1. `toml_edit`, e por que não é uma família nova

`toml_edit` é o parser de TOML que preserva a árvore sintática — comentários, espaços, ordem de chaves e tabelas, forma da chave (tabela, tabela inline, chave pontilhada) e decoração de cada valor. É do mesmo projeto (`toml-rs`) e implementa a mesma especificação 1.1 do `toml` que o workspace já usa; licença MIT/Apache-2.0, compatível com a GPLv3 ([ADR-0010](0010-licenciamento.md)).

**Ele já está no grafo**: `toml_edit 0.25` entra no `Cargo.lock` hoje como dependência de compilação (`proc-macro-crate`, via `num_enum` → `android-activity` → `winit`). A entrada direta não traz crate novo ao build das plataformas onde ele já compila, e vale a regra das outras dependências do projeto: **versão pinada no `Cargo.toml` do crate**, subida como tarefa própria. É a primeira dependência nova desde o `win32job` e entra na tabela de stack do README.

Só `porecatu-config` depende dele. `porecatu-ui` recebe tipos do próprio `porecatu-config` — nenhum tipo do `toml_edit` atravessa a fronteira, a mesma disciplina do `alacritty_terminal` em `porecatu-term`.

### 2. A unidade é a edição por chave, não o `Config`

Um módulo novo, `porecatu-config::edit`, com três tipos:

- **`KeyPath`** — o caminho pontilhado de uma chave (`terminal.font.size`, `keybindings.windows."ctrl+shift+o"`), com a mesma grafia que os avisos de chave desconhecida já usam ([`serde_ignored`](https://docs.rs/serde_ignored)).
- **`EditValue`** — `Bool`, `Integer(i64)`, `Float(f64)`, `String`, `StringList(Vec<String>)`, `StringMap(BTreeMap<String, String>)`. Tipado; nunca texto de interface ([ADR-0056](0056-catalogo-de-textos-da-interface.md)). `Float` é sempre escrito com parte decimal (`16.0`, nunca `16`), porque o campo do `Config` é `f64` e o arquivo de exemplo escreve assim.
- **`Edit`** — `Set(KeyPath, EditValue)` ou `Remove(KeyPath)`.

E um documento: **`ConfigDocument`**, que guarda o texto lido (`base`) e o `toml_edit::DocumentMut` dele, com `apply(&[Edit]) -> Result<String, EditError>` devolvendo o texto novo.

As regras de aplicação são as que fazem o [PRD-016](../prd/prd-016-tela-de-configuracoes.md) RF-16.17 ("byte a byte fora das chaves com pendência") verdadeiro:

- **`Set` sobre chave existente troca só o valor**, onde quer que ele esteja escrito — tabela, tabela inline ou chave pontilhada — e **herda a decoração do valor antigo**: o espaço antes e o comentário de fim de linha (`size = 14.0   # RF-5.3` vira `size = 16.0   # RF-5.3`).
- **`Set` sobre chave ausente** a acrescenta ao fim da tabela dela, com a decoração das chaves irmãs; tabela ausente é criada ao fim do documento, precedida de uma linha em branco. Nunca reordena o que existe.
- **`Remove`** tira a linha da chave e **preserva o bloco de comentário acima dela**, transferindo-o para o item seguinte da mesma tabela (ou para o fim da tabela, se ela era a última). No arquivo criado a partir do exemplo, cada chave tem a documentação dela em comentário logo acima; "Restaurar padrão" que levasse junto a documentação apagaria exatamente o que faz o arquivo de exemplo valer a pena.
- **`StringMap`** (`[shell.env]`) é aplicado como edições por chave dentro da tabela: entradas que ficam conservam linha e comentário, as novas entram ao fim, as que saíram são removidas pela regra acima.
- **Final de linha**: o texto novo usa o do texto lido (CRLF ou LF), detectado na primeira quebra.

**Por que não `Serialize` no `Config`.** Serializar o `Config` inteiro reescreveria todas as ~350 chaves com os defaults explícitos, apagaria todo comentário e, pior, congelaria no arquivo o default de hoje: uma chave que o usuário nunca escreveu deixaria de acompanhar uma mudança de default numa versão futura. Serializar só as seções tocadas e mesclar no documento é a mesma edição por chave com um passo a mais e uma fonte de erro a mais — `skip_serializing_if` em dezenas de structs para não escrever o que não mudou. A lista de edições é o que a tela produz de qualquer forma (uma pendência por linha alterada) e é o que a **mescla com o arquivo alterado fora** precisa (§3).

### 3. Gravação atômica, alvo real, e conflito por conteúdo

- **Atômica**, como a sessão ([ADR-0036](0036-formato-do-arquivo-de-sessao.md) §5, `porecatu_session::save_to`): `<nome>.tmp` no mesmo diretório, `sync_all`, `rename` sobre o final. Um crash no meio deixa o arquivo anterior intacto.
- **No alvo real do link simbólico.** Quem versiona dotfiles costuma ter `porecatu.toml` como link para o repositório; `rename` sobre o link o substituiria por um arquivo comum e desligaria o arquivo do repositório em silêncio. A gravação resolve o caminho (`fs::canonicalize`) e escreve o temporário **ao lado do alvo**. O watcher do [ADR-0030](0030-escopo-do-hot-reload.md) continua assistindo o diretório do caminho resolvido pela carga; o `.tmp` não é `porecatu.toml` e não dispara recarga.
- **Arquivo inexistente**: o documento base é o **arquivo de exemplo embutido** (que `porecatu-ui` já carrega por `include_str!` para a engrenagem e passa como texto), e as edições se aplicam sobre ele (RF-16.21). `porecatu-config` não embute o exemplo — continua sem saber de onde o texto veio.
- **Conflito por conteúdo, não por `mtime`.** `ConfigDocument` guarda o texto lido (`base`). Antes de gravar, o arquivo é relido: se o texto difere de `base`, houve escrita externa, e a gravação devolve `EditError::Changed` com o texto novo em vez de gravar. Quem decide é a tela (RF-16.23): **Recarregar** troca a base; **Manter** reaplica a mesma lista de edições sobre o texto novo — é o que a edição por chave dá de graça — e grava. `mtime` não serve: tem resolução de segundos em parte dos sistemas de arquivo, muda num `touch` sem mudança de conteúdo e não muda num `git checkout` que restaure o mesmo instante. Depois de gravar, `base` passa a ser o texto gravado, e a recarga que a gravação dispara chega à tela com o mesmo texto: **a própria escrita nunca é lida como conflito**.

### 4. Revalidar com o `parse` da carga, antes do disco

O texto produzido por `apply` passa por **`porecatu_config::parse`** — a mesma função que a carga e a recarga usam — antes de qualquer byte ir ao disco. Se falhar, nada é gravado e o erro tipado (`ConfigErrorKind`, com linha e coluna) sobe para a tela, que o mostra como aviso (RF-16.19). As chaves desconhecidas que o `parse` devolve não bloqueiam: elas já estavam no arquivo, e a tela não as toca.

A validação de **faixa** (tamanho de fonte entre um mínimo e um máximo, opacidade entre 0 e 1) **não** entra aqui nem em `Config`: é regra de edição da tela ([ADR-0059](0059-janela-de-configuracoes.md) §4), e um arquivo escrito à mão com um valor fora dela continua sendo carregado como hoje. Mudar isso seria mudar a carga, que este ADR não toca.

### 5. Atalhos vão para a tabela da plataforma em uso

Os atalhos efetivos resultam de três níveis ([ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md)): embutidos → `[keybindings]` comum → `[keybindings.<plataforma>]`. A tela grava **só** em `[keybindings.windows]`, `[keybindings.linux]` ou `[keybindings.macos]`, conforme o sistema em que roda:

- é o nível de **maior precedência**, então o que a tela grava é exatamente o que passa a valer, sem depender do que esteja no comum;
- **nunca muda outra plataforma** — o arquivo viaja entre máquinas em dotfiles, e remapear no Windows não pode desfazer o atalho de quem usa o mesmo arquivo num Mac;
- tirar o atalho de uma ação é escrever `"<tecla>" = "none"` para cada tecla que a resolução dá a ela — a convenção que o ADR-0029 já define; "Restaurar padrão" é `Remove` de toda entrada que a tela escreveu para a ação naquela tabela.

O `[keybindings]` comum não é editado pela tela; o que estiver nele continua valendo onde a tabela da plataforma não sobrepõe.

### 6. O que continua igual

- **Quem aplica é a recarga.** Gravar não chama `apply_config_reload` nem troca o `Arc<Config>`; o watcher vê o arquivo mudar e faz o que faz para qualquer editor. Um caminho de aplicação, como o [PRD-016](../prd/prd-016-tela-de-configuracoes.md) mede.
- **A carga não muda.** `load`, `parse`, `LoadResult`, os avisos de chave desconhecida e a precedência de `--config`/`PORECATU_CONFIG` ficam como estão; `edit` é um módulo ao lado, não uma mudança deles.
- **`porecatu-config` continua sem GUI e sem PTY** (regra de dependência do CLAUDE.md): `edit` só conhece texto, caminho e os próprios tipos.

## Alternativas consideradas

### Derivar `Serialize` e regravar o arquivo inteiro

Descartada (§2): apaga comentários — inclusive a documentação do arquivo criado a partir do exemplo —, congela defaults e transforma cada gravação num `git diff` do arquivo inteiro. É exatamente o custo que o ADR-0009 §6 disse que o painel não pode ter.

### Edição textual por linha, sem parser

Procurar `size = ` dentro de `[terminal.font]` e trocar a linha. Não sobrevive a tabela inline, chave pontilhada, chave entre aspas, string multilinha nem a uma tabela declarada em dois lugares; cada caso viraria um bug de corrupção do arquivo do usuário. `toml_edit` já resolve todos eles.

### Um arquivo separado só para o que a tela grava (`porecatu.ui.toml`)

Evitaria tocar no arquivo do usuário, e criaria a segunda fonte de verdade que o ADR-0009 §6 proíbe, com a pergunta "qual dos dois vence?" respondida por precedência invisível. O usuário que edita à mão uma chave que a tela sobrepõe veria a edição dele não ter efeito.

### Validação de faixa dentro de `Config`

Daria uma regra só para a tela e para o arquivo, e mudaria o comportamento da carga para todo usuário com valor fora da faixa no arquivo hoje — de "carrega" para "aviso e default". É uma decisão de produto sobre a carga, não sobre a escrita, e fica fora daqui (§4).

### Atalhos no `[keybindings]` comum

Mais simples de ler no arquivo, e mudaria o atalho de outras plataformas para quem compartilha o arquivo — e, no macOS, nem valeria, porque `[keybindings.macos]` tem precedência sobre o comum e já traz os defaults da plataforma.

## Consequências

### Positivas

- A tela de configurações deixa de ter o problema que a manteve no `[v2]`: o arquivo é regravado sem perder nada fora das chaves editadas.
- A lista de edições é a mesma estrutura para salvar, descartar, restaurar padrão e mesclar com mudança externa — um conceito, quatro gestos.
- Revalidar com o `parse` da carga torna impossível, por construção, a tela gravar um arquivo que o app recusaria.
- Quem usa link simbólico para dotfiles continua com o link.

### Negativas

- Uma dependência direta nova, com a disciplina de pinagem que o `wgpu` e o `alacritty_terminal` já pedem: `toml_edit` ainda é `0.x` e quebra API entre versões.
- Duas bibliotecas de TOML no mesmo crate (`toml` para ler em `Config`, `toml_edit` para editar). Aceito: trocar a leitura para `toml_edit` + `serde` mudaria a carga, que funciona e tem testes.
- `Remove` com transferência de comentário é a regra menos óbvia do módulo e precisa de teste próprio — o caso "última chave da tabela" e o caso "chave seguida de tabela" são os que quebram.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Uma forma de TOML que o `toml_edit` reescreve diferente do original ao aplicar uma edição vizinha | Média | Alto (ruído no `git diff` do usuário, métrica do PRD-016) | Teste de ida e volta: o `porecatu.example.toml` inteiro e um arquivo com tabelas inline, chaves pontilhadas, aspas e CRLF, com **zero** edições, saem byte a byte iguais; com uma edição, o diff tem exatamente as linhas dela |
| `rename` sobre arquivo aberto em editor no Windows falha | Baixa | Médio | Erro de disco vira aviso com a causa e as pendências ficam na janela (RF-16.19); nada é perdido |
| Watcher dispara recarga pelo `.tmp` | Baixa | Baixo | O watcher filtra pelo nome do arquivo de config; teste no `classify` do `reload.rs` |
| Subir `toml_edit` como efeito colateral de outra subida | Média | Médio | Versão pinada; anotado nas armadilhas do CLAUDE.md junto com `wgpu` e `accesskit_winit` |
