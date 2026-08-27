# ADR-0006 — Modelo de abas e grupos

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** PRD-001, PRD-002, ADR-0005

## Contexto

Os requisitos 1 e 2 pedem abas e agrupamento de abas com nome. A forma exata do modelo de dados decide o que é fácil e o que é impossível depois:

- Grupos podem se sobrepor? Uma aba pode estar em dois grupos?
- Grupos aninham?
- Abas fora de grupo existem, ou tudo é grupo?
- Grupos são contíguos na barra, ou uma etiqueta que pode estar espalhada?

Além disso, o modelo precisa sobreviver ao round-trip de persistência ([ADR-0005](0005-persistencia-de-sessao.md)) — o que impõe requisitos sobre identidade.

Referência mental deliberada: **grupos de abas do Chrome**. É o modelo que os usuários já conhecem, e ele é restritivo de propósito.

## Decisão

### Estrutura

```
Workspace
 └── Vec<Group>          (ordenado; a ordem é a ordem na barra)
      └── Vec<TabId>     (ordenado; a ordem é a ordem dentro do grupo)
```

Com quatro restrições:

1. **Uma aba pertence a exatamente um grupo.** Sem sobreposição.
2. **Grupos não aninham.** Um nível, só.
3. **Grupos são contíguos na barra.** A ordem visual é a ordem do modelo; não existe grupo com abas espalhadas.
4. **Abas "sem grupo" pertencem a um grupo implícito.** Ele não tem nome nem cor, não é desenhado como pílula, e não pode ser renomeado, colapsado ou removido. Existe para que o código tenha um caminho único — o resto da aplicação nunca lida com `Option<GroupId>`.

O último ponto é o que mais paga: sem ele, toda operação de mover, reordenar e persistir precisaria de dois caminhos.

### Identidade

`TabId` e `GroupId` são **inteiros opacos e estáveis**, gerados por contador monotônico por workspace, serializados junto com a sessão.

Estáveis é o requisito operante: o ID é referenciado pela sessão salva, pelo `Wakeup` vindo da thread de leitura do PTY, e pelo estado de drag em andamento. Índice de posição **não serve** como identidade — reordenar uma aba invalidaria referências.

### Operações do domínio

Todas em `porecatu-core`, puras, sem I/O, testáveis:

| Operação | Efeito |
|---|---|
| `new_tab(group, pos)` | Cria aba no grupo, na posição |
| `close_tab(id)` | Remove; se o grupo ficar vazio e for explícito, remove o grupo |
| `move_tab(id, group, pos)` | Move entre grupos ou dentro do grupo |
| `group_tabs(ids, nome, cor)` | Cria grupo com as abas; as abas passam a ser contíguas na ordem em que aparecem **na barra** |
| `ungroup(group)` | Abas voltam ao grupo implícito, na posição do grupo dissolvido |
| `rename_group(id, nome)` / `set_group_color` | Metadados |
| `collapse_group(id, bool)` | Afeta o desenho **e a ordem navegável** (ver nota) |
| `activate_tab(id)` | Define a aba ativa |

> **Correções de fato (2026-08-27), decididas no [ADR-0020](0020-grupos-explicitos.md).**
>
> - `group_tabs` dizia *"na ordem em que foram selecionadas"*. O RF-2.5 e o
>   cenário de aceite do [PRD-002](../prd/prd-002-grupos-de-abas.md) pedem a
>   ordem **da barra** (*"Dado abas nas posições 1, 3 e 5 selecionadas … então as
>   três ficam contíguas na barra, na ordem 1, 3, 5"*), que é o que vale. Ordem
>   de seleção produziria `5, 1, 3` para quem clicou de trás para a frente.
> - `collapse_group` dizia *"só afeta o desenho, não a estrutura"*. O RF-2.15
>   remove as abas colapsadas da navegação sequencial **e do acesso por índice**,
>   o que é a definição de uma ordem, não uma questão de pintura: o ADR-0020
>   separa `visual_order()` de `navigable_order()`, e colapso age na segunda.
> - **O grupo implícito não é único.** A seção "Decisão" o descreve no singular,
>   o que colide com a restrição de contiguidade quando um grupo explícito nasce
>   no meio de abas soltas. Há um grupo implícito por trecho contíguo de abas sem
>   grupo. A promessa que motivava o singular — *"o resto da aplicação nunca lida
>   com `Option<GroupId>`"* — continua valendo: o que muda é a cardinalidade, não
>   a existência.

### Invariantes verificadas em teste

- Todo `TabId` está em exatamente um grupo.
- A ordem dos grupos e das abas é total e sem lacunas.
- Fechar a aba ativa move o foco para a vizinha do mesmo grupo; se não houver, para a vizinha do grupo adjacente — **seguinte antes de anterior em todos os níveis, e grupo colapsado é pulado**, como o [ADR-0020](0020-grupos-explicitos.md) desambigua.
- Colapsar um grupo que contém a aba ativa move o foco para fora dele.
- Round-trip `Workspace -> JSON -> Workspace` preserva IDs, ordem e metadados.

### Múltiplas janelas

`Workspace` é por janela. Cada janela tem seus grupos e abas. Mover aba entre janelas está fora do escopo do v1 (registrado no PRD-001 como não-objetivo) — mas o modelo não impede: é `move_tab` com destino em outro `Workspace`.

## Alternativas consideradas

### Etiquetas (uma aba em vários grupos)

Mais expressivo: uma aba poderia ser "backend" e "produção" ao mesmo tempo. Descartada porque quebra a metáfora visual — uma aba ocupa um lugar na barra, e não há como desenhar uma aba em dois grupos contíguos ao mesmo tempo sem duplicá-la. Expressividade que não é desenhável não é útil aqui.

### Grupos aninhados (árvore)

Grupo dentro de grupo. Descartada por custo de UI desproporcional: exigiria breadcrumb ou barra em múltiplas linhas, e o valor marginal sobre um nível é baixo para o caso de uso (separar contextos de trabalho, não construir hierarquia).

### Grupos não-contíguos

Grupo como pura etiqueta de cor, abas em qualquer posição. Descartada porque destrói a legibilidade que é o ponto do recurso — se as abas de um grupo estão espalhadas, o grupo não ajuda a achar nada.

### `Option<GroupId>` em vez de grupo implícito

Modelagem mais "honesta" em Rust. Descartada por custo de código: dobra os caminhos de toda operação de movimentação, ordenação e persistência, em troca de nada observável pelo usuário.

### Splits/panes dentro da aba

Fora do escopo do v1 por decisão de produto (PRD-000). Registrado aqui porque afeta o modelo: se entrar, `Tab` passa a conter uma árvore de panes em vez de um terminal. O modelo atual não impede — `Tab` já é uma struct, não um alias de terminal — mas a mudança seria real e mereceria ADR próprio.

## Consequências

### Positivas

- Modelo pequeno o bastante para caber na cabeça e em testes unitários rápidos.
- Estrutura em árvore rasa mapeia direto para JSON de sessão, sem tradução.
- IDs estáveis resolvem simultaneamente persistência, roteamento de `Wakeup` e drag & drop.
- Grupo implícito elimina uma classe inteira de código condicional.

### Negativas

- Sem sobreposição nem aninhamento: usuários que queiram organização multidimensional não são atendidos.
- A restrição de contiguidade significa que agrupar abas espalhadas **as reordena**. Isso precisa ser visível na UI (animação de movimento), senão surpreende.
- Grupo implícito é um caso especial que existe no modelo e precisa ser documentado, ou alguém tenta renomeá-lo.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Reordenação ao agrupar surpreender o usuário | Média | Baixo | Animar o movimento das abas ao formar o grupo |
| Grupo implícito vazar na UI como grupo editável | Média | Baixo | Tipo distinto ou flag verificada em teste; nunca desenhado como pílula |
| Overflow da barra com muitos grupos | Alta | Médio | Colapso de grupo e scroll da barra fazem parte do PRD-002, não são melhoria futura |
| Pressão por splits mudar o modelo depois | Média | Médio | `Tab` já é struct própria; a troca é localizada e teria ADR próprio |
