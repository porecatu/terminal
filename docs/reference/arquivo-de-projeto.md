# Arquivo de projeto — `.porecatu`

Referência do formato do arquivo `.porecatu`: o que ele é, como se escreve, e o
que é preciso para que ele rode. Requisito em
[PRD-012](../prd/prd-012-comando-de-projeto-por-diretorio.md), decisões em
[ADR-0051](../adr/0051-arquivo-de-projeto-porecatu.md).

> **Ainda não implementado.** O formato e o comportamento descritos aqui estão
> decididos, mas o binário ainda não lê o `.porecatu`, e a seção
> `[project_file]` está comentada no
> [arquivo de exemplo](../config/porecatu.example.toml) porque as chaves ainda
> não existem em `Config`. Ver [docs/roadmap.md](../roadmap.md).

> Diferente de [integracao-de-shell.md](integracao-de-shell.md), este arquivo
> **não** é embutido no binário: o app não mostra nada daqui na tela. É
> documentação para quem escreve um `.porecatu`.

## Por que isso existe

O Porecatu restaura cada aba no diretório em que ela estava
([PRD-003](../prd/prd-003-persistencia-de-sessao.md)) — e para aí. O que estava
rodando naquela aba, o usuário digita de novo, toda vez que reabre o app.

O `.porecatu` é onde o **projeto** declara como se levanta. Ele fica na raiz do
projeto, entra no controle de versão junto com ele, e tem uma seção por tipo de
terminal, porque o mesmo projeto é aberto em `pwsh` numa máquina e em `bash` na
outra.

**Ele roda só quando uma aba é restaurada de uma sessão gravada.** Abrir uma aba
nova naquele diretório não roda nada; dar `cd` para ele também não.

## A forma do arquivo

```
Este preâmbulo é ignorado: tudo antes da primeira seção é comentário do arquivo.

[pwsh]
npm run dev

[bash]
nvm use
npm run dev

[default]
echo Sem script para este shell.
```

As regras, todas:

- Uma linha da forma `[nome]`, sozinha, abre uma seção.
- **Todo o resto é o script**, literal, linha por linha, até a próxima seção ou
  o fim do arquivo. O que estiver escrito é o que o shell recebe.
- Linhas **antes da primeira seção** são ignoradas. É onde vai o comentário
  sobre o arquivo.
- **Não há sintaxe de comentário dentro de uma seção.** Um `#` ali é do script,
  e vai para o shell — que pode ou não tratá-lo como comentário. Em `cmd.exe`,
  por exemplo, `#` não comenta nada; use `rem`.
- **Não há escape.** Uma linha de script que seja literalmente `[algo]` não é
  expressável, porque seria lida como cabeçalho de seção. É a única limitação do
  formato.
- Linhas em branco no meio de uma seção são preservadas e enviadas ao shell; no
  fim de uma seção, são cortadas.
- O arquivo precisa ser UTF-8.
- Duas seções com o mesmo nome: a **primeira** vence, a segunda é ignorada.

## O nome da seção

É o nome do shell da aba, **sem caminho e sem extensão**, em minúsculas — o
mesmo que aparece na barra de status. `C:\Program Files\PowerShell\7\pwsh.exe`
vira `pwsh`; `/bin/zsh` vira `zsh`.

| Shell | Seção |
|---|---|
| PowerShell 7 | `[pwsh]` |
| Windows PowerShell 5.1 | `[powershell]` |
| Prompt de Comando | `[cmd]` |
| bash | `[bash]` |
| zsh | `[zsh]` |
| fish | `[fish]` |
| sh | `[sh]` |
| qualquer um, como último recurso | `[default]` |

O casamento é **exato** (sem diferenciar maiúsculas de minúsculas). `pwsh`
**não** cai em `[powershell]`, e `bash` **não** cai em `[sh]` — os dois pares
parecem intercambiáveis e não são: `&&` e `??` do PowerShell 7 não existem no
5.1, e um `bash` com o `nvm` carregado pelo `.bashrc` não é um `sh` puro.

Se não houver seção para o seu shell, vale `[default]`. Se não houver nem uma
nem outra, **nada acontece e nada é avisado** — o arquivo declara os shells que
suporta, e não suportar o seu não é erro.

## Autorizar o diretório

Um `.porecatu` só roda em diretório que você declarou confiável na **sua**
configuração — não no projeto. Em [`porecatu.toml`](../config/porecatu.example.toml):

```toml
[project_file]
trusted_paths = ["C:/Projetos"]
```

Um caminho listado cobre a árvore inteira sob ele: `C:/Projetos` autoriza
`C:/Projetos/api`, `C:/Projetos/web/admin` e todo o resto.

**A lista é vazia por padrão, e isso é deliberado.** O `.porecatu` chega junto
com o projeto, e projeto clonado é arquivo escrito por outra pessoa. Sem essa
lista, clonar um repositório qualquer e abrir uma aba nele bastaria para executar
o que estivesse no arquivo.

Pelo mesmo motivo, **pense antes de listar um caminho largo.** Se você aponta
para o diretório onde clona repositórios — ou para o seu home —, todo repositório
novo ali dentro passa a rodar o `.porecatu` dele sozinho, meses depois de você
ter escrito a linha. Prefira listar o diretório onde ficam os projetos que são
seus.

Quando o app encontra um `.porecatu` num diretório **não** autorizado, ele não
executa nada e escreve uma nota na aba, com o caminho encontrado e a linha a
acrescentar. A nota aparece uma vez por execução do app, não uma por aba.

Para desligar o recurso inteiro, inclusive essa nota:

```toml
[project_file]
enabled = false
```

## Exemplos

### Projeto Node, dois shells

```
[pwsh]
npm run dev

[bash]
nvm use
npm run dev
```

### Projeto com dois passos e uma variável de ambiente

```
[bash]
export DATABASE_URL=postgres://localhost/app_dev
docker compose up -d db
npm run dev
```

```
[pwsh]
$env:DATABASE_URL = 'postgres://localhost/app_dev'
docker compose up -d db
npm run dev
```

### Só informar, sem executar nada pesado

Nem todo `.porecatu` precisa subir um servidor. Lembrar o comando já resolve
metade do problema:

```
[default]
echo "Este projeto sobe com: make dev"
```

### `cmd.exe`

```
[cmd]
rem o rem e o comentario do cmd; o # nao e
set DATABASE_URL=postgres://localhost/app_dev
npm run dev
```

## O que **não** acontece

- **Não roda em aba nova.** `Ctrl+T` no mesmo diretório não dispara nada.
- **Não roda no `cd`.** Entrar no diretório com um `cd` não dispara nada.
- **Não sobe a árvore de diretórios.** Um `.porecatu` na raiz do projeto não
  alcança uma aba restaurada em `src/`. Quem quer o script nos dois lugares põe
  o arquivo nos dois.
- **Não roda se o diretório sumiu.** Se o diretório gravado não existe mais e a
  aba abre no seu home, nenhum `.porecatu` é procurado.
- **Não roda duas vezes.** No máximo uma execução por aba, por execução do app.
  Sair da aba e voltar não reexecuta.
- **Não roda escondido.** O script é escrito no terminal como se você o tivesse
  digitado: aparece na tela, entra no histórico do shell, e `Ctrl+C` o
  interrompe como qualquer outro comando.

## Conferindo

1. Crie o `.porecatu` na raiz do projeto, com a seção do seu shell.
2. Acrescente o diretório (ou um diretório acima dele) em `trusted_paths`, e
   reinicie o Porecatu — `[project_file]` só vale a partir da próxima execução.
3. Abra uma aba naquele diretório e **feche o app** com ela aberta, para que a
   sessão seja gravada.
4. Reabra. O comando deve aparecer escrito no prompt daquela aba e executar.

Se não acontecer nada, confira nesta ordem:

- **O nome da seção bate com o shell?** O nome do shell da aba aparece na barra
  de status, no canto esquerdo. Se ali está `powershell` e o arquivo tem
  `[pwsh]`, não casa.
- **O diretório é o que você acha que é?** No Windows, sem integração de shell,
  a aba é restaurada no diretório em que foi **aberta**, não naquele para onde
  você navegou depois — e é lá que o `.porecatu` é procurado. Ver
  [integracao-de-shell.md](integracao-de-shell.md).
- **Apareceu uma nota dizendo que o arquivo não está autorizado?** Então o
  caminho em `trusted_paths` não cobre o diretório. Confira barras e letra de
  unidade.
