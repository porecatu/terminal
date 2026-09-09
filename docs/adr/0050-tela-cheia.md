# ADR-0050 — Tela cheia por `F11`: janela `Borderless`, sem estado novo em `Config`

**Status:** Aceito
**Data:** 2026-09-09
**Relacionados:** ADR-0008, ADR-0027, ADR-0029

## Contexto

Pedido direto do usuário, fora da ordem de fases — o mesmo padrão do [ADR-0027](0027-controles-de-janela-e-resize-proprios.md) e da [ADR-0048](0048-barra-de-status.md): recurso que o v1 não previa, sem RF de PRD algum atrás dele. O catálogo de ações (`docs/reference/acoes.md`) é fechado por regra própria — *"ação nova exige requisito ou decisão nova primeiro"* — e este ADR é essa decisão para `window.toggle_fullscreen`.

`F11` é a tecla convencional de tela cheia no Windows e no Linux (todo browser e a maioria dos terminais a usam); não há convenção equivalente no macOS, onde o próprio botão verde do semáforo nativo já cobre o mesmo efeito.

## Decisão

**`window.toggle_fullscreen` alterna a janela ativa entre normal e tela cheia via `winit::window::Fullscreen::Borderless(None)`** — o monitor atual, sem trocar modo de vídeo. Sem argumento, sem estado persistido: a janela volta a normal ao reabrir o app, como qualquer outra janela do v1 antes da restauração de geometria da sessão decidir o contrário.

Default de tecla: **`F11`, na tabela comum** (`common_defaults`), valendo nas três plataformas pelo mesmo motivo de `f3`/`shift+f3` (ADR-0041 §10) — é convenção estabelecida, não um atalho que compita com outro já em uso. Sem entrada própria em `macos_defaults`: a tabela comum já cobre o Mac, e não há `Cmd+F11` de convenção para substituí-la.

### Por que `Borderless`, não `Exclusive`

`Fullscreen::Exclusive` troca o modo de vídeo do monitor — a técnica de jogo, pensada para minimizar latência de apresentação. Nada no Porecatu precisa disso, e o custo (flicker de troca de modo, risco de não restaurar em crash) é desproporcional ao ganho para um emulador de terminal. `Borderless` é o que todo app de produtividade usa: a janela ocupa a tela inteira, sem troca de modo.

### Desmaximizar antes de pedir fullscreen

No Windows, `set_fullscreen` numa janela **maximizada** não produz efeito visível: o estilo `WS_MAXIMIZE` sobrevive ao pedido, o `SetWindowPos` que o `winit` faz por baixo perde a briga contra ele, e a janela não muda de tamanho — mas o aviso de "estou em tela cheia" que o `winit` já mandou pro shell fica valendo, deixando a taskbar num estado visual quebrado sem nunca se esconder de verdade. A implementação desmaximiza primeiro (`set_maximized(false)`) sempre que a janela estiver maximizada, e guarda esse fato (`fullscreen_restore_maximized`) pra remaximizar ao sair — não dá pra confiar no `SavedWindow` que o próprio `winit` guarda, porque ele captura a geometria **depois** de já termos desmaximizado.

### O que não muda

- **A barra de abas e os controles de janela próprios (ADR-0027) continuam desenhados como sempre.** Fora do macOS, minimizar/maximizar-restaurar/fechar seguem na barra; em tela cheia eles só deixam de ter função visível de moldura do SO por baixo, porque não há moldura do SO em nenhum estado da janela deste projeto.
- **Nenhum valor de aparência novo.** Tela cheia é geometria de janela, não um token da especificação visual.
- **Nenhum campo novo em `Config` ou na sessão persistida.** O estado não sobrevive a fechar e reabrir — se isso vier a ser pedido, é ADR próprio, não extensão silenciosa deste.

## Alternativas consideradas

### Esconder a barra de abas em tela cheia (estilo "imersivo")

Rejeitado: ninguém pediu, e a barra é onde vivem abas, grupos e os próprios controles de janela — escondê-la tornaria a tela cheia um beco sem saída visual (só `F11` de novo, às cegas, resolveria).

### Persistir o estado na sessão (RF-3.\*)

Rejeitado por escopo: a sessão (ADR-0036) já tem um envelope de janela fechado; entrelaçar tela cheia nele é decisão própria, não o efeito colateral de um atalho de teclado.

## Consequências

### Positivas

- Atalho que todo usuário de Windows/Linux já espera, sem depender de decoração nativa (que o projeto não tem fora do macOS).
- Implementação inteira em `WindowState` (mesmo lugar que já resolve minimizar/maximizar): nenhuma dependência nova, nenhum estado por processo.

### Negativas

- Mais uma linha no catálogo fechado, mais uma entrada de teste de round-trip em `porecatu-core`.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| `F11` colidir com o que o programa hospedado (ex. `vim`) espera receber | Baixa | Baixo | `F11` não tem uso convencional em terminal; `none` no `[keybindings]` do usuário devolve a tecla a ele, como qualquer outro binding |
| Usuário preso em tela cheia sem saber a tecla de saída | Baixa | Baixo | Mesma tecla alterna nos dois sentidos (toggle), igual a todo app do gênero |
