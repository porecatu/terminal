# PRD-012 — Comando de projeto por diretório

**Status:** Aprovado
**Data:** 2026-09-10
**Requisito de origem:** derivado da métrica que define o produto ([PRD-000](prd-000-visao-de-produto.md)) — "reconstrução manual de contexto após reabrir: **zero ações do usuário**", que a persistência de sessão cumpre só até o diretório
**Relacionados:** [ADR-0051](../adr/0051-arquivo-de-projeto-porecatu.md), [ADR-0003](../adr/0003-formato-de-configuracao.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0037](../adr/0037-aba-nao-iniciada.md), [ADR-0039](../adr/0039-convite-a-integracao-de-shell.md), [ADR-0042](../adr/0042-hyperlinks-osc-8.md), [PRD-003](prd-003-persistencia-de-sessao.md)

> Aprovado em 2026-09-10, por decisão do dono do produto, **fora da ordem de fases** — como a barra de status ([PRD-009](prd-009-barra-de-status.md)). Não é elemento do canvas nem rascunho promovido: é requisito novo, escrito depois de o v1 estar em uso. As cinco decisões que ele deixa em aberto — formato, casamento de shell, onde vive a confiança, quando escrever no PTY e o que fazer com um arquivo não autorizado — estão no [ADR-0051](../adr/0051-arquivo-de-projeto-porecatu.md).

## Problema

O [PRD-003](prd-003-persistencia-de-sessao.md) entregou metade da promessa. Reabrir o app devolve as abas, os grupos, os nomes, as cores e os diretórios — e para exatamente aí.

O que ele **não** devolve é o que cada aba estava fazendo. E é isso que o usuário faz a seguir, uma aba por vez, de memória:

- **Aba "api" → `npm run dev`.** Toda vez.
- **Aba "infra" → `docker compose up`.** Toda vez.
- **Aba "web" → `nvm use` e depois `npm start`**, porque aquele projeto está preso numa versão antiga do Node.
- **Aba "logs" → `kubectl logs -f deploy/api`.** Toda vez.

Com quatro abas isso é irritante. Com quinze — o número que o [PRD-000](prd-000-visao-de-produto.md) usa para descrever o usuário-alvo — a estrutura volta em um segundo e o **conteúdo** leva minutos, que é exatamente o custo que a persistência de sessão existia para eliminar.

Três agravantes, e nenhum é hipotético:

- **O comando não está na cabeça, está no projeto.** Ninguém lembra a invocação exata do projeto que não toca há três semanas. Ela está no `README`, no `package.json`, ou na memória do histórico do shell daquela máquina.
- **O comando difere por shell, e o mesmo projeto é aberto em shells diferentes.** O que funciona no `bash` do CI não funciona no `pwsh` da máquina Windows, e `&&` não existe no PowerShell 5.1.
- **A métrica falhou, e falhou em silêncio.** "Zero ações do usuário" foi medida no fechamento da F6 contra a estrutura restaurada, não contra o trabalho retomado. Quem reabre o app ainda digita quinze comandos.

O que falta é um lugar, **versionado junto com o projeto**, onde o projeto declare como se levanta — e o app o obedeça ao restaurar a aba daquele diretório.

## Usuário-alvo

O mesmo do [PRD-000](prd-000-visao-de-produto.md): quem trabalha em mais de um repositório ao mesmo tempo e reabre o emulador todo dia. O valor aparece já no primeiro projeto e cresce por projeto, não por aba.

**Não é para** quem abre o terminal para um comando avulso: um `.porecatu` num diretório onde nada é rotineiro não tem o que declarar.

## O arquivo, em uma tela

Um arquivo chamado `.porecatu`, na raiz do projeto, versionado junto com ele. Uma seção por tipo de terminal; o corpo é o script, literal, como o usuário o digitaria:

```
[pwsh]
npm run dev

[bash]
nvm use
npm run dev

[default]
echo Sem script para este shell. Veja o .porecatu.
```

E, uma vez, na config do usuário — **não** no projeto:

```toml
[project_file]
trusted_paths = ["C:/Projetos"]
```

A referência completa do formato está em [docs/reference/arquivo-de-projeto.md](../reference/arquivo-de-projeto.md).

## Requisitos funcionais

### O arquivo

**RF-12.1** — Ao restaurar uma aba de sessão, se o diretório de trabalho dela contém um arquivo chamado `.porecatu`, o app procura nele o script correspondente ao shell daquela aba e o executa.

**RF-12.2** — O formato é de **seções cruas**: uma linha `[nome]` abre uma seção, e todo o resto até a próxima seção — ou até o fim do arquivo — é o script daquela seção, literal, linha por linha. Linhas antes da primeira seção são preâmbulo e são ignoradas.

**RF-12.3** — A seção é escolhida pelo nome do shell da aba, por casamento **exato** e insensível a maiúsculas. Sem seção correspondente, vale `[default]`. Sem nenhuma das duas, nada acontece e nada é informado — o arquivo declara os shells que suporta, e não suportar um não é erro.

**RF-12.4** — Arquivo ilegível, ou fora de UTF-8, não executa nada e informa o motivo na própria aba.

### Confiança

**RF-12.5** — O script só é executado se o diretório estiver declarado confiável na configuração do usuário, em `[project_file] trusted_paths`. Um caminho listado cobre a árvore inteira sob ele. **A lista é vazia por default:** numa instalação recém-feita, nenhum `.porecatu` roda.

**RF-12.6** — Um `.porecatu` encontrado em diretório **não** autorizado não roda e **não passa em silêncio**: o app informa uma vez por execução, na aba onde o encontrou, dizendo o caminho e a linha exata a acrescentar na config para autorizá-lo. Silêncio aqui seria indistinguível de um recurso quebrado.

**RF-12.7** — `[project_file] enabled = false` desliga o recurso inteiro antes de qualquer leitura de disco, incluindo o aviso do RF-12.6.

### Execução

**RF-12.8** — O script é entregue ao shell **como se o usuário o tivesse digitado**: aparece na tela, entra no histórico do shell, e é interrompível com `Ctrl+C` como qualquer outro comando. Nada é executado fora da vista.

**RF-12.9** — O script roda no máximo **uma vez por aba, por execução do app**. Sair da aba e voltar não reexecuta nada.

**RF-12.10** — O script só é escrito quando o shell está pronto para recebê-lo. Um script escrito antes de o shell chegar ao primeiro prompt se perde, e perder-se em silêncio é o pior resultado possível.

**RF-12.11** — Com `lazy_restore = true` (o default do [RF-3.8](prd-003-persistencia-de-sessao.md)), o script roda quando a aba de fato inicia — no primeiro foco —, não no arranque do app.

### Fronteiras

**RF-12.12** — **Só restauração.** Aba criada por `tab.new`, `group.new_tab` ou `window.new` nunca dispara o `.porecatu`, mesmo herdando um diretório que tenha um. Mudar de diretório com `cd` também não dispara.

**RF-12.13** — A busca acontece **só no diretório exato** da aba. O app não sobe a árvore de diretórios: abrir uma aba numa subpasta do projeto não dispara o script da raiz.

**RF-12.14** — Se o diretório gravado não existe mais e a aba caiu no home ([RF-3.10](prd-003-persistencia-de-sessao.md)), nada é procurado e nada roda. O projeto sumiu; o script dele não tem onde rodar.

**RF-12.15** — Nada do que o script faz é gravado na sessão. O `.porecatu` é do projeto, e a sessão continua gravando só estrutura e diretórios.

## Critérios de aceite

```gherkin
Cenário: o caso que motiva o recurso
  Dado um diretório C:\Projetos\api declarado em trusted_paths
  E um .porecatu ali com uma seção [pwsh] contendo "npm run dev"
  E uma sessão gravada com uma aba nesse diretório, sob pwsh
  Quando o usuário reabre o app
  Então a aba volta em C:\Projetos\api
  E "npm run dev" aparece escrito no prompt daquela aba
  E o servidor de desenvolvimento sobe sem o usuário digitar nada

Cenário: diretório não autorizado não executa
  Dado um diretório C:\Baixados\repo-clonado fora de trusted_paths
  E um .porecatu ali com uma seção [pwsh]
  Quando a aba daquele diretório é restaurada
  Então nenhum comando é executado
  E a aba informa que encontrou um .porecatu não autorizado
  E informa a linha de config que o autorizaria

Cenário: o aviso não vira ruído
  Dado cinco abas restauradas em diretórios não autorizados, todos com .porecatu
  Quando a sessão é restaurada
  Então o app informa uma vez, não cinco

Cenário: shell sem seção
  Dado um .porecatu com as seções [bash] e [zsh] apenas
  E uma aba restaurada sob cmd, num diretório autorizado
  Quando a aba é restaurada
  Então nada é executado
  E nada é informado

Cenário: seção default
  Dado um .porecatu com [bash] e [default]
  E uma aba restaurada sob pwsh, num diretório autorizado
  Quando a aba é restaurada
  Então o script de [default] é executado

Cenário: aba nova não dispara
  Dado uma aba ativa em C:\Projetos\api, com .porecatu e diretório autorizado
  Quando o usuário abre uma aba nova com Ctrl+T
  Então a aba nova abre no mesmo diretório
  E nenhum comando é executado nela

Cenário: subpasta não herda o script da raiz
  Dado um .porecatu em C:\Projetos\api
  E uma sessão com uma aba gravada em C:\Projetos\api\src
  Quando a sessão é restaurada
  Então nada é executado naquela aba

Cenário: restauração preguiçosa
  Dado uma sessão com dez abas em diretórios autorizados, todas com .porecatu
  E lazy_restore ligado
  Quando o usuário reabre o app
  Então só o script da aba ativa é executado
  E o script de cada outra aba roda quando ela é focada pela primeira vez

Cenário: o script é visível e interrompível
  Dado uma aba restaurada que executou "npm run dev" pelo .porecatu
  Quando o usuário pressiona Ctrl+C
  Então o comando é interrompido como qualquer outro
  E a seta para cima traz "npm run dev" do histórico do shell

Cenário: diretório removido
  Dado uma aba gravada em C:\Projetos\removido, com o diretório apagado
  Quando a sessão é restaurada
  Então a aba abre no diretório home
  E nenhum .porecatu é procurado
  E nenhum comando é executado

Cenário: desligado é desligado
  Dado project_file com enabled = false
  E abas restauradas em diretórios autorizados com .porecatu
  Quando a sessão é restaurada
  Então nenhum comando é executado
  E nenhum aviso é mostrado

Cenário: uma vez por aba
  Dado uma aba restaurada que já executou seu script
  Quando o usuário troca para outra aba e volta
  Então o script não é executado de novo
```

## Fora de escopo

Cada item é decisão, não esquecimento. Os motivos completos estão no [ADR-0051](../adr/0051-arquivo-de-projeto-porecatu.md) §8.

- **Subir a árvore de diretórios** procurando o `.porecatu` do projeto. Faria uma aba numa subpasta disparar o script da raiz, que é surpresa, e multiplicaria a superfície de execução.
- **Disparar em aba nova ou ao mudar de diretório.** É o modelo do `direnv`; exige integração de shell funcionando (no Windows, sem OSC 7, não funcionaria) e transforma um recurso de restauração num hook contínuo.
- **Variáveis, interpolação, condicional ou qualquer lógica dentro do arquivo.** O corpo é literal por decisão; quem precisa de lógica a escreve no script, que é do shell.
- **Metadados por seção** (`title` para renomear a aba, `working_dir`, `enabled`). Cada um é uma chave a manter num formato que o app passa a versionar sozinho, e nenhum foi pedido.
- **Ação no catálogo para reexecutar o script à mão.** O catálogo é fechado ([docs/reference/acoes.md](../reference/acoes.md)), e ação sem requisito não entra.
- **Autorizar um diretório pela interface.** O app não escreve na config do usuário, por decisão consistente desde o [ADR-0031](../adr/0031-temas-nomeados.md).
- **Um `.porecatu` global, fora do projeto.** Para isso já existe o arquivo de perfil do próprio shell.
- **Restaurar o comando que estava rodando.** Continua proibido pelo [PRD-003](prd-003-persistencia-de-sessao.md), e este documento não o reabre — ver abaixo.

### O que este documento **não** contradiz

O [PRD-003](prd-003-persistencia-de-sessao.md) diz que reexecutar automaticamente o comando que estava rodando não é seguro, e que restaurar um `rm -rf` ou um `terraform apply` interrompido seria pior que não restaurar nada. **Isso continua valendo, sem emenda.**

A diferença é a origem do comando. O PRD-003 proíbe executar o que o app **gravou** observando o usuário; este PRD executa o que o usuário **declarou** num arquivo do projeto — que ele escreveu, que está no controle de versão e que ele revisa como revisa qualquer outro arquivo do repositório. Um é replay, o outro é declaração.

Isso não torna a declaração inofensiva: o arquivo chega junto com o projeto, e projeto clonado é projeto de outra pessoa. É por isso que o `.porecatu` de um diretório precisa ser autorizado (RF-12.5) — enquanto o replay do PRD-003 não seria seguro nem com autorização, porque ninguém declarou nada.

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Ações do usuário para retomar um projeto com servidor de dev, após reabrir | **zero**, com o diretório autorizado |
| Diretórios em que um script roda sem o usuário ter declarado confiança | **zero** |
| Comandos executados sem aparecer na tela do usuário | **zero** |
| `.porecatu` encontrado e ignorado sem o usuário ficar sabendo | **zero** |
| Formatos de arquivo novos que o app passa a manter | 1 |

A primeira é a razão de este documento existir: é a métrica do [PRD-000](prd-000-visao-de-produto.md) medida contra o **trabalho** retomado, e não só contra a estrutura. A segunda é a que impede a primeira de custar caro demais.
