# ADR-0064 — Janela de configurações modal, possuída pela janela de terminal

**Status:** Aceito
**Data:** 2026-10-17
**Relacionados:** [ADR-0015](0015-multiplas-janelas.md), [ADR-0027](0027-controles-de-janela-e-resize-proprios.md), [PRD-016](../prd/prd-016-tela-de-configuracoes.md)
**Supersedes:** [ADR-0059](0059-janela-de-configuracoes.md) (parcial — §2, só o que ele deixava implícito: a janela de configurações como janela independente do SO, com botão próprio na barra de tarefas e sem bloquear as de terminal)

## Contexto

O ADR-0059 fez da tela de configurações uma janela própria do SO, fora de `App.windows`, e não disse nada sobre a relação dela com as janelas de terminal. O resultado no Windows foi uma janela de primeiro nível como qualquer outra: um segundo botão "Porecatu" na barra de tarefas, como se houvesse dois apps abertos, e as janelas de terminal continuando a aceitar entrada por trás dela. O dono do produto pediu que ela seja **modal** e **pertença** à janela de onde foi aberta.

## Decisão

### 1. No Windows, a janela de origem é a dona

A janela de configurações é criada com `with_owner_window` apontando para a janela de terminal que disparou `settings.open`. Janela possuída não ganha botão na barra de tarefas, fica sempre na frente da dona e some e volta com ela.

### 2. Modal para o app inteiro

Enquanto ela está aberta, **todas** as janelas de terminal ficam desabilitadas (`set_enable(false)`, o `EnableWindow` do SO): clicar numa delas pisca a de configurações em vez de agir. Fechar a de configurações reabilita todas **antes** de destruí-la — com a dona ainda desabilitada, o Windows passaria o foco para outro programa.

Todas, e não só a dona: com outra janela de terminal ativa, ela poderia ser fechada (ou o app encerrado) por baixo, e a janela possuída morreria junto com a dona sem o diálogo de pendências do ADR-0059 §2.

> **Revisto pelo [ADR-0080](0080-editor-de-perfil-em-janela-propria.md) — decidido, a implementar.** A modalidade encadeia: com o editor de perfil aberto, ele é possuído pela janela de configurações e a desabilita do mesmo jeito; fechar o editor reabilita a de configurações **antes** de destruí-lo, pelo mesmo motivo. As janelas de terminal continuam desabilitadas pela de configurações. Fora do Windows, a modalidade do editor é lógica (o input da tela é descartado).

### 3. Minimizar minimiza as janelas de terminal

O botão de minimizar do cabeçalho da tela continua desenhado (a aparência não muda) e passa a minimizar as janelas de terminal; a possuída some com elas e volta quando a dona é restaurada pela barra de tarefas. Minimizar só ela a deixaria sem botão na barra de tarefas e com as janelas de terminal desabilitadas — o app pareceria travado. O minimizar do menu do sistema fica desligado pelo mesmo motivo.

### 4. Só Windows

O `winit` não expõe dona nem desabilitar janela nas outras plataformas de forma equivalente; nelas o comportamento do ADR-0059 continua.

> *Revisto em 2026-10-21*, a pedido do dono do produto (uso em i3): no **X11**, a janela de configurações passa a ser criada com `_NET_WM_WINDOW_TYPE_DIALOG` (`WindowAttributesExtX11::with_x11_window_type`, API segura do `winit`), sempre, sem detectar o gerenciador. Gerenciador tiling flutua janela de diálogo por conta própria, então ela sobrepõe a janela de terminal em vez de dividir a tela com ela. Continua **sem** dona e **sem** modal ali — o `winit` 0.30 não define `WM_TRANSIENT_FOR`. No **Wayland** nada muda: o `winit` não expõe janela pai, e o guia do usuário traz a regra manual do sway pelo título. Verificação: só compilação e clippy no CI Linux; o teste ao vivo no i3 fica com o dono do produto.

## Consequências

### Positivas

- Um botão só na barra de tarefas; a tela de configurações se comporta como o diálogo de configurações de qualquer app do Windows.
- Nenhuma ação de terminal acontece por trás de uma alteração pendente.

### Negativas

- O encerramento adiado do ADR-0059 §2 (fechar a última janela de terminal com pendências) fica inalcançável pela interface no Windows: as janelas de terminal não recebem o clique. O código continua, e vale nas outras plataformas.
- Não dá para olhar o terminal (rolar, copiar um valor) com a tela aberta.
