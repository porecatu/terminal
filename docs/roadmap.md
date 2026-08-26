# Roadmap

Fases de entrega do v1. Cada fase tem **critério de saída explícito** — enquanto ele não for atendido, a fase não terminou e a próxima não começa.

A ordem não é arbitrária: cada fase entrega algo utilizável e é pré-requisito real da seguinte. Em particular, F1 entrega um emulador de terminal funcional; tudo depois disso é o diferencial do produto ([PRD-000](prd/prd-000-visao-de-produto.md)).

O alvo visual está em [`docs/design/`](design/README.md). O mockup mostra o produto **completo**, não o v1 — a [tabela de fases](design/especificacao-visual.md) diz o que pertence a cada etapa. Nenhuma fase abaixo implementa elemento `[v2]`.

---

## F0 — Esqueleto

Colocar a infraestrutura de pé antes de escrever qualquer lógica.

- Workspace Cargo com os oito crates de [arquitetura.md](arquitetura.md)
- Grafo de dependências entre crates conforme a tabela de [CLAUDE.md](../CLAUDE.md), com os `Cargo.toml` já refletindo as restrições
- Janela `winit` + surface `wgpu` limpando a tela, respondendo a resize e a mudança de DPI
- Versões de `wgpu` e `alacritty_terminal` pinadas com igualdade exata ([ADR-0001](adr/0001-stack-de-gui.md), [ADR-0002](adr/0002-motor-vte.md))
- `rust-toolchain.toml` com a stable do dia, MSRV igual, edition 2024 e lints em `[workspace.lints]` ([ADR-0011](adr/0011-toolchain-rust.md))
- Cabeçalho `// SPDX-License-Identifier: GPL-3.0-or-later` em todo arquivo ([ADR-0010](adr/0010-licenciamento.md))

**O CI já existe.** `.github/workflows/ci.yml` traz a matriz das três plataformas com `fmt`, `clippy -D warnings`, `build` e `test`, além das dependências de sistema do Linux. Ele está dormindo: o job `detect` pula a matriz enquanto não houver `Cargo.toml` na raiz. **Criar o workspace liga o CI sozinho** — não editar o workflow.

Três pendências pontuais nesta fase:

- Remover a checagem `verificar_fase_documentacao()` de `scripts/verify-docs.py` — ela falha de propósito quando aparece código Rust, e o commit que cria o `Cargo.toml` é o que deve removê-la
- Conferir se as dependências de sistema listadas no workflow bastam para o `winit` e o `wgpu` de verdade
- Trocar `dtolnay/rust-toolchain@stable` pela versão pinada e ligar o job canário ([ADR-0011](adr/0011-toolchain-rust.md)). Instalar `stable` com um `rust-toolchain.toml` apontando para outra versão faz o CI baixar duas toolchains por job e cachear a errada

> **A ordem de escolha das versões não é livre.** Nenhum ADR fixa números — eles são escolhidos aqui, olhando o crates.io do dia — mas quatro crates da stack são acoplados:
>
> ```
> glyphon → wgpu → raw-window-handle → winit
> ```
>
> Escolher os quatro de forma independente produz erro de tipo em `raw-window-handle` que não se parece com o que é, e é o jeito mais fácil de perder um dia na F0. A ordem que funciona: **fixar o `glyphon` primeiro**, aceitar o `wgpu` que ele exige, e só então escolher o `winit` cuja `raw-window-handle` casa com a do `wgpu`. `alacritty_terminal` e `portable-pty` são independentes dessa cadeia.
>
> O registro das escolhas é o `Cargo.lock`, que é versionado de propósito ([.gitignore](../.gitignore)). Considerar `--locked` nos comandos do CI para que a build seja reproduzível de fato.

**Critério de saída:** CI verde nas três plataformas, com a matriz Rust efetivamente rodando (não pulada); janela abre, redimensiona e fecha em Windows, Linux e macOS.

---

## F1 — Terminal único

O maior salto de risco técnico. Ao fim desta fase o projeto é um emulador de terminal.

- `porecatu-pty`: spawn, read, write, resize, encerramento ([ADR-0004](adr/0004-pty-cross-platform.md))
- `porecatu-term`: `alacritty_terminal` encapsulado, snapshot de grid
- Thread de leitura por terminal, `Wakeup` via `EventLoopProxy`, render damage-driven ([ADR-0007](adr/0007-modelo-de-threading.md))
- `porecatu-render`: pipeline de quads e de texto com `glyphon`, atlas em cache
- Fontes do design embutidas em `assets/fonts/`, com o texto da OFL e a atribuição ([ADR-0016](adr/0016-fontes-embutidas.md)) — sem subsetting
- Snapshot de grade conforme a [seção 4 da arquitetura](arquitetura.md): tipos próprios, buffer reusado, cor não resolvida, `wide_spacer` para largura dupla
- Roteamento de input, codificação de teclas, bracketed paste ([ADR-0008](adr/0008-teclas-e-roteamento-de-input.md))
- Ambiente do shell: `TERM=xterm-256color`, `COLORTERM`, `TERM_PROGRAM` ([ADR-0012](adr/0012-identificacao-do-terminal.md))
- Reporte de mouse ao programa (modos 1000/1002/1003, encoding SGR 1006), com `Shift` forçando seleção local ([ADR-0013](adr/0013-mouse-selecao-e-clipboard.md))
- Seleção com mouse pelos quatro modos do motor, cópia com recorte de espaço e remontagem de `WRAPLINE` ([ADR-0013](adr/0013-mouse-selecao-e-clipboard.md))
- Clipboard via `arboard`; OSC 52 com escrita permitida e leitura negada ([ADR-0013](adr/0013-mouse-selecao-e-clipboard.md))
- Rolagem do scrollback por teclado e por roda, com tela alternativa tratada ([ADR-0013](adr/0013-mouse-selecao-e-clipboard.md))
- Forçar UTF-8 no spawn do ConPTY
- `Wakeup` carregando `(WindowId, TabId)` desde já — o tipo atravessa a fronteira de três crates, e corrigir depois é mexer no caminho quente ([ADR-0015](adr/0015-multiplas-janelas.md))

**Critério de saída:** `vim`, `htop` e `fzf` usáveis sem artefatos nas três plataformas; **mouse funciona dentro do `htop` e do `fzf`, e `Shift`+arraste seleciona texto mesmo com o programa pedindo o mouse**; roda do mouse rola `less` e `man`; copiar e colar funciona nas três plataformas, incluindo Wayland; resize funciona com TUI aberta; acentuação por tecla morta em teclado ABNT2 funciona; CPU em ~0% com o terminal ocioso; verificação de que a última linha de saída não se perde ao encerrar o shell.

---

## F2 — Abas

Implementa [PRD-001](prd/prd-001-abas.md).

- `porecatu-core`: `Workspace`, `Tab`, IDs estáveis, operações puras com testes de invariante ([ADR-0006](adr/0006-modelo-de-abas-e-grupos.md))
- Barra de abas: layout como função pura, hit-testing, hover, foco
- Ciclo de vida, título com precedência (customizado > OSC > processo > shell), renomeação inline
- Navegação sequencial e por índice; reordenação por drag e por teclado
- Overflow: encolhimento até o mínimo, depois rolagem
- Indicadores de atividade e campainha
- Menu de contexto da aba, diálogo de confirmação e superfície de aviso do app ([ADR-0014](adr/0014-superficie-de-aviso-e-dialogo.md)) — RF-1.6 é cenário de aceite desta fase e depende do diálogo
- Janelas múltiplas em escopo mínimo: `window.new`, `window.close`, um `Workspace` e uma surface por janela ([ADR-0015](adr/0015-multiplas-janelas.md))

**Critério de saída:** todos os cenários de aceite de PRD-001 passam; **barra de abas confere com [`mockup-estatico.html`](design/mockup-estatico.html)** nos elementos `[v1]` — dimensões, raios, cores por estado, rename inline; layout da barra testado sem abrir janela; 50 abas abertas sem degradação perceptível; IME e teclas mortas continuam funcionando com a barra presente; duas janelas abertas em monitores de DPI diferente desenham com métricas corretas, e saída numa delas não redesenha a outra.

---

## F3 — Grupos

Implementa [PRD-002](prd/prd-002-grupos-de-abas.md). É o diferencial do produto.

- `Group` em `porecatu-core`, grupo implícito, invariantes de contiguidade
- Seleção múltipla de abas
- Criar, dissolver, renomear, recolorir; atribuição automática de cor sem repetir
- Colapso, com efeito sobre navegação e foco
- Drag de aba entre grupos e drag de grupo inteiro
- Menu de contexto do grupo, incluindo fechar todas com confirmação
- Animação da reordenação ao formar grupo

**Critério de saída:** todos os cenários de aceite de PRD-002 passam; **pílula, tingimento do wrapper, sublinhado da aba e editor de grupo conferem com o mockup**; 10 grupos numa janela sem quebra de layout; processos seguem vivos em grupo colapsado.

---

## F4 — Configuração

Implementa [PRD-004](prd/prd-004-aparencia-do-chrome.md) e [PRD-005](prd/prd-005-aparencia-do-terminal.md).

- `porecatu-config`: structs `serde`, defaults completos, resolução de caminho, `PORECATU_CONFIG`, `--config`
- Hot reload com `notify`, parse fora da main thread, debounce
- Erro com linha e chave; chave desconhecida como aviso; config inválida preserva a anterior
- Parser de `[keybindings]` contra o [catálogo fechado de ações](reference/acoes.md): ação desconhecida é erro com sugestão do nome mais próximo, binding duplicado cita as duas linhas ([ADR-0008](adr/0008-teclas-e-roteamento-de-input.md))
- Toda a superfície de [porecatu.example.toml](config/porecatu.example.toml) ligada de fato ao desenho, com os valores default vindos da [tabela de tokens](design/especificacao-visual.md)
- Fallback de fonte, temas nomeados com override, zoom por atalho
- Recálculo de grade e resize de todos os PTYs ao mudar métricas de fonte

**Critério de saída:** zero valores de aparência hardcoded (verificação por revisão dirigida); todos os cenários de aceite de PRD-004 e PRD-005 passam; **o binário com a config padrão bate com o mockup** — divergência visível é bug, não configuração ([ADR-0009](adr/0009-referencia-visual-e-reconciliacao.md)); auditoria de rastreabilidade — nenhuma chave do exemplo sem requisito, nenhum requisito sem chave.

---

## F5 — Sessão

Implementa [PRD-003](prd/prd-003-persistencia-de-sessao.md).

- `porecatu-session`: schema versionado, escrita atômica, debounce, save síncrono no exit
- Restauração preguiçosa, geometria de janela, recuperação de monitor ausente
- Captura de OSC 7; fallbacks `/proc` no Linux e `libproc` no macOS
- Detecção de ausência de OSC 7 e convite à integração de shell, com snippets por shell
- Recuperação: arquivo ausente, corrompido, schema antigo, schema mais novo

**Critério de saída:** todos os cenários de aceite de PRD-003 passam; restauração de 20 abas em menos de 1 s; teste de crash durante a gravação preservando a sessão anterior; limitação do Windows sem OSC 7 verificada e documentada como comportamento esperado, não como bug.

---

## F6 — Polimento

O que separa "funciona" de "usável o dia inteiro".

- Busca no scrollback, com as ações `search.*` do [catálogo](reference/acoes.md)
- Hyperlinks OSC 8, com clique ([ADR-0012](adr/0012-identificacao-do-terminal.md))
- **Acessibilidade via `accesskit`** — dívida assumida em [ADR-0001](adr/0001-stack-de-gui.md), não esquecimento. Precisa cobrir também os três widgets do [ADR-0014](adr/0014-superficie-de-aviso-e-dialogo.md): diálogo modal, menu e notificação são justamente os papéis que leitor de tela trata de forma especial
- Menu de contexto do terminal, com `selection.select_all`
- Notificação de desktop opcional na campainha
- Ícone e empacotamento por plataforma
- Documentação de usuário e página de release

**Critério de saída:** leitor de tela consegue navegar a barra de abas; métricas de [PRD-000](prd/prd-000-visao-de-produto.md) medidas e atingidas; artefatos de release para as três plataformas.

---

## Fora do v1

Registrado para não ser reinventado como ideia nova. Cada item está justificado nos PRDs correspondentes.

A coluna **Desenhado** marca o que já tem alvo visual aprovado no canvas. Estar desenhado não traz nada para o v1 — só significa que, quando entrar, a aparência já está decidida ([ADR-0009](adr/0009-referencia-visual-e-reconciliacao.md)).

| Item | Onde está justificado | Desenhado |
|---|---|---|
| Splits / panes na aba | [PRD-000](prd/prd-000-visao-de-produto.md), [ADR-0006](adr/0006-modelo-de-abas-e-grupos.md), [PRD-006](prd/prd-006-paineis-divididos.md) *(rascunho)* | sim, `[v2]` |
| Perfis de aba (WSL, SSH, container) | [PRD-000](prd/prd-000-visao-de-produto.md), [PRD-007](prd/prd-007-perfis-de-aba.md) *(rascunho)* | sim, `[v2]` |
| Paleta de comandos | [PRD-008](prd/prd-008-paleta-de-comandos.md) *(rascunho)* | sim, `[v2]` |
| Barra de status | [PRD-009](prd/prd-009-barra-de-status.md) *(rascunho)* | sim, `[v2]` |
| Configuração por GUI | [ADR-0003](adr/0003-formato-de-configuracao.md), [ADR-0009](adr/0009-referencia-visual-e-reconciliacao.md) | sim, `[v2]` |
| Barra de título customizada | [ADR-0009](adr/0009-referencia-visual-e-reconciliacao.md), [PRD-004](prd/prd-004-aparencia-do-chrome.md) | sim, `[v2]` |
| Multiplexação remota | [PRD-000](prd/prd-000-visao-de-produto.md) | não |
| Plugins e config programável | [ADR-0003](adr/0003-formato-de-configuracao.md) | não |
| Persistir scrollback | [PRD-003](prd/prd-003-persistencia-de-sessao.md) | não |
| Mover aba entre janelas | [PRD-001](prd/prd-001-abas.md) | não |
| Temas como arquivo importável | [PRD-004](prd/prd-004-aparencia-do-chrome.md), [PRD-005](prd/prd-005-aparencia-do-terminal.md) | não |
| Tema claro/escuro seguindo o sistema | [PRD-004](prd/prd-004-aparencia-do-chrome.md) | não |
| Agrupamento automático por projeto | [PRD-002](prd/prd-002-grupos-de-abas.md) | não |
| Protocolos de imagem (sixel) | [ADR-0002](adr/0002-motor-vte.md) | não |
