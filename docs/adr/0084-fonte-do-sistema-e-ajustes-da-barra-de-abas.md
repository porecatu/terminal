# ADR-0084 — Fonte do sistema no chrome e ajustes da barra de abas

**Status:** Aceito — aval visual do dono do produto dado em 2026-11-04 (só no Windows; macOS e Linux não foram vistos)
**Data:** 2026-11-04
**Relacionados:** [ADR-0016](0016-fontes-embutidas.md), [ADR-0018](0018-composicao-de-frame.md), [ADR-0022](0022-animacao-de-interface.md), [ADR-0024](0024-face-de-icones.md), [ADR-0026](0026-chrome-unificado-em-iosevka-fixed.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0065](0065-fonte-unica-para-terminal-e-chrome.md), [ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md), [ADR-0082](0082-botoes-da-barra-com-o-tamanho-do-botao-de-menu.md)

## Contexto

O dono do produto fez uma série de ajustes visuais experimentais na barra de abas e no chrome, viu o resultado no binário (Windows) e aprovou. Todos mexem em seções da [especificação visual](../design/especificacao-visual.md) cobertas pelo [ADR-0032](0032-interface-do-v1-fechada.md), que exige ADR para isso. Este registro junta as decisões, que saíram juntas:

- a barra de abas cheia mostrava dois botões de chevron (um por ponta) sobre a trilha;
- o rótulo do grupo era uma pílula com fundo, rim de vidro e chevron, em cima da cápsula do mesmo tom;
- as abas de dentro de um grupo tinham borda, sobre a cápsula de cor cheia;
- o "x" da aba não reagia a hover nem a clique, ao contrário do "+";
- todo texto do chrome saía em Iosevka Fixed ([ADR-0026](0026-chrome-unificado-em-iosevka-fixed.md), [ADR-0065](0065-fonte-unica-para-terminal-e-chrome.md)), a fonte do terminal, e o dono quis a fonte de interface do sistema;
- um clique com o botão direito sobre os botões da zona fixa da direita disparava o menu de uma aba escondida embaixo deles.

## Decisão

### 1. Os botões de abas fora da vista saem

A trilha continua rolando pela roda do mouse (passo `[appearance.tabs.overflow] scroll_step`, 90px) e para trazer a aba ativa à vista (RF-1.18). Saem o desenho, o clique e os dois nós de acessibilidade ([ADR-0043](0043-arvore-de-acessibilidade.md)) dos indicadores, e com eles a contagem de abas ocultas (`Overflow` guarda só o deslocamento já saturado).

Saem do schema `[appearance.tabs.overflow] indicator_size` e `edge_inset`, a cor `[tabs] overflow_indicator_background` dos temas (e `ResolvedPalette::overflow_count_background`) e as frases `access.tabs_hidden_left`/`right` dos catálogos. Arquivo do usuário que ainda as traga cai no aviso de chave desconhecida, como qualquer outra.

### 2. O rótulo do grupo é só o nome, e o colapso vira pontos

- Sem fundo, sem borda de vidro, sem chevron e sem o espaço reservado a ele. Sem hover por brilho (`[appearance.groups] label_hover_brightness` sai); o clique continua alternando o colapso e o duplo clique abrindo o editor. Quem dá a cor do grupo é a cápsula atrás.
- **Colapsado**, um ponto de `indicator_dot_size` (6px) por aba do grupo, à direita do nome, com `tab_gap` (4px) entre os pontos e **8px** (`COLLAPSED_DOTS_GAP`, em `tab_bar.rs`) entre o nome e o primeiro ponto e entre o último e a borda da pílula, no lugar do padding direito. Expandido não reserva nada além do `label_padding_right`. Os pontos têm a cor do nome (`group_new_tab_icon`).
- O ponto agregado de atividade/campainha (RF-2.16) continua à esquerda do nome.

  > **Revisto pelo [ADR-0085](0085-ponto-por-aba-no-grupo-colapsado-e-margem-do-x.md):** o ponto agregado saiu; o ponto da aba com atividade/campainha leva a cor do indicador.
- O **fantasma de arraste** do grupo ([espec. §2.19.1](../design/especificacao-visual.md)) mantém o preenchimento na cor do grupo (`label_alpha`), **sem borda**: flutua sobre a barra, e só com o texto não teria onde se apoiar.

### 3. Aba de dentro de grupo não leva borda

Vale também para a aba que esmaece ao colapsar. A borda de seleção continua (é feedback, RF-2.2), e a aba solta mantém a dela.

### 4. Aba 20px mais estreita

O teto do rótulo (`TabBarStyle::label_max_width`, constante, não chave) vai de 180 para **160px**, e a aba fixa de 229 para **209px** (10 + 160 + 8 + 25 + 6). `max_width` (260) continua só como saturação. `min_width` do arquivo de exemplo, que nunca teve consumidor, passa a 209 para continuar descrevendo a largura real.

### 5. O "x" da aba tem hover e pressionado como o "+"

Pintado pelo mesmo `paint_zone_icon_button` ([ADR-0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md)), com as mesmas cores (`window_controls.hover_background`/`pressed_background`), e disparando no release como já fazia. O quadrado revelado tem `TabBarStyle::close_hover_size()` = `right_zone_button_size` − 2 × `wrapper_padding` = **24px**: parecido com o do "+" (30), mas menor para caber com 5px de folga numa aba de 34px de grupo. A altura do retângulo do botão passa de 17 para 24 para a área de clique cobrir o quadrado; largura (25) e desenho do ícone não mudam, e `close_button_size` (17) segue valendo para o "x" de excluir das linhas do popover de sessões.

### 6. A fonte de interface do sistema

Uma face nova, `FontFace::System { weight }` (em `porecatu-render`), pede ao `fontdb` a família genérica `sans-serif`, e `TextMeasurer` a registra na fonte de interface da plataforma: **Segoe UI** no Windows e **Helvetica Neue** no macOS; no Linux fica o padrão do `fontdb`/fontconfig. Não é embutida ([ADR-0016](0016-fontes-embutidas.md) segue valendo para Iosevka e Lucide) e não tem chave de configuração.

Usam a face do sistema: o rótulo da aba e o campo de renomear, o nome do grupo, a barra de status, as telas de configurações e de edição de perfil (`BODY_FONT`/`TITLE_FONT` de `settings::content`) e tudo o que o `overlay` pinta com `overlay::BODY_FONT`/`TITLE_FONT` — menus, popovers, tooltip, editor de grupo, diálogos e avisos — mais o contador de abas do popover de destino de grupo.

Continuam em Iosevka: o terminal, badges de perfil, chips de atalho, o campo de senha mascarado e o campo da barra de busca (`search_bar::FIELD_FONT`). O `FontFace::Sans` segue sendo a Iosevka do chrome.

**Peso.** A Segoe UI não tem 500: o `Medium` cairia no 400. O nome do grupo e os títulos passam a `SemiBold` (600), que a Segoe UI tem.

**Centragem vertical.** O chrome centra texto por `origem = centro − size_px / 2`, conta que só acerta para as métricas da Iosevka: o `cosmic-text` assenta a baseline conforme ascent e descent da própria face dentro da linha de `size_px × 1.2`, e a Segoe UI saía 1 a 2px baixa. A correção é uma só, no render: para toda run `System`, o topo é deslocado de `size_px × (0,5 − f)`, com `f` o centro da altura de versal da face, em frações da em, medido da rasterização do `H` (`text_measurer::cap_center_fraction`, uma vez por face e por janela). Nenhuma geometria do chrome mudou.

### 7. Clique e hover só dentro da janela visível da trilha

O `hit_test` da barra trabalha em coordenadas de conteúdo e acha abas roladas para fora de vista **por baixo** da zona fixa da direita: um clique direito num botão de sessões, configurações ou de janela abria o menu dessa aba. `tab_bar::point_in_track(x)` restringe a trilha ao recorte de `trilha_width`, e `handle_bar_click`, o release do "+"/"x" (`bar_button_hit_at`) e o hover da barra o consultam antes do `hit_test`. Fora do recorte o clique direito não faz nada, e o esquerdo nas frestas entre os botões vira arraste da janela.

## Alternativas consideradas

- **Corrigir a centragem em cada ponto de chamada** (um helper por rótulo): rejeitada. São dezenas de pontos nas telas de configurações, nos popovers e na barra de status, e a próxima tela nova reintroduziria o defeito. No render, a face carrega a correção.
- **Embutir uma fonte de interface**: rejeitada. O pedido é a fonte *do sistema*; embutir de novo trocaria uma Iosevka por outra face igualmente fixa.
- **Chave de configuração para a fonte de interface**: adiada. O pedido era um experimento visual, e `[terminal.font]` continua sendo a única família configurável.
- **Separar diálogos e avisos dos popovers** (manter Iosevka neles): rejeitada por custo. `overlay.rs` usa as duas constantes em dezenas de pontos de medida e de pintura, e separá-las sem poder rodar o binário arriscava descasar medida e desenho. Os dois acompanham os popovers.

## Consequências

### Positivas

- A barra de abas fica sem dois botões que cobriam abas e sem pílula sobre cápsula; o colapso mostra quantas abas o grupo esconde.
- O "x" confirma o alvo no hover e no clique, como o "+".
- Texto proporcional na fonte que o usuário vê no resto do sistema.
- O clique direito sob os botões da direita deixa de abrir menu de aba escondida, e o hover deixa de acender o "x" dela.

### Negativas

- A aparência do chrome passa a depender da máquina, exatamente o que o [ADR-0016](0016-fontes-embutidas.md) evitava: largura de texto, onde o título trunca e o peso mudam entre sistemas. O layout trunca por medida real, então nada quebra, mas dois usuários não veem o mesmo.
- Sem o botão de overflow, nada na barra indica abas fora da vista além do corte na ponta; a rolagem fica por conta da roda do mouse.
- Os pontos de um grupo colapsado crescem com o número de abas (10px por aba) e não têm teto.
- Quem tinha as chaves removidas no `porecatu.toml` vê um aviso de chave desconhecida.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| `Helvetica Neue` ou a família padrão do Linux ausentes ou com métricas diferentes | Média | Baixo: o `cosmic-text` cai na cadeia de fallback da plataforma | verificação no macOS e no Linux fica como dívida, no roadmap |
| Centragem medida pelo `H` deixa texto só em minúsculas ~1px baixo | Baixa | Baixo | trocar o `H` por uma média com a altura do `x` em `cap_center_fraction` |
| `rustfmt` deixa `Self { scroll_step: 90 }` numa linha e o `verify-docs.py` não achava o valor | Ocorreu | Baixo | o padrão do script agora aceita `}` depois do valor |

## Revisões em outros ADRs

[ADR-0026](0026-chrome-unificado-em-iosevka-fixed.md) e [ADR-0065](0065-fonte-unica-para-terminal-e-chrome.md) (a fonte do chrome é a do terminal) e [ADR-0016](0016-fontes-embutidas.md) (todas as faces são embutidas) trazem um blockquote de revisão apontando para este.
