# PRD-003 — Persistência de sessão

**Status:** Aprovado
**Data:** 2026-08-26
**Requisito de origem:** 3 — ao fechar e reabrir o emulador, as abas e grupos devem voltar abertos nos mesmos diretórios (sem os processos)
**Relacionados:** [ADR-0005](../adr/0005-persistencia-de-sessao.md), [ADR-0004](../adr/0004-pty-cross-platform.md), [PRD-002](prd-002-grupos-de-abas.md)

## Problema

Organizar quinze terminais em grupos nomeados custa tempo. Se esse trabalho evapora ao fechar a janela, o recurso de grupos ([PRD-002](prd-002-grupos-de-abas.md)) vira um brinquedo — ninguém investe em organizar o que se perde no próximo reboot.

Hoje, reconstruir o contexto após fechar o emulador é trabalho manual feito de memória: abrir N abas, `cd` em cada uma, reagrupar. Vários minutos, toda vez, e quase sempre incompleto.

Persistência de sessão é o que transforma grupos de enfeite em ferramenta.

## Usuário-alvo

Todo usuário que criou mais de duas abas. O valor cresce com o tamanho da sessão.

## O que é restaurado

| Item | Restaurado |
|---|---|
| Geometria e monitor de cada janela | Sim |
| Grupos: ordem, nome, cor, estado de colapso | Sim |
| Abas: ordem dentro do grupo | Sim |
| Diretório de trabalho de cada aba | Sim — **com ressalva importante, ver abaixo** |
| Título customizado da aba | Sim |
| Aba ativa de cada janela | Sim |
| Programa de spawn diferente do shell padrão | Sim |
| **Processos em execução** | **Não** — por definição do requisito |
| **Conteúdo do scrollback** | **Não** — fora do escopo do v1 |
| Histórico de comandos | Não — é do shell, não nosso |

Processos não são restaurados por decisão explícita, não por limitação: reexecutar automaticamente o comando que estava rodando não é seguro. Restaurar um `rm -rf` ou um `terraform apply` interrompido seria pior que não restaurar nada.

## A ressalva do diretório de trabalho

Esta seção é a limitação conhecida mais relevante do produto. Está aqui, no PRD, e não escondida no ADR, porque afeta diretamente o que o usuário percebe.

Descobrir o diretório atual de uma aba exige que o **shell informe** esse diretório ao emulador, através da sequência **OSC 7**. É assim que todos os emuladores modernos fazem, e não há alternativa confiável.

| Situação | Resultado da restauração |
|---|---|
| Shell emite OSC 7 (fish, starship, ou hook configurado) | Diretório exato onde o usuário estava |
| Linux sem OSC 7 | Diretório correto via fallback `/proc` |
| macOS sem OSC 7 | Diretório correto via fallback `libproc` |
| **Windows sem OSC 7** | **Diretório de abertura da aba**, não o atual |

No Windows não existe fallback: não há API viável para ler o diretório de outro processo ([ADR-0005](../adr/0005-persistencia-de-sessao.md) detalha por que as opções existentes foram rejeitadas).

**RF-3.1** — Quando o app detecta que uma aba nunca emitiu OSC 7, ele oferece **uma vez**, de forma não intrusiva e dispensável em definitivo, o trecho de configuração adequado ao shell detectado. No Windows esse convite é mais proeminente, porque lá ele não é uma melhoria — é a condição para o recurso funcionar.

## Requisitos funcionais

### Gravação

**RF-3.2** — A sessão é gravada automaticamente após qualquer mudança estrutural: abrir, fechar, reordenar ou renomear aba; criar, dissolver, renomear, recolorir ou colapsar grupo; mudar diretório de aba; mover ou redimensionar janela.

**RF-3.3** — A gravação é agrupada com atraso de cerca de 2 segundos. Fechar dez abas em sequência gera uma escrita, não dez.

**RF-3.4** — Ao encerrar o app, a sessão é gravada de forma síncrona antes da saída.

**RF-3.5** — A gravação é atômica: arquivo temporário, `fsync`, `rename`. Um crash durante a escrita preserva a sessão anterior íntegra.

**RF-3.6** — O usuário desliga a persistência na config. Desligada, nada é gravado e o app sempre abre com uma aba limpa.

### Restauração

**RF-3.7** — Ao abrir sem argumentos, o app restaura a última sessão gravada.

**RF-3.8** — Restauração **preguiçosa**: apenas a aba ativa de cada janela tem seu shell iniciado no start. As demais mostram a aba na barra, com seu título e grupo, e iniciam o shell ao serem focadas pela primeira vez. *(É o que permite restaurar 50 abas rápido, em vez de disparar 50 processos de uma vez.)*

**RF-3.9** — Uma aba ainda não iniciada é visualmente distinguível de uma aba com shell rodando — discretamente, sem poluir a barra.

**RF-3.10** — Se o diretório gravado não existe mais, a aba abre no diretório home e informa isso na primeira linha. A estrutura de grupos é preservada.

**RF-3.11** — Se o monitor onde a janela estava não existe mais, a janela é reposicionada no monitor primário, com tamanho preservado dentro dos limites da tela.

**RF-3.12** — Abrir o app com um caminho como argumento cria uma sessão nova naquele diretório, **sem** restaurar e **sem** sobrescrever a sessão gravada.

### Robustez

**RF-3.13** — Arquivo de sessão ausente é situação normal, não erro: o app abre com uma aba no home.

**RF-3.14** — Arquivo de sessão inválido ou truncado é preservado com o sufixo `.corrupt`, o app abre uma sessão nova e informa o ocorrido.

**RF-3.15** — Arquivo de sessão de uma versão de schema mais antiga é migrado automaticamente e regravado na versão atual.

**RF-3.16** — Arquivo de sessão de uma versão **mais nova** que a suportada **não é sobrescrito**. O app abre sessão nova, preserva o arquivo e avisa. *(Impede que uma versão antiga do app destrua o estado de uma mais nova.)*

**RF-3.17** — Múltiplas janelas são gravadas e restauradas como um conjunto.

## Critérios de aceite

```gherkin
Cenário: restauração completa da estrutura
  Dado uma sessão com dois grupos nomeados e cinco abas em diretórios distintos
  E os shells emitindo OSC 7
  Quando o usuário fecha o app e o reabre
  Então os dois grupos voltam com nome, cor e estado de colapso
  E as cinco abas voltam na mesma ordem
  E cada aba abre no diretório em que estava
  E nenhum processo anterior é reexecutado

Cenário: restauração preguiçosa
  Dado uma sessão gravada com vinte abas
  Quando o usuário reabre o app
  Então apenas o shell da aba ativa é iniciado
  E as outras dezenove aparecem na barra sem processo
  E o shell de uma aba inicia quando ela é focada

Cenário: diretório removido
  Dado uma aba gravada em /tmp/build-123
  E esse diretório não existe mais
  Quando a sessão é restaurada
  Então a aba abre no diretório home
  E informa que o diretório original não foi encontrado
  E permanece no seu grupo original

Cenário: crash não corrompe a sessão
  Dado uma sessão gravada válida
  Quando o app é encerrado à força durante uma gravação
  Então a sessão anterior permanece íntegra e restaurável

Cenário: arquivo corrompido
  Dado um arquivo de sessão com JSON inválido
  Quando o usuário abre o app
  Então o arquivo é preservado como session.json.corrupt
  E o app abre com uma aba nova
  E informa o ocorrido

Cenário: versão de schema mais nova é preservada
  Dado um arquivo de sessão com schema_version maior que a suportada
  Quando o usuário abre o app
  Então o arquivo não é sobrescrito
  E o app abre uma sessão nova avisando o motivo

Cenário: Windows sem OSC 7
  Dado o Windows com um shell que não emite OSC 7
  E uma aba aberta em C:\Projetos e navegada até C:\Projetos\app\src
  Quando a sessão é restaurada
  Então a aba abre em C:\Projetos
  E o app oferece uma vez o trecho de integração do shell

Cenário: abrir com caminho não afeta a sessão
  Dado uma sessão gravada com dez abas
  Quando o usuário abre o app passando /tmp como argumento
  Então uma janela nova abre com uma aba em /tmp
  E a sessão gravada permanece intacta
```

## Fora de escopo

- Persistir conteúdo do scrollback (aumentaria o arquivo em ordens de grandeza; revisitar em v2)
- Restaurar processos em execução (decisão explícita, não limitação)
- Múltiplas sessões nomeadas, salvas e alternáveis (ideia para v2)
- Sincronizar sessão entre máquinas
- Anexar a sessão remota, estilo `tmux`

## Métricas de sucesso

| Métrica | Alvo |
|---|---|
| Ações do usuário para reconstruir o contexto após reabrir | **zero** |
| Tempo de restauração de 20 abas até janela interativa | < 1 s |
| Abas cujo diretório é restaurado corretamente, com OSC 7 | 100% |
| Sessões perdidas por corrupção | 0 |
| Tamanho do arquivo com 50 abas | < 100 KB |

A primeira métrica é a razão de ser deste PRD. Se o usuário ainda precisa reorganizar alguma coisa depois de reabrir, o recurso falhou.
