# PRD-000 — Visão de produto

**Status:** Aprovado
**Data:** 2026-08-26

## Problema

Emuladores de terminal modernos resolveram bem os problemas de 2015: renderização acelerada por GPU, conformidade VT, performance. Alacritty, WezTerm, Kitty e Windows Terminal são todos rápidos e corretos.

O que nenhum deles resolve bem é o problema de 2026: **o desenvolvedor tem quinze terminais abertos e não sabe qual é qual.**

Os sintomas são conhecidos por qualquer um que trabalhe em mais de um contexto:

- A barra de abas vira uma fileira de rótulos idênticos — `bash`, `bash`, `zsh`, `node`, `bash`.
- Não há como dizer "estas quatro abas são o projeto A, estas três são o projeto B". A única separação disponível é abrir outra janela, o que troca um problema por outro.
- Fechar a janela, reiniciar a máquina ou o app travar apaga todo o contexto. Reconstruir quinze terminais nos diretórios certos é trabalho manual de vários minutos, feito de memória.
- Multiplexadores (`tmux`, `zellij`) resolvem parte disso, mas ao custo de uma camada inteira: outro conjunto de teclas, outro modelo mental, e uma barra de status que compete com a do emulador.

## Proposta

Um emulador de terminal onde **a organização de terminais é o recurso principal**, não um acessório.

Três apostas:

1. **Grupos de abas nomeados e coloridos**, no modelo mental dos grupos de aba do Chrome — que os usuários já conhecem, e que funciona porque é restritivo.
2. **Sessão que sobrevive ao fechamento.** Reabrir o app devolve a estrutura de trabalho: mesmas abas, mesmos grupos, mesmos diretórios.
3. **Aparência que o usuário controla de verdade**, do desenho da aba à paleta do terminal, por arquivo de configuração legível.

## Usuário-alvo

O desenvolvedor que trabalha em vários contextos simultâneos: mais de um repositório, ou frontend e backend e infra, ou vários clientes. Confortável em editar um arquivo TOML. Usa terminal o dia inteiro e sente o atrito de perder contexto.

**Não é para:** quem usa um terminal por vez (qualquer emulador serve), nem quem já vive dentro de `tmux` com uma configuração madura (o Porecatu não tenta substituí-lo).

## Posicionamento

| Ferramenta | Força | Lacuna que o Porecatu ataca |
|---|---|---|
| Alacritty | Rápido, minimalista | Sem abas; organização é problema do usuário |
| WezTerm | Completíssimo, abas e panes, config em Lua | Sem grupos nomeados; sessão exige plugin/script |
| Kitty | Rápido, extensível | Unix-only; modelo de layout próprio, curva de aprendizado |
| Windows Terminal | Abas boas, integração Windows | Windows-only; sem grupos; sem restauração de diretório |
| tmux / zellij | Persistência e organização reais | Camada extra: teclas próprias, modelo próprio, atrito de aprendizado |

O Porecatu não compete em conformidade VT (usa o motor do Alacritty — [ADR-0002](../adr/0002-motor-vte.md)) nem em contagem de features. Compete em **uma coisa: gestão de muitos terminais.**

## Princípios

1. **Organização é o produto.** Quando houver conflito entre simplicidade e organização, a organização ganha.
2. **Sem camada extra.** Grupos e sessão são do emulador. O usuário não aprende um segundo modelo mental.
3. **Config é interface.** Aparência é controlada por arquivo legível, com defaults sensatos e sem exigir recompilação.
4. **Terminal ocioso custa zero.** Nenhum frame renderizado sem mudança ([ADR-0007](../adr/0007-modelo-de-threading.md)).
5. **Nunca perder trabalho.** Config inválida não derruba o app; crash não corrompe a sessão.
6. **Cross-platform de verdade.** Windows, Linux e macOS desde a primeira fase, não como porte posterior.

## Escopo do v1

| # | Recurso | PRD |
|---|---|---|
| 1 | Abas para múltiplos terminais na mesma janela | [PRD-001](prd-001-abas.md) |
| 2 | Agrupamento de abas com nome e cor | [PRD-002](prd-002-grupos-de-abas.md) |
| 3 | Persistência e restauração de sessão | [PRD-003](prd-003-persistencia-de-sessao.md) |
| 4 | Aparência configurável do chrome | [PRD-004](prd-004-aparencia-do-chrome.md) |
| 5 | Cores e fontes do terminal configuráveis | [PRD-005](prd-005-aparencia-do-terminal.md) |

O alvo visual desses cinco recursos está desenhado em [`docs/design/`](../design/README.md) — comece pelo [mockup estático](../design/mockup-estatico.html) e pela [especificação visual](../design/especificacao-visual.md).

## Não-objetivos do v1

Cada um destes é uma decisão, não um esquecimento.

Vários deles **já estão desenhados** no design canvas ([`docs/design/`](../design/README.md)). Isso não os traz para o v1: o desenho existe como norte de longo prazo, com cada elemento etiquetado `[v1]` ou `[v2]` na tabela de fases da [especificação visual](../design/especificacao-visual.md). Ver [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md).

- **Splits / panes dentro da aba.** Grupos e panes resolvem necessidades parecidas; fazer os dois ao mesmo tempo dilui ambos. Se entrar, muda o modelo de `Tab` ([ADR-0006](../adr/0006-modelo-de-abas-e-grupos.md)) e merece ADR próprio. — *desenhado, `[v2]`, [PRD-006](prd-006-paineis-divididos.md) (rascunho)*
- **Perfis de aba** (aba que abre WSL, aba que abre SSH). Recurso natural para v2; não é do núcleo. — *desenhado, `[v2]`, [PRD-007](prd-007-perfis-de-aba.md) (rascunho)*
- **Paleta de comandos.** Busca unificada de abas, grupos e ações. — *desenhado, `[v2]`, [PRD-008](prd-008-paleta-de-comandos.md) (rascunho)*
- **Barra de status.** — *desenhado, `[v2]`, [PRD-009](prd-009-barra-de-status.md) (rascunho)*
- **Configuração por interface gráfica.** No v1, a config é o arquivo TOML ([ADR-0003](../adr/0003-formato-de-configuracao.md)). — *desenhado, `[v2]`; quando existir, escreverá no TOML ([ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md))*
- **Faixa de identidade da barra de título** (logo, nome do app, título da aba ativa). Decorações nativas continuam só no macOS; Windows/Linux já têm controles de janela e resize próprios ([ADR-0027](../adr/0027-controles-de-janela-e-resize-proprios.md)). — *desenhado, `[v2]` ([ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md))*
- **Multiplexação remota** (anexar a sessão em servidor, estilo `tmux -CC`). Problema grande e ortogonal.
- **Sistema de plugins.** Sem lógica programável na config no v1 ([ADR-0003](../adr/0003-formato-de-configuracao.md)).
- **Protocolo de imagens** (sixel, kitty graphics). Fora do que o motor escolhido oferece.
- **Mover abas entre janelas** por drag. O modelo permite; a UI fica para depois.
- **Persistir scrollback.** Só estrutura e diretórios ([PRD-003](prd-003-persistencia-de-sessao.md)).

## Métricas de sucesso

Do v1, medidas em uso real:

| Métrica | Alvo |
|---|---|
| Tempo até o primeiro prompt utilizável | < 300 ms em máquina modesta |
| Uso de CPU com todas as abas ociosas | ~0% |
| Restauração de sessão com 20 abas | < 1 s até a janela interativa |
| Latência de tecla até pixel | < 16 ms (um intervalo de frame) |
| Reconstrução manual de contexto após reabrir | zero ações do usuário |

A última é a que define o produto. Se o usuário ainda precisa reorganizar abas depois de reabrir, o v1 falhou no que se propôs.

## Riscos de produto

| Risco | Mitigação |
|---|---|
| Restauração de diretório depender de integração de shell, sobretudo no Windows | Detecção da ausência + convite com snippet pronto; limitação documentada ([ADR-0005](../adr/0005-persistencia-de-sessao.md)) |
| Grupos serem percebidos como complexidade desnecessária | Grupos são opcionais; sem nenhum grupo, o app é um emulador com abas comum |
| Escopo de escrever todo o chrome na mão estourar | Fases dedicadas no [roadmap](../roadmap.md); F1 entrega valor sozinha |
| Usuário de `tmux` não ver motivo para trocar | Não é o público-alvo; o Porecatu funciona com `tmux` dentro dele |
