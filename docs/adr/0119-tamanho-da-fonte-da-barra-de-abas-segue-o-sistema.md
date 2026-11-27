# ADR-0119 — Tamanho da fonte da barra de abas segue o sistema

**Status:** Proposto — implementado; verificado só por teste automatizado (aval visual do dono pendente)
**Data:** 2026-11-27
**Relacionados:** [ADR-0026](0026-chrome-unificado-em-iosevka-fixed.md), [ADR-0065](0065-fonte-unica-para-terminal-e-chrome.md) (**revisto**), [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md)
**Supersedes:** ADR-0065 (parcial: o tamanho do texto da barra de abas)

## Contexto

O [ADR-0065](0065-fonte-unica-para-terminal-e-chrome.md) fez o rótulo da aba, o nome do grupo e o campo de renomear usarem `[terminal.font] size`, e o [ADR-0084](0084-fonte-do-sistema-e-ajustes-da-barra-de-abas.md) trocou a família deles pela fonte de interface do sistema, mantendo o tamanho. O resultado é que aumentar a fonte do terminal faz o texto da barra crescer junto, numa aba de altura fixa (34px), e a barra deixa de parecer a interface do sistema. O dono pediu, em 2026-11-27, que esse texto acompanhe o tamanho padrão da fonte do sistema.

## Decisão

Rótulo da aba, nome do grupo e campo de renomear têm um tamanho só: o **padrão da fonte de interface do sistema**, a constante `tab_bar::SYSTEM_UI_FONT_SIZE` em pixels lógicos:

| Plataforma | Valor | Origem |
|---|---|---|
| Windows | 12 | Segoe UI 9pt a 96 dpi |
| macOS | 13 | fonte de sistema, 13pt |
| demais | 13.33 | 10pt, o padrão de GTK e Qt |

Não há chave de configuração: `[terminal.font] size` e o zoom de sessão passam a valer só para a grade. A escala do monitor continua sendo aplicada pelo render, como em todo valor lógico.

## Consequências

- No Windows o texto da barra passa de 14 a 12px com a configuração padrão; no macOS, a 13px.
- A descrição da opção "Tamanho" na tela de configurações cita só o terminal.
- Os valores de `TabBarStyle::font_size` e `pill_font_size` saem da verificação de `scripts/verify-docs.py`, porque não têm mais chave no TOML.
- **Dívida:** o valor é constante por plataforma, não lido do sistema; uma escala de texto personalizada do usuário (Windows, GNOME, KDE) não é refletida. Ler o tamanho do SO exigiria uma chamada de plataforma insegura ou um crate novo, e fica para um ADR próprio.
