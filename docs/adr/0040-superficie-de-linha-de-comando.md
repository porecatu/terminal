# ADR-0040 — Superfície de linha de comando

**Status:** Aceito
**Data:** 2026-09-04
**Relacionados:** [ADR-0003](0003-formato-de-configuracao.md), [ADR-0005](0005-persistencia-de-sessao.md), [ADR-0010](0010-licenciamento.md), [ADR-0036](0036-formato-do-arquivo-de-sessao.md), PRD-003

## Contexto

O binário nunca leu `argv`. Duas coisas colidem nisso ao abrir a F5.

**O RF-3.12 exige:** *"Abrir o app com um caminho como argumento cria uma sessão nova naquele diretório, **sem** restaurar e **sem** sobrescrever a sessão gravada."* É requisito aprovado do PRD-003, e é impossível sem ler `argv`.

**E há uma dívida da F4 esperando exatamente isso.** A etapa 1 daquela fase entregou `resolve_config_path(cli_config: Option<&Path>)` com a precedência do ADR-0003 pronta e testada nas três fontes (`--config` → `PORECATU_CONFIG` → caminho de plataforma). Nada chama a função com `Some(path)`: `App::new` chama `porecatu_config::load(None)` fixo. A flag `--config` está documentada no ADR-0003 e **não existe de fato**. O roadmap a registra como dívida da etapa 1 da F4, e a engrenagem que a paga é a mesma que o RF-3.12 pede.

A decisão que falta tem duas partes: **como** parsear, e **o que exatamente** a superfície aceita. A segunda importa mais que parece — a linha de comando de um app é contrato público, e o que entra nela não sai sem quebrar o script de alguém.

## Decisão

**Parsing à mão, sem crate novo, com uma superfície deliberadamente pequena: `--config <caminho>`, um caminho posicional, `--help` e `--version`.**

### 1. A superfície, inteira

| Forma | Efeito |
|---|---|
| `porecatu` | Restaura a última sessão gravada (RF-3.7) |
| `porecatu <diretório>` | Sessão nova naquele diretório; não restaura, não sobrescreve (RF-3.12) |
| `porecatu --config <arquivo>` | Usa esse arquivo de config, vencendo `PORECATU_CONFIG` e o caminho de plataforma (ADR-0003) |
| `porecatu --help` / `-h` | Imprime as formas acima e sai |
| `porecatu --version` / `-V` | Imprime nome, versão e a licença ([ADR-0010](0010-licenciamento.md)) e sai |

`--config` e o caminho posicional combinam. Argumento desconhecido, `--config` sem valor, ou mais de um posicional: mensagem de erro numa linha, código de saída diferente de zero, **sem abrir janela**. Falhar cedo e visível é melhor que abrir uma janela que ignorou o que o usuário pediu.

Não há flag para o arquivo de **sessão**: só `PORECATU_SESSION` ([ADR-0036](0036-formato-do-arquivo-de-sessao.md) §6). A assimetria é deliberada — `--config` existe porque o ADR-0003 a decidiu e porque trocar de config é caso de uso real; trocar o destino da sessão é costura de teste, e costura de teste não vira contrato público.

### 2. Semântica do caminho posicional (RF-3.12)

Um diretório na linha de comando significa **"quero uma sessão descartável aqui"**:

- Uma janela, uma aba, com `cwd` naquele diretório — que vence o `[general] startup_directory`.
- A sessão gravada **não é lida**.
- A sessão gravada **não é sobrescrita**: enquanto o app rodar nesse modo, nenhuma gravação acontece, nem por debounce nem no encerramento. É o mesmo caminho de código de `[session] enabled = false`, acionado por argumento em vez de por chave.

É essa segunda metade que faz o requisito valer a pena: `porecatu ~/projeto` numa janela rápida não pode custar a sessão de vinte abas que o usuário deixou montada.

Caminho que não existe, ou que é arquivo em vez de diretório: erro na saída padrão de erro e saída sem abrir janela, pelo mesmo princípio do §1. Não cair no home em silêncio — o RF-3.10 faz isso para um `cwd` **gravado** que sumiu, que é situação normal; um caminho que o usuário acabou de digitar errado é outra coisa.

### 3. Parsing à mão

Um laço sobre `std::env::args_os`, num módulo próprio do binário, com a função de parse **pura** (recebe os argumentos, devolve um resultado) para ser testável sem processo — o mesmo formato que `resolve_config_path` já usa para a precedência das três fontes.

Sem crate de CLI. Cinco formas não justificam uma dependência, e o projeto já tem precedente escrito: o `percent_decode` de `porecatu-term/src/osc7.rs` foi escrito à mão justamente para não puxar `percent-encoding` por causa de uma função. Um parser de argumentos com esta superfície é menor que aquele decodificador.

Se a superfície crescer — subcomandos, muitas flags —, a decisão se revisita com um ADR novo. O gatilho é explícito: mais de uma dúzia de formas, ou a primeira flag que precise de valor com `=`, agrupamento de curtas ou repetição.

### 4. Onde mora

Em `src/main.rs`, o binário — não em `porecatu-ui`. O event loop mora em `ui` por razões de plataforma, mas `argv` é do processo, e o binário é a única camada que pode conhecê-lo sem furar a regra de dependência. `porecatu_ui::run()` passa a receber o que o parse produziu; a assinatura muda uma vez, agora, e não volta a mudar por causa disso.

## Alternativas consideradas

### `clap`

O padrão do ecossistema, com `--help` gerado, validação e mensagens boas de graça. Descartada por proporção: `clap` com derive arrasta uma árvore de dependências e um tempo de compilação relevantes para cinco formas de invocação, num projeto cuja política de dependência exige ADR para cada crate novo. Revisitável pelo gatilho do §3.

### `lexopt` ou `pico-args`

Minúsculos, sem árvore, feitos exatamente para este tamanho. Descartada por pouco: ainda é uma dependência nova para um laço de vinte linhas, e nenhuma delas gera o `--help` — que é justamente o que daria trabalho e que teríamos de escrever de qualquer forma.

### Só `--config`, deixando o RF-3.12 para depois

Pagaria a dívida da F4 sem abrir a superfície posicional. Descartada: o RF-3.12 é requisito aprovado do PRD-003, e a F5 é a fase que implementa o PRD-003. Adiar seria abrir dívida nova no meio da fase que existe para fechar a antiga.

### Caminho posicional abrindo uma aba **na** sessão restaurada

Leitura alternativa do RF-3.12, e o que alguns emuladores fazem. Descartada porque contradiz o texto do requisito, que diz "sem restaurar" com todas as letras — e porque a garantia de não sobrescrever é o valor real do modo.

### Aceitar também um comando para executar (`porecatu -e <cmd>`)

Convenção antiga de emulador de terminal, e certamente pedida um dia. Descartada nesta fase: não é requisito de nenhum PRD aprovado, e cada forma que entra na linha de comando é contrato que não sai mais. Fica para quando houver requisito.

## Consequências

### Positivas

- O RF-3.12 fica implementável, e a dívida da etapa 1 da F4 é paga pela mesma engrenagem — `resolve_config_path` deixa de ter um parâmetro que ninguém preenche.
- `--help` e `--version` existem desde o primeiro dia em que o app aceita argumentos, em vez de serem acrescentados depois de um relato.
- Nenhuma dependência nova, e o parse é uma função pura com teste próprio.
- O modo posicional não pode destruir a sessão gravada, por construção: ele nem chama a gravação.

### Negativas

- Mensagens de erro e `--help` são escritos e mantidos à mão. Com cinco formas é barato; com quinze não seria.
- A assinatura de `porecatu_ui::run()` muda.
- Toda forma aceita aqui vira contrato. É o custo de ter uma linha de comando, e a mitigação é a superfície ser pequena de propósito.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Modo posicional gravar sessão por engano e sobrescrever a do usuário | Baixa | **Alto** | Reusa o caminho de `[session] enabled = false`, um só lugar que decide gravar; teste que roda no modo posicional e afirma que o arquivo não foi tocado |
| Argumento com caractere não-UTF-8 (caminho no Windows) quebrar o parse | Média | Médio | `args_os` e `OsString`, nunca `args()`; caminho nunca passa por `String` |
| Superfície crescer aos poucos até o parser à mão ficar frágil | Média | Médio | Gatilho de revisão escrito no §3 |
| `--help` divergir do que o app aceita | Média | Baixo | Teste que compara as formas aceitas pelo parser com as listadas no texto de ajuda |
