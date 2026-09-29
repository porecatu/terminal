# ADR-0057 — Idiomas nos artefatos de release: `locales/` dentro de todo instalador, e o binário cru deixa de ser publicado

**Status:** Aceito
**Data:** 2026-09-29
**Relacionados:** [ADR-0010](0010-licenciamento.md), [ADR-0011](0011-toolchain-rust.md), [ADR-0044](0044-empacotamento-e-release.md), [ADR-0045](0045-primeira-versao-0-7-0.md), [ADR-0056](0056-catalogo-de-textos-da-interface.md), [PRD-011](../prd/prd-011-polimento.md), [PRD-015](../prd/prd-015-idioma-da-interface.md)
**Supersedes:** ADR-0044 §1 (**parcial**: só o parágrafo que mantinha o binário cru publicado ao lado dos instaladores; os quatro instaladores, a matriz e o `--locked` continuam valendo) · ADR-0044 §4 (**parcial**: a lista do que vai dentro de todo artefato ganha os arquivos de idioma; o resto da lista não muda)

## Contexto

O [ADR-0056](0056-catalogo-de-textos-da-interface.md) decidiu que o texto da interface vive **só em disco**, em `locales/<idioma>.toml`, e que o binário não embute cópia nenhuma. Com isso, um `porecatu.exe` sem a pasta ao lado abre com os identificadores das frases no lugar do texto, e um aviso fixo em inglês (ADR-0056 §8) — funciona, mas não é um produto.

Hoje o `release.yml` publica, por plataforma:

- os instaladores do [ADR-0044](0044-empacotamento-e-release.md) §1 — MSI no Windows, `.deb` e AppImage no Linux, `.app` dentro de `.dmg` no macOS;
- e o **binário cru**, com `LICENSE`, `README.md`, as duas atribuições de fonte e o `porecatu.example.toml` como arquivos soltos ao lado (passo *"Empacotar binário cru"*).

A publicação usa `softprops/action-gh-release` com `files: artefatos/**`, que sobe **arquivos**, um a um, sem estrutura de diretório: não há como uma pasta `locales/` viajar ao lado de um executável solto na página de release.

## Decisão

**Todo instalador leva `locales/` num caminho que o resolvedor do ADR-0056 §6 procura, e o CI verifica isso por artefato. O binário cru deixa de ser publicado.**

### 1. Onde os arquivos ficam em cada artefato

| Artefato | Executável | Arquivos de idioma | Mudança |
|---|---|---|---|
| MSI | `APPLICATIONFOLDER\bin\porecatu.exe` | `APPLICATIONFOLDER\bin\locales\{en_US,pt_BR}.toml` | dois `Component` novos em `wix/main.wxs`, com `Source` relativo à **raiz** do projeto (armadilha registrada da F6) |
| `.deb` | `/usr/bin/porecatu` | `/usr/share/porecatu/locales/` | duas linhas em `assets` de `[package.metadata.deb]` |
| AppImage | `$APPDIR/usr/bin/porecatu` | `$APPDIR/usr/share/porecatu/locales/` | um `cp` no passo do AppImage |
| `.app` | `Contents/MacOS/porecatu` | `Contents/Resources/locales/` | um `cp` no passo do bundle |

Cada caminho é o que o resolvedor acha a partir do executável: `<exe>/locales` no Windows, `<exe>/../share/porecatu/locales` no Linux, `<exe>/../Resources/locales` no macOS. No Linux o destino é `share/porecatu/`, **não** `/usr/share/locale/` — esse é o diretório de catálogos `.mo` do gettext, com outra estrutura e outro dono.

No `.app`, os arquivos vão **dentro** do bundle, não ao lado dele no `.dmg` como os acompanhantes do RF-11.21: o usuário arrasta só o `.app` para `Applications`, e o que ficar fora dele fica para trás.

A lista **não é escrita à mão** por idioma na receita de cada artefato quando a ferramenta aceita padrão: o `.deb`, o AppImage e o `.app` copiam `locales/*.toml`. O MSI, que exige um `Component` por arquivo, é o único que precisa de linha nova quando um idioma entra — e a verificação do §2 é o que pega o esquecimento.

### 2. Verificação por artefato no CI

No mesmo molde das verificações de atribuição de fonte que o `release.yml` já tem, um passo por artefato reprova se faltar `en_US.toml` ou `pt_BR.toml` no lugar da tabela do §1:

- MSI: extração administrativa (`msiexec /a`) e busca do arquivo na árvore extraída;
- `.deb`: listagem do conteúdo **capturada numa variável** e casada contra ela — nunca `dpkg-deb -c | grep -q`, a armadilha do `pipefail` que derrubou a `v0.7.5`;
- AppImage: teste de existência no `AppDir` antes do `appimagetool`;
- `.app`: teste de existência em `Contents/Resources/locales/` antes do `hdiutil`.

### 3. O binário cru deixa de ser publicado

É isto que revisa o ADR-0044 §1, que dizia *"o binário cru continua sendo publicado, ao lado dos instaladores"*. Sem os arquivos de idioma ao lado, ele deixou de ser um jeito de usar o app, e publicá-lo seria oferecer na página de release um download que abre sem texto.

O passo *"Empacotar binário cru"* e a verificação de atribuição correspondente saem do `release.yml`. Os acompanhantes soltos (`LICENSE`, `README.md`, as atribuições, o exemplo de config) saem junto: estavam lá para acompanhar o binário cru, e **todos continuam dentro de cada instalador**, que é o que o §4 do ADR-0044 exige. O `sha256` de cada artefato publicado continua.

Quem punha o binário num diretório do `PATH` à mão tem dois caminhos: o AppImage no Linux, que já é um executável único; e, em qualquer plataforma, apontar `PORECATU_LOCALES` para uma pasta com os arquivos — costura que existe para teste e desenvolvimento (ADR-0056 §6) e que não vira contrato por estar citada aqui.

**`cargo install` não é canal suportado**, pelo mesmo motivo: instala só o executável.

### 4. Sequência de entrega

Entre a etapa que tira as frases do código e a que põe os arquivos nos instaladores, **nenhuma tag** `v*` é criada: uma release nesse intervalo sairia sem texto. O [roadmap](../roadmap.md) ordena as etapas de forma que o empacotamento feche antes da `0.8.0`.

## Alternativas consideradas

### Publicar o binário cru como `.zip`/`.tar.gz` com `locales/` dentro

Preservaria o canal do "ponho no `PATH` à mão". Recusado pelo dono do produto: é um quinto formato a manter e verificar por plataforma, para um uso que o AppImage e os instaladores já cobrem.

### Manter o binário cru solto e aceitar identificadores nele

Zero trabalho de release. Recusado: seria publicar, com o nome do projeto, um download que abre mostrando `dialog.close_tab.title`.

### Embutir os arquivos só no binário cru

Resolveria o canal sem mudar os instaladores. Recusado pelo mesmo motivo do ADR-0056: duas fontes de verdade, e a decisão de nada embutir é do dono do produto, sem exceção por artefato.

### Instalar os arquivos na pasta de config do usuário

Um só lugar, o mesmo em toda plataforma. Recusado: instalador de sistema não escreve no perfil do usuário, a pasta de config é **do usuário** (ADR-0056 §6), e uma atualização teria de decidir se sobrescreve o que ele editou.

## Consequências

### Positivas

- Toda forma publicada do app abre com texto, e o CI prova isso por artefato.
- Idioma novo no repositório chega aos três instaladores que copiam por padrão sem mudança de receita.
- A página de release fica com um download por plataforma e formato, sem o arquivo solto que ninguém sabe qual escolher.

### Negativas

- **Um canal a menos**: some o executável solto. Quem o usava no Windows ou no macOS passa a instalar.
- O MSI precisa de um `Component` por idioma novo.
- `cargo install porecatu`, se um dia for publicado no crates.io, não funcionaria sem `PORECATU_LOCALES`.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Idioma novo no repositório esquecido no `wix/main.wxs` | Média | Baixo — o idioma falta só no Windows, com aviso e reserva em `en_US` | Verificação do §2 lista os arquivos de `locales/` do repositório e exige cada um no MSI extraído |
| Caminho do instalador divergir do que o resolvedor procura | Baixa | Alto — app sem texto | O §2 verifica o caminho exato; a tabela do §1 e a do ADR-0056 §6 são a mesma e mudam no mesmo PR |
| Tag criada entre remover as frases e empacotar | Baixa | Alto | Regra do §4, escrita no roadmap |
| Verificação do `.deb` com falso negativo por `pipefail` | Baixa | Médio | Saída capturada em variável, a correção já registrada no CLAUDE.md |
