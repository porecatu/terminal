# PRD-007 — Perfis de aba

**Status:** Rascunho — fase v2
**Data:** 2026-08-26
**Relacionados:** [ADR-0004](../adr/0004-pty-cross-platform.md), [ADR-0009](../adr/0009-referencia-visual-e-reconciliacao.md), [PRD-001](prd-001-abas.md), [PRD-004](prd-004-aparencia-do-chrome.md)

> Rascunho. Existe para dar endereço a um elemento desenhado no canvas mas **fora do escopo do v1** ([PRD-000](prd-000-visao-de-produto.md)). Não implementar antes de o documento ser promovido a Aprovado.

## Problema

No v1, uma aba nova abre o shell padrão no diretório da aba ativa. Isso cobre o caso comum e nada além dele.

Quem trabalha em Windows raramente quer só PowerShell: quer PowerShell, WSL, `cmd` para uma ferramenta legada, e um `ssh` para o servidor de produção. Hoje, cada um desses exige abrir uma aba e digitar o comando à mão, toda vez.

Perfis transformam isso em uma escolha. E, por tabela, resolvem um segundo problema: **identificar visualmente o tipo de terminal na barra**, que é justamente o que o badge do design faz.

## O que o design mostra

Ver [especificação visual](../design/especificacao-visual.md), seções 2.5, 2.9 e 2.13.

Seis perfis de exemplo:

| Badge | Nome | Comando | Cor | Sistema |
|---|---|---|---|---|
| `PS` | Windows PowerShell | `pwsh.exe -NoLogo` | `#6fa8f5` | Windows |
| `CMD` | Prompt de Comando | `cmd.exe /k` | `#8b929e` | Windows |
| `WSL` | Ubuntu 24.04 | `wsl.exe -d Ubuntu` | `#e0b060` | Linux |
| `SSH` | prod-web-01 | `ssh deploy@10.4.1.20` | `#ef8a8a` | Remoto |
| `ZSH` | macOS zsh | `/bin/zsh -l` | `#a68cf0` | macOS |
| `PY` | Python 3.12 | `python -i` | `#5ed3bc` | Multi |

Três superfícies:

- **Badge na aba** — mono 9px, na cor do grupo (não do perfil), fundo tingido `.14`
- **Menu de perfis** — popover ao clicar no `+`, com nome, badge e tecla; ao final, "Novo grupo de abas"
- **Tela de nova aba** — grade de cards quando não há aba aberta

Note a decisão de cor: no design, o badge usa a cor **do grupo**, não a do perfil. A cor do perfil aparece só no menu e nas configurações. Mantém a barra legível por grupo, que é o eixo principal de organização.

## Requisitos esboçados

- **RF-7.1** — Perfis definidos no arquivo de config: nome, comando, argumentos, ambiente, diretório inicial, badge, cor, ícone opcional.
- **RF-7.2** — Perfil padrão por plataforma, usado quando nada é escolhido.
- **RF-7.3** — Escolher perfil ao abrir aba: menu no `+`, tela de nova aba, ou atalho direto.
- **RF-7.4** — Badge do perfil exibido na aba, ligável e desligável na config ([PRD-004](prd-004-aparencia-do-chrome.md) RF-4.23).
- **RF-7.5** — Perfil registrado na sessão e restaurado ([PRD-003](prd-003-persistencia-de-sessao.md) já grava o programa de spawn quando difere do padrão).
- **RF-7.6** — Detecção automática de perfis disponíveis: distribuições WSL instaladas, PowerShell 7, shells em `/etc/shells`.
- **RF-7.7** — Atalhos por perfil, obedecendo [ADR-0008](../adr/0008-teclas-e-roteamento-de-input.md) — nada de `Ctrl+<número>` sozinho se colidir com o terminal.

## Questões em aberto

- Perfil determina também tema de cores da aba?
- Perfil de `ssh` guarda credenciais? (Provável não — deixar para o `ssh_config`.)
- Perfil pode fixar um grupo de destino?
- Detecção automática roda no start ou sob demanda?

## Fora de escopo

Gerenciar conexões SSH (host, chave, porta) dentro do app; perfis de container com ciclo de vida; sincronizar perfis entre máquinas.
