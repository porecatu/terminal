# ADR-0077 — Avisos do app em diálogo modal, no lugar de nota escrita no grid

**Status:** Aceito
**Data:** 2026-10-28
**Relacionados:** [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0039](0039-convite-a-integracao-de-shell.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md), [ADR-0076](0076-reconexao-automatica.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md)

## Contexto

O [ADR-0014](0014-superficie-de-aviso-e-dialogo.md) definiu dois canais para o que o app quer dizer ao usuário: o aviso da pilha (canal 1) e a **nota escrita no grid** do terminal (canal 2, `Terminal::inject_note`). A nota no grid se mistura com a saída do shell, entra no scrollback e na seleção, e some quando a tela é limpa. Era o caminho do convite de integração de shell (OSC 7), das mensagens de conexão, falha e reconexão SSH, do código de saída de um processo, do diretório que não existe mais, do perfil que sumiu e do `.porecatu`.

O dono do produto pediu que essas mensagens deixem de ser impressas no terminal e passem a um popover centralizado na janela, modal sobre o terminal. Isto vale como o aval visual que o CLAUDE.md exige.

## Decisão

**Toda nota que o app escrevia no grid vira um `ConfirmDialog` modal, centralizado na janela, com o fundo escurecido. Nenhuma nota do app sobra no grid.**

1. **Mesmo diálogo, mesmas medidas.** Reusa `[appearance.dialog]`, `layout_dialog` e `paint_dialog`; nenhum valor de aparência novo. O que muda: o primeiro texto é o corpo, os demais viram linhas (`LineTone::Text`), e o botão de confirmar pode não existir (aviso que só informa tem só **Fechar**).
2. **Fila por janela, sem roubar foco** (`notice_queue.rs`, espelho de `prompt_queue.rs`): o aviso abre quando a janela está em foco, a aba é a ativa, o painel é o focado e nenhum outro diálogo está aberto. Antes disso espera. Pergunta do backend (chave de host, senha) tem prioridade.
3. **Conectando e "Enter para conectar" (SSH).** "Conectando a {host}…" é um aviso vivo (`NoticeSlot::Connecting`) que fecha sozinho quando o painel conecta ou falha, e **cede** à pergunta do backend (chave de host, senha): sem isso a conexão esperaria a resposta e o aviso esperaria o diálogo fechar. "Enter para conectar a {nome}" (painel restaurado, RF-20.33) é um aviso com **Conectar**, de foco inicial -- o diálogo não é destrutivo e o `Enter` conecta, como o gesto no painel -- e **Fechar**; o gesto no painel segue valendo.
4. **Reconexão SSH: um aviso vivo por painel.** O texto (causa e estado: aguardando, conectando, desistiu) é **substituído** a cada passo do ciclo, no diálogo aberto ou na fila (`NoticeSlot::Reconnect`). Tem **Reconectar agora** (`ssh.reconnect`) e **Fechar**. Fechado pelo usuário, não reabre a cada tentativa: só o estado final (desistiu) o reabre. Ao reconectar, o aviso de espera some e entra o aviso informativo de que é outro shell (RF-20.29).
5. **Convite OSC 7:** texto da consequência e do trecho, com **Copiar trecho** (área de transferência), **Não mostrar mais** (dispensa definitiva, o que o marcador digitado fazia) e **Fechar**. O texto não pede mais para digitar o marcador. A detecção do marcador em `porecatu-term` fica, inofensiva e compatível.
6. **Texto no catálogo** ([ADR-0056](0056-catalogo-de-textos-da-interface.md)): `dialog.notice.*` nos cinco idiomas; o corpo reaproveita os `note.*` existentes.

## Consequências

- O scrollback e a seleção deixam de receber texto do app. `Terminal::inject_note` continua na API de `porecatu-term`, sem chamador em `porecatu-ui`.
- Um diálogo por janela: um aviso espera outro fechar. Avisos acumulados abrem um a um.
- Um processo que sai com código diferente de zero deixa o painel `Exited` com o aviso no modal, e não mais a linha no grid.

## Revisões

[ADR-0014](0014-superficie-de-aviso-e-dialogo.md) (canal 2), [ADR-0039](0039-convite-a-integracao-de-shell.md) (§1 e §4, a dispensa) e [ADR-0076](0076-reconexao-automatica.md) (§4, as notas por tentativa) têm a parte de **onde** o texto aparece revista aqui; o resto delas segue valendo. Cada uma ganha um blockquote apontando para este ADR.

## Riscos

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Linha longa do aviso cortada no fim (`measurer.truncate`) | Média | Baixo | As frases do catálogo cabiam na largura 760 (o default agora é 360: frase mais longa que o piso é cortada com reticências); as de erro do SSH têm uma ideia por linha |
| Modal interrompe quem digita no painel | Baixa | Médio | Abre só no painel focado e **Esc** fecha; o aviso vivo fechado não volta até o estado final |
| Aval visual do dono ainda não dado ao binário | Alta | Baixo | Registrado no roadmap como pendente |
