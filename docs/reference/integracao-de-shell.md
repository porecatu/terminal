# Integração de shell — OSC 7

Fonte única dos trechos que o Porecatu oferece no convite do RF-3.1
([ADR-0039](../adr/0039-convite-a-integracao-de-shell.md)). O binário **embute
os snippets a partir deste arquivo**: corrigir aqui corrige o que o usuário vê
na tela.

## Por que isso existe

O Porecatu restaura cada aba no diretório onde ela estava
([PRD-003](../prd/prd-003-persistencia-de-sessao.md)). Para saber qual é esse
diretório, ele escuta a sequência **OSC 7**, que o shell emite a cada mudança de
diretório:

```
ESC ] 7 ; file://<host>/<caminho> ESC \
```

É a única fonte confiável, e funciona igual nas três plataformas
([ADR-0005](../adr/0005-persistencia-de-sessao.md)). Sem ela:

| Plataforma | O que acontece |
|---|---|
| Linux | diretório correto, por um caminho mais caro (`sysinfo`, [ADR-0038](../adr/0038-fallbacks-de-cwd.md)) |
| macOS | idem |
| **Windows** | **o diretório não é restaurado** — a aba volta no diretório em que foi aberta |

No Windows não há alternativa: ler o diretório de outro processo exige a leitura
do PEB por API não documentada, que o ADR-0005 rejeitou (sensível a 32 vs 64
bits, quebra entre versões e dispara heurística de antivírus). Por isso o convite
é mais insistente lá — não é melhoria, é a condição do recurso.

## Forma do URI

O caminho vai como URI `file://`. O host é ignorado pelo Porecatu, então
`file:///caminho` (com o host vazio) serve.

- **Unix:** `file:///home/ana/projeto`
- **Windows:** `file:///C:/Users/ana/projeto` — barras **normais**, não invertidas.
- Caracteres fora de `A-Za-z0-9/._-~` devem ir percent-encodados (`%20` para
  espaço). O Porecatu decodifica `%XX`; sem encodar, um caminho com espaço ainda
  funciona, mas um com `%` literal não.

---

## bash

```bash
__porecatu_osc7() {
    printf '\033]7;file://%s%s\033\\' "${HOSTNAME}" "${PWD}"
}
PROMPT_COMMAND="__porecatu_osc7${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
```

Em `~/.bashrc`. A forma acima **preserva** um `PROMPT_COMMAND` que já exista.

Muitas distribuições já trazem `/etc/profile.d/vte.sh`, que faz o mesmo com
percent-encoding completo. Se `__vte_osc7` já existir na sua sessão, não há nada
a fazer.

## zsh

```zsh
__porecatu_osc7() {
    printf '\033]7;file://%s%s\033\\' "${HOST}" "${PWD}"
}
autoload -Uz add-zsh-hook
add-zsh-hook precmd __porecatu_osc7
```

Em `~/.zshrc`.

## fish

Versões recentes do fish emitem OSC 7 sozinhas. Confira antes de configurar:
se o diretório já é restaurado corretamente, não acrescente nada. Se não for:

```fish
function __porecatu_osc7 --on-variable PWD
    printf '\033]7;file://%s%s\033\\' (hostname) "$PWD"
end
```

Em `~/.config/fish/config.fish`.

## PowerShell (5.1 e 7)

```powershell
function prompt {
    $path = (Get-Location).ProviderPath
    $uri  = 'file:///' + $path.Replace('\', '/')
    $esc  = [char]27
    Write-Host -NoNewline "$esc]7;$uri$esc\"
    "PS $path> "
}
```

No arquivo que `$PROFILE` aponta. **Substitua a função `prompt` que você já tem**,
em vez de acrescentar uma segunda — a última definida vence, e uma delas deixaria
de emitir. Se você usa `oh-my-posh` ou `starship`, veja abaixo.

O `.Replace('\', '/')` não é cosmético: sem ele o URI sai com barras invertidas e
não é um `file://` válido.

## starship / oh-my-posh

O **starship** emite OSC 7 por padrão nas três plataformas. Nada a fazer.

O **oh-my-posh** substitui a função `prompt`; a integração vai na configuração
dele, não num `prompt` próprio. Consulte a documentação da versão instalada.

## cmd.exe

**Não há forma confiável.** A variável `PROMPT` do `cmd` não reexpande variáveis
de ambiente a cada prompt, então não há como emitir o diretório atual por ali. Um
usuário de `cmd` no Porecatu tem a estrutura de abas e grupos restaurada, mas não
os diretórios.

Se restaurar diretório importa, use PowerShell — que é também o que o Porecatu
escolhe sozinho quando `pwsh` está instalado e nenhum `[shell]` foi configurado.

---

## Conferindo

Com o snippet aplicado, abra uma aba nova, mude de diretório, feche o Porecatu e
reabra. A aba deve voltar no diretório para onde você mudou, não no de origem.

Se não voltar, e você estiver no Windows: confira que o URI está saindo com
barras normais e com a forma `file:///C:/...`.
