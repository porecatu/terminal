# ADR-0106 — Pareamento, identidade e autorização: QR na tela de configurações, confirmação no desktop, lista de aparelhos escrita só por gesto

**Status:** Proposto
**Data:** 2026-11-15
**Relacionados:** [PRD-025](../prd/prd-025-acesso-remoto.md), [ADR-0014](0014-superficie-de-aviso-e-dialogo.md), [ADR-0073](0073-credenciais-no-cofre-do-sistema.md), [ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md), [ADR-0103](0103-acesso-remoto-tres-partes-e-protocolo-externo.md), [ADR-0104](0104-crate-porecatu-remote.md), [ADR-0108](0108-superficies-de-ui-do-acesso-remoto.md)

## Contexto

Quem controla uma sessão remota digita num terminal do desktop: é execução de código com o usuário dele. O dono decidiu que um aparelho pareado acessa **tudo**, sem permissão por aparelho; a segurança inteira está, portanto, em **quem consegue parear** e em **quão rápido dá para desfazer**.

O protocolo já fixa a mecânica ([02 — Handshake e pareamento](https://github.com/porecatu/protocol/blob/main/docs/especificacao/02-handshake-e-pareamento.md)): chave estática X25519 e chave de roteamento Ed25519 do host; QR com relay, `host_id`, chave pública e um token de uso único de 5 minutos; handshake Noise `XX` para parear e `IK` depois; confirmação no host com nome e impressão digital do aparelho; `not-paired` e `revoked` como erros. Este ADR decide **como o Porecatu cumpre o lado do host**.

## Decisão

**O QR só existe por gesto na tela de configurações; o pareamento só se conclui com confirmação no desktop; a lista de aparelhos pareados é um arquivo próprio escrito só por gesto; desligar, desconectar e revogar derrubam conexões na hora.**

### 1. Identidade do host

- As duas chaves do host são geradas por `porecatu-remote` na primeira vez que o acesso é ligado, e guardadas no cofre do sistema pelo módulo `vault` do crate ([ADR-0104](0104-crate-porecatu-remote.md) §5). Nunca vão para arquivo nem para log.
- **Recriar identidade** (RF-25.11) gera as duas de novo, num gesto só, e esvazia a lista de aparelhos — todos os pareamentos ficam inválidos de uma vez, porque os aparelhos guardaram a chave antiga e o `host_id` antigo.
- Cofre indisponível (Linux sem serviço de segredos): o acesso remoto **não liga**, com o motivo na tela de configurações. Não há fallback em arquivo: uma chave que dá acesso de digitação ao desktop não fica em texto no disco.

### 2. O QR

- Só aparece por **gesto explícito** no grupo "Acesso remoto" da tela de configurações ([ADR-0108](0108-superficies-de-ui-do-acesso-remoto.md)), com o acesso ligado e a conexão com o relay de pé (sem relay, o QR não serviria e não é mostrado).
- O token vive 5 minutos, é de uso único e há no máximo um vivo: mostrar de novo, fechar a tela de configurações ou trocar de grupo nela invalida o anterior. A contagem regressiva é visível.
- O QR é desenhado com `Quad`s ([ADR-0108](0108-superficies-de-ui-do-acesso-remoto.md) §2) a partir da matriz de módulos gerada pelo crate `qrcode` (MIT OR Apache-2.0), em `porecatu-ui`. Nenhuma primitiva nova no render.

### 3. Confirmação

- O pedido de pareamento (token válido) chega como `RemoteEvent` e abre um **diálogo modal** ([ADR-0014](0014-superficie-de-aviso-e-dialogo.md), largura e quebra do [ADR-0078](0078-largura-e-quebra-de-texto-do-dialogo.md)) **na janela de configurações**, que é onde o QR está — quem leu o QR está na frente dela. O diálogo mostra o nome do aparelho, a plataforma e a impressão digital em quatro grupos de quatro.
- Botão padrão é **Recusar**; confirmar exige o botão "Parear". `Esc` recusa.
- Sem resposta em 120 s, o diálogo fecha sozinho e o pedido é recusado (prazo no `WaitUntil`, como os outros temporizadores de UI).
- Um segundo pedido chegando com um diálogo aberto é recusado (`busy` no host, `pair-rejected` no cliente): não há fila de confirmações.

### 4. Conexão de aparelho pareado

- O handshake `IK` traz a chave estática do aparelho; `porecatu-remote` confere contra a lista **na thread dele**, sem perguntar à main thread. Desconhecido: completa o handshake, manda `error { not-paired }` e fecha.
- Conectado: `RemoteEvent::ClientConnected` com o aparelho; a main thread atualiza o indicador (RF-25.12) e empilha o aviso de informação (RF-25.14).

### 5. `remote_devices.json`

Ao lado do `session.json`, no mesmo diretório de dados, com o padrão do `pinned.json` ([ADR-0086](0086-abas-fixadas-modelo-arquivo-e-ciclo-de-vida.md)):

```json
{
  "schema_version": 1,
  "devices": [
    {
      "key": "base64url da chave estática X25519 do aparelho",
      "name": "Pixel 9",
      "platform": "android",
      "paired_at": "2026-11-15T14:03:00Z",
      "last_seen": "2026-11-15T18:40:12Z"
    }
  ]
}
```

- **Escrito só por gesto**: confirmar pareamento, revogar, recriar identidade. `last_seen` é a exceção: atualizado ao fim de cada sessão de um aparelho, na mesma thread.
- Escrita atômica (arquivo temporário + renomear), na thread de `porecatu-remote`.
- Só chaves **públicas**: o arquivo não dá acesso a nada se vazar. Mas quem consegue **escrever** nele acrescenta um aparelho; o mesmo vale para `porecatu.toml` e `session.json`, e o modelo de ameaça do protocolo deixa malware no desktop fora do escopo.
- Arquivo ilegível: o acesso liga com a lista vazia e um aviso, e o arquivo ruim é renomeado com sufixo `.corrompido-<data>`, nunca sobrescrito — a regra "nunca perder trabalho" do PRD-000 aplicada a uma lista que o usuário construiu.

### 6. Desfazer

| Gesto | Efeito | Lista |
|---|---|---|
| Desligar o acesso (`[remote] enabled = false`, ação `remote.toggle`) | `goodbye { remote-disabled }` a todos, conexão com o relay fechada, runtime destruído | intacta |
| Desconectar um aparelho (indicador) | `goodbye { normal }` e canal fechado; o aparelho pode reconectar | intacta |
| Desconectar todos (`remote.disconnect_all`) | idem, para todos | intacta |
| Revogar um aparelho | `goodbye { revoked }`, canal fechado, entrada removida | sem o aparelho |
| Recriar identidade | todos desconectados, chaves novas | vazia |

Todos acontecem **antes** do próximo quadro: o comando vai a `porecatu-remote` por `try_send`, e a UI já reflete o estado pedido sem esperar a rede.

### 7. Token do operador

Digitado pelo usuário na tela de configurações, guardado no cofre por `porecatu-remote`. Token recusado pelo relay (HTTP 401) para de tentar e mostra o motivo; não entra no ciclo de reconexão, que é para queda de rede.

## Alternativas consideradas

### Permissão por aparelho (só leitura / controle total)

Recomendada como opção e recusada pelo dono: acesso é total. Fica registrada como evolução possível, que o protocolo permite sem mudança de handshake.

### Abas marcadas como compartilhadas

Restringiria o que o celular vê. Recusada pelo dono; traria estado novo ao modelo de aba e à sessão.

### Pareamento por código curto digitado

Útil sem câmera, mas exige um PAKE para não reduzir a segurança a 8 caracteres. Fora do v1, por decisão do dono.

### Lista de aparelhos no `porecatu.toml`

Config é o que o usuário escreve; a lista é estado criado por gesto, com chave e data. Pela mesma razão das fixadas, vai num arquivo próprio.

## Consequências

### Positivas

- Parear exige estar na frente do desktop, com a tela de configurações aberta, e confirmar um aparelho identificado.
- Desfazer é sempre um gesto e sempre imediato.
- Nenhum segredo em arquivo.

### Negativas

- Sem cofre do sistema, não há acesso remoto.
- Recriar identidade obriga a parear de novo todos os aparelhos.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Usuário confirmar um aparelho que não é o dele | Baixa | Alto | Botão padrão "Recusar"; nome e impressão digital no diálogo e no celular; aviso a cada conexão; indicador permanente |
| Celular perdido desbloqueado | Baixa | Alto | Revogar pela lista; recriar identidade; bloqueio por biometria no app (decisão do Porecatu Remote) |
| Lista corrompida apagar pareamentos | Baixa | Baixo | Arquivo ruim preservado com sufixo; repareio |
