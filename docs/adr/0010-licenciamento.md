# ADR-0010 — Licenciamento sob GPL-3.0-or-later

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** ADR-0001, ADR-0002, ADR-0004

## Contexto

O README declarava *"licença a definir antes do primeiro release (candidatas: MIT ou Apache-2.0 / MIT dual)"*. Definir agora, antes de existir código e de existirem contribuidores externos, é muito mais barato do que definir depois: relicenciar um projeto exige o consentimento de cada pessoa que já contribuiu.

Duas forças em jogo, e a segunda é a que efetivamente restringe a escolha.

**Intenção do projeto.** O Porecatu quer ser software livre com garantia de que continuará livre — quem receber o binário deve poder obter e modificar o código. Isso aponta para copyleft.

**Compatibilidade com a stack travada.** As dependências já foram decididas em ADRs anteriores e não estão em disputa:

| Dependência | Licença | ADR |
|---|---|---|
| `winit` | Apache-2.0 | [ADR-0001](0001-stack-de-gui.md) |
| `wgpu` | MIT **ou** Apache-2.0 | [ADR-0001](0001-stack-de-gui.md) |
| `glyphon` / `cosmic-text` | MIT ou Apache-2.0 | [ADR-0001](0001-stack-de-gui.md) |
| `alacritty_terminal` | Apache-2.0 | [ADR-0002](0002-motor-vte.md) |
| `portable-pty` | MIT | [ADR-0004](0004-pty-cross-platform.md) |

O ponto crítico: a FSF declara **Apache-2.0 incompatível com a GPLv2**, por causa da cláusula de patentes e dos requisitos de indenização, que impõem restrições que a GPLv2 não prevê. Já a **GPLv3 é compatível com Apache-2.0** — a compatibilidade foi um objetivo explícito da revisão de 2007.

Como `winit` e `alacritty_terminal` são Apache-2.0 sem alternativa de licença dupla, a versão da GPL não é uma preferência estética: é uma restrição técnica.

## Decisão

O Porecatu é licenciado sob **GPL-3.0-or-later** (SPDX: `GPL-3.0-or-later`).

- **Texto da licença:** `LICENSE` na raiz, cópia verbatim de `https://www.gnu.org/licenses/gpl-3.0.txt`, com integridade verificada por hash no CI. O texto de uma licença nunca é editado ou reescrito.
- **Titular do copyright:** Leonardo Otaviano Pedrozo.
- **Cabeçalho por arquivo:** todo arquivo de código-fonte começa com `// SPDX-License-Identifier: GPL-3.0-or-later`. A convenção está registrada em [CLAUDE.md](../../CLAUDE.md) desde antes de existir o primeiro `.rs`, justamente para não precisar varrer o repositório depois.
- **"or later":** contribuições futuras não ficam presas à v3. Se a FSF publicar uma v4, a migração não exigirá localizar e obter consentimento de cada contribuidor — problema que já inviabilizou relicenciamentos em outros projetos.
- **Contribuições:** entram sob os mesmos termos, conforme [CONTRIBUTING.md](../../CONTRIBUTING.md). Sem CLA: o `git log` é o registro de autoria.

## Alternativas consideradas

### MIT, ou dual Apache-2.0/MIT

O padrão de fato do ecossistema Rust — é o que quase toda a stack usa. Maximiza adoção: qualquer projeto, inclusive proprietário, pode incorporar o código sem obrigações práticas.

Descartada por não oferecer a garantia que o projeto quer. Sob MIT, um fork proprietário com melhorias fechadas é perfeitamente lícito, e os usuários desse fork não teriam acesso ao código. Para uma ferramenta de trabalho diário como um emulador de terminal, garantir que melhorias permaneçam disponíveis é escolha deliberada, com custo conhecido.

### GPLv2

Descartada por incompatibilidade real, não por preferência: `winit` e `alacritty_terminal` são Apache-2.0. Adotar a GPLv2 exigiria abandonar as duas e reabrir [ADR-0001](0001-stack-de-gui.md) e [ADR-0002](0002-motor-vte.md) — trocando a stack de janela e o motor VT inteiros por causa da licença. Custo desproporcional a qualquer benefício.

### LGPL-3.0

Copyleft mais fraco, pensado para bibliotecas que precisam ser vinculadas a software proprietário. O Porecatu é uma aplicação de usuário final, não uma biblioteca; a flexibilização que a LGPL oferece não tem a quem servir aqui.

### AGPL-3.0

Acrescenta o §13: quem oferece o software como serviço pela rede deve disponibilizar o fonte aos usuários desse serviço.

Descartada porque a cláusula nunca dispararia. O Porecatu é um aplicativo desktop que roda na máquina do usuário; não há uso "pela rede" a cobrir. O efeito prático seria apenas afastar adoção corporativa — muitas empresas proíbem AGPL por política, sem análise caso a caso — em troca de uma proteção que, para este produto, é vazia.

### Deixar para depois

Descartada pelo custo assimétrico. Definir agora custa um arquivo e um ADR. Definir depois de o projeto ter contribuidores exige localizar cada um e obter consentimento — e basta uma pessoa não responder para travar tudo.

## Consequências

### Positivas

- Melhorias distribuídas em binário voltam para a comunidade; forks proprietários fechados não são possíveis.
- Compatível com toda a stack já decidida, sem reabrir nenhum ADR.
- `LICENSE` verbatim com verificação de hash elimina uma classe de problema: texto de licença adulterado por acidente de editor ou de fim de linha.
- Decidido antes do primeiro contribuidor externo — o momento mais barato possível.
- O `or later` mantém aberta uma porta que costuma fechar em definitivo.

### Negativas

- **Copyleft forte reduz adoção corporativa.** Empresas com política restritiva a GPL não incorporarão o código nem contribuirão. Isto é escolha, não efeito colateral — mas é real, e o projeto abre mão de uma parcela de contribuidores por causa dela.
- Fora do padrão do ecossistema Rust (majoritariamente MIT/Apache-2.0), o que pode surpreender quem chega.
- Nenhum código do Porecatu poderá ser reaproveitado por projetos MIT ou Apache-2.0 — a via de mão única do copyleft.
- Cabeçalhos SPDX exigem disciplina em cada arquivo novo.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Dependência futura com licença incompatível com GPLv3 | Média | Alto | Verificar licença antes de adotar qualquer crate novo; item do checklist de PR |
| Arquivo sem cabeçalho SPDX | Alta | Baixo | Convenção em CLAUDE.md e no template de PR; checagem automatizável na F0 |
| `LICENSE` alterado por acidente (fim de linha, editor) | Média | Médio | Verificação de hash no workflow `docs`; `.gitattributes` normaliza para LF |
| Adoção menor por causa da GPL | Média | Médio | Aceito. É a contrapartida consciente da garantia de liberdade |
| Pressão futura para relicenciar | Baixa | Alto | `or later` cobre mudança de versão; mudar de família de licença exigiria consentimento de todos, e ADR novo |
