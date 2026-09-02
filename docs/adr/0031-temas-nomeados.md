# ADR-0031 — Temas nomeados: escopo, merge e ciclo

**Status:** Aceito
**Data:** 2026-09-02
**Relacionados:** ADR-0003, ADR-0009, ADR-0028, ADR-0030, PRD-004, PRD-005, [catálogo de ações](../reference/acoes.md)

## Contexto

O [PRD-005](../prd/prd-005-aparencia-do-terminal.md) dedica quatro requisitos de uma linha cada aos temas nomeados: o usuário os define no arquivo e seleciona por nome (RF-5.18); chave declarada fora do tema tem precedência sobre a do tema (RF-5.19); trocar o tema aplica a quente (RF-5.20); e há um atalho que **cicla** entre os temas definidos, sem editar o arquivo (RF-5.21, `theme.cycle`, `Ctrl+Shift+Y`).

O que fica indefinido:

- **Escopo.** No [`porecatu.example.toml`](../config/porecatu.example.toml), `[[themes]]` só contém cores de terminal — nada de chrome. Um tema pode mudar a cor da barra de abas? A fonte? A geometria?
- **Merge parcial.** RF-5.19 diz que a chave de fora vence, mas não o que acontece com um tema que declara **metade** de uma paleta.
- **Ciclo.** Em que ordem `theme.cycle` cicla, se o estado "sem tema" participa, e se ciclar **escreve no arquivo** — o RF-5.9 é explícito de que o zoom de fonte não escreve; o tema não diz nada.

Enquanto isso não estiver decidido, a F4 não pode implementar RF-5.18 a RF-5.21 sem inventar as três respostas.

## Decisão

### 1. Um tema é um conjunto de **cores**, e nada mais

Tema pode declarar qualquer chave de **cor** do arquivo — de terminal e de chrome —, e **nenhuma** chave de fonte, dimensão, raio, espaçamento, tempo ou comportamento. `[[themes]]` aceita as subárvores de cor de `[terminal.colors]`, `[appearance.tabs.colors]`, `[appearance.groups]` (as cores dela, mais `palette` e `ungrouped_color`) e as dos cinco widgets.

Por que essa fronteira: tema é o que o usuário troca para mudar de humor, várias vezes por dia, e espera que **nada se mexa de lugar**. Um tema que mudasse `tab_height` refluiria a barra e redimensionaria os PTYs (classe B do [ADR-0030](0030-escopo-do-hot-reload.md)) a cada `Ctrl+Shift+Y` — que é exatamente o que ninguém quer de um atalho de tema. Com a fronteira em cor, `theme.cycle` é sempre classe A: troca o `Arc`, redesenha, e o loop volta a dormir.

O tema **pode** mudar cor de chrome — a alternativa (só terminal) deixaria a barra de abas em desacordo com um tema claro, que é o caso de uso mais óbvio depois do escuro.

### 2. Merge é por chave, com três níveis de precedência

```
chave declarada fora do tema  >  chave do tema ativo  >  default embutido
```

Merge **parcial e por folha**: um tema que declara só `[terminal.colors] background` muda só o fundo do terminal; todo o resto cai no default. Não há "tema completo" nem validação de completude — um tema de uma chave é válido.

Exceção deliberada: `palette` de grupos é **substituída inteira**, não mesclada por índice. Ela é uma lista ordenada em que a posição tem significado (é a ordem de atribuição automática, [ADR-0020](0020-grupos-explicitos.md) §5); mesclar índice a índice produziria uma paleta que ninguém escolheu, com quatro cores de um tema e duas do default.

A regra do RF-5.19 é o que permite adotar um tema e ajustar uma cor sem copiar a paleta. O custo é que o usuário que declarou uma cor solta fora do tema e esqueceu vai ver essa cor "resistir" à troca de tema — e é por isso que o aviso do §4 existe.

### 3. Ciclo: ordem do arquivo, e "sem tema" participa

`theme.cycle` anda na **ordem de declaração** dos `[[themes]]` no arquivo — não alfabética: a ordem do arquivo é a única que o usuário controla sem renomear nada. O estado **sem tema** (`theme = ""`, tudo nos defaults, que são os valores do design) é o **primeiro** da lista e participa do ciclo: sem ele, adotar um tema seria uma via de mão única, e a aparência aprovada do binário ([ADR-0028](0028-o-binario-como-referencia-visual.md)) ficaria inalcançável por atalho.

Com nenhum `[[themes]]` declarado, `theme.cycle` não faz nada — não é erro, é uma lista de um elemento.

### 4. Ciclar **não escreve no arquivo**

Mesma regra do zoom de fonte (RF-5.9), e pela mesma razão: o arquivo é do usuário, o app não o edita — o painel de configurações `[v2]` do [ADR-0009](0009-referencia-visual-e-reconciliacao.md) é a única superfície que um dia escreverá nele, e por ação explícita. O tema escolhido por atalho vale **para a sessão**, e:

- **Uma recarga do arquivo mantém o tema ciclado**, desde que ele ainda exista. O tema de sessão é uma escolha do usuário, não um valor de config, e descartá-lo a cada gravação faria `Ctrl+Shift+Y` parecer aleatório para quem tem o editor aberto.
- Se o tema ciclado **desaparecer** do arquivo, a sessão volta ao `theme` declarado (ou a nenhum) e um aviso de informação diz qual tema sumiu.
- **Persistir o tema de sessão é F5**, junto com o resto do estado de janela ([PRD-003](../prd/prd-003-persistencia-de-sessao.md)); até lá, reiniciar volta ao `theme` do arquivo.
- **O ciclo é do processo, não da janela.** Duas janelas compartilham o `Arc<Config>` (ADR-0030), então ciclar numa muda as duas. Tema por janela seria a primeira config com escopo de janela no projeto, e nada no PRD-005 pede isso.

### 5. Nome desconhecido é aviso, não erro

`theme = "nao-existe"` cai nos defaults e gera aviso citando a chave e os nomes disponíveis — regra 4 do [ADR-0003](0003-formato-de-configuracao.md). Nome duplicado entre dois `[[themes]]` é **erro** de config, pela mesma razão que binding duplicado é ([ADR-0029](0029-enum-de-acao-e-gramatica-de-tecla.md)): não há como o usuário saber qual dos dois está valendo.

## Alternativas consideradas

### Tema pode declarar qualquer chave de aparência

Mais poder, e é o que alguns terminais fazem. Rejeitada pelo §1: transformaria `theme.cycle` numa operação de classe B (recálculo de grade e resize de todos os PTYs) e faria a barra mudar de altura por atalho. Quem quer trocar fonte junto com cor edita o arquivo — é uma mudança que se faz uma vez, não a cada hora.

### Tema apenas de cores de terminal, como o exemplo hoje sugere

Escopo mínimo, sem ambiguidade. Rejeitada porque deixa o chrome fora: um tema claro com a barra de abas escura é um produto quebrado, e o usuário não teria como consertar sem duplicar as cores fora do tema — perdendo, aí sim, a troca por atalho.

### Ciclar escrevendo `theme` no arquivo

Sobreviveria a reinício sem esperar a F5. Rejeitada: o app reescrevendo o arquivo do usuário reordena comentários e formatação (o problema que o ADR-0009 §6 já registrou para o painel `[v2]`), e um atalho não deve modificar arquivo versionado em dotfiles.

### Ordem alfabética no ciclo

Previsível sem olhar o arquivo. Rejeitada: a ordem do arquivo é a que o usuário controla, e a alfabética obriga a renomear temas para reordená-los.

## Consequências

### Positivas

- RF-5.18 a RF-5.21 ficam implementáveis sem inventar semântica.
- `theme.cycle` é garantidamente classe A do ADR-0030: um frame, sem resize de PTY.
- A fronteira "tema = cor" dá ao struct de `Config` uma divisão natural entre o que é paleta e o que é métrica — útil também para o painel `[v2]`.

### Negativas

- Um tema não pode trazer a fonte que o autor dele desenhou junto com as cores; quem publicar um tema precisa dizer "e ajuste a fonte para X".
- A exceção da `palette` (substituição inteira, não merge) é uma regra a mais para o usuário lembrar. Fica escrita ao lado da chave no arquivo de exemplo.
- Tema de sessão que sobrevive a recarga mas não a reinício é um comportamento intermediário até a F5 — explicável, mas não elegante.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Usuário declarar cor fora do tema, esquecer, e concluir que o tema está quebrado | Alta | Baixo | Aviso ao trocar de tema quando há cor declarada fora dele, listando quais chaves estão vencendo o tema |
| Alguém adicionar chave não-cor a `[[themes]]` e esperar que funcione | Média | Baixo | Chave fora do escopo do §1 dentro de um tema é aviso de chave desconhecida, com a razão citada |
| Tema claro expor cor de chrome que o design nunca previu para fundo claro | Média | Médio | Fora do escopo do v1 (tema claro seguindo o sistema é `[v2]`, ver o roadmap); o v1 entrega o mecanismo, não uma biblioteca de temas |
