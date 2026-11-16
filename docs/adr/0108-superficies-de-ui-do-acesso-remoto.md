# ADR-0108 — Superfícies de UI do acesso remoto: grupo nas configurações, QR em `Quad`s, diálogo de pareamento e botão de aparelhos conectados na zona fixa

**Status:** Proposto — aval visual do dono **pendente** (anatomia inteira)
**Data:** 2026-11-15
**Relacionados:** [PRD-016](../prd/prd-016-tela-de-configuracoes.md), [PRD-025](../prd/prd-025-acesso-remoto.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0019](0019-tooltip.md), [ADR-0024](0024-face-de-icones.md), [ADR-0028](0028-o-binario-como-referencia-visual.md), [ADR-0032](0032-interface-do-v1-fechada.md), [ADR-0043](0043-arvore-de-acessibilidade.md), [ADR-0055](0055-botao-e-popover-de-sessoes.md), [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md), [ADR-0060](0060-anatomia-da-tela-de-configuracoes.md), [ADR-0075](0075-superficies-de-ui-do-ssh.md), [ADR-0078](0078-largura-e-quebra-de-texto-do-dialogo.md), [ADR-0082](0082-botoes-da-barra-com-o-tamanho-do-botao-de-menu.md), [ADR-0106](0106-pareamento-identidade-e-autorizacao.md)

## Contexto

O [ADR-0032](0032-interface-do-v1-fechada.md) fechou a interface do v1: superfície nova entra por ADR. O [PRD-025](../prd/prd-025-acesso-remoto.md) pede quatro: um lugar para ligar e configurar o acesso, um QR para parear, uma confirmação de pareamento e um **indicador obrigatório** de que há celular conectado, em toda janela (RF-25.12), que não pode ser desligado enquanto houver conexão.

O que já existe e serve de molde: a anatomia da tela de configurações ([ADR-0060](0060-anatomia-da-tela-de-configuracoes.md)), com o grupo "Perfis" como precedente de painel que tem lista além de linhas ([ADR-0075](0075-superficies-de-ui-do-ssh.md) §6); o campo mascarado do diálogo de segredo (`MaskedFieldState`); o diálogo de confirmação com largura e quebra do [ADR-0078](0078-largura-e-quebra-de-texto-do-dialogo.md); a zona fixa à direita da barra, "reservada para o que a barra ganhar à direita daqui em diante" ([ADR-0055](0055-botao-e-popover-de-sessoes.md)), com o botão e o popover de sessões.

Nada aqui foi visto pelo dono. É proposta, para o aval antes do PR de implementação, e o binário continua normativo depois dele ([ADR-0028](0028-o-binario-como-referencia-visual.md)).

## Decisão

**Um grupo "Acesso remoto" na tela de configurações, com o QR desenhado em `Quad`s dentro do painel; o pedido de pareamento num diálogo de confirmação da janela de configurações; e, enquanto houver aparelho conectado, um terceiro botão na zona fixa da barra de abas, à esquerda do de sessões, que abre um popover com os aparelhos conectados. Valores novos só os do QR.**

### 1. Grupo "Acesso remoto" na tela de configurações

Um grupo novo na guia lateral, **depois de "Perfis"**, com o painel em três seções (rótulo de seção da §2.12 da especificação):

**CONEXÃO**

| Linha | Controle | Chave / destino |
|---|---|---|
| Acesso remoto | Alternância | `[remote] enabled` (pendente, gravado pelo Salvar, [ADR-0058](0058-escrita-do-arquivo-de-configuracao.md)) |
| Endereço do relay | Campo de texto | `[remote] relay_url` (pendente; inválida se não for `wss://`, salvo `ws://` em `localhost`, `127.0.0.1` e `[::1]`, como em [01 — Transporte e relay](https://github.com/porecatu/protocol/blob/main/docs/especificacao/01-transporte-e-relay.md) §1) |
| Token do relay | Campo mascarado (`MaskedFieldState`) | Cofre do sistema, gravado pelo Salvar junto com o arquivo, **nunca** no `porecatu.toml` |
| Estado | Texto, sem controle | Desligado · Conectando · Conectado · Falha: relay inacessível / token recusado / versão incompatível / cofre indisponível |

**PAREAR UM APARELHO**

- Um botão "Mostrar QR code" (anatomia do botão do diálogo), desabilitado — esmaecido, nunca ausente (§2.16) — enquanto o estado não for "Conectado", com a razão na descrição.
- Mostrado, o QR ocupa o lugar do botão (§2), com a contagem regressiva dos 5 minutos abaixo, em 11px **Terciário**, e um botão "Cancelar". Expirado, volta o botão.
- Gerar o QR **não** é edição pendente: é um gesto imediato, como os da lista de perfis.

**APARELHOS PAREADOS**

- Lista com a anatomia da linha do popover de sessões ([ADR-0055](0055-botao-e-popover-de-sessoes.md)): nome (12.5px), abaixo plataforma · pareado em · último acesso (11px **Terciário**), à direita o ponto 6×6 dos indicadores da aba em **Acento** quando o aparelho está conectado agora, e o ✕ de revogar com a anatomia do excluir do popover. Revogar pede confirmação (diálogo, destrutivo).
- Lista vazia: uma linha de texto 11px **Terciário**.
- Abaixo, o botão "Recriar identidade", tom destrutivo do diálogo, com confirmação que diz que todos os aparelhos terão de parear de novo.

Revogar e recriar são imediatos, não pendentes: mexem em `remote_devices.json` e no cofre, não no `porecatu.toml`.

### 2. O QR

- Matriz gerada pelo crate `qrcode` (MIT OR Apache-2.0, compatível com GPLv3), nível de correção **M**, em `porecatu-ui`. Desenho: **um `Quad` por série horizontal de módulos escuros** contíguos (não um por módulo), sobre um `Quad` claro que inclui a zona de silêncio de 4 módulos.
- O lado do módulo é um **número inteiro de pixels físicos** (o maior que caiba no lado alvo), para não haver costura entre `Quad`s — a regra de arredondar ao pixel físico que o projeto já segue.
- **Sempre escuro sobre claro**, nos dois temas: leitor de QR comum não lê o invertido. Isso exige **duas cores novas**, `qr_dark` e `qr_light`, na seção `[remote]` dos temas (`#000000` e `#ffffff` nos dois `thm_*.toml`), e **uma dimensão nova**, `[appearance.remote] qr_size = 208` (lado alvo em px lógicos, zona de silêncio incluída). As três entram nos três lugares (campo com `#[serde(skip_deserializing)]` e linha em `theme::mergeable_fields` para as cores; `porecatu.example.toml`, especificação visual e `VALORES` do `verify-docs.py` para a dimensão) **no PR de implementação**, depois do aval.
- O QR não entra na árvore de acessibilidade como imagem: o nó é um botão "Copiar link de pareamento" que põe a URI na área de transferência, para quem não pode apontar a câmera (o celular abre o link pelo esquema `porecatu://`).

### 3. Diálogo de pareamento

O diálogo de confirmação existente ([ADR-0014](0014-superficie-de-aviso-e-dialogo.md), largura 360 com piso e quebra do [ADR-0078](0078-largura-e-quebra-de-texto-do-dialogo.md)), **modal na janela de configurações**:

- título "Parear este aparelho?";
- corpo: nome do aparelho e plataforma; abaixo, a impressão digital em **Iosevka** (como o campo de senha e os badges), quatro grupos de quatro; abaixo, 11px **Terciário**: "Confira se o celular mostra o mesmo código.";
- botões: **Recusar** (padrão, foco inicial) e **Parear**;
- fecha sozinho em 120 s. No fio, Recusar manda `pair-rejected { user-declined }` e o prazo vencido, `pair-rejected { timeout }` ([02 — Handshake e pareamento](https://github.com/porecatu/protocol/blob/main/docs/especificacao/02-handshake-e-pareamento.md) §3.2); para o usuário do desktop os dois são o mesmo fim: nada foi pareado.

Nenhum valor novo.

### 4. Botão de aparelhos conectados

- **Onde:** zona fixa à direita, **à esquerda do botão de sessões**, separado pelo `trilha_gap` da zona. A ordem fica: trilha · **aparelhos** · sessões · configurações · botões de janela.
- **Quando:** só existe enquanto houver **ao menos um aparelho conectado**, em **toda janela** do processo. Aparecer e sumir recalculam a trilha pela função de layout da barra (nada de PTY ou grade muda). Com o recurso desligado ou sem conexão, a barra é exatamente a de hoje.
- **Anatomia:** a dos botões da zona ([ADR-0082](0082-botoes-da-barra-com-o-tamanho-do-botao-de-menu.md), hover e pressionado do botão de configurações), com o ícone Lucide **`smartphone`** (codepoint em `porecatu_render::icon`) e, com dois ou mais aparelhos, a contagem num **badge** com a anatomia do badge de perfil da aba. Ícone em **Acento**, não no tom neutro dos outros botões: é o único botão da zona que sinaliza estado, e precisa ser visto.
- **Não há chave para escondê-lo** (RF-25.12): `show_*` não existe para este botão, de propósito.
- **Tooltip** ([ADR-0019](0019-tooltip.md)): "N aparelho(s) conectado(s): nome, nome".
- **Clique:** abre um popover com a anatomia do popover de sessões, sem campo de texto: uma linha por aparelho conectado (nome; abaixo, plataforma · conectado há X), com "Desconectar" à direita; abaixo da lista, "Desconectar todos" e "Abrir configurações de acesso remoto". Desconectar não revoga (RF-25.13).
- **Acessibilidade** ([ADR-0043](0043-arvore-de-acessibilidade.md)): nó de botão com o mesmo texto do tooltip, presente e ausente junto com o botão.

### 5. Aviso a cada conexão

RF-25.14: o aviso do app ([ADR-0014](0014-superficie-de-aviso-e-dialogo.md)), severidade informação, título "Aparelho conectado", corpo com o nome, expirando sozinho como os outros de informação, na janela focada. Desconexão não gera aviso.

### 6. Textos

Toda frase acima nasce no catálogo ([ADR-0056](0056-catalogo-de-textos-da-interface.md)): identificadores em `porecatu-ui`, valores em todo arquivo de `locales/`. Terminologia: "aparelho", "parear", "revogar", "relay" (nome do produto, sem tradução).

## Alternativas consideradas

### Indicador como segmento da barra de status

Era o lugar natural para "estado". Recusado porque a barra de status pode ser desligada (`[appearance.status_bar] enabled = false`), e o indicador não pode sumir por configuração. Ligar a barra à força quando alguém conecta mudaria a altura da grade e redimensionaria o PTY.

### Indicador no botão do app

O botão do app já alterna o gerenciador de tarefas e pode ser escondido (`show_app_menu_button`). Sobrecarregar um botão escondível com um aviso obrigatório não serve.

### QR numa janela própria

Daria mais espaço, mas seria uma oitava superfície e mais uma janela modal. O painel de configurações comporta 208 px com folga.

### QR como textura

Rasterizar a matriz numa imagem e desenhá-la como textura. Funciona, mas cria caminho de textura para algo que é uma grade de retângulos; `Quad`s não exigem nada novo do render.

## Consequências

### Positivas

- Nenhuma primitiva nova no render, nenhum widget novo: grupo de configurações, diálogo, botão de zona e popover já existem.
- O desktop nunca esconde que há alguém conectado.

### Negativas

- Três valores visuais novos (duas cores, uma dimensão).
- A zona fixa ganha um terceiro botão, que encurta a trilha enquanto existe.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| QR ilegível em monitor de DPI baixo | Baixa | Médio | Módulo em pixels físicos inteiros; nível M; testar com câmeras reais na verificação ao vivo |
| Botão aparecendo e sumindo deslocar a trilha durante um arraste | Baixa | Baixo | Mudança de layout entra no fim do evento, como qualquer outro recálculo da barra; o arraste já tolera a trilha mudando |
| Aval visual mudar a anatomia | Alta | Baixo | Status "Proposto"; implementação só depois do aval |

## Registro do aval visual

Pendente.
