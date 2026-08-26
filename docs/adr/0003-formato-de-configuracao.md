# ADR-0003 — Configuração em TOML

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** PRD-004, PRD-005, ADR-0008

## Contexto

Os requisitos 4 e 5 do produto colocam a configuração no centro: aparência de abas e grupos (PRD-004) e cores e fontes do terminal (PRD-005) devem ser customizáveis por arquivo. Isso significa uma superfície de config grande — dezenas de chaves, aninhadas, com listas (paleta ANSI, temas nomeados, cadeia de fallback de fonte).

O arquivo é editado à mão, por humanos, num editor de texto, com o app aberto. O formato precisa ser legível, tolerar erro de digitação com mensagem útil, e ser barato de parsear no start (o tempo até o primeiro prompt importa num terminal).

Não há requisito de lógica programável na config. Nenhum dos cinco requisitos pede condicional, laço ou valor computado.

## Decisão

**TOML**, via `serde` + o crate `toml`.

Regras de comportamento, todas obrigatórias:

1. **Defaults completos.** Ausência de arquivo de config é estado válido: o app abre com aparência padrão sensata. Toda chave tem default. Nenhuma chave é obrigatória.
2. **Config inválida nunca derruba o app.** No start, config inválida = usa defaults + mostra o erro. No hot reload, config inválida = mantém a config anterior + mostra o erro. O usuário está editando o arquivo com o app rodando; estado intermediário inválido é normal, não excepcional.
3. **Erro localizado.** Mensagem de erro cita linha, chave e o que era esperado. `toml` fornece span de erro — usar.
4. **Chave desconhecida é aviso, não erro.** Permite abrir uma config feita numa versão mais nova sem quebrar. Aviso visível, execução continua.
5. **Hot reload** via crate `notify`, com parse fora da main thread. Ver [arquitetura.md](../arquitetura.md).

### Localização do arquivo

Resolução por ordem de precedência:

1. Flag de linha de comando `--config <caminho>`
2. Variável de ambiente `PORECATU_CONFIG`
3. Caminho padrão da plataforma, via crate `dirs`:

| Plataforma | Caminho |
|---|---|
| Linux | `$XDG_CONFIG_HOME/porecatu/porecatu.toml` (default `~/.config/porecatu/porecatu.toml`) |
| macOS | `~/.config/porecatu/porecatu.toml` |
| Windows | `%APPDATA%\porecatu\porecatu.toml` |

No macOS a escolha é deliberadamente `~/.config` em vez de `~/Library/Application Support`: é onde usuários de terminal esperam encontrar config de ferramenta de terminal, e permite compartilhar dotfiles entre macOS e Linux sem simlink. Isso difere de onde a **sessão** é gravada — ver [ADR-0005](0005-persistencia-de-sessao.md), que segue a convenção da plataforma porque não é arquivo para o usuário editar.

Referência completa comentada: [porecatu.example.toml](../config/porecatu.example.toml).

## Alternativas consideradas

### KDL

Formato do Zellij, bom para estruturas nomeadas e aninhadas — grupos de abas cairiam bem nele. Descartada por familiaridade e ecossistema: TOML é o formato que todo usuário de Rust já conhece de `Cargo.toml`, e o suporte de editor (syntax highlight, LSP, formatter) é incomparavelmente melhor. Para a nossa superfície de config, o ganho de expressividade do KDL não paga o custo de aprendizado.

### YAML

Descartada. Indentação significativa em arquivo editado à mão é fonte permanente de erro, e as armadilhas de tipagem implícita do YAML (o clássico `no` virando booleano, versões virando float) são especialmente ruins numa config cheia de códigos de cor e nomes de fonte.

### TOML + Lua opcional

TOML declarativo para o comum, um `init.lua` opcional para lógica — keybinds dinâmicos, título de aba computado, hooks. É o modelo do WezTerm e funciona bem lá.

Descartada **para o v1**, não em definitivo. Nenhum dos cinco requisitos precisa de lógica, e o custo é real: runtime Lua embutido, superfície de sandbox a pensar, tempo de start maior, e uma API estável a manter para os scripts do usuário. É uma decisão a revisitar com ADR próprio se e quando aparecer um caso de uso que a config declarativa não cubra.

### JSON

Descartada: sem comentários. Config de aparência sem comentário é inutilizável — o usuário precisa saber o que cada chave faz enquanto edita.

## Consequências

### Positivas

- Familiar para o público-alvo; suporte de editor excelente.
- `serde` faz o mapeamento; a struct de `Config` é a especificação do formato.
- Parse rápido, dependência leve, sem runtime embutido.
- Comentários no arquivo permitem que o próprio `porecatu.example.toml` seja a documentação de referência.

### Negativas

- Sem lógica: cada capacidade nova precisa de uma chave nova. A superfície de config cresce por acréscimo.
- Aninhamento profundo em TOML fica verboso (`[appearance.tabs.active]`). Mitigação: manter a hierarquia rasa, no máximo três níveis.
- Listas de tabelas (`[[themes]]`) são a parte menos legível do TOML; usar com parcimônia.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Superfície de config virar inconsistente com o tempo | Média | Médio | Regra de verificação: toda chave do exemplo tem que estar em PRD-004 ou PRD-005; nenhuma chave órfã |
| Hot reload em loop (editor salvando em duas etapas) | Média | Baixo | Debounce de ~200 ms no watcher, comparar conteúdo antes de reparsear |
| Usuário se perder numa config grande | Média | Médio | `porecatu.example.toml` comentado e completo; erro de parse com linha e chave |
| Pressão futura por lógica programável | Média | Baixo | Revisitar com ADR novo; a decisão foi tomada para o v1, não para sempre |
