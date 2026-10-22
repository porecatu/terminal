# ADR-0067 — Temas em arquivos próprios

**Status:** Aceito
**Data:** 2026-10-22
**Supersedes:** [ADR-0031](0031-temas-nomeados.md) (parcialmente: §1 onde o tema mora, §2 precedência, §3 ordem do ciclo, §5 nome duplicado)
**Relacionados:** [ADR-0003](0003-formato-de-configuracao.md), [ADR-0030](0030-escopo-do-hot-reload.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [ADR-0059](0059-janela-de-configuracoes.md), [ADR-0066](0066-estado-pressionado-dos-botoes-de-janela.md), [PRD-005](../prd/prd-005-aparencia-do-terminal.md)

## Contexto

Os temas do [ADR-0031](0031-temas-nomeados.md) moravam dentro do `porecatu.toml`, como `[[themes]]`, e vinham sete deles embutidos (`catppuccin-mocha`, `gruvbox-dark`, `dracula`, `nord`, `catppuccin-latte`, `gruvbox-light`, `solarized-light`). As cores-base ficavam espalhadas em `[terminal.colors]` e em cada tabela de `[appearance.*]`, e a precedência "cor declarada fora do tema vence o tema" fazia uma cor esquecida resistir à troca de tema.

O arquivo de exemplo, que é também o arquivo criado para o usuário (RF-11.27), chegou a 1848 linhas, 238 delas citando RF, ADR ou PRD — umas 490 só de temas. O dono do produto pediu:

1. cada tema num arquivo próprio, `thm_<nome>.toml`, ao lado do `porecatu.toml`, inclusive o padrão;
2. só dois temas: o padrão escuro e um claro novo;
3. um `porecatu.toml` legível para quem não conhece a arquitetura;
4. os fundos dos botões de janela semitransparentes, no escuro e no claro.

O item 4 muda as seções 1/2 da especificação visual, e o [ADR-0032](0032-interface-do-v1-fechada.md) exige ADR para isso; ele entra aqui porque as cores agora moram nos arquivos de tema que este ADR cria.

## Decisão

### 1. Tema é arquivo, e é o único lugar de cor

Um tema é um arquivo `thm_<nome>.toml` na pasta do `porecatu.toml` (a do caminho resolvido, então `--config` e `PORECATU_CONFIG` deslocam os dois juntos, como já acontece com `locales/`). O nome do tema é o do arquivo, sem `thm_` e sem `.toml`; não há chave `name`. O escopo do ADR-0031 §1 continua: tema declara cor, e só cor.

**Toda cor sai do `porecatu.toml`.** Os campos de cor de `Config` deixam de ser lidos dele (`#[serde(skip_deserializing)]`): `[terminal.colors]`, as cores de `[appearance.*]` e `[[themes]]` viram chaves ignoradas. Um arquivo antigo traz dezenas delas, então elas não geram um aviso por chave: o app junta todas num aviso só, com a contagem (`notice.colors_ignored`).

O arquivo de tema tem tabelas curtas, que espelham as seções do config: `[terminal]` (com `[terminal.normal]` e `[terminal.bright]`), `[window]`, `[tabs]`, `[window_controls]`, `[groups]`, `[notices]`, `[dialog]`, `[context_menu]`, `[tooltip]`, `[group_editor]`, `[status_bar]` e `[settings]`. Toda cor do `Config` é temável — inclusive as 19 que o ADR-0031 deixava de fora (bordas e indicadores de aba, campo de renomear, fundo do indicador de overflow, fundo e borda da janela, commits atrás do remoto, nota no terminal).

### 2. Dois temas embutidos, gravados ao lado da config

`default` (o escuro, os valores do design) e `default_light` (claro, a partir da paleta Latte que o `catppuccin-latte` já usava, completada nas cores que ele não tinha). Os dois vêm de `docs/config/thm_default.toml` e `docs/config/thm_default_light.toml`, embutidos no binário por `include_str!`: o app funciona sem arquivo nenhum. Os outros cinco temas saem.

Quando o app cria o `porecatu.toml` (botão "Abrir arquivo no editor" ou o primeiro Salvar da tela de configurações), grava também os dois arquivos de tema que ainda não existirem — **nunca** sobrescreve. Um arquivo no disco com o nome de um embutido o substitui.

`thm_default.toml` e `Config::default()` são a mesma coisa, garantido por teste; os dois embutidos declaram **toda** cor, também por teste. Um campo de cor novo em `Config` sem entrada nos dois arquivos reprova.

### 3. Precedência: tema escolhido, depois `default`

```
tema escolhido  >  tema default  >  default embutido
```

Chave ausente no tema escolhido cai no tema `default` — o do disco, se o usuário o editou. Assim um tema do usuário pode declarar só o que muda. `palette` continua substituída inteira (ADR-0031 §2). Some a regra "cor declarada fora do tema vence" e, com ela, o aviso de cor vencendo o tema.

### 4. Seleção e ciclo

`[terminal] theme` passa a ter default `"default"`. Nome vazio ou sem arquivo desenha com o `default`; sem arquivo continua aviso (ADR-0031 §5). Nome duplicado deixa de existir: o sistema de arquivos não deixa.

`theme.cycle` anda na ordem `default`, `default_light`, depois os do usuário em ordem alfabética do nome — os arquivos não têm outra ordem. O passo "sem tema" do ADR-0031 §3 sai: sem tema **é** o `default`, que continua alcançável pelo atalho. Uma sessão gravada com o `""` antigo vale como nenhum tema de sessão. Ciclar continua sem escrever no arquivo (ADR-0031 §4).

### 5. Recarga

Os `thm_*.toml` da pasta assistida recarregam como o próprio `porecatu.toml` (classe A, ADR-0030): o watcher já assistia a pasta, e o evento de um tema passa a contar como evento da config. Arquivo de tema inválido ou ilegível não derruba nada: o tema dele fica fora da lista e um aviso cita o arquivo (`notice.theme_file_invalid`); chave desconhecida num tema também é aviso (`notice.theme_file_unknown_key`).

### 6. Fundos dos botões de janela translúcidos

| Chave do tema | `default` | `default_light` |
|---|---|---|
| `[window_controls] hover_background` | `#ffffff1a` (branco a `.10`) | `#0000001a` |
| `[window_controls] pressed_background` | `#ffffff29` (branco a `.16`) | `#00000029` |
| `[window_controls] close_hover_background` | `#c4413fd9` (`.85`) | `#d20f39d9` |
| `[window_controls] close_pressed_background` | `#a33533e6` (`.90`) | `#a50c2de6` |

`.16` é o alfa do rim de vidro (§1.2 da especificação); `.10` é um degrau abaixo, como o hover era um degrau abaixo do pressionado no ADR-0066. Os vermelhos mantêm o tom e ganham alfa alto, para o fechar continuar lido como destrutivo. Ícones continuam opacos. Com imagem de fundo na janela ([ADR-0062](0062-imagem-de-fundo-da-janela.md)) ou janela transparente, o botão deixa de tapar o que está atrás da barra.

### 7. O `porecatu.toml` para leigo

O exemplo é reescrito em português simples, com comentário de uma linha dizendo o efeito e os valores válidos, sem RF, ADR, PRD nem classe de recarga. A ordem é a de quem procura: geral, shell, terminal, janela, sessão, arquivo de projeto, Git, painéis, atalhos. As dimensões de aparência que o `scripts/verify-docs.py` exige no exemplo continuam lá, numa seção **"Avançado"** no fim, sem comentário por linha. O teste de que o exemplo é `Config::default()` com zero chaves desconhecidas continua valendo.

## Alternativas consideradas

### Manter cor no `porecatu.toml` como override do tema

Era a regra do ADR-0031 §2, e permite ajustar uma cor sem criar arquivo. Rejeitada a pedido do dono do produto: deixa duas fontes de cor, mantém a cor esquecida que "resiste" ao tema e obriga o exemplo a listar a paleta inteira. Quem quer ajustar uma cor copia o tema.

### Temas só em disco, sem embutido

Mesmo modelo de `locales/`. Rejeitada: sem os arquivos o app ficaria sem tema claro, e cada instalador precisaria levá-los. Embutido mais gravado cobre os dois casos.

### Tirar as dimensões técnicas do exemplo

O arquivo ficaria menor ainda. Rejeitada a pedido do dono do produto: a regra de três lados (código, exemplo, especificação) do `verify-docs.py` continua, e a seção "Avançado" separa o que o leigo não precisa ler.

## Consequências

### Positivas

- O `porecatu.toml` cai de 1848 para menos de 500 linhas, sem uma cor.
- Criar um tema é copiar um arquivo; trocar de tema não arrasta cor esquecida.
- Toda cor do app é temável, e o tema claro deixa de ter bordas e indicadores escuros.

### Negativas

- Quem usava um dos cinco temas que saíram ou tinha `[[themes]]` próprio perde o tema ao atualizar e precisa recriá-lo como arquivo. O aviso de cores ignoradas diz para onde elas foram.
- Ajustar uma só cor exige copiar o tema inteiro (ou criar um tema de poucas chaves, que cai no `default`).

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Usuário atualizar com o arquivo antigo e ver as cores dele sumirem | Alta | Médio | Um aviso, com a contagem e o nome dos arquivos de tema; os dois temas embutidos são gravados ao abrir o arquivo |
| `thm_default.toml` divergir de `Config::default()` | Baixa | Médio | Teste de igualdade e de completude; `verify-docs.py` rastreia as cores dele contra a especificação |
| Usuário editar `thm_default.toml` e quebrá-lo | Média | Baixo | Arquivo inválido é aviso; o `default` embutido continua valendo |
