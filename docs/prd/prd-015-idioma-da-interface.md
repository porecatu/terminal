# PRD-015 — Idioma da interface

**Status:** Aprovado
**Data:** 2026-09-29
**Requisito de origem:** pedido direto do dono do produto — *"incluir o recurso de internacionalização para este app"*, com arquivos de idioma nomeados pelo idioma (`pt_BR.toml`, `en_US.toml`) e a troca parametrizável no `porecatu.toml`
**Relacionados:** [ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md), [ADR-0057](../adr/0057-idiomas-nos-artefatos.md), [ADR-0003](../adr/0003-formato-de-configuracao.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md), [ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md), [ADR-0030](../adr/0030-escopo-do-hot-reload.md), [ADR-0039](../adr/0039-convite-a-integracao-de-shell.md), [ADR-0040](../adr/0040-superficie-de-linha-de-comando.md), [ADR-0043](../adr/0043-arvore-de-acessibilidade.md), [ADR-0044](../adr/0044-empacotamento-e-release.md), [PRD-004](prd-004-aparencia-do-chrome.md), [PRD-010](prd-010-interacao-e-superficie-de-app.md)

> Aprovado em 2026-09-29, por decisão do dono do produto, **fora da ordem de fases** — como o arquivo de projeto ([PRD-012](prd-012-comando-de-projeto-por-diretorio.md)), a sincronização com o remoto ([PRD-013](prd-013-sincronizacao-com-o-remoto-do-git.md)) e as sessões nomeadas ([PRD-014](prd-014-sessoes-nomeadas.md)). É requisito novo: nenhum documento do projeto mencionava idioma de interface, nem como objetivo, nem como não-objetivo — o [PRD-000](prd-000-visao-de-produto.md) não tem linha a emendar. Onde o catálogo mora, em que formato e como é achado está no [ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md); como os arquivos chegam à máquina do usuário, no [ADR-0057](../adr/0057-idiomas-nos-artefatos.md).

## Problema

Todo texto que o Porecatu mostra está escrito **dentro do código, em português do Brasil**. São perto de 180 frases espalhadas por menus, diálogos, avisos, a barra de busca, a barra de status, o popover de sessões, o editor de grupo, os rótulos que o leitor de tela anuncia e as notas que o app escreve no grid do terminal. Não há tabela de textos, nem um lugar onde uma frase possa ser trocada sem recompilar.

Isso produz três problemas:

- **Quem não lê português não usa o app.** O público natural de um emulador de terminal é internacional, e a primeira coisa que ele vê ao fechar uma aba com processo rodando é *"Fechar mesmo assim?"*.
- **Traduzir exige um fork.** Uma frase por idioma, em ~180 lugares do código, com plural escrito à mão (`aba{plural}`) — cada release reabriria o trabalho inteiro.
- **Parte da prosa nasce em crates que não deviam produzi-la.** Mensagens de erro de `porecatu-core`, `porecatu-config` e `porecatu-session` chegam prontas, em português, a superfícies de interface. Qualquer tradução precisa, antes, tirar a prosa de lá.

## O que já existe e não muda

- A **terminologia "abas"**, nunca "guias" ([ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md) §8), continua valendo — e passa a ser regra verificada do arquivo `pt_BR.toml` (RF-15.19).
- As **superfícies** não mudam: os sete widgets de chrome, a barra de status, as notas no grid e os avisos continuam sendo os mesmos, com a mesma anatomia. Este documento troca **o texto**, não o desenho — exceto o truncamento de rótulo de menu do RF-15.17, que tem aval de aparência do dono do produto.
- Os **identificadores** de ação (`tab.new`, `session.save_named`), as chaves de config e os nomes de tecla nos chips (`Ctrl+Shift+S`) já são inglês de identificador e não são texto de interface ([docs/reference/acoes.md](../reference/acoes.md), Convenções).

## Usuário-alvo

O mesmo do [PRD-000](prd-000-visao-de-produto.md), sem recorte por idioma: quem lê inglês, quem lê português, e quem quer pôr o app num terceiro idioma sem esperar o projeto. **Não é para** quem quer mudar o idioma do shell, do sistema ou dos programas que rodam no terminal — isso é do SO e de cada programa.

## Em uma tela

No `porecatu.toml`:

```toml
[general]
language = "pt_BR"   # default "en_US"
```

Em disco, ao lado do binário instalado, e opcionalmente na pasta de config do usuário:

```
<diretório do app>/locales/        <pasta do porecatu.toml>/locales/
  en_US.toml                          pt_BR.toml   ← só as frases que o usuário mudou
  pt_BR.toml                          de_DE.toml   ← idioma novo, do usuário
```

Dentro de um arquivo, tabelas por superfície e frases com marcadores nomeados:

```toml
[tab_menu]
close = "Fechar aba"

[group_menu]
close = { one = "Fechar grupo ({count} aba)", other = "Fechar grupo ({count} abas)" }
```

## Requisitos funcionais

### Escolher o idioma

**RF-15.1** — A chave `[general] language` escolhe o idioma da interface. O valor é o **nome do arquivo sem a extensão**: `"en_US"` usa `en_US.toml`, `"pt_BR"` usa `pt_BR.toml`.

**RF-15.2** — Sem a chave, o idioma é **`en_US`**. Não há detecção do idioma do sistema: o valor não muda de uma máquina para outra sem o usuário escrever nada.

**RF-15.3** — O nome segue a forma `xx_YY` — duas ou três letras minúsculas de idioma, sublinhado, duas letras maiúsculas de região —, com a caixa exata. Valor fora da forma (`pt-BR`, `pt_br`, `../x`) é tratado como idioma não encontrado (RF-15.9), com o valor citado no aviso. O resto da config não é afetado: idioma inválido nunca descarta o arquivo inteiro.

**RF-15.4** — Na primeira entrega, o projeto fornece **exatamente dois** arquivos: `en_US.toml` e `pt_BR.toml`, completos — toda frase das superfícies do RF-15.14 existe nos dois.

### Arquivos

**RF-15.5** — Os arquivos de idioma **vivem em disco**, nunca dentro do binário. Trocar uma frase não exige recompilar.

**RF-15.6** — O app procura cada arquivo em dois diretórios: o **diretório do app**, onde o instalador os pôs ([ADR-0057](../adr/0057-idiomas-nos-artefatos.md)), e a pasta **`locales/` ao lado do `porecatu.toml`** em uso — que, portanto, acompanha `--config` e `PORECATU_CONFIG`. O app não cria essa pasta; ela é do usuário.

**RF-15.7** — Quando os dois diretórios têm arquivo com o mesmo nome, eles se **mesclam frase a frase**: a frase do usuário vence a instalada, e a frase que o usuário não escreveu vem da instalada. Um usuário que quer mudar uma frase escreve um arquivo com uma frase só. Uma atualização do app que acrescenta frases novas as traz, mesmo com o arquivo do usuário presente.

**RF-15.8** — Um idioma que só existe na pasta do usuário (`de_DE.toml`, por exemplo) é um idioma válido: basta declará-lo em `language`.

### Falhas

**RF-15.9** — Idioma escolhido **não encontrado** em nenhum dos dois diretórios: a interface usa `en_US`, e um **aviso** do app ([ADR-0014](../adr/0014-superficie-de-aviso-e-dialogo.md)) diz qual idioma faltou e em quais diretórios o app procurou.

**RF-15.10** — Arquivo **ilegível ou com erro de sintaxe**: aquele arquivo é ignorado — só ele, não o outro diretório do mesmo idioma — e um **erro** do app diz o caminho, a linha e a coluna, como o erro de config do RF-4.21.

**RF-15.11** — Frase **ausente** no idioma escolhido cai na frase de `en_US`. Frase ausente também em `en_US` é mostrada como o **próprio identificador** da frase (`dialog.close_tab.title`), nunca como texto vazio. Frases ausentes geram **uma** informação agregada por idioma (*"12 textos sem tradução em de_DE"*), não uma por frase, e que expira sozinha.

**RF-15.12** — Nenhum arquivo de idioma encontrado — nem o escolhido, nem `en_US`: a interface mostra identificadores, e o app avisa com **uma frase fixa em inglês**, a única do app escrita no código, dizendo que os arquivos de idioma não foram encontrados e onde procurou. A mesma frase vai para a saída de erro.

**RF-15.13** — Frase com marcador que o app não conhece (`{contagem}` onde o app preenche `{count}`), ou frase de plural sem a forma `other`, é tratada como ausente (RF-15.11). Frase que omite um marcador é aceita: é escolha do tradutor.

### Escopo

**RF-15.14** — Passam pelo arquivo de idioma:

- **chrome**: menus de contexto (aba, grupo, terminal), popover de grupo de destino, editor de grupo, diálogos de confirmação, avisos do app, barra de busca, barra de status, popover de sessões e o nome padrão de grupo novo;
- **rótulos de acessibilidade** anunciados pelo leitor de tela ([ADR-0043](../adr/0043-arvore-de-acessibilidade.md)), incluindo os nomes das cores da paleta de grupo;
- **notas no grid**: processo encerrado, convite à integração de shell (o texto em volta do snippet), arquivo de projeto não autorizado, diretório de sessão que não existe mais;
- **avisos de config**: config inválida, chave desconhecida, tema desconhecido, atalho inválido, chave que não vale a quente — na barra e na saída de erro.

**RF-15.15** — Ficam fora, e aparecem sempre iguais em qualquer idioma: o texto da **linha de comando** (`--help`, `--version`, erro de argumento), que passa a ser **em inglês fixo**; os nomes de tecla nos chips; o título "Porecatu"; `UTF-8` na barra de status; o marcador de dispensa do convite, que o usuário digita no terminal; o conteúdo dos snippets de integração de shell (é código); e o texto que chega de fora do app — mensagem do analisador de TOML, do sistema operacional, do `git`.

**RF-15.16** — O nome de grupo novo (*"Novo grupo"*) é resolvido **no idioma corrente no momento em que o grupo nasce**, e daí em diante é dado do usuário: é gravado na sessão e não muda se o idioma mudar depois.

**RF-15.17** — Rótulo de menu mais largo que o menu **trunca com reticências**, pelo mesmo truncamento que já corta título de aba, em vez de invadir o chip de atalho. Nenhuma dimensão muda: é o menu que já existe, com o corte que já existe.

**RF-15.18** — O leitor de tela é informado do idioma da árvore de acessibilidade, para anunciar com a voz certa.

### Terminologia e documentação

**RF-15.19** — O arquivo `pt_BR.toml` usa **"aba"/"abas"**, nunca "guia"/"guias", e isso é verificado automaticamente, não por revisão.

**RF-15.20** — Texto de interface **citado** em PRD, ADR, especificação visual e guia do usuário é o de **`pt_BR.toml`**. A identidade de uma frase é o seu identificador, não o texto: requisitos que citam *"Salvar esta janela…"* continuam valendo, lidos como "a frase `session_picker.save_item`", e não são emendados um a um.

### Troca ao vivo

**RF-15.21** — Mudar `language` no `porecatu.toml` com o app aberto vale **ao vivo** (classe A do [ADR-0030](../adr/0030-escopo-do-hot-reload.md)): o próximo frame de toda janela já desenha no idioma novo. O mesmo vale para editar um arquivo na pasta `locales/` do usuário.

**RF-15.22** — O que é desenhado a cada frame muda na hora: barra, menus (inclusive um já aberto), barra de status, busca, popovers, rótulos de acessibilidade. O que foi **composto por um evento** fica como foi escrito: notas já no grid, avisos já empilhados, um diálogo já aberto. Isso é coerente com o grid: o app não reescreve texto que já entregou ao terminal.

**RF-15.23** — Troca ao vivo para um idioma que falha (RF-15.9 a RF-15.12) **mantém o idioma anterior** e avisa — o mesmo que a recarga de config faz com config inválida. No arranque, sem idioma anterior, vale o RF-15.9.

## Cenários

```gherkin
Cenário: sem configuração, o app abre em inglês
  Dado um porecatu.toml sem a chave language
  Quando o app abre
  Então o menu de contexto da aba mostra "Close tab"

Cenário: escolher português
  Dado [general] language = "pt_BR"
  Quando o usuário clica com o botão direito numa aba
  Então o menu mostra "Fechar aba"

Cenário: trocar o idioma com o app aberto
  Dado o app aberto em en_US, com um diálogo de confirmação aberto
  Quando o usuário grava language = "pt_BR" no porecatu.toml
  Então a barra de abas e a barra de status passam a português no próximo frame
  E o diálogo já aberto continua em inglês até ser fechado

Cenário: corrigir uma frase sem copiar o arquivo inteiro
  Dado language = "pt_BR"
  E <pasta de config>/locales/pt_BR.toml contendo só tab_menu.close = "Encerrar aba"
  Quando o usuário abre o menu de contexto da aba
  Então o item mostra "Encerrar aba"
  E todos os outros itens mostram as frases do pt_BR.toml instalado

Cenário: idioma que não existe
  Dado language = "fr_FR" e nenhum fr_FR.toml em disco
  Quando o app abre
  Então a interface aparece em inglês
  E um aviso diz que fr_FR não foi encontrado, com os diretórios procurados

Cenário: idioma do usuário incompleto
  Dado <pasta de config>/locales/de_DE.toml com metade das frases
  E language = "de_DE"
  Quando o app abre
  Então as frases presentes aparecem em alemão e as ausentes em inglês
  E uma única informação diz quantas frases faltam

Cenário: instalação quebrada
  Dado nenhum arquivo de idioma em nenhum diretório
  Quando o app abre
  Então os textos da interface aparecem como identificadores
  E um aviso em inglês diz que os arquivos de idioma não foram encontrados

Cenário: nome de grupo não é retraduzido
  Dado um grupo criado em pt_BR com o nome padrão "Novo grupo"
  Quando o usuário troca para en_US
  Então o grupo continua chamado "Novo grupo"
  E um grupo criado depois disso nasce "New group"
```

## Fora de escopo

Cada item é decisão, não esquecimento.

- **Detectar o idioma do sistema.** O default é fixo (RF-15.2); a interface nunca muda porque o usuário trocou de máquina.
- **Embutir os arquivos no binário**, mesmo como reserva. Os arquivos são a fonte, e a única frase no código é a do RF-15.12.
- **Traduzir a linha de comando.** `--help` e erros de argumento acontecem antes de a config ser lida, e passam a inglês fixo (RF-15.15).
- **Traduzir o arquivo de exemplo de config, a documentação ou o guia do usuário.** São documentação, e a documentação do projeto é em português do Brasil.
- **Idiomas com mais de duas formas de plural** (russo, polonês, árabe). O formato tem `one` e `other`; mais formas pedem ADR próprio ([ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md) §5).
- **Escrita da direita para a esquerda** e **escritas fora do recorte das fontes embutidas**. O chrome desenha latim, grego e cirílico com a face do design; outras escritas caem na fonte do sistema, com métrica diferente, e isso é limitação registrada, não requisito.
- **Tela de escolha de idioma.** A escolha é a chave do TOML, como toda escolha do app ([ADR-0003](../adr/0003-formato-de-configuracao.md)); o painel de configuração por GUI continua `[v2]`.
- **Aviso de "sua interface mudou de idioma"** no primeiro arranque após atualizar. A mudança do default é comunicada no CHANGELOG e no guia do usuário.
- **Traduzir o texto do `git`, do analisador de TOML ou do sistema operacional.** Chega pronto de fora e é mostrado como chegou.

### Migração

Até esta entrega, a interface é sempre português. Com o default `en_US` (RF-15.2), **todo usuário atual passa a ver a interface em inglês** ao atualizar — decisão consciente do dono do produto. Quem quer continuar em português escreve uma linha: `language = "pt_BR"`. A versão que traz a mudança é a **`0.8.0`**, e a nota do CHANGELOG, na seção "Alterado", traz essa linha pronta para copiar.

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Frases de interface escritas no código fora dos arquivos de idioma | **uma** — a do RF-15.12 |
| Frases presentes em `en_US.toml` e ausentes em `pt_BR.toml`, ou o contrário | **zero**, verificado no CI |
| Ocorrências de "guia"/"guias" em `pt_BR.toml` | **zero**, verificado no CI |
| Gestos para trocar o idioma | **um** — gravar a chave no `porecatu.toml` |
| Frases que um usuário precisa copiar para corrigir uma | **uma** |
| Artefatos de release que abrem sem os arquivos de idioma | **zero** |
| Dependências novas de internacionalização (crate de i18n) | **zero** |

A primeira é a que mantém o recurso de pé: cada frase nova escrita direto no código é uma frase que nenhum idioma alcança, e ela entra sem ninguém perceber se não for contada.
