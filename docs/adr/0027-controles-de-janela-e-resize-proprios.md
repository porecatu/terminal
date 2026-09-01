# ADR-0027 — Controles de janela e resize próprios (Windows/Linux)

**Status:** Aceito
**Data:** 2026-09-01
**Supersedes:** ADR-0009 (parcial: a linha "Barra de título customizada" da tabela de escopo faseado, seção "2. Escopo faseado, roadmap intacto", e a linha equivalente da divergência "Design tem barra de título própria; default é `decorations = true`" na seção 4.4 da especificação visual)
**Relacionados:** ADR-0009, ADR-0015, ADR-0018, ADR-0021, ADR-0024, PRD-004

## Contexto

O [ADR-0009](0009-referencia-visual-e-reconciliacao.md) classificou "barra de título customizada" como `[v2]`, porque contrariava o default `decorations = true` do v1 — ninguém tinha decidido tirar a decoração nativa ainda, e o mockup desenha uma faixa própria de 36px (logo, nome do app, travessão, título da aba ativa, três botões de 44px) empilhada acima da barra de abas de 52px.

Pedido do usuário: remover a decoração nativa do Windows/Linux agora, sem esperar o v2. Motivo — a decoração nativa é a última superfície visível da janela que escapa da paleta do design: borda e barra de título do SO não seguem os tokens do chrome, e a barra de abas fica encaixada sob uma faixa que não é nossa. O ADR-0009 nunca vinculou "sair da decoração nativa" a "construir a faixa de 36px inteira" — são duas decisões que o mockup só desenhou juntas.

## Decisão

**Windows e Linux perdem a decoração nativa (`decorations = false`); macOS mantém a nativa.** O que a decoração nativa fazia passa a ser resolvido pela própria barra de abas de 52px, sem empilhar uma segunda barra:

- **Drag region** — a área vazia da barra (fora de aba, pílula, "+", botão de configurações e botões de janela) arrasta a janela, convenção Firefox (`WindowState::resolve_titlebar_drag`). Duplo clique nessa área maximiza/restaura, resolvido no *press*: `Window::drag_window()` entrega o gesto ao loop modal do SO sem garantir o `MouseInput::Released` de volta, então "foi duplo clique?" precisa ser decidido antes de chamar `drag_window`, não depois.
- **Botões de janela** — três, 46px de largura cada, altura cheia da barra, colados na borda direita: minimizar, maximizar/restaurar, fechar (`tab_bar::window_button_rect`/`WINDOW_BUTTON_WIDTH`). Cores reaproveitadas da seção 2.1 da especificação visual (hover `#252a33`; fechar vira `#c4413f` com ícone `#ffffff`) — é o único trecho da faixa `[v2]` com token de cor já aprovado, e nenhuma cor nova entrou. Ícones Lucide `minus`/`square`/`copy` (minimizar/maximizar/restaurar) somam-se ao catálogo de `porecatu-render::icon`; fechar reusa `x`.
- **Resize por borda** — 6px em toda borda da janela, não só na faixa da barra (`titlebar::resize_direction_at`, novo módulo, puro e testável sem `Window` real). Desliga sozinho com a janela maximizada. Sem token de design: a especificação nunca cobriu resize sem decoração nativa.
- **Fechar continua passando pelo diálogo de confirmação** de múltiplas abas: sobe como `NewTabRequest::CloseWindowRequested` até `App::request_close_window`, mesmo caminho de `WindowEmptied` — `WindowState` não pode abrir diálogo sozinho.
- **macOS mantém decoração nativa** (semáforo/traffic lights) via `with_titlebar_transparent` + `with_title_hidden` + `with_fullsize_content_view`: a barra de abas se estende por baixo da titlebar nativa, que fica invisível exceto o semáforo. A trilha reserva `MACOS_TRAFFIC_LIGHT_INSET` (78px) à esquerda para não desenhar sob ele (`tab_bar::left_inset`) — valor de partida, não medido contra `NSWindow` real.

O que **não muda**: a faixa de 36px do mockup (logo "Porecatu", travessão, título da aba ativa) continua fora de escopo. Ela é identidade de app, não controle de janela, e nada no pedido do usuário exigia reconstruí-la. Continua `[v2]` na especificação visual (seção 2.1) — só que agora com os botões e o resize já fora dela, vivendo na barra de abas `[v1]` (seção 2.2.1).

## Alternativas consideradas

### Construir a faixa de 36px completa do mockup, empilhada sobre a barra de abas

Era o desenho que o ADR-0009 tinha aprovado. Rejeitada: dobraria o orçamento vertical de chrome fixo (36+52 = 88px, contra os 52px que todo `bar_height()` e todo teste de layout já assumem) só para ganhar logo, nome do app e título da aba ativa — nenhum dos três resolve o pedido real (remover decoração nativa e controlar a janela). Fica registrada como próximo passo caso o produto queira identidade de app na titlebar; não é este ADR.

### Semáforo próprio também no macOS, decoração nativa fora ali também

Rejeitada: `winit` não expõe reconstrução do semáforo com o comportamento nativo (clique-direito com Option, hover por item, animação de zoom em tela cheia), e reimplementar isso é superfície grande para um resultado que ninguém pediu. macOS já tinha o padrão correto — decoração nativa, cliente reservando espaço — antes deste ADR.

### Decoração nativa fora só no Windows, mantida no Linux

Cogitada porque Linux/Wayland é o ambiente sem verificação interativa do projeto (clipboard no Wayland já é uma pendência conhecida). Rejeitada: X11 e a maioria dos compositores Wayland suportam `decorations=false` com resize/drag client-side do mesmo jeito que o Windows; abrir um terceiro ramo de plataforma por uma lacuna de *verificação*, não de *suporte*, teria custo permanente por um risco temporário. O risco fica registrado abaixo, sem excluir Linux do escopo.

## Consequências

### Positivas

- Fora do macOS, a janela inteira — inclusive a borda — segue a paleta do design; fecha a última superfície visível que escapava do chrome.
- `is_macos()` centraliza a única bifurcação de plataforma nova, mesmo padrão de `WindowState::is_secondary_bar_click` — nenhum `#[cfg(target_os)]` espalhado fora de `open_window`.
- `titlebar::resize_direction_at` é puro e testável sem `Window` real, mesma cultura de teste do resto do crate.

### Negativas

- A janela perde a faixa de identidade do mockup (logo, nome do app, título da aba ativa). No Windows/Linux o título ainda existe (`with_title("Porecatu")`), mas só aparece na barra de tarefas/alternador de janelas, não em nenhum pixel da própria janela.
- Duas geometrias de "botão de janela" passam a conviver na documentação: os 44px/altura-36 do mockup (§2.1, nunca desenhados) e os 46px/altura-52 (`WINDOW_BUTTON_WIDTH`) que este ADR desenha de verdade — mais uma linha na tabela de divergências da especificação visual (§4.4).
- `MACOS_TRAFFIC_LIGHT_INSET` (78px) é palpite, não medição contra `NSWindow` real — nenhum ambiente macOS rodou este código ainda.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Compositor Wayland específico não suportar bem resize/drag client-side | Média | Médio | Mesma pendência de verificação interativa em Linux/Wayland já registrada nas Armadilhas conhecidas do CLAUDE.md; revisar quando houver ambiente para testar |
| `MACOS_TRAFFIC_LIGHT_INSET` sair errado na régua real do semáforo do macOS | Média | Baixo | Constante isolada em `tab_bar::left_inset`; ajuste é uma linha quando alguém rodar em Mac de verdade |
| Confundir a ausência da faixa de 36px com feature incompleta | Baixa | Baixo | Este ADR e a seção 4.4 da especificação visual documentam que a faixa é decisão consciente, não pendência |
