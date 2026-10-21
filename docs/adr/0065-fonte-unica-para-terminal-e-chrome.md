# ADR-0065 — Fonte única para terminal e chrome

**Status:** Aceito
**Data:** 2026-10-21
**Relacionados:** [ADR-0026](0026-chrome-unificado-em-iosevka-fixed.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0032](0032-interface-do-v1-fechada.md), [PRD-004](../prd/prd-004-aparencia-do-chrome.md), [PRD-005](../prd/prd-005-aparencia-do-terminal.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md)

## Contexto

O [ADR-0026](0026-chrome-unificado-em-iosevka-fixed.md) unificou a **face** do chrome e do terminal (Iosevka Fixed nos dois), mas não as **chaves**: o arquivo de configuração continuou com quatro valores de fonte para a barra de abas, independentes de `[terminal.font]`, como o RF-4.10 e o RF-5.1 pediam:

| Chave | Valor | Uso |
|---|---|---|
| `[appearance.tabs] font_family` | `"Iosevka Fixed"` | família do chrome |
| `[appearance.tabs] font_size` | 13 | rótulo da aba |
| `[appearance.tabs.rename] font_size` | 12 | campo de rename da aba |
| `[appearance.groups] label_font_size` | 13 | nome da pílula do grupo |
| `[terminal.font] family` / `size` | `"Iosevka Fixed"` / 14 | grade |

O resultado foi texto de terminal a 14px ao lado de rótulo de aba a 13px, e uma tela de configurações ([PRD-016](../prd/prd-016-tela-de-configuracoes.md)) que só oferecia a fonte do terminal — mudar a família ali mudava a grade e deixava a barra na fonte antiga. Havia ainda um defeito de recarga: `[appearance.tabs] font_family` é classe A, mas as famílias só eram reconstruídas quando `[terminal.font]` mudava, então mudar só a família do chrome não tinha efeito até a próxima mudança de fonte do terminal.

O dono do produto pediu que tamanho e família da fonte das abas, dos grupos e do terminal sejam os mesmos.

## Decisão

### 1. `[terminal.font]` é a fonte da barra de abas

`family` e `size` de `[terminal.font]` passam a valer para o rótulo da aba, o nome da pílula do grupo e o campo de rename da aba. As quatro chaves da tabela acima saem do schema; escritas no arquivo do usuário, caem no aviso de chave desconhecida (RF-4.22), como qualquer outra.

### 2. Tamanho: 14

O valor que fica é o do terminal, 14 — par, para o avanço de 0.5 em da Iosevka Fixed cair em pixel inteiro (RF-5.3). Rótulo e pílula sobem de 13 para 14, o campo de rename de 12 para 14. A aba tem altura e largura fixas (§2.5 da especificação visual); o teto de 180px do rótulo passa a caber menos caracteres, e nada mais se move.

### 3. O zoom de sessão continua só na grade

`font.increase`/`decrease`/`reset` mudam o tamanho efetivo do terminal, não o do chrome: a barra lê o `size` configurado, sem zoom. A aba tem altura fixa (34px); um rótulo crescendo com o zoom estouraria a aba.

### 4. O peso da pílula continua 500

Família e tamanho são iguais; o nome do grupo continua em `Medium` para ler como bold contra o 400 da aba (§2.4). `[appearance.groups] label_font_weight` fica como estava.

### 5. Fora do escopo

Menus, avisos, diálogo, tooltip, editor de grupo, tela de configurações e barra de status mantêm os próprios tamanhos. O pedido foi a barra de abas e de grupos.

## Alternativas consideradas

### Manter as chaves do chrome com default igual ao do terminal

Igualaria a aparência padrão, mas manteria as duas fontes divergindo assim que o usuário mudasse uma — exatamente o que a tela de configurações provocava, por só expor uma delas. Descartada.

### Unificar em 13

Mudaria o terminal, que é o que o usuário lê o tempo todo, e um tamanho ímpar arredonda a célula a 100% de escala. Descartada pelo dono do produto.

### Zoom de sessão aplicado também ao chrome

Exigiria recalcular altura de aba e de barra a cada zoom, mudando a área do terminal duas vezes por gesto. Descartada pelo dono do produto.

## Consequências

### Positivas

- Uma família e um tamanho só, editáveis pela tela de configurações.
- Some o defeito de recarga da família do chrome: ela é a de `[terminal.font]`, que já reconstrói as famílias.

### Negativas

- Quem tinha as chaves antigas no arquivo passa a ver o aviso de chave desconhecida até removê-las.
- O rótulo da aba a 14px corta títulos alguns caracteres mais cedo.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Um tamanho de terminal grande (ex. 24) não caber na aba de 34px | Média | Baixo | Texto continua centrado na altura da aba; quem aumenta muito a fonte pode subir `tab_height`. Registrado aqui, sem teto novo |
| Alguém reintroduzir uma chave de fonte própria para o chrome | Baixa | Médio | Este ADR; reverter exige ADR novo |
