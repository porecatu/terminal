# ADR-0001 — Stack de GUI: winit + wgpu + glyphon

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** ADR-0007, PRD-004, PRD-005

## Contexto

O Porecatu precisa desenhar duas coisas muito diferentes na mesma janela:

1. **A grade do terminal** — dezenas de milhares de células, cada uma com glyph, cor de frente, cor de fundo e atributos. Precisa atualizar a 60+ FPS enquanto um `cargo build` cospe saída.
2. **O chrome** — barra de abas, pílulas de grupo, indicadores, títulos. Layout dinâmico, hover, drag & drop.

O requisito 4 do produto (PRD-004) exige que **cores, desenho e dimensões das abas e grupos sejam customizáveis por arquivo de configuração**: altura, padding, raio dos cantos, cores por estado, espessura e posição do indicador de grupo. Isso não é "aplicar um tema" — é controle sobre a geometria do desenho.

Toolkits de UI prontos resolvem o item 2 rápido, mas cobram por isso exatamente onde dói: o desenho fica preso ao modelo de widget e ao sistema de temas do toolkit, e o item 1 exige um caminho de renderização de texto customizado de qualquer forma. Ou seja: adotar um toolkit não elimina o trabalho de render de texto, só adiciona um intermediário entre a config do usuário e os pixels.

Além disso, as três plataformas-alvo (Windows, Linux, macOS) precisam de comportamento consistente, incluindo HiDPI e múltiplos monitores com escalas diferentes.

## Decisão

Renderização própria sobre GPU:

| Peça | Crate | Papel |
|---|---|---|
| Janela e eventos | `winit` | criação de janela, event loop, input, DPI, monitores |
| Render | `wgpu` | abstração sobre Vulkan / Metal / D3D12 |
| Shaping e atlas de texto | `glyphon` + `cosmic-text` | shaping, fallback de fonte, atlas de glyphs em cache |

`porecatu-render` expõe primitivas de desenho (`Quad`, `RoundedQuad`, `TextRun`, clip) e nada de domínio. Cantos arredondados via SDF no fragment shader — nenhuma dimensão ou cor hardcoded. Detalhes em [arquitetura.md](../arquitetura.md).

Escolha de `cosmic-text` em vez de `rusttype`/`fontdue` puros: fallback de fonte e shaping complexo são requisito real (emoji e CJK em prompt de shell são comuns, e PRD-005 prevê cadeia de fallback configurável).

## Alternativas consideradas

### egui (eframe)

UI declarativa, produtiva, `wgpu` por baixo. Descartada porque a customização visual passa pelo sistema de `Style` do egui, que não expõe a superfície que o PRD-004 pede (raio por canto, geometria de indicador de grupo, densidade). E o grid do terminal teria que ser um widget de pintura customizada de todo jeito — então o egui ficaria pagando overhead de layout imediato por frame sem entregar o desenho que precisamos.

### Iced

Arquitetura Elm-like madura, `wgpu` por baixo, cross-platform sólido. Mais próxima de viável que o egui. Descartada por dois motivos: o grid do terminal continua sendo um shader/widget custom (o mesmo trabalho), e o modelo de mensagens do Iced adiciona uma camada de indireção entre o `Wakeup` do PTY e o frame — atrito direto com o render damage-driven de [ADR-0007](0007-modelo-de-threading.md).

### GPUI

O framework do Zed, provadamente capaz de renderizar texto rápido. Descartada por risco de plataforma e de API: o suporte a Windows era imaturo no momento da decisão e a API não tem garantia de estabilidade fora do Zed. Windows é plataforma primária aqui.

### GTK4 / Qt

Integração nativa boa no Linux, temas do sistema de graça. Descartada por peso de dependência no Windows e macOS, por bindings Rust de qualidade desigual, e porque o sistema de tema do toolkit brigaria com a config do usuário em vez de servi-la.

## Consequências

### Positivas

- Controle total do desenho: qualquer coisa que o PRD-004 e o PRD-005 pedirem é implementável sem lutar contra um toolkit.
- Um só caminho de renderização para chrome e terminal — mesmo atlas de fonte, mesmo pipeline, mesma noção de DPI.
- Binário enxuto, sem runtime de toolkit.
- `wgpu` cobre as três plataformas com um backend nativo em cada.

### Negativas

- **Todo o chrome é código nosso**: hit-testing, foco, ordem de tab, drag & drop, menu de contexto, tooltip. Nada vem de graça.
- **Acessibilidade é trabalho explícito.** Leitores de tela não enxergam pixels de GPU. Mitigação: integrar `accesskit` (que já conversa com `winit`) numa fase posterior — registrado no [roadmap](../roadmap.md) como dívida consciente, não esquecimento.
- IME (entrada de CJK, dead keys) precisa ser tratado na mão via os eventos de IME do `winit`.
- Sem widgets prontos: caixa de input para renomear aba/grupo é um mini-editor de texto que temos que escrever.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| `wgpu` quebra API a cada release | Alta | Médio | Pinar versão exata; subir `wgpu` é tarefa própria, nunca efeito colateral |
| Driver GPU ruim / VM sem aceleração | Média | Alto | `wgpu` cai para backend software; detectar e avisar no primeiro start |
| Custo de escrever chrome subestimado | Média | Médio | F2/F3 do roadmap são fases inteiras dedicadas a isso, não sub-tarefas |
| Acessibilidade adiada virar "nunca" | Média | Alto | Item nomeado no roadmap com fase própria (F6) |
