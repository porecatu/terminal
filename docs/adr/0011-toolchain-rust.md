# ADR-0011 — Toolchain Rust pinada e política de versão

**Status:** Aceito
**Data:** 2026-08-26
**Relacionados:** ADR-0001, ADR-0002

## Contexto

O projeto pina versões com igualdade exata onde a instabilidade custa caro: `wgpu` quebra API a cada release ([ADR-0001](0001-stack-de-gui.md)) e `alacritty_terminal` não segue SemVer estável ([ADR-0002](0002-motor-vte.md)). Nos dois casos a decisão registrada é a mesma: atualizar é tarefa deliberada, nunca efeito colateral.

O compilador ficou de fora dessa disciplina. O `.github/workflows/ci.yml` usa `dtolnay/rust-toolchain@stable` — canal flutuante — combinado com `RUSTFLAGS: -D warnings` e `cargo clippy -- -D warnings`. As duas coisas juntas produzem um modo de falha específico:

**Uma release de Rust com lint novo do clippy derruba um commit que estava verde, sem ninguém ter tocado no código.** O PR do dia seguinte falha em código que não mudou, e quem abriu o PR gasta a manhã descobrindo que o problema não é dele. Com `-D warnings`, todo lint novo é uma quebra de build, não um aviso.

Há um agravante de ambiente. O desenvolvimento primário é Windows e o CI roda nas três plataformas; sem pin, cada máquina de contribuidor compila com um rustc diferente do CI, e o erro aparece só no PR.

Duas decisões acessórias precisam de resposta antes do primeiro `Cargo.toml`, porque mudá-las depois toca todos os crates: qual edition, e onde os lints ficam declarados.

## Decisão

**A toolchain é pinada no repositório. Subir de versão é tarefa própria, com changelog na mão** — a mesma regra já aplicada a `wgpu` e ao motor VT.

### `rust-toolchain.toml` na raiz

```toml
[toolchain]
channel = "1.XX.Y"          # stable vigente no início da F0
components = ["rustfmt", "clippy"]
profile = "minimal"
```

O número é preenchido pelo commit da F0, com o `rustc --version` do dia. **Não é inventado agora**: registrar uma versão que talvez nem exista quando a F0 começar seria pior que não registrar nada.

O `rustup` honra este arquivo automaticamente em qualquer invocação de `cargo` dentro do repositório. Isso alinha, sem esforço adicional, as máquinas dos contribuidores e o CI.

### MSRV igual ao canal pinado

`workspace.package.rust-version` recebe o mesmo valor do `channel`, herdado pelos crates com `rust-version.workspace = true`.

O Porecatu é **aplicação, não biblioteca**: não há consumidor externo a quem prometer compatibilidade retroativa, e por isso não existe motivo para carregar uma MSRV conservadora. O campo existe por um motivo único e prático: quem tentar compilar com um rustc antigo recebe *"requires rustc 1.XX"* em vez de um erro de sintaxe incompreensível no meio de um arquivo.

### Edition 2024

Escolhida por ser a edition estável no momento da decisão. Migrar edition depois é mecânico mas mexe em todos os crates de uma vez; começar na atual evita a tarefa.

### Lints no workspace, não espalhados no código

```toml
[workspace.lints.clippy]
# ...
[workspace.lints.rust]
# ...
```

Cada crate herda com `lints.workspace = true`. A alternativa — `#![deny(...)]` no topo de cada `lib.rs` — espalha a mesma política por oito arquivos e garante que eles divirjam. Um só lugar para ler e para mudar.

O contrato de comandos do [CLAUDE.md](../../CLAUDE.md) não muda: `fmt --check`, `clippy -D warnings`, `build`, `test`, nas três plataformas.

### O CI usa a versão pinada, não `stable`

A matriz do `ci.yml` passa a instalar exatamente a versão do `rust-toolchain.toml`. Isso importa mais do que parece: se a action instalar `stable` e o arquivo pinar outra versão, o shim do `rustup` baixa a pinada na primeira invocação de `cargo` — o CI passa a baixar **duas** toolchains por job e o cache guarda a errada.

O mecanismo exato (input `toolchain:` da action atual, ou trocar por uma action que leia o arquivo) é detalhe a confirmar no commit da F0, junto da verificação das dependências de sistema do Linux que o roadmap já prevê.

### Job canário

Um job separado, **fora do caminho de PR**: agendamento semanal mais `workflow_dispatch`, rodando a mesma matriz contra `stable`, com `continue-on-error: true`.

É o que transforma "lint novo derruba PR alheio" em "lint novo aparece num job que já se sabe que pode falhar". A atualização passa a ser agendada em vez de descoberta.

Escrito agora e dormindo até a F0, pela mesma estratégia que o `ci.yml` e o `release.yml` já usam.

## Alternativas consideradas

### Manter `stable` flutuante (status quo)

Zero manutenção: o projeto acompanha o compilador de graça e nunca fica defasado.

Descartada pelo modo de falha descrito no contexto. Com `-D warnings`, "acompanhar de graça" significa que uma release do Rust pode quebrar o CI num commit que não mudou nada — e a pessoa que sofre é a próxima a abrir PR, não quem escolheu a política. Incoerente com o resto do repositório: seria o único item da stack sem pin, e o mais capaz de quebrar tudo de uma vez.

### MSRV conservadora, algumas releases atrás da stable

Prática correta em biblioteca: amplia quem consegue compilar, e é o que distros pedem para empacotar.

Descartada porque o benefício não existe aqui. Não há consumidor de API, e o custo é concreto: renunciar a recursos de linguagem e de `cargo` por compatibilidade com ninguém em particular. Se algum dia um empacotador de distro pedir uma MSRV mais baixa, isso é um ADR novo com um pedido real por trás, não especulação agora.

### `nightly`

Daria acesso a features instáveis e ao `-Z` de ferramentas.

Descartada por não haver demanda: nenhuma peça da stack travada nos ADRs anteriores exige nightly. Em troca viria instabilidade diária e um CI que quebra por motivos alheios ao projeto.

### Pinar sem canário

Mais simples: pina e pronto.

Descartada porque pin sem mecanismo de alerta apodrece. Sem sinal periódico, a defasagem só aparece quando alguém tenta subir a versão e encontra dois anos de lints acumulados de uma vez — exatamente o custo que o pin queria evitar, só concentrado num dia ruim.

## Consequências

### Positivas

- Build reproduzível: CI e máquinas de contribuidores usam o mesmo rustc, sem combinar nada.
- Release do Rust não quebra PR de terceiro. A atualização vira tarefa agendada.
- Coerência com ADR-0001 e ADR-0002: tudo que quebra API está pinado.
- Lints num só lugar, herdados; um `Cargo.toml` para auditar em vez de oito arquivos.
- MSRV declarada dá erro legível a quem compila com toolchain velha.

### Negativas

- Mais um arquivo a manter atualizado, e o canário é um job de CI que consome minutos sem bloquear nada.
- Subir a toolchain vira tarefa explícita no backlog. Isso é o objetivo, mas é trabalho que antes não existia.
- Contribuidor com toolchain fixada por outro motivo (distro, ambiente corporativo) tem o download da versão pinada forçado pelo `rustup`.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Pin virar defasagem esquecida | Alta | Médio | Job canário semanal; a falha dele é o lembrete |
| `rust-toolchain.toml` e `rust-version` divergirem | Média | Baixo | Preencher os dois no mesmo commit; checagem no `verify-docs.py` é ideia registrada, não implementada agora |
| CI baixar duas toolchains por job | Média | Baixo | A matriz instala a versão pinada, não `stable`; conferir no commit da F0 |
| Lints acumulados na primeira subida de versão | Média | Médio | Canário expõe cedo; subir uma versão por vez, nunca várias de uma vez |
| Edition 2024 esconder incompatibilidade de dependência pinada | Baixa | Médio | `cargo build --workspace` na F0 é o teste; `wgpu` e `alacritty_terminal` já suportam a edition atual |
