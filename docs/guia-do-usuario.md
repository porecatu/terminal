# Guia do usuário

RF-11.22, escrito no fechamento da F6. Cobre instalação, arquivo de
configuração, tela de configurações, idioma da interface, atalhos, painéis divididos, imagem de fundo, efeito acrílico, integração de shell e a
convenção do `Shift` para selecionar texto dentro de um programa que pede o mouse. Para decisões de
arquitetura e o "porquê" de cada coisa, veja os ADRs linkados; este guia é
só o "como usar".

## Instalação

Cada release publica, por plataforma, um instalador nativo, com a licença,
a atribuição de fonte, o exemplo de configuração e os arquivos de idioma
dentro, e um `sha256` para cada arquivo
([ADR-0044](adr/0044-empacotamento-e-release.md),
[ADR-0057](adr/0057-idiomas-nos-artefatos.md)). **Não há binário cru**: o
texto da interface vive em arquivos de idioma ao lado do executável, e um
`.exe` solto abriria sem texto. `cargo install` também não é um caminho
suportado, pelo mesmo motivo.

| Plataforma | Artefato | O que ele faz |
|---|---|---|
| Windows | `porecatu-vX.Y.Z-x86_64-pc-windows-msvc.msi` | Instala em `Arquivos de Programas`, cria entrada no menu iniciar com o ícone próprio, registra em Adicionar/Remover Programas |
| macOS (Apple Silicon) | `porecatu-vX.Y.Z-aarch64-apple-darwin.dmg` | Monta um volume com `Porecatu.app` — arraste para `Applications` |
| macOS (Intel) | `porecatu-vX.Y.Z-x86_64-apple-darwin.dmg` | Idem, para Mac Intel |
| Linux (Debian/Ubuntu) | `porecatu-vX.Y.Z-x86_64-unknown-linux-gnu.deb` | `sudo apt install ./porecatu-*.deb` (ou `dpkg -i`) — entra no menu de aplicativos |
| Linux (qualquer distribuição) | `porecatu-vX.Y.Z-x86_64-unknown-linux-gnu.AppImage` | `chmod +x`, executar direto — não precisa instalar nada |

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

A fonte é **uma só**: `[terminal.font] family` e `size` valem para o
terminal e também para a barra de abas — título da aba, nome do grupo e
campo de renomear ([ADR-0065](adr/0065-fonte-unica-para-terminal-e-chrome.md)).
As antigas `[appearance.tabs] font_family`/`font_size`,
`[appearance.tabs.rename] font_size` e `[appearance.groups] label_font_size`
não existem mais; se estiverem no seu arquivo, viram aviso de chave
desconhecida e podem ser apagadas.

**Cores não ficam no `porecatu.toml`**: ficam nos temas (ver
[Temas](#temas) abaixo). Uma cor escrita no `porecatu.toml` — de um arquivo
antigo, por exemplo — é ignorada, e o app avisa uma vez quantas encontrou.

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

## Temas

As cores do app — terminal, barra de abas, menus, avisos, barra de status,
janela de configurações — vêm de um **tema**. Cada tema é um arquivo
`thm_<nome>.toml` na mesma pasta do `porecatu.toml`, e o nome do tema é o do
arquivo, sem `thm_` e sem `.toml`.

O Porecatu traz dois:

| Tema | Arquivo |
|---|---|
| `default` (escuro, o padrão) | `thm_default.toml` |
| `default_light` (claro) | `thm_default_light.toml` |

Os dois funcionam mesmo sem os arquivos, porque vêm dentro do app. Quando o
`porecatu.toml` é criado (pelo botão **Open file in editor** ou pelo primeiro
Salvar da tela de configurações), o app grava também os dois arquivos de
tema, se ainda não existirem — nunca por cima de um que você já tenha.

Para **escolher** o tema: na tela de configurações, grupo **Aparência**, ou no
arquivo:

```toml
[terminal]
theme = "default_light"
```

`Ctrl+Shift+Y` (`Cmd+Shift+Y` no macOS) passa para o próximo tema só nesta
sessão, sem gravar nada: `default`, `default_light` e depois os seus, em
ordem alfabética.

Para **criar** um tema: copie `thm_default.toml` com outro nome, por exemplo
`thm_meu.toml`, e troque as cores. Cor que o seu tema não tiver usa a do
`default`, então um tema pode ter só as cores que mudam. Cores aceitam
`"#rrggbb"`, `"#rrggbbaa"` (os dois últimos dígitos são a transparência) ou
`"transparent"`. Salvou, aplicou: o app relê os temas sozinho. Um arquivo de
tema com erro vira aviso, e o tema dele fica fora da lista até ser
corrigido.

Editar `thm_default.toml` muda o tema padrão. Para voltar ao original, apague
o arquivo: o app volta ao embutido e o grava de novo na próxima vez que criar
os arquivos.

## Idioma da interface

O texto da interface — menus, diálogos, avisos, barra de busca, barra de
status, notas no terminal — vem de arquivos de idioma
([PRD-015](prd/prd-015-idioma-da-interface.md),
[ADR-0056](adr/0056-catalogo-de-textos-da-interface.md)). A chave é
`language`, em `[general]`, e o valor é o nome do arquivo sem a extensão:

```toml
[general]
language = "en_US"
```

Vêm dois idiomas prontos: **`en_US`** (inglês) e **`pt_BR`**
(português do Brasil). O default é **`en_US`**, fixo: o Porecatu não olha o
idioma do sistema. **Quem usava o app em português precisa de uma linha**:

```toml
[general]
language = "pt_BR"
```

A troca vale **ao vivo**, sem reiniciar: menu aberto e barra mudam na hora
nas janelas abertas. O que já foi composto por um evento fica como estava
até fechar — um diálogo aberto continua no idioma antigo, e um grupo que
você criou com o nome "Novo grupo" continua "Novo grupo" depois de trocar
para inglês (grupo novo nasce "New group"). Se o valor não for um nome de
idioma válido (`pt-BR` com hífen, por exemplo) ou não houver arquivo para
ele, o app abre em inglês e avisa, com o valor e os diretórios procurados.

### Onde ficam os arquivos

Os arquivos vivem **só em disco**; nenhum é embutido no executável. O app
procura em dois lugares e **mescla frase a frase**, com o inglês como
reserva:

| Camada | Onde |
|---|---|
| Instalado (Windows) | `bin\locales\` dentro da pasta de instalação (o `.exe` está em `bin\`) |
| Instalado (`.deb`) | `/usr/share/porecatu/locales/` |
| Instalado (AppImage) | `usr/share/porecatu/locales/`, dentro do próprio AppImage |
| Instalado (macOS) | `Porecatu.app/Contents/Resources/locales/` |
| Usuário | `locales/` **ao lado** do `porecatu.toml` (ver a tabela em "Arquivo de configuração") |

A camada do usuário vence a instalada, frase por frase. A que faltar nas
duas cai no `en_US`; a que faltar até no `en_US` aparece como o próprio
identificador (`tab_menu.close`), o que é feio de propósito: dá para ver
o que está faltando.

### Corrigir uma frase

Crie `locales/pt_BR.toml` ao lado do `porecatu.toml` e escreva **só** a
frase que quer trocar. Um arquivo de uma linha basta:

```toml
[tab_menu]
new = "Abrir aba"
```

O resto continua vindo do arquivo instalado. Os identificadores estão nos
arquivos instalados (`locales/en_US.toml` no repositório é a referência).
Um rótulo de menu que ficar largo demais é cortado com reticências.

### Acrescentar um idioma

1. Copie `en_US.toml` para `locales/<xx_YY>.toml` ao lado do
   `porecatu.toml`. O **nome do arquivo é o idioma**: duas ou três
   minúsculas, `_`, duas maiúsculas (`es_ES`, `de_DE`). Não há campo de
   idioma dentro do arquivo.
2. Traduza os valores. Não mude os identificadores nem os marcadores entre
   chaves (`{nome}`); plural é `{ one = "...", other = "..." }`.
3. Escreva `language = "es_ES"` em `[general]`.

Não precisa traduzir tudo: o que faltar sai em inglês, e o app mostra
**uma** nota dizendo quantos textos ficaram sem tradução, que some sozinha
em alguns segundos.

### O que fica sempre em inglês

- A **linha de comando**: `porecatu --help`, `--version` e as mensagens de
  argumento inválido. Não passam pelo catálogo.
- Os **nomes de tecla** (`Ctrl+Shift+N`) e os nomes das ações e das chaves
  do `porecatu.toml`.
- O detalhe técnico que vem de uma biblioteca, como a descrição do tipo
  errado num aviso de config inválida — o título e a posição saem no seu
  idioma.

### Quando os arquivos faltam

Instalação sem nenhum arquivo de idioma (executável solto, sem
`locales/`): a interface mostra os **identificadores** no lugar das frases
e uma única frase fixa em inglês — *"language files not found"*, com os
diretórios procurados — no aviso e no `stderr`. Reinstalar pelo instalador
resolve. Fora de uma instalação, `PORECATU_LOCALES=<pasta>` aponta o app
para uma pasta de arquivos; é um recurso de teste e desenvolvimento, não um
contrato.

## Tela de configurações

Para mudar uma opção sem editar o `porecatu.toml` à mão, o Porecatu tem uma
tela de configurações ([PRD-016](prd/prd-016-tela-de-configuracoes.md)): uma
janela própria, com a lista de grupos à esquerda e as opções do grupo
escolhido no resto da janela. O arquivo continua sendo a **única fonte de
verdade** — a tela só o lê e o grava, e quem aplica a mudança é a recarga a
quente de sempre. Editar o arquivo à mão continua sendo um caminho completo.

### Como abrir

- clique na **engrenagem**, na zona fixa à direita da barra de abas;
- ou a ação `settings.open`: `Ctrl+Shift+O` no Windows e no Linux, `Cmd+,` no
  macOS (no macOS, `config.reload` foi para `Cmd+Shift+,`).

A janela abre centrada sobre a janela de terminal de onde você a pediu, no
grupo **Geral**. Existe no máximo uma: abrir de novo a traz para a frente,
no mesmo grupo e com as alterações pendentes intactas. Ela não entra na
sessão e não volta ao reabrir o app. Fechar a última janela de terminal
fecha a tela junto, e, se houver alterações pendentes, pergunta antes.

**Em gerenciador de janelas tiling** (i3, bspwm e afins): no **X11** a tela
se declara janela de diálogo, então ela **flutua** sobre a janela de
terminal em vez de dividir a tela com ela, sem regra nenhuma no config do
gerenciador. No **Wayland** (sway e afins) não há esse sinal; para que ela
flutue, use uma regra pelo título, que segue o idioma da interface — por
exemplo, no sway:

```
for_window [title="^Configurações$"] floating enable
```

(`"^Settings$"` com a interface em inglês.)

### Os grupos

| Grupo | O que tem |
|---|---|
| **Geral** | idioma, diretório inicial, confirmações ao fechar aba e janela |
| **Shell** | programa, argumentos e variáveis de ambiente do shell |
| **Terminal** | fonte (do terminal **e** da barra de abas e grupos), cursor, histórico e rolagem, seleção, clipboard, hyperlinks, opacidade do fundo, imagem de fundo |
| **Aparência** | tema, animações, opacidade e decoração da janela, imagem de fundo da janela, posição da barra de abas, o que a aba mostra, barra de status |
| **Sessão** | gravar e restaurar a sessão, restauração preguiçosa, geometria das janelas, convite de integração de shell |
| **Projeto** | arquivo `.porecatu` ligado e os diretórios autorizados |
| **Git** | consultar o remoto e o intervalo |
| **Painéis** | mínimo de colunas e de linhas de um painel |
| **Atalhos** | todas as ações vinculáveis, com o atalho em vigor |

Cada opção mostra nome, descrição curta e o controle que o tipo pede:
alternância, escolha entre valores, campo numérico, campo de texto, lista.
Quando a mudança não vale na hora, a linha diz o escopo antes de você mexer —
"applies to new tabs" (vale em aba nova), "applies to the next window"
(vale na próxima janela) ou a necessidade de reiniciar.

Na **Aparência**, o tema é uma lista com uma amostra das cores de cada um —
`default`, `default_light` e os seus arquivos `thm_*.toml`. Um
tema escolhido na sessão por `theme.cycle` não é pendência nem é gravado: a
lista mostra o tema do arquivo, e uma linha abaixo diz qual a sessão está
usando, se for outro.

### Alterar, salvar e descartar

- Mudar uma opção **não grava**: a alteração fica pendente, com um ponto na
  linha e outro no nome do grupo.
- **Salvar** (`Ctrl+S`, `Cmd+S` no macOS) grava todas as pendências de uma
  vez; **Descartar** volta ao que o arquivo diz. Os dois ficam esmaecidos
  enquanto não há nada pendente.
- Valor recusado — fora da faixa, número malformado, nome de variável vazio
  ou repetido — é marcado na própria linha, com a razão, e **Salvar** fica
  indisponível até ele ser corrigido.
- Fechar a janela com pendências abre um diálogo com três saídas: salvar e
  fechar, descartar e fechar, ou cancelar (o foco inicial é o cancelar).
- Depois de salvar, a recarga a quente aplica a mudança em todas as janelas
  de terminal, como se você tivesse gravado o arquivo num editor.

Salvar mexe **só nas chaves que você alterou**. Comentários, linhas em
branco, a ordem das chaves e das tabelas, chaves desconhecidas, tabelas que a
tela não mostra (os tokens de `[appearance.*]`, os atalhos de
outras plataformas) e o final de linha do arquivo (CRLF ou LF) ficam como
estavam: num arquivo versionado, o `git diff` mostra apenas as linhas que você
mexeu. Chave que ainda não estava no arquivo entra na tabela dela, que é
criada se não existir. O texto novo é relido como configuração **antes** de ir
ao disco; se o app o recusaria, nada é gravado e um aviso diz o porquê.

### Restaurar o padrão

Cada opção tem um botão de **restaurar padrão** (a seta circular), visível
sob o cursor e na linha com o foco. Restaurar é uma pendência como outra: ao
salvar, a chave **sai do arquivo** em vez de ser regravada com o valor
padrão, e o arquivo volta a acompanhar o padrão de versões futuras. O
comentário que estava acima da chave fica.

### Teclado dentro da janela

`Tab` e `Shift+Tab` percorrem a lista de grupos, as opções e os botões do
rodapé; as setas movem dentro da lista de grupos e entre valores de uma
escolha; `Espaço` alterna; `Enter` confirma um campo; `Alt+↑` e `Alt+↓`
reordenam o item de uma lista em edição; `Esc` fecha a janela quando nenhum
campo, lista ou captura de atalho o está usando. Essas teclas valem só ali:
não são ações do catálogo, e o mapa de `[keybindings]` não é consultado na
janela de configurações.

### Atalhos

O grupo **Atalhos** lista as ações agrupadas por domínio (`tab.*`, `group.*`,
`window.*`, ...) com o atalho **em vigor na sua plataforma** — o que resulta
da resolução em três níveis, não só o que está no arquivo. Ação sem atalho
mostra "No shortcut". O campo no topo filtra pelo nome da ação ou pela tecla.

- **Clicar no atalho** (ou `Enter` com ele focado) entra em captura: a
  próxima combinação vira o atalho novo. `Esc` cancela; `Backspace` remove o
  atalho. Modificador sozinho, tecla morta e composição de IME são ignorados.
- Se a combinação já é de outra ação, a linha mostra "Already used by …" e
  oferece **Substituir** (a outra perde o atalho) ou **Cancelar**. Nenhum
  atalho fica com duas ações.
- Combinação `Ctrl+<letra>` sozinha, no Windows e no Linux, é aceita com a
  advertência de que o terminal deixa de recebê-la.
- **Restaurar padrão** remove do arquivo o que a tela escreveu para aquela
  ação.

As mudanças vão para a tabela da **plataforma em uso** — `[keybindings.windows]`,
`[keybindings.linux]` ou `[keybindings.macos]` —, para nunca alterar o que
vale nas outras plataformas de quem divide o arquivo entre máquinas. Para dar
um segundo atalho a uma ação que já tem um, edite o arquivo: clicar no atalho
existente o substitui.

### O que fica só no arquivo

A tela mostra um catálogo curado, não o arquivo inteiro. Ficam só no arquivo:
as dimensões, raios e espaçamentos de `[appearance.*]` (tokens de
design), criar, editar ou apagar temas (os arquivos `thm_*.toml` — a tela só
**escolhe** qual vale), os atalhos comuns de `[keybindings]` e os de outras plataformas, e
qualquer chave que o catálogo não cobre. O botão **Open file in editor**, no
rodapé, abre o arquivo no editor do sistema (e o cria a partir do exemplo, se
ele não existir). Também não existem pré-visualização antes de salvar nem
desfazer depois: o arquivo é seu e pode estar versionado.

### Quando o arquivo não está como a tela espera

- **Arquivo inexistente.** A tela abre normalmente, mostrando os padrões. O
  primeiro Salvar cria o arquivo **a partir do exemplo embutido** — o mesmo
  que "Open file in editor" cria — e aplica as pendências sobre ele, para você
  encontrar depois a documentação de cada chave.
- **Arquivo inválido** (sintaxe quebrada, tipo errado). A tela abre **somente
  leitura**, com uma faixa no topo do painel com o erro (linha e coluna, quando
  houver) e o botão para abrir o arquivo. Os controles mostram os valores em
  vigor e não podem ser alterados; corrigido o arquivo, a tela volta ao normal
  sozinha.
- **Arquivo alterado fora da tela** (editor externo, `git pull` nos seus
  dotfiles). Sem pendências, a tela passa a mostrar o arquivo novo, em
  silêncio. Com pendências, uma faixa avisa que o arquivo mudou e oferece
  **Reload** (descarta as pendências e mostra o arquivo novo) e **Keep my
  changes** (o próximo Salvar aplica as pendências sobre o arquivo novo, chave
  a chave, sem desfazer o que mudou fora). Salvar com a faixa à vista equivale
  a manter. A gravação da própria tela nunca dispara a faixa.

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
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | `Cmd++` / `Cmd+-` / `Cmd+0` | Zoom da fonte do terminal (aumentar/diminuir/resetar); a barra de abas não muda |
| `Ctrl+Shift+Y` | `Cmd+Y` | Alternar tema |
| `Ctrl+Shift+O` | `Cmd+,` | Abrir a tela de configurações (`settings.open`) |
| `Ctrl+Shift+,` | `Cmd+Shift+,` | Recarregar config na hora |

Todo atalho é reconfigurável pela [tela de configurações](#tela-de-configurações),
no grupo Atalhos, ou na seção `[keybindings]` do arquivo (e
`[keybindings.macos]`/`[keybindings.linux]`/`[keybindings.windows]` para
desvio por plataforma) — [`porecatu.example.toml`](config/porecatu.example.toml)
tem a tabela inteira, comentada.

## Perfis de aba

Um **perfil** é uma forma de abrir uma aba: um programa, seus argumentos e o
que mais ele precisar. Quem usa um shell só não precisa de nada disso — sem
perfil declarado, o `+`, os atalhos e a barra se comportam como sempre, e
nenhum badge aparece. O `[shell]` do arquivo de configuração continua
sendo o perfil padrão.

### Declarar perfis

Cada perfil é um `[[profiles.list]]`; `[profiles] default` diz qual abre
quando você clica no `+` ou usa `Ctrl+Shift+T`:

```toml
[profiles]
default = "pwsh"     # o "id" de um perfil; vazio é o [shell]
detect  = true       # procura shells na máquina (veja abaixo)

[[profiles.list]]
id      = "pwsh"
name    = "PowerShell 7"
badge   = "PS"       # até 4 caracteres, aparece na aba
program = "pwsh.exe"

[[profiles.list]]
id      = "ubuntu"
name    = "Ubuntu"
badge   = "WSL"
program = "wsl.exe"
args    = ["-d", "Ubuntu"]
cwd     = "~/projetos"            # opcional: diretório inicial
env     = { EDITOR = "nvim" }     # opcional: soma-se a [shell.env]
```

`id` é letras minúsculas, números, `-` e `_`, e não se repete. Um perfil com
erro (id repetido, `program` vazio, `type` desconhecido) é descartado e os
outros continuam valendo; o aviso diz a linha e a coluna.
Um servidor SSH é um perfil com `type = "ssh"`, descrito na seção "Perfis SSH" abaixo.

> **`env` não é lugar de segredo.** O `porecatu.toml` é texto puro, costuma
> ir para repositório de dotfiles, e o ambiente de um processo é legível
> por quem roda na mesma conta. Senhas e tokens ficam no cofre do sistema
> ou no próprio programa.

Mudar `[profiles]` com o app aberto vale para **abas novas**; as abertas
seguem como estão.

### Perfis SSH

Um perfil `type = "ssh"` abre uma aba que fala com um servidor, pelo cliente SSH embutido — sem o programa `ssh` do sistema:

```toml
[[profiles.list]]
id            = "prod"
name          = "prod-web-01"
type          = "ssh"
host          = "10.4.1.20"
port          = 22                       # opcional
user          = "deploy"                 # obrigatório
auth          = ["agent", "key"]         # opcional: a ordem tentada
identity_file = "~/.ssh/id_ed25519"      # opcional; "~" é a sua pasta pessoal
```

- `host` e `user` são obrigatórios e não aceitam espaço nem os caracteres `, | [ ] @ # * ? !` (e `user` também não aceita `@` nem `:`): eles viram uma linha do `known_hosts`. Um endereço IPv6 se escreve sem colchetes.
- `program`, `args`, `env` e `cwd` **não valem** num perfil SSH e são erro: o perfil é ignorado, com um aviso que diz a linha.
- `auth` é a ordem em que os métodos são tentados (`agent`, `key`, `password`), parando no primeiro aceito; um método fora da lista não é tentado. Sem `auth`, a ordem é agente, depois a chave **se** houver `identity_file`, depois senha. `auth = ["key"]` sem `identity_file` é erro. **Nesta versão** a autenticação funciona por agente e por chave **sem** passphrase; a senha e a passphrase ainda não estão disponíveis.
- O `badge` é `SSH` se você não escrever outro.
- **Senha e passphrase não vão no `porecatu.toml`.** Uma chave de perfil com `password`, `passphrase` ou `secret` no nome é ignorada, com um aviso: o valor nem é lido.

**Primeira conexão.** O Porecatu confere a chave do servidor no `~/.ssh/known_hosts`, o mesmo arquivo do OpenSSH: o que você já confia lá vale aqui, e o contrário. Com um host que o arquivo não conhece, abre um diálogo com o tipo da chave e a impressão digital `SHA256:…`; **Cancelar** é o foco inicial. **Confiar e conectar** acrescenta uma linha ao fim do arquivo (em hash, se o seu arquivo já usa hash) e conecta. Se a chave do host **mudou**, ou foi marcada `@revoked`, a conexão é bloqueada com uma nota que diz a impressão digital apresentada e o arquivo e a linha da entrada — o Porecatu nunca oferece "substituir": quem resolve é o seu editor, ou o `ssh-keygen -R`. Se o `known_hosts` existe e não pode ser lido (por exemplo, regravado em UTF-16 pelo PowerShell 5), a conexão nem é tentada.

Se a pergunta chega com a aba em segundo plano, o diálogo **não rouba o foco**: a campainha da aba acende, e o diálogo abre quando você chega ao painel.

O estado da conexão (conectando, falhou, caiu) aparece como nota no próprio terminal, abaixo do conteúdo. Dividir um painel SSH abre uma conexão nova.

**O rótulo e a barra de status.** Sem título seu e enquanto o servidor não manda um, a aba se chama como o perfil (`prod-web-01`). A barra de status mostra `usuário@host` no lugar do nome do shell. A pasta aparece só se o shell do servidor informá-la (OSC 7, como no [convite de integração de shell](reference/integracao-de-shell.md)) **com o mesmo nome de host do perfil**: um perfil escrito com o IP e um shell que diz o nome da máquina não mostram pasta. Os segmentos de Git não aparecem — o repositório que o Porecatu enxerga é o local.

**Fechar.** Fechar uma aba, um painel ou a janela com uma conexão SSH **aberta** (conectando, autenticando ou conectada) pede confirmação, atrás de `general.confirm_close_with_process`, como um programa em primeiro plano. Cada painel conta por si: o painel cujo shell remoto já saiu fecha sem perguntar. Aba já encerrada, ou que nunca conectou, também. Quando o servidor encerra a sessão, vale o de um shell local: `exit` (código 0) fecha a aba; qualquer outro código, ou um sinal, deixa a aba aberta com a nota; uma conexão que falhou ou caiu fica aberta com a causa.

**Reabrir o app.** Todo painel SSH restaurado — ao abrir o app ou ao abrir uma sessão nomeada — volta **esmaecido, sem conectar**, com a nota "Enter para conectar a `nome`", **mesmo com `lazy_restore = false` e mesmo sendo o painel ativo**. Ele conecta no primeiro **clique** dentro dele ou no primeiro **Enter** com ele focado; trocar de aba, `Ctrl+Tab` e `tab.goto_N` só o mostram. Abrir o app nunca pede senha nem abre o diálogo de chave de host sozinho. Painéis locais voltam como sempre.

**Pastas.** `tab.new` numa aba SSH abre o perfil padrão na pasta de início (`startup_directory`), nunca numa pasta do servidor; o `.porecatu` nunca roda numa aba SSH; a sessão grava o perfil do painel e **nada da conexão** — nem pasta, nem segredo.

### Escolher o perfil

- **Clique direito no `+`** (de um grupo, ou o de abas soltas) abre o menu
  de perfis: o padrão primeiro, os declarados, o `[shell]` quando não é o
  padrão, os detectados e, no fim, "Novo grupo de abas". Escolher uma linha
  abre a aba **naquele grupo**. O **clique esquerdo** no `+` não mudou: abre
  o padrão, sem menu.
- **`profile.open_menu`** abre o mesmo menu no `+` do grupo da aba ativa.
- **`profile.new_tab_1` … `profile.new_tab_9`** abrem o N-ésimo perfil
  **declarado** (na ordem do arquivo) no grupo da aba ativa; sem N-ésimo,
  não fazem nada. O índice nunca conta os detectados, então um atalho não
  muda de alvo porque a máquina ganhou um shell.

Nenhuma dessas ações tem tecla de fábrica. Para ligar uma, no
`porecatu.toml`:

```toml
[keybindings]
"ctrl+shift+m" = "profile.open_menu"
"ctrl+f1"      = "profile.new_tab_1"
```

O menu mostra, à direita de cada linha, a tecla que você ligou.

### O badge na aba

A aba mostra o **badge** do perfil, na cor do grupo, **só quando o perfil
dela não é o padrão** — e nunca para o `[shell]`. Ele consome espaço do
título, e a aba não muda de largura. Numa aba com atividade, a ordem é
badge, ponto, título. `show_profile_badge = false` em `[appearance.tabs]`
esconde o badge em todas as abas.

### Shells detectados

Na **primeira vez que o menu abre** num processo, e com `detect = true`, o
app procura os shells da máquina — `pwsh`, `powershell` e `cmd` no Windows,
mais as distribuições do WSL; o que estiver em `/etc/shells` no Linux e no
macOS — e os acrescenta ao menu quando a busca termina, sem fechá-lo nem
mover a linha realçada. Passados 3 segundos a busca desiste em silêncio. O
que repete uma linha acima (mesmo programa e argumentos) não aparece de
novo. Os detectados não têm atalho nem entram na conta de
`profile.new_tab_N`. **`detect = false` desliga a busca por inteiro**: nada
é lançado, nem o `wsl.exe`.

### O que a sessão grava

O **painel** guarda o identificador do perfil com que nasceu, não o
comando. Ao reabrir, a aba volta com o mesmo perfil, mesmo que o padrão
tenha mudado; dividir um painel abre outro **do mesmo perfil**. Um
`profile` que não existe mais na configuração abre o perfil padrão, com uma
nota na primeira linha do painel. Para os detectados o identificador é
`auto:shell:pwsh.exe`, `auto:shell:/bin/zsh` ou `auto:wsl:Ubuntu`, e é
**revalidado** ao restaurar: o Windows só aceita `pwsh.exe`,
`powershell.exe` e `cmd.exe`; Linux e macOS, um caminho que esteja em
`/etc/shells`; para o WSL, um nome de distribuição de letras, números,
`.`, `_` e `-`. Um arquivo de sessão copiado ou editado à mão não consegue
pedir outro executável.

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

## Imagem de fundo

Uma imagem **PNG** ou **JPEG** atrás do texto de todo terminal: aba solta,
aba de grupo e cada painel de uma aba dividida. Cada painel mostra a imagem
**inteira** no próprio quadro (os painéis não dividem uma imagem só entre
eles), e o vão entre dois painéis fica sem imagem. Ela aparece também pela
tela de configurações, no grupo **Terminal**, logo depois da opacidade do
fundo — mas o arquivo é onde tudo está:

```toml
[terminal.background_image]
path = "imagens/montanha.jpg"   # vazio = sem imagem (o padrão)
mode = "stretch"                # "stretch", "tile" ou "center"
opacity = 0.35                  # 0.0 a 1.0
```

**O caminho.** `path` vazio desliga a imagem, e o terminal fica exatamente
como sempre foi. O formato é reconhecido pelo **conteúdo** do arquivo, não
pela extensão: um `.jpg` que por dentro é PNG funciona. Qualquer outro
formato (GIF, WebP, BMP, SVG) é recusado com um aviso. O canal de
transparência de um PNG é respeitado: onde a imagem é transparente, aparece o
fundo do terminal.

- **Caminho relativo** vale a partir da pasta do **`porecatu.toml` em uso**
  (o do `--config`, o de `PORECATU_CONFIG` ou o padrão da plataforma), não da
  pasta de onde o app foi aberto. É a única chave do arquivo com essa regra:
  `startup_directory` e `[project_file] trusted_paths` seguem outra.
- **Caminho absoluto** vale como está.
- **`~/` ou `~\`** no começo é a sua pasta pessoal.

**Os modos.**

| `mode` | O que faz |
|---|---|
| `stretch` (padrão) | Estica a imagem até o quadro, nos dois eixos, **sem manter a proporção**: num quadro largo a imagem sai achatada. |
| `tile` | Repete a imagem lado a lado, no tamanho natural, a partir do canto superior esquerdo. O que passa da borda fica cortado. |
| `center` | Põe a imagem no tamanho natural, no meio. Menor que o quadro, sobra o fundo do terminal em volta; maior, é cortada nas bordas, mantendo o centro. |

**Tamanho natural** é um pixel da imagem para um pixel da tela. A imagem não
cresce com o zoom de fonte nem com a escala da janela (uma imagem de 200 px
ocupa 200 pixels físicos, em qualquer monitor).

**As duas opacidades se multiplicam.** A opacidade com que a imagem é
desenhada é `opacity` **vezes** `[terminal] background_opacity`: com o
terminal a `0.8` e a imagem a `0.5`, a imagem sai a `0.4`. Por isso um
terminal transparente nunca fica opaco por ter imagem, e o que está atrás do
quadro continua aparecendo através dele: o desktop, ou a imagem da janela, se
houver uma. A imagem também não faz a janela nascer transparente: isso
continua sendo decidido por `background_opacity` e por
`[appearance.window] opacity`, e com imagem da janela configurada só por esta
última.

**Legibilidade.** Imagem forte atrás do texto cansa a leitura, e `opacity` é o
controle que a devolve: comece **baixo**, entre `0.15` e `0.4`, e suba até a
imagem aparecer sem competir com o texto. Fotos com muito contraste pedem
valores mais baixos que ilustrações lisas.

**O que cobre a imagem.** Uma célula com cor de fundo própria, posta pelo
programa (ANSI, 256 cores ou true color), **cobre** a imagem, como cobre o
fundo do terminal; um `htop` ou um editor com tema que pintam o próprio
fundo escondem a imagem onde pintam. As células sem cor própria a deixam
aparecer. A seleção, o realce de busca, o sublinhado de hyperlink e o cursor
ficam por cima da imagem.

**Aplicação.** Mudar `path`, `mode` ou `opacity` vale na hora, em todas as
janelas, sem reiniciar e sem redimensionar terminal nenhum. O app **não
vigia** o arquivo da imagem: trocar o conteúdo dele, mantendo o caminho, só
vale na próxima recarga da configuração (salvar o `porecatu.toml`, ou
`config.reload`).

**Quando algo dá errado.** Arquivo inexistente, ilegível, de formato não
aceito, corrompido ou grande demais para carregar **não derruba nada**: o
terminal abre sem imagem e um aviso diz qual arquivo e por quê. O aviso
aparece uma vez por arquivo e por problema, não por janela nem a cada
recarga. Na tela de configurações, o campo do caminho mostra uma nota quando o
arquivo não existe; ela não impede o Salvar, e os outros problemas (formato,
arquivo corrompido) só aparecem no aviso, depois da recarga, porque descobri-los
exige abrir a imagem. Uma imagem maior que a textura que a placa de vídeo
aceita é **reduzida**, mantendo a proporção, sem aviso.

A imagem é lida **fora** da thread da interface: o app abre e o terminal
funciona sem ela, e ela aparece quando termina de carregar. Imagens muito
grandes (dezenas de megapixels) demoram um pouco mais para aparecer, e é só
isso que custam ao arranque.

### Imagem de fundo da janela

Uma segunda imagem, **PNG** ou **JPEG**, atrás da **janela inteira**: da barra de abas, da barra de status, da margem em volta dos terminais e do vão entre painéis. É uma imagem só, não uma por painel, e fica **abaixo** dos quadros de terminal. Ela aparece também pela tela de configurações, no grupo **Aparência**, logo depois de `window_opacity`.

```toml
[appearance.window.background_image]
path = "imagens/praia.jpg"   # vazio = sem imagem (o padrão)
mode = "stretch"             # "stretch", "tile" ou "center"
opacity = 1.0                # 0.0 a 1.0
```

`path` sozinho já desenha a imagem na janela inteira. O caminho, o formato, os três modos e o tamanho natural (um pixel da imagem por pixel da tela) são **os mesmos** da imagem do terminal, e valem para a janela como um todo: `stretch` a estica até a janela inteira sem manter a proporção, `tile` a repete a partir do canto superior esquerdo da janela, e `center` a põe no meio da janela, com o fundo da janela em volta. Maximizar ou redimensionar a janela refaz a conta sem recarregar nada do disco.

**Onde aparece.** Atrás das abas, das cápsulas e das pílulas de grupo, que continuam translúcidas e deixam a imagem transparecer na medida dos alfas que já têm. Com a imagem exibida, a barra de abas **deixa de pintar o fundo** dela, e por isso a sombra do quadro do terminal passa a aparecer na faixa de baixo da barra. Sem imagem, nada disso muda. A **janela de configurações não recebe** a imagem, nem os avisos, os menus, os tooltips e os diálogos.

**Através do terminal translúcido.** Com `[terminal] background_opacity` abaixo de `1.0`, a imagem da janela aparece **dentro** do quadro do terminal, por baixo do fundo dele e da imagem do terminal. Com a imagem da janela configurada, o terminal translúcido mostra a imagem, **nunca o desktop**, em qualquer janela: com a imagem mais transparente, o que aparece por ela é o fundo da janela. Para ver o que está atrás da janela, use `[appearance.window] opacity` (abaixo). Com o terminal opaco, o quadro cobre a imagem e ela só se vê fora dele.

**As duas imagens juntas.** Se as duas chaves estão configuradas, dentro do quadro a imagem do terminal fica **por cima** do fundo do quadro e a da janela **por baixo** dele; fora dos quadros só aparece a da janela. Se as duas apontam para **o mesmo arquivo**, ele é lido e guardado **uma vez só**.

**As opacidades.** O alfa da imagem da janela é só o `opacity` dela. Quem multiplica é a opacidade da janela: com `[appearance.window] opacity = 0.8` e a imagem a `1.0`, a imagem sai a 80% e o que está atrás da janela continua aparecendo. É diferente da imagem do terminal, que multiplica por `background_opacity`.

**O que cobre a imagem da janela.** Uma célula com cor de fundo própria, posta pelo programa, se mistura sobre a imagem da janela com a transparência de `background_opacity`, como o fundo do terminal, e cobre a imagem do terminal. As células sem cor própria deixam a imagem aparecer.

**Aplicação e erros.** Mudar `path`, `mode` ou `opacity` vale na hora, em todas as janelas, sem reiniciar e sem redimensionar terminal nenhum. Arquivo inexistente, ilegível, de formato não aceito, corrompido ou grande demais **não derruba nada**: a janela abre sem imagem e um aviso diz qual arquivo e por quê, **"da janela"** ou **"do terminal"**. O mesmo arquivo ausente nas duas chaves dá dois avisos, um de cada, porque são duas configurações erradas. Na tela de configurações, o campo do caminho mostra uma nota quando o arquivo não existe, e ela não impede o Salvar.

**Legibilidade.** Uma imagem forte atrás das abas e da barra de status pode atrapalhar a leitura; `opacity` é o controle. Comece com `0.3` a `0.6` e suba até a imagem aparecer sem competir com o texto.

## Efeito acrílico

O efeito acrílico mostra a **imagem de fundo da janela desfocada** através do quadro do terminal, tingida pela cor de fundo dele. Fora do quadro — na barra de abas, na barra de status, na margem e no vão entre painéis — a imagem continua **nítida**, e a imagem de fundo do terminal, se houver, fica nítida **por cima** do desfoque. O texto sai nítido como sempre. É o que deixa uma foto de bordas fortes ou de muito contraste servir de fundo sem competir com o que está na tela.

```toml
[terminal.acrylic]
enabled = true   # false = desligado (o padrão)
blur = 24        # raio do desfoque, de 1 a 64 pixels (o padrão é 24)
```

**Duas condições.** O efeito só vale com as duas juntas:

1. `[appearance.window] opacity = 1.0` — a janela **opaca**. Com a janela translúcida, o que aparece atrás dela é o desktop, que o app não tem como desfocar; o efeito sairia metade desfocado, metade nítido.
2. `[appearance.window.background_image] path` **preenchido** — sem imagem só há uma cor lisa, e desfocar uma cor lisa não muda nada.

Faltando uma delas, o app **desenha sem o efeito** e mostra um aviso que diz qual condição falta ("opacidade da janela diferente de 100%" ou "sem imagem de fundo da janela"); se faltam as duas, o aviso fala da opacidade. O aviso sai uma vez por vez que a configuração cria o problema, não a cada quadro. A imagem ainda carregando, ou que falhou, não conta como falta: o efeito já vale e passa a mostrar a imagem desfocada quando ela chega (o aviso da falha da imagem é o da própria imagem).

**Quanto aparece depende de `[terminal] background_opacity`.** O fundo do quadro faz o papel de tingimento: com `background_opacity = 0.7` o efeito aparece bem; com `1.0`, o fundo é opaco e **cobre** o efeito, como cobre a imagem nítida. A opção não fica bloqueada por isso — baixe `background_opacity` para ver o desfoque.

**O raio.** `blur` é em pixels **lógicos**, então o mesmo valor desfoca o mesmo tanto em 100% e em 150% de escala. Fora de `1` a `64` o valor é limitado ao extremo mais próximo e a recarga avisa. O desfoque acompanha a imagem da janela: o mesmo `mode` e a mesma `opacity` dela valem, e maximizar ou redimensionar a janela o refaz sem recarregar a imagem do disco.

**É só desfoque e tingimento.** Não há ruído, camada de luminosidade nem aumento de saturação, como no acrílico do Windows. O acrílico do sistema operacional também não é usado: o efeito é desenhado pelo app, sobre a imagem da janela, e não desfoca o desktop.

**Tela de configurações.** As duas opções estão no grupo **Aparência**, logo depois das três da imagem de fundo da janela: **Efeito acrílico** (alternância) e **Desfoque do acrílico** (número de 1 a 64). Sem as duas condições, as **duas linhas ficam esmaecidas e sem edição**, com uma nota abaixo da descrição — "Exige a opacidade da janela em 100%" ou "Exige uma imagem de fundo da janela". A conta usa os valores ainda **não salvos** da própria tela: baixar a opacidade da janela para `0.9` esmaece as linhas na hora, e voltá-la a `1.0` as devolve, antes de Salvar. Esmaecer não apaga nada: o valor que está no arquivo continua lá, e o Salvar grava as outras mudanças sem tocar nas chaves do acrílico.

**Aplicação.** Mudar `enabled` ou `blur` vale **na hora**, em todas as janelas, sem reiniciar e sem redimensionar terminal nenhum, e o mesmo vale quando uma das condições passa a valer ou deixa de valer por recarga. Com o acrílico ligado e nada mudando, nenhum quadro é desenhado: o terminal ocioso continua custando zero. A janela de configurações, os avisos, os menus, o tooltip e os diálogos **não têm** o efeito.

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
grade cresce de volta, na hora, sem reiniciar. Altura e tamanho da
fonte também são configuráveis; ver `[appearance.status_bar]` no
[arquivo de exemplo](config/porecatu.example.toml). As cores são do tema
(`[status_bar]` em `thm_*.toml`).

A única coisa clicável na barra é o número de commits atrás do remoto
(acima). Em todo o resto dela, a borda inferior da janela continua sendo
a área de redimensionar, mesmo em cima da barra — inclusive nos cantos,
que o número nunca alcança.

## Quando o app fecha sozinho

Se o Porecatu fechar de repente, a causa fica registrada em `crash.log`, ao
lado do `session.json`: no Windows, `%LOCALAPPDATA%\porecatu\crash.log`; no
Linux, `$XDG_STATE_HOME/porecatu/crash.log`; no macOS,
`~/Library/Application Support/porecatu/crash.log`. Cada fechamento acrescenta
uma entrada, em inglês, com a mensagem, o ponto do código e o backtrace. É
esse arquivo que vale anexar ao relatar o problema.
