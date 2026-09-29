# ADR-0056 — Catálogo de textos da interface: arquivos TOML por idioma em disco, crate `porecatu-locale`, prosa só em `porecatu-ui`

**Status:** Aceito
**Data:** 2026-09-29
**Relacionados:** [ADR-0003](0003-formato-de-configuracao.md), [ADR-0007](0007-modelo-de-threading.md), [ADR-0009](0009-referencia-visual-e-reconciliacao.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0015](0015-multiplas-janelas.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0039](0039-convite-a-integracao-de-shell.md), [ADR-0040](0040-superficie-de-linha-de-comando.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0054](0054-sessoes-nomeadas.md), [ADR-0057](0057-idiomas-nos-artefatos.md), [PRD-015](../prd/prd-015-idioma-da-interface.md)
**Supersedes:** ADR-0040 §1 (**parcial**: só o idioma do texto impresso por `--help` e pelas mensagens de erro de argumento, que passa a inglês fixo; a superfície, a semântica e o parsing não mudam)

## Contexto

O [PRD-015](../prd/prd-015-idioma-da-interface.md) pede que o idioma da interface seja escolhido por `[general] language`, com arquivos nomeados pelo idioma (`en_US.toml`, `pt_BR.toml`), **só em disco**, com `en_US` como default e como reserva, e com troca ao vivo.

O ponto de partida é um inventário, não uma folha em branco. Hoje:

- **~180 frases em pt-BR escritas no código.** A maior parte está em `porecatu-ui` — `lib.rs` (avisos, diálogos, notas no grid, o nome padrão *"Novo grupo"*), `access.rs` (rótulos do leitor de tela, nomes de cor), `overlay.rs` (constantes do diálogo, do editor de grupo e do popover de sessões), `context_menu.rs`, `terminal_menu.rs`, `group_menu.rs`, `search_bar.rs`, `status_bar.rs`, `git.rs`, `reload.rs`, `shell_integration.rs`, `session_writer.rs` e `keymap.rs`.
- **Prosa nascendo em crates de baixo.** `porecatu-core/src/action.rs` formata *"ação desconhecida: … você quis dizer …?"*; `porecatu-config` monta `ConfigError { message: String }` com frases próprias (`error.rs`, `color.rs`, `lib.rs`); `porecatu-session/src/named.rs` tem o `Display` de `SaveError`; `porecatu-render/src/gpu.rs`, o de `SurfaceError`. Essas frases chegam a avisos de interface pelo `to_string()`.
- **Plural escrito à mão**, como `format!("Fechar grupo ({tab_count} aba{plural})")`.
- **Nenhuma** menção a idioma de interface em documento algum.

Quatro restrições decidem a forma:

1. A **regra de dependência** do CLAUDE.md: `porecatu-core` não depende de nada, `porecatu-config` não conhece GUI, `porecatu-render` não conhece domínio. Um catálogo que todos consultassem furaria a tabela.
2. **Nada embutido**, por decisão do dono do produto: os arquivos são a fonte e ficam em disco. O `cargo run` do desenvolvimento, que não tem instalação, também precisa achá-los.
3. **Config inválida não derruba o app** ([ADR-0003](0003-formato-de-configuracao.md) regra 2) — e o mesmo tem de valer para um arquivo de idioma inválido, que é ainda mais provável: é um arquivo que o usuário edita com a frase na mão.
4. **Recarga é um evento e um frame** ([ADR-0030](0030-escopo-do-hot-reload.md)), com parse fora da main thread.

## Decisão

**Um crate folha novo, `porecatu-locale`, lê, valida e mescla arquivos TOML de idioma achados em disco; `porecatu-ui` é o único lugar em que texto de interface é composto, por acessores tipados gerados de um registro de mensagens; os crates de baixo devolvem erros tipados e nunca prosa de interface.**

### 1. Onde o catálogo mora

**`porecatu-locale`**, crate novo **sem nenhuma dependência do projeto** — só `serde` e `toml`, que o workspace já tem. Ele possui:

- o formato do arquivo (§4) e o parse, com erro localizado por linha e coluna;
- a **mescla de camadas** (§6), como função pura;
- a validação contra um **esquema recebido do chamador** — a lista de identificadores, com os marcadores e se é plural —, sem conhecer quais frases o app tem;
- a substituição de marcadores e a escolha de forma de plural (§5);
- um resolvedor puro que recebe os diretórios candidatos como argumento e nunca lê `std::env` nem `current_exe` — o mesmo desenho de `resolve_config_path`.

Só **`porecatu-ui`** depende dele. A tabela de dependência do CLAUDE.md ganha uma linha:

| Crate | Pode depender de | Nunca depende de |
|---|---|---|
| `porecatu-locale` | — | todo crate do projeto |

`porecatu-ui` possui o resto:

- o **registro de mensagens**: uma macro declarativa que lista cada identificador, seus marcadores e se é plural, e gera acessores tipados (`msg::group_menu::close(&catalog, count)`). Esquecer um argumento é erro de compilação, não um `{count}` cru na tela. O mesmo registro produz o esquema que `porecatu-locale` valida;
- a construção dos caminhos reais (§6), o watcher (§9) e o `Arc<Catalog>`, que é **do processo**, como o `Arc<Config>` ([ADR-0015](0015-multiplas-janelas.md), [ADR-0030](0030-escopo-do-hot-reload.md)): duas janelas, um catálogo, uma troca redesenha as duas.

`porecatu-config` ganha **só** o campo `general.language: String`, default `"en_US"`, classe A. Ele **não valida** o valor: pela regra 2 do ADR-0003, um erro de desserialização descarta a config inteira, e perder `confirm_close_window` e o tema por causa de um `pt_br` é desproporcional. Quem valida é o resolvedor (§6), com aviso próprio.

### 2. Texto de interface nasce só em `porecatu-ui`

Regra: **nenhum crate abaixo de `porecatu-ui` produz frase destinada a uma superfície de interface.** O `Display` dos erros continua existindo — é o que vai para a saída de erro e para depuração, fora do escopo do PRD-015 —, e `porecatu-ui` passa a **nunca** chamar `to_string()` num erro de outro crate para montar aviso, diálogo ou nota. Ele casa a variante e compõe a frase pelo catálogo.

- **Já tipados, só falta o mapeamento em `ui`:** `ActionParseError` (com `input` e `suggestion`), `SaveError`, `SurfaceError`, os erros de spawn de PTY. A **causa** que vem do sistema operacional (`io::Error`) entra como marcador `{cause}` e é mostrada como chegou.
- **`ConfigError` ganha um `kind`.** O campo `message: String` dá lugar a um enum — `Toml { detail }` (o texto do crate `toml`, mostrado como chegou), `Unreadable { path, cause }`, `DuplicateThemeName { name }` —, com `line` e `column` intocados. É a mesma estrutura localizada da regra 3 do ADR-0003, só que a frase deixa de ser montada no crate errado.
- **O erro de cor não tem como ser tipado**: ele viaja dentro de `serde::de::Error::custom` e sai do outro lado como texto do crate `toml`. A mensagem passa a ser **inglês técnico neutro** (`invalid color "x": expected "#rrggbb", "#rrggbbaa" or "transparent"`) e é tratada como o resto do texto do `toml`: detalhe técnico dentro de um aviso traduzido, que é exatamente o que um usuário em pt_BR já vê hoje para qualquer outro erro de sintaxe.
- **`keymap.rs`** já é `porecatu-ui`: os seus erros viram variantes e passam pelo catálogo como o resto.

### 3. Escopo, e o que fica de fora

O que entra e o que não entra é o RF-15.14 e o RF-15.15 do PRD. Três casos pedem registro, porque cada um tem uma resposta errada tentadora:

- **Linha de comando em inglês fixo, sem catálogo** — e é isto que revisa o ADR-0040 §1. `argv` é lido em `src/main.rs` **antes** de a config existir; traduzir `--help` exigiria ler e resolver config e idioma antes de parsear o argumento que diz qual config ler. Inglês fixo é coerente com o default e não cria ordem circular. É texto do binário, não de interface, e por isso não conta contra a métrica de "uma frase no código" do PRD.
- **Os snippets do convite de integração de shell continuam embutidos** de [docs/reference/integracao-de-shell.md](../reference/integracao-de-shell.md) ([ADR-0039](0039-convite-a-integracao-de-shell.md) §5): são código colado na config do shell do usuário, não texto de interface. O que vai para o catálogo é a frase em volta. O marcador de dispensa que o usuário digita não é traduzido — é protocolo, e traduzi-lo quebraria a dispensa de quem trocou de idioma depois de dispensar.
- **Chips de tecla** (`Ctrl`, `Shift`, `Cmd`, `PageDown`) ficam fora: são nomes de tecla, iguais nos dois idiomas. Um idioma futuro que precise de `Strg` pede ADR.

### 4. Formato do arquivo

TOML, pela mesma razão do ADR-0003: o usuário já edita um. Um arquivo por idioma, **o nome do arquivo é a identidade** — não há campo `locale` dentro dele para divergir do nome.

```toml
# pt_BR.toml — comentários em pt-BR (regra do projeto para texto que não é código)

[tab_menu]
new = "Nova aba"
close = "Fechar aba"

[group_menu]
close = { one = "Fechar grupo ({count} aba)", other = "Fechar grupo ({count} abas)" }

[dialog.close_tab]
title = "Fechar aba?"
body = "\"{title}\" tem um programa em primeiro plano. Fechar mesmo assim?"

[group_editor]
section_group = "GRUPO"
```

- **Tabelas por superfície**, no máximo dois níveis (`dialog.close_tab.title`), dentro do teto de três do ADR-0003. As de partida: `tab_menu`, `terminal_menu`, `group_menu`, `group_editor`, `move_to_group`, `dialog.*`, `notice.*` (config, sessão, git, gpu, idioma), `note.*` (notas no grid), `search_bar`, `status_bar`, `session_picker`, `color` (nomes da paleta de grupo) e `access` (rótulos que só o leitor de tela lê).
- **O identificador é a identidade**, nunca o texto em inglês. `tab_menu.close`, não `"Close tab"`: uma frase inglesa corrigida não pode mudar a chave de todas as traduções.
- **Caixa alta é do texto**, não do código: `"GRUPO"` e `"GROUP"` estão escritos assim. O tradutor decide.
- **A tabela `meta` é reservada** e ignorada sem aviso: um campo futuro de metadado entra sem quebrar arquivo existente.
- Chave que o esquema não conhece gera **aviso agregado** (§8), como chave desconhecida na config (RF-4.22).

### 5. Marcadores e plural

- **Marcador** é `{ident}`, com `ident` em `[a-z_][a-z0-9_]*`. `{{` e `}}` são chaves literais. A substituição é literal e não recursiva: um `{title}` cujo valor contém `{count}` não é expandido de novo — o valor vem de um título de aba, que vem de um programa.
- Frase com marcador que o esquema **não** declara para aquela chave é **inválida** e cai na reserva (RF-15.13). Frase que **omite** um marcador declarado é aceita em tempo de execução; o teste do §10 exige que os dois arquivos do projeto os tenham todos.
- **Plural** é uma tabela inline com **`one`** e **`other`**, e o código escolhe `one` quando `n == 1` e `other` em todo o resto. É exatamente a regra que o código já aplica hoje (`count == 1`) e a do inglês. A diferença do CLDR para o português (em que `0` também é `one`) não aparece em nenhuma frase atual: toda contagem que o app mostra é ≥ 1 (abas de um grupo, painéis ≥ 2, commits atrás > 0). Frase de plural sem `other` é inválida; sem `one`, usa `other` para todo `n`.
- Idiomas com mais formas (`few`, `many`) **não são expressáveis**, e isso é limitação registrada, não bug. `zero`/`few`/`many` hoje são chave desconhecida — o formato fica aberto para um ADR futuro que adote regras de plural do CLDR (`intl_pluralrules`, MIT/Apache) sem invalidar arquivo nenhum.

### 6. Onde procurar, e como mesclar

**Nome.** O valor de `language` tem de casar `^[a-z]{2,3}_[A-Z]{2}$`, com a caixa exata. Isso fecha o caminho de `../` antes de qualquer acesso a disco, e evita o "funciona no Windows, falha no Linux" de um `pt_br` num sistema de arquivos que distingue caixa. Subetiqueta de escrita (`zh_Hant_TW`) fica fora — limitação registrada. Nome inválido é tratado como não encontrado (RF-15.3).

**Diretório do usuário:** `locales/` ao lado do **caminho resolvido** do `porecatu.toml` — derivado, não resolvido de novo, pelo mesmo raciocínio do `sessions/` no [ADR-0054](0054-sessoes-nomeadas.md) §2. `--config` e `PORECATU_CONFIG` o deslocam junto, e a costura de teste continua sendo uma só. O app nunca cria essa pasta.

**Diretório do app**, o primeiro candidato que existir, relativo a `canonicalize(current_exe()).parent()`:

| Ordem | Candidato | Quem o usa |
|---|---|---|
| 1 | `PORECATU_LOCALES` | costura de teste e de desenvolvimento, **não contrato público** — mesmo estatuto de `PORECATU_SESSION` |
| 2 | Windows: `<exe>/locales` | MSI ([ADR-0057](0057-idiomas-nos-artefatos.md)) |
| 3 | Linux: `<exe>/../share/porecatu/locales`, depois `<exe>/locales` | `.deb` e AppImage |
| 4 | macOS: `<exe>/../Resources/locales`, depois `<exe>/locales` | `.app` |
| 5 | **Só em build de debug:** `concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales")` | `cargo run` no repositório |

O candidato 5 embute um **caminho**, não um conteúdo: o arquivo continua sendo lido do disco, e editá-lo no repositório recarrega ao vivo. Em build de release ele não existe. Build de release fora de instalação — o `--target-dir` separado que o CLAUDE.md recomenda para não brigar com o `.exe` em uso — precisa de `PORECATU_LOCALES=<repo>/locales`.

No repositório, os arquivos ficam em **`locales/`, na raiz** — ao lado de `assets/`, não dentro de `docs/`: são arquivo de produto, não documentação.

**Mescla, frase a frase.** Com `⊕` significando "a camada da direita vence por chave":

```
resolve(L)  = app/L.toml ⊕ usuário/L.toml
catálogo    = resolve(en_US) ⊕ resolve(language)
```

Frase ausente de todas as camadas é o **identificador** (RF-15.11). **Cada camada falha sozinha**: um `pt_BR.toml` do usuário com erro de sintaxe derruba só aquela camada; o `pt_BR.toml` instalado continua valendo, e o aviso sobre o erro sai em português.

### 7. Arranque

A ordem em `App::new` passa a ser: config → catálogo → **só então** formatar os avisos pendentes do arranque e os erros do mapa de teclas. `pending_startup_warnings` guarda **identificador e argumentos**, não frase pronta — é isso que deixa o aviso de config inválida sair no idioma do catálogo que acabou de ser montado.

**Config inválida no arranque não troca o idioma.** Hoje `LoadResult::Invalid` devolve `Config::default()` inteiro, o que poria a interface em inglês justo quando um usuário em pt_BR tem um erro de digitação para ler. Se o arquivo é TOML sintaticamente válido e falhou só na desserialização, `general.language` é lido de um parse cru (`toml::Value`) e usado. Se a sintaxe está quebrada, não há o que ler, e `en_US` é inevitável.

### 8. Avisos, e a única frase no código

Os avisos do idioma são **consumidores novos do canal 1** do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), cuja lista é fechada por intenção:

| Caso | Severidade | Em que idioma o aviso sai |
|---|---|---|
| Idioma escolhido não encontrado, ou nome inválido | aviso, com o valor e os diretórios procurados | `en_US` (a reserva que carregou) |
| Arquivo ilegível ou com erro de sintaxe | erro, com caminho, linha e coluna | o que carregou |
| Frases ausentes ou inválidas numa camada | **informação agregada**: *"N textos sem tradução em xx_YY"* | o que carregou |
| Chaves desconhecidas numa camada | aviso agregado | o que carregou |
| Nenhum catálogo | a frase fixa abaixo | inglês |

Frase ausente é **informação**, que expira sozinha em 6 s (RF-10.16, [ADR-0014](0014-superficie-de-aviso-e-dialogo.md)), e não aviso persistente: um idioma do usuário fica incompleto a cada atualização do app que acrescenta frases, e isso não pode virar um aviso a dispensar em todo arranque.

**A frase fixa.** Sem nenhum arquivo, não há de onde ler o texto do aviso que diz isso. A saída honesta é **uma** frase em inglês no código — `language files not found (searched: …)`, com a lista de diretórios —, no aviso e na saída de erro. É a única exceção à regra do §2, e não é um catálogo embutido disfarçado: é uma constante, uma frase, e o teste do §10 a conta.

### 9. Troca ao vivo

`general.language` é **classe A** do [ADR-0030](0030-escopo-do-hot-reload.md), escrita ao lado da chave no arquivo de exemplo como toda chave.

- **Gatilhos:** recarga de config em que `language` mudou; e evento de arquivo em `<pasta do porecatu.toml>/locales/`. O diretório do app **não** é assistido — ele só muda quando o app é atualizado, e aí o binário também muda.
- **Um watch a mais, não recursivo.** O watcher de hoje assiste a pasta da config **sem** recursão, e não vê o que muda dentro de `locales/`. Entra um segundo watch, não recursivo, sobre `locales/`, criado quando o primeiro vê a pasta nascer. Não se troca por um watch recursivo: com `--config ~/porecatu.toml`, ele assistiria o home inteiro.
- **Parse fora da main thread.** A thread do watcher guarda o último `language` que leu, monta o catálogo ali e entrega `Arc<Catalog>` e os avisos pelo `EventLoopProxy`. Mesmo debounce de ~200 ms: uma recarga, um evento, um frame.
- **Falha na troca mantém o catálogo anterior** e avisa (RF-15.23) — espelho da recarga de config, que mantém a config anterior. No arranque não há anterior, e vale a reserva.
- **O que muda e o que fica.** A regra é *"o que é lido a cada frame muda; o que foi composto por um evento fica"*. Muda no próximo frame: a barra, os menus — inclusive um aberto, porque o layout do menu é refeito por frame —, a barra de status, a busca, os popovers, o tooltip e a árvore de acessibilidade. Fica como estava: texto já escrito no grid, avisos já empilhados, e um diálogo aberto, cujo título e corpo são copiados na abertura. Retraduzir o grid seria o app reescrevendo texto que já entregou ao terminal.
- **"Novo grupo" é dado do usuário.** É resolvido no idioma corrente quando o grupo nasce, gravado na sessão, e nunca retraduzido (RF-15.16).

### 10. Verificação automática

Um teste de integração em `crates/porecatu-ui/tests/locales.rs`, no `cargo test` das três plataformas, lê os arquivos do repositório por `CARGO_MANIFEST_DIR` e reprova se:

- algum arquivo de `locales/` não parsear;
- o conjunto de chaves de um arquivo for diferente do registro, nos dois sentidos;
- os marcadores de uma chave não forem os que o registro declara;
- uma chave de plural não tiver exatamente `one` e `other`;
- algum valor for vazio;
- `pt_BR.toml` tiver `\bguias?\b`, sem distinguir caixa — a regra de terminologia do [ADR-0009](0009-referencia-visual-e-reconciliacao.md) §8 vira teste.

E um segundo teste conta as frases escritas no código fora do registro que chegam a uma superfície: tem de ser **uma**, a do §8.

`scripts/verify-docs.py` **não** muda: ele verifica documentação, e estes arquivos são produto.

Testes unitários que hoje comparam frase em pt-BR (o plural de `group_menu.rs`, por exemplo) passam a carregar o `pt_BR.toml` do repositório por um auxiliar de teste, e os de plural são escritos nos dois idiomas.

### 11. Acessibilidade

A raiz da árvore do `accesskit` recebe o idioma com `Node::set_language`, em BCP 47 (`pt-BR`, `en-US`) — derivado do nome do arquivo trocando `_` por `-`. Sem isso, um leitor de tela configurado em português leria rótulos em inglês com a fonética portuguesa. O rótulo da raiz, "Porecatu", é nome próprio e não passa pelo catálogo.

### 12. Truncamento de rótulo de menu

O menu de contexto, o menu de grupo e o popover de grupo de destino têm largura fixa e **não truncam** rótulo — só aviso, corpo de diálogo e nomes truncam. Com o texto vindo de arquivo que o usuário pode escrever, um rótulo longo invadiria o chip de atalho. O rótulo de item passa a truncar com reticências pelo `TextMeasurer::truncate` que já corta título de aba.

É **mudança de aparência**, e passou pelo que o [ADR-0032](0032-interface-do-v1-fechada.md) exige: aval do dono do produto, dado em 2026-09-29, registrado aqui. **Zero valor novo**: nenhuma largura, cor ou espaçamento muda; um rótulo que cabe — todo rótulo dos dois arquivos do projeto — desenha exatamente como hoje. A especificação visual registra o comportamento na §4.4 no PR que o implementa.

## Alternativas consideradas

### Embutir os arquivos no binário, com `include_str!`

É o padrão que o projeto já usa para fontes, ícone e snippets, e deixaria o app sempre com texto. Recusado **por decisão do dono do produto**: a tradução deve ser trocável sem recompilar, e um catálogo embutido como reserva seria uma segunda fonte de verdade, que diverge da primeira em silêncio sempre que alguém edita o arquivo e esquece que o binário tem outra cópia. O custo aceito é o do [ADR-0057](0057-idiomas-nos-artefatos.md): todo artefato de release precisa levar os arquivos, e o binário cru deixa de ser publicado.

### Detectar o idioma do sistema

Seria o comportamento que a maioria dos apps tem, com um crate pequeno (`sys-locale`). Recusado pelo dono do produto: o default fixo faz o app se comportar igual em toda máquina sem ninguém escrever nada, e o usuário que quer outro idioma escreve uma linha.

### Fluent, gettext ou `rust-i18n`

Fluent (`fluent-bundle`) resolve plural e gênero de qualquer idioma, e gettext é o padrão histórico. Os dois trazem um formato que não é TOML, que o usuário não conhece, e mais uma dependência grande; `rust-i18n` é macro que embute os arquivos no binário, o que viola a decisão do §1. As necessidades de hoje — ~180 frases, dois idiomas, plural de duas formas — cabem num formato de quatro regras, e a porta para regras de plural do CLDR fica aberta (§5).

### Substituir o arquivo inteiro, em vez de mesclar

Mais simples de explicar: "o arquivo do usuário é o arquivo". Recusado pelo dono do produto: quem quer mudar uma frase teria de copiar 180, e cada atualização que acrescenta frases as faria cair em inglês em silêncio para quem tem o próprio arquivo.

### O catálogo como módulo de `porecatu-ui`

Menos cerimônia, sem crate novo. Recusado porque `lib.rs` já passa de dez mil linhas e o carregador é lógica pura — parse, mescla, validação — que se testa melhor isolada, sem arrastar o crate de GUI para o teste.

### O catálogo como módulo de `porecatu-config`

Há precedente: o `.porecatu` mora lá ([arquitetura](../arquitetura.md) §6.1), e `ConfigError::from_toml` seria reaproveitado. Recusado porque mistura "o arquivo de config do usuário" com "o texto da interface", e obrigaria `config` a conhecer o diretório do executável, que não é dele.

### Deixar os crates de baixo produzirem prosa, e traduzi-la depois

Seria menos trabalho: um catálogo acessível de `core`, `config` e `session`. Recusado porque obriga `porecatu-core` a depender do catálogo, furando a única regra da tabela que não tem exceção nenhuma, e porque uma frase montada longe da superfície não sabe em que superfície vai cair.

### Copiar `locales/` para `target/` num `build.rs`

Faria o `cargo run` achar os arquivos ao lado do executável, como numa instalação. Recusado: achar o diretório de saída a partir de `OUT_DIR` é frágil, e a edição ao vivo do arquivo no repositório exigiria recompilar — o contrário do que o candidato 5 do §6 dá de graça.

### Traduzir a linha de comando pelo catálogo

Coerente com o resto. Recusado pela ordem circular do §3: seria preciso resolver a config antes de parsear o argumento que diz qual config usar.

## Consequências

### Positivas

- Trocar uma frase, ou acrescentar um idioma, não exige recompilar nem abrir PR no projeto.
- Os crates de baixo ficam mais limpos: erro tipado em `config`, `core` e `session`, prosa num lugar só.
- O plural escrito à mão some do código, e a regra de terminologia "abas" passa de revisão humana a teste.
- O leitor de tela passa a saber em que idioma a árvore está.

### Negativas

- **Todo usuário atual passa a ver inglês** ao atualizar, até escrever `language = "pt_BR"`. Decisão consciente, comunicada no CHANGELOG da `0.8.0`.
- **Um canal de distribuição a menos**: sem os arquivos, o binário cru mostraria identificadores, e deixa de ser publicado ([ADR-0057](0057-idiomas-nos-artefatos.md)). `cargo install` não é canal suportado pelo mesmo motivo.
- **Uma frase continua no código**, a do §8, e um teste existe só para garantir que ela é a única.
- Cada frase nova de interface passa a custar três lugares — registro, `en_US.toml`, `pt_BR.toml` — em vez de um. O teste do §10 é o que torna isso barato de lembrar.
- O `--help` fica em inglês mesmo com a interface em português.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Frase nova escrita direto no código, fora do registro | Alta | Médio — nenhum idioma a alcança | Teste de contagem do §10; registro com acessores tipados torna o caminho certo o mais curto |
| Release publicada sem os arquivos | Média | Alto — app sem texto | Verificação por artefato no CI ([ADR-0057](0057-idiomas-nos-artefatos.md)); nenhuma tag entre a migração das frases e o empacotamento |
| Rótulo traduzido longo demais para o menu | Média (idiomas do usuário) | Baixo | Truncamento do §12 |
| Truncar rótulo de menu por frame vira custo de medição | Baixa | Médio | `truncate` é um shaping só, e só roda quando a largura medida passa do orçamento — que nenhum rótulo dos dois arquivos do projeto passa |
| Idioma do usuário numa escrita fora do recorte da Iosevka | Baixa | Baixo — desenha com fonte do sistema, métrica diferente | Limitação registrada no PRD-015; o recorte cobre latim, grego e cirílico |
| `pt_BR` e `en_US` divergirem em significado, não em chave | Média | Baixo | Revisão de PR; o teste pega chave e marcador, não sentido |
| Build de release fora de instalação abrir sem texto | Alta (só desenvolvimento) | Baixo | `PORECATU_LOCALES`, registrado nas armadilhas do CLAUDE.md |
