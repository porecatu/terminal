# ADR-0024 — Face de ícones embutida (Lucide)

**Status:** Superseded by [ADR-0025](0025-iosevka-no-lugar-da-ibm-plex.md)
**Data:** 2026-08-28
**Supersedes:** ADR-0016
**Relacionados:** ADR-0009, ADR-0010, ADR-0018, PRD-004, PRD-005

## Contexto

O [ADR-0016](0016-fontes-embutidas.md) decidiu embutir no binário as faces do design e fechou a lista: *"Exatamente as que a [seção 1.1](../design/especificacao-visual.md) lista — nada além"* — IBM Plex Mono 400/500 e IBM Plex Sans 400/500/600. À época deste ADR o `fontdb` do projeto **nunca** chamava `load_system_fonts`, o que dava a precedência prometida ali ao custo de não haver cadeia de fallback nenhuma — coisa que o próprio ADR-0016 exigia ("permanece **fora do binário**, vinda do sistema"). Isso foi corrigido depois: hoje as embutidas são registradas **antes** das do sistema, o que dá precedência e fallback ao mesmo tempo. O parágrafo abaixo descreve o estado em que os ícones sumiram, que é o que motivou esta decisão.

As duas regras se combinam num efeito que a F3 entregou sem perceber: **um codepoint que nenhuma das cinco faces cobre não tem fallback nenhum e simplesmente não desenha.**

E o chrome usa codepoints assim. A especificação visual descreve os ícones da barra com os glyphs Unicode que um navegador desenharia — "✕ 10px" na §2.5, "▶ 8px" na §2.4 — e a implementação os tomou ao pé da letra, pedindo-os à IBM Plex Sans:

| Glyph | Codepoint | Onde | Na IBM Plex Sans |
|---|---|---|---|
| ✕ | U+2715 | botão de fechar da aba, botão de fechar do aviso | **ausente** |
| ▶ | U+25B6 | caret da pílula, grupo colapsado | **ausente** |
| ▼ | U+25BC | caret da pílula, grupo expandido | **ausente** |
| ‹ › | U+2039/203A | pílulas de overflow | presente |
| + | U+002B | botões de nova aba | presente |

Os três primeiros não apareciam na tela. Não é bug de cor, de recorte nem de camada: o glyph não existe, o `cosmic-text` não tem para onde cair, e o `TextRun` sai vazio. É o custo, previsto mas não registrado, do *"sem cópia do sistema para competir"* do ADR-0016 — e o preço aparece justamente nos ícones, que é onde o texto do chrome sai do alfabeto latino.

Trocar por glyphs que a Plex tem (`x` minúsculo, `>` maiúsculo) resolveria o desenho e afundaria o resultado: ícone de fechar não é a letra x, e a §1.7 dá a ele uma caixa de 17×17 própria.

## Decisão

**Embutir uma sexta face, só de ícones: [Lucide](https://lucide.dev), em `assets/fonts/Lucide.ttf`.** Ela ganha uma variante própria de `FontFace` — `FontFace::Icon`, sem peso — e os codepoints em uso são nomeados em `porecatu_render::icon`.

Este ADR **substitui o ADR-0016 por inteiro**: tudo que aquele decidiu continua valendo com o mesmo texto — as cinco faces do design, TTF estático, `include_bytes!` sem I/O de disco, precedência sobre a cópia do sistema, emoji e CJK fora do binário, sem subsetting da IBM Plex. O que muda é uma cláusula: a lista de faces embutidas deixa de ser *"exatamente as da seção 1.1, nada além"* e passa a ser **as cinco faces de texto do design mais uma face de ícones**.

### Por que uma face, e não uma primitiva de ícone

`porecatu-render` não tem primitiva de path nem de rotação (é o mesmo motivo de o caret trocar de glyph em vez de girar, e de a sombra de popover continuar em falta na [seção 4.4](../design/especificacao-visual.md) da especificação). Desenhar um `✕` a partir de quads exigiria rotação; desenhar a partir do SVG do Lucide exigiria tesselação — um subsistema novo de render para cinco desenhos.

Como face, o ícone passa **pelo mesmo caminho que todo o resto do texto**: mesmo atlas de glyphs, mesma medição por `TextMeasurer`, mesmo recorte por camada do [ADR-0018](0018-composicao-de-frame.md). Não há código novo de pipeline — só uma entrada a mais no `fontdb` e um braço a mais no `match` de `attrs_for`.

E mantém `porecatu-render` sem domínio: `icon::X` é o nome do ícone no catálogo da fonte, não "o botão de fechar da aba".

### Por que Lucide

- **Licença ISC**, compatível com a GPLv3 — a conferência que o [ADR-0010](0010-licenciamento.md) exige de todo componente novo, e este parágrafo é o registro dela. O texto acompanha a distribuição em `assets/fonts/LICENSE-ISC-lucide.txt`, nunca no `LICENSE` da raiz (cujo hash o workflow `docs` verifica).
- **Sem cláusula de Reserved Font Name**, ao contrário da OFL da IBM Plex: subsetting aqui seria permitido, o que deixa a porta aberta se o tamanho incomodar.
- Grade de 24px com traço uniforme, que é a mesma linguagem visual dos ícones desenhados no mockup — traço, não preenchimento.
- Distribuída como TTF pronta (pacote `lucide-static`), com os codepoints mapeados na Área de Uso Privado. Nada a gerar em tempo de build.

### O que **não** é subsetado

A face entra inteira: 2059 ícones, ~840 KB, dos quais `porecatu_render::icon` nomeia cinco. Subsetar exigiria uma dependência de build (`fontTools` ou equivalente) e um passo de geração no repositório para economizar menos de 1 MB num binário que já embute cinco faces de texto. Se o tamanho virar problema, este é o primeiro lugar a cortar — e, ao contrário da IBM Plex, cortar aqui não esbarra em licença.

### `size_px` é a em, não o desenho

É a pegadinha da face, e ela morde de dois jeitos ao mesmo tempo.

Toda glyph do Lucide avança **1 em** e tem o desenho centrado nela, mas o desenho ocupa só parte disso — 0.59 em num `x`, 0.34 em na largura de um `chevron-right`. Tomar os números da especificação como tamanho de fonte (`✕ 10px` → `size_px = 10`) desenha um ✕ de **5.9 px**: foi o que a primeira versão fez, e o relato foi *"os ícones ficaram muito pequenos"*.

Junto vem um efeito de cor que parece outro bug e não é. O traço do Lucide é `2/24` da em: a 10 px ele tem 0.83 px de espessura, o antialiasing o espalha por dois pixels a meia cobertura, e o que chega à tela fica a meio caminho do fundo — o relato foi *"quase da mesma cor do fundo, quase invisíveis"*. A cor estava certa (`#727a86`, o token da §2.5, inalterado); o traço é que era fino demais para render sólido.

**Os ícones do chrome usam em de 20 px** (`chrome::ICON_EM_SIZE`), o que dá desenho de 11.8 px no ✕ e traço de 1.67 px. Os números da especificação (10 px, 15 px, 8 px) passam a ser lidos como tamanho de *desenho*, e o binário desenha maior que eles — divergência registrada na [seção 4.4](../design/especificacao-visual.md), a pedido do usuário depois de ver a primeira versão em tela.

Duas consequências no layout, ambas resolvidas com `Icon::ink_width` em vez do avanço: a pílula de overflow reserva a largura do chevron desenhado (6.8 px), não a em inteira, que estouraria os 34 px dela; e o slot do caret na pílula de grupo vem do caret **mais largo dos dois**, senão a pílula mudaria de largura ao colapsar — justo no gesto que já está animando.

### Centragem

O desenho é centrado na em nos dois eixos, então basta centrar a em — mas não pela metade da altura, como se faz com texto. A face declara ascent = em e descent = 0, e o `cosmic-text` centra o bloco ascent+descent dentro da altura de linha (`size_px * 1.2`, a mesma do medidor e do pipeline), o que põe a baseline em `1.1 * size_px` abaixo do topo do `TextRun`. O centro da em fica a `0.6 * size_px` do topo, não a `0.5`; centrar como texto desenha o ícone `0.1 * size_px` baixo demais.

Tanto essa constante quanto o tamanho do desenho de cada ícone são **medidos**, não estimados: um teste rasteriza cada ícone com o `swash` e compara com o que `icon.rs` declara. Foi ele que corrigiu uma primeira leitura errada da tabela `glyf` do arquivo, que dizia que o desenho era ancorado no canto inferior esquerdo — não é, é centrado.

## Alternativas consideradas

### Ligar `load_system_fonts` como fallback

Uma linha, e os três glyphos passam a desenhar em qualquer máquina que tenha uma fonte com cobertura de setas — o que é quase toda.

Descartada **para o chrome**, e é uma distinção que vale registrar: a cadeia de fallback do sistema foi ligada depois desta decisão (o ADR-0016 sempre a exigiu), mas ela não substitui a face de ícones. Se o ✕ do botão de fechar viesse do sistema, o desenho do chrome passaria a depender de qual fonte a máquina tem, e o critério *"o binário com a config padrão bate com o mockup"* voltaria a ter asterisco de ambiente. *(Nota de clareza, 2026-09-02: aquele critério mudou de forma no [ADR-0028](0028-o-binario-como-referencia-visual.md) — hoje o binário **é** a referência. O argumento sobrevive intacto e vale ainda mais: se o desenho do ícone varia por máquina, não existe referência.)* Pior que na F2: a variação seria de **glyph**, não de métrica — o mesmo botão com desenhos diferentes por máquina. O fallback do sistema serve o **conteúdo do terminal**, que é texto de outra pessoa e não tem paridade a manter; o chrome, que é desenho nosso, vem de face embutida.

### Trocar os glyphs por caracteres que a IBM Plex já tem

`x` no lugar de ✕, `>`/`v` no lugar de ▶/▼. Zero bytes no binário, zero licença nova.

Descartada por qualidade: são letras num lugar onde o design pede ícone, com peso, traço e caixa de letra — a §1.7 dá ao botão de fechar uma caixa de 17×17 e um alvo de acerto com folga de 2px, coisas que se dá a um ícone, não a um caractere de texto.

### Desenhar os ícones com as primitivas existentes

Um ✕ são dois retângulos cruzados; um chevron, dois.

Descartada porque `RoundedQuad` não tem rotação (mesma limitação registrada na §4.4 da especificação). Sem rotação, "dois retângulos cruzados" não é um ✕ — é um `+`. Acrescentar rotação à primitiva para desenhar cinco ícones é um subsistema de render a mais para manter em três plataformas.

### Rasterizar os SVGs do Lucide em PNG no build

Controle total do resultado, sem fonte nova.

Descartada por dois motivos que se somam: `porecatu-render` não tem pipeline de textura de imagem (só quad, quad arredondado e texto), e PNG rasterizado em tamanho fixo sai borrado no HiDPI — o [ADR-0018](0018-composicao-de-frame.md) já decidiu que nada é escalado depois de rasterizado, e é por isso que o texto é shapado direto no tamanho físico. Uma face vetorial herda essa garantia de graça.

## Consequências

### Positivas

- Os ícones do chrome desenham — botão de fechar da aba, caret do grupo, botão de nova aba, chevrons de overflow.
- Desenham **igual em qualquer máquina**, que é a mesma promessa que o ADR-0016 fez para a métrica de texto, agora estendida ao desenho do chrome.
- Nenhum código novo de pipeline: uma entrada no `fontdb`, um braço no `match`, e o ícone herda atlas, medição e recorte do caminho de texto.
- O catálogo de 2059 ícones cobre com folga o que a F4/F5 vão pedir (menu de contexto, editor de grupo, indicadores) — sem decisão nova a cada ícone.
- Subsetting fica disponível como corte de tamanho futuro, porque a ISC não tem a cláusula que a OFL tem.

### Negativas

- Binário cresce ~840 KB — mais que as cinco faces de texto somadas, para cinco desenhos em uso.
- Mais uma licença a acompanhar na distribuição e a citar no README.
- Mais uma fonte a atualizar como tarefa do projeto.
- Os codepoints são da Área de Uso Privado: `"\u{e1b2}"` não diz nada sozinho, e ler o código depende do módulo `icon` manter o nome do catálogo ao lado de cada constante.
- `size_px` deixa de significar "o tamanho do ícone", que é a leitura natural de quem chega. O módulo `icon` documenta a diferença e `Icon::ink_width` dá o outro número, mas é uma armadilha a mais na API.
- A especificação visual continua descrevendo os ícones pelos glyphs Unicode (✕, ▶, ▼). O que o binário desenha é o ícone equivalente do Lucide, não aquele codepoint — divergência registrada na [seção 4.4](../design/especificacao-visual.md).

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Um ícone novo ser usado por codepoint cru, sem passar pelo módulo `icon` | Média | Baixo | `icon.rs` é o único lugar com codepoints; o nome do catálogo fica no doc-comment de cada constante |
| Alguém repetir o erro original e pedir um glyph que nenhuma face cobre | Média | Médio | O teste `every_named_icon_has_a_glyph_in_the_icon_face` mede cada ícone nomeado e falha se a largura for zero |
| A centragem sair errada numa troca de fonte ou de altura de linha | Baixa | Baixo | A constante de baseline e o tamanho do desenho de cada ícone são pinados por teste contra a rasterização real, não escolhidos a olho |
| Atualização do Lucide remapear codepoints da Área de Uso Privado | Média | Médio | O teste de glyph pega o remapeamento na hora; a fonte é pinada como arquivo no repositório, não resolvida em build |
| Tamanho do binário incomodar | Média | Baixo | Subsetting permitido pela ISC — é o primeiro corte disponível |
