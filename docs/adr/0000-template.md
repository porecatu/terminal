# ADR-0000 — Template e índice

## Índice de decisões

| # | Título | Status |
|---|--------|--------|
| [0001](0001-stack-de-gui.md) | Stack de GUI: winit + wgpu + glyphon | Aceito |
| [0002](0002-motor-vte.md) | Motor VT: crate alacritty_terminal | Aceito |
| [0003](0003-formato-de-configuracao.md) | Configuração em TOML | Aceito |
| [0004](0004-pty-cross-platform.md) | PTY cross-platform via portable-pty | Aceito |
| [0005](0005-persistencia-de-sessao.md) | Persistência de sessão em JSON versionado | Aceito |
| [0006](0006-modelo-de-abas-e-grupos.md) | Modelo de abas e grupos | Aceito |
| [0007](0007-modelo-de-threading.md) | Modelo de threading e render damage-driven | Aceito |
| [0008](0008-teclas-e-roteamento-de-input.md) | Keybindings e roteamento de input | Aceito |
| [0009](0009-referencia-visual-e-reconciliacao.md) | Referência visual e reconciliação com o design canvas | Aceito |
| [0010](0010-licenciamento.md) | Licenciamento sob GPL-3.0-or-later | Aceito |
| [0011](0011-toolchain-rust.md) | Toolchain Rust pinada e política de versão | Aceito |
| [0012](0012-identificacao-do-terminal.md) | Identificação do terminal: `TERM` e capacidades anunciadas | Aceito |
| [0013](0013-mouse-selecao-e-clipboard.md) | Mouse, seleção e clipboard | Aceito |
| [0014](0014-superficie-de-aviso-e-dialogo.md) | Superfície de aviso, diálogo e menu de contexto | Aceito |
| [0015](0015-multiplas-janelas.md) | Múltiplas janelas no v1, em escopo mínimo | Aceito |
| [0016](0016-fontes-embutidas.md) | Fontes do design embutidas no binário | Aceito |

## Convenção

- Numeração sequencial de quatro dígitos, nunca reaproveitada.
- Nome do arquivo: `NNNN-titulo-em-kebab-case.md`.
- Status possíveis: `Proposto`, `Aceito`, `Rejeitado`, `Superseded by ADR-NNNN`.
- Decisão aceita não se edita. Para mudar, escreva um ADR novo com `Supersedes: ADR-NNNN` e marque o antigo. Correção de erro factual ou de clareza no texto é permitida; mudança de decisão, não.
- Ao adicionar um ADR, atualize a tabela acima e, se a decisão aparecer lá, a tabela de stack do [README](../../README.md).

---

## Template

```markdown
# ADR-NNNN — Título

**Status:** Proposto | Aceito | Rejeitado | Superseded by ADR-MMMM
**Data:** AAAA-MM-DD
**Relacionados:** ADR-XXXX, PRD-YYY

## Contexto

Qual é a força em jogo. O que torna esta decisão necessária agora, e o que
acontece se ela não for tomada. Requisitos e restrições concretos, não
generalidades.

## Decisão

O que foi decidido, em voz ativa e no presente. Uma frase, se possível.
Depois, os detalhes que a tornam acionável.

## Alternativas consideradas

Para cada alternativa séria: o que era, por que era plausível, e o motivo
específico de não ter sido escolhida. Alternativa sem motivo de rejeição
não é alternativa considerada, é enfeite.

## Consequências

### Positivas
### Negativas
### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
```
