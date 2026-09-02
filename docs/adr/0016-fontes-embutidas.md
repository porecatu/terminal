# ADR-0016 — Fontes do design embutidas no binário

**Status:** Superseded by [ADR-0024](0024-face-de-icones.md)
**Data:** 2026-08-26
**Relacionados:** ADR-0009, ADR-0010, PRD-004, PRD-005

## Contexto

Duas afirmações do projeto não podem ser verdadeiras ao mesmo tempo hoje.

A primeira está no [CLAUDE.md](../../CLAUDE.md) e no [ADR-0009](0009-referencia-visual-e-reconciliacao.md):

> O binário com a config padrão deve bater com o [mockup](../design/mockup-estatico.html); divergência visível é bug de implementação, não questão de configuração.

A segunda está no [`porecatu.example.toml`](../config/porecatu.example.toml) e na [especificação visual](../design/especificacao-visual.md), seção 1.1: as famílias padrão são **IBM Plex Mono** (terminal) e **IBM Plex Sans** (chrome).

Nenhuma das duas vem instalada no Windows, no macOS ou na maioria das distribuições Linux. Numa máquina limpa, o RF-5.8 age como projetado — cai para uma monoespaçada do sistema e avisa qual família não foi encontrada — e o binário com a config padrão **não bate com o mockup**. A métrica não é opinável: métrica de fonte diferente muda largura de célula, altura de linha, largura de aba e onde o título trunca.

Isso torna o critério de saída da F2 (*"barra de abas confere com o mockup"*) e o da F4 (*"o binário com a config padrão bate com o mockup"*) inalcançáveis como estão escritos — não por bug, por premissa faltando.

> **Nota de clareza (2026-09-02), sem mudança de decisão.** O critério que este
> ADR cita — *"o binário com a config padrão bate com o mockup"* — **não existe
> mais nessa forma**: o [ADR-0028](0028-o-binario-como-referencia-visual.md)
> inverteu a autoridade visual, e é o binário que define a aparência, com a
> especificação passando a descrevê-lo. A decisão tomada aqui não muda por isso,
> e o argumento dela fica mais forte, não mais fraco: a razão de embutir a fonte
> é que **métrica de fonte é invariante de layout** — ela decide largura de
> célula, de aba e onde o título trunca. Com o binário como referência, uma face
> vinda do sistema faria a referência mudar de máquina para máquina, que é
> exatamente o que não pode acontecer.


A decisão precisa sair antes da F1, porque é a F1 que constrói o caminho de carregamento de fonte.

## Decisão

**Embutir no binário as cinco faces que a especificação visual declara.** O app registra essas faces na base de fontes ao iniciar; elas não dependem do sistema.

### Quais faces

Exatamente as que a [seção 1.1](../design/especificacao-visual.md) lista — nada além:

| Família | Pesos | Uso |
|---|---|---|
| IBM Plex Mono | 400, 500 | conteúdo do terminal, badges, chips, contadores |
| IBM Plex Sans | 400, 500, 600 | títulos de aba, rótulos de grupo, menus |

Itálico e negrito além desses pesos continuam **sintetizados**, que é o default já declarado (`synthesize_bold = true`, `synthesize_italic = true`, RF-5.4). Embutir face que o design não pede seria inventar valor pela porta de trás.

### Formato e carregamento

TTF estático, um arquivo por face, em `assets/fonts/`. Embutidas com `include_bytes!` e registradas na base de fontes no start — **sem I/O de disco**, o que respeita a regra do [ADR-0007](0007-modelo-de-threading.md) de nunca carregar fonte de disco na main thread.

Fonte variável cobriria os três pesos do Sans num arquivo só e reduziria o tamanho. Fica como otimização a avaliar na F1, não como decisão: a seleção de instância nomeada em `cosmic-text`/`fontdb` precisa de verificação prática antes de virar compromisso.

### Precedência sobre a cópia do sistema

As faces embutidas **vencem** para essas duas famílias, mesmo que o usuário tenha IBM Plex instalada. É o ponto todo: o default precisa ser idêntico em qualquer máquina, senão o critério de saída volta a depender do ambiente de quem testa.

Isso não fecha nada para o usuário. Qualquer outra família configurada em `[terminal.font]` ou `[appearance.tabs]` continua vindo do sistema, e a cadeia de fallback do RF-5.2 continua funcionando.

### O que **não** é embutido

A cadeia de fallback default — `Symbols Nerd Font Mono`, `Noto Color Emoji`, `Noto Sans CJK` — permanece **fora do binário**, vinda do sistema.

Isso é deliberado e tem consequência honesta: numa máquina sem fonte de emoji, emoji continua saindo como retângulo vazio. Embutir cobertura de emoji e CJK acrescentaria dezenas de megabytes ao binário para resolver um caso que o RF-5.2 já endereça por configuração. O mockup não exibe emoji nem CJK; a garantia de paridade visual cobre o que o mockup mostra.

### Licenciamento

IBM Plex é distribuída sob **SIL Open Font License 1.1**, licença livre e compatível com a distribuição junto de software GPLv3 — o [ADR-0010](0010-licenciamento.md) exige conferir a licença de todo componente novo, e este ADR é esse registro.

Duas obrigações práticas:

1. **O texto da OFL acompanha a distribuição.** Vai em `assets/fonts/LICENSE-OFL.txt`, com a atribuição de copyright. **Nunca** no `LICENSE` da raiz, que é cópia verbatim da GPL e tem o hash verificado pelo workflow `docs`.
2. **Não fazer subsetting.** Reduzir a fonte para só os glyphs usados é permitido pela OFL, mas conta como modificação — e a cláusula de Reserved Font Name proíbe manter o nome original numa versão modificada. Renomear quebraria `family = "IBM Plex Mono"` na config e na especificação visual. O ganho de tamanho não paga a confusão.

## Alternativas consideradas

### Trocar o default por fontes garantidas em cada plataforma

Consolas no Windows, SF Mono no macOS, DejaVu Sans Mono no Linux. Zero peso no binário e cada plataforma com a fonte que seus usuários já conhecem.

Descartada por dois motivos que se somam. O mockup deixaria de ser testável — não existe "o default" para comparar, existem três. E contraria o princípio 6 do PRD-000, *"cross-platform de verdade"*: o produto teria três aparências padrão, e a captura de tela do README seria verdade em uma máquina só.

### Relaxar o critério para "bate quando as fontes do design estão instaladas"

Honesto e de graça. É a alternativa mais tentadora.

Descartada porque transforma uma asserção verificável numa condicional que ninguém verifica. O valor da regra do ADR-0009 está em ser binária: abriu o binário, comparou com o mockup, divergiu, é bug. Com asterisco de ambiente, ela deixa de pegar regressão — e era exatamente para pegar regressão que ela foi escrita.

### Baixar as fontes no primeiro start

Binário pequeno e fonte sempre atualizada.

Descartada por acrescentar dependência de rede ao caminho de inicialização de um emulador de terminal, que é ferramenta de trabalho offline. Ainda exigiria manter um espelho, validar integridade do download e decidir o que fazer quando falha — trabalho e superfície de falha desproporcionais a menos de 1 MB de binário.

### Embutir também emoji e CJK

Cobertura total de glyph sem depender do sistema, eliminando o retângulo vazio de vez.

Descartada por tamanho: Noto Color Emoji e Noto Sans CJK somam dezenas de megabytes, contra menos de 1 MB das cinco faces do design. O RF-5.2 já resolve por configuração, e o mockup não exibe nenhum dos dois.

## Consequências

### Positivas

- O critério de saída da F2 e da F4 passa a ser alcançável: *"bate com o mockup"* vira afirmação sobre o binário, não sobre a máquina.
- A regra "nenhum valor inventado" ganha garantia real — a métrica de fonte, que decide largura de célula e de aba, deixa de variar por ambiente.
- Primeiro start não depende de fonte instalada, e a captura de tela do README vale para qualquer usuário.
- Sem I/O de disco para as fontes do default, coerente com o ADR-0007.

### Negativas

- Binário cresce; a estimativa é abaixo de 1 MB para as cinco faces, a confirmar na F1.
- Arquivos binários entram no repositório. O [.gitattributes](../../.gitattributes) já marca `*.ttf` como binário, então não há risco de normalização de fim de linha.
- Mais uma licença a acompanhar na distribuição, e uma obrigação de atribuição que não existia.
- Emoji e CJK continuam dependendo do sistema: a paridade com o mockup cobre o que o mockup mostra, não todo Unicode.
- Atualizar a IBM Plex passa a ser tarefa do projeto, não do sistema do usuário.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Alguém fazer subsetting para cortar tamanho e violar a cláusula de Reserved Font Name | Média | Médio | Proibição explícita nesta decisão; o ganho de tamanho não justifica |
| Texto da OFL acabar no `LICENSE` da raiz | Baixa | Médio | O workflow `docs` verifica o hash do `LICENSE` e falha; arquivo próprio em `assets/fonts/` |
| Face embutida não vencer a cópia do sistema em alguma plataforma | Média | Médio | Registro explícito na base de fontes com precedência; teste nas três plataformas no critério de saída da F4 |
| Fonte variável parecer economia e quebrar seleção de peso | Média | Baixo | Decisão é TTF estático; variável só entra com verificação na F1 |
| Usuário achar que o app ignora a IBM Plex que ele instalou | Baixa | Baixo | Comportamento documentado no arquivo de exemplo; qualquer outra família continua vindo do sistema |
