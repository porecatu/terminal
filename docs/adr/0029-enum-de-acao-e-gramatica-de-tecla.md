# ADR-0029 — Enum de ação e gramática de tecla

**Status:** Aceito
**Data:** 2026-09-02
**Relacionados:** ADR-0003, ADR-0008, ADR-0015, ADR-0020, PRD-004, PRD-010, [catálogo de ações](../reference/acoes.md)

## Contexto

O [ADR-0008](0008-teclas-e-roteamento-de-input.md) decidiu a **semântica** de `[keybindings]`: conjunto fechado de ações, `"none"` libera a tecla, binding duplicado é erro citando as duas linhas, ação desconhecida é erro com sugestão do nome mais próximo, e nenhum default usa `Ctrl+<letra>` sozinho fora do macOS. O [catálogo de ações](../reference/acoes.md) enumera as 49 ações e se declara *"insumo direto do parser de `[keybindings]` na F4"*.

Duas coisas que aquele ADR não decidiu, e sem as quais o parser não pode ser escrito:

**1. Onde o enum de ação vive.** Não existe `enum Action` no código. O roteamento da F2/F3 é um `match` sobre tecla com defaults fixos (`handle_tab_action_key`), e o que existe de tipo são enums de *superfície*: `MenuAction`, `GroupAction`, `DialogAction`, `ActionOutcome` — cada um com o subconjunto que aquele widget oferece. Nenhum deles cobre o catálogo, e nenhum é endereçável por nome. O parser precisa transformar `"tab.new"` em algo, e `porecatu-config` **não pode depender de `porecatu-ui`** (regra de dependência de CLAUDE.md).

**2. A sintaxe da tecla.** O exemplo do ADR-0008 mostra `"ctrl+shift+t"` e nada mais. Fica indefinido: como se escreve `=`, `,`, `PageDown`, `0`; se `shift+ctrl+t` é o mesmo binding que `ctrl+shift+t` (sem canonicalização, "duplicado é erro" é indetectável — e o TOML já rejeita chave literal repetida, então o caso interessante é justamente o das grafias diferentes); qual é o nome do modificador de macOS; e **como um arquivo de dotfiles compartilhado entre máquinas expressa defaults diferentes por plataforma**, que é o caso real do usuário que sincroniza `~/.config` entre Linux e Mac.

## Decisão

### 1. `Action` nasce em `porecatu-core`

Enum exaustivo, uma variante por linha do catálogo, com `FromStr`/`Display` que casam **exatamente** o nome do catálogo (`tab.new`, `group.toggle_collapse`). Fica em `porecatu-core` porque é o único crate que `config` e `ui` podem ambos ver, e porque ação é vocabulário de domínio — não de configuração nem de desenho.

- As ações com argumento (`group.set_color`, `tab.move_to_group`) carregam o argumento na variante, e o `FromStr` **rejeita** ambas: elas não são vinculáveis a tecla, e o catálogo já as marca `Arg`. Rejeitar no parser é melhor que aceitar e ignorar.
- `none` **não** é variante de `Action`: é a ausência dela. O mapa resolvido é `HashMap<Chord, Action>`, e `"none"` remove a entrada em vez de inserir uma variante inerte que todo consumidor teria de filtrar.
- Os enums de superfície (`MenuAction`, `GroupAction`) **continuam existindo**, e cada um converte para `Action`. Eles são o que aquele widget oferece, com o alvo já resolvido pelo contexto de invocação; fundir tudo num enum só faria o menu de aba precisar de um `match` com 49 braços, 44 deles inalcançáveis.
- O teste que amarra os dois lados: **toda variante de `Action` aparece no catálogo, e toda linha do catálogo é uma variante** — a auditoria bidirecional que o critério de saída da F4 pede, mas em teste, não em revisão manual.

### 2. Gramática de tecla

Uma chave de `[keybindings]` é `modificador* tecla`, separados por `+`, tudo em **minúsculas**:

```toml
[keybindings]
"ctrl+shift+t"        = "tab.new"
"ctrl+shift+pagedown" = "group.next"
"ctrl+equals"         = "font.increase"
"alt+1"               = "tab.goto_1"
"ctrl+shift+q"        = "none"
```

- **Modificadores:** `ctrl`, `shift`, `alt`, `cmd`. `cmd` é o nome único do modificador de macOS — não `super`, não `meta`: é o nome escrito na tecla física, é como o ADR-0008 já o chama na tabela de defaults, e `super` num Mac lê como "Windows key". Fora do macOS, `cmd` casa a tecla lógica `Super` do `winit`; nenhum default a usa lá.
- **Teclas nomeadas por palavra**, não por símbolo, onde o símbolo é ambíguo em TOML ou depende de layout: `equals`, `minus`, `comma`, `period`, `slash`, `backslash`, `semicolon`, `quote`, `bracketleft`, `bracketright`, `backtick`, `space`, `tab`, `enter`, `escape`, `backspace`, `delete`, `insert`, `home`, `end`, `pageup`, `pagedown`, `up`, `down`, `left`, `right`, `f1`..`f24`. Letras e dígitos são o próprio caractere (`t`, `1`).
- **Canonicalização antes de qualquer coisa:** os modificadores são ordenados (`ctrl`, `alt`, `shift`, `cmd`) e a tecla normalizada, produzindo um `Chord` que é a chave real do mapa. `shift+ctrl+t` e `ctrl+shift+t` colidem, e é essa colisão que o erro de duplicado do ADR-0008 relata — citando as duas linhas do arquivo, que o span do `toml` fornece.
- **Tecla lógica, não física.** O casamento é sobre `Key::Character`/`Key::Named` do `winit`, com o layout do usuário já aplicado. É o que faz `ctrl+shift+comma` funcionar num ABNT2 sem o usuário descobrir onde a vírgula "fisicamente" mora. IME e teclas mortas continuam passando direto, sem consulta ao mapa (ADR-0008).

### 3. Defaults por plataforma num arquivo só

`[keybindings]` é a tabela comum, e `[keybindings.windows]`, `[keybindings.linux]` e `[keybindings.macos]` sobrescrevem por plataforma. A resolução é: defaults embutidos da plataforma atual → `[keybindings]` → a tabela da plataforma atual. Um dotfile sincronizado entre máquinas fica com uma seção comum e o desvio de cada SO explícito, em vez de exigir arquivos separados ou condicional que o TOML não tem.

Um binding definido na tabela comum e na da plataforma **não** é duplicado: é override, que é o ponto. Duplicado é a mesma tecla duas vezes na mesma tabela, com grafias diferentes.

### 4. Erro de config em `[keybindings]` nunca deixa o app sem tecla

Chave malformada, tecla desconhecida ou ação desconhecida são erro **daquela linha**: ela é descartada, o default embutido permanece, e o aviso do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) cita linha, chave e a sugestão do nome mais próximo. Isso é mais fino que a regra 2 do [ADR-0003](0003-formato-de-configuracao.md) — que descarta a config inteira e mantém a anterior —, e a diferença é deliberada: um erro de digitação numa tecla não deveria reverter uma mudança de cor feita na mesma gravação, e um mapa vazio deixaria o usuário sem `Ctrl+Shift+C`.

Trocar `[keybindings]` **durante um modo de captura** (rename inline, editor de grupo, diálogo) não afeta a captura em curso: o modo tem teclado próprio (ADR-0008 passo 1), e o mapa novo passa a valer quando ele fecha.

## Alternativas consideradas

### `Action` em `porecatu-config`

Seria o dono natural do que o parser produz. Rejeitada porque `porecatu-core` não pode depender de `config` (a flecha é a inversa), e o `Workspace` do core é quem executa metade das ações — `group.next`, `tab.move_left`, `activate_tab`. O enum acabaria duplicado nas duas pontas.

### Enum único, com os widgets lendo subconjuntos dele

Menos tipos. Rejeitada porque cada superfície tem alvo próprio (o grupo clicado, a aba clicada, a aba ativa) e disponibilidade própria (item esmaecido sobre run implícito, RF-10.20). Um enum só empurra essa lógica para dentro de cada `match`, e o compilador para de ajudar: nada impediria o menu de aba de despachar `window.close`.

### Tecla física (`KeyCode`) em vez de lógica

Estável entre layouts, e é o que jogos usam. Rejeitada pelo ambiente-alvo: num ABNT2 a vírgula e o ponto-e-vírgula não estão onde o `KeyCode` diz, e o usuário teria de escrever o nome da tecla US que ocupa aquela posição. O ADR-0008 já escolheu lógica ao mandar IME passar direto.

### Um arquivo de keybindings por plataforma

Simples de implementar. Rejeitada porque quebra o dotfile único, que é o motivo pelo qual o ADR-0003 escolheu `~/.config` até no macOS.

## Consequências

### Positivas

- O parser da F4 tem contrato completo: nome, gramática, canonicalização, precedência e o que fazer no erro.
- O teste bidirecional `Action` ↔ catálogo automatiza metade da auditoria de rastreabilidade do critério de saída da F4.
- Ação nomeável abre caminho para a paleta de comandos `[v2]`, que precisa exatamente disso — nome, rótulo e execução por identificador.

### Negativas

- Mais um enum grande em `porecatu-core`, com conversões nas duas pontas; adicionar ação passa a mexer em três lugares (core, catálogo, superfície que a oferece). O teste bidirecional é o que impede que fiquem fora de sincronia.
- A tabela de nomes de tecla é um vocabulário que o usuário precisa descobrir. Mitigação: o arquivo de exemplo lista todos, e o erro sugere o nome mais próximo.
- Aceitar `cmd` fora do macOS é uma pequena inconsistência (o modificador existe, nenhum default o usa), mas rejeitá-lo faria um dotfile compartilhado dar erro na máquina errada.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Nome de tecla escolhido aqui divergir do que outros terminais usam, confundindo quem migra | Média | Baixo | Os nomes seguem a grafia do `winit`/W3C em minúsculas, que é a convenção mais comum; o erro sugere o nome mais próximo |
| Canonicalização mudar depois e invalidar arquivos existentes | Baixa | Médio | A ordem dos modificadores é interna ao `Chord`; o que o usuário escreve continua em qualquer ordem |
| Descarte por linha (§4) esconder um erro que o usuário não percebe | Média | Baixo | O aviso do ADR-0014 persiste até dispensa para erro e aviso — só informação sai sozinha |
