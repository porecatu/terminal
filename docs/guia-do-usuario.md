# Guia do usuário

RF-11.22, escrito no fechamento da F6. Cobre instalação, arquivo de
configuração, atalhos, integração de shell e a convenção do `Shift` para
selecionar texto dentro de um programa que pede o mouse. Para decisões de
arquitetura e o "porquê" de cada coisa, veja os ADRs linkados; este guia é
só o "como usar".

## Instalação

Cada release publica, por plataforma, um instalador nativo **e** o binário
cru — os dois carregam a mesma licença, a mesma atribuição de fonte e o
mesmo `sha256` ([ADR-0044](adr/0044-empacotamento-e-release.md)).

| Plataforma | Artefato | O que ele faz |
|---|---|---|
| Windows | `porecatu-vX.Y.Z-x86_64-pc-windows-msvc.msi` | Instala em `Arquivos de Programas`, cria entrada no menu iniciar com o ícone próprio, registra em Adicionar/Remover Programas |
| macOS (Apple Silicon) | `porecatu-vX.Y.Z-aarch64-apple-darwin.dmg` | Monta um volume com `Porecatu.app` — arraste para `Applications` |
| macOS (Intel) | `porecatu-vX.Y.Z-x86_64-apple-darwin.dmg` | Idem, para Mac Intel |
| Linux (Debian/Ubuntu) | `porecatu-vX.Y.Z-x86_64-unknown-linux-gnu.deb` | `sudo apt install ./porecatu-*.deb` (ou `dpkg -i`) — entra no menu de aplicativos |
| Linux (qualquer distribuição) | `porecatu-vX.Y.Z-x86_64-unknown-linux-gnu.AppImage` | `chmod +x`, executar direto — não precisa instalar nada |
| Qualquer plataforma | binário cru (`.exe` ou sem extensão) | Para quem prefere pôr o executável num diretório do `PATH` à mão |

### Conferindo o `sha256`

Todo artefato vem com um arquivo `.sha256` ao lado. Depois de baixar os dois:

```bash
# Linux/macOS
sha256sum -c porecatu-vX.Y.Z-<alvo>.sha256

# Windows (PowerShell)
Get-FileHash porecatu-vX.Y.Z-x86_64-pc-windows-msvc.msi -Algorithm SHA256
# compare o hash impresso com o conteúdo do arquivo .sha256 ao lado
```

`OK`/hash batendo confirma que o arquivo chegou intacto e é o que a release
publicou — não confirma quem o publicou (isso é o que assinatura de código
faria; ver a seção seguinte).

### SmartScreen (Windows) e Gatekeeper (macOS)

O Porecatu **não assina** os executáveis nesta versão
([ADR-0044 §5](adr/0044-empacotamento-e-release.md)): certificado
Authenticode (Windows) e conta de desenvolvedor Apple com notarização
(macOS) são custo recorrente em dinheiro e exigem guardar chave privada como
segredo de CI — fora de escopo por ora.

Consequência prática:

- **Windows**: o SmartScreen mostra "O Windows protegeu o computador", com
  um app "desconhecido". Clique em **Mais informações** e depois
  **Executar assim mesmo**. Isso é esperado, todo primeiro download.
- **macOS**: o Gatekeeper recusa abrir na primeira tentativa ("não é
  possível verificar o desenvolvedor" ou similar). Clique com o botão
  direito (ou `Ctrl`+clique) no app, escolha **Abrir**, e confirme no
  diálogo — só precisa fazer isso uma vez.

Nenhum dos dois avisos significa que o binário foi adulterado; é o sistema
sinalizando "não assinado", que é diferente de "malicioso". Conferir o
`sha256` acima é a verificação que o projeto **pode** oferecer sem custo, e
que uma assinatura não substitui — ela garante integridade do download, não
identidade do publicador.

## Arquivo de configuração

Formato TOML, lido de um caminho por plataforma
([ADR-0003](adr/0003-formato-de-configuracao.md)):

| Plataforma | Caminho |
|---|---|
| Linux | `$XDG_CONFIG_HOME/porecatu/porecatu.toml` (default `~/.config/porecatu/porecatu.toml`) |
| macOS | `~/.config/porecatu/porecatu.toml` |
| Windows | `%APPDATA%\porecatu\porecatu.toml` |

Sem arquivo nesse caminho, o Porecatu roda com os defaults embutidos — não
é erro. [`porecatu.example.toml`](config/porecatu.example.toml) é a
referência completa: toda chave existente, com o valor default e um
comentário explicando o efeito. Copie o que quiser mudar; **chave que você
não escrever continua no default**, e chave desconhecida vira aviso na
barra do app, não erro que bloqueia o arranque.

Alterações no arquivo entram **a quente** (a maioria das chaves, sem
reiniciar) — o app assiste o arquivo e recarrega sozinho. Algumas mudanças
(fonte, por exemplo) exigem recalcular a grade e podem levar um instante a
mais para aparecer; nenhuma exige fechar e reabrir o app.

Outras formas de apontar o app para um arquivo de config diferente
([ADR-0040](adr/0040-superficie-de-linha-de-comando.md)):

```
porecatu --config /caminho/para/outro.toml
```

vence a variável de ambiente `PORECATU_CONFIG`, que por sua vez vence o
caminho de plataforma da tabela acima.

### Linha de comando, completa

```
porecatu                     restaura a última sessão gravada
porecatu <diretório>         sessão nova naquele diretório -- não restaura, não sobrescreve a gravada
porecatu --config <arquivo>  usa esse arquivo de config
porecatu --help / -h         imprime as formas acima e sai
porecatu --version / -V      imprime nome, versão e licença, e sai
```

## Atalhos

Catálogo completo e fechado em [docs/reference/acoes.md](reference/acoes.md)
— toda ação vinculável, com a seção de `[keybindings]` que a liga. Os mais
usados, no default de fábrica:

| Atalho (Win/Linux) | Atalho (macOS) | Ação |
|---|---|---|
| `Ctrl+Shift+T` | `Cmd+T` | Nova aba |
| `Ctrl+Shift+W` | `Cmd+W` | Fechar aba |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | igual | Próxima/anterior aba |
| `Alt+1`…`Alt+9` | `Cmd+1`…`Cmd+9` | Ir para a N-ésima aba |
| `Ctrl+Shift+G` | `Cmd+G` | Criar grupo com a seleção |
| `Ctrl+Shift+H` | `Cmd+Shift+H` | Dividir a aba: painel novo **abaixo** |
| `Ctrl+Shift+D` | `Cmd+Shift+D` | Dividir a aba: painel novo **à direita** |
| `Alt+←` `→` `↑` `↓` | igual | Trocar o painel focado |
| `Ctrl+Shift+N` | `Cmd+N` | Nova janela |
| `F11` | igual | Alternar tela cheia |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | `Cmd+C` / `Cmd+V` | Copiar/colar |
| `Shift+PageUp` / `Shift+PageDown` | igual | Rolar o scrollback |
| `Ctrl+Shift+F` | `Cmd+F` | Buscar no scrollback |
| `F3` / `Shift+F3` | igual | Próxima/anterior ocorrência da busca |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | `Cmd++` / `Cmd+-` / `Cmd+0` | Zoom da fonte (aumentar/diminuir/resetar) |
| `Ctrl+Shift+Y` | `Cmd+Y` | Alternar tema |
| `Ctrl+Shift+,` | `Cmd+,` | Recarregar config na hora |

Todo atalho é reconfigurável na seção `[keybindings]` (e
`[keybindings.macos]`/`[keybindings.linux]`/`[keybindings.windows]` para
desvio por plataforma) — [`porecatu.example.toml`](config/porecatu.example.toml)
tem a tabela inteira, comentada.

## Dividir a aba em painéis

Uma aba pode ser dividida em vários terminais, cada um com o próprio
shell, scrollback e diretório. `Ctrl+Shift+H` divide com o painel novo
**abaixo** do focado; `Ctrl+Shift+D`, com o painel novo **à direita**. O
gesto se repete sobre qualquer painel, quantas vezes couber — e é sempre
o **painel focado** que se divide, não a aba inteira.

> "Horizontal" e "vertical" nomeiam o **divisor**, não o arranjo, e cada
> emulador escolhe um lado dessa ambiguidade. Aqui: `H` deixa os painéis
> um **sobre** o outro, `D` deixa-os **lado a lado**.

O painel novo abre no mesmo diretório do painel de origem, e já nasce
focado.

**Trocar de painel:** clique dentro dele, ou `Alt+←` / `→` / `↑` / `↓`.
O painel focado é o que recebe tudo o que você digita — e é ele que a
barra de status descreve. Quando há mais de um painel, ela também mostra
a contagem.

**Como saber qual está focado:** pelo **cursor**. O painel focado tem o
cursor cheio, como sempre; os outros mostram o mesmo cursor **vazado**,
só o contorno. É a única diferença entre eles — não há cabeçalho, borda
nem esmaecimento. (Num painel rodando `vim` em modo normal, ou outro
programa que esconda o cursor, não há marca nenhuma: o foco é o último
painel em que você clicou ou para o qual navegou.)

**Redimensionar:** arraste o **espaço** entre dois painéis. Não há
divisor desenhado — o vão entre os quadros *é* o divisor, com a mesma
medida da margem entre a janela e o terminal. O cursor muda de forma
quando você passa por cima dele, e o conteúdo reencaixa enquanto você
arrasta. O arraste para sozinho quando um dos painéis chega ao tamanho
mínimo (`[panes] min_columns` e `min_rows` na config).

**Fechar um painel:** digite `exit` no shell dele, como numa aba. Fechar
o último painel fecha a aba. Se preferir uma tecla, vincule a ação
`pane.close` em `[keybindings]` — ela não tem atalho de fábrica, e
`Ctrl+Shift+W` continua fechando a **aba inteira**, com todos os painéis.

O layout volta quando você reabre o app: a divisão, as proporções e o
diretório de cada painel fazem parte da sessão.

## Selecionar texto dentro de um programa que pede o mouse (`Shift`)

Programas como `vim`, `htop`, `fzf` ou `less -R` pedem eventos de mouse
diretamente — quando isso acontece, arrastar o mouse normalmente vira
input **para o programa** (rolar uma lista, redimensionar um painel), não
seleção de texto. Isso é convenção de terminal antiga (xterm, e todo
emulador moderno segue): **segurar `Shift` enquanto seleciona força a
seleção local do Porecatu, sempre**, não importa o que o programa pediu
([ADR-0013](adr/0013-mouse-selecao-e-clipboard.md)).

Sem saber disso, a seleção "parece quebrada" dentro de qualquer programa
assim — não é bug, é o programa recebendo o clique. Segure `Shift` e
arraste para selecionar; copie com o atalho de sempre (`Ctrl+Shift+C` /
`Cmd+C`) ou o menu de contexto do botão direito.

## Integração de shell (restaurar o diretório de cada aba)

Fechar e reabrir o Porecatu restaura cada aba no diretório em que ela
estava — **quando o shell emite OSC 7** a cada mudança de diretório. Sem
isso, no Linux e macOS o Porecatu ainda descobre o diretório por um
caminho mais caro; no **Windows não há alternativa**, a aba volta para
onde foi aberta.

O Porecatu detecta a ausência e convida a configurar, uma vez por
execução, com o snippet pronto para colar no `bashrc`/`zshrc`/perfil do
PowerShell — os snippets por shell (bash, zsh, fish, PowerShell) estão em
[docs/reference/integracao-de-shell.md](reference/integracao-de-shell.md),
com o passo a passo de instalação e a forma exata do URI que o Porecatu
espera.

O convite aparece uma vez e pode ser dispensado. Quem diz o tempo todo em
que pé está é a **barra de status**, abaixo.

## Rodar um comando ao restaurar a aba (`.porecatu`)

A sessão devolve suas abas nos diretórios certos, mas não o que elas
estavam fazendo — o `npm run dev`, o `docker compose up`, você digita de
novo. Um arquivo chamado `.porecatu` na raiz do projeto resolve isso: ele
declara o que rodar, com uma seção por tipo de terminal.

```
[pwsh]
npm run dev

[bash]
nvm use
npm run dev
```

Tudo depois de `[nome]` é o script daquela seção, literal, até a próxima
seção. O nome é o do seu shell sem caminho nem extensão — o mesmo que
aparece no canto esquerdo da barra de status. `[default]` vale para
qualquer shell que não tenha seção própria.

Isso **não basta para o arquivo rodar**. Como o `.porecatu` vem junto com
o projeto — e projeto clonado é arquivo escrito por outra pessoa —, ele
só é executado em diretórios que você declarou na **sua** configuração:

```toml
[project_file]
trusted_paths = ["C:/Projetos"]
```

Um caminho listado cobre tudo abaixo dele. A lista é **vazia por
padrão**: numa instalação nova, nenhum `.porecatu` roda. Por isso mesmo,
evite listar o diretório onde você clona repositórios de terceiros, ou o
seu home — todo repositório novo ali dentro passaria a rodar o arquivo
dele sozinho.

Encontrando um `.porecatu` num diretório que você não autorizou, o
Porecatu não executa nada e escreve na aba o caminho e a linha que o
autorizaria, uma vez por execução.

Formato completo, exemplos por shell e o que fazer quando não funciona:
[docs/reference/arquivo-de-projeto.md](reference/arquivo-de-projeto.md).

O que o `.porecatu` **não** faz: não roda em aba nova (só na restauração
de uma sessão gravada), não roda quando você dá `cd` para o diretório,
não sobe a árvore de diretórios (um arquivo na raiz do projeto não
alcança uma aba em `src/`), não roda duas vezes na mesma aba, e não roda
escondido — o comando é escrito no terminal como se você o tivesse
digitado, aparece na tela, entra no histórico do shell e `Ctrl+C` o
interrompe. Para desligar tudo, `[project_file] enabled = false`.

## Sessões nomeadas

A sessão automática (acima) responde "onde eu parei?" — grava sozinha e
devolve no arranque. As sessões nomeadas respondem a uma pergunta vizinha:
"como eu monto o ambiente do projeto X?". Você salva a janela atual com um
nome, e reabre aquela disposição — grupos, abas, painéis — sempre que
quiser, numa janela nova.

**Salvar.** Dois jeitos: o atalho `Ctrl+Shift+S` (`Cmd+Shift+S` no macOS),
ou o botão de marcador na zona fixa da barra, à esquerda da engrenagem —
ele abre um popover com "Salvar esta janela…" no topo. Os
dois abrem um campo de texto; digite um nome e `Enter` confirma, `Esc`
cancela sem gravar nada. Só a janela em que você está entra — as demais
janelas abertas não são tocadas.

**Restaurar.** Clique no botão de marcador para ver a lista, e clique
numa sessão (ou realce com as setas e dê `Enter`). Isso abre uma **janela
nova**, em cascata a partir da atual, com a mesma disposição salva: os
mesmos grupos, abas e painéis, cada aba no `cwd` gravado (ou no diretório
inicial, com uma nota, se aquele diretório não existe mais). A janela de
onde você restaurou **não muda em nada**.

**Sobrescrever e excluir.** Salvar com um nome que já existe (sem
diferenciar maiúsculas) pede confirmação antes de substituir o arquivo; o
`✕` de cada linha exclui, também com confirmação. Nenhum dos dois
acontece sem esse passo.

**A diferença com a sessão automática, numa frase:** `[session] enabled =
false` desliga só a automática — as sessões nomeadas continuam
funcionando, gravadas e restauradas por gesto, com ou sem ela ligada.

O `.porecatu` roda numa janela restaurada por nome exatamente como rodaria
numa restaurada no arranque: só em diretório declarado em
`[project_file] trusted_paths`, uma vez por aba, no painel focado (acima).

**Onde os arquivos ficam**, num diretório `sessions/` ao lado do
`session.json`:

| Plataforma | Caminho |
|---|---|
| Windows | `%LOCALAPPDATA%\porecatu\sessions\` |
| Linux | `$XDG_STATE_HOME/porecatu/sessions/` (default `~/.local/state/porecatu/sessions/`) |
| macOS | `~/Library/Application Support/porecatu/sessions/` |

Cada sessão é um arquivo `.json` próprio — um por nome salvo, nunca lido
no arranque do app, só quando você abre o popover, salva ou exclui.

## Barra de status

A faixa no rodapé da janela mostra, da aba ativa:

| Zona | O que aparece |
|---|---|
| Esquerda | nome do shell, diretório atual, branch do Git, quantos commits atrás do remoto, grupo da aba, contagem de painéis |
| Direita | codificação e o sistema |

O diretório aparece com `~` no lugar da sua pasta pessoal, e é cortado à
direita quando a janela estreita — os outros campos são curtos e não
cedem espaço.

Com a aba dividida em painéis, o shell e o diretório mostrados são os do
**painel focado**. A contagem de painéis fecha a zona esquerda, depois do
grupo, e só aparece **a partir de dois painéis** — com um só, ela some por
completo, não vira "1 painel".

### Diretório apagado: o que significa

**Quando o diretório aparece num tom mais apagado que o resto, ele é o
diretório em que a aba foi aberta, não o atual.** O Porecatu não tem como saber para onde você
navegou: quem informa isso é o shell, com OSC 7 (acima). Sem essa
integração, um `cd` não chega até aqui.

Não é um erro, e nada quebra — mas duas coisas dependem disso: a
restauração de sessão reabre a aba no lugar errado, e uma aba nova herda
o diretório errado. Aplicar o snippet da seção anterior resolve as três
de uma vez, e o esmaecimento some assim que o primeiro `cd` acontecer.

### Repositório Git

Quando o diretório da aba está dentro de um repositório, aparece um ícone
de ramificação e o nome da branch atual. Fora de um repositório, os dois
somem — o ícone é a própria resposta a "estou num repositório?".

Com o `HEAD` destacado (depois de um `git checkout <commit>`), o lugar da
branch traz os sete primeiros caracteres do commit, como o próprio `git`
abrevia.

Sobre o **estado dos seus arquivos**, a barra não diz nada: alterações não
commitadas, o que está em stage, arquivos novos — nada disso aparece.
Saber a branch é ler um arquivo de trinta bytes; saber o resto é percorrer
a árvore inteira, e o Porecatu não faz isso enquanto você digita.

Quantos commits você está **atrás do remoto**, esse sim aparece — é a
seção seguinte, e é a única coisa que o Porecatu consulta pela rede.

O nome da branch vem do diretório que a barra conhece — então, se o
diretório estiver no tom apagado (acima), a branch pode ser a de outro
repositório. A integração de shell resolve as duas coisas de uma vez.

### Commits novos no remoto

De tempos em tempos, o Porecatu pergunta ao servidor se a branch da aba
ativa tem commits que você ainda não tem. Havendo, aparece um número ao
lado da branch:

```
  pwsh   ~/Projetos/api   ⑂ main   ↓ 3 commits atrás
```

**Esse número é clicável.** Clicar traz os commits — em segundo plano, sem
travar nada e sem você sair do que estava fazendo. Nada é trazido sem esse
clique: o Porecatu nunca mexe nos seus arquivos sozinho.

Sem commits novos, o número não existe. Não há "0 atrás" nem versão
apagada — como o ícone de branch, a ausência já é a resposta.

**Quando você também tem commits locais ainda não enviados**, o rótulo
muda e deixa de ser clicável:

```
  pwsh   ~/Projetos/api   ⑂ main   ↓ 2 atrás, 1 à frente
```

Nesse caso as duas histórias divergiram, e trazer os commits exigiria um
merge ou um rebase — decisão sua, tomada no terminal, onde você vê o que
está fazendo. O Porecatu não oferece um botão que ele já sabe que não
funcionaria.

Se o clique não der certo — arquivos modificados no caminho, senha
recusada, rede fora —, o app avisa no canto superior, com a mensagem que o
`git` deu. Clicar e não acontecer nada é o único resultado que não existe.

O que o Porecatu **não** faz aqui: não envia commits, não faz merge, não
faz rebase, não resolve conflito e não escolhe servidor (usa o que a
própria branch já segue). Uma branch que você nunca enviou não é
consultada e não mostra número.

**Mudar o intervalo, ou desligar:**

```toml
[git]
remote_poll_interval_secs = 300   # 0 desliga
```

O padrão é 300 segundos (cinco minutos). **`0` desliga tudo**: nenhuma
consulta, nenhum número, nenhuma conversa com a rede. Vale desligar se
você trabalha offline, se a rede é limitada, ou se abre repositórios de
terceiros que prefere não consultar — a consulta não toca nos seus
arquivos, mas escreve dentro da pasta `.git` do repositório.

Valores entre 1 e 29 viram 30, e o app avisa que subiu: um `1` ali seria
uma consulta de rede por segundo, em cada repositório aberto.

### Desligar

```toml
[appearance.status_bar]
enabled = false
```

Desligada, a barra devolve a altura ao terminal — o número de linhas da
grade cresce de volta, na hora, sem reiniciar. Cores, altura e tamanho da
fonte também são configuráveis; ver `[appearance.status_bar]` no
[arquivo de exemplo](config/porecatu.example.toml).

A única coisa clicável na barra é o número de commits atrás do remoto
(acima). Em todo o resto dela, a borda inferior da janela continua sendo
a área de redimensionar, mesmo em cima da barra — inclusive nos cantos,
que o número nunca alcança.
