# ADR-0008 — Keybindings e roteamento de input

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** ADR-0003, ADR-0006, [ADR-0009](0009-referencia-visual-e-reconciliacao.md), PRD-001, PRD-002

> **Nota (ADR-0009).** O design canvas exibe `Ctrl+T`, `Ctrl+G`, `Ctrl+,` e `Ctrl+1..6`. Esses rótulos são **ilustrativos**: este ADR prevalece e a regra "nunca `Ctrl+<letra>` sozinho" permanece. Dois defaults foram ajustados dentro da decisão vigente, sem alterá-la — `Ctrl+Shift+P` passou a abrir a paleta de comandos `[v2]` e `theme.cycle` migrou para `Ctrl+Shift+Y`. A tabela abaixo já reflete isso.

## Contexto

Todo emulador com abas enfrenta o mesmo conflito: as teclas que a aplicação quer usar para gerenciar abas são teclas que o programa dentro do terminal também quer. `Ctrl+T`, `Ctrl+W`, `Ctrl+N` têm significado em `emacs`, `readline`, `tmux` e editores. Uma tecla capturada pela aplicação **nunca chega** ao programa dentro do terminal — e o usuário só descobre quando algo não funciona e ele não entende por quê.

O Porecatu tem mais ações de app que um emulador comum (grupos, colapso, navegação entre grupos), o que agrava o problema.

Além disso, as convenções diferem por plataforma: macOS usa `Cmd` para ações de aplicação e deixa `Ctrl` livre para o terminal; Windows e Linux usam `Ctrl+Shift` justamente porque `Ctrl` sozinho pertence ao terminal.

## Decisão

### Cadeia de resolução

Um evento de teclado é resolvido em ordem, parando no primeiro que casar:

```
1. Modo de captura ativo?  (renomear aba/grupo, busca)
       -> consome tudo, exceto Esc e Enter
2. Keybind de aplicação    (config [keybindings])
       -> executa a ação, NÃO repassa ao terminal
3. Terminal
       -> codifica em bytes e escreve no PTY
```

A regra que evita a classe inteira de bugs: **um binding que casa nunca cai para o terminal.** Sem repasse parcial, sem "tenta os dois". Comportamento previsível vale mais que conveniência.

### Defaults por plataforma

| Ação | Windows / Linux | macOS |
|---|---|---|
| Nova aba | `Ctrl+Shift+T` | `Cmd+T` |
| Fechar aba | `Ctrl+Shift+W` | `Cmd+W` |
| Próxima / anterior | `Ctrl+Tab` / `Ctrl+Shift+Tab` | `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| Ir para aba N | `Alt+1`..`Alt+9` | `Cmd+1`..`Cmd+9` |
| Renomear aba | `Ctrl+Shift+R` | `Cmd+R` |
| Agrupar seleção | `Ctrl+Shift+G` | `Cmd+G` |
| Desagrupar | `Ctrl+Shift+U` | `Cmd+Shift+G` |
| Renomear grupo | `Ctrl+Shift+E` | `Cmd+E` |
| Colapsar grupo | `Ctrl+Shift+K` | `Cmd+K` |
| Próximo / anterior grupo | `Ctrl+Shift+PageDown` / `PageUp` | `Cmd+Alt+Right` / `Left` |
| Copiar / colar | `Ctrl+Shift+C` / `Ctrl+Shift+V` | `Cmd+C` / `Cmd+V` |
| Aumentar / diminuir fonte | `Ctrl+=` / `Ctrl+-` | `Cmd+=` / `Cmd+-` |
| Ciclar tema | `Ctrl+Shift+Y` | `Cmd+Shift+Y` |
| Paleta de comandos `[v2]` | `Ctrl+Shift+P` | `Cmd+Shift+P` |
| Recarregar config | `Ctrl+Shift+,` | `Cmd+,` |

Nenhum default usa `Ctrl+<letra>` sozinho em Windows e Linux. Esse espaço pertence ao terminal, sem exceção.

`Ctrl+Tab` é a única exceção ao padrão `Ctrl+Shift`, e é deliberada: é a convenção universal de troca de aba, e poucos programas de terminal a usam.

### Configuração

Bindings são totalmente rebindáveis na seção `[keybindings]` do TOML ([ADR-0003](0003-formato-de-configuracao.md)):

```toml
[keybindings]
"ctrl+shift+t" = "tab.new"
"ctrl+shift+g" = "group.create"
"ctrl+shift+q" = "none"        # libera a tecla para o terminal
```

- Ação `"none"` **remove** o binding, entregando a tecla ao terminal. É a válvula de escape para quem precisa da tecla no `emacs`.
- Binding duplicado é erro de config com mensagem citando ambas as linhas.
- O conjunto de ações é fechado e enumerado (`tab.new`, `tab.close`, `group.create`, ...). Ação desconhecida é erro de config com sugestão do nome mais próximo.

### Codificação para o terminal

O que não é binding de app vira bytes segundo os modos do terminal, e isso é responsabilidade de `porecatu-term`, não de `porecatu-ui`:

- Modo de cursor de aplicação (DECCKM) muda as setas entre `ESC [ A` e `ESC O A`
- Modo de teclado numérico de aplicação
- Modificadores em CSI-u / xterm, conforme o modo negociado
- **Bracketed paste**: colagem sempre envolvida em `ESC [ 200 ~` / `ESC [ 201 ~` quando o modo está ativo. Não é opcional — é o que impede que um texto colado com quebras de linha seja executado comando a comando

### IME e teclas mortas

Eventos de IME do `winit` (composição de CJK, acentuação por tecla morta em teclados ABNT2 e internacionais) passam **direto** ao terminal, sem consulta à tabela de keybindings. A composição em andamento é desenhada sobre a posição do cursor.

Isso importa no ambiente-alvo: teclado ABNT2 usa teclas mortas para acentuação, e capturá-las por engano tornaria o terminal inutilizável em português.

## Alternativas consideradas

### `Ctrl+<letra>` sem `Shift` nos defaults

Mais confortável de digitar, e é o que navegadores fazem. Descartada porque em terminal `Ctrl+letra` já significa outra coisa: `Ctrl+W` apaga palavra no readline, `Ctrl+T` transpõe caracteres, `Ctrl+N` desce no histórico. Roubar essas teclas por padrão quebra o uso normal do shell.

### Tecla líder (estilo tmux: `Ctrl+B` e depois a ação)

Resolve o conflito por completo com um único binding capturado. Descartada porque adiciona latência e um modo mental para ação que deveria ser imediata, e porque quem usa `tmux` dentro do Porecatu teria duas teclas líder concorrendo. Continua disponível para quem quiser: é configurável.

### Repassar a tecla ao terminal *e* executar a ação

Descartada de imediato. Comportamento ambíguo, impossível de depurar do ponto de vista do usuário.

### Detectar a aplicação em primeiro plano e adaptar os bindings

Ex.: desativar `Ctrl+Shift+W` quando `vim` está em foco. Descartada por fragilidade — exigiria varrer a árvore de processos, teria comportamento diferente por plataforma ([ADR-0004](0004-pty-cross-platform.md)) e produziria uma UI cujo comportamento muda sem explicação visível.

## Consequências

### Positivas

- Defaults que não brigam com o uso normal do shell.
- Cadeia de resolução simples de explicar e de depurar: primeiro que casa, vence.
- Válvula de escape (`"none"`) para qualquer conflito que apareça.
- Teclado ABNT2 e IME funcionam por decisão explícita, não por sorte.

### Negativas

- `Ctrl+Shift+<letra>` é mais desconfortável que `Ctrl+<letra>`. É o preço de não quebrar o terminal.
- Bindings diferentes entre plataformas complicam a documentação e a memória muscular de quem alterna entre sistemas.
- Conjunto fechado de ações significa que cada recurso novo precisa de um nome de ação novo, documentado.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Binding de app engolir tecla que o usuário precisa | Média | Médio | Ação `"none"`; lista completa de defaults no `porecatu.example.toml` |
| Tecla morta / IME capturada por engano | Média | Alto | Eventos de IME nunca consultam a tabela; teste manual com ABNT2 no critério de saída de F2 |
| Usuário não achar como rebindar | Média | Baixo | Seção `[keybindings]` completa e comentada no arquivo de exemplo |
| Colagem sem bracketed paste executar comandos | Baixa | Alto | Bracketed paste obrigatório quando o modo está ativo; teste dedicado |
