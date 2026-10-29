# ADR-0078 — Largura e quebra de texto do diálogo: piso 360, teto 720, nunca reticências

**Status:** Aceito
**Data:** 2026-10-29
**Relacionados:** [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md), [ADR-0077](0077-avisos-do-app-em-dialogo-modal.md)

## Contexto

Na 0.9.7 o default de `appearance.dialog.width` caiu de 760 para 360, e os avisos do app passaram a ser diálogos ([ADR-0077](0077-avisos-do-app-em-dialogo-modal.md)). O diálogo só crescia para botões, linhas de dado e a linha da alternância; **título, corpo, linhas de texto e de aviso e a nota do cofre** eram cortados com reticências (`TextMeasurer::truncate`) na largura do piso, e o título nem era medido. Resultado: o convite de integração de shell aparecia num modal de 360 numa janela de 800, com as frases cortadas — o texto era ilegível e sobrava janela. O ADR-0077 listava o corte como risco aceito, calculado para a largura 760 que não valia mais.

O dono do produto pediu: piso 360; crescer até o dobro (720), nunca além da janela com 20 de margem de cada lado; e, passando disso, quebrar o texto em várias linhas, **nunca** esconder atrás de reticências.

## Decisão

1. **Largura.** `largura = clamp(texto mais largo + 2·padding, piso, teto)`, com `piso = min(appearance.dialog.width, janela − 40)` e `teto = min(2 · appearance.dialog.width, janela − 40)`. Botões e a linha da alternância ("Lembrar…", a caixa de excluir perfil) não quebram: ampliam a largura além do teto se preciso, mas nunca além de `janela − 40`.
2. **Quebra.** O texto que não cabe na largura final quebra em linhas com `wrap_body` — o mesmo do aviso —, entre palavras; uma palavra mais larga que a linha (caminho de chave, impressão digital) parte por caractere. Vale para título (com a face do título), corpo, linhas (`Data`, `Warning`, `Text`) e a nota. **Nada é cortado com reticências**: o caminho de chave deixa de ser cortado no meio, e `SecretFacts::body_is_path` some.
3. **Altura.** Uma linha de texto custa o corpo da fonte, como antes; cada linha de continuação custa a entrelinha `appearance.notices.line_height` (16) — valor que já existe, mesma fonte de corpo, nenhum valor de aparência novo. O `gap` separa só título, corpo e linhas entre si. Uma função (`dialog_block_height`) dá a altura de um bloco, usada pelo layout e pelo paint.
4. **Onde se quebra.** Em `layout_dialog`, uma vez; o resultado vai em `DialogLayout` (`title_rows`, `body_rows`, `line_rows`, `note_rows`) e o paint só desenha. O layout roda por quadro e por clique: cada texto é medido **uma vez**, e só o texto que passa da largura é quebrado.
5. **Constantes, não config.** O teto (`2 ×` a largura configurada) e a margem de 20 são constantes de `overlay.rs`, sem chave no `porecatu.toml`.

## Consequências

- O diálogo nunca esconde texto atrás de reticências; numa janela estreita ele fica menor e mais alto.
- O corpo do diálogo deixa de ser "uma linha": a espec §2.15 descreve a largura, a quebra e a entrelinha.
- `Terminal`/catálogo não mudam; nenhuma frase de interface foi alterada.
- Recuo (espaços iniciais) de uma linha de texto que **precisa** quebrar se perde, porque a quebra junta palavras com um espaço; linha que cabe é desenhada intacta.
- Botão que passa da janela com a margem continua estourando o modal; é o caso raro de três saídas em janela minúscula.

## Revisões

Revê parcialmente: [ADR-0077](0077-avisos-do-app-em-dialogo-modal.md) (risco "linha longa cortada") e [ADR-0075](0075-superficies-de-ui-do-ssh.md) §2 (caminho de chave "truncado no meio") e §3 ("cortada ao conteúdo"). Cada um ganha um blockquote apontando para este ADR.

## Riscos

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Modal alto em janela baixa (muitas linhas quebradas) | Baixa | Baixo | Os avisos do catálogo têm poucas linhas; o modal continua centrado |
| Entrelinha 16 e a nova largura ainda sem aval visual do dono | Alta | Baixo | Registrado no roadmap como pendente |
