# ADR-0045 — A primeira versão publicada é `0.7.0`, não `1.0.0`

**Status:** Aceito
**Data:** 2026-09-04
**Relacionados:** [ADR-0044](0044-empacotamento-e-release.md)
**Supersedes:** ADR-0044 §3 (parcial — só o número da primeira versão; o resto do ADR-0044 continua valendo por inteiro)

## Contexto

O [ADR-0044](0044-empacotamento-e-release.md) §3 decidiu que a primeira release do Porecatu sairia como `1.0.0`, com o raciocínio de que a superfície pública (config, sessão, ações, linha de comando) já tem regra de compatibilidade escrita — então `1.0.0` seria honesto, não bravata.

Ao fechar a etapa 6 da F6, e com ela o roadmap inteiro do v1, o dono do produto revisitou esse número. A F6 fecha com dívida de verificação real, não hipotética: instalação de verdade em macOS e Linux nunca aconteceu neste fluxo (só compilação e CI), o leitor de tela foi confirmado só por UI Automation direto (NVDA de verdade não estava instalado na máquina do teste), e toda fase anterior carrega a mesma limitação de teclado sintético bloqueado pela proteção de foco do Windows. Some-se a isso bugs conhecidos deixados para depois e melhorias que o dono do produto ainda quer fazer antes de considerar o app usável por terceiros — nenhuma delas é requisito não implementado (o PRD-011 fechou com os RFs que tinha), mas juntas descrevem um produto que ainda não passou pelo teste de "alguém de fora usa isto".

`1.0.0` promete que remover uma chave de config, uma ação do catálogo, ou quebrar o formato de sessão passa a exigir uma major. Prometer isso agora, com essa dívida ainda em aberto, seria compromisso cedo demais — a decisão técnica do ADR-0044 §3 (a superfície é pequena e já tem regra) continua correta; o que muda é se **este momento** é a hora de fazer essa promessa, e essa segunda pergunta é do dono do produto, não da arquitetura.

## Decisão

**A primeira versão publicada é `0.7.0`.** Só o número muda — nada do resto do ADR-0044 é revisto: instalador nativo por plataforma, o mesmo `release.yml` orquestrando, sem assinatura de código, o mesmo conteúdo de artefato (executável, `LICENSE`, as duas atribuições de fonte, `porecatu.example.toml`, `sha256`).

SemVer `0.x` não promete estabilidade entre versões menores — é exatamente o espaço que falta agora. Quando a dívida de verificação listada no roadmap fechar e o dono do produto decidir que o app está pronto para o compromisso de compatibilidade, um ADR novo revisita o número (`1.0.0` ou outro) com o raciocínio do ADR-0044 §3 já escrito e pronto para valer.

## Alternativas consideradas

### Manter `1.0.0` (ADR-0044 §3 como está)

É o que a superfície pública já suportaria tecnicamente. Rejeitada: a pergunta não é "a superfície aguenta a promessa", é "o dono do produto quer fazer a promessa agora" — e a resposta, dada a dívida listada no Contexto, é não.

### `0.2.0` (incremento mínimo a partir de `0.1.0`)

Mais conservador. Não foi o valor pedido; `0.7.0` comunica que seis fases de trabalho substancial já aconteceram, sem chegar perto de prometer o que `1.0.0` prometeria.

## Consequências

### Positivas

- Nenhuma promessa de compatibilidade de major que a dívida atual não sustenta.
- `0.x` é honesto sobre onde o projeto está: seis fases entregues (F0 a F6), dívida de verificação real e melhorias que o próprio dono do produto ainda quer fazer — tudo registrado no [roadmap](../roadmap.md).
- O raciocínio técnico do ADR-0044 §3 não é jogado fora — fica pronto para quando o número for revisitado.

### Negativas

- Todo lugar que já dizia `1.0.0` (CHANGELOG, roadmap, README, CLAUDE.md) precisa da mesma correção — feito na mesma leva desta etapa. O corpo do ADR-0044 em si **não** é editado (decisão aceita não se edita); só o `Status` no topo aponta para este ADR.
- "v1" continua sendo o apelido do produto completo desde o roadmap da F0 (sete fases, todas fechadas com esta etapa); o número de versão deixa de bater com esse apelido. Mitigação: CHANGELOG e roadmap explicam que "v1" nomeia o escopo de produto, e `0.7.0` é o número SemVer — coisas diferentes, e a confusão só existiria por coincidência de nome.
