# PRD-019 — Efeito acrílico no fundo do terminal

**Status:** Aprovado
**Data:** 2026-10-22
**Requisito de origem:** pedido direto do dono do produto, sobre a imagem de fundo da janela ([PRD-018](prd-018-imagem-de-fundo-da-janela.md))
**Relacionados:** [ADR-0068](../adr/0068-efeito-acrilico-no-fundo-do-terminal.md), [ADR-0007](../adr/0007-modelo-de-threading.md), [ADR-0030](../adr/0030-escopo-do-hot-reload.md), [ADR-0032](../adr/0032-interface-do-v1-fechada.md), [ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md), [ADR-0060](../adr/0060-anatomia-da-tela-de-configuracoes.md), [ADR-0061](../adr/0061-imagem-de-fundo-do-terminal.md), [ADR-0062](../adr/0062-imagem-de-fundo-da-janela.md), [ADR-0063](../adr/0063-imagem-da-janela-sem-furo-ate-o-desktop.md), [PRD-016](prd-016-tela-de-configuracoes.md), [PRD-017](prd-017-imagem-de-fundo-do-terminal.md), [PRD-018](prd-018-imagem-de-fundo-da-janela.md)

> Aprovado em 2026-10-22, por decisão do dono do produto, **fora da ordem de fases**, pelo caminho do [PRD-017](prd-017-imagem-de-fundo-do-terminal.md) e do [PRD-018](prd-018-imagem-de-fundo-da-janela.md): requisito novo, escrito com o v1 em uso, sem elemento do canvas. Revoga em parte o "fora de escopo" de **desfoque** dos dois, emendados por blockquote, não apagados: o desfoque passa a existir **só** para a imagem da janela vista através do quadro do terminal. Muda pixel dentro do quadro do terminal (§2.7 da [especificação visual](../design/especificacao-visual.md)), então passa pelo ADR que o [ADR-0032](../adr/0032-interface-do-v1-fechada.md) exige: o [ADR-0068](../adr/0068-efeito-acrilico-no-fundo-do-terminal.md), que também revoga, só para este efeito, o "sem passo de filtro em `porecatu-render`, e é definitivo".

## Problema

Com a imagem de fundo da janela (PRD-018) e o terminal translúcido (`[terminal] background_opacity < 1`), a imagem aparece através do quadro do terminal **nítida**, com todo o detalhe dela. O fundo do quadro só deixa passar menos dela; não a deixa menos detalhada. Uma foto com bordas fortes, texto ou alto contraste disputa a leitura com a grade, e o único remédio hoje é baixar a opacidade da imagem até ela quase sumir.

O **acrílico** resolve isso do jeito que os sistemas operacionais resolvem: o que está atrás do painel aparece **desfocado**, tingido pela cor do painel. A imagem continua lá — cor, luz, composição —, mas sem o detalhe que compete com o texto. Fora do quadro (barra de abas, margem, vão entre painéis), ela continua nítida.

Há um conflito que define a forma do recurso. A janela pode ser translúcida (`[appearance.window] opacity < 1`): o app compõe a cena inteira com esse alfa sobre o desktop. Com isso, por trás do acrílico desenhado pelo app apareceria o **desktop nítido**, que o app não tem como desfocar — o efeito sairia errado, metade desfocado, metade não. Por isso o acrílico **só funciona com a janela opaca**, e a tela de configurações não deixa ligá-lo de outro jeito.

## Usuário-alvo

O mesmo do [PRD-017](prd-017-imagem-de-fundo-do-terminal.md) e do [PRD-018](prd-018-imagem-de-fundo-da-janela.md): quem pôs uma imagem na janela, quer vê-la através do terminal e ainda ler o que está na tela.

## Em uma tela

```toml
[terminal.acrylic]
# Só vale com [appearance.window] opacity = 1.0 e com
# [appearance.window.background_image] path preenchido.
enabled = true
# Raio do desfoque, em pixels lógicos, de 1 a 64.
blur = 24
```

O que o usuário vê: a imagem da janela nítida na barra de abas, na barra de status e na margem; dentro do quadro do terminal, a mesma imagem desfocada, tingida pela cor de fundo do terminal na medida de `background_opacity`, com o texto por cima.

## Requisitos funcionais

### O efeito

**RF-19.1** — A chave `[terminal.acrylic] enabled` liga o efeito. **O padrão é `false`**; desligado, o app é desenhado exatamente como hoje.

**RF-19.2** — Ligado, o que aparece **através do quadro do terminal** é a imagem de fundo da janela ([PRD-018](prd-018-imagem-de-fundo-da-janela.md)) **desfocada**. Dentro do quadro, de baixo para cima: a imagem da janela desfocada, o fundo do quadro com a transparência de `background_opacity` (que faz o papel do tingimento), a imagem de fundo do terminal ([PRD-017](prd-017-imagem-de-fundo-do-terminal.md)), **nítida**, e as células.

**RF-19.3** — Fora dos quadros — barra de abas, barra de status, margem e vão entre painéis —, a imagem da janela continua **nítida**. O acrílico é só do terminal.

**RF-19.4** — A imagem desfocada é **uma só por janela**: dois painéis vizinhos mostram pedaços contínuos dela, como mostram da imagem nítida (RF-18.6), e não duas imagens desfocadas cada uma no seu quadro.

**RF-19.5** — O efeito aparece **na medida de `background_opacity`**. Com `background_opacity = 1`, o fundo do quadro é opaco e cobre o acrílico, como cobre a imagem nítida hoje. Isso não bloqueia a opção; é dito na descrição dela e no guia.

**RF-19.6** — Célula com cor de fundo própria faz blend sobre o acrílico com a transparência de `background_opacity`, como já faz sobre a imagem da janela ([ADR-0063](../adr/0063-imagem-da-janela-sem-furo-ate-o-desktop.md)), e cobre a imagem do terminal (RF-17.10).

**RF-19.7** — O acrílico é **desfoque e tingimento**, nada mais: sem a textura de ruído, a camada de luminosidade ou o aumento de saturação do acrílico da Microsoft.

### O desfoque

**RF-19.8** — A chave `blur` é o **raio do desfoque**, em pixels lógicos, de `1` a `64`. **O padrão é `24`.** Fora da faixa, o valor é limitado ao extremo mais próximo e a recarga avisa, como os outros números de configuração.

**RF-19.9** — O raio acompanha a escala da janela: o mesmo `blur` desfoca o mesmo tanto em 100% e em 150%, e mudar a janela de monitor refaz o desfoque sem recarregar a imagem.

### Quando o efeito pode valer

**RF-19.10** — O acrílico só vale com **as duas condições**:

1. `[appearance.window] opacity` **igual a `1.0`** — com a janela translúcida, o desktop apareceria nítido por trás do que o app desfoca;
2. `[appearance.window.background_image] path` **não vazio** — sem imagem, atrás do quadro só há a cor de fundo da janela, e desfocar uma cor lisa não muda nada.

**RF-19.11** — O critério é a **configuração**, não o estado da carga da imagem (a mesma regra do [ADR-0063](../adr/0063-imagem-da-janela-sem-furo-ate-o-desktop.md) §1). Enquanto a imagem carrega, ou se a carga falhou, o acrílico desfoca o fundo da janela, sem efeito visível, e passa a mostrar a imagem quando ela fica pronta. A falha da imagem já tem aviso próprio (RF-18.17); o acrílico não soma outro.

**RF-19.12** — Com `enabled = true` e alguma condição faltando, o app desenha **como se o acrílico estivesse desligado** e mostra um aviso que diz **qual** condição falta (a opacidade da janela, ou a imagem de fundo da janela). O aviso sai uma vez por carga ou recarga da configuração que cria o problema, não a cada frame nem uma vez por janela.

**RF-19.13** — O acrílico **não liga** a transparência da janela nem a muda: quem decide continua sendo o que o [ADR-0063](../adr/0063-imagem-da-janela-sem-furo-ate-o-desktop.md) §2 decide. Com as duas condições do RF-19.10, a janela já nasce opaca.

**RF-19.14** — O acrílico **não aparece na janela de configurações**, que não tem imagem de fundo (RF-18.13).

### Recarga e desempenho

**RF-19.15** — Mudar `enabled` ou `blur` vale **na hora**, em todas as janelas, sem reiniciar e sem redimensionar terminal nenhum (classe A do [ADR-0030](../adr/0030-escopo-do-hot-reload.md)). O mesmo vale quando uma das condições do RF-19.10 passa a valer ou deixa de valer por recarga.

**RF-19.16** — Com o acrílico ligado e nada mudando, **nenhum frame é desenhado** ([ADR-0007](../adr/0007-modelo-de-threading.md)). O desfoque é refeito só quando muda o que entra nele — tamanho ou escala da janela, a imagem da janela (pronta, trocada, `mode`, `opacity`), `blur`, a cor de fundo da janela —, nunca por saída do terminal.

**RF-19.17** — O arranque não espera pelo acrílico: ele aparece junto com a imagem da janela, depois do primeiro prompt (RF-18.20).

### Tela de configurações

**RF-19.18** — A tela de configurações ([PRD-016](prd-016-tela-de-configuracoes.md)) ganha duas opções no grupo **Aparência**, logo depois das três da imagem de fundo da janela: **efeito acrílico** (`terminal.acrylic.enabled`, alternância) e **desfoque do acrílico** (`terminal.acrylic.blur`, campo numérico de `1` a `64`, passo `1`). Cada uma tem Restaurar padrão.

**RF-19.19** — Sem as duas condições do RF-19.10, as **duas** linhas ficam **indisponíveis**: esmaecidas e sem edição, como uma linha somente leitura ([ADR-0060](../adr/0060-anatomia-da-tela-de-configuracoes.md) §2, "indisponível fica esmaecido, nunca ausente"), com uma nota abaixo da descrição dizendo o motivo — "Exige a opacidade da janela em 100%" ou "Exige uma imagem de fundo da janela". Com as duas faltando, a nota fala da opacidade primeiro.

**RF-19.20** — A disponibilidade é avaliada com os valores **ainda não salvos** da própria tela: baixar a opacidade da janela para `0.9` esmaece as linhas do acrílico na hora, e voltá-la a `1.0` as devolve, antes de Salvar.

**RF-19.21** — Esmaecer não apaga nada: o valor que está no arquivo continua lá, e Salvar grava as outras pendências sem tocar nas chaves do acrílico. O app em execução segue o RF-19.12.

**RF-19.22** — Toda frase nova (rótulos, descrições, notas, avisos) vem do catálogo de textos, em todo arquivo de `locales/` ([ADR-0056](../adr/0056-catalogo-de-textos-da-interface.md)).

## Cenários

```gherkin
Cenário: o caso que motiva o recurso
  Dado uma imagem de janela configurada com opacity = 1.0
  E [appearance.window] opacity = 1.0
  E [terminal] background_opacity = 0.7
  E [terminal.acrylic] enabled = true e blur = 24
  Quando o usuário abre o app
  Então dentro do quadro do terminal a imagem da janela aparece desfocada
  E na barra de abas, na barra de status e na margem ela aparece nítida
  E o texto do terminal aparece nítido por cima

Cenário: janela translúcida
  Dado [terminal.acrylic] enabled = true
  E uma imagem de janela configurada
  E [appearance.window] opacity = 0.9
  Quando o app abre
  Então o quadro do terminal é desenhado sem acrílico
  E um aviso diz que o acrílico exige a opacidade da janela em 100%

Cenário: sem imagem de janela
  Dado [terminal.acrylic] enabled = true
  E [appearance.window.background_image] path vazio
  Quando o app abre
  Então o quadro do terminal é desenhado sem acrílico
  E um aviso diz que o acrílico exige uma imagem de fundo da janela

Cenário: a tela de configurações acompanha a opacidade pendente
  Dado a tela de configurações aberta no grupo Aparência
  E uma imagem de janela configurada e a opacidade da janela em 1.0
  Quando o usuário muda a opacidade da janela para 0.9, sem salvar
  Então as linhas "Efeito acrílico" e "Desfoque do acrílico" ficam esmaecidas e sem edição
  E uma nota diz que elas exigem a opacidade da janela em 100%
  Quando o usuário volta a opacidade da janela para 1.0
  Então as duas linhas voltam a ser editáveis

Cenário: a imagem do terminal não é desfocada
  Dado o acrílico ativo e uma imagem de terminal configurada
  Quando o terminal é desenhado
  Então a imagem do terminal aparece nítida
  E por baixo dela a imagem da janela aparece desfocada

Cenário: painéis divididos
  Dado o acrílico ativo
  E uma aba dividida em dois painéis lado a lado
  Quando o terminal é desenhado
  Então a imagem desfocada continua sem costura de um painel para o outro, saltando só o vão
  E o vão entre eles mostra a imagem nítida

Cenário: troca ao vivo
  Dado o app aberto com o acrílico ativo e blur = 24
  Quando o usuário salva blur = 8 no porecatu.toml
  Então em menos de 500 ms toda janela mostra o desfoque menor
  E nenhum terminal é redimensionado

Cenário: redimensionar
  Dado o acrílico ativo com a imagem em mode = "stretch"
  Quando o usuário maximiza a janela
  Então o desfoque cobre a imagem esticada na janela maximizada
  E a imagem não é recarregada do disco

Cenário: terminal ocioso
  Dado o acrílico ativo
  Quando nada acontece no terminal
  Então nenhum frame é desenhado
```

## Fora de escopo

- **Acrílico do sistema operacional** (DWM no Windows, `NSVisualEffectView` no macOS) e qualquer desfoque **do desktop**. O sistema não deixa configurar o raio no Windows, age na janela inteira e não tem equivalente no X11. É o motivo do RF-19.10.
- **Ruído, luminosidade e saturação** do acrílico da Microsoft (RF-19.7).
- **Desfocar a barra de abas, a barra de status, a margem ou o vão**, ou a imagem do terminal.
- **Acrílico nos widgets de chrome** (avisos, menus, tooltip, popovers, editor de grupo, diálogos, barra de busca) e na janela de configurações.
- **Sombra desfocada de verdade.** A sombra continua em camadas (§1.2 da especificação); o passe de desfoque do [ADR-0068](../adr/0068-efeito-acrilico-no-fundo-do-terminal.md) não é aproveitado para ela.
- **Acrílico por aba, por grupo ou por tema**, e cor de tingimento própria: o tingimento é o fundo do terminal que já existe.
- **Animação** do raio ao ligar, desligar ou mudar.

## Métricas de sucesso

- **Ligar o acrílico: uma chave**, com as condições do RF-19.10 atendidas.
- **Frames com o terminal ocioso e o acrílico ativo: zero.**
- **Tempo até o primeiro prompt com o acrílico: igual ao sem ele** (`PORECATU_TRACE`, PRD-011).
- **Custo de refazer o desfoque medido** num redimensionamento contínuo em 1080p e em 4K, e registrado no roadmap.
- **Configuração que derruba o app ou deixa a janela sem desenhar: zero.**
