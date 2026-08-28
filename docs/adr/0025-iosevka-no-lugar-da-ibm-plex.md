# ADR-0025 — Iosevka no lugar da IBM Plex

**Status:** Superseded by ADR-0026 (só a escolha de família para o chrome; a Fixed no terminal e o recorte continuam valendo)
**Data:** 2026-08-28
**Supersedes:** ADR-0024
**Relacionados:** ADR-0009, ADR-0010, ADR-0016, ADR-0018, PRD-004, PRD-005

## Contexto

O [ADR-0016](0016-fontes-embutidas.md) escolheu a IBM Plex por um motivo que continua de pé: sem fonte embutida, o critério *"o binário com a config padrão bate com o mockup"* não é verificável, porque a métrica varia com o que a máquina tem instalado. A escolha da **família**, no entanto, veio do design, não de um levantamento do que um emulador de terminal precisa desenhar.

E um emulador de terminal desenha mais que texto. Um usuário relatou que `btop` e o Claude Code CLI saíam quebrados. Medindo o `cmap` da IBM Plex Mono contra o que essas TUIs usam:

| Bloco | Para quê | IBM Plex Mono |
|---|---|---|
| Box drawing `U+2500–257F` | molduras de `btop`, `vim`, Claude Code | 128/128 |
| Elementos de bloco `U+2580–259F` | barras de progresso | 32/32 |
| **Braille `U+2800–28FF`** | **os gráficos do `btop`** | **0/256** |
| Formas geométricas `U+25A0–25FF` | marcadores de TUI | 1/96 |
| Setas `U+2190–21FF` | | 22/112 |
| Dingbats `U+2700–27BF` | o `✽` do spinner do Claude Code | 2/192 |
| Símbolos `U+2600–26FF` | | 0/256 |
| Powerline `U+E0B0–E0BF` | prompts de shell | 0/16 |

Molduras funcionavam; tudo que fosse gráfico, indicador ou prompt decorado, não. E não havia como o usuário perceber que era falta de glyph: o `fontdb` do projeto não chamava `load_system_fonts`, então não havia fallback nenhum e o `TextRun` saía **vazio** — sem erro, sem log, sem tofu.

A falta de fallback já foi corrigida à parte, porque era um bug contra o próprio ADR-0016 (que sempre mandou a cadeia do RF-5.2 vir do sistema). Mas isso deixa a cobertura de uma TUI dependendo do que a máquina tem — exatamente o que o ADR-0016 se recusou a aceitar para a métrica de texto.

## Decisão

**Trocar a IBM Plex pela [Iosevka](https://typeof.net/Iosevka/) como família do design**, nas duas frentes:

| Uso | Antes | Agora |
|---|---|---|
| Conteúdo do terminal | IBM Plex Mono 400/500 | **Iosevka Fixed** 400/500 |
| Chrome (aba, rótulo, menu) | IBM Plex Sans 400/500/600 | **Iosevka Aile** 400/500/600 |
| Ícones do chrome | Lucide (ADR-0024) | Lucide, sem mudança |

Tudo que o ADR-0016 decidiu sobre *como* embutir continua valendo com o mesmo texto: TTF estático, `include_bytes!` sem I/O de disco, faces do design com precedência sobre a cópia do sistema, emoji e CJK fora do binário. Este ADR troca **qual** família, e revoga uma cláusula: a proibição de subsetting, que existia por causa da licença da IBM Plex e não se aplica à Iosevka.

### Por que Iosevka

Medida contra as duas alternativas que estavam na mesa:

| | IBM Plex Mono | Google Sans Code | **Iosevka Fixed** |
|---|---|---|---|
| Box drawing | 128/128 | 128/128 | 128/128 |
| Braille | 0/256 | 0/256 | **256/256** |
| Geométricos | 1/96 | 16/96 | **96/96** |
| Setas | 22/112 | 5/112 | **112/112** |
| Powerline | 0/16 | 0/16 | **16/16** |
| Cirílico | 192/256 | 0/256 | **256/256** |
| Total de codepoints | 1049 | 670 | **7582** |

A Google Sans Code foi considerada primeiro, a pedido, e **descartada por medição**: cobre menos que a IBM Plex Mono no que interessa, não tem braille, e só existe como fonte variável — que o ADR-0016 deixou explicitamente pendente de verificação prática.

A Iosevka resolve o problema na origem: os blocos que uma TUI desenha estão na face, no avanço da célula, sem depender de fallback nem de máquina. É uma fonte desenhada para terminal, e a variedade de pacotes dela deixa a escolha explícita em vez de acidental.

### Fixed, não Term — por causa das ligaduras

A Iosevka distribui variantes; duas importavam:

- **Term** — glyphs mais estreitos para caber na célula, **com** ligaduras contextuais (`calt`).
- **Fixed** — **sem** ligaduras.

Escolhida a **Fixed**. Ligadura é substituição de N glyphs por 1 durante o shaping: `!=` viraria um glyph só, com um avanço só, e a linha inteira sairia da grade. O `paint.rs` verifica avanço **por caractere**, e essa verificação não pega substituição de grupo — o desalinhamento passaria. A IBM Plex Mono não tinha ligaduras, e por isso o problema nunca existiu; ele nasceria com a troca.

O recorte (abaixo) mantém só `ccmp,locl,mark,mkmk` de layout, então a ausência de ligaduras é garantida por construção, não por confiança na variante.

### Subsetting, agora permitido

O ADR-0016 **proibiu** subsetting, e por um motivo correto: a OFL da IBM Plex traz a cláusula de Reserved Font Name "Plex", que trata recorte como modificação e obrigaria a renomear a família — quebrando `family = "IBM Plex Mono"` na config e na especificação.

A OFL da Iosevka **não tem Reserved Font Name**. Recortar mantendo o nome é permitido, e aqui é necessário: as faces originais têm ~8.7 MB (Fixed) e ~10.7 MB (Aile) cada, porque o pacote carrega todos os conjuntos estilísticos alternativos. Cinco faces assim somariam **~48 MB** — o mesmo custo que o ADR-0016 recusou para emoji e CJK, e pelo mesmo motivo.

O recorte é feito por [`scripts/subset-fonts.py`](../../scripts/subset-fonts.py), que documenta cada faixa mantida com o motivo dela, e leva as cinco faces a **2.1 MB somadas**. Roda à mão quando a versão da Iosevka sobe; o resultado é versionado em `assets/fonts/`, então nem o build nem o CI dependem do `fonttools`.

O que fica de fora sai pela cadeia de fallback do sistema, que é a mesma divisão do ADR-0016: o binário garante o que o design promete, o sistema cobre o resto do Unicode.

### Licenciamento

Iosevka é **SIL OFL 1.1**, Copyright © 2015-2026 Renzhi Li — a mesma licença da IBM Plex, sem a cláusula de Reserved Font Name. Compatível com a distribuição junto de software GPLv3; é a conferência que o [ADR-0010](0010-licenciamento.md) exige de todo componente novo.

O texto acompanha a distribuição em `assets/fonts/LICENSE-OFL-iosevka.txt`, nunca no `LICENSE` da raiz, cujo hash o workflow `docs` verifica. O `LICENSE-OFL.txt` da IBM Plex sai junto com as faces dela.

## Alternativas consideradas

### Ficar na IBM Plex e depender só do fallback do sistema

Zero mudança de identidade visual, zero bytes a mais, e `btop` volta a renderizar — porque o fallback foi ligado de qualquer forma.

Descartada porque devolve ao ambiente uma garantia que o projeto tinha comprado. Numa máquina sem fonte com braille, os gráficos do `btop` voltam a sumir; numa com, eles aparecem com a métrica de outra fonte, encolhidos para caber na célula. É a mesma condicional de ambiente que o ADR-0016 recusou para a métrica de texto, agora aplicada ao desenho.

### Iosevka só como face de fallback embutida, mantendo a IBM Plex visível

Preservaria o design e garantiria cobertura por metade do peso.

Descartada por não ser controlável: a ordem de fallback do `cosmic-text` não é exposta, então não há como afirmar que a Iosevka embutida seria consultada antes das fontes do sistema. Seria uma promessa que o código não sustenta.

### Trocar só a mono, mantendo a IBM Plex Sans no chrome

Menos churn de documentação, e o chrome não precisa de braille.

Descartada por coerência: duas superfamílias no mesmo binário para resolver um problema que uma resolve. A Iosevka Aile é a face proporcional da mesma família, desenhada para conviver com a Fixed.

### Google Sans Code

Ver a tabela acima. Descartada por medição, não por preferência.

## Consequências

### Positivas

- `btop`, Claude Code CLI e qualquer TUI com braille, powerline ou marcador geométrico desenham — **em qualquer máquina**, que é a promessa que o ADR-0016 fez e a família anterior não sustentava.
- O fallback do sistema deixa de ser o caminho normal e volta a ser o que sempre devia: exceção para emoji e CJK.
- Menos glyph de fallback significa menos quebra de `TextRun` no `paint.rs`, que é o caminho lento da grade.
- Subsetting deixa de ser proibido, o que dá controle real sobre o tamanho do binário — e já foi usado: 48 MB viraram 2.1 MB.
- A Iosevka Fixed não tem ligaduras, então a grade fica imune à classe de bug que o `paint.rs` não sabe detectar.

### Negativas

- **A identidade visual muda.** O mockup, a especificação e as capturas de tela mostram IBM Plex; o binário agora mostra Iosevka, que é bem mais estreita. A célula do terminal encolhe e cabe mais coluna na mesma largura.
- As faces embutidas passam de ~950 KB (cinco IBM Plex) para ~2.1 MB (cinco Iosevka recortadas).
- Entra um passo manual: subir a versão da Iosevka exige rodar `scripts/subset-fonts.py` e versionar o resultado.
- O recorte é uma decisão a manter: um bloco esquecido nele vira "não desenha" silencioso — a mesma classe de falha que originou este ADR, agora causada por nós.
- Mais uma licença OFL a acompanhar, com atribuição diferente.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Alguém trocar a Fixed pela Term e reintroduzir ligaduras | Baixa | Alto | O recorte mantém só `ccmp,locl,mark,mkmk`; ligadura não sobrevive a ele nem se a variante mudar |
| Um bloco necessário ficar de fora do recorte | Média | Médio | A lista tem o motivo de cada faixa ao lado; o que escapa cai no fallback do sistema em vez de sumir |
| Teste amarrado à largura de avanço da fonte | Média | Baixo | Já aconteceu na troca: quatro testes de `paint.rs` fixavam a célula em 8.4 px. Hoje derivam de `measure_mono_cell`, como o runtime |
| Subir a Iosevka e esquecer o recorte | Média | Baixo | O script é a única forma documentada de gerar `assets/fonts/`; o tamanho do arquivo denuncia |
