# ADR-0102 — Fechar o terminal parado ao restaurar uma sessão na janela atual

**Status:** Aceito — verificado só por teste automatizado (verificação ao vivo no Windows é dívida, no roadmap)
**Data:** 2026-11-12
**Relacionados:** [ADR-0017](0017-ciclo-de-vida-da-aba.md), [ADR-0034](0034-deteccao-de-processo-ativo-para-confirmacao.md), [ADR-0037](0037-aba-nao-iniciada.md), [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md), [ADR-0092](0092-restauracao-nomeada-na-janela-atual.md), [PRD-023](../prd/prd-023-restaurar-sessao-nomeada-na-janela-atual.md)
**Supersedes:** ADR-0092 §4 e §6 (**parcial**: "nenhuma aba é fechada" ganha uma exceção) · PRD-023 RF-23.7 (**parcial**)

## Contexto

Com `[session] restore_named_in_current_window = true`, restaurar uma sessão nomeada **acrescenta** grupos e abas ao fim da trilha da janela de origem e nunca fecha nada (ADR-0092). O caso mais comum de uso é abrir o app, que sobe com um terminal vazio, e escolher uma sessão. O terminal vazio fica sobrando como primeira aba, sem que o usuário o tenha usado.

## Decisão

**Se a janela de origem tem uma única aba e ela é um terminal parado, essa aba é fechada depois de a sessão ser acrescentada.**

### 1. O que é "única aba" e "terminal parado"

`app_named_append::lone_idle_tab(&Workspace, is_idle)` é pura e decide pela forma do modelo; a metade de runtime entra como `is_idle`, que vem de `WindowState::tab_is_idle_terminal`. Todas as condições valem:

- nenhuma aba fixada, e exatamente **uma** aba na trilha (solta ou sozinha num grupo explícito);
- **um painel só** (um painel dividido é um layout deliberado);
- a aba **não** é `NotStarted` (uma aba restaurada e ainda não aberta é trabalho salvo, não um terminal vazio) e o shell **não** terminou (uma aba com o shell encerrado continua aberta para que a última saída possa ser lida);
- o painel tem runtime, **não** é SSH (vivo, dormente ou reconectando) e `any_pane_needs_confirmation` é falso: sem processo extra no grupo, sem tela alternativa e sem relato de mouse.

`any_pane_needs_confirmation` é chamada com `confirm_close_with_process` **forçado a `true`**: fechar uma aba por conta própria nunca pode matar um processo em silêncio, mesmo que o usuário tenha desligado o diálogo de fechar.

### 2. A ordem em `append_named_window` importa

1. `lone_idle_tab` é avaliada **antes** de acrescentar; depois há mais de uma aba e a pergunta não tem resposta.
2. Acrescenta, sobe os runtimes, ativa a aba ativa traduzida da sessão.
3. **Só então** `close_tab_unconditionally(parada)`. A aba parada deixou de ser a ativa, então a escada de foco do core não roda, e `activate_tab` não expande um grupo colapsado da sessão (RF-23.6).

Uma sessão sem nenhuma aba (RF-23.13) acrescenta nada e **não** fecha a aba parada. Uma sessão que não gravou aba ativa também não: não há para onde mandar o foco com segurança.

### 3. Limitação assumida

Fora do Windows, `ProcessGroup::process_count()` vale sempre `1`, então só a tela alternativa e o relato de mouse protegem um processo em primeiro plano. É a mesma limitação do diálogo de fechar aba (ADR-0034): um `node server.js` solto num terminal comum, em Linux ou macOS, é visto como parado. Fica como dívida, junto com a do ADR-0034.

## Alternativas descartadas

- **Fechar também a aba com shell encerrado.** Pode ter uma mensagem de erro que o usuário queria ler.
- **Respeitar `confirm_close_with_process`.** Quem desligou o diálogo desligou a pergunta, não a proteção contra perda silenciosa.
- **Perguntar.** O caso do terminal vazio é justamente o que o usuário já disse não ligar para ele.

## Consequências

- O comentário do arquivo de exemplo e a descrição da opção nos cinco idiomas passam a citar a exceção.
- O PRD-023 ganha o RF-23.16 e uma emenda no RF-23.7.
- Fica pendente a verificação ao vivo: abrir vazio, restaurar, e repetir com um processo rodando.
