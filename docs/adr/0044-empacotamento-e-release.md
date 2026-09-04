# ADR-0044 — Empacotamento por plataforma e a primeira release

**Status:** Aceito
**Data:** 2026-09-04
**Relacionados:** ADR-0010, ADR-0011, ADR-0016, ADR-0026, ADR-0027, ADR-0040, PRD-000, PRD-011

## Contexto

O `release.yml` está escrito desde a F0 e **nunca publicou nada**. Ele dispara em tag `v*`, compila `--release --locked` nas três plataformas, copia o executável, o `LICENSE` e o `README.md` para `dist/`, gera `sha256` e anexa tudo à release do GitHub. O comentário no topo diz literalmente: *"nenhuma release foi publicada ainda: o roadmap coloca artefatos na F6"*.

O que ele produz é um **executável solto**. Isso não instala nada:

- no Windows, um `.exe` avulso não cria entrada no menu iniciar, não registra o app e dispara o SmartScreen a cada execução;
- no macOS, um binário Mach-O não é um `.app` — não aparece no Launchpad, não tem `Info.plist`, e o Gatekeeper barra;
- no Linux, o usuário move o arquivo à mão e não há `.desktop`, então o app não existe para o menu de aplicativos.

Há uma ironia concreta nisso: o **ícone já está pronto e entregue fora de fase** — `app_icon.rs` decodifica um PNG embutido para toda janela, e um `build.rs` com `winres` embute o `.ico` como recurso PE no Windows. O ícone de janela funciona; o de aplicativo não tem onde aparecer, porque não há aplicativo instalado, só um arquivo.

Duas decisões estavam abertas e nenhuma tem dono natural: **qual ferramenta empacota**, e **qual número de versão** o primeiro release carrega — o workspace está em `0.1.0` e o `CHANGELOG` diz que o SemVer começa a valer *"a partir do primeiro release"*, sem dizer qual é.

Some-se um detalhe que só aparece quando alguém tenta: a matriz do `release.yml` tem `aarch64-apple-darwin` e nenhum alvo Intel para macOS.

## Decisão

**Instalador nativo por plataforma, orquestrado no `release.yml` que já existe, e a primeira release é a `1.0.0`. Sem assinatura de código no v1.**

### 1. Um instalador por plataforma

| Plataforma | Artefato | O que ele resolve |
|---|---|---|
| Windows | instalador MSI | entrada no menu iniciar, ícone de aplicativo, desinstalação pelo painel do sistema |
| macOS | `.app` dentro de `.dmg` | Launchpad, `Info.plist`, ícone, instalação por arraste |
| Linux | `.deb` e AppImage | `.desktop` e ícone no menu; o AppImage cobre quem não usa distribuição Debian |

O **binário cru continua sendo publicado**, ao lado dos instaladores e com o mesmo `sha256`. Ele é o caminho de quem põe o app num diretório do `PATH` à mão, e já funciona hoje — tirá-lo seria remover uma opção que existe para acrescentar outra.

Duas correções entram junto, porque são a mesma conversa:

- **`x86_64-apple-darwin` entra na matriz.** Publicar só `aarch64` deixaria todo Mac Intel de fora, e a matriz atual não é uma decisão registrada em lugar nenhum — é o que estava escrito.
- **`--locked` passa a valer também no `ci.yml`.** O `release.yml` já usa; o `ci.yml` não, e essa é a pendência aberta desde a F0 (*"nada impede o CI de resolver versões novas"*). Um release reproduzível verificado por um CI que não é reproduzível é meia garantia.

### 2. A ferramenta é escolhida na etapa, contra três critérios fixos

Este ADR **não pina a ferramenta**, e isso é deliberado: empacotador é software de build que muda rápido, e a decisão aqui é o contrato que ele tem de cumprir. Os critérios, em ordem:

1. **Roda no CI sem passo manual.** Um instalador que só sai da máquina de alguém não é release, é favor.
2. **Não altera o binário.** O empacotador envolve; ele não recompila com flags próprias nem substitui o ícone que o `build.rs` já embutiu.
3. **Licença compatível com GPLv3**, conferida na adoção, como a convenção manda.

Ferramenta de empacotamento é **ferramenta de build, não dependência ligada**: ela não entra no binário distribuído, o que muda o peso do critério 3 — mas não o dispensa, e a conferência acontece na etapa, não por presunção aqui.

As candidatas levantadas, para quem for executar não recomeçar do zero: `cargo-dist` (orquestra as três plataformas de uma vez), `cargo-packager`, e a combinação direta `cargo-wix` + `cargo-bundle` + `cargo-deb`. A primeira é a que mais se aproxima do critério 1.

### 3. A primeira release é `1.0.0`

O roadmap chama o produto inteiro de **v1** desde a F0, e a F6 é a última fase dele. Publicar como `0.x` diria que ainda não é o que sete fases dizem que é.

O que torna `1.0.0` honesto e não bravata é a superfície pública ser pequena e já ter regra de compatibilidade escrita:

- o **arquivo de configuração** já trata chave desconhecida como aviso e não como erro ([ADR-0003](0003-formato-de-configuracao.md)), então acrescentar chave numa `1.x` não quebra config existente;
- o **arquivo de sessão** já é DTO versionado com migração e com regra para schema mais novo ([ADR-0036](0036-formato-do-arquivo-de-sessao.md));
- a **linha de comando** é pequena de propósito ([ADR-0040](0040-superficie-de-linha-de-comando.md));
- o **catálogo de ações** é fechado e documentado.

O que muda com `1.0.0`: remover uma chave de config, remover uma ação do catálogo ou quebrar o formato de sessão passam a exigir uma major. É exatamente a promessa que se quer fazer.

O `version.workspace` sobe de `0.1.0` para `1.0.0` no PR de release, junto com o `CHANGELOG`.

### 4. O que vai dentro de todo artefato

- O executável.
- `LICENSE` — cópia verbatim da FSF, que o workflow `docs` já verifica por hash ([ADR-0010](0010-licenciamento.md)).
- **A atribuição das fontes embutidas**: `assets/fonts/LICENSE-OFL-iosevka.txt` e `assets/fonts/LICENSE-ISC-lucide.txt`. Não é cortesia — as três faces são embutidas e recortadas no binário, e as duas licenças exigem que o texto acompanhe a distribuição ([ADR-0016](0016-fontes-embutidas.md), [ADR-0024](0024-face-de-icones.md), [ADR-0026](0026-chrome-unificado-em-iosevka-fixed.md)). Os dois arquivos existem no repositório e **não** são copiados para `dist/`: o `release.yml` copia só `LICENSE` e `README.md`. É um descumprimento de licença que só não aconteceu porque nada foi publicado.
- `porecatu.example.toml`, que é a documentação real da configuração.
- O `sha256` de cada artefato.

### 5. Sem assinatura de código no v1

Assinatura exige certificado Authenticode (anual, pago) no Windows e conta de desenvolvedor Apple mais notarização no macOS, além de guardar chaves privadas como segredos do CI. Fica **fora do v1**, e a consequência é assumida e documentada em vez de descoberta pelo usuário: no Windows aparece o aviso do SmartScreen, no macOS é preciso liberar o app na primeira execução.

A documentação de usuário (RF-11.22) explica os dois casos e diz como conferir o `sha256` — que é a verificação que o projeto **pode** oferecer sem custo, e que uma assinatura não substitui.

### 6. Documentação de usuário e página de release

A documentação de usuário nasce nesta fase (RF-11.22) e cobre instalação, configuração, atalhos, integração de shell e a convenção do `Shift` para selecionar texto dentro de um programa que pede o mouse — este último é exigência nominal do [ADR-0013](0013-mouse-selecao-e-clipboard.md): *"usuário que não conheça a convenção do `Shift` vai achar que a seleção quebrou dentro do `htop`. Precisa estar na documentação de usuário da F6."*

As notas de release saem do `CHANGELOG`, escrito em português como o resto da documentação. O `generate_release_notes: true` do workflow continua ligado, mas como complemento — lista de commits não é nota de versão.

## Alternativas consideradas

### Publicar só o binário cru, como hoje

Zero trabalho: o workflow já faz. Rejeitada porque não entrega o requisito — o app não aparece em menu nenhum, o ícone embutido não tem onde ser usado, e o primeiro contato de qualquer usuário vira "descompacte e mova à mão". Um emulador de terminal que exige linha de comando para ser instalado tem um problema de porta de entrada.

### Publicar em gerenciadores de pacote (winget, Homebrew, Flatpak, AUR)

É o que os usuários realmente preferem, e cada um resolve instalação e atualização de uma vez. Rejeitado para o v1 por dependência externa: winget e Homebrew exigem PR em repositório de terceiros e aprovação com prazo que não controlamos, e Flatpak pede manifesto e runtime próprios. Nada impede que venham depois — todos consomem o artefato que esta decisão cria, então nenhum trabalho aqui é jogado fora.

### Pinar a ferramenta de empacotamento neste ADR

Seria mais decidido, e o padrão de todo o resto da stack é pinar. Rejeitado porque a stack pinada é de **crates ligados ao binário**, onde a versão muda o que roda no usuário — e o [ADR-0011](0011-toolchain-rust.md) construiu essa disciplina para esse caso. Empacotador é ferramenta de build: pinar a versão exata continua valendo no workflow, mas escolher **qual** sem ter rodado nenhuma das três no CI seria decidir no papel algo que uma tarde de execução decide melhor. O contrato — os três critérios — é o que precisa estar escrito, e está.

### Sair como `0.1.0` e deixar `1.0.0` para depois

Conservador, e evita a promessa de compatibilidade. Rejeitada porque o roadmap chama isto de v1 em sete lugares e a F6 é a fase que o fecha; publicar `0.1.0` obrigaria a explicar em toda página por que a "v1" é `0.x`. E a promessa de compatibilidade que o `1.0.0` faz é sobre config, sessão, ações e linha de comando — superfícies que já têm regra escrita, não sobre API de crate, que não é pública.

### Assinar no v1

Elimina SmartScreen e Gatekeeper, que são o atrito mais visível de um app novo. Rejeitada por custo recorrente em dinheiro e por segredos de assinatura no CI, que são uma superfície de segurança própria. Fica registrada como a primeira candidata a entrar depois do v1.

### Construir o Linux num contêiner de glibc antiga

Resolveria de vez o piso de glibc. Rejeitada para o v1 por complexidade de CI desproporcional; a mitigação escolhida é mais simples e está nos riscos abaixo.

## Consequências

### Positivas

- O app passa a ser **instalável** nas três plataformas, e o ícone que já existe finalmente aparece onde deveria.
- A obrigação de atribuição da OFL passa a ser cumprida na distribuição — hoje não seria.
- `--locked` no `ci.yml` fecha a última pendência aberta da F0.
- Mac Intel deixa de ficar de fora.
- `1.0.0` transforma config, sessão, ações e linha de comando em compromissos versionados.

### Negativas

- SmartScreen e Gatekeeper vão incomodar todo primeiro usuário, e a resposta do projeto é uma seção de documentação, não uma solução.
- O `release.yml` cresce e passa a ter passo específico por plataforma, deixando de ser a matriz simétrica que é hoje.
- `1.0.0` fecha a porta para remover chave de config ou ação sem uma major.
- Mais uma ferramenta externa no caminho de publicação, com versão a manter.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Piso de glibc do runner deixar distribuições LTS de fora do `.deb` | **Alta** | Médio | Construir na imagem Ubuntu mais antiga ainda suportada pelo GitHub, e o AppImage como caminho alternativo. Piso declarado na documentação de usuário |
| Ferramenta de empacotamento recompilar o binário com flags próprias e perder o recurso de ícone do `winres` | Média | Médio | Critério 2 da §2 é exatamente isto; verificação do artefato é abrir o `.exe` instalado e conferir o ícone |
| Release publicada sem a atribuição das fontes | Média | **Alto** (licença) | Passo de empacotamento copia `assets/fonts/` junto com `LICENSE`; teste no workflow que reprova se o artefato não contiver o arquivo de atribuição |
| macOS não verificável neste fluxo — `.dmg` sair quebrado | Alta | Médio | O CI monta e o job falha se a montagem falhar; instalação real fica como dívida de verificação, no formato das outras |
| Tag empurrada por engano publicar uma release | Baixa | Médio | O workflow já exige tag `v*`; a release sai como pré-release enquanto o binário não for produzido, comportamento que já está no arquivo |
| `1.0.0` obrigar major cedo demais por uma chave mal desenhada | Média | Baixo | Chave desconhecida é aviso, não erro: renomear com o nome antigo aceito por um ciclo é mudança menor, não major |
