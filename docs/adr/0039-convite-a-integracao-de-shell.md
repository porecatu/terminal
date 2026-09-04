# ADR-0039 — Convite à integração de shell: nota no grid, uma vez, dispensável em definitivo

**Status:** Aceito
**Data:** 2026-09-04
**Relacionados:** [ADR-0005](0005-persistencia-de-sessao.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0036](0036-formato-do-arquivo-de-sessao.md), [ADR-0038](0038-fallbacks-de-cwd.md), PRD-003, PRD-010

## Contexto

O RF-3.1 é o requisito que decide se a restauração de diretório funciona ou não para a maioria dos usuários:

> Quando o app detecta que uma aba nunca emitiu OSC 7, ele oferece **uma vez**, de forma não intrusiva e dispensável em definitivo, o trecho de configuração adequado ao shell detectado. No Windows esse convite é mais proeminente, porque lá ele não é uma melhoria — é a condição para o recurso funcionar.

O ADR-0005 o registrou como "decisão de produto" e parou aí. Faltam quatro coisas, e nenhuma é detalhe de implementação:

1. **Qual superfície.** O [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) definiu três canais (aviso, diálogo, nota no grid) e o projeto tem cinco widgets de chrome. Um convite com bloco de código não cabe naturalmente em nenhum deles: a barra de aviso expira em 6 s e é de uma linha, e o diálogo interrompe o start.
2. **Qual o critério de "nunca emitiu".** Ausência é um não-evento; sem um gatilho, "nunca" é indistinguível de "ainda não".
3. **Onde vive a dispensa definitiva.** A config é arquivo do usuário e o app **não escreve nela** — a decisão do [ADR-0031](0031-temas-nomeados.md) de que ciclar tema não grava no arquivo vale aqui pelo mesmo motivo.
4. **Onde vivem os snippets**, e quais shells eles cobrem.

Sem essas quatro, o recurso central do PRD-003 ficaria escondido atrás de uma configuração que o usuário não sabe que precisa fazer — e, no Windows, o produto pareceria quebrado ([ADR-0038](0038-fallbacks-de-cwd.md) §3).

## Decisão

**O convite é uma nota escrita no grid da própria aba, com o snippet do shell detectado, mostrada no máximo uma vez por sessão do app e dispensável em definitivo.**

### 1. Superfície: nota no grid

Usa o canal 2 do ADR-0014 e o mecanismo que já existe: `Terminal::inject_note`, que passa os bytes pelo parser do próprio motor (o grid é do motor; escrever nele por fora furaria a fronteira da seção 4 da arquitetura). A marcação visual é a do ADR-0014 — `#5ed3bc`, nunca imitando prompt.

Três consequências que decidem a escolha:

- **O snippet fica copiável de graça.** É texto no terminal; a seleção e o `Ctrl+Shift+C` do [ADR-0013](0013-mouse-selecao-e-clipboard.md) já funcionam sobre ele. Nenhuma outra superfície do projeto sabe entregar um bloco de código ao clipboard.
- **Nenhum widget novo.** O ADR-0014 continua com cinco, e o ADR-0032 continua valendo: nada muda de pixel.
- **Não expira e não interrompe.** A nota fica no scrollback, some quando o usuário rolar, e não bloqueia nada — que é a definição operacional de "não intrusivo".

**Posição: após a última linha de saída**, não na primeira. A regra do ADR-0017 §5 é que a posição segue o fato, e o fato aqui acontece depois de o usuário já ter usado a aba (§2), não na abertura dela.

### 2. Critério de detecção: o primeiro `cd` sem evento

A aba é marcada como "sem OSC 7" quando **o shell reporta um prompt novo depois de o usuário ter mudado de diretório, e nenhum `TermEvent::Cwd` chegou naquela aba até então**. Na prática, o gatilho é o primeiro caso em que o fallback do [ADR-0038](0038-fallbacks-de-cwd.md) devolve um `cwd` **diferente** do `cwd` de spawn da aba — o que prova que o diretório mudou e que o app só ficou sabendo pelo caminho caro.

No Windows não há fallback, então o gatilho é o tempo: a aba que completou o handshake do ConPTY e ficou **interativa por um intervalo** sem nenhum `TermEvent::Cwd` é tratada como sem OSC 7. É o único lugar em que o critério difere por plataforma, e difere porque a evidência disponível difere.

Nunca é um temporizador solto: como todo estado com prazo no projeto, ele recebe `Instant` de fora e entra no `next_deadline()` da janela via `ControlFlow::WaitUntil`.

### 3. Proeminência no Windows

Mesma superfície, texto diferente. Fora do Windows o convite diz que a restauração de diretório fica **mais exata** com o snippet. No Windows ele diz que, sem o snippet, o diretório **não é restaurado** — porque é a verdade, e é o que o PRD-003 documenta como limitação esperada.

"Mais proeminente" não vira outra superfície. Um diálogo modal no start seria proeminente e seria a pior primeira impressão possível do app.

### 4. Uma vez, e dispensável em definitivo

- **Uma vez por execução do app**, não por aba: dez abas sem OSC 7 produzem uma nota, na primeira que disparar o critério.
- **Dispensa definitiva** gravada em `session.json`, no campo `shell_integration_dismissed` do [ADR-0036](0036-formato-do-arquivo-de-sessao.md) §1. É estado da máquina, e o arquivo de sessão é onde estado da máquina mora — a config não é escrita pelo app, e um arquivo de marcação separado seria um terceiro lugar de estado sem ganho.
- **A dispensa é explícita**, não implícita: fechar a aba ou rolar a nota para fora da vista não dispensa nada. A nota traz a instrução de como dispensar, e a ação de dispensa é digitada pelo usuário no próprio terminal, do mesmo jeito que ele copiaria o snippet.
- `[session] suggest_shell_integration = false` desliga o convite antes de qualquer detecção, e `[session] enabled = false` o desliga junto com o resto (não há onde gravar a dispensa).

### 5. Snippets

Cobrem os shells que `resolve_default_shell` sabe escolher e os que o usuário tem chance de configurar em `[shell]`: **bash**, **zsh**, **fish**, **PowerShell** (5.1 e 7) e **cmd**. Para fish e para prompts que já emitem OSC 7 por padrão (starship, por exemplo), o texto diz que nada é preciso — detectá-los e mandar configurar seria pior que ficar calado.

Os snippets vivem em `docs/reference/integracao-de-shell.md`, versionados como documentação, e são **embutidos no binário** a partir dali. Um só lugar para corrigir, e o que o usuário vê na tela é o que o repositório diz.

O shell é o detectado no spawn da aba (`shell_name`, que o domínio já carrega). Shell não reconhecido: nota genérica explicando o que é OSC 7 e apontando o arquivo de referência, em vez de um snippet errado.

## Alternativas consideradas

### Barra de aviso do ADR-0014

Superfície pronta e já usada para o RF-4.21 e o RF-3.14. Descartada por duas razões que se somam: a barra expira em 6 s, e o convite precisa sobreviver o suficiente para o usuário ler, copiar e colar; e ela é de uma linha, então o snippet teria de sair por outro caminho — um botão "copiar" que colocaria um bloco de código no clipboard sem o usuário ter visto o conteúdo.

### Diálogo com "copiar" e "não mostrar de novo"

É a superfície mais proeminente, e "proeminente no Windows" parecia pedir isso. Descartada: interrompe o start com um modal sobre um recurso que o usuário ainda não sabe que quer, e o ADR-0014 já registra que diálogo é para decisão destrutiva, não para informação. O RF-3.1 pede "não intrusivo" na mesma frase em que pede "proeminente no Windows" — a leitura que satisfaz as duas é a nota com texto mais forte.

### Sexto widget de chrome, um painel de snippet

Resolveria copiar e dispensar com dois botões. Descartada: o ADR-0032 fechou a interface do v1, e abrir um widget novo para um convite mostrado uma vez na vida do usuário é o oposto da economia que o ADR-0014 estabeleceu.

### Gravar a dispensa na config do usuário

Seria o lugar "natural" de uma preferência. Descartada: o app não escreve na config, por decisão consistente desde o ADR-0031 (`theme.cycle` não grava). Escrever ali significaria reformatar um arquivo que o usuário mantém à mão, com comentários dele dentro.

### Um convite por aba, em vez de um por execução

Mais chance de ser visto. Descartada: dez abas restauradas produziriam dez notas idênticas, que é ruído, e o RF-3.1 diz "uma vez" literalmente.

## Consequências

### Positivas

- O snippet chega ao clipboard pelo mecanismo que o usuário de terminal já usa, sem botão, sem widget e sem o app pôr conteúdo no clipboard por conta própria.
- Nenhum pixel novo, nenhum widget novo: o ADR-0014 continua com cinco e o ADR-0032 continua fechado.
- A dispensa mora num arquivo que o app já escreve, com escrita atômica e recuperação já decididas.
- No Windows o texto diz a verdade sobre a limitação, que é o que o critério de saída da F5 exige documentar como comportamento esperado.

### Negativas

- Uma nota no scrollback é fácil de perder de vista: quem rolar a tela antes de ler não vê mais o convite naquela execução. Aceito — a alternativa era interromper.
- A dispensa por comando digitado é menos descobrível que um botão. Mitigado pela própria nota, que a explica.
- O gatilho no Windows é temporal, e temporal é sempre uma aproximação: um usuário que abra o app e não faça nada por muito tempo pode ver o convite sem nunca ter mudado de diretório.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Convite aparecer para quem **já tem** OSC 7 configurado | Baixa | Médio | O critério exige a ausência do evento; com OSC 7 presente ele nunca dispara. Teste com um shell emitindo OSC 7, verificando que nenhuma nota é escrita |
| Snippet errado para o shell detectado | Média | Alto | Shell não reconhecido cai na nota genérica, nunca num snippet de outro shell. Snippets versionados num arquivo só, embutidos dali |
| Nota escrita no meio da saída de um programa em tela alternativa | Média | Médio | Não escrever com a tela alternativa ativa; adiar para a volta à tela primária |
| Gatilho temporal do Windows disparar cedo demais | Média | Baixo | Intervalo dimensionado na etapa, com o `Instant` vindo de fora para ser testável sem dormir |
| Usuário dispensar sem querer e não achar como voltar | Baixa | Baixo | `[session] suggest_shell_integration` e o próprio arquivo de referência, que continua no repositório |
