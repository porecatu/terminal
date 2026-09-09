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
| `Ctrl+Shift+N` | `Cmd+N` | Nova janela |
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

## Barra de status

A faixa no rodapé da janela mostra, da aba ativa:

| Zona | O que aparece |
|---|---|
| Esquerda | nome do shell (o único item colorido), diretório atual, branch do Git, grupo da aba |
| Direita | codificação e o sistema |

O diretório aparece com `~` no lugar da sua pasta pessoal, e é cortado à
direita quando a janela estreita — os outros campos são curtos e não
cedem espaço.

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

A barra mostra **só a branch**: se há alterações não commitadas, quantos
commits você está à frente ou atrás do remoto, o que está em stage — nada
disso aparece. Saber a branch é ler um arquivo de trinta bytes; saber o
resto é percorrer a árvore inteira, e o Porecatu não faz isso enquanto
você digita.

O nome da branch vem do diretório que a barra conhece — então, se o
diretório estiver no tom apagado (acima), a branch pode ser a de outro
repositório. A integração de shell resolve as duas coisas de uma vez.

### Desligar

```toml
[appearance.status_bar]
enabled = false
```

Desligada, a barra devolve a altura ao terminal — o número de linhas da
grade cresce de volta, na hora, sem reiniciar. Cores, altura e tamanho da
fonte também são configuráveis; ver `[appearance.status_bar]` no
[arquivo de exemplo](config/porecatu.example.toml).

A barra não tem nada clicável: a borda inferior da janela continua sendo
a área de redimensionar, mesmo em cima dela.
