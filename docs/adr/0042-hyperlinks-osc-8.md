# ADR-0042 — Hyperlinks OSC 8: spans no snapshot, abertura sob modificador, esquemas fechados

**Status:** Aceito
**Data:** 2026-09-04
**Relacionados:** ADR-0002, ADR-0004, ADR-0010, ADR-0012, ADR-0013, ADR-0014, ADR-0021, ADR-0032, PRD-011

## Contexto

A linha `| OSC 8 — hyperlinks | não — F6 |` está na **lista fechada** de sequências do [ADR-0012](0012-identificacao-do-terminal.md) desde antes da F1. A F6 é a fase que a vira, e três coisas precisam ser decididas antes de qualquer linha de código.

**A primeira é como o URI atravessa a fronteira.** Ao contrário do OSC 7 — que o `vte` descarta antes de chamar `Handler` nenhum, obrigando o `Osc7Watcher` a existir como segundo parser —, o **OSC 8 passa pelo `Handler`**: o `alacritty_terminal` implementa `set_hyperlink` e guarda o URI na célula (`Cell::hyperlink()`). O motor já faz o trabalho. O problema está do nosso lado: `porecatu_term::snapshot::Cell` é `Copy`, tem quatro campos e é reusado entre frames sem alocar (ADR-0007), e o comentário no topo de `snapshot.rs` registra a exclusão de propósito — *"não inclui tudo que o motor rastreia (hyperlink, cor de sublinhado) — só o que a especificação visual e o roadmap de F1 pedem; o resto entra quando tiver consumidor"*. Agora tem consumidor.

**A segunda é o gesto**, e ela tem um conflito de plataforma que já apareceu antes: no macOS `Ctrl`+clique **é** o clique secundário, o que o [ADR-0021](0021-selecao-multipla-e-gestos-da-barra.md) resolveu para a seleção múltipla escolhendo `Cmd` lá. E o `Shift` está tomado: ele é o modificador que força seleção local sobre o mouse do programa ([ADR-0013](0013-mouse-selecao-e-clipboard.md)), e essa regra não cede.

**A terceira é segurança, e é a que pesa.** O URI vem da **saída de um programa**. Um `cat` num arquivo hostil, um `ls --hyperlink` num diretório preparado, uma resposta de servidor ecoada no terminal — qualquer um pode plantar uma sequência OSC 8. Se um clique entrega o URI ao handler do sistema sem filtro, `file:///C:/Windows/System32/calc.exe` transforma "ler um arquivo" em "executar um binário". O v1 não pode oferecer essa escada.

Some-se uma restrição de código: `unsafe_code = "deny"` vale para o workspace e nunca foi excepcionado. Abrir um URI pelo handler do sistema é `ShellExecuteW` no Windows, `xdg-open` no Linux e `open` no macOS — nada disso se chama sem `unsafe` ou sem crate wrapper.

## Decisão

**O URI viaja como lista de spans ao lado do snapshot, a affordance e a abertura exigem modificador, e apenas quatro esquemas são aceitos — sendo `file` revelado no gerenciador de arquivos, nunca aberto.**

### 1. `Cell` não muda um byte

O hyperlink **não entra em `Cell`**. Entra como campo próprio do `GridSnapshot`, ao lado de `selection`, que já é exatamente esse tipo de informação esparsa:

- uma arena `String` reusada entre frames, no padrão de `clusters`, com os URIs concatenados;
- uma `Vec` de spans, cada um com a linha, a coluna inicial e final do trecho contíguo, o par de offsets na arena e o **id** que o OSC 8 carrega.

`Cell` continua `Copy`, com quatro campos, e o buffer continua reusado sem alocar no caminho quente. Link é esparso: uma grade de 80×24 tem 1920 células e quase sempre zero ou um link.

O **id** existe porque um mesmo link pode ser quebrado em vários trechos — por quebra de linha, ou por o programa reemitir a sequência. Passar o cursor sobre qualquer trecho sublinha **todos os trechos do mesmo id na vista**, que é o que faz um link partido em duas linhas parecer um link só.

### 2. O modificador é por plataforma, e não é o `Shift`

| Plataforma | Modificador |
|---|---|
| Windows, Linux | `Ctrl` |
| macOS | `Cmd` |

A razão de `Cmd` no macOS é a mesma que o ADR-0021 registrou: lá `Ctrl`+clique é o clique secundário e abriria o menu de contexto do terminal, que esta mesma fase cria. `Shift` está fora de discussão — é o modificador de seleção local do ADR-0013, e essa regra não tem exceção.

### 3. Affordance sob demanda, nunca por default

Sem o modificador pressionado, célula com link **desenha exatamente como qualquer outra**. Com o modificador pressionado, o trecho sob o cursor (e os demais trechos do mesmo id) ganha sublinhado e o cursor do mouse muda de forma.

Isso não é economia de trabalho, é decisão de produto: a saída de um programa não se enfeita sozinha. Sublinhado permanente marcaria de azul qualquer coisa que um `curl` ecoasse, e o Porecatu não decide o que na saída do usuário merece destaque.

Sem valor de aparência novo: o sublinhado é a flag `UNDERLINE` que o snapshot já carrega e o pintor já desenha, na cor do próprio texto.

### 4. Quatro esquemas, e `file` é **revelado**, nunca aberto

| Esquema | Comportamento |
|---|---|
| `http`, `https`, `mailto` | abre no handler padrão do sistema |
| `file` | **revelado no gerenciador de arquivos** — o caminho é localizado e selecionado, não entregue ao handler por extensão |
| qualquer outro | **não abre** — o URI é copiado para o clipboard e o app informa o esquema recusado |

O recorte de `file` é o ponto inteiro desta decisão. `http` e `https` levam a um navegador, que é um sandbox; `mailto` abre um cliente de e-mail com um rascunho. `file` entregue ao **handler por extensão do sistema** significa que um `.exe`, um `.bat` ou um `.lnk` **executam** — e o caminho veio da saída de um programa.

Revelar em vez de abrir elimina esse caminho por completo: o gerenciador de arquivos abre na pasta com o item selecionado, o usuário **vê** o que o link apontava, e abrir de fato continua sendo uma decisão dele, tomada fora do Porecatu. Não há diálogo, não há confirmação, não há gesto extra — e não há execução possível. Diretório e arquivo recebem o mesmo tratamento, então não há nem a ramificação de `Path::is_dir`.

O crate de abertura escolhido oferece as duas operações — abrir e revelar — atrás de features distintas, então isto não custa dependência a mais.

Esquema recusado **não é engolido em silêncio**: copiar o URI e dizer por quê é o que evita o usuário achar que o clique não funcionou.

### 5. A lista de esquemas não é configurável

`[terminal.hyperlinks] enabled` liga e desliga o recurso inteiro, e é tudo. Não há chave para acrescentar esquema.

A chave nasce na **etapa 3**, junto com o recurso: o arquivo de exemplo tem de bater chave por chave com `Config::default()`, e uma seção que a struct não conhece reprova o teste de auditoria da F4. Até lá ela está escrita no arquivo, comentada.

Uma lista de esquemas configurável é um mecanismo de escalada de privilégio embalado como conveniência: quem copia um `porecatu.toml` de um gist não lê a linha que acrescentou `ms-msdt:` à lista. O [ADR-0003](0003-formato-de-configuracao.md) já estabelece que a config governa **aparência e comportamento**, não a fronteira de segurança do app.

### 6. Abertura por crate wrapper, com o URI como argumento

A abertura passa por um crate que encapsula o `unsafe` da chamada de plataforma, do mesmo jeito que `portable-pty`, `arboard`, `png` e `win32job` já fazem — a regra `unsafe_code = "deny"` continua sem exceção, como continuou na correção de processo zumbi. Requisito de licença compatível com GPLv3, conferido antes de adotar, como a convenção manda.

Restrição que a implementação tem de honrar: o URI vai como **argumento**, nunca interpolado numa string de shell. `xdg-open "$uri"` montado por concatenação é injeção de comando com o conteúdo da saída de um programa — o pior caminho possível.

### 7. Onde a decisão de abrir mora

`porecatu-term` reporta o span e o URI. **Quem decide abrir é `porecatu-ui`**, porque a política de esquemas é comportamento de app e porque `porecatu-term` não conhece GUI nem clipboard. O crate de terminal continua sem opinião sobre o que um URI significa — é a mesma fronteira que já vale para cor não resolvida e para `TermParams`.

## Alternativas consideradas

### `Option<String>` de hyperlink dentro de `Cell`

O caminho mais direto: o motor já guarda o URI na célula dele, e copiar isso para a nossa seria uma linha. Rejeitado por dois efeitos que se somam — `Cell` deixaria de ser `Copy`, e o snapshot passaria a alocar por célula com link **a cada frame**, no caminho mais quente do app. O ADR-0007 comprou o snapshot reusado justamente para não alocar ali.

### Par de offsets na arena, guardado em cada `Cell`

Preserva o `Copy` e foi a forma considerada primeiro. Rejeitada porque paga 8 bytes por célula para representar informação que quase sempre não existe: numa grade de 80×24 seriam ~15 KB por frame de `(0, 0)` para dizer "sem link". A lista de spans custa proporcional ao número de links, e o snapshot já tem precedente exato disso em `selection`.

### Sublinhado permanente em todo link

Convenção de vários emuladores, e mais descobrível. Rejeitada porque marca a saída do usuário sem pedir licença: `git log`, `cargo` e qualquer coisa com `--hyperlink` passariam a pintar trechos que o usuário não escolheu destacar. A affordance sob modificador dá a mesma informação no momento em que ela é útil.

### Clique simples, sem modificador

Menos atrito para seguir um link. Rejeitada por dois motivos independentes. Segurança: um clique acidental sobre a saída de um programa abriria um URI plantado, e a lista de esquemas seria a única defesa. E colisão de gesto: clique simples na área do terminal **já** posiciona seleção e já é reportado ao programa quando ele pede o mouse (modos 1000/1002/1003) — um link no meio do `htop` roubaria o clique dele.

### Só pelo menu de contexto, sem clique nenhum

Zero risco de abertura acidental, e reusaria o menu do terminal que esta fase cria de todo jeito — foi seriamente considerada. Rejeitada por custo de uso: seguir um link viraria dois gestos e uma leitura de menu, todas as vezes. O modificador entrega a mesma proteção com um gesto, porque exige intenção explícita. O item de menu **também** existe, para quem preferir; o que não existe é *só* ele.

### `file` na mesma vala de `http`, entregue ao handler do sistema

É o que a maioria dos emuladores faz. Rejeitado porque é a única entrada da lista que leva a **execução** e não a visualização: no Windows o handler por extensão roda `.exe`, `.bat`, `.cmd`, `.lnk`, e o caminho veio da saída de um programa.

### `file` aberto após confirmação em diálogo

Foi a decisão até a véspera desta redação, e reusaria o diálogo do [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), que já existe. Rejeitada quando ficou claro que o crate de abertura expõe **revelar** como operação de primeira classe: revelar não é "abrir com uma pergunta antes", é *não abrir* — o caminho de execução deixa de existir em vez de ficar atrás de um clique. E fica mais leve para o usuário, não mais pesado: um gesto, sem modal. Confirmação só faria sentido se revelar não fosse possível.

### Lista de esquemas configurável, com os quatro como default

Flexibilidade barata de implementar, e alguém vai pedir. Rejeitada: mover a fronteira de segurança para dentro de um arquivo de texto que as pessoas copiam de terceiros transforma a config em vetor. Quem realmente precisa de outro esquema copia o URI — que é o que o recurso já faz nesse caso.

### Detecção de URL em texto plano, sem OSC 8

Cobriria a maioria dos links reais, já que poucos programas emitem OSC 8. Rejeitada pelo custo no caminho quente: exigiria varrer a grade com regex por frame, e "medir texto sem cache no caminho quente" é a armadilha de performance nomeada deste projeto, com uma fase inteira de cicatriz. Fica fora do v1, registrado no PRD-011.

## Consequências

### Positivas

- `Cell` sai desta fase **byte a byte igual**, e o snapshot continua sem alocar por frame.
- A metade difícil já estava pronta: o motor entrega `Cell::hyperlink()` pelo `Handler`, sem o segundo parser que o OSC 7 precisou.
- Zero valor de aparência novo: a affordance é a flag `UNDERLINE` que o pintor já desenha.
- O único caminho de execução real (`file` entregue ao handler por extensão) **deixa de existir**: revelar não executa, e nenhum diálogo foi preciso.
- A superfície de configuração cresce **uma** chave booleana.

### Negativas

- Descobribilidade menor: sem modificador pressionado, nada indica que há link na tela. É a troca deliberada contra enfeitar a saída do usuário.
- `file` nunca abre o arquivo: quem clica num `ls --hyperlink` esperando ver o conteúdo recebe o gerenciador de arquivos. É a troca deliberada contra a única escalada real do recurso.
- Uma dependência nova no workspace, com o wrapper de `unsafe` que ela existe para prover.
- `GridSnapshot` ganha dois campos, e o teste de introspecção de `porecatu-session` **não** os pega — ele cobre `porecatu-core`, não o snapshot. Campo de snapshot continua sem rede.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| URI interpolado em string de shell por descuido na implementação | Baixa | **Alto** | A decisão §6 é explícita, e o crate wrapper recebe argumento; teste que passa um URI com metacaractere de shell e confirma que nada é interpretado |
| Esquema perigoso passar por normalização (maiúsculas, `%` encoding, espaço) | Média | Alto | Comparação de esquema depois de normalizar, e a lista é *allowlist* — o default de qualquer coisa não reconhecida é recusar, não abrir |
| Revelar não ser suportado em algum ambiente Linux (o mecanismo depende do gerenciador de arquivos) | Média | Baixo | Sem suporte, `file` cai na vala do esquema recusado: copia o URI e informa. Nunca cai no handler por extensão como fallback |
| `[terminal.hyperlinks]` ligado incomodar quem não quer o recurso | Baixa | Baixo | `enabled = false` desliga o recurso inteiro, inclusive a affordance |
| Link partido em muitas linhas gerar spans demais | Baixa | Baixo | Spans são por trecho contíguo por linha, e a lista é da vista, não do scrollback inteiro |
| Crate de abertura com licença incompatível com GPLv3 | Baixa | Médio | Conferência de licença antes de adotar, regra já registrada em CLAUDE.md |
