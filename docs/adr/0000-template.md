# ADR-0000 — Template e índice

## Índice de decisões

| # | Título | Status |
|---|--------|--------|
| [0001](0001-stack-de-gui.md) | Stack de GUI: winit + wgpu + glyphon | Aceito |
| [0002](0002-motor-vte.md) | Motor VT: crate alacritty_terminal | Aceito |
| [0003](0003-formato-de-configuracao.md) | Configuração em TOML | Aceito |
| [0004](0004-pty-cross-platform.md) | PTY cross-platform via portable-pty | Aceito · "Decisão" revista por ADR-0071 (**parcial**: o PTY vira um de dois backends) |
| [0005](0005-persistencia-de-sessao.md) | Persistência de sessão em JSON versionado | Superseded by ADR-0036 e ADR-0038 (os dois **parcialmente**) · "Quando gravar" revisto por ADR-0054 (**parcial**: só a premissa de que toda gravação é automática) |
| [0006](0006-modelo-de-abas-e-grupos.md) | Modelo de abas e grupos | Aceito · Alternativas Superseded by ADR-0053 (**parcial**: só o item "Splits/panes dentro da aba") |
| [0007](0007-modelo-de-threading.md) | Modelo de threading e render damage-driven | Aceito · uma linha das Consequências revista por ADR-0052 (**parcial**) · "Runtime async" revisto por ADR-0072 (**parcial**: `tokio` só dentro de `porecatu-ssh`) |
| [0008](0008-teclas-e-roteamento-de-input.md) | Keybindings e roteamento de input | Aceito |
| [0009](0009-referencia-visual-e-reconciliacao.md) | Referência visual e reconciliação com o design canvas | Superseded by ADR-0027, ADR-0028 e ADR-0032 (parcial) · §6 Superseded by ADR-0059 (**parcial**: só a forma do painel, que vira janela própria; escrever no TOML sem segunda fonte de verdade continua) |
| [0010](0010-licenciamento.md) | Licenciamento sob GPL-3.0-or-later | Aceito |
| [0011](0011-toolchain-rust.md) | Toolchain Rust pinada e política de versão | Aceito |
| [0012](0012-identificacao-do-terminal.md) | Identificação do terminal: `TERM` e capacidades anunciadas | Aceito |
| [0013](0013-mouse-selecao-e-clipboard.md) | Mouse, seleção e clipboard | Aceito |
| [0014](0014-superficie-de-aviso-e-dialogo.md) | Superfície de aviso, diálogo e menu de contexto | Aceito · menu de contexto revisto por ADR-0070 (**parcial**: o menu de perfis, ancorado num botão) · diálogo e canais revistos por ADR-0075 (**parcial**: campo, caixa de marcar e linhas de dado; consumidores do SSH) |
| [0015](0015-multiplas-janelas.md) | Múltiplas janelas no v1, em escopo mínimo | Aceito · "Escopo" Superseded by ADR-0059 (**parcial**: uma janela sem abas, a de configurações) |
| [0016](0016-fontes-embutidas.md) | Fontes do design embutidas no binário | Superseded by ADR-0024 |
| [0017](0017-ciclo-de-vida-da-aba.md) | Ciclo de vida e identidade da aba | Superseded by ADR-0034, ADR-0037 e ADR-0053 (os três **parcialmente**) · cobertura do RF-1.6 revista por ADR-0071 (**parcial**: aba SSH do cliente embutido confirma) |
| [0018](0018-composicao-de-frame.md) | Composição de frame: camadas, recorte e medição de texto | Aceito |
| [0019](0019-tooltip.md) | Tooltip, o quarto widget de chrome | Aceito |
| [0020](0020-grupos-explicitos.md) | Grupos explícitos: multiplicidade do implícito, colapso e foco | Aceito |
| [0021](0021-selecao-multipla-e-gestos-da-barra.md) | Seleção múltipla e gestos da barra de abas | Aceito |
| [0022](0022-animacao-de-interface.md) | Animação de interface sob render damage-driven | Aceito |
| [0023](0023-editor-de-grupo.md) | Editor de grupo, o quinto widget de chrome | Superseded by ADR-0035 (parcial) |
| [0024](0024-face-de-icones.md) | Face de ícones embutida (Lucide) | Superseded by ADR-0025 |
| [0025](0025-iosevka-no-lugar-da-ibm-plex.md) | Iosevka no lugar da IBM Plex | Superseded by [ADR-0026](0026-chrome-unificado-em-iosevka-fixed.md) (**parcial**: só a família do chrome; Fixed no terminal e o recorte continuam valendo) |
| [0026](0026-chrome-unificado-em-iosevka-fixed.md) | Chrome unificado em Iosevka Fixed | Aceito |
| [0027](0027-controles-de-janela-e-resize-proprios.md) | Controles de janela e resize próprios (Windows/Linux) | Aceito · botões revistos por ADR-0066 (estado pressionado) |
| [0028](0028-o-binario-como-referencia-visual.md) | O binário como referência visual | Aceito |
| [0029](0029-enum-de-acao-e-gramatica-de-tecla.md) | Enum de ação e gramática de tecla | Aceito |
| [0030](0030-escopo-do-hot-reload.md) | Escopo do hot reload | Aceito |
| [0031](0031-temas-nomeados.md) | Temas nomeados: escopo, merge e ciclo | Superseded by ADR-0067 (**parcial**: onde o tema mora, precedência, ordem do ciclo; "tema é só cor" e ciclar sem escrever continuam) |
| [0032](0032-interface-do-v1-fechada.md) | A interface do v1 está fechada | Aceito |
| [0033](0033-job-object-encerramento-de-processo.md) | Encerramento robusto de árvore de processo (Windows) | Aceito |
| [0034](0034-deteccao-de-processo-ativo-para-confirmacao.md) | Detecção de processo ativo para confirmação de fechamento | Aceito · "Decisão" revista por ADR-0071 (**parcial**: conexão SSH aberta também confirma) |
| [0035](0035-selecao-de-texto-em-campo-de-nome.md) | Cursor navegável e seleção de texto no campo de nome | Aceito |
| [0036](0036-formato-do-arquivo-de-sessao.md) | Formato do arquivo de sessão: DTO versionado em `porecatu-session` | Aceito · `TabV1` Superseded by ADR-0053 (**parcial**: campo novo, sem subir a versão) · §1 e §6 Superseded by ADR-0054 (**parcial**: dois campos opcionais e o diretório `sessions/`) · §3 revisto por ADR-0070 (**parcial**: `profile` por painel, sem subir a versão) |
| [0037](0037-aba-nao-iniciada.md) | Aba não iniciada: o terceiro estado do ciclo de vida | Aceito · Superseded by ADR-0053 (**parcial**: só a granularidade) · §2 e §6 revistos por ADR-0075 (**parcial**: painel SSH conecta por gesto, nunca por foco nem por `lazy_restore = false`) |
| [0038](0038-fallbacks-de-cwd.md) | Diretório de trabalho sem OSC 7: fallbacks por `sysinfo` | Aceito |
| [0039](0039-convite-a-integracao-de-shell.md) | Convite à integração de shell: nota no grid, uma vez, dispensável em definitivo | Aceito · Superseded by ADR-0053 (**parcial**: só o endereço da nota) |
| [0040](0040-superficie-de-linha-de-comando.md) | Superfície de linha de comando | Aceito · §1 Superseded by ADR-0056 (**parcial**: só o idioma do texto impresso, que passa a inglês fixo) |
| [0041](0041-busca-no-scrollback.md) | Busca no scrollback: barra sobreposta no topo do quadro | Aceito · Superseded by ADR-0053 (**parcial**: só o quadro sobre o qual ela se posiciona) |
| [0042](0042-hyperlinks-osc-8.md) | Hyperlinks OSC 8: spans no snapshot, abertura sob modificador, esquemas fechados | Aceito |
| [0043](0043-arvore-de-acessibilidade.md) | Árvore de acessibilidade: `accesskit` sobre o chrome, grade fora do v1 | Aceito · acrescentado por ADR-0053 (**parcial**: nós de painel; a grade continua fora) |
| [0044](0044-empacotamento-e-release.md) | Empacotamento por plataforma e a primeira release | Superseded by ADR-0045 (**parcial**: só §3, o número da versão) · §1 e §4 Superseded by ADR-0057 (**parcial**: binário cru deixa de ser publicado; arquivos de idioma em todo artefato) |
| [0045](0045-primeira-versao-0-7-0.md) | A primeira versão publicada é `0.7.0`, não `1.0.0` | Aceito |
| [0048](0048-barra-de-status.md) | Barra de status: faixa no rodapé que encolhe a grade | Aceito · §8 Superseded by ADR-0049 (**parcial**) · §3, §5 e §8 Superseded by ADR-0052 (**parcial**) · §5 Superseded by ADR-0053 (**parcial**: a contagem de painéis volta) |
| [0049](0049-branch-git-na-barra-de-status.md) | Branch do Git na barra de status: `.git/HEAD` por `mtime`, sem estado da árvore | Aceito · §7 Superseded by ADR-0052 (**parcial**: só ahead/behind) |
| [0050](0050-tela-cheia.md) | Tela cheia por `F11`: janela `Borderless`, sem estado novo em `Config` | Aceito |
| [0051](0051-arquivo-de-projeto-porecatu.md) | Arquivo `.porecatu`: seções cruas, confiança por allowlist e escrita no PTY | Aceito · §6 Superseded by ADR-0053 (**parcial**: qual terminal recebe o comando) |
| [0052](0052-sincronizacao-com-o-remoto-do-git.md) | Sincronização com o remoto do Git: poll em thread, contagem por `rev-list`, integração só por clique | Aceito |
| [0053](0053-paineis-divididos.md) | Painéis divididos: árvore binária no core, divisor que é ausência de pixel, foco pelo cursor | Aceito |
| [0054](0054-sessoes-nomeadas.md) | Sessões nomeadas: um arquivo por sessão ao lado do `session.json`, mesmo schema, restauração pelo caminho do arranque | Aceito |
| [0055](0055-botao-e-popover-de-sessoes.md) | Botão e popover de sessões: segundo botão da zona fixa, sétimo widget de chrome | Aceito |
| [0056](0056-catalogo-de-textos-da-interface.md) | Catálogo de textos da interface: arquivos TOML por idioma em disco, crate `porecatu-locale`, prosa só em `porecatu-ui` | Aceito |
| [0057](0057-idiomas-nos-artefatos.md) | Idiomas nos artefatos de release: `locales/` dentro de todo instalador, e o binário cru deixa de ser publicado | Aceito |
| [0058](0058-escrita-do-arquivo-de-configuracao.md) | Escrita do arquivo de configuração: `toml_edit`, edição por chave e gravação atômica | Aceito |
| [0059](0059-janela-de-configuracoes.md) | Janela de configurações: a primeira janela que não é um workspace de terminal | Aceito · §2 revisto por ADR-0064 (**parcial**: no Windows, modal e possuída pela janela de terminal) |
| [0060](0060-anatomia-da-tela-de-configuracoes.md) | Anatomia da tela de configurações: guia lateral, painel de opções e os controles que faltavam | Aceito — aval visual em 2026-10-02 · acrescentado por ADR-0075 (**parcial**: grupo "Perfis", com lista e formulário) |
| [0061](0061-imagem-de-fundo-do-terminal.md) | Imagem de fundo do terminal: `image` para decodificar, primitiva de imagem no render, alfa multiplicado | Aceito |
| [0062](0062-imagem-de-fundo-da-janela.md) | Imagem de fundo da janela: cabeça da camada da grade, recorte por quadro e estado em dois slots | Aceito · §4 Superseded by ADR-0063 (**parcial**: sem furo até o desktop com imagem da janela) |
| [0063](0063-imagem-da-janela-sem-furo-ate-o-desktop.md) | Imagem da janela sem furo até o desktop | Aceito |
| [0064](0064-janela-de-configuracoes-modal.md) | Janela de configurações modal, possuída pela janela de terminal | Aceito · §4 revisto (no X11, tipo diálogo para flutuar em gerenciador tiling) |
| [0065](0065-fonte-unica-para-terminal-e-chrome.md) | Fonte única para terminal e chrome: `[terminal.font]` vale para a barra de abas | Aceito |
| [0066](0066-estado-pressionado-dos-botoes-de-janela.md) | Estado pressionado dos botões de janela | Aceito · cores revistas por ADR-0067 (fundos translúcidos) |
| [0067](0067-temas-em-arquivos-proprios.md) | Temas em arquivos próprios (`thm_*.toml`), cor só no tema, botões de janela translúcidos | Aceito |
| [0068](0068-efeito-acrilico-no-fundo-do-terminal.md) | Efeito acrílico no fundo do terminal: desfoque da camada da janela em passe próprio, amostrado por quadro | Aceito — aval visual pendente |
| [0069](0069-hover-e-pressionado-dos-botoes-da-zona-fixa.md) | Hover e pressionado dos botões da zona fixa (sessões e configurações): quadrado arredondado revelado, disparo no release | Aceito |
| [0070](0070-perfis-de-aba.md) | Perfis de aba: lista em `[profiles]`, `[shell]` como perfil padrão implícito, identificador no painel, menu no clique direito do `+` | Aceito — aval visual pendente · §1 revisto por ADR-0072 (**parcial**: `type = "ssh"` aceito, com campos próprios) |
| [0071](0071-backend-de-terminal.md) | Backend de terminal: o PTY local passa a ser uma de duas implementações de um trait interno de `porecatu-term`, e o motor sobrevive à troca de backend | Aceito |
| [0072](0072-cliente-ssh-embutido.md) | Cliente SSH embutido: `russh` com `ring` num crate novo `porecatu-ssh`, runtime `tokio` isolado dentro dele, perfil `ssh` em `[[profiles.list]]` | Aceito |
| [0073](0073-credenciais-no-cofre-do-sistema.md) | Credenciais SSH no cofre do sistema: `keyring` dentro de `porecatu-ssh`, entrada por destino, gravada só depois de aceita | Aceito |
| [0074](0074-verificacao-da-chave-do-host.md) | Verificação da chave do host: `~/.ssh/known_hosts` do OpenSSH, leitura pelo `ssh-key`, casamento próprio, só acréscimo | Aceito |
| [0075](0075-superficies-de-ui-do-ssh.md) | Superfícies de UI do SSH: diálogo com campo mascarado e linhas de dado, pergunta que espera o foco da aba, grupo "Perfis" na tela de configurações | Aceito — aval visual pendente |
| [0076](0076-reconexao-automatica.md) | Reconexão automática: keepalive de 15 s, cinco tentativas em 1-2-4-8-16 s decididas por estado puro em `porecatu-ui`, uma reconexão por conexão | Aceito |
| [0077](0077-avisos-do-app-em-dialogo-modal.md) | Avisos do app em diálogo modal, no lugar de nota escrita no grid | Aceito |
| [0078](0078-largura-e-quebra-de-texto-do-dialogo.md) | Largura e quebra de texto do diálogo: piso 360, teto 720, nunca reticências | Aceito |
| [0079](0079-botao-de-menu-do-app.md) | Botão de menu do app: zona fixa à esquerda, com a caixa de uma aba solta e o ícone do app; hover e pressionado, sem ação | Aceito |

## Convenção

- Numeração sequencial de quatro dígitos, nunca reaproveitada.
- **0046 e 0047 não existem.** Foram escritos (largura de ícones tipo-emoji e desenho nativo de box drawing) e revertidos junto com o código que os acompanhava, antes de qualquer um dos dois chegar ao histórico. Os números ficam vagos, pela regra acima: quem os retomar escreve um ADR novo, com número novo.
- Nome do arquivo: `NNNN-titulo-em-kebab-case.md`.
- Status possíveis: `Proposto`, `Aceito`, `Rejeitado`, `Superseded by ADR-NNNN`.
- Decisão aceita não se edita. Para mudar, escreva um ADR novo com `Supersedes: ADR-NNNN` e marque o antigo. Correção de erro factual ou de clareza no texto é permitida; mudança de decisão, não.
- Ao adicionar um ADR, atualize a tabela acima e, se a decisão aparecer lá, a lista "Stack travada" do [CLAUDE.md](../../CLAUDE.md). *(Correção de fato: esta linha apontava para uma tabela de stack no README, que não existe.)*

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
